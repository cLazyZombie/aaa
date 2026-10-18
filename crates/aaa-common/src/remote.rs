//! 다른 기기에서 host 웹으로 붙는 Front API 연결(`aaa attach --remote`, plan §6.2). herdr `--remote <ssh-target>`이 같은
//! client 규약을 SSH 너머로 싣듯, 같은 머신 socket의 Front API를 host 웹 WebSocket에 싣는다(SSH는 쓰지 않는다, 결정 6).
//!
//! - 주소는 host가 `aaa web publish`로 공개한 HTTPS 주소다. TLS는 그 주소 인증서(tailscale serve)를 webpki 루트로 확인한다
//!   (host의 다른 client `http.rs`·`notify.rs`와 같은 rustls·ring). `http://`는 host 머신 loopback(`127.0.0.1:<포트>`)만
//!   받는다: 장치 token을 다른 기기로 평문으로 보내지 않는다.
//! - 입장은 웹과 같은 장치 pairing이다. `POST /api/v1/pair`에 코드를 내 장치 token(브라우저 cookie와 같은 값)을 받고, 이 기기
//!   상태 폴더 `remote/`에 주소마다 0600으로 둔다. 연결은 그 token을 cookie 헤더로 보낸다.
//! - 비브라우저지만 웹 규칙을 그대로 지난다: Host·Origin을 그 주소로 보낸다(host `guard` 허용 목록).
//! - 통신은 `GET /api/v1/front` WebSocket이다. 글 메시지 하나가 같은 머신 socket의 제어 frame 하나다.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{self, Message};

use crate::conn::{FrameTx, frame_channel};

/// 기기를 취소해 닫은 WebSocket(host `DEVICE_REVOKED_CLOSE_CODE`)
pub const DEVICE_REVOKED_CLOSE_CODE: u16 = 4401;
/// host는 15초마다 ping한다: 이만큼 아무것도 오지 않으면 끊긴 것이다(잠든 노트북·바뀐 망)
const DEAD_AFTER: Duration = Duration::from_secs(60);
/// 연결·TLS·handshake·pairing 응답 한도
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// pairing 응답 상한(머리·본문 각각)
const PAIR_MAX: usize = 64 << 10;

/// 붙을 host 웹 주소
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub tls: bool,
    pub host: String,
    pub port: u16,
}

/// host가 장치 token을 받지 않았다(없거나 취소된 기기, 다른 host): 새로 pairing한다
#[derive(Debug, thiserror::Error)]
#[error("host가 이 기기의 token을 받지 않았다")]
pub struct Unauthorized;

impl Target {
    /// `https://<이름>[:포트]`(scheme이 없으면 https) 또는 `http://127.0.0.1:<포트>`. 경로는 `/`까지만 받는다.
    pub fn parse(url: &str) -> Result<Target> {
        let url = url.trim();
        let (tls, rest) = match url.split_once("://") {
            Some((s, r)) if s.eq_ignore_ascii_case("https") => (true, r),
            Some((s, r)) if s.eq_ignore_ascii_case("http") => (false, r),
            Some((s, _)) => bail!("주소 scheme `{s}`는 받지 않는다(https, 또는 host 머신의 http://127.0.0.1:<포트>)"),
            None => (true, url),
        };
        let authority = rest.strip_suffix('/').unwrap_or(rest);
        let bad = || anyhow!("주소 꼴이 아니다: `{url}`(예: https://mac-a.example.ts.net:8443)");
        if authority.is_empty()
            || authority.contains(['/', '?', '#', '@', '[', ']', '\\'])
            || authority.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(bad());
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().ok().filter(|p| *p != 0).ok_or_else(bad)?),
            None => (authority, if tls { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err(bad());
        }
        let host = host.to_ascii_lowercase();
        if !tls && host != "127.0.0.1" {
            bail!(
                "http는 host 머신 loopback(http://127.0.0.1:<포트>)만 받는다: 장치 token을 평문으로 보내지 않는다. 다른 기기는 \
                 host가 `aaa web publish`로 공개한 https 주소로 붙는다"
            );
        }
        Ok(Target { tls, host, port })
    }

    /// Host 헤더 꼴. https 기본 포트는 뺀다(host는 `:443`을 떼고 견준다). http는 host가 `127.0.0.1:<포트>` 그대로 견주므로
    /// 포트를 늘 붙인다.
    pub fn authority(&self) -> String {
        if self.tls && self.port == 443 { self.host.clone() } else { format!("{}:{}", self.host, self.port) }
    }

    /// 브라우저가 보낼 Origin(host `web_origin`과 같은 꼴)
    pub fn origin(&self) -> String {
        format!("{}://{}", if self.tls { "https" } else { "http" }, self.authority())
    }

    /// 장치 cookie 이름: HTTPS 경유면 host가 `__Host-aaa`만 본다(host `cookie_token`)
    fn cookie_name(&self) -> &'static str {
        if self.tls { "__Host-aaa" } else { "aaa" }
    }

    /// 이 주소의 장치 token 파일(상태 폴더 `remote/`)
    pub fn token_file(&self, state_root: &Path) -> PathBuf {
        let name: String = format!("{}-{}-{}", if self.tls { "https" } else { "http" }, self.host, self.port)
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '.') { c } else { '_' })
            .collect();
        state_root.join("remote").join(format!("{name}.token"))
    }
}

