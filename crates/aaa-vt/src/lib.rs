//! VT adapter. 정본 화면 상태는 session host가 갖는다(기획서 §4.5).
//!
//! 기획서는 ghostty-vt를 1순위로 두었지만 빌드에 Zig 0.16이 필요해 M1-1은 순수 Rust `vt100`을 쓴다.
//! 이 adapter 뒤에서 엔진을 바꿀 수 있게 front·host 쪽은 replay bytes와 글자 화면만 본다.

use aaa_proto::{GridRun, MouseEncoding, MouseMode, PaneModes, ReadFormat, ReadSource, ScreenText};

pub struct Vt {
    parser: vt100::Parser<Modes>,
}

/// vt100이 화면에 담지 않는 입력 모드. replay가 다시 말해 늦게 붙은 front도 프로그램이 요청한 형식으로 키를 보낸다.
#[derive(Default)]
struct Modes {
    /// modifyOtherKeys 단계(`CSI > 4 ; n m`). Claude Code는 2를 요청하고 Ctrl+Enter를 `CSI 27;5;13~`로 받는다.
    modify_other_keys: u16,
    /// harness 상태 감지에 쓰는 마지막 OSC 0/2 제목과 OSC 9 진행 payload(herdr `AgentOscStateTracker`, pane/osc.rs 494~565).
    /// Claude·Codex는 제목 앞 spinner로 작업 중을 알린다. 빈 제목은 지운다.
    osc_title: Option<String>,
    osc_progress: Option<String>,
}

/// herdr `AGENT_OSC_MAX_CHARS`: 제목은 프로그램이 정한 글이라 길이를 묶는다
const AGENT_OSC_MAX_CHARS: usize = 256;

/// herdr `sanitize_agent_osc_string`: 제어 문자를 빼고 256자까지
fn sanitize_osc(payload: &[u8]) -> String {
    String::from_utf8_lossy(payload).chars().filter(|ch| !ch.is_control()).take(AGENT_OSC_MAX_CHARS).collect()
}

impl Modes {
    fn title(&mut self, payload: &[u8]) {
        let title = sanitize_osc(payload);
        self.osc_title = (!title.is_empty()).then_some(title);
    }
}

impl vt100::Callbacks for Modes {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title(title);
    }

    /// vt100(vte)은 OSC 본문을 `;`마다 나눈다. herdr는 첫 `;`에서만 나누므로(`parse_agent_osc_body`) 나머지를 다시 잇는다:
    /// `;`가 든 제목과 OSC 9 진행(`9;4;3;` → `4;3;`)이 여기로 온다.
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        let [command, rest @ ..] = params else { return };
        if rest.is_empty() {
            return;
        }
        let payload = rest.join(&b';');
        match *command {
            b"0" | b"2" => self.title(&payload),
            b"9" => self.osc_progress = Some(sanitize_osc(&payload)),
            _ => {}
        }
    }

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

/// scrollback 줄 수. herdr 기본 기록(`DEFAULT_SCROLLBACK_LIMIT_BYTES` 10,000,000, ghostty 메모리 상한)이 120칸에서 담는
/// 줄 수와 같게 둔다(ghostty 칸 8바이트: 10,000,000 / (120×8+8) ≈ 10,300줄). vt100 칸은 32바이트라 꽉 차면 120칸 pane
/// 하나가 약 38MB다(plan §8.67).
pub const SCROLLBACK: usize = 10_000;

