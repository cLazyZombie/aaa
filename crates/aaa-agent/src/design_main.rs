//! 기획 main 직접 저장(design_main.v1, t-2002). 사용자 결정(11-18 q-4213~q-4218)이 11-16의 기획 branch 흐름을 대신한다: 기획
//! 저장 하나가 곧 origin 기본 branch commit·push 하나다.
//!
//! - 쓰는 자리는 저장소·프로젝트마다 하나인 「main 기획 자리」(작업 root마다 `<저장소 이름>/<프로젝트>-design-main`, detached)다.
//!   이 모듈만 쓴다. 쓸 때마다 origin을 받고 그 끝으로 맞춘 뒤(`checkout --detach --force`·`clean -fd`) 쓴다: 올리지 않은 것은
//!   저장이 아니므로 앞서 죽은 쓰기가 남긴 것은 버린다. 사람 checkout과 로컬 branch는 건드리지 않는다.
//! - 파일마다 읽은 blob과 지금 blob을 견주고(다르면 Stale), 한 commit으로 남긴 뒤 저장소 검사(`check`)를 그 commit에서 돌리고
//!   force 없이 push한다. push가 non-fast-forward로 거절되면(그 사이 누가 main에 올렸다) 새 끝에서 처음부터 다시 한다. 인증·
//!   네트워크 실패는 다시 하지 않는다.
//! - 대화 thread는 이 자리를 읽기만 한다(`DesignMainOpen`이 origin 끝으로 맞춘다).
use aaa_proto::{
    AgentResponse, DESIGN_LOG_MAX, DESIGN_MAIN_TRIES, DESIGN_MAX_FILE, DESIGN_MAX_TEXT, DesignIdMark, DesignLogEntry,
    DesignMainBlob, DesignMainChange, DesignMainFile, ErrorBody, ErrorCode, design_clean_path, design_commit_subject,
    design_inside, valid_project,
};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use crate::design::{Stale, base, entries, fetch, fetch_now, git, git_status, has_origin, reply, toplevel};
use crate::design_write::{CHECK_WAIT, GIT_WAIT, PUSH_WAIT, case_twin, git_told, plain_parents, write_path};
use crate::threads::ThreadsConfig;

/// 검사 실패 까닭으로 보이는 stderr 상한(바이트)
const CHECK_TOLD: usize = 2048;
/// 한 저장에 넣는 파일 수 상한
const FILES_MAX: usize = 64;

/// 자리마다 쓰기 하나씩(host가 저장소마다 차례를 지키지만, 열기·쓰기·ID 달기가 겹쳐도 자리를 함께 만지지 않게)
static SLOTS: LazyLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> = LazyLock::new(Default::default);

fn slot_lock(top: &Path, project: &str) -> Arc<tokio::sync::Mutex<()>> {
    SLOTS.lock().unwrap().entry(top.join(project)).or_default().clone()
}

fn slot_name(project: &str) -> String {
    format!("{project}-design-main")
}

/// git이 아는 worktree(경로, 잠김)
async fn worktrees(top: &Path) -> Result<Vec<(PathBuf, bool)>> {
    let list = git(top, &["worktree", "list", "--porcelain", "-z"], 1 << 20).await?;
    let mut out = Vec::new();
    for record in list.split(|b| *b == 0).collect::<Vec<_>>().split(|f| f.is_empty()) {
        let fields: Vec<String> = record.iter().map(|f| String::from_utf8_lossy(f).into_owned()).collect();
        let Some(path) = fields.iter().find_map(|f| f.strip_prefix("worktree ")) else { continue };
        let locked = fields.iter().any(|f| f == "locked" || f.starts_with("locked "));
        out.push((PathBuf::from(path), locked));
    }
    Ok(out)
}

