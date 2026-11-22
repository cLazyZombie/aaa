//! 제어 plane 메시지. 모두 JSON이며 `t` 필드로 종류를 나눈다.
//!
//! - Agent link: host ↔ agent backend (`LinkMsg`)
//! - Session IPC: agent backend ↔ session host (`SessionMsg`)
//! - Front API: front(CLI·웹) ↔ host (`FrontMsg`, 웹은 `FrontRequest`를 HTTP로 투영)
//!
//! 모르는 선택 필드는 무시한다(serde 기본). 필수 필드가 없거나 모르는 메시지 종류는 오류다.

mod link;
pub use link::*;
mod view;
pub use view::*;
mod front;
pub use front::*;
mod term;
use std::collections::BTreeMap;
pub use term::*;

use base64::Engine as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

use crate::orch::{
    AgentStatus, Cleanup, FlowEvent, HarnessEvent, InboxItem, Launch, LibraryPrunePlan, LibraryRef, LibraryReport,
    LibrarySkip, LibraryView, LinkView, MessageMode, MessageView, OwnerChoice, ProfileView, ProjectAction,
    ProjectChange, ProjectContext, ProjectFilesCheck, ProjectOpened, ProjectView, QuestionView, ReportSnapshot, Role,
    RoutineView, ThreadAdopt, ThreadAdopted, ThreadKind, ThreadPlacement, ThreadPrepare, ThreadReady, ThreadView,
};
use crate::orch::{
    MenuPick, NoticeView, TaskBoard, TaskConflict, TaskLogEntry, TaskLogFilter, TaskOp, ThreadStartPreview,
    ThreadSummaryView,
};
use crate::{EvaluationView, EvaluatorView};

use crate::version::Version;

/// JSON에서 base64 문자열로 나가는 bytes.
#[derive(Debug, Clone, Default, PartialEq, Eq, TS)]
#[ts(type = "string")]
pub struct B64(pub Vec<u8>);

/// serde JSON의 wire 모양은 그대로 두고 가림 serializer에 바이트 타입을 알린다.
#[doc(hidden)]
pub const B64_SERDE_NAME: &str = "aaa.bytes.b64";

impl Serialize for B64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_newtype_struct(B64_SERDE_NAME, &base64::engine::general_purpose::STANDARD.encode(&self.0))
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
    /// 본 판과 지금 파일이 다르다(자동화 저장 등). 다시 읽고 차이를 본 뒤 고친다
    Conflict,
    /// 공개 주소로 연 조작 기기의 승인 필요 동작: 이 기기 passkey의 사용자 확인(UV)과 함께 다시 보낸다(관리 GUI W7).
    /// passkey가 없으면 message가 다시 짝짓기를 안내한다
    PasskeyRequired,
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
    /// macOS의 실제 session-host 책임 경로. agent가 재연결 때 조회하며 권한 등록 여부는 뜻하지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub responsibility: Option<Responsibility>,
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

/// PID의 부모 관계와 별개인 macOS 책임 프로세스 관측. 조회 실패를 정상으로 취급하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Responsibility {
    pub state: ResponsibilityState,
    pub pid: Option<u32>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ResponsibilityState {
    Launcher,
    Detached,
    Unknown,
}

/// 사람이 붙이는 pane 이름의 최대 글자 수(참고 herdr-web-ui `PaneRename` 입력 `maxLength={80}`)
pub const MAX_PANE_LABEL_CHARS: usize = 80;

/// Debug는 env 값을 찍지 않는다([`crate::EnvNames`]).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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
    /// host 정본 profile로 harness를 띄운다(argv는 무시하고 profile이 정한다, 상태 hook도 붙는다).
    /// agent가 `profile.v1` 기능을 알릴 때만 보낸다: 옛 agent는 이 칸을 버리고 셸을 띄운다(plan §8.126).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub profile: Option<String>,
    /// host_profiles.v1: host가 푼 정의. agent는 머신별 경로·env·hook만 적용한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(skip)]
    pub resolved_profile: Option<crate::ResolvedProfile>,
}

impl std::fmt::Debug for SpawnSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let SpawnSpec { name, argv, cwd, env, cols, rows, role, harness, profile, resolved_profile } = self;
        f.debug_struct("SpawnSpec")
            .field("name", name)
            .field("argv", argv)
            .field("cwd", cwd)
            .field("env", &crate::EnvNames::of(env.iter().map(|(k, _)| k)))
            .field("cols", cols)
            .field("rows", rows)
            .field("role", role)
            .field("harness", harness)
            .field("profile", profile)
            .field("resolved_profile", resolved_profile)
            .finish()
    }
}

