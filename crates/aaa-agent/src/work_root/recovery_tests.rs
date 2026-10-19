//! 상태 폴더·실제 Git 경계. root 전환과 디스크 부재를 journal 유실로 오인하지 않는다.
use super::*;
use crate::threads::{self, ThreadsConfig};
use aaa_proto::{ThreadKind, ThreadPrepare, ThreadReady};
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(30);
struct Lab {
    dir: PathBuf,
    state: PathBuf,
    repo: PathBuf,
    cfg: ThreadsConfig,
}
impl Lab {
    async fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("aaa-root-recovery-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&dir).unwrap();
        let dir = dir.canonicalize().unwrap();
        let state = dir.join("state");
        let repo = dir.join("repo");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]).await;
        std::fs::write(repo.join("file"), "base\n").unwrap();
        git(&repo, &["add", "file"]).await;
        git(&repo, &["-c", "user.name=Example", "-c", "user.email=example@example.com", "commit", "-qm", "base"]).await;
        let cfg = ThreadsConfig {
            work_root: dir.join("A"),
            // 실제 현재 볼륨을 명시해 Unix 어디서도 휴대용 root의 Git 잠금 계약을 실행한다.
            work_mount: Some(mount_point(&dir).unwrap()),
            state_dir: Some(state.clone()),
            exe: "/opt/aaa".into(),
            profiles: [(
                "fake".into(),
                aaa_config::Profile { harness: "shell".into(), command: vec!["/bin/sh".into()], ..Default::default() },
            )]
            .into(),
            ..Default::default()
        };
        Self { dir, state, repo, cfg }
    }
    fn request(&self, id: &str) -> ThreadPrepare {
        ThreadPrepare {
            resolved_profile: None,
            project: "demo".into(),
            thread: id.into(),
            title: "task".into(),
            kind: ThreadKind::Worktree,
            repo: Some(self.repo.to_string_lossy().into()),
            base: Some("main".into()),
            profile: "fake".into(),
            brief: "work".into(),
            safety: None,
        }
    }
    async fn prepare(&self, id: &str) -> ThreadReady {
        threads::prepare(&self.cfg, self.request(id), WAIT).await.unwrap()
    }
    fn journal(&self, id: &str) -> PathBuf {
        self.state.join("thread-preparations").join(format!("demo-{id}.json"))
    }
    async fn clean(&self, ready: &ThreadReady, uncopied: bool) -> aaa_proto::Cleanup {
        threads::cleanup_recorded(
            &self.state,
            self.repo.to_str(),
            &ready.cwd,
            ready.branch.as_deref(),
            None,
            false,
            uncopied,
            WAIT,
        )
        .await
        .unwrap()
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).unwrap();
    }
}
async fn git(repo: &Path, args: &[&str]) -> String {
    threads::git(repo, args, WAIT).await.unwrap()
}

#[tokio::test]
async fn root_switch_imports_legacy_once_and_recovers_the_original_worktree() {
    let mut lab = Lab::new().await;
    // 구판 root에 있던 실제 준비 기록을 새 agent가 가져오는 저장 형식 계약이다.
    let old = lab.prepare("t-0001").await;
    let mut legacy: serde_json::Value = serde_json::from_slice(&std::fs::read(lab.journal("t-0001")).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("root");
    let legacy_path = lab.cfg.work_root.join(".preparations/demo-t-0001.json");
    std::fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
    std::fs::write(&legacy_path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    std::fs::remove_file(lab.journal("t-0001")).unwrap();
    let original = std::fs::read(&legacy_path).unwrap();
    let a = lab.cfg.work_root.clone();
    activate(&lab.state, &a, lab.cfg.work_mount.as_deref()).unwrap();
    assert_eq!(std::fs::read(&legacy_path).unwrap(), original);
    assert!(lab.journal("t-0001").is_file());
    threads::protect_recorded_worktrees(&lab.cfg).await.unwrap();
    assert!(git(&lab.repo, &["worktree", "list", "--porcelain"]).await.contains("locked aaa:"));
    // 가져오기 완료 뒤 옛 사본이 정본을 덮지 않는다.
    std::fs::write(&legacy_path, b"stale legacy copy").unwrap();
    activate(&lab.state, &a, lab.cfg.work_mount.as_deref()).unwrap();
    lab.cfg.work_root = lab.dir.join("B");
    let state = activate(&lab.state, &lab.cfg.work_root, lab.cfg.work_mount.as_deref()).unwrap();
    assert_eq!(state.roots.iter().map(|r| &r.path).collect::<Vec<_>>(), vec![&a, &lab.cfg.work_root]);
    let recovered = threads::placement(&lab.cfg, "demo", "t-0001", "task", ThreadKind::Worktree, lab.repo.to_str())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.cwd, old.cwd);
    // 현재 B는 정상이더라도 생성 당시 A의 볼륨 검사를 생략하지 않는다.
    let registry = lab.state.join("work-roots.json");
    let bytes = std::fs::read(&registry).unwrap();
    let mut unavailable: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    unavailable["roots"][0]["mount"] =
        serde_json::Value::String(lab.dir.join("missing-volume").to_string_lossy().into());
    std::fs::write(&registry, serde_json::to_vec(&unavailable).unwrap()).unwrap();
    let error = threads::placement(&lab.cfg, "demo", "t-0001", "task", ThreadKind::Worktree, lab.repo.to_str())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("작업 볼륨"), "{error:#}");
    assert!(Path::new(&old.cwd).exists());
    std::fs::write(&registry, bytes).unwrap();
    assert_eq!(lab.prepare("t-0001").await.cwd, old.cwd);
    assert!(!lab.cfg.work_root.join("repo/demo-t-0001").exists());
    assert!(lab.prepare("t-0002").await.cwd.starts_with(lab.cfg.work_root.to_str().unwrap()));
    // 기록 없는 옛 자리도 '없음'으로 정리하지 않고 restart로 회수한다.
    std::fs::remove_file(lab.journal("t-0001")).unwrap();
    let error = threads::placement(&lab.cfg, "demo", "t-0001", "task", ThreadKind::Worktree, lab.repo.to_str())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("기록 없는 준비 자리"), "{error:#}");
    assert_eq!(lab.prepare("t-0001").await.cwd, old.cwd);
    assert!(lab.clean(&old, false).await.worktree_removed);
    // root 변경 트랜잭션 동안 새 준비는 시작하지 않으며 다른 state도 같은 root를 차지하지 못한다.
    let transaction = RootTransaction::open(&lab.state).unwrap();
    let error = threads::prepare(&lab.cfg, lab.request("t-0003"), WAIT).await.unwrap_err();
    assert!(error.to_string().contains("진행 중"), "{error:#}");
    drop(transaction);
    let error = activate(&lab.dir.join("other-state"), &lab.cfg.work_root, lab.cfg.work_mount.as_deref()).unwrap_err();
    assert!(error.to_string().contains("다른 agent"), "{error:#}");
}

