//! 파일별 stage/commit와 내구 journal. 권한 검증은 이 엔진에 진입하는 I6 executor의 책임이다.
use super::*;
use std::io::{Read, Write};

mod prepare;
mod recovery;
mod references;
mod reverse;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileSpec {
    path: PathBuf,
    kind: Kind,
    before: Option<String>,
    after: String,
    before_mode: Option<u32>,
    before_inode: Option<String>,
    force: bool,
    mode: u32,
    preserve: bool,
    shared: bool,
    reference_before: Option<references::Record>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootSpec {
    path: PathBuf,
    mount: Option<PathBuf>,
    volume: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: u32,
    plan: InstallPlan,
    files: Vec<FileSpec>,
    root: Option<RootSpec>,
    directories: Vec<PathBuf>,
}

struct Proposed {
    path: PathBuf,
    kind: Kind,
    before: Option<String>,
    before_mode: Option<u32>,
    before_inode: Option<String>,
    force: bool,
    bytes: Vec<u8>,
    mode: Option<u32>,
    preserve: bool,
    shared: bool,
}
struct Request {
    plan: InstallPlan,
    files: Vec<Proposed>,
    root: Option<RootSpec>,
    directories: Vec<PathBuf>,
}

pub(super) struct Locks(Vec<File>);
impl Drop for Locks {
    fn drop(&mut self) {
        #[cfg(unix)]
        for file in &self.0 {
            let _ = file.unlock();
        }
        // Windows는 handle의 share reservation을 닫아 해제한다.
        self.0.clear();
    }
}
struct Transaction {
    state: PathBuf,
    dir: PathBuf,
    manifest: Manifest,
    journal: Journal,
    reused: bool,
    _locks: Locks,
}

pub(super) fn real_directory(path: &Path) -> Result<PathBuf> {
    let real = aaa_agent::work_root::check(path, None).map_err(|_| Fault::conflict())?;
    for ancestor in real.ancestors() {
        match ancestor.symlink_metadata() {
            Ok(meta) if meta.is_dir() => return Ok(real),
            Ok(_) => return Err(Fault::conflict()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Fault::io(e)),
        }
    }
    Err(Fault::conflict())
}
fn target(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() || path.components().any(|part| part == std::path::Component::ParentDir) {
        return Err(Fault::conflict());
    }
    let parent = real_directory(path.parent().ok_or_else(Fault::conflict)?)?;
    let path = parent.join(path.file_name().ok_or_else(Fault::conflict)?);
    if path.symlink_metadata().is_ok_and(|m| !m.is_file()) {
        return Err(Fault::conflict());
    }
    Ok(path)
}
fn mode(meta: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        if meta.permissions().readonly() { 0o400 } else { 0o600 }
    }
}
fn snapshot(path: &Path) -> Result<(Option<String>, Option<u32>)> {
    let meta = match path.symlink_metadata() {
        Ok(m) if m.is_file() => m,
        Ok(_) => return Err(Fault::conflict()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Windows의 PATH_NOT_FOUND는 일반 파일 아래 경로에도 올 수 있다.
            // proposal과 실제 적용이 같은 디렉터리 조상 규칙으로 없음 여부를 판정한다.
            real_directory(path.parent().ok_or_else(Fault::conflict)?)?;
            return Ok((None, None));
        }
        Err(e) => return Err(Fault::io(e)),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != unsafe { libc::geteuid() } || mode(&meta) & 0o6000 != 0 {
            return Err(Fault::conflict());
        }
    }
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let mut file = options.open(path).map_err(Fault::io)?;
    if !file.metadata().map_err(Fault::io)?.is_file() || meta.len() > 1024 * 1024 * 1024 {
        return Err(Fault::conflict());
    }
    let opened = file.metadata().map_err(Fault::io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != meta.dev() || opened.ino() != meta.ino() {
            return Err(Fault::conflict());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(Fault::conflict());
        }
    }
    let mut sha = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(Fault::io)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > 1024 * 1024 * 1024 {
            return Err(Fault::conflict());
        }
        sha.update(&buf[..n]);
    }
    let finished = file.metadata().map_err(Fault::io)?;
    if finished.len() != total || opened.modified().ok() != finished.modified().ok() || mode(&opened) != mode(&finished)
    {
        return Err(Fault::conflict());
    }
    Ok((Some(hex::encode(sha.finalize())), Some(mode(&finished))))
}
fn resource_key(path: &Path) -> String {
    #[cfg(windows)]
    {
        hash(path.to_string_lossy().to_lowercase().as_bytes())
    }
    #[cfg(not(windows))]
    {
        hash(path.as_os_str().as_encoded_bytes())
    }
}
fn locks(state: &Path, manifest: &Manifest) -> Result<Locks> {
    let config = real_directory(Path::new(&manifest.plan.context.config_dir))?;
    let root = manifest.root.as_ref().map(|root| real_directory(&root.path)).transpose()?;
    let mut resources: Vec<_> =
        manifest.files.iter().map(|f| f.path.clone()).chain([state.to_owned(), config]).chain(root).collect();
    resources.sort();
    resources.dedup();
    let mut held = Locks(Vec::new());
    for resource in resources {
        let parent = real_directory(resource.parent().ok_or_else(Fault::conflict)?)?;
        private::parents(&parent).map_err(Fault::io)?;
        let key = resource_key(&resource);
        let path = parent.join(format!(".aaa-install-resource-{key}.lock"));
        let file = private::lock(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                Fault { code: "installation_resource_busy", exit: 4 }
            } else {
                Fault::io(e)
            }
        })?;
        held.0.push(file);
    }
    Ok(held)
}
fn json_bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| Fault::conflict())
}
fn pointer(path: &Path) -> Result<Option<String>> {
    let bytes = match private::read(path, 1024) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Fault::conflict()),
    };
    let id: String = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
    if !id_ok(&id) {
        return Err(Fault::conflict());
    }
    Ok(Some(id))
}
fn identity(path: &Path) -> Result<String> {
    let meta = path.symlink_metadata().map_err(Fault::io)?;
    if !meta.is_file() {
        return Err(Fault::conflict());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(format!("{:x}-{:x}", meta.dev(), meta.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle};
        let file = File::open(path).map_err(Fault::io)?;
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(Fault::io(std::io::Error::last_os_error()));
        }
        Ok(format!("{:x}-{:x}-{:x}", info.dwVolumeSerialNumber, info.nFileIndexHigh, info.nFileIndexLow))
    }
}
fn load(state: &Path, journal: &Journal) -> Result<Manifest> {
    let bytes =
        private::read(&journal_dir(state, &journal.id).join("plan.json"), 2 * 1024 * 1024).map_err(Fault::io)?;
    if hash(&bytes) != journal.manifest_sha256 {
        return Err(Fault::conflict());
    }
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
    if manifest.schema != 1
        || real_directory(Path::new(&manifest.plan.context.state_dir))? != state
        || manifest.files.len() != journal.entries.len()
        || aaa_config::install::plan_sha256(&manifest.plan) != journal.plan_sha256
        || hash(manifest.plan.context.target_account.as_bytes()) != account_hash()?
        || manifest.plan.choices.instance != journal.instance
        || manifest.plan.choices.role != Some(journal.role)
        || manifest.plan.context.platform != journal.platform
        || manifest.plan.context.binary_sha256 != journal.binary_sha256
        || journal.root.is_some() != manifest.root.is_some()
        || journal.phase == Phase::Configured
            && (journal.entries.iter().any(|e| e.state != Step::Applied)
                || journal.root.is_some_and(|step| step != Step::Applied))
        || manifest
            .files
            .iter()
            .zip(&journal.entries)
            .any(|(f, e)| f.kind != e.kind || f.before != e.before || f.after != e.after || f.mode & !0o777 != 0)
    {
        return Err(Fault::conflict());
    }
    for directory in &manifest.directories {
        if real_directory(directory)? != *directory {
            return Err(Fault::conflict());
        }
    }
    if let Some(root) = &manifest.root {
        root_volume(root)?;
        if aaa_agent::work_root::check(&root.path, root.mount.as_deref()).map_err(|_| Fault::conflict())? != root.path {
            return Err(Fault::conflict());
        }
    }
    for f in &manifest.files {
        if target(&f.path)? != f.path {
            return Err(Fault::conflict());
        }
    }
    Ok(manifest)
}

