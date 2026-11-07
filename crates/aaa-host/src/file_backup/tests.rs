use super::*;
use crate::Host;
use aaa_proto::{IdeaAuthor, IdeaTaskDraft};
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;

struct Lab {
    root: PathBuf,
    state: PathBuf,
    cfg: HostConfig,
    key: PathBuf,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-files-{}", aaa_common::util::new_id()));
        private_file::directory(&root).unwrap();
        let state = root.join("host");
        let key = root.join("identity");
        let mut cfg = HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.backup.directory = root.to_string_lossy().into();
        cfg.backup.recipient = backup::keygen(&key).unwrap();
        fs::create_dir_all(root.join("projects/demo")).unwrap();
        fs::write(root.join("projects/demo/PROJECT.md"), "# demo\n").unwrap();
        fs::write(root.join("projects/demo/TASKS.md"), "## Backlog\n").unwrap();
        Self { root, state, cfg, key }
    }
    fn host(&self) -> Arc<Host> {
        Host::open(self.cfg.clone(), self.state.clone()).unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[tokio::test]
async fn restores_revisions_metadata_media_handoff_uploads_and_short_ids_in_a_new_host() {
    let lab = Lab::new();
    let host = lab.host();
    let note = host.idea_create("demo", "복원 시험", "# 복원 시험\n첫 판\n", None, "test").unwrap();
    let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
    let image = host.idea_file_save("demo", "n-0001", png, Some("fixture.png".into())).unwrap();
    let folder = host.idea_folder("demo", "n-0001").unwrap();
    let text = format!("# 복원 시험\n![그림]({image})\n");
    assert!(host.idea_write("demo", &folder, 1, &text, None, IdeaAuthor::User, "test").unwrap().is_ok());
    host.idea_handoff(
        "demo",
        "n-0001",
        2,
        vec![IdeaTaskDraft { title: "구현".into(), repo: None, notes: String::new() }],
        "test",
    )
    .await
    .unwrap();
    let copy = host.idea_copy("demo", "n-0001", "test").unwrap();
    let archive = run(&lab.state, &lab.cfg, 1).unwrap();
    let cipher = fs::read(&archive).unwrap();
    assert!(!cipher.windows(text.len()).any(|w| w == text.as_bytes()));
    let out = lab.root.join("restored");
    restore(&archive, &lab.key, &out).unwrap();
    let mut cfg = HostConfig::default();
    cfg.projects.root = out.join("projects").to_string_lossy().into();
    let restored = Host::open(cfg, out.join("host")).unwrap();
    let f = restored.idea_folder("demo", "n-0001").unwrap();
    assert_eq!(f.id, note.idea.id);
    assert_eq!(fs::read(f.dir.join("note.md")).unwrap(), text.as_bytes());
    assert_eq!(fs::read(f.dir.join("revs/0001.md")).unwrap(), note.text.as_bytes());
    assert_eq!(fs::read(f.dir.join("revs/0002.md")).unwrap(), text.as_bytes());
    assert_eq!(fs::read(f.dir.join("idea.json")).unwrap(), fs::read(folder.dir.join("idea.json")).unwrap());
    assert_eq!(restored.idea_file_read("demo", &f.id, &image).unwrap(), png);
    assert_eq!(restored.task_file_read("demo", &image).unwrap(), png);
    assert_eq!(restored.idea_folder("demo", "n-0002").unwrap().id, copy.idea.id);
    assert_eq!(restored.idea_create("demo", "다음", "", None, "test").unwrap().idea.short_id, "n-0003");
    assert!(restored.db.meta("db-backup:last-success-ms").unwrap().is_none());
    assert!(!lab.state.join("file-backup-staging/snapshot.tar").exists());
    assert!(!out.join(".snapshot.tar").exists());
    assert!(restore(&archive, &lab.key, &out).is_err());
    assert_eq!(restored.task_file_read("demo", &image).unwrap(), png);
}

#[test]
fn keeps_thirty_completed_file_archives_and_never_rotates_db_other_hosts_or_partials() {
    let lab = Lab::new();
    let _host = lab.host();
    let archive = run(&lab.state, &lab.cfg, 100).unwrap();
    let directory = archive.parent().unwrap();
    // 실제 암호화 31회를 반복하지 않고 회전 입력을 만든다. 마지막 새 사본은 실제 run으로 공개한다.
    for n in 1..30 {
        fs::write(directory.join(format!("files-{n:020}-{}{SUFFIX}", aaa_common::util::new_id())), b"old ciphertext")
            .unwrap();
    }
    let db = directory.join(format!("host-{:020}-{}.db.gz.age", 1, aaa_common::util::new_id()));
    fs::write(&db, b"db").unwrap();
    let partial = directory.join(".unfinished.partial");
    fs::write(&partial, b"partial").unwrap();
    let other = lab.root.join("aaa-host-other");
    fs::create_dir(&other).unwrap();
    fs::write(other.join("untouched"), b"other").unwrap();
    let current = run(&lab.state, &lab.cfg, 0).unwrap();
    let count = fs::read_dir(directory)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("files-"))
        .count();
    assert_eq!(count, 30);
    assert!(
        current.exists() && archive.exists() && db.exists() && partial.exists() && other.join("untouched").exists()
    );
}

