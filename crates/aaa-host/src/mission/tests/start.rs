//! L5 owner 계약: 실제 host/DB와 기존 agent frame fixture를 사용한다.
use super::*;
use crate::automation::{self, Action};

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
    assert!(automatic[0].mission.is_none());
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
