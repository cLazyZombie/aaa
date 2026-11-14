use super::*;

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
