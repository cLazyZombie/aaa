//! PR 조회 실패의 소유 머신·인증 진단과 프로젝트 전체 회복 경계.
use super::*;
use aaa_common::child_process::StdCommandExt;

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
  exit "${GH_FAILURE_CODE:-1}"
fi
# 실제 인증 값을 읽지 않고 자식에 전달된 시험용 설정 위치만 확인한다.
[ "$GH_CONFIG_DIR" = "$GH_STUB/config" ] || exit 1
printf '%s\n' "$PWD" >> "$GH_STUB/cwds"
case "$1 $2" in
  "pr view") cat "$GH_STUB/view.json" ;;
  "api user") echo thread-bot ;;
  *) echo '[]' ;;
esac
"#,
    )
    .unwrap();
    // 부모의 쓰기 fd가 병렬 fork로 넘어가지 않게 별도 복사 프로세스를 쓴다.
    assert!(Command::new("cp").arg(source).arg(root.join("gh/bin/gh")).status_guarded().unwrap().success());
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
        let branch = w.show(&id)["branch"].as_str().unwrap().to_string();
        git(&w.repo, &["update-ref", &format!("refs/remotes/origin/{branch}"), &branch]);
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
    assert!(text.contains(&ids[0]) && text.contains("labA") && text.contains("요청 실패"), "{text}");
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