/// 저장한 장치 token(없으면 None)
pub fn load_token(file: &Path) -> Option<String> {
    std::fs::read_to_string(file).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// 장치 token을 0600 파일로 둔다(폴더는 0700). 만든 뒤 권한을 바꾸지 않고 처음부터 그 권한으로 만든다.
/// Windows는 같은 뜻의 DACL이다(본인과 SYSTEM만, `win`).
pub fn save_token(file: &Path, token: &str) -> Result<()> {
    use std::io::Write as _;
    let dir = file.parent().context("token 폴더가 없다")?;
    std::fs::create_dir_all(dir).with_context(|| format!("{} 만들기", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    #[cfg(windows)]
    win::private_dir(dir).with_context(|| format!("{}를 본인만 열게 하기", dir.display()))?;
    let tmp = file.with_extension("token.tmp");
    let _ = std::fs::remove_file(&tmp);
    #[cfg(windows)]
    let mut f = win::create_private(&tmp).with_context(|| format!("{} 쓰기", tmp.display()))?;
    #[cfg(not(windows))]
    let mut f = {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            o.mode(0o600);
        }
        o.open(&tmp).with_context(|| format!("{} 쓰기", tmp.display()))?
    };
    f.write_all(token.as_bytes())?;
    f.sync_all()?;
    std::fs::rename(&tmp, file).with_context(|| format!("{} 두기", file.display()))?;
    Ok(())
}

/// host가 받지 않는 token을 지운다
pub fn forget_token(file: &Path) -> Result<()> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(e).with_context(|| format!("{} 지우기", file.display()))
        }
        _ => Ok(()),
    }
}

/// 이 기기의 장치 token으로 붙는다(`aaa attach --remote`의 입장, plan §8.181). token이 없거나 host가 받지 않으면(401)
/// `ask_code`에 까닭을 주고 받은 pairing 코드로 새 token을 받아 둔다(받지 않은 token은 지운다). 새로 받은 token도 거부되면
/// 멈춘다. `state_root`는 이 기기 상태 폴더, `client`는 front 이름(lease), `device`는 host 기기 목록의 이름이다.
pub async fn admit(
    target: &Target,
    state_root: &Path,
    client: &str,
    device: &str,
    mut ask_code: impl FnMut(&str) -> Result<String>,
) -> Result<crate::front::FrontClient> {
    let file = target.token_file(state_root);
    let mut fresh = false;
    loop {
        let why = match load_token(&file) {
            Some(token) => match crate::front::FrontClient::connect_remote(target, &token, client).await {
                Ok(c) => return Ok(c),
                Err(e) if e.downcast_ref::<Unauthorized>().is_some() => {
                    forget_token(&file)?;
                    if fresh {
                        bail!("{}: pairing으로 받은 token도 host가 받지 않았다", target.origin());
                    }
                    "저장한 기기 token을 host가 받지 않는다(기기를 취소했거나 다른 host다)"
                }
                Err(e) => return Err(e),
            },
            None => "이 기기는 아직 그 host와 pairing하지 않았다",
        };
        let code = ask_code(why)?;
        let token = pair(target, &code, device).await?;
        save_token(&file, &token)?;
        fresh = true;
    }
}

