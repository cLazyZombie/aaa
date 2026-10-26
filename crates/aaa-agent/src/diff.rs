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
            let out = git
                .run(&d::diff_args("--patch", q.ignore_whitespace, &base_sha, &paths), None, d::FILE_PATCH_MAX_JSON)
                .await?;
            if !out.over && !out.status.is_some_and(|s| s.success()) {
                bail!("git diff 실패");
            }
            let (patch, cut) =
                d::cut_patch(String::from_utf8_lossy(&out.bytes).into_owned(), usize::MAX, d::FILE_PATCH_MAX_JSON);
            view.truncated |= cut || out.over;
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
mod tests {
    use super::*;
    use aaa_proto::{DiffFileRef, DiffStatus};
    use std::process::Command;

    /// 비교가 건드리면 안 되는 것: index 바이트·HEAD·ref·작업 파일(.git 안 포함) 경로와 내용
    #[derive(PartialEq)]
    struct Snapshot {
        index: Vec<u8>,
        head: String,
        refs: String,
        files: Vec<(String, Vec<u8>)>,
    }

    struct Repo {
        root: PathBuf,
        dir: PathBuf,
        empty: PathBuf,
    }

    impl Repo {
        fn new() -> Repo {
            let root = std::env::temp_dir().join(format!("aaa-diff-test-{}", aaa_common::util::new_id()));
            let dir = root.join("repo");
            std::fs::create_dir_all(&dir).unwrap();
            let empty = root.join("empty-config");
            std::fs::write(&empty, "").unwrap();
            let r = Repo { root, dir, empty };
            r.git(&["init", "-q", "-b", "main"]);
            std::fs::create_dir_all(r.dir.join(".git/info")).unwrap();
            r
        }
        fn git(&self, args: &[&str]) -> String {
            let out = Command::new("git")
                .args(["-c", "init.templateDir=", "-c", "user.name=Test", "-c", "user.email=test@example.com"])
                .args(args)
                .current_dir(&self.dir)
                .env("GIT_CONFIG_GLOBAL", &self.empty)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        fn write(&self, path: &str, text: &str) {
            let p = self.dir.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        fn snapshot(&self) -> Snapshot {
            fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
                let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
                entries.sort();
                for p in entries {
                    let rel = p.strip_prefix(base).unwrap().to_string_lossy().into_owned();
                    if p.is_dir() {
                        walk(base, &p, out);
                    } else if rel != ".git/index" {
                        out.push((rel, std::fs::read(&p).unwrap()));
                    }
                }
            }
            let mut files = Vec::new();
            walk(&self.dir, &self.dir, &mut files);
            let index = std::fs::read(self.dir.join(".git/index")).unwrap();
            Snapshot { index, head: self.git(&["rev-parse", "HEAD"]), refs: self.git(&["for-each-ref"]), files }
        }
    }

    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn q(scope: DiffScope, patch: bool) -> DiffQuery {
        DiffQuery { scope, patch, ..Default::default() }
    }

