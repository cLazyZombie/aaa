//! `aaa agent`: agent backend. 이 머신의 세션(session host)을 만들고 감독하며, host와 Agent link로 통신한다.
//!
//! - 외부에 아무것도 listen하지 않는다. host로 나가는 연결만 연다(§4.2 5).
//! - 세션은 session host 프로세스가 소유하므로 agent가 재시작해도 이어진다. 시작할 때 세션 폴더를 다시 읽어 붙는다.
//! - host의 세션 요청은 세션별 pump task가 session host로 중계한다. 응답과 출력이 같은 task를 지나므로 순서가 지켜진다.
//! - Claude·Codex 세션은 pump가 화면을 300ms마다 읽어 상태(작업 중·idle·질문)를 정한다(herdr 0.9.5 감지, plan §8.110).

mod account_spawn;
mod attach;
mod chat;
mod ci;
mod command;
mod cwd;
pub mod deploy;
pub use deploy::guard::worker as deployment_guard;
pub mod detect;
mod diff;
mod files;
mod gh;
mod git_watch;
mod health;
pub mod hooks;
pub mod identity;
pub mod image;
pub mod integration;
mod journal;
pub mod link;
mod page;
mod plan;
pub mod post;
mod pr;
mod quota;
mod responsibility;
mod selfcheck;
mod settings_jobs;
mod shell_chat;
pub use selfcheck::start_selfcheck;
mod slash;
pub mod threads;
pub mod work_root;
mod workspace_metrics;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aaa_proto::{
    AgentEvent, AgentRequest, AgentResponse, B64, ErrorBody, ErrorCode, LinkMsg, MAX_FRAME, Outcome, SESSION_IPC,
    ScreenText, SessionInfo, SessionMsg, SessionRequest, SessionState, Snapshot, SpawnSpec,
};
use anyhow::{Context, Result, bail};
use tokio::sync::mpsc;

use aaa_common::conn::{FrameRx, FrameTx, Incoming, spawn_writer};
use aaa_common::util;
use aaa_session::files::{ExitFile, MetaFile, SessionPaths, SpecFile};

#[cfg(test)]
mod chat_paths_tests;

/// 대화 기록 감시 간격(참고 conversation-monitor.ts `STAT_INTERVAL_MS`·`RESOLVE_INTERVAL_MS`·`MISSING_RESOLVE_INTERVAL_MS`)
const CHAT_STAT_EVERY: Duration = Duration::from_millis(250);
const CHAT_RESOLVE_EVERY: Duration = Duration::from_secs(5);
const CHAT_MISSING_RESOLVE_EVERY: Duration = Duration::from_secs(2);

enum PumpCmd {
    /// host 요청을 session host로 보낸다. 응답은 pump가 host에 보낸다.
    Host { host_id: String, body: SessionRequest },
    /// host 연결이 끊겼다: 구독을 멈춘다.
    HostGone,
    /// 끝난 세션을 지운다(forget): session host가 더 응답하는 동안을 기다리지 않고 연결을 닫는다.
    Close,
    /// 화면 색이 바뀌었다: session host에 알린다(응답은 버린다)
    Colors(aaa_proto::TermColors),
}

struct Entry {
    info: SessionInfo,
    pump: Option<mpsc::Sender<PumpCmd>>,
}

type SessionRx = FrameRx<tokio::io::ReadHalf<aaa_common::ipc::LocalStream>>;

/// 인사를 마친 session host 연결
struct SessionConn {
    tx: FrameTx,
    rx: SessionRx,
    proto: aaa_proto::Version,
    info: SessionInfo,
}

/// session host에 붙지 못한 까닭
enum ConnectError {
    /// 받는 쪽이 없다(socket이 없거나 아무도 listen하지 않는다): session host가 사라졌다
    Gone(anyhow::Error),
    /// session host가 거부했다(token·판). 다시 해도 같다(herdr `ClientEndpointStatus::Attention`)
    Rejected(anyhow::Error),
    /// 그 밖(인사 시한, 인사 중 끊김): 다시 해 볼 만하다
    Retry(anyhow::Error),
}

impl ConnectError {
    fn into_error(self) -> anyhow::Error {
        match self {
            ConnectError::Gone(e) | ConnectError::Rejected(e) | ConnectError::Retry(e) => e,
        }
    }
}

/// pump가 session host 연결을 놓은 까닭
enum Dropped {
    /// forget이 닫았거나 세션 항목이 사라졌다: 다시 붙지 않는다
    Closed,
    /// 연결이 끝났다(까닭). session host가 끝나지 않았으면 다시 붙는다
    Lost(String),
}

/// session host에 연결해 인사한다(token·판)
async fn connect_session(paths: &SessionPaths, spec: &SpecFile) -> Result<SessionConn, ConnectError> {
    let stream = aaa_common::ipc::connect(&paths.sock).await.map_err(|e| {
        let gone = matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused);
        let e = anyhow::Error::new(e).context("session socket 연결");
        if gone { ConnectError::Gone(e) } else { ConnectError::Retry(e) }
    })?;
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let hello = SessionMsg::Hello { proto: SESSION_IPC, token: spec.token.clone() };
    tx.json(&hello).await.map_err(|e| ConnectError::Retry(e.into()))?;
    let welcome = match tokio::time::timeout(Duration::from_secs(5), rx.control()).await {
        Ok(Ok(w)) => w,
        Ok(Err(e)) => return Err(ConnectError::Retry(e.context("welcome 대기"))),
        Err(e) => return Err(ConnectError::Retry(anyhow::Error::new(e).context("welcome 대기"))),
    };
    match welcome {
        SessionMsg::Welcome { proto, info, account_rules } => {
            if let Some(rules) = account_rules {
                aaa_common::redact::restore(&rules.0)
                    .map_err(|_| ConnectError::Rejected(anyhow::anyhow!("인증 가림 규칙 오류")))?;
            }
            Ok(SessionConn { tx, rx, proto, info })
        }
        SessionMsg::Reject(e) => Err(ConnectError::Rejected(anyhow::anyhow!("session host가 거부했다: {e}"))),
        other => Err(ConnectError::Retry(anyhow::anyhow!("예상하지 못한 응답: {other:?}"))),
    }
}

/// session host 프로세스가 있다(`meta.json`의 pid). 좀비이거나 pid가 다시 쓰였으면 있다고 하지만, 그때는 다시 붙을 때
/// 받는 쪽이 없어(`ConnectError::Gone`) 안다
fn session_host_alive(paths: &SessionPaths) -> bool {
    util::read_json::<MetaFile>(&paths.meta())
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok_and(|m| util::pid_alive(m.host_pid))
}

/// 끊긴 뒤 `attempt`번째로 다시 붙기 전 간격: 0.5초부터 두 배씩, 많아야 30초(herdr `client/endpoint/supervisor.rs`
/// `retry_delay`·Local의 `MAX_LOCAL_RETRY_DELAY`)
fn reattach_delay(attempt: u32) -> Duration {
    const INITIAL: Duration = Duration::from_millis(500);
    const MAX: Duration = Duration::from_secs(30);
    INITIAL.saturating_mul(1u32.checked_shl(attempt.saturating_sub(1).min(8)).unwrap_or(u32::MAX)).min(MAX)
}

/// 그린 HTML 페이지: 경로·크기·고친 시각과 bytes
type DrawnPage = (String, u64, std::time::SystemTime, Arc<Vec<u8>>);

pub struct Agent {
    pub state: PathBuf,
    pub machine_id: String,
    pub name: Mutex<String>,
    pub boot_id: String,
    pub dev_mode: bool,
    pub default_cwd: Option<String>,
    exe: PathBuf,
    deployments: deploy::Manager,
    guard_gate: tokio::sync::Mutex<()>,
    sessions: Mutex<HashMap<String, Entry>>,
    /// 연결 변경은 옛 전송 큐에서 기다리는 send_host도 깨운다.
    host: tokio::sync::watch::Sender<Option<FrameTx>>,
    pub selfcheck: Mutex<Option<serde_json::Value>>,
    threads_cfg: Mutex<threads::ThreadsConfig>,
    /// 파일 보기가 여는 폴더(plan §8.125)
    file_roots: Mutex<files::Roots>,
    /// 마지막으로 그린 HTML 페이지(경로·크기·고친 시각): host가 조각으로 읽는 동안 다시 그리지 않는다
    page_cache: Mutex<Option<DrawnPage>>,
    /// report.md 감시 task(thread ID별)
    watches: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// 대화 기록 감시 task(host가 정한 key별, 차이 목록 C3). host 연결이 바뀌면 모두 끝낸다: 새 host가 청한 것만 다시 본다
    chat_watches: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
    /// 기록 상태를 `ChatChanged.signature`로 감추는 이 프로세스의 무작위 열쇠(참고 conversation-monitor.ts `salt`)
    chat_salt: std::hash::RandomState,
    /// 세션별로 다시 알릴 harness 사건(host가 다시 붙으면 다시 보낸다). 칸마다(`Slot`) 마지막 하나다.
    /// 세션 폴더 `harness.json`에도 적어 agent가 다시 떠도 남는다.
    harness_last: Mutex<HashMap<String, Vec<aaa_proto::HarnessEvent>>>,
    /// 연결 시 변경분 전달과 새 harness 사건의 기억·전송 순서를 함께 지킨다.
    harness_delivery: tokio::sync::Mutex<()>,
    /// 세션마다 SessionStart 때 적은 harness 세션 ID(다른 세션의 사건을 거른다)
    harness_started: Mutex<HashMap<String, String>>,
    /// 세션마다 계획·배경 작업 수 추적(plan.rs, 차이 목록 S5)
    plans: Mutex<HashMap<String, plan::Track>>,
    /// 세션마다 지금 harness 프로세스가 시작한 때(Claude SessionStart startup·resume, ms)
    plan_since: Mutex<HashMap<String, u64>>,
    /// 세션마다 진행 알림 기록(Claude·Codex hook 안내, herdr-projects progress.rs `Record`). 기억만 한다: agent가 다시 뜨면 알림이
    /// 한 번 일찍 갈 수 있을 뿐이다.
    progress: Mutex<HashMap<String, hooks::Progress>>,
    /// pane 등록보다 먼저 온 Codex hook을 등록 사건으로 깨운다.
    session_ready: tokio::sync::Notify,
    /// 마지막 머신 건강(host가 다시 붙으면 다시 보낸다)
    health: Mutex<Option<aaa_proto::MachineHealth>>,
    /// 지금 host가 건강 보고(`health.v1`)를 받는가
    host_health: std::sync::atomic::AtomicBool,
    host_recovery: std::sync::atomic::AtomicBool,
    /// 지금 host가 정리 사건(`compacting.v1`, plan §8.309)을 아는가. 모르는 host에는 정리 중을 `working`으로 보낸다
    host_compacting: std::sync::atomic::AtomicBool,
    host_post: Mutex<Option<bool>>,
    host_automation_results: Mutex<bool>,
    host_image: std::sync::atomic::AtomicBool,
    image_waiters: Mutex<HashMap<String, tokio::sync::oneshot::Sender<aaa_proto::Outcome<aaa_proto::ImageResponse>>>>,
    host_mission: Mutex<bool>,
    host_work_files: Mutex<bool>,
    post_gate: Mutex<()>,
    post_waiters: Mutex<HashMap<String, tokio::sync::oneshot::Sender<aaa_proto::PostResult>>>,
    /// 세션마다 사람이 붙인 이름(`spec.json`의 `label`). session host는 띄울 때 이름만 알므로(이전 판은 label을 모른다)
    /// 그가 보내는 정보의 이름은 여기서 덮는다.
    labels: Mutex<HashMap<String, String>>,
    /// host가 알려 준 화면 색(plan §8.187). 새 세션의 spec에 싣고, 다시 붙은 session host에도 보낸다
    term_colors: Mutex<Option<aaa_proto::TermColors>>,
}

