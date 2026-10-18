//! pane scrollbar(herdr 0.9.5 `src/ui/scrollbar.rs`). 기록이 있는 보통 화면 pane의 오른쪽 1칸(gutter)에 그린다.
//! thumb 길이 = 화면 줄 × 트랙 / (기록 + 화면 줄)(반올림, 1 이상), thumb 위치는 맨 위에서 내려온 비율.
//! 누르면 thumb 가운데가 그 줄에 오도록 뛰고, thumb를 잡아 끌면 잡은 자리를 지키며 옮긴다.
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

use aaa_proto::GridScroll;
use aaa_proto::layout::Rect;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};

use crate::theme::Palette;

/// 기록 위치와 화면 줄 수(herdr `ScrollMetrics`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Metrics {
    pub offset: usize,
    pub max: usize,
    pub rows: usize,
}

impl Metrics {
    pub fn new(scroll: GridScroll, rows: u16) -> Self {
        Metrics { offset: scroll.offset as usize, max: scroll.max as usize, rows: rows as usize }
    }
}

/// (thumb 맨 위 줄, 길이). 기록이 없으면 None(herdr `scrollbar_thumb`)
pub fn thumb(m: Metrics, track: Rect) -> Option<(u16, u16)> {
    if m.max == 0 || track.h == 0 {
        return None;
    }
    let h = track.h as usize;
    let total = m.max + m.rows;
    let len = ((m.rows * h) as f32 / total as f32).round().max(1.0).min(h as f32) as usize;
    let max_top = h.saturating_sub(len);
    let from_top = m.max.saturating_sub(m.offset);
    let top = if max_top == 0 {
        0
    } else {
        ((from_top * max_top) as f32 / m.max as f32).round().clamp(0.0, max_top as f32) as usize
    };
    Some((track.y + top as u16, len as u16))
}

/// thumb 위를 눌렀으면 thumb 안에서 몇째 줄인가(herdr `scrollbar_thumb_grab_offset`)
pub fn grab(m: Metrics, track: Rect, row: u16) -> Option<u16> {
    let (top, len) = thumb(m, track)?;
    (row >= top && row < top + len).then(|| row - top)
}

fn offset_from_thumb_top(m: Metrics, track: Rect, top: usize) -> usize {
    if m.max == 0 {
        return 0;
    }
    let len = thumb(m, track).map(|(_, l)| l as usize).unwrap_or(1);
    let max_top = track.h as usize - len.min(track.h as usize);
    if max_top == 0 {
        return 0;
    }
    let from_top = ((top.min(max_top) * m.max) as f32 / max_top as f32).round() as usize;
    m.max.saturating_sub(from_top)
}

/// 트랙을 눌렀을 때: thumb 가운데가 그 줄에 오는 기록 위치(herdr `scrollbar_offset_from_row`)
pub fn offset_from_row(m: Metrics, track: Rect, row: u16) -> usize {
    let Some((_, len)) = thumb(m, track) else { return 0 };
    let row = row.clamp(track.y, track.y + track.h.saturating_sub(1));
    let top = ((row - track.y) as usize).saturating_sub(len as usize / 2);
    offset_from_thumb_top(m, track, top)
}

/// thumb를 `grab`째 줄로 잡고 끌 때의 기록 위치(herdr `scrollbar_offset_from_drag_row`)
pub fn offset_from_drag_row(m: Metrics, track: Rect, row: u16, grab: u16) -> usize {
    let row = row.clamp(track.y, track.y + track.h.saturating_sub(1));
    let top = ((row - track.y) as usize).saturating_sub(grab as usize);
    offset_from_thumb_top(m, track, top)
}

/// 트랙 `▕`, thumb 포커스 `▐`(overlay1)·아니면 `▕`(overlay0), 트랙 색 포커스 overlay0·아니면 surface_dim
/// (herdr `render_pane_scrollbar_buffer`)
pub fn render(buf: &mut Buffer, m: Metrics, track: Rect, p: &Palette, focused: bool) {
    let Some((top, len)) = thumb(m, track) else { return };
    let (track_color, thumb_color, thumb_symbol): (Color, Color, &str) =
        if focused { (p.overlay0, p.overlay1, "▐") } else { (p.surface_dim, p.overlay0, "▕") };
    for y in track.y..track.y + track.h {
        let c = &mut buf[(track.x, y)];
        c.set_symbol("▕");
        c.set_style(Style::default().fg(track_color));
    }
    for y in top..top + len {
        let c = &mut buf[(track.x, y)];
        c.set_symbol(thumb_symbol);
        c.set_style(Style::default().fg(thumb_color));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// herdr `scrollbar_thumb`·`scrollbar_offset_from_row`·`_drag_row`(시험 `tests/copy.rs` 탐색 scrollbar와 같은 차례):
    /// 맨 아래 줄을 누르면 맨 아래, 맨 위를 누르면 맨 위, thumb를 잡기만 하면 그대로, 아래로 끝까지 끌면 맨 아래
    #[test]
    fn thumb_and_offsets_like_herdr() {
        let track = Rect { x: 79, y: 1, w: 1, h: 20 };
        let bottom = Metrics { offset: 0, max: 180, rows: 20 };
        // 20 × 20 / 200 = 2줄, 맨 아래면 트랙 끝
        assert_eq!(thumb(bottom, track), Some((19, 2)));
        assert_eq!(thumb(Metrics { offset: 180, ..bottom }, track), Some((1, 2)));
        assert_eq!(thumb(Metrics { offset: 90, ..bottom }, track), Some((10, 2)));
        assert_eq!(thumb(Metrics { max: 0, ..bottom }, track), None);
        // 기록이 화면보다 짧아도 thumb는 1줄 이상, 트랙보다 길지 않다
        assert_eq!(thumb(Metrics { offset: 0, max: 1, rows: 20 }, track), Some((2, 19)));
        assert_eq!(thumb(Metrics { offset: 0, max: 5000, rows: 20 }, track), Some((20, 1)));

        assert_eq!(offset_from_row(bottom, track, 20), 0);
        assert_eq!(offset_from_row(bottom, track, 1), 180);
        assert_eq!(offset_from_row(bottom, track, 0), 180);
        let top = Metrics { offset: 180, ..bottom };
        let g = grab(top, track, 2).unwrap();
        assert_eq!(g, 1);
        assert_eq!(offset_from_drag_row(top, track, 2, g), 180);
        assert_eq!(offset_from_drag_row(top, track, 25, g), 0);
        assert_eq!(grab(top, track, 5), None);

        let p = Palette::catppuccin();
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 80, 22));
        render(&mut buf, Metrics { offset: 90, ..bottom }, track, &p, true);
        assert_eq!((buf[(79, 1)].symbol(), buf[(79, 1)].fg), ("▕", p.overlay0));
        assert_eq!((buf[(79, 10)].symbol(), buf[(79, 10)].fg), ("▐", p.overlay1));
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 80, 22));
        render(&mut buf, Metrics { max: 0, ..bottom }, track, &p, true);
        assert_eq!(buf[(79, 1)].symbol(), " ");
    }
}
