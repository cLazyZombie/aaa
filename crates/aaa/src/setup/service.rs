//! `aaa setup-service`: `aaa setup`이 등록한 SCM 서비스의 입구. SCM 밖에서 실행하면 agent를 띄우지 않고 끝난다.
//!
//! 서비스 token의 사용자 환경(레지스트리 PATH 포함)을 만들어 codex·claude를 사람 셸과 같은 자리에서 찾는다.
//! 출력은 상태 폴더의 `agent-service.log`로 보내고, host 연결 상태를 `setup-service.json`에 써서 설치기가 확인한다.
use std::io::Write;
use std::os::windows::io::AsRawHandle;
use std::process::ExitCode;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use windows_sys::Win32::{
    Foundation::*,
    System::{Console::*, Services::*},
};

use super::flow::ServiceStatus;

/// 로그가 이보다 크면 시작할 때 `.1`로 돌린다
const LOG_ROTATE: u64 = 8 * 1024 * 1024;

struct Control {
    handle: usize,
    status: Mutex<SERVICE_STATUS>,
    stop: tokio::sync::watch::Sender<bool>,
}
static CONTROL: OnceLock<Control> = OnceLock::new();

impl Control {
    fn report(&self, state: u32, code: u32) {
        let Ok(mut status) = self.status.lock() else { return };
        status.dwCurrentState = state;
        status.dwControlsAccepted =
            if state == SERVICE_RUNNING { SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN } else { 0 };
        status.dwWin32ExitCode = if code == 0 { NO_ERROR } else { ERROR_SERVICE_SPECIFIC_ERROR };
        status.dwServiceSpecificExitCode = code;
        let pending = state == SERVICE_START_PENDING || state == SERVICE_STOP_PENDING;
        status.dwCheckPoint = if pending { status.dwCheckPoint + 1 } else { 0 };
        status.dwWaitHint = if pending { 60_000 } else { 0 };
        unsafe { SetServiceStatus(self.handle as SERVICE_STATUS_HANDLE, &*status) };
    }
}

unsafe extern "system" fn handler(control: u32, _: u32, _: *mut core::ffi::c_void, _: *mut core::ffi::c_void) -> u32 {
    let Some(state) = CONTROL.get() else { return ERROR_CALL_NOT_IMPLEMENTED };
    match control {
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            state.report(SERVICE_STOP_PENDING, 0);
            let _ = state.stop.send(true);
            NO_ERROR
        }
        SERVICE_CONTROL_INTERROGATE => NO_ERROR,
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}

unsafe extern "system" fn service_main(argc: u32, argv: *mut *mut u16) {
    let code = std::panic::catch_unwind(|| match serve(argc, argv) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("aaa setup-service: {e:#}");
            1
        }
    })
    .unwrap_or(1);
    if let Some(control) = CONTROL.get() {
        control.report(SERVICE_STOPPED, code);
    }
}

pub(crate) fn dispatch() -> ExitCode {
    let mut name: Vec<u16> = "aaa".encode_utf16().chain(Some(0)).collect();
    let table = [
        SERVICE_TABLE_ENTRYW { lpServiceName: name.as_mut_ptr(), lpServiceProc: Some(service_main) },
        SERVICE_TABLE_ENTRYW { lpServiceName: std::ptr::null_mut(), lpServiceProc: None },
    ];
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        eprintln!("aaa setup-service는 Windows 서비스 관리자가 실행한다(직접 실행하지 않는다). 설치는 `aaa setup`.");
        return ExitCode::from(2);
    }
    ExitCode::SUCCESS
}

