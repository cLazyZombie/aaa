use super::*;

struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("aaa-logs-{}", aaa_common::util::new_id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn open(&self, policy: LogsSection) -> RotatingFile {
        let path = self.0.join("host.log");
        append().create(true).open(&path).unwrap();
        RotatingFile::open(&path, &fs::metadata(&path).unwrap(), policy).unwrap()
    }

    fn archives(&self) -> Vec<PathBuf> {
        let mut paths: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.file_name().unwrap().to_string_lossy().starts_with("host.log.rotated-"))
            .collect();
        paths.sort();
        paths
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exact_size_stays_active_and_rotation_keeps_the_newest_records() {
    let lab = Lab::new();
    let mut log = lab.open(LogsSection { keep_files: 1, ..LogsSection::default() });
    log.max_bytes = 8;
    log.record(b"start!!\n");
    assert!(lab.archives().is_empty());
    log.record(b"second!\n");
    log.record(b"third!!\n");
    assert_eq!(fs::read(&log.path).unwrap(), b"third!!\n");
    let archives = lab.archives();
    assert_eq!(archives.len(), 1);
    assert_eq!(fs::read(&archives[0]).unwrap(), b"second!\n");
    assert_eq!(fs::metadata(&log.path).unwrap().mode() & 0o077, 0);
}

#[test]
fn pruning_after_restart_respects_age_and_skips_other_files_and_links() {
    let lab = Lab::new();
    let policy = LogsSection { keep_files: 4, keep_days: 1, ..LogsSection::default() };
    let mut log = lab.open(policy);
    log.record(b"old\n");
    log.rotate(10_000).unwrap();
    let ignored = lab.0.join("agent.log.rotated-10000-01AAAAAAAAAAAAAAAAAAAAAAAA");
    fs::write(&ignored, b"other role").unwrap();
    let link = lab.0.join("host.log.rotated-1-01BBBBBBBBBBBBBBBBBBBBBBBB");
    std::os::unix::fs::symlink(&ignored, &link).unwrap();
    let oldest = lab.archives().into_iter().find(|path| *path != link).unwrap();
    log.record(b"recent\n");
    log.rotate(20_000).unwrap();
    drop(log);
    let mut reopened = lab.open(policy);
    reopened.prune(86_409_999).unwrap();
    assert!(oldest.exists());
    reopened.prune(86_410_000).unwrap();
    assert!(!oldest.exists());
    assert_eq!(lab.archives().len(), 2); // 아직 보관할 사본과 무시한 링크
    assert!(link.is_symlink());
    assert_eq!(fs::read(ignored).unwrap(), b"other role");
}

#[test]
fn failed_rotation_keeps_receiving_and_does_not_replace_a_foreign_file() {
    let lab = Lab::new();
    let mut log = lab.open(LogsSection::default());
    log.max_bytes = 5;
    log.record(b"old!\n");
    let renamed = log.path.with_extension("moved");
    fs::rename(&log.path, &renamed).unwrap();
    fs::write(&log.path, b"foreign\n").unwrap();
    log.record(b"next\n");
    log.record(b"more\n");
    assert_eq!(fs::read(&log.path).unwrap(), b"foreign\n");
    let output = fs::read_to_string(renamed).unwrap();
    assert_eq!(output.matches("회전 실패").count(), 1);
    assert!(output.ends_with("next\nmore\n"));
    assert!(lab.archives().is_empty());
}

#[test]
fn same_role_rejects_a_second_writer_but_another_role_can_write() {
    let lab = Lab::new();
    let log = lab.open(LogsSection::default());
    let other = RotatingFile::open(&log.path, &fs::metadata(&log.path).unwrap(), LogsSection::default());
    assert!(matches!(other, Err(error) if error.kind() == io::ErrorKind::WouldBlock));
    let agent = log.path.with_file_name("agent.log");
    fs::write(&agent, b"").unwrap();
    assert!(RotatingFile::open(&agent, &fs::metadata(&agent).unwrap(), LogsSection::default()).is_ok());
}

#[test]
fn collector_caps_a_split_line_and_drains_the_last_fragment_on_shutdown() {
    let lab = Lab::new();
    let log = lab.open(LogsSection::default());
    let (input, mut output) = UnixStream::pair().unwrap();
    let inherited = output.try_clone().unwrap();
    let drain = std::thread::spawn(move || log.drain(input));
    output.write_all(&vec![b'x'; 300_000]).unwrap();
    output.write_all(b"\nsecond\nfragment").unwrap();
    output.shutdown(Shutdown::Write).unwrap();
    drop(drain.join().unwrap());
    drop(inherited);
    let bytes = fs::read(lab.0.join("host.log")).unwrap();
    assert_eq!(bytes.iter().position(|byte| *byte == b'\n'), Some(262_143));
    assert!(bytes[262_144 - TRUNCATED.len()..].starts_with(TRUNCATED));
    assert_eq!(&bytes[262_144..], b"second\nfragment");
}

#[test]
fn stdout_stderr_and_exec_continue_in_the_active_file() {
    use aaa_common::child_process::StdCommandExt;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    const CASE: &str = "service_logs::tests::stdout_stderr_and_exec_continue_in_the_active_file";
    const PATH: &str = "AAA_TEST_ROLE_LOG_EXEC_PATH";
    const STAGE: &str = "AAA_TEST_ROLE_LOG_EXEC_STAGE";
    let command = || {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([CASE, "--exact", "--nocapture"]);
        command
    };

    if let Some(path) = std::env::var_os(PATH) {
        let path = PathBuf::from(path);
        let expected = fs::metadata(&path).unwrap();
        let stage = std::env::var(STAGE).unwrap();
        let original = STD_FDS.map(|fd| duplicate(fd).unwrap());
        assert!(original.iter().all(|file| same_file(&file.metadata().unwrap(), &expected)));
        let guard = Capture::open(&path, expected, original, LogsSection { max_file_mib: 1, ..LogsSection::default() })
            .unwrap();
        println!("stdout during {stage}");
        eprintln!("stderr during {stage}");
        drop(guard);
        eprintln!("stderr after {stage}");
        if stage == "first" {
            panic!("시험 exec에 실패했다: {}", command().env(STAGE, "second").exec());
        }
        return;
    }

    let lab = Lab::new();
    let path = lab.0.join("host.log");
    let legacy = append().create_new(true).open(&path).unwrap();
    legacy.set_len(1_048_577).unwrap();
    let status = command()
        .env(PATH, &path)
        .env(STAGE, "first")
        .stdout(legacy.try_clone().unwrap())
        .stderr(legacy)
        .status_guarded()
        .unwrap();
    assert!(status.success(), "{status}");
    assert_eq!(lab.archives().len(), 1);
    let text = fs::read_to_string(&path).unwrap();
    for line in [
        "stdout during first",
        "stderr during first",
        "stderr after first",
        "stdout during second",
        "stderr during second",
        "stderr after second",
    ] {
        assert!(text.contains(line), "누락한 로그 {line}: {text}");
    }
}
