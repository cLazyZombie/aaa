//! 준비 소유권과 결과를 먼저 기록한다. 참고: herdr-projects 0.2.35 threads.rs의 즉시 placement 기록·재진입 clean.
//! aaa는 agent 연결이 끊겨도 작업을 끝내므로, journal은 agent에 둔다. 같은 자리의 요청은 연결을 넘어 직렬화한다.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

type OpLock = tokio::sync::Mutex<()>;
static LOCKS: LazyLock<Mutex<HashMap<PathBuf, Weak<OpLock>>>> = LazyLock::new(Default::default);
fn lock(path: &Path) -> Arc<OpLock> {
    let mut locks = LOCKS.lock().unwrap();
    locks.retain(|_, v| v.strong_count() > 0);
    if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(OpLock::new(()));
    locks.insert(path.into(), Arc::downgrade(&lock));
    lock
}

use crate::journal::{read, write};

#[derive(Serialize, Deserialize)]
struct Placement {
    #[serde(default)]
    root: Option<PathBuf>,
    repo: Option<PathBuf>,
    cwd: PathBuf,
    branch: Option<String>,
    base: Option<String>,
    head: Option<String>,
    ready: Option<ThreadReady>,
}
fn journal_key(cfg: &ThreadsConfig, project: &str, thread: &str) -> Result<PathBuf> {
    Ok(cfg
        .state_dir
        .as_ref()
        .context("준비 기록을 둘 agent 상태 폴더가 없다")?
        .join("thread-preparations")
        .join(format!("{project}-{thread}.json")))
}
fn journal(cfg: &ThreadsConfig, p: &ThreadPrepare) -> Result<PathBuf> {
    journal_key(cfg, &p.project, &p.thread)
}

/// 구판 기록은 등록한 root의 정확한 경로 형태만 가져온다. 원본은 보존한다.
pub(super) fn import_legacy(state: &Path, root: &crate::work_root::RootInfo) -> Result<usize> {
    let dir = root.path.join(".preparations");
    match dir.symlink_metadata() {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e.into()),
        Ok(m) if !m.is_dir() || m.file_type().is_symlink() => bail!("옛 준비 기록 폴더는 symlink일 수 없다"),
        Ok(_) => {}
    }
    let mut imported = 0;
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let key = path.file_stem().and_then(|n| n.to_str()).context("옛 준비 기록 이름이 올바르지 않다")?;
        let (project, number) = key.rsplit_once("-t-").context("옛 준비 기록 이름이 올바르지 않다")?;
        let thread = format!("t-{number}");
        if !valid_project(project) || !valid_thread(&thread) {
            bail!("옛 준비 기록 이름이 올바르지 않다");
        }
        let mut saved: Placement = read(&path)?.context("옛 준비 기록이 없다")?;
        let expected = match &saved.repo {
            Some(repo) => root.path.join(repo.file_name().context("옛 repo 이름이 없다")?).join(key),
            None => root.path.join(project).join(&thread),
        };
        if saved.cwd != expected || saved.root.as_ref().is_some_and(|r| r != &root.path) {
            bail!("옛 준비 기록이 등록된 root 밖을 가리킨다: {key}");
        }
        root.check(&saved.cwd)?;
        saved.root = Some(root.path.clone());
        let target = state.join("thread-preparations").join(format!("{key}.json"));
        if let Some(existing) = read::<Placement>(&target)? {
            if serde_json::to_value(&existing)? != serde_json::to_value(&saved)? {
                bail!("새 기록과 옛 준비 기록이 충돌한다: {key}");
            }
        } else {
            write(&target, &saved)?;
            imported += 1;
        }
    }
    Ok(imported)
}