/// 이 저장소·프로젝트의 main 기획 자리. 없으면 지금 작업 root에 origin 기본 branch 끝으로 만든다
async fn slot(cfg: &ThreadsConfig, top: &Path, project: &str) -> Result<PathBuf> {
    let name = top.file_name().context("저장소 이름이 없다")?;
    let have = worktrees(top).await?;
    for root in cfg.known_roots()? {
        let place = root.path.join(name).join(slot_name(project));
        let Ok(want) = crate::work_root::real_location(&place) else { continue };
        let Some((path, locked)) =
            have.iter().find(|(p, _)| crate::work_root::real_location(p).is_ok_and(|p| p == want))
        else {
            continue;
        };
        if path.symlink_metadata().is_ok() {
            return Ok(path.clone());
        }
        if *locked {
            bail!("main 기획 자리가 지금 없다(작업 디스크가 붙어 있는지 본다): {}", path.display());
        }
        // 폴더를 지운 자리: 그 기록만 지우고 새로 만든다
        git_told(top, &["worktree", "remove", "--force", &path.to_string_lossy()], GIT_WAIT).await?;
    }
    if let Err(e) = fetch(top).await {
        tracing::warn!(top = %top.display(), "main 기획 자리를 만들기 전에 origin을 받지 못했다(가진 ref로 만든다): {e:#}");
    }
    let (_, main) = base(top).await?;
    let cwd = cfg.work_root.join(name).join(slot_name(project));
    cfg.check_path(&cwd)?;
    if cwd.symlink_metadata().is_ok() {
        bail!("main 기획 자리가 이미 있다(다른 것이 차지했다): {}", cwd.display());
    }
    std::fs::create_dir_all(cwd.parent().context("자리의 부모가 없다")?)?;
    let at = cwd.to_string_lossy().into_owned();
    let reason = cfg.lock_reason(&cwd)?;
    let mut args = vec!["worktree", "add", "--quiet", "--detach"];
    if let Some(reason) = &reason {
        args.extend(["--lock", "--reason", reason]);
    }
    args.extend([at.as_str(), main.as_str()]);
    if let Err(e) = git_told(top, &args, GIT_WAIT).await {
        // 방금 만들다 만 자리만 치운다(detached라 잃을 commit이 없다)
        if worktrees(top).await.is_ok_and(|w| w.iter().any(|(p, _)| p == &cwd)) {
            aaa_common::diagnostic::ignored(
                git_told(top, &["worktree", "remove", "--force", &at], GIT_WAIT).await,
                "만들다 만 main 기획 자리 치우기",
            );
        }
        return Err(e.context("main 기획 자리를 만들지 못했다"));
    }
    let real = crate::work_root::real_location(&cwd)?;
    Ok(worktrees(top).await?.into_iter().map(|(p, _)| p).find(|p| p == &real || p == &cwd).unwrap_or(real))
}

/// 자리를 그 commit으로 맞춘다: 추적 파일은 그 판대로, 추적하지 않는 파일은 지운다(ignore된 것은 둔다)
async fn sync(cwd: &Path, commit: &str) -> Result<()> {
    git_told(cwd, &["checkout", "--quiet", "--detach", "--force", commit], GIT_WAIT).await?;
    git_told(cwd, &["clean", "-fdq"], GIT_WAIT).await?;
    Ok(())
}

/// origin을 지금 받고 읽는 기본 branch 이름(`main` 등)과 끝
async fn fresh_main(top: &Path) -> Result<(String, String)> {
    fetch_now(top).await?;
    let (name, sha) = base(top).await?;
    let branch = name.strip_prefix("origin/").context("origin 기본 branch가 없다")?.to_string();
    Ok((branch, sha))
}

