//! 병합 단계(§8.24). git은 이 머신의 agent만 실행한다.
//!
//! 한 번에 하는 일: thread worktree에 커밋하지 않은 변경이 없는지 보고, 원격을 받아 branch가 `remote/target`을 포함하는지 확인한다.
//! 그다음 임시 detached worktree(`remote/target`)에서 `merge --no-ff`를 하고, 병합 tree가 branch tree와 같은지 본다.
//! 같으면 thread가 시험한 것과 같은 내용이므로 push한다(force 없음). 끝나면 사용자 checkout이 깨끗한 target이면 ff-only로 맞춘다.
//! 임시 worktree는 성공·실패와 상관없이 지운다(aa가 만든 것이라 force로 지운다).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use aa_proto::{LandOutcome, MergeStep};
use anyhow::{Context, Result, bail};

const GIT_T: Duration = Duration::from_secs(30);
const NET_T: Duration = Duration::from_secs(120);

async fn git(dir: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let out = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            // 사람에게 묻는 창(인증·편집기)을 띄우지 않는다
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_EDITOR", "true")
            .stdin(std::process::Stdio::null())
            .output(),
    )
    .await
    .with_context(|| format!("git {} 시간 초과", args.join(" ")))?
    .context("git 실행")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        bail!("git {} 실패: {}", args.join(" "), if err.is_empty() { "(출력 없음)" } else { err });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

struct Steps {
    steps: Vec<MergeStep>,
}

impl Steps {
    fn push(&mut self, name: &str, t0: Instant, r: Result<String>) -> bool {
        let (ok, note) = match r {
            Ok(n) => (true, n),
            Err(e) => (false, format!("{e:#}")),
        };
        self.steps.push(MergeStep { name: name.into(), ok, note, ms: t0.elapsed().as_millis() as u64 });
        ok
    }
}

