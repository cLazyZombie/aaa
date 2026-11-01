//! 실제 CLI에서 host 없이 검증을 실행하고 원문 전달·실패 코드·종료 원장을 확인한다.
#![cfg(unix)]

use serde_json::Value;
use std::{fs, path::PathBuf, process::Command};

struct Repo(PathBuf);
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_detects_a_script_and_records_success_and_failure_without_host_or_repo_adapters() {
    let repo = Repo(std::env::temp_dir().join(format!("aaa-verify-{}", aaa_common::util::new_id())));
    fs::create_dir_all(repo.0.join("scripts")).unwrap();
    fs::write(repo.0.join(".gitignore"), "target/\n").unwrap();
    fs::write(
        repo.0.join("scripts/verify.sh"),
        r#"printf 'streamed stdout\n'
printf 'streamed stderr\n' >&2
if [ "${1:-}" = failure ]; then
  printf 'test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'
  exit 7
fi
printf 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n'
"#,
    )
    .unwrap();
    for args in [
        vec!["init", "-qb", "main"],
        vec!["remote", "add", "origin", "https://example.com/team/game.git"],
        vec!["add", "."],
        vec!["commit", "-qm", "verification fixture"],
    ] {
        let out = Command::new("git")
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(args)
            .current_dir(&repo.0)
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    for (args, expected) in [(vec!["verify"], 0), (vec!["verify", "--", "bash", "scripts/verify.sh", "failure"], 7)] {
        let out = Command::new(env!("CARGO_BIN_EXE_aaa"))
            .args(args)
            .current_dir(&repo.0)
            .env_remove("AAA_SESSION_ID")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(expected), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(out.stdout.starts_with(b"streamed stdout\n"));
        assert!(out.stderr.starts_with(b"streamed stderr\n"));
    }
    let out = Command::new(env!("CARGO_BIN_EXE_aaa"))
        .args(["--json", "verify", "--", "bash", "scripts/verify.sh", "failure"])
        .current_dir(&repo.0)
        .env_remove("AAA_SESSION_ID")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(7));
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result, serde_json::json!({"success":false,"code":7}));
    assert!(String::from_utf8_lossy(&out.stderr).contains("streamed stdout"));
    let runs: Vec<_> = fs::read_dir(repo.0.join(".git/aaa-workspace-runs"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let source: Value = serde_json::from_slice(&fs::read(path.join("provenance.json")).unwrap()).unwrap();
            assert_eq!(source["format"], 3);
            assert_eq!(source["runner"], "aaa-verify");
            serde_json::from_slice::<Value>(&fs::read(path.join("metrics.json")).unwrap()).unwrap()
        })
        .collect();
    assert_eq!(runs.len(), 3);
    assert!(runs.iter().any(|r| r["outcome"] == "succeeded" && r["tests"]["passed"] == 1));
    assert!(runs.iter().any(|r| r["outcome"] == "failed" && r["tests"]["failed"] == 1));
    assert!(runs.iter().all(|r| !r.to_string().contains("streamed")));
}
