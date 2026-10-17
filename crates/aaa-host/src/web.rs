//! host 웹(기획서 §6.3·§6.4). `127.0.0.1:<web_port>`의 HTTP·WS로만 열고, 다른 기기에는 `tailscale serve` HTTPS로 공개한다.
//!
//! - 입장: aaa pairing 코드 → 장치 token cookie(HttpOnly·SameSite=Strict, HTTPS면 Secure·`__Host-`). loopback은 인증 신호가 아니다.
//! - Host 헤더 허용 목록(DNS rebinding), 변경 요청·WebSocket의 Origin 검사, CSP.
//! - `/api/v1/call`은 Front API 요청을 그대로 받는다(같은 타입, 같은 권한 판정).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aaa_proto::{
    B64, DeviceRole, ErrorBody, ErrorCode, FrontEvent, FrontRequest, Outcome, TermClientMsg, TermServerMsg,
};
use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;

use super::{FrontRef, Host, PaneEvent};
use crate::output::{OUTPUT_HARD_BYTES, OUTPUT_STALLED_CLOSE_CODE, OutputWindow, Verdict, check_unacked};

mod assets {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; \
                   connect-src 'self'; font-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

pub async fn start(host: &Arc<Host>) -> Result<()> {
    let addr = host.cfg.web.listen.clone();
    if addr.is_empty() {
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind(&addr).await.with_context(|| format!("웹 listen {addr}"))?;
    host.web_port.store(listener.local_addr()?.port(), std::sync::atomic::Ordering::Relaxed);
    *host.web_listen.lock().unwrap() = Some(listener.local_addr()?.to_string());
    tracing::info!(%addr, "웹 listen(loopback)");
    let app = router(host.clone());
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("웹 서버 끝: {e}");
        }
    });
    Ok(())
}

pub fn router(host: Arc<Host>) -> Router {
    Router::new()
        .route("/api/v1/pair", post(pair))
        .route("/api/v1/me", get(me))
        .route("/api/v1/call", post(call))
        .route("/api/v1/events", get(events_ws))
        .route("/api/v1/push", get(push_info))
        .route("/api/v1/push/subscribe", post(push_subscribe))
        .route("/api/v1/push/unsubscribe", post(push_unsubscribe))
        .route("/api/v1/push/test", post(push_test))
        .route("/api/v1/diag/input", post(diag_input))
        .route("/api/v1/usage", get(usage_report))
        .route("/api/v1/panes/{id}/ws", get(pane_ws))
        .route("/v/{cap}/{*path}", get(view_file))
        .fallback(get(static_file))
        .layer(middleware::from_fn_with_state(host.clone(), guard))
        .with_state(host)
}

fn host_header(h: &HeaderMap) -> String {
    h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
}

fn is_loopback_host(host: &str) -> bool {
    host.starts_with("127.0.0.1:") || host.starts_with("localhost:") || host.starts_with("[::1]:")
}

/// 요청이 HTTPS(serve) 경유인가: 브라우저가 보는 주소가 loopback이 아니거나 proxy가 https라고 알린다.
fn via_https(h: &HeaderMap) -> bool {
    let proto = h.get("x-forwarded-proto").and_then(|v| v.to_str().ok());
    proto == Some("https") || !is_loopback_host(&host_header(h))
}

fn origin_allowed(host: &Host, origin: &str) -> bool {
    let port = host.web_port();
    if origin == format!("http://127.0.0.1:{port}") || origin == format!("http://localhost:{port}") {
        return true;
    }
    let Some(rest) = origin.strip_prefix("https://") else { return false };
    let name = rest.rsplit_once(':').map(|(h, _)| h).unwrap_or(rest);
    host.db.meta("publish_dns").ok().flatten().is_some_and(|d| d.eq_ignore_ascii_case(name))
}

