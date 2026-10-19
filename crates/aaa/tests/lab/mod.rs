//! E2E 실험실: 한 머신에 host와 agent backend 여러 개를 각자 다른 설정·상태 폴더로 띄운다(결정 3).
//!
//! - Unix는 UDS 길이 제한(macOS 104바이트) 때문에 `/tmp/aae-<pid>-<n>`처럼 짧게 둔다. Windows는 Named Pipe라 임시 폴더를 쓴다.
//! - 띄운 프로세스는 `Child`로만 잡고, 끝날 때 그 PID만 종료한다. session host(setsid로 분리)는
//!   상태 폴더의 meta.json에 적힌 PID 중 명령줄이 이 실험실 경로를 가리키는 것만 종료한다.

#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;

pub mod watch;

pub const BIN: &str = env!("CARGO_BIN_EXE_aaa");
static SEQ: AtomicU32 = AtomicU32::new(0);

pub struct Lab {
    pub root: PathBuf,
    procs: Vec<(String, Child)>,
}

#[derive(Clone)]
pub struct Node {
    pub cfg: PathBuf,
    pub state: PathBuf,
}

impl Node {
    pub fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(BIN);
        c.args(args)
            .env("AAA_CONFIG_DIR", &self.cfg)
            .env("AAA_STATE_DIR", &self.state)
            .env("RUST_LOG", "aaa=info")
            // 기본 profile(claude·codex) 목록이 이 머신의 설치·로그인에 따라 바뀌지 않게(보는 시험만 채운다)
            .env("AAA_TEST_BUILTINS", "")
            // routine `every <N>s`: host가 시험에서만 받는다
            .env("AAA_TEST_ROUTINE_SECONDS", "1")
            // 실험실 CLI는 바깥 aaa pane의 신원이 아니다. 세션 안 동작은 각 시험이 env로 명시한다.
            .env_remove("AAA_SESSION_ID")
            .env_remove("AAA_HOOK_SOCK")
            .env_remove("NO_COLOR")
            // herdr pane 안에서 돌리면 물려받는다(herdr가 pane마다 건다). CI와 같게 빼서 aaa가 거는지 본다
            .env_remove("fish_handle_reflow")
            .stdin(Stdio::null());
        // 명령 없는 pane의 기본 셸을 사람의 셸 설정(zsh 테마·plugin) 대신 /bin/sh로: 로컬과 CI가 같은 화면을 본다(herdr tests/cli/harness.rs)
        #[cfg(unix)]
        c.env("SHELL", "/bin/sh");
        // 그 셸은 login으로 떠(portable-pty `new_default_prog`) 기계 rc를 읽는다: macOS `/etc/profile` → `/etc/bashrc`가 prompt를
        // `\h:\W \u\$ `로 정해, runner의 긴 hostname이 좁은 pane에서 접혔다(GitHub macOS `pane_modes_like_herdr`). sh는 rc 뒤에
        // `$ENV` 파일을 읽으므로 거기서 `$ `로 정한다(`Lab::node`가 쓴다). `/etc/bashrc_$TERM_PROGRAM`도 돌지 않게 뺀다
        #[cfg(unix)]
        c.env("ENV", self.cfg.join("shrc")).env_remove("TERM_PROGRAM");
        c
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().expect("aaa 실행")
    }

    /// `--json`을 붙여 실행하고 JSON을 돌려준다. 실패하면 Err(표준 오류의 `{"error":…}` JSON, 아니면 출력 전부).
    /// 오류 JSON은 herdr처럼 표준 오류로 온다.
    pub fn json(&self, args: &[&str]) -> Result<Value, String> {
        // 맨 앞에 둔다: `pane run <pane> <글…>`처럼 뒤 인자를 모두 글로 받는 명령에 섞이지 않게
        let mut a = vec!["--json"];
        a.extend_from_slice(args);
        let o = self.run(&a);
        let out = String::from_utf8_lossy(&o.stdout).to_string();
        let err = String::from_utf8_lossy(&o.stderr).to_string();
        if o.status.success() {
            return serde_json::from_str::<Value>(&out).map_err(|_| format!("{out}{err}"));
        }
        match err.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok()) {
            Some(v) => Err(v.to_string()),
            None => Err(format!("{out}{err}")),
        }
    }

    pub fn ok(&self, args: &[&str]) -> Value {
        self.json(args).unwrap_or_else(|e| panic!("aaa {} 실패: {e}", args.join(" ")))
    }

    /// 사람이 터미널(aaa 세션 밖)에서 친 것처럼 실행한다(사람만 하는 명령의 시험, `crates/aaa/src/person.rs`): `script`로
    /// PTY를 만들고, `answers`의 물음(`(보일 글, 칠 줄)`)이 보이면 차례로 친다. 표준 출력과 오류를 이어 돌려준다.
    #[cfg(unix)]
    pub fn in_terminal(&self, args: &[&str], answers: &[(&str, &str)], timeout: Duration) -> String {
        self.in_terminal_env(args, &[], answers, timeout)
    }

    /// `in_terminal`과 같되 환경 변수를 더 준다(예: 가짜 `launchctl`을 PATH 앞에, HOME을 실험실에)
    pub fn in_terminal_env(
        &self,
        args: &[&str],
        env: &[(&str, &str)],
        answers: &[(&str, &str)],
        timeout: Duration,
    ) -> String {
        self.in_terminal_bin(Path::new(BIN), args, env, answers, timeout)
    }

    /// `in_terminal_env`와 같되 실행 파일을 고른다(`aaa update`는 지금 도는 실행 파일을 바꾸므로 실험실 사본을 띄운다)
    pub fn in_terminal_bin(
        &self,
        bin: &Path,
        args: &[&str],
        env: &[(&str, &str)],
        answers: &[(&str, &str)],
        timeout: Duration,
    ) -> String {
        let bin = bin.to_str().expect("실행 파일 경로");
        let mut c = Command::new("script");
        if cfg!(target_os = "macos") {
            c.args(["-q", "/dev/null", bin]).args(args);
        } else {
            let line = std::iter::once(bin).chain(args.iter().copied()).map(shell_word).collect::<Vec<_>>().join(" ");
            c.args(["-qec", &line, "/dev/null"]);
        }
        c.env("AAA_CONFIG_DIR", &self.cfg)
            .env("AAA_STATE_DIR", &self.state)
            .env("AAA_TEST_BUILTINS", "")
            .env_remove("AAA_SESSION_ID")
            .env_remove("AAA_HOOK_SOCK")
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().expect("script 실행");
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let mut out = child.stdout.take().unwrap();
        let s2 = seen.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut b = [0u8; 256];
            while let Ok(n) = std::io::Read::read(&mut out, &mut b) {
                if n == 0 {
                    break;
                }
                s2.lock().unwrap().extend_from_slice(&b[..n]);
            }
            let _ = done_tx.send(());
        });
        let text = || String::from_utf8_lossy(&seen.lock().unwrap()).to_string();
        let mut stdin = child.stdin.take().unwrap();
        // 물음이 보인 뒤에 친다(script는 입력이 끝나면 바로 EOF를 넣는다)
        for (prompt, typed) in answers {
            wait_for_diag(&format!("물음 {prompt:?}"), timeout, || text().contains(prompt).then_some(()), text);
            std::io::Write::write_all(&mut stdin, format!("{typed}\n").as_bytes()).unwrap();
        }
        // 출력이 끝날(명령이 끝날) 때까지. 답하지 않은 물음에서 기다리면 멈추지 않고 실패로 끝낸다(입력을 닫으면 macOS
        // script가 친 답보다 EOF를 먼저 넣기도 해 닫지 않는다)
        if done_rx.recv_timeout(timeout).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            panic!("{args:?}가 {timeout:?} 안에 끝나지 않았다(답하지 않은 물음?):\n{}", text());
        }
        let o = child.wait_with_output().unwrap();
        drop(stdin);
        reader.join().unwrap();
        format!("{}{}", text(), String::from_utf8_lossy(&o.stderr))
    }

    /// 보이는 화면 글(`pane read --source visible`)
    pub fn screen(&self, pane: &str) -> String {
        let v = self.ok(&["pane", "read", pane, "--source", "visible"]);
        v["text"].as_str().unwrap().to_string()
    }

    /// 새 탭에 pane을 띄우고(`tab create`) 그 pane ID를 돌려준다. `workspace`가 있으면 그 workspace에, `cwd`가 있으면 그 폴더에서.
    pub fn new_pane(
        &self,
        workspace: Option<&str>,
        machine: &str,
        label: Option<&str>,
        argv: &[&str],
        cwd: Option<&str>,
    ) -> String {
        let mut a = vec!["tab", "create", "--machine", machine];
        if let Some(w) = workspace {
            a.extend(["--workspace", w]);
        }
        if let Some(c) = cwd {
            a.extend(["--cwd", c]);
        }
        if let Some(l) = label {
            a.extend(["--label", l]);
        }
        if !argv.is_empty() {
            a.push("--");
            a.extend(argv);
        }
        let w = self.ok(&a);
        let tab = w["tabs"].as_array().and_then(|t| t.last()).unwrap_or_else(|| panic!("탭이 없다: {w}"));
        tab["root"]["pane"].as_str().unwrap_or_else(|| panic!("새 탭의 pane이 없다: {w}")).to_string()
    }

    /// 출력에 글이 나올 때까지 기다린다(`pane wait-output`, host가 출력 사건마다 다시 본다)
    pub fn wait_output(&self, pane: &str, needle: &str, timeout: Duration) -> Value {
        let ms = timeout.as_millis().to_string();
        self.json(&["pane", "wait-output", pane, &format!("--match={needle}"), "--timeout", &ms]).unwrap_or_else(|e| {
            panic!("{pane}에 {needle:?}가 {timeout:?} 안에 나오지 않았다: {e}\n화면:\n{}", self.screen(pane))
        })
    }

    /// `wait_output`과 같되 화면 폭에서 접힌 줄을 이어 본다(`--source recent-unwrapped`, 긴 줄을 기다릴 때)
    pub fn wait_output_unwrapped(&self, pane: &str, needle: &str, timeout: Duration) -> Value {
        let ms = timeout.as_millis().to_string();
        let args = [
            "pane",
            "wait-output",
            pane,
            "--source",
            "recent-unwrapped",
            &format!("--match={needle}"),
            "--timeout",
            &ms,
        ];
        self.json(&args).unwrap_or_else(|e| {
            panic!("{pane}에 {needle:?}가 {timeout:?} 안에 나오지 않았다: {e}\n화면:\n{}", self.screen(pane))
        })
    }

    pub fn pane(&self, pane: &str) -> Value {
        let v = self.ok(&["pane", "list"]);
        v["panes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == pane || p["name"] == pane)
            .cloned()
            .unwrap_or_else(|| panic!("pane {pane}이 목록에 없다: {v}"))
    }

    pub fn machine(&self, name: &str) -> Option<Value> {
        let v = self.json(&["machine", "list"]).ok()?;
        v["machines"].as_array()?.iter().find(|m| m["name"] == name).cloned()
    }

    pub fn connected(&self, name: &str) -> bool {
        self.machine(name).is_some_and(|m| m["connection"] == "connected")
    }

    /// host ticker를 `n`번 돌리고(`aaa ticker tick`: 이 명령 뒤에 시작한 tick이 끝나면 돌아온다) 끝난 tick 수를 돌려준다.
    /// 「host가 보고도 아무것도 하지 않았다」를 고정 sleep 없이 단언할 때 쓴다(herdr-projects 시험의 `ticker::tick_project`).
    /// 시간 규칙(idle·quiet·schedule·outage_secs)이 막는 일은 tick만으로 일어나지 않는다: 그 규칙은 시간이 지났음을 먼저 보인다.
    pub fn ticks(&self, n: usize) -> u64 {
        (0..n).fold(0, |_, _| self.ok(&["ticker", "tick"])["runs"].as_u64().expect("ticker tick 응답"))
    }

    /// 이 agent 노드에서 세션의 session host PID(세션 폴더 meta.json)
    pub fn session_host_pid(&self, session: &str) -> Option<u32> {
        let text = std::fs::read_to_string(self.state.join("agent/sessions").join(session).join("meta.json")).ok()?;
        serde_json::from_str::<Value>(&text).ok()?["host_pid"].as_u64().map(|p| p as u32)
    }

    /// 그 session host가 끝났다(PID가 없거나 다른 프로세스가 됐다). 세션 폴더를 지운 뒤에도 명령줄로 가린다
    pub fn session_host_gone(&self, pid: u32, session: &str) -> bool {
        command_of(pid).is_none_or(|c| !(cfg!(windows) || c.contains(session)))
    }
}

