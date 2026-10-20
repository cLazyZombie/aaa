//! 새 OS 프로세스나 신호 없이 실제 host·DB·frame owner를 검사한다.
use crate::{
    Host,
    automation::{self, Action},
    orch::ThreadRec,
};
use aaa_common::util;
use sha2::{Digest, Sha256};
use std::sync::Arc;
fn sha(s: &str) -> String {
    hex::encode(Sha256::digest(s.as_bytes()))
}
use aaa_common::conn::{FrameRx, spawn_writer};
use aaa_proto::*;
use std::sync::Mutex;

struct Lab {
    h: Arc<Host>,
    root: std::path::PathBuf,
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-automation-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        Self { h, root }
    }
    async fn fake(&self, features: Vec<String>) -> Fake {
        self.fake_on("m", "mac-a", features).await
    }
    async fn fake_on(&self, mid: &str, name: &str, mut features: Vec<String>) -> Fake {
        let machine = mid.to_string();
        features.push("host_profiles.v1".into());
        let (a, b) = tokio::io::duplex(2 << 20);
        let (ar, aw) = tokio::io::split(a);
        drop(ar);
        let tx = spawn_writer(aw, MAX_FRAME);
        let conn = crate::AgentConn::new(tx, 1);
        let requests = Arc::new(Mutex::new(vec![]));
        let log = requests.clone();
        let h = self.h.clone();
        let pending = conn.pending.clone();
        let screen = Arc::new(Mutex::new(
            std::fs::read_to_string(format!(
                "{}/tests/fixtures/prompt_box/claude-empty.ansi",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap(),
        ));
        let inbox_names = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen_names = inbox_names.clone();
        let shown = screen.clone();
        let prepared_root = self.root.join("prepared");
        let handle = tokio::spawn(async move {
            let mut rx = FrameRx::new(b, MAX_FRAME);
            let mut spawns = 0;
            while let Ok(msg) = rx.control::<LinkMsg>().await {
                let LinkMsg::Req { id, body, .. } = msg else { continue };
                log.lock().unwrap().push(body.clone());
                let answer = match body {
                    AgentRequest::MissionInboxStatus { .. } | AgentRequest::WorkFileInboxStatus { .. } => {
                        let names = seen_names.lock().unwrap().clone();
                        AgentResponse::MissionInbox { pending: names.len() as u32, names }
                    }
                    AgentRequest::ThreadPrepare(p) => {
                        let cwd = prepared_root.join(&p.thread);
                        let work = cwd.join(".aaa").join(format!("{}-{}", p.project, p.thread));
                        std::fs::create_dir_all(&work).unwrap();
                        AgentResponse::ThreadReady(ThreadReady {
                            cwd: cwd.to_string_lossy().into(),
                            work_dir: work.to_string_lossy().into(),
                            branch: None,
                            base: None,
                            launch: Launch {
                                profile: p.profile,
                                harness: p.resolved_profile.map(|r| r.harness).unwrap_or_else(|| "claude".into()),
                                argv: vec!["fake".into()],
                                env: vec![],
                                resume_argv: vec!["fake-resume".into()],
                                session_key: None,
                            },
                        })
                    }
                    AgentRequest::MissionStateList { work_dir } | AgentRequest::WorkFileList { work_dir } => {
                        let mut files = vec![];
                        let mut paths = vec!["state.md".to_owned(), "processed.txt".into()];
                        let root = std::path::Path::new(&work_dir);
                        let mut dirs = vec![root.join("memory")];
                        while let Some(dir) = dirs.pop() {
                            for entry in std::fs::read_dir(dir).into_iter().flatten() {
                                let entry = entry.unwrap();
                                if entry.file_type().unwrap().is_dir() {
                                    dirs.push(entry.path());
                                } else {
                                    paths.push(
                                        entry.path().strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"),
                                    );
                                }
                            }
                        }
                        paths.sort();
                        for path in paths {
                            if let Ok(bytes) = std::fs::read(std::path::Path::new(&work_dir).join(&path)) {
                                files.push(LibFile {
                                    path,
                                    size: bytes.len() as u64,
                                    sha256: hex::encode(Sha256::digest(&bytes)),
                                });
                            }
                        }
                        AgentResponse::MissionFiles { files }
                    }
                    AgentRequest::MissionStateRead { work_dir, path, offset }
                    | AgentRequest::WorkFileRead { work_dir, path, offset } => {
                        let bytes = std::fs::read(std::path::Path::new(&work_dir).join(path)).unwrap();
                        AgentResponse::FileChunk { data: B64(bytes[offset as usize..].to_vec()), eof: true }
                    }
                    AgentRequest::MissionStateWrite { work_dir, path, data, .. }
                    | AgentRequest::WorkFileWriteChunk { work_dir, path, data, .. } => {
                        let dest = std::path::Path::new(&work_dir).join(path);
                        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
                        std::fs::write(dest, data.0).unwrap();
                        AgentResponse::Done
                    }
                    AgentRequest::Screen { session } => {
                        let ansi = shown.lock().unwrap().clone();
                        AgentResponse::Screen(ScreenText {
                            session,
                            cols: 120,
                            rows: 32,
                            lines: ansi.lines().map(str::to_string).collect(),
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi,
                        })
                    }
                    AgentRequest::Input { session, data } => {
                        if data.0 == b"\r" || data.0 == b"\n" {
                            h.on_harness(&machine,serde_json::from_value(serde_json::json!({"session":session,"harness":"claude","kind":"working","at_ms":util::now_ms()})).unwrap());
                        } else {
                            let mut s = shown.lock().unwrap();
                            *s = format!("{}\n{}", String::from_utf8_lossy(&data.0), *s);
                        }
                        AgentResponse::Done
                    }
                    AgentRequest::ThreadRebrief { brief, mut launch, profile, resolved_profile, .. } => {
                        assert!(brief.contains("적용 판"));
                        if let Some(profile) = profile {
                            launch.profile = profile;
                        }
                        if let Some(profile) = resolved_profile {
                            launch.harness = profile.harness;
                        }
                        AgentResponse::Relaunch(launch)
                    }
                    AgentRequest::ReportCollect { .. } => AgentResponse::Report(ReportSnapshot {
                        hash: sha(""),
                        report: B64(vec![]),
                        files: vec![],
                        skipped: vec![],
                    }),
                    AgentRequest::RunCommand { .. } => AgentResponse::CommandRan(CommandRan {
                        exit_code: Some(0),
                        timed_out: false,
                        output: "같은 결과".into(),
                        truncated: false,
                    }),
                    AgentRequest::Spawn(spec) => {
                        spawns += 1;
                        let mut s = info(&if machine == "m" {
                            format!("new{spawns}")
                        } else {
                            format!("{machine}-new{spawns}")
                        });
                        s.role = spec.role;
                        s.argv = spec.argv;
                        AgentResponse::Session(s)
                    }
                    _ => AgentResponse::Done,
                };
                if let Some(tx) = pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(Outcome::Ok(answer));
                }
            }
        });
        self.h.agent_connected(
            &AgentHello {
                proto: if features.iter().any(|f| f == "work_files.v1") {
                    AGENT_LINK
                } else {
                    Version { major: 1, minor: 9 }
                },
                machine_id: mid.into(),
                machine_name: name.into(),
                boot_id: "boot".into(),
                os: "test".into(),
                arch: "test".into(),
                software: "test".into(),
                features,
                dev_mode: true,
                selfcheck: None,
                sessions: vec![],
                harness: vec![],
            },
            mid == "m",
            conn,
        );
        Fake { requests, _screen: screen, handle }
    }
}
struct Fake {
    requests: Arc<Mutex<Vec<AgentRequest>>>,
    _screen: Arc<Mutex<String>>,
    handle: tokio::task::JoinHandle<()>,
}
impl Drop for Fake {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
fn info(id: &str) -> SessionInfo {
    SessionInfo {
        id: id.into(),
        name: "감시".into(),
        argv: vec!["fake".into()],
        cwd: None,
        pid: None,
        started_at_ms: 0,
        boot_id: "boot".into(),
        cols: 120,
        rows: 32,
        state: SessionState::Running,
        role: Some(Role::Worker),
        label: None,
        harness: Some("claude".into()),
    }
}

// L5 owner 계약: 실제 host/DB와 기존 agent frame fixture를 사용한다.

fn rule(l: &Lab, extra: &str) -> crate::routine::Routine {
    let dir = l.h.project_dir("demo").unwrap();
    std::fs::create_dir_all(dir.join("automations")).unwrap();
    let text = format!(
        "[on.schedule]\nevery='10m'\n[do.start]\ntitle='점검 {{date}}'\ntext='자료를 확인한다'\nprofiles=['claude']\nmachines=['mac-a','mac-b']\nwait=['timing']\n{extra}\n"
    );
    std::fs::write(dir.join("automations/check.toml"), &text).unwrap();
    automation::parse(&dir, "check", &text).unwrap()
}
fn approve(l: &Lab, r: &crate::routine::Routine) {
    let (sha, _) = l.h.automation_start_approval("demo", r).unwrap().unwrap();
    l.h.automation_approve("demo", &r.name, &sha, "local:cli").unwrap();
}
fn auto_lab() -> Lab {
    let mut l = Lab::new();
    Arc::get_mut(&mut l.h).unwrap().cfg.safety.insert(
        "demo".into(),
        aaa_config::safety::SafetyLayer { start_threads: Some("auto".into()), ..Default::default() },
    );
    l
}
fn seed(l: &Lab, machine: &str, holds: &[&str]) -> ThreadRec {
    let mut r =
        ThreadRec::starting("demo", "측정 중", machine, "claude", Role::Worker, ThreadKind::Dir, util::now_ms());
    r.holds = holds.iter().map(|s| s.to_string()).collect();
    r.status = ThreadStatus::Open;
    r.brief = BriefState::Delivered;
    let id =
        l.h.db
            .thread_insert_next("demo", |id| {
                r.id = id.into();
                serde_json::to_string(&r).unwrap()
            })
            .unwrap();
    assert_eq!(r.id, id);
    r
}
fn starts(f: &Fake) -> usize {
    f.requests.lock().unwrap().iter().filter(|q| matches!(q, AgentRequest::ThreadPrepare(_))).count()
}

#[tokio::test]
async fn start_scope_requires_person_only_for_first_or_broader_scope() {
    let l = auto_lab();
    let _a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    let (sha, approved) = l.h.automation_start_approval("demo", &r).unwrap().unwrap();
    assert!(!approved);
    assert!(l.h.automation_approve("demo", "check", "stale", "local:cli").is_err());
    assert!(l.h.automation_approve("demo", "check", &sha, "local:cli@s1").is_err());
    approve(&l, &r);
    let mut narrow = r.clone();
    let Action::Start(a) = &mut narrow.action else { panic!() };
    a.machines.pop();
    a.text = Some("다른 지침".into());
    narrow.limits.max_per_day = Some(400);
    narrow.limits.max_running = Some(100);
    assert!(l.h.automation_start_approval("demo", &narrow).unwrap().unwrap().1);
    let mut broad = r.clone();
    broad.trigger = crate::routine::Trigger::Schedule(crate::routine::Schedule::Every(60));
    assert!(!l.h.automation_start_approval("demo", &broad).unwrap().unwrap().1);
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 0);
}

