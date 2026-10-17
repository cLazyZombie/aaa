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

/// replay가 앞에 싣는 기록 줄 상한. 웹 xterm scrollback(Term.tsx 2000)과 같다.
/// 붙을 때마다 보내므로 5000줄 다 싣지 않는다(색이 많은 120칸 5000줄이면 약 650KB, 2000줄이면 약 270KB).
pub const REPLAY_HISTORY: usize = 2000;

/// replay가 앞에 싣는 기록 bytes 상한. snapshot은 JSON 한 frame(base64, `aaa_proto::MAX_FRAME` 1MiB)으로 session host →
/// agent → host를 지나므로, 줄마다 색이 바뀌는 넓은 줄이 많아도 화면과 함께 그 안에 들게 최근 줄만 싣는다.
pub const REPLAY_HISTORY_BYTES: usize = 384 << 10;

impl Vt {
    pub fn new(rows: u16, cols: u16) -> Self {
        Vt { parser: vt100::Parser::new_with_callbacks(rows.max(1), cols.max(1), SCROLLBACK, Modes::default()) }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// 크기를 바꾼다. 줄이 줄어 커서 줄이 새 높이 밖으로 나가면, 먼저 그만큼 위 줄을 기록으로 올린다.
    /// vt100 `set_size`는 아래 줄을 잘라 버려(커서 줄과 프롬프트가 사라진다) xterm·herdr(ghostty)와 달랐다(plan §8.59).
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let (old_rows, _) = self.parser.screen().size();
        let (cursor_row, cursor_col) = self.parser.screen().cursor_position();
        let lift = (cursor_row + 1).saturating_sub(rows);
        if rows < old_rows && lift > 0 {
            // 화면 전체를 lift줄 올린다: scroll region을 풀고 맨 아래에서 줄을 내린다(위 줄은 기록으로).
            // 프로그램은 SIGWINCH 뒤 region을 다시 건다. 커서는 같은 글 위(lift줄 위)로, 모양은 DECSC·DECRC로 되돌린다.
            let mut b = format!("\x1b7\x1b[r\x1b[{old_rows};1H").into_bytes();
            b.extend(std::iter::repeat_n(b'\n', usize::from(lift)));
            b.extend_from_slice(format!("\x1b8\x1b[{};{}H", cursor_row - lift + 1, cursor_col + 1).as_bytes());
            self.parser.process(&b);
        }
        self.parser.screen_mut().set_size(rows, cols);
    }

    pub fn size(&self) -> (u16, u16) {
        self.parser.screen().size()
    }

