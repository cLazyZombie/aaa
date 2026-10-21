//! coordinator 배포 문. host가 정한 pane 소속과 매번 다시 읽은 safety만 믿는다.
//! 웹 조작 기기는 서비스 재시작만 연다(관리 GUI W6a). 실행 파일 교체는 rollout으로만 한다.
use crate::{Host, err};
use aaa_proto::{
    AgentRequest, AgentResponse, DeployAction, DeployJob, DeployJobCard, DeployState, DeployWork, DeviceRole,
    ErrorBody, ErrorCode, FrontResponse,
};
use std::time::Duration;

/// 웹 「배포」의 최근 기록 수
pub(crate) const RECENT: usize = 10;
/// 웹 조작 기기의 연결인가(보기 전용 기기·세션 안 연결은 아니다)
pub(crate) fn web_controller(role: DeviceRole, actor: &str) -> bool {
    role == DeviceRole::Controller && device(actor)
}
/// 웹 기기 actor. 시작할 때 조작 기기만 받았으므로 기록의 이 actor는 웹 승인이다
pub(crate) fn device(actor: &str) -> bool {
    actor.strip_prefix("device:").is_some_and(|id| !id.is_empty())
}
fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}
/// 기록·응답을 화면용 요약으로 줄인다. 실행기 PID·결과 원문(경로·self-check 출력)은 싣지 않는다
pub(crate) fn job_card(
    id: String,
    machine: String,
    record: &serde_json::Value,
    job: Option<&DeployJob>,
) -> DeployJobCard {
    let created = record["created_ms"].as_u64().unwrap_or(0);
    DeployJobCard {
        id,
        machine,
        work: job
            .map(|j| DeployWork::from(&j.action))
            .or_else(|| serde_json::from_value::<DeployAction>(record["action"].clone()).ok().map(|a| (&a).into()))
            .unwrap_or(DeployWork::Update),
        state: job.map(|j| j.state),
        error: job.and_then(|j| match j.state {
            DeployState::Failed => {
                Some("작업이 실패했습니다. 상세 진단은 coordinator의 배포 기록에서 확인하세요.".into())
            }
            DeployState::Interrupted => {
                Some("작업이 도중에 끝났습니다(다시 실행하지 않았습니다). 실행 파일과 서비스 상태를 확인하세요.".into())
            }
            _ => None,
        }),
        web: record["web"] == true,
        created_ms: job.map(|j| j.created_ms).unwrap_or(created),
        updated_ms: job.map(|j| j.updated_ms).unwrap_or(created),
    }
}
impl Host {
    pub(crate) fn coordinator_project_of_actor(&self, actor: &str) -> Option<String> {
        let pane = actor.strip_prefix("local:")?.rsplit_once('@')?.1;
        // thread adopt는 pane에 role이 없어도 thread 기록이 우선한다.
        if self.thread_of_pane(pane).is_some() {
            return None;
        }
        // 전송 연결과 소속은 별개다. peer PID로 확인한 pane의 host 정본 metadata를 쓴다.
        // coordinator_panes/running_coordinators는 입력 가능한 연결만 골라 재시작 창에 쓰면 안 된다.
        let pane = self.pane_view_of(pane, aaa_proto::DeviceRole::Local)?;
        pane.state.is_running().then_some(pane.coordinator_project).flatten()
    }
    pub(crate) fn deployment_context(&self, actor: &str) -> FrontResponse {
        let project = self.coordinator_project_of_actor(actor);
        let allowed = project.as_deref().is_some_and(|p| self.safety(p).coordinator_deploy);
        let session = actor.contains('@')
            || actor.starts_with("worker:")
            || actor.starts_with("evaluator:")
            || actor.starts_with("detached:");
        FrontResponse::DeployContext { project, session, allowed }
    }
    pub(crate) fn deployment_authorize(&self, actor: &str) -> Result<String, ErrorBody> {
        let p = self.coordinator_project_of_actor(actor).ok_or_else(|| {
            err(
                ErrorCode::Forbidden,
                "배포는 이 프로젝트의 coordinator 세션에서만 요청한다. thread·임의 pane·웹 기기는 허용하지 않는다",
            )
        })?;
        if !self.safety(&p).coordinator_deploy {
            return Err(err(
                ErrorCode::Forbidden,
                "coordinator 배포가 꺼져 있다. 사람이 aaa 밖 터미널에서 `aaa safety set coordinator_deploy on`으로 허용한다",
            ));
        }
        Ok(p)
    }
    pub(crate) async fn deployment_start(
        &self,
        role: DeviceRole,
        actor: &str,
        machine: Option<String>,
        id: String,
        action: DeployAction,
    ) -> Result<FrontResponse, ErrorBody> {
        let web = web_controller(role, actor);
        let project = if web {
            if !matches!(action, DeployAction::Restart { .. }) {
                return Err(err(
                    ErrorCode::Forbidden,
                    "웹은 서비스 재시작만 한다. 실행 파일 교체는 설정 「배포」의 rollout으로 한다",
                ));
            }
            self.instance_project()?
        } else {
            self.deployment_authorize(actor)?
        };
        if id.len() != 26 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(err(ErrorCode::BadRequest, "배포 작업 ID 형식이 아니다"));
        }
        let mid = self.resolve_machine(machine.as_deref())?;
        if matches!(action, DeployAction::CheckedRollback { .. }) && !self.machine_feature(&mid, "rollout_recovery.v1")
        {
            return Err(err(ErrorCode::UnsupportedVersion, "SHA 고정 복구에는 rollout_recovery.v1이 필요하다"));
        }
        if matches!(action, DeployAction::UpdateStaged { .. }) && !self.machine_feature(&mid, "deploy_stage.v1") {
            return Err(err(ErrorCode::UnsupportedVersion, "staged 교체에는 deploy_stage.v1이 필요하다"));
        }
        if !self.machine_feature(&mid, "deployment.v1") {
            return Err(err(ErrorCode::BadRequest, "이 agent는 배포 API가 없다. 먼저 사람이 agent binary를 반영한다"));
        }
        // host 재시작으로 답이 끊겨도 작업 ID와 대상은 남는다. 이 기록으로 자동 재실행하지 않는다.
        {
            let _gate = self.deployment_gate.lock().unwrap();
            if self.rollout_busy()? {
                return Err(err(ErrorCode::BadRequest, "rollout 진행 중에는 개별 배포를 시작하지 않는다"));
            }
            let key = format!("deploy:{id}");
            let request = serde_json::json!({"project":project,"machine":mid,"action":action});
            match self.db.meta(&key).map_err(|e| err(ErrorCode::Internal, e.to_string()))? {
                // 같은 요청인지는 대상·작업만 본다. 시각·출처는 처음 기록 그대로 둔다
                Some(old) => {
                    let old: serde_json::Value = serde_json::from_str(&old).unwrap_or_default();
                    if ["project", "machine", "action"].iter().any(|k| old[k] != request[k]) {
                        return Err(err(ErrorCode::BadRequest, "같은 배포 ID를 다른 작업에 쓸 수 없다"));
                    }
                }
                None => {
                    let mut record = request;
                    record["created_ms"] = aaa_common::util::now_ms().into();
                    record["web"] = web.into();
                    self.db.set_meta(&key, &record.to_string()).map_err(|e| err(ErrorCode::Internal, e.to_string()))?
                }
            }
        }
        self.db.audit(actor, "deployment_start", &format!("project={project} machine={mid} job={id}"));
        let response =
            self.live_conn(&mid)?.request(AgentRequest::DeployStart { id: id.clone(), project, action }).await?;
        match response {
            AgentResponse::Deployment { job, .. } if web => Ok(FrontResponse::DeployJobs {
                jobs: vec![job_card(id.clone(), self.machine_name(&mid), &self.deploy_record(&id)?, Some(&job))],
            }),
            AgentResponse::Deployment { job, running_software, running_sha256, .. } => Ok(FrontResponse::Deployment {
                machine: self.machine_name(&mid),
                job,
                running_software,
                running_sha256,
            }),
            _ => Err(err(ErrorCode::Internal, "배포 응답이 아니다")),
        }
    }
    fn deploy_record(&self, id: &str) -> Result<serde_json::Value, ErrorBody> {
        self.db
            .meta(&format!("deploy:{id}"))
            .map_err(internal)?
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .ok_or_else(|| err(ErrorCode::NotFound, "배포 기록이 없다"))
    }
    pub(crate) async fn deployment_status(
        &self,
        role: DeviceRole,
        actor: &str,
        machine: Option<String>,
        id: String,
    ) -> Result<FrontResponse, ErrorBody> {
        // 상태도 자기 coordinator 범위에서 본다. 실행 허용 철회 뒤에도 결과는 확인할 수 있다. 웹 조작 기기는 이 aaa의 프로젝트다.
        let web = web_controller(role, actor);
        let project = if web {
            self.instance_project()?
        } else {
            self.coordinator_project_of_actor(actor)
                .ok_or_else(|| err(ErrorCode::Forbidden, "배포 기록은 coordinator 세션에서 확인한다"))?
        };
        let record = self
            .db
            .meta(&format!("deploy:{id}"))
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .filter(|r| r["project"] == project)
            .ok_or_else(|| err(ErrorCode::NotFound, "이 프로젝트의 배포 기록이 없다"))?;
        let mid =
            record["machine"].as_str().ok_or_else(|| err(ErrorCode::Internal, "배포 대상 기록이 없다"))?.to_string();
        if let Some(machine) = machine
            && self.resolve_machine(Some(&machine))? != mid
        {
            return Err(err(ErrorCode::BadRequest, "기록된 배포 대상과 다르다"));
        }
        if !self.machine_feature(&mid, "deployment.v1") {
            return Err(err(
                ErrorCode::BadRequest,
                "이 agent는 배포 상태 API가 없다. 머신 판·SHA와 재연결을 직접 확인한다",
            ));
        }
        match self.live_conn(&mid)?.request(AgentRequest::DeployStatus { id: id.clone() }).await? {
            AgentResponse::Deployment { job, .. } if web && job.project == project => {
                Ok(FrontResponse::DeployJobs { jobs: vec![job_card(id, self.machine_name(&mid), &record, Some(&job))] })
            }
            AgentResponse::Deployment { job, running_software, running_sha256, .. } if job.project == project => {
                Ok(FrontResponse::Deployment {
                    machine: self.machine_name(&mid),
                    job,
                    running_software,
                    running_sha256,
                })
            }
            AgentResponse::Deployment { .. } => Err(err(ErrorCode::Forbidden, "다른 프로젝트의 배포 기록이다")),
            _ => Err(err(ErrorCode::Internal, "배포 응답이 아니다")),
        }
    }
    /// 웹 「배포」의 최근 개별 작업(새것부터 RECENT개). 상태는 그 머신 agent에게 묻고, 끊긴 머신은 상태 없이 둔다
    pub(crate) async fn deployment_jobs(&self) -> Result<FrontResponse, ErrorBody> {
        let project = self.instance_project()?;
        let mut records = Vec::new();
        for id in self.db.meta_suffixes("deploy:").map_err(internal)? {
            let Ok(record) = self.deploy_record(&id) else { continue };
            if record["project"] == project.as_str()
                && let Some(mid) = record["machine"].as_str().map(str::to_string)
            {
                records.push((record["created_ms"].as_u64().unwrap_or(0), id, mid, record));
            }
        }
        records.sort_by(|a, b| (b.0, &b.1).cmp(&(a.0, &a.1)));
        records.truncate(RECENT);
        let project = &project;
        let asks = records.into_iter().map(|(_, id, mid, record)| async move {
            let job = match self.live_conn(&mid) {
                Ok(conn) if self.machine_feature(&mid, "deployment.v1") => {
                    match conn
                        .request_timeout(AgentRequest::DeployStatus { id: id.clone() }, Duration::from_secs(5))
                        .await
                    {
                        Ok(AgentResponse::Deployment { job, .. }) if &job.project == project => Some(job),
                        _ => None,
                    }
                }
                _ => None,
            };
            job_card(id, self.machine_name(&mid), &record, job.as_ref())
        });
        Ok(FrontResponse::DeployJobs { jobs: futures_util::future::join_all(asks).await })
    }
    pub(crate) fn deployment_changed(&self, mid: &str, job: aaa_proto::DeployJob) {
        self.rollout_wake.notify_one();
        // 웹 「배포」의 작업 목록도 다시 읽는다(`rollout` 사건, 관리 GUI W6a)
        self.rollout_changed.send_modify(|v| *v += 1);
        let record = self
            .db
            .meta(&format!("deploy:{}", job.id))
            .ok()
            .flatten()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
        if record.as_ref().is_some_and(|r| {
            r["machine"] == mid
                && r["project"] == job.project
                && r["action"] == serde_json::to_value(&job.action).unwrap_or_default()
        }) {
            self.flow_project(&job.project, "deployment_finished", None, job.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{DeviceRole, FrontRequest, Role, SessionInfo, SessionState};

    #[test]
    fn coordinator_identity_survives_local_agent_disconnect() {
        let root = std::env::temp_dir().join(format!("aaa-deploy-caller-{}", aaa_common::util::new_id()));
        // Windows에서도 background task가 보유한 DB를 닫은 뒤 상태 폴더를 지운다.
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let mut cfg = aaa_config::HostConfig::default();
            cfg.projects.root = root.join("projects").to_string_lossy().into();
            cfg.notify.web_push = false;
            let h = Host::open(cfg, root.join("host")).unwrap();
            h.project_create("demo", Default::default(), "test").unwrap();
            let pane = SessionInfo {
                id: "coordinator".into(),
                name: "coordinator".into(),
                argv: vec!["codex".into()],
                cwd: Some(h.project_dir("demo").unwrap().to_string_lossy().into()),
                pid: Some(std::process::id()),
                started_at_ms: 0,
                boot_id: "boot".into(),
                cols: 80,
                rows: 24,
                state: SessionState::Running,
                role: Some(Role::Coordinator),
                label: None,
                harness: Some("codex".into()),
            };
            h.agent_connected(
                &aaa_proto::AgentHello {
                    proto: aaa_proto::AGENT_LINK,
                    machine_id: "m".into(),
                    machine_name: "mac-a".into(),
                    boot_id: "boot".into(),
                    os: "test".into(),
                    arch: "test".into(),
                    software: "test".into(),
                    features: vec!["deployment.v1".into()],
                    dev_mode: true,
                    sha256: None,
                    selfcheck: None,
                    sessions: vec![],
                    harness: vec![],
                },
                true,
                crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), aaa_proto::MAX_FRAME), 1),
            );
            h.on_session("m", pane.clone(), false);
            h.db.set_meta("deploy:job", r#"{"project":"demo","machine":"m"}"#).unwrap();
            let actor = "local:test@coordinator";
            let context = || h.call(DeviceRole::Local, actor, FrontRequest::DeployContext);
            assert!(
                matches!(context().await.unwrap(), FrontResponse::DeployContext { project: Some(p), .. } if p == "demo")
            );
            h.agent_disconnected("m", 1);
            assert_eq!(h.caller_of(Some(std::process::id())), crate::caller::Caller::Pane("coordinator".into()));
            assert!(
                matches!(context().await.unwrap(), FrontResponse::DeployContext { project: Some(p), .. } if p == "demo"),
                "연결 단절은 coordinator 소속을 없애지 않는다"
            );
            let status = || FrontRequest::DeployStatus { machine: Some("mac-a".into()), id: "job".into() };
            assert_eq!(h.call(DeviceRole::Local, actor, status()).await.unwrap_err().code, ErrorCode::AgentDisconnected);
            for actor in ["local:test@unverified", "worker:coordinator", "device:coordinator"] {
                assert_eq!(h.call(DeviceRole::Local, actor, status()).await.unwrap_err().code, ErrorCode::Forbidden);
            }
            // 같은 cwd의 일반 셸과 끝난 pane에는 캐시된 프로젝트 권한도 주지 않는다.
            let mut shell = pane.clone();
            shell.role = None;
            shell.harness = None;
            shell.argv = vec!["sh".into()];
            h.on_session("m", shell, false);
            assert_eq!(h.call(DeviceRole::Local, actor, status()).await.unwrap_err().code, ErrorCode::Forbidden);
            let mut exited = pane;
            exited.state = SessionState::Lost;
            h.on_session("m", exited, false);
            assert_eq!(h.call(DeviceRole::Local, actor, status()).await.unwrap_err().code, ErrorCode::Forbidden);
            drop(h);
        });
        std::fs::remove_dir_all(root).unwrap();
    }
}
