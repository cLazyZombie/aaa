//! 계정 pane의 인증 수명과 Codex refresh 중계. 비밀은 stdin에서만 받는다.
use aaa_proto::{AccountCredential, AccountRefresh, AccountRefreshed, SecretText};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

pub struct BrokerRequest {
    pub request: AccountRefresh,
    pub reply: oneshot::Sender<AccountRefreshed>,
}
pub struct Prepared {
    pub env: Vec<(String, String)>,
    home: Option<PathBuf>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Drop for Prepared {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        if let Some(home) = &self.home {
            let _ = std::fs::remove_file(home.join("auth.json"));
        }
    }
}
pub async fn prepare(
    home: &Path,
    credential: AccountCredential,
    exe: &Path,
    tx: mpsc::Sender<BrokerRequest>,
    resume: bool,
) -> Result<Prepared> {
    aaa_common::redact::remember_auth(&credential.harness, &credential.secret.0);
    if credential.harness == "claude"
        && (!aaa_proto::account_run_parts(&credential.run).is_some_and(|(_, task)| aaa_proto::valid_thread(task))
            || (resume && !home.exists()))
    {
        // 도입 전 로컬 홈에 남긴 대화의 재개는 원래 기록 홈을 유지한다.
        return Ok(Prepared {
            env: vec![("CLAUDE_CODE_OAUTH_TOKEN".into(), credential.secret.0)],
            home: None,
            task: None,
        });
    }
    if home.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("인증 홈은 symlink일 수 없다")
    }
    aaa_common::util::ensure_private_dir(home)?;
    // cwd를 먼저 대조한다. host의 허용 경로를 부모 repo나 다른 작업까지 넓히지 않는다.
    let trusted = credential
        .trust_cwd
        .as_deref()
        .map(|cwd| {
            let path = Path::new(cwd);
            anyhow::ensure!(path.is_absolute() && path.is_dir(), "신뢰할 작업 폴더가 없다");
            std::fs::canonicalize(path).context("신뢰할 작업 폴더 확인 실패")
        })
        .transpose()?;
    match credential.harness.as_str() {
        "claude" => {
            let mut projects = serde_json::Map::new();
            if let Some(cwd) = trusted {
                projects
                    .insert(cwd.to_string_lossy().into_owned(), serde_json::json!({"hasTrustDialogAccepted": true}));
            }
            // 로컬 .claude.json에는 인증도 있으므로 읽거나 복사하지 않는다. 이 실행의 전용 파일만 만든다.
            let path = home.join(".claude.json");
            anyhow::ensure!(
                !path.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()),
                "격리 Claude 설정은 symlink일 수 없다"
            );
            let mut config = match std::fs::read(&path) {
                Ok(bytes) => serde_json::from_slice::<serde_json::Value>(&bytes)
                    .map_err(|_| anyhow::anyhow!("격리 Claude 설정 형식 오류"))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => serde_json::json!({}),
                Err(_) => bail!("격리 Claude 설정 읽기 실패"),
            };
            let config = config.as_object_mut().context("격리 Claude 설정 형식 오류")?;
            // host에서 로그인한 토큰으로 시작한다. 빈 격리 홈에서 로그인 안내를 다시 열지 않는다.
            config.entry("hasCompletedOnboarding").or_insert(true.into());
            config.insert("projects".into(), projects.into());
            private(&path, &serde_json::to_vec(config)?)?;
            Ok(Prepared {
                env: vec![
                    ("CLAUDE_CODE_OAUTH_TOKEN".into(), credential.secret.0),
                    ("CLAUDE_CONFIG_DIR".into(), home.to_string_lossy().into_owned()),
                ],
                home: None,
                task: None,
            })
        }
        "codex" => {
            let home = home.to_path_buf();
            let mut out = Prepared { env: vec![], home: Some(home.clone()), task: None };
            private(&home.join("auth.json"), credential.secret.0.as_bytes())?;
            let notify = serde_json::to_string(&vec![
                exe.to_string_lossy().to_string(),
                "hook".into(),
                "--agent".into(),
                "codex".into(),
            ])?;
            let mut config = format!("cli_auth_credentials_store = \"file\"\nnotify = {notify}\n");
            if let Some(cwd) = trusted {
                // JSON 문자열은 TOML basic string으로도 유효하다(따옴표·역슬래시를 escape).
                let key = serde_json::to_string(&cwd.to_string_lossy())?;
                config.push_str(&format!("\n[projects.{key}]\ntrust_level = \"trusted\"\n"));
            }
            private(&home.join("config.toml"), config.as_bytes())?;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let token = hex::encode(rand::random::<[u8; 32]>());
            let url = format!("http://{}/{token}", listener.local_addr()?);
            aaa_common::redact::remember(&token);
            out.env = vec![
                ("CODEX_HOME".into(), home.to_string_lossy().to_string()),
                ("CODEX_REFRESH_TOKEN_URL_OVERRIDE".into(), url),
            ];
            let run = credential.run;
            let version = std::sync::Arc::new(tokio::sync::Mutex::new(credential.version));
            out.task = Some(tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let (tx, token, run, version) = (tx.clone(), token.clone(), run.clone(), version.clone());
                    tokio::spawn(async move {
                        let _ = serve(stream, &token, run, version, tx).await;
                    });
                }
            }));
            Ok(out)
        }
        _ => bail!("지원하지 않는 인증 harness"),
    }
}
pub fn private(path: &Path, bytes: &[u8]) -> Result<()> {
    // 기존 auth 파일/symlink를 따라 쓰지 않고 새 파일로 교체한다.
    let dir = path.parent().context("인증 폴더가 없다")?;
    let tmp = dir.join(format!(".{}.tmp", aaa_common::util::new_id()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    use std::io::Write;
    let result = (|| {
        let mut f = o.open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map_err(Into::into)
}
async fn serve(
    mut stream: tokio::net::TcpStream,
    token: &str,
    run: String,
    version: std::sync::Arc<tokio::sync::Mutex<u64>>,
    tx: mpsc::Sender<BrokerRequest>,
) -> Result<()> {
    // 작업의 refresh만 직렬화한다. PTY 작업 실행은 잠그지 않는다.
    let mut current = version.lock().await;
    let body = tokio::time::timeout(std::time::Duration::from_secs(10), read_http(&mut stream, token)).await??;
    let id = aaa_common::util::new_id();
    let request = AccountRefresh { run, session: String::new(), id, version: *current, request: SecretText(body) };
    let (reply, rx) = oneshot::channel();
    tx.send(BrokerRequest { request, reply }).await.map_err(|_| anyhow::anyhow!("인증 중계가 닫혔다"))?;
    let response = tokio::time::timeout(std::time::Duration::from_secs(20), rx).await;
    let (status, body) = match response {
        Ok(Ok(r)) if r.response.is_some() => {
            *current = r.version;
            let b = r.response.unwrap().0;
            aaa_common::redact::remember_auth("codex", &b);
            ("200 OK", b)
        }
        _ => ("503 Service Unavailable", "{\"error\":\"credential_refresh_unavailable\"}".into()),
    };
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}
async fn read_http(stream: &mut tokio::net::TcpStream, token: &str) -> Result<String> {
    let mut buf = Vec::new();
    let mut chunk = [0; 4096];
    let split = loop {
        if let Some(at) = buf.windows(4).position(|v| v == b"\r\n\r\n") {
            break at;
        }
        if buf.len() > 16 * 1024 {
            bail!("인증 요청 머리 상한")
        }
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            bail!("인증 요청 끝")
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..split])?;
    let mut lines = head.lines();
    if lines.next() != Some(&format!("POST /{token} HTTP/1.1")) {
        bail!("허용하지 않는 인증 요청")
    }
    let sizes: Vec<_> = lines
        .filter_map(|l| l.split_once(':'))
        .filter(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .map(|(_, v)| v.trim().parse::<usize>())
        .collect();
    if sizes.len() != 1 {
        bail!("인증 요청 길이 오류")
    }
    let len = *sizes[0].as_ref().map_err(|_| anyhow::anyhow!("인증 요청 길이 오류"))?;
    if len > 128 * 1024 {
        bail!("인증 요청 본문 상한")
    }
    let end = split + 4 + len;
    while buf.len() < end {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            bail!("인증 요청 끝")
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8(buf[split + 4..end].to_vec()).map_err(|_| anyhow::anyhow!("인증 요청 형식 오류"))
}

pub const AUTH_ENV: &[&str] = &[
    "CLAUDE_CODE_OAUTH_TOKEN",
    "CLAUDE_CODE_OAUTH_REFRESH_TOKEN",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_API_KEY",
    "CODEX_API_KEY",
    "CODEX_ACCESS_TOKEN",
    "CODEX_AUTH_IDENTITY_TOKEN",
    "CODEX_REFRESH_TOKEN_URL_OVERRIDE",
    "CODEX_SQLITE_HOME",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
];

#[cfg(test)]
mod tests {
    use super::*;

    struct Lab(PathBuf);
    impl Lab {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("aaa-account-trust-{}", aaa_common::util::new_id()));
            std::fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Lab {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn credential(cwd: Option<&Path>) -> AccountCredential {
        AccountCredential {
            run: "demo/t-0001".into(),
            account: "fixture".into(),
            harness: "claude".into(),
            version: 1,
            secret: SecretText("fake-test-credential".into()),
            trust_cwd: cwd.map(|p| p.to_string_lossy().into_owned()),
        }
    }

    #[tokio::test]
    async fn isolated_claude_preserves_preferences_and_replaces_only_trust() {
        let lab = Lab::new();
        let home = lab.0.join("claude-home");
        let cwd = lab.0.join("folder with spaces and 한글");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        let config = home.join(".claude.json");
        std::fs::write(&config, r#"{"hasCompletedOnboarding":true,"theme":"dark","projects":{"/unrelated":{"hasTrustDialogAccepted":true}}}"#).unwrap();
        let (tx, _) = mpsc::channel(1);
        let out = prepare(&home, credential(Some(&cwd)), Path::new("aaa"), tx.clone(), false).await.unwrap();
        assert!(out.env.iter().any(|(k, v)| k == "CLAUDE_CONFIG_DIR" && v == &home.to_string_lossy()));
        let read = || serde_json::from_slice::<serde_json::Value>(&std::fs::read(&config).unwrap()).unwrap();
        let v = read();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["hasCompletedOnboarding"], true);
        let projects = v["projects"].as_object().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(
            projects[&std::fs::canonicalize(&cwd).unwrap().to_string_lossy().into_owned()]["hasTrustDialogAccepted"],
            true
        );
        prepare(&home, credential(None), Path::new("aaa"), tx, true).await.unwrap();
        assert!(read()["projects"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn legacy_claude_resume_keeps_its_original_home() {
        let lab = Lab::new();
        let home = lab.0.join("absent-claude-home");
        let (tx, _) = mpsc::channel(1);
        let out = prepare(&home, credential(None), Path::new("aaa"), tx, true).await.unwrap();
        assert_eq!(out.env.len(), 1);
        assert_eq!(out.env[0].0, "CLAUDE_CODE_OAUTH_TOKEN");
        assert!(!home.exists());
    }
}
