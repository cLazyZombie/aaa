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
    run_with_state(agent, target, max_backoff, None).await
}

/// 실제 Welcome/replay 완료와 연결 종료를 감독자에 알린다. backend 기동 준비와는 별개다.
pub async fn run_with_state(
    agent: Arc<Agent>,
    target: Target,
    max_backoff: Duration,
    state: Option<tokio::sync::watch::Sender<bool>>,
) {
    let mut backoff = Duration::from_millis(250);
    loop {
        let started = Instant::now();
        match connect(&target).await {
            Ok(stream) => match session_state(&agent, stream, state.as_ref(), &mut backoff).await {
                Ok(()) => tracing::info!("host 연결이 닫혔다({target})"),
                Err(e) => tracing::warn!("host 연결 끝: {e:#}"),
            },
            Err(e) => tracing::debug!("host 연결 실패({target}): {e:#}"),
        }
        agent.set_host(None);
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
        sha256: Some(agent.deployments.running_sha256().into()),
        // cwd.v1: `AgentRequest::Cwd`(옛 agent는 모르는 요청에 연결을 끊으므로 host는 이 표시를 보고 보낸다), rename.v1: `AgentRequest::Rename`,
        // history.v1: `AgentRequest::History`(plan §8.118)
        // file.v1: 파일 보기 `FileLocate`·`FileView`(plan §8.125), profile.v1: `SpawnSpec.profile`(plan §8.126)
        // ask_anew.v1: 막힘에 입력이 간 뒤 화면에 남은 확인창을 같은 글이어도 다시 알린다(`detect::Watch::input_sent`).
        // host는 그 질문을 답한 질문의 늦은 사건으로 거르지 않는다
        // dir.v1: 폴더 보기 `DirList`(plan §8.148), file_in.v1: 정한 폴더 기준 찾기 `FileLocateIn`(보고서 안 파일 링크, plan §8.171)
        // profiles.v2: 옛 host의 이름 기반 요청. host_profiles.v1이면 host 정본과 머신별 덮어쓰기만 쓴다(plan §8.170),
        // launch_safety.v1: `ThreadPrepare.safety`·`ThreadRebrief.safety`(yolo·thread_agent_args)를 profile 인자에 더한다
        // adopt.v1: `AgentRequest::ThreadAdopt`(사람이 띄운 agent 세션을 thread로 데려온다, plan §8.180)
        // term_colors.v1: `AgentRequest::TermColors`(색 질의에 답할 화면 색, plan §8.187)
        // diff.v1: `AgentRequest::ThreadDiff`(thread worktree의 바뀐 파일·patch, plan §8.305)
        // chat_watch.v1: `AgentRequest::ChatWatch`와 `AgentEvent::ChatChanged`(대화 기록 변화 push, plan §8.191)
        // commands.v1: `AgentRequest::Commands`(입력칸 「/」 목록, plan §8.323)
        features: vec![
            "commands.v1".into(),
            "account_auth.v1".into(),
            "account_auth.v2".into(),
            "account_quota.v1".into(),
            "terminal.v1".into(),
            "report_copy.v2".into(),
            "deployment.v1".into(),
            #[cfg(target_os = "macos")]
            "rollout.v1".into(),
            #[cfg(target_os = "macos")]
            "deploy_stage.v1".into(),
            #[cfg(target_os = "macos")]
            "rollout_recovery.v1".into(),
            // 웹 「배포」의 빌드 후보 읽기(`AgentRequest::DeployCandidate`, 관리 GUI W6a)
            #[cfg(target_os = "macos")]
            "deploy_candidate.v1".into(),
            "post.v1".into(),
            "imagegen.v1".into(),
            "mission.v1".into(), // N-1 host의 파일 전송
            "work_files.v1".into(),
            "cwd.v1".into(),
            "pr.v1".into(),
            "pr.v2".into(),
            "ci.v1".into(),
            "git.v1".into(),
            "workspace_metrics.v1".into(),
            "git_merged.v1".into(),
            "rename.v1".into(),
            "history.v1".into(),
            "profiles.v2".into(),
            // host의 ResolvedProfile과 설치 harness 조회(Harnesses)를 받는다.
            "host_profiles.v1".into(),
            "thread_journal.v1".into(),
            "launch_safety.v1".into(),
            "file.v1".into(),
            "profile.v1".into(),
            "ask_anew.v1".into(),
            "dir.v1".into(),
            "file_in.v1".into(),
            "adopt.v1".into(),
            "term_colors.v1".into(),
            "chat_watch.v1".into(),
            "chat_pages.v1".into(),
            "diff.v1".into(),
            // task 첨부를 thread 작업 폴더로 받는다(WorkAttachWrite, plan §8.327)
            "work_attach.v1".into(),
            // 웹 설정 관리(관리 GUI W5c): 작업 root 보기·점검·전환, Codex hook 상태·설치·제거
            "work_root.manage.v1".into(),
            "hook.manage.v1".into(),
        ],
        dev_mode: agent.dev_mode,
        selfcheck: agent.selfcheck.lock().unwrap().clone(),
        sessions: agent.sessions(),
        identity: Some(agent.deployments.runtime_identity()),
        harness: agent.harness_events(),
    }
}

