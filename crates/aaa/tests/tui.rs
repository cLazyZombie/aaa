//! TUI harness E2E: PTY 안에서 `aaa attach`를 띄우고, 나온 bytes를 독립 parser(vte)로 화면에 옮겨 단언한다.
//! production VT(vt100)와 다른 구현이라 TUI가 그린 것을 따로 관측한다.
//! 실행: `cargo test -p aaa --test tui`
//!
//! 기다림은 시간이 아니라 관측한 사건으로 끝낸다: 화면은 bytes가 들어올 때마다 알리고(Condvar), host 상태는
//! Front 연결의 사건(Workspace·State)으로 받는다. 시간 한도는 멈췄을 때 실패로 끝내는 안전망일 뿐이다.

#![cfg(unix)]

mod lab;

use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use aaa_proto::{FrontEvent, FrontMsg, FrontRequest, FrontResponse, PaneView, Workspace};
use lab::{BIN, Lab, Node, wait_for};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use unicode_width::UnicodeWidthChar;

const T: Duration = Duration::from_secs(20);
/// 기본 시험 터미널 폭(목록 시험은 WIDE도 쓴다)
const COLS: u16 = 90;
const WIDE: u16 = 130;
const ROWS: u16 = 30;
/// 펼친 사이드바 폭(구분선 포함, 시험 설정 `sidebar_width = 26`: herdr 기본. aaa 기본 36은 `sidebar_width_like_herdr`)과
/// 접힌 compact 폭(4)
const SIDE: u16 = 26;
/// 보통 화면 pane 오른쪽 scrollbar 칸(`[tui] pane_scrollbars` 기본 켬, herdr `stable_scrollbar_gutter`): PTY가 그만큼 좁다
const GUTTER: u16 = 1;
const COMPACT: u16 = 4;
/// herdr catppuccin 팔레트의 탐색 커서 줄 바탕(`selection_bg`)과 지금 pane 줄 바탕(`active_row_bg`)
const SELECTION_BG: u32 = 0x0200_0000 | (49 << 16) | (50 << 8) | 68;
const ACTIVE_BG: u32 = 0x0200_0000 | (30 << 16) | (30 << 8) | 46;
/// copy 모드 찾기: 현재 일치(accent)와 나머지 일치(`surface1`) 바탕
const ACCENT_BG: u32 = 0x0200_0000 | (137 << 16) | (180 << 8) | 250;
const SURFACE1_BG: u32 = 0x0200_0000 | (69 << 16) | (71 << 8) | 90;

/// 시험 설정: 사용자 입력기는 건드리지 않고(입력기 전환은 기본 켬이라 끈다), prefix 뒤 자모 해석은 켠다(기본값).
/// 사이드바는 herdr 기본 폭 26칸으로 그린다(시험 화면 배치가 그 폭을 기준으로 한다)
const LOCAL_ONLY: &str = r#"schema = 1
[host]
agent_listen = ""
[web]
listen = ""
[local_agent]
mode = "none"
[tui]
switch_ascii_input_source_in_prefix = false
prefix_jamo_keys = true
sidebar_width = 26
"#;

/// 화면 관측기: 출력·커서 이동·지우기와 바탕색(SGR 40~49·100~107·48;5·48;2)만 다룬다.
/// 바탕색은 herdr처럼 색으로만 보이는 사이드바의 고른 줄·지금 줄을 찾는 데 쓴다.
struct Screen {
    /// 켜 둔 DEC 개인 모드(`CSI ? n h`): 마우스 보고(1000·1002·1003·1006) 등
    modes: std::collections::BTreeSet<u16>,
    cells: Vec<Vec<char>>,
    /// 칸마다 바탕색(0 기본, 0x01_000000|n 256색, 0x02_rrggbb RGB)
    bgs: Vec<Vec<u32>>,
    bg: u32,
    row: usize,
    col: usize,
    cols: usize,
    /// alt 화면(`CSI ? 1049 h`)에 든 동안 둔 보통 화면(글·바탕·커서). 실제 터미널처럼 alt 화면은 비어서 시작하고, 나오면
    /// 보통 화면이 돌아온다: TUI 앞에 찍힌 글(원격 pairing 물음)이 TUI 화면에 섞이지 않는다
    main: Option<Saved>,
}

/// alt 화면에 든 동안 둔 보통 화면
struct Saved {
    cells: Vec<Vec<char>>,
    bgs: Vec<Vec<u32>>,
    row: usize,
    col: usize,
}

impl Screen {
    fn new(cols: u16) -> Screen {
        Screen {
            modes: Default::default(),
            cells: vec![vec![' '; cols as usize]; ROWS as usize],
            bgs: vec![vec![0; cols as usize]; ROWS as usize],
            bg: 0,
            row: 0,
            col: 0,
            cols: cols as usize,
            main: None,
        }
    }

    fn text(&self) -> String {
        self.cells.iter().map(|l| l.iter().filter(|c| **c != '\0').collect::<String>()).collect::<Vec<_>>().join("\n")
    }

    fn clear_line(&mut self, row: usize, from: usize, to: usize) {
        if let Some(l) = self.cells.get_mut(row) {
            for c in l.iter_mut().take(to).skip(from) {
                *c = ' ';
            }
        }
        let bg = self.bg;
        if let Some(l) = self.bgs.get_mut(row) {
            for c in l.iter_mut().take(to).skip(from) {
                *c = bg;
            }
        }
    }

    /// 사이드바 칸(왼쪽 SIDE칸) 중 `from` 줄부터 바탕이 `bg`인 첫 줄: (줄 번호, 그 줄의 사이드바 글)
    fn side_row_from(&self, from: usize, bg: u32) -> Option<(usize, String)> {
        let w = (SIDE as usize).min(self.cols);
        (from..self.cells.len())
            .find(|r| self.bgs[*r][..w - 1].contains(&bg))
            .map(|r| (r, self.cells[r][..w].iter().filter(|c| **c != '\0').collect()))
    }

    fn side_row(&self, bg: u32) -> Option<(usize, String)> {
        self.side_row_from(0, bg)
    }

    /// 사이드바 ` agents` 머리 줄(herdr 아래 구역, 없으면 맨 위부터 본다)
    fn agents_top(&self) -> usize {
        self.side_lines().iter().position(|l| l.starts_with(" agents")).map_or(0, |i| i + 1)
    }

    /// 탐색 모드에서 고른 줄(herdr `selection_bg`)
    fn selected(&self) -> Option<(usize, String)> {
        self.side_row(SELECTION_BG)
    }

    /// 사이드바 칸만 줄마다(구분선과 오른쪽 pane 글은 뺀다)
    fn side_lines(&self) -> Vec<String> {
        let w = (SIDE as usize - 1).min(self.cols);
        self.cells
            .iter()
            .map(|l| l[..w].iter().filter(|c| **c != '\0').collect::<String>().trim_end().to_string())
            .collect()
    }

    /// agents 구역의 지금 pane 줄(herdr `active_row_bg`)
    fn current(&self) -> Option<String> {
        self.side_row_from(self.agents_top(), ACTIVE_BG).map(|(_, l)| l)
    }

    /// spaces 구역의 지금 workspace 줄(같은 `active_row_bg`, agents 머리 위)
    fn current_space(&self) -> Option<String> {
        self.side_row(ACTIVE_BG).filter(|(r, _)| *r < self.agents_top()).map(|(_, l)| l)
    }

    /// 글이 처음 보이는 칸(줄, 열). 넓은 글자는 두 칸으로 센다(마우스로 누를 자리).
    fn find(&self, needle: &str) -> Option<(usize, usize)> {
        self.cells.iter().enumerate().find_map(|(r, line)| {
            let (text, cols): (String, Vec<usize>) =
                line.iter().enumerate().filter(|(_, c)| **c != '\0').map(|(i, c)| (*c, i)).unzip();
            text.find(needle).map(|at| (r, cols[text[..at].chars().count()]))
        })
    }

    fn sgr(&mut self, params: &vte::Params) {
        let p: Vec<u16> = params.iter().map(|x| x[0]).collect();
        let mut i = 0;
        while i < p.len().max(1) {
            match p.get(i).copied().unwrap_or(0) {
                0 | 49 => self.bg = 0,
                n @ 40..=47 => self.bg = 0x0100_0000 | u32::from(n - 40),
                n @ 100..=107 => self.bg = 0x0100_0000 | u32::from(n - 100 + 8),
                48 if p.get(i + 1) == Some(&5) => {
                    self.bg = 0x0100_0000 | u32::from(p.get(i + 2).copied().unwrap_or(0));
                    i += 2;
                }
                48 if p.get(i + 1) == Some(&2) => {
                    let c = |k: usize| u32::from(p.get(i + k).copied().unwrap_or(0));
                    self.bg = 0x0200_0000 | (c(2) << 16) | (c(3) << 8) | c(4);
                    i += 4;
                }
                38 if matches!(p.get(i + 1), Some(&5)) => i += 2,
                38 if matches!(p.get(i + 1), Some(&2)) => i += 4,
                _ => {}
            }
            i += 1;
        }
    }
}
impl vte::Perform for Screen {
    fn print(&mut self, c: char) {
        let w = c.width().unwrap_or(0);
        if w == 0 {
            return;
        }
        if self.col + w > self.cols {
            self.col = 0;
            self.row = (self.row + 1).min(ROWS as usize - 1);
        }
        let (r, col) = (self.row, self.col);
        // 넓은 글자의 반쪽을 덮으면 나머지 반쪽은 빈칸이 된다(실제 터미널과 같다)
        let line = &mut self.cells[r];
        if line[col] == '\0' && col > 0 {
            line[col - 1] = ' ';
        }
        let end = col + w;
        if end < line.len() && line[end] == '\0' {
            line[end] = ' ';
        }
        line[col] = c;
        self.bgs[r][col] = self.bg;
        if w == 2 && col + 1 < self.cols {
            // 넓은 글자의 둘째 칸은 text()에서 뺀다
            self.cells[r][col + 1] = '\0';
            self.bgs[r][col + 1] = self.bg;
        }
        self.col += w;
    }

    fn execute(&mut self, b: u8) {
        match b {
            b'\r' => self.col = 0,
            b'\n' => self.row = (self.row + 1).min(ROWS as usize - 1),
            0x08 => self.col = self.col.saturating_sub(1),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, inter: &[u8], _ignore: bool, action: char) {
        if inter == b"?" && matches!(action, 'h' | 'l') {
            for m in params.iter().map(|x| x[0]) {
                if action == 'h' {
                    self.modes.insert(m);
                } else {
                    self.modes.remove(&m);
                }
                if m == 1049 && action == 'h' && self.main.is_none() {
                    let blank = (vec![vec![' '; self.cols]; ROWS as usize], vec![vec![0; self.cols]; ROWS as usize]);
                    let cells = std::mem::replace(&mut self.cells, blank.0);
                    let bgs = std::mem::replace(&mut self.bgs, blank.1);
                    self.main = Some(Saved { cells, bgs, row: self.row, col: self.col });
                } else if m == 1049
                    && action == 'l'
                    && let Some(Saved { cells, bgs, row, col }) = self.main.take()
                {
                    (self.cells, self.bgs, self.row, self.col) = (cells, bgs, row, col);
                }
            }
            return;
        }
        if !inter.is_empty() {
            return;
        }
        if action == 'm' {
            self.sgr(params);
            return;
        }
        let p: Vec<u16> = params.iter().map(|x| x[0]).collect();
        let n = |i: usize| p.get(i).copied().filter(|v| *v > 0).unwrap_or(1) as usize;
        let (rows, cols) = (ROWS as usize, self.cols);
        match action {
            'H' | 'f' => {
                self.row = (n(0) - 1).min(rows - 1);
                self.col = (n(1) - 1).min(cols - 1);
            }
            'A' => self.row = self.row.saturating_sub(n(0)),
            'B' => self.row = (self.row + n(0)).min(rows - 1),
            'C' => self.col = (self.col + n(0)).min(cols - 1),
            'D' => self.col = self.col.saturating_sub(n(0)),
            'G' => self.col = (n(0) - 1).min(cols - 1),
            'd' => self.row = (n(0) - 1).min(rows - 1),
            'J' => {
                let mode = p.first().copied().unwrap_or(0);
                let (from, to) = match mode {
                    0 => (self.row + 1, rows),
                    1 => (0, self.row),
                    _ => (0, rows),
                };
                for r in from..to {
                    self.clear_line(r, 0, cols);
                }
                match mode {
                    0 => self.clear_line(self.row, self.col, cols),
                    1 => self.clear_line(self.row, 0, self.col + 1),
                    _ => {}
                }
            }
            'K' => match p.first().copied().unwrap_or(0) {
                0 => self.clear_line(self.row, self.col, cols),
                1 => self.clear_line(self.row, 0, self.col + 1),
                _ => self.clear_line(self.row, 0, cols),
            },
            'X' => self.clear_line(self.row, self.col, self.col + n(0)),
            _ => {}
        }
    }
}

/// PTY 안에서 도는 `aaa attach`
struct Tui {
    /// 화면과, bytes가 들어와 화면이 바뀔 때마다 깨우는 Condvar
    screen: Arc<(Mutex<Screen>, Condvar)>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Tui {
    fn start(node: &Node) -> Tui {
        Self::start_env(node, &[])
    }

    fn start_env(node: &Node, env: &[(&str, &str)]) -> Tui {
        Self::start_cols(node, env, COLS)
    }

    fn start_cols(node: &Node, env: &[(&str, &str)], cols: u16) -> Tui {
        Self::start_args(node, &["attach"], env, cols)
    }

    /// 명령줄을 정해 띄운다(`attach --remote <주소>` 등)
    fn start_args(node: &Node, args: &[&str], env: &[(&str, &str)], cols: u16) -> Tui {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows: ROWS, cols, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = CommandBuilder::new(BIN);
        cmd.args(args);
        cmd.env("AAA_CONFIG_DIR", &node.cfg);
        cmd.env("AAA_STATE_DIR", &node.state);
        cmd.env("TERM", "xterm-256color");
        cmd.env("RUST_LOG", "off");
        // 이 관측기가 터미널이고 색(고른 줄·지금 줄 바탕)을 본다. 시험을 돌린 셸의 NO_COLOR를 물려받으면
        // crossterm이 색을 내지 않아 그 줄을 찾지 못한다(`Node::cmd`처럼 뺀다)
        cmd.env_remove("NO_COLOR");
        // 시험을 aaa pane 안에서 돌려도 그 세션을 물려받지 않는다(사람만 하는 일의 문 `person::require`가 본다)
        cmd.env_remove("AAA_SESSION_ID");
        cmd.env_remove("AAA_HOOK_SOCK");
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let screen = Arc::new((Mutex::new(Screen::new(cols)), Condvar::new()));
        let s = screen.clone();
        std::thread::spawn(move || {
            let mut parser = vte::Parser::new();
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                parser.advance(&mut *s.0.lock().unwrap(), &buf[..n]);
                s.1.notify_all();
            }
        });
        Tui { screen, writer, child, _master: pair.master }
    }

