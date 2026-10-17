//! 제어 plane 메시지. 모두 JSON이며 `t` 필드로 종류를 나눈다.
//!
//! - Agent link: host ↔ agent backend (`LinkMsg`)
//! - Session IPC: agent backend ↔ session host (`SessionMsg`)
//! - Front API: front(CLI·TUI·웹) ↔ host (`FrontMsg`, 웹은 `FrontRequest`를 HTTP로 투영)
//!
//! 모르는 선택 필드는 무시한다(serde 기본). 필수 필드가 없거나 모르는 메시지 종류는 오류다.

use std::collections::BTreeMap;

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

use crate::orch::{
    AgentStatus, Cleanup, FlowEvent, HarnessEvent, InboxItem, Launch, LibraryView, MergeEntry, MessageMode,
    MessageView, OwnerChoice, ProfileView, ProjectContext, ProjectView, QuestionView, ReportSnapshot, Role,
    RoutineView, ThreadKind, ThreadPrepare, ThreadReady, ThreadView,
};
use crate::orch::{EvaluationView, EvaluatorView, LandOutcome, MergeLandView};

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
    /// 이 기기에 그 push 구독이 없다(시험 알림, herdr-web-ui `subscription_not_found`)
    SubscriptionNotFound,
    /// push 서비스가 그 구독을 버렸다(404·410, host가 지웠다, herdr-web-ui `subscription_gone`)
    SubscriptionGone,
    /// 받을 수 있는 크기를 넘었다(공유 설정 64KB, herdr-web-ui `too_large`)
    TooLarge,
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
    /// 사람이 붙인 이름(`pane rename`, herdr `manual_label`). 있으면 `name`이 이것이고, 지우면 띄울 때 이름으로 돌아간다.
    /// 정본은 agent의 `spec.json`이다(이전 session host는 모른다, plan §8.109).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// 띄울 때 알려 준 harness(`SpawnSpec.harness`). 다시 띄울 때(resume) 그대로 넘긴다.
    #[serde(default)]
    pub harness: Option<String>,
}

/// 사람이 붙이는 pane 이름의 최대 글자 수(참고 herdr-web-ui `PaneRename` 입력 `maxLength={80}`)
pub const MAX_PANE_LABEL_CHARS: usize = 80;

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
    /// 화면으로 상태를 볼 harness(`claude`·`codex`, profile harness). 없으면 agent가 argv 첫 인자로 정한다(plan §8.110).
    #[serde(default)]
    pub harness: Option<String>,
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
    /// `replay`의 기록 앞에 session host가 가진 기록이 더 있다(plan §8.118). 그 앞은 `History` 요청으로 받는다.
    /// 옛 session host는 보내지 않는다(false: 묻지 않는다).
    #[serde(default)]
    pub history_more: bool,
}

/// 받은 기록 맨 위 줄들(`anchor`) 바로 앞 기록 한 덩어리(plan §8.67·§8.118). replay 기록과 같은 모양(색 포함)이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryChunk {
    pub data: B64,
    pub lines: u32,
    /// 그 앞이 더 있다
    pub more: bool,
    /// anchor를 찾았다(못 찾으면 붙이지 않는다)
    pub found: bool,
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
    /// 보이는 화면을 SGR(굵게·흐리게·반전·색) escape와 함께 한 글로(`Vt::read` Visible·Ansi). herdr `agent read --format ansi`와 같은
    /// 모양이라 herdr-projects `prompt_box`가 흐린 placeholder와 사람이 친 글을 가른다(plan §8.77). 옛 agent는 비워 보낸다.
    #[serde(default)]
    pub ansi: String,
}

/// `pane read`·`pane wait-output`이 읽는 곳(herdr `ReadSource`, `src/api/schema`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ReadSource {
    /// 보이는 화면
    Visible,
    /// 기록과 화면의 마지막 줄들(기본 80줄, 커서·마지막 글 줄까지)
    #[default]
    Recent,
    /// Recent와 같되 화면 폭에서 접힌 줄을 한 줄로 잇는다
    RecentUnwrapped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReadFormat {
    #[default]
    Text,
    /// 글 모양(SGR)을 남긴다
    Ansi,
}

/// `pane read` 결과(herdr `PaneReadResult`). `truncated`면 `lines`보다 앞 줄이 더 있다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReadView {
    pub pane: String,
    pub source: ReadSource,
    pub format: ReadFormat,
    pub text: String,
    pub truncated: bool,
}

/// `thread read`·`thread keys` 결과: 그 thread의 pane과 그때의 harness 상태, 읽은 화면(read) 또는 한 일(keys)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadPaneView {
    pub project: String,
    pub thread: String,
    pub pane: String,
    /// 읽거나 보내기 전 harness 상태(hook 없는 harness는 없다)
    pub agent: Option<crate::AgentActivity>,
    pub read: Option<ReadView>,
    /// 친 글자 수
    pub typed: u32,
    /// 누른 키
    pub pressed: Vec<String>,
}

/// `pane wait-output`이 찾는 것(herdr `OutputMatch`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputMatch {
    Substring { value: String },
    Regex { value: String },
}

/// `pane resize`·`pane split` 방향(herdr `PaneDirection`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PaneDirection {
    Left,
    Right,
    Up,
    Down,
}

/// workspace 하나의 요약(`workspace list`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceSummary {
    pub name: String,
    pub tabs: u32,
    pub panes: u32,
}

/// harness 상태 감지 입력(plan §8.110). herdr처럼 화면 끝 글과 마지막 OSC 제목·진행을 함께 읽는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetectScreen {
    /// 화면 변화 순번(출력이 오거나 크기가 바뀔 때마다 는다)
    pub seq: u64,
    /// 화면 끝 글(herdr `detection_text`). 요청의 `unchanged_since`와 순번이 같으면 None이다(다시 볼 것이 없다).
    pub text: Option<String>,
    /// 마지막 OSC 0/2 제목(없으면 빈 글)
    pub osc_title: String,
    /// 마지막 OSC 9 payload(없으면 빈 글)
    pub osc_progress: String,
}

