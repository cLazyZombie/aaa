//! thread 자리(worktree·작업 폴더)와 보고 회수(§8.7, §8.12). git은 이 머신의 agent만 실행한다.
//!
//! - 작업 폴더는 `<cwd>/.aaa/<project>-<thread>/`이고 brief.md·report.md·library/를 둔다. repo면 `.aaa/`를 git exclude에 넣는다.
//! - report.md는 regular file만 본다(symlink는 무시). library는 symlink를 따라가지 않고 상한을 넘는 파일은 건너뛴다.
//! - 정리는 force 없이 한다: 고치지 않은 worktree만 지워지고, 기준에 병합된 branch만 지운다.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use aaa_config::Profile;
use aaa_proto::{
    Cleanup, Launch, LibFile, ProfileView, ReportSnapshot, Skipped, ThreadKind, ThreadPrepare, ThreadReady, fill_brief,
    slug, valid_project, valid_thread,
};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// report.md 본문 상한(이보다 크면 회수하지 않는다)
pub const MAX_REPORT_BYTES: u64 = 4 << 20;

#[derive(Debug, Clone, Default)]
pub struct ThreadsConfig {
    pub work_root: PathBuf,
    /// hook 명령에 쓰는 aaa 실행 파일
    pub exe: PathBuf,
    pub profiles: BTreeMap<String, Profile>,
}

pub fn profile_views(cfg: &ThreadsConfig) -> Vec<ProfileView> {
    cfg.profiles
        .iter()
        .map(|(name, p)| ProfileView {
            name: name.clone(),
            harness: p.harness.clone(),
            model: Some(p.model.clone()).filter(|m| !m.is_empty()),
            effort: Some(p.effort.clone()).filter(|m| !m.is_empty()),
            description: p.description.clone(),
        })
        .collect()
}

/// 있는 작업 폴더에 brief.md를 다시 쓰고, 새 대화로 띄울 launch를 만든다(claude는 새 session ID).
/// brief의 작업 폴더·aaa 실행 파일 자리는 이 머신 값으로 채운다(`fill_brief`).
pub fn rebrief(
    cfg: &ThreadsConfig,
    work_dir: &str,
    brief: &str,
    mut launch: Launch,
    profile: Option<&str>,
) -> Result<Launch> {
    let dir = check_work_dir(work_dir)?;
    std::fs::write(dir.join("brief.md"), fill_brief(brief, &dir.to_string_lossy(), &cfg.exe.to_string_lossy()))?;
    if let Some(name) = profile {
        // profile이 모든 설정을 가진다: 옛 model·args는 쓰지 않는다(herdr-projects threads.rs `restart`)
        return launch_in(cfg, name, &dir);
    }
    if let Some(i) = launch.argv.iter().position(|a| a == "--session-id")
        && i + 1 < launch.argv.len()
    {
        let key = uuid_v4();
        launch.argv[i + 1] = key.clone();
        launch.session_key = Some(key);
    }
    Ok(launch)
}

