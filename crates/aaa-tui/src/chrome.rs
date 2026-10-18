//! pane 테두리와 제목. herdr 0.9.5 `src/ui/panes.rs` `apply_pane_chrome`(기본 `pane_borders = "auto"`·`pane_gaps`·
//! `pane_outer_borders`)·`zoomed_pane_borders`·`render_pane_borders`·`add_pane_border_cells`·`line_touches_pane`·
//! `line_cell_symbol`·`render_pane_border_titles`·`pane_border_title`, `src/layout.rs` `split_rect`,
//! `src/ui/text.rs` `truncate_end`를 옮겼다.
//!
//! - `auto`(기본): pane이 둘 이상인 탭(zoom이어도)은 pane마다 온 테두리 상자를 두른다. 상자끼리는 붙는다(나눔 칸 없음).
//! - `always`: pane 하나여도 두른다. `off`: 테두리 없이 오른쪽·아래에 이웃이 있으면 빈 칸 하나를 띄운다(선은 없다).
//! - PTY 크기는 테두리 안쪽 칸이다.
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

use std::collections::HashMap;

use aaa_config::PaneBorders;
use aaa_proto::layout::{self, Rect};
use aaa_proto::{LayoutNode, SplitDir};
use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::theme::Palette;
use crate::unicode_width;

/// 그릴 pane 하나: 바깥 칸(테두리 포함), 안쪽 칸(PTY), 테두리를 두르나
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneBox {
    pub pane: String,
    pub rect: Rect,
    /// PTY 칸(테두리·scrollbar 칸 안쪽)
    pub inner: Rect,
    pub bordered: bool,
    /// scrollbar 칸(herdr `stable_scrollbar_gutter`: 보통 화면 pane의 오른쪽 1칸)
    pub gutter: Option<Rect>,
}

/// 탭 layout을 `area`에 놓는다(herdr `apply_pane_chrome`). `zoomed`면 그 pane 하나가 칸 전체다(`zoomed_pane_borders`).
pub fn layout(root: &LayoutNode, area: Rect, mode: PaneBorders, zoomed: Option<&str>) -> Vec<PaneBox> {
    let multi = layout::leaves(root).len() > 1;
    let shows = match mode {
        PaneBorders::Off => false,
        PaneBorders::Auto => multi,
        PaneBorders::Always => true,
    };
    if let Some(z) = zoomed {
        return vec![pane_box(z.to_string(), area, shows)];
    }
    let mut tiles = Vec::new();
    tile(root, area, &mut tiles);
    let mut out: Vec<PaneBox> = tiles.iter().map(|(p, r)| pane_box(p.clone(), *r, shows)).collect();
    if mode == PaneBorders::Off && multi {
        // herdr `pane_gaps`: 오른쪽·아래에 맞닿은 이웃이 있으면 한 칸 줄인다
        for (b, (_, r)) in out.iter_mut().zip(&tiles) {
            let touches = |o: &Rect, right: bool| {
                if right {
                    o.x == r.x + r.w && o.y < r.y + r.h && o.y + o.h > r.y
                } else {
                    o.y == r.y + r.h && o.x < r.x + r.w && o.x + o.w > r.x
                }
            };
            if r.w > 1 && tiles.iter().any(|(_, o)| touches(o, true)) {
                b.rect.w -= 1;
            }
            if r.h > 1 && tiles.iter().any(|(_, o)| touches(o, false)) {
                b.rect.h -= 1;
            }
            b.inner = b.rect;
        }
    }
    out
}

fn pane_box(pane: String, rect: Rect, bordered: bool) -> PaneBox {
    let inner = if bordered && rect.w >= 2 && rect.h >= 2 {
        Rect { x: rect.x + 1, y: rect.y + 1, w: rect.w - 2, h: rect.h - 2 }
    } else {
        rect
    };
    PaneBox { pane, rect, inner, bordered: bordered && rect.w >= 2 && rect.h >= 2, gutter: None }
}