/// harness 대화 기록 한 줄(§8.27, 읽기 전용). agent가 Claude·Codex 기록 파일에서 골라 낸다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    /// 도구 호출 한 줄(예: `Bash: cargo test`). 도구 결과 본문은 싣지 않는다.
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatView {
    pub pane: String,
    pub harness: String,
    pub messages: Vec<ChatMessage>,
    /// 기록이 더 길어 끝부분만 실었다
    pub truncated: bool,
}

/// 파일 보기의 종류(herdr-web-ui `FileKind`, plan §8.125)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Image,
    Video,
    Audio,
    Pdf,
    Text,
    Binary,
}

/// 파일 보기가 여는 파일(herdr-web-ui `FileInfo`). `path`는 agent 머신의 실제 경로(symlink를 따라간 것)다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub modified_ms: u64,
    pub mime: String,
    pub kind: FileKind,
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
    /// 세션별 마지막 harness 사건(agent가 `harness.json`에 둔 것). host가 연결 즉시 harness 세션 ID를 알아 재부팅 뒤
    /// 대화를 이어 띄운다. host가 `harness_hello.v1`을 알리면 agent는 연결 뒤 같은 사건을 다시 보내지 않는다.
    #[serde(default)]
    pub harness: Vec<HarnessEvent>,
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
    /// 그 세션이 지금 있는 폴더(앞 프로그램, 없으면 셸). agent가 `cwd.v1` 기능을 알릴 때만 보낸다.
    Cwd {
        session: String,
    },
    /// session host 기록에서 `anchor` 바로 앞 덩어리(host mirror에 없는 앞 기록, plan §8.118).
    /// agent가 `history.v1` 기능을 알리고 snapshot이 `history_more`였을 때만 보낸다.
    History {
        session: String,
        anchor: Vec<String>,
        depth: u32,
    },
    /// 파일 보기: 그 세션이 지금 있는 폴더 기준으로 경로를 찾는다(plan §8.125). 허용 폴더 밖이면 Forbidden,
    /// 없으면 NotFound다. agent가 `file.v1` 기능을 알릴 때만 보낸다.
    FileLocate {
        session: String,
        path: String,
    },
    /// 파일 보기: `FileLocate`가 돌려준 실제 경로의 한 조각(`FileChunk`). 그 사이 바뀌었거나 허용 폴더 밖이면 거부한다.
    /// `page`면 HTML 파일을 페이지로 그린 bytes(제 폴더의 그림·style·script를 넣은 것)의 조각이다.
    FileView {
        path: String,
        offset: u64,
        len: u32,
        #[serde(default)]
        page: bool,
    },
    /// 끝난(종료·중단·유실) 세션의 기록을 지운다. 실행 중이면 거부한다.
    Forget {
        session: String,
    },
    /// 세션에 사람이 붙인 이름(`None`이면 지움). 답은 바뀐 `Session`이다. agent가 `rename.v1` 기능을 알릴 때만 보낸다.
    Rename {
        session: String,
        label: Option<String>,
    },
    /// harness 대화 기록의 끝부분(§8.27). agent는 자기 Claude·Codex 기록 폴더 안의 파일만 읽는다.
    Chat {
        harness: String,
        /// Claude hook이 알린 기록 파일
        transcript: Option<String>,
        /// Codex thread-id(기록 파일 이름에 들어 있다)
        harness_session: Option<String>,
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
    /// (claude는 새 `--session-id`). `profile`이 없으면 저장된 launch를 그대로 쓰고, 있으면 이 머신 agent.toml에서
    /// 그 profile을 다시 해석한다(`thread restart --profile`, AGENT_LINK 1.4). 옛 agent는 `profile`을 모르고 저장된
    /// launch를 돌려주므로 host는 돌려받은 `Launch.profile`로 확인한다.
    ThreadRebrief {
        work_dir: String,
        brief: String,
        launch: Launch,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
    },
    /// routine 명령을 `sh -c`로 한 번 실행한다(자기 process group, 시간 초과면 그 group을 끝낸다).
    /// `cwd`가 없으면 `<work_root>/<project>/routines`에서 실행한다.
    RunCommand {
        project: String,
        cwd: Option<String>,
        command: String,
        timeout_ms: u64,
    },
    /// 붙여 넣은 그림 한 조각을 세션 폴더(`sessions/<id>/attachments/`)에 쓴다. `att`는 host가 정한 이름(hex.확장자)이다.
    /// `offset`은 지금까지 쓴 길이와 같아야 하고, `eof`면 sha256을 확인해 이름을 바꾸고 절대 경로를 돌려준다.
    AttachWrite {
        session: String,
        att: String,
        offset: u64,
        data: B64,
        eof: bool,
    },
    /// 사후 평가자 사건 묶음을 작업 폴더의 `inbox/` 아래에 쓴다(§8.13). `path`는 `inbox/<이름>.json`만 받는다.
    WorkFileWrite {
        work_dir: String,
        path: String,
        data: B64,
    },
    /// 병합 단계(§8.24). `repo`는 사용자 checkout, `cwd`는 thread worktree다. `remote`/`target`(예: origin/main)으로 push한다.
    MergeLand {
        repo: String,
        cwd: String,
        branch: String,
        remote: String,
        target: String,
        message: String,
    },
    /// thread 정리: worktree를 force 없이 지우고, PR이 병합됐고 worktree가 지워졌고 branch 끝이 병합된 PR head와
    /// 같을 때만 branch를 지운다(herdr-projects threads.rs `clean`·`delete_branch`, plan §8.113).
    /// `merged_head`: PR이 병합됐으면 그 head commit(모르면 빈 값), 병합되지 않았으면 None. 옛 host가 보낸
    /// `base`는 읽지 않는다(branch를 남긴다).
    ThreadCleanup {
        repo: Option<String>,
        cwd: String,
        branch: Option<String>,
        #[serde(default)]
        merged_head: Option<String>,
        keep_worktree: bool,
    },
    /// PR 조회(§8.80, herdr-projects `pr.rs`). `cwd`(thread worktree)의 origin을 읽고 `gh`로 묻는다. host는 gh를 실행하지
    /// 않는다(결정 3). agent 기능 `pr.v1`이 있을 때만 보낸다.
    PrQuery {
        cwd: String,
        query: PrQuery,
    },
}

