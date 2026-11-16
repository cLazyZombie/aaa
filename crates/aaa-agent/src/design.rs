//! 기획 폴더 읽기(design.v1, t-1894). 저장소 main checkout의 작업 파일·index·branch는 바꾸지 않는다: origin을 받고(같은 저장소는
//! 1분에 한 번, FETCH_HEAD를 쓰지 않는다) origin 기본 branch commit의 git 객체에서 읽는다. origin이 없는 저장소는 `HEAD`다.
use aaa_common::child_process::TokioCommandExt as _;
use aaa_proto::{
    AgentResponse, B64, DESIGN_MAX_DOCS, DESIGN_MAX_FILE, DESIGN_MAX_TEXT, DesignChange, DesignDocEntry, DesignTree,
    ErrorBody, ErrorCode, design_branch, design_clean_path, design_inside, design_slug_ok, design_title,
};
use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

/// 같은 저장소를 다시 받기까지
const FETCH_EVERY: Duration = Duration::from_secs(60);
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const QUERY_TIMEOUT: Duration = Duration::from_secs(20);
/// 제목을 찾으려고 문서마다 앞에서 읽는 바이트
const TITLE_BYTES: usize = 8 * 1024;
/// 명령 출력 상한(ls-tree 등)
const OUTPUT_MAX: usize = 8 * 1024 * 1024;

/// 마지막으로 origin을 받은 때와 그때 실패한 까닭
type Fetched = (Instant, Option<String>);
/// 저장소(toplevel)마다 마지막 받기(1분 안에 다시 읽어도 같은 까닭을 싣는다)
static FETCHED: Mutex<Option<HashMap<PathBuf, Fetched>>> = Mutex::new(None);

/// 잘못된 요청(경로·commit)과 못 찾음을 나눈다
#[derive(Debug)]
pub(crate) struct Missing(pub(crate) String);
impl std::fmt::Display for Missing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Missing {}

pub(crate) fn reply(r: Result<AgentResponse>) -> Result<AgentResponse, ErrorBody> {
    r.map_err(|e| {
        let code = if e.downcast_ref::<Missing>().is_some() {
            ErrorCode::NotFound
        } else if e.downcast_ref::<Stale>().is_some() {
            ErrorCode::Conflict
        } else {
            ErrorCode::BadRequest
        };
        ErrorBody::new(code, format!("{e:#}"))
    })
}

/// 기준 판과 지금 판이 다르다(저장 겹침)
#[derive(Debug)]
pub(crate) struct Stale(pub(crate) String);
impl std::fmt::Display for Stale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Stale {}

/// 기획 폴더 트리. `branch`(slug)면 그 기획 branch 끝에서 읽고 main과 달리 고친 파일·앞선 commit 수를 싣는다
pub async fn tree(repo: &str, folder: &str, branch: Option<&str>) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let folder = design_clean_path(folder).context("기획 폴더 경로가 잘못됐다")?;
            let top = toplevel(repo).await?;
            let fetch_error = fetch(&top).await.err().map(|e| format!("{e:#}"));
            tokio::time::timeout(QUERY_TIMEOUT, async {
                let main = base(&top).await?;
                let tree = match branch {
                    None => list(&top, &folder, &main.0, &main.1, fetch_error).await?,
                    Some(slug) => {
                        let tip = branch_tip(&top, slug).await?;
                        let mut tree = list(&top, &folder, &design_branch(slug), &tip, fetch_error).await?;
                        (tree.changed, tree.ahead) = changes(&top, &folder, &main.1, &tip).await?;
                        tree
                    }
                };
                Ok(AgentResponse::DesignTree(tree))
            })
            .await
            .context("기획 폴더 읽기 시간 상한이다")?
        }
        .await,
    )
}

