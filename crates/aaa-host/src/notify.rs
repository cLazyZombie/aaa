//! 알림 webhook(§8.7, M1-4)과 Web Push(§8.26, `push.rs`). inbox 항목 가운데 사람이 봐야 할 것(확인 필요·새 보고·시작 실패·
//! 세션 끝남·머신 단절)을 밖으로 보낸다. thread 항목은 herdr-projects가 notifier를 부르는 사건만 보낸다(transitions.rs
//! `PUSH_EVENTS`): 쉼·resolved·전달 모름은 inbox에만 둔다.
//!
//! - `debounce_secs` 동안 모아 프로젝트마다 한 통으로 보낸다. 여러 thread가 잇달아 바뀌어도 한 통이다.
//! - 보내기 직전에 다시 본다(herdr-web-ui push.ts 「기다리는 동안 바뀌면 보내지 않는다」): 그 사이 끝낸 항목, 이미 확인(ack)한
//!   보고, 확인 필요가 끝난 thread(확인창에 답함)의 항목은 뺀다. 남은 것이 없으면 보내지 않는다.
//! - 실패하면 다시 보내지 않는다(알림 폭주를 막는다). 실패가 이어지는 동안 경고는 한 번만 남긴다.
//! - 주소에는 봇 token 같은 비밀값이 들어갈 수 있다. 로그·흐름 사건·오류에는 주소를 넣지 않는다.
//! - https는 webpki 루트 인증서로 검증한다. http는 설정 검증에서 loopback만 허용한다(시험용).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::InboxItem;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::Host;

const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// 텔레그램 메시지 한도(4096자) 안에 들게 줄인다
const TEXT_CAP: usize = 3500;

/// 알림을 모으는 시간(설정 `debounce_secs`). `AAA_TEST_NOTIFY_DEBOUNCE_MS`는 시험에서만 줄인다(웹 E2E 17·18: push 판정마다
/// 1초를 기다렸다. 12는 설정 1초로 돈다)
fn debounce(secs: u64) -> Duration {
    std::env::var("AAA_TEST_NOTIFY_DEBOUNCE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(Duration::from_secs(secs), Duration::from_millis)
}

/// post 글은 알림에 넣지 않는다(웹 push·webhook 공통).
pub(crate) fn safe_summary(i: &InboxItem) -> &str {
    if i.item_kind == "post" { "사용자 결정 요청" } else { &i.summary }
}

/// 알림이 가는 자리: 프로젝트 하나, 또는 머신 사건 묶음(그 사건을 쓴 프로젝트들). 머신 사건은 그 머신에 thread가 열린
/// 프로젝트마다 inbox 항목이 생기지만 밖으로는 한 번만 알린다(plan §8.209)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Place {
    Project(String),
    Machines(Vec<String>),
}

impl Place {
    /// 흐름 사건을 남길 프로젝트
    pub(crate) fn projects(&self) -> &[String] {
        match self {
            Place::Project(p) => std::slice::from_ref(p),
            Place::Machines(ps) => ps,
        }
    }
}

/// 머신 하나의 사건인가(연결 `outage`·점검 `health`·세션 끝남). 같은 사건을 프로젝트마다 쓴다. `gh` 실패는 프로젝트마다
/// 따로 재고(pr.rs), 세션 복원 실패는 thread 하나의 일이다
pub(crate) fn machine_item(i: &InboxItem) -> bool {
    match i.item_kind.as_str() {
        "outage" => i.subject != "gh",
        "health" => true,
        "session" => i.event == "sessions ended",
        _ => false,
    }
}

/// 프로젝트마다 모은 알림 항목
pub(crate) type Batch = Vec<(String, Vec<InboxItem>)>;

/// 묶음에서 머신 사건을 빼 같은 종류·머신·사건은 하나만(먼저 온 것) 남긴다. 남은 프로젝트 묶음과 머신 사건 자리
pub(crate) fn split_machine(batch: Batch) -> (Batch, Option<(Place, Vec<InboxItem>)>) {
    let (mut rest, mut machine, mut projects) = (Vec::new(), Vec::<InboxItem>::new(), Vec::<String>::new());
    for (project, items) in batch {
        let (m, r): (Vec<InboxItem>, Vec<InboxItem>) = items.into_iter().partition(machine_item);
        if !m.is_empty() && !projects.contains(&project) {
            projects.push(project.clone());
        }
        for i in m {
            let same = |x: &InboxItem| x.item_kind == i.item_kind && x.subject == i.subject && x.event == i.event;
            if !machine.iter().any(same) {
                machine.push(i);
            }
        }
        if !r.is_empty() {
            rest.push((project, r));
        }
    }
    (rest, (!machine.is_empty()).then_some((Place::Machines(projects), machine)))
}

