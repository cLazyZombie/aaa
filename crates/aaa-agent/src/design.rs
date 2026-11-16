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
/// 표지 세기(트리 읽기 안). 넘으면 수를 모른다고 알리고 트리는 보인다
const MARKS_TIMEOUT: Duration = Duration::from_secs(5);
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
                    None => list(&top, &folder, &main.0, &main.1, fetch_error, true).await?,
                    Some(slug) => {
                        // branch 트리는 host가 고친 파일·앞선 수만 쓴다: 표지는 세지 않는다
                        let tip = branch_tip(&top, slug).await?;
                        let mut tree = list(&top, &folder, &design_branch(slug), &tip, fetch_error, false).await?;
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

/// `git` 출력과 종료 코드(0이 아니어도 실패로 보지 않는다: `git grep`의 1은 「맞는 것 없음」이다)
async fn git_status(dir: &Path, args: &[&str], limit: usize) -> Result<(Vec<u8>, Option<i32>)> {
    let mut child = command(dir, args).stdin(Stdio::null()).spawn_guarded().context("git 실행 실패")?;
    let mut stdout = child.stdout.take().context("git 출력이 없다")?.take(limit as u64 + 1);
    let mut bytes = Vec::new();
    stdout.read_to_end(&mut bytes).await?;
    if bytes.len() > limit {
        bail!("git {} 출력이 상한을 넘는다", args[0]);
    }
    Ok((bytes, child.wait().await?.code()))
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

async fn list(
    top: &Path,
    folder: &str,
    base: &str,
    commit: &str,
    fetch_error: Option<String>,
    count: bool,
) -> Result<DesignTree> {
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
    // 표지 세기는 따로 짧은 시한이다: 못 세면 수를 모른다고 알리고 트리는 그대로 보인다
    let counted = if count { tokio::time::timeout(MARKS_TIMEOUT, marks(top, &commit, folder, &docs)).await.ok().flatten() } else { None };
    let marked = counted.is_some();
    let mut marks = counted.unwrap_or_default();
    let docs = docs
        .into_iter()
        .zip(titles)
        .map(|(e, title)| {
            let Marks { plans, open, tasks } = marks.remove(&e.path).unwrap_or_default();
            DesignDocEntry { path: e.path, title, bytes: e.size, plans, open, tasks }
        })
        .collect();
    Ok(DesignTree { commit, committed_ms, base, docs, truncated, fetch_error, marks: marked, ..Default::default() })
}

/// 문서 하나의 블록 표지 수(계획·미정)와 넘긴 계획의 d-ID(나온 차례)
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Marks {
    pub(crate) plans: u32,
    pub(crate) open: u32,
    pub(crate) tasks: Vec<String>,
}

/// 표지 글자가 든 문서만 `git grep -l`로 골라 그 글을 읽는 대로 `count_marks`로 센다. 못 읽으면 None(수를 모름)
async fn marks(top: &Path, commit: &str, folder: &str, docs: &[Entry]) -> Option<HashMap<String, Marks>> {
    // 고정 글자로 거른다(locale·정규식 엔진과 무관). `color.grep` 설정이 파일 이름을 칠하지 않게 한다.
    // 맞는 문서가 없으면 git grep은 1로 끝난다: 그 출력은 비어 있다
    let args = ["grep", "--no-color", "-l", "-z", "-I", "-F", "-e", "[!계획]", "-e", "[!미정]", commit, "--", folder];
    let raw = match git_status(top, &args, OUTPUT_MAX).await {
        Ok((raw, Some(0 | 1))) => raw,
        _ => return None,
    };
    let prefix = format!("{commit}:");
    let hits: std::collections::HashSet<&str> = raw
        .split(|b| *b == 0)
        .filter_map(|p| std::str::from_utf8(p).ok())
        .map(|p| p.strip_prefix(&prefix).unwrap_or(p))
        .collect();
    let docs: Vec<&Entry> = docs.iter().filter(|e| hits.contains(e.path.as_str())).collect();
    let shas: Vec<&str> = docs.iter().map(|e| e.sha.as_str()).collect();
    let mut out = HashMap::new();
    each_head(top, &shas, DESIGN_MAX_TEXT as usize, |i, text| {
        out.insert(docs[i].path.clone(), count_marks(&String::from_utf8_lossy(text)));
    })
    .await
    .ok()?;
    Some(out)
}

/// `d-` 뒤 숫자의 정규형 할 일 번호(웹 `canonicalShortId`와 같다: 앞 0을 빼고 네 자리 이상, 0이나 i64를 넘으면 번호가 아님)
fn task_id(digits: &str) -> Option<String> {
    let n = digits.chars().take_while(char::is_ascii_digit).count();
    let value = digits[..n].trim_start_matches('0');
    if value.is_empty() || value.len() > 19 || (value.len() == 19 && value > "9223372036854775807") {
        return None;
    }
    Some(format!("d-{value:0>4}"))
}

/// 웹(`designModel.ts`의 `mdBlocks`·`calloutOf`)과 같은 규칙으로 센다: 빈 줄·제목·코드 울타리로 나눈 덩어리 중
/// 모든 줄이 인용(`>`)이고 첫 줄이 `> [!계획]`·`> [!미정]`인 것만 표지다. 울타리 안의 표기 예시는 세지 않는다
pub(crate) fn count_marks(text: &str) -> Marks {
    fn fence_of(line: &str) -> Option<(char, usize)> {
        let t = line.trim_start();
        let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
        let n = t.chars().take_while(|x| *x == c).count();
        (n >= 3).then_some((c, n))
    }
    fn heading(line: &str) -> bool {
        let n = line.chars().take_while(|c| *c == '#').count();
        (1..=6).contains(&n) && line[n..].chars().next().is_some_and(char::is_whitespace)
    }
    fn tally(block: &[&str], m: &mut Marks) {
        let Some(first) = block.first() else { return };
        if !block.iter().all(|l| l.trim_start().starts_with('>')) {
            return;
        }
        let head = first.trim_start()[1..].trim_start();
        if let Some(rest) = head.strip_prefix("[!계획]") {
            m.plans += 1;
            if let Some(id) = rest.trim_start().strip_prefix("d-").and_then(task_id) {
                m.tasks.push(id);
            }
        } else if head.starts_with("[!미정]") {
            m.open += 1;
        }
    }
    let mut m = Marks::default();
    let mut block: Vec<&str> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    // CRLF 문서도 같은 덩어리다(웹 `mdBlocks`는 `\r?\n`으로 나눈다)
    for line in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)) {
        let mark = fence_of(line);
        if let Some((c, n)) = fence {
            // 같은 글자로 연 것보다 짧지 않은 울타리만 닫는다
            if let Some((c2, n2)) = mark
                && c2 == c
                && n2 >= n
                && line.trim().chars().all(|x| x == c)
            {
                fence = None;
            }
            continue;
        }
        if mark.is_some() || line.trim().is_empty() || heading(line) {
            tally(&block, &mut m);
            block.clear();
            fence = mark;
            continue;
        }
        block.push(line);
    }
    tally(&block, &mut m);
    m
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
            changed.push(DesignChange {
                path,
                status: String::from_utf8_lossy(status).into_owned(),
                ..Default::default()
            });
        }
    }
    // 줄 수(`더함\t뺌\t경로\0`, 바이너리는 `-`)
    let counts = git(top, &["diff", "--numstat", "-z", "--no-renames", &fork, tip, "--", folder], OUTPUT_MAX).await?;
    for entry in counts.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let line = String::from_utf8_lossy(entry);
        let mut f = line.splitn(3, '\t');
        let (Some(add), Some(del), Some(path)) = (f.next(), f.next(), f.next()) else { continue };
        if let Some(c) = changed.iter_mut().find(|c| c.path == path) {
            c.added = add.parse().ok();
            c.removed = del.parse().ok();
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
    let shas: Vec<&str> = docs.iter().map(|e| e.sha.as_str()).collect();
    let mut titles = Vec::with_capacity(docs.len());
    each_head(top, &shas, TITLE_BYTES, |_, head| titles.push(design_title(&String::from_utf8_lossy(head)))).await?;
    Ok(titles)
}

