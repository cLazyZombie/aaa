//! `aaa host`: host backend. 정본 목록·권한·Front API·웹을 갖고, 각 머신의 agent backend와 Agent link로 통신한다.
//!
//! - host의 자식 실행은 tailscale CLI, 개발 모드 로컬 agent(§4.2), 세션 밖 CLI가 요청한 격리 공식 로그인뿐이다.
//! - pane 화면은 agent(session host)가 정본이다. host는 마지막 화면 mirror를 가져 agent가 끊겨도 읽기 전용으로 보여 준다.

use aaa_common::child_process::StdCommandExt as _;
mod adopt;
mod answer;
mod answer_delivery;
mod app_badge;
mod ask_correct;
mod asks;
mod attach;
mod auto_merge;
mod auto_mode;
mod question_ids;
/// 자동화 정본 타입의 편집기용 JSON Schema. host 상태를 읽지 않는다.
pub fn automation_schema() -> String {
    serde_json::to_string_pretty(&automation::schema()).expect("schema serialize")
}
mod account_login;
mod account_meta;
mod account_plan;
mod account_policy;
mod account_pool;
mod account_rename;
mod accounts;
mod agent_conn;
mod agy_quota;
mod auto_start;
mod automation;
mod automation_thread;
pub mod backup;
mod bgwait;
mod brief;
mod brief_text;
mod caller;
mod chat_rest;
mod chatwatch;
mod ci;
mod colors;
mod coord;
mod ctrl_enter;
pub mod db;
mod deploy;
mod design;
mod design_branch;
mod design_chat;
mod design_handoff;
mod design_ids;
mod diff;
mod enroll_guide;
pub mod file_backup;
mod file_rev;
mod files;
mod followups;
pub mod front;
mod git_watch;
pub mod guide;
mod harness;
mod health;
mod http;
mod image;
mod image_rpc;
mod inbox;
mod input_log;
pub mod installation;
mod legacy_mission;
mod library;
mod library_archive;
mod library_catalog;
mod library_delete;
mod library_inventory;
mod library_links;
mod library_publish;
mod library_retention;
mod library_upload;
mod lifecycle;
pub mod link;
mod machine_rename;
mod machine_reserve;
mod machine_setup;
pub mod migrate;
mod notice;
mod notify;
mod open;
mod orch;
mod output;
mod park;
mod passkey;
mod post;
mod prefs;
mod project;
mod prompt;
mod prompt_box;
mod prompt_receipt;
mod push;
mod question_guard;
mod question_repeat;
mod quota;
mod quota_rpc;
mod record_search;
mod report_copy;
mod report_ids;
mod report_mark;
mod report_sandbox;
mod resource;
mod restore;
mod review;
mod rollout;
mod routine;
mod routine_runs;
pub mod safe_mode;
mod seen;
mod settings_admin;
mod short_ids;
mod summary;
mod supervisor;
pub mod tailscale;
mod task_attach;
mod task_clean;
mod task_edit;
mod task_ids;
mod task_queue;
mod task_repo;
mod tasks;
mod text;
mod thread_read;
mod thread_stall;
mod transitions;
mod trust_screen;
pub mod usage;
mod usage_limit;
mod watch;
mod watch_event;
pub mod web;
mod web_publication;
mod workspace_access;
mod workspace_metrics;
pub use agent_conn::*;
mod agent_sync;
mod dispatch;
mod enroll;
mod pairing;
mod pane_ops;
mod views;
mod workspaces;
pub use pairing::*;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aaa_config::{HostConfig, LocalAgentMode};
use aaa_proto::{
    AgentRequest, AgentResponse, Connection, DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontRequest, FrontResponse,
    HostStatus, LayoutChange, LayoutNode, LeaseView, LinkMsg, MachineView, Outcome, PaneView, PendingEnroll,
    ScreenText, SessionInfo, SessionState, SpawnSpec, Tab, TerminalChunk, WorkRootOp, Workspace,
};
use anyhow::{Context, Result};
use sha2::Digest;
use tokio::sync::{broadcast, oneshot, watch as tokio_watch};

