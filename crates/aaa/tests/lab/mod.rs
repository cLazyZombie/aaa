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

use aaa_common::child_process::StdCommandExt;
use serde_json::Value;

pub mod watch;

pub const BIN: &str = env!("CARGO_BIN_EXE_aaa");
/// t-0578의 동시 검증(load 28.96)에서 30초를 넘긴 시작·전달 대기만 두 배 여유를 둔다.
/// 사건이 오면 즉시 끝난다. 일반 대기·제품의 복구/성능 시한에는 쓰지 않는다(docs/testing.md).
pub const LOAD_TIMEOUT: Duration = Duration::from_secs(60);
static SEQ: AtomicU32 = AtomicU32::new(0);
static CAPTURE_SEQ: AtomicU32 = AtomicU32::new(0);

/// `Command::output()` 대신 쓴다: 표준 출력·오류를 pipe가 아니라 파일로 받고, 자식이 끝나면 바로 돌아온다.
/// 생성은 제품과 같은 aaa-common 잠금을 쓰고 기다리기는 잠금 밖이다. 파일 출력 정책은 아래 손자 보존 회귀도 지킨다.
/// macOS std는 pipe를 만든 뒤 CLOEXEC를 따로 건다(`pipe2`가 없다). 그 틈에 같은 시험 프로세스의 다른 thread가 띄운 자식
/// (host·agent처럼 오래 사는 것)이 쓰기 끝을 물려받으면, 이 명령이 끝나도 EOF가 오지 않아 `output()`이 그 자식이 끝날 때까지
/// 막힌다(t-1097: m13 accounts 「labA 연결」 30초 실패, host는 1초 안에 연결했다). 파일은 `O_CLOEXEC`로 원자적으로 열린다.
pub fn captured(cmd: &mut Command) -> std::io::Result<Output> {
    let stem = format!("aaa-lab-out-{}-{}", std::process::id(), CAPTURE_SEQ.fetch_add(1, Ordering::SeqCst));
    let (out, err) =
        (std::env::temp_dir().join(format!("{stem}.out")), std::env::temp_dir().join(format!("{stem}.err")));
    let result = (|| {
        let status = cmd
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(&out)?)
            .stderr(std::fs::File::create(&err)?)
            .status_guarded()?;
        Ok(Output { status, stdout: std::fs::read(&out)?, stderr: std::fs::read(&err)? })
    })();
    for path in [&out, &err] {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// 같은 프로그램·version이면서 SHA가 다른 시험 실행 파일. macOS는 서명 식별자로 구분한다.
/// 실행 파일을 쓰는 FD가 병렬 시험의 fork에 물려가지 않도록 복사·수정은 자식 프로세스에서 한다.
#[cfg(unix)]
pub fn variant_binary(source: &Path, target: &Path, variant: &str) {
    assert!(Command::new("cp").args([source, target]).status_guarded().unwrap().success());
    #[cfg(target_os = "macos")]
    for args in [vec!["--force", "--sign", "-", "--identifier", variant], vec!["--verify", "--strict"]] {
        let out = Command::new("codesign").args(args).arg(target).capture_output().unwrap();
        assert!(out.status.success(), "시험 실행 파일 서명: {}", String::from_utf8_lossy(&out.stderr));
    }
    #[cfg(not(target_os = "macos"))]
    assert!(
        Command::new("/bin/sh")
            .args(["-c", r#"printf '%s' "$1" >> "$2""#, "fixture", variant])
            .arg(target)
            .status_guarded()
            .unwrap()
            .success()
    );
}

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
        self.cmd_bin(Path::new(BIN), args)
    }
    /// 업데이트 경계 시험은 공용 binary 대신 실험실 사본을 실행한다.
    pub fn cmd_bin(&self, bin: &Path, args: &[&str]) -> Command {
        let mut c = Command::new(bin);
        c.args(args)
            .env("AAA_CONFIG_DIR", &self.cfg)
            .env("AAA_STATE_DIR", &self.state)
            .env("RUST_LOG", "aaa=info")
            // 기본 profile(claude·codex) 목록이 이 머신의 설치·로그인에 따라 바뀌지 않게(보는 시험만 채운다)
            .env("AAA_TEST_BUILTINS", "")
            // 가짜 harness의 출력 준비는 각 시험의 PTY 표지로 확인한다. 시작 화면을
            // 숨기는 실제 3초 경계는 detect::state의 시각 주입 owner가 검사한다.
            .env("AAA_TEST_DETECT_GRACE_MS", "0")
            // ticker도 사용량을 읽는다. 실제 CLI 로그인·keychain·외부 구독에 닿지 않는다.
            .env("AAA_USAGE_HOME", self.cfg.join("usage-home"))
            .env_remove("AAA_USAGE_ORIGIN")
            // routine `every <N>s`: host가 시험에서만 받는다
            .env("AAA_TEST_ROUTINE_SECONDS", "1")
            // aaa pane에서 시험을 돌려도 실험실 CLI가 운영 thread의 호출자·hook을 물려받지 않는다.
            // worker 권한을 보는 시험은 자기 실험실의 값을 다시 넣는다.
            .env_remove("AAA_SESSION_ID")
            .env("AAA_TEST_AUTOMATION_CLOCK_FILE", self.cfg.join("automation-clock"))
            .env_remove("AAA_PROJECT")
            .env_remove("AAA_THREAD")
            .env_remove("AAA_HOOK_SOCK")
            .env_remove("AAA_HOOK_TOKEN")
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
        captured(&mut self.cmd(args)).expect("aaa 실행")
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

    /// 사람이 터미널(aaa 세션 밖)에서 친 것처럼 실행한다. 직접 연 PTY를 EOF까지 읽고 자식 종료를 기다린다.
    /// 중간 전달 프로세스(script)는 자식 종료를 먼저 보면 PTY에 남은 출력을 버릴 수 있다.
    #[cfg(unix)]
    pub fn in_terminal(&self, args: &[&str], answers: &[(&str, &str)], timeout: Duration) -> String {
        self.in_terminal_env(args, &[], answers, timeout)
    }

    /// `in_terminal`과 같되 환경 변수를 더 준다(예: 가짜 `launchctl`을 PATH 앞에, HOME을 실험실에)
    #[cfg(unix)]
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
    #[cfg(unix)]
    pub fn in_terminal_bin(
        &self,
        bin: &Path,
        args: &[&str],
        env: &[(&str, &str)],
        answers: &[(&str, &str)],
        timeout: Duration,
    ) -> String {
        let mut c = terminal_command(bin);
        // script와 같은 cwd를 쓴다. PTY 기본값인 HOME은 시험에서 아직 없는 격리 폴더일 수 있다.
        c.cwd(std::env::current_dir().unwrap());
        c.args(args);
        c.env("AAA_CONFIG_DIR", &self.cfg);
        c.env("AAA_STATE_DIR", &self.state);
        c.env("AAA_TEST_BUILTINS", "");
        // ticker도 실험실 안 빈 사용량 폴더만 본다.
        c.env("AAA_USAGE_HOME", self.cfg.join("usage-home"));
        for key in
            ["AAA_USAGE_ORIGIN", "AAA_SESSION_ID", "AAA_HOOK_SOCK", "AAA_HOOK_TOKEN", "AAA_PROJECT", "AAA_THREAD"]
        {
            c.env_remove(key);
        }
        for (key, value) in env {
            c.env(key, value);
        }
        Terminal::spawn(c).read(args, answers, timeout)
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

    /// `name` 머신이 연결될 때까지 `machine list`로 기다린다. 시간이 지나면 시도마다의 시각·걸린 시간·종료·그 머신 상태·
    /// 오류 첫 줄을 남긴다(CLI 오류를 None으로 버리면 host 연결과 판정이 어긋난 까닭을 가를 수 없다, t-1097)
    pub fn wait_connected(&self, name: &str, timeout: Duration) {
        let tries = std::cell::RefCell::new(Vec::<String>::new());
        let start = Instant::now();
        wait_for_diag(
            &format!("{name} 연결"),
            timeout,
            || {
                let at = start.elapsed();
                let o = self.run(&["--json", "machine", "list"]);
                let v: Option<Value> = serde_json::from_slice(&o.stdout).ok();
                let state = v.as_ref().and_then(|v| {
                    v["machines"].as_array()?.iter().find(|m| m["name"] == name).map(|m| m["connection"].to_string())
                });
                let err = String::from_utf8_lossy(&o.stderr);
                tries.borrow_mut().push(format!(
                    "+{}ms {}ms exit={:?} {name}={} {}",
                    at.as_millis(),
                    start.elapsed().saturating_sub(at).as_millis(),
                    o.status.code(),
                    state.as_deref().unwrap_or("없음"),
                    err.lines().next().unwrap_or("")
                ));
                (o.status.success() && state.as_deref() == Some("\"connected\"")).then_some(())
            },
            || {
                let t = tries.borrow();
                format!("machine list 시도 {}번(마지막 10번):\n{}", t.len(), t[t.len().saturating_sub(10)..].join("\n"))
            },
        )
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
        // agent 시작이 작업 root의 쓰기 점검도 한다. 설정 없는 실험실도 실제 HOME에 쓰지 않는다.
        std::fs::write(
            node.cfg.join("agent.toml"),
            format!("schema = 1\n[agent]\nwork_root = {:?}\n", dir.join("work")),
        )
        .unwrap();
        // 시험 셸 prompt(`Node::cmd`의 `ENV`): 기계 hostname·rc와 상관없이 같다
        #[cfg(unix)]
        std::fs::write(node.cfg.join("shrc"), "PS1='$ '\n").unwrap();
        if let Some(t) = host_toml {
            // projects root를 주지 않은 시험 host도 실제 HOME의 프로젝트를 세지 않게(한 host 한 프로젝트 시작 검사, plan §8.283)
            let t = if t.contains("[projects]") {
                t.to_string()
            } else {
                format!("{t}\n[projects]\nroot = {:?}\n", dir.join("projects"))
            };
            std::fs::write(node.cfg.join("host.toml"), t).unwrap();
        }
        node
    }

    pub fn spawn(&mut self, tag: &str, node: &Node, args: &[&str]) -> u32 {
        self.spawn_env(tag, node, args, &[])
    }

    pub fn spawn_env(&mut self, tag: &str, node: &Node, args: &[&str], env: &[(&str, &str)]) -> u32 {
        let mut cmd = node.cmd(args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        self.spawn_command(tag, cmd)
    }
    pub fn spawn_command(&mut self, tag: &str, mut cmd: Command) -> u32 {
        let log = std::fs::File::create(self.root.join(format!("{tag}.log"))).unwrap();
        let child = cmd.stdout(log.try_clone().unwrap()).stderr(log).spawn_guarded().expect("spawn");
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
        let mut child = node.cmd(args).stdout(Stdio::piped()).stderr(log).spawn_guarded().expect("spawn");
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
            let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &pid.to_string()]).capture_output();
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
    let o = captured(Command::new("ps").args(["-o", "command=", "-p", &pid.to_string()])).ok()?;
    let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Windows: 명령줄 대신 그 PID의 실행 파일 이름이 aaa인지 본다(CI 전용 임시 runner).
#[cfg(windows)]
fn command_of(pid: u32) -> Option<String> {
    let filter = format!("PID eq {pid}");
    let o = Command::new("tasklist").args(["/FI", &filter, "/FO", "CSV", "/NH"]).capture_output().ok()?;
    let s = String::from_utf8_lossy(&o.stdout).to_string();
    s.to_ascii_lowercase().contains("\"aaa.exe\"").then_some(s)
}

impl Drop for Lab {
    fn drop(&mut self) {
        for (_, c) in &mut self.procs {
            let _ = c.kill();
            let _ = c.wait();
        }
        // 독립 배포 감시는 agent의 Child가 아니다. 기록 PID와 실제 명령의 실험실 경로를 함께 확인한다.
        #[cfg(unix)]
        if let Ok(nodes) = std::fs::read_dir(&self.root) {
            for node in nodes.flatten() {
                let state = node.path().join("s");
                let Ok(records) = std::fs::read_dir(state.join("agent/deployments")) else { continue };
                for entry in records.flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".guard.json")) {
                    let Ok(bytes) = std::fs::read(entry.path()) else { continue };
                    let Ok(view) = serde_json::from_slice::<Value>(&bytes) else { continue };
                    let Some(pid) = view["worker_pid"].as_u64().and_then(|p| u32::try_from(p).ok()) else { continue };
                    if command_of(pid)
                        .is_some_and(|c| c.contains("deployment-guard") && c.contains(&*state.to_string_lossy()))
                    {
                        unsafe {
                            libc::kill(pid as i32, libc::SIGKILL);
                        }
                    }
                }
            }
        }
        for pid in self.session_hosts() {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL)
            };
            #[cfg(windows)]
            let _ = Command::new("taskkill").args(["/F", "/T", "/PID", &pid.to_string()]).capture_output();
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

    /// 지금까지 읽은 사건 다음 위치. 새 동작의 결과를 기다릴 때 앞 턴의 기록을 다시 고르지 않는다.
    pub fn cursor(&self) -> usize {
        self.log.len()
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

/// 시험 CLI를 직접 실행한다. PTY 준비와 수집을 나눠 늦은 reader 순서도 검증한다.
#[cfg(unix)]
pub fn terminal_command(bin: &Path) -> portable_pty::CommandBuilder {
    #[cfg(target_os = "macos")]
    {
        // XNU의 cttyopen이 session의 S_CTTYREF를 잡아야 stdio close가 미수집 출력을
        // flush하지 않는다. exec 뒤에도 이 참조는 남아 종료 때 master의 drain을 기다린다.
        let mut command = portable_pty::CommandBuilder::new("/bin/sh");
        command.env_remove("BASH_ENV");
        command.env_remove("ENV");
        command.args(["-c", "exec 3<>/dev/tty || exit 125; exec 3>&-; exec \"$@\"", "aaa-test-pty"]);
        command.arg(bin);
        command
    }
    #[cfg(not(target_os = "macos"))]
    portable_pty::CommandBuilder::new(bin)
}

#[cfg(unix)]
pub struct Terminal {
    master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}
#[cfg(unix)]
impl Terminal {
    pub fn spawn(command: portable_pty::CommandBuilder) -> Self {
        use portable_pty::{PtySize, native_pty_system};
        let pair = aaa_common::child_process::with_spawn_lock(|| native_pty_system().openpty(PtySize::default()))
            .expect("시험 PTY 열기");
        let child =
            aaa_common::child_process::with_spawn_lock(|| pair.slave.spawn_command(command)).expect("시험 CLI 실행");
        // 부모의 slave 사본이 남으면 자식이 끝나도 master에 EOF가 오지 않는다.
        drop(pair.slave);
        Self { master: pair.master, child }
    }
    pub fn read(mut self, args: &[&str], answers: &[(&str, &str)], timeout: Duration) -> String {
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let mut out = self.master.try_clone_reader().unwrap();
        let s2 = seen.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut b = [0u8; 256];
            let result = loop {
                match std::io::Read::read(&mut out, &mut b) {
                    Ok(0) => break Ok(()),
                    Ok(n) => s2.lock().unwrap().extend_from_slice(&b[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) => break Err(e),
                }
            };
            let _ = done_tx.send(result);
        });
        let text = || String::from_utf8_lossy(&seen.lock().unwrap()).to_string();
        let mut stdin = self.master.take_writer().unwrap();
        let mut sent = Vec::new();
        for (prompt, typed) in answers {
            wait_for_diag(&format!("물음 {prompt:?}"), timeout, || text().contains(prompt).then_some(()), text);
            std::io::Write::write_all(&mut stdin, format!("{typed}\n").as_bytes()).unwrap();
            sent.push((*prompt, typed.len() + 1));
        }
        match done_rx.recv_timeout(timeout) {
            Ok(Ok(())) => {}
            failed => {
                let status = self.child.try_wait();
                let cleanup =
                    if matches!(status, Ok(None)) { Some((self.child.kill(), self.child.wait())) } else { None };
                panic!(
                    "{args:?} PTY 출력이 {timeout:?} 안에 끝나지 않았다: {failed:?}, pid={:?}, child={status:?}, prompt/write={sent:?}, cleanup={cleanup:?}\n{}",
                    self.child.process_id(),
                    text()
                );
            }
        }
        let status = self.child.wait().unwrap();
        drop(stdin);
        reader.join().unwrap();
        let output = text();
        if output.is_empty() {
            eprintln!("PTY 종료: {args:?}, {status:?}, EOF까지 읽은 출력 0바이트");
        }
        output
    }
}