/// main 기획 자리를 만들거나 origin 기본 branch 끝으로 맞춘다(대화 thread가 읽기 전)
pub async fn open(cfg: &ThreadsConfig, project: &str, repo: &str) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            if !valid_project(project) {
                bail!("프로젝트 이름 형식이 아니다");
            }
            let _lease = cfg.lease().context("작업 root 잠금")?;
            let top = toplevel(repo).await?;
            let lock = slot_lock(&top, project);
            let _one = lock.lock().await;
            let cwd = slot(cfg, &top, project).await?;
            if has_origin(&top).await?
                && let Err(e) = fetch_now(&top).await
            {
                tracing::warn!(repo, "main 기획 자리를 맞추기 전에 origin을 받지 못했다(가진 ref로 맞춘다): {e:#}");
            }
            let (name, main) = base(&top).await?;
            sync(&cwd, &main).await?;
            Ok(AgentResponse::DesignBranch {
                cwd: cwd.to_string_lossy().into_owned(),
                head: main,
                base: name,
                opened: "main".into(),
            })
        }
        .await,
    )
}

/// 조각을 모으는 자리(agent 상태 폴더 안, 프로젝트·저장소·내용마다 하나). 다 받으면 `.ready`로 바꾼다
fn stage_file(state: &Path, project: &str, repo: &str, sha256: &str, ready: bool) -> PathBuf {
    use sha2::Digest as _;
    let key = hex::encode(sha2::Sha256::digest(format!("{project}\0{repo}\0{sha256}").as_bytes()));
    state.join("design-main-stage").join(format!("{}.{}", &key[..32], if ready { "ready" } else { "part" }))
}