#[tokio::test]
async fn start_waits_for_approval_and_holds_then_creates_each_occurrence_once() {
    let l = auto_lab();
    let a = l.fake(vec![]).await;
    let b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    l.h.automation_submit("demo", &r, "due:1", "first push".into(), None).unwrap();
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(starts(&a) + starts(&b), 0);
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 0);
    assert!(l.h.db.threads(Some("demo")).unwrap().is_empty());
    let one = seed(&l, "m", &["timing"]);
    let two = seed(&l, "b", &["timing"]);
    approve(&l, &r);
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(starts(&a) + starts(&b), 0);
    l.h.update_rec("demo", &two.id, |rec| rec.status = ThreadStatus::Resolved).unwrap();
    let crash = rusqlite::Connection::open(l.root.join("host/host.db")).unwrap();
    crash.execute_batch("CREATE TRIGGER crash_start_receipt BEFORE INSERT ON meta WHEN NEW.key LIKE 'automation_done:%' BEGIN SELECT RAISE(ABORT,'crash after thread commit'); END;").unwrap();
    let error = l.h.automation_drain("demo", &r).await.unwrap_err();
    assert!(error.message.contains("crash after thread commit"));
    crash.execute_batch("DROP TRIGGER crash_start_receipt").unwrap();
    let records = l.h.db.threads(Some("demo")).unwrap();
    let automatic: Vec<ThreadRec> = records
        .iter()
        .map(|s| serde_json::from_str(s).unwrap())
        .filter(|r: &ThreadRec| r.automation.is_some())
        .collect();
    assert_eq!(automatic.len(), 1);
    assert_eq!(automatic[0].machine_id, "b");
    assert_eq!(automatic[0].role, Role::Worker);
    assert_eq!(automatic[0].started_by.as_deref(), Some("routine:check"));
    assert!(
        std::fs::read_to_string(
            l.h.project_dir("demo").unwrap().join("threads").join(format!("{}.task.md", automatic[0].id))
        )
        .unwrap()
        .contains("first push")
    );
    let id = automatic[0].id.clone();
    // thread commit 뒤 receipt 실패로 남은 실제 inflight를 다시 처리해도 두 번째 thread를 만들지 않는다.
    l.h.automation_submit("demo", &r, "due:1", "first push".into(), None).unwrap();
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(l.h.db.meta(&crate::auto_start::start_key("demo", "check", "due:1")).unwrap(), Some(id));
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 3);
    let history = l.h.routine_history("demo", "check").unwrap();
    assert!(
        history.runs.iter().any(|r| r.id == "due:1" && r.status == "done" && r.thread.is_some()),
        "생성 뒤 receipt 복구도 실제 시작 완료로 기록한다"
    );
    assert_eq!(history.today, 1);
    // 이전 자동 thread가 열린 채여도 다른 사건은 제한을 생략했으므로 실행된다.
    l.h.automation_submit("demo", &r, "due:2", "second push".into(), None).unwrap();
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 4);
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 2);
    assert_eq!(l.h.load("demo", &one.id).unwrap().holds, vec!["timing"]);
}

