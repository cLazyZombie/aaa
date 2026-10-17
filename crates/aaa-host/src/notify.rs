//! 알림 webhook(§8.7, M1-4)과 Web Push(§8.26, `push.rs`). inbox 항목 가운데 사람이 봐야 할 것(보고·질문·막힘·세션 끝남·머신 단절)을 밖으로 보낸다.
//!
//! - `debounce_secs` 동안 모아 프로젝트마다 한 통으로 보낸다. 질문 창이 잇달아 떠도 한 통이다.
//! - 실패하면 다시 보내지 않는다(알림 폭주를 막는다). 실패가 이어지는 동안 경고는 한 번만 남긴다.
//! - 주소에는 봇 token 같은 비밀값이 들어갈 수 있다. 로그·흐름 사건·오류에는 주소를 넣지 않는다.
//! - https는 webpki 루트 인증서로 검증한다. http는 설정 검증에서 loopback만 허용한다(시험용).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::InboxItem;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::Host;

const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// 텔레그램 메시지 한도(4096자) 안에 들게 줄인다
const TEXT_CAP: usize = 3500;

/// 알림 글: 프로젝트 한 줄과 항목마다 한 줄
pub(crate) fn notify_text(project: &str, items: &[InboxItem]) -> String {
    let mut s = format!("[aaa] {project}");
    for i in items {
        let summary: String = i.summary.chars().take(200).collect();
        s.push_str(&format!("\n- {} {}: {summary}", crate::inbox::who(i), i.event));
    }
    if s.chars().count() > TEXT_CAP {
        s = s.chars().take(TEXT_CAP).collect::<String>() + "\n…";
    }
    s
}

pub(crate) fn notify_body(format: &str, chat_id: &str, project: &str, items: &[InboxItem]) -> serde_json::Value {
    let text = notify_text(project, items);
    match format {
        "telegram" => serde_json::json!({ "chat_id": chat_id, "text": text, "disable_web_page_preview": true }),
        _ => serde_json::json!({
            "project": project,
            "text": text,
            "items": items.iter().map(|i| serde_json::json!({
                "id": i.id, "kind": i.item_kind, "subject": i.subject, "event": i.event, "summary": i.summary,
            })).collect::<Vec<_>>(),
        }),
    }
}

pub(crate) struct Target {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    pub path: String,
}

pub(crate) fn parse_url(url: &str) -> Result<Target, String> {
    let (tls, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("http(s) 주소가 아니다".into());
    };
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.ends_with(']') || h.starts_with('[') => {
            (h.trim_matches(|c| c == '[' || c == ']').to_string(), p.parse().map_err(|_| "port 형식이 아니다")?)
        }
        _ => (authority.to_string(), if tls { 443 } else { 80 }),
    };
    if host.is_empty() {
        return Err("host가 비었다".into());
    }
    Ok(Target { tls, host, port, path })
}

/// JSON 본문 POST
async fn post(url: &str, body: &[u8]) -> Result<u16, String> {
    post_raw(url, &[("Content-Type", "application/json".to_string())], body).await
}