#[test]
fn rejects_incomplete_snapshots_wrong_keys_and_corrupt_ciphertext_without_losing_a_good_copy() {
    let lab = Lab::new();
    let host = lab.host();
    host.idea_create("demo", "노트", "# 노트\n", None, "test").unwrap();
    let good = run(&lab.state, &lab.cfg, 1).unwrap();
    let f = host.idea_folder("demo", "n-0001").unwrap();
    fs::write(f.dir.join("note.md"), "저장 중간").unwrap();
    assert!(run(&lab.state, &lab.cfg, 2).is_err());
    assert!(good.exists());
    assert!(!lab.state.join("file-backup-staging/snapshot.tar").exists());
    let wrong = lab.root.join("wrong");
    backup::keygen(&wrong).unwrap();
    let out = lab.root.join("out");
    assert!(restore(&good, &wrong, &out).is_err());
    assert!(!out.exists());
    let mut bytes = fs::read(&good).unwrap();
    bytes.pop();
    let broken = lab.root.join("broken.age");
    fs::write(&broken, bytes).unwrap();
    assert!(restore(&broken, &lab.key, &out).is_err());
    assert!(!out.exists());
    let mut missing = lab.cfg.clone();
    missing.backup.directory = lab.root.join("unmounted/backup").to_string_lossy().into();
    assert!(run(&lab.state, &missing, 3).is_err());
    assert!(!lab.root.join("unmounted").exists());
}

#[test]
fn snapshot_cannot_enter_a_multi_file_write_and_releases_its_lock_before_encryption() {
    let lab = Lab::new();
    let host = lab.host();
    let plain = lab.root.join("snapshot.tar");
    let writer = host.idea_writing().unwrap();
    // 별도 CLI와 같은 독립 FD로 배타를 확인한다. 시각이나 sleep에 기대지 않는다.
    assert!(files_lock(&lab.state, false).is_err());
    drop(writer);
    snapshot(&lab.state, Path::new(&lab.cfg.projects.root), &plain).unwrap();
    let _writer = host.idea_writing().unwrap();
    assert!(files_lock(&lab.state, false).is_err());
}

async fn wait_backup(host: &Arc<Host>, now: u64) {
    let mut changes = host.changed.subscribe();
    host.backup_pass_at(now);
    tokio::time::timeout(Duration::from_secs(5), async {
        while host.file_backup_running.load(Ordering::Acquire) {
            changes.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn daily_file_backup_is_independent_of_disabled_db_backup_and_remembers_failures_and_success() {
    let mut lab = Lab::new();
    lab.cfg.file_backup.enabled = true;
    lab.cfg.file_backup.daily_utc = "00:00".into();
    let host = lab.host();
    wait_backup(&host, 86_400_000).await;
    assert_eq!(status(&lab.state).unwrap().last_success_ms, Some(86_400_000));
    assert!(backup::status(&lab.state).unwrap().last_attempt_ms.is_none());
    drop(host);
    lab.cfg.backup.directory = lab.root.join("missing").to_string_lossy().into();
    let host = lab.host();
    wait_backup(&host, 86_400_001).await;
    assert!(status(&lab.state).unwrap().last_error.is_none(), "같은 차례는 재시작 후에도 다시 시도하지 않는다");
    wait_backup(&host, 172_800_000).await;
    assert_eq!(host.db.inbox_count("demo").unwrap(), 1);
    assert!(status(&lab.state).unwrap().last_error.unwrap().contains("목적지"));
    wait_backup(&host, 259_200_000).await;
    assert_eq!(host.db.inbox_count("demo").unwrap(), 1, "열린 실패 알림은 중복하지 않는다");
    assert_eq!(status(&lab.state).unwrap().last_success_ms, Some(86_400_000));
}

#[test]
fn encrypted_archive_rejects_traversal_links_duplicates_and_unknown_schema() {
    let lab = Lab::new();
    let _host = lab.host();
    for (n, kind) in ["schema", "link", "duplicate", "traversal"].iter().enumerate() {
        let plain = lab.root.join(format!("invalid-{n}.tar"));
        let mut tar = tar::Builder::new(private_file::new_file(&plain).unwrap());
        let schema = if *kind == "schema" { 2 } else { 1 };
        let bytes = serde_json::to_vec(&Index { schema, ideas: Vec::new() }).unwrap();
        append(&mut tar, Path::new(INDEX), bytes.len() as u64, bytes.as_slice()).unwrap();
        if *kind == "duplicate" {
            append(&mut tar, Path::new(INDEX), bytes.len() as u64, bytes.as_slice()).unwrap();
        }
        if matches!(*kind, "link" | "traversal") {
            let mut header = tar::Header::new_ustar();
            header.set_size(0);
            header.set_mode(0o600);
            if *kind == "link" {
                header.set_path("projects/demo/uploads/link").unwrap();
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_link_name("../../../escape").unwrap();
            } else {
                // Builder의 경로 검사를 우회한 악성 tar 입력을 만든다.
                header.as_mut_bytes()[..9].copy_from_slice(b"../escape");
            }
            header.set_cksum();
            tar.append(&header, std::io::empty()).unwrap();
        }
        drop(tar.into_inner().unwrap());
        let archive = backup::publish(&lab.state, &lab.cfg.backup, n as u64, &plain, "files", SUFFIX).unwrap();
        let out = lab.root.join(format!("out-{n}"));
        let error = restore(&archive, &lab.key, &out).unwrap_err();
        if *kind == "schema" {
            assert!(error.to_string().contains("schema"), "{error:#}");
        }
        assert!(!out.exists());
        assert!(!lab.root.join("escape").exists());
    }
    for path in [
        "../escape",
        "/absolute",
        "projects/demo/ideas/../../escape",
        "projects/demo/uploads/C:escape",
        "projects/demo/uploads/a\\escape",
        "projects/demo/uploads/CON",
        "host/host.db",
    ] {
        assert!(!safe_entry(path), "{path}");
    }
}
