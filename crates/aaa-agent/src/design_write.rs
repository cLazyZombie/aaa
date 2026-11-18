//! 기획 branch 쓰기(design_write.v1, t-1894 2단계). 기획서는 main에 바로 쓰지 않고 기획 branch `design/<slug>`에서 고친다
//! (사용자 결정 11-16). 그 branch를 꺼낸 전용 worktree는 이 모듈만 만들고 쓴다: thread worktree와 달리 대화 thread가 바뀌어도
//! 남고(대화 하나가 thread 여럿을 거칠 수 있다), 자리는 `<작업 root>/<저장소 이름>/<프로젝트>-design-<slug>`다. 그 branch가 다른
//! 자리(main checkout·thread worktree·사람의 worktree, 다른 프로젝트)에 꺼내져 있으면 쓰지도 내리지도 않는다.
//!
//! - 쓰기는 이 모듈이 쓴 그 파일만 `git add -- <경로>`로 commit한다. HEAD가 그 branch가 아니거나 다른 변경이 있으면 거부하고,
//!   쓰다 실패하면 그 파일을 되돌린다. 저장소 hook(`core.hooksPath`를 없는 폴더로)·서명은 돌리지 않는다(사람 없는 자리에서 창을
//!   띄우거나 멈추지 않게, 검사는 착지가 한다). 작성자는 그 머신 git 설정이다.
//! - origin에는 `refs/heads/design/<slug>`만 force 없이 올린다. 지울 때는 host가 올린 그 판일 때만 지운다(lease).
//! - 내릴 때(쉼·버리기) worktree는 force 없이 지운다: 올리지 않은 변경이 있으면 실패해 아무것도 잃지 않는다.
use aaa_proto::{
    AgentResponse, DESIGN_MAX_FILE, DESIGN_MAX_TEXT, ErrorBody, ErrorCode, design_branch, design_clean_path,
    design_inside, design_slug_ok, design_write_path_ok, valid_project,
};
use anyhow::{Context, Result, anyhow, bail};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::design::{Missing, Stale, base, entries, fetch, git, has_origin, reply, toplevel};
use crate::threads::ThreadsConfig;

/// worktree 만들기·commit·지우기 상한
pub(crate) const GIT_WAIT: Duration = Duration::from_secs(60);
/// origin 올리기·지우기 상한
pub(crate) const PUSH_WAIT: Duration = Duration::from_secs(60);
/// 올리기 전 저장소 기획 검사 상한(host는 올리기 요청을 이것과 PUSH_WAIT보다 길게 기다린다)
pub(crate) const CHECK_WAIT: Duration = Duration::from_secs(300);

fn checked(project: &str, slug: &str) -> Result<String> {
    if !valid_project(project) {
        bail!("프로젝트 이름 형식이 아니다");
    }
    if !design_slug_ok(slug) {
        bail!("기획 branch 이름은 영문 소문자·숫자·`-`다(1~48자)");
    }
    Ok(design_branch(slug))
}

/// 저장소 hook을 끄는 자리: 이 agent 실행 파일(폴더가 아닌 파일이라 그 아래에 hook이 있을 수 없고, 남이 바꿀 수 없다). OS마다
/// 같은 길이다
fn no_hooks() -> Result<String> {
    Ok(std::env::current_exe().context("agent 실행 파일 경로를 모른다")?.to_string_lossy().into_owned())
}

