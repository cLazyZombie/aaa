//! host 웹(기획서 §6.3·§6.4). `127.0.0.1:<web_port>`의 HTTP·WS로만 열고, 다른 기기에는 `tailscale serve` HTTPS로 공개한다.
//!
//! - 입장: aaa pairing 코드 → 장치 token cookie(HttpOnly·SameSite=Strict, HTTPS면 Secure·`__Host-`). loopback은 인증 신호가 아니다.
//! - Host 헤더 허용 목록(DNS rebinding), 변경 요청·WebSocket의 Origin 검사, CSP.
//! - `/api/v1/call`은 Front API 요청을 그대로 받는다(같은 타입, 같은 권한 판정).
//! - 옛 TUI 전용 `/api/v1/front`는 인증 뒤 삭제 안내로 거절한다.

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
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;

use super::{FrontRef, Host, PaneEvent};
use crate::chatwatch::ChatNote;
use crate::output::{OUTPUT_HARD_BYTES, OUTPUT_STALLED_CLOSE_CODE, OutputWindow, Verdict, check_unacked};

mod assets {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

const OUTPUT_STALLED_REASON: &str = "terminal output consumer stalled";
/// 기기를 취소해 닫은 WebSocket. 웹은 다시 붙지 않고 pairing 화면으로 간다(web/src/api.ts `REVOKED_CLOSE_CODE`)
pub const DEVICE_REVOKED_CLOSE_CODE: u16 = 4401;
const DEVICE_REVOKED_REASON: &str = "device revoked";

const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
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
            let _ = axum::serve(listener, router(host)).await;
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
    h.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("").to_string()
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
    let library = req.uri().path().starts_with("/v/");
    // opaque 보고서는 앱 API를 읽지 못한다. 쿠키 정책에만 기대지 않는다.
    if api && h.get(header::ORIGIN).is_some_and(|v| v == "null") {
        return StatusCode::FORBIDDEN.into_response();
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

/// 파일 보기 응답(herdr-web-ui `fileResponse`): 이 origin에서 연 HTML·SVG가 앱 권한으로 script를 돌리지 못하게 sandbox.
/// PDF는 브라우저 자체 viewer가 sandbox 문서를 거부하므로 뺀다(그 script는 viewer 안에 머문다)
const FILE_CSP: &str = "sandbox; default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'";
const PDF_CSP: &str = "frame-ancestors 'self'";
/// 페이지로 그린 HTML(참고 `PAGE_POLICY`): 제 script는 돌지만 origin 없는 sandbox라 이 앱의 cookie·저장소가 없고,
/// `connect-src`가 없어 이 서버에도 웹에도 묻지 못한다. 제 폴더 파일은 agent가 미리 넣었다(data:). 이 앱만 frame에 넣는다.
const PAGE_CSP: &str = "sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' data:; \
                        style-src 'unsafe-inline' data:; img-src data: blob:; media-src data: blob:; font-src data:; \
                        base-uri 'none'; form-action 'none'; frame-ancestors 'self'";

#[derive(serde::Deserialize)]
struct FileQuery {
    download: Option<String>,
    /// HTML 파일을 페이지로(`PAGE_CSP`). 다른 파일은 본다
    render: Option<String>,
}

/// `/f/<cap>/<이름>`: 파일 보기 읽기 경로(plan §8.125). 그 파일 하나만 30분 읽고, 짝지은 조작 기기여야 한다. Range를 받는다.
async fn pane_file_view(
    State(host): State<Arc<Host>>,
    Path((cap, _name)): Path<(String, String)>,
    Query(q): Query<FileQuery>,
    headers: HeaderMap,
) -> Response {
    let text = |code: StatusCode, msg: String| {
        (code, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], msg).into_response()
    };
    // 다른 site의 페이지가 그림·script·fetch로 이 파일을 읽으려는 것(참고 `crossSiteFileRead`). 새 탭·내려받기(navigate)는 된다
    let fetch = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    if fetch("sec-fetch-site").is_some_and(|s| s != "same-origin" && s != "none")
        && fetch("sec-fetch-mode") != Some("navigate")
    {
        return text(StatusCode::FORBIDDEN, "다른 site에서 읽을 수 없다".into());
    }
    // 읽기 경로만으로는 열지 않는다: 그 주소가 새어도 짝지은 조작 기기가 아니면 못 읽는다(참고도 기기 인증 뒤다)
    match device(&host, &headers) {
        None => return text(StatusCode::UNAUTHORIZED, "pairing이 필요하다".into()),
        Some((_, DeviceRole::Reader)) => {
            return text(StatusCode::FORBIDDEN, "보기 권한 기기는 파일을 열 수 없다".into());
        }
        Some(_) => {}
    }
    let (conn, info) = match host.file_cap(&cap) {
        Ok(x) => x,
        Err(e) => {
            let code =
                if e.code == ErrorCode::Unauthorized { StatusCode::UNAUTHORIZED } else { StatusCode::BAD_GATEWAY };
            return text(code, e.message);
        }
    };
    let disposition = if q.download.is_some() { "attachment" } else { "inline" };
    let name = format!("{disposition}; filename*=UTF-8''{}", crate::files::pct(&info.name));
    let path: Arc<str> = Arc::from(info.path.as_str());
    // HTML 페이지: 그린 bytes는 길이를 미리 모르고 Range도 받지 않는다
    if q.render.is_some() && q.download.is_none() && info.kind == aaa_proto::FileKind::Text && info.mime == "text/html"
    {
        let first = match crate::files::read_at(&conn, &info.path, 0, u64::MAX, true).await {
            Ok(b) => b,
            Err(e) => return text(StatusCode::NOT_FOUND, format!("파일을 읽지 못했다: {}", e.message)),
        };
        return Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CONTENT_DISPOSITION, name)
            .header(header::CACHE_CONTROL, "private, no-store")
            .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
            .header(header::CONTENT_SECURITY_POLICY, PAGE_CSP)
            .header(header::X_FRAME_OPTIONS, "SAMEORIGIN")
            .body(Body::from_stream(crate::files::file_stream(conn, path, 0, u64::MAX, first, true)))
            .unwrap();
    }
    let size = info.size;
    let range = match crate::files::parse_range(fetch("range"), size) {
        Ok(r) => r,
        Err(()) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .unwrap();
        }
    };
    let (start, len) = range.map(|(a, b)| (a, b - a + 1)).unwrap_or((0, size));
    // 응답 전에 첫 조각을 읽어, 그 사이 지워졌거나 바뀐 파일은 오류로 끝낸다
    let first = if len == 0 {
        (Vec::new(), true)
    } else {
        match crate::files::read_at(&conn, &info.path, start, len, false).await {
            Ok(b) => b,
            Err(e) => return text(StatusCode::NOT_FOUND, format!("파일을 읽지 못했다: {}", e.message)),
        }
    };
    // 글은 확장자가 무엇으로 돈다고 하든 글로 보낸다
    let kind = if info.kind == aaa_proto::FileKind::Text && info.mime != "image/svg+xml" {
        "text/plain; charset=utf-8".to_string()
    } else {
        info.mime.clone()
    };
    let mut res = Response::builder()
        .status(if range.is_some() { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK })
        .header(header::CONTENT_TYPE, kind)
        .header(header::CONTENT_LENGTH, len)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_DISPOSITION, name)
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::X_FRAME_OPTIONS, "SAMEORIGIN");
    if let Some((a, b)) = range {
        res = res.header(header::CONTENT_RANGE, format!("bytes {a}-{b}/{size}"));
    }
    // PDF는 브라우저의 PDF 보기가 그리는데, 그것은 sandbox 문서를 거부한다(참고와 같다). 그 script는 그 보기 안에 머문다.
    // 비워 두면 앱 기본 CSP(`frame-ancestors 'none'`)가 붙어 파일 보기 iframe에 들지 못하므로 이 앱만 frame에 넣게 한다
    let csp = if info.kind == aaa_proto::FileKind::Pdf { PDF_CSP } else { FILE_CSP };
    res = res.header(header::CONTENT_SECURITY_POLICY, csp);
    res.body(Body::from_stream(crate::files::file_stream(conn, path, start, len, first, false))).unwrap()
}

/// 결과물: opaque origin(sandbox)으로 보여 제어 UI의 cookie·API에 닿지 못하게 한다. 같은 host의 웹 화면 안 iframe에만 넣을 수 있다.
const VIEW_CSP: &str = "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox allow-downloads; \
                        default-src 'none'; script-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob:; \
                        style-src 'self' 'unsafe-inline' data:; img-src 'self' data: blob:; \
                        media-src 'self' data: blob:; font-src 'self' data:; \
                        connect-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'";

/// capability가 가리키는 보고서 폴더만 읽는다. 끝의 /는 형제 cap·API와의 접두사 충돌을 막는다.
/// /v/는 redirect하지 않는다(CSP는 redirect 뒤 경로를 비교하지 않으므로 그 계약이 필요하다).
fn view_csp(origin: &str, cap: &str) -> HeaderValue {
    let folder = format!("{origin}/v/{}/", crate::files::pct(cap));
    HeaderValue::from_str(
        &VIEW_CSP
            .replace("'self'", &folder)
            .replace(&format!("frame-ancestors {folder}"), "frame-ancestors 'self'")
            .replace("connect-src 'none'", &format!("connect-src {folder}")),
    )
    .expect("검증한 웹 origin과 percent encoding은 안전한 헤더다")
}

