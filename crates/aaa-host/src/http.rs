//! 응답 본문까지 읽는 작은 HTTP/1.1 client(구독 사용량 `usage.rs`가 쓴다).
//!
//! `notify.rs`의 client는 상태 코드만 읽는다. 여기는 본문을 Content-Length·chunked·연결 닫힘 세 방식으로 읽고,
//! `max` bytes를 넘으면 그만 읽고 실패로 돌린다. https는 webpki 루트로 검증한다(notify와 같은 rustls·ring).
//! 압축은 받지 않는다(`Accept-Encoding: identity`). 새 crate 없이 이미 쓰는 TLS 위에 둔다(plan §8.45).

use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::notify::parse_url;

pub(crate) struct Response {
    pub status: u16,
    /// 머리 이름은 소문자
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub(crate) enum HttpError {
    /// 연결·TLS·형식 실패(주소의 비밀값을 넣지 않는다)
    Io(String),
    /// 본문이 상한을 넘었다
    TooLarge,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpError::Io(e) => f.write_str(e),
            HttpError::TooLarge => f.write_str("응답이 상한보다 크다"),
        }
    }
}

trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

/// 요청 하나. `headers`에 Host·Content-Length·Connection은 넣지 않는다(여기서 붙인다).
pub(crate) async fn request(
    method: &str,
    url: &str,
    headers: &[(&str, String)],
    body: &[u8],
    max: usize,
) -> Result<Response, HttpError> {
    let t = parse_url(url).map_err(HttpError::Io)?;
    let host = if t.host.contains(':') { format!("[{}]", t.host) } else { t.host.clone() };
    let authority = if t.port == if t.tls { 443 } else { 80 } { host } else { format!("{host}:{}", t.port) };
    let head = request_head(method, &authority, &t.path, headers, body)?;
    let tcp = tokio::net::TcpStream::connect((t.host.as_str(), t.port))
        .await
        .map_err(|e| HttpError::Io(format!("연결 실패: {e}")))?;
    let mut s: Box<dyn Stream> = if t.tls {
        let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| HttpError::Io(e.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(t.host.clone())
            .map_err(|_| HttpError::Io("host 이름 형식이 아니다".into()))?;
        Box::new(
            tokio_rustls::TlsConnector::from(Arc::new(cfg))
                .connect(name, tcp)
                .await
                .map_err(|e| HttpError::Io(format!("TLS 실패: {e}")))?,
        )
    } else {
        Box::new(tcp)
    };
    s.write_all(head.as_bytes()).await.map_err(|e| HttpError::Io(e.to_string()))?;
    s.write_all(body).await.map_err(|e| HttpError::Io(e.to_string()))?;
    read_response(&mut s, max).await
}

fn request_head(
    method: &str,
    authority: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> Result<String, HttpError> {
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\n");
    for (k, v) in headers {
        if v.contains(['\r', '\n']) {
            return Err(HttpError::Io(format!("{k} 헤더에 줄바꿈이 있다")));
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body.is_empty() || method != "GET" {
        head.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    head.push_str("Accept-Encoding: identity\r\nConnection: close\r\n\r\n");
    Ok(head)
}
/// OS가 보호한 LocalAPI stream도 같은 bounded HTTP framing을 사용한다.
pub(crate) async fn request_stream<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
    max: usize,
) -> Result<Response, HttpError> {
    let head = request_head(method, "local-tailscaled.sock", path, headers, body)?;
    stream.write_all(head.as_bytes()).await.map_err(|e| HttpError::Io(e.to_string()))?;
    stream.write_all(body).await.map_err(|e| HttpError::Io(e.to_string()))?;
    read_response(&mut stream, max).await
}

async fn read_response<S: AsyncRead + Unpin + ?Sized>(s: &mut S, max: usize) -> Result<Response, HttpError> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    // 머리(빈 줄까지)
    let split = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        if buf.len() > 64 * 1024 {
            return Err(HttpError::Io("응답 머리가 너무 길다".into()));
        }
        match s.read(&mut chunk).await {
            Ok(0) => return Err(HttpError::Io("응답 머리 전에 닫혔다".into())),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(HttpError::Io(e.to_string())),
        }
    };
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let mut rest = buf[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or_else(|| HttpError::Io("HTTP 응답이 아니다".into()))?;
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    let get = |name: &str| headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
    let chunked = get("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked"));
    let length = get("content-length").and_then(|v| v.parse::<usize>().ok());
    // 본문을 `want`만큼(None이면 닫힐 때까지) 더 읽는다. 쌓인 것이 `limit`를 넘으면 그만 읽는다.
    async fn fill<S: AsyncRead + Unpin + ?Sized>(
        s: &mut S,
        rest: &mut Vec<u8>,
        want: Option<usize>,
        limit: usize,
    ) -> Result<(), HttpError> {
        let mut chunk = [0u8; 8192];
        while want.is_none_or(|w| rest.len() < w) {
            if rest.len() > limit {
                return Err(HttpError::TooLarge);
            }
            match s.read(&mut chunk).await {
                Ok(0) => break,
                Ok(n) => rest.extend_from_slice(&chunk[..n]),
                Err(e) => return Err(HttpError::Io(e.to_string())),
            }
        }
        Ok(())
    }
    // 크기 줄·chunk 끝 CRLF 같은 덧붙은 bytes의 여유
    const SLACK: usize = 16 * 1024;
    let body = if chunked {
        let mut out = Vec::new();
        loop {
            // 크기 줄(앞 chunk까지는 지웠다)
            let line_end = loop {
                if let Some(i) = rest.windows(2).position(|w| w == b"\r\n") {
                    break i;
                }
                let before = rest.len();
                fill(s, &mut rest, Some(before + 1), SLACK).await?;
                if rest.len() == before {
                    return Err(HttpError::Io("chunked 본문이 중간에 끝났다".into()));
                }
            };
            let size_text = String::from_utf8_lossy(&rest[..line_end]).to_string();
            let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| HttpError::Io("chunked 크기 형식이 아니다".into()))?;
            if size == 0 {
                break;
            }
            if out.len() + size > max {
                return Err(HttpError::TooLarge);
            }
            let start = line_end + 2;
            fill(s, &mut rest, Some(start + size + 2), size + SLACK).await?;
            if rest.len() < start + size {
                return Err(HttpError::Io("chunked 본문이 중간에 끝났다".into()));
            }
            out.extend_from_slice(&rest[start..start + size]);
            rest.drain(..(start + size + 2).min(rest.len()));
        }
        out
    } else if let Some(n) = length {
        if n > max {
            return Err(HttpError::TooLarge);
        }
        fill(s, &mut rest, Some(n), n + SLACK).await?;
        rest.truncate(n);
        rest
    } else {
        fill(s, &mut rest, None, max).await?;
        if rest.len() > max {
            return Err(HttpError::TooLarge);
        }
        rest
    };
    Ok(Response { status, headers, body })
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn explicit_http_port_reaches_the_servers_host_authority() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut peer, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                peer.read_exact(&mut byte).await.unwrap();
                request.extend(byte);
                assert!(request.len() < 8192);
            }
            let head = String::from_utf8(request).unwrap();
            let status = if head.lines().any(|line| line == format!("Host: {address}")) {
                "200 OK"
            } else {
                "421 Misdirected Request"
            };
            peer.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 2\r\n\r\nok").as_bytes()).await.unwrap();
        });
        let reply = request("GET", &format!("http://{address}/instance-check"), &[], &[], 1024).await.unwrap();
        assert_eq!(reply.status, 200, "사용자가 고른 포트가 Host authority에서도 유지돼야 한다");
        server.await.unwrap();
    }

    use super::*;

    async fn read(raw: &'static [u8], max: usize) -> Result<Response, HttpError> {
        let mut s = raw;
        read_response(&mut s, max).await
    }

    #[tokio::test]
    async fn reads_content_length_chunked_and_close() {
        let r = read(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nRetry-After: 30\r\n\r\nhelloEXTRA", 100).await.unwrap();
        assert_eq!((r.status, r.body.as_slice(), r.header("retry-after")), (200, &b"hello"[..], Some("30")));
        let r = read(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n6;x=y\r\npedia \r\n0\r\n\r\n",
            100,
        )
        .await
        .unwrap();
        assert_eq!(r.body, b"Wikipedia ");
        // 작은 chunk가 많아도 덧붙은 bytes를 본문으로 세지 않는다
        let many: &'static [u8] = Box::leak(
            (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec().into_iter())
                .chain((0..5000).flat_map(|_| b"1\r\nx\r\n".to_vec()))
                .chain(b"0\r\n\r\n".to_vec())
                .collect::<Vec<u8>>()
                .into_boxed_slice(),
        );
        assert_eq!(read(many, 5000).await.unwrap().body.len(), 5000);
        let r = read(b"HTTP/1.0 429 Too Many\r\n\r\nbody until close", 100).await.unwrap();
        assert_eq!((r.status, r.body.as_slice()), (429, &b"body until close"[..]));
    }

    #[tokio::test]
    async fn refuses_more_than_max() {
        assert!(matches!(read(b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\n", 10).await, Err(HttpError::TooLarge)));
        assert!(matches!(
            read(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n20\r\n0123456789abcdef0123456789abcdef\r\n0\r\n\r\n", 10).await,
            Err(HttpError::TooLarge)
        ));
        assert!(matches!(read(b"HTTP/1.1 200 OK\r\n\r\n0123456789abcdef", 10).await, Err(HttpError::TooLarge)));
        assert!(matches!(
            read(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nab", 10).await,
            Err(HttpError::Io(_))
        ));
    }
}
