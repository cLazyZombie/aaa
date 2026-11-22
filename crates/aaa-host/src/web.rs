//! host 웹(기획서 §6.3·§6.4). `127.0.0.1:<web_port>`의 HTTP·WS로만 열고, 다른 기기에는 `tailscale serve` HTTPS로 공개한다.
//!
//! - 입장: aaa pairing 코드 → 장치 token cookie(HttpOnly·SameSite=Strict, HTTPS면 Secure·`__Host-`). loopback은 인증 신호가 아니다.
//! - Host 헤더 허용 목록(DNS rebinding), 변경 요청·WebSocket의 Origin 검사, CSP.
//! - `/api/v1/call`은 Front API 요청을 그대로 받는다(같은 타입, 같은 권한 판정).
//! - 옛 TUI 전용 `/api/v1/front`는 인증 뒤 삭제 안내로 거절한다.

mod files;
use files::*;
mod push_api;
use push_api::*;
mod events;
use events::*;
mod pane_ws;
use pane_ws::*;
mod static_files;
use static_files::*;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use aaa_proto::{
    B64, DeviceRole, ErrorBody, ErrorCode, EventsClientMsg, FrontEvent, FrontRequest, Outcome, SubmitStage,
    TermClientMsg, TermServerMsg,
};
use anyhow::{Context, Result};
use axum::body::Body;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Extension, Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;

use super::{FrontRef, Host, PaneEvent};
use crate::chatwatch::ChatNote;
use crate::output::{OUTPUT_HARD_BYTES, OUTPUT_STALLED_CLOSE_CODE, OutputWindow, Verdict, check_unacked};

mod assets;
#[cfg(test)]
mod events_tests;
mod report_delivery;

const OUTPUT_STALLED_REASON: &str = "terminal output consumer stalled";
/// 기기를 취소해 닫은 WebSocket. 웹은 다시 붙지 않고 pairing 화면으로 간다(web/src/api.ts `REVOKED_CLOSE_CODE`)
pub const DEVICE_REVOKED_CLOSE_CODE: u16 = 4401;
const DEVICE_REVOKED_REASON: &str = "device revoked";

const CSP: &str = "default-src 'self'; script-src 'self'; worker-src 'self' blob:; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
                   connect-src 'self'; font-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

pub async fn start(host: &Arc<Host>) -> Result<()> {
    let addr = host.cfg.web.listen.clone();
    if addr.is_empty() {
        // 웹을 끈 설정: 다음 시작의 안전 모드도 웹을 열지 않는다(지난 주소를 남기지 않는다)
        if let Err(e) = host.db.set_meta(crate::safe_mode::LISTEN_KEY, "") {
            tracing::warn!("웹 주소 기록 실패: {e:#}");
        }
        return Ok(());
    }
    let listener = tokio::net::TcpListener::bind(&addr).await.with_context(|| format!("웹 listen {addr}"))?;
    start_bound(host, listener)
}

/// 안전 모드와 같은 주소면 이미 열린 소켓·접속 대기열을 그대로 이어 받는다.
/// 「다시 읽기」로 주소를 바꾸거나 웹을 끄면 새 설정을 따른다.
pub(crate) async fn resume(host: &Arc<Host>, listener: tokio::net::TcpListener) -> Result<()> {
    let addr = listener.local_addr()?;
    let same_address = host
        .cfg
        .web
        .listen
        .parse::<std::net::SocketAddr>()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .is_some_and(|configured| {
            configured.ip() == addr.ip() && (configured.port() == 0 || configured.port() == addr.port())
        });
    if same_address {
        start_bound(host, listener)
    } else {
        drop(listener);
        start(host).await
    }
}

