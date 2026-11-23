use super::*;

#[test]
fn nudge_logs_each_reason_once_even_when_retries_alternate() {
    let mut wait = NudgeWait::default();
    assert!(wait.first("batch-1", Some("pane-a"), "quiet_wait"));
    assert!(wait.first("batch-1", Some("pane-a"), "prompt_changed"));
    assert!(!wait.first("batch-1", Some("pane-a"), "quiet_wait"));
    assert!(!wait.first("batch-1", Some("pane-a"), "prompt_changed"));
    assert!(wait.first("batch-1", Some("pane-b"), "quiet_wait"));
    assert!(wait.first("batch-2", Some("pane-b"), "quiet_wait"));
}

#[test]
fn nudge_windows_reset_on_changed_observations_and_require_two_empty_checks() {
    let t0 = Instant::now();
    let mut seen = HashMap::new();
    let idle = Duration::from_secs(2);
    assert!(!steady_observation(&mut seen, "pane-a", "screen-1", t0, idle, false));
    assert!(!steady_observation(&mut seen, "pane-a", "screen-1", t0 + idle - Duration::from_nanos(1), idle, false));
    assert!(steady_observation(&mut seen, "pane-a", "screen-1", t0 + idle, idle, false));
    assert!(!steady_observation(&mut seen, "pane-a", "screen-2", t0 + idle, idle, false));
    assert!(!steady_observation(&mut seen, "pane-b", "screen-2", t0 + idle, idle, false));

    let quiet = Duration::from_secs(1);
    let mut boxes = HashMap::new();
    assert!(!steady_observation(&mut boxes, "demo", "pane-a", t0, quiet, true));
    assert!(!steady_observation(&mut boxes, "demo", "pane-a", t0 + quiet - Duration::from_nanos(1), quiet, true));
    assert!(steady_observation(&mut boxes, "demo", "pane-a", t0 + quiet, quiet, true));
    assert!(!steady_observation(&mut boxes, "demo", "pane-b", t0 + quiet, quiet, true));
    assert!(!steady_observation(&mut boxes, "other", "pane-b", t0 + quiet, quiet, true));
    boxes.remove("demo");
    assert!(!steady_observation(&mut boxes, "demo", "pane-b", t0 + quiet, Duration::ZERO, true));
    assert!(steady_observation(&mut boxes, "demo", "pane-b", t0 + quiet, Duration::ZERO, true));
}

#[test]
fn nudge_hook_idle_and_started_wait_for_the_exact_configured_window() {
    let window = Duration::from_secs(2);
    for activity in [AgentActivity::Idle, AgentActivity::Started] {
        assert!(!idle_long_enough(&activity, 1_000, 999, window));
        assert!(!idle_long_enough(&activity, 1_000, 2_999, window));
        assert!(idle_long_enough(&activity, 1_000, 3_000, window));
    }
    for activity in [AgentActivity::Working, AgentActivity::Question, AgentActivity::Error, AgentActivity::Exited] {
        assert!(!idle_long_enough(&activity, 0, u64::MAX, window));
    }
}