fn sha256_ok(sha256: &str) -> bool {
    sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// 그림·영상 조각 하나를 받는다. 마지막 조각이면 크기·sha256을 확인하고 쓸 수 있게 둔다
pub fn stage(
    state: &Path,
    (project, repo): (&str, &str),
    sha256: &str,
    offset: u64,
    data: &[u8],
    eof: bool,
) -> Result<AgentResponse, ErrorBody> {
    let part = stage_file(state, project, repo, sha256, false);
    let result = (|| -> Result<()> {
        if !valid_project(project) || !sha256_ok(sha256) {
            bail!("프로젝트 이름이나 sha256 형식이 아니다");
        }
        let dir = part.parent().context("조각 폴더가 없다")?;
        std::fs::create_dir_all(dir)?;
        if offset == 0 {
            // 끊긴 전송이 남긴 하루 지난 조각은 치운다
            for e in std::fs::read_dir(dir)?.flatten() {
                if e.metadata()
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.elapsed().is_ok_and(|a| a.as_secs() > 86_400))
                {
                    aaa_common::diagnostic::ignored(std::fs::remove_file(e.path()), "지난 기획 조각 치우기");
                }
            }
        }
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
        drop(f);
        if eof {
            use sha2::Digest as _;
            if hex::encode(sha2::Sha256::digest(std::fs::read(&part)?)) != sha256 {
                bail!("넘긴 파일의 sha256이 다르다");
            }
            std::fs::rename(&part, stage_file(state, project, repo, sha256, true))?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(AgentResponse::Done),
        Err(e) => {
            aaa_common::diagnostic::ignored(std::fs::remove_file(&part), "실패한 기획 조각 지우기");
            Err(ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
        }
    }
}

/// 미리 준비한 파일 하나
struct Prepared {
    path: String,
    base: Option<String>,
    /// None이면 지운다
    bytes: Option<Vec<u8>>,
    /// 조각으로 넘긴 그림(이름이 내용 hash다): 같은 내용이 이미 있으면 붙인 것이다
    staged: bool,
}

/// 저장소 검사를 그 자리에서 셸 없이 돌린다. 인자의 `{base}`·`{head}`를 바꾸고, 실패하면 stderr 앞부분을 까닭으로 싣는다
async fn run_check(cwd: &Path, check: &[String], base: &str, head: &str) -> Result<()> {
    use aaa_common::child_process::TokioCommandExt as _;
    let args: Vec<String> = check.iter().map(|a| a.replace("{base}", base).replace("{head}", head)).collect();
    let (exe, rest) = args.split_first().context("기획 검사 명령이 비었다")?;
    let out = tokio::time::timeout(
        CHECK_WAIT,
        tokio::process::Command::new(exe)
            .args(rest)
            .current_dir(cwd)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output_guarded(),
    )
    .await
    .map_err(|_| anyhow!("저장소 문서 검사 시간 상한이다: {}", check.join(" ")))?
    .with_context(|| format!("저장소 문서 검사를 띄우지 못했다: {}", check.join(" ")))?;
    if !out.status.success() {
        let told = String::from_utf8_lossy(&out.stderr);
        let mut told = told.trim().to_string();
        if told.len() > CHECK_TOLD {
            let mut end = CHECK_TOLD;
            while !told.is_char_boundary(end) {
                end -= 1;
            }
            told.truncate(end);
            told.push('…');
        }
        bail!(
            "저장소 문서 검사를 통과하지 못해 저장하지 않았다(종료 코드 {:?}, {}){}",
            out.status.code(),
            check.join(" "),
            if told.is_empty() { String::new() } else { format!(":\n{told}") }
        );
    }
    if !git_told(cwd, &["status", "--porcelain", "--untracked-files=all"], GIT_WAIT).await?.is_empty() {
        bail!("저장소 문서 검사가 파일을 남겨 저장하지 않았다: 검사를 고치거나 .gitignore에 둔다");
    }
    Ok(())
}

enum Pushed {
    Done,
    /// 그 사이 다른 판이 올라왔다(non-fast-forward)
    Behind,
}

/// force 없이 올린다. 거절 까닭은 `--porcelain` 출력의 상태 줄로만 가른다(stderr·첫 줄에는 원격 주소가 섞인다)
async fn push(cwd: &Path, head: &str, branch: &str, old: &str) -> Result<Pushed> {
    let spec = format!("{head}:refs/heads/{branch}");
    let (out, code) = tokio::time::timeout(
        PUSH_WAIT,
        git_status(cwd, &["push", "--porcelain", "--no-verify", "origin", &spec], 64 << 10),
    )
    .await
    .map_err(|_| anyhow!("origin에 올리는 시간이 상한을 넘었다"))??;
    if code == Some(0) {
        // 받기 전에도 다음 읽기가 새 판을 보게 추적 ref를 옮긴다(git도 옮기지만 fetch refspec이 다른 저장소가 있다)
        aaa_common::diagnostic::ignored(
            git_told(cwd, &["update-ref", &format!("refs/remotes/origin/{branch}"), head, old], GIT_WAIT).await,
            "origin 추적 ref 옮기기(다음 받기가 맞춘다)",
        );
        return Ok(Pushed::Done);
    }
    let text = String::from_utf8_lossy(&out);
    let behind = text.lines().any(|l| {
        l.starts_with('!') && l.contains("[rejected]") && (l.contains("non-fast-forward") || l.contains("fetch first"))
    });
    if behind {
        return Ok(Pushed::Behind);
    }
    bail!("origin {branch}에 올리지 못했다(종료 코드 {code:?}, 권한·연결·보호 규칙을 본다)")
}

/// 자리의 HEAD에서 기획 폴더 목록
async fn listing(cwd: &Path, folder: &str) -> Result<Vec<crate::design::Entry>> {
    entries(&git(cwd, &["ls-tree", "-r", "-z", "-l", "--full-tree", "HEAD", "--", folder], 8 << 20).await?)
}

/// 쓰는 동안 쥔 main 기획 자리(작업 root 잠금·자리 차례)
struct Slot {
    top: PathBuf,
    cwd: PathBuf,
    _one: tokio::sync::OwnedMutexGuard<()>,
    _lease: Option<crate::work_root::Lease>,
}

impl Slot {
    async fn take(cfg: &ThreadsConfig, project: &str, repo: &str, message: &str) -> Result<Slot> {
        if !valid_project(project) {
            bail!("프로젝트 이름 형식이 아니다");
        }
        if message.trim().is_empty() {
            bail!("commit 메시지가 비었다");
        }
        let lease = cfg.lease().context("작업 root 잠금")?;
        let top = toplevel(repo).await?;
        if !has_origin(&top).await? {
            bail!("origin이 없는 저장소다");
        }
        let one = slot_lock(&top, project).lock_owned().await;
        let cwd = slot(cfg, &top, project).await?;
        Ok(Slot { top, cwd, _one: one, _lease: lease })
    }

    /// origin을 지금 받고 자리를 그 끝으로 맞춘다: (기본 branch 이름, 끝)
    async fn fresh(&self) -> Result<(String, String)> {
        let (branch, main) = fresh_main(&self.top).await?;
        sync(&self.cwd, &main).await?;
        Ok((branch, main))
    }

    /// 바뀐 경로를 한 commit으로 남기고 검사한 뒤 올린다. 올렸으면 새 commit, 그 사이 main이 앞서 갔으면 None(새 끝에서 다시)
    async fn finish(
        &self,
        changed: &[String],
        message: &str,
        check: &[String],
        (branch, main): (&str, &str),
    ) -> Result<Option<String>> {
        let literal: Vec<String> = changed.iter().map(|p| format!(":(literal){p}")).collect();
        let mut add: Vec<&str> = vec!["add", "--all", "--"];
        add.extend(literal.iter().map(String::as_str));
        let mut commit: Vec<&str> = vec!["commit", "--quiet", "--no-verify", "-m", message, "--"];
        commit.extend(literal.iter().map(String::as_str));
        git_told(&self.cwd, &add, GIT_WAIT).await?;
        git_told(&self.cwd, &commit, GIT_WAIT).await?;
        let head = git_told(&self.cwd, &["rev-parse", "HEAD"], GIT_WAIT).await?;
        if !check.is_empty() {
            run_check(&self.cwd, check, main, &head).await?;
        }
        match push(&self.cwd, &head, branch, main).await? {
            Pushed::Done => Ok(Some(head)),
            Pushed::Behind => {
                tracing::info!(top = %self.top.display(), "기획 저장 사이 main이 바뀌었다: 새 끝에서 다시 한다");
                Ok(None)
            }
        }
    }
}

fn gave_up() -> anyhow::Error {
    anyhow!("main이 계속 바뀌어 {DESIGN_MAIN_TRIES}번 모두 올리지 못했다: 잠시 뒤 다시 저장한다")
}

/// 기획 폴더 파일 여럿을 한 commit으로 main에 올린다(글·조각으로 넘긴 그림·지우기)
pub async fn write(
    cfg: &ThreadsConfig,
    state: &Path,
    (project, repo, folder): (&str, &str, &str),
    files: &[DesignMainFile],
    message: &str,
    check: &[String],
) -> Result<AgentResponse, ErrorBody> {
    let mut staged_used = Vec::new();
    let result = reply(
        async {
            if files.is_empty() || files.len() > FILES_MAX {
                bail!("한 번에 1~{FILES_MAX}개 파일을 저장한다");
            }
            let mut prepared: Vec<Prepared> = Vec::with_capacity(files.len());
            let mut folder_clean = String::new();
            for f in files {
                let (clean_folder, path) = write_path(folder, &f.path)?;
                folder_clean = clean_folder;
                if prepared.iter().any(|p| p.path == path) {
                    bail!("같은 파일을 두 번 저장한다: {path}");
                }
                if f.base.as_deref().is_some_and(|b| !aaa_common::git_watch::valid_sha(b)) {
                    bail!("기준 판은 blob SHA다");
                }
                let (bytes, staged) = match &f.change {
                    DesignMainChange::Text { text } => {
                        if text.len() as u64 > DESIGN_MAX_TEXT {
                            bail!("문서가 너무 크다({}바이트, 상한 {DESIGN_MAX_TEXT})", text.len());
                        }
                        (Some(text.as_bytes().to_vec()), false)
                    }
                    DesignMainChange::Staged { sha256 } => {
                        if !sha256_ok(sha256) {
                            bail!("sha256 형식이 아니다");
                        }
                        let ready = stage_file(state, project, repo, sha256, true);
                        let bytes = std::fs::read(&ready).context("넘긴 그림이 없다(조각을 먼저 넘긴다)")?;
                        use sha2::Digest as _;
                        if hex::encode(sha2::Sha256::digest(&bytes)) != *sha256 {
                            bail!("넘긴 그림의 sha256이 다르다");
                        }
                        staged_used.push(ready);
                        (Some(bytes), true)
                    }
                    DesignMainChange::Delete => {
                        if f.base.is_none() {
                            bail!("지우는 파일은 기준 판이 있어야 한다: {path}");
                        }
                        (None, false)
                    }
                };
                prepared.push(Prepared { path, base: f.base.clone(), bytes, staged });
            }
            let folder = folder_clean;
            let slot = Slot::take(cfg, project, repo, message).await?;
            let mut done = None;
            for _ in 0..DESIGN_MAIN_TRIES {
                let (branch, main) = slot.fresh().await?;
                let cwd = slot.cwd.as_path();
                let mut warnings = Vec::new();
                let listed = listing(cwd, &folder).await?;
                let mut changed = Vec::new();
                for p in &prepared {
                    let now = match listed.iter().find(|e| e.path == p.path) {
                        Some(e) if e.regular() => Some(e.sha.clone()),
                        Some(_) => bail!("일반 파일이 아니다: {}", p.path),
                        None => None,
                    };
                    if now.is_none()
                        && let Some(twin) = case_twin(listed.iter().map(|e| e.path.as_str()), &p.path)
                    {
                        bail!("대소문자만 다른 파일이 이미 있다: {twin}");
                    }
                    if p.staged
                        && let Some(now) = &now
                    {
                        // 이름이 내용 hash다: 같은 내용이면 이미 붙어 있다
                        let have = git(cwd, &["cat-file", "blob", now], DESIGN_MAX_FILE as usize + 1).await?;
                        if Some(&have) == p.bytes.as_ref() {
                            continue;
                        }
                    }
                    if now != p.base {
                        return Err(Stale(match now {
                            Some(now) => format!("그 사이 main에 다른 판이 생겼다: {} 지금 blob {now}", p.path),
                            None => format!("기준 판이 없다: {}이(가) main에 없다", p.path),
                        })
                        .into());
                    }
                    plain_parents(cwd, &p.path)?;
                    let target = cwd.join(&p.path);
                    match &p.bytes {
                        Some(bytes) => {
                            std::fs::create_dir_all(target.parent().context("폴더가 없다")?)?;
                            std::fs::write(&target, bytes).with_context(|| format!("쓰지 못했다: {}", p.path))?;
                        }
                        None => {
                            let found = tokio::time::timeout(
                                GIT_WAIT,
                                crate::design::image_reference_warnings(cwd, &main, &p.path),
                            )
                            .await
                            .context("그림 참조 검사 시간 상한이다")??;
                            warnings.extend(found);
                            std::fs::remove_file(&target).with_context(|| format!("지우지 못했다: {}", p.path))?;
                        }
                    }
                    changed.push(p.path.clone());
                }
                if changed.is_empty() {
                    done = Some((main.clone(), main, warnings));
                    break;
                }
                if let Some(head) = slot.finish(&changed, message, check, (&branch, &main)).await? {
                    done = Some((head, main, warnings));
                    break;
                }
            }
            let (commit, base, warnings) = done.ok_or_else(gave_up)?;
            let top = toplevel(repo).await?;
            let mut blobs = Vec::new();
            for p in &prepared {
                let blob = if p.bytes.is_some() {
                    Some(
                        String::from_utf8_lossy(
                            &git(&top, &["rev-parse", &format!("{commit}:{}", p.path)], 4096).await?,
                        )
                        .trim()
                        .to_string(),
                    )
                } else {
                    None
                };
                blobs.push(DesignMainBlob { path: p.path.clone(), blob });
            }
            Ok(AgentResponse::DesignMainSaved { commit, base, blobs, warnings })
        }
        .await,
    );
    if result.is_ok() {
        for ready in staged_used {
            aaa_common::diagnostic::ignored(std::fs::remove_file(ready), "올린 기획 조각 지우기");
        }
    }
    result
}

/// 기획서 ID 표지를 문서 여럿에 넣어 한 commit으로 main에 올린다. 이미 단 문서·지운 문서·`vendor/` 아래는 건너뛴다
pub async fn mark_ids(
    cfg: &ThreadsConfig,
    (project, repo, folder): (&str, &str, &str),
    ids: &[DesignIdMark],
    message: &str,
    check: &[String],
) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let folder = design_clean_path(folder).context("기획 폴더 경로가 잘못됐다")?;
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
            let slot = Slot::take(cfg, project, repo, message).await?;
            for _ in 0..DESIGN_MAIN_TRIES {
                let (branch, main) = slot.fresh().await?;
                let cwd = slot.cwd.as_path();
                let listed = listing(cwd, &folder).await?;
                let mut marked = Vec::new();
                for (path, id) in &picks {
                    if aaa_proto::design_id_unmarked_path(path) {
                        continue;
                    }
                    let Some(e) = listed.iter().find(|e| e.path == *path) else { continue };
                    if !e.regular() || !Path::new(path).extension().is_some_and(|x| x.eq_ignore_ascii_case("md")) {
                        continue;
                    }
                    plain_parents(cwd, path)?;
                    let target = cwd.join(path);
                    let Ok(text) = String::from_utf8(std::fs::read(&target)?) else { continue };
                    let out = aaa_proto::design_put_id(&text, id);
                    if out == text || out.len() as u64 > DESIGN_MAX_TEXT {
                        continue;
                    }
                    std::fs::write(&target, out).with_context(|| format!("문서를 쓰지 못했다: {path}"))?;
                    marked.push(path.clone());
                }
                if marked.is_empty() {
                    return Ok(AgentResponse::DesignIdsMarked { commit: None, marked });
                }
                if let Some(commit) = slot.finish(&marked, message, check, (&branch, &main)).await? {
                    return Ok(AgentResponse::DesignIdsMarked { commit: Some(commit), marked });
                }
            }
            Err(gave_up())
        }
        .await,
    )
}

