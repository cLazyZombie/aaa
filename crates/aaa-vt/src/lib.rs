//! VT adapter. 정본 화면 상태는 session host가 갖는다(기획서 §4.5).
//!
//! 기획서는 ghostty-vt를 1순위로 두었지만 빌드에 Zig 0.16이 필요해 M1-1은 순수 Rust `vt100`을 쓴다.
//! 이 adapter 뒤에서 엔진을 바꿀 수 있게 front·host 쪽은 replay bytes와 글자 화면만 본다.

use aaa_proto::{GridRun, MouseEncoding, MouseMode, PaneModes, ReadFormat, ReadSource, ScreenText};

pub mod copy;

pub struct Vt {
    parser: vt100::Parser<Modes>,
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
        Vt { parser: vt100::Parser::new_with_callbacks(rows.max(1), cols.max(1), SCROLLBACK, Modes::default()) }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    /// 크기를 바꾼다. xterm처럼 폭을 먼저, 줄 수를 그다음 바꾼다.
    /// - 폭이 바뀌면 기록과 화면을 새 폭으로 다시 접는다(reflow). vt100 `set_size`는 폭이 줄면 줄 끝을 버려, 휴대폰이 좁게 쥐었다
    ///   놓으면 넓은 화면에서 줄 오른쪽이 비었다(coordinator 10-18, t-0330 발견). herdr는 ghostty가 다시 접는다(src/pane.rs 110줄).
    /// - 줄 수만 바뀌어도 화면에 접힌 줄이 있으면 다시 쓴다: vt100 `set_size`는 보이는 줄의 이음 표시를 지워(`Row::resize`) 휴대폰
    ///   키보드가 열렸다 닫히기만 해도 나중에 넓힐 때 그 줄이 펴지지 않았다.
    /// - 줄이 줄면 커서 아래 줄을 먼저 버리고, 그래도 커서 줄이 새 높이 밖이면 그만큼 위 줄을 기록으로 올린다. vt100 `set_size`는
    ///   아래 줄을 잘라 버려(커서 줄과 프롬프트가 사라진다) xterm·herdr(ghostty)와 달랐다(plan §8.59).
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
        let mut p = vt100::Parser::new_with_callbacks(rows, cols, SCROLLBACK, modes);
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

    /// 맨 아래에서 `offset`줄 올라간 자리의 화면 줄과 normal screen 기록 줄 수(TUI 휠 기록 넘기기, herdr `scroll_up`).
    /// alt 화면이면 기록이 없다(지금 화면, 0). 기록 위치는 읽고 바로 되돌린다(다른 읽기는 늘 지금 화면을 본다).
    pub fn grid_rows_at(&mut self, offset: usize) -> (Vec<Vec<GridRun>>, usize) {
        let (rows, _) = self.parser.screen().size();
        if self.parser.screen().alternate_screen() {
            return ((0..rows).map(|r| self.grid_row(r)).collect(), 0);
        }
        let max = self.history_len();
        self.parser.screen_mut().set_scrollback(offset.min(max));
        let lines = (0..rows).map(|r| self.grid_row(r)).collect();
        self.parser.screen_mut().set_scrollback(0);
        (lines, max)
    }

    /// 고른 글(herdr `PaneSelectionRead` → ghostty `read_text_screen`). 줄 번호는 기록 맨 앞 줄이 0이고 화면 줄이 그 뒤에
    /// 잇는다(alt 화면이면 화면 줄만). `from`·`to`는 읽는 차례와 상관없고 끝 칸까지 넣는다. 화면 폭에서 접힌 줄은 줄바꿈 없이
    /// 잇고, 접히지 않은 줄은 끝 공백을 뺀다. `max_bytes`를 넘으면 앞쪽만 두고 `true`.
    pub fn selection_text(&mut self, from: (usize, u16), to: (usize, u16), max_bytes: usize) -> (String, bool) {
        let ((r1, c1), (r2, c2)) = if from <= to { (from, to) } else { (to, from) };
        let back = if self.parser.screen().alternate_screen() { 0 } else { self.history_len() };
        let screen = self.parser.screen_mut();
        let (rows, cols) = screen.size();
        let last = back + usize::from(rows) - 1;
        if r1 > last {
            return (String::new(), false);
        }
        let r2 = r2.min(last);
        let mut out = String::new();
        let mut truncated = false;
        for i in r1..=r2 {
            // 기록 줄 i를 맨 위에 두려면 위로 (back - i)줄 올린다. 화면 줄이면 올리지 않는다.
            let (offset, row) = if i < back { (back - i, 0) } else { (0, i - back) };
            screen.set_scrollback(offset);
            let start = if i == r1 { c1.min(cols) } else { 0 };
            let end = if i == r2 { c2.saturating_add(1).min(cols) } else { cols };
            let mut t = screen.rows(start, end.saturating_sub(start)).nth(row).unwrap_or_default();
            let wrapped = screen.row_wrapped(row as u16);
            if !wrapped || i == r2 {
                t.truncate(t.trim_end().len());
            }
            if out.len() + t.len() + 1 > max_bytes {
                truncated = true;
                if out.ends_with('\n') {
                    out.pop();
                }
                break;
            }
            out.push_str(&t);
            if i < r2 && !wrapped {
                out.push('\n');
            }
        }
        screen.set_scrollback(0);
        (out, truncated)
    }