    fn text(&self) -> String {
        self.screen.0.lock().unwrap().text()
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    /// prefix(ctrl+b) 다음 키. PREFIX 막대가 보인 뒤(TUI가 prefix를 받은 뒤) 키를 보낸다.
    fn cmd(&mut self, key: &str) {
        self.send(b"\x02");
        self.wait_text("PREFIX 막대", |s| s.contains(" PREFIX "));
        self.send(key.as_bytes());
    }

    fn type_line(&mut self, s: &str) {
        self.send(s.as_bytes());
        self.send(b"\r");
    }

    /// 화면이 조건을 만족할 때까지: 화면이 바뀔 때마다 다시 본다
    fn wait_text(&self, what: &str, f: impl Fn(&str) -> bool) {
        self.wait_screen(what, |s| f(&s.text()));
    }

    fn wait_screen(&self, what: &str, f: impl Fn(&Screen) -> bool) {
        let (lock, cv) = &*self.screen;
        let deadline = Instant::now() + T;
        let mut g = lock.lock().unwrap();
        loop {
            if f(&g) {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "{T:?} 안에 {what}이 되지 않았다\nTUI 화면:\n{}", g.text());
            g = cv.wait_timeout(g, left).unwrap().0;
        }
    }

    fn selected(&self) -> Option<(usize, String)> {
        self.screen.0.lock().unwrap().selected()
    }

    /// 화면에서 글을 찾을 때까지 기다려 그 칸(줄, 열)
    fn wait_find(&self, needle: &str) -> (usize, usize) {
        self.wait_screen(needle, |s| s.find(needle).is_some());
        self.screen.0.lock().unwrap().find(needle).unwrap()
    }

    /// SGR 마우스(`CSI < b ; x ; y M`, 뗌은 `m`, 칸은 1부터). b: 0 왼쪽, 32 끌기, 64·65 휠 위·아래
    fn mouse(&mut self, b: u16, (row, col): (usize, usize), up: bool) {
        self.send(format!("\x1b[<{b};{};{}{}", col + 1, row + 1, if up { 'm' } else { 'M' }).as_bytes());
    }

    fn click(&mut self, at: (usize, usize)) {
        self.mouse(0, at, false);
        self.mouse(0, at, true);
    }

    fn wait_exit(&mut self) -> bool {
        wait_for("TUI 종료", T, || self.child.try_wait().unwrap()).success()
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// host 상태를 사건으로 본다: Front 연결 하나가 받는 Workspace·State 사건을 쌓고, 조건이 될 때까지 다음 사건을 기다린다.
struct HostWatch {
    rt: tokio::runtime::Runtime,
    rx: tokio::sync::mpsc::UnboundedReceiver<FrontMsg>,
    ws: Option<Workspace>,
    panes: Vec<PaneView>,
    machines: Vec<aaa_proto::MachineView>,
    /// 받은 앱 안 알림(차례대로)
    notices: Vec<aaa_proto::NoticeView>,
}

impl HostWatch {
    fn new(node: &Node) -> HostWatch {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let rx = rt.block_on(async {
            let c = aaa_common::front::FrontClient::connect_as(&node.state, "test-watch").await.unwrap();
            let (mut tx, rx) = c.split();
            tx.send(FrontRequest::Workspace { name: None }).await.unwrap();
            // 보내는 쪽을 닫지 않게 task에 둔다(연결이 살아 있어야 사건이 온다)
            tokio::spawn(async move {
                let _keep = tx;
                std::future::pending::<()>().await
            });
            rx
        });
        HostWatch { rt, rx, ws: None, panes: vec![], machines: vec![], notices: vec![] }
    }

    fn apply(&mut self, m: FrontMsg) {
        match m {
            FrontMsg::Event(FrontEvent::Workspace(w)) if w.name == aaa_proto::DEFAULT_WORKSPACE => self.ws = Some(w),
            FrontMsg::Event(FrontEvent::State { machines, panes }) => {
                self.machines = machines;
                self.panes = panes;
            }
            FrontMsg::Event(FrontEvent::Notice(n)) => self.notices.push(n),
            FrontMsg::Res { result, .. } => {
                if let Ok(FrontResponse::Workspace(w)) = result.into_result() {
                    self.ws = Some(w);
                }
            }
            _ => {}
        }
    }

    /// 조건이 될 때까지 host 사건을 받는다
    fn wait<R>(&mut self, what: &str, mut f: impl FnMut(&HostWatch) -> Option<R>) -> R {
        let deadline = Instant::now() + T;
        loop {
            if let Some(v) = f(self) {
                return v;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            // 시간 한도 타이머는 runtime 안에서 만든다
            let rx = &mut self.rx;
            let m = self.rt.block_on(async move { tokio::time::timeout(left, rx.recv()).await });
            match m {
                Ok(Some(m)) => self.apply(m),
                Ok(None) => panic!("host 연결이 끊겼다({what})"),
                Err(_) => panic!(
                    "{T:?} 안에 {what}이 되지 않았다\nworkspace: {:?}\npanes: {:?}",
                    self.ws,
                    self.panes.iter().map(|p| (&p.id, &p.name, &p.state, &p.lease, p.cols, p.rows)).collect::<Vec<_>>()
                ),
            }
        }
    }

    /// 모든 탭의 pane(layout 차례)
    fn leaves(&self) -> Vec<String> {
        self.ws.iter().flat_map(|w| w.tabs.iter().flat_map(|t| aaa_proto::layout::leaves(&t.root))).collect()
    }

    fn pane(&self, id: &str) -> Option<&PaneView> {
        self.panes.iter().find(|p| p.id == id)
    }

    /// 그 pane이 든 탭의 pane들(layout 차례)
    fn tab_of(&self, id: &str) -> Option<Vec<String>> {
        let ws = self.ws.as_ref()?;
        ws.tabs.iter().map(|t| aaa_proto::layout::leaves(&t.root)).find(|l| l.iter().any(|p| p == id))
    }

    fn connected(&self, name: &str) -> bool {
        self.machines.iter().any(|m| m.name == name && m.connection == aaa_proto::Connection::Connected)
    }

    /// 그 pane의 lease 이름이 `label`로 시작하나
    fn held_by(&self, id: &str, label: &str) -> bool {
        self.pane(id).and_then(|p| p.lease.as_ref()).is_some_and(|l| l.label.starts_with(label))
    }
}

/// host와 agent(labA)를 띄우고, 사건을 볼 연결을 준다
fn start(lab: &mut Lab) -> (Node, HostWatch) {
    start_with(lab, LOCAL_ONLY)
}

fn start_with(lab: &mut Lab, host_toml: &str) -> (Node, HostWatch) {
    start_agent_env(lab, host_toml, &[])
}

/// `start_with`에 agent 환경 변수를 더한다(agent가 띄우는 pane이 물려받는다)
fn start_agent_env(lab: &mut Lab, host_toml: &str, env: &[(&str, &str)]) -> (Node, HostWatch) {
    let h = lab.node("h", Some(host_toml));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    let mut hw = HostWatch::new(&h);
    lab.spawn_env("agent", &h, &["agent", "--host", "local", "--name", "labA"], env);
    hw.wait("labA 연결", |w| w.connected("labA").then_some(()));
    (h, hw)
}

/// 탭 막대의 이름표들(herdr: 붙인 이름 또는 자리 번호). 맨 윗줄에서 사이드바 구분선 뒤, 오른쪽 조각(확인 필요 수·다른 화면 크기) 앞.
/// 시험의 탭 이름에는 빈칸을 쓰지 않는다.
fn tab_chips(text: &str) -> Vec<String> {
    let first = text.lines().next().unwrap_or("");
    let rest = first.split_once('│').map_or("", |(_, r)| r);
    let rest = rest.split(" ● 확인 필요").next().unwrap_or(rest);
    // 알림 카드(`render_notice`: 오른쪽 위 3줄)가 덮은 오른쪽도 이름표가 아니다
    let rest = rest.split('┌').next().unwrap_or(rest);
    // 마우스 단추(` + `, 넘칠 때 ` < ` ` > `)와 잘린 표시 `…`는 이름표가 아니다
    let mut chips: Vec<String> =
        rest.split_whitespace().filter(|c| !matches!(*c, "+" | "<" | ">" | "…")).map(str::to_string).collect();
    if chips.last().is_some_and(|c| c == "크기") {
        chips.truncate(chips.len().saturating_sub(2));
    }
    chips
}

/// 줄에서 사이드바(첫 `│`까지)를 뺀 pane 쪽
fn panes_part(line: &str) -> &str {
    line.split_once('│').map_or(line, |(_, r)| r)
}

fn leaves(w: &serde_json::Value) -> Vec<String> {
    fn walk(n: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(p) = n["pane"].as_str() {
            out.push(p.into());
        } else {
            walk(&n["a"], out);
            walk(&n["b"], out);
        }
    }
    let mut out = vec![];
    for t in w["tabs"].as_array().unwrap() {
        walk(&t["root"], &mut out);
    }
    out
}

/// layout 그리기·나누기·입력·탭 전환·나갔다 다시 붙기
#[test]
fn attach_draws_layout_and_detach_keeps_sessions() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    // 실행 파일 이름이 claude인 셸: agent가 이 pane 화면을 Claude 규칙(herdr claude.toml)으로 본다
    let claude = lab.root.join("bin").join("claude");
    std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
    std::fs::write(&claude, "#!/bin/sh\nexec /bin/sh\n").unwrap();
    std::fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "first", "--", lab::path_str(&claude)]);
    let first = leaves(&w)[0].clone();

    let mut tui = Tui::start(&h);
    // 탭 이름표는 herdr처럼 붙인 이름 또는 자리 번호다. 사이드바에 「기타」 묶음과 pane 줄(머신 이름)이 있고 지금 pane 줄은 active_row_bg
    // 바탕이다(herdr 사이드바). 이 pane은 실행 파일 이름이 claude라 agent가 띄운 지 3초(`STARTUP_GRACE`) 뒤 화면을 보고
    // 쉼을 알린다(`○`·「입력 대기」). 상태 없는 `·`은 그 3초 동안만 보여 TUI가 3초 넘게 늦게 뜨면 볼 수 없다.
    // 시작 시각에 기대지 않게 감지가 알린 쉼을 기다린다.
    tui.wait_screen("탭과 사이드바", |s| {
        let t = s.text();
        tab_chips(&t) == ["first"]
            && t.contains(" ▾ 기타")
            && s.current().is_some_and(|l| l.contains(" ○ first labA") && l.contains("입력 대기"))
    });
    // 처음 포커스한 pane은 아무도 쓰지 않으니 칸 크기(사이드바 26칸과 탭 막대 한 줄을 뺀 칸)로 맞추고
    // 이 화면이 지금 쓰는 화면이 된다. 상태 줄은 없다(herdr).
    hw.wait("칸 크기와 lease", |w| {
        let p = w.pane(&first)?;
        ((p.cols, p.rows) == (COLS - SIDE - GUTTER, ROWS - 1) && w.held_by(&first, "tui@")).then_some(())
    });

    tui.type_line("echo tui-$((6*7))");
    tui.wait_text("입력 결과", |s| s.contains("tui-42"));

    // harness 상태는 화면으로 바뀌고 사이드바 줄·pane badge에 보인다: Claude 권한 확인창
    // (`bash_permission_prompt`)은 질문, spinner 줄(`live_turn_working`)은 작업 중이다. 탭 막대 오른쪽은 thread가 아닌
    // pane의 질문을 세지 않는다(herdr-projects `projects: N need you`는 thread 수다, side_list_like_web)
    tui.type_line(
        "clear; printf '%s\\n' ' Bash command' '' '   ls' '' ' Do you want to proceed?' ' ❯ 1. Yes' '   2. No' '' \
         ' Esc to cancel · Tab to amend'",
    );
    tui.wait_text("질문 상태", |s| {
        s.contains("질문 — aaa question") && s.lines().next().is_some_and(|l| !l.contains(" ● 확인 필요"))
    });
    tui.type_line("clear; printf '✻ Working… (1s · esc to interrupt)\\n'");
    tui.wait_text("작업 상태", |s| {
        s.lines().any(|l| l.contains(" ● first") && l.contains("작업 중")) && !s.contains("질문 — aaa question")
    });
    tui.type_line("echo tui-$((6*7))");
    tui.wait_text("입력 결과", |s| s.contains("tui-42"));

    // prefix v(herdr split_vertical)로 좌우로 나누면 host layout이 바뀌고 pane마다 테두리 상자와 새 pane이 그려진다.
    // 포커스는 새 pane(오른쪽)으로 간다.
    tui.cmd("v");
    let second = hw.wait("split 반영", |w| {
        let l = w.leaves();
        (l.len() == 2).then(|| l[1].clone())
    });
    tui.wait_text("테두리", |s| {
        s.lines().nth(1).is_some_and(|l| panes_part(l).starts_with('┌') && panes_part(l).contains("┐┌"))
            && s.lines().nth(5).is_some_and(|l| panes_part(l).contains("││"))
    });
    tui.type_line("echo right-$((5+5))");
    tui.wait_text("오른쪽 출력", |s| {
        s.lines().map(panes_part).any(|l| l.find("right-10").is_some_and(|i| l[..i].contains('│')))
    });
    assert!(h.screen(&second).contains("right-10"));
    assert!(!h.screen(&first).contains("right-10"), "포커스한 pane에만 들어가야 한다");

    // 새 탭과 탭 전환. herdr처럼 이름을 먼저 묻는다: 기본값(탭 수+1) 그대로 Enter면 자리 번호다.
    // 한글 입력기가 켜진 채 누른 명령 키(ㅊ = c 자리)도 같은 명령이다
    // (자모 해석은 기본 켬. 시험 설정은 입력기 전환을 꺼 사용자 입력기를 건드리지 않는다)
    tui.cmd("c");
    tui.wait_text("새 탭 이름 창", |s| s.contains("새 탭") && s.contains("↵ 저장"));
    tui.send(b"\r");
    tui.wait_text("둘째 탭", |s| tab_chips(s) == ["first", "2"]);
    tui.cmd("ㅊ");
    tui.wait_text("새 탭 이름 창(자모 ㅊ)", |s| s.contains("새 탭") && s.contains("↵ 저장"));
    tui.send("작업\r".as_bytes());
    tui.wait_text("셋째 탭(이름 작업)", |s| tab_chips(s) == ["first", "2", "작업"]);
    tui.cmd("1");
    tui.wait_text("첫 탭으로", |s| s.contains("tui-42") && s.contains("right-10"));

    // prefix q로 나가도(detach) 세션은 그대로다
    tui.cmd("q");
    assert!(tui.wait_exit(), "detach는 성공으로 끝나야 한다");
    drop(tui);
    hw.wait("lease 해제", |w| w.pane(&first).is_some_and(|p| p.lease.is_none()).then_some(()));
    assert!(hw.pane(&first).unwrap().state.is_running());
    assert_eq!(hw.leaves().len(), 4);

    // 다시 붙으면 같은 layout과 화면이 보인다
    let tui = Tui::start(&h);
    tui.wait_text("다시 붙기", |s| {
        s.contains("tui-42") && s.contains("right-10") && tab_chips(s) == ["first", "2", "작업"]
    });
    // 보고서용 화면 텍스트(`AAA_TUI_DUMP=파일`)
    if let Ok(path) = std::env::var("AAA_TUI_DUMP") {
        std::fs::write(path, tui.text()).unwrap();
    }
}

/// 다른 기기 TUI(plan §6.2, 차이 목록 4장 A 「다른 머신에서 TUI 붙기」 지금 497줄, herdr `--remote`): host 웹 주소의
/// WebSocket 위 Front API로 같은 화면·입력·lease를 얻는다. 처음에는 host 사람이 낸 pairing 코드를 묻고 기기 token을 이 기기에
/// 0600으로 두며, 다음부터는 묻지 않는다. 보기 기기는 「보기 전용」이고 입력이 pane에 가지 않는다. 기기를 취소하면 그 화면이
/// 까닭과 함께 끝나고, 다시 붙으면 token을 지우고 새 코드를 묻는다. http는 loopback만 받는다.
#[test]
fn remote_attach_rides_the_web() {
    let mut lab = Lab::new();
    let with_web = LOCAL_ONLY.replace("[web]\nlisten = \"\"", "[web]\nlisten = \"127.0.0.1:0\"");
    assert_ne!(with_web, LOCAL_ONLY);
    let (h, mut hw) = start_with(&mut lab, &with_web);
    let web = h.ok(&["status"])["web_listen"].as_str().unwrap().to_string();
    let url = format!("http://{web}");
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "far", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();
    // 다른 기기: 제 설정·상태 폴더(기기 token은 여기 둔다). 설정이 없어 사이드바는 기본 폭(36)이다
    let dev = lab.node("dev", None);
    let side = aaa_config::SIDEBAR_WIDTH;
    // 보고서용 화면 텍스트(`AAA_TUI_DUMP_REMOTE=폴더`)
    let dump = |name: &str, tui: &Tui| {
        if let Ok(dir) = std::env::var("AAA_TUI_DUMP_REMOTE") {
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}.txt")), tui.text()).unwrap();
        }
    };

    // http는 host 머신 loopback만: 다른 이름이면 연결하기 전에 거부한다(token을 평문으로 보내지 않는다)
    let out = dev.run(&["attach", "--remote", "http://mac-a.example.ts.net:8443"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("평문으로 보내지 않는다"), "{out:?}");

    // 처음: pairing 코드를 묻는다
    let code = h.ok(&["web", "pair"])["code"].as_str().unwrap().to_string();
    let mut tui = Tui::start_args(&dev, &["attach", "--remote", &url], &[], COLS);
    tui.wait_text("pairing 물음", |s| s.contains("아직 그 host와 pairing하지 않았다") && s.contains("pairing 코드:"));
    dump("pair", &tui);
    tui.type_line(&code);
    tui.wait_text("탭과 사이드바", |s| tab_chips(s) == ["far"] && s.contains(" ▾ 기타"));
    hw.wait("원격 화면의 lease와 칸 크기", |w| {
        let p = w.pane(&first)?;
        (w.held_by(&first, "tui@") && (p.cols, p.rows) == (COLS - side - GUTTER, ROWS - 1)).then_some(())
    });
    tui.type_line("echo far-$((6*7))");
    tui.wait_text("입력 결과", |s| s.contains("far-42"));
    dump("controller", &tui);
    assert!(h.screen(&first).contains("far-42"));
    let token = dev.state.join("remote").join(format!("http-{}.token", web.replace(':', "-")));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&token).unwrap().permissions().mode() & 0o777, 0o600);
    }
    // host 기기 목록에는 조작 기기 하나(이름 「aaa TUI · <이 기기>」)
    let devices = h.ok(&["web", "devices"]);
    let mine: Vec<&serde_json::Value> = devices["devices"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["label"].as_str().is_some_and(|l| l.starts_with("aaa TUI · ")))
        .collect();
    assert_eq!(mine.len(), 1, "{devices}");
    assert_eq!(mine[0]["role"], "controller");
    let id = mine[0]["id"].as_str().unwrap().to_string();

    // 나갔다 다시 붙으면 묻지 않는다. 나간 화면의 lease는 풀린다(같은 머신 TUI와 같다)
    tui.cmd("q");
    assert!(tui.wait_exit(), "detach는 성공으로 끝나야 한다");
    drop(tui);
    hw.wait("lease 해제", |w| w.pane(&first).is_some_and(|p| p.lease.is_none()).then_some(()));
    let mut tui = Tui::start_args(&dev, &["attach", "--remote", &url], &[], COLS);
    tui.wait_text("다시 붙기", |s| s.contains("far-42") && tab_chips(s) == ["far"]);
    assert!(!tui.text().contains("pairing 코드"), "{}", tui.text());

    // 보기 기기: 「보기 전용」, 크기를 걸지 않고 입력은 pane에 가지 않는다
    let viewer = lab.node("viewer", None);
    let code = h.ok(&["web", "pair", "--reader"])["code"].as_str().unwrap().to_string();
    let mut ro = Tui::start_args(&viewer, &["attach", "--remote", &url], &[], WIDE);
    ro.wait_text("pairing 물음(보기)", |s| s.contains("pairing 코드:"));
    ro.type_line(&code);
    ro.wait_text("보기 화면", |s| s.contains("far-42") && s.contains(" 보기 전용 "));
    // 조작 기기 입력이 따라 보일 때는 보기 기기가 붙으며 보냈을 요청의 답도 이미 왔다(한 연결 안 차례): 크기를 걸었다면
    // host 거부(「보기 권한으로는 조작할 수 없다」)가 오류 막대에 서 있다
    tui.type_line("echo mid-$((1+1))");
    tui.wait_text("조작 기기 입력", |s| s.contains("mid-2"));
    ro.wait_text("보기 화면도 따라옴", |s| s.contains("mid-2"));
    assert!(!ro.text().contains("ERROR"), "보기 기기가 host가 거부할 요청을 보냈다:\n{}", ro.text());
    hw.wait("조작 기기의 lease와 크기", |w| {
        let p = w.pane(&first)?;
        (w.held_by(&first, "tui@") && (p.cols, p.rows) == (COLS - side - GUTTER, ROWS - 1)).then_some(())
    });
    ro.type_line("echo ro-$((2*3))");
    ro.wait_text("입력 거부", |s| s.contains("보기 권한 화면이라 pane에 입력하지 않는다"));
    dump("reader", &ro);
    tui.type_line("echo after-$((3*3))");
    tui.wait_text("조작 기기 입력", |s| s.contains("after-9"));
    ro.wait_text("보기 화면도 따라옴", |s| s.contains("after-9"));
    assert!(!h.screen(&first).contains("ro-6"), "보기 기기 입력이 pane에 갔다:\n{}", h.screen(&first));

    // 조작 기기를 취소하면 그 화면은 까닭과 함께 끝난다
    h.ok(&["web", "revoke", &id]);
    assert!(!tui.wait_exit(), "취소된 화면은 실패로 끝나야 한다");
    tui.wait_text("취소 까닭", |s| s.contains("이 기기가 host에서 취소됐다"));
    dump("revoked", &tui);
    drop(tui);
    // 다시 붙으면 받지 않는 token을 지우고 새 코드를 묻는다
    let tui = Tui::start_args(&dev, &["attach", "--remote", &url], &[], COLS);
    tui.wait_text("다시 pairing", |s| {
        s.contains("저장한 기기 token을 host가 받지 않는다") && s.contains("pairing 코드:")
    });
    assert!(!token.exists());
}

/// herdr 키와 모드(herdr 0.9.5 `KeysConfig::default`·`route_key`): prefix는 시간 제한 없이 다음 키 하나를 기다리고
/// Esc·모르는 키는 아무것도 보내지 않고 취소한다. prefix 두 번은 prefix를 보낸다. v·- 나누기, h/j/k/l·Tab·Shift+Tab 포커스,
/// x 닫기, o 질문 pane으로, ? 단축키 창(검색), shift+r 설정 다시 읽기(prefix를 바꿈, 읽지 못하면 ERROR 막대).
#[test]
fn herdr_keys_and_modes() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "keys"]);
    let first = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| tab_chips(s) == ["keys"]);
    hw.wait("이 화면 크기", |w| w.held_by(&first, "tui@").then_some(()));

    // 1) PREFIX 막대(herdr render_mode_bar)와 Esc 취소: ESC는 셸에 가지 않는다
    tui.send(b"\x02");
    tui.wait_text("PREFIX 막대", |s| {
        s.contains(" PREFIX ") && s.contains("esc 취소") && s.contains("ctrl+b prefix 보내기")
    });
    tui.send(b"\x1b");
    tui.wait_text("취소", |s| !s.contains(" PREFIX "));
    // 모르는 키(y)도 취소하고 보내지 않는다
    tui.cmd("y");
    tui.wait_text("취소", |s| !s.contains(" PREFIX "));
    tui.type_line("echo after-$((2+3))");
    tui.wait_text("뒤 입력", |s| s.contains("after-5"));
    let line = h.screen(&first);
    assert!(!line.contains("yecho") && !line.contains("^["), "취소한 키가 셸에 갔다:\n{line}");

    // 2) prefix 두 번은 prefix 키(ctrl+b = 0x02)를 pane에 보낸다
    tui.type_line("stty -iexten; cat -v");
    tui.wait_text("cat 대기", |s| s.contains("cat -v"));
    tui.cmd("\x02");
    tui.wait_text("PREFIX 끝", |s| !s.contains(" PREFIX "));
    tui.send(b"\r");
    // `^B` 하나는 tty echo(echoctl)라 cat이 뜨기 전에도 보인다. cat이 읽고 다시 찍은 둘째 `^B`를 본 뒤 끊는다
    // (그 전에 ^C가 닿으면 입력 대기열이 비워지고 그 뒤에 cat이 떠 남는다)
    tui.wait_text("cat이 읽은 ^B", |s| s.matches("^B").count() >= 2);
    tui.send(b"\x03");
    tui.type_line("clear");

    // 3) v 좌우, - 위아래, h/j/k/l·Tab·Shift+Tab 포커스. 입력이 간 pane으로 포커스를 본다.
    tui.cmd("v");
    let right = hw.wait("좌우 나눔", |w| (w.leaves().len() == 2).then(|| w.leaves()[1].clone()));
    tui.cmd("h");
    tui.wait_text("PREFIX 끝", |s| !s.contains(" PREFIX "));
    tui.cmd("-");
    // 왼쪽 pane 아래에 새 pane: 차례는 [first, bottom, right]
    let bottom = hw.wait("위아래 나눔", |w| (w.leaves().len() == 3).then(|| w.leaves()[1].clone()));
    assert_eq!(hw.leaves(), vec![first.clone(), bottom.clone(), right.clone()]);
    let typed_in = |tui: &mut Tui, word: &str, pane: &str| {
        tui.type_line(&format!("echo {word}-$((1+1))"));
        let out = format!("{word}-2");
        tui.wait_text(word, |s| s.contains(&out));
        assert!(h.screen(pane).contains(&out), "{word}이 {pane}에 가지 않았다");
    };
    typed_in(&mut tui, "bottom", &bottom);
    tui.cmd("k");
    typed_in(&mut tui, "top", &first);
    tui.cmd("l");
    typed_in(&mut tui, "east", &right);
    tui.cmd("\t");
    typed_in(&mut tui, "cycled", &first);
    tui.cmd("\x1b[Z");
    typed_in(&mut tui, "back", &right);

    // 4) 첫 pane으로 돌아간다(`o`는 알림 카드의 pane으로 간다: agent_cards_like_herdr)
    tui.cmd("\t");
    typed_in(&mut tui, "asked", &first);

    // 5) x: 포커스 pane을 닫는다
    tui.cmd("x");
    hw.wait("닫힘", |w| (w.leaves() == vec![bottom.clone(), right.clone()]).then_some(()));

    // 6) ? 단축키 창: 묶음과 키, / 검색, Esc 두 번으로 닫기
    tui.cmd("?");
    tui.wait_text("단축키 창", |s| s.contains("prefix+?") && s.contains("prefix 모드") && s.contains("esc 닫기"));
    // 넘기면 아래 묶음이 보인다
    tui.send(b"\x1b[6~");
    tui.wait_text("넘김", |s| s.contains("prefix+w") && s.contains("목록 탐색") && !s.contains("prefix 모드"));
    tui.send(b"/");
    tui.wait_text("검색", |s| s.contains("esc 돌아가기"));
    tui.send("나누기".as_bytes());
    tui.wait_text("거름", |s| {
        s.contains("좌우로 나누기") && s.contains("위아래로 나누기") && !s.contains("목록 탐색")
    });
    tui.send(b"\x1b");
    tui.wait_text("검색 끝", |s| s.contains("prefix 모드") && s.contains("esc 닫기"));
    tui.send(b"\x1b");
    tui.wait_text("닫힘", |s| !s.contains("prefix 모드"));

    // 7) shift+r: host.toml [tui]를 다시 읽는다. prefix를 ctrl+a로 바꾸면 ctrl+a가 prefix다.
    let cfg = h.cfg.join("host.toml");
    std::fs::write(&cfg, LOCAL_ONLY.replace("[tui]\n", "[tui]\nprefix = [\"ctrl+a\"]\n")).unwrap();
    tui.cmd("R");
    tui.wait_text("다시 읽음", |s| s.contains("설정을 다시 읽었다"));
    tui.send(b"\x01");
    tui.wait_text("새 prefix", |s| s.contains(" PREFIX ") && s.contains("ctrl+a prefix 보내기"));
    tui.send(b"?");
    tui.wait_text("단축키 창", |s| s.contains("prefix 모드") && s.contains("ctrl+a"));
    tui.send(b"\x1b");
    tui.wait_text("닫힘", |s| !s.contains("prefix 모드"));
    // 읽지 못하는 값이면 ERROR 막대로 알리고 기본(ctrl+b)으로 돌아간다
    std::fs::write(&cfg, LOCAL_ONLY.replace("[tui]\n", "[tui]\nprefix = \"nope\"\n")).unwrap();
    tui.send(b"\x01");
    tui.wait_text("PREFIX", |s| s.contains(" PREFIX "));
    tui.send(b"R");
    tui.wait_text("ERROR 막대", |s| s.contains(" ERROR ") && s.contains("prefix 값을 읽지 못했다: nope"));
    tui.send(b"\x02");
    tui.wait_text("기본 prefix", |s| s.contains(" PREFIX ") && s.contains("ctrl+b prefix 보내기"));
    tui.send(b"q");
    assert!(tui.wait_exit());
}

