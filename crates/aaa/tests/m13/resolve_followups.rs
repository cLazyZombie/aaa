use super::*;

/// 이전 보고·안내 예시의 Next는 후속이 아니며, 실제 Next의 CLI 종료 차단은 유지한다.
#[test]
fn cli_resolve_ignores_fenced_examples_but_requires_real_followups() {
    let w = report_shell_world();
    let task = w.task("fenced-next.txt", "noreport");
    let started = w.start(&["--title", "보고 예시", "--profile", "fake", "--kind", "dir", "--task-file", &task]);
    let id = started["id"].as_str().unwrap();
    w.wait_show("brief 받음", id, |v| v["brief"] == "delivered");
    let work = Path::new(started["work_dir"].as_str().unwrap());
    let report = "[완료] 실제 결과\n## Report\n예전 보고:\n````markdown\n## Next\n- 이어 하기\n```\n````\n## Next\n- 결과를 검토한다\n~~~sh\n## Next\n- 예시 명령\n~~~\n";
    std::fs::write(work.join("report.md.tmp"), report).unwrap();
    std::fs::rename(work.join("report.md.tmp"), work.join("report.md")).unwrap();
    let view = w.wait_show("보고 수집", id, |v| v["report_hash"].is_string());
    assert_eq!(view["next"], serde_json::json!(["결과를 검토한다"]));
    assert_eq!(view["followups"].as_array().unwrap().len(), 1, "{view}");
    assert_eq!(view["followups"][0]["line"], "결과를 검토한다");
    let error = w.h.json(&["thread", "resolve", id]).expect_err("실제 후속은 계속 차단한다");
    assert!(error.contains("결과를 검토한다"), "{error}");
    assert!(!error.contains("이어 하기") && !error.contains("예시 명령") && !error.contains("```"), "{error}");
    assert_eq!(w.show(id)["status"], "open");
    w.finish_fixture_followups(id);
    assert_eq!(w.h.ok(&["thread", "resolve", id])["status"], "resolved");
}

/// 실제 CLI의 오류 종료가 pane·Git·할 일을 보존하고 긴 ID 연결 뒤에만 resolve한다.
#[test]
fn cli_resolve_requires_followups_before_changing_the_thread() {
    let w = report_shell_world();
    let task = w.task("followup.txt", "dirty 후속 확인");
    let started =
        w.start(&["--title", "후속 보호", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
    let id = started["id"].as_str().unwrap();
    let pane = started["pane"].as_str().unwrap();
    let cwd = PathBuf::from(started["cwd"].as_str().unwrap());
    let view = w.wait_show("완료 보고와 후속", id, |v| {
        v["brief"] == "delivered" && v["followups"].as_array().is_some_and(|f| f.len() == 2)
    });
    let tasks_path = w.projects.join("demo/TASKS.md");
    let tasks = std::fs::read(&tasks_path).unwrap();
    let head = git(&cwd, &["rev-parse", "HEAD"]);
    let dirty = std::fs::read(cwd.join("dirty.txt")).unwrap();
    for extra in [None, Some("--skip-copy")] {
        let mut args = vec!["thread", "resolve", id];
        args.extend(extra);
        let error = w.h.json(&args).expect_err("미처리 후속은 CLI 오류 종료");
        for entry in view["followups"].as_array().unwrap() {
            assert!(error.contains(entry["id"].as_str().unwrap()), "{error}");
            assert!(error.contains(entry["line"].as_str().unwrap()), "{error}");
        }
        assert!(error.contains("--task d-NNNN") && error.contains("--reason-file"), "{error}");
        assert_eq!(w.show(id)["status"], "open");
        assert_eq!(std::fs::read(&tasks_path).unwrap(), tasks);
        assert_eq!(git(&cwd, &["rev-parse", "HEAD"]), head);
        assert_eq!(std::fs::read(cwd.join("dirty.txt")).unwrap(), dirty);
        assert!(w.h.ok(&["pane", "list"])["panes"].as_array().unwrap().iter().any(|p| p["id"] == pane));
    }
    w.h.ticks(1);
    assert!(
        w.h.ok(&["inbox", "list", "demo", "--all"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["event"] != "task needs check")
    );
    w.h.ok(&["task", "add", "후속 구현"]);
    let board = w.h.ok(&["task", "list"]);
    let target = board["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "후속 구현").unwrap();
    let entry = &view["followups"][0];
    let linked = w.h.ok(&[
        "thread",
        "followup",
        id,
        "--id",
        entry["id"].as_str().unwrap(),
        "--task",
        target["id"].as_str().unwrap(),
    ]);
    assert_eq!(linked["followups"][0]["task"], target["short_id"]);
    w.finish_fixture_followups(id);
    assert_eq!(w.h.ok(&["thread", "resolve", id])["status"], "resolved");
}
