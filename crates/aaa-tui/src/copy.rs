//! copy 모드(herdr 0.9.5 `src/client/shell/copy_mode.rs`·`src/copy_mode.rs`). `prefix [`로 포커스 pane에 들어가 키로 커서를
//! 옮기며 이 화면의 기록을 넘기고(`PaneScroll`), 고른 글은 끌어 고르기처럼 host가 기록까지 읽어 복사한다(`PaneSelectionRead`).
//! 커서·고르기는 기록 줄 번호(`mouse::Point`)로 둔다. 화면 위치는 frame을 기다리지 않고 보낸 값으로 셈한다(herdr와 같다).
//! 낱말·문단 이동(`w b e W B E { }`)은 herdr처럼 host가 화면 사본의 기록까지 보고 정하고(`PaneCopyMotion`), 답이 올 때까지
//! 다음 키를 붙잡아 그 자리에서 잇는다. 찾기(`/`·`?`·`n`·`N`)는 아직 없다.
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

use aaa_proto::{CopyMotion, FrontRequest, GridRun, TextPoint};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};

use crate::keys::Combo;
use crate::mouse::{self, Point, Selection};
use crate::{App, FrontSender, Mode, Pending};

/// copy 모드 상태(herdr `ClientCopyModeState`). pane에 붙어 있어 포커스가 다른 pane으로 가면 Terminal 모드가 되고,
/// 돌아오면 다시 copy 모드다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyMode {
    pub pane: String,
    pub cursor: Point,
    /// 지금 보는 기록 위치(보낸 값)
    pub offset: u32,
    /// 들어올 때의 기록 위치(나갈 때 되돌린다)
    pub entry: u32,
    pub select: Option<Select>,
}

/// 고르기 시작점(herdr `ClientCopySelection`): 글자 단위는 그 칸, 줄 단위는 그 줄
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Select {
    Char(Point),
    Line(u32),
}

/// 쪽 넘기기 줄 수(herdr `copy_mode_page_lines`)
pub fn page_lines(height: u16, half: bool) -> u32 {
    if height <= 2 {
        1
    } else if half {
        u32::from(height / 2)
    } else {
        u32::from(height - 2)
    }
}

/// herdr `copy_mode_command_char`: Shift 말고 다른 수정키가 있으면 명령이 아니다. Shift 글자는 대문자.
pub fn command_char(c: Combo) -> Option<char> {
    let KeyCode::Char(ch) = c.code else { return None };
    if !c.mods.difference(KeyModifiers::SHIFT).is_empty() {
        return None;
    }
    Some(if c.mods.contains(KeyModifiers::SHIFT) { ch.to_ascii_uppercase() } else { ch })
}

/// 줄의 첫 빈칸 아닌 칸과 마지막 글자 칸(herdr `first_non_blank_col`·`last_character_col`)
pub fn line_cols(runs: &[GridRun]) -> Option<(u16, u16)> {
    let cs = mouse::cells(runs);
    let first = cs.iter().find(|(_, ch, _)| !ch.is_whitespace())?.0;
    let last = cs.iter().rev().find(|(_, ch, _)| !ch.is_whitespace())?.0;
    Some((first, last))
}

/// 줄 단위 고르기: 시작 줄 첫 칸부터 끝 줄 마지막 칸까지(herdr `Selection::line_range`)
pub fn line_range(pane: String, anchor: u32, cursor: u32, end_col: u16) -> Selection {
    let (a, c) = if anchor <= cursor { (0, end_col) } else { (end_col, 0) };
    Selection { pane, anchor: (anchor, a), head: (cursor, c), until: None }
}

impl App {
    /// 그 pane의 그린 칸 크기(폭, 높이)와 기록 줄 수
    fn copy_geometry(&self, pane: &str) -> Option<(u16, u16, u32)> {
        let max = self.grids.get(pane)?.scroll.max;
        self.hits.borrow().panes.iter().find(|b| b.pane == pane).map(|b| (b.inner.w, b.inner.h, max))
    }

