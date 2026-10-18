//! agent → host 연결(Agent link). 연결은 agent가 연다. 로컬은 UDS, 원격은 TCP+TLS 1.3(fingerprint 고정).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{AGENT_LINK, AgentHello, EnrollRequest, LinkMsg, MAX_FRAME, MAX_FRAME_PREAUTH};
use anyhow::{Context, Result, bail};

use super::Agent;
use super::identity::{HostPin, save_pin};
use aaa_common::conn::{FrameRx, Incoming, spawn_writer};
use aaa_common::ipc::LocalStream;
use aaa_common::tls;

pub enum Target {
    Local(PathBuf),
    Remote { addr: String, pin: String, id: tls::Identity },
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Local(p) => write!(f, "local {}", p.display()),
            Target::Remote { addr, .. } => write!(f, "{addr}"),
        }
    }
}

use aaa_proto::{LINK_DEAD_AFTER as DEAD_AFTER, LINK_PING_EVERY as PING_EVERY};

pub async fn connect(t: &Target) -> Result<LocalStream> {
    match t {
        Target::Local(p) => {
            Ok(aaa_common::ipc::connect(p).await.with_context(|| format!("host socket {}", p.display()))?)
        }
        Target::Remote { addr, pin, id } => {
            let tcp = tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(addr))
                .await
                .context("연결 시간 초과")??;
            tcp.set_nodelay(true)?;
            let connector = tokio_rustls::TlsConnector::from(tls::client_config(id, Some(pin.clone()))?);
            let s = tokio::time::timeout(Duration::from_secs(10), connector.connect(tls::server_name(), tcp))
                .await
                .context("TLS 시간 초과")??;
            Ok(Box::new(s))
        }
    }
}