/// 보고서 HTML 끝에 붙이는 그림 누름 알림(plan §8.353). 그 문서와 같은 sandbox 안에서 돌고, 부모(앱)에게 누른 그림과 그 문서의
/// 그림 주소만 알린다. 앱은 그 iframe에서 온, 그 보고서 `/v/<cap>/` 아래 주소만 받는다(web `reportImages.ts`)
const REPORT_IMAGES: &str = concat!("<script>", include_str!("report_images.js"), "</script>");

/// 알림을 붙이는 응답: HTML을 문서·iframe으로 여는 전체 응답만. Range(파일 보기의 앞부분 읽기)·내려받기·fetch·HEAD는 원본이다
fn report_images_bridge(kind: &str, range: bool, download: bool, navigation: bool) -> bool {
    kind.starts_with("text/html") && !range && !download && navigation
}

async fn view_root(
    State(host): State<Arc<Host>>,
    Path(cap): Path<String>,
    Query(q): Query<FileQuery>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    serve_view(host, cap, String::new(), q, headers, method).await
}

/// `/v/<cap>/<path>`: 보기 경로가 곧 권한이다(그 thread 결과물 읽기만, 2시간). host 사본이 없으면 agent에서 스트리밍한다.
/// 파일 보기(차이 목록 R1)가 같은 경로로 읽는다: Range(글 앞부분·동영상 넘기기)와 `?download=1`(내려받기)을 받는다.
async fn view_file(
    State(host): State<Arc<Host>>,
    Path((cap, path)): Path<(String, String)>,
    Query(q): Query<FileQuery>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    serve_view(host, cap, path, q, headers, method).await
}

/// 브라우저 문서 탐색만 열람이다. cookie 없는 외부 탭도 포함하며 fetch·그림·미리보기·HEAD는 제외한다.
fn report_navigation(method: &Method, headers: &HeaderMap) -> bool {
    let value = |key: &str| headers.get(key).and_then(|v| v.to_str().ok()).unwrap_or("");
    method == Method::GET
        && value("sec-fetch-mode") == "navigate"
        && matches!(value("sec-fetch-dest"), "document" | "iframe" | "frame")
        && !["purpose", "sec-purpose", "x-purpose"].iter().any(|key| {
            let purpose = value(key).to_ascii_lowercase();
            ["prefetch", "preview", "prerender"].iter().any(|p| purpose.contains(p))
        })
}

async fn serve_view(
    host: Arc<Host>,
    cap: String,
    path: String,
    q: FileQuery,
    headers: HeaderMap,
    method: Method,
) -> Response {
    // sandbox의 CORS 요청은 Origin: null이다. 쿠키를 동반한 읽기는 허용하지 않는다.
    let opaque = headers.get(header::ORIGIN).is_some_and(|v| v == "null");
    if opaque && headers.contains_key(header::COOKIE) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let fail = |e: ErrorBody| {
        let code = match e.code {
            ErrorCode::Unauthorized => StatusCode::UNAUTHORIZED,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::BadRequest => StatusCode::BAD_REQUEST,
            _ => StatusCode::BAD_GATEWAY,
        };
        (code, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], e.message).into_response()
    };
    let file = match host.view_source(&cap, &path).await {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let src = file.source;
    let size = src.size();
    let generated = matches!(src, crate::library::ViewSource::Page(_));
    let range = match crate::files::parse_range(headers.get(header::RANGE).and_then(|v| v.to_str().ok()), size) {
        Ok(r) => r,
        Err(()) => {
            return Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .unwrap();
        }
    };
    let (start, len) = range.map(|(a, b)| (a, b - a + 1)).unwrap_or((0, size));
    let body = match src.open(start, len).await {
        Ok(b) => b,
        Err(e) => return fail(e),
    };
    let kind =
        if generated || path.is_empty() { "text/html; charset=utf-8" } else { crate::library::content_type(&path) };
    let navigation = report_navigation(&method, &headers);
    // 그림 누름 알림(plan §8.353): HTML 문서 탐색의 전체 응답 끝에만 붙인다. 원본 바이트·Range·내려받기·fetch(소스 보기)는 그대로다
    let tail: &[u8] = if report_images_bridge(kind, range.is_some(), q.download.is_some(), navigation) {
        REPORT_IMAGES.as_bytes()
    } else {
        &[]
    };
    let body: crate::library::ByteStream = if tail.is_empty() {
        body
    } else {
        use futures_util::StreamExt;
        body.chain(futures_util::stream::once(async move { Ok(tail.to_vec()) })).boxed()
    };
    // PDF는 브라우저의 PDF 보기가 그리는데 그것은 sandbox 문서를 거부한다(`/f/`의 `PDF_CSP`와 같은 까닭)
    let Some(origin) = host.web_origin(&host_header(&headers)) else {
        return StatusCode::MISDIRECTED_REQUEST.into_response();
    };
    let csp = if kind == "application/pdf" { HeaderValue::from_static(PDF_CSP) } else { view_csp(&origin, &cap) };
    let mut res = Response::builder()
        .status(if range.is_some() { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK })
        .header(header::CONTENT_TYPE, kind)
        .header(header::CONTENT_LENGTH, len + tail.len() as u64)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::CONTENT_SECURITY_POLICY, csp)
        .header(header::X_FRAME_OPTIONS, "SAMEORIGIN");
    if opaque {
        // credential 허용·preflight 허용은 주지 않는다. 유효한 cap의 GET/HEAD 성공 응답만 공유한다.
        res = res.header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "null");
    }
    if let Some((a, b)) = range {
        res = res.header(header::CONTENT_RANGE, format!("bytes {a}-{b}/{size}"));
    }
    if q.download.is_some() {
        let name = path.rsplit('/').next().unwrap_or(&path);
        res = res
            .header(header::CONTENT_DISPOSITION, format!("attachment; filename*=UTF-8''{}", crate::files::pct(name)));
    }
    if q.download.is_none() && navigation {
        host.library_opened(&file.project, &file.name);
    }
    res.body(Body::from_stream(body)).unwrap()
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

/// task 첨부(plan §8.327, 그림·md·html 문서): 프로젝트 uploads/에 쓰고 메모에 적을 `uploads/<이름>`을 돌려준다. 대화 첨부와 같이 인증·권한을
/// 확인한 뒤 원본 bytes를 16MiB 한도로 읽는다. 메모 줄은 웹이 `task_edit`로 쓴다
#[derive(serde::Deserialize)]
struct TaskFileQuery {
    /// 문서 종류(md·html). 없으면 그림이다
    kind: Option<String>,
}

async fn task_file(
    State(host): State<Arc<Host>>,
    Path(project): Path<String>,
    Query(q): Query<TaskFileQuery>,
    req: Request,
) -> Response {
    let Some((id, role)) = device(&host, req.headers()) else { return unauthorized() };
    if !role.can_control() {
        return error_response(ErrorBody::new(ErrorCode::Forbidden, "보기 권한으로는 첨부할 수 없다"));
    }
    let Ok(bytes) = axum::body::to_bytes(req.into_body(), crate::task_attach::MAX_BYTES as usize).await else {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::BadRequest, "파일은 16MiB까지 첨부한다"))),
        )
            .into_response();
    };
    match host.task_file_save(role, &format!("device:{id}"), &project, q.kind.as_deref(), &bytes) {
        Ok(path) => Json(Outcome::Ok(serde_json::json!({ "path": path }))).into_response(),
        Err(e) => error_response(e),
    }
}

/// 웹 「보고서 올리기」(관리 GUI W5b, plan §8.355): 파일 하나를 올리기 폴더의 `path`에 쓴다(16MiB). 다 올리면 웹이
/// `library_upload_publish`로 등록한다. 돌려주는 것은 그 올리기의 파일 수·합계
#[derive(serde::Deserialize)]
struct LibraryUploadQuery {
    path: String,
}

async fn library_upload(
    State(host): State<Arc<Host>>,
    Path((project, upload)): Path<(String, String)>,
    Query(q): Query<LibraryUploadQuery>,
    req: Request,
) -> Response {
    let Some((_, role)) = device(&host, req.headers()) else { return unauthorized() };
    if !role.can_control() {
        return error_response(ErrorBody::new(ErrorCode::Forbidden, "보기 권한으로는 보고서를 올릴 수 없다"));
    }
    let Ok(bytes) = axum::body::to_bytes(req.into_body(), crate::library_upload::FILE_MAX as usize).await else {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::TooLarge, "파일 하나는 16MiB까지 올린다"))),
        )
            .into_response();
    };
    match host.library_upload_put(role, &project, &upload, &q.path, &bytes) {
        Ok((files, total)) => Json(Outcome::Ok(serde_json::json!({ "files": files, "total": total }))).into_response(),
        Err(e) => error_response(e),
    }
}