impl Agent {
    pub fn new(
        state: PathBuf,
        machine_id: String,
        name: String,
        dev_mode: bool,
        default_cwd: Option<String>,
    ) -> Result<Arc<Self>> {
        util::ensure_private_dir(&state)?;
        util::ensure_private_dir(&state.join("sessions"))?;
        util::ensure_private_dir(&state.join("run"))?;
        let name = match std::fs::read_to_string(state.join("machine-label")) {
            Ok(label) if util::machine_name_ok(label.trim()) => label.trim().to_string(),
            Ok(_) => anyhow::bail!("agent machine-label 이름 형식 오류"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => name,
            Err(e) => return Err(e.into()),
        };
        let deployments = deploy::Manager::new(&state, &std::env::current_exe()?)?;
        Ok(Arc::new(Agent {
            deployments,
            guard_gate: tokio::sync::Mutex::new(()),
            state,
            machine_id,
            name: Mutex::new(name),
            boot_id: util::boot_id(),
            dev_mode,
            default_cwd,
            exe: std::env::current_exe()?,
            sessions: Mutex::new(HashMap::new()),
            host: tokio::sync::watch::channel(None).0,
            selfcheck: Mutex::new(None),
            threads_cfg: Mutex::new(threads::ThreadsConfig::default()),
            file_roots: Mutex::new(files::Roots::default()),
            page_cache: Mutex::new(None),
            watches: Mutex::new(HashMap::new()),
            chat_watches: Mutex::new(HashMap::new()),
            chat_salt: std::hash::RandomState::new(),
            harness_last: Mutex::new(HashMap::new()),
            harness_delivery: tokio::sync::Mutex::new(()),
            labels: Mutex::new(HashMap::new()),
            harness_started: Mutex::new(HashMap::new()),
            plans: Mutex::new(HashMap::new()),
            plan_since: Mutex::new(HashMap::new()),
            progress: Mutex::new(HashMap::new()),
            session_ready: tokio::sync::Notify::new(),
            health: Mutex::new(None),
            host_health: std::sync::atomic::AtomicBool::new(false),
            host_recovery: std::sync::atomic::AtomicBool::new(false),
            host_compacting: std::sync::atomic::AtomicBool::new(false),
            term_colors: Mutex::new(None),
            host_post: Mutex::new(None),
            host_automation_results: Mutex::new(false),
            host_image: std::sync::atomic::AtomicBool::new(false),
            image_waiters: Mutex::new(HashMap::new()),
            host_mission: Mutex::new(false),
            host_work_files: Mutex::new(false),
            post_gate: Mutex::new(()),
            post_waiters: Mutex::new(HashMap::new()),
        }))
    }

    /// 세션 안의 `aaa hook`·`aaa report`가 보내는 사건을 받는다(로컬 socket, 같은 사용자만)
    pub fn start_hooks(self: &Arc<Self>) -> Result<()> {
        let listener = aaa_common::ipc::LocalListener::bind(&hooks::socket(&self.state)).context("hook socket")?;
        let me = self.clone();
        tokio::spawn(async move {
            while let Ok((mut s, pid)) = listener
                .accept_peer()
                .await
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "수신 루프 종료 원인"))
            {
                let me = me.clone();
                tokio::spawn(async move {
                    let reply = hooks::reply(me.on_hook(&mut s, pid).await);
                    aaa_common::diagnostic::ignored(
                        tokio::io::AsyncWriteExt::write_all(&mut s, reply.as_bytes()).await,
                        "보조 작업 실패 뒤 기존 처리를 계속한다",
                    );
                    aaa_common::diagnostic::ignored(
                        tokio::io::AsyncWriteExt::shutdown(&mut s).await,
                        "보조 작업 실패 뒤 기존 처리를 계속한다",
                    );
                });
            }
        });
        Ok(())
    }

    /// hook 사건 하나. 돌려준 것은 `aaa hook`이 Claude·Codex에 출력할 진행 안내다.
    async fn on_hook(
        self: &Arc<Self>,
        s: &mut aaa_common::ipc::LocalStream,
        pid: Option<u32>,
    ) -> Result<Option<hooks::Reply>> {
        let msg = hooks::read_msg(s).await?;
        if msg.harness == "deploy_guard" {
            let guard = deploy::guard::read(&self.state, &msg.session)?;
            if pid != Some(guard.worker_pid) {
                bail!("감시 프로세스의 알림이 아니다");
            }
            if self.host_recovery.load(std::sync::atomic::Ordering::Relaxed) {
                self.send_host(&aaa_proto::LinkMsg::Event(aaa_proto::AgentEvent::DeployGuardChanged {
                    id: guard.spec.id,
                }))
                .await;
            }
            return Ok(None);
        }
        if msg.session.is_empty() || !msg.session.chars().all(|c| c.is_ascii_alphanumeric()) {
            if msg.harness == "post" {
                self.reject_post(&msg, "세션 ID 형식이 아니다").await;
            }
            bail!("세션 ID 형식이 아니다");
        }
        let spec: SpecFile = match util::read_json(&SessionPaths::new(&self.state, &msg.session).spec()) {
            Ok(spec) => spec,
            Err(e) => {
                if msg.harness == "post" {
                    self.reject_post(&msg, "세션 기록이 없다").await;
                    bail!("세션 기록이 없다");
                }
                return Err(e);
            }
        };
        if hooks::hook_token(&spec.token) != msg.token {
            if msg.harness == "post" {
                self.reject_post(&msg, "hook token이 맞지 않는다").await;
            }
            bail!("hook token이 맞지 않는다");
        }
        if msg.harness == "imagegen" {
            return self.on_image(msg, pid).await.map(hooks::Reply::Image).map(Some);
        }
        if msg.harness == "post" {
            let result = self.on_post(msg.clone(), pid).await;
            if let Err(e) = &result {
                self.reject_post(&msg, &e.to_string()).await;
            }
            return result.map(hooks::Reply::Text).map(Some);
        }
        if matches!(msg.harness.as_str(), "codex" | "quota") {
            // 옛 notify는 harness를 선언하지 않은 aaa 셸에서도 왔다. token·peer 검사는 계속 적용한다.
            if msg.harness == "quota" {
                anyhow::ensure!(spec.spec.harness.as_deref() == Some("claude"), "Claude 상태줄이 아니다");
            } else if msg.payload.get("type").and_then(|v| v.as_str()) != Some("agent-turn-complete") {
                anyhow::ensure!(
                    detect::Harness::of_session(spec.spec.harness.as_deref(), &spec.spec.argv)
                        == Some(detect::Harness::Codex),
                    "Codex로 띄운 aaa 세션이 아니다"
                );
            }
            // SessionStart는 attach의 pane 등록보다 먼저 올 수 있다. 재시도·sleep 대신 등록 사건을 기다린다.
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let ready = self.session_ready.notified();
                    tokio::pin!(ready);
                    ready.as_mut().enable();
                    if self.sessions.lock().unwrap().contains_key(&msg.session) {
                        break;
                    }
                    ready.await;
                }
            })
            .await
            .context("hook 세션 등록 대기 시간 초과")?;
            // env·token을 복사한 세션 밖 프로세스는 전역 hook을 가장할 수 없다(post와 같은 peer 판정).
            let panes: Vec<_> = self
                .sessions()
                .into_iter()
                .filter(|s| s.state.is_running())
                .filter_map(|s| s.pid.map(|p| (s.id, p)))
                .collect();
            anyhow::ensure!(
                pid.is_some_and(|p| aaa_common::caller::judge(p, &panes, &[])
                    == aaa_common::caller::Caller::Pane(msg.session.clone())),
                "hook peer가 이 세션 안에 없다"
            );
        }
        if msg.harness == "quota" {
            return self.record_quota(&spec, &msg.payload).map(hooks::Reply::Text).map(Some);
        }
        if msg.harness == "claude"
            && msg.payload.get("hook_event_name").and_then(|v| v.as_str()) == Some("SessionStart")
            && matches!(msg.payload.get("source").and_then(|v| v.as_str()), Some("startup" | "resume"))
        {
            self.harness_process_started(&msg.session, util::now_ms());
        }
        let now = util::now_ms();
        if let Some(ev) = hooks::normalize(&msg.session, &msg.harness, &msg.payload, now) {
            if msg.harness == "report" {
                let mut map = self.progress.lock().unwrap();
                let rec = map.entry(msg.session.clone()).or_default();
                (rec.reported_at, rec.percent) = (now, ev.percent);
            }
            self.harness_event(ev).await;
        }
        // aaa가 hook 설정을 주는 thread 세션만 안내한다(herdr-projects `managed`: coordinator·thread pane)
        let managed = spec.spec.role.is_some();
        Ok(if managed && matches!(msg.harness.as_str(), "claude" | "codex") {
            let auto_context = if spec.spec.role == Some(aaa_proto::Role::Coordinator)
                && matches!(hooks::guide_kind(&msg.payload), Some("SessionStart" | "UserPromptSubmit"))
            {
                let project = spec
                    .spec
                    .cwd
                    .as_deref()
                    .and_then(|p| std::path::Path::new(p).file_name())
                    .and_then(|p| p.to_str())
                    .unwrap_or_default();
                Some(self.coordinator_auto_context(project).await)
            } else {
                None
            };
            self.guide(
                &msg.session,
                &msg.payload,
                spec.spec
                    .env
                    .iter()
                    .find(|(key, _)| key == aaa_proto::AUTOMATION_REENTRY_ENV)
                    .map(|(_, text)| text.as_str()),
                now,
                auto_context.as_deref(),
            )
            .map(hooks::Reply::Text)
        } else {
            None
        })
    }

    /// Claude·Codex hook에 넣을 진행 안내(herdr-projects progress.rs `hook`). 이 세션이 시작한 harness 세션과 다른 세션의 사건에는
    /// 넣지 않는다(`foreign_session`). 실행 파일 경로는 brief와 같은 꼴이다(`fill_brief`).
    fn guide(
        &self,
        session: &str,
        p: &serde_json::Value,
        reentry: Option<&str>,
        now: u64,
        auto_context: Option<&str>,
    ) -> Option<String> {
        let kind = hooks::guide_kind(p)?;
        let id = p.get("session_id").and_then(|v| v.as_str());
        if kind != "SessionStart"
            && let (Some(started), Some(id)) = (self.harness_started.lock().unwrap().get(session), id)
            && started != id
        {
            return None;
        }
        let prefix = aaa_proto::fill_brief(aaa_proto::BRIEF_EXE, "", &self.exe.to_string_lossy());
        let mut map = self.progress.lock().unwrap();
        let mut text = hooks::respond(map.entry(session.to_string()).or_default(), kind, &prefix, now, reentry)?;
        if let Some(context) = auto_context {
            text.push_str("\n\n");
            text.push_str(context);
        }

        Some(hooks::output(kind, &text))
    }

    /// coordinator는 host 머신에 있다. 재시작·compaction 뒤에도 캐시 대신 host의 정본을 읽는다.
    async fn coordinator_auto_context(&self, project: &str) -> String {
        let root = self.state.parent().unwrap_or(&self.state);
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            let mut client = aaa_common::front::FrontClient::connect(root).await?;
            match client.call(aaa_proto::FrontRequest::AutoModeGet { project: project.into() }).await?? {
                aaa_proto::FrontResponse::AutoMode(mode) => Ok::<_, anyhow::Error>(mode),
                _ => anyhow::bail!("auto mode 조회의 응답 종류가 다르다"),
            }
        })
        .await;
        match result {
            Ok(Ok(mode)) => format!(
                "aaa coordinator auto_mode: {} · 자동 결정 {}. 매 턴 aaa context로 다시 확인한다. harness 권한과 무관하며 사용자 전용 승인을 대신하지 않는다. 예외: {}",
                if mode.on { "on" } else { "off" },
                mode.decisions,
                aaa_proto::AUTO_MODE_EXCEPTIONS
            ),
            other => {
                tracing::warn!("coordinator hook auto mode 조회 실패: {other:?}");
                "aaa coordinator auto_mode: 확인 못함. 자동 결정 전에 aaa context로 host 상태와 예외 목록을 확인한다. 이전 상태를 추측하지 않는다.".into()
            }
        }
    }

    /// harness 사건을 기억하고(다시 알릴 것이면) host에 보낸다
    async fn harness_event(self: &Arc<Self>, ev: aaa_proto::HarnessEvent) {
        let _delivery = self.harness_delivery.lock().await;
        // 이 pane이 시작한 harness 세션과 다른 세션(중첩 Codex thread 등)의 사건은 버린다(herdr-projects `foreign_session`)
        if ev.harness != "report" {
            let mut started = self.harness_started.lock().unwrap();
            if ev.kind == "session_start" {
                if let Some(id) = &ev.harness_session {
                    started.insert(ev.session.clone(), id.clone());
                }
            } else if hooks::foreign_session(started.get(&ev.session).map(String::as_str), &ev) {
                return;
            }
        }
        let ev = if Slot::of(&ev).is_some() {
            let me = self.clone();
            let kept = ev.clone();
            tokio::task::spawn_blocking(move || me.remember_harness(kept)).await.unwrap_or(ev)
        } else {
            ev
        };
        let ev = for_host(ev, self.host_compacting.load(std::sync::atomic::Ordering::Relaxed));
        self.send_host(&LinkMsg::Event(AgentEvent::Harness(ev))).await;
    }

    /// 화면 감지가 알린 상태(pump에서)
    async fn detected(self: &Arc<Self>, session: &str, harness: detect::Harness, r: detect::Report) {
        tracing::info!(%session, kind = r.kind, rule = ?r.rule, "harness 상태(화면 감지)");
        self.harness_event(aaa_proto::HarnessEvent {
            session: session.to_string(),
            harness: harness.label().to_string(),
            kind: r.kind.to_string(),
            harness_session: None,
            message: r.message,
            question_kind: r.question_kind.map(str::to_string),
            percent: None,
            at_ms: util::now_ms(),
            transcript: None,
            plan: None,
            background: None,
            context: None,
            model_observation: None,
        })
        .await;
    }

    /// 다시 알릴 사건을 바꾸고 세션 폴더에 적는다. 같은 칸의 앞 사건을 바꾸되, 이번 사건에 없는 기록 위치·
    /// harness 세션 ID는 앞 사건 것을 이어받는다. 돌려준 사건을 host에 보낸다: agent가 다시 떠 같은 상태를 다시
    /// 감지했으면 앞 사건(그 상태가 된 때)을 그대로 보낸다.
    fn remember_harness(&self, mut ev: aaa_proto::HarnessEvent) -> aaa_proto::HarnessEvent {
        let Some(slot) = Slot::of(&ev) else { return ev };
        let mut map = self.harness_last.lock().unwrap();
        let list = map.entry(ev.session.clone()).or_default();
        let send = ev.clone();
        // 셸 복귀 뒤 host가 다시 붙어도 옛 대화·계획을 되살리지 않는다.
        if ev.kind == "released" {
            list.clear();
            self.harness_started.lock().unwrap().remove(&ev.session);
        }
        if let Some(i) = list.iter().position(|e| Slot::of(e) == Some(slot)) {
            let same_state = |o: &aaa_proto::HarnessEvent| {
                slot == Slot::State && (&o.harness, &o.kind, &o.message) == (&ev.harness, &ev.kind, &ev.message)
            };
            if same_state(&list[i]) {
                return list[i].clone();
            }
            let old = list.remove(i);
            if old.harness == ev.harness && slot != Slot::Session {
                ev.transcript = ev.transcript.or(old.transcript);
                ev.harness_session = ev.harness_session.or(old.harness_session);
            }
        }
        list.push(ev.clone());
        list.sort_by_key(|e| e.at_ms);
        // 파일 쓰기도 lock 안에서 해 순서가 뒤바뀌지 않게 한다
        if let Err(e) = util::write_json(&SessionPaths::new(&self.state, &ev.session).harness(), list) {
            tracing::warn!(session = %ev.session, "harness.json을 쓰지 못했다: {e}");
        }
        send
    }

    /// agent.toml의 thread 설정(작업 root·profile)
    pub fn configure_threads(&self, mut cfg: threads::ThreadsConfig) {
        cfg.state_dir = Some(self.state.clone());
        *self.threads_cfg.lock().unwrap() = cfg;
    }

    /// 파일 보기가 열 수 있는 폴더(§6.5 허용 폴더, plan §8.125). 없는 폴더는 빠진다.
    pub fn configure_file_roots(&self, roots: Vec<PathBuf>) {
        *self.file_roots.lock().unwrap() = files::Roots::new(roots);
    }

    /// 페이지로 그린 HTML 파일의 한 조각. 파일이 그대로면 마지막으로 그린 것을 다시 쓴다(조각마다 다시 그리지 않는다)
    fn page_chunk(&self, path: &str, offset: u64, len: u32, roots: &files::Roots) -> anyhow::Result<(Vec<u8>, bool)> {
        let (size, modified) = files::page_key(path, roots)?;
        let cached = self
            .page_cache
            .lock()
            .unwrap()
            .as_ref()
            .filter(|c| c.0 == path && c.1 == size && c.2 == modified)
            .map(|c| c.3.clone());
        let bytes = match cached {
            Some(b) => b,
            None => {
                let b = Arc::new(page::render(Path::new(path), roots)?);
                *self.page_cache.lock().unwrap() = Some((path.to_string(), size, modified, b.clone()));
                b
            }
        };
        let start = (offset as usize).min(bytes.len());
        let end = start.saturating_add(len.min(files::MAX_CHUNK) as usize).min(bytes.len());
        Ok((bytes[start..end].to_vec(), end >= bytes.len()))
    }

    /// 파일 보기: 세션이 지금 있는 폴더(없으면 시작 폴더) 기준으로 경로를 찾는다
    async fn file_locate(&self, session: &str, path: String) -> Result<AgentResponse, ErrorBody> {
        let started = {
            let s = self.sessions.lock().unwrap();
            s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?.info.cwd.clone()
        };
        // 지금 폴더를 못 읽어도 찾는다(참고 `paneContext` 실패: 절대·~ 경로는 그대로 열린다)
        let cwd = match self.session_cwd(session).await {
            Ok(AgentResponse::Cwd { cwd: Some(c) }) => Some(c),
            _ => started,
        };
        self.locate_in(cwd, path).await
    }

    /// 정한 폴더 기준으로 찾는다(보고서 안 파일 링크: thread worktree, plan §8.171). 규칙·허용 폴더는 `file_locate`와 같다
    async fn locate_in(&self, cwd: Option<String>, path: String) -> Result<AgentResponse, ErrorBody> {
        let roots = self.file_roots.lock().unwrap().clone();
        let home = aaa_config::paths::home();
        let found =
            tokio::task::spawn_blocking(move || files::locate(&path, cwd.as_deref().map(Path::new), &home, &roots))
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        match found {
            files::Located::Found(info) => Ok(AgentResponse::FileLocated { info: Some(info), candidates: Vec::new() }),
            files::Located::Candidates(candidates) => Ok(AgentResponse::FileLocated { info: None, candidates }),
            files::Located::Missing => Err(ErrorBody::new(ErrorCode::NotFound, "이 경로에 읽을 수 있는 파일이 없다")),
            files::Located::Outside => Err(ErrorBody::new(
                ErrorCode::Forbidden,
                "허용한 폴더 밖의 파일이라 열지 않는다(그 머신 agent.toml [agent] file_roots)",
            )),
        }
    }

    /// 폴더 보기: 세션이 지금 있는 폴더(없으면 시작 폴더) 기준으로 폴더 한 단계를 읽는다(`file_locate`와 같은 기준)
    async fn dir_list(&self, session: &str, path: String, hidden: bool) -> Result<AgentResponse, ErrorBody> {
        let started = {
            let s = self.sessions.lock().unwrap();
            s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?.info.cwd.clone()
        };
        let cwd = match self.session_cwd(session).await {
            Ok(AgentResponse::Cwd { cwd: Some(c) }) => Some(c),
            _ => started,
        };
        let roots = self.file_roots.lock().unwrap().clone();
        let home = aaa_config::paths::home();
        let listed = tokio::task::spawn_blocking(move || {
            files::list_dir(&path, cwd.as_deref().map(Path::new), &home, &roots, hidden)
        })
        .await
        .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        match listed {
            files::Listed::Dir(d) => Ok(AgentResponse::DirListed(d)),
            files::Listed::Missing => Err(ErrorBody::new(ErrorCode::NotFound, "이 경로에 읽을 수 있는 폴더가 없다")),
            files::Listed::Outside => Err(ErrorBody::new(
                ErrorCode::Forbidden,
                "허용한 폴더 밖이라 열지 않는다(그 머신 agent.toml [agent] file_roots)",
            )),
        }
    }

    /// report.md hash를 1초마다 보고 바뀌면 host에 알린다. 등록하면 지금 값을 한 번 보낸다.
    fn watch_report(self: &Arc<Self>, thread: String, work_dir: PathBuf) {
        let me = self.clone();
        let t = thread.clone();
        let task = tokio::spawn(async move {
            let mut last: Option<Option<String>> = None;
            loop {
                let wd = work_dir.clone();
                let hash = tokio::task::spawn_blocking(move || threads::report_hash(&wd))
                    .await
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "보고 감시 task 실패 뒤 다음 사건 대기")
                    })
                    .unwrap_or(None);
                if last.as_ref() != Some(&hash) {
                    me.send_host(&LinkMsg::Event(AgentEvent::ReportChanged { thread: t.clone(), hash: hash.clone() }))
                        .await;
                    last = Some(hash);
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        if let Some(old) = self.watches.lock().unwrap().insert(thread, task) {
            old.abort();
        }
    }

    fn chat_spec(&self, session: Option<&str>) -> Option<SpecFile> {
        session.filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric())).and_then(|s| {
            util::read_json::<SpecFile>(&SessionPaths::new(&self.state, s).spec())
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
    }

    pub(crate) fn codex_root_for(&self, session: Option<&str>) -> PathBuf {
        let spec = self.chat_spec(session);
        spec.as_ref()
            .and_then(|s| s.account_run.as_deref())
            .and_then(|r| {
                aaa_session::files::account_home(&self.state, r)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .or_else(|| {
                spec.as_ref()?
                    .spec
                    .env
                    .iter()
                    .rev()
                    .find_map(|(k, v)| (k == "CODEX_HOME" && Path::new(v).is_absolute()).then(|| PathBuf::from(v)))
            })
            .map(|h| h.join("sessions"))
            .unwrap_or_else(chat::codex_root)
    }
    pub(crate) fn claude_root_for(&self, session: Option<&str>) -> PathBuf {
        let run = session
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()))
            .and_then(|s| {
                util::read_json::<SpecFile>(&SessionPaths::new(&self.state, s).spec())
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .filter(|s| s.spec.harness.as_deref() == Some("claude"))
            .and_then(|s| s.account_run)
            .filter(|r| aaa_proto::account_run_parts(r).is_some_and(|(_, task)| aaa_proto::valid_thread(task)));
        run.and_then(|r| {
            aaa_session::files::account_home(&self.state, &r)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .map(|h| h.with_file_name("claude-home").join("projects"))
        .filter(|h| h.parent().is_some_and(Path::is_dir))
        .unwrap_or_else(chat::claude_root)
    }
    fn chat_path(
        &self,
        session: Option<&str>,
        harness: &str,
        transcript: Option<&str>,
        harness_session: Option<&str>,
    ) -> Result<PathBuf, String> {
        if harness == "codex" {
            let id = harness_session.ok_or("Codex thread-id가 없다")?;
            let root = self.codex_root_for(session);
            if let Some(path) = transcript.and_then(|p| chat::codex_hint(&root, id, p)) {
                return Ok(path);
            }
            if let Some(spec) = self.chat_spec(session)
                && spec.spec.harness.as_deref() == Some("codex")
                && spec
                    .account_run
                    .as_deref()
                    .and_then(aaa_proto::account_run_parts)
                    .is_some_and(|(_, task)| aaa_proto::valid_thread(task))
                && let Some(cwd) = spec.spec.cwd.as_deref()
            {
                let resume = spec.spec.argv.windows(2).any(|a| a[0] == "resume" && util::harness_session_ok(&a[1]));
                let started = if resume { 0 } else { spec.created_at_ms };
                chat::codex_account_file(&root, id, Path::new(cwd), started)
            } else {
                chat::codex_file(&root, id)
            }
        } else if harness == "claude" {
            let transcript = transcript.ok_or("Claude가 아직 기록 위치를 알리지 않았다(첫 hook 사건 전)")?;
            chat::inside(&self.claude_root_for(session), Path::new(transcript))
        } else {
            chat::path(harness, transcript, harness_session)
        }
    }

    /// 대화 기록 파일을 `CHAT_STAT_EVERY`마다 보고 바뀌면 host에 알린다(차이 목록 C3, herdr-web-ui `server/conversation-monitor.ts`
    /// `ConversationMonitor`). 켜면 지금 값을 한 번 보낸다. 경로는 찾았으면 `CHAT_RESOLVE_EVERY`, 아직 없으면
    /// `CHAT_MISSING_RESOLVE_EVERY`마다 다시 푼다(Codex 기록은 첫 턴 뒤에 생긴다). 파일이 사라지면 바로 다시 푼다
    fn watch_chat(
        self: &Arc<Self>,
        key: String,
        harness: String,
        transcript: Option<String>,
        harness_session: Option<String>,
    ) {
        let me = self.clone();
        let k = key.clone();
        let task = tokio::spawn(async move {
            let mut observed: Option<String> = None;
            let mut path: Option<Result<PathBuf, String>> = None;
            let mut resolve_at = tokio::time::Instant::now();
            loop {
                let now = tokio::time::Instant::now();
                let resolve = now >= resolve_at;
                let (h, t, s, known) = (harness.clone(), transcript.clone(), harness_session.clone(), path.clone());
                let m = me.clone();
                let session = k.clone();
                let (found, seen) = tokio::task::spawn_blocking(move || {
                    let found = match known {
                        Some(p) if !resolve => p,
                        _ => m.chat_path(Some(&session), &h, t.as_deref(), s.as_deref()),
                    };
                    let seen = match &found {
                        Ok(p) => format!("{}\0{}", p.display(), chat::identity(p)),
                        Err(e) => format!("unavailable:{e}"),
                    };
                    (found, seen)
                })
                .await
                .unwrap_or_else(|e| (Err(e.to_string()), "unavailable".into()));
                if resolve {
                    resolve_at = now
                        + if found
                            .as_ref()
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                            })
                            .is_ok()
                        {
                            CHAT_RESOLVE_EVERY
                        } else {
                            CHAT_MISSING_RESOLVE_EVERY
                        };
                }
                let missing = found
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
                    && seen.ends_with("\0missing");
                path = Some(found);
                if observed.as_ref() != Some(&seen) {
                    if missing {
                        resolve_at = now;
                    }
                    let signature = format!("{:016x}", std::hash::BuildHasher::hash_one(&me.chat_salt, (&k, &seen)));
                    me.send_host(&LinkMsg::Event(AgentEvent::ChatChanged { key: k.clone(), signature })).await;
                    observed = Some(seen);
                }
                tokio::time::sleep(CHAT_STAT_EVERY).await;
            }
        });
        if let Some(old) = self.chat_watches.lock().unwrap().insert(key, task) {
            old.abort();
        }
    }

    /// 실행 중인 세션이 지금 있는 폴더(새 탭·나누기 cwd follow). 끝났거나 읽지 못하면 None.
    /// 입력칸 「/」 목록(plan §8.323): 세션이 지금 있는 폴더(못 읽으면 띄운 폴더)를 프로젝트로 본다
    async fn slash_commands(&self, session: &str, harness: String) -> Result<AgentResponse, ErrorBody> {
        let started = {
            let s = self.sessions.lock().unwrap();
            s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?.info.cwd.clone()
        };
        let cwd = match self.session_cwd(session).await {
            Ok(AgentResponse::Cwd { cwd: Some(c) }) => Some(c),
            _ => started,
        };
        let list = tokio::task::spawn_blocking(move || {
            slash::list(&harness, cwd.as_deref().map(Path::new), &chat::claude_home(), &chat::codex_home())
        })
        .await
        .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        Ok(AgentResponse::Commands(list))
    }

    async fn session_cwd(&self, session: &str) -> Result<AgentResponse, ErrorBody> {
        let pid = {
            let s = self.sessions.lock().unwrap();
            let e = s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?;
            e.info.state.is_running().then_some(e.info.pid).flatten()
        };
        let cwd = match pid {
            Some(pid) => tokio::task::spawn_blocking(move || cwd::follow_cwd(pid))
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?,
            None => None,
        };
        Ok(AgentResponse::Cwd { cwd: cwd.map(|p| p.to_string_lossy().into_owned()) })
    }

    /// 사람이 띄운 세션을 thread로 데려온다: 세션이 지금 있는 폴더(앞 프로그램 기준, 못 읽으면 띄운 폴더)에 작업 폴더를 만든다
    async fn thread_adopt(&self, a: aaa_proto::ThreadAdopt) -> Result<AgentResponse, ErrorBody> {
        let (pid, spawned) = {
            let s = self.sessions.lock().unwrap();
            let e = s.get(&a.session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?;
            if !e.info.state.is_running() {
                return Err(ErrorBody::new(ErrorCode::BadRequest, "실행 중인 세션만 데려온다"));
            }
            (e.info.pid, e.info.cwd.clone())
        };
        let live = match pid {
            Some(pid) => tokio::task::spawn_blocking(move || cwd::follow_cwd(pid))
                .await
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                .flatten(),
            None => None,
        };
        let Some(dir) = live.or_else(|| spawned.map(PathBuf::from)) else {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 폴더를 모른다"));
        };
        let exe = self.threads_cfg.lock().unwrap().exe.clone();
        threads::adopt(&exe, a, &dir)
            .await
            .map(AgentResponse::ThreadAdopted)
            .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
    }

    pub fn sessions(&self) -> Vec<SessionInfo> {
        let mut v: Vec<_> = self.sessions.lock().unwrap().values().map(|e| e.info.clone()).collect();
        for info in &mut v {
            *info = self.labeled(info.clone());
        }
        v.sort_by(|a, b| a.started_at_ms.cmp(&b.started_at_ms).then(a.id.cmp(&b.id)));
        v
    }

    fn host_tx(&self) -> Option<FrameTx> {
        self.host.borrow().clone()
    }

    /// 세션별 마지막 harness 사건(시각 차례). hello에 싣거나 옛 host에 다시 보낸다.
    pub fn harness_events(&self) -> Vec<aaa_proto::HarnessEvent> {
        let mut last: Vec<_> = self.harness_last.lock().unwrap().values().flatten().cloned().collect();
        last.sort_by_key(|e| e.at_ms);
        last
    }

    /// host 연결을 바꾼다. 연결 시 harness 변경분은 link가 live 사건과 같은 울타리 안에서 먼저 보낸다.
    pub fn set_host(&self, tx: Option<FrameTx>) {
        let gone = tx.is_none();
        self.host.send_replace(tx);
        for (_, t) in self.chat_watches.lock().unwrap().drain() {
            t.abort();
        }
        if gone {
            let pumps: Vec<_> = self.sessions.lock().unwrap().values().filter_map(|e| e.pump.clone()).collect();
            for p in pumps {
                aaa_common::diagnostic::ignored(
                    p.try_send(PumpCmd::HostGone),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
            }
        }
    }

    async fn send_host(&self, msg: &LinkMsg) {
        let mut host = self.host.subscribe();
        let tx = host.borrow_and_update().clone();
        if let Some(tx) = tx {
            // 실행 환경은 실행 입력이다. ThreadReady/Relaunch는 기록 경계에서만 가린다.
            let clean = match msg {
                LinkMsg::Res {
                    result: Outcome::Ok(AgentResponse::ThreadReady(_) | AgentResponse::Relaunch(_)),
                    ..
                } => msg.clone(),
                _ => match aaa_common::redact::json(msg.clone()) {
                    Ok(clean) => clean,
                    Err(_) => return, // 가림 실패는 기록한 뒤 메시지를 거부한다. 원문은 보내지 않는다.
                },
            };
            // 읽기를 멈춘 옛 host의 큐가 차 있어도 새 연결의 replay 울타리를 붙잡지 않는다.
            // 기억한 harness 상태는 다음 연결이 최신 snapshot으로 전달한다.
            tokio::select! {
                _ = host.changed() => {}
                _ = tx.link(&clean) => {}
            }
        }
    }

    /// 시작할 때: 세션 폴더를 읽어 살아 있는 session host에 다시 붙고, 나머지는 종료·중단·유실로 분류한다.
    pub async fn recover(self: &Arc<Self>) -> Result<()> {
        let dir = self.state.join("sessions");
        let mut ids: Vec<String> = std::fs::read_dir(&dir)?
            .filter_map(|e| {
                e.inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
            })
            .filter(|e| e.path().join("spec.json").exists())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        ids.sort();
        for id in ids {
            let paths = SessionPaths::new(&self.state, &id);
            let spec: SpecFile = match util::read_json(&paths.spec()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(%id, "spec.json 읽기 실패: {e:#}");
                    continue;
                }
            };
            if let Some(label) = &spec.label {
                self.labels.lock().unwrap().insert(id.clone(), label.clone());
            }
            if let Ok(list) = util::read_json::<Vec<aaa_proto::HarnessEvent>>(&paths.harness()) {
                self.harness_last
                    .lock()
                    .unwrap()
                    .insert(id.clone(), list.into_iter().filter(|e| e.session == id).collect());
            }
            if paths.exit().exists() {
                let info = self.offline_info(&paths, &spec);
                self.sessions.lock().unwrap().insert(id, Entry { info, pump: None });
                continue;
            }
            match self.attach(&paths, &spec).await {
                Ok(_) => tracing::info!(%id, "세션에 다시 붙었다"),
                Err(e) => {
                    let info = self.offline_info(&paths, &spec);
                    tracing::info!(%id, state = ?info.state, "붙지 못했다: {e:#}");
                    self.sessions.lock().unwrap().insert(id, Entry { info, pump: None });
                }
            }
        }
        Ok(())
    }

    /// 화면 색을 받았다: 새 세션에 실을 것으로 두고 모든 pump에 넘긴 뒤 답한다. host는 이 답을 받고서야 그 화면에 pane을
    /// 보이므로 그 화면이 본 뒤의 요청은 pump에서 색 뒤에 선다(plan §8.187)
    async fn set_term_colors(&self, c: aaa_proto::TermColors) {
        *self.term_colors.lock().unwrap() = Some(c);
        let pumps: Vec<_> = self.sessions.lock().unwrap().values().filter_map(|e| e.pump.clone()).collect();
        for p in pumps {
            aaa_common::diagnostic::ignored(
                p.send(PumpCmd::Colors(c)).await,
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
    }

    /// session host가 없는 세션의 정보(파일에서)
    fn offline_info(&self, paths: &SessionPaths, spec: &SpecFile) -> SessionInfo {
        let meta: Option<MetaFile> = util::read_json(&paths.meta())
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok();
        let exit: Option<ExitFile> = util::read_json(&paths.exit())
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok();
        let state = match &exit {
            Some(x) => SessionState::Exited { code: x.code },
            None if spec.boot_id != self.boot_id => SessionState::Interrupted,
            None => SessionState::Lost,
        };
        self.labeled(SessionInfo {
            responsibility: None,
            id: spec.id.clone(),
            name: spec.spec.name.clone(),
            argv: spec.spec.argv.clone(),
            cwd: spec.spec.cwd.clone(),
            pid: meta.as_ref().and_then(|m| m.child_pid),
            started_at_ms: meta.map(|m| m.started_at_ms).unwrap_or(spec.created_at_ms),
            boot_id: spec.boot_id.clone(),
            cols: exit.as_ref().map(|x| x.cols).unwrap_or(spec.spec.cols),
            rows: exit.as_ref().map(|x| x.rows).unwrap_or(spec.spec.rows),
            state,
            role: spec.spec.role,
            label: None,
            harness: spec.spec.harness.clone(),
        })
    }

    /// session host나 옛 spec이 준 정보에 사람이 붙인 이름을 덮는다
    fn labeled(&self, mut info: SessionInfo) -> SessionInfo {
        if let Some(label) = self.labels.lock().unwrap().get(&info.id) {
            info.name = label.clone();
            info.label = Some(label.clone());
        }
        info.responsibility = responsibility::observe(&SessionPaths::new(&self.state, &info.id), &info);
        info
    }

    /// session host에 연결하고 pump를 띄운다.
    async fn attach(self: &Arc<Self>, paths: &SessionPaths, spec: &SpecFile) -> Result<SessionInfo> {
        let SessionConn { tx, rx, proto, info } =
            connect_session(paths, spec).await.map_err(ConnectError::into_error)?;
        let info = self.labeled(info);
        let detect_ipc = detect_ipc(proto);
        let colors_ipc = colors_ipc(proto);
        let watch = watch_of(spec, &info, detect_ipc);
        // 띄울 때 harness를 모르는 pane(셸)은 herdr처럼 앞 프로세스 이름으로 agent를 찾는다(plan §8.110)
        let front = info
            .pid
            .filter(|_| cfg!(unix) && watch.is_none() && info.state.is_running())
            .map(|pid| (pid, detect::foreground::Presence::new(std::time::Instant::now())));
        let (ptx, prx) = mpsc::channel(256);
        self.sessions.lock().unwrap().insert(info.id.clone(), Entry { info: info.clone(), pump: Some(ptx) });
        self.session_ready.notify_waiters();
        let me = self.clone();
        let paths = paths.clone();
        let spec = spec.clone();
        tokio::spawn(async move {
            me.pump(paths, spec, tx, rx, prx, Detecting { watch, front, detect_ipc, colors_ipc }).await
        });
        Ok(info)
    }

    /// 세션 하나의 중계. session host 연결이 끊겼는데 session host가 끝나지 않았으면(`exit.json` 없음) 다시 붙는다(`reattach`).
    async fn pump(
        self: Arc<Self>,
        paths: SessionPaths,
        spec: SpecFile,
        mut tx: FrameTx,
        mut rx: SessionRx,
        mut cmds: mpsc::Receiver<PumpCmd>,
        detecting: Detecting,
    ) {
        let id = spec.id.clone();
        let Detecting { mut watch, mut front, detect_ipc, colors_ipc } = detecting;
        let mut shell_chat = Some(shell_chat::Tracker::new());
        let mut seq = 0u64;
        let pending = loop {
            let mut pending: HashMap<String, (String, bool)> = HashMap::new();
            let mut forwarding = false;
            // 이 연결의 session host에 지금 화면 색을 알린다(다시 붙은 session host, agent보다 오래 산 session host). 쓰지 못했으면
            // 아래 읽기가 끊김을 본다
            let colors = *self.term_colors.lock().unwrap();
            if let Some(c) = colors.filter(|_| colors_ipc) {
                seq += 1;
                aaa_common::diagnostic::ignored(
                    tx.json(&SessionMsg::Req { id: format!("c{seq}"), body: SessionRequest::Colors(c) }).await,
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
            }
            let dropped = loop {
                let watch_at = watch.as_ref().and_then(detect::Watch::next_at);
                let chat_at = front.as_ref().and_then(|_| shell_chat.as_ref().map(|c| c.next));
                let detect_at =
                    watch_at.into_iter().chain(front.as_ref().map(|(_, p)| p.next_at())).chain(chat_at).min();
                tokio::select! {
                    _ = sleep_until(detect_at) => {
                        let now = std::time::Instant::now();
                        if let Some((pid, _)) = front.as_ref()
                            && shell_chat.as_ref().is_some_and(|c| c.next <= now)
                        {
                            let mut tracker = shell_chat.take().expect("셸 대화 감시");
                            let (pid, codex, claude) = (*pid, self.codex_root_for(Some(&id)), self.claude_root_for(Some(&id)));
                            let result = tokio::task::spawn_blocking(move || {
                                let changed = tracker.poll(pid, &codex, &claude);
                                (tracker, changed)
                            }).await;
                            let Ok((tracker, changed)) = result.inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")) else {
                                break Dropped::Lost("셸 대화 기록 조사 task가 끝났다".into());
                            };
                            shell_chat = Some(tracker);
                            if let Some((h, b)) = changed {
                                self.shell_chat_event(&id, h, b).await;
                            }
                        }
                        if let Some((pid, p)) = front.as_mut()
                            && p.next_at() <= now
                        {
                            match foreground_tick(p, *pid, now) {
                                Some(detect::foreground::Change::Acquired(h)) => {
                                    watch = Some(detect::Watch::new(h, now, detect_ipc, now));
                                }
                                Some(detect::foreground::Change::Exited(h)) => {
                                    watch = None;
                                    let r = detect::Report { kind: "idle", message: None, question_kind: None, rule: None };
                                    self.detected(&id, h, r).await;
                                }
                                Some(detect::foreground::Change::Released(h)) => {
                                    watch = None;
                                    let r = detect::Report { kind: "released", message: None, question_kind: None, rule: None };
                                    self.detected(&id, h, r).await;
                                }
                                None => {}
                            }
                        }
                        let due = watch.as_ref().and_then(detect::Watch::next_at).is_some_and(|t| t <= now);
                        if let Some((sid, body)) = watch.as_mut().filter(|_| due).and_then(|w| w.due(now))
                            && tx.json(&SessionMsg::Req { id: sid, body }).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err()
                        {
                            break Dropped::Lost("session host에 쓰지 못했다".into());
                        }
                    }
                    cmd = cmds.recv() => match cmd {
                        Some(PumpCmd::Host { host_id, body }) => {
                            if matches!(body, SessionRequest::Input { .. }) && let Some(w) = watch.as_mut() {
                                w.input_sent(std::time::Instant::now());
                            }
                            seq += 1;
                            let sid = format!("h{seq}");
                            let is_sub = matches!(body, SessionRequest::Subscribe { .. });
                            if matches!(body, SessionRequest::Unsubscribe) { forwarding = false; }
                            pending.insert(sid.clone(), (host_id, is_sub));
                            if tx.json(&SessionMsg::Req { id: sid, body }).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                                break Dropped::Lost("session host에 쓰지 못했다".into());
                            }
                        }
                        Some(PumpCmd::HostGone) => {
                            if forwarding {
                                forwarding = false;
                                seq += 1;
                                aaa_common::diagnostic::ignored(tx.json(&SessionMsg::Req { id: format!("u{seq}"), body: SessionRequest::Unsubscribe }).await, "수신자가 끝난 뒤에도 현재 작업은 마친다");
                            }
                        }
                        Some(PumpCmd::Close) | None => break Dropped::Closed,
                        // 답(`c…`)은 pending에 없어 버린다
                        Some(PumpCmd::Colors(c)) => {
                            seq += 1;
                            if colors_ipc
                                && tx.json(&SessionMsg::Req { id: format!("c{seq}"), body: SessionRequest::Colors(c) }).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err()
                            {
                                break Dropped::Lost("session host에 쓰지 못했다".into());
                            }
                        }
                    },
                    incoming = rx.next() => match incoming {
                        Ok(Some(Incoming::Terminal(chunk))) => {
                            if forwarding {
                                match self.host_tx() {
                                    Some(h) => { if h.terminal(&chunk).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { forwarding = false; } }
                                    None => forwarding = false,
                                }
                            }
                        }
                        Ok(Some(Incoming::Control(b))) => match serde_json::from_slice::<SessionMsg>(&b) {
                            Ok(SessionMsg::Res { id: sid, mut result }) => {
                                if aaa_common::terminal_payload::validate_ipc_response(&mut result).as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                                    break Dropped::Lost("IPC 터미널 본문 거부".into());
                                }
                                if let Some(w) = watch.as_mut().filter(|w| w.owns(&sid)) {
                                    let harness = w.harness();
                                    if let Some(r) = w.answer(std::time::Instant::now(), result) {
                                        self.detected(&id, harness, r).await;
                                    }
                                } else if let Some((host_id, is_sub)) = pending.remove(&sid) {
                                    if is_sub && matches!(result, Outcome::Ok(_)) { forwarding = true; }
                                    self.send_host(&LinkMsg::Res { id: host_id, result }).await;
                                }
                            }
                            Ok(SessionMsg::AccountRefresh(mut request)) => {
                                request.session=id.clone();
                                self.send_host(&LinkMsg::Event(AgentEvent::AccountRefresh(request))).await;
                            }
                            Ok(SessionMsg::Updated(info)) => {
                                let info = self.labeled(info);
                                if let Some(e) = self.sessions.lock().unwrap().get_mut(&id) { e.info = info.clone(); }
                                self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info))).await;
                            }
                            Ok(SessionMsg::Resync) => {
                                forwarding = false;
                                self.send_host(&LinkMsg::Event(AgentEvent::Resync { session: id.clone() })).await;
                            }
                            Ok(other) => tracing::debug!(%id, "무시한 session 메시지: {other:?}"),
                            Err(e) => break Dropped::Lost(format!("session 메시지를 해석하지 못했다: {e}")),
                        },
                        Ok(None) => break Dropped::Lost("session host가 연결을 닫았다".into()),
                        // frame 오류는 session host가 끝난 것이 아니다(plan §8.162)
                        Err(e) => break Dropped::Lost(format!("session host frame을 읽지 못했다: {e}")),
                    },
                }
            };
            // 이 연결로 보낸 감지 요청에는 답이 오지 않는다
            if let Some(w) = watch.as_mut() {
                w.connection_lost(std::time::Instant::now());
            }
            // 끝난 session host는 종료 기록(`exit.json`)을 쓴 뒤에 연결을 닫는다
            let why = match dropped {
                Dropped::Lost(_) if paths.exit().exists() => break pending,
                Dropped::Lost(why) if !session_host_alive(&paths) => {
                    tracing::info!(%id, "session host가 없다({why}): 연결을 놓는다");
                    break pending;
                }
                Dropped::Lost(why) => why,
                Dropped::Closed => break pending,
            };
            tracing::warn!(%id, "session host 연결이 끊겼다({why}): 다시 붙는다");
            for (_, (host_id, _)) in pending {
                let err = ErrorBody::new(ErrorCode::Unavailable, format!("session host 연결이 끊겼다: {why}"));
                self.send_host(&LinkMsg::Res { id: host_id, result: Outcome::Err(err) }).await;
            }
            let Some(conn) = self.reattach(&id, &paths, &spec, &mut cmds).await else { break HashMap::new() };
            let info = self.labeled(conn.info);
            (tx, rx) = (conn.tx, conn.rx);
            if let Some(e) = self.sessions.lock().unwrap().get_mut(&id) {
                e.info = info.clone();
            }
            self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info))).await;
            // host는 mirror의 마지막 offset부터 다시 구독한다(session host 보존 범위 안이면 빈틈 없이 bytes만)
            self.send_host(&LinkMsg::Event(AgentEvent::Resync { session: id.clone() })).await;
        };
        // session host가 끝났다(또는 사라졌다, forget이 닫았다): 종료 기록을 기다렸다 읽는다.
        for _ in 0..20 {
            if paths.exit().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let info = self.offline_info(&paths, &spec);
        // harness가 끝났다(herdr는 프로세스가 사라지면 agent를 놓는다. aaa 세션은 harness 하나라 세션 끝이 그것이다)
        if let Some(w) = &watch
            && !info.state.is_running()
        {
            let r = detect::Report { kind: "exited", message: None, question_kind: None, rule: None };
            self.detected(&id, w.harness(), r).await;
        }
        if let Some(e) = self.sessions.lock().unwrap().get_mut(&id) {
            e.info = info.clone();
            e.pump = None;
        }
        for (_, (host_id, _)) in pending {
            let err = ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다");
            self.send_host(&LinkMsg::Res { id: host_id, result: Outcome::Err(err) }).await;
        }
        self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info))).await;
    }

    /// 끊긴 session host에 다시 붙는다. herdr가 Local endpoint와의 연결이 끊기면 하는 것과 같다(`client/endpoint/supervisor.rs`
    /// `disconnected` → Reconnecting, `reattach_delay` 간격, 붙으면 다음 끊김은 다시 첫 간격부터). 기다리는 동안 온 host 요청에는
    /// 「다시 붙는 중」으로 답한다(herdr는 끊긴 endpoint의 요청을 취소한다). herdr의 Local 서버는 사람이 다시 띄울 수 있지만
    /// session host는 다시 뜨지 않으므로, 끝났거나(`exit.json`) 사라졌거나(받는 쪽 없음) 거부하면 그만둔다. forget이 닫아도 그만둔다.
    async fn reattach(
        &self,
        id: &str,
        paths: &SessionPaths,
        spec: &SpecFile,
        cmds: &mut mpsc::Receiver<PumpCmd>,
    ) -> Option<SessionConn> {
        let mut attempt = 0u32;
        loop {
            attempt = attempt.saturating_add(1);
            let until = tokio::time::Instant::now() + reattach_delay(attempt);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(until) => break,
                    cmd = cmds.recv() => match cmd {
                        Some(PumpCmd::Host { host_id, .. }) => {
                            let err = ErrorBody::new(ErrorCode::Unavailable, "세션에 다시 붙는 중이다");
                            self.send_host(&LinkMsg::Res { id: host_id, result: Outcome::Err(err) }).await;
                        }
                        // 다시 붙으면 그때의 색을 보낸다
                        Some(PumpCmd::HostGone) | Some(PumpCmd::Colors(_)) => {}
                        Some(PumpCmd::Close) | None => return None,
                    },
                }
            }
            if paths.exit().exists() {
                return None;
            }
            match connect_session(paths, spec).await {
                Ok(conn) => {
                    tracing::info!(%id, attempt, "세션에 다시 붙었다");
                    return Some(conn);
                }
                Err(ConnectError::Retry(e)) => {
                    let next = reattach_delay(attempt.saturating_add(1));
                    tracing::info!(%id, attempt, "다시 붙지 못했다({e:#}): {}ms 뒤 다시 한다", next.as_millis());
                }
                Err(e) => {
                    tracing::warn!(%id, attempt, "session host에 다시 붙지 못해 놓는다: {:#}", e.into_error());
                    return None;
                }
            }
        }
    }

    /// 새 세션: spec을 쓰고 session host를 분리된 프로세스로 띄운 뒤 붙는다.
    pub async fn spawn_session(self: &Arc<Self>, spec: SpawnSpec) -> Result<SessionInfo> {
        self.spawn_session_inner(spec, None).await
    }
    async fn spawn_session_inner(
        self: &Arc<Self>,
        mut spec: SpawnSpec,
        credential: Option<aaa_proto::AccountCredential>,
    ) -> Result<SessionInfo> {
        if let Some(c) = &credential {
            aaa_common::redact::remember_auth(&c.harness, &c.secret.0);
        }

        let id = util::new_id();
        let paths = SessionPaths::new(&self.state, &id);
        util::ensure_private_dir(&paths.dir)
            .with_context(|| format!("pane 상태 폴더 준비: {}", paths.dir.display()))?;
        if spec.cwd.is_none() {
            spec.cwd = self.default_cwd.clone();
        }
        // 웹 새 세션 창은 `~/…`도 받는다(herdr-web-ui 「absolute path or ~/…」): 이 머신 사용자의 집으로 푼다
        if let Some(rest) = spec.cwd.as_deref().and_then(|c| c.strip_prefix('~'))
            && (rest.is_empty() || rest.starts_with('/'))
            && let Some(home) = std::env::var_os("HOME")
        {
            spec.cwd = Some(format!("{}{rest}", home.to_string_lossy()));
        }
        if let Some(cwd) = &spec.cwd
            && !Path::new(cwd).is_dir()
        {
            bail!("작업 폴더가 없다: {cwd}");
        }
        if let Some(cwd) = &spec.cwd {
            let cfg = self.threads_cfg.lock().unwrap().clone();
            cfg.check_path(Path::new(cwd))?;
        }
        // profile로 띄우기(웹 새 세션 창의 agent 고르기): thread처럼 profile을 argv·env로 풀고 상태 hook을 붙인다.
        // hook 설정 파일은 thread 작업 폴더 대신 이 세션 폴더에 둔다.
        if let Some(name) = spec.profile.clone() {
            // 새 host 정의 + 머신 덮어쓰기. 옛 host의 이름 기반 요청도 전환 기간에는 받는다.
            let cfg = self.threads_cfg.lock().unwrap().clone().fresh()?;
            let profile = match &spec.resolved_profile {
                Some(resolved) => threads::local_profile(&cfg, resolved)?,
                None => threads::find_profile(&cfg, &name)?.0,
            };
            let launch = threads::with_hooks(
                &cfg,
                threads::resolve(&name, &profile)?,
                &paths.dir,
                profile.command.len().max(1),
            )?;
            // thread처럼 profile harness의 화면으로 상태를 본다(host orch `harness: Some(launch.harness)`, plan §8.110)
            spec.harness.get_or_insert(launch.harness);
            spec.argv = launch.argv;
            spec.env.extend(launch.env);
            if spec.name.is_empty() {
                spec.name = name;
            }
        }
        if spec.profile.is_none()
            && let Some(resolved) = &spec.resolved_profile
        {
            // 저장 때 가린 env만 복원한다. 일반 env와 재개 argv는 종전 값이 유지된다.
            if credential.is_some() || spec.env.iter().any(|(_, value)| value.contains("********")) {
                let cfg = self.threads_cfg.lock().unwrap().clone().fresh()?;
                for (key, value) in threads::local_env(&cfg, resolved) {
                    if let Some((_, old)) =
                        spec.env.iter_mut().find(|(k, v)| k == &key && (credential.is_some() || v.contains("********")))
                    {
                        *old = value;
                    } else if credential.is_some() && !spec.env.iter().any(|(k, _)| k == &key) {
                        spec.env.push((key, value));
                    }
                }
            }
        }
        for (key, value) in &spec.env {
            aaa_common::redact::remember_env(key, value);
        }
        let version = if let Some(c) = &credential {
            let mut command = tokio::process::Command::new(spec.argv.first().map(String::as_str).unwrap_or(&c.harness));
            command.envs(spec.env.iter().cloned());
            if let Some(cwd) = &spec.cwd {
                command.current_dir(cwd);
            }
            Some(aaa_common::harness_version::observe(command, &c.harness).await)
        } else {
            None
        };
        // profile 없는 tab create와 저장된 Codex resume도 같은 pane 안에서 실행한다.
        if detect::Harness::of_session(None, &spec.argv) == Some(detect::Harness::Codex) {
            threads::ensure_codex_local(&mut spec.argv, 1);
        }
        if spec.name.is_empty() {
            spec.name = spec
                .argv
                .first()
                .map(|a| a.rsplit(['/', '\\']).next().unwrap_or(a).to_string())
                .unwrap_or_else(|| "shell".into());
        }
        if credential.as_ref().is_some_and(|c| c.harness == "claude" && c.auth_generation.is_some()) {
            let mut settings = hooks::claude_settings(&self.exe);
            settings["statusLine"] = serde_json::json!({"type":"command","command":format!("'{}' hook --agent quota", self.exe.to_string_lossy().replace('\'', "'\\''"))});
            let path = paths.dir.join("quota-settings.json");
            util::write_json(&path, &settings)?;
            let path = path.to_string_lossy().to_string();
            if let Some(i) = spec.argv.iter().rposition(|a| a == "--settings").filter(|i| i + 1 < spec.argv.len()) {
                spec.argv[i + 1] = path;
            } else {
                spec.argv.splice(1..1, ["--settings".into(), path]);
            }
        }
        let token = hex::encode(rand::random::<[u8; 16]>());
        // 세션 안의 `aaa hook`·`aaa report`가 이 agent에 사건을 보낼 곳
        spec.env.push((hooks::ENV_SOCK.into(), hooks::socket(&self.state).to_string_lossy().to_string()));
        spec.env.push((hooks::ENV_TOKEN.into(), hooks::hook_token(&token)));
        let file = SpecFile {
            id: id.clone(),
            token,
            boot_id: self.boot_id.clone(),
            created_at_ms: util::now_ms(),
            spec,
            label: None,
            colors: *self.term_colors.lock().unwrap(),
            account_run: credential.as_ref().map(|c| c.run.clone()),
            account_generation: credential.as_ref().and_then(|c| c.auth_generation.clone()),
        };
        util::write_json(&paths.spec(), &file)
            .with_context(|| format!("pane 실행 명세 쓰기: {}", paths.spec().display()))?;
        let mut log = std::fs::File::create(paths.log())
            .with_context(|| format!("session host 로그 열기: {}", paths.log().display()))?;
        if let Some(version) = version {
            use std::io::Write;
            // thread의 pane ID로 찾는 시작 기록. 판 조회나 기록 실패가 실행을 막지 않는다.
            let _ = writeln!(log, "harness_version session={id} run={:?} {version}", file.account_run);
        }
        let mut cmd = std::process::Command::new(&self.exe);
        cmd.arg("session-host").arg("--dir").arg(&paths.dir);
        cmd.arg("--startup-stdin").stdin(std::process::Stdio::piped());
        cmd.stdout(log.try_clone()?).stderr(log);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x0000_0008;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
            cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        }
        let mut child = cmd.spawn().context("session host 실행")?;
        {
            use std::io::Write;
            let startup = aaa_session::files::Startup { spec: file.spec.clone(), credential };
            let mut pipe = child.stdin.take().context("실행 전달 pipe가 없다")?;
            pipe.write_all(&serde_json::to_vec(&startup).map_err(|_| anyhow::anyhow!("실행 전달 형식 오류"))?)
                .context("실행 전달 pipe 오류")?;
        }
        let pid = child.id();
        // 좀비가 남지 않게 기다리는 thread. agent가 먼저 끝나면 OS 감독자가 거둔다.
        let (exit_tx, mut exit_rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let _ = exit_tx.send(child.wait());
        });
        let mut startup_exit = None;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if startup_exit.is_none() {
                startup_exit = exit_rx.try_recv().ok();
            }
            match self.attach(&paths, &file).await {
                Ok(info) => {
                    self.send_host(&LinkMsg::Event(AgentEvent::SessionStarted(info.clone()))).await;
                    return Ok(info);
                }
                Err(_) if paths.exit().exists() => {
                    // 붙기 전에 이미 끝났다: 종료한 세션으로 등록한다(마지막 화면은 파일에 있다).
                    let info = self.offline_info(&paths, &file);
                    self.sessions.lock().unwrap().insert(id.clone(), Entry { info: info.clone(), pump: None });
                    self.send_host(&LinkMsg::Event(AgentEvent::SessionStarted(info.clone()))).await;
                    return Ok(info);
                }
                Err(e) if tokio::time::Instant::now() < deadline && startup_exit.is_none() && util::pid_alive(pid) => {
                    tracing::trace!("session host 대기: {e:#}");
                    tokio::time::sleep(Duration::from_millis(40)).await;
                }
                Err(e) => {
                    if startup_exit.is_none() && !util::pid_alive(pid) {
                        startup_exit = exit_rx.await.ok();
                    }
                    let log = std::fs::read_to_string(paths.log()).unwrap_or_default();
                    bail!(
                        "session host가 준비되지 않았다: pid={pid} 종료={startup_exit:?} 상태 폴더={} 연결={e:#}\n{log}",
                        paths.dir.display()
                    );
                }
            }
        }
    }

    /// host 요청 하나. 세션 요청은 pump로 넘기고(응답은 pump가 보낸다), 나머지는 여기서 답한다.
    pub async fn handle(self: Arc<Self>, id: String, body: AgentRequest) {
        let reply = self.host_tx();
        self.handle_deadline(id, body, None, reply).await;
    }

    /// 연결이 끊겨도 이 task는 끝까지 수행한다. 자리·정리 결과는 agent journal에 남긴다.
    pub async fn handle_deadline(
        self: Arc<Self>,
        id: String,
        body: AgentRequest,
        deadline_ms: Option<u64>,
        reply: Option<FrameTx>,
    ) {
        let wait = Duration::from_millis(deadline_ms.unwrap_or(600_000).clamp(1, 86_400_000));
        let direct: Result<AgentResponse, ErrorBody> = match body {
            req @ (AgentRequest::DeployGuardArm { .. } | AgentRequest::DeployGuardControl { .. }) => {
                self.guard_request(req).await
            }
            req @ (AgentRequest::DeployFetch { .. }
            | AgentRequest::DeployCandidate { .. }
            | AgentRequest::DeployStage { .. }
            | AgentRequest::DeployStageStatus { .. }
            | AgentRequest::DeployStageClean { .. }) => {
                let agent = self.clone();
                tokio::task::spawn_blocking(move || agent.deployments.transport(&agent.exe, req))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r)
            }
            AgentRequest::DeployStart { id: job, project, action } => {
                return self.deploy_start(id, job, project, action).await;
            }
            AgentRequest::DeployStatus { id } => self.deployments.status(&id).map(|job| self.deployments.response(job)),
            AgentRequest::ImageReply { request_id, result } => {
                if let Some(tx) = self.image_waiters.lock().unwrap().remove(&request_id) {
                    aaa_common::diagnostic::ignored(tx.send(result), "수신자가 끝난 뒤에도 현재 작업은 마친다");
                }
                return;
            }
            AgentRequest::PostAck { post_id, result } => {
                self.post_ack(&post_id, result);
                return;
            }
            AgentRequest::MachineLabel { name } => self.machine_label(&name),
            AgentRequest::AccountQuota { session, generation } => self
                .quota_snapshot(&session, &generation)
                .map(|snapshot| AgentResponse::AccountQuota { snapshot })
                .map_err(|source_error| {
                    aaa_common::diagnostic::context(
                        source_error,
                        ErrorBody::new(ErrorCode::BadRequest, "사용량 관측을 읽지 못했다"),
                    )
                }),
            AgentRequest::SpawnAccount { spec, credential } => self
                .spawn_session_inner(spec, Some(credential))
                .await
                .map(AgentResponse::Session)
                .map_err(account_spawn::error),
            AgentRequest::AccountRefreshed { session, result } => {
                if let Some(v) = &result.response {
                    aaa_common::redact::remember_auth("codex", &v.0);
                }
                return self.to_session(id, &session, SessionRequest::AccountRefreshed(result)).await;
            }
            AgentRequest::Spawn(spec) => self
                .spawn_session(spec)
                .await
                .map(AgentResponse::Session)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::List => Ok(AgentResponse::Sessions { sessions: self.sessions() }),
            AgentRequest::Subscribe { session, from } => {
                return self.to_session(id, &session, SessionRequest::Subscribe { from }).await;
            }
            AgentRequest::Unsubscribe { session } => {
                return self.to_session(id, &session, SessionRequest::Unsubscribe).await;
            }
            AgentRequest::Input { session, data } => {
                if data.0.starts_with(b"\x1b[<") {
                    tracing::debug!(request = %id, %session, bytes = data.0.len(), "pane 입력: agent 수신");
                } else {
                    tracing::info!(request = %id, %session, bytes = data.0.len(), "pane 입력: agent 수신");
                }
                return self.to_session(id, &session, SessionRequest::Input { data }).await;
            }
            AgentRequest::Resize { session, cols, rows } => {
                return self.to_session(id, &session, SessionRequest::Resize { cols, rows }).await;
            }
            AgentRequest::Kill { session } => return self.to_session(id, &session, SessionRequest::Kill).await,
            AgentRequest::Screen { session } => return self.to_session(id, &session, SessionRequest::Screen).await,
            AgentRequest::History { session, anchor, depth } => {
                return self.to_session(id, &session, SessionRequest::History { anchor, depth }).await;
            }
            AgentRequest::Forget { session } => self.forget(&session).await,
            AgentRequest::Rename { session, label } => self.rename(&session, label).await,
            AgentRequest::Cwd { session } => self.session_cwd(&session).await,
            AgentRequest::FileLocate { session, path } => self.file_locate(&session, path).await,
            AgentRequest::FileLocateIn { dir, path } => self.locate_in(Some(dir), path).await,
            AgentRequest::DirList { session, path, hidden } => self.dir_list(&session, path, hidden).await,
            AgentRequest::Commands { session, harness } => self.slash_commands(&session, harness).await,
            AgentRequest::FileView { path, offset, len, page } => {
                let roots = self.file_roots.lock().unwrap().clone();
                let me = self.clone();
                tokio::task::spawn_blocking(move || {
                    if page {
                        me.page_chunk(&path, offset, len, &roots)
                    } else {
                        files::read(&path, offset, len, &roots)
                    }
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                .map(|(data, eof)| AgentResponse::FileChunk { data: B64(data), eof })
            }
            AgentRequest::Chat {
                session,
                harness,
                transcript,
                harness_session,
                thinking,
                compact,
                commands,
                before,
            } => {
                let path =
                    self.chat_path(session.as_deref(), &harness, transcript.as_deref(), harness_session.as_deref());
                tokio::task::spawn_blocking(move || {
                    chat::read_path(&harness, path?, thinking, compact, commands, before.as_deref())
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| {
                    r.map(|page| AgentResponse::Chat {
                        truncated: page.before.is_some(),
                        messages: page.messages,
                        before: page.before,
                        history: Some(page.history),
                    })
                    .map_err(|e| ErrorBody::new(ErrorCode::NotFound, e))
                })
            }
            AgentRequest::Harnesses => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                cfg.fresh()
                    .map(|c| AgentResponse::Profiles { profiles: threads::builtin_views(&c) })
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::Profiles => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                cfg.fresh()
                    .map(|c| AgentResponse::Profiles { profiles: threads::profile_views(&c) })
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::ThreadPrepare(p) => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                match cfg.fresh() {
                    Ok(cfg) => threads::prepare(&cfg, p, wait).await,
                    Err(e) => Err(e),
                }
                .map(AgentResponse::ThreadReady)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::ThreadPlacement { project, thread, title, kind, repo } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                threads::placement(&cfg, &project, &thread, &title, kind, repo.as_deref())
                    .await
                    .map(|placement| AgentResponse::ThreadPlacement { placement })
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::ThreadAdopt(a) => self.thread_adopt(a).await,
            AgentRequest::ThreadWatch { thread, work_dir } => {
                // 감시 key는 `<project>/<thread>`다(thread 번호는 프로젝트마다 다시 센다)
                let ok = thread
                    .split_once('/')
                    .is_some_and(|(p, t)| aaa_proto::valid_project(p) && aaa_proto::valid_thread(t));
                if ok {
                    self.watch_report(thread, PathBuf::from(work_dir));
                    Ok(AgentResponse::Done)
                } else {
                    Err(ErrorBody::new(ErrorCode::BadRequest, "감시 key는 <project>/<thread> 형식이다"))
                }
            }
            AgentRequest::ThreadUnwatch { thread } => {
                if let Some(t) = self.watches.lock().unwrap().remove(&thread) {
                    t.abort();
                }
                Ok(AgentResponse::Done)
            }
            AgentRequest::ChatWatch { key, harness, transcript, harness_session, enabled } => {
                if enabled {
                    self.watch_chat(key, harness, transcript, harness_session);
                } else if let Some(t) = self.chat_watches.lock().unwrap().remove(&key) {
                    t.abort();
                }
                Ok(AgentResponse::Done)
            }
            AgentRequest::ReportTransfer { work_dir, max_library_bytes } => {
                tokio::task::spawn_blocking(move || threads::collect(&work_dir, max_library_bytes))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                    .map(AgentResponse::ReportTransfer)
            }
            AgentRequest::ReportFileRead { work_dir, path, offset, len } => {
                tokio::task::spawn_blocking(move || threads::read_chunk(&work_dir, &path, offset, len))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                    .map(|(data, eof)| AgentResponse::ReportFileChunk { data: B64(data), eof })
            }
            AgentRequest::ReportCollect { work_dir, max_library_bytes } => {
                tokio::task::spawn_blocking(move || threads::collect(&work_dir, max_library_bytes))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                    .map(AgentResponse::Report)
            }
            AgentRequest::FileRead { work_dir, path, offset, len } => {
                tokio::task::spawn_blocking(move || threads::read_chunk(&work_dir, &path, offset, len))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                    .map(|(data, eof)| AgentResponse::FileChunk { data: B64(data), eof })
            }
            AgentRequest::ThreadRebrief { work_dir, brief, launch, profile, safety, resolved_profile } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                cfg.fresh()
                    .and_then(|cfg| {
                        threads::rebrief(
                            &cfg,
                            &work_dir,
                            &brief,
                            launch,
                            profile.as_deref(),
                            safety.as_ref(),
                            resolved_profile.as_ref(),
                        )
                    })
                    .map(AgentResponse::Relaunch)
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::RunCommand { project, cwd, command, timeout_ms } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                command::run(
                    &cfg.work_root,
                    cfg.work_mount.as_deref(),
                    &project,
                    cwd.as_deref(),
                    &command,
                    Duration::from_millis(timeout_ms),
                )
                .await
                .map(AgentResponse::CommandRan)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::WorkFileList { work_dir } => {
                if !*self.host_work_files.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "work_files.v1 미협상"))
                } else {
                    threads::work_file_state_list(&work_dir)
                        .map(|files| AgentResponse::WorkFiles { files })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::WorkFileRead { work_dir, path, offset } => {
                if !*self.host_work_files.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "work_files.v1 미협상"))
                } else {
                    threads::work_file_state_read(&work_dir, &path, offset)
                        .map(|(data, eof)| AgentResponse::FileChunk { data: aaa_proto::B64(data), eof })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::WorkFileWriteChunk { work_dir, path, offset, data, eof, sha256 } => {
                if !*self.host_work_files.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "work_files.v1 미협상"))
                } else {
                    threads::work_file_state_write(&work_dir, &path, offset, &data.0, eof, &sha256)
                        .map(|_| AgentResponse::Done)
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::WorkFileInboxStatus { work_dir } => {
                if !*self.host_work_files.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "host가 work_files.v1을 알리지 않았다"))
                } else {
                    threads::work_file_inbox_pending(&work_dir)
                        .map(|names| AgentResponse::WorkFileInbox { pending: names.len() as u32, names })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::MissionStateList { work_dir } => {
                if !*self.host_mission.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "mission.v1 미협상"))
                } else {
                    threads::work_file_state_list(&work_dir)
                        .map(|files| AgentResponse::MissionFiles { files })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::MissionStateRead { work_dir, path, offset } => {
                if !*self.host_mission.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "mission.v1 미협상"))
                } else {
                    threads::work_file_state_read(&work_dir, &path, offset)
                        .map(|(data, eof)| AgentResponse::FileChunk { data: aaa_proto::B64(data), eof })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::MissionStateWrite { work_dir, path, offset, data, eof, sha256 } => {
                if !*self.host_mission.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "mission.v1 미협상"))
                } else {
                    threads::work_file_state_write(&work_dir, &path, offset, &data.0, eof, &sha256)
                        .map(|_| AgentResponse::Done)
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::MissionInboxStatus { work_dir } => {
                if !*self.host_mission.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "host가 mission.v1을 알리지 않았다"))
                } else {
                    threads::work_file_inbox_pending(&work_dir)
                        .map(|names| AgentResponse::MissionInbox { pending: names.len() as u32, names })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::WorkFileWrite { work_dir, path, data } => {
                tokio::task::spawn_blocking(move || threads::write_inbox(&work_dir, &path, &data.0))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))))
                    .map(|()| AgentResponse::Done)
            }
            AgentRequest::AttachWrite { session, att, offset, data, eof } => {
                self.attach_write(&session, &att, offset, data.0, eof).await
            }
            AgentRequest::WorkAttachWrite { work_dir, att, offset, data, eof } => {
                tokio::task::spawn_blocking(move || {
                    threads::check_work_dir(&work_dir)
                        .and_then(|dir| attach::write_work(&dir, &att, offset, &data.0, eof))
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| {
                    r.map(|r| match r {
                        Some((path, sha256)) => AgentResponse::Attached { path, sha256 },
                        None => AgentResponse::Done,
                    })
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                })
            }
            AgentRequest::TermColors(c) => {
                self.set_term_colors(c).await;
                Ok(AgentResponse::Done)
            }
            AgentRequest::ThreadCleanup { repo, cwd, branch, merged_head, keep_worktree, uncopied } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                match cfg.check_path(Path::new(&cwd)) {
                    Err(e) => Err(ErrorBody::new(ErrorCode::Unavailable, format!("{e:#}"))),
                    Ok(()) => threads::cleanup_recorded(
                        &self.state,
                        repo.as_deref(),
                        &cwd,
                        branch.as_deref(),
                        merged_head.as_deref(),
                        keep_worktree,
                        uncopied,
                        wait,
                    )
                    .await
                    .map(AgentResponse::Cleanup)
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("{e:#}"))),
                }
            }
            AgentRequest::WorkspaceMetrics { cwd, workspace_key, known_code_commits } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                let state = self.state.clone();
                tokio::task::spawn_blocking(move || {
                    let path = std::path::Path::new(&cwd);
                    cfg.check_path(path)?;
                    workspace_metrics::collect(&state, path, &workspace_key, &known_code_commits)
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| {
                    r.map(AgentResponse::WorkspaceMetrics).map_err(|source_error| {
                        aaa_common::diagnostic::context(
                            source_error,
                            ErrorBody::new(ErrorCode::Unavailable, "지표 수집 자료를 읽지 못했다"),
                        )
                    })
                })
            }
            AgentRequest::GitQuery { cwd, query } => git_watch::query(&cwd, &query)
                .await
                .map(AgentResponse::Git)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::CiQuery { cwd, query } => ci::query(&cwd, &query)
                .await
                .map(AgentResponse::Ci)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::PrQuery { cwd, query } => pr::query(&cwd, &query)
                .await
                .map(AgentResponse::Pr)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::ThreadDiff { cwd, base, query } => diff::query(&cwd, base.as_deref(), &query)
                .await
                .map(AgentResponse::Diff)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::WorkRoot { action } => settings_jobs::work_root(&self.state, action).await,
            AgentRequest::CodexHook { install } => settings_jobs::codex_hook(install).await,
        };
        if let Some(reply) = reply {
            aaa_common::diagnostic::ignored(
                reply.link(&LinkMsg::Res { id, result: direct.into() }).await,
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
    }

    /// 실행 중인 세션 폴더에 첨부 한 조각을 쓴다
    async fn attach_write(
        &self,
        session: &str,
        att: &str,
        offset: u64,
        data: Vec<u8>,
        eof: bool,
    ) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        if !self.sessions.lock().unwrap().get(session).is_some_and(|e| e.info.state.is_running()) {
            return Err(ErrorBody::new(ErrorCode::NotFound, format!("실행 중인 세션이 아니다: {session}")));
        }
        let dir = SessionPaths::new(&self.state, session).dir;
        let att = att.to_string();
        tokio::task::spawn_blocking(move || attach::write(&dir, &att, offset, &data, eof))
            .await
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?
            .map(|r| match r {
                Some((path, sha256)) => AgentResponse::Attached { path, sha256 },
                None => AgentResponse::Done,
            })
            .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
    }

    /// 끝난 세션을 지울 때 pump가 떨어지기를 기다리는 한도
    const FORGET_WAIT: Duration = Duration::from_secs(5);

    /// 사람이 붙인 이름을 `spec.json`에 적고(지우면 뺀다) 바뀐 정보를 host에 알린다. 끝난 세션에도 붙인다(다시 열면 그 이름).
    fn machine_label(&self, name: &str) -> Result<AgentResponse, ErrorBody> {
        if !util::machine_name_ok(name) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "머신 이름 형식 오류"));
        }
        let mut label = self.name.lock().unwrap();
        let path = self.state.join("machine-label");
        if *label != name || !path.exists() {
            let temp = self.state.join("machine-label.tmp");
            std::fs::write(&temp, name).and_then(|_| std::fs::rename(&temp, &path)).map_err(|e| {
                ErrorBody::new(ErrorCode::Internal, format!("agent 이름표 저장 {}: {e}", path.display()))
            })?;
            *label = name.into();
        }
        Ok(AgentResponse::Done)
    }

    async fn rename(&self, session: &str, label: Option<String>) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        if label
            .as_ref()
            .is_some_and(|l| l.chars().count() > aaa_proto::MAX_PANE_LABEL_CHARS || l.chars().any(char::is_control))
        {
            return Err(ErrorBody::new(
                ErrorCode::BadRequest,
                format!("이름은 제어 문자 없이 {}자 안이어야 한다", aaa_proto::MAX_PANE_LABEL_CHARS),
            ));
        }
        if !self.sessions.lock().unwrap().contains_key(session) {
            return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}")));
        }
        let paths = SessionPaths::new(&self.state, session);
        let mut spec: SpecFile = util::read_json(&paths.spec())
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("spec.json 읽기: {e:#}")))?;
        spec.label = label.clone();
        util::write_json(&paths.spec(), &spec)
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("spec.json 쓰기: {e:#}")))?;
        match &label {
            Some(l) => self.labels.lock().unwrap().insert(session.to_string(), l.clone()),
            None => self.labels.lock().unwrap().remove(session),
        };
        let info = {
            let mut map = self.sessions.lock().unwrap();
            let Some(e) = map.get_mut(session) else {
                return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}")));
            };
            e.info.name = spec.name().to_string();
            e.info.label = label;
            e.info.clone()
        };
        self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info.clone()))).await;
        Ok(AgentResponse::Session(info))
    }

    /// 끝난 세션 기록(폴더)을 지운다. 실행 중이면 거부한다.
    /// pump가 붙어 있으면 떨어질 때까지 기다렸다 지운다. 먼저 지우면 pump가 끝나며 보내는 종료 알림이 기록을 되살린다.
    /// session host는 종료를 알린 뒤에도 2초 더 응답하므로(늦게 붙는 agent도 마지막 화면을 받게) 그 연결은 여기서 닫는다:
    /// 종료는 이미 받았고 마지막 화면은 파일에 있다. 닫기를 기다리면 끝난 thread resolve·pane 닫기마다 2초가 걸렸다(t-0338, plan §8.160).
    async fn forget(&self, session: &str) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        let deadline = tokio::time::Instant::now() + Self::FORGET_WAIT;
        let mut closing = false;
        loop {
            {
                let mut map = self.sessions.lock().unwrap();
                match map.get(session) {
                    None => return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}"))),
                    Some(e) if e.info.state.is_running() => {
                        return Err(ErrorBody::new(ErrorCode::BadRequest, "실행 중인 세션은 지울 수 없다"));
                    }
                    Some(e) if e.pump.is_none() => {
                        map.remove(session);
                        break;
                    }
                    Some(e) if !closing => {
                        closing = e.pump.as_ref().is_some_and(|p| {
                            p.try_send(PumpCmd::Close)
                                .as_ref()
                                .inspect_err(|error| {
                                    aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                                })
                                .is_ok()
                        });
                    }
                    Some(_) => {}
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ErrorBody::new(ErrorCode::BadRequest, "session host가 아직 닫히지 않았다"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.harness_last.lock().unwrap().remove(session);
        self.labels.lock().unwrap().remove(session);
        self.progress.lock().unwrap().remove(session);
        let paths = SessionPaths::new(&self.state, session);
        if let Err(e) = std::fs::remove_dir_all(&paths.dir) {
            tracing::warn!(%session, "세션 폴더 삭제 실패: {e}");
        }
        Ok(AgentResponse::Done)
    }

    async fn to_session(&self, id: String, session: &str, body: SessionRequest) {
        let (pump, info) = {
            let map = self.sessions.lock().unwrap();
            match map.get(session) {
                Some(e) => (e.pump.clone(), Some(e.info.clone())),
                None => (None, None),
            }
        };
        let result = match (pump, info) {
            (Some(p), _) => {
                if p.send(PumpCmd::Host { host_id: id.clone(), body })
                    .await
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
                {
                    return;
                }
                Err(ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다"))
            }
            (None, Some(info)) => self.offline_request(&info, body),
            (None, None) => Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}"))),
        };
        self.send_host(&LinkMsg::Res { id, result: result.into() }).await;
    }

    /// 끝난 세션: 마지막 화면 파일로 답한다.
    fn offline_request(&self, info: &SessionInfo, body: SessionRequest) -> Result<AgentResponse, ErrorBody> {
        let paths = SessionPaths::new(&self.state, &info.id);
        match body {
            SessionRequest::Subscribe { .. } => {
                let replay = std::fs::read(paths.final_replay())
                    .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
                    .unwrap_or_default();
                let end = util::read_json::<ExitFile>(&paths.exit())
                    .map(|x| x.end_offset)
                    .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
                    .unwrap_or(0);
                Ok(AgentResponse::Subscribed {
                    session: info.id.clone(),
                    snapshot: Some(Snapshot {
                        compression: None,
                        session: info.id.clone(),
                        offset: end,
                        cols: info.cols,
                        rows: info.rows,
                        replay: B64(replay),
                        history_more: false,
                    }),
                    offset: end,
                })
            }
            SessionRequest::Unsubscribe => Ok(AgentResponse::Done),
            SessionRequest::Screen => util::read_json::<ScreenText>(&paths.final_screen())
                .map(AgentResponse::Screen)
                .map_err(|source_error| {
                    aaa_common::diagnostic::context(
                        source_error,
                        ErrorBody::new(ErrorCode::SessionExited, "마지막 화면 기록이 없다"),
                    )
                }),
            _ => Err(ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다")),
        }
    }
}

/// 다시 알릴 harness 사건의 칸. 칸마다 마지막 하나를 남긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// 진행 보고(`aaa report`)
    Report,
    /// harness 세션 ID·기록 위치(SessionStart hook, Codex notify)
    Session,
    /// 상태(화면 감지, 끝남). 세션 ID 사건이 이것을 덮으면 다시 붙은 host가 상태를 잃는다.
    State,
    /// 계획·배경 작업 수(plan.rs)
    Plan,
}

impl Slot {
    /// 다시 알리지 않는 사건(입력 제출 같은 한때 신호)은 None
    fn of(e: &aaa_proto::HarnessEvent) -> Option<Slot> {
        match e.kind.as_str() {
            _ if e.harness == "report" => Some(Slot::Report),
            "session_start" | "session" => Some(Slot::Session),
            "plan" => Some(Slot::Plan),
            "prompt_submit" => None,
            _ => Some(Slot::State),
        }
    }
}

/// pump의 화면 감지: 띄울 때 정한 harness의 감지 차례(`watch`), 또는 셸 pane 앞 agent 찾기(`front`, 셸 pid와 상태).
/// 앞에서 agent를 잡으면 그 harness로 `watch`를 만든다. `detect_ipc`: session host가 `Detect`를 받는다.
/// `colors_ipc`: session host가 `Colors`를 받는다(감지는 아니지만 같은 session host 판으로 정한다).
struct Detecting {
    watch: Option<detect::Watch>,
    front: Option<(u32, detect::foreground::Presence)>,
    detect_ipc: bool,
    colors_ipc: bool,
}

/// 셸 pane 앞의 agent를 한 차례 본다. herdr의 Windows 프로세스 트리 조사는 옮기지 않았다(unix만, plan §8.110).
fn foreground_tick(
    p: &mut detect::foreground::Presence,
    shell_pid: u32,
    now: std::time::Instant,
) -> Option<detect::foreground::Change> {
    #[cfg(unix)]
    {
        p.tick(now, cwd::foreground_process_group_id(shell_pid), |g| detect::foreground::probe(shell_pid, g))
    }
    #[cfg(not(unix))]
    {
        let _ = (p, shell_pid, now);
        None
    }
}

/// session host가 SESSION_IPC 1.1 이상일 때만 `Detect`를 보낸다(그 전 session host는 모르는 요청을 받으면 연결을 끊는다)
fn detect_ipc(proto: aaa_proto::Version) -> bool {
    proto.major == SESSION_IPC.major && proto.minor >= aaa_proto::SESSION_IPC_DETECT
}

fn colors_ipc(proto: aaa_proto::Version) -> bool {
    proto.major == SESSION_IPC.major && proto.minor >= aaa_proto::SESSION_IPC_COLORS
}

/// 화면으로 상태를 볼 세션이면 감지 차례
fn watch_of(spec: &SpecFile, info: &SessionInfo, detect_ipc: bool) -> Option<detect::Watch> {
    if !info.state.is_running() {
        return None;
    }
    let harness = detect::Harness::of_session(spec.spec.harness.as_deref(), &spec.spec.argv)?;
    let now = std::time::Instant::now();
    // 띄운 지 오래된 세션(agent가 다시 떠 붙었다)은 시작 유예가 이미 끝났다
    let age = Duration::from_millis(util::now_ms().saturating_sub(info.started_at_ms));
    let started = now.checked_sub(age).unwrap_or(now);
    Some(detect::Watch::new(harness, started, detect_ipc, now))
}

/// `at`까지 잔다(None이면 깨지 않는다)
async fn sleep_until(at: Option<std::time::Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(tokio::time::Instant::from_std(t)).await,
        None => std::future::pending().await,
    }
}

