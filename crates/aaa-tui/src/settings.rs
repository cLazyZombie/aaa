//! 설정 창(`prefix s`). herdr 0.9.5 `src/client/shell/settings.rs`·`settings_overlay.rs` `render_settings_overlay`의 모양과
//! 동작을 따른다: 가운데 76×22 창, 머리 ` 설정`, 구역 이름표, 구분선, 고르는 줄(`▸`, 지금 테마 `✓`), 안내와 적용·닫기 단추.
//! 고른 값은 host.toml `[tui]`에 쓴다(`aaa_config::tui_edit`, herdr `write_edit`).
//!
//! herdr의 sound·toasts·integrations 구역은 aaa TUI에 그 기능(소리, 배경 알림 전달 방식, harness 연동 설치)이 없어 뺐다.

use aaa_proto::layout::Rect;
use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::help::{panel, popup};
use crate::prompt::{button, label_widths, row};
use crate::theme::{Palette, THEME_NAMES, canonical_theme_name, contrast};
use crate::unicode_width;

/// herdr `ClientSettingsSection`(aaa에 있는 것만)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Theme,
    Indicators,
}

impl Section {
    pub const ALL: [Section; 2] = [Section::Theme, Section::Indicators];

    pub fn label(self) -> &'static str {
        match self {
            Section::Theme => "테마",
            Section::Indicators => "상태 표시",
        }
    }
}

/// 상태 표시 고르기(herdr `render_choice_section` indicators): 차례는 `dots`·`symbols`
pub const INDICATOR_CHOICES: [&str; 2] = ["색 점  ● ● ● ○ ·", "기호  × ◐ ✓ ○ ·"];

/// herdr `ClientSettingsOverlay`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub section: Section,
    pub selected: usize,
    /// 창을 열 때의 테마(Esc면 그것으로 되돌리고, 그 줄에 `✓`)
    pub original_theme: String,
}

impl Settings {
    /// 그 구역의 고를 수 있는 줄 수(herdr `settings_choice_count`)
    pub fn choice_count(&self) -> usize {
        match self.section {
            Section::Theme => THEME_NAMES.len(),
            Section::Indicators => INDICATOR_CHOICES.len(),
        }
    }

    /// 다음·앞 구역(끝에서 돈다, herdr `move_settings_section`)
    pub fn next_section(&self, forward: bool) -> Section {
        let i = Section::ALL.iter().position(|s| *s == self.section).unwrap_or(0);
        let n = Section::ALL.len();
        Section::ALL[if forward { (i + 1) % n } else { (i + n - 1) % n }]
    }

    /// 고른 줄 옮기기(끝에서 멈춤, herdr `move_settings_selection`)
    pub fn step(&mut self, down: bool) {
        let last = self.choice_count().saturating_sub(1);
        self.selected = if down { (self.selected + 1).min(last) } else { self.selected.saturating_sub(1) };
    }
}

/// 테마 이름의 차례(모르면 0, herdr `theme_index`)
pub fn theme_index(name: &str) -> usize {
    let name = canonical_theme_name(name).unwrap_or(THEME_NAMES[0]);
    THEME_NAMES.iter().position(|n| *n == name).unwrap_or(0)
}

pub const APPLY: &str = " ↵ 적용 ";
pub const CLOSE: &str = " esc 닫기 ";

/// 마우스가 맞히는 자리
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hits {
    pub popup: Rect,
    pub tabs: Vec<(Rect, Section)>,
    pub choices: Vec<(Rect, usize)>,
    pub apply: Rect,
    pub close: Rect,
}

/// 고르는 줄 하나(herdr `draw_choice`: 고른 줄은 accent 바탕 굵게와 `▸`, 지금 값은 끝에 `✓`)
fn draw_choice(buf: &mut Buffer, r: Rect, label: &str, selected: bool, current: bool, p: &Palette) {
    let style = if selected {
        Style::default().fg(contrast(p)).bg(p.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.text).bg(p.panel_bg)
    };
    for x in r.x..r.x + r.w {
        buf.set_string(x, r.y, " ", style);
    }
    let marker = if selected { "▸" } else { " " };
    let current = if current { " ✓" } else { "" };
    buf.set_stringn(r.x, r.y, format!(" {marker} {label}{current}"), r.w as usize, style);
}