/// agent 알림 카드(herdr 0.9.5 `notification_policy.rs`·`render_notification_card`, plan §8.185): 보는 화면 없이 다른 agent의
/// 턴이 끝나거나 질문이 열리면 오른쪽 아래 카드가 1초 뒤 뜬다. 지금 탭의 pane은 띄우지 않고(herdr는 pane이 아니라 탭으로
/// 거른다), `prefix o`·카드 누름은 그 pane으로 간다. host는 보는 pane(lease)의 끝남은 알리지 않는다
#[test]
fn agent_cards_like_herdr() {
    let mut lab = Lab::new();
    // 화면 감지 시작 유예(3초)를 뺀다: 시작 화면이 바로 쉼이다
    let (h, mut hw) = start_agent_env(&mut lab, LOCAL_ONLY, &[("AAA_TEST_DETECT_GRACE_MS", "0")]);
    // 실행 파일 이름이 claude인 셸: 줄 하나마다 Claude 화면을 그린다(go 일하는 줄, end 입력 상자, ask 권한 확인창)
    let claude = lab.root.join("cbin").join("claude");
    std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
    std::fs::write(&claude, "#!/bin/sh\nexec /bin/sh \"$@\"\n").unwrap();
    std::fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let clear = "printf '\\033[2J\\033[H'";
    let idle = format!("{clear}; printf '%s\\n' '────────────────' '❯ ' '────────────────' '  ? for shortcuts'");
    let work = format!("{clear}; printf '%s\\n' '✻ Working… (1s · esc to interrupt)'");
    let ask = format!(
        "{clear}; printf '%s\\n' ' Bash command' '' '   make test' '' ' Do you want to proceed?' ' ❯ 1. Yes' '   2. No' '' \
         ' Esc to cancel · Tab to amend'"
    );
    let script =
        format!("stty -echo; {idle}; while read a; do case $a in go) {work};; end) {idle};; ask) {ask};; esac; done");
    // 탭 here: 셸(TUI가 처음 여는 pane)과 그 옆 agent(side), 탭 work: agent 하나
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "here"]);
    let shell = leaves(&w)[0].clone();
    let w = h.ok(&["pane", "split", &shell, "--direction", "right", "--", lab::path_str(&claude), "-c", &script]);
    let side = leaves(&w)[1].clone();
    let w =
        h.ok(&["tab", "create", "--machine", "labA", "--label", "work", "--", lab::path_str(&claude), "-c", &script]);
    let busy = leaves(&w)[2].clone();
    let activity = |w: &HostWatch, p: &str| w.pane(p).and_then(|v| v.agent.as_ref()).map(|a| a.activity);
    hw.wait("두 agent 쉼", |w| {
        (activity(w, &side) == Some(aaa_proto::AgentActivity::Idle)
            && activity(w, &busy) == Some(aaa_proto::AgentActivity::Idle))
        .then_some(())
    });
    let mut tui = Tui::start(&h);
    tui.wait_text("두 탭", |s| tab_chips(s) == ["here", "work"]);
    hw.wait("셸 lease", |w| w.held_by(&shell, "tui@").then_some(()));
    // 턴 하나: 작업 중을 본 뒤 쉼으로 끝낸다. 끝난 뒤 host 알림 수
    let turn = |hw: &mut HostWatch, pane: &str| {
        h.ok(&["pane", "run", pane, "go"]);
        hw.wait("작업 중", |w| (activity(w, pane) == Some(aaa_proto::AgentActivity::Working)).then_some(()));
        h.ok(&["pane", "run", pane, "end"]);
        hw.wait("쉼", |w| (activity(w, pane) == Some(aaa_proto::AgentActivity::Idle)).then_some(()));
    };
    let notices_of = |hw: &HostWatch, pane: &str| hw.notices.iter().filter(|n| n.pane.as_deref() == Some(pane)).count();
    // 카드: 오른쪽 아래 네 줄(테두리·`● 제목`·본문·테두리), 마지막 칸이 테두리다
    let card = |s: &Screen, title: &str, body: &str| {
        let lines: Vec<String> = s.text().lines().map(str::to_string).collect();
        let n = lines.len();
        n >= 4
            && lines[n - 3].contains(&format!("● {title}"))
            && lines[n - 2].contains(body)
            && lines[n - 3].trim_end().ends_with('│')
            && lines[n - 1].trim_end().ends_with('┘')
    };

    // 1) 지금 탭(here)의 agent가 끝났다: host는 알리지만(그 pane을 보는 화면은 없다) 카드는 지금 탭이라 뜨지 않는다.
    // 2) 다른 탭(work)의 agent가 끝났다: 카드가 뜬다. 카드는 한 번에 하나이고 1)이 먼저 왔으므로, 1)이 떴다면 2)보다 앞에 보였다
    turn(&mut hw, &side);
    hw.wait("side 끝남 알림", |w| (notices_of(w, &side) == 1).then_some(()));
    turn(&mut hw, &busy);
    let side_shown = std::cell::Cell::new(false);
    tui.wait_screen("work 끝남 카드", |s| {
        if card(s, "claude 끝남", "main · here") {
            side_shown.set(true);
        }
        card(s, "claude 끝남", "main · work")
    });
    assert!(!side_shown.get(), "지금 탭의 끝남 카드가 떴다");
    if let Ok(path) = std::env::var("AAA_TUI_CARD_DUMP") {
        std::fs::write(path, tui.text()).unwrap();
    }
    // prefix o: 카드를 거두고 그 pane(work 탭)으로 간다(이 화면이 그 pane을 쓴다)
    tui.cmd("o");
    tui.wait_text("카드 거둠", |s| !s.contains("claude 끝남"));
    hw.wait("work lease", |w| w.held_by(&busy, "tui@").then_some(()));

    // 3) 보는 pane(이 TUI가 쓰는 work)의 끝남은 host가 알리지 않는다. 울타리: 그 뒤 side의 끝남 알림이 같은 연결로 온다
    turn(&mut hw, &busy);
    turn(&mut hw, &side);
    hw.wait("side 둘째 끝남 알림", |w| (notices_of(w, &side) == 2).then_some(()));
    assert_eq!(notices_of(&hw, &busy), 1, "보는 pane의 끝남을 알렸다: {:?}", hw.notices);
    tui.wait_screen("side 끝남 카드", |s| card(s, "claude 끝남", "main · here"));

    // 4) 입력 필요(질문이 열림): 같은 pane의 끝남 카드를 바꾼다. 카드를 누르면 그 pane(here 탭의 side)으로 간다
    h.ok(&["pane", "run", &side, "ask"]);
    tui.wait_screen("입력 필요 카드", |s| {
        card(s, "claude 입력 필요", "main · here") && !s.text().contains("claude 끝남")
    });
    let at = tui.wait_find("● claude 입력 필요");
    tui.click(at);
    hw.wait("side lease", |w| w.held_by(&side, "tui@").then_some(()));
    tui.wait_text("카드 거둠", |s| !s.contains("claude 입력 필요"));
    // 카드가 없으면 o는 아무것도 하지 않는다(herdr): 포커스는 side 그대로
    tui.cmd("o");
    tui.wait_text("PREFIX 끝", |s| !s.contains(" PREFIX "));
    tui.type_line("x");
    assert!(hw.held_by(&side, "tui@"));

    // 5) 쓰던 pane이라도 다른 탭으로 가면 보는 것이 아니다: TUI는 lease를 놓지 않지만 그 pane을 그리지 않는다(herdr: 보이는
    // 탭이 아니면 안 봄). 거기서 끝난 턴은 host가 알리고 카드가 뜬다
    tui.cmd("2");
    tui.wait_text("work 탭", |s| s.contains("? for shortcuts") && !s.contains("Do you want to proceed?"));
    assert!(hw.held_by(&side, "tui@"), "탭을 옮겨도 lease는 그대로다(이 단계의 전제)");
    turn(&mut hw, &side);
    hw.wait("side 셋째 끝남 알림", |w| (notices_of(w, &side) == 3).then_some(()));
    tui.wait_screen("다른 탭에서 쓰던 pane의 끝남 카드", |s| card(s, "claude 끝남", "main · here"));
}

/// 탭(herdr 0.9.5 `open_new_tab_overlay`·`save_rename_overlay`·`request_tab_close`, `[terminal] new_cwd = "follow"`):
/// 새 탭은 이름을 묻고, 새 pane(나누기·새 탭)은 원래 pane이 지금 있는 폴더에서 뜬다. `T` 이름, `X` 닫기(그 탭 세션도 끝냄),
/// 마지막 탭은 닫기 전에 묻는다.
#[test]
fn tabs_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();
    let deep = lab.root.join("작업 폴더");
    std::fs::create_dir_all(&deep).unwrap();
    let want = deep.canonicalize().unwrap();
    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| tab_chips(s) == ["1"]);
    hw.wait("이 화면 크기", |w| w.held_by(&first, "tui@").then_some(()));

    // 1) 셸이 cd한 폴더를 나눈 pane이 이어받는다(host가 agent에 그 pane의 지금 폴더를 묻는다)
    tui.type_line(&format!("cd '{}' && echo moved-$((3*3))", lab::path_str(&deep)));
    tui.wait_text("cd", |s| s.contains("moved-9"));
    // 나누기 바로 뒤에 친 키도 새 pane으로 간다(TUI는 응답이 올 때까지 키를 붙잡는다. herdr는 서버가 차례대로 처리한다)
    tui.cmd("v");
    tui.type_line("pwd; echo held-$((2*2))");
    let right = hw.wait("나눔", |w| (w.leaves().len() == 2).then(|| w.leaves()[1].clone()));
    // TUI는 host 화면(grid)을 그리므로, 보였으면 host 화면에도 있다
    tui.wait_text("새 pane 출력", |s| s.contains("held-4"));
    assert!(h.screen(&right).contains("held-4"), "나누기 뒤 키가 새 pane으로 가지 않았다:\n{}", h.screen(&right));
    // pwd 줄은 pane 폭에서 접힌다(macOS `/private/tmp/…`). 예전에는 기계 prompt의 `\W`(폴더 이름)가 대신 맞았다
    h.wait_output_unwrapped(&right, "/작업 폴더", T);
    assert!(!h.screen(&first).contains("held-4"), "나누기 뒤 키가 옛 pane으로 갔다");
    let cwd_of = |hw: &HostWatch, id: &str| hw.pane(id).and_then(|p| p.cwd.clone()).map(std::path::PathBuf::from);
    hw.wait("나눈 pane의 폴더", |w| cwd_of(w, &right).map(|c| c.canonicalize().ok() == Some(want.clone())));
    assert_eq!(cwd_of(&hw, &right).and_then(|c| c.canonicalize().ok()), Some(want.clone()));

    // 2) 새 탭: 이름 창(기본값 "2"가 골라져 있어 처음 친 글이 바꾼다), 포커스 pane의 폴더에서 뜬다
    tui.cmd("c");
    tui.wait_text("새 탭 이름 창", |s| s.contains("새 탭") && s.contains("↵ 저장") && s.contains("esc 취소"));
    tui.send("빌드\r".as_bytes());
    let (tab2, pane2) = hw.wait("새 탭", |w| {
        let t = w.ws.as_ref()?.tabs.get(1)?;
        (t.name == "빌드").then(|| (t.id.clone(), aaa_proto::layout::leaves(&t.root)[0].clone()))
    });
    hw.wait("새 탭 pane의 폴더", |w| cwd_of(w, &pane2).map(|c| c.canonicalize().ok() == Some(want.clone())));
    tui.wait_text("이름표", |s| tab_chips(s) == ["1", "빌드"]);
    // Esc는 아무것도 만들지 않는다
    tui.cmd("c");
    tui.wait_text("새 탭 이름 창", |s| s.contains("↵ 저장"));
    tui.send(b"\x1b");
    tui.wait_text("창 닫힘", |s| !s.contains("↵ 저장"));

    // 3) T: 지금 탭 이름(^u로 지우고 새로 쓴다), 비우고 Enter면 그대로 둔다
    tui.cmd("T");
    tui.wait_text("이름 창", |s| s.contains("탭 이름 바꾸기") && s.contains("빌드"));
    tui.send(b"\x15");
    tui.send("서버\r".as_bytes());
    hw.wait("이름 바뀜", |w| w.ws.as_ref()?.tabs.iter().any(|t| t.id == tab2 && t.name == "서버").then_some(()));
    tui.wait_text("이름표", |s| tab_chips(s) == ["1", "서버"]);
    tui.cmd("T");
    tui.wait_text("이름 창", |s| s.contains("탭 이름 바꾸기"));
    tui.send(b"\x03\r");
    tui.wait_text("창 닫힘", |s| !s.contains("탭 이름 바꾸기"));
    assert_eq!(tab_chips(&tui.text()), ["1", "서버"]);

    // 4) X: 지금 탭(서버)을 닫는다. 그 탭의 세션도 끝난다. 묻지 않는다(마지막 탭이 아니다)
    tui.cmd("X");
    hw.wait("탭 닫힘", |w| (w.ws.as_ref()?.tabs.len() == 1).then_some(()));
    // 닫힌 탭의 pane은 세션이 끝나고 기록에서도 빠진다(herdr `tab close`, §8.75 `close_panes`)
    hw.wait("세션 끝남", |w| w.pane(&pane2).is_none().then_some(()));
    tui.wait_text("이름표", |s| tab_chips(s) == ["1"]);

    // 5) 마지막 탭은 묻는다: Esc는 그대로, Enter는 닫는다
    tui.cmd("X");
    tui.wait_text("확인 창", |s| {
        s.contains("마지막 탭을 닫을까?") && s.contains("pane 2개") && s.contains("↵ 닫기")
    });
    tui.send(b"\x1b");
    tui.wait_text("창 닫힘", |s| !s.contains("마지막 탭을 닫을까?"));
    assert_eq!(hw.leaves(), vec![first.clone(), right.clone()]);
    tui.cmd("X");
    tui.wait_text("확인 창", |s| s.contains("마지막 탭을 닫을까?"));
    tui.send(b"\r");
    hw.wait("모두 닫힘", |w| w.ws.as_ref()?.tabs.is_empty().then_some(()));
    tui.wait_text("빈 화면", |s| s.contains("탭이 없다"));
    hw.wait("세션 끝남", |w| [&first, &right].iter().all(|p| w.pane(p).is_none()).then_some(()));
    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// 지금 탭(첫 탭) 뿌리 나눔의 비율
fn split_ratio(w: &HostWatch) -> Option<u16> {
    match &w.ws.as_ref()?.tabs.first()?.root {
        aaa_proto::LayoutNode::Split { ratio, .. } => Some(*ratio),
        aaa_proto::LayoutNode::Pane { .. } => None,
    }
}

/// pane 모드(herdr 0.9.5 `route_resize_key`·`apply_pane_zoom`·`swap_pane_*`): `r` 크기 모드, `z` zoom,
/// `H/J/K/L` 자리 바꾸기. zoom은 host 탭에 남고 나누면 풀린다.
#[test]
fn pane_modes_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| tab_chips(s) == ["1"]);
    hw.wait("이 화면 크기", |w| w.held_by(&first, "tui@").then_some(()));
    tui.type_line("echo left-$((3+4))");
    tui.wait_text("왼쪽 출력", |s| s.contains("left-7"));
    tui.cmd("v");
    let right = hw.wait("나눔", |w| (w.leaves().len() == 2).then(|| w.leaves()[1].clone()));
    assert_eq!(hw.wait("비율", split_ratio), 500);
    // 나눈 pane은 명령 없는 기본 셸(login sh)이다. prompt는 실험실이 정한 `$`다: 기계 rc의 hostname prompt면 runner의 긴
    // hostname이 좁은 pane에서 접혀, 크기를 바꿀 때마다 셸이 다시 그리는 화면이 기계마다 달랐다
    lab::wait_for_diag(
        "나눈 pane prompt `$`",
        T,
        || (h.screen(&right).trim() == "$").then_some(()),
        || h.screen(&right),
    );

    // 1) r 크기 모드: RESIZE 막대. 포커스(오른쪽 pane)는 오른쪽 가장자리 나눔이 없어 왼쪽 가장자리를 옮긴다
    //    (herdr `resize_focused`): l이면 나눔이 오른쪽으로 5%씩
    tui.cmd("r");
    tui.wait_text("RESIZE 막대", |s| s.contains(" RESIZE ") && s.contains("h/l 너비") && s.contains("esc 끝"));
    tui.send(b"l");
    hw.wait("550", |w| (split_ratio(w) == Some(550)).then_some(()));
    tui.send(b"l");
    hw.wait("600", |w| (split_ratio(w) == Some(600)).then_some(()));
    // 칸이 바뀌면 PTY 크기도 다시 건다: 왼쪽이 넓어진다
    hw.wait("왼쪽이 넓다", |w| (w.pane(&first)?.cols >= w.pane(&right)?.cols + 4).then_some(()));
    tui.send(b"\x1b");
    tui.wait_text("크기 모드 끝", |s| !s.contains(" RESIZE "));
    tui.type_line("echo right-$((4+5))");
    tui.wait_text("오른쪽 출력", |s| s.contains("right-9"));
    assert!(h.screen(&right).contains("right-9"), "크기 모드가 끝난 뒤 키는 pane으로 간다");

    // 2) z: host 탭에 zoom, 이름표 `1 Z`, 포커스 pane만 칸 전체로 그리고 크기도 칸 전체
    tui.cmd("z");
    hw.wait("zoom", |w| w.ws.as_ref()?.tabs[0].zoomed.then_some(()));
    tui.wait_text("zoom 화면", |s| tab_chips(s) == ["1", "Z"] && s.contains("right-9") && !s.contains("left-7"));
    hw.wait("칸 전체 크기", |w| (w.pane(&right)?.cols > w.pane(&first)?.cols).then_some(()));
    // zoom 중에도 h/j/k/l은 나눈 칸으로 이웃을 찾고, 옮긴 포커스 pane이 크게 보인다(herdr)
    tui.cmd("h");
    tui.wait_text("왼쪽 zoom", |s| s.contains("left-7") && !s.contains("right-9"));
    tui.cmd("z");
    hw.wait("zoom 풀림", |w| (!w.ws.as_ref()?.tabs[0].zoomed).then_some(()));
    tui.wait_text("둘 다", |s| tab_chips(s) == ["1"] && s.contains("left-7") && s.contains("right-9"));

    // 3) L: 포커스(왼쪽) pane을 오른쪽 이웃과 바꾼다. 칸·비율은 그대로, 포커스는 옮긴 pane을 따라간다
    tui.cmd("L");
    hw.wait("자리 바뀜", |w| (w.leaves() == [right.clone(), first.clone()]).then_some(()));
    assert_eq!(split_ratio(&hw), Some(600));
    tui.type_line("echo moved-$((5+6))");
    tui.wait_text("옮긴 pane 출력", |s| s.contains("moved-11"));
    assert!(h.screen(&first).contains("moved-11"), "포커스가 옮긴 pane을 따라가지 않았다");

    // 4) zoom 중에 나누면 zoom이 풀린다(herdr `split_*`)
    tui.cmd("z");
    hw.wait("다시 zoom", |w| w.ws.as_ref()?.tabs[0].zoomed.then_some(()));
    tui.cmd("-");
    hw.wait("나누면 풀림", |w| (w.leaves().len() == 3 && !w.ws.as_ref()?.tabs[0].zoomed).then_some(()));
    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// 다른 front가 쓰던 pane에도 바로 입력된다(가져오기 없이, §8.33). 입력하거나 터미널 focus를 다시 얻은 화면이
/// 지금 쓰는 화면이 되어 칸 크기를 다시 건다. focus가 없는 동안은 크기를 걸지 않는다.
#[test]
fn lease_conflict_with_another_front() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "shared", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();

    // 다른 front(같은 계약을 쓰는 연결)가 먼저 쓰고 크기를 건다
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let mut other = rt.block_on(aaa_common::front::FrontClient::connect_as(&h.state, "other-front")).unwrap();
    let call = |other: &mut aaa_common::front::FrontClient, req| {
        let r = rt.block_on(other.call(req)).unwrap();
        assert!(r.is_ok(), "{r:?}");
    };
    let other_types = |other: &mut aaa_common::front::FrontClient, text: &str| {
        call(other, aaa_proto::FrontRequest::PaneResize { pane: id.clone(), cols: 70, rows: 15 });
        let data = aaa_proto::B64(format!("{text}\r").into_bytes());
        call(other, aaa_proto::FrontRequest::PaneInput { pane: id.clone(), data });
    };
    other_types(&mut other, "echo from-other");

    let mut tui = Tui::start(&h);
    tui.wait_text("다른 front 표시", |s| s.contains("other-front 크기") && s.contains("from-other"));
    assert_eq!(h.pane(&id)["cols"].as_u64(), Some(70), "다른 화면이 쓰는 동안 크기를 바꿨다");

    // 1) 그냥 입력하면 들어가고, 이 화면이 지금 쓰는 화면이 되어 칸 크기로 맞춘다
    tui.type_line("echo direct-$((6*7))");
    tui.wait_text("바로 입력", |s| s.contains("direct-42"));
    hw.wait("입력한 화면의 크기", |w| {
        (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE - GUTTER).then_some(())
    });
    tui.wait_text("다른 화면 표시가 사라짐", |s| !s.contains("other-front 크기"));

    // 2) 다른 화면이 다시 쓰면 그 크기다. 이 터미널이 focus를 다시 얻으면(ESC [ I) 크기를 다시 건다
    other_types(&mut other, "echo again");
    hw.wait("다른 화면", |w| w.held_by(&id, "other-front").then_some(()));
    tui.wait_text("다른 front 표시", |s| s.contains("other-front 크기"));
    tui.send(b"\x1b[I");
    hw.wait("focus 얻은 화면의 크기", |w| {
        (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE - GUTTER).then_some(())
    });

    // 3) focus를 잃은 동안(ESC [ O)은 크기를 걸지 않는다: 다른 화면이 썼다 놓아도 그 크기 그대로다
    tui.send(b"\x1b[O");
    std::thread::sleep(Duration::from_millis(200));
    other_types(&mut other, "echo third");
    hw.wait("다른 화면", |w| w.held_by(&id, "other-front").then_some(()));
    call(&mut other, aaa_proto::FrontRequest::PaneRelease { pane: id.clone() });
    hw.wait("놓음", |w| w.pane(&id).is_some_and(|p| p.lease.is_none()).then_some(()));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(h.pane(&id)["cols"].as_u64(), Some(70), "focus가 없는 화면이 크기를 걸었다");
    assert!(h.pane(&id)["lease"].is_null());
    drop(other);
}

