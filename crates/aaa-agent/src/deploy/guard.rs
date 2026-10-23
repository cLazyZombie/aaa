//! agent/host 교체와 별개인 옛 이미지 프로세스. 실행 허가는 agent가 띄운 PID에만 한 번 보낸다.
use super::*;
use aaa_common::{
    conn::{FrameRx, spawn_writer},
    ipc::LocalListener,
    util,
};
use aaa_proto::{
    AgentRequest, AgentResponse, DeployGuardControl as Control, DeployGuardPhase as Phase, DeployGuardSpec,
    DeployGuardView, RuntimeIdentity,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    process::{Command, Stdio},
    time::Duration,
};

pub const WINDOW_MS: u64 = 120_000;
/// 설치 파일이 감시의 배포 전 SHA(지금 실행 판)와 다르다: 개별 update·rollback이 바꾼 뒤다. host는 재시작 rollout에서
/// 이 답(`conflict`)을 받으면 감시 없이 그 파일을 띄우는 개별 재시작으로 보낸다(W6c)
#[derive(Debug)]
struct InstalledChanged;
impl std::fmt::Display for InstalledChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("설치 파일이 지금 실행 판과 다르다(복구 사본을 만들 배포 전 SHA가 아니다)")
    }
}
impl std::error::Error for InstalledChanged {}
pub type RecoveryExecutor = fn(&Path, DeployAction, &Path) -> Result<String, String>;
#[derive(Serialize, Deserialize)]
struct Bootstrap {
    parent: u32,
    view: DeployGuardView,
}
#[derive(Serialize, Deserialize)]
struct AgentAuth {
    pid: u32,
    token: String,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    token: String,
    control: Control,
}
pub(super) fn write_identity(root: &Path) -> Result<()> {
    aaa_config::profile_edit::write_atomic(
        &root.join("executor.identity.json"),
        &serde_json::to_vec(&AgentAuth { pid: std::process::id(), token: util::new_id() })?,
    )?;
    Ok(())
}
fn socket(state: &Path, id: &str, prefix: &str) -> PathBuf {
    state.join("run").join(format!("{prefix}{id}.sock"))
}
fn record(state: &Path, id: &str) -> Result<PathBuf> {
    if !valid(id) {
        bail!("guard ID 형식이 아니다");
    }
    Ok(state.join("deployments").join(format!("{id}.guard.json")))
}
pub(crate) fn read(state: &Path, id: &str) -> Result<DeployGuardView> {
    util::read_json(&record(state, id)?)
}
fn save(state: &Path, view: &DeployGuardView) -> Result<()> {
    aaa_config::profile_edit::write_atomic(&record(state, &view.spec.id)?, &serde_json::to_vec(view)?)?;
    Ok(())
}
fn agent_pid(state: &Path) -> Result<u32> {
    Ok(agent_auth(state)?.pid)
}
fn agent_auth(state: &Path) -> Result<AgentAuth> {
    util::read_json(&state.join("deployments/executor.identity.json"))
}
fn views(state: &Path) -> Vec<DeployGuardView> {
    std::fs::read_dir(state.join("deployments"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".guard.json").map(str::to_owned))
        .filter_map(|id| read(state, &id).ok())
        .collect()
}
pub(super) fn for_install(state: &Path, job: &str) -> Option<DeployGuardView> {
    views(state).into_iter().find(|v| v.spec.install_job.as_deref() == Some(job))
}
pub(super) async fn rpc(state: &Path, id: &str, control: Control) -> Result<DeployGuardView> {
    record(state, id)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        let stream = aaa_common::ipc::connect(&socket(state, id, "g")).await?;
        let (r, w) = tokio::io::split(stream);
        let tx = spawn_writer(w, aaa_proto::MAX_FRAME);
        let mut rx = FrameRx::new(r, aaa_proto::MAX_FRAME);
        tx.json(&Envelope { token: agent_auth(state)?.token, control }).await?;
        rx.control::<Result<DeployGuardView, String>>().await?.map_err(anyhow::Error::msg)
    })
    .await
    .context("guard 응답 한도")?
}
impl Agent {
    pub(crate) async fn guard_request(self: &Arc<Self>, req: AgentRequest) -> Result<AgentResponse, ErrorBody> {
        let result = match req {
            AgentRequest::DeployGuardArm { spec } => self.arm_guard(spec).await,
            AgentRequest::DeployGuardControl { id, control } => {
                let terminal_read =
                    matches!(control, Control::Status | Control::Cancel | Control::Commit | Control::Recover { .. });
                match read(&self.state, &id) {
                    Ok(view) if view.phase.terminal() && terminal_read => Ok(view),
                    Ok(_) => match rpc(&self.state, &id, control).await {
                        Ok(view) => Ok(view),
                        Err(e) => match read(&self.state, &id) {
                            // terminal commit 뒤 worker가 끝나는 것과 Status가 경주해도 재실행하지 않고 내구 영수증을 읽는다.
                            Ok(view) if view.phase.terminal() && terminal_read => Ok(view),
                            _ => Err(e),
                        },
                    },
                    Err(e) => Err(e),
                }
            }
            _ => unreachable!(),
        };
        result.map(|guard| AgentResponse::DeployGuard { guard: Box::new(guard) }).map_err(|e| {
            if e.downcast_ref::<tokio::time::error::Elapsed>().is_some() {
                ErrorBody::new(ErrorCode::Timeout, e.to_string())
            } else if e.downcast_ref::<InstalledChanged>().is_some() {
                ErrorBody::new(ErrorCode::Conflict, e.to_string())
            } else {
                error(e)
            }
        })
    }
    async fn arm_guard(self: &Arc<Self>, spec: DeployGuardSpec) -> Result<DeployGuardView> {
        let _gate = self.guard_gate.lock().await;
        for id in [&spec.id, &spec.rollout, &spec.rollback_job, &spec.restart_job, &spec.host_restart_job] {
            if !valid(id) {
                bail!("guard 작업 ID 형식이 아니다");
            }
        }
        if spec.install_job.as_ref().is_some_and(|id| !valid(id)) {
            bail!("설치 작업 ID 형식이 아니다");
        }
        for sha in [&spec.baseline.sha256, &spec.expected_sha256] {
            if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit()) {
                bail!("guard SHA-256 형식이 아니다");
            }
        }
        if spec.baseline.instance.is_empty()
            || !spec.baseline.software.starts_with("aaa ")
            || !spec.expected_software.starts_with("aaa ")
        {
            bail!("guard 실행 신원이 없다");
        }
        if views(&self.state).iter().any(|old| old.spec.rollout != spec.rollout && !old.phase.terminal()) {
            bail!("다른 rollout의 독립 복구 감시가 진행 중이다");
        }
        let pin = self.state.join("deployments").join(format!("{}.guard.bin", spec.rollout));
        let view = if record(&self.state, &spec.id)?.exists() {
            let old = read(&self.state, &spec.id)?;
            if old.spec != spec {
                bail!("같은 guard ID를 다른 요청에 쓸 수 없다");
            }
            if old.phase.terminal() {
                return Ok(old);
            }
            if let Ok(live) = rpc(&self.state, &spec.id, Control::Status).await {
                return Ok(live);
            }
            let lock = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(self.state.join("deployments").join(format!("{}.guard.lock", spec.id)))?;
            match lock.try_lock() {
                Ok(()) => {
                    lock.unlock()?;
                }
                Err(std::fs::TryLockError::WouldBlock) => return read(&self.state, &spec.id),
                Err(e) => return Err(e.into()),
            }
            old
        } else {
            if !pin.exists() {
                if sha(&self.exe).map_err(|e| anyhow::anyhow!(e.message))? != spec.baseline.sha256 {
                    return Err(InstalledChanged.into());
                }
                std::fs::copy(&self.exe, &pin)?;
                // Windows FlushFileBuffers는 쓰기 핸들이 필요하다. 읽기 전용 File::open은 오류 5다.
                let file = std::fs::OpenOptions::new().write(true).open(&pin).context("복구 사본 동기화 핸들")?;
                file.sync_all().context("복구 사본 내구 동기화")?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&pin, std::fs::Permissions::from_mode(0o700))?;
                }
            }
            let view = DeployGuardView {
                spec: spec.clone(),
                phase: Phase::Armed,
                worker_pid: 0,
                deadline_ms: util::now_ms() + WINDOW_MS,
                install_started: false,
                activated: false,
                restore_started: false,
                restart_index: 0,
                restart_started: false,
                host_restart_ms: 0,
                agent_restart_ms: 0,
                observed: vec![],
                installed_sha256: String::new(),
                error: None,
                pending_web: None,
            };
            save(&self.state, &view)?;
            view
        };
        let target = self.state.join("deployments").join(format!("{}.guard.target", spec.id));
        if target.exists() {
            let old: PathBuf = util::read_json(&target)?;
            if old != self.exe {
                bail!("같은 guard의 설치 경로가 바뀌었다");
            }
        } else {
            aaa_config::profile_edit::write_atomic(&target, &serde_json::to_vec(&self.exe)?)?;
        }
        if sha(&pin).map_err(|e| anyhow::anyhow!(e.message))? != spec.baseline.sha256 {
            bail!("복구 실행기 사본 SHA 불일치");
        }
        // 새 baseline 사본이 준비된 뒤에만 지난 terminal guard의 사본을 회수한다.
        let previous: Vec<_> = views(&self.state).into_iter().filter(|v| v.spec.rollout != spec.rollout).collect();
        if previous.iter().any(|old| !old.phase.terminal()) {
            bail!("다른 rollout의 독립 복구 감시가 진행 중이다");
        }
        for old in previous {
            for path in [
                record(&self.state, &old.spec.id)?,
                self.state.join("deployments").join(format!("{}.guard.target", old.spec.id)),
                self.state.join("deployments").join(format!("{}.guard.lock", old.spec.id)),
                self.state.join("deployments").join(format!("{}.guard.bin", old.spec.rollout)),
            ] {
                match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
            }
        }
        let boot = LocalListener::bind(&socket(&self.state, &spec.id, "b"))?;
        let log = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.state.join("deployments").join(format!("{}.guard.log", spec.id)))?;
        let mut command = Command::new(&pin);
        command
            .arg("--state-dir")
            .arg(self.state.parent().context("agent 상태 상위 폴더")?)
            .args(["deployment-guard", "--id", &spec.id])
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            use windows_sys::Win32::System::Threading::{
                CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS,
            };
            // 서비스 종료·부모 Job 수명과 분리한다. breakaway가 금지되면 세우기 단계에서 실패하며 교체하지 않는다.
            command.creation_flags(CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
        }
        let mut child = command.spawn().context("독립 복구 감시 실행")?;
        let child_pid = child.id();
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        let result = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let (stream, pid) = boot.accept_peer().await?;
                if pid != Some(child_pid) {
                    continue;
                }
                let (r, w) = tokio::io::split(stream);
                let tx = spawn_writer(w, aaa_proto::MAX_FRAME);
                let mut rx = FrameRx::new(r, aaa_proto::MAX_FRAME);
                tx.json(&Bootstrap { parent: std::process::id(), view: view.clone() }).await?;
                return rx.control::<DeployGuardView>().await;
            }
        })
        .await
        .context("감시 준비 한도")??;
        if result.spec != spec || result.worker_pid != child_pid {
            bail!("감시 준비 신원 불일치");
        }
        Ok(result)
    }
    pub(crate) fn resume_guards(self: &Arc<Self>) {
        let agent = self.clone();
        tokio::spawn(async move {
            for view in views(&agent.state).into_iter().filter(|v| !v.phase.terminal()) {
                if let Err(e) = agent.arm_guard(view.spec).await {
                    tracing::error!("복구 감시 재개: {e:#}");
                }
            }
        });
    }
    pub(crate) async fn observe_guards(&self, service: DeployService, identity: RuntimeIdentity) {
        for view in views(&self.state).into_iter().filter(|v| !v.phase.terminal()) {
            if let Err(e) =
                rpc(&self.state, &view.spec.id, Control::Observe { service, identity: identity.clone() }).await
            {
                tracing::warn!("복구 감시 실행 신원 전달: {e:#}");
            }
        }
    }
}

