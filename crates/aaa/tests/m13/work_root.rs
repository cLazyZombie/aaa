//! 작업 경로는 CLI 설정 → 실제 agent 시작 경계에서 검사한다.
use super::*;

#[test]
#[cfg(target_os = "macos")]
fn agent_restart_preserves_threads_after_branch_changes_and_isolates_invalid_preparations() {
    use std::os::unix::ffi::OsStrExt;
    let mut w = world();
    let root = w.lab.root.join("portable");
    let path = std::ffi::CString::new(w.lab.root.as_os_str().as_bytes()).unwrap();
    let mut fs: libc::statfs = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::statfs(path.as_ptr(), &mut fs) }, 0);
    let mount = unsafe { std::ffi::CStr::from_ptr(fs.f_mntonname.as_ptr()) }.to_str().unwrap();
    w.h.ok(&["work-root", "set", path_str(&root), "--mount", mount]);
    w.lab.kill("agentA");
    wait_for("옛 agent 종료", T, || (!w.h.connected("labA")).then_some(()));
    w.lab.spawn("portable-agent", &w.h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("휴대용 root agent 연결", T, || w.h.connected("labA").then_some(()));
    let mut threads = Vec::new();
    for title in ["changed", "healthy", "foreign"] {
        let thread = w.start(&[
            "--profile",
            "fake",
            "--repo",
            path_str(&w.repo),
            "--title",
            title,
            "--task-file",
            &w.task(&format!("{title}.txt"), "noreport"),
        ]);
        w.wait_show("brief 전달", thread["id"].as_str().unwrap(), |v| v["brief"] == "delivered");
        threads.push(thread);
    }
    let cwd = Path::new(threads[0]["cwd"].as_str().unwrap());
    git(cwd, &["switch", "-qc", "next-stage"]);
    std::fs::write(cwd.join("next-stage.txt"), "keep dirty work\n").unwrap();
    let preparations = w.h.state.join("agent/thread-preparations");
    let changed_journal = preparations.join(format!("demo-{}.json", threads[0]["id"].as_str().unwrap()));
    let original = std::fs::read(&changed_journal).unwrap();
    let pids: Vec<_> = threads.iter().map(|t| w.h.session_host_pid(t["pane"].as_str().unwrap()).unwrap()).collect();
    // Git 잠금은 다시 부여해야 한다. 단순히 오류를 무시하는 수리로는 이 계약을 통과하지 못한다.
    for thread in &threads {
        git(&w.repo, &["worktree", "unlock", thread["cwd"].as_str().unwrap()]);
    }
    let foreign = preparations.join(format!("demo-{}.json", threads[2]["id"].as_str().unwrap()));
    let foreign_original = std::fs::read(&foreign).unwrap();
    // 옛 repo에 Git 등록이 남은 경로를 다른 repo로 바꾼다. 등록 목록만 보면 소유권을 오인한다.
    let foreign_cwd = Path::new(threads[2]["cwd"].as_str().unwrap());
    std::fs::remove_file(foreign_cwd.join(".git")).unwrap();
    git(foreign_cwd, &["init", "-q", "-b", "main"]);
    let broken = preparations.join("demo-t-9000.json");
    std::fs::write(&broken, "{broken journal").unwrap();
    w.lab.kill("portable-agent");
    wait_for("휴대용 agent 종료", T, || (!w.h.connected("labA")).then_some(()));
    w.lab.spawn("restarted-agent", &w.h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("branch 변경 뒤 agent 연결", T, || {
        let log = w.lab.log("restarted-agent");
        assert!(!log.contains("Error:"), "agent 시작 실패: {log}");
        w.h.connected("labA").then_some(())
    });
    for (thread, pid) in threads[..2].iter().zip(pids) {
        assert_eq!(w.h.session_host_pid(thread["pane"].as_str().unwrap()), Some(pid));
        assert!(w.h.json(&["pane", "read", thread["pane"].as_str().unwrap()]).is_ok());
    }
    let list = git(&w.repo, &["worktree", "list", "--porcelain"]);
    for (index, thread) in threads.iter().enumerate() {
        let record = list.split("\n\n").find(|r| r.contains(thread["cwd"].as_str().unwrap())).unwrap();
        assert_eq!(record.contains("locked aaa:"), index < 2, "{record}");
    }
    assert_eq!(git(cwd, &["branch", "--show-current"]), "next-stage");
    assert_eq!(std::fs::read_to_string(cwd.join("next-stage.txt")).unwrap(), "keep dirty work\n");
    assert_eq!(std::fs::read(&changed_journal).unwrap(), original, "준비 재진입의 원래 branch 계약은 보존한다");
    assert_eq!(std::fs::read(&foreign).unwrap(), foreign_original);
    assert_eq!(std::fs::read_to_string(&broken).unwrap(), "{broken journal");
    let log = w.lab.log("restarted-agent");
    assert!(log.contains("Git 잠금 점검 보류"), "{log}");
    assert!(log.contains(foreign.file_name().unwrap().to_str().unwrap()), "{log}");
    assert!(log.contains("demo-t-9000.json"), "{log}");
}

#[test]
fn agent_refuses_an_unmounted_work_volume_before_creating_a_workspace() {
    let mut lab = Lab::new();
    let node = lab.node("agent", None);
    // 빠진 볼륨 자리에 남은 일반 폴더: exists 검사만 하면 통과하는 입력이다.
    let mount = lab.root.join("volume");
    let root = mount.join("work");
    std::fs::create_dir(&mount).unwrap();
    for command in [
        vec!["work-root", "check", "--path", path_str(&root), "--mount", path_str(&mount)],
        vec!["work-root", "set", path_str(&root), "--mount", path_str(&mount)],
    ] {
        let error = node.json(&command).expect_err("명시 mount 인자를 무시하면 안 된다");
        assert!(error.contains("작업 볼륨이 마운트되지 않았다"), "{error}");
        assert!(!root.exists());
        assert!(!node.cfg.join("agent.local.toml").exists());
    }
    std::fs::write(
        node.cfg.join("agent.toml"),
        format!("schema = 1\n[agent]\nwork_root = {root:?}\nwork_mount = {mount:?}\n"),
    )
    .unwrap();
    for command in [vec!["work-root", "check"], vec!["work-root", "set", path_str(&root)]] {
        let mut args = vec!["--json"];
        args.extend(command);
        let output = node.run(&args);
        assert!(!output.status.success(), "CLI도 같은 mount 검사를 거쳐야 한다");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert!(error["error"].as_str().unwrap().contains("작업 볼륨이 마운트되지 않았다"), "{error}");
        assert!(!root.exists());
        assert!(!node.cfg.join("agent.local.toml").exists());
    }
    lab.spawn("agent-start", &node, &["agent"]);
    assert!(!lab.wait_exit("agent-start", T).success());
    assert!(lab.log("agent-start").contains("작업 볼륨이 마운트되지 않았다"), "{}", lab.log("agent-start"));
    assert!(!root.exists(), "내장에 대체 작업 폴더를 만들면 안 된다");
}

#[test]
fn work_root_set_changes_only_the_next_start_and_old_threads_resolve_and_sweep_in_place() {
    let mut w = world();
    let a = w.lab.root.join("workA");
    let b = w.lab.root.join("workB");
    let original = std::fs::read(w.h.cfg.join("agent.toml")).unwrap();
    let local = format!(
        "schema = 1 # schema comment\n\n[agent] # machine comment\nwork_root = {a:?} # root comment\nname = \"keep-name\" # name comment\n\n[reconnect]\nmax_backoff_s = 1 # reconnect comment\n"
    );
    let local_path = w.h.cfg.join("agent.local.toml");
    std::fs::write(&local_path, &local).unwrap();
    fn start(w: &World, title: &str) -> Value {
        w.start(&[
            "--profile",
            "fake",
            "--repo",
            path_str(&w.repo),
            "--title",
            title,
            "--task-file",
            &w.task(&format!("{title}.txt"), "noreport"),
        ])
    }
    let old = start(&w, "old");
    let id = old["id"].as_str().unwrap().to_string();
    w.wait_show("옛 작업의 brief 전달", &id, |v| v["brief"] == "delivered");
    let configured = w.h.ok(&["work-root", "set", path_str(&b)]);
    assert_eq!(configured["configured"]["path"], path_str(&b));
    assert_eq!(configured["last_start"]["path"], path_str(&a));
    assert_eq!(configured["restart_required"], true);
    assert_eq!(std::fs::read(w.h.cfg.join("agent.toml")).unwrap(), original);
    let edited = std::fs::read_to_string(&local_path).unwrap();
    for keep in [
        "# schema comment",
        "[agent] # machine comment",
        "# root comment",
        "name = \"keep-name\" # name comment",
        "max_backoff_s = 1 # reconnect comment",
    ] {
        assert!(edited.contains(keep), "{keep}: {edited}");
    }
    let repeated = w.h.ok(&["work-root", "set", path_str(&b)]);
    assert_eq!(repeated["changed"], false);
    assert_eq!(repeated["imported_preparations"], 0);
    assert_eq!(std::fs::read_to_string(&local_path).unwrap(), edited);
    let before = start(&w, "before");
    let before_pane = before["pane"].as_str().unwrap();
    let before_pid = w.h.session_host_pid(before_pane).unwrap();
    let before_id = before["id"].as_str().unwrap().to_string();
    w.wait_show("재시작 전 A 작업", &before_id, |v| v["brief"] == "delivered");
    assert!(Path::new(before["cwd"].as_str().unwrap()).starts_with(&a));
    // dirty인 A를 resolve해 남긴 뒤 B 전환 후 sweep으로 회수한다.
    let old_cwd = Path::new(old["cwd"].as_str().unwrap());
    std::fs::write(old_cwd.join("file-to-keep"), "dirty\n").unwrap();
    w.h.ok(&["thread", "resolve", "demo", &id]);
    assert!(old_cwd.is_dir());
    w.lab.kill("agentA");
    wait_for("agent 연결 종료", T, || (!w.h.connected("labA")).then_some(()));
    w.lab.spawn("agentB", &w.h, &["agent", "--host", "local", "--name", "labA"]);
    wait_for("새 root agent 연결", T, || w.h.connected("labA").then_some(()));
    let shown = w.h.ok(&["work-root", "show"]);
    assert_eq!(shown["last_start"]["path"], path_str(&b));
    assert_eq!(shown["restart_required"], false);
    // resolve하지 않은 기존 pane의 session host는 그대로다.
    assert_eq!(w.h.session_host_pid(before_pane), Some(before_pid));
    w.h.ok(&["thread", "resolve", "demo", &before_id]);
    assert!(!Path::new(before["cwd"].as_str().unwrap()).exists());
    std::fs::remove_file(old_cwd.join("file-to-keep")).unwrap();
    w.h.ok(&["project", "sweep", "demo", "--yes"]);
    assert!(!old_cwd.exists());
    let fresh = start(&w, "after");
    assert!(Path::new(fresh["cwd"].as_str().unwrap()).starts_with(&b), "{fresh}");
}

#[test]
fn work_root_set_recovers_prepared_state_imports_legacy_and_rejects_unsafe_config_files() {
    let lab = Lab::new();
    let node = lab.node("node", None);
    let a = lab.root.join("A");
    let b = lab.root.join("B");
    std::fs::write(node.cfg.join("agent.toml"), format!("schema = 1\n[agent]\nwork_root = {a:?}\n")).unwrap();
    let legacy_dir = a.join(".preparations");
    std::fs::create_dir_all(&legacy_dir).unwrap();
    let cwd = a.join("demo/t-9000");
    std::fs::create_dir_all(&cwd).unwrap();
    let legacy = legacy_dir.join("demo-t-9000.json");
    let bytes = serde_json::to_vec(
        &serde_json::json!({"repo":null,"cwd":cwd,"branch":null,"base":null,"head":null,"ready":null}),
    )
    .unwrap();
    std::fs::write(&legacy, &bytes).unwrap();
    let changed = node.ok(&["work-root", "set", path_str(&b)]);
    assert_eq!(changed["configured"]["path"], path_str(&b));
    assert_eq!(changed["last_start"], Value::Null);
    assert_eq!(changed["restart_required"], true);
    assert_eq!(changed["imported_preparations"], 1);
    let local = node.cfg.join("agent.local.toml");
    let before_checkpoint = std::fs::read(&local).unwrap();
    let c = lab.root.join("C");
    // 실제 등록 완료·config rename 직전 중단 상태에서 같은 set을 이어 간다.
    {
        let mut tx = aaa_agent::work_root::RootTransaction::open(&node.state.join("agent")).unwrap();
        tx.register(&c, None).unwrap();
    }
    assert_eq!(std::fs::read(&local).unwrap(), before_checkpoint);
    let resumed = node.ok(&["work-root", "set", path_str(&c)]);
    assert_eq!(resumed["configured"]["path"], path_str(&c));
    assert_eq!(resumed["imported_preparations"], 0);
    assert_eq!(node.ok(&["work-root", "set", path_str(&c)])["changed"], false);

    assert_eq!(std::fs::read(&legacy).unwrap(), bytes);
    let saved: Value =
        serde_json::from_slice(&std::fs::read(node.state.join("agent/thread-preparations/demo-t-9000.json")).unwrap())
            .unwrap();
    assert_eq!(saved["cwd"], path_str(&cwd));
    assert_eq!(saved["root"], path_str(&a));
    let local = node.cfg.join("agent.local.toml");
    let good = std::fs::read(&local).unwrap();
    for kind in ["readonly", "symlink", "hardlink", "malformed"] {
        std::fs::remove_file(&local).unwrap();
        let other = node.cfg.join(format!("original-{kind}.toml"));
        std::fs::write(&other, &good).unwrap();
        match kind {
            "readonly" => {
                std::fs::write(&local, &good).unwrap();
                std::fs::set_permissions(&local, std::os::unix::fs::PermissionsExt::from_mode(0o400)).unwrap();
            }
            "symlink" => std::os::unix::fs::symlink(&other, &local).unwrap(),
            "hardlink" => std::fs::hard_link(&other, &local).unwrap(),
            "malformed" => std::fs::write(&local, "[agent\n").unwrap(),
            _ => unreachable!(),
        }
        let previous = std::fs::read(&local).unwrap();
        assert!(node.json(&["work-root", "set", path_str(&a)]).is_err(), "{kind} 설정을 교체하면 안 된다");
        assert_eq!(std::fs::read(&local).unwrap(), previous);
        assert_eq!(std::fs::read(other).unwrap(), good);
    }
}
