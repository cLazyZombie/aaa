//! 배포는 host가 판정한 typed 요청만 받는다. 실행 전 기록하고 같은 ID는 다시 실행하지 않는다.
use crate::Agent;
use aaa_proto::{DeployAction, DeployJob, DeployService, DeployState, ErrorBody, ErrorCode};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub type Executor = fn(&Path, DeployAction) -> Result<String, String>;
pub(crate) struct Manager {
    root: PathBuf,
    _lease: std::fs::File,
    instance: String,
    running_sha256: String,
    gate: Mutex<()>,
    executor: Mutex<Option<Executor>>,
}
fn error(e: impl std::fmt::Display) -> ErrorBody {
    ErrorBody::new(ErrorCode::BadRequest, e.to_string())
}
fn valid(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}
fn sha(path: &Path) -> Result<String, ErrorBody> {
    let mut f = std::fs::File::open(path).map_err(error)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(error)?;
    Ok(hex::encode(h.finalize()))
}
impl Manager {
    pub(crate) fn new(state: &Path, exe: &Path) -> anyhow::Result<Self> {
        let root = state.join("deployments");
        aaa_common::util::ensure_private_dir(&root)?;
        // OS가 프로세스 종료 때 푸는 잠금이다. PID 재사용·재부팅으로 옛 작업을 살아 있다고 오인하지 않는다.
        let lease = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("executor.lock"))?;
        lease.try_lock().map_err(|e| anyhow::anyhow!("같은 상태 폴더의 배포 실행기가 이미 살아 있다: {e}"))?;
        let me = Self {
            _lease: lease,
            root,
            instance: aaa_common::util::new_id(),
            running_sha256: sha(exe).map_err(|e| anyhow::anyhow!(e.message))?,
            gate: Mutex::new(()),
            executor: Mutex::new(None),
        };
        for entry in std::fs::read_dir(&me.root)? {
            let entry = entry?;
            let Some(id) = entry.path().file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            if !valid(&id) {
                continue;
            }
            let Ok(mut job) = me.status(&id) else {
                continue;
            };
            if !matches!(job.state, DeployState::Queued | DeployState::Running) {
                continue;
            }
            let restarted = job.state == DeployState::Running
                && job.action == (DeployAction::Restart { service: DeployService::Agent })
                && sha(exe).ok().as_deref() == Some(job.expected_sha256.as_str());
            job.state = if restarted { DeployState::Completed } else { DeployState::Interrupted };
            job.result = Some(
                if restarted {
                    "새 agent 프로세스와 실행 파일 SHA-256을 확인했다"
                } else {
                    "이전 agent가 실행 중에 끝났다. 다시 실행하지 않았다. 실행 파일과 서비스를 확인한다"
                }
                .into(),
            );
            job.updated_ms = aaa_common::util::now_ms();
            me.save(&job).map_err(|e| anyhow::anyhow!(e.message))?;
        }
        Ok(me)
    }
    pub(crate) fn response(&self, job: DeployJob) -> aaa_proto::AgentResponse {
        aaa_proto::AgentResponse::Deployment {
            job,
            running_software: aaa_common::util::software(),
            running_sha256: self.running_sha256.clone(),
        }
    }
    fn path(&self, id: &str) -> Result<PathBuf, ErrorBody> {
        if !valid(id) {
            return Err(error("배포 작업 ID 형식이 아니다"));
        }
        Ok(self.root.join(format!("{id}.json")))
    }
    fn save(&self, job: &DeployJob) -> Result<(), ErrorBody> {
        aaa_config::profile_edit::write_atomic(&self.path(&job.id)?, &serde_json::to_vec(job).map_err(error)?)
            .map_err(error)
    }
    pub(crate) fn status(&self, id: &str) -> Result<DeployJob, ErrorBody> {
        serde_json::from_slice(
            &std::fs::read(self.path(id)?)
                .map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("배포 기록이 없다: {e}")))?,
        )
        .map_err(error)
    }
    fn prepare(
        &self,
        exe: &Path,
        id: String,
        project: String,
        action: DeployAction,
    ) -> Result<(DeployJob, bool), ErrorBody> {
        let _gate = self.gate.lock().unwrap();
        let path = self.path(&id)?;
        if path.exists() {
            let old = self.status(&id)?;
            if old.project != project || old.action != action {
                return Err(error("같은 배포 ID를 다른 작업에 쓸 수 없다"));
            }
            return Ok((old, false));
        }
        if self.executor.lock().unwrap().is_none() {
            return Err(error("이 agent에는 배포 실행기가 없다"));
        }
        for entry in std::fs::read_dir(&self.root).map_err(error)? {
            let entry = entry.map_err(error)?;
            let Some(id) = entry.path().file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
                continue;
            };
            if valid(&id) {
                let job = self.status(&id)?;
                if matches!(job.state, DeployState::Queued | DeployState::Running) {
                    return Err(error(format!("배포 작업 {}가 진행 중이다", job.id)));
                }
            }
        }
        let now = aaa_common::util::now_ms();
        let job = DeployJob {
            id,
            project,
            action,
            state: DeployState::Queued,
            instance: self.instance.clone(),
            executor_pid: std::process::id(),
            expected_sha256: sha(exe)?,
            result: None,
            created_ms: now,
            updated_ms: now,
        };
        self.save(&job)?;
        Ok((job, true))
    }
    async fn execute(&self, exe: PathBuf, mut job: DeployJob) -> Option<DeployJob> {
        job.state = DeployState::Running;
        job.updated_ms = aaa_common::util::now_ms();
        if let Err(e) = self.save(&job) {
            tracing::error!(job=%job.id, "배포 실행 전 기록 실패: {}", e.message);
            return None;
        }
        let Some(execute) = *self.executor.lock().unwrap() else {
            return None;
        };
        let action = job.action.clone();
        let result =
            tokio::task::spawn_blocking(move || execute(&exe, action)).await.map_err(|e| e.to_string()).and_then(|r| r);
        job.state = if result.is_ok() { DeployState::Completed } else { DeployState::Failed };
        job.result = Some(result.unwrap_or_else(|e| e).chars().take(4096).collect());
        job.updated_ms = aaa_common::util::now_ms();
        if let Err(e) = self.save(&job) {
            tracing::error!(job=%job.id, "배포 결과 기록 실패: {}", e.message);
            return None;
        }
        Some(job)
    }
}
impl Agent {
    pub fn configure_deployment(&self, executor: Executor) {
        *self.deployments.executor.lock().unwrap() = Some(executor);
    }
    pub(crate) async fn deploy_start(
        self: Arc<Self>,
        request_id: String,
        id: String,
        project: String,
        action: DeployAction,
    ) {
        let result = self.deployments.prepare(&self.exe, id, project, action);
        let run = result.as_ref().ok().filter(|(_, run)| *run).map(|(job, _)| job.clone());
        self.send_host(&aaa_proto::LinkMsg::Res {
            id: request_id,
            result: result.map(|(job, _)| self.deployments.response(job)).into(),
        })
        .await;
        if let Some(job) = run {
            tokio::spawn(async move {
                if let Some(job) = self.deployments.execute(self.exe.clone(), job).await {
                    self.send_host(&aaa_proto::LinkMsg::Event(aaa_proto::AgentEvent::DeploymentChanged { job })).await;
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn execute(exe: &Path, _: DeployAction) -> Result<String, String> {
        Ok(exe.display().to_string())
    }
    #[tokio::test]
    async fn jobs_are_durable_once_and_reconcile_only_after_the_executor_releases_its_lock() {
        let root = std::env::temp_dir().join(format!("aaa-deploy-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let exe = root.join("aaa");
        std::fs::write(&exe, b"original").unwrap();
        let manager = Manager::new(&root, &exe).unwrap();
        *manager.executor.lock().unwrap() = Some(execute);
        let id = aaa_common::util::new_id();
        let action = DeployAction::Update { from: Some("candidate".into()), version: None, dry_run: false };
        let (job, run) = manager.prepare(&exe, id.clone(), "demo".into(), action.clone()).unwrap();
        assert!(run);
        assert!(!manager.prepare(&exe, id.clone(), "demo".into(), action.clone()).unwrap().1);
        assert!(
            manager
                .prepare(&exe, id.clone(), "demo".into(), DeployAction::Restart { service: DeployService::Host })
                .is_err()
        );
        assert!(manager.prepare(&exe, aaa_common::util::new_id(), "demo".into(), action.clone()).is_err());
        let finished = manager.execute(exe.clone(), job).await.unwrap();
        assert_eq!(finished.state, DeployState::Completed);
        assert_eq!(manager.status(&id).unwrap(), finished);
        assert!(!manager.prepare(&exe, id, "demo".into(), action).unwrap().1, "완료를 다시 실행했다");

        let id = aaa_common::util::new_id();
        let (mut restart, _) = manager
            .prepare(&exe, id.clone(), "demo".into(), DeployAction::Restart { service: DeployService::Agent })
            .unwrap();
        restart.state = DeployState::Running;
        manager.save(&restart).unwrap();
        assert!(Manager::new(&root, &exe).is_err(), "살아 있는 실행기와 중복으로 열었다");
        // PID는 여전히 살아 있어도 OS 잠금이 풀렸으면 실행기는 끝났다(PID 재사용에도 같다).
        assert_eq!(restart.executor_pid, std::process::id());
        drop(manager);
        let restored = Manager::new(&root, &exe).unwrap();
        assert_eq!(restored.status(&id).unwrap().state, DeployState::Completed);
        restart.expected_sha256 = "다른 파일".into();
        restored.save(&restart).unwrap();
        drop(restored);
        let wrong = Manager::new(&root, &exe).unwrap();
        assert_eq!(wrong.status(&id).unwrap().state, DeployState::Interrupted);
        restart.action = DeployAction::Update { from: None, version: None, dry_run: false };
        wrong.save(&restart).unwrap();
        drop(wrong);
        let interrupted = Manager::new(&root, &exe).unwrap();
        assert_eq!(
            interrupted.status(&id).unwrap().state,
            DeployState::Interrupted,
            "중단된 교체를 다시 실행하지 않는다"
        );
        assert!(interrupted.status("../escape").is_err());
        drop(interrupted);
        std::fs::remove_dir_all(root).unwrap();
    }
}