    /// `prefix [`(herdr `enter_copy_mode`): 커서는 pane 안에 보이는 터미널 커서, 없으면 맨 아래 줄 첫 칸.
    /// 끌어 고른 글은 지운다.
    pub(crate) async fn enter_copy(&mut self, tx: &mut FrontSender) -> Result<()> {
        let Some(pane) = self.focus.clone() else { return Ok(()) };
        if self.copy.as_ref().is_some_and(|c| c.pane == pane) {
            self.mode = Mode::Copy;
            return Ok(());
        }
        if self.copy.is_some() {
            self.exit_copy(tx, false).await?;
        }
        let (Some((w, h, _)), Some(g)) = (self.copy_geometry(&pane), self.grids.get(&pane)) else { return Ok(()) };
        let top = mouse::view_top(g.scroll);
        let (row, col, visible) = g.cursor;
        let cursor = if visible && row < h && col < w {
            (top + u32::from(row), col)
        } else {
            (top + u32::from(h.saturating_sub(1)), 0)
        };
        let offset = g.scroll.offset;
        self.selection = None;
        self.autoscroll = None;
        self.copy = Some(CopyMode { pane, cursor, offset, entry: offset, select: None });
        self.mode = Mode::Copy;
        Ok(())
    }

    /// 나가기(herdr `exit_copy_mode`): `copy`면 보이는 고르기를 복사한다. 고르기를 지우고 들어올 때의 기록 위치로 되돌린다.
    pub(crate) async fn exit_copy(&mut self, tx: &mut FrontSender, copy: bool) -> Result<()> {
        let Some(c) = self.copy.take() else { return Ok(()) };
        if copy && let Some(sel) = self.selection.clone() {
            self.copy_selection(tx, &sel).await?;
        }
        self.selection = None;
        self.scroll_to(tx, c.pane, c.entry as usize).await?;
        if self.mode == Mode::Copy {
            self.mode = Mode::Terminal;
        }
        Ok(())
    }

    /// herdr `copy_or_terminal_mode`: copy 모드 pane에 포커스가 있으면 Terminal 대신 Copy, 떠나면 Terminal(상태는 둔다).
    /// 그 pane이 없어지면 끝낸다.
    pub(crate) fn settle_copy(&mut self) {
        let Some(pane) = self.copy.as_ref().map(|c| c.pane.clone()) else { return };
        if !self.all_panes().contains(&pane) {
            self.copy = None;
            if self.mode == Mode::Copy {
                self.mode = Mode::Terminal;
            }
            return;
        }
        // herdr: 떠나면 고른 칸을 지우고, 돌아오면 copy 모드의 고르기로 다시 칠한다
        let here = self.focus.as_deref() == Some(pane.as_str());
        match self.mode {
            Mode::Terminal if here => {
                self.mode = Mode::Copy;
                if self.selection.is_none() {
                    self.copy_sync();
                }
            }
            Mode::Copy if !here => {
                self.mode = Mode::Terminal;
                if self.selection.as_ref().is_some_and(|s| s.pane == pane) {
                    self.selection = None;
                }
            }
            _ => {}
        }
    }

