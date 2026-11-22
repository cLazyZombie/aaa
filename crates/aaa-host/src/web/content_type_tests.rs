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
        assert!(
            line.contains("include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/../../web/dist/")
                || line.contains("include_bytes!(concat!(env!(\"OUT_DIR\"), \"/web-asset-"),
            "{line}"
        );
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
    for kind in ["image/svg+xml", "text/plain; charset=utf-8", "application/pdf", "text/javascript; charset=utf-8"] {
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