impl Lab {
    pub fn new() -> Lab {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let name = format!("aae-{}-{n}", std::process::id());
        let root = if cfg!(unix) { PathBuf::from("/tmp").join(name) } else { std::env::temp_dir().join(name) };
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        Lab { root, procs: vec![] }
    }

    /// 설정 폴더에 host.toml을 쓴다. agent_listen·web listen은 `127.0.0.1:0`이면 OS가 고른다.
    pub fn node(&self, name: &str, host_toml: Option<&str>) -> Node {
        let dir = self.root.join(name);
        let node = Node { cfg: dir.join("c"), state: dir.join("s") };
        std::fs::create_dir_all(&node.cfg).unwrap();
        std::fs::create_dir_all(&node.state).unwrap();
        // 시험 셸 prompt(`Node::cmd`의 `ENV`): 기계 hostname·rc와 상관없이 같다
        #[cfg(unix)]
        std::fs::write(node.cfg.join("shrc"), "PS1='$ '\n").unwrap();
        if let Some(t) = host_toml {
            std::fs::write(node.cfg.join("host.toml"), t).unwrap();
        }
        node
    }

    pub fn spawn(&mut self, tag: &str, node: &Node, args: &[&str]) -> u32 {
        self.spawn_env(tag, node, args, &[])
    }