/// blob 여럿의 앞 `keep`바이트를 준 차례대로 하나씩 넘긴다(`cat-file --batch` 한 번, 한 번에 하나만 쥔다)
async fn each_head(top: &Path, shas: &[&str], keep: usize, mut f: impl FnMut(usize, &[u8])) -> Result<()> {
    if shas.is_empty() {
        return Ok(());
    }
    let mut child =
        command(top, &["cat-file", "--batch"]).stdin(Stdio::piped()).spawn_guarded().context("git 실행 실패")?;
    let input: String = shas.iter().map(|sha| format!("{sha}\n")).collect();
    let mut stdin = child.stdin.take().context("git 입력이 없다")?;
    let writer = tokio::spawn(async move {
        let r = stdin.write_all(input.as_bytes()).await;
        drop(stdin);
        r
    });
    let mut out = BufReader::new(child.stdout.take().context("git 출력이 없다")?);
    for (i, want) in shas.iter().enumerate() {
        let mut header = String::new();
        out.read_line(&mut header).await?;
        let mut parts = header.split_whitespace();
        let (Some(sha), Some(_kind), Some(size)) = (parts.next(), parts.next(), parts.next()) else {
            bail!("cat-file 머리 형식이 아니다");
        };
        if sha != *want {
            bail!("cat-file 차례가 다르다");
        }
        let size: u64 = size.parse().context("cat-file 크기")?;
        let take = size.min(keep as u64) as usize;
        let mut head = vec![0; take];
        out.read_exact(&mut head).await?;
        // 나머지와 끝 줄바꿈은 버린다
        tokio::io::copy(&mut (&mut out).take(size - take as u64 + 1), &mut tokio::io::sink()).await?;
        f(i, &head);
    }
    writer.await.context("git 입력 작업")?.context("git 입력")?;
    let status = child.wait().await?;
    if !status.success() {
        bail!("git cat-file 실패(종료 코드 {:?})", status.code());
    }
    Ok(())
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
        assert!(t.marks, "표지 글자가 없는 트리(git grep 1)도 센 것이다");
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

    /// 표지는 웹과 같은 덩어리 규칙으로 센다: 덩어리 첫 줄의 인용 표지만, 코드 울타리 안 예시와 인용 중간 줄은 뺀다
    #[test]
    fn marks_count_only_callout_blocks_like_the_web() {
        let text = "# 게임 루프\n\n본문 d-0007은 칩일 뿐 표지가 아니다.\n\n> [!계획]\n> 100층까지 늘린다.\n\n\
                    > [!계획] d-0012\n> 처음 1칸.\n## 절\n>[!계획]d-0013 붙여 써도 같다\n\n> [!미정]\n> 확률\n\n\
                    > 앞 인용\n> [!계획] 중간 줄은 표지가 아니다\n\n> [!계획]\n섞인 덩어리는 표지가 아니다\n\n\
                    ````md\n> [!계획] d-0099\n```\n> [!미정] 짧은 울타리는 닫지 않는다\n````\n\n\
                    ~~~\n> [!미정]\n~~~\n> [!계획] d-0014x\n";
        assert_eq!(
            count_marks(text),
            Marks { plans: 4, open: 1, tasks: vec!["d-0012".into(), "d-0013".into(), "d-0014".into()] }
        );
        assert_eq!(count_marks("> [!계획]"), Marks { plans: 1, open: 0, tasks: vec![] }, "끝 줄바꿈 없음");
        assert_eq!(count_marks("> [!계획] d-\n> 번호 없는 d-"), Marks { plans: 1, open: 0, tasks: vec![] });
        assert_eq!(count_marks("####### [!계획]\n> [!미정]\n"), Marks::default(), "제목이 아닌 줄과 이어진 덩어리");
    }

    /// 웹과 같은 공용 견본(`web/test/fixtures/design-marks.md`)으로 센다. CRLF로 바꾼 판도 같다
    #[test]
    fn marks_match_the_shared_web_fixture() {
        let raw = include_str!("../../../web/test/fixtures/design-marks.md").replace("\r\n", "\n");
        let expect = raw.split("expect ").nth(1).and_then(|s| s.split(" -->").next()).expect("견본 기대값");
        let field = |k: &str| expect.split(' ').find_map(|p| p.strip_prefix(k)).expect("기대 칸");
        let want = Marks {
            plans: field("plans=").parse().unwrap(),
            open: field("open=").parse().unwrap(),
            tasks: field("tasks=").split(',').filter(|s| !s.is_empty()).map(String::from).collect(),
        };
        assert_eq!(count_marks(&raw), want);
        assert_eq!(count_marks(&raw.replace('\n', "\r\n")), want, "CRLF");
        assert_eq!(task_id("00012x"), Some("d-0012".into()));
        assert_eq!(task_id("12345"), Some("d-12345".into()));
        assert_eq!(task_id("0"), None);
        assert_eq!(task_id("9223372036854775808"), None, "i64를 넘으면 번호가 아니다");
        assert_eq!(task_id("9223372036854775807"), Some("d-9223372036854775807".into()));
    }

    /// 트리의 문서마다 표지 수·넘긴 d-ID를 붙인다(표지 없는 문서와 기획 폴더 밖은 0)
    #[tokio::test]
    async fn tree_counts_marks_per_document() {
        let (t, clone) = lab().await;
        // origin에 올리는 쪽은 lab의 작업 저장소다(clone은 옛 판에 머문다)
        let work = t.path().join("work");
        std::fs::create_dir_all(work.join("docs/bosses")).unwrap();
        std::fs::write(work.join("docs/bosses/README.md"), "# 보스\n\n> [!계획] d-0003\n> 해골 왕\n\n> [!미정]\n> 3페이즈\n")
            .unwrap();
        std::fs::write(work.join("docs/notation.md"), "# 표기\n\n```\n> [!계획] d-0001\n```\n").unwrap();
        std::fs::write(work.join("README.md"), "# 밖\n\n> [!계획]\n> 밖은 세지 않는다\n").unwrap();
        sh(&work, &["add", "."]).await;
        sh(&work, &["commit", "-q", "-m", "표지"]).await;
        sh(&work, &["push", "-q", "origin", "HEAD:main"]).await;
        // 머신의 `color.grep` 설정이 파일 이름을 칠해도 센다
        sh(&clone, &["config", "color.grep", "always"]).await;
        let repo = clone.to_str().unwrap();
        let Ok(AgentResponse::DesignTree(t)) = tree(repo, "docs", None).await else { panic!("트리") };
        assert!(t.marks, "셌다");
        let got: Vec<_> = t.docs.iter().map(|d| (d.path.as_str(), d.plans, d.open, d.tasks.clone())).collect();
        assert_eq!(
            got,
            [
                ("docs/bosses/README.md", 1, 1, vec!["d-0003".to_string()]),
                ("docs/game-loop/README.md", 0, 0, vec![]),
                ("docs/notation.md", 0, 0, vec![]),
                ("docs/overview.md", 0, 0, vec![]),
            ]
        );
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
