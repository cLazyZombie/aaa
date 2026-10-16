//! host 웹(기획서 §6.3·§6.4). `127.0.0.1:<web_port>`의 HTTP·WS로만 열고, 다른 기기에는 `tailscale serve` HTTPS로 공개한다.
//!
//! - 입장: aa pairing 코드 → 장치 token cookie(HttpOnly·SameSite=Strict, HTTPS면 Secure·`__Host-`). loopback은 인증 신호가 아니다.
//! - Host 헤더 허용 목록(DNS rebinding), 변경 요청·WebSocket의 Origin 검사, CSP.
//! - `/api/v1/call`은 Front API 요청을 그대로 받는다(같은 타입, 같은 권한 판정).

use std::sync::Arc;

use aa_proto::{
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
        .route("/api/v1/panes/{id}/ws", get(pane_ws))
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
    hs.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    hs.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    hs.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    hs.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    res
}

fn cookie_token(h: &HeaderMap) -> Option<String> {
    for v in h.get_all(header::COOKIE) {
        let Ok(s) = v.to_str() else { continue };
        for part in s.split(';') {
            let (k, val) = part.trim().split_once('=')?;
            if k == "__Host-aa" || k == "aa" {
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
                format!("__Host-aa={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=31536000")
            } else {
                format!("aa={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000")
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

async fn events_ws(State(host): State<Arc<Host>>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    let Some((_, role)) = device(&host, &headers) else { return unauthorized() };
    ws.on_upgrade(move |socket| events_loop(host, role, socket))
}

async fn events_loop(host: Arc<Host>, role: DeviceRole, mut socket: WebSocket) {
    let mut changed = host.changed.subscribe();
    loop {
        let ev = FrontEvent::State { machines: host.machine_views(), panes: host.pane_views(role) };
        let text = serde_json::to_string(&ev).unwrap();
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
        tokio::select! {
            r = changed.changed() => if r.is_err() { break },
            m = socket.recv() => match m {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {}
            },
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
    ws.on_upgrade(move |socket| pane_loop(host, id, dev, role, socket))
}

async fn send_msg(socket: &mut WebSocket, m: &TermServerMsg) -> bool {
    socket.send(Message::Text(serde_json::to_string(m).unwrap().into())).await.is_ok()
}

/// 입력할 수 있는지와 그 이유(끊김 > 종료 > 권한 순)
fn readonly_msg(view: &aa_proto::PaneView, role: DeviceRole) -> TermServerMsg {
    let reason = if view.connection == aa_proto::Connection::Disconnected {
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
            "<!doctype html><meta charset=utf-8><title>aa</title><p>웹 front가 빌드되지 않았다(web/에서 npm run build 뒤 aa를 다시 빌드한다).",
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
    let cache = if name.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    Response::builder()
        .header(header::CONTENT_TYPE, content_type(name))
        .header(header::CACHE_CONTROL, cache)
        .body(Body::from(bytes))
        .unwrap()
}