    pub fn spawn_env(&mut self, tag: &str, node: &Node, args: &[&str], env: &[(&str, &str)]) -> u32 {
        let log = std::fs::File::create(self.root.join(format!("{tag}.log"))).unwrap();
        let mut cmd = node.cmd(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        let child = cmd.stdout(log.try_clone().unwrap()).stderr(log).spawn().expect("spawn");
        let pid = child.id();
        self.procs.push((tag.to_string(), child));
        pid
    }

    /// 출력을 읽어야 하는 프로세스(예: enroll)
    pub fn spawn_piped(
        &mut self,
        tag: &str,
        node: &Node,
        args: &[&str],
    ) -> (u32, BufReader<std::process::ChildStdout>) {
        let log = std::fs::File::create(self.root.join(format!("{tag}.log"))).unwrap();
        let mut child = node.cmd(args).stdout(Stdio::piped()).stderr(log).spawn().expect("spawn");
        let out = BufReader::new(child.stdout.take().unwrap());
        let pid = child.id();
        self.procs.push((tag.to_string(), child));
        (pid, out)
    }

    /// 띄운 프로세스 하나를 SIGKILL로 끝낸다(갑작스러운 죽음을 흉내 낸다).
    /// 이 실험실이 띄운 프로세스에 신호를 보낸다(SIGSTOP·SIGCONT로 응답을 붙잡는 시험)
    #[cfg(unix)]
    pub fn signal(&self, tag: &str, sig: i32) {
        let (_, c) = self.procs.iter().find(|(t, _)| t == tag).unwrap_or_else(|| panic!("{tag} 없음"));
        assert_eq!(unsafe { libc::kill(c.id() as i32, sig) }, 0, "{tag}에 신호 {sig}");
    }

    pub fn kill(&mut self, tag: &str) {
        let i = self.procs.iter().position(|(t, _)| t == tag).unwrap_or_else(|| panic!("{tag} 없음"));
        let (_, mut c) = self.procs.remove(i);
        let _ = c.kill();
        let _ = c.wait();
    }

    pub fn wait_exit(&mut self, tag: &str, timeout: Duration) -> std::process::ExitStatus {
        let i = self.procs.iter().position(|(t, _)| t == tag).unwrap();
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(s) = self.procs[i].1.try_wait().unwrap() {
                self.procs.remove(i);
                return s;
            }
            assert!(Instant::now() < deadline, "{tag}이 {timeout:?} 안에 끝나지 않았다");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn log(&self, tag: &str) -> String {
        std::fs::read_to_string(self.root.join(format!("{tag}.log"))).unwrap_or_default()
    }

    /// 이 실험실의 session host를 모두 SIGKILL한다(재부팅 흉내: 프로세스가 사라진다).
    pub fn kill_session_hosts(&self) -> usize {
        let pids = self.session_hosts();
        for pid in &pids {
            #[cfg(unix)]
            unsafe {
                libc::kill(*pid as i32, libc::SIGKILL)
            };
            #[cfg(windows)]
            let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &pid.to_string()]).output();
        }
        pids.len()
    }

