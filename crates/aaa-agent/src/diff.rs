//! thread worktree 비교(diff.v1, plan §8.305). 작업 파일·index·ref를 바꾸지 않는다.
//!
//! t3code 4b1024da `GitVcsDriverCore.ts:2639-3005`처럼 index를 임시 파일로 복사하고 그 사본에만 새 파일을
//! intent-to-add로 담아(`GIT_INDEX_FILE`) 커밋·미커밋·새 파일을 한 비교로 읽는다. 저장소의 index·작업 파일은 그대로다.
use aaa_common::child_process::TokioCommandExt as _;
use aaa_common::diff as d;
use aaa_proto::{DiffQuery, DiffScope, DiffView};
use anyhow::{Context, Result, bail};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn query(cwd: &str, base: Option<&str>, q: &DiffQuery) -> Result<DiffView> {
    let dir = Path::new(cwd);
    if !dir.is_absolute() || !dir.is_dir() {
        bail!("thread 작업 폴더가 없다");
    }
    tokio::time::timeout(d::QUERY_TIMEOUT, inner(dir, base, q)).await.context("비교 시간 상한이다")?
}

struct Out {
    bytes: Vec<u8>,
    /// 상한에서 읽기를 멈췄다(명령은 끝낸다)
    over: bool,
    status: Option<ExitStatus>,
}

struct Git<'a> {
    top: &'a Path,
    /// 임시 index와 그 객체 폴더(저장소 객체는 읽기만 하는 alternate)
    temp: Option<&'a TempIndex>,
}

impl Git<'_> {
    async fn run(&self, args: &[String], stdin: Option<&[u8]>, limit: usize) -> Result<Out> {
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(d::git_prefix())
            .args(args)
            .current_dir(self.top)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(t) = self.temp {
            // intent-to-add도 빈 blob 객체를 쓴다: 저장소 `.git/objects`가 아니라 임시 폴더에 쓰게 한다
            cmd.env("GIT_INDEX_FILE", t.path())
                .env("GIT_OBJECT_DIRECTORY", t.0.join("objects"))
                .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &t.1);
        }
        let mut child = cmd.spawn_guarded().context("git 실행 실패")?;
        if let Some(input) = stdin {
            let mut w = child.stdin.take().unwrap();
            w.write_all(input).await?;
            drop(w);
        }
        let mut stdout = child.stdout.take().unwrap().take(limit as u64 + 1);
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).await?;
        if bytes.len() > limit {
            bytes.truncate(limit);
            return Ok(Out { bytes, over: true, status: None });
        }
        Ok(Out { bytes, over: false, status: Some(child.wait().await?) })
    }

    async fn ok(&self, args: &[&str]) -> Result<Vec<u8>> {
        let args: Vec<String> = args.iter().map(|s| (*s).into()).collect();
        self.checked(&args, None, d::OUTPUT_MAX_BYTES).await
    }

    async fn checked(&self, args: &[String], stdin: Option<&[u8]>, limit: usize) -> Result<Vec<u8>> {
        let out = self.run(args, stdin, limit).await?;
        if out.over {
            bail!("git {} 출력이 상한을 넘는다", args[0]);
        }
        if !out.status.is_some_and(|s| s.success()) {
            bail!("git {} 실패", args[0]);
        }
        Ok(out.bytes)
    }

    /// commit 이름을 full SHA로(없으면 None)
    async fn commit(&self, name: &str) -> Option<String> {
        let spec = format!("{name}^{{commit}}");
        let out = self
            .ok(&["rev-parse", "--verify", "--quiet", "--end-of-options", &spec])
            .await
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        Some(String::from_utf8_lossy(&out).trim().to_string()).filter(|s| !s.is_empty())
    }
}

/// 이 비교 한 번만 쓰는 index 사본과 객체 폴더(`.0`), 저장소의 객체 폴더(`.1`). 끝나면 임시 폴더째 지운다
struct TempIndex(PathBuf, PathBuf);

impl TempIndex {
    fn path(&self) -> PathBuf {
        self.0.join("index")
    }
}