#[tokio::test]
async fn start_propose_keeps_one_durable_proposal_without_creating_thread() {
    let l = Lab::new();
    let a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    approve(&l, &r);
    for _ in 0..2 {
        l.h.automation_submit("demo", &r, "due:1", "data".into(), None).unwrap();
        l.h.automation_drain("demo", &r).await.unwrap();
    }
    let proposals: Vec<_> =
        l.h.db.inbox_list(Some("demo"), true).unwrap().into_iter().filter(|i| i.event == "proposed").collect();
    assert_eq!(proposals.len(), 1);
    assert!(l.h.db.threads(Some("demo")).unwrap().is_empty());
    assert_eq!(starts(&a), 0);
    let records = l.h.routine_history("demo", "check").unwrap();
    assert!(records.runs.iter().any(|r| r.detail.contains("제안")));
}

#[test]
fn start_transaction_rechecks_late_hold_and_durable_receipt_before_insert() {
    let l = Lab::new();
    let holder = seed(&l, "m", &["timing"]);
    let mut policy = crate::db::StartAdmission {
        machine: "m".into(),
        holds: vec!["timing".into()],
        rule: "check".into(),
        max_running: None,
        max_auto_threads: 0,
    };
    let insert = |key: &str, policy: &crate::db::StartAdmission| {
        l.h.db.thread_insert_marked("demo", Some(key), Some(policy), |id| {
            let mut r =
                ThreadRec::starting("demo", "점검", "m", "claude", Role::Worker, ThreadKind::Dir, util::now_ms());
            r.id = id.into();
            r.automation = Some("check".into());
            serde_json::to_string(&r).unwrap()
        })
    };
    assert!(insert("start:1", &policy).unwrap_err().to_string().contains("start_wait"));
    assert!(l.h.db.meta("start:1").unwrap().is_none());
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 1);
    l.h.update_rec("demo", &holder.id, |r| r.status = ThreadStatus::Resolved).unwrap();
    let (first, created) = insert("start:1", &policy).unwrap();
    assert!(created);
    policy.max_running = Some(1);
    let (again, created) = insert("start:1", &policy).unwrap();
    assert_eq!(first, again);
    assert!(!created);
    assert!(insert("start:2", &policy).is_err());
    policy.max_running = None;
    assert!(insert("start:2", &policy).unwrap().1);
    // 파일을 다시 여는 새 DB 연결에도 영수증이 남는다.
    let reopened = crate::db::Db::open(&l.root.join("host/host.db")).unwrap();
    assert_eq!(reopened.meta("start:1").unwrap(), Some(first));
}

