//! 별도 process의 실제 host/agent backend. 외부 SCM RUNNING 보고만 관찰로 바꾼다.
use super::*;
use std::io::{BufRead, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const OWNER: &str = "install::engine::supervisor::tests::lifecycle::offline_backend_initializes_and_stops";

struct Role {
    child: Child,
    events: mpsc::Receiver<String>,
    stderr: Option<std::thread::JoinHandle<()>>,
}
impl Role {
    fn start(spec: &Spec, context: &Path) -> Self {
        private::directory(&spec.directory()).unwrap();
        private::write(context, &serde_json::to_vec(spec).unwrap()).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", OWNER, "--nocapture"]).env_clear();
        for key in ["PATH", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        for key in ["HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA"] {
            command.env(key, &spec.profile);
        }
        command
            .env("AAA_CONFIG_DIR", &spec.config)
            .env("AAA_STATE_DIR", &spec.state)
            .env("AAA_I5_OWNER_CONTEXT", context)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().unwrap();
        let (tx, events) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let _ = tx.send(line);
            }
        });
        let stderr = child.stderr.take().unwrap();
        let stderr = Some(std::thread::spawn(move || {
            for line in std::io::BufReader::new(stderr).lines().map_while(std::result::Result::ok) {
                eprintln!("service child: {line}");
            }
        }));
        Self { child, events, stderr }
    }
    fn running(&self) {
        let end = Instant::now() + Duration::from_secs(40);
        loop {
            let line = self
                .events
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .expect("SCM RUNNING은 실제 backend 초기화 뒤에 와야 한다");
            if line == "SCM_RUNNING" {
                return;
            }
            assert!(!line.starts_with("SERVICE_ERROR"), "{line}");
        }
    }
    fn rejected(&mut self) {
        let end = Instant::now() + Duration::from_secs(40);
        loop {
            let line =
                self.events.recv_timeout(end.saturating_duration_since(Instant::now())).expect("backend 실패 사건");
            assert_ne!(line, "SCM_RUNNING", "listener를 열지 못한 backend를 RUNNING으로 보고했다");
            if line == "SERVICE_ERROR service_backend_not_ready" {
                break;
            }
        }
        assert!(!self.child.wait().unwrap().success());
    }
    fn stop(&mut self) {
        self.child.stdin.take().unwrap().write_all(b"stop\n").unwrap();
        let end = Instant::now() + Duration::from_secs(5);
        loop {
            let line = self
                .events
                .recv_timeout(end.saturating_duration_since(Instant::now()))
                .expect("stop이 backend를 끝내야 한다");
            if line == "SERVICE_STOPPED" {
                break;
            }
            assert!(!line.starts_with("SERVICE_ERROR"), "{line}");
        }
        assert!(self.child.wait().unwrap().success());
    }
}
impl Drop for Role {
    fn drop(&mut self) {
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.write_all(b"stop\n");
        }
        // 소유한 자식만 정리한다. 정상 owner는 위 stop 사건으로 끝나고 이 경로는 실패 정리용이다.
        let end = Instant::now() + Duration::from_secs(5);
        while self.child.try_wait().unwrap().is_none() {
            if Instant::now() >= end {
                self.child.kill().unwrap();
                self.child.wait().unwrap();
                break;
            }
            std::thread::yield_now();
        }
        if let Some(stderr) = self.stderr.take() {
            stderr.join().unwrap();
        }
    }
}

