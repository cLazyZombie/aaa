//! PATH 기본은 보존이다. Windows 사용자 값만 별도 범위/registry transaction으로 변경한다.
use super::*;
#[cfg(any(windows, test))]
mod model;
#[cfg(windows)]
mod native;
pub(super) fn permissions(plan: &InstallPlan) -> Result<Vec<InstallPermission>> {
    if plan.choices.path != Some(InstallPathPolicy::User) {
        return Ok(Vec::new());
    }
    #[cfg(windows)]
    {
        native::permissions(plan)
    }
    #[cfg(not(windows))]
    {
        Err(Fault { code: "unsupported_user_path_platform", exit: 5, cause: None })
    }
}
pub(super) fn apply(plan: &InstallPlan, lease: &authorization::Lease) -> Result<bool> {
    if plan.choices.path != Some(InstallPathPolicy::User) {
        return Ok(false);
    }
    #[cfg(windows)]
    {
        native::apply(plan, lease)
    }
    #[cfg(not(windows))]
    {
        let _ = lease;
        Err(Fault { code: "unsupported_user_path_platform", exit: 5, cause: None })
    }
}
pub(super) fn observe(plan: &InstallPlan, result: &mut InstallResult) -> Result<()> {
    if plan.choices.path != Some(InstallPathPolicy::User) {
        return Ok(());
    }
    #[cfg(windows)]
    {
        native::observe(plan, result)
    }
    #[cfg(not(windows))]
    {
        let _ = result;
        Err(Fault { code: "unsupported_user_path_platform", exit: 5, cause: None })
    }
}