/// 감독자의 읽기 검증도 파일 엔진 정본에서 계획을 얻는다. 별도 승인/권한 입력은 만들지 않는다.
pub(super) fn installed_plan(state: &Path, journal: &Journal) -> Result<InstallPlan> {
    Ok(load(state, journal)?.plan)
}

pub(super) fn observe(state: &Path, journal: Journal) -> Result<InstallResult> {
    let manifest = load(state, &journal)?;
    if journal.phase == Phase::Configured
        && manifest.files.iter().any(|f| snapshot(&f.path).ok() != Some((Some(f.after.clone()), Some(f.mode))))
    {
        return Err(Fault::conflict());
    }
    if journal.phase == Phase::Configured {
        observe_root(state, &manifest)?;
    }
    if journal.phase == Phase::Applying {
        for (index, f) in manifest.files.iter().enumerate() {
            let entry = &journal.entries[index];
            let current = snapshot(&f.path)?;
            let before = current == (f.before.clone(), f.before_mode)
                && f.before_inode.as_ref() == if current.0.is_some() { identity(&f.path).ok() } else { None }.as_ref();
            let after = current == (Some(f.after.clone()), Some(f.mode))
                && entry.inode.as_ref().is_none_or(|ino| identity(&f.path).ok().as_ref() == Some(ino));
            if match entry.state {
                Step::Pending => !before,
                Step::Staged => !before && !after,
                Step::Applied => !after,
                Step::Reverted => true,
            } {
                return Err(Fault::conflict());
            }
        }
    }
    Ok(journal.result())
}
/// 파일 단계가 끝난 뒤 감독자가 같은 config/root/binary 자원을 잠근 채 이어 간다.
pub(super) fn journal_locks(state: &Path, journal: &Journal) -> Result<Locks> {
    let manifest = load(state, journal)?;
    locks(state, &manifest)
}
fn root_volume(root: &RootSpec) -> Result<()> {
    aaa_agent::work_root::check(&root.path, root.mount.as_deref()).map_err(|_| Fault::conflict())?;
    if super::super::platform::storage(&root.path).map(|s| s.0).as_ref() != Some(&root.volume) {
        return Err(Fault::conflict());
    }
    Ok(())
}
fn observe_root(state: &Path, manifest: &Manifest) -> Result<()> {
    let Some(root) = &manifest.root else { return Ok(()) };
    root_volume(root)?;
    let real = aaa_agent::work_root::check(&root.path, root.mount.as_deref()).map_err(|_| Fault::conflict())?;
    let saved = aaa_agent::work_root::inspect(&state.join("agent"))
        .map_err(|_| Fault::conflict())?
        .ok_or_else(Fault::conflict)?;
    if saved.applied.as_ref() != Some(&root.path)
        || saved.applied_mount != root.mount
        || !saved.roots.iter().any(|r| r.path == root.path && r.real_path == real && r.imported)
    {
        return Err(Fault::conflict());
    }
    let marker = root.path.join(".aaa-root-owner.json");
    if snapshot(&marker)?.0.is_none() || marker.metadata().map_err(Fault::io)?.len() > 4096 {
        return Err(Fault::conflict());
    }
    let marker: serde_json::Value =
        serde_json::from_slice(&std::fs::read(marker).map_err(Fault::io)?).map_err(|_| Fault::conflict())?;
    if marker.get("schema").and_then(serde_json::Value::as_u64) != Some(1)
        || marker.get("owner").and_then(serde_json::Value::as_str) != Some(&saved.owner)
    {
        return Err(Fault::conflict());
    }
    Ok(())
}

