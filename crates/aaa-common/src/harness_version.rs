//! Harness 판은 진단 자료다. 값·종료 코드·조회 실패가 실행 허용 여부를 정하지 않는다.
use crate::child_process::TokioCommandExt as _;
use std::{process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

/// 호출자가 준비한 환경·격리 홈을 그대로 사용한다. 인증 값·전체 argv는 기록하지 않는다.
pub async fn observe(mut command: Command, harness: &str) -> String {
    let executable = command.as_std().get_program().to_string_lossy().into_owned();
    let result = probe(&mut command).await;
    let record = crate::redact::text(&format!("harness={harness} 실행 파일={executable} {result}"));
    tracing::info!(observation = %record, "harness 판 조회");
    record
}

async fn probe(command: &mut Command) -> String {
    command.arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = match command.spawn_guarded() {
        Ok(child) => child,
        Err(error) => return format!("조회 실행 실패: {error}"),
    };
    let pid = child.id();
    let mut stdout = child.stdout.take().expect("판 조회 stdout").take(4097);
    let mut stderr = child.stderr.take().expect("판 조회 stderr").take(4097);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let (status, _, _) =
            tokio::try_join!(child.wait(), stdout.read_to_end(&mut out), stderr.read_to_end(&mut err))?;
        Ok::<_, std::io::Error>(status)
    })
    .await;
    let outcome = match result {
        Ok(Ok(status)) => format!("종료={status}"),
        Ok(Err(error)) => format!("조회 읽기 실패: {error}"),
        Err(_) => {
            let _ = child.kill().await;
            format!("조회 5초 초과 종료={:?}", child.wait().await)
        }
    };
    format!(
        "pid={pid:?} {outcome} stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out),
        String::from_utf8_lossy(&err)
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn records_any_version_and_failed_probe_without_rejecting_or_leaking_auth() {
        let root = std::env::temp_dir().join(format!("aaa-harness-version-{}", crate::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        let exe = root.join("harness");
        std::fs::write(&exe, "#!/bin/sh\nprintf '%s' \"$FIXTURE_VERSION\"\nprintf '%s' \"$OPENAI_API_KEY\" >&2\nexit \"$FIXTURE_EXIT\"\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let secret = "qzF09R4sK2nB7vL6fixtureAuth";
        crate::redact::remember_env("OPENAI_API_KEY", secret);
        for harness in ["codex", "claude"] {
            for (version, exit) in [("arbitrary development build\r\n", "0"), ("future-version\n", "7")] {
                let mut command = Command::new(&exe);
                command.env("FIXTURE_VERSION", version).env("FIXTURE_EXIT", exit).env("OPENAI_API_KEY", secret);
                let record = observe(command, harness).await;
                assert!(record.contains(harness) && record.contains("pid=") && record.contains("종료="));
                assert!(record.contains(version.trim()), "{record}");
                assert!(!record.contains(secret));
            }
        }
        let record = observe(Command::new(root.join("missing")), "codex").await;
        assert!(record.contains("조회 실행 실패") && record.contains("missing"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