pub async fn land(
    repo: &str,
    cwd: &str,
    branch: &str,
    remote: &str,
    target: &str,
    message: &str,
) -> Result<LandOutcome> {
    for (what, v) in [("branch", branch), ("remote", remote), ("target", target)] {
        if v.is_empty() || v.starts_with('-') || v.contains("..") || v.chars().any(|c| c.is_whitespace()) {
            bail!("{what} 이름 형식이 아니다: {v:?}");
        }
    }
    let mut s = Steps { steps: Vec::new() };
    let upstream = format!("{remote}/{target}");

    // 1. 확인: thread worktree에 커밋하지 않은 변경(추적 파일)이 없다
    let t = Instant::now();
    let r = async {
        let dirty = git(Path::new(cwd), &["status", "--porcelain", "--untracked-files=no"], GIT_T).await?;
        if !dirty.is_empty() {
            bail!("thread worktree에 커밋하지 않은 변경이 있다:\n{dirty}");
        }
        Ok("커밋하지 않은 변경 없음".to_string())
    }
    .await;
    if !s.push("확인", t, r) {
        return Ok(done(s, None));
    }
    let top = PathBuf::from(git(Path::new(repo), &["rev-parse", "--show-toplevel"], GIT_T).await?);

    // 2. fetch
    let t = Instant::now();
    let r = git(&top, &["fetch", "--quiet", remote], NET_T).await.map(|_| format!("{remote}에서 받았다"));
    if !s.push("fetch", t, r) {
        return Ok(done(s, None));
    }

    // 3. 원격 일치: branch가 remote/target을 포함하고, 새 커밋이 있다
    let t = Instant::now();
    let r = async {
        let up = git(&top, &["rev-parse", "--verify", &format!("{upstream}^{{commit}}")], GIT_T).await?;
        let head = git(&top, &["rev-parse", "--verify", &format!("{branch}^{{commit}}")], GIT_T).await?;
        if up == head {
            bail!("병합할 커밋이 없다(branch가 {upstream}와 같다)");
        }
        if git(&top, &["merge-base", "--is-ancestor", &upstream, branch], GIT_T).await.is_err() {
            bail!(
                "branch가 {upstream}({})의 내용을 포함하지 않는다. thread에서 `git fetch {remote} && git merge {upstream}`을 실행해 합치고, 시험한 뒤 다시 한다",
                &up[..up.len().min(12)]
            );
        }
        Ok(format!("{upstream} {} ⊂ {branch} {}", &up[..up.len().min(12)], &head[..head.len().min(12)]))
    }
    .await;
    if !s.push("원격 일치", t, r) {
        return Ok(done(s, None));
    }

    // 4. 임시 detached worktree에서 병합
    let tmp = std::env::temp_dir().join(format!("aa-land-{}-{}", std::process::id(), nanos()));
    let tmp_s = tmp.to_string_lossy().to_string();
    let t = Instant::now();
    let r = async {
        git(&top, &["worktree", "add", "--quiet", "--detach", &tmp_s, &upstream], NET_T).await?;
        git(&tmp, &["merge", "--no-ff", "--no-edit", "-m", message, branch], NET_T).await?;
        git(&tmp, &["rev-parse", "HEAD"], GIT_T).await
    }
    .await;
    let commit = r.as_ref().ok().cloned();
    let mut ok = s.push("병합", t, r.map(|c| format!("병합 commit {}", &c[..c.len().min(12)])));
    if ok {
        // 5. tree 같음: 병합 결과가 thread가 시험한 branch와 같은 내용이다
        let t = Instant::now();
        let r = async {
            let merged = git(&tmp, &["rev-parse", "HEAD^{tree}"], GIT_T).await?;
            let tested = git(&top, &["rev-parse", &format!("{branch}^{{tree}}")], GIT_T).await?;
            if merged != tested {
                bail!("병합 tree({merged})가 branch tree({tested})와 다르다");
            }
            Ok(format!("tree {}", &merged[..merged.len().min(12)]))
        }
        .await;
        ok = s.push("tree 같음", t, r);
    }
    if ok {
        // 6. push(force 없음). 그 사이 원격이 움직였으면 거부된다.
        let t = Instant::now();
        let r = git(&tmp, &["push", "--quiet", remote, &format!("HEAD:refs/heads/{target}")], NET_T)
            .await
            .map(|_| format!("{remote} {target}에 올렸다"))
            .map_err(|e| {
                anyhow::anyhow!("{e:#}\n원격 {target}이 그 사이 움직였으면 thread에서 다시 합친 뒤 다시 한다")
            });
        ok = s.push("push", t, r);
    }
    if ok {
        // 7. 사용자 checkout: 깨끗한 target이면 ff-only로 맞추고, 아니면 그대로 둔다(실패로 치지 않는다)
        let t = Instant::now();
        let note = user_checkout(&top, remote, target).await;
        s.push("사용자 checkout", t, Ok(note));
    }
    // 8. 정리: 임시 worktree(aa가 만든 것)
    let t = Instant::now();
    let r = async {
        if tmp.exists() {
            git(&top, &["worktree", "remove", "--force", &tmp_s], GIT_T).await?;
        }
        let _ = git(&top, &["worktree", "prune"], GIT_T).await;
        Ok("임시 worktree를 지웠다".to_string())
    }
    .await;
    let cleaned = s.push("정리", t, r);
    Ok(LandOutcome { ok: ok && cleaned, steps: s.steps, commit: if ok { commit } else { None } })
}

fn done(s: Steps, commit: Option<String>) -> LandOutcome {
    LandOutcome { ok: false, steps: s.steps, commit }
}

fn nanos() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

async fn user_checkout(top: &Path, remote: &str, target: &str) -> String {
    let cur = git(top, &["symbolic-ref", "--quiet", "--short", "HEAD"], GIT_T).await.unwrap_or_default();
    if cur != target {
        return format!("그대로 둠(지금 {})", if cur.is_empty() { "detached" } else { &cur });
    }
    match git(top, &["status", "--porcelain", "--untracked-files=no"], GIT_T).await {
        Ok(d) if d.is_empty() => {}
        Ok(_) => return "그대로 둠(고친 파일이 있다)".into(),
        Err(e) => return format!("그대로 둠({e:#})"),
    }
    match git(top, &["merge", "--ff-only", "--quiet", &format!("{remote}/{target}")], GIT_T).await {
        Ok(_) => format!("{target} branch를 {remote}/{target}에 맞췄다(ff-only)"),
        Err(e) => format!("그대로 둠(ff-only 실패: {e:#})"),
    }
}
