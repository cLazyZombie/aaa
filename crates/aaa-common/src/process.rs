//! 같은 머신의 다른 프로세스: 부모·세션·제어 터미널. front 연결이 어느 aaa pane 안에서 왔는지 host가 env 자기 신고 대신
//! 판정할 때 쓴다(plan §8.164).
//!
//! - macOS `proc_pidinfo(PROC_PIDTBSDINFO)`의 `pbi_ppid`·`e_tdev`, Linux `/proc/<pid>/stat`의 ppid·tty_nr, Windows Toolhelp
//!   snapshot의 부모(세션·제어 터미널은 없다). herdr 0.9.5 `src/platform/{macos,linux,windows}.rs`가 agent 감지에 읽는 것과
//!   같은 자리다. 세션은 unix `getsid(pid)`(macOS·Linux 모두 다른 세션의 프로세스도 읽는다).
//! - 못 읽으면 `None`이다(끝난 프로세스, 권한).

/// 조상을 이만큼까지만 거슬러 오른다(고리·깊은 사슬)
const MAX_DEPTH: usize = 64;

/// macOS 책임 PID. 비공개 진단 API가 없거나 조회에 실패하면 모름이다.
/// PPID·TCC preflight·실행 속도로 책임자를 추측하지 않는다.
pub fn responsible(pid: u32) -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        if pid == 0 || pid > i32::MAX as u32 {
            return None;
        }
        // SAFETY: 조회만 하는 libSystem ABI. 지원하지 않는 OS는 symbol 없음으로 끝낸다.
        unsafe {
            let symbol = libc::dlsym(libc::RTLD_DEFAULT, c"responsibility_get_pid_responsible_for_pid".as_ptr());
            if symbol.is_null() {
                return None;
            }
            let lookup: unsafe extern "C" fn(i32) -> i32 = std::mem::transmute(symbol);
            u32::try_from(lookup(pid as i32)).ok().filter(|p| *p > 0)
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// 등록 대상 launcher 번들의 실제 image 경로 모양. 권한 승인 여부와는 별개다.
pub fn is_launcher(path: &std::path::Path) -> bool {
    path.ends_with("aaa-launcher.app/Contents/MacOS/aaa-launcher")
}

/// `pid`의 조상(부모부터, 1·0에서 멈춘다. 1은 넣는다)
pub fn ancestors(pid: u32) -> Vec<u32> {
    #[cfg(windows)]
    {
        win::ancestors(pid)
    }
    #[cfg(not(windows))]
    {
        let mut out = Vec::new();
        let mut cur = pid;
        while out.len() < MAX_DEPTH {
            let Some(p) = parent(cur) else { break };
            if p == 0 || out.contains(&p) {
                break;
            }
            out.push(p);
            if p == 1 {
                break;
            }
            cur = p;
        }
        out
    }
}

/// OS가 관찰한 실제 실행 image. argv·env의 경로나 프로세스 표시 이름은 쓰지 않는다.
pub fn image(pid: u32) -> Option<std::path::PathBuf> {
    if pid == 0 {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStringExt;
        let mut bytes = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let count = unsafe { libc::proc_pidpath(pid as i32, bytes.as_mut_ptr().cast(), bytes.len() as u32) };
        if count <= 0 {
            return None;
        }
        bytes.truncate(bytes.iter().position(|b| *b == 0)?);
        Some(std::path::PathBuf::from(std::ffi::OsString::from_vec(bytes)))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW},
        };
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            return None;
        }
        let mut path = vec![0u16; 32768];
        let mut size = path.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut size) };
        unsafe { CloseHandle(handle) };
        if ok == 0 {
            return None;
        }
        String::from_utf16(&path[..size as usize]).ok().map(std::path::PathBuf::from)
    }
    #[cfg(not(any(unix, windows)))]
    {
        None
    }
}

/// 부모 pid
pub fn parent(pid: u32) -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        shortinfo(pid).map(|i| i.pbsi_ppid)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        stat_field(pid, 1).and_then(|v| v.parse().ok())
    }
    #[cfg(windows)]
    {
        win::ancestors(pid).first().copied()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        None
    }
}

