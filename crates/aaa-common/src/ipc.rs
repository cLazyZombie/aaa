//! 같은 머신 IPC. Unix는 사용자 전용 UDS(peer UID 확인), Windows는 Named Pipe.
//! session host·로컬 agent link·front CLI가 쓴다. 외부 네트워크에는 열지 않는다.

use std::io;
use std::path::{Path, PathBuf};

use tokio::io::{AsyncRead, AsyncWrite};

pub trait Duplex: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Duplex for T {}

pub type LocalStream = Box<dyn Duplex>;

pub struct LocalListener {
    #[cfg(unix)]
    inner: tokio::net::UnixListener,
    #[cfg(windows)]
    inner: tokio::sync::Mutex<tokio::net::windows::named_pipe::NamedPipeServer>,
    #[cfg(windows)]
    name: String,
    path: PathBuf,
}

#[cfg(windows)]
fn pipe_name(path: &Path) -> String {
    // 같은 기존 디렉터리의 8.3 이름과 긴 이름도 같은 주소다. socket leaf는
    // 파일이 아니므로 부모만 OS에 조회한다. 조회 실패는 기존 주소를 유지한다.
    let path = PathBuf::from(path.to_string_lossy().replace('/', "\\"));
    let expanded = path.parent().and_then(|parent| long_pipe_parent(parent).ok()).zip(path.file_name());
    legacy_pipe_name(&expanded.map_or_else(|| path.clone(), |(parent, leaf)| parent.join(leaf)))
}

#[cfg(windows)]
fn long_pipe_parent(path: &Path) -> io::Result<PathBuf> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows_sys::Win32::Storage::FileSystem::GetLongPathNameW;
    let input: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut output = vec![0u16; 32768];
    // SAFETY: input은 NUL 종료, output은 호출 동안 유효한 쓰기 버퍼다.
    let count = unsafe { GetLongPathNameW(input.as_ptr(), output.as_mut_ptr(), output.len() as u32) };
    if count == 0 {
        return Err(io::Error::last_os_error());
    }
    if count as usize >= output.len() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "IPC parent path too long"));
    }
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&output[..count as usize])))
}

#[cfg(windows)]
fn legacy_pipe_name(path: &Path) -> String {
    use sha2::Digest;
    let h = sha2::Sha256::digest(path.to_string_lossy().as_bytes());
    format!(r"\\.\pipe\aaa-{}", hex::encode(&h[..12]))
}

#[cfg(windows)]
fn pipe_alias(path: &Path) -> Option<String> {
    // 기존 pipe 이름을 바꾸지 않는다. install의 canonical state(\\?\...)와
    // 이미 실행 중인 host의 DOS state가 같은 실제 디렉터리일 때만 별칭을 찾는다.
    let parent = path.parent()?.canonicalize().ok()?;
    let leaf = path.file_name()?;
    let text = leaf.to_str()?;
    if text.ends_with(['.', ' ']) || text.contains(':') {
        return None;
    }
    let canonical = parent.join(leaf);
    let alias = if canonical != path {
        canonical
    } else {
        let text = canonical.to_str()?;
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{rest}"))
        } else {
            let rest = text.strip_prefix(r"\\?\")?;
            if !matches!(canonical.components().next(), Some(std::path::Component::Prefix(p)) if matches!(p.kind(), std::path::Prefix::VerbatimDisk(_)))
            {
                return None;
            }
            PathBuf::from(rest)
        }
    };
    // verbatim 이름의 DOS 해석이 다른 경로를 가리키면 확장하지 않는다.
    if alias.parent()?.canonicalize().ok()? != parent {
        return None;
    }
    (alias != path).then(|| pipe_name(&alias))
}

#[cfg(windows)]
fn pipe_server(name: &str, first: bool) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::net::windows::named_pipe::ServerOptions;
    let (_descriptor, mut attrs) = crate::private_file::security_for(false, false)?;
    // SAFETY: attrs와 그 descriptor는 동기 생성 호출이 끝날 때까지 살아 있다.
    unsafe {
        ServerOptions::new().first_pipe_instance(first).reject_remote_clients(true).create_with_security_attributes_raw(
            name,
            (&mut attrs as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES).cast(),
        )
    }
}

