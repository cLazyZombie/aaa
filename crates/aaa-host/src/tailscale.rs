//! host 웹 HTTPS 공개 — aaa가 Tailscale을 아는 유일한 곳(결정 6 예외, 기획서 §6.3).
//!
//! herdr-web-ui 0.4.10(`server/tailscale.ts`, `scripts/plugin.ts`)의 방식을 따른다.
//! - 읽기: `tailscale status --json`, `tailscale serve status --json`(2.5초 timeout)
//! - 공개: 이미 aaa 웹을 proxy하는 HTTPS가 있으면 그 주소, 없으면 후보 포트 중 serve의 TCP에 없는 첫 포트로
//!   `tailscale serve --bg --yes --https=<포트> http://127.0.0.1:<web_port>`(180초)
//! - 끄기: aaa가 만든 포트만 `tailscale serve --https=<포트> off`. `serve reset`은 쓰지 않는다.
//!
//! 차이: 입장은 aaa pairing만 쓰고 `Tailscale-User-Login` 헤더는 보지 않는다(정할 것 4).

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{Result, bail};
use serde::Deserialize;

mod exact;
mod localapi;
pub use exact::{Exact, ExactError, exact};
pub use localapi::{Conditional, conditional_snapshot};

pub const MAC_APP_CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";
const READ_TIMEOUT: Duration = Duration::from_millis(2500);
const SERVE_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, Default)]
pub struct TsOutput {
    pub status: Option<String>,
    pub serve: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// missing | stopped | running
    pub state: &'static str,
    pub dns_name: Option<String>,
    /// aaa 웹을 이미 proxy하는 HTTPS 주소와 포트
    pub serving: Option<(String, u16)>,
    /// 새로 쓸 수 있는 첫 후보 포트
    pub free_port: Option<u16>,
    /// serve가 이미 쓰는 포트(기록용)
    pub taken: Vec<u16>,
}

#[derive(Deserialize)]
struct StatusJson {
    #[serde(rename = "BackendState")]
    backend_state: Option<String>,
    #[serde(rename = "Self")]
    me: Option<NodeJson>,
}

#[derive(Deserialize)]
struct NodeJson {
    #[serde(rename = "DNSName")]
    dns_name: Option<String>,
}

#[derive(Deserialize, Default)]
struct ServeJson {
    #[serde(rename = "TCP", default)]
    tcp: BTreeMap<String, TcpJson>,
    #[serde(rename = "Web", default)]
    web: BTreeMap<String, WebJson>,
}

#[derive(Deserialize, Default)]
struct TcpJson {
    #[serde(rename = "HTTPS", default)]
    https: bool,
}

#[derive(Deserialize, Default)]
struct WebJson {
    #[serde(rename = "Handlers", default)]
    handlers: BTreeMap<String, HandlerJson>,
}

#[derive(Deserialize, Default)]
struct HandlerJson {
    #[serde(rename = "Proxy")]
    proxy: Option<String>,
}

fn proxies_to(target: Option<&str>, port: u16) -> bool {
    let Some(t) = target else { return false };
    let rest = t.strip_prefix("http://").unwrap_or(t);
    let (hostport, path) = rest.split_once('/').unwrap_or((rest, ""));
    let (host, p) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()),
        None => (hostport, Some(80)),
    };
    matches!(host, "127.0.0.1" | "localhost" | "[::1]") && p == Some(port) && path.is_empty()
}

pub fn https_url(host: &str, port: u16) -> String {
    if port == 443 { format!("https://{host}") } else { format!("https://{host}:{port}") }
}

// serde의 struct는 빈 sequence도 읽을 수 있다. Serve의 객체 자리를 배열로 받은 것은 미설정이 아니다.
fn object_values(value: &serde_json::Value, key: &str) -> bool {
    value.get(key).is_none_or(|v| v.as_object().is_some_and(|map| map.values().all(serde_json::Value::is_object)))
}

