//! PR 조회 실패의 소유 머신·인증 진단과 프로젝트 전체 회복 경계.
use super::*;

fn environment(root: &Path) -> Vec<(String, String)> {
    let mut env = fake_gh(root);
    let source = root.join("gh/source");
    std::fs::write(
        &source,
        r#"#!/bin/sh
branch=
for arg do case "$arg" in --head=*) branch=${arg#--head=} ;; esac; done
phase=ok
if [ -f "$GH_STUB/fail" ] && [ "$branch" = "$(cat "$GH_STUB/fail")" ]; then phase=fail; fi
printf '%s %s\n' "$phase" "$branch" >> "$GH_STUB/attempts"
if [ "$phase" = fail ]; then
  echo 'To get started with GitHub CLI, please run: gh auth login' >&2
  echo 'Alternatively, populate GH_TOKEN. stderr-private-canary' >&2
  exit 4
fi
# 실제 인증 값을 읽지 않고 자식에 전달된 시험용 설정 위치만 확인한다.
[ "$GH_CONFIG_DIR" = "$GH_STUB/config" ] || exit 1
printf '%s\n' "$PWD" >> "$GH_STUB/cwds"
echo '[]'
"#,
    )
    .unwrap();
    // 부모의 쓰기 fd가 병렬 fork로 넘어가지 않게 별도 복사 프로세스를 쓴다.
    assert!(Command::new("cp").arg(source).arg(root.join("gh/bin/gh")).status().unwrap().success());
    std::fs::set_permissions(root.join("gh/bin/gh"), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    env.push(("GH_CONFIG_DIR".into(), path_str(&root.join("gh/config")).into()));
    env
}

#[test]
fn pr_environment_failure_is_not_recovered_by_another_thread() {
    let mut w = world_setup("tick_secs=1\npr_poll_secs=1\noutage_secs=0\n", false, environment);
    git(&w.repo, &["remote", "set-url", "origin", "https://github.com/o/app.git"]);
    let task = w.task("pr.md", "PR 조회를 확인한다");
    let mut ids = Vec::new();
    for title in ["first", "second"] {
        let t = w.start(&["--title", title, "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
        let id = t["id"].as_str().unwrap().to_string();
        w.wait_show("보고 수신", &id, |t| !t["report_hash"].is_null());
        ids.push(id);
    }
    let stub = w.lab.root.join("gh");
    let branch = w.show(&ids[0])["branch"].as_str().unwrap().to_string();
    std::fs::write(stub.join("fail"), &branch).unwrap();
    let attempts = || std::fs::read_to_string(stub.join("attempts")).unwrap_or_default();
    // 뒤의 pass가 시작되면 앞 pass의 모든 thread 조회와 outage 판정이 끝났다.
    wait_for("실패를 포함한 세 pass 완료", T, || {
        (attempts().lines().filter(|l| l.starts_with("fail ")).count() >= 4).then_some(())
    });
    let items = w.h.ok(&["inbox", "list", "demo", "--peek", "--all"]);
    let gh: Vec<_> = items["items"].as_array().unwrap().iter().filter(|i| i["subject"] == "gh").collect();
    assert_eq!(
        gh.iter().filter(|i| i["event"] == "working again").count(),
        0,
        "다른 thread 성공은 회복이 아니다: {gh:?}"
    );
    let failure = gh.iter().find(|i| i["event"] == "failing").expect("실패 알림");
    let text = failure.to_string();
    assert!(text.contains(&ids[0]) && text.contains("labA") && text.contains("인증"), "{text}");
    assert!(!text.contains("stderr-private-canary"), "stderr를 inbox로 넘기지 않는다");
    let cwds = std::fs::read_to_string(stub.join("cwds")).unwrap();
    assert!(cwds.lines().all(|p| p == "/"), "gh는 worktree에 의존하지 않는다: {cwds}");
    std::fs::remove_file(stub.join("fail")).unwrap();
    let mut flow = w.lab.follow("recovery", &w.h, "demo");
    flow.until("gh 회복", T, |e| e["kind"] == "gh_recovered");
    let items = w.h.ok(&["inbox", "list", "demo", "--peek", "--all"]);
    assert_eq!(
        items["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["subject"] == "gh" && i["event"] == "working again")
            .count(),
        1
    );
    let cwd = PathBuf::from(w.show(&ids[0])["cwd"].as_str().unwrap());
    let moved = cwd.with_extension("moved");
    std::fs::rename(&cwd, &moved).unwrap();
    let event = flow.until("사라진 worktree 진단", T, |e| e["kind"] == "gh_failing");
    let detail = event["detail"].as_str().unwrap();
    assert!(detail.contains(&ids[0]) && detail.contains("폴더"), "{event}");
    assert!(!detail.contains("PATH") && !detail.contains(path_str(&cwd)), "{event}");
    std::fs::rename(moved, cwd).unwrap();
}