    /// copy 모드 키(herdr `route_copy_mode_key`). prefix는 prefix 모드로 가고 명령 뒤 copy 모드로 돌아온다.
    pub(crate) async fn on_copy_key(&mut self, c: Combo, tx: &mut FrontSender) -> Result<()> {
        if self.is_prefix(c) {
            self.mode = Mode::Prefix;
            return Ok(());
        }
        let ctrl = c.mods == KeyModifiers::CONTROL;
        match c.code {
            // 고르기가 있으면 지우기만, 없으면 나간다
            KeyCode::Esc => {
                if let Some(copy) = self.copy.as_mut().filter(|c| c.select.is_some()) {
                    copy.select = None;
                    self.selection = None;
                    return Ok(());
                }
                return self.exit_copy(tx, false).await;
            }
            KeyCode::Enter => return self.exit_copy(tx, true).await,
            KeyCode::Left => return self.copy_move(tx, 0, -1).await,
            KeyCode::Down => return self.copy_move(tx, 1, 0).await,
            KeyCode::Up => return self.copy_move(tx, -1, 0).await,
            KeyCode::Right => return self.copy_move(tx, 0, 1).await,
            KeyCode::PageUp => return self.copy_page(tx, true, false).await,
            KeyCode::PageDown => return self.copy_page(tx, false, false).await,
            KeyCode::Home => {
                self.copy_col(0);
                return Ok(());
            }
            KeyCode::End => {
                self.copy_line(true);
                return Ok(());
            }
            KeyCode::Char('b') if ctrl => return self.copy_page(tx, true, false).await,
            KeyCode::Char('f') if ctrl => return self.copy_page(tx, false, false).await,
            KeyCode::Char('u') if ctrl => return self.copy_page(tx, true, true).await,
            KeyCode::Char('d') if ctrl => return self.copy_page(tx, false, true).await,
            _ => {}
        }
        match command_char(c) {
            Some('q') => self.exit_copy(tx, false).await?,
            Some('y') => self.exit_copy(tx, true).await?,
            Some('v' | ' ') => self.copy_begin(false),
            Some('V') => self.copy_begin(true),
            Some('h') => self.copy_move(tx, 0, -1).await?,
            Some('j') => self.copy_move(tx, 1, 0).await?,
            Some('k') => self.copy_move(tx, -1, 0).await?,
            Some('l') => self.copy_move(tx, 0, 1).await?,
            Some('g') => self.copy_history(tx, true).await?,
            Some('G') => self.copy_history(tx, false).await?,
            Some('0') => self.copy_col(0),
            Some('$') => self.copy_line(true),
            Some('^') => self.copy_line(false),
            Some('w') => self.copy_motion(tx, CopyMotion::NextWordStart).await?,
            Some('b') => self.copy_motion(tx, CopyMotion::PreviousWordStart).await?,
            Some('e') => self.copy_motion(tx, CopyMotion::NextWordEnd).await?,
            Some('W') => self.copy_motion(tx, CopyMotion::NextBigWordStart).await?,
            Some('B') => self.copy_motion(tx, CopyMotion::PreviousBigWordStart).await?,
            Some('E') => self.copy_motion(tx, CopyMotion::NextBigWordEnd).await?,
            Some('{') => self.copy_motion(tx, CopyMotion::PreviousParagraph).await?,
            Some('}') => self.copy_motion(tx, CopyMotion::NextParagraph).await?,
            _ => {}
        }
        Ok(())
    }

    /// 커서를 줄·칸만큼(herdr `move_copy_cursor`): 칸은 폭 안, 줄은 기록 맨 앞부터 화면 끝까지. 보이게 넘긴다.
    async fn copy_move(&mut self, tx: &mut FrontSender, rows: i64, cols: i64) -> Result<()> {
        let Some(pane) = self.copy.as_ref().map(|c| c.pane.clone()) else { return Ok(()) };
        let Some((w, h, max)) = self.copy_geometry(&pane) else { return Ok(()) };
        let Some(c) = self.copy.as_mut() else { return Ok(()) };
        let last = max + u32::from(h.max(1)) - 1;
        c.cursor.0 = (i64::from(c.cursor.0) + rows).clamp(0, i64::from(last)) as u32;
        c.cursor.1 = (i64::from(c.cursor.1) + cols).clamp(0, i64::from(w.saturating_sub(1))) as u16;
        self.copy_reveal(tx).await?;
        self.copy_sync();
        Ok(())
    }

    /// 쪽 넘기기(herdr `move_copy_page`): 커서와 기록 위치를 같이 옮긴다
    async fn copy_page(&mut self, tx: &mut FrontSender, up: bool, half: bool) -> Result<()> {
        let Some(pane) = self.copy.as_ref().map(|c| c.pane.clone()) else { return Ok(()) };
        let Some((_, h, max)) = self.copy_geometry(&pane) else { return Ok(()) };
        let Some(c) = self.copy.as_mut() else { return Ok(()) };
        let lines = page_lines(h, half);
        if up {
            c.cursor.0 = c.cursor.0.saturating_sub(lines);
            c.offset = (c.offset + lines).min(max);
        } else {
            c.cursor.0 = (c.cursor.0 + lines).min(max + u32::from(h.max(1)) - 1);
            c.offset = c.offset.saturating_sub(lines);
        }
        let offset = c.offset;
        self.scroll_to(tx, pane, offset as usize).await?;
        self.copy_sync();
        Ok(())
    }