/// 공개 전 branch는 조회하지 않고, 공개된 branch만 찾는다. sentinel의 다음 호출은 앞 pass 완료의 울타리다.
#[test]
fn pr_unpublished_branch_never_calls_gh() {
    let w = world_setup("tick_secs=1\npr_poll_secs=1\noutage_secs=0\n", false, environment);
    git(&w.repo, &["remote", "set-url", "origin", "https://github.com/o/app.git"]);
    let mut branches = Vec::new();
    let mut ids = Vec::new();
    for title in ["local", "published"] {
        let task = w.task(&format!("{title}.md"), "PR 조회 조건");
        let t = w.start(&["--title", title, "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
        let id = t["id"].as_str().unwrap();
        w.wait_show("보고 수신", id, |t| !t["report_hash"].is_null());
        branches.push(w.show(id)["branch"].as_str().unwrap().to_string());
        ids.push(id.to_string());
    }
    git(&w.repo, &["update-ref", &format!("refs/remotes/origin/{}", branches[1]), &branches[1]]);
    let attempts = || std::fs::read_to_string(w.lab.root.join("gh/attempts")).unwrap_or_default();
    wait_for("공개 branch 세 번 조회", T, || {
        (attempts().lines().filter(|l| l.ends_with(&branches[1])).count() >= 3).then_some(())
    });
    assert!(
        !attempts().lines().any(|l| l.ends_with(&branches[0])),
        "게시하지 않은 branch에서 gh를 불렀다: {}",
        attempts()
    );
    // 보고가 PR을 지정하면 원격 branch 참조 없이도 계속 추적한다.
    let url = "https://github.com/o/app/pull/7";
    std::fs::write(w.lab.root.join("gh/view.json"), pr_view_json(&branches[0], "OPEN", "", "abc")).unwrap();
    let work = PathBuf::from(w.show(&ids[0])["work_dir"].as_str().unwrap());
    std::fs::write(work.join("report.md"), format!("[진행 중] PR 추적\nPR: {url}\n\n## Report\n진행\n")).unwrap();
    w.wait_show("보고 주소 PR 추적", &ids[0], |t| t["pr"]["url"] == url && t["pr"]["state"] == "OPEN");
}

#[test]
fn pr_unauthenticated_machine_is_quiet_and_resumes() {
    let w = world_setup("tick_secs=1\npr_poll_secs=1\noutage_secs=0\n", false, |root| {
        let mut env = environment(root);
        env.push(("GH_FAILURE_CODE".into(), "4".into()));
        env
    });
    git(&w.repo, &["remote", "set-url", "origin", "https://github.com/o/app.git"]);
    // 앞의 미게시 thread가 지난 pass의 사용 불가 상태를 다시 고정해서는 안 된다.
    let local_task = w.task("local.md", "미게시 thread");
    let local =
        w.start(&["--title", "local", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &local_task]);
    w.wait_show("앞 thread 보고", local["id"].as_str().unwrap(), |t| !t["report_hash"].is_null());
    let task = w.task("pr.md", "인증 없는 머신은 조용히 건너뛴다");
    let t = w.start(&["--title", "auth", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
    let id = t["id"].as_str().unwrap();
    w.wait_show("보고 수신", id, |t| !t["report_hash"].is_null());
    let branch = w.show(id)["branch"].as_str().unwrap().to_string();
    let stub = w.lab.root.join("gh");
    std::fs::write(stub.join("fail"), &branch).unwrap();
    git(&w.repo, &["update-ref", &format!("refs/remotes/origin/{branch}"), &branch]);
    let attempts = || std::fs::read_to_string(stub.join("attempts")).unwrap_or_default();
    wait_for("인증 실패 세 pass 완료", T, || {
        (attempts().lines().filter(|l| l.starts_with("fail ")).count() >= 4).then_some(())
    });
    let items = w.h.ok(&["inbox", "list", "demo", "--peek", "--all"]);
    assert!(
        !items["items"].as_array().unwrap().iter().any(|i| i["subject"] == "gh"),
        "인증 없는 머신은 알리지 않는다: {items}"
    );
    let state = || w.h.ok(&["machine", "list"])["machines"][0]["pr_tracking"].as_str().unwrap_or_default().to_string();
    assert!(state().contains("사용 불가") && state().contains("인증"), "{}", state());
    std::fs::remove_file(stub.join("fail")).unwrap();
    wait_for("로그인 뒤 자동 회복", T, || (state() == "사용 가능").then_some(()));
    let items = w.h.ok(&["inbox", "list", "demo", "--peek", "--all"]);
    assert!(
        !items["items"].as_array().unwrap().iter().any(|i| i["subject"] == "gh"),
        "사용 재개도 outage가 아니다: {items}"
    );
}

#[test]
fn pr_missing_executable_is_quiet() {
    let w = world_setup("tick_secs=1\npr_poll_secs=1\noutage_secs=0\n", false, |root| {
        let env = environment(root);
        // resolver가 찾은 파일도 interpreter/실행 파일이 사라지면 ENOENT다. 운영 gh·PATH는 건드리지 않는다.
        let source = root.join("gh/source-missing");
        std::fs::write(&source, "#!/aaa-missing-interpreter\n").unwrap();
        assert!(Command::new("cp").arg(source).arg(root.join("gh/bin/gh")).status_guarded().unwrap().success());
        env
    });
    git(&w.repo, &["remote", "set-url", "origin", "https://github.com/o/app.git"]);
    let task = w.task("missing.md", "gh를 실행할 수 없으면 조용히 건너뛴다");
    let t = w.start(&["--title", "missing", "--profile", "fake", "--repo", path_str(&w.repo), "--task-file", &task]);
    let id = t["id"].as_str().unwrap();
    let branch = w.show(id)["branch"].as_str().unwrap().to_string();
    git(&w.repo, &["update-ref", &format!("refs/remotes/origin/{branch}"), &branch]);
    wait_for("gh 없음 상태", T, || {
        let v = w.h.ok(&["machine", "list"]);
        v["machines"][0]["pr_tracking"]
            .as_str()
            .filter(|s| s.contains("사용 불가") && s.contains("실행 파일 없음"))
            .map(str::to_string)
    });
    w.h.ticks(2);
    let items = w.h.ok(&["inbox", "list", "demo", "--peek", "--all"]);
    assert!(
        !items["items"].as_array().unwrap().iter().any(|i| i["subject"] == "gh"),
        "gh 없음은 알리지 않는다: {items}"
    );
}
