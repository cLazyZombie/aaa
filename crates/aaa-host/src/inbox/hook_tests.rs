//! 턴 도중 inbox hook 알림과 턴 끝 nudge(t-1627)

use super::*;
use aaa_common::conn::{FrameRx, spawn_writer};
use aaa_proto::{AgentRequest, AgentResponse, FrontRequest, FrontResponse, LinkMsg, Role};

struct Lab(std::path::PathBuf);
impl Drop for Lab {
    fn drop(&mut self) {
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&self.0), "시험 폴더 정리");
    }
}

fn host(idle_secs: u64, quiet_secs: u64, tick_secs: u64) -> (Arc<Host>, Lab) {
    let path = std::env::temp_dir().join(format!("aaa-inbox-hook-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = path.join("projects").to_string_lossy().into();
    cfg.projects.nudge_idle_secs = idle_secs;
    cfg.projects.nudge_quiet_secs = quiet_secs;
    cfg.projects.tick_secs = tick_secs;
    cfg.notify.web_push = false;
    let h = Host::open(cfg, path.clone()).unwrap();
    h.project_create("demo", Default::default(), "test").unwrap();
    (h, Lab(path))
}

fn harness(kind: &str, at_ms: u64) -> aaa_proto::HarnessEvent {
    aaa_proto::HarnessEvent {
        session: "coord".into(),
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
    }
}

/// coordinator pane 하나와, 입력창이 빈 화면을 보이고 입력을 기록하는 가짜 agent. 받은 입력 글은 `typed`로 온다
fn coordinator(h: &Arc<Host>) -> (tokio::sync::mpsc::UnboundedReceiver<String>, tokio::task::JoinHandle<()>) {
    coordinator_observed(h, |_, _| {})
}

fn coordinator_observed(
    h: &Arc<Host>,
    mut observe: impl FnMut(&AgentRequest, &mut String) + Send + 'static,
) -> (tokio::sync::mpsc::UnboundedReceiver<String>, tokio::task::JoinHandle<()>) {
    let (writer, reader) = tokio::io::duplex(65536);
    let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 2);
    h.agent_connected(
        &aaa_proto::AgentHello {
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
            sessions: vec![],
            identity: None,
            harness: vec![],
        },
        true,
        conn.clone(),
    );
    h.on_session(
        "m",
        aaa_proto::SessionInfo {
            responsibility: None,
            id: "coord".into(),
            name: "coordinator".into(),
            argv: vec!["codex".into()],
            cwd: Some(h.project_dir("demo").unwrap().to_string_lossy().into()),
            pid: Some(std::process::id()),
            started_at_ms: 0,
            boot_id: "boot".into(),
            cols: 80,
            rows: 24,
            state: aaa_proto::SessionState::Running,
            role: Some(Role::Coordinator),
            label: None,
            harness: Some("codex".into()),
        },
        false,
    );
    let (typed_tx, typed) = tokio::sync::mpsc::unbounded_channel();
    let host = h.clone();
    let fake = tokio::spawn(async move {
        let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
        let mut line = String::new();
        while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
            let mut entered = false;
            observe(&body, &mut line);
            let reply = match body {
                AgentRequest::Screen { .. } => {
                    let text = format!("› {line}");
                    AgentResponse::Screen(aaa_proto::ScreenText {
                        session: "coord".into(),
                        cols: 80,
                        rows: 24,
                        lines: vec![text.clone()],
                        cursor_row: 0,
                        cursor_col: 0,
                        ansi: text,
                    })
                }
                AgentRequest::Input { data, .. } => {
                    if data.0 == b"\r" {
                        entered = true;
                    } else {
                        line.push_str(&String::from_utf8_lossy(&data.0));
                    }
                    AgentResponse::Done
                }
                other => panic!("예상하지 못한 요청: {other:?}"),
            };
            if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                tx.send(aaa_proto::Outcome::Ok(reply)).unwrap();
            }
            if entered {
                // 받은 harness는 일을 시작한다: 입력창이 비고 작업 중이 된다
                typed_tx.send(std::mem::take(&mut line)).unwrap();
                host.on_harness("m", harness("working", aaa_common::util::now_ms()));
            }
        }
    });
    (typed, fake)
}

