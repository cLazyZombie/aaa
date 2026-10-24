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
        Self::with_commands(false)
    }
    fn with_commands(commands: bool) -> Self {
        let root = std::env::temp_dir().join(format!("aaa-automation-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        if commands {
            cfg.projects.routine_commands.push("demo".into());
        }
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        Self { h, root }
    }
    async fn fake(&self, features: Vec<String>) -> Fake {
        self.fake_on("m", "mac-a", features).await
    }
    async fn fake_on(&self, mid: &str, name: &str, features: Vec<String>) -> Fake {
        self.fake_capabilities(mid, name, features, true).await
    }
    async fn fake_capabilities(&self, mid: &str, name: &str, mut features: Vec<String>, profiles: bool) -> Fake {
        let machine = mid.to_string();
        if profiles {
            features.push("host_profiles.v1".into());
        }
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
                sha256: None,
                selfcheck: None,
                sessions: vec![],
                identity: None,
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
        responsibility: None,
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

/// 브라우저·실제 로그인·구독 요청 없이 웹과 같은 내구 캐시로 원격 Codex 자동화를 판정한다.
#[tokio::test]
async fn quota_gate_codex_uses_bound_account_and_preserves_pending_until_each_window_recovers() {
    let mut l = auto_lab();
    Arc::get_mut(&mut l.h)
        .unwrap()
        .cfg
        .profiles
        .insert("builder".into(), aaa_config::Profile { harness: "codex".into(), ..Default::default() });
    let _a = l.fake(vec![]).await;
    let agent = l.fake_on("b", "mac-b", vec!["work_files.v1".into()]).await;
    let auth = r#"{"tokens":{"access_token":"fixture-access-only","refresh_token":"fixture-refresh-only"}}"#;
    for account in ["work", "other"] {
        l.h.accounts.import(account, "codex", auth).unwrap();
    }
    l.h.set_default_account("codex", Some("work")).unwrap();
    let mut r = rule(&l, "reuse='open'");
    let Action::Start(a) = &mut r.action else { panic!() };
    a.profiles = vec!["builder".into()];
    r.limits.usage_session_below = Some(5.0);
    r.limits.usage_week_below = Some(10.0);
    let root = l.h.project_dir("demo").unwrap();
    std::fs::write(root.join("automations/check.toml"), toml::to_string(&automation::File::from_routine(&r)).unwrap())
        .unwrap();
    approve(&l, &r);
    let generation = l.h.accounts.generation("work").unwrap();
    let cache = |generation: &str, session: Option<f64>, week: f64| {
        let now = util::now_ms();
        let windows: Vec<_> = [(300, session), (10080, Some(week))].into_iter().filter_map(|(duration, used)| {
            used.map(|used| serde_json::json!({"limit_id":"codex","window_id":format!("w-{duration}"),"scope":null,"duration_mins":duration,"used_percent":used,"observed_at_ms":now,"resets_at_ms":now+3600000}))
        }).collect();
        let entry = serde_json::json!({"generation":generation,"attempted_ms":now,"next_ms":now+60000,"readings":{"host":{"machine":"host","received_at_ms":now,"snapshot":{"generation":generation,"source":"codex_rpc","sampled_at_ms":now,"problem":null,"retry_at_ms":null,"windows":windows}}}});
        l.h.db.set_meta("account-quota-v1/work", &entry.to_string()).unwrap();
    };
    cache(&generation, Some(50.0), 19.0);
    let ready = crate::auto_start::Decision::Ready(crate::auto_start::Choice {
        machine: "b".into(),
        profile: "builder".into(),
        repo: None,
    });
    assert!(!l.h.machine_is_local("b"), "원격 실행 fixture");
    // 로컬 로그인 캐시는 비어 있지만 원격 머신도 등록 기본 계정으로 판정한다.
    let updating = l.h.account_quota.lock().await;
    assert_eq!(l.h.automation_start_usage("demo", &ready, &r.limits).await.unwrap(), automation::usage::Check::Allowed);
    drop(updating);
    let rec = seed(&l, "b", &[]);
    l.h.update_rec("demo", &rec.id, |t| {
        t.profile = "builder".into();
        t.account = Some("work".into());
        t.automation = Some("check".into());
        t.pane = Some("s1".into());
        t.work_dir = Some(l.root.join("work").to_string_lossy().into());
    })
    .unwrap();
    // 기본 계정의 다른 신원·빈 사용량으로 기존 thread를 막거나 통과시키지 않는다.
    l.h.set_default_account("codex", Some("other")).unwrap();
    assert!(matches!(
        l.h.automation_start_usage("demo", &ready, &r.limits).await.unwrap(),
        automation::usage::Check::Unknown(_)
    ));
    l.h.automation_submit("demo", &r, "input-quota", "보존할 입력".into(), None).unwrap();
    cache(&generation, None, 19.0);
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, 1);
    assert!(l.h.routine_runs("demo", "check").unwrap().iter().any(|r| r.detail.contains("세션(5시간) 창 없음")));
    cache("previous-login", Some(0.0), 0.0);
    assert!(
        matches!(
            l.h.automation_start_usage("demo", &crate::auto_start::Decision::Reuse(rec.id.clone()), &r.limits)
                .await
                .unwrap(),
            automation::usage::Check::Unknown(_)
        ),
        "이전 로그인 세대는 배제"
    );
    cache(&generation, Some(96.0), 19.0);
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, 1);
    assert!(l.h.routine_runs("demo", "check").unwrap().iter().any(|r| r.detail == "사용량 문턱"));
    cache(&generation, Some(50.0), 19.0);
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, 0);
    l.h.automation_submit("demo", &r, "input-quota", "보존할 입력".into(), None).unwrap();
    l.h.automation_drain("demo", &r).await.unwrap();
    assert_eq!(starts(&agent), 0);
    let writes =
        agent.requests.lock().unwrap().iter().filter(|q| matches!(q, AgentRequest::WorkFileWrite { .. })).count();
    assert_eq!(writes, 1, "보존한 입력은 열린 thread에 한 번만 전달");
    l.h.accounts.remove("work").unwrap();
    assert!(
        matches!(
            l.h.automation_start_usage("demo", &crate::auto_start::Decision::Reuse(rec.id.clone()), &r.limits)
                .await.unwrap(),
            automation::usage::Check::Unknown(s) if s.contains("캐시 조회 실패")
        ),
        "삭제한 계정의 캐시는 실행 근거가 되지 않는다"
    );
    l.h.set_default_account("codex", None).unwrap();
    assert!(matches!(
        l.h.automation_start_usage("demo", &ready, &r.limits).await.unwrap(),
        automation::usage::Check::Unknown(s) if s.contains("등록 계정 없음")
    ));
    assert_eq!(
        l.h.automation_start_usage("demo", &ready, &automation::Limits::default()).await.unwrap(),
        automation::usage::Check::Allowed
    );
}