pub async fn read(
    repo: &str,
    folder: &str,
    commit: Option<&str>,
    path: &str,
    branch: Option<&str>,
) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let (folder, path) = inside(folder, path)?;
            let top = toplevel(repo).await?;
            tokio::time::timeout(QUERY_TIMEOUT, async {
                let commit = match (commit, branch) {
                    (Some(c), _) => checked_commit(&top, c, branch).await?,
                    (None, Some(slug)) => branch_tip(&top, slug).await?,
                    (None, None) => base(&top).await?.1,
                };
                let committed_ms = committed_ms(&top, &commit).await?;
                let (blob, size) = blob(&top, &commit, &folder, &path).await?;
                if size > DESIGN_MAX_TEXT {
                    bail!("문서가 너무 크다({size}바이트, 상한 {DESIGN_MAX_TEXT})");
                }
                let bytes = git(&top, &["cat-file", "blob", &blob], DESIGN_MAX_TEXT as usize).await?;
                Ok(AgentResponse::DesignText {
                    text: String::from_utf8_lossy(&bytes).into_owned(),
                    blob,
                    commit,
                    committed_ms,
                })
            })
            .await
            .context("기획 문서 읽기 시간 상한이다")?
        }
        .await,
    )
}

pub async fn file(
    repo: &str,
    folder: &str,
    commit: &str,
    path: &str,
    (offset, len): (u64, u32),
    branch: Option<&str>,
) -> Result<AgentResponse, ErrorBody> {
    reply(
        async {
            let (folder, path) = inside(folder, path)?;
            let top = toplevel(repo).await?;
            tokio::time::timeout(QUERY_TIMEOUT, async {
                let commit = checked_commit(&top, commit, branch).await?;
                let (blob, total) = blob(&top, &commit, &folder, &path).await?;
                if total > DESIGN_MAX_FILE {
                    bail!("파일이 너무 크다({total}바이트, 상한 {DESIGN_MAX_FILE})");
                }
                if offset > total {
                    bail!("파일 범위 밖이다");
                }
                // len 0은 크기만 묻는다(웹 Range를 풀기 전)
                let data = if len == 0 || offset == total {
                    Vec::new()
                } else {
                    blob_slice(&top, &blob, offset, len as usize).await?
                };
                // 덜 읽은 조각을 그대로 보내면 웹이 잘린 그림을 오래 둔다
                let want = u64::from(len).min(total - offset);
                if data.len() as u64 != want {
                    bail!("파일을 다 읽지 못했다({}/{want}바이트)", data.len());
                }
                Ok(AgentResponse::DesignChunk { data: B64(data), total })
            })
            .await
            .context("기획 파일 읽기 시간 상한이다")?
        }
        .await,
    )
}

pub(crate) fn inside(folder: &str, path: &str) -> Result<(String, String)> {
    let folder = design_clean_path(folder).context("기획 폴더 경로가 잘못됐다")?;
    let path = design_clean_path(path).context("문서 경로가 잘못됐다")?;
    if !design_inside(&folder, &path) {
        bail!("기획 폴더 밖 경로다");
    }
    Ok((folder, path))
}

pub(crate) async fn toplevel(repo: &str) -> Result<PathBuf> {
    let dir = Path::new(repo);
    if !dir.is_absolute() || !dir.is_dir() {
        return Err(Missing("저장소 폴더가 없다".into()).into());
    }
    let out = git(dir, &["rev-parse", "--show-toplevel"], 4096).await.context("git 저장소가 아니다")?;
    Ok(PathBuf::from(String::from_utf8_lossy(&out).trim()))
}

/// 저장소에서 git을 한 번 돌려 표준 출력을 받는다(실패면 종료 코드만 알린다. stderr에는 원격 주소가 섞일 수 있다)
pub(crate) async fn git(dir: &Path, args: &[&str], limit: usize) -> Result<Vec<u8>> {
    let mut child = command(dir, args).stdin(Stdio::null()).spawn_guarded().context("git 실행 실패")?;
    let mut stdout = child.stdout.take().context("git 출력이 없다")?.take(limit as u64 + 1);
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes).await?;
    if bytes.len() > limit {
        bail!("git {} 출력이 상한을 넘는다", args[0]);
    }
    let status = child.wait().await?;
    if !status.success() {
        bail!("git {} 실패(종료 코드 {:?})", args[0], status.code());
    }
    Ok(bytes)
}

