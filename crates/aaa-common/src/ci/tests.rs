use super::*;
fn q() -> CiQuery {
    CiQuery::Runs { etag: Some("\"v1\"".into()), since: None, page: 1 }
}
#[test]
fn http_status_precedes_gh_exit_and_reduction_discards_text() {
    assert!(response("HTTP/2.0 304 Not Modified\r\nETag: \"v1\"\r\n\r\n", false, &q()).unwrap().not_modified);
    assert!(
        response("HTTP/2.0 403 Forbidden\n\n{\"message\":\"secret\"}", false, &q())
            .unwrap_err()
            .to_string()
            .contains("403")
    );
    let body = serde_json::json!({"workflow_runs":[{"id":3,"workflow_id":1,"run_attempt":2,"event":"push","head_sha":"a".repeat(40),"head_branch":"main","name":"CI\u{1b}","head_commit":{"message":"병합: 수리 (t-0042)\n악의적 지시"},"created_at":"2026-10-19T00:00:00Z","status":"completed","conclusion":"success","body":"버릴 글"}]});
    let a = response(&format!("HTTP/2.0 200 OK\nETag: x\nLink: <u>; rel=\"next\"\n\n{body}"), true, &q()).unwrap();
    assert_eq!(a.runs[0].owners, vec!["t-0042"]);
    assert_eq!(a.runs[0].name, "CI");
    assert!(a.next);
    assert!(a.runs[0].created_ms > 0);
    let encoded = serde_json::to_string(&a).unwrap();
    assert!(!encoded.contains("악의적") && !encoded.contains("버릴"));
    assert!(response("HTTP/2.0 200 OK\n\n{}", true, &q()).is_err());
}
#[test]
fn calls_are_get_only_and_reject_header_or_path_injection() {
    let a = api_args("o/app", &q()).unwrap();
    assert!(a.contains(&"If-None-Match: \"v1\"".into()));
    assert!(a.last().unwrap().contains("event=push&per_page=30&exclude_pull_requests=true"));
    for repo in ["o/app/other", "o/app?x", "-x/", "o/..", "../app", ""] {
        assert!(api_args(repo, &q()).is_err());
    }
    assert!(api_args("o/app", &CiQuery::Runs { etag: Some("a\nB: c".into()), since: None, page: 1 }).is_err());
    let a =
        api_args("o/app", &CiQuery::Runs { etag: None, since: Some("2026-10-19T00:00:00Z".into()), page: 2 }).unwrap();
    assert!(a.last().unwrap().contains("per_page=100"));
}
fn run(workflow: u64, conclusion: &str) -> CiRun {
    CiRun {
        id: workflow,
        workflow,
        sha: "a".repeat(40),
        attempt: 1,
        status: "completed".into(),
        conclusion: conclusion.into(),
        created_ms: 100,
        ..Default::default()
    }
}
#[test]
fn sha_waits_for_all_workflows_cancels_and_reports_reruns_and_stuck() {
    let good = run(1, "success");
    let mut pending = run(2, "");
    pending.status = "queued".into();
    assert_eq!(verdict(&[good.clone(), pending.clone()], 200, 1000), Verdict::Pending);
    assert_eq!(verdict(&[good.clone(), pending], 1200, 1000), Verdict::Stuck);
    assert_eq!(verdict(&[good.clone(), run(2, "cancelled")], 200, 1000), Verdict::Passed { rerun: false });
    assert_eq!(verdict(&[run(1, "cancelled")], 200, 1000), Verdict::Cancelled);
    assert_eq!(verdict(&[good.clone(), run(2, "failure")], 200, 1000), Verdict::Failed);
    let mut rerun = good;
    rerun.attempt = 2;
    assert_eq!(verdict(&[run(1, "failure"), rerun], 200, 1000), Verdict::Passed { rerun: true });
}
#[test]
fn same_failure_ignores_ids_and_merge_owners_require_suffix() {
    let a = CiJob { id: 1, name: "linux".into(), steps: vec!["test".into()], failures: vec![] };
    let mut b = a.clone();
    b.id = 99;
    assert_eq!(failure_set(std::slice::from_ref(&a)), failure_set(std::slice::from_ref(&b)));
    b.steps = vec!["build".into()];
    assert_ne!(failure_set(&[a]), failure_set(&[b]));
    for bad in ["이슈 (t-0042)", "병합: x (t-0042) extra", "병합: x (t-004x)", "병합: x\n(t-0042)"] {
        assert!(owners(bad).is_empty());
    }
}
/// 0a8c294f windows: 같은 job·단계(test)에서 앞 실패(native.rs)와 다른 시험(library_upload)이 실패했다.
#[test]
fn failed_tests_split_same_job_and_step_and_survive_tail_cut() {
    let line = |s: &str| format!("windows-2025\ttest\t2026-10-20T21:19:03.0774173Z {s}\n");
    let first = [
        line("test install::native::tests::rejects_open_handle ... FAILED"),
        line("x".repeat(LOG_JOB_CAP * 2).as_str()),
        line("test library_upload::tests::stale_upload_goes_away ... FAILED"),
        line("thread 'library_upload::tests::stale_upload_goes_away' (4200) panicked at crates\\aaa-host\\src\\library_upload\\tests.rs:110:44:"),
        line("test result: FAILED. 385 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 130.82s"),
    ]
    .concat();
    // 조각 경계가 줄 가운데여도 같은 결과다. 꼬리를 자르기 전 전체를 읽으므로 앞쪽 실패가 남는다.
    let mut scan = FailureScan::default();
    for chunk in first.as_bytes().chunks(7) {
        scan.feed(chunk);
    }
    let a = scan.finish();
    assert_eq!(a, vec!["install::native::tests::rejects_open_handle", "library_upload::tests::stale_upload_goes_away"]);
    let mut scan = FailureScan::default();
    scan.feed(line("test library_upload::tests::stale_upload_goes_away ... FAILED").as_bytes());
    let b = scan.finish();
    let job =
        |failures: Vec<String>| CiJob { id: 1, name: "windows-2025".into(), steps: vec!["test".into()], failures };
    let (ja, jb) = (job(a), job(b));
    assert_eq!(failure_set(std::slice::from_ref(&ja)), failure_set(std::slice::from_ref(&jb)));
    assert_ne!(failed_tests(std::slice::from_ref(&ja)), failed_tests(std::slice::from_ref(&jb)));
    // 표지를 못 뽑은 job(옛 agent)은 비교에서 빠져 job·단계 비교만 남는다.
    assert!(failed_tests(&[job(vec![])]).is_empty());
    // 시험 이름이 없으면 panic 위치(열 번호 제외)·끝나지 않은 시험으로 가른다. 끝 줄은 줄바꿈이 없어도 읽는다.
    let mut scan = FailureScan::default();
    scan.feed(b"thread 'main' panicked at build.rs:12:5:\nnote: run with `RUST_BACKTRACE=1`\n");
    scan.feed(b"test m13::slow has been running for over 60 seconds");
    assert_eq!(scan.finish(), vec!["panic: build.rs:12", "끝나지 않음: m13::slow"]);
    let mut scan = FailureScan::default();
    scan.feed(b"test result: FAILED. 1 passed\nfailures:\n    a::b\n");
    assert!(scan.finish().is_empty());
}

