//! PR을 제거한 실제 host·agent·보고·정리 경계.
use super::*;

#[test]
fn reports_and_cleanup_never_query_prs_or_resolve_on_main_membership() {
    let (mut w, env) = world_files("tick_secs=3600\noutage_secs=0\n", false, Some("0"), true, |root| {
        let bin = root.join("gh-trap");
        std::fs::create_dir_all(&bin).unwrap();
        let gh = bin.join("gh");
        std::fs::write(&gh, "#!/bin/sh\necho called >> \"$GH_CALL_LOG\"\nexit 1\n").unwrap();
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        vec![
            ("PATH".into(), format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default())),
            ("GH_CALL_LOG".into(), root.join("gh-calls").display().to_string()),
        ]
    });
    w.host_env.push(("AAA_TEST_SUBMIT_GAP_MS".into(), "0".into()));
    let w = world_start(w, env);
    let dir = w.projects.join("demo");
    assert!(!dir.join("automations/pr-followup.toml").exists());
    std::fs::write(dir.join("routines/old-pr.md"), "+++\non='pr'\nenabled=true\n+++\n옛 규칙\n").unwrap();
    let task = w.task("no-report.md", "noreport");
    let t = w.start(&["--title", "보고 확인", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
    let id = t["id"].as_str().unwrap();
    let branch = t["branch"].as_str().unwrap();
    let cwd = PathBuf::from(t["cwd"].as_str().unwrap());
    w.wait_show("brief 받음", id, |v| v["brief"] == "delivered");
    let report = "[완료] 확인 대기\nPR: https://github.com/o/app/pull/7\n\n## Report\n사용자 확인 대기\n\n## Next\n";
    std::fs::write(Path::new(t["work_dir"].as_str().unwrap()).join("report.md"), report).unwrap();
    let shown = w.wait_show("보고 회수", id, |v| v["report_hash"].is_string());
    assert!(shown.get("pr").is_none(), "{shown}");
    assert_eq!(shown["status"], "open");
    w.h.ticks(2);
    assert_eq!(w.show(id)["status"], "open", "main에 포함돼도 직접 끝내기 전에는 유지한다");
    let routines = w.h.ok(&["routine", "list"]);
    assert!(routines["routines"].as_array().unwrap().is_empty(), "{routines}");
    let inbox = w.h.ok(&["inbox", "list", "--all", "--peek"]);
    assert!(
        !inbox["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["item_kind"] == "pr" || i["subject"] == "gh" || i["subject"] == "old-pr"),
        "{inbox}"
    );
    w.h.ok(&["thread", "resolve", id]);
    assert!(!cwd.exists());
    assert_eq!(git(&w.repo, &["branch", "--list", branch]), "", "main 조상 branch는 정리한다");
    let unmerged =
        w.start(&["--title", "미병합", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
    let next = unmerged["id"].as_str().unwrap();
    let next_branch = unmerged["branch"].as_str().unwrap();
    let next_cwd = PathBuf::from(unmerged["cwd"].as_str().unwrap());
    w.wait_show("다음 brief 받음", next, |v| v["brief"] == "delivered");
    git(&next_cwd, &["commit", "--allow-empty", "-qm", "아직 main에 없는 작업"]);
    std::fs::write(Path::new(unmerged["work_dir"].as_str().unwrap()).join("report.md"), report).unwrap();
    w.wait_show("다음 보고 회수", next, |v| v["report_hash"].is_string());
    w.h.ok(&["thread", "resolve", next]);
    assert!(!next_cwd.exists());
    assert!(!git(&w.repo, &["branch", "--list", next_branch]).is_empty(), "미병합 branch는 남긴다");
    assert!(!w.lab.root.join("gh-calls").exists(), "보고·tick·정리가 gh를 호출했다");
}
