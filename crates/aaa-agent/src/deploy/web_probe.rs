//! host 감시가 새 host의 웹이 답하는지 본다(관리 GUI W6c, 답 4A). 실행 신원이 맞아도 웹이 답하지 않으면 새 판 준비가 아니다.
//! 인증 없이 `GET /api/v1/me`를 물어 인증 요구(401 `unauthorized`)가 오면 답한 것으로 본다. 안전 모드(503 `unavailable`)·
//! 연결 거부·무응답·다른 답은 아직이다. build 이름·판은 인증 없는 응답에 싣지 않는다(공개 주소로도 열린 웹이다).
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 한 번 묻는 한도
const ASK_LIMIT: Duration = Duration::from_secs(2);
/// 답하지 않았을 때 다시 묻기 전 간격. 시한은 감시가 따로 센다(이 간격은 시한을 늘리지 않는다)
pub(super) const RETRY_EVERY: Duration = Duration::from_secs(1);
const MAX_REPLY: u64 = 16 * 1024;

/// 한 번 묻고, 답하지 않았으면 다음 물음까지 기다린 뒤 거짓을 돌려준다(감시 루프의 한 회전)
pub(super) async fn ask_once(listen: String) -> bool {
    let answered = answers(&listen, ASK_LIMIT).await;
    if !answered {
        tokio::time::sleep(RETRY_EVERY).await;
    }
    answered
}

pub(super) async fn answers(listen: &str, limit: Duration) -> bool {
    let Ok(addr) = listen.parse::<SocketAddr>() else {
        return false;
    };
    // 모든 주소에 연 웹도 같은 머신에서는 loopback으로 묻는다
    let target = if addr.ip().is_unspecified() {
        let loopback: IpAddr = if addr.is_ipv4() { Ipv4Addr::LOCALHOST.into() } else { Ipv6Addr::LOCALHOST.into() };
        SocketAddr::new(loopback, addr.port())
    } else {
        addr
    };
    tokio::time::timeout(limit, ask(target)).await.ok().flatten().is_some_and(|reply| unauthorized(&reply))
}

async fn ask(addr: SocketAddr) -> Option<Vec<u8>> {
    let mut stream = tokio::net::TcpStream::connect(addr).await.ok()?;
    // host는 Host 헤더가 정확히 `127.0.0.1:<연 port>`일 때만 loopback 요청으로 받는다(DNS rebinding 방어)
    let request = format!(
        "GET /api/v1/me HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
        addr.port()
    );
    stream.write_all(request.as_bytes()).await.ok()?;
    let mut reply = Vec::new();
    stream.take(MAX_REPLY).read_to_end(&mut reply).await.ok()?;
    Some(reply)
}

/// 상태 줄이 401이고 본문이 `unauthorized` 오류다(host 웹의 짝 없는 기기 답)
fn unauthorized(reply: &[u8]) -> bool {
    let text = String::from_utf8_lossy(reply);
    let Some((head, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    head.lines().next().is_some_and(|status| status.split_whitespace().nth(1) == Some("401"))
        && body.contains(r#""code":"unauthorized""#)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// 요청 한 번을 받아 그 글을 돌려주고 `reply`로 답하는 가짜 웹(`None`이면 답하지 않고 연결을 쥔다)
    async fn fake(reply: Option<&'static str>) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 4096];
            let n = s.read(&mut buf).await.unwrap();
            let _ = tx.send(String::from_utf8_lossy(&buf[..n]).into_owned());
            match reply {
                Some(r) => s.write_all(r.as_bytes()).await.unwrap(),
                None => std::future::pending::<()>().await,
            }
        });
        (addr, rx)
    }

    #[tokio::test]
    async fn only_the_unpaired_answer_of_a_normal_web_counts() {
        let normal = "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: 58\r\n\r\n\
                      {\"err\":{\"code\":\"unauthorized\",\"message\":\"pairing이 필요하다\"}}";
        let (addr, request) = fake(Some(normal)).await;
        assert!(answers(&addr, ASK_LIMIT).await);
        let port = addr.rsplit(':').next().unwrap();
        let request = request.await.unwrap();
        assert!(request.starts_with("GET /api/v1/me HTTP/1.1\r\n"), "{request}");
        assert!(request.contains(&format!("\r\nHost: 127.0.0.1:{port}\r\n")), "{request}");

        // 안전 모드, 다른 오류, 상태만 401인 다른 글은 답이 아니다
        for reply in [
            "HTTP/1.1 503 Service Unavailable\r\nx-aaa-mode: safe_mode\r\n\r\n{\"err\":{\"code\":\"unavailable\"}}",
            "HTTP/1.1 421 Misdirected Request\r\n\r\n{\"err\":{\"code\":\"unauthorized\"}}",
            "HTTP/1.1 401 Unauthorized\r\n\r\nnot json",
            "garbage",
        ] {
            let (addr, _request) = fake(Some(reply)).await;
            assert!(!answers(&addr, ASK_LIMIT).await, "{reply}");
        }
    }

    #[tokio::test]
    async fn refused_hung_or_unreadable_addresses_are_not_an_answer() {
        // 닫힌 port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed = listener.local_addr().unwrap().to_string();
        drop(listener);
        assert!(!answers(&closed, ASK_LIMIT).await);
        // 받고 답하지 않는 웹: 한도에서 끝난다
        let (addr, request) = fake(None).await;
        assert!(!answers(&addr, Duration::from_millis(200)).await);
        assert!(request.await.is_ok(), "요청은 닿았다");
        assert!(!answers("not-an-address", ASK_LIMIT).await);
    }

    #[tokio::test]
    async fn an_unspecified_listen_address_is_asked_on_loopback() {
        let normal = "HTTP/1.1 401 Unauthorized\r\n\r\n{\"err\":{\"code\":\"unauthorized\"}}";
        let (addr, _request) = fake(Some(normal)).await;
        let port = addr.rsplit(':').next().unwrap();
        assert!(answers(&format!("0.0.0.0:{port}"), ASK_LIMIT).await);
    }
}