#[tokio::test]
async fn start_idlest_counts_working_threads_before_list_order() {
    let l = auto_lab();
    let _a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    approve(&l, &r);
    let busy = seed(&l, "m", &[]);
    l.h.on_session("m", info("busy"), false);
    l.h.update_rec("demo", &busy.id, |t| {
        t.pane = Some("busy".into());
        t.delivered_at_ms = Some(util::now_ms());
    })
    .unwrap();
    assert_eq!(l.h.thread_show("demo", &busy.id).unwrap().group, ThreadGroup::Working);
    l.h.automation_submit("demo", &r, "due:1", "data".into(), None).unwrap();
    l.h.automation_drain("demo", &r).await.unwrap();
    let id = l.h.db.meta(&crate::auto_start::start_key("demo", "check", "due:1")).unwrap().unwrap();
    assert_eq!(l.h.load("demo", &id).unwrap().machine_id, "b");
    for (cpu_a, cpu_b, connected_a, expected) in [(90, 10, true, "b"), (10, 10, true, "m"), (0, 90, false, "b")] {
        let l = auto_lab();
        let _a = l.fake(vec![]).await;
        let _b = l.fake_on("b", "mac-b", vec![]).await;
        let r = rule(&l, "");
        approve(&l, &r);
        for (mid, cpu) in [("m", cpu_a), ("b", cpu_b)] {
            l.h.on_health(mid,serde_json::from_value(serde_json::json!({"at_ms":util::now_ms(),"cpu_pct":cpu,"cpus":8,"toolchain":[],"problems":[],"warnings":[]})).unwrap());
        }
        if !connected_a {
            l.h.agent_disconnected("m", 1);
        }
        l.h.automation_submit("demo", &r, "due:1", "data".into(), None).unwrap();
        l.h.automation_drain("demo", &r).await.unwrap();
        let id = l.h.db.meta(&crate::auto_start::start_key("demo", "check", "due:1")).unwrap().unwrap();
        assert_eq!(l.h.load("demo", &id).unwrap().machine_id, expected, "CPU {cpu_a}/{cpu_b}, 연결 {connected_a}");
    }
}