/// replay가 앞에 싣는 기록 줄 상한. 붙을 때마다 보내므로 기록 전부를 싣지 않는다(색이 많은 120칸 2000줄이면 약 270KB).
/// 그 앞은 웹이 위로 굴려 끝에 닿을 때 `older`로 한 덩어리씩 받아 온다(plan §8.67).
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
        self.replay_more().0
    }

    /// `replay`와 그 앞에 실리지 않은 기록이 더 있는지
    pub fn replay_more(&self) -> (Vec<u8>, bool) {
        let screen = self.parser.screen();
        // 화면을 지우고 처음부터 그린다(재접속한 front의 이전 화면을 덮는다).
        let mut tail = b"\x1b[H\x1b[2J".to_vec();
        tail.extend_from_slice(&screen.state_formatted());
        let level = self.parser.callbacks().modify_other_keys;
        if level > 0 {
            tail.extend_from_slice(format!("\x1b[>4;{level}m").as_bytes());
        }
        let mut out = Vec::new();
        let more;
        if screen.alternate_screen() {
            // vt100은 보이는 grid만 내준다: 사본에서 alternate screen을 나가 normal screen을 얻는다(1049l은 커서도 되돌린다)
            let (rows, cols) = screen.size();
            let mut normal = vt100::Parser::new(rows, cols, 0);
            *normal.screen_mut() = screen.clone();
            normal.process(b"\x1b[?1049l");
            let mut under = b"\x1b[H\x1b[2J".to_vec();
            under.extend_from_slice(&normal.screen().contents_formatted());
            under.extend_from_slice(b"\x1b[?1049h");
            let (h, m) = history(normal.screen(), history_budget(under.len() + tail.len()));
            more = m;
            out.extend_from_slice(&h);
            out.extend_from_slice(&under);
        } else {
            let (h, m) = history(screen, history_budget(tail.len()));
            more = m;
            out.extend_from_slice(&h);
        }
        out.extend_from_slice(&tail);
        (out, more)
    }

    /// 받은 기록 맨 위 줄들(`anchor`, 글만)의 바로 앞 기록 `want`줄을 replay 기록과 같은 모양으로 준다(위로 굴린 웹이 받아 붙인다).
    /// `depth`는 받은 쪽에서 anchor 첫 줄부터 맨 아래 줄까지의 줄 수다. 같은 글이 여러 곳이면 그 깊이에 가장 가까운 곳이다.
    /// alternate screen이거나 anchor를 찾지 못하면 None(기록을 맞출 수 없다).
    pub fn older(&self, anchor: &[String], depth: usize, want: usize) -> Option<Older> {
        let screen = self.parser.screen();
        if screen.alternate_screen() || anchor.is_empty() {
            return None;
        }
        let mut s = screen.clone();
        let (rows, cols) = s.size();
        s.set_scrollback(usize::MAX);
        let len = s.scrollback();
        // 기록(오래된 것부터)과 화면의 글
        let mut plain: Vec<String> = Vec::with_capacity(len + usize::from(rows));
        let mut back = len;
        while back > 0 {
            s.set_scrollback(back);
            let take = back.min(usize::from(rows));
            plain.extend(s.rows(0, cols).take(take).map(|r| r.trim_end().to_string()));
            back -= take;
        }
        s.set_scrollback(0);
        plain.extend(s.rows(0, cols).map(|r| r.trim_end().to_string()));
        let n = anchor.len();
        if plain.len() < n {
            return None;
        }
        let target = plain.len().saturating_sub(depth);
        let at = (0..=plain.len() - n)
            .filter(|&i| plain[i..i + n].iter().zip(anchor).all(|(a, b)| a == b.trim_end()))
            .min_by_key(|&i| i.abs_diff(target))?;
        // anchor 앞 줄부터 거꾸로, 줄 수·bytes 상한 안에서
        let mut lines: Vec<Vec<u8>> = Vec::new();
        let mut size = 0;
        let mut i = at;
        while i > 0 && lines.len() < want {
            let row = i - 1;
            let mut line = if row < len {
                s.set_scrollback(len - row);
                s.rows_formatted(0, cols).next().unwrap_or_default()
            } else {
                s.set_scrollback(0);
                s.rows_formatted(0, cols).nth(row - len).unwrap_or_default()
            };
            line.extend_from_slice(b"\x1b[m\r\n");
            if size + line.len() > REPLAY_HISTORY_BYTES && !lines.is_empty() {
                break;
            }
            size += line.len();
            lines.push(line);
            i = row;
        }
        let mut data = Vec::with_capacity(size);
        for line in lines.iter().rev() {
            data.extend_from_slice(line);
        }
        Some(Older { data, lines: lines.len(), more: i > 0 })
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
        ScreenText {
            session: session.to_string(),
            cols,
            rows,
            lines,
            cursor_row: cr,
            cursor_col: cc,
            // herdr `agent read --source visible --format ansi`와 같은 글(`read`의 Visible·Ansi). prompt_box가 읽는다
            ansi: rows_text(screen.rows_formatted(0, cols).map(ansi_row).collect()),
        }
    }

    /// 글로 읽는다. herdr `read_terminal_snapshot`(src/app/api_helpers.rs)과 `ghostty_recent_*`(src/pane/terminal.rs)의 규칙:
    /// - Visible: 보이는 줄, 끝 빈 줄은 뺀다. `lines`면 그 마지막 줄들.
    /// - Recent: 기록과 화면을 이어 「마지막 글 줄과 커서 줄 가운데 아래쪽」까지의 마지막 `lines`줄(기본 80). alternate
    ///   screen이면 그 화면의 마지막 줄들. 앞에 줄이 더 있으면 `truncated`.
    /// - RecentUnwrapped: Recent와 같되 화면 폭에서 접힌 줄을 잇는다.
    ///
    /// 기록을 보려고 scrollback 위치를 잠깐 옮겼다가 되돌린다(사본을 만들지 않는다: 기록이 가득하면 수십 MB다).
    pub fn read(&mut self, source: ReadSource, lines: Option<u32>, format: ReadFormat) -> ReadText {
        let limit = lines.map(|l| (l as usize).min(READ_MAX_LINES));
        let ansi = format == ReadFormat::Ansi;
        match source {
            ReadSource::Visible => {
                let screen = self.parser.screen();
                let (_, cols) = screen.size();
                let rows: Vec<String> = if ansi {
                    screen.rows_formatted(0, cols).map(ansi_row).collect()
                } else {
                    screen.rows(0, cols).map(|l| l.trim_end().to_string()).collect()
                };
                limit_lines(rows_text(rows), limit)
            }
            ReadSource::Recent | ReadSource::RecentUnwrapped => {
                self.recent(limit.unwrap_or(READ_RECENT_LINES), source == ReadSource::RecentUnwrapped, ansi)
            }
        }
    }

    fn recent(&mut self, lines: usize, unwrap: bool, ansi: bool) -> ReadText {
        let (rows, cols) = self.parser.screen().size();
        let (rows, cols) = (usize::from(rows), cols);
        let alt = self.parser.screen().alternate_screen();
        let back = if alt { 0 } else { self.history_len() };
        let total = back + rows;
        if total == 0 || lines == 0 {
            return ReadText { text: String::new(), truncated: false };
        }
        let end = if alt {
            total - 1
        } else {
            // 마지막 글 줄과 커서 줄 가운데 아래쪽까지(뒤의 빈 줄은 읽지 않는다)
            let screen = self.parser.screen();
            let cursor = back + usize::from(screen.cursor_position().0);
            let last =
                screen.rows(0, cols).enumerate().filter(|(_, l)| !l.trim().is_empty()).map(|(i, _)| back + i).last();
            last.map(|l| l.max(cursor)).unwrap_or(total - 1)
        };
        let start = (end + 1).saturating_sub(lines);
        let got = self.rows_between(start, end, ansi);
        let mut out: Vec<String> = Vec::with_capacity(got.len());
        let mut joining = false;
        for (text, wrapped) in got {
            match out.last_mut() {
                Some(prev) if unwrap && joining => prev.push_str(&text),
                _ => out.push(text),
            }
            joining = wrapped;
        }
        while out.last().is_some_and(|l| strip_sgr(l).trim().is_empty()) {
            out.pop();
        }
        ReadText { text: rows_text(out), truncated: total > lines }
    }

    /// normal screen의 기록 줄 수
    fn history_len(&mut self) -> usize {
        let screen = self.parser.screen_mut();
        screen.set_scrollback(usize::MAX);
        let n = screen.scrollback();
        screen.set_scrollback(0);
        n
    }

    /// 기록(0..기록 길이)과 화면(그 뒤)을 이은 줄 번호로 `start..=end` 줄과 그 줄이 다음 줄로 접혔는지
    fn rows_between(&mut self, start: usize, end: usize, ansi: bool) -> Vec<(String, bool)> {
        let back = if self.parser.screen().alternate_screen() { 0 } else { self.history_len() };
        let screen = self.parser.screen_mut();
        let (rows, cols) = screen.size();
        let mut out = Vec::with_capacity(end + 1 - start);
        let mut i = start;
        while i <= end {
            // 기록 줄 i를 맨 위에 두려면 위로 (back - i)줄 올린다. 화면 줄이면 올리지 않는다.
            let (offset, first) = if i < back { (back - i, 0) } else { (0, i - back) };
            screen.set_scrollback(offset);
            let take = (usize::from(rows) - first).min(end + 1 - i);
            let texts: Vec<String> = if ansi {
                screen.rows_formatted(0, cols).skip(first).take(take).map(ansi_row).collect()
            } else {
                screen.rows(0, cols).skip(first).take(take).map(|l| l.trim_end().to_string()).collect()
            };
            for (k, t) in texts.into_iter().enumerate() {
                out.push((t, screen.row_wrapped((first + k) as u16)));
            }
            i += take;
        }
        screen.set_scrollback(0);
        out
    }

    /// 이 화면의 프로그램이 켠 입력 모드
    pub fn modes(&self) -> KeyModes {
        let s = self.parser.screen();
        KeyModes {
            application_cursor: s.application_cursor(),
            bracketed_paste: s.bracketed_paste(),
            modify_other_keys: self.parser.callbacks().modify_other_keys,
        }
    }

    /// 화면에 실어 보내는 모드(TUI가 마우스를 넘기고 휠·화살표를 고른다). vt100은 1015(urxvt)·1016(SGR 픽셀)을 모른다.
    pub fn pane_modes(&self) -> PaneModes {
        let s = self.parser.screen();
        PaneModes {
            mouse: match s.mouse_protocol_mode() {
                vt100::MouseProtocolMode::None => MouseMode::None,
                vt100::MouseProtocolMode::Press => MouseMode::Press,
                vt100::MouseProtocolMode::PressRelease => MouseMode::PressRelease,
                vt100::MouseProtocolMode::ButtonMotion => MouseMode::ButtonMotion,
                vt100::MouseProtocolMode::AnyMotion => MouseMode::AnyMotion,
            },
            mouse_encoding: match s.mouse_protocol_encoding() {
                vt100::MouseProtocolEncoding::Default => MouseEncoding::Default,
                vt100::MouseProtocolEncoding::Utf8 => MouseEncoding::Utf8,
                vt100::MouseProtocolEncoding::Sgr => MouseEncoding::Sgr,
            },
            alt_screen: s.alternate_screen(),
            app_cursor: s.application_cursor(),
        }
    }

    /// 마지막 OSC 0/2 제목(없거나 지웠으면 빈 글)
    pub fn osc_title(&self) -> &str {
        self.parser.callbacks().osc_title.as_deref().unwrap_or("")
    }

    /// 마지막 OSC 9 진행 payload(`9;` 다음, 예: `4;0;`). 없으면 빈 글.
    pub fn osc_progress(&self) -> &str {
        self.parser.callbacks().osc_progress.as_deref().unwrap_or("")
    }

    /// harness 상태 감지가 읽는 화면 끝 글. herdr `ghostty_detection_text_for_cache`(pane/terminal.rs 2849줄)처럼 화면
    /// 높이만큼의 최근 줄(`recent`, herdr `ghostty_recent_read_range`)이다: 마지막 내용 줄과 커서 줄 가운데 아래쪽에서 끝나고,
    /// 화면이 덜 찼으면 그 위는 기록(scrollback) 줄이며, alternate screen이면 보이는 화면이다.
    pub fn detection_text(&mut self) -> String {
        let rows = usize::from(self.parser.screen().size().0).max(1);
        self.recent(rows, false, false).text
    }
}

