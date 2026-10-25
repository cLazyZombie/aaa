use super::*;
use crate::db::Db;
use std::path::PathBuf;
struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("aaa-ws-host-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn run(id: &str, at: u64, machine: &str, profile: &str, ms: u64, outcome: &str) -> MetricRun {
    MetricRun {
        run_id: id.into(),
        commit: "a".repeat(40),
        observed_at_ms: at,
        machine: machine.into(),
        profile: Some(profile.into()),
        verification_ms: Some(ms),
        outcome: outcome.into(),
        ..Default::default()
    }
}
#[test]
fn daily_median_keeps_machine_profile_boundaries_excludes_failure_and_never_fills_gaps() {
    let day = 86_400_000;
    let now = day + 16 * 3_600_000; // UTC 16시 = KST 다음 날 1시
    assert_eq!(period_start(now, 1, 0), day);
    assert_eq!(period_start(now, 1, 540), day + 15 * 3_600_000);
    let rows = [
        run("a", day, "a", "dev", 100, "succeeded"),
        run("b", day + 10, "a", "dev", 300, "succeeded"),
        run("bad", day + 20, "a", "dev", 9000, "failed"),
        run("c", day, "b", "dev", 700, "succeeded"),
        run("d", day, "a", "verify", 1100, "succeeded"),
        run("e", day * 3, "a", "dev", 900, "succeeded"),
    ];
    let query = WorkspaceQuery { granularity: Some("day".into()), utc_offset_minutes: Some(540), ..Default::default() };
    let got = points(&rows, &query, |r| r.verification_ms);
    assert_eq!(got.len(), 4);
    let a = got.iter().find(|p| p.machine == "a" && p.profile.as_deref() == Some("dev") && p.samples == 2).unwrap();
    assert_eq!(a.value, 200);
    assert!(got.iter().all(|p| p.outcome == "succeeded"));
    let raw = points(&rows, &WorkspaceQuery::default(), |r| r.verification_ms);
    assert_eq!(raw.len(), 6);
    assert!(raw.iter().any(|p| p.outcome == "failed"));
}
#[test]
fn ledger_groups_remote_identity_across_machines_and_projects_and_resets_only_old_metrics() {
    let l = Lab::new();
    let path = l.0.join("db");
    let db = Db::open(&path).unwrap();
    let remote = "a".repeat(64);
    let a = db.repo_workspace_id("p", "checkout-a", Some(&remote)).unwrap();
    let b = db.repo_workspace_id("other", "checkout-b", Some(&remote)).unwrap();
    assert_eq!(a, b);
    assert_ne!(a, db.repo_workspace_id("p", "fork", Some(&"b".repeat(64))).unwrap());
    let value = WorkspaceCollected {
        runs: vec![run("r", 10, "spoof", "verify", 100, "failed"), MetricRun::default()],
        code_lines: Some(CodeLines {
            commit: "a".repeat(40),
            branch: "main".into(),
            rules_version: 1,
            observed_at_ms: 10,
            ..Default::default()
        }),
        ..Default::default()
    };
    for machine in ["mac-a", "mac-a", "mac-b"] {
        db.save_workspace_metrics("", &a, machine, &value).unwrap();
    }
    db.set_meta("unrelated", "keep").unwrap();
    db.save_workspace("keep", "{}").unwrap();
    let extra = rusqlite::Connection::open(&path).unwrap();
    extra
        .execute("INSERT INTO threads(project,id,status,data,updated_at) VALUES('p','t-fixture','open','{}',1)", [])
        .unwrap();
    drop(db);
    let db = Db::open(&path).unwrap();
    let (runs, lines, _) = db.workspace_records("", &a, 0).unwrap();
    assert_eq!(runs.len(), 2);
    assert_eq!(lines.len(), 1);
    assert!(runs.iter().all(|r| r.machine != "spoof"));
    db.set_meta("workspace-metrics-schema", "1").unwrap();
    drop(db);
    let db = Db::open(&path).unwrap();
    assert!(db.workspace_records("", &a, 0).unwrap().0.is_empty());
    assert_eq!(db.repo_workspace_id("p", "checkout-a", None).unwrap(), "");
    assert_eq!(db.meta("unrelated").unwrap().as_deref(), Some("keep"));
    assert_eq!(db.workspaces().unwrap(), vec![("keep".into(), "{}".into())]);
    assert_eq!(
        extra.query_row("SELECT COUNT(*) FROM threads WHERE id='t-fixture'", [], |r| r.get::<_, u32>(0)).unwrap(),
        1
    );
    assert_eq!(db.meta("workspace-metrics-schema").unwrap().as_deref(), Some("2"));
    db.save_workspace_metrics("", &a, "mac-a", &value).unwrap();
    drop(db);
    assert_eq!(Db::open(&path).unwrap().workspace_records("", &a, 0).unwrap().0.len(), 1);
}
#[tokio::test]
async fn reader_queries_only_the_host_ledger_with_filters_missing_values_and_disconnected_agents() {
    let l = Lab::new();
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = l.0.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, l.0.join("host")).unwrap();
    h.project_create("p", Default::default(), "test").unwrap();
    let hello = AgentHello {
        proto: AGENT_LINK,
        machine_id: "m-a".into(),
        machine_name: "mac-a".into(),
        boot_id: "boot".into(),
        os: "macos".into(),
        arch: "aarch64".into(),
        software: aaa_common::util::software(),
        sha256: None,
        features: vec!["terminal.v1".into(), "workspace_metrics.v2".into()],
        dev_mode: true,
        selfcheck: None,
        sessions: vec![],
        identity: None,
        harness: vec![],
    };
    h.agent_connected(
        &hello,
        false,
        crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), MAX_FRAME), 1),
    );
    let mut second = hello.clone();
    second.machine_id = "m-b".into();
    second.machine_name = "mac-b".into();
    h.agent_connected(
        &second,
        false,
        crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), MAX_FRAME), 1),
    );
    let dir = h.project_dir("p").unwrap();
    std::fs::write(
        dir.join("project.toml"),
        "[[repos]]\npath='/work/app'\nmachine='mac-a'\nworkspace='app'\n[[repos]]\npath='/fork/app'\nmachine='mac-a'\n[[repos]]\npath='/other/app'\nmachine='mac-b'\n",
    )
    .unwrap();
    for reg in h.metric_registrations().unwrap() {
        let remote = if reg.path == "/fork/app" { "b" } else { "a" }.repeat(64);
        h.db.repo_workspace_id("p", &reg.registration, Some(&remote)).unwrap();
    }
    let list = h.workspace_dashboard(WorkspaceQuery::default()).unwrap();
    assert_eq!(list.workspaces.len(), 2);
    assert!(list.workspaces.iter().any(|w| w.machines.len() == 2));
    let id = format!("ws-{}", "a".repeat(64));
    let now = aaa_common::util::now_ms();
    let data = WorkspaceCollected {
        runs: vec![run("one", now, "x", "verify", 42, "succeeded"), run("two", now, "x", "dev", 88, "failed")],
        ..Default::default()
    };
    h.db.save_workspace_metrics("", &id, "mac-a", &data).unwrap();
    let q = WorkspaceQuery { workspace_id: Some(id.clone()), profile: Some("verify".into()), ..Default::default() };
    let FrontResponse::WorkspaceDashboard(view) =
        h.call(DeviceRole::Reader, "reader:test", FrontRequest::WorkspaceDashboard { query: q }).await.unwrap()
    else {
        panic!()
    };
    assert_eq!(view.runs.len(), 1);
    assert_eq!(view.runs[0].verification_ms, Some(42));
    assert!(view.runs[0].tests.is_none());
    assert!(view.runs[0].artifact_bytes.is_none());
    assert_eq!(view.verification_trend.len(), 1);
    assert!(
        h.workspace_dashboard(WorkspaceQuery {
            workspace_id: Some(id.clone()),
            machine: Some("absent".into()),
            ..Default::default()
        })
        .unwrap()
        .runs
        .is_empty()
    );
    assert!(
        h.workspace_dashboard(WorkspaceQuery { workspace_id: Some("p/missing".into()), ..Default::default() })
            .unwrap()
            .workspace_id
            .is_none()
    );
}