fn uuid_v4() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h = hex::encode(b);
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// profile → 실행 argv. resume argv는 같은 model·effort·args·env를 쓰고 `{session}` 자리에 harness 세션 ID가 들어간다.
pub fn resolve(name: &str, p: &Profile) -> Result<Launch> {
    let base = |default: &str| if p.command.is_empty() { vec![default.to_string()] } else { p.command.clone() };
    let env: Vec<(String, String)> = p.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let (argv, resume_argv, session_key) = match p.harness.as_str() {
        "claude" => {
            let mut flags = Vec::new();
            if !p.model.is_empty() {
                flags.extend(["--model".to_string(), p.model.clone()]);
            }
            if !p.effort.is_empty() {
                flags.extend(["--effort".to_string(), p.effort.clone()]);
            }
            flags.extend(p.args.iter().cloned());
            let key = uuid_v4();
            let mut argv = base("claude");
            argv.extend(flags.iter().cloned());
            argv.extend(["--session-id".to_string(), key.clone()]);
            let mut resume = base("claude");
            resume.extend(flags);
            resume.extend(["--resume".to_string(), "{session}".to_string()]);
            (argv, resume, Some(key))
        }
        "codex" => {
            let mut flags = Vec::new();
            if !p.model.is_empty() {
                flags.extend(["-m".to_string(), p.model.clone()]);
            }
            if !p.effort.is_empty() {
                flags.extend(["-c".to_string(), format!("model_reasoning_effort={}", p.effort)]);
            }
            flags.extend(p.args.iter().cloned());
            let mut argv = base("codex");
            argv.extend(flags.iter().cloned());
            let mut resume = base("codex");
            resume.extend(["resume".to_string(), "{session}".to_string()]);
            resume.extend(flags);
            (argv, resume, None)
        }
        "shell" | "custom" => {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
            let mut argv = base(&shell);
            argv.extend(p.args.iter().cloned());
            (argv.clone(), argv, None)
        }
        other => bail!("profile {name}: 모르는 harness {other:?}"),
    };
    Ok(Launch { profile: name.to_string(), harness: p.harness.clone(), argv, env, resume_argv, session_key })
}

pub(crate) async fn git(dir: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let out = tokio::time::timeout(
        timeout,
        tokio::process::Command::new("git").arg("-C").arg(dir).args(args).stdin(std::process::Stdio::null()).output(),
    )
    .await
    .with_context(|| format!("git {} 시간 초과", args.join(" ")))?
    .context("git 실행")?;
    if !out.status.success() {
        bail!("git {} 실패: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

const GIT_T: Duration = Duration::from_secs(30);

/// repo의 `info/exclude`에 `.aaa/`를 한 번만 넣는다(작업 폴더가 git 상태에 보이지 않게)
async fn exclude_work_dirs(repo: &Path) -> Result<()> {
    let common = git(repo, &["rev-parse", "--git-common-dir"], GIT_T).await?;
    let common = if Path::new(&common).is_absolute() { PathBuf::from(common) } else { repo.join(common) };
    let path = common.join("info").join("exclude");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if !text.lines().any(|l| l.trim() == ".aaa/") {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
        std::fs::write(&path, format!("{text}{sep}.aaa/\n"))?;
    }
    Ok(())
}

pub async fn prepare(cfg: &ThreadsConfig, p: ThreadPrepare) -> Result<ThreadReady> {
    if !valid_project(&p.project) || !valid_thread(&p.thread) {
        bail!("프로젝트·thread 이름 형식이 아니다: {} {}", p.project, p.thread);
    }
    let profile = find_profile(cfg, &p.profile)?;
    let (cwd, branch, base) = match p.kind {
        ThreadKind::Worktree => {
            let repo = PathBuf::from(p.repo.as_deref().context("worktree thread에는 repo 경로가 필요하다")?);
            let top = PathBuf::from(git(&repo, &["rev-parse", "--show-toplevel"], GIT_T).await?);
            let has_origin = git(&top, &["remote"], GIT_T).await?.lines().any(|l| l == "origin");
            if has_origin {
                // 원격이 늦어도 시작은 막지 않는다(로컬 ref로 진행)
                if let Err(e) = git(&top, &["fetch", "--quiet", "origin"], Duration::from_secs(15)).await {
                    tracing::warn!("fetch 실패(로컬 ref로 진행): {e:#}");
                }
            }
            let base = match &p.base {
                Some(b) => b.clone(),
                None => match git(&top, &["rev-parse", "--abbrev-ref", "origin/HEAD"], GIT_T).await {
                    Ok(b) if has_origin && !b.is_empty() && b != "origin/HEAD" => b,
                    _ => match git(&top, &["symbolic-ref", "--short", "HEAD"], GIT_T).await {
                        Ok(b) => b,
                        Err(_) => git(&top, &["rev-parse", "HEAD"], GIT_T).await?,
                    },
                },
            };
            let s = slug(&p.title);
            let branch = if s.is_empty() {
                format!("aaa/{}/{}", p.project, p.thread)
            } else {
                format!("aaa/{}/{}-{s}", p.project, p.thread)
            };
            let name = top.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "repo".into());
            let path = cfg.work_root.join(name).join(format!("{}-{}", p.project, p.thread));
            if path.exists() {
                bail!("worktree 자리가 이미 있다: {}", path.display());
            }
            std::fs::create_dir_all(path.parent().unwrap())?;
            let path_s = path.to_string_lossy().to_string();
            git(&top, &["worktree", "add", "-b", &branch, &path_s, &base], Duration::from_secs(120)).await?;
            exclude_work_dirs(&top).await?;
            (path, Some(branch), Some(base))
        }
        ThreadKind::Dir => {
            let path = cfg.work_root.join(&p.project).join(&p.thread);
            if path.exists() {
                bail!("작업 폴더가 이미 있다: {}", path.display());
            }
            std::fs::create_dir_all(&path)?;
            (path, None, None)
        }
    };
    let work_dir = cwd.join(".aaa").join(format!("{}-{}", p.project, p.thread));
    std::fs::create_dir_all(work_dir.join("library"))?;
    std::fs::write(
        work_dir.join("brief.md"),
        fill_brief(&p.brief, &work_dir.to_string_lossy(), &cfg.exe.to_string_lossy()),
    )?;
    let launch = with_hooks(cfg, resolve(&p.profile, profile)?, &work_dir)?;
    Ok(ThreadReady {
        cwd: cwd.to_string_lossy().to_string(),
        work_dir: work_dir.to_string_lossy().to_string(),
        branch,
        base,
        launch,
    })
}

fn find_profile<'a>(cfg: &'a ThreadsConfig, name: &str) -> Result<&'a Profile> {
    cfg.profiles.get(name).with_context(|| {
        format!("이 머신 agent.toml에 profile `{name}`가 없다(있는 것: {:?})", cfg.profiles.keys().collect::<Vec<_>>())
    })
}

