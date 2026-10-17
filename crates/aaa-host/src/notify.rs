//! 알림 webhook(§8.7, M1-4)과 Web Push(§8.26, `push.rs`). inbox 항목 가운데 사람이 봐야 할 것(사람 차례·새 보고·시작 실패·
//! 세션 끝남·머신 단절)을 밖으로 보낸다. thread 항목은 herdr-projects가 notifier를 부르는 사건만 보낸다(transitions.rs
//! `PUSH_EVENTS`): 쉼·resolved·전달 모름은 inbox에만 둔다.
//!
//! - `debounce_secs` 동안 모아 프로젝트마다 한 통으로 보낸다. 여러 thread가 잇달아 바뀌어도 한 통이다.
//! - 보내기 직전에 다시 본다(herdr-web-ui push.ts 「기다리는 동안 바뀌면 보내지 않는다」): 그 사이 끝낸 항목, 이미 확인(ack)한
//!   보고, 사람 차례가 끝난 thread(확인창에 답함)의 항목은 뺀다. 남은 것이 없으면 보내지 않는다.
//! - 실패하면 다시 보내지 않는다(알림 폭주를 막는다). 실패가 이어지는 동안 경고는 한 번만 남긴다.
//! - 주소에는 봇 token 같은 비밀값이 들어갈 수 있다. 로그·흐름 사건·오류에는 주소를 넣지 않는다.
//! - https는 webpki 루트 인증서로 검증한다. http는 설정 검증에서 loopback만 허용한다(시험용).

use std::collections::{BTreeMap, HashSet};
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

/// 밖으로 알릴 항목인가: 고른 종류(`notify.item_kinds`)이고, thread 항목이면 사람이 할 일(`PUSH_EVENTS`), PR 항목이면
/// 병합·check 실패·리뷰 활동이다(herdr-projects steps.rs `pull_requests`의 notifier.send)
pub(crate) fn pushes(kinds: &[String], i: &InboxItem) -> bool {
    kinds.iter().any(|k| k == &i.item_kind)
        && match i.item_kind.as_str() {
            "thread-state" => crate::transitions::PUSH_EVENTS.contains(&i.event.as_str()),
            "pr" => matches!(i.event.as_str(), "PR merged" | "PR checks failing" | "PR review activity"),
            _ => true,
        }
}

/// 아직 알릴 만한 항목만 남긴다: 끝내지 않았고(`open`), 지난 일(`stale`: 확인한 보고, 사람 차례가 끝난 thread)이 아니다.
/// 돌려주는 수는 뺀 항목 수다.
pub(crate) fn news(
    items: Vec<InboxItem>,
    open: &HashSet<String>,
    stale: impl Fn(&InboxItem) -> bool,
) -> (Vec<InboxItem>, usize) {
    let n = items.len();
    let keep: Vec<InboxItem> = items.into_iter().filter(|i| open.contains(&i.id) && !stale(i)).collect();
    let dropped = n - keep.len();
    (keep, dropped)
}

impl Host {
    /// 보내기 직전에 다시 본 항목(`news`). 읽지 못하면 그대로 보낸다(알림을 잃지 않는 쪽).
    fn still_news(&self, project: &str, items: Vec<InboxItem>) -> Vec<InboxItem> {
        let open: HashSet<String> = match self.db.inbox_list(Some(project), false) {
            Ok(v) => v.into_iter().map(|i| i.id).collect(),
            Err(e) => {
                tracing::warn!("알림 전 inbox 다시 읽기 실패: {e:#}");
                return items;
            }
        };
        let stale = |i: &InboxItem| {
            if i.item_kind != "thread-state" {
                return false;
            }
            let Ok(r) = self.load(project, &i.subject) else { return false };
            if i.event == "new report" {
                return r.report_hash.is_some() && r.report_hash == r.acked_hash;
            }
            self.thread_view(&r).group != aaa_proto::ThreadGroup::WaitingOnYou
        };
        let (keep, dropped) = news(items, &open, stale);
        if dropped > 0 {
            self.flow_project(project, "notify_skipped", None, format!("그 사이 답했거나 확인한 항목 {dropped}개"));
        }
        keep
    }

