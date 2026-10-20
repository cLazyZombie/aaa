use super::*;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

const CANARY: &str = "install-secret-canary-DO-NOT-PRINT";
static SEQ: AtomicU32 = AtomicU32::new(0);
pub(in crate::install::engine) struct Lab(pub(in crate::install::engine) PathBuf);
impl Lab {
    pub(in crate::install::engine) fn new() -> Self {
        let path = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "aaa-install-engine-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        private::directory(&path).unwrap();
        Self(path)
    }
    pub(super) fn put(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        private::parents(path.parent().unwrap()).unwrap();
        private::write(&path, bytes).unwrap();
        path
    }
    pub(super) fn plan(&self, state: &str) -> InstallPlan {
        let source = self.put("source", b"verified executable bytes");
        let state = self.0.join(state);
        let cfg = self.0.join(format!("{}-config", state.file_name().unwrap().to_string_lossy()));
        let choices = InstallChoices {
            role: Some(InstallRole::Agent),
            name: Some("mac-a".into()),
            instance: Some("lab".into()),
            config_dir: Some(cfg.to_string_lossy().into_owned()),
            state_dir: Some(state.to_string_lossy().into_owned()),
            host: Some("example.ts.net:7430".into()),
            network: Some(InstallNetwork::Direct),
            enroll: Some(InstallEnroll::Skip),
            work_root: Some(
                self.0
                    .join(format!("{}-work", state.file_name().unwrap().to_string_lossy()))
                    .to_string_lossy()
                    .into_owned(),
            ),
            work_storage: Some(InstallStorage::Internal),
            min_free_gb: Some(7),
            harness: vec![InstallHarness::None],
            hooks: Some(InstallHooks::Preserve),
            service: Some(InstallService::None),
            start: Some(InstallStart::Deferred),
            from: Some(source.to_string_lossy().into_owned()),
            sha256: Some(hash(b"verified executable bytes")),
            ..Default::default()
        };
        let context = InstallPlanContext {
            platform: crate::install::platform::platform().unwrap_or_else(|_| panic!("시험 OS")),
            version: "0.1.2".into(),
            binary_sha256: hash(b"verified executable bytes"),
            bin_path: self.0.join("bin/aaa").to_string_lossy().into_owned(),
            config_dir: cfg.to_string_lossy().into_owned(),
            state_dir: state.to_string_lossy().into_owned(),
            projects_root: self.0.join("projects").to_string_lossy().into_owned(),
            default_shell: if cfg!(windows) { "cmd.exe".into() } else { "/bin/sh".into() },
            effective_config_sha256: hash(b"effective config"),
            target_account: crate::install::platform::account().unwrap(),
        };
        aaa_config::install::plan(choices, context).unwrap()
    }
    pub(super) fn edit(&self, name: &str, after: &[u8]) -> Proposed {
        let path = self.0.join(name);
        let before = std::fs::read(&path).ok();
        prepare::proposal(path, Kind::Config, before, after.to_vec(), None, true, false).unwrap()
    }
    pub(super) fn request(&self, state: &str, files: Vec<Proposed>) -> Request {
        Request { plan: self.plan(state), files, root: None, directories: Vec::new() }
    }
    pub(in crate::install::engine) fn configured_service(&self) -> (InstallPlan, String) {
        let mut plan = self.plan("state");
        if cfg!(windows) {
            plan.choices.service = Some(InstallService::WindowsScm);
            plan.choices.service_account = Some(InstallAccount::CurrentUser);
            plan.choices.startup = Some(InstallStartup::Auto);
            plan.choices.service_password_source = Some(InstallPasswordSource::Prompt);
        }
        let plan = aaa_config::install::plan(plan.choices, plan.context).unwrap();
        let mut tx = Transaction::create(prepare::install(plan.clone()).unwrap()).unwrap();
        tx.run().unwrap();
        (plan, tx.journal.id.clone())
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
pub(in crate::install::engine) fn tree(
    path: &Path,
) -> BTreeMap<PathBuf, (Vec<u8>, u32, Option<std::time::SystemTime>)> {
    fn visit(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, (Vec<u8>, u32, Option<std::time::SystemTime>)>) {
        let meta = path.symlink_metadata().unwrap();
        out.insert(
            path.strip_prefix(root).unwrap().into(),
            (if meta.is_file() { std::fs::read(path).unwrap() } else { Vec::new() }, mode(&meta), meta.modified().ok()),
        );
        if meta.is_dir() {
            for entry in std::fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), out);
            }
        }
    }
    let mut view = BTreeMap::new();
    visit(path, path, &mut view);
    view
}