/// 있는 작업 폴더에서 profile을 다시 해석한다(brief.md는 host가 준 것을 이미 썼다)
fn launch_in(cfg: &ThreadsConfig, name: &str, work_dir: &Path) -> Result<Launch> {
    with_hooks(cfg, resolve(name, find_profile(cfg, name)?)?, work_dir)
}

/// harness별 상태 hook을 launch에 붙인다
fn with_hooks(cfg: &ThreadsConfig, mut launch: Launch, work_dir: &Path) -> Result<Launch> {
    if launch.harness == "claude" {
        // 실행마다 hook 설정을 준다(사용자 ~/.claude 설정은 고치지 않는다). resume도 같은 설정을 쓴다.
        let settings = work_dir.join("claude-settings.json");
        std::fs::write(&settings, serde_json::to_vec_pretty(&crate::hooks::claude_settings(&cfg.exe))?)?;
        let s = settings.to_string_lossy().to_string();
        for argv in [&mut launch.argv, &mut launch.resume_argv] {
            argv.insert(1, s.clone());
            argv.insert(1, "--settings".into());
        }
    }
    if launch.harness == "codex" {
        // 턴이 끝날 때마다 notify로 상태와 thread ID(resume에 쓴다)를 받는다
        let n = crate::hooks::codex_notify(&cfg.exe);
        for argv in [&mut launch.argv, &mut launch.resume_argv] {
            argv.insert(1, n.clone());
            argv.insert(1, "-c".into());
        }
    }
    Ok(launch)
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

/// report.md hash. 없거나 regular file이 아니면 None
pub fn report_hash(work_dir: &Path) -> Option<String> {
    let path = work_dir.join("report.md");
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.file_type().is_file() || meta.len() > MAX_REPORT_BYTES {
        return None;
    }
    sha256_file(&path).ok()
}

/// work_dir가 thread 작업 폴더 형식인가(host가 준 경로를 그대로 믿지 않는다)
fn check_work_dir(work_dir: &str) -> Result<PathBuf> {
    let p = PathBuf::from(work_dir);
    let ok = p.is_absolute()
        && p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == ".aaa")
        && p.components().all(|c| !matches!(c, Component::ParentDir));
    if !ok {
        bail!("thread 작업 폴더 형식이 아니다: {work_dir}");
    }
    let meta = std::fs::symlink_metadata(&p).with_context(|| format!("작업 폴더가 없다: {work_dir}"))?;
    if !meta.is_dir() {
        bail!("작업 폴더가 폴더가 아니다(symlink 등): {work_dir}");
    }
    Ok(p)
}

