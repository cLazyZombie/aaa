//! 작업 요약 검사는 host와 프로젝트 없이 실행된다.

use std::{fs, path::PathBuf, process::Command};

struct Lab(PathBuf);

impl Drop for Lab {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

impl Lab {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("aaa-summary-{}", aaa_common::util::new_id()));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_aaa"));
        cmd.current_dir(&self.0)
            .env_remove("AAA_SESSION_ID")
            .env_remove("AAA_INSTANCE")
            .env("AAA_CONFIG_DIR", self.0.join("config"))
            .env("AAA_STATE_DIR", self.0.join("state"))
            .args(["thread", "summary"]);
        cmd
    }
}

#[test]
fn checks_summary_files_and_reports_original_line_numbers() {
    let lab = Lab::new();
    let cases = [
        ("# 목표\n작업 요약 검사\n## 진행\n- [~] 검사 중\n", 0, "작업 요약 형식이 맞다"),
        ("# 목표\n작업 요약 검사\n## 정함\n- 하나\n- 둘\n- 셋\n- 넷\n", 1, "7번 줄: 한 절은 3줄까지다"),
        ("\r\n\r\n## 목표\r\n작업 요약 검사\r\n", 1, "3번 줄: 모르는 머리 `## 목표`"),
    ];
    let file = lab.0.join("요약.md");
    for (text, code, expected) in cases {
        fs::write(&file, text).unwrap();
        let out = lab.command().args(["--check-file", "요약.md"]).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(code), "{stderr}");
        if code == 0 {
            assert!(String::from_utf8_lossy(&out.stdout).contains(expected));
            assert!(stderr.is_empty(), "{stderr}");
        } else {
            assert!(stderr.contains(expected), "{stderr}");
            assert!(out.stdout.is_empty());
        }
        assert_eq!(fs::read(&file).unwrap(), text.as_bytes());
    }
    assert!(!lab.0.join("config").exists());
    assert!(!lab.0.join("state").exists());
}

#[test]
fn check_file_cannot_be_combined_with_saving_or_a_thread_target() {
    let lab = Lab::new();
    for args in [
        &["--check-file", "요약.md", "--file", "요약.md"][..],
        &["t-0001", "--check-file", "요약.md"],
        &["--file", "요약.md"],
        &[],
    ] {
        let out = lab.command().args(args).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {stderr}");
    }
}