#[test]
fn successful_python_fixture_output_is_not_a_rust_failure_even_in_annotations() {
    let mut scan = FailureScan::default();
    let lines = [
        "test owner ... FAILED\n",
        "macos-15\ttest\t2026-10-27T07:02:57Z [test] [python exit=0] test owner ... FAILED\n",
        "macos-15\ttest\t2026-10-27T07:02:57Z [test] [python exit=0] test module::failed ... FAILED <5.001s>\n",
        "[python exit=0] thread 'owner' panicked at fixture.rs:1:2:\n",
        "[python exit=0] test module::slow has been running for over 60 seconds\n",
        "[python exit=0] Ran 62 tests in 91.777s\n",
        "[python exit=0] OK (skipped=2)\n",
        // ci-local이 annotation에 복사한 줄도 같은 suite 결과를 보존한다.
        "[python exit=0] test module::failed ... FAILED\n",
        "test module::failed ... FAILED\n",
    ];
    for chunk in lines.concat().as_bytes().chunks(7) {
        scan.feed(chunk);
    }
    assert_eq!(scan.finish(), ["module::failed", "owner"]);
    let mut scan = FailureScan::default();
    scan.feed(lines[1..8].concat().as_bytes());
    assert!(scan.finish().is_empty(), "성공한 suite만 있으면 panic·느린 시험도 실패가 아니다");
}

