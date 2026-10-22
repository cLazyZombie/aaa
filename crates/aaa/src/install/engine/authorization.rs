//! I6 로컬 발급자. 서명 입력이 신뢰 root를 공급하지 않으며 소비 기록은 private 저장소에만 둔다.
use super::*;
#[cfg(test)]
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};

const MAX_LIFETIME_MS: u64 = 10 * 60 * 1000;
const LIMIT: u64 = 1024 * 1024;

fn denied() -> Fault {
    Fault { code: "installation_authorization_invalid", exit: 3 }
}
fn bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    aaa_common::install_crypto::canonical(value).map_err(|_| denied())
}

fn lock_fault(error: std::io::Error) -> Fault {
    if error.kind() == std::io::ErrorKind::WouldBlock { Fault { code: "installation_busy", exit: 4 } } else { denied() }
}

pub(super) struct Authority {
    root: PathBuf,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    authorization_sha256: String,
    installation_id: String,
    consumed: bool,
}

/// 발급 뒤 입력 유실/중단에도 원래 계획을 복원할 private payload. 일반 journal/출력에 넣지 않는다.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Issued {
    pub(super) plan: InstallPlan,
    pub(super) authorization: InstallAuthorization,
}

/// 이 guard의 수명 동안 같은 발급자의 다른 쓰기 실행기는 진입하지 못한다.
pub(super) struct Lease {
    id: String,
    plan_sha256: String,
    permissions: Vec<InstallPermission>,
    expires_at_ms: u64,
    _lock: private::Lock,
}
impl Lease {
    pub(super) fn id(&self) -> &str {
        &self.id
    }
    pub(super) fn valid_at(&self, now: u64) -> Result<()> {
        if now >= self.expires_at_ms { Err(denied()) } else { Ok(()) }
    }
    pub(super) fn target(
        &self,
        plan: &InstallPlan,
        operation: InstallOperation,
        target: &str,
    ) -> Result<&InstallPermission> {
        if self.plan_sha256 != aaa_config::install::plan_sha256(plan) {
            return Err(denied());
        }
        let mut matches = self.permissions.iter().filter(|p| p.operation == operation && p.target == target);
        let permission = matches.next().ok_or_else(denied)?;
        if matches.next().is_some() {
            return Err(denied());
        }
        Ok(permission)
    }
    pub(super) fn permission(&self, plan: &InstallPlan, goal: &InstallPermission) -> Result<&InstallPermission> {
        let permission = self.target(plan, goal.operation, &goal.target)?;
        if permission.after_sha256 != goal.after_sha256 {
            return Err(denied());
        }
        Ok(permission)
    }
    pub(super) fn permits(&self, plan: &InstallPlan, permissions: &[InstallPermission]) -> Result<()> {
        if self.plan_sha256 != aaa_config::install::plan_sha256(plan) {
            return Err(denied());
        }
        for requested in permissions {
            let mut allowed = false;
            for signed in &self.permissions {
                if scope::same_permission(requested, signed)? {
                    allowed = true;
                    break;
                }
            }
            if !allowed {
                return Err(denied());
            }
        }
        Ok(())
    }
}