/// 설정 창을 그리고 맞히는 자리를 준다(herdr `render_settings_overlay`). 화면이 작아 안쪽이 20×8보다 작으면 그리지 않는다.
pub fn render(buf: &mut Buffer, area: Rect, s: &Settings, p: &Palette) -> Option<Hits> {
    let q = popup(area, 76, 22)?;
    let i = panel(buf, q, p.accent, p.panel_bg);
    if i.w < 20 || i.h < 8 {
        return None;
    }
    let none = Rect { x: 0, y: 0, w: 0, h: 0 };
    let mut hits = Hits { popup: q, tabs: vec![], choices: vec![], apply: none, close: none };
    buf.set_stringn(
        i.x,
        i.y,
        " 설정",
        i.w as usize,
        Style::default().fg(p.text).bg(p.panel_bg).add_modifier(Modifier::BOLD),
    );
    // 구역 이름표: 지금 구역은 accent 바탕 굵게, 나머지는 overlay1
    let mut x = i.x;
    for section in Section::ALL {
        let label = format!(" {} ", section.label());
        let w = (unicode_width(&label) as u16).min((i.x + i.w).saturating_sub(x));
        let r = Rect { x, y: i.y + 1, w, h: 1 };
        let style = if section == s.section {
            Style::default().fg(contrast(p)).bg(p.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.overlay1).bg(p.panel_bg)
        };
        buf.set_stringn(r.x, r.y, &label, r.w as usize, style);
        hits.tabs.push((r, section));
        x += w + 1;
        if x >= i.x + i.w {
            break;
        }
    }
    buf.set_stringn(
        i.x,
        i.y + 2,
        "─".repeat(i.w as usize),
        i.w as usize,
        Style::default().fg(p.surface0).bg(p.panel_bg),
    );
    let content = Rect { x: i.x, y: i.y + 4, w: i.w, h: i.h.saturating_sub(7) };
    match s.section {
        Section::Theme => {
            // 고른 줄이 보이게 내린다
            let visible = content.h as usize;
            let skip = s.selected.saturating_sub(visible.saturating_sub(1));
            let original = canonical_theme_name(&s.original_theme);
            for (row_i, (n, name)) in THEME_NAMES.iter().enumerate().skip(skip).take(visible).enumerate() {
                let r = Rect { y: content.y + row_i as u16, h: 1, ..content };
                draw_choice(buf, r, name, n == s.selected, original == Some(*name), p);
                hits.choices.push((r, n));
            }
        }
        Section::Indicators => {
            let title = Style::default().fg(p.text).bg(p.panel_bg).add_modifier(Modifier::BOLD);
            buf.set_stringn(content.x, content.y, "agent 상태 표시", content.w as usize, title);
            buf.set_stringn(
                content.x,
                content.y + 1,
                "상태마다 색 점 또는 다른 기호",
                content.w as usize,
                Style::default().fg(p.overlay1).bg(p.panel_bg),
            );
            for (n, choice) in INDICATOR_CHOICES.iter().enumerate() {
                let y = content.y + 3 + n as u16;
                if y >= content.y + content.h {
                    break;
                }
                let r = Rect { y, h: 1, ..content };
                draw_choice(buf, r, choice, n == s.selected, false, p);
                hits.choices.push((r, n));
            }
        }
    }
    buf.set_stringn(
        i.x,
        i.y + i.h - 2,
        " ↑↓ 고르기  tab 구역",
        i.w as usize,
        Style::default().fg(p.overlay1).bg(p.panel_bg),
    );
    if let [apply, close] = row(i, &label_widths(&[APPLY, CLOSE]), 2, i.h - 1).as_slice() {
        button(buf, *apply, APPLY, Style::default().fg(contrast(p)).bg(p.accent).add_modifier(Modifier::BOLD));
        button(buf, *close, CLOSE, Style::default().fg(p.text).bg(p.surface0).add_modifier(Modifier::BOLD));
        (hits.apply, hits.close) = (*apply, *close);
    }
    Some(hits)
}
