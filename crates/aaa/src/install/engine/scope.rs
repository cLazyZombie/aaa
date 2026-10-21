//! 실제 파일 범위와 고정 OS 작업을 하나의 서명 범위에 묶는다.
use super::*;
pub(super) fn all(plan: &InstallPlan) -> Result<Vec<InstallPermission>> {
    complete(plan, transaction::permissions(plan)?)
}
pub(super) fn complete(plan: &InstallPlan, mut files: Vec<InstallPermission>) -> Result<Vec<InstallPermission>> {
    files.extend(supervisor::permissions(plan)?);
    files.extend(super::enrollment::permissions(plan)?);
    files.extend(web::permissions(plan)?);
    files.extend(user_path::permissions(plan)?);
    Ok(files)
}
pub(super) fn enrollment(plan: &InstallPlan) -> Vec<InstallPermission> {
    aaa_config::install::enrollment_permission(plan).into_iter().collect()
}
pub(super) fn files(scope: &[InstallPermission]) -> Vec<InstallPermission> {
    scope
        .iter()
        .filter(|p| {
            matches!(
                p.operation,
                InstallOperation::WriteFile
                    | InstallOperation::StageBinary
                    | InstallOperation::StageLauncher
                    | InstallOperation::ConfigureHook
                    | InstallOperation::PrepareWorkRoot
                    | InstallOperation::CreateProject
            )
        })
        .cloned()
        .collect()
}