    /// copy 모드 낱말·문단 이동(herdr `pane.copy_motion`). 갈 곳이 없으면 커서 그대로, 문단 이동은 칸을 지킨다.
    pub fn copy_motion(&mut self, cursor: copy::Point, m: copy::Motion) -> copy::Point {
        let total = self.total_rows();
        if cursor.0 >= total {
            return cursor;
        }
        match m {
            copy::Motion::PreviousParagraph | copy::Motion::NextParagraph => {
                // herdr `paragraph_motion_target`: 999줄 안의 바로 위·아래 빈 줄(이어진 빈 줄도 한 줄씩)
                let reach = total.min(1000) - 1;
                let up = m == copy::Motion::PreviousParagraph;
                let (lo, hi) = if up {
                    if cursor.0 == 0 {
                        return cursor;
                    }
                    (cursor.0.saturating_sub(reach), cursor.0 - 1)
                } else {
                    if cursor.0 + 1 >= total {
                        return cursor;
                    }
                    (cursor.0 + 1, (cursor.0 + reach).min(total - 1))
                };
                let rows = self.text_rows(lo, hi);
                let at = if up { rows.iter().rposition(copy::is_blank) } else { rows.iter().position(copy::is_blank) };
                at.map_or(cursor, |k| (lo + k, cursor.1))
            }
            _ => self.word_motion(cursor, m, total).unwrap_or(cursor),
        }
    }

    /// herdr `word_motion_target`: 64줄 창에서 찾고, 못 찾았거나 낱말이 창 끝 너머로 이어질 수 있으면 창을 두 배로
    fn word_motion(&mut self, cursor: copy::Point, m: copy::Motion, total: usize) -> Option<copy::Point> {
        let mut window = 64usize;
        loop {
            let (start, end) = if m.backward() {
                (cursor.0.saturating_sub(window - 1), cursor.0)
            } else {
                (cursor.0, (cursor.0 + window - 1).min(total - 1))
            };
            // 뒤로 갈 때는 창 바로 위 줄이 접혔는지(낱말이 위로 이어지는가)도 본다
            let lead = m.backward() && start > 0;
            let read = self.text_rows(if lead { start - 1 } else { start }, end);
            let (above_wrapped, rows) = if lead { (read[0].wrapped, &read[1..]) } else { (false, &read[..]) };
            let target = copy::word_target(rows, start, cursor, m);
            let more = match target {
                None => true,
                Some(t) if m.backward() => t.0 == start && above_wrapped,
                Some(t) if m.word_end() => {
                    rows.last().is_some_and(|r| r.wrapped && r.cells.last().is_some_and(|c| (end, c.col) == t))
                }
                Some(_) => false,
            };
            let edge = if m.backward() { start == 0 } else { end + 1 >= total };
            if !more || edge {
                return target;
            }
            window *= 2;
        }
    }

    /// copy 모드 찾기(herdr `pane.copy_search`): 기록과 화면 전체에서 `query`를 찾아 커서(`previous`가 있으면 그 범위) 다음
    /// 일치와 그 언저리 `keep`개를 준다
    pub fn copy_search(
        &mut self,
        query: &str,
        forward: bool,
        cursor: copy::Point,
        previous: Option<copy::Range>,
        keep: usize,
    ) -> copy::Found {
        let Some(m) = copy::Matcher::new(query) else { return copy::Found::default() };
        let total = self.total_rows();
        let (mut lines, mut all) = (copy::Lines::default(), Vec::new());
        let mut i = 0;
        while i < total {
            let end = (i + 255).min(total - 1);
            for (k, r) in self.text_rows(i, end).iter().enumerate() {
                lines.push_row(i + k, r, &m, &mut all);
            }
            i = end + 1;
        }
        lines.finish(&m, &mut all);
        copy::pick(all, forward, cursor, previous, keep)
    }

    /// 기록과 화면을 이은 줄 수(alt 화면이면 화면 줄 수)
    fn total_rows(&mut self) -> usize {
        let back = self.history_rows();
        back + usize::from(self.parser.screen().size().0)
    }

    /// 절대 줄 `start..=end`의 칸(넓은 글자 뒤 칸은 뺀다)과 접힘. 화면 높이씩 기록을 올려 읽고 맨 아래로 되돌린다.
    fn text_rows(&mut self, start: usize, end: usize) -> Vec<copy::Row> {
        let back = self.history_rows();
        let screen = self.parser.screen_mut();
        let (rows, cols) = screen.size();
        let total = back + usize::from(rows);
        if start >= total || start > end {
            return vec![];
        }
        let end = end.min(total - 1);
        let mut out = Vec::with_capacity(end + 1 - start);
        let mut i = start;
        while i <= end {
            let (offset, first) = if i < back { (back - i, 0) } else { (0, i - back) };
            screen.set_scrollback(offset);
            let take = (usize::from(rows) - first).min(end + 1 - i);
            for k in 0..take {
                let r = (first + k) as u16;
                let mut cells = Vec::with_capacity(usize::from(cols));
                for c in 0..cols {
                    let Some(cell) = screen.cell(r, c) else { continue };
                    if cell.is_wide_continuation() {
                        continue;
                    }
                    let end_col = if cell.is_wide() { (c + 1).min(cols - 1) } else { c };
                    cells.push(copy::Cell { col: c, end_col, text: cell.contents().to_string() });
                }
                out.push(copy::Row { cells, wrapped: screen.row_wrapped(r) });
            }
            i += take;
        }
        screen.set_scrollback(0);
        out
    }

