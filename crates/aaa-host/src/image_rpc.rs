//! Codex 0.163.0 내장 이미지 생성. 환경·홈·작업 폴더를 격리하고 stdout 원문은 버린다.
use crate::{
    Host,
    image::{Job, descriptor, image_extension},
};
use aaa_common::util;
use aaa_proto::{IMAGE_MAX_BYTES, IMAGE_MAX_COUNT, ImageFile, SecretText};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const FAILED: &str = "host Codex 이미지 생성 실패 · 같은 작업은 자동으로 다시 생성하지 않는다";
const UNSUPPORTED: &str = "이미지 생성은 host의 Codex 0.163.0 설치가 필요하다";
const MAX_LINE: usize = 72 * 1024 * 1024;

pub(crate) struct RunFailure {
    pub reason: &'static str,
    pub uncertain: bool,
    pub sent: bool,
}
impl From<&'static str> for RunFailure {
    fn from(reason: &'static str) -> Self {
        Self { reason, uncertain: false, sent: false }
    }
}

struct Home(PathBuf);
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Proxy(tokio::task::JoinHandle<()>);
impl Drop for Proxy {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn command(exe: &Path, home: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(exe);
    c.env_clear();
    for k in ["PATH", "SystemRoot", "WINDIR", "TMPDIR", "TMP", "TEMP"] {
        if let Some(v) = std::env::var_os(k) {
            c.env(k, v);
        }
    }
    c.env("CODEX_HOME", home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home)
        .env("XDG_DATA_HOME", home)
        .env("XDG_STATE_HOME", home)
        .env("XDG_CACHE_HOME", home)
        .current_dir(home)
        .stderr(Stdio::null())
        .kill_on_drop(true);
    c
}
async fn line<R: AsyncBufRead + Unpin>(r: &mut R) -> Result<Value, &'static str> {
    let mut out = Vec::new();
    loop {
        let b = r.fill_buf().await.map_err(|_| FAILED)?;
        if b.is_empty() {
            return Err(FAILED);
        }
        let n = b.iter().position(|b| *b == b'\n').map(|n| n + 1).unwrap_or(b.len());
        if out.len() + n > MAX_LINE {
            return Err(FAILED);
        }
        let end = b[n - 1] == b'\n';
        out.extend_from_slice(&b[..n]);
        r.consume(n);
        if end {
            return serde_json::from_slice(&out).map_err(|_| FAILED);
        }
    }
}
async fn write(tx: &mut tokio::process::ChildStdin, v: Value) -> Result<(), &'static str> {
    let mut bytes = serde_json::to_vec(&v).map_err(|_| FAILED)?;
    bytes.push(b'\n');
    tx.write_all(&bytes).await.map_err(|_| FAILED)
}
async fn response<R: AsyncBufRead + Unpin>(rx: &mut R, id: u32) -> Result<Value, &'static str> {
    for _ in 0..128 {
        let v = line(rx).await?;
        if v.get("id").and_then(Value::as_u64) == Some(id as u64) && v.get("method").is_none() {
            if v.get("error").is_some() {
                return Err(FAILED);
            }
            return Ok(v["result"].clone());
        }
        if v.get("id").is_some() {
            return Err(FAILED);
        } // 승인·동적 도구·로그인 요청은 실행하지 않는다.
    }
    Err(FAILED)
}

