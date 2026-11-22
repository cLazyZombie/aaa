//! VT adapter. 정본 화면 상태는 session host가 갖는다(기획서 §4.5).
//!
//! 기획서는 ghostty-vt를 1순위로 두었지만 빌드에 Zig 0.16이 필요해 M1-1은 순수 Rust `vt100`을 쓴다.
//! 이 adapter 뒤에서 엔진을 바꿀 수 있게 front·host 쪽은 replay bytes와 글자 화면만 본다.

use aaa_proto::{ReadFormat, ReadSource, ScreenText, TermColors};

pub struct Vt {
    parser: vt100::Parser<Modes>,
    scrollback: usize,
}

/// vt100이 화면에 담지 않는 입력 모드. replay가 다시 말해 늦게 붙은 front도 프로그램이 요청한 형식으로 키를 보낸다.
#[derive(Default, Clone)]
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
        Vt {
            parser: vt100::Parser::new_with_callbacks(rows.max(1), cols.max(1), SCROLLBACK, Modes::default()),
            scrollback: SCROLLBACK,
        }
    }

    /// 화면 사본에 넣는다. 웹에는 원래 출력 bytes를 전달한다.
    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let (old_rows, old_cols) = self.parser.screen().size();
        if (rows, cols) == (old_rows, old_cols) {
            return;
        }
        let screen = self.parser.screen_mut();
        let alt = screen.alternate_screen();
        let rewrap = if cols != old_cols {
            alt || wraps_differently(screen, cols)
        } else {
            // alternate screen이면 그 밑 normal screen의 줄을 볼 수 없어 늘 다시 쓴다(`set_size`는 두 화면을 다 지운다)
            alt || (0..old_rows).any(|r| screen.row_wrapped(r))
        };
        if rewrap {
            self.reflow(rows, cols);
        } else {
            resize_grid(&mut self.parser, rows, cols);
        }
    }

    /// 기록과 화면을 논리 줄로 읽어 새 크기의 새 parser에 다시 쓴다. alternate screen은 다시 접지 않고(ghostty·xterm과 같다)
    /// 크기만 바꾸며, 그 밑 normal screen을 다시 접는다. 입력 모드·글자 모양·커서 보임·modifyOtherKeys는 그대로 옮긴다.
    fn reflow(&mut self, rows: u16, cols: u16) {
        let screen = self.parser.screen();
        let modes = self.parser.callbacks().clone();
        let input_mode = screen.input_mode_formatted();
        let pen = screen.attributes_formatted();
        let hidden = screen.hide_cursor();
        let alt = screen.alternate_screen().then(|| {
            let (r, c) = screen.size();
            let mut a = vt100::Parser::new(r, c, 0);
            a.process(&screen.contents_formatted());
            resize_grid(&mut a, rows, cols);
            a.screen().contents_formatted()
        });
        if alt.is_some() {
            // 나가면 들어갈 때 둔 커서로 돌아온다(1049): 그 커서를 다시 접은 자리로 옮긴 뒤 다시 들어간다
            self.parser.process(b"\x1b[?1049l");
        }
        let mut out = rewrap(self.parser.screen_mut(), rows, cols);
        if let Some(a) = alt {
            out.extend_from_slice(b"\x1b[?1049h\x1b[H\x1b[2J");
            out.extend_from_slice(&a);
        }
        out.extend_from_slice(&pen);
        out.extend_from_slice(&input_mode);
        if hidden {
            out.extend_from_slice(b"\x1b[?25l");
        }
        let mut p = vt100::Parser::new_with_callbacks(rows, cols, self.scrollback, modes);
        p.process(&out);
        self.parser = p;
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
        if screen.alternate_screen() {
            tail.extend_from_slice(&screen.state_formatted());
        } else {
            screen_rows(screen, &mut tail);
            tail.extend_from_slice(&screen.attributes_formatted());
            tail.extend_from_slice(&screen.input_mode_formatted());
        }
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
            screen_rows(normal.screen(), &mut under);
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
            let r = if row < len {
                s.set_scrollback(len - row);
                0
            } else {
                s.set_scrollback(0);
                (row - len) as u16
            };
            // 가장 새 줄(anchor 바로 앞)은 받는 쪽의 이미 있는 줄과 따로 그리므로 늘 줄바꿈
            // 창은 이어진 줄을 보이므로 다음 줄은 r+1이다(anchor 앞 줄이라 화면 맨 아래 줄이 아니다)
            let joins = row + 1 != at && joins_next(&s, r, cols, s.cell(r + 1, 0).is_some_and(vt100::Cell::is_wide));
            let mut line = Vec::new();
            history_row(&s, r, cols, joins, &mut line);
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
    /// - Detection: 상태 감지가 읽는 글(`detection_text`). `lines`면 그 마지막 줄들(기본 없음). ansi여도 모양 없는 글이다.
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
            ReadSource::Detection => limit_lines(self.detection_text(), limit),
        }
    }

    /// 기록 전체와 화면(접힌 줄은 잇는다, 모양 없는 글, 줄 상한 없음). herdr `recent_unwrapped_text_snapshot(usize::MAX)`, 기록
    /// 편집기(`prefix e`)가 쓴다
    pub fn unwrapped_all(&mut self) -> String {
        self.recent(usize::MAX, true, false).text
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
        let got = self.rows_between(start, end, ansi, unwrap);
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
    fn rows_between(&mut self, start: usize, end: usize, ansi: bool, unwrap: bool) -> Vec<(String, bool)> {
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
                screen
                    .rows(0, cols)
                    .skip(first)
                    .take(take)
                    .enumerate()
                    .map(|(k, l)| {
                        // 접힌 줄 끝의 실제 공백은 다음 줄과 이을 때 보존한다.
                        if unwrap && screen.row_wrapped((first + k) as u16) { l } else { l.trim_end().to_string() }
                    })
                    .collect()
            };
            for (k, t) in texts.into_iter().enumerate() {
                // 접혔거나 넓은 글자를 다음 줄로 내렸으면 다음 줄로 이어진다(`joins_next`). 창 맨 아래 줄의 다음 줄은 한 줄 내려 본다
                let r = (first + k) as u16;
                let next_wide = if r + 1 < rows {
                    screen.cell(r + 1, 0).is_some_and(vt100::Cell::is_wide)
                } else if offset > 0 {
                    screen.set_scrollback(offset - 1);
                    let wide = screen.cell(rows - 1, 0).is_some_and(vt100::Cell::is_wide);
                    screen.set_scrollback(offset);
                    wide
                } else {
                    false
                };
                out.push((t, joins_next(screen, r, cols, next_wide)));
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
    /// API로 넣는 글(herdr `encode_api_text`, src/app/api_helpers.rs): bracketed paste가 켜져 있으면 감싼다.
    /// 글의 ESC와 C1 CSI(U+009B)는 뺀다: 글 안의 `ESC[201~`가 붙여넣기를 일찍 끝내 뒤 글을 친 키로 읽게 했다(t-0340 보안 감사, 웹
    /// `pasteText.ts`와 같다). 참고 herdr도 그대로 감싼다. 키는 `key`로 보낸다.
    pub fn paste(&self, text: &str) -> Vec<u8> {
        let text: String = text.chars().filter(|&c| c != '\x1b' && c != '\u{9b}').collect();
        if self.bracketed_paste { format!("\x1b[200~{text}\x1b[201~").into_bytes() } else { text.into_bytes() }
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
/// 다시 쓰지 않고 크기만 바꾼다(지킬 이음 표시가 없을 때·alternate screen). 줄이 줄어 커서 줄이 새 높이 밖으로 나가면, 먼저 그만큼
/// 위 줄을 기록으로 올린다(plan §8.59).
/// 절대 줄 `start..=end`의 칸(넓은 글자 뒤 칸은 뺀다)과 접힘. `back`은 기록 줄 수(alt 화면이면 0). 화면 높이씩 기록을 올려
/// 읽고 읽기 전 위치(feed 밖이면 맨 아래, feed 안 콜백이면 버린 줄 세기 위치)로 되돌린다.
fn resize_grid<C: vt100::Callbacks>(parser: &mut vt100::Parser<C>, rows: u16, cols: u16) {
    let (old_rows, _) = parser.screen().size();
    let (cursor_row, cursor_col) = parser.screen().cursor_position();
    let lift = (cursor_row + 1).saturating_sub(rows);
    if rows < old_rows && lift > 0 {
        // 화면 전체를 lift줄 올린다: scroll region을 풀고 맨 아래에서 줄을 내린다(위 줄은 기록으로).
        // 프로그램은 SIGWINCH 뒤 region을 다시 건다. 커서는 같은 글 위(lift줄 위)로, 모양은 DECSC·DECRC로 되돌린다.
        let mut b = format!("\x1b7\x1b[r\x1b[{old_rows};1H").into_bytes();
        b.extend(std::iter::repeat_n(b'\n', usize::from(lift)));
        b.extend_from_slice(format!("\x1b8\x1b[{};{}H", cursor_row - lift + 1, cursor_col + 1).as_bytes());
        parser.process(&b);
    }
    parser.screen_mut().set_size(rows, cols);
}

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
/// 줄은 글과 글자 모양만으로 쓴다(`history_row`): 접힌 줄은 줄바꿈 없이 이어 써서 받는 터미널(xterm)이 같은 폭에서 다시 접고
/// 이음 표시를 가진다. 그래야 그 화면의 폭이 바뀔 때 xterm이 기록 줄도 다시 접는다.
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
        for r in 0..take {
            let r = r as u16;
            // 다음 줄 첫 칸: 이 창 맨 아래 줄이면 다음 창에서 본다
            let next_wide = if r + 1 < rows {
                screen.cell(r + 1, 0).is_some_and(vt100::Cell::is_wide)
            } else {
                screen.set_scrollback(back - usize::from(rows));
                let wide = screen.cell(0, 0).is_some_and(vt100::Cell::is_wide);
                screen.set_scrollback(back);
                wide
            };
            let joins = back - usize::from(r) > 1 && joins_next(&screen, r, cols, next_wide);
            let mut row = Vec::new();
            history_row(&screen, r, cols, joins, &mut row);
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

/// 새 폭에서 줄이 달리 접히는가: 이어진 줄이 있거나(넓히면 펴진다), 새 폭 밖에 글·색이 있다(줄이면 접힌다)
fn wraps_differently(screen: &mut vt100::Screen, cols: u16) -> bool {
    let (_, old_cols) = screen.size();
    let mut found = false;
    let mut split = false;
    each_row(screen, |s, r| {
        let first_wide = s.cell(r, 0).is_some_and(vt100::Cell::is_wide);
        found = (split && first_wide)
            || s.row_wrapped(r)
            || (cols..old_cols).any(|c| s.cell(r, c).is_some_and(|c| c.has_contents() || !Look::of(c).plain()));
        split = wide_split(s, r, old_cols);
        !found
    });
    found
}

/// vt100은 넓은 글자가 끝 칸 하나에 들지 않으면 이음 표시 없이 다음 줄로 간다(`col_wrap`, tmux와 같다). 그런 줄: 접히지 않았고
/// 끝 칸이 비었으며 그 앞 칸에 글이 있다. 다음 줄 첫 칸이 넓은 글자면 이어진 줄로 본다(ghostty는 이 칸을 spacer로 표시한다).
fn wide_split(s: &vt100::Screen, r: u16, cols: u16) -> bool {
    cols >= 2
        && !s.row_wrapped(r)
        && s.cell(r, cols - 1).is_some_and(|c| !c.has_contents() && Look::of(c).plain())
        && s.cell(r, cols - 2).is_some_and(|c| c.has_contents() || c.is_wide_continuation())
}

/// 기록(오래된 것부터)과 화면의 줄마다 f(screen, 보이는 줄 번호). f가 false면 멈춘다. 끝나면 scrollback 위치를 0으로 되돌린다.
fn each_row(screen: &mut vt100::Screen, mut f: impl FnMut(&vt100::Screen, u16) -> bool) {
    let (rows, _) = screen.size();
    screen.set_scrollback(usize::MAX);
    let mut back = screen.scrollback();
    'all: loop {
        screen.set_scrollback(back);
        // 위로 back줄 올린 화면의 첫 줄이 끝에서 back번째 기록 줄이다
        let take = if back == 0 { rows } else { back.min(usize::from(rows)) as u16 };
        for r in 0..take {
            if !f(screen, r) {
                break 'all;
            }
        }
        if back == 0 {
            break;
        }
        back -= usize::from(take);
    }
    screen.set_scrollback(0);
}

/// 한 칸의 모양(vt100 Cell이 알려 주는 것)
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct Look {
    fg: vt100::Color,
    bg: vt100::Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Look {
    fn of(c: &vt100::Cell) -> Look {
        Look {
            fg: c.fgcolor(),
            bg: c.bgcolor(),
            bold: c.bold(),
            dim: c.dim(),
            italic: c.italic(),
            underline: c.underline(),
            inverse: c.inverse(),
        }
    }

    /// 지운 칸처럼 보인다(줄 끝에서 버려도 같은 모습)
    fn plain(&self) -> bool {
        self.bg == vt100::Color::Default && !self.inverse
    }

    /// 앞 모양(prev)에서 이 모양으로 바꾸는 SGR. 앞이 기본이면 지우지 않고 켤 것만 말한다(vt100이 쓰는 모양과 같다)
    fn sgr(&self, prev: &Look, out: &mut Vec<u8>) {
        use std::io::Write;
        out.extend_from_slice(b"\x1b[");
        let mut sep = if *prev == Look::default() {
            ""
        } else {
            out.push(b'0');
            ";"
        };
        for (on, code) in [(self.bold, 1), (self.dim, 2), (self.italic, 3), (self.underline, 4), (self.inverse, 7)] {
            if on {
                let _ = write!(out, "{sep}{code}");
                sep = ";";
            }
        }
        for (color, base, bright, ext) in [(self.fg, 30u16, 90u16, 38), (self.bg, 40, 100, 48)] {
            let _ = match color {
                vt100::Color::Default => continue,
                vt100::Color::Idx(i) if i < 8 => write!(out, "{sep}{}", base + u16::from(i)),
                vt100::Color::Idx(i) if i < 16 => write!(out, "{sep}{}", bright + u16::from(i) - 8),
                vt100::Color::Idx(i) => write!(out, "{sep}{ext};5;{i}"),
                vt100::Color::Rgb(r, g, b) => write!(out, "{sep}{ext};2;{r};{g};{b}"),
            };
            sep = ";";
        }
        out.push(b'm');
    }
}

/// 논리 줄 하나(접힌 줄을 이은 것)의 칸
#[derive(Default)]
struct Line {
    text: String,
    /// (text 안 끝 위치, 넓은 글자인가, 글이 있는가, 모양)
    cells: Vec<(usize, bool, bool, Look)>,
}

fn w(wide: bool) -> usize {
    if wide { 2 } else { 1 }
}

impl Line {
    fn push(&mut self, c: &vt100::Cell) {
        self.text.push_str(c.contents());
        self.cells.push((self.text.len(), c.is_wide(), c.has_contents(), Look::of(c)));
    }

    fn width(&self) -> usize {
        self.cells.iter().map(|c| w(c.1)).sum()
    }

    /// 줄 끝의 지운 칸을 뺀 칸 수
    fn trimmed(&self) -> usize {
        let mut n = self.cells.len();
        while n > 0 && !self.cells[n - 1].2 && self.cells[n - 1].3.plain() {
            n -= 1;
        }
        n
    }

    /// 열 offset(줄 처음부터)이 폭 cols로 쓴 줄에서 놓이는 (줄, 칸): 그 offset의 칸이 쓰일 자리. vt100처럼 넓은 글자가 끝 칸
    /// 하나에 들지 않으면 다음 줄로 가고(`col_wrap`), 줄 끝에 닿으면 다음 줄 처음이다. 글 뒤는 좁은 빈 칸이 이어진다고 본다.
    fn place(&self, offset: usize, cols: usize) -> (usize, usize) {
        let (mut row, mut col, mut at) = (0, 0, 0);
        for i in 0.. {
            let cw = self.cells.get(i).map_or(1, |c| w(c.1));
            if col + cw > cols {
                row += 1;
                col = 0;
            }
            if at + cw > offset {
                break;
            }
            col += cw;
            at += cw;
        }
        (row, col)
    }

    /// 폭 cols에서 앞 n칸이 차지하는 줄 수(빈 줄도 한 줄)
    fn rows(&self, n: usize, cols: usize) -> usize {
        let (mut row, mut col) = (0, 0);
        for c in &self.cells[..n] {
            if col + w(c.1) > cols {
                row += 1;
                col = 0;
            }
            col += w(c.1);
        }
        row + 1
    }

    /// 앞 n칸 가운데 폭 cols의 앞 k행에 드는 칸 수
    fn fit(&self, n: usize, k: usize, cols: usize) -> usize {
        let (mut row, mut col) = (0, 0);
        for (i, c) in self.cells[..n].iter().enumerate() {
            if col + w(c.1) > cols {
                row += 1;
                col = 0;
            }
            if row >= k {
                return i;
            }
            col += w(c.1);
        }
        n
    }

    fn write(&self, n: usize, out: &mut Vec<u8>) {
        let mut look = Look::default();
        let mut from = 0;
        // 바로 앞에 쓴 칸의 글과 그 뒤 out 길이(그 뒤에 escape를 썼으면 길이가 다르다)
        let mut prev: Option<(usize, &str)> = None;
        for &(end, _, has, l) in &self.cells[..n] {
            if l != look {
                l.sgr(&look, out);
                look = l;
            }
            let text = if has { &self.text[from..end] } else { " " };
            // 따로 놓인 칸인데 escape 없이 이어 쓰면 받는 터미널이 앞 칸에 붙이는 글(사이의 escape로 끊겼던 묶음, plan §8.167):
            // 모양을 다시 써서 끊는다
            if prev.is_some_and(|(at, p)| at == out.len() && vt100::joins_after(p, text)) {
                look.sgr(&look, out);
            }
            out.extend_from_slice(text.as_bytes());
            prev = Some((out.len(), text));
            from = end;
        }
    }
}

/// 다시 쓰는 중의 상태: 새 배치에서 쓴 줄 수와 커서·화면 맨 위가 간 자리
struct Rewrap {
    cols: usize,
    out: Vec<u8>,
    line: Line,
    /// 새 배치에서 지금까지 쓴 행 수
    rows: usize,
    /// 쓴 논리 줄 수(둘째부터 앞에 줄바꿈)
    lines: usize,
    /// 아직 쓰지 않은 빈 줄(뒤에 글이 오면 쓰고, 끝까지 비면 버린다)
    blank: usize,
    /// 이 논리 줄 안의 열 offset: 커서, 화면 맨 위
    cursor_at: Option<usize>,
    top_at: Option<usize>,
    cursor: (usize, usize),
    top: usize,
    /// 커서 줄부터 쓴 것. 줄 수가 줄면 커서 아래 행을 버리려고 둔다(화면 하나 넘게 쌓이지 않는다)
    tail: Vec<Tail>,
}

/// 커서 줄부터 쓴 행(들): 첫 행 번호, 그 행을 시작한 줄바꿈 뒤 out 길이, 글 줄이면 그 줄과 쓴 칸 수(빈 행이면 None)
struct Tail {
    row: usize,
    at: usize,
    line: Option<(Line, usize)>,
}

impl Rewrap {
    fn newline(&mut self) {
        if self.lines > 0 {
            self.out.extend_from_slice(b"\x1b[0m\r\n");
        }
        self.lines += 1;
    }

    fn finish(&mut self) {
        let n = self.line.trimmed();
        if n == 0 && self.cursor_at.is_none() && self.top_at.is_none() {
            self.blank += 1;
            self.line = Line::default();
            return;
        }
        for _ in 0..std::mem::take(&mut self.blank) {
            self.newline();
            if !self.tail.is_empty() {
                self.tail.push(Tail { row: self.rows, at: self.out.len(), line: None });
            }
            self.rows += 1;
        }
        self.newline();
        let start = self.rows;
        let at = self.out.len();
        self.line.write(n, &mut self.out);
        let mut used = self.line.rows(n, self.cols);
        let cursor_line = self.cursor_at.is_some();
        if let Some(off) = self.cursor_at.take() {
            let (r, c) = self.line.place(off, self.cols);
            self.cursor = (start + r, c);
            // 커서가 글 아래 줄이면 그 줄까지 만든다
            while used < r + 1 {
                self.out.extend_from_slice(b"\x1b[0m\r\n");
                used += 1;
            }
        }
        if let Some(off) = self.top_at.take() {
            self.top = start + self.line.place(off, self.cols).0;
        }
        self.rows += used;
        let line = std::mem::take(&mut self.line);
        if cursor_line || !self.tail.is_empty() {
            self.tail.push(Tail { row: start, at, line: Some((line, n)) });
        }
    }
}

/// normal screen(기록과 화면)을 rows×cols로 다시 쓰는 bytes(그 크기의 빈 터미널에 쓴다). xterm처럼 폭을 먼저 다시 접고 줄 수를
/// 그다음 바꾼다. 화면 맨 위는 전과 같은 줄에서 시작하고(clear 뒤 맨 위의 프롬프트가 그대로), 넘치면 맨 아래에 맞춘다. 커서는
/// 같은 글자 자리로 간다.
fn rewrap(screen: &mut vt100::Screen, rows: u16, cols: u16) -> Vec<u8> {
    let (old_rows, old_cols) = screen.size();
    let (cur_row, cur_col) = screen.cursor_position();
    screen.set_scrollback(usize::MAX);
    let hist = screen.scrollback();
    let cursor_abs = hist + usize::from(cur_row);
    let mut st = Rewrap {
        cols: usize::from(cols),
        out: Vec::new(),
        line: Line::default(),
        rows: 0,
        lines: 0,
        blank: 0,
        cursor_at: None,
        top_at: None,
        cursor: (0, 0),
        top: 0,
        tail: Vec::new(),
    };
    let mut abs = 0usize;
    // 앞 줄이 이 줄로 이어지는가: 접혔거나, vt100이 넓은 글자를 끝 칸 하나에 넣지 못하고 이음 표시 없이 내린 줄(끝 칸이 비고
    // 그 앞에 글이 있으며 이 줄 첫 칸이 넓은 글자)
    let mut wrapped = false;
    let mut split_wide = false;
    // 화면에서 글이 있는 마지막 줄
    let mut last_content: Option<usize> = None;
    each_row(screen, |s, r| {
        let first_wide = s.cell(r, 0).is_some_and(vt100::Cell::is_wide);
        let joins = wrapped || (split_wide && first_wide);
        if abs > 0 && !joins {
            st.finish();
        } else if split_wide && first_wide && !wrapped {
            // 앞 줄 끝에 남은 빈 칸은 빼고 잇는다
            st.line.cells.pop();
        }
        let base = st.line.width();
        if abs == cursor_abs {
            st.cursor_at = Some(base + usize::from(cur_col));
        }
        if abs == hist {
            st.top_at = Some(base);
        }
        for c in 0..old_cols {
            if let Some(cell) = s.cell(r, c)
                && !cell.is_wide_continuation()
            {
                st.line.push(cell);
            }
        }
        wrapped = s.row_wrapped(r);
        split_wide = wide_split(s, r, old_cols);
        if abs >= hist && (0..old_cols).any(|c| s.cell(r, c).is_some_and(|c| c.has_contents() || !Look::of(c).plain()))
        {
            last_content = Some(abs - hist);
        }
        abs += 1;
        true
    });
    st.finish();

    // 폭: 화면 아래의 빈 줄(커서와 글 아래)은 늘어난 글이 먼저 쓰고, 줄어들면 그대로 둔다. 남은 화면은 맨 아래에 맞추므로 넘친
    // 글은 기록으로 가고, 넓혀서 줄면 기록이 다시 내려온다(xterm·ghostty reflow와 같다). clear 뒤 맨 위 프롬프트는 그대로 맨 위다.
    let (old_rows, rows) = (usize::from(old_rows), usize::from(rows));
    let used = last_content.unwrap_or(0).max(usize::from(cur_row));
    let blank = (old_rows - 1 - used).saturating_sub((st.rows - st.top).saturating_sub(used + 1));
    let top = (st.rows + blank).saturating_sub(old_rows);
    // 줄 수: 줄면 커서 아래 행을 먼저 버리고, 그래도 커서가 밖이면 위 행을 기록으로 올린다(xterm `Buffer.resize`). 늘면 아래에
    // 빈 행이 붙는다(vt100 `set_size`와 같다)
    let top = if rows < old_rows { top.max((st.cursor.0 + 1).saturating_sub(rows)) } else { top };
    let end = top + rows;
    if end >= st.rows {
        // 글은 다 쓴다. 맨 위가 첫 행이 아니면 end행까지 써야 그 행이 맨 위다
        if top > 0 {
            for _ in st.rows..end {
                st.out.extend_from_slice(b"\x1b[0m\r\n");
            }
        }
    } else {
        // end행부터는 버린다: 행 end-1을 담은 것(커서 줄부터 둔 것 가운데)에서 자른다
        let i = st.tail.iter().rposition(|t| t.row < end).expect("커서 줄은 end 앞이다");
        let t = &st.tail[i];
        st.out.truncate(t.at);
        if let Some((line, n)) = &t.line {
            let k = end - t.row;
            let m = line.fit(*n, k, st.cols);
            line.write(m, &mut st.out);
            // 커서가 글 아래 행이면 그 행까지
            for _ in line.rows(m, st.cols)..k {
                st.out.extend_from_slice(b"\x1b[0m\r\n");
            }
        }
    }
    let row = st.cursor.0 - top;
    st.out.extend_from_slice(format!("\x1b[0m\x1b[{};{}H", row + 1, st.cursor.1 + 1).as_bytes());
    st.out
}

/// 지운 화면에 보이는 줄을 위에서부터 글과 글자 모양으로 쓰고 커서를 둔다(`history_row`, 커서는 vt100 `cursor_state_formatted`).
/// 다음 줄로 이어지는 줄은 줄바꿈 없이 이어 써서 받는 터미널(xterm)이 같은 폭에서 다시 접고 이음 표시를 갖는다. 그래야 그 화면이
/// 제 크기를 걸 때 xterm이 줄을 잇는다. vt100 `contents_formatted`는 넓은 글자를 다음 줄로 내린 줄을 줄바꿈으로 써서 잇지 못했다.
fn screen_rows(s: &vt100::Screen, out: &mut Vec<u8>) {
    let (rows, cols) = s.size();
    for r in 0..rows {
        let last = r + 1 == rows;
        let joins = !last && joins_next(s, r, cols, s.cell(r + 1, 0).is_some_and(vt100::Cell::is_wide));
        let at = out.len();
        history_row(s, r, cols, joins, out);
        // 맨 아래 줄 뒤 줄바꿈은 화면을 올린다
        if last && out[at..].ends_with(b"\r\n") {
            out.truncate(out.len() - 2);
        }
    }
    out.extend_from_slice(&s.cursor_state_formatted());
}

/// 보이는 줄 r이 다음 줄로 이어지는가: 접혔거나, 넓은 글자를 끝 칸에 넣지 못해 다음 줄로 내렸다(다음 줄 첫 칸이 넓다: `next_wide`)
fn joins_next(s: &vt100::Screen, r: u16, cols: u16, next_wide: bool) -> bool {
    s.row_wrapped(r) || (next_wide && wide_split(s, r, cols))
}

/// 기록 한 줄(보이는 줄 r)을 글과 글자 모양(SGR)만으로, 커서 이동 없이 쓴다. 다음 줄로 이어지면(`joins`) 줄바꿈하지 않아 받는
/// 터미널이 같은 폭에서 다시 접고 이음 표시를 가진다: 접힌 줄은 빈 칸까지 끝까지, 넓은 글자를 내린 줄은 끝 빈 칸을 빼고 쓴다.
/// 아니면 끝의 지운 칸을 빼고 줄바꿈한다.
fn history_row(s: &vt100::Screen, r: u16, cols: u16, joins: bool, out: &mut Vec<u8>) {
    let mut line = Line::default();
    for c in 0..cols {
        if let Some(cell) = s.cell(r, c)
            && !cell.is_wide_continuation()
        {
            line.push(cell);
        }
    }
    let full = joins && s.row_wrapped(r);
    line.write(if full { line.cells.len() } else { line.trimmed() }, out);
    out.extend_from_slice(if joins { b"\x1b[m" } else { b"\x1b[m\r\n" });
}

/// 가림이 붙잡은 글과 무관하게 질의 시점의 실제 커서를 계산한다.
/// 원문 화면은 프로세스 메모리에만 있고 조회·replay·저장 API를 노출하지 않는다.
/// scrollback은 보존하지 않으며, 외부 화면·기록은 별도의 가린 Vt가 맡는다.
pub struct QueryState(Vt);

impl QueryState {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self(Vt {
            parser: vt100::Parser::new_with_callbacks(rows.max(1), cols.max(1), 0, Modes::default()),
            scrollback: 0,
        })
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        self.0.feed(bytes);
    }
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.0.resize(rows, cols);
    }
    pub fn cursor_report(&self) -> Vec<u8> {
        self.0.cursor_report()
    }
}

/// 프로그램이 터미널에 묻는 것 중 session host가 답하는 것(답은 herdr 0.9.5의 ghostty와 같다)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Query {
    /// DA1 `CSI c`·`CSI 0 c`
    PrimaryAttributes,
    /// DA2 `CSI > c`·`CSI > 0 c`
    SecondaryAttributes,
    /// DSR 6 `CSI 6 n`(커서 위치)
    CursorPosition,
    /// OSC 10/11 `?`(기본 글자·바탕 색). 답은 `DefaultColors::reply`
    DefaultColor(DefaultColor, Terminator),
}

impl Query {
    /// 커서·색과 상관없는 답. 커서 위치는 질의 시점의 화면으로(`Vt::cursor_report`), 색은 `DefaultColors`로 답한다.
    /// DA1은 ghostty `device_attributes.zig` `Primary` 기본값(VT220·ANSI 색), DA2는 `Secondary` 기본값(VT220·판 0)이다.
    pub fn fixed_reply(self) -> Option<&'static [u8]> {
        match self {
            Query::PrimaryAttributes => Some(b"\x1b[?62;22c"),
            Query::SecondaryAttributes => Some(b"\x1b[>1;0;0c"),
            Query::CursorPosition | Query::DefaultColor(..) => None,
        }
    }
}

