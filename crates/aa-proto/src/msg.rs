//! 제어 plane 메시지. 모두 JSON이며 `t` 필드로 종류를 나눈다.
//!
//! - Agent link: host ↔ agent backend (`LinkMsg`)
//! - Session IPC: agent backend ↔ session host (`SessionMsg`)
//! - Front API: front(CLI·TUI·웹) ↔ host (`FrontMsg`, 웹은 `FrontRequest`를 HTTP로 투영)
//!
//! 모르는 선택 필드는 무시한다(serde 기본). 필수 필드가 없거나 모르는 메시지 종류는 오류다.

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

use crate::orch::{
    AgentStatus, Cleanup, DecisionView, FlowEvent, HarnessEvent, InboxItem, Launch, MergeEntry, MessageMode,
    MessageView, ProfileView, ProjectView, QuestionView, ReportSnapshot, Role, RoutineView, ThreadKind, ThreadPrepare,
    ThreadReady, ThreadView,
};

use crate::version::Version;

/// JSON에서 base64 문자열로 나가는 bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, TS)]
#[ts(type = "string")]
pub struct B64(pub Vec<u8>);

impl Serialize for B64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for B64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD.decode(s.as_bytes()).map(B64).map_err(serde::de::Error::custom)
    }
}

/// 안정된 오류 코드. 문자열 값은 CLI JSON 출력과 웹에서 그대로 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    Unauthorized,
    Forbidden,
    NotFound,
    AgentDisconnected,
    SessionExited,
    UnsupportedVersion,
    NotEnrolled,
    EnrollClosed,
    EnrollCodeMismatch,
    NameTaken,
    Timeout,
    Unavailable,
    Internal,
    /// 다른 front가 이 pane의 조작 lease를 갖고 있다(가져오려면 take)
    LeaseHeld,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

impl ErrorBody {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        ErrorBody { code, message: message.into() }
    }
}

impl std::fmt::Display for ErrorBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let code = serde_json::to_value(self.code).ok().and_then(|v| v.as_str().map(String::from));
        write!(f, "{}: {}", code.unwrap_or_default(), self.message)
    }
}

impl std::error::Error for ErrorBody {}

/// 요청 결과. JSON은 `{"ok": ...}` 또는 `{"err": {...}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Outcome<T> {
    Ok(T),
    Err(ErrorBody),
}

impl<T> From<Result<T, ErrorBody>> for Outcome<T> {
    fn from(r: Result<T, ErrorBody>) -> Self {
        match r {
            Ok(v) => Outcome::Ok(v),
            Err(e) => Outcome::Err(e),
        }
    }
}

impl<T> Outcome<T> {
    pub fn into_result(self) -> Result<T, ErrorBody> {
        match self {
            Outcome::Ok(v) => Ok(v),
            Outcome::Err(e) => Err(e),
        }
    }
}

// ───────────────────────── 세션 ─────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionState {
    Running,
    Exited {
        code: Option<i32>,
    },
    /// 기록된 boot ID와 지금 boot ID가 다르다(재부팅). 진행 중이던 일은 성공이 아니다.
    Interrupted,
    /// session host가 종료 기록 없이 사라졌다.
    Lost,
}

impl SessionState {
    pub fn is_running(&self) -> bool {
        matches!(self, SessionState::Running)
    }
}

/// 세션(=pane 하나) 실행 사실. 정본은 agent(session host)다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub pid: Option<u32>,
    #[ts(type = "number")]
    pub started_at_ms: u64,
    pub boot_id: String,
    pub cols: u16,
    pub rows: u16,
    #[serde(flatten)]
    pub state: SessionState,
    /// agent 세션 역할(thread가 띄운 세션만). 이전 session host는 보내지 않는다.
    #[serde(default)]
    pub role: Option<Role>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SpawnSpec {
    pub name: String,
    /// 비어 있으면 그 머신 사용자의 기본 셸
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    #[serde(default)]
    pub role: Option<Role>,
}