use aaa_common::conn::FrameTx;
use aaa_common::util;
use aaa_vt::Vt;

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
    /// agent가 hello에서 알린 실행 신원(관리 GUI W6c 재시작 rollout의 재시작 전 신원). 옛 agent는 없다
    identity: Option<aaa_proto::RuntimeIdentity>,
    /// 로컬 agent 프로세스(로컬 socket peer credential). 그 세션에서 띄운 명령(routine 명령)은 떼어 낸 프로세스가 아니다(plan §8.164).
    agent_pid: Option<u32>,
    /// 이 연결에 화면 색을 차례로 보내는 곳(agent 기능 `term_colors.v1`, colors.rs). 놓으면 보내던 task가 끝난다
    colors: Option<colors::ColorPushes>,
    /// 마지막 hello의 CPU 종류·판·SHA·자가 점검(웹 머신 상세, plan §8.372)
    build: MachineBuild,
}

/// 머신 상세에 보이는 마지막 hello의 실행 파일 정보. 앞의 셋은 host DB에도 남는다(자가 점검은 이 host 실행 동안만)
#[derive(Default)]
struct MachineBuild {
    arch: Option<String>,
    software: Option<String>,
    sha256: Option<String>,
    selfcheck: Option<aaa_proto::MachineSelfcheck>,
}

