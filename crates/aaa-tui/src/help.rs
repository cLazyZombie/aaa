//! 단축키 창(`prefix ?`). herdr 0.9.5 `src/client/shell/overlays.rs` `render_help_overlay`·`help_lines`의
//! 모양을 따른다: 가운데 76×22 테두리 창, 제목·닫기 단추, `/` 검색 줄, 묶음별 키 목록과 scrollbar, 아래 안내.

use aaa_proto::layout::Rect;
use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::theme::{Palette, contrast};
use crate::{Help, bind, unicode_width};

/// 창 자리(herdr `popup`: 화면에서 좌우 4칸·위아래 2칸은 남긴다)
pub(crate) fn popup(a: Rect, w: u16, h: u16) -> Option<Rect> {
    let w = w.min(a.w.saturating_sub(4));
    let h = h.min(a.h.saturating_sub(2));
    (w >= 4 && h >= 4).then(|| Rect { x: a.x + (a.w - w) / 2, y: a.y + (a.h - h) / 2, w, h })
}

/// 테두리 창을 그리고 안쪽 칸을 준다(herdr `panel`: 판 바탕 `bg`, 테두리 색 `border`)
pub(crate) fn panel(buf: &mut Buffer, a: Rect, border: ratatui::style::Color, bg: ratatui::style::Color) -> Rect {
    let line = Style::default().fg(border).bg(bg).remove_modifier(Modifier::DIM);
    let bg = Style::default().bg(bg).remove_modifier(Modifier::DIM);
    for y in a.y..a.y + a.h {
        for x in a.x..a.x + a.w {
            buf.set_string(x, y, " ", bg);
        }
    }
    let (r, b) = (a.x + a.w - 1, a.y + a.h - 1);
    for x in a.x..=r {
        let (top, bottom) = if x == a.x {
            ("┌", "└")
        } else if x == r {
            ("┐", "┘")
        } else {
            ("─", "─")
        };
        buf.set_string(x, a.y, top, line);
        buf.set_string(x, b, bottom, line);
    }
    for y in a.y + 1..b {
        buf.set_string(a.x, y, "│", line);
        buf.set_string(r, y, "│", line);
    }
    Rect { x: a.x + 1, y: a.y + 1, w: a.w - 2, h: a.h - 2 }
}

/// 보일 줄들: (묶음 머리인가, 키, 설명)
fn lines(prefixes: &str, query: &str) -> Vec<(bool, String, String)> {
    let groups = bind::filter(bind::help_groups(prefixes), query);
    if groups.is_empty() {
        return vec![(false, String::new(), " 맞는 단축키가 없다".into())];
    }
    let key_w = groups.iter().flat_map(|(_, e)| e.iter().map(|(k, _)| unicode_width(k))).max().unwrap_or(8);
    let mut out = Vec::new();
    for (g, entries) in groups {
        out.push((true, String::new(), format!(" {g}")));
        for (k, l) in entries {
            let pad = key_w - unicode_width(&k);
            out.push((false, format!(" {k}{} ", " ".repeat(pad)), l.to_string()));
        }
        out.push((false, String::new(), String::new()));
    }
    out
}

/// 넘길 수 있는 끝(창 크기와 검색어로 정해진다)
pub(crate) fn max_scroll(area: Rect, prefixes: &str, query: &str) -> usize {
    let rows = popup(area, 76, 22).map_or(0, |q| q.h.saturating_sub(7) as usize);
    lines(prefixes, query).len().saturating_sub(rows)
}

/// 창을 그리고, 검색 줄에 쓰는 중이면 커서 자리를 준다
pub(crate) fn render(buf: &mut Buffer, area: Rect, h: &Help, prefixes: &str, p: &Palette) -> Option<(u16, u16)> {
    let q = popup(area, 76, 22)?;
    let i = panel(buf, q, p.accent, p.panel_bg);
    if i.w < 20 || i.h < 6 {
        return None;
    }
    let right = i.x + i.w;
    let on_panel = Style::default().fg(p.text).bg(p.panel_bg);
    buf.set_stringn(i.x, i.y, "단축키", i.w as usize, on_panel.add_modifier(Modifier::BOLD));
    // herdr: 닫기 단추는 13칸, 글은 가운데
    let close = if h.search { " esc 돌아가기 " } else { " esc 닫기 " };
    let button = Style::default().fg(contrast(p)).bg(p.accent).add_modifier(Modifier::BOLD);
    let cx = right.saturating_sub(13);
    for x in cx..right {
        buf.set_string(x, i.y, " ", button);
    }
    let cw = (unicode_width(close) as u16).min(13);
    buf.set_stringn(cx + (13 - cw) / 2, i.y, close, cw as usize, button);
    let sy = i.y + 1;
    let mut cursor = None;
    if h.search {
        buf.set_stringn(i.x, sy, " / ", i.w as usize, on_panel);
        let end = buf.set_stringn(i.x + 3, sy, &h.query, i.w.saturating_sub(3) as usize, on_panel).0;
        cursor = Some((end.min(right - 1), sy));
    } else {
        buf.set_stringn(
            i.x,
            sy,
            " / 를 누르면 명령·키로 거른다",
            i.w as usize,
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
    }
    let body = Rect { x: i.x, y: i.y + 3, w: i.w, h: i.h.saturating_sub(5) };
    let all = lines(prefixes, &h.query);
    let rows = body.h as usize;
    let max_scroll = all.len().saturating_sub(rows);
    let scroll = h.scroll.min(max_scroll);
    let text_w = if max_scroll > 0 { body.w - 1 } else { body.w };
    for (n, (head, k, l)) in all.iter().skip(scroll).take(rows).enumerate() {
        let y = body.y + n as u16;
        if *head {
            let st = Style::default().fg(p.accent).bg(p.panel_bg).add_modifier(Modifier::BOLD);
            buf.set_stringn(body.x, y, l, text_w as usize, st);
            continue;
        }
        let key = Style::default().fg(p.mauve).bg(p.panel_bg).add_modifier(Modifier::BOLD);
        let x = buf.set_stringn(body.x, y, k, text_w as usize, key).0;
        let room = (body.x + text_w).saturating_sub(x) as usize;
        let label = if k.is_empty() { p.overlay1 } else { p.text };
        buf.set_stringn(x, y, l, room, Style::default().fg(label).bg(p.panel_bg));
    }
    // herdr scrollbar: 넘칠 때만, 칸 전체는 흐린 ▐, 보이는 자리는 밝은 ▐
    let len = (body.h as usize * rows / all.len()).max(1);
    if let Some(top) = ((body.h as usize).saturating_sub(len) * scroll).checked_div(max_scroll) {
        let x = body.x + body.w - 1;
        for dy in 0..body.h {
            let on = (dy as usize) >= top && (dy as usize) < top + len;
            buf.set_string(
                x,
                body.y + dy,
                "▐",
                Style::default().fg(if on { p.overlay1 } else { p.overlay0 }).bg(p.panel_bg),
            );
        }
    }
    let footer = if h.search {
        " 지우기 backspace·^u · 넘기기 ↑↓ · 돌아가기 esc"
    } else {
        " 검색 / · 넘기기 j/k/↑↓/pgup/pgdn · 닫기 esc/enter"
    };
    buf.set_stringn(i.x, i.y + i.h - 1, footer, i.w as usize, Style::default().fg(p.overlay0).bg(p.panel_bg));
    cursor
}
