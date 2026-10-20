//! `aaa host`: host backend. 정본 목록·권한·Front API·웹을 갖고, 각 머신의 agent backend와 Agent link로 통신한다.
//!
//! - host는 자식 프로세스를 만들지 않는다. 예외: tailscale CLI, 개발 모드의 로컬 agent 기동(§4.2).
//! - pane 화면은 agent(session host)가 정본이다. host는 마지막 화면 mirror를 가져 agent가 끊겨도 읽기 전용으로 보여 준다.

mod adopt;
mod app_badge;
mod attach;
/// 자동화 정본 타입의 편집기용 JSON Schema. host 상태를 읽지 않는다.
pub fn automation_schema() -> String {
    serde_json::to_string_pretty(&automation::schema()).expect("schema serialize")
}
mod accounts;
mod auto_start;
mod automation;
mod automation_thread;
mod bgwait;
mod brief;
mod brief_text;
mod caller;
mod chatwatch;
mod ci;
mod colors;
mod coord;
mod ctrl_enter;
pub mod db;
mod deploy;
mod diff;
mod file_rev;
mod files;
pub mod front;
mod git_watch;
mod harness;
mod health;
mod http;
mod inbox;
mod input_log;
mod legacy_mission;
mod library;
mod library_links;
mod library_publish;
mod library_retention;
mod lifecycle;
pub mod link;
pub mod migrate;
mod notice;
mod notify;
mod open;
mod orch;
mod output;
mod post;
mod pr;
mod prefs;
mod project;
mod prompt;
mod prompt_box;
mod push;
mod report_mark;
mod resource;
mod restore;
mod rollout;
mod routine;
mod routine_runs;
mod seen;
mod summary;
pub mod tailscale;
mod task_clean;
mod task_edit;
mod tasks;
mod text;
mod transitions;
mod trust_screen;
pub mod usage;
mod usage_limit;
mod watch;
mod watch_event;
pub mod web;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aaa_config::{HostConfig, LocalAgentMode};
use aaa_proto::{
    AgentRequest, AgentResponse, Connection, DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontRequest, FrontResponse,
    HostStatus, LayoutChange, LayoutNode, LeaseView, LinkMsg, MachineView, Outcome, PaneView, PendingEnroll,
    PublishState, ScreenText, SessionInfo, SessionState, SpawnSpec, Tab, TerminalChunk, Workspace,
};
use anyhow::{Context, Result};
use sha2::Digest;
use tokio::sync::{broadcast, oneshot, watch as tokio_watch};

use aaa_common::conn::FrameTx;
use aaa_common::util;
use aaa_vt::Vt;

pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

type Pending = Arc<Mutex<HashMap<String, oneshot::Sender<Outcome<AgentResponse>>>>>;

/// 연결된 agent 하나. 같은 머신이 다시 붙으면 generation이 바뀌고 이전 연결의 사건은 버린다.
#[derive(Clone)]
pub struct AgentConn {
    pub tx: FrameTx,
    pub pending: Pending,
    pub generation: u64,
    seq: Arc<AtomicU64>,
    timeouts: aaa_config::HostSection,
}

impl AgentConn {
    pub fn new(tx: FrameTx, generation: u64) -> Self {
        AgentConn {
            tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            generation,
            seq: Arc::new(AtomicU64::new(0)),
            timeouts: Default::default(),
        }
    }

    pub async fn request(&self, body: AgentRequest) -> Result<AgentResponse, ErrorBody> {
        let wait = match &body {
            AgentRequest::ThreadPrepare(_) | AgentRequest::ThreadPlacement { .. } => {
                Duration::from_secs(self.timeouts.thread_prepare_timeout_secs)
            }
            AgentRequest::ThreadCleanup { .. } => Duration::from_secs(self.timeouts.thread_cleanup_timeout_secs),
            AgentRequest::FileRead { .. } | AgentRequest::ReportCollect { .. } => {
                Duration::from_secs(self.timeouts.file_request_timeout_secs)
            }
            _ => REQUEST_TIMEOUT,
        };
        self.request_timeout(body, wait).await
    }

    /// 오래 걸리는 요청(routine 명령 등)
    pub async fn request_timeout(&self, body: AgentRequest, wait: Duration) -> Result<AgentResponse, ErrorBody> {
        let id = format!("c{}", self.seq.fetch_add(1, Ordering::Relaxed));
        let input = match &body {
            AgentRequest::Input { session, data } => {
                Some((session.clone(), data.0.len(), data.0.starts_with(b"\x1b[<")))
            }
            _ => None,
        };
        if let Some((session, bytes, true)) = &input {
            tracing::debug!(request = %id, %session, bytes, "pane 입력: host 전달");
        } else if let Some((session, bytes, _)) = &input {
            tracing::info!(request = %id, %session, bytes, "pane 입력: host 전달");
        }
        let (otx, orx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id.clone(), otx);
        let msg = LinkMsg::Req { id: id.clone(), deadline_ms: Some(wait.as_millis() as u64), body };
        if self.tx.json(&msg).await.is_err() {
            self.pending.lock().unwrap().remove(&id);
            if let Some((session, _, _)) = &input {
                tracing::warn!(request = %id, %session, "pane 입력: agent 연결 끊김");
            }
            return Err(ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다"));
        }
        let result = match tokio::time::timeout(wait, orx).await {
            Ok(Ok(o)) => o.into_result(),
            Ok(Err(_)) => Err(ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다")),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                // 무슨 요청이었는지 남긴다(op과 세션만: 입력 바이트 같은 내용은 남기지 않는다)
                let what = match &msg {
                    LinkMsg::Req { body, .. } => serde_json::to_value(body).ok(),
                    _ => None,
                };
                let field =
                    |k: &str| what.as_ref().and_then(|v| v.get(k)?.as_str().map(str::to_string)).unwrap_or_default();
                tracing::warn!(
                    id,
                    op = field("op"),
                    session = field("session"),
                    wait_ms = wait.as_millis() as u64,
                    "agent 응답 시간 초과"
                );
                Err(ErrorBody::new(ErrorCode::Timeout, "agent 응답 시간 초과"))
            }
        };
        if let Some((session, bytes, true)) = &input {
            tracing::debug!(request = %id, %session, bytes, ok = result.is_ok(), "pane 입력: host 확인");
        } else if let Some((session, bytes, _)) = &input {
            tracing::info!(request = %id, %session, bytes, ok = result.is_ok(), "pane 입력: host 확인");
        }
        result
    }

    /// 연결이 끝났을 때 대기 중인 요청을 모두 실패로 끝낸다(다시 보내지 않는다: §8.5 a).
    pub fn fail_all(&self) {
        let drained: Vec<_> = self.pending.lock().unwrap().drain().collect();
        for (_, tx) in drained {
            let _ = tx.send(Outcome::Err(ErrorBody::new(ErrorCode::AgentDisconnected, "agent 연결이 끊겼다")));
        }
    }
}

struct MachineRt {
    name: String,
    local: bool,
    os: Option<String>,
    boot_id: Option<String>,
    dev_mode: bool,
    conn: Option<AgentConn>,
    last_seen_ms: Option<u64>,
    /// 마지막 건강 보고와 그때의 시계 차이(agent - host, ms)
    health: Option<(aaa_proto::MachineHealth, i64)>,
    /// 마지막으로 화면에 보낸 CPU·MEM(값만 바뀐 보고는 움직였을 때만 보낸다, health.rs)
    health_sent: Option<health::HealthSent>,
    /// agent가 hello에서 알린 기능(`cwd.v1` 등). 모르는 요청을 받은 옛 agent는 연결을 끊으므로 보내기 전에 본다.
    features: Vec<String>,
    /// 로컬 agent 프로세스(로컬 socket peer credential). 그 세션에서 띄운 명령(routine 명령)은 떼어 낸 프로세스가 아니다(plan §8.164).
    agent_pid: Option<u32>,
    /// 이 연결에 화면 색을 차례로 보내는 곳(agent 기능 `term_colors.v1`, colors.rs). 놓으면 보내던 task가 끝난다
    colors: Option<tokio::sync::mpsc::UnboundedSender<colors::ColorPush>>,
}

struct Mirror {
    vt: Vt,
    offset: u64,
    synced: bool,
    /// 이 mirror를 채운 snapshot 앞에 session host 기록이 더 있다(plan §8.118): mirror 기록이 끝나면 session host에 묻는다
    source_more: bool,
}

/// 앞 기록 한 덩어리를 어디서 받는가(plan §8.67·§8.118)
pub enum OlderHistory {
    /// host mirror에서 바로(None이면 anchor를 찾지 못했다)
    Ready(Option<aaa_vt::Older>),
    /// mirror에 없다: 그 머신 agent를 거쳐 session host에 묻는다
    Ask(AgentConn),
}

/// 웹 터미널 등 구독자에게 보내는 pane 사건
#[derive(Clone, Debug)]
pub enum PaneEvent {
    Output(u64, Arc<Vec<u8>>),
    /// 화면을 snapshot으로 다시 받아야 한다
    Reset,
    /// 상태(끊김·종료·크기)가 바뀌었다
    State,
}

/// 웹 터미널 attach 결과: 사건 구독, replay와 그 앞 기록이 더 있는지, replay 끝 offset, 크기
pub struct Attach {
    pub rx: broadcast::Receiver<PaneEvent>,
    pub replay: Vec<u8>,
    pub more: bool,
    pub offset: u64,
    pub cols: u16,
    pub rows: u16,
}

struct PaneRt {
    machine_id: String,
    info: SessionInfo,
    mirror: Option<Mirror>,
    events: broadcast::Sender<PaneEvent>,
    /// 지금 쓰는 대화형 front(lease, 연결 하나). PTY 크기를 그 화면이 정한다. 다른 front가 입력·크기·그림을 보내면
    /// 그 front로 바뀌고(마지막 입력이 이긴다), 연결이 끝나거나 놓으면 비운다.
    lease: Option<FrontRef>,
    /// harness 상태(agent의 화면 감지, 옛 agent는 hook 사건)
    agent: Option<aaa_proto::AgentStatus>,
    /// 지금 턴이 시작된 때(작업·질문으로 들어선 harness 시각, notice.rs `turn_from`). 턴 밖이거나 시작을 못 봤으면 None
    turn_from: Option<u64>,
    /// 이 턴에서 완료·확인 필요 보고를 이미 알렸으면 턴 끝남 카드를 중복하지 않는다.
    turn_reported: bool,
    /// 알림이 마지막으로 본 상태(notice.rs `alert_activity`: BG로 기다리는 쉼은 작업 중). 턴의 시작·끝은 이것이 바뀔 때다
    alert: Option<aaa_proto::AgentActivity>,
    /// 지금 막힘(harness 상태 질문)에서 질문에 답했다. 상태가 질문에서 넘어가면 지운다: 늦게 온 질문 사건이 답한 질문을
    /// 다시 열지 않게 한다(harness.rs `on_harness`)
    answered: bool,
    /// Claude 대화 기록 파일(그 머신의 경로, §8.27). 웹·로그로 내보내지 않는다.
    transcript: Option<String>,
    /// 재부팅 뒤 다시 띄우지 못한 까닭(restore.rs)
    restore_error: Option<String>,
    /// 이 pane이 대신한 옛 pane(`replace_pane`, 오래된 것부터 [`REPLACED_KEEP`]개)
    replaced: Vec<String>,
    /// host가 이 세션에 끝내기를 보냈다(`kill_session`). 그렇게 끝난 pane은 스스로 끝난 것이 아니라 닫지 않는다.
    kill_sent: bool,
}

/// 사람이 셸에서 `exit`했다(herdr 0.9.5 `handle_pane_died`: pane 프로세스가 끝나면 그 pane을 닫는다): 실행 중이던 셸 pane이
/// 스스로 끝났다. 다음은 닫지 않는다.
/// - 역할 pane(thread·평가자): 수명은 thread 명령이 정한다(`close_panes`와 같은 규칙).
/// - 프로그램 pane(셸이 아닌 명령, aaa 확장): 끝난 화면과 종료 코드를 남긴다. herdr pane은 늘 셸이다.
/// - host가 끝내기를 보낸 pane: `pane kill`은 다시 띄우려고 남긴다(aaa 확장). 닫기·탭 닫기는 스스로 지운다.
/// - 재부팅(`Interrupted`)·session host 유실(`Lost`): 복원 대상이다(restore.rs). host가 실행 중을 본 적이 없는 세션(host를
///   다시 띄운 뒤 처음 듣는 끝난 세션)도 닫지 않는다.
fn exited_by_itself(old: &SessionInfo, new: &SessionInfo, kill_sent: bool) -> bool {
    old.state.is_running()
        && matches!(new.state, SessionState::Exited { .. })
        && !kill_sent
        && new.role.is_none()
        && restore::is_shell(&new.argv)
}

/// pane이 기억하는 대신한 옛 ID 수
const REPLACED_KEEP: usize = 8;

/// 대화형 front 연결(로컬 front 연결, 웹 터미널 WebSocket 하나)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontRef {
    pub id: String,
    pub label: String,
}

impl FrontRef {
    /// 대화형 front(웹 터미널). 한 번 보내는 CLI는 아니다.
    pub fn interactive(&self) -> bool {
        self.label != aaa_common::front::CLI_CLIENT
    }
}

pub use aaa_proto::DEFAULT_WORKSPACE;
pub use orch::{ProjectScaffold, project_scaffold};

struct Inner {
    machines: HashMap<String, MachineRt>,
    panes: HashMap<String, PaneRt>,
    generation: u64,
    /// 화면들이 알려 준 터미널 색(colors.rs)
    colors: colors::ColorFronts,
}

struct PendingRt {
    machine_id: String,
    fingerprint: String,
    sas: String,
    since_ms: u64,
    attempts: u32,
    accept: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
struct Enroll {
    open_until: Option<Instant>,
    pending: HashMap<String, PendingRt>,
}

/// 아직 쓰지 않은 pairing 코드 하나(herdr-web-ui `DeviceStore.pending`). 새 코드가 앞 코드를 바꾼다.
struct PendingCode {
    code: String,
    role: DeviceRole,
    expires: Instant,
    /// 이 코드로 틀린 수. `PAIR_CODE_ATTEMPTS`에 닿으면 코드를 버린다
    attempts: u32,
}

#[derive(Default)]
struct Pairing {
    pending: Option<PendingCode>,
    /// 코드와 상관없이 틀린 때, `PAIR_FAIL_WINDOW` 안의 것만(§6.4). 창이 지나면 저절로 줄어든다
    failures: VecDeque<Instant>,
}

impl Pairing {
    fn forget_old(&mut self, now: Instant) {
        while self.failures.front().is_some_and(|t| now.saturating_duration_since(*t) >= PAIR_FAIL_WINDOW) {
            self.failures.pop_front();
        }
    }