pub fn collect(work_dir: &str, max_library_bytes: u64) -> Result<ReportSnapshot> {
    let dir = check_work_dir(work_dir)?;
    let path = dir.join("report.md");
    let meta = std::fs::symlink_metadata(&path).context("report.md가 없다")?;
    if !meta.file_type().is_file() {
        bail!("report.md가 regular file이 아니다");
    }
    if meta.len() > MAX_REPORT_BYTES {
        bail!("report.md가 너무 크다({} bytes)", meta.len());
    }
    let report = std::fs::read(&path)?;
    let hash = hex::encode(Sha256::digest(&report));
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut total = 0u64;
    let lib = dir.join("library");
    if std::fs::symlink_metadata(&lib).is_ok_and(|m| m.is_dir()) {
        walk(&lib, &lib, &mut |rel, meta, abs| {
            if meta.file_type().is_symlink() {
                skipped
                    .push(Skipped { path: rel, reason: "symlink는 따라가지 않는다".into(), lazy_size: None });
            } else if !meta.is_file() {
                skipped.push(Skipped { path: rel, reason: "일반 파일이 아니다".into(), lazy_size: None });
            } else if total + meta.len() > max_library_bytes {
                skipped.push(Skipped {
                    path: rel,
                    reason: format!("복사 상한({} MB)을 넘는다. agent에 남는다", max_library_bytes >> 20),
                    lazy_size: Some(meta.len()),
                });
            } else {
                match sha256_file(abs) {
                    Ok(sha256) => {
                        total += meta.len();
                        files.push(LibFile { path: rel, size: meta.len(), sha256 });
                    }
                    Err(e) => {
                        skipped.push(Skipped { path: rel, reason: format!("읽기 실패: {e}"), lazy_size: None })
                    }
                }
            }
        })?;
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ReportSnapshot { hash, report: aaa_proto::B64(report), files, skipped })
}

fn walk(root: &Path, dir: &Path, f: &mut impl FnMut(String, std::fs::Metadata, &Path)) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let meta = std::fs::symlink_metadata(&path)?;
        let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
        if meta.is_dir() {
            walk(root, &path, f)?;
        } else {
            f(rel, meta, &path);
        }
    }
    Ok(())
}

/// library 파일 한 조각. 경로는 library 아래 상대 경로만, symlink는 거부한다.
pub fn read_chunk(work_dir: &str, rel: &str, offset: u64, len: u32) -> Result<(Vec<u8>, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let dir = check_work_dir(work_dir)?;
    let relp = Path::new(rel);
    if rel.is_empty() || !relp.components().all(|c| matches!(c, Component::Normal(_))) {
        bail!("library 상대 경로가 아니다: {rel}");
    }
    let mut cur = dir.join("library");
    for c in relp.components() {
        cur = cur.join(c);
        let meta = std::fs::symlink_metadata(&cur)?;
        if meta.file_type().is_symlink() {
            bail!("symlink는 읽지 않는다: {rel}");
        }
    }
    let mut f = std::fs::File::open(&cur)?;
    let size = f.metadata()?.len();
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = vec![0u8; len.min(512 * 1024) as usize];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok((buf, offset + n as u64 >= size))
}

