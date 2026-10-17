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
/// 기본 시험 터미널 폭: 왼쪽 목록이 접힌 채 시작하는 폭(목록 시험은 WIDE)
const COLS: u16 = 90;
const WIDE: u16 = 130;
const ROWS: u16 = 30;

/// 시험 설정: 사용자 입력기는 건드리지 않고(기본 끔), prefix 뒤 자모 해석은 켠다
const LOCAL_ONLY: &str = r#"schema = 1
[host]
agent_listen = ""
[web]
listen = ""
[local_agent]
mode = "none"
[tui]
prefix_jamo_keys = true
"#;

/// 화면 관측기: 출력·커서 이동·지우기만 다룬다(색은 보지 않는다).
struct Screen {
    cells: Vec<Vec<char>>,
    row: usize,
    col: usize,
    cols: usize,
}

impl Screen {
    fn new(cols: u16) -> Screen {
        Screen { cells: vec![vec![' '; cols as usize]; ROWS as usize], row: 0, col: 0, cols: cols as usize }
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
        if w == 2 && col + 1 < self.cols {
            // 넓은 글자의 둘째 칸은 text()에서 뺀다
            self.cells[r][col + 1] = '\0';
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
        if !inter.is_empty() {
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
        let (lock, cv) = &*self.screen;
        let deadline = Instant::now() + T;
        let mut g = lock.lock().unwrap();
        loop {
            let text = g.text();
            if f(&text) {
                return;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "{T:?} 안에 {what}이 되지 않았다\nTUI 화면:\n{text}");
            g = cv.wait_timeout(g, left).unwrap().0;
        }
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
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "first", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();

    let mut tui = Tui::start(&h);
    tui.wait_text("탭과 상태 줄", |s| s.contains("1:first") && s.contains("labA") && s.contains("실행"));
    // 처음 포커스한 pane은 아무도 쓰지 않으니 칸 크기로 맞추고 이 화면이 지금 쓰는 화면이 된다
    tui.wait_text("lease", |s| s.contains("이 화면 크기"));
    hw.wait("칸 크기와 lease", |w| {
        let p = w.pane(&first)?;
        ((p.cols, p.rows) == (COLS, ROWS - 2) && w.held_by(&first, "tui@")).then_some(())
    });

    tui.type_line("echo tui-$((6*7))");
    tui.wait_text("입력 결과", |s| s.contains("tui-42"));

    // harness 상태는 hook 사건으로만 바뀌고 상태 줄·badge에 보인다(세션 안 `aaa hook`)
    tui.type_line(&format!(
        "'{}' hook --agent claude '{{\"hook_event_name\":\"PermissionRequest\",\"tool_name\":\"Bash\"}}'; clear",
        lab::BIN
    ));
    tui.wait_text("질문 상태", |s| s.contains("실행 · 질문") && s.contains("질문 — aaa question"));
    tui.type_line(&format!("'{}' hook --agent claude '{{\"hook_event_name\":\"UserPromptSubmit\"}}'; clear", lab::BIN));
    tui.wait_text("작업 상태", |s| s.contains("실행 · 작업 중") && !s.contains("질문 — aaa question"));
    tui.type_line("echo tui-$((6*7))");
    tui.wait_text("입력 결과", |s| s.contains("tui-42"));

    // prefix v(herdr split_vertical)로 좌우로 나누면 host layout이 바뀌고 구분선과 새 pane이 그려진다.
    // 포커스는 새 pane(오른쪽)으로 간다.
    tui.cmd("v");
    let second = hw.wait("split 반영", |w| {
        let l = w.leaves();
        (l.len() == 2).then(|| l[1].clone())
    });
    tui.wait_text("구분선", |s| s.lines().nth(5).is_some_and(|l| l.contains('│')));
    tui.type_line("echo right-$((5+5))");
    tui.wait_text("오른쪽 출력", |s| s.lines().any(|l| l.find("right-10").is_some_and(|i| l[..i].contains('│'))));
    assert!(h.screen(&second).contains("right-10"));
    assert!(!h.screen(&first).contains("right-10"), "포커스한 pane에만 들어가야 한다");

    // 새 탭과 탭 전환. 한글 입력기가 켜진 채 누른 명령 키(ㅊ = c 자리)도 같은 명령이다
    // (시험 설정 prefix_jamo_keys = true. 기본은 끔이고 입력기 전환도 기본 끔이라 사용자 입력기를 건드리지 않는다)
    tui.cmd("c");
    tui.wait_text("둘째 탭", |s| s.lines().next().is_some_and(|l| l.contains("1:first") && l.contains(" 2:")));
    tui.cmd("ㅊ");
    tui.wait_text("셋째 탭(자모 ㅊ)", |s| s.lines().next().is_some_and(|l| l.contains(" 3:")));
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
    tui.wait_text("다시 붙기", |s| s.contains("tui-42") && s.contains("right-10") && s.contains(" 2:"));
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
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "keys", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();
    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| s.contains("1:keys") && s.contains("이 화면 크기"));

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
    tui.wait_text("^B", |s| s.contains("^B"));
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

    // 4) o: 질문이 열린 pane으로 간다
    h.ok(&[
        "pane",
        "send",
        &bottom,
        "--text",
        &format!(
            "'{}' hook --agent claude '{{\"hook_event_name\":\"PermissionRequest\",\"tool_name\":\"Bash\"}}'; clear",
            lab::BIN
        ),
        "--enter",
    ]);
    hw.wait("질문", |w| {
        w.pane(&bottom)?.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question).then_some(())
    });
    tui.cmd("o");
    typed_in(&mut tui, "asked", &bottom);

    // 5) x: 포커스 pane을 닫는다
    tui.cmd("x");
    hw.wait("닫힘", |w| (w.leaves() == vec![first.clone(), right.clone()]).then_some(()));

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

/// 다른 front가 쓰던 pane에도 바로 입력된다(가져오기 없이, §8.33). 입력하거나 터미널 focus를 다시 얻은 화면이
/// 지금 쓰는 화면이 되어 칸 크기를 다시 건다. focus가 없는 동안은 크기를 걸지 않는다.
#[test]
fn lease_conflict_with_another_front() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "shared", "--", "/bin/sh"]);
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
    hw.wait("입력한 화면의 크기", |w| (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS).then_some(()));
    tui.wait_text("이 화면 표시", |s| s.contains("이 화면 크기"));

    // 2) 다른 화면이 다시 쓰면 그 크기다. 이 터미널이 focus를 다시 얻으면(ESC [ I) 크기를 다시 건다
    other_types(&mut other, "echo again");
    hw.wait("다른 화면", |w| w.held_by(&id, "other-front").then_some(()));
    tui.wait_text("다른 front 표시", |s| s.contains("other-front 크기"));
    tui.send(b"\x1b[I");
    hw.wait("focus 얻은 화면의 크기", |w| (w.held_by(&id, "tui@") && w.pane(&id)?.cols == COLS).then_some(()));

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

