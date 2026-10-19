//! 로컬 bare 원격의 실제 push·조회 agent·thread 없는 사건 repo 구독 경계.
use super::*;
use sha2::{Digest, Sha256};
fn poll(db: &aaa_host::db::Db, key: &str, after: u64) -> u64 {
    wait_for("Git poll 저장", T, || {
        let v: Value = serde_json::from_str(&db.meta(key).ok()??).ok()?;
        let now = v["last_poll_ms"].as_u64()?;
        (now > after).then_some(now)
    })
}
fn quiet(w: &World, db: &aaa_host::db::Db, key: &str) {
    let now: Value = serde_json::from_str(&db.meta(key).unwrap().unwrap()).unwrap();
    let a = poll(db, key, now["last_poll_ms"].as_u64().unwrap());
    poll(db, key, a);
    w.h.ticks(2);
}
fn git_events(w: &World) -> Vec<Value> {
    w.h.ok(&["events", "demo"])["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["thread"].is_null()
                && e["kind"].as_str().is_some_and(|k| ["git_pushed", "git_forced", "git_deleted"].contains(&k))
        })
        .cloned()
        .collect()
}
fn head(dir: &Path) -> String {
    String::from_utf8(Command::new("git").arg("-C").arg(dir).args(["rev-parse", "HEAD"]).output().unwrap().stdout)
        .unwrap()
        .trim()
        .into()
}
fn failed_queries(trace: &Path) -> usize {
    let rows: Vec<Value> = std::fs::read_to_string(trace)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let sessions: std::collections::BTreeSet<_> = rows
        .iter()
        .filter(|v| v["event"] == "start" && v["argv"].as_array().is_some_and(|a| a.iter().any(|s| s == "ls-remote")))
        .filter_map(|v| v["sid"].as_str())
        .collect();
    rows.iter()
        .filter(|v| {
            v["event"] == "exit"
                && v["code"].as_i64().is_some_and(|c| c != 0)
                && v["sid"].as_str().is_some_and(|s| sessions.contains(s))
        })
        .count()
}
#[test]
fn git_push_mission_repo_filter_restart_force_delete_and_outage() {
    let mut w = world_with("tick_secs=1\ngit_poll_secs=1\npr_poll_secs=3600\noutage_secs=0\n");
    w.add_remote();
    let trace = w.lab.root.join("git-trace.json");
    w.lab.kill("agentB");
    wait_for("조회 agent 종료", T, || (!w.h.connected("labB")).then_some(()));
    let b = Node { cfg: w.lab.root.join("b/c"), state: w.lab.root.join("b/s") };
    w.lab.spawn_env(
        "agentB-trace",
        &b,
        &["agent", "--host", &w.addr, "--name", "labB"],
        &[("GIT_TRACE2_EVENT", path_str(&trace))],
    );
    wait_for("조회 agent 연결", T, || w.h.connected("labB").then_some(()));
    let config = std::fs::read_to_string(w.h.cfg.join("host.toml")).unwrap().replace("127.0.0.1:0", &w.addr);
    std::fs::write(w.h.cfg.join("host.toml"), config).unwrap();
    w.h.ok(&["project", "set", "demo", "repos.add", &format!("{}@labB", w.repo.display())]);
    let project = w.projects.join("demo");
    std::fs::create_dir_all(project.join("missions")).unwrap();
    for (name, repo) in [("git-watch", "repo"), ("other-watch", "unrelated")] {
        std::fs::write(project.join(format!("missions/{name}.md")), format!("+++\nprofile=\"resumable\"\nmachine=\"labA\"\nevents=[\"git_pushed\"]\nrepos=[{repo:?}]\nbatch_s=0\nmin_gap=\"1s\"\nmax_wakes=20\nmax_minutes=15\nreset_after_wakes=3\nwithdrawal=\"none\"\n+++\n변화 자료를 분류한다.\n")).unwrap();
        let approved = w.h.in_terminal(&["mission", "approve", "demo", name], &[("그대로 입력한다:", name)], T);
        assert!(!approved.contains("일치하지"), "{approved}");
        w.h.ok(&["mission", "start", "demo", name]);
        wait_for("임무 brief 전달", T, || {
            (w.h.ok(&["mission", "show", "demo", name])["mission"]["thread"]["brief"] == "delivered").then_some(())
        });
    }
    std::fs::write(
        project.join("routines/git-main.md"),
        "+++\non=\"git\"\nrepo=\"repo\"\nbranches=[\"main\"]\n+++\n변화를 기록한다.\n",
    )
    .unwrap();
    let mid = w.h.machine("labB").unwrap()["id"].as_str().unwrap().to_string();
    let key = format!("git:demo:{}", hex::encode(Sha256::digest(format!("{mid}:{}", w.repo.display()).as_bytes())));
    let db = aaa_host::db::Db::open(&w.h.state.join("host/host.db")).unwrap();
    poll(&db, &key, 0);
    quiet(&w, &db, &key);
    assert!(git_events(&w).is_empty(), "첫 등록과 변화 없음은 무음");
    let base = head(&w.repo);
    let fetch_head = std::fs::read(w.repo.join(".git/FETCH_HEAD")).ok();
    let seed = w.lab.root.join("seed");
    std::fs::write(seed.join("push.txt"), "push\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "병합: Git 감시 fixture (t-0042)"]);
    let pushed = head(&seed);
    let origin = w.lab.root.join("origin.git");
    let mut flow = w.lab.follow("git-flow", &w.h, "demo");
    git(&seed, &["push", "-q", path_str(&origin), "HEAD:main"]);
    flow.until("Git 사건 임무 깨움", T, |e| e["kind"] == "mission_wake");
    quiet(&w, &db, &key);
    let events = git_events(&w);
    assert_eq!(events.len(), 1);
    assert_eq!(
        w.h.ok(&["inbox", "list", "demo", "--peek"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == "git")
            .count(),
        1
    );
    assert_eq!(events[0]["repo"], path_str(&w.repo));
    let data: Value = serde_json::from_str(events[0]["detail"].as_str().unwrap()).unwrap();
    assert_eq!(data["change"]["before"], base);
    assert_eq!(data["change"]["after"], pushed);
    assert_eq!(data["change"]["changed_files"], 1);
    assert!(data["change"]["commits"][0]["subject"].as_str().unwrap().contains("t-0042"));
    assert_eq!(w.h.ok(&["mission", "show", "demo", "git-watch"])["mission"]["wakes_today"], 1);
    assert_eq!(w.h.ok(&["mission", "show", "demo", "other-watch"])["mission"]["wakes_today"], 0);
    assert_eq!(head(&w.repo), base, "조회 fetch가 checkout HEAD를 움직이지 않는다");
    let tracking =
        Command::new("git").arg("-C").arg(&w.repo).args(["rev-parse", "refs/remotes/origin/main"]).output().unwrap();
    assert!(tracking.status.success());
    assert_eq!(String::from_utf8(tracking.stdout).unwrap().trim(), base);
    assert_eq!(std::fs::read(w.repo.join(".git/FETCH_HEAD")).ok(), fetch_head);
    quiet(&w, &db, &key);
    assert_eq!(git_events(&w).len(), 1, "변화 없으면 새 사건 없음");
    let delivery_runs = || -> Vec<Value> {
        w.h.ok(&["routine", "runs", "demo", "git-main"])["runs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["id"].as_str().unwrap().starts_with("git-"))
            .cloned()
            .collect()
    };
    let history = delivery_runs();
    assert_eq!(history.len(), 1);
    w.lab.kill("host");
    w.spawn_host("host2");
    wait_for("재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    quiet(&w, &db, &key);
    assert_eq!(git_events(&w).len(), 1, "재시작 뒤 같은 SHA는 무음");
    assert_eq!(delivery_runs(), history);
    assert_eq!(w.h.ok(&["mission", "show", "demo", "git-watch"])["mission"]["wakes_today"], 1);
    git(&seed, &["reset", "--hard", &base]);
    std::fs::write(seed.join("forced.txt"), "force\n").unwrap();
    git(&seed, &["add", "."]);
    git(&seed, &["commit", "-qm", "다른 이력"]);
    git(&seed, &["push", "-q", "--force", path_str(&origin), "HEAD:main"]);
    wait_for("force 사건", T, || git_events(&w).iter().any(|e| e["kind"] == "git_forced").then_some(()));
    quiet(&w, &db, &key);
    // 같은 B→C force 전이가 다시 와도 새 사건이어야 한다(평생 SHA쌍 dedupe 금지).
    let forced = head(&seed);
    git(&seed, &["push", "-q", "--force", path_str(&origin), &format!("{pushed}:main")]);
    wait_for("되돌린 force", T, || (git_events(&w).len() == 3).then_some(()));
    quiet(&w, &db, &key);
    git(&seed, &["push", "-q", "--force", path_str(&origin), &format!("{forced}:main")]);
    wait_for("같은 SHA 전이의 새 사건", T, || (git_events(&w).len() == 4).then_some(()));
    quiet(&w, &db, &key);
    git(&origin, &["config", "receive.denyDeleteCurrent", "ignore"]);
    git(&seed, &["push", "-q", path_str(&origin), ":main"]);
    wait_for("삭제 사건", T, || git_events(&w).iter().any(|e| e["kind"] == "git_deleted").then_some(()));
    quiet(&w, &db, &key);
    assert_eq!(git_events(&w).len(), 5);
    // 실제 ls-remote 실패를 만든다. 인증·실제 원격 서버를 건드리지 않는다.
    std::fs::rename(&origin, w.lab.root.join("offline.git")).unwrap();
    wait_for("Git outage", T, || {
        w.h.ok(&["inbox", "list", "demo", "--peek"])["items"]
            .as_array()?
            .iter()
            .find(|i| i["item_kind"] == "outage" && i["subject"] == "git:git-main")
            .cloned()
    });
    let failed = failed_queries(&trace);
    wait_for("추가 ls-remote 실패 두 번", T, || (failed_queries(&trace) >= failed + 2).then_some(()));
    w.h.ticks(2);
    let items = w.h.ok(&["inbox", "list", "demo", "--peek"]);
    assert_eq!(
        items["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == "outage" && i["subject"] == "git:git-main")
            .count(),
        1
    );
    assert_eq!(w.h.ok(&["mission", "show", "demo", "git-watch"])["mission"]["wakes_today"], 1);
}