/// herdr `split_rect`: 나눔 칸 없이 첫 쪽은 `round(길이 × 비율)`. 나눌 수 없으면(1칸) None(첫 쪽만 그린다).
fn cut(dir: SplitDir, ratio: u16, r: Rect) -> Option<(Rect, Rect)> {
    let total = if dir == SplitDir::Horizontal { r.w } else { r.h };
    if total < 2 {
        return None;
    }
    let first =
        ((u32::from(total) * u32::from(ratio.clamp(1, 999)) + 500) / 1000).clamp(1, u32::from(total - 1)) as u16;
    Some(match dir {
        SplitDir::Horizontal => (Rect { w: first, ..r }, Rect { x: r.x + first, w: total - first, ..r }),
        SplitDir::Vertical => (Rect { h: first, ..r }, Rect { y: r.y + first, h: total - first, ..r }),
    })
}

fn tile(n: &LayoutNode, r: Rect, out: &mut Vec<(String, Rect)>) {
    match n {
        LayoutNode::Pane { pane } => out.push((pane.clone(), r)),
        LayoutNode::Split { dir, ratio, a, b } => match cut(*dir, *ratio, r) {
            Some((ra, rb)) => {
                tile(a, ra, out);
                tile(b, rb, out);
            }
            None => tile(a, r, out),
        },
    }
}

/// 끌어 비율을 바꿀 나눔(herdr `SplitBorder`): 뿌리에서의 길(false = a), 나눔 칸, b 쪽 시작 자리, 누를 칸
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitHit {
    pub path: Vec<bool>,
    pub dir: SplitDir,
    pub area: Rect,
    pub pos: u16,
    pub hit: Rect,
}

