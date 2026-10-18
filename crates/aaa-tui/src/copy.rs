//! copy 모드(herdr 0.9.5 `src/client/shell/copy_mode.rs`·`src/copy_mode.rs`). `prefix [`로 포커스 pane에 들어가 키로 커서를
//! 옮기며 이 화면의 기록을 넘기고(`PaneScroll`), 고른 글은 끌어 고르기처럼 host가 기록까지 읽어 복사한다(`PaneSelectionRead`).
//! 커서·고르기는 기록 줄 번호(`mouse::Point`)로 둔다. 화면 위치는 frame을 기다리지 않고 보낸 값으로 셈한다(herdr와 같다).
//! 낱말·문단 이동(`w b e W B E { }`)은 herdr처럼 host가 화면 사본의 기록까지 보고 정하고(`PaneCopyMotion`), 답이 올 때까지
//! 다음 키를 붙잡아 그 자리에서 잇는다. 찾기(`/`·`?` 입력 줄, `n`·`N` 다시)도 host가 하고(`PaneCopySearch`) 답이 오면
//! 현재 일치로 커서를 옮기고 일치를 칠한다.
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

use aaa_proto::layout::Rect;
use aaa_proto::{CopyMotion, FrontRequest, GridRun, SearchDirection, TextPoint, TextRange};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::style::Style;

use crate::editor::{self, Editor};
use crate::keys::Combo;
use crate::mouse::{self, Point, Selection};
use crate::theme::Palette;
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
    /// 찾기 입력 줄(herdr `ClientCopySearchPrompt`): `/` 앞으로·`?` 뒤로
    pub prompt: Option<(SearchDirection, Editor)>,
    pub search: Search,
}

/// 마지막 찾기(herdr `search_query`·`search_direction`·`search_matches`·…). 화면 내용이 바뀌면 일치만 지우고 `n`·`N`은 남는다.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Search {
    pub query: String,
    /// `/`·`?`로 연 방향(`n`은 이 방향, `N`은 반대)
    pub direction: Option<SearchDirection>,
    /// 현재 일치 언저리(host가 최대 1024개)
    pub matches: Vec<TextRange>,
    pub total: u64,
    pub current: Option<usize>,
    pub current_global: Option<u64>,
}

impl Search {
    fn clear_matches(&mut self) {
        self.matches.clear();
        self.total = 0;
        self.current = None;
        self.current_global = None;
    }

    /// Esc가 지울 것이 있는가(herdr: 찾은 글·일치·방향 중 하나라도)
    fn is_active(&self) -> bool {
        !self.query.is_empty() || !self.matches.is_empty() || self.direction.is_some()
    }

    /// 막대의 ` n/m`(herdr `match_status`: 찾았으나 없으면 ` 0/0`, 찾지 않았으면 빈 글)
    pub fn status(&self) -> String {
        match self.current_global {
            Some(n) => format!(" {}/{}", n + 1, self.total),
            None if !self.query.is_empty() => " 0/0".into(),
            None => String::new(),
        }
    }
}

/// copy 모드 요청의 답(`copy_apply`가 커서·찾기에 넣는다)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyReply {
    Cursor { pane: String, cursor: Point },
    Search { pane: String, query: String, direction: SearchDirection, repeat: bool, found: Found },
}

