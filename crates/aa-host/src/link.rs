//! host 쪽 Agent link listener. 로컬 agent는 UDS(peer UID), 원격 agent는 TCP+TLS 1.3(장치 키 fingerprint).
//! 등록되지 않은 인증서는 등록 창이 열려 있을 때만 enrollment로 받아들인다.

use std::sync::Arc;
use std::time::Duration;

use aa_proto::{
    AGENT_LINK, AgentEvent, AgentHello, AgentResponse, ErrorBody, ErrorCode, HostWelcome, LinkMsg, MAX_FRAME,
    MAX_FRAME_PREAUTH, Outcome, negotiate, require_features,
};
use anyhow::{Context, Result, bail};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Semaphore;

use super::{AgentConn, Host};
use aa_common::conn::{FrameRx, FrameTx, Incoming, spawn_writer};
use aa_common::tls;
use aa_proto::{LINK_DEAD_AFTER as DEAD_AFTER, LINK_PING_EVERY as PING_EVERY};

/// 인증 전 동시 연결 상한
const PREAUTH_LIMIT: usize = 16;

pub fn local_socket(host: &Host) -> std::path::PathBuf {
    host.state.join("agent.sock")
}

pub async fn start(host: &Arc<Host>) -> Result<()> {
    let local = aa_common::ipc::LocalListener::bind(&local_socket(host))
        .context("로컬 agent socket(이미 host가 실행 중인가?)")?;
    {
        let host = host.clone();
        tokio::spawn(async move {
            while let Ok(s) = local.accept().await {
                let host = host.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve(host, s, None).await {
                        tracing::info!("로컬 agent 연결 끝: {e:#}");
                    }
                });
            }
        });
    }
    let addr = host.cfg.host.agent_listen.clone();
    if !addr.is_empty() {
        let listener =
            tokio::net::TcpListener::bind(&addr).await.with_context(|| format!("Agent link listen {addr}"))?;
        let bound = listener.local_addr()?.to_string();
        *host.agent_listen.lock().unwrap() = Some(bound.clone());
        tracing::info!(%bound, "Agent link(TLS) listen");
        let acceptor = tokio_rustls::TlsAcceptor::from(tls::server_config(&host.identity)?);
        let gate = Arc::new(Semaphore::new(PREAUTH_LIMIT));
        let host = host.clone();
        tokio::spawn(async move {
            loop {
                let Ok((tcp, peer)) = listener.accept().await else { continue };
                let Ok(permit) = gate.clone().try_acquire_owned() else {
                    tracing::warn!(%peer, "인증 전 연결이 너무 많아 끊었다");
                    continue;
                };
                let (host, acceptor) = (host.clone(), acceptor.clone());
                tokio::spawn(async move {
                    let _ = tcp.set_nodelay(true);
                    let tls = match tokio::time::timeout(Duration::from_secs(10), acceptor.accept(tcp)).await {
                        Ok(Ok(s)) => s,
                        _ => {
                            tracing::debug!(%peer, "TLS handshake 실패");
                            return;
                        }
                    };
                    let (fp, sas_base) = {
                        let (_, c) = tls.get_ref();
                        let fp = tls::peer_fingerprint(c).unwrap_or_default();
                        let sas = tls::sas(c, &host.identity.fingerprint, &fp).unwrap_or_default();
                        (fp, sas)
                    };
                    let known = host.db.machine_by_fingerprint(&fp).ok().flatten();
                    let res = match known {
                        Some(m) => {
                            drop(permit);
                            serve(host, Box::new(tls), Some(m.id)).await
                        }
                        None => enroll(host, tls, &fp, &sas_base).await,
                    };
                    if let Err(e) = res {
                        tracing::info!(%peer, "agent 연결 끝: {e:#}");
                    }
                });
            }
        });
    }
    Ok(())
}

async fn reject(tx: &FrameTx, code: ErrorCode, msg: &str) -> Result<()> {
    let _ = tx.json(&LinkMsg::Reject(ErrorBody::new(code, msg))).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    bail!("{msg}")
}

/// 등록되지 않은 장치: Enroll 요청 → 사용자 승인 대기
async fn enroll<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    host: Arc<Host>,
    stream: S,
    fp: &str,
    sas: &str,
) -> Result<()> {
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME_PREAUTH);
    let mut rx = FrameRx::new(r, MAX_FRAME_PREAUTH);
    let first: LinkMsg =
        tokio::time::timeout(Duration::from_secs(10), rx.control()).await.context("첫 메시지 대기")??;
    let req = match first {
        LinkMsg::Enroll(req) => req,
        LinkMsg::Hello(_) => {
            return reject(&tx, ErrorCode::NotEnrolled, "등록되지 않은 장치다. `aa agent enroll`을 먼저 한다").await;
        }
        _ => return reject(&tx, ErrorCode::BadRequest, "Enroll이 와야 한다").await,
    };
    if let Err(e) = negotiate(AGENT_LINK, req.proto) {
        return reject(&tx, ErrorCode::UnsupportedVersion, &e.to_string()).await;
    }
    let accepted = match host.enroll_request(&req.machine_name, &req.machine_id, fp, sas) {
        Ok(rx) => rx,
        Err(e) => {
            let msg = e.message.clone();
            return reject(&tx, e.code, &msg).await;
        }
    };
    tx.json(&LinkMsg::EnrollPending).await?;
    tracing::info!(name = %req.machine_name, "등록 요청: host에서 확인 코드로 승인을 기다린다");
    // 상대가 끊으면 대기를 버린다
    let closed = async { while let Ok(Some(_)) = rx.next().await {} };
    tokio::select! {
        r = accepted => match r {
            Ok(()) => {
                tx.json(&LinkMsg::EnrollAccepted { machine_id: req.machine_id.clone() }).await?;
                tokio::time::sleep(Duration::from_millis(100)).await;
                tracing::info!(name = %req.machine_name, "등록 승인");
                Ok(())
            }
            Err(_) => reject(&tx, ErrorCode::EnrollCodeMismatch, "등록이 승인되지 않았다").await,
        },
        _ = closed => {
            host.enroll_cancel(&req.machine_name, fp);
            bail!("등록 대기 중 연결이 끊겼다")
        }
        _ = tokio::time::sleep(Duration::from_secs(15 * 60)) => {
            host.enroll_cancel(&req.machine_name, fp);
            reject(&tx, ErrorCode::EnrollClosed, "등록 대기 시간이 지났다").await
        }
    }
}

