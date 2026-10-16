//! workspace layout 트리 조작(순수 함수). 정본은 host가 갖고 TUI는 받은 트리를 칸으로 나눠 그리기만 한다.

use aa_proto::{LayoutNode, SplitDir, Tab, Workspace};

pub fn leaves(node: &LayoutNode) -> Vec<String> {
    let mut out = Vec::new();
    fn walk(n: &LayoutNode, out: &mut Vec<String>) {
        match n {
            LayoutNode::Pane { pane } => out.push(pane.clone()),
            LayoutNode::Split { a, b, .. } => {
                walk(a, out);
                walk(b, out);
            }
        }
    }
    walk(node, &mut out);
    out
}

/// `target` 자리를 나눠 `new`를 b 쪽(오른쪽·아래)에 둔다. 찾으면 true.
pub fn split(node: &mut LayoutNode, target: &str, new: &str, dir: SplitDir) -> bool {
    match node {
        LayoutNode::Pane { pane } if pane == target => {
            let old = std::mem::replace(node, LayoutNode::Pane { pane: String::new() });
            *node = LayoutNode::Split {
                dir,
                ratio: 500,
                a: Box::new(old),
                b: Box::new(LayoutNode::Pane { pane: new.to_string() }),
            };
            true
        }
        LayoutNode::Pane { .. } => false,
        LayoutNode::Split { a, b, .. } => split(a, target, new, dir) || split(b, target, new, dir),
    }
}

/// pane을 뺀다. 나뉜 자리는 남은 쪽이 차지한다. 트리가 비면 None.
pub fn remove(node: LayoutNode, target: &str) -> Option<LayoutNode> {
    match node {
        LayoutNode::Pane { ref pane } if pane == target => None,
        LayoutNode::Pane { .. } => Some(node),
        LayoutNode::Split { dir, ratio, a, b } => match (remove(*a, target), remove(*b, target)) {
            (Some(a), Some(b)) => Some(LayoutNode::Split { dir, ratio, a: Box::new(a), b: Box::new(b) }),
            (Some(x), None) | (None, Some(x)) => Some(x),
            (None, None) => None,
        },
    }
}

pub fn replace(node: &mut LayoutNode, old: &str, new: &str) -> bool {
    match node {
        LayoutNode::Pane { pane } if pane == old => {
            *pane = new.to_string();
            true
        }
        LayoutNode::Pane { .. } => false,
        LayoutNode::Split { a, b, .. } => {
            let x = replace(a, old, new);
            let y = replace(b, old, new);
            x || y
        }
    }
}

/// workspace의 모든 탭에서 pane을 뺀다. 빈 탭은 없앤다. 바뀌었으면 true.
pub fn ws_remove(ws: &mut Workspace, target: &str) -> bool {
    let before = ws.tabs.len();
    let mut changed = false;
    let tabs = std::mem::take(&mut ws.tabs);
    for t in tabs {
        if leaves(&t.root).iter().any(|p| p == target) {
            changed = true;
            if let Some(root) = remove(t.root, target) {
                ws.tabs.push(Tab { root, ..t });
            }
        } else {
            ws.tabs.push(t);
        }
    }
    changed || ws.tabs.len() != before
}

pub fn ws_replace(ws: &mut Workspace, old: &str, new: &str) -> bool {
    let mut changed = false;
    for t in &mut ws.tabs {
        changed |= replace(&mut t.root, old, new);
    }
    changed
}

