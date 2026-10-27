//! thread 보류(plan §8.426): 정리 맡기기 → `[보류 준비됨]` 보고 → pane 닫기·자리 반환 → 예약 기동. 보류는 restart·말 보내기·
//! auto-resolve에서 빠지고, 같은 머신 이어 하기는 저장한 대화(`--resume`)에 고유한 한 줄을 친다. 다른 머신 이어 하기는 push된
//! branch로 새 worktree와 요약 brief의 새 대화이고, 앞 머신으로 돌아가면 앞 자리에서 origin을 받으라는 brief다.
use super::*;

fn work_dir(w: &World, id: &str) -> PathBuf {
    PathBuf::from(w.show(id)["work_dir"].as_str().unwrap())
}

fn brief(w: &World, id: &str) -> String {
    std::fs::read_to_string(work_dir(w, id).join("brief.md")).unwrap()
}

#[test]
fn account_unpark_keeps_auth_and_history_on_the_same_machine() {
    account_unpark(false);
}

#[test]
fn account_unpark_keeps_auth_when_moving_and_returning() {
    account_unpark(true);
}

fn account_unpark(moving: bool) {
    let mut w = world();
    // 계정 선택·금고·실행 인증 원장·격리 홈은 제품 경로를 쓰고 harness만 대체한다.
    let script = RESUMABLE_AGENT.replace(
        "log=''",
        &r#"[ "$CLAUDE_CODE_OAUTH_TOKEN" = '__PARK_AUTH__' ] || exit 42
[ -n "$CLAUDE_CONFIG_DIR" ] || exit 43
if [ -f "$CLAUDE_CONFIG_DIR/park-history" ]; then history=kept; else history=new; fi
touch "$CLAUDE_CONFIG_DIR/park-history"
log="account-original history-$history
""#
        .replace("__PARK_AUTH__", &aaa_test_support::token("park-original")),
    );
    let script = script.replace("log=\"$1 $2", "log=\"$log$1 $2");
    std::fs::write(w.scripts.join("resumable.sh"), script).unwrap();
    for (name, token) in [
        ("park-original", aaa_test_support::token("park-original")),
        ("park-other", aaa_test_support::token("park-other")),
    ] {
        let file = w.task(&format!("{name}.token"), &token);
        w.h.ok(&["account", "import", name, "--harness", "claude", "--file", &file]);
    }
    w.h.ok(&["account", "default", "claude", "park-original"]);
    if moving {
        w.add_remote();
        let other = w.lab.root.join("b-src");
        std::fs::create_dir_all(&other).unwrap();
        git(&other, &["clone", "-q", path_str(&w.lab.root.join("origin.git")), "repo"]);
        std::fs::write(
            w.projects.join("demo/project.toml"),
            format!(
                "schema = 1\n[[repos]]\npath = {:?}\nmachine = \"labA\"\n[[repos]]\npath = {:?}\nmachine = \"labB\"\n",
                path_str(&w.repo),
                path_str(&other.join("repo"))
            ),
        )
        .unwrap();
    }
    let v = w.start(&[
        "--title",
        "인증 보류",
        "--machine",
        "labA",
        "--profile",
        "resumable",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("account-park.md", "인증 보류 시험"),
    ]);
    let id = v["id"].as_str().unwrap();
    let pane = v["pane"].as_str().unwrap();
    assert_eq!(v["account"], "park-original");
    w.wait_show("인증 brief 전달", id, |v| v["brief"] == "delivered");
    w.h.wait_output(pane, "account-original history-new", T);
    let line = w.h.wait_output(pane, "--session-id", T)["matched_line"].as_str().unwrap().to_string();
    let key = line.rsplit(' ').next().unwrap();
    let cwd = PathBuf::from(v["cwd"].as_str().unwrap());
    git(&cwd, &["push", "-q", "origin", "HEAD"]);
    let parked = w.h.ok(&["thread", "park", "demo", id, "--now"]);
    assert_eq!(parked["status"], "parked");
    assert!(parked["pane"].is_null());
    assert!(w.h.json(&["pane", "get", pane]).is_err());
    // 이어 하기는 새 작업의 기본 계정이 바뀌어도 처음 선택을 유지한다.
    w.h.ok(&["account", "default", "claude", "park-other"]);
    let target = if moving { "labB" } else { "labA" };
    let back = w.h.ok(&["thread", "unpark", "demo", id, "--machine", target]);
    assert_eq!(back["account"], "park-original");
    assert_eq!(back["machine"], target);
    assert!(back["park"].is_null());
    let new = back["pane"].as_str().unwrap();
    assert_ne!(new, pane);
    w.h.wait_output(new, if moving { "--session-id" } else { "--resume" }, T);
    w.h.wait_output(new, if moving { "account-original history-new" } else { "account-original history-kept" }, T);
    if moving {
        assert_ne!(back["cwd"], v["cwd"]);
        assert_eq!(back["branch"], v["branch"]);
        assert!(brief(&w, id).contains("# 보류에서 이어 하기"));
    } else {
        w.h.wait_output(new, &format!("--resume {key}"), T);
        assert_eq!(back["cwd"], v["cwd"]);
    }
    w.wait_show("이어 하기 전달", id, |v| v["brief"] == "delivered");
    if moving {
        w.h.ok(&["thread", "park", "demo", id, "--now"]);
        let home = w.h.ok(&["thread", "unpark", "demo", id, "--machine", "labA"]);
        assert_eq!(home["account"], "park-original");
        assert_eq!(home["cwd"], v["cwd"]);
        w.h.wait_output(home["pane"].as_str().unwrap(), "account-original history-kept", T);
        w.wait_show("돌아온 brief 전달", id, |v| v["brief"] == "delivered");
    }
}

