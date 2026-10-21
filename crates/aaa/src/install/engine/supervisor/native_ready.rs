//! SCM_RUNNING 이후의 실제 backend ready 사건을 기다린다. start를 다시 부르지 않는다.
use super::*;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::{Foundation::*, Storage::FileSystem::*, System::Threading::*};
struct Changes(HANDLE);
impl Drop for Changes {
    fn drop(&mut self) {
        unsafe {
            FindCloseChangeNotification(self.0);
        }
    }
}
impl Changes {
    fn new(directory: &Path) -> Result<Self> {
        let path: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
        let handle = unsafe {
            FindFirstChangeNotificationW(
                path.as_ptr(),
                0,
                FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE,
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(failure("service_ready_unavailable", 3));
        }
        Ok(Self(handle))
    }
    fn wait(&mut self, duration: std::time::Duration) -> Result<()> {
        match unsafe { WaitForSingleObject(self.0, duration.as_millis().min(u128::from(u32::MAX - 1)) as u32) } {
            WAIT_OBJECT_0 => {
                if unsafe { FindNextChangeNotification(self.0) } == 0 {
                    return Err(failure("service_ready_unavailable", 3));
                }
                Ok(())
            }
            WAIT_TIMEOUT => Err(failure("service_ready_wait_expired", 3)),
            _ => Err(failure("service_ready_unavailable", 3)),
        }
    }
}
pub(super) struct Waiting {
    changes: Changes,
    deadline: std::time::Instant,
}
impl Waiting {
    pub(super) fn new(spec: &Spec) -> Result<Self> {
        Ok(Self {
            changes: Changes::new(&spec.directory())?,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(u64::from(spec.wait_timeout_s)),
        })
    }
    pub(super) fn ready(&mut self, spec: &Spec, manager: &mut impl Manager, observed: &Observation) -> Result<()> {
        loop {
            let path = spec.directory().join("ready.json");
            let pending = match path.symlink_metadata() {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
                Err(_) => return Err(failure("service_ready_unverified", 3)),
                Ok(_) => {
                    let bytes = private::read(&path, 128 * 1024).map_err(|_| failure("service_ready_unverified", 3))?;
                    let ready: Ready = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
                    // 잘못된 PID/context는 기다려 성공시키지 않는다. 정상 중간 connected=false만 pending이다.
                    if ready.schema != 1
                        || ready.pid != observed.pid
                        || ready.context_sha256 != spec.sha()
                        || ready.account_sha256 != hash(spec.account.as_bytes())
                        || ready.profile_sha256 != hash(spec.profile.to_string_lossy().as_bytes())
                        || ready.binary_sha256 != spec.binary_sha256
                        || ready.session != 0
                        || ready.boot != aaa_common::util::boot_id()
                    {
                        return Err(failure("service_ready_unverified", 3));
                    }
                    if ready.connected {
                        verify_ready(spec, manager, observed)?;
                        return Ok(());
                    }
                    true
                }
            };
            if pending {
                let remaining = self
                    .deadline
                    .checked_duration_since(std::time::Instant::now())
                    .ok_or_else(|| failure("service_ready_wait_expired", 3))?;
                self.changes.wait(remaining)?;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_ready_notifications_wait_for_actual_file_changes_and_rearm() {
        let path = std::env::temp_dir().join(format!("aaa-ready-watch-{}", aaa_common::util::new_id()));
        private::directory(&path).unwrap();
        let mut watch = Changes::new(&path).unwrap();
        assert_eq!(watch.wait(std::time::Duration::ZERO).unwrap_err().code, "service_ready_wait_expired");
        for value in [b"first".as_slice(), b"second".as_slice()] {
            private::write(&path.join("ready.json"), value).unwrap();
            watch.wait(std::time::Duration::from_secs(10)).unwrap();
            assert!(private::read(&path.join("ready.json"), 65536).unwrap() == value);
        }
        drop(watch);
        std::fs::remove_dir_all(path).unwrap();
    }
}