/// Windows에서 본인만 여는 token 폴더·파일. herdr 0.9.5 `src/platform/windows.rs`의 SDDL을 따른다: 파일은
/// `create_config_temporary`·`user_security_descriptor`(:82-131·:307-349, SYSTEM과 이 계정 SID — 관리자 권한 토큰의 소유자
/// Administrators가 아니라 계정이다). 폴더는 `create_remote_private_dir`(:745-766)처럼 SYSTEM과 하나지만 소유자(OW) 대신 같은
/// 계정 SID다: herdr는 새 폴더만 만들지만 aaa는 있던 폴더를 다시 쓰므로, 관리자 권한 터미널에서 처음 만든 폴더(소유자
/// Administrators)를 나중에 일반 권한으로 다시 pairing할 때도 그 계정이 DACL을 바꿀 수 있어야 한다.
/// 상속을 끊은(`D:P`) DACL이라 %LOCALAPPDATA%가 물려주는 Administrators 같은 다른 항목이 빠진다.
#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::io;
    use std::os::windows::ffi::OsStrExt as _;
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use std::path::Path;
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        DACL_SECURITY_INFORMATION, GetTokenInformation, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        SECURITY_ATTRIBUTES, SetFileSecurityW, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CREATE_NEW, CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// 폴더: SYSTEM과 이 계정, 안에 만드는 것에도 물려준다
    fn dir_sddl(sid: &str) -> String {
        format!("D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;{sid})")
    }

    pub(super) fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    /// SDDL로 만든 보안 설명자. 사라질 때 LocalFree한다
    struct Sd(PSECURITY_DESCRIPTOR);

    impl Sd {
        fn new(sddl: &str) -> io::Result<Sd> {
            let text = wide(OsStr::new(sddl));
            let mut sd = null_mut();
            // SAFETY: text는 NUL로 끝나는 UTF-16, sd는 결과 자리다
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    null_mut(),
                )
            };
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(Sd(sd))
        }
    }

    impl Drop for Sd {
        fn drop(&mut self) {
            // SAFETY: ConvertStringSecurityDescriptorToSecurityDescriptorW가 LocalAlloc으로 준 것이다
            unsafe { LocalFree(self.0) };
        }
    }

    /// 이 프로세스 계정의 SID 글(`S-1-5-21-…`)
    pub(super) fn user_sid() -> io::Result<String> {
        let mut token = null_mut();
        // SAFETY: 지금 프로세스의 토큰을 읽기로 연다. 받은 handle은 OwnedHandle이 닫는다
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = unsafe { OwnedHandle::from_raw_handle(token) };
        let mut needed = 0u32;
        // SAFETY: 크기만 묻는다(버퍼 없음)
        unsafe { GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut needed) };
        if needed == 0 {
            return Err(io::Error::last_os_error());
        }
        // usize 버퍼라 TOKEN_USER 정렬이 맞고 뒤에 붙는 SID도 그 안에 산다
        let mut buf = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
        // SAFETY: buf는 needed bytes 이상이다
        let ok = unsafe {
            GetTokenInformation(token.as_raw_handle(), TokenUser, buf.as_mut_ptr().cast(), needed, &mut needed)
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: GetTokenInformation이 채운 TOKEN_USER다
        let user = unsafe { &*buf.as_ptr().cast::<TOKEN_USER>() };
        let mut text = null_mut();
        // SAFETY: user.User.Sid는 buf 안의 SID, text는 LocalAlloc된 글을 받는다
        if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: text는 NUL로 끝나는 UTF-16이고 다 읽은 뒤 놓는다
        let sid = unsafe {
            let len = (0..).take_while(|&i| *text.add(i) != 0).count();
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
            LocalFree(text.cast());
            s
        };
        Ok(sid)
    }

    /// 폴더를 본인과 SYSTEM만 열게 한다(Unix의 0700). 이미 있던 폴더도 그렇게 바꾼다
    pub(super) fn private_dir(dir: &Path) -> io::Result<()> {
        let sd = Sd::new(&dir_sddl(&user_sid()?))?;
        let path = wide(dir.as_os_str());
        // SAFETY: path는 NUL로 끝나는 UTF-16, sd는 살아 있는 보안 설명자다
        let ok = unsafe {
            SetFileSecurityW(path.as_ptr(), DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION, sd.0)
        };
        if ok == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }

    /// 본인과 SYSTEM만 여는 새 파일(Unix의 0600). 처음부터 그 DACL로 만든다(없던 파일만)
    pub(super) fn create_private(path: &Path) -> io::Result<std::fs::File> {
        let sd = Sd::new(&format!("D:P(A;;GA;;;SY)(A;;GA;;;{})", user_sid()?))?;
        let sa = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: 0,
        };
        let path = wide(path.as_os_str());
        // SAFETY: path는 NUL로 끝나는 UTF-16, sa와 그 설명자는 호출 동안 산다
        let h = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                &sa,
                CREATE_NEW,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        if h == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: CreateFileW가 준 handle을 File이 한 번 닫는다
        Ok(unsafe { std::fs::File::from_raw_handle(h) })
    }
}

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

fn webpki() -> rustls::RootCertStore {
    rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned())
}

/// TCP(+TLS) 연결. 키 하나하나가 바로 가게 Nagle을 끈다
async fn open(t: &Target, roots: &rustls::RootCertStore) -> Result<Box<dyn Io>> {
    let tcp = tokio::net::TcpStream::connect((t.host.as_str(), t.port))
        .await
        .with_context(|| format!("{}에 연결하지 못했다", t.origin()))?;
    let _ = tcp.set_nodelay(true);
    if !t.tls {
        return Ok(Box::new(tcp));
    }
    let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots.clone())
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from(t.host.clone())
        .map_err(|_| anyhow!("host 이름 꼴이 아니다: {}", t.host))?;
    let s = tokio_rustls::TlsConnector::from(Arc::new(cfg))
        .connect(name, tcp)
        .await
        .with_context(|| format!("{}의 TLS 인증서를 확인하지 못했다", t.origin()))?;
    Ok(Box::new(s))
}

/// pairing 코드를 내고 장치 token을 받는다(`POST /api/v1/pair`, 웹 pairing 화면과 같다). `label`은 host 기기 목록의 이름이다.
pub async fn pair(t: &Target, code: &str, label: &str) -> Result<String> {
    pair_with(t, code, label, &webpki()).await
}