#[test]
fn ticker_refreshes_usage_without_a_browser_and_releases_pending_input() {
    use std::sync::atomic::AtomicU64;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let root = std::env::temp_dir().join(format!("aaa-usage-tick-{}", aaa_common::util::new_id()));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let host_lifetime = runtime.block_on(async {
        let home = root.join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        let now = aaa_common::util::now_ms();
        let clock = Arc::new(AtomicU64::new(now));
        std::fs::write(
            home.join(".claude/.credentials.json"),
            serde_json::json!({"claudeAiOauth":{"accessToken":&aaa_test_support::token("test-only"),"expiresAt":now+86400000}}).to_string(),
        )
        .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (requests, mut received) = tokio::sync::mpsc::unbounded_channel();
        let (release, stalled) = tokio::sync::oneshot::channel();
        let mut stalled = Some(stalled);
        let server = tokio::spawn(async move {
            for (status, used) in [(200, 99), (200, 50), (503, 0), (200, 50)] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = [0; 4096];
                let mut request = Vec::new();
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let read = stream.read(&mut buf).await.unwrap();
                    assert_ne!(read, 0, "구독 HTTP 요청을 다 받기 전에 연결이 끝났다");
                    request.extend_from_slice(&buf[..read]);
                }
                requests.send(()).unwrap();
                if let Some(stalled) = stalled.take() {
                    stalled.await.unwrap();
                }
                let body = serde_json::json!({"five_hour":{"utilization":used,"resets_at":aaa_common::util::utc_iso(now+3600000)}}).to_string();
                stream
                    .write_all(
                        format!(
                            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
            }
        });
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        cfg.projects.tick_secs = 1;
        let mut h = Host::open(cfg, root.join("host")).unwrap();
        let time = clock.clone();
        Arc::get_mut(&mut h).unwrap().usage = crate::usage::UsageService::new(crate::usage::UsageContext {
            home,
            env: Default::default(),
            darwin: false,
            origin: Some(origin),
            keychain: Arc::new(|_, _| crate::usage::KeychainRead::Missing),
            run: Arc::new(|_| None),
            now: Arc::new(move || time.load(Ordering::SeqCst)),
        });
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        let dir = root.join("projects/demo");
        std::fs::create_dir_all(dir.join("automations")).unwrap();
        let text = "[on.schedule]\nevery='1h'\n[do.inbox]\ntext='입력 처리됨'\n[limits]\nusage_session_below=5\n";
        std::fs::write(dir.join("automations/check.toml"), text).unwrap();
        let rule = crate::automation::parse(&dir, "check", text).unwrap();
        h.automation_submit("demo", &rule, "pending-1", "보존할 입력".into(), None).unwrap();
        h.start_ticker();
        let mut release = Some(release);
        for (round, expected) in [(0, 99.0), (1, 50.0), (2, 50.0), (3, 50.0)] {
            clock.store(now + round * crate::usage::FRESH_MS, Ordering::SeqCst);
            tokio::time::timeout(Duration::from_secs(5), h.ticker_tick()).await.expect("조회 중에도 ticker가 끝난다");
            tokio::time::timeout(Duration::from_secs(5), async {
                received.recv().await.expect("ticker가 구독을 조회한다");
                if let Some(release) = release.take() {
                    // 응답을 막아 둔 사이 다음 tick도 외부 조회를 기다리지 않는다.
                    h.ticker_tick().await;
                    release.send(()).unwrap();
                }
                loop {
                    let report = h.usage.cached().await;
                    if report.providers.first().is_some_and(|p| {
                        p.windows.first().is_some_and(|w| w.used_percent == expected)
                            && (p.problem.is_some() == (round == 2))
                    }) {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("브라우저 없이 구독 갱신 완료");
            if round == 2 {
                h.automation_submit("demo", &rule, "pending-2", "확인 불가 입력".into(), None).unwrap();
            }
            h.automation_drain("demo", &rule).await.unwrap();
            let items = h.db.inbox_list(Some("demo"), true).unwrap();
            let done = items.iter().filter(|i| i.body == "보존할 입력").count();
            assert_eq!(done, usize::from(round > 0), "{items:?}");
            assert_eq!(
                items.iter().filter(|i| i.body == "확인 불가 입력").count(),
                usize::from(round == 3),
                "확인 불가 중 보존한 입력은 회복 뒤 한 번 처리한다: {items:?}"
            );
            if round == 2 {
                h.automation_drain("demo", &rule).await.unwrap();
                assert_eq!(
                    h.db.inbox_list(Some("demo"), true)
                        .unwrap()
                        .iter()
                        .filter(|i| i.event == "usage unavailable")
                        .count(),
                    2, "서로 다른 입력은 각각 알리고 같은 입력의 반복 drain은 추가하지 않는다"
                );
                assert!(
                    items
                        .iter()
                        .any(|i| i.item_kind == "usage" && i.subject == "check" && i.summary.contains("확인 불가")),
                    "{items:?}"
                );
                let view = h.routines("demo").unwrap();
                assert!(format!("{view:?}").contains("확인 불가"), "{view:?}");
            }
        }
        server.await.unwrap();
        let host_lifetime = Arc::downgrade(&h);
        drop(h);
        host_lifetime
    });
    // ticker와 그가 띄운 task도 Host를 보유한다. runtime 종료로 모두 놓은 뒤 지운다.
    // Windows의 SQLite 열린 핸들은 삭제 공유를 허용하지 않아, drop(h)만으로는 부족하다.
    drop(runtime);
    assert!(host_lifetime.upgrade().is_none(), "정리 전에 ticker와 배경 task가 Host·DB를 놓아야 한다");
    std::fs::remove_dir_all(root).unwrap();
}

/// 머신 끊김을 host 수준에서 센다(머신 연결 없이 `outage_pass`에 붙음 여부를 준다). 새 `Host`는 머신을 처음 보므로
/// host를 다시 띄운 직후와 같다(plan §8.134·§8.209)
#[test]
fn restarted_host_recovers_only_an_outage_still_open() {
    let state = std::env::temp_dir().join(format!("aaa-outage-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.notify.web_push = false;
    cfg.projects.root = state.join("projects").display().to_string();
    cfg.projects.outage_secs = 60;
    let h = Host::open(cfg.clone(), state.clone()).unwrap();
    let projects = ["demo".to_string()];
    for (n, p) in projects.iter().enumerate() {
        h.project_create(p, Default::default(), "test").unwrap();
        for (m, machine) in ["m-a", "m-b"].iter().enumerate() {
            let mut r =
                ThreadRec::starting(p, "일", machine, "fake", aaa_proto::Role::Worker, aaa_proto::ThreadKind::Dir, 1);
            r.id = format!("t-{n}{m}");
            r.status = ThreadStatus::Open;
            h.db.thread_import(p, &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
        }
    }
    let count = |h: &Host, p: &str, subject: &str, event: &str| {
        h.db.inbox_list(Some(p), true)
            .unwrap()
            .iter()
            .filter(|i| i.item_kind == "outage" && i.subject == subject && i.event == event)
            .count()
    };
    // 앞 host: mac-a 끊김을 알렸고 회복도 썼다(끊김 항목은 아무도 끝내지 않았다). mac-b는 끊김만 알렸다
    for p in &projects {
        h.inbox_add(p, "outage", "mac-a", "down", "끊김");
        h.inbox_add(p, "outage", "mac-a", "recovered", "회복");
        h.inbox_add(p, "outage", "mac-b", "down", "끊김");
    }
    let machines =
        |a: bool, b: bool| [("m-a".to_string(), "mac-a".to_string(), a), ("m-b".to_string(), "mac-b".to_string(), b)];
    // 새 host가 붙은 두 머신을 본다: 회복한 mac-a는 다시 쓰지 않고, 열린 끊김 mac-b만 회복을 한 번 쓴다
    h.outage_pass(&projects, &machines(true, true), 1_000);
    h.outage_pass(&projects, &machines(true, true), 2_000);
    for p in &projects {
        assert_eq!((count(&h, p, "mac-a", "recovered"), count(&h, p, "mac-b", "recovered")), (1, 1), "{p}");
    }
    // 회복 뒤 새로 오래 끊기면 다시 알린다(열린 옛 끊김 항목이 막지 않는다)
    h.outage_pass(&projects, &machines(false, true), 10_000);
    h.outage_pass(&projects, &machines(false, true), 69_999);
    assert_eq!(count(&h, "demo", "mac-a", "down"), 1, "기준 직전에는 알리지 않는다");
    h.outage_pass(&projects, &machines(false, true), 70_000);
    for p in &projects {
        assert_eq!(count(&h, p, "mac-a", "down"), 2, "{p}");
    }
    // 실제 DB를 다시 열어 이미 알린 끊김을 잇는다. 메모리 Outage를 넘기지 않는다.
    drop(h);
    let h = Host::open(cfg.clone(), state.clone()).unwrap();
    h.outage_pass(&projects, &machines(false, true), 500_000);
    h.outage_pass(&projects, &machines(false, true), 600_000);
    assert_eq!(count(&h, "demo", "mac-a", "down"), 2, "재시작 뒤 down 중복 없음");
    h.outage_pass(&projects, &machines(true, true), 600_001);
    h.outage_pass(&projects, &machines(true, true), 600_002);
    assert_eq!(count(&h, "demo", "mac-a", "recovered"), 2, "영속 down의 회복 한 번");
    drop(h);
    let h = Host::open(cfg, state.clone()).unwrap();
    // 옛 연결·알림 시각이 아무리 오래돼도 새 host가 처음 본 끊김부터 잰다.
    h.outage_pass(&projects, &machines(false, true), 9_000_000);
    h.outage_pass(&projects, &machines(false, true), 9_059_999);
    assert_eq!(count(&h, "demo", "mac-a", "down"), 2);
    h.outage_pass(&projects, &machines(true, true), 9_059_999);
    assert_eq!(count(&h, "demo", "mac-a", "recovered"), 2, "짧은 끊김은 알림 없음");
    h.outage_pass(&projects, &machines(false, true), 9_100_000);
    h.outage_pass(&projects, &machines(false, true), 9_160_000);
    h.outage_pass(&projects, &machines(false, true), 9_260_000);
    assert_eq!(count(&h, "demo", "mac-a", "down"), 3, "새 긴 끊김 한 번");
    drop(h);
    std::fs::remove_dir_all(&state).unwrap();
}

#[test]
fn short_outages_write_nothing_and_long_ones_write_one_item_each_way() {
    // herdr-projects steps.rs 같은 이름의 시험을 옮겼다(분 단위)
    let min = 60_000;
    let mut outage = Outage::default();
    assert_eq!(outage.record(false, 0, 10 * min), None);
    assert_eq!(outage.record(false, 5 * min, 10 * min), None);
    // 기준 전에 끝난 짧은 끊김은 아무것도 남기지 않는다
    assert_eq!(outage.record(true, 6 * min, 10 * min), None);

    assert_eq!(outage.record(false, 60 * min, 10 * min), None);
    assert_eq!(outage.record(false, 70 * min, 10 * min), Some(OutageEvent::Down));
    assert_eq!(outage.record(false, 90 * min, 10 * min), None);
    assert_eq!(outage.record(true, 91 * min, 10 * min), Some(OutageEvent::Recovered));
    assert_eq!(outage.record(true, 92 * min, 10 * min), None);
    // 다시 띄운 host: 앞 host가 알린 끊김이 열려 있으면 이어 끊겨도 다시 알리지 않고 돌아오면 한 번 알린다
    let mut outage = Outage::resumed(true);
    assert_eq!(outage.record(false, 0, 10 * min), None);
    assert_eq!(outage.record(false, 30 * min, 10 * min), None);
    assert_eq!(outage.record(true, 31 * min, 10 * min), Some(OutageEvent::Recovered));
    assert_eq!(Outage::resumed(false).record(true, 0, 10 * min), None);
}

fn item(kind: &str, subject: &str, event: &str) -> InboxItem {
    InboxItem {
        short_id: None,
        id: format!("{kind}-{subject}-{event}"),
        project: "demo".into(),
        item_kind: kind.into(),
        subject: subject.into(),
        event: event.into(),
        summary: "요약은 문구에 넣지 않는다".into(),
        body: String::new(),
        done: false,
        seen: false,
        created_at_ms: 0,
        done_at_ms: None,
    }
}

#[test]
fn nudge_groups_by_subject() {
    let mut items = vec![
        item("thread-state", "t-0001", "new report"),
        item("thread-state", "t-0001", "blocked on a prompt"),
        item("outage", "mac-b", "down"),
        item("thread-state", "t-0002", "new report"),
        item("session", "mac-c", "sessions ended"),
    ];
    for (n, i) in items.iter_mut().enumerate() {
        i.short_id = Some(format!("i-{:04}", n + 1));
    }
    assert_eq!(
        nudge_text("demo", &items),
        "[aaa inbox] t-0001 new report, blocked on a prompt [i-0001 i-0002]; machine mac-b down [i-0003]; t-0002 new report [i-0004]; machine mac-c sessions ended [i-0005] (aaa inbox list demo)"
    );
    let many: Vec<_> = (1..=7).map(|n| item("thread-state", &format!("t-000{n}"), "new report")).collect();
    assert!(nudge_text("demo", &many).contains("; 2 more (aaa inbox list demo)"));
}

#[test]
fn prompt_box_state() {
    let l = |t: &str| t.lines().map(String::from).collect::<Vec<_>>();
    let idle = "⏺ 끝\n──────\n❯ \n──────\n  Haiku 5.5 | ctx\n  ⏸ manual mode on\n";
    assert!(prompt_empty(&l(idle)));
    // 사람이 쓰던 글
    assert!(!prompt_empty(&l(&idle.replace("❯ ", "❯ 쓰던 글"))));
    // 지난 입력(위쪽 ❯ 줄)은 쓰던 글이 아니다
    assert!(prompt_empty(&l(&format!("❯ 지난 지시\n{idle}"))));
    // 입력 줄이 없는 일반 셸에는 보내지 않는다
    assert!(!prompt_empty(&l("user@mac-a demo % \n")));
}

#[test]
fn projection_is_toml_front_matter() {
    let mut i = item("thread-state", "t-0001", "new report");
    i.summary = "따옴표 \" 와 줄\n바꿈".into();
    let md = render_item(&i);
    let body = md.trim().trim_start_matches("+++").trim_end_matches("+++");
    let doc = body.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(doc["summary"].as_str(), Some("따옴표 \" 와 줄 바꿈"));
    assert_eq!(doc["kind"].as_str(), Some("thread-state"));
    assert_eq!(safe_subject("Mac B/외장 x!!"), "mac-b-x");
}

/// 크기 한도: 끝내지 않은 항목이 먼저(오래된 것부터), 끝낸 항목은 최근 것부터, 넘친 뒤로는 싣지 않는다(t-0334 결함 2)
#[test]
fn inbox_page_fits_open_first_then_recent_done() {
    let mk = |n: u64, done: bool| {
        let mut i = item("thread-state", &format!("t-{n:04}"), "idle");
        i.id = format!("{n:04}");
        i.created_at_ms = n;
        i.done = done;
        i
    };
    let size = serde_json::to_vec(&mk(1, false)).unwrap().len();
    let all: Vec<_> = (1..=4).map(|n| mk(n, true)).chain((5..=6).map(|n| mk(n, false))).collect();
    let ids = |p: &InboxPage| p.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    let p = fit_inbox(all.clone(), size * 100);
    assert_eq!((p.items.len(), p.omitted_open, p.omitted_done), (6, 0, 0));
    // 자리 셋: 끝내지 않은 둘, 끝낸 것 중 가장 최근 하나. 원래 차례로 돌려준다
    let p = fit_inbox(all.clone(), size * 3);
    assert_eq!(ids(&p), ["0004", "0005", "0006"]);
    assert_eq!((p.omitted_open, p.omitted_done), (0, 3));
    // 끝내지 않은 것만으로 넘친다: 오래된 것 하나만, 끝낸 것은 하나도
    let p = fit_inbox(all, size);
    assert_eq!(ids(&p), ["0005"]);
    assert_eq!((p.omitted_open, p.omitted_done), (1, 4));
    // 한 번 넘치면 뒤의 작은 항목도 싣지 않는다(실은 것은 이어진 앞이다)
    let mut big = mk(7, false);
    big.body = "x".repeat(size * 2);
    let p = fit_inbox(vec![mk(1, false), big, mk(9, false)], size * 2);
    assert_eq!(ids(&p), ["0001"]);
    assert_eq!(p.omitted_open, 2);
}

/// thread·자동화 pane이 context·inbox를 읽어도 새 항목은 「봤다」가 되지 않는다(coordinator 알림이 빠지지 않는다).
/// 세션 밖의 사람은 지금처럼 「봤다」를 남긴다(t-1225)
#[tokio::test]
async fn reading_from_a_thread_pane_keeps_the_coordinator_nudge() {
    use aaa_proto::{DeviceRole, FrontRequest};
    let state = std::env::temp_dir().join(format!("aaa-inbox-seen-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.notify.web_push = false;
    cfg.projects.root = state.join("projects").display().to_string();
    let h = Host::open(cfg, state.clone()).unwrap();
    h.project_create("demo", Default::default(), "test").unwrap();
    h.inbox_add("demo", "thread-state", "t-0001", "new report", "보고");
    let unseen = |h: &Host| h.db.inbox_list(Some("demo"), false).unwrap().iter().filter(|i| !i.seen).count();
    for actor in ["local:cli@p-thread", "worker:s1"] {
        let context = FrontRequest::Context { project: "demo".into(), peek: false };
        h.call(DeviceRole::Local, actor, context).await.unwrap();
        let list = FrontRequest::Inbox { project: "demo".into(), all: false, peek: false };
        h.call(DeviceRole::Local, actor, list).await.unwrap();
        assert_eq!(unseen(&h), 1, "{actor}");
    }
    h.call(DeviceRole::Local, "local:cli", FrontRequest::Context { project: "demo".into(), peek: false })
        .await
        .unwrap();
    assert_eq!(unseen(&h), 0, "세션 밖의 사람이 본 항목은 다시 알리지 않는다");
    drop(h);
    aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&state), "시험 폴더 정리");
}
