//! 보고를 회수할 때 주는 작성 안내. 정적 휴리스틱이며 보안 판정은 브라우저 CSP·서버가 맡는다.
use std::sync::LazyLock;

use regex::Regex;

static FETCH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bfetch\s*\(").unwrap());
static LOCAL_FETCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\bfetch\s*\(\s*['\"][^/:'\"\\][^:'\"\\]*['\"]\s*[,)]"#).unwrap());

/// script를 담을 수 있는 확장자만 검사한다. 음원·자료 bytes는 읽지 않는다.
pub(crate) fn is_script(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
    matches!(ext.as_str(), "html" | "htm" | "js" | "mjs" | "svg")
}

pub(crate) fn warning(path: &str, bytes: &[u8]) -> Option<String> {
    if !is_script(path) {
        return None;
    }
    let text = String::from_utf8_lossy(bytes);
    let mut apis = Vec::new();
    for api in [
        "WebSocket",
        "EventSource",
        "sendBeacon",
        "serviceWorker",
        "localStorage",
        "sessionStorage",
        "document.cookie",
        "withCredentials",
        "createMediaElementSource",
    ] {
        if text.contains(api) {
            apis.push(api);
        }
    }
    // 동적 URL과 절대 경로는 정적으로 폴더 경계를 증명할 수 없으므로 확인을 권한다.
    if FETCH.find_iter(&text).count() > LOCAL_FETCH.find_iter(&text).count()
        || (FETCH.is_match(&text) && (text.contains("credentials") || text.contains("method")))
    {
        apis.push("fetch 경로·옵션");
    }
    if text.contains("XMLHttpRequest") {
        apis.push("XHR 경로·옵션");
    }
    (!apis.is_empty()).then(|| format!(
        "{path}: 보고서 sandbox 확인({}). 자기 보고서 폴더의 상대 URL GET/HEAD만 읽고 쿠키·저장소·앱 API·외부 연결은 쓰지 않는다. 음원 혼합은 fetch → decodeAudioData → GainNode를 쓴다(docs/library.md). 정적 검사라 실제 브라우저 확인이 필요하다.",
        apis.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permits_local_fetch_and_flags_blocked_or_unproven_apis() {
        for source in ["fetch('audio/tone.wav')", "fetch(\"./data.json\").then(r=>r.json())"] {
            assert!(warning("index.html", source.as_bytes()).is_none());
        }
        for source in [
            "fetch('/api/v1/me')",
            "fetch('https://example.com/a')",
            "fetch(url)",
            "fetch('a', {credentials:'include'})",
            "fetch('a', {method:'POST'})",
            "new WebSocket(url)",
            "new XMLHttpRequest()",
            "localStorage.x",
            "document.cookie",
            "ctx.createMediaElementSource(audio)",
        ] {
            assert!(warning("nested/play.js", source.as_bytes()).is_some(), "{source}");
        }
        assert!(warning("data.txt", b"WebSocket").is_none());
    }
}
