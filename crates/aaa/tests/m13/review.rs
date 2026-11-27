//! CLI→원격 agent→host 원장→원격 실행기. 인증·후보·소유권과 one-shot 종료를 본다.
use super::*;
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn remote_review_is_pinned_owned_and_cancellable() {
    let (mut w, env) = world_files("", Some("0"), true, |_| vec![]);
    let host_config = w.h.cfg.join("host.toml");
    let config = fs::read_to_string(&host_config)
        .unwrap()
        .replace("\"resumable\", \"evil\"]", "\"resumable\", \"evil\", \"review-codex\", \"review-claude\"]");
    fs::write(
        host_config,
        format!("{config}\n[profiles.review-codex]\nagent = \"codex\"\nmodel = \"review-codex-model\"\neffort = \"high\"\nargs = [\"--profile-argument\"]\nenv = {{ REVIEW_PROFILE_ENV = \"fixture\" }}\n\
            [profiles.review-claude]\nagent = \"claude\"\nmodel = \"review-claude-model\"\n"),
    ).unwrap();
    w.host_env.push(("AAA_TEST_SUBMIT_GAP_MS".into(), "0".into()));
    let mut w = world_start(w, env);

    let runner = w.scripts.join("review-runner");
    fs::write(&runner, include_str!("../fixtures/review_runner.py")).unwrap();
    fs::set_permissions(&runner, fs::Permissions::from_mode(0o755)).unwrap();
    let remote = w.enroll_remote();
    let mut watch = HostWatch::new(&w.h, T);
    w.lab.spawn_env(
        "agentB",
        &remote,
        &["agent", "--host", &w.addr, "--name", "labB"],
        &[
            ("AAA_TEST_REVIEW_CODEX", path_str(&runner)),
            ("AAA_TEST_REVIEW_CLAUDE", path_str(&runner)),
            ("OPENAI_API_KEY", "fixture-inherited"),
        ],
    );
    watch.wait_diag("검수 agent 연결", |s| s.connected("labB").then_some(()), || w.lab.log("agentB"));
    w.h.ok(&["machine", "workspaces", "labB", "--allow", "작업 폴더"]);
    fs::write(
        w.projects.join("demo/project.toml"),
        format!("schema = 1\n[[repos]]\npath = {:?}\nmachine = \"labB\"\n", w.repo),
    )
    .unwrap();

    let codex = aaa_test_support::token("review-codex");
    let claude = aaa_test_support::token("review-claude");
    let auth = w.task("review-auth.json", &serde_json::json!({"tokens":{
        "access_token":codex,"account_id":"fake-workspace","refresh_token":aaa_test_support::token("review-refresh"),
    }}).to_string());
    let token = w.task("review-auth.token", &claude);
    for (name, harness, path) in [("review-openai", "codex", auth), ("review-anthropic", "claude", token)] {
        w.h.ok(&["account", "import", name, "--harness", harness, "--file", &path]);
        w.h.ok(&["account", "default", harness, name]);
    }

    let owner = w.start(&[
        "--title",
        "review owner",
        "--task-file",
        &w.task("review-owner.md", "noreport"),
        "--machine",
        "labB",
        "--repo",
        path_str(&w.repo),
        "--profile",
        "fake",
    ]);
    let id = owner["id"].as_str().unwrap();
    let pane = owner["pane"].as_str().unwrap();
    let cwd = Path::new(owner["cwd"].as_str().unwrap());
    w.wait_show("검수 thread 준비", id, |v| v["brief"] == "delivered");
    let base = git(cwd, &["rev-parse", "HEAD"]);
    fs::write(cwd.join("README.md"), "candidate-only\n").unwrap();
    git(cwd, &["add", "README.md"]);
    git(cwd, &["commit", "-qm", "candidate"]);
    let candidate = git(cwd, &["rev-parse", "HEAD"]);
    fs::write(w.scripts.join("review-fixture.json"), serde_json::json!({
        "original": cwd, "codex": hex::encode(Sha256::digest(&codex)), "claude": hex::encode(Sha256::digest(&claude)),
    }).to_string()).unwrap();

    run_probe(&w, pane, &base, "owner");
    assert_eq!(git(cwd, &["rev-parse", "HEAD"]), candidate);
    assert_eq!(fs::read_to_string(cwd.join("dirty.txt")).unwrap(), "live-only\n");
    assert_eq!(fs::read_dir(remote.state.join("agent/review-work")).unwrap().count(), 0);
    assert!(!w.h.state.join("agent/review-work").exists());
    let events = w.h.ok(&["events", "demo"]);
    assert_eq!(
        events["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["kind"] == "review_finished" && e["thread"] == id)
            .count(),
        4
    );

    let other = w.start(&[
        "--title",
        "other",
        "--task-file",
        &w.task("review-other.md", "noreport"),
        "--machine",
        "labB",
        "--kind",
        "dir",
        "--profile",
        "fake",
    ]);
    w.wait_show("다른 thread 준비", other["id"].as_str().unwrap(), |v| v["brief"] == "delivered");
    run_probe(&w, other["pane"].as_str().unwrap(), &base, "deny");
}

fn run_probe(w: &World, pane: &str, base: &str, mode: &str) {
    let python = w.task("review-probe.py", include_str!("../fixtures/review_probe.py"));
    let output = w.lab.root.join(format!("review-{mode}.out"));
    let done = w.lab.root.join(format!("review-{mode}.done"));
    let script = w.task(
        &format!("review-{mode}.sh"),
        &format!("python3 {python:?} {:?} {:?} {base} {mode} > {output:?} 2>&1\nprintf '%s' \"$?\" > {done:?}\necho review-probe-done\n", lab::BIN, w.scripts),
    );
    w.h.ok(&["pane", "run", pane, &format!("post-probe {script}")]);
    w.h.wait_output(pane, "review-probe-done", T);
    assert_eq!(fs::read_to_string(done).unwrap(), "0", "{}", fs::read_to_string(output).unwrap());
}