/// stderr까지 받아 실패 까닭을 오류에 남기는 git(worktree·commit). origin 주소가 섞일 수 있는 push에는 쓰지 않는다
pub(crate) async fn git_told(dir: &Path, args: &[&str], wait: Duration) -> Result<String> {
    use aaa_common::child_process::TokioCommandExt as _;
    let hooks = format!("core.hooksPath={}", no_hooks()?);
    let out = tokio::time::timeout(
        wait,
        tokio::process::Command::new("git")
            .args(aaa_common::diff::git_prefix())
            .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false", "-c", "commit.gpgsign=false", "-c", &hooks])
            .args(args)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GCM_INTERACTIVE", "never")
            .env("LC_ALL", "C")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output_guarded(),
    )
    .await
    .map_err(|_| anyhow!("git {} 시간 상한이다", args[0]))?
    .context("git 실행 실패")?;
    if !out.status.success() {
        bail!("git {} 실패: {}", args[0], String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// 그 branch를 꺼낸 worktree 자리(`git worktree list`)
async fn worktree_of(top: &Path, branch: &str) -> Result<Option<(PathBuf, bool)>> {
    let list = git(top, &["worktree", "list", "--porcelain", "-z"], 1 << 20).await?;
    let want = format!("branch refs/heads/{branch}");
    for record in list.split(|b| *b == 0).collect::<Vec<_>>().split(|f| f.is_empty()) {
        let fields: Vec<String> = record.iter().map(|f| String::from_utf8_lossy(f).into_owned()).collect();
        if fields.contains(&want) {
            let path = fields.iter().find_map(|f| f.strip_prefix("worktree ")).context("worktree 경로가 없다")?;
            let locked = fields.iter().any(|f| f == "locked" || f.starts_with("locked "));
            return Ok(Some((PathBuf::from(path), locked)));
        }
    }
    Ok(None)
}

/// 이 모듈의 자리(작업 root마다 `<저장소 이름>/<프로젝트>-design-<slug>`)인가
fn ours(cfg: &ThreadsConfig, top: &Path, project: &str, slug: &str, path: &Path) -> Result<bool> {
    let name = top.file_name().context("저장소 이름이 없다")?;
    let found = crate::work_root::real_location(path)?;
    for root in cfg.known_roots()? {
        let place = root.path.join(name).join(format!("{project}-design-{slug}"));
        // 디스크가 빠진 작업 root는 이 자리일 수 없다(그 root 때문에 다른 root의 자리까지 막지 않는다)
        if crate::work_root::real_location(&place).is_ok_and(|p| p == found) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 이 branch를 꺼낸 이 모듈의 worktree. 다른 자리에 꺼내져 있으면 오류, 폴더가 사라진 기록은 정리하고 없음이다
async fn own_worktree(cfg: &ThreadsConfig, top: &Path, project: &str, slug: &str) -> Result<Option<(PathBuf, bool)>> {
    let branch = design_branch(slug);
    let Some((path, locked)) = worktree_of(top, &branch).await? else { return Ok(None) };
    if !ours(cfg, top, project, slug, &path)? {
        bail!("{branch}가 기획 자리가 아닌 곳에 꺼내져 있다: {}(사람이 확인한다)", path.display());
    }
    if path.symlink_metadata().is_err() {
        // 잠긴 자리(휴대용 작업 root)는 디스크가 빠졌을 수 있다: 건드리지 않는다
        if locked {
            bail!("{branch}의 기획 자리가 지금 없다(작업 디스크가 붙어 있는지 본다): {}", path.display());
        }
        // 폴더를 지운 자리: 그 기록 하나만 지운다(branch는 그대로)
        git_told(top, &["worktree", "remove", &path.to_string_lossy()], GIT_WAIT).await?;
        return Ok(None);
    }
    Ok(Some((path, locked)))
}

pub(crate) async fn has_ref(top: &Path, full: &str) -> bool {
    git(top, &["show-ref", "--verify", "--quiet", full], 4096).await.is_ok()
}

pub(crate) async fn rev(top: &Path, full: &str) -> Result<String> {
    Ok(String::from_utf8_lossy(&git(top, &["rev-parse", "--verify", "--end-of-options", full], 4096).await?)
        .trim()
        .to_string())
}

/// 기획 branch worktree를 연다(있으면 그대로, branch만 있으면 다시 꺼내고, 없으면 origin 기본 branch에서 만든다)
pub async fn open(cfg: &ThreadsConfig, project: &str, repo: &str, slug: &str) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let branch = checked(project, slug)?;
            let _lease = cfg.lease().context("작업 root 잠금")?;
            let top = toplevel(repo).await?;
            if let Err(e) = fetch(&top).await {
                tracing::warn!(repo, "기획 branch를 열기 전에 origin을 받지 못했다(가진 ref로 연다): {e:#}");
            }
            let (base_name, main) = base(&top).await?;
            let (cwd, opened) = match own_worktree(cfg, &top, project, slug).await? {
                Some((cwd, _)) => (cwd, "open"),
                None => {
                    let name = top.file_name().context("저장소 이름이 없다")?;
                    let cwd = cfg.work_root.join(name).join(format!("{project}-design-{slug}"));
                    cfg.check_path(&cwd)?;
                    if cwd.symlink_metadata().is_ok() {
                        bail!("기획 branch 자리가 이미 있다(다른 것이 차지했다): {}", cwd.display());
                    }
                    std::fs::create_dir_all(cwd.parent().context("자리의 부모가 없다")?)?;
                    let at = cwd.to_string_lossy().into_owned();
                    let reason = cfg.lock_reason(&cwd)?;
                    let mut args = vec!["worktree", "add", "--quiet"];
                    if let Some(reason) = &reason {
                        args.extend(["--lock", "--reason", reason]);
                    }
                    let (local, remote) = (format!("refs/heads/{branch}"), format!("refs/remotes/origin/{branch}"));
                    let opened = match (has_ref(&top, &local).await, has_ref(&top, &remote).await) {
                        (true, has_remote) => {
                            // 로컬이 origin보다 뒤에만 있으면(다른 머신에서 더 썼다) 따라간다. 갈라졌으면 그대로 두고 올릴 때 알린다
                            if has_remote {
                                let (mine, theirs) = (rev(&top, &local).await?, rev(&top, &remote).await?);
                                if mine != theirs
                                    && git(&top, &["merge-base", "--is-ancestor", &mine, &theirs], 64).await.is_ok()
                                {
                                    git_told(&top, &["branch", "--force", "--no-track", &branch, &remote], GIT_WAIT)
                                        .await?;
                                }
                            }
                            args.extend([at.as_str(), branch.as_str()]);
                            "local"
                        }
                        (false, true) => {
                            args.extend(["--no-track", "-b", &branch, &at, &remote]);
                            "remote"
                        }
                        (false, false) => {
                            args.extend(["--no-track", "-b", &branch, &at, &main]);
                            "created"
                        }
                    };
                    if let Err(e) = git_told(&top, &args, GIT_WAIT).await {
                        // 방금 만들다 만 자리만 치운다(사람 글이 없다). 새로 만든 branch는 main 끝 그대로라 지워도 잃는 것이 없다
                        if let Ok(Some((made, locked))) = worktree_of(&top, &branch).await
                            && ours(cfg, &top, project, slug, &made).unwrap_or(false)
                        {
                            let made = made.to_string_lossy().into_owned();
                            if locked {
                                aaa_common::diagnostic::ignored(
                                    git_told(&top, &["worktree", "unlock", &made], GIT_WAIT).await,
                                    "만들다 만 기획 worktree 잠금 풀기",
                                );
                            }
                            aaa_common::diagnostic::ignored(
                                git_told(&top, &["worktree", "remove", "--force", &made], GIT_WAIT).await,
                                "만들다 만 기획 worktree 치우기",
                            );
                        }
                        if opened == "created" {
                            aaa_common::diagnostic::ignored(
                                git_told(&top, &["branch", "-D", &branch], GIT_WAIT).await,
                                "만들다 만 기획 branch 치우기",
                            );
                        }
                        return Err(e.context("기획 worktree를 만들지 못했다"));
                    }
                    // git이 아는 경로(실제 경로)로 돌려준다: 다시 열 때와 같은 글이다
                    let made =
                        worktree_of(&top, &branch).await?.map(|(cwd, _)| cwd).context("만든 worktree가 목록에 없다")?;
                    (made, opened)
                }
            };
            let head = git_told(&cwd, &["rev-parse", "HEAD"], GIT_WAIT).await?;
            Ok(AgentResponse::DesignBranch {
                cwd: cwd.to_string_lossy().into_owned(),
                head,
                base: base_name,
                opened: opened.to_string(),
            })
        }
        .await,
    )
}

/// worktree 안 경로의 부모 폴더가 symlink면 거부한다(worktree 밖에 쓰지 않게)
pub(crate) fn plain_parents(cwd: &Path, path: &str) -> Result<()> {
    let mut at = cwd.to_path_buf();
    let parts: Vec<&str> = path.split('/').collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        at.push(part);
        match at.symlink_metadata() {
            Ok(m) if m.file_type().is_symlink() => bail!("기획 폴더 안 경로에 symlink가 있다: {part}"),
            Ok(m) if !m.is_dir() => bail!("기획 폴더 안 경로가 폴더가 아니다: {part}"),
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        }
    }
    let target = cwd.join(path);
    if target.symlink_metadata().is_ok_and(|m| !m.is_file()) {
        bail!("일반 파일이 아니다: {path}");
    }
    Ok(())
}

/// 대소문자만 다른 파일·폴더가 이미 있으면 그 이름(macOS·Windows에서는 같은 파일이다)
pub(crate) fn case_twin<'a>(names: impl Iterator<Item = &'a str>, path: &str) -> Option<String> {
    let want: Vec<&str> = path.split('/').collect();
    for name in names {
        for (have, want) in name.split('/').zip(&want) {
            if have == *want {
                continue;
            }
            if have.to_lowercase() == want.to_lowercase() {
                return Some(name.to_string());
            }
            break;
        }
    }
    None
}

pub(crate) fn write_path(folder: &str, path: &str) -> Result<(String, String)> {
    let (folder, path) = crate::design::inside(folder, path)?;
    if !design_write_path_ok(&path) {
        bail!(
            "어느 OS에서나 같은 파일로 쓸 수 있는 이름이 아니다(`.git`, 끝의 `.`·빈칸, `:` 등, `CON` 같은 예약 이름): {path}"
        );
    }
    Ok((folder, path))
}

/// 저장과 삭제는 해당 branch를 꺼낸 깨끗한 기획 worktree에서만 한다.
async fn write_worktree(cfg: &ThreadsConfig, repo: &str, project: &str, slug: &str, branch: &str) -> Result<PathBuf> {
    let top = toplevel(repo).await?;
    let (cwd, _) = own_worktree(cfg, &top, project, slug)
        .await?
        .ok_or_else(|| Missing(format!("{branch}의 worktree가 없다(쉬는 branch면 먼저 연다)")))?;
    let head = git_told(&cwd, &["symbolic-ref", "-q", "HEAD"], GIT_WAIT).await?;
    if head != format!("refs/heads/{branch}") {
        bail!("기획 worktree가 {branch}에 있지 않다({head})");
    }
    let dirty = git_told(&cwd, &["status", "--porcelain", "--untracked-files=all"], GIT_WAIT).await?;
    if !dirty.is_empty() {
        bail!("기획 worktree에 commit하지 않은 변경이 있다: 사람이 확인한다({})", cwd.display());
    }
    Ok(cwd)
}