impl MachineBuild {
    fn of(hello: &aaa_proto::AgentHello) -> Self {
        MachineBuild {
            arch: Some(hello.arch.clone()),
            software: Some(hello.software.clone()),
            sha256: hello.sha256.clone(),
            // agent가 정한 꼴(`pass`·`reasons`)만 읽는다. 모르는 꼴이면 보이지 않는다
            selfcheck: hello.selfcheck.as_ref().and_then(|v| {
                serde_json::from_value(v.clone())
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            }),
        }
    }
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
    /// 모델 관측과 별개인 Codex 실행 속도와 관측 시각.
    codex_speed: Option<(aaa_proto::CodexSpeed, u64)>,
    /// 지금 턴이 시작된 때(작업·질문으로 들어선 harness 시각, notice.rs `turn_from`). 턴 밖이거나 시작을 못 봤으면 None
    turn_from: Option<u64>,
    /// 이 턴에서 완료·확인 필요 보고를 이미 알렸으면 턴 끝남 카드를 중복하지 않는다.
    turn_reported: bool,
    /// 알림이 마지막으로 본 상태(notice.rs `alert_activity`: BG로 기다리는 쉼은 작업 중). 턴의 시작·끝은 이것이 바뀔 때다
    alert: Option<aaa_proto::AgentActivity>,
    /// 지금 막힘에서 답한 질문(종류, 본문). brief 화면 읽기가 첫 harness 질문 관측보다 앞설 수 있다.
    /// 막힘을 벗어나면 지운다: 늦게 온 같은 첫 관측과 다른 새 질문을 구별한다(harness.rs `on_harness`).
    answered: Option<(String, String)>,
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
    pub(crate) peer_pid: Option<u32>,
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
    installation: Option<aaa_proto::install::InstallEnrollmentBinding>,
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
    backup_running: AtomicBool,
    file_backup_running: AtomicBool,
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
    pub workspace_metrics_changed: tokio_watch::Sender<u64>,
    /// 「답할 것」의 확인 필요 post·답한 것 기록이 바뀌었다(plan §8.345). 웹 events가 `asks` 사건을 보낸다
    pub asks_changed: tokio_watch::Sender<u64>,
    /// 알림함이 바뀐 횟수(웹 알림함 배지가 다시 읽는다)
    pub inbox_changed: tokio_watch::Sender<u64>,
    /// host 계정이 바뀐 횟수(로그인 진행·끝, 기본, 메일·상태). 웹 계정 페이지가 다시 읽는다
    pub accounts_changed: tokio_watch::Sender<u64>,
    /// 머신 등록 창·대기 목록이 바뀐 횟수(웹 「머신」 페이지가 다시 읽는다)
    pub enroll_changed: tokio_watch::Sender<u64>,
    /// SSH 머신 추가 작업을 읽고 바꾸는 한 차례(machine_setup.rs)
    setup_gate: Mutex<()>,
    machine_rename_gate: Mutex<()>,
    machine_label_gate: tokio::sync::Mutex<()>,
    metrics_gate: tokio::sync::Mutex<()>,
    metrics_pending: Mutex<HashSet<String>>,
    /// 계정 기록(`account-meta:*`) 읽고-고쳐-쓰기를 하나씩(로그인 끝·갱신 결과·ticker 상태 pass가 겹친다)
    account_meta_gate: Mutex<()>,
    account_rename_gate: account_rename::Gate,
    account_rename_recovery: Mutex<()>,
    /// 웹 기기 폐기의 「마지막 조작 기기」 세기와 폐기를 묶는다
    device_gate: Mutex<()>,
    publish_gate: tokio::sync::Mutex<()>,
    upload_gate: Arc<library_upload::UploadGate>,
    /// EMFILE 중에는 파일을 더 열지 않고, ticker가 여유가 생긴 뒤 시스템 경고를 저장한다.
    resource_exhausted: std::sync::atomic::AtomicBool,
    health: Mutex<supervisor::Health>,
    health_wake: tokio::sync::Notify,
    enroll: Mutex<Enroll>,
    pairing: Mutex<Pairing>,
    install_probes: Mutex<HashMap<String, (String, Instant)>>,
    pub agent_listen: Mutex<Option<String>>,
    pub web_listen: Mutex<Option<String>>,
    /// 거짓인 동안 같은 머신 agent에 Welcome을 보내지 않는다. `run_host`가 웹을 열 때까지만 닫는다(W6c): 그 Welcome의 host 실행
    /// 신원에 실제로 연 웹 주소가 실려야 host 감시가 웹 응답까지 본다. 웹은 link 뒤에 연다(먼저 열면 `:0` 웹이 고정한 agent port를 쥘 수 있다)
    pub(crate) web_gate: tokio::sync::watch::Sender<bool>,
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
    #[cfg(test)]
    post_link_barrier: Mutex<Option<Arc<std::sync::Barrier>>>,
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
    /// 같은 thread의 보고·resolve만 직렬화한다. 다른 머신의 전송은 서로 기다리지 않는다.
    collect_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// 확인창 질문이 열리거나 바뀌었다
    pub question_events: broadcast::Sender<aaa_proto::QuestionView>,
    /// 앱 안 알림(턴 끝남·새 보고, notice.rs). 기록하지 않는다
    pub notice_events: broadcast::Sender<aaa_proto::NoticeView>,
    /// 프로젝트 TASKS.md가 바뀌었다(프로젝트, 내용 sha256). ticker가 낸다(coord.rs `tasks_pass`)
    pub task_events: broadcast::Sender<(String, String)>,
    /// ticker가 마지막으로 본 프로젝트마다의 TASKS.md 상태
    tasks_seen: Mutex<HashMap<String, coord::TasksSeen>>,
    queue_gate: tokio::sync::Mutex<()>,
    /// 파일마다의 쓰기 lock(TASKS.md 편집과 ticker의 외부 변경 감지, file_rev.rs)
    file_locks: file_rev::Locks,
    /// 머신마다 마지막으로 알린 harness 모델 목록(설정 「모델」, t-1509). 끊긴 머신의 모델도 켜고 끌 수 있게 둔다
    model_catalogs: Mutex<std::collections::BTreeMap<String, Vec<aaa_proto::CatalogModel>>>,
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
    /// passkey 등록·승인의 begin과 finish 사이(관리 GUI W7). host를 다시 띄우면 처음부터 한다
    passkey_ops: passkey::Ops,
    /// pane별 harness 사건 대기자
    waiters: Mutex<harness::Waiters>,
    /// 메시지 전달은 한 번에 하나씩 한다
    deliver_lock: tokio::sync::Mutex<()>,
    /// 알림 갱신과 완료 파일 이동을 순서대로 투영한다.
    inbox_projection: Mutex<()>,
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
    /// 알림 검사를 깨운 사건 번호와 검사를 마친 번호(ticker tick의 명시적 확인 경계).
    nudge_requested: tokio_watch::Sender<u64>,
    nudge_checked: tokio_watch::Sender<u64>,
    /// hook과 pane이 같은 알림을 전달하지 않도록 마지막 선택과 전송을 직렬화한다.
    nudge_delivery: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// 프로젝트별로 coordinator에게 이미 알린 처음 보는 inbox 항목 ID(hook·nudge). hook은 여기 없는 항목만 알린다(t-1627)
    nudge_told: Mutex<HashMap<String, HashSet<String>>>,
    nudge_wait_log: Mutex<HashMap<String, inbox::NudgeWait>>,
    answer_receipts: Mutex<web::AnswerReceipts>,
    /// task.md Follow-ups 덧붙이기는 한 번에 하나씩
    follow_up_lock: Mutex<()>,
    /// thread 작업 요약 표지 재료(summary.rs): 목록이 매번 파일을 읽지 않게
    summary_marks: summary::MarkMap,
    /// thread의 사용량 한도가 풀리는 때(usage_limit.rs): ticker가 채우고 thread 보기가 읽는다
    usage_limits: usage_limit::LimitMap,
    agy_quota: agy_quota::AgyQuota,
    /// 지금 도는 routine 명령(`<project>/<name>`)
    automation_sources: tokio::sync::Mutex<()>,
    automation_dispatch: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    watch: Mutex<watch::Memory>,
    /// 결과물 보기 경로(`/v/<cap>/`) → 대상 thread
    view_caps: Mutex<HashMap<String, library::ViewCap>>,
    library_signing: std::sync::RwLock<library_links::SigningKey>,
    library_ops: std::sync::Mutex<()>,
    library_reaper: std::sync::OnceLock<library_delete::Reaper>,
    /// 파일 보기 읽기 경로(`/f/<cap>/`, plan §8.125)
    file_caps: Mutex<HashMap<String, files::FileCap>>,
    /// 기획: `<프로젝트>/<저장소>`마다 마지막으로 읽은 트리(머신이 꺼졌을 때 보인다, t-1894)
    design_trees: Mutex<HashMap<String, aaa_proto::DesignTree>>,
    /// Web Push 서명 키(설정이 켜져 있을 때)
    vapid: Option<push::Vapid>,
    /// 구독 사용량(웹이 물을 때만 읽는다)
    usage: usage::UsageService,
    account_quota: tokio::sync::Mutex<quota::Cache>,
    /// 알림 webhook 대기열(설정이 켜져 있을 때)
    notify_tx: Mutex<Option<tokio::sync::mpsc::UnboundedSender<aaa_proto::InboxItem>>>,
    /// 지금 쓰는(보이고 focus가 있는) 웹 화면 수(events WS `presence`). 하나라도 있으면 Web Push를 보내지 않는다
    pub(crate) in_use: std::sync::atomic::AtomicUsize,
    /// 지금 pane을 보는 웹 화면(「끝남(안 봄)」, seen.rs)
    viewing: Mutex<seen::Viewing>,
    /// 기기마다 최근에 push로 보낸 항목(push.rs `PushedLog`)
    pub(crate) pushed: Mutex<push::PushedLog>,
    /// thread 기록 다시 읽어 고치기(`update_rec`)는 한 번에 하나씩
    rec_lock: Mutex<()>,
    /// brief 배경 task가 도는 thread(`<project>/<thread>`, 하나만 돈다)
    brief_loops: Mutex<HashSet<String>>,
    thread_ops: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    #[cfg(test)]
    start_admitted_hook: Mutex<Option<orch::StartAdmittedHook>>,
    #[cfg(test)]
    inbox_finish_barrier: Mutex<Option<Arc<std::sync::Barrier>>>,
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

fn device_view(d: db::DeviceRow, passkey: bool) -> aaa_proto::DeviceView {
    aaa_proto::DeviceView {
        role: if d.role == "reader" { DeviceRole::Reader } else { DeviceRole::Controller },
        passkey,
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

/// 운영 로그의 대상은 짧은 ID 또는 그 외 입력의 고정 길이 지문이다.
fn operation_log_id(id: &str) -> String {
    if id.split_once('-').is_some_and(|(kind, number)| {
        matches!(kind, "t" | "q" | "d" | "p" | "i" | "r")
            && (1..=10).contains(&number.len())
            && number.bytes().all(|b| b.is_ascii_digit())
    }) {
        id.into()
    } else {
        hex::encode(&sha2::Sha256::digest(id.as_bytes())[..8])
    }
}

fn err(code: ErrorCode, msg: impl Into<String>) -> ErrorBody {
    ErrorBody::new(code, msg)
}

/// CLI도 시작·전송 전에 host와 같은 요약 형식을 검사한다. 상태를 읽거나 쓰지 않는다.
pub fn validate_thread_summary(text: &str) -> std::result::Result<(), ErrorBody> {
    // 저장할 글의 크기를 재면서 오류 줄 번호는 원본 기준으로 표시한다.
    let body = text.trim_start();
    let skipped_lines = text[..text.len() - body.len()].bytes().filter(|&b| b == b'\n').count();
    summary::parse_at(body.trim_end(), skipped_lines + 1)
        .map(|_| ())
        .map_err(|e| err(ErrorCode::BadRequest, format!("요약 형식: {e}\n형식:\n{}", summary::FORMAT)))
}

impl Host {
    pub fn open(cfg: HostConfig, state: PathBuf) -> Result<Arc<Host>> {
        util::ensure_private_dir(&state)?;
        let db = db::Db::open(&state.join("host.db")).context("host DB")?;
        account_rename::recover(&db, &state)?;
        let accounts = accounts::Accounts::new(&state).map_err(|e| anyhow::anyhow!(e.message))?;
        machine_rename::recover(&db)?;
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
                    health: db
                        .meta(&format!("machine_health:{}", m.id))?
                        .and_then(|v| serde_json::from_str(&v).ok())
                        .map(|h| (h, 0)),
                    health_sent: None,
                    features: vec![],
                    identity: None,
                    agent_pid: None,
                    colors: None,
                    build: MachineBuild { arch: m.arch, software: m.software, sha256: m.sha256, selfcheck: None },
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
        let web_port = cfg
            .web
            .listen
            .rsplit_once(':')
            .and_then(|(_, p)| {
                p.parse()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .unwrap_or(0);
        let dev_mode = cfg.local_agent.mode == LocalAgentMode::Child;
        let upload_gate = library_upload::upload_gate(&state).context("업로드 쓰기 잠금 경로")?;
        let host = Arc::new(Host {
            backup_running: AtomicBool::new(false),
            file_backup_running: AtomicBool::new(false),
            health: Mutex::new(supervisor::Health::load(&db)),
            health_wake: Default::default(),
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
            workspace_metrics_changed: tokio_watch::channel(0).0,
            asks_changed: tokio_watch::channel(0).0,
            inbox_changed: tokio_watch::channel(0).0,
            accounts_changed: tokio_watch::channel(0).0,
            enroll_changed: tokio_watch::channel(0).0,
            setup_gate: Mutex::new(()),
            machine_rename_gate: Mutex::new(()),
            machine_label_gate: tokio::sync::Mutex::new(()),
            metrics_gate: tokio::sync::Mutex::new(()),
            metrics_pending: Mutex::new(HashSet::new()),
            account_meta_gate: Mutex::new(()),
            account_rename_gate: Default::default(),
            account_rename_recovery: Mutex::new(()),
            device_gate: Mutex::new(()),
            publish_gate: tokio::sync::Mutex::new(()),
            upload_gate,
            enroll: Mutex::new(Enroll::default()),
            pairing: Mutex::new(Pairing::default()),
            install_probes: Mutex::new(HashMap::new()),
            agent_listen: Mutex::new(None),
            web_listen: Mutex::new(None),
            web_gate: tokio::sync::watch::channel(true).0,
            workspaces: Mutex::new(workspaces),
            layout_events: broadcast::channel(64).0,
            front_seq: AtomicU64::new(0),
            boot_id: util::boot_id(),
            config_dir: std::sync::OnceLock::new(),
            flow_events: broadcast::channel(256).0,
            flow_order: Mutex::new(()),
            thread_events: broadcast::channel(256).0,
            collect_locks: Mutex::new(HashMap::new()),
            question_events: broadcast::channel(256).0,
            post_gate: std::sync::Mutex::new(()),
            #[cfg(test)]
            post_link_barrier: Mutex::new(None),
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
            queue_gate: tokio::sync::Mutex::new(()),
            file_locks: Default::default(),
            model_catalogs: Default::default(),
            turn_ends: Mutex::new(HashMap::new()),
            question_lock: std::sync::Mutex::new(()),
            preferences,
            device_revoked: broadcast::channel(64).0,
            revoked_devices: Mutex::new(Default::default()),
            passkey_ops: Default::default(),
            waiters: Mutex::new(HashMap::new()),
            deliver_lock: tokio::sync::Mutex::new(()),
            inbox_projection: Mutex::new(()),
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
            nudge_requested: tokio_watch::channel(0).0,
            nudge_checked: tokio_watch::channel(0).0,
            nudge_delivery: Mutex::new(HashMap::new()),
            nudge_told: Mutex::new(HashMap::new()),
            nudge_wait_log: Mutex::new(HashMap::new()),
            answer_receipts: Mutex::new(web::AnswerReceipts::default()),
            follow_up_lock: Mutex::new(()),
            summary_marks: Mutex::new(HashMap::new()),
            usage_limits: Mutex::new(HashMap::new()),
            agy_quota: agy_quota::AgyQuota::default(),
            automation_sources: tokio::sync::Mutex::new(()),
            automation_dispatch: Mutex::new(HashMap::new()),
            watch: Mutex::new(watch::Memory::default()),
            view_caps: Mutex::new(HashMap::new()),
            library_signing: std::sync::RwLock::new(library_signing),
            library_ops: std::sync::Mutex::new(()),
            library_reaper: std::sync::OnceLock::new(),
            file_caps: Mutex::new(HashMap::new()),
            design_trees: Mutex::new(HashMap::new()),
            vapid,
            usage: usage::UsageService::new(usage::UsageContext::system()),
            account_quota: Default::default(),
            notify_tx: Mutex::new(None),
            in_use: std::sync::atomic::AtomicUsize::new(0),
            viewing: Mutex::new(seen::Viewing::default()),
            pushed: Mutex::new(push::PushedLog::default()),
            rec_lock: Mutex::new(()),
            brief_loops: Mutex::new(HashSet::new()),
            thread_ops: Mutex::new(HashMap::new()),
            #[cfg(test)]
            start_admitted_hook: Mutex::new(None),
            #[cfg(test)]
            inbox_finish_barrier: Mutex::new(None),
            brief_claims: Mutex::new(HashSet::new()),
            resource_exhausted: std::sync::atomic::AtomicBool::new(false),
            report_refresh: Mutex::new(HashMap::new()),
            restoring: Mutex::new(HashSet::new()),
            restore_tried: Mutex::new(HashSet::new()),
        });
        // 이 기록 전에 등록한 Codex 계정의 메일·plan(관리 GUI W5a, 사용자 답 3A)
        host.migrate_workspace_access()?;
        host.pool_migrate().map_err(|e| anyhow::anyhow!("계정 자동 배정 이전: {}", e.message))?;
        host.account_meta_backfill();
        host.backfill_report_stages()?;
        // 진행 중에 내려간 SSH 머신 추가 작업(plan §8.384)
        host.setup_recover();
        host.image_recover()?;
        host.review_recover()?;
        host.library_delete_recover();
        Ok(host)
    }

    fn bump(&self) {
        self.changed.send_modify(|v| *v += 1);
    }

    /// 보고서 목록이 바뀌었다(plan §8.256)
    pub(crate) fn reports_bump(&self) {
        self.reports_changed.send_modify(|v| *v += 1);
    }

    pub(crate) fn enroll_bump(&self) {
        self.enroll_changed.send_modify(|v| *v += 1);
    }

    pub(crate) fn accounts_bump(&self) {
        self.accounts_changed.send_modify(|v| *v += 1);
    }

    pub(crate) fn inbox_bump(&self) {
        self.inbox_changed.send_modify(|v| *v += 1);
    }

    // ───────────── 웹 공개 ─────────────

    /// 웹 요청의 Host 헤더가 이 host를 가리키는가(DNS rebinding 방어)
    pub fn web_port(&self) -> u16 {
        self.web_port.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Host 헤더가 가리키는 이 host의 origin. `http://127.0.0.1:<web_port>`와 tailscale serve 공개 주소
    /// (`https://<이름>[:포트]`, 443은 포트 없이) 둘뿐이고 포트까지 같아야 한다(§6.3, herdr-web-ui `sameOrigin`이
    /// 요청 주소의 origin 전체를 비교하는 것과 같다). 아니면 None
    pub fn web_origin(&self, host_header: &str) -> Option<String> {
        web_origin_in(self.web_port(), &self.db, host_header)
    }

    pub(crate) fn public_origin(&self) -> Option<String> {
        public_origin_in(&self.db)
    }

    /// 공개 주소를 tailscale에서 다시 읽는다(host 시작 때: 지난 실행 뒤 serve 포트가 바뀌었을 수 있다)
    pub async fn refresh_publish(&self) {
        self.publish_status().await;
    }

    /// host와 같은 머신의 agent인가(brief의 uploads 주석)
    pub(crate) fn machine_is_local(&self, machine_id: &str) -> bool {
        self.inner.lock().unwrap().machines.get(machine_id).is_some_and(|m| m.local)
    }

    pub fn local_machine_connected(&self) -> bool {
        self.inner.lock().unwrap().machines.values().any(|m| m.local && m.conn.is_some())
    }
}

/// Host 헤더가 가리키는 이 host의 origin(`Host::web_origin`). 안전 모드도 같은 판정을 쓴다(설정 없이 DB만 연다)
pub(crate) fn web_origin_in(web_port: u16, db: &db::Db, host_header: &str) -> Option<String> {
    let loopback = format!("127.0.0.1:{web_port}");
    if host_header == loopback {
        return Some(format!("http://{loopback}"));
    }
    if test_localhost_public() && host_header == format!("localhost:{web_port}") {
        return Some(format!("http://{host_header}"));
    }
    let public = public_origin_in(db)?;
    let asked = host_header.to_ascii_lowercase();
    let asked = asked.strip_suffix(":443").unwrap_or(&asked);
    (format!("https://{asked}") == public).then_some(public)
}

/// 시험에서만(debug 빌드 `AAA_TEST_WEB_LOCALHOST`): `http://localhost:<port>`를 공개 주소처럼 받는다. 브라우저 passkey는
/// IP 주소에 묶이지 못해 실험실(127.0.0.1)에서는 RP ID `localhost`로 시험한다(관리 GUI W7 E2E)
pub(crate) fn test_localhost_public() -> bool {
    cfg!(debug_assertions) && std::env::var_os("AAA_TEST_WEB_LOCALHOST").is_some()
}

/// 공개 origin. 아직 serve 상태를 한 번도 읽지 못한 옛 DB만 publish_port·이름으로 대신한다.
/// 매핑 없음도 관측한 상태이므로 publish_observed 뒤에는 옛 포트를 되살리지 않는다.
pub(crate) fn public_origin_in(db: &db::Db) -> Option<String> {
    db.publication_origin()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "공개 주소 조회 실패"))
        .ok()
        .flatten()
}

/// `aaa host` 실행: 설정 폴더를 읽어 host를 띄운다. 읽지 못하면 끝내지 않고 안전 모드(plan §8.372)로 웹만 열며, 되돌리거나
/// 다시 읽어 설정이 맞으면 정상 host로 이어 간다. OS 감독자에는 실제 listener 준비 뒤에만(안전 모드 웹이든 정상 host든 한 번) 알린다.
/// 서비스 등록 자체는 ready가 아니다.
pub async fn run(
    config_dir: PathBuf,
    state_root: PathBuf,
    mut ready: Option<tokio::sync::oneshot::Sender<()>>,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<()> {
    tokio::pin!(shutdown);
    let mut web_listener = None;
    loop {
        match aaa_config::load_host(&config_dir) {
            Ok(loaded) => {
                for w in &loaded.warnings {
                    eprintln!("{w}");
                }
                return run_host(
                    loaded.config,
                    config_dir,
                    &loaded.files,
                    state_root,
                    web_listener,
                    ready.take(),
                    shutdown,
                )
                .await;
            }
            Err(errors) => {
                match safe_mode::run(
                    errors,
                    config_dir.clone(),
                    state_root.clone(),
                    web_listener.take(),
                    ready.take(),
                    shutdown.as_mut(),
                )
                .await?
                {
                    safe_mode::SafeExit::Reload(listener) => {
                        web_listener = Some(listener);
                        #[cfg(all(unix, debug_assertions))]
                        safe_mode::test_reload_hold().await?;
                        continue;
                    }
                    safe_mode::SafeExit::Shutdown => return Ok(()),
                }
            }
        }
    }
}

async fn run_host(
    cfg: HostConfig,
    config_dir: PathBuf,
    files: &[PathBuf],
    state_root: PathBuf,
    web_listener: Option<tokio::net::TcpListener>,
    ready: Option<tokio::sync::oneshot::Sender<()>>,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<()> {
    let host = Host::open(cfg, state_root.join("host"))?;
    aaa_common::diagnostic::ignored(host.config_dir.set(config_dir.clone()), "보조 작업 실패 뒤 기존 처리를 계속한다");
    tracing::info!(host = %host.host_id, fp = %&host.identity.fingerprint[..16], "host 시작");
    // 한 host는 프로젝트 하나(plan §8.283): 입구를 열기 전에 본다. 보관 상태가 DB에 있어 open 뒤에야 셀 수 있다
    match host.sole_project() {
        Ok(project) => tracing::info!(project = project.as_deref().unwrap_or("(아직 없음)"), "이 host의 프로젝트"),
        Err(names) => anyhow::bail!(lifecycle::sole_project_refusal(&names)),
    }
    // 옛 판이 남긴 보내던 메시지는 입구를 열기 전에 잊는다(연 뒤면 막 기록한 새 메시지까지 「모른다」로 바뀐다)
    host.backfill_report_ids().map_err(|e| anyhow::anyhow!(e.message))?;
    host.backfill_completed_reports().map_err(|e| anyhow::anyhow!(e.message))?;
    host.backfill_review_questions().map_err(|e| anyhow::anyhow!(e.message))?;
    host.forget_queued_messages();
    host.start_supervisor();
    host.web_gate.send_replace(false);
    link::start(&host).await?;
    front::start(&host).await?;
    if let Some(listener) = web_listener {
        web::resume(&host, listener).await?;
    } else {
        web::start(&host).await?;
    }
    host.web_gate.send_replace(true);
    // 입구가 다 열렸다: 이 설정을 안전 모드의 「마지막 정상 설정」으로 보관한다
    if let Err(e) = safe_mode::keep_last_good(&config_dir, files) {
        tracing::warn!("마지막 정상 설정을 보관하지 못했다: {e:#}");
    }
    host.resume_rollout();
    host.start_ticker();
    host.start_notifier();
    if let Some(ready) = ready {
        aaa_common::diagnostic::ignored(ready.send(()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
    }
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
    shutdown.await;
    host.cancel_account_logins().await;
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
    let mut child = cmd.spawn_guarded()?;
    tracing::info!(pid = child.id(), "개발 모드: 로컬 agent를 띄웠다");
    std::thread::spawn(move || {
        aaa_common::diagnostic::ignored(child.wait(), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
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
        aaa_common::diagnostic::ignored(tokio::signal::ctrl_c().await, "보조 작업 실패 뒤 기존 처리를 계속한다");
    }
}

#[cfg(test)]
mod machine_view_tests;

#[cfg(test)]
mod pairing_tests;

#[cfg(test)]
mod exit_tests;