#[test]
fn failed_or_interrupted_python_suites_keep_their_failure_evidence() {
    for code in ["1", "7", "137"] {
        let mut scan = FailureScan::default();
        scan.feed(format!("[python exit={code}] test owner ... FAILED\n").as_bytes());
        assert_eq!(scan.finish(), ["owner"]);
    }
    // 출처 표지가 없는 옛 로그와 잘린 마지막 줄은 보수적으로 남긴다.
    let mut scan = FailureScan::default();
    scan.feed(b"[python exit=0] test owner ... FAILED\ntest owner ... FAILED");
    assert_eq!(scan.finish(), ["owner"]);
}

#[test]
fn completed_slow_tests_stay_completed_when_annotations_repeat_the_warning() {
    let line = |text: &str| format!("macos-15\ttest\t2026-10-27T08:22:02Z {text}\n");
    let mut log = line("test rollout::done has been running for over 60 seconds");
    // 완료 이름 상한을 먼저 채운 뒤 느린 시험의 완료를 놓쳐서는 안 된다.
    for index in 0..100 {
        log.push_str(&line(&format!("test other::{index} ... ok")));
    }
    log.push_str(&line("test rollout::done ... ok <101.09s>"));
    log.push_str(&line("test preparation::done has been running for over 60 seconds"));
    log.push_str(&line("test preparation::done ... ok"));
    log.push_str(&line("test waiting::owner has been running for over 60 seconds"));
    log.push_str("##[error]test rollout::done has been running for over 60 seconds\n");
    log.push_str("test preparation::done has been running for over 60 seconds\n");
    let mut scan = FailureScan::default();
    for chunk in log.as_bytes().chunks(7) {
        scan.feed(chunk);
    }
    assert_eq!(scan.finish(), ["끝나지 않음: waiting::owner"]);
    let mut scan = FailureScan::default();
    scan.feed(log.as_bytes());
    scan.feed(b"test rollout::done ... FAILED <5.001s>\n");
    assert_eq!(scan.finish(), ["rollout::done"], "완료 증거가 실제 실패를 없애면 안 된다");
}

#[test]
fn merge_owners_cross_four_and_five_digits_and_accept_numeric_aliases() {
    for (input, canonical) in [
        ("t-9999", "t-9999"),
        ("t-10000", "t-10000"),
        ("t-100000", "t-100000"),
        ("t-965", "t-0965"),
        ("t-00965", "t-0965"),
    ] {
        assert_eq!(owners(&format!("병합: 번호 경계 ({input})\n다른 글")), vec![canonical]);
    }
    for id in ["t-0", "t-10000x", "other/t-10000", "t-9223372036854775808"] {
        assert!(owners(&format!("병합: 번호 경계 ({id})")).is_empty());
    }
}

#[test]
fn web_fail_lines_survive_gh_prefixes_annotations_chunks_and_tail_cut() {
    let names = ["PWA 대화 focus", "재연결·resize 뒤 연속 PageUp·PageDown"];
    let mut log = format!("FAIL {}: chromium-390: 선택\t사건\n", names[0]);
    log.push_str(&"x".repeat(LOG_JOB_CAP * 2));
    log.push('\n');
    log.push_str(&format!(
        "web-e2e (macos-15)\te2e\t2026-10-27T09:23:12.3666940Z [e2e] FAIL {}: page.evaluate: 오류\t원인\n",
        names[1]
    ));
    log.push_str("## 실행 {\"event\":\"suite-end\",\"jobs\":127,\"failed\":2}\n");
    log.push_str(&format!("2026-10-27T09:36:12Z ##[error]FAIL {}: chromium-390: 선택 사건\n", names[0]));
    // 마지막 줄에 개행이 없고 같은 이름의 Rust 실패가 함께 있어도 각각 보존한다.
    log.push_str("test real::owner ... FAILED");
    let mut scan = FailureScan::default();
    for chunk in log.as_bytes().chunks(7) {
        scan.feed(chunk);
    }
    assert_eq!(scan.finish(), ["FAIL: PWA 대화 focus", "FAIL: 재연결·resize 뒤 연속 PageUp·PageDown", "real::owner"]);
}