#[tokio::test]
async fn start_admission_owns_the_thread_before_recovery_can_observe_it() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let l = auto_lab();
    let a = l.fake(vec![]).await;
    let b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    approve(&l, &r);
    let observed = Arc::new(AtomicBool::new(false));
    let captured = observed.clone();
    let weak = Arc::downgrade(&l.h);
    // DB commit 직후, 최초 기동이 ThreadPrepare를 보내기 전에 복구기를 끼워 넣는다.
    // 실제 자동화 시작 경로를 쓰며 sleep·스케줄러의 우연에 기대지 않는다.
    *l.h.start_admitted_hook.lock().unwrap() = Some(Arc::new(move |id| {
        let h = weak.upgrade().unwrap();
        h.recover_launches("demo");
        let rec = h.load("demo", id).unwrap();
        captured.store(
            rec.status == ThreadStatus::Starting && rec.error.is_none() && h.claim_thread("demo", id).is_err(),
            Ordering::SeqCst,
        );
    }));
    let mut events = l.h.thread_events.subscribe();
    l.h.automation_submit("demo", &r, "due:admission", "기동 선점 대조".into(), None).unwrap();
    let result = l.h.automation_drain_at("demo", &r, util::now_ms()).await;
    *l.h.start_admitted_hook.lock().unwrap() = None;
    assert!(observed.load(Ordering::SeqCst), "DB에 보인 새 기동을 복구기가 실패로 바꾸거나 선점했다");
    result.unwrap();
    let records = l.h.db.threads(Some("demo")).unwrap();
    assert_eq!(records.len(), 1);
    let rec: ThreadRec = serde_json::from_str(&records[0]).unwrap();
    launched(&l, &mut events, &rec.id).await;
    assert_eq!(starts(&a) + starts(&b), 1, "복구와 경주해도 실제 기동은 한 번이다");
}

