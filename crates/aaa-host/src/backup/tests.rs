use super::*;

struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("aaa-backup-{}", util::new_id()));
        private_file::directory(&path).unwrap();
        Self(path)
    }
    fn state(&self) -> PathBuf {
        self.0.join("host")
    }
    fn host(&self) -> Arc<Host> {
        Host::open(self.host_config(), self.state()).unwrap()
    }
    fn host_config(&self) -> aaa_config::HostConfig {
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = self.0.join("projects").to_string_lossy().into();
        cfg
    }
    fn config(&self, public: String, keep: u32) -> BackupSection {
        BackupSection {
            enabled: true,
            directory: self.0.to_string_lossy().into(),
            recipient: public,
            keep,
            ..Default::default()
        }
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn wal_backup_restores_auth_history_and_rotates_only_its_files() {
    let lab = Lab::new();
    let host = lab.host();
    let identity = lab.0.join("recovery-key");
    let cfg = lab.config(keygen(&identity).unwrap(), 2);
    let writer = Connection::open(lab.state().join("host.db")).unwrap();
    writer.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    let secret = aaa_test_support::token("db-backup-device");
    writer
        .execute("INSERT INTO devices(id,role,token_hash,created_at) VALUES('fixture','admin',?1,1)", [&secret])
        .unwrap();
    writer
        .execute(
            "INSERT INTO task_queue_log(project,actor,at_ms,before,after) VALUES('fixture','host',1,'[]','[1]')",
            [],
        )
        .unwrap();
    assert!(lab.state().join("host.db-wal").metadata().unwrap().len() > 0);
    let first = run(&lab.state(), &cfg, DAY_MS).unwrap();
    let directory = first.parent().unwrap();
    let unrelated = directory.join("manual.db.gz.age");
    fs::write(&unrelated, b"keep").unwrap();
    let partial = directory.join(".interrupted.partial");
    fs::write(&partial, b"partial").unwrap();
    let second = run(&lab.state(), &cfg, DAY_MS * 2).unwrap();
    let third = run(&lab.state(), &cfg, DAY_MS * 3).unwrap();
    assert!(!first.exists());
    assert!(second.exists() && third.exists() && unrelated.exists() && partial.exists());
    let fourth = run(&lab.state(), &cfg, 1).unwrap();
    assert!(fourth.exists() && third.exists() && !second.exists());
    let encrypted = fs::read(&third).unwrap();
    assert!(!encrypted.windows(secret.len()).any(|w| w == secret.as_bytes()));
    let restored = lab.0.join("restored.db");
    restore(&third, &identity, &restored).unwrap();
    let db = crate::db::Db::open(&restored).unwrap();
    let conn = readonly(&restored).unwrap();
    assert_eq!(
        conn.query_row("SELECT token_hash FROM devices WHERE id='fixture'", [], |r| r.get::<_, String>(0)).unwrap(),
        secret
    );
    assert_eq!(conn.query_row("SELECT after FROM task_queue_log", [], |r| r.get::<_, String>(0)).unwrap(), "[1]");
    assert_eq!(db.meta("host_id").unwrap().as_deref(), Some(host.host_id.as_str()));
    assert!(!lab.state().join("db-backup-staging/snapshot.db").exists());
}

#[test]
fn pinned_snapshot_allows_writer_and_keeps_one_committed_version() {
    let lab = Lab::new();
    let host = lab.host();
    host.db.set_meta("fixture", "before").unwrap();
    let out = lab.0.join("snapshot.db");
    drop(private_file::new_file(&out).unwrap());
    snapshot_with(&lab.state().join("host.db"), &out, || {
        host.db.set_meta("fixture", "after").unwrap();
    })
    .unwrap();
    let value: String =
        readonly(&out).unwrap().query_row("SELECT value FROM meta WHERE key='fixture'", [], |r| r.get(0)).unwrap();
    assert_eq!(value, "before");
    assert_eq!(host.db.meta("fixture").unwrap().as_deref(), Some("after"));
}

#[test]
fn failed_backup_and_wrong_key_or_corruption_preserve_existing_data() {
    let lab = Lab::new();
    let host = lab.host();
    let key = lab.0.join("key");
    let mut cfg = lab.config(keygen(&key).unwrap(), 1);
    let good = run(&lab.state(), &cfg, DAY_MS).unwrap();
    host.db.set_meta("schema", "999").unwrap();
    assert!(run(&lab.state(), &cfg, DAY_MS * 2).is_err());
    assert!(good.exists());
    host.db.set_meta("schema", &crate::db::SCHEMA_VERSION.to_string()).unwrap();
    cfg.recipient = "invalid".into();
    assert!(run(&lab.state(), &cfg, DAY_MS * 2).is_err());
    assert!(good.exists());
    cfg.recipient = keygen(&lab.0.join("wrong-key")).unwrap();
    cfg.directory = lab.0.join("unmounted/subdir").to_string_lossy().into();
    assert!(run(&lab.state(), &cfg, DAY_MS * 2).is_err());
    assert!(!lab.0.join("unmounted").exists());
    let out = lab.0.join("out.db");
    assert!(restore(&good, &lab.0.join("wrong-key"), &out).is_err());
    assert!(!out.exists());
    let mut corrupt = fs::read(&good).unwrap();
    corrupt.pop();
    let bad = lab.0.join("bad.age");
    fs::write(&bad, corrupt).unwrap();
    assert!(restore(&bad, &key, &out).is_err());
    assert!(!out.exists());
    fs::write(&out, b"existing").unwrap();
    assert!(restore(&good, &key, &out).is_err());
    assert_eq!(fs::read(&out).unwrap(), b"existing");
    assert!(keygen(&key).is_err());
    assert_eq!(host.db.meta("host_id").unwrap().unwrap(), host.host_id);
}

#[test]
fn daily_due_catches_up_once_and_handles_clock_reversal() {
    let cfg = BackupSection { daily_utc: "04:30".into(), ..Default::default() };
    let slot = 4 * 3_600_000 + 30 * 60_000;
    assert!(!due(&cfg, 0, slot - 1));
    assert!(due(&cfg, 0, slot));
    assert!(!due(&cfg, slot, slot + 1));
    assert!(!due(&cfg, slot + 1, slot));
    assert!(due(&cfg, slot, 3 * DAY_MS));
    assert!(!due(&cfg, 3 * DAY_MS, 3 * DAY_MS));
    assert!(due(&cfg, 3 * DAY_MS, 3 * DAY_MS + slot));
}

#[tokio::test]
async fn scheduled_failure_is_durable_and_notifies_inbox_once() {
    let lab = Lab::new();
    let mut cfg = lab.host_config();
    cfg.backup = lab.config(age::x25519::Identity::generate().to_public().to_string(), 2);
    cfg.backup.daily_utc = "00:00".into();
    cfg.backup.directory = lab.0.join("missing").to_string_lossy().into();
    let host = Host::open(cfg, lab.state()).unwrap();
    let project = host.projects_root().join("fixture");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("PROJECT.md"), "fixture").unwrap();
    let mut changed = host.changed.subscribe();
    host.backup_pass_at(DAY_MS);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if !host.backup_running.load(Ordering::Acquire) {
                break;
            }
            changed.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(host.db.inbox_count("fixture").unwrap(), 1);
    assert_eq!(host.db.meta(LAST_ATTEMPT).unwrap().as_deref(), Some(DAY_MS.to_string().as_str()));
    host.backup_pass_at(DAY_MS + 1);
    assert!(!host.backup_running.load(Ordering::Acquire));
    assert_eq!(host.db.inbox_count("fixture").unwrap(), 1);
}

#[tokio::test]
async fn scheduled_success_survives_restart_without_duplicate_backup() {
    let lab = Lab::new();
    let mut cfg = lab.host_config();
    cfg.backup = lab.config(age::x25519::Identity::generate().to_public().to_string(), 2);
    cfg.backup.daily_utc = "00:00".into();
    let host = Host::open(cfg.clone(), lab.state()).unwrap();
    let mut changed = host.changed.subscribe();
    host.backup_pass_at(DAY_MS);
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.backup_running.load(Ordering::Acquire) {
            changed.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let recorded = status(&lab.state()).unwrap();
    assert_eq!(recorded.last_attempt_ms, Some(DAY_MS));
    assert_eq!(recorded.last_success_ms, Some(DAY_MS));
    assert!(recorded.last_error.is_none());
    drop(changed);
    drop(host);
    let host = Host::open(cfg, lab.state()).unwrap();
    host.backup_pass_at(DAY_MS + 1);
    assert!(!host.backup_running.load(Ordering::Acquire));
    assert_eq!(status(&lab.state()).unwrap().last_attempt_ms, Some(DAY_MS));
}
