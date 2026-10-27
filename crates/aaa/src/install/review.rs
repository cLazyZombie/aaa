//! 외부 TTY의 검토/동의. 발급 직전에도 같은 현지 계획을 다시 대조한다.
use super::*;
use aaa_proto::{DeviceRole, FrontRequest, FrontResponse};
use args::Action;
use std::path::PathBuf;

type Result<T> = std::result::Result<T, Box<InstallResult>>;
pub(super) fn require_outside(state: &Path) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|source_error| {
        aaa_common::diagnostic::context(source_error, error("installation_authorization_unavailable", 3))
    })?;
    rt.block_on(outside_async(state))
}
pub(super) async fn outside_async(state: &Path) -> Result<()> {
    crate::person::require("설치 승인/보호 표시").map_err(|source_error| {
        aaa_common::diagnostic::context(source_error, error("installation_external_tty_required", 3))
    })?;
    if !std::io::stdout().is_terminal() {
        return Err(error("installation_private_display_required", 3));
    }
    match aaa_common::front::FrontClient::connect(state).await {
        Ok(mut client) => {
            if client.role != DeviceRole::Local || client.proto.minor < 42 {
                return Err(error("installation_external_tty_required", 3));
            }
            match client.call(FrontRequest::DeployContext).await {
                Ok(Ok(FrontResponse::DeployContext { session: false, .. })) => Ok(()),
                _ => Err(error("installation_external_tty_required", 3)),
            }
        }
        Err(_) if !state.join("host").exists() => {
            if aaa_common::caller::judge(std::process::id(), &[], &[]) == aaa_common::caller::Caller::Outside {
                Ok(())
            } else {
                Err(error("installation_external_tty_required", 3))
            }
        }
        _ => Err(error("installation_external_tty_required", 3)),
    }
}

pub(super) fn run(args: &Args, action: &Action, invocation: &InstallInvocation, json: bool) -> Result<InstallResult> {
    if args.automated(json)
        || args.draft().is_some()
        || invocation.events_file.is_some()
        || invocation.authorization_stdin
        || invocation.authorization_file.is_some()
        || invocation.secrets_stdin
        || invocation.service_password_ref.is_some()
        || invocation.dry_run
        || invocation.status.is_some()
        || invocation.resume.is_some()
    {
        return Err(error("installation_review_mode_conflict", 2));
    }
    let Action::Authorize { plan: path, out, resume } = action;
    let state = invocation
        .choices
        .state_dir
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| aaa_config::paths::default_state_dir_for(invocation.choices.instance.as_deref()));
    let state = engine::normalize_state(&state)?;
    require_outside(&state)?;
    let plan = if let Some(id) = resume {
        engine::validate_id(id)?;
        engine::original_plan(&state, id)?
    } else if let Some(path) = path {
        engine::read_plan(path)?
    } else {
        detect::inspect(invocation)?.1
    };
    if Path::new(&plan.context.state_dir) != state {
        return Err(error("installation_target_mismatch", 4));
    }
    let consent = confirm(args, &plan, resume.as_deref())?;
    engine::issue_authorization(&consent, out)
}

/// 실제 외부 TTY의 명시 동의를 지난 값. 역직렬화나 bool/환경 flag로 만들지 않는다.
pub(super) struct Consent {
    plan: InstallPlan,
    resume: Option<String>,
    scope: Vec<InstallPermission>,
}
impl Consent {
    pub(super) fn plan(&self) -> &InstallPlan {
        &self.plan
    }
    pub(super) fn scope(&self) -> &[InstallPermission] {
        &self.scope
    }
    pub(super) fn resume(&self) -> Option<&str> {
        self.resume.as_deref()
    }
}
pub(super) fn confirm(args: &Args, plan: &InstallPlan, resume: Option<&str>) -> Result<Consent> {
    let state = Path::new(&plan.context.state_dir);
    require_outside(state)?;
    let preview = engine::preview(plan, resume)?;
    println!("설치 계획: {}", aaa_config::install::plan_sha256(plan));
    println!("실행 파일 SHA-256: {}", plan.context.binary_sha256);
    println!(
        "대상 계정: {} / 역할: {:?} / instance: {:?}",
        plan.context.target_account, plan.choices.role, plan.choices.instance
    );
    for p in &preview {
        println!("{:?}: {} (전 {:?} → 후 {:?})", p.operation, p.target, p.before_sha256, p.after_sha256);
    }
    println!("승인은 이 계획에만 5분간 유효합니다. --yes는 발급 권한이 아닙니다.");
    let answer = if args.plain() {
        super::prompt::line("승인을 발급하려면 정확히 yes를 입력하세요: ")?
    } else {
        inquire::Text::new("승인을 발급하려면 정확히 yes를 입력하세요:").prompt().map_err(|failure| match failure {
            inquire::InquireError::OperationCanceled | inquire::InquireError::OperationInterrupted => {
                error("installation_cancelled", 130)
            }
            _ => error("installation_review_unavailable", 3),
        })?
    };
    if answer != "yes" {
        return Err(error("installation_review_cancelled", 3));
    }
    eprintln!("diag-review-stage consent-read");
    require_outside(state)?;
    eprintln!("diag-review-stage outside-checked");
    if resume.is_none() {
        let check = InstallInvocation {
            choices: plan.choices.clone(),
            dry_run: true,
            non_interactive: true,
            ..Default::default()
        };
        eprintln!("diag-review-stage inspect-enter");
        let current = detect::inspect(&check)?.1;
        eprintln!("diag-review-stage inspect-exit");
        if current != *plan || engine::preview(&current, None)? != preview {
            return Err(error("installation_content_conflict", 4));
        }
    }
    eprintln!("diag-review-stage consent-ready");
    Ok(Consent { plan: plan.clone(), resume: resume.map(str::to_owned), scope: preview })
}

/// 만료 또는 마법사의 명시적 재개 선택은 같은 원래 계획을 다시 검토한다.
/// 사용자에게 승인 파일 명령을 요구하지 않으며 서명·범위·5분 상한은 그대로다.
pub(super) fn resume(
    args: &Args,
    invocation: &InstallInvocation,
    reporter: &mut output::Reporter,
) -> Result<InstallResult> {
    let id = invocation.resume.as_deref().ok_or_else(|| error("invalid_installation_id", 2))?;
    let state = invocation
        .choices
        .state_dir
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| aaa_config::paths::default_state_dir_for(invocation.choices.instance.as_deref()));
    require_outside(&state)?;
    let mut observed = engine::status(&state, invocation.choices.instance.as_deref(), id)?;
    if observed.exit_code == 0
        && matches!(observed.status, InstallStatus::Completed | InstallStatus::Configured | InstallStatus::Unchanged)
    {
        return Ok(observed);
    }
    let plan = engine::original_plan(&state, id)?;
    if !engine::executor_matches(&plan)? {
        let mut result = needs("installation_resume_executor_changed", "installation");
        result.installation_id = Some(id.into());
        context(&plan, &mut result);
        return Ok(*result);
    }
    let consent = confirm(args, &plan, Some(id))?;
    // 과거 pending 원인은 새 동의 뒤 worker에서 다시 관찰한다. 파일/권한 검증을 생략하지 않는다.
    observed.status = InstallStatus::Planned;
    observed.exit_code = 0;
    observed.error = None;
    observed.needs_user.clear();
    engine::apply_inline(invocation, (observed, plan), &consent, reporter)
}
