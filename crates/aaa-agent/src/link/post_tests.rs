use super::*;
use aaa_common::util;
use aaa_proto::{HostWelcome, PostInput, PostType};

/// 실제 Welcome/replay 완료 사건으로 성공을 구분한다. 10초를 자거나 시계를 늘리지 않는다.
#[tokio::test]
async fn authenticated_short_connections_reset_backoff_but_rejections_preserve_it() {
    for accepted in [false, true] {
        let path = std::env::temp_dir().join(format!("aap-reconnect-{}", util::new_id()));
        let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let (stream, peer) = tokio::io::duplex(65536);
        let (state, mut ready) = tokio::sync::watch::channel(false);
        let running = tokio::spawn({
            let agent = agent.clone();
            async move {
                let mut backoff = Duration::from_secs(30);
                let result = session_state(&agent, Box::new(stream), Some(&state), &mut backoff).await;
                (result, backoff)
            }
        });
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Hello(_)));
        if accepted {
            tx.json(&LinkMsg::Welcome(HostWelcome {
                identity: None,
                proto: AGENT_LINK,
                host_id: "host".into(),
                features: vec![],
            }))
            .await
            .unwrap();
            drop(tokio::time::timeout(Duration::from_secs(3), ready.wait_for(|ready| *ready)).await.unwrap().unwrap());
        }
        tx.json(&LinkMsg::Reject(aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::Forbidden, "fixture closed")))
            .await
            .unwrap();
        let (result, backoff) = tokio::time::timeout(Duration::from_secs(3), running).await.unwrap().unwrap();
        assert!(
            result
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
        );
        assert!(!*ready.borrow());
        assert_eq!(backoff, if accepted { Duration::from_millis(250) } else { Duration::from_secs(30) });
        drop(tx);
        drop(rx);
        drop(agent);
        std::fs::remove_dir_all(path).unwrap();
    }
}
/// Hello를 읽은 뒤 Welcome을 보류한다. 그 사이 화면 감지가 기억한 idle은 연결 뒤 반드시 전달되고,
/// Hello를 읽는 host에는 이미 받은 진행 사건을 중복해서 보내지 않는다. Ping/Pong이 전달 완료 울타리다.
#[tokio::test]
async fn harness_changes_while_waiting_for_welcome_reach_the_host() {
    use aaa_proto::{AgentEvent, HarnessEvent};
    for reads_hello in [true, false] {
        let path = std::env::temp_dir().join(format!("aap-link-{}", util::new_id()));
        let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        util::ensure_private_dir(&path.join("sessions/s")).unwrap();
        let event = |kind: &str, at_ms| HarnessEvent {
            session: "s".into(),
            harness: "claude".into(),
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
        let working = event("turn_start", 1);
        let mut report = event("progress", 2);
        report.harness = "report".into();
        agent.harness_event(working.clone()).await;
        agent.harness_event(report.clone()).await;
        let (stream, peer) = tokio::io::duplex(1 << 20);
        let running = tokio::spawn({
            let agent = agent.clone();
            async move { session_state(&agent, Box::new(stream), None, &mut Duration::from_millis(250)).await }
        });
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        let LinkMsg::Hello(h) = rx.control().await.unwrap() else { panic!("hello가 없다") };
        assert_eq!(h.harness, vec![working, report.clone()]);
        // 실제 화면 감지 진입점을 마친 뒤에만 Welcome을 보낸다. 부하나 sleep에 기대지 않는다.
        let mut reply = event("turn_reply", 3);
        reply.turn_reply =
            Some(aaa_proto::TurnReply { started_ms: 1, finished_ms: 3, text: "진행할까요?".into() });
        agent.harness_event(reply.clone()).await;
        let idle = event("idle", 4);
        agent.harness_event(idle.clone()).await;
        tx.json(&LinkMsg::Welcome(HostWelcome {
            identity: None,
            proto: AGENT_LINK,
            host_id: "host".into(),
            features: if reads_hello { vec!["harness_hello.v1".into()] } else { vec![] },
        }))
        .await
        .unwrap();
        tx.json(&LinkMsg::Ping { seq: 42 }).await.unwrap();
        let mut received = vec![];
        loop {
            match tokio::time::timeout(Duration::from_secs(3), rx.control::<LinkMsg>()).await.unwrap().unwrap() {
                LinkMsg::Event(AgentEvent::Harness(ev)) => received.push(ev),
                LinkMsg::Pong { seq: 42 } => break,
                other => panic!("예상하지 못한 사건: {other:?}"),
            }
        }
        let expected = if reads_hello { vec![reply, idle] } else { vec![report, reply, idle] };
        assert_eq!(received, expected, "Hello와 Welcome 사이의 idle이 빠졌다(reads_hello={reads_hello})");
        // 연결 뒤 같은 밀리초에 바뀐 상태도 replay보다 뒤에 전달된다.
        let live = event("working", 4);
        agent.harness_event(live.clone()).await;
        assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Event(AgentEvent::Harness(live)));
        drop(tx);
        drop(rx);
        running.await.unwrap().unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

/// 작은 실제 전송 버퍼와 여러 pane의 변경분으로 replay를 멈춘 뒤 live hook을 함께 진행한다.
/// hook은 기억하지 않는 prompt_submit이라 replay 뒤 live 전송이 열리기 전에 실행되면 영원히 빠진다.
#[tokio::test]
async fn live_hook_waits_for_the_replay_to_finish() {
    use aaa_proto::{AgentEvent, HarnessEvent};
    for close_during_replay in [false, true] {
        let path = std::env::temp_dir().join(format!("aap-link-{}", util::new_id()));
        let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let (stream, peer) = tokio::io::duplex(64);
        let running = tokio::spawn({
            let agent = agent.clone();
            async move { session_state(&agent, Box::new(stream), None, &mut Duration::from_millis(250)).await }
        });
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        let LinkMsg::Hello(h) = rx.control().await.unwrap() else { panic!("Hello가 없다") };
        assert!(h.harness.is_empty());
        let event = |session: String, kind: &str| HarnessEvent {
            session,
            harness: "claude".into(),
            kind: kind.into(),
            harness_session: None,
            message: None,
            question_kind: None,
            percent: None,
            at_ms: 3,
            transcript: None,
            plan: None,
            background: None,
            context: None,
            model_observation: None,
            codex_speed: None,
            provider_turn: None,
            turn_reply: None,
        };
        // 전송 큐보다 많은 기억을 입력 fixture로 둔다. 디스크 저장은 위 회귀·실제 E2E가 맡고,
        // 이 owner는 전송 backpressure와 live hook의 순서만 본다.
        *agent.harness_last.lock().unwrap() = (0..600)
            .map(|n| {
                let id = format!("s{n}");
                (id.clone(), vec![event(id, "idle")])
            })
            .collect();
        tx.json(&LinkMsg::Welcome(HostWelcome {
            identity: None,
            proto: AGENT_LINK,
            host_id: "host".into(),
            features: vec!["harness_hello.v1".into()],
        }))
        .await
        .unwrap();
        let first = rx.control::<LinkMsg>().await.unwrap();
        assert!(matches!(first, LinkMsg::Event(AgentEvent::Harness(_))));
        if close_during_replay {
            // Ping 답도 full 큐에서 기다린다. write half만 닫고 읽기 half와 큐는 그대로 둔다.
            tx.json(&LinkMsg::Ping { seq: 99 }).await.unwrap();
            drop(tx);
            tokio::time::timeout(Duration::from_secs(3), running)
                .await
                .expect("replay 중 EOF를 읽지 못했다")
                .unwrap()
                .unwrap();
            drop(rx);
            std::fs::remove_dir_all(path).unwrap();
            continue;
        }
        let live = event("s0".into(), "prompt_submit");
        let (_, _, received) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join! {
                biased;
                // 실제 요청 owner를 replay drain보다 먼저 진행한다. NotFound 응답도 잃으면 안 된다.
                agent.clone().handle("screen".into(), aaa_proto::AgentRequest::Screen { session: "missing".into() }),
                agent.harness_event(live.clone()),
                async {
                    let mut received = vec![first];
                    while received.len() < 602 {
                        received.push(rx.control::<LinkMsg>().await.unwrap());
                    }
                    received
                }
            }
        })
        .await
        .expect("replay 도중 Screen 응답 또는 live hook이 빠졌다");
        assert!(
            received.iter().any(|m| matches!(m, LinkMsg::Res { id, result: aaa_proto::Outcome::Err(e) }
            if id == "screen" && e.code == aaa_proto::ErrorCode::NotFound)),
            "{received:?}"
        );
        assert_eq!(
            received.last(),
            Some(&LinkMsg::Event(AgentEvent::Harness(live))),
            "replay 도중 live hook이 앞질렀다"
        );
        drop(tx);
        drop(rx);
        running.await.unwrap().unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[tokio::test]
async fn post_feature_is_negotiated_and_old_host_receives_no_post() {
    let path = std::env::temp_dir().join(format!("aap-link-{}", util::new_id()));
    let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
    util::ensure_private_dir(&path.join("post/out")).unwrap();
    let input = PostInput {
        post_id: format!("{:032x}", 1),
        to: "coordinator".into(),
        post_type: PostType::Data,
        topic: "ci-red".into(),
        title: "기록".into(),
        body: "본문".into(),
        needs_user: false,
        refs: vec![],
    };
    util::write_json(
        &path.join("post/out").join(format!("{}.json", input.post_id)),
        &crate::post::Pending { session: "s".into(), input },
    )
    .unwrap();
    let (stream, peer) = tokio::io::duplex(1 << 20);
    let (state, mut observed_ready) = tokio::sync::watch::channel(false);
    let running = tokio::spawn({
        let a = agent.clone();
        async move { session_state(&a, Box::new(stream), Some(&state), &mut Duration::from_millis(250)).await }
    });
    let (r, w) = tokio::io::split(peer);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let LinkMsg::Hello(h) = rx.control().await.unwrap() else { panic!("hello가 없다") };
    assert!(h.features.iter().any(|f| f == "post.v1"));
    assert!(!*observed_ready.borrow());
    tx.json(&LinkMsg::Welcome(HostWelcome {
        identity: None,
        proto: AGENT_LINK,
        host_id: "old".into(),
        features: vec!["terminal.v1".into()],
    }))
    .await
    .unwrap();
    drop(tokio::time::timeout(Duration::from_secs(3), observed_ready.wait_for(|ready| *ready)).await.unwrap().unwrap());
    // Welcome 뒤 Post replay를 먼저 하고 Ping을 읽는다. Pong 앞에 모르는 post가 없어야 한다.
    tx.json(&LinkMsg::Ping { seq: 42 }).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), rx.control::<LinkMsg>()).await.unwrap().unwrap(),
        LinkMsg::Pong { seq: 42 }
    );
    assert_eq!(*agent.host_post.lock().unwrap(), Some(false));
    drop(tx);
    drop(rx);
    running.await.unwrap().unwrap();
    std::fs::remove_dir_all(path).unwrap();
}
