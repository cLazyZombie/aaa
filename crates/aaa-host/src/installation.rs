//! 좁은 설치 서명 승인. 기기 추가·일반 등록 창·임의 shell 권한을 발급하지 않는다.
use crate::{Host, err, front};
use aaa_common::{install_crypto, private_file, util};
use aaa_proto::{DeviceRole, ErrorBody, ErrorCode, FrontResponse, install::*};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
mod publication;

use std::{
    fs::File,
    path::{Path, PathBuf},
};

fn denied() -> ErrorBody {
    err(ErrorCode::Forbidden, "이 설치 계획의 서명 승인을 확인하세요")
}
fn invalid() -> ErrorBody {
    err(ErrorCode::BadRequest, "설치 제안의 계획·범위·신원을 확인하세요")
}
fn unavailable() -> ErrorBody {
    err(ErrorCode::Unavailable, "설치 발급자의 private 저장소를 확인하세요")
}
fn digest(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}
fn digest_ok(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}
fn id_ok(s: &str) -> bool {
    s.len() == 26 && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
fn root(state: &Path) -> PathBuf {
    state.join("install/host-issuer")
}
fn actor_allowed(role: DeviceRole, actor: &str) -> bool {
    !front::in_session(actor)
        && (role == DeviceRole::Controller && actor.starts_with("device:")
            || role == DeviceRole::Local && actor.starts_with("local:"))
}
struct Issuer {
    root: PathBuf,
    signer: install_crypto::Signer,
    _lock: File,
}
impl Issuer {
    fn initialize(state: &Path) -> Result<Self, ErrorBody> {
        private_file::directory(&state.join("install")).map_err(|_| unavailable())?;
        let root = root(state);
        private_file::directory(&root).map_err(|_| unavailable())?;
        let lock = private_file::lock(&root.join("issuer.lock")).map_err(|_| unavailable())?;
        let key = root.join("issuer.pk8");
        let pin = root.join("issuer.pin");
        match (key.symlink_metadata(), pin.symlink_metadata()) {
            (Err(a), Err(b))
                if a.kind() == std::io::ErrorKind::NotFound && b.kind() == std::io::ErrorKind::NotFound =>
            {
                let document = install_crypto::Signer::generate().map_err(|_| unavailable())?;
                let signer = install_crypto::Signer::open(&document).map_err(|_| unavailable())?;
                private_file::write(&key, &document).map_err(|_| unavailable())?;
                private_file::write(&pin, signer.public_key()).map_err(|_| unavailable())?;
            }
            (Ok(_), Ok(_)) => {}
            _ => return Err(unavailable()),
        }
        private_file::verify_directory(&root).map_err(|_| unavailable())?;
        let document = private_file::read(&key, 4096).map_err(|_| unavailable())?;
        let signer = install_crypto::Signer::open(&document).map_err(|_| unavailable())?;
        if private_file::read(&pin, 32).map_err(|_| unavailable())? != signer.public_key() {
            return Err(unavailable());
        }
        Ok(Self { root, signer, _lock: lock })
    }
    fn reference(&self) -> String {
        format!("local-ed25519:{}", digest(self.signer.public_key()))
    }
    fn package_path(&self, id: &str) -> Result<PathBuf, ErrorBody> {
        if !id_ok(id) {
            return Err(invalid());
        }
        Ok(self.root.join(format!("{id}.json")))
    }
}
/// host executor가 보호된 SSH 입력으로만 내보낼 package. Front 응답/일반 로그에는 넣지 않는다.
pub fn export(state: &Path, id: &str) -> Result<InstallBootstrapPackage, ErrorBody> {
    if !id_ok(id) {
        return Err(invalid());
    }
    let root = root(state);
    private_file::verify_directory(&root).map_err(|_| unavailable())?;
    let package: InstallBootstrapPackage = serde_json::from_slice(
        &private_file::read(&root.join(format!("{id}.json")), 2 * 1024 * 1024).map_err(|_| unavailable())?,
    )
    .map_err(|_| unavailable())?;
    let public = private_file::read(&root.join("issuer.pin"), 32).map_err(|_| unavailable())?;
    if package.authorization.claims.installation_id != id
        || URL_SAFE_NO_PAD.decode(&package.issuer_public_key).map_err(|_| unavailable())? != public
        || !install_crypto::verify_authorization(
            &public,
            &package.authorization.claims,
            &package.authorization.signature,
        )
        || package.authorization.claims.plan_sha256 != aaa_config::install::plan_sha256(&package.plan)
    {
        return Err(unavailable());
    }
    Ok(package)
}
impl Host {
    pub(crate) fn install_approve(
        &self,
        role: DeviceRole,
        actor: &str,
        request: InstallApprovalRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        if !actor_allowed(role, actor) {
            return Err(denied());
        }
        let checked = aaa_config::install::plan(request.plan.choices.clone(), request.plan.context.clone())
            .map_err(|_| invalid())?;
        let plan_sha = aaa_config::install::plan_sha256(&checked);
        let scope = install_crypto::canonical(&request.permissions).map_err(|_| invalid())?;
        if !util::machine_name_ok(checked.choices.name.as_deref().unwrap_or_default())
            || checked != request.plan
            || plan_sha != request.expected_plan_sha256
            || digest(&scope) != request.expected_scope_sha256
            || request.permissions.is_empty()
            || request.permissions.len() > 256
            || request.permissions.iter().any(|p| {
                p.target.is_empty()
                    || p.target.len() > 32768
                    || p.target.chars().any(char::is_control)
                    || p.before_sha256.as_deref().is_some_and(|s| !digest_ok(s))
                    || p.after_sha256.as_deref().is_some_and(|s| !digest_ok(s))
            })
            || request.agent_identity.as_deref().is_some_and(|s| !digest_ok(s))
            || request.agent_identity.is_some()
                && (checked.choices.role != Some(InstallRole::Agent)
                    || checked.choices.enroll != Some(InstallEnroll::Grant))
        {
            return Err(invalid());
        }
        let issuer = Issuer::initialize(&self.state)?;
        let id = request.installation_id.clone().unwrap_or_else(util::new_id);
        let path = issuer.package_path(&id)?;
        if request.installation_id.is_some() {
            let original = export(&self.state, &id)?;
            if original.plan != checked
                || original.authorization.claims.permissions != request.permissions
                || original
                    .authorization
                    .enrollment
                    .as_ref()
                    .is_some_and(|g| Some(&g.agent_identity) != request.agent_identity.as_ref())
            {
                return Err(invalid());
            }
        } else if path.symlink_metadata().is_ok() {
            return Err(invalid());
        }
        let now = util::now_ms();
        let expires = now.checked_add(5 * 60 * 1000).ok_or_else(invalid)?;
        let claims = InstallAuthorizationClaims {
            schema: InstallAuthorizationSchema::V1,
            installation_id: id.clone(),
            plan_sha256: plan_sha.clone(),
            platform: checked.context.platform.clone(),
            target_account: checked.context.target_account.clone(),
            instance: checked.choices.instance.clone(),
            binary_sha256: checked.context.binary_sha256.clone(),
            permissions: request.permissions,
            issued_at_ms: now,
            expires_at_ms: expires,
            nonce: util::new_id(),
            issuer: issuer.reference(),
        };
        let enrollment = if let Some(agent_identity) = request.agent_identity {
            let mut grant = InstallEnrollmentGrant {
                schema: InstallGrantSchema::V1,
                installation_id: id.clone(),
                plan_sha256: plan_sha.clone(),
                agent_identity,
                host_identity: self.identity.fingerprint.clone(),
                issuer: issuer.reference(),
                expires_at_ms: expires,
                nonce: util::new_id(),
                signature: String::new(),
            };
            grant.signature = issuer.signer.sign_enrollment(&grant).map_err(|_| unavailable())?;
            Some(grant)
        } else {
            None
        };
        let signature = issuer.signer.sign_authorization(&claims).map_err(|_| unavailable())?;
        let package = InstallBootstrapPackage {
            issuer_public_key: URL_SAFE_NO_PAD.encode(issuer.signer.public_key()),
            plan: checked,
            authorization: InstallAuthorization { claims, signature, enrollment },
        };
        private_file::write(&path, &serde_json::to_vec(&package).map_err(|_| unavailable())?)
            .map_err(|_| unavailable())?;
        self.db.audit(actor, "install_approve", &id);
        Ok(FrontResponse::InstallApproval {
            approval: InstallApprovalView { installation_id: id, plan_sha256: plan_sha, expires_at_ms: expires },
        })
    }
}

impl Host {
    pub(crate) fn install_enroll(
        &self,
        peer: &str,
        request: &InstallEnrollRequest,
        now: u64,
    ) -> Result<String, ErrorBody> {
        if !id_ok(&request.machine_id) || !digest_ok(peer) || !util::machine_name_ok(&request.machine_name) {
            return Err(invalid());
        }
        aaa_proto::negotiate(aaa_proto::AGENT_LINK, request.proto).map_err(|_| invalid())?;
        let grant = &request.grant;
        let package = export(&self.state, &grant.installation_id)?;
        let auth = &package.authorization;
        let public = URL_SAFE_NO_PAD.decode(&package.issuer_public_key).map_err(|_| unavailable())?;
        if auth.enrollment.as_ref() != Some(grant)
            || !install_crypto::verify_enrollment(&public, grant)
            || grant.agent_identity != peer
            || grant.host_identity != self.identity.fingerprint
            || grant.issuer != auth.claims.issuer
            || grant.plan_sha256 != aaa_config::install::plan_sha256(&package.plan)
            || grant.expires_at_ms <= now
            || auth.claims.issued_at_ms > now
            || auth.claims.expires_at_ms <= now
            || grant.expires_at_ms > auth.claims.expires_at_ms
            || !id_ok(&grant.nonce)
            || package.plan.choices.role != Some(InstallRole::Agent)
            || package.plan.choices.enroll != Some(InstallEnroll::Grant)
            || package.plan.choices.name.as_deref() != Some(&request.machine_name)
            || aaa_config::install::enrollment_permission(&package.plan).is_none_or(|goal| {
                !auth.claims.permissions.iter().any(|p| {
                    p.operation == goal.operation && p.target == goal.target && p.after_sha256 == goal.after_sha256
                })
            })
        {
            return Err(denied());
        }
        self.db
            .install_enroll(
                &grant.installation_id,
                &grant.plan_sha256,
                &grant.nonce,
                peer,
                &request.machine_id,
                &request.machine_name,
            )
            .map_err(|_| denied())?;
        self.machine_registered(&request.machine_id, &request.machine_name);
        self.bump();
        self.db.audit("install-grant", "install_enroll", &grant.installation_id);
        Ok(request.machine_id.clone())
    }
    pub(crate) fn install_lookup(&self, peer: &str, id: &str, plan: &str) -> Result<Option<String>, ErrorBody> {
        if !digest_ok(peer) || !id_ok(id) || !digest_ok(plan) {
            return Err(invalid());
        }
        self.db.install_enrolled(id, plan, peer).map_err(|_| unavailable())
    }
}

/// TLS 신원이 입증된 원격 연결에서만 호출한다. 일반 로컬 agent IPC에는 이 권한을 열지 않는다.
pub(crate) async fn link_control<R: tokio::io::AsyncRead + Unpin>(
    host: &Host,
    peer: &str,
    rx: &mut aaa_common::conn::FrameRx<R>,
    tx: &aaa_common::conn::FrameTx,
    mut first: aaa_proto::LinkMsg,
) -> anyhow::Result<Option<aaa_proto::LinkMsg>> {
    use aaa_proto::{AGENT_LINK, LinkMsg};
    let mut negotiated = false;
    loop {
        match first {
            LinkMsg::InstallEnrollProbe { proto } => {
                if aaa_proto::negotiate(AGENT_LINK, proto).is_err() {
                    tx.json(&LinkMsg::Reject(err(ErrorCode::UnsupportedVersion, "설치 등록 protocol이 맞지 않습니다")))
                        .await?;
                    return Ok(None);
                }
                tx.json(&LinkMsg::InstallEnrollCapabilities {
                    proto: AGENT_LINK,
                    features: vec!["install_enroll.v1".into()],
                })
                .await?;
                negotiated = true;
                first = tokio::time::timeout(std::time::Duration::from_secs(10), rx.control()).await??;
                // probe를 반복해 인증 전 연결 자원을 붙잡지 못한다.
                if matches!(first, LinkMsg::InstallEnrollProbe { .. }) {
                    return Ok(None);
                }
            }
            LinkMsg::InstallEnroll(request) if negotiated => {
                let reply = match host.install_enroll(peer, &request, util::now_ms()) {
                    Ok(machine_id) => {
                        LinkMsg::InstallEnrolled { installation_id: request.grant.installation_id.clone(), machine_id }
                    }
                    Err(e) => LinkMsg::Reject(e),
                };
                tx.json(&reply).await?;
                return Ok(None);
            }
            LinkMsg::InstallEnrollLookup { proto, installation_id, plan_sha256 } if negotiated => {
                let reply = if aaa_proto::negotiate(AGENT_LINK, proto).is_err() {
                    LinkMsg::Reject(invalid())
                } else {
                    match host.install_lookup(peer, &installation_id, &plan_sha256) {
                        Ok(Some(machine_id)) => LinkMsg::InstallEnrolled { installation_id, machine_id },
                        Ok(None) => LinkMsg::InstallEnrollUnknown,
                        Err(e) => LinkMsg::Reject(e),
                    }
                };
                tx.json(&reply).await?;
                return Ok(None);
            }
            LinkMsg::InstallAgentQuery { proto, machine_id, machine_name } if negotiated => {
                let reply = if aaa_proto::negotiate(AGENT_LINK, proto).is_err() {
                    LinkMsg::Reject(invalid())
                } else {
                    match host.db.machine_by_fingerprint(peer)? {
                        Some(machine) if !machine.local && machine.id == machine_id && machine.name == machine_name => {
                            LinkMsg::InstallAgentKnown { machine_id }
                        }
                        _ => LinkMsg::InstallEnrollUnknown,
                    }
                };
                tx.json(&reply).await?;
                return Ok(None);
            }
            LinkMsg::Enroll(request) if request.installation.is_some() && !negotiated => {
                tx.json(&LinkMsg::Reject(err(ErrorCode::UnsupportedVersion, "설치 등록 기능 협상이 필요합니다")))
                    .await?;
                return Ok(None);
            }
            other => return Ok(Some(other)),
        }
    }
}

#[cfg(test)]
mod tests;