/// 다른 front(휴대폰)가 좁게 쥐었다 놓아도 줄 끝이 돌아온다. host의 pane 화면(aaa-vt)이 폭이 바뀔 때 다시 접는다(herdr도
/// 다시 접는다: src/pane.rs 「Herdr reflows too」). 전에는 vt100 `set_size`가 좁힌 폭 밖 글을 버려 넓혀도 비었다(coordinator
/// 10-18, t-0330 발견). 셸은 그래서 fish에게 aaa가 다시 접는다고 알린다(`fish_handle_reflow=0`).
#[test]
fn narrow_claim_then_release_keeps_line_ends() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "reflow", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| tab_chips(s) == ["reflow"]);
    hw.wait("이 화면의 크기", |w| {
        (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE - GUTTER).then_some(())
    });
    // pane 폭(COLS-SIDE) 안에 드는 49자 줄. 입력한 명령 줄에는 이 글이 그대로 나오지 않는다
    let long = format!("LONG-{}-END", "0".repeat(40));
    tui.type_line("echo LONG-$(printf %040d 0)-END; echo \"reflow: ${fish_handle_reflow-none}\"");
    tui.wait_text("긴 줄과 fish 알림", |s| s.contains(&long) && s.contains("reflow: 0"));

    // 다른 front가 30칸으로 쥔다: 줄이 접혀 한 줄에 다 보이지 않는다
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let mut other = rt.block_on(aaa_common::front::FrontClient::connect_as(&h.state, "phone-front")).unwrap();
    let mut call = |req| {
        let r = rt.block_on(other.call(req)).unwrap();
        assert!(r.is_ok(), "{r:?}");
    };
    call(aaa_proto::FrontRequest::PaneResize { pane: id.clone(), cols: 30, rows: 15 });
    hw.wait("좁은 크기", |w| (w.held_by(&id, "phone-front") && w.pane(&id)?.cols == 30).then_some(()));
    tui.wait_text("좁게 접힌 줄", |s| {
        s.contains("phone-front 크기") && !s.contains(&long) && s.contains("LONG-0000")
    });

    // 놓고 이 화면이 되찾으면(focus) 넓은 폭에서 줄 끝까지 다시 한 줄이다
    call(aaa_proto::FrontRequest::PaneRelease { pane: id.clone() });
    hw.wait("놓음", |w| w.pane(&id).is_some_and(|p| p.lease.is_none()).then_some(()));
    tui.send(b"\x1b[I");
    hw.wait("되찾은 크기", |w| {
        (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE - GUTTER).then_some(())
    });
    tui.wait_text("줄 끝이 돌아옴", |s| s.contains(&long) && !s.contains("phone-front 크기"));
    // 세션 쪽 화면(`pane read`)도 다시 접었다
    let read = h.ok(&["pane", "read", &id]);
    assert!(read.to_string().contains(&long), "pane read: {read}");
    drop(tui);
}

/// 고른 줄이 spaces 구역 첫 workspace 줄(` spaces` 머리와 빈 줄 바로 아래)인가(탐색 줄 맨 위)
fn at_top(s: &Screen) -> bool {
    let side = s.side_lines();
    s.selected().is_some_and(|(i, _)| i >= 2 && side[i - 2].starts_with(" spaces"))
}

/// 탐색 모드에서 맨 위(spaces 구역 첫 workspace)로 올라간 뒤 `needle` 줄까지 내려간다(spaces 다음 agents 목록,
/// 고른 줄은 selection_bg 바탕).
/// 한 칸씩 고른 줄이 바뀐 것을 보고 다음 키를 누른다.
fn pick(tui: &mut Tui, needle: &str) {
    tui.wait_screen("탐색 모드", |s| s.text().contains(" NAVIGATE ") && s.selected().is_some());
    while !at_top(&tui.screen.0.lock().unwrap()) {
        let now = tui.selected().unwrap();
        tui.send(b"\x1b[A");
        tui.wait_screen("한 칸 올라감", |s| s.selected().is_some_and(|l| l != now));
    }
    for _ in 0..12 {
        let now = tui.selected().unwrap();
        if now.1.contains(needle) {
            return;
        }
        tui.send(b"\x1b[B");
        tui.wait_screen("한 칸 내려감", |s| s.selected().is_some_and(|l| l != now));
    }
    panic!("목록에 {needle}이 없다:\n{}", tui.text());
}

/// 왼쪽 사이드바(§8.35·§8.195, herdr 배치): 웹 목록처럼 프로젝트를 먼저 묶어 프로젝트 머리와 thread 두 줄(첫 줄 「id 제목」, 둘째 줄
/// 왼쪽 상태 말·오른쪽 머신 이름), thread에 딸리지 않은 pane의 「기타」 묶음(줄에 머신 이름)이 보이고, 새 thread를 따라가며, 고르면
/// 탭으로 열고 묶음 머리를 고르면 접는다.
/// herdr처럼 폭과 관계없이 펼친 채(26칸) 시작한다. herdr 키: prefix b 펴기·접기(접으면 4칸 compact, 포커스는 그대로),
/// prefix w 탐색 모드(↑↓ 고르기·Enter 열기·Esc 돌아가기, 접혀 있으면 펼친 목록을 pane 위에 띄워 보인다).
#[test]
fn side_list_like_web() {
    let mut lab = Lab::new();
    let projects = lab.root.join("projects");
    let host = format!("{LOCAL_ONLY}[projects]\nroot = \"{}\"\nallow_profiles = [\"cat\"]\n", lab::path_str(&projects));
    let host = format!("{host}\n[profiles.cat]\nagent = \"custom\"\ncommand = [\"/bin/cat\"]\n");
    let h = lab.node("h", Some(&host));
    std::fs::write(
        h.cfg.join("agent.toml"),
        format!("schema = 1\n[agent]\nwork_root = \"{}\"\n", lab::path_str(&lab.root.join("work"))),
    )
    .unwrap();
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    let mut hw = HostWatch::new(&h);
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "labA"]);
    hw.wait("labA 연결", |w| w.connected("labA").then_some(()));

    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "first", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();
    // 지금 workspace(main) 탭에 없는 pane: 다른 workspace에 띄운다
    let loose = h.new_pane(Some("scratch"), "labA", Some("loose"), &["/bin/sh"], None);
    h.ok(&["project", "create", "demo"]);
    let task = lab.root.join("task.md");
    std::fs::write(&task, "목록 시험\n").unwrap();
    let start = |title: &str| {
        h.ok(&[
            "thread",
            "start",
            "demo",
            "--title",
            title,
            "--machine",
            "labA",
            "--profile",
            "cat",
            "--kind",
            "dir",
            "--task-file",
            lab::path_str(&task),
        ])
    };
    let th = start("alpha-job");
    let alpha = th["id"].as_str().unwrap().to_string();
    let th_pane =
        wait_for("thread pane", T, || h.ok(&["thread", "show", "demo", &alpha])["pane"].as_str().map(String::from));

    // 1) 펼친 사이드바, pane 칸은 사이드바(구분선 포함 26칸)를 뺀 폭. 프로젝트 머리 아래 thread 줄 「id 제목」과 둘째 줄
    //    왼쪽 상태 말(묶음 말로 시작)·오른쪽 머신 이름, 그다음 「기타」 머리와 thread에 딸리지 않은 pane(머신 이름)
    let mut tui = Tui::start_cols(&h, &[], WIDE);
    let alpha_row = format!("{alpha} alpha-job");
    let words = ["확인 필요", "검토 대기", "작업 중", "쉼"];
    tui.wait_screen("목록", |s| {
        let side = s.side_lines();
        let at = |needle: &str| side.iter().position(|l| l.contains(needle));
        let (Some(demo), Some(row), Some(other), Some(first), Some(loose)) =
            (at(" ▾ demo"), at(&alpha_row), at(" ▾ 기타"), at(" first labA"), at(" loose labA"))
        else {
            return false;
        };
        let sub = side[row + 1].trim_start();
        demo + 1 == row
            && words.iter().any(|w| sub.starts_with(w))
            && sub.ends_with("labA")
            && row + 2 <= other
            && other < first.min(loose)
    });
    // thread pane은 pane 줄로 다시 나오지 않는다
    let side = tui.screen.0.lock().unwrap().side_lines();
    assert_eq!(side.iter().filter(|l| l.contains("alpha-job")).count(), 1, "{}", side.join("\n"));
    hw.wait("목록 옆 크기", |w| (w.pane(&first)?.cols == WIDE - SIDE - GUTTER).then_some(()));
    tui.type_line("echo list-$((6*7))");
    tui.wait_text("pane 입력", |s| s.contains("list-42"));

    // 2) 새 thread는 사건으로 따라온다(같은 프로젝트 머리 아래)
    let beta = start("beta-job")["id"].as_str().unwrap().to_string();
    tui.wait_screen("새 thread", |s| {
        let side = s.side_lines();
        side.iter().filter(|l| l.starts_with(" ▾ demo")).count() == 1
            && side.iter().any(|l| l.contains(&format!("{beta} beta-job")))
    });
    // 탭 막대 오른쪽은 사람이 볼 thread 수다(herdr-projects 탭 줄 `projects: N need you`): 보고 없이 pane이 닫힌 thread는
    // 확인 필요다. 그 전에는 없다
    assert!(!tui.text().lines().next().unwrap_or("").contains(" ● 확인 필요"), "{}", tui.text());
    let beta_pane =
        wait_for("beta pane", T, || h.ok(&["thread", "show", "demo", &beta])["pane"].as_str().map(String::from));
    h.ok(&["pane", "kill", &beta_pane]);
    tui.wait_text("확인 필요 수", |s| s.lines().next().is_some_and(|l| l.contains(" ● 확인 필요 1 ")));
    // 보고서용 화면 텍스트(`AAA_TUI_LIST_DUMP=파일`)
    if let Ok(path) = std::env::var("AAA_TUI_LIST_DUMP") {
        std::fs::write(path, tui.text()).unwrap();
    }

    // 3) prefix w로 탐색해 탭에 없는 pane을 고르면 새 탭으로 열린다
    tui.cmd("w");
    pick(&mut tui, "loose");
    tui.send(b"\r");
    hw.wait("새 탭", |w| w.leaves().contains(&loose).then_some(()));
    tui.wait_screen("pane으로 돌아옴", |s| {
        !s.text().contains(" NAVIGATE ") && s.selected().is_none() && s.current().is_some_and(|l| l.contains("loose"))
    });

    // 4) thread 줄을 고르면 그 thread의 pane이 탭으로 열린다
    tui.cmd("w");
    pick(&mut tui, "alpha-job");
    tui.send(b"\r");
    hw.wait("thread 탭", |w| w.leaves().contains(&th_pane).then_some(()));

    // 5) 이미 탭에 있는 pane은 그 탭으로 간다(탭이 늘지 않는다)
    tui.cmd("w");
    pick(&mut tui, "first");
    tui.send(b"\r");
    tui.wait_text("첫 탭", |s| s.contains("list-42"));
    assert_eq!(hw.leaves().len(), 3);

    // 6) 프로젝트 머리를 Enter로 고르면 접는다: 머리는 `▸`, 셈은 남고 thread 줄은 숨는다(지금 pane은 thread가 아니다). 다시
    //    고르면 편다
    tui.cmd("w");
    pick(&mut tui, "▾ demo");
    tui.send(b"\r");
    tui.wait_screen("접은 프로젝트", |s| {
        let side = s.side_lines();
        side.iter().any(|l| l.starts_with(" ▸ demo"))
            && !side.iter().any(|l| l.contains("alpha-job"))
            && !s.text().contains(" NAVIGATE ")
    });
    tui.cmd("w");
    pick(&mut tui, "▸ demo");
    tui.send(b"\r");
    tui.wait_screen("편 프로젝트", |s| {
        let side = s.side_lines();
        side.iter().any(|l| l.starts_with(" ▾ demo")) && side.iter().any(|l| l.contains("alpha-job"))
    });

    // 6b) 머신 머리 줄이 없어 탐색 `m`이 새 탭을 열 머신을 고른다(붙은 머신이 labA뿐이라 labA, U8,
    //     `new_workspace_on_picked_machine_like_herdr`)
    tui.cmd("w");
    tui.wait_text("탐색 모드", |s| s.contains(" NAVIGATE "));
    tui.send(b"m");
    tui.wait_text("머신을 고름", |s| s.contains("새 탭·workspace는 labA에"));
    tui.send(b"\x1b");
    tui.wait_text("탐색 끝", |s| !s.contains(" NAVIGATE "));
    tui.cmd("c");
    tui.wait_text("새 탭 이름 창", |s| s.contains("↵ 저장"));
    tui.send(b"\r");
    hw.wait("새 셸 탭", |w| (w.leaves().len() == 4).then_some(()));

    // 7) Esc는 탐색을 끝낸다
    tui.cmd("w");
    tui.wait_text("탐색 모드", |s| s.contains(" NAVIGATE "));
    tui.send(b"\x1b");
    tui.wait_text("Esc", |s| !s.contains(" NAVIGATE "));

    // 7b) 위 spaces 구역(herdr `render_sidebar`): 기본 workspace 먼저 workspace 줄, 지금 workspace는 active_row_bg, 아래 구분선과
    //     ` agents` 머리. 탐색은 지금 workspace 줄에서 시작하고, scratch를 골라 Enter면 그 workspace로 바뀌어 탭이 loose
    //     하나·포커스는 loose다(herdr `switch_workspace`, 포커스는 front마다). main을 다시 고르면 돌아온다
    tui.wait_screen("두 구역", |s| {
        let side = s.side_lines();
        let at = |needle: &str| side.iter().position(|l| l.contains(needle));
        let (Some(head), Some(main), Some(scratch), Some(agents)) =
            (at(" spaces"), at(" main"), at(" scratch"), at(" agents"))
        else {
            return false;
        };
        head + 2 == main
            && main + 1 == scratch
            && side[agents - 1].starts_with('─')
            && side[agents].ends_with("projects")
            && s.current_space().is_some_and(|l| l.contains("main"))
    });
    tui.cmd("w");
    tui.wait_screen("지금 workspace에서 시작", |s| s.selected().is_some_and(|(_, l)| l.contains("main")));
    pick(&mut tui, "scratch");
    tui.send(b"\r");
    tui.wait_screen("scratch로", |s| {
        tab_chips(&s.text()) == ["loose"]
            && s.current_space().is_some_and(|l| l.contains("scratch"))
            && s.current().is_some_and(|l| l.contains("loose"))
    });
    tui.cmd("w");
    pick(&mut tui, "main");
    tui.send(b"\r");
    tui.wait_screen("main으로", |s| {
        s.current_space().is_some_and(|l| l.contains("main")) && tab_chips(&s.text()).len() == 4
    });

    // 8) prefix b로 접으면 4칸 compact 열(번호·상태 점, herdr `render_collapsed_sidebar`)만 남는다
    tui.cmd("1");
    tui.wait_text("첫 탭", |s| s.contains("list-42"));
    tui.cmd("b");
    tui.wait_text("접힘", |s| !s.contains(" ▾ demo") && s.lines().next().is_some_and(|l| l.starts_with("1 ")));
    hw.wait("compact 옆 폭", |w| (w.pane(&first)?.cols == WIDE - COMPACT - GUTTER).then_some(()));
    let collapsed_kept = |want: bool| {
        let want = serde_json::json!(want);
        wait_for("host가 남긴 접기", T, || {
            (host_prefs(&h).settings.get("tuiSidebarCollapsed") == Some(&want)).then_some(())
        })
    };
    collapsed_kept(true);
    tui.cmd("q");
    assert!(tui.wait_exit());
    drop(tui);

    // 8b) 접은 것은 host가 기억한다(herdr `sidebar_collapsed`, plan §8.193): 다시 붙으면 접힌 채, prefix b로 펴면 그것을 기억한다
    let mut tui = Tui::start_cols(&h, &[], WIDE);
    tui.wait_text("접힌 채 다시 붙음", |s| {
        s.contains("list-42") && s.lines().next().is_some_and(|l| l.starts_with("1 "))
    });
    tui.cmd("b");
    tui.wait_text("폄", |s| s.contains(" ▾ demo"));
    collapsed_kept(false);
    tui.cmd("q");
    assert!(tui.wait_exit());
    drop(tui);

    // 9) 새로 붙은 TUI는 좁아도(90칸) 펼친 채 시작한다(herdr는 폭으로 접지 않는다, `sidebar_start_collapsed`와 기억한 값으로 정한다).
    //    좌우로 나눠 두 pane에 글을 둔 채 탐색 모드를 켜도 두 pane 글이 그대로 보인다(coordinator 실측 「w 탐색 때
    //    오른쪽 pane이 비어 보였다」 재현 시도). 접고 탐색하면 펼친 목록을 pane 위에 띄우고(pane 크기는 그대로),
    //    prefix b로 펴면 사이드바 옆 크기가 된다(포커스는 pane 그대로: 바로 입력된다)
    let mut tui = Tui::start(&h);
    tui.wait_text("펼친 채 시작", |s| {
        s.contains("list-42") && s.contains(" ▾ demo") && tab_chips(s).first().is_some_and(|c| c == "first")
    });
    hw.wait("사이드바 옆 폭", |w| (w.pane(&first)?.cols == COLS - SIDE - GUTTER).then_some(()));
    tui.cmd("v");
    let right = hw.wait("나눔", |w| w.tab_of(&first).filter(|l| l.len() == 2).map(|l| l[1].clone()));
    tui.type_line("echo right-side-$((4*4))");
    let both = |s: &str| {
        s.lines().map(panes_part).any(|l| l.find("right-side-16").is_some_and(|i| l[..i].contains('│')))
            && s.lines().map(panes_part).any(|l| l.contains("list-42"))
    };
    tui.wait_text("두 pane 글", both);
    tui.cmd("w");
    tui.wait_screen("탐색 중에도 두 pane 글", |s| {
        s.text().contains(" NAVIGATE ") && s.selected().is_some() && both(&s.text())
    });
    tui.send(b"\x1b");
    tui.wait_text("탐색 끝", |s| !s.contains(" NAVIGATE ") && both(s));
    tui.cmd("b");
    tui.wait_text("접힘", |s| !s.contains(" ▾ demo"));
    // 나눈 탭은 pane마다 테두리 상자라(herdr `pane_borders = "auto"`) 두 pane 폭 합은 칸 폭에서 4칸 작다
    hw.wait("compact 옆 폭", |w| {
        (w.pane(&right)?.cols + w.pane(&first)?.cols + 4 + 2 * GUTTER == COLS - COMPACT).then_some(())
    });
    let cols = hw.pane(&first).unwrap().cols;
    tui.cmd("w");
    tui.wait_screen("띄운 목록", |s| {
        let t = s.text();
        t.contains(" NAVIGATE ") && t.contains(" ▾ demo") && s.selected().is_some() && t.contains("right-side-16")
    });
    tui.send(b"\x1b");
    tui.wait_text("목록 닫힘", |s| !s.contains(" ▾ demo"));
    assert_eq!(hw.pane(&first).unwrap().cols, cols, "탐색 모드가 pane 크기를 바꿨다");
    tui.cmd("b");
    tui.wait_text("펼침", |s| s.contains(" ▾ demo") && !s.contains(" NAVIGATE "));
    hw.wait("사이드바 옆 크기", |w| {
        (w.pane(&right)?.cols + w.pane(&first)?.cols + 4 + 2 * GUTTER == COLS - SIDE).then_some(())
    });
    tui.type_line("echo still-$((3*3))");
    tui.wait_text("포커스 그대로", |s| s.contains("still-9"));
    assert!(h.screen(&right).contains("still-9"), "포커스는 나눈 오른쪽 pane 그대로");
}

/// agents 구역의 지금 pane 줄에 적힌 머신(「기타」 pane 줄은 제목 뒤에 머신 이름, §8.195)
fn current_machine(s: &Screen) -> Option<String> {
    let line = s.current()?;
    ["labA", "labB"].into_iter().find(|m| line.contains(&format!(" {m}"))).map(String::from)
}