/// 협상한 터미널 본문 codec. 수준은 wire에 넣지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TerminalCodec {
    Gzip,
    Zstd,
}

/// 협상한 네트워크 경계에서만 사용한다. IPC에는 보내지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TerminalCompression {
    pub codec: TerminalCodec,
    pub raw_len: u32,
}

// 필드 부재만 identity다. 명시 null/잘못된 metadata는 조용히 identity로 바꾸지 않는다.
fn terminal_compression<'de, D: Deserializer<'de>>(d: D) -> Result<Option<TerminalCompression>, D::Error> {
    TerminalCompression::deserialize(d).map(Some)
}

pub const TERMINAL_PAYLOAD_MAX: usize = 768 << 10;
pub const TERMINAL_ZSTD_FEATURE: &str = "terminal_payload.zstd.v1";

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
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "terminal_compression")]
    #[ts(optional)]
    pub compression: Option<TerminalCompression>,
    /// `replay`의 기록 앞에 session host가 가진 기록이 더 있다(plan §8.118). 그 앞은 `History` 요청으로 받는다.
    /// 옛 session host는 보내지 않는다(false: 묻지 않는다).
    #[serde(default)]
    pub history_more: bool,
}

/// 받은 기록 맨 위 줄들(`anchor`) 바로 앞 기록 한 덩어리(plan §8.67·§8.118). replay 기록과 같은 모양(색 포함)이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryChunk {
    pub data: B64,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "terminal_compression")]
    pub compression: Option<TerminalCompression>,
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
    /// harness 상태 감지가 읽는 글(`Vt::detection_text`, 화면 높이만큼). `pane read`만 받는다(herdr와 같다)
    Detection,
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
    /// 기록 파일/byte/블록 위치. 옛 agent는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub role: ChatRole,
    pub text: String,
    /// 사람 글이 harness 대기열에 있다(작업 중에 보내 아직 흡수되지 않음, Claude `queue-operation`). 기록 끝에 붙는다.
    /// 옛 agent는 보내지 않고 옛 front는 버린다(plan §8.247).
    #[serde(default)]
    pub pending: bool,
    /// 표시 상한으로 자르기 전의 사람 글. 옛 agent에는 없으므로 짧은 글 비교로 돌아간다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<crate::ChatReceipt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    /// 도구 호출 한 줄(예: `Bash: cargo test`). 도구 결과 본문은 싣지 않는다.
    Tool,
    /// agent의 생각(Claude thinking 블록, Codex reasoning 요약). 「생각 표시」를 켠 화면이 청할 때만 싣는다(plan §8.129).
    Thinking,
    /// 대화가 정리(compaction)된 자리(plan §8.309, herdr-web-ui `{ kind: "compact" }`). 글은 정리 요약이다(Codex는 빌 수 있다).
    /// host가 `AgentRequest::Chat.compact`로 청할 때만 싣는다: 옛 host는 이 역할을 읽지 못한다
    Compact,
    /// 사람이 친 harness 명령(Claude `<command-name>`, plan §8.323). 글은 `/이름 인자`다. `/compact`는 정리 자리가 대신한다.
    /// host가 `AgentRequest::Chat.commands`로 청할 때만 싣는다
    Command,
    /// harness 명령의 결과 글(Claude `local_command` 출력·`informational` 알림, 색 코드를 뺀 글). 바로 앞 `Command`의 결과다
    CommandOutput,
}

