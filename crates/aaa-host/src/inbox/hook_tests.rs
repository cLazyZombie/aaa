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
        provider_turn: None,
        turn_reply: None,
    }
}

/// coordinator pane 하나와, 입력창이 빈 화면을 보이고 입력을 기록하는 가짜 agent. 받은 입력 글은 `typed`로 온다
fn coordinator(h: &Arc<Host>) -> (tokio::sync::mpsc::UnboundedReceiver<String>, tokio::task::JoinHandle<()>) {
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
    assert!(ended.elapsed() >= Duration::from_secs(1), "쉼 대기를 건너뛰었다: {:?}", ended.elapsed());
    fake.abort();
}