/// 소유한 실제 listener를 serving에 넘긴다. cfg 값만으로 web 준비를 선언하지 않는다.
pub(crate) fn start_bound(host: &Arc<Host>, listener: tokio::net::TcpListener) -> Result<()> {
    let addr = listener.local_addr()?;
    host.web_port.store(listener.local_addr()?.port(), std::sync::atomic::Ordering::Relaxed);
    *host.web_listen.lock().unwrap() = Some(listener.local_addr()?.to_string());
    // 설정을 읽지 못한 다음 시작의 안전 모드가 같은 주소(tailscale serve가 가리키는 포트)에 뜨도록 실제로 연 주소를 남긴다
    if let Err(e) = host.db.set_meta(crate::safe_mode::LISTEN_KEY, &listener.local_addr()?.to_string()) {
        tracing::warn!("웹 주소 기록 실패: {e:#}");
    }
    tracing::info!(%addr, "웹 listen(loopback)");
    // 공개 주소(Host·Origin 허용)를 지금 serve 상태로 맞춘다. tailscale을 기다리지 않고 연다(그동안은 지난 값)
    let h = host.clone();
    tokio::spawn(async move { h.refresh_publish().await });
    let listener = Arc::new(listener);
    let host = host.clone();
    host.clone().supervise(crate::supervisor::Task::Web, move || {
        let host = host.clone();
        let listener = ResourceListener { inner: listener.clone(), host: host.clone() };
        async move {
            host.task_progress(crate::supervisor::Task::Web);
            // 오류의 원문은 외부 알림에 넣지 않는다. 정상 조기 종료도 supervisor가 이상으로 판정한다.
            if let Err(error) = axum::serve(listener, router(host)).await {
                tracing::warn!(error = %aaa_common::diagnostic::Cause::capture(&error), "웹 서버 중단");
            }
        }
    });
    Ok(())
}

