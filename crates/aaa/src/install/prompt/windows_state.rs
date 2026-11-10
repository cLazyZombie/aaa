//! 시험 부모의 요청이 왔을 때만 원래 stdin과 reader thread를 조회한다.
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use windows_sys::Win32::{Foundation::*, System::Threading::*};
#[path = "windows_state/wire.rs"]
pub(super) mod wire;
use wire::{Api, Outcome, Phase, Reply, Request, Sample};

struct ReadState {
    input: usize,
    thread: OwnedHandle,
    id: u64,
    tid: u32,
    phase: AtomicU8,
}
type Current = Arc<Mutex<Option<Arc<ReadState>>>>;
static CURRENT: OnceLock<Option<Current>> = OnceLock::new();

fn registry() -> Option<Current> {
    CURRENT
        .get_or_init(|| {
            if std::env::var_os("AAA_TEST_INSTALL_CONSOLE_TRACE").as_deref() != Some(std::ffi::OsStr::new("1")) {
                return None;
            }
            let socket = std::path::PathBuf::from(std::env::var_os("AAA_TEST_INSTALL_READ_STATE_SOCKET")?);
            let owner: u32 = std::env::var("AAA_TEST_INSTALL_READ_STATE_OWNER").ok()?.parse().ok()?;
            if owner == 0 || aaa_common::private_file::verify_directory(socket.parent()?).is_err() {
                return None;
            }
            let current: Current = Arc::new(Mutex::new(None));
            let worker_current = current.clone();
            match std::thread::Builder::new().name("console-read-state".into()).spawn(move || {
                let run = || -> std::io::Result<()> {
                    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
                    runtime.block_on(async {
                        let (mut stream, peer) = aaa_common::ipc::connect_peer(&socket).await?;
                        if peer != Some(owner) {
                            return Err(std::io::ErrorKind::PermissionDenied.into());
                        }
                        // 정상 입력 중에는 기다리기만 한다. 원래 stdin을 읽거나 대기 신호를 소비하지 않는다.
                        let request: Request = wire::read(&mut stream).await?;
                        let state = worker_current.lock().unwrap().clone();
                        let outcome = if request.format != 1 {
                            Outcome::InvalidRequest
                        } else if let Some(state) = state {
                            Outcome::OriginalStdin { sample: state.sample() }
                        } else {
                            Outcome::NoActiveRead
                        };
                        let reply = Reply {
                            format: 1,
                            cause: request.cause,
                            decided_unix_us: request.decided_unix_us,
                            outcome,
                        };
                        wire::write(&mut stream, &reply).await?;
                        tracing::debug!("install_console_read_state_replied");
                        Ok(())
                    })
                };
                if let Err(error) = run() {
                    tracing::debug!(os_code = error.raw_os_error(), "install_console_read_state_unavailable");
                }
            }) {
                Ok(_) => Some(current),
                Err(error) => {
                    tracing::debug!(os_code = error.raw_os_error(), "install_console_read_state_unavailable");
                    None
                }
            }
        })
        .clone()
}

pub(super) struct ReadGuard {
    current: Current,
    state: Arc<ReadState>,
    call_event: Option<OwnedHandle>,
}
impl ReadGuard {
    pub(super) fn register(input: HANDLE, id: u64) -> Option<Self> {
        let current = registry()?;
        let tid = unsafe { GetCurrentThreadId() };
        let thread = unsafe { OpenThread(THREAD_QUERY_INFORMATION, 0, tid) };
        if thread.is_null() {
            tracing::debug!(
                read_id = id,
                os_code = unsafe { GetLastError() },
                "install_console_read_state_unavailable"
            );
            return None;
        }
        // 읽기 thread의 실제 object를 붙들어 thread ID 재사용과 구분한다.
        let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
        let state = Arc::new(ReadState { input: input as usize, thread, id, tid, phase: AtomicU8::new(0) });
        *current.lock().unwrap() = Some(state.clone());
        // 통제 시험만 쓰는 경계다. 이름은 부모가 만든 임의의 event이며 private IPC 설정이 없으면 열지 않는다.
        let call_event = std::env::var_os("AAA_TEST_INSTALL_READ_CALL_EVENT").and_then(|name| {
            use std::os::windows::ffi::OsStrExt;
            let name: Vec<_> = name.encode_wide().chain(Some(0)).collect();
            let event = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
            (!event.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(event) })
        });
        Some(Self { current, state, call_event })
    }
    pub(super) fn call(&self) {
        self.state.phase.store(1, Ordering::Release);
        // 호출자의 마지막 로그 I/O가 반환한 뒤다. 성공 경로에서는 이 신호와 ReadConsoleW 사이에 I/O가 없다.
        if let Some(event) = &self.call_event {
            unsafe { SetEvent(event.as_raw_handle()) };
        }
    }
    pub(super) fn returned(&self) {
        self.state.phase.store(2, Ordering::Release);
    }
}
impl Drop for ReadGuard {
    fn drop(&mut self) {
        self.returned();
        let mut current = self.current.lock().unwrap();
        if current.as_ref().is_some_and(|state| Arc::ptr_eq(state, &self.state)) {
            *current = None;
        }
    }
}
impl ReadState {
    fn phase(&self) -> Phase {
        match self.phase.load(Ordering::Acquire) {
            0 => Phase::Prepared,
            1 => Phase::Call,
            _ => Phase::Returned,
        }
    }
    fn sample(&self) -> Sample {
        let phase_before = self.phase();
        let started = std::time::Instant::now();
        let sampled_unix_us =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_micros() as u64;
        // ReadConsoleW에 넘긴 바로 그 handle이다. CONIN$를 새로 열거나 DuplicateHandle하지 않는다.
        // stdin은 CLI 수명 동안 닫지 않는다. 조회 사이 반환 경주는 phase_after로 구분한다.
        let result = unsafe { WaitForSingleObject(self.input as HANDLE, 0) };
        let original_stdin_wait = if result == WAIT_FAILED {
            Api::Error { os_code: unsafe { GetLastError() } as i32 }
        } else {
            Api::Ok { value: result }
        };
        let mut pending = 0;
        let ok = unsafe { GetThreadIOPendingFlag(self.thread.as_raw_handle(), &mut pending) };
        let reader_io_pending = if ok == 0 {
            Api::Error { os_code: unsafe { GetLastError() } as i32 }
        } else {
            Api::Ok { value: pending != 0 }
        };
        Sample {
            process_id: std::process::id(),
            read_id: self.id,
            read_thread: self.tid,
            phase_before,
            phase_after: self.phase(),
            sampled_unix_us,
            query_elapsed_us: started.elapsed().as_micros() as u64,
            original_stdin_wait,
            reader_io_pending,
        }
    }
}