/// 사건의 고정 말. 영어 kind·event를 그대로 보이지 않는다. 대상 이름 뒤에 붙어 조사가 필요 없는 꼴로 쓴다(머신·thread
/// 이름은 받침을 알 수 없다). 모르는 사건은 None(요약을 쓴다)
fn phrase(i: &InboxItem) -> Option<&'static str> {
    Some(match (i.item_kind.as_str(), i.event.as_str()) {
        ("thread-state", "new report") => "보고 준비",
        ("thread-state", "blocked on a prompt") => "확인창 응답 기다림",
        ("thread-state", "waiting on you") => "확인 필요",
        ("thread-state", "pane closed") => "pane 닫힘",
        ("thread-state", "agent gone") => "agent 없음",
        ("thread-state", "not launched") => "시작 못 함",
        ("outage", "down") => "연결 끊김",
        ("outage", "recovered") => "다시 연결됨",
        ("outage", "failing") => "계속 실패 · PR 추적 멈춤",
        ("outage", "working again") => "다시 됨 · PR 추적 이어 감",
        ("health", "ok") => "점검 정상",
        ("health", "problem") => "점검 이상 · 새 세션 거부",
        ("session", "sessions ended") => "thread 세션 모두 끝남",
        ("pr", "PR merged") => "PR 병합됨",
        ("pr", "PR checks failing") => "PR check 실패",
        ("pr", "PR review activity") => "PR 리뷰 활동",
        ("post", "post request for user") => "사용자 결정 요청",
        ("routine", "due") => "차례",
        ("routine", "output changed") => "출력 바뀜",
        ("routine", "failed") => "실행 못 함",
        ("routine-approval", _) => "승인 필요",
        ("config-error", _) => "읽지 못함",
        ("evaluation", "evaluation") => "평가 도착",
        ("evaluation", "invalid") => "평가 기록 거부",
        _ => return None,
    })
}

/// 알림 줄의 대상 이름(`t-0012`, `mac-a`, `routine nightly`). 머신은 앞에 `machine`을 붙이지 않는다(inbox `who`와 다르다)
fn alert_who(i: &InboxItem) -> String {
    match i.item_kind.as_str() {
        "routine" | "routine-approval" => format!("routine {}", i.subject),
        _ => i.subject.clone(),
    }
}

/// 항목 한 줄(웹 push·webhook 공통): 「대상 사건 · thread 제목」(`t-0012 보고 준비 · 제목`, `mac-a 다시 연결됨`).
/// `who`가 false면 알림 제목이 이미 그 대상을 말한다(thread 하나·머신 하나 자리): 사건만 쓴다. 고정 말이 없는 사건은 요약이다
pub(crate) fn alert_line(i: &InboxItem, who: bool, titles: &HashMap<String, String>) -> String {
    let Some(p) = phrase(i) else { return safe_summary(i).chars().take(120).collect() };
    if !who {
        return p.to_string();
    }
    let mut s = format!("{} {p}", alert_who(i));
    if matches!(i.item_kind.as_str(), "thread-state" | "pr")
        && let Some(t) = titles.get(&i.subject).map(|t| t.trim()).filter(|t| !t.is_empty())
    {
        s.push_str(" · ");
        s.extend(t.chars().take(80));
    }
    s
}

/// 알림 하나의 제목·줄·누르면 갈 주소·자리(tag). 앱 이름(aaa)은 붙이지 않는다: 기기가 이미 보인다(herdr-web-ui
/// `statusMessage`·herdr-projects notify.rs 「<Project> · <thread>」).
/// - 한 thread 항목만: 「프로젝트 · thread 제목」(제목이 비면 thread ID), 자리는 그 thread, 누르면 그 thread 보기.
/// - 그 밖의 프로젝트 항목: 프로젝트 이름, 프로젝트 자리.
/// - 머신 사건: 머신 하나면 그 이름(자리도 그 머신), 여럿이면 「머신」.
pub(crate) struct AlertText {
    pub title: String,
    pub lines: Vec<String>,
    pub url: String,
    pub tag: String,
}

