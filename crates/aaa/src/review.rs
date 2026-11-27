//! 검수 CLI는 대상 SHA를 고정하고 자기 pane의 agent에 보낸다.
use aaa_common::{child_process::StdCommandExt as _, util};
use aaa_proto::{ReviewRequest, ReviewResponse, ReviewStart, ReviewTarget};
use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use std::process::{Command, ExitCode};

#[derive(Subcommand)]
pub enum Cmd {
    /// 명시한 profile로 커밋된 Git 후보를 원격 agent에 맡긴다
    Start {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        base: String,
        #[arg(long, default_value = "HEAD")]
        commit: String,
        /// 응답 유실 뒤 같은 작업을 찾을 때 재사용한다
        #[arg(long)]
        id: Option<String>,
    },
    /// 검수 상태를 본다
    Status {
        id: String,
        /// 계정 배정 또는 실행 결과가 나올 때까지 기다린다
        #[arg(long)]
        wait: bool,
    },
    /// 성공한 결과를 JSON으로 받는다
    Fetch { id: String },
    /// 실행 전 취소하거나 thread 머신 실행기의 종료를 요청한다
    Cancel { id: String },
}

fn git(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(aaa_common::diff::git_prefix())
        .args(args)
        .capture_output()
        .context("검수 Git 실행")?;
    ensure!(output.status.success(), "검수 git {} 실패({})", args[0], output.status);
    Ok(String::from_utf8(output.stdout)?)
}

fn resolve(rev: &str, kind: &str) -> Result<String> {
    Ok(git(&["rev-parse", "--verify", "--end-of-options", &format!("{rev}^{{{kind}}}")])?.trim_end().into())
}

fn earlier(target: &ReviewTarget) -> Option<bool> {
    let head = resolve("HEAD", "commit").ok()?;
    if head != target.commit {
        return Some(true);
    }
    let status = git(&["status", "--porcelain", "--untracked-files=normal"]).ok()?;
    Some(!status.is_empty())
}

pub(crate) fn run(json: bool, cmd: Cmd) -> Result<ExitCode> {
    tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async move {
        let fetch = matches!(cmd, Cmd::Fetch { .. });
        let response = match cmd {
            Cmd::Start { profile, base, commit, id } => {
                let cwd = git(&["rev-parse", "--show-toplevel"])?.trim_end().to_string();
                let base = resolve(&base, "commit")?;
                let commit = resolve(&commit, "commit")?;
                let tree = resolve(&commit, "tree")?;
                let id = id.unwrap_or_else(util::new_id);
                aaa_agent::review::send(ReviewRequest::Start(ReviewStart {
                    id: id.clone(),
                    profile,
                    cwd,
                    target: ReviewTarget { base, commit, tree },
                }))
                .await
                .with_context(|| format!("검수 시작 {id}"))?
            }
            Cmd::Status { id, wait } => loop {
                let response = aaa_agent::review::send(ReviewRequest::Status { id: id.clone(), wait }).await?;
                if !wait || !response.job.state.active() {
                    break response;
                }
            },
            Cmd::Fetch { id } => aaa_agent::review::send(ReviewRequest::Fetch { id }).await?,
            Cmd::Cancel { id } => aaa_agent::review::send(ReviewRequest::Cancel { id }).await?,
        };
        display(json, fetch, response)
    })
}

fn display(json: bool, fetch: bool, response: ReviewResponse) -> Result<ExitCode> {
    let ReviewResponse { job, result } = response;
    let earlier = earlier(&job.target);
    if json || fetch {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "job": job, "result": result, "earlier": earlier,
            }))?
        );
    } else {
        let state = serde_json::to_value(job.state)?;
        let freshness = match earlier {
            Some(true) => " · 이전 판",
            Some(false) => "",
            None => " · 현재 판 확인 불가",
        };
        println!(
            "{} {} · {} · {}{}",
            job.id,
            state.as_str().unwrap_or_default(),
            job.target.commit,
            job.reason,
            freshness
        );
    }
    Ok(if fetch && result.is_none() { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}