#[tokio::test]
async fn failed_start_admission_rolls_back_the_row_and_releases_the_claim() {
    let l = auto_lab();
    let _a = l.fake(vec![]).await;
    let _b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    approve(&l, &r);
    let sql = rusqlite::Connection::open(l.root.join("host/host.db")).unwrap();
    sql.execute_batch(
        "CREATE TRIGGER reject_admission BEFORE INSERT ON threads BEGIN SELECT RAISE(ABORT,'admission rejected'); END;",
    )
    .unwrap();
    // drainer는 시작 오류를 내구 대기로 돌린다. 여기서는 실제 시작 결과를 직접 검사한다.
    let now = util::now_ms();
    let crate::automation::Action::Start(start) = &r.action else { unreachable!() };
    let event = crate::automation::runtime::Event {
        id: "due:rejected".into(),
        at_ms: now,
        owner: None,
        body: "입장 실패 대조".into(),
        action: r.action.clone(),
        source: "test".into(),
        start_scope: Some(l.h.automation_start_scope("demo", &r, start).unwrap()),
        observe: None,
        revision: String::new(),
    };
    let error = l.h.automation_start("demo", &r, &event, now, "rejected-input").await.err().expect("입장 SQL 오류");
    assert!(error.message.contains("admission rejected"), "{error:?}");
    assert!(l.h.db.threads(Some("demo")).unwrap().is_empty());
    assert!(l.h.db.meta(&crate::auto_start::start_key("demo", "check", "due:rejected")).unwrap().is_none());
    assert!(l.h.claim_thread("demo", "t-0001").is_ok(), "롤백된 행의 기동 잠금이 남았다");
}