    /// 다음 시도를 받을 때. 창 안에서 `PAIR_FAIL_BUDGET`번까지는 기다리지 않고, 그 뒤로는 마지막 실패부터
    /// 1초·2초·4초… 60초까지(herdr-web-ui `recordAuthFailure`)
    fn open_at(&self) -> Option<Instant> {
        let n = self.failures.len();
        if n < PAIR_FAIL_BUDGET {
            return None;
        }
        let wait = PAIR_BACKOFF.saturating_mul(1u32 << (n - PAIR_FAIL_BUDGET).min(16)).min(PAIR_BACKOFF_MAX);
        self.failures.back().map(|t| *t + wait)
    }
}

/// pairing 코드 수명(herdr-web-ui `CODE_TTL_MS` 10분)
pub const PAIR_CODE_TTL: Duration = Duration::from_secs(10 * 60);
/// 코드 하나로 틀릴 수 있는 수(참고 `CODE_ATTEMPTS`)
const PAIR_CODE_ATTEMPTS: u32 = 5;
/// 틀린 시도를 세는 창(참고 `SHARED_WINDOW_MS` 10분): 실패가 이보다 오래되면 잊는다
const PAIR_FAIL_WINDOW: Duration = Duration::from_secs(10 * 60);
/// 기다리지 않고 틀릴 수 있는 수(참고 `AUTH_FAILURE_BUDGET`)
const PAIR_FAIL_BUDGET: usize = 5;
/// 그 뒤 기다림의 처음과 상한(참고 `AUTH_BACKOFF_MS` 1초·`AUTH_BACKOFF_MAX_MS` 60초)
const PAIR_BACKOFF: Duration = Duration::from_secs(1);
const PAIR_BACKOFF_MAX: Duration = Duration::from_secs(60);

/// 머신 이름 규칙(등록·로컬 agent가 새 이름을 쓸 때)
pub const MACHINE_NAME_RULE: &str = "머신 이름은 영문·숫자·`.`·`_`·`-`로 1~64자다(--name으로 정한다)";
/// 기기 이름 길이(참고 `LABEL_MAX`)
const DEVICE_LABEL_MAX: usize = 48;
/// 마지막으로 본 때를 적는 간격(참고 `LAST_SEEN_WRITE_MS`)
const DEVICE_SEEN_WRITE_MS: u64 = 60_000;

/// 기기 이름: 빈칸을 하나로 줄이고 48자까지. 비면 `fallback`(참고 `normalizeLabel`)
pub fn normalize_device_label(value: &str, fallback: &str) -> String {
    let label: String = value.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(DEVICE_LABEL_MAX).collect();
    let label = label.trim().to_string();
    if label.is_empty() { fallback.to_string() } else { label }
}

pub struct Host {
    pub cfg: HostConfig,
    accounts: accounts::Accounts,
    pub state: PathBuf,
    pub host_id: String,
    pub db: db::Db,
    pub identity: aaa_common::tls::Identity,
    pub dev_mode: bool,
    /// 실제로 listen한 웹 포트(설정이 `:0`이면 bind 뒤에 정해진다)
    pub web_port: std::sync::atomic::AtomicU16,
    inner: Mutex<Inner>,
    pub changed: tokio_watch::Sender<u64>,
    /// 보고서 목록이 바뀌었다(보내기·열기·지우기·thread 사본, plan §8.256). 웹 events가 안 본 수를 다시 보낸다
    pub reports_changed: tokio_watch::Sender<u64>,
    /// EMFILE 중에는 파일을 더 열지 않고, ticker가 여유가 생긴 뒤 시스템 경고를 저장한다.
    resource_exhausted: std::sync::atomic::AtomicBool,
    enroll: Mutex<Enroll>,
    pairing: Mutex<Pairing>,
    pub agent_listen: Mutex<Option<String>>,
    pub web_listen: Mutex<Option<String>>,
    workspaces: Mutex<HashMap<String, Workspace>>,
    /// workspace가 바뀔 때(front가 구독한다)
    pub layout_events: broadcast::Sender<Workspace>,
    front_seq: AtomicU64,
    /// 이 host 실행의 boot ID(흐름 사건에 같이 적는다)
    pub boot_id: String,
    /// host.toml이 있는 설정 폴더(`run`이 정한다). safety·`[defaults]`는 쓸 때마다 여기서 다시 읽는다(plan §8.163)
    pub config_dir: std::sync::OnceLock<PathBuf>,
    /// 새 흐름 사건(연결된 front에 알린다). 보내는 차례는 seq 차례다(`flow_order`)
    pub(crate) post_gate: std::sync::Mutex<()>,
    deployment_gate: std::sync::Mutex<()>,
    rollout_gate: tokio::sync::Mutex<()>,
    rollout_wake: tokio::sync::Notify,
    rollout_changed: tokio::sync::watch::Sender<u64>,
    rollout_instance: String,
    rollout_started_ms: u64,
    running_sha256: String,
    pub flow_events: broadcast::Sender<aaa_proto::FlowEvent>,
    /// 흐름 사건 하나를 저장(seq를 정함)부터 broadcast까지 한 번에 하나씩 한다(`orch.rs` `flow_event`)
    flow_order: Mutex<()>,
    /// thread 기록이 바뀌었다
    pub thread_events: broadcast::Sender<aaa_proto::ThreadView>,
    /// 보고 사본은 한 번에 하나씩 한다
    collect_lock: tokio::sync::Mutex<()>,
    /// 확인창 질문이 열리거나 바뀌었다
    pub question_events: broadcast::Sender<aaa_proto::QuestionView>,
    /// 앱 안 알림(턴 끝남·새 보고, notice.rs). 기록하지 않는다
    pub notice_events: broadcast::Sender<aaa_proto::NoticeView>,
    /// 프로젝트 TASKS.md가 바뀌었다(프로젝트, 내용 sha256). ticker가 낸다(coord.rs `tasks_pass`)
    pub task_events: broadcast::Sender<(String, String)>,
    /// ticker가 마지막으로 본 프로젝트마다의 TASKS.md 상태
    tasks_seen: Mutex<HashMap<String, coord::TasksSeen>>,
    /// 파일마다의 쓰기 lock(TASKS.md 편집과 ticker의 외부 변경 감지, file_rev.rs)
    file_locks: file_rev::Locks,
    /// ticker 묶음 셈까지 미룬 thread pane의 끝남(pane → 알림, notice.rs `turn_finished`)
    turn_ends: Mutex<HashMap<String, aaa_proto::NoticeView>>,
    /// 질문 저장과 화면 선택 창 붙이기(읽고 쓰기)를 차례로: 답한 질문을 선택 창 붙이기가 다시 열지 않는다
    question_lock: std::sync::Mutex<()>,
    /// 웹 화면들이 함께 쓰는 설정과 고정(prefs.rs). events 연결마다 구독한다
    pub(crate) preferences: prefs::Store,
    /// 취소한 기기 id. 그 기기의 열린 WebSocket(events·pane)이 이것을 듣고 바로 닫는다(§6.4)
    pub device_revoked: broadcast::Sender<String>,
    /// 이 host가 돌면서 취소한 기기. 열린 연결이 사건을 받기 전에 온 입력도 거른다
    revoked_devices: Mutex<std::collections::HashSet<String>>,
    /// pane별 harness 사건 대기자
    waiters: Mutex<harness::Waiters>,
    /// 메시지 전달은 한 번에 하나씩 한다
    deliver_lock: tokio::sync::Mutex<()>,
    /// pane 등록 전에 온 harness 사건
    held: Mutex<harness::Held>,
    /// ticker를 바로 깨운다(흐름 사건·harness 사건)
    tick_wake: tokio::sync::Notify,
    /// 배경 작업을 기다리는 쉼(차이 목록 S1). 잠금 차례는 `inner` 다음이다
    bgwait: Mutex<bgwait::BackgroundWait>,
    /// 대화 기록 변화 push(차이 목록 C3): 청한 연결 수와 agent에 켜 둔 감시. 잠금 차례는 `inner` 다음이다
    chats: Mutex<chatwatch::ChatWatch>,
    chat_notes: broadcast::Sender<chatwatch::ChatNote>,
    /// 감시 맞추기는 하나씩
    chat_sync: tokio::sync::Mutex<()>,
    /// 시작한 tick 수와 끝난 tick 수(`ticker_tick`이 「이 요청 뒤에 시작한 tick」이 끝나기를 기다린다)
    tick_started: std::sync::atomic::AtomicU64,
    /// 이 host가 뜬 시각. auto-resolve는 이때부터도 잰다(herdr-projects ticker `started`: 오래 꺼져 있던 뒤 한꺼번에 끝내지 않는다)
    pub(crate) started_ms: u64,
    tick_done: tokio_watch::Sender<u64>,
    /// 머신별 끊김: 이 host 프로세스가 끊김을 처음 본 때와 알렸는지(inbox::Outage)
    outages: Mutex<HashMap<String, inbox::Outage>>,
    /// 머신별 마지막으로 알린 건강 이상(이어 붙인 글, 정상이면 빈 글)
    health_marks: Mutex<HashMap<String, String>>,
    /// hook 없는 coordinator pane: 마지막 화면 hash와 그 화면이 된 때
    nudge_screens: Mutex<HashMap<String, (String, Instant)>>,
    /// 프로젝트별로 nudge가 기다리는 coordinator pane과 그 입력창이 비어 보인 때(herdr-projects `box_pane`·`box_empty_since`)
    nudge_box: Mutex<HashMap<String, (String, Instant)>>,
    /// task.md Follow-ups 덧붙이기는 한 번에 하나씩
    follow_up_lock: Mutex<()>,
    /// thread 작업 요약 표지 재료(summary.rs): 목록이 매번 파일을 읽지 않게
    summary_marks: summary::MarkMap,
    /// thread의 사용량 한도가 풀리는 때(usage_limit.rs): ticker가 채우고 thread 보기가 읽는다
    usage_limits: usage_limit::LimitMap,
    /// 지금 도는 routine 명령(`<project>/<name>`)
    routine_running: Mutex<HashSet<String>>,
    automation_sources: tokio::sync::Mutex<()>,
    /// PR 추적: 프로젝트별 마지막 조회·도는 중·`gh` 실패, `gh` 계정(pr.rs)
    pr: Mutex<pr::PrMemory>,
    watch: Mutex<watch::Memory>,
    /// 결과물 보기 경로(`/v/<cap>/`) → 대상 thread
    view_caps: Mutex<HashMap<String, library::ViewCap>>,
    library_signing: std::sync::RwLock<library_links::SigningKey>,
    library_ops: std::sync::Mutex<()>,
    /// 파일 보기 읽기 경로(`/f/<cap>/`, plan §8.125)
    file_caps: Mutex<HashMap<String, files::FileCap>>,
    /// Web Push 서명 키(설정이 켜져 있을 때)
    vapid: Option<push::Vapid>,
    /// 구독 사용량(웹이 물을 때만 읽는다)
    usage: usage::UsageService,
    /// 알림 webhook 대기열(설정이 켜져 있을 때)
    notify_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<aaa_proto::InboxItem>>>,
    /// 지금 쓰는(보이고 focus가 있는) 웹 화면 수(events WS `presence`). 하나라도 있으면 Web Push를 보내지 않는다
    pub(crate) in_use: std::sync::atomic::AtomicUsize,
    /// 지금 pane을 보는 웹 화면(「끝남(안 봄)」, seen.rs)
    viewing: Mutex<seen::Viewing>,
    /// 기기마다 최근에 push로 보낸 항목(push.rs `PushedLog`)
    pub(crate) pushed: Mutex<push::PushedLog>,
    /// 사후 평가자: 프로젝트별 다음 묶음 확인 때
    /// 사후 평가자 묶음은 한 번에 하나씩 만든다
    /// 평가 기록 채택·보류는 한 번에 하나씩
    /// thread 기록 다시 읽어 고치기(`update_rec`)는 한 번에 하나씩
    rec_lock: Mutex<()>,
    /// brief 배경 task가 도는 thread(`<project>/<thread>`, 하나만 돈다)
    brief_loops: Mutex<HashSet<String>>,
    thread_ops: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// 지금 brief 한 걸음을 하는 thread(배경 task와 `thread brief`가 겹치지 않게, herdr-projects brief.rs `claim`)
    brief_claims: Mutex<HashSet<String>>,
    /// 보고를 다시 확인하는 중인 thread(`<project>/<thread>`)와 그동안 또 확인을 부탁받았는지. 묶음 셈(transitions.rs)은
    /// 끝날 때까지 이 thread를 건너뛴다(orch.rs `refresh_report`)
    report_refresh: Mutex<HashMap<String, bool>>,
    /// Hello 상태 재생·재부팅 복원 중인 머신. 묶음 셈은 끝날 때까지 그 머신 thread를 건너뛴다(restore.rs)
    restoring: Mutex<HashSet<String>>,
    /// 이 host 실행에서 복원해 본 pane(한 번만 한다)
    restore_tried: Mutex<HashSet<String>>,
}

fn device_view(d: db::DeviceRow) -> aaa_proto::DeviceView {
    aaa_proto::DeviceView {
        role: if d.role == "reader" { DeviceRole::Reader } else { DeviceRole::Controller },
        id: d.id,
        label: d.label,
        created_at_ms: d.created_ms,
        last_seen_ms: d.last_seen_ms,
    }
}

pub fn token_hash(token: &str) -> String {
    hex::encode(sha2::Sha256::digest(token.as_bytes()))
}

/// 끝난 pane을 다시 띄울 때 줄 이름. 사람이 붙인 이름은 agent가 `name`에 덮어 알리므로 그대로 주면 그 이름이 띄울 때
/// 이름이 되어 지워도 돌아가지 않는다. 비워 agent가 명령으로 짓게 하고, 붙인 이름은 `replace_pane`이 label로 다시 붙인다
/// (처음 띄울 때 준 이름은 host가 모른다)
pub(crate) fn respawn_name(info: &SessionInfo) -> String {
    if info.label.is_some() { String::new() } else { info.name.clone() }
}

fn err(code: ErrorCode, msg: impl Into<String>) -> ErrorBody {
    ErrorBody::new(code, msg)
}

/// CLI도 시작·전송 전에 host와 같은 요약 형식을 검사한다. 상태를 읽거나 쓰지 않는다.
pub fn validate_thread_summary(text: &str) -> std::result::Result<(), ErrorBody> {
    summary::parse(text.trim())
        .map(|_| ())
        .map_err(|e| err(ErrorCode::BadRequest, format!("요약 형식: {e}\n형식:\n{}", summary::FORMAT)))
}

impl Host {
    pub fn open(cfg: HostConfig, state: PathBuf) -> Result<Arc<Host>> {
        util::ensure_private_dir(&state)?;
        let accounts = accounts::Accounts::new(&state).map_err(|e| anyhow::anyhow!(e.message))?;
        let db = db::Db::open(&state.join("host.db")).context("host DB")?;
        let library_signing = library_links::SigningKey::open(&state)?;
        let host_id = match db.meta("host_id")? {
            Some(v) => v,
            None => {
                let v = util::new_id();
                db.set_meta("host_id", &v)?;
                v
            }
        };
        let identity = aaa_common::tls::load_or_create(&state, "aaa-host")?;
        let preferences = prefs::Store::new(prefs::parse(db.meta(prefs::META_KEY)?.as_deref()));
        // Web Push 키가 깨졌으면 host는 뜨고 push만 끈다(로그에 남긴다)
        let vapid = if cfg.notify.web_push {
            match push::Vapid::load_or_create(&state) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::error!("Web Push를 끈다: {e:#}");
                    None
                }
            }
        } else {
            None
        };
        let mut machines = HashMap::new();
        for m in db.machines()? {
            machines.insert(
                m.id.clone(),
                MachineRt {
                    name: m.name,
                    local: m.local,
                    os: m.os,
                    boot_id: m.boot_id,
                    dev_mode: false,
                    conn: None,
                    last_seen_ms: m.last_seen_ms,
                    health: None,
                    health_sent: None,
                    features: vec![],
                    agent_pid: None,
                    colors: None,
                },
            );
        }
        let mut workspaces = HashMap::new();
        for (name, json) in db.workspaces()? {
            match serde_json::from_str::<Workspace>(&json) {
                Ok(w) => {
                    workspaces.insert(name, w);
                }
                Err(e) => tracing::warn!(%name, "workspace 기록을 읽지 못했다: {e}"),
            }
        }
        let web_port = cfg.web.listen.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(0);
        let dev_mode = cfg.local_agent.mode == LocalAgentMode::Child;
        Ok(Arc::new(Host {
            accounts,
            cfg,
            state,
            host_id,
            db,
            identity,
            dev_mode,
            web_port: std::sync::atomic::AtomicU16::new(web_port),
            inner: Mutex::new(Inner { machines, panes: HashMap::new(), generation: 0, colors: Default::default() }),
            changed: tokio_watch::channel(0).0,
            reports_changed: tokio_watch::channel(0).0,
            enroll: Mutex::new(Enroll::default()),
            pairing: Mutex::new(Pairing::default()),
            agent_listen: Mutex::new(None),
            web_listen: Mutex::new(None),
            workspaces: Mutex::new(workspaces),
            layout_events: broadcast::channel(64).0,
            front_seq: AtomicU64::new(0),
            boot_id: util::boot_id(),
            config_dir: std::sync::OnceLock::new(),
            flow_events: broadcast::channel(256).0,
            flow_order: Mutex::new(()),
            thread_events: broadcast::channel(256).0,
            collect_lock: tokio::sync::Mutex::new(()),
            question_events: broadcast::channel(256).0,
            post_gate: std::sync::Mutex::new(()),
            deployment_gate: std::sync::Mutex::new(()),
            rollout_gate: tokio::sync::Mutex::new(()),
            rollout_wake: tokio::sync::Notify::new(),
            rollout_changed: tokio::sync::watch::channel(0).0,
            rollout_instance: util::new_id(),
            rollout_started_ms: util::now_ms(),
            running_sha256: rollout::running_sha()?,
            notice_events: broadcast::channel(64).0,
            task_events: broadcast::channel(64).0,
            tasks_seen: Mutex::new(HashMap::new()),
            file_locks: Default::default(),
            turn_ends: Mutex::new(HashMap::new()),
            question_lock: std::sync::Mutex::new(()),
            preferences,
            device_revoked: broadcast::channel(64).0,
            revoked_devices: Mutex::new(Default::default()),
            waiters: Mutex::new(HashMap::new()),
            deliver_lock: tokio::sync::Mutex::new(()),
            held: Mutex::new(HashMap::new()),
            tick_wake: tokio::sync::Notify::new(),
            bgwait: Mutex::new(Default::default()),
            chats: Mutex::new(Default::default()),
            chat_notes: broadcast::channel(256).0,
            chat_sync: tokio::sync::Mutex::new(()),
            tick_started: std::sync::atomic::AtomicU64::new(0),
            started_ms: aaa_common::util::now_ms(),
            tick_done: tokio_watch::channel(0).0,
            outages: Mutex::new(HashMap::new()),
            health_marks: Mutex::new(HashMap::new()),
            nudge_screens: Mutex::new(HashMap::new()),
            nudge_box: Mutex::new(HashMap::new()),
            follow_up_lock: Mutex::new(()),
            summary_marks: Mutex::new(HashMap::new()),
            usage_limits: Mutex::new(HashMap::new()),
            routine_running: Mutex::new(HashSet::new()),
            automation_sources: tokio::sync::Mutex::new(()),
            pr: Mutex::new(pr::PrMemory::default()),
            watch: Mutex::new(watch::Memory::default()),
            view_caps: Mutex::new(HashMap::new()),
            library_signing: std::sync::RwLock::new(library_signing),
            library_ops: std::sync::Mutex::new(()),
            file_caps: Mutex::new(HashMap::new()),
            vapid,
            usage: usage::UsageService::new(usage::UsageContext::system()),
            notify_tx: Mutex::new(None),
            in_use: std::sync::atomic::AtomicUsize::new(0),
            viewing: Mutex::new(seen::Viewing::default()),
            pushed: Mutex::new(push::PushedLog::default()),
            rec_lock: Mutex::new(()),
            brief_loops: Mutex::new(HashSet::new()),
            thread_ops: Mutex::new(HashMap::new()),
            brief_claims: Mutex::new(HashSet::new()),
            resource_exhausted: std::sync::atomic::AtomicBool::new(false),
            report_refresh: Mutex::new(HashMap::new()),
            restoring: Mutex::new(HashSet::new()),
            restore_tried: Mutex::new(HashSet::new()),
        }))
    }

    fn bump(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    /// 보고서 목록이 바뀌었다(plan §8.256)
    pub(crate) fn reports_bump(&self) {
        self.reports_changed.send_modify(|v| *v += 1);
    }

    // ───────────── agent 연결 ─────────────

    pub fn next_gen(&self) -> u64 {
        let mut i = self.inner.lock().unwrap();
        i.generation += 1;
        i.generation
    }

    /// hello를 받아들였다: 머신·pane 목록을 갱신하고 모든 세션을 구독한다.
    pub fn agent_connected(self: &Arc<Self>, hello: &aaa_proto::AgentHello, local: bool, conn: AgentConn) {
        let _ = self.db.touch_machine(&hello.machine_id, &hello.os, &hello.boot_id);
        let generation = conn.generation;
        // 연결을 공개하기 전에 상태 복원 울타리를 친다. host 재시작 뒤에는 실행 중인 pane도 아직 agent=None이다.
        // Hello의 모든 harness 상태를 재생하기 전 ticker가 보면 쉼(no agent)라는 거짓 전이가 생긴다.
        // restore_after_reboot이 재부팅 복원까지 이어 맡고, 되살릴 pane이 없으면 바로 푼다.
        self.restoring.lock().unwrap().insert(hello.machine_id.clone());
        let subscribe: Vec<(String, Option<u64>)>;
        // agent가 내려간 사이 스스로 끝난 셸 pane(host는 그 전에 실행 중을 봤다)
        let mut exited = Vec::new();
        {
            let mut i = self.inner.lock().unwrap();
            if let Some(old) = i.machines.get(&hello.machine_id).and_then(|m| m.conn.clone()) {
                old.fail_all();
            }
            // 지금 화면 색부터 차례로 보낸다(inner lock 안이라 바뀐 색이 이 앞에 끼지 않는다)
            let current = i.colors.current;
            let pushes = hello
                .features
                .iter()
                .any(|f| f == "term_colors.v1")
                .then(|| colors::spawn_pushes(conn.clone(), current));
            let m = i.machines.entry(hello.machine_id.clone()).or_insert(MachineRt {
                name: hello.machine_name.clone(),
                local,
                os: None,
                boot_id: None,
                dev_mode: false,
                conn: None,
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: vec![],
                agent_pid: None,
                colors: None,
            });
            m.colors = pushes;
            m.os = Some(hello.os.clone());
            m.boot_id = Some(hello.boot_id.clone());
            m.dev_mode = hello.dev_mode;
            m.features = hello.features.clone();
            m.conn = Some(conn);
            // 다시 붙은 agent는 설정이 바뀌었을 수 있다. 새 보고를 기다린다.
            m.health = None;
            m.health_sent = None;
            m.last_seen_ms = Some(util::now_ms());
            let ids: std::collections::HashSet<_> = hello.sessions.iter().map(|s| s.id.clone()).collect();
            i.panes.retain(|id, p| p.machine_id != hello.machine_id || ids.contains(id));
            for s in &hello.sessions {
                let p = i.panes.entry(s.id.clone()).or_insert_with(|| PaneRt {
                    machine_id: hello.machine_id.clone(),
                    info: s.clone(),
                    mirror: None,
                    events: broadcast::channel(1024).0,
                    lease: None,
                    agent: None,
                    turn_from: None,
                    turn_reported: false,
                    alert: None,
                    transcript: None,
                    answered: false,
                    restore_error: None,
                    kill_sent: false,
                    replaced: Vec::new(),
                });
                if exited_by_itself(&p.info, s, p.kill_sent) {
                    exited.push(s.id.clone());
                }
                p.info = s.clone();
                if let Some(m) = p.mirror.as_mut() {
                    m.synced = false;
                }
                let _ = p.events.send(PaneEvent::State);
            }
            subscribe = hello
                .sessions
                .iter()
                .map(|s| (s.id.clone(), i.panes.get(&s.id).and_then(|p| p.mirror.as_ref()).map(|m| m.offset)))
                .collect();
        }
        self.bump();
        for id in exited {
            self.spawn_close_exited(id, hello.machine_id.clone());
        }
        // host가 내려간 사이 끝났거나 지워진 이 머신 pane의 질문을 닫는다(agent가 세션을 모두 알렸다)
        let open = self.questions(false);
        let gone: Vec<String> = {
            let i = self.inner.lock().unwrap();
            open.into_iter()
                .filter(|q| q.machine == hello.machine_name)
                .filter(|q| !i.panes.get(&q.pane).is_some_and(|p| p.info.state.is_running()))
                .map(|q| q.pane)
                .collect()
        };
        for pane in gone {
            self.close_questions_of(&pane);
        }
        // agent가 남겨 둔 세션별 마지막 harness 상태(재부팅 복원이 harness 세션 ID를 바로 안다)
        for ev in &hello.harness {
            self.on_harness(&hello.machine_id, ev.clone());
        }
        for (sid, from) in subscribe {
            self.replay_harness(&sid);
            self.spawn_subscribe(hello.machine_id.clone(), sid, from);
        }
        self.rewatch(hello.machine_id.clone());
        self.sync_chat_watches_if_wanted();
        self.restore_after_reboot(&hello.machine_id, generation);
        self.bump();
        self.tick_wake.notify_one();
    }

    pub fn agent_disconnected(&self, machine_id: &str, generation: u64) {
        let mut i = self.inner.lock().unwrap();
        let Some(m) = i.machines.get_mut(machine_id) else { return };
        if m.conn.as_ref().map(|c| c.generation) != Some(generation) {
            return; // 이미 새 연결로 바뀌었다
        }
        if let Some(c) = m.conn.take() {
            c.fail_all();
        }
        m.colors = None;
        m.last_seen_ms = Some(util::now_ms());
        for p in i.panes.values_mut().filter(|p| p.machine_id == machine_id) {
            if let Some(mm) = p.mirror.as_mut() {
                mm.synced = false;
            }
            let _ = p.events.send(PaneEvent::State);
        }
        drop(i);
        self.chat_machine_gone(machine_id);
        self.bump();
    }

    fn conn_of(&self, machine_id: &str) -> Option<AgentConn> {
        self.inner.lock().unwrap().machines.get(machine_id).and_then(|m| m.conn.clone())
    }

    fn spawn_subscribe(self: &Arc<Self>, machine_id: String, session: String, from: Option<u64>) {
        let me = self.clone();
        tokio::spawn(async move {
            let Some(conn) = me.conn_of(&machine_id) else { return };
            if let Err(e) = conn.request(AgentRequest::Subscribe { session: session.clone(), from }).await {
                tracing::debug!(%session, "구독 실패: {e}");
            }
        });
    }

    /// 링크 reader가 순서대로 부른다: 구독 응답은 출력 chunk보다 먼저 mirror에 반영된다.
    pub fn on_subscribed(&self, session: &str, snapshot: Option<aaa_proto::Snapshot>, offset: u64) {
        let mut i = self.inner.lock().unwrap();
        let Some(p) = i.panes.get_mut(session) else { return };
        match snapshot {
            // 끝난 세션인데 agent에 마지막 화면 파일이 없다(재부팅 등으로 session host가 기록 없이 사라짐):
            // host가 가진 마지막 화면을 그대로 둔다(§4.4 「마지막 snapshot과 tail」).
            Some(s) if s.replay.0.is_empty() && !p.info.state.is_running() && p.mirror.is_some() => {
                if let Some(m) = p.mirror.as_mut() {
                    m.synced = true;
                }
            }
            Some(s) => {
                let mut vt = Vt::new(s.rows, s.cols);
                vt.feed(&s.replay.0);
                p.mirror = Some(Mirror { vt, offset: s.offset, synced: true, source_more: s.history_more });
                let _ = p.events.send(PaneEvent::Reset);
            }
            None => match p.mirror.as_mut() {
                Some(m) if m.offset == offset => m.synced = true,
                _ => tracing::warn!(%session, "구독 offset이 mirror와 다르다"),
            },
        }
    }

    pub fn on_chunk(self: &Arc<Self>, machine_id: &str, c: TerminalChunk) {
        let resync = {
            let mut i = self.inner.lock().unwrap();
            let Some(p) = i.panes.get_mut(&c.session) else { return };
            let Some(m) = p.mirror.as_mut() else { return };
            if !m.synced {
                return;
            }
            let end = c.offset + c.data.len() as u64;
            if end <= m.offset {
                return;
            }
            if c.offset > m.offset {
                m.synced = false;
                Some(m.offset)
            } else {
                let data = c.data[(m.offset - c.offset) as usize..].to_vec();
                let start = m.offset;
                m.vt.feed(&data);
                m.offset = end;
                let _ = p.events.send(PaneEvent::Output(start, Arc::new(data)));
                None
            }
        };
        if let Some(from) = resync {
            tracing::warn!(session = %c.session, "출력에 빈틈: 다시 구독한다");
            self.spawn_subscribe(machine_id.to_string(), c.session, Some(from));
        }
    }

    pub fn on_session(self: &Arc<Self>, machine_id: &str, info: SessionInfo, started: bool) {
        let closing = {
            let mut i = self.inner.lock().unwrap();
            let p = i.panes.entry(info.id.clone()).or_insert_with(|| PaneRt {
                machine_id: machine_id.to_string(),
                info: info.clone(),
                mirror: None,
                events: broadcast::channel(1024).0,
                lease: None,
                agent: None,
                turn_from: None,
                turn_reported: false,
                alert: None,
                transcript: None,
                answered: false,
                restore_error: None,
                kill_sent: false,
                replaced: Vec::new(),
            });
            if let Some(m) = p.mirror.as_mut()
                && (p.info.cols, p.info.rows) != (info.cols, info.rows)
            {
                m.vt.resize(info.rows, info.cols);
            }
            let exited = exited_by_itself(&p.info, &info, p.kill_sent);
            p.info = info.clone();
            let _ = p.events.send(PaneEvent::State);
            exited
        };
        if closing {
            self.spawn_close_exited(info.id.clone(), machine_id.to_string());
        }
        self.bump();
        if !info.state.is_running() {
            self.close_questions_of(&info.id);
        }
        self.replay_harness(&info.id);
        // 끝난 세션은 thread 묶음을 바꾼다(transitions.rs): 다음 주기를 기다리지 않는다. 끝나기 직전에 쓴 보고를 먼저 본다.
        if !info.state.is_running() {
            match self.thread_of_pane(&info.id) {
                Some(r) if r.status == aaa_proto::ThreadStatus::Open => self.refresh_report(&r.project, &r.id),
                _ => self.tick_wake.notify_one(),
            }
        }
        if started {
            self.spawn_subscribe(machine_id.to_string(), info.id, None);
        }
    }

    pub fn on_resync(self: &Arc<Self>, machine_id: &str, session: &str) {
        let from = {
            let mut i = self.inner.lock().unwrap();
            i.panes.get_mut(session).and_then(|p| p.mirror.as_mut()).map(|m| {
                m.synced = false;
                m.offset
            })
        };
        self.spawn_subscribe(machine_id.to_string(), session.to_string(), from);
    }

    // ───────────── 보기 ─────────────

    pub fn machine_views(&self) -> Vec<MachineView> {
        let availability = self.pr.lock().unwrap().availability.clone();
        let i = self.inner.lock().unwrap();
        let mut v: Vec<_> = i
            .machines
            .iter()
            .map(|(id, m)| MachineView {
                pr_tracking: availability.get(id).cloned(),
                id: id.clone(),
                name: m.name.clone(),
                local: m.local,
                connection: if m.conn.is_some() { Connection::Connected } else { Connection::Disconnected },
                os: m.os.clone(),
                boot_id: m.boot_id.clone(),
                dev_mode: m.dev_mode,
                last_seen_ms: m.last_seen_ms,
                health: m.health.as_ref().map(|h| h.0.clone()),
                clock_skew_ms: m.health.as_ref().map(|h| h.1),
                problems: health::problems(m),
            })
            .collect();
        v.sort_by(|a, b| b.local.cmp(&a.local).then(a.name.cmp(&b.name)));
        v
    }

    fn pane_view(
        i: &Inner,
        id: &str,
        p: &PaneRt,
        role: DeviceRole,
        projects: &[(String, std::path::PathBuf)],
    ) -> PaneView {
        let m = i.machines.get(&p.machine_id);
        let connected = m.is_some_and(|m| m.conn.is_some());
        // herdr-projects coordinator::is_coordinator + report_tokens: host 머신의 프로젝트 홈 agent만 소속을 붙인다.
        // role 없는 기존 agent도 포함하지만, 같은 경로의 원격 pane·사람 셸은 coordinator가 아니다.
        let coordinator_project = if m.is_some_and(|m| m.local)
            && (p.info.role == Some(aaa_proto::Role::Coordinator)
                || p.agent.is_some()
                || crate::adopt::harness_of(p.info.harness.as_deref(), &p.info.argv).is_some())
        {
            p.info
                .cwd
                .as_deref()
                .and_then(|cwd| std::fs::canonicalize(cwd).ok())
                .and_then(|cwd| projects.iter().find(|(_, dir)| *dir == cwd).map(|(name, _)| name.clone()))
        } else {
            None
        };
        PaneView {
            id: id.to_string(),
            machine_id: p.machine_id.clone(),
            machine: m.map(|m| m.name.clone()).unwrap_or_default(),
            name: p.info.name.clone(),
            argv: p.info.argv.clone(),
            cwd: p.info.cwd.clone(),
            state: p.info.state.clone(),
            connection: if connected { Connection::Connected } else { Connection::Disconnected },
            readonly: !connected || !p.info.state.is_running() || !role.can_control(),
            cols: p.info.cols,
            rows: p.info.rows,
            lease: p.lease.as_ref().map(|l| LeaseView { front: l.id.clone(), label: l.label.clone() }),
            role: p.info.role,
            coordinator_project,
            agent: p.agent.clone().map(Box::new),
            label: p.info.label.clone(),
            restore_error: p.restore_error.clone(),
            replaced: p.replaced.clone(),
        }
    }

    pub fn pane_views(&self, role: DeviceRole) -> Vec<PaneView> {
        let projects = self.coordinator_projects();
        let i = self.inner.lock().unwrap();
        let mut v: Vec<_> = i.panes.iter().map(|(id, p)| Self::pane_view(&i, id, p, role, &projects)).collect();
        // 처음 띄운 차례(ULID): 재개·재부팅 복원으로 새 ID가 되어도 처음 ID 자리에 남는다(목록이 옮겨 다니지 않는다)
        let first = |p: &PaneView| p.replaced.first().cloned().unwrap_or_else(|| p.id.clone());
        v.sort_by(|a, b| a.machine.cmp(&b.machine).then_with(|| first(a).cmp(&first(b))));
        v
    }

    pub fn pane_view_of(&self, id: &str, role: DeviceRole) -> Option<PaneView> {
        let projects = self.coordinator_projects();
        let i = self.inner.lock().unwrap();
        i.panes.get(id).map(|p| Self::pane_view(&i, id, p, role, &projects))
    }

    fn coordinator_projects(&self) -> Vec<(String, std::path::PathBuf)> {
        self.project_directories()
            .into_iter()
            .filter(|(name, _)| self.project_status(name) != aaa_proto::ProjectStatus::Archived)
            .filter_map(|(name, dir)| std::fs::canonicalize(dir).ok().map(|dir| (name, dir)))
            .collect()
    }

    /// 웹 터미널 attach: 사건 구독과 snapshot을 같은 잠금 안에서 잡아 빈틈이 없게 한다.
    pub fn attach_pane(&self, id: &str) -> Option<Attach> {
        let i = self.inner.lock().unwrap();
        let p = i.panes.get(id)?;
        let rx = p.events.subscribe();
        let (replay, more, offset, rows, cols) = match &p.mirror {
            Some(m) => {
                let (rows, cols) = m.vt.size();
                let (replay, more) = m.vt.replay_more();
                (replay, more || m.source_more, m.offset, rows, cols)
            }
            None => (Vec::new(), false, 0, p.info.rows, p.info.cols),
        };
        Some(Attach { rx, replay, more, offset, cols, rows })
    }

    /// 웹이 위로 굴려 받은 기록 끝에 닿았다: host mirror 기록에서 anchor 바로 앞 덩어리(plan §8.67).
    /// mirror는 처음 구독 snapshot(최근 2000줄)부터 채워진다. 그보다 앞은 session host에만 있으므로, mirror에서 더 줄 것이
    /// 없으면 session host에 묻는다(plan §8.118, QA t-0337 Q8). 옛 agent·session host면 묻지 않는다.
    pub fn older_history(&self, id: &str, anchor: &[String], depth: usize) -> OlderHistory {
        let i = self.inner.lock().unwrap();
        let Some(p) = i.panes.get(id) else { return OlderHistory::Ready(None) };
        let Some(m) = p.mirror.as_ref() else { return OlderHistory::Ready(None) };
        let found = m.vt.older(anchor, depth, aaa_vt::REPLAY_HISTORY);
        match found {
            Some(mut o) if o.lines > 0 => {
                o.more |= m.source_more;
                OlderHistory::Ready(Some(o))
            }
            found if m.source_more => {
                let conn = i
                    .machines
                    .get(&p.machine_id)
                    .filter(|mc| mc.features.iter().any(|f| f == "history.v1"))
                    .and_then(|mc| mc.conn.clone());
                match conn {
                    Some(c) => OlderHistory::Ask(c),
                    None => OlderHistory::Ready(found),
                }
            }
            found => OlderHistory::Ready(found),
        }
    }

    /// session host에 앞 기록을 묻는다(plan §8.118). 실패하면 None(웹은 더 받지 않는다).
    pub async fn older_from_agent(
        conn: &AgentConn,
        session: String,
        anchor: Vec<String>,
        depth: usize,
    ) -> Option<aaa_vt::Older> {
        let depth = u32::try_from(depth).unwrap_or(u32::MAX);
        match conn.request(AgentRequest::History { session, anchor, depth }).await {
            Ok(AgentResponse::History(c)) if c.found => {
                Some(aaa_vt::Older { data: c.data.0, lines: c.lines as usize, more: c.more })
            }
            _ => None,
        }
    }

    fn resolve_pane(&self, prefix: &str) -> Result<(String, String), ErrorBody> {
        let i = self.inner.lock().unwrap();
        if let Some(p) = i.panes.get(prefix) {
            return Ok((prefix.to_string(), p.machine_id.clone()));
        }
        let up = prefix.to_ascii_uppercase();
        let mut hits: Vec<_> = i.panes.iter().filter(|(id, _)| prefix.len() >= 4 && id.starts_with(&up)).collect();
        if hits.is_empty() {
            // ID가 아니면 이름: 실행 중인 것을 먼저, 없으면 끝난 것까지
            let named: Vec<_> = i.panes.iter().filter(|(_, p)| p.info.name == prefix).collect();
            let running: Vec<_> = named.iter().copied().filter(|(_, p)| p.info.state.is_running()).collect();
            hits = if running.is_empty() { named } else { running };
        }
        match hits.as_slice() {
            [(id, p)] => Ok(((*id).clone(), p.machine_id.clone())),
            [] => Err(err(ErrorCode::NotFound, format!("pane이 없다: {prefix}"))),
            _ => Err(err(ErrorCode::BadRequest, format!("pane ID가 모호하다: {prefix}"))),
        }
    }

    fn resolve_machine(&self, name: Option<&str>) -> Result<String, ErrorBody> {
        let i = self.inner.lock().unwrap();
        let found = match name {
            Some(n) => i.machines.iter().find(|(id, m)| m.name == n || id.as_str() == n),
            None => i.machines.iter().find(|(_, m)| m.local),
        };
        found.map(|(id, _)| id.clone()).ok_or_else(|| {
            err(
                ErrorCode::NotFound,
                match name {
                    Some(n) => format!("머신이 없다: {n}"),
                    None => "로컬 agent가 없다. --machine으로 머신을 고른다".into(),
                },
            )
        })
    }

    fn live_conn(&self, machine_id: &str) -> Result<AgentConn, ErrorBody> {
        self.conn_of(machine_id).ok_or_else(|| err(ErrorCode::AgentDisconnected, "agent와 연결이 끊겨 있다(읽기 전용)"))
    }

    fn mirror_text(&self, id: &str) -> Option<ScreenText> {
        let i = self.inner.lock().unwrap();
        i.panes.get(id).and_then(|p| p.mirror.as_ref()).map(|m| m.vt.text(id))
    }

    // ───────────── Front API ─────────────

    pub async fn call(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        req: FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        self.call_from(role, actor, None, req).await
    }

    /// `front`는 대화형 연결(웹 터미널)일 때만 준다. 조작 lease는 이 연결에 묶인다.
    pub async fn call_from(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        front: Option<&FrontRef>,
        req: FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        if matches!(&req, FrontRequest::AccountImport { .. } | FrontRequest::AccountRemove { .. })
            && (role != DeviceRole::Local || front::in_session(actor) || !actor.starts_with("local:"))
        {
            return Err(err(ErrorCode::Forbidden, "계정 등록·삭제는 host의 aaa 밖 터미널에서 한다"));
        }
        let req = if matches!(&req, FrontRequest::AccountImport { .. }) { req } else { aaa_common::redact::json(req) };
        // 기기·머신 신뢰와 공개 범위는 사람이 바꾼다. coordinator도 Local이지만 세션 안이다.
        if matches!(
            req,
            FrontRequest::WebPair { .. }
                | FrontRequest::EnrollOpen { .. }
                | FrontRequest::EnrollApprove { .. }
                | FrontRequest::MachineRevoke { .. }
                | FrontRequest::DeviceRevoke { .. }
                | FrontRequest::WebPublish
                | FrontRequest::WebPublishOff
        ) && (front::in_session(actor)
            || actor.starts_with("worker:")
            || actor.starts_with("evaluator:")
            || actor.starts_with("detached:"))
        {
            return Err(err(
                ErrorCode::Forbidden,
                "기기·머신 등록과 폐기·웹 공개 변경은 사람이 aaa 밖 터미널이나 웹에서 한다",
            ));
        }
        if req.local_only() && role != DeviceRole::Local {
            return Err(err(ErrorCode::Forbidden, "이 머신의 CLI에서만 할 수 있다"));
        }
        if req.needs_control() && !role.can_control() {
            return Err(err(
                ErrorCode::Forbidden,
                format!("보기 권한으로는 조작할 수 없다{}", front::reader_reason(actor)),
            ));
        }
        let (mut req, selected_account) = match req {
            FrontRequest::ThreadStartAccount { account, start } => {
                if !matches!(&*start, FrontRequest::ThreadStart { .. }) {
                    return Err(err(ErrorCode::BadRequest, "계정 지정은 thread 시작 요청에만 쓴다"));
                }
                (*start, Some(account))
            }
            other => (other, None),
        };
        // 프로젝트를 생략한 요청(FRONT_API 1.50, plan §8.288): host가 맡은 프로젝트 하나로 채운다
        if let Some(project) = req.project_mut()
            && project.is_empty()
        {
            *project = self.instance_project()?;
        }
        match req {
            FrontRequest::AccountImport { name, harness, secret } => {
                self.accounts.import(&name, &harness, secret.0.trim())?;
                self.db.audit(actor, "account_import", &name);
                Ok(FrontResponse::Accounts { accounts: self.accounts.list()? })
            }
            FrontRequest::AccountList => Ok(FrontResponse::Accounts { accounts: self.accounts.list()? }),
            FrontRequest::AccountRemove { name } => {
                self.accounts.remove(&name)?;
                self.db.audit(actor, "account_remove", &name);
                Ok(FrontResponse::Done)
            }
            FrontRequest::ThreadStartAccount { .. } => unreachable!(),
            FrontRequest::Instance => Ok(FrontResponse::Instance { project: self.instance_view() }),
            FrontRequest::DebugInput { pane, thread, since_ms } => self.debug_input(pane, thread, since_ms),
            FrontRequest::RolloutStart { id, spec } => self.rollout_start(actor, id, spec).await,
            FrontRequest::RolloutStatus { id } => self.rollout_status(actor, id),
            FrontRequest::DeployContext => Ok(self.deployment_context(actor)),
            FrontRequest::DeployStart { machine, id, action } => {
                self.deployment_start(actor, machine, id, action).await
            }
            FrontRequest::DeployStatus { machine, id } => self.deployment_status(actor, machine, id).await,
            FrontRequest::Post { project, input } => {
                self.front_post(&project, input, actor).map(|result| FrontResponse::Post { result })
            }
            FrontRequest::PostStatus { project, post_id } => {
                self.post_status(&project, &post_id, actor).map(|result| FrontResponse::Post { result })
            }
            FrontRequest::PostList { project } => self
                .db
                .post_list(&project)
                .map(|posts| FrontResponse::Posts { posts })
                .map_err(|e| err(ErrorCode::Internal, e.to_string())),
            FrontRequest::PostUnmute { project, thread } => self.post_unmute(&project, &thread, actor),
            FrontRequest::Status => Ok(FrontResponse::Status(HostStatus {
                host_id: self.host_id.clone(),
                software: util::software(),
                proto: FRONT_API,
                dev_mode: self.dev_mode,
                agent_listen: self.agent_listen.lock().unwrap().clone(),
                web_listen: self.web_listen.lock().unwrap().clone(),
                machines: self.machine_views(),
            })),
            FrontRequest::Machines => Ok(FrontResponse::Machines { machines: self.machine_views() }),
            FrontRequest::Panes { role: want } => {
                // 기본은 평가자 세션을 빼고, 역할을 주면 그 역할만(§8.13)
                let panes = self
                    .pane_views(role)
                    .into_iter()
                    .filter(|p| match want {
                        None => true,
                        Some(r) => p.role == Some(r),
                    })
                    .collect();
                Ok(FrontResponse::Panes { panes })
            }
            FrontRequest::ProjectCreate { name, goal, repos, thread_profile, coordinator_profile } => self
                .project_create(
                    &name,
                    orch::ProjectOptions { goal, repos, thread_profile, coordinator_profile },
                    actor,
                ),
            FrontRequest::ProjectSet { project, key, value } => self.project_set(&project, &key, &value, actor),
            FrontRequest::Projects { all } => {
                Ok(FrontResponse::Projects { projects: if all { self.projects_all() } else { self.projects() } })
            }
            FrontRequest::ProjectLifecycle { project, action } => self.project_lifecycle(&project, action, actor).await,
            FrontRequest::ProjectDelete { project, force } => self.project_delete(&project, force, actor).await,
            FrontRequest::ProjectRename { project, to, dry_run } => {
                self.project_rename(&project, &to, dry_run, actor).await
            }
            FrontRequest::ProjectSweep { project, dry_run } => self.project_sweep(&project, dry_run, actor).await,
            FrontRequest::ProjectFiles { fix } => Ok(self.project_files(fix, actor)),
            FrontRequest::ProjectOpen { project, profile, new } => {
                self.project_open(&project, profile, new, actor).await
            }
            FrontRequest::Profiles { machine } => self.profiles(machine.as_deref()).await,
            FrontRequest::ProfileResolve { name } => self.profile_resolve(name.as_deref()),
            FrontRequest::ThreadStart {
                holds,
                project,
                title,
                machine,
                profile,
                kind,
                repo,
                base,
                task,
                from_task,
                background,
                link_task,
                task_block,
            } => {
                // herdr-projects threads.rs `placement`: repo가 있으면 worktree, 없으면 repo 없는 작업 폴더
                let kind = kind.unwrap_or(if repo.is_some() {
                    aaa_proto::ThreadKind::Worktree
                } else {
                    aaa_proto::ThreadKind::Dir
                });
                let mut a = orch::StartArgs {
                    account: selected_account,
                    holds,
                    project,
                    title,
                    machine,
                    profile,
                    kind,
                    repo,
                    base,
                    task,
                    role: aaa_proto::Role::Worker,
                };
                let (warning, picked) = match from_task {
                    Some(t) => {
                        let (w, t) = self.apply_from_task(&mut a, &t, task_block.as_deref()).await?;
                        (w, Some(t))
                    }
                    None => (None, None),
                };
                // 머신을 주지 않았으면 project.toml repos에 적힌 그 repo의 머신(herdr-projects `start`의 `listed`)
                if a.machine.is_none() {
                    let repos = self.settings(&a.project).repos;
                    a.machine =
                        a.repo.as_ref().and_then(|r| repos.into_iter().find(|x| &x.path == r)).and_then(|x| x.machine);
                }
                let project = a.project.clone();
                let mut v = if background {
                    self.thread_start_mode(a, actor, true).await?
                } else {
                    self.thread_start(a, actor).await?
                };
                v.warnings.extend(warning);
                // 시작에 성공했으면 그 task 줄 끝에 thread를 잇는다(할 일 T3, 시작할 때 읽은 블록으로)
                if link_task && let Some(t) = &picked {
                    v.warnings.extend(self.link_started(&project, t, &v.id, actor).await);
                }
                Ok(FrontResponse::Thread(v))
            }
            FrontRequest::ThreadStartPreview { project, from_task, task_block } => {
                self.thread_start_preview(&project, &from_task, task_block.as_deref()).await
            }
            FrontRequest::ThreadAdopt { project, pane, title, task } => {
                self.thread_adopt(&project, &pane, &title, &task, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::ThreadNext { project, thread, line, add } => {
                self.thread_next(&project, &thread, line, add, actor).await
            }
            FrontRequest::Context { project, peek } => self.context(&project, peek).await.map(|mut c| {
                if role.can_control() {
                    self.library_notice_links(&mut c.inbox);
                }
                FrontResponse::Context(c)
            }),
            FrontRequest::Tasks { project } => self.task_board(&project).map(FrontResponse::TaskBoard),
            FrontRequest::TaskEdit { project, ops, base_sha256 } => {
                self.task_edit(&project, ops, base_sha256, actor).await
            }
            FrontRequest::TaskLog { project, before, limit, filter } => self.task_log(&project, before, limit, filter),
            FrontRequest::TaskLogPrune { project, before_ms } => self.task_log_prune(&project, before_ms, actor),
            FrontRequest::Assignable { project, check } => {
                self.project_dir(&project)?;
                let owners = self.assignable(&project).await;
                let valid = match check {
                    Some(c) => Some(self.check_owner(&owners, &c)?),
                    None => None,
                };
                Ok(FrontResponse::Assignable { owners, valid })
            }
            FrontRequest::Threads { project, role: want } => {
                self.threads_of(project.as_deref(), want).map(|threads| FrontResponse::Threads { threads })
            }
            FrontRequest::ThreadShow { project, thread } => {
                self.thread_show(&project, &thread).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadSummary { project, thread } => {
                self.thread_summary(&project, &thread).map(FrontResponse::ThreadSummary)
            }
            FrontRequest::ThreadSummarySet { project, thread, text } => {
                self.thread_summary_set(&project, &thread, &text, actor).map(FrontResponse::ThreadSummary)
            }
            FrontRequest::ThreadLibrary { project, thread } => self
                .thread_library(&project, &thread, actor.strip_prefix("device:"), role.can_control())
                .map(FrontResponse::Library),
            FrontRequest::LibraryPublish { project, name, source, live, to_user, title, from_thread } => {
                let to_user = to_user.then_some(title.as_deref());
                self.library_publish(&project, &name, &source, live, to_user, from_thread.as_deref(), actor)
            }
            FrontRequest::LibraryDelete { reports } => self.library_delete(&reports, actor),
            FrontRequest::LibraryImport { project, source, prefix } => {
                self.library_import(&project, &source, &prefix, actor)
            }
            FrontRequest::LibraryPrune { project, older_than_days } => {
                self.library_prune_at(project.as_deref(), older_than_days, util::now_ms(), actor)
            }
            FrontRequest::LibraryPrunePreview { project, older_than_days } => self
                .library_prune_preview(project.as_deref(), older_than_days, util::now_ms())
                .map(FrontResponse::LibraryPrunePreview),
            FrontRequest::LibraryRotate => self.library_rotate(actor),
            FrontRequest::LibraryReports { project } => self
                .library_reports(project.as_deref(), role.can_control())
                .map(|reports| FrontResponse::LibraryReports { reports }),
            FrontRequest::LibraryOpen { project, name } => {
                let view = self.report_library(&project, &name, actor.strip_prefix("device:"), role.can_control())?;
                self.library_opened(&project, &name);
                Ok(FrontResponse::Library(view))
            }
            FrontRequest::LibraryUsage => self.library_usage(),
            FrontRequest::LibraryLink { project, thread, days } => {
                self.library_link(&project, &thread, days, actor).map(FrontResponse::LibraryLink)
            }
            FrontRequest::LibraryLinks { project } => {
                self.library_links(project.as_deref()).map(|links| FrontResponse::LibraryLinks { links })
            }
            FrontRequest::LibraryLinkRevoke { id } => {
                self.library_link_revoke(&id, actor).map(FrontResponse::LibraryLink)
            }
            FrontRequest::ThreadAck { project, thread } => {
                self.thread_ack(&project, &thread, actor).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadResolve { project, thread, reopen: true, .. } => {
                self.thread_reopen(&project, &thread, actor).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadResolve {
                project,
                thread,
                keep_worktree,
                reopen: false,
                skip_copy,
                discard_uncopied,
            } => self
                .thread_resolve(
                    &project,
                    &thread,
                    orch::ResolveOpts { keep_worktree, skip_copy, discard_uncopied },
                    orch::ResolveWhy::Manual,
                    actor,
                )
                .await
                .map(FrontResponse::Thread),
            FrontRequest::ThreadStop { project, thread } => {
                self.thread_stop(&project, &thread, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::ThreadRestart { project, thread, resume, profile } => {
                self.thread_restart(&project, &thread, resume, profile, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::ThreadBrief { project, thread } => {
                self.thread_brief(&project, &thread, actor).await.map(FrontResponse::Thread)
            }
            FrontRequest::ThreadRead { project, thread, lines } => self.thread_read(&project, &thread, lines).await,
            FrontRequest::ThreadKeys { project, thread, keys, text } => {
                self.thread_keys(&project, &thread, &keys, text, actor).await
            }
            FrontRequest::Questions { all } => Ok(FrontResponse::Questions { questions: self.questions(all) }),
            FrontRequest::QuestionAnswer { id, choice, menu, pick } => match (menu, pick) {
                (Some(menu), Some(pick)) => self.answer_menu(&id, &menu, pick, actor).await,
                _ => self.answer(&id, &choice, actor).await,
            }
            .map(FrontResponse::Question),
            FrontRequest::ThreadMessage { project, thread, text, mode } => {
                self.thread_message(&project, &thread, text, mode, actor).await.map(FrontResponse::Message)
            }
            FrontRequest::Preferences => Ok(FrontResponse::Preferences(self.preferences.get())),
            FrontRequest::PreferencesSet { settings, pins } => {
                let change = prefs::change_body(settings, pins)?;
                let doc = self.preferences.set(change, |text| self.db.set_meta(prefs::META_KEY, text))?;
                self.db.audit(actor, "preferences_set", &doc.revision.to_string());
                Ok(FrontResponse::Preferences(doc))
            }
            FrontRequest::Inbox { project, all, peek } => self.inbox(&project, all, peek).map(|mut p| {
                if role.can_control() {
                    self.library_notice_links(&mut p.items);
                }
                FrontResponse::Inbox {
                    items: p.items,
                    done: None,
                    omitted_open: p.omitted_open,
                    omitted_done: p.omitted_done,
                }
            }),
            FrontRequest::InboxDone { project, ids, all } => {
                self.inbox_done(&project, &ids, all, actor).map(|(done, p)| FrontResponse::Inbox {
                    items: p.items,
                    done: Some(done),
                    omitted_open: p.omitted_open,
                    omitted_done: p.omitted_done,
                })
            }
            FrontRequest::TickerTick => Ok(FrontResponse::Ticker { runs: self.ticker_tick().await }),
            FrontRequest::MissionList { .. } => Ok(FrontResponse::Missions { missions: vec![] }),
            FrontRequest::MissionShow { .. }
            | FrontRequest::MissionApprove { .. }
            | FrontRequest::MissionReject { .. }
            | FrontRequest::MissionStart { .. }
            | FrontRequest::MissionStop { .. }
            | FrontRequest::MissionPause { .. }
            | FrontRequest::MissionResume { .. }
            | FrontRequest::MissionReload { .. }
            | FrontRequest::MissionRestart { .. }
            | FrontRequest::MissionOpen { .. } => Err(legacy_mission::removed()),
            FrontRequest::Routines { project } => {
                self.routines(&project).map(|routines| FrontResponse::Routines { routines })
            }
            FrontRequest::AutomationApprove { project, name, approval_sha256 } => self
                .automation_approve(&project, &name, &approval_sha256, actor)
                .map(|routines| FrontResponse::Routines { routines }),
            FrontRequest::AutomationConvert { project, dry_run, plan_sha256 } => self
                .automation_convert(&project, dry_run, plan_sha256.as_deref(), actor)
                .await
                .map(|(files, plan_sha256)| FrontResponse::AutomationConverted { files, dry_run, plan_sha256 }),
            FrontRequest::AutomationUnblock { project, name, batch, discard } => self
                .automation_unblock(&project, &name, &batch, discard, actor)
                .and_then(|_| self.routines(&project))
                .map(|routines| FrontResponse::Routines { routines }),
            FrontRequest::RoutineRuns { project, name } => self.routine_runs(&project, &name).and_then(|runs| {
                Ok(FrontResponse::RoutineRuns { runs, source: self.routine_source(&project, &name)? })
            }),
            FrontRequest::RoutineToggle { project, name, on } => {
                self.routine_toggle(&project, &name, on, actor).map(|routines| FrontResponse::Routines { routines })
            }
            FrontRequest::RoutineApprove { project, name, command_sha256 } => self
                .routine_approve(&project, &name, &command_sha256, actor)
                .map(|routines| FrontResponse::Routines { routines }),
            FrontRequest::Events { project, after, limit } => {
                self.events(project.as_deref(), after, limit).map(|events| FrontResponse::Events { events })
            }
            FrontRequest::EvaluatorAssign { .. }
            | FrontRequest::EvaluatorStatus { .. }
            | FrontRequest::EvaluatorUnassign { .. }
            | FrontRequest::Evaluations { .. }
            | FrontRequest::EvaluationShow { .. }
            | FrontRequest::EvaluationDecide { .. } => Err(legacy_mission::removed()),
            FrontRequest::PaneRun { machine, name, argv, cwd, cols, rows, profile } => {
                let mid = self.resolve_machine(machine.as_deref())?;
                let resolved_profile =
                    profile.as_deref().map(|name| self.resolved_profile(None, &mid, name)).transpose()?;
                let spec = SpawnSpec {
                    name: name.unwrap_or_default(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: cols.unwrap_or(120),
                    rows: rows.unwrap_or(32),
                    role: None,
                    harness: None,
                    profile,
                    resolved_profile,
                };
                let id = self.spawn_on(&mid, spec, actor).await?;
                self.pane_view_of(&id, role)
                    .map(FrontResponse::Pane)
                    .ok_or_else(|| err(ErrorCode::Internal, "pane 목록에 없다"))
            }
            FrontRequest::PaneRead { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                let running = self.pane_view_of(&id, role).is_some_and(|p| p.state == SessionState::Running);
                if let (Some(conn), true) = (self.conn_of(&mid), running)
                    && let Ok(AgentResponse::Screen(s)) =
                        conn.request(AgentRequest::Screen { session: id.clone() }).await
                {
                    return Ok(FrontResponse::Screen(s));
                }
                // 끊겼거나 끝났다: 마지막 화면(mirror)
                self.mirror_text(&id)
                    .map(FrontResponse::Screen)
                    .ok_or_else(|| err(ErrorCode::AgentDisconnected, "화면 기록이 없다"))
            }
            FrontRequest::PaneFile { pane, path } => self.pane_file(role, actor, &pane, &path).await,
            FrontRequest::ThreadFile { project, thread, path } => {
                self.thread_file(actor, &project, &thread, &path).await
            }
            FrontRequest::ThreadDiff { project, thread, query } => self.thread_diff(&project, &thread, query).await,
            FrontRequest::PaneDir { pane, path, hidden } => self.pane_dir(role, actor, &pane, &path, hidden).await,
            FrontRequest::PaneChat { pane, thinking, compact, before } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                // 보기 범위(§8.25)는 pane 보기와 같다
                if self.pane_view_of(&id, role).is_none() {
                    return Err(err(ErrorCode::NotFound, format!("pane이 없다: {pane}")));
                }
                let (harness, transcript, harness_session) = {
                    let i = self.inner.lock().unwrap();
                    let p = i.panes.get(&id).ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))?;
                    let a = p.agent.as_ref();
                    (
                        a.map(|a| a.harness.clone()).unwrap_or_default(),
                        p.transcript.clone(),
                        a.and_then(|a| a.harness_session.clone()),
                    )
                };
                if !matches!(harness.as_str(), "claude" | "codex") {
                    return Err(err(ErrorCode::NotFound, "대화 기록은 Claude·Codex pane만 볼 수 있다"));
                }
                let pages = self
                    .inner
                    .lock()
                    .unwrap()
                    .machines
                    .get(&mid)
                    .is_some_and(|m| m.features.iter().any(|f| f == "chat_pages.v1"));
                if before.is_some() && !pages {
                    return Err(err(ErrorCode::UnsupportedVersion, "이 agent는 앞 기록 읽기를 지원하지 않는다"));
                }
                let conn = self.live_conn(&mid)?;
                match conn
                    .request(AgentRequest::Chat {
                        session: Some(id.clone()),
                        harness: harness.clone(),
                        transcript,
                        harness_session,
                        thinking,
                        compact,
                        before,
                    })
                    .await?
                {
                    AgentResponse::Chat { messages, truncated, before, history } => {
                        Ok(FrontResponse::Chat(aaa_proto::ChatView {
                            pane: id,
                            harness,
                            messages,
                            truncated,
                            before,
                            history,
                        }))
                    }
                    other => Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
                }
            }
            FrontRequest::PaneReadText { pane, source, lines, format } => {
                self.pane_read_text(&pane, source, lines, format).await
            }
            FrontRequest::PaneSendText { pane, text } => self.pane_send_text(&pane, text, actor).await,
            FrontRequest::PaneSendKeys { pane, keys } => self.pane_send_keys(&pane, &keys, actor).await,
            FrontRequest::PaneSubmit { pane, text } => self.pane_submit(&pane, &text, actor).await,
            FrontRequest::PaneWaitOutput { pane, matcher, source, lines, timeout_ms } => {
                self.pane_wait_output(&pane, matcher, source, lines, timeout_ms).await
            }
            FrontRequest::PaneGet { pane } => {
                let (id, _) = self.resolve_pane(&pane)?;
                self.pane_view_of(&id, role)
                    .map(FrontResponse::Pane)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("pane이 없다: {pane}")))
            }
            FrontRequest::PaneClose { pane } => self.pane_close(&pane, actor).await,
            FrontRequest::Workspaces => Ok(FrontResponse::Workspaces { workspaces: self.workspace_summaries() }),
            FrontRequest::WorkspaceClose { name } => self.workspace_close(&name, actor).await,
            FrontRequest::WorkspaceRename { name, to } => self.workspace_rename(&name, &to, actor),
            FrontRequest::PaneSend { pane, data } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.db.audit(actor, "pane_send", &format!("{id} {} bytes", data.0.len()));
                conn.request(AgentRequest::Input { session: id, data }).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneResize { pane, cols, rows } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                // 대화형 front가 크기를 걸면 그 화면이 지금 쓰는 화면이 된다. 한 번 보내는 CLI는 아무도 쓰지 않을 때만 바꾼다.
                let interactive = match front {
                    Some(f) if f.interactive() => {
                        self.lease_use(&id, f);
                        true
                    }
                    _ => {
                        self.lease_free(&id)?;
                        false
                    }
                };
                let conn = self.live_conn(&mid)?;
                if interactive {
                    return conn
                        .request(AgentRequest::Resize { session: id, cols, rows })
                        .await
                        .map(|_| FrontResponse::Done);
                }
                // 한 번 보내는 CLI(`pane pty-size`): 새 크기가 host에 닿은 뒤 돌아온다. 바로 뒤 읽기가 새 크기의 화면을 본다.
                let rx = self.pane_events(&id)?;
                conn.request(AgentRequest::Resize { session: id.clone(), cols, rows }).await?;
                self.wait_pane(
                    &id,
                    rx,
                    |p| (p.info.cols, p.info.rows) == (cols, rows),
                    "크기를 보냈지만 새 크기가 오지 않았다",
                )
                .await
                .map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneInput { pane, data } => {
                let f = front.ok_or_else(|| {
                    err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다(한 번 보내기는 pane_send)")
                })?;
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.lease_use(&id, f);
                // 입력 내용은 기록하지 않고, 복사 사고를 식별할 종류와 front 출처만 request=cN 로그에 묶는다.
                let input_kind = if data.0 == [3] {
                    "ctrl-c"
                } else if data.0.starts_with(b"\x1b[<") {
                    "mouse-sgr"
                } else {
                    "other"
                };
                tracing::Instrument::instrument(
                    conn.request(AgentRequest::Input { session: id, data }),
                    tracing::info_span!("pane_front", front_id = %f.id, front = %f.label, input_kind),
                )
                .await
                .map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneCtrlEnter { pane, trace } => {
                let f = front.ok_or_else(|| err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다"))?;
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.lease_use(&id, f);
                let data = self.ctrl_enter_data(&id, trace.as_deref());
                // steering할 수 없는 상태(입력 모름·쉬는 Codex 등)는 아무것도 보내지 않는다(plan §8.239 후속)
                if data.is_empty() {
                    return Ok(FrontResponse::Done);
                }
                conn.request(AgentRequest::Input { session: id, data: aaa_proto::B64(data) })
                    .await
                    .map(|_| FrontResponse::Done)
            }
            // herdr 서버가 client의 붙여넣기를 그 pane 모드대로 다시 감싸는 것과 같다(`KeyModes::paste`: 켰으면 감싸고 ESC는 뺀다)
            FrontRequest::PanePaste { pane, text } => {
                let f = front.ok_or_else(|| {
                    err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다(한 번 보내기는 pane_send)")
                })?;
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.lease_use(&id, f);
                let data = aaa_proto::B64(self.key_modes(&id).paste(&text));
                conn.request(AgentRequest::Input { session: id, data }).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneTake { pane } => {
                let f = front.ok_or_else(|| err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다"))?;
                let (id, _) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                self.lease_set(&id, Some(f.clone()));
                self.db.audit(actor, "pane_take", &id);
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneRelease { pane } => {
                let f = front.ok_or_else(|| err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다"))?;
                let (id, _) = self.resolve_pane(&pane)?;
                let mine = self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.lease.as_ref() == Some(f));
                if mine {
                    self.lease_set(&id, None);
                }
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneResume { pane } => self.resume(&pane, role, actor).await,
            FrontRequest::PaneForget { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.forget(&id, &mid, actor).await?;
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneRename { pane, label } => self.rename(&pane, label, role, actor).await,
            FrontRequest::PaneAttach { .. } => {
                Err(err(ErrorCode::BadRequest, "그림 붙여넣기는 같은 머신 front 연결(aaa pane attach)에서만 쓴다"))
            }
            FrontRequest::Workspace { name } => {
                Ok(FrontResponse::Workspace(self.workspace(name.as_deref().unwrap_or(DEFAULT_WORKSPACE))))
            }
            FrontRequest::LayoutApply { workspace, change } => {
                // workspace를 주지 않으면 그 pane·탭이 든 workspace(없으면 main)
                let found = match &change {
                    LayoutChange::Split { pane, .. }
                    | LayoutChange::Close { pane, .. }
                    | LayoutChange::Resize { pane, .. }
                    | LayoutChange::MoveToNewTab { pane, .. }
                    | LayoutChange::Zoom { pane, .. }
                    | LayoutChange::Swap { pane, .. } => self.workspace_of_pane(pane),
                    LayoutChange::RenameTab { tab, .. }
                    | LayoutChange::CloseTab { tab }
                    | LayoutChange::SplitRatio { tab, .. }
                    | LayoutChange::MoveTab { tab, .. } => self.workspace_of_tab(tab),
                    LayoutChange::NewTab { .. } | LayoutChange::AddTab { .. } => None,
                };
                let name = workspace.or(found).unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());
                self.layout_apply(&name, change, actor).await.map(FrontResponse::Workspace)
            }
            FrontRequest::PaneKill { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                let conn = self.live_conn(&mid)?;
                self.db.audit(actor, "pane_kill", &id);
                self.kill_session(&conn, &id).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::EnrollOpen { minutes } => {
                let minutes = minutes.clamp(1, 60);
                self.enroll.lock().unwrap().open_until =
                    Some(Instant::now() + Duration::from_secs(minutes as u64 * 60));
                self.db.audit(actor, "enroll_open", &format!("{minutes}m"));
                Ok(FrontResponse::Done)
            }
            FrontRequest::EnrollPending => {
                let e = self.enroll.lock().unwrap();
                let pending = e
                    .pending
                    .iter()
                    .map(|(name, p)| PendingEnroll {
                        name: name.clone(),
                        machine_id: p.machine_id.clone(),
                        fingerprint_prefix: p.fingerprint[..16].to_string(),
                        since_ms: p.since_ms,
                    })
                    .collect();
                Ok(FrontResponse::Pending { pending })
            }
            FrontRequest::EnrollApprove { name, code } => self.approve(&name, &code, actor),
            FrontRequest::MachineRevoke { name } => {
                let id = self.db.revoke_machine(&name).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
                let Some(id) = id else { return Err(err(ErrorCode::NotFound, format!("머신이 없다: {name}"))) };
                let conn = {
                    let mut i = self.inner.lock().unwrap();
                    i.panes.retain(|_, p| p.machine_id != id);
                    i.machines.remove(&id).and_then(|m| m.conn)
                };
                if let Some(c) = conn {
                    let _ = c.tx.json(&LinkMsg::Reject(err(ErrorCode::NotEnrolled, "등록이 폐기됐다"))).await;
                    c.fail_all();
                }
                self.db.audit(actor, "machine_revoke", &name);
                self.bump();
                Ok(FrontResponse::Done)
            }
            FrontRequest::WebPair { role: r } => {
                if r == DeviceRole::Local {
                    return Err(err(ErrorCode::BadRequest, "reader 또는 controller만 줄 수 있다"));
                }
                // 웹에서 만드는 코드는 controller만(D6, herdr-web-ui는 웹 pairing이 늘 drive). 보기 기기는 이 머신의 CLI로
                if role != DeviceRole::Local && r != DeviceRole::Controller {
                    return Err(err(
                        ErrorCode::Forbidden,
                        "보기 권한 코드는 host 머신의 CLI(aaa web pair --reader)에서만 만든다",
                    ));
                }
                let (code, expires_in_s) = self.mint_pairing(r, Instant::now());
                self.db.audit(actor, "web_pair_code", &format!("{r:?}"));
                Ok(FrontResponse::PairCode { code, role: r, expires_in_s })
            }
            FrontRequest::Devices => {
                let rows = self.db.devices().map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
                Ok(FrontResponse::Devices { devices: rows.into_iter().map(device_view).collect() })
            }
            FrontRequest::DeviceRename { id, label } => {
                let label = normalize_device_label(&label, db::DEVICE_LABEL_FALLBACK);
                if !self.db.rename_device(&id, &label).map_err(|e| err(ErrorCode::Internal, e.to_string()))? {
                    return Err(err(ErrorCode::NotFound, "그런 기기가 없다"));
                }
                self.db.audit(actor, "device_rename", &id);
                Ok(FrontResponse::Done)
            }
            FrontRequest::DeviceRevoke { id } => {
                if !self.revoke_device(&id)? {
                    return Err(err(ErrorCode::NotFound, "그런 기기가 없다"));
                }
                self.db.audit(actor, "device_revoke", &id);
                Ok(FrontResponse::Done)
            }
            FrontRequest::WebStatus => Ok(FrontResponse::Publish(self.publish_status().await)),
            FrontRequest::WebPublish => {
                self.db.audit(actor, "web_publish", "");
                self.publish().await.map(FrontResponse::Publish)
            }
            FrontRequest::WebPublishOff => {
                self.db.audit(actor, "web_publish_off", "");
                self.publish_off().await.map(FrontResponse::Publish)
            }
        }
    }

    /// agent에 세션을 띄우고 pane 목록에 넣는다. 새 pane ID를 돌려준다.
    async fn spawn_on(self: &Arc<Self>, mid: &str, spec: SpawnSpec, actor: &str) -> Result<String, ErrorBody> {
        let conn = self.live_conn(mid)?;
        self.check_health(mid)?;
        self.db.audit(actor, "pane_run", &db::pane_run_detail(&spec));
        match conn.request(AgentRequest::Spawn(spec)).await? {
            AgentResponse::Session(info) => {
                let id = info.id.clone();
                self.on_session(mid, info, false);
                Ok(id)
            }
            other => Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
        }
    }

    // ───────────── 조작 lease(§4.5) ─────────────

    pub fn new_front(&self, label: String) -> FrontRef {
        FrontRef { id: format!("f{}", self.front_seq.fetch_add(1, Ordering::Relaxed) + 1), label }
    }

    fn lease_set(&self, id: &str, to: Option<FrontRef>) {
        {
            let mut i = self.inner.lock().unwrap();
            let Some(p) = i.panes.get_mut(id) else { return };
            if p.lease == to {
                return;
            }
            p.lease = to;
            let _ = p.events.send(PaneEvent::State);
        }
        self.bump();
        // 보는 웹 화면이 그 pane을 쓰기 시작하면 봤다(seen.rs).
        if self.inner.lock().unwrap().panes.contains_key(id) && self.someone_sees(id) {
            self.mark_seen(&[id.to_string()]);
        }
    }

    /// 이 front가 지금 쓰는 화면이 된다(herdr-web-ui처럼 명시적 가져오기 없이, 마지막 입력이 이긴다).
    /// 바뀌면 pane 상태 사건이 나가고, 각 front는 자기 규칙으로 크기를 다시 건다.
    pub(crate) fn lease_use(&self, id: &str, f: &FrontRef) {
        self.lease_set(id, Some(f.clone()));
        // 그 화면이 썼다: 그 화면이 알린 색이 지금 색이다(herdr `promote_client_to_foreground`, plan §8.187)
        let i = &mut *self.inner.lock().unwrap();
        if i.colors.touch(&f.id, None) {
            colors::push_all(i);
        }
    }

    fn lease_free(&self, id: &str) -> Result<(), ErrorBody> {
        match self.inner.lock().unwrap().panes.get(id).and_then(|p| p.lease.clone()) {
            Some(h) => Err(err(ErrorCode::LeaseHeld, format!("{}이 쓰는 중이라 크기를 바꾸지 않았다", h.label))),
            None => Ok(()),
        }
    }

    /// front 연결이 끝났다: 그 연결의 lease를 모두 푼다.
    pub fn front_gone(&self, front_id: &str) {
        let freed: Vec<String> = {
            let mut i = self.inner.lock().unwrap();
            if i.colors.gone(front_id) {
                colors::push_all(&i);
            }
            i.panes
                .iter()
                .filter(|(_, p)| p.lease.as_ref().is_some_and(|l| l.id == front_id))
                .map(|(k, _)| k.clone())
                .collect()
        };
        for id in freed {
            self.lease_set(&id, None);
        }
    }

    // ───────────── 재개·정리 ─────────────

    /// 끝난 pane을 같은 spec(이름·argv·cwd·크기)으로 다시 띄우고 layout 자리를 새 pane으로 바꾼 뒤 옛 기록을 지운다.
    /// pane에 사람이 붙인 이름(herdr `pane rename`). 이름의 정본은 agent의 세션 기록이라 agent까지 간다(plan §8.109).
    async fn rename(
        self: &Arc<Self>,
        pane: &str,
        label: Option<String>,
        role: DeviceRole,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        if label.as_ref().is_some_and(|l| l.chars().count() > aaa_proto::MAX_PANE_LABEL_CHARS) {
            return Err(err(
                ErrorCode::BadRequest,
                format!("이름은 {}자 안이어야 한다", aaa_proto::MAX_PANE_LABEL_CHARS),
            ));
        }
        let conn = self.live_conn(&mid)?;
        // 옛 agent는 모르는 요청에 연결을 끊는다: 보내기 전에 거른다
        let knows =
            self.inner.lock().unwrap().machines.get(&mid).is_some_and(|m| m.features.iter().any(|f| f == "rename.v1"));
        if !knows {
            return Err(err(
                ErrorCode::BadRequest,
                "이 머신의 agent가 pane 이름 바꾸기를 모른다(agent binary 반영 필요)",
            ));
        }
        self.db.audit(actor, "pane_rename", &format!("{id} {}", label.as_deref().unwrap_or("-")));
        match conn.request(AgentRequest::Rename { session: id.clone(), label }).await? {
            AgentResponse::Session(info) => self.on_session(&mid, info, false),
            other => return Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
        }
        self.pane_view_of(&id, role).map(FrontResponse::Pane).ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))
    }

    async fn resume(self: &Arc<Self>, pane: &str, role: DeviceRole, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        let info = self
            .inner
            .lock()
            .unwrap()
            .panes
            .get(&id)
            .map(|p| p.info.clone())
            .ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))?;
        if info.state.is_running() {
            return Err(err(ErrorCode::BadRequest, "실행 중인 pane은 재개할 수 없다"));
        }
        let spec = SpawnSpec {
            name: respawn_name(&info),
            argv: info.argv.clone(),
            cwd: info.cwd.clone(),
            env: vec![],
            cols: info.cols,
            rows: info.rows,
            role: info.role,
            harness: info.harness.clone(),
            profile: None,
            resolved_profile: None,
        };
        let new_id = self.respawn(&id, &mid, spec, actor).await?;
        self.pane_view_of(&new_id, role)
            .map(FrontResponse::Pane)
            .ok_or_else(|| err(ErrorCode::Internal, "pane 목록에 없다"))
    }

    /// 끝난 pane 자리에 새 세션을 띄운다(`pane resume`·재부팅 복원): layout 자리를 바꾸고 옛 기록을 지운다.
    /// spec 이름은 [`respawn_name`]으로 짓는다
    pub(crate) async fn respawn(
        self: &Arc<Self>,
        id: &str,
        mid: &str,
        spec: SpawnSpec,
        actor: &str,
    ) -> Result<String, ErrorBody> {
        let new_id = self.spawn_on(mid, spec, actor).await?;
        self.db.audit(actor, "pane_resume", &format!("{id} -> {new_id}"));
        self.replace_pane(id, &new_id, mid, actor).await;
        Ok(new_id)
    }

    /// layout에서 `old` 자리를 `new`로 바꾸고 `old` 기록을 지운다. 사람이 붙인 이름은 `new`가 잇는다(herdr 복원
    /// `set_manual_label`). 이름의 정본은 agent 세션 기록이라 agent에 붙인다(plan §8.109)
    pub(crate) async fn replace_pane(self: &Arc<Self>, old: &str, new: &str, mid: &str, actor: &str) {
        let label = self.inner.lock().unwrap().panes.get(old).and_then(|p| p.info.label.clone());
        if let Some(label) = label.filter(|_| self.machine_feature(mid, "rename.v1")) {
            let renamed = match self.live_conn(mid) {
                Ok(conn) => conn.request(AgentRequest::Rename { session: new.to_string(), label: Some(label) }).await,
                Err(e) => Err(e),
            };
            match renamed {
                Ok(AgentResponse::Session(info)) => self.on_session(mid, info, false),
                Ok(other) => tracing::warn!(pane = %new, "이름 잇기: 예상하지 못한 응답 {other:?}"),
                Err(e) => tracing::warn!(pane = %new, "이름 잇기 실패: {}", e.message),
            }
        }
        // 옛 ID를 보던 화면이 새 pane을 따라가게 남긴다(QA Q19): 옛 pane이 대신했던 것까지 잇는다
        {
            let mut i = self.inner.lock().unwrap();
            let mut chain = i.panes.get(old).map(|p| p.replaced.clone()).unwrap_or_default();
            chain.push(old.to_string());
            let skip = chain.len().saturating_sub(REPLACED_KEEP);
            if let Some(p) = i.panes.get_mut(new) {
                p.replaced = chain.split_off(skip);
            }
        }
        // 데려온 thread는 그 pane이 정본이다(다시 띄울 launch가 없다): 새 pane이 대신하면 thread도 따라간다
        if let Some(r) = self.thread_of_pane(old).filter(|r| r.kind == aaa_proto::ThreadKind::Adopted) {
            let _ = self.update_rec(&r.project, &r.id, |x| x.pane = Some(new.to_string()));
        }
        {
            let mut ws = self.workspaces.lock().unwrap();
            let changed: Vec<Workspace> =
                ws.values_mut().filter_map(|w| aaa_proto::layout::ws_replace(w, old, new).then(|| w.clone())).collect();
            drop(ws);
            for w in changed {
                self.save_workspace(&w);
            }
        }
        if let Err(e) = self.forget(old, mid, actor).await {
            tracing::warn!(pane = %old, "재개 뒤 옛 기록 정리 실패: {}", e.message);
        }
    }

    /// 세션에 끝내기를 보낸다. host가 끝낸 pane은 스스로 끝난 셸이 아니다(`exited_by_itself`).
    async fn kill_session(&self, conn: &AgentConn, id: &str) -> Result<AgentResponse, ErrorBody> {
        if let Some(p) = self.inner.lock().unwrap().panes.get_mut(id) {
            p.kill_sent = true;
        }
        conn.request(AgentRequest::Kill { session: id.to_string() }).await
    }

    /// 스스로 끝난 셸 pane을 닫는다(herdr 0.9.5 `handle_pane_died`·`Workspace::remove_pane`): 기록을 지우고 모든 layout에서
    /// 뺀다(`forget`, 빈 탭은 없어진다). 그래서 탭이 하나도 남지 않은 workspace도 지운다(herdr는 마지막 pane이 끝난
    /// workspace를 지운다).
    fn spawn_close_exited(self: &Arc<Self>, id: String, mid: String) {
        let me = self.clone();
        tokio::spawn(async move { me.close_record(&id, &mid, "exit").await });
    }

    /// 끝난 pane을 닫는다(셸 exit, 기록 편집기 overlay): 기록을 지우고 모든 layout에서 빼고, 탭이 하나도 남지 않은 workspace도 지운다
    async fn close_record(self: &Arc<Self>, id: &str, mid: &str, actor: &str) {
        let had: Vec<String> = {
            let ws = self.workspaces.lock().unwrap();
            ws.values().filter(|w| aaa_proto::layout::ws_contains(w, id)).map(|w| w.name.clone()).collect()
        };
        if let Err(e) = self.forget(id, mid, actor).await {
            tracing::warn!(pane = %id, actor, "끝난 pane을 닫지 못했다: {}", e.message);
            return;
        }
        for name in had {
            let emptied = {
                let mut ws = self.workspaces.lock().unwrap();
                let empty = ws.get(&name).is_some_and(|w| w.tabs.is_empty());
                empty && ws.remove(&name).is_some()
            };
            if !emptied {
                continue;
            }
            if let Err(e) = self.db.delete_workspace(&name) {
                tracing::warn!(workspace = %name, "workspace 지우기 실패: {e}");
            }
            self.db.audit(actor, "workspace_close", &name);
            let _ = self.layout_events.send(Workspace { name, tabs: vec![] });
        }
    }

    async fn forget(self: &Arc<Self>, id: &str, mid: &str, actor: &str) -> Result<(), ErrorBody> {
        if self.inner.lock().unwrap().panes.get(id).is_some_and(|p| p.info.state.is_running()) {
            return Err(err(ErrorCode::BadRequest, "실행 중인 pane은 지울 수 없다"));
        }
        let conn = self.live_conn(mid)?;
        conn.request(AgentRequest::Forget { session: id.to_string() }).await?;
        self.inner.lock().unwrap().panes.remove(id);
        let changed: Vec<Workspace> = {
            let mut ws = self.workspaces.lock().unwrap();
            ws.values_mut().filter_map(|w| aaa_proto::layout::ws_remove(w, id).then(|| w.clone())).collect()
        };
        for w in changed {
            self.save_workspace(&w);
        }
        self.db.audit(actor, "pane_forget", id);
        self.bump();
        Ok(())
    }

    // ───────────── workspace layout(host 정본) ─────────────

    pub fn workspace(&self, name: &str) -> Workspace {
        self.workspaces
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .unwrap_or_else(|| Workspace { name: name.to_string(), tabs: vec![] })
    }

    fn save_workspace(&self, w: &Workspace) {
        if let Err(e) = self.db.save_workspace(&w.name, &serde_json::to_string(w).unwrap_or_default()) {
            tracing::warn!(workspace = %w.name, "workspace 저장 실패: {e}");
        }
        self.workspaces.lock().unwrap().insert(w.name.clone(), w.clone());
        let _ = self.layout_events.send(w.clone());
    }

    async fn layout_apply(
        self: &Arc<Self>,
        name: &str,
        change: LayoutChange,
        actor: &str,
    ) -> Result<Workspace, ErrorBody> {
        let pane_machine = |id: &str| self.inner.lock().unwrap().panes.get(id).map(|p| p.machine_id.clone());
        let mut w = self.workspace(name);
        match change {
            LayoutChange::NewTab { machine, name: pname, argv, cwd, cols, rows, label, from } => {
                let mid = self.resolve_machine(machine.as_deref())?;
                let cwd = self.new_pane_cwd(cwd, from.as_deref(), &mid).await;
                let spec = SpawnSpec {
                    name: pname.unwrap_or_default(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: cols.unwrap_or(120),
                    rows: rows.unwrap_or(32),
                    role: None,
                    harness: None,
                    profile: None,
                    resolved_profile: None,
                };
                let id = self.spawn_on(&mid, spec, actor).await?;
                // spawn 사이에 다른 변경이 있었을 수 있다: 최신을 다시 읽어 붙인다
                w = self.workspace(name);
                let label = label.map(|l| l.trim().to_string()).unwrap_or_default();
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: label,
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Split { pane, dir, machine, argv, cwd, ratio } => {
                let (target, target_mid) = self.resolve_pane(&pane)?;
                if !aaa_proto::layout::ws_contains(&w, &target) {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
                let mid = match machine {
                    Some(m) => self.resolve_machine(Some(&m))?,
                    None => pane_machine(&target).unwrap_or(target_mid),
                };
                let cwd = self.new_pane_cwd(cwd, Some(&target), &mid).await;
                let spec = SpawnSpec {
                    name: String::new(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: 80,
                    rows: 24,
                    role: None,
                    harness: None,
                    profile: None,
                    resolved_profile: None,
                };
                let id = self.spawn_on(&mid, spec, actor).await?;
                w = self.workspace(name);
                let done = w.tabs.iter_mut().any(|t| {
                    // herdr: 나누면 그 탭의 zoom을 푼다
                    let hit = aaa_proto::layout::split(&mut t.root, &target, &id, dir);
                    t.zoomed &= !hit;
                    hit
                });
                if let Some(r) = ratio {
                    // herdr `pane split --ratio`: 새로 생긴 나눔의 a 쪽 비율
                    for t in &mut w.tabs {
                        aaa_proto::layout::set_split_ratio(&mut t.root, &id, r.clamp(100, 900));
                    }
                }
                if !done {
                    return Err(err(ErrorCode::NotFound, "나누는 사이에 pane이 layout에서 빠졌다"));
                }
            }
            LayoutChange::AddTab { pane } => {
                let (id, _) = self.resolve_pane(&pane)?;
                if aaa_proto::layout::ws_contains(&w, &id) {
                    return Err(err(ErrorCode::BadRequest, "이미 이 workspace에 있다"));
                }
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: String::new(),
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Close { pane, kill } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                if kill && self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.info.state.is_running()) {
                    let conn = self.live_conn(&mid)?;
                    self.db.audit(actor, "pane_kill", &id);
                    self.kill_session(&conn, &id).await?;
                }
                if !aaa_proto::layout::ws_remove(&mut w, &id) {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
            }
            LayoutChange::Resize { pane, direction, amount } => {
                let (id, _) = self.resolve_pane(&pane)?;
                // herdr `handle_pane_resize`: 기본 0.05, 최대 0.5
                let amount = amount.unwrap_or(50).min(500);
                let mut found = false;
                for t in &mut w.tabs {
                    if aaa_proto::layout::leaves(&t.root).contains(&id) {
                        found = true;
                        aaa_proto::layout::resize(&mut t.root, &id, direction, amount);
                    }
                }
                if !found {
                    return Err(err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")));
                }
            }
            LayoutChange::RenameTab { tab, name: label } => {
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                // 앞뒤 빈칸은 뺀다. 빈 이름은 이름 없음(탭 번호로 보인다)
                t.name = label.trim().to_string();
            }
            LayoutChange::CloseTab { tab } => {
                let at = w
                    .tabs
                    .iter()
                    .position(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                let t = w.tabs.remove(at);
                // 탭을 뺀 layout을 먼저 저장한다(front에서 바로 사라진다). pane을 끝내는 동안 기다리므로, 그 사이 다른
                // 변경(새 탭·이름 바꾸기·나누기)이 있을 수 있다: 끝낸 뒤 최신을 다시 읽어 그것을 저장한다(보안 감사 t-0340:
                // 맨 위에서 복사한 것을 저장하면 그 사이 변경이 사라졌다)
                self.save_workspace(&w);
                self.close_panes(&aaa_proto::layout::leaves(&t.root), name, actor).await;
                w = self.workspace(name);
            }
            LayoutChange::MoveToNewTab { pane, name: label } => {
                let (id, _) = self.resolve_pane(&pane)?;
                // 옮긴다: 다른 workspace에 있던 자리도 뺀다(herdr `pane move`는 pane이 한 곳에만 있다)
                let others: Vec<Workspace> = {
                    let mut ws = self.workspaces.lock().unwrap();
                    ws.values_mut()
                        .filter(|o| o.name != name)
                        .filter_map(|o| aaa_proto::layout::ws_remove(o, &id).then(|| o.clone()))
                        .collect()
                };
                for o in others {
                    self.save_workspace(&o);
                }
                aaa_proto::layout::ws_remove(&mut w, &id);
                w.tabs.push(Tab {
                    id: util::new_id(),
                    name: label.unwrap_or_default(),
                    root: LayoutNode::Pane { pane: id },
                    zoomed: false,
                });
            }
            LayoutChange::Zoom { pane, zoomed } => {
                let (id, _) = self.resolve_pane(&pane)?;
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| aaa_proto::layout::leaves(&t.root).contains(&id))
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 pane {pane}이 없다")))?;
                // herdr `apply_pane_zoom`: pane이 하나뿐인 탭은 그대로
                if aaa_proto::layout::leaves(&t.root).len() > 1 {
                    t.zoomed = zoomed.unwrap_or(!t.zoomed);
                }
            }
            LayoutChange::Swap { pane, with } => {
                let (a, _) = self.resolve_pane(&pane)?;
                let (b, _) = self.resolve_pane(&with)?;
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| {
                        let l = aaa_proto::layout::leaves(&t.root);
                        l.contains(&a) && l.contains(&b)
                    })
                    .ok_or_else(|| {
                        err(ErrorCode::BadRequest, format!("workspace {name}의 한 탭에 {pane}·{with}가 함께 없다"))
                    })?;
                aaa_proto::layout::swap(&mut t.root, &a, &b);
            }
            LayoutChange::SplitRatio { tab, path, ratio } => {
                let t = w
                    .tabs
                    .iter_mut()
                    .find(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                // herdr `handle_layout_set_split_ratio`: 그 자리에 나눔이 없으면 split_not_found
                if !aaa_proto::layout::set_ratio_at(&mut t.root, &path, ratio) {
                    return Err(err(ErrorCode::NotFound, format!("탭 {tab}에 그 나눔({path:?})이 없다")));
                }
            }
            LayoutChange::MoveTab { tab, index } => {
                let from = w
                    .tabs
                    .iter()
                    .position(|t| t.id == tab)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace {name}에 탭 {tab}이 없다")))?;
                // herdr `handle_tab_move`: insert_index는 0..=탭 수
                if usize::from(index) > w.tabs.len() {
                    return Err(err(ErrorCode::BadRequest, format!("탭 자리 {index}가 0..={} 밖이다", w.tabs.len())));
                }
                aaa_proto::layout::move_tab(&mut w.tabs, from, usize::from(index));
            }
        }
        self.db.audit(actor, "layout", name);
        self.save_workspace(&w);
        Ok(w)
    }

    /// 탭·workspace를 닫을 때 그 안 pane을 끝내고 기록에서 지운다(herdr `tab close`·`workspace close`: pane이 사라진다).
    /// 역할이 있는 pane(thread·coordinator·평가자)은 끝내지 않는다: 그 수명은 thread 명령(resolve·stop)이 정한다.
    /// `closing` 밖 다른 workspace에도 있는 pane은 자리만 빠진다. agent가 끊긴 pane은 끝낼 수 없어 기록을 둔다.
    async fn close_panes(self: &Arc<Self>, panes: &[String], closing: &str, actor: &str) {
        for id in panes {
            let target = {
                let i = self.inner.lock().unwrap();
                i.panes.get(id).map(|p| (p.machine_id.clone(), p.info.state.is_running(), p.info.role.is_some()))
            };
            let Some((mid, running, has_role)) = target else { continue };
            let elsewhere = self
                .workspaces
                .lock()
                .unwrap()
                .values()
                .any(|w| w.name != closing && aaa_proto::layout::ws_contains(w, id));
            if has_role || elsewhere {
                continue;
            }
            let Ok(conn) = self.live_conn(&mid) else { continue };
            if running {
                let Ok(rx) = self.pane_events(id) else { continue };
                self.db.audit(actor, "pane_kill", id);
                if let Err(e) = self.kill_session(&conn, id).await {
                    tracing::warn!(pane = %id, "닫으며 끝내지 못했다: {}", e.message);
                    continue;
                }
                if let Err(e) = self
                    .wait_pane(id, rx, |p| !p.info.state.is_running(), "끝내기를 보냈지만 세션이 끝나지 않았다")
                    .await
                {
                    tracing::warn!(pane = %id, "{}", e.message);
                    continue;
                }
            }
            if let Err(e) = self.forget(id, &mid, actor).await {
                tracing::warn!(pane = %id, "닫으며 기록을 지우지 못했다: {}", e.message);
            }
        }
    }

    /// pane이 들어 있는 workspace(main을 먼저). layout 요청에 workspace를 주지 않으면 이것을 쓴다(herdr는 pane ID가 workspace를 정한다).
    pub fn workspace_of_pane(&self, pane: &str) -> Option<String> {
        let (id, _) = self.resolve_pane(pane).ok()?;
        let ws = self.workspaces.lock().unwrap();
        if ws.get(DEFAULT_WORKSPACE).is_some_and(|w| aaa_proto::layout::ws_contains(w, &id)) {
            return Some(DEFAULT_WORKSPACE.to_string());
        }
        let mut names: Vec<_> =
            ws.values().filter(|w| aaa_proto::layout::ws_contains(w, &id)).map(|w| w.name.clone()).collect();
        names.sort();
        names.into_iter().next()
    }

    /// 탭이 들어 있는 workspace
    pub fn workspace_of_tab(&self, tab: &str) -> Option<String> {
        let ws = self.workspaces.lock().unwrap();
        ws.values().find(|w| w.tabs.iter().any(|t| t.id == tab)).map(|w| w.name.clone())
    }

    fn workspace_summaries(&self) -> Vec<aaa_proto::WorkspaceSummary> {
        let ws = self.workspaces.lock().unwrap();
        let mut out: Vec<_> = ws
            .values()
            .map(|w| aaa_proto::WorkspaceSummary {
                name: w.name.clone(),
                tabs: w.tabs.len() as u32,
                panes: w.tabs.iter().map(|t| aaa_proto::layout::leaves(&t.root).len() as u32).sum(),
            })
            .collect();
        if !ws.contains_key(DEFAULT_WORKSPACE) {
            out.push(aaa_proto::WorkspaceSummary { name: DEFAULT_WORKSPACE.to_string(), tabs: 0, panes: 0 });
        }
        out.sort_by(|a, b| (a.name != DEFAULT_WORKSPACE).cmp(&(b.name != DEFAULT_WORKSPACE)).then(a.name.cmp(&b.name)));
        out
    }

    /// workspace를 닫는다: 그 안 pane을 끝내고(역할 있는 pane은 두고) layout을 지운다. front에는 빈 workspace로 알린다.
    async fn workspace_close(self: &Arc<Self>, name: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let w = self
            .workspaces
            .lock()
            .unwrap()
            .remove(name)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("workspace가 없다: {name}")))?;
        if let Err(e) = self.db.delete_workspace(name) {
            tracing::warn!(workspace = %name, "workspace 지우기 실패: {e}");
        }
        let panes: Vec<String> = w.tabs.iter().flat_map(|t| aaa_proto::layout::leaves(&t.root)).collect();
        self.close_panes(&panes, name, actor).await;
        self.db.audit(actor, "workspace_close", name);
        let _ = self.layout_events.send(Workspace { name: name.to_string(), tabs: vec![] });
        Ok(FrontResponse::Workspaces { workspaces: self.workspace_summaries() })
    }

    fn workspace_rename(&self, name: &str, to: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let to = to.trim();
        if to.is_empty() {
            return Err(err(ErrorCode::BadRequest, "새 이름이 비었다"));
        }
        let mut w = {
            let mut ws = self.workspaces.lock().unwrap();
            if ws.contains_key(to) {
                return Err(err(ErrorCode::NameTaken, format!("이미 있는 workspace: {to}")));
            }
            ws.remove(name).ok_or_else(|| err(ErrorCode::NotFound, format!("workspace가 없다: {name}")))?
        };
        if let Err(e) = self.db.delete_workspace(name) {
            tracing::warn!(workspace = %name, "workspace 지우기 실패: {e}");
        }
        let _ = self.layout_events.send(Workspace { name: name.to_string(), tabs: vec![] });
        w.name = to.to_string();
        self.save_workspace(&w);
        self.db.audit(actor, "workspace_rename", &format!("{name} -> {to}"));
        Ok(FrontResponse::Workspace(w))
    }

    /// 새 pane의 cwd(herdr `resolve_new_terminal_cwd`): 준 값이 먼저, 없으면 `[terminal] new_cwd`.
    /// follow면 원래 pane(`from`)이 같은 머신에 있을 때 그 pane이 지금 있는 폴더(agent `cwd.v1`),
    /// 읽지 못하면 그 pane을 띄운 폴더(herdr `launch_cwd_for_terminal`). 다른 머신이면 그 머신 기본 위치.
    async fn new_pane_cwd(&self, given: Option<String>, from: Option<&str>, mid: &str) -> Option<String> {
        if given.is_some() {
            return given;
        }
        match self.cfg.terminal.new_cwd.as_str() {
            "follow" => {}
            "home" => return None,
            path => return Some(path.to_string()),
        }
        let (src, _) = self.resolve_pane(from?).ok()?;
        let (conn, launch) = {
            let i = self.inner.lock().unwrap();
            let p = i.panes.get(&src)?;
            if p.machine_id != mid {
                return None;
            }
            let m = i.machines.get(mid);
            let can_ask = p.info.state.is_running() && m.is_some_and(|m| m.features.iter().any(|f| f == "cwd.v1"));
            (m.and_then(|m| m.conn.clone()).filter(|_| can_ask), p.info.cwd.clone())
        };
        if let Some(conn) = conn
            && let Ok(AgentResponse::Cwd { cwd: Some(cwd) }) = conn.request(AgentRequest::Cwd { session: src }).await
        {
            return Some(cwd);
        }
        launch
    }

    fn check_running(&self, id: &str) -> Result<(), ErrorBody> {
        let i = self.inner.lock().unwrap();
        match i.panes.get(id).map(|p| &p.info.state) {
            Some(SessionState::Running) => Ok(()),
            Some(_) => Err(err(ErrorCode::SessionExited, "세션이 끝났다")),
            None => Err(err(ErrorCode::NotFound, "pane이 없다")),
        }
    }

    // ───────────── enrollment ─────────────

    pub fn enroll_open(&self) -> bool {
        self.enroll.lock().unwrap().open_until.is_some_and(|t| t > Instant::now())
    }

    /// 등록 요청을 대기열에 넣는다. 승인되면 receiver가 끝난다.
    pub fn enroll_request(
        &self,
        name: &str,
        machine_id: &str,
        fingerprint: &str,
        sas: &str,
    ) -> Result<oneshot::Receiver<()>, ErrorBody> {
        if !self.enroll_open() {
            return Err(err(
                ErrorCode::EnrollClosed,
                "host에서 `aaa machine enroll --open 10m`으로 등록 창을 먼저 연다",
            ));
        }
        if !util::machine_name_ok(name) {
            return Err(err(ErrorCode::BadRequest, MACHINE_NAME_RULE));
        }
        if let Ok(Some(m)) = self.db.machine_by_name(name)
            && m.id != machine_id
        {
            return Err(err(ErrorCode::NameTaken, format!("이름 {name}은 다른 머신이 쓴다")));
        }
        let mut e = self.enroll.lock().unwrap();
        if e.pending.len() >= 8 && !e.pending.contains_key(name) {
            return Err(err(ErrorCode::Unavailable, "대기 중인 등록이 너무 많다"));
        }
        let (tx, rx) = oneshot::channel();
        e.pending.insert(
            name.to_string(),
            PendingRt {
                machine_id: machine_id.to_string(),
                fingerprint: fingerprint.to_string(),
                sas: sas.to_string(),
                since_ms: util::now_ms(),
                attempts: 0,
                accept: Some(tx),
            },
        );
        Ok(rx)
    }

    pub fn enroll_cancel(&self, name: &str, fingerprint: &str) {
        let mut e = self.enroll.lock().unwrap();
        if e.pending.get(name).is_some_and(|p| p.fingerprint == fingerprint) {
            e.pending.remove(name);
        }
    }

    fn approve(&self, name: &str, code: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let mut e = self.enroll.lock().unwrap();
        let Some(p) = e.pending.get_mut(name) else {
            return Err(err(ErrorCode::NotFound, format!("대기 중인 등록이 없다: {name}")));
        };
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        if code != p.sas {
            p.attempts += 1;
            if p.attempts >= 3 {
                e.pending.remove(name); // sender drop → agent 쪽은 거부로 끝난다
                self.db.audit(actor, "enroll_reject", name);
                return Err(err(ErrorCode::EnrollCodeMismatch, "확인 코드가 3번 틀려 이 등록 요청을 버렸다"));
            }
            return Err(err(ErrorCode::EnrollCodeMismatch, "확인 코드가 다르다(agent 화면의 코드를 입력한다)"));
        }
        let p = e.pending.remove(name).unwrap();
        self.db
            .upsert_machine(&p.machine_id, name, Some(&p.fingerprint), false)
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        self.inner.lock().unwrap().machines.insert(
            p.machine_id.clone(),
            MachineRt {
                name: name.to_string(),
                local: false,
                os: None,
                boot_id: None,
                dev_mode: false,
                conn: None,
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: vec![],
                agent_pid: None,
                colors: None,
            },
        );
        if let Some(tx) = p.accept {
            let _ = tx.send(());
        }
        self.db.audit(actor, "enroll_approve", name);
        drop(e);
        self.bump();
        Ok(FrontResponse::Done)
    }

    // ───────────── pairing ─────────────

    /// 새 pairing 코드. 아직 쓰지 않은 앞 코드는 버린다(herdr-web-ui `startPairing`: 「새 코드」·CLI가 같은 자리를 쓴다).
    /// 코드를 만드는 것은 주인(이 머신의 CLI나 controller 기기)이라, 앞서 틀린 시도의 잠금도 푼다: 남이 틀려 둔
    /// 잠금이 주인의 새 코드를 막지 않는다.
    fn mint_pairing(&self, role: DeviceRole, now: Instant) -> (String, u64) {
        let code = format!("{:06}", rand::random_range(0..1_000_000u32));
        let mut p = self.pairing.lock().unwrap();
        p.pending = Some(PendingCode { code: code.clone(), role, expires: now + PAIR_CODE_TTL, attempts: 0 });
        p.failures.clear();
        (code, PAIR_CODE_TTL.as_secs())
    }

    /// 웹 pairing 코드를 장치 token으로 바꾼다. 코드는 한 번만 쓰고, 한 코드로 5번 틀리면 그 코드를 버린다.
    /// 코드와 상관없이 10분 안에 5번 넘게 틀리면 1초부터 두 배씩 60초까지 기다리게 한다.
    pub fn redeem_pairing(&self, code: &str, label: &str) -> Result<(String, DeviceRole), ErrorBody> {
        self.redeem_pairing_at(code, label, Instant::now())
    }

    fn redeem_pairing_at(&self, code: &str, label: &str, now: Instant) -> Result<(String, DeviceRole), ErrorBody> {
        let mut p = self.pairing.lock().unwrap();
        p.forget_old(now);
        if let Some(t) = p.open_at()
            && t > now
        {
            return Err(err(
                ErrorCode::Unavailable,
                format!("시도가 너무 많다. {}초 뒤 다시 한다", (t - now).as_secs() + 1),
            ));
        }
        if p.pending.as_ref().is_some_and(|c| c.expires <= now) {
            p.pending = None;
        }
        let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
        // 걸린 시간으로 자리를 알 수 없게 끝까지 비교한다(참고 `timingSafeEqual`)
        let right = p.pending.as_ref().is_some_and(|c| {
            c.code.len() == code.len() && c.code.bytes().zip(code.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0
        });
        if !right {
            if let Some(c) = p.pending.as_mut() {
                c.attempts += 1;
                if c.attempts >= PAIR_CODE_ATTEMPTS {
                    p.pending = None;
                }
            }
            p.failures.push_back(now);
            return Err(err(ErrorCode::Unauthorized, "코드가 틀렸거나 이미 썼거나 만료됐다"));
        }
        let role = p.pending.take().map(|c| c.role).unwrap_or(DeviceRole::Reader);
        p.failures.clear();
        drop(p);
        let token =
            base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, rand::random::<[u8; 32]>());
        let id = util::new_id();
        let role_s = if role == DeviceRole::Reader { "reader" } else { "controller" };
        let label = normalize_device_label(label, db::DEVICE_LABEL_FALLBACK);
        self.db
            .add_device(&id, role_s, &token_hash(&token), &label)
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        self.db.audit(&format!("device:{id}"), "web_paired", role_s);
        Ok((token, role))
    }

    /// cookie token의 기기. 들어올 때마다 「마지막으로 본 때」를 1분에 한 번까지 적는다(참고 `match`)
    pub fn device_role(&self, token: &str) -> Option<(String, DeviceRole)> {
        let d = self.db.device_by_token_hash(&token_hash(token)).ok()??;
        let now = util::now_ms();
        if d.last_seen_ms.is_none_or(|seen| now.saturating_sub(seen) >= DEVICE_SEEN_WRITE_MS) {
            // 기록은 입장을 막지 않는다
            let _ = self.db.device_seen(&d.id, now);
        }
        let role = if d.role == "reader" { DeviceRole::Reader } else { DeviceRole::Controller };
        Some((d.id, role))
    }

    /// 기기를 취소하고 그 기기의 열린 연결에 알린다. 없으면(이미 취소) false
    pub fn revoke_device(&self, id: &str) -> Result<bool, ErrorBody> {
        let done = self.db.revoke_device(id).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        if done {
            self.revoked_devices.lock().unwrap().insert(id.to_string());
            let _ = self.device_revoked.send(id.to_string());
        }
        Ok(done)
    }

    /// 열린 연결의 기기가 이 host가 돈 뒤 취소됐나(그 전에 취소된 기기는 cookie 검사에서 이미 막힌다)
    pub fn is_revoked(&self, id: &str) -> bool {
        self.revoked_devices.lock().unwrap().contains(id)
    }

    // ───────────── 웹 공개 ─────────────

    async fn publish_status(&self) -> PublishState {
        let out = tailscale::read().await;
        let plan = tailscale::plan(out.as_ref(), self.web_port(), &self.cfg.web.publish_ports);
        let owned = self.db.meta("publish_port").ok().flatten().and_then(|p| p.parse().ok());
        if let Some(dns) = &plan.dns_name {
            let _ = self.db.set_meta("publish_dns", dns);
        }
        // 웹이 받는 Host·Origin은 이 주소(포트까지)뿐이다. tailscale이 돌 때만 고친다: 꺼졌거나 못 읽으면 지난 값을 둔다
        match &plan.serving {
            Some((url, _)) => {
                let _ = self.db.set_meta("publish_origin", &url.to_ascii_lowercase());
            }
            None if plan.state == "running" => {
                let _ = self.db.del_meta("publish_origin");
            }
            None => {}
        }
        PublishState {
            tailscale: plan.state.into(),
            dns_name: plan.dns_name.clone(),
            url: plan.serving.as_ref().map(|(u, _)| u.clone()),
            owned_port: owned,
            web_port: self.web_port(),
            message: match plan.state {
                "missing" => Some("tailscale CLI가 없다. 웹은 127.0.0.1에서만 열린다".into()),
                "stopped" => Some("Tailscale이 꺼졌거나 로그인 전이다".into()),
                _ if plan.serving.is_none() => Some("아직 공개하지 않았다: aaa web publish".into()),
                _ => None,
            },
        }
    }

    async fn publish(&self) -> Result<PublishState, ErrorBody> {
        if self.web_port() == 0 {
            return Err(err(ErrorCode::BadRequest, "host.toml의 web.listen이 꺼져 있다"));
        }
        let out = tailscale::read().await;
        let plan = tailscale::plan(out.as_ref(), self.web_port(), &self.cfg.web.publish_ports);
        if plan.state != "running" {
            return Ok(self.publish_status().await);
        }
        if plan.serving.is_none() {
            let Some(port) = plan.free_port else {
                return Err(err(ErrorCode::Unavailable, format!("후보 포트가 모두 쓰이고 있다: {:?}", plan.taken)));
            };
            let text = tailscale::serve(port, self.web_port())
                .await
                .map_err(|e| err(ErrorCode::Unavailable, format!("{e:#}")))?;
            tracing::info!("tailscale serve: {}", text.trim());
            let _ = self.db.set_meta("publish_port", &port.to_string());
        }
        Ok(self.publish_status().await)
    }

    async fn publish_off(&self) -> Result<PublishState, ErrorBody> {
        let owned: Option<u16> = self.db.meta("publish_port").ok().flatten().and_then(|p| p.parse().ok());
        let Some(port) = owned else {
            return Err(err(ErrorCode::NotFound, "aaa가 만든 공개가 없다(다른 서비스의 매핑은 건드리지 않는다)"));
        };
        tailscale::serve_off(port).await.map_err(|e| err(ErrorCode::Unavailable, format!("{e:#}")))?;
        let _ = self.db.del_meta("publish_port");
        Ok(self.publish_status().await)
    }

    /// 웹 요청의 Host 헤더가 이 host를 가리키는가(DNS rebinding 방어)
    pub fn web_port(&self) -> u16 {
        self.web_port.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Host 헤더가 가리키는 이 host의 origin. `http://127.0.0.1:<web_port>`와 tailscale serve 공개 주소
    /// (`https://<이름>[:포트]`, 443은 포트 없이) 둘뿐이고 포트까지 같아야 한다(§6.3, herdr-web-ui `sameOrigin`이
    /// 요청 주소의 origin 전체를 비교하는 것과 같다). 아니면 None
    pub fn web_origin(&self, host_header: &str) -> Option<String> {
        let loopback = format!("127.0.0.1:{}", self.web_port());
        if host_header == loopback {
            return Some(format!("http://{loopback}"));
        }
        let public = self.public_origin()?;
        let asked = host_header.to_ascii_lowercase();
        let asked = asked.strip_suffix(":443").unwrap_or(&asked);
        (format!("https://{asked}") == public).then_some(public)
    }

    /// 공개 origin. 아직 serve 상태를 읽지 못했으면(새 binary로 바꾼 뒤 tailscale을 못 읽음) aaa가 만든 공개
    /// (`publish_port`)와 이름으로 대신한다: 둘 다 포트까지 안다
    fn public_origin(&self) -> Option<String> {
        if let Some(o) = self.db.meta("publish_origin").ok().flatten() {
            return Some(o);
        }
        let dns = self.db.meta("publish_dns").ok().flatten()?;
        let port = self.db.meta("publish_port").ok().flatten()?.parse().ok()?;
        Some(tailscale::https_url(&dns, port).to_ascii_lowercase())
    }

    pub fn host_allowed(&self, host_header: &str) -> bool {
        self.web_origin(host_header).is_some()
    }

    /// 공개 주소를 tailscale에서 다시 읽는다(host 시작 때: 지난 실행 뒤 serve 포트가 바뀌었을 수 있다)
    pub async fn refresh_publish(&self) {
        let _ = self.publish_status().await;
    }

    /// host와 같은 머신의 agent인가(brief의 uploads 주석)
    pub(crate) fn machine_is_local(&self, machine_id: &str) -> bool {
        self.inner.lock().unwrap().machines.get(machine_id).is_some_and(|m| m.local)
    }

    pub fn local_machine_connected(&self) -> bool {
        self.inner.lock().unwrap().machines.values().any(|m| m.local && m.conn.is_some())
    }
}

/// `aaa host` 실행
pub async fn run(cfg: HostConfig, state_root: PathBuf) -> Result<()> {
    let host = Host::open(cfg, state_root.join("host"))?;
    let _ = host.config_dir.set(aaa_config::paths::config_dir());
    tracing::info!(host = %host.host_id, fp = %&host.identity.fingerprint[..16], "host 시작");
    // 한 host는 프로젝트 하나(plan §8.283): 입구를 열기 전에 본다. 보관 상태가 DB에 있어 open 뒤에야 셀 수 있다
    match host.sole_project() {
        Ok(project) => tracing::info!(project = project.as_deref().unwrap_or("(아직 없음)"), "이 host의 프로젝트"),
        Err(names) => anyhow::bail!(lifecycle::sole_project_refusal(&names)),
    }
    // 옛 판이 남긴 보내던 메시지는 입구를 열기 전에 잊는다(연 뒤면 막 기록한 새 메시지까지 「모른다」로 바뀐다)
    host.forget_queued_messages();
    link::start(&host).await?;
    front::start(&host).await?;
    web::start(&host).await?;
    host.resume_rollout();
    host.start_ticker();
    host.start_notifier();
    if host.cfg.local_agent.mode == LocalAgentMode::Child {
        let h = host.clone();
        let root = state_root.clone();
        // 개발 모드 감시: 로컬 agent가 없으면 띄운다(운영 모드에서는 LaunchAgent KeepAlive가 한다).
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1500)).await;
            let mut missing_since: Option<Instant> = Some(Instant::now() - Duration::from_secs(10));
            loop {
                if h.local_machine_connected() {
                    missing_since = None;
                } else {
                    let since = *missing_since.get_or_insert_with(Instant::now);
                    if since.elapsed() >= Duration::from_secs(3) {
                        if let Err(e) = spawn_local_agent(&root) {
                            tracing::error!("로컬 agent 기동 실패: {e:#}");
                        }
                        missing_since = Some(Instant::now() + Duration::from_secs(5));
                    }
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        });
    }
    shutdown_signal().await;
    tracing::info!("host 종료(세션은 agent·session host가 계속 유지한다)");
    Ok(())
}

/// 개발 모드: 로컬 agent backend를 분리된 프로세스로 띄운다(결정 3 MVP 허용). 운영 모드에서는 LaunchAgent가 띄운다.
fn spawn_local_agent(state_root: &std::path::Path) -> Result<()> {
    let dir = state_root.join("agent");
    util::ensure_private_dir(&dir)?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("agent.log"))?;
    let mut cmd = std::process::Command::new(std::env::current_exe()?);
    cmd.args(["agent", "--host", "local", "--dev-mode"]);
    cmd.stdin(std::process::Stdio::null()).stdout(log.try_clone()?).stderr(log);
    #[cfg(unix)]
    unsafe {
        use std::os::unix::process::CommandExt;
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    tracing::info!(pid = child.id(), "개발 모드: 로컬 agent를 띄웠다");
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut term = signal(SignalKind::terminate()).expect("SIGTERM");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod pairing_tests {
    //! pairing·기기(§6.4, 차이 목록 P1·P2). 시각은 넣어 준 `now`로 잰다(herdr-web-ui devices.test.ts처럼).
    use super::*;

    struct Tmp(PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn host() -> (Arc<Host>, Tmp) {
        let dir = std::env::temp_dir().join(format!("aaa-pairing-test-{}", util::new_id()));
        let mut cfg = HostConfig::default();
        cfg.notify.web_push = false;
        (Host::open(cfg, dir.clone()).unwrap(), Tmp(dir))
    }

    async fn mint(h: &Arc<Host>, by: DeviceRole, role: DeviceRole) -> Result<String, ErrorBody> {
        match h.call(by, "test", FrontRequest::WebPair { role }).await? {
            FrontResponse::PairCode { code, expires_in_s, .. } => {
                assert_eq!(expires_in_s, 600);
                Ok(code)
            }
            other => panic!("{other:?}"),
        }
    }

    async fn devices(h: &Arc<Host>) -> Vec<aaa_proto::DeviceView> {
        match h.call(DeviceRole::Reader, "test", FrontRequest::Devices).await.unwrap() {
            FrontResponse::Devices { devices } => devices,
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn trust_management_requires_a_person_outside_the_session() {
        let (h, _t) = host();
        let requests = [
            FrontRequest::WebPair { role: DeviceRole::Controller },
            FrontRequest::EnrollOpen { minutes: 1 },
            FrontRequest::EnrollApprove { name: "missing".into(), code: "000000".into() },
            FrontRequest::MachineRevoke { name: "missing".into() },
            FrontRequest::DeviceRevoke { id: "missing".into() },
            FrontRequest::WebPublish,
            FrontRequest::WebPublishOff,
        ];
        for actor in
            ["local:cli@coordinator", "local:cli@unverified", "worker:thread", "evaluator:thread", "detached:cli"]
        {
            for request in &requests {
                // actor 경계 자체가 Local 권한으로도 거부해야 한다.
                let e = h.call(DeviceRole::Local, actor, request.clone()).await.expect_err(actor);
                assert_eq!(e.code, ErrorCode::Forbidden, "{actor}: {request:?}: {e:?}");
                assert!(e.message.contains("사람이 aaa 밖 터미널이나 웹에서"), "{e:?}");
            }
        }
        for (role, actor) in [(DeviceRole::Local, "local:cli"), (DeviceRole::Controller, "device:person")] {
            let code = match h.call(role, actor, requests[0].clone()).await.unwrap() {
                FrontResponse::PairCode { code, .. } => code,
                other => panic!("{other:?}"),
            };
            let (token, _) = h.redeem_pairing(&code, "person").unwrap();
            let (id, _) = h.device_role(&token).unwrap();
            h.call(role, actor, FrontRequest::DeviceRevoke { id }).await.unwrap();
            assert!(h.device_role(&token).is_none());
        }
        h.call(DeviceRole::Local, "local:cli", requests[1].clone()).await.unwrap();
        for request in &requests[2..4] {
            assert_ne!(
                h.call(DeviceRole::Local, "local:cli", request.clone()).await.unwrap_err().code,
                ErrorCode::Forbidden
            );
        }
        // W0는 기존 로컬 전용 API를 웹에 새로 열지 않는다(W5).
        assert_eq!(
            h.call(DeviceRole::Controller, "device:person", requests[1].clone()).await.unwrap_err().code,
            ErrorCode::Forbidden
        );
    }

    #[tokio::test]
    async fn a_code_is_used_once_and_carries_its_role() {
        let (h, _t) = host();
        let code = mint(&h, DeviceRole::Local, DeviceRole::Reader).await.unwrap();
        let (token, role) = h.redeem_pairing(&code, "  iPhone \n · Safari ").unwrap();
        assert_eq!(role, DeviceRole::Reader);
        assert_eq!(h.device_role(&token).map(|(_, r)| r), Some(DeviceRole::Reader));
        assert_eq!(h.redeem_pairing(&code, "x").unwrap_err().code, ErrorCode::Unauthorized);
        let list = devices(&h).await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].label, "iPhone · Safari");
        assert_eq!(list[0].role, DeviceRole::Reader);
        // 처음 들어올 때 「마지막으로 본 때」를 적는다
        assert!(list[0].last_seen_ms.is_some());
    }

    #[tokio::test]
    async fn a_new_code_replaces_the_one_still_out() {
        let (h, _t) = host();
        let first = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
        let mut second = mint(&h, DeviceRole::Controller, DeviceRole::Controller).await.unwrap();
        // 같은 숫자가 나올 수 있다(1/10⁶): 다른 코드가 나올 때까지 다시 만든다
        while second == first {
            second = mint(&h, DeviceRole::Controller, DeviceRole::Controller).await.unwrap();
        }
        assert_eq!(h.redeem_pairing(&first, "").unwrap_err().code, ErrorCode::Unauthorized);
        let (_, role) = h.redeem_pairing(&second, "").unwrap();
        assert_eq!(role, DeviceRole::Controller);
        assert_eq!(devices(&h).await[0].label, db::DEVICE_LABEL_FALLBACK);
    }

    #[tokio::test]
    async fn a_code_expires_after_ten_minutes() {
        let (h, _t) = host();
        let now = Instant::now();
        let (code, _) = h.mint_pairing(DeviceRole::Controller, now);
        assert!(h.redeem_pairing_at(&code, "", now + PAIR_CODE_TTL).is_err());
        let (code, _) = h.mint_pairing(DeviceRole::Controller, now);
        assert!(h.redeem_pairing_at(&code, "", now + PAIR_CODE_TTL - Duration::from_secs(1)).is_ok());
    }

    #[tokio::test]
    async fn five_wrong_tries_spend_the_code_and_then_wait_doubles_up_to_a_minute() {
        let (h, _t) = host();
        let now = Instant::now();
        let (code, _) = h.mint_pairing(DeviceRole::Controller, now);
        let wrong = if code == "000000" { "111111" } else { "000000" };
        for _ in 0..5 {
            assert_eq!(h.redeem_pairing_at(wrong, "", now).unwrap_err().code, ErrorCode::Unauthorized);
        }
        // 기다리는 동안은 맞는 코드도 비교하지 않는다
        assert_eq!(h.redeem_pairing_at(&code, "", now).unwrap_err().code, ErrorCode::Unavailable);
        // 1초 뒤에는 받지만 그 코드는 이미 버렸다(한 코드로 5번)
        let mut at = now + PAIR_BACKOFF;
        assert_eq!(h.redeem_pairing_at(&code, "", at).unwrap_err().code, ErrorCode::Unauthorized);
        // 틀릴 때마다 기다림이 두 배, 60초에서 멈춘다(참고 1초·60초)
        let mut waits = vec![];
        for _ in 0..10 {
            let wait = (0..=120)
                .map(Duration::from_secs)
                .find(|d| h.redeem_pairing_at(wrong, "", at + *d).unwrap_err().code == ErrorCode::Unauthorized)
                .unwrap();
            waits.push(wait.as_secs());
            at += wait;
        }
        assert_eq!(waits, [2, 4, 8, 16, 32, 60, 60, 60, 60, 60]);
    }

    #[tokio::test]
    async fn wrong_tries_older_than_ten_minutes_are_forgotten() {
        let (h, _t) = host();
        let now = Instant::now();
        h.mint_pairing(DeviceRole::Controller, now);
        for i in 0..5 {
            // 다른 코드로 틀린다(한 코드의 5번 한도와 따로 센다)
            let _ = h.redeem_pairing_at(&format!("{i:06}x"), "", now);
        }
        assert_eq!(h.redeem_pairing_at("1", "", now).unwrap_err().code, ErrorCode::Unavailable);
        // 창이 지나면 처음처럼 기다림 없이 다섯 번
        let later = now + PAIR_FAIL_WINDOW;
        for _ in 0..5 {
            assert_eq!(h.redeem_pairing_at("1", "", later).unwrap_err().code, ErrorCode::Unauthorized);
        }
        assert_eq!(h.redeem_pairing_at("1", "", later).unwrap_err().code, ErrorCode::Unavailable);
    }

    #[tokio::test]
    async fn a_new_code_from_the_owner_lifts_the_wait() {
        let (h, _t) = host();
        let now = Instant::now();
        h.mint_pairing(DeviceRole::Controller, now);
        for _ in 0..12 {
            let _ = h.redeem_pairing_at("1", "", now);
        }
        // CLI(`aaa web pair`)나 controller 기기가 새 코드를 만들면 남이 틀려 둔 잠금 없이 바로 쓴다
        let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
        assert!(h.redeem_pairing_at(&code, "", now).is_ok());
    }

    #[tokio::test]
    async fn a_view_path_ends_with_the_device_that_got_it() {
        let (h, _t) = host();
        let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
        let (token, _) = h.redeem_pairing(&code, "").unwrap();
        let (id, _) = h.device_role(&token).unwrap();
        let until = Instant::now() + library::VIEW_TTL;
        let cap = |device: Option<&str>| library::ViewCap {
            project: "nope".into(),
            thread: "t-0001".into(),
            until,
            device: device.map(str::to_string),
        };
        h.view_caps.lock().unwrap().insert("dev".into(), cap(Some(&id)));
        h.view_caps.lock().unwrap().insert("cli".into(), cap(None));
        // 경로 검사를 지나면 다음은 프로젝트 찾기(없는 프로젝트)다
        let reach = |name: &'static str| {
            let h = h.clone();
            async move { h.view_source(name, "a.txt").await.err().map(|e| e.code) }
        };
        assert_ne!(reach("dev").await, Some(ErrorCode::Unauthorized));
        h.revoke_device(&id).unwrap();
        assert_eq!(reach("dev").await, Some(ErrorCode::Unauthorized));
        assert_ne!(reach("cli").await, Some(ErrorCode::Unauthorized));
    }

    #[tokio::test]
    async fn the_web_mints_controller_codes_and_a_reader_manages_nothing() {
        let (h, _t) = host();
        assert_eq!(mint(&h, DeviceRole::Controller, DeviceRole::Reader).await.unwrap_err().code, ErrorCode::Forbidden);
        assert_eq!(mint(&h, DeviceRole::Reader, DeviceRole::Controller).await.unwrap_err().code, ErrorCode::Forbidden);
        let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
        h.redeem_pairing(&code, "Mac · Chrome").unwrap();
        let id = devices(&h).await[0].id.clone();
        for req in [
            FrontRequest::DeviceRename { id: id.clone(), label: "x".into() },
            FrontRequest::DeviceRevoke { id: id.clone() },
        ] {
            assert_eq!(h.call(DeviceRole::Reader, "test", req).await.unwrap_err().code, ErrorCode::Forbidden);
        }
        let long = "가".repeat(60);
        h.call(DeviceRole::Controller, "test", FrontRequest::DeviceRename { id: id.clone(), label: long })
            .await
            .unwrap();
        assert_eq!(devices(&h).await[0].label.chars().count(), 48);
        let missing = FrontRequest::DeviceRename { id: "nope".into(), label: "x".into() };
        assert_eq!(h.call(DeviceRole::Controller, "test", missing).await.unwrap_err().code, ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn revoking_tells_open_connections_and_drops_the_token_and_push() {
        let (h, _t) = host();
        let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
        let (token, _) = h.redeem_pairing(&code, "").unwrap();
        let (id, _) = h.device_role(&token).unwrap();
        let sub = db::PushSub {
            endpoint: "https://push.example/a".into(),
            p256dh: "k".into(),
            auth: "a".into(),
            device: id.clone(),
            alerts: None,
        };
        h.db.push_put(&sub).unwrap();
        let mut heard = h.device_revoked.subscribe();
        assert!(!h.is_revoked(&id));
        h.call(DeviceRole::Controller, "test", FrontRequest::DeviceRevoke { id: id.clone() }).await.unwrap();
        assert_eq!(heard.try_recv().unwrap(), id);
        assert!(h.is_revoked(&id));
        assert!(h.device_role(&token).is_none());
        assert!(h.db.push_list(None).unwrap().is_empty());
        assert!(devices(&h).await.is_empty());
        let again = FrontRequest::DeviceRevoke { id };
        assert_eq!(h.call(DeviceRole::Local, "test", again).await.unwrap_err().code, ErrorCode::NotFound);
    }

    /// 보안 감사 t-0340 B10: endpoint만 아는 다른 기기는 구독을 가져가지 못하고, 키까지 같은 같은 브라우저(다시 pairing)는 옮긴다
    #[tokio::test]
    async fn another_device_takes_a_push_endpoint_only_with_its_keys() {
        let (h, _t) = host();
        let mut ids = Vec::new();
        for _ in 0..2 {
            let code = mint(&h, DeviceRole::Local, DeviceRole::Controller).await.unwrap();
            let (token, _) = h.redeem_pairing(&code, "").unwrap();
            ids.push(h.device_role(&token).unwrap().0);
        }
        let sub = |device: &str, auth: &str| db::PushSub {
            endpoint: "https://push.example/a".into(),
            p256dh: "k".into(),
            auth: auth.into(),
            device: device.into(),
            alerts: None,
        };
        assert!(h.db.push_put(&sub(&ids[0], "a")).unwrap());
        // 같은 기기는 키를 바꿀 수 있다
        assert!(h.db.push_put(&sub(&ids[0], "a2")).unwrap());
        assert!(!h.db.push_put(&sub(&ids[1], "x")).unwrap());
        assert_eq!(h.db.push_list(Some(&ids[0])).unwrap().len(), 1);
        assert!(h.db.push_list(Some(&ids[1])).unwrap().is_empty());
        assert!(h.db.push_put(&sub(&ids[1], "a2")).unwrap());
        assert!(h.db.push_list(Some(&ids[0])).unwrap().is_empty());
        assert_eq!(h.db.push_list(Some(&ids[1])).unwrap().len(), 1);
        // 기기가 고른 알림 종류(N3): 주면 바꾸고, 없이 다시 넣으면(불러올 때마다 다시 구독) 둔다
        let alerts = |h: &Host| h.db.push_list(Some(&ids[1])).unwrap()[0].alerts.unwrap();
        assert_eq!(alerts(&h), db::Alerts::default());
        let quiet = db::Alerts { input: false, done: true };
        assert!(h.db.push_put(&db::PushSub { alerts: Some(quiet), ..sub(&ids[1], "a2") }).unwrap());
        assert_eq!(alerts(&h), quiet);
        assert!(h.db.push_put(&sub(&ids[1], "a2")).unwrap());
        assert_eq!(alerts(&h), quiet);
    }

    /// 보안 감사 t-0340 B5: 제어 문자·공백이 든 이름으로는 등록을 기다리지 못한다(`aaa machine pending` 화면·push 본문 위조)
    #[tokio::test]
    async fn enroll_rejects_machine_names_with_control_characters() {
        let (h, _t) = host();
        h.enroll.lock().unwrap().open_until = Some(Instant::now() + Duration::from_secs(60));
        for bad in ["mac\x1b[2Ja", "mac a", "a\u{202e}b", ""] {
            let e = h.enroll_request(bad, "01M", "fp", "sas").unwrap_err();
            assert_eq!(e.code, ErrorCode::BadRequest, "{bad:?}");
        }
        assert!(h.enroll.lock().unwrap().pending.is_empty());
        assert!(h.enroll_request("mac-b", "01M", "fp", "sas").is_ok());
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;

    fn info(argv: &[&str], state: SessionState, role: Option<aaa_proto::Role>) -> SessionInfo {
        SessionInfo {
            id: "s".into(),
            name: "s".into(),
            argv: argv.iter().map(|a| a.to_string()).collect(),
            cwd: None,
            pid: None,
            started_at_ms: 0,
            boot_id: "b".into(),
            cols: 80,
            rows: 24,
            state,
            role,
            label: None,
            harness: None,
        }
    }

    /// 스스로 끝난 셸만 닫는다. 복원 대상(재부팅·유실)·host가 끝낸 것·역할·프로그램 pane·실행 중을 못 본 것은 남긴다.
    #[test]
    fn only_a_shell_that_exited_by_itself_closes() {
        use SessionState::*;
        let exited = || Exited { code: Some(0) };
        let role = Some(aaa_proto::Role::Worker);
        let cases = [
            ("기본 셸 exit", info(&[], Running, None), info(&[], exited(), None), false, true),
            (
                "login 셸 exit 3",
                info(&["/bin/zsh", "-l"], Running, None),
                info(&["/bin/zsh", "-l"], Exited { code: Some(3) }, None),
                false,
                true,
            ),
            ("pane kill", info(&["sh"], Running, None), info(&["sh"], exited(), None), true, false),
            ("역할 pane", info(&["sh"], Running, role), info(&["sh"], exited(), role), false, false),
            (
                "프로그램 pane",
                info(&["sh", "-c", "exit 0"], Running, None),
                info(&["sh", "-c", "exit 0"], exited(), None),
                false,
                false,
            ),
            ("재부팅", info(&["sh"], Running, None), info(&["sh"], Interrupted, None), false, false),
            ("유실", info(&["sh"], Running, None), info(&["sh"], Lost, None), false, false),
            ("이미 끝나 있던 것", info(&["sh"], Interrupted, None), info(&["sh"], exited(), None), false, false),
        ];
        for (what, old, new, kill_sent, want) in cases {
            assert_eq!(exited_by_itself(&old, &new, kill_sent), want, "{what}");
        }
    }
}