#[test]
fn file_steps_resume_after_rename_without_rewriting_or_leaking_private_payloads() {
    let lab = Lab::new();
    let a = lab.put("config/a.toml", format!("old={CANARY}").as_bytes());
    let mut tx = Transaction::create(lab.request(
        "state",
        vec![lab.edit("config/a.toml", format!("new={CANARY}").as_bytes()), lab.edit("config/b.toml", b"second=1\n")],
    ))
    .unwrap();
    tx.stage(0).unwrap();
    let id = tx.journal.id.clone();
    let state = tx.state.clone();
    // 실제 rename 뒤 journal 쓰기 전에 프로세스가 끊긴 상태. 승인 우회 hook을 넣지 않는다.
    replace(&tx.stage_path(0), &a, true).unwrap();
    let modified = a.metadata().unwrap().modified().ok();
    drop(tx);
    let mut resumed = Transaction::resume(&state, &id, Some("lab")).unwrap();
    let result = resumed.run().unwrap();
    assert_eq!(result.status, InstallStatus::Configured);
    assert_eq!(a.metadata().unwrap().modified().ok(), modified);
    assert_eq!(std::fs::read(&a).unwrap(), format!("new={CANARY}").as_bytes());
    assert_eq!(std::fs::read(lab.0.join("config/b.toml")).unwrap(), b"second=1\n");
    let journal = std::fs::read_to_string(resumed.dir.join("journal.json")).unwrap();
    assert!(!journal.contains(CANARY));
    assert!(!serde_json::to_string(&result).unwrap().contains(CANARY));
    private::verify(&File::open(resumed.dir.join("backup-0")).unwrap(), false).unwrap();
    let before = tree(&lab.0);
    let status = crate::install::engine::status(&state, Some("lab"), &id).unwrap_or_else(|_| panic!("실제 job 상태"));
    assert_eq!(status.status, InstallStatus::Configured);
    assert_eq!(tree(&lab.0), before);
    lab.put("config/a.toml", b"external editor\n");
    let changed = tree(&lab.0);
    assert_eq!(crate::install::engine::status(&state, Some("lab"), &id).err().unwrap().exit_code, 4);
    assert_eq!(tree(&lab.0), changed);
}

#[test]
fn edits_before_prepare_or_between_stage_and_commit_are_preserved() {
    let lab = Lab::new();
    let file = lab.put("config/value", b"original");
    let request = lab.request("state", vec![lab.edit("config/value", b"installer")]);
    lab.put("config/value", b"before prepare edit");
    assert!(Transaction::create(request).is_err());
    assert_eq!(std::fs::read(&file).unwrap(), b"before prepare edit");
    let mut tx = Transaction::create(lab.request("state", vec![lab.edit("config/value", b"installer")])).unwrap();
    tx.stage(0).unwrap();
    lab.put("config/value", b"between stage and commit");
    let before = tree(&lab.0);
    assert_eq!(crate::install::engine::status(&tx.state, Some("lab"), &tx.journal.id).err().unwrap().exit_code, 4);
    assert_eq!(tree(&lab.0), before);
    assert_eq!(tx.commit(0).err(), Some(Fault::conflict()));
    assert_eq!(std::fs::read(file).unwrap(), b"between stage and commit");
    let other = Lab::new();
    let file = other.put("config/value", b"original");
    let mut tx = Transaction::create(other.request("state", vec![other.edit("config/value", b"installer")])).unwrap();
    tx.stage(0).unwrap();
    other.put("config/value", b"installer");
    // 재개 때 temp가 남아 있다고 가정하지 않고 실제 파일 inode로 소유를 증명한다.
    std::fs::remove_file(tx.stage_path(0)).unwrap();
    let before = tree(&other.0);
    assert_eq!(tx.advance().err(), Some(Fault::conflict()), "같은 바이트의 외부 교체도 commit 소유 증명이 아니다");
    assert_eq!(std::fs::read(file).unwrap(), b"installer");
    assert_eq!(tree(&other.0), before);
}

