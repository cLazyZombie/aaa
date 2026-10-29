use super::*;
use std::{path::PathBuf, time::Duration};
struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("aaa-metrics-{}", aaa_common::util::new_id()));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn put(&self, p: &str, s: &str) {
        let p = self.0.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixture(l: &Lab) {
    l.put("target/ci-local/run/workspace-summary.tsv","v1|100|164|succeeded\nweb|ok|10:00|10:01|2|ignored\nbuild|ok|10:01|10:02|12|ignored\ntest|ok|10:02|10:03|100|ignored\ne2e|ok|10:02|10:04|140|ignored\n");
    l.put(
        "target/ci-local/run/rust/summary.json",
        r#"{"jobs":[{"name":"unit","seconds":100,"log":"a.log"},{"name":"m13-1","seconds":600,"log":"b.log"}]}"#,
    );
    l.put(
        "target/ci-local/run/rust/a.log",
        "test result: ok. 10 passed; 0 failed; 2 ignored; 0 measured; 4 filtered out; finished in 99s\n",
    );
    l.put(
        "target/ci-local/run/rust/b.log",
        "test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 5 filtered out; finished in 600s\n",
    );
    l.put("target/ci-local/run/e2e.log","## 요약: 파일 2개 × 1번, worker 2개, 모두 140.0초\n## 작업: 3개(독립 단계 분할 포함, 실패한 작업은 다시 실행하지 않음)\n##   01-first      통과 2/2  150.5초 (독립 작업 합계)\n##   02-second     실패 0/1  11.2초\n");
}
#[test]
fn ci_reads_existing_counts_and_spec_work_without_inventing_individual_test_times() {
    let l = Lab::new();
    fixture(&l);
    let r = ci_run(&l.0.join("target/ci-local/run"), 1000, "run".into()).unwrap();
    assert_eq!(r.verification_ms, Some(164000));
    assert_eq!(r.build_ms, Some(12000));
    let c = r.tests.unwrap();
    assert_eq!((c.passed, c.failed, c.skipped, c.filtered), (13, 1, 2, 9));
    assert!(r.slow_tests.items.is_empty());
    assert!(r.slow_tests.missing_reason.is_some());
    assert_eq!((r.e2e_files, r.e2e_jobs), (Some(2), Some(3)));
    assert_eq!(r.slow_e2e.items.iter().map(|t| t.duration_ms).collect::<Vec<_>>(), [150500, 11200]);
    assert_eq!(r.slow_e2e.unit, "spec-work-sum");
    assert!(r.profile.is_none());
    assert!(r.artifact_bytes.is_none());
}
#[test]
fn split_owner_results_keep_the_complete_rust_count() {
    let l = Lab::new();
    fixture(&l);
    let jobs: Vec<_> = (0..350).map(|n| serde_json::json!({"log": format!("{n}-owner.log")})).collect();
    l.put("target/ci-local/run/rust/summary.json", &serde_json::json!({"jobs": jobs}).to_string());
    for n in 0..350 {
        l.put(
            &format!("target/ci-local/run/rust/{n}-owner.log"),
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n",
        );
    }
    let r = ci_run(&l.0.join("target/ci-local/run"), 1000, "run".into()).unwrap();
    let counts = r.tests.expect("분할한 Rust 결과를 잃었다");
    assert_eq!((counts.passed, counts.failed, counts.skipped), (350, 0, 0));
}

#[test]
fn same_input_version_reuses_small_summary_after_source_logs_disappear() {
    let l = Lab::new();
    fixture(&l);
    main_repo(&l);
    let state = Lab::new();
    let a = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(a.runs.len(), 1);
    fs::remove_file(l.0.join("target/ci-local/run/rust/a.log")).unwrap();
    fs::remove_file(l.0.join("target/ci-local/run/e2e.log")).unwrap();
    let b = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(a.runs, b.runs, "한 번 읽은 입력은 로그를 다시 읽지 않는다");
}
#[test]
fn missing_and_repeated_e2e_timings_are_missing_instead_of_zero_or_fake_rankings() {
    let l = Lab::new();
    fixture(&l);
    l.put("target/ci-local/run/rust/a.log", "crashed\n");
    l.put("target/ci-local/run/e2e.log","## 요약: 파일 1개 × 2번, worker 1개, 모두 4.0초\n## 작업: 2개(독립 단계 분할 포함)\n##   01-repeat 통과 2/2 중간 2.0초, 가장 긴 3.0초\n");
    let r = ci_run(&l.0.join("target/ci-local/run"), 1000, "r".into()).unwrap();
    assert!(r.tests.is_none());
    assert!(r.slow_e2e.items.is_empty());
    assert!(!r.slow_e2e.complete);
}
#[test]
fn rc_reads_existing_individual_test_and_scenario_step_durations_and_rejects_old_results() {
    let l = Lab::new();
    l.put("target/verification/gates/.started", "");
    for name in ["default-tests", "workspace-tests"] {
        l.put(&format!("target/verification/{name}/report.json"),r#"[{"target":"game","exit":0,"tests":[{"name":"fast","event":"ok","exec_time":0.12},{"name":"slow","event":"failed","exec_time":1.5},{"name":"skip","event":"ignored"}],"suites":[{"passed":1,"failed":1,"ignored":1,"filtered_out":0}]}]"#);
    }
    l.put(
        "target/rc-e2e/report.json",
        r#"{"status":"passed","reports":[{"scenario":"a","duration_ms":312,"status":"passed"}]}"#,
    );
    l.put("target/verification/summary.json",r#"{"format":1,"status":"failed","mode":"default","totalSeconds":12,"gates":[{"name":"runtime","seconds":9,"status":"failed"}]}"#);
    let p = l.0.join("target/verification/summary.json");
    let (_, at) = version(&p).unwrap();
    let r = rc_run(&p, at, "r".into()).unwrap();
    assert_eq!(r.verification_ms, Some(12000));
    assert_eq!(r.outcome, "failed");
    let c = r.tests.unwrap();
    assert_eq!((c.passed, c.failed, c.skipped), (2, 2, 2));
    assert_eq!(r.slow_tests.items.len(), 4);
    assert_eq!(r.slow_tests.items[0].duration_ms, 1500);
    assert_eq!(r.e2e_files, Some(1));
    assert_eq!(r.e2e_unit.as_deref(), Some("scenario"));
    assert_eq!(r.slow_e2e.unit, "scenario-step-sum");
    assert_eq!(r.slow_e2e.items[0].duration_ms, 312);
    assert!(
        fresh_json(&l.0.join("target/rc-e2e/report.json"), at + 1, at + 2).is_none(),
        "다른 실행의 보고를 현재 기록에 붙이지 않는다"
    );
}
fn git(l: &Lab, args: &[&str]) -> String {
    let o = std::process::Command::new("git")
        .args(["-C", l.0.to_str().unwrap(), "-c", "user.name=Test", "-c", "user.email=test@example.com"])
        .args(args)
        .env("GIT_COMMITTER_DATE", "2001-01-01T00:00:00Z")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap().trim().into()
}
fn collect_lines(cwd: &Path, store: &Path, known: &[String]) -> Result<Option<aaa_proto::CodeLines>> {
    // 집계 규칙·오류 원인 시험은 실제 Git을 쓰되 머신 부하로 시간 상한에 걸리지 않게 한다.
    lines::collect_with_elapsed(cwd, store, known, || Duration::ZERO)
}
#[test]
fn code_lines_use_commit_objects_incrementally_handle_rename_delete_and_keep_mixed_code_as_total() {
    let l = Lab::new();
    let state = Lab::new();
    git(&l, &["init", "-q", "-b", "main"]);
    l.put("src/a.rs", "fn main() {}\n\n// 주석\n");
    l.put("web/e2e/a.mjs", "a\nb\n");
    l.put("vendor/x.rs", "exclude\n");
    git(&l, &["add", "."]);
    git(&l, &["commit", "-qm", "first"]);
    let a = collect_lines(&l.0, &state.0, &[]).unwrap().unwrap();
    assert_eq!(a.total_lines, 4);
    assert_eq!(a.scanned_files, 2);
    assert_eq!(a.breakdown.as_ref().unwrap().get("e2e"), Some(&2));
    l.put("src/a.rs", "dirty\nnew\nlines\n");
    let cached = collect_lines(&l.0, &state.0, &[]).unwrap().unwrap();
    assert_eq!(cached, a, "dirty 파일은 커밋의 줄 수를 바꾸지 않는다");
    git(&l, &["mv", "web/e2e/a.mjs", "src/b.mjs"]);
    l.put("src/a.rs", "fn main() {}\n#[cfg(test)]\nmod tests {}\n");
    git(&l, &["add", "."]);
    git(&l, &["commit", "-qm", "second"]);
    let b = collect_lines(&l.0, &state.0, &[]).unwrap().unwrap();
    assert_eq!(b.total_lines, 5);
    assert!(b.breakdown.is_none());
    assert_eq!(b.split_status, "mixed_inline_tests");
    assert_eq!(b.changed_files, 2);
    assert!(
        collect_lines(&l.0, &state.0, &[format!("{}:1", b.commit)]).unwrap().is_none(),
        "host가 이미 가진 커밋·규칙은 다른 머신에서도 재집계하지 않는다"
    );
    git(&l, &["rm", "src/b.mjs"]);
    git(&l, &["commit", "-qm", "third"]);
    let c = collect_lines(&l.0, &state.0, &[]).unwrap().unwrap();
    assert_eq!(c.total_lines, 3);
    assert_eq!(c.changed_files, 0);
}
#[test]
fn malformed_or_oversized_inputs_remain_pending_without_a_successful_zero() {
    let l = Lab::new();
    main_repo(&l);
    let state = Lab::new();
    l.put("target/verification/summary.json", "{");
    let r = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert!(r.runs.is_empty());
    assert!(!r.notes.is_empty());
    l.put("target/ci-local/large/workspace-summary.tsv", "");
    fs::OpenOptions::new()
        .write(true)
        .open(l.0.join("target/ci-local/large/workspace-summary.tsv"))
        .unwrap()
        .set_len(MAX_INPUT + 1)
        .unwrap();
    let large = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert!(large.runs.is_empty());
    assert!(!large.notes.is_empty());
    assert_eq!(millis(f64::NAN), None);
    assert_eq!(millis(-1.0), None);
    assert_eq!(millis(0.125), Some(125));
}

#[test]
fn engine_uses_existing_timings_and_runtime_reports_without_treating_rc_timing_as_engine() {
    let l = Lab::new();
    main_repo(&l);
    let state = Lab::new();
    l.put("target/verify-timing.txt", "verify.sh timing (all gates passed)\ntotal 12s\n");
    let rc = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert!(rc.runs.is_empty());
    assert!(!rc.notes.iter().any(|n| n.contains("형식")), "rc 시간표를 엔진으로 오인하지 않는다");
    l.put("scripts/verify_engine.sh", "# 엔진 검증 입구\n");
    l.put("target/verification/gates/cargo_fmt.status", "cargo_fmt\tcargo fmt\t0\t1\n");
    for name in ["workspace-tests", "automation-tests", "2d-tests", "3d-tests"] {
        l.put(&format!("target/verification/{name}/report.json"),r#"[{"target":"engine","exit":0,"tests":[{"name":"unit","event":"ok","exec_time":0.25}],"suites":[{"passed":1,"failed":0,"ignored":0,"filtered_out":0}]}]"#);
    }
    l.put(
        "target/no2-e2e/all-headless.json",
        r#"{"status":"passed","reports":[{"scenario":"headless","status":"passed","duration_ms":2500}]}"#,
    );
    l.put("target/verify-timing.txt", "verify_engine.sh timing (mode default, coverage not run, all gates passed)\ncargo fmt 1s\nworkspace tests 8s\ntotal 12s\n");
    let output = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(output.runs.len(), 1);
    let run = &output.runs[0];
    assert_eq!(run.verification_ms, Some(12000));
    assert_eq!(run.stages.len(), 2);
    assert_eq!(run.tests.as_ref().unwrap().passed, 4);
    assert!(run.profile.is_none());
    assert_eq!(run.slow_tests.items.len(), 4);
    assert_eq!(run.slow_e2e.items[0].duration_ms, 2500);
    assert_eq!(run.slow_e2e.unit, "scenario-step-sum");
}

#[test]
fn bounded_log_read_keeps_known_total_instead_of_discarding_the_entire_run() {
    let l = Lab::new();
    let state = Lab::new();
    fixture(&l);
    main_repo(&l);
    // 각각은 한도 안이지만 두 파일 합계는 회수 예산을 넘긴다.
    let log = "x".repeat((MAX_INPUT / 2 + 100) as usize);
    l.put("target/ci-local/run/rust/a.log", &log);
    l.put("target/ci-local/run/rust/b.log", &log);
    let result = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(result.runs.len(), 1);
    assert_eq!(result.runs[0].verification_ms, Some(164000));
    assert!(result.runs[0].tests.is_none());
    assert!(result.notes.iter().any(|n| n.contains("비용 상한")));
    fs::remove_file(l.0.join("target/ci-local/run/rust/a.log")).unwrap();
    let again = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(again.runs, result.runs, "부분 기록도 입력 판별로 재사용한다");
}

#[test]
fn failed_code_scan_is_not_repeated_for_the_same_commit_and_next_commit_can_recover() {
    let l = Lab::new();
    let state = Lab::new();
    git(&l, &["init", "-q", "-b", "main"]);
    fs::write(l.0.join("broken.ts"), [0xff, 0xfe]).unwrap();
    git(&l, &["add", "."]);
    git(&l, &["commit", "-qm", "broken source"]);
    // 같은 손상 객체도 예산이 먼저 끝나면 UTF-8 대신 시간 상한 오류가 난다.
    let expired = Lab::new();
    let limit_error = lines::collect_with_elapsed(&l.0, &expired.0, &[], || Duration::from_millis(750)).unwrap_err();
    assert_eq!(limit_error.to_string(), "코드 집계 경과 상한");
    let first = collect_lines(&l.0, &state.0, &[]).unwrap_err();
    assert!(first.to_string().contains("UTF-8"), "{first:#}");
    assert!(!state.0.join("lines.json").exists(), "실패한 집계를 성공 스냅샷으로 저장하지 않는다");
    // 같은 객체를 다시 해독하면 UTF-8 오류다. 두 번째 회수는 객체 읽기 전에 멈춘다.
    let second =
        lines::collect_with_elapsed(&l.0, &state.0, &[], || panic!("같은 커밋은 객체 읽기 전에 멈춘다")).unwrap_err();
    assert!(second.to_string().contains("이미 시도"), "{second:#}");
    l.put("broken.ts", "export const repaired = true;\n");
    git(&l, &["add", "."]);
    git(&l, &["commit", "-qm", "fixed source"]);
    assert_eq!(collect_lines(&l.0, &state.0, &[]).unwrap().unwrap().total_lines, 1);
}

#[test]
fn code_scan_elapsed_limit_preserves_snapshot_and_next_commit_can_recover() {
    let l = Lab::new();
    git(&l, &["init", "-q", "-b", "main"]);
    l.put("a.rs", "fn a() {}\n");
    l.put("b.rs", "fn b() {}\n");
    git(&l, &["add", "."]);
    git(&l, &["commit", "-qm", "first"]);
    let before_limit = Duration::from_nanos(749_999_999);
    let limit = Duration::from_millis(750);
    // 750ms 직전·정확한 경계·초과와 첫 객체 처리 뒤 상한 도달을 실제 대기 없이 검사한다.
    let cases = [
        ([before_limit; 2], true),
        ([limit; 2], false),
        ([Duration::from_millis(751); 2], false),
        ([Duration::ZERO, limit], false),
    ];
    let states: Vec<_> = cases.iter().map(|_| Lab::new()).collect();
    for state in &states {
        assert_eq!(collect_lines(&l.0, &state.0, &[]).unwrap().unwrap().total_lines, 2);
    }
    l.put("a.rs", "fn a() {}\n// changed\n");
    l.put("b.rs", "fn b() {}\n// changed\n");
    git(&l, &["add", "."]);
    git(&l, &["commit", "-qm", "changed"]);
    for ((times, succeeds), state) in cases.iter().zip(&states) {
        let snapshot = fs::read(state.0.join("lines.json")).unwrap();
        let mut checks = 0;
        let result = lines::collect_with_elapsed(&l.0, &state.0, &[], || {
            let elapsed = times[checks];
            checks += 1;
            elapsed
        });
        if *succeeds {
            assert_eq!(result.unwrap().unwrap().total_lines, 4);
            assert_eq!(checks, 2);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.to_string(), "코드 집계 경과 상한");
            assert_eq!(checks, if times[0] < limit { 2 } else { 1 });
            assert_eq!(fs::read(state.0.join("lines.json")).unwrap(), snapshot, "부분 집계는 저장하지 않는다");
            let again = lines::collect_with_elapsed(&l.0, &state.0, &[], || panic!("실패한 커밋은 재스캔하지 않는다"))
                .unwrap_err();
            assert!(again.to_string().contains("이미 시도"), "{again:#}");
        }
    }
    git(&l, &["commit", "--allow-empty", "-qm", "recover"]);
    for ((_, succeeded), state) in cases.iter().zip(&states) {
        let recovered = collect_lines(&l.0, &state.0, &[]).unwrap().unwrap();
        assert_eq!(recovered.total_lines, 4);
        assert_eq!(recovered.changed_files, if *succeeded { 0 } else { 2 });
    }
}

#[cfg(unix)]
#[test]
fn collection_lock_releases_when_the_owner_finishes_even_with_an_inherited_descriptor() {
    let l = Lab::new();
    let path = l.0.join("collect.lock");
    let owner = CollectionLock(aaa_common::private_file::lock(&path).unwrap());
    // fork가 물려받는 것과 같은 열린 파일 참조. 실제 다른 소유자가 있는 동안은 거부한다.
    let inherited = owner.0.try_clone().unwrap();
    assert!(aaa_common::private_file::lock(&path).is_err());
    drop(owner);
    let next = CollectionLock(aaa_common::private_file::lock(&path).unwrap());
    drop(inherited);
    drop(next);
}

fn main_repo(l: &Lab) {
    git(l, &["init", "-q", "-b", "main"]);
    l.put("src/app.ts", "export const n = 1;\n");
    git(l, &["add", "src"]);
    git(l, &["commit", "-qm", "main fixture"]);
    git(l, &["remote", "add", "origin", "https://example.com/team/app.git"]);
    git(l, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    if let Ok(body) = fs::read_to_string(l.0.join("target/ci-local/run/workspace-summary.tsv")) {
        l.put("target/ci-local/run/workspace-summary.tsv", &body.replace("v1|100|", "v1|1000000000|"));
    }
}
#[test]
fn remote_identity_normalizes_transport_but_keeps_forks_and_path_case_separate() {
    let id = mainline::remote_id("https://example.com/team/App.git");
    assert!(id.is_some());
    assert_eq!(id, mainline::remote_id(&format!("git@{}:team/App.git", "example.com".to_ascii_uppercase())));
    assert_eq!(id, mainline::remote_id("ssh://git@example.com/team/App"));
    assert_ne!(id, mainline::remote_id("https://example.com/fork/App.git"));
    assert_ne!(id, mainline::remote_id("https://example.com/team/app.git"));
    assert_eq!(mainline::remote_id("/work/app"), None);
}
#[test]
fn legacy_runs_need_main_checkout_but_confirmed_records_and_code_follow_pushed_objects() {
    let l = Lab::new();
    let state = Lab::new();
    fixture(&l);
    main_repo(&l);
    let saved = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(saved.runs.len(), 1);
    assert_eq!(saved.code_lines.as_ref().unwrap().branch, "main");
    git(&l, &["checkout", "-qb", "thread"]);
    let branch = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(branch.runs, saved.runs, "확정한 main 결과는 현재 checkout과 독립이다");
    assert_eq!(branch.code_lines.unwrap().commit, saved.code_lines.unwrap().commit);
    assert!(branch.remote_id.is_some());
    git(&l, &["checkout", "-q", "main"]);
    l.put("src/app.ts", "export const n = 2;\n");
    git(&l, &["add", "src"]);
    git(&l, &["commit", "-qm", "not pushed"]);
    assert_eq!(collect(&state.0, &l.0, "ws", &[]).unwrap().runs, saved.runs);
    git(&l, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let main = mainline::Mainline::read(&l.0).unwrap();
    assert!(!main.accepts(main.stable_since_ms - 1, main.stable_since_ms + 100));
    // 오래된 캐시 판은 전혀 읽지 않는다.
    fixture(&l);
    assert!(collect(&Lab::new().0, &l.0, "ws", &[]).unwrap().runs.is_empty());
}

fn archive_fixture(l: &Lab, name: &str, commit: &str, outcome: &str) {
    let tree = git(l, &["rev-parse", &format!("{commit}^{{tree}}")]);
    let remote = mainline::remote_id("https://example.com/team/app.git").unwrap();
    let dir = format!(".git/aaa-workspace-runs/run-{name}");
    let data = serde_json::json!({"format":1,"commit":commit,"tree":tree,"remote":remote,"clean":true,
        "started_at_ms":1000000000000u64,"ended_at_ms":1000000001000u64,"artifact_bytes":1234});
    l.put(&format!("{dir}/provenance.json"), &data.to_string());
    l.put(&format!("{dir}/workspace-summary.tsv"), &format!("v1|1000000000|1|{outcome}\nbuild|ok|0|1|0.25|unused\n"));
}

#[test]
fn ci_archive_waits_for_main_then_survives_stale_checkout_deleted_sources_and_recollection() {
    let l = Lab::new();
    let state = Lab::new();
    main_repo(&l);
    let old = git(&l, &["rev-parse", "HEAD"]);
    git(&l, &["checkout", "-qb", "landing"]);
    l.put("src/app.ts", "export const n = 2;\nexport const second = true;\n");
    git(&l, &["commit", "-qam", "landing tree"]);
    let pushed = git(&l, &["rev-parse", "HEAD"]);
    archive_fixture(&l, "success", &pushed, "succeeded");
    archive_fixture(&l, "failure", &pushed, "failed");
    git(&l, &["checkout", "-q", "main"]);
    let before = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert!(before.runs.is_empty());
    assert!(before.notes.iter().any(|n| n.contains("반영 대기 CI 실행 2개")));
    git(&l, &["update-ref", "refs/remotes/origin/main", &pushed]);
    assert_eq!(git(&l, &["rev-parse", "HEAD"]), old, "등록 checkout은 옛 main 그대로");
    let after = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(after.runs.len(), 2, "push 전의 부정 결과를 캐시하지 않는다");
    assert!(after.runs.iter().all(|r| r.commit == pushed && r.build_ms == Some(250) && r.artifact_bytes == Some(1234)));
    assert!(after.runs.iter().any(|r| r.outcome == "failed"));
    assert_eq!(after.code_lines.unwrap().total_lines, 2, "코드는 origin/main 객체 기준");
    fs::remove_dir_all(l.0.join(".git/aaa-workspace-runs/run-success")).unwrap();
    fs::remove_dir_all(l.0.join(".git/aaa-workspace-runs/run-failure")).unwrap();
    assert_eq!(collect(&state.0, &l.0, "ws", &[]).unwrap().runs, after.runs);
}

#[test]
fn ci_archive_excludes_side_branch_even_after_merge_and_reports_bad_provenance() {
    let l = Lab::new();
    main_repo(&l);
    git(&l, &["checkout", "-qb", "candidate"]);
    l.put("src/new.ts", "export const candidate = 1;\n");
    git(&l, &["add", "src"]);
    git(&l, &["commit", "-qm", "candidate"]);
    let candidate = git(&l, &["rev-parse", "HEAD"]);
    archive_fixture(&l, "candidate", &candidate, "succeeded");
    git(&l, &["checkout", "-q", "main"]);
    l.put("src/main.ts", "export const main = 1;\n");
    git(&l, &["add", "src"]);
    git(&l, &["commit", "-qm", "main changed since candidate verification"]);
    git(&l, &["merge", "--no-ff", "-qm", "landing", "candidate"]);
    let main = git(&l, &["rev-parse", "HEAD"]);
    git(&l, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    archive_fixture(&l, "landing", &main, "succeeded");
    archive_fixture(&l, "wrong-tree", &main, "succeeded");
    let path = l.0.join(".git/aaa-workspace-runs/run-wrong-tree/provenance.json");
    let mut source: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    source["tree"] = serde_json::json!("0".repeat(40));
    fs::write(path, source.to_string()).unwrap();
    let got = collect(&Lab::new().0, &l.0, "ws", &[]).unwrap();
    assert_eq!(got.runs.len(), 1);
    assert_eq!(got.runs[0].commit, main);
    assert!(got.notes.iter().any(|n| n.contains("tree 불일치")));
    assert!(got.notes.iter().any(|n| n.contains("반영 대기 CI 실행 1개")));
}

#[test]
fn verified_second_parent_is_attributed_to_same_tree_main_merge_for_every_runner() {
    let l = Lab::new();
    let state = Lab::new();
    main_repo(&l);
    git(&l, &["checkout", "-qb", "candidate"]);
    l.put("src/new.ts", "export const candidate = 1;\n");
    git(&l, &["add", "src"]);
    git(&l, &["commit", "-qm", "verified candidate"]);
    let candidate = git(&l, &["rev-parse", "HEAD"]);
    let tree = git(&l, &["rev-parse", "HEAD^{tree}"]);
    for runner in ["ci-local", "rc", "engine", "pt"] {
        for outcome in ["succeeded", "failed"] {
            let name = format!("{runner}-{outcome}");
            archive_fixture(&l, &name, &candidate, outcome);
            let path = l.0.join(format!(".git/aaa-workspace-runs/run-{name}/provenance.json"));
            let mut source: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            if runner != "ci-local" {
                source["format"] = serde_json::json!(2);
                source["runner"] = serde_json::json!(runner);
            }
            fs::write(path, source.to_string()).unwrap();
        }
    }
    // 같은 tree의 후보 조상과 관계없는 형제 커밋까지 확대하지 않는다.
    let unrelated = git(&l, &["commit-tree", &tree, "-m", "unrelated same tree"]);
    archive_fixture(&l, "unrelated", &unrelated, "succeeded");
    git(&l, &["commit", "--allow-empty", "-qm", "unverified candidate tip"]);
    let tip = git(&l, &["rev-parse", "HEAD"]);
    git(&l, &["checkout", "-q", "main"]);
    git(&l, &["merge", "--no-ff", "-qm", "pending merge", "candidate"]);
    assert!(collect(&state.0, &l.0, "ws", &[]).unwrap().runs.is_empty(), "미push 병합은 대기한다");
    git(&l, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    assert!(collect(&state.0, &l.0, "ws", &[]).unwrap().runs.is_empty(), "검증한 조상은 직접 둘째 부모가 아니다");
    // 실제 두 부모 병합을 구성한다. 등록 checkout은 앞 병합에 그대로 둔다.
    let base = git(&l, &["rev-parse", "main^1"]);
    let landing = git(&l, &["commit-tree", &tree, "-p", &base, "-p", &candidate, "-m", "landing verified tip"]);
    git(&l, &["update-ref", "refs/remotes/origin/main", &landing]);
    let got = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(got.runs.len(), 8);
    assert!(got.runs.iter().all(|r| r.commit == landing && r.verification_ms == Some(1000)));
    for run in &got.runs {
        assert!(run.notes.iter().any(|n| n.contains(&candidate) && n.contains(&tree) && n.contains(&landing)));
    }
    assert!(got.notes.iter().any(|n| n.contains("반영 대기 CI 실행 1개")));
    assert_ne!(tip, candidate);
    assert_eq!(collect(&state.0, &l.0, "ws", &[]).unwrap().runs, got.runs, "같은 run ID로 재회수한다");
    fs::remove_dir_all(l.0.join(".git/aaa-workspace-runs")).unwrap();
    // archive root 자체가 남아 있으면 원본 정리 뒤에도 확정 캐시를 읽는다.
    fs::create_dir(l.0.join(".git/aaa-workspace-runs")).unwrap();
    assert_eq!(collect(&state.0, &l.0, "ws", &[]).unwrap().runs, got.runs);
}

#[test]
fn new_ci_provenance_cannot_fall_back_to_legacy_and_archive_backlog_advances() {
    let l = Lab::new();
    let state = Lab::new();
    fixture(&l);
    main_repo(&l);
    l.put("target/ci-local/run/workspace-source.json", "{}");
    assert!(
        collect(&state.0, &l.0, "ws", &[]).unwrap().runs.is_empty(),
        "새 runner 출처 오류를 시각 추정으로 우회하지 않는다"
    );
    let commit = git(&l, &["rev-parse", "HEAD"]);
    for i in 0..33 {
        archive_fixture(&l, &format!("backlog-{i}"), &commit, "succeeded");
    }
    let first = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(first.runs.len(), 32);
    assert!(first.notes.iter().any(|n| n.contains("새 종료 입력 32개")));
    let next = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(next.runs.len(), 33, "이미 수집한 최신 입력이 다음 회수의 예산을 차지하지 않는다");
}

#[test]
fn verification_archive_supports_game_runners_on_unchanged_main_with_fresh_runtime_details() {
    let l = Lab::new();
    let state = Lab::new();
    main_repo(&l);
    let commit = git(&l, &["rev-parse", "HEAD"]);
    for runner in ["rc", "engine", "pt"] {
        for code in ["success", "failure"] {
            let name = format!("{runner}-{code}");
            archive_fixture(&l, &name, &commit, if code == "success" { "succeeded" } else { "failed" });
            let dir = format!(".git/aaa-workspace-runs/run-{name}");
            let path = l.0.join(&dir).join("provenance.json");
            let mut source: serde_json::Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
            source["format"] = serde_json::json!(2);
            source["runner"] = serde_json::json!(runner);
            source["mode"] = serde_json::json!("coverage");
            source["scope"] = serde_json::json!("full");
            let now = aaa_common::util::now_ms();
            source["started_at_ms"] = serde_json::json!(now - 1000);
            source["ended_at_ms"] = serde_json::json!(now + 1000);
            fs::write(path, source.to_string()).unwrap();
            for suite in ["default-tests", "workspace-tests", "automation-tests", "2d-tests", "3d-tests", "pt-tests"] {
                l.put(&format!("{dir}/verification/{suite}/report.json"),
                    r#"[{"target":"game","exit":0,"tests":[{"name":"unit","event":"ok","exec_time":0.25}],"suites":[{"passed":1,"failed":0,"ignored":0}]}]"#);
            }
            let mut e2e = serde_json::json!({"status":"passed","reports":[{"scenario":"smoke","duration_ms":30,"status":"passed"}]});
            if runner == "pt" {
                e2e["timing_unit"] = serde_json::json!("scenario-wall");
            }
            l.put(&format!("{dir}/e2e.json"), &e2e.to_string());
        }
    }
    let got = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(got.runs.len(), 6);
    for r in &got.runs {
        assert_eq!(r.commit, commit);
        assert!(r.notes.iter().any(|n| n.contains("coverage")));
        assert!(r.notes.iter().any(|n| n.contains("full")));
        let pt = r.source.starts_with("pt ");
        assert_eq!(
            r.tests.as_ref().unwrap().passed,
            if pt {
                1
            } else if r.source.starts_with("rc ") {
                2
            } else {
                4
            }
        );
        assert_eq!(r.slow_tests.items[0].duration_ms, 250);
        assert_eq!(r.e2e_files, Some(1));
        assert_eq!(r.slow_e2e.items[0].duration_ms, 30);
        assert_eq!(r.slow_e2e.unit, if pt { "scenario-wall" } else { "scenario-step-sum" });
    }
    assert_eq!(collect(&state.0, &l.0, "ws", &[]).unwrap().runs, got.runs);
}

#[test]
fn pt_actual_writer_preserves_failed_test_and_headless_results_through_collection() {
    let l = Lab::new();
    let state = Lab::new();
    main_repo(&l);
    let writer = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/workspace_record.py");
    let invoke = |args: &[&str]| {
        let output = std::process::Command::new("python3").arg(&writer).args(args).current_dir(&l.0).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    };
    let token = invoke(&["start", "pt", "default", "desktop"]);
    l.put("target/verification/pt-tests/report.json", r#"[{"target":"game","exit":7,"tests":[{"name":"ok","event":"ok","exec_time":0.125},{"name":"bad","event":"failed","exec_time":0.25},{"name":"skip","event":"ignored"}],"suites":[{"passed":1,"failed":1,"ignored":1,"filtered_out":0}]}]"#);
    l.put("target/pt-e2e/report.json", r#"{"status":"failed","timing_unit":"scenario-wall","reports":[{"scenario":"console-replay","status":"passed","duration_ms":12},{"scenario":"settings","status":"failed","duration_ms":34}]}"#);
    invoke(&["finish", &token, "7"]);
    let got = collect(&state.0, &l.0, "ws", &[]).unwrap();
    let run = got.runs.first().unwrap();
    assert_eq!(got.runs.len(), 1);
    assert_eq!(run.outcome, "failed");
    let counts = run.tests.as_ref().unwrap();
    assert_eq!((counts.passed, counts.failed, counts.skipped), (1, 1, 1));
    assert_eq!(run.slow_tests.items[0].duration_ms, 250);
    assert!(!run.slow_tests.complete);
    assert_eq!(run.e2e_files, Some(2));
    assert_eq!(run.slow_e2e.unit, "scenario-wall");
    assert_eq!(run.slow_e2e.items[0].name, "settings");
    assert_eq!(run.slow_e2e.items[0].duration_ms, 34);
    assert!(!run.slow_e2e.complete);
    assert!(run.profile.is_none());
}

#[test]
fn upgraded_archive_parser_revisits_old_errors_and_preserves_records_with_deleted_sources() {
    let l = Lab::new();
    let state = Lab::new();
    main_repo(&l);
    let commit = git(&l, &["rev-parse", "HEAD"]);
    archive_fixture(&l, "typed", &commit, "succeeded");
    let source = l.0.join(".git/aaa-workspace-runs/run-typed/provenance.json");
    let mut provenance: serde_json::Value = serde_json::from_str(&fs::read_to_string(&source).unwrap()).unwrap();
    provenance["format"] = serde_json::json!(2);
    provenance["runner"] = serde_json::json!("pt");
    fs::write(&source, provenance.to_string()).unwrap();
    let (old_version, _) = version(&source).unwrap();
    let remote = mainline::remote_id("https://example.com/team/app.git").unwrap();
    let common = git(&l, &["rev-parse", "--path-format=absolute", "--git-common-dir"]);
    let key = hex::encode(Sha256::digest(format!("{remote}:{common}:archive").as_bytes()));
    let store = state.0.join("workspace-metrics-v2").join(key);
    fs::create_dir_all(&store).unwrap();
    let cached = MetricRun { commit, run_id: "deleted-source".into(), ..Default::default() };
    let cache = Cache {
        versions: BTreeMap::from([("run-typed".into(), format!("parser-2:{old_version}"))]),
        errors: BTreeMap::from([("run-typed".into(), "CI 실행 출처 불일치".into())]),
        runs: BTreeMap::from([("deleted-source".into(), cached)]),
    };
    fs::write(store.join("runs.json"), serde_json::to_vec(&cache).unwrap()).unwrap();
    let result = collect(&state.0, &l.0, "ws", &[]).unwrap();
    assert_eq!(result.runs.len(), 2);
    assert!(result.runs.iter().any(|r| r.source.starts_with("pt 종료 원장")));
    assert!(result.runs.iter().any(|r| r.run_id == "deleted-source"));
    assert!(!result.notes.iter().any(|n| n.contains("출처 불일치")));
}