impl Drop for TempIndex {
    fn drop(&mut self) {
        aaa_common::diagnostic::ignored(
            std::fs::remove_dir_all(&self.0),
            "정리 실패가 본 작업의 결과를 덮지 않게 한다",
        );
    }
}

async fn temp_index(git: &Git<'_>, head: bool) -> Result<TempIndex> {
    // PID는 재사용된다. 이전 프로세스의 잔여 폴더와 겹치지 않게 비교마다 새 ID로 한 번 만든다.
    let dir = std::env::temp_dir().join(format!("aaa-diff-{}", aaa_common::util::new_id()));
    std::fs::create_dir(&dir).context("임시 index 폴더를 만들지 못했다")?;
    let path = |b: Vec<u8>| PathBuf::from(String::from_utf8_lossy(&b).trim());
    let objects = path(git.ok(&["rev-parse", "--path-format=absolute", "--git-path", "objects"]).await?);
    let tmp = TempIndex(dir, objects);
    std::fs::create_dir(tmp.0.join("objects")).context("임시 객체 폴더를 만들지 못했다")?;
    let real = path(git.ok(&["rev-parse", "--path-format=absolute", "--git-path", "index"]).await?);
    let index = tmp.path();
    let with = Git { top: git.top, temp: Some(&tmp) };
    if real.is_file() {
        {
            let mut source = std::fs::File::open(&real).context("index를 열지 못했다")?;
            let modified = source.metadata()?.modified()?;
            let mut dest = std::fs::File::create(&index).context("임시 index를 만들지 못했다")?;
            std::io::copy(&mut source, &mut dest).context("index를 복사하지 못했다")?;
            // index 시각은 racy entry의 내용 재검사 기준이다. 복사 시각으로 바꾸면 같은 초·크기 변경을 놓친다.
            dest.set_modified(modified).context("index 수정 시각을 보존하지 못했다")?;
        }
        // 사본이 저장소의 shared index를 가리키지 않게 한 파일로 만든다(t3code와 같다)
        with.ok(&["update-index", "--no-split-index"]).await?;
    } else if head {
        with.ok(&["read-tree", "HEAD"]).await?;
    }
    Ok(tmp)
}

