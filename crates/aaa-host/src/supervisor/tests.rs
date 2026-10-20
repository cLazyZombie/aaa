use super::*;
use aaa_config::HostConfig;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Lab(std::path::PathBuf);
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn host(webhook: String) -> (Lab, Arc<Host>) {
    let root = std::env::temp_dir().join(format!("aaa-supervisor-{}", aaa_common::util::new_id()));
    let mut cfg = HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    cfg.notify.webhook = webhook;
    cfg.notify.format = "json".into();
    let h = Host::open(cfg, root.join("host")).unwrap();
    let p = h.projects_root().join("demo");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(p.join("PROJECT.md"), "# demo\n").unwrap();
    (Lab(root), h)
}
async fn publish(h: &Host, now: Instant) {
    h.health_pass(now).await;
}

#[tokio::test(start_paused = true)]
async fn task_panics_and_early_exit_back_off_stop_and_notify_once() {
    let (_lab, h) = host(String::new());
    h.view_presence(1, "desk", true);
    h.touch_device("desk", "touch");
    let mut notices = h.notice_events.subscribe();
    let attempts = Arc::new(AtomicUsize::new(0));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let run = h.supervise(Task::Ticker, {
        let attempts = attempts.clone();
        move || {
            let (attempts, tx) = (attempts.clone(), tx.clone());
            async move {
                let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                tx.send(n).unwrap();
                if n != 2 {
                    panic!("fixture panic payload must never be copied");
                }
            }
        }
    });
    assert_eq!(rx.recv().await, Some(1));
    // pause/advance는 backoff 규칙만 본다. 완료는 factory의 실제 진입 사건이다.
    for (n, seconds) in [(2, 1), (3, 2), (4, 4)] {
        tokio::time::advance(Duration::from_millis(seconds * 1000 - 1)).await;
        assert_eq!(attempts.load(Ordering::SeqCst), n - 1);
        tokio::time::advance(Duration::from_millis(1)).await;
        assert_eq!(rx.recv().await, Some(n));
    }
    run.await.unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 4, "초기 실행 + 재시작 3회에서 멈춘다");
    assert!(!h.task_allowed(Task::Ticker));
    h.task_progress(Task::Ticker);
    publish(&h, Instant::now() + STABLE * 2).await;
    let n = notices.try_recv().unwrap();
    assert_eq!(n.kind, aaa_proto::NoticeKind::HostIssue);
    assert!(n.title.contains("ticker"));
    assert!(notices.try_recv().is_err(), "상한에서 회복·중복 알림 없음");
    let items = h.db.inbox_list(Some("demo"), true).unwrap();
    assert_eq!(items.len(), 1);
    assert!(!serde_json::to_string(&items).unwrap().contains("fixture panic payload"));
}

#[tokio::test]
async fn host_health_uses_closed_app_webhook_and_open_app_notice_once_per_transition() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, mut received) = tokio::sync::mpsc::unbounded_channel();
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let split = loop {
                let mut b = [0; 1024];
                let n = stream.read(&mut b).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&b[..n]);
                if let Some(pos) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    break pos + 4;
                }
            };
            let header = String::from_utf8_lossy(&bytes[..split]);
            let len: usize = header
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase().strip_prefix("content-length:").map(|s| s.trim().parse().unwrap())
                })
                .unwrap();
            while bytes.len() < split + len {
                let mut b = [0; 1024];
                let n = stream.read(&mut b).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&b[..n]);
            }
            tx.send(serde_json::from_slice::<serde_json::Value>(&bytes[split..split + len]).unwrap()).unwrap();
            stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n").await.unwrap();
        }
    });
    let (_lab, mut h) = host(format!("http://{addr}/hook"));
    let now = Instant::now();
    h.task_failed(Task::AgentLink);
    h.task_failed(Task::AgentLink);
    // 저장은 끝났지만 아직 inbox/외부 발행 전인 재시작 경계.
    let (cfg, state) = (h.cfg.clone(), h.state.clone());
    drop(h);
    h = Host::open(cfg, state).unwrap();
    let sql = rusqlite::Connection::open(h.state.join("host.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER refuse_health BEFORE INSERT ON inbox WHEN NEW.kind='host-health' BEGIN SELECT RAISE(ABORT, 'fixture storage failure'); END;").unwrap();
    drop(sql);
    publish(&h, now).await;
    assert!(h.db.inbox_list(Some("demo"), true).unwrap().is_empty());
    assert!(received.try_recv().is_err());
    // INSERT 실패가 ACK를 먼저 확정하면 재개방 뒤 사건이 없어져야 하므로 이 keeper가 잡는다.
    let (cfg, state) = (h.cfg.clone(), h.state.clone());
    drop(h);
    h = Host::open(cfg, state).unwrap();
    let sql = rusqlite::Connection::open(h.state.join("host.db")).unwrap();
    sql.execute_batch("DROP TRIGGER refuse_health;").unwrap();
    drop(sql);
    let mut notices = h.notice_events.subscribe();
    publish(&h, now).await;
    let failed =
        tokio::time::timeout(Duration::from_secs(3), received.recv()).await.expect("webhook 수신 사건").unwrap();
    assert_eq!(failed["items"][0]["event"], "failed");
    assert!(notices.try_recv().is_err(), "앱을 닫으면 외부 알림만");
    publish(&h, now + STABLE * 2).await;
    assert!(received.try_recv().is_err(), "조용해진 것만으로 회복하면 안 된다");
    // 열린 이상은 host 재시작으로 중복 발행하지 않고, 이후 회복을 이어 알린다.
    let (cfg, state) = (h.cfg.clone(), h.state.clone());
    drop(h);
    h = Host::open(cfg, state).unwrap();
    notices = h.notice_events.subscribe();
    assert!(h.task_allowed(Task::AgentLink));
    publish(&h, now + STABLE * 2).await;
    assert!(received.try_recv().is_err());
    h.health.lock().unwrap().progress(Task::AgentLink, now + STABLE * 2);
    publish(&h, now + STABLE * 3 - Duration::from_millis(1)).await;
    assert!(received.try_recv().is_err());
    assert!(notices.try_recv().is_err());
    // 회복 판정/저장 뒤 inbox commit 전에 재시작해도 회복 사건을 잃지 않는다.
    {
        let mut health = h.health.lock().unwrap();
        assert_eq!(health.pending(now + STABLE * 3).len(), 1);
        assert!(health.persist(&h.db));
    }
    let (cfg, state) = (h.cfg.clone(), h.state.clone());
    drop(h);
    h = Host::open(cfg, state).unwrap();
    notices = h.notice_events.subscribe();
    publish(&h, now + STABLE * 3).await;
    let recovered =
        tokio::time::timeout(Duration::from_secs(3), received.recv()).await.expect("webhook 수신 사건").unwrap();
    assert_eq!(recovered["items"][0]["event"], "recovered");
    assert!(recovered["items"][0]["summary"].as_str().unwrap().contains("2회"));
    publish(&h, now + STABLE * 4).await;
    assert!(received.try_recv().is_err());
    h.view_presence(1, "desk", true);
    h.touch_device("desk", "touch");
    h.task_failed(Task::AgentLink);
    publish(&h, now).await;
    assert_eq!(notices.try_recv().unwrap().kind, aaa_proto::NoticeKind::HostIssue);
    h.health.lock().unwrap().progress(Task::AgentLink, now);
    publish(&h, now + STABLE).await;
    assert_eq!(notices.try_recv().unwrap().kind, aaa_proto::NoticeKind::HostRecovered);
    assert!(notices.try_recv().is_err());
    assert!(received.try_recv().is_err(), "앱을 쓰면 webhook 중복 없음");
    let items = h.db.inbox_list(Some("demo"), true).unwrap();
    assert_eq!(items.iter().filter(|i| i.event == "failed").count(), 2);
    assert_eq!(items.iter().filter(|i| i.event == "recovered").count(), 2);
    server.abort();
    let _ = server.await;
}