/// 순수 판정: 두 명령의 출력(또는 없음)으로 공개 계획을 만든다.
pub fn plan(out: Option<&TsOutput>, web_port: u16, candidates: &[u16]) -> Plan {
    let none = Plan { state: "missing", dns_name: None, serving: None, free_port: None, taken: vec![] };
    let Some(out) = out else { return none };
    let status: Option<StatusJson> = out.status.as_deref().and_then(|s| serde_json::from_str(s).ok());
    let Some(status) = status.filter(|s| s.backend_state.as_deref() == Some("Running")) else {
        return Plan { state: "stopped", ..none };
    };
    let dns = status.me.and_then(|m| m.dns_name).map(|d| d.trim_end_matches('.').to_string()).filter(|d| !d.is_empty());
    let serve = out
        .serve
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .filter(|v| {
            v.is_null()
                || (v.is_object()
                    && object_values(v, "TCP")
                    && object_values(v, "Web")
                    && v.get("Web")
                        .and_then(serde_json::Value::as_object)
                        .is_none_or(|sites| sites.values().all(|site| object_values(site, "Handlers"))))
        })
        .and_then(|v| serde_json::from_value::<Option<ServeJson>>(v).ok());
    let serve = match serve {
        Some(serve) => serve.unwrap_or_default(),
        _ => return Plan { state: "error", dns_name: dns, ..none },
    };
    let taken: Vec<u16> = serve.tcp.keys().filter_map(|k| k.parse().ok()).collect();
    let mut serving = None;
    for (hostport, site) in &serve.web {
        let Some((host, p)) = hostport.rsplit_once(':') else { continue };
        let Ok(p) = p.parse::<u16>() else { continue };
        // HTTP listener는 쓸모가 없다(다른 기기에서 secure context가 아니다)
        if !serve.tcp.get(&p.to_string()).is_some_and(|t| t.https) {
            continue;
        }
        if proxies_to(site.handlers.get("/").and_then(|h| h.proxy.as_deref()), web_port) {
            serving = Some((https_url(host, p), p));
            break;
        }
    }
    let free_port = if serving.is_none() { candidates.iter().copied().find(|c| !taken.contains(c)) } else { None };
    Plan { state: "running", dns_name: dns, serving, free_port, taken }
}

/// 설치에서 명시한 공개 포트를 읽기 UI에서도 유지한다. 수동 W0 공개는 기존 자동 정책을 쓴다.
pub fn installation_plan(out: Option<&TsOutput>, web_port: u16, candidates: &[u16], selected: Option<u16>) -> Plan {
    let mut result = plan(out, web_port, candidates);
    if let Some(selected) = selected {
        match exact(out, web_port, selected) {
            Ok(exact) => result.serving = exact.reused.then(|| (exact.url(), exact.port)),
            Err(error) => {
                result.serving = None;
                if !error.conflict && result.state == "running" {
                    result.state = "error";
                    result.free_port = None;
                }
            }
        }
    }
    result
}

pub fn binary() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let p = dir.join("tailscale");
        if p.is_file() {
            return Some(p.to_string_lossy().into_owned());
        }
    }
    std::path::Path::new(MAC_APP_CLI).exists().then(|| MAC_APP_CLI.to_string())
}

