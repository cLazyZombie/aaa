//! package producer→실제 파일 transaction owner. 운영 app/launchd는 변경하지 않는다.
use super::super::tests::{Lab, tree};
use super::*;
use std::process::Command;

fn fixture(lab: &Lab) -> (InstallPlan, PathBuf) {
    let mut plan = lab.plan("state");
    let launcher = lab.0.join("Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher");
    plan.choices.service = Some(InstallService::Launchd);
    plan.choices.launcher_policy = Some(InstallLauncherPolicy::Install);
    plan.choices.launcher = Some(launcher.to_string_lossy().into_owned());
    plan = aaa_config::install::plan(plan.choices, plan.context).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let factory = lab.0.join("producer");
    let output =
        Command::new("bash").arg(repo.join("p0/launcher/install.sh")).arg("--dest").arg(&factory).output().unwrap();
    assert!(output.status.success(), "launcher producer: {}", String::from_utf8_lossy(&output.stderr));
    let asset = super::super::super::launcher_asset::source(&plan).unwrap();
    let output = Command::new("python3")
        .arg(repo.join("scripts/launcher_asset.py"))
        .arg("--app")
        .arg(factory.join("aaa-launcher.app"))
        .arg("--binary")
        .arg(plan.choices.from.as_ref().unwrap())
        .arg("--platform")
        .arg(format!("macos-{}", std::env::consts::ARCH))
        .arg("--version")
        .arg(&plan.context.version)
        .arg("--output")
        .arg(&asset)
        .output()
        .unwrap();
    assert!(output.status.success(), "launcher package: {}", String::from_utf8_lossy(&output.stderr));
    (plan, asset)
}
#[test]
fn launcher_package_uses_the_actual_file_transaction_and_preserves_a_shared_existing_app() {
    let lab = Lab::new();
    let (plan, _) = fixture(&lab);
    let permissions = super::super::permissions(&plan).unwrap();
    assert_eq!(permissions.iter().filter(|p| p.operation == InstallOperation::StageLauncher).count(), 4);
    let issuer =
        crate::install::engine::authorization::Authority::initialize(Path::new(&plan.context.state_dir)).unwrap();
    let approval = issuer.issue(&plan, permissions.clone(), 1000, 100).unwrap();
    let lease = issuer.begin(&approval, &plan, &permissions, 1000, None).unwrap();
    let mut first = super::super::AuthorizedTransaction::create(plan.clone(), &lease).unwrap();
    first.run().unwrap();
    let id = first.id().to_owned();
    drop(first);
    drop(lease);
    let launcher = Path::new(plan.choices.launcher.as_ref().unwrap());
    let app = launcher.parent().unwrap().parent().unwrap().parent().unwrap();
    let output = Command::new("/usr/bin/codesign").args(["--verify", "--strict"]).arg(app).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let before = std::fs::read(launcher).unwrap();
    let mut repeat = Transaction::create(install(plan.clone()).unwrap()).unwrap();
    assert!(repeat.reused);
    assert_eq!(repeat.run().unwrap().status, InstallStatus::Unchanged);
    drop(repeat);
    let mut second = plan.clone();
    second.choices.instance = Some("other".into());
    second.context.state_dir = lab.0.join("state-other").to_string_lossy().into_owned();
    second.choices.state_dir = Some(second.context.state_dir.clone());
    second.context.config_dir = lab.0.join("config-other").to_string_lossy().into_owned();
    second.choices.config_dir = Some(second.context.config_dir.clone());
    second.choices.work_root = Some(lab.0.join("work-other").to_string_lossy().into_owned());
    second.choices.default_cwd = second.choices.work_root.clone();
    second = aaa_config::install::plan(second.choices, second.context).unwrap();
    let mut other = Transaction::create(install(second).unwrap()).unwrap();
    other.run().unwrap();
    drop(other);
    let mut first =
        Transaction::resume(Path::new(&plan.context.state_dir), &id, plan.choices.instance.as_deref()).unwrap();
    first.uninstall(false).unwrap();
    assert_eq!(std::fs::read(launcher).unwrap(), before);
}
#[test]
fn foreign_launcher_packages_and_paths_fail_before_any_file_change() {
    let lab = Lab::new();
    let (plan, asset) = fixture(&lab);
    let original: serde_json::Value = serde_json::from_slice(&std::fs::read(&asset).unwrap()).unwrap();
    for field in ["schema", "version", "platform", "binary_sha256", "escape", "missing", "base64"] {
        let mut value = original.clone();
        match field {
            "schema" => value[field] = serde_json::json!(2),
            "binary_sha256" => value[field] = serde_json::json!("0".repeat(64)),
            "escape" => value["files"]["../escape"] = serde_json::json!("dGVzdA=="),
            "missing" => {
                value["files"].as_object_mut().unwrap().remove("Contents/Info.plist");
            }
            "base64" => value["files"]["Contents/MacOS/aaa-launcher"] = serde_json::json!("bad base64!?"),
            _ => value[field] = serde_json::json!("foreign"),
        }
        std::fs::write(&asset, serde_json::to_vec(&value).unwrap()).unwrap();
        let before = tree(&lab.0);
        assert!(install(plan.clone()).is_err(), "{field}");
        assert_eq!(tree(&lab.0), before, "{field}");
    }
}

#[test]
fn preserving_a_launcher_binds_its_bytes_to_the_approval_before_any_transaction_write() {
    use std::io::Write;
    let lab = Lab::new();
    let (mut plan, _) = fixture(&lab);
    let launcher = lab.0.join("producer/aaa-launcher.app/Contents/MacOS/aaa-launcher");
    plan.choices.launcher = Some(launcher.to_string_lossy().into_owned());
    plan.choices.launcher_policy = Some(InstallLauncherPolicy::Preserve);
    plan = aaa_config::install::plan(plan.choices, plan.context).unwrap();
    let allowed = super::super::permissions(&plan).unwrap();
    assert!(
        allowed.iter().any(|p| p.operation == InstallOperation::StageLauncher
            && p.target == launcher.to_str().unwrap()
            && p.before_sha256 == p.after_sha256
            && p.before_sha256.is_some()),
        "보존하는 실행 파일도 검토한 bytes에 묶어야 한다"
    );
    let issuer =
        crate::install::engine::authorization::Authority::initialize(Path::new(&plan.context.state_dir)).unwrap();
    // initialize/issue의 짧은 잠금 scope가 끝난 뒤 fork/dup 사본만 남은 경계다.
    #[cfg(unix)]
    let inherited_issuer = {
        let scope =
            private::lock(&Path::new(&plan.context.state_dir).join("install/authorization/issuer.lock")).unwrap();
        scope.try_clone().unwrap()
    };
    let approval = issuer.issue(&plan, allowed.clone(), 1000, 100).unwrap();
    let lease = issuer.begin(&approval, &plan, &allowed, 1000, None).unwrap();
    #[cfg(unix)]
    drop(inherited_issuer);
    std::fs::OpenOptions::new().append(true).open(&launcher).unwrap().write_all(b"changed after review").unwrap();
    let before = tree(&lab.0);
    assert!(super::super::AuthorizedTransaction::create(plan, &lease).is_err());
    assert_eq!(tree(&lab.0), before);
}
