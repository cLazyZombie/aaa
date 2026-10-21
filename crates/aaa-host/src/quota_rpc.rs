//! 별도 Codex helper: initialize → initialized → account/rateLimits/read만 보낸다.
//! 인증 갱신은 host의 기존 중계 소유다. helper의 갱신 URL은 자기 거부 서버에만 연결된다.
use aaa_common::{quota, util};
use aaa_proto::{QuotaProblem, QuotaSnapshot, QuotaSource};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const MAX: usize = 1 << 20;
const DEADLINE: Duration = Duration::from_secs(15);
struct Home(PathBuf);
impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Refuse(tokio::task::JoinHandle<()>);
impl Drop for Refuse {
    fn drop(&mut self) {
        self.0.abort();
    }
}

fn refuse_refresh(listener: tokio::net::TcpListener) -> Refuse {
    Refuse(tokio::spawn(async move {
        // 본문을 읽기 전에 TCP를 닫으면 갱신 POST가 401 대신 reset/broken pipe를 받는다.
        // 표준 HTTP 처리를 쓰고 본문은 크기를 제한해 읽은 뒤 저장·기록 없이 버린다.
        let app = axum::Router::new()
            .fallback(|_: axum::body::Bytes| async { (axum::http::StatusCode::UNAUTHORIZED, "{}") })
            .layer(axum::extract::DefaultBodyLimit::max(MAX));
        let _ = axum::serve(listener, app).await;
    }))
}