/// `AgentRequest::PrQuery`가 묻는 것
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
pub enum PrQuery {
    /// `gh pr view --json …` 출력(JSON 그대로)
    View { url: String },
    /// head가 이 branch인 PR 주소(없으면 빈 값). origin이 GitHub가 아니면 묻지 않는다.
    Find { branch: String },
    /// `gh`가 쓰는 계정 login
    Login,
    /// `repo`(`owner/repo`)에서 `gh` 계정이 연 열린 PR 번호들, 쉼표로(herdr-projects `open_own_numbers`).
    /// agent 기능 `pr.v2`가 있을 때만 보낸다.
    OpenOwn { repo: String },
}

/// PR 조회 결과. `out`은 질문마다 view JSON·찾은 주소·login이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrAnswer {
    /// `git remote get-url origin`(없으면 빈 값)
    pub origin: String,
    pub out: String,
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
    /// 상태 감지 입력(session host → agent, SESSION_IPC 1.1)
    Detect(DetectScreen),
    Done,
    Chat {
        messages: Vec<ChatMessage>,
        truncated: bool,
    },
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
    Landed(LandOutcome),
    Relaunch(Launch),
    CommandRan(CommandRan),
    /// 첨부를 다 썼다(절대 경로와 sha256)
    Attached {
        path: String,
        sha256: String,
    },
    /// 세션이 지금 있는 폴더(읽지 못하면 None)
    Cwd {
        cwd: Option<String>,
    },
    /// 찾은 파일. 없으면 끝이 같은 파일 후보(실제 경로, 둘 이상)
    FileLocated {
        info: Option<FileInfo>,
        candidates: Vec<String>,
    },
    Pr(PrAnswer),
    History(HistoryChunk),
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
    /// harness hook·notify·`aaa report`에서 온 상태 사건(adapter가 정규화)
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
    /// 머신 건강(주기 보고). host가 `health.v1`을 알렸을 때만 보낸다.
    Health(MachineHealth),
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
    Subscribe {
        from: Option<u64>,
    },
    Unsubscribe,
    Input {
        data: B64,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Kill,
    Screen,
    /// 상태 감지 입력(SESSION_IPC 1.1부터). 순번이 `unchanged_since`와 같으면 글을 싣지 않는다.
    Detect {
        unchanged_since: Option<u64>,
    },
    /// 기록에서 `anchor` 바로 앞 덩어리(plan §8.118). snapshot에 `history_more`를 실은 session host에만 보낸다.
    History {
        anchor: Vec<String>,
        depth: u32,
    },
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
    /// agent와 끊겼다. 세션은 계속 돌 수 있으나 aaa에서는 읽기 전용이다.
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
    /// 마지막 건강 보고(§8.10). 연결 중이 아니면 오래된 값이다.
    pub health: Option<MachineHealth>,
    /// agent 시계 - host 시계(보고를 받은 때 기준)
    #[ts(type = "number | null")]
    pub clock_skew_ms: Option<i64>,
    /// 새 세션을 거부하는 이유(연결 중일 때만). 비었으면 정상이다.
    pub problems: Vec<HealthProblem>,
}

/// agent가 주기적으로 보내는 머신 건강(§8.10)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineHealth {
    /// agent 시계
    #[ts(type = "number")]
    pub at_ms: u64,
    /// 작업 root가 있는 디스크의 여유·전체(알 수 없으면 None)
    #[ts(type = "number | null")]
    pub disk_free_bytes: Option<u64>,
    #[ts(type = "number | null")]
    pub disk_total_bytes: Option<u64>,
    /// 1분 평균 부하 ×100과 CPU 수
    pub load_x100: Option<u32>,
    pub cpus: u32,
    /// 전원에 연결됐는가(배터리가 없는 머신은 true, 알 수 없으면 None)
    pub on_ac: Option<bool>,
    pub toolchain: Vec<ToolCheck>,
    /// agent가 본 이상(디스크 부족·toolchain 실패). 새 세션을 거부한다.
    pub problems: Vec<HealthProblem>,
    /// 거부하지는 않는 주의(배터리 전원 등)
    pub warnings: Vec<String>,
}

/// 건강 이상 하나. `code`는 바뀌지 않는 종류(`disk`·`toolchain:<이름>`·`clock`)이고 `text`는 지금 값이 든 설명이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HealthProblem {
    pub code: String,
    pub text: String,
}