/// 재접속용 화면 snapshot. `replay`를 빈 터미널에 쓰면 지금 화면이 된다.
/// 이어지는 출력은 `offset`부터다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Snapshot {
    pub session: String,
    #[ts(type = "number")]
    pub offset: u64,
    pub cols: u16,
    pub rows: u16,
    pub replay: B64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ScreenText {
    pub session: String,
    pub cols: u16,
    pub rows: u16,
    /// 보이는 화면의 글자(줄 끝 공백 제거)
    pub lines: Vec<String>,
    pub cursor_row: u16,
    pub cursor_col: u16,
}

// ───────────────────────── Agent link ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentHello {
    pub proto: Version,
    pub machine_id: String,
    pub machine_name: String,
    pub boot_id: String,
    pub os: String,
    pub arch: String,
    pub software: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub dev_mode: bool,
    /// 시작 때 자가 점검 결과(doctor probe 요약). 해석은 host가 표시만 한다.
    #[serde(default)]
    pub selfcheck: Option<serde_json::Value>,
    #[serde(default)]
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostWelcome {
    pub proto: Version,
    pub host_id: String,
    #[serde(default)]
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub proto: Version,
    pub machine_id: String,
    pub machine_name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum LinkMsg {
    Hello(AgentHello),
    Welcome(HostWelcome),
    Enroll(EnrollRequest),
    /// host가 등록 요청을 받아 사용자의 확인을 기다린다.
    EnrollPending,
    EnrollAccepted {
        machine_id: String,
    },
    Reject(ErrorBody),
    Req {
        id: String,
        #[serde(default)]
        deadline_ms: Option<u64>,
        body: AgentRequest,
    },
    Res {
        id: String,
        result: Outcome<AgentResponse>,
    },
    Event(AgentEvent),
    Ping {
        seq: u64,
    },
    Pong {
        seq: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AgentRequest {
    Spawn(SpawnSpec),
    List,
    /// 출력 구독. `from`이 보존 범위 안이면 그 위치부터 bytes만, 아니면 snapshot을 먼저 보낸다.
    Subscribe {
        session: String,
        from: Option<u64>,
    },
    Unsubscribe {
        session: String,
    },
    Input {
        session: String,
        data: B64,
    },
    Resize {
        session: String,
        cols: u16,
        rows: u16,
    },
    Kill {
        session: String,
    },
    Screen {
        session: String,
    },
    /// 끝난(종료·중단·유실) 세션의 기록을 지운다. 실행 중이면 거부한다.
    Forget {
        session: String,
    },
    /// agent.toml에 정의된 profile 목록
    Profiles,
    /// thread 자리(worktree 또는 작업 폴더)를 만들고 brief.md를 쓰고 profile을 해석한다
    ThreadPrepare(ThreadPrepare),
    /// report.md 변화를 `ReportChanged`로 알린다. 등록 즉시 지금 hash를 한 번 보낸다. `thread`는 `<project>/<thread>`다.
    ThreadWatch {
        thread: String,
        work_dir: String,
    },
    ThreadUnwatch {
        thread: String,
    },
    /// report.md 본문과 library 목록. symlink는 따라가지 않고, 상한을 넘는 파일은 건너뛴다.
    ReportCollect {
        work_dir: String,
        max_library_bytes: u64,
    },
    /// work_dir 아래 파일 한 조각(library 사본용)
    FileRead {
        work_dir: String,
        path: String,
        offset: u64,
        len: u32,
    },
    /// thread를 다시 시작할 준비: 있는 작업 폴더에 brief.md를 다시 쓰고, 새 대화로 띄울 launch를 돌려준다
    /// (claude는 새 `--session-id`). 저장된 launch를 그대로 쓰고 profile을 다시 해석하지 않는다.
    ThreadRebrief {
        work_dir: String,
        brief: String,
        launch: Launch,
    },
    /// routine 명령을 `sh -c`로 한 번 실행한다(자기 process group, 시간 초과면 그 group을 끝낸다).
    /// `cwd`가 없으면 `<work_root>/<project>/routines`에서 실행한다.
    RunCommand {
        project: String,
        cwd: Option<String>,
        command: String,
        timeout_ms: u64,
    },
    /// thread 정리: worktree를 force 없이 지우고, 기준에 병합된 branch만 지운다
    ThreadCleanup {
        repo: Option<String>,
        cwd: String,
        branch: Option<String>,
        base: Option<String>,
        keep_worktree: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentResponse {
    Session(SessionInfo),
    Sessions {
        sessions: Vec<SessionInfo>,
    },
    /// 구독 시작. snapshot이 없으면 `from`부터 이어진다.
    Subscribed {
        session: String,
        snapshot: Option<Snapshot>,
        offset: u64,
    },
    Screen(ScreenText),
    Done,
    Profiles {
        profiles: Vec<ProfileView>,
    },
    ThreadReady(ThreadReady),
    Report(ReportSnapshot),
    FileChunk {
        data: B64,
        eof: bool,
    },
    Cleanup(Cleanup),
    Relaunch(Launch),
    CommandRan(CommandRan),
}

/// routine 명령 실행 결과
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRan {
    /// 종료 코드(신호로 끝났거나 시간 초과면 None)
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// stdout 뒤에 stderr. 상한에서 자른다.
    pub output: String,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum AgentEvent {
    /// harness hook·notify·`aa report`에서 온 상태 사건(adapter가 정규화)
    Harness(HarnessEvent),
    /// 감시 중인 thread의 report.md hash(없으면 None)가 바뀌었다
    ReportChanged {
        thread: String,
        hash: Option<String>,
    },
    SessionStarted(SessionInfo),
    SessionUpdated(SessionInfo),
    /// 이 세션의 출력 구독이 끊겼다(느린 연결). host가 마지막 offset부터 다시 구독한다.
    Resync {
        session: String,
    },
}

// ───────────────────────── Session IPC ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum SessionMsg {
    Hello {
        proto: Version,
        token: String,
    },
    Welcome {
        proto: Version,
        info: SessionInfo,
    },
    Reject(ErrorBody),
    Req {
        id: String,
        body: SessionRequest,
    },
    Res {
        id: String,
        result: Outcome<AgentResponse>,
    },
    /// 세션 상태 변화(종료·크기)
    Updated(SessionInfo),
    /// 구독자가 너무 느려 구독을 끊었다. 마지막으로 받은 offset부터 다시 구독한다.
    Resync,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum SessionRequest {
    Subscribe { from: Option<u64> },
    Unsubscribe,
    Input { data: B64 },
    Resize { cols: u16, rows: u16 },
    Kill,
    Screen,
}

// ───────────────────────── Front API ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeviceRole {
    /// 보기만 한다. 입력·resize·실행·업로드 불가
    Reader,
    /// 조작한다
    Controller,
    /// 같은 머신 사용자(UDS peer UID 확인)
    Local,
}

impl DeviceRole {
    pub fn can_control(self) -> bool {
        matches!(self, DeviceRole::Controller | DeviceRole::Local)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Connection {
    Connected,
    /// agent와 끊겼다. 세션은 계속 돌 수 있으나 aa에서는 읽기 전용이다.
    Disconnected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineView {
    pub id: String,
    pub name: String,
    pub local: bool,
    pub connection: Connection,
    pub os: Option<String>,
    pub boot_id: Option<String>,
    pub dev_mode: bool,
    #[ts(type = "number | null")]
    pub last_seen_ms: Option<u64>,
}

/// pane 조작 lease(대화형 front 하나가 키 입력과 PTY 크기를 정한다). 연결이 끊기면 풀린다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LeaseView {
    /// 보유한 front 연결 ID(front는 Welcome·attach 때 받은 자기 ID와 비교한다)
    pub front: String,
    /// 사람이 읽는 이름(예: `tui@mac-a`, `웹(ab12)`)
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PaneView {
    pub id: String,
    pub machine_id: String,
    pub machine: String,
    pub name: String,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    #[serde(flatten)]
    pub state: SessionState,
    pub connection: Connection,
    /// 지금 입력할 수 없으면 true(끊김·종료·권한). lease는 따로 본다.
    pub readonly: bool,
    pub cols: u16,
    pub rows: u16,
    pub lease: Option<LeaseView>,
    pub role: Option<Role>,
    /// harness 상태(hook을 보내는 agent 세션만)
    pub agent: Option<Box<AgentStatus>>,
}

// ───────────────────────── layout(host 정본, §4.4) ─────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SplitDir {
    /// 좌우로 나눈다(a가 왼쪽)
    Horizontal,
    /// 위아래로 나눈다(a가 위)
    Vertical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum LayoutNode {
    Pane {
        pane: String,
    },
    Split {
        dir: SplitDir,
        /// a 쪽 비율(천분율, 1..=999)
        ratio: u16,
        a: Box<LayoutNode>,
        b: Box<LayoutNode>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Tab {
    pub id: String,
    pub name: String,
    pub root: LayoutNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Workspace {
    pub name: String,
    pub tabs: Vec<Tab>,
}

/// layout 바꾸기. 새 pane이 필요한 동작은 host가 띄운 뒤 layout에 넣는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum LayoutChange {
    /// 새 탭에 새 pane
    NewTab { machine: Option<String>, name: Option<String>, argv: Vec<String>, cwd: Option<String> },
    /// pane 자리를 나눠 새 pane을 b 쪽에 둔다
    Split { pane: String, dir: SplitDir, machine: Option<String>, argv: Vec<String>, cwd: Option<String> },
    /// 이미 있는 pane을 새 탭으로 넣는다
    AddTab { pane: String },
    /// layout에서 뺀다(kill이면 세션도 끝낸다)
    Close { pane: String, kill: bool },
}

// ───────────────────────── TUI 화면(cell grid, §4.5) ─────────────────────────

/// 같은 모양 글자 묶음. 색은 0=기본, 0x0100_00nn=팔레트 nn, 0x0200_rrggbb=RGB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GridRun {
    /// 글자들. 넓은 글자(한글 등)는 두 칸을 차지하고 뒤 칸은 따로 적지 않는다.
    pub t: String,
    pub fg: u32,
    pub bg: u32,
    /// 1=굵게, 2=기울임, 4=밑줄, 8=반전
    pub a: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GridLine {
    pub row: u16,
    pub runs: Vec<GridRun>,
}

/// pane 화면 한 장면. `full`이면 모든 줄, 아니면 바뀐 줄만 들어 있다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct GridFrame {
    pub pane: String,
    pub cols: u16,
    pub rows: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
    pub full: bool,
    pub lines: Vec<GridLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PendingEnroll {
    pub name: String,
    pub machine_id: String,
    /// 상대 인증서 fingerprint 앞부분(참고용). 확인 코드는 agent 화면에만 보인다.
    pub fingerprint_prefix: String,
    #[ts(type = "number")]
    pub since_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PublishState {
    /// missing | stopped | running | error
    pub tailscale: String,
    pub dns_name: Option<String>,
    /// aa 웹을 이미 proxy하는 HTTPS 주소
    pub url: Option<String>,
    /// aa가 만든 serve 포트(끌 때 이 포트만 끈다)
    pub owned_port: Option<u16>,
    pub web_port: u16,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HostStatus {
    pub host_id: String,
    pub software: String,
    pub proto: Version,
    pub dev_mode: bool,
    pub agent_listen: Option<String>,
    pub web_listen: Option<String>,
    pub machines: Vec<MachineView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FrontRequest {
    Status,
    Machines,
    /// pane 목록. 기본은 평가자 세션을 빼고, `role`을 주면 그 역할만 보인다(§8.13).
    Panes {
        #[serde(default)]
        role: Option<Role>,
    },
    PaneRun {
        machine: Option<String>,
        name: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
        cols: Option<u16>,
        rows: Option<u16>,
    },
    PaneRead {
        pane: String,
    },
    PaneSend {
        pane: String,
        data: B64,
    },
    PaneResize {
        pane: String,
        cols: u16,
        rows: u16,
    },
    PaneKill {
        pane: String,
    },
    /// 대화형 입력(TUI·웹 터미널). 조작 lease를 확인하고, 비어 있으면 이 연결이 갖는다.
    /// 한 번 보내는 `PaneSend`(CLI·API)는 lease 대상이 아니다(§4.5).
    PaneInput {
        pane: String,
        data: B64,
    },
    /// 조작 lease를 이 연결로 가져온다(이전 보유자에게는 State로 알린다)
    PaneTake {
        pane: String,
    },
    PaneRelease {
        pane: String,
    },
    /// 끝난 pane(중단·유실·종료)을 같은 spec으로 다시 띄우고 layout 자리를 바꾼다
    PaneResume {
        pane: String,
    },
    /// 끝난 pane 기록을 지운다
    PaneForget {
        pane: String,
    },
    /// 이 연결로 pane 화면(cell grid)을 보낸다. 같은 머신 front 연결(UDS)에서만 쓴다.
    PaneWatch {
        pane: String,
    },
    PaneUnwatch {
        pane: String,
    },
    Workspace {
        name: Option<String>,
    },
    LayoutApply {
        workspace: Option<String>,
        change: LayoutChange,
    },
    EnrollOpen {
        minutes: u32,
    },
    EnrollPending,
    EnrollApprove {
        name: String,
        code: String,
    },
    MachineRevoke {
        name: String,
    },
    WebPair {
        role: DeviceRole,
    },
    WebPublish,
    WebPublishOff,
    WebStatus,
    ProjectCreate {
        name: String,
    },
    Projects,
    /// 머신의 agent.toml profile 목록
    Profiles {
        machine: Option<String>,
    },
    ThreadStart {
        project: String,
        title: String,
        machine: Option<String>,
        profile: String,
        #[serde(default)]
        kind: ThreadKind,
        repo: Option<String>,
        base: Option<String>,
        /// 할 일(threads/<id>.task.md로 저장하고 brief에 넣는다)
        task: String,
    },
    Threads {
        project: Option<String>,
    },
    ThreadShow {
        project: String,
        thread: String,
    },
    /// 지금 보고를 확인했다
    ThreadAck {
        project: String,
        thread: String,
    },
    /// 끝낸다: 마지막 사본(실패면 거부) → 세션 종료 → worktree 정리(force 없음)
    ThreadResolve {
        project: String,
        thread: String,
        #[serde(default)]
        keep_worktree: bool,
    },
    /// thread agent를 멈춘다(Esc 한 번). 세션·작업 폴더는 그대로다
    ThreadStop {
        project: String,
        thread: String,
    },
    /// 끝난 thread 세션을 다시 띄운다. 기본은 task를 다시 읽어 새 brief로 새 대화, `resume`이면 저장된 대화를 잇는다
    ThreadRestart {
        project: String,
        thread: String,
        #[serde(default)]
        resume: bool,
    },
    /// 사람이 명시적으로 brief 안내를 다시 보낸다(자동 재전송은 하지 않는다)
    ThreadBrief {
        project: String,
        thread: String,
    },
    /// 열린 확인창 질문(기본은 열린 것만)
    Questions {
        #[serde(default)]
        all: bool,
    },
    /// 질문에 답한다. host가 질문이 아직 열려 있는지 다시 확인한다.
    QuestionAnswer {
        id: String,
        choice: String,
    },
    /// thread agent에 메시지를 보낸다(전달을 모르면 다시 보내지 않는다)
    ThreadMessage {
        project: String,
        thread: String,
        text: String,
        #[serde(default)]
        mode: MessageMode,
    },
    /// 결정을 기록한다(decisions.md에 덧붙이고 흐름 사건을 낸다)
    DecisionAdd {
        project: String,
        text: String,
        #[serde(default)]
        replaces: Option<u32>,
    },
    /// 결정 기록 목록
    Decisions {
        project: String,
    },
    /// inbox. `peek`가 아니면 보인 항목을 「봤다」로 표시한다(처음 보는 항목만 coordinator에게 알린다)
    Inbox {
        project: String,
        #[serde(default)]
        all: bool,
        #[serde(default)]
        peek: bool,
    },
    /// inbox 항목을 끝낸다(`all`이면 끝내지 않은 것 모두)
    InboxDone {
        project: String,
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    /// 병합 대기열 맨 뒤에 넣는다(이미 있으면 그 자리)
    MergeEnqueue {
        project: String,
        thread: String,
    },
    /// 병합 대기열에서 뺀다. `landed`면 병합을 마친 것이고, 아니면 취소다.
    MergeDrop {
        project: String,
        thread: String,
        #[serde(default)]
        landed: bool,
    },
    MergeList {
        project: String,
    },
    /// routine 목록(`projects/<p>/routines/*.md`)과 상태
    Routines {
        project: String,
    },
    /// routine 명령 승인. `command_sha256`은 사람이 본 명령(머신 포함)의 hash이고, 파일이 그 사이 바뀌었으면 거부한다.
    RoutineApprove {
        project: String,
        name: String,
        command_sha256: String,
    },
    /// 흐름 사건을 `after`(cursor) 다음부터 읽는다
    Events {
        project: Option<String>,
        #[serde(default)]
        after: u64,
        #[serde(default)]
        limit: Option<u32>,
    },
}

impl FrontRequest {
    /// 조작(controller) 권한이 필요한 요청인가
    pub fn needs_control(&self) -> bool {
        !matches!(
            self,
            FrontRequest::Status
                | FrontRequest::Machines
                | FrontRequest::Panes { .. }
                | FrontRequest::Projects
                | FrontRequest::Profiles { .. }
                | FrontRequest::Threads { .. }
                | FrontRequest::ThreadShow { .. }
                | FrontRequest::Events { .. }
                | FrontRequest::Questions { .. }
                | FrontRequest::Decisions { .. }
                | FrontRequest::Inbox { peek: true, .. }
                | FrontRequest::MergeList { .. }
                | FrontRequest::Routines { .. }
                | FrontRequest::PaneRead { .. }
                | FrontRequest::PaneWatch { .. }
                | FrontRequest::PaneUnwatch { .. }
                | FrontRequest::Workspace { .. }
                | FrontRequest::WebStatus
        )
    }

    /// 같은 머신(Local)에서만 허용하는 관리 요청인가
    pub fn local_only(&self) -> bool {
        matches!(
            self,
            FrontRequest::EnrollOpen { .. }
                | FrontRequest::EnrollPending
                | FrontRequest::EnrollApprove { .. }
                | FrontRequest::MachineRevoke { .. }
                | FrontRequest::RoutineApprove { .. }
                | FrontRequest::WebPair { .. }
                | FrontRequest::WebPublish
                | FrontRequest::WebPublishOff
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrontResponse {
    Status(HostStatus),
    Machines {
        machines: Vec<MachineView>,
    },
    Panes {
        panes: Vec<PaneView>,
    },
    Pane(PaneView),
    Screen(ScreenText),
    Pending {
        pending: Vec<PendingEnroll>,
    },
    PairCode {
        code: String,
        role: DeviceRole,
        #[ts(type = "number")]
        expires_in_s: u64,
    },
    Publish(PublishState),
    Workspace(Workspace),
    Done,
    Projects {
        projects: Vec<ProjectView>,
    },
    Project(ProjectView),
    Profiles {
        machine: String,
        profiles: Vec<ProfileView>,
    },
    Threads {
        threads: Vec<ThreadView>,
    },
    Thread(ThreadView),
    Events {
        events: Vec<FlowEvent>,
    },
    Questions {
        questions: Vec<QuestionView>,
    },
    Question(QuestionView),
    Message(MessageView),
    Decisions {
        decisions: Vec<DecisionView>,
    },
    Decision(DecisionView),
    Inbox {
        items: Vec<InboxItem>,
    },
    Merge {
        queue: Vec<MergeEntry>,
    },
    Routines {
        routines: Vec<RoutineView>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum FrontEvent {
    /// 머신·pane 목록이 바뀌었다(전체 목록)
    State { machines: Vec<MachineView>, panes: Vec<PaneView> },
    /// workspace layout이 바뀌었다
    Workspace(Workspace),
    /// PaneWatch로 구독한 화면
    Grid(GridFrame),
    /// 흐름 사건(새로 기록될 때). 놓쳤으면 `Events`로 cursor부터 다시 읽는다.
    Flow(FlowEvent),
    /// thread 기록이 바뀌었다
    Thread(ThreadView),
    /// 확인창 질문이 생기거나 답해졌다(알림 대상)
    Question(QuestionView),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FrontMsg {
    Hello {
        proto: Version,
        client: String,
    },
    /// `front`는 이 연결의 ID다(lease 보유자 비교에 쓴다)
    Welcome {
        proto: Version,
        host_id: String,
        role: DeviceRole,
        front: String,
    },
    Reject(ErrorBody),
    Req {
        id: String,
        body: FrontRequest,
    },
    Res {
        id: String,
        result: Outcome<FrontResponse>,
    },
    Event(FrontEvent),
}

/// 웹 터미널 WebSocket에서 서버가 보내는 텍스트 메시지(출력은 binary message)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum TermServerMsg {
    /// 화면을 비우고 replay를 쓴다
    Snapshot {
        replay: B64,
        cols: u16,
        rows: u16,
        readonly: bool,
    },
    /// 입력 가능 여부가 바뀌었다
    Readonly {
        readonly: bool,
        reason: Option<String>,
    },
    State {
        pane: PaneView,
    },
    /// 이 WebSocket의 front ID(처음 한 번). pane.lease.front와 같으면 이 화면이 조작 중이다.
    Front {
        id: String,
    },
    Error(ErrorBody),
}

/// 웹 터미널 WebSocket에서 브라우저가 보내는 메시지
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum TermClientMsg {
    Input {
        data: String,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    /// 조작 lease를 가져온다
    Take,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn link_wire_shape_is_stable() {
        // wire 계약: 이 JSON 모양이 바뀌면 이전 agent와 호환이 깨진다.
        let msg = LinkMsg::Req {
            id: "c1".into(),
            deadline_ms: None,
            body: AgentRequest::Input { session: "s1".into(), data: B64(b"ls\r".to_vec()) },
        };
        assert_eq!(
            serde_json::to_value(&msg).unwrap(),
            json!({"t":"req","id":"c1","deadline_ms":null,"body":{"op":"input","session":"s1","data":"bHMN"}})
        );
        let res = LinkMsg::Res { id: "c1".into(), result: Outcome::Err(ErrorBody::new(ErrorCode::NotFound, "x")) };
        assert_eq!(
            serde_json::to_value(&res).unwrap(),
            json!({"t":"res","id":"c1","result":{"err":{"code":"not_found","message":"x"}}})
        );
    }

    #[test]
    fn unknown_optional_fields_are_ignored_but_unknown_kinds_fail() {
        // 더 새 agent가 모르는 필드를 보내도 이전 host가 읽는다.
        let v = json!({"t":"hello","proto":{"major":1,"minor":4},"machine_id":"m","machine_name":"n",
            "boot_id":"b","os":"macos","arch":"aarch64","software":"aa 9","future_field":{"x":1}});
        let LinkMsg::Hello(h) = serde_json::from_value::<LinkMsg>(v).unwrap() else { panic!() };
        assert!(h.sessions.is_empty() && !h.dev_mode && h.features.is_empty());
        assert!(serde_json::from_value::<LinkMsg>(json!({"t":"teleport"})).is_err());
        assert!(serde_json::from_value::<LinkMsg>(json!({"t":"hello"})).is_err());
    }

    #[test]
    fn session_state_flattens_into_info() {
        let info = SessionInfo {
            id: "s".into(),
            name: "n".into(),
            argv: vec![],
            cwd: None,
            pid: Some(3),
            started_at_ms: 1,
            boot_id: "b".into(),
            cols: 80,
            rows: 24,
            state: SessionState::Exited { code: Some(2) },
            role: None,
        };
        let v = serde_json::to_value(&info).unwrap();
        assert_eq!(v["state"], "exited");
        assert_eq!(v["code"], 2);
        assert_eq!(serde_json::from_value::<SessionInfo>(v).unwrap(), info);
    }

    #[test]
    fn b64_rejects_garbage() {
        assert!(serde_json::from_value::<B64>(json!("%%%")).is_err());
    }

    #[test]
    fn control_classification() {
        // reader가 보낼 수 있는 요청은 읽기뿐이어야 한다.
        assert!(!FrontRequest::PaneRead { pane: "p".into() }.needs_control());
        assert!(FrontRequest::PaneSend { pane: "p".into(), data: B64(vec![]) }.needs_control());
        assert!(FrontRequest::PaneResize { pane: "p".into(), cols: 1, rows: 1 }.needs_control());
        assert!(FrontRequest::WebPair { role: DeviceRole::Reader }.local_only());
        assert!(!FrontRequest::Panes { role: None }.local_only());
        assert!(!FrontRequest::Threads { project: None }.needs_control());
        assert!(FrontRequest::ThreadAck { project: "p".into(), thread: "t".into() }.needs_control());
        assert!(!FrontRequest::Questions { all: true }.needs_control());
        assert!(FrontRequest::QuestionAnswer { id: "q".into(), choice: "yes".into() }.needs_control());
    }

    #[test]
    fn additive_fields_keep_old_messages() {
        // 1.1 front의 `panes`(필드 없음), 1.1 session host의 SessionInfo(role 없음)를 그대로 읽는다
        assert_eq!(
            serde_json::from_value::<FrontRequest>(json!({"op":"panes"})).unwrap(),
            FrontRequest::Panes { role: None }
        );
        let v = json!({"id":"s","name":"","argv":[],"cwd":null,"pid":null,"started_at_ms":1,"boot_id":"b",
            "cols":80,"rows":24,"state":"running"});
        assert_eq!(serde_json::from_value::<SessionInfo>(v).unwrap().role, None);
        let v = json!({"name":"","argv":[],"cwd":null,"cols":80,"rows":24});
        assert_eq!(serde_json::from_value::<SpawnSpec>(v).unwrap().role, None);
        // 1.2 host의 PaneView(agent 없음)와 1.2 front의 thread message(mode 없음 → queue)
        let v = json!({"id":"p","machine_id":"m","machine":"mac-a","name":"","argv":[],"cwd":null,"state":"running",
            "connection":"connected","readonly":false,"cols":80,"rows":24,"lease":null,"role":null});
        assert_eq!(serde_json::from_value::<PaneView>(v).unwrap().agent, None);
        let v = json!({"op":"thread_message","project":"p","thread":"t","text":"x"});
        assert!(matches!(
            serde_json::from_value::<FrontRequest>(v).unwrap(),
            FrontRequest::ThreadMessage { mode: MessageMode::Queue, .. }
        ));
    }
}