async fn notice(h: &Arc<Host>, actor: &str, pane: &str) -> Result<Option<String>, ErrorBody> {
    let request = FrontRequest::InboxHookNotice { project: "demo".into(), pane: pane.into() };
    h.call(DeviceRole::Local, actor, request).await.map(|r| match r {
        FrontResponse::InboxHookNotice { text } => text,
        other => panic!("응답 종류: {other:?}"),
    })
}

// 턴 도중 hook이 항목을 한 번씩만 알리고, 턴 끝 nudge가 hook이 알린 묶음을 다시 보내지 않으며, 새 항목은 여전히 보낸다.
// thread·다른 pane은 coordinator 알림을 가로채지 못한다(가로채면 coordinator가 못 본 채 알린 것으로 적힌다).
#[tokio::test]
async fn hook_tells_each_new_item_once_and_turn_end_nudge_skips_what_hook_told() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let (h, _lab) = host(0, 0, 60);
        let (mut typed, fake) = coordinator(&h);
        h.on_harness("m", harness("working", aaa_common::util::now_ms()));
        h.inbox_add("demo", "thread-state", "t-0001", "new report", "보고");

        for actor in ["worker:coord", "detached:cli", "local:cli@p-other"] {
            let refused = notice(&h, actor, "coord").await.unwrap_err();
            assert_eq!(refused.code, ErrorCode::Forbidden, "{actor}");
        }
        assert_eq!(notice(&h, "local:aaa-agent", "p-other").await.unwrap(), None, "coordinator pane이 아니면 빈 답");

        let first = notice(&h, "local:aaa-agent", "coord").await.unwrap().expect("처음 보는 항목을 알린다");
        assert!(first.starts_with("[aaa inbox] t-0001 new report"), "{first}");
        assert_eq!(notice(&h, "local:aaa-agent", "coord").await.unwrap(), None, "같은 항목은 다시 알리지 않는다");

        h.inbox_add("demo", "thread-state", "t-0002", "new report", "보고");
        let second = notice(&h, "local:cli@coord", "coord").await.unwrap().expect("coordinator pane 자신도 묻는다");
        assert!(second.contains("t-0002") && !second.contains("t-0001"), "새 항목만: {second}");

        // 턴 끝: hook이 알린 묶음은 nudge가 보내지 않는다
        h.on_harness("m", harness("idle", aaa_common::util::now_ms()));
        let dir = h.project_dir("demo").unwrap();
        for _ in 0..2 {
            h.nudge("demo", &dir).await;
        }
        assert!(typed.try_recv().is_err(), "hook이 알린 묶음을 턴 끝에 다시 보냈다");

        // 마지막 도구 뒤 도착한 항목은 턴 끝 nudge가 보낸다
        h.inbox_add("demo", "thread-state", "t-0003", "new report", "보고");
        for _ in 0..2 {
            h.nudge("demo", &dir).await;
        }
        let sent = typed.recv().await.unwrap();
        assert!(sent.starts_with("[aaa inbox]") && sent.contains("t-0003"), "{sent}");
        assert!(!sent.contains("t-0001") && !sent.contains("t-0002"), "이미 hook으로 알렸다: {sent}");
        assert_eq!(
            notice(&h, "local:aaa-agent", "coord").await.unwrap(),
            None,
            "nudge가 보낸 항목은 hook이 다시 알리지 않는다"
        );
        fake.abort();
    })
    .await
    .expect("hook·nudge 알림은 5초 안에 끝나야 한다");
}

// 턴이 끝나면 15초(여기서는 60초) 주기 tick을 기다리지 않고 쉼·입력창 확인 시간만 지나 알림이 간다.
#[tokio::test]
async fn turn_end_nudge_wakes_the_ticker_instead_of_waiting_for_the_period() {
    let (h, _lab) = host(1, 1, 60);
    let (mut typed, fake) = coordinator(&h);
    h.start_ticker();
    h.inbox_add("demo", "thread-state", "t-0001", "new report", "보고");
    let ended = Instant::now();
    h.on_harness("m", harness("idle", aaa_common::util::now_ms()));
    let sent = tokio::time::timeout(Duration::from_secs(5), typed.recv())
        .await
        .expect("턴 끝 알림이 주기 tick(60초) 전에 가야 한다")
        .unwrap();
    assert!(sent.contains("t-0001"), "{sent}");
    // 쉼 1초 뒤 입력창 확인 1초를 차례로 지난다. 어느 한 창이 빠지면 2초가 되지 않는다(ms 절사 여유 0.1초).
    assert!(ended.elapsed() >= Duration::from_millis(1_900), "쉼·입력창 확인을 건너뛰었다: {:?}", ended.elapsed());
    fake.abort();
}