#[test]
fn offline_backend_initializes_and_stops() {
    if let Some(path) = std::env::var_os("AAA_I5_OWNER_CONTEXT") {
        let spec: Spec = serde_json::from_slice(&private::read(Path::new(&path), 128 * 1024).unwrap()).unwrap();
        let (stop, _) = tokio::sync::watch::channel(false);
        let input_stop = stop.clone();
        std::thread::spawn(move || {
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line);
            let _ = input_stop.send(true);
        });
        // Session 0·실효 token은 외부 OS 경계다. 실제 backend/ready 파일은 모형으로 대체하지 않는다.
        match super::super::runner::run(&spec, 0, false, u64::from(std::process::id()), stop, || {
            println!("SCM_RUNNING");
            std::io::stdout().flush().unwrap();
            true
        }) {
            Ok(()) => println!("SERVICE_STOPPED"),
            Err(e) => {
                println!("SERVICE_ERROR {}", e.code);
                std::io::stdout().flush().unwrap();
                panic!("서비스 준비 실패");
            }
        }
        return;
    }
    // Unix IPC의 주소 길이 한도 때문에 사건 owner는 기존 backend lab처럼 짧은 임시 root를 쓴다.
    let parent = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let root = parent.canonicalize().unwrap().join(format!("aai5-{}", aaa_common::util::new_id()));
    private::directory(&root).unwrap();
    let lab = Lab(root);
    for name in ["config", "state", "work"] {
        private::directory(&lab.0.join(name)).unwrap();
    }
    private::write(&lab.0.join("config/agent.toml"), format!(
        "schema = 1\n[agent]\nname = 'mac-a'\nhost = 'local'\nwork_root = {:?}\n[selfcheck]\nfirst_exec_samples = 0\n[health]\nmin_free_gb = 0\n", lab.0.join("work")
    ).as_bytes()).unwrap();
    private::write(&lab.0.join("config/host.toml"), format!(
        "schema = 1\n[host]\nagent_listen = '127.0.0.1:0'\n[web]\nlisten = '127.0.0.1:0'\n[local_agent]\nmode = 'windows_scm'\n[projects]\nroot = {:?}\n", lab.0.join("projects")
    ).as_bytes()).unwrap();
    let executable = std::env::current_exe().unwrap().canonicalize().unwrap();
    let spec = Spec {
        schema: 1,
        id: aaa_common::util::new_id(),
        role: InstallRole::Agent,
        instance: None,
        account: crate::install::platform::account().unwrap(),
        profile: lab.0.clone(),
        binary_sha256: hash(&std::fs::read(&executable).unwrap()),
        executable,
        config: lab.0.join("config"),
        state: lab.0.join("state"),
        environment: BTreeMap::new(),
        automatic: true,
    };
    let mut agent = Role::start(&spec, &lab.0.join("agent-context.json"));
    agent.running();
    let proof = spec.directory().join("ready.json");
    let first: Ready = serde_json::from_slice(&private::read(&proof, 128 * 1024).unwrap()).unwrap();
    assert!(!first.connected, "host가 없어도 backend는 실행하며 연결 성공은 따로 표시한다");
    let mut os = Os {
        config: Some(Configuration::desired(&spec)),
        pid: agent.child.id(),
        verified_pids: vec![agent.child.id()],
        running: true,
        ..Default::default()
    };
    let observed = os.query(&spec).unwrap().unwrap();
    assert_eq!(verify_ready(&spec, &mut os, &observed).unwrap_err().code, "service_ready_unverified");
    let mut host_spec = spec.clone();
    host_spec.role = InstallRole::Host;
    let mut host = Role::start(&host_spec, &lab.0.join("host-context.json"));
    host.running();
    os.verified_pids.push(host.child.id());
    agent.running(); // 실제 Welcome/replay 뒤 connection proof가 갱신되고 SCM 상태도 보고한다.
    let valid = private::read(&proof, 128 * 1024).unwrap();
    let ready: Ready = serde_json::from_slice(&valid).unwrap();
    assert!(ready.connected);
    verify_ready(&spec, &mut os, &observed).unwrap();
    // 유효 영수증은 fixture가 쓰지 않고 위 실제 backend가 만들었다. 각 독립 경계를 변조해 거부를 본다.
    for field in [
        "schema",
        "context_sha256",
        "account_sha256",
        "profile_sha256",
        "binary_sha256",
        "pid",
        "session",
        "boot",
        "connected",
        "elevated",
        "process_created",
    ] {
        let mut altered: serde_json::Value = serde_json::from_slice(&valid).unwrap();
        altered[field] = match field {
            "schema" => serde_json::json!(2),
            "pid" => serde_json::json!(host.child.id()),
            "session" => serde_json::json!(1),
            "connected" => serde_json::json!(false),
            "elevated" => serde_json::json!(true),
            "process_created" => serde_json::json!(first.process_created + 1),
            _ => serde_json::json!("different"),
        };
        if field == "pid" {
            altered["process_created"] = serde_json::json!(u64::from(host.child.id()));
        }
        let bytes = serde_json::to_vec(&altered).unwrap();
        private::write(&proof, &bytes).unwrap();
        assert_eq!(verify_ready(&spec, &mut os, &observed).unwrap_err().code, "service_ready_unverified", "{field}");
        assert_eq!(private::read(&proof, 128 * 1024).unwrap(), bytes, "조회가 영수증을 고치지 않는다");
    }
    private::write(&proof, &valid).unwrap();
    host.stop();
    agent.running();
    assert!(!serde_json::from_slice::<Ready>(&private::read(&proof, 128 * 1024).unwrap()).unwrap().connected);
    assert_eq!(verify_ready(&spec, &mut os, &observed).unwrap_err().code, "service_ready_unverified");
    assert!(agent.child.try_wait().unwrap().is_none(), "host 단절이 agent 서비스를 종료하지 않는다");
    let mut host = Role::start(&host_spec, &lab.0.join("host-context.json"));
    host.running();
    agent.running();
    verify_ready(&spec, &mut os, &observed).unwrap();
    agent.stop();
    assert!(!proof.exists(), "정상 stop 뒤 과거 ready가 남지 않는다");
    assert!(host.child.try_wait().unwrap().is_none(), "agent stop은 host 서비스를 끝내지 않는다");
    host.stop();
    // 실제 listener 충돌은 준비 사건을 낼 수 없다. 고정 sleep/시험 전용 성공 hook은 없다.
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    private::write(&host_spec.config.join("host.toml"), format!(
        "schema = 1\n[host]\nagent_listen = '{}'\n[web]\nlisten = '127.0.0.1:0'\n[local_agent]\nmode = 'windows_scm'\n[projects]\nroot = {:?}\n", occupied.local_addr().unwrap(), lab.0.join("projects")
    ).as_bytes()).unwrap();
    let mut blocked = Role::start(&host_spec, &lab.0.join("host-context.json"));
    blocked.rejected();
    assert!(!host_spec.directory().join("ready.json").exists());
}
