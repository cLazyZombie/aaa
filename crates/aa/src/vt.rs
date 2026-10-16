//! VT adapter. 정본 화면 상태는 session host가 갖는다(기획서 §4.5).
//!
//! 기획서는 ghostty-vt를 1순위로 두었지만 빌드에 Zig 0.16이 필요해 M1-1은 순수 Rust `vt100`을 쓴다.
//! 이 adapter 뒤에서 엔진을 바꿀 수 있게 front·host 쪽은 replay bytes와 글자 화면만 본다.

use aa_proto::ScreenText;

pub struct Vt {
    parser: vt100::Parser,
}

/// scrollback 줄 수(설정 정책은 M1-2에서 TOML로)
pub const SCROLLBACK: usize = 5000;

impl Vt {
    pub fn new(rows: u16, cols: u16) -> Self {
        Vt { parser: vt100::Parser::new(rows.max(1), cols.max(1), SCROLLBACK) }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows.max(1), cols.max(1));
    }

    pub fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    /// 빈 터미널에 쓰면 지금 화면(글자·속성·커서·입력 모드)이 되는 bytes
    pub fn replay(&self) -> Vec<u8> {
        // 화면을 지우고 처음부터 그린다(재접속한 front의 이전 화면을 덮는다).
        let mut out = b"\x1b[H\x1b[2J".to_vec();
        out.extend_from_slice(&self.parser.screen().state_formatted());
        out
    }

    /// 커서 위치 보고(DSR 6의 답, 1부터 센다)
    pub fn cursor_report(&self) -> Vec<u8> {
        let (r, c) = self.parser.screen().cursor_position();
        format!("\x1b[{};{}R", r + 1, c + 1).into_bytes()
    }

    pub fn text(&self, session: &str) -> ScreenText {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();
        let lines = screen.rows(0, cols).map(|l| l.trim_end().to_string()).collect();
        let (cr, cc) = screen.cursor_position();
        ScreenText { session: session.to_string(), cols, rows, lines, cursor_row: cr, cursor_col: cc }
    }
}

/// 출력에서 커서 위치 질의(DSR 6, `ESC [ 6 n`)가 끝나는 위치를 찾는다. chunk 경계에 걸쳐도 찾는다.
///
/// session host가 터미널 역할이라 이 질의에 답해야 한다. Windows ConPTY(INHERIT_CURSOR)는 시작할 때
/// 이 질의를 보내고 답이 올 때까지 자식의 출력과 종료를 막는다.
#[derive(Default)]
pub struct DsrScanner {
    matched: usize,
}

const DSR: &[u8] = b"\x1b[6n";

impl DsrScanner {
    pub fn scan(&mut self, data: &[u8]) -> Vec<usize> {
        let mut ends = Vec::new();
        for (i, &b) in data.iter().enumerate() {
            if b == DSR[self.matched] {
                self.matched += 1;
                if self.matched == DSR.len() {
                    ends.push(i + 1);
                    self.matched = 0;
                }
            } else {
                self.matched = usize::from(b == DSR[0]);
            }
        }
        ends
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dsr_found_across_chunks_and_answered_with_cursor() {
        let mut s = DsrScanner::default();
        assert_eq!(s.scan(b"ab\x1b[6ncd"), vec![6]);
        assert_eq!(s.scan(b"x\x1b["), Vec::<usize>::new());
        assert_eq!(s.scan(b"6n\x1b\x1b[6n"), vec![2, 7]);
        assert_eq!(s.scan(b"\x1b[61n\x1b[5n"), Vec::<usize>::new());
        let mut v = Vt::new(5, 20);
        v.feed(b"hi\r\nthere");
        assert_eq!(v.cursor_report(), b"\x1b[2;6R");
    }

    #[test]
    fn replay_reproduces_screen_in_a_fresh_vt() {
        let mut a = Vt::new(5, 20);
        a.feed("한글 ok\r\n\x1b[31mred\x1b[0m\r\nthird".as_bytes());
        let mut b = Vt::new(5, 20);
        b.feed(b"garbage that must be cleared");
        b.feed(&a.replay());
        assert_eq!(a.text("s").lines, b.text("s").lines);
        assert_eq!(b.text("s").lines[0], "한글 ok");
        assert_eq!((b.text("s").cursor_row, b.text("s").cursor_col), (2, 5));
    }
}
