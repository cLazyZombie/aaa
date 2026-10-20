//! 같은 실행 파일의 SCM entry. SCM이 아닌 console 호출은 실패하며 backend를 시작하지 않는다.
use super::*;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::{
    Foundation::*,
    System::{Console::*, Services::*},
};

struct Invocation {
    context: PathBuf,
    sha: String,
}
static INVOCATION: OnceLock<Invocation> = OnceLock::new();
struct Control {
    handle: usize,
    status: Mutex<SERVICE_STATUS>,
    stop: tokio::sync::watch::Sender<bool>,
}
static CONTROL: OnceLock<Control> = OnceLock::new();
impl Control {
    fn state(&self, state: u32, code: u32) -> bool {
        let Ok(mut status) = self.status.lock() else { return false };
        if state == SERVICE_RUNNING && status.dwCurrentState == SERVICE_STOP_PENDING {
            return false;
        }
        status.dwCurrentState = state;
        status.dwControlsAccepted =
            if state == SERVICE_RUNNING { SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN } else { 0 };
        status.dwWin32ExitCode = if code == 0 { 0 } else { ERROR_SERVICE_SPECIFIC_ERROR };
        status.dwServiceSpecificExitCode = code;
        status.dwCheckPoint = if state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING { 1 } else { 0 };
        status.dwWaitHint = if status.dwCheckPoint != 0 { 30000 } else { 0 };
        // handle은 ServiceMain에서 등록해 이 process 수명 동안 유지한다.
        unsafe { SetServiceStatus(self.handle as SERVICE_STATUS_HANDLE, &*status) != 0 }
    }
}
unsafe extern "system" fn handler(control: u32, _: u32, _: *mut core::ffi::c_void, _: *mut core::ffi::c_void) -> u32 {
    let Some(state) = CONTROL.get() else { return ERROR_CALL_NOT_IMPLEMENTED };
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            state.state(SERVICE_STOP_PENDING, 0);
            let _ = state.stop.send(true);
            0
        }
        SERVICE_CONTROL_INTERROGATE => {
            if let Ok(status) = state.status.lock() {
                unsafe {
                    SetServiceStatus(state.handle as SERVICE_STATUS_HANDLE, &*status);
                }
            }
            0
        }
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}
unsafe extern "system" fn service_main(argc: u32, argv: *mut *mut u16) {
    let result = std::panic::catch_unwind(|| entry(argc, argv));
    if let Some(control) = CONTROL.get() {
        let code = match result {
            Ok(Ok(())) => 0,
            Ok(Err(fault)) => u32::from(fault.exit),
            Err(_) => 1,
        };
        control.state(SERVICE_STOPPED, code);
    }
}
fn read_spec(invocation: &Invocation) -> Result<Spec> {
    if !digest_ok(&invocation.sha) || !invocation.context.is_absolute() {
        return Err(Fault::conflict());
    }
    let bytes = private::read(&invocation.context, 128 * 1024).map_err(Fault::io)?;
    if hash(&bytes) != invocation.sha {
        return Err(Fault::conflict());
    }
    let spec: Spec = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
    spec.validate()?;
    spec.guard_paths()?;
    if spec.context() != invocation.context || spec.sha() != invocation.sha {
        return Err(Fault::conflict());
    }
    Ok(spec)
}
fn entry(argc: u32, argv: *mut *mut u16) -> Result<()> {
    let invocation = INVOCATION.get().ok_or_else(Fault::conflict)?;
    if argc == 0 || argv.is_null() {
        return Err(Fault::conflict());
    }
    let service_name = unsafe { *argv };
    if service_name.is_null() {
        return Err(Fault::conflict());
    }
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let handle = unsafe { RegisterServiceCtrlHandlerExW(service_name, Some(handler), std::ptr::null_mut()) };
    if handle.is_null() {
        return Err(failure("service_controller_unavailable", 1));
    }
    CONTROL
        .set(Control {
            handle: handle as usize,
            status: Mutex::new(SERVICE_STATUS {
                dwServiceType: SERVICE_WIN32_OWN_PROCESS,
                dwCurrentState: SERVICE_START_PENDING,
                dwControlsAccepted: 0,
                dwWin32ExitCode: 0,
                dwServiceSpecificExitCode: 0,
                dwCheckPoint: 1,
                dwWaitHint: 30000,
            }),
            stop,
        })
        .map_err(|_| Fault::conflict())?;
    let control = CONTROL.get().ok_or_else(Fault::conflict)?;
    if !control.state(SERVICE_START_PENDING, 0) {
        return Err(failure("service_status_failed", 1));
    }
    let spec = read_spec(invocation)?;
    let expected = native::wide(std::ffi::OsStr::new(&spec.name()));
    for (index, value) in expected.iter().enumerate() {
        if unsafe { *service_name.add(index) } != *value {
            return Err(Fault::conflict());
        }
    }
    let identity = native::identity(std::process::id())?;
    if identity.sid != spec.account
        || identity.session != 0
        || identity.profile.canonicalize().map_err(Fault::io)? != spec.profile.canonicalize().map_err(Fault::io)?
        || std::env::current_exe().map_err(Fault::io)?.canonicalize().map_err(Fault::io)?
            != spec.executable.canonicalize().map_err(Fault::io)?
        || hash(&std::fs::read(&spec.executable).map_err(Fault::io)?) != spec.binary_sha256
    {
        return Err(failure("service_execution_context_mismatch", 4));
    }
    // SCM은 셸 startup 환경을 로드하지 않는다. 계획에서 검토한 비밀 없는 환경/경로만 복원한다.
    unsafe {
        std::env::set_var("USERPROFILE", &spec.profile);
        std::env::set_var("HOME", &spec.profile);
        std::env::set_var("AAA_CONFIG_DIR", &spec.config);
        std::env::set_var("AAA_STATE_DIR", &spec.state);
        if let Some(instance) = &spec.instance {
            std::env::set_var("AAA_INSTANCE", instance);
        } else {
            std::env::remove_var("AAA_INSTANCE");
        }
        for (key, value) in &spec.environment {
            std::env::set_var(key, value);
        }
    }
    let log = service_log(&spec.directory().join("service.log"))?;
    use std::os::windows::io::AsRawHandle;
    if unsafe { SetStdHandle(STD_OUTPUT_HANDLE, log.as_raw_handle()) } == 0
        || unsafe { SetStdHandle(STD_ERROR_HANDLE, log.as_raw_handle()) } == 0
    {
        return Err(failure("service_log_unavailable", 1));
    }
    let (ready, receiver) = tokio::sync::oneshot::channel();
    let role = spec.role;
    let backend = std::thread::Builder::new()
        .name("aaa-service-role".into())
        .spawn(move || crate::cli::supervised_role(role, ready, stop_rx))
        .map_err(Fault::io)?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(Fault::io)?;
    let prepared = rt.block_on(async { tokio::time::timeout(std::time::Duration::from_secs(30), receiver).await });
    let prepared = matches!(prepared, Ok(Ok(())));
    let mut running = false;
    if prepared && !*control.stop.borrow() {
        let ready = Ready {
            schema: 1,
            context_sha256: spec.sha(),
            account_sha256: hash(spec.account.as_bytes()),
            profile_sha256: hash(spec.profile.to_string_lossy().as_bytes()),
            binary_sha256: spec.binary_sha256.clone(),
            pid: std::process::id(),
            session: identity.session,
            elevated: identity.elevated,
            boot: aaa_common::util::boot_id(),
        };
        running = private::write(
            &spec.directory().join("ready.json"),
            &serde_json::to_vec(&ready).map_err(|_| Fault::conflict())?,
        )
        .is_ok()
            && control.state(SERVICE_RUNNING, 0);
        if !running {
            let _ = control.stop.send(true);
        }
    } else {
        let _ = control.stop.send(true);
    }
    let result = backend.join();
    // 다음 process의 proof를 지우지 않도록 현재 context/PID를 확인한다.
    let path = spec.directory().join("ready.json");
    if private::read(&path, 128 * 1024)
        .ok()
        .and_then(|b| serde_json::from_slice::<Ready>(&b).ok())
        .is_some_and(|proof| proof.pid == std::process::id() && proof.context_sha256 == spec.sha())
    {
        std::fs::remove_file(&path).map_err(Fault::io)?;
        private::sync_parent(&path).map_err(Fault::io)?;
    }
    drop(log);
    if !prepared {
        return Err(failure("service_backend_not_ready", 3));
    }
    if !running {
        return Err(failure("service_status_failed", 1));
    }
    match result {
        Ok(Ok(code)) if code == std::process::ExitCode::SUCCESS => Ok(()),
        _ => Err(failure("service_backend_failed", 1)),
    }
}
fn service_log(path: &Path) -> Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    match private::new_file(path) {
        Ok(file) => Ok(file),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            let file = std::fs::OpenOptions::new()
                .read(true)
                .append(true)
                .custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)
                .map_err(Fault::io)?;
            private::verify(&file, false).map_err(Fault::io)?;
            Ok(file)
        }
        Err(e) => Err(Fault::io(e)),
    }
}
pub(super) fn dispatch(context: &Path, sha: &str) -> std::process::ExitCode {
    if !digest_ok(sha)
        || !context.is_absolute()
        || INVOCATION.set(Invocation { context: context.into(), sha: sha.into() }).is_err()
    {
        return std::process::ExitCode::from(4);
    }
    let mut name = native::wide(std::ffi::OsStr::new("aaa"));
    let entries = [
        SERVICE_TABLE_ENTRYW { lpServiceName: name.as_mut_ptr(), lpServiceProc: Some(service_main) },
        SERVICE_TABLE_ENTRYW { lpServiceName: std::ptr::null_mut(), lpServiceProc: None },
    ];
    // SCM 연결 실패를 console host/agent 실행으로 바꾸지 않는다.
    if unsafe { StartServiceCtrlDispatcherW(entries.as_ptr()) } == 0 {
        eprintln!("service_controller_required");
        return std::process::ExitCode::from(3);
    }
    std::process::ExitCode::SUCCESS
}