    fn session_hosts(&self) -> Vec<u32> {
        let mut out: Vec<(u32, PathBuf)> = vec![];
        let Ok(nodes) = std::fs::read_dir(&self.root) else { return vec![] };
        for n in nodes.flatten() {
            let sessions = n.path().join("s/agent/sessions");
            let Ok(list) = std::fs::read_dir(&sessions) else { continue };
            for s in list.flatten() {
                let Ok(text) = std::fs::read_to_string(s.path().join("meta.json")) else { continue };
                let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                if let Some(pid) = v["host_pid"].as_u64() {
                    out.push((pid as u32, s.path()));
                }
            }
        }
        // 그 PID가 지금도 이 실험실의 session host인지 명령줄로 확인한다(PID 재사용 대비).
        out.into_iter()
            .filter(|(pid, dir)| command_of(*pid).is_some_and(|c| cfg!(windows) || c.contains(&*dir.to_string_lossy())))
            .map(|(p, _)| p)
            .collect()
    }
}

#[cfg(unix)]
fn command_of(pid: u32) -> Option<String> {
    let o = Command::new("ps").args(["-o", "command=", "-p", &pid.to_string()]).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Windows: 명령줄 대신 그 PID의 실행 파일 이름이 aaa인지 본다(CI 전용 임시 runner).
#[cfg(windows)]
fn command_of(pid: u32) -> Option<String> {
    let filter = format!("PID eq {pid}");
    let o = Command::new("tasklist").args(["/FI", &filter, "/FO", "CSV", "/NH"]).output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    s.to_ascii_lowercase().contains("\"aaa.exe\"").then_some(s)
}

impl Drop for Lab {
    fn drop(&mut self) {
        for (_, c) in &mut self.procs {
            let _ = c.kill();
            let _ = c.wait();
        }
        for pid in self.session_hosts() {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL)
            };
            #[cfg(windows)]
            let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &pid.to_string()]).output();
        }
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.root);
        } else {
            eprintln!("실험실 기록을 남겼다: {}", self.root.display());
        }
    }
}

