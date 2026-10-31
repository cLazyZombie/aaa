//! 시험에서만 켜는 읽기 전용 콘솔 관측. 키 값·문자·입력 원문을 기록하지 않는다.
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;
use std::time::Instant;
use windows_sys::Win32::{
    Foundation::*,
    System::{Console::*, Threading::*},
};

pub(super) struct Observation {
    done: Arc<OwnedHandle>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Observation {
    pub(super) fn start(input: HANDLE, read_id: u64) -> Option<Self> {
        if !tracing::enabled!(tracing::Level::DEBUG)
            || std::env::var_os("AAA_TEST_INSTALL_CONSOLE_TRACE").as_deref() != Some(std::ffi::OsStr::new("1"))
        {
            return None;
        }
        // SAFETY: 이름 없는 자기 프로세스의 수동 reset 사건이다.
        let done = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if done.is_null() {
            let os_code = std::io::Error::last_os_error().raw_os_error();
            tracing::debug!(read_id, os_code, "install_console_observer_unavailable");
            return None;
        }
        // SAFETY: 위 CreateEventW가 반환한 소유 handle이다.
        let done = Arc::new(unsafe { OwnedHandle::from_raw_handle(done) });
        let signal = done.clone();
        let input = input as usize;
        let start = Instant::now();
        let worker = std::thread::Builder::new().name("console-observer".into()).spawn(move || {
            // 입력 handle은 호출자의 ReadConsoleW와 Observation drop/join 동안 유효하다.
            let input = input as HANDLE;
            let handles = [signal.as_raw_handle(), input];
            let mut noted_other = false;
            tracing::debug!(read_id, "install_console_observer_ready");
            loop {
                // SAFETY: done은 이 thread가 소유하며 입력은 join 전까지 닫지 않는다.
                match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) } {
                    WAIT_OBJECT_0 => break,
                    value if value == WAIT_OBJECT_0 + 1 => {
                        let mut records = [INPUT_RECORD::default(); 128];
                        let mut count = 0;
                        // Peek는 소비하지 않는다. ReadConsoleW와 경주하므로 못 본 것은 도착하지 않았다는 증거가 아니다.
                        let ok =
                            unsafe { PeekConsoleInputW(input, records.as_mut_ptr(), records.len() as u32, &mut count) };
                        if ok == 0 {
                            let os_code = std::io::Error::last_os_error().raw_os_error();
                            tracing::debug!(read_id, os_code, "install_console_observer_failed");
                            break;
                        }
                        let key_down = records[..count as usize].iter().any(|record| {
                            record.EventType == KEY_EVENT as u16 && unsafe { record.Event.KeyEvent.bKeyDown } != 0
                        });
                        if key_down || (!noted_other && count > 0) {
                            tracing::debug!(
                                read_id,
                                observed_after_us = start.elapsed().as_micros(),
                                key_down,
                                "install_console_input_observed"
                            );
                            noted_other = true;
                        }
                        if key_down {
                            break;
                        }
                        // 소비하지 않은 비키 사건이 계속 signaled인 동안 바쁘게 돌지 않는다.
                        if unsafe { WaitForSingleObject(signal.as_raw_handle(), 5) } == WAIT_OBJECT_0 {
                            break;
                        }
                    }
                    _ => {
                        let os_code = std::io::Error::last_os_error().raw_os_error();
                        tracing::debug!(read_id, os_code, "install_console_observer_failed");
                        break;
                    }
                }
            }
        });
        match worker {
            Ok(worker) => Some(Self { done, worker: Some(worker) }),
            Err(error) => {
                tracing::debug!(read_id, os_code = error.raw_os_error(), "install_console_observer_unavailable");
                None
            }
        }
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        // SAFETY: 자기 사건을 깨우고 입력 handle이 유효할 때 worker를 끝낸다.
        unsafe {
            SetEvent(self.done.as_raw_handle());
        }
        if let Some(worker) = self.worker.take()
            && worker.join().is_err()
        {
            tracing::debug!("install_console_observer_join_failed");
        }
    }
}
