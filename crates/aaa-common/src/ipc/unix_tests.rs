use super::*;
use crate::child_process::StdCommandExt;
use std::os::unix::net::UnixListener as StdListener;

struct SocketDir(PathBuf);
impl SocketDir {
    fn new() -> Self {
        let path = PathBuf::from("/tmp").join(format!("aipc-{}", ulid::Ulid::new()));
        crate::util::ensure_private_dir(&path).unwrap();
        Self(path)
    }
    fn socket(&self) -> PathBuf {
        self.0.join("s")
    }
}
impl Drop for SocketDir {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn drop_preserves_replacement_listener() {
    let dir = SocketDir::new();
    let path = dir.socket();
    let original = LocalListener::bind(&path).unwrap();
    std::fs::rename(&path, dir.0.join("original")).unwrap();
    let replacement = StdListener::bind(&path).unwrap();
    drop(original);
    assert!(path.exists(), "종료한 listener가 교체된 listener 경로를 지웠다");
    let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
    let _accepted = replacement.accept().unwrap();
}

#[tokio::test]
async fn bind_preserves_non_socket_entry() {
    let dir = SocketDir::new();
    let path = dir.socket();
    std::fs::write(&path, b"keep").unwrap();
    let result = LocalListener::bind(&path);
    assert!(result.is_err(), "일반 파일을 지우고 소켓을 만들었다");
    assert_eq!(std::fs::read(&path).unwrap(), b"keep");
}

fn socket_child(role: &str, path: &Path) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "ipc::unix_tests::subprocess_fixture", "--ignored", "--nocapture"])
        .env("AAA_TEST_IPC_ROLE", role)
        .env("AAA_TEST_IPC_PATH", path)
        .capture_output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bind_preserves_listener_when_client_fds_are_exhausted() {
    let dir = SocketDir::new();
    socket_child("emfile", &dir.socket());
}

#[test]
#[ignore = "부모 시험이 경로와 역할을 지정해 격리 실행한다"]
fn subprocess_fixture() {
    let path = PathBuf::from(std::env::var_os("AAA_TEST_IPC_PATH").unwrap());
    let role = std::env::var("AAA_TEST_IPC_ROLE").unwrap();
    if role == "stale" {
        let _listener = StdListener::bind(&path).unwrap();
        return;
    }
    assert_eq!(role, "emfile");
    // FD 제한은 이 시험의 자식에게만 적용한다. 실제 listener를 연 뒤 connect의 EMFILE을 만든다.
    let listener = StdListener::bind(&path).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _entered = runtime.enter();
    let mut limit = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    // SAFETY: limit은 rlimit 크기의 유효한 출력 버퍼다.
    assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limit.as_mut_ptr()) }, 0);
    // SAFETY: getrlimit 성공으로 초기화됐다.
    let limit = unsafe { limit.assume_init() };
    let reduced = libc::rlimit { rlim_cur: limit.rlim_cur.min(64), rlim_max: limit.rlim_max };
    // SAFETY: 현재 시험 자식의 soft limit만 낮춘다.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &reduced) }, 0);
    let mut files = Vec::new();
    loop {
        match std::fs::File::open("/dev/null") {
            Ok(file) => files.push(file),
            Err(error) => {
                assert_eq!(error.raw_os_error(), Some(libc::EMFILE));
                break;
            }
        }
    }
    let result = LocalListener::bind(&path);
    drop(files);
    // SAFETY: 시험 자식이 낮춘 soft limit을 원래 값으로 돌린다.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limit) }, 0);
    let error = result.err().expect("FD가 고갈된 연결 검사가 성공했다");
    assert_eq!(error.raw_os_error(), Some(libc::EMFILE));
    assert!(path.exists(), "EMFILE 뒤 살아 있는 listener의 경로가 사라졌다");
    let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _accepted = listener.accept().unwrap();
}

#[test]
fn non_terminal_probe_errors_preserve_listener_and_errno() {
    let dir = SocketDir::new();
    let path = dir.socket();
    let listener = StdListener::bind(&path).unwrap();
    let before = std::fs::symlink_metadata(&path).unwrap();
    use std::os::unix::fs::MetadataExt;
    for errno in [
        libc::EACCES,
        libc::EPERM,
        libc::EMFILE,
        libc::ENFILE,
        libc::EINTR,
        libc::EAGAIN,
        libc::EINPROGRESS,
        libc::ETIMEDOUT,
        libc::ECONNRESET,
        libc::ENOBUFS,
        libc::EIO,
        libc::EPROTOTYPE,
    ] {
        let error = unix::prepare_socket_path(&path, |_| Err(io::Error::from_raw_os_error(errno))).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(errno));
        let after = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()), "errno={errno}");
    }
    // errno가 없는 오류도 죽은 소켓이라고 단정하지 않는다.
    let error = unix::prepare_socket_path(&path, |_| Err(io::Error::other("probe failure"))).unwrap_err();
    assert_eq!(error.to_string(), "probe failure");
    let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let _accepted = listener.accept().unwrap();
}