async fn session_state(
    agent: &Arc<Agent>,
    stream: LocalStream,
    state: Option<&tokio::sync::watch::Sender<bool>>,
    backoff: &mut Duration,
) -> Result<()> {
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let hello = hello(agent);
    let sent_harness = hello.harness.clone();
    tx.json(&LinkMsg::Hello(hello)).await?;
    let first: LinkMsg =
        tokio::time::timeout(Duration::from_secs(10), rx.control()).await.context("welcome 대기")??;
    let (health, replay, host_identity) = match first {
        LinkMsg::Welcome(w) => {
            tracing::info!(proto = %w.proto, host = %w.host_id, "host에 연결했다");
            *agent.host_post.lock().unwrap() = Some(w.features.iter().any(|f| f == "post.v1"));
            agent.host_image.store(w.features.iter().any(|f| f == "imagegen.v1"), std::sync::atomic::Ordering::Relaxed);
            *agent.host_work_files.lock().unwrap() = w.features.iter().any(|f| f == "work_files.v1");
            *agent.host_mission.lock().unwrap() = w.features.iter().any(|f| f == "mission.v1");
            agent
                .host_compacting
                .store(w.features.iter().any(|f| f == "compacting.v1"), std::sync::atomic::Ordering::Relaxed);
            agent
                .host_recovery
                .store(w.features.iter().any(|f| f == "rollout_recovery.v1"), std::sync::atomic::Ordering::Relaxed);
            // harness_hello.v1: host가 Hello의 사건을 읽었다. 새 host에는 그 뒤 변경분, 옛 host에는 최신 기억 전체를 보낸다
            (
                w.features.iter().any(|f| f == "health.v1"),
                !w.features.iter().any(|f| f == "harness_hello.v1"),
                w.identity,
            )
        }
        LinkMsg::Reject(e) => bail!("host가 거부했다: {e}"),
        other => bail!("예상하지 못한 응답: {other:?}"),
    };
    agent.host_health.store(health, std::sync::atomic::Ordering::Relaxed);
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
    // 큰 replay도 전송 backpressure에 막힐 수 있다. 그동안 EOF·기존 응답 상한과 요청 처리를 계속한다.
    let initialize = async {
        {
            // Hello의 스냅샷 뒤 Welcome을 기다리는 동안에도 감지·hook은 기억을 갱신한다.
            // 상태 사건은 replay 뒤에 세우되 일반 요청 응답은 Welcome 직후부터 보낸다.
            let _delivery = agent.harness_delivery.lock().await;
            agent.set_host(Some(tx.clone()));
            let compacting = agent.host_compacting.load(std::sync::atomic::Ordering::Relaxed);
            for ev in agent.harness_events().into_iter().filter(|ev| replay || !sent_harness.contains(ev)) {
                tx.json(&LinkMsg::Event(aaa_proto::AgentEvent::Harness(super::for_host(ev, compacting)))).await?;
            }
        }
        if let Some(identity) = host_identity {
            agent.observe_guards(aaa_proto::DeployService::Host, identity).await;
        }
        agent.observe_guards(aaa_proto::DeployService::Agent, agent.deployments.runtime_identity()).await;
        agent.replay_posts().await;
        let cached = agent.health.lock().unwrap().clone();
        if let Some(h) = cached {
            agent.send_health(h).await;
        }
        Ok::<(), anyhow::Error>(())
    };
    tokio::pin!(initialize);
    let mut initialized = false;
    let mut deadline = tokio::time::Instant::now() + DEAD_AFTER;
    let mut replies = tokio::task::JoinSet::new();
    let result = async {
        loop {
            while replies.try_join_next().is_some() {}
            let next = tokio::select! {
                biased;
                initialized_result = &mut initialize, if !initialized => {
                    initialized_result?;
                    initialized = true;
                    // 인증·초기 replay를 마친 연결은 짧아도 성공이다. 연속 host 재시작 때
                    // 예전 실패의 30~45초 대기를 다시 물려주지 않는다.
                    *backoff = Duration::from_millis(250);
                    if let Some(state) = state { state.send_replace(true); }
                    continue;
                }
                next = tokio::time::timeout_at(deadline, rx.next()) => next,
            };
            deadline = tokio::time::Instant::now() + DEAD_AFTER;
            match next {
                Err(_) => bail!("{}초 동안 host 응답이 없다", DEAD_AFTER.as_secs()),
                Ok(Err(e)) => return Err(e.into()),
                Ok(Ok(None)) => return Ok(()),
                Ok(Ok(Some(Incoming::Terminal(_)))) => {}
                Ok(Ok(Some(Incoming::Control(b)))) => match serde_json::from_slice::<LinkMsg>(&b) {
                    Ok(LinkMsg::Req { id, body, deadline_ms }) => {
                        tokio::spawn(agent.clone().handle_deadline(id, body, deadline_ms, Some(tx.clone())));
                    }
                    Ok(LinkMsg::Ping { seq }) => {
                        // 느린 상대의 답 전송이 EOF·응답 상한 읽기를 막지 않게 한다.
                        let tx = tx.clone();
                        replies.spawn(async move {
                            let _ = tx.json(&LinkMsg::Pong { seq }).await;
                        });
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
    if let Some(state) = state {
        state.send_replace(false);
    }
    pinger.abort();
    replies.abort_all();
    result
}

/// `aaa agent enroll <host:port>`: 등록 창이 열린 host에 장치 키를 등록한다.
/// 양쪽이 같은 6자리 확인 코드를 계산하고, 사용자가 host에서 그 코드를 입력해 승인한다.
pub async fn enroll(state: &std::path::Path, addr: &str, machine_id: &str, name: &str) -> Result<HostPin> {
    enroll_with(state, addr, machine_id, name, |code, host_fp| {
        println!("확인 코드: {} {}", &code[..3], &code[3..]);
        println!("host 머신에서 다음을 실행해 승인한다:");
        println!("  aaa machine approve {name} --code {code}");
        println!("(host fingerprint {}…)", &host_fp[..16]);
    })
    .await
}

/// host가 등록을 거부했다(등록 창 닫힘·이름 충돌 등). `aaa setup`이 code로 다음 안내를 고른다
#[derive(Debug)]
pub struct EnrollRejected(pub aaa_proto::ErrorBody);
impl std::fmt::Display for EnrollRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "등록 거부: {}", self.0)
    }
}
impl std::error::Error for EnrollRejected {}

/// `enroll`과 같은 wire. 확인 코드(6자리)와 host fingerprint는 출력하지 않고 `pending`에 넘긴다
pub async fn enroll_with(
    state: &std::path::Path,
    addr: &str,
    machine_id: &str,
    name: &str,
    mut pending: impl FnMut(&str, &str),
) -> Result<HostPin> {
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
        installation: None,
    }))
    .await?;
    loop {
        let msg: LinkMsg = rx.control().await?;
        match msg {
            LinkMsg::EnrollPending => pending(&code, &host_fp),
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
            LinkMsg::Reject(e) => return Err(EnrollRejected(e).into()),
            other => bail!("예상하지 못한 응답: {other:?}"),
        }
    }
}

