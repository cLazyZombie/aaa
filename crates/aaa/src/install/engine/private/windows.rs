//! 새 파일/폴더에 protected DACL을 함께 전달한다. OS 정책·기존 ACL은 바꾸지 않는다.
use super::*;
use std::os::windows::{
    ffi::OsStrExt,
    fs::MetadataExt,
    io::{AsRawHandle, FromRawHandle},
};
use windows_sys::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree},
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
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    },
};

struct Descriptor(*mut core::ffi::c_void);
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
    let sid = crate::install::platform::account().ok_or_else(denied)?;
    let inheritance = if directory { "OICI" } else { "" };
    let text: Vec<_> = format!("O:{sid}D:P(A;{inheritance};FA;;;SY)(A;{inheritance};FA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect();
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
pub(in crate::install::engine) fn create_directory(path: &Path) -> io::Result<()> {
    let (_descriptor, attrs) = security(true)?;
    // SAFETY: path와 attrs는 호출이 끝날 때까지 살아 있다.
    if unsafe { CreateDirectoryW(wide(path).as_ptr(), &attrs) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
pub(in crate::install::engine) fn new_file(path: &Path) -> io::Result<File> {
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
pub(in crate::install::engine) fn verify(file: &File, directory: bool) -> io::Result<()> {
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
    let account = crate::install::platform::account().ok_or_else(denied)?;
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
