//! 마우스(묶음 3d-1). herdr 0.9.5 `src/client/shell/mouse.rs`(맞히기 순서·누름/뗌·끌기), `word_selection.rs`(두 번 누름 낱말),
//! `src/ui/panes.rs` `automatic_selection_style`(선택 색)을 옮겼다. 그린 자리는 그릴 때 `Hits`에 적고, 사건은 그 자리로 맞힌다.
//!
//! - 탭 막대: 누르고 그 자리에서 떼면 그 탭, 휠은 앞·뒤 탭, ` + ` 새 탭, ` < ` ` > ` 넘기기.
//! - 사이드바: 줄을 누르고 떼면 연다, `«`·`»` 접기·펴기, 마지막 열(경계)을 끌면 폭(18~36), 휠은 한 줄씩.
//! - pane: 누르면 포커스, 안쪽을 끌면 보이는 화면 글을 고르고 떼면 복사(`copy_on_select`), 두 번 누르면 낱말.
//! - 창: 이름·닫기 확인은 단추가 아닌 곳을 누르면 닫고(안쪽도), 단축키 창은 밖을 누를 때만 닫는다.
//!
//! 마우스 보고를 켠 프로그램에 누름을 넘기는 것·나눔 경계 끌기·탭 끌어 옮기기는 host 요청이 필요해 다음 묶음(3d-2)이다.
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

use std::time::Duration;

use aaa_proto::GridRun;
use aaa_proto::layout::Rect;
use anyhow::Result;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::style::Color;
use tokio::time::Instant;

use crate::bind::Action;
use crate::chrome::PaneBox;
use crate::list::Row;
use crate::theme::Palette;
use crate::{App, FrontSender, Mode, Overlay, SIDEBAR_MAX, SIDEBAR_MIN, clip, help, unicode_width};

/// 두 번 누름으로 보는 사이(herdr `DOUBLE_CLICK_WINDOW`)
pub const DOUBLE_CLICK: Duration = Duration::from_millis(350);
/// 두 번 눌러 고른 낱말을 보이는 시간(herdr word highlight)
pub const WORD_SHOWN: Duration = Duration::from_millis(500);

/// 창의 단추
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Save,
    Clear,
    Cancel,
    Close,
}

/// 그린 자리(마지막으로 그린 화면)
#[derive(Debug, Clone, Default)]
pub struct Hits {
    /// 탭 이름표 자리와 탭 차례
    pub tabs: Vec<(Rect, usize)>,
    pub tab_bar: Option<Rect>,
    pub new_tab: Option<Rect>,
    pub scroll_left: Option<Rect>,
    pub scroll_right: Option<Rect>,
    /// 탭 막대가 지금 몇째 탭부터 보이나와 가장 많이 밀 수 있는 값
    pub tab_scroll: usize,
    pub max_tab_scroll: usize,
    /// 사이드바(펼침·접힘·띄운 목록) 전체 칸
    pub sidebar: Option<Rect>,
    /// 목록 줄: 줄 칸과 그 줄
    pub rows: Vec<(Rect, Row)>,
    /// 펼친 목록의 첫 보이는 줄과 보이는 줄 수(휠로 민다)
    pub list_skip: usize,
    pub list_h: u16,
    pub toggle: Option<Rect>,
    /// 펼친 사이드바의 오른쪽 경계(끌면 폭)
    pub divider: Option<Rect>,
    /// 좁은 화면 머리의 `목록` 단추
    pub header_button: Option<Rect>,
    pub panes: Vec<PaneBox>,
    /// 단축키 창
    pub popup: Option<Rect>,
    /// 이름·닫기 확인 창의 단추
    pub buttons: Vec<(Rect, Button)>,
}

pub fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

impl Hits {
    pub fn pane_at(&self, x: u16, y: u16) -> Option<&PaneBox> {
        self.panes.iter().find(|b| contains(b.rect, x, y))
    }

    pub fn row_at(&self, x: u16, y: u16) -> Option<&Row> {
        self.rows.iter().find(|(r, _)| contains(*r, x, y)).map(|(_, row)| row)
    }

    pub fn tab_at(&self, x: u16, y: u16) -> Option<usize> {
        self.tabs.iter().find(|(r, _)| contains(*r, x, y)).map(|(_, i)| *i)
    }

    pub fn button_at(&self, x: u16, y: u16) -> Option<Button> {
        self.buttons.iter().find(|(r, _)| contains(*r, x, y)).map(|(_, b)| *b)
    }

    pub fn on(&self, r: Option<Rect>, x: u16, y: u16) -> bool {
        r.is_some_and(|r| contains(r, x, y))
    }
}