/// OSC 10(기본 글자 색)·11(기본 바탕 색)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultColor {
    Foreground,
    Background,
}

/// OSC의 끝: BEL이나 ST(`ESC \`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Terminator {
    Bel,
    St,
}

/// 출력에서 찾은 것
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// 답할 질의. 출력에서 뺐다
    Query(Query),
    /// 프로그램이 기본 색을 정했다(OSC 10/11 색). 출력에 남긴다(화면도 그 색으로 그린다)
    Set(DefaultColor, [u8; 3]),
    /// 프로그램이 기본 색을 되돌렸다(OSC 110/111, `ESC c`). 출력에 남긴다
    Reset(DefaultColor),
}

const QUERIES: &[(&[u8], Query)] = &[
    (b"\x1b[c", Query::PrimaryAttributes),
    (b"\x1b[0c", Query::PrimaryAttributes),
    (b"\x1b[>c", Query::SecondaryAttributes),
    (b"\x1b[>0c", Query::SecondaryAttributes),
    (b"\x1b[6n", Query::CursorPosition),
];

/// 기본 색 OSC의 앞. 끝(BEL·ST)까지 붙들어 본다
const COLOR_OSC: &[&[u8]] = &[b"\x1b]10;", b"\x1b]11;", b"\x1b]110", b"\x1b]111"];
/// 이보다 길도록 끝나지 않는 OSC는 기본 색 OSC로 보지 않고 그대로 보낸다
const COLOR_OSC_MAX: usize = 256;