#[tokio::test]
async fn start_explicit_wait_expiry_and_changed_brief_notice_do_not_repeat() {
    let l = auto_lab();
    let _a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "wait_max='1m'");
    approve(&l, &r);
    seed(&l, "m", &["timing"]);
    seed(&l, "b", &["timing"]);
    l.h.automation_start_observe("demo", &r).unwrap();
    l.h.automation_submit("demo", &r, "due:1", "data".into(), None).unwrap();
    let now = util::now_ms();
    l.h.automation_drain_at("demo", &r, now).await.unwrap();
    l.h.automation_drain_at("demo", &r, now + 60_001).await.unwrap();
    l.h.automation_drain_at("demo", &r, now + 120_000).await.unwrap();
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 2);
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 0);
    let mut changed = r.clone();
    let Action::Start(a) = &mut changed.action else { panic!() };
    a.text = Some("새 작업 본문".into());
    l.h.automation_start_observe("demo", &changed).unwrap();
    l.h.automation_start_observe("demo", &changed).unwrap();
    let notices = l.h.db.inbox_list(Some("demo"), true).unwrap();
    assert_eq!(notices.iter().filter(|i| i.event == "wait expired").count(), 1);
    let changed: Vec<_> = notices.iter().filter(|i| i.event == "changed").collect();
    assert_eq!(changed.len(), 1);
    assert!(changed[0].body.contains("새 작업 본문"));
}

#[tokio::test]
async fn start_schedule_without_coordinator_keeps_the_due_slot_across_cursor_replay() {
    let l = auto_lab();
    let _a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    approve(&l, &r);
    let dir = l.h.project_dir("demo").unwrap();
    let last = util::now_ms() - 600_001;
    let saved = serde_json::json!({"last_run_ms":last}).to_string();
    l.h.db.set_meta("routine:demo:check", &saved).unwrap();
    l.h.routine_pass("demo", &dir, false);
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 1, "일정형 자동 시작은 coordinator pane이 없어도 실행한다");
    let event = format!("schedule:{}", last + 600_000);
    let receipt = l.h.db.meta(&crate::auto_start::start_key("demo", "check", &event)).unwrap();
    assert!(receipt.is_some(), "poll 시간이 아니라 예정 차례를 key로 쓴다");
    // source cursor를 저장하기 전 crash를 재현한다. 전달은 이미 thread와 함께 내구 확정됐다.
    l.h.db.set_meta("routine:demo:check", &saved).unwrap();
    l.h.routine_pass("demo", &dir, false);
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 1);
    assert_eq!(l.h.db.meta(&crate::auto_start::start_key("demo", "check", &event)).unwrap(), receipt);
}

#[tokio::test]
async fn start_changed_scope_cancels_old_input_and_body_updates_keep_approval() {
    let l = auto_lab();
    let _a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    approve(&l, &r);
    l.h.automation_submit("demo", &r, "scope:old", "data".into(), None).unwrap();
    let mut current = r.clone();
    let Action::Start(a) = &mut current.action else { panic!() };
    a.machines.pop();
    l.h.automation_drain("demo", &current).await.unwrap();
    assert!(l.h.db.threads(Some("demo")).unwrap().is_empty());
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 0);
    l.h.automation_submit("demo", &current, "body:next", "data".into(), None).unwrap();
    let Action::Start(a) = &mut current.action else { panic!() };
    a.text = Some("현재 작업 본문".into());
    l.h.automation_drain("demo", &current).await.unwrap();
    let id = l.h.db.meta(&crate::auto_start::start_key("demo", "check", "body:next")).unwrap().unwrap();
    let body = std::fs::read_to_string(l.h.project_dir("demo").unwrap().join(format!("threads/{id}.task.md"))).unwrap();
    assert!(body.contains("현재 작업 본문"));
    l.h.automation_submit("demo", &current, "action:old", "data".into(), None).unwrap();
    current.action = Action::Inbox(Default::default());
    l.h.automation_drain("demo", &current).await.unwrap();
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 1);
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 1);
    assert!(
        l.h.routine_history("demo", "check")
            .unwrap()
            .runs
            .iter()
            .any(|r| r.id == "action:old" && r.status == "skipped")
    );
}