async fn guard(State(host): State<Arc<Host>>, req: Request, next: Next) -> Response {
    let h = req.headers();
    let hh = host_header(h);
    if !host.host_allowed(&hh) {
        return (StatusCode::MISDIRECTED_REQUEST, "알 수 없는 Host").into_response();
    }
    let is_ws =
        h.get(header::UPGRADE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if !matches!(*req.method(), Method::GET | Method::HEAD) || is_ws {
        let origin = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !origin_allowed(&host, origin) {
            return (StatusCode::FORBIDDEN, "허용하지 않은 Origin").into_response();
        }
    }
    let mut res = next.run(req).await;
    let hs = res.headers_mut();
    // 결과물 보기(/v/)는 자기 CSP(sandbox)·frame 정책을 둔다
    hs.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static(CSP));
    hs.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    hs.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    hs.entry(header::X_FRAME_OPTIONS).or_insert(HeaderValue::from_static("DENY"));
    res
}

/// 결과물: opaque origin(sandbox)으로 보여 제어 UI의 cookie·API에 닿지 못하게 한다. 같은 host의 웹 화면 안 iframe에만 넣을 수 있다.
const VIEW_CSP: &str = "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox allow-downloads; \
                        default-src 'self' 'unsafe-inline' data: blob:; frame-ancestors 'self'";

/// `/v/<cap>/<path>`: 보기 경로가 곧 권한이다(그 thread 결과물 읽기만, 2시간). host 사본이 없으면 agent에서 스트리밍한다.
async fn view_file(State(host): State<Arc<Host>>, Path((cap, path)): Path<(String, String)>) -> Response {
    let src = match host.view_source(&cap, &path).await {
        Ok(s) => s,
        Err(e) => {
            let code = match e.code {
                ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
                ErrorCode::NotFound => StatusCode::NOT_FOUND,
                ErrorCode::BadRequest => StatusCode::BAD_REQUEST,
                _ => StatusCode::BAD_GATEWAY,
            };
            return (code, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], e.message).into_response();
        }
    };
    let size = src.size();
    let body = match src {
        crate::library::ViewSource::Local(p, _) => match tokio::fs::File::open(&p).await {
            Ok(f) => Body::from_stream(futures_util::stream::unfold(f, |mut f| async move {
                use tokio::io::AsyncReadExt;
                let mut buf = vec![0u8; 256 * 1024];
                match f.read(&mut buf).await {
                    Ok(0) => None,
                    Ok(n) => {
                        buf.truncate(n);
                        Some((Ok::<_, std::io::Error>(buf), f))
                    }
                    Err(e) => Some((Err(e), f)),
                }
            })),
            Err(e) => return (StatusCode::NOT_FOUND, format!("host 사본을 열지 못했다: {e}")).into_response(),
        },
        crate::library::ViewSource::Agent { conn, work_dir, path: p, first, eof, .. } => {
            Body::from_stream(crate::library::agent_stream(conn, work_dir, p, first, eof))
        }
    };
    Response::builder()
        .header(header::CONTENT_TYPE, crate::library::content_type(&path))
        .header(header::CONTENT_LENGTH, size)
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::CONTENT_SECURITY_POLICY, VIEW_CSP)
        .header(header::X_FRAME_OPTIONS, "SAMEORIGIN")
        .body(body)
        .unwrap()
}

fn cookie_token(h: &HeaderMap) -> Option<String> {
    for v in h.get_all(header::COOKIE) {
        let Ok(s) = v.to_str() else { continue };
        for part in s.split(';') {
            let (k, val) = part.trim().split_once('=')?;
            if k == "__Host-aaa" || k == "aaa" {
                return Some(val.to_string());
            }
        }
    }
    None
}

fn device(host: &Host, h: &HeaderMap) -> Option<(String, DeviceRole)> {
    host.device_role(&cookie_token(h)?)
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Unauthorized, "pairing이 필요하다"))))
        .into_response()
}

#[derive(Deserialize)]
struct PairBody {
    code: String,
}

#[derive(Serialize)]
struct Me {
    role: DeviceRole,
    device: String,
}

async fn pair(State(host): State<Arc<Host>>, headers: HeaderMap, Json(b): Json<PairBody>) -> Response {
    match host.redeem_pairing(&b.code) {
        Ok((token, role)) => {
            let secure = via_https(&headers);
            let cookie = if secure {
                format!("__Host-aaa={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=31536000")
            } else {
                format!("aaa={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000")
            };
            let mut res = Json(Outcome::Ok(serde_json::json!({ "role": role }))).into_response();
            res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
            res
        }
        Err(e) => (StatusCode::UNAUTHORIZED, Json(Outcome::<()>::Err(e))).into_response(),
    }
}

