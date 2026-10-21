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
            // 재시작은 독립 감시가 새 프로세스를 확인하는 재시작 rollout으로 한다(FRONT_API 1.100, 관리 GUI W6c)
            if let DeployAction::Restart { service } = action
                && c.proto.minor >= 100
            {
                eprintln!(
                    "재시작 rollout ID: {id}. 응답이 끊기면 다시 실행하지 말고 `aaa deploy status {id} --wait`로 확인한다"
                );
                let target = machine.as_deref().map(|m| format!(" --machine {m}")).unwrap_or_default();
                let spec = aaa_proto::RolloutSpec {
                    from: String::new(),
                    source_machine: None,
                    sha256: String::new(),
                    commit: None,
                    machines: machine.into_iter().collect(),
                    restart: Some(service),
                };
                let result = rt.block_on(c.call(FrontRequest::RolloutStart { id: id.clone(), spec }))?;
                // 개별 update·rollback이 설치 파일을 바꾼 뒤면 host는 그 파일을 띄우는 개별 재시작(감시 밖)으로 보낸다
                if matches!(result, Ok(FrontResponse::Deployment { .. })) {
                    eprintln!(
                        "설치 파일이 실행 판과 달라 감시 밖 개별 재시작으로 보냈다. `aaa update status {id}{target} --wait`로 확인한다"
                    );
                }
                return crate::cli::front_print(json, result).map(Some);
            }
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
            DeployAction::UpdateStaged { .. } => bail!("staged 후보는 agent가 자기 상태 폴더에서만 해석한다"),
            DeployAction::Update { from, version, dry_run } => {
                crate::update::update_approved(
                    true,
                    from.map(Into::into),
                    version,
                    dry_run,
                    crate::update::Approval::Coordinator,
                    Some(exe.to_path_buf()),
                    None,
                )?;
            }
            DeployAction::CheckedUpdate { from, sha256, dry_run } => {
                // 검증한 staging 영수증을 쓴다. 원본 경로를 다시 실행하지 않는다.
                return crate::update::update_approved(
                    true,
                    Some(from.into()),
                    None,
                    dry_run,
                    crate::update::Approval::Coordinator,
                    Some(exe.to_path_buf()),
                    Some(&sha256),
                );
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
            DeployAction::CheckedRollback { sha256 } => return crate::update::checked_rollback(exe, &sha256),
            DeployAction::Restart { service } => {
                let service_state = crate::launchd::restart_authorized(service.as_str())?;
                let identity = crate::update::installed_identity(exe)?;
                return Ok(serde_json::json!({"service": service_state, "installed": serde_json::from_str::<serde_json::Value>(&identity)?}).to_string());
            }
        }
        crate::update::installed_identity(exe)
    })();
    result.map_err(|e| format!("{e:#}"))
}

/// guard가 승인된 옛 사본을 보존하므로 일반 보관 정리와 독립적으로 복구한다.
pub(crate) fn execute_recovery(
    exe: &std::path::Path,
    action: DeployAction,
    pin: &std::path::Path,
) -> Result<String, String> {
    match action {
        DeployAction::CheckedRollback { sha256 } => {
            crate::update::checked_recovery(exe, &sha256, pin).map_err(|e| format!("{e:#}"))
        }
        action => execute(exe, action),
    }
}

/// 사건을 기다린다. 재시작으로 연결이 끊기면 같은 ID의 결과 확인을 다시 요청하며, 작업 자체는 재실행하지 않는다.
pub(crate) fn status(json: bool, machine: Option<String>, id: String, wait: bool) -> Result<ExitCode> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let c = aaa_common::front::FrontClient::connect(&aaa_config::paths::state_dir()).await?;
        status_on(c, json, machine, id, wait).await
    })
}