pub fn ws_contains(ws: &Workspace, pane: &str) -> bool {
    ws.tabs.iter().any(|t| leaves(&t.root).iter().any(|p| p == pane))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// pane별 칸과 구분선 칸
pub type Rects = (Vec<(String, Rect)>, Vec<(SplitDir, Rect)>);

/// 칸 나누기. 나뉜 두 칸 사이에 구분선 한 칸을 둔다(구분선 위치도 돌려준다).
pub fn rects(node: &LayoutNode, area: Rect) -> Rects {
    let mut panes = Vec::new();
    let mut lines = Vec::new();
    fn walk(n: &LayoutNode, r: Rect, panes: &mut Vec<(String, Rect)>, lines: &mut Vec<(SplitDir, Rect)>) {
        match n {
            LayoutNode::Pane { pane } => panes.push((pane.clone(), r)),
            LayoutNode::Split { dir, ratio, a, b } => {
                let total = match dir {
                    SplitDir::Horizontal => r.w,
                    SplitDir::Vertical => r.h,
                };
                if total < 3 {
                    // 나눌 자리가 없으면 a만 그린다
                    walk(a, r, panes, lines);
                    return;
                }
                let first = ((u32::from(total - 1) * u32::from((*ratio).clamp(1, 999))) / 1000)
                    .clamp(1, u32::from(total - 2)) as u16;
                let second = total - 1 - first;
                let (ra, line, rb) = match dir {
                    SplitDir::Horizontal => (
                        Rect { w: first, ..r },
                        Rect { x: r.x + first, w: 1, ..r },
                        Rect { x: r.x + first + 1, w: second, ..r },
                    ),
                    SplitDir::Vertical => (
                        Rect { h: first, ..r },
                        Rect { y: r.y + first, h: 1, ..r },
                        Rect { y: r.y + first + 1, h: second, ..r },
                    ),
                };
                lines.push((*dir, line));
                walk(a, ra, panes, lines);
                walk(b, rb, panes, lines);
            }
        }
    }
    walk(node, area, &mut panes, &mut lines);
    (panes, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str) -> LayoutNode {
        LayoutNode::Pane { pane: id.into() }
    }

    #[test]
    fn split_remove_replace() {
        let mut root = p("a");
        assert!(split(&mut root, "a", "b", SplitDir::Horizontal));
        assert!(split(&mut root, "b", "c", SplitDir::Vertical));
        assert!(!split(&mut root, "zz", "d", SplitDir::Vertical));
        assert_eq!(leaves(&root), ["a", "b", "c"]);
        assert!(replace(&mut root, "b", "b2"));
        assert_eq!(leaves(&root), ["a", "b2", "c"]);
        // 가운데를 빼면 남은 쪽이 자리를 차지한다
        let root = remove(root, "b2").unwrap();
        assert_eq!(
            root,
            LayoutNode::Split { dir: SplitDir::Horizontal, ratio: 500, a: Box::new(p("a")), b: Box::new(p("c")) }
        );
        let root = remove(root, "a").unwrap();
        assert_eq!(root, p("c"));
        assert_eq!(remove(root, "c"), None);
    }

    #[test]
    fn workspace_remove_drops_empty_tabs() {
        let mut ws = Workspace {
            name: "main".into(),
            tabs: vec![
                Tab { id: "t1".into(), name: "1".into(), root: p("a") },
                Tab { id: "t2".into(), name: "2".into(), root: p("b") },
            ],
        };
        assert!(ws_remove(&mut ws, "a"));
        assert_eq!(ws.tabs.len(), 1);
        assert!(!ws_remove(&mut ws, "zz"));
        assert!(ws_replace(&mut ws, "b", "b2"));
        assert!(ws_contains(&ws, "b2"));
    }

    #[test]
    fn rects_leave_one_cell_divider_and_cover_area() {
        let mut root = p("a");
        split(&mut root, "a", "b", SplitDir::Horizontal);
        split(&mut root, "b", "c", SplitDir::Vertical);
        let (panes, lines) = rects(&root, Rect { x: 0, y: 1, w: 81, h: 24 });
        assert_eq!(panes[0], ("a".into(), Rect { x: 0, y: 1, w: 40, h: 24 }));
        assert_eq!(lines[0], (SplitDir::Horizontal, Rect { x: 40, y: 1, w: 1, h: 24 }));
        assert_eq!(panes[1], ("b".into(), Rect { x: 41, y: 1, w: 40, h: 11 }));
        assert_eq!(panes[2], ("c".into(), Rect { x: 41, y: 13, w: 40, h: 12 }));
        // 너무 좁으면 나누지 않는다
        let (panes, lines) = rects(&root, Rect { x: 0, y: 0, w: 2, h: 5 });
        assert_eq!((panes.len(), lines.len()), (1, 0));
    }
}
