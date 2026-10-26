//! host 머신(또는 설정한 머신)의 gh 로그인으로 읽기만 한다. host에는 줄인 응답만 보낸다.
use aaa_common::child_process::TokioCommandExt as _;
use aaa_common::{ci, pr};
use aaa_proto::{CiAnswer, CiQuery};
use anyhow::{Context, Result, bail};
use std::{path::Path, process::Stdio};
use tokio::io::AsyncReadExt;

pub async fn query(cwd: &str, q: &CiQuery) -> Result<CiAnswer> {
    let dir = Path::new(cwd);
    if !dir.is_absolute() || !dir.is_dir() {
        bail!("CI 조회 저장소 폴더가 없다");
    }
    let origin = crate::pr::origin(dir).await;
    if pr::host_of(&origin) != "github.com" {
        bail!("CI origin은 github.com이어야 한다");
    }
    let repo = pr::normalize_origin(&origin).context("CI 조회 origin이 github.com 저장소가 아니다")?;
    let (args, log) = match q {
        CiQuery::Log { run, job } => {
            if *run == 0 || *job == 0 {
                bail!("run/job이 0이다");
            }
            (
                vec![
                    "run".into(),
                    "view".into(),
                    run.to_string(),
                    "--repo".into(),
                    format!("github.com/{repo}"),
                    "--job".into(),
                    job.to_string(),
                    "--log-failed".into(),
                ],
                true,
            )
        }
        _ => (ci::api_args(&repo, q)?, false),
    };
    let mut child = crate::gh::command()?
        .args(&args)
        .env("GH_HOST", "github.com")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn_guarded()
        .context("CI gh 실행 실패(PATH와 host 머신 로그인을 확인한다)")?;
    let mut stdout = child.stdout.take().unwrap();
    let cap = if log { ci::LOG_JOB_CAP } else { 4 * 1024 * 1024 };
    let mut scan = ci::FailureScan::default();
    let (bytes, truncated, status) = tokio::time::timeout(pr::GH_TIMEOUT, async {
        let mut bytes = Vec::new();
        let mut truncated = false;
        let mut buf = [0u8; 8192];
        loop {
            let n = stdout.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..n]);
            if log {
                // 꼬리만 남기기 전에 실패 시험을 모은다(앞쪽 실패가 잘려도 실패 집합에 남는다).
                scan.feed(&buf[..n]);
            }
            if bytes.len() > cap {
                if !log {
                    bail!("CI API 응답 크기 상한이다");
                }
                let extra = bytes.len() - cap;
                bytes.drain(..extra);
                truncated = true;
            }
        }
        Ok::<_, anyhow::Error>((bytes, truncated, child.wait().await?))
    })
    .await
    .context("CI gh 조회 시간 상한이다")??;
    if status.code() == Some(4) {
        crate::gh::check(status).context("CI gh")?;
    }
    let out = String::from_utf8_lossy(&bytes);
    let mut answer = if log {
        if !status.success() {
            bail!("CI 실패 로그를 읽지 못했다");
        }
        // ANSI·제어문자는 지운다. 로그는 실행할 지시가 아닌 JSON 문자열로만 전달한다.
        let mut clean: String = out.chars().filter(|c| !c.is_control() || matches!(c, '\n' | '\t')).collect();
        if clean.len() > ci::LOG_JOB_CAP {
            let mut end = ci::LOG_JOB_CAP;
            while !clean.is_char_boundary(end) {
                end -= 1;
            }
            clean.truncate(end);
        }
        CiAnswer { log: clean, truncated, failures: scan.finish(), ..Default::default() }
    } else {
        ci::response(&out, status.success(), q)?
    };
    answer.origin = origin;
    Ok(answer)
}
