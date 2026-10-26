//! 터미널 질의와 화면 가림은 서로 기다리지 않는다.
//! ConPTY의 첫 DSR 응답이 인증 접두부 보류에 막히면 자식은 시작조차 못 한다.
use aaa_common::redact::LiveStream;
use aaa_proto::TermColors;
use aaa_vt::{DefaultColors, Found, Query, QueryScanner, QueryState};

pub(crate) struct Output {
    scanner: QueryScanner,
    cursor: Option<QueryState>,
    colors: DefaultColors,
    redactor: LiveStream,
}

impl Output {
    pub fn new(rows: u16, cols: u16, colors: Option<TermColors>) -> Self {
        Self {
            scanner: QueryScanner::default(),
            cursor: aaa_common::redact::active().then(|| QueryState::new(rows, cols)),
            colors: DefaultColors::new(colors),
            redactor: LiveStream::default(),
        }
    }

    pub fn feed(&mut self, data: &[u8], screen: &mut aaa_vt::Vt) -> (Vec<u8>, Vec<Vec<u8>>) {
        let scanned = self.scanner.scan(data);
        let mut start = 0;
        let mut replies = Vec::new();
        let mut clean = Vec::new();
        for (at, found) in scanned.found {
            self.advance(&scanned.out[start..at], screen, &mut clean);
            start = at;
            let reply = match found {
                Found::Query(Query::CursorPosition) => {
                    Some(self.cursor.as_ref().map_or_else(|| screen.cursor_report(), QueryState::cursor_report))
                }
                Found::Query(Query::DefaultColor(color, term)) => self.colors.reply(color, term),
                Found::Query(query) => query.fixed_reply().map(<[u8]>::to_vec),
                _ => {
                    self.colors.note(found);
                    None
                }
            };
            if let Some(reply) = reply {
                replies.push(reply);
            }
        }
        self.advance(&scanned.out[start..], screen, &mut clean);
        (clean, replies)
    }

    fn advance(&mut self, raw: &[u8], screen: &mut aaa_vt::Vt, clean: &mut Vec<u8>) {
        if let Some(cursor) = &mut self.cursor {
            cursor.feed(raw);
        }
        // 질의를 뺀 본문만 가린다. 질의 앞뒤 인증 조각도 같은 LiveStream에서 검사한다. 앞 최대 7바이트 선표시는 q-1285 승인 범위다.
        let safe = self.redactor.push(raw);
        screen.feed(&safe);
        clean.extend(safe);
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        if let Some(cursor) = &mut self.cursor {
            cursor.resize(rows, cols);
        }
    }
    pub fn host_colors(&mut self, colors: TermColors) {
        self.colors.host = Some(colors);
    }
    pub fn finish(&mut self) -> Vec<u8> {
        self.redactor.push(&self.scanner.take_held())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        output: Output,
        screen: aaa_vt::Vt,
    }
    impl Fixture {
        fn new(rows: u16, cols: u16, colors: Option<TermColors>) -> Self {
            aaa_common::redact::remember(&aaa_test_support::token("qzb-lab-output-cursor0910"));
            Self { output: Output::new(rows, cols, colors), screen: aaa_vt::Vt::new(rows, cols) }
        }
        fn feed(&mut self, bytes: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
            self.output.feed(bytes, &mut self.screen)
        }
        fn resize(&mut self, rows: u16, cols: u16) {
            self.output.resize(rows, cols);
            self.screen.resize(rows, cols);
        }
        fn finish(&mut self) -> Vec<u8> {
            self.output.finish()
        }
    }