async fn launched(l: &Lab, events: &mut tokio::sync::broadcast::Receiver<ThreadView>, id: &str) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while l.h.load("demo", id).unwrap().status == ThreadStatus::Starting {
            events.recv().await.unwrap();
        }
        let rec = l.h.load("demo", id).unwrap();
        assert_eq!(rec.status, ThreadStatus::Open, "{:?}", rec.error);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn rollout_preserves_flow_input_and_scope_until_agent_capability_returns() {
    let mut l = auto_lab();
    let a = l.fake(vec!["mission.v1".into()]).await;
    let b = l.fake_on("b", "mac-b", vec!["mission.v1".into()]).await;
    let mut r = rule(&l, "reuse='open'");
    r.trigger = crate::routine::Trigger::Thread(automation::Threads {
        events: vec!["report_ready".into()],
        repos: vec![],
        include_automation: false,
    });
    std::fs::write(
        l.h.project_dir("demo").unwrap().join("automations/check.toml"),
        toml::to_string(&automation::File::from_routine(&r)).unwrap(),
    )
    .unwrap();
    approve(&l, &r);
    let scope = l.h.automation_start_approval("demo", &r).unwrap();
    l.h.automation_pass("demo").await; // 등록 cursor
    l.h.agent_disconnected("m", 1);
    l.h.agent_disconnected("b", 1);
    assert_eq!(l.h.automation_start_approval("demo", &r).unwrap(), scope, "연결은 승인 범위가 아니다");
    let old_a = l.fake_capabilities("m", "mac-a", vec!["mission.v1".into()], false).await;
    let old_b = l.fake_capabilities("b", "mac-b", vec!["mission.v1".into()], false).await;
    assert_eq!(l.h.automation_start_approval("demo", &r).unwrap(), scope, "agent 판은 승인 범위가 아니다");
    let source = seed(&l, "m", &[]);
    l.h.flow(&source, "report_ready", None, "입력 자료");
    l.h.automation_pass("demo").await;
    let waiting = l.h.routines("demo").unwrap().into_iter().find(|r| r.name == "check").unwrap();
    assert!(waiting.error.is_none(), "{:?}", waiting.error);
    assert_eq!(waiting.start_approved, Some(true));
    assert_eq!(waiting.queue.unwrap().pending, 1);
    let run = waiting.last_result.unwrap();
    assert_eq!(run.status, "waiting");
    assert!(run.id.starts_with("flow:"));
    assert!(run.detail.contains("host_profiles.v1 없음") && run.detail.contains("다시 시도 가능"), "{}", run.detail);
    assert_eq!(starts(&a) + starts(&b) + starts(&old_a) + starts(&old_b), 0);
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 0);
    // 새 host도 저장된 cursor·입력·승인을 이어 받는다. 새 agent 연결 뒤 같은 사건을 한 번만 시작한다.
    l.h = Host::open(l.h.cfg.clone(), l.h.state.clone()).unwrap();
    let new = l.fake(vec!["mission.v1".into()]).await;
    let _still_old = l.fake_capabilities("b", "mac-b", vec!["mission.v1".into()], false).await;
    let mut events = l.h.thread_events.subscribe();
    l.h.automation_pass("demo").await;
    let done = l.h.routines("demo").unwrap().into_iter().find(|r| r.name == "check").unwrap();
    let queue = done.queue.unwrap();
    assert_eq!(queue.pending, 0, "{:?}", done.last_result);
    assert!(queue.inflight.is_none());
    let done = done.last_result.unwrap();
    assert_eq!(done.id, run.id);
    assert_eq!(done.status, "done");
    assert!(done.thread.is_some());
    launched(&l, &mut events, done.thread.as_deref().unwrap()).await;
    l.h.automation_submit("demo", &r, &run.id, "같은 입력 재생".into(), None).unwrap();
    l.h.automation_pass("demo").await;
    assert_eq!(starts(&new), 1, "{:?}", l.h.routine_runs("demo", "check").unwrap());
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 1);
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 2, "원인 thread 하나와 자동 thread 하나");
}