async fn status_on(
    mut c: aaa_common::front::FrontClient,
    json: bool,
    machine: Option<String>,
    id: String,
    wait: bool,
) -> Result<ExitCode> {
    use aaa_proto::{DeployState, FrontEvent, FrontMsg};
    if c.proto.minor < 42 {
        bail!("이 host는 배포 상태 API가 없다");
    }
    loop {
        let response = c.call(FrontRequest::DeployStatus { machine: machine.clone(), id: id.clone() }).await?;
        let target = match &response {
            Ok(FrontResponse::Deployment { machine, job, .. }) => {
                if !wait || !matches!(job.state, DeployState::Queued | DeployState::Running) {
                    let failed = matches!(job.state, DeployState::Failed | DeployState::Interrupted);
                    crate::cli::front_print(json, response)?;
                    return Ok(if failed { ExitCode::from(1) } else { ExitCode::SUCCESS });
                }
                Some(machine.clone())
            }
            // 권한 오류는 숨기지 않는다. 일시적인 agent 단절만 같은 ID의 재연결 사건을 기다린다.
            Err(e) if wait && e.code == aaa_proto::ErrorCode::AgentDisconnected => machine.clone(),
            _ => return crate::cli::front_print(json, response),
        };
        loop {
            match c.next().await? {
                Some(FrontMsg::Event(FrontEvent::Flow(ev))) if ev.kind == "deployment_finished" && ev.detail == id => {
                    break;
                }
                Some(FrontMsg::Event(FrontEvent::State { machines, .. }))
                    if machines.iter().any(|m| {
                        m.connection == aaa_proto::Connection::Connected
                            && target.as_ref().is_none_or(|t| m.name == *t || m.id == *t)
                    }) =>
                {
                    break;
                }
                None => bail!("배포 상태 연결이 끊겼다. 작업을 다시 보내지 말고 같은 ID {id}의 상태를 확인한다"),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_common::{
        conn::{FrameRx, spawn_writer},
        front::{FrontClient, socket},
        ipc::LocalListener,
    };
    use aaa_proto::*;

    #[tokio::test]
    async fn status_waits_for_reconnection_but_preserves_nonwait_and_forbidden_errors() {
        for (code, wait, machine) in [
            (ErrorCode::AgentDisconnected, true, Some("m")),
            (ErrorCode::AgentDisconnected, true, None),
            (ErrorCode::AgentDisconnected, false, Some("mac-a")),
            (ErrorCode::Forbidden, true, Some("mac-a")),
        ] {
            let root = std::env::temp_dir().join(format!("dw-{}", aaa_common::util::new_id()));
            // Windows Named Pipe는 폴더를 만들지 않으므로 시험이 직접 만들고 정리한다.
            std::fs::create_dir(&root).unwrap();
            let listener = LocalListener::bind(&socket(&root)).unwrap();
            let target = machine.map(str::to_string);
            let expected = FrontRequest::DeployStatus { machine: target.clone(), id: "job".into() };
            let completes = code == ErrorCode::AgentDisconnected && wait;
            let server = tokio::spawn(async move {
                let stream = listener.accept().await.unwrap();
                let (r, w) = tokio::io::split(stream);
                let tx = spawn_writer(w, MAX_FRAME);
                let mut rx = FrameRx::new(r, MAX_FRAME);
                assert!(matches!(rx.control::<FrontMsg>().await.unwrap(), FrontMsg::Hello { .. }));
                tx.json(&FrontMsg::Welcome {
                    proto: FRONT_API,
                    host_id: "host".into(),
                    role: DeviceRole::Local,
                    front: "f".into(),
                    warnings: vec![],
                })
                .await
                .unwrap();
                let FrontMsg::Req { id, body } = rx.control::<FrontMsg>().await.unwrap() else {
                    panic!("상태 요청")
                };
                assert_eq!(serde_json::to_value(body).unwrap(), serde_json::to_value(&expected).unwrap());
                tx.json(&FrontMsg::Res {
                    id,
                    result: Outcome::Err(ErrorBody::new(code, "test disconnect or forbidden")),
                })
                .await
                .unwrap();
                if completes {
                    // 응답을 받는 동안 도착한 연결 사건도 FrontClient가 보존해야 한다.
                    let m = MachineView {
                        pr_tracking: None,
                        id: "m".into(),
                        name: "mac-a".into(),
                        local: true,
                        connection: Connection::Connected,
                        os: None,
                        boot_id: None,
                        dev_mode: true,
                        last_seen_ms: None,
                        health: None,
                        clock_skew_ms: None,
                        problems: vec![],
                        arch: None,
                        software: None,
                        sha256: None,
                        software_differs: false,
                        features: vec![],
                        selfcheck: None,
                    };
                    tx.json(&FrontMsg::Event(FrontEvent::State { machines: vec![m], panes: vec![] })).await.unwrap();
                    let FrontMsg::Req { id, body } = rx.control::<FrontMsg>().await.unwrap() else {
                        panic!("같은 상태 재조회")
                    };
                    assert_eq!(
                        serde_json::to_value(body).unwrap(),
                        serde_json::to_value(&expected).unwrap(),
                        "작업 시작을 다시 보내면 안 된다"
                    );
                    let job = DeployJob {
                        id: "job".into(),
                        project: "demo".into(),
                        action: DeployAction::Restart { service: DeployService::Agent },
                        state: DeployState::Completed,
                        instance: "new-agent".into(),
                        executor_pid: 1,
                        expected_sha256: "sha".into(),
                        result: None,
                        created_ms: 0,
                        updated_ms: 1,
                    };
                    tx.json(&FrontMsg::Res {
                        id,
                        result: Outcome::Ok(FrontResponse::Deployment {
                            machine: "mac-a".into(),
                            job,
                            running_software: "test".into(),
                            running_sha256: "sha".into(),
                        }),
                    })
                    .await
                    .unwrap();
                }
            });
            let c = FrontClient::connect_as(&root, "test").await.unwrap();
            let result = status_on(c, true, target, "job".into(), wait).await.unwrap();
            assert_eq!(result, if completes { ExitCode::SUCCESS } else { ExitCode::from(1) }, "{code:?}, wait={wait}");
            server.await.unwrap();
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