pub(crate) fn alert_text(place: &Place, items: &[InboxItem], titles: &HashMap<String, String>) -> AlertText {
    let lines = |who: bool| items.iter().map(|i| alert_line(i, who, titles)).collect::<Vec<_>>();
    match place {
        Place::Project(project) => {
            let one = items
                .first()
                .map(|i| i.subject.as_str())
                .filter(|s| items.iter().all(|i| i.subject == *s && i.item_kind == "thread-state"));
            match one {
                Some(t) => AlertText {
                    title: format!(
                        "{project} · {}",
                        titles.get(t).map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or(t)
                    ),
                    lines: lines(false),
                    url: format!("/#thread={}", crate::push::url_escape(&format!("{project}/{t}"))),
                    tag: format!("aaa-{project}/{t}"),
                },
                None => AlertText {
                    title: project.clone(),
                    lines: lines(true),
                    url: "/".into(),
                    tag: format!("aaa-{project}"),
                },
            }
        }
        Place::Machines(_) => {
            let one = items.first().map(|i| i.subject.as_str()).filter(|s| items.iter().all(|i| i.subject == *s));
            match one {
                Some(m) => AlertText {
                    title: m.to_string(),
                    lines: lines(false),
                    url: "/".into(),
                    tag: format!("aaa-machine-{m}"),
                },
                None => {
                    AlertText {
                        title: "머신".into(), lines: lines(true), url: "/".into(), tag: "aaa-machines".into()
                    }
                }
            }
        }
    }
}

/// webhook 글: 제목 한 줄과 항목마다 한 줄(웹 push와 같은 `alert_text`). 공개 결과물 보기 링크는 그 항목 줄 아래
pub(crate) fn notify_text(place: &Place, items: &[InboxItem], titles: &HashMap<String, String>) -> String {
    let t = alert_text(place, items, titles);
    let mut s = t.title;
    for (i, line) in items.iter().zip(t.lines) {
        s.push_str("\n- ");
        s.push_str(&line);
        if i.item_kind != "post" {
            for line in i.body.lines().filter(|line| line.starts_with("보기: https://")) {
                s.push('\n');
                s.push_str(line);
            }
        }
    }
    if s.chars().count() > TEXT_CAP {
        s = s.chars().take(TEXT_CAP).collect::<String>() + "\n…";
    }
    s
}