/// 차이 목록 U8(herdr endpoint 줄 `handle_endpoint_machine_click`·spaces footer ` new`·`NewWorkspace`): 「+ 새 셸」 줄은
/// 없다. 고른 머신에 새 workspace(spaces 맨 아랫줄 ` new · 머신`, `prefix shift+n`, 이름 `workspace`·`workspace-2`)와 새 탭
/// (`prefix c`)을 연다. 목록이 프로젝트 먼저라(§8.195) 머신 머리 줄은 없고, 머신은 ` · 머신`을 누르거나 탐색 `m`으로 다음 붙은
/// 머신으로 돌려 고른다. 고른 머신은 포커스가 바뀌면 풀린다(포커스 pane의 머신). pane이 하나도 없는 원격 머신에 셸을 여는 길이다.
#[test]
fn new_workspace_on_picked_machine_like_herdr() {
    let mut lab = Lab::new();
    let host = LOCAL_ONLY.replace("agent_listen = \"\"", "agent_listen = \"127.0.0.1:0\"");
    let (h, mut hw) = start_with(&mut lab, &host);
    // 원격 머신 labB를 등록한다(e2e `enroll_remote_agent_and_run_short_command`와 같다)
    let addr = h.ok(&["status"])["agent_listen"].as_str().expect("agent_listen").to_string();
    let b = lab.node("b", None);
    h.ok(&["machine", "enroll", "--open", "5m"]);
    let (_, mut out) = lab.spawn_piped("enroll", &b, &["agent", "enroll", &addr, "--name", "labB"]);
    let line = lab::read_line_containing(&mut out, "--code");
    let code = line.split("--code").nth(1).unwrap().trim().to_string();
    h.ok(&["machine", "approve", "labB", "--code", &code]);
    assert!(lab.wait_exit("enroll", T).success(), "enroll 실패: {}", lab.log("enroll"));
    lab.spawn("agentB", &b, &["agent", "--host", &addr, "--name", "labB"]);
    hw.wait("labB 연결", |w| w.connected("labB").then_some(()));
    h.ok(&["tab", "create", "--machine", "labA", "--", "/bin/sh"]);
    let machine_of = |ws: &str| -> Vec<String> {
        leaves(&h.ok(&["workspace", "get", ws])).iter().map(|p| h.pane(p)["machine"].as_str().unwrap().into()).collect()
    };
    // spaces 맨 아랫줄(오른쪽 `menu`는 뺀다)
    let footer = |m: &'static str| {
        move |s: &Screen| s.side_lines().iter().any(|l| l.trim_end_matches("menu").trim_end() == format!(" new · {m}"))
    };
    // 탐색 `m`으로 다음 붙은 머신을 고르고 탐색을 끝낸다(Esc는 포커스를 바꾸지 않는다)
    let pick_next = |tui: &mut Tui, want: &'static str| {
        tui.cmd("w");
        tui.wait_text("탐색 모드", |s| s.contains(" NAVIGATE "));
        tui.send(b"m");
        tui.wait_screen("머신을 고름", footer(want));
        tui.send(b"\x1b");
        tui.wait_text("탐색 끝", |s| !s.contains(" NAVIGATE "));
    };

    // 1) 「+ 새 셸」 줄도 머신 머리 줄도 없고 spaces 맨 아랫줄은 ` new · labA`(포커스 pane의 머신). pane 줄에 머신 이름
    let mut tui = Tui::start_cols(&h, &[], WIDE);
    tui.wait_screen("머신 둘", |s| footer("labA")(s) && current_machine(s).as_deref() == Some("labA"));
    assert!(!tui.text().contains("새 셸") && !tui.text().contains(" ▾ labB"), "{}", tui.text());

    // 2) ` · labA`를 누르면 다음 붙은 머신 labB(` new · labB`). ` new`를 누르면 labB에 새 workspace `workspace`가 생겨 그리로
    //    가고 포커스는 그 셸이다
    let at = tui.wait_find(" new · labA");
    tui.click((at.0, at.1 + 5));
    tui.wait_screen("labB를 고름", footer("labB"));
    let at = tui.wait_find(" new · labB");
    tui.click((at.0, at.1 + 1));
    tui.wait_screen("새 workspace", |s| {
        s.current_space().is_some_and(|l| l.trim_end_matches(['│', ' ']).ends_with(" workspace"))
            && current_machine(s).as_deref() == Some("labB")
    });
    assert_eq!(machine_of("workspace"), ["labB"]);
    tui.type_line("echo on-b-$((5*5))");
    tui.wait_text("labB 셸에 입력", |s| s.contains("on-b-25"));

    // 3) 탐색 `m`으로 labA를 고르고 prefix shift+n: 없는 이름 `workspace-2`가 labA에
    pick_next(&mut tui, "labA");
    tui.cmd("N");
    tui.wait_screen("둘째 새 workspace", |s| {
        s.current_space().is_some_and(|l| l.contains("workspace-2")) && current_machine(s).as_deref() == Some("labA")
    });
    assert_eq!(machine_of("workspace-2"), ["labA"]);

    // 4) labB를 고르고 prefix c: 새 탭이 labB에 생기고 포커스가 그리로
    pick_next(&mut tui, "labB");
    tui.cmd("c");
    tui.wait_text("새 탭 이름 창", |s| s.contains("↵ 저장"));
    tui.send(b"\r");
    tui.wait_screen("새 탭", |s| tab_chips(&s.text()) == ["1", "2"] && current_machine(s).as_deref() == Some("labB"));
    assert_eq!(machine_of("workspace-2"), ["labA", "labB"]);

    // 5) 고른 머신은 포커스가 바뀌면 풀린다: labB pane에서 labA를 고른 뒤 탭 1(labA)을 거쳐 탭 2로 돌아오면 labB
    pick_next(&mut tui, "labA");
    tui.cmd("1");
    tui.wait_screen("탭 1", |s| current_machine(s).as_deref() == Some("labA"));
    tui.cmd("2");
    tui.wait_screen("탭 2", |s| current_machine(s).as_deref() == Some("labB") && footer("labB")(s));
    drop(tui);
}

/// 재부팅 흉내 뒤 중단 표시. herdr에는 재개 키가 없다: 셸 pane은 저절로 되살아나고(m12
/// `reboot_restores_shells_and_resumes_agents`), 다시 실행하지 않는 프로그램 pane은 badge가 알려 주는 CLI로 띄운다.
#[test]
fn interrupted_badge_and_resume() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    // 띄울 때마다 차례를 센다(첫 실행 `run-1`, 재개 `run-2`)
    let runs = lab.root.join("runs").display().to_string();
    let script = r#"echo x >> "$0"; echo "run-$(( $(wc -l < "$0") ))"; exec cat"#;
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "job", "--", "/bin/sh", "-c", script, &runs]);
    let old = leaves(&w)[0].clone();

    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| s.contains("run-1"));

    lab.kill("agent");
    tui.wait_text("끊김 표시", |s| s.contains("끊김·읽기 전용"));
    assert_eq!(lab.kill_session_hosts(), 1);
    lab.spawn_env("agent2", &h, &["agent", "--host", "local", "--name", "labA"], &[("AAA_TEST_BOOT_ID", "boot-after")]);
    let short: String = old.chars().take(8).collect();
    let badge = format!("중단(재부팅) — aaa pane resume {short}");
    tui.wait_text("중단 표시", |s| s.contains(&badge) && s.contains("run-1"));

    h.ok(&["pane", "resume", &short]);
    hw.wait("재개", |w| {
        let l = w.leaves();
        (l.len() == 1 && l[0] != old && w.pane(&l[0]).is_some_and(|p| p.state.is_running())).then_some(())
    });
    // badge는 옛 pane 기록이 빠질 때 먼저 사라진다. 포커스는 TUI가 새 layout을 받을 때 새 pane으로 옮기므로,
    // 그 전에 친 키는 옛 pane으로 가 사라진다(첫 글자를 잃음). 같은 spec으로 다시 돈 새 pane 화면(`run-2`)이 그려진 뒤 친다.
    tui.wait_text("재개 뒤 새 pane", |s| !s.contains("중단(재부팅)") && s.contains("run-2"));
    // 입력은 새 세션(cat)이 받는다: 터미널 echo와 cat 출력으로 두 번
    tui.type_line("typed-after-resume");
    tui.wait_text("재개 뒤 입력", |s| s.matches("typed-after-resume").count() >= 2);
}

/// 세션 폴더에 저장된 첨부 파일들
fn attachments(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = vec![];
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.parent().is_some_and(|d| d.ends_with("attachments")) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// 불투명한 기본색 3x2(색 관리가 바꾸지 않는 값)
const PIXELS: [[u8; 4]; 6] =
    [[255, 0, 0, 255], [0, 255, 0, 255], [0, 0, 255, 255], [255, 255, 255, 255], [0, 0, 0, 255], [255, 255, 0, 255]];

fn encode_rgba(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().unwrap();
    wr.write_image_data(rgba).unwrap();
    wr.finish().unwrap();
    out
}

fn decode_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut r = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba, "RGBA PNG여야 한다");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// 마우스(herdr `mouse.rs`, 묶음 3d-1): 마우스 보고를 켜고, 탭 이름표·탭 막대 휠·` + `, 이름 창·단축키 창 밖 누르기,
/// 사이드바 줄·`«`/`»`·경계 끌기(열 + 1), pane 누르기 포커스, 끌어 고르기·두 번 누름 낱말 복사, 나가면 보고를 끈다.
/// 복사는 사람의 clipboard 대신 `AAA_TEST_CLIPBOARD_TEXT` 파일로 본다.
#[test]
fn mouse_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let one = leaves(&h.ok(&["tab", "create", "--machine", "labA", "--label", "one", "--", "/bin/sh"]))[0].clone();
    let two = leaves(&h.ok(&["tab", "create", "--machine", "labA", "--label", "two", "--", "/bin/sh"]))[1].clone();
    let copied = lab.root.join("copied.txt");
    let copied_s = lab::path_str(&copied);
    let read_copied = || std::fs::read_to_string(&copied).unwrap_or_default();
    let mut tui = Tui::start_env(&h, &[("AAA_TEST_CLIPBOARD_TEXT", copied_s)]);
    tui.wait_text("두 탭", |s| tab_chips(s) == ["one", "two"]);

    // 1) 마우스 보고를 켠다(crossterm EnableMouseCapture: 1000·1002·1003·1006)
    tui.wait_screen("마우스 보고", |s| [1000, 1002, 1003, 1006].iter().all(|m| s.modes.contains(m)));
    hw.wait("첫 탭 크기", |w| (w.pane(&one)?.cols == COLS - SIDE - GUTTER).then_some(()));
    tui.type_line("echo one-$((1))");
    tui.wait_text("첫 탭 글", |s| s.contains("one-1"));

    // 2) 이름표를 누르고 그 자리에서 떼면 그 탭, 탭 막대 휠은 앞·뒤 탭(herdr)
    let chip = tui.wait_find("two");
    assert_eq!(chip.0, 0, "탭 막대의 이름표");
    tui.click(chip);
    tui.wait_text("둘째 탭", |s| !s.contains("one-1"));
    tui.type_line("echo two-$((2))");
    tui.wait_text("둘째 탭 글", |s| s.contains("two-2"));
    assert!(h.screen(&two).contains("two-2"), "누른 탭의 pane에 들어가야 한다");
    tui.mouse(64, chip, false);
    tui.wait_text("휠로 앞 탭", |s| s.contains("one-1") && !s.contains("two-2"));

    // 3) ` + `는 새 탭 이름 창, 이름 창은 단추가 아닌 곳을 누르면 닫힌다(새 탭 없음)
    let plus = tui.wait_find(" + ");
    assert_eq!(plus.0, 0, "탭 막대의 + 단추");
    tui.click((0, plus.1 + 1));
    tui.wait_text("이름 창", |s| s.contains("새 탭") && s.contains("↵ 저장"));
    tui.click((ROWS as usize - 3, 2));
    tui.wait_text("이름 창 닫힘", |s| !s.contains("↵ 저장"));
    assert_eq!(tab_chips(&tui.text()), ["one", "two"]);

    // 4) 단축키 창은 안을 눌러도 그대로(뒤의 `/`가 검색이 된다), 밖을 누르면 닫힌다
    tui.cmd("?");
    let inside = tui.wait_find("/ 를 누르면");
    tui.click(inside);
    tui.send(b"/");
    tui.wait_text("단축키 창 그대로", |s| s.contains("esc 돌아가기"));
    tui.click((ROWS as usize - 1, 1));
    tui.wait_text("단축키 창 닫힘", |s| !s.contains("단축키"));

    // 5) 사이드바 pane 줄을 누르고 떼면 그 pane이 있는 탭으로
    let row = tui.wait_find(" · two");
    tui.click(row);
    tui.wait_text("둘째 탭 열림", |s| s.contains("two-2"));

    // 6) `«`는 접기(compact 4칸), `»`는 펴기
    let fold = tui.wait_find("«");
    tui.click(fold);
    hw.wait("compact 폭", |w| (w.pane(&two)?.cols == COLS - COMPACT - GUTTER).then_some(()));
    let unfold = tui.wait_find("»");
    tui.click(unfold);
    hw.wait("펼친 폭", |w| (w.pane(&two)?.cols == COLS - SIDE - GUTTER).then_some(()));

    // 7) 경계(사이드바 마지막 열)를 끌면 폭 = 열 + 1(herdr, 한도는 `sidebar_width_like_herdr`)
    let drag = |tui: &mut Tui, from: usize, to: usize| {
        tui.mouse(0, (10, from), false);
        tui.mouse(32, (10, to), false);
        tui.mouse(0, (10, to), true);
    };
    drag(&mut tui, SIDE as usize - 1, 31);
    hw.wait("32칸 사이드바", |w| (w.pane(&two)?.cols == COLS - 32 - GUTTER).then_some(()));
    drag(&mut tui, 31, 40);
    hw.wait("41칸 사이드바", |w| (w.pane(&two)?.cols == COLS - 41 - GUTTER).then_some(()));
    drag(&mut tui, 40, SIDE as usize - 1);
    hw.wait("26칸으로", |w| (w.pane(&two)?.cols == COLS - SIDE - GUTTER).then_some(()));

    // 8) 나눈 탭에서 pane을 누르면 그 pane에 포커스
    tui.cmd("v");
    hw.wait("나눔", |w| w.tab_of(&two).filter(|l| l.len() == 2).map(|_| ()));
    tui.type_line("echo right-$((4))");
    tui.wait_text("오른쪽 글", |s| s.contains("right-4"));
    tui.click((5, SIDE as usize + 3));
    tui.type_line("echo left-$((3))");
    tui.wait_text("왼쪽 글", |s| s.contains("left-3"));
    assert!(h.screen(&two).contains("left-3"), "누른 pane에 들어가야 한다");

    // 9) 끌어 고르기: 누른 칸을 벗어나 끌면 고르고, 떼면 복사하고 알린다(herdr copy_on_select)
    let at = tui.wait_find("left-3");
    tui.mouse(0, at, false);
    tui.mouse(32, (at.0, at.1 + 5), false);
    tui.mouse(0, (at.0, at.1 + 5), true);
    wait_for("고른 글 복사", T, || (read_copied() == "left-3").then_some(()));
    tui.wait_text("복사 알림", |s| s.contains("클립보드에 복사했다"));

    // 10) 두 번 누르면 낱말(빈칸·괄호 등으로 가르고 경로는 한 낱말)
    tui.type_line("printf 'w1 %s w2\\n' src/main.rs");
    let at = tui.wait_find("w1 src/main.rs");
    let word = (at.0, at.1 + 5);
    tui.click(word);
    tui.click(word);
    wait_for("낱말 복사", T, || (read_copied() == "src/main.rs").then_some(()));

    // 11) 나가면(prefix q) 마우스 보고를 끈다
    tui.cmd("q");
    assert!(tui.wait_exit());
    tui.wait_screen("마우스 보고 끔", |s| ![1000, 1002, 1003, 1006].iter().any(|m| s.modes.contains(m)));
}

/// 펼친 사이드바의 경계 열(`│`)로 잰 폭: 모든 줄에서 처음 나오는 `│`가 같은 열일 때(다 그린 화면)만
fn side_width(s: &Screen) -> Option<usize> {
    let mut cols = s.cells.iter().map(|l| l.iter().position(|c| *c == '│'));
    let first = cols.next()??;
    cols.all(|c| c == Some(first)).then_some(first + 1)
}

/// host 공유 설정 문서(TUI 사이드바 배치가 남는 곳, plan §8.193)
fn host_prefs(node: &Node) -> aaa_proto::SharedPreferences {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let mut c = aaa_common::front::FrontClient::connect_as(&node.state, "test-prefs").await.unwrap();
        match c.call(FrontRequest::Preferences).await.unwrap() {
            Ok(FrontResponse::Preferences(p)) => p,
            other => panic!("공유 설정 응답이 아니다: {other:?}"),
        }
    })
}

/// 링크 열기(herdr 0.9.5 ctrl+누름 `pane.link.activate`, plan §8.194): OSC 8 링크는 그 uri, 글 속 URL은 끝 문장 부호와 짝
/// 없는 괄호를 뺀 http(s) 주소를 TUI 기기에서 연다(ctrl만, herdr와 같다). 연 누름의 끌기·놓기는 고르지 않는다. alt+누름은 링크를
/// 열지 않는다. 링크가 아닌 칸은 보통 누름으로 다시 넣는다: 끌면 고르고, 마우스 보고를 켠 프로그램에는 ctrl·alt 표시 그대로 간다.
/// 연 주소는 브라우저 대신 `AAA_TEST_OPEN_URL` 파일로 본다.
#[test]
fn link_click_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let one = leaves(&h.ok(&["tab", "create", "--machine", "labA", "--label", "one", "--", "/bin/sh"]))[0].clone();
    let (opened, copied) = (lab.root.join("opened.txt"), lab.root.join("copied.txt"));
    let read = |p: &std::path::Path| std::fs::read_to_string(p).unwrap_or_default();
    let env = [("AAA_TEST_OPEN_URL", lab::path_str(&opened)), ("AAA_TEST_CLIPBOARD_TEXT", lab::path_str(&copied))];
    let mut tui = Tui::start_env(&h, &env);
    tui.wait_text("탭", |s| tab_chips(s) == ["one"]);
    hw.wait("이 화면 크기", |w| (w.pane(&one)?.cols == COLS - SIDE - GUTTER).then_some(()));

    // 1) OSC 8 링크 글을 ctrl+누르고 끌어 놓으면 그 uri를 열고 고르지 않는다
    tui.type_line("printf '\\033]8;;%s://example.com/osc\\007DOCS\\033]8;;\\007 tail\\n' https");
    let docs = tui.wait_find("DOCS tail");
    tui.mouse(16, (docs.0, docs.1 + 1), false);
    tui.mouse(16 + 32, (docs.0, docs.1 + 3), false);
    tui.mouse(16, (docs.0, docs.1 + 3), true);
    wait_for("OSC 8 링크 열기", T, || (read(&opened) == "https://example.com/osc\n").then_some(()));

    // 2) 글 속 URL: alt+누름은 열지 않고(사용자 결정 10-18, herdr처럼 ctrl만), ctrl+누름은 끝 점과 짝 없는 괄호를 뺀 주소를 연다.
    //    alt가 열었으면 ctrl보다 먼저 파일에 들어가 아래 비교가 어긋난다
    tui.type_line("printf 'see %s://example.com/a(b)c. end\\n' https");
    let see = tui.wait_find("see https");
    tui.mouse(8, (see.0, see.1 + 10), false);
    tui.mouse(8, (see.0, see.1 + 10), true);
    tui.mouse(16, (see.0, see.1 + 10), false);
    tui.mouse(16, (see.0, see.1 + 10), true);
    let both = "https://example.com/osc\nhttps://example.com/a(b)c\n";
    wait_for("글 속 URL 열기", T, || (read(&opened) == both).then_some(()));
    assert_eq!(read(&copied), "", "링크를 연 누름은 고르지 않는다");

    // 3) 링크가 아닌 칸(`end`, "see " 4칸 + URL 26칸 + 빈칸 뒤)의 ctrl+누름은 보통 누름: 끌어 놓으면 고르고 복사한다
    let end = (see.0, see.1 + 31);
    tui.mouse(16, end, false);
    tui.mouse(16 + 32, (end.0, end.1 + 2), false);
    tui.mouse(16, (end.0, end.1 + 2), true);
    wait_for("보통 누름으로 고르기", T, || (read(&copied) == "end").then_some(()));
    assert_eq!(read(&opened), both, "링크가 아닌 칸은 열지 않는다");

    // 4) 마우스 보고를 켠 프로그램에는 링크가 아닌 ctrl+누름이 ctrl 표시(16), alt+누름이 alt 표시(8) 그대로 간다
    tui.type_line("clear; printf '\\033[?1000h\\033[?1006h'; echo mouse-$((1)); cat -v");
    tui.wait_text("보고 켬", |s| s.contains("mouse-1") && !s.contains("DOCS"));
    let at = (6, SIDE as usize + 4);
    let (x, y) = (at.1 - SIDE as usize + 1, at.0);
    tui.mouse(16, at, false);
    tui.mouse(16, at, true);
    tui.mouse(8, at, false);
    tui.mouse(8, at, true);
    tui.send(b"\r");
    let sent = format!("^[[<16;{x};{y}M^[[<16;{x};{y}m^[[<8;{x};{y}M^[[<8;{x};{y}m");
    tui.wait_text("cat이 읽은 ctrl·alt 보고", |s| s.matches(&sent).count() >= 2);
    tui.send(b"\x03");
}