async fn me(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    match device(&host, &headers) {
        Some((id, role)) => Json(Outcome::Ok(Me { role, device: id })).into_response(),
        None => unauthorized(),
    }
}

async fn call(State(host): State<Arc<Host>>, headers: HeaderMap, Json(req): Json<FrontRequest>) -> Response {
    let Some((id, role)) = device(&host, &headers) else { return unauthorized() };
    let result = host.call(role, &format!("device:{id}"), req).await;
    Json(Outcome::from(result)).into_response()
}

fn bad_request(msg: impl Into<String>) -> Response {
    (StatusCode::BAD_REQUEST, Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::BadRequest, msg)))).into_response()
}

#[derive(Serialize)]
struct PushInfo {
    /// host 설정에서 Web Push가 켜져 있다
    enabled: bool,
    /// `applicationServerKey`(VAPID 공개키, base64url)
    key: Option<String>,
    /// 이 장치의 구독 수
    subscribed: usize,
}

/// Web Push(§8.26): 키와 이 장치의 구독 상태. 보기 권한 장치도 알림은 받는다.
async fn push_info(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let subscribed = host.db.push_list(Some(&id)).map(|v| v.len()).unwrap_or(0);
    let key = host.vapid.as_ref().map(|v| v.public_b64());
    Json(Outcome::Ok(PushInfo { enabled: key.is_some(), key, subscribed })).into_response()
}

#[derive(Deserialize)]
struct PushKeys {
    p256dh: String,
    auth: String,
}

/// 브라우저 `PushSubscription.toJSON()` 모양
#[derive(Deserialize)]
struct PushSubBody {
    endpoint: String,
    keys: PushKeys,
}

async fn push_subscribe(State(host): State<Arc<Host>>, headers: HeaderMap, Json(b): Json<PushSubBody>) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    if host.vapid.is_none() {
        return bad_request("host 설정에서 Web Push가 꺼져 있다(notify.web_push)");
    }
    if let Err(e) = crate::push::check_endpoint(&b.endpoint, &host.cfg.notify.push_hosts)
        .and_then(|_| crate::push::check_keys(&b.keys.p256dh, &b.keys.auth))
    {
        return bad_request(e);
    }
    let sub = crate::db::PushSub { endpoint: b.endpoint, p256dh: b.keys.p256dh, auth: b.keys.auth, device: id.clone() };
    if let Err(e) = host.db.push_put(&sub) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Internal, e.to_string()))),
        )
            .into_response();
    }
    // endpoint는 남기지 않는다(그 기기에 보내는 열쇠다)
    host.db.audit(&format!("device:{id}"), "push_subscribe", "");
    Json(Outcome::Ok(serde_json::json!({ "subscribed": true }))).into_response()
}

#[derive(Deserialize)]
struct PushEndpoint {
    endpoint: String,
}

async fn push_unsubscribe(State(host): State<Arc<Host>>, headers: HeaderMap, Json(b): Json<PushEndpoint>) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let removed = host.db.push_del(&b.endpoint, Some(&id)).unwrap_or(false);
    if removed {
        host.db.audit(&format!("device:{id}"), "push_unsubscribe", "");
    }
    Json(Outcome::Ok(serde_json::json!({ "removed": removed }))).into_response()
}

/// 이 장치의 구독에 시험 알림을 보낸다
async fn push_test(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let subs = host.db.push_list(Some(&id)).unwrap_or_default();
    let body =
        serde_json::json!({ "title": "aaa", "body": "알림이 켜졌다", "url": "/", "tag": "aaa-test" }).to_string();
    let (mut sent, mut failed) = (0, Vec::new());
    for s in &subs {
        match host.push_one(s, body.as_bytes()).await {
            crate::push::Sent::Ok => sent += 1,
            crate::push::Sent::Gone => failed.push("브라우저가 구독을 버렸다(지웠다)".to_string()),
            crate::push::Sent::Failed(e) => failed.push(e),
        }
    }
    Json(Outcome::Ok(serde_json::json!({ "sent": sent, "failed": failed }))).into_response()
}