#[tokio::test]
async fn bind_reclaims_stale_socket_transports_bytes_and_cleans_own_path() {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = SocketDir::new();
    let path = dir.socket();
    // 다른 병렬 시험의 exec 전 자식이 부모 FD를 보유할 수 있다.
    // 전용 자식이 만든 listener는 그 자식의 종료를 기다린 뒤에만 stale로 쓴다.
    socket_child("stale", &path);
    let stale_error = std::os::unix::net::UnixStream::connect(&path).unwrap_err();
    assert_eq!(stale_error.raw_os_error(), Some(libc::ECONNREFUSED));
    let listener = LocalListener::bind(&path).unwrap();
    assert_eq!(listener.path(), path);
    assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o600);
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut client = connect(&path).await.unwrap();
        let (mut server, pid) = listener.accept_peer().await.unwrap();
        assert_eq!(pid, Some(std::process::id()));
        client.write_all(b"live").await.unwrap();
        let mut received = [0; 4];
        server.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"live");
    })
    .await
    .unwrap();
    drop(listener);
    assert!(!path.exists(), "자기 소켓은 정상 종료 때 정리한다");
    drop(LocalListener::bind(&path).unwrap());
}

#[tokio::test]
async fn bind_preserves_live_listener() {
    let dir = SocketDir::new();
    let path = dir.socket();
    let listener = StdListener::bind(&path).unwrap();
    let error = LocalListener::bind(&path).err().expect("살아 있는 listener에 중복 bind했다");
    assert_eq!(error.kind(), io::ErrorKind::AddrInUse);
    listener.set_nonblocking(true).unwrap();
    let _probe = listener.accept().unwrap();
    let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
    let _accepted = listener.accept().unwrap();
}

#[tokio::test]
async fn stale_probe_preserves_replacement_listener() {
    for errno in [libc::ECONNREFUSED, libc::ENOENT] {
        let dir = SocketDir::new();
        let path = dir.socket();
        // 다른 병렬 시험의 exec 전 자식이 부모 FD를 보유할 수 있다.
        // 전용 자식이 만든 listener는 그 자식의 종료를 기다린 뒤에만 stale로 쓴다.
        socket_child("stale", &path);
        let mut replacement = None;
        let prepared = unix::prepare_socket_path(&path, |path| {
            let refused = std::os::unix::net::UnixStream::connect(path).unwrap_err();
            assert_eq!(refused.raw_os_error(), Some(libc::ECONNREFUSED));
            // 옛 inode를 이름으로 유지해 inode 재사용의 우연을 없앤다.
            std::fs::rename(path, dir.0.join("stale")).unwrap();
            replacement = Some(StdListener::bind(path).unwrap());
            Err(io::Error::from_raw_os_error(errno))
        });
        if errno == libc::ECONNREFUSED {
            assert_eq!(prepared.unwrap_err().kind(), io::ErrorKind::AddrInUse);
        } else {
            prepared.unwrap();
            assert_eq!(tokio::net::UnixListener::bind(&path).unwrap_err().kind(), io::ErrorKind::AddrInUse);
        }
        let listener = replacement.unwrap();
        let _client = std::os::unix::net::UnixStream::connect(&path).unwrap();
        listener.set_nonblocking(true).unwrap();
        let _accepted = listener.accept().unwrap();
    }
}

#[tokio::test]
async fn bind_and_drop_preserve_symlinks_files_and_missing_paths() {
    use std::os::unix::fs::symlink;
    let dir = SocketDir::new();
    let path = dir.socket();
    let target = dir.0.join("target");
    std::fs::write(&target, b"keep").unwrap();
    for exists in [true, false] {
        let target = if exists { target.clone() } else { dir.0.join("missing") };
        symlink(&target, &path).unwrap();
        assert!(LocalListener::bind(&path).is_err(), "symlink 경로를 회수했다");
        assert_eq!(std::fs::read_link(&path).unwrap(), target);
        std::fs::remove_file(&path).unwrap();
    }
    for replacement in ["file", "symlink", "missing"] {
        let listener = LocalListener::bind(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        match replacement {
            "file" => std::fs::write(&path, b"keep").unwrap(),
            "symlink" => symlink(&target, &path).unwrap(),
            _ => {}
        }
        drop(listener);
        match replacement {
            "file" => assert_eq!(std::fs::read(&path).unwrap(), b"keep"),
            "symlink" => assert_eq!(std::fs::read_link(&path).unwrap(), target),
            _ => assert!(!path.exists()),
        }
        if replacement != "missing" {
            std::fs::remove_file(&path).unwrap();
        }
    }
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
}