#[tokio::test]
async fn rollout_selects_capable_candidate_while_an_older_agent_is_connected() {
    let l = auto_lab();
    let old = l.fake_capabilities("m", "mac-a", vec![], false).await;
    let new = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    approve(&l, &r);
    let mut events = l.h.thread_events.subscribe();
    l.h.automation_submit("demo", &r, "rollout:1", "자료".into(), None).unwrap();
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    let id = l.h.db.meta(&crate::auto_start::start_key("demo", "check", "rollout:1")).unwrap().unwrap();
    launched(&l, &mut events, &id).await;
    assert_eq!(starts(&old), 0);
    assert_eq!(starts(&new), 1);
    assert_eq!(l.h.load("demo", &id).unwrap().machine_id, "b");
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
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    assert_eq!(starts(&a) + starts(&b), 0);
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 0);
    assert!(l.h.db.threads(Some("demo")).unwrap().is_empty());
    let one = seed(&l, "m", &["timing"]);
    let two = seed(&l, "b", &["timing"]);
    approve(&l, &r);
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    assert_eq!(starts(&a) + starts(&b), 0);
    l.h.update_rec("demo", &two.id, |rec| rec.status = ThreadStatus::Resolved).unwrap();
    let crash = rusqlite::Connection::open(l.root.join("host/host.db")).unwrap();
    crash.execute_batch("CREATE TRIGGER crash_start_receipt BEFORE INSERT ON meta WHEN NEW.key LIKE 'automation_done:%' BEGIN SELECT RAISE(ABORT,'crash after thread commit'); END;").unwrap();
    let error = l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap_err();
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
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    assert_eq!(l.h.db.meta(&crate::auto_start::start_key("demo", "check", "due:1")).unwrap(), Some(id));
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 3);
    let history = l.h.routine_history("demo", "check").unwrap();
    assert!(
        history.runs.iter().any(|r| r.id == "due:1" && r.status == "done" && r.thread.is_some()),
        "생성 뒤 receipt 복구도 실제 시작 완료로 기록한다"
    );
    assert_eq!(history.today, 1);
    // 새 기본은 규칙당 1개다. 다음 사건은 입력을 보존하며 자리가 날 때까지 기다린다.
    l.h.automation_submit("demo", &r, "due:2", "second push".into(), None).unwrap();
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 3);
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 1);
    assert!(
        l.h.routine_history("demo", "check").unwrap().runs.iter().any(|r| r.detail.contains("규칙 동시 실행 상한 1"))
    );
    let mut expanded = r.clone();
    expanded.limits.max_running = Some(2);
    l.h.automation_drain_at("demo", &expanded, util::now_ms()).await.unwrap();
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
        l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
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
    assert!(insert("start:2", &policy).unwrap_err().to_string().contains("상한 1"));
    policy.max_running = Some(0);
    assert!(insert("start:2", &policy).is_err());
    policy.max_running = Some(2);
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
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
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
        l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
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
    assert_eq!(
        l.h.db.automation_executions("demo", Some("check"), None).unwrap()[0].state,
        AutomationRunState::Skipped
    );
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
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    assert_eq!(l.h.db.threads(Some("demo")).unwrap().len(), 1, "일정형 자동 시작은 coordinator pane이 없어도 실행한다");
    let event = format!("schedule:{}", last + 600_000);
    let receipt = l.h.db.meta(&crate::auto_start::start_key("demo", "check", &event)).unwrap();
    assert!(receipt.is_some(), "poll 시간이 아니라 예정 차례를 key로 쓴다");
    // source cursor를 저장하기 전 crash를 재현한다. 전달은 이미 thread와 함께 내구 확정됐다.
    l.h.db.set_meta("routine:demo:check", &saved).unwrap();
    l.h.routine_pass("demo", &dir, false);
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
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
    l.h.automation_drain_at("demo", &current, util::now_ms()).await.unwrap();
    assert!(l.h.db.threads(Some("demo")).unwrap().is_empty());
    assert_eq!(l.h.automation_count("demo", "check").unwrap(), 0);
    l.h.automation_submit("demo", &current, "body:next", "data".into(), None).unwrap();
    let Action::Start(a) = &mut current.action else { panic!() };
    a.text = Some("현재 작업 본문".into());
    l.h.automation_drain_at("demo", &current, util::now_ms()).await.unwrap();
    let id = l.h.db.meta(&crate::auto_start::start_key("demo", "check", "body:next")).unwrap().unwrap();
    let body = std::fs::read_to_string(l.h.project_dir("demo").unwrap().join(format!("threads/{id}.task.md"))).unwrap();
    assert!(body.contains("현재 작업 본문"));
    l.h.automation_submit("demo", &current, "action:old", "data".into(), None).unwrap();
    current.action = Action::Inbox(Default::default());
    l.h.automation_drain_at("demo", &current, util::now_ms()).await.unwrap();
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
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
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
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, 0);
    assert_eq!(starts(&a) + starts(&b), 0);
    // 같은 입력 receipt는 다시 전달하지 않는다.
    l.h.automation_submit("demo", &r, "input-32", "body-32".into(), None).unwrap();
    assert_eq!(l.h.automation_queue("demo", "check").unwrap().pending, 0);
    for id in [&older.id, &newer.id] {
        l.h.update_rec("demo", id, |r| r.status = ThreadStatus::Resolved).unwrap();
    }
    l.h.automation_submit("demo", &r, "fresh", "다음 입력".into(), None).unwrap();
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    assert!(l.h.routine_runs("demo", "check").unwrap().iter().any(|r| r.detail.contains("제안")));
    assert_eq!(starts(&a) + starts(&b), 0);
}

