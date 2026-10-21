//! SID·home·파일 존재·CLI 판만으로 서비스 인증을 ready라고 추정하지 않는다.
use super::*;
pub(super) fn observe(plan: &InstallPlan, result: &mut InstallResult) {
    let mut selected = false;
    let deferred = plan.choices.login == Some(InstallLogin::Defer);
    for harness in &plan.choices.harness {
        let name = match harness {
            InstallHarness::Claude => "claude",
            InstallHarness::Codex => "codex",
            InstallHarness::None => continue,
        };
        selected = true;
        result.checks.push(InstallCheck {
            name: format!("{name}_login"),
            state: if deferred { InstallCheckState::NotRequested } else { InstallCheckState::Unknown },
        });
        result.checks.push(InstallCheck {
            name: format!("{name}_hook_trust"),
            state: if plan.choices.hooks == Some(InstallHooks::Install) {
                InstallCheckState::Unknown
            } else {
                InstallCheckState::NotRequested
            },
        });
        if !deferred {
            result.needs_user.push(InstallNeedsUser { code: "harness_auth_runtime_unverified".into(), target: name.into(), message: "실제 서비스 문맥의 인증 확인이 남았습니다. 인증을 복사하거나 저장 방식을 바꾸지 않습니다. 확인을 미루려면 login=defer의 새 계획을 검토하세요".into() });
        }
    }
    if selected && !deferred && result.exit_code == 0 {
        result.status = InstallStatus::NeedsUser;
        result.exit_code = 3;
        result.error = None;
    }
    if !selected {
        result.checks.push(InstallCheck { name: "harness_login".into(), state: InstallCheckState::NotRequested });
    }
}
pub(super) fn final_status(plan: &InstallPlan, result: &mut InstallResult) {
    if result.exit_code != 0 || result.status == InstallStatus::Unchanged {
        return;
    }
    let configured = plan.choices.start != Some(InstallStart::Now)
        || plan.choices.service == Some(InstallService::None)
        || plan.choices.harness.iter().any(|h| *h != InstallHarness::None)
            && plan.choices.login == Some(InstallLogin::Defer)
        || plan.choices.role == Some(InstallRole::Agent) && plan.choices.enroll == Some(InstallEnroll::Skip);
    result.status = if configured { InstallStatus::Configured } else { InstallStatus::Completed };
}