/// 사이드바 폭(사용자 10-18 「herdr만큼 크게, 더 크게도, 테마를 바꿔도 리셋되지 않게」, plan §8.193): 기본 36칸(herdr 기본은
/// 26, 사용자가 herdr에서 쓰는 폭), 경계를 끌면 72칸까지(`[tui] sidebar_max_width` 기본). 끌어 둔 폭은 설정 창에서 테마를
/// 바꿔도(host.toml `[tui] theme`만 쓰고 다시 읽는다) 그대로이고, host 공유 설정 `tuiSidebarWidth`에 남아 다시 붙어도 그
/// 폭으로 시작한다(herdr는 client 기기 파일 `client-shell/*.json`). 경계를 두 번 누르면 설정 폭으로 돌아가고 남긴 값을 지운다.
#[test]
fn sidebar_width_like_herdr() {
    const W: u16 = 160;
    let mut lab = Lab::new();
    let (h, mut hw) = start_with(&mut lab, &LOCAL_ONLY.replace("sidebar_width = 26\n", ""));
    let one = leaves(&h.ok(&["tab", "create", "--machine", "labA", "--label", "one", "--", "/bin/sh"]))[0].clone();
    let host_toml = || std::fs::read_to_string(h.cfg.join("host.toml")).unwrap();
    let tab_bg = |s: &Screen| s.bgs[0][W as usize - 1];
    let width_kept = |want: serde_json::Value| {
        wait_for("host가 남긴 폭", T, || {
            (host_prefs(&h).settings.get("tuiSidebarWidth") == Some(&want)).then_some(())
        })
    };
    // 보고서용 화면 텍스트(`AAA_TUI_DUMP=폴더`)
    let dump = |name: &str, tui: &Tui| {
        if let Ok(dir) = std::env::var("AAA_TUI_DUMP") {
            std::fs::write(std::path::Path::new(&dir).join(format!("{name}.txt")), tui.text()).unwrap();
        }
    };
    let mut tui = Tui::start_cols(&h, &[], W);
    tui.wait_text("셸", |s| s.contains('$'));

    // 1) 기본 36칸
    tui.wait_screen("기본 36칸", |s| side_width(s) == Some(36) && s.text().contains(" ▾ 기타"));
    dump("1-default", &tui);
    hw.wait("pane 폭", |w| (w.pane(&one)?.cols == W - 36 - GUTTER).then_some(()));

    // 2) 경계를 끌어 키운다: 열 + 1, 72칸에서 멈춘다
    let drag = |tui: &mut Tui, from: usize, to: usize| {
        tui.mouse(0, (10, from), false);
        tui.mouse(32, (10, to), false);
        tui.mouse(0, (10, to), true);
    };
    drag(&mut tui, 35, 59);
    tui.wait_screen("60칸", |s| side_width(s) == Some(60));
    dump("2-dragged", &tui);
    drag(&mut tui, 59, 150);
    tui.wait_screen("72칸에서 멈춤", |s| side_width(s) == Some(72));
    dump("2b-max", &tui);
    hw.wait("pane 폭 72", |w| (w.pane(&one)?.cols == W - 72 - GUTTER).then_some(()));
    drag(&mut tui, 71, 59);
    tui.wait_screen("60칸으로", |s| side_width(s) == Some(60));
    width_kept(serde_json::json!(60));

    // 3) 설정 창에서 테마를 바꿔도(쓰고 다시 읽기) 폭은 그대로다. 테마를 입힌 그 화면에서 본다
    tui.cmd("s");
    tui.wait_text("설정 창", |s| s.contains("▸ catppuccin ✓"));
    tui.send(b"jj\r");
    tui.wait_screen("terminal 테마", |s| !s.text().contains(" ↑↓ 고르기") && tab_bg(s) == 0);
    let shown = tui.screen.0.lock().unwrap().text();
    assert_eq!(side_width(&tui.screen.0.lock().unwrap()), Some(60), "테마를 바꿔도 끌어 둔 폭: {shown}");
    dump("3-after-theme", &tui);
    let saved = host_toml();
    assert!(saved.contains("theme = \"terminal\"") && !saved.contains("sidebar_width"), "{saved}");

    // 4) 나갔다 다시 붙으면 host가 기억한 폭으로
    tui.cmd("q");
    assert!(tui.wait_exit());
    let mut tui = Tui::start_cols(&h, &[], W);
    tui.wait_screen("다시 붙어도 60칸", |s| side_width(s) == Some(60));
    dump("4-reattached", &tui);
    hw.wait("pane 폭 60", |w| (w.pane(&one)?.cols == W - 60 - GUTTER).then_some(()));

    // 5) 경계를 두 번 누르면 설정 폭(기본 36)으로, 남긴 폭은 지운다(null)
    tui.click((10, 59));
    tui.click((10, 59));
    tui.wait_screen("설정 폭으로", |s| side_width(s) == Some(36));
    width_kept(serde_json::Value::Null);
    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// workspace 이름·닫기(herdr `prefix shift+w`·`prefix shift+d`, 다시 세기 546)와 사이드바 구역 비율(herdr `sidebar_section_split`,
/// 다시 세기 341, plan §8.193): spaces 줄을 눌러 간 workspace의 이름을 바꾸면 그 workspace에 그대로 있고(보던 pane 글이 보인다),
/// 닫기는 확인 창(`confirm_close`) 뒤 host가 그 pane을 끝내고 TUI는 남은 workspace(main)로 간다. spaces·agents 구역 경계 줄을
/// 끌면 비율이 바뀌고 host 공유 설정 `tuiSidebarSplit`에 남아 다시 붙어도 그 비율이다.
#[test]
fn workspace_keys_and_section_split_like_herdr() {
    let mut lab = Lab::new();
    let (h, _hw) = start(&mut lab);
    h.ok(&["tab", "create", "--machine", "labA", "--label", "home", "--", "/bin/sh"]);
    let side = h.ok(&["workspace", "create", "--label", "side", "--machine", "labA", "--", "/bin/sh"]);
    let side_pane = leaves(&side)[0].clone();
    let names = || -> Vec<String> {
        h.ok(&["workspace", "list"])["workspaces"]
            .as_array()
            .map(|a| a.iter().filter_map(|w| w["name"].as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let mut tui = Tui::start(&h);
    tui.wait_text("두 workspace", |s| s.contains(" · side") && tab_chips(s) == ["home"]);

    // 1) spaces 줄을 눌러 side로 가서 글을 둔다
    let at = tui.wait_find(" · side");
    tui.click(at);
    tui.wait_screen("side로", |s| s.current_space().is_some_and(|l| l.contains("side")));
    tui.type_line("echo side-$((7*6))");
    tui.wait_text("side 글", |s| s.contains("side-42"));

    // 2) prefix shift+w: 지금 이름으로 시작하는 이름 창, ^u로 지우고 새 이름
    tui.cmd("W");
    tui.wait_text("이름 창", |s| s.contains("workspace 이름 바꾸기") && s.contains("side"));
    tui.send(b"\x15");
    tui.send(b"work\r");
    wait_for("host 이름 바뀜", T, || {
        let n = names();
        (n.contains(&"work".to_string()) && !n.contains(&"side".to_string())).then_some(())
    });
    tui.wait_screen("새 이름에 그대로", |s| {
        s.current_space().is_some_and(|l| l.contains("work"))
            && s.text().contains("side-42")
            && !s.text().contains(" · side")
    });

    // 3) prefix shift+d: 확인 창(Esc면 그대로), Enter면 닫고 main으로
    tui.cmd("D");
    tui.wait_text("확인 창", |s| s.contains("workspace를 닫을까?") && s.contains("work — pane 1개"));
    tui.send(b"\x1b");
    tui.wait_text("창 닫힘", |s| !s.contains("workspace를 닫을까?"));
    assert!(names().contains(&"work".to_string()));
    tui.cmd("D");
    tui.wait_text("확인 창", |s| s.contains("workspace를 닫을까?"));
    tui.send(b"\r");
    wait_for("host workspace 닫힘", T, || (!names().contains(&"work".to_string())).then_some(()));
    tui.wait_screen("main으로", |s| {
        s.current_space().is_some_and(|l| l.contains("main"))
            && tab_chips(&s.text()) == ["home"]
            && !s.text().contains(" · work")
    });
    wait_for("닫은 workspace의 pane 끝남", T, || h.json(&["pane", "get", &side_pane]).is_err().then_some(()));

    // 4) 구역 경계 줄(30줄이면 위에서 15째 줄)을 끌어 21째 줄로: 비율 0.7, host가 남긴다
    let divider = |s: &Screen, row: usize| s.side_lines().get(row).is_some_and(|l| l.starts_with(&"─".repeat(20)));
    tui.wait_screen("반반", |s| divider(s, 15));
    tui.mouse(0, (15, 3), false);
    tui.mouse(32, (21, 3), false);
    tui.mouse(0, (21, 3), true);
    tui.wait_screen("21째 줄", |s| divider(s, 21));
    wait_for("host가 남긴 비율", T, || {
        host_prefs(&h).settings.get("tuiSidebarSplit").and_then(|v| v.as_f64()).filter(|r| (r - 0.7).abs() < 1e-6)
    });
    tui.cmd("q");
    assert!(tui.wait_exit());
    let tui = Tui::start(&h);
    tui.wait_screen("다시 붙어도 21째 줄", |s| divider(s, 21));
    drop(tui);
}

/// 묶음 3d-2(herdr): 마우스 보고를 켠 프로그램에는 누름·뗌·휠을 넘기고, 보고를 끈 alt 화면은 휠을 화살표로(DECCKM이면 `ESC O`),
/// 나눔 경계를 끌면 그 나눔 비율, 탭 이름표를 끌면 탭 차례(host `SplitRatio`·`MoveTab`, FRONT_API 1.14)
#[test]
fn mouse_reaches_programs_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let prog = leaves(&h.ok(&["tab", "create", "--machine", "labA", "--label", "prog", "--", "/bin/sh"]))[0].clone();
    h.ok(&["tab", "create", "--machine", "labA", "--label", "b", "--", "/bin/sh"]);
    h.ok(&["tab", "create", "--machine", "labA", "--label", "c", "--", "/bin/sh"]);
    let mut tui = Tui::start(&h);
    tui.wait_text("세 탭", |s| tab_chips(s) == ["prog", "b", "c"]);
    hw.wait("이 화면 크기", |w| (w.pane(&prog)?.cols == COLS - SIDE - GUTTER).then_some(()));
    // pane 하나인 탭은 테두리가 없어 안쪽이 (1, SIDE)부터다. 보고 좌표는 안쪽 0부터 + 1
    let at = (6, SIDE as usize + 4);
    let (x, y) = (at.1 - SIDE as usize + 1, at.0);

    // 1) SGR 보고(1000·1006)를 켠 프로그램에는 누름·뗌·휠이 간다. tty echo 하나와 cat이 다시 찍은 것 하나를 본 뒤 끊는다
    tui.type_line("printf '\\033[?1000h\\033[?1006h'; echo mouse-$((1)); cat -v");
    tui.wait_text("보고 켬", |s| s.contains("mouse-1"));
    tui.click(at);
    tui.mouse(65, at, false);
    tui.send(b"\r");
    let sent = format!("^[[<0;{x};{y}M^[[<0;{x};{y}m^[[<65;{x};{y}M");
    tui.wait_text("cat이 읽은 보고", |s| s.matches(&sent).count() >= 2);
    tui.send(b"\x03");

    // 2) 보고를 끈 alt 화면이면 휠 한 번에 화살표 하나(herdr alternate scroll), DECCKM이면 휠도 키도 `ESC O`
    tui.type_line("printf '\\033[?1000l\\033[?1006l\\033[?1049h'; echo alt-$((2)); cat -v");
    tui.wait_text("alt 화면", |s| s.contains("alt-2") && !s.contains("mouse-1"));
    tui.mouse(65, at, false);
    tui.mouse(64, at, false);
    tui.send(b"\r");
    tui.wait_text("휠 화살표", |s| s.matches("^[[B^[[A").count() >= 2);
    tui.send(b"\x03");
    tui.type_line("printf '\\033[?1h'; echo app-$((3)); cat -v");
    tui.wait_text("DECCKM", |s| s.contains("app-3"));
    tui.mouse(64, at, false);
    tui.send(b"\x1b[A");
    tui.send(b"\r");
    tui.wait_text("ESC O 화살표", |s| s.matches("^[OA^[OA").count() >= 2);
    tui.send(b"\x03");
    tui.type_line("printf '\\033[?1l\\033[?1049l'; clear");
    tui.wait_text("alt 화면 나감", |s| !s.contains("app-3"));

    // 3) 나눔 경계(맞닿은 두 테두리 `┐┌`)를 끌면 그 나눔 비율: 64칸 반(32)에서 10칸 오른쪽 → 42칸, 왼쪽 안쪽 40칸(PTY는 scrollbar 칸을 뺀 39)
    tui.cmd("v");
    let right = hw.wait("좌우 나눔", |w| w.tab_of(&prog).filter(|l| l.len() == 2).map(|l| l[1].clone()));
    hw.wait("나눈 폭", |w| (w.pane(&prog)?.cols == 30 - GUTTER).then_some(()));
    let border = tui.wait_find("┐┌");
    tui.mouse(0, (10, border.1), false);
    tui.mouse(32, (10, border.1 + 10), false);
    tui.mouse(0, (10, border.1 + 10), true);
    hw.wait("끈 만큼 넓어짐", |w| (w.pane(&prog)?.cols == 40 - GUTTER).then_some(()));

    // 4) 이름표를 끌면 넣을 자리에 accent `│`, 떼면 그 자리로(이름표 26·35·44, 마지막 오른쪽 끝 52 = 맨 끝)
    let chip = tui.wait_find("prog");
    assert_eq!(chip.0, 0, "탭 막대의 이름표");
    tui.mouse(0, chip, false);
    tui.mouse(32, (0, 52), false);
    tui.wait_text("넣을 자리 표시", |s| s.lines().next().is_some_and(|l| l.chars().nth(52) == Some('│')));
    tui.mouse(0, (0, 52), true);
    tui.wait_text("탭 차례", |s| tab_chips(s) == ["b", "c", "prog"]);
    hw.wait("host 탭 차례", |w| {
        let names: Vec<String> = w.ws.as_ref()?.tabs.iter().map(|t| t.name.clone()).collect();
        (names == ["b", "c", "prog"]).then_some(())
    });
    // 보던 탭(prog, 포커스는 나눈 새 pane)에 남는다
    tui.type_line("echo still-$((5))");
    tui.wait_text("prog 탭에 남음", |s| s.contains("still-5"));
    assert!(h.screen(&right).contains("still-5"), "보던 pane에 들어가야 한다");

    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// 묶음 3d-3(herdr `context_menu.rs`): 탭 이름표·pane을 오른쪽 누르면 메뉴. 위·아래·Enter나 누르기로 고르고(움직이면 그 줄),
/// Esc·밖 누르기로 닫는다. 「오른쪽 누름을 pane에 보내기」를 켠 보고 pane에만 오른쪽 누름이 간다.
#[test]
fn context_menu_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    h.ok(&["tab", "create", "--machine", "labA", "--label", "one", "--", "/bin/sh"]);
    let two = leaves(&h.ok(&["tab", "create", "--machine", "labA", "--label", "two", "--", "/bin/sh"]))[1].clone();
    let mut tui = Tui::start(&h);
    tui.wait_text("두 탭", |s| tab_chips(s) == ["one", "two"]);
    let menu_open = |s: &str| s.contains("이름 바꾸기") && s.contains("닫기");
    let right = |tui: &mut Tui, at: (usize, usize)| {
        tui.mouse(2, at, false);
        tui.mouse(2, at, true);
    };

    // 1) 탭 메뉴: Esc로 닫고, 다시 열어 아래·Enter(이름 바꾸기)면 그 탭으로 옮겨 이름 창
    let chip = tui.wait_find("two");
    right(&mut tui, chip);
    tui.wait_text("탭 메뉴", |s| menu_open(s) && s.contains("새 탭"));
    tui.send(b"\x1b");
    tui.wait_text("Esc로 닫힘", |s| !menu_open(s));
    right(&mut tui, chip);
    tui.wait_text("탭 메뉴 다시", menu_open);
    tui.send(b"\x1b[B");
    tui.send(b"\r");
    tui.wait_text("탭 이름 창", |s| s.contains("탭 이름 바꾸기") && s.contains("↵ 저장"));
    tui.send(b"\x1b");
    tui.wait_text("이름 창 닫힘", |s| !s.contains("↵ 저장"));
    hw.wait("이 화면 크기", |w| w.held_by(&two, "tui@").then_some(()));

    // 2) pane 메뉴: 움직여 「아래로 나누기」를 고르고 Enter → 아래에 새 pane(포커스)
    right(&mut tui, (8, SIDE as usize + 10));
    let down = tui.wait_find("아래로 나누기");
    tui.mouse(35, down, false);
    tui.send(b"\r");
    let bottom = hw.wait("아래로 나눔", |w| w.tab_of(&two).filter(|l| l.len() == 2).map(|l| l[1].clone()));

    // 3) 포커스(아래)가 아닌 위 pane의 메뉴에는 「포커스 pane과 자리 바꾸기」가 있고, 누르면 자리를 바꾼다
    right(&mut tui, (4, SIDE as usize + 10));
    let swap = tui.wait_find("포커스 pane과 자리 바꾸기");
    tui.click(swap);
    hw.wait("자리 바꿈", |w| (w.tab_of(&two)? == [bottom.clone(), two.clone()]).then_some(()));

    // 4) 메뉴는 밖을 누르면 닫힌다
    right(&mut tui, chip);
    tui.wait_text("탭 메뉴(밖 누르기)", menu_open);
    tui.click((ROWS as usize - 3, SIDE as usize + 2));
    tui.wait_text("밖 누르기로 닫힘", |s| !menu_open(s));

    // 5) 보고 pane의 오른쪽 누름은 메뉴가 먼저다. 「오른쪽 누름을 pane에 보내기」를 켜면 넘긴다
    tui.type_line("printf '\\033[?1000h\\033[?1006h'; echo rc-$((1)); cat -v");
    let at = tui.wait_find("rc-1");
    let inside = (at.0, at.1 + 2);
    right(&mut tui, inside);
    let pass = tui.wait_find("오른쪽 누름을 pane에 보내기");
    tui.click(pass);
    tui.wait_text("메뉴 닫힘", |s| !s.contains("오른쪽 누름을 pane에"));
    right(&mut tui, inside);
    tui.send(b"\r");
    tui.wait_text("cat이 읽은 오른쪽 누름", |s| s.matches("^[[<2;").count() >= 4);
    assert!(!tui.text().contains("pane 닫기"), "넘긴 오른쪽 누름에 메뉴가 열렸다");
    tui.send(b"\x03");

    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// pane 이름(herdr `prefix+shift+p`·pane 메뉴 「Rename pane」·「Clear pane name」, host `pane_rename` plan §8.109):
/// 이름 창에서 붙이면 host·사이드바에 그 이름, 메뉴의 「pane 이름 지우기」는 붙인 이름이 있을 때만 있고 누르면 띄울 때 이름으로
#[test]
fn pane_rename_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "one", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    hw.wait("이 화면 크기", |w| w.held_by(&id, "tui@").then_some(()));
    let right = |tui: &mut Tui, at: (usize, usize)| {
        tui.mouse(2, at, false);
        tui.mouse(2, at, true);
    };

    // 1) prefix shift+p: 붙인 이름이 없으니 빈 창, 치고 Enter면 host 이름과 사이드바 줄
    tui.cmd("P");
    tui.wait_text("pane 이름 창", |s| s.contains("pane 이름 바꾸기") && s.contains("↵ 저장"));
    tui.send(b"api-log\r");
    hw.wait("붙인 이름", |w| (w.pane(&id)?.label.as_deref() == Some("api-log")).then_some(()));
    tui.wait_text("사이드바 이름", |s| s.contains("api-log") && !s.contains("↵ 저장"));

    // 2) pane 메뉴: 붙인 이름이 있으면 「pane 이름 지우기」, 누르면 띄울 때 이름으로
    let at = (10, SIDE as usize + 20);
    right(&mut tui, at);
    let clear = tui.wait_find("pane 이름 지우기");
    tui.click(clear);
    hw.wait("이름 지움", |w| w.pane(&id)?.label.is_none().then_some(()));
    tui.wait_text("사이드바 처음 이름", |s| !s.contains("api-log"));

    // 3) 지운 뒤 메뉴에는 지우기가 없고, 「pane 이름 바꾸기」는 이름 창을 연다
    right(&mut tui, at);
    let rename = tui.wait_find("pane 이름 바꾸기");
    assert!(!tui.text().contains("pane 이름 지우기"), "붙인 이름이 없는데 지우기가 있다");
    tui.click(rename);
    tui.wait_text("이름 창", |s| s.contains("↵ 저장"));
    tui.send(b"db\r");
    hw.wait("다시 붙인 이름", |w| (w.pane(&id)?.label.as_deref() == Some("db")).then_some(()));

    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// 묶음 5a-1(herdr `apply_scroll`·`scroll_reset`): 보통 화면 pane의 휠은 이 화면의 기록을 3줄씩 넘기고(프로그램에는 안 감),
/// 올라가 있는 동안 새 출력이 와도 보던 줄을 지키며, 글자를 치면 맨 아래로 돌아온다(host `PaneScroll`, FRONT_API 1.26).
#[test]
fn wheel_scrolls_history_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "seq", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    hw.wait("이 화면 크기", |w| w.held_by(&id, "tui@").then_some(()));
    // pane은 탭 막대 아래 29줄: 맨 아래 장면은 173..200과 prompt(그 위 기록은 명령 줄과 1..172)
    // 줄 끝 scrollbar 칸(`▕`·`▐`)은 뺀다
    let text = |l: &str| panes_part(l).trim_end_matches(['▕', '▐', ' ']).to_string();
    let shows = |s: &str, n: &str| s.lines().any(|l| text(l) == n);
    let rows = |s: &str| s.lines().skip(1).map(text).collect::<Vec<_>>();
    tui.type_line("seq 1 200");
    tui.wait_text("seq 끝", |s| {
        rows(s).first().is_some_and(|l| l == "173") && rows(s).last().is_some_and(|l| l.ends_with('$'))
    });
    let at = (10, SIDE as usize + 20);

    // 1) 휠 위 한 칸: 3줄 위(170..198)
    tui.mouse(64, at, false);
    tui.wait_text("3줄 위", |s| {
        rows(s).first().is_some_and(|l| l == "170") && rows(s).last().is_some_and(|l| l == "198")
    });

    // 2) 올라가 있는 동안 다른 연결(CLI)의 출력(명령 줄·201..205·prompt 6줄)이 와도 보던 줄을 지킨다:
    //    한 칸 더 올리면 167..195. 지키지 않으면 맨 아래에서 6줄 위(173..201)가 된다.
    h.ok(&["pane", "run", &id, "seq 201 205"]);
    // 205만이 아니라 그 다음 prompt까지 기다린다: 아래 「셸에 아무것도 보내지 않았다」는 prompt 줄을 보는데, 205가 보인 뒤 prompt가
    // 오기 전에 그 단언이 오면(짐 아래 ci-local 10-18 13:57) 화면 끝이 205라 떨어졌다. 아래 「6줄」 셈도 prompt까지 온 뒤의 것이다
    wait_for("host 화면에 205와 prompt", T, || {
        let s = h.screen(&id);
        let tail: Vec<&str> = s.lines().map(str::trim_end).filter(|l| !l.is_empty()).rev().take(2).collect();
        (tail == ["$", "205"]).then_some(())
    });
    tui.mouse(64, at, false);
    tui.wait_text("보던 줄에서 3줄 더 위", |s| {
        rows(s).first().is_some_and(|l| l == "167") && rows(s).last().is_some_and(|l| l == "195")
    });
    // 휠은 셸에 아무것도 보내지 않았다(보냈으면 prompt 줄에 글이 있다)
    assert!(h.screen(&id).trim_end().ends_with('$'), "{}", h.screen(&id));

    // 3) 휠 아래 네 칸(12줄)이면 다시 맨 아래(205와 prompt)
    for _ in 0..4 {
        tui.mouse(65, at, false);
    }
    tui.wait_text("맨 아래", |s| shows(s, "205") && rows(s).last().is_some_and(|l| l.ends_with('$')));

    // 4) 올라간 채 글자를 치면 맨 아래로 돌아와 친 글이 보인다(herdr `scroll_reset`)
    tui.mouse(64, at, false);
    tui.wait_text("다시 위", |s| !shows(s, "205"));
    tui.send(b"echo back-$((1+1))");
    tui.wait_text("맨 아래로", |s| rows(s).last().is_some_and(|l| l.ends_with("echo back-$((1+1))")));
    tui.send(b"\r");
    tui.wait_text("친 명령", |s| shows(s, "back-2"));

    // 5) 붙여넣기(bracketed paste 한 덩이, host `PanePaste`)도 친 글처럼 먼저 맨 아래로
    tui.mouse(64, at, false);
    tui.wait_text("또 위", |s| !shows(s, "back-2"));
    tui.send(b"\x1b[200~echo pasted-$((2+3))\x1b[201~");
    tui.wait_text("붙여 넣고 맨 아래로", |s| {
        rows(s).last().is_some_and(|l| l.ends_with("echo pasted-$((2+3))"))
    });
    tui.send(b"\r");
    tui.wait_text("붙여 넣은 명령", |s| shows(s, "pasted-5"));

    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// 묶음 5a-2(herdr `pane_scrollbars`·`ui/scrollbar.rs`·`mouse.rs` scrollbar): 보통 화면 pane은 오른쪽 1칸이 scrollbar 칸이라
/// PTY가 그만큼 좁고, 기록이 생기면 트랙 `▕`과 thumb `▐`(포커스)가 보인다. 트랙 맨 위를 누르면 맨 위로 뛰고, thumb를 잡아 끝까지
/// 끌면 맨 아래. alt 화면(vim 등)에서는 칸을 돌려줘 PTY가 넓어지고, 나오면 다시 좁아진다.
#[test]
fn pane_scrollbar_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "seq", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    hw.wait("scrollbar 칸을 뺀 크기", |w| {
        (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE - GUTTER).then_some(())
    });
    let bar = COLS as usize - 1;
    let text = |l: &str| panes_part(l).trim_end_matches(['▕', '▐', ' ']).to_string();
    let rows = |s: &str| s.lines().skip(1).map(text).collect::<Vec<_>>();
    let ends = |s: &str, row: usize| s.lines().nth(row).and_then(|l| l.chars().last());

    // 1) 기록이 생기면 트랙과 thumb(맨 아래면 트랙 끝)
    tui.type_line("seq 1 200");
    tui.wait_text("scrollbar", |s| {
        rows(s).last().is_some_and(|l| l.ends_with('$'))
            && ends(s, 1) == Some('▕')
            && ends(s, ROWS as usize - 1) == Some('▐')
    });

    // 2) 트랙 맨 위를 누르면 맨 위(명령 줄과 1부터), thumb도 위로
    tui.click((1, bar));
    tui.wait_text("맨 위", |s| rows(s).get(1).is_some_and(|l| l == "1") && ends(s, 1) == Some('▐'));

    // 3) thumb를 잡아 아래 끝까지 끌고 떼면 맨 아래
    tui.mouse(0, (2, bar), false);
    tui.mouse(32, (15, bar), false);
    tui.mouse(32, (ROWS as usize - 1, bar), false);
    tui.mouse(0, (ROWS as usize - 1, bar), true);
    tui.wait_text("맨 아래", |s| {
        rows(s).last().is_some_and(|l| l.ends_with('$')) && rows(s).iter().any(|l| l == "200")
    });

    // 4) alt 화면에서는 칸을 돌려준다(PTY가 넓어짐), 나오면 다시 좁아진다
    tui.type_line("printf '\\033[?1049h'; echo alt-$((4)); read x");
    tui.wait_text("alt 화면", |s| s.contains("alt-4"));
    hw.wait("alt 화면은 칸 전체", |w| (w.pane(&id)?.cols == COLS - SIDE).then_some(()));
    tui.send(b"\r");
    tui.type_line("printf '\\033[?1049l'; echo normal-$((5))");
    tui.wait_text("보통 화면", |s| s.contains("normal-5"));
    hw.wait("다시 scrollbar 칸", |w| (w.pane(&id)?.cols == COLS - SIDE - GUTTER).then_some(()));

    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// 묶음 5b-1(herdr `selection.rs`·`mouse.rs` selection autoscroll·`PaneSelectionRead`): 끌어 고르다 pane 위 밖으로 나가면
/// 그 거리만큼 넘기고 30ms마다 한 줄씩 더 넘기며 고르고, 떼면 host가 화면 사본의 기록까지 읽어 복사한다. 고르는 중 휠은
/// 기록을 넘기며 고른 끝을 다시 잡는다(host `PaneSelectionRead`, FRONT_API 1.27).
#[test]
fn drag_select_scrolls_history_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "seq", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let copied = lab.root.join("copied.txt");
    let copied_s = lab::path_str(&copied);
    let read_copied = || std::fs::read_to_string(&copied).unwrap_or_default();
    let mut tui = Tui::start_env(&h, &[("AAA_TEST_CLIPBOARD_TEXT", copied_s)]);
    hw.wait("이 화면 크기", |w| w.held_by(&id, "tui@").then_some(()));
    let text = |l: &str| panes_part(l).trim_end_matches(['▕', '▐', ' ']).to_string();
    let rows = |s: &str| s.lines().skip(1).map(text).collect::<Vec<_>>();
    let top = |s: &str| rows(s).first().and_then(|l| l.parse::<u32>().ok());
    tui.type_line("seq 1 200");
    tui.wait_text("seq 끝", |s| top(s) == Some(173) && rows(s).last().is_some_and(|l| l.ends_with('$')));
    let col = SIDE as usize;

    // 1) 「182」 줄(화면 10째 줄) 첫 칸을 누르고 탭 막대(pane 위 밖 1줄)로 끌어 잡고 있으면 3줄 바로, 그 뒤 30ms마다 한 줄씩 올라간다
    tui.mouse(0, (10, col), false);
    tui.mouse(32, (0, col + 2), false);
    tui.wait_text("끌어 고르며 기록 넘기기", |s| top(s).is_some_and(|n| n <= 150));
    tui.mouse(0, (0, col + 2), true);
    // 고른 끝은 뗄 때의 맨 위 줄 셋째 칸, 처음은 「182」의 첫 칸: 누를 때 화면에 없던 기록 줄까지 이어진 숫자로 복사된다
    let got = wait_for("기록까지 복사", T, || {
        let c = read_copied();
        c.ends_with("\n181\n1").then_some(c)
    });
    let nums: Vec<u32> = got.lines().skip(1).map(|l| l.parse().unwrap()).collect();
    assert!(nums.first().is_some_and(|n| *n <= 152) && nums.contains(&160), "{got}");
    assert!(nums.windows(2).all(|w| w[1] == w[0] + 1 || w[1] == 1), "{got}");
    tui.wait_text("복사 알림", |s| s.contains("클립보드에 복사했다"));

    // 2) 치면 맨 아래로 돌아온다. 「w-6」 첫 칸을 누르고 두 칸 끈 채 휠 위 한 칸이면 3줄 위가 고른 끝이 된다(셸에는 안 감)
    tui.type_line("echo w-$((6))");
    let at = tui.wait_find("w-6");
    tui.mouse(0, at, false);
    tui.mouse(32, (at.0, at.1 + 2), false);
    tui.mouse(64, (at.0, at.1 + 2), false);
    // 「w-6」이 있던 줄에 세 줄 위의 「199」가 보인다(사이는 명령 줄과 200)
    tui.wait_text("고르며 휠", |s| s.lines().nth(at.0).map(text).as_deref() == Some("199"));
    tui.mouse(0, (at.0, at.1 + 2), true);
    let got = wait_for("휠로 넓힌 고르기 복사", T, || {
        let c = read_copied();
        c.ends_with("\nw").then_some(c)
    });
    assert!(got.starts_with("9\n200\n") && got.lines().count() == 4, "{got}");
    assert!(h.screen(&id).trim_end().ends_with('$'), "휠이 셸에 가지 않았다: {}", h.screen(&id));

    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// 묶음 5b-2(herdr `copy_mode.rs`): `prefix [`로 copy 모드에 들어가 커서는 터미널 커서 자리, `k`·`0`·`v`·`j`·`y`로 두 줄에 걸쳐
/// 골라 복사하고 나간다. `g`는 기록 맨 앞(명령 줄), `V`·`j`로 줄 단위로 고르고 Enter. PageUp은 화면 줄 − 2만큼 넘기고,
/// `q`·Esc로 나가면 들어올 때의 자리(맨 아래)로 돌아온다.
#[test]
fn copy_mode_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "seq", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let copied = lab.root.join("copied.txt");
    let copied_s = lab::path_str(&copied);
    let read_copied = || std::fs::read_to_string(&copied).unwrap_or_default();
    // 넓게: copy 모드 막대(herdr처럼 넘치면 자른다)가 끝까지 보인다
    let mut tui = Tui::start_cols(&h, &[("AAA_TEST_CLIPBOARD_TEXT", copied_s)], WIDE);
    hw.wait("이 화면 크기", |w| w.held_by(&id, "tui@").then_some(()));
    let text = |l: &str| panes_part(l).trim_end_matches(['▕', '▐', ' ']).to_string();
    let rows = |s: &str| s.lines().skip(1).map(text).collect::<Vec<_>>();
    // 복사 알림 카드가 오른쪽 위 세 줄을 가리므로 줄의 첫 낱말만 본다
    let first = |l: &str| l.split_whitespace().next().unwrap_or_default().to_string();
    let top = |s: &str| rows(s).first().and_then(|l| first(l).parse::<u32>().ok());
    tui.type_line("seq 1 200");
    tui.wait_text("seq 끝", |s| top(s) == Some(173) && rows(s).last().is_some_and(|l| l.ends_with('$')));

    // 1) 커서는 prompt 줄에서: 세 줄 위(198) 첫 칸에서 글자 단위로 한 줄 내려 고르고 y면 복사하고 나간다
    tui.cmd("[");
    tui.wait_text("copy 모드 막대", |s| s.contains(" COPY ") && s.contains("v/space 고르기"));
    tui.send(b"kkk0v");
    tui.wait_text("고르는 중", |s| s.contains("고르는 중"));
    tui.send(b"jy");
    wait_for("글자 단위 복사", T, || (read_copied() == "198\n1").then_some(()));
    tui.wait_text("copy 모드 끝", |s| !s.contains(" COPY "));

    // 2) g는 기록 맨 앞(명령 줄이 맨 위), V·j·j는 세 줄을 통째로, Enter면 복사하고 들어올 때의 자리(맨 아래)로 돌아온다
    tui.cmd("[");
    tui.wait_text("copy 모드 막대", |s| s.contains(" COPY "));
    tui.send(b"g");
    tui.wait_text("기록 맨 앞", |s| {
        rows(s).first().is_some_and(|l| l.contains("seq 1 200")) && rows(s).get(1).is_some_and(|l| first(l) == "1")
    });
    tui.send(b"Vjj\r");
    wait_for("줄 단위 복사", T, || read_copied().ends_with("seq 1 200\n1\n2").then_some(()));
    tui.wait_text("맨 아래로 돌아옴", |s| top(s) == Some(173) && !s.contains(" COPY "));

    // 3) PageUp은 29줄 화면에서 27줄 넘기고, q면 넘기기 전 자리로
    tui.cmd("[");
    tui.wait_text("copy 모드 막대", |s| s.contains(" COPY "));
    tui.send(b"\x1b[5~");
    tui.wait_text("한 쪽 위", |s| top(s) == Some(146));
    tui.send(b"q");
    tui.wait_text("q로 나가 맨 아래", |s| top(s) == Some(173) && !s.contains(" COPY "));

    // 4) copy 모드에서 휠로 3줄 올리면(맨 위 170) 커서(prompt 줄)는 화면 아래 밖이다. `k`면 커서가 화면 맨 아래 줄에 오게
    //    2줄만 내려와 맨 위 172(휠로 옮긴 자리에서 셈한다. 휠 전 자리로 셈하면 넘기지 않아 170에 남는다)
    tui.cmd("[");
    tui.wait_text("copy 모드 막대", |s| s.contains(" COPY "));
    tui.mouse(64, (10, SIDE as usize + 20), false);
    tui.wait_text("copy 모드에서 휠", |s| top(s) == Some(170));
    tui.send(b"k");
    tui.wait_text("커서가 보이게 넘김", |s| top(s) == Some(172));
    tui.send(b"q");
    tui.wait_text("q로 나가 맨 아래", |s| top(s) == Some(173) && !s.contains(" COPY "));
    // 5) 낱말 이동(herdr `PaneCopyMotion`, host가 정함): 바로 위 줄 첫 칸에서 고르기를 시작해 `w`(two)·`w`(-, 문장 부호는
    //    따로 낱말)·`e`(three 끝)로 옮기고 `y`. 답을 기다리는 동안 붙잡은 키가 옮긴 자리에서 이어 「one two-three」가 된다
    tui.type_line("printf 'one two-three four\\n'");
    tui.wait_text("낱말 줄", |s| rows(s).iter().any(|l| l == "one two-three four"));
    tui.cmd("[");
    tui.wait_text("copy 모드 막대", |s| s.contains(" COPY ") && s.contains("w/b/e"));
    tui.send(b"k0vwwey");
    wait_for("낱말 이동 복사", T, || (read_copied() == "one two-three").then_some(()));
    tui.wait_text("copy 모드 끝", |s| !s.contains(" COPY "));

    // 6) 찾기(herdr `PaneCopySearch`): `?19` Enter는 뒤로 찾아 199(19·119·190~199 열두 개 중 마지막)로, 막대에 12/12.
    //    `n`은 같은 방향 다음(198, 11/12), `N`은 반대(199), Esc는 찾기만 지우고 커서(199 첫 칸)는 둔다(herdr). 거기서 다시
    //    `?19` Enter는 커서 앞에서 끝나는 마지막 일치(198), `n`은 197, `ve`·`y`면 「197」(답을 기다리는 동안 붙잡은 키로 잇는다).
    tui.cmd("[");
    tui.wait_text("copy 모드 막대", |s| s.contains(" COPY ") && s.contains("/ ? 찾기"));
    tui.send(b"?19");
    tui.wait_text("찾기 입력 줄", |s| s.contains(" COPY  ?19") && s.contains("enter 찾기"));
    tui.send(b"\r");
    // 현재 일치(199의 「19」)는 accent, 나머지(198)는 surface1 바탕. 커서 칸(accent)은 일치 첫 칸이라 둘째 칸으로 본다
    let bg_at = |s: &Screen, needle: &str, dx: usize| s.find(needle).map(|(r, c)| s.bgs[r][c + dx]);
    tui.wait_screen("뒤로 찾음", |s| {
        s.text().contains("n/N 다시 12/12")
            && bg_at(s, "199", 1) == Some(ACCENT_BG)
            && bg_at(s, "198", 1) == Some(SURFACE1_BG)
            && bg_at(s, "198", 2) != Some(SURFACE1_BG)
    });
    tui.send(b"n");
    tui.wait_text("같은 방향 다음", |s| s.contains("n/N 다시 11/12"));
    tui.send(b"N");
    tui.wait_text("반대 방향", |s| s.contains("n/N 다시 12/12"));
    tui.send(b"\x1b");
    tui.wait_text("찾기 지움", |s| s.contains(" COPY ") && !s.contains("12/12") && s.contains("q/esc 끝"));
    tui.send(b"?19\rnvey");
    wait_for("찾은 자리에서 복사", T, || (read_copied() == "197").then_some(()));
    tui.wait_text("copy 모드 끝", |s| !s.contains(" COPY "));
    // copy 모드 키는 셸에 가지 않았다
    assert!(h.screen(&id).trim_end().ends_with('$'), "{}", h.screen(&id));

    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// 4C 설정 창(herdr `prefix s` `render_settings_overlay`·`apply_settings_choice`, plan §8.178): 테마 구역에서 ↓면 바로 입혀
/// 보고(탭 막대 `panel_bg`), Esc면 되돌리고 아무것도 쓰지 않는다. Enter면 실험실 host.toml `[tui] theme`에 쓰고(다른 줄은
/// 그대로) 닫는다. 상태 표시 구역에서 기호를 고르면 창을 둔 채 `status_indicators`를 쓰고 확인 대기 pane 점이 `×`가 된다.
/// spaces 맨 아랫줄 `menu`(전체 메뉴)를 누르고 Enter면 설정 창이 열린다. host.local.toml이 같은 key를 덮으면 알린다.
#[test]
fn settings_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    // 실행 파일 이름이 claude인 셸(`herdr_keys_and_modes`처럼): 화면에 Claude 확인창을 그려 질문(확인 대기 점)을 연다
    let claude = lab.root.join("bin").join("claude");
    std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
    std::fs::write(&claude, "#!/bin/sh\nexec /bin/sh\n").unwrap();
    std::fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let first = leaves(&h.ok(&["tab", "create", "--machine", "labA", "--", lab::path_str(&claude)]))[0].clone();
    let host_toml = || std::fs::read_to_string(h.cfg.join("host.toml")).unwrap();
    // 탭 막대 오른쪽 끝 칸 바탕(테마 `panel_bg`): catppuccin은 RGB(24,24,37), terminal은 터미널 기본
    const CATPPUCCIN: u32 = 0x0200_0000 | (24 << 16) | (24 << 8) | 37;
    let tab_bg = |s: &Screen| s.bgs[0][COLS as usize - 1];
    let asking_dot = |s: &Screen, dot: &str| {
        let side = s.side_lines();
        let top = s.agents_top();
        side[top..].iter().any(|l| l.starts_with(&format!(" {dot} ")))
    };
    let mut tui = Tui::start(&h);
    tui.wait_text("셸", |s| s.contains('$'));
    h.ok(&[
        "pane",
        "run",
        &first,
        "clear; printf '%s\\n' ' Bash command' '' '   ls' '' ' Do you want to proceed?' ' ❯ 1. Yes' '   2. No' '' \
         ' Esc to cancel · Tab to amend'",
    ]);
    hw.wait("질문", |w| {
        w.pane(&first)?.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question).then_some(())
    });
    tui.wait_screen("확인 대기 점", |s| asking_dot(s, "●") && tab_bg(s) == CATPPUCCIN);

    // 1) 열면 지금 테마에 ✓, ↓↓면 terminal을 입혀 보고, Esc면 되돌리고 쓰지 않는다
    tui.cmd("s");
    tui.wait_text("설정 창", |s| s.contains(" 설정") && s.contains("▸ catppuccin ✓") && s.contains(" 상태 표시 "));
    tui.send(b"jj");
    tui.wait_screen("미리 보기", |s| s.text().contains("▸ terminal") && tab_bg(s) == 0);
    tui.send(b"\x1b");
    tui.wait_screen("되돌림", |s| !s.text().contains(" ↑↓ 고르기") && tab_bg(s) == CATPPUCCIN);
    assert!(!host_toml().contains("theme"), "{}", host_toml());

    // 2) Enter면 host.toml `[tui] theme`에 쓰고 닫는다(실험실 설정의 다른 줄은 그대로)
    tui.cmd("s");
    tui.wait_text("설정 창", |s| s.contains("▸ catppuccin ✓"));
    tui.send(b"jj");
    tui.wait_text("terminal 고름", |s| s.contains("▸ terminal"));
    tui.send(b"\r");
    tui.wait_screen("적용", |s| !s.text().contains(" ↑↓ 고르기") && tab_bg(s) == 0);
    let saved = host_toml();
    assert!(saved.contains("theme = \"terminal\"") && saved.contains("prefix_jamo_keys = true"), "{saved}");

    // 3) 상태 표시 구역: 기호를 고르면 쓰고 창은 그대로, 확인 대기 pane 점이 `×`
    tui.cmd("s");
    tui.wait_text("설정 창", |s| s.contains("▸ terminal ✓"));
    tui.send(b"\t");
    tui.wait_text("상태 표시 구역", |s| s.contains("▸ 색 점"));
    tui.send(b"j\r");
    tui.wait_screen("기호", |s| s.text().contains("▸ 기호") && asking_dot(s, "×"));
    assert!(host_toml().contains("status_indicators = \"symbols\""), "{}", host_toml());
    tui.send(b"\x1b");
    tui.wait_text("닫힘", |s| !s.contains(" ↑↓ 고르기"));

    // 4) spaces 맨 아랫줄 `menu`: 전체 메뉴(설정·단축키·설정 다시 읽기·나가기), 첫 항목 Enter면 설정 창
    let at = tui.wait_find("menu");
    tui.click(at);
    tui.wait_text("전체 메뉴", |s| s.contains("설정 다시 읽기") && s.contains("나가기"));
    tui.send(b"\r");
    tui.wait_text("메뉴에서 설정 창", |s| s.contains(" ↑↓ 고르기") && !s.contains("설정 다시 읽기"));
    tui.send(b"\x1b");
    tui.wait_text("닫힘", |s| !s.contains(" ↑↓ 고르기"));

    // 5) host.local.toml이 theme를 덮으면 써도 바뀌지 않았다고 알린다
    std::fs::write(h.cfg.join("host.local.toml"), "schema = 1\n[tui]\ntheme = \"nord\"\n").unwrap();
    tui.cmd("s");
    tui.wait_text("설정 창", |s| s.contains("▸ terminal ✓"));
    tui.send(b"j\r");
    tui.wait_text("덮음 알림", |s| s.contains("host.local.toml의 [tui] theme가 덮고 있어"));
    assert!(host_toml().contains("theme = \"tokyo-night\""), "{}", host_toml());
    drop(tui);
}

/// 기록을 편집기로(herdr 0.9.5 `prefix e` `edit_scrollback`, plan §8.176): 그 pane 머신 agent가 기록을 임시 파일(0600)로 쓰고,
/// host가 그 pane을 나눠 편집기 pane을 띄워 탭을 zoom한다. 편집기 pane이 끝나면(끝남·닫기) 그 pane을 닫고 파일을 지우고, 끝날
/// 때도 zoom이면 나누기 전 zoom으로 돌리고, 연 pane으로 포커스를 돌린다. 편집기는 agent `EDITOR`(받은 경로를 적고 내용을 보인
/// 뒤 Enter를 기다리는 스크립트), 임시 폴더는 agent `TMPDIR`로 실험실 안에 둔다.
#[test]
fn edit_scrollback_like_herdr() {
    use std::os::unix::fs::PermissionsExt;
    let mut lab = Lab::new();
    let tmp = lab.root.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let seen = lab.root.join("editor-path.txt");
    let editor = lab.root.join("editor.sh");
    let script = format!("#!/bin/sh\necho \"$1\" > '{}'\ncat \"$1\"\necho editor-ready\nread x\n", seen.display());
    std::fs::write(&editor, script).unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let env = [("EDITOR", editor.to_str().unwrap()), ("TMPDIR", tmp.to_str().unwrap())];
    let (h, mut hw) = start_agent_env(&mut lab, LOCAL_ONLY, &env);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| tab_chips(s) == ["1"]);
    hw.wait("이 화면 크기", |w| w.held_by(&first, "tui@").then_some(()));
    tui.type_line("seq 1 200; echo seq-$((1+1))");
    tui.wait_text("seq 끝", |s| s.contains("seq-2"));
    let zoomed = |w: &HostWatch| w.ws.as_ref().is_some_and(|ws| ws.tabs[0].zoomed);
    // 편집기를 열고 그 pane과 편집기가 받은 경로를 돌려준다
    let open = |tui: &mut Tui, hw: &mut HostWatch, n: usize| {
        let _ = std::fs::remove_file(&seen);
        tui.cmd("e");
        let ed = hw.wait("편집기 pane", |w| (w.leaves().len() == n + 1 && zoomed(w)).then(|| w.leaves()[n].clone()));
        tui.wait_text("편집기 화면", |s| {
            tab_chips(s) == ["1", "Z"] && s.contains("기록을 편집기로 열었다") && s.contains("editor-ready")
        });
        let path = wait_for("편집기가 받은 경로", T, || {
            std::fs::read_to_string(&seen).ok().filter(|p| p.ends_with('\n')).map(|p| p.trim().to_string())
        });
        (ed, std::path::PathBuf::from(path))
    };

    // 1) prefix e: 나눠 편집기 pane, 탭 zoom(` Z`), 포커스는 편집기. 파일은 실험실 임시 폴더의 `aaa-scrollback-*.txt`(0600)이고
    //    기록 처음(1)부터 끝(200)까지 들었다. Enter로 편집기가 끝나면 그 pane을 닫고(나누기 전처럼 zoom 아님) 연 pane으로 돌아간다
    let (_, path) = open(&mut tui, &mut hw, 1);
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    assert!(path.starts_with(&tmp) && name.starts_with("aaa-scrollback-"), "{}", path.display());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.lines().any(|l| l == "1") && text.lines().any(|l| l == "200"), "기록 전체가 아니다:\n{text}");
    assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    tui.send(b"\r");
    hw.wait("편집기 pane 닫힘", |w| (w.leaves() == [first.clone()] && !zoomed(w)).then_some(()));
    tui.wait_text("돌아옴", |s| tab_chips(s) == ["1"] && s.contains("seq-2"));
    wait_for("임시 파일 지움", T, || (!path.exists()).then_some(()));
    tui.type_line("echo back-$((2+3))");
    tui.wait_text("연 pane 출력", |s| s.contains("back-5"));
    assert!(h.screen(&first).contains("back-5"), "포커스가 연 pane으로 돌아오지 않았다");

    // 2) 편집기 pane을 닫으면(prefix x, 세션을 끝낸다) 셸이 지우지 못한 파일을 host가 지운다
    let (_, path) = open(&mut tui, &mut hw, 1);
    tui.cmd("x");
    hw.wait("닫은 편집기 pane", |w| (w.leaves() == [first.clone()] && !zoomed(w)).then_some(()));
    wait_for("닫은 편집기의 임시 파일 지움", T, || (!path.exists()).then_some(()));
    // 다음 키는 TUI가 닫힘을 받은 뒤에(host 사건은 이 관측기에 먼저 올 수 있다: 그 전 키는 닫히는 편집기 pane으로 간다)
    tui.wait_text("닫은 뒤 화면", |s| tab_chips(s) == ["1"] && !s.contains("editor-ready"));

    // 3) 나누기 전 탭이 zoom이었으면 끝날 때 다시 zoom, 포커스는 연 pane(나눈 오른쪽)
    tui.cmd("v");
    let right = hw.wait("나눔", |w| (w.leaves().len() == 2).then(|| w.leaves()[1].clone()));
    tui.cmd("z");
    hw.wait("zoom", |w| zoomed(w).then_some(()));
    let (ed, _) = open(&mut tui, &mut hw, 2);
    tui.send(b"\r");
    hw.wait("편집기 pane 닫힘, 다시 zoom", |w| {
        (!w.leaves().contains(&ed) && w.leaves().len() == 2 && zoomed(w)).then_some(())
    });
    tui.wait_text("닫은 뒤 화면", |s| tab_chips(s) == ["1", "Z"] && !s.contains("editor-ready"));
    tui.type_line("echo right-$((4+5))");
    tui.wait_text("오른쪽 zoom 출력", |s| tab_chips(s) == ["1", "Z"] && s.contains("right-9"));
    assert!(h.screen(&right).contains("right-9"), "포커스가 연 pane으로 돌아오지 않았다");

    // 4) 편집기를 보는 사이 사람이 zoom을 풀었으면 그대로 둔다(host는 빼기와 zoom을 한 사건으로 저장한다: 빠진 사건이 끝 상태)
    let (ed, _) = open(&mut tui, &mut hw, 2);
    tui.cmd("z");
    hw.wait("zoom 풀림", |w| (!zoomed(w)).then_some(()));
    tui.send(b"\r");
    hw.wait("편집기 pane 닫힘", |w| (!w.leaves().contains(&ed) && w.leaves().len() == 2).then_some(()));
    assert!(!zoomed(&hw), "사람이 푼 zoom을 다시 켰다");

    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// pane 폭만 바뀌어도 그 pane을 전부 다시 그린다(QA t-0337 Q23, herdr는 크기가 바뀐 pane을 통째로 그린다).
/// 사이드바를 접고 펴면 줄 수는 그대로이고 폭만 바뀐다. 전에는 host가 바뀐 줄만 보내고 TUI는 폭이 바뀐 grid를 비워서,
/// 내용이 같은 줄(앞 출력)이 빈칸으로 남았다. 바뀐 뒤 친 표지 줄이 보인 다음(그 뒤 장면까지 왔다) 앞 출력을 본다.
#[test]
fn width_only_change_redraws_the_pane() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "wide", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    hw.wait("lease", |w| w.held_by(&id, "tui@").then_some(()));
    tui.type_line("echo before-$((1)); echo again-$((2))");
    tui.wait_text("앞 출력", |s| s.contains("again-2"));

    let fold = tui.wait_find("«");
    tui.click(fold);
    hw.wait("compact 폭", |w| (w.pane(&id)?.cols == COLS - COMPACT - GUTTER).then_some(()));
    tui.type_line("echo folded-$((3))");
    tui.wait_text("접은 뒤 표지", |s| s.contains("folded-3"));
    let text = tui.text();
    assert!(text.contains("before-1") && text.contains("again-2"), "폭이 넓어진 pane의 앞 출력이 비었다:\n{text}");

    let unfold = tui.wait_find("»");
    tui.click(unfold);
    hw.wait("펼친 폭", |w| (w.pane(&id)?.cols == COLS - SIDE - GUTTER).then_some(()));
    tui.type_line("echo unfolded-$((4))");
    tui.wait_text("편 뒤 표지", |s| s.contains("unfolded-4"));
    let text = tui.text();
    assert!(text.contains("before-1") && text.contains("folded-3"), "폭이 좁아진 pane의 앞 출력이 비었다:\n{text}");
    tui.cmd("q");
    assert!(tui.wait_exit());
}