#[derive(Deserialize)]
struct UsageQuery {
    refresh: Option<String>,
}

/// 이 host 머신의 CLI가 로그인한 구독의 한도(`usage.rs`). 보기 권한(reader) 기기도 본다(참고: 들어온 client 누구나).
/// 계정 이름(email)이 들어 있으니 저장하지 않게 한다.
async fn usage_report(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<UsageQuery>,
) -> Response {
    if device(&host, &headers).is_none() {
        return unauthorized();
    }
    let report = host.usage.report(q.refresh.as_deref() == Some("1")).await;
    let mut res = Json(Outcome::Ok(report)).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// 입력 사건 기록 하나의 상한과 남기는 개수
const DIAG_MAX: usize = 1 << 20;
const DIAG_KEEP: usize = 20;

/// 실기 입력 사건 기록(web/src/inputTrace.ts)을 host 상태 폴더 `diag/`에 남긴다. iPad 화면 키보드 같은 OS 입력기의
/// 사건 순서는 headless로 만들 수 없어 실기에서 모아 E2E 재생으로 옮긴다(herdr-web-ui terminal-ime-diagnostic과 같은 목적).
async fn diag_input(State(host): State<Arc<Host>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    if body.len() > DIAG_MAX {
        return bad_request(format!("기록은 {}KiB까지", DIAG_MAX >> 10));
    }
    if serde_json::from_slice::<serde_json::Value>(&body).is_err() {
        return bad_request("JSON이 아니다");
    }
    let dir = host.state.join("diag");
    let name = format!("input-{}.json", aaa_common::util::now_ms());
    let saved = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(dir.join(&name), &body));
    if let Err(e) = saved {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Internal, e.to_string()))),
        )
            .into_response();
    }
    // 오래된 것부터 지워 DIAG_KEEP개만 둔다(이름이 시각 순이다)
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut names: Vec<_> = rd
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("input-") && n.ends_with(".json"))
            .collect();
        names.sort();
        let extra = names.len().saturating_sub(DIAG_KEEP);
        for n in &names[..extra] {
            let _ = std::fs::remove_file(dir.join(n));
        }
    }
    host.db.audit(&format!("device:{id}"), "diag_input", &name);
    Json(Outcome::Ok(serde_json::json!({ "saved": name }))).into_response()
}

async fn events_ws(State(host): State<Arc<Host>>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    let Some((_, role)) = device(&host, &headers) else { return unauthorized() };
    ws.on_upgrade(move |socket| events_loop(host, role, socket))
}

