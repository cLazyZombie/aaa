//! 파일·보고서·첨부·그림을 보내는 HTTP 경로.

use super::*;

/// 파일 보기 응답(herdr-web-ui `fileResponse`): 이 origin에서 연 HTML·SVG가 앱 권한으로 script를 돌리지 못하게 sandbox.
/// PDF는 브라우저 자체 viewer가 sandbox 문서를 거부하므로 뺀다(그 script는 viewer 안에 머문다)
pub(super) const FILE_CSP: &str =
    "sandbox; default-src 'none'; img-src 'self' data:; media-src 'self'; style-src 'unsafe-inline'";

pub(super) const PDF_CSP: &str = "frame-ancestors 'self'";

/// 페이지로 그린 HTML(참고 `PAGE_POLICY`): 제 script는 돌지만 origin 없는 sandbox라 이 앱의 cookie·저장소가 없고,
/// `connect-src`가 없어 이 서버에도 웹에도 묻지 못한다. 제 폴더 파일은 agent가 미리 넣었다(data:). 이 앱만 frame에 넣는다.
pub(super) const PAGE_CSP: &str = "sandbox allow-scripts; default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' data:; \
                        style-src 'unsafe-inline' data:; img-src data: blob:; media-src data: blob:; font-src data:; \
                        base-uri 'none'; form-action 'none'; frame-ancestors 'self'";

#[derive(serde::Deserialize)]
pub(super) struct FileQuery {
    pub(super) download: Option<String>,
    /// HTML 파일을 페이지로(`PAGE_CSP`). 다른 파일은 본다
    pub(super) render: Option<String>,
}