#[tokio::test]
async fn owned_git_locks_survive_prune_and_interrupted_cleanup_preserves_other_worktrees() {
    let lab = Lab::new().await;
    activate(&lab.state, &lab.cfg.work_root, lab.cfg.work_mount.as_deref()).unwrap();
    let first = lab.prepare("t-0001").await;
    let second = lab.prepare("t-0002").await;
    let locks = git(&lab.repo, &["worktree", "list", "--porcelain"]).await;
    assert_eq!(locks.matches("locked aaa:").count(), 2, "{locks}");
    let cwd = Path::new(&first.cwd);
    // 빠진 디스크와 같은 Git 관찰: worktree 경로가 없어도 prune이 등록을 보존한다.
    let detached = lab.dir.join("detached");
    std::fs::rename(cwd, &detached).unwrap();
    git(&lab.repo, &["worktree", "prune", "--expire=now"]).await;
    assert!(git(&lab.repo, &["worktree", "list", "--porcelain"]).await.contains(&first.cwd));
    std::fs::rename(&detached, cwd).unwrap();
    // 복사 누락·dirty면 자기 잠금도 복구한 채 남긴다.
    assert!(!lab.clean(&first, true).await.worktree_removed);
    std::fs::write(cwd.join("file"), "keep me\n").unwrap();
    assert!(!lab.clean(&first, false).await.worktree_removed);
    assert_eq!(std::fs::read_to_string(cwd.join("file")).unwrap(), "keep me\n");
    assert!(git(&lab.repo, &["worktree", "list", "--porcelain"]).await.contains("locked aaa:"));
    std::fs::write(cwd.join("file"), "base\n").unwrap();
    // 정리의 내구 의도 기록 뒤 unlock 직후 중단된 저장 상태로 재시작 복구를 검증한다.
    let cleanup_path = std::fs::read_dir(lab.state.join("thread-cleanup")).unwrap().next().unwrap().unwrap().path();
    let mut receipt: serde_json::Value = serde_json::from_slice(&std::fs::read(&cleanup_path).unwrap()).unwrap();
    receipt["relock"] = serde_json::Value::String(lab.cfg.lock_reason(cwd).unwrap().unwrap());
    receipt["result"] = serde_json::Value::Null;
    std::fs::write(&cleanup_path, serde_json::to_vec(&receipt).unwrap()).unwrap();
    git(&lab.repo, &["worktree", "unlock", &first.cwd]).await;
    threads::recover_cleanup_locks(&lab.state).await.unwrap();
    let restored = git(&lab.repo, &["worktree", "list", "--porcelain"]).await;
    assert_eq!(restored.matches("locked aaa:").count(), 2, "{restored}");
    // 사람의 잠금은 풀지 않는다.
    git(&lab.repo, &["worktree", "unlock", &first.cwd]).await;
    git(&lab.repo, &["worktree", "lock", "--reason", "human", &first.cwd]).await;
    assert!(!lab.clean(&first, false).await.worktree_removed);
    assert!(git(&lab.repo, &["worktree", "list", "--porcelain"]).await.contains("locked human"));
    git(&lab.repo, &["worktree", "unlock", &first.cwd]).await;
    // 별도 작업이 수동으로 사라졌어도 다른 대상을 정리하며 넓게 prune하지 않는다.
    git(&lab.repo, &["worktree", "unlock", &second.cwd]).await;
    std::fs::remove_dir_all(&second.cwd).unwrap();
    assert!(lab.clean(&first, false).await.worktree_removed);
    assert!(git(&lab.repo, &["worktree", "list", "--porcelain"]).await.contains(&second.cwd));
    assert!(lab.clean(&second, false).await.worktree_removed);
    assert!(!git(&lab.repo, &["worktree", "list", "--porcelain"]).await.contains(&second.cwd));
}