/// task 첨부 파일(보기 권한도). 이름이 내용 hash라 바뀌지 않아 오래 둔다. 올린 HTML은 결과물처럼 sandbox(opaque origin)로 보내
/// 앱 cookie·API에 닿지 못하게 한다
async fn task_file_get(
    State(host): State<Arc<Host>>,
    Path((project, name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Response {
    if device(&host, &headers).is_none() {
        return unauthorized();
    }
    match host.task_file_read(&project, &name) {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, crate::task_attach::mime(&name))
            .header(header::CACHE_CONTROL, "private, max-age=31536000, immutable")
            .header("x-content-type-options", "nosniff")
            .header(header::CONTENT_SECURITY_POLICY, VIEW_CSP)
            .header(header::X_FRAME_OPTIONS, "SAMEORIGIN")
            .body(Body::from(bytes))
            .unwrap(),
        Err(e) => error_response(e),
    }
}

/// 「답할 것」 질문 본문의 그림·링크(plan §8.345, 보기 권한도): 열린 질문은 그 보고의 host 사본, 답한 것은 보존한 사본에서 준다.
/// HTML은 결과물처럼 sandbox(opaque origin)로 보내 앱 cookie·API에 닿지 못하게 한다
fn ask_file_response(
    host: &Host,
    headers: &HeaderMap,
    path: Result<std::path::PathBuf, ErrorBody>,
    name: &str,
) -> Response {
    if device(host, headers).is_none() {
        return unauthorized();
    }
    let bytes = path.and_then(|p| std::fs::read(p).map_err(|_| ErrorBody::new(ErrorCode::NotFound, "파일이 없다")));
    match bytes {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, crate::library::content_type(name))
            .header(header::CACHE_CONTROL, "private, no-cache")
            .header("x-content-type-options", "nosniff")
            .header(header::CONTENT_SECURITY_POLICY, VIEW_CSP)
            .header(header::X_FRAME_OPTIONS, "SAMEORIGIN")
            .body(Body::from(bytes))
            .unwrap(),
        Err(e) => error_response(e),
    }
}

async fn question_file_get(
    State(host): State<Arc<Host>>,
    Path((project, thread, hash, path)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let file = host.question_file(&project, &thread, &hash, &path);
    ask_file_response(&host, &headers, file, &path)
}

async fn ask_file_get(
    State(host): State<Arc<Host>>,
    Path((project, id, path)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let file = host.ask_file(&project, &id, &path);
    ask_file_response(&host, &headers, file, &path)
}

async fn request_file_get(
    State(host): State<Arc<Host>>,
    Path((project, item, hash, path)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let file = host.request_file(&project, &item, &hash, &path);
    ask_file_response(&host, &headers, file, &path)
}

/// 참고 `/api/pane/image`: 저장만 하고 경로를 돌려준다. Origin·cookie는 기존 웹 guard와 같다.
/// 인증·권한을 확인한 뒤 본문을 읽으며, base64 대신 원본 bytes를 16MiB 한도로 받는다.
async fn chat_image(State(host): State<Arc<Host>>, Path(pane): Path<String>, req: Request) -> Response {
    let Some((id, role)) = device(&host, req.headers()) else { return unauthorized() };
    if !role.can_control() {
        return (
            StatusCode::FORBIDDEN,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Forbidden, "보기 권한으로는 붙여 넣을 수 없다"))),
        )
            .into_response();
    }
    let Ok(bytes) = axum::body::to_bytes(req.into_body(), 16 << 20).await else {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::BadRequest, "그림은 16MiB까지 붙여 넣는다"))),
        )
            .into_response();
    };
    match host.chat_image(role, &format!("device:{id}"), &pane, &bytes).await {
        Ok(path) => Json(Outcome::Ok(serde_json::json!({ "path": path }))).into_response(),
        Err(e) => error_response(e),
    }
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
    Json(Outcome::from(result)).into_response()
}

/// 인증한 웹 기기에만 질문/hash·답한 사본에 묶인 짧은 읽기 경로를 발급한다.
/// opaque iframe의 하위 요청·새 탭·Range가 기기 cookie에 의존하지 않는다.
fn attachment_base(host: &Host, device: &str, base: &mut Option<String>) {
    use crate::library::{Attachment, VIEW_TTL, ViewCap};
    use std::time::Instant;
    let Some(raw) = base.as_deref().and_then(|b| b.strip_prefix("/api/v1/projects/")) else { return };
    let parts: Vec<_> = raw.trim_end_matches('/').split('/').collect();
    let (project, id, attachment) = match parts.as_slice() {
        [p, "questions", id, hash] => (*p, *id, Attachment::Question { hash: (*hash).into() }),
        [p, "requests", id, hash] => (*p, *id, Attachment::Request { hash: (*hash).into() }),
        [p, "asks", id] => (*p, *id, Attachment::Answer),
        _ => return,
    };
    let mut caps = host.view_caps.lock().unwrap();
    caps.retain(|_, c| c.until > Instant::now());
    let cap = caps
        .iter()
        .find(|(_, c)| {
            c.project == project
                && c.thread == id
                && c.device.as_deref() == Some(device)
                && c.attachment.as_ref() == Some(&attachment)
        })
        .map(|(key, _)| key.clone())
        .unwrap_or_else(|| {
            let cap = hex::encode(rand::random::<[u8; 24]>());
            caps.insert(
                cap.clone(),
                ViewCap {
                    project: project.into(),
                    thread: id.into(),
                    until: Instant::now() + VIEW_TTL,
                    device: Some(device.into()),
                    attachment: Some(attachment),
                },
            );
            cap
        });
    *base = Some(format!("/v/{cap}/"));
}

fn attachment_thread(host: &Host, device: &str, thread: &mut aaa_proto::ThreadView) {
    if let Some(q) = &mut thread.questions {
        attachment_base(host, device, &mut q.files_base);
    }
}

fn attachment_views(host: &Host, device: &str, response: &mut aaa_proto::FrontResponse) {
    use aaa_proto::FrontResponse;
    match response {
        FrontResponse::Thread(t) => attachment_thread(host, device, t),
        FrontResponse::Threads { threads } => {
            for t in threads {
                attachment_thread(host, device, t);
            }
        }
        FrontResponse::Asks { requests, done, .. } => {
            for request in requests {
                if let Some(q) = &mut request.questions {
                    attachment_base(host, device, &mut q.files_base);
                }
            }
            for record in done {
                attachment_base(host, device, &mut record.files_base);
            }
        }
        _ => {}
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
    /// 이 장치 구독이 고른 알림 종류(차이 목록 N3, 구독이 없으면 없음)
    alerts: Option<crate::db::Alerts>,
    /// 이 장치에 최근 push로 보낸 항목(오래된 것부터). 깨어난 페이지는 이것으로 이미 받은 알림을 다시 띄우지 않는다(plan §8.141)
    pushed: Vec<crate::push::PushedItem>,
}

/// Web Push(§8.26): 키와 이 장치의 구독 상태. 보기 권한 장치도 알림은 받는다.
async fn push_info(State(host): State<Arc<Host>>, headers: HeaderMap) -> Response {
    let Some((id, _)) = device(&host, &headers) else { return unauthorized() };
    let mine = host.db.push_list(Some(&id)).unwrap_or_default();
    let subscribed = mine.len();
    let alerts = mine.first().and_then(|s| s.alerts);
    let key = host.vapid.as_ref().map(|v| v.public_b64());
    let in_use = host.in_use.load(std::sync::atomic::Ordering::SeqCst);
    let (user, you) = {
        let v = host.viewing.lock().unwrap();
        (v.user(), v.is_user(&id))
    };
    let kinds = host.cfg.notify.item_kinds();
    let pushed = host.pushed.lock().unwrap().of(&id);
    Json(Outcome::Ok(PushInfo { enabled: key.is_some(), key, subscribed, in_use, user, you, kinds, alerts, pushed }))
        .into_response()
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
    /// 이 기기가 받을 알림(차이 목록 N3). 없으면 전에 고른 것을 둔다
    #[serde(default)]
    alerts: Option<crate::db::Alerts>,
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
    let sub = crate::db::PushSub {
        endpoint: b.endpoint,
        p256dh: b.keys.p256dh,
        auth: b.keys.auth,
        device: id.clone(),
        alerts: b.alerts,
    };
    match host.db.push_put(&sub) {
        Ok(true) => {}
        Ok(false) => {
            host.db.audit(&format!("device:{id}"), "push_subscribe", "다른 기기의 구독이라 거절");
            return (
                StatusCode::FORBIDDEN,
                Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Forbidden, "다른 기기의 구독이다"))),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Internal, e.to_string()))),
            )
                .into_response();
        }
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

/// 등록 계정의 창별 사용량. 메일·원문 응답·인증 경로는 이 응답에 없다.
async fn account_quota_report(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    axum::extract::Query(q): axum::extract::Query<UsageQuery>,
) -> Response {
    if device(&host, &headers).is_none() {
        return unauthorized();
    }
    let mut res = Json(Outcome::from(host.quota_report(q.refresh.as_deref() == Some("1")).await)).into_response();
    res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res
}

/// 입력 사건 기록 하나의 상한과 남기는 개수
const DIAG_MAX: usize = 1 << 20;
const DIAG_KEEP: usize = 20;