/// 목록에서 고른 줄(▸ 표시)의 목록 칸 글
fn selected(text: &str) -> Option<String> {
    text.lines().map(|l| l.chars().take(32).collect::<String>()).find(|l| l.contains('▸'))
}

/// 목록 맨 위(고른 줄 바로 위가 「Thread n」 머리)인가
fn at_top(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    lines
        .iter()
        .position(|l| l.chars().take(32).any(|c| c == '▸'))
        .is_some_and(|i| i > 0 && lines[i - 1].contains("Thread "))
}

/// 탐색 모드에서 목록 맨 위(첫 thread)로 올라간 뒤 `needle` 줄까지 내려간다(herdr 탐색은 ↑↓). 한 칸씩 화면이 바뀐 것을 보고
/// 다음 키를 누른다.
fn pick(tui: &mut Tui, needle: &str) {
    tui.wait_text("탐색 모드", |s| s.contains(" NAVIGATE ") && selected(s).is_some());
    while !at_top(&tui.text()) {
        let now = selected(&tui.text()).unwrap();
        tui.send(b"\x1b[A");
        tui.wait_text("한 칸 올라감", |s| selected(s).is_some_and(|l| l != now));
    }
    for _ in 0..12 {
        let now = selected(&tui.text()).unwrap();
        if now.contains(needle) {
            return;
        }
        tui.send(b"\x1b[B");
        tui.wait_text("한 칸 내려감", |s| selected(s).is_some_and(|l| l != now));
    }
    panic!("목록에 {needle}이 없다:\n{}", tui.text());
}

