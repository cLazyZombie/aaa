//! 승인 소비와 파일 transaction 사이의 내구 intent. 공개 ID는 바뀌지 않는다.
use super::*;
use authorization::{Authority, Lease};
use transaction::AuthorizedTransaction;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Intent,
    FilesPrepared,
    FilesApplied,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Pending {
    PrivateDisplay,
    PrivateEnrollment,
    Grant,
    Password,
    Service,
    Other,
}
impl Pending {
    fn code(self) -> &'static str {
        match self {
            Self::PrivateDisplay => "installation_private_display_required",
            Self::PrivateEnrollment => "installation_private_enrollment_not_verified",
            Self::Grant => "installation_enrollment_grant_required",
            Self::Password => "service_account_auth_unavailable",
            Self::Service => "service_setup_incomplete",
            Self::Other => "installation_execution_incomplete",
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    id: String,
    worker_pid: u32,
    account_sha256: String,
    instance: Option<String>,
    plan_sha256: String,
    scope_sha256: String,
    files: Option<String>,
    stage: Stage,
    last_exit: Option<u8>,
    pending: Option<Pending>,
}
fn record_path(state: &Path, id: &str) -> Result<PathBuf> {
    if !id_ok(id) {
        return Err(Fault::conflict());
    }
    Ok(state.join("install/executions").join(format!("{id}.json")))
}
fn scope_sha(scope: &[InstallPermission]) -> Result<String> {
    Ok(hash(&serde_json::to_vec(scope).map_err(|_| Fault::conflict())?))
}
fn read(state: &Path, id: &str) -> Result<Option<Record>> {
    match private::read(&record_path(state, id)?, 128 * 1024) {
        Ok(bytes) => {
            let record: Record = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
            if record.schema != 1
                || record.id != id
                || record.worker_pid == 0
                || record.account_sha256 != account_hash()?
                || !digest_ok(&record.plan_sha256)
                || !digest_ok(&record.scope_sha256)
                || record.last_exit.is_some_and(|exit| !matches!(exit, 0..=6 | 130))
                || record.files.as_deref().is_some_and(|id| !id_ok(id))
                || (record.stage == Stage::Intent) != record.files.is_none()
            {
                return Err(Fault::conflict());
            }
            let issued = Authority::open(state).and_then(|a| a.issued(id)).map_err(|_| Fault::conflict())?;
            if Path::new(&issued.plan.context.state_dir) != transaction::real_directory(state)?
                || record.plan_sha256 != aaa_config::install::plan_sha256(&issued.plan)
                || record.scope_sha256 != scope_sha(&issued.authorization.claims.permissions)?
                || record.instance != issued.plan.choices.instance
            {
                return Err(Fault::conflict());
            }
            if let Some(file_id) = &record.files {
                let (original, scope) = transaction::resume_scope(state, file_id, record.instance.as_deref())?;
                if file_id == id {
                    if original != issued.plan || scope != scope::files(&issued.authorization.claims.permissions) {
                        return Err(Fault::conflict());
                    }
                } else {
                    // unchanged receipt에는 새 쓰기 권한이 없다. 원래 목표/최종 bytes가 같은 경우만 관찰한다.
                    let old_choices = transaction::file_goal_choices(original.choices);
                    let choices = transaction::file_goal_choices(issued.plan.choices);
                    let mut old_context = original.context;
                    let mut context = issued.plan.context;
                    old_context.effective_config_sha256.clear();
                    context.effective_config_sha256.clear();
                    let final_scope = |scope: Vec<InstallPermission>| {
                        scope
                            .into_iter()
                            .map(|mut p| {
                                p.before_sha256 = None;
                                p
                            })
                            .collect::<Vec<_>>()
                    };
                    if old_choices != choices
                        || old_context != context
                        || final_scope(scope) != final_scope(scope::files(&issued.authorization.claims.permissions))
                    {
                        return Err(Fault::conflict());
                    }
                }
            }
            Ok(Some(record))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(Fault::conflict()),
    }
}

/// 발급된 planned ID와 이미 실행된 ID를 구분한다. 실제 nonce 경쟁은 begin의 issuer lease로 닫는다.
pub(super) fn ensure_not_started(state: &Path, id: &str) -> Result<()> {
    if read(state, id)?.is_some() { Err(Fault::conflict()) } else { Ok(()) }
}
/// 구독자는 Started를 그대로 믿지 않고 durable owner PID까지 실제 소유 자식과 맞춘다.
pub(super) fn verify_started(state: &Path, id: &str, pid: u32) -> Result<()> {
    let record = read(state, id)?.ok_or_else(Fault::conflict)?;
    if record.worker_pid != pid {
        return Err(Fault::conflict());
    }
    Ok(())
}
pub(super) struct Execution {
    plan: InstallPlan,
    state: PathBuf,
    record: Record,
    lease: Lease,
}
impl Execution {
    pub(super) fn begin(
        plan: InstallPlan,
        authorization: &InstallAuthorization,
        now: u64,
        resume: bool,
    ) -> Result<Self> {
        let state = transaction::real_directory(Path::new(&plan.context.state_dir))?;
        let id = &authorization.claims.installation_id;
        let previous = read(&state, id)?;
        if previous.is_some() && !resume {
            return Err(Fault::conflict());
        }
        let scope = match previous.as_ref().and_then(|r| r.files.as_deref()) {
            Some(file_id) if file_id == id => {
                let (original, scope) = transaction::resume_scope(&state, file_id, plan.choices.instance.as_deref())?;
                if original != plan {
                    return Err(Fault::conflict());
                }
                let mut complete = scope;
                complete.extend(
                    authorization
                        .claims
                        .permissions
                        .iter()
                        .filter(|p| scope::files(std::slice::from_ref(p)).is_empty())
                        .cloned(),
                );
                complete
            }
            Some(_) => authorization.claims.permissions.clone(),
            None if enrollment::prepared(&plan, id) => {
                let mut permissions = transaction::permissions(&plan)?;
                permissions.extend(
                    authorization
                        .claims
                        .permissions
                        .iter()
                        .filter(|p| scope::files(std::slice::from_ref(p)).is_empty())
                        .cloned(),
                );
                permissions
            }
            None => scope::all(&plan)?,
        };
        let expected_scope = scope_sha(&scope)?;
        if previous.as_ref().is_some_and(|r| {
            r.instance != plan.choices.instance
                || r.plan_sha256 != aaa_config::install::plan_sha256(&plan)
                || r.scope_sha256 != expected_scope
        }) {
            return Err(Fault::conflict());
        }
        let authority = Authority::open(&state)?;
        let lease = authority.begin(authorization, &plan, &scope, now, resume.then_some(id.as_str()))?;
        let mut record = previous.unwrap_or_else(|| Record {
            schema: 1,
            id: id.clone(),
            worker_pid: std::process::id(),
            account_sha256: hash(plan.context.target_account.as_bytes()),
            instance: plan.choices.instance.clone(),
            plan_sha256: aaa_config::install::plan_sha256(&plan),
            scope_sha256: expected_scope,
            files: None,
            stage: Stage::Intent,
            last_exit: None,
            pending: None,
        });
        record.worker_pid = std::process::id();
        record.last_exit = None;
        record.pending = None;
        let this = Self { plan, state, record, lease };
        private::directory(&this.state.join("install/executions")).map_err(Fault::io)?;
        this.save()?;
        Ok(this)
    }
    pub(super) async fn enroll(
        &mut self,
        authorization: &InstallAuthorization,
        subscriber: &mut Option<worker::private_display::Subscriber>,
    ) -> Result<bool> {
        if !scope::enrollment(&self.plan).is_empty() {
            self.lease.valid_at(aaa_common::util::now_ms())?;
        }
        enrollment::apply(&self.plan, &self.lease, authorization, subscriber).await
    }
    pub(super) fn finish(&mut self, result: &InstallResult) -> Result<()> {
        self.record.last_exit = Some(result.exit_code);
        self.record.pending = if result.exit_code == 3 {
            Some(match result.needs_user.first().map(|n| n.code.as_str()) {
                Some("installation_private_display_required" | "installation_private_display_lost") => {
                    Pending::PrivateDisplay
                }
                Some("installation_private_enrollment_not_verified" | "installation_enrollment_wait_expired") => {
                    Pending::PrivateEnrollment
                }
                Some("installation_enrollment_grant_required") => Pending::Grant,
                Some("service_account_auth_unavailable") => Pending::Password,
                Some(code) if code.starts_with("service_") => Pending::Service,
                _ => Pending::Other,
            })
        } else {
            None
        };
        self.save()
    }
    pub(super) fn user_path(&mut self) -> Result<bool> {
        user_path::apply(&self.plan, &self.lease)
    }
    pub(super) async fn publish(&mut self) -> Result<bool> {
        if self.plan.choices.tailscale_serve == Some(InstallServe::On) {
            self.lease.valid_at(aaa_common::util::now_ms())?;
        }
        web::publish(&self.plan, &self.record.id).await
    }
    pub(super) async fn pairing(
        &mut self,
        subscriber: &mut Option<worker::private_display::Subscriber>,
    ) -> Result<bool> {
        web::pairing(&self.plan, subscriber).await
    }
    pub(super) fn services(&mut self, password: Option<&supervisor::Password>) -> Result<bool> {
        let file_id = self.record.files.as_deref().ok_or_else(Fault::conflict)?;
        supervisor::apply(&self.plan, file_id, &self.lease, password)
    }
    /// 완료 event 전에 실제 파일·서비스·등록을 다시 읽는다. 저장 phase만으로 성공하지 않는다.
    pub(super) async fn verify(&self) -> Result<InstallResult> {
        let file_id = self.record.files.as_deref().ok_or_else(Fault::conflict)?;
        let journal = read_journal(&self.state, file_id, self.plan.choices.instance.as_deref())?;
        let result = transaction::observe(&self.state, journal)?;
        let mut result = supervisor::observe(&self.state, self.plan.choices.instance.as_deref(), file_id, result)?;
        enrollment::observe(&self.plan, &self.record.id, &mut result)?;
        user_path::observe(&self.plan, &mut result)?;
        readiness::observe(&self.plan, &mut result);
        web::observe(&self.plan, &self.record.id, &mut result).await?;
        result.installation_id = Some(self.record.id.clone());
        result.plan_sha256 = Some(self.record.plan_sha256.clone());
        crate::install::context(&self.plan, &mut result);
        Ok(result)
    }
    fn save(&self) -> Result<()> {
        private::write(
            &record_path(&self.state, &self.record.id)?,
            &serde_json::to_vec(&self.record).map_err(|_| Fault::conflict())?,
        )
        .map_err(Fault::io)
    }
    fn prepare_files(&mut self) -> Result<AuthorizedTransaction> {
        if let Some(file_id) = &self.record.files {
            if file_id != &self.record.id {
                return Err(Fault::conflict());
            }
            return AuthorizedTransaction::resume(
                &self.state,
                file_id,
                self.plan.choices.instance.as_deref(),
                &self.lease,
            );
        }
        let tx = AuthorizedTransaction::create(self.plan.clone(), &self.lease)?;
        self.record.files = Some(tx.id().to_owned());
        self.record.stage = Stage::FilesPrepared;
        self.save()?;
        Ok(tx)
    }
    /// 파일 ID를 intent에 내구 기록한 뒤에만 run한다. 재사용 receipt에는 쓰기를 다시 열지 않는다.
    pub(super) fn files(&mut self) -> Result<InstallResult> {
        let configured = self
            .record
            .files
            .as_deref()
            .map(|id| {
                read_journal(&self.state, id, self.plan.choices.instance.as_deref())
                    .map(|j| j.phase == Phase::Configured)
            })
            .transpose()?
            .unwrap_or(false);
        let mut result = if let Some(file_id) = &self.record.files
            && file_id != &self.record.id
        {
            // I2의 unchanged 재사용 ID는 이번 승인 ID가 아니다. 실제 상태만 읽는다.
            let journal = read_journal(&self.state, file_id, self.plan.choices.instance.as_deref())?;
            if journal.phase != Phase::Configured {
                return Err(Fault::conflict());
            }
            transaction::observe(&self.state, journal)?
        } else {
            self.prepare_files()?.run()?
        };
        if configured && result.exit_code == 0 {
            result.status = InstallStatus::Unchanged;
            for change in &mut result.changes {
                change.state = InstallChangeState::Unchanged;
            }
        }
        self.record.stage = Stage::FilesApplied;
        self.save()?;
        result.installation_id = Some(self.record.id.clone());
        result.plan_sha256 = Some(self.record.plan_sha256.clone());
        result.resume = (result.exit_code != 0).then(|| InstallResume { installation_id: self.record.id.clone() });
        Ok(result)
    }
}

/// 조회는 발급 root/lock/intent를 만들지 않고 파일 receipt의 실제 상태를 읽는다.
pub(super) fn observe(state: &Path, instance: Option<&str>, id: &str) -> Result<Option<InstallResult>> {
    let Some(record) = read(state, id)? else {
        // I2 파일 receipt가 있으면 실제 상태가 우선이다. 같은 ID의 승인만 보고 planned로 가리지 않는다.
        match journal_dir(state, id).join("journal.json").symlink_metadata() {
            Ok(_) => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Fault::conflict()),
        }
        // 승인만 발급된 ID도 조회 가능하다. status를 위해 nonce/lock/intent는 만들지 않는다.
        let input_path = state.join("install/authorization").join(format!("{id}.input.json"));
        match input_path.symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(Fault::conflict()),
            Ok(_) => {}
        }
        let issued = Authority::open(state)?.issued(id)?;
        if issued.plan.choices.instance.as_deref() != instance {
            return Err(Fault::conflict());
        }
        let mut result = *super::super::error("installation_planned", 0);
        result.status = InstallStatus::Planned;
        result.error = None;
        result.role = issued.plan.choices.role;
        result.instance = issued.plan.choices.instance.clone();
        result.platform = Some(issued.plan.context.platform.clone());
        result.binary_sha256 = Some(issued.plan.context.binary_sha256.clone());
        result.plan_sha256 = Some(aaa_config::install::plan_sha256(&issued.plan));
        result.installation_id = Some(id.into());
        if enrollment::prepared(&issued.plan, id) {
            enrollment::observe(&issued.plan, id, &mut result)?;
            user_path::observe(&issued.plan, &mut result)?;
            readiness::observe(&issued.plan, &mut result);
            web::observe_readonly(&issued.plan, id, &mut result)?;
        }
        if result.exit_code != 0 {
            result.resume = Some(InstallResume { installation_id: id.into() });
        }
        crate::install::context(&issued.plan, &mut result);
        return Ok(Some(result));
    };
    if record.instance.as_deref() != instance {
        return Err(Fault::conflict());
    }
    let mut result = if let Some(file_id) = &record.files {
        let journal = read_journal(state, file_id, instance)?;
        let result = transaction::observe(state, journal)?;
        supervisor::observe(state, instance, file_id, result)?
    } else {
        let mut result = *super::super::error("installation_incomplete", 3);
        result.status = InstallStatus::NeedsUser;
        result.error = None;
        result.needs_user.push(InstallNeedsUser {
            code: "installation_resume_required".into(),
            target: "installation".into(),
            message: "같은 ID의 승인과 실제 상태를 확인해 재개하세요".into(),
        });
        result
    };
    let issued = Authority::open(state)?.issued(id)?;
    enrollment::observe(&issued.plan, id, &mut result)?;
    user_path::observe(&issued.plan, &mut result)?;
    readiness::observe(&issued.plan, &mut result);
    web::observe_readonly(&issued.plan, id, &mut result)?;
    if result.exit_code == 0 && record.last_exit.is_some_and(|exit| exit != 0) {
        let exit = record.last_exit.expect("확인한 저장 종료 코드");
        let failure = *super::super::error("installation_execution_failed", exit);
        result.status = failure.status;
        result.exit_code = exit;
        result.error = failure.error;
        result.needs_user.extend(failure.needs_user);
    }
    if result.exit_code == 0
        && record.last_exit.is_none()
        && scope::files(&issued.authorization.claims.permissions).len() != issued.authorization.claims.permissions.len()
    {
        result.status = InstallStatus::NeedsUser;
        result.exit_code = 3;
        result.error = None;
        result.needs_user.push(InstallNeedsUser {
            code: "installation_execution_incomplete".into(),
            target: "installation".into(),
            message: "실제 상태를 확인하고 같은 ID로 재개하세요".into(),
        });
    }
    if let Some(pending) = record.pending
        && result.exit_code == 3
        && !result.needs_user.iter().any(|n| n.code == pending.code())
    {
        result.status = InstallStatus::NeedsUser;
        result.error = None;
        result.needs_user.push(InstallNeedsUser {
            code: pending.code().into(),
            target: "installation".into(),
            message: "보호된 승인·표시·입력과 실제 상태를 확인한 뒤 같은 ID로 재개하세요".into(),
        });
    }
    crate::install::context(&issued.plan, &mut result);
    result.installation_id = Some(record.id.clone());
    result.plan_sha256 = Some(record.plan_sha256);
    result.resume = (result.exit_code != 0).then_some(InstallResume { installation_id: record.id });
    Ok(Some(result))
}

#[cfg(test)]
mod tests;