    /// `g` 기록 맨 앞 줄·`G` 화면 끝 줄(herdr `move_copy_history`)
    async fn copy_history(&mut self, tx: &mut FrontSender, top: bool) -> Result<()> {
        let Some(pane) = self.copy.as_ref().map(|c| c.pane.clone()) else { return Ok(()) };
        let Some((_, h, max)) = self.copy_geometry(&pane) else { return Ok(()) };
        let Some(c) = self.copy.as_mut() else { return Ok(()) };
        (c.cursor.0, c.offset) = if top { (0, max) } else { (max + u32::from(h.max(1)) - 1, 0) };
        let offset = c.offset;
        self.scroll_to(tx, pane, offset as usize).await?;
        self.copy_sync();
        Ok(())
    }

    /// 커서가 화면 밖이면 그 줄이 맨 위·맨 아래에 오게 넘긴다(herdr `reveal_copy_cursor`)
    async fn copy_reveal(&mut self, tx: &mut FrontSender) -> Result<()> {
        let Some(pane) = self.copy.as_ref().map(|c| c.pane.clone()) else { return Ok(()) };
        let Some((_, h, max)) = self.copy_geometry(&pane) else { return Ok(()) };
        let Some(c) = self.copy.as_mut() else { return Ok(()) };
        let top = max.saturating_sub(c.offset);
        let rows = u32::from(h.saturating_sub(1));
        let want = if c.cursor.0 < top {
            c.cursor.0
        } else if c.cursor.0 > top + rows {
            c.cursor.0 - rows
        } else {
            top
        };
        let offset = max.saturating_sub(want);
        if offset != c.offset {
            c.offset = offset;
            self.scroll_to(tx, pane, offset as usize).await?;
        }
        Ok(())
    }

    /// 낱말·문단 이동(herdr `move_copy_word`·`move_copy_paragraph`): host에 묻고 답(`copy_apply`)이 올 때까지 키를 붙잡는다
    async fn copy_motion(&mut self, tx: &mut FrontSender, motion: CopyMotion) -> Result<()> {
        let Some(c) = self.copy.as_ref() else { return Ok(()) };
        let (pane, cursor) = (c.pane.clone(), TextPoint { row: c.cursor.0, col: c.cursor.1 });
        if !self.host_at_least(17) {
            return Ok(());
        }
        let id = tx.send(FrontRequest::PaneCopyMotion { pane, cursor, motion }).await?;
        self.pending.insert(id.clone(), Pending::CopyMotion);
        self.copy_wait = Some(id);
        Ok(())
    }

    /// host가 정한 자리로 커서를 옮기고 보이게 넘긴다. 그사이 copy 모드를 나갔거나 다른 pane이면 버린다.
    pub(crate) async fn copy_apply(&mut self, tx: &mut FrontSender) -> Result<()> {
        let Some((pane, cursor)) = self.copy_reply.take() else { return Ok(()) };
        let Some(c) = self.copy.as_mut().filter(|c| c.pane == pane) else { return Ok(()) };
        c.cursor = cursor;
        self.copy_reveal(tx).await?;
        self.copy_sync();
        Ok(())
    }

    fn copy_col(&mut self, col: u16) {
        if let Some(c) = self.copy.as_mut() {
            c.cursor.1 = col;
        }
        self.copy_sync();
    }

