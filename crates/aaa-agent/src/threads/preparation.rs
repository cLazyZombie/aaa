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

fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().context("journal 부모가 없다")?;
    std::fs::create_dir_all(dir)?;
    if dir.symlink_metadata()?.file_type().is_symlink() {
        bail!("journal 폴더는 symlink일 수 없다");
    }
    aaa_common::util::ensure_private_dir(dir)?;
    let tmp = dir.join(format!(".{}.tmp", aaa_common::util::new_id()));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp)?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.sync_all()?;
    drop(file);
    // Windows에서도 기존 journal을 원자적으로 교체한다.
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let m = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !m.is_file() || m.len() > 4 << 20 {
        bail!("journal은 4MiB 이하 일반 파일이어야 한다");
    }
    Ok(Some(serde_json::from_slice(&std::fs::read(path)?)?))
}

#[derive(Serialize, Deserialize)]
struct Placement {
    repo: Option<PathBuf>,
    cwd: PathBuf,
    branch: Option<String>,
    base: Option<String>,
    head: Option<String>,
    ready: Option<ThreadReady>,
}
fn journal_key(cfg: &ThreadsConfig, project: &str, thread: &str) -> PathBuf {
    cfg.work_root.join(".preparations").join(format!("{project}-{thread}.json"))
}
fn journal(cfg: &ThreadsConfig, p: &ThreadPrepare) -> PathBuf {
    journal_key(cfg, &p.project, &p.thread)
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
    let path = journal_key(cfg, project, thread);
    let lock = lock(&path);
    let _guard = lock.lock().await;
    let Some(saved) = read::<Placement>(&path)? else {
        // 구판 준비에는 journal이 없다. 실제 자리를 보지 않고 없음으로 확정하면 다시 고아가 된다.
        let expected = match kind {
            ThreadKind::Worktree => {
                let repo = PathBuf::from(repo.context("worktree의 repo가 없다")?);
                let top = git(&repo, &["rev-parse", "--show-toplevel"], GIT_T).await.map(PathBuf::from).unwrap_or(repo);
                cfg.work_root.join(top.file_name().context("repo 이름이 없다")?).join(format!("{project}-{thread}"))
            }
            ThreadKind::Dir => cfg.work_root.join(project).join(thread),
            ThreadKind::Adopted => bail!("데려온 자리는 준비 기록으로 정리하지 않는다"),
        };
        match expected.symlink_metadata() {
            Ok(_) => bail!(
                "기록 없는 준비 자리가 있다: {}. `aaa thread restart {project} {thread}`로 소유권을 확인해 회수한 뒤 resolve한다",
                expected.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        }
    };
    let expected = match kind {
        ThreadKind::Worktree => {
            let repo = Path::new(repo.context("worktree의 repo가 없다")?);
            let top = PathBuf::from(git(repo, &["rev-parse", "--show-toplevel"], GIT_T).await?);
            let slug = slug(title);
            let branch = format!("aaa/{project}/{thread}{}", if slug.is_empty() { slug } else { format!("-{slug}") });
            if saved.repo.as_ref() != Some(&top) || saved.branch.as_ref() != Some(&branch) {
                bail!("준비 journal과 repo·branch가 다르다. 사람이 확인해야 한다");
            }
            cfg.work_root.join(top.file_name().context("repo 이름이 없다")?).join(format!("{project}-{thread}"))
        }
        ThreadKind::Dir if saved.repo.is_none() && saved.branch.is_none() => cfg.work_root.join(project).join(thread),
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
    let path = journal(cfg, &p);
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
    let path = journal(cfg, p);
    let mut old: Option<Placement> = read(&path)?;
    let placement = match p.kind {
        ThreadKind::Worktree => {
            let repo = PathBuf::from(p.repo.as_deref().context("worktree thread에는 repo 경로가 필요하다")?);
            let top = PathBuf::from(git(&repo, &["rev-parse", "--show-toplevel"], GIT_T).await?);
            let s = slug(&p.title);
            let branch = format!("aaa/{}/{}{}", p.project, p.thread, if s.is_empty() { s } else { format!("-{s}") });
            let name = top.file_name().context("repo 이름이 없다")?;
            let cwd = cfg.work_root.join(name).join(format!("{}-{}", p.project, p.thread));
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
                if branch_exists {
                    let tip = git(&top, &["rev-parse", &format!("refs/heads/{branch}")], GIT_T).await?;
                    if tip != head {
                        bail!("준비 branch에 새 commit이 있다. 사람이 확인해야 한다");
                    }
                    git(&top, &["worktree", "add", &cwd, &branch], wait).await?;
                } else {
                    git(&top, &["worktree", "add", "-b", &branch, &cwd, head], wait).await?;
                }
            }
            placement
        }
        ThreadKind::Dir => {
            let cwd = cfg.work_root.join(&p.project).join(&p.thread);
            let placement = match old {
                Some(old) if old.repo.is_none() && old.cwd == cwd => old,
                Some(_) => bail!("준비 journal과 작업 폴더가 다르다"),
                None => {
                    if cwd.exists() {
                        bail!("작업 폴더가 이미 있다: {}", cwd.display());
                    }
                    let placement = Placement { repo: None, cwd, branch: None, base: None, head: None, ready: None };
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
    write(&path, &CleanupRecord { input, result: None })?;
    let result = super::cleanup(repo, cwd, branch, merged_head, keep_worktree, uncopied, wait).await?;
    let mut record: CleanupRecord = read(&path)?.context("정리 journal이 없다")?;
    record.result = Some(result.clone());
    write(&path, &record)?;
    Ok(result)
}