struct ProbeChild(tokio::process::Child);
impl std::ops::Deref for ProbeChild {
    type Target = tokio::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ProbeChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for ProbeChild {
    fn drop(&mut self) {
        kill_group(&self.0);
    }
}
fn kill_group(child: &tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // spawn한 그 PID를 leader로 만든 전용 group뿐이다. wait 전이므로 PID는 재사용되지 않는다.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(not(unix))]
    let _ = child;
}
async fn terminate(child: &mut ProbeChild) {
    kill_group(child);
    let _ = child.kill().await;
    let _ = child.wait().await;
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
    #[cfg(unix)]
    c.process_group(0);
    c
}

async fn line<R: AsyncBufRead + Unpin>(r: &mut R) -> Result<Value, ()> {
    let mut out = vec![];
    loop {
        let b = r.fill_buf().await.map_err(|_| ())?;
        if b.is_empty() {
            return Err(());
        }
        let n = b.iter().position(|b| *b == b'\n').map(|n| n + 1).unwrap_or(b.len());
        if out.len() + n > MAX {
            return Err(());
        }
        let end = b[n - 1] == b'\n';
        out.extend_from_slice(&b[..n]);
        r.consume(n);
        if end {
            return serde_json::from_slice(&out).map_err(|_| ());
        }
    }
}

async fn response<R: AsyncBufRead + Unpin>(r: &mut R, id: u32) -> Result<Value, ()> {
    for _ in 0..64 {
        let v = line(r).await?;
        if v.get("id").and_then(Value::as_u64) == Some(id as u64) {
            return Ok(v);
        }
        // server가 새 인증·도구 실행을 요구해도 응답하지 않는다. 알림은 원문을 버린다.
        if v.get("id").is_some() {
            return Err(());
        }
    }
    Err(())
}

fn error(v: &Value, now: u64) -> (QuotaProblem, Option<u64>) {
    let e = &v["error"];
    let status = e.pointer("/data/status").and_then(Value::as_u64).or_else(|| e.get("code").and_then(Value::as_u64));
    let msg = e.get("message").and_then(Value::as_str).unwrap_or("");
    let has = |code| status == Some(code) || msg.split(|c: char| !c.is_ascii_digit()).any(|s| s == code.to_string());
    if has(401) {
        (QuotaProblem::Expired, None)
    } else if has(429) {
        let wait = e
            .pointer("/data/retryAfterSeconds")
            .or_else(|| e.pointer("/data/retry_after"))
            .and_then(Value::as_u64)
            .unwrap_or(60);
        (QuotaProblem::RateLimited, Some(now.saturating_add(wait.saturating_mul(1000))))
    } else {
        (QuotaProblem::Failed, None)
    }
}

pub(crate) async fn read(state: &Path, generation: String, auth: &str, now: u64) -> QuotaSnapshot {
    // 기존 실험실의 실제 CLI·인증 차단 경계를 이어 쓴다. 명시한 가짜 RPC 실행 파일만 시험에서 허용한다.
    let fake = if cfg!(debug_assertions) { std::env::var_os("AAA_TEST_QUOTA_CODEX") } else { None };
    if std::env::var_os("AAA_USAGE_HOME").is_some() && fake.is_none() {
        return QuotaSnapshot {
            generation,
            source: QuotaSource::CodexRpc,
            sampled_at_ms: now,
            windows: vec![],
            problem: Some(QuotaProblem::Unsupported),
            retry_at_ms: None,
        };
    }
    read_with(fake.as_deref().map(Path::new).unwrap_or(Path::new("codex")), state, generation, auth, now).await
}

async fn read_with(exe: &Path, state: &Path, generation: String, auth: &str, now: u64) -> QuotaSnapshot {
    aaa_common::redact::remember_auth("codex", auth);
    let mut snapshot = QuotaSnapshot {
        generation,
        source: QuotaSource::CodexRpc,
        sampled_at_ms: now,
        windows: vec![],
        problem: Some(QuotaProblem::Failed),
        retry_at_ms: None,
    };
    let parent = state.join("quota-probes");
    if parent.symlink_metadata().is_ok_and(|m| m.file_type().is_symlink()) || util::ensure_private_dir(&parent).is_err()
    {
        return snapshot;
    }
    let home = Home(parent.join(util::new_id()));
    if util::ensure_private_dir(&home.0).is_err() {
        return snapshot;
    }
    // 버전 조회도 같은 격리 홈에서 실행한다. 지원하지 않는 설치판에는 인증을 주지 않는다.
    let version = command(exe, &home.0).arg("--version").stdin(Stdio::null()).stdout(Stdio::piped()).spawn();
    let Ok(version) = version else {
        snapshot.problem = Some(QuotaProblem::Unsupported);
        return snapshot;
    };
    let mut version = ProbeChild(version);
    let check = async {
        let mut bytes = vec![];
        version.stdout.take().ok_or(())?.take(129).read_to_end(&mut bytes).await.map_err(|_| ())?;
        let status = version.wait().await.map_err(|_| ())?;
        (status.success() && bytes == b"codex-cli 0.163.0\n").then_some(()).ok_or(())
    };
    let supported = matches!(tokio::time::timeout(Duration::from_secs(5), check).await, Ok(Ok(())));
    terminate(&mut version).await;
    if !supported {
        snapshot.problem = Some(QuotaProblem::Unsupported);
        return snapshot;
    }
    if util::write_private(&home.0.join("auth.json"), auth.as_bytes()).is_err()
        || util::write_private(
            &home.0.join("config.toml"),
            b"cli_auth_credentials_store = \"file\"\n[analytics]\nenabled = false\n",
        )
        .is_err()
    {
        return snapshot;
    }
    let Ok(listener) = tokio::net::TcpListener::bind("127.0.0.1:0").await else { return snapshot };
    let Ok(addr) = listener.local_addr() else { return snapshot };
    let _refuse = refuse_refresh(listener);
    let child = command(exe, &home.0)
        .arg("app-server")
        .env("CODEX_REFRESH_TOKEN_URL_OVERRIDE", format!("http://{addr}/refuse"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn();
    let Ok(child) = child else { return snapshot };
    let mut child = ProbeChild(child);
    let exchange = async {
        let mut tx = child.stdin.take().ok_or(())?;
        let mut rx = BufReader::new(child.stdout.take().ok_or(())?);
        tx.write_all(format!("{}\n", json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"aaa-quota","title":"aaa","version":env!("CARGO_PKG_VERSION")}}})).as_bytes()).await.map_err(|_| ())?;
        let init = response(&mut rx, 1).await?;
        if init.get("error").is_some() {
            return Ok(init);
        }
        tx.write_all(
            b"{\"method\":\"initialized\",\"params\":{}}\n{\"id\":2,\"method\":\"account/rateLimits/read\"}\n",
        )
        .await
        .map_err(|_| ())?;
        response(&mut rx, 2).await
    };
    let result = tokio::time::timeout(DEADLINE, exchange).await;
    // 자기 자식 PID만 종료하고 wait가 끝난 뒤 인증 홈을 지운다.
    terminate(&mut child).await;
    if let Ok(Ok(v)) = result {
        if v.get("error").is_some() {
            let (problem, retry) = error(&v, now);
            snapshot.problem = Some(problem);
            snapshot.retry_at_ms = retry;
        } else if let Some(v) = v.get("result") {
            snapshot.windows = quota::codex(v, now);
            snapshot.windows.retain(|w| {
                aaa_common::redact::text(&w.limit_id) == w.limit_id
                    && w.scope.as_ref().is_none_or(|s| aaa_common::redact::text(s) == *s)
            });
            snapshot.problem = if snapshot.windows.is_empty() {
                // 창을 응답했는데 정규화할 수 없으면 미관측 첫 실행의 근거로 쓰지 않는다.
                let has_value = |bucket: &Value| {
                    ["primary", "secondary"]
                        .iter()
                        .any(|key| bucket.get(*key).is_some_and(|w| w.get("usedPercent").is_some()))
                };
                let malformed =
                    v.get("rateLimitsByLimitId").and_then(Value::as_object).is_some_and(|m| m.values().any(has_value))
                        || v.get("rateLimits").is_some_and(has_value);
                Some(if malformed { QuotaProblem::Failed } else { QuotaProblem::Unknown })
            } else {
                None
            };
        }
    }
    snapshot
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn refresh_refusal_reads_the_post_body_before_closing() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let _refuse = refuse_refresh(listener);
        let exchange = async {
            let mut client = BufReader::new(tokio::net::TcpStream::connect(addr).await.unwrap());
            client
                .get_mut()
                .write_all(b"POST /refuse HTTP/1.1\r\nHost: localhost\r\nContent-Length: 4\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            // 본문을 보낼 수 있다는 HTTP 사건을 기다린다. 원래 서버는 여기서 바로 401을 보낸다.
            let mut status = String::new();
            client.read_line(&mut status).await.unwrap();
            assert_eq!(status, "HTTP/1.1 100 Continue\r\n");
            let mut end = String::new();
            client.read_line(&mut end).await.unwrap();
            assert_eq!(end, "\r\n");
            client.get_mut().write_all(b"fake").await.unwrap();
            let mut response = String::new();
            client.read_to_string(&mut response).await.unwrap();
            assert!(response.starts_with("HTTP/1.1 401 Unauthorized\r\n"), "{response}");
            assert!(response.ends_with("\r\n\r\n{}"), "{response}");
        };
        tokio::time::timeout(Duration::from_secs(5), exchange).await.expect("가짜 갱신 POST 완료");
    }

    fn fake(root: &Path, mode: &str) -> PathBuf {
        let exe = root.join(format!("codex-{mode}"));
        let log = root.join(format!("{mode}.methods"));
        let script = format!(
            r##"#!/usr/bin/env python3
import os,sys,json,hashlib,urllib.request,urllib.error
if sys.argv[1:] == ['--version']:
    print('codex-cli 0.163.0'); sys.exit(0)
assert sys.argv[1:] == ['app-server']
home=os.environ['CODEX_HOME']
assert 'OPENAI_API_KEY' not in os.environ and 'CLAUDE_CODE_OAUTH_TOKEN' not in os.environ
auth=json.load(open(home+'/auth.json'))
assert auth['tokens']['access_token'] == 'fake-access-canary-0123456789'
mode={mode:?}
log={log:?}
def failure(kind,error,tb):
    with open(log+'.failure','w') as f:
        f.write(kind.__name__+' errno='+str(getattr(error,'errno',None)))
sys.excepthook=failure
def send(v): print(json.dumps(v),flush=True)
for line in sys.stdin:
    v=json.loads(line)
    with open(log,'a') as f: f.write(v['method']+'\n')
    if v['method']=='initialize':
        send({{'method':'opaque/notification','params':{{'token':auth['tokens']['access_token']}}}})
        send({{'id':v['id'],'result':{{}}}})
    elif v['method']=='account/rateLimits/read':
        sys.stderr.write(auth['tokens']['access_token']);sys.stderr.flush()
        if mode=='refresh':
            try: urllib.request.urlopen(urllib.request.Request(os.environ['CODEX_REFRESH_TOKEN_URL_OVERRIDE'],data=b'fake-refresh-canary-9876543210'))
            except urllib.error.HTTPError as e: assert e.code==401
        if mode in ['401','429']:
            send({{'id':v['id'],'error':{{'code':-32603,'message':auth['tokens']['access_token'],'data':{{'status':int(mode),'retryAfterSeconds':123}}}}}})
        elif mode=='broken': print('bad-json',flush=True)
        elif mode=='missing': send({{'id':v['id'],'result':{{'rateLimits':{{'primary':None}}}}}})
        elif mode=='invalid': send({{'id':v['id'],'result':{{'rateLimits':{{'primary':{{'usedPercent':None}}}}}}}})
        else:
            buckets={{'codex':{{'primary':{{'usedPercent':25,'windowDurationMins':300,'resetsAt':9999}},'secondary':{{'usedPercent':50,'windowDurationMins':10080,'resetsAt':19999}}}}}}
            buckets[auth['tokens']['access_token']]={{'primary':{{'usedPercent':99,'windowDurationMins':300,'resetsAt':9999}}}}
            send({{'id':v['id'],'result':{{'token':auth['tokens']['access_token'],'rateLimitsByLimitId':buckets}}}})
    else: assert v['method']=='initialized'
"##,
            log = log.to_string_lossy()
        );
        std::fs::write(&exe, script).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        exe
    }
    #[tokio::test]
    async fn fake_rpc_reads_without_threads_turns_or_raw_credentials_and_cleans_each_home() {
        let root = Home(std::env::temp_dir().join(format!("aaa-quota-{}", util::new_id())));
        util::ensure_private_dir(&root.0).unwrap();
        let auth = r#"{"tokens":{"access_token":"fake-access-canary-0123456789","refresh_token":"fake-refresh-canary-9876543210"}}"#;
        let futures = ["ok", "refresh", "401", "429", "broken", "missing", "invalid"].map(|mode| {
            let exe = fake(&root.0, mode);
            let state = root.0.clone();
            async move { (mode, read_with(&exe, &state, format!("generation-{mode}"), auth, 1000).await) }
        });
        for (mode, snapshot) in futures_util::future::join_all(futures).await {
            let failure = std::fs::read_to_string(root.0.join(format!("{mode}.methods.failure"))).unwrap_or_default();
            let diagnostic = format!("mode={mode} snapshot={snapshot:?} fake_failure={failure}");
            let methods = std::fs::read_to_string(root.0.join(format!("{mode}.methods")))
                .unwrap_or_else(|e| panic!("{diagnostic}: RPC 기록 읽기: {e}"));
            assert_eq!(methods, "initialize\ninitialized\naccount/rateLimits/read\n", "{diagnostic}: 무작업 RPC 기록");
            assert_eq!(snapshot.generation, format!("generation-{mode}"), "{diagnostic}");
            match mode {
                "ok" | "refresh" => {
                    assert_eq!(snapshot.problem, None, "{diagnostic}");
                    assert_eq!(snapshot.windows.len(), 2, "{diagnostic}");
                }
                "401" => assert_eq!(snapshot.problem, Some(QuotaProblem::Expired), "{diagnostic}"),
                "429" => {
                    assert_eq!(snapshot.problem, Some(QuotaProblem::RateLimited), "{diagnostic}");
                    assert_eq!(snapshot.retry_at_ms, Some(124000), "{diagnostic}");
                }
                "missing" => assert_eq!(snapshot.problem, Some(QuotaProblem::Unknown), "{diagnostic}"),
                _ => assert_eq!(snapshot.problem, Some(QuotaProblem::Failed), "{diagnostic}"),
            }
            let out = serde_json::to_string(&snapshot).unwrap();
            assert!(!out.contains("canary"), "원문 응답·stderr가 정규화 결과에 없다");
        }
        assert_eq!(std::fs::read_dir(root.0.join("quota-probes")).unwrap().count(), 0, "종료 뒤 인증·log 홈 정리");
    }
}
