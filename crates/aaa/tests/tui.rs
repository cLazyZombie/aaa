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
/// 펼친 사이드바 폭(구분선 포함, herdr `sidebar_width` 26)과 접힌 compact 폭(4)
const SIDE: u16 = 26;
const COMPACT: u16 = 4;
/// herdr catppuccin 팔레트의 탐색 커서 줄 바탕(`selection_bg`)과 지금 pane 줄 바탕(`active_row_bg`)
const SELECTION_BG: u32 = 0x0200_0000 | (49 << 16) | (50 << 8) | 68;
const ACTIVE_BG: u32 = 0x0200_0000 | (30 << 16) | (30 << 8) | 46;

/// 시험 설정: 사용자 입력기는 건드리지 않고(입력기 전환은 기본 켬이라 끈다), prefix 뒤 자모 해석은 켠다(기본값)
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

    /// 사이드바 칸(왼쪽 SIDE칸) 중 바탕이 `bg`인 첫 줄: (줄 번호, 그 줄의 사이드바 글)
    fn side_row(&self, bg: u32) -> Option<(usize, String)> {
        let w = (SIDE as usize).min(self.cols);
        (0..self.cells.len())
            .find(|r| self.bgs[*r][..w - 1].contains(&bg))
            .map(|r| (r, self.cells[r][..w].iter().filter(|c| **c != '\0').collect()))
    }

    /// 탐색 모드에서 고른 줄(herdr `selection_bg`)
    fn selected(&self) -> Option<(usize, String)> {
        self.side_row(SELECTION_BG)
    }

    /// 지금 pane 줄(herdr `active_row_bg`)
    fn current(&self) -> Option<String> {
        self.side_row(ACTIVE_BG).map(|(_, l)| l)
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
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows: ROWS, cols, pixel_width: 0, pixel_height: 0 }).unwrap();
        let mut cmd = CommandBuilder::new(BIN);
        cmd.arg("attach");
        cmd.env("AAA_CONFIG_DIR", &node.cfg);
        cmd.env("AAA_STATE_DIR", &node.state);
        cmd.env("TERM", "xterm-256color");
        cmd.env("RUST_LOG", "off");
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
        HostWatch { rt, rx, ws: None, panes: vec![], machines: vec![] }
    }

    fn apply(&mut self, m: FrontMsg) {
        match m {
            FrontMsg::Event(FrontEvent::Workspace(w)) if w.name == aaa_proto::DEFAULT_WORKSPACE => self.ws = Some(w),
            FrontMsg::Event(FrontEvent::State { machines, panes }) => {
                self.machines = machines;
                self.panes = panes;
            }
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
    let h = lab.node("h", Some(host_toml));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    let mut hw = HostWatch::new(&h);
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "labA"]);
    hw.wait("labA 연결", |w| w.connected("labA").then_some(()));
    (h, hw)
}