async fn inner(dir: &Path, base: Option<&str>, q: &DiffQuery) -> Result<DiffView> {
    if q.full_context && (!q.patch || q.file.is_none()) {
        bail!("전체 맥락은 파일 하나의 patch를 지정해야 한다");
    }
    let plain = Git { top: dir, temp: None };
    let top = plain.ok(&["rev-parse", "--show-toplevel"]).await.context("git 저장소가 아니다")?;
    let top = PathBuf::from(String::from_utf8_lossy(&top).trim());
    let git = Git { top: &top, temp: None };
    let head_sha = git.commit("HEAD").await;
    let head = match git.ok(&["symbolic-ref", "--quiet", "--short", "HEAD"]).await {
        Ok(b) => String::from_utf8_lossy(&b).trim().to_string(),
        Err(source_error) => {
            aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            "HEAD".into()
        }
    };
    let (base_name, base_sha) = match q.scope {
        DiffScope::Uncommitted => ("HEAD".to_string(), head_sha.clone().unwrap_or_else(|| d::EMPTY_TREE.into())),
        DiffScope::Branch => branch_base(&git, base, head_sha.as_deref()).await?,
    };

    let tmp = temp_index(&git, head_sha.is_some()).await?;
    let git = Git { top: &top, temp: Some(&tmp) };
    let others = git.ok(&["ls-files", "--others", "--exclude-standard", "-z"]).await?;
    // 안에 든 다른 저장소(`dir/`)는 git이 파일로 담지 못한다
    let others: Vec<String> = d::nul_paths(&others).into_iter().filter(|p| !p.ends_with('/')).collect();
    let untracked_omitted = others.len() > d::UNTRACKED_MAX_FILES;
    let mut untracked = HashSet::new();
    if !untracked_omitted && !others.is_empty() {
        let mut list = Vec::new();
        for p in &others {
            list.extend_from_slice(p.as_bytes());
            list.push(0);
        }
        let args: Vec<String> =
            ["--literal-pathspecs", "add", "--intent-to-add", "--pathspec-from-file=-", "--pathspec-file-nul"]
                .iter()
                .map(|s| (*s).into())
                .collect();
        git.checked(&args, Some(&list), d::OUTPUT_MAX_BYTES).await?;
        untracked.extend(others);
    }

    let mut view = DiffView {
        scope: q.scope,
        base: base_name,
        base_sha: base_sha.clone(),
        head,
        untracked_omitted,
        truncated: untracked_omitted,
        ..Default::default()
    };
    if let Some(file) = &q.file {
        let mut paths = vec![file.path.clone()];
        paths.extend(file.old_path.clone());
        if q.patch {
            let mut args = d::diff_args("--patch", q.ignore_whitespace, &base_sha, &paths);
            if q.full_context {
                args.insert(1, "--unified=2147483647".into());
            }
            let out = git.run(&args, None, d::FILE_PATCH_MAX_JSON).await?;
            if !out.over && !out.status.is_some_and(|s| s.success()) {
                bail!("git diff 실패");
            }
            let (patch, cut) =
                d::cut_patch(String::from_utf8_lossy(&out.bytes).into_owned(), usize::MAX, d::FILE_PATCH_MAX_JSON);
            view.truncated |= cut || out.over;
            view.full_context = q.full_context && !cut && !out.over;
            view.patch = Some(patch);
        }
        return Ok(view);
    }

    let (numstat_args, names_args) = (
        d::diff_args("--numstat", q.ignore_whitespace, &base_sha, &[]),
        d::diff_args("--name-status", false, &base_sha, &[]),
    );
    let (numstat, names) = tokio::try_join!(
        git.checked(&numstat_args, None, d::OUTPUT_MAX_BYTES),
        git.checked(&names_args, None, d::OUTPUT_MAX_BYTES)
    )?;
    let files = d::files(d::parse_numstat(&numstat), &d::parse_name_status(&names), &untracked);
    let empty = files.is_empty();
    if files.len() > d::MAX_FILES {
        view.truncated = true;
    } else {
        view.files = Some(files);
    }
    if q.patch {
        if empty {
            view.patch = Some(String::new());
        } else {
            let out = git
                .run(&d::diff_args("--patch", q.ignore_whitespace, &base_sha, &[]), None, d::PATCH_MAX_BYTES)
                .await?;
            if !out.over && !out.status.is_some_and(|s| s.success()) {
                bail!("git diff 실패");
            }
            if out.over && view.files.is_some() {
                // 웹이 파일마다 읽는다(t3code lazy 파일별 읽기)
                view.truncated = true;
            } else {
                let (patch, cut) = d::cut_patch(
                    String::from_utf8_lossy(&out.bytes).into_owned(),
                    d::PATCH_MAX_BYTES,
                    d::FILE_PATCH_MAX_JSON,
                );
                view.truncated |= cut || out.over;
                view.patch = Some(patch);
            }
        }
    }
    Ok(view)
}

/// branch 범위의 기준: 후보 중 처음 있는 commit과 HEAD의 merge-base. 첫 커밋 전이면 빈 tree, 후보가 없으면 HEAD(미커밋과 같다)
async fn branch_base(git: &Git<'_>, base: Option<&str>, head: Option<&str>) -> Result<(String, String)> {
    let Some(head) = head else { return Ok(("HEAD".into(), d::EMPTY_TREE.into())) };
    for name in d::base_candidates(base) {
        if let Some(sha) = git.commit(&name).await {
            let mb = git.ok(&["merge-base", "--end-of-options", &sha, head]).await;
            if let Ok(mb) = mb {
                let mb = String::from_utf8_lossy(&mb).trim().to_string();
                if !mb.is_empty() {
                    return Ok((name, mb));
                }
            }
        }
    }
    Ok(("HEAD".into(), head.to_string()))
}

#[cfg(test)]
mod tests;