/// `Vt::read` 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadText {
    pub text: String,
    pub truncated: bool,
}

/// Recent 기본 줄 수와 읽기 상한(herdr `read_terminal_snapshot`과 같다)
pub const READ_RECENT_LINES: usize = 80;
pub const READ_MAX_LINES: usize = 1000;

/// 줄들을 `\n`으로 잇고 끝에 `\n`(herdr `lines_to_text`·`recent_text_from_rows`). Visible은 끝 빈 줄을 뺀다.
fn rows_text(mut rows: Vec<String>) -> String {
    while rows.last().is_some_and(|l| strip_sgr(l).trim().is_empty()) {
        rows.pop();
    }
    let text = rows.join("\n");
    if text.is_empty() { text } else { format!("{text}\n") }
}

/// 마지막 `limit`줄만(herdr `limit_snapshot_lines`)
fn limit_lines(text: String, limit: Option<usize>) -> ReadText {
    let Some(limit) = limit else { return ReadText { text, truncated: false } };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    ReadText { text: lines[lines.len().saturating_sub(limit)..].concat(), truncated: lines.len() > limit }
}

/// 글 모양이 있는 줄 하나: 줄 끝에서 모양을 되돌려 다음 줄에 번지지 않게 한다
fn ansi_row(row: Vec<u8>) -> String {
    let mut s = String::from_utf8_lossy(&row).into_owned();
    s.push_str("\x1b[m");
    s
}

