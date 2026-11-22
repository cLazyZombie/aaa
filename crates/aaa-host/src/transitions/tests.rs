use super::*;

/// Hello의 앞 pane 상태를 적용한 사건 경계에서 ticker를 돌린다. 뒤 pane의 저장된 Working을
/// 아직 적용하지 않았다고 쉼(no agent) 알림을 만들면 안 된다. sleep·부하에 기대지 않는다.
#[test]
fn hello_replay_does_not_publish_idle_between_panes() {
    use aaa_common::conn::spawn_writer;
    use aaa_proto::{AgentHello, HarnessEvent, SessionInfo, SessionState};
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let roots = rt.block_on(async {
        let mut roots = Vec::new();
        for local in [true, false] {
            let root = std::env::temp_dir().join(format!("aaa-hello-{}", aaa_common::util::new_id()));
            let mut cfg = aaa_config::HostConfig::default();
            cfg.projects.root = root.join("projects").display().to_string();
            cfg.notify.web_push = false;
            let h = Host::open(cfg, root.join("host")).unwrap();
            h.project_create("p", Default::default(), "test").unwrap();
            h.db.upsert_machine("m", "mac-a", None, local).unwrap();
            let mut r = rec();
            r.pane = Some("s".into());
            r.last_group = Some(ThreadGroup::Working);
            h.db.thread_import("p", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
            let event = |session: &str, kind: &str, at_ms| HarnessEvent {
                session: session.into(),
                harness: "codex".into(),
                kind: kind.into(),
                harness_session: None,
                message: None,
                question_kind: None,
                percent: None,
                at_ms,
                transcript: None,
                plan: None,
                background: None,
                context: None,
                model_observation: None,
                codex_speed: None,
                provider_turn: None,
                turn_reply: None,
            };
            let sessions = ["first", "s"].map(|id| SessionInfo {
                responsibility: None,
                id: id.into(),
                name: "codex".into(),
                argv: vec!["codex".into()],
                cwd: None,
                pid: None,
                started_at_ms: 0,
                boot_id: "boot".into(),
                cols: 80,
                rows: 24,
                state: SessionState::Running,
                role: None,
                label: None,
                harness: Some("codex".into()),
            });
            let (done, seen) = tokio::sync::oneshot::channel();
            let during = h.clone();
            h.waiters.lock().unwrap().insert(
                "first".into(),
                vec![(
                    &["working"],
                    done,
                    Some(Box::new(move || {
                        during.thread_pass("p");
                        let items = during.inbox("p", true, true).unwrap().items;
                        assert!(items.is_empty(), "Hello 복원 중 거짓 전이: {items:?}");
                        let r = during.load("p", "t-0001").unwrap();
                        assert_eq!(during.thread_view(&r).group, ThreadGroup::Working);
                    })),
                )],
            );
            let (writer, _peer) = tokio::io::duplex(65536);
            let generation = h.next_gen();
            h.agent_connected(
                &AgentHello {
                    proto: aaa_proto::AGENT_LINK,
                    machine_id: "m".into(),
                    machine_name: "mac-a".into(),
                    boot_id: "boot".into(),
                    os: "test".into(),
                    arch: "test".into(),
                    software: "test".into(),
                    features: vec![],
                    dev_mode: true,
                    sha256: None,
                    selfcheck: None,
                    sessions: sessions.to_vec(),
                    identity: None,
                    harness: vec![event("first", "working", 1), event("s", "working", 1)],
                },
                local,
                crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), generation),
            );
            assert_eq!(seen.await.unwrap(), "working");
            h.thread_pass("p");
            assert!(h.inbox("p", true, true).unwrap().items.is_empty());
            assert_eq!(h.thread_view(&h.load("p", &r.id).unwrap()).group, ThreadGroup::Working);
            // 초기화가 끝난 뒤의 실제 쉼은 숨기지 않는다.
            h.on_harness("m", event("s", "idle", 2));
            tokio::time::timeout(std::time::Duration::from_secs(3), async {
                loop {
                    let changed = h.tick_wake.notified();
                    tokio::pin!(changed);
                    changed.as_mut().enable();
                    if !h.report_refresh.lock().unwrap().contains_key("p/t-0001") {
                        break;
                    }
                    changed.await;
                }
            })
            .await
            .unwrap();
            h.thread_pass("p");
            let items = h.inbox("p", true, true).unwrap().items;
            assert_eq!(items.len(), 1, "{items:?}");
            assert!(items[0].summary.contains("쉼(idle)"), "{items:?}");
            drop(h);
            roots.push(root);
        }
        roots
    });
    // agent_connected의 구독 task도 Host·DB를 잡고 있다. runtime을 먼저 끝내 열린 DB 파일을 닫는다.
    drop(rt);
    for root in roots {
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// 계속 일하는 thread도 명시한 착지는 알린다. adopt와 새로 띄운 thread의 보고 계약은 같다.
/// 실제 사본·DB·inbox 경계에서 hash당 한 번, ack와 작업 상태는 별개임을 본다.
#[tokio::test]
async fn working_threads_publish_landing_reports_once() {
    use sha2::Digest;
    let root = std::env::temp_dir().join(format!("aaa-landing-report-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    let mut notices = h.notice_events.subscribe();
    let (tx, mut notifications) = tokio::sync::mpsc::unbounded_channel();
    *h.notify_tx.lock().unwrap() = Some(tx);
    h.project_create("p", Default::default(), "local:cli").unwrap();
    let dir = h.project_dir("p").unwrap().join("threads");
    std::fs::create_dir_all(&dir).unwrap();
    for (id, kind) in [
        ("t-0001", aaa_proto::ThreadKind::Adopted),
        ("t-0002", aaa_proto::ThreadKind::Worktree),
        ("t-0003", aaa_proto::ThreadKind::Dir),
    ] {
        let mut r = rec();
        r.id = id.into();
        r.kind = kind;
        r.automation = (id == "t-0003").then(|| "check".into());
        r.last_group = Some(ThreadGroup::Working);
        h.db.thread_import("p", id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
        let write = |r: &mut ThreadRec, body: &str| {
            std::fs::write(dir.join(format!("{id}.md")), body).unwrap();
            r.report_hash = Some(hex::encode(sha2::Sha256::digest(body)));
            // 사람이 이미 사본을 읽고 ack해도 inbox 기록은 hash당 한 번 남아야 한다.
            r.acked_hash = r.report_hash.clone();
            h.db.thread_save("p", id, "open", &serde_json::to_string(r).unwrap()).unwrap();
        };
        write(&mut r, "[진행 중] 검증 중\n\n## Report\n중간\n");
        assert!(!h.new_report_item("p", &r));
        std::fs::write(dir.join(format!("{id}.md")), "[착지] 아직 기록에 반영되기 전 사본\n").unwrap();
        assert!(!h.new_report_item("p", &r), "다른 hash의 사본을 이 보고로 알렸다");
        write(&mut r, "[착지] 1234abcd 첫 변경\n\n## Report\n후속 작업 중\n");
        assert!(h.new_report_item("p", &r), "{kind:?}: 작업 중 착지가 inbox에서 빠졌다");
        assert!(notifications.try_recv().is_err(), "중간 착지는 inbox에만 남고 push하지 않는다");
        r = h.load("p", id).unwrap();
        assert_eq!(r.last_group, Some(ThreadGroup::Working));
        assert!(!h.new_report_item("p", &r), "같은 hash의 중복 알림");
        write(&mut r, "[착지] abcd5678 다음 변경\n\n## Report\n계속한다\n");
        assert!(h.new_report_item("p", &r));
        let items = h.inbox("p", true, true).unwrap().items;
        assert_eq!(items.iter().filter(|i| i.subject == id && i.event == "new report").count(), 2);
        r = h.load("p", id).unwrap();
        write(&mut r, "[병합 준비됨] 전체 확인 뒤 착지 창 대기\n");
        assert_eq!(h.thread_view(&r).report_attention, None);
        assert!(h.new_report_item("p", &r), "병합 대기도 coordinator inbox에 남긴다");
        r = h.load("p", id).unwrap();
        assert!(!h.new_report_item("p", &r));
        assert!(notifications.try_recv().is_err(), "병합 대기로 외부 push를 만들지 않는다");
        for (mark, attention, event) in [
            ("[완료]", aaa_proto::ReportAttention::Completed, "new report"),
            ("[결정 필요]", aaa_proto::ReportAttention::NeedsUser, "report needs user"),
            ("[검수 대기]", aaa_proto::ReportAttention::NeedsUser, "report needs user"),
            ("[확인 필요]", aaa_proto::ReportAttention::NeedsUser, "report needs user"),
            ("[사람 필요]", aaa_proto::ReportAttention::NeedsUser, "report needs user"),
        ] {
            r = h.load("p", id).unwrap();
            write(&mut r, &format!("{mark} 명시적인 알림\n## Report\n결과\n"));
            r.acked_hash = None;
            h.db.thread_save("p", id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
            assert_eq!(h.thread_view(&r).report_attention, Some(attention));
            assert!(h.new_report_item("p", &r));
            assert_eq!(notices.try_recv().is_ok(), !(id == "t-0003" && mark == "[완료]"), "{id} {mark} 앱 안 알림");
            let items = h.inbox("p", true, true).unwrap().items;
            assert!(items.iter().any(|i| i.subject == id && i.event == event), "앱 안 inbox에 보고가 남아야 한다");
            assert!(notifications.try_recv().is_err(), "thread 완료·확인 필요 보고는 외부 푸시 대기열에 넣지 않는다");
        }
        write(&mut r, "[완료인 척] 표지 아님\n[완료] 본문\n");
        assert_eq!(h.thread_view(&r).report_attention, None);
        assert!(!h.new_report_item("p", &r));
    }
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

fn rec() -> ThreadRec {
    serde_json::from_value(serde_json::json!({
        "project": "p", "id": "t-0001", "title": "t", "machine_id": "m", "profile": "x", "role": "worker",
        "kind": "dir", "status": "open", "brief": "delivered", "created_at_ms": 0, "updated_at_ms": 0
    }))
    .unwrap()
}

/// herdr-projects steps.rs `auto_resolve`: 묶음·보고가 바뀐 때와 ticker(host)가 뜬 때 중 가장 늦은 때부터 잰다
#[test]
fn auto_resolve_counts_from_the_latest_change_or_host_start() {
    let mut r = rec();
    let due = |r: &ThreadRec, now, started| auto_resolve_due(r, now, started, 100);
    assert!(!due(&r, 1_000, 0), "잴 시각이 없는 옛 기록은 끝내지 않는다");
    r.group_changed_ms = Some(500);
    assert!(due(&r, 600, 0) && !due(&r, 599, 0));
    r.report_changed_ms = Some(550);
    assert!(!due(&r, 600, 0), "보고가 더 늦게 바뀌었다");
    assert!(!due(&r, 1_000, 950), "host가 방금 떴다: 꺼져 있던 동안은 세지 않는다");
    assert!(due(&r, 1_050, 950));

    // m13 보류 keeper의 2.1초 대기 대신 실제 pass의 자격 판정을 지난 시각에도 검사한다.
    r.last_group = Some(ThreadGroup::Idle);
    assert!(auto_resolve_eligible(&r) && due(&r, 1_050, 950));
    r.status = ThreadStatus::Parked;
    assert!(due(&r, 1_050, 950));
    assert!(!auto_resolve_eligible(&r), "기한이 지난 쉼 기록이어도 보류는 자동 종료하지 않는다");
    r.status = ThreadStatus::Open;
    r.role = Role::Evaluator;
    assert!(!auto_resolve_eligible(&r));
    r.role = Role::Worker;
    for group in [ThreadGroup::Working, ThreadGroup::WaitingOnYou, ThreadGroup::ReadyForReview] {
        r.last_group = Some(group);
        assert!(!auto_resolve_eligible(&r), "{group:?}: 쉼만 자동 종료한다");
    }
}

fn agent(activity: AgentActivity) -> AgentStatus {
    AgentStatus {
        activity,
        since_ms: 0,
        harness: "claude".into(),
        message: None,
        percent: None,
        progress: None,
        progress_at_ms: None,
        progress_since_ms: None,
        harness_session: None,
        seen: true,
        plan: None,
        background: None,
        background_wait: false,
        compacting: false,
        context: None,
        model_observation: None,
        provider_turn: None,
    }
}

#[test]
fn sessions_are_lost_only_when_every_recorded_pane_of_two_or_more_is_gone() {
    let m = |recorded, missing| MachinePanes { recorded, missing, threads: vec![] };
    // 하나뿐인 pane이 닫힌 것은 그 thread의 전이(pane closed)다
    assert!(!m(1, 1).lost());
    assert!(!m(2, 1).lost() && !m(3, 2).lost());
    assert!(m(2, 2).lost() && m(3, 3).lost());
    assert!(!m(0, 0).lost());
}

#[test]
fn notes_and_events_follow_the_reference() {
    let q = agent(AgentActivity::Question);
    assert_eq!(note(false, Some(&q)), "pane closed");
    assert_eq!(note(true, None), "no agent");
    assert_eq!(note(true, Some(&q)), "blocked");
    let t = |to, note: &str| Transition { to, note: note.into() };
    assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "blocked")), "blocked on a prompt");
    assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "pane closed")), "pane closed");
    assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "no agent")), "agent gone");
    assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "idle")), "waiting on you");
    assert_eq!(transition_event(&t(ThreadGroup::Idle, "idle")), "idle");
    assert!(writes_item(ThreadGroup::WaitingOnYou) && writes_item(ThreadGroup::Idle));
    assert!(!writes_item(ThreadGroup::Working) && !writes_item(ThreadGroup::ReadyForReview));
    assert!(!writes_item(ThreadGroup::Resolved));
}
