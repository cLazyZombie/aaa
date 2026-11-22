use super::*;

type ServerRx = FrameRx<tokio::io::ReadHalf<tokio::io::DuplexStream>>;

async fn read_hello() -> (tokio::task::JoinHandle<Result<FrontClient>>, FrameTx, ServerRx) {
    let (client, server) = tokio::io::duplex(4096);
    let (rx, tx) = tokio::io::split(Box::new(client) as crate::ipc::LocalStream);
    let connecting = tokio::spawn(FrontClient::hello(
        spawn_writer(tx, MAX_FRAME),
        Inbound::Local(FrameRx::new(rx, MAX_FRAME)),
        CLI_CLIENT,
        None,
    ));
    let (rx, tx) = tokio::io::split(server);
    let mut rx = FrameRx::new(rx, MAX_FRAME);
    assert!(matches!(rx.control::<FrontMsg>().await.unwrap(), FrontMsg::Hello { .. }));
    (connecting, spawn_writer(tx, MAX_FRAME), rx)
}

#[tokio::test(start_paused = true)]
async fn no_welcome_reports_first_response_and_sends_no_request() {
    let (connecting, _tx, mut rx) = read_hello().await;
    tokio::time::advance(Duration::from_secs(10)).await;
    let Err(error) = connecting.await.unwrap() else { panic!("Welcome 없이 연결됐다") };
    assert_eq!(error.to_string(), "첫 응답 시간 초과 (10000ms) · 이번 요청 미전송");
    assert!(!host_absent(&error));
    assert!(rx.next().await.unwrap().is_none());
}

#[tokio::test(start_paused = true)]
async fn request_timeout_and_lost_reply_preserve_delivery_uncertainty() {
    let (connecting, tx, mut rx) = read_hello().await;
    tx.json(&FrontMsg::Welcome {
        proto: FRONT_API,
        host_id: "test".into(),
        role: DeviceRole::Local,
        front: "f1".into(),
        warnings: vec![],
    })
    .await
    .unwrap();
    let mut client = connecting.await.unwrap().unwrap();
    let message = FrontRequest::ThreadMessage {
        project: "demo".into(),
        thread: "t-0100".into(),
        text: "lab".into(),
        mode: aaa_proto::MessageMode::Queue,
    };

    for (request, expected) in [(FrontRequest::Status, false), (message.clone(), true)] {
        let (result, ()) = tokio::join!(client.call(request), async {
            let FrontMsg::Req { id, .. } = rx.control().await.unwrap() else { panic!("요청이 없다") };
            tokio::time::advance(Duration::from_secs(2)).await;
            tx.json(&FrontMsg::Res {
                id,
                result: aaa_proto::Outcome::Err(ErrorBody::new(ErrorCode::Timeout, "agent 대기")),
            })
            .await
            .unwrap();
        });
        let error = result.unwrap().unwrap_err();
        assert_eq!(error.code, ErrorCode::Timeout);
        assert!(error.message.starts_with("요청 처리 시간 초과 (2000ms)"));
        assert!(error.message.ends_with("agent 대기"));
        assert_eq!(error.message.contains("결과 미확정"), expected);
    }
    let (result, ()) = tokio::join!(client.call(message), async move {
        assert!(matches!(rx.control::<FrontMsg>().await.unwrap(), FrontMsg::Req { .. }));
        drop(tx);
    });
    let error = result.unwrap_err().to_string();
    assert!(error.contains("요청 처리 응답 수신 실패"), "{error}");
    assert!(error.contains("결과 미확정"), "{error}");
}