/// installer의 보호 표시 callback에만 전달한다. Debug/Serialize/일반 로그에 넣지 않는다.
pub struct InstallChallenge {
    pub code: String,
    pub host_fingerprint: String,
}
/// 검증된 설치 계획의 대상과 기존 TLS pin. 비밀 코드 표시 callback과 분리한다.
pub struct PrivateEnrollment<'a> {
    pub state: &'a std::path::Path,
    pub addr: &'a str,
    pub machine_id: &'a str,
    pub name: &'a str,
    pub binding: &'a aaa_proto::install::InstallEnrollmentBinding,
    pub pending_host: Option<&'a str>,
}
/// 설치 UI에는 wire의 기계용 code만 넘긴다. 원격 message/SAS/지문은 공개 오류에 복사하지 않는다.
#[derive(Debug)]
pub struct InstallEnrollmentRejected {
    pub code: aaa_proto::ErrorCode,
}
impl std::fmt::Display for InstallEnrollmentRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("installation enrollment rejected")
    }
}
impl std::error::Error for InstallEnrollmentRejected {}

/// 기존 수동 등록 wire를 UI 출력 없이 사용한다. code는 현재 TLS 연결 한 번에만 유효하다.
pub async fn enroll_private<F, Fut, C>(target: PrivateEnrollment<'_>, display: F, closed: C) -> Result<HostPin>
where
    F: FnOnce(InstallChallenge) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
    C: std::future::Future<Output = ()>,
{
    let PrivateEnrollment { state, addr, machine_id, name, binding, pending_host } = target;
    let id = tls::load_or_create_private(state, "aaa-agent")?;
    let pin_path = state.join("host.json");
    let existing = match pin_path.symlink_metadata() {
        Ok(_) => {
            Some(serde_json::from_slice::<HostPin>(&aaa_common::private_file::read(&pin_path, 65536)?)?.fingerprint)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let tcp = tokio::time::timeout(Duration::from_secs(10), tokio::net::TcpStream::connect(addr)).await??;
    let connector = tokio_rustls::TlsConnector::from(tls::client_config(
        &id,
        existing.or_else(|| pending_host.map(str::to_owned)),
    )?);
    let stream = connector.connect(tls::server_name(), tcp).await?;
    let (host_fp, code) = {
        let (_, connection) = stream.get_ref();
        let host = tls::peer_fingerprint(connection).context("host 인증서 없음")?;
        let code = tls::sas(connection, &host, &id.fingerprint)?;
        (host, code)
    };
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME_PREAUTH);
    let mut rx = FrameRx::new(r, MAX_FRAME_PREAUTH);
    tx.json(&LinkMsg::InstallEnrollProbe { proto: AGENT_LINK }).await?;
    match rx.control::<LinkMsg>().await? {
        LinkMsg::InstallEnrollCapabilities { proto, features } => {
            aaa_proto::negotiate(AGENT_LINK, proto)?;
            aaa_proto::require_features(&["install_enroll.v1"], &features)?;
        }
        LinkMsg::Reject(error) => return Err(InstallEnrollmentRejected { code: error.code }.into()),
        _ => bail!("host가 install_enroll.v1을 지원하지 않습니다"),
    }
    tx.json(&LinkMsg::Enroll(EnrollRequest {
        proto: AGENT_LINK,
        machine_id: machine_id.into(),
        machine_name: name.into(),
        installation: Some(binding.clone()),
    }))
    .await?;
    let mut display = Some(display);
    let mut shown = false;
    tokio::pin!(closed);
    loop {
        let message = tokio::select! {
            _=&mut closed=>bail!("설치 보호 표시 연결이 닫혔습니다"),
            message=rx.control::<LinkMsg>()=>message?,
        };
        match message {
            LinkMsg::EnrollPending => {
                let callback = display.take().context("중복된 설치 등록 challenge")?;
                callback(InstallChallenge { code: code.clone(), host_fingerprint: host_fp.clone() }).await?;
                shown = true;
            }
            LinkMsg::EnrollAccepted { machine_id: accepted } if shown && accepted == machine_id => {
                let pin =
                    HostPin { address: addr.into(), fingerprint: host_fp, enrolled_at_ms: aaa_common::util::now_ms() };
                return Ok(pin);
            }
            LinkMsg::Reject(error) => return Err(InstallEnrollmentRejected { code: error.code }.into()),
            _ => bail!("설치 수동 등록을 확인하지 못했습니다"),
        }
    }
}

/// 설치 전용 grant. caller가 신뢰한 발급자 서명을 검사한 grant만 전달한다.
/// host pin은 grant의 서명된 fingerprint이며 최초 TLS 응답으로 바꾸지 않는다.
pub async fn enroll_install(
    state: &std::path::Path,
    addr: &str,
    machine_id: &str,
    name: &str,
    grant: &aaa_proto::install::InstallEnrollmentGrant,
) -> Result<HostPin> {
    let id = tls::load_or_create_private(state, "aaa-agent")?;
    anyhow::ensure!(id.fingerprint == grant.agent_identity, "설치 grant의 agent 신원이 다릅니다");
    let pin_path = state.join("host.json");
    match pin_path.symlink_metadata() {
        Ok(_) => {
            let pin: HostPin = serde_json::from_slice(&aaa_common::private_file::read(&pin_path, 65536)?)?;
            anyhow::ensure!(pin.fingerprint == grant.host_identity, "기존 host pin이 다릅니다");
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let lookup = LinkMsg::InstallEnrollLookup {
        proto: AGENT_LINK,
        installation_id: grant.installation_id.clone(),
        plan_sha256: grant.plan_sha256.clone(),
    };
    let reply = install_exchange(&id, addr, &grant.host_identity, &lookup).await?;
    let machine = match reply {
        LinkMsg::InstallEnrolled { installation_id, machine_id } if installation_id == grant.installation_id => {
            machine_id
        }
        LinkMsg::InstallEnrollUnknown => {
            let request = LinkMsg::InstallEnroll(Box::new(aaa_proto::install::InstallEnrollRequest {
                proto: AGENT_LINK,
                machine_id: machine_id.into(),
                machine_name: name.into(),
                grant: grant.clone(),
            }));
            match install_exchange(&id, addr, &grant.host_identity, &request).await? {
                LinkMsg::InstallEnrolled { installation_id, machine_id }
                    if installation_id == grant.installation_id =>
                {
                    machine_id
                }
                _ => bail!("설치 grant 등록이 거부됐습니다"),
            }
        }
        _ => bail!("설치 등록 결과를 확인하지 못했습니다"),
    };
    anyhow::ensure!(machine == machine_id, "설치 등록의 machine ID가 다릅니다");
    let pin = HostPin {
        address: addr.into(),
        fingerprint: grant.host_identity.clone(),
        enrolled_at_ms: aaa_common::util::now_ms(),
    };
    Ok(pin)
}
/// 응답 유실 조회는 저장된 이전 TLS fingerprint로만 한다. 서명 입력이나 첫 응답으로 새 pin을 만들지 않는다.
pub async fn lookup_install(
    state: &std::path::Path,
    addr: &str,
    machine_id: &str,
    binding: &aaa_proto::install::InstallEnrollmentBinding,
    host_fingerprint: &str,
) -> Result<Option<HostPin>> {
    let id = tls::load_or_create_private(state, "aaa-agent")?;
    let request = LinkMsg::InstallEnrollLookup {
        proto: AGENT_LINK,
        installation_id: binding.installation_id.clone(),
        plan_sha256: binding.plan_sha256.clone(),
    };
    match install_exchange(&id, addr, host_fingerprint, &request).await? {
        LinkMsg::InstallEnrolled { installation_id, machine_id: actual }
            if installation_id == binding.installation_id && actual == machine_id =>
        {
            Ok(Some(HostPin {
                address: addr.into(),
                fingerprint: host_fingerprint.into(),
                enrolled_at_ms: aaa_common::util::now_ms(),
            }))
        }
        LinkMsg::InstallEnrollUnknown => Ok(None),
        _ => bail!("설치 등록 조회의 대상이 다릅니다"),
    }
}
/// 등록된 같은 TLS key / machine ID / name만 읽는다. 신뢰를 추가·복구하지 않는다.
pub async fn lookup_agent(
    state: &std::path::Path,
    addr: &str,
    machine_id: &str,
    name: &str,
    host_fingerprint: &str,
) -> Result<Option<HostPin>> {
    let id = tls::load_or_create_private(state, "aaa-agent")?;
    let request =
        LinkMsg::InstallAgentQuery { proto: AGENT_LINK, machine_id: machine_id.into(), machine_name: name.into() };
    match install_exchange(&id, addr, host_fingerprint, &request).await? {
        LinkMsg::InstallAgentKnown { machine_id: actual } if actual == machine_id => Ok(Some(HostPin {
            address: addr.into(),
            fingerprint: host_fingerprint.into(),
            enrolled_at_ms: aaa_common::util::now_ms(),
        })),
        LinkMsg::InstallEnrollUnknown => Ok(None),
        _ => bail!("기존 agent 조회의 대상이 다릅니다"),
    }
}
async fn install_exchange(id: &tls::Identity, addr: &str, pin: &str, request: &LinkMsg) -> Result<LinkMsg> {
    // 요청을 재시도하지 않는다. lookup과 consume은 별개의 protocol 단계다.
    tokio::time::timeout(Duration::from_secs(20), async {
        let tcp = tokio::net::TcpStream::connect(addr).await?;
        let connector = tokio_rustls::TlsConnector::from(tls::client_config(id, Some(pin.into()))?);
        let stream = connector.connect(tls::server_name(), tcp).await?;
        let (r, w) = tokio::io::split(stream);
        let tx = spawn_writer(w, MAX_FRAME_PREAUTH);
        let mut rx = FrameRx::new(r, MAX_FRAME_PREAUTH);
        tx.json(&LinkMsg::InstallEnrollProbe { proto: AGENT_LINK }).await?;
        match rx.control::<LinkMsg>().await? {
            LinkMsg::InstallEnrollCapabilities { proto, features } => {
                aaa_proto::negotiate(AGENT_LINK, proto)?;
                aaa_proto::require_features(&["install_enroll.v1"], &features)?;
            }
            _ => bail!("host가 install_enroll.v1을 지원하지 않습니다"),
        }
        tx.json(request).await?;
        rx.control().await
    })
    .await
    .context("설치 등록 응답 시간 초과")?
}

#[cfg(test)]
mod post_tests {
    use super::*;
    use aaa_common::util;
    use aaa_proto::{HostWelcome, PostInput, PostType};

    /// 실제 Welcome/replay 완료 사건으로 성공을 구분한다. 10초를 자거나 시계를 늘리지 않는다.
    #[tokio::test]
    async fn authenticated_short_connections_reset_backoff_but_rejections_preserve_it() {
        for accepted in [false, true] {
            let path = std::env::temp_dir().join(format!("aap-reconnect-{}", util::new_id()));
            let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
            let (stream, peer) = tokio::io::duplex(65536);
            let (state, mut ready) = tokio::sync::watch::channel(false);
            let running = tokio::spawn({
                let agent = agent.clone();
                async move {
                    let mut backoff = Duration::from_secs(30);
                    let result = session_state(&agent, Box::new(stream), Some(&state), &mut backoff).await;
                    (result, backoff)
                }
            });
            let (r, w) = tokio::io::split(peer);
            let tx = spawn_writer(w, MAX_FRAME);
            let mut rx = FrameRx::new(r, MAX_FRAME);
            assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Hello(_)));
            if accepted {
                tx.json(&LinkMsg::Welcome(HostWelcome {
                    identity: None,
                    proto: AGENT_LINK,
                    host_id: "host".into(),
                    features: vec![],
                }))
                .await
                .unwrap();
                drop(
                    tokio::time::timeout(Duration::from_secs(3), ready.wait_for(|ready| *ready))
                        .await
                        .unwrap()
                        .unwrap(),
                );
            }
            tx.json(&LinkMsg::Reject(aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::Forbidden, "fixture closed")))
                .await
                .unwrap();
            let (result, backoff) = tokio::time::timeout(Duration::from_secs(3), running).await.unwrap().unwrap();
            assert!(result.is_err());
            assert!(!*ready.borrow());
            assert_eq!(backoff, if accepted { Duration::from_millis(250) } else { Duration::from_secs(30) });
            drop(tx);
            drop(rx);
            drop(agent);
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    /// Hello를 읽은 뒤 Welcome을 보류한다. 그 사이 화면 감지가 기억한 idle은 연결 뒤 반드시 전달되고,
    /// Hello를 읽는 host에는 이미 받은 진행 사건을 중복해서 보내지 않는다. Ping/Pong이 전달 완료 울타리다.
    #[tokio::test]
    async fn harness_changes_while_waiting_for_welcome_reach_the_host() {
        use aaa_proto::{AgentEvent, HarnessEvent};
        for reads_hello in [true, false] {
            let path = std::env::temp_dir().join(format!("aap-link-{}", util::new_id()));
            let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
            util::ensure_private_dir(&path.join("sessions/s")).unwrap();
            let event = |kind: &str, at_ms| HarnessEvent {
                session: "s".into(),
                harness: "claude".into(),
                kind: kind.into(),
                harness_session: None,
                message: None,
                question_kind: None,
                percent: None,
                at_ms,
                transcript: None,
                plan: None,
                background: None,
                context: None,
                model_observation: None,
            };
            let working = event("turn_start", 1);
            let mut report = event("progress", 2);
            report.harness = "report".into();
            agent.harness_event(working.clone()).await;
            agent.harness_event(report.clone()).await;
            let (stream, peer) = tokio::io::duplex(1 << 20);
            let running = tokio::spawn({
                let agent = agent.clone();
                async move { session_state(&agent, Box::new(stream), None, &mut Duration::from_millis(250)).await }
            });
            let (r, w) = tokio::io::split(peer);
            let tx = spawn_writer(w, MAX_FRAME);
            let mut rx = FrameRx::new(r, MAX_FRAME);
            let LinkMsg::Hello(h) = rx.control().await.unwrap() else { panic!("hello가 없다") };
            assert_eq!(h.harness, vec![working, report.clone()]);
            // 실제 화면 감지 진입점을 마친 뒤에만 Welcome을 보낸다. 부하나 sleep에 기대지 않는다.
            let idle = event("idle", 3);
            agent.harness_event(idle.clone()).await;
            tx.json(&LinkMsg::Welcome(HostWelcome {
                identity: None,
                proto: AGENT_LINK,
                host_id: "host".into(),
                features: if reads_hello { vec!["harness_hello.v1".into()] } else { vec![] },
            }))
            .await
            .unwrap();
            tx.json(&LinkMsg::Ping { seq: 42 }).await.unwrap();
            let mut received = vec![];
            loop {
                match tokio::time::timeout(Duration::from_secs(3), rx.control::<LinkMsg>()).await.unwrap().unwrap() {
                    LinkMsg::Event(AgentEvent::Harness(ev)) => received.push(ev),
                    LinkMsg::Pong { seq: 42 } => break,
                    other => panic!("예상하지 못한 사건: {other:?}"),
                }
            }
            let expected = if reads_hello { vec![idle] } else { vec![report, idle] };
            assert_eq!(received, expected, "Hello와 Welcome 사이의 idle이 빠졌다(reads_hello={reads_hello})");
            // 연결 뒤 같은 밀리초에 바뀐 상태도 replay보다 뒤에 전달된다.
            let live = event("working", 3);
            agent.harness_event(live.clone()).await;
            assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Event(AgentEvent::Harness(live)));
            drop(tx);
            drop(rx);
            running.await.unwrap().unwrap();
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    /// 작은 실제 전송 버퍼와 여러 pane의 변경분으로 replay를 멈춘 뒤 live hook을 함께 진행한다.
    /// hook은 기억하지 않는 prompt_submit이라 replay 뒤 live 전송이 열리기 전에 실행되면 영원히 빠진다.
    #[tokio::test]
    async fn live_hook_waits_for_the_replay_to_finish() {
        use aaa_proto::{AgentEvent, HarnessEvent};
        for close_during_replay in [false, true] {
            let path = std::env::temp_dir().join(format!("aap-link-{}", util::new_id()));
            let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
            let (stream, peer) = tokio::io::duplex(64);
            let running = tokio::spawn({
                let agent = agent.clone();
                async move { session_state(&agent, Box::new(stream), None, &mut Duration::from_millis(250)).await }
            });
            let (r, w) = tokio::io::split(peer);
            let tx = spawn_writer(w, MAX_FRAME);
            let mut rx = FrameRx::new(r, MAX_FRAME);
            let LinkMsg::Hello(h) = rx.control().await.unwrap() else { panic!("Hello가 없다") };
            assert!(h.harness.is_empty());
            let event = |session: String, kind: &str| HarnessEvent {
                session,
                harness: "claude".into(),
                kind: kind.into(),
                harness_session: None,
                message: None,
                question_kind: None,
                percent: None,
                at_ms: 3,
                transcript: None,
                plan: None,
                background: None,
                context: None,
                model_observation: None,
            };
            // 전송 큐보다 많은 기억을 입력 fixture로 둔다. 디스크 저장은 위 회귀·실제 E2E가 맡고,
            // 이 owner는 전송 backpressure와 live hook의 순서만 본다.
            *agent.harness_last.lock().unwrap() = (0..600)
                .map(|n| {
                    let id = format!("s{n}");
                    (id.clone(), vec![event(id, "idle")])
                })
                .collect();
            tx.json(&LinkMsg::Welcome(HostWelcome {
                identity: None,
                proto: AGENT_LINK,
                host_id: "host".into(),
                features: vec!["harness_hello.v1".into()],
            }))
            .await
            .unwrap();
            let first = rx.control::<LinkMsg>().await.unwrap();
            assert!(matches!(first, LinkMsg::Event(AgentEvent::Harness(_))));
            if close_during_replay {
                // Ping 답도 full 큐에서 기다린다. write half만 닫고 읽기 half와 큐는 그대로 둔다.
                tx.json(&LinkMsg::Ping { seq: 99 }).await.unwrap();
                drop(tx);
                tokio::time::timeout(Duration::from_secs(3), running)
                    .await
                    .expect("replay 중 EOF를 읽지 못했다")
                    .unwrap()
                    .unwrap();
                drop(rx);
                std::fs::remove_dir_all(path).unwrap();
                continue;
            }
            let live = event("s0".into(), "prompt_submit");
            let (_, _, received) = tokio::time::timeout(Duration::from_secs(3), async {
                tokio::join! {
                    biased;
                    // 실제 요청 owner를 replay drain보다 먼저 진행한다. NotFound 응답도 잃으면 안 된다.
                    agent.clone().handle("screen".into(), aaa_proto::AgentRequest::Screen { session: "missing".into() }),
                    agent.harness_event(live.clone()),
                    async {
                        let mut received = vec![first];
                        while received.len() < 602 {
                            received.push(rx.control::<LinkMsg>().await.unwrap());
                        }
                        received
                    }
                }
            })
            .await
            .expect("replay 도중 Screen 응답 또는 live hook이 빠졌다");
            assert!(
                received.iter().any(|m| matches!(m, LinkMsg::Res { id, result: aaa_proto::Outcome::Err(e) }
                if id == "screen" && e.code == aaa_proto::ErrorCode::NotFound)),
                "{received:?}"
            );
            assert_eq!(
                received.last(),
                Some(&LinkMsg::Event(AgentEvent::Harness(live))),
                "replay 도중 live hook이 앞질렀다"
            );
            drop(tx);
            drop(rx);
            running.await.unwrap().unwrap();
            std::fs::remove_dir_all(path).unwrap();
        }
    }

    #[tokio::test]
    async fn post_feature_is_negotiated_and_old_host_receives_no_post() {
        let path = std::env::temp_dir().join(format!("aap-link-{}", util::new_id()));
        let agent = Agent::new(path.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        util::ensure_private_dir(&path.join("post/out")).unwrap();
        let input = PostInput {
            post_id: format!("{:032x}", 1),
            to: "coordinator".into(),
            post_type: PostType::Data,
            topic: "ci-red".into(),
            title: "기록".into(),
            body: "본문".into(),
            needs_user: false,
            refs: vec![],
        };
        util::write_json(
            &path.join("post/out").join(format!("{}.json", input.post_id)),
            &crate::post::Pending { session: "s".into(), input },
        )
        .unwrap();
        let (stream, peer) = tokio::io::duplex(1 << 20);
        let (state, mut observed_ready) = tokio::sync::watch::channel(false);
        let running = tokio::spawn({
            let a = agent.clone();
            async move { session_state(&a, Box::new(stream), Some(&state), &mut Duration::from_millis(250)).await }
        });
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        let LinkMsg::Hello(h) = rx.control().await.unwrap() else { panic!("hello가 없다") };
        assert!(h.features.iter().any(|f| f == "post.v1"));
        assert!(!*observed_ready.borrow());
        tx.json(&LinkMsg::Welcome(HostWelcome {
            identity: None,
            proto: AGENT_LINK,
            host_id: "old".into(),
            features: vec!["terminal.v1".into()],
        }))
        .await
        .unwrap();
        drop(
            tokio::time::timeout(Duration::from_secs(3), observed_ready.wait_for(|ready| *ready))
                .await
                .unwrap()
                .unwrap(),
        );
        // Welcome 뒤 Post replay를 먼저 하고 Ping을 읽는다. Pong 앞에 모르는 post가 없어야 한다.
        tx.json(&LinkMsg::Ping { seq: 42 }).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), rx.control::<LinkMsg>()).await.unwrap().unwrap(),
            LinkMsg::Pong { seq: 42 }
        );
        assert_eq!(*agent.host_post.lock().unwrap(), Some(false));
        drop(tx);
        drop(rx);
        running.await.unwrap().unwrap();
        std::fs::remove_dir_all(path).unwrap();
    }
}