/// `CopySearch` 답(herdr `ClientCopySearchResult`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub matches: Vec<TextRange>,
    pub total: u64,
    pub current: Option<u32>,
    pub current_global: Option<u64>,
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
        self.copy = Some(CopyMode {
            pane,
            cursor,
            offset,
            entry: offset,
            select: None,
            prompt: None,
            search: Search::default(),
        });
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
        // 찾기 입력 줄이 열려 있으면 키는 모두 그 줄로(prefix도, herdr)
        if self.copy.as_ref().is_some_and(|c| c.prompt.is_some()) {
            return self.on_prompt_key(c, tx).await;
        }
        if self.is_prefix(c) {
            self.mode = Mode::Prefix;
            return Ok(());
        }
        let ctrl = c.mods == KeyModifiers::CONTROL;
        match c.code {
            // 고르기나 찾기가 있으면 지우기만, 없으면 나간다
            KeyCode::Esc => {
                if let Some(copy) = self.copy.as_mut().filter(|c| c.select.is_some() || c.search.is_active()) {
                    copy.select = None;
                    copy.search = Search::default();
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
            Some('/') => self.open_prompt(SearchDirection::Forward),
            Some('?') => self.open_prompt(SearchDirection::Backward),
            Some('n') => self.repeat_search(tx, false).await?,
            Some('N') => self.repeat_search(tx, true).await?,
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
        self.copy_reveal(tx, false).await?;
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

    /// 커서가 화면 밖이면 그 줄이 맨 위·맨 아래에 오게 넘긴다(herdr `reveal_copy_cursor`). `bar`면 맨 아랫줄(모드 막대)
    /// 위까지만 쓴다(찾은 자리).
    async fn copy_reveal(&mut self, tx: &mut FrontSender, bar: bool) -> Result<()> {
        let Some(pane) = self.copy.as_ref().map(|c| c.pane.clone()) else { return Ok(()) };
        let Some((_, h, max)) = self.copy_geometry(&pane) else { return Ok(()) };
        let Some(c) = self.copy.as_mut() else { return Ok(()) };
        let top = max.saturating_sub(c.offset);
        let rows = u32::from(h.saturating_sub(if bar { 2 } else { 1 }));
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
        if !self.host_at_least(23) {
            return Ok(());
        }
        let id = tx.send(FrontRequest::PaneCopyMotion { pane, cursor, motion }).await?;
        self.pending.insert(id.clone(), Pending::CopyMotion);
        self.copy_wait = Some(id);
        Ok(())
    }

    /// host 답을 넣는다(herdr `apply_copy_motion_target`·`apply_copy_search_result`): 옮긴 자리, 찾았으면 현재 일치의 시작으로
    /// 커서를 옮기고 보이게 넘긴다. 그사이 copy 모드를 나갔거나 다른 pane이면 버린다.
    pub(crate) async fn copy_apply(&mut self, tx: &mut FrontSender) -> Result<()> {
        let bar = match self.copy_reply.take() {
            None => return Ok(()),
            Some(CopyReply::Cursor { pane, cursor }) => {
                let Some(c) = self.copy.as_mut().filter(|c| c.pane == pane) else { return Ok(()) };
                c.cursor = cursor;
                false
            }
            Some(CopyReply::Search { pane, query, direction, repeat, found }) => {
                let Some(c) = self.copy.as_mut().filter(|c| c.pane == pane) else { return Ok(()) };
                let Found { matches, total, current, current_global } = found;
                let current = current.map(|i| i as usize).filter(|i| *i < matches.len());
                c.search.query = query;
                if !repeat {
                    c.search.direction = Some(direction);
                }
                c.search = Search { matches, total, current, current_global, ..std::mem::take(&mut c.search) };
                let Some(at) = current.and_then(|i| c.search.matches.get(i)) else { return Ok(()) };
                c.cursor = (at.start.row, at.start.col);
                true
            }
        };
        self.copy_reveal(tx, bar).await?;
        self.copy_sync();
        Ok(())
    }

    /// `/`·`?`(herdr `open_copy_search`): 빈 입력 줄을 연다
    fn open_prompt(&mut self, direction: SearchDirection) {
        if let Some(c) = self.copy.as_mut() {
            c.prompt = Some((direction, Editor::default()));
        }
    }

    /// 입력 줄 키(herdr `route_copy_search_prompt_key`): Esc 닫기, Enter 찾기, 나머지는 편집
    async fn on_prompt_key(&mut self, k: Combo, tx: &mut FrontSender) -> Result<()> {
        let Some(c) = self.copy.as_mut() else { return Ok(()) };
        match k.code {
            KeyCode::Esc => c.prompt = None,
            KeyCode::Enter => {
                if let Some((direction, input)) = c.prompt.take() {
                    self.copy_search(tx, input.as_str().to_string(), direction, false).await?;
                }
            }
            _ => {
                if let Some((_, input)) = c.prompt.as_mut() {
                    input.key(k.code, k.mods);
                }
            }
        }
        Ok(())
    }

    /// 입력 줄에 붙여넣기(herdr `insert_copy_search_text`). 입력 줄이 없으면 false.
    pub(crate) fn prompt_paste(&mut self, text: &str) -> bool {
        let Some((_, input)) = self.copy.as_mut().and_then(|c| c.prompt.as_mut()) else { return false };
        input.insert(text);
        true
    }

    /// `n` 같은 방향·`N` 반대로 다시 찾기(herdr `repeat_copy_search`)
    async fn repeat_search(&mut self, tx: &mut FrontSender, reverse: bool) -> Result<()> {
        let Some(c) = self.copy.as_ref() else { return Ok(()) };
        let Some(direction) = c.search.direction.filter(|_| !c.search.query.is_empty()) else { return Ok(()) };
        let direction = match (direction, reverse) {
            (SearchDirection::Forward, true) => SearchDirection::Backward,
            (SearchDirection::Backward, true) => SearchDirection::Forward,
            (d, false) => d,
        };
        let query = c.search.query.clone();
        self.copy_search(tx, query, direction, true).await
    }

    /// 찾기 요청(herdr `dispatch_next_copy_operation`의 Search): 다시 찾기이고 커서가 현재 일치의 시작이면 그 일치 다음부터.
    /// 답이 올 때까지 키를 붙잡는다.
    async fn copy_search(
        &mut self,
        tx: &mut FrontSender,
        query: String,
        direction: SearchDirection,
        repeat: bool,
    ) -> Result<()> {
        let Some(c) = self.copy.as_ref().filter(|_| !query.is_empty()) else { return Ok(()) };
        let cursor = TextPoint { row: c.cursor.0, col: c.cursor.1 };
        let previous = repeat
            .then(|| c.search.current.and_then(|i| c.search.matches.get(i).copied()).filter(|m| m.start == cursor))
            .flatten();
        let pane = c.pane.clone();
        if !self.host_at_least(23) {
            return Ok(());
        }
        let req = FrontRequest::PaneCopySearch { pane, query: query.clone(), direction, cursor, previous };
        let id = tx.send(req).await?;
        self.pending.insert(id.clone(), Pending::CopySearch { query, direction, repeat });
        self.copy_wait = Some(id);
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

    /// host가 올라가 있는 화면의 보던 줄을 지키려 기록이 는 만큼 위치를 올린다(§8.142). 보낸 위치(copy 모드·끌어 고르기)도
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
            // herdr는 화면 내용이 바뀌면(content revision) 일치를 지운다. aaa frame에는 그 번호가 없어 기록이 늘거나
            // alt 화면이 바뀐 때 지운다
            if grew > 0 || alt_changed {
                c.search.clear_matches();
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

/// 찾기 입력 줄 막대(herdr `render_mode_bar` Copy의 search prompt): ` COPY ` 다음 `/`·`?`, 입력 칸, 넓으면 오른쪽에 안내.
/// 입력 칸은 `text` 글에 `panel_bg` 바탕, 커서 칸은 뒤집는다.
pub fn render_prompt(buf: &mut Buffer, bar: Rect, prompt: &(SearchDirection, Editor), p: &Palette, styles: [Style; 3]) {
    let [chip, key, base] = styles;
    let (direction, input) = prompt;
    buf.set_stringn(bar.x, bar.y, " COPY ", usize::from(bar.w), chip);
    let prefix = 8.min(bar.w);
    if bar.w >= 8 {
        buf.set_string(bar.x + 7, bar.y, if *direction == SearchDirection::Forward { "/" } else { "?" }, key);
    }
    let footer = "  enter 찾기  esc 취소";
    let footer_w = if bar.w >= 50 { crate::unicode_width(footer) as u16 } else { 0 };
    let field = Rect { x: bar.x + prefix, w: bar.w.saturating_sub(prefix + footer_w), ..bar };
    if let Some((x, y)) = editor::render(buf, field, input, Style::default().fg(p.text).bg(p.panel_bg))
        && let Some(cell) = buf.cell_mut((x, y))
    {
        cell.set_style(Style::default().fg(p.panel_bg).bg(p.text));
    }
    if footer_w > 0 {
        buf.set_string(bar.x + bar.w - footer_w, bar.y, footer, base);
    }
}

/// 찾은 일치 칠하기(herdr `render_client_copy_search_highlights`): `current`면 현재 일치만 accent 바탕 굵게, 아니면 나머지를
/// `surface1` 바탕에 `text` 글로. `r`은 pane 안쪽 칸, `top`은 화면 맨 위 줄의 기록 줄 번호.
pub fn paint_matches(buf: &mut Buffer, r: Rect, top: u32, search: &Search, p: &Palette, current: bool) {
    if r.w == 0 || r.h == 0 {
        return;
    }
    let bottom = top + u32::from(r.h) - 1;
    let style = if current {
        Style::default().fg(crate::theme::contrast(p)).bg(p.accent).add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        Style::default().fg(p.text).bg(p.surface1)
    };
    for (i, m) in search.matches.iter().enumerate() {
        if (search.current == Some(i)) != current || m.end.row < top || m.start.row > bottom {
            continue;
        }
        for row in m.start.row.max(top)..=m.end.row.min(bottom) {
            let from = if row == m.start.row { m.start.col } else { 0 };
            let to = if row == m.end.row { m.end.col } else { r.w - 1 }.min(r.w - 1);
            for col in from..=to {
                if let Some(cell) = buf.cell_mut((r.x + col, r.y + (row - top) as u16)) {
                    cell.set_style(style);
                }
            }
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