#[tokio::test]
async fn reuse_accepts_codex_and_seeds_memory_reentry_only_when_enabled() {
    for memory in [false, true] {
        let mut l = auto_lab();
        Arc::get_mut(&mut l.h)
            .unwrap()
            .cfg
            .profiles
            .insert("builder".into(), aaa_config::Profile { harness: "codex".into(), ..Default::default() });
        let agent = l.fake(vec!["work_files.v1".into()]).await;
        let _b = l.fake_on("b", "mac-b", vec![]).await;
        rule(&l, &format!("reuse='open'\nmemory={memory}"));
        let root = l.h.project_dir("demo").unwrap();
        let file = root.join("automations/check.toml");
        let text = std::fs::read_to_string(&file)
            .unwrap()
            .replace("profiles=['claude']", "profiles=['builder']")
            .replace("machines=['mac-a','mac-b']", "machines=['mac-a']");
        std::fs::write(&file, &text).unwrap();
        let r = automation::parse(&root, "check", &text).unwrap();
        // 허용된 profile의 기존 승인 절차는 유지한다.
        approve(&l, &r);
        let mut events = l.h.thread_events.subscribe();
        l.h.automation_submit("demo", &r, "input-1", "자료".into(), None).unwrap();
        l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
        let rec = l.h.automation_open("demo", "check").unwrap().unwrap();
        launched(&l, &mut events, &rec.id).await;
        let rec = l.h.load("demo", &rec.id).unwrap();
        let spec = agent
            .requests
            .lock()
            .unwrap()
            .iter()
            .find_map(|q| match q {
                AgentRequest::Spawn(spec) => Some(spec.clone()),
                _ => None,
            })
            .expect("자동화 세션 시작");
        assert_eq!(spec.harness.as_deref(), Some("codex"));
        let reentry = spec.env.iter().find(|(key, _)| key == "AAA_AUTOMATION_REENTRY");
        assert_eq!(reentry.is_some(), memory);
        if let Some((_, text)) = reentry {
            let work = rec.work_dir.as_deref().unwrap();
            assert!(text.contains(&format!("{work}/memory/memory.md")), "{text}");
            assert!(text.contains(&format!("{work}/brief.md")), "{text}");
            assert!(text.contains("4KiB") && text.contains("processed.txt"), "{text}");
            assert!(std::path::Path::new(work).join("memory/memory.md").is_file());
        }
        // 둘째 입력도 새 thread를 만들지 않고 같은 실행에 잇는다.
        l.h.automation_submit("demo", &r, "input-2", "다음 자료".into(), None).unwrap();
        l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
        assert_eq!(l.h.automation_open("demo", "check").unwrap().unwrap().id, rec.id);
        assert_eq!(starts(&agent), 1);
    }
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

#[tokio::test]
async fn command_result_and_uncertain_intent_survive_restart_without_reexecution() {
    for lose_result in [false, true] {
        let l = Lab::with_commands(true);
        let fake = l.fake(vec![]).await;
        let root = l.h.project_dir("demo").unwrap();
        let text = "schema=2\n[trigger]\nkind='schedule'\nevery='1m'\n[action]\nkind='command'\ncommand='check-once'\nmachine='mac-a'\n";
        std::fs::write(root.join("automations/check.toml"), text).unwrap();
        let rule = automation::parse(&root, "check", text).unwrap();
        l.h.routine_approve("demo", "check", &rule.command_sha256(), "local:cli").unwrap();
        l.h.automation_submit("demo", &rule, "one", String::new(), None).unwrap();
        let sql = rusqlite::Connection::open(l.root.join("host/host.db")).unwrap();
        if lose_result {
            sql.execute_batch("CREATE TRIGGER reject_result BEFORE UPDATE ON meta WHEN NEW.key='automation:demo:check' AND json_extract(NEW.value,'$.inflight.command_result') IS NOT NULL BEGIN SELECT RAISE(ABORT,'result failed'); END;").unwrap();
        } else {
            sql.execute_batch("CREATE TRIGGER reject_receipt BEFORE INSERT ON meta WHEN NEW.key LIKE 'automation_done:%' BEGIN SELECT RAISE(ABORT,'receipt failed'); END;").unwrap();
        }
        assert!(l.h.automation_drain_at("demo", &rule, util::now_ms()).await.is_err());
        let count =
            || fake.requests.lock().unwrap().iter().filter(|r| matches!(r, AgentRequest::RunCommand { .. })).count();
        assert_eq!(count(), 1);
        sql.execute_batch("DROP TRIGGER IF EXISTS reject_result; DROP TRIGGER IF EXISTS reject_receipt;").unwrap();
        let restarted = Host::open(l.h.cfg.clone(), l.root.join("host")).unwrap();
        restarted.automation_drain_at("demo", &rule, util::now_ms()).await.unwrap();
        let run = restarted.db.automation_executions("demo", Some("check"), None).unwrap().remove(0);
        assert_eq!(run.state, if lose_result { AutomationRunState::Unknown } else { AutomationRunState::Succeeded });
        assert_eq!(count(), 1, "저장된 결과는 이어 받고 결과 불명은 재실행하지 않는다");
        if lose_result {
            assert!(restarted.automation_queue("demo", "check").unwrap().inflight.unwrap().unknown);
        }
    }
}

#[tokio::test]
async fn durable_start_receipt_recovers_success_without_restarting_or_spending_budget() {
    let l = auto_lab();
    let a = l.fake(vec![]).await;
    let b = l.fake_on("b", "mac-b", vec![]).await;
    let r = rule(&l, "");
    l.h.automation_submit("demo", &r, "due:recovered", "data".into(), None).unwrap();
    l.h.db.set_meta(&crate::auto_start::start_key("demo", &r.name, "due:recovered"), "t-0042").unwrap();
    l.h.automation_drain_at("demo", &r, util::now_ms()).await.unwrap();
    let run = l.h.db.automation_executions("demo", Some(&r.name), None).unwrap().remove(0);
    assert_eq!(run.state, AutomationRunState::Succeeded);
    assert_eq!(run.receipt, Some(AutomationReceipt::ThreadStarted));
    assert_eq!(run.thread.as_deref(), Some("t-0042"));
    assert!(run.outcome.is_none());
    assert_eq!(l.h.automation_count("demo", &r.name).unwrap(), 0);
    assert!(l.h.db.threads(Some("demo")).unwrap().is_empty());
    assert_eq!(starts(&a) + starts(&b), 0);
}

#[tokio::test]
async fn toggle_waits_for_dispatch_and_preserves_new_definition() {
    let l = auto_lab();
    let r = rule(&l, "");
    let gate = l.h.automation_dispatch_lock("demo");
    let guard = gate.lock().await;
    let future = l.h.routine_toggle("demo", &r.name, Some(false), "local:cli");
    tokio::pin!(future);
    assert!(matches!(
        std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx))).await,
        std::task::Poll::Pending
    ));
    drop(guard);
    let views = future.await.unwrap();
    assert!(!views.iter().find(|v| v.name == r.name).unwrap().enabled);
}
