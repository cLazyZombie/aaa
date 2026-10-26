//! 새 파일/폴더에 protected DACL을 함께 전달한다. 일반 실행의 state 이행만 자기 소유 ACL을 좁힌다.
use super::*;
use std::os::windows::{
    ffi::OsStrExt,
    fs::MetadataExt,
    io::{AsRawHandle, FromRawHandle},
};
use windows_sys::Win32::{
    Foundation::{ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
            SE_FILE_OBJECT,
        },
        DACL_SECURITY_INFORMATION, GetAce, OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_NORMAL,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_ALWAYS,
    },
};

pub(crate) struct Descriptor(*mut core::ffi::c_void);
impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: descriptor는 Windows가 LocalAlloc으로 할당했다.
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn security(directory: bool) -> io::Result<(Descriptor, SECURITY_ATTRIBUTES)> {
    security_for(directory, true)
}
/// Pipe는 현재 SID만, 파일은 설치기와 같은 현재 SID+SYSTEM을 허용한다.
pub(crate) fn security_for(directory: bool, system: bool) -> io::Result<(Descriptor, SECURITY_ATTRIBUTES)> {
    let sid = super::account().ok_or_else(denied)?;
    let inheritance = if directory { "OICI" } else { "" };
    let system = if system { format!("(A;{inheritance};FA;;;SY)") } else { String::new() };
    let text: Vec<_> =
        format!("O:{sid}D:P{system}(A;{inheritance};FA;;;{sid})").encode_utf16().chain(Some(0)).collect();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: 입출력은 NUL 종료 buffer와 유효한 descriptor pointer다.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), 1, &mut descriptor, std::ptr::null_mut())
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let attrs = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    Ok((Descriptor(descriptor), attrs))
}

