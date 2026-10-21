//! 실제 부모 수명 소멸을 사용한다. Windows는 시험 소유 KILL_ON_JOB_CLOSE Job에 부모를 넣은 뒤 worker를 만든다.
use super::authorization::{redact_private, terminal_controlled};
use super::*;

#[cfg(windows)]
mod os {
    use windows_sys::Win32::{
        Foundation::*,
        System::{JobObjects::*, Threading::*},
    };
    pub struct Job(HANDLE);
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    impl Job {
        pub fn contain(child: &dyn portable_pty::Child, breakaway: bool) -> Self {
            let job = Self(unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) });
            assert!(!job.0.is_null(), "owned job create: {}", std::io::Error::last_os_error());
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            limits.BasicLimitInformation.LimitFlags =
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | if breakaway { JOB_OBJECT_LIMIT_BREAKAWAY_OK } else { 0 };
            assert_ne!(
                unsafe {
                    SetInformationJobObject(
                        job.0,
                        JobObjectExtendedLimitInformation,
                        (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                        std::mem::size_of_val(&limits) as u32,
                    )
                },
                0,
                "owned job limits: {}",
                std::io::Error::last_os_error()
            );
            assert_ne!(
                unsafe { AssignProcessToJobObject(job.0, child.as_raw_handle().unwrap()) },
                0,
                "owned parent assign: {}",
                std::io::Error::last_os_error()
            );
            job
        }
    }
    pub struct Worker(HANDLE);
    impl Drop for Worker {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    impl Worker {
        pub fn observe(pid: u32) -> Self {
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
            assert!(!process.is_null(), "owned worker observe: {}", std::io::Error::last_os_error());
            assert_eq!(
                unsafe { WaitForSingleObject(process, 0) },
                WAIT_TIMEOUT,
                "worker가 부모 종료 전에 실제 살아 있어야 한다"
            );
            Self(process)
        }
        pub fn exited(self) {
            assert_eq!(unsafe { WaitForSingleObject(self.0, 10000) }, WAIT_OBJECT_0, "worker 종료 사건이 와야 한다");
            let mut code = 0;
            assert_ne!(unsafe { GetExitCodeProcess(self.0, &mut code) }, 0);
            assert_eq!(code, 3, "부모 소멸 뒤 private 표시 소실을 저장하고 끝내야 한다");
        }
    }
}
#[cfg(unix)]
mod os {
    pub struct Worker {
        fd: i32,
        #[cfg(target_os = "macos")]
        pid: u32,
    }
    impl Drop for Worker {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.fd);
            }
        }
    }
    impl Worker {
        pub fn observe(pid: u32) -> Self {
            #[cfg(target_os = "macos")]
            {
                let fd = unsafe { libc::kqueue() };
                assert!(fd >= 0);
                let change = libc::kevent {
                    ident: pid as usize,
                    filter: libc::EVFILT_PROC,
                    flags: libc::EV_ADD | libc::EV_ENABLE,
                    fflags: libc::NOTE_EXIT,
                    data: 0,
                    udata: std::ptr::null_mut(),
                };
                assert_eq!(
                    unsafe { libc::kevent(fd, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) },
                    0,
                    "owned worker exit subscribe: {}",
                    std::io::Error::last_os_error()
                );
                Self { fd, pid }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) } as i32;
                assert!(fd >= 0, "owned worker pidfd: {}", std::io::Error::last_os_error());
                Self { fd }
            }
        }
        pub fn exited(self) {
            #[cfg(target_os = "macos")]
            {
                let mut event: libc::kevent = unsafe { std::mem::zeroed() };
                let limit = libc::timespec { tv_sec: 10, tv_nsec: 0 };
                assert_eq!(
                    unsafe { libc::kevent(self.fd, std::ptr::null(), 0, &mut event, 1, &limit) },
                    1,
                    "worker 종료 사건이 와야 한다"
                );
                let observed = event.ident;
                assert_eq!(observed, self.pid as usize);
                assert_ne!(event.fflags & libc::NOTE_EXIT, 0);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let mut event = libc::pollfd { fd: self.fd, events: libc::POLLIN, revents: 0 };
                assert_eq!(unsafe { libc::poll(&mut event, 1, 10000) }, 1, "worker 종료 사건이 와야 한다");
                assert_ne!(event.revents & libc::POLLIN, 0);
            }
        }
    }
}