/// 나눔 경계의 누를 칸(herdr `collect_splits`·`split_hit_rect`). 바깥 나눔이 앞에 온다. 테두리가 있으면 맞닿은 두 선
/// (`pos-1`·`pos`), `off`면 빈 칸(`pos-1`, pane 칸과 겹치면 뺀다). zoom이면 없다.
pub fn splits(root: &LayoutNode, area: Rect, boxes: &[PaneBox], zoomed: bool) -> Vec<SplitHit> {
    fn walk(n: &LayoutNode, r: Rect, path: &mut Vec<bool>, out: &mut Vec<(Vec<bool>, SplitDir, Rect, u16)>) {
        let LayoutNode::Split { dir, ratio, a, b } = n else { return };
        let Some((ra, rb)) = cut(*dir, *ratio, r) else { return };
        out.push((path.clone(), *dir, r, if *dir == SplitDir::Horizontal { rb.x } else { rb.y }));
        path.push(false);
        walk(a, ra, path, out);
        path.pop();
        path.push(true);
        walk(b, rb, path, out);
        path.pop();
    }
    if zoomed {
        return Vec::new();
    }
    let mut found = Vec::new();
    walk(root, area, &mut Vec::new(), &mut found);
    let bordered = boxes.iter().any(|b| b.bordered);
    let overlaps = |h: &Rect, o: &Rect| h.x < o.x + o.w && h.x + h.w > o.x && h.y < o.y + o.h && h.y + h.h > o.y;
    found
        .into_iter()
        .filter_map(|(path, dir, area, pos)| {
            let (start, len) = if bordered { (pos - 1, 2) } else { (pos - 1, 1) };
            let hit = match dir {
                SplitDir::Horizontal => Rect { x: start, w: len, ..area },
                SplitDir::Vertical => Rect { y: start, h: len, ..area },
            };
            (bordered || !boxes.iter().any(|b| overlaps(&hit, &b.rect))).then_some(SplitHit {
                path,
                dir,
                area,
                pos,
                hit,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Default)]
struct LineCell {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

/// herdr `line_cell_symbol`
fn symbol(c: LineCell) -> &'static str {
    match (c.up, c.down, c.left, c.right) {
        (true, true, true, true) => "┼",
        (true, true, true, false) => "┤",
        (true, true, false, true) => "├",
        (true, false, true, true) => "┴",
        (false, true, true, true) => "┬",
        (true, true, false, false) | (true, false, false, false) | (false, true, false, false) => "│",
        (false, false, true, true) | (false, false, true, false) | (false, false, false, true) => "─",
        (false, true, false, true) => "┌",
        (false, true, true, false) => "┐",
        (true, false, false, true) => "└",
        (true, false, true, false) => "┘",
        _ => "",
    }
}

/// herdr `line_touches_pane`(`pane_gaps`): 그 pane 자기 테두리 칸인가
fn own_border(x: u16, y: u16, r: Rect) -> bool {
    if r.w == 0 || r.h == 0 {
        return false;
    }
    let (right, bottom) = (r.x + r.w - 1, r.y + r.h - 1);
    let in_rows = y >= r.y && y <= bottom;
    let in_cols = x >= r.x && x <= right;
    (in_rows && (x == r.x || x == right)) || (in_cols && (y == r.y || y == bottom))
}

/// 테두리(포커스 pane에 닿은 칸은 accent, 나머지 overlay0)와 위 테두리의 제목(` 이름 `, 포커스면 accent 굵게)을 그린다.
pub fn render(
    buf: &mut Buffer,
    boxes: &[PaneBox],
    focus: Option<&str>,
    title: impl Fn(&str) -> Option<String>,
    p: &Palette,
) {
    let focused = boxes.iter().find(|b| Some(b.pane.as_str()) == focus).map(|b| b.rect);
    let mut cells: HashMap<(u16, u16), LineCell> = HashMap::new();
    for b in boxes.iter().filter(|b| b.bordered) {
        let r = b.rect;
        let (right, bottom) = (r.x + r.w - 1, r.y + r.h - 1);
        for x in r.x..=right {
            for y in [r.y, bottom] {
                let cell = cells.entry((x, y)).or_default();
                cell.left |= x > r.x;
                cell.right |= x < right;
            }
        }
        for y in r.y..=bottom {
            for x in [r.x, right] {
                let cell = cells.entry((x, y)).or_default();
                cell.up |= y > r.y;
                cell.down |= y < bottom;
            }
        }
    }
    let area = buf.area;
    for ((x, y), cell) in cells {
        let s = symbol(cell);
        if s.is_empty() || x >= area.x + area.width || y >= area.y + area.height {
            continue;
        }
        let near = focused.is_some_and(|f| own_border(x, y, f));
        buf.set_string(x, y, s, Style::default().fg(if near { p.accent } else { p.overlay0 }));
    }
    for b in boxes.iter().filter(|b| b.bordered && b.rect.w > 4) {
        let Some(label) = title(&b.pane) else { continue };
        let label = label.trim();
        if label.is_empty() {
            continue;
        }
        let t = format!(" {} ", truncate_end(label, usize::from(b.rect.w - 4)));
        let is_focus = Some(b.pane.as_str()) == focus;
        let st = if is_focus {
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.overlay0)
        };
        buf.set_stringn(b.rect.x + 1, b.rect.y, t, usize::from(b.rect.w - 2), st);
    }
}

/// herdr `truncate_end`: 폭을 넘으면 잘라 `…`
pub fn truncate_end(text: &str, max: usize) -> String {
    if unicode_width(text) <= max {
        return text.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in text.chars() {
        let cw = unicode_width(&c.to_string());
        if w + cw > max - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    fn split(dir: SplitDir, ratio: u16, a: LayoutNode, b: LayoutNode) -> LayoutNode {
        LayoutNode::Split { dir, ratio, a: Box::new(a), b: Box::new(b) }
    }

    fn pane(p: &str) -> LayoutNode {
        LayoutNode::Pane { pane: p.into() }
    }

    /// herdr `split_hit_rect`: 테두리면 맞닿은 두 선, off면 빈 칸, 바깥 나눔이 먼저, zoom이면 없다
    #[test]
    fn split_hits_like_herdr() {
        let t = split(SplitDir::Horizontal, 500, pane("a"), split(SplitDir::Vertical, 500, pane("b"), pane("c")));
        let area = r(26, 1, 64, 29);
        let boxes = layout(&t, area, PaneBorders::Auto, None);
        let s = splits(&t, area, &boxes, false);
        assert_eq!(s.len(), 2);
        assert_eq!(
            (s[0].path.clone(), s[0].dir, s[0].area, s[0].pos, s[0].hit),
            (vec![], SplitDir::Horizontal, area, 58, r(57, 1, 2, 29))
        );
        assert_eq!(
            (s[1].path.clone(), s[1].area, s[1].pos, s[1].hit),
            (vec![true], r(58, 1, 32, 29), 16, r(58, 15, 32, 2))
        );
        let boxes = layout(&t, area, PaneBorders::Off, None);
        let s = splits(&t, area, &boxes, false);
        assert_eq!((s[0].hit, s[1].hit), (r(57, 1, 1, 29), r(58, 15, 32, 1)));
        assert!(splits(&t, area, &boxes, true).is_empty());
    }

    /// herdr 기본(auto): 하나면 테두리 없음, 둘 이상이면 pane마다 상자, 나눔 칸 없이 반올림
    #[test]
    fn auto_borders_like_herdr() {
        let one = pane("a");
        let b = layout(&one, r(26, 1, 64, 29), PaneBorders::Auto, None);
        assert_eq!(
            b,
            vec![PaneBox {
                pane: "a".into(),
                rect: r(26, 1, 64, 29),
                inner: r(26, 1, 64, 29),
                bordered: false,
                gutter: None
            }]
        );
        let two = split(SplitDir::Horizontal, 500, pane("a"), pane("b"));
        let b = layout(&two, r(26, 1, 64, 29), PaneBorders::Auto, None);
        assert_eq!(b[0].rect, r(26, 1, 32, 29));
        assert_eq!(b[0].inner, r(27, 2, 30, 27));
        assert_eq!(b[1].rect, r(58, 1, 32, 29));
        // 홀수 폭은 첫 쪽이 반올림(herdr split_rect)
        let b = layout(&two, r(0, 0, 65, 10), PaneBorders::Auto, None);
        assert_eq!((b[0].rect.w, b[1].rect.w), (33, 32));
        // zoom이면 그 pane 하나가 칸 전체, 다른 pane이 있으니 테두리
        let b = layout(&two, r(26, 1, 64, 29), PaneBorders::Auto, Some("b"));
        assert_eq!(
            b,
            vec![PaneBox {
                pane: "b".into(),
                rect: r(26, 1, 64, 29),
                inner: r(27, 2, 62, 27),
                bordered: true,
                gutter: None
            }]
        );
        // always는 하나여도, off는 이웃 쪽으로 빈 칸 하나(선 없음)
        assert!(layout(&one, r(0, 0, 10, 5), PaneBorders::Always, None)[0].bordered);
        let b = layout(&two, r(0, 0, 65, 10), PaneBorders::Off, None);
        assert_eq!((b[0].rect, b[0].inner, b[1].rect), (r(0, 0, 32, 10), r(0, 0, 32, 10), r(33, 0, 32, 10)));
        let four = split(SplitDir::Vertical, 500, two.clone(), pane("c"));
        let b = layout(&four, r(0, 0, 20, 10), PaneBorders::Off, None);
        assert_eq!(b.iter().map(|x| x.rect).collect::<Vec<_>>(), vec![r(0, 0, 9, 4), r(10, 0, 10, 4), r(0, 5, 20, 5)]);
    }

    /// 상자가 붙으면 `┐┌`, 포커스 상자만 accent, 제목은 위 테두리 x+1에 ` 이름 `(폭-4로 자름, 포커스면 accent 굵게)
    #[test]
    fn borders_and_titles_like_herdr() {
        let p = Palette::catppuccin();
        let two = split(SplitDir::Horizontal, 500, pane("a"), pane("b"));
        let boxes = layout(&two, r(0, 0, 20, 5), PaneBorders::Auto, None);
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 20, 5));
        render(&mut buf, &boxes, Some("b"), |id| (id == "b").then(|| "로그 보기".to_string()), &p);
        // 넓은 글자 뒤 칸은 건너뛴다(터미널에 보이는 대로)
        let line = |y: u16| {
            let (mut out, mut x) = (String::new(), 0);
            while x < 20 {
                let sym = buf[(x, y)].symbol();
                out.push_str(sym);
                x += (unicode_width(sym) as u16).max(1);
            }
            out
        };
        assert_eq!(line(0), "┌────────┐┌ 로그 … ┐");
        assert_eq!(line(2), "│        ││        │");
        assert_eq!(line(4), "└────────┘└────────┘");
        assert_eq!(buf[(9, 2)].fg, p.overlay0);
        assert_eq!(buf[(10, 2)].fg, p.accent);
        assert_eq!(buf[(12, 0)].fg, p.accent);
        assert!(buf[(12, 0)].modifier.contains(Modifier::BOLD));
        assert_eq!(truncate_end("빌드 서버 로그", 7), "빌드 …");
        assert_eq!(truncate_end("abc", 3), "abc");
    }
}
