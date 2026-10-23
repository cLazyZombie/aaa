//! 설치가 선택한 한 포트만 다룬다. 읽기 오류·다른 포트 재사용·Funnel을 빈 매핑으로 보지 않는다.
use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[derive(Clone, Copy, Debug)]
pub struct ExactError {
    pub code: &'static str,
    pub conflict: bool,
    pub cause: Option<aaa_common::diagnostic::Cause>,
}
// 진단 위치·파싱 행은 같은 오류 분류의 부가 자료다. 기존 code/conflict 비교 계약을 유지한다.
impl PartialEq for ExactError {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code && self.conflict == other.conflict
    }
}
impl Eq for ExactError {}
impl std::fmt::Display for ExactError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code)?;
        if let Some(cause) = self.cause {
            write!(f, ": {cause}")?;
        }
        Ok(())
    }
}
impl std::error::Error for ExactError {}
impl aaa_common::diagnostic::WithCause for ExactError {
    fn with_cause(mut self, cause: aaa_common::diagnostic::Cause) -> Self {
        self.cause = Some(cause);
        self
    }
}
fn unknown() -> ExactError {
    ExactError { code: "tailscale_status_unknown", conflict: false, cause: None }
}
fn conflict() -> ExactError {
    ExactError { code: "serve_port_conflict", conflict: true, cause: None }
}
#[derive(Clone)]
pub struct Exact {
    pub port: u16,
    pub target_port: u16,
    pub reused: bool,
    dns: String,
    config: Value,
}
fn digest(value: &impl serde::Serialize) -> std::result::Result<String, ExactError> {
    Ok(hex::encode(Sha256::digest(
        aaa_common::install_crypto::canonical(value).map_err(|e| aaa_common::diagnostic::context(e, unknown()))?,
    )))
}
fn map<'a>(value: &'a Value, key: &str) -> std::result::Result<Option<&'a serde_json::Map<String, Value>>, ExactError> {
    match value.get(key) {
        None => Ok(None),
        Some(Value::Object(map)) => Ok(Some(map)),
        _ => Err(unknown()),
    }
}
fn occupied(value: &Value, port: u16) -> std::result::Result<bool, ExactError> {
    let tcp = map(value, "TCP")?.is_some_and(|m| m.contains_key(&port.to_string()));
    let web = map(value, "Web")?
        .is_some_and(|m| m.keys().any(|k| k.rsplit_once(':').is_some_and(|(_, p)| p.parse() == Ok(port))));
    let funnel = map(value, "AllowFunnel")?.is_some_and(|m| {
        m.iter()
            .any(|(k, v)| v.as_bool() != Some(false) && k.rsplit_once(':').is_some_and(|(_, p)| p.parse() == Ok(port)))
    });
    Ok(tcp || web || funnel)
}
pub fn exact(out: Option<&TsOutput>, target_port: u16, port: u16) -> std::result::Result<Exact, ExactError> {
    if port == 0 || target_port == 0 {
        return Err(conflict());
    }
    let out = out.ok_or(ExactError { code: "tailscale_missing", conflict: false, cause: None })?;
    let status: StatusJson = serde_json::from_str(out.status.as_deref().ok_or_else(unknown)?)
        .map_err(|e| aaa_common::diagnostic::context(e, unknown()))?;
    if status.backend_state.as_deref() != Some("Running") {
        return Err(ExactError { code: "tailscale_login_required", conflict: false, cause: None });
    }
    let dns = status.me.and_then(|node| node.dns_name).ok_or_else(unknown)?.trim_end_matches('.').to_ascii_lowercase();
    if dns.is_empty()
        || dns.len() > 253
        || !dns.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        || dns
            .split('.')
            .any(|label| label.is_empty() || label.len() > 63 || label.starts_with('-') || label.ends_with('-'))
    {
        return Err(unknown());
    }
    let mut config: Value = serde_json::from_str(out.serve.as_deref().ok_or_else(unknown)?)
        .map_err(|e| aaa_common::diagnostic::context(e, unknown()))?;
    // 성공한 status의 JSON null은 미설정이다. 깨진 JSON은 미설정이 아니다.
    if config.is_null() {
        config = json!({});
    }
    if !config.is_object()
        || config
            .as_object()
            .unwrap()
            .keys()
            .any(|key| !matches!(key.as_str(), "TCP" | "Web" | "AllowFunnel" | "Foreground" | "Services"))
    {
        return Err(unknown());
    }
    let alternate = map(&config, "Foreground")?;
    if let Some(alternate) = alternate {
        for value in alternate.values() {
            if !value.is_object() {
                return Err(unknown());
            }
            if occupied(value, port)? {
                return Err(conflict());
            }
        }
    }
    let _ = map(&config, "Services")?;
    let hp = format!("{dns}:{port}");
    let tcp = map(&config, "TCP")?.and_then(|m| m.get(&port.to_string()));
    let sites = map(&config, "Web")?;
    let site = sites.and_then(|m| m.get(&hp));
    let funnel = map(&config, "AllowFunnel")?;
    if funnel.is_some_and(|m| {
        m.iter().any(|(key, value)| {
            key.rsplit_once(':').is_some_and(|(_, p)| p.parse() == Ok(port)) && value.as_bool() != Some(false)
        })
    }) {
        return Err(conflict());
    }
    let reused = if tcp.is_none() && site.is_none() {
        if occupied(&config, port)? {
            return Err(conflict());
        }
        false
    } else {
        let tcp = tcp.and_then(Value::as_object).ok_or_else(conflict)?;
        if tcp.get("HTTPS").and_then(Value::as_bool) != Some(true)
            || tcp.iter().any(|(key, value)| match key.as_str() {
                "HTTPS" => value != &json!(true),
                "HTTP" => value != &json!(false),
                "TCPForward" | "TerminateTLS" => value != "",
                "ProxyProtocol" => value != &json!(0),
                _ => true,
            })
        {
            return Err(conflict());
        }
        if sites.is_some_and(|m| {
            m.keys().any(|key| key != &hp && key.rsplit_once(':').is_some_and(|(_, p)| p.parse() == Ok(port)))
        }) {
            return Err(conflict());
        }
        let site = site.and_then(Value::as_object).ok_or_else(conflict)?;
        if site.len() != 1 {
            return Err(conflict());
        }
        let handlers = site.get("Handlers").and_then(Value::as_object).ok_or_else(conflict)?;
        if handlers.len() != 1 {
            return Err(conflict());
        }
        let root = handlers.get("/").and_then(Value::as_object).ok_or_else(conflict)?;
        if root.len() != 1 || !proxies_to(root.get("Proxy").and_then(Value::as_str), target_port) {
            return Err(conflict());
        }
        true
    };
    Ok(Exact { port, target_port, reused, dns, config })
}
impl Exact {
    pub fn permission(&self) -> std::result::Result<aaa_proto::install::InstallPermission, ExactError> {
        Ok(aaa_proto::install::InstallPermission {
            operation: aaa_proto::install::InstallOperation::PublishWeb,
            target: format!("https:{}/loopback:{}", self.port, self.target_port),
            before_sha256: Some(self.before_sha256()?),
            after_sha256: Some(self.goal_sha256()?),
        })
    }
    pub(super) fn desired_config(&self) -> Value {
        let mut value = self.config.clone();
        for key in ["TCP", "Web"] {
            if value.get(key).is_none() {
                value[key] = json!({});
            }
        }
        value["TCP"][self.port.to_string()] = json!({"HTTPS":true});
        value["Web"][format!("{}:{}", self.dns, self.port)] =
            json!({"Handlers":{"/":{"Proxy":format!("http://127.0.0.1:{}", self.target_port)}}});
        value
    }
    pub fn url(&self) -> String {
        https_url(&self.dns, self.port)
    }
    pub fn before_sha256(&self) -> std::result::Result<String, ExactError> {
        digest(&self.config)
    }
    pub fn goal_sha256(&self) -> std::result::Result<String, ExactError> {
        digest(&(self.port, self.target_port, &self.dns, "https-loopback.v1"))
    }
    pub fn mapping_sha256(&self) -> std::result::Result<String, ExactError> {
        let hp = format!("{}:{}", self.dns, self.port);
        digest(&(
            self.config.get("TCP").and_then(|m| m.get(self.port.to_string())),
            self.config.get("Web").and_then(|m| m.get(&hp)),
            self.config.get("AllowFunnel").and_then(|m| m.get(&hp)),
        ))
    }
    pub fn preserves(&self, after: &Self) -> bool {
        fn remainder(exact: &Exact) -> Value {
            let mut value = exact.config.clone();
            for (kind, key) in [
                ("TCP", exact.port.to_string()),
                ("Web", format!("{}:{}", exact.dns, exact.port)),
                ("AllowFunnel", format!("{}:{}", exact.dns, exact.port)),
            ] {
                if let Some(map) = value.get_mut(kind).and_then(Value::as_object_mut) {
                    map.remove(&key);
                    if map.is_empty() {
                        value.as_object_mut().unwrap().remove(kind);
                    }
                }
            }
            value
        }
        self.dns == after.dns
            && self.port == after.port
            && self.target_port == after.target_port
            && remainder(self) == remainder(after)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn out(serve: &str) -> TsOutput {
        TsOutput {
            status: Some(r#"{"BackendState":"Running","Self":{"DNSName":"host-a.example.ts.net."}}"#.into()),
            serve: Some(serve.into()),
        }
    }
    #[test]
    fn exact_port_does_not_fall_back_or_treat_unreadable_and_foreign_slots_as_free() {
        for broken in ["", "{", "[]", r#"{"TCP":null}"#, r#"{"Foreground":[]} "#] {
            assert_eq!(exact(Some(&out(broken)), 7431, 8443).err().unwrap(), unknown());
        }
        for foreign in [
            r#"{"TCP":{"8443":{"TCPForward":"127.0.0.1:9000"}}}"#,
            r#"{"AllowFunnel":{"host-a.example.ts.net:8443":true}}"#,
            r#"{"Foreground":{"session":{"TCP":{"8443":{"HTTPS":true}}}}}"#,
            r#"{"Web":{"other.example.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431"}}}}}"#,
        ] {
            assert_eq!(exact(Some(&out(foreign)), 7431, 8443).err().unwrap(), conflict());
        }
        let other = out(
            r#"{"TCP":{"443":{"HTTPS":true}},"Web":{"host-a.example.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431"}}}}}"#,
        );
        let selected = exact(Some(&other), 7431, 8443).unwrap();
        assert!(!selected.reused, "같은 웹을 향하는 다른 포트도 선택 포트의 재사용이 아니다");
        assert_eq!(selected.url(), "https://host-a.example.ts.net:8443");
        assert!(!exact(Some(&out("null")), 7431, 443).unwrap().reused);
    }
    #[test]
    fn exact_mapping_reuse_and_other_services_and_funnel_are_preserved_independently() {
        let before = out(
            r#"{"TCP":{"9000":{"TCPForward":"127.0.0.1:9001"}},"AllowFunnel":{"foreign.example.ts.net:9000":true},"Services":{"svc:other":{"TCP":{"8443":{"HTTPS":true}}}}}"#,
        );
        let initial = exact(Some(&before), 7431, 8443).unwrap();
        let mut after: Value = serde_json::from_str(before.serve.as_ref().unwrap()).unwrap();
        after["TCP"]["8443"] = json!({"HTTPS":true});
        after["Web"] = json!({"host-a.example.ts.net:8443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431"}}}});
        let verified = exact(Some(&out(&after.to_string())), 7431, 8443).unwrap();
        assert!(verified.reused && initial.preserves(&verified));
        assert_eq!(verified.goal_sha256().unwrap(), initial.goal_sha256().unwrap());
        assert_ne!(verified.before_sha256().unwrap(), initial.before_sha256().unwrap());
        let mut both = after.clone();
        both["TCP"]["443"] = json!({"HTTPS":true});
        both["Web"]["host-a.example.ts.net:443"] = json!({"Handlers":{"/":{"Proxy":"http://127.0.0.1:7431"}}});
        let both = out(&both.to_string());
        assert_eq!(plan(Some(&both), 7431, &[443, 8443]).serving.as_ref().map(|(_, port)| *port), Some(443));
        let selected = installation_plan(Some(&both), 7431, &[443, 8443], Some(8443));
        assert_eq!(selected.serving.as_ref().map(|(_, port)| *port), Some(8443));
        assert!(installation_plan(Some(&before), 7431, &[443, 8443], Some(8443)).serving.is_none());
        after["TCP"]["9000"]["TCPForward"] = json!("127.0.0.1:9002");
        assert!(!initial.preserves(&exact(Some(&out(&after.to_string())), 7431, 8443).unwrap()));
    }
}