/// 인증된 agent: hello → welcome → 요청·사건·출력 처리. `known`은 TLS로 확인한 머신 ID(로컬은 None).
async fn serve(host: Arc<Host>, stream: aa_common::ipc::LocalStream, known: Option<String>) -> Result<()> {
    let local = known.is_none();
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let first: LinkMsg = tokio::time::timeout(Duration::from_secs(10), rx.control()).await.context("hello 대기")??;
    let LinkMsg::Hello(hello) = first else {
        return reject(&tx, ErrorCode::BadRequest, "hello가 와야 한다").await;
    };
    let agreed = match negotiate(AGENT_LINK, hello.proto)
        .and_then(|v| require_features(&["terminal.v1"], &hello.features).map(|_| v))
    {
        Ok(v) => v,
        Err(e) => return reject(&tx, ErrorCode::UnsupportedVersion, &e.to_string()).await,
    };
    if let Some(id) = &known
        && *id != hello.machine_id
    {
        return reject(&tx, ErrorCode::Unauthorized, "등록된 machine ID와 다르다").await;
    }
    if local {
        register_local(&host, &hello, &tx).await?;
    }
    tx.json(&LinkMsg::Welcome(HostWelcome {
        proto: agreed,
        host_id: host.host_id.clone(),
        features: vec!["terminal.v1".into()],
    }))
    .await?;
    let generation = host.next_gen();
    let conn = AgentConn::new(tx.clone(), generation);
    let pending = conn.pending.clone();
    host.agent_connected(&hello, local, conn);
    tracing::info!(machine = %hello.machine_name, sessions = hello.sessions.len(), local, "agent 연결");

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
    let mid = hello.machine_id.clone();
    let result: Result<()> = async {
        loop {
            let next = tokio::time::timeout(DEAD_AFTER, rx.next()).await;
            match next {
                Err(_) => bail!("{}초 동안 agent 응답이 없다", DEAD_AFTER.as_secs()),
                Ok(Err(e)) => return Err(e.into()),
                Ok(Ok(None)) => return Ok(()),
                Ok(Ok(Some(Incoming::Terminal(c)))) => host.on_chunk(&mid, c),
                Ok(Ok(Some(Incoming::Control(b)))) => match serde_json::from_slice::<LinkMsg>(&b)? {
                    LinkMsg::Res { id, result } => {
                        // 구독 응답은 이어지는 출력보다 먼저 mirror에 반영한다(같은 reader 순서).
                        if let Outcome::Ok(AgentResponse::Subscribed { session, snapshot, offset }) = &result {
                            host.on_subscribed(session, snapshot.clone(), *offset);
                        }
                        if let Some(waiter) = pending.lock().unwrap().remove(&id) {
                            let _ = waiter.send(result);
                        }
                    }
                    LinkMsg::Event(AgentEvent::SessionStarted(info)) => host.on_session(&mid, info, true),
                    LinkMsg::Event(AgentEvent::SessionUpdated(info)) => host.on_session(&mid, info, false),
                    LinkMsg::Event(AgentEvent::Resync { session }) => host.on_resync(&mid, &session),
                    LinkMsg::Event(AgentEvent::ReportChanged { thread, hash }) => {
                        host.on_report_changed(&mid, &thread, hash)
                    }
                    LinkMsg::Ping { seq } => {
                        let _ = tx.json(&LinkMsg::Pong { seq }).await;
                    }
                    LinkMsg::Pong { .. } => {}
                    other => tracing::debug!("무시한 agent 메시지: {other:?}"),
                },
            }
        }
    }
    .await;
    pinger.abort();
    host.agent_disconnected(&mid, generation);
    tracing::info!(machine = %hello.machine_name, "agent 연결 끊김");
    result
}

/// 같은 머신의 agent(UDS, 같은 사용자): 등록 없이 로컬 머신으로 받아들인다.
async fn register_local(host: &Host, hello: &AgentHello, tx: &FrameTx) -> Result<()> {
    if let Ok(Some(m)) = host.db.machine_by_name(&hello.machine_name)
        && m.id != hello.machine_id
    {
        return reject(tx, ErrorCode::NameTaken, &format!("이름 {}은 다른 머신이 쓴다", hello.machine_name)).await;
    }
    host.db.upsert_machine(&hello.machine_id, &hello.machine_name, None, true)?;
    Ok(())
}