fn begin_restore(view: &mut DeployGuardView, reason: String, now: u64) {
    view.pending_web = None;
    view.phase = Phase::Restoring;
    view.error = Some(reason);
    view.deadline_ms = now + WINDOW_MS;
}
fn restart_failed(view: &mut DeployGuardView, error: String, now: u64) {
    // SCM 응답보다 새 프로세스의 신원 확인·Commit이 먼저 끝날 수 있다. 확정 영수증은 되돌리지 않는다.
    match view.phase {
        Phase::AwaitingNew | Phase::NewReady => begin_restore(view, error, now),
        Phase::Restarting => {
            view.phase = Phase::Failed;
            view.error = Some(error);
        }
        _ => {}
    }
}
fn apply(view: &mut DeployGuardView, control: Control, now: u64) -> Result<()> {
    match control {
        Control::Status => {}
        Control::BeginInstall { job_id }
            if view.phase == Phase::Armed && view.spec.install_job.as_deref() == Some(&job_id) =>
        {
            view.install_started = true;
        }
        Control::Activate if view.phase == Phase::Armed => {
            view.activated = true;
            view.phase = Phase::AwaitingNew;
            view.deadline_ms = now + WINDOW_MS;
        }
        Control::Activate if matches!(view.phase, Phase::AwaitingNew | Phase::NewReady | Phase::VerifiedNew) => {}
        Control::Commit if view.phase == Phase::NewReady && view.installed_sha256 == view.spec.expected_sha256 => {
            view.phase = Phase::VerifiedNew
        }
        Control::Commit if view.phase == Phase::VerifiedNew => {}
        Control::Recover { reason } if matches!(view.phase, Phase::Armed | Phase::AwaitingNew | Phase::NewReady) => {
            begin_restore(view, reason, now)
        }
        Control::Cancel if view.phase == Phase::Armed => {
            if untouched(view) {
                view.phase = Phase::Cancelled;
            } else {
                begin_restore(view, "취소 때 설치 파일이 바뀌어 배포 전 SHA를 복구한다".into(), now);
            }
        }
        Control::Recover { .. }
            if matches!(
                view.phase,
                Phase::Restoring | Phase::Restarting | Phase::AwaitingOld | Phase::Restored | Phase::Failed
            ) => {}
        Control::Cancel if view.phase.terminal() => {}
        Control::Observe { service, identity } => {
            let old_instance = if service == DeployService::Host {
                &view.spec.old_host_instance
            } else {
                &view.spec.baseline.instance
            };
            if identity.instance.is_empty() || &identity.instance == old_instance {
                return Ok(());
            }
            if matches!(view.phase, Phase::AwaitingNew | Phase::NewReady)
                && service == view.spec.service
                && identity.started_ms >= view.deadline_ms.saturating_sub(WINDOW_MS)
            {
                if identity.sha256 == view.spec.expected_sha256 && identity.software == view.spec.expected_software {
                    // host는 웹도 답해야 준비다(W6c). 웹 주소가 없으면(웹을 끈 설정·옛 host) 실행 신원만 본다
                    match identity.web.filter(|w| service == DeployService::Host && !w.is_empty()) {
                        Some(web) if view.phase == Phase::AwaitingNew => view.pending_web = Some(web),
                        Some(_) => {}
                        None => view.phase = Phase::NewReady,
                    }
                } else {
                    begin_restore(view, "재시작 뒤 옛 판/다른 SHA가 돌아왔다".into(), now);
                }
            } else if matches!(view.phase, Phase::Restarting | Phase::AwaitingOld)
                && identity.started_ms
                    >= if service == DeployService::Host { view.host_restart_ms } else { view.agent_restart_ms }
                && (if service == DeployService::Host { view.host_restart_ms } else { view.agent_restart_ms }) != 0
                && identity.sha256 == view.spec.baseline.sha256
                && identity.software == view.spec.baseline.software
            {
                if !view.observed.contains(&service) {
                    view.observed.push(service);
                }
                let both = view.spec.host_machine && view.spec.service == DeployService::Agent;
                if view.observed.contains(&view.spec.service) && (!both || view.observed.contains(&DeployService::Host))
                {
                    view.phase = Phase::Restored;
                }
            }
        }
        _ => bail!("이 감시 단계에서는 허용하지 않는 전이"),
    }
    Ok(())
}
/// 기다리던 새 host의 웹이 답했다. 시한은 늘리지 않는다
fn web_answered(view: &mut DeployGuardView) {
    if view.phase == Phase::AwaitingNew && view.pending_web.take().is_some() {
        view.phase = Phase::NewReady;
    }
}
/// 띄우기 전 감시가 되돌릴 것이 없다: 설치를 시작하지 않았고, 설치 파일이 배포 전 그대로이거나 기대 판이 배포 전 판이다.
/// 기대 판이 배포 전 판인 감시(재시작만 하는 rollout)는 아무것도 설치하지 않으니, 그 사이 바뀐 설치 파일은 이 rollout이 한
/// 일이 아니다(개별 update·rollback을 되돌리지 않는다)
fn untouched(view: &DeployGuardView) -> bool {
    !view.install_started
        && (view.installed_sha256 == view.spec.baseline.sha256
            || view.spec.expected_sha256 == view.spec.baseline.sha256)
}
fn expired(view: &mut DeployGuardView, now: u64) {
    if now < view.deadline_ms || view.phase.terminal() {
        return;
    }
    match view.phase {
        Phase::Armed if untouched(view) => view.phase = Phase::Cancelled,
        Phase::Armed | Phase::AwaitingNew | Phase::NewReady => {
            begin_restore(view, "새 실행 판을 한도 안에 확인하지 못했다".into(), now)
        }
        _ => {
            view.phase = Phase::Failed;
            view.error = Some("복구 실행/옛 실행 판 검증 한도 초과: 사람이 확인한다".into());
        }
    }
}
async fn publish(state: &Path, view: &DeployGuardView) -> Result<()> {
    if read(state, &view.spec.id).is_ok_and(|old| old == *view) {
        return Ok(());
    }
    save(state, view)?;
    // 상태 파일 commit 뒤에만 알린다. agent가 내려가도 기록과 복구 프로세스는 남는다.
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut s = aaa_common::ipc::connect(&crate::hooks::socket(state)).await?;
        let msg = crate::hooks::HookMsg {
            session: view.spec.id.clone(),
            token: String::new(),
            harness: "deploy_guard".into(),
            payload: serde_json::json!({"id":view.spec.id}),
        };
        s.write_all(&serde_json::to_vec(&msg)?).await?;
        s.shutdown().await?;
        let mut reply = Vec::new();
        s.read_to_end(&mut reply).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await;
    Ok(())
}