    /// branch 범위는 merge-base 대비 커밋·staged·미커밋·새 파일을 담고, 무시한 파일·`.aaa/`는 빼며, 저장소를 바꾸지 않는다.
    #[tokio::test]
    async fn branch_and_uncommitted_read_every_change_without_touching_the_repo() {
        let r = Repo::new();
        r.write("keep.txt", "one\ntwo\n");
        r.write("gone.txt", "bye\n");
        r.write("old name.txt", "same\nsame\nsame\n");
        r.write(".gitignore", "*.log\n");
        r.git(&["add", "-A"]);
        r.git(&["commit", "-qm", "first"]);
        let main = r.git(&["rev-parse", "HEAD"]);
        r.git(&["switch", "-qc", "aaa/demo/t-0001-x"]);
        r.write("src/lib.rs", "fn a() {}\n");
        r.git(&["add", "src/lib.rs"]);
        r.git(&["commit", "-qm", "branch"]);
        // main이 앞으로 가도 merge-base 대비다(main 쪽 변경은 보이지 않는다)
        r.git(&["switch", "-q", "main"]);
        r.write("main-only.txt", "x\n");
        r.git(&["add", "main-only.txt"]);
        r.git(&["commit", "-qm", "main moves"]);
        r.git(&["switch", "-q", "aaa/demo/t-0001-x"]);
        r.git(&["rm", "-q", "gone.txt"]);
        r.git(&["mv", "old name.txt", "new name.txt"]);
        r.write("keep.txt", "one\nTWO\nthree\n");
        r.write("new dir/fresh.md", "# hi\n");
        r.write("debug.log", "ignored\n");
        r.write(".aaa/demo-t-0001/report.md", "work dir\n");
        std::fs::write(r.dir.join(".git/info/exclude"), ".aaa/\n").unwrap();
        let before = r.snapshot();
        let cwd = r.dir.to_string_lossy().into_owned();

        let v = query(&cwd, Some("main"), &q(DiffScope::Branch, true)).await.unwrap();
        assert_eq!(
            (v.base.as_str(), v.base_sha.as_str(), v.head.as_str()),
            ("main", main.as_str(), "aaa/demo/t-0001-x")
        );
        let rows: Vec<_> = v
            .files
            .as_ref()
            .unwrap()
            .iter()
            .map(|f| (f.path.as_str(), f.old_path.as_deref(), f.status, f.additions, f.deletions))
            .collect();
        assert_eq!(
            rows,
            [
                ("gone.txt", None, DiffStatus::Deleted, Some(0), Some(1)),
                ("keep.txt", None, DiffStatus::Modified, Some(2), Some(1)),
                ("new dir/fresh.md", None, DiffStatus::Untracked, Some(1), Some(0)),
                ("new name.txt", Some("old name.txt"), DiffStatus::Renamed, Some(0), Some(0)),
                ("src/lib.rs", None, DiffStatus::Added, Some(1), Some(0)),
            ]
        );
        let patch = v.patch.unwrap();
        assert!(patch.contains("diff --git a/keep.txt b/keep.txt") && patch.contains("+TWO"), "{patch}");
        assert!(patch.contains("+# hi") && !patch.contains("main-only") && !patch.contains("ignored"));
        assert!(!v.truncated && !v.untracked_omitted);

        // 미커밋은 HEAD 대비: 브랜치 커밋(src/lib.rs)은 없다
        let u = query(&cwd, Some("main"), &q(DiffScope::Uncommitted, false)).await.unwrap();
        let paths: Vec<_> = u.files.unwrap().into_iter().map(|f| f.path).collect();
        assert_eq!(paths, ["gone.txt", "keep.txt", "new dir/fresh.md", "new name.txt"]);
        assert_eq!(u.base, "HEAD");
        assert!(u.patch.is_none());

        // 파일 하나(공백 무시): 그 파일만
        let one = DiffQuery {
            file: Some(DiffFileRef { path: "keep.txt".into(), old_path: None }),
            ignore_whitespace: true,
            ..q(DiffScope::Branch, true)
        };
        let f = query(&cwd, Some("main"), &one).await.unwrap();
        let fp = f.patch.unwrap();
        assert!(fp.starts_with("diff --git a/keep.txt b/keep.txt") && !fp.contains("fresh.md"));
        assert!(f.files.is_none());

        // 없는 base는 다음 후보로(main)
        let g = query(&cwd, Some("origin/nope"), &q(DiffScope::Branch, false)).await.unwrap();
        assert_eq!(g.base, "main");

        let after = r.snapshot();
        assert!(after.index == before.index, "index가 바뀌었다");
        assert_eq!((&after.head, &after.refs), (&before.head, &before.refs), "HEAD·ref가 바뀌었다");
        let names = |s: &Snapshot| s.files.iter().map(|f| f.0.clone()).collect::<Vec<_>>();
        let extra: Vec<_> = names(&after).into_iter().filter(|n| !names(&before).contains(n)).collect();
        assert_eq!(names(&after), names(&before), "파일 목록이 바뀌었다: {extra:?}");
        assert!(after.files == before.files, "파일 내용이 바뀌었다");
    }