#[test]
fn park_returns_the_slot_and_unpark_resumes_the_same_conversation() {
    let w = world_with("auto_resolve_day_secs = 1\n");
    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "1"]);
    w.h.ok(&["project", "set", "demo", "auto_resolve_days", "1"]);
    let v = w.start(&[
        "--title",
        "검수 남음",
        "--profile",
        "resumable",
        "--kind",
        "dir",
        "--task-file",
        &w.task("park-a.md", "x"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    let pane = v["pane"].as_str().unwrap().to_string();
    w.wait_show("brief 받음", &id, |v| v["brief"] == "delivered");
    let line = w.h.wait_output(&pane, "--session-id", T)["matched_line"].as_str().unwrap().to_string();
    let key = line.rsplit(' ').next().unwrap().to_string();
    // 자리가 찬 동안 기다리는 예약
    w.h.ok(&[
        "task",
        "add",
        "뒤 일",
        "--profile",
        "fake",
        "--machine",
        "labA",
        "--repo",
        "작업 폴더",
        "--note",
        "noreport",
    ]);
    w.h.ok(&["task", "reserve", "뒤 일"]);
    w.h.ticks(1);
    assert!(w.h.ok(&["task", "queue"])["entries"][0]["thread"].is_null());

    // coordinator는 사용자 지시를 인용해야 보류한다(4A)
    let coord = w.h.ok(&["project", "open", "demo", "--profile", "fake"]);
    let cpane = coord["pane"]["id"].as_str().unwrap().to_string();
    w.wait_screen(&cpane, "fake>");
    let mut number = 0;
    let mut probe = |command: &str| {
        number += 1;
        let prefix = w.lab.root.join(format!("park-request-{number}"));
        let script = w.task(
            &format!("park-request-{number}.sh"),
            &format!(
                "'{}' --json {command} > '{p}.out' 2> '{p}.err'\nprintf '%s' \"$?\" > '{p}.rc'\nprintf 'park-request-{number}-finished\\n'\n",
                lab::BIN,
                p = prefix.display()
            ),
        );
        w.h.ok(&["pane", "run", &cpane, &format!("post-probe {script}")]);
        w.h.wait_output(&cpane, &format!("park-request-{number}-finished"), T);
        (
            std::fs::read_to_string(prefix.with_extension("rc")).unwrap(),
            std::fs::read_to_string(prefix.with_extension("err")).unwrap(),
            std::fs::read_to_string(prefix.with_extension("out")).unwrap(),
        )
    };
    let (code, error, _) = probe(&format!("thread park demo {id}"));
    assert_ne!(code, "0");
    assert!(error.contains("--user-request"), "{error}");
    assert!(w.show(&id)["park"].is_null(), "거부한 요청이 기록을 남겼다");
    let reason = "사용자가 채팅에서 검수만 남은 thread를 보류하라고 지시";
    let (code, error, out) = probe(&format!("thread park demo {id} --note '사람 검수 대기' --user-request '{reason}'"));
    assert_eq!(code, "0", "{error}");
    let asked: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(asked["status"], "open");
    assert!(asked["park"]["parked_at_ms"].is_null(), "{asked}");
    assert_eq!(asked["park"]["user_request"], reason);
    assert!(asked["park"]["actor"].as_str().unwrap().starts_with("local:cli@"), "{asked}");
    w.h.wait_output(&pane, "[aaa 보류 준비]", T);
    let again = w.h.json(&["thread", "park", "demo", &id]).unwrap_err();
    assert!(again.contains("이미 정리를 맡겼다"), "{again}");
    w.h.ticks(1);
    assert_eq!(w.show(&id)["status"], "open", "보고 전에는 닫지 않는다");
    assert!(w.h.ok(&["task", "queue"])["entries"][0]["thread"].is_null(), "정리 중에는 자리를 가진다");

    // thread가 정리를 마쳤다: 새 보고 첫 줄 `[보류 준비됨]`
    std::fs::write(
        work_dir(&w, &id).join("report.md"),
        "[보류 준비됨] 정리 끝\n\n## Handoff\n- 다음: 검수\n\n## Next\n- 이어 하기\n",
    )
    .unwrap();
    let parked =
        w.wait_show("보류·pane 정리", &id, |v| v["status"] == "parked" && v["park"]["parked_at_ms"].is_u64());
    assert_eq!(parked["group"], "parked");
    assert_eq!(parked["park"]["resumable"], true, "{parked}");
    assert_eq!(parked["park"]["note"], "사람 검수 대기");
    assert!(parked["park"]["parked_at_ms"].is_u64(), "{parked}");
    assert!(parked["pane"].is_null(), "{parked}");
    let panes = w.h.ok(&["pane", "list"]);
    assert!(
        panes["panes"].as_array().unwrap().iter().all(|p| p["id"] != pane.as_str()),
        "pane을 지우지 않았다: {panes}"
    );
    // 돌려준 자리로 예약이 뜬다
    let next = wait_for("예약 기동", T, || {
        w.h.ok(&["task", "queue"])["entries"][0]["thread"].as_str().map(str::to_string)
    });
    w.wait_show("예약 준비", &next, |v| v["brief"] == "delivered");
    let listed = w.h.run(&["thread", "show", "demo", &id]);
    let text = String::from_utf8_lossy(&listed.stdout);
    assert!(text.contains("\t보류\t") && text.contains("같은 머신은 같은 대화"), "{text}");

    // 보류는 다시 띄우기·말 보내기에서 빠진다. 다시 보류할 수도 없다
    let e = w.h.json(&["thread", "restart", "demo", &id, "--resume"]).unwrap_err();
    assert!(e.contains("thread unpark"), "{e}");
    let e = w.h.json(&["thread", "message", "demo", &id, "--text", "x"]).unwrap_err();
    assert!(e.contains("열린 thread"), "{e}");
    let e = w.h.json(&["thread", "park", "demo", &id, "--now"]).unwrap_err();
    assert!(e.contains("이미 보류"), "{e}");
    // 자리가 차 있으면 이어 하기를 거부하고 보류로 남는다
    let plain = w.h.run(&["thread", "unpark", "demo", &id, "--dry-run"]);
    let text = String::from_utf8_lossy(&plain.stdout);
    assert!(text.contains("labA (보류한 머신)\t같은 대화") && text.contains("실행 한도가 찼다"), "{text}");
    let e = w.h.json(&["thread", "unpark", "demo", &id]).unwrap_err();
    assert!(e.contains("실행 한도"), "{e}");
    let still = w.show(&id);
    assert_eq!(still["status"], "parked", "{still}");
    assert!(still["park"]["parked_at_ms"].is_u64(), "거부가 보류 기록을 지웠다: {still}");
    // fixed wait: auto_resolve_days(시험 하루 = 1초)를 넘긴다. 쉬는 열린 thread는 끝나도 보류는 남는다
    std::thread::sleep(Duration::from_millis(2100));
    w.h.ticks(2);
    assert_eq!(w.show(&id)["status"], "parked", "보류는 auto-resolve하지 않는다");

    w.h.ok(&["project", "set", "demo", "max_parallel_threads", "3"]);
    let back = w.h.ok(&["thread", "unpark", "demo", &id]);
    assert_eq!(back["status"], "open", "{back}");
    assert!(back["park"].is_null(), "{back}");
    let new = back["pane"].as_str().unwrap().to_string();
    assert_ne!(new, pane);
    w.h.wait_output(&new, &format!("--resume {key}"), T);
    w.h.wait_output(&new, &format!("got: 보류를 풀었다({id} 이어 하기)"), T);
    w.wait_show("이은 대화가 받음", &id, |v| v["brief"] == "delivered");
    assert!(!w.h.screen(&new).contains("brief.md"), "{}", w.h.screen(&new));
    let kinds = w.kinds(0);
    for k in ["thread_park_requested", "thread_parked", "thread_unparked"] {
        assert!(kinds.iter().any(|x| x == k), "{k}: {kinds:?}");
    }
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    assert!(
        items
            .iter()
            .any(|i| i["event"] == "parked" && i["summary"].as_str().is_some_and(|s| s.contains("thread unpark"))),
        "{items:?}"
    );
}

#[test]
fn park_moves_between_machines_through_the_pushed_branch() {
    let mut w = world();
    w.add_remote();
    let other = w.lab.root.join("b-src");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["clone", "-q", path_str(&w.lab.root.join("origin.git")), "repo"]);
    let repo_b = other.join("repo");
    std::fs::write(
        w.projects.join("demo/project.toml"),
        format!(
            "schema = 1\nthread_profile = \"fake\"\n\n[[repos]]\npath = {:?}\nmachine = \"labA\"\n\n[[repos]]\npath = {:?}\nmachine = \"labB\"\n",
            path_str(&w.repo),
            path_str(&repo_b)
        ),
    )
    .unwrap();
    let v = w.start(&[
        "--title",
        "조명 검수",
        "--machine",
        "labA",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &w.task("park-b.md", "조명 일"),
    ]);
    let id = v["id"].as_str().unwrap().to_string();
    let branch = v["branch"].as_str().unwrap().to_string();
    let cwd_a = PathBuf::from(v["cwd"].as_str().unwrap());
    w.wait_show("첫 보고", &id, |v| v["report_hash"].is_string() && v["brief"] == "delivered");

    // 커밋하지 않은 채 바로 보류: 다른 머신으로는 못 옮긴다
    std::fs::write(cwd_a.join("wip.txt"), "wip\n").unwrap();
    let parked = w.h.ok(&["thread", "park", "demo", &id, "--now", "--note", "검수 대기"]);
    assert_eq!(parked["status"], "parked", "{parked}");
    assert_eq!(parked["park"]["immediate"], true);
    assert_eq!(parked["park"]["pushed"], false, "{parked}");
    assert_eq!(parked["park"]["uncommitted"], 1, "{parked}");
    assert_eq!(parked["park"]["resumable"], false, "custom harness는 같은 대화를 잇지 못한다");
    let preview = w.h.ok(&["thread", "unpark", "demo", &id, "--dry-run"]);
    let options = preview["options"].as_array().unwrap();
    let b = options.iter().find(|o| o["machine"] == "labB").unwrap();
    assert!(b["blocked"].as_str().unwrap().contains("push된 branch"), "{preview}");
    let a = options.iter().find(|o| o["machine"] == "labA").unwrap();
    assert_eq!((a["same"].as_bool(), a["conversation"].as_str()), (Some(true), Some("new")), "{preview}");
    let e = w.h.json(&["thread", "unpark", "demo", &id, "--machine", "labB"]).unwrap_err();
    assert!(e.contains("push된 branch"), "{e}");

    // 같은 머신: 같은 worktree에서 새 대화와 이어 하기 brief(잇지 못하는 harness)
    let back = w.h.ok(&["thread", "unpark", "demo", &id]);
    assert_eq!((back["status"].as_str(), back["machine"].as_str()), (Some("open"), Some("labA")), "{back}");
    let text = brief(&w, &id);
    assert!(
        text.contains("# 보류에서 이어 하기") && text.contains("검수 대기") && text.contains("같은 worktree"),
        "{text}"
    );
    assert!(text.contains("## 마지막 보고") && text.contains("맡긴 일 끝"), "{text}");
    assert!(cwd_a.join("wip.txt").is_file());
    w.wait_show("다시 받은 brief", &id, |v| v["brief"] == "delivered");

    // 커밋·push 뒤 정리 맡기기: 가짜 agent가 Handoff 보고를 쓰면 host가 닫는다
    git(&cwd_a, &["add", "wip.txt"]);
    git(&cwd_a, &["commit", "-qm", "wip"]);
    git(&cwd_a, &["push", "-q", "origin", "HEAD"]);
    let head = git(&cwd_a, &["rev-parse", "HEAD"]);
    w.h.ok(&["thread", "park", "demo", &id]);
    let parked =
        w.wait_show("정리 뒤 보류", &id, |v| v["status"] == "parked" && v["park"]["parked_at_ms"].is_u64());
    assert_eq!(parked["park"]["pushed"], true, "{parked}");
    assert_eq!(parked["park"]["uncommitted"], 0, "{parked}");
    assert_eq!(parked["park"]["head"], head.as_str());
    let copied = std::fs::read_to_string(w.projects.join(format!("demo/threads/{id}.md"))).unwrap();
    assert!(copied.starts_with("[보류 준비됨]") && copied.contains("HANDOFF-7"), "마지막 사본: {copied}");
    let preview = w.h.ok(&["thread", "unpark", "demo", &id, "--dry-run"]);
    let b = preview["options"].as_array().unwrap().iter().find(|o| o["machine"] == "labB").unwrap().clone();
    assert!(b["blocked"].is_null(), "{preview}");
    assert_eq!(b["repo"].as_str(), Some(path_str(&repo_b)));

    // 다른 머신: push된 branch로 새 worktree, 할 일·마지막 보고를 담은 새 대화
    let moved = w.h.ok(&["thread", "unpark", "demo", &id, "--machine", "labB"]);
    assert_eq!(moved["machine"], "labB", "{moved}");
    assert_eq!(moved["branch"], branch.as_str());
    let cwd_b = PathBuf::from(moved["cwd"].as_str().unwrap());
    assert!(cwd_b.starts_with(w.lab.root.join("workB")), "{}", cwd_b.display());
    assert_eq!(git(&cwd_b, &["rev-parse", "HEAD"]), head);
    assert!(cwd_b.join("wip.txt").is_file());
    let text = brief(&w, &id);
    assert!(text.contains("새 worktree") && text.contains("HANDOFF-7") && text.contains(&branch), "{text}");
    assert!(cwd_a.join("wip.txt").is_file(), "앞 머신 worktree를 지웠다");
    let done = w.wait_show("labB의 새 보고", &id, |v| {
        v["brief"] == "delivered" && v["report_hash"] != parked["report_hash"]
    });
    assert_eq!(done["status"], "open");

    // 앞 머신으로 돌아간다: 앞 자리에서 origin을 받으라는 brief
    let parked_b = w.h.ok(&["thread", "park", "demo", &id, "--now"]);
    assert_eq!(parked_b["park"]["pushed"], true, "{parked_b}");
    let home = w.h.ok(&["thread", "unpark", "demo", &id, "--machine", "labA"]);
    assert_eq!((home["machine"].as_str(), home["cwd"].as_str()), (Some("labA"), cwd_a.to_str()), "{home}");
    let text = brief(&w, &id);
    assert!(text.contains(&format!("git merge --ff-only origin/{branch}")), "{text}");
    w.wait_show("labA 다시 받음", &id, |v| v["brief"] == "delivered");

    // 끝낼 때 다른 머신에 남은 자리를 알린다
    w.h.ok(&["thread", "resolve", "demo", &id]);
    let items = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"].as_array().unwrap().clone();
    assert!(
        items.iter().any(|i| i["event"] == "resolved"
            && i["summary"].as_str().is_some_and(|s| s.contains("앞 자리 labB") && s.contains(path_str(&cwd_b)))),
        "{items:?}"
    );
}