fn serve(argc: u32, argv: *mut *mut u16) -> Result<u32> {
    anyhow::ensure!(argc > 0 && !argv.is_null(), "서비스 이름이 없다");
    let (stop, _) = tokio::sync::watch::channel(false);
    let handle = unsafe { RegisterServiceCtrlHandlerExW(*argv, Some(handler), std::ptr::null_mut()) };
    anyhow::ensure!(!handle.is_null(), "서비스 제어 handler를 등록하지 못했다");
    let control = CONTROL.get_or_init(|| Control {
        handle: handle as usize,
        status: Mutex::new(SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: SERVICE_START_PENDING,
            dwControlsAccepted: 0,
            dwWin32ExitCode: NO_ERROR,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: 60_000,
        }),
        stop,
    });
    control.report(SERVICE_START_PENDING, 0);

    // 명령줄(--instance·--config-dir·--state-dir)이 정한 자리는 사용자 환경으로 덮지 않는다
    let keep: Vec<(String, std::ffi::OsString)> = ["AAA_INSTANCE", "AAA_CONFIG_DIR", "AAA_STATE_DIR"]
        .iter()
        .filter_map(|k| std::env::var_os(k).map(|v| (k.to_string(), v)))
        .collect();
    let (environment, profile) = super::win::service_environment()?;
    // 이 서비스 프로세스의 환경만 바꾼다(사용자·시스템 환경 설정은 쓰지 않는다). 아직 다른 thread가 없다
    unsafe {
        for (key, _) in std::env::vars_os() {
            std::env::remove_var(key);
        }
        for (key, value) in &environment {
            std::env::set_var(key, value);
        }
        std::env::set_var("HOME", &profile);
        for (key, value) in &keep {
            std::env::set_var(key, value);
        }
    }
    std::env::set_current_dir(&profile).with_context(|| format!("{}로 이동", profile.display()))?;

    let state = aaa_config::paths::state_dir();
    std::fs::create_dir_all(&state).with_context(|| format!("{} 만들기", state.display()))?;
    let log_path = state.join("agent-service.log");
    if std::fs::metadata(&log_path).is_ok_and(|m| m.len() > LOG_ROTATE) {
        let _ = std::fs::rename(&log_path, state.join("agent-service.log.1"));
    }
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(&log_path)?;
    let _ = writeln!(log, "--- {} 서비스 시작 pid {}", aaa_common::util::software(), std::process::id());
    unsafe {
        SetStdHandle(STD_OUTPUT_HANDLE, log.as_raw_handle());
        SetStdHandle(STD_ERROR_HANDLE, log.as_raw_handle());
    }

    let status_path = state.join("setup-service.json");
    let write_status = |connected: bool| {
        let status = ServiceStatus { pid: std::process::id(), connected, changed_ms: aaa_common::util::now_ms() };
        if let Err(e) = aaa_common::util::write_json(&status_path, &status) {
            eprintln!("연결 상태 파일을 쓰지 못했다: {e}");
        }
    };
    let (ready, ready_rx) = tokio::sync::oneshot::channel();
    let (connection, mut connected) = tokio::sync::watch::channel(false);
    let (ended, mut done) = tokio::sync::oneshot::channel::<()>();
    let backend_stop = control.stop.subscribe();
    let backend = std::thread::Builder::new().name("aaa-setup-agent".into()).spawn(move || {
        let result =
            crate::cli::supervised_role(aaa_proto::install::InstallRole::Agent, ready, connection, backend_stop);
        let _ = ended.send(());
        result
    })?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let mut stop_rx = control.stop.subscribe();
    rt.block_on(async {
        let prepared = tokio::select! {
            biased;
            _ = stop_rx.wait_for(|s| *s) => false,
            _ = &mut done => false,
            r = tokio::time::timeout(Duration::from_secs(60), ready_rx) => matches!(r, Ok(Ok(()))),
        };
        if !prepared {
            if !*control.stop.borrow() {
                eprintln!("agent가 60초 안에 준비되지 않았다");
                let _ = control.stop.send(true);
            }
            return;
        }
        control.report(SERVICE_RUNNING, 0);
        write_status(false);
        loop {
            tokio::select! {
                biased;
                _ = stop_rx.wait_for(|s| *s) => break,
                _ = &mut done => break,
                changed = connected.changed() => {
                    if changed.is_err() { break }
                    let now = *connected.borrow_and_update();
                    write_status(now);
                }
            }
        }
    });
    let stopping = *control.stop.borrow();
    let _ = control.stop.send(true);
    let result = backend.join();
    let _ = std::fs::remove_file(&status_path);
    let code = match result {
        Ok(Ok(code)) if code == ExitCode::SUCCESS && stopping => 0,
        Ok(Ok(_)) => 1,
        Ok(Err(e)) => {
            eprintln!("agent 실패: {e:#}");
            1
        }
        Err(_) => 1,
    };
    let _ = writeln!(log, "--- 서비스 끝 code {code}");
    Ok(code)
}
