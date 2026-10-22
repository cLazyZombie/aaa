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

/// 옛 installer는 hook의 별칭 target을 서명하고 journal에는 정규 경로를 저장했다.
/// 서명된 문서를 고치지 않고 같은 operation·전후 SHA와 현재 파일 경로 동등성만 입증한다.
pub(super) fn same_permission(actual: &InstallPermission, signed: &InstallPermission) -> Result<bool> {
    if actual.operation != signed.operation
        || actual.before_sha256 != signed.before_sha256
        || actual.after_sha256 != signed.after_sha256
    {
        return Ok(false);
    }
    if actual.target == signed.target {
        return Ok(true);
    }
    let normalize = |target: &str| -> Result<PathBuf> {
        match actual.operation {
            InstallOperation::WriteFile
            | InstallOperation::StageBinary
            | InstallOperation::StageLauncher
            | InstallOperation::ConfigureHook => transaction::target(Path::new(target)),
            InstallOperation::PrepareWorkRoot | InstallOperation::CreateProject => {
                if Path::new(target).is_dir() {
                    transaction::real_directory(Path::new(target))
                } else {
                    transaction::target(Path::new(target))
                }
            }
            _ => Err(Fault::conflict()),
        }
    };
    if files(std::slice::from_ref(actual)).is_empty() {
        return Ok(false);
    }
    Ok(normalize(&actual.target)? == normalize(&signed.target)?)
}
pub(super) fn same_permissions(actual: &[InstallPermission], signed: &[InstallPermission]) -> Result<bool> {
    if actual.len() != signed.len() {
        return Ok(false);
    }
    for (actual, signed) in actual.iter().zip(signed) {
        if !same_permission(actual, signed)? {
            return Ok(false);
        }
    }
    Ok(true)
}