/// 질의를 뺀 출력과, 그 출력의 어느 자리에서 무엇을 찾았는가
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scanned {
    pub out: Vec<u8>,
    /// (`out`에서의 자리, 찾은 것). 질의는 빠진 자리, 색 지정은 그 OSC 바로 뒤다. 커서 위치는 `out[..자리]`를 먹인 화면으로 답한다
    pub found: Vec<(usize, Found)>,
}

/// 출력에서 session host가 답할 질의를 빼낸다(chunk 경계에 걸쳐도). 터미널은 session host이므로 질의는 front(xterm)에 가지
/// 않는다: 붙은 front 수와 상관없이 답은 한 번이다(herdr는 서버의 ghostty가 답하고 front에는 그린 화면만 보낸다,
/// `src/pane/terminal.rs` `terminal_responses`). 질의일 수 있는 끝은 다음 출력까지 붙든다(끝나지 않은 escape라 어느 터미널도
/// 아직 그리지 않는다, herdr-web-ui `server/attach-output.ts` `AttachStartupProbes`). Windows ConPTY(INHERIT_CURSOR)는 시작할 때
/// 커서 위치를 묻고 답이 올 때까지 자식의 출력과 종료를 막는다. 색 질의에 답하려면 프로그램이 정한 기본 색도 알아야 해서
/// OSC 10/11 지정·110/111·`ESC c`도 찾는다(herdr `DefaultColorOscTracker`).
#[derive(Default)]
pub struct QueryScanner {
    held: Vec<u8>,
}