    /// 빈 터미널에 쓰면 지금 화면(글자·속성·커서·입력 모드)이 되는 bytes. 화면 위로 밀려난 줄(scrollback)도 먼저 쓴다.
    /// alternate screen이면 그 밑 normal screen(기록과 화면)을 먼저 그리고 alternate screen으로 들어간다: 붙은 front도 휠을
    /// 프로그램 화면에 맞게 나누고(plan §8.55, herdr `WheelRouting`), 프로그램이 나가면 그 밑 화면이 돌아온다.
    pub fn replay(&self) -> Vec<u8> {
        let screen = self.parser.screen();
        // 화면을 지우고 처음부터 그린다(재접속한 front의 이전 화면을 덮는다).
        let mut tail = b"\x1b[H\x1b[2J".to_vec();
        tail.extend_from_slice(&screen.state_formatted());
        let level = self.parser.callbacks().modify_other_keys;
        if level > 0 {
            tail.extend_from_slice(format!("\x1b[>4;{level}m").as_bytes());
        }
        let mut out = Vec::new();
        if screen.alternate_screen() {
            // vt100은 보이는 grid만 내준다: 사본에서 alternate screen을 나가 normal screen을 얻는다(1049l은 커서도 되돌린다)
            let (rows, cols) = screen.size();
            let mut normal = vt100::Parser::new(rows, cols, 0);
            *normal.screen_mut() = screen.clone();
            normal.process(b"\x1b[?1049l");
            let mut under = b"\x1b[H\x1b[2J".to_vec();
            under.extend_from_slice(&normal.screen().contents_formatted());
            under.extend_from_slice(b"\x1b[?1049h");
            out.extend_from_slice(&history(normal.screen(), history_budget(under.len() + tail.len())));
            out.extend_from_slice(&under);
        } else {
            out.extend_from_slice(&history(screen, history_budget(tail.len())));
        }
        out.extend_from_slice(&tail);
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

/// 화면 bytes가 `screen`일 때 기록에 쓸 수 있는 bytes: snapshot JSON(base64는 4/3배)이 한 frame에 들고 64KiB가 남게
fn history_budget(screen: usize) -> usize {
    REPLAY_HISTORY_BYTES.min((aaa_proto::MAX_FRAME / 4 * 3).saturating_sub(screen + (64 << 10)))
}

/// scrollback 줄을 오래된 것부터 한 줄씩 쓰고, 마지막 줄까지 화면 위로 밀어 올리는 bytes.
/// 빈 터미널이 이것을 받으면 그 scrollback에 같은 줄이 생긴다(plan §8.55: 기록은 session host vt가 갖고, 붙는 front에 채운다).
/// 화면 밖 줄이라 줄 이음(soft wrap)은 남기지 않는다.
fn history(screen: &vt100::Screen, budget: usize) -> Vec<u8> {
    let mut screen = screen.clone();
    let (rows, cols) = screen.size();
    // 크게 주면 실제 scrollback 길이로 줄어든다
    screen.set_scrollback(usize::MAX);
    let len = screen.scrollback().min(REPLAY_HISTORY);
    let mut out = Vec::new();
    if len == 0 {
        return out;
    }
    // 위로 k줄 올린 화면의 첫 줄이 끝에서 k번째 기록 줄이다: 한 화면씩 내려오며 모은다
    let mut lines: Vec<Vec<u8>> = Vec::with_capacity(len);
    let mut back = len;
    while back > 0 {
        screen.set_scrollback(back);
        let take = back.min(usize::from(rows));
        for mut row in screen.rows_formatted(0, cols).take(take) {
            row.extend_from_slice(b"\x1b[m\r\n");
            lines.push(row);
        }
        back -= take;
    }
    // bytes 상한 안의 최근 줄만
    let mut size = 0;
    let keep = lines.iter().rev().take_while(|l| {
        size += l.len();
        size <= budget
    });
    let first = lines.len() - keep.count();
    if first == lines.len() {
        return out;
    }
    for line in &lines[first..] {
        out.extend_from_slice(line);
    }
    // 커서는 마지막 기록 줄 다음 줄 맨 앞이다. rows-1번 더 내리면 기록이 모두 화면 위로 넘어간다
    out.extend(std::iter::repeat_n(b'\n', usize::from(rows) - 1));
    out
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
    fn replay_carries_scrollback_into_a_fresh_vt() {
        let mut a = Vt::new(5, 20);
        for i in 1..=30 {
            a.feed(format!("\x1b[1mline-{i}\x1b[m\r\n").as_bytes());
        }
        a.feed(b"tail");
        let mut b = Vt::new(5, 20);
        b.feed(&a.replay());
        assert_eq!(b.text("s").lines, a.text("s").lines);
        // 위로 26줄: 1~26은 기록, 27~30과 tail이 화면
        let mut s = b.parser.screen().clone();
        s.set_scrollback(usize::MAX);
        assert_eq!(s.scrollback(), 26);
        assert_eq!(s.rows(0, 20).next().unwrap(), "line-1");
        assert!(s.cell(0, 0).unwrap().bold());
        s.set_scrollback(1);
        assert_eq!(s.rows(0, 20).next().unwrap(), "line-26");
        // 상한을 넘는 기록은 최근 것만 싣는다
        let mut long = Vt::new(5, 20);
        for i in 1..=REPLAY_HISTORY + 100 {
            long.feed(format!("n{i}\r\n").as_bytes());
        }
        let mut d = Vt::new(5, 20);
        d.feed(&long.replay());
        let mut s = d.parser.screen().clone();
        s.set_scrollback(usize::MAX);
        assert_eq!(s.scrollback(), REPLAY_HISTORY);
        // 화면은 마지막 4줄과 빈 커서 줄이라 기록은 n1~n(N-4): 그 가운데 끝 REPLAY_HISTORY줄
        assert_eq!(s.rows(0, 20).next().unwrap(), format!("n{}", 100 - 4 + 1));
        // 칸마다 색이 바뀌는 넓은 줄이 가득해도 snapshot이 한 frame 안에 든다(base64는 4/3배)
        let mut dense = Vt::new(50, 250);
        for i in 0..REPLAY_HISTORY + 50 {
            let mut line = String::new();
            for c in 0..250 {
                line.push_str(&format!(
                    "\x1b[38;2;{};{};{}m{}",
                    (i + c) % 256,
                    c % 256,
                    i % 256,
                    char::from(b'a' + (c % 26) as u8)
                ));
            }
            dense.feed(format!("{line}\x1b[m\r\n").as_bytes());
        }
        let replay = dense.replay();
        assert!(replay.len() * 4 / 3 + 4096 < aaa_proto::MAX_FRAME, "replay {} bytes", replay.len());
        let mut e = Vt::new(50, 250);
        e.feed(&replay);
        assert_eq!(e.text("s").lines, dense.text("s").lines);
        let mut s = e.parser.screen().clone();
        s.set_scrollback(usize::MAX);
        assert!(s.scrollback() > 50, "기록 {}줄", s.scrollback());
        // 기록이 없으면 앞에 아무것도 붙이지 않는다
        let mut c = Vt::new(5, 20);
        c.feed(b"x");
        assert!(c.replay().starts_with(b"\x1b[H\x1b[2J"));
    }

    #[test]
    fn replay_keeps_alternate_screen_over_normal_screen() {
        let mut a = Vt::new(5, 20);
        for i in 1..=10 {
            a.feed(format!("n{i}\r\n").as_bytes());
        }
        a.feed(b"\x1b[?1049h\x1b[Halt-screen");
        let mut b = Vt::new(5, 20);
        b.feed(&a.replay());
        assert!(b.parser.screen().alternate_screen());
        assert_eq!(b.text("s").lines, a.text("s").lines);
        // 프로그램이 나가면 그 밑 normal screen과 기록이 돌아온다
        a.feed(b"\x1b[?1049l");
        b.feed(b"\x1b[?1049l");
        assert_eq!(b.text("s").lines, a.text("s").lines);
        let mut s = b.parser.screen().clone();
        s.set_scrollback(usize::MAX);
        assert_eq!(s.rows(0, 20).next().unwrap(), "n1");
    }

    #[test]
    fn shrinking_lifts_lines_into_history_and_keeps_the_cursor_line() {
        let mut v = Vt::new(10, 20);
        for i in 1..=9 {
            v.feed(format!("l{i}\r\n").as_bytes());
        }
        v.feed(b"\x1b[1m$ cmd");
        // 커서는 9번째 줄(0부터) 6칸. 5줄로 줄이면 위 5줄이 기록으로 가고 프롬프트 줄이 맨 아래에 남는다
        v.resize(5, 20);
        let lines = v.text("s").lines;
        assert_eq!(lines, vec!["l6", "l7", "l8", "l9", "$ cmd"]);
        assert_eq!(v.cursor(), (4, 5, true));
        let mut s = v.parser.screen().clone();
        s.set_scrollback(usize::MAX);
        assert_eq!(s.scrollback(), 5);
        assert_eq!(s.rows(0, 20).next().unwrap(), "l1");
        // 글자 모양(굵게)도 그대로: 뒤에 쓴 글이 굵다
        v.feed(b"x");
        assert!(v.parser.screen().cell(4, 5).unwrap().bold());
        // 커서가 새 높이 안이면 아래 빈 줄만 줄인다(기록으로 올리지 않는다)
        let mut w = Vt::new(10, 20);
        w.feed(b"top\r\nnext");
        w.resize(5, 20);
        assert_eq!(w.text("s").lines[..2], ["top".to_string(), "next".to_string()]);
        let mut s = w.parser.screen().clone();
        s.set_scrollback(usize::MAX);
        assert_eq!(s.scrollback(), 0);
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
    fn replay_restores_input_modes_in_a_fresh_vt() {
        // 다시 붙은 화면도 화살표 모드(DECCKM)·bracketed paste·mouse 보고를 안다(차이 목록 「DECCKM」 재연결 뒤)
        for alt in [false, true] {
            let mut a = Vt::new(5, 20);
            if alt {
                a.feed(b"[?1049h");
            }
            a.feed(b"[?1h[?2004h[?1000h[?1006h");
            let mut b = Vt::new(5, 20);
            b.feed(&a.replay());
            let s = b.parser.screen();
            assert!(s.application_cursor() && s.bracketed_paste(), "alt={alt}");
            assert_eq!(s.mouse_protocol_mode(), vt100::MouseProtocolMode::PressRelease, "alt={alt}");
            assert_eq!(s.mouse_protocol_encoding(), vt100::MouseProtocolEncoding::Sgr, "alt={alt}");
            assert_eq!(s.alternate_screen(), alt);
            // 끄면 끈 채로
            a.feed(b"[?1l");
            let mut c = Vt::new(5, 20);
            c.feed(&a.replay());
            assert!(!c.parser.screen().application_cursor(), "alt={alt}");
        }
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
