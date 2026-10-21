//! 첫 Controller도 기존 W0 pairing으로만 등록한다. signed install 권한으로 기기 권한을 만들지 않는다.
use super::*;
use aaa_common::front::FrontClient;
use aaa_proto::{DeviceRole, FrontRequest, FrontResponse};
use worker::private_display::{Request, Subscriber};
fn required() -> Fault {
    Fault { code: "installation_first_device_pairing_required", exit: 3 }
}
fn wants_pairing(plan: &InstallPlan) -> bool {
    plan.choices.pairing == Some(InstallPairing::Controller)
}
async fn client(plan: &InstallPlan) -> Result<FrontClient> {
    tokio::time::timeout(std::time::Duration::from_secs(10), FrontClient::connect(Path::new(&plan.context.state_dir)))
        .await
        .map_err(|_| required())?
        .map_err(|_| required())
}
async fn has_controller(client: &mut FrontClient) -> Result<bool> {
    match tokio::time::timeout(std::time::Duration::from_secs(10), client.call(FrontRequest::Devices)).await {
        Ok(Ok(Ok(FrontResponse::Devices { devices }))) => {
            Ok(devices.iter().any(|device| device.role == DeviceRole::Controller))
        }
        _ => Err(required()),
    }
}
fn wants_publish(plan: &InstallPlan) -> bool {
    plan.choices.tailscale_serve == Some(InstallServe::On)
}
fn publish_ports(plan: &InstallPlan) -> Result<(u16, u16)> {
    let port = plan.choices.serve_port.ok_or_else(Fault::conflict)?;
    let target = plan
        .choices
        .web_listen
        .as_deref()
        .unwrap_or("127.0.0.1:7431")
        .parse::<std::net::SocketAddr>()
        .map_err(|_| Fault::conflict())?
        .port();
    Ok((port, target))
}
fn publish_fault(error: aaa_host::tailscale::ExactError) -> Fault {
    Fault { code: error.code, exit: if error.conflict { 4 } else { 3 } }
}
pub(super) fn permissions(plan: &InstallPlan) -> Result<Vec<InstallPermission>> {
    if !wants_publish(plan) {
        return Ok(Vec::new());
    }
    let (port, target) = publish_ports(plan)?;
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(Fault::io)?;
        let native = rt.block_on(aaa_host::tailscale::conditional_snapshot(target, port)).map_err(publish_fault)?;
        Ok(vec![native.snapshot.permission().map_err(publish_fault)?])
    })
    .join()
    .map_err(|_| Fault::conflict())?
}
fn publication_error(error: aaa_proto::ErrorBody) -> Fault {
    if error.code == aaa_proto::ErrorCode::Conflict {
        return Fault { code: "serve_port_conflict", exit: 4 };
    }
    let code = match error.message.as_str() {
        "tailscale_conditional_apply_unavailable" => "tailscale_conditional_apply_unavailable",
        "tailscale_serve_permission_required" => "tailscale_serve_permission_required",
        "tailscale_serve_setup_required" => "tailscale_serve_setup_required",
        _ => "tailscale_publication_unverified",
    };
    Fault { code, exit: 3 }
}
async fn publication(plan: &InstallPlan, id: &str, writing: bool) -> Result<InstallPublication> {
    let request = if writing {
        FrontRequest::InstallPublish { installation_id: id.into() }
    } else {
        FrontRequest::InstallPublishStatus { installation_id: id.into() }
    };
    let action = async {
        let mut client = FrontClient::connect(Path::new(&plan.context.state_dir))
            .await
            .map_err(|_| Fault { code: "installation_host_unavailable", exit: 3 })?;
        // 이 요청이 처음 추가된 판을 고정한다. 미래 minor와 비교하지 않는다.
        if client.proto.major != 1 || client.proto.minor < 81 {
            return Err(Fault { code: "installation_host_upgrade_required", exit: 3 });
        }
        match client.call(request).await {
            Ok(Ok(FrontResponse::InstallPublished { publication })) => Ok(publication),
            Ok(Err(error)) => Err(publication_error(error)),
            _ => Err(Fault { code: "tailscale_publication_unverified", exit: 3 }),
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(600), action)
        .await
        .map_err(|_| Fault { code: "tailscale_publication_unverified", exit: 3 })?
}
pub(super) async fn publish(plan: &InstallPlan, id: &str) -> Result<bool> {
    if !wants_publish(plan) {
        return Ok(false);
    }
    let publication = publication(plan, id, true).await?;
    if publication.mapping != InstallCheckState::Passed || publication.public_https != InstallCheckState::Passed {
        return Err(Fault { code: "tailscale_public_https_unverified", exit: 3 });
    }
    Ok(!publication.reused)
}
pub(super) async fn pairing(plan: &InstallPlan, subscriber: &mut Option<Subscriber>) -> Result<bool> {
    if !wants_pairing(plan) {
        return Ok(false);
    }
    let mut client = client(plan).await?;
    if has_controller(&mut client).await? {
        return Ok(false);
    }
    let subscriber = subscriber.as_mut().ok_or_else(required)?;
    subscriber.request(Request::Pairing).await.map_err(|_| required())?;
    // 표시 ACK는 완료가 아니다. 실제 redeem의 host 상태 사건 뒤 Devices를 다시 읽는다.
    let closed = subscriber.closed();
    let wait = async {
        tokio::pin!(closed);
        loop {
            if has_controller(&mut client).await? {
                return Ok(true);
            }
            tokio::select! {
                _ = &mut closed => return Err(required()),
                event = client.next() => if !event.is_ok_and(|event| event.is_some()) { return Err(required()); },
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(600), wait).await.map_err(|_| required())?
}
pub(super) async fn observe(plan: &InstallPlan, id: &str, result: &mut InstallResult) -> Result<()> {
    if wants_publish(plan) {
        let publication = publication(plan, id, false).await;
        if let Err(fault) = publication.as_ref()
            && fault.exit == 4
        {
            return Err(*fault);
        }
        crate::install::summary::populate(plan, result);
        if let (Some(summary), Ok(observed)) = (result.web.as_mut(), publication.as_ref()) {
            summary.tailscale_serve.reused = Some(observed.reused);
            summary.tailscale_serve.ownership =
                if observed.owned { InstallOwnership::ThisInstance } else { InstallOwnership::External };
        }
        let mapping = publication.as_ref().is_ok_and(|p| p.mapping == InstallCheckState::Passed);
        let https = publication.as_ref().is_ok_and(|p| p.public_https == InstallCheckState::Passed);
        result.checks.push(InstallCheck {
            name: "tailscale_exact_mapping".into(),
            state: if mapping { InstallCheckState::Passed } else { InstallCheckState::Unknown },
        });
        result.checks.push(InstallCheck {
            name: "public_https".into(),
            state: if https { InstallCheckState::Passed } else { InstallCheckState::Unknown },
        });
        if (!mapping || !https) && result.exit_code == 0 {
            result.exit_code = 3;
            result.status = InstallStatus::NeedsUser;
            result.error = None;
            result.needs_user.push(InstallNeedsUser {
                code: "tailscale_public_https_unverified".into(),
                target: "web".into(),
                message: "선택한 포트의 mapping과 현재 host HTTPS 응답을 확인한 뒤 같은 ID로 재개하세요".into(),
            });
        }
    }
    if !wants_pairing(plan) {
        return Ok(());
    }
    let ready = match client(plan).await {
        Ok(mut client) => has_controller(&mut client).await.unwrap_or(false),
        Err(_) => false,
    };
    result.checks.push(InstallCheck {
        name: "first_controller_pairing".into(),
        state: if ready { InstallCheckState::Passed } else { InstallCheckState::NotRun },
    });
    if !ready && result.exit_code == 0 {
        result.exit_code = 3;
        result.status = InstallStatus::NeedsUser;
        result.error = None;
        result.needs_user.push(InstallNeedsUser {
            code: required().code.into(),
            target: "pairing".into(),
            message: "세션 밖 터미널의 첫 Controller pairing을 실제 기기에서 마친 뒤 같은 ID로 재개하세요".into(),
        });
    }
    Ok(())
}
/// CLI 읽기 status는 실행 runtime과 분리해 네트워크 조회를 한다. 새 code/token/journal은 만들지 않는다.
pub(super) fn observe_readonly(plan: &InstallPlan, id: &str, result: &mut InstallResult) -> Result<()> {
    if !wants_pairing(plan) && !wants_publish(plan) {
        return Ok(());
    }
    let id = id.to_owned();
    let plan = plan.clone();
    let mut snapshot = result.clone();
    *result = std::thread::spawn(move || -> Result<InstallResult> {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(Fault::io)?;
        rt.block_on(observe(&plan, &id, &mut snapshot))?;
        Ok(snapshot)
    })
    .join()
    .map_err(|_| required())??;
    Ok(())
}