pub(crate) fn command(dir: &Path, args: &[&str]) -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(aaa_common::diff::git_prefix())
        // 서명 표시(`log.showSignature`)가 켜진 머신에서도 `show --format` 출력이 그 꼴 하나다
        .args(["-c", "gc.auto=0", "-c", "maintenance.auto=false", "-c", "log.showSignature=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        // Windows Git Credential Manager도 창을 띄우지 않는다(origin 받기는 사람이 없는 자리에서 돈다)
        .env("GCM_INTERACTIVE", "never")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    cmd
}

pub(crate) async fn has_origin(top: &Path) -> Result<bool> {
    Ok(String::from_utf8_lossy(&git(top, &["remote"], 64 * 1024).await?).lines().any(|l| l.trim() == "origin"))
}

/// origin을 받는다(1분에 한 번). 실패해도 가진 ref로 읽는다
pub(crate) async fn fetch(top: &Path) -> Result<()> {
    if !has_origin(top).await? {
        return Ok(());
    }
    {
        let mut guard = FETCHED.lock().unwrap();
        let seen = guard.get_or_insert_with(HashMap::new);
        if let Some((_, last)) = seen.get(top).filter(|(at, _)| at.elapsed() < FETCH_EVERY) {
            return last.clone().map_or(Ok(()), |e| Err(anyhow!(e)));
        }
        // 실패해도 1분 안에는 다시 받지 않는다(끊긴 원격에 매번 기다리지 않게)
        seen.insert(top.to_path_buf(), (Instant::now(), None));
    }
    let result = fetch_now(top).await;
    if let Err(e) = &result
        && let Some(entry) = FETCHED.lock().unwrap().as_mut().and_then(|m| m.get_mut(top))
    {
        entry.1 = Some(format!("{e:#}"));
    }
    result
}

async fn fetch_now(top: &Path) -> Result<()> {
    tokio::time::timeout(
        FETCH_TIMEOUT,
        git(
            top,
            &["fetch", "--quiet", "--no-tags", "--no-write-fetch-head", "--recurse-submodules=no", "origin"],
            4096,
        ),
    )
    .await
    .map_err(|_| anyhow!("origin을 받는 시간이 상한을 넘었다"))?
    .map(|_| ())
    .context("origin을 받지 못했다")
}

/// 읽을 ref와 그 commit: origin 기본 branch(`origin/HEAD`가 가리키는 것, 없으면 `origin/main`·`origin/master`), origin이 없으면 `HEAD`
pub(crate) async fn base(top: &Path) -> Result<(String, String)> {
    // 줄인 이름(`origin/main`)은 같은 이름의 로컬 branch(`refs/heads/origin/main`)에 가려지므로 온전한 ref로 읽는다
    let mut refs = Vec::new();
    if has_origin(top).await? {
        if let Ok(out) = git(top, &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"], 4096).await {
            let target = String::from_utf8_lossy(&out).trim().to_string();
            if target.starts_with("refs/remotes/origin/") && target != "refs/remotes/origin/HEAD" {
                refs.push(target);
            }
        }
        refs.extend(["refs/remotes/origin/main".to_string(), "refs/remotes/origin/master".to_string()]);
    } else {
        refs.push("HEAD".to_string());
    }
    for full in refs {
        let spec = format!("{full}^{{commit}}");
        if let Ok(out) = git(top, &["rev-parse", "--verify", "--quiet", "--end-of-options", &spec], 4096).await {
            let sha = String::from_utf8_lossy(&out).trim().to_string();
            if aaa_common::git_watch::valid_sha(&sha) {
                return Ok((full.strip_prefix("refs/remotes/").unwrap_or(&full).to_string(), sha));
            }
        }
    }
    Err(Missing("읽을 기본 branch가 없다(origin/HEAD·origin/main·origin/master)".into()).into())
}

/// 받은 commit은 읽는 기본 branch의 판(그 조상)이어야 한다. `branch`를 주면 그 기획 branch의 판도 된다. 다른 branch·올리지 않은
/// commit·stash의 기획 폴더는 읽지 않는다
async fn checked_commit(top: &Path, commit: &str, branch: Option<&str>) -> Result<String> {
    if !aaa_common::git_watch::valid_sha(commit) {
        bail!("commit 형식이 아니다");
    }
    let spec = format!("{commit}^{{commit}}");
    let out = git(top, &["rev-parse", "--verify", "--quiet", "--end-of-options", &spec], 4096)
        .await
        .map_err(|_| Missing("그 commit이 저장소에 없다".into()))?;
    let commit = String::from_utf8_lossy(&out).trim().to_string();
    let (name, tip) = base(top).await?;
    if git(top, &["merge-base", "--is-ancestor", &commit, &tip], 64).await.is_ok() {
        return Ok(commit);
    }
    if let Some(slug) = branch
        && let Ok(branch_tip) = branch_tip(top, slug).await
        && git(top, &["merge-base", "--is-ancestor", &commit, &branch_tip], 64).await.is_ok()
    {
        return Ok(commit);
    }
    Err(Missing(format!("그 commit은 {name}의 판이 아니다")).into())
}

pub(crate) async fn committed_ms(top: &Path, commit: &str) -> Result<u64> {
    let out = git(top, &["show", "-s", "--format=%ct", commit], 4096).await?;
    let secs: u64 = String::from_utf8_lossy(&out).trim().parse().context("commit 시각을 읽지 못했다")?;
    Ok(secs * 1000)
}

/// 기획 폴더 안 일반 파일의 blob과 크기(symlink·폴더·submodule은 없는 것으로 본다)
async fn blob(top: &Path, commit: &str, folder: &str, path: &str) -> Result<(String, u64)> {
    debug_assert!(design_inside(folder, path));
    let out = git(top, &["ls-tree", "-z", "-l", "--full-tree", commit, "--", path], 64 * 1024).await?;
    let entry = entries(&out)?.into_iter().find(|e| e.path == path);
    let Some(e) = entry.filter(|e| e.regular()) else {
        return Err(Missing(format!("기획 폴더에 없다: {path}")).into());
    };
    Ok((e.sha, e.size))
}

pub(crate) struct Entry {
    mode: String,
    kind: String,
    pub(crate) sha: String,
    size: u64,
    pub(crate) path: String,
}

impl Entry {
    pub(crate) fn regular(&self) -> bool {
        self.kind == "blob" && matches!(self.mode.as_str(), "100644" | "100755")
    }
}

/// `ls-tree -z -l` 출력: `<mode> <type> <object> <size>\t<path>\0`
pub(crate) fn entries(out: &[u8]) -> Result<Vec<Entry>> {
    let mut list = Vec::new();
    for raw in out.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let line = String::from_utf8_lossy(raw);
        let (meta, path) = line.split_once('\t').context("ls-tree 줄 형식이 아니다")?;
        let mut parts = meta.split_whitespace();
        let (Some(mode), Some(kind), Some(sha), Some(size)) = (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            bail!("ls-tree 줄 형식이 아니다");
        };
        list.push(Entry {
            mode: mode.into(),
            kind: kind.into(),
            sha: sha.into(),
            size: size.parse().unwrap_or(0),
            path: path.into(),
        });
    }
    Ok(list)
}

