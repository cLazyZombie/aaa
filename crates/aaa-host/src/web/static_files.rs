//! 내장한 웹 자산 보내기.

use super::*;

/// 웹 자산의 Content-Type. 확장자는 마지막 경로 조각에서만 본다(`fonts/meslotendard-0.3.1/LICENSE`의 버전 점을
/// 확장자로 읽지 않게). 글꼴 고지(LICENSE·.txt·.md)는 브라우저가 바로 보이게 글자로 보낸다.
pub(super) fn content_type(path: &str) -> &'static str {
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
pub(super) fn touch_icon_alias(path: &str) -> &str {
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

pub(super) async fn static_file(req: Request) -> Response {
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
    let found = assets::WEB_ASSETS.iter().find(|asset| asset.name == want);
    // 없는 파일 주소에는 화면(HTML)이 아니라 404를 준다. 그림 주소에 HTML을 200으로 주면 그것을 아이콘으로 받는
    // 쪽(iOS 홈 화면·앱 전환)이 깨진 그림을 갖는다. 확장자 없는 주소만 화면 경로다(참고 static.ts는 /assets·/icons만 404).
    if found.is_none() && path.rsplit('/').next().is_some_and(|f| f.contains('.')) {
        return (StatusCode::NOT_FOUND, [(header::CACHE_CONTROL, "no-cache")], "없는 파일").into_response();
    }
    let asset = found.or_else(|| assets::WEB_ASSETS.iter().find(|asset| asset.name == "index.html")).unwrap();
    let name = asset.name;
    // assets/는 이름에 hash가, fonts/는 폴더에 버전이 있다(글꼴을 바꾸면 폴더도 바꾼다)
    let cache = if name.starts_with("assets/") || name.starts_with("fonts/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let compressed = asset.gzip && assets::accepts_gzip(req.headers());
    let body = if compressed {
        Body::from(asset.data)
    } else {
        match asset.identity() {
            Ok(std::borrow::Cow::Borrowed(bytes)) => Body::from(bytes),
            Ok(std::borrow::Cow::Owned(bytes)) => Body::from(bytes),
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                return (StatusCode::INTERNAL_SERVER_ERROR, "웹 자산을 복원하지 못했다").into_response();
            }
        }
    };
    let mut response =
        Response::builder().header(header::CONTENT_TYPE, content_type(name)).header(header::CACHE_CONTROL, cache);
    if asset.gzip {
        response = response.header(header::VARY, "Accept-Encoding");
    }
    if compressed {
        response = response.header(header::CONTENT_ENCODING, "gzip");
    }
    response.body(body).unwrap()
}