impl QueryScanner {
    pub fn scan(&mut self, data: &[u8]) -> Scanned {
        let input = if self.held.is_empty() {
            std::borrow::Cow::Borrowed(data)
        } else {
            let mut v = std::mem::take(&mut self.held);
            v.extend_from_slice(data);
            std::borrow::Cow::Owned(v)
        };
        let mut s = Scanned { out: Vec::with_capacity(input.len()), found: Vec::new() };
        let mut i = 0;
        while i < input.len() {
            let Some(esc) = input[i..].iter().position(|&b| b == 0x1b) else {
                s.out.extend_from_slice(&input[i..]);
                break;
            };
            s.out.extend_from_slice(&input[i..i + esc]);
            i += esc;
            let rest = &input[i..];
            if let Some((q, kind)) = QUERIES.iter().find(|(q, _)| rest.starts_with(q)) {
                s.found.push((s.out.len(), Found::Query(*kind)));
                i += q.len();
                continue;
            }
            if rest.starts_with(b"\x1bc") {
                s.out.extend_from_slice(b"\x1bc");
                s.found.push((s.out.len(), Found::Reset(DefaultColor::Foreground)));
                s.found.push((s.out.len(), Found::Reset(DefaultColor::Background)));
                i += 2;
                continue;
            }
            if COLOR_OSC.iter().any(|p| rest.starts_with(p)) {
                match osc_end(rest) {
                    OscEnd::Done { body, len, term } => {
                        match color_osc(&rest[2..body], term) {
                            Some(ColorOsc::Queries(qs)) => {
                                s.found.extend(qs.into_iter().map(|q| (s.out.len(), Found::Query(q))));
                            }
                            Some(ColorOsc::Keep(found)) => {
                                s.out.extend_from_slice(&rest[..len]);
                                s.found.extend(found.into_iter().map(|f| (s.out.len(), f)));
                            }
                            None => s.out.extend_from_slice(&rest[..len]),
                        }
                        i += len;
                        continue;
                    }
                    OscEnd::Open if rest.len() < COLOR_OSC_MAX => {
                        self.held = rest.to_vec();
                        break;
                    }
                    OscEnd::Open | OscEnd::Broken => {}
                }
            } else if QUERIES.iter().any(|(q, _)| q.starts_with(rest)) || COLOR_OSC.iter().any(|p| p.starts_with(rest))
            {
                self.held = rest.to_vec();
                break;
            }
            s.out.push(0x1b);
            i += 1;
        }
        s
    }