#[test]
fn resource_locks_serialize_processes() {
    if let Some(state) = std::env::var_os("AAA_INSTALL_TEST_LOCK_STATE") {
        let id = std::env::var("AAA_INSTALL_TEST_LOCK_ID").unwrap();
        let state = Path::new(&state);
        let error = if std::env::var("AAA_INSTALL_TEST_LOCK_SAME_CONFIG").as_deref() == Ok("1") {
            let journal = read_journal(state, &id, Some("lab")).unwrap();
            let mut plan = load(state, &journal).unwrap().plan;
            let path = Path::new(&plan.context.config_dir).join("different-file");
            let other_state = state.with_file_name("other-state");
            plan.context.state_dir = other_state.to_string_lossy().into_owned();
            plan.choices.state_dir = Some(plan.context.state_dir.clone());
            let file =
                prepare::proposal(path, Kind::Config, None, b"other installer".to_vec(), None, true, false).unwrap();
            Transaction::create(Request { plan, files: vec![file], root: None, directories: Vec::new() }).err().unwrap()
        } else {
            Transaction::resume(state, &id, Some("lab")).err().unwrap()
        };
        assert_eq!(error.code, "installation_resource_busy");
        return;
    }
    let lab = Lab::new();
    let tx = Transaction::create(lab.request("state", vec![lab.edit("config/value", b"new")])).unwrap();
    for same_config in [false, true] {
        let before = tree(&lab.0);
        let result = crate::install::engine::status(&tx.state, Some("lab"), &tx.journal.id).unwrap();
        assert!(result.status == InstallStatus::NeedsUser);
        assert_eq!(tree(&lab.0), before, "다른 적용은 막되 읽기 status/snapshot은 가능해야 한다");
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "install::engine::transaction::tests::resource_locks_serialize_processes", "--nocapture"])
            .env("AAA_INSTALL_TEST_LOCK_STATE", &tx.state)
            .env("AAA_INSTALL_TEST_LOCK_ID", &tx.journal.id)
            .env("AAA_INSTALL_TEST_LOCK_SAME_CONFIG", if same_config { "1" } else { "0" })
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "same_config={same_config}: {} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let state = tx.state.clone();
    let id = tx.journal.id.clone();
    drop(tx);
    assert!(Transaction::resume(&state, &id, Some("lab")).is_ok());
}