/// 흐름 사건을 따라 읽는다(`aaa events --follow --json`, cursor 0부터라 따라 읽기 전에 난 사건도 본다).
/// 기다림은 사건이 오는 것으로 끝난다. 시간은 오지 않을 때 멈추는 안전 상한일 뿐이다(polling하지 않는다).
pub struct Follow {
    rx: std::sync::mpsc::Receiver<Value>,
    /// 지금까지 읽은 사건(순서대로)
    pub seen: Vec<Value>,
}

impl Follow {
    /// `f`에 맞는 사건이 올 때까지 읽는다. 이미 읽은 사건은 다시 보지 않는다.
    pub fn until(&mut self, what: &str, timeout: Duration, f: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(e) => {
                    self.seen.push(e.clone());
                    if f(&e) {
                        return e;
                    }
                }
                Err(_) => panic!("{timeout:?} 안에 사건 {what}이 오지 않았다\n읽은 사건: {:#?}", self.seen),
            }
        }
    }

    /// 종류(`kind`)와 thread로 기다린다
    pub fn kind(&mut self, kind: &str, thread: &str, timeout: Duration) -> Value {
        self.until(&format!("{kind} {thread}"), timeout, |e| e["kind"] == kind && e["thread"] == thread)
    }

    pub fn kinds(&self) -> Vec<String> {
        self.seen.iter().map(|e| e["kind"].as_str().unwrap_or_default().to_string()).collect()
    }
}

impl Lab {
    /// 프로젝트의 흐름 사건을 따라 읽는 프로세스를 띄운다(끝나면 다른 프로세스처럼 이 실험실이 정리한다)
    pub fn follow(&mut self, tag: &str, node: &Node, project: &str) -> Follow {
        let (_, out) = self.spawn_piped(tag, node, &["events", project, "--follow", "--json"]);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in out.lines() {
                let Ok(line) = line else { break };
                if let Ok(v) = serde_json::from_str::<Value>(&line)
                    && tx.send(v).is_err()
                {
                    break;
                }
            }
        });
        Follow { rx, seen: Vec::new() }
    }
}

/// host가 front 연결로 밀어 주는 사건(thread 기록·질문·흐름 사건·pane 목록)을 받아 조건이 맞을 때까지 기다린다.
/// 시간마다 다시 묻지 않고 사건이 올 때마다 조건을 본다. 받은 사건은 모두 남겨 두고 기다릴 때마다 처음부터 다시 본다
/// (두 사건이 어느 순서로 와도 차례로 기다릴 수 있게). 조건에는 시각·ID처럼 그 사건만 고르는 것을 넣는다.
pub struct Events {
    rt: tokio::runtime::Runtime,
    client: aaa_common::front::FrontClient,
    log: Vec<aaa_proto::FrontEvent>,
}