async fn run(bin: &str, args: &[&str], timeout: Duration) -> Result<(bool, String)> {
    let child = tokio::process::Command::new(bin)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(timeout, child).await {
        Ok(Ok(o)) => {
            let mut text = String::from_utf8_lossy(&o.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            Ok((o.status.success(), text))
        }
        Ok(Err(e)) => Err(e.into()),
        Err(_) => bail!("tailscale {}이 {}초 안에 끝나지 않았다", args.join(" "), timeout.as_secs()),
    }
}

pub async fn read() -> Option<TsOutput> {
    let bin = binary()?;
    let (s, v) = tokio::join!(
        run(&bin, &["status", "--json"], READ_TIMEOUT),
        run(&bin, &["serve", "status", "--json"], READ_TIMEOUT)
    );
    let ok = |r: Result<(bool, String)>| r.ok().filter(|(ok, _)| *ok).map(|(_, t)| t);
    Some(TsOutput { status: ok(s), serve: ok(v) })
}

/// 새 포트에 공개한다. 성공하면 (주소, 포트). 처음이면 tailnet HTTPS 승인 링크가 출력에 나올 수 있다.
pub async fn serve(port: u16, web_port: u16) -> Result<String> {
    let bin = binary().ok_or_else(|| anyhow::anyhow!("tailscale CLI가 없다"))?;
    let https = format!("--https={port}");
    let target = format!("http://127.0.0.1:{web_port}");
    let (ok, text) = run(&bin, &["serve", "--bg", "--yes", &https, &target], SERVE_TIMEOUT).await?;
    if !ok {
        bail!("tailscale serve 실패: {}", text.trim());
    }
    Ok(text)
}

pub async fn serve_off(port: u16) -> Result<()> {
    let bin = binary().ok_or_else(|| anyhow::anyhow!("tailscale CLI가 없다"))?;
    let https = format!("--https={port}");
    let (ok, text) = run(&bin, &["serve", &https, "off"], Duration::from_secs(30)).await?;
    if !ok {
        bail!("tailscale serve off 실패: {}", text.trim());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &str =
        r#"{"BackendState":"Running","Self":{"DNSName":"host1.example.ts.net.","UserID":123456789012345678}}"#;
    const CANDS: &[u16] = &[443, 8443, 7443, 17443];

    fn out(serve: &str) -> TsOutput {
        TsOutput { status: Some(STATUS.into()), serve: Some(serve.into()) }
    }

    #[test]
    fn missing_and_stopped() {
        assert_eq!(plan(None, 7431, CANDS).state, "missing");
        let stopped = TsOutput { status: Some(r#"{"BackendState":"NeedsLogin"}"#.into()), serve: None };
        assert_eq!(plan(Some(&stopped), 7431, CANDS).state, "stopped");
        let unreadable = TsOutput { status: None, serve: None };
        assert_eq!(plan(Some(&unreadable), 7431, CANDS).state, "stopped");
    }

    #[test]
    fn empty_serve_takes_first_candidate() {
        let p = plan(Some(&out("{}")), 7431, CANDS);
        assert_eq!(
            (p.state, p.dns_name.as_deref(), p.free_port, p.serving),
            ("running", Some("host1.example.ts.net"), Some(443), None)
        );
    }

    #[test]
    fn unreadable_serve_is_unknown_instead_of_an_empty_configuration() {
        for serve in [
            "",
            "{",
            "[]",
            r#"{"TCP":"broken"}"#,
            r#"{"Web":null}"#,
            r#"{"TCP":{"443":[]}}"#,
            r#"{"Web":{"mac-a.example.ts.net:443":[]}}"#,
            r#"{"Web":{"mac-a.example.ts.net:443":{"Handlers":{"/":[]}}}}"#,
        ] {
            let p = plan(Some(&out(serve)), 7431, CANDS);
            assert_eq!(p.state, "error");
            assert!(p.serving.is_none() && p.free_port.is_none());
        }
        assert_eq!(plan(Some(&out("null")), 7431, CANDS).free_port, Some(443));
        let selected = installation_plan(Some(&out(r#"{"FutureFormat":{}}"#)), 7431, CANDS, Some(443));
        assert_eq!(selected.state, "error");
    }

    #[test]
    fn skips_ports_used_by_other_services_and_never_reuses_them() {
        // herdr-web-ui가 443을 7317로 쓰고 있다: 덮지 않고 다음 후보로 간다.
        let serve = r#"{"TCP":{"443":{"HTTPS":true}},"Web":{"host1.example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7317"}}}}}"#;
        let p = plan(Some(&out(serve)), 7431, CANDS);
        assert_eq!((p.free_port, p.serving, p.taken), (Some(8443), None, vec![443]));
    }

    #[test]
    fn reuses_existing_https_mapping_to_our_port() {
        let serve = r#"{"TCP":{"443":{"HTTPS":true},"17443":{"HTTPS":true}},"Web":{
            "host1.example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7317"}}},
            "host1.example.ts.net:17443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431"}}}}}"#;
        let p = plan(Some(&out(serve)), 7431, CANDS);
        assert_eq!(p.serving, Some(("https://host1.example.ts.net:17443".into(), 17443)));
        assert_eq!(p.free_port, None);
    }

    #[test]
    fn ignores_http_listener_and_subpath_proxy() {
        let serve = r#"{"TCP":{"80":{"HTTP":true},"8443":{"HTTPS":true}},"Web":{
            "host1.example.ts.net:80":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431"}}},
            "host1.example.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431/sub"}}}}}"#;
        let p = plan(Some(&out(serve)), 7431, CANDS);
        assert_eq!((p.serving, p.free_port), (None, Some(443)));
    }

    /// 같은 Mac의 둘째 aaa(plan §8.294): 기본 인스턴스가 443을 7431로 쓰면 7433의 인스턴스는 그 주소를 제 것으로 보지 않고
    /// 다음 후보를 고른다. 기본 인스턴스도 인스턴스의 8443을 제 것으로 보지 않는다
    #[test]
    fn another_instance_on_the_same_mac_keeps_its_port() {
        let serve = r#"{"TCP":{"443":{"HTTPS":true},"8443":{"HTTPS":true}},"Web":{
            "host1.example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431"}}},
            "host1.example.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7433"}}}}}"#;
        let work = plan(Some(&out(serve)), 7433, CANDS);
        assert_eq!(work.serving, Some(("https://host1.example.ts.net:8443".into(), 8443)));
        let default = plan(Some(&out(serve)), 7431, CANDS);
        assert_eq!(default.serving, Some(("https://host1.example.ts.net".into(), 443)));
        let fresh = r#"{"TCP":{"443":{"HTTPS":true}},"Web":{
            "host1.example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431"}}}}}"#;
        let p = plan(Some(&out(fresh)), 7433, CANDS);
        assert_eq!((p.serving, p.free_port), (None, Some(8443)));
    }

    #[test]
    fn all_candidates_taken() {
        let serve =
            r#"{"TCP":{"443":{"HTTPS":true},"8443":{"HTTPS":true},"7443":{"HTTPS":true},"17443":{"HTTPS":true}}}"#;
        assert_eq!(plan(Some(&out(serve)), 7431, CANDS).free_port, None);
    }
}
