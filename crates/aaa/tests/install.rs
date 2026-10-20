//! install의 실제 argv·출력·현지 읽기 경계. 서비스/host/agent를 띄우지 않는다.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_aaa");
const CANARY: &str = "install-secret-canary-DO-NOT-PRINT";
static SEQ: AtomicU32 = AtomicU32::new(0);
struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "aaa-install-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn cmd(&self, args: &[&str]) -> Output {
        let mut c = Command::new(BIN);
        c.env_clear().args(args).stdin(Stdio::null());
        for key in ["PATH", "SystemRoot", "WINDIR", "COMSPEC", "PATHEXT"] {
            if let Some(value) = std::env::var_os(key) {
                c.env(key, value);
            }
        }
        for key in ["HOME", "USERPROFILE", "LOCALAPPDATA", "APPDATA", "XDG_CONFIG_HOME", "XDG_STATE_HOME"] {
            c.env(key, &self.0);
        }
        c.env("AAA_CONFIG_DIR", self.0.join("config"))
            .env("AAA_STATE_DIR", self.0.join("state"))
            .env("SHELL", if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
        let out = c.output().unwrap();
        for bytes in [&out.stdout, &out.stderr] {
            assert!(!String::from_utf8_lossy(bytes).contains(CANARY), "비밀 원문 출력");
        }
        out
    }
    fn args(&self) -> Vec<String> {
        [
            "install",
            "--role",
            "agent",
            "--name",
            "mac-a",
            "--host",
            "example.ts.net:7430",
            "--network",
            "direct",
            "--enroll",
            "skip",
            "--work-root",
            self.0.join("작업 root").to_str().unwrap(),
            "--work-storage",
            "internal",
            "--harness",
            "none",
            "--service",
            "none",
            "--start",
            "deferred",
            "--dry-run",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
    fn run(&self, args: &[String]) -> Output {
        self.cmd(&args.iter().map(String::as_str).collect::<Vec<_>>())
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let p = entry.unwrap().path();
            let key = p.strip_prefix(root).unwrap().to_owned();
            if p.is_dir() {
                out.insert(key, vec![]);
                visit(root, &p, out);
            } else {
                out.insert(key, std::fs::read(p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
fn json(out: &Output) -> Value {
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["schema"], "aaa.install.v1");
    assert_eq!(v["exit_code"].as_i64(), out.status.code().map(i64::from));
    v
}

#[test]
fn install_errors_are_sanitized_and_never_prompt_or_write() {
    let lab = Lab::new();
    let before = snapshot(&lab.0);
    for args in [
        vec!["install", "--json", "--dry-run", "--unknown", CANARY],
        vec!["--instance", CANARY, "install", "--json", "--dry-run"],
        vec!["install", "--json", "--dry-run", "--role", CANARY],
        vec!["install", "--json", "--dry-run"],
    ] {
        let v = json(&lab.cmd(&args));
        assert_eq!(v["exit_code"], 2);
        assert_eq!(v["status"], "failed");
        assert!(v["installation_id"].is_null());
    }
    let out = lab.cmd(&["install"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn install_dry_run_is_read_only_and_stream_has_the_same_plan() {
    let lab = Lab::new();
    let before = snapshot(&lab.0);
    let mut args = lab.args();
    args.push("--json".into());
    let v = json(&lab.run(&args));
    assert_eq!(v["status"], "planned");
    assert_eq!(v["exit_code"], 0);
    assert!(v["installation_id"].is_null());
    assert!(v["resume"].is_null());
    assert!(v["changes"].as_array().unwrap().iter().all(|c| c["state"] == "planned" || c["state"] == "unchanged"));
    for name in ["write_probe", "host_connected", "service_ready"] {
        assert!(v["checks"].as_array().unwrap().iter().any(|c| c["name"] == name && c["state"] == "not_run"));
    }
    *args.last_mut().unwrap() = "--json-stream".into();
    let out = lab.run(&args);
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let items: Vec<Value> =
        String::from_utf8(out.stdout).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect();
    for (i, item) in items.iter().enumerate() {
        assert_eq!(item["seq"], (i + 1) as u64);
    }
    assert_eq!(items.last().unwrap()["type"], "result");
    assert_eq!(items.last().unwrap()["result"]["plan_sha256"], v["plan_sha256"]);
    assert_eq!(snapshot(&lab.0), before, "dry-run은 lock/events/probe도 만들지 않는다");
    args.extend(["--events-file".into(), lab.0.join("events").to_string_lossy().into_owned()]);
    let out = lab.run(&args);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn install_reads_local_overrides_and_preserves_existing_installations() {
    let lab = Lab::new();
    let cfg = lab.0.join("config");
    std::fs::create_dir(&cfg).unwrap();
    std::fs::write(cfg.join("agent.toml"), "schema = 1\n[health]\nmin_free_gb = 7\n").unwrap();
    let mut args = lab.args();
    args.extend(["--json".into(), "--mode".into(), "repair".into()]);
    let a = json(&lab.run(&args));
    assert_eq!(a["status"], "planned");
    std::fs::write(
        cfg.join("agent.local.toml"),
        format!("schema = 1\n[health]\nmin_free_gb = 9\n[harnesses.codex.env]\nAPI_TOKEN = '{CANARY}'\n"),
    )
    .unwrap();
    let b = json(&lab.run(&args));
    assert_eq!(b["status"], "planned");
    assert_ne!(a["plan_sha256"], b["plan_sha256"]);
    assert_eq!(json(&lab.run(&args))["plan_sha256"], b["plan_sha256"]);
    // CLI 선택에 없는 실효 설정도 계획을 무효화해야 한다.
    let local = cfg.join("agent.local.toml");
    let text = std::fs::read_to_string(&local).unwrap();
    std::fs::write(&local, format!("{text}\n[reconnect]\nmax_backoff_s = 31\n")).unwrap();
    let changed = json(&lab.run(&args));
    assert_eq!(changed["status"], "planned");
    assert_ne!(changed["plan_sha256"], b["plan_sha256"]);
    let before = snapshot(&lab.0);
    args.pop();
    args.push("fresh".into());
    let conflict = json(&lab.run(&args));
    assert_eq!(conflict["exit_code"], 4);
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn install_rejects_unmounted_storage_and_wrong_binary_without_changes() {
    let lab = Lab::new();
    let mount = lab.0.join("missing-volume");
    let mut args = lab.args();
    let i = args.iter().position(|s| s == "--work-storage").unwrap();
    args.splice(i..i + 2, ["--work-mount".into(), mount.to_string_lossy().into_owned()]);
    args.push("--json".into());
    for leftover in [false, true] {
        if leftover {
            std::fs::create_dir(&mount).unwrap();
        }
        let before = snapshot(&lab.0);
        let v = json(&lab.run(&args));
        assert_eq!(v["exit_code"], 2);
        assert_eq!(v["error"]["code"], "work_mount_unavailable");
        assert_eq!(snapshot(&lab.0), before);
    }
    let binary = lab.0.join("candidate");
    std::fs::write(&binary, CANARY).unwrap();
    let mut args = lab.args();
    args.extend([
        "--json".into(),
        "--from".into(),
        binary.to_string_lossy().into_owned(),
        "--sha256".into(),
        "a".repeat(64),
    ]);
    let before = snapshot(&lab.0);
    let v = json(&lab.run(&args));
    assert_eq!(v["exit_code"], 2);
    assert_eq!(v["error"]["code"], "binary_digest_mismatch");
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn install_does_not_apply_or_execute_selected_files_and_reads_repos_without_writes() {
    use sha2::{Digest, Sha256};
    let lab = Lab::new();
    let candidate = lab.0.join("candidate");
    std::fs::write(&candidate, format!("#!/bin/sh\necho {CANARY}\n")).unwrap();
    let sha = hex::encode(Sha256::digest(std::fs::read(&candidate).unwrap()));
    let mut args = lab.args();
    args.extend(["--json".into(), "--from".into(), candidate.to_string_lossy().into_owned(), "--sha256".into(), sha]);
    let before = snapshot(&lab.0);
    let v = json(&lab.run(&args));
    assert_eq!(v["exit_code"], 5);
    assert_eq!(v["error"]["code"], "binary_platform_mismatch");
    let mut args = lab.args();
    args.retain(|s| s != "--dry-run");
    args.extend(["--json".into(), "--yes".into()]);
    let v = json(&lab.run(&args));
    assert_eq!(v["exit_code"], 5);
    assert_eq!(v["error"]["code"], "install_apply_not_available");
    assert_eq!(snapshot(&lab.0), before);
    let repo = lab.0.join("repo");
    assert!(Command::new("git").args(["init", "--quiet"]).arg(&repo).status().unwrap().success());
    let mut args = lab.args();
    args.extend(["--json".into(), "--repo".into(), repo.to_string_lossy().into_owned()]);
    let before = snapshot(&lab.0);
    let v = json(&lab.run(&args));
    assert_eq!(v["status"], "planned");
    assert!(v["checks"].as_array().unwrap().iter().any(|c| c["name"] == "repo" && c["state"] == "passed"));
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn install_profile_imports_use_the_host_contract_and_reject_secrets_and_shells() {
    let lab = Lab::new();
    let profile = lab.0.join("profiles.toml");
    let mut args: Vec<String> = [
        "install",
        "--role",
        "host",
        "--name",
        "mac-a",
        "--local-agent",
        "disabled",
        "--project-mode",
        "skip",
        "--agent-listen",
        "127.0.0.1:7430",
        "--web",
        "off",
        "--tailscale-serve",
        "off",
        "--pairing",
        "skip",
        "--service",
        "none",
        "--dry-run",
        "--json",
        "--profile-file",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    args.push(profile.to_string_lossy().into_owned());
    args.extend(["--thread-profile".into(), "demo".into()]);
    for (extra, exit) in [
        ("env.LANG = 'ko_KR.UTF-8'".to_string(), 0),
        (format!("env.API_TOKEN = '{CANARY}'"), 2),
        ("command = ['/bin/sh', '-c', 'echo unexpected']".to_string(), 2),
    ] {
        std::fs::write(&profile, format!("schema = 1\n[profiles.demo]\nagent = 'codex'\n{extra}\n")).unwrap();
        let before = snapshot(&lab.0);
        let v = json(&lab.run(&args));
        assert_eq!(v["exit_code"], exit);
        assert_eq!(snapshot(&lab.0), before);
    }
}

#[cfg(unix)]
#[test]
fn install_observes_harness_paths_without_running_their_login_or_version_commands() {
    use std::os::unix::fs::PermissionsExt;
    let lab = Lab::new();
    let tool = lab.0.join("claude");
    std::fs::write(&tool, format!("#!/bin/sh\ntouch \"$HOME/executed\"\necho {CANARY}\n")).unwrap();
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o600)).unwrap();
    let mut args = lab.args();
    let harness = args.iter().position(|s| s == "--harness").unwrap() + 1;
    args[harness] = "claude".into();
    args.extend(["--json".into(), "--claude-path".into(), tool.to_string_lossy().into_owned()]);
    let before = snapshot(&lab.0);
    let missing = json(&lab.run(&args));
    assert_eq!(missing["status"], "needs_user");
    assert_eq!(missing["exit_code"], 3);
    assert_eq!(snapshot(&lab.0), before);
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    let before = snapshot(&lab.0);
    let found = json(&lab.run(&args));
    assert_eq!(found["status"], "planned");
    assert!(
        found["checks"].as_array().unwrap().iter().any(|c| c["name"] == "claude_version" && c["state"] == "unknown")
    );
    assert_eq!(snapshot(&lab.0), before, "dry-run은 harness 조회의 부작용도 만들지 않는다");
}
