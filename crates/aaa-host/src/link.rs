//! host 쪽 Agent link listener. 로컬 agent는 UDS(peer UID), 원격 agent는 TCP+TLS 1.3(장치 키 fingerprint).
//! 등록되지 않은 인증서는 등록 창이 열려 있을 때만 enrollment로 받아들인다.

use crate::supervisor::Task;
use futures_util::FutureExt;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{
    AGENT_LINK, AgentEvent, AgentHello, AgentResponse, ErrorBody, ErrorCode, HostWelcome, LinkMsg, MAX_FRAME,
    MAX_FRAME_PREAUTH, Outcome, negotiate, require_features,
};
use anyhow::{Context, Result, bail};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::Semaphore;

use super::{AgentConn, Host};
use aaa_common::conn::{FrameRx, FrameTx, Incoming, spawn_writer};
use aaa_common::tls;
use aaa_common::util;
use aaa_proto::{LINK_DEAD_AFTER as DEAD_AFTER, LINK_PING_EVERY as PING_EVERY};

/// 인증 전 동시 연결 상한
const PREAUTH_LIMIT: usize = 16;

pub fn local_socket(host: &Host) -> std::path::PathBuf {
    host.state.join("agent.sock")
}

pub async fn start(host: &Arc<Host>) -> Result<()> {
    let local = aaa_common::ipc::LocalListener::bind(&local_socket(host))
        .context("로컬 agent socket(이미 host가 실행 중인가?)")?;
    {
        let local = Arc::new(local);
        let host = host.clone();
        host.clone().supervise(Task::LocalLink, move || {
            let host = host.clone();
            let local = local.clone();
            async move {
                host.task_progress(Task::LocalLink);
                loop {
                    let (s, pid) = match local.accept_peer().await {
                        Ok(pair) => pair,
                        Err(e) => {
                            host.resource_error("local agent accept", &e);
                            tracing::warn!(error = %e, "로컬 agent accept 실패");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    let host = host.clone();
                    tokio::spawn(async move {
                        if let Err(e) = serve(host, s, None, pid).await {
                            tracing::info!("로컬 agent 연결 끝: {e:#}");
                        }
                    });
                }
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
        let listener = Arc::new(listener);
        host.clone().supervise(Task::RemoteLink, move || {
            let (host, listener, gate, acceptor) = (host.clone(), listener.clone(), gate.clone(), acceptor.clone());
            async move {
                host.task_progress(Task::RemoteLink);
                loop {
                    let (tcp, peer) = match listener.accept().await {
                        Ok(pair) => pair,
                        Err(e) => {
                            host.resource_error("agent link accept", &e);
                            tracing::warn!(error = %e, "agent link accept 실패");
                            tokio::time::sleep(Duration::from_secs(1)).await;
                            continue;
                        }
                    };
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
                                serve(host, Box::new(tls), Some(m.id), None).await
                            }
                            None => enroll(host, tls, &fp, &sas_base).await,
                        };
                        if let Err(e) = res {
                            tracing::info!(%peer, "agent 연결 끝: {e:#}");
                        }
                    });
                }
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
            return reject(&tx, ErrorCode::NotEnrolled, "등록되지 않은 장치다. `aaa agent enroll`을 먼저 한다").await;
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

/// 이미 등록된 머신이 다시 등록하려 할 때의 안내: 등록 없이 붙는 길과 다시 등록하는 길
fn already_enrolled(name: &str) -> String {
    format!(
        "이 머신({name})은 이미 이 host에 등록돼 있다. 다시 등록하지 않고 `aaa agent`로 붙는다(agent.toml [agent] host). \
         다시 등록하려면 host 머신에서 `aaa machine revoke {name}`·`aaa machine enroll --open 10m`을 한 뒤 이 머신에서 \
         `aaa agent enroll <host:port>`를 다시 한다"
    )
}

/// 인증된 agent: hello → welcome → 요청·사건·출력 처리. `known`은 TLS로 확인한 머신 ID(로컬은 None), `pid`는 로컬 agent
/// 프로세스(socket peer credential).
type Cleanup = (String, u64, super::Pending, tokio::task::JoinHandle<()>);

async fn serve(
    host: Arc<Host>,
    stream: aaa_common::ipc::LocalStream,
    known: Option<String>,
    pid: Option<u32>,
) -> Result<()> {
    if !host.task_allowed(Task::AgentLink) {
        bail!("agent-link 자동 복구 상한: host 재시작 필요");
    }
    tokio::time::sleep(host.task_retry_delay(Task::AgentLink)).await;
    if !host.task_allowed(Task::AgentLink) {
        bail!("agent-link 자동 복구 상한: host 재시작 필요");
    }
    let mut cleanup = None;
    let result =
        AssertUnwindSafe(serve_connection(host.clone(), stream, known, pid, &mut cleanup)).catch_unwind().await;
    let panicked = result.is_err();
    if panicked {
        host.task_failed(Task::AgentLink);
    }
    // 정리 중 poison 등으로 또 panic해도 감시 기록은 남는다. 새 generation을 끊지 않는다.
    let cleaned = std::panic::catch_unwind(AssertUnwindSafe(|| {
        if let Some((mid, generation, pending, pinger)) = cleanup {
            pinger.abort();
            pending.lock().unwrap().clear();
            host.agent_disconnected(&mid, generation);
        }
    }));
    if cleaned.is_err() && !panicked {
        host.task_failed(Task::AgentLink);
    }
    match result {
        Ok(result) if cleaned.is_ok() => result,
        _ => bail!("agent-link panic: 내용 숨김"),
    }
}

async fn serve_connection(
    host: Arc<Host>,
    stream: aaa_common::ipc::LocalStream,
    known: Option<String>,
    pid: Option<u32>,
    cleanup: &mut Option<Cleanup>,
) -> Result<()> {
    let local = known.is_none();
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let first: LinkMsg = tokio::time::timeout(Duration::from_secs(10), rx.control()).await.context("hello 대기")??;
    let hello = match first {
        LinkMsg::Hello(hello) => hello,
        // 이미 등록된 장치 키로 `aaa agent enroll`을 다시 했다(QA t-0337 Q17): 무엇이 문제이고 어떻게 하는지 알린다
        LinkMsg::Enroll(req) if known.is_some() => {
            let name = known
                .as_deref()
                .and_then(|id| host.db.machines().ok()?.into_iter().find(|m| m.id == id))
                .map_or(req.machine_name, |m| m.name);
            return reject(&tx, ErrorCode::BadRequest, &already_enrolled(&name)).await;
        }
        _ => return reject(&tx, ErrorCode::BadRequest, "hello가 와야 한다").await,
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
        identity: hello.features.iter().any(|f| f == "rollout_recovery.v1").then(|| aaa_proto::RuntimeIdentity {
            sha256: host.running_sha256.clone(),
            software: aaa_common::util::software(),
            instance: host.rollout_instance.clone(),
            started_ms: host.rollout_started_ms,
        }),
        proto: agreed,
        host_id: host.host_id.clone(),
        // harness_hello.v1: hello의 harness 사건을 읽는다(agent는 연결 뒤 다시 보내지 않는다, 재부팅 복원)
        features: vec![
            "terminal.v1".into(),
            "health.v1".into(),
            "harness_hello.v1".into(),
            "post.v1".into(),
            "mission.v1".into(), // N-1 agent의 파일 전송
            "work_files.v1".into(),
            "rollout_recovery.v1".into(),
            // compacting.v1: 화면 감지 사건 `compacting`(정리 중, plan §8.309)을 읽는다. 옛 host에는 agent가 `working`으로 보낸다
            "compacting.v1".into(),
        ],
    }))
    .await?;
    let generation = host.next_gen();
    let mut conn = AgentConn::new(tx.clone(), generation);
    conn.timeouts = host.cfg.host.clone();
    conn.health_host = Arc::downgrade(&host);
    let pending = conn.pending.clone();
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
    *cleanup = Some((mid.clone(), generation, pending.clone(), pinger));
    host.agent_connected(&hello, local, conn);
    if local {
        host.note_agent_pid(&hello.machine_id, pid);
    }
    tracing::info!(machine = %hello.machine_name, sessions = hello.sessions.len(), local, pid = ?pid, "agent 연결");

    host.task_progress(Task::AgentLink);
    let result: Result<()> = async {
        loop {
            let next = tokio::time::timeout(DEAD_AFTER, rx.next()).await;
            match next {
                Err(_) => bail!("{}초 동안 agent 응답이 없다", DEAD_AFTER.as_secs()),
                Ok(Err(e)) => return Err(e.into()),
                Ok(Ok(None)) => return Ok(()),
                Ok(Ok(Some(Incoming::Terminal(c)))) => host.on_chunk(&mid, c),
                Ok(Ok(Some(Incoming::Control(b)))) => match match serde_json::from_slice::<LinkMsg>(&b)? {
                    // 실행 입력과 배포 후보는 표시용 글이 아니다. B64 안의 인증 조각을 가리면
                    // 길이는 같아도 바이너리 SHA가 달라져 마지막 EOF 검증에서 멈춘다.
                    msg @ LinkMsg::Res { result: Outcome::Ok(AgentResponse::ThreadReady(_) | AgentResponse::Relaunch(_) | AgentResponse::DeployChunk { .. }), .. } => msg,
                    msg => aaa_common::redact::json(msg).map_err(|_| anyhow::anyhow!("agent 메시지 가림 거부"))?,
                } {
                    LinkMsg::Event(AgentEvent::AccountRefresh(request)) => {
                        let h=host.clone();let m=mid.clone();tokio::spawn(async move {h.account_refresh(m,request).await;});
                    }
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
                    LinkMsg::Event(AgentEvent::Harness(ev)) => {
                        host.on_harness(&mid, ev);
                        // 대화 감시는 harness·기록 위치를 따라간다
                        host.sync_chat_watches_if_wanted();
                    }
                    LinkMsg::Event(AgentEvent::DeploymentChanged { job }) => host.deployment_changed(&mid, job),
                    LinkMsg::Event(AgentEvent::DeployGuardChanged { .. }) => host.rollout_wake.notify_one(),
                    LinkMsg::Event(AgentEvent::ChatChanged { key, signature }) => {
                        host.on_chat_changed(&mid, key, signature)
                    }
                    LinkMsg::Event(AgentEvent::PostRejected { session, post_id, bytes, sha256, reason }) => {
                        if hello.features.iter().any(|f| f == "post.v1") {
                            host.db.audit(&format!("post:unverified@{}", host.machine_name(&mid)), "post_refuse", &serde_json::json!({"session":session,"post_id":post_id,"bytes":bytes,"sha256":sha256,"reason":reason}).to_string());
                        }
                    }
                    LinkMsg::Event(AgentEvent::Post { session, input }) => {
                        if hello.features.iter().any(|f| f == "post.v1") {
                            let result = host.receive_post(&mid, &session, input.clone());
                            tx.json(&LinkMsg::Req { id: format!("post-{}", input.post_id), deadline_ms: None, body: aaa_proto::AgentRequest::PostAck { post_id: input.post_id, result } }).await?;
                        }
                    }
                    LinkMsg::Event(AgentEvent::Health(h)) => host.on_health(&mid, h),
                    LinkMsg::Ping { seq } => {
                        let _ = tx.json(&LinkMsg::Pong { seq }).await;
                    }
                    LinkMsg::Pong { .. } => {}
                    other => tracing::debug!("무시한 agent 메시지: {other:?}"),
                },
            }
            host.task_progress(Task::AgentLink);
        }
    }
    .await;

    tracing::info!(machine = %hello.machine_name, "agent 연결 끊김");
    result
}

/// 같은 머신의 agent(UDS, 같은 사용자): 등록 없이 로컬 머신으로 받아들인다.
async fn register_local(host: &Host, hello: &AgentHello, tx: &FrameTx) -> Result<()> {
    let known = host.db.machine_by_name(&hello.machine_name).ok().flatten();
    if let Some(m) = &known
        && m.id != hello.machine_id
    {
        return reject(tx, ErrorCode::NameTaken, &format!("이름 {}은 다른 머신이 쓴다", hello.machine_name)).await;
    }
    // 새 이름만 검사한다. 이미 이 머신의 이름으로 등록된 것은 그대로 받는다(올리기 전 이름).
    if known.is_none() && !util::machine_name_ok(&hello.machine_name) {
        return reject(tx, ErrorCode::BadRequest, crate::MACHINE_NAME_RULE).await;
    }
    host.db.upsert_machine(&hello.machine_id, &hello.machine_name, None, true)?;
    Ok(())
}

#[cfg(test)]
mod post_tests {
    use super::*;
    use aaa_proto::{PostInput, PostType};
    #[tokio::test]
    async fn deployment_bytes_survive_auth_redaction_and_a_partial_tail() {
        use aaa_proto::{AgentRequest, B64, DEPLOY_CHUNK};
        use base64::Engine;
        use sha2::{Digest, Sha256};

        let path = std::env::temp_dir().join(format!("aaa-link-tail-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = path.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let host = Host::open(cfg, path.clone()).unwrap();
        let secret = "fixture-deployment-auth-value-0525";
        aaa_common::redact::remember_auth("codex", &serde_json::json!({"access_token":secret}).to_string());
        let encoded = base64::engine::general_purpose::STANDARD.encode(secret);
        let mut bytes: Vec<u8> = (0..DEPLOY_CHUNK * 2 + 3176).map(|i| (i % 251) as u8).collect();
        // 인증과 겹치는 바이트를 온전한 조각·마지막 부분 조각에 둔다. 실제 인증은 읽지 않는다.
        for at in [23, DEPLOY_CHUNK * 2 + 17] {
            bytes[at..at + secret.len()].copy_from_slice(secret.as_bytes());
            bytes[at + 100..at + 100 + encoded.len()].copy_from_slice(encoded.as_bytes());
        }
        let expected = hex::encode(Sha256::digest(&bytes));
        let (stream, peer) = tokio::io::duplex(1024);
        let running = tokio::spawn(serve(host.clone(), Box::new(stream), None, None));
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        tx.json(&LinkMsg::Hello(AgentHello {
            proto: AGENT_LINK,
            machine_id: "transfer-agent".into(),
            machine_name: "mac-a".into(),
            boot_id: "boot".into(),
            os: "test".into(),
            arch: "test".into(),
            software: "fixture".into(),
            features: vec!["terminal.v1".into(), "deploy_stage.v1".into()],
            dev_mode: true,
            selfcheck: None,
            sessions: vec![],
            harness: vec![],
        }))
        .await
        .unwrap();
        assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Welcome(_)));
        tx.json(&LinkMsg::Ping { seq: 525 }).await.unwrap();
        assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 525 });
        let conn = host.live_conn("transfer-agent").unwrap();
        let mut received = Vec::new();
        for chunk in bytes.chunks(DEPLOY_CHUNK) {
            let offset = received.len() as u64;
            let eof = offset + chunk.len() as u64 == bytes.len() as u64;
            let request = AgentRequest::DeployFetch {
                path: "/build/aaa".into(),
                offset,
                len: DEPLOY_CHUNK as u32,
                sha256: expected.clone(),
            };
            let (response, ()) = tokio::join!(conn.request(request.clone()), async {
                let LinkMsg::Req { id, body, .. } = rx.control::<LinkMsg>().await.unwrap() else {
                    panic!("요청이어야 한다")
                };
                assert_eq!(body, request);
                tx.json(&LinkMsg::Res {
                    id,
                    result: Outcome::Ok(AgentResponse::DeployChunk {
                        offset,
                        data: B64(chunk.to_vec()),
                        eof,
                        total: bytes.len() as u64,
                    }),
                })
                .await
                .unwrap();
            });
            let AgentResponse::DeployChunk { offset: got, data, eof: done, total } = response.unwrap() else {
                panic!("조각이어야 한다")
            };
            assert_eq!((got, done, total, data.0.len()), (offset, eof, bytes.len() as u64, chunk.len()));
            received.extend(data.0);
        }
        // 파일 보기 응답은 여전히 가린다. 전송 보존 때문에 표시 경계의 가림을 끄면 안 된다.
        let (visible, ()) = tokio::join!(
            conn.request(AgentRequest::FileView {
                path: "/build/readme.txt".into(),
                offset: 0,
                len: DEPLOY_CHUNK as u32,
                page: false,
            }),
            async {
                let LinkMsg::Req { id, .. } = rx.control::<LinkMsg>().await.unwrap() else {
                    panic!("요청이어야 한다")
                };
                tx.json(&LinkMsg::Res {
                    id,
                    result: Outcome::Ok(AgentResponse::FileChunk { data: B64(secret.as_bytes().to_vec()), eof: true }),
                })
                .await
                .unwrap();
            }
        );
        let AgentResponse::FileChunk { data, .. } = visible.unwrap() else { panic!("보기 조각이어야 한다") };
        assert!(
            !data.0.windows(8).any(|s| secret.as_bytes().windows(8).any(|n| s == n)),
            "파일 보기에서 인증이 노출됐다"
        );
        // host 일반 응답의 B64 글자만 겹쳐도 reader가 살아 있어야 한다.
        aaa_common::redact::remember("QkxPQ0tTRUNSRVQ5host-fixture0525");
        let snapshot = base64::engine::general_purpose::STANDARD.decode("QkxPQ0tTRUNSRVQ5AAAA").unwrap();
        let (visible, ()) = tokio::join!(
            conn.request(AgentRequest::FileView {
                path: "/build/snapshot.bin".into(),
                offset: 0,
                len: DEPLOY_CHUNK as u32,
                page: false,
            }),
            async {
                let LinkMsg::Req { id, .. } = rx.control::<LinkMsg>().await.unwrap() else {
                    panic!("요청이어야 한다")
                };
                tx.json(&LinkMsg::Res {
                    id,
                    result: Outcome::Ok(AgentResponse::FileChunk { data: B64(snapshot.clone()), eof: true }),
                })
                .await
                .unwrap();
            }
        );
        assert!(matches!(visible.unwrap(), AgentResponse::FileChunk { data, .. } if data.0 == snapshot));
        tx.json(&LinkMsg::Ping { seq: 526 }).await.unwrap();
        assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 526 });
        drop(conn);
        drop(tx);
        drop(rx);
        running.await.unwrap().unwrap();
        drop(host);
        std::fs::remove_dir_all(path).unwrap();
        assert_eq!(hex::encode(Sha256::digest(&received)), expected, "전송 EOF SHA-256/크기 불일치");
        assert_eq!(received, bytes);
    }
    #[tokio::test]
    async fn post_ack_is_never_sent_to_agent_without_feature() {
        let path = std::env::temp_dir().join(format!("aap-host-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = path.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let host = Host::open(cfg, path.clone()).unwrap();
        let (stream, peer) = tokio::io::duplex(1 << 20);
        let running = tokio::spawn(serve(host, Box::new(stream), None, None));
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        tx.json(&LinkMsg::Hello(AgentHello {
            proto: AGENT_LINK,
            machine_id: "old-agent".into(),
            machine_name: "mac-a".into(),
            boot_id: "boot".into(),
            os: "test".into(),
            arch: "test".into(),
            software: "old".into(),
            features: vec!["terminal.v1".into()],
            dev_mode: true,
            selfcheck: None,
            sessions: vec![],
            harness: vec![],
        }))
        .await
        .unwrap();
        assert!(
            matches!(rx.control::<LinkMsg>().await.unwrap(),LinkMsg::Welcome(w) if w.features.iter().any(|f|f=="post.v1"))
        );
        tx.json(&LinkMsg::Event(AgentEvent::Post {
            session: "s".into(),
            input: PostInput {
                post_id: format!("{:032x}", 1),
                to: "coordinator".into(),
                post_type: PostType::Data,
                topic: "test".into(),
                title: "기록".into(),
                body: "본문".into(),
                needs_user: false,
                refs: vec![],
            },
        }))
        .await
        .unwrap();
        tx.json(&LinkMsg::Ping { seq: 43 }).await.unwrap();
        // 같은 연결의 Pong은 앞 사건 처리가 끝났다는 울타리다. Ack를 먼저 보내면 여기서 실패한다.
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), rx.control::<LinkMsg>()).await.unwrap().unwrap(),
            LinkMsg::Pong { seq: 43 }
        );
        drop(tx);
        drop(rx);
        running.await.unwrap().unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(test)]
mod supervision_tests;
