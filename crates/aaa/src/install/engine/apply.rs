//! 공개 CLI의 입력을 검증된 원래 계획과 고정 worker에만 전달한다.
use super::*;
use authorization::Authority;
use input::Input;

type PublicResult = std::result::Result<InstallResult, Box<InstallResult>>;
fn required(planned: Option<InstallResult>) -> InstallResult {
    crate::install::authorization_required(planned)
}

pub(in crate::install) fn run(
    invocation: &InstallInvocation,
    planned: Option<(InstallResult, InstallPlan)>,
    state: &Path,
    instance: Option<&str>,
    reporter: &mut crate::install::output::Reporter,
) -> PublicResult {
    let authority = match Authority::open(state) {
        Ok(authority) => authority,
        Err(source_error) => {
            aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            return Ok(required(planned.map(|p| p.0)));
        }
    };
    let resume = invocation.resume.as_deref();
    let original = resume.and_then(|id| {
        authority
            .issued(id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
    });
    if resume.is_some() && original.is_none() {
        return Ok(required(None));
    }
    let provided = if let Some(path) = &invocation.authorization_file {
        match Input::file(Path::new(path)) {
            Ok(authorization) => Some(authorization),
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                return Ok(required(planned.map(|p| p.0)));
            }
        }
    } else if invocation.authorization_stdin {
        match Input::authorization(std::io::stdin().lock()) {
            Ok(authorization) => Some(authorization),
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                return Ok(required(planned.map(|p| p.0)));
            }
        }
    } else {
        None
    };
    if let Some((result, _)) = &planned
        && !result.needs_user.is_empty()
    {
        return Ok(result.clone());
    }
    let input = if invocation.secrets_stdin {
        Input::framed(std::io::stdin().lock(), provided).map_err(|e| e.public())?
    } else if let Some(authorization) = provided {
        Input { authorization, password: None }
    } else if let Some(issued) = &original {
        Input { authorization: issued.authorization.clone(), password: None }
    } else {
        return Ok(required(planned.map(|p| p.0)));
    };
    execute(invocation, planned, state, instance, input, reporter)
}
pub(in crate::install) fn inline(
    invocation: &InstallInvocation,
    planned: (InstallResult, InstallPlan),
    consent: &crate::install::review::Consent,
    reporter: &mut crate::install::output::Reporter,
) -> PublicResult {
    if &planned.1 != consent.plan() {
        return Err(crate::install::error("installation_content_conflict", 4));
    }
    let plan = consent.plan();
    let state = Path::new(&plan.context.state_dir);
    if consent.resume().is_none() && scope::all(plan).map_err(|e| e.public())? != consent.scope() {
        return Err(crate::install::error("installation_content_conflict", 4));
    }
    let authority = Authority::initialize(state).map_err(|e| e.public())?;
    let now = aaa_common::util::now_ms();
    let authorization = match consent.resume() {
        Some(id) => authority.refresh(id, now, 5 * 60 * 1000),
        None => authority.issue(plan, consent.scope().to_vec(), now, 5 * 60 * 1000),
    }
    .map_err(|e| e.public())?;
    execute(
        invocation,
        Some(planned),
        state,
        plan.choices.instance.as_deref(),
        Input { authorization, password: None },
        reporter,
    )
}
fn execute(
    invocation: &InstallInvocation,
    planned: Option<(InstallResult, InstallPlan)>,
    state: &Path,
    instance: Option<&str>,
    mut input: Input,
    reporter: &mut crate::install::output::Reporter,
) -> PublicResult {
    let authority = Authority::open(state).map_err(|e| e.public())?;
    let resume = invocation.resume.as_deref();
    let id = &input.authorization.claims.installation_id;
    let issued = match authority.issued(id) {
        Ok(issued) => issued,
        Err(source_error) => {
            aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            return Ok(required(planned.map(|p| p.0)));
        }
    };
    if resume.is_some_and(|resume| resume != id) || issued.plan.choices.instance.as_deref() != instance {
        return Err(crate::install::error("installation_target_mismatch", 4));
    }
    if authority
        .validate(
            &input.authorization,
            &issued.plan,
            &input.authorization.claims.permissions,
            aaa_common::util::now_ms(),
        )
        .as_ref()
        .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_err()
    {
        return Ok(required(planned.map(|p| p.0)));
    }
    if let Some((result, plan)) = &planned {
        if *plan != issued.plan {
            return Err(crate::install::error("installation_content_conflict", 4));
        }
        if !result.needs_user.is_empty() {
            return Ok(result.clone());
        }
    } else {
        let mut supplied = invocation.choices.clone();
        supplied.instance = None;
        supplied.config_dir = None;
        supplied.state_dir = None;
        if supplied.service_password_source == issued.plan.choices.service_password_source {
            supplied.service_password_source = None;
        }
        if supplied != InstallChoices::default() {
            return Err(crate::install::error("installation_resume_changes_require_new_plan", 4));
        }
    }
    if issued.plan.choices.service != Some(InstallService::None)
        && !matches!(
            (issued.plan.context.platform.os, issued.plan.choices.service),
            (InstallOs::Windows, Some(InstallService::WindowsScm)) | (InstallOs::Macos, Some(InstallService::Launchd))
        )
    {
        return Err(crate::install::error("unsupported_service_platform", 5));
    }
    if input.password.is_some() {
        if issued.plan.choices.service != Some(InstallService::WindowsScm) {
            return Err(crate::install::error("installation_unexpected_secret_input", 2));
        }
        #[cfg(windows)]
        if !supervisor::requires_password(&issued.plan).map_err(|e| e.public())? {
            return Err(crate::install::error("installation_unexpected_secret_input", 2));
        }
    }
    reporter.configure(invocation, &issued.plan)?;
    #[cfg(windows)]
    if issued.plan.choices.service == Some(InstallService::WindowsScm)
        && input.password.is_none()
        && supervisor::requires_password(&issued.plan).map_err(|e| e.public())?
    {
        let pending = match issued.plan.choices.service_password_source {
            Some(InstallPasswordSource::Prompt)
                if !invocation.non_interactive && !invocation.json && !invocation.json_stream =>
            {
                super::super::review::require_outside(state)?;
                input.password = Some(input::prompt().map_err(|e| e.public())?);
                None
            }
            Some(InstallPasswordSource::Broker) => Some("installation_password_broker_unavailable"),
            Some(InstallPasswordSource::Prompt) => Some("installation_private_password_input_required"),
            _ => Some("service_account_auth_unavailable"),
        };
        if let Some(code) = pending {
            let mut result = crate::install::needs(code, "service");
            result.installation_id = Some(id.clone());
            result.plan_sha256 = Some(input.authorization.claims.plan_sha256.clone());
            result.resume = Some(InstallResume { installation_id: id.clone() });
            return Ok(*result);
        }
    }
    #[cfg(not(windows))]
    let _ = &mut input;

    let known_id = input.authorization.claims.installation_id.clone();
    let known_hash = input.authorization.claims.plan_sha256.clone();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|source_error| {
        aaa_common::diagnostic::context(source_error, crate::install::error("installation_worker_unavailable", 3))
    })?;
    runtime
        .block_on(worker::launch(
            state,
            input,
            resume.is_some(),
            reporter,
            !invocation.non_interactive && !invocation.json && !invocation.json_stream,
        ))
        .map_err(|fault| {
            let mut result =
                if fault.exit == 3 { crate::install::needs(fault.code, "installation") } else { fault.public() };
            crate::install::context(&issued.plan, &mut result);
            result.installation_id = Some(known_id.clone());
            result.plan_sha256 = Some(known_hash);
            result.resume = Some(InstallResume { installation_id: known_id });
            result
        })
}
