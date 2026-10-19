//! 생성 root 이력은 작업 디스크가 아니라 agent 상태 폴더에 둔다(t3code previousWorktreesDirectories).
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RootInfo {
    pub path: PathBuf,
    pub real_path: PathBuf,
    pub mount: Option<PathBuf>,
    pub imported: bool,
}
impl RootInfo {
    pub(crate) fn check(&self, path: &Path) -> Result<()> {
        if check(&self.path, self.mount.as_deref())? != self.real_path {
            bail!("등록한 작업 root의 실제 경로가 바뀌었다: {}", self.path.display());
        }
        check_managed_path(&self.path, self.mount.as_deref(), path)?;
        if !check(path, self.mount.as_deref())?.starts_with(&self.real_path) {
            bail!("작업 경로가 등록된 root 밖을 가리킨다: {}", path.display());
        }
        Ok(())
    }
    pub(crate) fn portable(&self) -> bool {
        self.mount.is_some() || inferred_mount(&self.path).is_some() || inferred_mount(&self.real_path).is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootState {
    pub schema: u32,
    pub owner: String,
    pub roots: Vec<RootInfo>,
    /// 마지막 agent 시작에 적용된 경로다. 프로세스가 지금 연결됐다는 증명은 아니다.
    pub applied: Option<PathBuf>,
}

pub fn inspect(state: &Path) -> Result<Option<RootState>> {
    let saved: Option<RootState> = crate::journal::read(&state.join("work-roots.json"))?;
    if let Some(s) = &saved {
        if s.schema != 1 || s.owner.is_empty() || !s.owner.bytes().all(|c| c.is_ascii_alphanumeric()) {
            bail!("작업 root 이력의 schema·소유자가 올바르지 않다");
        }
        for root in &s.roots {
            if !root.path.is_absolute()
                || !root.real_path.is_absolute()
                || root.path.components().any(|c| c == std::path::Component::ParentDir)
                || root.mount.as_ref().is_some_and(|m| !m.is_absolute())
            {
                bail!("작업 root 이력의 경로가 올바르지 않다");
            }
        }
    }
    Ok(saved)
}

/// 준비·정리는 공유 잠금, root 전환은 독점 잠금이다. CLI와 agent 사이에도 적용한다.
pub(crate) struct Lease(std::fs::File);
impl Drop for Lease {
    fn drop(&mut self) {
        // close만 하면 fork~exec 사이 자식이 잠금 참조를 붙잡을 수 있다(deploy::Lease와 같다).
        if let Err(e) = self.0.unlock() {
            tracing::error!(%e, "작업 root 잠금 해제 실패");
        }
    }
}
fn lease(state: &Path, shared: bool) -> Result<Lease> {
    if !state.is_absolute() {
        bail!("agent 상태 폴더는 절대 경로여야 한다");
    }
    aaa_common::util::ensure_private_dir(state)?;
    let path = state.join("work-root.lock");
    match path.symlink_metadata() {
        Ok(m) if !m.is_file() => bail!("작업 root 잠금은 일반 파일이어야 한다"),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    let result = if shared { file.try_lock_shared() } else { file.try_lock() };
    result.map_err(|e| anyhow::anyhow!("작업 root 전환 또는 준비·정리가 진행 중이다: {e}"))?;
    Ok(Lease(file))
}
pub(crate) fn shared(state: &Path) -> Result<Lease> {
    lease(state, true)
}

pub struct RootTransaction {
    state: PathBuf,
    _lease: Lease,
    data: RootState,
}
impl RootTransaction {
    pub fn open(state: &Path) -> Result<Self> {
        let lease = lease(state, false)?;
        let data = inspect(state)?.unwrap_or_else(|| RootState {
            schema: 1,
            owner: aaa_common::util::new_id(),
            roots: Vec::new(),
            applied: None,
        });
        let tx = Self { state: state.into(), _lease: lease, data };
        // root 소유 표지를 만들기 전에 owner를 먼저 내구화한다. 중단 뒤에도 같은 소유자다.
        tx.save()?;
        Ok(tx)
    }
    fn save(&self) -> Result<()> {
        crate::journal::write(&self.state.join("work-roots.json"), &self.data)
    }
    /// 가져오기 완료 뒤에만 설정을 바꿀 수 있다. 기존 root는 지우거나 이동하지 않는다.
    pub fn register(&mut self, path: &Path, mount: Option<&Path>) -> Result<(RootInfo, usize)> {
        let old = self.data.roots.iter().find(|r| r.path == path).cloned();
        let mount = match (&old, mount) {
            (Some(old), Some(m)) if old.mount.as_ref().is_some_and(|p| p != m) => {
                bail!("같은 작업 root의 mount 지점을 바꿀 수 없다")
            }
            (Some(old), _) => mount.map(PathBuf::from).or(old.mount.clone()),
            _ => mount.map(PathBuf::from),
        };
        let real_path = check(path, mount.as_deref())?;
        if old.as_ref().is_some_and(|r| r.real_path != real_path) {
            bail!("등록한 작업 root의 실제 경로가 바뀌었다");
        }
        if self.data.roots.iter().any(|r| r.path != path && r.real_path == real_path) {
            bail!("같은 실제 작업 root를 다른 경로로 등록할 수 없다");
        }
        let marker = path.join(".aaa-root-owner.json");
        if let Some(owner) = crate::journal::read::<RootOwner>(&marker)? {
            self.check_owner(&owner)?;
        }
        initialize(path, mount.as_deref())?;
        // 다른 인스턴스가 같은 root를 동시에 차지하면 create_new의 한 소유자만 성공한다.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&marker) {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(&serde_json::to_vec(&RootOwner { schema: 1, owner: self.data.owner.clone() })?)?;
                f.sync_all()?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                self.check_owner(&crate::journal::read(&marker)?.context("root 소유 표지가 없다")?)?;
            }
            Err(e) => return Err(e.into()),
        }
        let index = match self.data.roots.iter().position(|r| r.path == path) {
            Some(i) => {
                self.data.roots[i].mount = mount;
                i
            }
            None => {
                self.data.roots.push(RootInfo { path: path.into(), real_path, mount, imported: false });
                self.data.roots.len() - 1
            }
        };
        self.save()?;
        let mut imported = 0;
        if !self.data.roots[index].imported {
            imported = crate::threads::import_legacy(&self.state, &self.data.roots[index])?;
            self.data.roots[index].imported = true;
            self.save()?;
        }
        Ok((self.data.roots[index].clone(), imported))
    }
    fn check_owner(&self, owner: &RootOwner) -> Result<()> {
        if owner.schema != 1 || owner.owner != self.data.owner {
            bail!("다른 agent 인스턴스가 소유한 작업 root다");
        }
        Ok(())
    }
    pub fn apply(&mut self, path: &Path, mount: Option<&Path>) -> Result<RootState> {
        self.register(path, mount)?;
        self.data.applied = Some(path.into());
        self.save()?;
        Ok(self.data.clone())
    }
}
#[derive(Serialize, Deserialize)]
struct RootOwner {
    schema: u32,
    owner: String,
}

pub fn activate(state: &Path, path: &Path, mount: Option<&Path>) -> Result<RootState> {
    RootTransaction::open(state)?.apply(path, mount)
}