fn recorded_root(cfg: &ThreadsConfig, saved: &Placement) -> Result<PathBuf> {
    let roots = cfg.known_roots()?;
    let root = roots
        .iter()
        .find(|r| saved.root.as_ref().is_some_and(|p| p == &r.path))
        .or_else(|| roots.iter().find(|r| saved.root.is_none() && saved.cwd.starts_with(&r.path)))
        .context("준비 기록의 생성 root를 알 수 없다")?;
    root.check(&saved.cwd)?;
    Ok(root.path.clone())
}

/// 준비 응답을 잃은 resolve용. 생성자와 같은 lock을 기다린 뒤 소유권이 맞는 기록만 돌려준다.
pub async fn placement(
    cfg: &ThreadsConfig,
    project: &str,
    thread: &str,
    title: &str,
    kind: ThreadKind,
    repo: Option<&str>,
) -> Result<Option<aaa_proto::ThreadPlacement>> {
    if !valid_project(project) || !valid_thread(thread) {
        bail!("프로젝트·thread 이름 형식이 아니다");
    }
    let _lease = cfg.lease()?;
    let path = journal_key(cfg, project, thread)?;
    let lock = lock(&path);
    let _guard = lock.lock().await;
    let Some(saved) = read::<Placement>(&path)? else {
        // 구판 준비에는 journal이 없다. 실제 자리를 보지 않고 없음으로 확정하면 다시 고아가 된다.
        let relative = match kind {
            ThreadKind::Worktree => {
                let repo = PathBuf::from(repo.context("worktree의 repo가 없다")?);
                let top = git(&repo, &["rev-parse", "--show-toplevel"], GIT_T).await.map(PathBuf::from).unwrap_or(repo);
                PathBuf::from(top.file_name().context("repo 이름이 없다")?).join(format!("{project}-{thread}"))
            }
            ThreadKind::Dir => PathBuf::from(project).join(thread),
            ThreadKind::Adopted => bail!("데려온 자리는 준비 기록으로 정리하지 않는다"),
        };
        if let Some(expected) = find_existing(cfg, &relative)? {
            bail!(
                "기록 없는 준비 자리가 있다: {}. `aaa thread restart {project} {thread}`로 소유권을 확인해 회수한 뒤 resolve한다",
                expected.display()
            );
        }
        return Ok(None);
    };
    let root = recorded_root(cfg, &saved)?;
    let expected = match kind {
        ThreadKind::Worktree => {
            let repo = Path::new(repo.context("worktree의 repo가 없다")?);
            let top = PathBuf::from(git(repo, &["rev-parse", "--show-toplevel"], GIT_T).await?);
            let slug = slug(title);
            let branch = format!("aaa/{project}/{thread}{}", if slug.is_empty() { slug } else { format!("-{slug}") });
            if saved.repo.as_ref() != Some(&top) || saved.branch.as_ref() != Some(&branch) {
                bail!("준비 journal과 repo·branch가 다르다. 사람이 확인해야 한다");
            }
            root.join(top.file_name().context("repo 이름이 없다")?).join(format!("{project}-{thread}"))
        }
        ThreadKind::Dir if saved.repo.is_none() && saved.branch.is_none() => root.join(project).join(thread),
        _ => bail!("준비 journal과 thread 종류가 다르다"),
    };
    if saved.cwd != expected {
        bail!("준비 journal의 자리가 다르다");
    }
    Ok(Some(aaa_proto::ThreadPlacement {
        cwd: saved.cwd.to_string_lossy().into(),
        work_dir: saved.cwd.join(".aaa").join(format!("{project}-{thread}")).to_string_lossy().into(),
        branch: saved.branch,
        base: saved.base,
    }))
}

pub(super) async fn prepare(cfg: &ThreadsConfig, p: ThreadPrepare, wait: Duration) -> Result<ThreadReady> {
    if !valid_project(&p.project) || !valid_thread(&p.thread) {
        bail!("프로젝트·thread 이름 형식이 아니다");
    }
    let _lease = cfg.lease()?;
    let path = journal(cfg, &p)?;
    let lock = lock(&path);
    let _guard = lock.lock().await;
    let ready = super::prepare_inner(cfg, p, wait).await?;
    let mut placement: Placement = read(&path)?.context("준비 journal이 없다")?;
    placement.ready = Some(ready.clone());
    write(&path, &placement)?;
    Ok(ready)
}