pub(crate) fn notify_body(
    format: &str,
    chat_id: &str,
    place: &Place,
    items: &[InboxItem],
    titles: &HashMap<String, String>,
) -> serde_json::Value {
    let text = notify_text(place, items, titles);
    match format {
        "telegram" => serde_json::json!({ "chat_id": chat_id, "text": text, "disable_web_page_preview": true }),
        _ => serde_json::json!({
            "project": match place { Place::Project(p) => Some(p.as_str()), Place::Machines(_) => None },
            "text": text,
            "items": items.iter().map(|i| serde_json::json!({
                "id": i.id, "project": i.project, "kind": i.item_kind, "subject": i.subject, "event": i.event,
                "summary": safe_summary(i),
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
            "post" => i.event == "post request for user",
            "thread-state" => crate::transitions::PUSH_EVENTS.contains(&i.event.as_str()),
            "pr" => matches!(i.event.as_str(), "PR merged" | "PR checks failing" | "PR review activity"),
            _ => true,
        }
}

/// 프로젝트 `mute`(herdr-projects notify.rs 「mute = true silences everything but errors」, 차이 목록 N10)에도 나가는 종류:
/// 오류(`gh` 실패 등 `outage`, 설정 파일 `config-error`)와 머신 이상(`health`)
pub(crate) const MUTE_PASSES: &[&str] = &["outage", "config-error", "health"];

/// 이 항목을 프로젝트 mute가 막는가
pub(crate) fn muted(mute: bool, i: &InboxItem) -> bool {
    mute && !MUTE_PASSES.contains(&i.item_kind.as_str())
}

/// 아직 알릴 만한 항목만 남긴다: 끝내지 않았고(`open`), 지난 일(`stale`: 확인한 보고, 확인 필요가 끝난 thread)이 아니다.
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

    /// 알림 줄에 쓰는 thread 제목(thread·PR 항목의 대상). 읽지 못한 thread는 ID로 남는다
    pub(crate) fn alert_titles(&self, place: &Place, items: &[InboxItem]) -> HashMap<String, String> {
        let Place::Project(project) = place else { return HashMap::new() };
        let mut out = HashMap::new();
        for i in items.iter().filter(|i| matches!(i.item_kind.as_str(), "thread-state" | "pr")) {
            if !out.contains_key(&i.subject)
                && let Ok(r) = self.load(project, &i.subject)
            {
                out.insert(i.subject.clone(), r.title);
            }
        }
        out
    }

    /// inbox 항목을 알림 대기열에 넣는다(설정에서 켜고 고른 것만 `pushes`, 프로젝트 mute면 오류·머신 이상만 `muted`)
    pub(crate) fn notify(&self, item: &InboxItem) {
        if pushes(&self.cfg.notify.item_kinds(), item)
            && !muted(self.settings(&item.project).mute, item)
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
            let window = debounce(me.cfg.notify.debounce_secs);
            let mut failing = false;
            while let Some(first) = rx.recv().await {
                // 첫 항목부터 window 동안 모은다
                let mut batch: BTreeMap<String, Vec<InboxItem>> = BTreeMap::new();
                batch.entry(first.project.clone()).or_default().push(first);
                let until = tokio::time::Instant::now() + window;
                while let Ok(Some(i)) = tokio::time::timeout_at(until, rx.recv()).await {
                    batch.entry(i.project.clone()).or_default().push(i);
                }
                let batch: Batch = batch.into_iter().map(|(p, items)| (p.clone(), me.still_news(&p, items))).collect();
                // 머신 사건은 프로젝트마다 나누지 않고 한 번(plan §8.209)
                let (rest, machine) = split_machine(batch);
                let places = rest.into_iter().map(|(p, items)| (Place::Project(p), items)).chain(machine);
                for (place, items) in places {
                    let titles = me.alert_titles(&place, &items);
                    me.push_items(&place, &items, &titles).await;
                    let n = &me.cfg.notify;
                    if n.webhook.is_empty() {
                        continue;
                    }
                    let mut items = items;
                    me.library_notice_links(&mut items);
                    let body = notify_body(&n.format, &n.telegram_chat_id, &place, &items, &titles);
                    let sent = tokio::time::timeout(SEND_TIMEOUT, post(&n.webhook, body.to_string().as_bytes())).await;
                    let res = match sent {
                        Ok(Ok(code)) if (200..300).contains(&code) => Ok(code),
                        Ok(Ok(code)) => Err(format!("HTTP {code}")),
                        Ok(Err(e)) => Err(e),
                        Err(_) => Err(format!("{}초 안에 끝나지 않았다", SEND_TIMEOUT.as_secs())),
                    };
                    for project in place.projects() {
                        match &res {
                            Ok(_) => me.flow_project(project, "notify_sent", None, format!("항목 {}개", items.len())),
                            Err(e) => me.flow_project(project, "notify_failed", None, e.clone()),
                        }
                    }
                    match res {
                        Ok(_) => failing = false,
                        Err(e) => {
                            if !failing {
                                tracing::warn!(projects = ?place.projects(), "알림을 보내지 못했다(다시 보내지 않는다): {e}");
                            }
                            failing = true;
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

    /// 프로젝트 mute는 오류·머신 이상만 남긴다(herdr-projects `mute`, N10)
    #[test]
    fn mute_keeps_errors_only() {
        let mut i = item("t-0001", "new report");
        assert!(!muted(false, &i) && muted(true, &i));
        for k in ["outage", "config-error", "health"] {
            i.item_kind = k.into();
            assert!(!muted(true, &i), "{k}");
        }
        for k in ["pr", "routine", "session"] {
            i.item_kind = k.into();
            assert!(muted(true, &i), "{k}");
        }
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
        let demo = Place::Project("demo".into());
        let titles: HashMap<String, String> = [("t-0001".to_string(), "웹 결과물".to_string())].into();
        let t = notify_body("telegram", "-100", &demo, &items, &titles);
        assert_eq!(t["chat_id"], "-100");
        // 웹 push와 같은 제목·줄: 앱 이름·영어 사건을 붙이지 않는다
        assert_eq!(t["text"], "demo\n- t-0001 보고 준비 · 웹 결과물\n- t-0002 확인창 응답 기다림");
        // 한 thread면 제목이 그 thread다
        let one = notify_body("telegram", "-100", &demo, &items[..1], &titles);
        assert_eq!(one["text"], "demo · 웹 결과물\n- 보고 준비");
        let j = notify_body("json", "", &demo, &items, &titles);
        assert_eq!(
            (j["project"].as_str(), j["items"][1]["event"].as_str(), j["items"][1]["project"].as_str()),
            (Some("demo"), Some("blocked on a prompt"), Some("demo"))
        );
        let mut down = item("mac-a", "down");
        down.item_kind = "outage".into();
        let m = notify_body("json", "", &Place::Machines(vec!["demo".into()]), &[down], &titles);
        assert_eq!((m["project"].clone(), m["text"].as_str()), (serde_json::Value::Null, Some("mac-a\n- 연결 끊김")));
        let many: Vec<_> = (0..100).map(|n| item(&format!("t-{n:04}"), "new report")).collect();
        assert!(notify_text(&demo, &many, &titles).chars().count() <= TEXT_CAP + 2);
    }

    /// 사건 한 줄은 고정 말로(영어 kind·event를 앞에 붙이지 않는다). 대상 이름 뒤에 붙는 말이라 조사가 없다: 받침을 알 수 없는
    /// 머신 이름에 「가」를 붙이던 「머신 mac-1가」 같은 틀린 조사가 생기지 않는다. 모르는 사건은 요약이다
    #[test]
    fn alert_lines() {
        let titles: HashMap<String, String> = [("t-0012".to_string(), "휴대폰 알림".to_string())].into();
        let line = |kind: &str, subject: &str, event: &str| {
            let mut i = item(subject, event);
            i.item_kind = kind.into();
            (alert_line(&i, true, &titles), alert_line(&i, false, &titles))
        };
        let cases = [
            (("thread-state", "t-0012", "new report"), "t-0012 보고 준비 · 휴대폰 알림", "보고 준비"),
            (("thread-state", "t-0013", "pane closed"), "t-0013 pane 닫힘", "pane 닫힘"),
            (("outage", "mac-1", "recovered"), "mac-1 다시 연결됨", "다시 연결됨"),
            (("outage", "mac-1", "down"), "mac-1 연결 끊김", "연결 끊김"),
            (("outage", "gh", "failing"), "gh 계속 실패 · PR 추적 멈춤", "계속 실패 · PR 추적 멈춤"),
            (("session", "mac-a", "sessions ended"), "mac-a thread 세션 모두 끝남", "thread 세션 모두 끝남"),
            (("pr", "t-0012", "PR merged"), "t-0012 PR 병합됨 · 휴대폰 알림", "PR 병합됨"),
            (("routine", "nightly", "due"), "routine nightly 차례", "차례"),
            (("post", "t-0012", "post request for user"), "t-0012 사용자 결정 요청", "사용자 결정 요청"),
            (("thread-state", "t-0012", "idle"), "사본 complete", "사본 complete"),
        ];
        for ((kind, subject, event), with_who, without) in cases {
            assert_eq!(line(kind, subject, event), (with_who.to_string(), without.to_string()), "{kind} {event}");
        }
        for (_, a, b) in cases {
            for s in [a, b] {
                assert!(!s.contains("machine ") && !s.contains("recovered") && !s.contains("new report"), "{s}");
            }
        }
    }

    /// 머신 사건은 그 머신에 thread가 열린 프로젝트마다 항목이 생기지만 밖으로는 한 번이다. 프로젝트 항목은 그대로 남고,
    /// `gh` 실패(프로젝트마다 따로 잰다)와 세션 복원 실패(thread 하나의 일)는 머신 사건이 아니다
    #[test]
    fn machine_items_go_out_once() {
        let it = |project: &str, kind: &str, subject: &str, event: &str| {
            let mut i = item(subject, event);
            (i.project, i.item_kind, i.id) = (project.into(), kind.into(), format!("{project}-{subject}-{event}"));
            i
        };
        let batch = vec![
            (
                "demo".to_string(),
                vec![
                    it("demo", "thread-state", "t-0001", "new report"),
                    it("demo", "outage", "mac-a", "recovered"),
                    it("demo", "outage", "mac-b", "recovered"),
                    it("demo", "outage", "gh", "failing"),
                ],
            ),
            (
                "side".to_string(),
                vec![it("side", "outage", "mac-a", "recovered"), it("side", "outage", "mac-b", "recovered")],
            ),
            (
                "third".to_string(),
                vec![it("third", "health", "mac-a", "problem"), it("third", "session", "mac-a", "restore failed")],
            ),
        ];
        let (rest, machine) = split_machine(batch);
        let ids = |v: &[InboxItem]| v.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!(rest.len(), 2);
        assert_eq!(
            (rest[0].0.as_str(), ids(&rest[0].1)),
            ("demo", vec!["demo-t-0001-new report".into(), "demo-gh-failing".into()])
        );
        assert_eq!((rest[1].0.as_str(), ids(&rest[1].1)), ("third", vec!["third-mac-a-restore failed".to_string()]));
        let (place, items) = machine.unwrap();
        assert_eq!(place, Place::Machines(vec!["demo".into(), "side".into(), "third".into()]));
        assert_eq!(ids(&items), ["demo-mac-a-recovered", "demo-mac-b-recovered", "third-mac-a-problem"]);
        assert_eq!(
            alert_text(&place, &items, &HashMap::new()).lines,
            ["mac-a 다시 연결됨", "mac-b 다시 연결됨", "mac-a 점검 이상 · 새 세션 거부"]
        );
        // 머신 사건이 없으면 머신 자리도 없다
        let (rest, machine) =
            split_machine(vec![("demo".into(), vec![it("demo", "thread-state", "t-0001", "new report")])]);
        assert!(machine.is_none() && rest.len() == 1);
    }

    /// 실제 https(webpki 루트) 확인: `cargo test -p aaa-host notify_https -- --ignored`(네트워크 필요)
    #[tokio::test]
    #[ignore = "실제 외부 서버(example.com·badssl.com·Apple push)에 요청한다: 손으로만 돌린다(ci-local·CI는 --ignored를 쓰지 않는다)"]
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

    /// 받은 webhook 요청 본문(JSON)을 차례로 넘기는 loopback 서버
    async fn hook_server() -> (String, mpsc::UnboundedReceiver<serde_json::Value>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", l.local_addr().unwrap());
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let mut req = Vec::new();
                let mut b = [0u8; 4096];
                // 머리 뒤 Content-Length만큼 읽는다
                let body = loop {
                    let n = s.read(&mut b).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    req.extend_from_slice(&b[..n]);
                    let text = String::from_utf8_lossy(&req).to_string();
                    if let Some((head, body)) = text.split_once("\r\n\r\n") {
                        let len: usize = head
                            .lines()
                            .find_map(|l| l.strip_prefix("Content-Length: "))
                            .and_then(|v| v.trim().parse().ok())
                            .unwrap();
                        if body.len() >= len {
                            break body.to_string();
                        }
                    }
                };
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").await.unwrap();
                let _ = tx.send(serde_json::from_str(&body).unwrap());
            }
        });
        (url, rx)
    }

    /// 머신 사건은 그 머신에 thread가 열린 프로젝트마다 inbox 항목이 생기지만(프로젝트마다 그대로 남는다) 밖 알림은 한 통이다.
    /// 「한 통뿐」은 뒤따르는 알림(울타리)이 다음 요청인 것으로 본다: 프로젝트마다 보냈다면 둘째 프로젝트 것이 먼저 온다
    #[tokio::test]
    async fn machine_event_goes_out_once_across_projects() {
        let (url, mut rx) = hook_server().await;
        let state = std::env::temp_dir().join(format!("aaa-notify-once-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.notify.webhook = url;
        // 모으는 창 0: 이 시험의 항목은 notifier task가 돌기 전에 모두 대기열에 있다(current_thread runtime)
        cfg.notify.debounce_secs = 0;
        cfg.projects.root = state.join("projects").display().to_string();
        let h = Host::open(cfg, state.clone()).unwrap();
        for p in ["demo", "side"] {
            h.project_create(p, Default::default(), "test").unwrap();
        }
        h.start_notifier();
        for p in ["demo", "side"] {
            h.inbox_add(p, "outage", "mac-a", "recovered", "머신 mac-a: 다시 연결됐다");
        }
        let got = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.unwrap().unwrap();
        assert_eq!(got["project"], serde_json::Value::Null, "{got}");
        assert_eq!(got["text"], "mac-a\n- 다시 연결됨", "{got}");
        assert_eq!(got["items"].as_array().unwrap().len(), 1, "{got}");
        // 두 프로젝트의 inbox 항목은 그대로다
        for p in ["demo", "side"] {
            assert_eq!(h.db.inbox_list(Some(p), false).unwrap().len(), 1, "{p}");
        }
        h.inbox_add("demo", "thread-state", "t-0009", "new report", "보고");
        let next = tokio::time::timeout(Duration::from_secs(10), rx.recv()).await.unwrap().unwrap();
        assert_eq!(
            (next["project"].as_str(), next["text"].as_str()),
            (Some("demo"), Some("demo · t-0009\n- 보고 준비")),
            "{next}"
        );
        drop(h);
        let _ = std::fs::remove_dir_all(&state);
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
