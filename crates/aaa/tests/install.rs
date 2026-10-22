//! install의 실제 argv·출력·현지 읽기 경계. 실제 launchd 등록 owner는 CI 전용 ignored 시험이다.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_aaa");
const CANARY: &str = "install-secret-canary-DO-NOT-PRINT";
static SEQ: AtomicU32 = AtomicU32::new(0);
#[cfg(windows)]
#[path = "install/job.rs"]
mod job;
struct Lab(PathBuf);
impl Lab {
    fn instance(&self) -> String {
        format!("i{:x}", std::process::id())
    }
    fn new() -> Self {
        Self::in_dir(&std::env::temp_dir())
    }
    fn in_dir(parent: &Path) -> Self {
        #[cfg(windows)]
        job::prepare_runner();
        let p = parent.join(format!("aaa-install-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn command(&self, args: &[&str]) -> Command {
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
            .env("AAA_INSTANCE", self.instance())
            .env("SHELL", if cfg!(windows) { "cmd.exe" } else { "/bin/sh" });
        c
    }
    fn cmd(&self, args: &[&str]) -> Output {
        let out = self.command(args).output().unwrap();
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
#[derive(PartialEq, Eq)]
struct FileView {
    readonly: bool,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    mode: u32,
    bytes: Option<Vec<u8>>,
}
// Equality retains every byte; a failing assertion must not dump lab signing keys/passwords/binaries.
impl std::fmt::Debug for FileView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use sha2::Digest;
        let mut view = f.debug_struct("FileView");
        view.field("readonly", &self.readonly).field("modified", &self.modified);
        #[cfg(unix)]
        view.field("mode", &self.mode);
        view.field(
            "length_sha256",
            &self.bytes.as_ref().map(|bytes| (bytes.len(), hex::encode(sha2::Sha256::digest(bytes)))),
        )
        .finish()
    }
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, FileView> {
    fn visit(root: &Path, at: &Path, out: &mut BTreeMap<PathBuf, FileView>) {
        let meta = at.metadata().unwrap();
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        out.insert(
            at.strip_prefix(root).unwrap().to_owned(),
            FileView {
                readonly: meta.permissions().readonly(),
                modified: meta.modified().ok(),
                #[cfg(unix)]
                mode: meta.permissions().mode(),
                bytes: if meta.is_file() { Some(std::fs::read(at).unwrap()) } else { None },
            },
        );
        if !meta.is_dir() {
            return;
        }
        for entry in std::fs::read_dir(at).unwrap() {
            let p = entry.unwrap().path();
            visit(root, &p, out);
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
    let typed: aaa_proto::install::InstallResult = serde_json::from_value(v.clone()).unwrap();
    assert!(typed.valid_exit(), "v1 status/exit 계약: {v}");
    v
}

#[test]
fn install_errors_are_sanitized_and_never_prompt_or_write() {
    let lab = Lab::new();
    let before = snapshot(&lab.0);
    let malformed_json = format!("--json={CANARY}");
    for args in [
        vec![malformed_json.as_str(), "install", "--dry-run"],
        vec!["--unknown", CANARY, "install", "--json", "--dry-run"],
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
    let malformed_stream = format!("--json-stream={CANARY}");
    let out = lab.cmd(&["install", &malformed_stream]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stderr.is_empty());
    let item: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(item["type"], "result");
    assert_eq!(item["result"]["exit_code"], 2);
    // 다른 명령의 경로/인자에 install이 있어도 그 명령의 오류 계약은 유지한다.
    let other = lab.cmd(&["--config-dir", "install", "completion", "install", "--json"]);
    assert_eq!(other.status.code(), Some(2));
    assert!(other.stdout.is_empty() && !other.stderr.is_empty());
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn install_status_is_read_only_and_input_authorization_cannot_open_writes() {
    let lab = Lab::new();
    let authorization = lab.0.join("authorization.json");
    std::fs::write(&authorization, format!("{{\"approved\":true,\"secret\":\"{CANARY}\"}}")).unwrap();
    let before = snapshot(&lab.0);
    let id = "00000000000000000000000000";
    let mut args = lab.args();
    args.retain(|a| a != "--dry-run");
    args.extend([
        "--json".into(),
        "--yes".into(),
        "--authorization-file".into(),
        authorization.to_string_lossy().into_owned(),
    ]);
    for out in [lab.run(&args), lab.cmd(&["install", "--resume", id, "--yes", "--json"])] {
        let v = json(&out);
        assert_eq!(v["exit_code"], 3);
        assert_eq!(v["status"], "needs_user");
        assert!(v["needs_user"].as_array().unwrap().iter().any(|n| n["code"] == "installation_authorization_required"));
        assert!(v["installation_id"].is_null(), "시작하지 않은 내구 job을 지어내지 않는다");
        assert_eq!(snapshot(&lab.0), before);
    }
    let missing = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(missing["exit_code"], 2);
    assert_eq!(missing["error"]["code"], "installation_not_found");
    let invalid = json(&lab.cmd(&["install", "--status", "../outside", "--json"]));
    assert_eq!(invalid["exit_code"], 2);
    assert_eq!(invalid["error"]["code"], "invalid_installation_id");
    assert_eq!(snapshot(&lab.0), before, "status/apply/resume은 승인 없이 lock/journal/probe도 만들지 않는다");
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
    let file_root = lab.0.join("file-root");
    std::fs::write(&file_root, "work-root must be a directory\n").unwrap();
    let mut args = lab.args();
    let at = args.iter().position(|s| s == "--work-root").unwrap() + 1;
    args[at] = file_root.to_string_lossy().into_owned();
    args.push("--json".into());
    let before = snapshot(&lab.0);
    let v = json(&lab.run(&args));
    assert_eq!(v["exit_code"], 2, "existing regular file is not a work-root");
    assert_eq!(v["error"]["code"], "work_root_not_directory");
    assert_eq!(snapshot(&lab.0), before);
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
    assert_eq!(v["exit_code"], 3);
    assert_eq!(v["status"], "needs_user");
    assert!(v["needs_user"].as_array().unwrap().iter().any(|n| n["code"] == "installation_authorization_required"));
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
    // 빌드 볼륨의 실제 파일을 읽는다. macOS 외장 볼륨에서도 파일을 폴더로 검사하면 안 된다.
    let lab = Lab::in_dir(Path::new(BIN).parent().unwrap());
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

#[cfg(target_os = "macos")]
#[test]
fn install_macos_launchagent_inventory_is_read_only_even_when_service_is_not_requested() {
    let lab = Lab::new();
    let dir = lab.0.join("Library/LaunchAgents");
    std::fs::create_dir_all(&dir).unwrap();
    let plist = dir.join(format!("dev.aaa.{}.agent.plist", lab.instance()));
    std::fs::write(&plist, "<plist><dict/></plist>\n").unwrap();
    let mut args = lab.args();
    args.push("--json".into());
    let before = snapshot(&lab.0);
    let v = json(&lab.run(&args));
    assert_eq!(v["exit_code"], 4);
    assert_eq!(v["error"]["code"], "existing_installation_conflict");
    assert_eq!(snapshot(&lab.0), before);
}

#[cfg(target_os = "linux")]
#[test]
fn install_linux_mount_and_systemd_user_inventory_are_read_only() {
    let lab = Lab::new();
    // stat의 mount 관찰을 별도 oracle로 삼아 /proc parser가 실제 볼륨을 인정하는지 본다.
    let stat = Command::new("stat").args(["-c", "%m"]).arg(&lab.0).output().unwrap();
    assert!(stat.status.success());
    let mount = String::from_utf8(stat.stdout).unwrap();
    let mut args = lab.args();
    let at = args.iter().position(|s| s == "--work-storage").unwrap();
    args.splice(at..at + 2, ["--work-mount".into(), mount.trim().into()]);
    let at = args.iter().position(|s| s == "--service").unwrap() + 1;
    args[at] = "systemd-system".into();
    args.push("--json".into());
    let before = snapshot(&lab.0);
    let v = json(&lab.run(&args));
    assert_eq!(v["status"], "planned");
    assert!(v["checks"].as_array().unwrap().iter().any(|c| c["name"] == "work_mount" && c["state"] == "passed"));
    // booted marker는 manager 조회와 다른 OS 관찰이다. systemd가 없는 환경은 unknown이다.
    let expected = if Path::new("/run/systemd/system").is_dir() { "passed" } else { "unknown" };
    assert!(v["checks"].as_array().unwrap().iter().any(|c| c["name"] == "service_manager" && c["state"] == expected));
    assert_eq!(snapshot(&lab.0), before);
    // AAA가 쓰는 XDG_CONFIG_HOME과 같은 실제 user-unit 위치다. daemon-reload/등록은 하지 않는다.
    args[at] = "systemd-user".into();
    let dir = lab.0.join("systemd/user");
    std::fs::create_dir_all(&dir).unwrap();
    let unit = dir.join(format!("aaa-{}-agent.service", lab.instance()));
    std::fs::write(&unit, "[Service]\nExecStart=/usr/bin/false\n").unwrap();
    let before = snapshot(&lab.0);
    let v = json(&lab.run(&args));
    assert_eq!(v["exit_code"], 4);
    assert_eq!(v["error"]["code"], "existing_installation_conflict");
    assert_eq!(snapshot(&lab.0), before);
    let at = args.iter().position(|s| s == "--work-root").unwrap() + 1;
    args[at] = "relative/work".into();
    let v = json(&lab.run(&args));
    assert_eq!(v["exit_code"], 2);
    assert_eq!(snapshot(&lab.0), before);
}

#[cfg(windows)]
#[test]
fn install_windows_drive_mount_and_scm_queries_are_read_only() {
    let lab = Lab::new();
    let canonical = lab.0.canonicalize().unwrap();
    let mount = canonical.ancestors().last().unwrap().to_string_lossy().into_owned();
    let mut args = lab.args();
    let at = args.iter().position(|s| s == "--work-storage").unwrap();
    args.splice(at..at + 2, ["--work-mount".into(), mount]);
    let at = args.iter().position(|s| s == "--service").unwrap() + 1;
    args[at] = "windows-scm".into();
    args.push("--json".into());
    let before = snapshot(&lab.0);
    let v = json(&lab.run(&args));
    assert_eq!(v["status"], "planned");
    for name in ["service_manager", "work_mount"] {
        assert!(v["checks"].as_array().unwrap().iter().any(|c| c["name"] == name && c["state"] == "passed"));
    }
    assert!(
        v["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["name"] == "write_permission_preflight" && c["state"] == "unknown")
    );
    assert_eq!(snapshot(&lab.0), before);
    // drive 상대/불완전 UNC는 filesystem/SCM을 바꾸기 전의 계약 오류다.
    let at = args.iter().position(|s| s == "--work-root").unwrap() + 1;
    for bad in ["C:relative", "\\\\example-server"] {
        args[at] = bad.into();
        let v = json(&lab.run(&args));
        assert_eq!(v["exit_code"], 2);
        assert_eq!(snapshot(&lab.0), before);
    }
}

#[test]
fn internal_service_entry_requires_the_os_supervisor_and_never_falls_back_to_console_backend() {
    let lab = Lab::new();
    let before = snapshot(&lab.0);
    let context = lab.0.join("untrusted-context.json");
    let out = lab.cmd(&["service-run", "--context", context.to_str().unwrap(), "--context-sha256", &"0".repeat(64)]);
    assert_eq!(out.status.code(), Some(if cfg!(any(windows, target_os = "macos")) { 3 } else { 5 }));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains(if cfg!(any(windows, target_os = "macos")) {
        "service_controller_required"
    } else {
        "unsupported_service_platform"
    }));
    assert_eq!(snapshot(&lab.0), before);
}

#[path = "install/authorization.rs"]
mod authorization;
#[path = "install/recovery.rs"]
mod recovery;

#[path = "install/grant.rs"]
mod grant;

#[path = "install/wizard.rs"]
mod wizard;

#[path = "install/network.rs"]
mod network;

#[path = "install/worker_lifetime.rs"]
mod worker_lifetime;

#[path = "install/pairing.rs"]
mod pairing;

#[cfg(target_os = "macos")]
#[path = "install/launchd.rs"]
mod launchd;
