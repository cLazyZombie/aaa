//! routine 명령 실행(§8.9). host는 명령을 실행하지 않고, 대상 머신의 agent가 여기서 실행한다.
//!
//! - `sh -c`(Windows는 `cmd /C`)로 실행한다. stdin은 비우고 stdout·stderr를 받는다.
//! - unix에서는 자식을 새 process group으로 띄운다. 시간이 넘으면 **그 group만**(자기가 만든 pgid) 끝낸다.
//! - 출력은 상한까지만 모으고 나머지는 읽어서 버린다(파이프가 막혀 자식이 멈추지 않게).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use aaa_proto::{CommandRan, valid_project};
use anyhow::{Context, Result, bail};
use tokio::io::AsyncReadExt;

/// 모으는 출력 상한(바이트). host가 사람에게 보일 때 더 줄인다.
const CAPTURE_BYTES: usize = 64 * 1024;
/// 시간 초과로 group을 끝낸 뒤 남은 출력을 기다리는 시간(group 밖으로 나간 손자가 파이프를 쥐고 있을 수 있다)
const DRAIN_AFTER_KILL: Duration = Duration::from_secs(2);

pub async fn run(
    work_root: &Path,
    work_mount: Option<&Path>,
    project: &str,
    cwd: Option<&str>,
    command: &str,
    timeout: Duration,
) -> Result<CommandRan> {
    if !valid_project(project) {
        bail!("프로젝트 이름 형식이 아니다: {project}");
    }
    if command.trim().is_empty() {
        bail!("명령이 비었다");
    }
    let dir = match cwd {
        Some(c) => {
            let p = PathBuf::from(c);
            crate::work_root::check_managed_path(work_root, work_mount, &p)?;
            if !p.is_absolute() || !p.is_dir() {
                bail!("실행 폴더가 없다: {c}");
            }
            p
        }
        None => {
            crate::work_root::check(work_root, work_mount)?;
            let p = work_root.join(project).join("routines");
            crate::work_root::check_managed_path(work_root, work_mount, &p)?;
            std::fs::create_dir_all(&p).with_context(|| format!("{}", p.display()))?;
            p
        }
    };
    #[cfg(unix)]
    let mut cmd = {
        let mut c = tokio::process::Command::new("/bin/sh");
        c.arg("-c").arg(command).process_group(0);
        c
    };
    #[cfg(windows)]
    let mut cmd = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };
    cmd.current_dir(&dir).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().context("명령 실행")?;
    let out = tokio::spawn(capture(child.stdout.take()));
    let err = tokio::spawn(capture(child.stderr.take()));
    let (status, timed_out) = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(s) => (Some(s?), false),
        Err(source_error) => {
            aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            kill_group(&mut child);
            (
                child
                    .wait()
                    .await
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok(),
                true,
            )
        }
    };
    let collect = |h: tokio::task::JoinHandle<(Vec<u8>, bool)>| async move {
        match tokio::time::timeout(DRAIN_AFTER_KILL.max(Duration::from_millis(1)), h).await {
            Ok(Ok(v)) => v,
            _ => (Vec::new(), true),
        }
    };
    let (mut bytes, cut_out) = collect(out).await;
    let (err_bytes, cut_err) = collect(err).await;
    bytes.extend_from_slice(&err_bytes);
    let exit_code = if timed_out { None } else { status.and_then(|s| s.code()) };
    Ok(CommandRan {
        exit_code,
        timed_out,
        output: String::from_utf8_lossy(&bytes).into_owned(),
        truncated: cut_out || cut_err,
    })
}

async fn capture(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> (Vec<u8>, bool) {
    let Some(mut pipe) = pipe else { return (Vec::new(), false) };
    let mut buf = Vec::new();
    let mut cut = false;
    let mut chunk = [0u8; 8192];
    loop {
        match pipe.read(&mut chunk).await {
            Ok(0) => break,
            Err(error) => {
                tracing::warn!(error = %aaa_common::diagnostic::Cause::capture(&error), "명령 출력 읽기 실패");
                cut = true;
                break;
            }
            Ok(n) => {
                let room = CAPTURE_BYTES.saturating_sub(buf.len());
                buf.extend_from_slice(&chunk[..n.min(room)]);
                cut |= n > room;
            }
        }
    }
    (buf, cut)
}

/// 자기가 만든 process group만 끝낸다(pgid = 자식 PID, `process_group(0)`)
fn kill_group(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
        }
        return;
    }
    aaa_common::diagnostic::ignored(child.start_kill(), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runs_and_times_out_whole_group() {
        let root = std::env::temp_dir().join(format!("aaa-cmd-{}", std::process::id()));
        let r =
            run(&root, None, "demo", None, "echo out; echo err >&2; exit 3", Duration::from_secs(10)).await.unwrap();
        assert_eq!((r.exit_code, r.timed_out, r.output.as_str()), (Some(3), false, "out\nerr\n"));
        assert!(root.join("demo/routines").is_dir());
        // 손자(sleep)까지 group으로 끝나야 파이프가 닫혀 바로 돌아온다
        let t0 = std::time::Instant::now();
        let r = run(&root, None, "demo", None, "echo start; sleep 30 & sleep 30", Duration::from_millis(300))
            .await
            .unwrap();
        assert!(r.timed_out && r.exit_code.is_none(), "{r:?}");
        assert_eq!(r.output, "start\n");
        assert!(t0.elapsed() < Duration::from_secs(2), "{:?}", t0.elapsed());
        let r = run(&root, None, "demo", None, "head -c 100000 /dev/zero | tr '\\0' x", Duration::from_secs(10))
            .await
            .unwrap();
        assert!(r.truncated && r.output.len() == CAPTURE_BYTES, "{}", r.output.len());
        assert!(run(&root, None, "../x", None, "true", Duration::from_secs(1)).await.is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