/// 제어 터미널 장치 번호(없으면 `None`). 같은 PTY를 제어 터미널로 가진 프로세스는 같은 pane 안이다.
pub fn tty(pid: u32) -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        // 제어 터미널이 없으면 NODEV(-1)
        bsdinfo(pid).map(|i| i.e_tdev).filter(|d| *d != 0 && *d != u32::MAX).map(u64::from)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        stat_field(pid, 4).and_then(|v| v.parse::<i64>().ok()).filter(|d| *d > 0).map(|d| d as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// 세션 ID(그 세션 leader의 pid). Windows는 없다.
pub fn session(pid: u32) -> Option<u32> {
    #[cfg(unix)]
    {
        if pid == 0 {
            return None;
        }
        // SAFETY: getsid는 pid만 읽는다
        let sid = unsafe { libc::getsid(pid as libc::pid_t) };
        u32::try_from(sid).ok().filter(|s| *s > 0)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        None
    }
}

/// 실행 사용자(effective uid). 다른 사용자(root 포함)의 프로세스도 읽는다. Windows는 없다
#[cfg(unix)]
pub fn uid(pid: u32) -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        shortinfo(pid).map(|i| i.pbsi_uid)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        status.lines().find_map(|l| l.strip_prefix("Uid:"))?.split_whitespace().nth(1)?.parse().ok()
    }
}

/// 부모·사용자만 담은 짧은 정보. `PROC_PIDTBSDINFO`는 다른 사용자의 프로세스(root sshd 등)에 EPERM이지만 이것은 읽힌다(`ps`와 같은 값).
/// 조상을 root 프로세스 너머까지 따라가는 데 쓴다(install 검증 SSH 입구가 root sshd를 찾는다)
#[cfg(target_os = "macos")]
fn shortinfo(pid: u32) -> Option<libc::proc_bsdshortinfo> {
    if pid == 0 {
        return None;
    }
    let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int;
    // SAFETY: info는 쓸 수 있는 proc_bsdshortinfo이고 size가 그 크기다
    let ret = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    (ret == size).then_some(info)
}

#[cfg(target_os = "macos")]
fn bsdinfo(pid: u32) -> Option<libc::proc_bsdinfo> {
    if pid == 0 {
        return None;
    }
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: info는 쓸 수 있는 proc_bsdinfo이고 size가 그 크기다
    let ret = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    (ret == size).then_some(info)
}

/// `/proc/<pid>/stat`의 `)` 뒤 n번째 칸(0: state, 1: ppid, 2: pgrp, 3: session, 4: tty_nr). comm에 빈칸·괄호가 있을 수 있어
/// 마지막 `)` 뒤부터 센다.
#[cfg(all(unix, not(target_os = "macos")))]
fn stat_field(pid: u32, n: usize) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = stat.get(stat.rfind(')')? + 2..)?;
    rest.split_whitespace().nth(n).map(str::to_string)
}

#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use std::mem::size_of;

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    /// snapshot 하나로 부모 표를 만들어 거슬러 오른다(Windows는 부모가 끝나도 다시 잇지 않아 끊긴 데서 멈춘다)
    pub fn ancestors(pid: u32) -> Vec<u32> {
        // SAFETY: 실패하면 INVALID_HANDLE_VALUE
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut parents = HashMap::new();
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        // SAFETY: entry.dwSize를 채웠다
        let mut ok = unsafe { Process32FirstW(snap, &mut entry) } != 0;
        while ok {
            parents.insert(entry.th32ProcessID, entry.th32ParentProcessID);
            // SAFETY: 같은 snapshot과 entry
            ok = unsafe { Process32NextW(snap, &mut entry) } != 0;
        }
        // SAFETY: 위에서 연 snapshot
        unsafe { CloseHandle(snap) };
        let mut out = Vec::new();
        let mut cur = pid;
        while out.len() < super::MAX_DEPTH {
            let Some(&p) = parents.get(&cur) else { break };
            if p == 0 || out.contains(&p) || !parents.contains_key(&p) {
                break;
            }
            out.push(p);
            cur = p;
        }
        out
    }
}

// 시험은 unix 것뿐이다(Windows에서 `use super::*`가 쓰이지 않아 clippy -D warnings가 막았다)
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn own_ancestry_and_terminal() {
        let me = std::process::id();
        let up = ancestors(me);
        assert_eq!(up.first().copied(), parent(me));
        assert!(!up.is_empty(), "부모가 있다");
        // 자식은 부모의 제어 터미널을 물려받는다(있으면)
        let child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
        let cpid = child.id();
        assert_eq!(parent(cpid), Some(me));
        assert_eq!(ancestors(cpid).first().copied(), Some(me));
        assert_eq!(tty(cpid), tty(me));
        assert_eq!(session(cpid), session(me));
        assert!(session(me).is_some());
        let mut child = child;
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(parent(0).is_none() && tty(0).is_none() && session(0).is_none());
    }

    /// 일반 사용자도 root 프로세스(init·launchd)의 부모·사용자를 읽는다: 조상이 root 프로세스에서 끊기지 않는다
    #[test]
    fn root_processes_are_readable_by_any_user() {
        assert_eq!(parent(1), Some(0));
        assert_eq!(uid(1), Some(0));
        assert_eq!(uid(std::process::id()), Some(unsafe { libc::geteuid() }));
        assert_eq!(ancestors(std::process::id()).last().copied(), Some(1), "조상은 1번까지 간다");
    }
}