// 오래 걸리는 보고서 작업이 ticker를 잡은 동안에도 답 알림의 두 빈 창 관측과 제출은 진행한다.
#[test]
fn approval_arrives_while_ticker_waits_on_library_work() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(3).enable_all().build().unwrap();
    let lab = rt.block_on(async {
        let (h, lab) = host(0, 0, 60);
        let (mut typed, fake) = coordinator(&h);
        h.on_harness("m", harness("idle", aaa_common::util::now_ms()));
        let (locked, ready) = tokio::sync::oneshot::channel();
        let (release, held) = std::sync::mpsc::channel::<()>();
        let holder = h.clone();
        let library = tokio::task::spawn_blocking(move || {
            let _guard = holder.library_ops.lock().unwrap();
            locked.send(()).unwrap();
            let _ = held.recv();
        });
        ready.await.unwrap();
        h.start_ticker();
        h.inbox_add("demo", "thread-state", "coordinator", "question answered", "답");
        let result = tokio::time::timeout(Duration::from_secs(3), async {
            while h.tick_started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
            typed.recv().await.unwrap()
        })
        .await;
        assert_eq!(*h.tick_done.borrow(), 0);
        drop(release);
        library.await.unwrap();
        assert!(result.expect("보고서 잠금을 풀기 전에 승인 알림이 도착한다").contains("question answered"));
        fake.abort();
        lab
    });
    drop(rt);
    drop(lab);
}

// 두 번째 빈 창 검사 뒤 사람이 입력했다. 붙여넣기 직전 화면에 남은 글과 Enter가 보존된다.
#[tokio::test]
async fn a_draft_after_the_quiet_check_keeps_the_answer_pending() {
    let (h, _lab) = host(0, 0, 60);
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = reads.clone();
    let (mut typed, fake) = coordinator_observed(&h, move |req, line| {
        if matches!(req, AgentRequest::Screen { .. }) && seen.fetch_add(1, Ordering::SeqCst) == 2 {
            *line = "작성 중".into();
        }
        assert!(!matches!(req, AgentRequest::Input { .. }), "작성 중인 창에 입력을 보냈다");
    });
    h.on_harness("m", harness("idle", aaa_common::util::now_ms()));
    h.inbox_add("demo", "thread-state", "coordinator", "question answered", "답");
    let dir = h.project_dir("demo").unwrap();
    h.nudge("demo", &dir).await;
    h.nudge("demo", &dir).await;
    assert_eq!(reads.load(Ordering::SeqCst), 3);
    assert!(typed.try_recv().is_err());
    assert_eq!(h.screen_lines("m", "coord").await.unwrap(), ["› 작성 중"]);
    assert!(notice(&h, "local:aaa-agent", "coord").await.unwrap().unwrap().contains("question answered"));
    fake.abort();
}

#[tokio::test]
async fn hook_during_pane_submission_does_not_repeat_the_answer() {
    let (h, _lab) = host(0, 0, 60);
    let hook_host = h.clone();
    let (hooked, mut results) = tokio::sync::mpsc::unbounded_channel();
    let (mut typed, fake) = coordinator_observed(&h, move |request, _| {
        if matches!(request, AgentRequest::Input { data, .. } if data.0 != b"\r") {
            hooked.send(hook_host.inbox_hook_notice("demo", "coord", "local:aaa-agent").unwrap()).unwrap();
        }
    });
    h.on_harness("m", harness("idle", aaa_common::util::now_ms()));
    h.inbox_add("demo", "thread-state", "coordinator", "question answered", "답");
    let dir = h.project_dir("demo").unwrap();
    h.nudge("demo", &dir).await;
    h.nudge("demo", &dir).await;
    assert_eq!(results.try_recv().unwrap(), None);
    assert!(typed.try_recv().unwrap().contains("question answered"));
    assert_eq!(notice(&h, "local:aaa-agent", "coord").await.unwrap(), None);
    assert!(typed.try_recv().is_err());
    fake.abort();
}

