//! Codex app-server를 격리 홈에서 열고 host 금고의 access token으로 한 turn만 실행한다.
use super::{
    INSTRUCTIONS, Runner, command, controlled, provider_error, result, schema, stop_child, workspace::Workspace,
};
use aaa_common::{child_process::TokioCommandExt as _, util};
use aaa_proto::{ReviewAuth, ReviewResult, ReviewRun};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

const PROVIDER: &str = "aaa-review";
const CONFIG: &str = r#"
model_provider = "aaa-review"
cli_auth_credentials_store = "file"
forced_login_method = "chatgpt"
web_search = "disabled"
project_doc_max_bytes = 0
[model_providers.aaa-review]
name = "OpenAI"
wire_api = "responses"
requires_openai_auth = true
supports_websockets = true
request_max_retries = 0
stream_max_retries = 0
[analytics]
enabled = false
[skills.bundled]
enabled = false
[features]
shell_tool = false
unified_exec = false
hooks = false
codex_hooks = false
plugin_hooks = false
plugins = false
apps = false
image_generation = false
multi_agent = false
multi_agent_v2 = false
multi_agent_v2_dynamic_tools = false
collab = false
enable_fanout = false
js_repl = false
code_mode = false
browser_use = false
computer_use = false
view_image = false
memories = false
tool_suggest = false
daemon_auto_start = false
remote_control = false
recommended_plugins = false
"#;