/// 기획 branch에 문서 하나를 쓰고 그 파일만 commit한다
#[allow(clippy::too_many_arguments)]
pub async fn save(
    cfg: &ThreadsConfig,
    project: &str,
    repo: &str,
    folder: &str,
    slug: &str,
    path: &str,
    base_blob: Option<&str>,
    (text, message): (&str, &str),
) -> Result<AgentResponse, ErrorBody> {
    save_bytes(cfg, project, repo, folder, slug, path, base_blob, (text.as_bytes(), DESIGN_MAX_TEXT), message).await
}

/// 파일 하나(글이나 그림)를 쓰고 그 파일만 commit한다. `max`는 크기 상한
#[allow(clippy::too_many_arguments)]
pub async fn save_bytes(
    cfg: &ThreadsConfig,
    project: &str,
    repo: &str,
    folder: &str,
    slug: &str,
    path: &str,
    base_blob: Option<&str>,
    (text, max): (&[u8], u64),
    message: &str,
) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let branch = checked(project, slug)?;
            let (folder, path) = write_path(folder, path)?;
            if text.len() as u64 > max {
                bail!("문서가 너무 크다({}바이트, 상한 {max})", text.len());
            }
            if message.trim().is_empty() {
                bail!("commit 메시지가 비었다");
            }
            let cwd = write_worktree(cfg, repo, project, slug, &branch).await?;
            let listed =
                git(&cwd, &["ls-tree", "-r", "-z", "-l", "--full-tree", "HEAD", "--", &folder], 8 << 20).await?;
            let listed = entries(&listed)?;
            let now = match listed.iter().find(|e| e.path == path) {
                Some(e) if e.regular() => Some(e.sha.clone()),
                Some(_) => bail!("일반 파일이 아니다: {path}"),
                None => None,
            };
            if now.is_none()
                && let Some(twin) = case_twin(listed.iter().map(|e| e.path.as_str()), &path)
            {
                bail!("대소문자만 다른 문서가 이미 있다: {twin}");
            }
            if now.as_deref() != base_blob {
                return Err(Stale(match now {
                    Some(now) => format!("그 사이 다른 판이 생겼다: 지금 blob {now}"),
                    None => "기준 판이 없다: 그 문서가 이 branch에 없다".into(),
                })
                .into());
            }
            plain_parents(&cwd, &path)?;
            let target = cwd.join(&path);
            std::fs::create_dir_all(target.parent().context("문서 폴더가 없다")?)?;
            std::fs::write(&target, text).with_context(|| format!("문서를 쓰지 못했다: {path}"))?;
            let committed = async {
                git_told(&cwd, &["add", "--", &path], GIT_WAIT).await?;
                git_told(&cwd, &["commit", "--quiet", "--no-verify", "-m", message, "--", &path], GIT_WAIT).await
            }
            .await;
            if let Err(e) = committed {
                // 쓴 파일을 되돌린다: 남겨 두면 다음 저장·쉼이 「commit하지 않은 변경」으로 막힌다. index는 되돌릴 수 있으면
                // 되돌리고(잠겨 있으면 못 한다), 작업 파일은 늘 원래 글로 둔다
                let file = match &now {
                    // index와 작업 파일을 함께 HEAD로. 안 되면(index 잠김) 작업 파일만 원래 글로
                    Some(blob) => match git_told(&cwd, &["checkout", "--quiet", "HEAD", "--", &path], GIT_WAIT).await {
                        Ok(_) => Ok(()),
                        Err(_) => match git(&cwd, &["cat-file", "blob", blob], 64 << 20).await {
                            Ok(original) => std::fs::write(&target, original).map_err(Into::into),
                            Err(e) => Err(e),
                        },
                    },
                    None => {
                        aaa_common::diagnostic::ignored(
                            git_told(&cwd, &["rm", "--quiet", "--cached", "--ignore-unmatch", "--", &path], GIT_WAIT)
                                .await,
                            "실패한 기획 저장의 index 되돌리기",
                        );
                        std::fs::remove_file(&target).map_err(Into::into)
                    }
                };
                aaa_common::diagnostic::ignored(file, "실패한 기획 저장의 작업 파일 되돌리기");
                return Err(e);
            }
            let commit = git_told(&cwd, &["rev-parse", "HEAD"], GIT_WAIT).await?;
            let blob = git_told(&cwd, &["rev-parse", &format!("HEAD:{path}")], GIT_WAIT).await?;
            Ok(AgentResponse::DesignSaved { commit, blob })
        }
        .await,
    )
}

/// 기획서 ID 한 번에 달기(design_ids.v1, t-1957): 기획 branch worktree의 문서마다 표지가 없으면 넣고, 바뀐 파일만 한 commit으로
/// 남긴다. 이미 표지가 있는 문서는 그대로다(다시 돌려도 안전하다). 검사는 `save_bytes`와 같고(그 branch·깨끗한 worktree·일반
/// 파일·symlink 없는 폴더), commit하지 못하면 쓴 파일을 모두 HEAD로 되돌린다
pub async fn mark_ids(
    cfg: &ThreadsConfig,
    (project, repo, folder, slug): (&str, &str, &str, &str),
    ids: &[aaa_proto::DesignIdMark],
    message: &str,
) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let branch = checked(project, slug)?;
            let folder = design_clean_path(folder).context("기획 폴더 경로가 잘못됐다")?;
            if message.trim().is_empty() {
                bail!("commit 메시지가 비었다");
            }
            let mut picks = Vec::with_capacity(ids.len());
            for m in ids {
                let path = design_clean_path(&m.path).context("문서 경로가 잘못됐다")?;
                if !design_inside(&folder, &path) {
                    bail!("기획 폴더 밖 경로다: {path}");
                }
                let id = aaa_proto::canonical_short_id(&m.id, aaa_proto::DESIGN_ID_PREFIX)
                    .with_context(|| format!("기획서 ID가 아니다: {}", m.id))?;
                picks.push((path, id));
            }
            let top = toplevel(repo).await?;
            let (cwd, _) = own_worktree(cfg, &top, project, slug)
                .await?
                .ok_or_else(|| Missing(format!("{branch}의 worktree가 없다(쉬는 branch면 먼저 연다)")))?;
            let head = git_told(&cwd, &["symbolic-ref", "-q", "HEAD"], GIT_WAIT).await.unwrap_or_default();
            if head != format!("refs/heads/{branch}") {
                bail!("기획 worktree가 {branch}에 있지 않다({head})");
            }
            let dirty = git_told(&cwd, &["status", "--porcelain", "--untracked-files=all"], GIT_WAIT).await?;
            if !dirty.is_empty() {
                bail!("기획 worktree에 commit하지 않은 변경이 있다: 사람이 확인한다({})", cwd.display());
            }
            let listed = entries(
                &git(&cwd, &["ls-tree", "-r", "-z", "-l", "--full-tree", "HEAD", "--", &folder], 8 << 20).await?,
            )?;
            // 검사와 새 글을 먼저 모두 만든 뒤 쓴다: 검사 실패로 반쯤 쓴 worktree를 남기지 않는다
            let mut writes: Vec<(String, std::path::PathBuf, String)> = Vec::new();
            for (path, id) in &picks {
                // vendor 폴더(남의 원문)는 host가 고르지 않지만 agent도 고치지 않는다
                if aaa_proto::design_id_unmarked_path(path) {
                    continue;
                }
                // 이 branch에 없는 문서(그 사이 지웠다·옮겼다)는 건너뛴다: 답의 수가 줄어 host가 알린다
                let Some(e) = listed.iter().find(|e| e.path == *path) else { continue };
                if !e.regular() {
                    bail!("일반 파일이 아니다: {path}");
                }
                if !Path::new(path).extension().is_some_and(|x| x.eq_ignore_ascii_case("md")) {
                    continue;
                }
                plain_parents(&cwd, path)?;
                let target = cwd.join(path);
                let text = std::fs::read(&target).with_context(|| format!("문서를 읽지 못했다: {path}"))?;
                let Ok(text) = String::from_utf8(text) else { continue };
                let out = aaa_proto::design_put_id(&text, id);
                if out == text || out.len() as u64 > DESIGN_MAX_TEXT {
                    continue;
                }
                writes.push((path.clone(), target, out));
            }
            let mut marked = Vec::new();
            for (path, target, out) in writes {
                marked.push(path.clone());
                if let Err(e) = std::fs::write(&target, out) {
                    restore(&cwd, &marked).await;
                    return Err(anyhow!(e).context(format!("문서를 쓰지 못했다: {path}")));
                }
            }
            if marked.is_empty() {
                return Ok(AgentResponse::DesignIdsMarked { commit: None, marked });
            }
            let mut add: Vec<&str> = vec!["add", "--"];
            add.extend(marked.iter().map(String::as_str));
            let mut commit: Vec<&str> = vec!["commit", "--quiet", "--no-verify", "-m", message, "--"];
            commit.extend(marked.iter().map(String::as_str));
            let committed = async {
                git_told(&cwd, &add, GIT_WAIT).await?;
                git_told(&cwd, &commit, GIT_WAIT).await
            }
            .await;
            if let Err(e) = committed {
                restore(&cwd, &marked).await;
                return Err(e);
            }
            let commit = git_told(&cwd, &["rev-parse", "HEAD"], GIT_WAIT).await?;
            Ok(AgentResponse::DesignIdsMarked { commit: Some(commit), marked })
        }
        .await,
    )
}