/// 정리 사건(`compacting`, plan §8.309)을 모르는 host(`compacting.v1`을 알리지 않음)에는 작업 중으로 보낸다: 옛 host는 모르는 kind를
/// 버려 정리 내내 앞 상태(쉼)로 남고, 받음 대기(`ACCEPTED`)도 끝내 받지 못한다. 기억하는 사건(`remember_harness`)은 그대로 둔다
fn for_host(mut ev: aaa_proto::HarnessEvent, host_compacting: bool) -> aaa_proto::HarnessEvent {
    if ev.kind == "compacting" && !host_compacting {
        ev.kind = "working".into();
    }
    ev
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tab create와 저장된 resume 모두 session host에 넘기는 spec에서 daemon을 막는다.
    /// 없는 실행 파일에서 멈추므로 실제 harness·서비스·프로세스 신호는 쓰지 않는다.
    #[tokio::test]
    async fn codex_spawn_spec_stays_in_the_pane() {
        let title = r#"tui.terminal_title=["run-state","activity","project-name"]"#;
        for (argv, expected) in [
            (vec!["codex"], vec!["codex", "--no-daemon", "-c", title]),
            (
                vec!["C:\\tools\\codex.exe", "resume", "test-id"],
                vec!["C:\\tools\\codex.exe", "--no-daemon", "resume", "test-id", "-c", title],
            ),
            (vec!["codex", "--no-daemon"], vec!["codex", "--no-daemon", "-c", title]),
            (vec!["codex", "--", "--no-daemon"], vec!["codex", "--no-daemon", "-c", title, "--", "--no-daemon"]),
            (vec!["sh", "-c", "codex"], vec!["sh", "-c", "codex"]),
        ] {
            let state = std::env::temp_dir().join(format!("aaa-spawn-{}", util::new_id()));
            let mut a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
            Arc::get_mut(&mut a).unwrap().exe = state.join("missing-session-host");
            let (spec, _) = session(None, &argv, SessionState::Running);
            let error = a.spawn_session(spec.spec).await.unwrap_err();
            assert_eq!(error.to_string(), "session host 실행");
            let dirs: Vec<_> = std::fs::read_dir(state.join("sessions")).unwrap().collect();
            assert_eq!(dirs.len(), 1);
            let file: SpecFile = util::read_json(&dirs[0].as_ref().unwrap().path().join("spec.json")).unwrap();
            assert_eq!(file.spec.argv, expected, "session host에 공유 daemon을 허용하는 argv를 넘겼다");
            assert!(file.spec.env.iter().any(|(k, v)| k == hooks::ENV_TOKEN && !v.is_empty()));
            std::fs::remove_dir_all(state).unwrap();
        }
    }

    /// 읽기를 멈춘 옛 host의 전송 큐를 그대로 둬도 연결 변경은 그 전송 대기를 끝내야 한다.
    /// harness_event는 전송까지 순서 울타리를 잡으므로, 옛 대기가 남으면 다음 Welcome도 진행하지 못한다.
    #[tokio::test]
    async fn pending_host_send_lets_go_when_the_connection_changes() {
        use std::future::Future;
        use std::task::Poll;
        let state = std::env::temp_dir().join(format!("aach-{}", util::new_id()));
        let a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let (old, _old_out) = aaa_common::conn::frame_channel();
        a.set_host(Some(old.clone()));
        for seq in 0..512 {
            old.json(&LinkMsg::Ping { seq }).await.unwrap();
        }
        // agent의 일반 전달도 B64 글자 충돌로 panic하거나 응답을 잃지 않아야 한다.
        use base64::Engine;
        aaa_common::redact::remember("QkxPQ0tTRUNSRVQ5agent-fixture0525");
        let snapshot = base64::engine::general_purpose::STANDARD.decode("QkxPQ0tTRUNSRVQ5AAAA").unwrap();
        let message = LinkMsg::Res {
            id: "snapshot-request".into(),
            result: Outcome::Ok(AgentResponse::Subscribed {
                session: "snapshot-session".into(),
                offset: snapshot.len() as u64,
                snapshot: Some(aaa_proto::Snapshot {
                    compression: None,
                    session: "snapshot-session".into(),
                    offset: snapshot.len() as u64,
                    cols: 80,
                    rows: 24,
                    replay: B64(snapshot),
                    history_more: false,
                }),
            }),
        };
        let mut sending = std::pin::pin!(a.send_host(&message));
        std::future::poll_fn(|cx| {
            assert!(sending.as_mut().poll(cx).is_pending(), "가득 찬 옛 전송 큐에서 기다려야 한다");
            Poll::Ready(())
        })
        .await;
        a.set_host(None);
        tokio::time::timeout(Duration::from_secs(3), sending).await.expect("옛 전송 대기가 새 연결을 막았다");
        let (new, mut out) = aaa_common::conn::frame_channel();
        a.set_host(Some(new));
        a.send_host(&message).await;
        let (_, bytes) = out.recv().await.unwrap();
        assert_eq!(serde_json::from_slice::<LinkMsg>(&bytes).unwrap(), message);
        std::fs::remove_dir_all(state).unwrap();
    }

    #[tokio::test]
    async fn compaction_goes_to_an_old_host_as_working() {
        let base = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() };
        let state = base.join(format!("aach-{}", util::new_id()));
        let a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let (tx, mut out) = aaa_common::conn::frame_channel();
        a.set_host(Some(tx));
        let ev = aaa_proto::HarnessEvent {
            session: "s".into(),
            harness: "claude".into(),
            kind: "compacting".into(),
            harness_session: None,
            message: None,
            question_kind: None,
            percent: None,
            at_ms: util::now_ms(),
            transcript: None,
            plan: None,
            background: None,
            context: None,
            model_observation: None,
        };
        let mut sent = async |host_compacting: bool| {
            a.host_compacting.store(host_compacting, std::sync::atomic::Ordering::Relaxed);
            a.harness_event(ev.clone()).await;
            let (_, bytes) = out.recv().await.unwrap();
            match serde_json::from_slice(&bytes).unwrap() {
                LinkMsg::Event(AgentEvent::Harness(ev)) => ev.kind,
                other => panic!("{other:?}"),
            }
        };
        // 옛 host(`compacting.v1` 없음)는 모르는 kind를 버리므로 작업 중으로 보낸다
        assert_eq!(sent(false).await, "working");
        assert_eq!(sent(true).await, "compacting");
        // 다시 알릴 사건은 원래 kind다: 새 host에 다시 붙으면 정리 중으로 보인다
        assert!(a.harness_events().iter().any(|e| e.kind == "compacting"));
        std::fs::remove_dir_all(state).unwrap();
    }

    #[tokio::test]
    async fn codex_start_before_pane_registration_waits_for_registration() {
        use std::task::Poll;
        use tokio::io::AsyncWriteExt;
        let base = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() };
        let state = base.join(format!("aach-{}", util::new_id()));
        let a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let (mut spec, mut info) = session(Some("codex"), &["codex"], SessionState::Running);
        spec.spec.role = Some(aaa_proto::Role::Worker);
        info.pid = Some(std::process::id());
        let paths = SessionPaths::new(&state, "s");
        util::ensure_private_dir(&paths.dir).unwrap();
        util::write_json(&paths.spec(), &spec).unwrap();
        let (tx, mut out) = aaa_common::conn::frame_channel();
        a.set_host(Some(tx));
        let msg = hooks::HookMsg {
            session: "s".into(),
            token: hooks::hook_token(&spec.token),
            harness: "codex".into(),
            payload: serde_json::json!({"hook_event_name":"SessionStart","source":"startup","session_id":"019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0"}),
        };
        let (mut writer, reader) = tokio::io::duplex(8192);
        writer.write_all(&serde_json::to_vec(&msg).unwrap()).await.unwrap();
        let mut reader: aaa_common::ipc::LocalStream = Box::new(reader);
        let mut pending = std::pin::pin!(a.on_hook(&mut reader, Some(std::process::id())));
        // 입력은 이미 완성됐다. 등록 전 poll에서 거부하면 실제 시작 race다.
        std::future::poll_fn(|cx| {
            assert!(pending.as_mut().poll(cx).is_pending(), "등록 전에 정상 hook을 거부했다");
            Poll::Ready(())
        })
        .await;
        // 실제 attach가 pane을 등록하고 깨운다. 시험이 등록 완료 신호를 대신 공급하지 않는다.
        let listener = aaa_common::ipc::LocalListener::bind(&paths.sock).unwrap();
        let fake = tokio::spawn(async move {
            let (r, w) = tokio::io::split(listener.accept_peer().await.unwrap().0);
            let tx = spawn_writer(w, MAX_FRAME);
            let mut rx = FrameRx::new(r, MAX_FRAME);
            assert!(matches!(rx.control::<SessionMsg>().await.unwrap(), SessionMsg::Hello { .. }));
            tx.json(&SessionMsg::Welcome { proto: SESSION_IPC, info, account_rules: None }).await.unwrap();
            std::future::pending::<()>().await;
            drop((tx, rx, listener));
        });
        a.attach(&paths, &spec).await.unwrap();
        let hooks::Reply::Text(reply) = pending.await.unwrap().unwrap() else { panic!("hook 안내 형식") };
        assert!(reply.contains("report --percent 25"), "{reply}");
        let (_, bytes) = out.recv().await.unwrap();
        let event: LinkMsg = serde_json::from_slice(&bytes).unwrap();
        assert!(matches!(event, LinkMsg::Event(AgentEvent::Harness(ev)) if ev.kind == "session_start"));
        fake.abort();
        std::fs::remove_dir_all(state).unwrap();
    }

    pub(super) fn session(harness: Option<&str>, argv: &[&str], state: SessionState) -> (SpecFile, SessionInfo) {
        let spec = SpawnSpec {
            name: "s".into(),
            argv: argv.iter().map(|a| a.to_string()).collect(),
            cwd: None,
            env: vec![],
            cols: 80,
            rows: 24,
            role: None,
            harness: harness.map(str::to_string),
            profile: None,
            resolved_profile: None,
        };
        let info = SessionInfo {
            responsibility: None,
            id: "s".into(),
            name: "s".into(),
            argv: spec.argv.clone(),
            cwd: None,
            pid: Some(1),
            // 오래전에 띄운 세션(시작 유예가 끝났다)
            started_at_ms: 1,
            boot_id: "b".into(),
            cols: 80,
            rows: 24,
            state,
            role: None,
            label: None,
            harness: spec.harness.clone(),
        };
        (
            SpecFile {
                id: "s".into(),
                token: "t".into(),
                boot_id: "b".into(),
                created_at_ms: 1,
                spec,
                label: None,
                colors: None,
                account_run: None,
                account_generation: None,
            },
            info,
        )
    }

    /// 감지 요청: 유예가 끝난 차례 다음의 첫 요청
    fn first_request(w: &mut detect::Watch) -> SessionRequest {
        let now = std::time::Instant::now();
        assert!(w.due(now).is_none());
        w.due(w.next_at().unwrap()).unwrap().1
    }

    #[test]
    fn detection_asks_old_session_hosts_only_for_the_screen() {
        let v = |minor| detect_ipc(aaa_proto::Version { major: SESSION_IPC.major, minor });
        let running = SessionState::Running;
        let (spec, info) = session(Some("codex"), &["/opt/bin/codex", "-c", "x"], running.clone());
        // SESSION_IPC 1.0 session host는 `Detect`를 모르고 연결을 끊는다: 보이는 화면만 묻는다
        let mut old = watch_of(&spec, &info, v(0)).unwrap();
        assert_eq!(old.harness(), detect::Harness::Codex);
        assert_eq!(first_request(&mut old), SessionRequest::Screen);
        let mut new = watch_of(&spec, &info, v(aaa_proto::SESSION_IPC_DETECT)).unwrap();
        assert_eq!(first_request(&mut new), SessionRequest::Detect { unchanged_since: None });
        // profile harness가 shell·custom이면 실행 파일 이름으로, 그것도 모르면 보지 않는다
        let (spec, info) = session(Some("custom"), &["/x/claude", "--settings", "f"], running.clone());
        assert_eq!(watch_of(&spec, &info, v(1)).map(|w| w.harness()), Some(detect::Harness::Claude));
        let (spec, info) = session(None, &["/bin/sh"], running);
        assert!(watch_of(&spec, &info, v(1)).is_none());
        let (spec, info) = session(Some("claude"), &["claude"], SessionState::Exited { code: Some(0) });
        assert!(watch_of(&spec, &info, v(1)).is_none(), "끝난 세션은 보지 않는다");
    }

    /// 다시 붙기 간격은 0.5초에서 두 배씩 늘고 30초에서 멈춘다(넘치지 않는다, herdr `retry_backoff_is_bounded`)
    #[test]
    fn reattach_delay_is_bounded() {
        assert_eq!(reattach_delay(1), Duration::from_millis(500));
        assert_eq!(reattach_delay(2), Duration::from_secs(1));
        assert_eq!(reattach_delay(100), Duration::from_secs(30));
    }

    /// 가짜 session host가 본 것(연결 번호)
    #[derive(Debug)]
    enum Seen {
        Hello(u32),
        Req(u32, SessionRequest),
    }

    /// 가짜 session host가 첫 연결을 끝내는 길. 모두 frame 오류가 아니다.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum FirstEnd {
        /// 연결만 닫는다(옛 session host가 모르는 요청을 받았을 때처럼)
        Eof,
        /// agent가 모르는 메시지를 보낸다(새 session host)
        Unknown,
        /// 연결을 닫고 listen도 그만둔다(session host가 사라졌다)
        Gone,
        /// 화면 감지 요청을 받고 답하지 않은 채 닫는다(harness 세션)
        AfterDetect,
    }

    /// session host 하나를 흉내 낸다: 연결마다 token을 보고 Welcome(실행 중)을 보낸다. 첫 연결은 `first`로 끝내고, 그 뒤 연결은
    /// 요청마다 Done으로 답한다(Subscribe는 snapshot 없이 offset 0). `open`이 거짓인 동안은 두째 연결부터 인사만 받고 닫는다.
    fn fake_session_host(
        listener: aaa_common::ipc::LocalListener,
        info: SessionInfo,
        token: String,
        first: FirstEnd,
        open: Arc<std::sync::atomic::AtomicBool>,
    ) -> mpsc::UnboundedReceiver<Seen> {
        let (seen, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut listener = Some(listener);
            for n in 1u32.. {
                let Some(l) = listener.as_ref() else { break };
                let Ok(stream) = l.accept().await else { break };
                let (r, w) = tokio::io::split(stream);
                let tx = spawn_writer(w, MAX_FRAME);
                let mut rx = FrameRx::new(r, MAX_FRAME);
                match rx.control::<SessionMsg>().await {
                    Ok(SessionMsg::Hello { token: t, .. }) if t == token => {}
                    other => panic!("인사가 아니다: {other:?}"),
                }
                let _ = seen.send(Seen::Hello(n));
                if n > 1 && !open.load(std::sync::atomic::Ordering::SeqCst) {
                    continue;
                }
                tx.json(&SessionMsg::Welcome { proto: SESSION_IPC, info: info.clone(), account_rules: None })
                    .await
                    .unwrap();
                if n == 1 {
                    match first {
                        FirstEnd::Eof => {}
                        FirstEnd::Unknown => tx.control(br#"{"t":"later_kind"}"#.to_vec()).await.unwrap(),
                        FirstEnd::Gone => listener = None,
                        FirstEnd::AfterDetect => {
                            while let Ok(Some(Incoming::Control(b))) = rx.next().await {
                                if let Ok(SessionMsg::Req { body, .. }) = serde_json::from_slice::<SessionMsg>(&b) {
                                    let _ = seen.send(Seen::Req(1, body));
                                    break;
                                }
                            }
                        }
                    }
                    // 쓰기 handle을 놓으면 보낸 것을 다 쓴 뒤 닫힌다(읽는 쪽은 여기서 놓는다)
                    continue;
                }
                let seen = seen.clone();
                tokio::spawn(async move {
                    while let Ok(Some(Incoming::Control(b))) = rx.next().await {
                        let Ok(SessionMsg::Req { id, body }) = serde_json::from_slice::<SessionMsg>(&b) else { break };
                        let result = match &body {
                            SessionRequest::Subscribe { .. } => {
                                AgentResponse::Subscribed { session: "s".into(), snapshot: None, offset: 0 }
                            }
                            _ => AgentResponse::Done,
                        };
                        let _ = seen.send(Seen::Req(n, body));
                        if tx.json(&SessionMsg::Res { id, result: Outcome::Ok(result) }).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        rx
    }

    /// host 쪽 줄에서 다음 link 메시지
    async fn next_link(out: &mut aaa_common::conn::FrameOut) -> LinkMsg {
        let (_, b) = out.recv().await.expect("host 줄이 닫혔다");
        serde_json::from_slice(&b).unwrap()
    }

    /// 실험: 가짜 session host에 붙은 agent와 host 쪽 줄
    struct Rig {
        agent: Arc<Agent>,
        state: PathBuf,
        seen: mpsc::UnboundedReceiver<Seen>,
        /// 가짜가 마지막으로 받은 인사의 연결 번호
        hello: u32,
        out: aaa_common::conn::FrameOut,
    }

    impl Rig {
        /// 가짜가 `first`로 첫 연결을 끝낸 뒤 agent의 다음 연결(Hello 2)과 host로 간 「세션이 끝났다」 중 먼저 온 것
        async fn after_first_connection_ends(
            first: FirstEnd,
            open: bool,
        ) -> (Rig, Arc<std::sync::atomic::AtomicBool>, Result<(), SessionState>) {
            // macOS UDS 경로 한도(104바이트) 때문에 짧은 /tmp에 둔다(m13 실험실과 같다)
            let state = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() }
                .join(format!("aar-{}-{first:?}-{open}", std::process::id()));
            let _ = std::fs::remove_dir_all(&state);
            let agent = Agent::new(state.clone(), "m".into(), "m".into(), false, None).unwrap();
            let (spec, mut info) = match first {
                // 화면으로 상태를 보는 세션(오래전에 띄워 시작 유예가 끝났다)
                FirstEnd::AfterDetect => session(Some("claude"), &["claude"], SessionState::Running),
                _ => session(None, &["/bin/sh"], SessionState::Running),
            };
            // 앞 프로세스 감지(셸 pid)는 보지 않는다
            info.pid = None;
            let spec = SpecFile { boot_id: agent.boot_id.clone(), ..spec };
            let paths = SessionPaths::new(&state, &spec.id);
            util::ensure_private_dir(&paths.dir).unwrap();
            util::write_json(&paths.spec(), &spec).unwrap();
            // session host는 이 시험 프로세스다(살아 있다)
            let meta = MetaFile { host_pid: std::process::id(), child_pid: None, started_at_ms: 1 };
            util::write_json(&paths.meta(), &meta).unwrap();
            let listener = aaa_common::ipc::LocalListener::bind(&paths.sock).unwrap();
            let gate = Arc::new(std::sync::atomic::AtomicBool::new(open));
            let mut seen = fake_session_host(listener, info, spec.token.clone(), first, gate.clone());
            let (host, mut out) = aaa_common::conn::frame_channel();
            agent.set_host(Some(host));
            agent.recover().await.unwrap();
            assert!(matches!(seen.recv().await, Some(Seen::Hello(1))));
            // 안전 상한(오지 않을 때 멈춘다). 다시 붙기 첫 간격은 0.5초다
            let outcome = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    // 가짜는 Welcome보다 먼저 Hello를 알린다: 먼저 보면 그 뒤에 오는 host 알림(Resync)을 여기서 버리지 않는다
                    tokio::select! {
                        biased;
                        Some(s) = seen.recv() => if matches!(s, Seen::Hello(2)) { return Ok(()) },
                        m = next_link(&mut out) => if let LinkMsg::Event(AgentEvent::SessionUpdated(i)) = m
                            && !i.state.is_running()
                        {
                            return Err(i.state);
                        },
                    }
                }
            })
            .await
            .expect("다시 붙지도, 놓지도 않았다");
            (Rig { agent, state, seen, hello: 2, out }, gate, outcome)
        }

        /// host로 간 다음 응답(id·결과)
        async fn next_res(&mut self) -> (String, Outcome<AgentResponse>) {
            loop {
                if let LinkMsg::Res { id, result } = next_link(&mut self.out).await {
                    return (id, result);
                }
            }
        }

        async fn input(&self, id: &str) {
            let body = AgentRequest::Input { session: "s".into(), data: B64(b"x".to_vec()) };
            self.agent.clone().handle(id.into(), body).await;
        }

        /// 가짜 session host가 받은 다음 요청(연결 번호·요청). 인사는 건너뛴다
        async fn next_req(&mut self) -> (u32, SessionRequest) {
            // 안전 상한(오지 않을 때 멈춘다). 다시 붙기 간격은 길어야 몇 초다
            let next = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    match self.seen.recv().await {
                        Some(Seen::Hello(n)) => self.hello = n,
                        Some(Seen::Req(n, body)) => return (n, body),
                        None => panic!("가짜 session host가 끝났다"),
                    }
                }
            });
            next.await.expect("session host가 요청을 받지 않았다")
        }

        /// 다시 붙었다: 세션은 실행 중이고, host는 다시 구독하라는 알림을 받고(aaa-host `on_resync`가 마지막 offset부터 다시
        /// 구독한다), 입력은 새 연결로 session host에 간다
        async fn works_again(mut self) {
            assert!(self.agent.sessions().iter().all(|s| s.state.is_running()), "{:?}", self.agent.sessions());
            let resync = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    if let LinkMsg::Event(AgentEvent::Resync { session }) = next_link(&mut self.out).await {
                        return session;
                    }
                }
            });
            assert_eq!(resync.await.expect("다시 구독 알림이 없다"), "s");
            self.input("q1").await;
            let to = loop {
                match self.seen.recv().await {
                    Some(Seen::Hello(n)) => self.hello = n,
                    Some(Seen::Req(n, SessionRequest::Input { .. })) => break n,
                    other => panic!("입력 대신 {other:?}"),
                }
            };
            assert_eq!(to, self.hello, "입력이 마지막 연결로 가지 않았다");
            assert_eq!(self.next_res().await, ("q1".into(), Outcome::Ok(AgentResponse::Done)));
            std::fs::remove_dir_all(&self.state).unwrap();
        }
    }

    /// pump가 frame 오류 밖의 까닭으로 끝나도 session host가 살아 있으면 다시 붙는다(herdr Local endpoint 재연결, plan §8.186).
    /// session host가 연결만 닫았다(옛 session host가 모르는 요청을 받았을 때, `detect_ipc` 주석)
    #[tokio::test]
    async fn pump_reattaches_after_the_session_host_closes_the_connection() {
        let (rig, _, outcome) = Rig::after_first_connection_ends(FirstEnd::Eof, true).await;
        assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
        rig.works_again().await;
    }

    /// session host가 agent가 모르는 메시지를 보냈다(pump의 「session 메시지 해석 실패」)
    #[tokio::test]
    async fn pump_reattaches_after_an_unknown_session_message() {
        let (rig, _, outcome) = Rig::after_first_connection_ends(FirstEnd::Unknown, true).await;
        assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
        rig.works_again().await;
    }

    /// 다시 붙는 동안 온 요청에는 「세션에 다시 붙는 중이다」로 답한다(세션은 실행 중 그대로). 인사를 받지 못한 연결은 다음
    /// 간격에 다시 해 보고, 붙은 뒤의 요청은 새 연결로 간다.
    #[tokio::test]
    async fn requests_while_reattaching_are_told_so() {
        let (mut rig, gate, outcome) = Rig::after_first_connection_ends(FirstEnd::Eof, false).await;
        assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
        // 가짜가 인사를 받지 않는 동안은 붙을 수 없다: 이 요청은 다시 붙는 중에 온다
        rig.input("w1").await;
        let (id, result) = rig.next_res().await;
        assert_eq!(id, "w1");
        let Outcome::Err(e) = result else { panic!("다시 붙는 중인데 답했다: {result:?}") };
        assert_eq!((e.code, e.message.as_str()), (ErrorCode::Unavailable, "세션에 다시 붙는 중이다"));
        assert!(rig.agent.sessions().iter().all(|s| s.state.is_running()), "{:?}", rig.agent.sessions());
        gate.store(true, std::sync::atomic::Ordering::SeqCst);
        rig.works_again().await;
    }

    /// 끊긴 연결로 보낸 감지 요청에는 답이 오지 않는다: 다시 붙은 뒤 새 연결로 다시 묻는다(감지가 멈추지 않는다)
    #[tokio::test]
    async fn detection_asks_again_after_reattach() {
        let (mut rig, _, outcome) = Rig::after_first_connection_ends(FirstEnd::AfterDetect, true).await;
        assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
        let again = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match rig.seen.recv().await {
                    Some(Seen::Req(n, SessionRequest::Detect { .. })) if n > 1 => return n,
                    Some(_) => {}
                    None => panic!("가짜 session host가 끝났다"),
                }
            }
        });
        assert!(again.await.is_ok(), "다시 붙은 뒤 화면 감지를 다시 묻지 않는다");
        std::fs::remove_dir_all(&rig.state).unwrap();
    }

    /// 화면 색(plan §8.187): 다시 붙는 동안 받은 색은 붙자마자 그 session host에 가고, 붙어 있는 동안 받은 색은 바로 간다.
    /// 둘 다 agent가 답하기 전에 pump에 넘겼다
    #[tokio::test]
    async fn colors_reach_the_session_host_after_and_while_attached() {
        let dark = aaa_proto::TermColors { fg: [0xd8, 0xd0, 0xc3], bg: [0x18, 0x16, 0x13] };
        let light = aaa_proto::TermColors { fg: [0x2a, 0x25, 0x1f], bg: [0xfa, 0xf8, 0xf3] };
        let (mut rig, gate, outcome) = Rig::after_first_connection_ends(FirstEnd::Unknown, false).await;
        assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
        rig.agent.clone().handle("c1".into(), AgentRequest::TermColors(dark)).await;
        assert_eq!(rig.next_res().await, ("c1".into(), Outcome::Ok(AgentResponse::Done)));
        gate.store(true, std::sync::atomic::Ordering::SeqCst);
        let (n, body) = rig.next_req().await;
        assert_eq!(body, SessionRequest::Colors(dark), "다시 붙은 session host에 받아 둔 색을 먼저 보내지 않았다");
        rig.agent.clone().handle("c2".into(), AgentRequest::TermColors(light)).await;
        assert_eq!(rig.next_req().await, (n, SessionRequest::Colors(light)));
        std::fs::remove_dir_all(&rig.state).unwrap();
    }

    /// session host가 사라졌으면(socket이 없다) 다시 붙으려다 놓는다: 세션은 Lost이고 더 묻지 않는다
    #[tokio::test]
    async fn pump_lets_go_once_the_session_host_is_gone() {
        let (mut rig, _, outcome) = Rig::after_first_connection_ends(FirstEnd::Gone, true).await;
        assert_eq!(outcome, Err(SessionState::Lost));
        assert!(rig.agent.sessions().iter().all(|s| s.state == SessionState::Lost), "{:?}", rig.agent.sessions());
        assert!(rig.seen.try_recv().is_err(), "사라진 session host에 또 붙었다");
        std::fs::remove_dir_all(&rig.state).unwrap();
    }
}

