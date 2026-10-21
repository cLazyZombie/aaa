//! 시험 소유 자식의 종료 구독. callback의 kill/wait가 먼저 reap해도 사건을 보존한다.
pub struct ChildExit {
    #[cfg(unix)]
    fd: Option<std::os::fd::OwnedFd>,
    #[cfg(target_os = "macos")]
    pid: u32,
    #[cfg(windows)]
    handle: std::os::windows::io::OwnedHandle,
}
impl ChildExit {
    pub fn new(child: &mut dyn portable_pty::Child) -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::fd::FromRawFd;
            let pid = child.process_id().unwrap();
            if child.try_wait()?.is_some() {
                return Ok(Self {
                    fd: None,
                    #[cfg(target_os = "macos")]
                    pid,
                });
            }
            #[cfg(target_os = "macos")]
            let fd = {
                use std::os::fd::AsRawFd;
                let raw = unsafe { libc::kqueue() };
                if raw < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) };
                let change = libc::kevent {
                    ident: pid as usize,
                    filter: libc::EVFILT_PROC,
                    flags: libc::EV_ADD | libc::EV_ENABLE,
                    fflags: libc::NOTE_EXIT,
                    data: 0,
                    udata: std::ptr::null_mut(),
                };
                if unsafe { libc::kevent(fd.as_raw_fd(), &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(libc::ESRCH) && child.try_wait()?.is_some() {
                        return Ok(Self { fd: None, pid });
                    }
                    return Err(error);
                }
                fd
            };
            #[cfg(not(target_os = "macos"))]
            let fd = {
                let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
                if raw < 0 {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() == Some(libc::ESRCH) && child.try_wait()?.is_some() {
                        return Ok(Self { fd: None });
                    }
                    return Err(error);
                }
                unsafe { std::os::fd::OwnedFd::from_raw_fd(raw) }
            };
            // callback의 kill()도 내부에서 reap할 수 있다. 그 전에 OS 종료 구독을 고정한다.
            Ok(Self {
                fd: Some(fd),
                #[cfg(target_os = "macos")]
                pid,
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::BorrowedHandle;
            // 원 child handle은 worker 수명 owner의 callback도 계속 사용한다.
            let handle = unsafe { BorrowedHandle::borrow_raw(child.as_raw_handle().unwrap()) }.try_clone_to_owned()?;
            Ok(Self { handle })
        }
    }
    pub fn wait(self) -> std::io::Result<()> {
        #[cfg(unix)]
        let result = loop {
            use std::os::fd::AsRawFd;
            let Some(fd) = &self.fd else { break Ok(()) };
            #[cfg(target_os = "macos")]
            let ready = {
                let mut event: libc::kevent = unsafe { std::mem::zeroed() };
                let count =
                    unsafe { libc::kevent(fd.as_raw_fd(), std::ptr::null(), 0, &mut event, 1, std::ptr::null()) };
                if count == 1 {
                    if event.flags & libc::EV_ERROR != 0 && event.data != 0 {
                        break Err(std::io::Error::from_raw_os_error(event.data as i32));
                    }
                    event.ident == self.pid as usize && event.fflags & libc::NOTE_EXIT != 0
                } else {
                    false
                }
            };
            #[cfg(not(target_os = "macos"))]
            let ready = {
                let mut event = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
                let count = unsafe { libc::poll(&mut event, 1, -1) };
                if count == 1 && event.revents & (libc::POLLERR | libc::POLLNVAL) != 0 {
                    break Err(std::io::Error::other("소유한 pidfd 종료 구독 오류"));
                }
                count == 1 && event.revents & libc::POLLIN != 0
            };
            if ready {
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
            if unsafe { WaitForSingleObject(self.handle.as_raw_handle(), INFINITE) } == WAIT_OBJECT_0 {
                Ok(())
            } else {
                Err(std::io::Error::last_os_error())
            }
        };
        result
    }
}
