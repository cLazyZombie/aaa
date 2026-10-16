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
    use sha2::Digest;
    let h = sha2::Sha256::digest(path.to_string_lossy().as_bytes());
    format!(r"\\.\pipe\aaa-{}", hex::encode(&h[..12]))
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
            use tokio::net::windows::named_pipe::ServerOptions;
            let name = pipe_name(path);
            let server = ServerOptions::new().first_pipe_instance(true).create(&name)?;
            Ok(LocalListener { inner: tokio::sync::Mutex::new(server), name, path: path.to_path_buf() })
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// 같은 사용자 연결만 돌려준다(Unix peer UID). 다른 사용자는 끊고 다음을 기다린다.
    pub async fn accept(&self) -> io::Result<LocalStream> {
        #[cfg(unix)]
        loop {
            let (s, _) = self.inner.accept().await?;
            let uid = unsafe { libc::getuid() };
            match s.peer_cred() {
                Ok(c) if c.uid() == uid => return Ok(Box::new(s)),
                _ => {
                    tracing::warn!("다른 사용자의 로컬 연결을 거부했다");
                    continue;
                }
            }
        }
        #[cfg(windows)]
        {
            use tokio::net::windows::named_pipe::ServerOptions;
            let mut guard = self.inner.lock().await;
            guard.connect().await?;
            let next = ServerOptions::new().create(&self.name)?;
            let connected = std::mem::replace(&mut *guard, next);
            Ok(Box::new(connected))
        }
    }
}

impl Drop for LocalListener {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.path);
    }
}

pub async fn connect(path: &Path) -> io::Result<LocalStream> {
    #[cfg(unix)]
    {
        Ok(Box::new(tokio::net::UnixStream::connect(path).await?))
    }
    #[cfg(windows)]
    {
        use tokio::net::windows::named_pipe::ClientOptions;
        Ok(Box::new(ClientOptions::new().open(pipe_name(path))?))
    }
}