async fn pair_with(t: &Target, code: &str, label: &str, roots: &rustls::RootCertStore) -> Result<String> {
    let body = serde_json::json!({ "code": code, "label": label }).to_string();
    let head = format!(
        "POST /api/v1/pair HTTP/1.1\r\nHost: {}\r\nOrigin: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Accept-Encoding: identity\r\nConnection: close\r\n\r\n",
        t.authority(),
        t.origin(),
        body.len()
    );
    let mut s = tokio::time::timeout(CONNECT_TIMEOUT, open(t, roots)).await.context("연결 시간이 지났다")??;
    s.write_all(head.as_bytes()).await?;
    s.write_all(body.as_bytes()).await?;
    s.flush().await?;
    let (status, headers, body) =
        tokio::time::timeout(CONNECT_TIMEOUT, read_response(&mut s)).await.context("pairing 응답 시간이 지났다")??;
    if status != 200 {
        let why = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v["err"]["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_string());
        bail!("host가 pairing을 거부했다(HTTP {status}): {why}");
    }
    let prefix = format!("{}=", t.cookie_name());
    headers
        .iter()
        .filter(|(k, _)| k == "set-cookie")
        .find_map(|(_, v)| v.split(';').next()?.trim().strip_prefix(&prefix).map(str::to_string))
        .filter(|t| !t.is_empty())
        .context("host가 장치 token을 주지 않았다")
}

/// HTTP/1.1 응답 하나: (상태, 소문자 머리, 본문). 본문은 Content-Length만큼 읽는다(없으면 닫힐 때까지)
async fn read_response<S: AsyncRead + Unpin + ?Sized>(s: &mut S) -> Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let split = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > PAIR_MAX {
            bail!("응답 머리가 너무 길다");
        }
        match s.read(&mut chunk).await? {
            0 => bail!("응답 머리 전에 닫혔다"),
            n => buf.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let mut body = buf[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .context("HTTP 응답이 아니다")?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let len = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse::<usize>().ok());
    if len.is_some_and(|n| n > PAIR_MAX) {
        bail!("응답 본문이 너무 길다");
    }
    while len.is_none_or(|n| body.len() < n) && body.len() <= PAIR_MAX {
        match s.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            // TLS 상대가 close_notify 없이 닫으면 여기로 온다: 길이를 모르면 받은 데까지다
            Err(_) if len.is_none() => break,
            Err(e) => return Err(e.into()),
        }
    }
    if let Some(n) = len {
        if body.len() < n {
            bail!("응답 본문이 끝나기 전에 닫혔다");
        }
        body.truncate(n);
    }
    Ok((status, headers, body))
}

/// 원격 front 연결(Front API handshake 전): 보내는 handle과 받는 쪽. `token`은 장치 token이다
pub(crate) async fn connect(t: &Target, token: &str) -> Result<(FrameTx, WsIn)> {
    connect_with(t, token, &webpki()).await
}

async fn connect_with(t: &Target, token: &str, roots: &rustls::RootCertStore) -> Result<(FrameTx, WsIn)> {
    let url = format!("{}://{}/api/v1/front", if t.tls { "wss" } else { "ws" }, t.authority());
    let mut req = url.into_client_request()?;
    let h = req.headers_mut();
    h.insert("Origin", HeaderValue::from_str(&t.origin())?);
    // token 값은 오류 글에 넣지 않는다
    let cookie = HeaderValue::from_str(&format!("{}={token}", t.cookie_name()))
        .map_err(|_| anyhow!("저장한 장치 token 꼴이 아니다"))?;
    h.insert("Cookie", cookie);
    let io = tokio::time::timeout(CONNECT_TIMEOUT, open(t, roots)).await.context("연결 시간이 지났다")??;
    let cfg = WebSocketConfig::default().max_message_size(Some(aaa_proto::MAX_FRAME));
    let handshake = tokio_tungstenite::client_async_with_config(req, io, Some(cfg));
    let ws =
        match tokio::time::timeout(CONNECT_TIMEOUT, handshake).await.context("WebSocket handshake 시간이 지났다")?
        {
            Ok((ws, _)) => ws,
            Err(tungstenite::Error::Http(res)) => return Err(refusal(t, res.status().as_u16())),
            Err(e) => return Err(anyhow!(e).context(format!("{} WebSocket 연결 실패", t.origin()))),
        };
    let (mut sink, stream) = ws.split();
    let (tx, mut out) = frame_channel();
    tokio::spawn(async move {
        // front는 제어 frame(JSON)만 보낸다
        while let Some((_, payload)) = out.recv().await {
            let Ok(text) = String::from_utf8(payload) else { continue };
            if sink.send(Message::Text(text.into())).await.is_err() {
                return;
            }
        }
        let _ = sink.close().await;
    });
    Ok((tx, WsIn { stream }))
}

/// WebSocket을 받지 않은 HTTP 답
fn refusal(t: &Target, status: u16) -> anyhow::Error {
    match status {
        401 => Unauthorized.into(),
        403 => anyhow!(
            "host가 이 주소의 Origin({})을 받지 않았다: host가 공개한 주소 그대로 붙는다(host에서 `aaa web status`)",
            t.origin()
        ),
        421 => anyhow!(
            "host가 이 주소({})를 모른다: host가 공개한 주소 그대로 붙는다(host에서 `aaa web status`)",
            t.authority()
        ),
        s => anyhow!("host가 원격 front를 받지 않았다(HTTP {s}). 옛 host면 host binary를 새로 바꾼다"),
    }
}

/// 원격 front의 받는 쪽
pub(crate) struct WsIn {
    stream: futures_util::stream::SplitStream<WebSocketStream<Box<dyn Io>>>,
}

