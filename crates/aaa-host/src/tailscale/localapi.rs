//! Windows의 관리자 보호 LocalAPI에서 ETag 조건부 쓰기. 구판/거부는 비조건부 CLI로 우회하지 않는다.
use super::*;
use exact::ExactError;
use tokio::io::{AsyncRead, AsyncWrite};
const CONFIG: &str = "/localapi/v0/serve-config";
const STATUS: &str = "/localapi/v0/status";
const MAX: usize = 1024 * 1024;
fn unavailable() -> ExactError {
    ExactError { code: "tailscale_conditional_apply_unavailable", conflict: false }
}
fn permission() -> ExactError {
    ExactError { code: "tailscale_serve_permission_required", conflict: false }
}
fn conflict() -> ExactError {
    ExactError { code: "serve_port_conflict", conflict: true }
}
fn response(response: &crate::http::Response) -> std::result::Result<(), ExactError> {
    match response.status {
        200 => Ok(()),
        412 => Err(conflict()),
        401 | 403 => Err(permission()),
        _ => Err(ExactError { code: "tailscale_serve_setup_required", conflict: false }),
    }
}
async fn connect() -> std::result::Result<aaa_common::ipc::LocalStream, ExactError> {
    #[cfg(windows)]
    {
        // 이 prefix는 관리자만 server를 만들 수 있다. 임의 pipe/user-mode fallback은 사용하지 않는다.
        let pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(r"\\.\pipe\ProtectedPrefix\Administrators\Tailscale\tailscaled")
            .map_err(|_| unavailable())?;
        Ok(Box::new(pipe))
    }
    #[cfg(not(windows))]
    {
        Err(unavailable())
    }
}
async fn request(
    method: &str,
    path: &str,
    headers: &[(&str, String)],
    body: &[u8],
) -> std::result::Result<crate::http::Response, ExactError> {
    tokio::time::timeout(Duration::from_secs(10), async {
        crate::http::request_stream(connect().await?, method, path, headers, body, MAX).await.map_err(|_| unavailable())
    })
    .await
    .map_err(|_| unavailable())?
}
pub struct Conditional {
    pub snapshot: Exact,
    etag: String,
}
pub async fn conditional_snapshot(target_port: u16, port: u16) -> std::result::Result<Conditional, ExactError> {
    let status = request("GET", STATUS, &[], &[]).await?;
    response(&status)?;
    let config = request("GET", CONFIG, &[], &[]).await?;
    response(&config)?;
    let etag = config
        .header("etag")
        .filter(|etag| !etag.is_empty() && etag.len() <= 256 && etag.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or_else(unavailable)?
        .to_owned();
    let status = String::from_utf8(status.body).map_err(|_| unavailable())?;
    let serve = String::from_utf8(config.body).map_err(|_| unavailable())?;
    let snapshot = exact(Some(&TsOutput { status: Some(status), serve: Some(serve) }), target_port, port)?;
    Ok(Conditional { snapshot, etag })
}
async fn post<S: AsyncRead + AsyncWrite + Unpin>(
    stream: S,
    conditional: &Conditional,
) -> std::result::Result<(), ExactError> {
    if conditional.etag.is_empty()
        || conditional.etag.len() > 256
        || !conditional.etag.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(unavailable());
    }
    let body = serde_json::to_vec(&conditional.snapshot.desired_config()).map_err(|_| unavailable())?;
    let reply = crate::http::request_stream(
        stream,
        "POST",
        CONFIG,
        &[("If-Match", conditional.etag.clone()), ("Content-Type", "application/json".into())],
        &body,
        MAX,
    )
    .await
    .map_err(|_| unavailable())?;
    response(&reply)
}
impl Conditional {
    pub async fn publish(self) -> std::result::Result<Exact, ExactError> {
        if self.snapshot.reused {
            return Ok(self.snapshot);
        }
        tokio::time::timeout(Duration::from_secs(10), async { post(connect().await?, &self).await })
            .await
            .map_err(|_| unavailable())??;
        let after = conditional_snapshot(self.snapshot.target_port, self.snapshot.port).await?.snapshot;
        if !after.reused || !self.snapshot.preserves(&after) {
            return Err(conflict());
        }
        Ok(after)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn conditional_write_sends_if_match_and_preserves_foreign_config_and_stale_is_conflict() {
        for stale in [false, true] {
            let out = TsOutput { status: Some(r#"{"BackendState":"Running","Self":{"DNSName":"host-a.example.ts.net."}}"#.into()), serve: Some(r#"{"TCP":{"9000":{"TCPForward":"127.0.0.1:9001"}},"AllowFunnel":{"foreign.example.ts.net:9000":true}}"#.into()) };
            let conditional =
                Conditional { snapshot: exact(Some(&out), 7431, 8443).unwrap(), etag: "\"fixture-etag\"".into() };
            let (client, mut server) = tokio::io::duplex(8192);
            let peer = tokio::spawn(async move {
                let mut head = Vec::new();
                while !head.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    server.read_exact(&mut byte).await.unwrap();
                    head.extend(byte);
                    assert!(head.len() < 8192);
                }
                let head = String::from_utf8(head).unwrap();
                assert!(head.starts_with("POST /localapi/v0/serve-config HTTP/1.1\r\nHost: local-tailscaled.sock\r\n"));
                assert!(head.lines().any(|line| line == "If-Match: \"fixture-etag\""));
                let length: usize =
                    head.lines().find_map(|line| line.strip_prefix("Content-Length: ")).unwrap().parse().unwrap();
                let mut body = vec![0; length];
                server.read_exact(&mut body).await.unwrap();
                let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(value["TCP"]["9000"]["TCPForward"], "127.0.0.1:9001");
                assert_eq!(value["AllowFunnel"]["foreign.example.ts.net:9000"], true);
                assert_eq!(value["TCP"]["8443"]["HTTPS"], true);
                assert_eq!(
                    value["Web"]["host-a.example.ts.net:8443"]["Handlers"]["/"]["Proxy"],
                    "http://127.0.0.1:7431"
                );
                assert!(value["AllowFunnel"].get("host-a.example.ts.net:8443").is_none());
                let status = if stale { "412 Precondition Failed" } else { "200 OK" };
                server.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n").as_bytes()).await.unwrap();
            });
            let result = post(client, &conditional).await;
            if stale {
                assert_eq!(result.unwrap_err(), conflict());
            } else {
                result.unwrap();
            }
            peer.await.unwrap();
        }
    }
}
