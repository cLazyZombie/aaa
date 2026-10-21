use super::*;
use aaa_proto::AgentRequest;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::io::{DuplexStream, ReadBuf};

// 실험실 전송 계층이 읽기 중 panic한다. 제품 flag/주입 hook은 없다.
struct FaultIo {
    stream: DuplexStream,
    fault: Arc<AtomicBool>,
}
impl AsyncRead for FaultIo {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        let result = Pin::new(&mut self.stream).poll_read(cx, buf);
        if result.is_ready() && self.fault.load(Ordering::SeqCst) {
            panic!("fixture-link-private-panic-payload");
        }
        result
    }
}
impl AsyncWrite for FaultIo {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
struct Lab(std::path::PathBuf);
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn host() -> (Lab, Arc<Host>) {
    let path = std::env::temp_dir().join(format!("aaa-link-watch-{}", util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = path.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, path.join("host")).unwrap();
    let project = path.join("projects/demo");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("PROJECT.md"), "# demo\n").unwrap();
    (Lab(path), h)
}
type Reader = FrameRx<tokio::io::ReadHalf<DuplexStream>>;
async fn connect(h: Arc<Host>) -> (tokio::task::JoinHandle<Result<()>>, FrameTx, Reader, Arc<AtomicBool>) {
    let (stream, peer) = tokio::io::duplex(65536);
    let fault = Arc::new(AtomicBool::new(false));
    let running = tokio::spawn(serve(h, Box::new(FaultIo { stream, fault: fault.clone() }), None, None));
    let (r, w) = tokio::io::split(peer);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    tx.json(&LinkMsg::Hello(AgentHello {
        proto: AGENT_LINK,
        machine_id: "fake-agent".into(),
        machine_name: "mac-a".into(),
        boot_id: "boot".into(),
        os: "test".into(),
        arch: "test".into(),
        software: "fixture".into(),
        features: vec!["terminal.v1".into()],
        dev_mode: true,
        sha256: None,
        selfcheck: None,
        sessions: vec![],
        harness: vec![],
    }))
    .await
    .unwrap();
    assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Welcome(_)));
    tx.json(&LinkMsg::Ping { seq: 1 }).await.unwrap();
    assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 1 });
    (running, tx, rx, fault)
}

#[tokio::test(start_paused = true)]
async fn link_panics_release_pending_requests_and_bound_reconnections() {
    let (_lab, h) = host();
    h.view_presence(1, "desk", true);
    h.touch_device("desk", "touch");
    let mut notices = h.notice_events.subscribe();
    let monitor = h.start_supervisor();
    for _ in 0..4 {
        let (running, tx, mut rx, fault) = connect(h.clone()).await;
        let conn = h.live_conn("fake-agent").unwrap();
        let request = tokio::spawn({
            let conn = conn.clone();
            async move { conn.request(AgentRequest::Profiles).await }
        });
        assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Req { .. }));
        fault.store(true, Ordering::SeqCst);
        tx.json(&LinkMsg::Ping { seq: 2 }).await.unwrap();
        let result = running.await.expect("panic을 감시 경계가 받아야 한다");
        assert!(result.unwrap_err().to_string().contains("내용 숨김"));
        assert_eq!(
            request.await.unwrap().unwrap_err().code,
            ErrorCode::AgentDisconnected,
            "panic 뒤 timeout까지 기다리지 않고 pending을 풀어야 한다"
        );
        assert!(conn.pending.lock().unwrap().is_empty());
        assert!(h.live_conn("fake-agent").is_err());
    }
    assert!(!h.task_allowed(Task::AgentLink));
    let (stream, _peer) = tokio::io::duplex(1024);
    assert!(serve(h.clone(), Box::new(stream), None, None).await.unwrap_err().to_string().contains("상한"));
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(10), notices.recv()).await.expect("알림 사건").unwrap().kind,
        aaa_proto::NoticeKind::HostIssue
    );
    // 종료 사건을 모두 처리한 뒤 같은 이상은 inbox에도 하나다.
    assert_eq!(h.db.inbox_list(Some("demo"), true).unwrap().len(), 1);
    assert!(notices.try_recv().is_err());
    let items = h.db.inbox_list(Some("demo"), true).unwrap();
    assert!(!serde_json::to_string(&items).unwrap().contains("private-panic-payload"));
    monitor.abort();
    let _ = monitor.await;
}

#[tokio::test(start_paused = true)]
async fn real_agent_request_timeout_burst_reaches_the_independent_monitor() {
    let (_lab, h) = host();
    h.view_presence(1, "desk", true);
    h.touch_device("desk", "touch");
    let mut notices = h.notice_events.subscribe();
    let monitor = h.start_supervisor();
    let (running, tx, mut rx, _fault) = connect(h.clone()).await;
    let conn = h.live_conn("fake-agent").unwrap();
    for n in 0..5 {
        let (result, ()) =
            tokio::join!(conn.request_timeout(AgentRequest::Profiles, Duration::from_millis(10)), async {
                assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Req { .. }));
                // 요청은 받되 답하지 않는다. 실제 AgentConn deadline 사건으로 끝난다.
            });
        assert_eq!(result.unwrap_err().code, ErrorCode::Timeout);
        assert!(conn.pending.lock().unwrap().is_empty());
        if n < 4 {
            assert!(notices.try_recv().is_err());
        }
    }
    let notice = tokio::time::timeout(Duration::from_secs(10), notices.recv()).await.expect("알림 사건").unwrap();
    assert_eq!(notice.kind, aaa_proto::NoticeKind::HostIssue);
    assert!(notice.title.contains("agent-responses"));
    assert!(notice.title.contains("5회"));
    // 같은 reader가 이후 실제 성공 응답도 받는다. 전송 단절로 속여 회복시키지 않는다.
    let (result, ()) = tokio::join!(conn.request(AgentRequest::Profiles), async {
        let LinkMsg::Req { id, .. } = rx.control::<LinkMsg>().await.unwrap() else { panic!("요청") };
        tx.json(&LinkMsg::Res { id, result: Outcome::Ok(AgentResponse::Profiles { profiles: vec![] }) }).await.unwrap();
    });
    assert!(matches!(result.unwrap(), AgentResponse::Profiles { .. }));
    assert_eq!(h.db.inbox_list(Some("demo"), true).unwrap().len(), 1);
    drop(tx);
    drop(rx);
    drop(conn);
    running.await.unwrap().unwrap();
    monitor.abort();
    let _ = monitor.await;
}