/// pane 화면의 한 칸(줄, 열). 0부터.
pub type Cell = (u16, u16);

/// 고른 글 범위(pane 화면 칸, 읽는 차례로 `start` ≤ `end`, 끝 칸 포함)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub pane: String,
    pub anchor: Cell,
    pub head: Cell,
    /// 두 번 누름 낱말: 이 때까지만 보인다
    pub until: Option<Instant>,
}

impl Selection {
    pub fn range(&self) -> (Cell, Cell) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    pub fn covers(&self, row: u16, col: u16) -> bool {
        let ((r1, c1), (r2, c2)) = self.range();
        (row, col) >= (r1, c1) && (row, col) <= (r2, c2) && row >= r1 && row <= r2
    }
}

/// 누른 채 끄는 것
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drag {
    /// 탭 이름표를 눌렀다(그 자리에서 떼면 그 탭)
    Tab { index: usize },
    /// 목록 줄을 눌렀다(같은 줄에서 떼면 연다)
    Row(Row),
    /// 사이드바 경계를 끈다
    Sidebar,
    /// pane 글을 고른다. `moved`는 누른 칸을 벗어났나(herdr: 벗어나야 보인다)
    Select { pane: String, anchor: Cell, moved: bool },
}

/// 한 줄의 칸들: (열, 글자, 폭). 넓은 글자는 두 칸이고 뒤 칸은 없다.
pub fn cells(runs: &[GridRun]) -> Vec<(u16, char, u16)> {
    let mut out = Vec::new();
    let mut col = 0u16;
    for run in runs {
        for ch in run.t.chars() {
            let w = unicode_width(ch.encode_utf8(&mut [0; 4])) as u16;
            if w == 0 {
                continue;
            }
            out.push((col, ch, w));
            col += w;
        }
    }
    out
}

/// 고른 범위의 글(herdr `PaneSelectionRead`): 줄마다 끝 빈칸을 빼고 줄바꿈으로 잇는다. 넓은 글자는 걸치면 넣는다.
pub fn text(lines: &[Vec<GridRun>], (start, end): (Cell, Cell)) -> String {
    let mut out: Vec<String> = Vec::new();
    for row in start.0..=end.0 {
        let from = if row == start.0 { start.1 } else { 0 };
        let to = if row == end.0 { end.1 } else { u16::MAX };
        let line: String = lines
            .get(row as usize)
            .map(|runs| {
                cells(runs).into_iter().filter(|(c, _, w)| c + w > from && *c <= to).map(|(_, ch, _)| ch).collect()
            })
            .unwrap_or_default();
        out.push(line.trim_end().to_string());
    }
    out.join("\n")
}

/// 낱말을 가르는 글자(herdr `word_bounds_at_column`의 구분자 토큰). `:`·`/`·`.`은 가르지 않아 URL·경로가 한 낱말이다.
fn separator(c: char) -> bool {
    c.is_whitespace() || "\"'`()[]{}<>,;|".contains(c)
}

/// `col` 칸의 낱말 범위(열, 끝 칸 포함). 빈칸·구분자 위면 None.
pub fn word_at(runs: &[GridRun], col: u16) -> Option<(u16, u16)> {
    let cs = cells(runs);
    let at = cs.iter().position(|(c, _, w)| col >= *c && col < c + w)?;
    if separator(cs[at].1) {
        return None;
    }
    let mut a = at;
    while a > 0 && !separator(cs[a - 1].1) {
        a -= 1;
    }
    let mut b = at;
    while b + 1 < cs.len() && !separator(cs[b + 1].1) {
        b += 1;
    }
    Some((cs[a].0, cs[b].0 + cs[b].2 - 1))
}

/// 선택 칸의 색(herdr `automatic_selection_style`): 바탕을 흰색(어두운 바탕)·검정(밝은 바탕) 쪽으로 28% 섞고, 글은 대비가 큰 쪽.
/// aaa는 터미널 바탕색을 묻지 않아 herdr의 대신 값(`panel_bg`, 없으면 `surface_dim`)에서 섞는다.
pub fn selection_colors(p: &Palette) -> (Color, Color) {
    let base = if p.panel_bg == Color::Reset { p.surface_dim } else { p.panel_bg };
    let Color::Rgb(r, g, b) = base else {
        // 이름 색 테마(terminal): 섞지 않고 그 색에 대비 글
        return (base, crate::theme::contrast(p));
    };
    let target = if luminance((r, g, b)) < 0.5 { (255, 255, 255) } else { (0, 0, 0) };
    let mix = |a: u8, t: u8| (f32::from(a) + (f32::from(t) - f32::from(a)) * 0.28).round() as u8;
    let bg = (mix(r, target.0), mix(g, target.1), mix(b, target.2));
    let l = luminance(bg);
    let fg = if (l + 0.05) / 0.05 > 1.05 / (l + 0.05) { Color::Rgb(0, 0, 0) } else { Color::Rgb(255, 255, 255) };
    (Color::Rgb(bg.0, bg.1, bg.2), fg)
}

