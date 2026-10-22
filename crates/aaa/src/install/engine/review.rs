//! 외부 TTY에서 검토한 private 계획의 발급. 임의 JSON 역직렬화는 권한이 아니다.
use super::*;
use authorization::Authority;
use std::io::Write;

type PublicResult<T> = std::result::Result<T, Box<InstallResult>>;
fn public(fault: Fault) -> Box<InstallResult> {
    crate::install::error(fault.code, fault.exit)
}
pub(in crate::install) fn normalize_state(state: &Path) -> PublicResult<PathBuf> {
    crate::install::detect::real_dir(state)
}
pub(in crate::install) fn read_plan(path: &Path) -> PublicResult<InstallPlan> {
    let bytes =
        private::read(path, 1024 * 1024).map_err(|_| crate::install::error("installation_plan_untrusted", 2))?;
    serde_json::from_slice(&bytes).map_err(|_| crate::install::error("installation_plan_invalid", 2))
}
pub(in crate::install) fn original_plan(state: &Path, id: &str) -> PublicResult<InstallPlan> {
    Ok(Authority::open(state).and_then(|a| a.issued(id)).map_err(public)?.plan)
}
pub(in crate::install) fn preview(plan: &InstallPlan, resume: Option<&str>) -> PublicResult<Vec<InstallPermission>> {
    if let Some(id) = resume {
        let input = Authority::open(Path::new(&plan.context.state_dir)).and_then(|a| a.issued(id)).map_err(public)?;
        if input.plan != *plan {
            return Err(crate::install::error("installation_content_conflict", 4));
        }
        return Ok(input.authorization.claims.permissions);
    }
    scope::all(plan).map_err(public)
}
pub(in crate::install) fn issue(
    consent: &crate::install::review::Consent,
    output: &Path,
) -> PublicResult<InstallResult> {
    let plan = consent.plan();
    let resume = consent.resume();
    let perform = || -> Result<InstallResult> {
        if !output.is_absolute() || output.components().any(|c| c == std::path::Component::ParentDir) {
            return Err(Fault::conflict());
        }
        let parent = crate::install::detect::real_dir(output.parent().ok_or_else(Fault::conflict)?)
            .map_err(|_| Fault::conflict())?;
        let output = parent.join(output.file_name().ok_or_else(Fault::conflict)?);
        match output.symlink_metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            _ => return Err(Fault::conflict()),
        }
        if resume.is_none() && scope::all(plan)? != consent.scope() {
            return Err(Fault::conflict());
        }
        let authority = Authority::initialize(Path::new(&plan.context.state_dir))?;
        let now = aaa_common::util::now_ms();
        let authorization = match resume {
            Some(id) => authority.refresh(id, now, 5 * 60 * 1000)?,
            None => authority.issue(plan, consent.scope().to_vec(), now, 5 * 60 * 1000)?,
        };
        // 기존 out은 덮어쓰지 않는다. 생성 때부터 사용자 전용 ACL/mode를 가진다.
        private::parents(&parent).map_err(Fault::io)?;
        let mut file = private::new_file(&output).map_err(Fault::io)?;
        serde_json::to_writer(&mut file, &authorization).map_err(|_| Fault::conflict())?;
        file.flush().map_err(Fault::io)?;
        file.sync_all().map_err(Fault::io)?;
        drop(file);
        private::sync_parent(&output).map_err(Fault::io)?;
        let mut result = *crate::install::error("installation_authorized", 0);
        result.status = InstallStatus::Planned;
        result.changes.push(InstallChange {
            kind: "authorization".into(),
            action: "issue".into(),
            state: InstallChangeState::Applied,
        });
        result.error = None;
        result.installation_id = Some(authorization.claims.installation_id);
        result.plan_sha256 = Some(authorization.claims.plan_sha256);
        result.role = plan.choices.role;
        result.instance = plan.choices.instance.clone();
        result.platform = Some(plan.context.platform.clone());
        result.binary_sha256 = Some(plan.context.binary_sha256.clone());
        Ok(result)
    };
    perform().map_err(public)
}

pub(in crate::install) fn resume_requires_consent(state: &Path, id: &str, now: u64) -> PublicResult<bool> {
    let issuer = Authority::open(state).map_err(public)?;
    let issued = issuer.issued(id).map_err(public)?;
    Ok(issuer.validate(&issued.authorization, &issued.plan, &issued.authorization.claims.permissions, now).is_err())
}
pub(in crate::install) fn executor_matches(plan: &InstallPlan) -> PublicResult<bool> {
    let image = std::env::current_exe().map_err(|_| public(Fault::conflict()))?;
    let sha = crate::update::sha256(&image).map_err(|_| public(Fault::conflict()))?;
    Ok(sha == plan.context.binary_sha256)
}
/// 선택한 instance 안의 최신 서명 기록만 읽는다. 다른 instance를 재개하거나 기록을 고치지 않는다.
pub(in crate::install) fn previous_installation(
    state: &Path,
    instance: Option<&str>,
) -> PublicResult<Option<(String, InstallPlan, InstallResult)>> {
    let root = state.join("install/authorization");
    match root.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(public(Fault::conflict())),
        Ok(_) => {}
    }
    let issuer = Authority::open(state).map_err(public)?;
    let mut latest = None;
    for entry in std::fs::read_dir(root).map_err(|_| public(Fault::conflict()))? {
        let entry = entry.map_err(|_| public(Fault::conflict()))?;
        let name = entry.file_name();
        let Some(id) = name.to_str().and_then(|name| name.strip_suffix(".input.json")) else { continue };
        if id_ok(id) && latest.as_deref().is_none_or(|prior: &str| prior < id) {
            latest = Some(id.to_owned());
        }
    }
    let Some(id) = latest else { return Ok(None) };
    let issued = issuer.issued(&id).map_err(public)?;
    if issued.plan.choices.instance.as_deref() != instance
        || issued.plan.context.target_account
            != crate::install::platform::account().ok_or_else(|| public(Fault::conflict()))?
        || transaction::real_directory(Path::new(&issued.plan.context.state_dir)).map_err(public)?
            != transaction::real_directory(state).map_err(public)?
    {
        return Err(public(Fault::conflict()));
    }
    let result = super::status(state, instance, &id).unwrap_or_else(|result| *result);
    Ok(Some((id, issued.plan, result)))
}