pub(super) async fn place(
    cfg: &ThreadsConfig,
    p: &ThreadPrepare,
    wait: Duration,
) -> Result<(PathBuf, Option<String>, Option<String>)> {
    let path = journal(cfg, p)?;
    let mut old: Option<Placement> = read(&path)?;
    let recorded = old.as_ref().map(|saved| recorded_root(cfg, saved)).transpose()?;
    let placement = match p.kind {
        ThreadKind::Worktree => {
            let repo = PathBuf::from(p.repo.as_deref().context("worktree thread에는 repo 경로가 필요하다")?);
            let top = PathBuf::from(git(&repo, &["rev-parse", "--show-toplevel"], GIT_T).await?);
            let s = slug(&p.title);
            let branch = format!("aaa/{}/{}{}", p.project, p.thread, if s.is_empty() { s } else { format!("-{s}") });
            let name = top.file_name().context("repo 이름이 없다")?;
            let relative = PathBuf::from(name).join(format!("{}-{}", p.project, p.thread));
            let cwd = match &recorded {
                Some(root) => root.join(&relative),
                None => find_existing(cfg, &relative)?.unwrap_or_else(|| cfg.work_root.join(&relative)),
            };
            cfg.check_path(&cwd)?;
            if let Some(old) = &old {
                if old.repo.as_ref() != Some(&top) || old.cwd != cwd || old.branch.as_ref() != Some(&branch) {
                    bail!("준비 journal과 요청의 repo·자리·branch가 다르다. 사람이 확인해야 한다");
                }
                if p.base.as_ref().is_some_and(|b| old.base.as_ref() != Some(b)) {
                    bail!("준비 journal과 기준 ref가 다르다");
                }
            }
            let placement = match old.take() {
                Some(old) => old,
                None => {
                    let has_origin = git(&top, &["remote"], GIT_T).await?.lines().any(|l| l == "origin");
                    if has_origin && let Err(e) = git(&top, &["fetch", "--quiet", "origin"], wait).await {
                        tracing::warn!("fetch 실패(로컬 ref로 진행): {e:#}");
                    }
                    let base = match &p.base {
                        Some(b) => b.clone(),
                        None => match git(&top, &["rev-parse", "--abbrev-ref", "origin/HEAD"], GIT_T).await {
                            Ok(b) if has_origin && b != "origin/HEAD" => b,
                            _ => git(&top, &["rev-parse", "--abbrev-ref", "HEAD"], GIT_T).await?,
                        },
                    };
                    let head = git(&top, &["rev-parse", "--verify", &format!("{base}^{{commit}}")], GIT_T).await?;
                    let exists =
                        git(&top, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")], GIT_T)
                            .await
                            .is_ok();
                    // 옛 agent가 남긴 완성된 자리도 정확한 경로·branch·HEAD·깨끗함이 모두 맞을 때만 회수한다.
                    if exists && !cwd.exists() || !exists && cwd.exists() {
                        bail!(
                            "{}: 기록된 worktree가 없는데 branch `{branch}` 또는 자리가 이미 있다. 사람이 확인한다: `aaa thread resolve`",
                            p.thread
                        );
                    }
                    let placement = Placement {
                        root: Some(cfg.root_for_path(&cwd)?.context("생성 root가 없다")?.path),
                        repo: Some(top.clone()),
                        cwd,
                        branch: Some(branch.clone()),
                        base: Some(base),
                        head: Some(head),
                        ready: None,
                    };
                    if exists {
                        check_worktree(&placement).await?;
                    }
                    write(&path, &placement)?;
                    placement
                }
            };
            exclude_work_dirs(&top).await?;
            let branch_exists =
                git(&top, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")], GIT_T).await.is_ok();
            if placement.cwd.exists() {
                check_worktree(&placement).await?;
            } else {
                std::fs::create_dir_all(placement.cwd.parent().unwrap())?;
                let cwd = placement.cwd.to_string_lossy();
                let head = placement.head.as_deref().context("준비 기준 commit이 없다")?;
                let reason = cfg.lock_reason(&placement.cwd)?;
                let mut args = vec!["worktree", "add"];
                if let Some(reason) = &reason {
                    args.extend(["--lock", "--reason", reason]);
                }
                if branch_exists {
                    let tip = git(&top, &["rev-parse", &format!("refs/heads/{branch}")], GIT_T).await?;
                    if tip != head {
                        bail!("준비 branch에 새 commit이 있다. 사람이 확인해야 한다");
                    }
                    args.extend([cwd.as_ref(), branch.as_str()]);
                } else {
                    args.extend(["-b", &branch, &cwd, head]);
                }
                git(&top, &args, wait).await?;
            }
            protect(cfg, &top, &placement.cwd).await?;
            placement
        }
        ThreadKind::Dir => {
            let relative = PathBuf::from(&p.project).join(&p.thread);
            let cwd = recorded.as_ref().unwrap_or(&cfg.work_root).join(&relative);
            if old.is_none()
                && let Some(existing) = find_existing(cfg, &relative)?
            {
                bail!("작업 폴더가 이미 있다: {}", existing.display());
            }
            cfg.check_path(&cwd)?;
            let placement = match old {
                Some(old) if old.repo.is_none() && old.cwd == cwd => old,
                Some(_) => bail!("준비 journal과 작업 폴더가 다르다"),
                None => {
                    if cwd.exists() {
                        bail!("작업 폴더가 이미 있다: {}", cwd.display());
                    }
                    let placement = Placement {
                        root: Some(cfg.work_root.clone()),
                        repo: None,
                        cwd,
                        branch: None,
                        base: None,
                        head: None,
                        ready: None,
                    };
                    write(&path, &placement)?;
                    placement
                }
            };
            std::fs::create_dir_all(&placement.cwd)?;
            placement
        }
        ThreadKind::Adopted => bail!("데려온 thread는 자리를 만들지 않는다(`thread adopt`)"),
    };
    Ok((placement.cwd, placement.branch, placement.base))
}

/// 어느 옛 root도 읽지 못하면 '없음'으로 확정하지 않는다. 서로 다른 root의 중복도 보존한다.
fn find_existing(cfg: &ThreadsConfig, relative: &Path) -> Result<Option<PathBuf>> {
    let mut found = None;
    for root in cfg.known_roots()? {
        let cwd = root.path.join(relative);
        root.check(&cwd)?;
        match cwd.symlink_metadata() {
            Ok(_) if found.is_some() => bail!("여러 작업 root에 같은 thread 자리가 있다. 사람이 확인해야 한다"),
            Ok(_) => found = Some(cwd),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(found)
}

pub(super) async fn worktree_lock(repo: &Path, cwd: &Path) -> Result<Option<Option<String>>> {
    let expected = crate::work_root::real_location(cwd)?;
    let list = git(repo, &["worktree", "list", "--porcelain", "-z"], GIT_T).await?;
    for record in list.split("\0\0") {
        let mut fields = record.split('\0');
        if fields
            .next()
            .and_then(|s| s.strip_prefix("worktree "))
            .map(|p| crate::work_root::real_location(Path::new(p)))
            .transpose()?
            .is_some_and(|p| p == expected)
        {
            return Ok(Some(fields.find_map(|s| {
                if s == "locked" { Some(String::new()) } else { s.strip_prefix("locked ").map(Into::into) }
            })));
        }
    }
    Ok(None)
}
async fn protect(cfg: &ThreadsConfig, repo: &Path, cwd: &Path) -> Result<()> {
    if let Some(reason) = cfg.lock_reason(cwd)? {
        match worktree_lock(repo, cwd).await? {
            Some(None) => {
                git(repo, &["worktree", "lock", "--reason", &reason, &cwd.to_string_lossy()], GIT_T).await?;
            }
            Some(Some(_)) => {} // 사람의 잠금도 자동 prune을 막는다. 정리 때는 소유 잠금만 푼다.
            None => bail!("준비한 worktree의 Git 등록이 없다"),
        }
    }
    Ok(())
}

async fn check_worktree(p: &Placement) -> Result<()> {
    if p.cwd.symlink_metadata()?.file_type().is_symlink() {
        bail!("준비 자리가 symlink다");
    }
    let top = git(&p.cwd, &["rev-parse", "--show-toplevel"], GIT_T).await?;
    let branch = git(&p.cwd, &["symbolic-ref", "--short", "HEAD"], GIT_T).await?;
    let head = git(&p.cwd, &["rev-parse", "HEAD"], GIT_T).await?;
    let common = git(&p.cwd, &["rev-parse", "--path-format=absolute", "--git-common-dir"], GIT_T).await?;
    let repo_common =
        git(p.repo.as_ref().unwrap(), &["rev-parse", "--path-format=absolute", "--git-common-dir"], GIT_T).await?;
    if Path::new(&top).canonicalize()? != p.cwd.canonicalize()?
        || common != repo_common
        || p.branch.as_ref() != Some(&branch)
        || p.head.as_ref() != Some(&head)
        || !git(&p.cwd, &["status", "--porcelain", "--untracked-files=all"], GIT_T).await?.is_empty()
    {
        bail!("준비 자리의 repo·branch·HEAD가 다르거나 변경이 있다. 사람이 확인해야 한다: {}", p.cwd.display());
    }
    Ok(())
}

/// 구판에서 가져온 외장 worktree도 agent가 다시 연결되기 전에 자동 prune으로부터 보호한다.
pub async fn protect_recorded_worktrees(cfg: &ThreadsConfig) -> Result<()> {
    let _lease = cfg.lease()?;
    protect_recorded_worktrees_locked(cfg).await
}

pub(crate) async fn protect_recorded_worktrees_locked(cfg: &ThreadsConfig) -> Result<()> {
    let dir = cfg.state_dir.as_ref().context("agent 상태 폴더가 없다")?.join("thread-preparations");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let saved: Placement = read(&path)?.context("준비 journal이 없다")?;
        let Some(repo) = &saved.repo else { continue };
        if cfg.lock_reason(&saved.cwd)?.is_none() {
            continue;
        }
        if let Err(e) = recorded_root(cfg, &saved) {
            tracing::warn!(%e, "옛 작업 root의 Git 잠금 점검 보류");
            continue;
        }
        if !saved.cwd.is_dir() {
            continue;
        }
        // 실행 중 작업의 새 commit·dirty는 허용하되 다른 repo/branch를 차지하지 않는다.
        let common = git(&saved.cwd, &["rev-parse", "--path-format=absolute", "--git-common-dir"], GIT_T).await?;
        if common != git(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"], GIT_T).await?
            || saved.branch.as_ref() != Some(&git(&saved.cwd, &["symbolic-ref", "--short", "HEAD"], GIT_T).await?)
        {
            bail!("준비 journal과 현재 worktree의 repo·branch가 다르다");
        }
        protect(cfg, repo, &saved.cwd).await?;
    }
    Ok(())
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct CleanupInput {
    repo: Option<String>,
    cwd: String,
    branch: Option<String>,
    merged_head: Option<String>,
    keep_worktree: bool,
    uncopied: bool,
}
#[derive(Serialize, Deserialize)]
struct CleanupRecord {
    input: CleanupInput,
    result: Option<aaa_proto::Cleanup>,
    /// 이 의도를 먼저 내구화한 뒤 aaa 소유의 Git 잠금을 잠깐 푼다.
    #[serde(default)]
    relock: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub async fn cleanup_recorded(
    state: &Path,
    repo: Option<&str>,
    cwd: &str,
    branch: Option<&str>,
    merged_head: Option<&str>,
    keep_worktree: bool,
    uncopied: bool,
    wait: Duration,
) -> Result<Cleanup> {
    let _lease = crate::work_root::shared(state)?;
    let cfg = ThreadsConfig { state_dir: Some(state.into()), ..Default::default() };
    cfg.check_path(Path::new(cwd))?;
    let owned_lock = cfg.lock_reason(Path::new(cwd))?;
    let path = state.join("thread-cleanup").join(format!("{}.json", hex::encode(Sha256::digest(cwd.as_bytes()))));
    let lock = lock(&path);
    let _guard = lock.lock().await;
    let input = CleanupInput {
        repo: repo.map(Into::into),
        cwd: cwd.into(),
        branch: branch.map(Into::into),
        merged_head: merged_head.map(Into::into),
        keep_worktree,
        uncopied,
    };
    // 완료 영수증이 있어도 현재 filesystem을 다시 본다. 사람이 다시 만든 자리·branch를 옛 완료로 덮지 않는다.
    if let Some(old) = read::<CleanupRecord>(&path)?
        && old.input == input
        && let Some(result) = old.result
        && result.worktree_removed
        && !Path::new(cwd).exists()
        && result.branch_deleted
        && let (Some(repo), Some(branch)) = (repo, branch)
        && git(Path::new(repo), &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")], GIT_T)
            .await
            .is_err()
    {
        return Ok(result);
    }
    if let Some(mut old) = read::<CleanupRecord>(&path)? {
        restore_lock(state, &mut old).await?;
    }
    write(&path, &CleanupRecord { input, result: None, relock: owned_lock.clone() })?;
    let result =
        super::cleanup(repo, cwd, branch, merged_head, keep_worktree, uncopied, owned_lock.as_deref(), wait).await?;
    let mut record: CleanupRecord = read(&path)?.context("정리 journal이 없다")?;
    restore_lock(state, &mut record).await?;
    record.result = Some(result.clone());
    write(&path, &record)?;
    Ok(result)
}

async fn restore_lock(state: &Path, record: &mut CleanupRecord) -> Result<()> {
    let Some(reason) = &record.relock else { return Ok(()) };
    let cfg = ThreadsConfig { state_dir: Some(state.into()), ..Default::default() };
    let cwd = Path::new(&record.input.cwd);
    cfg.check_path(cwd)?;
    if cfg.lock_reason(cwd)?.as_ref() != Some(reason) {
        bail!("정리 journal의 잠금 소유자가 다르다");
    }
    if let Some(repo) = &record.input.repo
        && worktree_lock(Path::new(repo), cwd).await? == Some(None)
    {
        git(Path::new(repo), &["worktree", "lock", "--reason", reason, &record.input.cwd], GIT_T).await?;
    }
    record.relock = None;
    Ok(())
}

/// 중단된 정리는 다시 실행하지 않는다. Git 잠금만 되살려 다음 resolve/sweep을 기다린다.
pub async fn recover_cleanup_locks(state: &Path) -> Result<()> {
    let _lease = crate::work_root::shared(state)?;
    let dir = state.join("thread-cleanup");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let mut record: CleanupRecord = read(&path)?.context("정리 journal이 없다")?;
        if record.relock.is_none() {
            continue;
        }
        match restore_lock(state, &mut record).await {
            Ok(()) => write(&path, &record)?,
            // 옛 볼륨이 빠졌다면 그 자리의 정리를 완료로 바꾸지 않는다.
            Err(e) => tracing::warn!(%e, "중단한 정리의 Git 잠금 복구 보류"),
        }
    }
    Ok(())
}