#[test]
fn prepared_plan_reuses_root_and_preserves_local_settings_comments_and_second_instance() {
    let lab = Lab::new();
    let plan = lab.plan("first");
    let cfg = PathBuf::from(&plan.context.config_dir);
    private::parents(&cfg).unwrap();
    private::write(
        &cfg.join("agent.toml"),
        format!("schema = 1\n# keep base\n[health]\nmin_free_gb = 5\n[future]\ntext = '{CANARY}'\n").as_bytes(),
    )
    .unwrap();
    private::write(&cfg.join("agent.local.toml"), b"schema = 1\n# keep local\n[health]\nmin_free_gb = 9\n").unwrap();
    let mut tx = Transaction::create(prepare::install(plan.clone()).unwrap()).unwrap();
    tx.run().unwrap();
    let id = tx.journal.id.clone();
    drop(tx);
    let base = std::fs::read_to_string(cfg.join("agent.toml")).unwrap();
    assert!(base.contains("# keep base") && base.contains(CANARY) && base.contains("min_free_gb = 5"));
    let local = std::fs::read_to_string(cfg.join("agent.local.toml")).unwrap();
    assert!(local.contains("# keep local") && local.contains("min_free_gb = 7"));
    let loaded = aaa_config::load_agent(&cfg).unwrap();
    assert_eq!(loaded.config.agent.name, "mac-a");
    assert_eq!(loaded.config.health.min_free_gb, 7);
    let before = tree(&lab.0);
    let mut again = Transaction::create(prepare::install(plan.clone()).unwrap()).unwrap();
    assert_eq!(again.run().unwrap().status, InstallStatus::Unchanged);
    assert_eq!(again.journal.id, id);
    drop(again);
    assert_eq!(tree(&lab.0), before);
    let mut second = lab.plan("second");
    second.choices.instance = Some("other".into());
    let mut other = Transaction::create(prepare::install(second).unwrap()).unwrap();
    other.run().unwrap();
    assert_eq!(std::fs::read(cfg.join("agent.toml")).unwrap(), base.as_bytes());
    assert_eq!(std::fs::read(cfg.join("agent.local.toml")).unwrap(), local.as_bytes());
    let root = PathBuf::from(plan.choices.work_root.unwrap());
    assert!(root.join(".aaa-root-owner.json").is_file());
}

#[test]
fn incomplete_job_blocks_a_new_apply_and_status_never_repairs_missing_root_marker() {
    let lab = Lab::new();
    let request = lab.request("state", vec![lab.edit("config/value", b"new")]);
    let tx = Transaction::create(request).unwrap();
    let state = tx.state.clone();
    let id = tx.journal.id.clone();
    drop(tx);
    let next = lab.request("state", vec![lab.edit("config/value", b"different")]);
    let before = tree(&lab.0);
    assert_eq!(Transaction::create(next).err(), Some(Fault::conflict()));
    assert_eq!(tree(&lab.0), before);
    let mut resumed = Transaction::resume(&state, &id, Some("lab")).unwrap();
    resumed.run().unwrap();
    drop(resumed);

    let root_lab = Lab::new();
    let plan = root_lab.plan("state");
    let marker = PathBuf::from(plan.choices.work_root.clone().unwrap()).join(".aaa-root-owner.json");
    let mut tx = Transaction::create(prepare::install(plan).unwrap()).unwrap();
    tx.run().unwrap();
    std::fs::remove_file(&marker).unwrap();
    let before = tree(&root_lab.0);
    assert_eq!(crate::install::engine::status(&tx.state, Some("lab"), &tx.journal.id).err().unwrap().exit_code, 4);
    assert!(!marker.exists());
    assert_eq!(tree(&root_lab.0), before, "status는 root probe/복구/잠금을 쓰지 않는다");
}

#[test]
fn unsafe_targets_readonly_and_untrusted_journals_preserve_originals() {
    let lab = Lab::new();
    let file = lab.put("config/value", b"original");
    let request = lab.request("state", vec![lab.edit("config/value", b"new")]);
    let mut tx = Transaction::create(request).unwrap();
    let original_permissions = file.metadata().unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    std::fs::set_permissions(&file, permissions).unwrap();
    assert_eq!(tx.stage(0).err(), Some(Fault::conflict()));
    assert_eq!(std::fs::read(&file).unwrap(), b"original");
    // Windows의 readonly 파일은 임시 폴더 정리 전에 원래 쓰기 권한으로 돌린다.
    #[cfg(windows)]
    {
        std::fs::set_permissions(&file, original_permissions).unwrap();
    }
    let journal = tx.dir.join("journal.json");
    private::write(&journal, format!("{{\"schema\":999,\"secret\":\"{CANARY}\"}}").as_bytes()).unwrap();
    let before = tree(&lab.0);
    let error = crate::install::engine::status(&tx.state, Some("lab"), &tx.journal.id).err().unwrap();
    assert_eq!(error.exit_code, 2);
    assert!(!serde_json::to_string(&error).unwrap().contains(CANARY));
    assert_eq!(tree(&lab.0), before);
    assert!(target(&lab.0.join("config/../escape")).is_err());
    assert!(!lab.0.join("escape").exists());
    let parent = lab.put("file-parent", b"not directory");
    assert!(prepare::proposal(parent.join("child"), Kind::Config, None, b"new".to_vec(), None, true, false).is_err());
}