/// Ctrl+V 그림 붙여넣기(§8.28): 그림이 없으면 0x16이 그대로 가고, 있으면 pane 머신 세션 폴더에 PNG로 저장된다.
/// 셸 pane에도 따옴표 경로와 빈칸을 넣는다(Enter 없음, herdr·herdr-web-ui처럼, plan §8.51). 같은 길을 `aaa pane attach`(raw ssh 셸용)로도 확인한다.
/// 기본은 `AAA_TEST_CLIPBOARD_PNG` 파일을 clipboard로 쓴다(개발 Mac의 clipboard를 덮어쓰지 않는다).
/// `AAA_CLIP_NATIVE=1`(CI)이면 실제 시스템 clipboard에 쓰고 TUI가 그것을 읽는다.
#[test]
fn ctrl_v_pastes_clipboard_image() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "pic", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let native = std::env::var_os("AAA_CLIP_NATIVE").is_some();
    let rgba: Vec<u8> = PIXELS.concat();
    let fake = lab.root.join("clip.png");
    let fake_s = fake.to_str().unwrap().to_string();
    let mut clip = native.then(|| arboard::Clipboard::new().expect("clipboard를 열지 못했다"));

    let mut tui = if native { Tui::start(&h) } else { Tui::start_env(&h, &[("AAA_TEST_CLIPBOARD_PNG", &fake_s)]) };
    hw.wait("lease", |w| w.held_by(&id, "tui@").then_some(()));

    // 1) 그림이 없으면 Ctrl+V는 원래 키다(lnext를 끈 cat -v가 ^V를 보인다)
    if let Some(c) = clip.as_mut() {
        c.set_text("no image here").unwrap();
    }
    // stty가 끝난 뒤 보낸다: 그 전에 tty에 닿은 0x16은 lnext로 먹히고 CR이 글자 ^M으로 들어간다(부하 아래 ci-local)
    tui.type_line("stty -iexten; echo lnext-$((0)); cat -v");
    tui.wait_text("stty 끝", |s| s.contains("lnext-0"));
    tui.send(b"\x16");
    tui.send(b"\r");
    // ^V는 tty echo(ECHOCTL)와 cat -v 출력 둘이다. cat이 줄을 읽어 낸 둘째를 본 뒤 ^C를 보낸다: 그때 cat이 앞 그룹이다
    // (뒤 그룹이 tty를 읽으면 멈춘다). echo만 보고 보내면 셸이 cat을 앞으로 올리기 전의 ^C가 셸로 가고 읽지 않은 줄을
    // 버려, 살아남은 cat이 다음 줄을 먹었다(ci-local 부하 아래 「화면 지움」 실패)
    tui.wait_text("원래 Ctrl+V", |s| s.matches("^V").count() >= 2);
    tui.send(b"\x03");
    assert!(attachments(&h.state).is_empty(), "그림이 없는데 첨부가 생겼다");

    // 2) 그림이 있으면 대상 머신에 저장하고 셸 줄에 따옴표 경로를 넣고(Enter 없음) 오른쪽 위 알림 카드로 알린다.
    match clip.as_mut() {
        Some(c) => c.set_image(arboard::ImageData { width: 3, height: 2, bytes: rgba.as_slice().into() }).unwrap(),
        None => std::fs::write(&fake, encode_rgba(3, 2, &rgba)).unwrap(),
    }
    tui.type_line("clear; echo cleared-$((2+2))");
    tui.wait_text("화면 지움", |s| s.contains("cleared-4"));
    tui.send(b"\x16");
    tui.wait_text("경로 표시", |s| s.contains("그림 경로를 넣었다"));
    let files = attachments(&h.state);
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].extension().is_some_and(|e| e == "png"), "{files:?}");
    assert_eq!(decode_rgba(&std::fs::read(&files[0]).unwrap()), (3, 2, rgba.clone()), "대상 머신 그림의 pixel");
    let line: String = h.screen(&id).replace('\n', "");
    let quoted = format!("'{}' ", files[0].display());
    assert!(line.contains(quoted.trim_end()), "셸 줄에 따옴표 경로가 없다: {line}");
    // Enter가 없었으니 셸은 실행하지 않았다. 그 줄을 지운다.
    assert!(!line.contains("Permission denied") && !line.contains("not found"), "{line}");
    tui.send(b"\x15");
    drop(clip);

    // 3) 여러 조각(1.2MB)을 `aaa pane attach`로 올린다. TUI가 쓰는 중이어도 되고, 한 번 보내는 CLI라
    //    지금 쓰는 화면은 TUI 그대로다.
    let big = lab.root.join("big.png");
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.resize(1_200_000, 7);
    std::fs::write(&big, &bytes).unwrap();
    let r = h.ok(&["pane", "attach", &id, big.to_str().unwrap()]);
    assert_eq!(h.pane(&id)["lease"]["label"].as_str().map(|l| l.starts_with("tui@")), Some(true));
    tui.cmd("q");
    assert!(tui.wait_exit());
    drop(tui);
    let path = r["path"].as_str().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes, "{r}");
    assert_eq!(r["inserted"], true, "{r}");
    // 그림이 아니면 거부한다
    let txt = lab.root.join("note.txt");
    std::fs::write(&txt, "hello").unwrap();
    let r: serde_json::Value =
        serde_json::from_str(&h.json(&["pane", "attach", &id, txt.to_str().unwrap()]).unwrap_err()).unwrap();
    assert!(r["error"]["message"].as_str().is_some_and(|m| m.contains("PNG·JPEG·GIF·WebP")), "{r}");
    assert_eq!(attachments(&h.state).len(), 2);
}