#[tokio::test]
async fn report_without_repo_collects_all_registrations_on_its_machine_and_continues_after_failure() {
    use aaa_common::conn::{FrameRx, spawn_writer};
    let l = Lab::new();
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = l.0.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, l.0.join("host")).unwrap();
    h.project_create("p", Default::default(), "test").unwrap();
    let (a, b) = tokio::io::duplex(MAX_FRAME * 2);
    let (_, aw) = tokio::io::split(a);
    let conn = crate::AgentConn::new(spawn_writer(aw, MAX_FRAME), 1);
    let pending = conn.pending.clone();
    let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
    let worker = tokio::spawn(async move {
        let mut rx = FrameRx::new(b, MAX_FRAME);
        while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
            let AgentRequest::WorkspaceMetrics { cwd, .. } = body else { panic!("지표 조회만 허용") };
            let response = if cwd == "/work/broken" {
                Outcome::Err(err(ErrorCode::Internal, "fixture source error"))
            } else {
                Outcome::Ok(AgentResponse::WorkspaceMetrics(WorkspaceCollected {
                    remote_id: Some("b".repeat(64)),
                    runs: vec![run("main-check", 10, "ignored", "verify", 100, "succeeded")],
                    ..Default::default()
                }))
            };
            sent.send(cwd).unwrap();
            if let Some(tx) = pending.lock().unwrap().remove(&id) {
                let _ = tx.send(response);
            }
        }
    });
    h.agent_connected(
        &AgentHello {
            proto: AGENT_LINK,
            machine_id: "m-a".into(),
            machine_name: "mac-a".into(),
            boot_id: "boot".into(),
            os: "test".into(),
            arch: "test".into(),
            software: "test".into(),
            sha256: None,
            features: vec!["workspace_metrics.v2".into()],
            dev_mode: true,
            selfcheck: None,
            sessions: vec![],
            identity: None,
            harness: vec![],
        },
        false,
        conn,
    );
    std::fs::write(
        h.project_dir("p").unwrap().join("project.toml"),
        "[[repos]]\npath='/work/broken'\nmachine='mac-a'\n[[repos]]\npath='/work/engine'\nmachine='mac-a'\n",
    )
    .unwrap();
    let mut rec = crate::orch::ThreadRec::starting("p", "t-0001", "m-a", "fake", Role::Worker, ThreadKind::Dir, 1);
    rec.status = ThreadStatus::Open;
    assert!(rec.repo.is_none());
    h.db.thread_import("p", "t-0001", "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    tokio::join!(
        h.collect_workspace_metrics("p", "t-0001"),
        h.collect_workspace_metrics("p", "t-0001"),
        h.collect_workspace_metrics("p", "t-0001"),
    );
    // 첫 회수와 진행 중 요청을 모은 후속 회수 두 차례다.
    for _ in 0..2 {
        assert_eq!(received.try_recv().unwrap(), "/work/broken");
        assert_eq!(received.try_recv().unwrap(), "/work/engine");
    }
    assert!(received.try_recv().is_err());
    let rows = h.db.workspace_records("", &format!("ws-{}", "b".repeat(64)), 0).unwrap().0;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].machine, "mac-a");
    worker.abort();
    let _ = worker.await;
}
