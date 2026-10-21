//! 쓰기·승격 없이 현지 프로세스 계정과 파일시스템을 읽는다.
use aaa_proto::install::*;
use std::path::Path;

pub fn platform() -> Result<InstallPlatform, Box<InstallResult>> {
    let os = match std::env::consts::OS {
        "macos" => InstallOs::Macos,
        "linux" => InstallOs::Linux,
        "windows" => InstallOs::Windows,
        _ => return Err(super::error("unsupported_platform", 5)),
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => InstallArch::Aarch64,
        "x86_64" => InstallArch::X86_64,
        _ => return Err(super::error("unsupported_platform", 5)),
    };
    Ok(InstallPlatform { os, arch })
}

pub fn account() -> Option<String> {
    aaa_common::private_file::account()
}

pub fn ancestor(path: &Path) -> Option<&Path> {
    path.ancestors().find(|p| p.is_dir())
}

#[cfg(unix)]
pub fn storage(path: &Path) -> Option<(String, u64, Option<bool>)> {
    use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
    let path = ancestor(path)?;
    let text = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: text는 NUL로 끝나고 stat은 syscall이 쓸 구조다. access는 파일을 만들지 않는다.
    if unsafe { libc::statvfs(text.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    let free = u64::try_from(u128::from(stat.f_bavail) * u128::from(stat.f_frsize)).ok()?;
    let writable = unsafe { libc::access(text.as_ptr(), libc::W_OK) } == 0;
    Some((format!("device:{}", path.metadata().ok()?.dev()), free, Some(writable)))
}

#[cfg(windows)]
pub fn storage(path: &Path) -> Option<(String, u64, Option<bool>)> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumeInformationW, GetVolumePathNameW};
    let path = ancestor(path)?;
    let wide: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = vec![0u16; 32768];
    let mut free = 0;
    let mut serial = 0;
    // SAFETY: 입력은 NUL로 끝나며 쓰기 buffer/숫자는 유효하다. 조회만 하고 볼륨을 열거나 바꾸지 않는다.
    unsafe {
        if GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) == 0
            || GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) == 0
            || GetVolumeInformationW(
                root.as_ptr(),
                std::ptr::null_mut(),
                0,
                &mut serial,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                0,
            ) == 0
        {
            return None;
        }
    }
    // ACL 쓰기 허용 여부는 관찰하지 않았다. 실제 probe/ACL 확인은 I2다.
    Some((format!("volume:{serial}"), free, None))
}