pub(super) async fn run(
    runner: &mut Runner,
    ws: &Workspace,
    exe: &Path,
    request: &ReviewRun,
    input: &str,
) -> Result<ReviewResult> {
    let ReviewAuth::Codex { access_token, account_id, plan } = &request.auth else {
        bail!("Codex 검수 인증 형식 오류");
    };
    util::write_private(&ws.home.join("config.toml"), CONFIG.as_bytes()).context("Codex 검수 설정 쓰기")?;
    let mut child = command(exe, &ws.home, &ws.home)?
        .env("CODEX_HOME", &ws.home)
        .args(["app-server", "--stdio", "--strict-config"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn_guarded()
        .context("Codex 검수 실행기 시작 실패")?;
    let mut rpc = Rpc {
        input: child.stdin.take().context("Codex 검수 입력 pipe 없음")?,
        output: BufReader::new(child.stdout.take().context("Codex 검수 출력 pipe 없음")?),
    };
    let mut used = false;
    let mut cancel = runner.cancel.clone();
    let exchange = async {
        rpc.request(
            1,
            "initialize",
            json!({
                "clientInfo":{"name":"aaa-review","version":env!("CARGO_PKG_VERSION")},
                "capabilities":{"experimentalApi":true},
            }),
        )
        .await?;
        rpc.send(json!({"method":"initialized","params":{}})).await?;
        let login = rpc.request(2, "account/login/start", json!({
            "type":"chatgptAuthTokens","accessToken":access_token.0,"chatgptAccountId":account_id.0,"chatgptPlanType":plan,
        })).await?;
        ensure!(login["type"] == "chatgptAuthTokens", "Codex 검수에서 host 계정 인증을 확인하지 못했다");
        let thread = rpc
            .request(
                3,
                "thread/start",
                json!({
                    "model": request.model, "cwd": ws.tree, "ephemeral": true,
                    "approvalPolicy": "never", "sandbox": "read-only", "baseInstructions": INSTRUCTIONS,
                }),
            )
            .await?;
        ensure!(
            thread["model"] == request.model && thread["modelProvider"] == PROVIDER,
            "Codex가 지정한 검수 모델·제공자를 유지하지 않았다"
        );
        let model = thread["model"].as_str().context("Codex 검수 모델 응답 없음")?.to_string();
        let thread = thread.pointer("/thread/id").and_then(Value::as_str).context("Codex 검수 thread 응답 없음")?;
        used = true;
        let turn = rpc
            .request(
                4,
                "turn/start",
                json!({
                    "threadId": thread, "model": model,
                    "effort": (!request.effort.is_empty()).then_some(&request.effort),
                    "outputSchema": schema(), "input": [{"type":"text","text":input}],
                }),
            )
            .await?;
        let turn = turn.pointer("/turn/id").and_then(Value::as_str).context("Codex 검수 turn 응답 없음")?;
        rpc.finish(thread, turn, model).await
    };
    let answer = controlled(&mut cancel, runner.remaining(), exchange).await;
    runner.used = used;
    drop(rpc);
    stop_child(&mut child).await?;
    answer
}

struct Rpc {
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Rpc {
    async fn send(&mut self, msg: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&msg)?;
        bytes.push(b'\n');
        self.input.write_all(&bytes).await.context("Codex 검수 RPC 전송 실패")
    }

    async fn next(&mut self) -> Result<Value> {
        let mut line = Vec::new();
        loop {
            let bytes = self.output.fill_buf().await.context("Codex 검수 RPC 수신 실패")?;
            ensure!(!bytes.is_empty(), "Codex 검수 실행기와 연결이 끝났다");
            let count = bytes.iter().position(|b| *b == b'\n').map_or(bytes.len(), |i| i + 1);
            // app-server의 userMessage 사건이 escape된 원본 patch를 다시 싣기도 한다.
            ensure!(line.len() + count <= aaa_proto::MAX_FRAME, "Codex 검수 RPC 출력 상한을 넘었다");
            line.extend_from_slice(&bytes[..count]);
            self.output.consume(count);
            if line.ends_with(b"\n") {
                return serde_json::from_slice(&line).context("Codex 검수 RPC JSON 해독");
            }
        }
    }

    fn check(v: &Value) -> Result<()> {
        match v["method"].as_str() {
            Some("account/chatgptAuthTokens/refresh") => bail!("Codex 검수의 지정 계정 인증 만료·거부"),
            Some("model/rerouted") => bail!("Codex가 지정한 검수 모델을 변경했다"),
            Some("error") if v["params"]["willRetry"] != true => {
                bail!("Codex 검수 실패 · {}", provider_error(&v["params"]["error"]["message"].to_string()));
            }
            Some(_) if v["id"].is_number() || v["id"].is_string() => bail!("Codex 검수에 도구 실행·승인 요청이 왔다"),
            _ => Ok(()),
        }
    }

    async fn request(&mut self, id: u64, method: &str, params: Value) -> Result<Value> {
        self.send(json!({"id":id,"method":method,"params":params})).await?;
        loop {
            let mut msg = self.next().await?;
            Self::check(&msg)?;
            if msg["id"] == id {
                ensure!(
                    msg["error"].is_null(),
                    "Codex 검수 {method} 실패 · {}",
                    provider_error(&msg["error"]["message"].to_string())
                );
                return Ok(msg["result"].take());
            }
        }
    }

    async fn finish(&mut self, thread: &str, turn: &str, model: String) -> Result<ReviewResult> {
        let mut output = None;
        loop {
            let msg = self.next().await?;
            Self::check(&msg)?;
            let params = &msg["params"];
            if params["threadId"].as_str() != Some(thread) {
                continue;
            }
            match msg["method"].as_str() {
                Some("item/completed") if params["turnId"].as_str() == Some(turn) => {
                    let item = &params["item"];
                    if item["type"] == "agentMessage" && item["phase"] != "commentary" {
                        output = item["text"].as_str().map(str::to_owned);
                    }
                }
                Some("turn/completed") if params["turn"]["id"].as_str() == Some(turn) => {
                    ensure!(
                        params["turn"]["status"] == "completed",
                        "Codex 검수 완료 실패 · {}",
                        provider_error(&params["turn"]["error"]["message"].to_string())
                    );
                    let payload = output.context("Codex 검수 결과가 없다")?;
                    return result(model, serde_json::from_str(&payload).context("Codex finding JSON 해독")?);
                }
                _ => {}
            }
        }
    }
}