/// 머신·pane 목록(바뀔 때마다 전체)과 확인창 질문·thread 사건. 질문·thread는 연결한 뒤 목록을 한 번 읽고 이 사건으로 따라간다.
async fn events_loop(host: Arc<Host>, role: DeviceRole, mut socket: WebSocket) {
    let mut changed = host.changed.subscribe();
    let mut questions = host.question_events.subscribe();
    let mut threads = host.thread_events.subscribe();
    let mut send_state = true;
    loop {
        if send_state {
            let ev = FrontEvent::State { machines: host.machine_views(), panes: host.pane_views(role) };
            if socket.send(Message::Text(serde_json::to_string(&ev).unwrap().into())).await.is_err() {
                break;
            }
        }
        tokio::select! {
            r = changed.changed() => {
                if r.is_err() { break }
                send_state = true;
            }
            q = questions.recv() => {
                send_state = false;
                match q {
                    Ok(q) => {
                        let text = serde_json::to_string(&FrontEvent::Question(q)).unwrap();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    // 놓쳤으면 끊는다. 웹은 다시 붙으며 질문 목록을 새로 읽는다.
                    Err(_) => break,
                }
            }
            t = threads.recv() => {
                send_state = false;
                match t {
                    Ok(t) => {
                        let text = serde_json::to_string(&FrontEvent::Thread(t)).unwrap();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            m = socket.recv() => {
                send_state = false;
                match m {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

/// pane WebSocket 주소의 선택. `flow=ack`면 출력 ACK를 보내는 화면이다(herdr-web-ui attach `flow_control: "ack"`).
#[derive(Deserialize)]
struct PaneWsQuery {
    flow: Option<String>,
}

async fn pane_ws(
    State(host): State<Arc<Host>>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<PaneWsQuery>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some((dev, role)) = device(&host, &headers) else { return unauthorized() };
    if host.pane_view_of(&id, role).is_none() {
        return (StatusCode::NOT_FOUND, "pane이 없다").into_response();
    }
    // 그림 조각(512KiB, base64)이 들어갈 만큼만 받는다
    // ACK를 하지 않는 화면(다시 띄우기 전에 받은 옛 웹)은 보내기 줄로만 잰다
    let acks = q.flow.as_deref() == Some("ack");
    ws.max_message_size(1 << 20).on_upgrade(move |socket| pane_loop(host, id, dev, role, acks, socket))
}

/// 한 터미널 연결의 보내기 줄. frame은 쓰기 task가 차례로 보내고, `buffered`는 아직 보내지 못한 바이트다
/// (herdr-web-ui가 보는 WebSocket `getBufferedAmount`). 그래서 느린 브라우저가 이 연결의 사건 처리를 막지 않고,
/// 따라오는지는 ACK와 이 줄로 잰다(plan §8.62).
struct Outbox {
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    buffered: Arc<AtomicU64>,
    stop: tokio::sync::watch::Sender<bool>,
}

fn frame_len(m: &Message) -> u64 {
    match m {
        Message::Text(t) => t.len() as u64,
        Message::Binary(b) => b.len() as u64,
        _ => 0,
    }
}

impl Outbox {
    fn new(mut sink: futures_util::stream::SplitSink<WebSocket, Message>) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
        let buffered = Arc::new(AtomicU64::new(0));
        let (stop, mut stopped) = tokio::sync::watch::channel(false);
        let left = buffered.clone();
        tokio::spawn(async move {
            loop {
                // 연결이 그냥 끝나 보내는 쪽이 사라진 것(Err)은 정지가 아니다: 4008 없이 끝낸다
                let m = tokio::select! {
                    biased;
                    r = stopped.changed() => if r.is_ok() { break } else { return },
                    m = rx.recv() => match m { Some(m) => m, None => return },
                };
                let n = frame_len(&m);
                // 막힌 보내기도 닫기가 끊는다(보내던 frame은 tungstenite 버퍼에 통째로 있어 흐름이 깨지지 않는다)
                let ok = tokio::select! {
                    biased;
                    r = stopped.changed() => if r.is_ok() { break } else { return },
                    r = sink.send(m) => r.is_ok(),
                };
                left.fetch_sub(n, Ordering::Relaxed);
                if !ok {
                    return;
                }
            }
            // 따라오지 못한 연결: 남은 줄은 버리고 4008로 닫는다. 그 닫기도 못 가면 연결을 버린다.
            let close = Message::Close(Some(CloseFrame {
                code: OUTPUT_STALLED_CLOSE_CODE,
                reason: "terminal output consumer stalled".into(),
            }));
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), sink.send(close)).await;
        });
        Self { tx, buffered, stop }
    }

    fn send(&self, m: Message) -> bool {
        self.buffered.fetch_add(frame_len(&m), Ordering::Relaxed);
        self.tx.send(m).is_ok()
    }

    fn msg(&self, m: &TermServerMsg) -> bool {
        self.send(Message::Text(serde_json::to_string(m).unwrap().into()))
    }

    fn buffered(&self) -> u64 {
        self.buffered.load(Ordering::Relaxed)
    }

    /// 출력을 따라오지 못했다: 4008로 닫는다. 다시 보내지 않는다(브라우저의 「다시 연결」이 새로 붙는다).
    fn stop(&self, why: &str) {
        tracing::info!(why, "웹 터미널 출력 정지(4008)");
        let _ = self.stop.send(true);
    }
}

/// snapshot 하나가 새 출력 구독이다: 새 번호의 창을 열고, replay가 느린 연결을 건널 시간을 준다(참고 `join`)
fn open_stream(
    out: &Outbox,
    streams: &mut u32,
    replay: Vec<u8>,
    cols: u16,
    rows: u16,
    readonly: bool,
    now: u64,
) -> Option<OutputWindow> {
    *streams += 1;
    let mut window = OutputWindow::new(*streams);
    let bytes = replay.len() as u64;
    let text =
        serde_json::to_string(&TermServerMsg::Snapshot { replay: B64(replay), cols, rows, readonly, stream: *streams })
            .unwrap();
    if bytes > 0 {
        let wire = text.len() as u64;
        window.join(wire, out.buffered(), now);
        window.write(bytes, wire);
    }
    out.send(Message::Text(text.into())).then_some(window)
}

/// 입력할 수 있는지와 그 이유(끊김 > 종료 > 권한 순)
fn readonly_msg(view: &aaa_proto::PaneView, role: DeviceRole) -> TermServerMsg {
    let reason = if view.connection == aaa_proto::Connection::Disconnected {
        Some("agent와 연결이 끊겼다(읽기 전용)")
    } else if !view.state.is_running() {
        Some("세션이 끝났다")
    } else if !role.can_control() {
        Some("보기 권한")
    } else {
        None
    };
    TermServerMsg::Readonly { readonly: view.readonly, reason: reason.map(str::to_string) }
}

/// WebSocket 하나가 대화형 front 하나다. 「지금 쓰는 화면」(lease)은 이 연결에 묶이고 끝나면 풀린다.
async fn pane_loop(host: Arc<Host>, id: String, dev: String, role: DeviceRole, acks: bool, socket: WebSocket) {
    let short: String = dev.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let front = host.new_front(format!("웹({short})"));
    pane_session(&host, &id, &dev, role, &front, acks, socket).await;
    host.front_gone(&front.id);
}

async fn pane_session(
    host: &Arc<Host>,
    id: &str,
    dev: &str,
    role: DeviceRole,
    front: &FrontRef,
    acks: bool,
    socket: WebSocket,
) {
    let id = id.to_string();
    let actor = format!("device:{dev}");
    let Some((mut rx, replay, mut next, cols, rows)) = host.attach_pane(&id) else { return };
    let mut upload: Option<crate::attach::Upload> = None;
    // 첨부가 실패하면 그 첨부의 남은 조각은 오류 없이 버린다(다음 첨부는 offset 0으로 시작한다)
    let mut upload_failed = false;
    let Some(view) = host.pane_view_of(&id, role) else { return };
    let (sink, mut stream) = socket.split();
    let out = Outbox::new(sink);
    let started = std::time::Instant::now();
    let now = || started.elapsed().as_millis() as u64;
    let mut streams = 0;
    if !out.msg(&TermServerMsg::Front { id: front.id.clone() }) {
        return;
    }
    let Some(mut window) = open_stream(&out, &mut streams, replay, cols, rows, view.readonly, now()) else { return };
    if !out.msg(&readonly_msg(&view, role)) || !out.msg(&TermServerMsg::State { pane: view }) {
        return;
    }
    // ACK가 아예 오지 않아도(가려진 탭) 정지를 잰다(참고 outputTimer 100ms)
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut queue_since = None;
    loop {
        let verdict = if acks {
            window.check(out.buffered(), now())
        } else {
            check_unacked(out.buffered(), &mut queue_since, now())
        };
        if verdict == Verdict::Stop {
            out.stop("ACK가 나아가지 않는다");
            break;
        }
        tokio::select! {
            _ = tick.tick() => {}
            ev = rx.recv() => {
                // 빈틈·지연(Lagged)·agent 재구독(Reset)은 host 안의 일이라 snapshot으로 다시 맞춘다. 브라우저가
                // 느린 것은 위 창이 잰다: 그때는 다시 보내지 않고 닫는다(참고: 과부하는 자동 replay하지 않는다).
                let resync = match ev {
                    Ok(PaneEvent::Output(start, data)) => {
                        let end = start + data.len() as u64;
                        if end <= next { false }
                        else if start > next { true }
                        else {
                            let part = data[(next - start) as usize..].to_vec();
                            next = end;
                            let n = part.len() as u64;
                            if (acks && window.pending() + n > OUTPUT_HARD_BYTES) || out.buffered() >= OUTPUT_HARD_BYTES {
                                out.stop("밀린 출력이 상한을 넘었다");
                                break;
                            }
                            window.write(n, n);
                            if !out.send(Message::Binary(part.into())) { break; }
                            false
                        }
                    }
                    Ok(PaneEvent::Reset) | Err(RecvError::Lagged(_)) => true,
                    Ok(PaneEvent::State) => {
                        let Some(view) = host.pane_view_of(&id, role) else { break };
                        if !out.msg(&readonly_msg(&view, role)) || !out.msg(&TermServerMsg::State { pane: view }) { break; }
                        false
                    }
                    Err(RecvError::Closed) => break,
                };
                if resync {
                    let Some((nrx, replay, offset, cols, rows)) = host.attach_pane(&id) else { break };
                    rx = nrx;
                    next = offset;
                    let readonly = host.pane_view_of(&id, role).map(|p| p.readonly).unwrap_or(true);
                    let Some(w) = open_stream(&out, &mut streams, replay, cols, rows, readonly, now()) else { break };
                    window = w;
                }
            }
            m = stream.next() => {
                let req = match m {
                    Some(Ok(Message::Text(t))) => match serde_json::from_str::<TermClientMsg>(&t) {
                        Ok(TermClientMsg::Ack { stream, offset }) => {
                            // 앞 구독(snapshot 전)의 늦은 ACK는 그냥 버린다
                            if stream == window.id && !window.acknowledge(stream, offset) {
                                let e = ErrorBody::new(ErrorCode::BadRequest, "ACK offset이 보낸 출력을 넘는다");
                                if !out.msg(&TermServerMsg::Error(e)) { break; }
                            }
                            None
                        }
                        Ok(TermClientMsg::Input { data }) => Some(FrontRequest::PaneInput { pane: id.clone(), data: B64(data.into_bytes()) }),
                        Ok(TermClientMsg::Resize { cols, rows }) if role.can_control() => {
                            Some(FrontRequest::PaneResize { pane: id.clone(), cols: cols.clamp(10, 500), rows: rows.clamp(3, 300) })
                        }
                        Ok(TermClientMsg::Take) => Some(FrontRequest::PaneTake { pane: id.clone() }),
                        Ok(TermClientMsg::Release) => Some(FrontRequest::PaneRelease { pane: id.clone() }),
                        Ok(TermClientMsg::Attach { offset, total, sha256, data }) => {
                            if offset != 0 && upload_failed {
                                continue;
                            }
                            let reply = match host.attach_chunk(role, &actor, front, &id, &mut upload, offset, total, &sha256, data.0).await {
                                Ok(None) => None,
                                Ok(Some(a)) => Some(TermServerMsg::Attached { path: a.path, inserted: a.inserted, note: a.note }),
                                Err(e) => Some(TermServerMsg::Error(e)),
                            };
                            upload_failed = matches!(reply, Some(TermServerMsg::Error(_)));
                            if let Some(r) = reply && !out.msg(&r) {
                                break;
                            }
                            None
                        }
                        _ => None,
                    },
                    Some(Ok(Message::Binary(b))) => Some(FrontRequest::PaneInput { pane: id.clone(), data: B64(b.to_vec()) }),
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => None,
                };
                if let Some(req) = req
                    && let Err(e) = host.call_from(role, &actor, Some(front), req).await
                    && !out.msg(&TermServerMsg::Error(e))
                {
                    break;
                }
            }
        }
    }
}

/// 웹 자산의 Content-Type. 확장자는 마지막 경로 조각에서만 본다(`fonts/meslotendard-0.3.1/LICENSE`의 버전 점을
/// 확장자로 읽지 않게). 글꼴 고지(LICENSE·.txt·.md)는 브라우저가 바로 보이게 글자로 보낸다.
fn content_type(path: &str) -> &'static str {
    let file = path.rsplit('/').next().unwrap_or(path);
    if file == "LICENSE" {
        return "text/plain; charset=utf-8";
    }
    match file.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("") {
        "txt" | "md" => "text/plain; charset=utf-8",
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "json" | "map" => "application/json",
        "woff2" => "font/woff2",
        "webmanifest" => "application/manifest+json",
        _ => "application/octet-stream",
    }
}

/// iOS·브라우저가 link 없이도 관례로 찾는 터치 아이콘 주소(`apple-touch-icon-precomposed.png`,
/// `apple-touch-icon-180x180.png`, `apple-touch-icon-120x120-precomposed.png`)는 `apple-touch-icon.png`과 같은 그림이다
fn touch_icon_alias(path: &str) -> &str {
    let alias = path.strip_prefix("apple-touch-icon").and_then(|rest| rest.strip_suffix(".png")).is_some_and(|mid| {
        mid.is_empty()
            || mid.starts_with('-')
                && mid[1..].split('-').all(|p| {
                    p == "precomposed"
                        || p.split_once('x')
                            .is_some_and(|(w, h)| !w.is_empty() && w.bytes().all(|b| b.is_ascii_digit()) && w == h)
                })
    });
    if alias { "apple-touch-icon.png" } else { path }
}

async fn static_file(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "없는 API").into_response();
    }
    if assets::WEB_ASSETS.is_empty() {
        return (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            "<!doctype html><meta charset=utf-8><title>aaa</title><p>웹 front가 빌드되지 않았다(web/에서 npm run build 뒤 aaa를 다시 빌드한다).",
        )
            .into_response();
    }
    let want = if path.is_empty() { "index.html" } else { touch_icon_alias(path) };
    let found = assets::WEB_ASSETS.iter().find(|(n, _)| *n == want).copied();
    // 없는 파일 주소에는 화면(HTML)이 아니라 404를 준다. 그림 주소에 HTML을 200으로 주면 그것을 아이콘으로 받는
    // 쪽(iOS 홈 화면·앱 전환)이 깨진 그림을 갖는다. 확장자 없는 주소만 화면 경로다(참고 static.ts는 /assets·/icons만 404).
    if found.is_none() && path.rsplit('/').next().is_some_and(|f| f.contains('.')) {
        return (StatusCode::NOT_FOUND, [(header::CACHE_CONTROL, "no-cache")], "없는 파일").into_response();
    }
    let (name, bytes) = found.or_else(|| assets::WEB_ASSETS.iter().find(|(n, _)| *n == "index.html").copied()).unwrap();
    // assets/는 이름에 hash가, fonts/는 폴더에 버전이 있다(글꼴을 바꾸면 폴더도 바꾼다)
    let cache = if name.starts_with("assets/") || name.starts_with("fonts/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type(name))
        .header(header::CACHE_CONTROL, cache)
        .body(Body::from(bytes))
        .unwrap()
}

#[cfg(test)]
mod content_type_tests {
    use super::content_type;

    #[test]
    fn touch_icon_aliases() {
        use super::touch_icon_alias;
        for p in [
            "apple-touch-icon.png",
            "apple-touch-icon-precomposed.png",
            "apple-touch-icon-180x180.png",
            "apple-touch-icon-120x120-precomposed.png",
        ] {
            assert_eq!(touch_icon_alias(p), "apple-touch-icon.png", "{p}");
        }
        for p in [
            "apple-touch-icon-x.png",
            "apple-touch-icon-180x120.png",
            "apple-touch-icon.svg",
            "icon-192.png",
            "assets/apple-touch-icon.png",
        ] {
            assert_eq!(touch_icon_alias(p), p, "{p}");
        }
    }

    #[test]
    fn by_last_segment() {
        assert_eq!(content_type("fonts/meslotendard-0.3.1/Meslotendard-Regular.woff2"), "font/woff2");
        assert_eq!(content_type("fonts/meslotendard-0.3.1/LICENSE"), "text/plain; charset=utf-8");
        assert_eq!(content_type("fonts/meslotendard-0.3.1/README.md"), "text/plain; charset=utf-8");
        assert_eq!(content_type("fonts/meslotendard-0.3.1/licenses/MESLO_APACHE-2.0.txt"), "text/plain; charset=utf-8");
        assert_eq!(content_type("assets/index-abc.js"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("v1.2/blob"), "application/octet-stream");
    }
}
