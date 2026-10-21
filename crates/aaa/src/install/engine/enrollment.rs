//! 승인된 신원 준비와 설치 grant만 처리한다. 일반 등록 창/기기 권한은 열지 않는다.
use super::*;
use authorization::Lease;
mod files;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    schema: u32,
    installation_id: String,
    plan_sha256: String,
    account_sha256: String,
    complete: bool,
    agent_identity: Option<String>,
    enrolled: bool,
    host_identity: Option<String>,
    before: files::Snapshot,
    prepared: Option<files::Snapshot>,
    pin_intent: Option<String>,
}
fn required(code: &'static str) -> Fault {
    Fault { code, exit: 3 }
}
pub(super) fn permissions(plan: &InstallPlan) -> Result<Vec<InstallPermission>> {
    let mut permissions = scope::enrollment(plan);
    if let Some(permission) = permissions.first_mut() {
        permission.before_sha256 =
            Some(files::digest(&files::snapshot(&Path::new(&plan.context.state_dir).join("agent"))?)?);
    }
    Ok(permissions)
}
fn record_path(plan: &InstallPlan, id: &str) -> PathBuf {
    Path::new(&plan.context.state_dir).join("install/authorization").join(format!("{id}.identity.json"))
}
fn read_record(plan: &InstallPlan, lease: &Lease, path: &Path) -> Result<Prepared> {
    let record: Prepared =
        serde_json::from_slice(&private::read(path, 65536).map_err(Fault::io)?).map_err(|_| Fault::conflict())?;
    let goal = scope::enrollment(plan).into_iter().next().ok_or_else(Fault::conflict)?;
    let permission = lease.permission(plan, &goal)?;
    if record.schema != 2
        || record.installation_id != lease.id()
        || record.plan_sha256 != aaa_config::install::plan_sha256(plan)
        || record.account_sha256 != account_hash()?
        || Some(files::digest(&record.before)?) != permission.before_sha256
    {
        return Err(Fault::conflict());
    }
    Ok(record)
}
pub(super) fn prepared(plan: &InstallPlan, id: &str) -> bool {
    record_path(plan, id).symlink_metadata().is_ok()
}
pub(super) fn prepare(
    plan: &InstallPlan,
    lease: &Lease,
) -> Result<Option<(aaa_agent::identity::AgentIdentity, aaa_common::tls::Identity)>> {
    let Some(goal) = scope::enrollment(plan).into_iter().next() else { return Ok(None) };
    let permission = lease.permission(plan, &goal)?;
    let path = record_path(plan, lease.id());
    let mut record = match path.symlink_metadata() {
        Ok(_) => read_record(plan, lease, &path)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let before = files::snapshot(&Path::new(&plan.context.state_dir).join("agent"))?;
            if Some(files::digest(&before)?) != permission.before_sha256 {
                return Err(Fault::conflict());
            }
            let record = Prepared {
                schema: 2,
                installation_id: lease.id().into(),
                plan_sha256: aaa_config::install::plan_sha256(plan),
                account_sha256: account_hash()?,
                complete: false,
                agent_identity: None,
                enrolled: false,
                host_identity: None,
                before,
                prepared: None,
                pin_intent: None,
            };
            private::write(&path, &serde_json::to_vec(&record).map_err(|_| Fault::conflict())?).map_err(Fault::io)?;
            record
        }
        Err(_) => return Err(Fault::conflict()),
    };
    files::prepare(plan, lease, &path, &mut record).map(Some)
}
pub(super) async fn apply(
    plan: &InstallPlan,
    lease: &Lease,
    authorization: &InstallAuthorization,
    subscriber: &mut Option<worker::private_display::Subscriber>,
) -> Result<bool> {
    let Some((identity, tls)) = prepare(plan, lease)? else { return Ok(false) };
    let agent_state = Path::new(&plan.context.state_dir).join("agent");
    if agent_state.join("host.json").symlink_metadata().is_ok() {
        let pin: aaa_agent::identity::HostPin =
            serde_json::from_slice(&private::read(&agent_state.join("host.json"), 65536).map_err(Fault::io)?)
                .map_err(|_| Fault::conflict())?;
        if Some(&pin.address) != plan.choices.host.as_ref() {
            return Err(Fault::conflict());
        }
        if aaa_agent::link::lookup_agent(
            &agent_state,
            &pin.address,
            &identity.machine_id,
            plan.choices.name.as_deref().ok_or_else(Fault::conflict)?,
            &pin.fingerprint,
        )
        .await
        .map_err(|_| required("installation_enrollment_not_verified"))?
        .is_some()
        {
            record_enrolled(plan, lease, &pin.fingerprint)?;
            return Ok(false);
        }
        return Err(required("installation_enrollment_not_verified"));
    }
    match plan.choices.enroll {
        Some(InstallEnroll::Grant) => {
            let grant =
                authorization.enrollment.as_ref().ok_or_else(|| required("installation_enrollment_grant_required"))?;
            if grant.agent_identity != tls.fingerprint {
                return Err(Fault::conflict());
            }
            let pin = aaa_agent::link::enroll_install(
                &Path::new(&plan.context.state_dir).join("agent"),
                plan.choices.host.as_deref().ok_or_else(Fault::conflict)?,
                &identity.machine_id,
                plan.choices.name.as_deref().ok_or_else(Fault::conflict)?,
                grant,
            )
            .await
            .map_err(|_| required("installation_enrollment_not_verified"))?;
            files::commit(plan, lease, &pin).map(|_| true)
        }
        Some(InstallEnroll::Wait) => {
            use worker::private_display::Request;
            let subscriber = subscriber.as_mut().ok_or_else(|| required("installation_private_display_required"))?;
            let path = Path::new(&plan.context.state_dir)
                .join("install/authorization")
                .join(format!("{}.identity.json", lease.id()));
            let record: Prepared = serde_json::from_slice(&private::read(&path, 65536).map_err(Fault::io)?)
                .map_err(|_| Fault::conflict())?;
            let binding = InstallEnrollmentBinding {
                installation_id: lease.id().into(),
                plan_sha256: aaa_config::install::plan_sha256(plan),
            };
            if let Some(host) = &record.host_identity {
                if let Some(pin) = aaa_agent::link::lookup_install(
                    &Path::new(&plan.context.state_dir).join("agent"),
                    plan.choices.host.as_deref().ok_or_else(Fault::conflict)?,
                    &identity.machine_id,
                    &binding,
                    host,
                )
                .await
                .map_err(|_| required("installation_private_enrollment_not_verified"))?
                {
                    files::commit(plan, lease, &pin)?;
                    return Ok(false);
                }
                if record.enrolled {
                    return Err(required("installation_private_enrollment_not_verified"));
                }
            }
            subscriber.request(Request::CanDisplay).await?;
            let closed = subscriber.closed();
            let agent_state = Path::new(&plan.context.state_dir).join("agent");
            let enroll = aaa_agent::link::enroll_private(
                aaa_agent::link::PrivateEnrollment {
                    state: &agent_state,
                    addr: plan.choices.host.as_deref().ok_or_else(Fault::conflict)?,
                    machine_id: &identity.machine_id,
                    name: plan.choices.name.as_deref().ok_or_else(Fault::conflict)?,
                    binding: &binding,
                    pending_host: record.host_identity.as_deref(),
                },
                |challenge| async move {
                    record_pending(plan, lease, &challenge.host_fingerprint)
                        .map_err(|_| anyhow::anyhow!("설치 등록 신원 기록을 확인하세요"))?;
                    subscriber
                        .request(Request::Sas { code: challenge.code, host_fingerprint: challenge.host_fingerprint })
                        .await
                        .map_err(|_| anyhow::anyhow!("보호 표시 연결 종료"))
                },
                closed,
            );
            match tokio::time::timeout(
                std::time::Duration::from_secs(u64::from(plan.choices.wait_timeout.unwrap_or(600))),
                enroll,
            )
            .await
            {
                Ok(Ok(pin)) => files::commit(plan, lease, &pin).map(|_| true),
                Ok(Err(_)) => Err(required("installation_private_enrollment_not_verified")),
                Err(_) => Err(required("installation_enrollment_wait_expired")),
            }
        }
        _ => Err(Fault::conflict()),
    }
}

