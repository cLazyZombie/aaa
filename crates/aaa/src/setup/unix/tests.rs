use super::service::{Manager, Spec};
use super::*;

fn spec(kind: ServiceKind, root: &Path) -> Spec {
    Spec {
        kind,
        name: "aaa-test-example-agent".into(),
        exe: root.join("bin/a $b% c"),
        home: root.into(),
        config: root.join("config"),
        state: root.join("state"),
        instance: Some("example".into()),
        uid: 501,
        user: "example".into(),
        file: root.join("example.service"),
        launcher: None,
        env: [("PATH".into(), "/usr/bin:/bin".into()), ("HOME".into(), root.to_string_lossy().into_owned())]
            .into_iter()
            .collect(),
    }
}

#[test]
fn systemd_argv_quotes_dollars_percent_and_spaces_without_a_shell() {
    let spec = spec(ServiceKind::SystemdSystem, Path::new("/opt/example"));
    spec.validate().unwrap();
    let unit = spec.render();
    assert!(unit.contains("ExecStart=\":/opt/example/bin/a $b%% c\" \"--instance\" \"example\""));
    assert!(unit.contains("User=example\n"));
    assert!(unit.contains("WorkingDirectory=/opt/example\n"));
    assert!(!unit.contains("/bin/sh"));
    let user = Spec { kind: ServiceKind::SystemdUser, ..spec };
    assert!(!user.render().contains("User="));
    assert!(user.render().contains("WantedBy=default.target\n"));
}

#[test]
fn service_inputs_reject_newlines_and_credential_environment() {
    let mut spec = spec(ServiceKind::SystemdUser, Path::new("/opt/example"));
    spec.env.insert("PATH".into(), "/bin\nExecStart=/bin/false".into());
    assert!(spec.validate().is_err());
    spec.env.insert("PATH".into(), "/bin".into());
    spec.env.insert("EXAMPLE_TOKEN".into(), "must-not-be-captured".into());
    assert!(spec.validate().is_err());
}

#[test]
fn service_ownership_refuses_changed_file_and_symlink() {
    let root = std::env::temp_dir().join(format!("aaa-setup-owner-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let manager = Manager { spec: spec(ServiceKind::SystemdUser, &root) };
    assert!(!manager.owned().unwrap());
    std::fs::write(&manager.spec.file, manager.spec.render()).unwrap();
    assert!(manager.owned().unwrap());
    std::fs::write(&manager.spec.file, "foreign service").unwrap();
    assert!(manager.delete().is_err());
    assert_eq!(std::fs::read_to_string(&manager.spec.file).unwrap(), "foreign service");
    std::fs::remove_file(&manager.spec.file).unwrap();
    let other = root.join("other");
    std::fs::write(&other, manager.spec.render()).unwrap();
    std::os::unix::fs::symlink(&other, &manager.spec.file).unwrap();
    assert!(manager.owned().is_err());
    assert!(other.exists());
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn launchd_uses_fixed_launcher_and_explicit_paths() {
    let mut spec = spec(ServiceKind::Launchd, Path::new("/opt/example"));
    spec.launcher = Some("/opt/example/launcher.app/Contents/MacOS/aaa-launcher".into());
    spec.validate().unwrap();
    let plist = spec.render();
    assert!(plist.contains("<key>LimitLoadToSessionType</key>"));
    assert!(plist.contains("<string>/opt/example/launcher.app/Contents/MacOS/aaa-launcher</string>"));
    assert!(plist.contains("<string>setup-service</string>"));
    assert!(plist.contains("<string>--config-dir</string>"));
}