/// 왼쪽 목록(§8.35): 웹 목록처럼 Thread 묶음·머신별 pane·새 셸이 보이고, 새 thread를 따라가며, 고르면 탭으로 연다.
/// 넓은 터미널은 펼친 채, 좁은 터미널은 접힌 채 시작한다. herdr 키: prefix b 펴기·접기(포커스는 그대로),
/// prefix w 탐색 모드(↑↓ 고르기·Enter 열기·Esc 돌아가기, 접혀 있으면 pane 위에 띄워 보인다).
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

    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "first", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();
    let loose = h.ok(&["pane", "run", "--machine", "labA", "--name", "loose", "--", "/bin/sh"])["id"]
        .as_str()
        .unwrap()
        .to_string();
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

    // 1) 넓은 터미널: 펼친 목록, pane 칸은 목록(32)과 구분선(1)을 뺀 폭
    let mut tui = Tui::start_cols(&h, &[], WIDE);
    tui.wait_text("목록", |s| {
        s.contains("Thread 1")
            && s.contains("alpha-job")
            && s.contains("■ labA")
            && s.contains("+ 새 셸")
            && s.contains("first")
            && s.contains("loose")
    });
    hw.wait("목록 옆 크기", |w| (w.pane(&first)?.cols == WIDE - 33).then_some(()));
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
    tui.wait_text("탐색 모드", |s| s.contains(" NAVIGATE ") && selected(s).is_some());
    pick(&mut tui, "loose");
    tui.send(b"\r");
    hw.wait("새 탭", |w| w.leaves().contains(&loose).then_some(()));
    tui.wait_text("pane으로 돌아옴", |s| !s.contains(" NAVIGATE ") && s.contains(" loose "));

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

    // 8) prefix b로 접으면 pane이 전체 폭을 쓴다
    tui.cmd("1");
    tui.wait_text("첫 탭", |s| s.contains("list-42"));
    tui.cmd("b");
    tui.wait_text("접힘", |s| !s.contains("■ labA"));
    hw.wait("전체 폭", |w| (w.pane(&first)?.cols == WIDE).then_some(()));
    tui.cmd("q");
    assert!(tui.wait_exit());
    drop(tui);

    // 9) 좁은 터미널은 접힌 채 시작한다. 탐색 모드는 접힌 목록을 pane 위에 띄워 보이고(pane 크기는 그대로),
    //    prefix b로 펴면 목록 옆 크기가 된다(포커스는 pane 그대로: 바로 입력된다)
    let mut tui = Tui::start(&h);
    tui.wait_text("접힌 채 시작", |s| s.contains("list-42") && s.contains("1:first"));
    hw.wait("전체 폭", |w| (w.pane(&first)?.cols == COLS).then_some(()));
    assert!(!tui.text().contains("■ labA"));
    tui.cmd("w");
    tui.wait_text("띄운 목록", |s| s.contains(" NAVIGATE ") && s.contains("■ labA"));
    tui.send(b"\x1b");
    tui.wait_text("목록 닫힘", |s| !s.contains("■ labA"));
    assert_eq!(hw.pane(&first).unwrap().cols, COLS, "탐색 모드가 pane 크기를 바꿨다");
    tui.cmd("b");
    tui.wait_text("펼침", |s| s.contains("■ labA") && !s.contains(" NAVIGATE "));
    hw.wait("목록 옆 크기", |w| (w.pane(&first)?.cols == COLS - 33).then_some(()));
    tui.type_line("echo still-$((3*3))");
    tui.wait_text("포커스 그대로", |s| s.contains("still-9"));
}

