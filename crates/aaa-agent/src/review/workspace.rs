//! 등록 저장소의 객체에서 사본만 만든다. 원본 Git 설정·hook·filter·작업 파일을 물려받지 않는다.
use super::{Runner, command, controlled, executable, read_bounded, stop_child};
use aaa_common::{child_process::TokioCommandExt as _, util};
use aaa_proto::ReviewRun;
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

pub(super) struct Workspace {
    root: PathBuf,
    pub(super) tree: PathBuf,
    pub(super) home: PathBuf,
}

impl Workspace {
    pub(super) fn new(state: &Path) -> Result<Self> {
        let parent = state.join("review-work");
        util::ensure_private_dir(&parent).context("검수 임시 작업 상위 폴더")?;
        let root = parent.canonicalize()?.join(util::new_id());
        std::fs::create_dir(&root).context("검수 임시 작업 폴더")?;
        let ws = Self { tree: root.join("candidate"), home: root.join("home"), root };
        util::ensure_private_dir(&ws.home).context("검수 실행기 전용 홈")?;
        std::fs::create_dir(&ws.tree).context("검수 후보 작업 사본")?;
        Ok(ws)
    }

    pub(super) async fn prepare(&self, runner: &Runner, run: &ReviewRun) -> Result<String> {
        let git = executable("git")?;
        let repo = canonical_dir(&run.repo)?;
        let source = canonical_dir(&run.thread_cwd)?;
        let requested = canonical_dir(&run.requested_cwd)?;
        let source_top = self.git(runner, &git, &source, &["rev-parse", "--show-toplevel"], None, 8192).await?;
        ensure!(canonical_dir(source_top.trim_end())? == requested, "검수 요청 위치가 thread 작업 사본과 다르다");
        let common = ["rev-parse", "--path-format=absolute", "--git-common-dir"];
        let registered = self.git(runner, &git, &repo, &common, None, 8192).await?;
        let worktree = self.git(runner, &git, &source, &common, None, 8192).await?;
        ensure!(
            canonical_dir(registered.trim_end())? == canonical_dir(worktree.trim_end())?,
            "thread 작업 사본이 등록 저장소에 속하지 않는다"
        );
        let objects = self
            .git(runner, &git, &repo, &["rev-parse", "--path-format=absolute", "--git-path", "objects"], None, 8192)
            .await?;
        let objects = canonical_dir(objects.trim_end())?;
        let format = if run.target.commit.len() == 64 { "--object-format=sha256" } else { "--object-format=sha1" };
        self.git(runner, &git, &self.tree, &["init", "--quiet", "--template=", format], None, 4096).await?;
        let tree_ref = format!("{}^{{tree}}", run.target.commit);
        let tree = self
            .git(
                runner,
                &git,
                &self.tree,
                &["rev-parse", "--verify", "--end-of-options", &tree_ref],
                Some(&objects),
                4096,
            )
            .await?;
        ensure!(tree.trim_end() == run.target.tree, "검수 후보 커밋과 요청한 tree가 다르다");
        self.git(
            runner,
            &git,
            &self.tree,
            &["-c", "core.symlinks=false", "checkout", "--quiet", "--detach", &run.target.commit, "--"],
            Some(&objects),
            4096,
        )
        .await
        .context("검수 후보 detached checkout")?;
        let range = format!("{}...{}", run.target.base, run.target.commit);
        self.git(
            runner,
            &git,
            &self.tree,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--unified=24",
                &range,
                "--",
            ],
            Some(&objects),
            aaa_common::diff::PATCH_MAX_BYTES,
        )
        .await
        .context("검수 기준과 후보의 Git diff")
    }

    async fn git(
        &self,
        runner: &Runner,
        exe: &Path,
        path: &Path,
        args: &[&str],
        objects: Option<&Path>,
        limit: usize,
    ) -> Result<String> {
        let mut cmd = command(exe, path, &self.home)?;
        cmd.args(aaa_common::diff::git_prefix())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(objects) = objects {
            cmd.env("GIT_ALTERNATE_OBJECT_DIRECTORIES", std::env::join_paths([objects])?);
        }
        let mut cancel = runner.cancel.clone();
        if *cancel.borrow() {
            return Err(super::Cancelled.into());
        }
        let mut child = cmd.spawn_guarded().context("검수 Git 실행 실패")?;
        let stdout = child.stdout.take().context("검수 Git 출력 pipe 없음")?;
        let answer = controlled(&mut cancel, runner.remaining().min(Duration::from_secs(30)), async {
            let data = read_bounded(stdout, limit).await?;
            let status = child.wait().await?;
            ensure!(status.success(), "검수 Git {} 실패({status})", args[0]);
            String::from_utf8(data).context("검수 Git 출력이 UTF-8이 아니다")
        })
        .await;
        stop_child(&mut child).await?;
        answer
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&self.root), "검수 임시 사본 정리");
    }
}

fn canonical_dir(path: &str) -> Result<PathBuf> {
    let path = Path::new(path);
    ensure!(path.is_absolute() && path.is_dir(), "검수 작업 폴더가 없다");
    path.canonicalize().context("검수 작업 폴더 확인")
}
