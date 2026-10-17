//! TUI harness E2E: PTY 안에서 `aaa attach`를 띄우고, 나온 bytes를 독립 parser(vte)로 화면에 옮겨 단언한다.
//! production VT(vt100)와 다른 구현이라 TUI가 그린 것을 따로 관측한다.
//! 실행: `cargo test -p aaa --test tui`

#![cfg(unix)]

mod lab;

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lab::{BIN, Lab, Node, wait_for, wait_for_diag};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use unicode_width::UnicodeWidthChar;

const T: Duration = Duration::from_secs(20);
const COLS: u16 = 100;
const ROWS: u16 = 30;

const LOCAL_ONLY: &str = r#"schema = 1
[host]
agent_listen = ""
[web]
listen = ""
[local_agent]
mode = "none"
"#;

/// 화면 관측기: 출력·커서 이동·지우기만 다룬다(색은 보지 않는다).
struct Screen {
    cells: Vec<Vec<char>>,
    row: usize,
    col: usize,
}

impl Screen {
    fn new() -> Screen {
        Screen { cells: vec![vec![' '; COLS as usize]; ROWS as usize], row: 0, col: 0 }
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
        if self.col + w > COLS as usize {
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
        if w == 2 && col + 1 < COLS as usize {
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
        let (rows, cols) = (ROWS as usize, COLS as usize);
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
    screen: Arc<Mutex<Screen>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    _master: Box<dyn portable_pty::MasterPty + Send>,
}

impl Tui {
    fn start(node: &Node) -> Tui {
        Self::start_env(node, &[])
    }

    fn start_env(node: &Node, env: &[(&str, &str)]) -> Tui {
        let pty = native_pty_system();
        let pair = pty.openpty(PtySize { rows: ROWS, cols: COLS, pixel_width: 0, pixel_height: 0 }).unwrap();
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
        let screen = Arc::new(Mutex::new(Screen::new()));
        let s = screen.clone();
        std::thread::spawn(move || {
            let mut parser = vte::Parser::new();
            let mut buf = [0u8; 8192];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                parser.advance(&mut *s.lock().unwrap(), &buf[..n]);
            }
        });
        Tui { screen, writer, child, _master: pair.master }
    }

    fn text(&self) -> String {
        self.screen.lock().unwrap().text()
    }

    fn send(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    /// prefix(C-b) 다음 키
    fn cmd(&mut self, key: &str) {
        self.send(b"\x02");
        // crossterm은 ESC 다음 바로 오는 글자를 Alt 조합으로 읽으므로 따로 보낸다
        std::thread::sleep(Duration::from_millis(50));
        self.send(key.as_bytes());
    }

    fn type_line(&mut self, s: &str) {
        self.send(s.as_bytes());
        self.send(b"\r");
    }

    fn wait_text(&self, what: &str, f: impl Fn(&str) -> bool) {
        wait_for_diag(what, T, || f(&self.text()).then_some(()), || format!("TUI 화면:\n{}", self.text()));
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

fn connected(node: &Node, name: &str) -> bool {
    node.machine(name).is_some_and(|m| m["connection"] == "connected")
}

fn start(lab: &mut Lab) -> Node {
    let h = lab.node("h", Some(LOCAL_ONLY));
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", T, || h.json(&["status"]).ok());
    lab.spawn("agent", &h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("labA 연결", T, || connected(&h, "labA").then_some(()));
    h
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
    let h = start(&mut lab);
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "first", "--", "/bin/sh"]);
    let first = leaves(&w)[0].clone();

    let mut tui = Tui::start(&h);
    tui.wait_text("탭과 상태 줄", |s| s.contains("1:first") && s.contains("labA") && s.contains("실행"));
    // 처음 포커스한 pane은 아무도 쓰지 않으니 칸 크기로 맞추고 이 화면이 지금 쓰는 화면이 된다
    tui.wait_text("lease", |s| s.contains("이 화면 크기"));
    let p = h.pane(&first);
    assert_eq!((p["cols"].as_u64(), p["rows"].as_u64()), (Some(COLS as u64), Some(ROWS as u64 - 2)), "{p}");
    assert_eq!(p["lease"]["label"].as_str().map(|l| l.starts_with("tui@")), Some(true), "{p}");

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

    // 좌우로 나누면 host layout이 바뀌고 구분선과 새 pane이 그려진다. 포커스는 새 pane으로 간다.
    tui.cmd("%");
    wait_for("split 반영", T, || (leaves(&h.ok(&["ws", "show"])).len() == 2).then_some(()));
    let second = leaves(&h.ok(&["ws", "show"]))[1].clone();
    tui.wait_text("구분선", |s| s.lines().nth(5).is_some_and(|l| l.contains('│')));
    tui.type_line("echo right-$((5+5))");
    tui.wait_text("오른쪽 출력", |s| s.lines().any(|l| l.find("right-10").is_some_and(|i| l[..i].contains('│'))));
    assert!(h.screen(&second).contains("right-10"));
    assert!(!h.screen(&first).contains("right-10"), "포커스한 pane에만 들어가야 한다");

    // 새 탭과 탭 전환
    tui.cmd("c");
    tui.wait_text("둘째 탭", |s| s.lines().next().is_some_and(|l| l.contains("1:first") && l.contains(" 2:")));
    tui.cmd("1");
    tui.wait_text("첫 탭으로", |s| s.contains("tui-42") && s.contains("right-10"));

    // 나가도(detach) 세션은 그대로다
    tui.cmd("d");
    assert!(tui.wait_exit(), "detach는 성공으로 끝나야 한다");
    drop(tui);
    wait_for("lease 해제", T, || h.pane(&first)["lease"].is_null().then_some(()));
    assert_eq!(h.pane(&first)["state"], "running");
    assert_eq!(leaves(&h.ok(&["ws", "show"])).len(), 3);

    // 다시 붙으면 같은 layout과 화면이 보인다
    let tui = Tui::start(&h);
    tui.wait_text("다시 붙기", |s| s.contains("tui-42") && s.contains("right-10") && s.contains(" 2:"));
    // 보고서용 화면 텍스트(`AAA_TUI_DUMP=파일`)
    if let Ok(path) = std::env::var("AAA_TUI_DUMP") {
        std::fs::write(path, tui.text()).unwrap();
    }
}

/// 다른 front가 쓰던 pane에도 바로 입력된다(가져오기 없이, §8.33). 입력하거나 터미널 focus를 다시 얻은 화면이
/// 지금 쓰는 화면이 되어 칸 크기를 다시 건다. focus가 없는 동안은 크기를 걸지 않는다.
#[test]
fn lease_conflict_with_another_front() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
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
    let held_by = |label: &str| {
        let l = label.to_string();
        let h = &h;
        let id = &id;
        move || h.pane(id)["lease"]["label"].as_str().is_some_and(|x| x.starts_with(&l)).then_some(())
    };

    let mut tui = Tui::start(&h);
    tui.wait_text("다른 front 표시", |s| s.contains("other-front 크기") && s.contains("from-other"));
    assert_eq!(h.pane(&id)["cols"].as_u64(), Some(70), "다른 화면이 쓰는 동안 크기를 바꿨다");

    // 1) 그냥 입력하면 들어가고, 이 화면이 지금 쓰는 화면이 되어 칸 크기로 맞춘다
    tui.type_line("echo direct-$((6*7))");
    tui.wait_text("바로 입력", |s| s.contains("direct-42"));
    wait_for("입력한 화면", T, held_by("tui@"));
    wait_for("크기", T, || (h.pane(&id)["cols"].as_u64() == Some(COLS as u64)).then_some(()));
    tui.wait_text("이 화면 표시", |s| s.contains("이 화면 크기"));

    // 2) 다른 화면이 다시 쓰면 그 크기다. 이 터미널이 focus를 다시 얻으면(ESC [ I) 크기를 다시 건다
    other_types(&mut other, "echo again");
    wait_for("다른 화면", T, held_by("other-front"));
    tui.wait_text("다른 front 표시", |s| s.contains("other-front 크기"));
    tui.send(b"\x1b[I");
    wait_for("focus 얻은 화면", T, held_by("tui@"));
    wait_for("크기", T, || (h.pane(&id)["cols"].as_u64() == Some(COLS as u64)).then_some(()));

    // 3) focus를 잃은 동안(ESC [ O)은 크기를 걸지 않는다: 다른 화면이 썼다 놓아도 그 크기 그대로다
    tui.send(b"\x1b[O");
    std::thread::sleep(Duration::from_millis(200));
    other_types(&mut other, "echo third");
    wait_for("다른 화면", T, held_by("other-front"));
    call(&mut other, aaa_proto::FrontRequest::PaneRelease { pane: id.clone() });
    wait_for("놓음", T, || h.pane(&id)["lease"].is_null().then_some(()));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(h.pane(&id)["cols"].as_u64(), Some(70), "focus가 없는 화면이 크기를 걸었다");
    assert!(h.pane(&id)["lease"].is_null());

    // 4) prefix t(명시적 가져오기)도 그대로 된다
    tui.send(b"\x1b[I");
    wait_for("focus 얻은 화면", T, held_by("tui@"));
    other_types(&mut other, "echo fourth");
    wait_for("다른 화면", T, held_by("other-front"));
    tui.cmd("t");
    wait_for("가져오기", T, held_by("tui@"));
    wait_for("크기", T, || (h.pane(&id)["cols"].as_u64() == Some(COLS as u64)).then_some(()));
    drop(other);
}

/// 재부팅 흉내 뒤 중단 표시, prefix r로 같은 자리에서 재개
#[test]
fn interrupted_badge_and_resume() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
    let w = h.ok(&["ws", "tab", "--machine", "labA", "--name", "job", "--", "/bin/sh"]);
    let old = leaves(&w)[0].clone();
    h.ok(&["pane", "send", &old, "--text", "echo before-$((1+1))", "--enter"]);
    wait_for("출력", T, || h.screen(&old).contains("before-2").then_some(()));

    let mut tui = Tui::start(&h);
    tui.wait_text("첫 화면", |s| s.contains("before-2"));

    lab.kill("agent");
    tui.wait_text("끊김 표시", |s| s.contains("끊김·읽기 전용"));
    assert_eq!(lab.kill_session_hosts(), 1);
    lab.spawn_env("agent2", &h, &["agent", "--host", "local", "--name", "labA"], &[("AAA_TEST_BOOT_ID", "boot-after")]);
    tui.wait_text("중단 표시", |s| s.contains("중단(재부팅) — 재개: prefix r") && s.contains("before-2"));

    tui.cmd("r");
    wait_for("재개", T, || {
        let l = leaves(&h.ok(&["ws", "show"]));
        (l.len() == 1 && l[0] != old).then(|| l[0].clone())
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
/// 셸 pane이라 입력창에 경로를 넣지 않는다. 같은 길을 `aaa pane attach`(raw ssh 셸용)로도 확인한다.
/// 기본은 `AAA_TEST_CLIPBOARD_PNG` 파일을 clipboard로 쓴다(개발 Mac의 clipboard를 덮어쓰지 않는다).
/// `AAA_CLIP_NATIVE=1`(CI)이면 실제 시스템 clipboard에 쓰고 TUI가 그것을 읽는다.
#[test]
fn ctrl_v_pastes_clipboard_image() {
    let mut lab = Lab::new();
    let h = start(&mut lab);
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

    // 2) 그림이 있으면 대상 머신에 저장하고 상태 줄에 경로를 보인다. 셸에는 경로를 넣지 않는다.
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
    tui.wait_text("저장 표시", |s| s.contains("그림을 저장했다"));
    let files = attachments(&h.state);
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].extension().is_some_and(|e| e == "png"), "{files:?}");
    assert_eq!(decode_rgba(&std::fs::read(&files[0]).unwrap()), (3, 2, rgba.clone()), "대상 머신 그림의 pixel");
    assert!(!h.screen(&id).contains("attachments"), "셸에 경로가 들어갔다");
    drop(clip);

    // 3) 여러 조각(1.2MB)을 `aaa pane attach`로 올린다. TUI가 쓰는 중이어도 되고, 한 번 보내는 CLI라
    //    지금 쓰는 화면은 TUI 그대로다.
    let big = lab.root.join("big.png");
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.resize(1_200_000, 7);
    std::fs::write(&big, &bytes).unwrap();
    let r = h.ok(&["pane", "attach", &id, big.to_str().unwrap()]);
    assert_eq!(h.pane(&id)["lease"]["label"].as_str().map(|l| l.starts_with("tui@")), Some(true));
    tui.cmd("d");
    assert!(tui.wait_exit());
    drop(tui);
    let path = r["path"].as_str().unwrap();
    assert_eq!(std::fs::read(path).unwrap(), bytes, "{r}");
    assert_eq!(r["inserted"], false, "{r}");
    // 그림이 아니면 거부한다
    let txt = lab.root.join("note.txt");
    std::fs::write(&txt, "hello").unwrap();
    let r: serde_json::Value =
        serde_json::from_str(&h.json(&["pane", "attach", &id, txt.to_str().unwrap()]).unwrap_err()).unwrap();
    assert!(r["error"]["message"].as_str().is_some_and(|m| m.contains("PNG·JPEG·GIF·WebP")), "{r}");
    assert_eq!(attachments(&h.state).len(), 2);
}