/// 읽기 status에서 파일 적용만 끝난 설치를 등록 완료라고 하지 않는다.
pub(super) fn observe(plan: &InstallPlan, id: &str, result: &mut InstallResult) -> Result<()> {
    if scope::enrollment(plan).is_empty() {
        return Ok(());
    }
    let state = Path::new(&plan.context.state_dir);
    let path = state.join("install/authorization").join(format!("{id}.identity.json"));
    let ready = (|| -> Result<bool> {
        let record: Prepared =
            serde_json::from_slice(&private::read(&path, 65536).map_err(Fault::io)?).map_err(|_| Fault::conflict())?;
        if record.schema != 2
            || record.installation_id != id
            || record.plan_sha256 != aaa_config::install::plan_sha256(plan)
            || record.account_sha256 != account_hash()?
        {
            return Err(Fault::conflict());
        }
        let issued = authorization::Authority::open(state)?.issued(id)?;
        let goal = scope::enrollment(plan).into_iter().next().ok_or_else(Fault::conflict)?;
        if !issued.authorization.claims.permissions.iter().any(|p| {
            p.operation == goal.operation
                && p.target == goal.target
                && p.after_sha256 == goal.after_sha256
                && p.before_sha256.as_ref() == files::digest(&record.before).ok().as_ref()
        }) {
            return Err(Fault::conflict());
        }
        if !record.complete || !record.enrolled {
            return Ok(false);
        }
        let agent = state.join("agent");
        private::verify_directory(&agent).map_err(Fault::io)?;
        let current = files::snapshot(&agent)?;
        let desired = record.prepared.as_ref().ok_or_else(Fault::conflict)?;
        if ["identity.json", "tls/key.der", "tls/cert.der"].iter().any(|name| current[*name] != desired[*name]) {
            return Err(Fault::conflict());
        }
        let cert = private::read(&agent.join("tls/cert.der"), 1024 * 1024).map_err(Fault::io)?;
        let pin: aaa_agent::identity::HostPin =
            serde_json::from_slice(&private::read(&agent.join("host.json"), 65536).map_err(Fault::io)?)
                .map_err(|_| Fault::conflict())?;
        if Some(hash(&cert)) != record.agent_identity
            || Some(pin.fingerprint) != record.host_identity
            || Some(pin.address) != plan.choices.host
        {
            return Err(Fault::conflict());
        }
        Ok(true)
    })();
    let ready = match ready {
        Ok(value) => value,
        Err(e) if e.exit == 4 => return Err(e),
        Err(_) => false,
    };
    result.checks.push(InstallCheck {
        name: "agent_enrollment".into(),
        state: if ready { InstallCheckState::Passed } else { InstallCheckState::NotRun },
    });
    if !ready && result.exit_code == 0 {
        result.status = InstallStatus::NeedsUser;
        result.exit_code = 3;
        result.error = None;
        result.needs_user.push(InstallNeedsUser {
            code: "installation_enrollment_required".into(),
            target: "enrollment".into(),
            message: "같은 설치의 host 승인과 보호 표시를 확인한 뒤 재개하세요".into(),
        });
    }
    Ok(())
}