/// 바꾼 파일을 index·작업 파일 모두 HEAD로 되돌린다(남기면 다음 저장이 「commit하지 않은 변경」으로 막힌다)
pub(crate) async fn restore(cwd: &Path, paths: &[String]) {
    let mut args: Vec<&str> = vec!["checkout", "--quiet", "HEAD", "--"];
    args.extend(paths.iter().map(String::as_str));
    aaa_common::diagnostic::ignored(git_told(cwd, &args, GIT_WAIT).await, "실패한 기획 파일 변경 되돌리기");
}

/// 저장소 기획 검사를 그 worktree에서 셸 없이 돌린다(출력은 받지 않는다: 비밀값이 섞일 수 있다)
async fn run_check(cwd: &Path, check: &[String]) -> Result<()> {
    use aaa_common::child_process::TokioCommandExt as _;
    let (exe, args) = check.split_first().context("기획 검사 명령이 비었다")?;
    let status = tokio::time::timeout(
        CHECK_WAIT,
        tokio::process::Command::new(exe)
            .args(args)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .status_guarded(),
    )
    .await
    .map_err(|_| anyhow!("기획 검사 시간 상한이다: {}", check.join(" ")))?
    .with_context(|| format!("기획 검사를 띄우지 못했다: {}", check.join(" ")))?;
    if !status.success() {
        bail!("기획 검사를 통과하지 못해 올리지 않았다(종료 코드 {:?}): {}", status.code(), check.join(" "));
    }
    if !git_told(cwd, &["status", "--porcelain", "--untracked-files=all"], GIT_WAIT).await?.is_empty() {
        bail!("기획 검사가 worktree에 파일을 남겨 올리지 않았다: 검사를 고치거나 .gitignore에 둔다({})", cwd.display());
    }
    Ok(())
}

/// 기획 branch를 origin의 같은 이름으로 올린다(force 없음, push hook 없음). `check`가 있으면 먼저 통과해야 한다
pub async fn push(
    cfg: &ThreadsConfig,
    project: &str,
    repo: &str,
    slug: &str,
    check: &[String],
) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let branch = checked(project, slug)?;
            let top = toplevel(repo).await?;
            if !has_origin(&top).await? {
                bail!("origin이 없는 저장소다");
            }
            if !check.is_empty() {
                let (cwd, _) = own_worktree(cfg, &top, project, slug)
                    .await?
                    .ok_or_else(|| anyhow!("기획 검사를 돌릴 {branch} worktree가 없다(다시 연 뒤 올린다)"))?;
                run_check(&cwd, check).await?;
            }
            let full = format!("refs/heads/{branch}");
            let head = rev(&top, &full).await?;
            let spec = format!("{full}:{full}");
            tokio::time::timeout(PUSH_WAIT, git(&top, &["push", "--quiet", "--no-verify", "origin", &spec], 4096))
                .await
                .map_err(|_| anyhow!("origin에 올리는 시간이 상한을 넘었다"))?
                .context("origin에 올리지 못했다(앞선 판이 origin에 있으면 사람이 합친다)")?;
            Ok(AgentResponse::DesignPushed { head })
        }
        .await,
    )
}

/// origin의 그 branch 끝(없으면 None)
async fn remote_tip(top: &Path, branch: &str) -> Result<Option<String>> {
    let full = format!("refs/heads/{branch}");
    let out = tokio::time::timeout(PUSH_WAIT, git(top, &["ls-remote", "--heads", "origin", &full], 4096))
        .await
        .map_err(|_| anyhow!("origin을 보는 시간이 상한을 넘었다"))?
        .context("origin을 보지 못했다")?;
    Ok(String::from_utf8_lossy(&out).split_whitespace().next().map(str::to_string))
}

/// 기획 branch worktree를 force 없이 내린다. `discard`면 로컬 branch(`keep_local`이면 남긴다), `remote`(host가 올린 판)면
/// origin branch가 그 판일 때만 지운다. origin에 남이 그 위로 올렸으면 아무것도 지우기 전에 멈춘다
pub async fn close(
    cfg: &ThreadsConfig,
    project: &str,
    repo: &str,
    slug: &str,
    (discard, keep_local): (bool, bool),
    remote: Option<&str>,
) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let branch = checked(project, slug)?;
            let _lease = cfg.lease().context("작업 root 잠금")?;
            let top = toplevel(repo).await?;
            let remote = match remote {
                Some(expect) if has_origin(&top).await? => match remote_tip(&top, &branch).await? {
                    Some(tip) if tip == expect => Some(tip),
                    Some(tip) => {
                        bail!("origin의 {branch}에 다른 판({tip})이 올라와 있어 지우지 않았다: 사람이 확인한다")
                    }
                    None => None,
                },
                _ => None,
            };
            if let Some((cwd, locked)) = own_worktree(cfg, &top, project, slug).await? {
                let at = cwd.to_string_lossy().into_owned();
                if locked {
                    git_told(&top, &["worktree", "unlock", &at], GIT_WAIT).await?;
                }
                if let Err(e) = git_told(&top, &["worktree", "remove", &at], GIT_WAIT).await {
                    if locked {
                        let reason = cfg.lock_reason(&cwd)?.unwrap_or_default();
                        aaa_common::diagnostic::ignored(
                            git_told(&top, &["worktree", "lock", "--reason", &reason, &at], GIT_WAIT).await,
                            "지우지 못한 자리를 다시 잠근다",
                        );
                    }
                    return Err(
                        e.context("기획 worktree를 지우지 못했다(commit하지 않은 변경이 있으면 사람이 확인한다)")
                    );
                }
            }
            if discard && !keep_local && has_ref(&top, &format!("refs/heads/{branch}")).await {
                git_told(&top, &["branch", "-D", &branch], GIT_WAIT).await?;
            }
            if let Some(tip) = remote {
                let full = format!("refs/heads/{branch}");
                let lease = format!("--force-with-lease={full}:{tip}");
                tokio::time::timeout(
                    PUSH_WAIT,
                    git(&top, &["push", "--quiet", "--no-verify", &lease, "origin", &format!(":{full}")], 4096),
                )
                .await
                .map_err(|_| anyhow!("origin branch를 지우는 시간이 상한을 넘었다"))?
                .context("origin branch를 지우지 못했다(그 사이 다른 판이 올라왔으면 사람이 확인한다)")?;
            }
            Ok(AgentResponse::Done)
        }
        .await,
    )
}