/// SGR(`ESC [ … m`)을 뺀 글: 모양만 있는 빈 줄을 가린다
fn strip_sgr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\x1b' && it.peek() == Some(&'[') {
            for d in it.by_ref() {
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// 프로그램이 켠 입력 모드. 키 이름을 bytes로 바꿀 때 쓴다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KeyModes {
    /// DECCKM(`CSI ? 1 h`): 화살표를 `ESC O x`로
    pub application_cursor: bool,
    /// `CSI ? 2004 h`: 붙여 넣은 글을 `ESC [200~ … ESC [201~`로 감싼다
    pub bracketed_paste: bool,
    /// modifyOtherKeys 단계(`CSI > 4 ; n m`)
    pub modify_other_keys: u16,
}

impl KeyModes {
    /// API로 넣는 글(herdr `encode_api_text`, src/app/api_helpers.rs): bracketed paste가 켜져 있으면 감싼다
    pub fn paste(&self, text: &str) -> Vec<u8> {
        if self.bracketed_paste { format!("\x1b[200~{text}\x1b[201~").into_bytes() } else { text.as_bytes().to_vec() }
    }

    /// 키 이름 → 입력 bytes. 모르는 이름이면 None(herdr `invalid_key`).
    pub fn key(&self, name: &str) -> Option<Vec<u8>> {
        let (key, m) = parse_key(name)?;
        Some(encode_key(key, m, self))
    }
}

/// 키 이름의 키 부분
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Backspace,
    Left,
    Right,
    Up,
    Down,
    F(u8),
}

/// 키 이름의 modifier(xterm 인자: 1 + shift + 2·alt + 4·ctrl)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    /// cmd·super·hyper: xterm 방식 입력에는 자리가 없어 bytes에는 싣지 않는다(이름은 받아들인다)
    pub meta: bool,
}

impl Mods {
    fn param(&self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// herdr 키 이름 문법(`parse_api_key`·`parse_key_combo`, src/app/api_helpers.rs·src/config/keybinds.rs):
/// `+`로 modifier(ctrl·control·shift·alt·option·meta·cmd·command·super·hyper)와 키 하나를 잇는다. 키는 enter·return·esc·escape·
/// tab·backspace·bs·space·left·right·up·down·f1..·minus·comma·period·slash·backslash·quote·double_quote·semicolon·colon·
/// percent·ampersand·backtick·plus 또는 글자 하나(대문자는 shift). `C-c`는 ctrl+c, `+`는 plus다. 대소문자는 가리지 않는다.
pub fn parse_key(raw: &str) -> Option<(Key, Mods)> {
    let s = match raw.trim() {
        "C-c" | "c-c" => "ctrl+c",
        "+" => return Some((Key::Char('+'), Mods::default())),
        s => s,
    };
    let (body, plus_key) = match s.strip_suffix("++") {
        Some(m) => (m, true),
        None => (s, false),
    };
    let mut m = Mods::default();
    let mut key: Option<&str> = None;
    for part in body.split('+') {
        let p = part.trim();
        if p.is_empty() {
            return None;
        }
        match p.to_lowercase().as_str() {
            "ctrl" | "control" => m.ctrl = true,
            "shift" => m.shift = true,
            "alt" | "option" | "meta" => m.alt = true,
            "cmd" | "command" | "super" | "hyper" => m.meta = true,
            _ if key.is_some() || plus_key => return None,
            _ => key = Some(p),
        }
    }
    if plus_key {
        return Some((Key::Char('+'), m));
    }
    let k = key?;
    let lower = k.to_lowercase();
    let code = match lower.as_str() {
        "space" => Key::Char(' '),
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Esc,
        "tab" if m.shift => {
            m.shift = false;
            Key::BackTab
        }
        "tab" => Key::Tab,
        "backspace" | "bs" => Key::Backspace,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "minus" => Key::Char('-'),
        "comma" => Key::Char(','),
        "period" => Key::Char('.'),
        "slash" => Key::Char('/'),
        "backslash" => Key::Char('\\'),
        "quote" => Key::Char('\''),
        "double_quote" | "double-quote" => Key::Char('"'),
        "semicolon" => Key::Char(';'),
        "colon" => Key::Char(':'),
        "percent" => Key::Char('%'),
        "ampersand" => Key::Char('&'),
        "backtick" => Key::Char('`'),
        "plus" => Key::Char('+'),
        _ if k.chars().count() == 1 => {
            let c = k.chars().next()?;
            if c.is_ascii_uppercase() {
                m.shift = true;
                Key::Char(c.to_ascii_lowercase())
            } else {
                Key::Char(c)
            }
        }
        f if f.starts_with('f') => Key::F(f[1..].parse::<u8>().ok().filter(|n| (1..=20).contains(n))?),
        _ => return None,
    };
    Some((code, m))
}

fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '-' | '7' | '/' => Some(0x1f),
        '8' | '?' => Some(0x7f),
        _ => None,
    }
}

