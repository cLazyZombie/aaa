//! host 웹(기획서 §6.3·§6.4). `127.0.0.1:<web_port>`의 HTTP·WS로만 열고, 다른 기기에는 `tailscale serve` HTTPS로 공개한다.
//!
//! - 입장: aaa pairing 코드 → 장치 token cookie(HttpOnly·SameSite=Strict, HTTPS면 Secure·`__Host-`). loopback은 인증 신호가 아니다.
//! - Host 헤더 허용 목록(DNS rebinding), 변경 요청·WebSocket의 Origin 검사, CSP.
//! - `/api/v1/call`은 Front API 요청을 그대로 받는다(같은 타입, 같은 권한 판정).

use std::sync::Arc;

use aaa_proto::{
    B64, DeviceRole, ErrorBody, ErrorCode, FrontEvent, FrontRequest, Outcome, TermClientMsg, TermServerMsg,
};
use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;

use super::{FrontRef, Host, PaneEvent};

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

async fn pane_ws(
    State(host): State<Arc<Host>>,
    Path(id): Path<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let Some((dev, role)) = device(&host, &headers) else { return unauthorized() };
    if host.pane_view_of(&id, role).is_none() {
        return (StatusCode::NOT_FOUND, "pane이 없다").into_response();
    }
    // 그림 조각(512KiB, base64)이 들어갈 만큼만 받는다
    ws.max_message_size(1 << 20).on_upgrade(move |socket| pane_loop(host, id, dev, role, socket))
}

async fn send_msg(socket: &mut WebSocket, m: &TermServerMsg) -> bool {
    socket.send(Message::Text(serde_json::to_string(m).unwrap().into())).await.is_ok()
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

/// WebSocket 하나가 대화형 front 하나다. 조작 lease는 이 연결에 묶이고 끝나면 풀린다.
async fn pane_loop(host: Arc<Host>, id: String, dev: String, role: DeviceRole, socket: WebSocket) {
    let short: String = dev.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let front = host.new_front(format!("웹({short})"));
    pane_session(&host, &id, &dev, role, &front, socket).await;
    host.front_gone(&front.id);
}

async fn pane_session(
    host: &Arc<Host>,
    id: &str,
    dev: &str,
    role: DeviceRole,
    front: &FrontRef,
    mut socket: WebSocket,
) {
    let id = id.to_string();
    let actor = format!("device:{dev}");
    let Some((mut rx, replay, mut next, cols, rows)) = host.attach_pane(&id) else { return };
    let mut upload: Option<crate::attach::Upload> = None;
    // 첨부가 실패하면 그 첨부의 남은 조각은 오류 없이 버린다(다음 첨부는 offset 0으로 시작한다)
    let mut upload_failed = false;
    let Some(view) = host.pane_view_of(&id, role) else { return };
    let snapshot = TermServerMsg::Snapshot { replay: B64(replay), cols, rows, readonly: view.readonly };
    if !send_msg(&mut socket, &TermServerMsg::Front { id: front.id.clone() }).await
        || !send_msg(&mut socket, &snapshot).await
        || !send_msg(&mut socket, &readonly_msg(&view, role)).await
        || !send_msg(&mut socket, &TermServerMsg::State { pane: view }).await
    {
        return;
    }
    loop {
        tokio::select! {
            ev = rx.recv() => {
                let resync = match ev {
                    Ok(PaneEvent::Output(start, data)) => {
                        let end = start + data.len() as u64;
                        if end <= next { false }
                        else if start > next { true }
                        else {
                            let part = data[(next - start) as usize..].to_vec();
                            next = end;
                            if socket.send(Message::Binary(part.into())).await.is_err() { break; }
                            false
                        }
                    }
                    Ok(PaneEvent::Reset) | Err(RecvError::Lagged(_)) => true,
                    Ok(PaneEvent::State) => {
                        let Some(view) = host.pane_view_of(&id, role) else { break };
                        if !send_msg(&mut socket, &readonly_msg(&view, role)).await { break; }
                        if !send_msg(&mut socket, &TermServerMsg::State { pane: view }).await { break; }
                        false
                    }
                    Err(RecvError::Closed) => break,
                };
                if resync {
                    let Some((nrx, replay, offset, cols, rows)) = host.attach_pane(&id) else { break };
                    rx = nrx;
                    next = offset;
                    let readonly = host.pane_view_of(&id, role).map(|p| p.readonly).unwrap_or(true);
                    if !send_msg(&mut socket, &TermServerMsg::Snapshot { replay: B64(replay), cols, rows, readonly }).await { break; }
                }
            }
            m = socket.recv() => {
                let req = match m {
                    Some(Ok(Message::Text(t))) => match serde_json::from_str::<TermClientMsg>(&t) {
                        Ok(TermClientMsg::Input { data }) => Some(FrontRequest::PaneInput { pane: id.clone(), data: B64(data.into_bytes()) }),
                        Ok(TermClientMsg::Resize { cols, rows }) if role.can_control() => {
                            Some(FrontRequest::PaneResize { pane: id.clone(), cols: cols.clamp(10, 500), rows: rows.clamp(3, 300) })
                        }
                        Ok(TermClientMsg::Take) => Some(FrontRequest::PaneTake { pane: id.clone() }),
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
                            if let Some(r) = reply && !send_msg(&mut socket, &r).await {
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
                    && !send_msg(&mut socket, &TermServerMsg::Error(e)).await
                {
                    break;
                }
            }
        }
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
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
    let want = if path.is_empty() { "index.html" } else { path };
    let (name, bytes) = assets::WEB_ASSETS
        .iter()
        .find(|(n, _)| *n == want)
        .or_else(|| assets::WEB_ASSETS.iter().find(|(n, _)| *n == "index.html"))
        .copied()
        .unwrap();
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