#[test]
fn isolated_timeouts_expire_and_a_burst_requires_a_success_before_recovery() {
    let mut health = Health::default();
    let now = Instant::now();
    for n in 0..4 {
        health.timeout(now + Duration::from_secs(n * 31), n);
    }
    assert!(health.pending(now + STABLE * 5).is_empty());
    for n in 0..5 {
        health.timeout(now + Duration::from_secs(200 + n), n);
    }
    let events = health.pending(now + Duration::from_secs(205));
    assert_eq!(events.len(), 1);
    assert!(events[0].summary().contains("5회"));
    health.events.clear(); // 외부 판정 시험은 이 첫 사건의 ACK 뒤를 이어 본다.
    health.progress(Task::Responses, now + Duration::from_secs(206));
    health.timeout(now + Duration::from_secs(210), 210);
    assert!(health.pending(now + Duration::from_secs(500)).is_empty(), "새 시간 초과는 회복 시계를 무효로 한다");
    health.progress(Task::Responses, now + Duration::from_secs(501));
    let recovered = health.pending(now + Duration::from_secs(531));
    assert_eq!(recovered.len(), 1);
    assert!(recovered[0].recovered);
}

#[tokio::test]
async fn a_failed_inbox_commit_holds_later_recovery() {
    let (_lab, h) = host(String::new());
    h.view_presence(1, "desk", true);
    h.touch_device("desk", "touch");
    let mut notices = h.notice_events.subscribe();
    let now = Instant::now();
    h.task_failed(Task::Ticker);
    h.health.lock().unwrap().progress(Task::Ticker, now);
    let sql = rusqlite::Connection::open(h.state.join("host.db")).unwrap();
    sql.execute_batch("CREATE TRIGGER refuse_failed BEFORE INSERT ON inbox WHEN NEW.kind='host-health' AND NEW.event='failed' BEGIN SELECT RAISE(ABORT, 'fixture storage failure'); END;").unwrap();
    publish(&h, now + STABLE).await;
    assert!(h.db.inbox_list(Some("demo"), true).unwrap().is_empty(), "이상 기록이 실패하면 뒤 회복도 기다려야 한다");
    assert!(notices.try_recv().is_err());
    sql.execute_batch("DROP TRIGGER refuse_failed;").unwrap();
    drop(sql);
    publish(&h, now + STABLE).await;
    assert_eq!(notices.try_recv().unwrap().kind, aaa_proto::NoticeKind::HostIssue);
    assert_eq!(notices.try_recv().unwrap().kind, aaa_proto::NoticeKind::HostRecovered);
    assert!(notices.try_recv().is_err());
    publish(&h, now + STABLE * 2).await;
    assert!(notices.try_recv().is_err());
    let items = h.db.inbox_list(Some("demo"), true).unwrap();
    assert_eq!(items.iter().map(|i| i.event.as_str()).collect::<Vec<_>>(), ["failed", "recovered"]);
}