    /// 붙들고 있는 끝(세션이 끝날 때 마저 내보낸다)
    pub fn take_held(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }
}

enum OscEnd {
    /// `body`까지가 내용(`ESC ]` 뒤부터), `len`은 끝까지 길이
    Done { body: usize, len: usize, term: Terminator },
    /// 아직 끝나지 않았다
    Open,
    /// ST가 아닌 escape가 끼었다(OSC가 아니게 됐다)
    Broken,
}

fn osc_end(rest: &[u8]) -> OscEnd {
    for (j, &b) in rest.iter().enumerate().skip(2) {
        match b {
            0x07 => return OscEnd::Done { body: j, len: j + 1, term: Terminator::Bel },
            0x1b => {
                return match rest.get(j + 1) {
                    Some(b'\\') => OscEnd::Done { body: j, len: j + 2, term: Terminator::St },
                    Some(_) => OscEnd::Broken,
                    None => OscEnd::Open,
                };
            }
            _ => {}
        }
    }
    OscEnd::Open
}

enum ColorOsc {
    /// 모두 질의다(`10;?`, `10;?;?`처럼 이어진 것도). 출력에서 뺀다
    Queries(Vec<Query>),
    /// 색 지정·되돌리기. 출력에 남긴다(알아보지 못한 색은 사건 없이)
    Keep(Vec<Found>),
}