impl Transaction {
    fn create(request: Request) -> Result<Self> {
        let Request { plan, files, root, directories } = request;
        if plan.context.target_account != super::super::platform::account().ok_or_else(Fault::conflict)?
            || files.len() > 128
        {
            return Err(Fault::conflict());
        }
        let state = real_directory(Path::new(&plan.context.state_dir))?;
        let mut specs = Vec::new();
        let mut payloads = Vec::new();
        for f in files {
            let path = target(&f.path)?;
            if snapshot(&path)? != (f.before.clone(), f.before_mode) {
                return Err(Fault::conflict());
            }
            let mode = if cfg!(windows) { 0o600 } else { f.mode.unwrap_or_else(|| f.before_mode.unwrap_or(0o600)) };
            specs.push(FileSpec {
                path,
                kind: f.kind,
                before: f.before,
                after: hash(&f.bytes),
                before_mode: f.before_mode,
                before_inode: f.before_inode,
                force: f.force,
                mode,
                preserve: f.preserve,
                shared: f.shared,
                reference_before: None,
            });
            payloads.push(f.bytes);
        }
        let mut sorted: Vec<_> = specs.iter().map(|f| &f.path).collect();
        sorted.sort();
        sorted.dedup();
        if sorted.len() != specs.len() {
            return Err(Fault::conflict());
        }
        let mut manifest = Manifest { schema: 1, plan, files: specs, root, directories };
        let mut goal = manifest.plan.choices.clone();
        goal.mode = None;
        let goal_sha256 = hash(&json_bytes(&(
            goal,
            &manifest.plan.context.platform,
            &manifest.plan.context.target_account,
            &manifest.plan.context.binary_sha256,
            manifest.files.iter().map(|f| (&f.path, &f.after, f.mode)).collect::<Vec<_>>(),
        ))?);
        let active = pointer(&state.join("install/active.json"))?;
        if let Ok(bytes) = private::read(&state.join("install/receipt.json"), 1024)
            && let Ok(id) = serde_json::from_slice::<String>(&bytes)
            && id_ok(&id)
            && active.as_ref().is_none_or(|active| active == &id)
            && let Ok(journal) = read_journal(&state, &id, manifest.plan.choices.instance.as_deref())
            && journal.phase == Phase::Configured
            && !manifest.files.iter().any(|f| f.force)
            && journal.goal_sha256 == goal_sha256
            && observe(&state, journal.clone()).is_ok()
        {
            let old = load(&state, &journal)?;
            return Ok(Self {
                dir: journal_dir(&state, &id),
                state,
                manifest: old,
                journal,
                reused: true,
                _locks: Locks(Vec::new()),
            });
        }
        let held = locks(&state, &manifest)?;
        if let Some(id) = pointer(&state.join("install/active.json"))? {
            let previous = read_journal(&state, &id, manifest.plan.choices.instance.as_deref())?;
            if !matches!(previous.phase, Phase::Configured | Phase::RolledBack | Phase::Uninstalled)
                || previous.phase == Phase::Configured && manifest.plan.choices.mode == Some(InstallMode::Fresh)
            {
                return Err(Fault::conflict());
            }
        }
        for f in &manifest.files {
            if snapshot(&f.path)? != (f.before.clone(), f.before_mode)
                || f.before_inode.as_ref().is_some_and(|inode| identity(&f.path).ok().as_ref() != Some(inode))
            {
                return Err(Fault::conflict());
            }
        }
        references::preflight(&manifest)?;
        for f in &mut manifest.files {
            if f.shared {
                f.reference_before = references::read(f)?;
            }
        }
        private::parents(&state).map_err(Fault::io)?;
        private::directory(&state.join("install")).map_err(Fault::io)?;
        private::directory(&state.join("install/jobs")).map_err(Fault::io)?;
        let id = aaa_common::util::new_id();
        let dir = journal_dir(&state, &id);
        private::directory(&dir).map_err(Fault::io)?;
        for (i, bytes) in payloads.iter().enumerate() {
            private::write(&dir.join(format!("payload-{i}")), bytes).map_err(Fault::io)?;
        }
        let bytes = json_bytes(&manifest)?;
        private::write(&dir.join("plan.json"), &bytes).map_err(Fault::io)?;
        let journal = Journal {
            schema: 1,
            id,
            account: account_hash()?,
            instance: manifest.plan.choices.instance.clone(),
            role: manifest.plan.choices.role.ok_or_else(Fault::conflict)?,
            platform: manifest.plan.context.platform.clone(),
            version: aaa_config::parse_version(&manifest.plan.context.version)
                .map(|_| manifest.plan.context.version.clone()),
            binary_sha256: manifest.plan.context.binary_sha256.clone(),
            plan_sha256: aaa_config::install::plan_sha256(&manifest.plan),
            goal_sha256,
            manifest_sha256: hash(&bytes),
            phase: Phase::Applying,
            entries: manifest
                .files
                .iter()
                .map(|f| Entry {
                    kind: f.kind,
                    before: f.before.clone(),
                    after: f.after.clone(),
                    state: Step::Pending,
                    inode: None,
                    changed: f.force || f.before.as_ref() != Some(&f.after) || f.before_mode != Some(f.mode),
                })
                .collect(),
            root: manifest.root.as_ref().map(|_| Step::Pending),
            reverse_sha256: None,
            reverse: Vec::new(),
            rollback_remaining: Vec::new(),
        };
        let tx = Self { state, dir, manifest, journal, reused: false, _locks: held };
        tx.save()?;
        private::write(&tx.state.join("install/active.json"), &json_bytes(&tx.journal.id)?).map_err(Fault::io)?;
        Ok(tx)
    }
    fn resume(state: &Path, id: &str, instance: Option<&str>) -> Result<Self> {
        if !id_ok(id) {
            return Err(Fault::conflict());
        }
        let state = real_directory(state)?;
        let journal = read_journal(&state, id, instance)?;
        let manifest = load(&state, &journal)?;
        let held = locks(&state, &manifest)?;
        let current = read_journal(&state, id, instance)?;
        if json_bytes(&current)? != json_bytes(&journal)? {
            return Err(Fault::conflict());
        }
        match pointer(&state.join("install/active.json"))? {
            Some(active) if active != id => return Err(Fault::conflict()),
            None => private::write(&state.join("install/active.json"), &json_bytes(&id)?).map_err(Fault::io)?,
            _ => {}
        }
        Ok(Self { dir: journal_dir(&state, id), state, manifest, journal, reused: false, _locks: held })
    }
    fn save(&self) -> Result<()> {
        private::write(&self.dir.join("journal.json"), &json_bytes(&self.journal)?).map_err(Fault::io)
    }
    fn stage_path(&self, i: usize) -> PathBuf {
        self.manifest.files[i].path.parent().unwrap().join(format!(".aaa-install-{}-{i}.tmp", self.journal.id))
    }
    fn stage(&mut self, i: usize) -> Result<()> {
        let f = &self.manifest.files[i];
        if target(&f.path)? != f.path
            || snapshot(&f.path)? != (f.before.clone(), f.before_mode)
            || f.before_inode.as_ref().is_some_and(|inode| identity(&f.path).ok().as_ref() != Some(inode))
        {
            return Err(Fault::conflict());
        }
        if f.before.as_ref() == Some(&f.after) && f.before_mode == Some(f.mode) && !f.force {
            self.journal.entries[i].state = Step::Applied;
            return self.save();
        }
        if f.path.metadata().is_ok_and(|m| m.permissions().readonly()) {
            return Err(Fault::conflict());
        }
        if f.before.is_some() {
            // 백업은 일반 journal 옆의 private 파일이며 공개 결과에 내용·경로를 넣지 않는다.
            let bytes = std::fs::read(&f.path).map_err(Fault::io)?;
            if Some(hash(&bytes)) != f.before {
                return Err(Fault::conflict());
            }
            private::write(&self.dir.join(format!("backup-{i}")), &bytes).map_err(Fault::io)?;
        }
        let bytes = private::read(&self.dir.join(format!("payload-{i}")), 1024 * 1024 * 1024).map_err(Fault::io)?;
        if hash(&bytes) != f.after {
            return Err(Fault::conflict());
        }
        private::parents(f.path.parent().unwrap()).map_err(Fault::io)?;
        let staged = self.stage_path(i);
        match private::new_file(&staged) {
            Ok(mut file) => {
                file.write_all(&bytes).map_err(Fault::io)?;
                file.sync_all().map_err(Fault::io)?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if snapshot(&staged)?.0 != Some(f.after.clone()) {
                    return Err(Fault::conflict());
                }
            }
            Err(e) => return Err(Fault::io(e)),
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(f.mode)).map_err(Fault::io)?;
        }
        private::sync_file(&staged).map_err(Fault::io)?;
        private::sync_parent(&staged).map_err(Fault::io)?;
        self.journal.entries[i].inode = Some(identity(&staged)?);
        self.journal.entries[i].state = Step::Staged;
        self.save()
    }
    fn commit(&mut self, i: usize) -> Result<()> {
        let f = &self.manifest.files[i];
        if target(&f.path)? != f.path
            || snapshot(&f.path)? != (f.before.clone(), f.before_mode)
            || f.before_inode.as_ref().is_some_and(|inode| identity(&f.path).ok().as_ref() != Some(inode))
        {
            return Err(Fault::conflict());
        }
        if snapshot(&self.stage_path(i))? != (Some(f.after.clone()), Some(f.mode)) {
            return Err(Fault::conflict());
        }
        replace(&self.stage_path(i), &f.path, f.before.is_some())?;
        private::sync_parent(&f.path).map_err(Fault::io)?;
        self.journal.entries[i].state = Step::Applied;
        self.save()
    }
    fn advance(&mut self) -> Result<bool> {
        if self.journal.phase == Phase::Configured {
            observe(&self.state, self.journal.clone())?;
            self.receipt()?;
            return Ok(false);
        }
        if self.journal.phase != Phase::Applying {
            return Err(Fault::conflict());
        }
        if let Some(root) = &self.manifest.root {
            root_volume(root)?;
        }
        for directory in &self.manifest.directories {
            if real_directory(directory)? != *directory {
                return Err(Fault::conflict());
            }
            private::parents(directory).map_err(Fault::io)?;
        }
        for i in 0..self.manifest.files.len() {
            let f = &self.manifest.files[i];
            let current = snapshot(&f.path)?;
            let forced_pending = f.force && self.journal.entries[i].state == Step::Pending;
            let forced_staged_before = f.force
                && self.journal.entries[i].state == Step::Staged
                && self.journal.entries[i].inode.as_ref() != identity(&f.path).ok().as_ref();
            if current == (Some(f.after.clone()), Some(f.mode)) && !forced_pending && !forced_staged_before {
                if self.journal.entries[i].state == Step::Pending
                    && (f.before.as_ref() != Some(&f.after) || f.before_mode != Some(f.mode))
                {
                    return Err(Fault::conflict());
                }
                if self.journal.entries[i].state == Step::Staged {
                    if self.journal.entries[i].inode.as_ref() != Some(&identity(&f.path)?) {
                        return Err(Fault::conflict());
                    }
                    let staged = self.stage_path(i);
                    if staged.exists() {
                        if identity(&staged)? != identity(&f.path)? {
                            return Err(Fault::conflict());
                        }
                        std::fs::remove_file(staged).map_err(Fault::io)?;
                    }
                }
                if self.journal.entries[i].state != Step::Applied {
                    self.journal.entries[i].state = Step::Applied;
                    self.save()?;
                }
                continue;
            }
            match self.journal.entries[i].state {
                Step::Pending => self.stage(i)?,
                Step::Staged => self.commit(i)?,
                _ => return Err(Fault::conflict()),
            }
            return Ok(true);
        }
        if self.journal.root == Some(Step::Pending) {
            let root = self.manifest.root.as_ref().ok_or_else(Fault::conflict)?;
            let agent = self.state.join("agent");
            private::directory(&agent).map_err(Fault::io)?;
            aaa_agent::work_root::activate(&agent, &root.path, root.mount.as_deref()).map_err(|_| Fault::conflict())?;
            self.journal.root = Some(Step::Applied);
            self.save()?;
            return Ok(true);
        }
        let mut observed = self.journal.clone();
        observed.phase = Phase::Configured;
        observe(&self.state, observed)?;
        references::register(self)?;
        self.journal.phase = Phase::Configured;
        self.save()?;
        self.receipt()?;
        Ok(false)
    }
    fn receipt(&self) -> Result<()> {
        let path = self.state.join("install/receipt.json");
        let bytes = json_bytes(&self.journal.id)?;
        if private::read(&path, 1024).ok().as_ref() != Some(&bytes) {
            private::write(&path, &bytes).map_err(Fault::io)?;
        }
        Ok(())
    }
    fn run(&mut self) -> Result<InstallResult> {
        if matches!(self.journal.phase, Phase::RollingBack | Phase::Uninstalling) {
            while self.reverse_advance()? {}
            return Ok(self.journal.result());
        }
        if self.reused {
            let mut result = self.journal.result();
            result.status = InstallStatus::Unchanged;
            for c in &mut result.changes {
                c.state = InstallChangeState::Unchanged;
            }
            return Ok(result);
        }
        while self.advance()? {}
        Ok(self.journal.result())
    }
}

fn replace(staged: &Path, target: &Path, existing: bool) -> Result<()> {
    #[cfg(unix)]
    {
        if existing {
            std::fs::rename(staged, target).map_err(Fault::io)?;
        } else {
            std::fs::hard_link(staged, target).map_err(Fault::io)?;
            std::fs::remove_file(staged).map_err(Fault::io)?;
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW};
        let from: Vec<_> = staged.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<_> = target.as_os_str().encode_wide().chain(Some(0)).collect();
        // SAFETY: 기존 ACL 병합 실패를 무시하지 않고, 새 파일에는 REPLACE_EXISTING을 쓰지 않는다.
        let ok = unsafe {
            if existing {
                ReplaceFileW(to.as_ptr(), from.as_ptr(), std::ptr::null(), 0, std::ptr::null(), std::ptr::null())
            } else {
                MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH)
            }
        };
        if ok == 0 {
            return Err(Fault::io(std::io::Error::last_os_error()));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests;