#[tokio::test]
async fn other_pane_and_approval_proceed_while_a_local_cwd_is_resolved() {
    let (h, _lab) = host(0, 0, 60);
    let (mut typed, fake) = coordinator(&h);
    let mut remote = h.inner.lock().unwrap().panes["coord"].info.clone();
    remote.id = "remote".into();
    h.on_session("another-machine", remote, false);
    h.on_harness("m", harness("idle", aaa_common::util::now_ms()));
    h.inbox_add("demo", "thread-state", "coordinator", "question answered", "답");
    let (entered, mut resolving) = tokio::sync::mpsc::unbounded_channel();
    let (release, held) = std::sync::mpsc::channel::<()>();
    let reader = h.clone();
    let list = tokio::task::spawn_blocking(move || {
        reader.pane_views_resolving(DeviceRole::Local, |cwd| {
            entered.send(()).unwrap();
            let _ = held.recv();
            std::fs::canonicalize(cwd)
        })
    });
    resolving.recv().await.unwrap();
    assert!(h.inner.try_lock().is_ok(), "경로 정규화가 host 상태 잠금을 잡았다");
    assert!(h.attach_pane("remote").is_some());
    assert!(h.pane_view_of("remote", DeviceRole::Local).unwrap().coordinator_project.is_none());
    let dir = h.project_dir("demo").unwrap();
    h.nudge("demo", &dir).await;
    h.nudge("demo", &dir).await;
    assert!(typed.try_recv().unwrap().contains("question answered"));
    drop(release);
    let panes = list.await.unwrap();
    assert_eq!(panes.iter().find(|p| p.id == "coord").unwrap().coordinator_project.as_deref(), Some("demo"));
    fake.abort();
}

#[tokio::test]
async fn user_input_after_nudge_paste_prevents_enter_and_is_preserved() {
    let (h, _lab) = host(0, 0, 60);
    let (pasted, mut events) = tokio::sync::mpsc::unbounded_channel();
    let (mut typed, fake) = coordinator_observed(&h, move |req, _| {
        if matches!(req, AgentRequest::Input { data, .. } if data.0 != b"\r") {
            pasted.send(()).unwrap();
        }
    });
    h.on_harness("m", harness("idle", aaa_common::util::now_ms()));
    h.inbox_add("demo", "thread-state", "coordinator", "question answered", "답");
    let dir = h.project_dir("demo").unwrap();
    h.nudge("demo", &dir).await;
    let send = h.nudge("demo", &dir);
    tokio::pin!(send);
    tokio::select! {
        biased;
        Some(()) = events.recv() => {}
        _ = &mut send => panic!("붙여넣기 사건보다 제출이 먼저 끝났다"),
    }
    h.live_conn("m")
        .unwrap()
        .request(AgentRequest::Input {
            session: "coord".into(), data: aaa_proto::B64("내 초안".as_bytes().to_vec())
        })
        .await
        .unwrap();
    send.await;
    assert!(typed.try_recv().is_err(), "사용자가 입력한 뒤 Enter가 갔다");
    assert!(h.screen_lines("m", "coord").await.unwrap()[0].ends_with("내 초안"));
    assert_eq!(notice(&h, "local:aaa-agent", "coord").await.unwrap(), None);
    fake.abort();
}

#[tokio::test]
async fn a_full_agent_queue_does_not_defer_a_guarded_enter_past_user_input() {
    let (tx, _reader) = aaa_common::conn::frame_channel();
    while tx.try_json(&"채운 frame").unwrap() {}
    let conn = crate::AgentConn::new(tx, 1);
    let watch = conn.watch_input("coord", true);
    let enter = AgentRequest::Input { session: "coord".into(), data: aaa_proto::B64(vec![13]) };
    assert!(conn.request_watched(enter, Duration::from_secs(1), Some(&watch), true).await.unwrap().is_none());
    assert!(conn.pending.lock().unwrap().is_empty());
}