/// `10;…`·`11;…`·`110`·`111`. xterm처럼 `10;a;b`는 10에 a, 11에 b다(12부터는 다른 색이라 보지 않는다).
/// 질의와 지정이 섞였거나 12 이상에 닿는 질의는 None(화면이 받는다)
fn color_osc(body: &[u8], term: Terminator) -> Option<ColorOsc> {
    let which = |n: usize| match n {
        10 => Some(DefaultColor::Foreground),
        11 => Some(DefaultColor::Background),
        _ => None,
    };
    let text = std::str::from_utf8(body).ok()?;
    let (num, args) = text.split_once(';').unwrap_or((text, ""));
    match num {
        "110" => return Some(ColorOsc::Keep(vec![Found::Reset(DefaultColor::Foreground)])),
        "111" => return Some(ColorOsc::Keep(vec![Found::Reset(DefaultColor::Background)])),
        _ => {}
    }
    let first: usize = num.parse().ok()?;
    let items: Vec<(usize, &str)> = args.split(';').enumerate().map(|(k, a)| (first + k, a)).collect();
    if items.iter().all(|(_, a)| *a == "?") {
        let qs =
            items.iter().map(|(n, _)| which(*n).map(|c| Query::DefaultColor(c, term))).collect::<Option<Vec<_>>>()?;
        return Some(ColorOsc::Queries(qs));
    }
    if items.iter().any(|(_, a)| *a == "?") {
        return None;
    }
    let found = items.iter().filter_map(|(n, a)| Some(Found::Set(which(*n)?, parse_color(a)?))).collect();
    Some(ColorOsc::Keep(found))
}