type Peer = std::io::Result<(aaa_common::ipc::LocalStream, Option<u32>)>;
struct Controls {
    rx: tokio::sync::mpsc::Receiver<Peer>,
    accept: tokio::task::JoinHandle<()>,
}
impl Controls {
    fn new(listener: LocalListener) -> Self {
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let accept = tokio::spawn(async move {
            loop {
                let peer = listener.accept_peer().await;
                let failed = peer.is_err();
                if tx.send(peer).await.is_err() || failed {
                    break;
                }
            }
        });
        Self { rx, accept }
    }
}
impl Drop for Controls {
    fn drop(&mut self) {
        self.accept.abort();
    }
}
async fn handle_control(
    (stream, pid): (aaa_common::ipc::LocalStream, Option<u32>),
    state: &Path,
    exe: &Path,
    view: &mut DeployGuardView,
) -> Result<()> {
    if pid.is_none() || pid != agent_pid(state).ok() {
        return Ok(());
    }
    view.installed_sha256 = sha(exe).unwrap_or_default();
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, aaa_proto::MAX_FRAME);
    let mut rx = FrameRx::new(r, aaa_proto::MAX_FRAME);
    let response = match tokio::time::timeout(Duration::from_secs(2), rx.control::<Envelope>()).await {
        Ok(Ok(envelope)) if envelope.token == agent_auth(state)?.token => {
            apply(view, envelope.control, util::now_ms()).map(|_| view.clone()).map_err(|e| e.to_string())
        }
        _ => Err("감시 제어 frame을 읽지 못했다".into()),
    };
    publish(state, view).await?;
    let _ = tx.json(&response).await;
    Ok(())
}
/// 파일 복구·SCM 중지/시작을 기다리는 동안에도 Status·Observe·Recover를 처리한다.
/// Windows listener는 별도 accept task가 즉시 다음 pipe instance를 준비하므로 작업 시간만큼 BUSY가 남지 않는다.
async fn during<F: std::future::Future>(
    controls: &mut Controls,
    state: &Path,
    exe: &Path,
    view: &mut DeployGuardView,
    operation: F,
) -> Result<F::Output> {
    tokio::pin!(operation);
    loop {
        tokio::select! {
            result = &mut operation => return Ok(result),
            peer = controls.rx.recv() => {
                handle_control(peer.context("감시 listener 종료")??, state, exe, view).await?;
            }
        }
    }
}

