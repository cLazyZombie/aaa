//! journal/payload/backup은 생성 순간부터 사용자 전용이다. 기존 파일 권한을 넓히지 않는다.
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;

fn denied() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "설치 private 파일의 종류·소유권·권한을 확인하세요")
}

pub fn directory(path: &Path) -> io::Result<()> {
    match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(denied()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            parents(path.parent().ok_or_else(denied)?)?;
            create_directory(path)?;
        }
        Err(e) => return Err(e),
    }
    verify_directory(path)
}

/// 승인 검증/status에서는 없는 신뢰 저장소를 만들지 않는다.
pub fn verify_directory(path: &Path) -> io::Result<()> {
    verify(&open(path, true, false)?, true)
}

/// 새 부모만 사용자 전용으로 만들고 기존 부모의 권한은 바꾸지 않는다.
pub fn parents(path: &Path) -> io::Result<()> {
    match path.symlink_metadata() {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(denied()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            parents(path.parent().ok_or_else(denied)?)?;
            create_directory(path)
        }
        Err(e) => Err(e),
    }
}

#[cfg(unix)]
fn create_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new().mode(0o700).create(path)
}

#[cfg(unix)]
pub fn new_file(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(path)
}

fn open(path: &Path, directory: bool, write: bool) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(write);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | if directory { FILE_FLAG_BACKUP_SEMANTICS } else { 0 });
    }
    let _ = directory;
    options.open(path)
}

pub fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = open(path, false, false)?;
    verify(&file, false)?;
    if file.metadata()?.len() > limit {
        return Err(denied());
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit { Err(denied()) } else { Ok(bytes) }
}

pub fn lock(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        windows::lock(path)
    }
    #[cfg(unix)]
    {
        let file = match new_file(path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => open(path, false, true)?,
            Err(e) => return Err(e),
        };
        verify(&file, false)?;
        file.try_lock()
            .map_err(|_| io::Error::new(io::ErrorKind::WouldBlock, "설치 자원을 다른 실행기가 사용 중입니다"))?;
        Ok(file)
    }
}

/// stage의 마지막 권한 변경까지 내구화한다. Windows FlushFileBuffers에는 쓰기 handle이 필요하다.
pub fn sync_file(path: &Path) -> io::Result<()> {
    let file = open(path, false, cfg!(windows))?;
    if !file.metadata()?.is_file() {
        return Err(denied());
    }
    #[cfg(windows)]
    verify(&file, false)?;
    file.sync_all()
}

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_file_name(format!(".aaa-install-{}.tmp", crate::util::new_id()));
    let result = (|| {
        let mut file = new_file(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        if path.exists() {
            verify(&open(path, false, false)?, false)?;
        }
        std::fs::rename(&tmp, path)?;
        sync_parent(path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn sync_parent(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path.parent().ok_or_else(denied)?)?.sync_all()?;
    let _ = path;
    Ok(())
}

#[cfg(unix)]
pub fn verify(file: &File, directory: bool) -> io::Result<()> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let meta = file.metadata()?;
    // SAFETY: geteuid는 현재 유효 사용자 번호를 읽는다.
    if meta.uid() != unsafe { libc::geteuid() }
        || meta.permissions().mode() & 0o077 != 0
        || (if directory { !meta.is_dir() } else { !meta.is_file() || meta.nlink() != 1 })
    {
        return Err(denied());
    }
    Ok(())
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::create_directory;
#[cfg(windows)]
pub use windows::{new_file, verify};

#[cfg(unix)]
pub fn account() -> Option<String> {
    // SAFETY: 인자가 없고 현재 프로세스의 실효 uid만 읽는다. 승격 helper는 이 값을 바꾸어 쓸 수 없다.
    Some(format!("uid:{}", unsafe { libc::geteuid() }))
}

#[cfg(windows)]
pub fn account() -> Option<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, LocalFree},
        Security::Authorization::ConvertSidToStringSidW,
        Security::{GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    // SAFETY: 현재 token을 읽기 권한으로 열고 정렬된 buffer를 제공한다. 모든 handle/OS 메모리를 해제한다.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let mut len = 0;
        GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
        let mut buffer = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
        let ok = len > 0 && GetTokenInformation(token, TokenUser, buffer.as_mut_ptr().cast(), len, &mut len) != 0;
        CloseHandle(token);
        if !ok {
            return None;
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = std::ptr::null_mut();
        if ConvertSidToStringSidW(user.User.Sid, &mut text) == 0 {
            return None;
        }
        let mut n = 0;
        while *text.add(n) != 0 {
            n += 1;
        }
        let sid = String::from_utf16(std::slice::from_raw_parts(text, n)).ok();
        LocalFree(text.cast());
        sid
    }
}