/// A2의 runtime owner: 33건은 한 번에 32건만 전달하고, propose는 기존 thread의 이어 보내기를 막지 않는다.
#[tokio::test]
async fn reuse_open_batches_32_and_queues_without_new_thread_even_in_propose() {
    let l = Lab::new();
    let a = l.fake(vec!["mission.v1".into()]).await;
    let b = l.fake_on("b", "mac-b", vec!["mission.v1".into()]).await;
    let mut r = rule(&l, "reuse='open'");
    r.trigger = crate::routine::Trigger::Thread(automation::Threads {
        events: vec!["thread_failed".into()],
        repos: vec![],
        include_automation: false,
    });
    std::fs::write(
        l.h.project_dir("demo").unwrap().join("automations/check.toml"),
        toml::to_string(&automation::File::from_routine(&r)).unwrap(),
    )
    .unwrap();
    approve(&l, &r);
    let older = seed(&l, "m", &[]);
    let newer = seed(&l, "b", &[]);
    l.h.on_session("b", info("s1"), false);
    l.h.on_harness(
        "b",
        serde_json::from_value(
            serde_json::json!({"session":"s1","harness":"claude","kind":"idle","at_ms":util::now_ms()}),
        )
        .unwrap(),
    );
    for (id, at) in [(&older.id, 1), (&newer.id, 2)] {
        l.h.update_rec("demo", id, |t| {
            t.automation = Some("check".into());
            t.created_at_ms = at;
            t.pane = Some("s1".into());
            t.work_dir = Some("/lab/.aaa/demo-t-0001".into());
        })
        .unwrap();
    }
    for n in 0..33 {
        l.h.automation_submit("demo", &r, &format!("input-{n}"), format!("body-{n}"), None).unwrap();
    }
    // 승인 hash는 이어 보내기·기억·보고 첨부·끝내기 설정에 영향받지 않는다.
    let mut next = r.clone();
    let Action::Start(s) = &mut next.action else { panic!() };
    s.memory = true;
    s.reports = true;
    s.close_idle = Some("30m".into());
    assert_eq!(
        l.h.automation_start_approval("demo", &r).unwrap(),
        l.h.automation_start_approval("demo", &next).unwrap()
    );
    l.h.automation_drain("demo", &r).await.unwrap();
    let queue = l.h.automation_queue("demo", "check").unwrap();
    assert_eq!((queue.pending, queue.inflight), (1, None));
    let writes: Vec<_> = b
        .requests
        .lock()
        .unwrap()
        .iter()
        .filter_map(|q| match q {
            AgentRequest::WorkFileWrite { data, .. } => Some(data.0.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(writes.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&writes[0]).unwrap();
    assert_eq!(body["inputs"].as_array().unwrap().len(), 32);
    assert!(l.h.routine_runs("demo", "check").unwrap().iter().all(|run| run.thread.as_deref() == Some(&newer.id)));
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, 0);
    assert_eq!(starts(&a) + starts(&b), 0);
    // 같은 입력 receipt는 다시 전달하지 않는다.
    l.h.automation_submit("demo", &r, "input-32", "body-32".into(), None).unwrap();
    assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, 0);
    for id in [&older.id, &newer.id] {
        l.h.update_rec("demo", id, |r| r.status = ThreadStatus::Resolved).unwrap();
    }
    l.h.automation_submit("demo", &r, "fresh", "다음 입력".into(), None).unwrap();
    l.h.automation_drain("demo", &r).await.unwrap();
    assert!(l.h.routine_runs("demo", "check").unwrap().iter().any(|r| r.detail.contains("제안")));
    assert_eq!(starts(&a) + starts(&b), 0);
}

#[tokio::test]
async fn reuse_rejects_codex_without_compaction_reentry_hook() {
    let mut l = Lab::new();
    Arc::get_mut(&mut l.h)
        .unwrap()
        .cfg
        .profiles
        .insert("builder".into(), aaa_config::Profile { harness: "codex".into(), ..Default::default() });
    let _a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let mut r = rule(&l, "reuse='open'");
    let Action::Start(a) = &mut r.action else { panic!() };
    a.profiles = vec!["builder".into()];
    assert!(l.h.automation_start_approval("demo", &r).unwrap_err().message.contains("compaction"));
}

#[tokio::test]
async fn start_reports_are_optional_capped_snapshots_of_source_threads() {
    let l = auto_lab();
    let _a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let mut r = rule(&l, "");
    let root = l.h.project_dir("demo").unwrap();
    std::fs::write(root.join("threads/t-0099.md"), "x".repeat(70000)).unwrap();
    for (id, reports) in [("without", false), ("with", true)] {
        let Action::Start(a) = &mut r.action else { panic!() };
        a.reports = reports;
        // 서로 다른 사건을 받기 위해 일정 coalescing은 쓰지 않는다.
        r.trigger = crate::routine::Trigger::Thread(automation::Threads {
            events: vec!["thread_failed".into()],
            repos: vec![],
            include_automation: false,
        });
        l.h.automation_submit(
            "demo",
            &r,
            id,
            serde_json::json!({"owners":["t-0099","../../outside"],"reports":{"stale":"old"}}).to_string(),
            Some("t-0099".into()),
        )
        .unwrap();
    }
    let Action::Start(a) = &mut r.action else { panic!() };
    a.reuse = automation::Reuse::Open;
    let error = l.h.automation_submit("demo", &r, "oversized", "x".repeat(512 * 1024), None).unwrap_err();
    assert!(error.message.contains("512KiB"));
    let raw: serde_json::Value = serde_json::from_str(&l.h.db.meta("automation:demo:check").unwrap().unwrap()).unwrap();
    assert_eq!(raw["pending"].as_array().unwrap().len(), 2, "거부한 입력은 접수하지 않는다");
    let first: serde_json::Value = serde_json::from_str(raw["pending"][0]["body"].as_str().unwrap()).unwrap();
    assert!(first.get("reports").is_none());
    let second: serde_json::Value = serde_json::from_str(raw["pending"][1]["body"].as_str().unwrap()).unwrap();
    assert_eq!(second["reports"]["t-0099"].as_str().unwrap().len(), 65536);
    assert_eq!(second["reports"].as_object().unwrap().len(), 1);
    std::fs::write(root.join("threads/t-0099.md"), "changed").unwrap();
    assert!(second["reports"]["t-0099"].as_str().unwrap().bytes().all(|b| b == b'x'));
}

#[tokio::test]
async fn memory_return_preserves_edits_made_on_host_while_thread_is_open() {
    for features in [vec!["mission.v1".into()], vec!["work_files.v1".into()]] {
        let mut l = auto_lab();
        let _peer = l.fake(features.clone()).await;
        let rule = rule(&l, "reuse='open'\nmemory=true");
        let Action::Start(a) = &rule.action else { panic!() };
        let mut rec = seed(&l, "m", &[]);
        let work = l.root.join("work/.aaa/demo-memory");
        std::fs::create_dir_all(&work).unwrap();
        let memory = l.h.project_dir("demo").unwrap().join("automations/check.memory");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(memory.join("memory.md"), "이전 판단\n").unwrap();
        rec =
            l.h.update_rec("demo", &rec.id, |r| {
                r.automation = Some("check".into());
                r.work_dir = Some(work.to_string_lossy().into());
                r.automation_run = Some(crate::automation_thread::Run::new(a, util::now_ms()));
            })
            .unwrap();
        l.h.automation_prepare(&rec).await.unwrap();
        std::fs::write(memory.join("memory.md"), "이전 판단\ncoordinator가 남긴 줄\n").unwrap();
        std::fs::write(work.join("memory/memory.md"), "thread의 새 판단\n").unwrap();
        std::fs::create_dir_all(work.join("memory/history")).unwrap();
        std::fs::write(work.join("memory/history/before.md"), "이전 판단\n").unwrap();
        std::fs::create_dir_all(memory.join("history")).unwrap();
        std::fs::write(memory.join("history/coordinator.md"), "사용자 원문\n").unwrap();
        l.h.automation_copy_memory(&l.h.load("demo", &rec.id).unwrap(), true).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(memory.join("memory.md")).unwrap(),
            "이전 판단\ncoordinator가 남긴 줄\n",
            "열린 동안 host에서 쓴 줄이 회수로 사라졌다"
        );
        assert_eq!(std::fs::read_to_string(memory.join("history/before.md")).unwrap(), "이전 판단\n");
        assert_eq!(std::fs::read_to_string(memory.join("history/coordinator.md")).unwrap(), "사용자 원문\n");
        // 새 host owner가 JSON/DB의 공통 판과 같은 agent 파일로 마지막 회수를 이어 간다.
        l.h = Host::open(l.h.cfg.clone(), l.h.state.clone()).unwrap();
        let _reconnected = l.fake(features).await;
        let stored = l.h.load("demo", &rec.id).unwrap();
        assert!(stored.automation_run.as_ref().unwrap().memory_common.is_some());
        // 오래된 rec을 다시 넘겨도 최신 DB의 기준을 쓴다.
        l.h.automation_copy_memory(&rec, true).await.unwrap();
        let conflicts = memory.with_file_name("check.memory-conflicts").join(&rec.id);
        let copies: Vec<_> = std::fs::read_dir(conflicts).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(copies.len(), 1);
        assert_eq!(std::fs::read_to_string(copies[0].join("thread/memory.md")).unwrap(), "thread의 새 판단\n");
        assert_eq!(
            std::fs::read_to_string(copies[0].join("host/memory.md")).unwrap(),
            "이전 판단\ncoordinator가 남긴 줄\n"
        );
        let items = l.h.db.inbox_list(Some("demo"), false).unwrap();
        assert_eq!(
            items
                .iter()
                .filter(|i| i.item_kind == "automation-memory" && i.event.starts_with("memory conflict:"))
                .count(),
            1
        );
        // 양쪽은 각각 전송 상한 이하여도 합친 history가 1000개를 넘으면 원문을 교체하지 않는다.
        for i in 0..500 {
            std::fs::write(memory.join(format!("history/host-{i}.md")), "host").unwrap();
            std::fs::write(work.join(format!("memory/history/thread-{i}.md")), "thread").unwrap();
        }
        let before = std::fs::read(memory.join("memory.md")).unwrap();
        let error = l.h.automation_copy_memory(&rec, true).await.unwrap_err();
        assert!(error.message.contains("합친 기억 이동 상한"));
        assert_eq!(std::fs::read(memory.join("memory.md")).unwrap(), before);
        assert!(memory.join("history/host-499.md").is_file());
        assert!(!memory.join("history/thread-0.md").exists());
        assert!(work.join("memory/history/thread-499.md").is_file());
    }
}