impl Authority {
    fn at(state: &Path) -> Self {
        Self { root: state.join("install/authorization") }
    }
    /// 호출자는 외부 TTY의 실제 검토/동의를 먼저 확인해야 한다. 검증 경로에서는 호출하지 않는다.
    pub(super) fn initialize(state: &Path) -> Result<Self> {
        let state = transaction::real_directory(state)?;
        let this = Self::at(&state);
        private::directory(&state.join("install")).map_err(Fault::io)?;
        private::directory(&this.root).map_err(Fault::io)?;
        let _lock = private::lock(&this.root.join("issuer.lock")).map_err(lock_fault)?;
        let key = this.root.join("issuer.pk8");
        let pin = this.root.join("issuer.pin");
        match (key.symlink_metadata(), pin.symlink_metadata()) {
            (Err(a), Err(b))
                if a.kind() == std::io::ErrorKind::NotFound && b.kind() == std::io::ErrorKind::NotFound =>
            {
                let document = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| denied())?;
                let pair = Ed25519KeyPair::from_pkcs8(document.as_ref()).map_err(|_| denied())?;
                private::write(&key, document.as_ref()).map_err(Fault::io)?;
                private::write(&pin, pair.public_key().as_ref()).map_err(Fault::io)?;
            }
            (Ok(_), Ok(_)) => {}
            // 중간 종료/기존 손상은 키 교체나 새 trust로 수리하지 않는다.
            _ => return Err(denied()),
        }
        this.public_key()?;
        Ok(this)
    }
    /// 읽기 검증. 없는 root·lock·pin을 생성하거나 권한을 고치지 않는다.
    pub(super) fn open(state: &Path) -> Result<Self> {
        let state = transaction::real_directory(state)?;
        let this = Self::at(&state);
        this.public_key()?;
        Ok(this)
    }
    fn public_key(&self) -> Result<Vec<u8>> {
        private::verify_directory(self.root.parent().ok_or_else(denied)?).map_err(|_| denied())?;
        private::verify_directory(&self.root).map_err(|_| denied())?;
        let pin = private::read(&self.root.join("issuer.pin"), 32).map_err(|_| denied())?;
        if pin.len() != 32 {
            return Err(denied());
        }
        Ok(pin)
    }
    fn record_path(&self, nonce: &str) -> Result<PathBuf> {
        if !id_ok(nonce) {
            return Err(denied());
        }
        Ok(self.root.join(format!("{nonce}.json")))
    }
    fn input_path(&self, id: &str) -> Result<PathBuf> {
        if !id_ok(id) {
            return Err(denied());
        }
        Ok(self.root.join(format!("{id}.input.json")))
    }
    /// 조회 자체는 만료와 상관없이 원래 서명/발급 기록을 읽는다. 실행은 begin에서 현재 시간을 다시 검사한다.
    pub(super) fn issued(&self, id: &str) -> Result<Issued> {
        let input: Issued =
            serde_json::from_slice(&private::read(&self.input_path(id)?, 2 * LIMIT).map_err(|_| denied())?)
                .map_err(|_| denied())?;
        if input.authorization.claims.installation_id != id {
            return Err(denied());
        }
        self.verify(
            &input.authorization,
            &input.plan,
            &input.authorization.claims.permissions,
            input.authorization.claims.issued_at_ms,
        )?;
        Ok(input)
    }
    fn check_plan(plan: &InstallPlan, permissions: &[InstallPermission]) -> Result<()> {
        let checked = aaa_config::install::plan(plan.choices.clone(), plan.context.clone()).map_err(|_| denied())?;
        if checked != *plan
            || super::super::platform::account().as_deref() != Some(&plan.context.target_account)
            || super::super::platform::platform().ok().as_ref() != Some(&plan.context.platform)
            || permissions.is_empty()
            || permissions.len() > 256
            || permissions.iter().any(|p| {
                p.target.is_empty()
                    || p.target.chars().any(char::is_control)
                    || p.before_sha256.as_deref().is_some_and(|v| !digest_ok(v))
                    || p.after_sha256.as_deref().is_some_and(|v| !digest_ok(v))
            })
        {
            return Err(denied());
        }
        Ok(())
    }
    pub(super) fn issue(
        &self,
        plan: &InstallPlan,
        permissions: Vec<InstallPermission>,
        now: u64,
        lifetime: u64,
    ) -> Result<InstallAuthorization> {
        self.issue_for_id(plan, permissions, None, now, lifetime)
    }
    /// 재발급도 외부 TTY의 재검토가 전제다. 원래 ID·계획·범위를 넓히지 않는다.
    pub(super) fn refresh(&self, id: &str, now: u64, lifetime: u64) -> Result<InstallAuthorization> {
        let input = self.issued(id)?;
        self.issue_for_id(&input.plan, input.authorization.claims.permissions, Some(id), now, lifetime)
    }
    fn issue_for_id(
        &self,
        plan: &InstallPlan,
        permissions: Vec<InstallPermission>,
        id: Option<&str>,
        now: u64,
        lifetime: u64,
    ) -> Result<InstallAuthorization> {
        Self::check_plan(plan, &permissions)?;
        if lifetime == 0 || lifetime > MAX_LIFETIME_MS {
            return Err(denied());
        }
        let expires_at_ms = now.checked_add(lifetime).ok_or_else(denied)?;
        let pin = self.public_key()?;
        let _lock = private::lock(&self.root.join("issuer.lock")).map_err(lock_fault)?;
        if let Some(id) = id {
            let previous = self.issued(id)?;
            if previous.plan != *plan || previous.authorization.claims.permissions != permissions {
                return Err(denied());
            }
        }
        let document = private::read(&self.root.join("issuer.pk8"), 4096).map_err(|_| denied())?;
        let pair = Ed25519KeyPair::from_pkcs8(&document).map_err(|_| denied())?;
        if pair.public_key().as_ref() != pin {
            return Err(denied());
        }
        let claims = InstallAuthorizationClaims {
            schema: InstallAuthorizationSchema::V1,
            installation_id: id.map(str::to_owned).unwrap_or_else(aaa_common::util::new_id),
            plan_sha256: aaa_config::install::plan_sha256(plan),
            platform: plan.context.platform.clone(),
            target_account: plan.context.target_account.clone(),
            instance: plan.choices.instance.clone(),
            binary_sha256: plan.context.binary_sha256.clone(),
            permissions,
            issued_at_ms: now,
            expires_at_ms,
            nonce: aaa_common::util::new_id(),
            issuer: format!("local-ed25519:{}", hash(&pin)),
        };
        let signer = aaa_common::install_crypto::Signer::open(&document).map_err(|_| denied())?;
        let signature = signer.sign_authorization(&claims).map_err(|_| denied())?;
        let authorization = InstallAuthorization { claims, signature, enrollment: None };
        let record = Record {
            authorization_sha256: hash(&bytes(&authorization)?),
            installation_id: authorization.claims.installation_id.clone(),
            consumed: false,
        };
        let path = self.record_path(&authorization.claims.nonce)?;
        if path.symlink_metadata().is_ok() {
            return Err(denied());
        }
        if id.is_none() && self.input_path(&authorization.claims.installation_id)?.symlink_metadata().is_ok() {
            return Err(denied());
        }
        private::write(&path, &bytes(&record)?).map_err(Fault::io)?;
        let input = Issued { plan: plan.clone(), authorization: authorization.clone() };
        private::write(&self.input_path(&authorization.claims.installation_id)?, &bytes(&input)?).map_err(Fault::io)?;
        Ok(authorization)
    }
    fn verify(
        &self,
        authorization: &InstallAuthorization,
        plan: &InstallPlan,
        permissions: &[InstallPermission],
        now: u64,
    ) -> Result<Record> {
        let pin = self.public_key()?;
        Self::check_signed(&pin, authorization, plan, permissions, now)?;
        let c = &authorization.claims;
        let record: Record =
            serde_json::from_slice(&private::read(&self.record_path(&c.nonce)?, LIMIT).map_err(|_| denied())?)
                .map_err(|_| denied())?;
        if record.installation_id != c.installation_id || record.authorization_sha256 != hash(&bytes(authorization)?) {
            return Err(denied());
        }
        Ok(record)
    }
    fn check_signed(
        pin: &[u8],
        authorization: &InstallAuthorization,
        plan: &InstallPlan,
        permissions: &[InstallPermission],
        now: u64,
    ) -> Result<()> {
        Self::check_plan(plan, permissions)?;
        let c = &authorization.claims;
        if !id_ok(&c.installation_id)
            || !id_ok(&c.nonce)
            || c.plan_sha256 != aaa_config::install::plan_sha256(plan)
            || c.platform != plan.context.platform
            || c.target_account != plan.context.target_account
            || c.instance != plan.choices.instance
            || c.binary_sha256 != plan.context.binary_sha256
            || c.permissions != permissions
            || c.issuer != format!("local-ed25519:{}", hash(pin))
            || c.issued_at_ms > now
            || c.expires_at_ms <= now
            || c.expires_at_ms.checked_sub(c.issued_at_ms).is_none_or(|v| v == 0 || v > MAX_LIFETIME_MS)
        {
            return Err(denied());
        }
        if let Some(grant) = &authorization.enrollment
            && (plan.choices.role != Some(InstallRole::Agent)
                || plan.choices.enroll != Some(InstallEnroll::Grant)
                || grant.installation_id != c.installation_id
                || grant.plan_sha256 != c.plan_sha256
                || grant.issuer != c.issuer
                || grant.expires_at_ms <= now
                || grant.expires_at_ms > c.expires_at_ms
                || !id_ok(&grant.nonce)
                || !digest_ok(&grant.agent_identity)
                || !digest_ok(&grant.host_identity)
                || !aaa_common::install_crypto::verify_enrollment(pin, grant))
        {
            return Err(denied());
        }
        if !aaa_common::install_crypto::verify_authorization(pin, c, &authorization.signature) {
            return Err(denied());
        }
        Ok(())
    }
    /// 최초 신뢰는 OS가 입증한 SSH 작업에서만 연다. 입력 공개키만으로는 이 타입을 얻을 수 없다.
    pub(super) fn import(
        state: &Path,
        package: &InstallBootstrapPackage,
        _ssh: &super::bootstrap::VerifiedSsh,
        now: u64,
    ) -> Result<Self> {
        use base64::Engine as _;
        let pin = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(&package.issuer_public_key)
            .map_err(|_| denied())?;
        if pin.len() != 32 || Path::new(&package.plan.context.state_dir) != transaction::real_directory(state)? {
            return Err(denied());
        }
        let auth = &package.authorization;
        Self::check_signed(&pin, auth, &package.plan, &auth.claims.permissions, now)?;
        let this = Self::at(state);
        let existing = match this.root.symlink_metadata() {
            Ok(_) => {
                if this.public_key()? != pin {
                    return Err(denied());
                }
                match this.input_path(&auth.claims.installation_id)?.symlink_metadata() {
                    Ok(_) => Some(this.issued(&auth.claims.installation_id)?),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(_) => return Err(denied()),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return Err(denied()),
        };
        if let Some(original) = &existing {
            if original.plan != package.plan
                || original.authorization.claims.permissions != auth.claims.permissions
                || original.authorization.enrollment.as_ref().is_some_and(|old| {
                    auth.enrollment.as_ref().is_none_or(|new| {
                        old.agent_identity != new.agent_identity || old.host_identity != new.host_identity
                    })
                })
            {
                return Err(denied());
            }
        } else if scope::all(&package.plan)? != auth.claims.permissions {
            return Err(denied());
        }
        private::directory(&state.join("install")).map_err(Fault::io)?;
        private::directory(&this.root).map_err(Fault::io)?;
        let _lock = private::lock(&this.root.join("issuer.lock")).map_err(lock_fault)?;
        let pin_path = this.root.join("issuer.pin");
        match pin_path.symlink_metadata() {
            Ok(_) => {
                if this.public_key()? != pin {
                    return Err(denied());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // root가 경쟁 중 생겼더라도 원래 key가 있으면 다른 최초 신뢰를 덮지 않는다.
                if this.root.join("issuer.pk8").symlink_metadata().is_ok() {
                    return Err(denied());
                }
                use std::io::Write;
                let mut file = private::new_file(&pin_path).map_err(Fault::io)?;
                file.write_all(&pin).map_err(Fault::io)?;
                file.sync_all().map_err(Fault::io)?;
                drop(file);
                private::sync_parent(&pin_path).map_err(Fault::io)?;
            }
            Err(_) => return Err(denied()),
        }
        // 잠금 뒤 같은 ID의 불변식을 다시 검사한다.
        if let Ok(original) = this.issued(&auth.claims.installation_id)
            && (original.plan != package.plan || original.authorization.claims.permissions != auth.claims.permissions)
        {
            return Err(denied());
        }
        let path = this.record_path(&auth.claims.nonce)?;
        let record = Record {
            authorization_sha256: hash(&bytes(auth)?),
            installation_id: auth.claims.installation_id.clone(),
            consumed: false,
        };
        match path.symlink_metadata() {
            Ok(_) => {
                let prior: Record = serde_json::from_slice(&private::read(&path, LIMIT).map_err(|_| denied())?)
                    .map_err(|_| denied())?;
                if prior.authorization_sha256 != record.authorization_sha256
                    || prior.installation_id != record.installation_id
                {
                    return Err(denied());
                }
                // 같은 입력의 재전달은 소비된 nonce를 미소비로 되돌리지 않는다.
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                private::write(&path, &bytes(&record)?).map_err(Fault::io)?
            }
            Err(_) => return Err(denied()),
        }
        private::write(
            &this.input_path(&auth.claims.installation_id)?,
            &bytes(&Issued { plan: package.plan.clone(), authorization: auth.clone() })?,
        )
        .map_err(Fault::io)?;
        Ok(this)
    }
    /// 구독자가 자식을 실행하기 전 읽기 검증한다. 소비는 worker의 begin만 한다.
    pub(super) fn validate(
        &self,
        authorization: &InstallAuthorization,
        plan: &InstallPlan,
        permissions: &[InstallPermission],
        now: u64,
    ) -> Result<()> {
        self.verify(authorization, plan, permissions, now).map(|_| ())
    }
    pub(super) fn begin(
        &self,
        authorization: &InstallAuthorization,
        plan: &InstallPlan,
        permissions: &[InstallPermission],
        now: u64,
        resume: Option<&str>,
    ) -> Result<Lease> {
        // 불신 입력은 lock 파일도 만들지 않는다. 잠금을 얻은 뒤 소비 기록을 다시 대조한다.
        self.verify(authorization, plan, permissions, now)?;
        let lock = private::lock(&self.root.join("issuer.lock")).map_err(lock_fault)?;
        let mut record = self.verify(authorization, plan, permissions, now)?;
        let consume = match resume {
            Some(id) if id == record.installation_id => !record.consumed,
            None if !record.consumed => true,
            _ => return Err(denied()),
        };
        if consume {
            record.consumed = true;
            private::write(&self.record_path(&authorization.claims.nonce)?, &bytes(&record)?).map_err(Fault::io)?;
        }
        Ok(Lease {
            id: record.installation_id,
            plan_sha256: authorization.claims.plan_sha256.clone(),
            permissions: permissions.to_vec(),
            expires_at_ms: authorization.claims.expires_at_ms,
            _lock: lock,
        })
    }
}

#[cfg(test)]
mod tests;