fn luminance((r, g, b): (u8, u8, u8)) -> f32 {
    let ch = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

impl App {
    /// 마우스 사건(herdr `handle_mouse`). 창이 열려 있으면 창이 먼저다.
    pub(crate) async fn on_mouse(&mut self, m: MouseEvent, tx: &mut FrontSender) -> Result<()> {
        if !self.mouse_capture {
            return Ok(());
        }
        let (x, y) = (m.column, m.row);
        let hits = self.hits.borrow().clone();
        if self.overlay.is_some() {
            self.overlay_mouse(m, &hits, tx).await?;
        } else {
            match m.kind {
                MouseEventKind::Down(MouseButton::Left) => self.press(x, y, &hits, tx).await?,
                MouseEventKind::Drag(MouseButton::Left) => self.drag_to(x, y, &hits),
                MouseEventKind::Up(MouseButton::Left) => self.release(x, y, &hits, tx).await?,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    self.wheel(x, y, m.kind == MouseEventKind::ScrollDown, &hits, tx).await?
                }
                _ => {}
            }
        }
        // 탭이 바뀌면 포커스를 그 탭으로(키와 같다)
        let leaves = self.current_leaves();
        if !self.focus.as_ref().is_some_and(|f| leaves.contains(f)) {
            self.focus = leaves.first().cloned();
        }
        self.reconcile_ascii();
        Ok(())
    }

    /// herdr: 이름·닫기 확인 창은 단추가 아닌 곳을 누르면(안쪽도) 닫는다. 단축키 창은 밖을 누를 때만 닫고 휠로 3줄씩 민다.
    async fn overlay_mouse(&mut self, m: MouseEvent, hits: &Hits, tx: &mut FrontSender) -> Result<()> {
        let (x, y) = (m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => match self.overlay.as_mut() {
                Some(Overlay::Help(_)) => {
                    if !hits.on(hits.popup, x, y) {
                        self.overlay = None;
                    }
                }
                Some(Overlay::Rename(r)) => match hits.button_at(x, y) {
                    Some(Button::Save) => {
                        if let Some(Overlay::Rename(r)) = self.overlay.take() {
                            self.save_rename(r, tx).await?;
                        }
                    }
                    Some(Button::Clear) => r.input.clear(),
                    _ => self.overlay = None,
                },
                Some(Overlay::Confirm(_)) => match hits.button_at(x, y) {
                    Some(Button::Close) => {
                        if let Some(Overlay::Confirm(c)) = self.overlay.take() {
                            self.confirm(c, tx).await?;
                        }
                    }
                    _ => self.overlay = None,
                },
                None => {}
            },
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let area = Rect { x: 0, y: 0, w: self.width, h: self.height };
                let prefixes = self.prefix_label();
                if let Some(Overlay::Help(h)) = self.overlay.as_mut() {
                    let max = help::max_scroll(area, &prefixes, &h.query);
                    h.scroll = if m.kind == MouseEventKind::ScrollDown {
                        (h.scroll + 3).min(max)
                    } else {
                        h.scroll.saturating_sub(3)
                    };
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 왼쪽 누름(herdr 맞히기 순서: 사이드바 접기 → 경계 → 새 탭 → `<` `>` → 탭 → 목록 줄 → pane)
    async fn press(&mut self, x: u16, y: u16, hits: &Hits, tx: &mut FrontSender) -> Result<()> {
        self.selection = None;
        if self.mode == Mode::Prefix {
            self.mode = Mode::Terminal;
        }
        if hits.on(hits.toggle, x, y) {
            self.toggle_list();
            return Ok(());
        }
        if hits.on(hits.divider, x, y) {
            self.drag = Some(Drag::Sidebar);
            return Ok(());
        }
        if hits.on(hits.header_button, x, y) {
            // 좁은 화면 `목록` 단추(herdr mobile switch): 탐색 모드를 켜고 끈다
            self.mode = if self.mode == Mode::Navigate { Mode::Terminal } else { Mode::Navigate };
            self.list_sel = None;
            return Ok(());
        }
        if hits.on(hits.new_tab, x, y) {
            return self.act(Action::NewTab, tx).await;
        }
        if hits.on(hits.scroll_left, x, y) {
            self.tab_scroll = Some((hits.tab_scroll.saturating_sub(1), self.tab));
            return Ok(());
        }
        if hits.on(hits.scroll_right, x, y) {
            self.tab_scroll = Some(((hits.tab_scroll + 1).min(hits.max_tab_scroll), self.tab));
            return Ok(());
        }
        if let Some(index) = hits.tab_at(x, y) {
            self.drag = Some(Drag::Tab { index });
            return Ok(());
        }
        if let Some(row) = hits.row_at(x, y) {
            self.drag = Some(Drag::Row(row.clone()));
            return Ok(());
        }
        // 목록 밖을 누르면 탐색을 끝낸다
        if self.mode == Mode::Navigate {
            self.mode = Mode::Terminal;
            self.list_sel = None;
        }
        let Some(b) = hits.pane_at(x, y).cloned() else { return Ok(()) };
        self.focus = Some(b.pane.clone());
        if !contains(b.inner, x, y) {
            return Ok(());
        }
        let cell = (y - b.inner.y, x - b.inner.x);
        let now = Instant::now();
        let double = self.last_click.as_ref().is_some_and(|(t, p, c)| {
            now.duration_since(*t) <= DOUBLE_CLICK && *p == b.pane && c.0 == cell.0 && c.1.abs_diff(cell.1) <= 1
        });
        if double {
            // 두 번 누름: 낱말을 고르고 복사한 뒤 잠깐 보인다(herdr word selection)
            self.last_click = None;
            let word =
                self.grids.get(&b.pane).and_then(|g| g.lines.get(cell.0 as usize)).and_then(|l| word_at(l, cell.1));
            if let Some((a, z)) = word {
                let sel =
                    Selection { pane: b.pane, anchor: (cell.0, a), head: (cell.0, z), until: Some(now + WORD_SHOWN) };
                self.copy_selection(&sel);
                self.selection = Some(sel);
            }
            return Ok(());
        }
        self.last_click = Some((now, b.pane.clone(), cell));
        self.drag = Some(Drag::Select { pane: b.pane, anchor: cell, moved: false });
        Ok(())
    }

    fn drag_to(&mut self, x: u16, y: u16, hits: &Hits) {
        match self.drag.clone() {
            // herdr: 폭 = 열 + 1, 18~36
            Some(Drag::Sidebar) => {
                let left = hits.sidebar.map_or(0, |s| s.x);
                self.sidebar_width = (x + 1).saturating_sub(left).clamp(SIDEBAR_MIN, SIDEBAR_MAX);
            }
            // 누른 칸을 벗어나야 고른다. pane 밖으로 끌면 가장자리 칸으로 묶는다.
            Some(Drag::Select { pane, anchor, moved }) => {
                let Some(i) = hits.panes.iter().find(|b| b.pane == pane).map(|b| b.inner) else { return };
                if i.w == 0 || i.h == 0 {
                    return;
                }
                let cell = (y.clamp(i.y, i.y + i.h - 1) - i.y, x.clamp(i.x, i.x + i.w - 1) - i.x);
                let moved = moved || cell != anchor;
                self.drag = Some(Drag::Select { pane: pane.clone(), anchor, moved });
                if moved {
                    self.selection = Some(Selection { pane, anchor, head: cell, until: None });
                }
            }
            _ => {}
        }
    }

    async fn release(&mut self, x: u16, y: u16, hits: &Hits, tx: &mut FrontSender) -> Result<()> {
        match self.drag.take() {
            // herdr: 누른 이름표에서 떼면 그 탭(`TabFocus`)
            Some(Drag::Tab { index }) if hits.tab_at(x, y) == Some(index) => self.tab = index,
            // 누른 줄에서 떼면 연다(탐색 중이면 탐색도 끝낸다)
            Some(Drag::Row(row)) if hits.row_at(x, y) == Some(&row) => {
                if self.mode == Mode::Navigate {
                    self.mode = Mode::Terminal;
                    self.list_sel = None;
                }
                self.open_row(tx, row).await?;
            }
            Some(Drag::Select { pane, moved: true, .. }) => {
                if let Some(sel) = self.selection.clone().filter(|s| s.pane == pane)
                    && self.copy_on_select
                {
                    self.copy_selection(&sel);
                    self.selection = None;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 휠: 탭 막대는 앞·뒤 탭, 펼친 목록은 한 줄씩, pane은 포커스(기록 넘기기·프로그램에 넘기기는 다음 묶음)
    async fn wheel(&mut self, x: u16, y: u16, down: bool, hits: &Hits, tx: &mut FrontSender) -> Result<()> {
        if hits.on(hits.tab_bar, x, y) {
            return self.act(if down { Action::NextTab } else { Action::PreviousTab }, tx).await;
        }
        if hits.on(hits.sidebar, x, y) {
            if hits.list_h > 0 {
                let max = self.list_rows().len().saturating_sub(hits.list_h as usize);
                let top = if down { hits.list_skip + 1 } else { hits.list_skip.saturating_sub(1) };
                self.list_top = Some(top.min(max));
            }
            return Ok(());
        }
        if let Some(b) = hits.pane_at(x, y)
            && self.focus.as_deref() != Some(b.pane.as_str())
        {
            self.focus = Some(b.pane.clone());
        }
        Ok(())
    }

    /// 고른 글을 복사하고 알린다(herdr 「copied to clipboard」)
    pub(crate) fn copy_selection(&mut self, sel: &Selection) {
        let Some(g) = self.grids.get(&sel.pane) else { return };
        let t = text(&g.lines, sel.range());
        if t.trim().is_empty() {
            return;
        }
        match clip::copy_text(&t) {
            Ok(()) => self.set_message("클립보드에 복사했다"),
            Err(e) => self.set_error(format!("복사하지 못했다: {e}")),
        }
    }

    /// 고른 채 둔 글(`copy_on_select` 끔)은 ctrl+c·cmd+c로 복사하고, 다른 키는 지우고 그대로 처리한다(herdr)
    pub(crate) fn selection_key(&mut self, mods: KeyModifiers, ch: Option<char>) -> bool {
        let Some(sel) = self.selection.take() else { return false };
        let copy = ch == Some('c') && (mods == KeyModifiers::CONTROL || mods == KeyModifiers::SUPER);
        if copy && sel.until.is_none() {
            self.copy_selection(&sel);
        }
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(s: &str) -> Vec<GridRun> {
        vec![GridRun { t: s.into(), fg: 0, bg: 0, a: 0 }]
    }

    /// 넓은 글자는 두 칸, 걸치면 넣고, 줄 끝 빈칸은 뺀다
    #[test]
    fn selection_text_by_columns() {
        let lines = vec![runs("ab 한글 cd   "), runs("둘째 줄"), runs("셋째")];
        assert_eq!(text(&lines, ((0, 3), (0, 6))), "한글");
        assert_eq!(text(&lines, ((0, 4), (0, 5))), "한글");
        assert_eq!(text(&lines, ((0, 8), (1, 1))), "cd\n둘");
        assert_eq!(text(&lines, ((0, 0), (2, 100))), "ab 한글 cd\n둘째 줄\n셋째");
        let s = Selection { pane: "p".into(), anchor: (1, 3), head: (0, 5), until: None };
        assert_eq!(s.range(), ((0, 5), (1, 3)));
        assert!(s.covers(0, 9) && s.covers(1, 0) && !s.covers(1, 4) && !s.covers(0, 4));
    }

    /// 두 번 누름 낱말: 구분자로 가르고 URL·경로는 한 낱말(herdr `word_bounds_at_column`)
    #[test]
    fn word_like_herdr() {
        let r = runs("see https://example.com/a?b=1 (src/main.rs) 한글낱말");
        assert_eq!(word_at(&r, 0), Some((0, 2)));
        assert_eq!(word_at(&r, 3), None);
        assert_eq!(word_at(&r, 10), Some((4, 28)));
        assert_eq!(word_at(&r, 30), None);
        assert_eq!(word_at(&r, 36), Some((31, 41)));
        assert_eq!(word_at(&r, 45), Some((44, 51)));
        assert_eq!(text(std::slice::from_ref(&r), ((0, 31), (0, 41))), "src/main.rs");
    }

    /// 선택 색: catppuccin `panel_bg`(어두움)를 흰색 쪽으로 28%, 글은 대비가 큰 검정
    #[test]
    fn selection_colors_like_herdr() {
        let p = Palette::catppuccin();
        let (bg, fg) = selection_colors(&p);
        let Color::Rgb(r, g, b) = p.panel_bg else { panic!() };
        let mix = |a: u8| (f32::from(a) + (255.0 - f32::from(a)) * 0.28).round() as u8;
        assert_eq!(bg, Color::Rgb(mix(r), mix(g), mix(b)));
        assert!(matches!(fg, Color::Rgb(0, 0, 0) | Color::Rgb(255, 255, 255)));
    }
}
