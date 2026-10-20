//! 프로세스 안의 파일 한도(herdr platform/macos.rs raise_server_nofile_limit과 같은 방식)와 core dump 끄기.
//! hard·시스템·launchd 설정은 고치지 않으며, 이미 높은 soft 파일 한도는 내리지 않는다.

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

/// 데몬(host·agent·session host)의 core dump를 끈다: 메모리의 env 값이 core 파일로 남지 않게(키 관리자 K0, plan §8.307).
/// soft 한도만 0으로 내리고 hard는 두므로 pane 안에서 `ulimit -c`로 다시 켤 수 있다(자식 프로세스는 soft를 물려받는다).
pub fn disable_core_dump(role: &str) {
    #[cfg(unix)]
    if let Err(e) = core_soft_zero() {
        tracing::warn!(role, error = %e, "core dump를 끄지 못했다");
    }
    #[cfg(not(unix))]
    let _ = role;
}

#[cfg(unix)]
fn core_soft_zero() -> std::io::Result<()> {
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: getrlimit은 성공하면 limit을 초기화한다.
    if unsafe { libc::getrlimit(libc::RLIMIT_CORE, limit.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let mut limit = unsafe { limit.assume_init() };
    limit.rlim_cur = 0;
    // SAFETY: 현재 프로세스의 soft만 내린다(hard 이하).
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
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

#[cfg(all(test, unix))]
mod tests {
    /// core soft 한도를 hard까지 연 뒤 끄면 soft만 0이고 hard는 그대로다
    #[test]
    fn disable_core_dump_zeroes_only_the_soft_limit() {
        let get = || {
            let mut l = libc::rlimit { rlim_cur: 0, rlim_max: 0 };
            assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut l) }, 0);
            l
        };
        let mut open = get();
        open.rlim_cur = open.rlim_max;
        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_CORE, &open) }, 0);
        super::disable_core_dump("test");
        let after = get();
        assert_eq!((after.rlim_cur, after.rlim_max), (0, open.rlim_max));
    }
}