#[tokio::test]
async fn previous_agent_keeps_input_and_memory_transfer_with_legacy_file_frames() {
    let l = auto_lab();
    let peer = l.fake(vec!["mission.v1".into()]).await;
    let rule = rule(&l, "reuse='open'\nmemory=true\nclose_idle='30m'");
    let Action::Start(a) = &rule.action else { panic!() };
    let mut rec = seed(&l, "m", &[]);
    let work = l.root.join("work/.aaa/demo-old");
    std::fs::create_dir_all(&work).unwrap();
    let memory = l.h.project_dir("demo").unwrap().join("automations/check.memory");
    std::fs::create_dir_all(&memory).unwrap();
    std::fs::write(memory.join("memory.md"), "이전 판단\n").unwrap();
    rec =
        l.h.update_rec("demo", &rec.id, |r| {
            r.automation = Some("check".into());
            r.work_dir = Some(work.to_string_lossy().into());
            let mut run = crate::automation_thread::Run::new(a, util::now_ms());
            run.first_input = Some(("old-input".into(), "CI 자료".into()));
            r.automation_run = Some(run);
        })
        .unwrap();
    l.h.automation_prepare(&rec).await.unwrap();
    assert_eq!(std::fs::read_to_string(work.join("memory/memory.md")).unwrap(), "이전 판단\n");
    // unchunked input write is part of the old link too.
    assert!(
        peer.requests
            .lock()
            .unwrap()
            .iter()
            .any(|q| matches!(q,AgentRequest::WorkFileWrite{path,..} if path=="inbox/old-input.json"))
    );
    std::fs::write(work.join("memory/memory.md"), "갱신 판단\n").unwrap();
    l.h.automation_copy_memory(&l.h.load("demo", &rec.id).unwrap(), false).await.unwrap();
    assert_eq!(std::fs::read_to_string(memory.join("memory.md")).unwrap(), "갱신 판단\n");
    l.h.automation_close_pass("demo").await;
    let log = peer.requests.lock().unwrap();
    assert!(log.iter().any(|q| matches!(q, AgentRequest::MissionStateWrite { .. })));
    assert!(log.iter().any(|q| matches!(q, AgentRequest::MissionStateRead { .. })));
    assert!(log.iter().any(|q| matches!(q, AgentRequest::MissionInboxStatus { .. })));
    assert!(
        !log.iter().any(|q| matches!(
            q,
            AgentRequest::WorkFileRead { .. }
                | AgentRequest::WorkFileList { .. }
                | AgentRequest::WorkFileWriteChunk { .. }
                | AgentRequest::WorkFileInboxStatus { .. }
        )),
        "1.9 agent에 새 태그를 보냈다"
    );
}