async fn proxy(h: &Arc<Host>, job: &Job, version: u64) -> Result<(Proxy, String), &'static str> {
    use axum::{Router, http::StatusCode, routing::post};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|_| FAILED)?;
    let addr = listener.local_addr().map_err(|_| FAILED)?;
    let cap = util::new_id();
    let route = format!("/{cap}");
    let url = format!("http://{addr}{route}");
    let account = job.view.account.clone().ok_or(FAILED)?;
    let generation = job.allocation.as_ref().ok_or(FAILED)?.generation.clone();
    let h = h.clone();
    let version = Arc::new(tokio::sync::Mutex::new(version));
    let app = Router::new()
        .route(
            &route,
            post(move |body: axum::body::Bytes| {
                let h = h.clone();
                let account = account.clone();
                let generation = generation.clone();
                let version = version.clone();
                async move {
                    let mut version = version.lock().await;
                    if h.accounts.generation(&account).ok().as_deref() != Some(&generation) {
                        return (StatusCode::UNAUTHORIZED, "{}".to_string());
                    }
                    let Ok(body) = String::from_utf8(body.to_vec()) else {
                        return (StatusCode::BAD_REQUEST, "{}".into());
                    };
                    let (result, outcome) = h.accounts.refresh(&account, *version, &SecretText(body)).await;
                    // 중앙 gate 대기 중 다시 로그인했으면 Served가 새 신원을 담을 수 있다.
                    // 옛 이미지 작업에는 그 응답을 전달하지 않는다.
                    if h.accounts.generation(&account).ok().as_deref() != Some(&generation) {
                        return (StatusCode::UNAUTHORIZED, "{}".into());
                    }
                    h.account_refresh_seen(&account, outcome, *version);
                    if let Some(response) = result.response {
                        *version = result.version;
                        (StatusCode::OK, response.0)
                    } else {
                        (StatusCode::UNAUTHORIZED, "{}".into())
                    }
                }
            }),
        )
        .layer(axum::extract::DefaultBodyLimit::max(128 * 1024));
    Ok((
        Proxy(tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        })),
        url,
    ))
}

// 명시한 모델만 사용한다. API key·로컬 설정·자동 model 전환·망 재시도는 허용하지 않는다.
fn config(model: &str) -> Result<Vec<u8>, &'static str> {
    let mut value = toml::Table::new();
    value.insert("model".into(), model.into());
    value.insert("model_provider".into(), "aaa-image".into());
    value.insert("cli_auth_credentials_store".into(), "file".into());
    value.insert("forced_login_method".into(), "chatgpt".into());
    value.insert("web_search".into(), "disabled".into());
    value.insert("project_doc_max_bytes".into(), 0.into());
    let mut features = toml::Table::new();
    for name in [
        "shell_tool",
        "unified_exec",
        "multi_agent",
        "multi_agent_v2",
        "multi_agent_v2_dynamic_tools",
        "collab",
        "enable_fanout",
        "apps",
        "plugins",
        "hooks",
        "codex_hooks",
        "plugin_hooks",
        "js_repl",
        "code_mode",
        "browser_use",
        "computer_use",
        "view_image",
        "memories",
        "tool_suggest",
        "daemon_auto_start",
        "remote_control",
        "recommended_plugins",
    ] {
        features.insert(name.into(), false.into());
    }
    features.insert("image_generation".into(), true.into());
    value.insert("features".into(), features.into());
    value.insert("analytics".into(), toml::Table::from_iter([("enabled".into(), false.into())]).into());
    value.insert(
        "skills".into(),
        toml::Table::from_iter([("bundled".into(), toml::Table::from_iter([("enabled".into(), false.into())]).into())])
            .into(),
    );
    // 제공자 기본 주소는 CLI 정본을 쓴다. retry만 0으로 제한한다.
    value.insert(
        "model_providers".into(),
        toml::Table::from_iter([(
            "aaa-image".into(),
            toml::Table::from_iter([
                ("name".into(), "OpenAI".into()),
                ("wire_api".into(), "responses".into()),
                ("requires_openai_auth".into(), true.into()),
                ("supports_websockets".into(), true.into()),
                ("request_max_retries".into(), 0.into()),
                ("stream_max_retries".into(), 0.into()),
            ])
            .into(),
        )])
        .into(),
    );
    toml::to_string(&value).map(String::into_bytes).map_err(|_| FAILED)
}