/// CLI 내부 진입점. 임의 spec/실행 경로를 받지 않고 agent가 띄운 PID에 보낸 grant만 받는다.
pub async fn worker(id: &str, execute: RecoveryExecutor) -> Result<()> {
    let state = aaa_config::paths::state_dir().join("agent");
    record(&state, id)?;
    let parent = aaa_common::process::parent(std::process::id()).context("복구 실행기의 부모 PID")?;
    let stream = aaa_common::ipc::connect(&socket(&state, id, "b")).await.context("승인한 agent의 bootstrap이 없다")?;
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, aaa_proto::MAX_FRAME);
    let mut rx = FrameRx::new(r, aaa_proto::MAX_FRAME);
    let grant = tokio::time::timeout(Duration::from_secs(10), rx.control::<Bootstrap>()).await??;
    if grant.parent != parent || agent_pid(&state)? != parent || grant.view.spec.id != id {
        bail!("agent 부모/guard 승인 불일치");
    }
    let pin = state.join("deployments").join(format!("{}.guard.bin", grant.view.spec.rollout));
    if std::env::current_exe()?.canonicalize()? != pin.canonicalize()?
        || sha(&pin).map_err(|e| anyhow::anyhow!(e.message))? != grant.view.spec.baseline.sha256
    {
        bail!("승인한 옛 실행기 사본이 아니다");
    }
    let lease = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(state.join("deployments").join(format!("{id}.guard.lock")))?;
    lease.try_lock().context("같은 guard 실행기가 이미 살아 있다")?;
    let _lease = Lease(lease);
    let mut controls = Controls::new(LocalListener::bind(&socket(&state, id, "g"))?);
    let mut view = read(&state, id)?;
    if view.spec != grant.view.spec {
        bail!("감시 정본이 승인과 다르다");
    }
    view.worker_pid = std::process::id();
    save(&state, &view)?;
    tx.json(&view).await?;
    let exe_path = pin.parent().context("사본 상위")?;
    // 설치 대상은 agent가 bootstrap에 덧붙인 별도 로컬 기록이다. 원격 문자열은 실행 경로가 되지 않는다.
    let exe: PathBuf = util::read_json(&exe_path.join(format!("{id}.guard.target")))?;
    // 새 host 웹 묻기(W6c). 루프 회전마다 새로 시작하면 잦은 상태 조회에 한 번의 물음도 끝나지 못하므로 따로 띄워 둔다
    let mut probe: Option<(String, tokio::task::JoinHandle<()>)> = None;
    loop {
        view.installed_sha256 = sha(&exe).map(|s| s.to_string()).unwrap_or_default();
        expired(&mut view, util::now_ms());
        publish(&state, &view).await?;
        if view.phase.terminal() {
            return Ok(());
        }
        #[cfg(windows)]
        if start_windows_restart(&mut view, util::now_ms()) {
            // 재시작 의도를 먼저 내구 기록한다. 감시가 죽어 재개돼도 같은 SCM 요청을 다시 실행하지 않는다.
            publish(&state, &view).await?;
            let target = exe.clone();
            let source = pin.clone();
            let operation = tokio::task::spawn_blocking(move || {
                execute(&target, DeployAction::Restart { service: DeployService::Agent }, &source)
            });
            let result = during(&mut controls, &state, &exe, &mut view, operation).await??;
            if let Err(error) = result {
                restart_failed(&mut view, format!("Windows 서비스 재시작 실패: {error}"), util::now_ms());
            }
            continue;
        }
        if view.phase == Phase::Restoring {
            let uncertain = view.restore_started;
            view.restore_started = true;
            publish(&state, &view).await?;
            let target = exe.clone();
            let source = pin.clone();
            let root = state.join("deployments");
            let expected = view.spec.baseline.sha256.clone();
            let activated = view.activated;
            let operation = tokio::task::spawn_blocking(move || -> Result<bool, String> {
                let _replacement = replacement_lease(&root).map_err(|e| e.to_string())?;
                let current = sha(&target).map_err(|e| e.message)?;
                // 이 잠금 뒤에는 진행 중 설치의 rename이 남아 있지 않다.
                if current == expected && !activated {
                    return Ok(false);
                }
                if uncertain && current != expected {
                    return Err("복구 결과가 모호하다. 같은 작업을 재실행하지 않는다".into());
                }
                if current != expected {
                    execute(&target, DeployAction::CheckedRollback { sha256: expected.clone() }, &source)?;
                }
                if sha(&target).map_err(|e| e.message)? != expected {
                    return Err("복구 뒤 설치 SHA 불일치".into());
                }
                Ok(true)
            });
            let result = during(&mut controls, &state, &exe, &mut view, operation).await??;
            match result {
                Ok(false) => {
                    view.phase = Phase::Cancelled;
                    continue;
                }
                Ok(true) => {}
                Err(e) => {
                    view.phase = Phase::Failed;
                    view.error = Some(e);
                    continue;
                }
            }
            view.phase = Phase::Restarting;
            view.deadline_ms = util::now_ms() + WINDOW_MS;
            continue;
        }
        if view.phase == Phase::Restarting {
            let roles = if view.spec.host_machine && view.spec.service == DeployService::Agent {
                vec![DeployService::Host, DeployService::Agent]
            } else {
                vec![view.spec.service]
            };
            if view.restart_started {
                view.phase = Phase::AwaitingOld;
                continue;
            }
            if let Some(service) = roles.get(view.restart_index as usize).copied() {
                view.restart_started = true;
                if service == DeployService::Host {
                    view.host_restart_ms = util::now_ms();
                } else {
                    view.agent_restart_ms = util::now_ms();
                }
                publish(&state, &view).await?;
                let target = exe.clone();
                let source = pin.clone();
                let operation =
                    tokio::task::spawn_blocking(move || execute(&target, DeployAction::Restart { service }, &source));
                let result = during(&mut controls, &state, &exe, &mut view, operation).await??;
                if let Err(e) = result {
                    restart_failed(&mut view, e, util::now_ms());
                    continue;
                }
                view.restart_index += 1;
                view.restart_started = false;
                continue;
            }
            view.phase = Phase::AwaitingOld;
            view.deadline_ms = util::now_ms() + WINDOW_MS;
            continue;
        }
        let remaining = view.deadline_ms.saturating_sub(util::now_ms());
        // 실행 신원이 맞은 새 host의 웹 응답을 시한까지 묻는다(답하지 않으면 시한이 지나 복구)
        let web = view.pending_web.clone().filter(|_| view.phase == Phase::AwaitingNew);
        if probe.as_ref().map(|(addr, _)| addr) != web.as_ref() {
            if let Some((_, old)) = probe.take() {
                old.abort();
            }
            probe = web.map(|addr| (addr.clone(), tokio::spawn(super::web_probe::until_answer(addr))));
        }
        let answer = async {
            match probe.as_mut() {
                Some((_, task)) => task.await.is_ok(),
                None => std::future::pending().await,
            }
        };
        // 묻기 task가 끝났는가(Some(답했다)). 끝난 handle은 다시 기다리지 않는다
        let mut probed = None;
        tokio::select! {
            _=tokio::time::sleep(Duration::from_millis(remaining))=>{},
            ok=answer => probed = Some(ok),
            peer=controls.rx.recv()=>{
                handle_control(peer.context("감시 listener 종료")??, &state, &exe, &mut view).await?;
            }
        }
        if let Some(answered) = probed {
            probe = None;
            if answered {
                web_answered(&mut view);
            }
        }
    }
}