/// 실기 입력 사건 기록(web/src/inputTrace.ts)을 host 상태 폴더 `diag/`에 남긴다. iPad 화면 키보드 같은 OS 입력기의
/// 사건 순서는 headless로 만들 수 없어 실기에서 모아 E2E 재생으로 옮긴다(herdr-web-ui terminal-ime-diagnostic과 같은 목적).
/// 친 글 원문이 들어 있어 조작 권한 기기만 쓰고, 파일은 사용자 전용(0600)이다. `application/json`만 받는다: 다른 사이트의
/// 폼이 보낼 수 있는 `text/plain` 단순 요청으로는 쓸 수 없다(보안 감사 t-0340 A6·B8, herdr-web-ui는 전용 헤더를 요구한다)
async fn diag_input(State(host): State<Arc<Host>>, headers: HeaderMap, body: axum::body::Bytes) -> Response {
    let Some((id, role)) = device(&host, &headers) else { return unauthorized() };
    if !role.can_control() {
        return (
            StatusCode::FORBIDDEN,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Forbidden, "보기 권한으로는 조작할 수 없다"))),
        )
            .into_response();
    }
    let json = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(';').next().is_some_and(|t| t.trim().eq_ignore_ascii_case("application/json")));
    if !json {
        return bad_request("content-type이 application/json이 아니다");
    }
    if body.len() > DIAG_MAX {
        return bad_request(format!("기록은 {}KiB까지", DIAG_MAX >> 10));
    }
    if serde_json::from_slice::<serde_json::Value>(&body).is_err() {
        return bad_request("JSON이 아니다");
    }
    let dir = host.state.join("diag");
    let name = format!("input-{}.json", aaa_common::util::now_ms());
    let saved = std::fs::create_dir_all(&dir).and_then(|_| aaa_common::util::write_private(&dir.join(&name), &body));
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
    let mut notices = host.notice_events.subscribe();
    let mut tasks = host.task_events.subscribe();
    let mut preferences = host.preferences.subscribe();
    let mut reports = host.reports_changed.subscribe();
    let mut asks = host.asks_changed.subscribe();
    let mut inbox = host.inbox_changed.subscribe();
    let mut accounts = host.accounts_changed.subscribe();
    let mut enroll = host.enroll_changed.subscribe();
    let mut rollouts = host.rollout_changed.subscribe();
    let mut revoked = host.device_revoked.subscribe();
    let mut chat_notes = host.chat_notes();
    // 이 연결이 대화를 청한 pane(차이 목록 C3). 연결이 끝나면 그만둔다
    let mut chats = ChatInterest { host: host.clone(), panes: HashSet::new() };
    let mut presence = Presence { conn: host.conn_open(&dev), host: host.clone(), dev: dev.clone(), active: false };
    let conn = presence.conn;
    let mut send_state = true;
    // 공유 설정 문서는 연결을 연다: 다시 붙은 화면(또는 읽기가 경주에 진 화면)도 마지막 문서를 갖는다(herdr-web-ui machine-api.ts)
    let mut send_preferences = true;
    // 보고서 안 본 수(활동 막대 배지, plan §8.256): 열 때와 목록이 바뀔 때
    let mut send_reports = true;
    // 「답할 것」(plan §8.345): 열 때와 확인 필요 post·답한 것이 바뀔 때
    let mut send_asks = true;
    let mut send_inbox = true;
    let mut send_accounts = true;
    let mut send_enroll = true;
    let mut send_rollout = true;
    let mut send_badge = true;
    let mut last_badge = None;
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
    // 「쓰는 화면」 lease: 웹은 쓰는 동안 10초마다 presence를 다시 알린다. 이 연결에서 앱 글(Text)이 lease 넘게 없으면 페이지가
    // 멈춘 것이라(iOS가 잠재운 앱) 쓰는 화면에서 뺀다. 연결은 두고, 다음 presence가 오면 다시 센다. WebSocket pong은
    // 브라우저 네트워크 층이 답해 멈춘 페이지에서도 오므로 lease를 늘리지 않는다(10-18 iPhone 실기, plan §8.133)
    let lease = presence_lease();
    let mut app_heard = tokio::time::Instant::now();
    // 이 host가 주는 웹 빌드: 연결마다 알린다. 다시 보이거나(10초 넘게 가려지면 다시 붙는다) host가 다시 뜨면 새 연결이라 웹이
    // 제 빌드와 견준다(참고는 보이는 동안 상태를 묻는다, plan §8.166)
    if let Some(build) = web_build() {
        let text = serde_json::to_string(&FrontEvent::Web { build }).unwrap();
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    loop {
        if send_badge || send_state || send_reports {
            send_badge = false;
            if let Ok(count) = host.app_badge_count()
                && last_badge != Some(count)
            {
                last_badge = Some(count);
                let text = serde_json::to_string(&FrontEvent::AppBadge { count }).unwrap();
                if socket.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
        if send_preferences {
            send_preferences = false;
            let doc = preferences.borrow_and_update().clone();
            let text = serde_json::to_string(&FrontEvent::Preferences(doc)).unwrap();
            if socket.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        if send_reports {
            send_reports = false;
            reports.borrow_and_update();
            let unseen = host.db.library_unseen_count().unwrap_or(0);
            let text = serde_json::to_string(&FrontEvent::Reports { unseen }).unwrap();
            if socket.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        if send_asks {
            send_asks = false;
            asks.borrow_and_update();
            let text = serde_json::to_string(&FrontEvent::Asks { requests: host.user_request_count() }).unwrap();
            if socket.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        if send_inbox {
            send_inbox = false;
            inbox.borrow_and_update();
            let text = serde_json::to_string(&FrontEvent::Inbox).unwrap();
            if socket.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        if send_accounts {
            send_accounts = false;
            accounts.borrow_and_update();
            let text = serde_json::to_string(&FrontEvent::Accounts).unwrap();
            if socket.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        if send_enroll {
            send_enroll = false;
            enroll.borrow_and_update();
            let text = serde_json::to_string(&FrontEvent::Enroll).unwrap();
            if socket.send(Message::Text(text.into())).await.is_err() {
                break;
            }
        }
        if send_rollout {
            send_rollout = false;
            rollouts.borrow_and_update();
            let result = match host.rollout_card() {
                Ok(card) => Outcome::Ok(card),
                Err(e) => Outcome::Err(e),
            };
            let text = serde_json::to_string(&FrontEvent::Rollout { result }).unwrap();
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
                send_rollout = true;
            }
            q = questions.recv() => {
                send_state = false;
                send_badge = true;
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
                send_badge = true;
                match t {
                    Ok(mut t) => {
                        attachment_thread(&host, &dev, &mut t);
                        let text = serde_json::to_string(&FrontEvent::Thread(Box::new(t))).unwrap();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            // 앱 안 알림(notice.rs). 기록하지 않으므로 놓친 것은 버린다(끊지 않는다)
            n = notices.recv() => {
                send_state = false;
                match n {
                    Ok(n) => {
                        let text = serde_json::to_string(&FrontEvent::Notice(n)).unwrap();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
            // TASKS.md가 바뀌었다(할 일 화면이 다시 읽는다). 놓쳤으면 끊는다: 웹은 다시 붙으며 새로 읽는다
            t = tasks.recv() => {
                send_state = false;
                match t {
                    Ok((project, sha256)) => {
                        let text = serde_json::to_string(&FrontEvent::Tasks { project, sha256 }).unwrap();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => break,
                }
            }
            r = preferences.changed() => {
                if r.is_err() { break }
                send_state = false;
                send_preferences = true;
            }
            r = rollouts.changed() => {
                if r.is_err() { break; }
                send_rollout = true;
            }
            r = reports.changed() => {
                if r.is_err() { break }
                send_state = false;
                send_reports = true;
            }
            r = asks.changed() => {
                if r.is_err() { break }
                send_state = false;
                send_asks = true;
            }
            r = inbox.changed() => {
                if r.is_err() { break }
                send_state = false;
                send_inbox = true;
            }
            r = accounts.changed() => {
                if r.is_err() { break }
                send_state = false;
                send_accounts = true;
            }
            r = enroll.changed() => {
                if r.is_err() { break }
                send_state = false;
                send_enroll = true;
            }
            n = chat_notes.recv() => {
                send_state = false;
                let evs = match n {
                    Ok(ChatNote::Changed { pane, signature }) if chats.panes.contains(&pane) => {
                        vec![FrontEvent::ChatChanged { pane, signature }]
                    }
                    Ok(ChatNote::Live { pane, live }) if chats.panes.contains(&pane) => {
                        vec![FrontEvent::ChatWatching { pane, live }]
                    }
                    Ok(_) | Err(RecvError::Closed) => vec![],
                    // 놓친 바뀜이 있을 수 있다: 청한 pane을 모두 다시 읽게 한다
                    Err(RecvError::Lagged(_)) => chats
                        .panes
                        .iter()
                        .flat_map(|p| {
                            [
                                FrontEvent::ChatWatching { pane: p.clone(), live: host.chat_live(p) },
                                FrontEvent::ChatChanged { pane: p.clone(), signature: "lagged".into() },
                            ]
                        })
                        .collect(),
                };
                let mut gone = false;
                for ev in evs {
                    if socket.send(Message::Text(serde_json::to_string(&ev).unwrap().into())).await.is_err() {
                        gone = true;
                        break;
                    }
                }
                if gone {
                    break;
                }
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
            _ = tokio::time::sleep_until(app_heard + lease), if presence.active => {
                send_state = false;
                tracing::info!("쓰는 화면 lease 끝: 앱 글이 {}초 동안 없다", lease.as_secs());
                host.conn_presence(conn, format!("lease 끝(앱 글 {}초 없음)", lease.as_secs()), None);
                presence.set(false);
            }
            m = socket.recv() => {
                send_state = false;
                heard = tokio::time::Instant::now();
                match m {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Pong(_))) => host.conn_pong(conn),
                    Some(Ok(Message::Text(t))) => {
                        app_heard = tokio::time::Instant::now();
                        host.conn_text(conn);
                        match serde_json::from_str::<EventsClientMsg>(&t) {
                            Ok(EventsClientMsg::Presence { active, why, visible, focus, build }) => {
                                host.conn_presence(conn, presence_why(active, why, visible, focus), build);
                                presence.set(active)
                            }
                            Ok(EventsClientMsg::Touch) => host.touch_device(&dev, "touch"),
                            Ok(EventsClientMsg::Alert { project, target, kind, outcome }) => {
                                host.alert_outcome(&dev, conn, project.as_deref(), &target, &kind, &outcome)
                            }
                            Ok(EventsClientMsg::Ping { id }) => {
                                let text = serde_json::to_string(&FrontEvent::Pong { id }).unwrap();
                                if socket.send(Message::Text(text.into())).await.is_err() {
                                    break;
                                }
                            }
                            // 보기 범위(§8.25)는 대화 읽기(`pane_chat`)와 같다
                            Ok(EventsClientMsg::ChatWatch { pane, enabled }) => {
                                if !enabled {
                                    if chats.panes.remove(&pane) {
                                        host.chat_watch(&pane, false);
                                    }
                                } else if host.pane_view_of(&pane, role).is_some() {
                                    if chats.panes.insert(pane.clone()) {
                                        host.chat_watch(&pane, true);
                                    }
                                    let ev = FrontEvent::ChatWatching { live: host.chat_live(&pane), pane };
                                    if socket.send(Message::Text(serde_json::to_string(&ev).unwrap().into())).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 상태 사건 연결이 대화를 청한 pane들. 연결이 어떻게 끝나든 그만둔다
struct ChatInterest {
    host: Arc<Host>,
    panes: HashSet<String>,
}

impl Drop for ChatInterest {
    fn drop(&mut self) {
        for pane in self.panes.drain() {
            self.host.chat_watch(&pane, false);
        }
    }
}

/// 옛 TUI 전용 endpoint. 기존 기기 인증은 유지하고 명시적인 삭제 안내만 보낸다.
async fn front_ws(State(host): State<Arc<Host>>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if device(&host, &headers).is_none() {
        return unauthorized();
    }
    ws.max_message_size(aaa_proto::MAX_FRAME).on_upgrade(|mut socket| async move {
        let message =
            aaa_proto::FrontMsg::Reject(ErrorBody::new(ErrorCode::Unavailable, aaa_common::front::TUI_REMOVED));
        if let Ok(text) = serde_json::to_string(&message) {
            let _ = socket.send(Message::Text(text.into())).await;
        }
        let _ = socket.close().await;
    })
}

/// 상태 사건 연결의 WebSocket ping 간격과, 아무것도 받지 못하면 끊는 시간(15초·45초: ping 두 번을 놓치고도 남는다).
/// `AAA_TEST_EVENTS_PING_MS`는 시험에서만 줄인다(끊는 시간은 그 세 배).
fn events_liveness() -> (std::time::Duration, std::time::Duration) {
    let ms = std::env::var("AAA_TEST_EVENTS_PING_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(15_000);
    (std::time::Duration::from_millis(ms), std::time::Duration::from_millis(ms * 3))
}

/// 「쓰는 화면」 lease(25초: 웹의 10초 presence를 두 번 놓치고도 남는다). `AAA_TEST_PRESENCE_LEASE_MS`는 시험에서만 줄인다
/// 포함한 웹의 빌드 이름: index.html이 여는 주 묶음(`/assets/index-XXXX.js`)의 파일 이름. 웹 `api.ts` `BUILD`와 같은 꼴이다.
/// 웹을 빌드하지 않았으면 없다. 시험은 `AAA_TEST_WEB_BUILD`로 다른 빌드를 알리게 한다(새 웹이 나온 host를 흉내 낸다)
fn web_build() -> Option<String> {
    if let Ok(v) = std::env::var("AAA_TEST_WEB_BUILD") {
        return Some(v);
    }
    let (_, html) = assets::WEB_ASSETS.iter().find(|(n, _)| *n == "index.html")?;
    main_bundle(&String::from_utf8_lossy(html))
}

/// index.html의 `<script … src="/assets/NAME.js">`에서 NAME
fn main_bundle(html: &str) -> Option<String> {
    html.split("<script").skip(1).find_map(|tag| {
        let tag = &tag[..tag.find('>')?];
        let src = tag.split("src=\"").nth(1)?.split('"').next()?;
        let name = src.strip_prefix("/assets/")?.strip_suffix(".js")?;
        (!name.is_empty() && !name.contains('/')).then(|| name.to_string())
    })
}

fn presence_lease() -> std::time::Duration {
    let ms = std::env::var("AAA_TEST_PRESENCE_LEASE_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(25_000);
    std::time::Duration::from_millis(ms)
}

/// presence 판정 근거 한 줄: 웹이 보낸 까닭과 화면 값(옛 웹은 없다)
fn presence_why(active: bool, why: Option<String>, visible: Option<bool>, focus: Option<bool>) -> String {
    let flag =
        |v: Option<bool>, yes: &str, no: &str| v.map_or("?".to_string(), |b| if b { yes } else { no }.to_string());
    let why: String =
        why.unwrap_or_else(|| "?".into()).chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(24).collect();
    format!(
        "{} {why} {}·{}",
        if active { "true" } else { "false" },
        flag(visible, "보임", "가려짐"),
        flag(focus, "focus", "focus 없음")
    )
}

/// 한 events 연결이 「쓰는 화면」인지. 바뀔 때만 host 수를 고치고, 연결이 어떻게 끝나든 drop에서 뺀다.
struct Presence {
    /// 판정 근거의 연결 번호(seen.rs `conn_open`)
    conn: u64,
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
            self.host.view_presence(self.conn, &self.dev, active);
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.set(false);
        self.host.conn_close(self.conn);
    }
}

/// pane WebSocket 주소의 선택. `flow=ack`면 출력 ACK를 보내는 화면이다(herdr-web-ui attach `flow_control: "ack"`).
/// `fg`·`bg`(`rrggbb`)는 그 화면의 터미널 기본 색이다(plan §8.187).
#[derive(Deserialize)]
struct PaneWsQuery {
    flow: Option<String>,
    fg: Option<String>,
    bg: Option<String>,
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
    let colors = q.fg.zip(q.bg).and_then(|(fg, bg)| aaa_proto::TermColors::from_hex(&fg, &bg));
    ws.max_message_size(1 << 20).on_upgrade(move |socket| pane_loop(host, id, dev, role, acks, colors, socket))
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

/// `Outbox`에 보내기만 하는 손잡이(앞 기록 답처럼 연결 loop 밖에서 끝나는 일)
struct OutSender {
    tx: tokio::sync::mpsc::UnboundedSender<Message>,
    buffered: Arc<AtomicU64>,
}

impl OutSender {
    fn msg(&self, m: &TermServerMsg) -> bool {
        let m = Message::Text(serde_json::to_string(m).unwrap().into());
        self.buffered.fetch_add(frame_len(&m), Ordering::Relaxed);
        self.tx.send(m).is_ok()
    }
}

/// 앞 기록 답(plan §8.67): 찾지 못했으면 붙이지 않는다
fn history_reply(id: u32, o: Option<aaa_vt::Older>) -> TermServerMsg {
    match o {
        Some(o) => TermServerMsg::History { id, data: B64(o.data), lines: o.lines as u32, more: o.more, found: true },
        None => TermServerMsg::History { id, data: B64(Vec::new()), lines: 0, more: false, found: false },
    }
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

    /// 다른 task가 이 줄에 보낼 손잡이(보낸 bytes는 같은 `buffered`에 센다)
    fn sender(&self) -> OutSender {
        OutSender { tx: self.tx.clone(), buffered: self.buffered.clone() }
    }

    fn buffered(&self) -> u64 {
        self.buffered.load(Ordering::Relaxed)
    }

    /// 출력을 따라오지 못했다: 4008로 닫는다. 다시 보내지 않는다(브라우저의 「다시 연결」이 새로 붙는다).
    /// 기록에는 어느 pane인지와 밀린 양(ACK 안 된 것·보내지 못한 것)을 남긴다: 무엇이 쏟아졌는지·누가 느렸는지를 가른다.
    fn stop(&self, why: &str, pane: &str, pending: u64) {
        tracing::info!(why, pane, pending, buffered = self.buffered(), "웹 터미널 출력 정지(4008)");
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
async fn pane_loop(
    host: Arc<Host>,
    id: String,
    dev: String,
    role: DeviceRole,
    acks: bool,
    colors: Option<aaa_proto::TermColors>,
    socket: WebSocket,
) {
    let short: String = dev.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let front = host.new_front(format!("웹({short})"));
    // 그 pane의 agent가 이 화면의 색을 받은 뒤에 화면을 연다: 화면이 본 뒤 프로그램이 묻는 색에는 이 색으로 답한다(plan §8.187).
    // agent가 늦으면 여는 것도 늦다(요청 시한까지). 끊긴 agent는 기다리지 않는다
    if let Some(wait) = colors.and_then(|c| host.front_colors(&front.id, &id, c)) {
        let _ = wait.await;
    }
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
    let mut input_traces = std::collections::HashMap::<String, u8>::new();
    let Some(view) = host.pane_view_of(&id, role) else { return };
    let (sink, mut stream) = socket.split();
    let out = Outbox::new(sink);
    let started = std::time::Instant::now();
    let now = || started.elapsed().as_millis() as u64;
    let mut streams = 0;
    if !out.msg(&TermServerMsg::Front { id: front.id.clone(), steer_queued: true }) {
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
    // submit을 기다리는 동안 받아 둔 frame(차례대로 먼저 읽는다)
    let mut later: std::collections::VecDeque<Option<Result<Message, axum::Error>>> = std::collections::VecDeque::new();
    'conn: loop {
        let verdict = if acks {
            window.check(out.buffered(), now())
        } else {
            check_unacked(out.buffered(), &mut queue_since, now())
        };
        if verdict == Verdict::Stop {
            out.stop("ACK가 나아가지 않는다", &id, window.pending());
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
                                out.stop("밀린 출력이 상한을 넘었다", &id, window.pending() + n);
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
            m = async { match later.pop_front() { Some(m) => m, None => stream.next().await } } => {
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
                        Ok(TermClientMsg::CtrlEnter { trace }) => {
                            let trace = trace.filter(|t| crate::input_log::valid_trace(t)).unwrap_or_else(aaa_common::util::new_id);
                            Some(FrontRequest::PaneCtrlEnter { pane: id.clone(), trace: Some(format!("{}:{trace}", front.id)) })
                        },
                        Ok(TermClientMsg::Resize { cols, rows }) if role.can_control() => {
                            Some(FrontRequest::PaneResize { pane: id.clone(), cols: cols.clamp(10, 500), rows: rows.clamp(3, 300) })
                        }
                        Ok(TermClientMsg::InputTrace { id: trace, event, method }) => {
                            if role.can_control() && crate::input_log::valid_trace(&trace)
                                && matches!(event, aaa_proto::InputTraceEvent::CtrlEnterBlocked | aaa_proto::InputTraceEvent::Preparing) && input_traces.len() < 128 {
                                input_traces.entry(trace.clone()).or_insert(0);
                            }
                            if role.can_control() && let Some(count) = input_traces.get_mut(&trace) && *count < 16 {
                                *count += 1;
                                host.input_event(&id, &format!("{}:{trace}", front.id), "ui", &format!("event={event:?} method={method:?}"));
                            }
                            None
                        }
                        Ok(TermClientMsg::Submit { id: sid, data, steer, trace, meta }) => {
                            let bytes = data.len();
                            let trace = trace.filter(|t| crate::input_log::valid_trace(t)).unwrap_or_else(|| sid.to_string());
                            if input_traces.len() >= 128 { input_traces.clear(); }
                            input_traces.insert(trace.clone(), 0);
                            let trace = format!("{}:{trace}", front.id);
                            host.input_event(&id, &trace, "received", &format!("{} pasted_source={} attachments={}", crate::input_log::submit_meta(&data, steer), meta.pasted, meta.attachments));
                            tracing::info!(pane = %id, front = %front.id, submit = sid, bytes, steer, "웹 입력: submit 수신");
                            host.touch_device(dev, "입력");
                            // 글과 Enter를 쓰는 동안에도 이 submit의 멈춤은 바로 답한다(plan §8.247). 다른 frame은 뒤에 차례대로 둔다:
                            // 이 연결의 다음 입력은 Enter 뒤에 간다
                            let stage = AtomicU8::new(SUBMIT_BEFORE);
                            let work = submit_work(host, role, &actor, front, dev, &id, sid, data, steer, meta.ctrl_enter || steer, &trace, &stage);
                            tokio::pin!(work);
                            let mut reading = true;
                            let end = loop {
                                tokio::select! {
                                    biased;
                                    end = &mut work => break end,
                                    m = stream.next(), if reading => {
                                        if let Some(Ok(Message::Text(t))) = &m
                                            && let Ok(TermClientMsg::SubmitCancel { id: cid }) = serde_json::from_str::<TermClientMsg>(t)
                                        {
                                            let answer = if cid == sid { cancel_submit(&stage) } else { SubmitStage::TooLate };
                                            host.input_event(&id, &trace, "cancel", &format!("stage={answer:?}"));
                                            tracing::info!(pane = %id, front = %front.id, submit = cid, ?answer, "웹 입력: 멈춤");
                                            if !out.msg(&TermServerMsg::SubmitCancelled { id: cid, stage: answer }) {
                                                reading = false;
                                            }
                                            continue;
                                        }
                                        reading = !matches!(m, Some(Ok(Message::Close(_))) | None | Some(Err(_)));
                                        later.push_back(m);
                                    }
                                }
                            };
                            let sent = match end {
                                SubmitEnd::Revoked => {
                                    tracing::warn!(pane = %id, front = %front.id, submit = sid, "웹 입력: 기기 폐기로 Enter 취소");
                                    out.revoked();
                                    break 'conn;
                                }
                                SubmitEnd::Stopped(at) => Err(ErrorBody::new(
                                    ErrorCode::BadRequest,
                                    if at == SubmitStage::NotSent { "멈춤: 보내지 않았다" } else { "멈춤: Enter를 보내지 않았다" },
                                )),
                                SubmitEnd::Done(r) => r,
                            };
                            host.db.audit(&actor, "web_submit", &format!("pane={id} front={} submit={sid} bytes={bytes} steer={steer} ok={}", front.id, sent.is_ok()));
                            host.input_event(&id, &trace, if sent.is_ok() { "delivered" } else { "failed" }, &format!("code={:?}", sent.as_ref().err().map(|e| &e.code)));
                            let reply = TermServerMsg::Submitted { id: sid, ok: sent.is_ok(), message: sent.err().map(|e| e.message) };
                            if !out.msg(&reply) {
                                tracing::warn!(pane = %id, front = %front.id, submit = sid, "웹 입력: 확인 응답 연결 닫힘");
                                break 'conn;
                            }
                            None
                        }
                        // 대기 메시지 「지금 보내기」(plan §8.271): 키만 보내고 답한다. submit 중이면 이 frame은 그 뒤(Enter 뒤)에 온다
                        Ok(TermClientMsg::SteerQueued { id: sid, trace }) => {
                            let trace = trace.filter(|t| crate::input_log::valid_trace(t)).unwrap_or_else(aaa_common::util::new_id);
                            let trace = format!("{}:{trace}", front.id);
                            host.touch_device(dev, "입력");
                            let reply = steer_queued(host, role, &actor, front, &id, sid, &trace).await;
                            if !out.msg(&reply) {
                                break 'conn;
                            }
                            None
                        }
                        // 진행 중인 submit이 없을 때의 멈춤: 이미 끝났다
                        Ok(TermClientMsg::SubmitCancel { id: cid }) => {
                            if !out.msg(&TermServerMsg::SubmitCancelled { id: cid, stage: SubmitStage::TooLate }) {
                                break 'conn;
                            }
                            None
                        }
                        Ok(TermClientMsg::History { id: hid, anchor, depth }) => {
                            // 앞 기록 한 덩어리(plan §8.67). 출력 흐름(ACK 창) 밖의 답이라 창에 넣지 않는다:
                            // 한 덩어리는 replay 기록 상한(2000줄·384KiB) 안이다.
                            let anchor: Vec<String> = anchor.into_iter().take(8).collect();
                            match host.older_history(&id, &anchor, depth as usize) {
                                crate::OlderHistory::Ready(o) => {
                                    if !out.msg(&history_reply(hid, o)) {
                                        break;
                                    }
                                }
                                // mirror에 없는 앞 기록: session host에 묻는 동안 이 연결의 출력·입력을 막지 않는다(plan §8.118)
                                crate::OlderHistory::Ask(conn) => {
                                    let out = out.sender();
                                    let session = id.clone();
                                    tokio::spawn(async move {
                                        let o = Host::older_from_agent(&conn, session, anchor, depth as usize).await;
                                        out.msg(&history_reply(hid, o));
                                    });
                                }
                            }
                            None
                        }
                        // 테마를 바꿨다: 연 때처럼 agent가 받은 뒤 이 화면의 다음 메시지를 본다
                        Ok(TermClientMsg::Colors { fg, bg }) => {
                            let wait = aaa_proto::TermColors::from_hex(&fg, &bg).and_then(|c| host.front_colors(&front.id, &id, c));
                            if let Some(wait) = wait {
                                let _ = wait.await;
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
                    host.touch_device(dev, "입력");
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

/// 대기 메시지 「지금 보내기」 하나(plan §8.271). host 판정(`steer_queued_data`)이 고른 키를 바로 그 뒤에 쓴다: 판정과 쓰기 사이에
/// 다른 기다림이 없어 Codex Esc는 패널을 확인한 같은 mirror 상태에서 나간다(남는 틈은 agent까지 가는 길뿐이다).
/// 보기 권한은 판정 전에 거절한다. 보냄·거절·실패를 모두 입력 사건으로 남긴다(`steer_queued` 판정, `steer_sent`·`steer_failed`)
async fn steer_queued(
    host: &Arc<Host>,
    role: DeviceRole,
    actor: &str,
    front: &FrontRef,
    pane: &str,
    sid: u32,
    trace: &str,
) -> TermServerMsg {
    use aaa_proto::SteerRefusal;
    let refused = |reason: SteerRefusal, message: Option<String>| TermServerMsg::SteerQueued {
        id: sid,
        sent: false,
        reason: Some(reason),
        message,
    };
    if !role.can_control() {
        host.input_event(pane, trace, "steer_queued", "reason=Failed role=readonly");
        return refused(SteerRefusal::Failed, Some("보기 권한 기기는 입력할 수 없다".into()));
    }
    let data = match host.steer_queued_data(pane, trace) {
        Ok(data) => data,
        Err(reason) => return refused(reason, None),
    };
    let sent = host
        .call_from(role, actor, Some(front), FrontRequest::PaneInput { pane: pane.to_string(), data: B64(data) })
        .await;
    host.db.audit(actor, "web_steer_queued", &format!("pane={pane} front={} ok={}", front.id, sent.is_ok()));
    match sent {
        Ok(_) => {
            host.input_event(pane, trace, "steer_sent", "agent_ack=true");
            TermServerMsg::SteerQueued { id: sid, sent: true, reason: None, message: None }
        }
        Err(e) => {
            host.input_event(pane, trace, "steer_failed", &format!("code={:?}", e.code));
            refused(SteerRefusal::Failed, Some(e.message))
        }
    }
}

/// submit 하나의 단계(plan §8.247). 일꾼과 멈춤이 compare-exchange로 다투어 둘 중 하나만 이긴다:
/// 멈춤이 이기면 일꾼은 그 다음 쓰기를 하지 않는다
const SUBMIT_BEFORE: u8 = 0;
const SUBMIT_TYPING: u8 = 1;
const SUBMIT_TYPED: u8 = 2;
const SUBMIT_ENTERING: u8 = 3;
const SUBMIT_STOPPED_BEFORE: u8 = 4;
const SUBMIT_STOPPED_NO_ENTER: u8 = 5;

enum SubmitEnd {
    Done(Result<(), ErrorBody>),
    Stopped(SubmitStage),
    /// 기다리는 동안 기기가 폐기됐다: Enter를 보내지 않고 연결을 닫는다(t-0340 감사 A9, t3code `awaitInvalidation`)
    Revoked,
}

/// 멈춤 요청: 아직 아무것도 쓰지 않았으면 쓰지 않고, 글을 쓰는 중이거나 썼으면 Enter를 보내지 않는다. Enter를 보내기
/// 시작했으면 늦었다. 같은 submit을 두 번 멈추면 처음 답을 다시 준다
fn cancel_submit(stage: &AtomicU8) -> SubmitStage {
    loop {
        let now = stage.load(Ordering::SeqCst);
        let (next, answer) = match now {
            SUBMIT_BEFORE => (SUBMIT_STOPPED_BEFORE, SubmitStage::NotSent),
            SUBMIT_TYPING | SUBMIT_TYPED => (SUBMIT_STOPPED_NO_ENTER, SubmitStage::NoEnter),
            SUBMIT_STOPPED_BEFORE => return SubmitStage::NotSent,
            SUBMIT_STOPPED_NO_ENTER => return SubmitStage::NoEnter,
            _ => return SubmitStage::TooLate,
        };
        if stage.compare_exchange(now, next, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            return answer;
        }
    }
}

/// 일꾼이 다음 단계로 간다. 멈춤이 먼저 이겼으면 false(그 쓰기를 하지 않는다)
fn advance_submit(stage: &AtomicU8, from: u8, to: u8) -> bool {
    stage.compare_exchange(from, to, Ordering::SeqCst, Ordering::SeqCst).is_ok()
}

/// 글, 잠깐 뒤 Enter(한 덩어리로 보내면 Codex가 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다). 답은 둘 다 pane에 들어간 뒤다.
/// 쓰기마다 그 앞에서 멈춤을 본다: 막힌 agent에 간 글 요청은 거둘 수 없지만 Enter는 보내지 않는다
#[allow(clippy::too_many_arguments)]
async fn submit_work(
    host: &Arc<Host>,
    role: DeviceRole,
    actor: &str,
    front: &FrontRef,
    dev: &str,
    pane: &str,
    sid: u32,
    data: String,
    steer: bool,
    ctrl_enter: bool,
    trace: &str,
    stage: &AtomicU8,
) -> SubmitEnd {
    let typed = |data: Vec<u8>| FrontRequest::PaneInput { pane: pane.to_string(), data: B64(data) };
    if !advance_submit(stage, SUBMIT_BEFORE, SUBMIT_TYPING) {
        return SubmitEnd::Stopped(SubmitStage::NotSent);
    }
    let sent = host.call_from(role, actor, Some(front), typed(data.into_bytes())).await;
    tracing::info!(pane, front = %front.id, submit = sid, ok = sent.is_ok(), code = ?sent.as_ref().err().map(|e| &e.code), phase = "text", "웹 입력: agent 응답");
    if let Err(e) = sent {
        return SubmitEnd::Done(Err(e));
    }
    host.input_event(pane, trace, "written_text", "agent_ack=true");
    if !advance_submit(stage, SUBMIT_TYPING, SUBMIT_TYPED) {
        return SubmitEnd::Stopped(SubmitStage::NoEnter);
    }
    crate::text::submission_gap().await;
    if host.is_revoked(dev) {
        return SubmitEnd::Revoked;
    }
    if !advance_submit(stage, SUBMIT_TYPED, SUBMIT_ENTERING) {
        return SubmitEnd::Stopped(SubmitStage::NoEnter);
    }
    // Ctrl+Enter로 보낸 글: 작업 중인 Claude는 Enter 대신 sendNow로 지금 턴에 넣는다(그 밖은 Enter, plan §8.239 후속)
    let enter = host.submit_key_data(pane, steer, ctrl_enter.then_some(trace));
    let sent = host.call_from(role, actor, Some(front), typed(enter)).await;
    tracing::info!(pane, front = %front.id, submit = sid, ok = sent.is_ok(), code = ?sent.as_ref().err().map(|e| &e.code), phase = if steer { "steer" } else { "enter" }, "웹 입력: agent 응답");
    if sent.is_ok() {
        host.input_event(pane, trace, "written_submit", if steer { "steer=true" } else { "key=enter" });
    }
    SubmitEnd::Done(sent.map(|_| ()))
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
    if path.starts_with("api/") || path.starts_with("v/") {
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
    #[test]
    fn report_connections_have_one_exact_origin_and_capability_prefix() {
        for origin in ["http://127.0.0.1:7000", "https://example.ts.net"] {
            let csp = super::view_csp(origin, "abc.def");
            let csp = csp.to_str().unwrap();
            let connect = csp.split(';').find(|d| d.trim().starts_with("connect-src ")).unwrap().trim();
            assert_eq!(connect, format!("connect-src {origin}/v/abc.def/"));
            assert!(!csp.contains("allow-same-origin"));
            for directive in ["media-src", "img-src", "script-src", "style-src", "font-src"] {
                let value = csp.split(';').find(|d| d.trim().starts_with(directive)).unwrap().trim();
                assert!(value.starts_with(&format!("{directive} {origin}/v/abc.def/")), "{value}");
            }
            assert!(csp.contains("frame-ancestors 'self'"));
        }
        let csp = super::view_csp("https://example.ts.net", "x/; connect-src *");
        assert!(!csp.to_str().unwrap().contains("connect-src *"));
    }

    use super::content_type;

    /// host가 알리는 웹 빌드는 웹 `BUILD`(주 묶음 파일 이름에서 확장자를 뺀 것)와 같은 꼴이다
    #[test]
    fn main_bundle_is_the_index_script_name() {
        let html = r#"<!doctype html><head><link rel="manifest" href="/manifest.webmanifest">
<script type="module" crossorigin src="/assets/index-Di-S2J_3.js"></script>
<link rel="stylesheet" crossorigin href="/assets/index-C9x.css"></head>"#;
        assert_eq!(super::main_bundle(html).as_deref(), Some("index-Di-S2J_3"));
        assert_eq!(super::main_bundle("<script>let a = 1</script>"), None);
        assert_eq!(super::main_bundle(r#"<script src="https://example.com/x.js"></script>"#), None);
    }

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
    fn report_images_bridge_goes_only_on_html_navigation() {
        use super::{REPORT_IMAGES, report_images_bridge};
        assert!(report_images_bridge("text/html; charset=utf-8", false, false, true));
        // 원본 그대로: fetch(소스 보기)·Range·내려받기·HTML 아닌 파일
        assert!(!report_images_bridge("text/html; charset=utf-8", false, false, false));
        assert!(!report_images_bridge("text/html; charset=utf-8", true, false, true));
        assert!(!report_images_bridge("text/html; charset=utf-8", false, true, true));
        for kind in ["image/svg+xml", "text/plain; charset=utf-8", "application/pdf", "text/javascript; charset=utf-8"]
        {
            assert!(!report_images_bridge(kind, false, false, true), "{kind}");
        }
        // 붙인 script 하나가 문서 끝에서 닫힌다
        assert!(REPORT_IMAGES.starts_with("<script>") && REPORT_IMAGES.ends_with("</script>"));
        assert_eq!(REPORT_IMAGES.matches("</script").count(), 1);
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

    /// 조작 기기 passkey(관리 GUI W7): 공개 주소의 조작 기기는 passkey 없이 승인하지 못하고, serve를 거친 요청은 Host를
    /// loopback으로 적어도 host Mac 화면으로 보지 않는다(serve는 받은 Host를 넘긴다: 훔친 cookie로 UV를 건너뛰는 길)
    #[tokio::test]
    async fn only_a_direct_loopback_screen_approves_without_a_passkey() {
        let l = lab("https://mac-a.example.ts.net:8443");
        let t = token(&l.0);
        let call = |host: &'static str, more: Vec<(&'static str, String)>| {
            let h = l.0.clone();
            async move {
                let origin = if host == LOOP { "http://127.0.0.1:7000" } else { "https://mac-a.example.ts.net:8443" };
                let mut req = Request::builder()
                    .method("POST")
                    .uri("/api/v1/call")
                    .header(header::HOST, host)
                    .header(header::ORIGIN, origin)
                    .header(header::CONTENT_TYPE, "application/json");
                for (k, v) in more {
                    req = req.header(k, v);
                }
                let body = Body::from(r#"{"op":"web_pair","role":"controller"}"#);
                let res = router(h).oneshot(req.body(body).unwrap()).await.unwrap();
                let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
                serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()
            }
        };
        let direct = call(LOOP, vec![("cookie", format!("{}={t}", cookie_name(false)))]).await;
        assert_eq!(direct["ok"]["kind"], "pair_code", "{direct}");
        let public = call(PUBLIC, vec![("cookie", format!("{}={t}", cookie_name(true)))]).await;
        assert_eq!(public["err"]["code"], "passkey_required", "{public}");
        for (k, v, secure) in [("x-forwarded-proto", "https", true), ("x-forwarded-for", "203.0.113.9", false)] {
            let cookie = format!("{}={t}", cookie_name(secure));
            let bent = call(LOOP, vec![("cookie", cookie), (k, v.to_string())]).await;
            assert_eq!(bent["err"]["code"], "passkey_required", "{k}: {bent}");
        }
    }

    async fn code(h: &Arc<Host>, method: &str, path: &str, host: &str, more: &[(&str, &str)]) -> StatusCode {
        ask(h, method, path, host, more).await.status()
    }

    #[tokio::test]
    async fn attachment_api_errors_are_utf8_and_caps_expire_with_the_device() {
        let l = lab("https://mac-a.example.ts.net:8443");
        let t = token(&l.0);
        let (id, _) = l.0.device_role(&t).unwrap();
        for path in [
            "/api/v1/projects/demo/questions/t-0001/hash/library/a.mp4",
            "/api/v1/projects/demo/requests/item/hash/library/a.mp4",
            "/api/v1/projects/demo/asks/item/library/a.mp4",
        ] {
            let response = ask(&l.0, "GET", path, LOOP, &[]).await;
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json; charset=utf-8");
            let bytes = axum::body::to_bytes(response.into_body(), 4096).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["err"]["message"], "pairing이 필요하다");
        }
        let original = Some("/api/v1/projects/demo/asks/item/".to_string());
        let mut base = original.clone();
        attachment_base(&l.0, &id, &mut base);
        assert!(base.as_deref().unwrap().starts_with("/v/"));
        let mut same = original;
        attachment_base(&l.0, &id, &mut same);
        assert_eq!(base, same, "목록을 갱신해도 같은 기기·사본은 같은 경로다");
        let cap = base.unwrap().trim_start_matches("/v/").trim_end_matches('/').to_string();
        assert_eq!(l.0.library_resolve("demo", &cap).unwrap_err().code, ErrorCode::BadRequest);
        // 유효 cap은 대상 파일 확인까지 가고, 만료되면 대상보다 먼저 권한 오류다.
        assert_ne!(l.0.view_source(&cap, "library/a.mp4").await.err().unwrap().code, ErrorCode::Unauthorized);
        l.0.view_caps.lock().unwrap().get_mut(&cap).unwrap().until = Instant::now();
        assert_eq!(l.0.view_source(&cap, "library/a.mp4").await.err().unwrap().code, ErrorCode::Unauthorized);
        let mut base = Some("/api/v1/projects/demo/asks/item/".to_string());
        attachment_base(&l.0, &id, &mut base);
        let cap = base.unwrap().trim_start_matches("/v/").trim_end_matches('/').to_string();
        l.0.revoke_device(&id).unwrap();
        assert_eq!(l.0.view_source(&cap, "library/a.mp4").await.err().unwrap().code, ErrorCode::Unauthorized);
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
        // 다른 기기 TUI의 Front API(`/api/v1/front`, plan §6.2)도 같은 규칙이다
        for path in ["/api/v1/events", "/api/v1/front"] {
            for bad in ["https://mac-a.example.ts.net:9999", "https://mac-a.example.ts.net"] {
                assert_eq!(code(&l.0, "GET", path, PUBLIC, &ws(bad)).await, StatusCode::FORBIDDEN, "{path} {bad}");
            }
            assert_ne!(
                code(&l.0, "GET", path, PUBLIC, &ws("https://mac-a.example.ts.net:8443")).await,
                StatusCode::FORBIDDEN,
                "{path}"
            );
        }
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

    /// 같은 hostname의 두 인스턴스가 서로의 로그인 cookie를 덮지 않는다. 기본 인스턴스는 지금 이름 그대로다
    #[test]
    fn instance_cookie_names() {
        assert_eq!(cookie_name_in(None, true), "__Host-aaa");
        assert_eq!(cookie_name_in(None, false), "aaa");
        assert_eq!(cookie_name_in(Some("work"), true), "__Host-aaa-work");
        assert_eq!(cookie_name_in(Some("work"), false), "aaa-work");
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
        for path in ["/api/v1/me", "/api/v1/usage", "/api/v1/push", "/api/v1/app-badge"] {
            let res = ask(&l.0, "GET", path, LOOP, &[]).await;
            assert_eq!(
                res.headers().get(header::CACHE_CONTROL).map(|v| v.to_str().unwrap()),
                Some("no-store"),
                "{path}"
            );
        }
    }
}

#[cfg(test)]
mod submit_tests {
    use super::*;

    /// 멈춤과 일꾼은 단계마다 하나만 이긴다: 쓰기 전 멈춤은 아무것도 쓰지 않고, 글을 쓰는 중·쓴 뒤 멈춤은 Enter를 막고,
    /// Enter를 보내기 시작한 뒤 멈춤은 늦다(plan §8.247)
    #[test]
    fn cancel_races_each_write() {
        let stage = AtomicU8::new(SUBMIT_BEFORE);
        assert_eq!(cancel_submit(&stage), SubmitStage::NotSent);
        assert!(!advance_submit(&stage, SUBMIT_BEFORE, SUBMIT_TYPING), "멈춘 submit이 글을 썼다");
        assert_eq!(cancel_submit(&stage), SubmitStage::NotSent, "두 번째 멈춤은 처음 답");

        for typed in [SUBMIT_TYPING, SUBMIT_TYPED] {
            let stage = AtomicU8::new(SUBMIT_BEFORE);
            assert!(advance_submit(&stage, SUBMIT_BEFORE, SUBMIT_TYPING));
            if typed == SUBMIT_TYPED {
                assert!(advance_submit(&stage, SUBMIT_TYPING, SUBMIT_TYPED));
            }
            assert_eq!(cancel_submit(&stage), SubmitStage::NoEnter);
            assert!(!advance_submit(&stage, SUBMIT_TYPING, SUBMIT_TYPED));
            assert!(!advance_submit(&stage, SUBMIT_TYPED, SUBMIT_ENTERING), "멈춘 submit이 Enter를 보냈다");
        }

        let stage = AtomicU8::new(SUBMIT_BEFORE);
        for (from, to) in
            [(SUBMIT_BEFORE, SUBMIT_TYPING), (SUBMIT_TYPING, SUBMIT_TYPED), (SUBMIT_TYPED, SUBMIT_ENTERING)]
        {
            assert!(advance_submit(&stage, from, to));
        }
        assert_eq!(cancel_submit(&stage), SubmitStage::TooLate);
        assert_eq!(stage.load(Ordering::SeqCst), SUBMIT_ENTERING, "늦은 멈춤이 단계를 바꿨다");
    }
}
