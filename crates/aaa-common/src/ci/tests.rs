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
    for repo in ["o/app/other", "o/app?x", "-x/", ""] {
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
    let a = CiJob { id: 1, name: "linux".into(), steps: vec!["test".into()] };
    let mut b = a.clone();
    b.id = 99;
    assert_eq!(failure_set(&[a.clone()]), failure_set(&[b.clone()]));
    b.steps = vec!["build".into()];
    assert_ne!(failure_set(&[a]), failure_set(&[b]));
    for bad in ["이슈 (t-0042)", "병합: x (t-0042) extra", "병합: x (t-004)", "병합: x\n(t-0042)"] {
        assert!(owners(bad).is_empty());
    }
}
