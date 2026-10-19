//! 프로세스 안의 파일 한도. herdr platform/macos.rs raise_server_nofile_limit과 같은 방식이다.
//! hard·시스템·launchd 설정은 고치지 않으며, 이미 높은 soft 한도는 내리지 않는다.

pub fn raise_nofile_limit(role: &str) {
    #[cfg(unix)]
    match raise_nofile() {
        Ok((before, soft, hard)) => tracing::info!(role, before, soft, hard, "프로세스 파일 한도"),
        Err(e) => tracing::warn!(role, error = %e, "프로세스 파일 한도를 올리지 못했다"),
    }
    #[cfg(not(unix))]
    let _ = role;
}

#[cfg(unix)]
fn raise_nofile() -> std::io::Result<(u64, u64, u64)> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: getrlimit은 성공하면 limit을 초기화한다.
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut limit = unsafe { limit.assume_init() };
    let before = limit.rlim_cur;
    let target = 8192.min(limit.rlim_max);
    if before < target {
        limit.rlim_cur = target;
        // SAFETY: 현재 프로세스의 soft만 기존 hard 이하로 올린다.
        if unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok((before, limit.rlim_cur, limit.rlim_max))
}

pub fn is_nofile_error(error: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        matches!(error.raw_os_error(), Some(libc::EMFILE | libc::ENFILE))
    }
    #[cfg(not(unix))]
    {
        let _ = error;
        false
    }
}
