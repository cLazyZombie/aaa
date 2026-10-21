//! Git 감시. checkout의 작업 파일·ref·FETCH_HEAD를 바꾸지 않고 객체만 받는다.
use aaa_common::{git_watch as git, pr};
use aaa_proto::{GitAnswer, GitChange, GitCommit, GitQuery};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::{Output, Stdio},
};
use tokio::io::AsyncReadExt;

async fn run(dir: &Path, args: &[String]) -> Result<Output> {
    let mut child = tokio::process::Command::new("git")
        .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("Git 감시 명령 실행 실패")?;
    let mut stdout = child.stdout.take().unwrap().take(4 * 1024 * 1024 + 1);
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes).await?;
    if bytes.len() > 4 * 1024 * 1024 {
        bail!("Git 감시 출력 크기 상한이다");
    }
    Ok(Output { status: child.wait().await?, stdout: bytes, stderr: vec![] })
}
async fn checked(dir: &Path, args: &[String]) -> Result<String> {
    let out = run(dir, args).await?;
    if !out.status.success() {
        bail!("Git {} 실패(종료 코드 {:?})", args[0], out.status.code());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| (*s).into()).collect()
}
async fn ensure(dir: &Path, origin: &str, sha: &str) -> Result<()> {
    if !git::valid_sha(sha) {
        bail!("Git SHA 형식이 아니다");
    }
    if !run(dir, &args(&["cat-file", "-e", &format!("{sha}^{{commit}}")])).await?.status.success() {
        checked(
            dir,
            &args(&["fetch", "--no-tags", "--no-write-fetch-head", "--recurse-submodules=no", "--", origin, sha]),
        )
        .await?;
    }
    Ok(())
}
pub async fn query(cwd: &str, q: &GitQuery) -> Result<GitAnswer> {
    let dir = Path::new(cwd);
    if !dir.is_absolute() || !dir.is_dir() {
        bail!("Git 감시 저장소 폴더가 없다");
    }
    tokio::time::timeout(git::QUERY_TIMEOUT, query_inner(dir, q)).await.context("Git 감시 조회 시간 상한이다")?
}
async fn query_inner(dir: &Path, q: &GitQuery) -> Result<GitAnswer> {
    let origin = checked(dir, &args(&["remote", "get-url", "origin"])).await?;
    let origin = origin.trim();
    if origin.is_empty() || origin.starts_with('-') || origin.chars().any(char::is_control) {
        bail!("Git origin 형식이 아니다");
    }
    let mut answer = GitAnswer { origin_hash: hex::encode(Sha256::digest(origin.as_bytes())), ..Default::default() };
    match q {
        GitQuery::Heads { branches } => {
            git::validate_branches(branches)?;
            let mut a = args(&["ls-remote", "--heads", "--refs", "--", origin]);
            a.extend(branches.iter().map(|b| format!("refs/heads/{b}")));
            answer.heads = git::heads(&checked(dir, &a).await?, branches)?;
            // 기준 head도 보유해야 뒤의 force push에서 이전 객체를 비교할 수 있다.
            for sha in answer.heads.values().flatten() {
                ensure(dir, origin, sha).await?;
            }
        }
        GitQuery::MergedIntoMain { branch, commits } => {
            if branch.as_ref().is_some_and(|b| !git::valid_branch(b))
                || commits.len() > 8
                || commits.iter().any(|c| !(7..=64).contains(&c.len()) || !c.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                bail!("병합 조회 branch 또는 커밋 형식이 아니다");
            }
            // 로컬 main·오래된 remote ref를 신뢰하지 않는다. ref와 FETCH_HEAD는 바꾸지 않는다.
            let branches = vec!["main".to_string()];
            let heads = git::heads(
                &checked(dir, &args(&["ls-remote", "--heads", "--refs", "--", origin, "refs/heads/main"])).await?,
                &branches,
            )?;
            let Some(main) = heads["main"].as_deref() else { return Ok(answer) };
            ensure(dir, origin, main).await?;
            let mut candidates = Vec::new();
            if let Some(branch) = branch {
                candidates.push(format!("refs/heads/{branch}"));
            }
            candidates.extend(commits.iter().cloned());
            for candidate in candidates {
                let out = run(dir, &args(&["rev-parse", "--verify", &format!("{candidate}^{{commit}}")])).await?;
                if !out.status.success() {
                    continue; // 사라진 branch·없거나 모호한 축약 SHA는 완료 근거가 아니다.
                }
                let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !git::valid_sha(&sha) {
                    bail!("병합 조회 커밋 형식이 아니다");
                }
                let out = run(dir, &args(&["merge-base", "--is-ancestor", &sha, main])).await?;
                match out.status.code() {
                    Some(0) => {
                        answer.merged_commit = Some(sha);
                        break;
                    }
                    Some(1) => {}
                    _ => bail!("Git 병합 조상 판정 실패"),
                }
            }
        }
        GitQuery::Change { branch, before, after } => {
            git::validate_branches(std::slice::from_ref(branch))?;
            if before == after {
                bail!("Git head 변화가 없다");
            }
            for sha in before.iter().chain(after.iter()) {
                ensure(dir, origin, sha).await?;
            }
            let ff = if let (Some(a), Some(b)) = (before, after) {
                let out = run(dir, &args(&["merge-base", "--is-ancestor", a, b])).await?;
                match out.status.code() {
                    Some(0) => true,
                    Some(1) => false,
                    _ => bail!("Git 조상 판정 실패"),
                }
            } else {
                true
            };
            let mut commits = vec![];
            let mut truncated = false;
            if let Some(head) = after {
                let range = before.as_ref().map_or_else(|| head.clone(), |base| format!("{base}..{head}"));
                let text = checked(
                    dir,
                    &args(&["log", "-n", &(git::MAX_COMMITS + 1).to_string(), "--format=%H%x00%s", &range, "--"]),
                )
                .await?;
                for line in text.lines() {
                    let (sha, subject) = line.split_once('\0').context("Git log 출력 형식이 아니다")?;
                    if !git::valid_sha(sha) {
                        bail!("Git log SHA 형식이 아니다");
                    }
                    if commits.len() == git::MAX_COMMITS {
                        truncated = true;
                        break;
                    }
                    commits.push(GitCommit {
                        sha: sha.into(),
                        subject: pr::sanitize(subject),
                        owners: aaa_common::ci::owners(subject),
                    });
                }
            }
            // 빈 tree를 객체 저장소에만 써서 branch 생성·삭제의 파일 수도 같은 diff로 센다.
            let empty = checked(dir, &args(&["hash-object", "-w", "-t", "tree", "--stdin"])).await?;
            let text = checked(
                dir,
                &args(&[
                    "diff",
                    "--shortstat",
                    before.as_deref().unwrap_or(empty.trim()),
                    after.as_deref().unwrap_or(empty.trim()),
                    "--",
                ]),
            )
            .await?;
            let changed_files = if text.trim().is_empty() {
                0
            } else {
                text.split_whitespace().next().context("Git diff 파일 수가 없다")?.parse()?
            };
            answer.change = Some(GitChange {
                branch: branch.clone(),
                before: before.clone(),
                after: after.clone(),
                kind: git::kind(before.as_deref(), after.as_deref(), ff).unwrap().into(),
                commits,
                changed_files,
                truncated,
            });
        }
    }
    Ok(answer)
}
