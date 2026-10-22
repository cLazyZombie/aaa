//! 실행 파일에는 작은 표현만 넣고 HTTP 클라이언트가 gzip을 못 받으면 원문으로 복원한다.
use std::borrow::Cow;
use std::io::Read as _;

pub struct Asset {
    pub name: &'static str,
    pub data: &'static [u8],
    pub gzip: bool,
}

include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));

impl Asset {
    pub fn identity(&self) -> std::io::Result<Cow<'static, [u8]>> {
        if !self.gzip {
            return Ok(Cow::Borrowed(self.data));
        }
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(self.data).read_to_end(&mut bytes)?;
        Ok(Cow::Owned(bytes))
    }
}

pub fn accepts_gzip(headers: &axum::http::HeaderMap) -> bool {
    let mut gzip = None;
    let mut wildcard = false;
    for value in headers.get_all(axum::http::header::ACCEPT_ENCODING) {
        let Ok(value) = value.to_str() else { continue };
        for encoding in value.split(',') {
            let mut parts = encoding.trim().split(';');
            let name = parts.next().unwrap_or("").trim();
            let mut accepted = true;
            for parameter in parts {
                if let Some((key, value)) = parameter.trim().split_once('=')
                    && key.trim().eq_ignore_ascii_case("q")
                {
                    accepted = value.trim().parse::<f32>().is_ok_and(|q| q > 0.0 && q <= 1.0);
                }
            }
            if name.eq_ignore_ascii_case("gzip") {
                gzip = Some(accepted);
            } else if name == "*" {
                wildcard = accepted;
            }
        }
    }
    gzip.unwrap_or(wildcard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_assets_restore_exact_build_input() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
        fn files(root: &std::path::Path, dir: &std::path::Path, names: &mut std::collections::BTreeSet<String>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    files(root, &path, names);
                } else {
                    names.insert(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
                }
            }
        }
        for asset in WEB_ASSETS {
            assert_eq!(
                asset.identity().unwrap().as_ref(),
                std::fs::read(root.join(asset.name)).unwrap(),
                "{}",
                asset.name
            );
        }
        if root.is_dir() {
            let mut names = std::collections::BTreeSet::new();
            files(&root, &root, &mut names);
            assert_eq!(names, WEB_ASSETS.iter().map(|asset| asset.name.to_owned()).collect(), "웹 자산을 누락했다");
            assert!(WEB_ASSETS.iter().any(|a| a.name == "index.html"));
            assert!(WEB_ASSETS.iter().any(|a| a.gzip));
        }
    }

    #[test]
    fn gzip_negotiation_respects_explicit_refusal_and_multiple_headers() {
        use axum::http::{HeaderMap, HeaderValue, header::ACCEPT_ENCODING};
        for (values, expected) in [
            (vec![], false),
            (vec![""], false),
            (vec!["br, gzip, deflate"], true),
            (vec!["GZip; q=0.5"], true),
            (vec!["gzip;q=0, *;q=1"], false),
            (vec!["*;q=1", "gzip;q=0.000"], false),
            (vec!["br", "gzip"], true),
            (vec!["*;q=0.3"], true),
            (vec!["gzip;q=NaN"], false),
            (vec!["gzip;q=2"], false),
        ] {
            let mut headers = HeaderMap::new();
            for value in values {
                headers.append(ACCEPT_ENCODING, HeaderValue::from_str(value).unwrap());
            }
            assert_eq!(accepts_gzip(&headers), expected, "{headers:?}");
        }
    }

    #[tokio::test]
    async fn http_keeps_identity_gzip_cache_types_and_missing_routes() {
        use axum::{
            body::{Body, to_bytes},
            http::{Request, StatusCode, header},
        };
        if WEB_ASSETS.is_empty() {
            return;
        }
        for asset in WEB_ASSETS.iter().filter(|a| {
            a.name == "index.html"
                || a.name == "sw.js"
                || a.name.ends_with(".woff2")
                || a.name.starts_with("assets/index-")
        }) {
            for accept in ["gzip", "identity", "gzip;q=0, *;q=1"] {
                let response = super::super::static_file(
                    Request::builder()
                        .uri(format!("/{}", asset.name))
                        .header(header::ACCEPT_ENCODING, accept)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(response.headers()[header::CONTENT_TYPE], super::super::content_type(asset.name));
                let immutable = asset.name.starts_with("assets/") || asset.name.starts_with("fonts/");
                assert_eq!(
                    response.headers()[header::CACHE_CONTROL].to_str().unwrap().contains("immutable"),
                    immutable
                );
                if asset.gzip {
                    assert_eq!(response.headers()[header::VARY], "Accept-Encoding");
                }
                let gzip = asset.gzip && accept == "gzip";
                assert_eq!(response.headers().contains_key(header::CONTENT_ENCODING), gzip);
                let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
                if gzip {
                    assert_eq!(&bytes[..], asset.data);
                } else {
                    assert_eq!(&bytes[..], asset.identity().unwrap().as_ref());
                }
            }
        }
        for (path, status) in [
            ("/settings", StatusCode::OK),
            ("/missing.js", StatusCode::NOT_FOUND),
            ("/api/missing", StatusCode::NOT_FOUND),
            ("/v/missing", StatusCode::NOT_FOUND),
        ] {
            let response = super::super::static_file(Request::builder().uri(path).body(Body::empty()).unwrap()).await;
            assert_eq!(response.status(), status, "{path}");
        }
        let index = WEB_ASSETS.iter().find(|a| a.name == "index.html").unwrap().identity().unwrap();
        assert!(super::super::main_bundle(&String::from_utf8_lossy(&index)).is_some());

        use tower::ServiceExt as _;
        let response = axum::Router::new()
            .fallback(axum::routing::get(super::super::static_file))
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri("/")
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
        assert!(to_bytes(response.into_body(), usize::MAX).await.unwrap().is_empty());
    }
}
