//! 전역 설치 CLI 경계. 운영 HOME·서비스·프로세스 신호를 쓰지 않는다.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

// Mac에서 교차 빌드한 시험 binary를 Windows 임시 폴더에서 실행할 때만 경로를 지정한다.
fn bin() -> PathBuf {
    std::env::var_os("AAA_TEST_BIN").map(PathBuf::from).unwrap_or_else(|| env!("CARGO_BIN_EXE_aaa").into())
}

/// 화면 감지 → Agent link → host 질문 → 공통 Front 답변 → 실제 전송 바이트.
/// CLI·웹·휴대폰은 같은 QuestionAnswer 요청을 쓰며 시험은 프로세스 신호 없이 끝난다.
#[tokio::test]
async fn codex_trust_detection_question_and_answer_share_the_front_path() {
    use aaa_agent::detect::{self, DetectionInput, Harness, State};
    use aaa_common::{
        conn::{FrameRx, spawn_writer},
        ipc, util,
    };
    use aaa_proto::*;
    for (choice, selected_quit, expected) in
        [("yes", false, b"\r".as_slice()), ("yes", true, b"\x1b[A\r".as_slice()), ("no", false, b"\x1b[B\r".as_slice())]
    {
        let home = Home::new();
        let mut cfg = aaa_config::HostConfig::default();
        cfg.host.agent_listen.clear();
        cfg.projects.root = home.0.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let host = aaa_host::Host::open(cfg, home.0.join("host")).unwrap();
        aaa_host::link::start(&host).await.unwrap();
        let sample = include_str!("../../aaa-agent/src/detect/fixtures/codex-trust.txt");
        let screen =
            if selected_quit { sample.replace("› 1.", "  1.").replace("  2.", "› 2.") } else { sample.to_string() };
        let detected =
            detect::detect(Harness::Codex, DetectionInput { screen: &screen, osc_title: "", osc_progress: "" });
        assert_eq!(detected.state, State::Blocked);
        let (kind, text) = detect::question_of(detected.rule.as_deref(), &screen);
        let event = HarnessEvent {
            session: "s".into(),
            harness: "codex".into(),
            kind: "question".into(),
            message: Some(text),
            question_kind: Some(kind.into()),
            at_ms: util::now_ms(),
            harness_session: None,
            percent: None,
            transcript: None,
            plan: None,
            background: None,
        };
        let mut questions = host.question_events.subscribe();
        let stream = ipc::connect(&aaa_host::link::local_socket(&host)).await.unwrap();
        let (reader, writer) = tokio::io::split(stream);
        let tx = spawn_writer(writer, MAX_FRAME);
        let mut rx = FrameRx::new(reader, MAX_FRAME);
        tx.json(&LinkMsg::Hello(AgentHello {
            proto: AGENT_LINK,
            machine_id: "lab-agent".into(),
            machine_name: "mac-a".into(),
            boot_id: util::boot_id(),
            os: std::env::consts::OS.into(),
            arch: "test".into(),
            software: "test".into(),
            features: vec!["terminal.v1".into()],
            dev_mode: true,
            selfcheck: None,
            harness: vec![event],
            sessions: vec![SessionInfo {
                id: "s".into(),
                name: "codex".into(),
                argv: vec!["codex".into()],
                cwd: None,
                pid: None,
                started_at_ms: util::now_ms(),
                boot_id: util::boot_id(),
                cols: 80,
                rows: 24,
                state: SessionState::Running,
                role: Some(Role::Worker),
                label: None,
                harness: Some("codex".into()),
            }],
        }))
        .await
        .unwrap();
        assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Welcome(_)));
        let (inputs, mut received) = tokio::sync::mpsc::unbounded_channel();
        let fake = tokio::spawn(async move {
            while let Ok(msg) = rx.control::<LinkMsg>().await {
                match msg {
                    LinkMsg::Req { id, body, .. } => {
                        let reply = match body {
                            AgentRequest::Screen { .. } => AgentResponse::Screen(ScreenText {
                                session: "s".into(),
                                cols: 80,
                                rows: 24,
                                lines: screen.lines().map(str::to_string).collect(),
                                cursor_row: 0,
                                cursor_col: 0,
                                ansi: screen.clone(),
                            }),
                            AgentRequest::Input { data, .. } => {
                                inputs.send(data.0).unwrap();
                                AgentResponse::Done
                            }
                            _ => AgentResponse::Done,
                        };
                        tx.json(&LinkMsg::Res { id, result: Outcome::Ok(reply) }).await.unwrap();
                    }
                    LinkMsg::Ping { seq } => {
                        tx.json(&LinkMsg::Pong { seq }).await.unwrap();
                    }
                    _ => {}
                }
            }
        });
        let q = tokio::time::timeout(std::time::Duration::from_secs(5), questions.recv()).await.unwrap().unwrap();
        assert_eq!(q.question_kind, "trust");
        assert_eq!(q.options, ["yes", "no"]);
        let response = host
            .call(
                DeviceRole::Local,
                "test",
                FrontRequest::QuestionAnswer { id: q.id, choice: choice.into(), menu: None, pick: None },
            )
            .await
            .unwrap();
        assert!(matches!(response, FrontResponse::Question(q) if q.state == QuestionState::Answered));
        assert_eq!(received.recv().await.unwrap(), expected);
        fake.abort();
    }
}

struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        let base = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() };
        let p = base.join(format!("aaci-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn cmd(&self, bin: &Path) -> Command {
        let mut c = Command::new(bin);
        c.env("HOME", &self.0)
            .env("USERPROFILE", &self.0)
            .env("CODEX_HOME", &self.0)
            .env("AAA_CONFIG_DIR", self.0.join("config"))
            .env("AAA_STATE_DIR", self.0.join("state"));
        for name in ["AAA_SESSION_ID", "AAA_HOOK_SOCK", "AAA_HOOK_TOKEN", "CODEX_THREAD_ID"] {
            c.env_remove(name);
        }
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.cmd(&bin()).args(args).output().unwrap()
    }
    fn hook_command(&self, command: &str) -> Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // Codex command_runner::build_command의 기본 Windows 실행 경계와 같다.
            let shell = std::env::var_os("COMSPEC").unwrap_or_else(|| "cmd.exe".into());
            let mut c = self.cmd(Path::new(&shell));
            c.arg("/C").raw_arg(format!(r#""{command}""#));
            c
        }
        #[cfg(not(windows))]
        {
            let mut c = self.cmd(Path::new("sh"));
            c.args(["-c", command]);
            c
        }
    }
    fn ok(&self, args: &[&str]) -> Value {
        let o = self.run(args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice(&o.stdout).unwrap()
    }
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.0.join(name)).unwrap()
    }
    fn hooks(&self) -> Value {
        serde_json::from_str(&self.read("hooks.json")).unwrap()
    }
    fn files(&self) -> Vec<String> {
        let mut v: Vec<_> = std::fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn install_move_and_remove_keep_foreign_hooks_notify_and_backups() {
    let h = Home::new();
    let foreign = json!({"matcher":".*","hooks":[{"type":"command","command":"herdr session"},{"type":"command","command":"herdr-projects hook --agent codex"},{"type":"command","command":"user hook"}]});
    let original = json!({"custom":{"keep":true},"hooks":{"SessionStart":[foreign.clone()],"Stop":[{"hooks":[{"command":"user stop"}]}]}});
    let text = format!("{}\n", serde_json::to_string_pretty(&original).unwrap());
    std::fs::write(h.0.join("hooks.json"), &text).unwrap();
    let config = "# 사용자 주석\nnotify = ['user-notify', 'fixed arg']\n[features]\nhooks = false # 꺼짐\nother = true\n[profiles.demo]\nnotify = ['profile-notify']\n";
    std::fs::write(h.0.join("config.toml"), config).unwrap();
    let out = h.ok(&["--json", "integration", "install", "codex"]);
    let backups = out["backups"].as_array().unwrap();
    assert_eq!(backups.len(), 2, "원래 있던 두 설정을 백업한다: {out}");
    assert!(backups.iter().any(|p| std::fs::read_to_string(p.as_str().unwrap()).unwrap() == text));
    assert!(backups.iter().any(|p| std::fs::read_to_string(p.as_str().unwrap()).unwrap() == config));
    let v = h.hooks();
    assert_eq!(v["custom"], original["custom"]);
    assert_eq!(v["hooks"]["Stop"], original["hooks"]["Stop"]);
    assert_eq!(v["hooks"]["SessionStart"][0], foreign);
    let cfg = h.read("config.toml");
    assert!(
        cfg.contains("notify = ['user-notify', 'fixed arg']")
            && cfg.contains("notify = ['profile-notify']")
            && cfg.contains("# 사용자 주석")
            && cfg.contains("other = true"),
        "{cfg}"
    );
    let enabled: toml_edit::DocumentMut = cfg.parse().unwrap();
    assert_eq!(enabled["features"]["hooks"].as_bool(), Some(true));
    let files = h.files();
    let second = h.ok(&["--json", "integration", "install", "codex"]);
    assert_eq!(second["backups"], json!([]));
    assert_eq!(h.files(), files);
    assert_eq!(h.hooks(), v);
    assert_eq!(h.read("config.toml"), cfg);

    // 실행 파일이 옮겨도 이전 명령만 제거한다. 공백·작은따옴표·$·유니코드 경로를 실제 실행한다.
    let moved = h.0.join(if cfg!(windows) { "새 aaa ' $ tool.exe" } else { "새 aaa ' $ tool" });
    std::fs::copy(bin(), &moved).unwrap();
    let o = h.cmd(&moved).args(["--json", "integration", "install", "codex"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let mut v = h.hooks();
    let start = v["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(start.len(), 2, "이전 aaa 명령이 남았다: {v}");
    assert_eq!(start[0], foreign);
    let hook_command = start[1]["hooks"][0]["command"].as_str().unwrap();
    let o = h.hook_command(hook_command).output().unwrap();
    assert!(o.status.success() && o.stdout.is_empty() && o.stderr.is_empty(), "{o:?}");
    // 설치 뒤 사용자가 aaa와 같은 그룹에 더한 handler·matcher도 제거에서 보존한다.
    v["hooks"]["SessionStart"][1]["hooks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"type":"command","command":"later user hook"}));
    std::fs::write(h.0.join("hooks.json"), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    h.ok(&["--json", "integration", "uninstall", "codex"]);
    let v = h.hooks();
    assert_eq!(v["hooks"]["SessionStart"][0], foreign);
    assert_eq!(v["hooks"]["SessionStart"][1]["hooks"], json!([{"type":"command","command":"later user hook"}]));
    assert_eq!(v["hooks"]["UserPromptSubmit"], json!([]));
    assert_eq!(v["hooks"]["PostToolUse"], json!([]));
    assert_eq!(h.read("config.toml"), cfg, "제거는 공용 feature·notify를 고치지 않는다");
    let files = h.files();
    assert_eq!(h.ok(&["--json", "integration", "uninstall", "codex"])["backups"], json!([]));
    assert_eq!(h.files(), files);
}

#[test]
fn doctor_reports_and_fixes_missing_hooks_without_a_host() {
    let h = Home::new();
    assert_eq!(h.ok(&["--json", "doctor"])["codex_hooks"]["ok"], false);
    assert!(!h.0.join("hooks.json").exists(), "점검만으로 설치했다");
    assert_eq!(h.ok(&["--json", "doctor", "--fix"])["codex_hooks"]["ok"], true);
    let config = h.read("config.toml");
    assert_eq!(h.ok(&["--json", "doctor", "--fix"])["codex_hooks"]["backups"], json!([]));
    assert_eq!(h.read("config.toml"), config);
}

#[test]
fn foreign_or_incomplete_session_hooks_are_silent() {
    let h = Home::new();
    for args in [
        vec!["hook", "--agent", "codex"],
        vec!["hook", "--agent", "codex", "not json"],
        vec!["hook", "--agent", "codex", r#"{"hook_event_name":"SessionStart"}"#],
    ] {
        for missing in ["AAA_SESSION_ID", "AAA_HOOK_SOCK", "AAA_HOOK_TOKEN"] {
            let o = h
                .cmd(&bin())
                .args(&args)
                .env("AAA_SESSION_ID", "s1")
                .env("AAA_HOOK_SOCK", "unused")
                .env("AAA_HOOK_TOKEN", "unused")
                .env_remove(missing)
                .output()
                .unwrap();
            assert!(o.status.success() && o.stdout.is_empty() && o.stderr.is_empty(), "{missing}: {o:?}");
        }
    }
    assert!(h.files().is_empty());
}

#[test]
fn malformed_config_is_never_overwritten_or_partially_installed() {
    for (name, text) in [
        ("hooks.json", "{"),
        ("hooks.json", r#"{"hooks":{"SessionStart":{}}}"#),
        ("hooks.json", r#"{"hooks":{"PostToolUse":[{"hooks":{}}]}}"#),
        ("config.toml", "[features]\nhooks = 'bad'\n"),
        ("aaa-codex-integration.json", "{}"),
        ("config.toml", "features = false\n"),
    ] {
        let h = Home::new();
        std::fs::write(h.0.join(name), text).unwrap();
        let files = h.files();
        let o = h.run(&["integration", "install", "codex"]);
        assert!(!o.status.success(), "{name}: {o:?}");
        assert_eq!(h.read(name), text);
        assert_eq!(h.files(), files, "검증 전 쓰기가 일어났다");
    }
}

#[cfg(unix)]
#[test]
fn symlinks_and_hardlinks_are_not_replaced() {
    for hard in [false, true] {
        let h = Home::new();
        let target = h.0.join("user.json");
        let path = h.0.join("hooks.json");
        std::fs::write(&target, "{}").unwrap();
        if hard {
            std::fs::hard_link(&target, &path).unwrap();
        } else {
            std::os::unix::fs::symlink(&target, &path).unwrap();
        }
        let files = h.files();
        assert!(!h.run(&["integration", "install", "codex"]).status.success());
        assert_eq!(h.read("user.json"), "{}");
        assert_eq!(h.files(), files);
    }
}

/// 가짜 session host만 둔다. CLI→실제 agent hook→실제 host→Front 상태는 모두 제품 코드다.
/// PTY·운영 프로세스·종료 신호 없이도 token·peer·세션 ID·기다림 정리의 전송 경계를 검사한다.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn codex_hook_cli_reaches_host_and_returns_guidance() {
    use aaa_common::{
        conn::{FrameRx, spawn_writer},
        front::FrontClient,
        ipc::LocalListener,
        util,
    };
    use aaa_proto::{
        AgentActivity, AgentResponse, FrontEvent, FrontMsg, FrontRequest, MAX_FRAME, Outcome, Role, SessionInfo,
        SessionMsg, SessionRequest, SessionState, SpawnSpec, Version,
    };
    use aaa_session::files::{SessionPaths, SpecFile};
    use std::time::Duration;
    let h = Home::new();
    // 인증된 경로도 특수 문자·한글이 있는 실행 파일에서 설치한 실제 명령을 쓴다.
    let moved = h.0.join(if cfg!(windows) { "새 aaa ' $ tool.exe" } else { "새 aaa ' $ tool" });
    std::fs::copy(bin(), &moved).unwrap();
    let out = h.cmd(&moved).args(["--json", "integration", "install", "codex"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    let installed = h.hooks()["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str().unwrap().to_string();
    let a = aaa_agent::Agent::new(h.0.join("agent"), "lab-agent".into(), "mac-a".into(), true, None).unwrap();
    let mut fakes = Vec::new();
    for (sid, pid) in [("s1", std::process::id()), ("s2", 0)] {
        let paths = SessionPaths::new(&a.state, sid);
        util::ensure_private_dir(&paths.dir).unwrap();
        let spec = SpecFile {
            id: sid.into(),
            token: "test-session-secret".into(),
            boot_id: util::boot_id(),
            created_at_ms: util::now_ms(),
            label: None,
            colors: None,
            spec: SpawnSpec {
                name: "codex".into(),
                argv: vec!["codex".into()],
                cwd: None,
                env: vec![],
                cols: 80,
                rows: 24,
                role: Some(Role::Worker),
                harness: Some("codex".into()),
                profile: None,
                resolved_profile: None,
            },
        };
        util::write_json(&paths.spec(), &spec).unwrap();
        let info = SessionInfo {
            id: sid.into(),
            name: "codex".into(),
            argv: vec!["codex".into()],
            cwd: None,
            pid: Some(pid),
            // 화면 감지가 이미 시작할 수 있는 세션이다. 실제 감지 사건을 기다려 순서를 고정한다.
            started_at_ms: util::now_ms().saturating_sub(aaa_agent::detect::STARTUP_GRACE.as_millis() as u64),
            boot_id: util::boot_id(),
            cols: 80,
            rows: 24,
            state: SessionState::Running,
            role: Some(Role::Worker),
            label: None,
            harness: Some("codex".into()),
        };
        let listener = LocalListener::bind(&paths.sock).unwrap();
        fakes.push(tokio::spawn(async move {
            let stream = listener.accept_peer().await.unwrap().0;
            let (r, w) = tokio::io::split(stream);
            let tx = spawn_writer(w, MAX_FRAME);
            let mut rx = FrameRx::new(r, MAX_FRAME);
            assert!(matches!(rx.control::<SessionMsg>().await.unwrap(), SessionMsg::Hello { .. }));
            tx.json(&SessionMsg::Welcome { proto: Version { major: 1, minor: 0 }, info: info.clone() }).await.unwrap();
            while let Ok(SessionMsg::Req { id, body }) = rx.control::<SessionMsg>().await {
                let reply = match body {
                    SessionRequest::Subscribe { .. } => {
                        AgentResponse::Subscribed { session: info.id.clone(), snapshot: None, offset: 0 }
                    }
                    SessionRequest::Screen => AgentResponse::Screen(aaa_proto::ScreenText {
                        session: info.id.clone(),
                        cols: 80,
                        rows: 24,
                        lines: vec![],
                        cursor_row: 0,
                        cursor_col: 0,
                        ansi: String::new(),
                    }),
                    _ => AgentResponse::Done,
                };
                tx.json(&SessionMsg::Res { id, result: Outcome::Ok(reply) }).await.unwrap();
            }
        }));
    }
    a.recover().await.unwrap();
    a.start_hooks().unwrap();
    let mut cfg = aaa_config::HostConfig::default();
    cfg.host.agent_listen.clear();
    cfg.projects.root = h.0.join("projects").to_string_lossy().into_owned();
    cfg.notify.web_push = false;
    let host = aaa_host::Host::open(cfg, h.0.join("host")).unwrap();
    aaa_host::link::start(&host).await.unwrap();
    aaa_host::front::start(&host).await.unwrap();
    let link = tokio::spawn(aaa_agent::link::run(
        a.clone(),
        aaa_agent::link::Target::Local(aaa_host::link::local_socket(&host)),
        Duration::from_secs(1),
    ));
    let mut front = FrontClient::connect_as(&h.0, "hook-test").await.unwrap();
    async fn state(
        front: &mut FrontClient,
        sid: &str,
        test: impl Fn(&aaa_proto::PaneView) -> bool,
    ) -> aaa_proto::PaneView {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let response = front.call(FrontRequest::Panes { role: None }).await.unwrap().unwrap();
                let aaa_proto::FrontResponse::Panes { panes } = response else { panic!("pane 목록") };
                if let Some(p) = panes.into_iter().find(|p| p.id == sid && test(p)) {
                    return p;
                }
                // 모아 둔 옛 State로 통과하지 않는다. 사건이 오면 현재 상태를 다시 읽는다.
                loop {
                    let msg = front.next().await.unwrap().expect("host가 사건 연결을 닫았다");
                    if matches!(msg, FrontMsg::Event(FrontEvent::State { .. })) {
                        break;
                    }
                }
            }
        })
        .await
        .expect("host 상태 사건")
    }
    async fn after_hook(front: &mut FrontClient) -> Vec<aaa_proto::PaneView> {
        // hook이 보낸 사건 뒤에 같은 agent link로 답을 받는다. 거부의 무변화도 이 울타리 뒤에 검사한다.
        assert!(front.call(FrontRequest::PaneRead { pane: "s1".into() }).await.unwrap().is_ok());
        let response = front.call(FrontRequest::Panes { role: None }).await.unwrap().unwrap();
        let aaa_proto::FrontResponse::Panes { panes } = response else { panic!("pane 목록") };
        panes
    }
    state(&mut front, "s1", |_| true).await;
    // hook을 보내기 전에 실제 화면 감지의 Idle을 확인한다. Windows shell 시작 시간과 무관하다.
    for sid in ["s1", "s2"] {
        let p = state(&mut front, sid, |p| p.agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Idle)).await;
        assert!(p.agent.as_ref().unwrap().harness_session.is_none());
    }
    let before_rejections = after_hook(&mut front).await;
    let unchanged = |panes: &[aaa_proto::PaneView]| {
        for sid in ["s1", "s2"] {
            let before = before_rejections.iter().find(|p| p.id == sid).unwrap();
            let after = panes.iter().find(|p| p.id == sid).unwrap();
            assert_eq!(after.agent, before.agent, "{sid}: 거부한 hook이 상태를 바꿨다");
        }
    };
    let invoke = |sid: &str, token: &str, inherited: Option<&str>, args: Vec<String>| {
        let payload = if args.first().is_some_and(|a| a == "hook") { Some(args[3].clone()) } else { None };
        let mut c = if payload.is_some() {
            let mut shell = h.hook_command(&installed);
            shell.stdin(std::process::Stdio::piped());
            shell
        } else {
            let mut cli = h.cmd(&bin());
            cli.args(args);
            cli
        };
        c.env("AAA_SESSION_ID", sid)
            .env("AAA_HOOK_SOCK", aaa_agent::hooks::socket(&a.state))
            .env("AAA_HOOK_TOKEN", token);
        if let Some(id) = inherited {
            c.env("CODEX_THREAD_ID", id);
        }
        async move {
            tokio::task::spawn_blocking(move || {
                if let Some(payload) = payload {
                    use std::io::Write;
                    c.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
                    let mut child = c.spawn().unwrap();
                    child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
                    child.wait_with_output().unwrap()
                } else {
                    c.output().unwrap()
                }
            })
            .await
            .unwrap()
        }
    };
    let token = aaa_agent::hooks::hook_token("test-session-secret");
    let id = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0";
    let event = |name: &str, source: &str| {
        vec![
            "hook".into(),
            "--agent".into(),
            "codex".into(),
            json!({"hook_event_name":name,"session_id":id,"source":source}).to_string(),
        ]
    };
    let silent = |o: Output| {
        assert!(o.status.success() && o.stdout.is_empty() && o.stderr.is_empty(), "{o:?}");
    };
    let notify = || {
        vec![
            "hook".into(),
            "--agent".into(),
            "codex".into(),
            json!({"type":"agent-turn-complete","thread-id":id}).to_string(),
        ]
    };
    // notify는 안내 출력이 원래 없다. guard가 사라져 조용히 host만 바꾸는 결함도 잡는다.
    for (sid, key, inherited, payload) in [
        ("s1", "forged", None, notify()),
        ("s2", token.as_str(), None, notify()),
        ("s1", "forged", None, event("SessionStart", "startup")),
        ("s2", token.as_str(), None, event("SessionStart", "startup")),
        ("s1", token.as_str(), Some("parent-thread"), event("SessionStart", "startup")),
    ] {
        silent(invoke(sid, key, inherited, payload).await);
        unchanged(&after_hook(&mut front).await);
    }
    let spec_path = SessionPaths::new(&a.state, "s1").spec();
    let saved: SpecFile = util::read_json(&spec_path).unwrap();
    let mut other_harness = saved.clone();
    other_harness.spec.harness = Some("claude".into());
    util::write_json(&spec_path, &other_harness).unwrap();
    silent(invoke("s1", &token, Some(id), event("SessionStart", "startup")).await);
    unchanged(&after_hook(&mut front).await);
    // 배포 전 notify 명령은 harness를 선언하지 않은 aaa 셸에서도 실행됐다. 호환 사건은 token·peer 뒤 받는다.
    let legacy_id = "0a0a0a0a-0000-4000-8000-00000000000a";
    let legacy = vec![
        "hook".into(),
        "--agent".into(),
        "codex".into(),
        json!({"type":"agent-turn-complete","thread-id":legacy_id}).to_string(),
    ];
    silent(invoke("s1", &token, None, legacy).await);
    state(&mut front, "s1", |p| p.agent.as_ref().is_some_and(|s| s.harness_session.as_deref() == Some(legacy_id)))
        .await;
    let before_subagent = after_hook(&mut front).await;
    util::write_json(&spec_path, &saved).unwrap();
    let subagent = vec![
        "hook".into(),
        "--agent".into(),
        "codex".into(),
        json!({"hook_event_name":"SessionStart","session_id":id,"transcript_path":r"C:\work\subagents\child.jsonl"})
            .to_string(),
    ];
    silent(invoke("s1", &token, Some(id), subagent).await);
    let after_subagent = after_hook(&mut front).await;
    for sid in ["s1", "s2"] {
        assert_eq!(
            after_subagent.iter().find(|p| p.id == sid).unwrap().agent,
            before_subagent.iter().find(|p| p.id == sid).unwrap().agent,
            "{sid}: 하위 agent 사건이 상태를 바꿨다"
        );
    }
    let guidance = |o: Output, name: &str| {
        assert!(o.status.success() && o.stderr.is_empty(), "{o:?}");
        let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{name}: {e}; {o:?}"));
        assert_eq!(v["hookSpecificOutput"]["hookEventName"], name, "{v}");
        v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().to_string()
    };
    let start = invoke("s1", &token, Some(id), event("SessionStart", "startup")).await;
    let text = guidance(start, "SessionStart");
    assert!(text.contains("report --percent 25") && text.contains("진행 보고"), "{text}");
    let p =
        state(&mut front, "s1", |p| p.agent.as_ref().is_some_and(|s| s.harness_session.as_deref() == Some(id))).await;
    assert_eq!(p.agent.unwrap().harness, "codex");
    // 정상 s1 hook도 s2의 기존 감지 상태를 바꾸지 않는다.
    let panes = after_hook(&mut front).await;
    assert_eq!(
        panes.iter().find(|p| p.id == "s2").unwrap().agent,
        before_rejections.iter().find(|p| p.id == "s2").unwrap().agent
    );
    silent(
        invoke(
            "s1",
            &token,
            None,
            vec!["report".into(), "--unknown".into(), "--activity".into(), "Waiting for you".into()],
        )
        .await,
    );
    state(&mut front, "s1", |p| p.agent.as_ref().is_some_and(|s| s.progress.as_deref() == Some("Waiting for you")))
        .await;
    let prompt = invoke("s1", &token, Some(id), event("UserPromptSubmit", "")).await;
    let text = guidance(prompt, "UserPromptSubmit");
    assert!(text.contains("Waiting for you") && text.contains("새 일을"), "{text}");
    state(&mut front, "s1", |p| p.agent.as_ref().is_some_and(|s| s.progress.is_none())).await;
    // 압축도 참고의 SessionStart로만 처리한다. 새 진행 안내를 돌려준다.
    let compact = invoke("s1", &token, Some(id), event("SessionStart", "compact")).await;
    let text = guidance(compact, "SessionStart");
    assert!(text.contains("report --percent 25") && text.contains("진행 보고"), "{text}");
    silent(invoke("s1", &token, Some(id), event("PostToolUse", "")).await);
    silent(invoke("s1", &token, Some(id), event("Stop", "")).await);
    silent(
        invoke(
            "s1",
            &token,
            None,
            vec!["report".into(), "--percent".into(), "65".into(), "--activity".into(), "검증 완료".into()],
        )
        .await,
    );
    state(&mut front, "s1", |p| {
        p.agent.as_ref().is_some_and(|s| s.percent == Some(65) && s.progress.as_deref() == Some("검증 완료"))
    })
    .await;
    link.abort();
    for fake in fakes {
        fake.abort();
    }
}

#[test]
fn enabled_toml_is_byte_preserved_and_readonly_fails_before_writing() {
    for text in [
        "# 유지\n[features]\nhooks = true # 유지\n",
        "features = { hooks = true, other = false }\n",
        "features.hooks = true\n",
    ] {
        let h = Home::new();
        std::fs::write(h.0.join("config.toml"), text).unwrap();
        h.ok(&["--json", "integration", "install", "codex"]);
        assert_eq!(h.read("config.toml"), text);
    }
    let h = Home::new();
    let path = h.0.join("config.toml");
    std::fs::write(&path, "[features]\nhooks = false\n").unwrap();
    let original = std::fs::metadata(&path).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&path, readonly).unwrap();
    let files = h.files();
    let out = h.run(&["integration", "install", "codex"]);
    std::fs::set_permissions(&path, original).unwrap();
    assert!(!out.status.success(), "{out:?}");
    assert_eq!(h.files(), files, "읽기 전용 설정 앞에서 다른 파일을 썼다");
}

#[test]
fn default_home_install_and_removal_do_not_depend_on_workspace_or_valid_toml() {
    let h = Home::new();
    let out = h
        .cmd(&bin())
        .env_remove("CODEX_HOME")
        .current_dir(&h.0)
        .args(["integration", "install", "codex"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(h.0.join(".codex/hooks.json").is_file());
    let config = h.0.join(".codex/config.toml");
    std::fs::write(&config, "invalid [toml").unwrap();
    let out = h.cmd(&bin()).env_remove("CODEX_HOME").args(["integration", "uninstall", "codex"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "invalid [toml");
}

#[test]
fn foreign_hook_exits_before_stdin_eof() {
    let h = Home::new();
    h.ok(&["--json", "integration", "install", "codex"]);
    let installed = h.hooks()["hooks"]["SessionStart"][0]["hooks"][0]["command"].as_str().unwrap().to_string();
    let mut child = h
        .hook_command(&installed)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        tx.send(child.wait_with_output().unwrap()).unwrap();
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(3));
    // 결함이면 EOF로 정상 종료하게 하고 실패한다. 신호로 멈추거나 프로세스를 남기지 않는다.
    drop(input);
    let out = match result {
        Ok(o) => o,
        Err(_) => {
            let draining = std::time::Instant::now();
            let out = rx.recv().unwrap();
            reader.join().unwrap();
            panic!(
                "aaa 밖 hook이 열린 stdin에서 3초 안에 끝나지 않았다(기동 지연 또는 EOF 대기). \
                 stdin을 닫은 뒤 {:?}: {out:?}",
                draining.elapsed()
            );
        }
    };
    reader.join().unwrap();
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
}
