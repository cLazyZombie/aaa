//! 로컬 bare 원격의 실제 push·조회 agent·thread 없는 사건 repo 구독 경계.
use super::*;
use aaa_common::child_process::StdCommandExt;
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
    String::from_utf8(
        Command::new("git").arg("-C").arg(dir).args(["rev-parse", "HEAD"]).capture_output().unwrap().stdout,
    )
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
fn git_push_restart_force_delete_and_outage() {
    let mut w = world_setup("tick_secs=1\ngit_poll_secs=1\npr_poll_secs=3600\noutage_secs=0\n", false, |_| {
        vec![("AAA_TEST_WATCH_POLL_MS".into(), "10".into())]
    });
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
    flow.until("Git 사건", T, |e| e["kind"] == "git_pushed");
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
    assert_eq!(head(&w.repo), base, "조회 fetch가 checkout HEAD를 움직이지 않는다");
    let tracking = Command::new("git")
        .arg("-C")
        .arg(&w.repo)
        .args(["rev-parse", "refs/remotes/origin/main"])
        .capture_output()
        .unwrap();
    assert!(tracking.status.success());
    assert_eq!(String::from_utf8(tracking.stdout).unwrap().trim(), base);
    assert_eq!(std::fs::read(w.repo.join(".git/FETCH_HEAD")).ok(), fetch_head);
    quiet(&w, &db, &key);
    assert_eq!(git_events(&w).len(), 1, "변화 없으면 새 사건 없음");
    let delivery_runs = |w: &World| -> Vec<Value> {
        w.h.ok(&["routine", "runs", "demo", "git-main"])["runs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["id"].as_str().unwrap().starts_with("git-"))
            .cloned()
            .collect()
    };
    let history = wait_for("Git 사건의 자동화 실행 완료", T, || {
        let runs = delivery_runs(&w);
        (runs.first().is_some_and(|r| r["status"] == "done" && r["detail"] == "coordinator 안내")).then_some(runs)
    });
    assert_eq!(history.len(), 1);
    w.lab.kill("host");
    w.spawn_host("host2");
    wait_for("재연결", T, || (w.h.connected("labA") && w.h.connected("labB")).then_some(()));
    quiet(&w, &db, &key);
    assert_eq!(git_events(&w).len(), 1, "재시작 뒤 같은 SHA는 무음");
    assert_eq!(delivery_runs(&w), history);
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
}

#[test]
fn deploy_proposals_count_main_commits_across_restart_and_success_without_installing() {
    let mut w = world_setup("tick_secs=1\ngit_poll_secs=1\npr_poll_secs=3600\n", false, |_| {
        vec![("AAA_TEST_WATCH_POLL_MS".into(), "10".into())]
    });
    w.h.ok(&["project", "set", "demo", "repos.add", path_str(&w.repo)]);
    let project = w.projects.join("demo");
    std::fs::create_dir_all(project.join("automations")).unwrap();
    std::fs::write(
        project.join("automations/deploy.toml"),
        "do='deploy'\n[on.git]\nrepo='repo'\nbranches=['main']\n[limits]\nmin_commits=2\n",
    )
    .unwrap();
    let mid = w.h.machine("labA").unwrap()["id"].as_str().unwrap().to_string();
    let key = format!(
        "git:demo:{}",
        hex::encode(Sha256::digest(format!("{mid}:{}", w.repo.canonicalize().unwrap().display()).as_bytes()))
    );
    let db = aaa_host::db::Db::open(&w.h.state.join("host/host.db")).unwrap();
    let proposals = |w: &World| -> Vec<Value> {
        w.h.ok(&["inbox", "list", "demo", "--all", "--peek"])["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["item_kind"] == "deploy_proposal")
            .cloned()
            .collect()
    };
    poll(&db, &key, 0);
    quiet(&w, &db, &key);
    assert!(proposals(&w).is_empty(), "등록 때는 기준만 잡는다");
    let base = head(&w.repo);
    let seed = w.lab.root.join("seed");
    let origin = w.lab.root.join("origin.git");
    let push = |n: u32| -> String {
        std::fs::write(seed.join("deploy.txt"), format!("{n}\n")).unwrap();
        git(&seed, &["add", "."]);
        git(&seed, &["commit", "-qm", &format!("병합: private-deploy-subject {n} (t-0042)")]);
        git(&seed, &["push", "-q", path_str(&origin), "HEAD:main"]);
        head(&seed)
    };
    push(1);
    quiet(&w, &db, &key);
    assert!(proposals(&w).is_empty(), "두 커밋 전에는 제안하지 않는다");
    w.lab.kill("host");
    w.spawn_host("host2");
    wait_for("재접속", T, || w.h.connected("labA").then_some(()));
    std::fs::write(project.join("threads/t-0042.md"), "private-deploy-report").unwrap();
    let first = push(2);
    wait_for("두 커밋의 제안", T, || (proposals(&w).len() == 1).then_some(()));
    let before = proposals(&w);
    assert!(before[0]["body"].as_str().unwrap().contains(&first));
    assert!(before[0]["body"].as_str().unwrap().contains("scripts/deploy-build.sh"));
    assert_eq!(db.meta("rollout:active").unwrap(), None, "제안이 실제 rollout을 만들지 않는다");
    quiet(&w, &db, &key);
    let inbox = w.h.ok(&["inbox", "list", "demo", "--all", "--peek"]);
    assert!(
        !inbox["items"].as_array().unwrap().iter().any(|i| i["item_kind"] == "git"),
        "제안 외의 Git 원문 항목을 만들지 않는다"
    );
    let all = format!("{}{}", inbox, serde_json::to_string(&git_events(&w)).unwrap());
    for private in ["private-deploy-subject", "private-deploy-report", path_str(&w.repo)] {
        assert!(!all.contains(private), "전체 inbox에 원문이 샜다: {private}: {all}");
    }
    assert_eq!(proposals(&w), before, "같은 head는 무음");
    // 완료/실패 정본은 기존 L3 실행기 owner의 산출 형식이다. 실패한 최신 요청은 성공 기준을 덮지 않는다.
    let now = aaa_common::util::now_ms();
    for (state, commit, at) in [("done", first.as_str(), now), ("stopped", base.as_str(), now + 1)] {
        let id = aaa_common::util::new_id();
        let record = serde_json::json!({"id":id,"project":"demo","actor":"local:coordinator@fixture","spec":{"from":"/candidate/aaa","sha256":"a".repeat(64),"commit":commit,"machines":[]},"state":state,"steps":[],"cursor":0,"version":"0.1.2","error":null,"finish":null,"created_ms":at,"updated_ms":at});
        db.start_rollout(&id, &record.to_string()).unwrap();
    }
    let active = db.meta("rollout:active").unwrap();
    push(3);
    quiet(&w, &db, &key);
    assert_eq!(proposals(&w), before, "최근 성공 배포 뒤 한 커밋으로는 제안하지 않는다");
    let second = push(4);
    wait_for("다음 성공 기준의 두 커밋", T, || (proposals(&w).len() == 2).then_some(()));
    assert!(proposals(&w).iter().any(|i| i["body"].as_str().unwrap().contains(&second)));
    assert_eq!(db.meta("rollout:active").unwrap(), active, "제안이 배포 정본을 바꾸지 않는다");
    // 실험실 bare ref를 직접 되감고 지워 forced/deleted 관측을 만든다. 실제 remote force push는 없다.
    git(&origin, &["update-ref", "refs/heads/main", &base]);
    quiet(&w, &db, &key);
    assert_eq!(proposals(&w).len(), 2, "강제 이동은 배포 제안이 아니다");
    git(&origin, &["update-ref", "-d", "refs/heads/main"]);
    quiet(&w, &db, &key);
    assert_eq!(proposals(&w).len(), 2, "삭제는 배포 제안이 아니다");
    // 과거 성공 정본을 남긴 채 다른 history의 origin을 등록한다.
    let other = w.lab.root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    make_repo(&other);
    git(&other.join("seed"), &["checkout", "-q", "--orphan", "separate"]);
    std::fs::write(other.join("seed/README.md"), "different history\n").unwrap();
    git(&other.join("seed"), &["add", "."]);
    git(&other.join("seed"), &["commit", "-qm", "독립 origin 등록"]);
    git(&other.join("seed"), &["push", "-q", path_str(&other.join("origin.git")), "HEAD:refs/heads/registration"]);
    git(&other.join("origin.git"), &["update-ref", "refs/heads/main", &head(&other.join("seed"))]);
    let other_origin = other.join("origin.git");
    let other_seed = other.join("seed");
    git(&w.repo, &["remote", "set-url", "origin", path_str(&other_origin)]);
    quiet(&w, &db, &key);
    w.lab.kill("host2");
    w.spawn_host("host3");
    wait_for("origin 교체 뒤 재접속", T, || w.h.connected("labA").then_some(()));
    let push_other = |n: u32| {
        std::fs::write(other_seed.join("other.txt"), format!("{n}\n")).unwrap();
        git(&other_seed, &["add", "."]);
        git(&other_seed, &["commit", "-qm", &format!("새 origin {n}")]);
        git(&other_seed, &["push", "-q", path_str(&other_origin), "HEAD:main"]);
    };
    push_other(1);
    quiet(&w, &db, &key);
    assert_eq!(proposals(&w).len(), 2, "새 origin도 등록 뒤 한 커밋은 무음");
    push_other(2);
    wait_for("교체 origin의 두 커밋 제안", T, || (proposals(&w).len() == 3).then_some(()));
    assert!(proposals(&w).iter().any(|i| i["body"].as_str().unwrap().contains(&head(&other_seed))));
}
