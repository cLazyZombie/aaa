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
            .env_remove("NO_COLOR")
            .stdin(Stdio::null());
        c
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().expect("aaa 실행")
    }

    /// `--json`을 붙여 실행하고 JSON을 돌려준다. 실패하면 Err(JSON 또는 출력).
    pub fn json(&self, args: &[&str]) -> Result<Value, String> {
        let mut a = args.to_vec();
        let at = a.iter().position(|x| *x == "--").unwrap_or(a.len());
        a.insert(at, "--json");
        let o = self.run(&a);
        let text = String::from_utf8_lossy(&o.stdout).to_string();
        match serde_json::from_str::<Value>(&text) {
            Ok(v) if o.status.success() => Ok(v),
            Ok(v) => Err(v.to_string()),
            Err(_) => Err(format!("{text}{}", String::from_utf8_lossy(&o.stderr))),
        }
    }

    pub fn ok(&self, args: &[&str]) -> Value {
        self.json(args).unwrap_or_else(|e| panic!("aaa {} 실패: {e}", args.join(" ")))
    }

    pub fn screen(&self, pane: &str) -> String {
        let v = self.ok(&["pane", "read", pane]);
        v["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect::<Vec<_>>().join("\n")
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
        let (_, out) = self.spawn_piped(tag, node, &["events", "--follow", "--json", "--project", project]);
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
