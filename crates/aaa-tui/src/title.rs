//! 웹 Sidebar의 전체 제목 tooltip을 TUI의 읽기 전용 창으로 옮긴다. 목록은 두 줄 카드 그대로 두고,
//! 선택 제목은 pane 위에 미리 보인다. CJK·결합 문자·ZWJ emoji를 터미널 칸과 grapheme 단위로 감싼다.

use aaa_proto::layout::Rect;
use crossterm::event::KeyCode;
use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::Palette;
use crate::{App, Overlay, fill, help, list, unicode_width};

pub(super) struct View {
    text: String,
    scroll: usize,
}

/// 단어 생략 없이 감싼다. 제목의 제어 문자는 화면 조작 대신 공백으로 보인다.
fn lines(text: &str, width: u16) -> Vec<String> {
    if width == 0 {
        return vec![];
    }
    let (mut out, mut row, mut used) = (Vec::new(), String::new(), 0);
    for g in text.graphemes(true) {
        if g == "\n" || g == "\r\n" {
            out.push(std::mem::take(&mut row));
            used = 0;
            continue;
        }
        let g = if g.chars().any(char::is_control) { " " } else { g };
        let w = unicode_width(g);
        if used + w > width as usize && !row.is_empty() {
            out.push(std::mem::take(&mut row));
            used = 0;
        }
        row.push_str(g);
        used += w;
    }
    out.push(row);
    out
}

pub(super) fn popup(area: Rect) -> Option<Rect> {
    help::popup(area, 76, 22)
}

impl View {
    fn layout(&self, area: Rect) -> Option<(Rect, Vec<String>, usize)> {
        let q = popup(area)?;
        let body = Rect { x: q.x + 1, y: q.y + 2, w: q.w - 2, h: q.h.saturating_sub(4) };
        let all = lines(&self.text, body.w);
        let max = all.len().saturating_sub(body.h as usize);
        Some((body, all, max))
    }

    pub(super) fn key(&mut self, key: KeyCode, area: Rect) {
        let Some((body, _, max)) = self.layout(area) else { return };
        self.scroll = self.scroll.min(max);
        self.scroll = match key {
            KeyCode::Up | KeyCode::Char('k') => self.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll.saturating_add(1).min(max),
            KeyCode::PageUp => self.scroll.saturating_sub(body.h as usize),
            KeyCode::PageDown => self.scroll.saturating_add(body.h as usize).min(max),
            KeyCode::Home => 0,
            KeyCode::End => max,
            _ => self.scroll,
        };
    }
}

pub(super) fn render(buf: &mut Buffer, area: Rect, v: &View, p: &Palette) {
    let (Some(q), Some((body, all, max))) = (popup(area), v.layout(area)) else { return };
    let inner = help::panel(buf, q, p.accent, p.panel_bg);
    let st = Style::default().fg(p.text).bg(p.panel_bg);
    buf.set_stringn(inner.x, inner.y, "제목 전체 · esc 닫기", inner.w as usize, st.add_modifier(Modifier::BOLD));
    let scroll = v.scroll.min(max);
    for (n, text) in all.iter().skip(scroll).take(body.h as usize).enumerate() {
        buf.set_stringn(body.x, body.y + n as u16, text, body.w as usize, st);
    }
    let footer = format!("↑↓ PgUp/Dn Home/End · {}/{}", scroll + 1, max + 1);
    buf.set_stringn(inner.x, inner.y + inner.h - 1, footer, inner.w as usize, st.fg(p.overlay1));
}

pub(super) fn preview(body: Rect, text: String) -> Option<(Rect, Vec<String>)> {
    if body.w < 4 || body.h < 8 {
        return None;
    }
    let all = lines(&text, body.w.saturating_sub(2));
    let h = 1 + all.len().min(3) as u16;
    Some((Rect { y: body.y + body.h - h - 1, h, ..body }, all))
}

pub(super) fn render_preview(buf: &mut Buffer, r: Rect, all: &[String], p: &Palette) {
    let st = Style::default().fg(p.text).bg(p.panel_bg);
    fill(buf, r, st);
    let hint = if all.len() > 3 { "선택 제목 · i 전체 ↓" } else { "선택 제목 · i 전체" };
    buf.set_stringn(r.x, r.y, hint, r.w as usize, st.fg(p.accent));
    for (n, text) in all.iter().take(3).enumerate() {
        buf.set_stringn(r.x + 1, r.y + n as u16 + 1, text, r.w.saturating_sub(2) as usize, st);
    }
}

impl App {
    pub(super) fn row_title(&self, row: &list::Row) -> Option<String> {
        match row {
            list::Row::Thread(id) => self.threads.get(id).map(|t| t.title.clone()),
            list::Row::Pane(id) => self.panes.get(id).map(|p| list::pane_title(p, &self.threads)),
            _ => None,
        }
    }

    pub(super) fn selected_title(&self) -> Option<String> {
        let rows = self.nav_rows();
        rows.get(self.list_index(&rows)).and_then(|row| self.row_title(row))
    }

    pub(super) fn show_selected_title(&mut self) {
        if let Some(text) = self.selected_title() {
            self.show_title(text);
        }
    }

    pub(super) fn show_title(&mut self, text: String) {
        self.drag = None;
        self.overlay = Some(Overlay::Title(View { text, scroll: 0 }));
    }
}