/// xterm 색 지정 중 `#`(3·6·9·12자리)·hex 3·6자리·`rgb:r/g/b`(1~4자리)를 읽는다(ghostty `color.zig` `RGB.parse`, X11 이름과
/// `rgbi:`는 읽지 않는다)
fn parse_color(spec: &str) -> Option<[u8; 3]> {
    let hex = |v: &str| -> Option<u8> {
        if v.is_empty() || v.len() > 4 {
            return None;
        }
        let n = u32::from_str_radix(v, 16).ok()?;
        Some((n * 255 / ((1u32 << (4 * v.len())) - 1)) as u8)
    };
    let spec = spec.trim_matches([' ', '\t']);
    let digits = spec
        .strip_prefix('#')
        .filter(|d| matches!(d.len(), 3 | 6 | 9 | 12))
        .or(Some(spec).filter(|d| matches!(d.len(), 3 | 6)));
    if let Some(d) = digits.filter(|d| d.is_ascii()) {
        let w = d.len() / 3;
        return Some([hex(&d[..w])?, hex(&d[w..2 * w])?, hex(&d[2 * w..])?]);
    }
    let mut parts = spec.strip_prefix("rgb:")?.split('/');
    let rgb = [hex(parts.next()?)?, hex(parts.next()?)?, hex(parts.next()?)?];
    parts.next().is_none().then_some(rgb)
}

