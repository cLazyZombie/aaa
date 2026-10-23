//! PR 없는 직접 병합의 실제 host·agent·Git·할 일 보관 경계.
use super::*;

#[test]
fn main_ancestor_archives_landing_without_asking_but_keeps_unmerged_and_missing_reports() {
    let mut w = world_with("tick_secs=3600\npr_poll_secs=3600\n");
    w.h.ok(&["project", "set", "demo", "repos.add", &format!("{}@labA", path_str(&w.repo))]);
    w.h.ok(&["machine", "workspaces", "labA", "--allow", "repo"]);
    let mut events = w.lab.follow("follow", &w.h, "demo");
    let board = w.projects.join("demo/TASKS.md");
    std::fs::write(&board, "## Backlog\n- [ ] 병합됨\n- [ ] 미병합\n- [ ] 보고 커밋\n- [ ] 중단\n").unwrap();
    let mut cases = Vec::new();
    for title in ["병합됨", "보고 커밋", "중단", "미병합"] {
        let started = w.start(&["--from-task", title, "--link-task", "--profile", "fake", "--repo", path_str(&w.repo)]);
        let id = started["id"].as_str().unwrap().to_string();
        events.kind("report_ready", &id, T);
        let rec = w.show(&id);
        let cwd = PathBuf::from(rec["cwd"].as_str().unwrap());
        let work = PathBuf::from(rec["work_dir"].as_str().unwrap());
        let branch = rec["branch"].as_str().unwrap().to_string();
        // 실제 thread branch에 서로 다른 커밋을 만든다.
        git(&cwd, &["commit", "--allow-empty", "-qm", title]);
        if title != "미병합" {
            git(&w.repo, &["merge", "--no-ff", "-m", "병합: 시험", &branch]);
            git(&w.repo, &["push", "-q", "origin", "main"]);
        } else {
            // 로컬 main에만 합쳤다. push 없는 병합은 완료 근거가 아니다.
            git(&w.repo, &["merge", "--no-ff", "-m", "push 안 한 병합", &branch]);
        }
        // stale origin/main으로는 확인할 수 없어야 한다. 실제 원격 head를 읽는다.
        let base = git(&w.repo, &["rev-list", "--max-parents=0", "HEAD"]);
        git(&w.repo, &["update-ref", "refs/remotes/origin/main", &base]);
        let report = if title == "보고 커밋" {
            // branch tip은 미병합이어도 보고가 명시한 병합 커밋으로 확인한다.
            git(&cwd, &["commit", "--allow-empty", "-qm", "남은 로컬 커밋"]);
            format!(
                "[착지] 직접 병합\n## Report\n- 병합 커밋: `{}`\n## Next\n",
                &git(&w.repo, &["rev-parse", "HEAD"])[..10]
            )
        } else {
            "[착지] 직접 병합\n## Report\n병합 확인\n## Next\n".into()
        };
        std::fs::write(work.join("report.md"), &report).unwrap();
        let mut args = vec!["thread", "resolve", "demo", &id, "--keep-worktree"];
        if title == "중단" {
            args.push("--skip-copy");
        }
        let before = repo_state(&w.repo);
        w.h.ok(&args);
        assert_eq!(repo_state(&w.repo), before, "병합 조회는 로컬 ref/FETCH_HEAD를 바꾸지 않는다");
        // 종료 응답 직후부터 완료 보관까지 확인 카드가 잠깐 생기는 것도 막는다.
        if matches!(title, "병합됨" | "보고 커밋") {
            let tasks = w.h.ok(&["task", "list"]);
            let task = tasks["tasks"].as_array().unwrap().iter().find(|t| t["thread"] == id);
            assert!(task.is_none_or(|t| t["attention"].is_null()), "{tasks}");
        }
        cases.push((title, id));
    }
    w.h.ticks(1);
    let archived = w.h.ok(&["task", "archive"]);
    let titles: Vec<_> = archived["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            assert_eq!(e["reason"], "origin/main 병합");
            e["task"]["title"].as_str().unwrap()
        })
        .collect();
    assert_eq!(titles.len(), 2, "{archived}");
    assert!(titles.contains(&"병합됨") && titles.contains(&"보고 커밋"), "{archived}");
    assert!(!titles.contains(&"미병합") && !titles.contains(&"중단"), "{archived}");
    let inbox = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"]);
    for (title, id) in &cases {
        let notices: Vec<_> = inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == "task" && i["subject"] == *id)
            .collect();
        let wanted = if matches!(*title, "미병합" | "중단") { "task needs check" } else { "task auto-cleaned" };
        assert_eq!(notices.len(), 1, "{title}: {notices:?}");
        assert_eq!(notices[0]["event"], wanted, "{title}: {notices:?}");
    }
    // 보고를 만들지 않는 thread는 branch가 main 조상이어도 확인 필요다.
    let no_report = w.start(&[
        "--title",
        "보고 없는 병합",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("no-report.md", "noreport"),
    ]);
    let id = no_report["id"].as_str().unwrap();
    w.wait_show("보고 없이 brief 받음", id, |v| v["brief"] == "delivered");
    w.h.ok(&["task", "add", "진짜 보고 없음", "--thread", id]);
    w.h.ok(&["thread", "resolve", "demo", id, "--keep-worktree"]);
    w.h.ticks(1);
    let tasks = w.h.ok(&["task", "list"]);
    let kept = tasks["tasks"].as_array().unwrap().iter().find(|t| t["title"] == "진짜 보고 없음").unwrap();
    assert!(kept["attention"].as_str().unwrap().contains("보고가 없다"), "{kept}");
    // 완료 판정은 한 번: 뒤 tick에도 중복 보관/알림이 없다.
    w.h.ticks(1);
    assert_eq!(w.h.ok(&["task", "archive"]), archived);
}

fn repo_state(repo: &Path) -> (String, String, Option<Vec<u8>>) {
    (
        git(repo, &["rev-parse", "main"]),
        git(repo, &["rev-parse", "origin/main"]),
        std::fs::read(repo.join(".git/FETCH_HEAD")).ok(),
    )
}