    /// PID가 다시 배정돼도 이전 프로세스의 임시 index 폴더를 건드리지 않고 비교한다.
    #[tokio::test]
    async fn stale_pid_temp_index_never_blocks_a_new_process() {
        const CHILD: &str = "AAA_TEST_DIFF_PID_REUSE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            // 새 시험 프로세스라 옛 실행기의 첫 순번은 0이다. 남의 잔여 폴더를 지우거나 재사용하면 안 된다.
            let legacy = std::env::temp_dir().join(format!("aaa-diff-{}-0", std::process::id()));
            std::fs::create_dir(&legacy).unwrap();
            std::fs::write(legacy.join("keep"), b"left by an earlier process").unwrap();
            let r = Repo::new();
            r.write("one.txt", "one\n");
            r.git(&["add", "-A"]);
            r.git(&["commit", "-qm", "first"]);
            r.write("one.txt", "two\n");
            let v = query(&r.dir.to_string_lossy(), None, &q(DiffScope::Uncommitted, true)).await.unwrap();
            assert!(v.patch.unwrap().contains("+two"));
            assert_eq!(std::fs::read(legacy.join("keep")).unwrap(), b"left by an earlier process");
            return;
        }
        // TMP 계열은 자식 환경에만 둔다. 다른 병렬 시험의 전역 환경·임시 폴더를 바꾸지 않는다.
        let root = std::env::temp_dir().join(format!("aaa-diff-pid-owner-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "diff::tests::stale_pid_temp_index_never_blocks_a_new_process", "--nocapture"])
            .env(CHILD, "1")
            .env("TMPDIR", &root)
            .env("TMP", &root)
            .env("TEMP", &root)
            .output()
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// 큰 비교는 patch 없이 목록만 주고(웹이 파일마다 읽는다), 파일 하나도 frame 안으로 줄 경계에서 자른다.
    #[tokio::test]
    async fn large_changes_hand_over_to_per_file_reads_inside_the_frame() {
        let r = Repo::new();
        r.write("a.txt", "a\n");
        r.git(&["add", "-A"]);
        r.git(&["commit", "-qm", "first"]);
        let line = format!("{}\n", "\"q\\".repeat(40));
        r.write("big.txt", &line.repeat(d::FILE_PATCH_MAX_JSON / line.len() + 10));
        r.write("small.txt", "s\n");
        let cwd = r.dir.to_string_lossy().into_owned();
        let v = query(&cwd, None, &q(DiffScope::Branch, true)).await.unwrap();
        assert!(v.truncated && v.patch.is_none());
        assert_eq!(v.files.unwrap().len(), 2);
        let one = DiffQuery {
            file: Some(DiffFileRef { path: "big.txt".into(), old_path: None }),
            ..q(DiffScope::Branch, true)
        };
        let f = query(&cwd, None, &one).await.unwrap();
        let p = f.patch.clone().unwrap();
        assert!(f.truncated && p.ends_with('\n') && d::json_len(&p) <= d::FILE_PATCH_MAX_JSON);
        let frame = serde_json::to_vec(&aaa_proto::AgentResponse::Diff(f)).unwrap();
        assert!(frame.len() < aaa_proto::MAX_FRAME, "{}", frame.len());
    }

    /// 저장소 설정의 외부 diff·textconv·fsmonitor를 부르지 않는다(`work_git_read_commands`와 같은 경계).
    #[cfg(unix)]
    #[tokio::test]
    async fn repository_callbacks_never_run() {
        use std::os::unix::fs::PermissionsExt;
        let r = Repo::new();
        // Linux git의 초 단위 stat 비교를 OS·실행 속도와 무관하게 만든다.
        r.git(&["config", "core.checkStat", "minimal"]);
        r.git(&["config", "core.trustctime", "false"]);
        let stamp = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        let set_mtime = |path: &Path| {
            std::fs::File::options().write(true).open(path).unwrap().set_modified(stamp).unwrap();
        };
        r.write("one.txt", "one\n");
        set_mtime(&r.dir.join("one.txt"));
        r.git(&["add", "-A"]);
        r.git(&["commit", "-qm", "first"]);
        // 같은 초의 add→덮어쓰기인 racy entry. 원본 git은 내용을 다시 검사한다.
        set_mtime(&r.dir.join(".git/index"));
        let marker = r.root.join("callback-ran");
        let callback = r.root.join("callback");
        std::fs::write(&callback, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).unwrap();
        std::fs::set_permissions(&callback, std::fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["core.fsmonitor", "core.pager", "diff.external"] {
            r.git(&["config", name, callback.to_str().unwrap()]);
        }
        r.git(&["config", "diff.fixture.textconv", callback.to_str().unwrap()]);
        std::fs::write(r.dir.join(".git/info/attributes"), "*.txt diff=fixture\n").unwrap();
        r.write("one.txt", "two\n");
        set_mtime(&r.dir.join("one.txt"));
        r.write("new.txt", "new\n");
        let index = r.dir.join(".git/index");
        let before = std::fs::read(&index).unwrap();
        let v = query(&r.dir.to_string_lossy(), None, &q(DiffScope::Uncommitted, true)).await.unwrap();
        assert!(v.patch.unwrap().contains("+two"));
        assert!(!marker.exists(), "외부 callback을 실행하면 안 된다");
        assert_eq!(std::fs::read(&index).unwrap(), before);
        assert_eq!(std::fs::metadata(&index).unwrap().modified().unwrap(), stamp);
    }

    #[tokio::test]
    async fn folders_outside_git_are_refused() {
        let root = std::env::temp_dir().join(format!("aaa-diff-plain-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let e = query(&root.to_string_lossy(), None, &q(DiffScope::Branch, false)).await.unwrap_err();
        assert!(format!("{e:#}").contains("git 저장소가 아니다"), "{e:#}");
        std::fs::remove_dir_all(root).unwrap();
        assert!(query("relative/dir", None, &q(DiffScope::Branch, false)).await.is_err());
    }
}