/// HTTP/1.1 POST 한 번. 상태 코드를 돌려준다. 오류 글에는 주소(비밀값)를 넣지 않는다.
pub(crate) async fn post_raw(url: &str, headers: &[(&str, String)], body: &[u8]) -> Result<u16, String> {
    let t = parse_url(url)?;
    let mut head = format!("POST {} HTTP/1.1\r\nHost: {}\r\n", t.path, t.host);
    for (k, v) in headers {
        if v.contains(['\r', '\n']) {
            return Err(format!("{k} 헤더에 줄바꿈이 있다"));
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nUser-Agent: aaa\r\nConnection: close\r\n\r\n", body.len()));
    let tcp = tokio::net::TcpStream::connect((t.host.as_str(), t.port)).await.map_err(|e| format!("연결 실패: {e}"))?;
    let mut buf = Vec::new();
    if t.tls {
        let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(t.host.clone()).map_err(|_| "host 이름 형식이 아니다")?;
        let mut s = tokio_rustls::TlsConnector::from(Arc::new(cfg))
            .connect(name, tcp)
            .await
            .map_err(|e| format!("TLS 실패: {e}"))?;
        s.write_all(head.as_bytes()).await.map_err(|e| e.to_string())?;
        s.write_all(body).await.map_err(|e| e.to_string())?;
        read_head(&mut s, &mut buf).await;
    } else {
        let mut s = tcp;
        s.write_all(head.as_bytes()).await.map_err(|e| e.to_string())?;
        s.write_all(body).await.map_err(|e| e.to_string())?;
        read_head(&mut s, &mut buf).await;
    }
    let line = String::from_utf8_lossy(&buf).lines().next().unwrap_or_default().to_string();
    line.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| "HTTP 응답이 아니다".into())
}

/// 응답 머리(빈 줄까지)만 읽는다. 상태 코드만 쓴다.
/// Apple push 서버는 `Connection: close`를 보내도 응답 뒤 연결을 닫지 않는다(10-17 실측: 0.75초에 응답, 15초 넘게
/// 열림). 닫힐 때까지 읽으면 전달된 알림도 timeout 실패로 기록된다.
async fn read_head<S: tokio::io::AsyncRead + Unpin>(s: &mut S, buf: &mut Vec<u8>) {
    let mut chunk = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 64 * 1024 {
        match s.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

impl Host {
    /// inbox 항목을 알림 대기열에 넣는다(설정에서 켜고 고른 종류만)
    pub(crate) fn notify(&self, item: &InboxItem) {
        if self.cfg.notify.kinds.iter().any(|k| k == &item.item_kind)
            && let Some(tx) = self.notify_tx.lock().unwrap().as_ref()
        {
            let _ = tx.send(item.clone());
        }
    }

    pub fn start_notifier(self: &Arc<Self>) {
        if self.cfg.notify.webhook.is_empty() && self.vapid.is_none() {
            return;
        }
        let (tx, mut rx) = mpsc::unbounded_channel::<InboxItem>();
        *self.notify_tx.lock().unwrap() = Some(tx);
        let me = self.clone();
        tokio::spawn(async move {
            let window = Duration::from_secs(me.cfg.notify.debounce_secs);
            let mut failing = false;
            while let Some(first) = rx.recv().await {
                // 첫 항목부터 window 동안 모은다
                let mut batch: BTreeMap<String, Vec<InboxItem>> = BTreeMap::new();
                batch.entry(first.project.clone()).or_default().push(first);
                let until = tokio::time::Instant::now() + window;
                while let Ok(Some(i)) = tokio::time::timeout_at(until, rx.recv()).await {
                    batch.entry(i.project.clone()).or_default().push(i);
                }
                for (project, items) in batch {
                    me.push_items(&project, &items).await;
                    let n = &me.cfg.notify;
                    if n.webhook.is_empty() {
                        continue;
                    }
                    let body = notify_body(&n.format, &n.telegram_chat_id, &project, &items);
                    let sent = tokio::time::timeout(SEND_TIMEOUT, post(&n.webhook, body.to_string().as_bytes())).await;
                    let res = match sent {
                        Ok(Ok(code)) if (200..300).contains(&code) => Ok(code),
                        Ok(Ok(code)) => Err(format!("HTTP {code}")),
                        Ok(Err(e)) => Err(e),
                        Err(_) => Err(format!("{}초 안에 끝나지 않았다", SEND_TIMEOUT.as_secs())),
                    };
                    match res {
                        Ok(_) => {
                            failing = false;
                            me.flow_project(&project, "notify_sent", None, format!("항목 {}개", items.len()));
                        }
                        Err(e) => {
                            if !failing {
                                tracing::warn!(%project, "알림을 보내지 못했다(다시 보내지 않는다): {e}");
                            }
                            failing = true;
                            me.flow_project(&project, "notify_failed", None, e);
                        }
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(subject: &str, event: &str) -> InboxItem {
        InboxItem {
            id: format!("x-{subject}"),
            project: "demo".into(),
            item_kind: "report".into(),
            subject: subject.into(),
            event: event.into(),
            summary: "[완료] 사본 complete".into(),
            body: String::new(),
            done: false,
            seen: false,
            created_at_ms: 0,
            done_at_ms: None,
        }
    }

    #[test]
    fn bodies() {
        let items = [item("t-0001", "report ready"), item("t-0002", "question")];
        let t = notify_body("telegram", "-100", "demo", &items);
        assert_eq!(t["chat_id"], "-100");
        assert_eq!(
            t["text"],
            "[aaa] demo\n- t-0001 report ready: [완료] 사본 complete\n- t-0002 question: [완료] 사본 complete"
        );
        let j = notify_body("json", "", "demo", &items);
        assert_eq!((j["project"].as_str(), j["items"][1]["event"].as_str()), (Some("demo"), Some("question")));
        let many: Vec<_> = (0..100).map(|n| item(&format!("t-{n:04}"), "report ready")).collect();
        assert!(notify_text("demo", &many).chars().count() <= TEXT_CAP + 2);
    }

    /// 실제 https(webpki 루트) 확인: `cargo test -p aaa-host notify_https -- --ignored`(네트워크 필요)
    #[tokio::test]
    #[ignore]
    async fn notify_https() {
        let code = post("https://example.com/", b"{}").await.unwrap();
        assert!((200..600).contains(&code), "{code}");
        let e = post("https://self-signed.badssl.com/", b"{}").await.unwrap_err();
        assert!(e.contains("TLS"), "{e}");
        // Apple push는 응답 뒤 연결을 닫지 않는다: 머리만 읽고 바로 돌아와야 한다(잘못된 경로라 403)
        let t = std::time::Instant::now();
        let code = post("https://web.push.apple.com/aaa-probe-invalid", b"{}").await.unwrap();
        assert!((400..500).contains(&code) && t.elapsed() < Duration::from_secs(5), "{code} {:?}", t.elapsed());
    }

    /// 응답 뒤 연결을 닫지 않는 서버(Apple push 실측)에서도 상태 코드를 바로 돌려준다
    #[tokio::test]
    async fn response_without_close() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut req = Vec::new();
            let mut b = [0u8; 1024];
            while !req.ends_with(b"{}") {
                let n = s.read(&mut b).await.unwrap();
                req.extend_from_slice(&b[..n]);
            }
            // 머리를 두 번에 나눠 보내고 연결은 열어 둔다
            s.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n").await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            s.write_all(b"\r\n").await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let r = tokio::time::timeout(Duration::from_secs(3), post(&format!("http://{addr}/p"), b"{}")).await;
        assert_eq!(r.expect("연결이 닫히기를 기다렸다"), Ok(201));
        server.abort();
    }

    #[test]
    fn urls() {
        let t = parse_url("https://api.example.org/botSECRET/sendMessage").unwrap();
        assert_eq!(
            (t.tls, t.host.as_str(), t.port, t.path.as_str()),
            (true, "api.example.org", 443, "/botSECRET/sendMessage")
        );
        let t = parse_url("http://127.0.0.1:8080").unwrap();
        assert_eq!((t.tls, t.port, t.path.as_str()), (false, 8080, "/"));
        assert!(parse_url("ftp://x").is_err());
    }
}