/// 사후 평가자 사건 묶음을 `<work_dir>/inbox/<이름>.json`에 쓴다(§8.13). 다른 곳에는 쓰지 않는다.
pub fn write_inbox(work_dir: &str, rel: &str, data: &[u8]) -> Result<()> {
    const MAX: usize = 8 << 20;
    let dir = check_work_dir(work_dir)?;
    let name = rel
        .strip_prefix("inbox/")
        .and_then(|n| n.strip_suffix(".json"))
        .filter(|n| !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
        .with_context(|| format!("inbox/<이름>.json만 쓴다: {rel}"))?;
    if data.len() > MAX {
        bail!("사건 묶음이 너무 크다({}바이트)", data.len());
    }
    let inbox = dir.join("inbox");
    match std::fs::symlink_metadata(&inbox) {
        Ok(m) if !m.is_dir() => bail!("inbox가 폴더가 아니다(symlink 등)"),
        Ok(_) => {}
        Err(_) => {
            std::fs::create_dir(&inbox)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o700))?;
            }
        }
    }
    let tmp = inbox.join(format!(".{name}.tmp"));
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, inbox.join(format!("{name}.json")))?;
    Ok(())
}

/// thread 정리(herdr-projects threads.rs `clean`). worktree는 force 없이 지운다. branch는 PR이 병합됐고(`merged_head`가
/// 있다) worktree가 지워졌을 때만 `delete_branch`로 지운다.
pub async fn cleanup(
    repo: Option<&str>,
    cwd: &str,
    branch: Option<&str>,
    merged_head: Option<&str>,
    keep_worktree: bool,
) -> Result<Cleanup> {
    let mut c = Cleanup { worktree_removed: false, branch_deleted: false, notes: Vec::new() };
    let Some(repo) = repo else {
        c.notes.push("작업 폴더는 남긴다(repo 없는 thread)".into());
        return Ok(c);
    };
    let repo = PathBuf::from(repo);
    if keep_worktree {
        c.notes.push("worktree를 남겼다(요청)".into());
    } else {
        match git(&repo, &["worktree", "remove", cwd], Duration::from_secs(60)).await {
            Ok(_) => c.worktree_removed = true,
            // force는 쓰지 않는다: 고친 파일이 있으면 남는다
            Err(e) => c.notes.push(format!("worktree를 남겼다: {e:#}")),
        }
        let _ = git(&repo, &["worktree", "prune"], GIT_T).await;
    }
    if let Some(branch) = branch.filter(|b| !b.is_empty()) {
        match (merged_head, c.worktree_removed) {
            (Some(head), true) => match delete_branch(&repo, branch, head).await {
                Ok(true) => {
                    c.branch_deleted = true;
                    c.notes.push(format!("branch {branch}를 지웠다(PR이 병합됐다)"));
                }
                Ok(false) => c.notes.push(format!("branch {branch}는 이미 지워졌다")),
                Err(e) => c.notes.push(format!("branch {branch}를 남겼다: {e:#}")),
            },
            (None, _) => c.notes.push(format!("branch {branch}를 남겼다: PR이 병합되지 않았다")),
            (Some(_), false) => c.notes.push(format!("branch {branch}를 남겼다: worktree가 남아 있다")),
        }
    }
    Ok(c)
}