fn is_markdown(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

async fn list(top: &Path, folder: &str, base: &str, commit: &str, fetch_error: Option<String>) -> Result<DesignTree> {
    let (base, commit) = (base.to_string(), commit.to_string());
    let committed_ms = committed_ms(top, &commit).await?;
    let out = git(top, &["ls-tree", "-r", "-z", "-l", "--full-tree", &commit, "--", folder], OUTPUT_MAX).await?;
    let mut docs: Vec<Entry> = entries(&out)?
        .into_iter()
        .filter(|e| e.regular() && is_markdown(&e.path) && design_inside(folder, &e.path))
        .collect();
    docs.sort_by(|a, b| a.path.cmp(&b.path));
    let truncated = docs.len() > DESIGN_MAX_DOCS;
    docs.truncate(DESIGN_MAX_DOCS);
    let titles = titles(top, &docs).await?;
    let docs =
        docs.into_iter().zip(titles).map(|(e, title)| DesignDocEntry { path: e.path, title, bytes: e.size }).collect();
    Ok(DesignTree { commit, committed_ms, base, docs, truncated, fetch_error, ..Default::default() })
}

/// 기획 branch 끝(로컬 `design/<slug>`, 없으면 origin의 같은 branch)
pub(crate) async fn branch_tip(top: &Path, slug: &str) -> Result<String> {
    if !design_slug_ok(slug) {
        bail!("기획 branch 이름이 아니다");
    }
    let branch = design_branch(slug);
    for full in [format!("refs/heads/{branch}"), format!("refs/remotes/origin/{branch}")] {
        let spec = format!("{full}^{{commit}}");
        if let Ok(out) = git(top, &["rev-parse", "--verify", "--quiet", "--end-of-options", &spec], 4096).await {
            let sha = String::from_utf8_lossy(&out).trim().to_string();
            if aaa_common::git_watch::valid_sha(&sha) {
                return Ok(sha);
            }
        }
    }
    Err(Missing(format!("기획 branch가 없다: {branch}")).into())
}

/// main 끝과 갈라진 곳부터 기획 branch 끝까지 기획 폴더에서 바뀐 파일(이름 바꿈은 지움+더함)과 main보다 앞선 commit 수
async fn changes(top: &Path, folder: &str, main: &str, tip: &str) -> Result<(Vec<DesignChange>, u32)> {
    let fork = String::from_utf8_lossy(&git(top, &["merge-base", main, tip], 4096).await?).trim().to_string();
    let out = git(top, &["diff", "--name-status", "-z", "--no-renames", &fork, tip, "--", folder], OUTPUT_MAX).await?;
    let mut parts = out.split(|b| *b == 0).filter(|p| !p.is_empty());
    let mut changed = Vec::new();
    while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
        let path = String::from_utf8_lossy(path).into_owned();
        if design_inside(folder, &path) {
            changed.push(DesignChange { path, status: String::from_utf8_lossy(status).into_owned() });
        }
    }
    let ahead = String::from_utf8_lossy(&git(top, &["rev-list", "--count", &format!("{main}..{tip}")], 4096).await?)
        .trim()
        .parse()
        .unwrap_or(0);
    Ok((changed, ahead))
}

