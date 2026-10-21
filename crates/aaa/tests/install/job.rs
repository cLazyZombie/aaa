//! 시험 소유 Job만 구성한다. runner의 바깥 Job 제한은 바꾸거나 벗어나지 않는다.
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::System::{JobObjects::*, Threading::*};

pub struct Job(OwnedHandle);
impl Job {
    fn new(breakaway: bool) -> Self {
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        assert!(!handle.is_null(), "owned job create: {}", std::io::Error::last_os_error());
        let job = Self(unsafe { OwnedHandle::from_raw_handle(handle) });
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | if breakaway { JOB_OBJECT_LIMIT_BREAKAWAY_OK } else { 0 };
        assert_ne!(
            unsafe {
                SetInformationJobObject(
                    job.0.as_raw_handle(),
                    JobObjectExtendedLimitInformation,
                    (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                    std::mem::size_of_val(&limits) as u32,
                )
            },
            0,
            "owned job limits: {}",
            std::io::Error::last_os_error()
        );
        job
    }
    fn assign(&self, process: windows_sys::Win32::Foundation::HANDLE) {
        assert_ne!(
            unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) },
            0,
            "owned parent assign: {}",
            std::io::Error::last_os_error()
        );
    }
    pub fn contain(child: &dyn portable_pty::Child, breakaway: bool) -> Self {
        let job = Self::new(breakaway);
        job.assign(child.as_raw_handle().unwrap());
        job
    }
    pub fn terminate(self) {
        // Job close에는 비정상 종료 코드 계약이 없다. 실제 Job 종료에 코드를 명시한다.
        // worker는 이 Job에서 이미 벗어났어야 하며, 이어 handle도 닫힌다.
        assert_ne!(
            unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) },
            0,
            "owned job terminate: {}",
            std::io::Error::last_os_error()
        );
    }
}

pub fn prepare_runner() {
    static OWNED: std::sync::OnceLock<Job> = std::sync::OnceLock::new();
    OWNED.get_or_init(|| {
        // GitHub의 바깥 KILL_ON_JOB_CLOSE는 유지한다. 양성 사례의 직접 부모 Job만
        // 명시적으로 허용한다. Windows nested breakaway는 금지한 바깥 Job에 남는다.
        let job = Job::new(true);
        job.assign(unsafe { GetCurrentProcess() });
        job
    });
}