impl Events {
    pub fn connect(node: &Node) -> Events {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let client = rt.block_on(aaa_common::front::FrontClient::connect(&node.state)).expect("front 연결");
        Events { rt, client, log: Vec::new() }
    }

    /// `f`가 Some을 내는 첫 사건(지금까지 받은 것부터)
    pub fn wait<T>(&mut self, what: &str, timeout: Duration, f: impl FnMut(&aaa_proto::FrontEvent) -> Option<T>) -> T {
        self.wait_from(0, what, timeout, f).1
    }

    /// `from`번째 받은 사건부터 본다. (그 사건의 번호, 값)을 돌려준다: 다음에는 번호 + 1부터 보면 사건을 차례로 받는다.
    pub fn wait_from<T>(
        &mut self,
        from: usize,
        what: &str,
        timeout: Duration,
        mut f: impl FnMut(&aaa_proto::FrontEvent) -> Option<T>,
    ) -> (usize, T) {
        if let Some((i, v)) = self.log.iter().enumerate().skip(from).find_map(|(i, e)| f(e).map(|v| (i, v))) {
            return (i, v);
        }
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let client = &mut self.client;
            let got = self.rt.block_on(async { tokio::time::timeout_at(deadline, client.next()).await });
            let e = match got {
                Ok(Ok(Some(aaa_proto::FrontMsg::Event(e)))) => e,
                Ok(Ok(Some(_))) => continue,
                Ok(Ok(None)) => panic!("{what}: host 연결이 끊겼다"),
                Ok(Err(e)) => panic!("{what}: {e:#}"),
                Err(_) => panic!("{timeout:?} 안에 {what} 사건이 오지 않았다. 받은 사건:\n{:#?}", self.log),
            };
            let v = f(&e);
            self.log.push(e);
            if let Some(v) = v {
                return (self.log.len() - 1, v);
            }
        }
    }
}

/// 셸 낱말 하나로 감싼다(Linux `script -c` 명령줄)
pub fn shell_word(w: &str) -> String {
    format!("'{}'", w.replace('\'', "'\\''"))
}

/// 지금 시각(ms). 사건 조건에 「이 뒤에 생긴」을 넣을 때 쓴다(agent·host가 같은 시계로 적는다).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64
}

/// 조건이 참이 될 때까지 기다린다. 상태 전이를 관측하는 대기이지 실패한 동작의 재시도가 아니다.
pub fn wait_for<T>(what: &str, timeout: Duration, f: impl FnMut() -> Option<T>) -> T {
    wait_for_diag(what, timeout, f, String::new)
}

/// 시간이 지나면 `diag`가 모은 상태(pane·화면·파일)를 함께 보여 준다.
pub fn wait_for_diag<T>(
    what: &str,
    timeout: Duration,
    mut f: impl FnMut() -> Option<T>,
    diag: impl FnOnce() -> String,
) -> T {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(v) = f() {
            return v;
        }
        if Instant::now() >= deadline {
            panic!("{timeout:?} 안에 {what}이 되지 않았다\n{}", diag());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 실험실 아래 모든 파일 목록(크기)과 작은 JSON 파일 내용. 실패 진단용.
pub fn tree(root: &Path) -> String {
    let mut out = String::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(list) = std::fs::read_dir(&dir) else { continue };
        for e in list.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let len = e.metadata().map(|m| m.len()).unwrap_or(0);
            out.push_str(&format!("{} ({len})\n", p.strip_prefix(root).unwrap_or(&p).display()));
            if p.extension().is_some_and(|x| x == "json") && len < 2000 {
                out.push_str(&format!("    {}\n", std::fs::read_to_string(&p).unwrap_or_default().replace('\n', " ")));
            }
        }
    }
    out
}

pub fn read_line_containing(out: &mut impl BufRead, needle: &str) -> String {
    let mut line = String::new();
    loop {
        line.clear();
        let n = out.read_line(&mut line).unwrap();
        assert!(n > 0, "출력이 끝날 때까지 {needle:?}이 없었다");
        if line.contains(needle) {
            return line;
        }
    }
}

pub fn path_str(p: &Path) -> &str {
    p.to_str().unwrap()
}