/// axum 기본 listener의 재시도(1초)를 유지하고 EMFILE을 시스템 inbox에도 남긴다.
struct ResourceListener {
    inner: Arc<tokio::net::TcpListener>,
    host: Arc<Host>,
}
impl axum::serve::Listener for ResourceListener {
    type Io = tokio::net::TcpStream;
    type Addr = std::net::SocketAddr;
    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            match self.inner.accept().await {
                Ok(pair) => return pair,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::ConnectionReset
                    ) =>
                {
                    continue;
                }
                Err(e) => {
                    self.host.resource_error("web accept", &e);
                    tracing::warn!(error = %e, "웹 연결 accept 실패");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

pub fn router(host: Arc<Host>) -> Router {
    Router::new()
        .route("/api/v1/pair", post(pair))
        .route("/api/v1/install-probe/{challenge}", get(install_probe))
        .route("/api/v1/me", get(me).delete(sign_out))
        .route("/api/v1/call", post(call))
        .route("/api/v1/passkey/register/begin", post(passkey_register_begin))
        .route("/api/v1/passkey/register", post(passkey_register))
        .route("/api/v1/passkey/approve/begin", post(passkey_approve_begin))
        .route("/api/v1/passkey/approve", post(passkey_approve))
        .route("/api/v1/events", get(events_ws))
        .route("/api/v1/front", get(front_ws))
        .route("/api/v1/app-badge", get(app_badge))
        .route("/api/v1/push", get(push_info))
        .route("/api/v1/push/subscribe", post(push_subscribe))
        .route("/api/v1/push/unsubscribe", post(push_unsubscribe))
        .route("/api/v1/push/test", post(push_test))
        .route("/api/v1/diag/input", post(diag_input))
        .route("/api/v1/usage", get(usage_report))
        .route("/api/v1/account-usage", get(account_quota_report))
        .route("/api/v1/panes/{id}/ws", get(pane_ws))
        .route("/api/v1/panes/{id}/image", post(chat_image))
        .route("/api/v1/projects/{project}/uploads", post(task_file))
        .route("/api/v1/projects/{project}/library-uploads/{upload}", post(library_upload))
        .route("/api/v1/projects/{project}/uploads/{name}", get(task_file_get))
        .route("/api/v1/projects/{project}/design/{repo}/{*path}", get(design_file_get))
        .route("/api/v1/projects/{project}/design-files/{repo}/{branch}", post(design_file_post))
        .route("/api/v1/projects/{project}/questions/{thread}/{hash}/{*path}", get(question_file_get))
        .route("/api/v1/projects/{project}/requests/{item}/{hash}/{*path}", get(request_file_get))
        .route("/api/v1/projects/{project}/asks/{id}/{*path}", get(ask_file_get))
        .route("/v/{cap}/{*path}", get(view_file))
        .route("/v/{cap}/", get(view_root))
        .route("/v", get(|| async { StatusCode::NOT_FOUND }))
        .route("/v/", get(|| async { StatusCode::NOT_FOUND }))
        .route("/f/{cap}/{*name}", get(pane_file_view))
        .fallback(get(static_file))
        .layer(middleware::from_fn_with_state(host.clone(), guard))
        .with_state(host)
}

pub(crate) fn host_header(h: &HeaderMap) -> String {
    h.get(header::HOST)
        .and_then(|v| {
            v.to_str()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .unwrap_or("")
        .to_string()
}

async fn install_probe(State(host): State<Arc<Host>>, Path(challenge): Path<String>) -> Response {
    match host.install_probe(&challenge) {
        Some(proof) => (StatusCode::OK, proof).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.starts_with("127.0.0.1:") || host.starts_with("localhost:") || host.starts_with("[::1]:")
}

/// 요청이 HTTPS(serve) 경유인가: 브라우저가 보는 주소가 loopback이 아니거나 proxy가 https라고 알린다.
pub(crate) fn via_https(h: &HeaderMap) -> bool {
    let proto = h.get("x-forwarded-proto").and_then(|v| {
        v.to_str()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
    });
    proto == Some("https") || !is_loopback_host(&host_header(h))
}

#[derive(Clone, Copy)]
struct PaneIngress {
    started: std::time::Instant,
    origin_us: u64,
}

async fn guard(State(host): State<Arc<Host>>, mut req: Request, next: Next) -> Response {
    let started = std::time::Instant::now();
    let h = req.headers();
    // Host는 이 host의 두 주소 중 하나(포트까지), Origin은 그 주소와 정확히 같아야 한다: 같은 이름의 다른 포트
    // (같은 머신의 다른 웹 서비스)나 포트 없는 Origin도 남이다(herdr-web-ui `sameOrigin`, t3code `EnvironmentAuth`)
    let Some(expected) = host.web_origin(&host_header(h)) else {
        return (StatusCode::MISDIRECTED_REQUEST, "알 수 없는 Host").into_response();
    };
    let origin_us = started.elapsed().as_micros() as u64;
    let is_ws = h
        .get(header::UPGRADE)
        .and_then(|v| {
            v.to_str()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if !matches!(*req.method(), Method::GET | Method::HEAD) || is_ws {
        let origin = h
            .get(header::ORIGIN)
            .and_then(|v| {
                v.to_str()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .unwrap_or("");
        if !origin.eq_ignore_ascii_case(&expected) {
            return (StatusCode::FORBIDDEN, "허용하지 않은 Origin").into_response();
        }
    }
    let api = req.uri().path().starts_with("/api/");
    let library = req.uri().path().starts_with("/v/");
    // opaque 보고서는 앱 API를 읽지 못한다. 쿠키 정책에만 기대지 않는다.
    if api && h.get(header::ORIGIN).is_some_and(|v| v == "null") {
        return StatusCode::FORBIDDEN.into_response();
    }
    if is_ws && req.uri().path().starts_with("/api/v1/panes/") {
        req.extensions_mut().insert(PaneIngress { started, origin_us });
    }
    let mut res = next.run(req).await;
    let hs = res.headers_mut();
    // JSON 자체는 UTF-8이지만 Safari의 직접 열기에서도 한글 오류를 올바르게 표시한다.
    if hs.get(header::CONTENT_TYPE).is_some_and(|v| v == "application/json") {
        hs.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    }
    // 기기·pane 상태를 담은 API 답은 브라우저·proxy가 두지 않는다(herdr-web-ui `index.ts` API no-store)
    if api {
        hs.entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-store"));
    }
    // /v/의 성공·오류·Range 응답 모두에 강제한다. handler 머리가 빠져도 앱 origin으로 실행하지 않는다.
    if library {
        let pdf = hs.get(header::CONTENT_TYPE).is_some_and(|v| v == "application/pdf");
        hs.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static(if pdf {
            PDF_CSP
        } else {
            VIEW_CSP
        }));
        hs.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    } else {
        hs.entry(header::CONTENT_SECURITY_POLICY).or_insert(HeaderValue::from_static(CSP));
    }
    hs.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    hs.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    hs.entry(header::X_FRAME_OPTIONS).or_insert(HeaderValue::from_static("DENY"));
    res
}

/// 장치 cookie 이름. cookie는 포트를 가리지 않아 같은 hostname의 두 인스턴스가 한 이름을 쓰면 서로의 로그인을 덮는다.
/// 그래서 인스턴스는 이름에 꼬리를 단다(`__Host-aaa-work`, plan §8.294). 기본 인스턴스는 지금 이름 그대로다
pub(crate) fn cookie_name(https: bool) -> String {
    cookie_name_in(aaa_config::paths::instance().as_deref(), https)
}

fn cookie_name_in(instance: Option<&str>, https: bool) -> String {
    let base = aaa_config::paths::named("aaa", instance);
    if https { format!("__Host-{base}") } else { base }
}

/// 장치 cookie. HTTPS에서는 `__Host-aaa`만 본다: prefix 없는 `aaa`는 같은 이름의 다른 서비스(다른 포트)나
/// 하위 경로가 심을 수 있다. `=`이 없는 등 깨진 항목은 건너뛴다(herdr-web-ui `parseCookies`: 남의 cookie 하나가
/// 주인을 막지 않는다)
pub(crate) fn cookie_token(h: &HeaderMap) -> Option<String> {
    let name = cookie_name(via_https(h));
    for v in h.get_all(header::COOKIE) {
        let Ok(s) = v
            .to_str()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            continue;
        };
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

fn error_response(e: ErrorBody) -> Response {
    let status = match e.code {
        ErrorCode::Forbidden => StatusCode::FORBIDDEN,
        ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    };
    (status, Json(Outcome::<()>::Err(e))).into_response()
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
    /// 이 host의 FRONT_API(1.58부터). 웹은 옛 host가 버리는 요청 칸(변환 미리보기)을 이것으로 고른다.
    front_api: String,
    /// 공개 주소로 연 화면의 passkey 상태(관리 GUI W7). loopback 화면은 없다: 승인에 passkey가 필요 없다
    #[serde(skip_serializing_if = "Option::is_none")]
    passkey: Option<aaa_proto::PasskeyState>,
}

/// passkey 판정에 쓰는 이 화면의 주소(`guard`가 이미 Host를 확인했다). tailscale serve는 받은 Host를 그대로 넘기므로, serve를
/// 거친 요청(serve가 붙이는 X-Forwarded-For·X-Forwarded-Proto)은 Host가 loopback이어도 host Mac 화면이 아니다: 공개 주소로 본다.
/// 공개 주소를 아직 모르면 어떤 passkey와도 맞지 않는 이름으로 둔다(승인 거부)
fn origin(host: &Host, h: &HeaderMap) -> String {
    let o = host.web_origin(&host_header(h)).unwrap_or_default();
    let forwarded = h.contains_key("x-forwarded-for") || h.contains_key("x-forwarded-proto");
    if forwarded && crate::passkey::rp_of(&o).is_none() {
        return host.public_origin().unwrap_or_else(|| "https://serve.invalid".into());
    }
    o
}

async fn pair(State(host): State<Arc<Host>>, headers: HeaderMap, Json(b): Json<PairBody>) -> Response {
    match host.redeem_pairing(&b.code, &b.label) {
        Ok((token, role)) => {
            let secure = via_https(&headers);
            let name = cookie_name(secure);
            let cookie = if secure {
                format!("{name}={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=31536000")
            } else {
                format!("{name}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age=31536000")
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
        format!("{}=; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=0", cookie_name(true)),
        format!("{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0", cookie_name(false)),
    ] {
        res.headers_mut().append(header::SET_COOKIE, HeaderValue::from_str(&c).unwrap());
    }
    res
}

async fn me(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    // 시험에서만(debug 빌드 `AAA_TEST_WEB_HOLD_ME`): 실행 신원은 맞는데 웹이 답하지 않는 새 판을 흉내 낸다(W6c 감시 m13)
    if cfg!(debug_assertions) && std::env::var_os("AAA_TEST_WEB_HOLD_ME").is_some() {
        std::future::pending::<()>().await;
    }
    match device(&host, &headers) {
        Some((id, role)) => {
            let passkey = host.passkey_state(&id, role, &origin(&host, &headers));
            Json(Outcome::Ok(Me { role, device: id, front_api: aaa_proto::FRONT_API.to_string(), passkey }))
                .into_response()
        }
        None => unauthorized(),
    }
}

async fn app_badge(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    if device(&host, &headers).is_none() {
        return unauthorized();
    }
    Json(Outcome::from(host.app_badge_count())).into_response()
}

async fn call(State(host): State<Arc<Host>>, headers: HeaderMap, Json(req): Json<FrontRequest>) -> Response {
    let Some((id, role)) = device(&host, &headers) else { return unauthorized() };
    // 공개 주소의 조작 기기: 승인 필요 동작은 passkey 승인(`/api/v1/passkey/approve`)으로만 한다(관리 GUI W7)
    if let Err(e) = host.passkey_gate(&id, role, &origin(&host, &headers), &req) {
        return Json(Outcome::<()>::Err(e)).into_response();
    }
    let result = host.call(role, &format!("device:{id}"), req).await.map(|mut response| {
        attachment_views(&host, &id, &mut response);
        response
    });
    api_json(&headers, &Outcome::from(result))
}

/// 이보다 작은 API 답은 압축하지 않는다(조작 답은 대부분 작고, 압축 머리가 득보다 크다).
const API_GZIP_MIN: usize = 1024;

/// API 답 JSON. 큰 답(thread·답할 것·할 일 목록은 수백 KB~수 MB)은 브라우저가 받는다고 한 gzip으로 보낸다(t-1767).
/// 정적 자산·보고서 보기와 같은 협상이다. 압축하지 못하면 원문으로 답한다.
fn api_json<T: serde::Serialize>(headers: &HeaderMap, value: &T) -> Response {
    use std::io::Write as _;
    let bytes = match serde_json::to_vec(value) {
        Ok(bytes) => bytes,
        Err(error) => {
            aaa_common::diagnostic::observe(&error, "API 답 직렬화 실패는 기존 오류 응답을 따른다");
            return Json(value).into_response();
        }
    };
    let json = [(header::CONTENT_TYPE, "application/json")];
    if bytes.len() < API_GZIP_MIN {
        return (json, bytes).into_response();
    }
    let vary = [(header::VARY, "Accept-Encoding")];
    if !assets::accepts_gzip(headers) {
        return (json, vary, bytes).into_response();
    }
    let mut encoder = flate2::write::GzEncoder::new(Vec::with_capacity(bytes.len() / 4), flate2::Compression::fast());
    match encoder.write_all(&bytes).and_then(|()| encoder.finish()) {
        Ok(body) => (json, vary, [(header::CONTENT_ENCODING, "gzip")], body).into_response(),
        Err(error) => {
            aaa_common::diagnostic::observe(&error, "API 답 gzip 실패 뒤 원문으로 답한다");
            (json, vary, bytes).into_response()
        }
    }
}

async fn passkey_register_begin(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    let Some((id, role)) = device(&host, &headers) else { return unauthorized() };
    Json(Outcome::from(host.passkey_register_begin(&id, role, &origin(&host, &headers)))).into_response()
}

async fn passkey_register(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    Json(b): Json<aaa_proto::PasskeyRegistration>,
) -> Response {
    let Some((id, role)) = device(&host, &headers) else { return unauthorized() };
    Json(Outcome::from(host.passkey_register(&id, role, &origin(&host, &headers), b))).into_response()
}

async fn passkey_approve_begin(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    Json(req): Json<FrontRequest>,
) -> Response {
    let Some((id, role)) = device(&host, &headers) else { return unauthorized() };
    Json(Outcome::from(host.passkey_approve_begin(&id, role, &origin(&host, &headers), req))).into_response()
}

async fn passkey_approve(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    Json(b): Json<aaa_proto::PasskeyAssertion>,
) -> Response {
    let Some((id, role)) = device(&host, &headers) else { return unauthorized() };
    Json(Outcome::from(host.passkey_approve(&id, role, &origin(&host, &headers), b).await)).into_response()
}

fn bad_request(msg: impl Into<String>) -> Response {
    (StatusCode::BAD_REQUEST, Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::BadRequest, msg)))).into_response()
}

#[cfg(test)]
mod content_type_tests;

/// 입구 검사(t-0340 보안 감사 A1·A4·A5·A8·A10): router를 그대로 부른다. 공개 주소는 같은 이름의 8443이고, 같은
/// 머신 443에 다른 웹 서비스가 있는 운영 배치를 흉내 낸다.
#[cfg(test)]
mod guard_tests;

#[cfg(test)]
mod submit_tests;