    /// normal screen 기록 줄 수(alt 화면이면 0)
    pub fn history_rows(&mut self) -> usize {
        if self.parser.screen().alternate_screen() { 0 } else { self.history_len() }
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

fn color(c: vt100::Color) -> u32 {
    match c {
        vt100::Color::Default => 0,
        vt100::Color::Idx(i) => 0x0100_0000 | u32::from(i),
        vt100::Color::Rgb(r, g, b) => 0x0200_0000 | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b),
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
}

impl Query {
    /// 커서와 상관없는 답. 커서 위치는 질의 시점의 화면으로 답한다(`Vt::cursor_report`).
    /// DA1은 ghostty `device_attributes.zig` `Primary` 기본값(VT220·ANSI 색), DA2는 `Secondary` 기본값(VT220·판 0)이다.
    pub fn fixed_reply(self) -> Option<&'static [u8]> {
        match self {
            Query::PrimaryAttributes => Some(b"\x1b[?62;22c"),
            Query::SecondaryAttributes => Some(b"\x1b[>1;0;0c"),
            Query::CursorPosition => None,
        }
    }
}

const QUERIES: &[(&[u8], Query)] = &[
    (b"\x1b[c", Query::PrimaryAttributes),
    (b"\x1b[0c", Query::PrimaryAttributes),
    (b"\x1b[>c", Query::SecondaryAttributes),
    (b"\x1b[>0c", Query::SecondaryAttributes),
    (b"\x1b[6n", Query::CursorPosition),
];

/// 질의를 뺀 출력과, 그 출력의 어느 자리에 어떤 질의가 있었는가
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scanned {
    pub out: Vec<u8>,
    /// (`out`에서 질의가 있던 자리, 질의). 커서 위치는 `out[..자리]`를 먹인 화면으로 답한다
    pub queries: Vec<(usize, Query)>,
}

/// 출력에서 session host가 답할 질의를 빼낸다(chunk 경계에 걸쳐도). 터미널은 session host이므로 질의는 front(xterm)에 가지
/// 않는다: 붙은 front 수와 상관없이 답은 한 번이다(herdr는 서버의 ghostty가 답하고 front에는 그린 화면만 보낸다,
/// `src/pane/terminal.rs` `terminal_responses`). 질의일 수 있는 끝은 다음 출력까지 붙든다(끝나지 않은 escape라 어느 터미널도
/// 아직 그리지 않는다, herdr-web-ui `server/attach-output.ts` `AttachStartupProbes`). Windows ConPTY(INHERIT_CURSOR)는 시작할 때
/// 커서 위치를 묻고 답이 올 때까지 자식의 출력과 종료를 막는다.
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
        let mut s = Scanned { out: Vec::with_capacity(input.len()), queries: Vec::new() };
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
                s.queries.push((s.out.len(), *kind));
                i += q.len();
            } else if QUERIES.iter().any(|(q, _)| q.starts_with(rest)) {
                self.held = rest.to_vec();
                break;
            } else {
                s.out.push(0x1b);
                i += 1;
            }
        }
        s
    }

    /// 붙들고 있는 끝(세션이 끝날 때 마저 내보낸다)
    pub fn take_held(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.held)
    }
}