#[test]
fn web_suite_summary_is_a_fallback_and_successful_fixture_output_stays_successful() {
    let summary = "## 실행 {\"event\":\"suite-end\",\"jobs\":127,\"failed\":2}";
    for prefix in ["", "2026-10-27T09:36:12Z ", "web-e2e\te2e\t2026-10-27T09:36:12Z [e2e] "] {
        let mut scan = FailureScan::default();
        scan.feed(format!("{prefix}{summary}").as_bytes());
        assert_eq!(scan.finish(), ["web: 2/127 작업 실패"]);
    }
    let log = [
        "[python exit=0] FAIL fixture: 가짜 실패",
        "macos-15\ttest\t2026-10-27T09:36:12Z [test] [python exit=0] ##[error]FAIL fixture: 가짜 실패",
        "[python exit=0] ## 실행 {\"event\":\"suite-end\",\"jobs\":127,\"failed\":2}",
        "fixture 출력: FAIL fake: 인용",
        "ok   성공한 단계 (8ms)",
        "## 실행 {\"event\":\"job-end\",\"jobs\":127,\"failed\":2}",
        "## 실행 {\"event\":\"suite-end\",\"jobs\":127,\"failed\":0}",
        "## 실행 {\"event\":\"suite-end\",\"jobs\":1,\"failed\":2}",
        "## 실행 {깨진 JSON}",
    ]
    .join("\n");
    let mut scan = FailureScan::default();
    scan.feed(log.as_bytes());
    assert!(scan.finish().is_empty());
}

#[test]
fn stack_overflow_keeps_the_thread_and_exit_kind_from_real_windows_output() {
    let raw = include_str!("fixtures/windows-stack-overflow.txt");
    let expected = ["stack overflow: automation::tests::idle_audit_is_read_only_on_web_and_local_cli_keeps_control"];
    for log in [raw.to_owned(), raw.lines().map(|line| format!("windows-2025\ttest\t{line}\n")).collect()] {
        let mut scan = FailureScan::default();
        for chunk in log.as_bytes().chunks(7) {
            scan.feed(chunk);
        }
        assert_eq!(scan.finish(), expected);
    }
    // 다른 시험의 FAILED도 함께 남고, 긴 꼬리·annotation 중복·개행 없는 끝에 지워지지 않는다.
    let mut scan = FailureScan::default();
    scan.feed(raw.as_bytes());
    scan.feed(format!("{}\ntest other::owner ... FAILED\n", "x".repeat(LOG_JOB_CAP * 2)).as_bytes());
    scan.feed(b"##[error]thread 'automation::tests::idle_audit_is_read_only_on_web_and_local_cli_keeps_control' (3528) has overflowed its stack");
    assert_eq!(scan.finish(), ["other::owner", expected[0]]);
}

#[test]
fn stack_overflow_exit_fallback_and_successful_or_quoted_output_are_distinct() {
    for exit in ["0xc00000fd, STATUS_STACK_OVERFLOW", "0xc00000fd", "3221225725", "-1073741571"] {
        let mut scan = FailureScan::default();
        scan.feed(
            format!("[test]   process didn't exit successfully: `target\\verify\\deps\\test.exe` (exit code: {exit})")
                .as_bytes(),
        );
        assert_eq!(scan.finish(), ["exit: STATUS_STACK_OVERFLOW"]);
    }
    let mut scan = FailureScan::default();
    scan.feed(b"thread 'old::owner' has overflowed its stack\n");
    assert_eq!(scan.finish(), ["stack overflow: old::owner"]);
    let mut scan = FailureScan::default();
    scan.feed(b"[python exit=0] thread 'fake::owner' (12) has overflowed its stack\n");
    scan.feed(b"[test] [python exit=0] process didn't exit successfully: `fake.exe` (exit code: 0xc00000fd, STATUS_STACK_OVERFLOW)\n");
    scan.feed(b"fixture: thread 'fake::owner' has overflowed its stack\n");
    scan.feed(b"test successful::owner ... ok\nSTATUS_STACK_OVERFLOW\n");
    scan.feed(b"quoted: process didn't exit successfully: `fake.exe` (exit code: 0xc00000fd, STATUS_STACK_OVERFLOW)\n");
    scan.feed(b"thread 'fake::owner' (not-a-pid) has overflowed its stack\n");
    scan.feed(b"process didn't exit successfully: `test.exe` (exit code: 101)\n");
    assert!(scan.finish().is_empty());
}