/// 문서마다 앞부분만 읽어 첫 `# ` 제목을 찾는다(`cat-file --batch` 한 번)
async fn titles(top: &Path, docs: &[Entry]) -> Result<Vec<String>> {
    if docs.is_empty() {
        return Ok(Vec::new());
    }
    let mut child =
        command(top, &["cat-file", "--batch"]).stdin(Stdio::piped()).spawn_guarded().context("git 실행 실패")?;
    let input: String = docs.iter().map(|e| format!("{}\n", e.sha)).collect();
    let mut stdin = child.stdin.take().context("git 입력이 없다")?;
    let writer = tokio::spawn(async move {
        let r = stdin.write_all(input.as_bytes()).await;
        drop(stdin);
        r
    });
    let mut out = BufReader::new(child.stdout.take().context("git 출력이 없다")?);
    let mut titles = Vec::with_capacity(docs.len());
    for e in docs {
        let mut header = String::new();
        out.read_line(&mut header).await?;
        let mut parts = header.split_whitespace();
        let (Some(sha), Some(_kind), Some(size)) = (parts.next(), parts.next(), parts.next()) else {
            bail!("cat-file 머리 형식이 아니다");
        };
        if sha != e.sha {
            bail!("cat-file 차례가 다르다");
        }
        let size: u64 = size.parse().context("cat-file 크기")?;
        let keep = size.min(TITLE_BYTES as u64) as usize;
        let mut head = vec![0; keep];
        out.read_exact(&mut head).await?;
        // 나머지와 끝 줄바꿈은 버린다
        tokio::io::copy(&mut (&mut out).take(size - keep as u64 + 1), &mut tokio::io::sink()).await?;
        titles.push(design_title(&String::from_utf8_lossy(&head)));
    }
    writer.await.context("git 입력 작업")?.context("git 입력")?;
    let status = child.wait().await?;
    if !status.success() {
        bail!("git cat-file 실패(종료 코드 {:?})", status.code());
    }
    Ok(titles)
}