/// pane의 harness 명령 목록(plan §8.323). agent가 harness 버전에 묶인 내장 표와 그 머신의 명령·skill 파일로 만든다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CommandList {
    pub harness: String,
    /// 내장 표가 기준으로 삼은 harness 버전(`2.1.304`, `0.163`)
    pub builtin_version: String,
    pub commands: Vec<SlashCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SlashCommand {
    /// 트리거 뒤 이름(`compact`, `frontend:review`, `plugin:skill`)
    pub name: String,
    pub description: String,
    pub source: CommandSource,
    pub kind: CommandKind,
    /// 인자 안내(`[지시]`) 또는 선택창을 건너뛰는 꼴(`/model sonnet`)
    pub hint: Option<String>,
    /// 같은 명령의 다른 이름(Claude `/reset`·`/new`는 `/clear`)
    pub aliases: Vec<String>,
    /// 작업 중에는 목록에서 뺀다(harness가 막거나 턴 뒤로 미룬다)
    pub idle_only: bool,
    /// 고르면 입력칸에 넣을 글(`/compact `, Codex skill은 `$name `)
    pub insert: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CommandSource {
    Builtin,
    User,
    Project,
    Skill,
    Plugin,
}

/// 보낼 때 웹이 할 일
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    /// 그대로 보낸다(바로 끝나거나 모델이 일한다)
    Send,
    /// 인자 없이 보내면 터미널 선택창이 뜬다. aaa는 그것을 질문으로 보지 못한다: 보낸 뒤 「터미널 보기」를 알린다
    Picker,
    /// 결과가 터미널 화면에만 남는다(기록에 없다): 보낸 뒤 「터미널 보기」를 알린다
    Screen,
    /// 맥락을 비운다: 확인창으로 묻는다
    Confirm,
    /// harness를 끝내거나 되돌릴 수 없다: 목록에서 숨기고, 직접 쳐서 보내도 확인창으로 묻는다
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ChatView {
    pub pane: String,
    pub harness: String,
    pub messages: Vec<ChatMessage>,
    /// 기록이 더 길어 끝부분만 실었다
    pub truncated: bool,
    /// chat_pages.v1: 이전 쪽 cursor, null이면 처음. history가 없으면 옛 agent다.
    #[serde(default)]
    #[ts(optional)]
    pub before: Option<String>,
    /// 경로를 드러내지 않는 파일 정체성. append에는 같다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub history: Option<String>,
}

/// 파일 보기의 종류(herdr-web-ui `FileKind`, plan §8.125)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Image,
    Video,
    Audio,
    Pdf,
    Text,
    #[default]
    Binary,
}

impl FileKind {
    /// MIME으로 정하는 종류(agent 파일 보기와 host 결과물 목록이 같은 표를 쓴다)
    pub fn of_mime(mime: &str) -> FileKind {
        if mime.starts_with("image/") {
            FileKind::Image
        } else if mime.starts_with("video/") {
            FileKind::Video
        } else if mime.starts_with("audio/") {
            FileKind::Audio
        } else if mime == "application/pdf" {
            FileKind::Pdf
        } else if mime.starts_with("text/")
            || matches!(
                mime,
                "application/json"
                    | "application/xml"
                    | "application/yaml"
                    | "application/toml"
                    | "application/x-sh"
                    | "application/sql"
            )
        {
            FileKind::Text
        } else {
            FileKind::Binary
        }
    }
}

/// 확장자로 정하는 MIME(모르면 None: agent는 내용으로 본다)
pub fn mime_of(name: &str) -> Option<&'static str> {
    let (_, ext) = name.rsplit_once('.')?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "flac" => "audio/flac",
        "pdf" => "application/pdf",
        "html" | "htm" => "text/html",
        "md" | "markdown" => "text/markdown",
        "json" => "application/json",
        "js" | "mjs" | "cjs" => "text/javascript",
        "css" => "text/css",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "sh" => "application/x-sh",
        "sql" => "application/sql",
        "txt" | "log" | "rs" | "ts" | "tsx" | "jsx" | "py" | "go" | "rb" | "java" | "kt" | "swift" | "c" | "h"
        | "cc" | "cpp" | "hpp" | "cs" | "php" | "lua" | "zig" | "diff" | "patch" | "ini" | "conf" | "env" | "lock" => {
            "text/plain"
        }
        "zip" | "gz" | "tgz" | "tar" | "7z" | "bz2" | "xz" => "application/octet-stream",
        "wasm" | "so" | "dylib" | "dll" | "exe" | "bin" | "o" | "a" => "application/octet-stream",
        _ => return None,
    })
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

/// 폴더 보기의 폴더 한 단계(herdr-web-ui `DirectoryListing`). `path`는 agent 머신의 실제 경로다. `parent`는 허용 폴더 안일
/// 때만 있다. 폴더·파일은 각각 처음 500개까지(`truncated`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DirListing {
    pub path: String,
    pub parent: Option<String>,
    pub home: String,
    pub directories: Vec<String>,
    pub files: Vec<DirFile>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DirFile {
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
}

#[cfg(test)]
mod tests;