/// `/f/<cap>/<이름>`: 파일 보기 읽기 경로(plan §8.125). 그 파일 하나만 30분 읽고, 짝지은 조작 기기여야 한다. Range를 받는다.
pub(super) async fn pane_file_view(
    State(host): State<Arc<Host>>,
    Path((cap, _name)): Path<(String, String)>,
    Query(q): Query<FileQuery>,
    headers: HeaderMap,
) -> Response {
    let text = |code: StatusCode, msg: String| {
        (code, [(header::CONTENT_TYPE, "text/plain; charset=utf-8")], msg).into_response()
    };
    // 다른 site의 페이지가 그림·script·fetch로 이 파일을 읽으려는 것(참고 `crossSiteFileRead`). 새 탭·내려받기(navigate)는 된다
    let fetch = |name: &str| {
        headers.get(name).and_then(|v| {
            v.to_str()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
    };
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
pub(super) const VIEW_CSP: &str = "sandbox allow-scripts allow-popups allow-popups-to-escape-sandbox allow-downloads; \
                        default-src 'none'; script-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob:; \
                        style-src 'self' 'unsafe-inline' data:; img-src 'self' data: blob:; \
                        media-src 'self' data: blob:; font-src 'self' data:; \
                        connect-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'self'";

/// capability가 가리키는 보고서 폴더만 읽는다. 끝의 /는 형제 cap·API와의 접두사 충돌을 막는다.
/// /v/는 redirect하지 않는다(CSP는 redirect 뒤 경로를 비교하지 않으므로 그 계약이 필요하다).
pub(super) fn view_csp(origin: &str, cap: &str) -> HeaderValue {
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
pub(super) const REPORT_IMAGES: &str = concat!("<script>", include_str!("../report_images.js"), "</script>");

/// 알림을 붙이는 응답: HTML을 문서·iframe으로 여는 전체 응답만. Range(파일 보기의 앞부분 읽기)·내려받기·fetch·HEAD는 원본이다
pub(super) fn report_images_bridge(kind: &str, range: bool, download: bool, navigation: bool) -> bool {
    kind.starts_with("text/html") && !range && !download && navigation
}

pub(super) async fn view_root(
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
pub(super) async fn view_file(
    State(host): State<Arc<Host>>,
    Path((cap, path)): Path<(String, String)>,
    Query(q): Query<FileQuery>,
    headers: HeaderMap,
    method: Method,
) -> Response {
    serve_view(host, cap, path, q, headers, method).await
}

/// 브라우저 문서 탐색만 열람이다. cookie 없는 외부 탭도 포함하며 fetch·그림·미리보기·HEAD는 제외한다.
pub(super) fn report_navigation(method: &Method, headers: &HeaderMap) -> bool {
    let value = |key: &str| {
        headers
            .get(key)
            .and_then(|v| {
                v.to_str()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .unwrap_or("")
    };
    method == Method::GET
        && value("sec-fetch-mode") == "navigate"
        && matches!(value("sec-fetch-dest"), "document" | "iframe" | "frame")
        && !["purpose", "sec-purpose", "x-purpose"].iter().any(|key| {
            let purpose = value(key).to_ascii_lowercase();
            ["prefetch", "preview", "prerender"].iter().any(|p| purpose.contains(p))
        })
}

pub(super) async fn serve_view(
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
    let range = match crate::files::parse_range(
        headers.get(header::RANGE).and_then(|v| {
            v.to_str()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        }),
        size,
    ) {
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
    // Range·내려받기는 원본 bytes/길이를 유지한다. HTML bridge도 같은 stream 안에서 압축한다.
    let negotiate = !headers.contains_key(header::RANGE)
        && q.download.is_none()
        && len + tail.len() as u64 >= 1024
        && report_delivery::compressible(kind);
    let gzip = negotiate && assets::accepts_gzip(&headers);
    let mut res = Response::builder()
        .status(if range.is_some() { StatusCode::PARTIAL_CONTENT } else { StatusCode::OK })
        .header(header::CONTENT_TYPE, kind)
        .header(header::ACCEPT_RANGES, if gzip { "none" } else { "bytes" })
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::CONTENT_SECURITY_POLICY, csp)
        .header(header::X_FRAME_OPTIONS, "SAMEORIGIN");
    if negotiate {
        res = res.header(header::VARY, "Accept-Encoding");
    }
    let body = if gzip {
        res = res.header(header::CONTENT_ENCODING, "gzip");
        report_delivery::gzip(body)
    } else {
        res = res.header(header::CONTENT_LENGTH, len + tail.len() as u64);
        body
    };
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

/// task 첨부(plan §8.327, 그림·md·html 문서): 프로젝트 uploads/에 쓰고 메모에 적을 `uploads/<이름>`을 돌려준다. 대화 첨부와 같이 인증·권한을
/// 확인한 뒤 원본 bytes를 16MiB 한도로 읽는다. 메모 줄은 웹이 `task_edit`로 쓴다
#[derive(serde::Deserialize)]
pub(super) struct TaskFileQuery {
    /// 문서 종류(md·html). 없으면 그림이다
    pub(super) kind: Option<String>,
}

pub(super) async fn task_file(
    State(host): State<Arc<Host>>,
    Path(project): Path<String>,
    Query(q): Query<TaskFileQuery>,
    req: Request,
) -> Response {
    let Some((id, role)) = device(&host, req.headers()) else { return unauthorized() };
    if !role.can_control() {
        return error_response(ErrorBody::new(ErrorCode::Forbidden, "보기 권한으로는 첨부할 수 없다"));
    }
    let Ok(bytes) = axum::body::to_bytes(req.into_body(), crate::task_attach::MAX_BYTES as usize)
        .await
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
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
pub(super) struct LibraryUploadQuery {
    pub(super) path: String,
}

pub(super) async fn library_upload(
    State(host): State<Arc<Host>>,
    Path((project, upload)): Path<(String, String)>,
    Query(q): Query<LibraryUploadQuery>,
    req: Request,
) -> Response {
    let started = std::time::Instant::now();
    let Some((_, role)) = device(&host, req.headers()) else { return unauthorized() };
    if !role.can_control() {
        return error_response(ErrorBody::new(ErrorCode::Forbidden, "보기 권한으로는 보고서를 올릴 수 없다"));
    }
    let Ok(bytes) = axum::body::to_bytes(req.into_body(), crate::library_upload::FILE_MAX as usize)
        .await
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::TooLarge, "파일 하나는 16MiB까지 올린다"))),
        )
            .into_response();
    };
    let body_us = started.elapsed().as_micros() as u64;
    // 동시 요청이 파일 I/O·업로드 잠금으로 async 실행기를 막지 않게 한다.
    let result = tokio::task::spawn_blocking(move || host.library_upload_put(role, &project, &upload, &q.path, &bytes))
        .await
        .map_err(crate::orch::internal)
        .and_then(|result| result);
    tracing::debug!(target: "aaa_upload", body_us,
        request_us = started.elapsed().as_micros() as u64, ok = result.is_ok(),
        "보고서 파일 요청");
    match result {
        Ok((files, total)) => Json(Outcome::Ok(serde_json::json!({ "files": files, "total": total }))).into_response(),
        Err(e) => error_response(e),
    }
}

pub(super) async fn design_file_post() -> Response {
    error_response(ErrorBody::new(ErrorCode::BadRequest, "그림은 기획 편집 작업의 branch에서 추가한다"))
}

#[derive(Deserialize)]
pub(super) struct DesignFileQuery {
    /// 문서를 읽은 commit. 같은 commit의 그림은 바뀌지 않아 오래 둔다
    pub(super) c: Option<String>,
    /// 기획 branch에서 읽은 문서면 그 slug(FRONT_API 1.171): commit이 그 branch의 판이어도 된다
    pub(super) b: Option<String>,
}

/// 기획 문서 안 그림·영상(보기 권한도, t-1894). repo 머신 agent가 그 commit의 git 객체에서 읽는다. sandbox CSP로 보내 SVG·HTML이
/// 앱 cookie·API에 닿지 못하게 한다
pub(super) async fn design_file_get(
    State(host): State<Arc<Host>>,
    Path((project, repo, path)): Path<(String, String, String)>,
    Query(q): Query<DesignFileQuery>,
    headers: HeaderMap,
) -> Response {
    if device(&host, &headers).is_none() {
        return unauthorized();
    }
    let commit = q.c.filter(|c| aaa_common::git_watch::valid_sha(c));
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let branch = q.b.filter(|b| aaa_proto::design_slug_ok(b));
    let read = match host.design_file_read(&project, &repo, &path, (commit.as_deref(), branch.as_deref()), range).await
    {
        Ok(read) => read,
        Err(e) => return error_response(e),
    };
    let res = Response::builder()
        .header(header::CONTENT_TYPE, crate::design::mime(&path))
        .header(
            header::CACHE_CONTROL,
            if commit.is_some() { "private, max-age=31536000, immutable" } else { "private, no-cache" },
        )
        .header(header::ACCEPT_RANGES, "bytes")
        .header("x-content-type-options", "nosniff")
        .header(header::CONTENT_SECURITY_POLICY, VIEW_CSP)
        .header(header::X_FRAME_OPTIONS, "SAMEORIGIN");
    match read {
        crate::design::DesignBytes::Whole(bytes) => res.body(Body::from(bytes)),
        crate::design::DesignBytes::Part { bytes, start, end, total } => res
            .status(StatusCode::PARTIAL_CONTENT)
            .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{total}"))
            .body(Body::from(bytes)),
        crate::design::DesignBytes::Unsatisfiable { total } => res
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CONTENT_RANGE, format!("bytes */{total}"))
            .body(Body::empty()),
    }
    .unwrap()
}

/// task 첨부 파일(보기 권한도). 이름이 내용 hash라 바뀌지 않아 오래 둔다. 올린 HTML은 결과물처럼 sandbox(opaque origin)로 보내
/// 앱 cookie·API에 닿지 못하게 한다
pub(super) async fn task_file_get(
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
pub(super) fn ask_file_response(
    host: &Host,
    headers: &HeaderMap,
    path: Result<std::path::PathBuf, ErrorBody>,
    name: &str,
) -> Response {
    if device(host, headers).is_none() {
        return unauthorized();
    }
    let bytes = path.and_then(|p| {
        std::fs::read(p).map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, ErrorBody::new(ErrorCode::NotFound, "파일이 없다"))
        })
    });
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

pub(super) async fn question_file_get(
    State(host): State<Arc<Host>>,
    Path((project, thread, hash, path)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let file = host.question_file(&project, &thread, &hash, &path);
    ask_file_response(&host, &headers, file, &path)
}

pub(super) async fn ask_file_get(
    State(host): State<Arc<Host>>,
    Path((project, id, path)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let file = host.ask_file(&project, &id, &path);
    ask_file_response(&host, &headers, file, &path)
}

pub(super) async fn request_file_get(
    State(host): State<Arc<Host>>,
    Path((project, item, hash, path)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Response {
    let file = host.request_file(&project, &item, &hash, &path);
    ask_file_response(&host, &headers, file, &path)
}

/// 참고 `/api/pane/image`: 저장만 하고 경로를 돌려준다. Origin·cookie는 기존 웹 guard와 같다.
/// 인증·권한을 확인한 뒤 본문을 읽으며, base64 대신 원본 bytes를 16MiB 한도로 받는다.
pub(super) async fn chat_image(State(host): State<Arc<Host>>, Path(pane): Path<String>, req: Request) -> Response {
    let Some((id, role)) = device(&host, req.headers()) else { return unauthorized() };
    if !role.can_control() {
        return (
            StatusCode::FORBIDDEN,
            Json(Outcome::<()>::Err(ErrorBody::new(ErrorCode::Forbidden, "보기 권한으로는 붙여 넣을 수 없다"))),
        )
            .into_response();
    }
    let Ok(bytes) = axum::body::to_bytes(req.into_body(), 16 << 20)
        .await
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
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

/// 인증한 웹 기기에만 질문/hash·답한 사본에 묶인 짧은 읽기 경로를 발급한다.
/// opaque iframe의 하위 요청·새 탭·Range가 기기 cookie에 의존하지 않는다.
pub(super) fn attachment_base(host: &Host, device: &str, base: &mut Option<String>) {
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

pub(super) fn attachment_thread(host: &Host, device: &str, thread: &mut aaa_proto::ThreadView) {
    if let Some(q) = &mut thread.questions {
        attachment_base(host, device, &mut q.files_base);
    }
}

pub(super) fn attachment_views(host: &Host, device: &str, response: &mut aaa_proto::FrontResponse) {
    use aaa_proto::FrontResponse;
    match response {
        FrontResponse::Thread(t) => attachment_thread(host, device, t),
        FrontResponse::Threads { threads, .. } => {
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
        FrontResponse::AskHistory { done, .. } => {
            for record in done {
                attachment_base(host, device, &mut record.files_base);
            }
        }
        _ => {}
    }
}
