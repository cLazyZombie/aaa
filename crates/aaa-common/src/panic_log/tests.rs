use super::*;
use std::process::Command;

use aaa_test_support::CANARY as SECRET;
const NAME_SECRET: &str = SECRET;
const BUILD_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SOURCE_FILES: &[&str] = &["crates/aaa-common/src/panic_log.rs", "crates/aaa-common/src/panic_log/tests.rs"];

// 공개 hook을 실제 자식 프로세스에서 실행한다. 전역 hook·환경을 다른 시험과 공유하지 않는다.
#[test]
fn hook_child() {
    let Ok(role) = std::env::var("AAA_PANIC_TEST_ROLE") else { return };
    install(BUILD_SHA, SOURCE_FILES);
    set_process(match role.as_str() {
        "host" => ProcessKind::Host,
        "agent" => ProcessKind::Agent,
        "session-host" => ProcessKind::SessionHost,
        _ => ProcessKind::Cli,
    });
    #[inline(never)]
    fn secret_panic() {
        let _ = std::panic::catch_unwind(|| std::panic::panic_any(SECRET.to_owned()));
    }
    // stderr 소비자가 막혀 있어도 감시자는 panic 위치를 읽을 수 있어야 한다.
    let stderr = std::io::stderr().lock();
    let panicking = std::thread::Builder::new().name(NAME_SECRET.into()).spawn(secret_panic).unwrap();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observer_stop = stop.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let observer = std::thread::spawn(move || {
        while !observer_stop.load(Ordering::Relaxed) {
            if latest().is_some() {
                let _ = tx.send(());
                break;
            }
            std::thread::yield_now();
        }
    });
    let observed = rx.recv_timeout(std::time::Duration::from_secs(5));
    drop(stderr);
    stop.store(true, Ordering::Relaxed);
    panicking.join().unwrap();
    observer.join().unwrap();
    assert!(observed.is_ok(), "stderr가 막힌 동안 panic 위치를 읽지 못했다");
    // 동적으로 주어진 thread 이름·String payload와 서로 다른 thread의 같은 위치 반복.
    for _ in 0..2 {
        std::thread::Builder::new().name(NAME_SECRET.into()).spawn(secret_panic).unwrap().join().unwrap();
    }
    std::thread::Builder::new()
        .name("aaa-cli".into())
        .spawn(|| {
            // 문자열이 아닌 payload도 포맷하지 않아야 한다.
            struct PrivatePayload(#[allow(dead_code)] String);
            let _ = std::panic::catch_unwind(|| std::panic::panic_any(PrivatePayload(SECRET.into())));
        })
        .unwrap()
        .join()
        .unwrap();
    tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap().block_on(async {
        assert!(tokio::spawn(async { panic!("{SECRET}") }).await.is_err());
    });
    let last = latest().expect("panic 위치가 없다");
    assert!(last.task_id.is_some());
    assert_eq!(last.thread, "tokio-runtime-worker");
    // 알림 소비자가 받는 snapshot에도 비밀이 없어야 한다.
    println!("\nlatest: {}", serde_json::to_string(&last).unwrap());
}

#[test]
fn hook_reports_safe_locations_for_every_role_without_payload_or_names() {
    for role in ["cli", "host", "agent", "session-host"] {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "panic_log::tests::hook_child", "--nocapture"])
            .env("AAA_PANIC_TEST_ROLE", role)
            .env("AAA_PANIC_TEST_SECRET", SECRET)
            .env("RUST_BACKTRACE", "full")
            .output()
            .unwrap();
        assert!(output.status.success(), "panic 실험실 실패: {}", String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        for log in [&stdout, &stderr] {
            for forbidden in [SECRET, NAME_SECRET, "stack backtrace", "panicked at", "/Users/me/", "\\Users\\me\\"] {
                assert!(!log.contains(forbidden), "{role}: 금지된 정보가 출력됐다");
            }
            assert!(!log.contains(env!("CARGO_MANIFEST_DIR")), "{role}: 빌드 절대 경로가 출력됐다");
        }
        let events: Vec<serde_json::Value> = stderr
            .lines()
            .map(|line| serde_json::from_str(line.strip_prefix("aaa panic: ").expect("예상 밖 stderr")).unwrap())
            .collect();
        assert_eq!(events.len(), 5, "{role}: panic 기록을 잃었다");
        for event in &events {
            assert_eq!(event["process"], role);
            assert_eq!(event["file"], "crates/aaa-common/src/panic_log/tests.rs");
            assert!(event["line"].as_u64().unwrap() > 0);
            assert!(event["column"].as_u64().unwrap() > 0);
            assert!(event["thread_id"].as_str().unwrap().starts_with("ThreadId("));
            assert_eq!(event["build_sha"], BUILD_SHA);
            let stack = event["stack"].as_array().expect("콜스택이 없다");
            assert!(!stack.is_empty());
            assert!(stack.len() <= 64);
            assert!(
                stack.iter().any(|f| f["function"].as_str().unwrap().contains("panic_log::tests::hook_child")),
                "{stack:?}"
            );
            for frame in stack {
                let file = frame["file"].as_str().unwrap();
                assert!(file == "external" || SOURCE_FILES.contains(&file), "원본 경로: {file}");
            }
        }
        for (index, event) in events[..3].iter().enumerate() {
            assert_eq!(event["count"], index + 1);
            assert_eq!(event["thread"], "other");
            assert_eq!(event["line"], events[0]["line"]);
            assert_eq!(event["column"], events[0]["column"]);
        }
        let ids: std::collections::BTreeSet<_> = events[..3].iter().map(|e| e["thread_id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), 3, "다른 thread의 같은 위치도 합산해야 한다");
        assert_eq!(events[3]["thread"], "aaa-cli");
        assert_eq!(events[3]["count"], 1);
        assert_eq!(events[4]["thread"], "tokio-runtime-worker");
        assert!(events[4]["task_id"].as_str().is_some_and(|s| s.parse::<u64>().is_ok()));
        let latest: serde_json::Value = serde_json::from_str(
            stdout.lines().find_map(|s| s.strip_prefix("latest: ")).expect("snapshot 출력을 잃었다"),
        )
        .unwrap();
        assert_eq!(latest, events[4]);
    }
}

#[test]
fn location_paths_are_selected_from_repository_files_only() {
    let known = "crates/aaa-common/src/panic_log.rs";
    for file in [
        known.into(),
        format!("/Users/me/private-build/{known}"),
        format!("C:\\Users\\me\\private-build\\{}", known.replace('/', "\\")),
    ] {
        assert_eq!(safe_file(&file, SOURCE_FILES), known);
    }
    for file in ["/Users/me/private-dependency/src/lib.rs", "C:\\Users\\me\\private.rs", "private-panic-payload.rs"] {
        assert_eq!(safe_file(file, SOURCE_FILES), "external");
    }
}
