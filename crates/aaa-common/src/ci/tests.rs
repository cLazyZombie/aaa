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