async fn blob_slice(top: &Path, blob: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
    let mut child =
        command(top, &["cat-file", "blob", blob]).stdin(Stdio::null()).spawn_guarded().context("git 실행 실패")?;
    let mut out = child.stdout.take().context("git 출력이 없다")?;
    tokio::io::copy(&mut (&mut out).take(offset), &mut tokio::io::sink()).await?;
    let mut data = Vec::with_capacity(len);
    (&mut out).take(len as u64).read_to_end(&mut data).await?;
    drop(out);
    // 앞부분만 읽고 끝낸다(남은 출력은 버린다)
    aaa_common::diagnostic::ignored(child.kill().await, "읽을 만큼 읽었다");
    Ok(data)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) async fn sh(dir: &Path, args: &[&str]) {
        let st = tokio::process::Command::new("git")
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "init.defaultBranch=main"])
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    /// 시험 폴더(끝나면 지운다)
    pub(crate) struct Lab(PathBuf);
    impl Lab {
        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Lab {
        fn drop(&mut self) {
            aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&self.0), "시험 폴더 정리");
        }
    }

    /// git 출력 한 줄(`input`은 표준 입력)
    pub(crate) async fn out(dir: &Path, args: &[&str], input: &[u8]) -> String {
        let mut child = tokio::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).await.unwrap();
        let o = child.wait_with_output().await.unwrap();
        assert!(o.status.success(), "git {args:?}");
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    }

    /// origin(bare)에 기획 폴더를 올리고, 그 clone의 작업 파일은 옛 판으로 둔다
    pub(crate) async fn lab() -> (Lab, PathBuf) {
        let t = Lab(std::env::temp_dir().join(format!("aaa-design-{}", aaa_common::util::new_id())));
        let origin = t.path().join("origin.git");
        let work = t.path().join("work");
        let clone = t.path().join("clone");
        std::fs::create_dir_all(&work).unwrap();
        sh(t.path(), &["init", "--bare", "-q", origin.to_str().unwrap()]).await;
        sh(t.path(), &["init", "-q", work.to_str().unwrap()]).await;
        std::fs::create_dir_all(work.join("docs/game-loop/media")).unwrap();
        std::fs::write(work.join("docs/overview.md"), "# 개요\n").unwrap();
        std::fs::write(work.join("README.md"), "# 밖\n").unwrap();
        sh(&work, &["add", "."]).await;
        sh(&work, &["commit", "-q", "-m", "a"]).await;
        sh(&work, &["remote", "add", "origin", origin.to_str().unwrap()]).await;
        sh(&work, &["push", "-q", "origin", "HEAD:main"]).await;
        sh(t.path(), &["clone", "-q", origin.to_str().unwrap(), clone.to_str().unwrap()]).await;
        std::fs::write(work.join("docs/game-loop/README.md"), "---\nx: 1\n---\n# 게임 루프\n\n![그림](media/a.png)\n")
            .unwrap();
        std::fs::write(work.join("docs/game-loop/media/a.png"), b"\x89PNG0123456789").unwrap();
        sh(&work, &["add", "."]).await;
        sh(&work, &["commit", "-q", "-m", "b"]).await;
        sh(&work, &["push", "-q", "origin", "HEAD:main"]).await;
        (t, clone)
    }

    #[tokio::test]
    async fn tree_reads_origin_main_without_touching_checkout() {
        let (_t, clone) = lab().await;
        let repo = clone.to_str().unwrap();
        let head_before = git(&clone, &["rev-parse", "HEAD"], 4096).await.unwrap();
        // 같은 이름의 로컬 branch(refs/heads/origin/main, 옛 판)가 origin의 판을 가리지 않는다
        sh(&clone, &["branch", "origin/main", "HEAD"]).await;
        let Ok(AgentResponse::DesignTree(t)) = tree(repo, "docs", None).await else { panic!("트리") };
        assert_eq!(t.base, "origin/main");
        assert_eq!(t.fetch_error, None);
        assert!(!clone.join(".git/FETCH_HEAD").exists(), "FETCH_HEAD를 쓰지 않는다");
        let got: Vec<_> = t.docs.iter().map(|d| (d.path.as_str(), d.title.as_str())).collect();
        assert_eq!(
            got,
            [("docs/game-loop/README.md", "게임 루프"), ("docs/overview.md", "개요")],
            "받은 origin의 새 문서, 밖 README와 그림은 뺀다"
        );
        assert_eq!(git(&clone, &["rev-parse", "HEAD"], 4096).await.unwrap(), head_before, "checkout HEAD는 그대로");
        assert!(!clone.join("docs/game-loop").exists(), "작업 파일은 그대로");

        let Ok(AgentResponse::DesignText { text, commit, .. }) =
            read(repo, "docs", Some(&t.commit), "docs/game-loop/README.md", None).await
        else {
            panic!("읽기")
        };
        assert!(text.contains("# 게임 루프") && commit == t.commit);
        let Ok(AgentResponse::DesignChunk { data, total }) =
            file(repo, "docs", &t.commit, "docs/game-loop/media/a.png", (4, 3), None).await
        else {
            panic!("파일")
        };
        assert_eq!((data.0.as_slice(), total), (&b"012"[..], 14));
    }

    /// origin이 앞서 가도 문서·그림은 트리를 읽은 commit으로 읽고, 고르지 않으면 새 기본 branch다
    #[tokio::test]
    async fn pinned_commit_reads_the_old_text_after_origin_moves() {
        let (t, clone) = lab().await;
        let repo = clone.to_str().unwrap();
        let Ok(AgentResponse::DesignTree(old)) = tree(repo, "docs", None).await else { panic!("트리") };
        let work = t.path().join("work");
        std::fs::write(work.join("docs/overview.md"), "# 개요 둘\n").unwrap();
        sh(&work, &["commit", "-q", "-am", "c"]).await;
        sh(&work, &["push", "-q", "origin", "HEAD:main"]).await;
        // 1분 받기 간격과 별개로 clone이 새 판을 가진 상태
        sh(&clone, &["fetch", "-q", "origin"]).await;
        let Ok(AgentResponse::DesignText { text, commit, .. }) =
            read(repo, "docs", Some(&old.commit), "docs/overview.md", None).await
        else {
            panic!("옛 판")
        };
        assert_eq!((text.as_str(), commit.as_str()), ("# 개요\n", old.commit.as_str()));
        let Ok(AgentResponse::DesignText { text, commit, .. }) =
            read(repo, "docs", None, "docs/overview.md", None).await
        else {
            panic!("새 판")
        };
        assert_eq!(text, "# 개요 둘\n");
        assert_ne!(commit, old.commit);
        // 기본 branch에 없는 commit(올리지 않은 로컬 commit)의 기획 폴더는 읽지 않는다
        std::fs::write(clone.join("docs/overview.md"), "# 비밀 초안\n").unwrap();
        sh(&clone, &["commit", "-q", "-am", "local"]).await;
        let local = out(&clone, &["rev-parse", "HEAD"], b"").await;
        let e = read(repo, "docs", Some(&local), "docs/overview.md", None).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{e}");
        let e = file(repo, "docs", &local, "docs/overview.md", (0, 1), None).await.unwrap_err();
        assert!(e.message.contains("origin/main의 판이 아니다"), "{e}");
    }

    /// origin을 받지 못하면 가진 origin/main으로 읽고 까닭을 함께 보인다
    #[tokio::test]
    async fn fetch_failure_keeps_the_held_tree() {
        let (t, clone) = lab().await;
        let gone = t.path().join("gone.git");
        sh(&clone, &["remote", "set-url", "origin", gone.to_str().unwrap()]).await;
        let Ok(AgentResponse::DesignTree(tree)) = tree(clone.to_str().unwrap(), "docs", None).await else {
            panic!("트리")
        };
        assert!(
            tree.fetch_error.as_deref().is_some_and(|e| e.contains("origin을 받지 못했다")),
            "{:?}",
            tree.fetch_error
        );
        let got: Vec<_> = tree.docs.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(got, ["docs/overview.md"], "clone이 가진 첫 판");
        // 1분 안에 다시 읽으면 받지 않지만 실패 까닭은 그대로 싣는다
        let Ok(AgentResponse::DesignTree(again)) = super::tree(clone.to_str().unwrap(), "docs", None).await else {
            panic!("다시 트리")
        };
        assert_eq!(again.fetch_error, tree.fetch_error);
    }

    /// symlink·submodule은 목록에도 없고 읽지도 않는다(폴더 밖을 가리킬 수 있다). 조각 범위와 글 크기 상한
    #[tokio::test]
    async fn links_are_not_documents_and_slices_stay_in_bounds() {
        let (t, clone) = lab().await;
        let work = t.path().join("work");
        // OS의 symlink 지원과 관계없이 index에 직접 넣는다
        let link = out(&work, &["hash-object", "-w", "--stdin"], b"../README.md").await;
        let head = out(&work, &["rev-parse", "HEAD"], b"").await;
        sh(&work, &["update-index", "--add", "--cacheinfo", &format!("120000,{link},docs/link.md")]).await;
        sh(&work, &["update-index", "--add", "--cacheinfo", &format!("160000,{head},docs/sub.md")]).await;
        std::fs::write(work.join("docs/big.md"), vec![b'a'; 512 * 1024 + 1]).unwrap();
        sh(&work, &["add", "docs/big.md"]).await;
        sh(&work, &["commit", "-q", "-m", "links"]).await;
        sh(&work, &["push", "-q", "origin", "HEAD:main"]).await;
        let repo = clone.to_str().unwrap();
        let Ok(AgentResponse::DesignTree(tree)) = tree(repo, "docs", None).await else { panic!("트리") };
        let got: Vec<_> = tree.docs.iter().map(|d| d.path.as_str()).collect();
        assert_eq!(got, ["docs/big.md", "docs/game-loop/README.md", "docs/overview.md"]);
        for path in ["docs/link.md", "docs/sub.md"] {
            let e = read(repo, "docs", Some(&tree.commit), path, None).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::NotFound, "{path}: {e}");
        }
        let e = read(repo, "docs", Some(&tree.commit), "docs/big.md", None).await.unwrap_err();
        assert!(e.message.contains("문서가 너무 크다"), "{e}");
        let png = "docs/game-loop/media/a.png";
        let Ok(AgentResponse::DesignChunk { data, total }) = file(repo, "docs", &tree.commit, png, (0, 0), None).await
        else {
            panic!("크기만")
        };
        assert_eq!((data.0.len(), total), (0, 14));
        let Ok(AgentResponse::DesignChunk { data, .. }) = file(repo, "docs", &tree.commit, png, (10, 100), None).await
        else {
            panic!("끝 조각")
        };
        assert_eq!(data.0, b"6789");
        let Ok(AgentResponse::DesignChunk { data, .. }) = file(repo, "docs", &tree.commit, png, (14, 10), None).await
        else {
            panic!("끝")
        };
        assert!(data.0.is_empty());
        let e = file(repo, "docs", &tree.commit, png, (15, 1), None).await.unwrap_err();
        assert!(e.message.contains("범위 밖"), "{e}");
    }

    #[tokio::test]
    async fn read_refuses_outside_and_missing() {
        let (_t, clone) = lab().await;
        let repo = clone.to_str().unwrap();
        let e = read(repo, "docs", None, "README.md", None).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{e}");
        let e = read(repo, "docs", None, "docs/../README.md", None).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{e}");
        let e = read(repo, "docs", None, "docs/none.md", None).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{e}");
        let e = file(repo, "docs", "zz", "docs/overview.md", (0, 1), None).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{e}");
        let e = tree("/no/such/repo", "docs", None).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{e}");
    }
}
