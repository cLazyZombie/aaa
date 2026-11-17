use super::*;
use crate::Host;
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

/// 첨부 하나를 `uploads/`에 넣는다(내용 hash 이름)
fn seed_upload(lab: &Lab) -> (String, Vec<u8>) {
    let png = b"\x89PNG\r\n\x1a\nupload".to_vec();
    let name = "a6d715956dab435e.png".to_string();
    fs::create_dir_all(lab.root.join("projects/demo/uploads")).unwrap();
    fs::write(lab.root.join("projects/demo/uploads").join(&name), &png).unwrap();
    (name, png)
}

#[test]
fn restores_uploads_into_a_new_folder_without_idea_notes() {
    let lab = Lab::new();
    let _host = lab.host();
    let (name, png) = seed_upload(&lab);
    // 지운 옛 아이디어 폴더가 남아 있어도 묶지 않는다(t-1959)
    fs::create_dir_all(lab.root.join("projects/demo/ideas/01JD0000000000000000000000")).unwrap();
    fs::write(lab.root.join("projects/demo/ideas/01JD0000000000000000000000/note.md"), "옛 노트 비밀 글").unwrap();
    let archive = run(&lab.state, &lab.cfg, 1).unwrap();
    let cipher = fs::read(&archive).unwrap();
    assert!(!cipher.windows(png.len()).any(|w| w == png.as_slice()));
    let out = lab.root.join("restored");
    restore(&archive, &lab.key, &out).unwrap();
    assert_eq!(fs::read(out.join("projects/demo/uploads").join(&name)).unwrap(), png);
    assert_eq!(fs::read_to_string(out.join("projects/demo/PROJECT.md")).unwrap(), "# demo\n");
    assert!(!out.join("projects/demo/ideas").exists());
    assert!(!out.join("host").exists(), "번호 DB를 만들지 않는다");
    assert!(!lab.state.join("file-backup-staging/snapshot.tar").exists());
    assert!(!out.join(".snapshot.tar").exists());
    assert!(restore(&archive, &lab.key, &out).is_err());
}

/// 옛 판이 만든 백업(번호 원장·아이디어 노트 포함)은 첨부만 복원하고 아이디어 항목은 건너뛴다
#[test]
fn restores_old_archives_and_skips_their_idea_entries() {
    let lab = Lab::new();
    let _host = lab.host();
    let plain = lab.root.join("old.tar");
    let mut tar = tar::Builder::new(private_file::new_file(&plain).unwrap());
    let index = br#"{"schema":1,"ideas":[["demo","01JD0000000000000000000000",1]]}"#;
    append(&mut tar, Path::new(OLD_INDEX), index.len() as u64, index.as_slice()).unwrap();
    let note = b"# old\n";
    append(
        &mut tar,
        Path::new("projects/demo/ideas/01JD0000000000000000000000/note.md"),
        note.len() as u64,
        note.as_slice(),
    )
    .unwrap();
    let up = b"upload bytes";
    append(&mut tar, Path::new("projects/demo/uploads/0123456789abcdef.png"), up.len() as u64, up.as_slice()).unwrap();
    drop(tar.into_inner().unwrap());
    let archive = backup::publish(&lab.state, &lab.cfg.backup, 7, &plain, "files", SUFFIX).unwrap();
    let out = lab.root.join("out-old");
    restore(&archive, &lab.key, &out).unwrap();
    assert_eq!(fs::read(out.join("projects/demo/uploads/0123456789abcdef.png")).unwrap(), up);
    assert!(!out.join(OLD_INDEX).exists());
    assert!(!out.join("projects/demo/ideas").exists());
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
fn rejects_symlinked_sources_wrong_keys_and_corrupt_ciphertext_without_losing_a_good_copy() {
    let lab = Lab::new();
    let _host = lab.host();
    seed_upload(&lab);
    let good = run(&lab.state, &lab.cfg, 1).unwrap();
    // 일반 파일이 아닌 원본(symlink)은 실패로 알리고 앞 사본은 남는다
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/hosts", lab.root.join("projects/demo/uploads/link.png")).unwrap();
        assert!(run(&lab.state, &lab.cfg, 2).is_err());
        fs::remove_file(lab.root.join("projects/demo/uploads/link.png")).unwrap();
    }
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
    let writer = host.files_writing().unwrap();
    // 별도 CLI와 같은 독립 FD로 배타를 확인한다. 시각이나 sleep에 기대지 않는다.
    assert!(files_lock(&lab.state, false).is_err());
    drop(writer);
    snapshot(&lab.state, Path::new(&lab.cfg.projects.root), &plain).unwrap();
    let _writer = host.files_writing().unwrap();
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
fn encrypted_archive_rejects_traversal_links_and_duplicates() {
    let lab = Lab::new();
    let _host = lab.host();
    for (n, kind) in ["link", "duplicate", "traversal"].iter().enumerate() {
        let plain = lab.root.join(format!("invalid-{n}.tar"));
        let mut tar = tar::Builder::new(private_file::new_file(&plain).unwrap());
        let bytes = b"x";
        let path = Path::new("projects/demo/uploads/0123456789abcdef.png");
        append(&mut tar, path, bytes.len() as u64, bytes.as_slice()).unwrap();
        if *kind == "duplicate" {
            append(&mut tar, path, bytes.len() as u64, bytes.as_slice()).unwrap();
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
        assert!(restore(&archive, &lab.key, &out).is_err(), "{kind}");
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