/// agent.toml `[health.toolchain]`의 점검 명령 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ToolCheck {
    pub name: String,
    pub ok: bool,
    /// 출력 첫 줄(실패면 이유)
    pub version: String,
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
    /// 사람이 붙인 이름이 있으면 그 이름(`name`과 같다). 웹 메뉴가 「이름 지우기」를 보일지 정한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub label: Option<String>,
    /// 재부팅 뒤 다시 띄우지 못한 까닭(herdr `restore_error`). 그 pane은 중단으로 남는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub restore_error: Option<String>,
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
    /// pane 하나를 탭 가득 보인다(herdr `Tab.zoomed`). 보이는 pane은 front마다 그 front의 포커스 pane이다.
    /// 나누기·닫기·옮기기로 탭의 pane이 바뀌면 풀린다.
    #[serde(default)]
    pub zoomed: bool,
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
    /// 새 탭에 새 pane. `name`은 pane(세션) 이름, `label`은 탭 이름(없으면 자리 번호).
    /// cwd를 주지 않으면 host `[terminal] new_cwd`를 따른다(follow면 `from` pane이 지금 있는 폴더, plan §8.76).
    /// `cols`·`rows`는 화면이 붙기 전 처음 PTY 크기(기본 120×32)
    NewTab {
        machine: Option<String>,
        name: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
        #[serde(default)]
        cols: Option<u16>,
        #[serde(default)]
        rows: Option<u16>,
        #[serde(default)]
        label: Option<String>,
        #[serde(default)]
        from: Option<String>,
    },
    /// pane 자리를 나눠 새 pane을 b 쪽에 둔다. `ratio`는 a 쪽 천분율(기본 500, herdr `pane split --ratio`).
    /// cwd를 주지 않으면 `[terminal] new_cwd`(follow면 나눈 pane의 폴더)
    Split {
        pane: String,
        dir: SplitDir,
        machine: Option<String>,
        argv: Vec<String>,
        cwd: Option<String>,
        #[serde(default)]
        ratio: Option<u16>,
    },
    /// 이미 있는 pane을 새 탭으로 넣는다
    AddTab { pane: String },
    /// layout에서 뺀다(kill이면 세션도 끝낸다)
    Close { pane: String, kill: bool },
    /// pane 가장자리를 그 방향으로 옮긴다(herdr `pane resize`): 그쪽 가장자리의 나눔(없으면 반대쪽)의 비율을
    /// 오른쪽·아래면 늘리고 왼쪽·위면 줄인다. `amount`는 천분율(기본 50, 최대 500), 비율은 100..=900으로 묶는다.
    Resize { pane: String, direction: PaneDirection, amount: Option<u16> },
    /// 탭 이름을 바꾼다
    RenameTab { tab: String, name: String },
    /// 탭을 닫는다. 그 안 pane도 끝낸다(herdr `tab close`)
    CloseTab { tab: String },
    /// pane을 지금 자리에서 빼 새 탭에 둔다(herdr `pane move --new-tab`)
    MoveToNewTab { pane: String, name: Option<String> },
    /// 탭 가득 보이기(herdr `pane zoom`). `zoomed`가 없으면 뒤집는다. pane이 하나뿐인 탭은 그대로다(plan §8.91)
    Zoom {
        pane: String,
        #[serde(default)]
        zoomed: Option<bool>,
    },
    /// 같은 탭의 두 pane 자리를 바꾼다(herdr `pane swap`의 source·target 꼴). 칸과 비율은 그대로다.
    /// 방향 이웃은 화면 크기를 아는 front가 찾는다(host는 front마다 다른 칸을 모른다).
    Swap { pane: String, with: String },
    /// 나눔 하나의 비율을 바로 정한다(herdr `layout.set_split_ratio`, 나눔 경계 끌기). `path`는 탭 뿌리에서 그 나눔까지
    /// (false = a, true = b), `ratio`는 a 쪽 천분율이고 100..=900으로 묶는다(plan §8.106)
    SplitRatio { tab: String, path: Vec<bool>, ratio: u16 },
    /// 탭을 `index` 앞으로 옮긴다(herdr `tab.move`의 `insert_index`: 0..=탭 수, 옮기기 전 차례로 센다)
    MoveTab { tab: String, index: u16 },
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
    /// 그 pane 프로그램이 켠 모드(1.14부터, 옛 host는 기본값)
    #[serde(default)]
    pub modes: PaneModes,
}

/// pane 프로그램이 켠 터미널 모드. TUI가 마우스를 넘기고 휠·화살표를 고를 때 쓴다(herdr `wheel_routing`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PaneModes {
    #[serde(default)]
    pub mouse: MouseMode,
    #[serde(default)]
    pub mouse_encoding: MouseEncoding,
    /// alternate screen(`CSI ? 1049 h` 등)
    #[serde(default)]
    pub alt_screen: bool,
    /// DECCKM(`CSI ? 1 h`): 화살표를 `ESC O x`로
    #[serde(default)]
    pub app_cursor: bool,
}

/// 마우스 보고 모드(xterm `CSI ? 9/1000/1002/1003 h`)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MouseMode {
    #[default]
    None,
    /// 9: 누름만
    Press,
    /// 1000: 누름·뗌·휠
    PressRelease,
    /// 1002: 누른 채 움직임도
    ButtonMotion,
    /// 1003: 모든 움직임
    AnyMotion,
}

/// 마우스 보고 인코딩(기본 X10 바이트, `CSI ? 1005 h` UTF-8, `CSI ? 1006 h` SGR)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MouseEncoding {
    #[default]
    Default,
    Utf8,
    Sgr,
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
    /// aaa 웹을 이미 proxy하는 HTTPS 주소
    pub url: Option<String>,
    /// aaa가 만든 serve 포트(끌 때 이 포트만 끈다)
    pub owned_port: Option<u16>,
    pub web_port: u16,
    pub message: Option<String>,
}