#[cfg(any(windows, test))]
fn start_windows_restart(view: &mut DeployGuardView, now: u64) -> bool {
    if view.phase != Phase::AwaitingNew
        || !view.activated
        || view.spec.service != DeployService::Agent
        || view.agent_restart_ms != 0
    {
        return false;
    }
    view.agent_restart_ms = now;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn blocked_service_operation_still_answers_status_and_observes_the_new_agent() {
        let temp = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() };
        let root = temp.join(format!("gr-{}", util::new_id()));
        std::fs::create_dir_all(root.join("deployments")).unwrap();
        write_identity(&root.join("deployments")).unwrap();
        let exe = root.join("aaa");
        std::fs::write(&exe, b"candidate").unwrap();
        let mut v = view(DeployService::Agent, false);
        v.spec.expected_sha256 = sha(&exe).unwrap();
        apply(&mut v, Control::Activate, util::now_ms()).unwrap();
        let id = v.spec.id.clone();
        save(&root, &v).unwrap();
        let mut controls = Controls::new(LocalListener::bind(&socket(&root, &id, "g")).unwrap());
        let (release, pending) = tokio::sync::oneshot::channel();
        let server_root = root.clone();
        let server = tokio::spawn(async move {
            during(&mut controls, &server_root, &exe, &mut v, pending).await.unwrap().unwrap();
            restart_failed(&mut v, "늦게 도착한 SCM 오류".into(), util::now_ms());
            v
        });
        // 시간 경과가 아니라 세 개의 실제 RPC 응답을 받은 뒤에만 서비스 작업을 놓는다.
        tokio::time::timeout(Duration::from_secs(3), async {
            let (a, b, c) = tokio::join!(
                rpc(&root, &id, Control::Status),
                rpc(&root, &id, Control::Status),
                rpc(&root, &id, Control::Status)
            );
            for response in [a, b, c] {
                assert_eq!(response.unwrap().phase, Phase::AwaitingNew);
            }
            let ready = rpc(
                &root,
                &id,
                Control::Observe {
                    service: DeployService::Agent,
                    identity: RuntimeIdentity {
                        sha256: sha(&root.join("aaa")).unwrap(),
                        software: "aaa 0.1.2+new".into(),
                        instance: "new-agent".into(),
                        started_ms: util::now_ms(),
                        web: None,
                    },
                },
            )
            .await
            .unwrap();
            assert_eq!(ready.phase, Phase::NewReady);
            assert_eq!(rpc(&root, &id, Control::Commit).await.unwrap().phase, Phase::VerifiedNew);
            release.send(()).unwrap();
            assert_eq!(server.await.unwrap().phase, Phase::VerifiedNew);
        })
        .await
        .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn windows_restart_is_owned_by_the_guard_and_is_not_repeated_after_resume() {
        let mut v = view(DeployService::Agent, false);
        assert!(!start_windows_restart(&mut v, 1));
        apply(&mut v, Control::Activate, 2).unwrap();
        assert!(start_windows_restart(&mut v, 3));
        let mut resumed: DeployGuardView = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(!start_windows_restart(&mut resumed, 4));
        begin_restore(&mut resumed, "SCM start 실패".into(), 5);
        let recovery_deadline = resumed.deadline_ms;
        restart_failed(&mut resumed, "늦은 중복 실패".into(), 6);
        assert_eq!(resumed.deadline_ms, recovery_deadline);
        assert!(!start_windows_restart(&mut resumed, 6));
        assert_eq!(resumed.phase, Phase::Restoring);
        resumed.phase = Phase::Restored;
        restart_failed(&mut resumed, "확인 뒤 늦은 SCM 실패".into(), 7);
        assert_eq!(resumed.phase, Phase::Restored);
        let mut host = view(DeployService::Host, true);
        apply(&mut host, Control::Activate, 2).unwrap();
        assert!(!start_windows_restart(&mut host, 3));
    }
    fn view(service: DeployService, host_machine: bool) -> DeployGuardView {
        DeployGuardView {
            spec: DeployGuardSpec {
                id: util::new_id(),
                rollout: util::new_id(),
                project: "demo".into(),
                service,
                host_machine,
                baseline: RuntimeIdentity {
                    sha256: "a".repeat(64),
                    software: "aaa 0.1.2+old".into(),
                    instance: "old-agent".into(),
                    started_ms: 0,
                    web: None,
                },
                expected_sha256: "b".repeat(64),
                expected_software: "aaa 0.1.2+new".into(),
                old_host_instance: "old-host".into(),
                install_job: Some(util::new_id()),
                rollback_job: util::new_id(),
                restart_job: util::new_id(),
                host_restart_job: util::new_id(),
            },
            phase: Phase::Armed,
            worker_pid: 1,
            deadline_ms: WINDOW_MS,
            install_started: false,
            activated: false,
            restore_started: false,
            restart_index: 0,
            restart_started: false,
            host_restart_ms: 0,
            agent_restart_ms: 0,
            observed: vec![],
            installed_sha256: "a".repeat(64),
            error: None,
            pending_web: None,
        }
    }
    fn identity(sha: &str, software: &str, instance: &str) -> RuntimeIdentity {
        RuntimeIdentity {
            sha256: sha.into(),
            software: software.into(),
            instance: instance.into(),
            started_ms: 100,
            web: None,
        }
    }
    #[test]
    fn guard_requires_a_new_matching_process_and_does_not_renew_failure_deadlines() {
        for service in [DeployService::Host, DeployService::Agent] {
            let mut v = view(service, service == DeployService::Host);
            let spec = v.spec.clone();
            assert!(apply(&mut v, Control::Commit, 0).is_err());
            apply(&mut v, Control::Activate, 10).unwrap();
            let deadline = v.deadline_ms;
            apply(&mut v, Control::Activate, 20).unwrap();
            assert_eq!(v.deadline_ms, deadline);
            let old = if service == DeployService::Host { "old-host" } else { "old-agent" };
            let candidate = identity(&spec.expected_sha256, &spec.expected_software, old);
            apply(&mut v, Control::Observe { service, identity: candidate }, 30).unwrap();
            assert_eq!(v.phase, Phase::AwaitingNew, "옛 프로세스를 새 판으로 확인하면 안 된다");
            v.installed_sha256 = spec.expected_sha256.clone();
            apply(
                &mut v,
                Control::Observe { service, identity: identity(&spec.expected_sha256, &spec.expected_software, "new") },
                40,
            )
            .unwrap();
            assert_eq!(v.phase, Phase::NewReady);
            assert_eq!(v.deadline_ms, deadline);
            apply(&mut v, Control::Commit, 50).unwrap();
            assert_eq!(v.phase, Phase::VerifiedNew);
            assert_eq!(v.spec, spec, "관찰/재개에서 작업 ID가 바뀌었다");
            for (sha, software) in [
                ("c".repeat(64), spec.expected_software.clone()),
                (spec.expected_sha256.clone(), "aaa 0.1.2+wrong".into()),
            ] {
                let mut bad = view(service, service == DeployService::Host);
                apply(&mut bad, Control::Activate, 10).unwrap();
                apply(&mut bad, Control::Observe { service, identity: identity(&sha, &software, "new") }, 20).unwrap();
                assert_eq!(bad.phase, Phase::Restoring, "다른 SHA/판을 성공으로 넘겼다");
            }
            let mut timeout = view(service, service == DeployService::Host);
            apply(&mut timeout, Control::Activate, 10).unwrap();
            expired(&mut timeout, deadline - 1);
            assert_eq!(timeout.phase, Phase::AwaitingNew);
            expired(&mut timeout, deadline);
            assert_eq!(timeout.phase, Phase::Restoring);
            let recovery_deadline = timeout.deadline_ms;
            let recovery_spec = timeout.spec.clone();
            apply(&mut timeout, Control::Recover { reason: "같은 실패".into() }, deadline + 10).unwrap();
            assert_eq!(timeout.deadline_ms, recovery_deadline);
            assert_eq!(timeout.spec, recovery_spec, "복구/재개에서 새 작업 ID를 만들면 안 된다");
        }
    }
    #[test]
    fn a_new_host_is_ready_only_after_its_web_answers_within_the_same_deadline() {
        let with_web = |sha: &str, software: &str, web: &str| RuntimeIdentity {
            web: Some(web.into()),
            ..identity(sha, software, "new")
        };
        let mut v = view(DeployService::Host, true);
        let spec = v.spec.clone();
        apply(&mut v, Control::Activate, 10).unwrap();
        let deadline = v.deadline_ms;
        v.installed_sha256 = spec.expected_sha256.clone();
        let new = with_web(&spec.expected_sha256, &spec.expected_software, "127.0.0.1:7431");
        apply(&mut v, Control::Observe { service: DeployService::Host, identity: new.clone() }, 20).unwrap();
        assert_eq!(v.phase, Phase::AwaitingNew, "실행 신원만으로 새 판 준비로 넘겼다");
        assert_eq!(v.pending_web.as_deref(), Some("127.0.0.1:7431"));
        assert!(apply(&mut v, Control::Commit, 30).is_err(), "웹 응답 전에 확정했다");
        // 다시 붙어 같은 신원을 알려도 기다림은 그대로다
        apply(&mut v, Control::Observe { service: DeployService::Host, identity: new.clone() }, 40).unwrap();
        assert_eq!(v.phase, Phase::AwaitingNew);
        web_answered(&mut v);
        assert_eq!((v.phase, v.pending_web.as_deref()), (Phase::NewReady, None));
        assert_eq!(v.deadline_ms, deadline, "웹 응답이 시한을 늘렸다");
        apply(&mut v, Control::Commit, 50).unwrap();
        assert_eq!(v.phase, Phase::VerifiedNew);

        // 웹이 답하지 않으면 같은 시한에 옛 판으로 되돌린다
        let mut silent = view(DeployService::Host, true);
        apply(&mut silent, Control::Activate, 10).unwrap();
        apply(&mut silent, Control::Observe { service: DeployService::Host, identity: new }, 20).unwrap();
        let limit = silent.deadline_ms;
        expired(&mut silent, limit - 1);
        assert_eq!(silent.phase, Phase::AwaitingNew);
        expired(&mut silent, limit);
        assert_eq!((silent.phase, silent.pending_web.as_deref()), (Phase::Restoring, None));
        web_answered(&mut silent);
        assert_eq!(silent.phase, Phase::Restoring, "복구 중에 늦은 웹 응답으로 준비가 됐다");

        // 웹을 끈 설정·옛 host(주소 없음)는 실행 신원만 본다. agent 감시는 웹을 보지 않는다
        for (service, web) in [
            (DeployService::Host, None),
            (DeployService::Host, Some("")),
            (DeployService::Agent, Some("127.0.0.1:7431")),
        ] {
            let mut v = view(service, service == DeployService::Host);
            apply(&mut v, Control::Activate, 10).unwrap();
            let identity = RuntimeIdentity {
                web: web.map(str::to_owned),
                ..identity(&spec.expected_sha256, &spec.expected_software, "new")
            };
            apply(&mut v, Control::Observe { service, identity }, 20).unwrap();
            assert_eq!((v.phase, v.pending_web.as_deref()), (Phase::NewReady, None), "{service:?} {web:?}");
        }
    }
    #[test]
    fn unchanged_installs_stop_but_changed_installs_restore_and_verify_all_local_roles() {
        let mut unchanged = view(DeployService::Agent, false);
        apply(&mut unchanged, Control::Cancel, 1).unwrap();
        assert_eq!(unchanged.phase, Phase::Cancelled);
        let mut expired_unchanged = view(DeployService::Host, true);
        expired(&mut expired_unchanged, WINDOW_MS);
        assert_eq!(expired_unchanged.phase, Phase::Cancelled);
        let mut pending = view(DeployService::Host, true);
        let job_id = pending.spec.install_job.clone().unwrap();
        apply(&mut pending, Control::BeginInstall { job_id }, 1).unwrap();
        assert!(pending.install_started);
        let mut pending_expiry = pending.clone();
        apply(&mut pending, Control::Cancel, 2).unwrap();
        assert_eq!(pending.phase, Phase::Restoring, "설치 중에는 현재 SHA가 옛것이어도 감시를 종료하지 않는다");
        expired(&mut pending_expiry, WINDOW_MS);
        assert_eq!(pending_expiry.phase, Phase::Restoring);
        // 재시작만 하는 감시(기대 판 = 배포 전 판, 설치 없음): 띄우기 전이면 그 사이 개별 update가 바꾼 설치 파일을 되돌리지 않는다
        let restart_only = || {
            let mut v = view(DeployService::Agent, false);
            v.spec.expected_sha256 = v.spec.baseline.sha256.clone();
            v.spec.install_job = None;
            v.installed_sha256 = "c".repeat(64);
            v
        };
        let mut cancel = restart_only();
        apply(&mut cancel, Control::Cancel, 1).unwrap();
        let mut expiry = restart_only();
        expired(&mut expiry, WINDOW_MS);
        assert_eq!((cancel.phase, expiry.phase), (Phase::Cancelled, Phase::Cancelled));
        let mut activated = restart_only();
        apply(&mut activated, Control::Activate, 1).unwrap();
        expired(&mut activated, 1 + WINDOW_MS);
        assert_eq!(activated.phase, Phase::Restoring, "띄운 뒤 새 프로세스를 확인하지 못하면 복구다");
        let cancelled_job = unchanged.spec.install_job.clone().unwrap();
        assert!(apply(&mut unchanged, Control::BeginInstall { job_id: cancelled_job }, 2).is_err());
        let mut changed = view(DeployService::Agent, true);
        changed.installed_sha256 = changed.spec.expected_sha256.clone();
        let spec = changed.spec.clone();
        apply(&mut changed, Control::Cancel, 1).unwrap();
        assert_eq!(changed.phase, Phase::Restoring);
        changed.phase = Phase::AwaitingOld;
        changed.deadline_ms = 2 * WINDOW_MS;
        changed.agent_restart_ms = 5;
        changed.host_restart_ms = 5;
        let mut earlier = identity(&spec.baseline.sha256, &spec.baseline.software, "failed-before-restore");
        earlier.started_ms = 4;
        apply(&mut changed, Control::Observe { service: DeployService::Agent, identity: earlier }, 2).unwrap();
        assert!(changed.observed.is_empty(), "복구 재시작 전에 이미 뜬 프로세스를 복구 완료로 보았다");
        apply(
            &mut changed,
            Control::Observe {
                service: DeployService::Agent,
                identity: identity(&spec.baseline.sha256, &spec.baseline.software, "old-agent"),
            },
            2,
        )
        .unwrap();
        assert!(changed.observed.is_empty());
        apply(
            &mut changed,
            Control::Observe {
                service: DeployService::Agent,
                identity: identity(&spec.baseline.sha256, &spec.baseline.software, "restored-agent"),
            },
            3,
        )
        .unwrap();
        assert_eq!(changed.phase, Phase::AwaitingOld, "같은 파일을 쓰는 host 확인도 필요하다");
        apply(
            &mut changed,
            Control::Observe {
                service: DeployService::Host,
                identity: identity(&spec.expected_sha256, &spec.baseline.software, "restored-host"),
            },
            4,
        )
        .unwrap();
        assert_eq!(changed.phase, Phase::AwaitingOld, "옛 host의 실제 SHA를 대조한다");
        apply(
            &mut changed,
            Control::Observe {
                service: DeployService::Host,
                identity: identity(&spec.baseline.sha256, &spec.baseline.software, "restored-host"),
            },
            5,
        )
        .unwrap();
        assert_eq!(changed.phase, Phase::Restored);
        assert_eq!(changed.spec, spec);
        let mut failed = view(DeployService::Agent, false);
        failed.phase = Phase::AwaitingOld;
        expired(&mut failed, WINDOW_MS);
        assert_eq!(failed.phase, Phase::Failed, "재연결 없는 복구를 성공으로 표시하면 안 된다");
    }
}