fn record_enrolled(plan: &InstallPlan, lease: &Lease, host: &str) -> Result<()> {
    let path =
        Path::new(&plan.context.state_dir).join("install/authorization").join(format!("{}.identity.json", lease.id()));
    let mut record: Prepared =
        serde_json::from_slice(&private::read(&path, 65536).map_err(Fault::io)?).map_err(|_| Fault::conflict())?;
    record.enrolled = true;
    record.host_identity = Some(host.into());
    private::write(&path, &serde_json::to_vec(&record).map_err(|_| Fault::conflict())?).map_err(Fault::io)
}

fn record_pending(plan: &InstallPlan, lease: &Lease, host: &str) -> Result<()> {
    if !digest_ok(host) {
        return Err(Fault::conflict());
    }
    let path =
        Path::new(&plan.context.state_dir).join("install/authorization").join(format!("{}.identity.json", lease.id()));
    let mut record: Prepared =
        serde_json::from_slice(&private::read(&path, 65536).map_err(Fault::io)?).map_err(|_| Fault::conflict())?;
    if record.plan_sha256 != aaa_config::install::plan_sha256(plan)
        || record.installation_id != lease.id()
        || record.account_sha256 != account_hash()?
        || record.host_identity.as_deref().is_some_and(|old| old != host)
    {
        return Err(Fault::conflict());
    }
    record.host_identity = Some(host.into());
    private::write(&path, &serde_json::to_vec(&record).map_err(|_| Fault::conflict())?).map_err(Fault::io)
}