#[test]
fn approved_worker_outlives_its_cli_and_records_the_same_id_after_private_display_disappears() {
    #[cfg(windows)]
    let cases = [true, false];
    #[cfg(not(windows))]
    let cases = [true];
    for allowed in cases {
        let lab = Lab::new();
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let host = rt.block_on(async {
            let mut cfg = aaa_config::HostConfig::default();
            cfg.notify.web_push = false;
            cfg.host.agent_listen = "127.0.0.1:0".into();
            let host = aaa_host::Host::open(cfg, lab.0.join("control-host")).unwrap();
            aaa_host::link::start(&host).await.unwrap();
            host.call(aaa_proto::DeviceRole::Local, "local:cli", aaa_proto::FrontRequest::EnrollOpen { minutes: 10 })
                .await
                .unwrap();
            host
        });
        let address = host.agent_listen.lock().unwrap().clone().unwrap();
        let mut args = lab.args();
        args.retain(|a| a != "--dry-run");
        for (key, value) in [("--host", address.as_str()), ("--enroll", "wait")] {
            let at = args.iter().position(|a| a == key).unwrap();
            args[at + 1] = value.into();
        }
        args.extend(["--prompt".into(), "plain".into()]);
        let steps = [
            ("설치 방식", "fresh\r"),
            ("남겨 둘 여유 GB", "20\r"),
            ("harness 로그인 검사", "require\r"),
            ("사용자 PATH", "preserve\r"),
            ("검토 후 승인", "review\r"),
            ("정확히 yes", "yes\r"),
        ];
        let mut assigned = false;
        let mut ended = false;
        let mut record_path = None;
        #[cfg(windows)]
        let mut job = None;
        let (code, text) = terminal_controlled(&lab, &args, &steps, false, |child, text| {
            if !assigned && text.contains("설치 방식") {
                #[cfg(windows)]
                {
                    job = Some(os::Job::contain(child, allowed));
                }
                assigned = true;
            }
            if allowed && !ended && text.contains("확인 코드:") {
                let entries = std::fs::read_dir(lab.0.join("state/install/executions"))
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .collect::<Vec<_>>();
                assert_eq!(entries.len(), 1);
                let path = entries[0].clone();
                let record: Value =
                    serde_json::from_slice(&aaa_common::private_file::read(&path, 128 * 1024).unwrap()).unwrap();
                assert!(record["last_exit"].is_null());
                let worker = os::Worker::observe(record["worker_pid"].as_u64().unwrap() as u32);
                #[cfg(windows)]
                {
                    drop(job.take());
                }
                #[cfg(unix)]
                {
                    child.kill().unwrap();
                }
                worker.exited();
                let finished: Value =
                    serde_json::from_slice(&aaa_common::private_file::read(&path, 128 * 1024).unwrap()).unwrap();
                assert_eq!(finished["id"], record["id"]);
                assert_eq!(finished["last_exit"], 3, "부모 소멸 뒤 worker 자신의 내구 쓰기가 있어야 한다");
                record_path = Some(path);
                ended = true;
            }
        });
        assert!(assigned, "worker 생성 전에 실제 부모 Job/종료 경계를 준비한다");
        if allowed {
            assert!(ended, "{}", redact_private(&text));
            assert_ne!(code, 0, "종료한 부모 CLI 결과: {}", redact_private(&text));
            let record: Value =
                serde_json::from_slice(&aaa_common::private_file::read(&record_path.unwrap(), 128 * 1024).unwrap())
                    .unwrap();
            let before = snapshot(&lab.0);
            let observed = json(&lab.cmd(&["install", "--status", record["id"].as_str().unwrap(), "--json"]));
            assert_eq!(observed["installation_id"], record["id"]);
            assert_eq!(observed["exit_code"], 3);
            assert_eq!(snapshot(&lab.0), before);
        } else {
            assert_eq!(code, 3, "breakaway 금지는 다른 flag 재시도로 우회하지 않는다: {}", redact_private(&text));
            assert!(!lab.0.join("state/install/executions").exists());
            assert!(!lab.0.join("config").exists());
            assert!(!lab.0.join("작업 root").exists());
        }
    }
}
