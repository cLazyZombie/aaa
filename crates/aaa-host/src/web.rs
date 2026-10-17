//! host 웹(기획서 §6.3·§6.4). `127.0.0.1:<web_port>`의 HTTP·WS로만 열고, 다른 기기에는 `tailscale serve` HTTPS로 공개한다.
//!
//! - 입장: aaa pairing 코드 → 장치 token cookie(HttpOnly·SameSite=Strict, HTTPS면 Secure·`__Host-`). loopback은 인증 신호가 아니다.
//! - Host 헤더 허용 목록(DNS rebinding), 변경 요청·WebSocket의 Origin 검사, CSP.
//! - `/api/v1/call`은 Front API 요청을 그대로 받는다(같은 타입, 같은 권한 판정).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aaa_proto::{
    B64, DeviceRole, ErrorBody, ErrorCode, EventsClientMsg, FrontEvent, FrontRequest, Outcome, TermClientMsg,
    TermServerMsg,
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

const OUTPUT_STALLED_REASON: &str = "terminal output consumer stalled";
/// 기기를 취소해 닫은 WebSocket. 웹은 다시 붙지 않고 pairing 화면으로 간다(web/src/api.ts `REVOKED_CLOSE_CODE`)
pub const DEVICE_REVOKED_CLOSE_CODE: u16 = 4401;
const DEVICE_REVOKED_REASON: &str = "device revoked";

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
    // 공개 주소(Host·Origin 허용)를 지금 serve 상태로 맞춘다. tailscale을 기다리지 않고 연다(그동안은 지난 값)
    let h = host.clone();
    tokio::spawn(async move { h.refresh_publish().await });
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
        .route("/api/v1/me", get(me).delete(sign_out))
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