impl LocalListener {
    pub fn bind(path: &Path) -> io::Result<Self> {
        #[cfg(unix)]
        {
            if let Some(dir) = path.parent() {
                crate::util::ensure_private_dir(dir)?;
            }
            // 이전 프로세스가 남긴 socket 파일. 살아 있는 상대가 있으면 bind 전에 확인한다.
            if path.exists() {
                if std::os::unix::net::UnixStream::connect(path).is_ok() {
                    return Err(io::Error::new(io::ErrorKind::AddrInUse, format!("이미 사용 중: {}", path.display())));
                }
                std::fs::remove_file(path)?;
            }
            let inner = tokio::net::UnixListener::bind(path)?;
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
            Ok(LocalListener { inner, path: path.to_path_buf() })
        }
        #[cfg(windows)]
        {
            let name = pipe_name(path);
            let server = pipe_server(&name, true)?;
            Ok(LocalListener { inner: tokio::sync::Mutex::new(server), name, path: path.to_path_buf() })
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 같은 사용자 연결만 돌려준다(Unix peer UID). 다른 사용자는 끊고 다음을 기다린다.
    pub async fn accept(&self) -> io::Result<LocalStream> {
        self.accept_peer().await.map(|(s, _)| s)
    }

    /// [`accept`](Self::accept)와 같고 연결한 프로세스의 pid도 돌려준다(Unix peer credential — macOS `LOCAL_PEEREPID`,
    /// Linux `SO_PEERCRED` —, Windows `GetNamedPipeClientProcessId`). 못 읽으면 `None`.
    pub async fn accept_peer(&self) -> io::Result<(LocalStream, Option<u32>)> {
        #[cfg(unix)]
        loop {
            let (s, _) = self.inner.accept().await?;
            let uid = unsafe { libc::getuid() };
            match s.peer_cred() {
                Ok(c) if c.uid() == uid => {
                    let pid = c.pid().and_then(|p| u32::try_from(p).ok()).filter(|p| *p > 0);
                    return Ok((Box::new(s), pid));
                }
                _ => {
                    tracing::warn!("다른 사용자의 로컬 연결을 거부했다");
                    continue;
                }
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            let mut guard = self.inner.lock().await;
            guard.connect().await?;
            let next = pipe_server(&self.name, false)?;
            let connected = std::mem::replace(&mut *guard, next);
            let mut pid = 0u32;
            // SAFETY: 연결된 named pipe handle이고 pid는 쓸 수 있는 u32다
            let ok = unsafe {
                windows_sys::Win32::System::Pipes::GetNamedPipeClientProcessId(connected.as_raw_handle(), &mut pid)
            };
            Ok((Box::new(connected), (ok != 0 && pid > 0).then_some(pid)))
        }
    }
}

impl Drop for LocalListener {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(unix)]
type NativeClient = tokio::net::UnixStream;
#[cfg(windows)]
type NativeClient = tokio::net::windows::named_pipe::NamedPipeClient;
pub async fn connect(path: &Path) -> io::Result<LocalStream> {
    Ok(Box::new(native_connect(path).await?))
}
/// private installer callback은 server PID도 OS에서 받아 고정 부모 실행 파일과 대조한다.
pub async fn connect_peer(path: &Path) -> io::Result<(LocalStream, Option<u32>)> {
    let stream = native_connect(path).await?;
    #[cfg(unix)]
    let pid = {
        let peer = stream.peer_cred()?;
        if peer.uid() != unsafe { libc::getuid() } {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "private IPC peer owner"));
        }
        peer.pid().and_then(|pid| u32::try_from(pid).ok()).filter(|pid| *pid > 0)
    };
    #[cfg(windows)]
    let pid = {
        use std::os::windows::io::AsRawHandle;
        let mut pid = 0;
        let ok =
            unsafe { windows_sys::Win32::System::Pipes::GetNamedPipeServerProcessId(stream.as_raw_handle(), &mut pid) };
        (ok != 0 && pid > 0).then_some(pid)
    };
    Ok((Box::new(stream), pid))
}
async fn native_connect(path: &Path) -> io::Result<NativeClient> {
    #[cfg(unix)]
    {
        tokio::net::UnixStream::connect(path).await
    }
    #[cfg(windows)]
    {
        use std::time::Duration;

        use tokio::net::windows::named_pipe::ClientOptions;
        use tokio::time::{Instant, sleep_until};
        use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

        let mut names = vec![
            pipe_name(path),
            legacy_pipe_name(path),
            legacy_pipe_name(Path::new(&path.to_string_lossy().replace('/', "\\"))),
        ];
        if let Some(alias) = pipe_alias(path) {
            names.push(alias);
        }
        let mut unique = Vec::new();
        names.retain(|name| {
            if unique.contains(name) {
                false
            } else {
                unique.push(name.clone());
                true
            }
        });
        let mut names = names.into_iter();
        let mut name = names.next().expect("primary IPC name");
        // accept가 다음 인스턴스를 만들기 전의 BUSY만 기다린다. hook 전체의 3초 한도는 유지한다.
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            match ClientOptions::new().open(&name) {
                Ok(stream) => return Ok(stream),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    // 기존 raw/구분자 정규화 주소와 검증된 canonical 별칭만 읽는다.
                    match names.next() {
                        Some(next) => name = next,
                        None => return Err(error),
                    }
                }
                Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) && Instant::now() < deadline => {
                    sleep_until((Instant::now() + Duration::from_millis(5)).min(deadline)).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::future::{Future, poll_fn};
    use std::task::Poll;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

    #[test]
    fn every_pipe_instance_has_only_the_current_sid_and_transports_bytes() {
        use std::os::windows::io::AsRawHandle;
        let path = std::env::temp_dir().join(format!("aaa-pipe-acl-{}", ulid::Ulid::new()));
        let canonical = path.parent().unwrap().canonicalize().unwrap().join(path.file_name().unwrap());
        let mixed = PathBuf::from(path.to_string_lossy().replace('\\', "/"));
        let short = {
            use std::os::windows::ffi::{OsStrExt, OsStringExt};
            let input: Vec<u16> = path.parent().unwrap().as_os_str().encode_wide().chain(Some(0)).collect();
            let mut output = vec![0u16; 32768];
            // SAFETY: NUL 종료 입력과 충분한 출력 버퍼를 넘긴다.
            let count = unsafe {
                windows_sys::Win32::Storage::FileSystem::GetShortPathNameW(
                    input.as_ptr(),
                    output.as_mut_ptr(),
                    output.len() as u32,
                )
            };
            assert!(count > 0 && (count as usize) < output.len(), "short parent: {:?}", io::Error::last_os_error());
            PathBuf::from(std::ffi::OsString::from_wide(&output[..count as usize])).join(path.file_name().unwrap())
        };
        assert_ne!(path, canonical, "일반 state와 canonical state를 실제로 대조한다");
        for (case, (server_path, client_path)) in [
            (&path, &path),
            (&path, &canonical),
            (&canonical, &path),
            (&mixed, &canonical),
            (&canonical, &mixed),
            (&short, &canonical),
            (&canonical, &short),
        ]
        .into_iter()
        .enumerate()
        {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async {
                let listener = LocalListener::bind(server_path).unwrap_or_else(|error| {
                    panic!(
                        "pipe bind: case={}, server={server_path:?}, client={client_path:?}, os_error={:?}, error={error}",
                        case + 1,
                        error.raw_os_error()
                    )
                });
                assert!(LocalListener::bind(server_path).is_err(), "다른 listener가 첫 instance를 빼앗지 못한다");
                for repeat in 1..=3 {
                    crate::windows_acl_tests::assert_private(listener.inner.lock().await.as_raw_handle(), false);
                    let (client, server) = tokio::time::timeout(Duration::from_secs(3), async {
                        tokio::try_join!(connect(client_path), listener.accept_peer())
                    })
                    .await
                    .unwrap_or_else(|error| panic!("pipe event: case={}, repeat={repeat}, server={server_path:?}, client={client_path:?}, error={error}", case + 1))
                    .unwrap_or_else(|error| panic!("pipe connect/accept: case={}, repeat={repeat}, server={server_path:?}, client={client_path:?}, os_error={:?}, error={error}", case + 1, error.raw_os_error()));
                    let (mut server, pid) = server;
                    assert_eq!(pid, Some(std::process::id()));
                    let mut client = client;
                    client.write_all(b"private").await.unwrap();
                    let mut bytes = [0; 7];
                    server.read_exact(&mut bytes).await.unwrap();
                    assert_eq!(&bytes, b"private");
                    // Mio는 read 뒤 다음 overlapped read를 이미 걸어 둔다. client drop의
                    // 취소 완료/handle close를 서버 EOF로 확인한 뒤 서버를 닫는다.
                    drop(client);
                    let mut trailing = Vec::new();
                    tokio::time::timeout(Duration::from_secs(3), server.read_to_end(&mut trailing))
                        .await
                        .unwrap_or_else(|error| panic!("pipe EOF: case={}, repeat={repeat}, server={server_path:?}, client={client_path:?}, error={error}", case + 1))
                        .unwrap_or_else(|error| panic!("pipe EOF read: case={}, repeat={repeat}, server={server_path:?}, client={client_path:?}, os_error={:?}, error={error}", case + 1, error.raw_os_error()));
                    assert!(trailing.is_empty(), "예상하지 않은 추가 byte: case={}, repeat={repeat}", case + 1);
                    drop(server);
                }
                drop(listener);
            });
            // Mio의 합성 readiness/취소 완료가 Arc<Inner>를 소유한다. runtime의
            // IOCP drain까지 끝낸 뒤 같은 주소를 다시 잡아 실제 해제를 확인한다.
            drop(runtime);
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let _entered = runtime.enter();
            let rebound = LocalListener::bind(server_path).unwrap_or_else(|error| {
                panic!("pipe released rebind: case={}, server={server_path:?}, client={client_path:?}, os_error={:?}, error={error}", case + 1, error.raw_os_error())
            });
            drop(rebound);
        }
    }

    /// 실제 pipe를 점유해 BUSY를 만든다. 고정 sleep 없이 accept 사건으로 다음 연결을 푼다.
    #[tokio::test]
    async fn busy_connect_waits_for_accept_and_preserves_errors() {
        let path = std::env::temp_dir().join(format!("aaa-busy-{}", ulid::Ulid::new()));
        // BUSY가 아닌 오류는 첫 poll에서 그대로 돌려준다.
        let mut missing = std::pin::pin!(connect(&path));
        poll_fn(|cx| {
            match missing.as_mut().poll(cx) {
                Poll::Ready(Err(error)) => assert_eq!(error.kind(), io::ErrorKind::NotFound),
                _ => panic!("없는 pipe 연결을 기다리거나 성공했다"),
            }
            Poll::Ready(())
        })
        .await;

        let listener = LocalListener::bind(&path).unwrap();
        let occupied = connect(&path).await.unwrap();
        // 수정 전에도 231이 나오는 결정적 재현. 수정 뒤에도 계속 점유하면 3초 안에 같은 오류다.
        let result = tokio::time::timeout(Duration::from_secs(3), connect(&path)).await.unwrap();
        let error = result.err().expect("accept하지 않은 pipe의 두 번째 연결이 성공했다");
        assert_eq!(error.raw_os_error(), Some(ERROR_PIPE_BUSY as i32));

        let mut waiting = std::pin::pin!(connect(&path));
        poll_fn(|cx| {
            match waiting.as_mut().poll(cx) {
                Poll::Pending => {}
                Poll::Ready(Err(error)) => {
                    assert_eq!(error.raw_os_error(), Some(ERROR_PIPE_BUSY as i32));
                    panic!("BUSY를 그대로 반환했다: {error}");
                }
                Poll::Ready(Ok(_)) => panic!("점유된 pipe에 두 번째 연결이 바로 성공했다"),
            }
            Poll::Ready(())
        })
        .await;
        let (accepted, pid) = listener.accept_peer().await.unwrap();
        assert_eq!(pid, Some(std::process::id()));
        drop((occupied, accepted));

        tokio::time::timeout(Duration::from_secs(3), async {
            let (client, server) = tokio::try_join!(waiting, listener.accept_peer())?;
            let (mut server, pid) = server;
            assert_eq!(pid, Some(std::process::id()));
            let mut client = client;
            client.write_all(b"hook").await?;
            let mut bytes = [0; 4];
            server.read_exact(&mut bytes).await?;
            assert_eq!(&bytes, b"hook");
            Ok::<(), io::Error>(())
        })
        .await
        .unwrap()
        .unwrap();
    }
}