    /// `$` 마지막 글자 칸·`^` 첫 빈칸 아닌 칸. herdr는 서버에 묻고, aaa는 보낸 위치를 따라온 화면 frame의 그 줄에서 찾는다
    /// (아직 따라오지 않았으면 아무것도 하지 않는다).
    fn copy_line(&mut self, end: bool) {
        let Some(c) = self.copy.as_ref() else { return };
        let Some(g) = self.grids.get(&c.pane).filter(|g| g.scroll.offset == c.offset) else { return };
        let row = c.cursor.0.checked_sub(mouse::view_top(g.scroll));
        let Some(runs) = row.and_then(|r| g.lines.get(r as usize)) else { return };
        let col = line_cols(runs).map_or(0, |(first, last)| if end { last } else { first });
        self.copy_col(col);
    }

    /// `v`·space 글자 단위, `V` 줄 단위로 지금 커서에서 고르기 시작(herdr `begin_copy_selection`). 글자 단위는 움직여야 보인다.
    fn copy_begin(&mut self, line: bool) {
        let Some(c) = self.copy.as_mut() else { return };
        c.select = Some(if line { Select::Line(c.cursor.0) } else { Select::Char(c.cursor) });
        self.selection = None;
        if line {
            self.copy_sync();
        }
    }

    /// 커서가 움직인 뒤 고른 범위를 맞춘다(herdr `sync_copy_selection`)
    fn copy_sync(&mut self) {
        let Some(c) = self.copy.as_ref() else { return };
        let Some(select) = c.select else { return };
        let width = self.copy_geometry(&c.pane).map_or(1, |(w, _, _)| w);
        self.selection = Some(match select {
            Select::Char(anchor) => Selection { pane: c.pane.clone(), anchor, head: c.cursor, until: None },
            Select::Line(row) => line_range(c.pane.clone(), row, c.cursor.0, width.saturating_sub(1)),
        });
    }

    /// host가 올라가 있는 화면의 보던 줄을 지키려 기록이 는 만큼 위치를 올린다(§8.111). 보낸 위치(copy 모드·끌어 고르기)도
    /// 같이 올린다. 그 pane의 alt 화면이 바뀌면 줄 번호가 달라지므로 copy 모드의 고르기를 지운다(herdr).
    pub(crate) fn follow_grid(&mut self, pane: &str, before: aaa_proto::GridScroll, alt_changed: bool) {
        let Some(after) = self.grids.get(pane).map(|g| g.scroll) else { return };
        let grew = after.max.saturating_sub(before.max);
        if let Some(c) = self.copy.as_mut().filter(|c| c.pane == pane) {
            if c.offset > 0 {
                c.offset += grew;
            }
            c.offset = c.offset.min(after.max);
            if alt_changed {
                c.select = None;
                self.selection = None;
            }
        }
        if let Some(mouse::Drag::Select { pane: p, offset: Some(o), .. }) = self.drag.as_mut()
            && p == pane
        {
            if *o > 0 {
                *o += grew;
            }
            *o = (*o).min(after.max);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// herdr `copy_mode_page_lines`·`copy_mode_command_char`·`first_non_blank_col`·`last_character_col`·`line_range`
    #[test]
    fn copy_mode_helpers_like_herdr() {
        assert_eq!(
            [(2, false), (29, false), (29, true), (1, true)].map(|(h, half)| page_lines(h, half)),
            [1, 27, 14, 1]
        );
        let k = |code, mods| command_char(Combo::new(code, mods));
        assert_eq!(k(KeyCode::Char('v'), KeyModifiers::SHIFT), Some('V'));
        assert_eq!(k(KeyCode::Char('$'), KeyModifiers::NONE), Some('$'));
        assert_eq!(k(KeyCode::Char('v'), KeyModifiers::CONTROL), None);
        assert_eq!(k(KeyCode::Enter, KeyModifiers::NONE), None);
        let runs = vec![GridRun { t: "  한글 ab  ".into(), fg: 0, bg: 0, a: 0 }];
        assert_eq!(line_cols(&runs), Some((2, 8)));
        assert_eq!(line_cols(&[GridRun { t: "   ".into(), fg: 0, bg: 0, a: 0 }]), None);
        let s = line_range("p".into(), 5, 3, 79);
        assert_eq!(s.range(), ((3, 0), (5, 79)));
        assert!(s.covers(0, 4, 40) && s.covers(0, 3, 0) && !s.covers(0, 6, 0));
    }
}
