//! 이름 입력 창과 닫기 확인 창. herdr 0.9.5 `src/client/shell/overlays.rs` `render_rename_overlay`(56×7)·
//! `render_confirm_close_overlay`(64×6)와 `row`·`button`의 모양을 따른다.

use aaa_proto::layout::Rect;
use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::editor::{self, Editor};
use crate::help::{panel, popup};
use crate::theme::{Palette, contrast};
use crate::unicode_width;

/// 이름 입력 창이 무엇을 바꾸나
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameTarget {
    /// 새 탭(기본 이름 그대로면 자동 번호)
    NewTab { default: String },
    /// 탭 이름(자동 이름을 그대로 두면 바꾸지 않는다)
    Tab { id: String, auto: bool, original: String },
}

#[derive(Debug, Clone)]
pub struct Rename {
    pub title: &'static str,
    pub input: Editor,
    pub target: RenameTarget,
}

/// 닫기 확인(herdr `confirm_close`: 마지막 탭·마지막 pane을 닫을 때)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub detail: String,
    pub target: CloseTarget,
}

/// 확인하면 닫을 것
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseTarget {
    Tab(String),
    Pane(String),
}

/// 가운데 맞춘 단추 칸들(herdr `row`: 너비들, 사이 칸, 창 안 위에서 몇째 줄)
fn row(i: Rect, ws: &[u16], gap: u16, off: u16) -> Vec<Rect> {
    let total = ws.iter().sum::<u16>() + gap * (ws.len().saturating_sub(1) as u16);
    let mut x = i.x + i.w.saturating_sub(total) / 2;
    ws.iter()
        .map(|w| {
            let r = Rect { x, y: i.y + off.min(i.h.saturating_sub(1)), w: (*w).min(i.w.saturating_sub(x - i.x)), h: 1 };
            x += *w + gap;
            r
        })
        .collect()
}

/// 바탕을 칠하고 글을 가운데에(herdr `button`)
fn button(buf: &mut Buffer, r: Rect, t: &str, s: Style) {
    for x in r.x..r.x + r.w {
        buf.set_string(x, r.y, " ", s);
    }
    let w = (unicode_width(t) as u16).min(r.w);
    buf.set_stringn(r.x + (r.w - w) / 2, r.y, t, w as usize, s);
}

fn label_widths(labels: &[&str]) -> Vec<u16> {
    labels.iter().map(|l| unicode_width(l) as u16).collect()
}

pub const SAVE: &str = " ↵ 저장 ";
pub const CLEAR: &str = " ^c 지우기 ";
pub const CANCEL: &str = " esc 취소 ";
pub const CLOSE: &str = " ↵ 닫기 ";

/// 이름 입력 창. 커서 자리를 준다.
pub fn render_rename(buf: &mut Buffer, area: Rect, v: &Rename, p: &Palette) -> Option<(u16, u16)> {
    let q = popup(area, 56, 7)?;
    let i = panel(buf, q, p.accent, p.panel_bg);
    buf.set_stringn(
        i.x,
        i.y,
        v.title,
        i.w as usize,
        Style::default().fg(p.text).bg(p.panel_bg).add_modifier(Modifier::BOLD),
    );
    let input = Rect { x: i.x, y: i.y + 2, w: i.w, h: 1 };
    let field = Style::default().fg(p.text).bg(p.surface0);
    for x in input.x..input.x + input.w {
        buf.set_string(x, input.y, " ", field);
    }
    let cursor = editor::render(buf, Rect { x: input.x + 1, w: input.w.saturating_sub(1), ..input }, &v.input, field);
    let rs = row(i, &label_widths(&[SAVE, CLEAR, CANCEL]), 2, 3);
    if let [save, clear, cancel] = rs.as_slice() {
        button(buf, *save, SAVE, Style::default().fg(contrast(p)).bg(p.accent).add_modifier(Modifier::BOLD));
        let n = Style::default().fg(p.text).bg(p.surface0).add_modifier(Modifier::BOLD);
        button(buf, *clear, CLEAR, n);
        button(buf, *cancel, CANCEL, n);
    }
    cursor
}

/// 닫기 확인 창(빨간 테두리)
pub fn render_confirm(buf: &mut Buffer, area: Rect, c: &Confirm, p: &Palette) {
    let Some(q) = popup(area, 64, 6) else { return };
    let i = panel(buf, q, p.red, p.panel_bg);
    buf.set_stringn(
        i.x,
        i.y,
        format!(" {}", c.title),
        i.w as usize,
        Style::default().fg(p.red).bg(p.panel_bg).add_modifier(Modifier::BOLD),
    );
    buf.set_stringn(i.x, i.y + 1, format!(" {}", c.detail), i.w as usize, Style::default().fg(p.text).bg(p.panel_bg));
    let rs = row(i, &label_widths(&[CLOSE, CANCEL]), 2, 3);
    if let [ok, cancel] = rs.as_slice() {
        button(buf, *ok, CLOSE, Style::default().fg(contrast(p)).bg(p.red).add_modifier(Modifier::BOLD));
        button(buf, *cancel, CANCEL, Style::default().fg(p.text).bg(p.surface0).add_modifier(Modifier::BOLD));
    }
}