#[cfg(unix)]
#[test]
fn unix_links_and_private_permissions_cannot_redirect_status_or_apply() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let lab = Lab::new();
    let original = lab.put("original", b"private original");
    let link = lab.0.join("link");
    symlink(&original, &link).unwrap();
    assert!(
        prepare::proposal(link, Kind::Config, Some(b"private original".to_vec()), b"new".to_vec(), None, true, false)
            .is_err()
    );
    let hard = lab.0.join("hardlink");
    std::fs::hard_link(&original, &hard).unwrap();
    assert!(
        prepare::proposal(hard, Kind::Config, Some(b"private original".to_vec()), b"new".to_vec(), None, true, false)
            .is_err()
    );
    let tx = Transaction::create(lab.request("state", vec![lab.edit("config/value", b"new")])).unwrap();
    let journal = tx.dir.join("journal.json");
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o644)).unwrap();
    let before = tree(&lab.0);
    assert_eq!(crate::install::engine::status(&tx.state, Some("lab"), &tx.journal.id).err().unwrap().exit_code, 4);
    assert_eq!(tree(&lab.0), before);
    std::fs::set_permissions(&journal, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&journal, lab.0.join("journal-copy")).unwrap();
    let before = tree(&lab.0);
    assert_eq!(crate::install::engine::status(&tx.state, Some("lab"), &tx.journal.id).err().unwrap().exit_code, 4);
    assert_eq!(tree(&lab.0), before);
    assert_eq!(std::fs::read(original).unwrap(), b"private original");
}

#[cfg(windows)]
#[test]
fn windows_private_journal_hardlinks_and_public_dacl_are_untrusted_without_rewriting_them() {
    let lab = Lab::new();
    let tx = Transaction::create(lab.request("state", vec![lab.edit("config/value", b"new")])).unwrap();
    let journal = tx.dir.join("journal.json");
    // 실제 NTFS hardlink의 handle link 수를 읽는 Windows 분기를 CI에서 실행한다.
    std::fs::hard_link(&journal, lab.0.join("journal-copy")).unwrap();
    let before = tree(&lab.0);
    assert_eq!(crate::install::engine::status(&tx.state, Some("lab"), &tx.journal.id).err().unwrap().exit_code, 4);
    assert_eq!(tree(&lab.0), before);
    std::fs::remove_file(lab.0.join("journal-copy")).unwrap();
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, SE_FILE_OBJECT, SetNamedSecurityInfoW,
            },
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    let sddl: Vec<u16> = "D:P(A;;FA;;;WD)".encode_utf16().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut present = 0;
    let mut defaulted = 0;
    // 시험이 소유한 임시 일반 파일 하나의 DACL만 넓힌다. 계정/정책/디렉터리는 바꾸지 않는다.
    unsafe {
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                std::ptr::null_mut()
            ),
            0
        );
        assert_ne!(GetSecurityDescriptorDacl(descriptor, &mut present, &mut dacl, &mut defaulted), 0);
        assert_ne!(present, 0);
        let mut name: Vec<u16> = journal.as_os_str().encode_wide().chain(Some(0)).collect();
        let code = SetNamedSecurityInfoW(
            name.as_mut_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            dacl,
            std::ptr::null_mut(),
        );
        LocalFree(descriptor);
        assert_eq!(code, 0);
    }
    let before = tree(&lab.0);
    assert_eq!(crate::install::engine::status(&tx.state, Some("lab"), &tx.journal.id).err().unwrap().exit_code, 4);
    assert_eq!(tree(&lab.0), before, "열린 DACL을 status가 고쳐 신뢰하지 않는다");
}
