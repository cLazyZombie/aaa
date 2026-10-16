//! 여러 역할이 함께 쓰는 작은 도구: ID, 시각, boot ID, 사용자 전용 파일.

use std::io::Write as _;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 정렬 가능한 새 ID(ULID 26자)
pub fn new_id() -> String {
    ulid::Ulid::new().to_string()
}

/// 빌드 구분값. P0 업그레이드 모의에서 `AA_BUILD_SALT`로 바꿔 「다시 빌드한 실행 파일」을 만든다.
pub const BUILD_SALT: &str = match option_env!("AA_BUILD_SALT") {
    Some(s) => s,
    None => "",
};

pub fn software() -> String {
    if BUILD_SALT.is_empty() {
        format!("aa {}", env!("CARGO_PKG_VERSION"))
    } else {
        format!("aa {}+{}", env!("CARGO_PKG_VERSION"), BUILD_SALT)
    }
}

/// 이 부팅을 가리키는 ID. 재부팅하면 바뀐다(boot fencing).
pub fn boot_id() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Some(v) = sysctl_string("kern.bootsessionuuid") {
            return v;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(v) = std::fs::read_to_string("/proc/sys/kernel/random/boot_id") {
            return v.trim().to_string();
        }
    }
    #[cfg(windows)]
    {
        // 부팅 시각(초)을 30초 단위로 반올림한다. 정밀한 boot ID는 M2에서 정한다.
        let up_ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
        let boot_s = (now_ms().saturating_sub(up_ms)) / 1000;
        return format!("boot-{}", (boot_s + 15) / 30 * 30);
    }
    #[allow(unreachable_code)]
    "unknown-boot".to_string()
}

#[cfg(target_os = "macos")]
pub fn sysctl_string(name: &str) -> Option<String> {
    let cname = std::ffi::CString::new(name).ok()?;
    let mut len: libc::size_t = 0;
    unsafe {
        if libc::sysctlbyname(cname.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        if libc::sysctlbyname(cname.as_ptr(), buf.as_mut_ptr().cast(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        buf.truncate(len);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8(buf).ok()
    }
}

pub fn hostname() -> String {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        let ok = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0;
        if ok {
            let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
            let name = String::from_utf8_lossy(&buf[..end]).to_string();
            return name.trim_end_matches(".local").to_string();
        }
    }
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "unknown".into())
}

/// 사용자만 읽고 쓰는 폴더(0700)를 만든다.
pub fn ensure_private_dir(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// 사용자 전용(0600) 파일을 원자적으로 쓴다(임시 파일 → rename).
pub fn write_private(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = p.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".{}.tmp-{}", p.file_name().unwrap().to_string_lossy(), std::process::id()));
    {
        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, p)
}

pub fn write_json<T: serde::Serialize>(p: &Path, v: &T) -> std::io::Result<()> {
    write_private(p, &serde_json::to_vec_pretty(v).expect("JSON 직렬화"))
}

pub fn read_json<T: serde::de::DeserializeOwned>(p: &Path) -> anyhow::Result<T> {
    let bytes = std::fs::read(p)?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// 같은 사용자가 띄운 프로세스가 살아 있는지
pub fn pid_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let r = unsafe { libc::kill(pid as libc::pid_t, 0) };
        r == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return false;
            }
            let mut code = 0u32;
            let ok = GetExitCodeProcess(h, &mut code) != 0;
            CloseHandle(h);
            ok && code == 259 // STILL_ACTIVE
        }
    }
}