/// 기획 branch에 있는 일반 그림 파일 하나만 지우고 그 삭제를 commit한다.
pub async fn detach(
    cfg: &ThreadsConfig,
    (project, repo, folder, slug): (&str, &str, &str, &str),
    path: &str,
    message: &str,
) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let branch = checked(project, slug)?;
            let (folder, path) = write_path(folder, path)?;
            let ext = Path::new(&path).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
            let ext = match ext.as_str() {
                "jpeg" => "jpg",
                "png" | "jpg" | "gif" | "webp" => &ext,
                _ => bail!("PNG·JPEG·GIF·WebP 그림만 지운다: {path}"),
            };
            let cwd = write_worktree(cfg, repo, project, slug, &branch).await?;
            let head = rev(&cwd, "HEAD").await?;
            let (blob, size) = crate::design::blob(&cwd, &head, &folder, &path).await?;
            let header = crate::design::blob_slice(&cwd, &blob, 0, size.min(12) as usize).await?;
            if aaa_common::util::image_extension(&header) != Some(ext) {
                bail!("파일 내용이 그림 형식과 맞지 않다: {path}");
            }
            let warnings = tokio::time::timeout(GIT_WAIT, crate::design::image_reference_warnings(&cwd, &head, &path))
                .await
                .context("그림 참조 검사 시간 상한이다")??;
            if rev(&cwd, "HEAD").await? != head {
                return Err(Stale("그림을 검사하는 동안 기획 branch가 바뀌었다".into()).into());
            }
            plain_parents(&cwd, &path)?;
            let literal = format!(":(literal){path}");
            git_told(&cwd, &["rm", "--quiet", "--", &literal], GIT_WAIT).await?;
            let committed =
                git_told(&cwd, &["commit", "--quiet", "--no-verify", "-m", message, "--", &literal], GIT_WAIT).await;
            if let Err(e) = committed {
                restore(&cwd, std::slice::from_ref(&literal)).await;
                return Err(e);
            }
            let commit = rev(&cwd, "HEAD").await?;
            Ok(AgentResponse::DesignDetached { commit, warnings })
        }
        .await,
    )
}

/// 그림·영상 조각을 모으는 자리(agent 상태 폴더 안, 기획 branch·경로마다 하나)
fn attach_part(state: &Path, project: &str, repo: &str, slug: &str, path: &str) -> PathBuf {
    use sha2::Digest as _;
    let key = hex::encode(sha2::Sha256::digest(format!("{project}\0{repo}\0{slug}\0{path}").as_bytes()));
    state.join("design-attach").join(format!("{}.part", &key[..32]))
}

