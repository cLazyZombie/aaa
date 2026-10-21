//! 실제 파일 범위와 승인 guard의 결속. worker가 내구 ID를 저장하기 전 파일 적용을 시작하지 않는다.
use super::*;
use crate::install::engine::authorization::Lease;

fn file_permission(kind: Kind, path: &Path, before: Option<String>, after: String) -> Result<InstallPermission> {
    Ok(InstallPermission {
        operation: match kind {
            Kind::Binary => InstallOperation::StageBinary,
            Kind::Config => InstallOperation::WriteFile,
            Kind::Project => InstallOperation::CreateProject,
            Kind::Hook => InstallOperation::ConfigureHook,
        },
        target: path.to_str().ok_or_else(Fault::conflict)?.into(),
        before_sha256: before,
        after_sha256: Some(after),
    })
}

fn support_permissions(
    mut permissions: Vec<InstallPermission>,
    directories: &[PathBuf],
    root: Option<&RootSpec>,
) -> Result<Vec<InstallPermission>> {
    for directory in directories {
        permissions.push(InstallPermission {
            operation: InstallOperation::CreateProject,
            target: directory.to_str().ok_or_else(Fault::conflict)?.into(),
            before_sha256: None,
            after_sha256: None,
        });
    }
    if let Some(root) = root {
        permissions.push(InstallPermission {
            operation: InstallOperation::PrepareWorkRoot,
            target: root.path.to_str().ok_or_else(Fault::conflict)?.into(),
            before_sha256: None,
            after_sha256: Some(hash(&json_bytes(root)?)),
        });
    }
    Ok(permissions)
}

impl Request {
    fn permissions(&self) -> Result<Vec<InstallPermission>> {
        let permissions = self
            .files
            .iter()
            .map(|file| file_permission(file.kind, &file.path, file.before.clone(), hash(&file.bytes)))
            .collect::<Result<Vec<_>>>()?;
        support_permissions(permissions, &self.directories, self.root.as_ref())
    }
}

impl Manifest {
    fn permissions(&self) -> Result<Vec<InstallPermission>> {
        let permissions = self
            .files
            .iter()
            .map(|file| file_permission(file.kind, &file.path, file.before.clone(), file.after.clone()))
            .collect::<Result<Vec<_>>>()?;
        support_permissions(permissions, &self.directories, self.root.as_ref())
    }
}

/// 계획 미리보기/발급용 실제 파일 범위. 디렉터리·journal·잠금을 만들지 않는다.
pub(in crate::install::engine) fn permissions(plan: &InstallPlan) -> Result<Vec<InstallPermission>> {
    prepare::install(plan.clone())?.permissions()
}

/// 재개의 예상 범위는 일부 적용된 현재 파일을 새 계획처럼 해석하지 않고 원래 manifest에서 읽는다.
pub(in crate::install::engine) fn resume_scope(
    state: &Path,
    id: &str,
    instance: Option<&str>,
) -> Result<(InstallPlan, Vec<InstallPermission>)> {
    if !id_ok(id) {
        return Err(Fault::conflict());
    }
    let journal = read_journal(state, id, instance)?;
    let manifest = load(state, &journal)?;
    if real_directory(state)? != real_directory(Path::new(&manifest.plan.context.state_dir))? {
        return Err(Fault::conflict());
    }
    let scope = manifest.permissions()?;
    Ok((manifest.plan, scope))
}

/// worker가 내부 파일 job ID를 내구 기록한 뒤에만 실제 적용을 시작한다.
pub(in crate::install::engine) struct AuthorizedTransaction(Transaction);
impl AuthorizedTransaction {
    pub(in crate::install::engine) fn create(plan: InstallPlan, lease: &Lease) -> Result<Self> {
        let request = prepare::install(plan)?;
        lease.permits(&request.plan, &request.permissions()?)?;
        Ok(Self(Transaction::create_with_id(request, Some(lease.id()))?))
    }
    pub(in crate::install::engine) fn resume(
        state: &Path,
        id: &str,
        instance: Option<&str>,
        lease: &Lease,
    ) -> Result<Self> {
        let (plan, scope) = resume_scope(state, id, instance)?;
        lease.permits(&plan, &scope)?;
        let transaction = Transaction::resume(state, id, instance)?;
        lease.permits(&transaction.manifest.plan, &transaction.manifest.permissions()?)?;
        Ok(Self(transaction))
    }
    pub(in crate::install::engine) fn id(&self) -> &str {
        &self.0.journal.id
    }
    pub(in crate::install::engine) fn run(&mut self) -> Result<InstallResult> {
        self.0.run()
    }
}
