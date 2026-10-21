//! PTY EOF와 독립된 OS 자식 종료 사건. main driver가 wait하여 상태를 수집한다.
use super::{TerminalEvent, mpsc};

pub(super) fn watch(child: &dyn portable_pty::Child, tx: mpsc::Sender<TerminalEvent>) -> std::thread::JoinHandle<()> {
    #[cfg(unix)]
    let pid = child.process_id().unwrap();
    #[cfg(windows)]
    let handle = {
        use std::os::windows::io::BorrowedHandle;
        // 원 child handle은 worker 수명 owner의 callback도 계속 사용한다.
        unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle().unwrap()) }.try_clone_to_owned().unwrap()
    };
    std::thread::spawn(move || {
        #[cfg(unix)]
        let result = loop {
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            // 종료를 알리되 reaping은 driver의 Child::wait에 남긴다.
            if unsafe { libc::waitid(libc::P_PID, pid, info.as_mut_ptr(), libc::WEXITED | libc::WNOWAIT) } == 0 {
                break Ok(());
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                break Err(error);
            }
        };
        #[cfg(windows)]
        let result = {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::{
                Foundation::WAIT_OBJECT_0,
                System::Threading::{INFINITE, WaitForSingleObject},
            };
            if unsafe { WaitForSingleObject(handle.as_raw_handle(), INFINITE) } == WAIT_OBJECT_0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        };
        let _ = tx.send(TerminalEvent::ChildExited(result));
    })
}
