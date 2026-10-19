//! coordinator 배포 문. host가 정한 pane 소속과 매번 다시 읽은 safety만 믿는다.
use crate::{Host, err};
use aaa_proto::{AgentRequest, AgentResponse, DeployAction, ErrorBody, ErrorCode, FrontResponse};
impl Host {
    fn deployment_project(&self, actor: &str) -> Option<String> {
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
        let project = self.deployment_project(actor);
        let allowed = project.as_deref().is_some_and(|p| self.safety(p).coordinator_deploy);
        let session = actor.contains('@')
            || actor.starts_with("worker:")
            || actor.starts_with("evaluator:")
            || actor.starts_with("detached:");
        FrontResponse::DeployContext { project, session, allowed }
    }
    fn deployment_authorize(&self, actor: &str) -> Result<String, ErrorBody> {
        let p = self.deployment_project(actor).ok_or_else(|| {
            err(
                ErrorCode::Forbidden,
                "배포는 이 프로젝트의 coordinator 세션에서만 요청한다. thread·임의 pane·웹 기기는 허용하지 않는다",
            )
        })?;
        if !self.safety(&p).coordinator_deploy {
            return Err(err(
                ErrorCode::Forbidden,
                format!(
                    "coordinator 배포가 꺼져 있다. 사람이 aaa 밖 터미널에서 `aaa safety set {p} coordinator_deploy on`으로 허용한다"
                ),
            ));
        }
        Ok(p)
    }
    pub(crate) async fn deployment_start(
        &self,
        actor: &str,
        machine: Option<String>,
        id: String,
        action: DeployAction,
    ) -> Result<FrontResponse, ErrorBody> {
        let project = self.deployment_authorize(actor)?;
        if id.len() != 26 || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(err(ErrorCode::BadRequest, "배포 작업 ID 형식이 아니다"));
        }
        let mid = self.resolve_machine(machine.as_deref())?;
        if !self.machine_feature(&mid, "deployment.v1") {
            return Err(err(ErrorCode::BadRequest, "이 agent는 배포 API가 없다. 먼저 사람이 agent binary를 반영한다"));
        }
        // host 재시작으로 답이 끊겨도 작업 ID와 대상은 남는다. 이 기록으로 자동 재실행하지 않는다.
        {
            let _gate = self.deployment_gate.lock().unwrap();
            let key = format!("deploy:{id}");
            let record = serde_json::json!({"project":project,"machine":mid,"action":action}).to_string();
            match self.db.meta(&key).map_err(|e| err(ErrorCode::Internal, e.to_string()))? {
                Some(old) if old != record => {
                    return Err(err(ErrorCode::BadRequest, "같은 배포 ID를 다른 작업에 쓸 수 없다"));
                }
                Some(_) => {}
                None => self.db.set_meta(&key, &record).map_err(|e| err(ErrorCode::Internal, e.to_string()))?,
            }
        }
        self.db.audit(actor, "deployment_start", &format!("project={project} machine={mid} job={id}"));
        let response = self.live_conn(&mid)?.request(AgentRequest::DeployStart { id, project, action }).await?;
        match response {
            AgentResponse::Deployment { job, running_software, running_sha256 } => Ok(FrontResponse::Deployment {
                machine: self.machine_name(&mid),
                job,
                running_software,
                running_sha256,
            }),
            _ => Err(err(ErrorCode::Internal, "배포 응답이 아니다")),
        }
    }
    pub(crate) async fn deployment_status(
        &self,
        actor: &str,
        machine: Option<String>,
        id: String,
    ) -> Result<FrontResponse, ErrorBody> {
        // 상태도 자기 coordinator 범위에서 본다. 실행 허용 철회 뒤에도 결과는 확인할 수 있다.
        let project = self
            .deployment_project(actor)
            .ok_or_else(|| err(ErrorCode::Forbidden, "배포 기록은 coordinator 세션에서 확인한다"))?;
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
        match self.live_conn(&mid)?.request(AgentRequest::DeployStatus { id }).await? {
            AgentResponse::Deployment { job, running_software, running_sha256 } if job.project == project => {
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
    pub(crate) fn deployment_changed(&self, mid: &str, job: aaa_proto::DeployJob) {
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

    #[tokio::test]
    async fn coordinator_identity_survives_local_agent_disconnect() {
        let root = std::env::temp_dir().join(format!("aaa-deploy-caller-{}", aaa_common::util::new_id()));
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
        std::fs::remove_dir_all(root).unwrap();
    }
}