/// xterm 방식(legacy) 입력 bytes. 화살표는 DECCKM을, Ctrl·Shift+Enter 같은 조합은 modifyOtherKeys를 따른다
/// (herdr-web-ui `keys.ts ctrlEnterSequence`·aaa 웹 키 막대와 같은 규칙).
fn encode_key(key: Key, m: Mods, modes: &KeyModes) -> Vec<u8> {
    let p = m.param();
    let csi = |end: char| -> Vec<u8> {
        if p == 1 { format!("\x1b[{end}").into_bytes() } else { format!("\x1b[1;{p}{end}").into_bytes() }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if p == 1 { format!("\x1b[{n}~").into_bytes() } else { format!("\x1b[{n};{p}~").into_bytes() }
    };
    let other = |code: u32| format!("\x1b[27;{p};{code}~").into_bytes();
    let alt = |mut b: Vec<u8>| {
        if m.alt {
            b.insert(0, 0x1b);
        }
        b
    };
    match key {
        Key::Char(c) if m.ctrl => match ctrl_byte(c.to_ascii_lowercase()) {
            Some(b) => alt(vec![b]),
            None if modes.modify_other_keys > 0 => other(c as u32),
            None => alt(c.to_string().into_bytes()),
        },
        Key::Char(c) => {
            let c = if m.shift { c.to_ascii_uppercase() } else { c };
            alt(c.to_string().into_bytes())
        }
        Key::Enter if (m.ctrl || m.shift) && modes.modify_other_keys > 0 => other(13),
        Key::Enter => alt(b"\r".to_vec()),
        Key::Tab if m.ctrl && modes.modify_other_keys > 0 => other(9),
        Key::Tab => alt(b"\t".to_vec()),
        Key::BackTab => b"\x1b[Z".to_vec(),
        Key::Backspace if m.ctrl => alt(vec![0x08]),
        Key::Backspace => alt(vec![0x7f]),
        Key::Esc => alt(vec![0x1b]),
        Key::Up | Key::Down | Key::Right | Key::Left => {
            let end = match key {
                Key::Up => 'A',
                Key::Down => 'B',
                Key::Right => 'C',
                _ => 'D',
            };
            if p == 1 && modes.application_cursor { format!("\x1bO{end}").into_bytes() } else { csi(end) }
        }
        Key::F(n @ 1..=4) => {
            let end = (b'P' + n - 1) as char;
            if p == 1 { format!("\x1bO{end}").into_bytes() } else { csi(end) }
        }
        Key::F(n) => tilde([15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34][usize::from(n) - 5]),
    }
}

/// 화면 bytes가 `screen`일 때 기록에 쓸 수 있는 bytes: snapshot JSON(base64는 4/3배)이 한 frame에 들고 64KiB가 남게
fn history_budget(screen: usize) -> usize {
    REPLAY_HISTORY_BYTES.min((aaa_proto::MAX_FRAME / 4 * 3).saturating_sub(screen + (64 << 10)))
}

/// `Vt::older`의 답: anchor 바로 앞 기록 `lines`줄(줄마다 `ESC[m CR LF`로 끝난다)과, 그 앞에 기록이 더 있는지
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Older {
    pub data: Vec<u8>,
    pub lines: usize,
    pub more: bool,
}