#[cfg(test)]
mod mission_feature_tests {
    use super::*;
    use aaa_common::conn::{FrameRx, spawn_writer};
    #[tokio::test]
    async fn old_host_cannot_receive_new_mission_responses() {
        let state = std::env::temp_dir().join(format!("aaa-mission-feature-{}", util::new_id()));
        let a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        let work = state.join("work/.aaa/demo-t-0001");
        std::fs::create_dir_all(&work).unwrap();
        let (s, peer) = tokio::io::duplex(1 << 20);
        let (_, w) = tokio::io::split(s);
        a.set_host(Some(spawn_writer(w, aaa_proto::MAX_FRAME)));
        let mut rx = FrameRx::new(peer, aaa_proto::MAX_FRAME);
        a.clone()
            .handle("old".into(), AgentRequest::MissionStateList { work_dir: work.to_string_lossy().into() })
            .await;
        assert!(
            matches!(rx.control::<LinkMsg>().await.unwrap(),LinkMsg::Res{id,result:aaa_proto::Outcome::Err(e)} if id=="old" && e.code==ErrorCode::Forbidden)
        );
        *a.host_mission.lock().unwrap() = true;
        a.clone()
            .handle("new".into(), AgentRequest::MissionStateList { work_dir: work.to_string_lossy().into() })
            .await;
        assert!(
            matches!(rx.control::<LinkMsg>().await.unwrap(),LinkMsg::Res{id,result:aaa_proto::Outcome::Ok(AgentResponse::MissionFiles{files})} if id=="new" && files.is_empty())
        );
        std::fs::remove_dir_all(state).unwrap();
    }
}

#[cfg(test)]
mod machine_label_tests {
    use super::*;
    #[tokio::test]
    async fn machine_label_is_persistent_without_restarting_or_touching_sessions() {
        let state = std::env::temp_dir().join(format!("aaa-label-{}", util::new_id()));
        let a = Agent::new(state.clone(), "m-label".into(), "mac-b".into(), false, None).unwrap();
        assert_eq!(a.name.lock().unwrap().as_str(), "mac-b");
        assert!(matches!(a.machine_label("mac-c").unwrap(), AgentResponse::Done));
        assert_eq!(a.name.lock().unwrap().as_str(), "mac-c");
        assert_eq!(a.machine_id, "m-label");
        assert!(a.sessions.lock().unwrap().is_empty());
        assert!(a.machine_label("bad name").is_err());
        drop(a);
        let a = Agent::new(state.clone(), "m-label".into(), "mac-b".into(), false, None).unwrap();
        assert_eq!(a.name.lock().unwrap().as_str(), "mac-c", "설정의 옛 이름보다 host 이름표가 우선한다");
        assert_eq!(a.machine_id, "m-label");
        drop(a);
        std::fs::remove_dir_all(state).unwrap();
    }
}
