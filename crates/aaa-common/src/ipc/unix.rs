use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::Path;

/// 경로가 같은 것만으로 소유권을 정하지 않는다. symlink의 대상도 따라가지 않는다.
pub(super) struct SocketIdentity {
    dev: u64,
    ino: u64,
    uid: u32,
}

impl SocketIdentity {
    pub(super) fn read(path: &Path) -> io::Result<Self> {
        let meta = path.symlink_metadata()?;
        if !meta.file_type().is_socket() {
            return Err(in_use(path));
        }
        // SAFETY: geteuid는 현재 프로세스의 실효 사용자 번호만 읽는다.
        if meta.uid() != unsafe { libc::geteuid() } {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("로컬 소켓 소유자가 다릅니다: {}", path.display()),
            ));
        }
        Ok(Self { dev: meta.dev(), ino: meta.ino(), uid: meta.uid() })
    }

    /// 검사 뒤 교체된 항목은 남긴다. false는 현재 경로가 다른 항목이라는 뜻이다.
    pub(super) fn remove_if_owned(&self, path: &Path) -> io::Result<bool> {
        let meta = match path.symlink_metadata() {
            Ok(meta) => meta,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error),
        };
        if !meta.file_type().is_socket() || meta.dev() != self.dev || meta.ino() != self.ino || meta.uid() != self.uid {
            return Ok(false);
        }
        // Unix에는 inode를 조건으로 한 unlink가 없다. 마지막 조회와 unlink 사이의
        // 외부 경로 변경까지 원자적으로 막지는 못한다.
        match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error),
        }
    }
}

fn in_use(path: &Path) -> io::Error {
    io::Error::new(io::ErrorKind::AddrInUse, format!("이미 사용 중이거나 교체된 로컬 소켓 경로: {}", path.display()))
}

pub(super) fn prepare_socket_path(path: &Path, probe: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    let identity = match SocketIdentity::read(path) {
        Ok(identity) => identity,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    match probe(path) {
        Ok(()) => Err(in_use(path)),
        // 연결 거부만 죽은 stream socket의 근거다. 자원·권한·일시 오류는 그대로 돌려준다.
        Err(error) if error.raw_os_error() == Some(libc::ECONNREFUSED) => {
            if identity.remove_if_owned(path)? {
                Ok(())
            } else {
                Err(in_use(path))
            }
        }
        // 검사 중 사라진 경로는 지우지 않는다. 뒤 bind가 경로의 새 점유를 판정한다.
        Err(error) if error.raw_os_error() == Some(libc::ENOENT) => Ok(()),
        Err(error) => Err(error),
    }
}