/// 붙여넣기와 그림 파일 끌어놓기(차이 목록 4E, herdr client `EnableBracketedPaste`·`read_image_file_from_terminal_drop`,
/// plan §8.147): TUI는 바깥 터미널의 bracketed paste(2004)를 켜고 나갈 때 끈다. 붙여 넣은 글은 host가 pane 프로그램이 켠
/// 2004대로 감싸고 글 안의 ESC는 뺀다(herdr 서버가 client 붙여넣기를 다시 감싸는 것과 같다). 붙여 넣은 글이 그림 파일 하나의
/// 절대 경로면(터미널에 끌어 놓은 파일) 그 그림을 pane 머신에 올리고 셸 줄에 따옴표 경로를 넣는다(Enter 없음).
/// 그림이 아닌 파일·상대 경로는 글 그대로다. 붙여 넣은 줄마다 cat이 되돌린 것을 본 뒤 단언하고 다음 글을 친다.
#[test]
fn paste_and_image_drop_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "drop", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    hw.wait("lease", |w| w.held_by(&id, "tui@").then_some(()));
    tui.wait_screen("바깥 터미널 bracketed paste", |s| s.modes.contains(&2004));
    // 붙여 넣은 줄을 cat -v가 되돌릴 때까지(tty 메아리와 cat 출력 둘). 그 전에 다음 글을 치면 그 메아리 사이에 cat 출력이
    // 끼어든다(부하 아래 ci-local). `seen`은 감싸기·ESC 빼기가 맞든 틀리든 둘 다에 나오는 조각이라 틀리면 아래 단언이 실패한다
    let echoed = |tui: &Tui, seen: &str| tui.wait_text(seen, |s| s.matches(seen).count() >= 2);

    // 1) 2004를 켜지 않은 프로그램: 감싸지 않고, 글 안의 ESC는 뺀다(cat -v에 `^[`가 없다)
    tui.type_line("echo plain-$((1)); cat -v");
    tui.wait_text("cat", |s| s.contains("plain-1"));
    tui.send(b"\x1b[200~one\x1b[31mtwo\x1b[201~");
    tui.send(b"\r");
    echoed(&tui, "two");
    let text = tui.text();
    assert!(text.contains("one[31mtwo") && !text.contains("^["), "2004 없는 붙여넣기:\n{text}");

    // 2) 그림이 아닌 절대 경로·상대 그림 경로·없는 그림 파일은 글 그대로다
    let note = lab.root.join("note.txt");
    std::fs::write(&note, "hello").unwrap();
    let missing = lab.root.join("missing.png");
    let pastes = [note.display().to_string(), "shot.png".to_string(), missing.display().to_string()];
    for (i, p) in pastes.iter().enumerate() {
        tui.send(format!("\x1b[200~{p}\x1b[201~").as_bytes());
        tui.send(b"\r");
        echoed(&tui, p);
        assert_eq!(tui.text().matches(p.as_str()).count(), 2, "글로 붙인 {p}:\n{}", tui.text());
        // ^C는 cat이 그 줄을 되돌린 뒤에만 보낸다: 셸이 `clear`와 `cat` 사이일 때 닿으면 cat이 남아 다음 명령 줄을 읽는다
        tui.send(b"\x03");
        if i + 1 < pastes.len() {
            // 다음 붙여넣기는 cat이 뜨기 전에 와도 입력으로 기다린다
            tui.type_line("clear; cat -v");
            tui.wait_text("화면 지움", |s| !s.contains(p.as_str()));
        }
    }
    assert!(attachments(&h.state).is_empty(), "그림이 아닌 붙여넣기에 첨부가 생겼다");

    // 3) 프로그램이 2004를 켜면 host가 감싸고 글 안의 ESC는 뺀다(글 안의 `ESC[201~`가 붙여넣기를 일찍 끝내지 못한다).
    //    `ESC[201~` 자체는 바깥 터미널 붙여넣기의 끝이라 TUI까지 오지 않으므로 다른 CSI로 본다
    tui.type_line("clear; printf '\\033[?2004h'; echo wrap-$((2)); cat -v");
    tui.wait_text("cat", |s| s.contains("wrap-2"));
    tui.send(b"\x1b[200~x\x1b[31my\x1b[201~");
    tui.send(b"\r");
    echoed(&tui, "31my");
    let text = tui.text();
    assert_eq!(text.matches("^[[200~x[31my^[[201~").count(), 2, "감싼 붙여넣기:\n{text}");
    tui.send(b"\x03");
    tui.type_line("printf '\\033[?2004l'; clear; echo drop-$((3))");
    tui.wait_text("화면 지움", |s| s.contains("drop-3") && !s.contains("wrap-2"));

    // 4) 그림 파일 하나를 끌어 놓으면(터미널은 빈칸을 `\ `로 escape한 경로를 붙여 넣는다) 대상 머신에 올리고 셸 줄에
    //    따옴표 경로를 넣는다(Enter 없음)
    let rgba: Vec<u8> = PIXELS.concat();
    let shot = lab.root.join("drop shot.png");
    std::fs::write(&shot, encode_rgba(3, 2, &rgba)).unwrap();
    let dropped = shot.display().to_string().replace(' ', "\\ ");
    tui.send(format!("\x1b[200~{dropped}\x1b[201~").as_bytes());
    tui.wait_text("경로 표시", |s| s.contains("그림 경로를 넣었다"));
    let files = attachments(&h.state);
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(decode_rgba(&std::fs::read(&files[0]).unwrap()), (3, 2, rgba), "대상 머신 그림의 pixel");
    let line: String = h.screen(&id).replace('\n', "");
    assert!(line.contains(&format!("'{}'", files[0].display())), "셸 줄에 따옴표 경로가 없다: {line}");
    assert!(!line.contains(&dropped) && !line.contains("not found"), "{line}");
    tui.send(b"\x15");

    // 5) 창과 모드(herdr `insert_overlay_text`): prefix 모드의 붙여넣기는 버리고(pane에 가지 않는다), 검색 중인 단축키 창은
    //    검색어에, 새 탭 이름 창은 이름에 넣는다
    tui.send(b"\x02");
    tui.wait_text("PREFIX 막대", |s| s.contains(" PREFIX "));
    tui.send(b"\x1b[200~zz-prefix\x1b[201~");
    tui.send(b"\x1b");
    tui.wait_text("prefix 끝", |s| !s.contains(" PREFIX "));
    tui.type_line("echo end-$((5))");
    tui.wait_text("표지", |s| s.contains("end-5"));
    assert!(!h.screen(&id).contains("zz-prefix"), "prefix 모드의 붙여넣기가 pane에 갔다:\n{}", h.screen(&id));
    tui.cmd("?");
    tui.wait_text("단축키 창", |s| s.contains("prefix 모드") && s.contains("esc 닫기"));
    tui.send(b"/");
    tui.wait_text("검색", |s| s.contains("esc 돌아가기"));
    tui.send("\x1b[200~나누기\x1b[201~".as_bytes());
    tui.wait_text("거름", |s| s.contains("좌우로 나누기") && !s.contains("목록 탐색"));
    tui.send(b"\x1b");
    tui.wait_text("검색 끝", |s| s.contains("esc 닫기"));
    tui.send(b"\x1b");
    tui.wait_text("닫힘", |s| !s.contains("prefix 모드"));
    tui.cmd("c");
    tui.wait_text("새 탭 이름 창", |s| s.contains("새 탭") && s.contains("↵ 저장"));
    tui.send("\x1b[200~붙인\n탭\x1b[201~\r".as_bytes());
    hw.wait("붙여 넣은 이름의 탭", |w| w.ws.as_ref()?.tabs.iter().any(|t| t.name == "붙인 탭").then_some(()));

    // 6) 나가면(prefix q) bracketed paste를 끈다
    tui.cmd("q");
    assert!(tui.wait_exit());
    tui.wait_screen("bracketed paste 끔", |s| !s.modes.contains(&2004));
}