/// branch 끝이 병합된 PR의 head commit일 때만 지운다(herdr-projects `delete_branch`): 병합 뒤 push하지 않은 commit이
/// 있으면 남는다. `-D`인 까닭: GitHub가 병합됐다고 하고, squash 병합은 git이 보기에 병합되지 않은 branch를 남긴다.
/// `false`: 지울 branch가 없었다.
async fn delete_branch(repo: &Path, branch: &str, head: &str) -> Result<bool> {
    if head.is_empty() {
        bail!("병합된 PR의 head commit을 모른다");
    }
    let tip = git(repo, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")], GIT_T)
        .await
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if tip.is_empty() {
        return Ok(false);
    }
    if tip != head {
        bail!("병합된 PR에 없는 commit이 있다");
    }
    git(repo, &["branch", "-D", branch], GIT_T).await.map(|_| true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(harness: &str) -> Profile {
        Profile {
            harness: harness.into(),
            model: "opus".into(),
            effort: "high".into(),
            args: vec!["--no-daemon".into()],
            env: [("CODEX_SSH_SKIP_APP_SERVER_BOOT".to_string(), "true".to_string())].into(),
            command: vec![],
            description: String::new(),
        }
    }

    /// resume은 launch와 같은 model·effort·args(daemon 금지 인자 포함)·env를 쓴다(§8.2). 운영에서 빠졌던 문제를 막는다.
    #[test]
    fn resume_keeps_launch_spec() {
        for h in ["claude", "codex"] {
            let l = resolve("p", &profile(h)).unwrap();
            for flag in ["opus", "--no-daemon"] {
                assert!(l.argv.iter().any(|a| a == flag), "{h} argv {:?}", l.argv);
                assert!(l.resume_argv.iter().any(|a| a == flag), "{h} resume {:?}", l.resume_argv);
            }
            assert!(l.argv.iter().chain(&l.resume_argv).any(|a| a.contains("high")));
            assert!(l.resume_argv.iter().any(|a| a == "{session}"));
            assert_eq!(l.env, [("CODEX_SSH_SKIP_APP_SERVER_BOOT".to_string(), "true".to_string())]);
        }
        let c = resolve("p", &profile("claude")).unwrap();
        let key = c.session_key.clone().unwrap();
        assert_eq!(key.len(), 36);
        assert!(c.argv.ends_with(&["--session-id".into(), key]));
        assert_eq!(c.argv[0], "claude");
        let x = resolve("p", &profile("codex")).unwrap();
        assert_eq!(&x.resume_argv[..3], ["codex", "resume", "{session}"]);
        assert!(x.argv.contains(&"model_reasoning_effort=high".to_string()));
    }

    #[test]
    fn rebrief_writes_brief_and_new_claude_session() {
        let root = std::env::temp_dir().join(format!("aaa-rebrief-{}", std::process::id()));
        let wd = root.join(".aaa").join("p-t-0001");
        std::fs::create_dir_all(&wd).unwrap();
        let old = resolve("c", &profile("claude")).unwrap();
        let brief = format!("새 brief `{}/report.md` `{} report`", aaa_proto::BRIEF_WORK_DIR, aaa_proto::BRIEF_EXE);
        let mut cfg = ThreadsConfig { exe: "/opt/aaa".into(), ..Default::default() };
        let new = rebrief(&cfg, wd.to_str().unwrap(), &brief, old.clone(), None).unwrap();
        // 작업 폴더·실행 파일 자리는 이 머신 값으로 채운다
        assert_eq!(
            std::fs::read_to_string(wd.join("brief.md")).unwrap(),
            format!("새 brief `{}/report.md` `/opt/aaa report`", wd.display())
        );
        assert_ne!(new.session_key, old.session_key);
        let i = new.argv.iter().position(|a| a == "--session-id").unwrap();
        assert_eq!(Some(&new.argv[i + 1]), new.session_key.as_ref());
        assert_eq!(new.resume_argv, old.resume_argv);
        // --profile: 이 머신 agent.toml에서 다시 해석하고 그 harness의 hook을 붙인다(옛 launch는 쓰지 않는다)
        cfg.profiles.insert("x".into(), profile("codex"));
        let other = rebrief(&cfg, wd.to_str().unwrap(), &brief, old.clone(), Some("x")).unwrap();
        assert_eq!((other.profile.as_str(), other.harness.as_str()), ("x", "codex"));
        assert_eq!(other.argv[1], "-c", "{:?}", other.argv);
        let e = rebrief(&cfg, wd.to_str().unwrap(), &brief, old, Some("nope")).unwrap_err();
        assert!(format!("{e:#}").contains("profile `nope`가 없다"), "{e:#}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn work_dir_must_be_thread_folder() {
        assert!(check_work_dir("relative/.aaa/p-t-0001").is_err());
        assert!(check_work_dir("/tmp/x/../.aaa/p-t-0001").is_err());
        assert!(check_work_dir("/etc").is_err());
    }
}
