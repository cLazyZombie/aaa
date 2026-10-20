//! I5: 고정 사용자 SCM 작업과 내구 영수증. I6 전에는 쓰기 CLI가 이 모듈에 진입하지 않는다.
use super::*;
use std::collections::BTreeMap;

#[cfg(windows)]
mod native;
#[cfg(any(windows, test))]
mod runner;
#[cfg(windows)]
mod runtime;

fn failure(code: &'static str, exit: u8) -> Fault {
    Fault { code, exit }
}

/// 암호는 직렬화/Debug/Clone하지 않고 소유 UTF-16 buffer를 소비 뒤 지운다.
struct Password(Vec<u16>);
impl Password {
    fn new(mut input: Vec<u16>) -> Result<Self> {
        if input.is_empty() || input.len() > 1024 || input.contains(&0) {
            wipe(&mut input);
            return Err(failure("service_account_auth_unavailable", 3));
        }
        input.push(0);
        Ok(Self(input))
    }
}
fn wipe(input: &mut [u16]) {
    for item in input {
        // SAFETY: 소유 buffer의 유효한 원소다. 지우기를 최적화로 없애지 않는다.
        unsafe { std::ptr::write_volatile(item, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}
impl Drop for Password {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

#[derive(Default)]
struct LogonRights {
    allowed: bool,
}
impl LogonRights {
    fn observe(&mut self, rights: &[String], allow_eligible: bool) -> Result<()> {
        if rights.iter().any(|r| r == "SeDenyServiceLogonRight") {
            return Err(failure("service_logon_denied", 3));
        }
        self.allowed |= allow_eligible && rights.iter().any(|r| r == "SeServiceLogonRight");
        Ok(())
    }
    fn grant_required(&self) -> bool {
        !self.allowed
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Spec {
    schema: u32,
    id: String,
    role: InstallRole,
    instance: Option<String>,
    account: String,
    profile: PathBuf,
    executable: PathBuf,
    binary_sha256: String,
    config: PathBuf,
    state: PathBuf,
    environment: BTreeMap<String, String>,
    automatic: bool,
}
impl Spec {
    fn role(&self) -> &'static str {
        match self.role {
            InstallRole::Host => "host",
            InstallRole::Agent => "agent",
        }
    }
    #[cfg(windows)]
    fn name(&self) -> String {
        self.instance
            .as_ref()
            .map_or_else(|| format!("aaa-{}", self.role()), |name| format!("aaa-{name}-{}", self.role()))
    }
    fn directory(&self) -> PathBuf {
        journal_dir(&self.state, &self.id).join("services").join(self.role())
    }
    fn context(&self) -> PathBuf {
        self.directory().join("context.json")
    }
    fn sha(&self) -> String {
        hash(&serde_json::to_vec(self).expect("고정 service DTO"))
    }
    fn command(&self) -> String {
        [
            self.executable.to_string_lossy().into_owned(),
            "service-run".into(),
            "--context".into(),
            self.context().to_string_lossy().into_owned(),
            "--context-sha256".into(),
            self.sha(),
        ]
        .iter()
        .map(|s| quote(s))
        .collect::<Vec<_>>()
        .join(" ")
    }
    fn validate(&self) -> Result<()> {
        if self.schema != 1
            || !id_ok(&self.id)
            || self.account.is_empty()
            || !digest_ok(&self.binary_sha256)
            || self.instance.as_deref().is_some_and(|s| aaa_config::paths::check_instance(s).is_err())
            || [&self.profile, &self.executable, &self.config, &self.state].iter().any(|p| {
                !p.is_absolute()
                    || p.components().any(|c| c == std::path::Component::ParentDir)
                    || p.to_str().is_none_or(|s| s.contains('\0'))
            })
            || self.environment.iter().any(|(key, value)| {
                !matches!(
                    key.as_str(),
                    "PATH" | "LANG" | "LC_ALL" | "LC_CTYPE" | "TZ" | "TERM" | "CODEX_HOME" | "CLAUDE_CONFIG_DIR"
                ) || value.contains('\0')
                    || value.len() > 32768
            })
        {
            return Err(Fault::conflict());
        }
        Ok(())
    }
    fn verify_plan(&self, plan: &InstallPlan) -> Result<()> {
        if plan.context.target_account != self.account
            || plan.context.binary_sha256 != self.binary_sha256
            || transaction::real_directory(Path::new(&plan.context.bin_path).parent().ok_or_else(Fault::conflict)?)?
                .join(Path::new(&plan.context.bin_path).file_name().ok_or_else(Fault::conflict)?)
                != self.executable
            || Path::new(&plan.context.config_dir).canonicalize().map_err(Fault::io)? != self.config
            || Path::new(&plan.context.state_dir).canonicalize().map_err(Fault::io)? != self.state
            || plan.choices.instance != self.instance
            || (plan.choices.role != Some(self.role)
                && !(plan.choices.role == Some(InstallRole::Host)
                    && self.role == InstallRole::Agent
                    && plan.choices.local_agent == Some(InstallToggle::Enabled)))
        {
            return Err(Fault::conflict());
        }
        Ok(())
    }
    #[cfg(windows)]
    fn verify_files(&self) -> Result<()> {
        let journal = read_journal(&self.state, &self.id, self.instance.as_deref())?;
        if journal.phase != Phase::Configured {
            return Err(Fault::conflict());
        }
        let active = private::read(&self.state.join("install/active.json"), 128 * 1024).map_err(Fault::io)?;
        if serde_json::from_slice::<String>(&active).map_err(|_| Fault::conflict())? != self.id {
            return Err(Fault::conflict());
        }
        let plan = transaction::installed_plan(&self.state, &journal)?;
        if plan.context.platform.os != InstallOs::Windows
            || plan.choices.service != Some(InstallService::WindowsScm)
            || plan.choices.service_account != Some(InstallAccount::CurrentUser)
        {
            return Err(Fault::conflict());
        }
        self.verify_plan(&plan)?;
        transaction::observe(&self.state, journal)?;
        Ok(())
    }
    fn guard_directories(&self) -> Result<()> {
        for path in [&self.profile, &self.config, &self.state, &self.directory()] {
            if transaction::real_directory(path)? != *path {
                return Err(Fault::conflict());
            }
        }
        Ok(())
    }
    fn guard_paths(&self) -> Result<()> {
        self.guard_directories()?;
        if self.executable.canonicalize().map_err(Fault::io)? != self.executable {
            return Err(Fault::conflict());
        }
        Ok(())
    }
}

/// CreateProcess/SCM argv 규칙. 셸을 거치지 않으며 executable도 항상 인용한다.
fn quote(value: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in value.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        if c == '"' {
            out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
        } else {
            out.extend(std::iter::repeat_n('\\', slashes));
        }
        slashes = 0;
        out.push(c);
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Configuration {
    account: String,
    command: String,
    automatic: bool,
    own_process: bool,
}
impl Configuration {
    fn desired(spec: &Spec) -> Self {
        Self { account: spec.account.clone(), command: spec.command(), automatic: spec.automatic, own_process: true }
    }
    fn sha(&self) -> String {
        hash(&serde_json::to_vec(self).expect("고정 SCM config"))
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Process {
    pid: u32,
    created: u64,
    elevated: bool,
}
struct Observation {
    configuration: Configuration,
    running: bool,
    pid: u32,
}

/// 외부 OS 관리자 경계만 대체할 수 있다. ready 판정/파일 기록은 여기서 구현하지 않는다.
trait Manager {
    fn query(&mut self, spec: &Spec) -> Result<Option<Observation>>;
    fn create(&mut self, spec: &Spec, password: &Password) -> Result<()>;
    fn start(&mut self, spec: &Spec) -> Result<Observation>;
    fn stop(&mut self, spec: &Spec) -> Result<()>;
    fn remove(&mut self, spec: &Spec) -> Result<()>;
    fn credentials(&mut self, spec: &Spec, password: &Password) -> Result<()>;
    fn process(&mut self, spec: &Spec, pid: u32) -> Result<Option<Process>>;
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ServicePhase {
    Pending,
    Registering,
    Registered,
    Starting,
    Ready,
    Removing,
    Removed,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    context_sha256: String,
    account_sha256: String,
    before: Option<String>,
    owned: bool,
    logon_right_may_remain: bool,
    credentials_pending: bool,
    credentials_restart_unverified: bool,
    credentials_process_before: Option<Process>,
    phase: ServicePhase,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    schema: u32,
    context_sha256: String,
    account_sha256: String,
    profile_sha256: String,
    binary_sha256: String,
    pid: u32,
    connected: bool,
    process_created: u64,
    session: u32,
    elevated: bool,
    boot: String,
}

fn credentials_verified(record: &Record, process: Process) -> bool {
    !record.credentials_restart_unverified
        || record
            .credentials_process_before
            .is_none_or(|before| (before.pid, before.created) != (process.pid, process.created))
}

struct Job {
    spec: Spec,
    record: Record,
    _lock: File,
    _files: Option<transaction::Locks>,
}
impl Job {
    /// I2 실제 파일 receipt와 같은 계획/계정/ID만 받아 서비스 단계로 이어 간다.
    fn prepare(
        plan: &InstallPlan,
        id: &str,
        role: InstallRole,
        profile: PathBuf,
        environment: BTreeMap<String, String>,
    ) -> Result<Self> {
        if plan.context.platform.os != InstallOs::Windows
            || plan.choices.service != Some(InstallService::WindowsScm)
            || plan.choices.service_account != Some(InstallAccount::CurrentUser)
            || plan.choices.role != Some(role)
                && !(plan.choices.role == Some(InstallRole::Host)
                    && role == InstallRole::Agent
                    && plan.choices.local_agent == Some(InstallToggle::Enabled))
        {
            return Err(Fault::conflict());
        }
        Self::from_configured_files(plan, id, role, profile, environment)
    }
    /// 파일 엔진과 OS adapter 사이의 공통 내구 경계. OS 선택 검사는 prepare가 소유한다.
    fn from_configured_files(
        plan: &InstallPlan,
        id: &str,
        role: InstallRole,
        profile: PathBuf,
        environment: BTreeMap<String, String>,
    ) -> Result<Self> {
        let state = Path::new(&plan.context.state_dir);
        let journal = read_journal(state, id, plan.choices.instance.as_deref())?;
        if journal.phase != Phase::Configured || journal.plan_sha256 != aaa_config::install::plan_sha256(plan) {
            return Err(Fault::conflict());
        }
        let files = transaction::journal_locks(state, &journal)?;
        let current = read_journal(state, id, plan.choices.instance.as_deref())?;
        if current.phase != Phase::Configured || current.plan_sha256 != journal.plan_sha256 {
            return Err(Fault::conflict());
        }
        transaction::observe(state, current)?;
        let spec = Spec {
            schema: 1,
            id: id.into(),
            role,
            instance: plan.choices.instance.clone(),
            account: plan.context.target_account.clone(),
            profile: profile.canonicalize().map_err(Fault::io)?,
            executable: Path::new(&plan.context.bin_path).canonicalize().map_err(Fault::io)?,
            binary_sha256: plan.context.binary_sha256.clone(),
            config: Path::new(&plan.context.config_dir).canonicalize().map_err(Fault::io)?,
            state: state.canonicalize().map_err(Fault::io)?,
            environment,
            automatic: plan.choices.startup == Some(InstallStartup::Auto),
        };
        let mut job = Self::open(spec)?;
        job._files = Some(files);
        Ok(job)
    }
    fn open(spec: Spec) -> Result<Self> {
        spec.validate()?;
        spec.guard_paths()?;
        if hash(spec.account.as_bytes()) != account_hash()? {
            return Err(Fault::conflict());
        }
        private::directory(&spec.state.join("install")).map_err(Fault::io)?;
        let lock =
            private::lock(&spec.state.join("install/supervisor.lock")).map_err(|_| failure("installation_busy", 4))?;
        private::directory(&spec.directory()).map_err(Fault::io)?;
        let path = spec.directory().join("journal.json");
        let record = if path.exists() {
            let bytes = private::read(&path, 128 * 1024).map_err(Fault::io)?;
            let record: Record = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
            if record.schema != 1
                || record.context_sha256 != spec.sha()
                || record.account_sha256 != account_hash()?
                || record.before.as_ref().is_some_and(|s| !digest_ok(s))
                || private::read(&spec.context(), 128 * 1024).map_err(Fault::io)?
                    != serde_json::to_vec(&spec).map_err(|_| Fault::conflict())?
            {
                return Err(Fault::conflict());
            }
            record
        } else {
            private::write(&spec.context(), &serde_json::to_vec(&spec).map_err(|_| Fault::conflict())?)
                .map_err(Fault::io)?;
            Record {
                schema: 1,
                context_sha256: spec.sha(),
                account_sha256: account_hash()?,
                before: None,
                owned: false,
                logon_right_may_remain: false,
                credentials_pending: false,
                credentials_restart_unverified: false,
                credentials_process_before: None,
                phase: ServicePhase::Pending,
            }
        };
        let job = Self { spec, record, _lock: lock, _files: None };
        job.save()?;
        Ok(job)
    }
    fn save(&self) -> Result<()> {
        private::write(
            &self.spec.directory().join("journal.json"),
            &serde_json::to_vec(&self.record).map_err(|_| Fault::conflict())?,
        )
        .map_err(Fault::io)
    }
    fn verify_configuration(&self, observed: &Observation) -> Result<()> {
        if observed.configuration != Configuration::desired(&self.spec) {
            return Err(Fault::conflict());
        }
        Ok(())
    }
    fn register(&mut self, manager: &mut impl Manager, password: Option<&Password>) -> Result<()> {
        if matches!(self.record.phase, ServicePhase::Removing | ServicePhase::Removed) {
            return Err(Fault::conflict());
        }
        let observed = manager.query(&self.spec)?;
        if self.record.phase == ServicePhase::Pending {
            if let Some(ref current) = observed {
                self.verify_configuration(current)?;
            }
            self.record.before = observed.as_ref().map(|s| s.configuration.sha());
            self.record.phase = ServicePhase::Registering;
            self.save()?;
        }
        if let Some(current) = observed {
            self.verify_configuration(&current)?;
        } else {
            if self.record.before.is_some() || self.record.phase != ServicePhase::Registering {
                return Err(Fault::conflict());
            }
            let password = password.ok_or_else(|| failure("service_account_auth_unavailable", 3))?;
            // LSA 직후 중단돼도 권한을 제거했다고 추정하지 않는다. 다른 서비스의 권한은 보존한다.
            self.record.logon_right_may_remain = true;
            self.save()?;
            manager.create(&self.spec, password)?;
            self.verify_configuration(&manager.query(&self.spec)?.ok_or_else(Fault::conflict)?)?;
        }
        self.record.owned = self.record.before.is_none();
        self.record.phase = ServicePhase::Registered;
        self.save()
    }
    fn start(&mut self, manager: &mut impl Manager) -> Result<()> {
        if !matches!(self.record.phase, ServicePhase::Registered | ServicePhase::Starting | ServicePhase::Ready) {
            return Err(Fault::conflict());
        }
        self.verify_configuration(&manager.query(&self.spec)?.ok_or_else(Fault::conflict)?)?;
        self.record.phase = ServicePhase::Starting;
        self.save()?;
        let observed = manager.start(&self.spec)?;
        self.verify_configuration(&observed)?;
        let process = verify_ready(&self.spec, manager, &observed)?;
        if !credentials_verified(&self.record, process) {
            return Err(failure("service_credentials_restart_unverified", 3));
        }
        self.record.credentials_restart_unverified = false;
        self.record.credentials_process_before = None;
        self.record.phase = ServicePhase::Ready;
        self.save()
    }
    fn restart(&mut self, manager: &mut impl Manager) -> Result<()> {
        if !matches!(self.record.phase, ServicePhase::Registered | ServicePhase::Starting | ServicePhase::Ready) {
            return Err(Fault::conflict());
        }
        self.verify_configuration(&manager.query(&self.spec)?.ok_or_else(Fault::conflict)?)?;
        self.record.phase = ServicePhase::Starting;
        self.save()?;
        manager.stop(&self.spec)?;
        self.start(manager)
    }
    fn remove(&mut self, manager: &mut impl Manager) -> Result<()> {
        if self.record.phase == ServicePhase::Removed {
            return Ok(());
        }
        if !self.record.owned {
            return Err(failure("service_not_owned", 4));
        }
        self.record.phase = ServicePhase::Removing;
        self.save()?;
        if let Some(observed) = manager.query(&self.spec)? {
            self.verify_configuration(&observed)?;
            manager.remove(&self.spec)?;
        }
        if manager.query(&self.spec)?.is_some() {
            return Err(failure("service_removal_pending", 3));
        }
        self.record.phase = ServicePhase::Removed;
        self.record.credentials_pending = false;
        self.record.credentials_restart_unverified = false;
        self.record.credentials_process_before = None;
        self.save()
    }
    fn credentials(&mut self, manager: &mut impl Manager, password: &Password) -> Result<()> {
        let observed = manager.query(&self.spec)?.ok_or_else(Fault::conflict)?;
        self.verify_configuration(&observed)?;
        if !self.record.owned || matches!(self.record.phase, ServicePhase::Removing | ServicePhase::Removed) {
            return Err(Fault::conflict());
        }
        // OS의 옛 암호를 읽어 백업하지 않는다. 중단/실패는 새 보호 입력으로 다시 확인해야 한다.
        let before = if observed.running {
            Some(manager.process(&self.spec, observed.pid)?.ok_or_else(Fault::conflict)?)
        } else {
            None
        };
        self.record.credentials_process_before = before;
        self.record.credentials_restart_unverified = true;
        self.record.credentials_pending = true;
        self.save()?;
        manager.credentials(&self.spec, password)?;
        self.verify_configuration(&manager.query(&self.spec)?.ok_or_else(Fault::conflict)?)?;
        self.record.credentials_pending = false;
        self.save()
    }
}
fn verify_ready(spec: &Spec, manager: &mut impl Manager, observed: &Observation) -> Result<Process> {
    let bytes = private::read(&spec.directory().join("ready.json"), 128 * 1024)
        .map_err(|_| failure("service_ready_unverified", 3))?;
    let ready: Ready = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
    if !observed.running
        || observed.pid == 0
        || !ready.connected
        || ready.schema != 1
        || ready.pid != observed.pid
        || ready.context_sha256 != spec.sha()
        || ready.account_sha256 != hash(spec.account.as_bytes())
        || ready.profile_sha256 != hash(spec.profile.to_string_lossy().as_bytes())
        || ready.binary_sha256 != spec.binary_sha256
        || ready.session != 0
        || ready.boot != aaa_common::util::boot_id()
        || manager.process(spec, ready.pid)?
            != Some(Process { pid: ready.pid, created: ready.process_created, elevated: ready.elevated })
    {
        return Err(failure("service_ready_unverified", 3));
    }
    Ok(Process { pid: ready.pid, created: ready.process_created, elevated: ready.elevated })
}

fn incomplete(result: &mut InstallResult, id: &str, code: &str) {
    result.status = InstallStatus::NeedsUser;
    result.exit_code = 3;
    result.error = None;
    result.needs_user.push(InstallNeedsUser {
        code: code.into(),
        target: "service".into(),
        message: "서비스의 실제 상태와 필요한 승인/인증을 확인하세요".into(),
    });
    result.resume = Some(InstallResume { installation_id: id.into() });
}
fn record_status(spec: &Spec, record: &Record, check: InstallCheckState, result: &mut InstallResult) {
    result.checks.push(InstallCheck { name: format!("service_{}_ready", spec.role()), state: check });
    result.changes.push(InstallChange {
        kind: "service".into(),
        action: "register".into(),
        state: match record.phase {
            ServicePhase::Removed => InstallChangeState::Reverted,
            ServicePhase::Ready if check == InstallCheckState::Passed => InstallChangeState::Applied,
            _ => InstallChangeState::Pending,
        },
    });
    if !matches!(record.phase, ServicePhase::Ready | ServicePhase::Removed)
        || (record.phase == ServicePhase::Ready && check != InstallCheckState::Passed)
        || record.credentials_pending
    {
        incomplete(result, &spec.id, "service_setup_incomplete");
    }
}
fn observe_service(spec: &Spec, record: &Record, manager: &mut impl Manager, result: &mut InstallResult) -> Result<()> {
    if record.phase == ServicePhase::Removed {
        record_status(spec, record, InstallCheckState::NotRequested, result);
        return Ok(());
    }
    let mut ready = InstallCheckState::NotRun;
    let observation = manager.query(spec);
    match observation {
        Ok(Some(observed)) => {
            if observed.configuration != Configuration::desired(spec) {
                return Err(Fault::conflict());
            }
            result.checks.push(InstallCheck {
                name: format!("service_{}_registered", spec.role()),
                state: InstallCheckState::Passed,
            });
            result.checks.push(InstallCheck {
                name: format!("service_{}_running", spec.role()),
                state: if observed.running { InstallCheckState::Passed } else { InstallCheckState::Failed },
            });
            if record.phase == ServicePhase::Ready {
                match verify_ready(spec, manager, &observed) {
                    Ok(process) => {
                        ready = InstallCheckState::Passed;
                        result.checks.push(InstallCheck {
                            name: format!(
                                "service_{}_token_{}",
                                spec.role(),
                                if process.elevated { "elevated" } else { "standard" }
                            ),
                            state: InstallCheckState::Passed,
                        });
                        if !credentials_verified(record, process) {
                            incomplete(result, &spec.id, "service_credentials_restart_unverified");
                        }
                    }
                    Err(e) if e.exit == 3 => {
                        ready = InstallCheckState::Failed;
                        incomplete(result, &spec.id, e.code);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(None) => incomplete(result, &spec.id, "service_registration_missing"),
        Err(e) if e.exit == 3 => {
            ready = InstallCheckState::Unknown;
            incomplete(result, &spec.id, e.code);
        }
        Err(e) => return Err(e),
    }
    record_status(spec, record, ready, result);
    Ok(())
}
/// status는 실제 기록/SCM을 읽기만 한다. 잠금·probe·보정/기록 쓰기를 하지 않는다.
pub(super) fn observe(
    state: &Path,
    instance: Option<&str>,
    id: &str,
    mut result: InstallResult,
) -> Result<InstallResult> {
    let journal = read_journal(state, id, instance)?;
    let plan = transaction::installed_plan(state, &journal)?;
    let mut seen = Vec::new();
    for role in ["host", "agent"] {
        let directory = journal_dir(state, id).join("services").join(role);
        let path = directory.join("journal.json");
        if !path.try_exists().map_err(Fault::io)? {
            continue;
        }
        let record: Record = serde_json::from_slice(&private::read(&path, 128 * 1024).map_err(Fault::io)?)
            .map_err(|_| Fault::conflict())?;
        let bytes = private::read(&directory.join("context.json"), 128 * 1024).map_err(Fault::io)?;
        let spec: Spec = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
        spec.validate()?;
        if record.phase == ServicePhase::Removed {
            spec.guard_directories()?;
        } else {
            spec.guard_paths()?;
        }
        spec.verify_plan(&plan)?;
        if record.schema != 1
            || record.account_sha256 != account_hash()?
            || record.context_sha256 != hash(&bytes)
            || hash(spec.account.as_bytes()) != account_hash()?
            || spec.state != transaction::real_directory(state)?
            || spec.id != id
            || spec.instance.as_deref() != instance
            || spec.role() != role
            || record.before.as_ref().is_some_and(|s| !digest_ok(s))
        {
            return Err(Fault::conflict());
        }
        seen.push(role);
        #[cfg(windows)]
        observe_service(&spec, &record, &mut native::Scm, &mut result)?;
        #[cfg(not(windows))]
        {
            if record.phase == ServicePhase::Ready {
                return Err(failure("unsupported_service_platform", 5));
            }
            record_status(&spec, &record, InstallCheckState::NotRun, &mut result);
        }
    }
    if journal.phase == Phase::Configured && plan.choices.service == Some(InstallService::WindowsScm) {
        for (role, wanted) in [
            ("host", plan.choices.role == Some(InstallRole::Host)),
            (
                "agent",
                plan.choices.role == Some(InstallRole::Agent)
                    || plan.choices.local_agent == Some(InstallToggle::Enabled),
            ),
        ] {
            if wanted && !seen.contains(&role) {
                incomplete(&mut result, id, "service_setup_incomplete");
            }
        }
    }
    Ok(result)
}

pub(super) fn dispatch(context: &Path, sha: &str) -> std::process::ExitCode {
    #[cfg(windows)]
    {
        runtime::dispatch(context, sha)
    }
    #[cfg(not(windows))]
    {
        let _ = (context, sha);
        eprintln!("unsupported_service_platform");
        std::process::ExitCode::from(5)
    }
}

#[cfg(test)]
mod tests;
