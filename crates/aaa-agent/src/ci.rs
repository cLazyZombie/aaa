//! host 머신(또는 설정한 머신)의 gh 로그인으로 읽기만 한다. host에는 줄인 응답만 보낸다.
use aaa_common::child_process::TokioCommandExt as _;
use aaa_common::{ci, github};
use aaa_proto::{CiAnswer, CiQuery};
use anyhow::{Context, Result, bail};
use std::{path::Path, process::Stdio};
use tokio::io::AsyncReadExt;

pub async fn query(cwd: &str, q: &CiQuery) -> Result<CiAnswer> {
    let dir = Path::new(cwd);
    if !dir.is_absolute() || !dir.is_dir() {
        bail!("CI 조회 저장소 폴더가 없다");
    }
    let origin = crate::github::origin(dir).await;
    if github::host_of(&origin) != "github.com" {
        bail!("CI origin은 github.com이어야 한다");
    }
    let repo = github::normalize_origin(&origin).context("CI 조회 origin이 github.com 저장소가 아니다")?;
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
    let (bytes, truncated, status) = tokio::time::timeout(github::GH_TIMEOUT, async {
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
                // 꼬리만 남기기 전에 실패 시험과 오류 주변을 모은다.
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
        log_answer(&out, truncated, scan)
    } else {
        ci::response(&out, status.success(), q)?
    };
    answer.origin = origin;
    Ok(answer)
}

fn log_answer(tail: &str, mut truncated: bool, scan: ci::FailureScan) -> CiAnswer {
    let (failures, failure_context) = scan.finish();
    let context_bytes = failure_context.as_ref().map_or(0, |c| c.excerpts.iter().map(|e| e.byte_len()).sum::<usize>());
    let mut log = ci::clean_log(tail);
    // 문맥과 꼬리에 기존 job 상한을 함께 적용한다. 꼬리는 끝을 보존한다.
    let mut start = log.len().saturating_sub(ci::LOG_JOB_CAP - context_bytes);
    while !log.is_char_boundary(start) {
        start += 1;
    }
    truncated |= start > 0 || failure_context.as_ref().is_some_and(|c| c.truncated);
    log.drain(..start);
    CiAnswer { log, truncated, failures, failure_context, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn early_error_and_stack_survive_the_job_tail_cut_with_secrets_masked() {
        let secret = aaa_test_support::token("ci-preserved-context");
        aaa_common::redact::remember(&secret);
        let mut log = format!(
            "web-e2e\tRun web E2E\t2026-11-17T12:00:00Z [e2e] Error: Route is already handled! {secret}\r\n\
             web-e2e\tRun web E2E\t    at Route.continue (router.mjs:41:8)\r\n\
             web-e2e\tRun web E2E\t2026-11-17T12:00:01Z ## 실행 {{\"event\":\"job-end\"}}\n"
        );
        log.push_str(&"가".repeat(24000));
        log.push_str(&format!("\n가림 반복: {secret}"));
        log.push_str("\n## 실행 {\"event\":\"suite-end\",\"failed\":1,\"jobs\":56}");
        let mut scan = ci::FailureScan::default();
        for chunk in log.as_bytes().chunks(7) {
            scan.feed(chunk);
        }
        let raw_tail = String::from_utf8_lossy(&log.as_bytes()[log.len() - 32768..]);
        let answer = log_answer(&raw_tail, true, scan);
        assert!(answer.truncated);
        assert_eq!(answer.failures, ["web: 1/56 작업 실패"]);
        assert!(answer.log.ends_with("\"jobs\":56}"));
        assert!(!answer.log.contains(&secret[..8]), "꼬리에 인증 조각이 남았다");
        let context = answer.failure_context.unwrap();
        assert!(!context.truncated);
        assert_eq!(context.excerpts.len(), 1);
        let excerpt = &context.excerpts[0];
        assert_eq!(excerpt.step, "Run web E2E");
        assert!(excerpt.text.contains("Error: Route is already handled!"));
        assert!(excerpt.text.contains("at Route.continue (router.mjs:41:8)"));
        assert!(!excerpt.text.contains(&secret[..8]), "가린 인증 조각이 남았다");
        assert!(answer.log.len() + excerpt.byte_len() <= 32768);
    }
}