    /// inbox 항목을 알림 대기열에 넣는다(설정에서 켜고 고른 것만, `pushes`)
    pub(crate) fn notify(&self, item: &InboxItem) {
        if pushes(&self.cfg.notify.item_kinds(), item)
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
                    let items = me.still_news(&project, items);
                    if items.is_empty() {
                        continue;
                    }
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
            item_kind: "thread-state".into(),
            subject: subject.into(),
            event: event.into(),
            summary: "사본 complete".into(),
            body: String::new(),
            done: false,
            seen: false,
            created_at_ms: 0,
            done_at_ms: None,
        }
    }

    /// 보내기 직전: 끝낸 항목과 지난 일(stale)은 빠지고 나머지는 남는다
    #[test]
    fn drops_what_was_done_or_stale_meanwhile() {
        let mut q = item("t-0001", "blocked on a prompt");
        q.id = "q".into();
        let mut r1 = item("t-0001", "new report");
        r1.id = "r1".into();
        let mut r2 = item("t-0002", "new report");
        r2.id = "r2".into();
        let mut o = item("mac-b", "down");
        o.item_kind = "outage".into();
        o.id = "o".into();
        // 확인창 항목은 끝냈고, t-0001 보고는 확인했다
        let open: HashSet<String> = ["r1", "r2", "o"].iter().map(|s| s.to_string()).collect();
        let (keep, dropped) = news(vec![q, r1, r2, o], &open, |i| i.subject == "t-0001");
        assert_eq!(keep.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["r2", "o"]);
        assert_eq!(dropped, 2);
    }

    /// 옛 종류 이름도 thread 항목을 고르고, thread 항목은 사람이 할 일만 나간다
    #[test]
    fn pushes_needs_you_and_new_reports_only() {
        let old = aaa_config::NotifySection {
            kinds: ["report", "question", "outage"].map(String::from).to_vec(),
            ..Default::default()
        };
        let kinds = old.item_kinds();
        assert_eq!(kinds, ["thread-state", "outage"]);
        for e in ["blocked on a prompt", "pane closed", "agent gone", "waiting on you", "new report", "not launched"] {
            assert!(pushes(&kinds, &item("t-0001", e)), "{e}");
        }
        for e in ["idle", "resolved", "brief not delivered", "message not delivered"] {
            assert!(!pushes(&kinds, &item("t-0001", e)), "{e}");
        }
        let mut s = item("mac-a", "sessions ended");
        s.item_kind = "session".into();
        let all = aaa_config::NotifySection::default().item_kinds();
        assert!(!pushes(&kinds, &s) && pushes(&all, &s));
        let mut pr = item("t-0001", "PR merged");
        pr.item_kind = "pr".into();
        assert!(pushes(&all, &pr));
        pr.event = "PR opened".into();
        assert!(!pushes(&all, &pr));
    }

    #[test]
    fn bodies() {
        let items = [item("t-0001", "new report"), item("t-0002", "blocked on a prompt")];
        let t = notify_body("telegram", "-100", "demo", &items);
        assert_eq!(t["chat_id"], "-100");
        assert_eq!(
            t["text"],
            "[aaa] demo\n- t-0001 new report: 사본 complete\n- t-0002 blocked on a prompt: 사본 complete"
        );
        let j = notify_body("json", "", "demo", &items);
        assert_eq!(
            (j["project"].as_str(), j["items"][1]["event"].as_str()),
            (Some("demo"), Some("blocked on a prompt"))
        );
        let many: Vec<_> = (0..100).map(|n| item(&format!("t-{n:04}"), "new report")).collect();
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
