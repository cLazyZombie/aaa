use super::*;
use aaa_common::conn::spawn_writer;
use aaa_proto::{AGENT_LINK, AgentHello, MAX_FRAME};
struct Lab {
    root: std::path::PathBuf,
    h: Arc<Host>,
}
impl Lab {
    fn new(machine: Option<&str>) -> Self {
        let root = std::env::temp_dir().join(format!("aaa-ci-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.projects.ci_machine = machine.map(str::to_string);
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        Self { root, h }
    }
    fn connect(&self, id: &str, local: bool) {
        let (a, _b) = tokio::io::duplex(1024);
        let (_, aw) = tokio::io::split(a);
        let conn = AgentConn::new(spawn_writer(aw, MAX_FRAME), 1);
        self.h.agent_connected(
            &AgentHello {
                proto: AGENT_LINK,
                machine_id: id.into(),
                machine_name: id.into(),
                boot_id: "boot".into(),
                os: "test".into(),
                arch: "test".into(),
                software: "test".into(),
                features: vec!["host_profiles.v1".into(), "ci.v1".into()],
                dev_mode: true,
                sha256: None,
                selfcheck: None,
                sessions: vec![],
                identity: None,
                harness: vec![],
            },
            local,
            conn,
        );
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
#[tokio::test]
async fn query_machine_defaults_to_host_and_can_be_overridden_independently_of_repo() {
    for global in [None, Some("mac-b")] {
        let lab = Lab::new(global);
        lab.connect("mac-a", true);
        lab.connect("mac-b", false);
        let dir = lab.h.project_dir("demo").unwrap();
        std::fs::write(
            dir.join("project.toml"),
            "[[repos]]\npath=\"/work/aaa\"\n[[repos]]\npath=\"/work/aaa\"\nmachine=\"mac-b\"\n",
        )
        .unwrap();
        for (machine, want) in [(None, global.unwrap_or("mac-a")), (Some("mac-a"), "mac-a"), (Some("mac-b"), "mac-b")] {
            let machine = machine.map(|m| format!("machine={m:?}\n")).unwrap_or_default();
            let r = crate::routine::parse(
                "ci",
                &format!("+++\non=\"ci\"\nrepo=\"aaa\"\nbranches=[\"main\"]\n{machine}+++\n자료"),
            )
            .unwrap();
            let Trigger::Ci(c) = &r.trigger else { panic!() };
            let (_, cwd, mid) = lab.h.ci_target(&dir, &r, c).unwrap();
            assert_eq!((cwd.as_str(), mid.as_str()), ("/work/aaa", want));
        }
    }
}
#[tokio::test]
async fn durable_outbox_replay_deduplicates_inbox_flow_and_mission_pending() {
    let lab = Lab::new(None);
    let h = &lab.h;
    let n = Notice {
        legacy: Default::default(),
        id: "ci-proof".into(),
        routine: "ci-main".into(),
        skip_reason: None,
        kind: "failed".into(),
        sha: "a".repeat(40),
        owners: vec!["t-0042".into()],
        text: serde_json::json!({"source":"ci","repo":"aaa","branch":"main","already_recovered":false,"reports":{"t-0042":"보고 본문"}}).to_string(),
        wake: true,
        record_only: false,
        append: None,
        fixed: None,
        at: util::now_ms(),
    };
    let mut s = State { outbox: vec![n.clone()], ..Default::default() };
    assert!(h.ci_drain("demo", "ci:demo:test", &mut s).is_err());
    assert_eq!(s.outbox.len(), 1, "정의가 없으면 source outbox를 보존한다");
    std::fs::create_dir_all(h.project_dir("demo").unwrap().join("automations")).unwrap();
    std::fs::write(
        h.project_dir("demo").unwrap().join("automations/ci-main.toml"),
        "[on.ci]\nrepo='aaa'\nbranches=['main']\n[do.inbox]\ntext='확인 메모'\n",
    )
    .unwrap();
    h.ci_drain("demo", "ci:demo:test", &mut s).unwrap();
    assert!(s.outbox.is_empty(), "공통 내구 큐에 인계한 뒤 source를 비운다");
    h.automation_pass("demo").await;
    s.outbox.push(n);
    h.ci_drain("demo", "ci:demo:test", &mut s).unwrap();
    h.automation_pass("demo").await;
    assert!(s.outbox.is_empty());
    assert_eq!(h.db.inbox_list(Some("demo"), true).unwrap().iter().filter(|i| i.item_kind == "ci").count(), 1);
    assert_eq!(
        h.db.flow_after(Some("demo"), 0, 100).unwrap().iter().filter(|e| e.kind == "ci_failed").count(),
        2,
        "프로젝트와 owner 이력 각각 한 번"
    );
    let history = h.routine_history("demo", "ci-main").unwrap();
    let delivered: Vec<_> = history.runs.iter().filter(|r| r.id == "ci-proof").collect();
    assert_eq!(delivered.len(), 1, "실패한 전달 기록과 별개로 사건은 한 번 완료한다");
    assert_eq!(history.today, 1);
    assert_eq!(delivered[0].status, "done");
}
#[test]
fn ci_registration_rejects_mixed_commands_and_invalid_events() {
    let good = include_str!("../../../../docs/examples/ci/ci-main.md");
    assert!(crate::routine::parse("ci", good).is_ok());
    for extra in ["command=\"danger\"", "schedule=\"every 1m\"", "events=[\"rerun\"]", "branches=[]"] {
        let text = format!(
            "+++\non=\"ci\"\nrepo=\"aaa\"\n{}\n{extra}\n+++\n자료",
            if extra.starts_with("branches") { "" } else { "branches=[\"main\"]" }
        );
        assert!(crate::routine::parse("ci", &text).is_err());
    }
}

/// L0에서 쓰던 DB 문서를 새 공용 상태로 읽고 다시 써도 모든 cursor·미전달 입력이 남는다.
#[tokio::test]
async fn l0_saved_state_survives_watch_extraction() {
    let lab = Lab::new(None);
    let h = &lab.h;
    let old = include_str!("l0-state.json");
    let key = "ci:demo:legacy";
    h.db.set_meta(key, old).unwrap();
    let mut state: State = h.watch_state(key).unwrap();
    assert_eq!(state.source.etag.as_deref(), Some("\"old-etag\""));
    assert_eq!(state.outbox[0].id, "ci-pending");
    h.watch_store(key, &state).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&h.db.meta(key).unwrap().unwrap()).unwrap();
    assert_eq!(saved, serde_json::from_str::<serde_json::Value>(old).unwrap());
    // 없는 임무의 입력은 새 틀을 거친 뒤에도 DB에 남아 다음 전달을 기다린다.
    h.ci_drain("demo", key, &mut state).unwrap();
    let saved: State = h.watch_state(key).unwrap();
    assert_eq!(saved.outbox.len(), 1);
    assert_eq!(saved.source.runs[&8].number, 42);
    assert_eq!(saved.source.tracks["rule:main"].open.as_deref(), Some("ci-pending"));
}

/// 최근 poll이 저장돼도 새 host는 조건부 304 너머의 빠진 사건을 따라잡아야 한다.
#[tokio::test]
async fn fresh_saved_poll_still_catches_up_in_new_host() {
    let lab = Lab::new(None);
    let key = "ci:demo:restart";
    let state = State { last_poll_ms: 1000, ..Default::default() };
    lab.h.watch_store(key, &state).unwrap();
    lab.h.watch_polled(key);
    let every = Duration::from_secs(60);
    assert!(!lab.h.watch_catchup(key, 1000, 1001, every));
    let restarted = Host::open(lab.h.cfg.clone(), lab.root.join("host")).unwrap();
    let saved: State = restarted.watch_state(key).unwrap();
    assert!(
        restarted.watch_catchup(key, saved.last_poll_ms, 1001, every),
        "직전 poll이 1ms 전이어도 새 프로세스는 따라잡는다"
    );
}

#[tokio::test]
async fn inactive_watch_sources_release_tick_deadlines() {
    let lab = Lab::new(None);
    let h = &lab.h;
    let dir = h.project_dir("demo").unwrap();
    let ci = include_str!("../../../../docs/examples/ci/ci-main.md");
    let git = include_str!("../../../../docs/examples/git/git-main.md");
    std::fs::write(dir.join("routines/ci-main.md"), ci).unwrap();
    std::fs::write(dir.join("routines/git-main.md"), git).unwrap();
    h.watch_finished("ci", "demo");
    h.watch_finished("git", "demo");
    assert!(h.ci_next_due().is_some());
    assert!(h.git_next_due().is_some());
    std::fs::write(dir.join("routines/git-main.md"), git.replace("enabled = true", "enabled = false")).unwrap();
    assert!(h.git_next_due().is_none(), "비활성 Git 감시는 ticker 예약을 남기지 않는다");
    assert!(h.ci_next_due().is_some(), "다른 source의 예약은 유지한다");
    std::fs::write(dir.join("routines/ci-main.md"), ci.replace("enabled = true", "enabled = false")).unwrap();
    assert!(h.ci_next_due().is_none());
    h.watch_finished("ci", "demo");
    std::fs::remove_file(dir.join("PROJECT.md")).unwrap();
    assert!(h.ci_next_due().is_none(), "삭제한 프로젝트는 예약하지 않는다");
}

#[tokio::test]
async fn recovery_cancels_failure_still_waiting_in_rule_batch() {
    let lab = Lab::new(None);
    let dir = lab.h.project_dir("demo").unwrap();
    let r = crate::automation::parse(
        &dir,
        "check",
        "[on.ci]\nrepo='app'\nbranches=['main']\n[do.inbox]\n[limits]\nbatch='1m'\n",
    )
    .unwrap();
    lab.h
        .automation_submit(
            "demo",
            &r,
            "ci-red",
            serde_json::json!({"source":"ci","repo":"app","branch":"main","result":"failed"}).to_string(),
            None,
        )
        .unwrap();
    let (a, _b) = tokio::io::duplex(1024);
    let (_, write) = tokio::io::split(a);
    let conn = AgentConn::new(spawn_writer(write, MAX_FRAME), 1);
    let now = util::now_ms();
    let mut state = State::default();
    state.source.runs.insert(
        1,
        CiRun {
            id: 1,
            branch: "main".into(),
            sha: "a".repeat(40),
            status: "completed".into(),
            conclusion: "success".into(),
            created_ms: now,
            ..Default::default()
        },
    );
    lab.h.ci_rule("demo", &conn, "/work/app", &r, &mut state, now).await.unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&lab.h.db.meta("automation:demo:check").unwrap().unwrap()).unwrap();
    assert!(saved["pending"].as_array().unwrap().is_empty(), "이미 회복한 CI 실패는 batch 뒤에 늦게 전달하지 않는다");
    lab.h.automation_submit("demo", &r, "ci-red", "same delivery replay".into(), None).unwrap();
    lab.h.automation_drain_at("demo", &r, now + 60000).await.unwrap();
    assert_eq!(lab.h.automation_count("demo", "check").unwrap(), 0);
}

#[tokio::test]
async fn selected_ci_pass_in_toml_reaches_the_shared_action_once() {
    let lab = Lab::new(None);
    let dir = lab.h.project_dir("demo").unwrap();
    std::fs::create_dir_all(dir.join("automations")).unwrap();
    let text = "[on.ci]\nrepo='app'\nbranches=['main']\nevents=['passed']\n[do.inbox]\ntext='CI 통과 확인'\n";
    std::fs::write(dir.join("automations/green.toml"), text).unwrap();
    let r = crate::automation::parse(&dir, "green", text).unwrap();
    let (a, _b) = tokio::io::duplex(1024);
    let (_, write) = tokio::io::split(a);
    let conn = AgentConn::new(spawn_writer(write, MAX_FRAME), 1);
    let now = util::now_ms();
    let mut state = State::default();
    state.source.runs.insert(
        1,
        CiRun {
            id: 1,
            branch: "main".into(),
            sha: "a".repeat(40),
            status: "completed".into(),
            conclusion: "success".into(),
            created_ms: now,
            ..Default::default()
        },
    );
    lab.h.ci_rule("demo", &conn, "/work/app", &r, &mut state, now).await.unwrap();
    lab.h.ci_drain("demo", "ci:demo:green", &mut state).unwrap();
    lab.h.automation_pass("demo").await;
    lab.h.automation_pass("demo").await;
    let inbox = lab.h.db.inbox_list(Some("demo"), true).unwrap();
    assert_eq!(
        inbox.iter().filter(|i| i.item_kind == "routine" && i.subject == "green").count(),
        1,
        "새 형식의 명시 passed 조건도 공통 동작을 실행한다"
    );
    assert_eq!(lab.h.automation_count("demo", "green").unwrap(), 1);
}
