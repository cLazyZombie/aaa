//! aaa: Windows 콘솔의 bracketed paste 복원. herdr 0.9.5
//! client/input/windows_vti.rs와 같이 키 기록의 Unicode를 모아 한 Paste로 낸다.
use std::collections::VecDeque;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Item<T> {
    Key(T),
    Paste(String),
}

const START: &[u16] = &[27, 91, 50, 48, 48, 126];
const END: &[u16] = &[27, 91, 50, 48, 49, 126];
// 실제 Escape와 분할된 터미널 표지를 구분하는 입력 프로토콜 시한(시험은 시계를 직접 준다).
const ESCAPE_WINDOW: Duration = Duration::from_millis(25);
const MAX_PASTE: usize = 16 << 20;

pub(super) struct Paste<T> {
    prefix: Vec<T>,
    text: Option<Vec<u16>>,
    deadline: Option<Instant>,
    pub(super) ready: VecDeque<Item<T>>,
}

impl<T> Default for Paste<T> {
    fn default() -> Self {
        Self { prefix: Vec::new(), text: None, deadline: None, ready: VecDeque::new() }
    }
}

impl<T> Paste<T> {
    pub(super) fn wait(&self, now: Instant) -> Option<Duration> {
        self.deadline.map(|d| d.saturating_duration_since(now))
    }

    pub(super) fn expire(&mut self, now: Instant) {
        if self.deadline.is_some_and(|d| now >= d) {
            self.flush();
        }
    }

    fn flush(&mut self) {
        self.deadline = None;
        for record in self.prefix.drain(..) {
            self.ready.push_back(Item::Key(record));
        }
    }

    pub(super) fn key(
        &mut self,
        record: T,
        unicode: u16,
        down: bool,
        repeat: u16,
        modified: bool,
        now: Instant,
    ) -> std::io::Result<()> {
        if let Some(text) = self.text.as_mut() {
            // Win32 paste는 key-up과 modifier 기록도 섞는다. Unicode가 있는 누름만 본다.
            if down && unicode != 0 {
                for _ in 0..repeat.max(1) {
                    text.push(unicode);
                }
                if text.ends_with(END) {
                    text.truncate(text.len() - END.len());
                    self.ready.push_back(Item::Paste(String::from_utf16_lossy(text)));
                    self.text = None;
                } else if text.len() > MAX_PASTE {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "붙여넣기가 16Mi자를 넘었다"));
                }
            }
            return Ok(());
        }
        // ESC 다음 표지 글자만 잠깐 보류한다. 다른 키·수정키는 원래 순서로 돌려준다.
        if self.prefix.is_empty() && (!down || unicode != 27 || modified) {
            self.ready.push_back(Item::Key(record));
            return Ok(());
        }
        if !down {
            return Ok(());
        }
        let i = self.prefix.len();
        let matches = down && unicode == START[i] && repeat <= 1;
        self.prefix.push(record);
        if !matches {
            self.flush();
        } else if self.prefix.len() == START.len() {
            self.prefix.clear();
            self.deadline = None;
            self.text = Some(Vec::new());
        } else if self.deadline.is_none() {
            self.deadline = Some(now + ESCAPE_WINDOW);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aaa_native_paste_frames_and_preserves_keys() {
        let now = Instant::now();
        let mut p = Paste::default();
        assert_eq!(p.wait(now), None);
        // 실제 콘솔처럼 한 UTF-16 unit씩 누름·뗌이 번갈아 온다. 그림-only는 빈 Paste다.
        for text in ["", "한글🙂\r\n다음\t줄"] {
            for unit in format!("\x1b[200~{text}\x1b[201~").encode_utf16() {
                p.key(unit, unit, true, 1, false, now).unwrap();
                p.key(unit, unit, false, 1, false, now).unwrap();
            }
            assert_eq!(p.ready.pop_front(), Some(Item::Paste(text.into())));
            // 끝 표지 마지막 key-up은 보통 key처럼 돌아오며 TUI는 release를 무시한다.
            assert_eq!(p.ready.pop_front(), Some(Item::Key(126)));
            assert!(p.ready.is_empty());
        }
        // 단독 Esc는 프로토콜 시한 뒤, 표지가 아닌 ESC [ A는 원래 순서대로다.
        p.key(27, 27, true, 1, false, now).unwrap();
        p.expire(now);
        assert!(p.ready.is_empty());
        assert_eq!(p.wait(now), Some(ESCAPE_WINDOW));
        p.expire(now + ESCAPE_WINDOW);
        assert_eq!(p.ready.pop_front(), Some(Item::Key(27)));
        for u in "\x1b[A".encode_utf16() {
            p.key(u, u, true, 1, false, now).unwrap();
        }
        assert_eq!(p.ready.drain(..).collect::<Vec<_>>(), vec![Item::Key(27), Item::Key(91), Item::Key(65)]);
        // 표지가 진행 중일 때 OS resize/focus 등으로 호출이 끊겨도 상태는 보존된다.
        for u in START {
            p.key(*u, *u, true, 1, false, now).unwrap();
        }
        p.expire(now + Duration::from_secs(60));
        assert!(p.ready.is_empty());
        for u in END {
            p.key(*u, *u, true, 1, false, now).unwrap();
        }
        assert_eq!(p.ready.pop_front(), Some(Item::Paste(String::new())));
    }
}