pub(crate) async fn generate(h: &Arc<Host>, job: &Job) -> Result<(Vec<ImageFile>, Option<u64>), RunFailure> {
    let fake = if cfg!(debug_assertions) { std::env::var_os("AAA_TEST_IMAGE_CODEX") } else { None };
    if std::env::var_os("AAA_USAGE_HOME").is_some() && fake.is_none() {
        return Err(UNSUPPORTED.into());
    }
    let exe = fake.as_deref().map(Path::new).unwrap_or(Path::new("codex"));
    let dir = h.image_dir(&job.begin.id).map_err(|_| FAILED)?;
    let path = dir.join("rpc-home");
    if path.symlink_metadata().is_ok() {
        return Err(FAILED.into());
    }
    util::ensure_private_dir(&path).map_err(|_| FAILED)?;
    let home = Home(path.canonicalize().map_err(|_| FAILED)?);
    // 인증 전 버전 확인. child PID 하나만 종료하며 process group을 사용하지 않는다.
    let mut version = command(exe, &home.0)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| UNSUPPORTED)?;
    let check = async {
        let mut out = vec![];
        version.stdout.take().ok_or(UNSUPPORTED)?.take(129).read_to_end(&mut out).await.map_err(|_| UNSUPPORTED)?;
        let status = version.wait().await.map_err(|_| UNSUPPORTED)?;
        if !status.success() || out != b"codex-cli 0.163.0\n" {
            return Err(UNSUPPORTED);
        }
        Ok(())
    };
    let supported = tokio::time::timeout(Duration::from_secs(5), check).await;
    let _ = version.kill().await;
    let _ = version.wait().await;
    supported.map_err(|_| UNSUPPORTED)??;
    let account = job.view.account.as_ref().ok_or(FAILED)?;
    let credential = h.accounts.credential(account).map_err(|_| FAILED)?;
    if credential.auth_generation.as_ref() != job.allocation.as_ref().map(|a| &a.generation) {
        return Err(FAILED.into());
    }
    let auth: Value = serde_json::from_str(&credential.secret.0).map_err(|_| FAILED)?;
    if auth["tokens"]["access_token"].as_str().is_none()
        || auth["tokens"]["refresh_token"].as_str().is_none()
        || auth.get("OPENAI_API_KEY").is_some_and(|v| !v.is_null())
    {
        return Err(FAILED.into());
    }
    util::write_private(&home.0.join("auth.json"), credential.secret.0.as_bytes()).map_err(|_| FAILED)?;
    util::write_private(&home.0.join("config.toml"), &config(&job.model)?).map_err(|_| FAILED)?;
    let (_proxy, refresh_url) = proxy(h, job, credential.version).await?;
    let mut child = command(exe, &home.0)
        .arg("app-server")
        .arg("--stdio")
        .arg("--strict-config")
        .env("CODEX_REFRESH_TOKEN_URL_OVERRIDE", refresh_url)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_| FAILED)?;
    let mut sent = false;
    let mut terminal = false;
    let exchange = async {
        let mut tx = child.stdin.take().ok_or(FAILED)?;
        let mut rx = BufReader::new(child.stdout.take().ok_or(FAILED)?);
        write(&mut tx, json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"aaa-image","version":env!("CARGO_PKG_VERSION")}}})).await?;
        response(&mut rx, 1).await?;
        write(&mut tx, json!({"method":"initialized","params":{}})).await?;
        write(&mut tx, json!({"id":2,"method":"thread/start","params":{"model":job.model,"cwd":home.0,
            "ephemeral":true,"approvalPolicy":"never","sandbox":"read-only",
            "baseInstructions":"Generate or edit images using only the built-in image generation tool. Never use shell, files, plugins, subagents, or an API key. Generate exactly one image. Do not substitute code or text for an image.",
            "developerInstructions":"Use the supplied image inputs in order. After generating the image, stop. Do not read any local files or change models."}})).await?;
        let start = response(&mut rx, 2).await?;
        if start["model"] != job.model || start["modelProvider"] != "aaa-image" {
            return Err("Codex가 지정한 모델·provider를 유지하지 않았다 · 생성하지 않았다");
        }
        let thread = start.pointer("/thread/id").and_then(Value::as_str).ok_or(FAILED)?.to_string();
        let mut input = vec![json!({"type":"text","text": format!("{}\nBackground requirement: {}.", job.begin.prompt,
            if job.begin.transparent { "transparent with alpha" } else { "opaque" })})];
        for (index, _) in job.begin.inputs.iter().enumerate() {
            let bytes = std::fs::read(dir.join(format!("input-{index}"))).map_err(|_| FAILED)?;
            let mime = match image_extension(&bytes).ok_or(FAILED)? {
                "jpg" => "image/jpeg",
                "webp" => "image/webp",
                _ => "image/png",
            };
            input.push(json!({"type":"image","url":format!("data:{mime};base64,{}",STANDARD.encode(bytes))}));
        }
        sent = true;
        write(
            &mut tx,
            json!({"id":3,"method":"turn/start","params":{"threadId":thread,"model":job.model,
            "effort":(!job.effort.is_empty()).then_some(&job.effort),"input":input}}),
        )
        .await?;
        let mut outputs = vec![];
        let mut seen = HashSet::new();
        let mut tokens = None;
        for _ in 0..10000 {
            let v = line(&mut rx).await?;
            if v.get("id").is_some() {
                if v["id"] == 3 && v.get("method").is_none() {
                    if v.get("error").is_some() {
                        terminal = true;
                        return Err(FAILED);
                    }
                    continue;
                }
                return Err(FAILED);
            }
            let params = &v["params"];
            if params["threadId"].as_str().is_some_and(|id| id != thread) {
                return Err(FAILED);
            }
            match v["method"].as_str() {
                Some("thread/tokenUsage/updated") => {
                    tokens = params.pointer("/tokenUsage/total/totalTokens").and_then(Value::as_u64);
                }
                Some("item/completed") if params["item"]["type"] == "imageGeneration" => {
                    save_output(&params["item"], &home.0, &dir, &mut seen, &mut outputs)?;
                }
                Some("turn/completed") => {
                    terminal = true;
                    if params["turn"]["status"] != "completed" {
                        return Err(FAILED);
                    }
                    if let Some(items) = params["turn"]["items"].as_array() {
                        for item in items.iter().filter(|i| i["type"] == "imageGeneration") {
                            save_output(item, &home.0, &dir, &mut seen, &mut outputs)?;
                        }
                    }
                    if outputs.is_empty() {
                        return Err("Codex가 이미지 결과를 주지 않았다 · API 과금으로 전환하지 않았다");
                    }
                    return Ok((outputs, tokens));
                }
                Some("error") if params["willRetry"] != true => return Err(FAILED),
                _ => {}
            }
        }
        Err(FAILED)
    };
    let result = tokio::time::timeout(Duration::from_secs(600), exchange).await;
    let _ = child.kill().await;
    let _ = child.wait().await;
    result.unwrap_or(Err("이미지 생성 10분 상한 · 결과 불명, 자동 재생성 없음")).map_err(|reason| RunFailure {
        reason,
        uncertain: sent && !terminal,
        sent,
    })
}