/// 끊기면 지수 backoff + jitter로 다시 연결한다. 끝나지 않는다.
pub async fn run(agent: Arc<Agent>, target: Target, max_backoff: Duration) {
    let mut backoff = Duration::from_millis(250);
    loop {
        let started = Instant::now();
        match connect(&target).await {
            Ok(stream) => match session(&agent, stream).await {
                Ok(()) => tracing::info!("host 연결이 닫혔다({target})"),
                Err(e) => tracing::warn!("host 연결 끝: {e:#}"),
            },
            Err(e) => tracing::debug!("host 연결 실패({target}): {e:#}"),
        }
        agent.set_host(None, false);
        if started.elapsed() > Duration::from_secs(10) {
            backoff = Duration::from_millis(250);
        }
        let jitter = Duration::from_millis(rand::random_range(0..=(backoff.as_millis() as u64 / 2).max(1)));
        tokio::time::sleep(backoff + jitter).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

pub fn hello(agent: &Agent) -> AgentHello {
    AgentHello {
        proto: AGENT_LINK,
        machine_id: agent.machine_id.clone(),
        machine_name: agent.name.clone(),
        boot_id: agent.boot_id.clone(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        software: aaa_common::util::software(),
        // cwd.v1: `AgentRequest::Cwd`(옛 agent는 모르는 요청에 연결을 끊으므로 host는 이 표시를 보고 보낸다), rename.v1: `AgentRequest::Rename`,
        // history.v1: `AgentRequest::History`(plan §8.118)
        // file.v1: 파일 보기 `FileLocate`·`FileView`(plan §8.125), profile.v1: `SpawnSpec.profile`(plan §8.126)
        // ask_anew.v1: 막힘에 입력이 간 뒤 화면에 남은 확인창을 같은 글이어도 다시 알린다(`detect::Watch::input_sent`).
        // host는 그 질문을 답한 질문의 늦은 사건으로 거르지 않는다
        // dir.v1: 폴더 보기 `DirList`(plan §8.148)
        // profiles.v2: 기본 profile(claude·codex)을 이름으로 받고 profile을 쓸 때마다 agent.toml을 다시 읽는다(plan §8.163),
        // launch_safety.v1: `ThreadPrepare.safety`·`ThreadRebrief.safety`(yolo·thread_agent_args)를 profile 인자에 더한다
        // scrollback.v1: 기록을 편집기로 `ScrollbackWrite`·`ScrollbackRemove`(plan §8.167)
        features: vec![
            "terminal.v1".into(),
            "cwd.v1".into(),
            "pr.v1".into(),
            "pr.v2".into(),
            "rename.v1".into(),
            "history.v1".into(),
            "file.v1".into(),
            "profile.v1".into(),
            "ask_anew.v1".into(),
            "dir.v1".into(),
            "profiles.v2".into(),
            "launch_safety.v1".into(),
            "scrollback.v1".into(),
        ],
        dev_mode: agent.dev_mode,
        selfcheck: agent.selfcheck.lock().unwrap().clone(),
        sessions: agent.sessions(),
        harness: agent.harness_events(),
    }
}

async fn session(agent: &Arc<Agent>, stream: LocalStream) -> Result<()> {
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    tx.json(&LinkMsg::Hello(hello(agent))).await?;
    let first: LinkMsg =
        tokio::time::timeout(Duration::from_secs(10), rx.control()).await.context("welcome 대기")??;
    let (health, replay) = match first {
        LinkMsg::Welcome(w) => {
            tracing::info!(proto = %w.proto, host = %w.host_id, "host에 연결했다");
            // harness_hello.v1: host가 hello의 harness 사건을 읽었다. 옛 host에는 연결 뒤 다시 보낸다
            (w.features.iter().any(|f| f == "health.v1"), !w.features.iter().any(|f| f == "harness_hello.v1"))
        }
        LinkMsg::Reject(e) => bail!("host가 거부했다: {e}"),
        other => bail!("예상하지 못한 응답: {other:?}"),
    };
    agent.host_health.store(health, std::sync::atomic::Ordering::Relaxed);
    agent.set_host(Some(tx.clone()), replay);
    let cached = agent.health.lock().unwrap().clone();
    if let Some(h) = cached {
        agent.send_health(h).await;
    }
    let pinger = {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut seq = 0;
            loop {
                tokio::time::sleep(PING_EVERY).await;
                seq += 1;
                if tx.json(&LinkMsg::Ping { seq }).await.is_err() {
                    break;
                }
            }
        })
    };
    let result = async {
        loop {
            let next = tokio::time::timeout(DEAD_AFTER, rx.next()).await;
            match next {
                Err(_) => bail!("{}초 동안 host 응답이 없다", DEAD_AFTER.as_secs()),
                Ok(Err(e)) => return Err(e.into()),
                Ok(Ok(None)) => return Ok(()),
                Ok(Ok(Some(Incoming::Terminal(_)))) => {}
                Ok(Ok(Some(Incoming::Control(b)))) => match serde_json::from_slice::<LinkMsg>(&b) {
                    Ok(LinkMsg::Req { id, body, .. }) => {
                        tokio::spawn(agent.clone().handle(id, body));
                    }
                    Ok(LinkMsg::Ping { seq }) => {
                        let _ = tx.json(&LinkMsg::Pong { seq }).await;
                    }
                    Ok(LinkMsg::Pong { .. }) => {}
                    Ok(LinkMsg::Reject(e)) => bail!("host가 연결을 끊었다: {e}"),
                    Ok(other) => tracing::debug!("무시한 host 메시지: {other:?}"),
                    Err(e) => bail!("host 메시지 해석 실패: {e}"),
                },
            }
        }
    }
    .await;
    pinger.abort();
    result
}

/// `aaa agent enroll <host:port>`: 등록 창이 열린 host에 장치 키를 등록한다.
/// 양쪽이 같은 6자리 확인 코드를 계산하고, 사용자가 host에서 그 코드를 입력해 승인한다.
pub async fn enroll(state: &std::path::Path, addr: &str, machine_id: &str, name: &str) -> Result<HostPin> {
    let id = tls::load_or_create(state, "aaa-agent")?;
    let tcp = tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(addr))
        .await
        .context("연결 시간 초과")??;
    let connector = tokio_rustls::TlsConnector::from(tls::client_config(&id, None)?);
    let stream = connector.connect(tls::server_name(), tcp).await.context("TLS")?;
    let (host_fp, code) = {
        let (_, c) = stream.get_ref();
        let host_fp = tls::peer_fingerprint(c).context("host 인증서 없음")?;
        let code = tls::sas(c, &host_fp, &id.fingerprint)?;
        (host_fp, code)
    };
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME_PREAUTH);
    let mut rx = FrameRx::new(r, MAX_FRAME_PREAUTH);
    tx.json(&LinkMsg::Enroll(EnrollRequest {
        proto: AGENT_LINK,
        machine_id: machine_id.to_string(),
        machine_name: name.to_string(),
    }))
    .await?;
    loop {
        let msg: LinkMsg = rx.control().await?;
        match msg {
            LinkMsg::EnrollPending => {
                println!("확인 코드: {} {}", &code[..3], &code[3..]);
                println!("host 머신에서 다음을 실행해 승인한다:");
                println!("  aaa machine approve {name} --code {code}");
                println!("(host fingerprint {}…)", &host_fp[..16]);
            }
            LinkMsg::EnrollAccepted { machine_id: mid } => {
                if mid != machine_id {
                    bail!("host가 다른 machine ID로 등록했다: {mid}");
                }
                let pin = HostPin {
                    address: addr.to_string(),
                    fingerprint: host_fp,
                    enrolled_at_ms: aaa_common::util::now_ms(),
                };
                save_pin(state, &pin)?;
                return Ok(pin);
            }
            LinkMsg::Reject(e) => bail!("등록 거부: {e}"),
            other => bail!("예상하지 못한 응답: {other:?}"),
        }
    }
}
