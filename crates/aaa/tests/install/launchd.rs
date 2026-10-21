//! 기존 macos-15 일회성 계정에서만 실행하는 필수 실제 LaunchAgent owner.
use super::authorization::terminal_steps;
use super::*;

#[path = "launchd_args.rs"]
mod launchd_args;

fn ctl(args: &[&str]) -> Output {
    Command::new("/bin/launchctl").args(args).output().unwrap()
}
struct OwnedLabels(Vec<String>);
impl Drop for OwnedLabels {
    fn drop(&mut self) {
        for label in &self.0 {
            let _ = ctl(&["bootout", label]);
        }
    }
}
fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}
fn arguments(lab: &Lab, instance: &str, source: &Path, launcher: &Path) -> Vec<String> {
    let mut args: Vec<String> = [
        "install",
        "--role",
        "host",
        "--name",
        "host-a",
        "--local-agent",
        "disabled",
        "--project-mode",
        "skip",
        "--web",
        "off",
        "--tailscale-serve",
        "off",
        "--pairing",
        "skip",
        "--service",
        "launchd",
        "--launcher-policy",
        "install",
        "--start",
        "now",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let root = lab.0.join(instance);
    for (key, value) in [
        ("--instance", instance.to_owned()),
        ("--from", source.to_string_lossy().into_owned()),
        ("--sha256", {
            use sha2::Digest;
            hex::encode(sha2::Sha256::digest(std::fs::read(source).unwrap()))
        }),
        ("--launcher", launcher.to_string_lossy().into_owned()),
        ("--bin-path", root.join("bin/aaa").to_string_lossy().into_owned()),
        ("--config-dir", root.join("config").to_string_lossy().into_owned()),
        ("--state-dir", root.join("state").to_string_lossy().into_owned()),
        ("--agent-listen", format!("127.0.0.1:{}", port())),
    ] {
        args.extend([key.into(), value]);
    }
    args
}
fn service_tail(root: &Path) -> String {
    let mut lines = Vec::new();
    let jobs = root.join("state/install/jobs");
    if let Ok(jobs) = std::fs::read_dir(jobs) {
        for job in jobs.flatten() {
            for role in ["host", "agent"] {
                if let Ok(text) = std::fs::read_to_string(job.path().join("services").join(role).join("service.log")) {
                    lines.extend(text.lines().rev().take(12).map(str::to_owned).collect::<Vec<_>>().into_iter().rev());
                }
            }
        }
    }
    lines.join("\n")
}
fn authorize_apply(lab: &Lab, instance: &str, source: &Path, launcher: &Path) -> Value {
    let mut args = arguments(lab, instance, source, launcher);
    let approval = lab.0.join(instance).with_extension("approval.json");
    let issue = launchd_args::authorize(args.clone(), &approval);
    let (code, text) = terminal_steps(lab, &issue, &[("정확히 yes", "yes\r")], false);
    assert_eq!(code, 0, "{text}");
    args.extend([
        "--authorization-file".into(),
        approval.to_string_lossy().into_owned(),
        "--non-interactive".into(),
        "--yes".into(),
        "--json".into(),
    ]);
    let output = lab.run(&args);
    assert!(
        output.status.success(),
        "launchd apply status={:?}, stdout={}, stderr={}, service_tail={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        service_tail(&lab.0.join(instance))
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn package(lab: &Lab) -> (PathBuf, PathBuf) {
    let factory = lab.0.join("factory");
    std::fs::create_dir(&factory).unwrap();
    let source = factory.join("aaa");
    std::fs::copy(BIN, &source).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let built = Command::new("bash")
        .arg(repo.join("p0/launcher/install.sh"))
        .arg("--dest")
        .arg(factory.join("bundle"))
        .output()
        .unwrap();
    assert!(built.status.success(), "launcher build: {}", String::from_utf8_lossy(&built.stderr));
    let package = factory.join(format!("aaa-launcher-macos-{}.json", std::env::consts::ARCH));
    let version = String::from_utf8(lab.cmd(&["--version"]).stdout).unwrap();
    let version = version.trim().strip_prefix("aaa ").unwrap();
    let packed = Command::new("python3")
        .arg(repo.join("scripts/launcher_asset.py"))
        .arg("--app")
        .arg(factory.join("bundle/aaa-launcher.app"))
        .arg("--binary")
        .arg(&source)
        .arg("--platform")
        .arg(format!("macos-{}", std::env::consts::ARCH))
        .arg("--version")
        .arg(version)
        .arg("--output")
        .arg(&package)
        .output()
        .unwrap();
    assert!(packed.status.success(), "launcher pack: {}", String::from_utf8_lossy(&packed.stderr));
    let launcher = lab.0.join("Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher");
    (source, launcher)
}

#[test]
fn launchd_plan_requires_the_matching_launcher_without_registering_or_creating_install_files() {
    let lab = Lab::new();
    let (source, launcher) = package(&lab);
    let mut args = arguments(&lab, "plan", &source, &launcher);
    args.extend(["--dry-run".into(), "--json".into()]);
    let before = snapshot(&lab.0);
    let output = lab.run(&args);
    assert!(
        output.status.success(),
        "dry-run: {} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["status"], "planned", "{value}");
    assert_eq!(snapshot(&lab.0), before);
    let asset = source.parent().unwrap().join(format!("aaa-launcher-macos-{}.json", std::env::consts::ARCH));
    std::fs::remove_file(asset).unwrap();
    let before = snapshot(&lab.0);
    let output = lab.run(&args);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["exit_code"], 3, "{value}");
    assert!(
        value["needs_user"].as_array().unwrap().iter().any(|n| n["code"] == "installation_launcher_asset_required"),
        "{value}"
    );
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
#[ignore = "실제 GUI LaunchAgent 변경은 승인된 macos-15 CI 전용 필수 단계에서 실행"]
fn launchd_real_registration_ready_resume_and_second_instance_preserve_the_shared_launcher() {
    assert_eq!(
        std::env::var("GITHUB_ACTIONS").as_deref(),
        Ok("true"),
        "이 실제 등록 owner는 기존 GitHub macos-15 일회성 계정 전용이다"
    );
    let lab = Lab::new();
    let uid = unsafe { libc::geteuid() };
    let domain = format!("gui/{uid}");
    let probe = ctl(&["print", &domain]);
    assert!(probe.status.success(), "필수 launchd GUI 도메인 없음: {}", String::from_utf8_lossy(&probe.stderr));
    let (source, launcher) = package(&lab);
    let one = format!("i{:x}a", std::process::id());
    let two = format!("i{:x}b", std::process::id());
    let mut labels = OwnedLabels(vec![format!("{domain}/dev.aaa.{one}.host")]);
    let first = authorize_apply(&lab, &one, &source, &launcher);
    assert_eq!(first["exit_code"], 0);
    assert_eq!(first["status"], "completed");
    assert!(
        first["checks"].as_array().unwrap().iter().any(|v| v["name"] == "service_host_ready" && v["state"] == "passed"),
        "{first}"
    );
    let id = first["installation_id"].as_str().unwrap();
    let state = lab.0.join(&one).join("state");
    let stopped = ctl(&["bootout", &labels.0[0]]);
    assert!(stopped.status.success(), "first stop: {}", String::from_utf8_lossy(&stopped.stderr));
    let resumed = lab
        .command(&[
            "install",
            "--instance",
            &one,
            "--state-dir",
            state.to_str().unwrap(),
            "--resume",
            id,
            "--non-interactive",
            "--yes",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "same-ID resume: {} / {}",
        String::from_utf8_lossy(&resumed.stdout),
        String::from_utf8_lossy(&resumed.stderr)
    );
    let resumed: Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(resumed["installation_id"], id);
    assert!(
        resumed["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "service_host_ready" && v["state"] == "passed")
    );
    let bytes = std::fs::read(&launcher).unwrap();
    let first_plist = lab.0.join(format!("Library/LaunchAgents/dev.aaa.{one}.host.plist"));
    let before = std::fs::read(&first_plist).unwrap();
    labels.0.push(format!("{domain}/dev.aaa.{two}.host"));
    let second = authorize_apply(&lab, &two, &source, &launcher);
    assert_eq!(second["exit_code"], 0);
    assert_eq!(std::fs::read(&launcher).unwrap(), bytes);
    assert_eq!(std::fs::read(first_plist).unwrap(), before);
    let output = lab
        .command(&["install", "--instance", &one, "--state-dir", state.to_str().unwrap(), "--status", id, "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "status: {}", String::from_utf8_lossy(&output.stdout));
    // 정상 사용자 service 해제 경로로 자신의 두 label/plist만 제거한다.
    for instance in [&one, &two] {
        let args = vec![
            "service".into(),
            "uninstall".into(),
            "--role".into(),
            "host".into(),
            "--instance".into(),
            instance.clone(),
        ];
        let (code, text) = terminal_steps(&lab, &args, &[("계속하려면 yes", "yes\r")], false);
        assert_eq!(code, 0, "uninstall {instance}: {text}");
        assert!(!lab.0.join(format!("Library/LaunchAgents/dev.aaa.{instance}.host.plist")).exists());
    }
    labels.0.clear();
    assert_eq!(std::fs::read(&launcher).unwrap(), bytes);
}