    #[test]
    fn iphone_ascii_echo_reaches_screen_before_next_input() {
        for ch in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
            aaa_common::redact::remember(&format!("{ch}~~~~~~~~~~~"));
        }
        let mut output = Fixture::new(32, 120, None);
        let mut expected = String::new();
        for ch in "status aaa123! 한글".chars() {
            let text = ch.to_string();
            assert_eq!(output.feed(text.as_bytes()).0, text.as_bytes());
            expected.push(ch);
            assert!(output.screen.detection_text().contains(expected.trim_end()));
        }
        // 출력 쪽 수정이 입력 교정·붙여넣기 표시·Enter/Backspace 제어를 붙잡지 않는다.
        let controls = b"\x08 \x08\r\n\x1b[200~paste\x1b[201~\x1b[31m!\x1b[0m";
        assert_eq!(output.feed(controls).0, controls);
        assert!(output.finish().is_empty());
    }

    #[test]
    fn conpty_cursor_query_replies_before_any_further_output_despite_auth_suffix() {
        aaa_common::redact::remember(&format!("{}n", aaa_test_support::token("conpty")));
        let mut old = aaa_common::redact::Stream::default();
        assert_ne!(old.push(b"\x1b[6n", false), b"\x1b[6n", "원본 교착의 n 보류를 재현한다");
        for split in 0..=4 {
            let mut output = Fixture::new(32, 120, None);
            let (mut clean, mut replies) = output.feed(&b"\x1b[6n"[..split]);
            let (rest, more) = output.feed(&b"\x1b[6n"[split..]);
            clean.extend(rest);
            replies.extend(more);
            assert!(clean.is_empty());
            assert_eq!(replies, [b"\x1b[1;1R".to_vec()]);
        }
    }

    #[test]
    fn live_secret_masks_from_eighth_byte_and_keeps_query_cursor_correct() {
        let secret = &*format!("{}n", aaa_test_support::token("conpty"));
        aaa_common::redact::remember(secret);
        let mut output = Fixture::new(32, 120, None);
        let (mut clean, replies) = output.feed(&secret.as_bytes()[..7]);
        assert_eq!(clean, secret.as_bytes()[..7]);
        assert!(replies.is_empty());
        let (part, replies) = output.feed(b"\x1b[6n\x1b[c\x1b[>c");
        clean.extend(part);
        assert_eq!(replies, [b"\x1b[1;8R".to_vec(), b"\x1b[?62;22c".to_vec(), b"\x1b[>1;0;0c".to_vec()]);
        clean.extend(output.feed(&secret.as_bytes()[7..]).0);
        clean.extend(output.finish());
        assert_eq!(clean, [secret.as_bytes()[..7].to_vec(), vec![b'*'; secret.len() - 7]].concat());
        let mut screen = aaa_vt::Vt::new(32, 120);
        screen.feed(&clean);
        assert!(!screen.replay().windows(8).any(|w| secret.as_bytes().windows(8).any(|s| s == w)));
    }

    #[test]
    fn color_changes_and_resize_keep_query_order_while_the_record_tail_is_redacted() {
        let mut output = Fixture::new(4, 4, Some(TermColors { fg: [1, 2, 3], bg: [4, 5, 6] }));
        output.feed(b"abcdef");
        assert_eq!(output.feed(b"\x1b[6n").1, [b"\x1b[2;3R".to_vec()]);
        output.resize(4, 8);
        assert_eq!(output.feed(b"\x1b[6n").1, [b"\x1b[1;7R".to_vec()]);
        let (_, replies) = output.feed(b"\x1b]10;#aabbcc\x07\x1b]10;?\x07\x1b]110\x07\x1b]10;?\x07");
        assert_eq!(replies, [b"\x1b]10;rgb:aaaa/bbbb/cccc\x07".to_vec(), b"\x1b]10;rgb:0101/0202/0303\x1b\\".to_vec()]);
        let secret = &*aaa_test_support::token("partial-query-record-secret0910");
        aaa_common::redact::remember(secret);
        let mut output = Fixture::new(4, 8, None);
        let (mut clean, _) = output.feed(secret.as_bytes());
        clean.extend(output.feed(b"\x1b[").0);
        clean.extend(output.finish());
        assert_eq!(clean, [secret.as_bytes()[..7].to_vec(), vec![b'*'; secret.len() - 7], b"\x1b[".to_vec()].concat());
    }
}