/// scrollback 줄을 오래된 것부터 한 줄씩 쓰고, 마지막 줄까지 화면 위로 밀어 올리는 bytes와, 싣지 못한 앞 기록이 있는지.
/// 빈 터미널이 이것을 받으면 그 scrollback에 같은 줄이 생긴다(plan §8.55: 기록은 session host vt가 갖고, 붙는 front에 채운다).
/// 화면 밖 줄이라 줄 이음(soft wrap)은 남기지 않는다.
fn history(screen: &vt100::Screen, budget: usize) -> (Vec<u8>, bool) {
    let mut screen = screen.clone();
    let (rows, cols) = screen.size();
    // 크게 주면 실제 scrollback 길이로 줄어든다
    screen.set_scrollback(usize::MAX);
    let all = screen.scrollback();
    let len = all.min(REPLAY_HISTORY);
    let mut out = Vec::new();
    if len == 0 {
        return (out, false);
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
    let more = all > lines.len() - first;
    if first == lines.len() {
        return (out, more);
    }
    for line in &lines[first..] {
        out.extend_from_slice(line);
    }
    // 커서는 마지막 기록 줄 다음 줄 맨 앞이다. rows-1번 더 내리면 기록이 모두 화면 위로 넘어간다
    out.extend(std::iter::repeat_n(b'\n', usize::from(rows) - 1));
    (out, more)
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

    /// `ScreenText.ansi`는 `read`의 Visible·Ansi와 같은 글이다(prompt_box가 흐린 placeholder를 가른다)
    #[test]
    fn screen_text_ansi_is_the_visible_ansi_read() {
        let mut v = Vt::new(3, 30);
        v.feed("❯ \x1b[2mTry \"fix\"\x1b[0m\r\n\x1b[7mx\x1b[27m \x1b[38;5;135m─\x1b[0m".as_bytes());
        let a = v.text("s").ansi;
        assert_eq!(a, v.read(ReadSource::Visible, None, ReadFormat::Ansi).text);
        assert!(a.contains("\x1b[2m") || a.contains(";2m"), "{a:?}");
        assert_eq!(strip_sgr(&a), "❯ Try \"fix\"\nx ─\n");
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

    /// 기록 줄 글(오래된 것부터)과 화면 글
    fn all_text(vt: &Vt) -> Vec<String> {
        let mut s = vt.parser.screen().clone();
        let (rows, cols) = s.size();
        s.set_scrollback(usize::MAX);
        let len = s.scrollback();
        let mut out = Vec::new();
        let mut back = len;
        while back > 0 {
            s.set_scrollback(back);
            let take = back.min(usize::from(rows));
            out.extend(s.rows(0, cols).take(take).map(|r| r.trim_end().to_string()));
            back -= take;
        }
        s.set_scrollback(0);
        out.extend(s.rows(0, cols).map(|r| r.trim_end().to_string()));
        out
    }

    #[test]
    fn replay_says_whether_older_history_is_left_out() {
        let mut vt = Vt::new(10, 40);
        for i in 1..=50 {
            vt.feed(format!("L{i:04}\r\n").as_bytes());
        }
        assert!(!vt.replay_more().1, "다 실렸다");
        for i in 51..=REPLAY_HISTORY + 100 {
            vt.feed(format!("L{i:04}\r\n").as_bytes());
        }
        assert!(vt.replay_more().1, "앞 기록이 남았다");
    }

    #[test]
    fn older_gives_the_chunk_just_above_the_anchor_until_history_ends() {
        let mut vt = Vt::new(10, 40);
        for i in 1..=5000 {
            vt.feed(format!("\x1b[3{}mL{i:04}\x1b[m\r\n", i % 7).as_bytes());
        }
        let text = all_text(&vt);
        let total = text.len();
        // 웹이 L3001부터 갖고 있다: 그 앞 2000줄은 L1001~L3000
        let at = text.iter().position(|l| l == "L3001").unwrap();
        let anchor = vec!["L3001".to_string(), "L3002".to_string()];
        let o = vt.older(&anchor, total - at, 2000).unwrap();
        assert_eq!((o.lines, o.more), (2000, true));
        // 받은 덩어리를 빈 vt에 쓰면 그 줄들이 같은 글·색으로 기록에 쌓인다
        let mut fresh = Vt::new(10, 40);
        fresh.feed(&o.data);
        let got = all_text(&fresh);
        let got: Vec<_> = got.iter().filter(|l| !l.is_empty()).cloned().collect();
        assert_eq!(got.first().map(String::as_str), Some("L1001"));
        assert_eq!(got.last().map(String::as_str), Some("L3000"));
        assert_eq!(got.len(), 2000);
        assert!(String::from_utf8_lossy(&o.data).contains("\x1b[3"), "색을 싣는다");
        // 그 다음 덩어리는 기록 끝까지: L0001~L1000, 더 없음
        let at = text.iter().position(|l| l == "L1001").unwrap();
        let o = vt.older(&["L1001".to_string()], total - at, 2000).unwrap();
        assert_eq!((o.lines, o.more), (1000, false));
        assert!(String::from_utf8_lossy(&o.data).starts_with("\x1b[31mL0001"), "{:?}", &o.data[..12]);
        // 맨 앞이면 빈 덩어리, 더 없음
        let o = vt.older(&["L0001".to_string()], total, 2000).unwrap();
        assert_eq!((o.lines, o.more), (0, false));
        // 없는 글이면 맞출 수 없다
        assert!(vt.older(&["nope".to_string()], 10, 2000).is_none());
    }

    #[test]
    fn older_picks_the_repeated_anchor_nearest_the_given_depth() {
        let mut vt = Vt::new(5, 20);
        // 같은 글 「same」이 세 번: 각 앞 줄로 어느 것을 골랐는지 본다
        for i in 1..=300 {
            let l = if i % 100 == 0 { "same".to_string() } else { format!("x{i}") };
            vt.feed(format!("{l}\r\n").as_bytes());
        }
        let text = all_text(&vt);
        let total = text.len();
        for want in [100, 200, 300] {
            let at = text.iter().position(|l| *l == format!("x{}", want - 1)).unwrap() + 1;
            let o = vt.older(&["same".to_string()], total - at, 1).unwrap();
            assert_eq!(String::from_utf8_lossy(&o.data).trim_end_matches("\x1b[m\r\n"), format!("x{}", want - 1));
        }
    }

    #[test]
    fn older_is_none_on_the_alternate_screen() {
        let mut vt = Vt::new(5, 20);
        for i in 1..=50 {
            vt.feed(format!("L{i}\r\n").as_bytes());
        }
        vt.feed(b"\x1b[?1049h");
        assert!(vt.older(&["L40".to_string()], 15, 10).is_none());
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
    fn pane_modes_follow_the_program() {
        let mut v = Vt::new(5, 20);
        assert_eq!(v.pane_modes(), PaneModes::default());
        v.feed(b"\x1b[?1049h\x1b[?1h\x1b[?1002h\x1b[?1006h");
        assert_eq!(
            v.pane_modes(),
            PaneModes {
                mouse: MouseMode::ButtonMotion,
                mouse_encoding: MouseEncoding::Sgr,
                alt_screen: true,
                app_cursor: true
            }
        );
        v.feed(b"\x1b[?1002l\x1b[?1003h\x1b[?1006l\x1b[?1005h\x1b[?1049l\x1b[?1l");
        assert_eq!(
            v.pane_modes(),
            PaneModes {
                mouse: MouseMode::AnyMotion,
                mouse_encoding: MouseEncoding::Utf8,
                alt_screen: false,
                app_cursor: false
            }
        );
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

    fn read(v: &mut Vt, source: ReadSource, lines: Option<u32>) -> ReadText {
        v.read(source, lines, ReadFormat::Text)
    }

    #[test]
    fn read_follows_herdr_sources() {
        let mut v = Vt::new(5, 10);
        for i in 1..=20 {
            v.feed(format!("l{i}\r\n").as_bytes());
        }
        v.feed(b"$ ");
        // Visible: 보이는 줄(끝 빈 줄 없음)
        assert_eq!(read(&mut v, ReadSource::Visible, None).text, "l17\nl18\nl19\nl20\n$\n");
        assert_eq!(read(&mut v, ReadSource::Visible, Some(2)), ReadText { text: "l20\n$\n".into(), truncated: true });
        // Recent: 기록까지 이어 마지막 줄들, 앞에 더 있으면 truncated
        let r = read(&mut v, ReadSource::Recent, Some(3));
        assert_eq!(r, ReadText { text: "l19\nl20\n$\n".into(), truncated: true });
        let all = read(&mut v, ReadSource::Recent, None);
        assert!(all.text.starts_with("l1\nl2\n") && all.text.ends_with("l20\n$\n"), "{}", all.text);
        assert!(!all.truncated);
        // 읽은 뒤 scrollback 위치는 그대로(화면이 기록으로 밀리지 않는다)
        assert_eq!(read(&mut v, ReadSource::Visible, None).text, "l17\nl18\nl19\nl20\n$\n");
        // 커서가 마지막 글 줄보다 아래면 범위는 커서 줄까지다(herdr와 같다). 그 끝 빈 줄은 글에서 뺀다.
        let mut c = Vt::new(6, 10);
        c.feed(b"a\r\nb\r\n\r\n");
        assert_eq!(read(&mut c, ReadSource::Recent, Some(2)).text, "");
        assert_eq!(read(&mut c, ReadSource::Recent, Some(3)).text, "b\n");
        assert_eq!(read(&mut c, ReadSource::Recent, Some(10)).text, "a\nb\n");
    }

    #[test]
    fn recent_unwrapped_joins_soft_wrapped_rows() {
        let mut v = Vt::new(4, 5);
        v.feed(b"abcdefgh\r\nxy");
        assert_eq!(read(&mut v, ReadSource::Recent, None).text, "abcde\nfgh\nxy\n");
        assert_eq!(read(&mut v, ReadSource::RecentUnwrapped, None).text, "abcdefgh\nxy\n");
        // 접힌 줄이 기록과 화면에 걸쳐도 잇는다
        let mut w = Vt::new(2, 5);
        w.feed(b"0123456789AB\r\nz");
        assert_eq!(read(&mut w, ReadSource::RecentUnwrapped, None).text, "0123456789AB\nz\n");
    }

    #[test]
    fn read_alternate_screen_and_ansi() {
        let mut v = Vt::new(3, 10);
        v.feed(b"under\r\n\x1b[?1049h\x1b[Htop\r\n\x1b[31mred\x1b[m");
        assert_eq!(read(&mut v, ReadSource::Recent, None).text, "top\nred\n");
        let a = v.read(ReadSource::Visible, None, ReadFormat::Ansi).text;
        assert!(a.contains("\x1b[31mred") && a.contains("top"), "{a:?}");
        assert!(a.lines().all(|l| l.ends_with("\x1b[m")), "{a:?}");
    }

    #[test]
    fn key_names_follow_herdr_grammar_and_modes() {
        let plain = KeyModes::default();
        let k = |m: &KeyModes, n: &str| m.key(n);
        assert_eq!(k(&plain, "enter").unwrap(), b"\r");
        assert_eq!(k(&plain, "Return").unwrap(), b"\r");
        assert_eq!(k(&plain, "esc").unwrap(), [0x1b]);
        assert_eq!(k(&plain, "ctrl+c").unwrap(), [3]);
        assert_eq!(k(&plain, "C-c").unwrap(), [3]);
        assert_eq!(k(&plain, "Ctrl+C").unwrap(), [3]);
        assert_eq!(k(&plain, "shift+tab").unwrap(), b"\x1b[Z");
        assert_eq!(k(&plain, "tab").unwrap(), b"\t");
        assert_eq!(k(&plain, "up").unwrap(), b"\x1b[A");
        assert_eq!(k(&plain, "ctrl+left").unwrap(), b"\x1b[1;5D");
        assert_eq!(k(&plain, "alt+x").unwrap(), b"\x1bx");
        assert_eq!(k(&plain, "A").unwrap(), b"A");
        assert_eq!(k(&plain, "y").unwrap(), b"y");
        assert_eq!(k(&plain, "1").unwrap(), b"1");
        assert_eq!(k(&plain, "space").unwrap(), b" ");
        assert_eq!(k(&plain, "+").unwrap(), b"+");
        assert_eq!(k(&plain, "ctrl++").unwrap(), b"+");
        assert_eq!(k(&plain, "backspace").unwrap(), [0x7f]);
        assert_eq!(k(&plain, "f1").unwrap(), b"\x1bOP");
        assert_eq!(k(&plain, "f5").unwrap(), b"\x1b[15~");
        assert_eq!(k(&plain, "shift+f5").unwrap(), b"\x1b[15;2~");
        assert_eq!(k(&plain, "shift+enter").unwrap(), b"\r");
        for bad in ["", "ctrl+", "nope", "ctrl+a+b", "f0", "f21"] {
            assert!(k(&plain, bad).is_none(), "{bad}");
        }
        // 프로그램이 켠 모드를 따른다
        let app = KeyModes { application_cursor: true, bracketed_paste: true, modify_other_keys: 2 };
        assert_eq!(k(&app, "up").unwrap(), b"\x1bOA");
        assert_eq!(k(&app, "ctrl+up").unwrap(), b"\x1b[1;5A");
        assert_eq!(k(&app, "ctrl+enter").unwrap(), b"\x1b[27;5;13~");
        assert_eq!(k(&app, "shift+enter").unwrap(), b"\x1b[27;2;13~");
        assert_eq!(app.paste("hi"), b"\x1b[200~hi\x1b[201~");
        assert_eq!(plain.paste("hi"), b"hi");
    }

    #[test]
    fn modes_come_from_the_program() {
        let mut v = Vt::new(3, 10);
        assert_eq!(v.modes(), KeyModes::default());
        v.feed(b"\x1b[?1h\x1b[?2004h\x1b[>4;2m");
        assert_eq!(v.modes(), KeyModes { application_cursor: true, bracketed_paste: true, modify_other_keys: 2 });
    }

    /// herdr pane/osc.rs `AgentOscStateTracker`: 마지막 OSC 0/2 제목·OSC 9 진행. `;`가 든 제목, 빈 제목(지움), 제어 문자,
    /// 길이 상한, ST·BEL 끝, 나뉘어 온 bytes
    #[test]
    fn osc_title_and_progress_are_kept_for_detection() {
        let mut v = Vt::new(5, 40);
        assert_eq!((v.osc_title(), v.osc_progress()), ("", ""));
        v.feed("\x1b]0;⠋ codex · demo\x07".as_bytes());
        assert_eq!(v.osc_title(), "⠋ codex · demo");
        v.feed(b"\x1b]2;a;b;c\x1b\\");
        assert_eq!(v.osc_title(), "a;b;c");
        v.feed("\x1b]0;✳ x\x1b".as_bytes());
        v.feed(b"\\");
        assert_eq!(v.osc_title(), "✳ x");
        v.feed(b"\x1b]9;4;3;\x07");
        assert_eq!(v.osc_progress(), "4;3;");
        v.feed(b"\x1b]9;4;0;\x07\x1b]0;\x07");
        assert_eq!((v.osc_title(), v.osc_progress()), ("", "4;0;"));
        // 아이콘 이름(OSC 1)·다른 OSC는 제목이 아니다
        v.feed(b"\x1b]1;icon\x07\x1b]7;file:///tmp\x07");
        assert_eq!(v.osc_title(), "");
        let long = format!("\x1b]0;{}\x07", "가".repeat(300));
        v.feed(long.as_bytes());
        assert_eq!(v.osc_title().chars().count(), AGENT_OSC_MAX_CHARS);
        v.feed(b"\x1b]0;a\tb\x07");
        assert_eq!(v.osc_title(), "ab");
        // 화면 글에는 남지 않는다
        assert!(v.text("s").lines.iter().all(|l| l.is_empty()));
    }

    /// herdr pane/terminal.rs `ghostty_recent_read_range`: 끝은 마지막 내용 줄과 커서 줄 가운데 아래쪽, 덜 찬 화면은 위를
    /// 기록 줄로 채우고, 끝의 빈 줄은 뺀다. alternate screen은 보이는 화면만.
    #[test]
    fn detection_text_ends_at_content_or_cursor_and_reaches_into_scrollback() {
        let mut v = Vt::new(4, 20);
        assert_eq!(v.detection_text(), "");
        for i in 1..=6 {
            v.feed(format!("l{i}\r\n").as_bytes());
        }
        v.feed(b"tail   ");
        // 화면: l4 l5 l6 tail, 기록: l1 l2 l3
        assert_eq!(v.detection_text(), "l4\nl5\nl6\ntail\n");
        // 화면을 지우고(지운 줄은 기록으로 가지 않는다) 위쪽 두 줄만 그리면 그 위는 기록 끝 두 줄이다(herdr는 rows줄을 읽는다)
        v.feed(b"\x1b[2J\x1b[Ha\r\nb");
        assert_eq!(v.detection_text(), "l2\nl3\na\nb\n");
        // 커서가 내용보다 아래면 커서 줄까지(빈 줄은 끝에서 뺀다)
        v.feed(b"\x1b[4;1H");
        assert_eq!(v.detection_text(), "a\nb\n");
        // 기록이 모자라면 있는 만큼만
        let mut short = Vt::new(5, 20);
        short.feed(b"x\r\ny");
        assert_eq!(short.detection_text(), "x\ny\n");
        // 읽은 뒤 화면은 그대로다(기록 위치를 되돌린다)
        assert_eq!(v.text("s").lines, vec!["a", "b", "", ""]);
        v.feed(b"\x1b[?1049h\x1b[Hfull\x1b[3;1Hlast");
        assert_eq!(v.detection_text(), "full\n\nlast\n");
    }
}