fn save_output(
    item: &Value,
    home: &Path,
    dir: &Path,
    seen: &mut HashSet<String>,
    outputs: &mut Vec<ImageFile>,
) -> Result<(), &'static str> {
    let id = item["id"].as_str().ok_or(FAILED)?;
    if seen.contains(id) {
        return Ok(());
    }
    if item["failure"].is_object() {
        return Err("Codex 이미지 생성 한도·제공자 오류 · 계정과 상태를 확인한다");
    }
    if item["status"] != "completed" || outputs.len() >= IMAGE_MAX_COUNT {
        return Err(FAILED);
    }
    let bytes = if let Some(path) = item["savedPath"].as_str() {
        let path = Path::new(path);
        let canonical = path.canonicalize().map_err(|_| FAILED)?;
        if !canonical.starts_with(home.join("generated_images")) || path != canonical {
            return Err(FAILED);
        }
        let m = path.symlink_metadata().map_err(|_| FAILED)?;
        if !m.is_file() || m.len() > IMAGE_MAX_BYTES {
            return Err(FAILED);
        }
        std::fs::read(path).map_err(|_| FAILED)?
    } else {
        let data = item["result"].as_str().ok_or(FAILED)?;
        if data.len() > IMAGE_MAX_BYTES as usize * 4 / 3 + 4 {
            return Err(FAILED);
        }
        STANDARD.decode(data).map_err(|_| FAILED)?
    };
    if bytes.is_empty() || bytes.len() as u64 > IMAGE_MAX_BYTES || image_extension(&bytes).is_none() {
        return Err(FAILED);
    }
    util::write_private(&dir.join(format!("output-{}", outputs.len())), &bytes).map_err(|_| FAILED)?;
    outputs.push(descriptor(&bytes));
    seen.insert(id.into());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn refresh_waiting_on_the_central_gate_cannot_deliver_a_new_login_to_an_old_job() {
        use aaa_proto::{AccountSelection, AccountTaskSize, ImageBegin, ImageState, ImageView};
        let path = std::env::temp_dir().join(format!("aaa-image-generation-{}", util::new_id()));
        util::ensure_private_dir(&path).unwrap();
        let home = Home(path.canonicalize().unwrap());
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = home.0.join("projects").display().to_string();
        let h = Host::open(cfg, home.0.join("host")).unwrap();
        let auth = |label: &str| {
            json!({"tokens":{"access_token":format!("fixture-{label}-access-long"),"refresh_token":format!("fixture-{label}-refresh-long")}}).to_string()
        };
        h.accounts.import("work", "codex", &auth("old")).unwrap();
        let generation = h.accounts.generation("work").unwrap();
        let id = util::new_id();
        let job = Job {
            begin: ImageBegin {
                id: id.clone(),
                prompt: "fixture".into(),
                profile: "image".into(),
                account: None,
                transparent: false,
                inputs: vec![],
            },
            machine: "m".into(),
            session: "s".into(),
            project: "demo".into(),
            owner: "thread:t-0001".into(),
            model: "model-a".into(),
            effort: "high".into(),
            view: ImageView {
                id,
                state: ImageState::Running,
                account: Some("work".into()),
                reason: "fixture".into(),
                outputs: vec![],
                total_tokens: None,
            },
            allocation: Some(crate::account_pool::Allocation {
                generation,
                ended_at_ms: None,
                released: false,
                reserve_percent: Some(75),
                selection: AccountSelection {
                    size: AccountTaskSize::Medium,
                    group: "work".into(),
                    unobserved: false,
                    reason: "fixture".into(),
                    selected_at_ms: util::now_ms(),
                },
            }),
            launched: true,
        };
        let gate = h.accounts.refresh_gate_for_test("work");
        let held = gate.clone().lock_owned().await;
        let baseline = Arc::strong_count(&gate);
        let (_proxy, url) = proxy(&h, &job, 1).await.unwrap();
        let request = tokio::spawn(async move {
            crate::http::request("POST", &url, &[], br#"{"grant_type":"refresh_token"}"#, 128 * 1024).await.unwrap()
        });
        // 중앙 owner가 gate를 복제한 사건은 기존 로그인 세대를 검사한 뒤의 대기다.
        tokio::time::timeout(Duration::from_secs(3), async {
            while Arc::strong_count(&gate) == baseline {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        h.accounts.import("work", "codex", &auth("new")).unwrap();
        drop(held);
        let response = tokio::time::timeout(Duration::from_secs(3), request).await.unwrap().unwrap();
        assert_eq!(response.status, 401, "대기 중 다시 로그인한 인증을 옛 이미지 작업에 주면 안 된다");
        assert_eq!(response.body, b"{}");
    }

    #[tokio::test]
    #[ignore = "설치된 Codex 0.163.0의 무인증·무생성 설정 계약 확인"]
    async fn installed_codex_accepts_isolated_image_config_without_authentication() {
        let path = std::env::temp_dir().join(format!("aaa-image-config-{}", util::new_id()));
        util::ensure_private_dir(&path).unwrap();
        let home = Home(path.canonicalize().unwrap());
        util::write_private(&home.0.join("config.toml"), &config("model-a").unwrap()).unwrap();
        let mut child = command(Path::new("codex"), &home.0)
            .args(["app-server", "--stdio", "--strict-config"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut tx = child.stdin.take().unwrap();
        let mut rx = BufReader::new(child.stdout.take().unwrap());
        let check = async {
            write(
                &mut tx,
                json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"aaa-image-test","version":"0"}}}),
            )
            .await
            .unwrap();
            let value = response(&mut rx, 1).await;
            if value.is_err() {
                let mut error = String::new();
                child.stderr.take().unwrap().read_to_string(&mut error).await.unwrap();
                panic!("무인증 initialize 실패: {error}");
            }
            write(&mut tx, json!({"method":"initialized","params":{}})).await.unwrap();
            write(&mut tx, json!({"id":2,"method":"config/read","params":{}})).await.unwrap();
            let config = response(&mut rx, 2).await.unwrap();
            assert_eq!(config["config"]["model"], "model-a");
            assert_eq!(config["config"]["features"]["shell_tool"], false);
            assert_eq!(config["config"]["features"]["image_generation"], true);
            write(&mut tx, json!({"id":3,"method":"modelProvider/capabilities/read","params":{}})).await.unwrap();
            let capabilities = response(&mut rx, 3).await.unwrap();
            assert_eq!(capabilities["imageGeneration"], true, "공식 OpenAI 내장 이미지 도구 지원");
        };
        let result = tokio::time::timeout(Duration::from_secs(15), check).await;
        let _ = child.kill().await;
        let _ = child.wait().await;
        assert!(result.is_ok(), "Codex 무인증 설정 조회 시한");
        assert!(!home.0.join("auth.json").exists());
    }

    #[test]
    fn output_must_be_a_bounded_image_inside_generated_home() {
        let root = std::env::temp_dir().join(format!("aaa-image-output-{}", util::new_id()));
        util::ensure_private_dir(&root).unwrap();
        let home = Home(root.canonicalize().unwrap());
        let dir = home.0.join("outputs");
        std::fs::create_dir(&dir).unwrap();
        let bytes = b"\x89PNG\r\n\x1a\nfixture";
        let item = json!({"type":"imageGeneration","id":"one","status":"completed","result":STANDARD.encode(bytes)});
        let mut seen = HashSet::new();
        let mut outputs = vec![];
        save_output(&item, &home.0, &dir, &mut seen, &mut outputs).unwrap();
        save_output(&item, &home.0, &dir, &mut seen, &mut outputs).unwrap();
        assert_eq!(outputs, [descriptor(bytes)], "같은 item 사건은 한 번 저장");
        let mut wrong = item.clone();
        wrong["id"] = "two".into();
        wrong["savedPath"] = dir.join("output-0").to_string_lossy().as_ref().into();
        assert!(save_output(&wrong, &home.0, &dir, &mut seen, &mut outputs).is_err(), "generated_images 밖 읽기 거부");
        wrong["savedPath"] = Value::Null;
        wrong["result"] = STANDARD.encode(b"not an image").into();
        assert!(save_output(&wrong, &home.0, &dir, &mut seen, &mut outputs).is_err());
        wrong["failure"] = json!({"type":"usageLimitExceeded"});
        assert!(save_output(&wrong, &home.0, &dir, &mut seen, &mut outputs).is_err());
        #[cfg(unix)]
        {
            let images = home.0.join("generated_images");
            std::fs::create_dir(&images).unwrap();
            std::os::unix::fs::symlink(dir.join("output-0"), images.join("link.png")).unwrap();
            wrong["failure"] = Value::Null;
            wrong["savedPath"] = images.join("link.png").to_string_lossy().as_ref().into();
            assert!(save_output(&wrong, &home.0, &dir, &mut seen, &mut outputs).is_err());
        }
    }
}