#[cfg(test)]
mod width_cases;

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

    /// 질의는 출력에서 빠지고 자리와 함께 나온다. chunk 경계에 걸친 질의는 다음 chunk까지 붙들고, 질의가 아닌 escape는 그대로 둔다
    #[test]
    fn queries_are_taken_out_of_the_output_across_chunks() {
        use Query::*;
        let mut s = QueryScanner::default();
        let got = s.scan(b"ab\x1b[6ncd\x1b[c\x1b[0c\x1b[>c\x1b[>0cef");
        assert_eq!(got.out, b"abcdef");
        let at4 = [PrimaryAttributes, PrimaryAttributes, SecondaryAttributes, SecondaryAttributes].map(|q| (4, q));
        assert_eq!(got.queries, [vec![(2, CursorPosition)], at4.to_vec()].concat());
        // 끝의 앞부분은 붙들었다가 이어 본다
        assert_eq!(s.scan(b"x\x1b["), Scanned { out: b"x".to_vec(), queries: vec![] });
        assert_eq!(s.scan(b">"), Scanned::default());
        assert_eq!(s.scan(b"0cy\x1b"), Scanned { out: b"y".to_vec(), queries: vec![(0, SecondaryAttributes)] });
        assert_eq!(s.take_held(), b"\x1b");
        // 질의가 아닌 것: 다른 인자의 DA·DECXCPR·DSR 5·DA3·kitty 키보드 질의(aaa는 그 키 방식을 쓰지 않는다)·색·글자 모양
        let other: &[u8] = b"\x1b[1c\x1b[>1c\x1b[?6n\x1b[5n\x1b[=c\x1b[?u\x1b]11;?\x07\x1b[61n\x1b[31m\x1b\x1b[6";
        let got = s.scan(other);
        assert_eq!(got.out, &other[..other.len() - 3]);
        assert!(got.queries.is_empty());
        assert_eq!(s.scan(b"n"), Scanned { out: vec![], queries: vec![(0, CursorPosition)] });
        let mut v = Vt::new(5, 20);
        v.feed(b"hi\r\nthere");
        assert_eq!(v.cursor_report(), b"\x1b[2;6R");
        assert_eq!(PrimaryAttributes.fixed_reply(), Some(&b"\x1b[?62;22c"[..]));
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

    /// 화면 replay도 이어진 줄(넓은 글자를 내린 줄 포함)을 줄바꿈 없이 써서, 받은 터미널이 제 폭으로 넓히면 줄이 이어진다.
    /// 늦게 붙은 웹이 다른 기기 크기로 받은 뒤 제 크기를 거는 경우다
    #[test]
    fn replayed_screen_keeps_joins_for_the_receiver() {
        let mut a = Vt::new(6, 5);
        a.feed("가나다라마 끝\r\nabcdefgh\r\n$ ".as_bytes());
        // xterm은 이음 표시로만 잇는다: 논리 줄 안에 줄바꿈이 없어야 한다(vt100은 글로 알아보므로 아래 비교만으로는 모른다)
        let bytes = String::from_utf8_lossy(&a.replay()).replace("\x1b[m", "");
        assert!(bytes.contains("가나다라마 끝\r\nabcdefgh\r\n$"), "{bytes:?}");
        let mut b = Vt::new(6, 5);
        b.feed(&a.replay());
        assert_eq!(screen_lines(&b), screen_lines(&a));
        assert_eq!(b.cursor(), a.cursor());
        b.resize(6, 20);
        assert_eq!(screen_lines(&b)[..3], ["가나다라마 끝", "abcdefgh", "$"]);
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

    /// 기록 위치에서 읽은 줄(herdr `scroll_up`), 끝에서 멈춤, 읽은 뒤 지금 화면 그대로, alt 화면은 기록 없음
    #[test]
    fn grid_rows_at_scrollback_offset() {
        let mut v = Vt::new(3, 10);
        for i in 1..=8 {
            v.feed(format!("{i}\r\n").as_bytes());
        }
        let text = |rows: Vec<Vec<GridRun>>| {
            rows.iter().map(|r| r.iter().map(|g| g.t.as_str()).collect::<String>()).collect::<Vec<_>>()
        };
        let (now, max) = v.grid_rows_at(0);
        assert_eq!((text(now), max), (vec!["7".to_string(), "8".into(), String::new()], 6));
        assert_eq!(text(v.grid_rows_at(2).0), ["5", "6", "7"]);
        assert_eq!(text(v.grid_rows_at(99).0), ["1", "2", "3"]);
        assert_eq!(text(v.grid_rows_at(0).0), ["7", "8", ""]);
        assert_eq!(v.grid_row(0)[0].t, "7");
        v.feed(b"\x1b[?1049hA");
        let (alt, max) = v.grid_rows_at(5);
        assert_eq!((text(alt)[0].as_str(), max), ("A", 0));
    }

    /// 고른 글: 기록과 화면에 걸친 범위, 끝 칸 포함, 거꾸로 고른 범위, 접힌 줄 잇기, 끝을 넘으면 마지막 줄까지, 크기 상한,
    /// 읽은 뒤 지금 화면 그대로, alt 화면은 화면 줄만
    #[test]
    fn selection_text_spans_history() {
        let mut v = Vt::new(3, 10);
        for i in 1..=8 {
            v.feed(format!("line{i}\r\n").as_bytes());
        }
        // 기록 1..6(줄 0..5), 화면 7·8·빈 줄(줄 6..8)
        assert_eq!(
            v.selection_text((1, 2), (6, 3), usize::MAX),
            ("ne2\nline3\nline4\nline5\nline6\nline".into(), false)
        );
        assert_eq!(v.selection_text((6, 3), (1, 2), usize::MAX).0, "ne2\nline3\nline4\nline5\nline6\nline");
        assert_eq!(v.selection_text((7, 0), (99, 9), usize::MAX).0, "line8\n");
        assert_eq!(v.selection_text((0, 0), (5, 9), 12), ("line1\nline2".into(), true));
        assert_eq!(v.grid_row(0)[0].t, "line7");
        // 접힌 줄은 줄바꿈 없이(앞 글자 공백도 그대로)
        v.feed(b"abcdefghij  klm\r\n");
        let n = v.history_rows() + 3;
        assert_eq!(v.selection_text((n - 3, 0), (n - 2, 9), usize::MAX).0, "abcdefghij  klm");
        v.feed(b"\x1b[?1049h\x1b[HA\r\nB");
        assert_eq!(v.selection_text((0, 0), (1, 9), usize::MAX).0, "A\nB");
    }

    /// copy 모드 이동·찾기는 기록까지 보고(줄 번호는 기록 맨 앞이 0), 읽은 뒤 화면은 맨 아래 그대로
    #[test]
    fn copy_motion_and_search_span_history() {
        use copy::Motion::*;
        let mut v = Vt::new(3, 10);
        v.feed(b"alpha beta\r\n\r\ngamma\r\ndelta\r\n");
        // 줄 0 alpha beta, 1 빈 줄(기록), 2 gamma, 3 delta, 4 빈 줄(화면)
        assert_eq!(v.history_rows(), 2);
        assert_eq!(v.copy_motion((2, 3), PreviousParagraph), (1, 3));
        assert_eq!(v.copy_motion((0, 3), NextParagraph), (1, 3));
        assert_eq!(v.copy_motion((0, 0), PreviousParagraph), (0, 0));
        assert_eq!(v.copy_motion((0, 6), NextWordStart), (2, 0));
        assert_eq!(v.copy_motion((2, 0), PreviousWordStart), (0, 6));
        assert_eq!(v.copy_motion((3, 0), NextWordStart), (3, 0));
        let f = v.copy_search("a", true, (0, 4), None, 1024);
        assert_eq!((f.total, f.current_global, f.matches[2]), (6, Some(2), ((0, 9), (0, 9))));
        assert_eq!(v.copy_search("A", true, (0, 0), None, 1024).total, 0);
        assert_eq!(v.grid_row(0)[0].t, "gamma");

        // 접혀 100줄을 넘는 낱말: 64줄 창으로 못 끝내면 넓혀 끝까지 간다
        let mut v = Vt::new(5, 4);
        let mut long = b"x ".to_vec();
        long.extend(std::iter::repeat_n(b'a', 400));
        long.extend(b" y\r\n");
        v.feed(&long);
        assert_eq!(v.copy_motion((0, 0), NextWordStart), (0, 2));
        assert_eq!(v.copy_motion((0, 2), NextWordEnd), (100, 1));
        assert_eq!(v.copy_motion((100, 1), PreviousWordStart), (0, 2));
        assert_eq!(v.copy_motion((50, 0), NextBigWordStart), (100, 3));
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

    /// 기록 편집기: 읽기 상한(`READ_MAX_LINES`)과 상관없이 기록 처음부터, 접힌 줄은 잇는다
    #[test]
    fn unwrapped_all_reads_the_whole_history() {
        let mut v = Vt::new(5, 10);
        for i in 1..=READ_MAX_LINES + 500 {
            v.feed(format!("l{i}\r\n").as_bytes());
        }
        v.feed(b"0123456789AB\r\n$ ");
        let text = v.unwrapped_all();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), READ_MAX_LINES + 502);
        assert_eq!((lines[0], lines[lines.len() - 2], lines[lines.len() - 1]), ("l1", "0123456789AB", "$"));
    }

    /// 넓은 글자가 끝 칸 하나에 들지 않아 내려간 줄도 이어 읽는다(ghostty는 그 칸을 spacer로 두고 잇는다)
    #[test]
    fn recent_unwrapped_joins_rows_split_before_a_wide_char() {
        let mut v = Vt::new(4, 5);
        v.feed("가나다라마 끝\r\n".as_bytes());
        assert_eq!(read(&mut v, ReadSource::Recent, None).text, "가나\n다라\n마 끝\n");
        assert_eq!(read(&mut v, ReadSource::RecentUnwrapped, None).text, "가나다라마 끝\n");
        // 기록과 화면에 걸쳐도
        let mut h = Vt::new(2, 5);
        h.feed("가나다라마바사\r\n$ ".as_bytes());
        assert_eq!(read(&mut h, ReadSource::RecentUnwrapped, None).text, "가나다라마바사\n$\n");
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

    /// herdr `read_terminal_snapshot` Detection: 감지 글 그대로(보이는 화면도 기록 전부도 아니다), `lines`면 마지막 줄들,
    /// ansi여도 모양 없는 글
    #[test]
    fn read_detection_is_the_detection_text() {
        let mut v = Vt::new(4, 20);
        for i in 1..=6 {
            v.feed(format!("l{i}\r\n").as_bytes());
        }
        v.feed(b"\x1b[2J\x1b[H\x1b[31ma\x1b[m\r\nb");
        let all = ReadText { text: "l2\nl3\na\nb\n".into(), truncated: false };
        assert_eq!(read(&mut v, ReadSource::Detection, None), all);
        assert_eq!(read(&mut v, ReadSource::Detection, Some(1)), ReadText { text: "b\n".into(), truncated: true });
        assert_eq!(v.read(ReadSource::Detection, None, ReadFormat::Ansi), all);
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
        // 글 안의 ESC·C1 CSI는 빠진다: 붙여넣기를 일찍 끝내지 못한다
        assert_eq!(app.paste("a\x1b[201~rm x\r\u{9b}201~b"), "\x1b[200~a[201~rm x\r201~b\x1b[201~".as_bytes());
        assert_eq!(plain.paste("a\x1bb"), b"ab");
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

    /// 보이는 줄(끝 공백 뺌)
    fn screen_lines(v: &Vt) -> Vec<String> {
        v.text("s").lines
    }

    /// 기록과 화면의 줄, 끝 빈 줄 뺌
    fn all_lines(v: &Vt) -> Vec<String> {
        let mut out = all_text(v);
        while out.last().is_some_and(String::is_empty) {
            out.pop();
        }
        out
    }

    // 폭 reflow(coordinator 10-18, t-0330 발견): vt100 `set_size`는 폭이 줄면 줄 끝을 버렸다. herdr(ghostty)처럼 다시 접는다
    #[test]
    fn shrink_then_widen_keeps_line_ends() {
        let mut v = Vt::new(5, 20);
        v.feed(b"0123456789abcdefghij\r\n$ ");
        v.resize(5, 8);
        assert_eq!(screen_lines(&v), ["01234567", "89abcdef", "ghij", "$", ""]);
        assert_eq!(v.cursor(), (3, 2, true));
        v.resize(5, 20);
        assert_eq!(screen_lines(&v), ["0123456789abcdefghij", "$", "", "", ""]);
        assert_eq!(v.cursor(), (1, 2, true));
        // 다시 접은 화면을 replay해도 같다(새로 붙는 front)
        let mut fresh = Vt::new(5, 20);
        fresh.feed(&v.replay());
        assert_eq!(screen_lines(&fresh), screen_lines(&v));
    }

    /// 휴대폰은 폭과 줄 수를 함께 줄인다: 폭을 먼저 다시 접고 줄 수를 바꾼다. 아래 빈 줄을 먼저 버리고, 그래도 커서 줄이 밖이면
    /// 위 줄을 기록으로 올린다(xterm `Buffer.resize`). 빈 줄을 높이 12로 세어 글이 화면 밖(아래)으로 밀리지 않는다
    #[test]
    fn rows_and_cols_shrink_together() {
        let mut v = Vt::new(12, 20);
        v.feed(b"$ echo 0123456789abcdefgh\r\n0123456789abcdefgh\r\n$ ");
        v.resize(10, 8);
        assert_eq!(screen_lines(&v)[..8], ["$ echo 0", "12345678", "9abcdefg", "h", "01234567", "89abcdef", "gh", "$"]);
        assert_eq!(v.cursor(), (7, 2, true));
        v.resize(5, 8);
        assert_eq!(screen_lines(&v), ["h", "01234567", "89abcdef", "gh", "$"]);
        assert_eq!(all_lines(&v)[..3], ["$ echo 0", "12345678", "9abcdefg"]);
        assert_eq!(v.cursor(), (4, 2, true));
        v.resize(12, 20);
        assert_eq!(screen_lines(&v)[..4], ["$ echo 0123456789abc", "defgh", "0123456789abcdefgh", "$"]);
        assert_eq!(v.cursor(), (3, 2, true));
    }

    /// 줄 수만 바뀌어도(휴대폰 키보드) 접힌 줄은 이어진 채라 나중에 넓히면 펴진다. vt100 `set_size`는 보이는 줄의 이음 표시를 지웠다
    #[test]
    fn rows_only_resize_keeps_soft_wraps() {
        let mut v = Vt::new(6, 10);
        v.feed(b"abcdefghijklmnopqrstuvwxy\r\n$ ");
        v.resize(4, 10);
        v.resize(6, 10);
        v.resize(6, 30);
        assert_eq!(screen_lines(&v)[..2], ["abcdefghijklmnopqrstuvwxy", "$"]);
        assert_eq!(v.cursor(), (1, 2, true));
    }

    /// alternate screen(vim·less)이 떠 있는 동안 줄 수만 바뀌어도 그 밑 normal screen의 접힌 줄은 이어진 채다(vt100 `set_size`는 두
    /// 화면의 보이는 줄 이음 표시를 다 지운다)
    #[test]
    fn rows_only_resize_under_alternate_screen_keeps_soft_wraps() {
        let mut v = Vt::new(6, 10);
        v.feed(b"abcdefghijklmnopqrstuvwxy\r\n$ \x1b[?1049h\x1b[HALT");
        v.resize(4, 10);
        v.resize(6, 10);
        assert_eq!(screen_lines(&v)[0], "ALT");
        v.feed(b"\x1b[?1049l");
        v.resize(6, 30);
        assert_eq!(screen_lines(&v)[..2], ["abcdefghijklmnopqrstuvwxy", "$"]);
    }

    /// 줄이 줄면 커서 아래 행은 버린다(xterm·vt100과 같다). 커서 줄 안에서도 커서 행 아래는 버린다
    #[test]
    fn rows_below_cursor_are_dropped_when_rows_shrink() {
        let mut v = Vt::new(6, 20);
        v.feed(b"top\r\ninput> abcdefghijk\r\nstatus bar here\x1b[2;3H");
        v.resize(2, 10);
        assert_eq!(all_lines(&v), ["top", "input> abc"]);
        assert_eq!(v.cursor(), (1, 2, true));
    }

    #[test]
    fn soft_wrapped_line_unwraps_when_widened() {
        let mut v = Vt::new(4, 10);
        v.feed(b"abcdefghijklmnopqrstuvwxy\r\n");
        v.resize(4, 30);
        assert_eq!(screen_lines(&v), ["abcdefghijklmnopqrstuvwxy", "", "", ""]);
    }

    #[test]
    fn history_rewraps_too() {
        let mut v = Vt::new(3, 12);
        for i in 0..10 {
            v.feed(format!("line-{i}-abcdefg\r\n").as_bytes());
        }
        let before = all_lines(&v);
        v.resize(3, 6);
        v.resize(3, 12);
        assert_eq!(all_lines(&v), before);
        v.resize(3, 40);
        let wide: Vec<String> = (0..10).map(|i| format!("line-{i}-abcdefg")).collect();
        assert_eq!(all_lines(&v), wide);
    }

    #[test]
    fn colors_survive_reflow() {
        let mut v = Vt::new(3, 10);
        v.feed(b"\x1b[31mred\x1b[0m plain \x1b[1;44mbold-on-blue\x1b[0m\r\n");
        v.resize(3, 5);
        v.resize(3, 40);
        assert_eq!(screen_lines(&v)[0], "red plain bold-on-blue");
        let s = v.parser.screen();
        assert_eq!(s.cell(0, 0).unwrap().fgcolor(), vt100::Color::Idx(1));
        assert_eq!(s.cell(0, 4).unwrap().fgcolor(), vt100::Color::Default);
        let b = s.cell(0, 10).unwrap();
        assert!(b.bold());
        assert_eq!(b.bgcolor(), vt100::Color::Idx(4));
    }

    #[test]
    fn wide_chars_rejoin() {
        let mut v = Vt::new(4, 20);
        v.feed("가나다라마바사 끝\r\n".as_bytes());
        // 5칸이면 넓은 글자 둘 뒤 끝 칸 하나가 남아 셋째 글자는 이음 표시 없이 다음 줄로 간다(vt100 `col_wrap`)
        v.resize(4, 5);
        assert_eq!(all_lines(&v), ["가나", "다라", "마바", "사 끝"]);
        v.resize(4, 20);
        assert_eq!(screen_lines(&v)[0], "가나다라마바사 끝");
    }

    #[test]
    fn prompt_after_clear_stays_on_top() {
        let mut v = Vt::new(5, 20);
        for i in 0..30 {
            v.feed(format!("old-{i:02}-xxxxxxxxxxxx\r\n").as_bytes());
        }
        v.feed(b"\x1b[H\x1b[2J$ ");
        v.resize(5, 10);
        assert_eq!(screen_lines(&v), ["$", "", "", "", ""]);
        assert_eq!(v.cursor(), (0, 2, true));
    }

    #[test]
    fn cursor_in_the_middle_of_a_line() {
        let mut v = Vt::new(4, 20);
        v.feed(b"abcdefghij");
        v.resize(4, 4);
        assert_eq!(screen_lines(&v), ["abcd", "efgh", "ij", ""]);
        assert_eq!(v.cursor(), (2, 2, true));
        v.feed(b"K");
        v.resize(4, 20);
        assert_eq!(screen_lines(&v)[0], "abcdefghijK");
    }

    #[test]
    fn content_below_cursor_stays_below() {
        let mut v = Vt::new(6, 20);
        v.feed(b"top\r\ninput> abcdefghijk\r\nstatus bar here\x1b[2A\x1b[8G\x1b[B");
        assert_eq!(v.cursor(), (1, 7, true));
        v.resize(6, 10);
        v.feed(b"Z");
        v.resize(6, 20);
        assert_eq!(screen_lines(&v)[..3], ["top", "input> Zbcdefghijk", "status bar here"]);
    }

    #[test]
    fn alternate_screen_keeps_normal_underneath() {
        let mut v = Vt::new(4, 20);
        v.feed(b"0123456789abcdefghij\r\n$ ");
        v.feed(b"\x1b[?1049h\x1b[HALT");
        v.resize(4, 8);
        assert!(v.parser.screen().alternate_screen());
        assert_eq!(screen_lines(&v)[0], "ALT");
        assert_eq!(v.cursor(), (0, 3, true));
        v.resize(4, 20);
        assert_eq!(v.cursor(), (0, 3, true));
        v.feed(b"\x1b[?1049l");
        assert_eq!(screen_lines(&v)[..2], ["0123456789abcdefghij", "$"]);
        assert_eq!(v.cursor(), (1, 2, true));
    }

    #[test]
    fn input_modes_survive_reflow() {
        let mut v = Vt::new(4, 20);
        v.feed(b"0123456789abcdefghij\r\n\x1b[?2004h\x1b[?1h\x1b[?1000h\x1b[?1006h\x1b[>4;2m\x1b[?25l");
        v.resize(4, 8);
        let s = v.parser.screen();
        assert!(s.bracketed_paste());
        assert!(s.application_cursor());
        assert!(s.hide_cursor());
        assert_eq!(s.mouse_protocol_mode(), vt100::MouseProtocolMode::PressRelease);
        assert_eq!(s.mouse_protocol_encoding(), vt100::MouseProtocolEncoding::Sgr);
        assert_eq!(v.modes().modify_other_keys, 2);
    }

    #[test]
    fn blank_lines_between_stay() {
        let mut v = Vt::new(6, 20);
        v.feed(b"one\r\n\r\n\r\nfour-abcdefghijklmnop\r\n$ ");
        v.resize(6, 10);
        v.resize(6, 20);
        // 넓히면 기록에 갔던 "one"이 다시 화면에 내려온다(21자 줄은 20칸에서 두 줄)
        assert_eq!(screen_lines(&v), ["one", "", "", "four-abcdefghijklmno", "p", "$"]);
        assert_eq!(v.cursor(), (5, 2, true));
    }

    #[test]
    fn no_change_when_nothing_wraps() {
        let mut v = Vt::new(4, 20);
        v.feed(b"short\r\n$ ");
        v.resize(4, 10);
        assert_eq!(screen_lines(&v)[..2], ["short", "$"]);
        assert_eq!(v.cursor(), (1, 2, true));
    }

    /// 기록 replay는 접힌 줄을 줄바꿈 없이 이어 써서, 받은 터미널도 이음 표시를 갖고 폭이 바뀌면 다시 접는다(xterm도 같다)
    #[test]
    fn replayed_history_keeps_soft_wraps() {
        let mut a = Vt::new(3, 10);
        let lines: Vec<String> =
            (0..6).map(|i| format!("w{i}-abcdefghijklmnopq")).chain(["가나다라마바사아자".to_string()]).collect();
        for l in &lines {
            a.feed(format!("{l}\r\n").as_bytes());
        }
        // 화면 경계에 걸친 줄이 없게 빈 줄을 화면만큼
        a.feed(b"\r\n\r\n");
        let mut b = Vt::new(3, 10);
        b.feed(&a.replay());
        assert_eq!(all_lines(&b), all_lines(&a));
        b.resize(3, 40);
        assert_eq!(all_lines(&b), lines);
        // 위로 굴려 받는 앞 기록도 같다: 셋째 행(w1 첫 행) 앞은 20자 줄 w0의 두 행
        let anchor = vec![all_text(&a)[2].clone()];
        let o = a.older(&anchor, all_text(&a).len() - 2, 10).unwrap();
        assert_eq!(o.lines, 2);
        let mut c = Vt::new(3, 10);
        c.feed(&o.data);
        c.resize(3, 40);
        assert_eq!(all_lines(&c), [lines[0].clone()]);
    }

    /// 보이는 줄 r의 묶음 칸: (시작 칸, 폭, 글). 빈칸 글자 칸은 빈 칸으로 본다(replay는 줄 가운데 빈 칸을 빈칸으로 쓴다)
    fn clusters(s: &vt100::Screen, r: u16) -> Vec<(u16, u16, String)> {
        let (_, cols) = s.size();
        (0..cols)
            .filter_map(|c| {
                let cell = s.cell(r, c).filter(|x| x.has_contents() && x.contents() != " ")?;
                Some((c, if cell.is_wide() { 2 } else { 1 }, cell.contents().to_string()))
            })
            .collect()
    }

    fn cl(want: &[(u16, u16, &str)]) -> Vec<(u16, u16, String)> {
        want.iter().map(|&(c, w, s)| (c, w, s.to_string())).collect()
    }

    /// 줄 하나의 묶음 칸과 이음 표시
    type Clustered = (Vec<(u16, u16, String)>, bool);

    /// 기록과 화면의 줄마다(오래된 줄부터) 묶음 칸과 이음 표시
    fn all_clusters(v: &Vt) -> Vec<Clustered> {
        let mut s = v.parser.screen().clone();
        let mut out = Vec::new();
        each_row(&mut s, |s, r| {
            out.push((clusters(s, r), s.row_wrapped(r)));
            true
        });
        out
    }

    /// 글자 폭과 묶음은 웹 터미널(xterm + grapheme addon + herdr 보정)과 같다(plan §8.167). 사례의 칸은 생성기가 웹
    /// provider로 한 줄에 써서 만든다(web/scripts/vt-widths.ts). 한 byte씩 받아도 같다(xterm도 받은 덩이 경계에서 끊지 않는다)
    #[test]
    fn widths_follow_the_web_terminal() {
        for (text, want) in width_cases::CASES {
            let want = cl(want);
            let end = want.last().map_or(0, |&(c, w, _)| c + w);
            let mut v = Vt::new(3, 40);
            v.feed(text.as_bytes());
            assert_eq!(clusters(v.parser.screen(), 0), want, "{text:?}");
            assert_eq!(v.cursor(), (0, end, true), "{text:?}");
            let mut bytes = Vt::new(3, 40);
            for b in text.as_bytes() {
                bytes.feed(&[*b]);
            }
            assert_eq!(clusters(bytes.parser.screen(), 0), want, "한 byte씩 {text:?}");
        }
    }

    /// 끝 칸의 한 칸 묶음이 붙은 글자로 두 칸이 되면(⚠ 뒤 VS16) xterm처럼 그 묶음을 이은 줄로 다음 줄 처음에 옮기고 끝 칸은 비운다
    #[test]
    fn widened_cluster_at_the_last_column_moves_to_the_next_row() {
        let mut v = Vt::new(3, 5);
        v.feed("abcd\u{26a0}\u{fe0f}x".as_bytes());
        let s = v.parser.screen();
        assert_eq!(clusters(s, 0), cl(&[(0, 1, "a"), (1, 1, "b"), (2, 1, "c"), (3, 1, "d")]));
        assert!(s.row_wrapped(0));
        assert_eq!(clusters(s, 1), cl(&[(0, 2, "\u{26a0}\u{fe0f}"), (2, 1, "x")]));
        assert_eq!(v.cursor(), (1, 3, true));
    }

    /// replay를 받은 터미널의 칸은 살아 있는 칸과 같다. escape로 따로 놓인 칸은 모양이 같아도 이어 쓰면 받는 쪽에서 한 묶음이
    /// 되므로 끊어 쓴다: 기록·화면·alternate screen, 접힌 줄을 넘어서도
    #[test]
    fn replay_keeps_clusters_as_they_were_placed() {
        let lines = [
            // 모양을 바꾸지 않는 SGR로 끊긴 결합 문자, 같은 색을 다시 건 SGR
            "e\x1b[m\u{301}x",
            "\x1b[31me\x1b[31m\u{301}x\x1b[m",
            // 국기 반쪽 둘, VS16 앞 escape
            "\u{1f1f0}\x1b[m\u{1f1f7}",
            "\u{26a0}\x1b[m\u{fe0f}",
            // 커서를 옮긴 빈 칸 뒤 결합 문자(빈 칸은 빈칸 글자로 쓴다)
            "a\x1b[2C\u{301}b",
            // 끊기지 않은 묶음은 그대로 한 칸이다
            "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}\u{26a0}\u{fe0f}e\u{301}",
        ];
        let text: String = lines.iter().map(|l| format!("{l}\r\n")).collect();
        let check = |rows: u16, cols: u16, alt: bool, input: &str| {
            let mut a = Vt::new(rows, cols);
            if alt {
                a.feed(b"\x1b[?1049h");
            }
            a.feed(input.as_bytes());
            let mut b = Vt::new(rows, cols);
            b.feed(&a.replay());
            assert_eq!(all_clusters(&b), all_clusters(&a), "alt {alt} {input:?}");
            assert_eq!(b.cursor(), a.cursor(), "alt {alt} {input:?}");
        };
        // 셋째 줄 뒤로는 기록으로 밀린다
        check(3, 12, false, &text);
        check(8, 12, true, &text);
        // 끝 칸 뒤 escape로 끊겨 다음 줄로 접힌 결합 문자
        for alt in [false, true] {
            check(4, 5, alt, "abcde\x1b[m\u{301}");
        }
    }

    /// 폭이 바뀌어 다시 접어도 묶음과 따로 놓인 칸이 그대로다(다시 쓰는 줄도 끊어 쓴다)
    #[test]
    fn reflow_keeps_clusters_as_they_were_placed() {
        let mut v = Vt::new(3, 12);
        v.feed("a\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}e\x1b[m\u{301}\u{1f1f0}\x1b[m\u{1f1f7}x\r\n".as_bytes());
        let before = all_clusters(&v);
        v.resize(3, 4);
        assert_eq!(
            all_clusters(&v)[..2],
            [
                (cl(&[(0, 1, "a"), (1, 2, "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}"), (3, 1, "e")]), true),
                (cl(&[(0, 1, "\u{301}"), (1, 1, "\u{1f1f0}"), (2, 1, "\u{1f1f7}"), (3, 1, "x")]), false),
            ]
        );
        v.resize(3, 12);
        assert_eq!(all_clusters(&v), before);
    }
}
