//! CLI holds → 실제 두 agent → 승인/대기/시작 → host 재시작의 L5 전송 경계.
use super::*;
fn automatic(w: &World) -> Vec<Value> {
    w.h.ok(&["events", "demo"])["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "thread_started" && e["actor"] == "routine:check")
        .cloned()
        .collect()
}
#[test]
fn automation_start_holds_approval_two_agents_and_restart_receipt() {
    let mut w = world_with("tick_secs=1\n");
    w.add_remote();
    let config = std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap().replace("127.0.0.1:0", &w.addr);
    std::fs::write(w.h.cfg.join("host.toml"), config).unwrap();
    w.h.in_terminal(&["safety", "set", "start_threads", "auto"], &[("[y/N]", "y")], T);
    w.h.ok(&["project", "set", "demo", "repos.add", path_str(&w.repo)]);
    w.h.ok(&["project", "set", "demo", "repos.add", &format!("{}@labB", path_str(&w.repo))]);
    let task = w.task("hold.md", "noreport");
    let mut holders = vec![];
    for machine in ["labA", "labB"] {
        let row = w.start(&[
            "--title",
            "측정 중",
            "--profile",
            "fake",
            "--machine",
            machine,
            "--kind",
            "dir",
            "--holds",
            "timing",
            "--task-file",
            &task,
        ]);
        let id = row["id"].as_str().unwrap().to_string();
        let shown = w.wait_show("측정 thread brief", &id, |v| v["brief"] == "delivered");
        assert_eq!(shown["holds"], serde_json::json!(["timing"]));
        holders.push(id);
    }
    let dir = w.projects.join("demo/automations");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("check.toml"),"[on.thread]\nevents=['report_ready']\nrepos=['repo']\n[do.start]\ntitle='자동 점검'\nrepo='repo'\ntext='noreport'\nprofiles=['fake','mute']\nprofile_pick='week-left'\nmachines=['labA','labB']\nwait=['timing']\n").unwrap();
    // 기존 thread 사건은 등록 시 cursor의 기준일 뿐이다.
    w.h.ticks(2);
    let listed = w.h.ok(&["routine", "list", "demo"]);
    let r = listed["routines"].as_array().unwrap().iter().find(|r| r["name"] == "check").unwrap();
    assert_eq!(r["start_approved"], false);
    let source_task = w.task("source.md", "원인 사건의 실제 보고");
    let source = w.start(&[
        "--title",
        "원인 사건",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &source_task,
    ]);
    w.wait_show("원인 thread 준비", source["id"].as_str().unwrap(), |v| v["brief"] == "delivered");
    wait_for("승인 대기 기록", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "runs", "demo", "check"])["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["detail"].as_str().is_some_and(|s| s.contains("범위 승인 필요")))
            .then_some(())
    });
    assert!(automatic(&w).is_empty());
    // 사람 CLI가 앞서 검토한 hash를 승인한다. 세션 안 agent의 대리 승인은 쓰지 않는다.
    w.h.in_terminal(&["automation", "approve", "check", r["start_approval_sha256"].as_str().unwrap()], &[], T);
    wait_for("holds 대기 기록", T, || {
        w.h.ticks(1);
        w.h.ok(&["routine", "runs", "demo", "check"])["runs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["detail"].as_str().is_some_and(|s| s.contains("timing")))
            .then_some(())
    });
    assert!(automatic(&w).is_empty());
    w.h.ok(&["thread", "resolve", "demo", &holders[1], "--skip-copy"]);
    let event = wait_for("표지 해제 뒤 자동 시작", T, || automatic(&w).first().cloned());
    let id = event["thread"].as_str().unwrap();
    let shown = w.wait_show("자동 worker brief", id, |v| v["brief"] == "delivered");
    assert_eq!(shown["machine"], "labB");
    assert_eq!(shown["role"], "worker");
    assert_eq!(shown["repo"], path_str(&w.repo));
    assert_eq!(shown["thread_kind"], "worktree");
    assert_eq!(shown["profile"], "fake");
    w.lab.kill("host");
    w.spawn_host("host2");
    wait_for("두 agent 재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    w.h.ticks(2);
    assert_eq!(automatic(&w).len(), 1, "같은 사건은 재시작 뒤 다시 시작하지 않는다");
    // 다른 사건은 앞의 자동 thread가 열려 있어도 실행한다(기본 동시성 상한 없음).
    let second_task = w.task("second.md", "다른 원인 사건의 실제 보고");
    let second = w.start(&[
        "--title",
        "다른 원인",
        "--profile",
        "fake",
        "--repo",
        path_str(&w.repo),
        "--task-file",
        &second_task,
    ]);
    w.wait_show("다른 원인 준비", second["id"].as_str().unwrap(), |v| v["brief"] == "delivered");
    wait_for("서로 다른 사건 두 번째 시작", T, || (automatic(&w).len() == 2).then_some(()));
}
