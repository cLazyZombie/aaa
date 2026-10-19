//! PR 조회(§8.80). host는 `gh`를 실행하지 않는다(결정 3): thread 머신의 agent가 그 worktree의 origin으로 부른다.
//!
//! - origin은 `git remote get-url origin`(herdr-projects가 thread origin을 읽는 방법)이다. 없으면 빈 값이다.
//! - `gh` 인자는 `aaa_common::pr`가 만든다(주소는 늘 `--` 뒤, `github.com` 밖이면 이 호출에만 `GH_HOST`).
//! - 사람에게 묻는 창을 띄우지 않고(`GH_PROMPT_DISABLED`), 10초가 넘으면 끝낸다(herdr-projects `GH_TIMEOUT`).

use std::path::Path;
use std::process::Stdio;

use aaa_common::pr::{self, GH_TIMEOUT, GhCall};
use aaa_proto::{PrAnswer, PrQuery};
use anyhow::{Context, Result, bail};

pub async fn query(cwd: &str, q: &PrQuery) -> Result<PrAnswer> {
    let dir = Path::new(cwd);
    if !dir.is_absolute() || !dir.is_dir() {
        bail!("작업 폴더가 없다: {cwd}");
    }
    let origin = origin(dir).await;
    let out = match q {
        PrQuery::View { url } => {
            let call = pr::view_call(url, &origin).with_context(|| format!("PR 주소가 아니다: {url}"))?;
            gh(dir, &call).await?
        }
        PrQuery::Find { branch } => match pr::find_call(&origin, branch) {
            None => String::new(),
            Some(call) => {
                pr::pick_listed(&gh(dir, &call).await?).context("gh pr list 출력이 JSON이 아니다")?.unwrap_or_default()
            }
        },
        PrQuery::Login => pr::sanitize(gh(dir, &pr::login_call(&origin)).await?.trim()),
        PrQuery::OpenOwn { repo } => {
            let json = gh(dir, &pr::open_own_call(&origin, repo)).await?;
            let open = pr::pick_numbers(&json).context("gh pr list 출력이 JSON이 아니다")?;
            open.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
        }
    };
    Ok(PrAnswer { origin, out })
}

pub(crate) async fn origin(dir: &Path) -> String {
    let out = tokio::time::timeout(
        GH_TIMEOUT,
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["remote", "get-url", "origin"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await;
    match out {
        Ok(Ok(o)) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

async fn gh(dir: &Path, call: &GhCall) -> Result<String> {
    let mut cmd = tokio::process::Command::new("gh");
    cmd.args(&call.args)
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(host) = &call.gh_host {
        cmd.env("GH_HOST", host);
    }
    let what = call.args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    let out = tokio::time::timeout(GH_TIMEOUT, cmd.output())
        .await
        .with_context(|| format!("gh {what}: {}초가 지나 멈췄다", GH_TIMEOUT.as_secs()))?
        .with_context(|| format!("gh {what}: 실행하지 못했다(PATH에 gh가 있는가)"))?;
    if !out.status.success() {
        let err = pr::sanitize(String::from_utf8_lossy(&out.stderr).lines().last().unwrap_or(""));
        let code = out.status.code().map(|c| format!("종료 코드 {c}")).unwrap_or_else(|| "신호로 끝남".into());
        bail!("gh {what}: {}", if err.is_empty() { code } else { format!("{err}({code})") });
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