/// 재부팅 흉내 뒤 중단 표시. herdr에는 재개 키가 없다: 사람이 다시 띄우는 길은 badge가 알려 주는 CLI다.
#[test]
fn interrupted_badge_and_resume() {
    let mut lab = Lab::new();
    let (h, mut hw) = start(&mut lab);
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "job", "--", "/bin/sh"]);
    let old = leaves(&w)[0].clone();
    h.ok(&["pane", "send", &old, "--text", "echo before-$((1+1))", "--enter"]);

    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| s.contains("before-2"));

    lab.kill("agent");
    tui.wait_text("끊김 표시", |s| s.contains("끊김·읽기 전용"));
    assert_eq!(lab.kill_session_hosts(), 1);
    lab.spawn_env("agent2", &h, &["agent", "--host", "local", "--name", "labA"], &[("AAA_TEST_BOOT_ID", "boot-after")]);
    let short: String = old.chars().take(8).collect();
    let badge = format!("중단(재부팅) — aaa pane resume {short}");
    tui.wait_text("중단 표시", |s| s.contains(&badge) && s.contains("before-2"));

    h.ok(&["pane", "resume", &short]);
    hw.wait("재개", |w| {
        let l = w.leaves();
        (l.len() == 1 && l[0] != old && w.pane(&l[0]).is_some_and(|p| p.state.is_running())).then_some(())
    });
    tui.wait_text("재개 뒤 실행", |s| !s.contains("중단(재부팅)") && s.contains("실행"));
    tui.type_line("echo resumed-$((5*5))");
    tui.wait_text("재개 뒤 입력", |s| s.contains("resumed-25"));
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

fn decode_rgba(bytes: &[u8]) -> (u32, u32, Vec<u8>) {
    let mut r = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
    let mut buf = vec![0; r.output_buffer_size().unwrap()];
    let info = r.next_frame(&mut buf).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba, "RGBA PNG여야 한다");
    buf.truncate(info.buffer_size());
    (info.width, info.height, buf)
}

/// Ctrl+V 그림 붙여넣기(§8.28): 그림이 없으면 0x16이 그대로 가고, 있으면 pane 머신 세션 폴더에 PNG로 저장된다.
/// 셸 pane에도 따옴표 경로와 빈칸을 넣는다(Enter 없음, herdr·herdr-web-ui처럼, plan §8.51). 같은 길을 `aaa pane attach`(raw ssh 셸용)로도 확인한다.
/// 기본은 `AAA_TEST_CLIPBOARD_PNG` 파일을 clipboard로 쓴다(개발 Mac의 clipboard를 덮어쓰지 않는다).
/// `AAA_CLIP_NATIVE=1`(CI)이면 실제 시스템 clipboard에 쓰고 TUI가 그것을 읽는다.
#[test]
fn ctrl_v_pastes_clipboard_image() {
    let mut lab = Lab::new();
    let (h, _hw) = start(&mut lab);
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "pic", "--", "/bin/sh"]);
    let id = leaves(&w)[0].clone();
    let native = std::env::var_os("AAA_CLIP_NATIVE").is_some();
    let rgba: Vec<u8> = PIXELS.concat();
    let fake = lab.root.join("clip.png");
    let fake_s = fake.to_str().unwrap().to_string();
    let mut clip = native.then(|| arboard::Clipboard::new().expect("clipboard를 열지 못했다"));

    let mut tui = if native { Tui::start(&h) } else { Tui::start_env(&h, &[("AAA_TEST_CLIPBOARD_PNG", &fake_s)]) };
    tui.wait_text("lease", |s| s.contains("이 화면 크기"));

    // 1) 그림이 없으면 Ctrl+V는 원래 키다(lnext를 끈 cat -v가 ^V를 보인다)
    if let Some(c) = clip.as_mut() {
        c.set_text("no image here").unwrap();
    }
    tui.type_line("stty -iexten; cat -v");
    std::thread::sleep(Duration::from_millis(300));
    tui.send(b"\x16");
    tui.send(b"\r");
    tui.wait_text("원래 Ctrl+V", |s| s.contains("^V"));
    tui.send(b"\x03");
    assert!(attachments(&h.state).is_empty(), "그림이 없는데 첨부가 생겼다");

    // 2) 그림이 있으면 대상 머신에 저장하고 셸 줄에 따옴표 경로를 넣고(Enter 없음) 상태 줄에 알린다.
    match clip.as_mut() {
        Some(c) => c.set_image(arboard::ImageData { width: 3, height: 2, bytes: rgba.as_slice().into() }).unwrap(),
        None => {
            let mut out = Vec::new();
            let mut enc = png::Encoder::new(&mut out, 3, 2);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut wr = enc.write_header().unwrap();
            wr.write_image_data(&rgba).unwrap();
            wr.finish().unwrap();
            std::fs::write(&fake, out).unwrap();
        }
    }
    tui.type_line("clear");
    std::thread::sleep(Duration::from_millis(300));
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
