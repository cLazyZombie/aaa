//! 전역 설치 CLI 경계. 운영 HOME·서비스·프로세스 신호를 쓰지 않는다.
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_aaa");
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
        self.cmd(Path::new(BIN)).args(args).output().unwrap()
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
    std::fs::copy(BIN, &moved).unwrap();
    let o = h.cmd(&moved).args(["--json", "integration", "install", "codex"]).output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let mut v = h.hooks();
    let start = v["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(start.len(), 2, "이전 aaa 명령이 남았다: {v}");
    assert_eq!(start[0], foreign);
    let hook_command = start[1]["hooks"][0]["command"].as_str().unwrap();
    let mut shell = if cfg!(windows) { Command::new("powershell") } else { Command::new("sh") };
    if cfg!(windows) {
        shell.args(["-NoProfile", "-Command"]);
    } else {
        shell.arg("-c");
    }
    let o = shell
        .arg(hook_command)
        .env_remove("AAA_SESSION_ID")
        .env_remove("AAA_HOOK_SOCK")
        .env_remove("AAA_HOOK_TOKEN")
        .output()
        .unwrap();
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
                .cmd(Path::new(BIN))
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
        AgentResponse, FrontEvent, FrontMsg, FrontRequest, MAX_FRAME, Outcome, Role, SessionInfo, SessionMsg,
        SessionRequest, SessionState, SpawnSpec, Version,
    };
    use aaa_session::files::{SessionPaths, SpecFile};
    use std::time::Duration;
    let h = Home::new();
    h.ok(&["--json", "integration", "install", "codex"]);
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
            started_at_ms: util::now_ms(),
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
    async fn state(front: &mut FrontClient, test: impl Fn(&aaa_proto::PaneView) -> bool) -> aaa_proto::PaneView {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(FrontMsg::Event(FrontEvent::State { panes, .. })) = front.next().await.unwrap()
                    && let Some(p) = panes.into_iter().find(|p| p.id == "s1" && test(p))
                {
                    return p;
                }
            }
        })
        .await
        .expect("host 상태 사건")
    }
    state(&mut front, |_| true).await;
    // 실제 agent 요청·답의 울타리: link Welcome을 처리하고 hook을 보낼 준비가 됐다.
    assert!(front.call(FrontRequest::PaneRead { pane: "s1".into() }).await.unwrap().is_ok());
    let invoke = |sid: &str, token: &str, inherited: Option<&str>, args: Vec<String>| {
        let payload = if args.first().is_some_and(|a| a == "hook") { Some(args[3].clone()) } else { None };
        let mut c = if payload.is_some() {
            let mut shell = if cfg!(windows) { h.cmd(Path::new("powershell")) } else { h.cmd(Path::new("sh")) };
            if cfg!(windows) {
                shell.args(["-NoProfile", "-Command"]);
            } else {
                shell.arg("-c");
            }
            shell.arg(&installed).stdin(std::process::Stdio::piped());
            shell
        } else {
            let mut cli = h.cmd(Path::new(BIN));
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
    for (sid, key, inherited) in
        [("s1", "forged", None), ("s2", token.as_str(), None), ("s1", token.as_str(), Some("parent-thread"))]
    {
        silent(invoke(sid, key, inherited, event("SessionStart", "startup")).await);
    }
    let spec_path = SessionPaths::new(&a.state, "s1").spec();
    let saved: SpecFile = util::read_json(&spec_path).unwrap();
    let mut other_harness = saved.clone();
    other_harness.spec.harness = Some("claude".into());
    util::write_json(&spec_path, &other_harness).unwrap();
    silent(invoke("s1", &token, Some(id), event("SessionStart", "startup")).await);
    // 배포 전 notify 명령은 harness를 선언하지 않은 aaa 셸에서도 실행됐다. 호환 사건은 token·peer 뒤 받는다.
    let legacy_id = "0a0a0a0a-0000-4000-8000-00000000000a";
    let legacy = vec![
        "hook".into(),
        "--agent".into(),
        "codex".into(),
        json!({"type":"agent-turn-complete","thread-id":legacy_id}).to_string(),
    ];
    silent(invoke("s1", &token, None, legacy).await);
    state(&mut front, |p| p.agent.as_ref().is_some_and(|s| s.harness_session.as_deref() == Some(legacy_id))).await;
    util::write_json(&spec_path, &saved).unwrap();
    let subagent = vec![
        "hook".into(),
        "--agent".into(),
        "codex".into(),
        json!({"hook_event_name":"SessionStart","session_id":id,"transcript_path":r"C:\work\subagents\child.jsonl"})
            .to_string(),
    ];
    silent(invoke("s1", &token, Some(id), subagent).await);
    let start = invoke("s1", &token, Some(id), event("SessionStart", "startup")).await;
    assert!(start.status.success() && start.stderr.is_empty(), "{start:?}");
    let v: Value = serde_json::from_slice(&start.stdout).unwrap();
    assert!(v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("report --percent 25"), "{v}");
    let p = state(&mut front, |p| p.agent.as_ref().is_some_and(|s| s.harness_session.as_deref() == Some(id))).await;
    assert_eq!(p.agent.unwrap().harness, "codex");
    // 인증 거부 사건은 뒤따르는 정상 SessionStart보다 먼저 같은 link에 있어야 한다. s2는 상태가 생기지 않는다.
    let panes = front.call(FrontRequest::Panes { role: None }).await.unwrap().unwrap();
    let aaa_proto::FrontResponse::Panes { panes } = panes else { panic!("pane 목록") };
    assert!(panes.iter().find(|p| p.id == "s2").unwrap().agent.is_none());
    silent(
        invoke(
            "s1",
            &token,
            None,
            vec!["report".into(), "--unknown".into(), "--activity".into(), "Waiting for you".into()],
        )
        .await,
    );
    state(&mut front, |p| p.agent.as_ref().is_some_and(|s| s.progress.as_deref() == Some("Waiting for you"))).await;
    let prompt = invoke("s1", &token, Some(id), event("UserPromptSubmit", "")).await;
    assert!(String::from_utf8_lossy(&prompt.stdout).contains("Waiting for you"), "{prompt:?}");
    state(&mut front, |p| p.agent.as_ref().is_some_and(|s| s.progress.is_none())).await;
    // 압축도 참고의 SessionStart로만 처리한다. 새 진행 안내를 돌려준다.
    let compact = invoke("s1", &token, Some(id), event("SessionStart", "compact")).await;
    assert!(String::from_utf8_lossy(&compact.stdout).contains("report --percent 25"), "{compact:?}");
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
    state(&mut front, |p| p.agent.as_ref().is_some_and(|s| s.percent == Some(65))).await;
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
        .cmd(Path::new(BIN))
        .env_remove("CODEX_HOME")
        .current_dir(&h.0)
        .args(["integration", "install", "codex"])
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(h.0.join(".codex/hooks.json").is_file());
    let config = h.0.join(".codex/config.toml");
    std::fs::write(&config, "invalid [toml").unwrap();
    let out =
        h.cmd(Path::new(BIN)).env_remove("CODEX_HOME").args(["integration", "uninstall", "codex"]).output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(std::fs::read_to_string(&config).unwrap(), "invalid [toml");
}

#[test]
fn foreign_hook_exits_before_stdin_eof() {
    let h = Home::new();
    let mut child = h
        .cmd(Path::new(BIN))
        .args(["hook", "--agent", "codex"])
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
            let _ = rx.recv().unwrap();
            reader.join().unwrap();
            panic!("aaa 밖 hook이 stdin EOF를 기다렸다");
        }
    };
    reader.join().unwrap();
    assert!(out.status.success() && out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
}