/// 파일 하나의 main 이력(main이 실제로 가졌던 판만, 새것부터)
pub async fn log(repo: &str, folder: &str, path: &str, limit: u32) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let (_, path) = crate::design::inside(folder, path)?;
            let top = toplevel(repo).await?;
            if let Err(e) = fetch(&top).await {
                tracing::warn!(repo, "이력을 읽기 전에 origin을 받지 못했다(가진 ref로 읽는다): {e:#}");
            }
            let (_, main) = base(&top).await?;
            let limit = limit.clamp(1, DESIGN_LOG_MAX).to_string();
            let literal = format!(":(literal){path}");
            let out = git(
                &top,
                &[
                    "log",
                    "--first-parent",
                    "--no-color",
                    "--no-show-signature",
                    "-n",
                    &limit,
                    "--format=%H%x1f%ct%x1f%an%x1f%s%x1e",
                    &main,
                    "--",
                    &literal,
                ],
                1 << 20,
            )
            .await?;
            let mut entries = Vec::new();
            for record in String::from_utf8_lossy(&out).split('\x1e') {
                let mut f = record.trim_start_matches('\n').split('\x1f');
                let (Some(commit), Some(at), Some(author), Some(subject)) = (f.next(), f.next(), f.next(), f.next())
                else {
                    continue;
                };
                let (by, other) = match design_commit_subject(subject) {
                    Some((_, by)) => (by.to_string(), false),
                    None => (author.to_string(), true),
                };
                entries.push(DesignLogEntry {
                    commit: commit.to_string(),
                    committed_ms: at.parse::<u64>().unwrap_or(0).saturating_mul(1000),
                    subject: subject.to_string(),
                    by,
                    other,
                });
            }
            Ok(AgentResponse::DesignLog { entries })
        }
        .await,
    )
}

#[cfg(test)]
mod tests;