/// 기본 색 질의의 답: 프로그램이 정한 색이 먼저, 아니면 화면이 알려 준 색(herdr `default_color_query_response`). 둘 다 없으면
/// 답하지 않는다(herdr도 host theme을 모르면 ghostty 기본 색이 비어 답이 없다)
#[derive(Debug, Default, Clone)]
pub struct DefaultColors {
    /// 화면이 알려 준 색
    pub host: Option<TermColors>,
    child: [Option<[u8; 3]>; 2],
}

impl DefaultColors {
    pub fn new(host: Option<TermColors>) -> DefaultColors {
        DefaultColors { host, child: [None; 2] }
    }

    /// 출력에서 찾은 색 지정·되돌리기를 적는다
    pub fn note(&mut self, f: Found) {
        match f {
            Found::Set(c, rgb) => self.child[c as usize] = Some(rgb),
            Found::Reset(c) => self.child[c as usize] = None,
            Found::Query(_) => {}
        }
    }

    /// 프로그램이 정한 색은 질의의 끝으로(ghostty 기본 답), 화면 색은 늘 ST로(herdr `osc_rgb_response`) 답한다
    pub fn reply(&self, c: DefaultColor, term: Terminator) -> Option<Vec<u8>> {
        let host = self.host.map(|h| if c == DefaultColor::Foreground { h.fg } else { h.bg });
        let (rgb, term) = match self.child[c as usize] {
            Some(rgb) => (rgb, term),
            None => (host?, Terminator::St),
        };
        let n = if c == DefaultColor::Foreground { 10 } else { 11 };
        let [r, g, b] = rgb.map(|v| u16::from(v) * 257);
        let end: &str = if term == Terminator::Bel { "\x07" } else { "\x1b\\" };
        Some(format!("\x1b]{n};rgb:{r:04x}/{g:04x}/{b:04x}{end}").into_bytes())
    }
}

#[cfg(test)]
mod width_cases;

#[cfg(test)]
mod tests;
