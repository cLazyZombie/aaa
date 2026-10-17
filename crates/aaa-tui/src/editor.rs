//! 한 줄 글 편집(이름 입력 창). herdr 0.9.5 `src/client/shell/text_editor.rs` `TextEditor`를 옮겼다:
//! grapheme 단위 커서, 처음 치면 바꾸기(`replace_on_type`), emacs식 이동·지우기·kill/yank, 넓은 글자 viewport.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use aaa_proto::layout::Rect;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Editor {
    text: String,
    cursor: usize,
    replace_on_type: bool,
    killed: String,
}

impl Editor {
    /// `replace_on_type`면 처음 친 글이 넣어 둔 글을 바꾼다(herdr 새 탭 이름 기본값)
    pub fn new(text: &str, replace_on_type: bool) -> Editor {
        let mut e = Editor::default();
        e.insert(text);
        e.replace_on_type = replace_on_type;
        e
    }

    #[cfg(test)]
    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn trim(&self) -> &str {
        self.text.trim()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.replace_on_type = false;
    }

    /// 넣고 지운 뒤 커서가 합쳐진 grapheme 가운데에 남지 않게 앞으로 맞춘다
    fn repair_cursor(&mut self) {
        self.cursor =
            self.text.grapheme_indices(true).map(|(i, _)| i).find(|i| *i >= self.cursor).unwrap_or(self.text.len());
    }

    /// 글 넣기(붙여넣기도). 줄바꿈·탭은 빈칸, 다른 제어 문자는 뺀다. 바뀌었으면 true.
    pub fn insert(&mut self, text: &str) -> bool {
        let mut normalized = String::new();
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    normalized.push(' ');
                }
                '\n' | '\t' => normalized.push(' '),
                ch if !ch.is_control() => normalized.push(ch),
                _ => {}
            }
        }
        if normalized.is_empty() {
            return false;
        }
        let changed = !self.replace_on_type || self.text != normalized;
        if self.replace_on_type {
            self.clear();
        }
        self.text.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
        self.repair_cursor();
        changed
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor].grapheme_indices(true).next_back().map(|(i, _)| i).unwrap_or(0)
    }

    fn next(&self) -> usize {
        self.cursor + self.text[self.cursor..].graphemes(true).next().map(str::len).unwrap_or(0)
    }

    /// 낱말 경계: 빈칸·글자(영숫자와 _)·기호를 다른 종류로 본다
    fn word_boundary(&self, backward: bool) -> usize {
        let class = |g: &str| {
            let ch = g.chars().next().unwrap_or(' ');
            if ch.is_whitespace() {
                0
            } else if ch.is_alphanumeric() || ch == '_' {
                1
            } else {
                2
            }
        };
        let mut boundary = self.cursor;
        let mut run = 0;
        if backward {
            for (i, g) in self.text[..self.cursor].grapheme_indices(true).rev() {
                let c = class(g);
                if run != 0 && c != run {
                    break;
                }
                run = c;
                boundary = i;
            }
        } else {
            for (i, g) in self.text[self.cursor..].grapheme_indices(true) {
                let c = class(g);
                if run != 0 && c != run {
                    break;
                }
                run = c;
                boundary = self.cursor + i + g.len();
            }
        }
        boundary
    }

    fn remove(&mut self, start: usize, end: usize, kill: bool) {
        self.replace_on_type = false;
        if start == end {
            return;
        }
        if kill {
            self.killed = self.text[start..end].to_owned();
        }
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.repair_cursor();
    }

    /// 편집 키. 모르는 키면 None, 아니면 글이 바뀌었는지. Enter·Esc는 창이 받는다.
    pub fn key(&mut self, code: KeyCode, mods: KeyModifiers) -> Option<bool> {
        if matches!(code, KeyCode::Enter | KeyCode::Esc) {
            return None;
        }
        let before = self.text.len();
        let mut changed = false;
        let ctrl = mods == KeyModifiers::CONTROL;
        let alt = mods == KeyModifiers::ALT;
        let plain = mods.is_empty();
        let movement = match code {
            KeyCode::Left if plain => Some(self.previous()),
            KeyCode::Char('b') if ctrl => Some(self.previous()),
            KeyCode::Right if plain => Some(self.next()),
            KeyCode::Char('f') if ctrl => Some(self.next()),
            KeyCode::Home if plain => Some(0),
            KeyCode::Char('a') if ctrl => Some(0),
            KeyCode::End if plain => Some(self.text.len()),
            KeyCode::Char('e') if ctrl => Some(self.text.len()),
            KeyCode::Char('b') if alt => Some(self.word_boundary(true)),
            KeyCode::Char('f') if alt => Some(self.word_boundary(false)),
            _ => None,
        };
        if let Some(at) = movement {
            self.cursor = at;
            self.replace_on_type = false;
        } else {
            match code {
                KeyCode::Backspace if plain => {
                    if self.replace_on_type {
                        self.clear();
                    } else {
                        self.remove(self.previous(), self.cursor, false);
                    }
                }
                KeyCode::Char('h') if ctrl => {
                    if self.replace_on_type {
                        self.clear();
                    } else {
                        self.remove(self.previous(), self.cursor, false);
                    }
                }
                KeyCode::Delete if plain => self.remove(self.cursor, self.next(), false),
                KeyCode::Char('d') if ctrl => self.remove(self.cursor, self.next(), false),
                KeyCode::Char('u') if ctrl => self.remove(0, self.cursor, true),
                KeyCode::Char('k') if ctrl => self.remove(self.cursor, self.text.len(), true),
                KeyCode::Char('w') if ctrl => self.remove(self.word_boundary(true), self.cursor, true),
                KeyCode::Backspace if ctrl || alt => self.remove(self.word_boundary(true), self.cursor, true),
                KeyCode::Char('d') if alt => self.remove(self.cursor, self.word_boundary(false), true),
                KeyCode::Char('y') if ctrl => changed = self.insert(&self.killed.clone()),
                KeyCode::Char(ch) if mods.difference(KeyModifiers::SHIFT).is_empty() => {
                    changed = self.insert(&ch.to_string())
                }
                _ => return None,
            }
        }
        Some(changed || self.text.len() != before)
    }

    /// 폭 `width` 칸에 보일 부분과 그 안의 커서 칸(커서가 늘 보이게 민다)
    pub fn viewport(&self, width: u16) -> (&str, u16) {
        if width == 0 {
            return ("", 0);
        }
        let mut start = self.cursor;
        let mut cells = 0;
        for (i, g) in self.text[..self.cursor].grapheme_indices(true).rev() {
            let next = cells + g.width();
            if next >= usize::from(width) {
                break;
            }
            start = i;
            cells = next;
        }
        let mut end = self.cursor;
        let mut used = cells;
        for (i, g) in self.text[self.cursor..].grapheme_indices(true) {
            used += g.width();
            if used > usize::from(width) {
                break;
            }
            end = self.cursor + i + g.len();
        }
        (&self.text[start..end], cells as u16)
    }
}

