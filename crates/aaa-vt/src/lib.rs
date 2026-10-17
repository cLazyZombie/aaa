//! VT adapter. 정본 화면 상태는 session host가 갖는다(기획서 §4.5).
//!
//! 기획서는 ghostty-vt를 1순위로 두었지만 빌드에 Zig 0.16이 필요해 M1-1은 순수 Rust `vt100`을 쓴다.
//! 이 adapter 뒤에서 엔진을 바꿀 수 있게 front·host 쪽은 replay bytes와 글자 화면만 본다.

use aaa_proto::{GridRun, ScreenText};

pub struct Vt {
    parser: vt100::Parser<Modes>,
}

/// vt100이 화면에 담지 않는 입력 모드. replay가 다시 말해 늦게 붙은 front도 프로그램이 요청한 형식으로 키를 보낸다.
#[derive(Default)]
struct Modes {
    /// modifyOtherKeys 단계(`CSI > 4 ; n m`). Claude Code는 2를 요청하고 Ctrl+Enter를 `CSI 27;5;13~`로 받는다.
    modify_other_keys: u16,
}

impl vt100::Callbacks for Modes {
    fn unhandled_csi(&mut self, _: &mut vt100::Screen, i1: Option<u8>, i2: Option<u8>, params: &[&[u16]], c: char) {
        if i1 != Some(b'>') || i2.is_some() || !matches!(c, 'm' | 'n') {
            return;
        }
        // herdr-web-ui keys.ts modifyOtherKeysLevel과 같다: 빈 `CSI > m`·`CSI > 0 m`은 모두 끈다
        let first = params.first().and_then(|p| p.first()).copied();
        if params.is_empty() || (params.len() == 1 && first == Some(0)) {
            self.modify_other_keys = 0;
            return;
        }
        if first != Some(4) {
            return;
        }
        let value = params.get(1).and_then(|p| p.first()).copied();
        self.modify_other_keys = match (c, value) {
            ('m', Some(v)) => v,
            _ => 0,
        };
    }
}

/// scrollback 줄 수(설정 정책은 M1-2에서 TOML로)
pub const SCROLLBACK: usize = 5000;

impl Vt {
    pub fn new(rows: u16, cols: u16) -> Self {
        Vt { parser: vt100::Parser::new_with_callbacks(rows.max(1), cols.max(1), SCROLLBACK, Modes::default()) }
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
        let level = self.parser.callbacks().modify_other_keys;
        if level > 0 {
            out.extend_from_slice(format!("\x1b[>4;{level}m").as_bytes());
        }
        out
    }

    /// 한 줄을 같은 모양 글자 묶음으로(TUI front가 칸에 그린다). 넓은 글자의 뒤 칸은 건너뛴다.
    pub fn grid_row(&self, row: u16) -> Vec<GridRun> {
        let screen = self.parser.screen();
        let (_, cols) = screen.size();
        let mut runs: Vec<GridRun> = Vec::new();
        for col in 0..cols {
            let Some(cell) = screen.cell(row, col) else { continue };
            if cell.is_wide_continuation() {
                continue;
            }
            let t = if cell.has_contents() { cell.contents() } else { " " };
            let a = u8::from(cell.bold())
                | (u8::from(cell.italic()) << 1)
                | (u8::from(cell.underline()) << 2)
                | (u8::from(cell.inverse()) << 3);
            let (fg, bg) = (color(cell.fgcolor()), color(cell.bgcolor()));
            match runs.last_mut() {
                Some(r) if r.fg == fg && r.bg == bg && r.a == a => r.t.push_str(t),
                _ => runs.push(GridRun { t: t.to_string(), fg, bg, a }),
            }
        }
        // 줄 끝의 기본 모양 공백은 보내지 않는다
        if let Some(r) = runs.last_mut()
            && r.fg == 0
            && r.bg == 0
            && r.a == 0
        {
            let trimmed = r.t.trim_end_matches(' ').len();
            r.t.truncate(trimmed);
            if r.t.is_empty() {
                runs.pop();
            }
        }
        runs
    }

    /// (줄, 칸, 보이는가)
    pub fn cursor(&self) -> (u16, u16, bool) {
        let screen = self.parser.screen();
        let (r, c) = screen.cursor_position();
        (r, c, !screen.hide_cursor())
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

fn color(c: vt100::Color) -> u32 {
    match c {
        vt100::Color::Default => 0,
        vt100::Color::Idx(i) => 0x0100_0000 | u32::from(i),
        vt100::Color::Rgb(r, g, b) => 0x0200_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
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
    fn grid_runs_group_styles_and_skip_wide_continuation() {
        let mut v = Vt::new(3, 20);
        v.feed("a한\x1b[1;31mB\x1b[0m  ".as_bytes());
        let row = v.grid_row(0);
        assert_eq!(row.len(), 2);
        assert_eq!((row[0].t.as_str(), row[0].fg, row[0].a), ("a한", 0, 0));
        assert_eq!((row[1].t.as_str(), row[1].fg, row[1].a), ("B", 0x0100_0001, 1));
        assert!(v.grid_row(1).is_empty());
        assert_eq!(v.cursor(), (0, 6, true));
    }

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

    #[test]
    fn replay_says_modify_other_keys_again() {
        let mut v = Vt::new(5, 20);
        assert!(!v.replay().windows(4).any(|w| w == b"\x1b[>4"));
        v.feed(b"\x1b[>4;2m");
        assert!(v.replay().ends_with(b"\x1b[>4;2m"));
        // 다른 자원(1번)은 단계를 바꾸지 않는다
        v.feed(b"\x1b[>1;1m");
        assert!(v.replay().ends_with(b"\x1b[>4;2m"));
        v.feed(b"\x1b[>4n");
        assert!(!v.replay().windows(4).any(|w| w == b"\x1b[>4"));
        v.feed(b"\x1b[>4;1m\x1b[>m");
        assert!(!v.replay().windows(4).any(|w| w == b"\x1b[>4"));
    }
}