async fn guard(State(host): State<Arc<Host>>, req: Request, next: Next) -> Response {
    let h = req.headers();
    // Host는 이 host의 두 주소 중 하나(포트까지), Origin은 그 주소와 정확히 같아야 한다: 같은 이름의 다른 포트
    // (같은 머신의 다른 웹 서비스)나 포트 없는 Origin도 남이다(herdr-web-ui `sameOrigin`, t3code `EnvironmentAuth`)
    let Some(expected) = host.web_origin(&host_header(h)) else {
        return (StatusCode::MISDIRECTED_REQUEST, "알 수 없는 Host").into_response();
    };
    let is_ws =
        h.get(header::UPGRADE).and_then(|v| v.to_str().ok()).is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if !matches!(*req.method(), Method::GET | Method::HEAD) || is_ws {
        let origin = h.get(header::ORIGIN).and_then(|v| v.to_str().ok()).unwrap_or("");
        if !origin.eq_ignore_ascii_case(&expected) {
            return (StatusCode::FORBIDDEN, "허용하지 않은 Origin").into_response();
        }
    }
    let api = req.uri().path().starts_with("/api/");
    let mut res = next.run(req).await;
    let hs = res.headers_mut();
    // 기기·pane 상태를 담은 API 답은 브라우저·proxy가 두지 않는다(herdr-web-ui `index.ts` API no-store)
    if api {
        hs.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-store"));
    }
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

/// 장치 cookie. HTTPS에서는 `__Host-aaa`만 본다: prefix 없는 `aaa`는 같은 이름의 다른 서비스(다른 포트)나
/// 하위 경로가 심을 수 있다. `=`이 없는 등 깨진 항목은 건너뛴다(herdr-web-ui `parseCookies`: 남의 cookie 하나가
/// 주인을 막지 않는다)
fn cookie_token(h: &HeaderMap) -> Option<String> {
    let name = if via_https(h) { "__Host-aaa" } else { "aaa" };
    for v in h.get_all(header::COOKIE) {
        let Ok(s) = v.to_str() else { continue };
        for part in s.split(';') {
            let Some((k, val)) = part.split_once('=') else { continue };
            if k.trim() == name {
                return Some(val.trim().to_string());
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
    /// 이 브라우저의 이름(herdr-web-ui `deviceLabel`, 「iPhone · Safari」). 설정 → 기기에서 바꾼다
    #[serde(default)]
    label: String,
}

#[derive(Serialize)]
struct Me {
    role: DeviceRole,
    device: String,
}

async fn pair(State(host): State<Arc<Host>>, headers: HeaderMap, Json(b): Json<PairBody>) -> Response {
    match host.redeem_pairing(&b.code, &b.label) {
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

/// 이 브라우저의 로그인을 잊는다(herdr-web-ui `DELETE /api/auth`): 두 이름의 cookie를 모두 지운다. 기기 기록은 남는다
/// (참고와 같다. 지우려면 설정 → 기기에서 취소한다). push 구독은 웹이 먼저 지운다.
async fn sign_out() -> Response {
    let mut res = StatusCode::NO_CONTENT.into_response();
    for c in [
        "__Host-aaa=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0",
        "aaa=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0",
    ] {
        res.headers_mut().append(header::SET_COOKIE, HeaderValue::from_static(c));
    }
    res
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
    /// 지금 쓰는 웹 화면 수(보이고 focus)
    in_use: usize,
    /// 쓰는 기기(앞 7자): 마지막으로 누르거나 치거나 화면을 켠 기기. 그 기기가 쓰는 화면인 동안 Web Push를 보내지 않는다(plan §8.105)
    user: Option<String>,
    /// 이 장치가 쓰는 기기다
    you: bool,
    /// 알리는 inbox 종류(`notify.kinds`). 웹의 앱 안 알림·소리·탭 알림도 이 종류에만 띄운다(push와 같은 사건)
    kinds: Vec<String>,
}

/// Web Push(§8.26): 키와 이 장치의 구독 상태. 보기 권한 장치도 알림은 받는다.
async fn push_info(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let subscribed = host.db.push_list(Some(&id)).map(|v| v.len()).unwrap_or(0);
    let key = host.vapid.as_ref().map(|v| v.public_b64());
    let in_use = host.in_use.load(std::sync::atomic::Ordering::SeqCst);
    let (user, you) = {
        let v = host.viewing.lock().unwrap();
        (v.user(), v.is_user(&id))
    };
    let kinds = host.cfg.notify.item_kinds();
    Json(Outcome::Ok(PushInfo { enabled: key.is_some(), key, subscribed, in_use, user, you, kinds })).into_response()
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

#[derive(Deserialize, Default)]
struct PushTestBody {
    /// 이 구독 하나에만(herdr-web-ui `sendTestPush(endpoint)`). 없으면 이 장치의 모든 구독
    endpoint: Option<String>,
}

/// 이 장치의 구독에 시험 알림을 보낸다. `endpoint`를 주면 그 구독 하나에 보내고, 없거나(`subscription_not_found`)
/// push 서비스가 버렸으면(`subscription_gone`, 지운다) 오류로 답한다. 웹은 그 말로 「알림 다시 켜기」를 권한다.
async fn push_test(State(host): State<Arc<Host>>, headers: HeaderMap, body: Option<Json<PushTestBody>>) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let only = body.and_then(|Json(b)| b.endpoint);
    let mut subs = host.db.push_list(Some(&id)).unwrap_or_default();
    if let Some(e) = &only {
        subs.retain(|s| &s.endpoint == e);
        if subs.is_empty() {
            let err = ErrorBody::new(ErrorCode::SubscriptionNotFound, "이 기기의 push 구독이 없다");
            return (StatusCode::NOT_FOUND, Json(Outcome::<()>::Err(err))).into_response();
        }
    }
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
    // 시험 알림은 사람이 켜거나 「테스트 보내기」를 누를 때만 간다(쓰는 화면이 있어도 보낸다, 참고 server/push.ts sendTest).
    // 휴대폰에 뜬 배너가 어디서 왔는지 가릴 수 있게 남긴다(사용자 화면 10-18 00:27, plan §8.82)
    host.db.audit(&format!("device:{id}"), "push_test", &format!("sent {sent} failed {}", failed.len()));
    if only.is_some() && sent == 0 {
        let gone = failed.first().is_some_and(|f| f.starts_with("브라우저가 구독을 버렸다"));
        let err = if gone {
            ErrorBody::new(ErrorCode::SubscriptionGone, "push 서비스가 이 기기의 구독을 버렸다(지웠다)")
        } else {
            ErrorBody::new(ErrorCode::Unavailable, failed.join("; "))
        };
        return (StatusCode::BAD_GATEWAY, Json(Outcome::<()>::Err(err))).into_response();
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

/// 입력줄 `Submit`의 글과 Enter 사이(harness 메시지 전달과 같다, 300ms). `AAA_TEST_SUBMIT_GAP_MS`는 시험에서만
/// 늘린다(그 틈에 기기를 폐기하는 시험)
fn submit_enter_gap() -> std::time::Duration {
    let ms = std::env::var("AAA_TEST_SUBMIT_GAP_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(300);
    std::time::Duration::from_millis(ms)
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
    let Some((dev, role)) = device(&host, &headers) else { return unauthorized() };
    ws.on_upgrade(move |socket| events_loop(host, dev, role, socket))
}

/// 머신·pane 목록(바뀔 때마다 전체)과 확인창 질문·thread 사건. 질문·thread는 연결한 뒤 목록을 한 번 읽고 이 사건으로 따라간다.
async fn events_loop(host: Arc<Host>, dev: String, role: DeviceRole, mut socket: WebSocket) {
    let mut changed = host.changed.subscribe();
    let mut questions = host.question_events.subscribe();
    let mut threads = host.thread_events.subscribe();
    let mut preferences = host.preferences.subscribe();
    let mut revoked = host.device_revoked.subscribe();
    let mut presence = Presence { host: host.clone(), dev: dev.clone(), active: false };
    let mut send_state = true;
    // 공유 설정 문서는 연결을 연다: 다시 붙은 화면(또는 읽기가 경주에 진 화면)도 마지막 문서를 갖는다(herdr-web-ui machine-api.ts)
    let mut send_preferences = true;
    let close_revoked = |mut socket: WebSocket| async move {
        let frame = CloseFrame { code: DEVICE_REVOKED_CLOSE_CODE, reason: DEVICE_REVOKED_REASON.into() };
        let _ = socket.send(Message::Close(Some(frame))).await;
    };
    // 듣기 시작한 뒤에 다시 본다(pane_session과 같다)
    if host.is_revoked(&dev) {
        return close_revoked(socket).await;
    }
    // 살아 있는지: 브라우저는 WebSocket ping에 저절로 pong한다. 휴대폰이 잠들거나 망이 바뀌어 반쯤 열린 연결은 보내기가
    // 실패하지 않고 몇 분씩 남는다. 그동안 이 연결의 「쓰는 화면」이 남아 어느 기기에도 push가 가지 않고, 그 화면은 사건을
    // 받지 못한다(10-18 iPhone 실기). 아무것도 받지 못한 채 EVENTS_DEAD_AFTER가 지나면 끊는다(herdr-web-ui는 Bun
    // WebSocket의 ping·idleTimeout과 웹 ws.ts 「Liveness」 probe가 같은 일을 한다).
    let (every, dead_after) = events_liveness();
    let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    let mut heard = tokio::time::Instant::now();
    loop {
        if send_preferences {
            send_preferences = false;
            let doc = preferences.borrow_and_update().clone();
            let text = serde_json::to_string(&FrontEvent::Preferences(doc)).unwrap();
            if socket.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        if send_state {
            let ev = FrontEvent::State { machines: host.machine_views(), panes: host.pane_views(role) };
            if socket.send(Message::Text(serde_json::to_string(&ev).unwrap().into())).await.is_err() {
                break;
            }
        }
        tokio::select! {
            biased;
            r = revoked.recv() => {
                send_state = false;
                let mine = match r {
                    Ok(gone) => gone == dev,
                    Err(RecvError::Lagged(_)) => host.is_revoked(&dev),
                    Err(RecvError::Closed) => false,
                };
                if mine {
                    return close_revoked(socket).await;
                }
            }
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
            r = preferences.changed() => {
                if r.is_err() { break }
                send_state = false;
                send_preferences = true;
            }
            _ = ping.tick() => {
                send_state = false;
                if heard.elapsed() >= dead_after {
                    tracing::info!("상태 사건 연결이 {}초 동안 답하지 않아 끊는다", dead_after.as_secs());
                    break;
                }
                if socket.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
            m = socket.recv() => {
                send_state = false;
                heard = tokio::time::Instant::now();
                match m {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Text(t))) => match serde_json::from_str::<EventsClientMsg>(&t) {
                        Ok(EventsClientMsg::Presence { active }) => presence.set(active),
                        Ok(EventsClientMsg::Touch) => host.touch_device(&dev),
                        Ok(EventsClientMsg::Ping { id }) => {
                            let text = serde_json::to_string(&FrontEvent::Pong { id }).unwrap();
                            if socket.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => {}
                    },
                    _ => {}
                }
            }
        }
    }
}

/// 상태 사건 연결의 WebSocket ping 간격과, 아무것도 받지 못하면 끊는 시간(15초·45초: ping 두 번을 놓치고도 남는다).
/// `AAA_TEST_EVENTS_PING_MS`는 시험에서만 줄인다(끊는 시간은 그 세 배).
fn events_liveness() -> (std::time::Duration, std::time::Duration) {
    let ms = std::env::var("AAA_TEST_EVENTS_PING_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(15_000);
    (std::time::Duration::from_millis(ms), std::time::Duration::from_millis(ms * 3))
}

/// 한 events 연결이 「쓰는 화면」인지. 바뀔 때만 host 수를 고치고, 연결이 어떻게 끝나든 drop에서 뺀다.
struct Presence {
    host: Arc<Host>,
    /// 이 연결의 기기(그 기기가 연 pane 터미널을 「보는 화면」으로 센다, seen.rs)
    dev: String,
    active: bool,
}

impl Presence {
    fn set(&mut self, active: bool) {
        use std::sync::atomic::Ordering::SeqCst;
        if active != self.active {
            self.active = active;
            if active {
                self.host.in_use.fetch_add(1, SeqCst);
            } else {
                self.host.in_use.fetch_sub(1, SeqCst);
            }
            self.host.view_presence(&self.dev, active);
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.set(false);
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
    /// 남은 줄을 버리고 이 code로 닫는다(4008 출력 정지, 4401 기기 취소)
    stop: tokio::sync::watch::Sender<Option<(u16, &'static str)>>,
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
        let (stop, mut stopped) = tokio::sync::watch::channel(None);
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
            // 따라오지 못했거나 기기가 취소됐다: 남은 줄은 버리고 그 code로 닫는다. 그 닫기도 못 가면 연결을 버린다.
            let (code, reason) = stopped.borrow().unwrap_or((OUTPUT_STALLED_CLOSE_CODE, OUTPUT_STALLED_REASON));
            let close = Message::Close(Some(CloseFrame { code, reason: reason.into() }));
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
        let _ = self.stop.send(Some((OUTPUT_STALLED_CLOSE_CODE, OUTPUT_STALLED_REASON)));
    }

    /// 기기가 취소됐다: 밀린 출력도 보내지 않고 4401로 닫는다
    fn revoked(&self) {
        let _ = self.stop.send(Some((DEVICE_REVOKED_CLOSE_CODE, DEVICE_REVOKED_REASON)));
    }
}

/// snapshot 하나가 새 출력 구독이다: 새 번호의 창을 열고, replay가 느린 연결을 건널 시간을 준다(참고 `join`)
fn open_stream(
    out: &Outbox,
    streams: &mut u32,
    a: &mut crate::Attach,
    readonly: bool,
    now: u64,
) -> Option<OutputWindow> {
    *streams += 1;
    let mut window = OutputWindow::new(*streams);
    let bytes = a.replay.len() as u64;
    let text = serde_json::to_string(&TermServerMsg::Snapshot {
        replay: B64(std::mem::take(&mut a.replay)),
        cols: a.cols,
        rows: a.rows,
        readonly,
        stream: *streams,
        history_more: a.more,
    })
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
    // 이 연결이 그 pane을 보는 동안(seen.rs). 가려지면 웹은 lease를 놓을 뿐 연결은 두므로 「보는 중」은 기기의 presence가 정한다
    host.view_term(&front.id, &dev, Some(&id));
    pane_session(&host, &id, &dev, role, &front, acks, socket).await;
    host.view_term(&front.id, &dev, None);
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
    // 취소를 듣기 시작한 뒤에 다시 본다: upgrade와 취소가 엇갈려도 놓치지 않는다(t3code SessionStore `awaitInvalidation`)
    let mut revoked = host.device_revoked.subscribe();
    let Some(mut attach) = host.attach_pane(&id) else { return };
    let mut next = attach.offset;
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
    let Some(mut window) = open_stream(&out, &mut streams, &mut attach, view.readonly, now()) else { return };
    let mut rx = attach.rx;
    if !out.msg(&readonly_msg(&view, role)) || !out.msg(&TermServerMsg::State { pane: Box::new(view) }) {
        return;
    }
    // ACK가 아예 오지 않아도(가려진 탭) 정지를 잰다(참고 outputTimer 100ms)
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut queue_since = None;
    if host.is_revoked(dev) {
        out.revoked();
        return;
    }
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
            biased;
            r = revoked.recv() => {
                // 놓쳤으면(Lagged) 이 기기인지 다시 본다
                let mine = match r {
                    Ok(gone) => gone == dev,
                    Err(RecvError::Lagged(_)) => host.is_revoked(dev),
                    Err(RecvError::Closed) => false,
                };
                if mine {
                    out.revoked();
                    break;
                }
            }
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
                        if !out.msg(&readonly_msg(&view, role)) || !out.msg(&TermServerMsg::State { pane: Box::new(view) }) { break; }
                        false
                    }
                    Err(RecvError::Closed) => break,
                };
                if resync {
                    let Some(mut a) = host.attach_pane(&id) else { break };
                    next = a.offset;
                    let readonly = host.pane_view_of(&id, role).map(|p| p.readonly).unwrap_or(true);
                    let Some(w) = open_stream(&out, &mut streams, &mut a, readonly, now()) else { break };
                    rx = a.rx;
                    window = w;
                }
            }
            m = stream.next() => {
                // 취소 사건보다 먼저 와 있던 입력도 받지 않는다(§6.4 「대기 중인 입력에도 바로」)
                if host.is_revoked(dev) {
                    out.revoked();
                    break;
                }
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
                        Ok(TermClientMsg::Submit { id: sid, data }) => {
                            // 글, 잠깐 뒤 Enter(한 덩어리로 보내면 Codex가 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다).
                            // 이 연결의 다음 입력은 Enter 뒤에 간다. 답은 둘 다 pane에 들어간 뒤다.
                            host.touch_device(dev);
                            let typed = |data: Vec<u8>| FrontRequest::PaneInput { pane: id.clone(), data: B64(data) };
                            let mut sent = host.call_from(role, &actor, Some(front), typed(data.into_bytes())).await;
                            if sent.is_ok() {
                                tokio::time::sleep(submit_enter_gap()).await;
                                // 기다리는 동안 폐기된 기기의 Enter는 보내지 않는다(t-0340 감사 A9, t3code `awaitInvalidation`)
                                if host.is_revoked(dev) {
                                    out.revoked();
                                    break;
                                }
                                sent = host.call_from(role, &actor, Some(front), typed(b"\r".to_vec())).await;
                            }
                            let reply = TermServerMsg::Submitted { id: sid, ok: sent.is_ok(), message: sent.err().map(|e| e.message) };
                            if !out.msg(&reply) {
                                break;
                            }
                            None
                        }
                        Ok(TermClientMsg::History { id: hid, anchor, depth }) => {
                            // 앞 기록 한 덩어리(plan §8.67). 출력 흐름(ACK 창) 밖의 답이라 창에 넣지 않는다:
                            // 한 덩어리는 replay 기록 상한(2000줄·384KiB) 안이다.
                            let anchor: Vec<String> = anchor.into_iter().take(8).collect();
                            let reply = match host.older_history(&id, &anchor, depth as usize) {
                                Some(o) => TermServerMsg::History { id: hid, data: B64(o.data), lines: o.lines as u32, more: o.more, found: true },
                                None => TermServerMsg::History { id: hid, data: B64(Vec::new()), lines: 0, more: false, found: false },
                            };
                            if !out.msg(&reply) {
                                break;
                            }
                            None
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
                // pane에 친 기기가 쓰는 기기다(plan §8.105). 크기 걸기는 다시 붙을 때도 오므로 세지 않는다(누름은 웹이 touch로 알린다)
                if matches!(req, Some(FrontRequest::PaneInput { .. }) | Some(FrontRequest::PaneTake { .. })) {
                    host.touch_device(dev);
                }
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

    /// build.rs가 만든 자산 목록은 빌드한 worktree의 절대 경로를 갖지 않는다: build script 실행 결과를 캐시하는
    /// 도구가 다른 worktree에 이 파일을 되돌려도 그 worktree의 web/dist를 읽어야 한다(t-0327, kache build_script_run)
    #[test]
    fn web_assets_do_not_bake_build_paths() {
        let src = include_str!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
        for line in src.lines().filter(|l| l.contains("include_bytes!")) {
            assert!(line.contains("include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../web/dist/"), "{line}");
        }
    }

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

/// 입구 검사(t-0340 보안 감사 A1·A4·A5·A8·A10): router를 그대로 부른다. 공개 주소는 같은 이름의 8443이고, 같은
/// 머신 443에 다른 웹 서비스가 있는 운영 배치를 흉내 낸다.
#[cfg(test)]
mod guard_tests {
    use super::*;
    use std::time::Instant;
    use tower::ServiceExt;

    const PUBLIC: &str = "mac-a.example.ts.net:8443";
    const LOOP: &str = "127.0.0.1:7000";

    struct Lab(Arc<Host>, std::path::PathBuf);
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }

    fn lab(public_origin: &str) -> Lab {
        let dir = std::env::temp_dir().join(format!("aaa-guard-test-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, dir.clone()).unwrap();
        h.web_port.store(7000, std::sync::atomic::Ordering::Relaxed);
        h.db.set_meta("publish_origin", public_origin).unwrap();
        Lab(h, dir)
    }

    fn token(h: &Host) -> String {
        let (code, _) = h.mint_pairing(DeviceRole::Controller, Instant::now());
        h.redeem_pairing(&code, "").unwrap().0
    }

    async fn ask(h: &Arc<Host>, method: &str, path: &str, host: &str, more: &[(&str, &str)]) -> Response {
        let mut req = Request::builder().method(method).uri(path).header(header::HOST, host);
        for (k, v) in more {
            req = req.header(*k, *v);
        }
        router(h.clone()).oneshot(req.body(Body::empty()).unwrap()).await.unwrap()
    }

    async fn code(h: &Arc<Host>, method: &str, path: &str, host: &str, more: &[(&str, &str)]) -> StatusCode {
        ask(h, method, path, host, more).await.status()
    }

    #[tokio::test]
    async fn host_must_be_this_host_with_its_port() {
        let l = lab("https://mac-a.example.ts.net:8443");
        assert_ne!(code(&l.0, "GET", "/", PUBLIC, &[]).await, StatusCode::MISDIRECTED_REQUEST);
        assert_ne!(code(&l.0, "GET", "/", "MAC-A.example.ts.net:8443", &[]).await, StatusCode::MISDIRECTED_REQUEST);
        assert_ne!(code(&l.0, "GET", "/", LOOP, &[]).await, StatusCode::MISDIRECTED_REQUEST);
        // 같은 이름의 다른 포트·포트 없음(443의 다른 서비스), localhost, 다른 loopback 포트, 다른 이름
        for other in [
            "mac-a.example.ts.net:9999",
            "mac-a.example.ts.net",
            "localhost:7000",
            "127.0.0.1:7001",
            "evil.example:8443",
        ] {
            assert_eq!(code(&l.0, "GET", "/", other, &[]).await, StatusCode::MISDIRECTED_REQUEST, "{other}");
        }
    }

    #[tokio::test]
    async fn origin_must_match_the_whole_origin() {
        let l = lab("https://mac-a.example.ts.net:8443");
        let ok = [("origin", "https://mac-a.example.ts.net:8443")];
        assert_eq!(code(&l.0, "DELETE", "/api/v1/me", PUBLIC, &ok).await, StatusCode::NO_CONTENT);
        assert_eq!(
            code(&l.0, "DELETE", "/api/v1/me", LOOP, &[("origin", "http://127.0.0.1:7000")]).await,
            StatusCode::NO_CONTENT
        );
        for bad in [
            "https://mac-a.example.ts.net:9999",
            "https://mac-a.example.ts.net",
            "http://mac-a.example.ts.net:8443",
            "http://127.0.0.1:7000",
            "http://localhost:7000",
            "null",
            "",
        ] {
            assert_eq!(
                code(&l.0, "DELETE", "/api/v1/me", PUBLIC, &[("origin", bad)]).await,
                StatusCode::FORBIDDEN,
                "{bad:?}"
            );
        }
        assert_eq!(code(&l.0, "DELETE", "/api/v1/me", PUBLIC, &[]).await, StatusCode::FORBIDDEN);
        // loopback 주소에는 그 주소의 origin만
        let other = [("origin", "https://mac-a.example.ts.net:8443")];
        assert_eq!(code(&l.0, "DELETE", "/api/v1/me", LOOP, &other).await, StatusCode::FORBIDDEN);
        // WebSocket upgrade도 GET이지만 Origin을 본다(cookie가 있어도)
        let t = format!("__Host-aaa={}", token(&l.0));
        let ws = |origin| {
            [
                ("origin", origin),
                ("connection", "upgrade"),
                ("upgrade", "websocket"),
                ("sec-websocket-version", "13"),
                ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
                ("cookie", t.as_str()),
            ]
        };
        for bad in ["https://mac-a.example.ts.net:9999", "https://mac-a.example.ts.net"] {
            assert_eq!(code(&l.0, "GET", "/api/v1/events", PUBLIC, &ws(bad)).await, StatusCode::FORBIDDEN, "{bad}");
        }
        assert_ne!(
            code(&l.0, "GET", "/api/v1/events", PUBLIC, &ws("https://mac-a.example.ts.net:8443")).await,
            StatusCode::FORBIDDEN
        );
    }

    #[tokio::test]
    async fn the_default_https_port_has_no_port_in_host_or_origin() {
        let l = lab("https://mac-a.example.ts.net");
        for host in ["mac-a.example.ts.net", "mac-a.example.ts.net:443"] {
            let ok = [("origin", "https://mac-a.example.ts.net")];
            assert_eq!(code(&l.0, "DELETE", "/api/v1/me", host, &ok).await, StatusCode::NO_CONTENT, "{host}");
            let bad = [("origin", "https://mac-a.example.ts.net:8443")];
            assert_eq!(code(&l.0, "DELETE", "/api/v1/me", host, &bad).await, StatusCode::FORBIDDEN, "{host}");
        }
        assert_eq!(code(&l.0, "GET", "/", PUBLIC, &[]).await, StatusCode::MISDIRECTED_REQUEST);
    }

    #[tokio::test]
    async fn not_published_means_loopback_only() {
        let l = lab("https://mac-a.example.ts.net:8443");
        l.0.db.del_meta("publish_origin").unwrap();
        // 이름만 알던 옛 기록(publish_dns)으로는 열리지 않는다
        l.0.db.set_meta("publish_dns", "mac-a.example.ts.net").unwrap();
        assert_eq!(code(&l.0, "GET", "/", PUBLIC, &[]).await, StatusCode::MISDIRECTED_REQUEST);
        assert_ne!(code(&l.0, "GET", "/", LOOP, &[]).await, StatusCode::MISDIRECTED_REQUEST);
        // serve 상태를 아직 못 읽었어도 aaa가 만든 공개(포트를 안다)는 그 포트로만 열린다
        l.0.db.set_meta("publish_port", "8443").unwrap();
        assert_ne!(code(&l.0, "GET", "/", PUBLIC, &[]).await, StatusCode::MISDIRECTED_REQUEST);
        assert_eq!(code(&l.0, "GET", "/", "mac-a.example.ts.net", &[]).await, StatusCode::MISDIRECTED_REQUEST);
    }

    #[tokio::test]
    async fn cookies_skip_junk_and_https_takes_only_the_host_prefix() {
        let l = lab("https://mac-a.example.ts.net:8443");
        let t = token(&l.0);
        let me = |cookie: String| {
            let h = l.0.clone();
            let host = if cookie.contains("__Host-") { PUBLIC } else { LOOP };
            async move { code(&h, "GET", "/api/v1/me", host, &[("cookie", cookie.as_str())]).await }
        };
        // 같은 host의 다른 서비스가 심은 `=` 없는 항목·빈 이름이 앞에 있어도
        assert_eq!(me(format!("foo; =x; __Host-aaa={t}")).await, StatusCode::OK);
        assert_eq!(me(format!("junk; aaa={t}")).await, StatusCode::OK);
        // HTTPS에서 prefix 없는 cookie는 장치 cookie가 아니다(다른 포트·하위 경로가 심을 수 있다)
        assert_eq!(
            code(&l.0, "GET", "/api/v1/me", PUBLIC, &[("cookie", &format!("aaa={t}"))]).await,
            StatusCode::UNAUTHORIZED
        );
        let planted = format!("aaa=junk; __Host-aaa={t}");
        assert_eq!(code(&l.0, "GET", "/api/v1/me", PUBLIC, &[("cookie", planted.as_str())]).await, StatusCode::OK);
    }

    #[tokio::test]
    async fn api_answers_are_not_stored() {
        let l = lab("https://mac-a.example.ts.net:8443");
        for path in ["/api/v1/me", "/api/v1/usage", "/api/v1/push"] {
            let res = ask(&l.0, "GET", path, LOOP, &[]).await;
            assert_eq!(
                res.headers().get(header::CACHE_CONTROL).map(|v| v.to_str().unwrap()),
                Some("no-store"),
                "{path}"
            );
        }
    }
}
