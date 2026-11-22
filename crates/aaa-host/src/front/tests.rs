use super::*;

#[tokio::test]
async fn old_thread_records_keep_bounding_the_cli_role_after_index_creation() {
    use crate::orch::ThreadRec;
    use aaa_proto::{ThreadKind, ThreadStatus};

    let root = std::env::temp_dir().join(format!("aaa-front-pane-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").display().to_string();
    let state = root.join("host");
    {
        let h = Host::open(cfg.clone(), state.clone()).unwrap();
        h.db.conn_for_test().execute("DROP INDEX threads_pane", []).unwrap();
        assert!(h.db.thread_import("demo", "t-0001", "open", "{").unwrap());
        for (number, pane, status, role) in [
            (2, "claimed-pane", ThreadStatus::Resolved, aaa_proto::Role::Worker),
            (3, "another-pane", ThreadStatus::Open, aaa_proto::Role::Worker),
            (4, "claimed-pane", ThreadStatus::Open, aaa_proto::Role::Evaluator),
            (5, "claimed-pane", ThreadStatus::Open, aaa_proto::Role::Worker),
        ] {
            let mut row = ThreadRec::starting("demo", "lab", "mac-a", "demo", role, ThreadKind::default(), 0);
            row.id = format!("t-{number:04}");
            row.pane = Some(pane.into());
            row.status = status;
            assert!(
                h.db.thread_import("demo", &row.id, status.as_str(), &serde_json::to_string(&row).unwrap()).unwrap()
            );
        }
    }
    let h = Host::open(cfg, state).unwrap();
    let rec = h.thread_of_pane("claimed-pane").unwrap();
    assert_eq!(rec.id, "t-0004");
    assert!(h.thread_of_pane("missing-pane").is_none());
    let (client, server) = tokio::io::duplex(MAX_FRAME);
    let (cr, cw) = tokio::io::split(client);
    let tx = spawn_writer(cw, MAX_FRAME);
    let mut rx = FrameRx::new(cr, MAX_FRAME);
    let serving = tokio::spawn(serve(h.clone(), Box::new(server), Some(std::process::id()), Instant::now()));
    tx.json(&FrontMsg::Hello { proto: FRONT_API, client: "cli".into(), session: Some("claimed-pane".into()) })
        .await
        .unwrap();
    let hello = tokio::time::timeout(Duration::from_secs(2), rx.control::<FrontMsg>()).await.unwrap().unwrap();
    assert!(matches!(hello, FrontMsg::Welcome { role: DeviceRole::Reader, .. }));
    drop((tx, rx));
    serving.await.unwrap().unwrap();
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn local_wire_excludes_web_only_rollout_notices() {
    use aaa_proto::{NoticeKind, NoticeView};
    let root = std::env::temp_dir().join(format!("aaa-local-notice-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    let (client, server) = tokio::io::duplex(MAX_FRAME);
    let (sr, sw) = tokio::io::split(server);
    let front = h.new_front("cli".into());
    let task = tokio::spawn(serve_conn(
        h.clone(),
        spawn_writer(sw, MAX_FRAME),
        FrameRx::new(sr, MAX_FRAME),
        Conn { role: DeviceRole::Local, actor: "local:cli".into(), front, proto: FRONT_API },
    ));
    let mut rx = FrameRx::new(client, MAX_FRAME);
    assert!(matches!(rx.control::<FrontMsg>().await.unwrap(), FrontMsg::Event(FrontEvent::State { .. })));
    for kind in [NoticeKind::RolloutFinished, NoticeKind::RolloutFailed, NoticeKind::Finished] {
        h.send_notice(NoticeView {
            kind,
            pane: None,
            machine: String::new(),
            project: None,
            thread: None,
            title: "wire barrier".into(),
            agent: None,
            turn_ms: None,
            long: true,
            at_ms: 1,
        });
    }
    let msg = tokio::time::timeout(Duration::from_secs(2), rx.control::<FrontMsg>()).await.unwrap().unwrap();
    assert!(
        matches!(msg, FrontMsg::Event(FrontEvent::Notice(NoticeView { kind: NoticeKind::Finished, .. }))),
        "N-1 CLI가 모르는 웹 알림을 받았다: {msg:?}"
    );
    drop(rx);
    task.await.unwrap().unwrap();
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn thread_pages_require_a_client_that_reads_cursors() {
    let root = std::env::temp_dir().join(format!("aaa-thread-wire-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").display().to_string();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    let (client, server) = tokio::io::duplex(MAX_FRAME);
    let (sr, sw) = tokio::io::split(server);
    let (cr, cw) = tokio::io::split(client);
    let task = tokio::spawn(serve_conn(
        h.clone(),
        spawn_writer(sw, MAX_FRAME),
        FrameRx::new(sr, MAX_FRAME),
        Conn {
            role: DeviceRole::Local,
            actor: "local:cli".into(),
            front: h.new_front("cli".into()),
            proto: aaa_proto::Version { major: 1, minor: 127 },
        },
    ));
    let tx = spawn_writer(cw, MAX_FRAME);
    tx.json(&FrontMsg::Req {
        id: "list".into(),
        body: FrontRequest::Threads { project: None, role: None, query: Default::default() },
    })
    .await
    .unwrap();
    let mut rx = FrameRx::new(cr, MAX_FRAME);
    loop {
        let msg = tokio::time::timeout(Duration::from_secs(2), rx.control::<FrontMsg>()).await.unwrap().unwrap();
        if let FrontMsg::Res { result, .. } = msg {
            assert_eq!(result.into_result().unwrap_err().code, ErrorCode::UnsupportedVersion);
            break;
        }
    }
    drop(rx);
    drop(tx);
    task.await.unwrap().unwrap();
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

/// 구판 CLI가 B/A 원문을 위치의 A/B로 오해해 다른 답을 보내지 않게 한다.
#[tokio::test]
async fn legacy_cli_answers_require_source_letters_only_for_noncanonical_cards() {
    let root = std::env::temp_dir().join(format!("aaa-question-wire-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").display().to_string();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    h.project_create("p", Default::default(), "test").unwrap();
    for (id, choices, code) in [
        ("t-0001", "- B 새 안\n- A 원래\n", ErrorCode::UnsupportedVersion),
        ("t-0002", "- A 원래\n- B 새 안\n", ErrorCode::BadRequest),
    ] {
        let mut rec = crate::orch::ThreadRec::starting(
            "p",
            "질문",
            "m",
            "fake",
            aaa_proto::Role::Worker,
            aaa_proto::ThreadKind::Dir,
            0,
        );
        rec.id = id.into();
        rec.status = aaa_proto::ThreadStatus::Open;
        rec.brief = aaa_proto::BriefState::Delivered;
        rec.report_hash = Some("v".into());
        rec.questions = aaa_proto::report_questions(&format!("## Questions\n### 1. 진행\n{choices}"));
        h.db.thread_import("p", id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
        let (client, server) = tokio::io::duplex(MAX_FRAME);
        let (sr, sw) = tokio::io::split(server);
        let (cr, cw) = tokio::io::split(client);
        let task = tokio::spawn(serve_conn(
            h.clone(),
            spawn_writer(sw, MAX_FRAME),
            FrameRx::new(sr, MAX_FRAME),
            Conn {
                role: DeviceRole::Local,
                actor: "local:cli".into(),
                front: h.new_front("cli".into()),
                proto: aaa_proto::Version { major: 1, minor: 131 },
            },
        ));
        let tx = spawn_writer(cw, MAX_FRAME);
        tx.json(&FrontMsg::Req {
            id: "answer".into(),
            body: FrontRequest::ThreadAnswer {
                project: "p".into(),
                thread: id.into(),
                report_hash: "v".into(),
                picks: vec![Some(0)],
                text: None,
                comments: Vec::new(),
            },
        })
        .await
        .unwrap();
        let mut rx = FrameRx::new(cr, MAX_FRAME);
        loop {
            if let FrontMsg::Res { result, .. } =
                tokio::time::timeout(Duration::from_secs(2), rx.control::<FrontMsg>()).await.unwrap().unwrap()
            {
                let error = result.into_result().unwrap_err();
                assert_eq!(error.code, code);
                assert!(error.message.contains(if code == ErrorCode::UnsupportedVersion {
                    "1.132"
                } else {
                    "thread 세션이 없다"
                }));
                break;
            }
        }
        drop(rx);
        drop(tx);
        task.await.unwrap().unwrap();
        assert!(h.load("p", id).unwrap().question_answer.is_none());
    }
    drop(h);
    std::fs::remove_dir_all(root).unwrap();
}

/// 같은 pane의 쌓인 입력은 손실 없이 한 번에 전송하고, resize·다른 pane을 넘어서 묶지 않는다.
#[tokio::test]
async fn queued_input_batches_preserve_bytes_boundaries_and_receipts() {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let input =
        |pane: &str, data: &[u8]| FrontRequest::PaneInput { pane: pane.into(), data: aaa_proto::B64(data.to_vec()) };
    for (id, req) in [
        ("press", input("p", b"\x1b[<0;1;1M")),
        ("drag", input("p", b"\x1b[<32;8;2M")),
        ("up", input("p", b"\x1b[<0;8;2m")),
        ("resize", FrontRequest::PaneResize { pane: "p".into(), cols: 90, rows: 30 }),
        ("key", input("p", b"x")),
        ("other", input("q", b"y")),
        ("full", input("q", &vec![b'z'; 64 * 1024])),
        ("tail", input("q", b"!")),
    ] {
        tx.send((id.into(), req)).unwrap();
    }
    drop(tx);
    let mut ordered = OrderedInput::new(rx);
    let (ids, req) = ordered.next().await.unwrap();
    assert_eq!(ids, ["press", "drag", "up"]);
    assert_eq!(req, input("p", b"\x1b[<0;1;1M\x1b[<32;8;2M\x1b[<0;8;2m"));
    let (ids, req) = ordered.next().await.unwrap();
    assert_eq!(ids, ["resize"]);
    assert!(matches!(req, FrontRequest::PaneResize { .. }));
    for (id, pane, text) in [
        ("key", "p", b"x".as_slice()),
        ("other", "q", b"y"),
        ("full", "q", &vec![b'z'; 64 * 1024]),
        ("tail", "q", b"!"),
    ] {
        assert_eq!(ordered.next().await, Some((vec![id.into()], input(pane, text))));
    }
    assert!(ordered.next().await.is_none());

    // 빈 입력도 유효한 요청이다. byte 상한만으로는 그 입력을 끝없이 모을 수 있으므로 요청 수도 묶는다.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    for n in 0..300 {
        tx.send((n.to_string(), input("p", b""))).unwrap();
    }
    drop(tx);
    let mut ordered = OrderedInput::new(rx);
    let (first, req) = ordered.next().await.unwrap();
    assert_eq!(first.len(), 256);
    assert_eq!(req, input("p", b""));
    let (last, req) = ordered.next().await.unwrap();
    assert_eq!(last.len(), 44);
    assert_eq!(first.into_iter().chain(last).collect::<Vec<_>>(), (0..300).map(|n| n.to_string()).collect::<Vec<_>>());
    assert_eq!(req, input("p", b""));
    assert!(ordered.next().await.is_none());
}

/// 없앤 요청(1.12의 `decision_add`)·모르는 요청에는 그 id로 오류를 답하고, id도 없는 것만 끊는다(t-0340 보안 감사)
#[test]
fn unknown_requests_get_an_error_answer() {
    let known = br#"{"t":"req","id":"1","body":{"op":"projects"}}"#;
    assert!(
        matches!(read_req(known), Ok(ReqFrame::Req(id, body)) if id == "1" && matches!(*body, FrontRequest::Projects { all: false }))
    );
    let removed = br#"{"t":"req","id":"2","body":{"op":"decision_add","project":"p","text":"x"}}"#;
    match read_req(removed) {
        Ok(ReqFrame::Unknown(id, e)) => {
            assert_eq!((id.as_str(), e.code), ("2", ErrorCode::BadRequest));
            assert!(
                e.message.starts_with(&format!("이 host(FRONT_API {FRONT_API})가 모르는 요청이다")),
                "{}",
                e.message
            );
        }
        _ => panic!("모르는 요청에 답하지 않는다"),
    }
    assert!(read_req(b"not json").is_err());
    assert!(read_req(br#"{"t":"req","body":{"op":"nope"}}"#).is_err());
}

/// 구판 CLI는 모르는 응답 enum 대신 기존 done을 읽고, 오류는 어느 판에도 그대로 전달된다.
#[test]
fn automatic_reply_ack_is_compatible_with_old_cli() {
    let ack = FrontResponse::QuestionAnswered { id: "q-0001".into(), complete: true };
    let old = aaa_proto::Version { major: 1, minor: 165 };
    let current = aaa_proto::Version { major: 1, minor: 166 };
    assert_eq!(response_for_version(Ok(ack.clone()), old), Ok(FrontResponse::Done));
    assert_eq!(response_for_version(Ok(ack.clone()), current), Ok(ack));
    let error = ErrorBody::new(ErrorCode::BadRequest, "실제 답 처리 실패");
    assert_eq!(response_for_version(Err(error.clone()), old), Err(error.clone()));
    assert_eq!(response_for_version(Err(error.clone()), current), Err(error));
}

/// frame 상한을 넘는 응답은 연결을 닫는 대신 `too_large` 오류로 답한다(t-0334 pilot 결함 2)
#[test]
fn oversized_response_becomes_an_error() {
    let parse = |b: &[u8]| match serde_json::from_slice::<FrontMsg>(b).unwrap() {
        FrontMsg::Res { id, result } => (id, result.into_result()),
        other => panic!("{other:?}"),
    };
    let (id, r) = parse(&res_frame("7".into(), Ok(FrontResponse::Done)));
    assert_eq!((id.as_str(), r), ("7", Ok(FrontResponse::Done)));
    let item = aaa_proto::InboxItem {
        short_id: None,
        id: "i".into(),
        project: "p".into(),
        item_kind: "routine".into(),
        subject: "s".into(),
        event: "due".into(),
        summary: String::new(),
        body: "x".repeat(MAX_FRAME),
        done: false,
        seen: false,
        created_at_ms: 0,
        done_at_ms: None,
    };
    let big = FrontResponse::Inbox { items: vec![item], done: None, omitted_open: 0, omitted_done: 0 };
    let bytes = res_frame("8".into(), Ok(big));
    assert!(bytes.len() < 1024, "{}", bytes.len());
    let (id, r) = parse(&bytes);
    let e = r.unwrap_err();
    assert_eq!((id.as_str(), e.code), ("8", ErrorCode::TooLarge));
    assert!(e.message.starts_with("inbox 응답이 1024 KiB라 frame 상한 1024 KiB를 넘는다"), "{}", e.message);
}

/// 경계: 종류 byte를 더해 상한과 같으면 보낼 수 있고, payload가 상한과 같으면 `write_frame`이 거부하니 오류로 바꾼다
#[test]
fn response_frame_limit_counts_the_kind_byte() {
    let done = res_frame("9".into(), Ok(FrontResponse::Done));
    let n = done.len();
    assert_eq!(res_frame_within("9".into(), Ok(FrontResponse::Done), n + 1), done, "payload + 1 == max는 보낸다");
    let over = res_frame_within("9".into(), Ok(FrontResponse::Done), n);
    let FrontMsg::Res { result, .. } = serde_json::from_slice::<FrontMsg>(&over).unwrap() else { panic!() };
    assert_eq!(result.into_result().unwrap_err().code, ErrorCode::TooLarge, "payload == max는 넘는다");
    let mut buf = Vec::new();
    let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
    let mut wrote = |max| {
        rt.block_on(aaa_proto::frame::write_frame(&mut buf, aaa_proto::frame::FrameKind::Control, &done, max)).is_ok()
    };
    assert!(wrote(n + 1) && !wrote(n), "write_frame과 같은 경계");
}