/// 일반 실행의 옛 state/TLS 권한 이행. 자기 소유의 실제 파일/폴더 handle만 좁힌다.
/// 설치기의 승인/status 경로는 이 함수를 쓰지 않고 기존 ACL을 검증만 한다.
pub(crate) fn protect(path: &Path, directory: bool) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Security::Authorization::SetSecurityInfo,
        Security::{
            GetSecurityDescriptorControl, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner,
            PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        },
        Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES, READ_CONTROL, WRITE_DAC, WRITE_OWNER},
    };
    let current = super::account().ok_or_else(denied)?;
    let default_owner = super::default_owner().ok_or_else(denied)?;
    // 승격·서비스 token은 Administrators를 기본 owner로 쓸 수 있다. 이 경우에만
    // 최초 handle에 WRITE_OWNER를 요청한다. ReOpenFile은 디렉터리에서 접근 거부를
    // 반환하므로, 처음 연 실제 객체를 검증한 뒤 같은 handle로 소유권까지 좁힌다.
    let access = READ_CONTROL
        | WRITE_DAC
        | FILE_READ_ATTRIBUTES
        | if directory { 0 } else { GENERIC_READ }
        | if default_owner != current { WRITE_OWNER } else { 0 };
    let flags = FILE_FLAG_OPEN_REPARSE_POINT | if directory { FILE_FLAG_BACKUP_SEMANTICS } else { 0 };
    let file = OpenOptions::new().access_mode(access).custom_flags(flags).open(path)?;
    verify_kind(&file, directory)?;
    let mut owner = std::ptr::null_mut();
    let mut old_dacl = std::ptr::null_mut();
    let mut old = std::ptr::null_mut();
    // SAFETY: 실제 handle의 소유권을 읽어 타 사용자 파일의 ACL을 바꾸지 않는다.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut old_dacl,
            std::ptr::null_mut(),
            &mut old,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let _old = Descriptor(old);
    if owner.is_null() {
        return Err(denied());
    }
    let old_owner = sid_text(owner)?;
    let normalize_owner = old_owner != current;
    if normalize_owner && old_owner != default_owner {
        return Err(denied());
    }
    let (_descriptor, attrs) = security(directory)?;
    let mut dacl = std::ptr::null_mut();
    let mut present = 0;
    let mut defaulted = 0;
    // SAFETY: descriptor는 위에서 만든 protected DACL이며 호출 동안 살아 있다.
    if unsafe { GetSecurityDescriptorDacl(attrs.lpSecurityDescriptor, &mut present, &mut dacl, &mut defaulted) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if present == 0 || dacl.is_null() {
        return Err(denied());
    }
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: old와 두 ACL은 이 함수가 소유한 descriptor 안에 있고 호출 동안 살아 있다.
    if unsafe { GetSecurityDescriptorControl(old, &mut control, &mut revision) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // 같은 protected DACL을 다시 적용하면 Windows가 모든 자식에 상속을 재전파한다.
    // 경로 캐시 대신 매번 실제 handle의 소유권·종류·보호 비트·ACL 전체를 확인한다.
    if !normalize_owner && control & SE_DACL_PROTECTED != 0 && !old_dacl.is_null() {
        let unchanged = unsafe {
            let current = std::slice::from_raw_parts(old_dacl.cast::<u8>(), (*old_dacl).AclSize as usize);
            let desired = std::slice::from_raw_parts(dacl.cast::<u8>(), (*dacl).AclSize as usize);
            current == desired
        };
        if unchanged {
            return Ok(file);
        }
    }
    let mut new_owner = std::ptr::null_mut();
    if normalize_owner
        && unsafe { GetSecurityDescriptorOwner(attrs.lpSecurityDescriptor, &mut new_owner, &mut defaulted) } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let code = unsafe {
        SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION
                | PROTECTED_DACL_SECURITY_INFORMATION
                | if normalize_owner { OWNER_SECURITY_INFORMATION } else { 0 },
            new_owner,
            std::ptr::null_mut(),
            dacl,
            std::ptr::null_mut(),
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    verify(&file, directory)?;
    Ok(file)
}
pub(super) fn create_directory(path: &Path) -> io::Result<()> {
    let (_descriptor, attrs) = security(true)?;
    // SAFETY: path와 attrs는 호출이 끝날 때까지 살아 있다.
    if unsafe { CreateDirectoryW(wide(path).as_ptr(), &attrs) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub fn new_file(path: &Path) -> io::Result<File> {
    let (_descriptor, attrs) = security(false)?;
    // SAFETY: CREATE_NEW만 사용하고 handle 소유권을 File에 한 번 넘긴다.
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            &attrs,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { File::from_raw_handle(handle) })
}

/// 쓰기 handle 하나만 예약한다. 잠금 파일의 읽기 관찰은 허용하고 쓰기/삭제 handle은 거부한다.
/// Windows 전체 byte-range lock은 같은 process의 두 번째 read handle까지 막아 status/snapshot이 실패한다.
pub(super) fn lock(path: &Path) -> io::Result<File> {
    let (_descriptor, attrs) = security(false)?;
    // SAFETY: OPEN_ALWAYS는 기존 ACL을 바꾸지 않는다. reparse를 따라가지 않으며 반환 handle을 검증한다.
    let handle = unsafe {
        CreateFileW(
            wide(path).as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ,
            &attrs,
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = io::Error::last_os_error();
        return Err(if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) {
            io::Error::new(io::ErrorKind::WouldBlock, "설치 자원을 다른 실행기가 사용 중입니다")
        } else {
            error
        });
    }
    // SAFETY: 이 성공 handle의 소유권은 File 한 곳에만 넘긴다. 닫을 때 예약이 풀린다.
    let file = unsafe { File::from_raw_handle(handle) };
    verify(&file, false)?;
    Ok(file)
}
fn sid_text(sid: *mut core::ffi::c_void) -> io::Result<String> {
    let mut text = std::ptr::null_mut();
    // SAFETY: sid는 GetSecurityInfo/GetAce가 반환한 descriptor 내부다.
    if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut len = 0;
    unsafe {
        while *text.add(len) != 0 {
            len += 1;
        }
    }
    let result = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, len) });
    unsafe {
        LocalFree(text.cast());
    }
    Ok(result)
}
fn verify_kind(file: &File, directory: bool) -> io::Result<()> {
    let meta = file.metadata()?;
    if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || if directory { !meta.is_dir() } else { !meta.is_file() }
    {
        return Err(denied());
    }
    if !directory {
        let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.nNumberOfLinks != 1 {
            return Err(denied());
        }
    }
    Ok(())
}
pub fn verify(file: &File, directory: bool) -> io::Result<()> {
    verify_kind(file, directory)?;
    let account = super::account().ok_or_else(denied)?;
    let mut owner = std::ptr::null_mut();
    let mut dacl = std::ptr::null_mut();
    let mut descriptor = std::ptr::null_mut();
    // SAFETY: 실제로 연 handle에서만 descriptor를 읽고 모든 반환 buffer를 해제한다.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut dacl,
            std::ptr::null_mut(),
            &mut descriptor,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code as i32));
    }
    let _descriptor = Descriptor(descriptor);
    if owner.is_null() || dacl.is_null() || sid_text(owner)? != account || unsafe { (*dacl).AceCount } != 2 {
        return Err(denied());
    }
    let mut principals = std::collections::BTreeSet::new();
    for i in 0..2 {
        let mut ace = std::ptr::null_mut();
        if unsafe { GetAce(dacl, i, &mut ace) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // ACCESS_ALLOWED_ACE_TYPE=0. 다른 ACE 형식·권한은 private record로 신뢰하지 않는다.
        let header = unsafe { &*ace.cast::<ACE_HEADER>() };
        if header.AceType != 0 || usize::from(header.AceSize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>() {
            return Err(denied());
        }
        let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
        if allowed.Mask != FILE_ALL_ACCESS {
            return Err(denied());
        }
        principals.insert(sid_text(std::ptr::addr_of!(allowed.SidStart).cast_mut().cast())?);
    }
    if principals != [account, "S-1-5-18".into()].into_iter().collect() {
        return Err(denied());
    }
    Ok(())
}
