//! 불변 launcher는 계속 wait하고 worker만 정상 정리 후 exec한다. launcher·plist를 바꾸지 않는다.
use std::process::ExitCode;

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use aaa_common::{process, util};
    use anyhow::{Result, bail};
    use serde::{Deserialize, Serialize};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicBool, Ordering},
    };

    static REEXEC: AtomicBool = AtomicBool::new(false);

    #[derive(Serialize, Deserialize)]
    struct Ready {
        pid: u32,
        launcher: u32,
        created: u64,
        boot: String,
        nonce: String,
        #[serde(default)]
        approved_executable: Option<(PathBuf, String)>,
    }
    fn path(role: &str) -> PathBuf {
        aaa_config::paths::state_dir().join("service-restart").join(format!("{role}.json"))
    }
    fn created(pid: u32) -> Result<u64> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        // 같은 사용자 worker의 생성 시각. 갱신으로 옛 executable vnode가 없어져도 PID 신원을 확인한다.
        let size = std::mem::size_of_val(&info);
        let count = unsafe {
            libc::proc_pidinfo(
                pid as i32,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                size as i32,
            )
        };
        if count as usize != size || info.pbi_pid != pid || info.pbi_status == 5 {
            bail!("worker 생성 시각 조회 실패");
        }
        Ok(info.pbi_start_tvsec.saturating_mul(1_000_000).saturating_add(info.pbi_start_tvusec))
    }
    pub(crate) struct Registration {
        path: PathBuf,
        nonce: String,
    }
    impl Drop for Registration {
        fn drop(&mut self) {
            // exec 사이에도 지원 표지는 유지한다. 둘째 요청이 옛 서비스 전환으로 오인하면 launcher를 죽인다.
            if REEXEC.load(Ordering::SeqCst) {
                return;
            }
            if util::read_json::<Ready>(&self.path)
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok_and(|r| r.nonce == self.nonce)
            {
                aaa_common::diagnostic::ignored(
                    std::fs::remove_file(&self.path),
                    "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                );
            }
        }
    }
    pub(crate) fn register(role: &str, approved_executable: Option<(PathBuf, String)>) -> Result<Option<Registration>> {
        let pid = std::process::id();
        let Some(launcher) = process::parent(pid) else { return Ok(None) };
        if !process::image(launcher).is_some_and(|p| process::is_launcher(&p)) {
            return Ok(None);
        }
        let ready = Ready {
            pid,
            launcher,
            created: created(pid)?,
            boot: util::boot_id(),
            nonce: util::new_id(),
            approved_executable,
        };
        util::ensure_private_dir(path(role).parent().expect("role 경로"))?;
        util::write_json(&path(role), &ready)?;
        Ok(Some(Registration { path: path(role), nonce: ready.nonce }))
    }
    pub(super) fn supported(role: &str, launcher: u32) -> Result<bool> {
        let p = path(role);
        if !p.exists() {
            return Ok(false);
        }
        let ready: Ready = util::read_json(&p)?;
        // 재사용 PID·옛 boot·다른 instance·가짜 표지에 신호를 보내지 않는다.
        if ready.boot != util::boot_id()
            || ready.launcher != launcher
            || process::parent(ready.pid) != Some(launcher)
            || process::uid(ready.pid) != Some(unsafe { libc::geteuid() })
            || created(ready.pid)? != ready.created
            || !process::image(launcher).is_some_and(|p| process::is_launcher(&p))
        {
            bail!("재시작 준비 기록과 실제 프로세스가 다르다. 서비스를 확인한다");
        }
        if let Some((path, sha)) = ready.approved_executable
            && util::sha256_file(&path)? != sha
        {
            bail!("승인된 설치 파일과 다르다. 새 install 승인 뒤 재시작한다");
        }
        Ok(true)
    }
    pub(crate) fn request() {
        REEXEC.store(true, Ordering::SeqCst);
    }
    fn mask_restart(block: bool) -> std::io::Result<()> {
        // exec는 호출 thread의 mask를 보존한다. 새 image의 handler가 준비될 때까지 SIGUSR2를 보류한다.
        unsafe {
            let mut signals: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut signals);
            libc::sigaddset(&mut signals, libc::SIGUSR2);
            let code = libc::pthread_sigmask(
                if block { libc::SIG_BLOCK } else { libc::SIG_UNBLOCK },
                &signals,
                std::ptr::null_mut(),
            );
            if code == 0 { Ok(()) } else { Err(std::io::Error::from_raw_os_error(code)) }
        }
    }
    pub(crate) fn signals_ready() -> std::io::Result<()> {
        mask_restart(false)
    }
    pub(super) fn finish(code: ExitCode) -> ExitCode {
        if code != ExitCode::SUCCESS || !REEXEC.load(Ordering::SeqCst) {
            return code;
        }
        if let Err(error) = mask_restart(true) {
            eprintln!("aaa: worker 교체 신호 보류 실패: {error}");
            return ExitCode::FAILURE;
        }
        use std::os::unix::process::CommandExt;
        let error = match std::env::current_exe() {
            Ok(exe) => aaa_common::child_process::with_spawn_lock(|| {
                std::process::Command::new(exe).args(std::env::args_os().skip(1)).exec()
            }),
            Err(error) => error,
        };
        eprintln!("aaa: 새 worker 실행 실패: {error}");
        ExitCode::FAILURE
    }

    pub(super) async fn shutdown(role: &str) {
        use tokio::signal::unix::{SignalKind, signal};
        // 신호 구독이 준비된 뒤에만 지원 표지를 공개한다.
        let mut restart = signal(SignalKind::user_defined2()).expect("SIGUSR2");
        let registration = match register(role, None) {
            Ok(registration) => registration,
            Err(error) => {
                tracing::error!(%error, "worker 재시작 준비 실패");
                None
            }
        };
        if let Err(error) = signals_ready() {
            tracing::error!(%error, "재시작 신호 대기 해제 실패");
        }
        tokio::select! {
            _ = aaa_host::shutdown_signal() => {},
            _ = restart.recv(), if registration.is_some() => request(),
        }
    }
}

pub(crate) async fn shutdown(role: &str) {
    #[cfg(target_os = "macos")]
    mac::shutdown(role).await;
    #[cfg(not(target_os = "macos"))]
    {
        let _ = role;
        aaa_host::shutdown_signal().await;
    }
}
pub(crate) fn finish(code: ExitCode) -> ExitCode {
    #[cfg(target_os = "macos")]
    {
        mac::finish(code)
    }
    #[cfg(not(target_os = "macos"))]
    {
        code
    }
}
#[cfg(target_os = "macos")]
pub(crate) fn supported(role: &str, launcher: u32) -> anyhow::Result<bool> {
    mac::supported(role, launcher)
}
#[cfg(target_os = "macos")]
pub(crate) use mac::{register, request, signals_ready};
