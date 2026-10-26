//! 재개한 session pump가 Welcome 직후 구독 응답·종료 출력을 잃지 않는지 검사한다.

use super::*;
use aaa_proto::{AgentRequest, AgentResponse, HostWelcome, Outcome, SessionMsg, SessionRequest, TerminalChunk};

/// 실제 Welcome 협상이 handler의 큰 보고 응답을 분할하고, 구판에는 오류 뒤 다음 요청도 받아야 한다.
#[tokio::test]
async fn large_report_handler_negotiates_parts_and_keeps_legacy_links() {
    use aaa_common::response_parts::{FEATURE, ResponseParts};
    use aaa_proto::{B64, ErrorCode};
    use sha2::{Digest, Sha256};
    let root = std::env::temp_dir().join(format!("aaa-large-report-{}", aaa_common::util::new_id()));
    let work = root.join(".aaa/p-t-0001");
    std::fs::create_dir_all(&work).unwrap();
    let body = vec![b'x'; 2 << 20];
    std::fs::write(work.join("report.md"), &body).unwrap();
    let expected_hash = hex::encode(Sha256::digest(&body));
    for negotiated in [true, false] {
        let agent =
            Agent::new(root.join(if negotiated { "new" } else { "old" }), "m".into(), "mac-a".into(), true, None)
                .unwrap();
        let (stream, peer) = tokio::io::duplex(65536);
        let running = tokio::spawn(async move {
            session_state(&agent, Box::new(stream), None, &mut Duration::from_millis(250)).await
        });
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        let LinkMsg::Hello(hello) = rx.control().await.unwrap() else { panic!("hello가 없다") };
        assert!(hello.features.iter().any(|f| f == FEATURE));
        assert!(hello.features.iter().any(|f| f == "report_hash.v1"));
        tx.json(&LinkMsg::Welcome(HostWelcome {
            identity: None,
            proto: AGENT_LINK,
            host_id: "host".into(),
            features: if negotiated { vec![FEATURE.into()] } else { vec![] },
        }))
        .await
        .unwrap();
        let mut parts = ResponseParts::default();
        for (id, request) in [
            ("hash", AgentRequest::ReportHash { work_dir: work.display().to_string() }),
            ("large", AgentRequest::ReportTransfer { work_dir: work.display().to_string(), max_library_bytes: 0 }),
            ("after", AgentRequest::ReportHash { work_dir: work.display().to_string() }),
        ] {
            tx.json(&LinkMsg::Req { id: id.into(), deadline_ms: None, body: request }).await.unwrap();
            let response = loop {
                match rx.control::<LinkMsg>().await.unwrap() {
                    LinkMsg::ResPart { id, offset, total, data } => {
                        assert!(negotiated);
                        if let Some(response) = parts.push(&id, offset, total, data.0).unwrap() {
                            break response;
                        }
                    }
                    response => break response,
                }
            };
            let LinkMsg::Res { id: got, result } = response else { panic!("응답이 아니다") };
            assert_eq!(got, id);
            if id == "large" {
                if negotiated {
                    let Outcome::Ok(AgentResponse::ReportTransfer(snapshot)) = result else {
                        panic!("보고 사본이 아니다")
                    };
                    assert_eq!(snapshot.report, B64(body.clone()));
                    assert_eq!(snapshot.hash, expected_hash);
                } else {
                    assert!(matches!(result, Outcome::Err(e) if e.code == ErrorCode::TooLarge));
                }
            } else {
                assert_eq!(result, Outcome::Ok(AgentResponse::ReportHash { hash: expected_hash.clone() }));
            }
        }
        tx.json(&LinkMsg::Ping { seq: 1131 }).await.unwrap();
        assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 1131 });
        drop(tx);
        drop(rx);
        running.await.unwrap().unwrap();
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn requests_read_before_host_publication_keep_session_replies_and_exit_output() {
    let path = std::env::temp_dir().join(format!("aap-resume-{}", aaa_common::util::new_id()));
    let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
    let mut tasks = Vec::new();
    for session in ["s1", "s2"] {
        let (mut spec, mut info) = crate::tests::session(None, &["/bin/sh"], aaa_proto::SessionState::Running);
        spec.id = session.into();
        info.id = session.into();
        info.pid = None;
        let paths = aaa_session::files::SessionPaths::new(&path, session);
        let (stream, peer) = tokio::io::duplex(1 << 16);
        let stream: LocalStream = Box::new(stream);
        let (r, w) = tokio::io::split(stream);
        let (cmd, commands) = tokio::sync::mpsc::channel(16);
        agent.sessions.lock().unwrap().insert(session.into(), crate::Entry { info, pump: Some(cmd) });
        tasks.push(tokio::spawn(agent.clone().pump(
            paths,
            spec,
            spawn_writer(w, MAX_FRAME),
            FrameRx::new(r, MAX_FRAME),
            commands,
            crate::Detecting { watch: None, front: None, detect_ipc: false, colors_ipc: false },
        )));
        tasks.push(tokio::spawn(async move {
            let (r, w) = tokio::io::split(peer);
            let mut rx = FrameRx::new(r, MAX_FRAME);
            let tx = spawn_writer(w, MAX_FRAME);
            while let Ok(SessionMsg::Req { id, body }) = rx.control::<SessionMsg>().await {
                let result = match body {
                    SessionRequest::Subscribe { .. } => {
                        AgentResponse::Subscribed { session: session.into(), snapshot: None, offset: 0 }
                    }
                    SessionRequest::Input { data } => {
                        assert_eq!(data.0, b"quit\r");
                        AgentResponse::Done
                    }
                    other => panic!("예상하지 못한 session 요청: {other:?}"),
                };
                let quitting = matches!(result, AgentResponse::Done);
                tx.json(&SessionMsg::Res { id, result: Outcome::Ok(result) }).await.unwrap();
                if quitting {
                    tx.terminal(&TerminalChunk {
                        session: session.into(),
                        offset: 0,
                        data: b"foreground-harness-exited\r\n".to_vec(),
                    })
                    .await
                    .unwrap();
                }
            }
        }));
    }
    // 복구한 pump의 감지·hook 저장이 replay 울타리를 잡은 사건을 고정한다.
    // 시간을 흘리거나 부하에 기대지 않고 Welcome과 두 구독을 그 안에 넣는다.
    let delivery = agent.harness_delivery.lock().await;
    let (stream, peer) = tokio::io::duplex(1 << 16);
    let (ready, mut initialized) = tokio::sync::watch::channel(false);
    let running = tokio::spawn({
        let agent = agent.clone();
        async move { session_state(&agent, Box::new(stream), Some(&ready), &mut Duration::from_millis(250)).await }
    });
    let (r, w) = tokio::io::split(peer);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let tx = spawn_writer(w, MAX_FRAME);
    assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Hello(_)));
    tx.json(&LinkMsg::Welcome(HostWelcome {
        identity: None,
        proto: AGENT_LINK,
        host_id: "host".into(),
        features: vec!["harness_hello.v1".into()],
    }))
    .await
    .unwrap();
    for session in ["s1", "s2"] {
        tx.json(&LinkMsg::Req {
            id: format!("sub-{session}"),
            deadline_ms: None,
            body: AgentRequest::Subscribe { session: session.into(), from: Some(0) },
        })
        .await
        .unwrap();
    }
    // Pong은 잠금을 놓기 전에 reader가 Welcome·두 요청을 모두 읽은 울타리다.
    // 요청은 등록을 기다리되 EOF·Ping 처리는 계속되어야 한다.
    tx.json(&LinkMsg::Ping { seq: 42 }).await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), rx.control::<LinkMsg>()).await.unwrap().unwrap(),
        LinkMsg::Pong { seq: 42 }
    );
    assert!(!*initialized.borrow());
    drop(delivery);
    tokio::time::timeout(Duration::from_secs(3), async {
        let mut subscribed = std::collections::HashSet::new();
        while subscribed.len() < 2 {
            let LinkMsg::Res { id, result: Outcome::Ok(AgentResponse::Subscribed { session, .. }) } =
                rx.control::<LinkMsg>().await.unwrap()
            else {
                panic!("구독 응답이 아니다")
            };
            assert_eq!(id, format!("sub-{session}"));
            assert!(subscribed.insert(session));
        }
        tx.json(&LinkMsg::Req {
            id: "quit".into(),
            deadline_ms: None,
            body: AgentRequest::Input { session: "s1".into(), data: aaa_proto::B64(b"quit\r".to_vec()) },
        })
        .await
        .unwrap();
        assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Res { id, result: Outcome::Ok(AgentResponse::Done) } if id == "quit"));
        let Some(Incoming::Terminal(chunk)) = rx.next().await.unwrap() else { panic!("종료 출력이 없다") };
        assert_eq!(chunk.session, "s1");
        assert_eq!(chunk.data, b"foreground-harness-exited\r\n");
    })
    .await
    .expect("host 등록 전에 읽은 요청의 session 응답·종료 출력을 잃었다");
    drop(tokio::time::timeout(Duration::from_secs(3), initialized.wait_for(|ready| *ready)).await.unwrap().unwrap());
    drop(tx);
    drop(rx);
    tokio::time::timeout(Duration::from_secs(3), running).await.unwrap().unwrap().unwrap();
    for task in tasks {
        task.abort();
        let _ = task.await;
    }
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn eof_or_deadline_before_host_publication_does_not_start_mutating_requests() {
    for eof in [true, false] {
        let path = std::env::temp_dir().join(format!("aap-resume-end-{}", aaa_common::util::new_id()));
        let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let (spec, info) = crate::tests::session(None, &["/bin/sh"], aaa_proto::SessionState::Running);
        let paths = aaa_session::files::SessionPaths::new(&path, "s");
        aaa_common::util::ensure_private_dir(&paths.dir).unwrap();
        aaa_common::util::write_json(&paths.spec(), &spec).unwrap();
        agent.sessions.lock().unwrap().insert("s".into(), crate::Entry { info, pump: None });
        let delivery = agent.harness_delivery.lock().await;
        let (stream, peer) = tokio::io::duplex(1 << 16);
        let running = tokio::spawn({
            let agent = agent.clone();
            async move { session_state(&agent, Box::new(stream), None, &mut Duration::from_millis(250)).await }
        });
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Hello(_)));
        tx.json(&LinkMsg::Welcome(HostWelcome {
            identity: None,
            proto: AGENT_LINK,
            host_id: "host".into(),
            features: vec!["harness_hello.v1".into()],
        }))
        .await
        .unwrap();
        tx.json(&LinkMsg::Req {
            id: "rename".into(),
            deadline_ms: (!eof).then_some(1),
            body: AgentRequest::Rename { session: "s".into(), label: Some("changed".into()) },
        })
        .await
        .unwrap();
        tx.json(&LinkMsg::Ping { seq: 42 }).await.unwrap();
        // 요청 자체의 만료 사건과 Pong을 기다린다. 정한 시간을 자는 준비는 없다.
        tokio::time::timeout(Duration::from_secs(3), async {
            let (mut pong, mut expired) = (false, false);
            while !pong || (!eof && !expired) {
                match rx.control::<LinkMsg>().await.unwrap() {
                    LinkMsg::Pong { seq: 42 } => pong = true,
                    LinkMsg::Res { id, result: Outcome::Err(error) } => {
                        assert!(!eof);
                        assert_eq!(id, "rename");
                        assert_eq!(error.code, aaa_proto::ErrorCode::Timeout);
                        expired = true;
                    }
                    other => panic!("등록 전 요청을 실행했다: {other:?}"),
                }
            }
        })
        .await
        .unwrap();
        drop(tx);
        tokio::time::timeout(Duration::from_secs(3), running).await.unwrap().unwrap().unwrap();
        drop(rx);
        drop(delivery);
        assert!(agent.labels.lock().unwrap().is_empty());
        let stored: aaa_session::files::SpecFile = aaa_common::util::read_json(&paths.spec()).unwrap();
        assert_eq!(stored.label, None);
        std::fs::remove_dir_all(path).unwrap();
    }
}