/// pairing한 웹 기기 하나(herdr-web-ui `PairedDevice`). 「이 기기」 표시는 웹이 `/api/v1/me`의 id로 정한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeviceView {
    pub id: String,
    pub label: String,
    /// reader 또는 controller
    pub role: DeviceRole,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    /// 마지막으로 들어온 때(1분 단위로 적는다). 아직 한 번도 안 들어왔으면 없다
    #[ts(type = "number | null")]
    pub last_seen_ms: Option<u64>,
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
    /// harness(Claude·Codex) 대화 기록 보기(§8.27, 읽기 전용)
    PaneChat {
        pane: String,
    },
    /// 파일 보기(plan §8.125, 결정 20 D7): pane이 지금 있는 폴더 기준으로 경로를 찾고, 찾았으면 짧은 수명의
    /// 읽기 경로(`url`)를 준다. 조작 기기만(참고도 보기 전용 기기는 403).
    PaneFile {
        pane: String,
        path: String,
    },
    PaneSend {
        pane: String,
        data: B64,
    },
    /// 글자로 읽는다(herdr `pane read`). host가 가진 화면 사본에서 읽는다.
    PaneReadText {
        pane: String,
        #[serde(default)]
        source: ReadSource,
        /// 줄 수(기본: Recent 80줄, Visible은 화면 전부, 최대 1000)
        #[serde(default)]
        lines: Option<u32>,
        #[serde(default)]
        format: ReadFormat,
    },
    /// 글을 그대로 보낸다(herdr `pane send-text`: 감싸지 않고 Enter도 없다)
    PaneSendText {
        pane: String,
        text: String,
    },
    /// 키 이름(herdr 문법: `enter`·`esc`·`ctrl+c`·`shift+tab`·`up`·`f5`·글자 하나)을 그 프로그램이 켠 입력 모드(DECCKM 등)대로 보낸다.
    /// 모두 해석한 뒤에 보낸다(하나라도 모르면 아무것도 보내지 않는다).
    PaneSendKeys {
        pane: String,
        keys: Vec<String>,
    },
    /// 글 + Enter(herdr `pane run`): 프로그램이 bracketed paste를 켰으면 글을 감싸고 Enter와 한 번에 쓴다
    PaneSubmit {
        pane: String,
        text: String,
    },
    /// 출력에 글이 나올 때까지 기다린다(herdr `pane wait-output`). 화면 사본에 출력이 들어올 때마다 다시 본다.
    PaneWaitOutput {
        pane: String,
        #[serde(rename = "match")]
        matcher: OutputMatch,
        #[serde(default)]
        source: ReadSource,
        #[serde(default)]
        lines: Option<u32>,
        /// 없으면 끝없이 기다린다(herdr와 같다)
        #[serde(default)]
        #[ts(type = "number | null")]
        timeout_ms: Option<u64>,
    },
    /// pane 하나(ID·ID 앞부분·이름)
    PaneGet {
        pane: String,
    },
    /// pane을 닫는다(herdr `pane close`): 세션을 끝내고, 끝나기를 기다려 layout과 기록에서 지운다
    PaneClose {
        pane: String,
    },
    /// workspace 목록
    Workspaces,
    /// workspace를 닫는다: 그 안 pane을 끝내고 layout을 지운다. thread pane은 끝내지 않고 layout에서만 뺀다.
    WorkspaceClose {
        name: String,
    },
    WorkspaceRename {
        name: String,
        to: String,
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
    /// 붙여 넣은 그림 한 조각(TUI Ctrl+V·`aaa pane attach`, 최대 512KiB). 웹 `TermClientMsg::Attach`와 같은 규칙이다:
    /// `offset` 0이 새 첨부의 시작이고, 마지막 조각에서 `sha256`(파일 전체)을 확인한다. 받는 중인 첨부는 연결마다 하나다.
    PaneAttach {
        pane: String,
        #[ts(type = "number")]
        offset: u64,
        #[ts(type = "number")]
        total: u64,
        sha256: String,
        data: B64,
    },
    /// 끝난 pane(중단·유실·종료)을 같은 spec으로 다시 띄우고 layout 자리를 바꾼다
    PaneResume {
        pane: String,
    },
    /// 끝난 pane 기록을 지운다
    PaneForget {
        pane: String,
    },
    /// pane에 이름을 붙인다(herdr `pane rename`). 앞뒤 공백을 떼고, `None`이나 빈 이름은 붙인 이름을 지워 띄울 때 이름으로 돌린다.
    PaneRename {
        pane: String,
        label: Option<String>,
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
    /// pairing 코드를 만든다. 아직 쓰지 않은 앞 코드는 버린다(herdr-web-ui `startPairing`). 웹 기기는 controller 코드만(D6)
    WebPair {
        role: DeviceRole,
    },
    /// pairing한 웹 기기 목록
    Devices,
    /// 기기 이름을 바꾼다
    DeviceRename {
        id: String,
        label: String,
    },
    /// 기기를 취소한다. 그 기기의 열린 연결은 바로 닫히고 push 구독도 지워진다
    DeviceRevoke {
        id: String,
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
        /// 비면 `from_task`의 owner, 그다음 project.toml `thread_profile`(herdr-projects `thread_profile`)
        profile: String,
        /// 없으면 repo가 있으면 worktree, 없으면 dir(herdr-projects `placement`)
        #[serde(default)]
        kind: Option<ThreadKind>,
        repo: Option<String>,
        base: Option<String>,
        /// 할 일(threads/<id>.task.md로 저장하고 brief에 넣는다)
        task: String,
        /// TASKS.md의 task 제목(대소문자 무시). owner가 profile·machine의 기본값이 되고, 메모가 할 일에 붙는다.
        #[serde(default)]
        from_task: Option<String>,
    },
    /// thread의 Next 목록을 보이거나(`line`·`add` 없음), N번째 줄을 thread에 메시지로 보내거나, 줄을 더한다
    ThreadNext {
        project: String,
        thread: String,
        line: Option<u32>,
        add: Option<String>,
    },
    /// coordinator용 프로젝트 요약. 보인 inbox 항목은 「봤다」가 된다(`peek`이면 그대로).
    Context {
        project: String,
        #[serde(default)]
        peek: bool,
    },
    /// thread를 맡길 수 있는 owner 목록. `check`를 주면 그 owner를 TASKS.md에 써도 되는지 본다.
    Assignable {
        project: String,
        check: Option<String>,
    },
    /// thread 목록. 기본은 사후 평가자를 빼고, `role`을 주면 그 역할만 보인다(§8.13).
    Threads {
        project: Option<String>,
        #[serde(default)]
        role: Option<Role>,
    },
    ThreadShow {
        project: String,
        thread: String,
    },
    /// 보고서와 결과물 목록, 그리고 결과물을 볼 짧은 수명의 읽기 전용 경로
    ThreadLibrary {
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
        /// 끝낸 thread를 다시 연다(herdr-projects `thread resolve --reopen`). 아무것도 띄우지 않는다
        #[serde(default)]
        reopen: bool,
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
        /// 이 profile로 새 대화를 띄운다(herdr-projects `thread restart --profile`). `resume`과 함께 쓰지 않는다
        #[serde(default)]
        profile: Option<String>,
    },
    /// 사람이 명시적으로 brief 안내를 다시 보낸다(자동 재전송은 하지 않는다)
    ThreadBrief {
        project: String,
        thread: String,
    },
    /// thread pane이 지금 보이는 것(신뢰 창·질문 메뉴·권한 확인창, herdr-projects `thread read`). `lines`면 보이는 화면 대신
    /// 기록 끝 N줄
    ThreadRead {
        project: String,
        thread: String,
        #[serde(default)]
        lines: Option<u32>,
    },
    /// thread pane이 보이는 것에 답한다(herdr-projects `thread keys`): `text`를 치고(Enter 없음) `keys`를 누른다
    ThreadKeys {
        project: String,
        thread: String,
        #[serde(default)]
        keys: Vec<String>,
        #[serde(default)]
        text: Option<String>,
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
    /// 이 host의 웹 화면들이 함께 쓰는 설정과 고정(herdr-web-ui `GET /api/preferences`)
    Preferences,
    /// 공유 설정을 바꾼다: 바꾼 설정만, 고정은 바뀌었을 때만(herdr-web-ui `PUT /api/preferences`). 보기 권한은 못 한다
    PreferencesSet {
        #[ts(type = "Record<string, unknown>")]
        settings: BTreeMap<String, serde_json::Value>,
        #[serde(default)]
        pins: Option<Vec<String>>,
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
    /// ticker를 지금 한 번 돌리고, 이 요청 뒤에 시작한 tick이 끝나면 답한다(herdr-projects 시험의 `ticker::tick_project`,
    /// t3code worker `drain`과 같은 자리). host가 「봤는데 할 일이 없었다」를 시험이 기다릴 수 있다(docs/testing.md).
    TickerTick,
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
    /// 병합 대기열 맨 앞 thread를 병합한다(§8.24): 그 머신 agent가 원격 일치·병합·tree 같음 확인·push·사용자 checkout 맞춤·정리를 한다.
    /// `thread`를 주면 그것이 맨 앞이어야 한다.
    MergeLand {
        project: String,
        #[serde(default)]
        thread: Option<String>,
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
    /// routine 켜기·끄기(front matter `enabled`, herdr-projects `routine toggle`). `on`이 없으면 뒤집는다.
    /// 명령 승인은 그대로 필요하다.
    RoutineToggle {
        project: String,
        name: String,
        #[serde(default)]
        on: Option<bool>,
    },
    /// 흐름 사건을 `after`(cursor) 다음부터 읽는다
    Events {
        project: Option<String>,
        #[serde(default)]
        after: u64,
        #[serde(default)]
        limit: Option<u32>,
    },
    /// 사후 평가자 지정(§8.13): 역할이 evaluator인 thread를 시작한다. 프로젝트당 하나다.
    EvaluatorAssign {
        project: String,
        profile: String,
        machine: Option<String>,
        /// 개선을 직접 고칠 repo(없으면 repo 없는 작업 폴더)
        #[serde(default)]
        repo: Option<String>,
        /// 평가자에게 더할 말(brief 끝에 붙는다)
        #[serde(default)]
        note: Option<String>,
    },
    /// 사후 평가자 상태(프로젝트를 주지 않으면 전체)
    EvaluatorStatus {
        project: Option<String>,
    },
    /// 사후 평가자를 끝낸다(평가자 thread resolve)
    EvaluatorUnassign {
        project: String,
    },
    /// 평가 기록 목록. 기본은 정하지 않은 개선안이 있는 것만
    Evaluations {
        project: String,
        #[serde(default)]
        all: bool,
    },
    EvaluationShow {
        project: String,
        id: String,
    },
    /// 개선안 채택·보류(평가 기록에 누가 언제 정했는지 남는다)
    EvaluationDecide {
        project: String,
        id: String,
        proposals: Vec<String>,
        adopt: bool,
        #[serde(default)]
        note: Option<String>,
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
                | FrontRequest::ThreadLibrary { .. }
                | FrontRequest::ThreadNext { line: None, add: None, .. }
                | FrontRequest::Context { peek: true, .. }
                | FrontRequest::Assignable { .. }
                | FrontRequest::Events { .. }
                | FrontRequest::Questions { .. }
                | FrontRequest::Inbox { peek: true, .. }
                | FrontRequest::MergeList { .. }
                | FrontRequest::Routines { .. }
                | FrontRequest::ThreadRead { .. }
                | FrontRequest::PaneRead { .. }
                | FrontRequest::PaneReadText { .. }
                | FrontRequest::PaneWaitOutput { .. }
                | FrontRequest::Workspaces
                | FrontRequest::PaneGet { .. }
                | FrontRequest::PaneChat { .. }
                | FrontRequest::PaneWatch { .. }
                | FrontRequest::PaneUnwatch { .. }
                | FrontRequest::Workspace { .. }
                | FrontRequest::WebStatus
                | FrontRequest::Devices
                | FrontRequest::EvaluatorStatus { .. }
                | FrontRequest::Evaluations { .. }
                | FrontRequest::EvaluationShow { .. }
                | FrontRequest::Preferences
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
                | FrontRequest::MergeLand { .. }
                | FrontRequest::WebPublish
                | FrontRequest::WebPublishOff
                | FrontRequest::TickerTick
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
    Next {
        lines: Vec<String>,
        /// `line`으로 보낸 메시지
        message: Option<MessageView>,
    },
    Context(ProjectContext),
    Assignable {
        owners: Vec<OwnerChoice>,
        /// `check`가 통과했을 때 설명
        valid: Option<String>,
    },
    Screen(ScreenText),
    Read(ReadView),
    /// `thread read`·`thread keys`
    ThreadPane(ThreadPaneView),
    /// `pane wait-output`이 찾았다: 맞은 줄과 그때 읽은 글
    OutputMatched {
        matched_line: String,
        read: ReadView,
    },
    Workspaces {
        workspaces: Vec<WorkspaceSummary>,
    },
    Chat(ChatView),
    /// 찾은 파일과 읽기 경로(`/f/<cap>/<이름>`, 30분), 아니면 고를 후보
    PaneFile {
        info: Option<FileInfo>,
        candidates: Vec<String>,
        url: Option<String>,
    },
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
    Devices {
        devices: Vec<DeviceView>,
    },
    Workspace(Workspace),
    Done,
    /// 붙여 넣은 그림을 대상 머신에 저장했다(`PaneAttach` 마지막 조각). `inserted`면 harness 입력창에 경로를 넣었다.
    Attached {
        path: String,
        inserted: bool,
        note: Option<String>,
    },
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
    Inbox {
        items: Vec<InboxItem>,
        /// `inbox_done`이 끝낸 항목 수(그때 `items`는 남은 항목). 목록 요청에는 없다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        done: Option<u32>,
        /// 응답 크기 한도로 싣지 못한 끝내지 않은 항목 수(실은 것은 오래된 것부터다)
        #[serde(default)]
        omitted_open: u32,
        /// 응답 크기 한도로 싣지 못한 끝낸 항목 수(`all`, 실은 것은 최근 것부터다)
        #[serde(default)]
        omitted_done: u32,
    },
    Merge {
        queue: Vec<MergeEntry>,
    },
    /// `ticker_tick`: 이 host 실행에서 끝난 tick 수(방금 돈 것까지)
    Ticker {
        runs: u64,
    },
    MergeLand(Box<MergeLandView>),
    Routines {
        routines: Vec<RoutineView>,
    },
    Library(LibraryView),
    Evaluators {
        evaluators: Vec<EvaluatorView>,
    },
    Evaluations {
        evaluations: Vec<EvaluationView>,
    },
    Evaluation(Box<EvaluationView>),
    Preferences(SharedPreferences),
}

/// 이 host의 모든 웹 화면이 함께 쓰는 것: 기기에 기대지 않는 설정과 고정(herdr-web-ui `shared/preferences.ts` `SharedPreferences`).
/// 값의 뜻은 웹이 본다. host는 공유 이름(`SHARED_SETTING_KEYS`)과 모양·한도만 지킨다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SharedPreferences {
    /// 처음 쓰기 전에는 0, 쓸 때마다 하나 오른다
    #[ts(type = "number")]
    pub revision: u64,
    /// 어느 화면이 나눈 설정만 있다. 아무도 안 바꾼 설정은 없고, 기기마다 제 것으로 남는다
    #[ts(type = "Record<string, unknown>")]
    pub settings: BTreeMap<String, serde_json::Value>,
    /// 목록 고정 열쇠(agent는 `machine:pane`), 고정한 순서대로. 화면이 처음 고정을 나누기 전에는 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pins: Option<Vec<String>>,
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
    /// 공유 설정 문서 전체. 웹 events 연결이 열릴 때와 바뀔 때마다 온다(herdr-web-ui `preferences` 사건)
    Preferences(SharedPreferences),
    /// 상태 사건 연결의 `ping`에 대한 답(herdr-web-ui ws.ts 「Liveness」: 그 ping의 답만 연결이 살아 있음을 말한다)
    Pong { id: u32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FrontMsg {
    Hello {
        proto: Version,
        client: String,
        /// aaa 세션 안에서 실행한 CLI면 그 세션 ID(`AAA_SESSION_ID`). 사후 평가자 세션이면 host가 보기 권한만 준다(§8.13).
        #[serde(default)]
        session: Option<String>,
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
    /// 화면을 비우고 replay를 쓴다. `stream`은 이 snapshot부터의 출력 구독 번호다: 브라우저는 replay와 뒤따르는
    /// binary 출력을 xterm이 다 쓴 뒤 그 끝(replay 바이트 + 출력 바이트)을 `Ack`로 알린다(herdr-web-ui `pty-ack`).
    Snapshot {
        replay: B64,
        cols: u16,
        rows: u16,
        readonly: bool,
        stream: u32,
        /// replay에 싣지 않은 앞 기록이 host에 더 있다: 위로 굴려 끝에 닿으면 `History`로 받아 온다(plan §8.67)
        history_more: bool,
    },
    /// `History`의 답: anchor 바로 앞 기록 `lines`줄(replay 기록과 같은 모양, 줄마다 `ESC[m CR LF`), 그 앞에 더 있는지.
    /// `found`가 false면 host 기록에서 anchor를 찾지 못했다(크기가 바뀌어 줄이 달라졌거나 그만큼 지나 기록에서 빠짐).
    History {
        id: u32,
        data: B64,
        lines: u32,
        more: bool,
        found: bool,
    },
    /// 입력 가능 여부가 바뀌었다
    Readonly {
        readonly: bool,
        reason: Option<String>,
    },
    State {
        // box: PaneView가 커서 다른 variant까지 크게 만든다(clippy large_enum_variant). wire 모양은 같다
        pane: Box<PaneView>,
    },
    /// 이 WebSocket의 front ID(처음 한 번). pane.lease.front와 같으면 이 화면이 조작 중이다.
    Front {
        id: String,
    },
    /// 붙여 넣은 그림을 대상 머신에 저장했다. `inserted`면 harness 입력창에 경로를 넣었다(Enter는 보내지 않는다).
    Attached {
        path: String,
        inserted: bool,
        note: Option<String>,
    },
    /// `Submit`의 답(herdr-web-ui `submit-result`). `ok`면 글과 Enter가 pane에 들어갔다. 아니면 `message`가 이유다.
    Submitted {
        id: u32,
        ok: bool,
        message: Option<String>,
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
    /// 이 화면이 지금 쓰는 화면이 된다(크기를 이 화면으로). 입력·크기만으로도 같아지므로 보통은 필요 없다.
    Take,
    /// 이 화면을 쓰지 않는다(탭이 1초 넘게 가려짐): 다른 화면이 크기를 되찾게 놓는다
    Release,
    /// 입력줄 한 줄(herdr-web-ui `submit`): host가 `data`를 쓰고 조금 뒤 Enter를 쓴 다음 `Submitted`로 답한다.
    /// 브라우저는 그 답을 받고서야 보낸 앞부분을 입력줄에서 지운다(그 사이 친 글은 남는다).
    Submit {
        id: u32,
        data: String,
    },
    /// 받은 기록보다 앞 덩어리를 달라(위로 굴려 끝에 가까워졌다). `anchor`는 받은 기록 맨 위 몇 줄의 글,
    /// `depth`는 그 첫 줄부터 맨 아래 줄까지의 줄 수(같은 글이 여러 곳이면 가까운 곳을 고른다)
    History {
        id: u32,
        anchor: Vec<String>,
        depth: u32,
    },
    /// xterm이 이 구독(`Snapshot.stream`)의 출력을 `offset` 바이트까지 다 썼다(herdr-web-ui `pty-ack`).
    /// host는 이것으로 이 화면이 따라오는지 보고, 따라오지 못하면 4008로 닫는다(plan §8.62).
    Ack {
        stream: u32,
        #[ts(type = "number")]
        offset: u64,
    },
    /// 붙여 넣은 그림 한 조각(최대 512KiB). `offset` 0이 새 첨부의 시작이고, 마지막 조각에서 `sha256`(파일 전체)을 확인한다.
    Attach {
        #[ts(type = "number")]
        offset: u64,
        #[ts(type = "number")]
        total: u64,
        sha256: String,
        data: B64,
    },
}

/// 상태 사건 WebSocket(`/api/v1/events`)에서 브라우저가 보내는 메시지
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum EventsClientMsg {
    /// 이 화면을 지금 쓰고 있다(보이고 focus가 있다, herdr-web-ui `presence`). 연결이 끊기면 쓰지 않는 것이다.
    Presence { active: bool },
    /// 사람이 이 기기에서 방금 눌렀거나 쳤거나 화면을 켰다(보이고 focus가 됐다·앱을 열었다). host는 마지막으로 이것을 보낸
    /// 기기(또는 pane에 입력한 기기)를 「쓰는 기기」로 두고, 그 기기가 지금 쓰는 화면일 때만 Web Push를 건너뛴다
    /// (사용자 결정 10-18, plan §8.105. 참고는 쓰는 화면이 하나라도 있으면 건너뛴다)
    Touch,
    /// 연결이 살아 있는지 묻는다. host는 같은 `id`의 `pong`으로 답한다(herdr-web-ui ws.ts `probe`·server `ping`).
    /// 휴대폰이 잠들었다 깨거나 망이 바뀌면 열린 것처럼 보이는 죽은 연결이 몇 분씩 남는다.
    Ping { id: u32 },
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
            "boot_id":"b","os":"macos","arch":"aarch64","software":"aaa 9","future_field":{"x":1}});
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
            label: None,
            harness: Some("codex".into()),
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
        // thread pane 읽기는 보기, 키로 답하기는 조작이다(세션 안 CLI는 보기만, §8.25)
        let (project, thread) = ("p".to_string(), "t-0001".to_string());
        assert!(
            !FrontRequest::ThreadRead { project: project.clone(), thread: thread.clone(), lines: None }.needs_control()
        );
        assert!(FrontRequest::ThreadKeys { project, thread, keys: vec!["enter".into()], text: None }.needs_control());
        assert!(!FrontRequest::PaneChat { pane: "p".into() }.needs_control());
        // 파일 보기는 조작 기기만(herdr-web-ui도 보기 전용 기기는 fs/ 403)
        assert!(FrontRequest::PaneFile { pane: "p".into(), path: "a.txt".into() }.needs_control());
        assert!(FrontRequest::PaneSend { pane: "p".into(), data: B64(vec![]) }.needs_control());
        assert!(FrontRequest::PaneResize { pane: "p".into(), cols: 1, rows: 1 }.needs_control());
        assert!(
            FrontRequest::PaneAttach {
                pane: "p".into(),
                offset: 0,
                total: 1,
                sha256: String::new(),
                data: B64(vec![])
            }
            .needs_control()
        );
        // 웹 설정에서도 코드를 만든다(D6). 조작 권한이 있어야 하고, 목록은 보기 권한도 본다
        assert!(!FrontRequest::WebPair { role: DeviceRole::Controller }.local_only());
        assert!(FrontRequest::WebPair { role: DeviceRole::Controller }.needs_control());
        assert!(!FrontRequest::Devices.needs_control());
        assert!(FrontRequest::DeviceRename { id: "d".into(), label: "x".into() }.needs_control());
        assert!(FrontRequest::DeviceRevoke { id: "d".into() }.needs_control());
        assert!(!FrontRequest::Panes { role: None }.local_only());
        assert!(!FrontRequest::Threads { project: None, role: None }.needs_control());
        // 사후 평가자 세션은 Reader로 다룬다: 평가 보기는 되고, 지정·채택·메시지는 안 된다(§8.13)
        assert!(!FrontRequest::EvaluatorStatus { project: None }.needs_control());
        assert!(!FrontRequest::Evaluations { project: "p".into(), all: false }.needs_control());
        assert!(
            FrontRequest::EvaluationDecide {
                project: "p".into(),
                id: "e".into(),
                proposals: vec![],
                adopt: true,
                note: None
            }
            .needs_control()
        );
        assert!(FrontRequest::EvaluatorUnassign { project: "p".into() }.needs_control());
        assert!(
            FrontRequest::ThreadMessage {
                project: "p".into(),
                thread: "t".into(),
                text: "x".into(),
                mode: MessageMode::Queue
            }
            .needs_control()
        );
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
        // 1.2 front의 threads(role 없음)와 hello(session 없음)
        assert_eq!(
            serde_json::from_value::<FrontRequest>(json!({"op":"threads","project":null})).unwrap(),
            FrontRequest::Threads { project: None, role: None }
        );
        let v = json!({"t":"hello","proto":{"major":1,"minor":2},"client":"cli"});
        assert!(matches!(serde_json::from_value::<FrontMsg>(v), Ok(FrontMsg::Hello { session: None, .. })));
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