/// 그림·영상 조각 하나를 받는다. 마지막 조각이면 크기·sha256을 확인하고 새 파일로 commit한다(조각 자리는 늘 지운다)
#[allow(clippy::too_many_arguments)]
pub async fn attach(
    cfg: &ThreadsConfig,
    state: &Path,
    (project, repo, folder, slug, path): (&str, &str, &str, &str, &str),
    offset: u64,
    data: &[u8],
    eof: bool,
    sha256: &str,
    message: &str,
) -> Result<AgentResponse, ErrorBody> {
    let part = attach_part(state, project, repo, slug, path);
    let gathered = (|| -> Result<Vec<u8>> {
        std::fs::create_dir_all(part.parent().context("조각 폴더가 없다")?)?;
        let have =
            if offset == 0 { 0 } else { std::fs::metadata(&part).map(|m| m.len()).context("앞 조각이 없다")? };
        if have != offset {
            bail!("조각 자리가 어긋났다(받은 {have}바이트, 요청 {offset})");
        }
        if offset + data.len() as u64 > DESIGN_MAX_FILE {
            bail!("파일이 너무 크다(상한 {DESIGN_MAX_FILE}바이트)");
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(offset != 0)
            .write(true)
            .truncate(offset == 0)
            .open(&part)?;
        std::io::Write::write_all(&mut f, data)?;
        if !eof {
            return Ok(Vec::new());
        }
        drop(f);
        let bytes = std::fs::read(&part)?;
        use sha2::Digest as _;
        if hex::encode(sha2::Sha256::digest(&bytes)) != sha256 {
            bail!("넘긴 파일의 sha256이 다르다");
        }
        Ok(bytes)
    })();
    let bytes = match gathered {
        Ok(_) if !eof => return Ok(AgentResponse::Done),
        Ok(bytes) => bytes,
        Err(e) => {
            aaa_common::diagnostic::ignored(std::fs::remove_file(&part), "실패한 조각 자리 지우기");
            return Err(ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")));
        }
    };
    aaa_common::diagnostic::ignored(std::fs::remove_file(&part), "다 받은 조각 자리 지우기");
    match save_bytes(cfg, project, repo, folder, slug, path, None, (&bytes, DESIGN_MAX_FILE), message).await {
        // 이름이 내용 hash다: 같은 이름이 이미 있고 내용도 같으면 붙인 것이다(같은 그림을 다시 붙이거나 이웃 문서에 붙임)
        Err(e) if e.code == ErrorCode::Conflict => {
            let same = async {
                let top = toplevel(repo).await?;
                let (cwd, _) = own_worktree(cfg, &top, project, slug).await?.context("기획 worktree가 없다")?;
                let blob = git_told(&cwd, &["rev-parse", &format!("HEAD:{path}")], GIT_WAIT).await?;
                let now = git(&cwd, &["cat-file", "blob", &blob], DESIGN_MAX_FILE as usize + 1).await?;
                anyhow::Ok((now == bytes).then_some((git_told(&cwd, &["rev-parse", "HEAD"], GIT_WAIT).await?, blob)))
            }
            .await;
            match same {
                Ok(Some((commit, blob))) => Ok(AgentResponse::DesignSaved { commit, blob }),
                _ => Err(e),
            }
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::tests::{lab, out, sh};
    use aaa_proto::ErrorCode;

    fn cfg(root: &Path) -> ThreadsConfig {
        ThreadsConfig { work_root: root.join("work-root"), ..Default::default() }
    }

    async fn has(dir: &Path, full: &str) -> bool {
        has_ref(dir, full).await
    }

    /// 기획 branch를 열어 쓰고 올리고, 쉬느라 내렸다 다시 열고, 버린다. main checkout과 main은 그대로다
    /// 기획서 ID 한 번에 달기(t-1957): 표지 없는 문서만 한 commit, vendor·없는 문서·이미 단 문서는 건너뛰고, 다시 해도 그대로
    #[tokio::test]
    async fn mark_ids_adds_markers_in_one_commit() {
        use aaa_proto::DesignIdMark;
        let (t, clone) = lab().await;
        let repo = clone.to_str().unwrap();
        let cfg = cfg(t.path());
        sh(&clone, &["config", "user.name", "t"]).await;
        sh(&clone, &["config", "user.email", "t@example.com"]).await;
        let Ok(AgentResponse::DesignTree(main)) = crate::design::tree(repo, "docs", None).await else {
            panic!("트리")
        };
        assert!(main.ids, "트리는 ID 표지를 읽었다");
        assert!(main.docs.iter().all(|d| d.id.is_none() && !d.id_marked));
        let Ok(AgentResponse::DesignBranch { cwd, head, .. }) = open(&cfg, "demo", repo, "aaa-ids").await else {
            panic!("열기")
        };
        let mark = |path: &str, id: &str| DesignIdMark { path: path.into(), id: id.into() };
        let ids = [
            mark("docs/overview.md", "k-1"),
            mark("docs/game-loop/README.md", "k-0002"),
            mark("docs/missing.md", "k-0003"),
            mark("docs/vendor/LICENSE.md", "k-0004"),
        ];
        let Ok(AgentResponse::DesignIdsMarked { commit: Some(commit), marked }) =
            mark_ids(&cfg, ("demo", repo, "docs", "aaa-ids"), &ids, "기획: 기획서 ID 표지 4개 (사람)").await
        else {
            panic!("달기")
        };
        assert_eq!(marked, ["docs/overview.md", "docs/game-loop/README.md"]);
        let cwd = Path::new(&cwd);
        assert_eq!(out(cwd, &["rev-list", "--count", &format!("{head}..{commit}")], b"").await, "1", "한 commit");
        assert_eq!(std::fs::read_to_string(cwd.join("docs/overview.md")).unwrap(), "# 개요\n<!-- aaa-id: k-0001 -->\n");
        assert_eq!(
            std::fs::read_to_string(cwd.join("docs/game-loop/README.md")).unwrap(),
            "---\nx: 1\n---\n# 게임 루프\n<!-- aaa-id: k-0002 -->\n\n![그림](media/a.png)\n",
            "YAML 머리 뒤 제목 아래"
        );
        assert_eq!(out(cwd, &["status", "--porcelain"], b"").await, "", "남은 변경 없음");
        let Ok(AgentResponse::DesignIdsMarked { commit: None, marked }) =
            mark_ids(&cfg, ("demo", repo, "docs", "aaa-ids"), &ids, "기획: 다시").await
        else {
            panic!("다시 달기")
        };
        assert!(marked.is_empty(), "이미 단 문서는 그대로");
        let Ok(AgentResponse::DesignTree(branch)) = crate::design::tree(repo, "docs", Some("aaa-ids")).await else {
            panic!("branch 트리")
        };
        let got: Vec<_> = branch.docs.iter().map(|d| (d.path.as_str(), d.id.as_deref(), d.id_marked)).collect();
        assert_eq!(
            got,
            [("docs/game-loop/README.md", Some("k-0002"), true), ("docs/overview.md", Some("k-0001"), true)]
        );
        let e = mark_ids(&cfg, ("demo", repo, "docs", "aaa-ids"), &[mark("docs/overview.md", "d-1")], "기획: x")
            .await
            .unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{e}");
        // 뒤 문서가 일반 파일이 아니면 앞 문서도 쓰지 않는다(반쯤 쓴 worktree를 남기지 않는다, symlink는 Unix에서)
        #[cfg(unix)]
        {
            std::fs::write(cwd.join("docs/new.md"), "# 새 문서\n").unwrap();
            std::os::unix::fs::symlink("overview.md", cwd.join("docs/link.md")).unwrap();
            sh(cwd, &["add", "docs/new.md", "docs/link.md"]).await;
            sh(cwd, &["commit", "-q", "-m", "더함"]).await;
            let e = mark_ids(
                &cfg,
                ("demo", repo, "docs", "aaa-ids"),
                &[mark("docs/new.md", "k-5"), mark("docs/link.md", "k-6")],
                "기획: x",
            )
            .await
            .unwrap_err();
            assert!(e.message.contains("일반 파일이 아니다"), "{e}");
            assert_eq!(std::fs::read_to_string(cwd.join("docs/new.md")).unwrap(), "# 새 문서\n");
            assert_eq!(out(cwd, &["status", "--porcelain"], b"").await, "", "남은 변경 없음");
        }
        // 그림은 고르지 않는다
        let Ok(AgentResponse::DesignIdsMarked { commit: None, .. }) =
            mark_ids(&cfg, ("demo", repo, "docs", "aaa-ids"), &[mark("docs/game-loop/media/a.png", "k-7")], "기획: x")
                .await
        else {
            panic!("그림은 건너뛴다")
        };
    }

    #[tokio::test]
    async fn open_save_push_rest_and_discard() {
        let (t, clone) = lab().await;
        let repo = clone.to_str().unwrap();
        let origin = t.path().join("origin.git");
        let cfg = cfg(t.path());
        sh(&clone, &["config", "user.name", "t"]).await;
        sh(&clone, &["config", "user.email", "t@example.com"]).await;
        let clone_head = out(&clone, &["rev-parse", "HEAD"], b"").await;
        let main = out(&origin, &["rev-parse", "refs/heads/main"], b"").await;

        let Ok(AgentResponse::DesignBranch { cwd, head, base, .. }) = open(&cfg, "demo", repo, "boss-dex").await else {
            panic!("열기")
        };
        assert_eq!((head.as_str(), base.as_str()), (main.as_str(), "origin/main"), "origin 기본 branch에서 갈라진다");
        assert_eq!(
            Path::new(&cwd).canonicalize().unwrap(),
            t.path().join("work-root/clone/demo-design-boss-dex").canonicalize().unwrap()
        );
        let Ok(AgentResponse::DesignBranch { cwd: again, .. }) = open(&cfg, "demo", repo, "boss-dex").await else {
            panic!("다시 열기")
        };
        assert_eq!(again, cwd, "열린 자리를 그대로 준다");

        let doc = "docs/bosses/README.md";
        let Ok(AgentResponse::DesignSaved { commit: first, blob }) =
            save(&cfg, "demo", repo, "docs", "boss-dex", doc, None, ("# 보스 도감\n", "기획: 보스 도감 (사람)")).await
        else {
            panic!("새 문서")
        };
        for stale in [None, Some("0123456789abcdef0123456789abcdef01234567")] {
            let e =
                save(&cfg, "demo", repo, "docs", "boss-dex", doc, stale, ("# 겹침\n", "기획: 겹침")).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::Conflict, "{e}");
        }
        let Ok(AgentResponse::DesignSaved { commit: second, .. }) = save(
            &cfg,
            "demo",
            repo,
            "docs",
            "boss-dex",
            doc,
            Some(&blob),
            ("# 보스 도감\n\n## 해골 왕\n", "기획: 해골 왕"),
        )
        .await
        else {
            panic!("고침")
        };
        let e =
            save(&cfg, "demo", repo, "docs", "boss-dex", "README.md", None, ("# 밖\n", "기획: 밖")).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{e}");
        assert_eq!(out(&clone, &["rev-parse", "HEAD"], b"").await, clone_head, "main checkout은 그대로");
        assert!(!clone.join("docs/bosses").exists());
        assert_eq!(out(Path::new(&cwd), &["log", "-1", "--format=%s"], b"").await, "기획: 해골 왕");

        let Ok(AgentResponse::DesignTree(tree)) = crate::design::tree(repo, "docs", Some("boss-dex")).await else {
            panic!("branch 트리")
        };
        assert!(!tree.marks, "기획 branch 트리는 표지를 세지 않는다(host는 고친 파일만 쓴다)");
        assert_eq!((tree.base.as_str(), tree.commit.as_str(), tree.ahead), ("design/boss-dex", second.as_str(), 2));
        let changed: Vec<_> = tree.changed.iter().map(|c| (c.status.as_str(), c.path.as_str())).collect();
        assert_eq!(changed, [("A", doc)]);
        let Ok(AgentResponse::DesignText { text, .. }) =
            crate::design::read(repo, "docs", Some(&first), doc, Some("boss-dex")).await
        else {
            panic!("branch의 옛 판")
        };
        assert_eq!(text, "# 보스 도감\n");
        let e = crate::design::read(repo, "docs", Some(&first), doc, None).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "branch를 주지 않으면 main의 판만: {e}");

        // 저장소 기획 검사를 통과하지 못하면 올리지 않는다(셸 없이, 그 worktree에서)
        let failing = ["git".to_string(), "no-such-subcommand".to_string()];
        let e = push(&cfg, "demo", repo, "boss-dex", &failing).await.unwrap_err();
        assert!(e.message.contains("기획 검사를 통과하지 못해"), "{e}");
        assert!(!has(&origin, "refs/heads/design/boss-dex").await);
        let passing = ["git".to_string(), "--version".to_string()];
        let Ok(AgentResponse::DesignPushed { head }) = push(&cfg, "demo", repo, "boss-dex", &passing).await else {
            panic!("올리기")
        };
        assert_eq!(head, second);
        assert_eq!(out(&origin, &["rev-parse", "refs/heads/design/boss-dex"], b"").await, second);
        assert_eq!(out(&origin, &["rev-parse", "refs/heads/main"], b"").await, main, "main은 그대로");

        // 쉼: worktree만 내리고 branch는 남긴다. 올리지 않은 파일이 있으면 내리지 않는다
        std::fs::write(Path::new(&cwd).join("docs/scratch.md"), "초안").unwrap();
        assert!(close(&cfg, "demo", repo, "boss-dex", (false, false), None).await.is_err());
        assert!(Path::new(&cwd).join("docs/scratch.md").exists(), "남은 글은 그대로");
        std::fs::remove_file(Path::new(&cwd).join("docs/scratch.md")).unwrap();
        assert!(matches!(close(&cfg, "demo", repo, "boss-dex", (false, false), None).await, Ok(AgentResponse::Done)));
        assert!(!Path::new(&cwd).exists());
        assert!(has(&clone, "refs/heads/design/boss-dex").await);
        let Ok(AgentResponse::DesignBranch { head, .. }) = open(&cfg, "demo", repo, "boss-dex").await else {
            panic!("다시 꺼내기")
        };
        assert_eq!(head, second, "쉬던 branch를 이어 쓴다");
        // 누가 폴더를 지웠다: 그 기록만 지우고 다시 꺼낸다(branch·commit은 그대로)
        std::fs::remove_dir_all(&cwd).unwrap();
        let Ok(AgentResponse::DesignBranch { head, opened, .. }) = open(&cfg, "demo", repo, "boss-dex").await else {
            panic!("지워진 자리 다시 꺼내기")
        };
        assert_eq!((head.as_str(), opened.as_str()), (second.as_str(), "local"));
        assert!(Path::new(&cwd).exists());

        assert!(matches!(
            close(&cfg, "demo", repo, "boss-dex", (true, false), Some(&second)).await,
            Ok(AgentResponse::Done)
        ));
        assert!(!has(&clone, "refs/heads/design/boss-dex").await);
        assert!(!has(&origin, "refs/heads/design/boss-dex").await, "origin branch도 지운다");
        assert!(has(&origin, "refs/heads/main").await);
    }

    /// 앞선 판이 origin에 있으면 덮지 않고, 손으로 바꾼 worktree에는 쓰지 않는다. 로컬에 없는 branch는 origin에서 꺼낸다
    /// 그림 붙이기(t-1894 2-3): 조각을 이어 받아 마지막 조각에서 sha256을 보고 새 파일로 commit한다. 자리가 어긋나거나 sha256이
    /// 다르면 쓰지 않고 조각 자리를 지운다. 이미 있는 파일에는 쓰지 않는다
    #[tokio::test]
    async fn attach_gathers_chunks_and_commits_one_new_file() {
        use sha2::Digest as _;
        let (t, clone) = lab().await;
        let repo = clone.to_str().unwrap();
        let cfg = cfg(t.path());
        let state = t.path().join("agent-state");
        sh(&clone, &["config", "user.name", "t"]).await;
        sh(&clone, &["config", "user.email", "t@example.com"]).await;
        let Ok(AgentResponse::DesignBranch { cwd, .. }) = open(&cfg, "demo", repo, "boss-dex").await else {
            panic!("열기")
        };
        let bytes: Vec<u8> = (0..1_300_000u32).map(|n| (n % 251) as u8).collect();
        let sha = hex::encode(sha2::Sha256::digest(&bytes));
        let path = "docs/bosses/0123456789abcdef.png";
        let send = |offset: usize, chunk: &[u8], eof: bool, sha: &str| {
            let (cfg, state, chunk, sha) = (cfg.clone(), state.clone(), chunk.to_vec(), sha.to_string());
            async move {
                attach(
                    &cfg,
                    &state,
                    ("demo", repo, "docs", "boss-dex", path),
                    offset as u64,
                    &chunk,
                    eof,
                    &sha,
                    "기획: 그림 (사람)",
                )
                .await
            }
        };
        // 자리가 어긋난 조각(앞 조각 없이 중간부터)은 받지 않는다
        let e = send(512, &bytes[512..1024], false, &sha).await.unwrap_err();
        assert!(e.message.contains("앞 조각"), "{e}");
        // sha256이 다르면 commit하지 않고 조각 자리를 지운다
        assert!(matches!(send(0, &bytes[..1000], false, &sha).await, Ok(AgentResponse::Done)));
        let e = send(1000, &bytes[1000..2000], true, &"0".repeat(64)).await.unwrap_err();
        assert!(e.message.contains("sha256"), "{e}");
        assert!(!attach_part(&state, "demo", repo, "boss-dex", path).exists(), "실패한 조각 자리를 지운다");
        // 조각 셋으로 이어 받아 commit한다
        let cuts = [0, 500_000, 1_000_000, bytes.len()];
        let mut last = None;
        for w in cuts.windows(2) {
            last = Some(send(w[0], &bytes[w[0]..w[1]], w[1] == bytes.len(), &sha).await.unwrap());
        }
        let Some(AgentResponse::DesignSaved { commit, .. }) = last else { panic!("{last:?}") };
        assert_eq!(std::fs::read(Path::new(&cwd).join(path)).unwrap(), bytes);
        assert_eq!(out(Path::new(&cwd), &["rev-parse", "HEAD"], b"").await, commit);
        assert_eq!(
            out(Path::new(&cwd), &["show", "--name-only", "--format=", "HEAD"], b"").await,
            path,
            "그 파일만 commit"
        );
        assert!(!attach_part(&state, "demo", repo, "boss-dex", path).exists(), "다 받은 조각 자리를 지운다");
        // 이름이 내용 hash라 같은 이름·같은 내용은 이미 붙인 것이다(새 commit 없이 성공). 내용이 다르면 다른 판으로 거부한다
        let again = send(0, &bytes, true, &sha).await.unwrap();
        assert!(matches!(&again, AgentResponse::DesignSaved { commit: c, .. } if *c == commit), "{again:?}");
        let e = send(0, &bytes[..10], true, &hex::encode(sha2::Sha256::digest(&bytes[..10]))).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::Conflict, "{e}");
        // 상한을 넘는 조각은 받지 않는다
        let e = send(0, &vec![0; DESIGN_MAX_FILE as usize + 1], true, &sha).await.unwrap_err();
        assert!(e.message.contains("너무 크다"), "{e}");
    }

    #[tokio::test]
    async fn never_forces_or_writes_over_foreign_changes() {
        let (t, clone) = lab().await;
        let repo = clone.to_str().unwrap();
        let origin = t.path().join("origin.git");
        let cfg = cfg(t.path());
        sh(&clone, &["config", "user.name", "t"]).await;
        sh(&clone, &["config", "user.email", "t@example.com"]).await;
        let e = open(&cfg, "demo", repo, "Bad_Slug").await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{e}");
        let Ok(AgentResponse::DesignBranch { cwd, .. }) = open(&cfg, "demo", repo, "shop").await else {
            panic!("열기")
        };
        let doc = "docs/overview.md";
        let blob = out(&clone, &["rev-parse", &format!("HEAD:{doc}")], b"").await;
        let Ok(AgentResponse::DesignSaved { blob, .. }) =
            save(&cfg, "demo", repo, "docs", "shop", doc, Some(&blob), ("# 개요 상점\n", "기획: 상점")).await
        else {
            panic!("저장")
        };
        let Ok(AgentResponse::DesignPushed { head: ours_pushed }) = push(&cfg, "demo", repo, "shop", &[]).await else {
            panic!("올리기")
        };

        // 다른 곳에서 같은 branch에 다른 판을 올렸다
        let work = t.path().join("work");
        sh(&work, &["fetch", "-q", "origin"]).await;
        sh(&work, &["switch", "-q", "-c", "design/shop", "origin/design/shop"]).await;
        std::fs::write(work.join("docs/overview.md"), "# 다른 손\n").unwrap();
        sh(&work, &["commit", "-q", "-am", "다른 손"]).await;
        sh(&work, &["push", "-q", "origin", "design/shop"]).await;
        let theirs = out(&origin, &["rev-parse", "refs/heads/design/shop"], b"").await;
        let Ok(AgentResponse::DesignSaved { .. }) =
            save(&cfg, "demo", repo, "docs", "shop", doc, Some(&blob), ("# 개요 상점 둘\n", "기획: 상점 둘")).await
        else {
            panic!("로컬 저장")
        };
        assert!(push(&cfg, "demo", repo, "shop", &[]).await.is_err(), "force 없이 올린다");
        assert_eq!(out(&origin, &["rev-parse", "refs/heads/design/shop"], b"").await, theirs);
        // 버려도 남이 올린 판은 지우지 않는다: 아무것도 지우기 전에 멈춘다
        let e = close(&cfg, "demo", repo, "shop", (true, false), Some(&ours_pushed)).await.unwrap_err();
        assert!(e.message.contains("다른 판"), "{e}");
        assert_eq!(out(&origin, &["rev-parse", "refs/heads/design/shop"], b"").await, theirs);
        assert!(Path::new(&cwd).exists() && has(&clone, "refs/heads/design/shop").await);

        // 손으로 고친 파일이 있으면 쓰지 않는다
        std::fs::write(Path::new(&cwd).join(doc), "손").unwrap();
        let now = out(Path::new(&cwd), &["rev-parse", &format!("HEAD:{doc}")], b"").await;
        let e = save(&cfg, "demo", repo, "docs", "shop", doc, Some(&now), ("# 셋\n", "기획: 셋")).await.unwrap_err();
        assert!(e.message.contains("commit하지 않은 변경"), "{e}");
        sh(Path::new(&cwd), &["checkout", "--", doc]).await;

        // 로컬 branch·자리를 버린 뒤 다시 열면 origin의 branch에서 꺼낸다
        assert!(close(&cfg, "demo", repo, "shop", (true, false), None).await.is_ok());
        assert!(has(&origin, "refs/heads/design/shop").await, "remote를 주지 않으면 origin은 그대로");
        sh(&clone, &["fetch", "-q", "origin"]).await;
        let Ok(AgentResponse::DesignBranch { head, .. }) = open(&cfg, "demo", repo, "shop").await else {
            panic!("origin에서 꺼내기")
        };
        assert_eq!(head, theirs);
    }

    /// 쓰는 동안 저장소 hook은 돌지 않고, 쓰다 실패하면 되돌린다. 대소문자만 다른 문서·다른 자리에 꺼낸 branch에는 쓰지 않는다
    #[tokio::test]
    async fn no_hooks_rollback_case_twins_and_foreign_checkouts() {
        let (t, clone) = lab().await;
        let repo = clone.to_str().unwrap();
        let cfg = cfg(t.path());
        sh(&clone, &["config", "user.name", "t"]).await;
        sh(&clone, &["config", "user.email", "t@example.com"]).await;
        let marker = t.path().join("hook-ran");
        let hooks = clone.join(".git/hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        for hook in ["post-checkout", "prepare-commit-msg", "post-commit"] {
            std::fs::write(
                hooks.join(hook),
                format!(
                    "#!/bin/sh
echo {hook} >> '{}'
",
                    marker.display()
                ),
            )
            .unwrap();
            #[cfg(unix)]
            std::fs::set_permissions(hooks.join(hook), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        }
        let Ok(AgentResponse::DesignBranch { cwd, opened, .. }) = open(&cfg, "demo", repo, "notes").await else {
            panic!("열기")
        };
        assert_eq!(opened, "created");
        let Ok(AgentResponse::DesignSaved { .. }) = save(
            &cfg,
            "demo",
            repo,
            "docs",
            "notes",
            "docs/notes.md",
            None,
            (
                "# 노트
",
                "기획: 노트",
            ),
        )
        .await
        else {
            panic!("저장")
        };
        assert!(!marker.exists(), "hook이 돌았다: {:?}", std::fs::read_to_string(&marker));

        let e = save(
            &cfg,
            "demo",
            repo,
            "docs",
            "notes",
            "docs/Notes.md",
            None,
            (
                "# 둘
",
                "기획: 둘",
            ),
        )
        .await
        .unwrap_err();
        assert!(e.message.contains("대소문자만 다른"), "{e}");

        // index 잠금으로 add를 실패시킨다: 쓴 파일이 남지 않아 다음 저장이 막히지 않는다
        let gitdir = out(Path::new(&cwd), &["rev-parse", "--git-dir"], b"").await;
        let lock = Path::new(&cwd).join(gitdir).join("index.lock");
        std::fs::write(&lock, "").unwrap();
        assert!(
            save(
                &cfg,
                "demo",
                repo,
                "docs",
                "notes",
                "docs/new.md",
                None,
                (
                    "# 새
",
                    "기획: 새"
                )
            )
            .await
            .is_err()
        );
        std::fs::remove_file(&lock).unwrap();
        assert!(!Path::new(&cwd).join("docs/new.md").exists());
        assert_eq!(out(Path::new(&cwd), &["status", "--porcelain"], b"").await, "");
        assert!(
            save(
                &cfg,
                "demo",
                repo,
                "docs",
                "notes",
                "docs/new.md",
                None,
                (
                    "# 새
",
                    "기획: 새"
                )
            )
            .await
            .is_ok()
        );

        // 같은 branch를 기획 자리가 아닌 곳에 꺼냈으면 쓰지도 내리지도 않는다
        assert!(matches!(close(&cfg, "demo", repo, "notes", (false, false), None).await, Ok(AgentResponse::Done)));
        let elsewhere = t.path().join("elsewhere");
        sh(&clone, &["worktree", "add", "-q", elsewhere.to_str().unwrap(), "design/notes"]).await;
        let e = save(
            &cfg,
            "demo",
            repo,
            "docs",
            "notes",
            "docs/notes.md",
            None,
            (
                "# 셋
",
                "기획: 셋",
            ),
        )
        .await
        .unwrap_err();
        assert!(e.message.contains("기획 자리가 아닌 곳"), "{e}");
        assert!(close(&cfg, "demo", repo, "notes", (true, false), None).await.is_err());
        assert!(elsewhere.exists() && has(&clone, "refs/heads/design/notes").await, "사람의 자리는 그대로");
    }
}
