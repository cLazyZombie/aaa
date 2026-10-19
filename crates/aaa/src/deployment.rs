//! CLI는 출처 판정을 host에 맡긴다. 승인된 agent backend만 실행 함수를 부른다.
use aaa_proto::{DeployAction, FrontRequest, FrontResponse};
use anyhow::{Result, bail};
use std::process::ExitCode;

pub(crate) fn route(json: bool, machine: Option<String>, action: DeployAction) -> Result<Option<ExitCode>> {
    let declared = aaa_agent::hooks::in_session();
    let rt = tokio::runtime::Runtime::new()?;
    let result = rt.block_on(aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir()));
    let mut c = match result {
        Ok(c) => c,
        Err(e) if declared || machine.is_some() => {
            bail!("aaa 세션 안 배포는 host의 coordinator safety 확인이 필요하다: {e:#}")
        }
        Err(_) => return Ok(None),
    };
    if !c.role.can_control() {
        bail!("aaa 세션 안에서는 허용된 coordinator만 배포한다. thread는 계속 막는다");
    }
    if c.proto.minor < 42 {
        if declared || machine.is_some() {
            bail!("이 host는 coordinator 배포 API가 없다. 먼저 사람이 host·agent binary를 반영한다");
        }
        return Ok(None);
    }
    let context = rt.block_on(c.call(FrontRequest::DeployContext))?.map_err(|e| anyhow::anyhow!(e.message))?;
    match context {
        FrontResponse::DeployContext { project: Some(_), .. } if c.role.can_control() => {
            let id = aaa_common::util::new_id();
            let target = machine.as_deref().map(|m| format!(" --machine {m}")).unwrap_or_default();
            eprintln!(
                "배포 작업 ID: {id}. 응답이 끊기면 다시 실행하지 말고 `aaa update status {id}{target}`로 확인한다"
            );
            let result = rt.block_on(c.call(FrontRequest::DeployStart { machine, id, action }))?;
            crate::cli::front_print(json, result).map(Some)
        }
        FrontResponse::DeployContext { session: true, .. } => {
            bail!("aaa 세션 안에서는 허용된 coordinator만 배포한다. thread·임의 pane은 계속 막는다")
        }
        _ if declared || !c.role.can_control() => bail!("aaa 세션 안에서는 허용된 coordinator만 배포한다"),
        _ if machine.is_some() => bail!("원격 배포는 safety가 허용한 coordinator 세션에서 한다"),
        _ => Ok(None),
    }
}

pub(crate) fn execute(exe: &std::path::Path, action: DeployAction) -> Result<String, String> {
    let result: Result<String> = (|| {
        match action {
            DeployAction::Update { from, version, dry_run } => {
                crate::update::update_approved(
                    true,
                    from.map(Into::into),
                    version,
                    dry_run,
                    crate::update::Approval::Coordinator,
                    Some(exe.to_path_buf()),
                )?;
            }
            DeployAction::Rollback { name, dry_run } => {
                crate::update::rollback_approved(
                    true,
                    name,
                    dry_run,
                    crate::update::Approval::Coordinator,
                    Some(exe.to_path_buf()),
                )?;
            }
            DeployAction::Restart { service } => {
                return crate::launchd::restart_authorized(service.as_str());
            }
        }
        crate::update::installed_identity(exe)
    })();
    result.map_err(|e| format!("{e:#}"))
}

/// 사건을 기다린다. 재시작으로 연결이 끊기면 같은 ID의 결과 확인을 다시 요청하며, 작업 자체는 재실행하지 않는다.
pub(crate) fn status(json: bool, machine: Option<String>, id: String, wait: bool) -> Result<ExitCode> {
    use aaa_proto::{DeployState, FrontEvent, FrontMsg};
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let mut c = aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir()).await?;
        if c.proto.minor < 42 {
            bail!("이 host는 배포 상태 API가 없다");
        }
        loop {
            let response = c.call(FrontRequest::DeployStatus { machine: machine.clone(), id: id.clone() }).await?;
            let (target, job) = match &response {
                Ok(FrontResponse::Deployment { machine, job }) => (machine.clone(), job),
                _ => return crate::cli::front_print(json, response),
            };
            if !wait || !matches!(job.state, DeployState::Queued | DeployState::Running) {
                let failed = matches!(job.state, DeployState::Failed | DeployState::Interrupted);
                crate::cli::front_print(json, response)?;
                return Ok(if failed { ExitCode::from(1) } else { ExitCode::SUCCESS });
            }
            loop {
                match c.next().await? {
                    Some(FrontMsg::Event(FrontEvent::Flow(ev)))
                        if ev.kind == "deployment_finished" && ev.detail == id =>
                    {
                        break;
                    }
                    Some(FrontMsg::Event(FrontEvent::State { machines, .. }))
                        if machines
                            .iter()
                            .any(|m| m.name == target && m.connection == aaa_proto::Connection::Connected) =>
                    {
                        break;
                    }
                    None => bail!("배포 상태 연결이 끊겼다. 작업을 다시 보내지 말고 같은 ID {id}의 상태를 확인한다"),
                    _ => {}
                }
            }
        }
    })
}