impl WsIn {
    /// 다음 글 메시지(제어 frame). 연결이 끝나면 None. 기기 취소·무응답은 그 까닭의 오류다. 취소에 안전하다
    pub(crate) async fn next(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            let m = tokio::time::timeout(DEAD_AFTER, self.stream.next()).await.map_err(|_| {
                anyhow!("host 웹이 {}초 동안 답하지 않는다(망이 끊겼거나 host가 멈췄다)", DEAD_AFTER.as_secs())
            })?;
            match m {
                None => return Ok(None),
                Some(Ok(Message::Text(t))) => return Ok(Some(t.as_bytes().to_vec())),
                Some(Ok(Message::Close(Some(f)))) if u16::from(f.code) == DEVICE_REVOKED_CLOSE_CODE => {
                    bail!("이 기기가 host에서 취소됐다(`aaa web revoke`). 다시 붙으면 새 pairing 코드를 묻는다")
                }
                Some(Ok(Message::Close(_))) => return Ok(None),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(anyhow!(e).context("host 웹 연결이 끊겼다")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        let t = Target::parse("https://Mac-A.example.ts.net:8443/").unwrap();
        assert_eq!((t.tls, t.host.as_str(), t.port), (true, "mac-a.example.ts.net", 8443));
        assert_eq!(t.origin(), "https://mac-a.example.ts.net:8443");
        // HTTPS 경유면 host는 `__Host-` 이름의 cookie만 본다
        assert_eq!(t.cookie_name(), "__Host-aaa");
        // scheme이 없으면 https, 기본 포트는 Host·Origin에서 뺀다
        let t = Target::parse("mac-a.example.ts.net").unwrap();
        assert_eq!((t.tls, t.port, t.authority()), (true, 443, "mac-a.example.ts.net".to_string()));
        assert_eq!(t.origin(), "https://mac-a.example.ts.net");
        // http는 loopback만, 포트는 늘 붙인다(host가 `127.0.0.1:<포트>` 그대로 견준다)
        let t = Target::parse("http://127.0.0.1:7000").unwrap();
        assert_eq!((t.tls, t.origin(), t.cookie_name()), (false, "http://127.0.0.1:7000".to_string(), "aaa"));
        for bad in ["http://mac-a.example.ts.net:8443", "http://localhost:7000", "http://192.0.2.5:80"] {
            let e = Target::parse(bad).unwrap_err().to_string();
            assert!(e.contains("평문으로 보내지 않는다"), "{bad}: {e}");
        }
        for bad in [
            "",
            "ftp://mac-a.example.ts.net",
            "https://mac-a.example.ts.net/api",
            "https://u@mac-a.example.com",
            "https://mac-a.example.ts.net:0",
            "https://mac-a.example.ts.net:x",
            "https://[::1]:8443",
            "https://:8443",
        ] {
            assert!(Target::parse(bad).is_err(), "{bad}");
        }
        let f = Target::parse("https://mac-a.example.ts.net:8443").unwrap().token_file(Path::new("/s"));
        assert_eq!(f, Path::new("/s/remote/https-mac-a.example.ts.net-8443.token"));
    }

    #[cfg(unix)]
    #[test]
    fn token_file_is_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = std::env::temp_dir().join(format!("aaa-remote-token-{}", std::process::id()));
        let file = dir.join("remote").join("x.token");
        save_token(&file, "secret-1").unwrap();
        save_token(&file, "secret-2").unwrap();
        assert_eq!(load_token(&file).as_deref(), Some("secret-2"));
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(file.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        forget_token(&file).unwrap();
        forget_token(&file).unwrap();
        assert_eq!(load_token(&file), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Windows: token 파일과 폴더는 상속을 끊은 DACL에 SYSTEM과 이 계정만(herdr SDDL, 폴더도 계정 SID — `win` 설명).
    /// %LOCALAPPDATA% 아래 기본 DACL(상속, Administrators 포함)이면 실패한다
    #[cfg(windows)]
    #[test]
    fn token_file_is_private() {
        let dir = std::env::temp_dir().join(format!("aaa-remote-token-{}", std::process::id()));
        let file = dir.join("remote").join("x.token");
        save_token(&file, "secret-1").unwrap();
        save_token(&file, "secret-2").unwrap();
        assert_eq!(load_token(&file).as_deref(), Some("secret-2"));
        let sid = acl::sddl_sid(&win::user_sid().unwrap());
        let (flags, mut who) = acl::dacl(&file);
        who.sort();
        let mut want = vec![("A".to_string(), sid.clone()), ("A".to_string(), "SY".to_string())];
        want.sort();
        assert!(flags.contains('P'), "상속을 끊는다: {flags}");
        assert_eq!(who, want, "파일은 SYSTEM과 이 계정만");
        let (flags, mut who) = acl::dacl(file.parent().unwrap());
        who.sort();
        assert!(flags.contains('P'), "상속을 끊는다: {flags}");
        assert_eq!(who, want, "폴더도 SYSTEM과 이 계정만");
        forget_token(&file).unwrap();
        forget_token(&file).unwrap();
        assert_eq!(load_token(&file), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// 시험이 DACL을 읽는다(herdr `config_security_descriptor`·`config_security_sddl`처럼 SDDL로)
    #[cfg(windows)]
    mod acl {
        use std::path::Path;
        use std::ptr::null_mut;

        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{
            DACL_SECURITY_INFORMATION, GetFileSecurityW, OBJECT_SECURITY_INFORMATION, OWNER_SECURITY_INFORMATION,
            PSECURITY_DESCRIPTOR,
        };

        /// (DACL 깃발, [(ACE 종류, 대상)]). 예: `D:P(A;;FA;;;SY)` → ("P", [("A", "SY")])
        pub fn dacl(path: &Path) -> (String, Vec<(String, String)>) {
            let p = super::win::wide(path.as_os_str());
            let mut needed = 0u32;
            // SAFETY: 크기만 묻는다
            unsafe { GetFileSecurityW(p.as_ptr(), DACL_SECURITY_INFORMATION, null_mut(), 0, &mut needed) };
            assert!(needed > 0, "{}: {}", path.display(), std::io::Error::last_os_error());
            let mut sd = vec![0u8; needed as usize];
            // SAFETY: sd는 needed bytes다
            let ok = unsafe {
                GetFileSecurityW(p.as_ptr(), DACL_SECURITY_INFORMATION, sd.as_mut_ptr().cast(), needed, &mut needed)
            };
            assert!(ok != 0, "{}", std::io::Error::last_os_error());
            let sddl = to_sddl(sd.as_mut_ptr().cast(), DACL_SECURITY_INFORMATION);
            let body = sddl.strip_prefix("D:").unwrap_or_else(|| panic!("DACL이 아니다: {sddl}"));
            let (flags, aces) = body.split_once('(').unwrap_or((body, ""));
            let aces = aces
                .split('(')
                .map(|a| {
                    let f: Vec<&str> = a.trim_end_matches(')').split(';').collect();
                    (f[0].to_string(), f[5].to_string())
                })
                .collect();
            (flags.to_string(), aces)
        }

        /// SID를 SDDL이 쓰는 꼴로(내장 Administrator 계정 `S-1-5-21-…-500`은 `LA`처럼 별칭이 된다: CI runner 계정이 그렇다)
        pub fn sddl_sid(sid: &str) -> String {
            let text = super::win::wide(std::ffi::OsStr::new(&format!("O:{sid}")));
            let mut sd = null_mut();
            // SAFETY: text는 NUL로 끝나는 UTF-16, sd는 LocalAlloc된 설명자를 받는다
            let ok = unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    text.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    null_mut(),
                )
            };
            assert!(ok != 0, "{sid}: {}", std::io::Error::last_os_error());
            let owner = to_sddl(sd, OWNER_SECURITY_INFORMATION);
            // SAFETY: 위에서 받은 설명자다
            unsafe { LocalFree(sd) };
            owner.strip_prefix("O:").unwrap_or_else(|| panic!("소유자가 아니다: {owner}")).to_string()
        }

        fn to_sddl(sd: PSECURITY_DESCRIPTOR, info: OBJECT_SECURITY_INFORMATION) -> String {
            let mut text = null_mut();
            // SAFETY: sd는 살아 있는 설명자다. text는 LocalAlloc된 글을 받는다
            let ok = unsafe {
                ConvertSecurityDescriptorToStringSecurityDescriptorW(sd, SDDL_REVISION_1, info, &mut text, null_mut())
            };
            assert!(ok != 0, "{}", std::io::Error::last_os_error());
            // SAFETY: NUL로 끝나는 UTF-16, 읽은 뒤 놓는다
            unsafe {
                let len = (0..).take_while(|&i| *text.add(i) != 0).count();
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(text, len));
                LocalFree(text.cast());
                s
            }
        }
    }

    /// 가짜 host 웹(http 127.0.0.1, host `web.rs`의 pairing·`/api/v1/front` 규칙만): pairing은 코드 `good`에만 새 token을
    /// cookie(`aaa`)로 주고(틀리면 host처럼 401), WebSocket은 받아 둔 token만 들인다(아니면 401). 들인 연결의 Hello에
    /// Welcome으로 답한다.
    /// `revoke`면 Welcome 뒤 기기 취소처럼 4401로 닫는다. `reject_new`면 새로 준 token도 받지 않는다.
    #[derive(Default)]
    struct Fake {
        accepted: Vec<String>,
        issued: u32,
        /// pairing 요청(머리와 본문)
        pairs: Vec<String>,
        /// front 연결 요청 `경로|Host|Origin|Cookie`
        fronts: Vec<String>,
        revoke: bool,
        reject_new: bool,
    }

    async fn fake_host() -> (Target, Arc<std::sync::Mutex<Fake>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let st = Arc::new(std::sync::Mutex::new(Fake::default()));
        let s = st.clone();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = l.accept().await {
                tokio::spawn(fake_conn(tcp, s.clone()));
            }
        });
        (Target::parse(&format!("http://127.0.0.1:{port}")).unwrap(), st)
    }

    // handshake 거부 답(ErrorResponse)의 꼴은 tungstenite가 정한다
    #[allow(clippy::result_large_err)]
    async fn fake_conn(mut tcp: tokio::net::TcpStream, st: Arc<std::sync::Mutex<Fake>>) {
        use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
        let mut head = [0u8; 5];
        if tcp.peek(&mut head).await.unwrap_or(0) < 5 {
            return;
        }
        if &head[..5] == b"POST " {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            let (split, len) = loop {
                let n = tcp.read(&mut chunk).await.unwrap();
                assert!(n > 0, "pairing 요청이 끊겼다");
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let h = String::from_utf8_lossy(&buf[..i]).to_ascii_lowercase();
                    let len = h
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length:"))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if buf.len() >= i + 4 + len {
                        break (i, len);
                    }
                }
            };
            let body = &buf[split + 4..split + 4 + len];
            let code = serde_json::from_slice::<serde_json::Value>(body).unwrap()["code"].as_str().unwrap().to_string();
            let res = {
                let mut s = st.lock().unwrap();
                s.pairs.push(String::from_utf8_lossy(&buf).to_string());
                if code == "good" {
                    s.issued += 1;
                    let tok = format!("tok-{}", s.issued);
                    if !s.reject_new {
                        s.accepted.push(tok.clone());
                    }
                    format!(
                        "HTTP/1.1 200 OK\r\nset-cookie: aaa={tok}; Path=/; HttpOnly; SameSite=Strict\r\ncontent-length: 2\r\n\r\n{{}}"
                    )
                } else {
                    let b = r#"{"err":{"message":"코드가 틀렸다"}}"#;
                    format!("HTTP/1.1 401 Unauthorized\r\ncontent-length: {}\r\n\r\n{b}", b.len())
                }
            };
            tcp.write_all(res.as_bytes()).await.unwrap();
            let _ = tcp.shutdown().await;
            return;
        }
        let s2 = st.clone();
        let check = move |req: &Request, res: Response| -> std::result::Result<Response, ErrorResponse> {
            let h = |k: &str| req.headers().get(k).and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
            let mut s = s2.lock().unwrap();
            s.fronts.push(format!("{}|{}|{}|{}", req.uri().path(), h("host"), h("origin"), h("cookie")));
            let tok = h("cookie").strip_prefix("aaa=").unwrap_or("").to_string();
            if s.accepted.contains(&tok) {
                Ok(res)
            } else {
                let mut e = ErrorResponse::new(None);
                *e.status_mut() = tungstenite::http::StatusCode::UNAUTHORIZED;
                Err(e)
            }
        };
        let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, check).await else { return };
        let Some(Ok(Message::Text(t))) = ws.next().await else { return };
        let hello: aaa_proto::FrontMsg = serde_json::from_str(&t).unwrap();
        assert!(matches!(hello, aaa_proto::FrontMsg::Hello { .. }), "{hello:?}");
        let welcome = aaa_proto::FrontMsg::Welcome {
            proto: aaa_proto::FRONT_API,
            host_id: "h-1".into(),
            role: aaa_proto::DeviceRole::Controller,
            front: "f-1".into(),
        };
        ws.send(Message::Text(serde_json::to_string(&welcome).unwrap().into())).await.unwrap();
        if st.lock().unwrap().revoke {
            let frame = tungstenite::protocol::CloseFrame { code: DEVICE_REVOKED_CLOSE_CODE.into(), reason: "".into() };
            let _ = ws.close(Some(frame)).await;
            return;
        }
        while let Some(Ok(_)) = ws.next().await {}
    }

    /// 입장(`admit`): 처음에는 코드를 물어 pairing하고 token을 둔 뒤 붙는다. 다음에는 묻지 않는다. host가 token을 받지
    /// 않으면(401) 지우고 다시 묻고, 새로 받은 token도 거부되면 지운 채 멈춘다. 틀린 코드는 host 까닭과 함께 멈춘다.
    /// 연결은 Host·Origin을 그 주소로, token을 cookie로 보낸다. 기기를 취소하면(4401) 까닭과 함께 끝난다.
    #[tokio::test]
    async fn admit_pairs_keeps_and_forgets_tokens() {
        let (t, st) = fake_host().await;
        let root = std::env::temp_dir().join(format!("aaa-remote-admit-{}", std::process::id()));
        let file = t.token_file(&root);
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        // 한 번 붙는 동안 많아야 한 번 묻는다: 또 물으면(token을 두지 않은 고리) 그 입장은 실패로 끝난다
        let ask = |code: &'static str| {
            let asked = &asked;
            let mut once = false;
            move |why: &str| -> Result<String> {
                asked.borrow_mut().push(why.to_string());
                if std::mem::replace(&mut once, true) {
                    bail!("한 번 붙는 동안 또 물었다: {why}");
                }
                Ok(code.to_string())
            }
        };

        // 처음: 묻고 pairing → token을 둔다 → 붙는다(Welcome의 역할)
        let c = admit(&t, &root, "tui@x", "aaa TUI · x", ask("good")).await.unwrap();
        assert_eq!((c.front.as_str(), c.role), ("f-1", aaa_proto::DeviceRole::Controller));
        assert_eq!(asked.borrow().len(), 1);
        assert!(asked.borrow()[0].contains("아직 그 host와 pairing하지 않았다"), "{:?}", asked.borrow());
        assert_eq!(load_token(&file).as_deref(), Some("tok-1"));
        {
            let s = st.lock().unwrap();
            let auth = t.authority();
            assert!(s.pairs[0].starts_with("POST /api/v1/pair HTTP/1.1\r\n"), "{}", s.pairs[0]);
            assert!(s.pairs[0].contains(&format!("Host: {auth}\r\n")), "{}", s.pairs[0]);
            assert!(s.pairs[0].contains(&format!("Origin: http://{auth}\r\n")), "{}", s.pairs[0]);
            assert!(s.pairs[0].ends_with(r#"{"code":"good","label":"aaa TUI · x"}"#), "{}", s.pairs[0]);
            assert_eq!(s.fronts, vec![format!("/api/v1/front|{auth}|http://{auth}|aaa=tok-1")]);
        }
        drop(c);

        // 다음: 둔 token으로 묻지 않고 붙는다
        admit(&t, &root, "tui@x", "aaa TUI · x", ask("good")).await.unwrap();
        assert_eq!(asked.borrow().len(), 1, "둔 token이 있으면 묻지 않는다");

        // host가 받지 않는 token(기기 취소·다른 host): 지우고 다시 묻는다 → 새 token
        st.lock().unwrap().accepted.clear();
        admit(&t, &root, "tui@x", "aaa TUI · x", ask("good")).await.unwrap();
        assert!(asked.borrow()[1].contains("저장한 기기 token을 host가 받지 않는다"), "{:?}", asked.borrow());
        assert_eq!(load_token(&file).as_deref(), Some("tok-2"));

        // 새로 받은 token도 거부: 멈추고 token을 남기지 않는다
        {
            let mut s = st.lock().unwrap();
            s.accepted.clear();
            s.reject_new = true;
        }
        let e = admit(&t, &root, "tui@x", "aaa TUI · x", ask("good")).await.err().unwrap();
        assert!(format!("{e:#}").contains("pairing으로 받은 token도 host가 받지 않았다"), "{e:#}");
        assert_eq!(load_token(&file), None, "거부된 token은 지운다");
        assert!(!file.exists());

        // 틀린 코드: host 까닭으로 멈추고 token은 없다
        let e = admit(&t, &root, "tui@x", "aaa TUI · x", ask("bad")).await.err().unwrap();
        assert!(format!("{e:#}").contains("pairing을 거부했다(HTTP 401): 코드가 틀렸다"), "{e:#}");
        assert!(!file.exists());

        // 기기 취소(4401): 붙은 뒤 까닭과 함께 끝난다
        {
            let mut s = st.lock().unwrap();
            s.reject_new = false;
            s.revoke = true;
        }
        let mut c = admit(&t, &root, "tui@x", "aaa TUI · x", ask("good")).await.unwrap();
        let e = c.next().await.err().unwrap();
        assert!(format!("{e:#}").contains("이 기기가 host에서 취소됐다"), "{e:#}");
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 인증서 확인: 그 이름의 인증서를 믿는 루트로는 pairing이 되고(SNI·Host·Origin·cookie 이름 `__Host-aaa`),
    /// webpki 루트로는 자체 서명 인증서를 거부한다
    #[tokio::test]
    async fn tls_checks_the_certificate() {
        let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let cert = ck.cert.der().clone();
        let key = rustls::pki_types::PrivateKeyDer::try_from(ck.signing_key.serialize_der()).unwrap();
        let cfg = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert.clone()], key)
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(cfg));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut heads = Vec::new();
            for _ in 0..2 {
                let (tcp, _) = listener.accept().await.unwrap();
                let Ok(mut s) = acceptor.accept(tcp).await else { continue };
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap();
                heads.push(String::from_utf8_lossy(&buf[..n]).to_string());
                let body = br#"{"ok":{"role":"controller"}}"#;
                let res = format!(
                    "HTTP/1.1 200 OK\r\nset-cookie: __Host-aaa=tok-1; Path=/; HttpOnly; Secure\r\ncontent-length: {}\r\n\r\n",
                    body.len()
                );
                s.write_all(res.as_bytes()).await.unwrap();
                s.write_all(body).await.unwrap();
                s.shutdown().await.unwrap();
            }
            heads
        });
        let t = Target { tls: true, host: "localhost".into(), port };
        let e = pair_with(&t, "c", "l", &webpki()).await.unwrap_err();
        assert!(format!("{e:#}").contains("TLS 인증서를 확인하지 못했다"), "{e:#}");
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert).unwrap();
        assert_eq!(pair_with(&t, "c-1", "기기", &roots).await.unwrap(), "tok-1");
        let heads = server.await.unwrap();
        let head = &heads[0];
        assert!(head.starts_with("POST /api/v1/pair HTTP/1.1\r\n"), "{head}");
        assert!(head.contains(&format!("Host: localhost:{port}\r\n")), "{head}");
        assert!(head.contains(&format!("Origin: https://localhost:{port}\r\n")), "{head}");
        assert!(head.contains(r#"{"code":"c-1","label":"기기"}"#), "{head}");
    }
}