/// 탭 막대의 이름표들(herdr: 붙인 이름 또는 자리 번호). 맨 윗줄에서 사이드바 구분선 뒤, 오른쪽 조각(질문 수·다른 화면 크기) 앞.
/// 시험의 탭 이름에는 빈칸을 쓰지 않는다.
fn tab_chips(text: &str) -> Vec<String> {
    let first = text.lines().next().unwrap_or("");
    let rest = first.split_once('│').map_or("", |(_, r)| r);
    let rest = rest.split(" ● 질문").next().unwrap_or(rest);
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
    // 탭 이름표는 herdr처럼 붙인 이름 또는 자리 번호다. 사이드바에 머신 줄과 pane 줄(셸은 상태 점 `·`)이 있고
    // 지금 pane 줄은 active_row_bg 바탕이다(herdr 사이드바).
    tui.wait_screen("탭과 사이드바", |s| {
        let t = s.text();
        tab_chips(&t) == ["first"] && t.contains(" ▾ labA") && s.current().is_some_and(|l| l.contains(" · first"))
    });
    // 처음 포커스한 pane은 아무도 쓰지 않으니 칸 크기(사이드바 26칸과 탭 막대 한 줄을 뺀 칸)로 맞추고
    // 이 화면이 지금 쓰는 화면이 된다. 상태 줄은 없다(herdr).
    hw.wait("칸 크기와 lease", |w| {
        let p = w.pane(&first)?;
        ((p.cols, p.rows) == (COLS - SIDE, ROWS - 1) && w.held_by(&first, "tui@")).then_some(())
    });

    tui.type_line("echo tui-$((6*7))");
    tui.wait_text("입력 결과", |s| s.contains("tui-42"));

    // harness 상태는 화면으로 바뀌고 사이드바 줄·탭 막대 질문 수·pane badge에 보인다: Claude 권한 확인창
    // (`bash_permission_prompt`)은 질문, spinner 줄(`live_turn_working`)은 작업 중이다
    tui.type_line(
        "clear; printf '%s\\n' ' Bash command' '' '   ls' '' ' Do you want to proceed?' ' ❯ 1. Yes' '   2. No' '' \
         ' Esc to cancel · Tab to amend'",
    );
    tui.wait_text("질문 상태", |s| {
        s.lines().next().is_some_and(|l| l.contains(" ● 질문 1 ")) && s.contains("질문 — aaa question")
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

/// herdr 키와 모드(herdr 0.9.5 `KeysConfig::default`·`route_key`): prefix는 시간 제한 없이 다음 키 하나를 기다리고
/// Esc·모르는 키는 아무것도 보내지 않고 취소한다. prefix 두 번은 prefix를 보낸다. v·- 나누기, h/j/k/l·Tab·Shift+Tab 포커스,
/// x 닫기, o 질문 pane으로, ? 단축키 창(검색), shift+r 설정 다시 읽기(prefix를 바꿈, 읽지 못하면 ERROR 막대).
#[test]
fn herdr_keys_and_modes() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    // 첫 pane은 실행 파일 이름이 claude인 셸이다: 4)에서 Claude 확인창을 그려 질문을 연다(나눈 pane은 harness가 없다)
    let claude = lab.root.join("bin").join("claude");
    std::fs::create_dir_all(claude.parent().unwrap()).unwrap();
    std::fs::write(&claude, "#!/bin/sh\nexec /bin/sh\n").unwrap();
    std::fs::set_permissions(&claude, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "keys", "--", lab::path_str(&claude)]);
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

    // 4) o: 질문이 열린 pane으로 간다. 질문은 첫 pane 화면의 Claude 권한 확인창(`bash_permission_prompt`)이다.
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
    // o는 TUI가 아는 질문으로 간다: host가 바뀐 것만으로는 TUI가 아직 모를 수 있어 표시가 뜬 것을 본다
    tui.wait_text("질문 표시", |s| s.contains("질문 — aaa question"));
    tui.cmd("o");
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
    assert!(
        h.screen(&right).contains("held-4") && h.screen(&right).contains("작업 폴더"),
        "새 pane이 옮긴 폴더에서 뜨지 않았다:\n{}",
        h.screen(&right)
    );
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
        (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE).then_some(())
    });
    tui.wait_text("다른 화면 표시가 사라짐", |s| !s.contains("other-front 크기"));

    // 2) 다른 화면이 다시 쓰면 그 크기다. 이 터미널이 focus를 다시 얻으면(ESC [ I) 크기를 다시 건다
    other_types(&mut other, "echo again");
    hw.wait("다른 화면", |w| w.held_by(&id, "other-front").then_some(()));
    tui.wait_text("다른 front 표시", |s| s.contains("other-front 크기"));
    tui.send(b"\x1b[I");
    hw.wait("focus 얻은 화면의 크기", |w| {
        (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE).then_some(())
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
    hw.wait("이 화면의 크기", |w| (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE).then_some(()));
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
    hw.wait("되찾은 크기", |w| (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS - SIDE).then_some(()));
    tui.wait_text("줄 끝이 돌아옴", |s| s.contains(&long) && !s.contains("phone-front 크기"));
    // 세션 쪽 화면(`pane read`)도 다시 접었다
    let read = h.ok(&["pane", "read", &id]);
    assert!(read.to_string().contains(&long), "pane read: {read}");
    drop(tui);
}

/// 고른 줄 바로 위가 「Thread n」 머리인가(목록 맨 위)
fn at_top(s: &Screen) -> bool {
    let text = s.text();
    let lines: Vec<&str> = text.lines().collect();
    s.selected().is_some_and(|(i, _)| i > 0 && lines[i - 1].contains("Thread "))
}

/// 탐색 모드에서 목록 맨 위(첫 thread)로 올라간 뒤 `needle` 줄까지 내려간다(herdr 탐색은 ↑↓, 고른 줄은 selection_bg 바탕).
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

/// 왼쪽 사이드바(§8.35, herdr 배치): 웹 목록처럼 Thread 묶음·머신별 pane·새 셸이 보이고, 새 thread를 따라가며, 고르면 탭으로 연다.
/// herdr처럼 폭과 관계없이 펼친 채(26칸) 시작한다. herdr 키: prefix b 펴기·접기(접으면 4칸 compact, 포커스는 그대로),
/// prefix w 탐색 모드(↑↓ 고르기·Enter 열기·Esc 돌아가기, 접혀 있으면 펼친 목록을 pane 위에 띄워 보인다).
#[test]
fn side_list_like_web() {
    let mut lab = Lab::new();
    let projects = lab.root.join("projects");
    let host = format!("{LOCAL_ONLY}[projects]\nroot = \"{}\"\nallow_profiles = [\"cat\"]\n", lab::path_str(&projects));
    let h = lab.node("h", Some(&host));
    std::fs::write(
        h.cfg.join("agent.toml"),
        format!(
            "schema = 1\n[agent]\nwork_root = \"{}\"\n[profiles.cat]\nharness = \"custom\"\ncommand = [\"/bin/cat\"]\n",
            lab::path_str(&lab.root.join("work"))
        ),
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
    let th_pane = wait_for("thread pane", T, || {
        h.ok(&["thread", "show", "demo", th["id"].as_str().unwrap()])["pane"].as_str().map(String::from)
    });

    // 1) 펼친 사이드바, pane 칸은 사이드바(구분선 포함 26칸)를 뺀 폭
    let mut tui = Tui::start_cols(&h, &[], WIDE);
    tui.wait_text("목록", |s| {
        s.contains("Thread 1")
            && s.contains("alpha-job")
            && s.contains(" ▾ labA")
            && s.contains(" + 새 셸")
            && s.contains("first")
            && s.contains("loose")
    });
    hw.wait("목록 옆 크기", |w| (w.pane(&first)?.cols == WIDE - SIDE).then_some(()));
    tui.type_line("echo list-$((6*7))");
    tui.wait_text("pane 입력", |s| s.contains("list-42"));

    // 2) 새 thread는 사건으로 따라온다
    start("beta-job");
    tui.wait_text("새 thread", |s| s.contains("Thread 2") && s.contains("beta-job"));
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

    // 6) 새 셸은 그 머신에 새 탭으로
    tui.cmd("w");
    pick(&mut tui, "+ 새 셸");
    tui.send(b"\r");
    hw.wait("새 셸 탭", |w| (w.leaves().len() == 4).then_some(()));

    // 7) Esc는 탐색을 끝낸다
    tui.cmd("w");
    tui.wait_text("탐색 모드", |s| s.contains(" NAVIGATE "));
    tui.send(b"\x1b");
    tui.wait_text("Esc", |s| !s.contains(" NAVIGATE "));

    // 8) prefix b로 접으면 4칸 compact 열(번호·상태 점, herdr `render_collapsed_sidebar`)만 남는다
    tui.cmd("1");
    tui.wait_text("첫 탭", |s| s.contains("list-42"));
    tui.cmd("b");
    tui.wait_text("접힘", |s| !s.contains(" ▾ labA") && s.lines().next().is_some_and(|l| l.starts_with("1 ")));
    hw.wait("compact 옆 폭", |w| (w.pane(&first)?.cols == WIDE - COMPACT).then_some(()));
    tui.cmd("q");
    assert!(tui.wait_exit());
    drop(tui);

    // 9) 새로 붙은 TUI는 좁아도(90칸) 펼친 채 시작한다(herdr는 폭으로 접지 않는다, `sidebar_start_collapsed`로 정한다).
    //    좌우로 나눠 두 pane에 글을 둔 채 탐색 모드를 켜도 두 pane 글이 그대로 보인다(coordinator 실측 「w 탐색 때
    //    오른쪽 pane이 비어 보였다」 재현 시도). 접고 탐색하면 펼친 목록을 pane 위에 띄우고(pane 크기는 그대로),
    //    prefix b로 펴면 사이드바 옆 크기가 된다(포커스는 pane 그대로: 바로 입력된다)
    let mut tui = Tui::start(&h);
    tui.wait_text("펼친 채 시작", |s| {
        s.contains("list-42") && s.contains(" ▾ labA") && tab_chips(s).first().is_some_and(|c| c == "first")
    });
    hw.wait("사이드바 옆 폭", |w| (w.pane(&first)?.cols == COLS - SIDE).then_some(()));
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
    tui.wait_text("접힘", |s| !s.contains(" ▾ labA"));
    // 나눈 탭은 pane마다 테두리 상자라(herdr `pane_borders = "auto"`) 두 pane 폭 합은 칸 폭에서 4칸 작다
    hw.wait("compact 옆 폭", |w| (w.pane(&right)?.cols + w.pane(&first)?.cols + 4 == COLS - COMPACT).then_some(()));
    let cols = hw.pane(&first).unwrap().cols;
    tui.cmd("w");
    tui.wait_screen("띄운 목록", |s| {
        let t = s.text();
        t.contains(" NAVIGATE ") && t.contains(" ▾ labA") && s.selected().is_some() && t.contains("right-side-16")
    });
    tui.send(b"\x1b");
    tui.wait_text("목록 닫힘", |s| !s.contains(" ▾ labA"));
    assert_eq!(hw.pane(&first).unwrap().cols, cols, "탐색 모드가 pane 크기를 바꿨다");
    tui.cmd("b");
    tui.wait_text("펼침", |s| s.contains(" ▾ labA") && !s.contains(" NAVIGATE "));
    hw.wait("사이드바 옆 크기", |w| {
        (w.pane(&right)?.cols + w.pane(&first)?.cols + 4 == COLS - SIDE).then_some(())
    });
    tui.type_line("echo still-$((3*3))");
    tui.wait_text("포커스 그대로", |s| s.contains("still-9"));
    assert!(h.screen(&right).contains("still-9"), "포커스는 나눈 오른쪽 pane 그대로");
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
/// 사이드바 줄·`«`/`»`·경계 끌기(18~36), pane 누르기 포커스, 끌어 고르기·두 번 누름 낱말 복사, 나가면 보고를 끈다.
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
    hw.wait("첫 탭 크기", |w| (w.pane(&one)?.cols == COLS - SIDE).then_some(()));
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
    hw.wait("compact 폭", |w| (w.pane(&two)?.cols == COLS - COMPACT).then_some(()));
    let unfold = tui.wait_find("»");
    tui.click(unfold);
    hw.wait("펼친 폭", |w| (w.pane(&two)?.cols == COLS - SIDE).then_some(()));

    // 7) 경계(사이드바 마지막 열)를 끌면 폭 = 열 + 1, 18~36으로 묶는다(herdr)
    let drag = |tui: &mut Tui, from: usize, to: usize| {
        tui.mouse(0, (10, from), false);
        tui.mouse(32, (10, to), false);
        tui.mouse(0, (10, to), true);
    };
    drag(&mut tui, SIDE as usize - 1, 31);
    hw.wait("32칸 사이드바", |w| (w.pane(&two)?.cols == COLS - 32).then_some(()));
    drag(&mut tui, 31, 60);
    hw.wait("36칸에서 멈춤", |w| (w.pane(&two)?.cols == COLS - 36).then_some(()));
    drag(&mut tui, 35, SIDE as usize - 1);
    hw.wait("26칸으로", |w| (w.pane(&two)?.cols == COLS - SIDE).then_some(()));

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
    hw.wait("이 화면 크기", |w| (w.pane(&prog)?.cols == COLS - SIDE).then_some(()));
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

    // 3) 나눔 경계(맞닿은 두 테두리 `┐┌`)를 끌면 그 나눔 비율: 64칸 반(32)에서 10칸 오른쪽 → 42칸, 왼쪽 안쪽 40칸
    tui.cmd("v");
    let right = hw.wait("좌우 나눔", |w| w.tab_of(&prog).filter(|l| l.len() == 2).map(|l| l[1].clone()));
    hw.wait("나눈 폭", |w| (w.pane(&prog)?.cols == 30).then_some(()));
    let border = tui.wait_find("┐┌");
    tui.mouse(0, (10, border.1), false);
    tui.mouse(32, (10, border.1 + 10), false);
    tui.mouse(0, (10, border.1 + 10), true);
    hw.wait("끈 만큼 넓어짐", |w| (w.pane(&prog)?.cols == 40).then_some(()));

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
    hw.wait("compact 폭", |w| (w.pane(&id)?.cols == COLS - COMPACT).then_some(()));
    tui.type_line("echo folded-$((3))");
    tui.wait_text("접은 뒤 표지", |s| s.contains("folded-3"));
    let text = tui.text();
    assert!(text.contains("before-1") && text.contains("again-2"), "폭이 넓어진 pane의 앞 출력이 비었다:\n{text}");

    let unfold = tui.wait_find("»");
    tui.click(unfold);
    hw.wait("펼친 폭", |w| (w.pane(&id)?.cols == COLS - SIDE).then_some(()));
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
    tui.wait_text("원래 Ctrl+V", |s| s.contains("^V"));
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
/// plan §8.126): TUI는 바깥 터미널의 bracketed paste(2004)를 켜고 나갈 때 끈다. 붙여 넣은 글은 host가 pane 프로그램이 켠
/// 2004대로 감싸고 글 안의 ESC는 뺀다(herdr 서버가 client 붙여넣기를 다시 감싸는 것과 같다). 붙여 넣은 글이 그림 파일 하나의
/// 절대 경로면(터미널에 끌어 놓은 파일) 그 그림을 pane 머신에 올리고 셸 줄에 따옴표 경로를 넣는다(Enter 없음).
/// 그림이 아닌 파일·상대 경로는 글 그대로다. 각 붙여넣기 뒤에 친 표지 줄을 cat이 되돌린 것을 보고 그 앞 줄을 단언한다.
#[test]
fn paste_and_image_drop_like_herdr() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["tab", "create", "--machine", "labA", "--label", "drop", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    hw.wait("lease", |w| w.held_by(&id, "tui@").then_some(()));
    tui.wait_screen("바깥 터미널 bracketed paste", |s| s.modes.contains(&2004));
    // 표지 줄을 치고 cat -v가 되돌릴 때까지(tty 메아리와 cat 출력 둘)
    let mark = |tui: &mut Tui, m: &str| {
        tui.type_line(m);
        tui.wait_text(m, |s| s.matches(m).count() >= 2);
    };

    // 1) 2004를 켜지 않은 프로그램: 감싸지 않고, 글 안의 ESC는 뺀다(cat -v에 `^[`가 없다)
    tui.type_line("echo plain-$((1)); cat -v");
    tui.wait_text("cat", |s| s.contains("plain-1"));
    tui.send(b"\x1b[200~one\x1b[31mtwo\x1b[201~");
    tui.send(b"\r");
    mark(&mut tui, "end-a");
    let text = tui.text();
    assert!(text.contains("one[31mtwo") && !text.contains("^["), "2004 없는 붙여넣기:\n{text}");

    // 2) 그림이 아닌 절대 경로·상대 그림 경로·없는 그림 파일은 글 그대로다
    let note = lab.root.join("note.txt");
    std::fs::write(&note, "hello").unwrap();
    let missing = lab.root.join("missing.png");
    for p in [note.display().to_string(), "shot.png".to_string(), missing.display().to_string()] {
        tui.send(format!("\x1b[200~{p}\x1b[201~").as_bytes());
        tui.send(b"\r");
        mark(&mut tui, "end-b");
        assert_eq!(tui.text().matches(p.as_str()).count(), 2, "글로 붙인 {p}:\n{}", tui.text());
        tui.send(b"\x03");
        tui.type_line("clear; cat -v");
        tui.wait_text("화면 지움", |s| !s.contains("end-b"));
    }
    assert!(attachments(&h.state).is_empty(), "그림이 아닌 붙여넣기에 첨부가 생겼다");
    tui.send(b"\x03");

    // 3) 프로그램이 2004를 켜면 host가 감싸고 글 안의 ESC는 뺀다(글 안의 `ESC[201~`가 붙여넣기를 일찍 끝내지 못한다).
    //    `ESC[201~` 자체는 바깥 터미널 붙여넣기의 끝이라 TUI까지 오지 않으므로 다른 CSI로 본다
    tui.type_line("clear; printf '\\033[?2004h'; echo wrap-$((2)); cat -v");
    tui.wait_text("cat", |s| s.contains("wrap-2"));
    tui.send(b"\x1b[200~x\x1b[31my\x1b[201~");
    tui.send(b"\r");
    mark(&mut tui, "end-c");
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

    // 5) 나가면(prefix q) bracketed paste를 끈다
    tui.cmd("q");
    assert!(tui.wait_exit());
    tui.wait_screen("bracketed paste 끔", |s| !s.modes.contains(&2004));
}
