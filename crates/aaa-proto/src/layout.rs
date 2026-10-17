//! workspace layout 트리 조작(순수 함수). 정본은 host가 갖고 TUI는 받은 트리를 칸으로 나눠 그리기만 한다.

use crate::msg::{LayoutNode, PaneDirection, SplitDir, Tab, Workspace};

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

/// `new`가 b 쪽 잎인 나눔(방금 `split`으로 만든 것)의 비율을 정한다
pub fn set_split_ratio(node: &mut LayoutNode, new: &str, value: u16) -> bool {
    match node {
        LayoutNode::Pane { .. } => false,
        LayoutNode::Split { ratio, a, b, .. } => {
            if matches!(&**b, LayoutNode::Pane { pane } if pane == new) {
                *ratio = value;
                return true;
            }
            set_split_ratio(a, new, value) || set_split_ratio(b, new, value)
        }
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

/// pane 가장자리를 `dir` 쪽으로 옮긴다(herdr `resize_focused`, src/layout.rs). 그쪽 가장자리의 가장 가까운 나눔(pane이 a쪽이면
/// 오른쪽·아래, b쪽이면 왼쪽·위 가장자리)을 고르고, 없으면 반대쪽 가장자리의 가장 가까운 나눔을 고른다. 오른쪽·아래면 비율을
/// `amount`(천분율)만큼 늘리고 왼쪽·위면 줄인다. 비율은 100..=900(herdr 0.1..0.9)으로 묶는다. 바뀌었으면 true.
pub fn resize(node: &mut LayoutNode, target: &str, dir: PaneDirection, amount: u16) -> bool {
    // 뿌리에서 target까지 지나는 나눔: (나눔 방향, target이 b쪽인가)
    fn path(n: &LayoutNode, target: &str, out: &mut Vec<(SplitDir, bool)>) -> bool {
        match n {
            LayoutNode::Pane { pane } => pane == target,
            LayoutNode::Split { dir, a, b, .. } => {
                out.push((*dir, false));
                if path(a, target, out) {
                    return true;
                }
                out.last_mut().unwrap().1 = true;
                if path(b, target, out) {
                    return true;
                }
                out.pop();
                false
            }
        }
    }
    let mut steps = Vec::new();
    if !path(node, target, &mut steps) {
        return false;
    }
    let want = match dir {
        PaneDirection::Left | PaneDirection::Right => SplitDir::Horizontal,
        PaneDirection::Up | PaneDirection::Down => SplitDir::Vertical,
    };
    let grows = matches!(dir, PaneDirection::Right | PaneDirection::Down);
    // 오른쪽·아래 가장자리 나눔은 target이 a쪽인 것, 왼쪽·위는 b쪽인 것
    let edge = |in_b: bool, toward_grow: bool| in_b != toward_grow;
    let pick = |toward_grow: bool| {
        steps.iter().enumerate().rev().find(|(_, (d, in_b))| *d == want && edge(*in_b, toward_grow)).map(|(i, _)| i)
    };
    let Some(depth) = pick(grows).or_else(|| pick(!grows)) else { return false };
    let mut n = node;
    for (_, in_b) in &steps[..depth] {
        let LayoutNode::Split { a, b, .. } = n else { return false };
        n = if *in_b { b } else { a };
    }
    let LayoutNode::Split { ratio, .. } = n else { return false };
    let before = *ratio;
    let next = if grows { i32::from(before) + i32::from(amount) } else { i32::from(before) - i32::from(amount) };
    *ratio = next.clamp(100, 900) as u16;
    *ratio != before
}

/// 뿌리에서 `path`(false = a, true = b)로 찾은 나눔의 비율을 정한다(herdr `set_ratio_at`). 비율은 100..=900으로 묶는다.
/// 그 자리에 나눔이 없으면 false.
pub fn set_ratio_at(node: &mut LayoutNode, path: &[bool], value: u16) -> bool {
    let LayoutNode::Split { ratio, a, b, .. } = node else { return false };
    match path.split_first() {
        None => {
            *ratio = value.clamp(100, 900);
            true
        }
        Some((true, rest)) => set_ratio_at(b, rest, value),
        Some((false, rest)) => set_ratio_at(a, rest, value),
    }
}

/// 탭 `from`을 `insert` 앞으로 옮긴다(herdr `Workspace::move_tab`: `insert`는 옮기기 전 차례로 0..=탭 수). 바뀌었으면 true.
pub fn move_tab(tabs: &mut Vec<Tab>, from: usize, insert: usize) -> bool {
    if from >= tabs.len() || insert > tabs.len() {
        return false;
    }
    let to = if from < insert { insert - 1 } else { insert }.min(tabs.len() - 1);
    if from == to {
        return false;
    }
    let t = tabs.remove(from);
    tabs.insert(to, t);
    true
}

/// workspace의 모든 탭에서 pane을 뺀다. 빈 탭은 없앤다. 바뀌었으면 true.
pub fn ws_remove(ws: &mut Workspace, target: &str) -> bool {
    let before = ws.tabs.len();
    let mut changed = false;
    let tabs = std::mem::take(&mut ws.tabs);
    for t in tabs {
        if leaves(&t.root).iter().any(|p| p == target) {
            changed = true;
            // herdr `detach_pane`: 탭의 pane이 바뀌면 zoom을 푼다
            if let Some(root) = remove(t.root, target) {
                ws.tabs.push(Tab { root, zoomed: false, ..t });
            }
        } else {
            ws.tabs.push(t);
        }
    }
    changed || ws.tabs.len() != before
}

/// 두 pane의 자리를 바꾼다(herdr `swap_panes`: 칸과 나눔 비율은 그대로, 잎의 pane만 바꾼다). 둘 다 있으면 true.
pub fn swap(node: &mut LayoutNode, x: &str, y: &str) -> bool {
    let l = leaves(node);
    if x == y || !l.iter().any(|p| p == x) || !l.iter().any(|p| p == y) {
        return false;
    }
    fn walk(n: &mut LayoutNode, x: &str, y: &str) {
        match n {
            LayoutNode::Pane { pane } if pane == x => *pane = y.to_string(),
            LayoutNode::Pane { pane } if pane == y => *pane = x.to_string(),
            LayoutNode::Pane { .. } => {}
            LayoutNode::Split { a, b, .. } => {
                walk(a, x, y);
                walk(b, x, y);
            }
        }
    }
    walk(node, x, y);
    true
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
                Tab { id: "t1".into(), name: "1".into(), root: p("a"), zoomed: false },
                Tab { id: "t2".into(), name: "2".into(), root: p("b"), zoomed: false },
            ],
        };
        assert!(ws_remove(&mut ws, "a"));
        assert_eq!(ws.tabs.len(), 1);
        // pane이 빠진 탭은 zoom이 풀린다(herdr `detach_pane`)
        split(&mut ws.tabs[0].root, "b", "c", SplitDir::Horizontal);
        ws.tabs[0].zoomed = true;
        assert!(ws_remove(&mut ws, "c"));
        assert!(!ws.tabs[0].zoomed);
        assert!(!ws_remove(&mut ws, "zz"));
        assert!(ws_replace(&mut ws, "b", "b2"));
        assert!(ws_contains(&ws, "b2"));
    }

    /// herdr `swap_panes_exchanges_leaf_ids_without_changing_cells`
    #[test]
    fn swap_exchanges_leaves_and_keeps_cells() {
        // a | (b / c), 비율 300
        let mut root = p("a");
        split(&mut root, "a", "b", SplitDir::Horizontal);
        split(&mut root, "b", "c", SplitDir::Vertical);
        set_split_ratio(&mut root, "b", 300);
        let area = Rect { x: 0, y: 0, w: 81, h: 21 };
        let before: Vec<Rect> = rects(&root, area).0.into_iter().map(|(_, r)| r).collect();
        assert!(swap(&mut root, "a", "c"));
        assert_eq!(leaves(&root), ["c", "b", "a"]);
        let after = rects(&root, area);
        assert_eq!(after.0.iter().map(|(_, r)| *r).collect::<Vec<_>>(), before);
        assert!(!swap(&mut root, "a", "a") && !swap(&mut root, "a", "zz"));
        assert_eq!(leaves(&root), ["c", "b", "a"]);
    }

    /// herdr `layout_set_split_ratio_updates_existing_split`·`move_tab`
    #[test]
    fn ratio_at_path_and_move_tab_like_herdr() {
        // a | (b / c)
        let mut root = p("a");
        split(&mut root, "a", "b", SplitDir::Horizontal);
        split(&mut root, "b", "c", SplitDir::Vertical);
        assert!(set_ratio_at(&mut root, &[true], 950));
        assert!(set_ratio_at(&mut root, &[], 600));
        let LayoutNode::Split { ratio, b, .. } = &root else { panic!() };
        assert_eq!(*ratio, 600);
        assert!(matches!(&**b, LayoutNode::Split { ratio: 900, .. }));
        assert!(!set_ratio_at(&mut root, &[false], 300) && !set_ratio_at(&mut root, &[true, true], 300));

        let tab = |id: &str| Tab { id: id.into(), name: String::new(), root: p(id), zoomed: false };
        let mut tabs: Vec<Tab> = ["1", "2", "3"].map(tab).into();
        let ids = |t: &[Tab]| t.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert!(move_tab(&mut tabs, 0, 3));
        assert_eq!(ids(&tabs), ["2", "3", "1"]);
        assert!(move_tab(&mut tabs, 2, 0));
        assert_eq!(ids(&tabs), ["1", "2", "3"]);
        assert!(move_tab(&mut tabs, 0, 2));
        assert_eq!(ids(&tabs), ["2", "1", "3"]);
        // 제자리(앞이나 바로 뒤에 넣기)·밖은 그대로
        assert!(
            !move_tab(&mut tabs, 1, 1)
                && !move_tab(&mut tabs, 1, 2)
                && !move_tab(&mut tabs, 3, 0)
                && !move_tab(&mut tabs, 0, 4)
        );
    }

    #[test]
    fn resize_moves_the_nearest_edge_like_herdr() {
        // a | (b / c)
        let mut root = p("a");
        split(&mut root, "a", "b", SplitDir::Horizontal);
        split(&mut root, "b", "c", SplitDir::Vertical);
        let ratio = |n: &LayoutNode| match n {
            LayoutNode::Split { ratio, b, .. } => (
                *ratio,
                match &**b {
                    LayoutNode::Split { ratio, .. } => *ratio,
                    _ => 0,
                },
            ),
            _ => (0, 0),
        };
        // a 오른쪽 가장자리를 오른쪽으로: 바깥 나눔 +50
        assert!(resize(&mut root, "a", PaneDirection::Right, 50));
        assert_eq!(ratio(&root), (550, 500));
        // a 왼쪽 가장자리는 없다: 반대쪽(오른쪽) 나눔을 왼쪽으로 옮긴다(줄어든다)
        assert!(resize(&mut root, "a", PaneDirection::Left, 50));
        assert_eq!(ratio(&root), (500, 500));
        // b 아래 가장자리를 아래로: 안쪽 나눔 +
        assert!(resize(&mut root, "b", PaneDirection::Down, 100));
        assert_eq!(ratio(&root), (500, 600));
        // c 위 가장자리를 위로: 같은 안쪽 나눔 -
        assert!(resize(&mut root, "c", PaneDirection::Up, 100));
        assert_eq!(ratio(&root), (500, 500));
        // 비율은 100..=900
        assert!(resize(&mut root, "c", PaneDirection::Left, 500));
        assert_eq!(ratio(&root), (100, 500));
        assert!(!resize(&mut root, "c", PaneDirection::Left, 500));
        // 그 방향 나눔이 없거나 pane이 없으면 그대로
        let mut one = p("a");
        assert!(!resize(&mut one, "a", PaneDirection::Right, 50));
        assert!(!resize(&mut root, "zz", PaneDirection::Right, 50));
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