/// 한 줄 칸에 그리고 커서 자리를 준다(herdr `text_editor::render`)
pub fn render(buf: &mut Buffer, area: Rect, e: &Editor, style: Style) -> Option<(u16, u16)> {
    if area.w == 0 || area.h == 0 {
        return None;
    }
    let (text, cursor) = e.viewport(area.w);
    for x in area.x..area.x + area.w {
        buf.set_string(x, area.y, " ", style);
    }
    buf.set_stringn(area.x, area.y, text, usize::from(area.w), style);
    Some((area.x + cursor, area.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(e: &mut Editor, code: KeyCode, mods: KeyModifiers) -> bool {
        e.key(code, mods).expect("편집 키")
    }

    #[test]
    fn first_keystroke_replaces_the_suggested_name() {
        // herdr 새 탭: 기본 이름 "3"이 골라져 있고 처음 친 글이 그것을 바꾼다
        let mut e = Editor::new("3", true);
        assert!(k(&mut e, KeyCode::Char('작'), KeyModifiers::NONE));
        assert!(k(&mut e, KeyCode::Char('업'), KeyModifiers::NONE));
        assert_eq!(e.as_str(), "작업");
        // 처음 Backspace는 통째로 지운다
        let mut e = Editor::new("3", true);
        k(&mut e, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(e.as_str(), "");
        // 이동하면 바꾸기를 끝내고 이어 쓴다
        let mut e = Editor::new("3", true);
        k(&mut e, KeyCode::End, KeyModifiers::NONE);
        k(&mut e, KeyCode::Char('a'), KeyModifiers::NONE);
        assert_eq!(e.as_str(), "3a");
    }

    #[test]
    fn emacs_moves_kills_and_yank() {
        let mut e = Editor::new("build web-ui", false);
        k(&mut e, KeyCode::Char('w'), KeyModifiers::CONTROL);
        assert_eq!(e.as_str(), "build web-");
        k(&mut e, KeyCode::Char('a'), KeyModifiers::CONTROL);
        k(&mut e, KeyCode::Char('y'), KeyModifiers::CONTROL);
        assert_eq!(e.as_str(), "uibuild web-");
        // yank 뒤 커서는 넣은 글 뒤: ^k는 그 뒤를 지운다
        k(&mut e, KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(e.as_str(), "ui");
        k(&mut e, KeyCode::Char('a'), KeyModifiers::CONTROL);
        k(&mut e, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(e.as_str(), "ui");
        k(&mut e, KeyCode::Char('e'), KeyModifiers::CONTROL);
        k(&mut e, KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(e.as_str(), "");
        assert_eq!(e.key(KeyCode::Enter, KeyModifiers::NONE), None);
        assert_eq!(e.key(KeyCode::F(2), KeyModifiers::NONE), None);
    }

    #[test]
    fn graphemes_and_wide_viewport() {
        // 한글·결합 문자는 grapheme 하나씩 지운다
        let mut e = Editor::new("가e\u{301}나", false);
        k(&mut e, KeyCode::Left, KeyModifiers::NONE);
        k(&mut e, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(e.as_str(), "가나");
        // 붙여넣은 줄바꿈은 빈칸, 제어 문자는 뺀다
        let mut e = Editor::new("", false);
        e.insert("a\r\nb\tc\u{7}");
        assert_eq!(e.as_str(), "a b c");
        // 넓은 글자: 커서가 보이게 앞을 자른다(커서 칸 하나는 남긴다)
        let e = Editor::new("한글이름길다", false);
        let (text, cursor) = e.viewport(7);
        assert_eq!((text, cursor), ("름길다", 6));
        let e = Editor::new("abc", false);
        assert_eq!(e.viewport(10), ("abc", 3));
    }
}
