//! 제어 plane 메시지. 모두 JSON이며 `t` 필드로 종류를 나눈다.
//!
//! - Agent link: host ↔ agent backend (`LinkMsg`)
//! - Session IPC: agent backend ↔ session host (`SessionMsg`)
//! - Front API: front(CLI·웹) ↔ host (`FrontMsg`, 웹은 `FrontRequest`를 HTTP로 투영)
//!
//! 모르는 선택 필드는 무시한다(serde 기본). 필수 필드가 없거나 모르는 메시지 종류는 오류다.

use std::collections::BTreeMap;

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
    /// 실행 파일 SHA-256(소문자 hex, 웹 머신 상세, plan §8.372). AGENT_LINK를 올리지 않는 선택 칸이다: 옛 agent는 보내지 않고 옛 host는 버린다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
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
    /// 대화를 이어 띄운다. host가 `harness_hello.v1`을 알리면 agent는 연결 뒤 이 스냅샷 이후 변경분만 보낸다.
    #[serde(default)]
    pub harness: Vec<HarnessEvent>,
    /// agent 실행 신원(관리 GUI W6c). 재시작만 하는 rollout이 재시작 전 신원으로 감시를 세운다. 옛 agent는 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<crate::RuntimeIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostWelcome {
    pub proto: Version,
    pub host_id: String,
    #[serde(default)]
    pub features: Vec<String>,
    #[serde(default)]
    pub identity: Option<crate::RuntimeIdentity>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnrollRequest {
    pub proto: Version,
    pub machine_id: String,
    pub machine_name: String,
    /// install_enroll.v1 협상 뒤만 사용한다. host의 사람이 SAS를 승인할 때 결과를 이 ID에 결속한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installation: Option<crate::install::InstallEnrollmentBinding>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum LinkMsg {
    Hello(AgentHello),
    Welcome(HostWelcome),
    Enroll(EnrollRequest),
    /// 등록 전 기능 협상. 구판에는 grant를 보내지 않는다.
    InstallEnrollCapabilities {
        proto: Version,
        features: Vec<String>,
    },
    InstallEnrollProbe {
        proto: Version,
    },
    InstallEnroll(Box<crate::install::InstallEnrollRequest>),
    InstallEnrollLookup {
        proto: Version,
        installation_id: String,
        plan_sha256: String,
    },
    /// 기존 agent 자기 신원만 읽는다. 설치 grant/등록창 권한을 쓰지 않는다.
    InstallAgentQuery {
        proto: Version,
        machine_id: String,
        machine_name: String,
    },
    InstallAgentKnown {
        machine_id: String,
    },
    InstallEnrollUnknown,
    InstallEnrolled {
        installation_id: String,
        machine_id: String,
    },
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
    /// response_parts.v1: 큰 응답 JSON을 순서대로 나눈다. 조립 후에도 Res와 같은 ID여야 한다.
    ResPart {
        id: String,
        offset: u32,
        total: u32,
        data: B64,
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
    /// machine_label.v1: host의 이름표를 agent 상태에 보관한다(세션은 유지).
    MachineLabel {
        name: String,
    },
    /// account_quota.v1: 해당 pane이 관측한 비밀 없는 창만 읽는다.
    AccountQuota {
        session: String,
        generation: String,
    },
    /// account_auth.v1. 비밀을 spec·journal에 넣지 않는 전용 spawn.
    SpawnAccount {
        spec: SpawnSpec,
        credential: crate::AccountCredential,
    },
    AccountRefreshed {
        session: String,
        result: crate::AccountRefreshed,
    },
    /// deploy_stage.v1: provenance가 있는 빌드 폴더의 aaa만 읽는다.
    DeployFetch {
        path: String,
        offset: u64,
        len: u32,
        sha256: String,
    },
    DeployStage {
        rollout: String,
        project: String,
        offset: u64,
        data: B64,
        eof: bool,
        sha256: String,
        total: u64,
    },
    DeployStageStatus {
        rollout: String,
    },
    /// deploy_candidate.v1: 빌드 폴더의 `aaa`와 옆 `provenance.json`을 대조하고 dry-run으로 self-check한다(관리 GUI W6a).
    /// 작업 기록·교체는 없다
    DeployCandidate {
        path: String,
    },
    DeployStageClean {
        rollout: String,
        keep: bool,
    },
    DeployGuardArm {
        spec: crate::DeployGuardSpec,
    },
    DeployGuardControl {
        id: String,
        control: crate::DeployGuardControl,
    },
    /// deployment.v1: host가 coordinator 출처와 safety를 확인한 요청만 보낸다.
    DeployStart {
        id: String,
        project: String,
        action: crate::DeployAction,
    },
    DeployStatus {
        id: String,
    },
    /// post.v1을 양쪽이 알렸을 때만 보낸다.
    PostAck {
        post_id: String,
        result: crate::PostResult,
    },
    /// imagegen.v1: 이미지 요청의 비밀 없는 응답.
    ImageReply {
        request_id: String,
        result: Outcome<crate::ImageResponse>,
    },
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
    /// 파일 보기: 세션 없이 정한 폴더(thread worktree) 기준으로 찾는다(보고서 안 파일 링크, plan §8.171). 찾는 규칙과 허용
    /// 폴더는 `FileLocate`와 같다. agent가 `file_in.v1` 기능을 알릴 때만 보낸다.
    FileLocateIn {
        dir: String,
        path: String,
    },
    /// 폴더 보기: 그 세션이 지금 있는 폴더 기준으로 폴더 한 단계를 읽는다(빈 경로·`~`는 집). 허용 폴더 밖이면 Forbidden,
    /// 폴더가 아니면 NotFound다. agent가 `dir.v1` 기능을 알릴 때만 보낸다.
    /// 입력칸 「/」 목록(plan §8.323): 그 harness의 내장 명령 표와 세션 폴더·사용자 폴더의 명령·skill 파일. agent가 `commands.v1`
    /// 기능을 알릴 때만 보낸다
    Commands {
        session: String,
        harness: String,
    },
    DirList {
        session: String,
        path: String,
        #[serde(default)]
        hidden: bool,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session: Option<String>,
        harness: String,
        /// Claude hook이 알린 기록 파일
        transcript: Option<String>,
        /// Codex thread-id(기록 파일 이름에 들어 있다)
        harness_session: Option<String>,
        /// 생각도 싣는다(plan §8.129). 옛 agent는 이 칸을 모르고 빼고 보낸다
        #[serde(default)]
        thinking: bool,
        /// 정리된 자리(`ChatRole::Compact`)도 싣는다(plan §8.309). 옛 host는 보내지 않아 agent가 빼고, 옛 agent는 이 칸을 모른다
        #[serde(default)]
        compact: bool,
        /// harness 명령과 결과(`ChatRole::Command`·`CommandOutput`, plan §8.323)도 싣는다. 옛 host는 보내지 않는다
        #[serde(default)]
        commands: bool,
        /// 이전 응답의 before. 생략하면 최신 쪽, 옛 상대는 무시한다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
    },
    /// 옛 host 호환: agent.toml에 정의된 profile 목록
    Profiles,
    /// host_profiles.v1: 정의를 제외한 설치·로그인한 기본 harness만 묻는다.
    Harnesses,
    /// thread 자리(worktree 또는 작업 폴더)를 만들고 brief.md를 쓰고 profile을 해석한다
    ThreadPrepare(ThreadPrepare),
    /// thread_journal.v1: 준비와 같은 lock에서 기록된 자리를 조회한다(새 자리는 만들지 않는다).
    ThreadPlacement {
        project: String,
        thread: String,
        title: String,
        kind: ThreadKind,
        repo: Option<String>,
    },
    /// 사람이 띄운 agent 세션을 thread로 데려온다: 그 세션의 지금 폴더에 작업 폴더·brief.md를 만든다(`thread adopt`).
    /// agent 기능 `adopt.v1`이 있을 때만 보낸다(옛 agent는 모르는 요청에 연결을 끊는다)
    ThreadAdopt(ThreadAdopt),
    /// report.md 변화를 `ReportChanged`로 알린다. 등록 즉시 지금 hash를 한 번 보낸다. `thread`는 `<project>/<thread>`다.
    ThreadWatch {
        thread: String,
        work_dir: String,
    },
    ThreadUnwatch {
        thread: String,
    },
    /// 대화 기록 파일 감시(차이 목록 C3, herdr-web-ui `conversation-watch`): 켜면 `Chat`과 같은 파일을 250ms마다 보고 바뀌면
    /// `ChatChanged`로 알린다. 켤 때 지금 값을 한 번 보낸다. `key`는 host가 정한 이름(pane ID)이고 같은 key를 다시 켜면 바꾼다.
    /// agent 기능 `chat_watch.v1`이 있을 때만 보낸다
    ChatWatch {
        key: String,
        harness: String,
        transcript: Option<String>,
        harness_session: Option<String>,
        enabled: bool,
    },
    /// report_hash.v1: library를 걷지 않고 report.md hash만 읽는다.
    ReportHash {
        work_dir: String,
    },
    /// report.md 본문과 library 목록. symlink는 따라가지 않고, 상한을 넘는 파일은 건너뛴다.
    ReportCollect {
        work_dir: String,
        max_library_bytes: u64,
    },
    /// report_copy.v2: 인증한 link에서 원본 무결성을 확인한 뒤 host 저장 경계에서 텍스트만 가린다.
    ReportTransfer {
        work_dir: String,
        max_library_bytes: u64,
    },
    ReportFileRead {
        work_dir: String,
        path: String,
        offset: u64,
        len: u32,
    },
    /// work_dir 아래 파일 한 조각(library 사본용)
    FileRead {
        work_dir: String,
        path: String,
        offset: u64,
        len: u32,
    },
    /// 새 대화 준비. host_profiles.v1이면 host가 푼 resolved_profile을 받고 머신 경로·env·hook을 적용한다.
    /// 옛 host는 profile 이름과 safety를 보내며, 둘 다 없으면 저장된 launch를 쓴다.
    ThreadRebrief {
        work_dir: String,
        brief: String,
        launch: Launch,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        profile: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resolved_profile: Option<crate::ResolvedProfile>,
        /// profile을 다시 풀 때 더할 safety(plan §8.163, `launch_safety.v1`을 알린 agent에만)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        safety: Option<crate::LaunchSafety>,
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
    /// task 첨부(plan §8.327): thread 작업 폴더 `attachments/`에 `AttachWrite`와 같은 꼴로 쓴다(같은 이름은 덮어쓴다). 답도 같다.
    /// agent 기능 `work_attach.v1`이 있을 때만 보낸다(옛 agent는 모르는 요청에 연결을 끊는다)
    WorkAttachWrite {
        work_dir: String,
        att: String,
        offset: u64,
        data: B64,
        eof: bool,
    },
    /// 화면의 터미널 기본 색이 바뀌었다(plan §8.187). agent는 모든 session host에 알리고 새 세션에 실은 뒤 답한다.
    /// agent 기능 `term_colors.v1`이 있을 때만 보낸다.
    TermColors(TermColors),
    /// 사후 평가자 사건 묶음을 작업 폴더의 `inbox/` 아래에 쓴다(§8.13). `path`는 `inbox/<이름>.json`만 받는다.
    /// mission.v1을 양쪽이 알린 연결에서만 임무 입력의 처리 여부를 읽는다.
    WorkFileInboxStatus {
        work_dir: String,
    },
    /// mission.v1: repo·machine 변경 때 상태 파일만 옮긴다. 본문은 모두 자료다.
    WorkFileList {
        work_dir: String,
    },
    WorkFileRead {
        work_dir: String,
        path: String,
        offset: u64,
    },
    WorkFileWriteChunk {
        work_dir: String,
        path: String,
        offset: u64,
        data: B64,
        eof: bool,
        sha256: String,
    },
    MissionInboxStatus {
        work_dir: String,
    },
    /// mission.v1: repo·machine 변경 때 상태 파일만 옮긴다. 본문은 모두 자료다.
    MissionStateList {
        work_dir: String,
    },
    MissionStateRead {
        work_dir: String,
        path: String,
        offset: u64,
    },
    MissionStateWrite {
        work_dir: String,
        path: String,
        offset: u64,
        data: B64,
        eof: bool,
        sha256: String,
    },
    WorkFileWrite {
        work_dir: String,
        path: String,
        data: B64,
    },
    /// cleanup_probe.v1 기능을 알린 agent에만 보낸다. 삭제하지 않고 입력 순서대로 부재를 확인한다(최대 500개).
    ThreadCleanupProbe {
        repo: String,
        targets: Vec<crate::CleanupTarget>,
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
        /// 마지막 사본에 집에 오지 않은 것이 있다(herdr-projects `clean` `copy_complete`, plan §8.135): 폴더가 남아 있으면
        /// worktree를 남긴다. 이 칸을 모르는 옛 agent도 지우지 않게 host는 `keep_worktree`를 함께 켠다(둘 다 켜져 있으면
        /// 사람의 「남기기」가 아니라 이것이다).
        #[serde(default)]
        uncopied: bool,
    },
    /// git.v1을 알린 repo 머신 agent만 origin head·변화 객체를 읽는다.
    /// workspace_metrics.v2: 등록한 main checkout의 push 확인 결과와 원격 신원을 회수한다.
    WorkspaceMetrics {
        cwd: String,
        workspace_key: String,
        known_code_commits: Vec<String>,
    },
    GitQuery {
        cwd: String,
        query: crate::GitQuery,
    },
    /// ci.v1을 알린 agent만 조건부 GitHub CI를 읽는다.
    CiQuery {
        cwd: String,
        query: crate::CiQuery,
    },
    /// pr.v1을 알린 agent만 thread origin의 PR을 gh로 읽는다.
    PrQuery {
        cwd: String,
        query: PrQuery,
    },
    /// diff.v1을 알린 agent만 thread worktree(`cwd`)의 바뀐 파일·patch를 읽는다(plan §8.305). `base`는 thread를 시작할 때
    /// 잡은 기준 ref(`origin/main` 등)이고, 없으면 agent가 `origin/HEAD`·`main`·`master`에서 고른다
    ThreadDiff {
        cwd: String,
        #[serde(default)]
        base: Option<String>,
        query: crate::DiffQuery,
    },
    /// work_root.manage.v1을 알린 agent만: 그 머신 작업 root 보기·점검·전환(관리 GUI W5c, `aaa work-root`와 같다)
    WorkRoot {
        action: crate::WorkRootOp,
    },
    /// hook.manage.v1을 알린 agent만: Codex 전역 hook 상태(`install` 없음)·설치(true)·제거(false). 고정 작업이라 다른 인자는 없다
    CodexHook {
        #[serde(default)]
        install: Option<bool>,
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
    /// PR 조회 생략 사유. gh 없음·인증 없음은 오류나 outage가 아니다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    /// 원격에 없는 branch·GitHub 밖 저장소여서 gh를 부르지 않았다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentResponse {
    WorkspaceMetrics(crate::WorkspaceCollected),
    AccountQuota {
        snapshot: Option<crate::QuotaSnapshot>,
    },
    DeployGuard {
        guard: Box<crate::DeployGuardView>,
    },
    DeployChunk {
        offset: u64,
        data: B64,
        eof: bool,
        total: u64,
    },
    DeployStage {
        stage: crate::DeployStageView,
    },
    DeployCandidate {
        sha256: String,
        commit: Option<String>,
        version: String,
        total_bytes: u64,
        built_at: Option<String>,
    },
    Deployment {
        job: crate::DeployJob,
        running_software: String,
        running_sha256: String,
        #[serde(default)]
        running_instance: String,
        #[serde(default)]
        running_started_ms: u64,
    },
    WorkFileInbox {
        pending: u32,
        names: Vec<String>,
    },
    WorkFiles {
        files: Vec<crate::LibFile>,
    },
    MissionInbox {
        pending: u32,
        names: Vec<String>,
    },
    MissionFiles {
        files: Vec<crate::LibFile>,
    },
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history: Option<String>,
    },
    Profiles {
        profiles: Vec<ProfileView>,
    },
    ThreadReady(ThreadReady),
    ThreadPlacement {
        placement: Option<ThreadPlacement>,
    },
    ThreadAdopted(ThreadAdopted),
    Report(ReportSnapshot),
    ReportHash {
        hash: String,
    },
    /// report_copy.v2 전용 전송 응답. 표시용 FileChunk와 구별한다.
    ReportTransfer(ReportSnapshot),
    ReportFileChunk {
        data: B64,
        eof: bool,
    },
    FileChunk {
        data: B64,
        eof: bool,
    },
    Cleanup(Cleanup),
    CleanupAbsent {
        absent: Vec<bool>,
    },
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
    /// 읽은 폴더 한 단계
    DirListed(DirListing),
    /// `AgentRequest::Commands`의 답
    Commands(CommandList),
    /// 찾은 파일. 없으면 끝이 같은 파일 후보(실제 경로, 둘 이상)
    FileLocated {
        info: Option<FileInfo>,
        candidates: Vec<String>,
    },
    Pr(PrAnswer),
    Ci(crate::CiAnswer),
    Git(crate::GitAnswer),
    History(HistoryChunk),
    Diff(crate::DiffView),
    WorkRoot(crate::WorkRootView),
    WorkRootChecked(crate::WorkRootCheckView),
    Hook(crate::HookView),
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
    /// imagegen.v1: agent가 peer PID·hook token을 확인한 자기 thread 요청.
    Image {
        request_id: String,
        session: String,
        request: crate::ImageRequest,
    },
    AccountRefresh(crate::AccountRefresh),
    DeploymentChanged {
        job: crate::DeployJob,
    },
    DeployGuardChanged {
        id: String,
    },
    /// post.v1: hook 판정에서 거부한 글의 메타데이터(본문·token 없음).
    PostRejected {
        session: String,
        post_id: String,
        bytes: u64,
        sha256: String,
        reason: String,
    },
    /// peer+token을 agent가 판정했다. 역할·프로젝트는 host가 정한다.
    Post {
        session: String,
        #[serde(flatten)]
        input: crate::PostInput,
    },
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
    /// `ChatWatch`로 감시하는 기록 파일이 바뀌었다. `signature`는 그 파일 상태의 뜻 없는 이름이다(경로를 싣지 않는다)
    ChatChanged {
        key: String,
        signature: String,
    },
}

// ───────────────────────── Session IPC ─────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum SessionMsg {
    AccountRefresh(crate::AccountRefresh),
    Hello {
        proto: Version,
        token: String,
    },
    Welcome {
        proto: Version,
        info: SessionInfo,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_rules: Option<crate::SecretText>,
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
    AccountRefreshed(crate::AccountRefreshed),
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
    /// 프로그램의 색 질의(OSC 10/11)에 답할 화면 색(SESSION_IPC 1.2부터, plan §8.187)
    Colors(TermColors),
}

/// 화면(front)의 터미널 기본 색. 프로그램의 색 질의(OSC 10/11)에 session host가 이 색으로 답한다(herdr 0.9.5 host terminal theme,
/// plan §8.187)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TermColors {
    /// 기본 글자 색 RGB
    pub fg: [u8; 3],
    /// 기본 바탕 색 RGB
    pub bg: [u8; 3],
}

impl TermColors {
    /// `rrggbb` 둘(웹 화면의 CSS token, 앞의 `#`은 있어도 된다)
    pub fn from_hex(fg: &str, bg: &str) -> Option<TermColors> {
        let rgb = |s: &str| -> Option<[u8; 3]> {
            let s = s.trim().trim_start_matches('#');
            if s.len() != 6 || !s.is_ascii() {
                return None;
            }
            let at = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
            Some([at(0)?, at(2)?, at(4)?])
        };
        Some(TermColors { fg: rgb(fg)?, bg: rgb(bg)? })
    }
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
    /// 마지막 PR 조회의 머신별 사용 상태. 조회 전·옛 host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pr_tracking: Option<String>,
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
    /// 마지막 hello의 CPU 종류(`aarch64`·`x86_64`). 아래 넷과 같이 1.88부터 싣고, 옛 host·아직 붙지 않은 머신은 생략한다(plan §8.372)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub arch: Option<String>,
    /// 마지막 hello의 aaa 판(`aaa --version`의 판 글)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub software: Option<String>,
    /// 마지막 hello의 실행 파일 SHA-256(소문자 hex). 옛 agent는 보내지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sha256: Option<String>,
    /// 판이 host와 다르다(판을 모르면 false)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<bool>")]
    pub software_differs: bool,
    /// 이 host 실행 중 마지막 연결이 알린 agent 기능(host 재시작 뒤 다시 붙기 전에는 비었다)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub features: Vec<String>,
    /// 마지막 연결의 시작 자가 점검(§10.4 6) 요약. 점검이 hello보다 늦게 끝났으면 다음 연결 때 온다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub selfcheck: Option<MachineSelfcheck>,
}

/// agent 시작 자가 점검 요약(hello `selfcheck`에서 host가 읽은 것). 실패 이유는 agent가 쓴 글 그대로다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSelfcheck {
    pub pass: bool,
    #[serde(default)]
    pub reasons: Vec<String>,
}

/// agent가 주기적으로 보내는 머신 건강(§8.10)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineHealth {
    /// 표시 전용 시스템 정보. 옛 agent는 생략하며 판 조회 실패로 세션을 막지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub info: Option<MachineInfo>,
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
    /// 직전 표본부터의 CPU 사용률(0~100, 모든 코어 평균). 옛 agent는 보내지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cpu_pct: Option<u8>,
    /// 사용 메모리와 전체 바이트. 사용 = macOS 앱(internal−purgeable)+wired+압축, Linux 전체−MemAvailable, Windows 전체−ullAvailPhys
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub mem_used_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub mem_total_bytes: Option<u64>,
    /// 전원에 연결됐는가(배터리가 없는 머신은 true, 알 수 없으면 None)
    pub on_ac: Option<bool>,
    pub toolchain: Vec<ToolCheck>,
    /// agent가 본 이상(디스크 부족·toolchain 실패). 새 세션을 거부한다.
    pub problems: Vec<HealthProblem>,
    /// 거부하지는 않는 주의(배터리 전원 등)
    pub warnings: Vec<String>,
}

/// agent가 10분마다 수집한 표시 전용 정보. 실패한 값은 None이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineInfo {
    #[ts(type = "number")]
    pub at_ms: u64,
    pub os_version: Option<String>,
    pub cpu_model: Option<String>,
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub installation: Option<String>,
    pub work_root: String,
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
    /// 사람이 읽는 이름(예: `웹(ab12)`)
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PaneView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub responsibility: Option<Responsibility>,
    /// host에 고정한 인증 별칭. 이전 판 thread의 인증은 ThreadView.account로 확인한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account: Option<String>,
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
    /// host 머신의 프로젝트 홈에서 도는 coordinator의 소속. host가 canonical cwd로 판정한다.
    /// 옛 host는 보내지 않으며 front는 이름·경로로 소속을 추측하지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub coordinator_project: Option<String>,
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
    /// 이 pane이 대신한 옛 pane ID(`pane resume`·재부팅 복원은 새 세션을 띄우고 옛 기록을 지운다, 오래된 것부터). 옛 ID를 보던
    /// 화면은 이것으로 새 pane을 따라간다(QA Q19).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub replaced: Vec<String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PendingEnroll {
    pub name: String,
    pub machine_id: String,
    /// 상대 인증서 fingerprint 앞부분(참고용). 확인 코드는 agent 화면에만 보인다.
    pub fingerprint_prefix: String,
    #[ts(type = "number")]
    pub since_ms: u64,
}

/// SSH 머신 추가 작업 하나(웹 머신 추가 M2, plan §8.384). host DB에 남아 웹 창을 닫았다 열어도 같은 작업을 본다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSetupView {
    pub job: String,
    /// 사람이 넣은 SSH 대상(별칭 또는 `user@host:port`)
    pub target: String,
    pub phase: MachineSetupPhase,
    /// `ssh -G`로 푼 접속(사용자·주소·포트, ProxyJump를 거치는가)
    pub ssh: Option<SshResolved>,
    /// 접속 때 받은 host key(인증 전). 사람이 지문을 보고 믿는다
    pub host_keys: Vec<HostKeyView>,
    /// 받은 키가 이미 믿는 키인가: `known` | `new` | `changed`(fail-closed)
    pub host_key_state: Option<String>,
    pub detected: Option<MachineDetected>,
    /// 실패·멈춤의 까닭(사람이 읽는 글)
    pub error: Option<String>,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
    /// 사람이 넣은 설치 설정(M3, plan §8.420). 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub settings: Option<MachineSetupSettings>,
    /// 원격 install이 낸 계획 요약(사람이 확인한다). 경로·계정이 든 계획 원문은 host 작업 폴더에만 둔다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<MachineSetupReview>")]
    pub review: Option<Box<MachineSetupReview>>,
    /// 원격 install 적용의 진행과 결과
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, as = "Option<MachineSetupInstall>")]
    pub install: Option<Box<MachineSetupInstall>>,
}

/// 새 머신의 install 설정(웹 폼). host가 install choices(role agent·등록 grant)로 옮긴다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSetupSettings {
    /// 새 머신 이름(host 규칙, 이미 있는 이름은 거부)
    pub name: String,
    /// 새 머신이 닿을 host agent 연결 주소(`enroll_guide` 후보 중 하나)
    pub host: String,
    /// 작업 root(worktree 위치, 원격 절대 경로)
    pub work_root: String,
    /// 작업 root가 있는 외장 디스크 mount(비우면 내장 디스크)
    pub work_mount: Option<String>,
    /// 그 머신에서 쓸 harness(비우면 none)
    pub harness: Vec<crate::install::InstallHarness>,
    pub login: crate::install::InstallLogin,
}

/// 원격 `install-probe`가 낸 계획의 공개 요약
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSetupReview {
    /// 사람이 확인하는 계획 hash(적용 요청에 그대로 실어 보낸다)
    pub plan_sha256: String,
    /// 보낸 aaa 실행 파일(원격에서 같은 SHA·판을 확인했다)
    pub binary_sha256: String,
    pub binary_version: String,
    /// 바꿀 것·검사·사람이 할 일(install 공개 결과 그대로)
    pub changes: Vec<crate::install::InstallChange>,
    pub checks: Vec<crate::install::InstallCheck>,
    pub needs_user: Vec<crate::install::InstallNeedsUser>,
    /// 승인 범위(operation 이름만, 원격 경로는 싣지 않는다)
    pub operations: Vec<String>,
    /// Windows 서비스 암호의 전용 가림 입력을 보인다. 값은 이 요약·작업에 절대 싣지 않는다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<bool>")]
    pub service_password: bool,
}

/// 웹 확인 창 → host 메모리 → SSH stdin 전용. Debug는 가리고 사용한 소유 버퍼는 지운다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(type = "string")]
pub struct MachineSetupPassword(pub String);

impl std::fmt::Debug for MachineSetupPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[비밀]")
    }
}

impl Drop for MachineSetupPassword {
    fn drop(&mut self) {
        // SAFETY: 소유한 UTF-8 저장소를 지운 뒤 문자열을 더 읽지 않는다.
        for byte in unsafe { self.0.as_bytes_mut() } {
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineSetupInstall {
    pub installation_id: Option<String>,
    /// 마지막 진행 사건(사람이 읽는 글)
    pub last_event: Option<String>,
    /// 끝난 install 결과(공개 envelope)
    pub result: Option<crate::install::InstallResult>,
    /// 등록된 머신 이름(grant 등록 뒤)
    pub machine: Option<String>,
    /// 끝 확인 shell pane이 적은 줄(작업 폴더·git·harness·PATH)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub checks: Vec<String>,
    /// 끝 확인 pane(사람이 열어 본다)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub check_pane: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MachineSetupPhase {
    /// 접속해 host key를 받는 중
    Connecting,
    /// 처음 보는 host key: 사람이 지문을 확인한다
    Trust,
    /// 믿는 키로 로그인해 OS·CPU를 읽는 중
    Detecting,
    /// 감지가 안 됐다: 사람이 OS·CPU를 고른다
    Choose,
    /// 감지(또는 고름) 끝, 지원하는 대상이다. 사람이 설치 설정을 넣는다
    Ready,
    /// 감지했지만 release 대상이 없다(예: Windows ARM64)
    Unsupported,
    Failed,
    /// aaa를 보내 확인하고 원격 install이 계획을 낸다(M3)
    Planning,
    /// 계획이 나왔다: 사람이 바꿀 것을 보고 「설치하고 등록」을 누른다
    Review,
    /// 서명 승인 → 원격 install 적용·등록 중
    Installing,
    /// install이 사람 일을 남겼다(그 머신에서 하고 새로 시작한다)
    NeedsUser,
    /// 설치·등록 끝
    Installed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SshResolved {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub proxy_jump: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HostKeyView {
    /// `ssh-ed25519` 등
    pub kind: String,
    /// `SHA256:<base64>`(ssh-keygen -l와 같은 꼴)
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MachineDetected {
    /// `macos` | `linux` | `windows`(MachineView.os와 같은 말)
    pub os: String,
    /// `aarch64` | `x86_64`
    pub arch: String,
    /// 원격이 말한 그대로(`Darwin arm64`, `Windows AMD64`). 사람이 고른 것이면 빈 글
    pub raw: String,
    /// 사람이 골랐다(감지 실패)
    pub chosen: bool,
    pub home: Option<String>,
    pub shell: Option<String>,
    /// 원격에 이미 있는 aaa 판(`aaa --version` 첫 줄)
    pub aaa: Option<String>,
}

/// 「직접 설치」 안내(plan §8.383): 새 머신이 host agent 연결에 닿을 주소 후보와, 새 머신 화면에 나올 host 지문
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EnrollGuide {
    /// host가 실제로 연 agent 연결 주소(`agent_listen`). 닫혀 있으면 None이고 새 머신은 붙지 못한다
    pub listen: Option<String>,
    /// 새 머신에서 칠 `<주소>:<포트>` 후보. 앞이 추천이다. 빈 목록이면 host 밖에서 닿을 주소가 없다(닫힘·loopback)
    pub addresses: Vec<EnrollAddress>,
    /// host 인증서 SHA-256 전체(소문자 hex). 새 머신 `aaa agent enroll` 화면은 앞 16자를 보인다
    pub host_fingerprint: String,
    /// host 판(`aaa --version`). 새 머신에 같은 판을 넣는다
    pub software: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EnrollAddress {
    pub address: String,
    /// 어디서 온 후보인가: `tailnet`(tailscale 이름)·`listen`(연 주소 그대로)·`hostname`(이 머신 이름)
    pub source: String,
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
    /// 지금 공개 주소의 passkey가 있다(관리 GUI W7). 옛 host는 보내지 않는다
    #[serde(default)]
    pub passkey: bool,
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
    /// 저장한 지표만 읽는다. agent 호출·로그 읽기 없음.
    WorkspaceDashboard {
        query: crate::WorkspaceQuery,
    },
    /// 설치 전용 서명 승인. Controller 또는 세션 밖 Local만, raw 승인서는 응답하지 않는다.
    InstallApprove {
        request: Box<crate::install::InstallApprovalRequest>,
    },
    /// 등록은 host의 세션 밖 Local만. 비밀값은 반환하거나 감사 기록에 넣지 않는다.
    AccountImport {
        name: String,
        harness: String,
        secret: crate::SecretText,
    },
    AccountList,
    AccountDefaults,
    AccountPool,
    AccountPoolMember {
        revision: String,
        account: String,
        member: Option<crate::AccountPoolMember>,
    },
    AccountPoolProject {
        revision: String,
        project: String,
        enabled: bool,
    },
    AccountStarts {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    AccountStartCancel {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
    },
    /// 별칭 `auto`와 충돌하지 않는 명시적 풀 시작. 같은 ID의 재요청은 같은 시작이다.
    ThreadStartPool {
        id: String,
        size: crate::AccountTaskSize,
        start: Box<FrontRequest>,
    },
    AccountDefault {
        harness: String,
        account: Option<String>,
    },
    AccountLogin {
        name: String,
        harness: String,
    },
    AccountLoginStatus {
        id: String,
    },
    AccountLoginCancel {
        id: String,
    },
    /// FRONT_API 1.79. 다시 로그인에 다른 메일이 들어왔을 때(`mismatch`) 고른다: `switch`면 이 별칭을 새 메일 인증으로 바꾸고,
    /// 아니면 새 인증을 버리고 옛 인증을 남긴다(`kept`). 조작 기기와 host의 세션 밖 CLI만
    AccountLoginConfirm {
        id: String,
        switch: bool,
    },
    /// 재로그인 없이 같은 harness의 별칭과 참조를 옮긴다. 사람 전용(FRONT_API 1.128).
    AccountRename {
        name: String,
        to: String,
    },
    AccountRemove {
        name: String,
    },
    /// 옛 host가 account 필드를 버리고 기본 인증으로 실행하지 않게 별도 요청을 쓴다.
    ThreadStartAccount {
        account: String,
        start: Box<FrontRequest>,
    },
    /// 로컬 입력 진단. 본문·화면을 담지 않는다.
    DebugInput {
        pane: Option<String>,
        thread: Option<String>,
        since_ms: u64,
    },
    RolloutStart {
        id: String,
        spec: crate::RolloutSpec,
    },
    RolloutStatus {
        id: String,
    },
    DeployContext,
    DeployStart {
        machine: Option<String>,
        id: String,
        action: crate::DeployAction,
    },
    DeployStatus {
        machine: Option<String>,
        id: String,
    },
    /// FRONT_API 1.82. 웹 「배포」의 시작 전 점검: 빌드 머신 후보의 provenance·SHA·dry-run self-check와 대상(host 먼저)
    RolloutPlan {
        source_machine: String,
        path: String,
        /// 비었으면 등록된 모든 머신
        #[serde(default)]
        machines: Vec<String>,
    },
    /// FRONT_API 1.82. 이 프로젝트의 최근 rollout 요약(새것부터)
    Rollouts,
    /// FRONT_API 1.82. 이 프로젝트의 최근 개별 배포 작업(재시작) 요약(새것부터)
    DeployJobs,
    Post {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        input: crate::PostInput,
    },
    PostStatus {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        post_id: String,
    },
    /// 사용자 답을 받은 확인 필요 post를 처리됨으로 표시한다. 질문 선택·승인·실행은 하지 않는다.
    PostHandled {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        post_id: String,
    },
    PostList {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 「답할 것」(FRONT_API 1.78, plan §8.345, 보기 권한): 열린 coordinator 확인 필요 post와 답한 것 기록
    Asks {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// coordinator 확인 필요 post를 「답함으로 표시」한다: 그 inbox 항목을 끝내고 기록에 남긴다
    AskHandle {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        item: String,
    },
    /// coordinator post의 구조화 질문에 답하고 요청을 닫는다(FRONT_API 1.87).
    AskAnswer {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        item: String,
        body_hash: String,
        picks: Vec<Option<u32>>,
        text: Option<String>,
    },
    /// coordinator 모드 상태. 읽기는 모든 front, 변경은 사람만.
    AutoDecisions {
        #[serde(default)]
        project: String,
        #[serde(default)]
        before: Option<String>,
        #[serde(default)]
        id: Option<String>,
    },
    AutoModeGet {
        #[serde(default)]
        project: String,
    },
    AutoModeSet {
        #[serde(default)]
        project: String,
        on: bool,
    },
    /// 검증한 coordinator만 자동 답을 보낸다. 일반 답과 별도 op라 구판 host에서 사람 답으로 강등되지 않는다.
    QuestionAutoAnswer {
        #[serde(default)]
        project: String,
        id: String,
        choice: Option<String>,
        text: Option<String>,
        reason: String,
    },
    /// 확인 처리 또는 새 답 전달. null은 확인, 글은 coordinator에 재판단 요청(되돌림 실행은 하지 않는다).
    AutoDecisionReview {
        #[serde(default)]
        project: String,
        id: String,
        answer: Option<String>,
    },
    /// 사람이 읽는 질문 ID로 선택·직접 답을 기록한다. 일부만 답하면 나머지는 열린 채로 남는다.
    QuestionReply {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
        choice: Option<String>,
        text: Option<String>,
    },
    /// 답한 것 기록을 지운다(모든 기기). `all`이면 모두
    AskForget {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    PostUnmute {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
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
        /// host 정본 profile로 harness를 띄운다(`argv`는 비운다, herdr-web-ui 새 세션의 agent 고르기)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        profile: Option<String>,
    },
    PaneRead {
        pane: String,
    },
    /// harness(Claude·Codex) 대화 기록 보기(§8.27, 읽기 전용)
    PaneChat {
        pane: String,
        /// 생각도 싣는다(웹 설정 「생각 표시」, plan §8.129)
        #[serde(default)]
        thinking: bool,
        /// 정리된 자리(`ChatRole::Compact`, plan §8.309, 1.61)도 싣는다. 옛 front는 생략해 그 줄을 받지 않는다(모르는 role로 답 전체를
        /// 읽지 못하므로)
        #[serde(default)]
        compact: bool,
        /// harness 명령과 결과(`ChatRole::Command`·`CommandOutput`, plan §8.323, 1.68)도 싣는다. 옛 front는 생략한다
        #[serde(default)]
        commands: bool,
        /// 이전 응답의 before. 생략하면 최신 쪽, 옛 상대는 무시한다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        before: Option<String>,
    },
    /// 입력칸 「/」 목록(plan §8.323): 그 pane harness의 명령. agent가 `commands.v1`을 알릴 때만, 조작 기기만
    PaneCommands {
        pane: String,
    },
    /// 파일 보기(plan §8.125, 결정 20 D7): pane이 지금 있는 폴더 기준으로 경로를 찾고, 찾았으면 짧은 수명의
    /// 읽기 경로(`url`)를 준다. 조작 기기만(참고도 보기 전용 기기는 403).
    PaneFile {
        pane: String,
        path: String,
    },
    /// 보고서 안 파일 링크(plan §8.171): 그 thread의 머신에서 thread 작업 폴더(worktree) 기준으로 찾는다. 답은 `PaneFile`과
    /// 같다. 조작 기기만(`PaneFile`과 같다)
    ThreadFile {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        path: String,
    },
    /// FRONT_API 1.59. thread worktree의 바뀐 파일과 patch(plan §8.305): 그 thread의 머신 agent가 읽는다(diff.v1). 소스를
    /// 보이므로 조작 기기만(`ThreadFile`과 같다)
    ThreadDiff {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        #[ts(optional, as = "Option<crate::DiffQuery>")]
        query: crate::DiffQuery,
    },
    /// 폴더 보기(파일 보기 안, 참고 `DirectoryBrowser`): pane이 지금 있는 폴더 기준으로 폴더 한 단계를 읽는다. 허용 폴더 안만,
    /// 조작 기기만(`PaneFile`과 같다)
    PaneDir {
        pane: String,
        path: String,
        #[serde(default)]
        hidden: bool,
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
    /// 대화형 입력(웹 터미널). 조작 lease를 확인하고, 비어 있으면 이 연결이 갖는다.
    /// 한 번 보내는 `PaneSend`(CLI·API)는 lease 대상이 아니다(§4.5).
    PaneInput {
        pane: String,
        data: B64,
    },
    /// 웹의 Ctrl+Enter. host가 감지 입력줄로 steering 키를 정한다(줄바꿈 없음, 작업 중일 때만, 모르면 보내지 않음). 권한·입력 순서는 PaneInput과 같다.
    PaneCtrlEnter {
        pane: String,
        #[serde(default)]
        trace: Option<String>,
    },
    /// 대화형 붙여넣기(herdr client가 서버로 넘기는 붙여넣기와 같은 형식).
    /// host가 그 프로그램이 켠 bracketed paste대로 감싸고 ESC·C1 CSI를 뺀다(`KeyModes::paste`). lease는 `PaneInput`과 같다.
    PanePaste {
        pane: String,
        text: String,
    },
    /// 조작 lease를 이 연결로 가져온다(이전 보유자에게는 State로 알린다)
    PaneTake {
        pane: String,
    },
    PaneRelease {
        pane: String,
    },
    /// 붙여 넣은 그림 한 조각(`aaa pane attach`, 최대 512KiB). 웹 `TermClientMsg::Attach`와 같은 규칙이다:
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
    /// FRONT_API 1.80. 등록 창을 시한 전에 닫는다(대기 중인 요청은 그대로, 새 요청만 막는다)
    EnrollClose,
    /// FRONT_API 1.81(관리 GUI W5c). safety 실효값과 출처. `scope`가 global이면 모든 프로젝트 기본 표
    SafetyShow {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        scope: crate::SettingsScope,
    },
    /// FRONT_API 1.81. safety 값 하나를 바꾼다(웹은 yolo·start_threads·trust_screens·coordinator_deploy만). `value`가 없으면
    /// 그 표의 값을 지워 아래 층을 따른다. `expected`는 읽은 host.toml 판
    SafetySet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        scope: crate::SettingsScope,
        key: String,
        value: Option<String>,
        expected: String,
    },
    /// FRONT_API 1.81. host.toml profile 목록(env는 이름만)과 판
    ProfileAdmin,
    /// FRONT_API 1.81. profile·허용 목록·기본값 바꾸기 미리보기(쓰지 않는다)
    ProfilePlan {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        change: crate::ProfileChangeReq,
    },
    /// FRONT_API 1.81. 미리 본 바꾸기를 쓴다. `expected`와 host.toml 판이 다르면 conflict
    ProfileApply {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        change: crate::ProfileChangeReq,
        expected: String,
    },
    /// FRONT_API 1.81. 머신 작업 root 보기(그 머신 agent의 work_root.manage.v1)
    WorkRootShow {
        machine: String,
    },
    /// FRONT_API 1.81. 작업 root 점검(검사 파일을 만들고 쓰기·읽기·삭제, 없는 root는 만든다). `path`가 없으면 지금 설정
    WorkRootCheck {
        machine: String,
        path: Option<String>,
        mount: Option<String>,
    },
    /// FRONT_API 1.81. 다음 agent 시작의 작업 root를 바꾼다(절대 경로, agent가 다시 점검한 뒤 저장)
    WorkRootSet {
        machine: String,
        path: String,
        mount: Option<String>,
    },
    /// FRONT_API 1.81. 그 머신 harness hook 연동 상태
    HookStatus {
        machine: String,
    },
    /// FRONT_API 1.81. 그 머신 agent의 고정 작업으로 Codex 전역 hook을 설치(true)·제거(false)한다
    HookSet {
        machine: String,
        install: bool,
    },
    EnrollPending,
    /// FRONT_API 1.93. SSH로 머신 추가 작업을 시작한다(웹 머신 추가 M2, plan §8.384): host의 OpenSSH로 접속해 host key만 받고, 아는 키면
    /// OS·CPU를 감지한다. 원격에 쓰지 않는다. 사람만(조작 기기·세션 밖)
    MachineSetupStart {
        target: String,
    },
    /// FRONT_API 1.93. 화면에서 본 host key 지문을 믿는다: host가 받은 키와 같으면 aaa 전용 known_hosts에 넣고 감지로 간다. 사람만
    MachineSetupTrust {
        job: String,
        host_key_sha256: String,
    },
    /// FRONT_API 1.93. 감지가 안 된 머신의 OS·CPU를 사람이 고른다(release 대상 다섯만). 사람만
    MachineSetupChoose {
        job: String,
        os: String,
        arch: String,
    },
    /// FRONT_API 1.93. 작업을 멈추고 목록에서 뺀다(원격 임시 폴더가 있으면 지운다). 사람만
    MachineSetupCancel {
        job: String,
    },
    /// FRONT_API 1.103. 준비된 작업에 설치 설정을 넣는다(M3, plan §8.420): host가 같은 판 aaa를 원격 임시 폴더에 보내 SHA·판을 확인하고
    /// 원격 `install-probe`로 계획을 받는다. 원격 설치 자리에는 아직 쓰지 않는다. 사람만
    MachineSetupPlan {
        job: String,
        settings: crate::MachineSetupSettings,
    },
    /// FRONT_API 1.103. 화면에서 본 계획 hash로 설치하고 등록한다: host가 그 계획에 서명하고, 원격 install이 적용하며 grant로 등록한다. 사람만
    MachineSetupApply {
        job: String,
        plan_sha256: String,
        /// FRONT_API 1.112. 사람의 웹 확인 창에서만 일회 전달한다. CLI·세션·저장·응답 금지.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        service_password: Option<MachineSetupPassword>,
    },
    /// FRONT_API 1.103. 설치·등록된 머신에 끝 확인 shell pane을 연다(작업 폴더·git·harness·PATH를 적는다, 모델 비용 없음). 사람만
    MachineSetupCheck {
        job: String,
    },
    /// FRONT_API 1.103. 설치·등록된 머신에 harness 로그인 pane(`claude`·`codex login`)을 연다. 사람만
    MachineSetupLogin {
        job: String,
        harness: crate::install::InstallHarness,
    },
    /// FRONT_API 1.93. 최근 SSH 추가 작업(Reader도 본다)
    MachineSetupList,
    /// FRONT_API 1.92. 새 머신에서 칠 「직접 설치」 명령에 넣을 host 주소 후보·host 지문·판(웹 머신 추가 M1, plan §8.383)
    EnrollGuide,
    EnrollApprove {
        name: String,
        code: String,
    },
    /// 등록 신원·연결을 유지하고 머신 이름과 참조를 함께 옮긴다.
    MachineRename {
        name: String,
        to: String,
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
    /// W0와 별개로, 실제 승인된 같은 머신 worker의 한 포트 범위만 검증한다.
    InstallPublish {
        installation_id: String,
    },
    InstallPublishStatus {
        installation_id: String,
    },
    WebPublish,
    /// 공개 포트를 바꾸고 저장한 링크와 푸시 구독을 이전한다(사람의 host CLI).
    WebPublishPort {
        port: u16,
    },
    WebPublishOff,
    WebStatus,
    /// 프로젝트 폴더를 만든다(herdr-projects `new`). 준 값만 project.toml에 적는다(FRONT_API 1.33)
    ProjectCreate {
        name: String,
        #[serde(default)]
        goal: String,
        /// `PATH` 또는 `PATH@MACHINE`(머신이 없으면 host 머신, 상대 경로는 CLI가 절대 경로로 바꿔 보낸다)
        #[serde(default)]
        repos: Vec<String>,
        #[serde(default)]
        thread_profile: Option<String>,
        #[serde(default)]
        coordinator_profile: Option<String>,
    },
    /// project.toml 값 하나를 바꾼다(herdr-projects `set`, FRONT_API 1.33). key는 `aaa project set --help`
    ProjectSet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        key: String,
        value: String,
    },
    /// 프로젝트 목록. archived는 `all`일 때만(herdr-projects `list --all`, FRONT_API 1.33)
    Projects {
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        all: bool,
    },
    /// 이 host가 맡은 프로젝트 하나(보관하지 않은 것, FRONT_API 1.50, plan §8.288). 아직 없으면 `project`가 없다
    Instance,
    /// 멈추기·되살리기·보관·보관 풀기(herdr-projects `pause`·`resume`·`archive`·`unarchive`, FRONT_API 1.33)
    ProjectLifecycle {
        project: String,
        action: ProjectAction,
    },
    /// 프로젝트 폴더를 휴지통(`<projects root>/.trash`)으로 옮기고 host 기록을 그 안 `aaa-records.json`으로 뺀다.
    /// worktree·branch·PR은 건드리지 않는다(herdr-projects `delete`)
    ProjectDelete {
        project: String,
        /// 살아 있는 pane이 있어도
        #[serde(default)]
        force: bool,
    },
    /// 프로젝트 이름(폴더 이름)을 바꾼다(herdr-projects `rename`). 끝나지 않은 thread나 도는 coordinator가 있으면 거부한다
    ProjectRename {
        project: String,
        to: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// 끝난 thread가 남긴 worktree·branch를 그 머신 agent가 다시 정리한다(herdr-projects `sweep`)
    ProjectSweep {
        project: String,
        #[serde(default)]
        dry_run: bool,
    },
    /// 모든 프로젝트 폴더의 aaa 파일(coordinator 안내 AGENTS.md·uploads/·routines/pr-followup.md)을 본다.
    /// `fix`면 고친다: 표지 있는 AGENTS.md는 지금 안내로 다시 쓰고, 표지 없는 것은 `.before-aaa`로 옮겨 둔다
    /// (herdr-projects `doctor [--fix]`, FRONT_API 1.34)
    ProjectFiles {
        #[serde(default)]
        fix: bool,
    },
    /// coordinator를 연다(herdr-projects `open`): 도는 coordinator가 있으면 그것을 알려 주고, 없으면(또는 `new`) host 머신에서
    /// 프로젝트 폴더를 작업 폴더로 profile을 띄워 프로젝트 이름 workspace의 새 탭에 둔다(FRONT_API 1.33)
    ProjectOpen {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        /// 안 주면 project.toml `coordinator_profile` → host.toml `[defaults] coordinator_profile` → claude
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        profile: Option<String>,
        /// coordinator가 돌아도 하나 더 띄운다
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        new: bool,
    },
    /// host 정본 profile과 그 머신에 설치·로그인한 기본 harness 목록
    Profiles {
        machine: Option<String>,
    },
    /// host 정본의 profile을 확인한다(설치 여부와 무관, 기본값도 host에서 고른다).
    ProfileResolve {
        name: Option<String>,
    },
    ThreadStart {
        /// FRONT_API 1.98. 사용자가 명시한 시작만 프로젝트 실행 한도를 넘길 수 있다.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        over_limit: bool,
        /// coordinator는 해당 task를 강제로 시작하라는 사용자 지시 근거를 한 줄로 남긴다.
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        user_request: Option<String>,
        #[serde(default)]
        holds: Vec<String>,
        /// 준비 완료를 기다리지 않고 Starting 기록을 돌려준다.
        #[serde(default)]
        background: bool,
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
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
        /// TASKS.md의 task ID(d-0001·ULID) 또는 제목(대소문자 무시). owner가 profile·machine의 기본값이 되고, 메모가 할 일에 붙는다.
        #[serde(default)]
        from_task: Option<String>,
        /// 호환 옵션(FRONT_API 1.63). 일반 기동은 생략해도 할 일을 먼저 기록하고 연결한다(plan §8.430).
        /// from_task는 읽은 블록 hash로 연결하며, 달라졌으면 pane을 띄우지 않는다. 자동화 do.start는 제외한다.
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        link_task: bool,
        /// 화면이 본 `from_task`의 블록 hash(FRONT_API 1.63). 지금 파일의 블록과 다르면 시작하지 않는다(본 것과 다른 일을 맡기지 않게)
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        task_block: Option<String>,
    },
    /// 할 일 「바로 시작」 확인창이 보일 것(FRONT_API 1.63, 조작 권한): `from_task`로 시작하면 쓸 제목·profile·머신·할 일 글과
    /// 저장소 고르기, 열린 thread 수와 한도. 아무것도 시작하지 않는다
    ThreadStartPreview {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        from_task: String,
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        task_block: Option<String>,
    },
    /// 사람이 이미 띄운 agent pane(Claude·Codex)을 이 프로젝트의 thread로 기록하고 brief를 안내한다(herdr-projects
    /// `thread adopt`). pane은 그대로 두고 그 폴더에 작업 폴더를 만든다
    ThreadAdopt {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        pane: String,
        title: String,
        /// 할 일. 비면 「하던 일을 이어 하되 이제 이 프로젝트 thread다」
        #[serde(default)]
        task: String,
    },
    /// thread의 Next 목록을 보이거나(`line`·`add` 없음), N번째 줄을 thread에 메시지로 보내거나, 줄을 더한다
    ThreadNext {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        line: Option<u32>,
        add: Option<String>,
    },
    /// 완료 보고의 후속을 같은 프로젝트의 할 일에 잇거나 사유를 남겨 버린다.
    ThreadFollowup {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        id: String,
        task: Option<String>,
        reason: Option<String>,
    },
    /// coordinator용 프로젝트 요약. 보인 inbox 항목은 「봤다」가 된다(`peek`이면 그대로).
    Context {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        peek: bool,
    },
    /// thread를 맡길 수 있는 owner 목록. `check`를 주면 그 owner를 TASKS.md에 써도 되는지 본다.
    Assignable {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        check: Option<String>,
    },
    /// 할 일 화면이 읽는 TASKS.md 전체(FRONT_API 1.60, Reader 가능). 형식 어긋남·줄 번호·블록 hash·「나중」 파일을 함께 준다.
    Tasks {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 예약 읽기·등록/내용 갱신·취소·순서 변경. 등록은 해당 내용의 실행 승인이다.
    TaskQueue {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        action: crate::QueueOp,
    },
    TaskArchive {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        /// 비면 보기, 있으면 같은 ID로 활성 목록에 복원(예약은 복원하지 않는다).
        restore: Option<String>,
        /// FRONT_API 1.106: 검색·저장소·상태를 host에서 거른 뒤 바이트 상한 안의 페이지로 준다.
        #[serde(default)]
        #[ts(optional)]
        view: Option<crate::TaskArchiveQuery>,
    },
    /// 할 일 편집(FRONT_API 1.62, 조작 권한). 동작을 차례로 적용해 한 번에 쓴다. 세션 안에서는 그 프로젝트 coordinator만 된다.
    /// `base_sha256`을 주면 파일 전체가 그 판일 때만 쓴다. 답은 `task_edited`(충돌이면 `conflict`, 쓰지 않음)
    TaskEdit {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        ops: Vec<TaskOp>,
        #[serde(default)]
        base_sha256: Option<String>,
        /// 사용자 지시로 모델·머신만 변경하는 근거 한 줄(FRONT_API 1.97). 예약 순서와 시작 승인을 유지한다.
        #[serde(default)]
        #[ts(optional)]
        user_request: Option<String>,
    },
    /// 할 일 기록(FRONT_API 1.62, 보기 권한). 최신부터, `before`(id)보다 앞의 것
    TaskLog {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        #[ts(type = "number | null")]
        before: Option<i64>,
        #[serde(default)]
        limit: Option<u32>,
        #[serde(default)]
        filter: Option<TaskLogFilter>,
        /// 표시 ID·ULID 또는 제목으로 기록을 거른다.
        #[serde(default)]
        #[ts(optional)]
        task: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        search: Option<crate::TaskHistorySearch>,
    },
    /// 이 시각(ms)보다 오래된 할 일 기록을 지운다(FRONT_API 1.62, 조작 권한)
    TaskLogPrune {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[ts(type = "number")]
        before_ms: u64,
    },
    /// thread 목록. role 생략은 모든 역할. FRONT_API 1.130부터 커서·개수·바이트 상한을 둔다.
    Threads {
        project: Option<String>,
        #[serde(default)]
        role: Option<Role>,
        #[serde(flatten)]
        query: crate::ThreadListQuery,
    },
    ThreadShow {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// FRONT_API 1.44. thread 작업 요약(plan §8.259)
    ThreadSummary {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// FRONT_API 1.44. 작업 요약을 통째로 다시 쓴다(조작 권한, coordinator가 `thread summary --file`로)
    ThreadSummarySet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        text: String,
    },
    /// 보고서와 결과물 목록, 그리고 결과물을 볼 짧은 수명의 읽기 전용 경로
    ThreadLibrary {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// FRONT_API 1.36. 보고서별 링크 발급(유효한 링크는 재사용). 기본 30일, 조작 권한만.
    LibraryLink {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        days: Option<u32>,
    },
    /// FRONT_API 1.36. 서명 경로를 담으므로 목록도 조작 권한만.
    LibraryLinks {
        project: Option<String>,
    },
    /// FRONT_API 1.36. 보고서 링크 폐기.
    LibraryLinkRevoke {
        id: String,
    },
    /// FRONT_API 1.38. host 머신의 CLI만 로컬 파일을 읽는다. live는 복사 없이 등록한다.
    LibraryPublish {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        source: String,
        #[serde(default)]
        live: bool,
        /// FRONT_API 1.43. 사용자에게 보내는 웹 보고서(index.html 필수, 「내 보고서」·안 봄·사용자 알림 post)
        #[serde(default)]
        to_user: bool,
        /// FRONT_API 1.43. 「내 보고서」에 보일 제목(없으면 이름)
        #[serde(default)]
        title: Option<String>,
        /// FRONT_API 1.56. 보고서를 낸 thread(`t-NNNN`). 없으면 host가 원본 경로·이름에서 그 프로젝트의 thread를 찾는다
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        from_thread: Option<String>,
    },
    /// FRONT_API 1.114. 현재 판 또는 지정한 r 판을 보관함에 넣거나 뺀다. 사본을 추가하지 않는다.
    LibraryArchive {
        reports: Vec<LibraryRef>,
        archived: bool,
    },
    /// FRONT_API 1.119. 확인한 목록의 안 본 보고서를 한 번에 읽음 처리한다. Reader도 기존 열람과 같은 권한이다.
    LibraryMarkRead {
        reports: Vec<LibraryRef>,
    },
    /// FRONT_API 1.43. 모든 판의 host 사본·목록·공유 링크·thread 보고를 지운다. 보관함도 명시 삭제한다.
    /// 진행 중 thread·완료 할 일 참조 보호는 유지한다.
    LibraryDelete {
        reports: Vec<LibraryRef>,
    },
    LibraryImport {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        source: String,
        /// 0.2의 t- 이름을 새 thread id와 구분해 가져올 때 쓴다.
        #[serde(default)]
        prefix: String,
    },
    LibraryPrune {
        project: Option<String>,
        older_than_days: u32,
    },
    /// FRONT_API 1.40. 별도 요청이라 옛 host가 dry-run을 삭제 명령으로 해석할 수 없다.
    LibraryPrunePreview {
        project: Option<String>,
        older_than_days: Option<u32>,
    },
    LibraryRotate,
    /// FRONT_API 1.80. 웹으로 올린 파일 묶음(`POST /api/v1/projects/<p>/library-uploads/<upload>?path=…`)을 보고서 하나로 등록한다
    /// (관리 GUI W5b). 등록하면 올린 자리는 지운다. 사용자 보고서(`to_user`)는 맨 위 index.html이 있어야 한다.
    /// `files`는 웹이 올린 파일 수다: 올린 자리의 파일 수가 다르면(오래돼 치워진 뒤 나머지만 올라온 경우) `conflict`로 거부한다
    LibraryUploadPublish {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        upload: String,
        name: String,
        files: u32,
        #[serde(default)]
        to_user: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        title: Option<String>,
    },
    /// 링크는 조작 권한에만 싣는다. 앱 안 열기는 읽기 권한도 된다.
    LibraryReports {
        project: Option<String>,
    },
    LibraryOpen {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// FRONT_API 1.71. 보고서 보기 주소(`/v/<cap>/…`)의 cap이 이 프로젝트의 어느 보고서인지(task 첨부가 붙인 주소를 앱 안
    /// 보고서 문서로 바꾼다). 조작 권한만
    LibraryResolve {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        cap: String,
    },
    LibraryUsage,
    /// 지금 보고를 확인했다
    ThreadAck {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// 끝낸다: 마지막 사본(실패면 거부) → 세션 종료 → worktree 정리(force 없음). 사본에 빠진 것이 있으면 worktree를 남긴다
    ThreadResolve {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        keep_worktree: bool,
        /// 끝낸 thread를 다시 연다(herdr-projects `thread resolve --reopen`). 아무것도 띄우지 않는다
        #[serde(default)]
        reopen: bool,
        /// 마지막 사본 없이 끝낸다(사본을 만들 수 없을 때, worktree는 남긴다. herdr-projects `--skip-copy`)
        #[serde(default)]
        skip_copy: bool,
        /// 집에 다 오지 않았어도 worktree를 지운다(herdr-projects `--discard-uncopied`)
        #[serde(default)]
        discard_uncopied: bool,
    },
    /// FRONT_API 1.105(plan §8.426). 보류: 기본은 thread에게 정리(커밋·push·`## Handoff` 보고 `[보류 준비됨]`)를 맡기고 그 보고가
    /// 오면 pane을 닫는다. `now`면 바로 닫는다. 자리를 돌려주고 worktree·대화 ID·보고는 남긴다. `cancel`은 맡긴 정리를 거둔다.
    /// 사용자(조작 기기·세션 밖 CLI)와, `user_request`를 인용한 그 프로젝트 coordinator만
    ThreadPark {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        now: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        note: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        user_request: Option<String>,
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        cancel: bool,
    },
    /// FRONT_API 1.105. 보류를 풀고 이어 한다. 같은 머신(기본)은 같은 대화를 잇고, 다른 머신은 push된 branch로 새 worktree를 만들고
    /// 할 일·마지막 보고(Handoff)를 담은 brief로 새 대화를 띄운다. `dry_run`은 머신별 길만 답한다(`UnparkPreview`)
    ThreadUnpark {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        machine: Option<String>,
        /// 다른 머신의 저장소 경로(같은 이름의 저장소가 하나뿐이면 생략)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        repo: Option<String>,
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        dry_run: bool,
    },
    /// thread agent를 멈춘다(Esc 한 번). 세션·작업 폴더는 그대로다
    ThreadStop {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// 끝난 thread 세션을 다시 띄운다. 기본은 task를 다시 읽어 새 brief로 새 대화, `resume`이면 저장된 대화를 잇는다
    ThreadRestart {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
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
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// thread pane이 지금 보이는 것(신뢰 창·질문 메뉴·권한 확인창, herdr-projects `thread read`). `lines`면 보이는 화면 대신
    /// 기록 끝 N줄
    ThreadRead {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        #[serde(default)]
        lines: Option<u32>,
    },
    /// thread pane이 보이는 것에 답한다(herdr-projects `thread keys`): `text`를 치고(Enter 없음) `keys`를 누른다
    ThreadKeys {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
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
    /// 질문에 답한다. host가 질문이 아직 열려 있는지 다시 확인한다. `choice`(예·아니오) 대신 `pick`이면 화면 선택 창
    /// `menu`(그 id)에 답한다(plan §8.172)
    QuestionAnswer {
        id: String,
        #[serde(default)]
        choice: String,
        #[serde(default)]
        menu: Option<String>,
        #[serde(default)]
        pick: Option<MenuPick>,
    },
    /// thread agent에 메시지를 보낸다(전달을 모르면 다시 보내지 않는다)
    ThreadMessage {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        text: String,
        #[serde(default)]
        mode: MessageMode,
    },
    /// thread의 `## Questions`에 답한다(FRONT_API 1.72, plan §8.329). host가 지금 보고의 질문인지·아직 답하지 않았는지 다시 보고,
    /// 「1A 2B」 꼴 글을 `ThreadMessage`와 같은 길로 thread에 보낸 뒤 답을 기록한다(모든 기기가 「답함」을 본다)
    ThreadAnswer {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
        /// 답하는 질문의 보고 hash(`ThreadQuestions.report_hash`)
        report_hash: String,
        /// 질문마다 고른 선택지(0부터). 직접 쓴 답으로 대신하는 질문은 null
        picks: Vec<Option<u32>>,
        #[serde(default)]
        text: Option<String>,
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
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        all: bool,
        #[serde(default)]
        peek: bool,
    },
    /// inbox 항목을 끝낸다(`all`이면 끝내지 않은 것 모두)
    InboxDone {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        ids: Vec<String>,
        #[serde(default)]
        all: bool,
    },
    /// ticker를 지금 한 번 돌리고, 이 요청 뒤에 시작한 tick이 끝나면 답한다(herdr-projects 시험의 `ticker::tick_project`,
    /// t3code worker `drain`과 같은 자리). host가 「봤는데 할 일이 없었다」를 시험이 기다릴 수 있다(docs/testing.md).
    TickerTick,
    /// 특수 임무 목록과 적용 판. Reader도 볼 수 있다.
    MissionList {
        project: Option<String>,
    },
    MissionShow {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// 사람만 승인한다. Controller 웹 기기에서도 가능하며 세션 안 연결은 거부한다.
    MissionApprove {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        approval_sha256: String,
    },
    MissionReject {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionStart {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionStop {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionPause {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionResume {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionReload {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionRestart {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    MissionOpen {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// routine 목록(`projects/<p>/routines/*.md`)과 상태
    Routines {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 옛 routine·임무 파일을 값 검증 뒤 순수 TOML로 변환한다(Controller).
    AutomationApprove {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        approval_sha256: String,
    },
    /// `dry_run`이면 쓰지 않고 계획만 답한다(FRONT_API 1.58, Reader 가능). 옛 host는 그 칸을 버리고 변환하므로
    /// 웹은 `/api/v1/me`의 `front_api`가 1.58 이상일 때만 보낸다. `plan_sha256`을 주면 계획이 그 사이 바뀌었을 때 거부한다.
    AutomationConvert {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        #[ts(optional, as = "Option<bool>")]
        dry_run: bool,
        #[serde(default)]
        #[ts(optional)]
        plan_sha256: Option<String>,
    },
    /// 보냈는지 모르는 `do.message` 묶음을 사람이 확인하고 푼다(FRONT_API 1.58). `batch`는 화면에서 본 묶음이다.
    /// 그 묶음은 다시 보내지 않는다. `discard`면 남은 입력도 버리고, 아니면 남은 입력을 이어 보낸다. 세션 안 연결은 거부한다.
    AutomationUnblock {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        batch: String,
        #[serde(default)]
        discard: bool,
    },
    /// 자동화 파일 원문·판·본문과 검사 결과(관리 GUI W2, FRONT_API 1.76, Reader 가능)
    AutomationRead {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// 초안을 쓰지 않고 검사한다(Reader 가능). `form`(TOML과 같은 모양의 값)을 주면 host가 그 값을 원문에 합친다:
    /// 바뀐 칸만 고치고 주석·나머지 칸은 그대로 둔다. `body`는 `text_file`·`brief_file`이 가리킬 본문 초안이다.
    AutomationValidate {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        text: String,
        #[serde(default)]
        #[ts(optional, type = "Record<string, unknown>")]
        form: Option<serde_json::Value>,
        #[serde(default)]
        #[ts(optional)]
        body: Option<String>,
    },
    /// 검사한 초안을 쓴다. `expected_sha256`이 없으면 새로 만들기(그 이름 파일이 있으면 거부), 있으면 그 판일 때만 고친다
    /// (지운 규칙은 되살리지 않는다). 다르면 `conflict`로 거부하고 아무것도 쓰지 않는다. 본문 파일은 먼저, 규칙은 마지막에 쓴다.
    AutomationSave {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        text: String,
        #[serde(default)]
        #[ts(optional)]
        expected_sha256: Option<String>,
        #[serde(default)]
        #[ts(optional)]
        body: Option<String>,
        /// 본문 파일의 본 판. 없으면 그 파일이 아직 없어야 한다
        #[serde(default)]
        #[ts(optional)]
        body_expected_sha256: Option<String>,
    },
    /// 끈 뒤 `automations/.trash/`로 옮긴다. 대기 입력은 취소하고 처리 중 묶음·기록은 둔다. 본문 파일은 그대로 둔다.
    AutomationDelete {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        expected_sha256: String,
    },
    /// 지운 자동화를 꺼진 채로 되살린다. 같은 이름이 있으면 거부한다.
    AutomationRestore {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
    },
    /// 조건을 기다리지 않고 동작을 한 번 돌린다(수동 사건 하나). 같은 `key`는 한 번만 받는다. 꺼짐·파일 오류·승인되지 않은 범위는 거부한다.
    AutomationRun {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        key: String,
    },
    /// FRONT_API 1.77. 「이 aaa」·「저장소」 설정 화면이 읽는 프로젝트 설정(관리 GUI W4, plan §8.343). Reader 가능
    ProjectSettings {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// FRONT_API 1.77. 저장소 하나를 제자리에서 바꾼다(`PATH[@MACHINE]`, `repos.remove`+`repos.add`를 한 번에, 차례는 그대로)
    /// 머신별 workspace 자동 배정 허용(Reader도 읽는다). 경로는 project 설정이 정본이다.
    WorkspaceAccess {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        machine: String,
    },
    /// Owner(웹 조작 기기·세션 밖 CLI)만 허용을 바꾼다.
    WorkspaceAccessSet {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        machine: String,
        workspace: String,
        enabled: bool,
        revision: String,
    },
    ProjectRepoEdit {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        from: String,
        to: String,
    },
    /// FRONT_API 1.77. 끝내기 전에 잃을 수 있는 것(미커밋·미병합·사본 상태)을 읽는다. 기록을 고치지 않는다. 바뀐 파일 수를
    /// 그 머신 agent가 읽으므로 조작 기기만(`ThreadDiff`와 같다)
    ThreadResolvePreview {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        thread: String,
    },
    /// FRONT_API 1.77. 미리보기(`library_prune_preview`)와 같은 판정으로 지우되, 후보가 `expected`(`project/name`)와 같을 때만
    /// 지운다(그 사이 바뀌면 `conflict`). 웹 「보고서 정리」의 실행이다. 지우는 것은 `library_delete`보다 좁다(report.md
    /// 사본·thread 기록은 남는다)
    LibraryPruneApply {
        project: Option<String>,
        older_than_days: Option<u32>,
        expected: Vec<String>,
    },
    /// 최근 실행·건너뜀 기록, 최대 50건(Reader 가능).
    AutomationExecutions {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        #[serde(default)]
        #[ts(optional)]
        before: Option<String>,
    },
    AutomationExecution {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
    },
    RoutineRuns {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
    },
    /// routine 명령 승인. `command_sha256`은 사람이 본 명령(머신 포함)의 hash다. 파일이 그 사이 바뀌면 거부한다.
    RoutineApprove {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        command_sha256: String,
    },
    /// routine 켜기·끄기(front matter `enabled`, herdr-projects `routine toggle`). `on`이 없으면 뒤집는다.
    /// 명령 승인은 그대로 필요하다.
    RoutineToggle {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        name: String,
        #[serde(default)]
        on: Option<bool>,
    },
    /// 흐름 사건을 `after`(cursor) 다음부터 읽는다
    Events {
        project: Option<String>,
        #[serde(default)]
        #[ts(type = "number")]
        after: u64,
        #[serde(default)]
        limit: Option<u32>,
        /// 가장 최근 `limit`개(차례는 그대로 오래된 것부터, FRONT_API 1.77). 웹 알림함 「흐름 사건」이 쓴다. 옛 host는 버린다
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        #[ts(optional, as = "Option<bool>")]
        tail: bool,
    },
    /// 사후 평가자 지정(§8.13): 역할이 evaluator인 thread를 시작한다. 프로젝트당 하나다.
    EvaluatorAssign {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
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
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
    },
    /// 평가 기록 목록. 기본은 정하지 않은 개선안이 있는 것만
    Evaluations {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        #[serde(default)]
        all: bool,
    },
    EvaluationShow {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
    },
    /// 개선안 채택·보류(평가 기록에 누가 언제 정했는지 남는다)
    EvaluationDecide {
        #[serde(default)]
        #[ts(optional, as = "Option<String>")]
        project: String,
        id: String,
        proposals: Vec<String>,
        adopt: bool,
        #[serde(default)]
        note: Option<String>,
    },
}

impl FrontRequest {
    /// 프로젝트를 받는 요청의 프로젝트 칸(FRONT_API 1.50, plan §8.288). 비어 있으면(생략) host가 맡은 프로젝트로 채운다.
    /// 프로젝트 자체를 바꾸는 요청(lifecycle·delete·rename·sweep)은 이름을 늘 받으므로 여기 없다.
    pub fn project_mut(&mut self) -> Option<&mut String> {
        match self {
            FrontRequest::ThreadStartAccount { start, .. } | FrontRequest::ThreadStartPool { start, .. } => {
                start.project_mut()
            }
            FrontRequest::AccountStarts { project } | FrontRequest::AccountStartCancel { project, .. } => Some(project),
            FrontRequest::Post { project, .. }
            | FrontRequest::PostStatus { project, .. }
            | FrontRequest::PostHandled { project, .. }
            | FrontRequest::PostList { project, .. }
            | FrontRequest::Asks { project, .. }
            | FrontRequest::AskHandle { project, .. }
            | FrontRequest::AskAnswer { project, .. }
            | FrontRequest::QuestionReply { project, .. }
            | FrontRequest::AutoModeGet { project, .. }
            | FrontRequest::AutoDecisions { project, .. }
            | FrontRequest::AutoModeSet { project, .. }
            | FrontRequest::QuestionAutoAnswer { project, .. }
            | FrontRequest::AutoDecisionReview { project, .. }
            | FrontRequest::AskForget { project, .. }
            | FrontRequest::PostUnmute { project, .. }
            | FrontRequest::ThreadFile { project, .. }
            | FrontRequest::ThreadDiff { project, .. }
            | FrontRequest::ProjectSet { project, .. }
            | FrontRequest::ProjectOpen { project, .. }
            | FrontRequest::ThreadStart { project, .. }
            | FrontRequest::ThreadAdopt { project, .. }
            | FrontRequest::ThreadNext { project, .. }
            | FrontRequest::ThreadFollowup { project, .. }
            | FrontRequest::Context { project, .. }
            | FrontRequest::Assignable { project, .. }
            | FrontRequest::Tasks { project }
            | FrontRequest::TaskQueue { project, .. }
            | FrontRequest::TaskArchive { project, .. }
            | FrontRequest::TaskEdit { project, .. }
            | FrontRequest::TaskLog { project, .. }
            | FrontRequest::TaskLogPrune { project, .. }
            | FrontRequest::ThreadStartPreview { project, .. }
            | FrontRequest::ThreadShow { project, .. }
            | FrontRequest::ThreadSummary { project, .. }
            | FrontRequest::ThreadSummarySet { project, .. }
            | FrontRequest::ThreadLibrary { project, .. }
            | FrontRequest::LibraryLink { project, .. }
            | FrontRequest::LibraryPublish { project, .. }
            | FrontRequest::LibraryUploadPublish { project, .. }
            | FrontRequest::SafetyShow { project, .. }
            | FrontRequest::SafetySet { project, .. }
            | FrontRequest::ProfilePlan { project, .. }
            | FrontRequest::ProfileApply { project, .. }
            | FrontRequest::LibraryImport { project, .. }
            | FrontRequest::LibraryOpen { project, .. }
            | FrontRequest::LibraryResolve { project, .. }
            | FrontRequest::ThreadAck { project, .. }
            | FrontRequest::ThreadResolve { project, .. }
            | FrontRequest::ThreadStop { project, .. }
            | FrontRequest::ThreadPark { project, .. }
            | FrontRequest::ThreadUnpark { project, .. }
            | FrontRequest::ThreadRestart { project, .. }
            | FrontRequest::ThreadBrief { project, .. }
            | FrontRequest::ThreadRead { project, .. }
            | FrontRequest::ThreadKeys { project, .. }
            | FrontRequest::ThreadMessage { project, .. }
            | FrontRequest::ThreadAnswer { project, .. }
            | FrontRequest::Inbox { project, .. }
            | FrontRequest::InboxDone { project, .. }
            | FrontRequest::MissionShow { project, .. }
            | FrontRequest::MissionApprove { project, .. }
            | FrontRequest::MissionReject { project, .. }
            | FrontRequest::MissionStart { project, .. }
            | FrontRequest::MissionStop { project, .. }
            | FrontRequest::MissionPause { project, .. }
            | FrontRequest::MissionResume { project, .. }
            | FrontRequest::MissionReload { project, .. }
            | FrontRequest::MissionRestart { project, .. }
            | FrontRequest::MissionOpen { project, .. }
            | FrontRequest::Routines { project, .. }
            | FrontRequest::RoutineRuns { project, .. }
            | FrontRequest::AutomationExecution { project, .. }
            | FrontRequest::AutomationExecutions { project, .. }
            | FrontRequest::RoutineApprove { project, .. }
            | FrontRequest::AutomationApprove { project, .. }
            | FrontRequest::AutomationConvert { project, .. }
            | FrontRequest::AutomationUnblock { project, .. }
            | FrontRequest::AutomationRead { project, .. }
            | FrontRequest::AutomationValidate { project, .. }
            | FrontRequest::AutomationSave { project, .. }
            | FrontRequest::AutomationDelete { project, .. }
            | FrontRequest::AutomationRestore { project, .. }
            | FrontRequest::AutomationRun { project, .. }
            | FrontRequest::ProjectSettings { project }
            | FrontRequest::WorkspaceAccess { project, .. }
            | FrontRequest::WorkspaceAccessSet { project, .. }
            | FrontRequest::ProjectRepoEdit { project, .. }
            | FrontRequest::ThreadResolvePreview { project, .. }
            | FrontRequest::RoutineToggle { project, .. }
            | FrontRequest::EvaluatorAssign { project, .. }
            | FrontRequest::EvaluatorUnassign { project, .. }
            | FrontRequest::Evaluations { project, .. }
            | FrontRequest::EvaluationShow { project, .. }
            | FrontRequest::EvaluationDecide { project, .. } => Some(project),
            _ => None,
        }
    }

    /// 조작(controller) 권한이 필요한 요청인가
    pub fn needs_control(&self) -> bool {
        !matches!(
            self,
            FrontRequest::AccountList
                | FrontRequest::AccountDefaults
                | FrontRequest::AccountPool
                | FrontRequest::AccountStarts { .. }
                | FrontRequest::Status
                | FrontRequest::DeployContext
                | FrontRequest::DeployStatus { .. }
                | FrontRequest::Rollouts
                | FrontRequest::DeployJobs
                | FrontRequest::PostStatus { .. }
                | FrontRequest::Asks { .. }
                | FrontRequest::AutoModeGet { .. }
                | FrontRequest::AutoDecisions { .. }
                | FrontRequest::WorkspaceDashboard { .. }
                | FrontRequest::Machines
                | FrontRequest::MachineSetupList
                | FrontRequest::Panes { .. }
                | FrontRequest::Projects { .. }
                | FrontRequest::Instance
                | FrontRequest::ProjectRename { dry_run: true, .. }
                | FrontRequest::ProjectSweep { dry_run: true, .. }
                | FrontRequest::ProjectFiles { fix: false }
                | FrontRequest::Profiles { .. }
                | FrontRequest::ProfileResolve { .. }
                | FrontRequest::Threads { .. }
                | FrontRequest::ThreadShow { .. }
                | FrontRequest::ThreadLibrary { .. }
                | FrontRequest::ThreadSummary { .. }
                | FrontRequest::ThreadUnpark { dry_run: true, .. }
                | FrontRequest::LibraryReports { .. }
                | FrontRequest::LibraryOpen { .. }
                | FrontRequest::LibraryMarkRead { .. }
                | FrontRequest::LibraryUsage
                | FrontRequest::LibraryPrunePreview { .. }
                | FrontRequest::ThreadNext { line: None, add: None, .. }
                | FrontRequest::Context { peek: true, .. }
                | FrontRequest::Assignable { .. }
                | FrontRequest::Tasks { .. }
                | FrontRequest::TaskQueue { action: crate::QueueOp::List, .. }
                | FrontRequest::TaskArchive { restore: None, .. }
                | FrontRequest::TaskLog { .. }
                | FrontRequest::Events { .. }
                | FrontRequest::Questions { .. }
                | FrontRequest::Inbox { peek: true, .. }
                | FrontRequest::MissionList { .. }
                | FrontRequest::MissionShow { .. }
                | FrontRequest::Routines { .. }
                | FrontRequest::RoutineRuns { .. }
                | FrontRequest::AutomationExecution { .. }
                | FrontRequest::AutomationExecutions { .. }
                | FrontRequest::AutomationConvert { dry_run: true, .. }
                | FrontRequest::AutomationRead { .. }
                | FrontRequest::AutomationValidate { .. }
                | FrontRequest::WorkspaceAccess { .. }
                | FrontRequest::ProjectSettings { .. }
                | FrontRequest::ThreadRead { .. }
                | FrontRequest::PaneRead { .. }
                | FrontRequest::PaneReadText { .. }
                | FrontRequest::PaneWaitOutput { .. }
                | FrontRequest::Workspaces
                | FrontRequest::PaneGet { .. }
                | FrontRequest::PaneChat { .. }
                | FrontRequest::Workspace { .. }
                | FrontRequest::InstallPublish { .. }
                | FrontRequest::InstallPublishStatus { .. }
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
            FrontRequest::AccountImport { .. }
                | FrontRequest::WebPublishPort { .. }
                | FrontRequest::AccountRemove { .. }
                | FrontRequest::DebugInput { .. }
                | FrontRequest::TickerTick
                | FrontRequest::LibraryPublish { .. }
                | FrontRequest::LibraryImport { .. }
                | FrontRequest::LibraryPrune { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrontResponse {
    WorkspaceDashboard(crate::WorkspaceDashboard),
    InstallPublished {
        publication: crate::install::InstallPublication,
    },
    InstallApproval {
        approval: crate::install::InstallApprovalView,
    },
    Accounts {
        accounts: Vec<crate::AccountView>,
    },
    AccountDefaults {
        defaults: Vec<crate::AccountDefault>,
    },
    AccountPool {
        pool: crate::AccountPoolView,
    },
    AccountStarts {
        starts: Vec<crate::AccountStartView>,
    },
    AccountStart {
        start: crate::AccountStartView,
    },
    AccountLogin {
        login: crate::AccountLoginView,
    },
    InputEvents {
        events: Vec<crate::InputEvent>,
    },
    DeployContext {
        project: Option<String>,
        session: bool,
        allowed: bool,
    },
    Rollout {
        rollout: crate::RolloutView,
    },
    /// 웹 조작 기기의 rollout 시작·조회와 `rollouts`. 표시용 요약만 싣는다
    RolloutCards {
        cards: Vec<crate::RolloutCard>,
    },
    RolloutPlan {
        plan: crate::RolloutPlanView,
    },
    /// 웹 조작 기기의 재시작 시작·조회와 `deploy_jobs`
    DeployJobs {
        jobs: Vec<crate::DeployJobCard>,
    },
    Deployment {
        machine: String,
        job: crate::DeployJob,
        running_software: String,
        running_sha256: String,
    },
    Missions {
        missions: Vec<crate::MissionView>,
    },
    Mission {
        mission: Box<crate::MissionView>,
    },
    Post {
        result: crate::PostResult,
        /// status 조회에서 확인한 inbox 처리 상태(FRONT_API 1.84). 옛 host는 생략한다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        handled: Option<bool>,
    },
    Posts {
        posts: Vec<crate::PostEnvelope>,
        /// 음소거된 보낸 쪽(`post unmute`에 줄 이름, FRONT_API 1.77). 옛 host는 생략한다
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<String>>")]
        muted: Vec<String>,
    },
    /// 「답할 것」(FRONT_API 1.78): 열린 확인 필요 post, 답한 것(최신부터), 자동 정리 기간
    AutoMode(crate::AutoMode),
    AutoDecisions {
        decisions: Vec<crate::AutoDecision>,
        next: Option<String>,
    },
    Asks {
        #[serde(default)]
        auto_mode: crate::AutoMode,
        #[serde(default)]
        auto_decisions: Vec<crate::AutoDecision>,
        #[serde(default)]
        auto_next: Option<String>,
        #[serde(default)]
        auto_exceptions: u32,
        requests: Vec<crate::UserRequestView>,
        done: Vec<crate::AskRecord>,
        keep_days: u32,
    },
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
    /// `tasks`의 답(FRONT_API 1.60)
    TaskBoard(TaskBoard),
    TaskQueue {
        entries: Vec<crate::TaskReservation>,
    },
    TaskArchive {
        entries: Vec<crate::ArchivedTask>,
        #[serde(default)]
        #[ts(optional)]
        next: Option<String>,
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<crate::TaskHistoryRepo>>")]
        repos: Vec<crate::TaskHistoryRepo>,
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<String>>")]
        profiles: Vec<String>,
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<String>>")]
        machines: Vec<String>,
    },
    /// `task_edit`의 답(FRONT_API 1.62): 쓴 뒤(또는 충돌이면 지금) board. `log_ids`는 이 편집이 남긴 기록
    TaskEdited {
        board: TaskBoard,
        conflict: Option<TaskConflict>,
        #[ts(type = "number[]")]
        log_ids: Vec<i64>,
    },
    /// `thread_start_preview`의 답(FRONT_API 1.63)
    ThreadStartPreview(ThreadStartPreview),
    /// `task_log`의 답(FRONT_API 1.62). `next`를 `before`로 주면 그 앞을 읽는다
    TaskLog {
        entries: Vec<TaskLogEntry>,
        #[ts(type = "number | null")]
        next: Option<i64>,
        #[serde(default)]
        #[ts(optional, as = "Option<Vec<crate::TaskHistoryRepo>>")]
        repos: Vec<crate::TaskHistoryRepo>,
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
    PaneDir {
        listing: DirListing,
    },
    PaneCommands {
        list: CommandList,
    },
    EnrollGuide {
        guide: crate::EnrollGuide,
    },
    MachineSetup {
        setup: crate::MachineSetupView,
    },
    MachineSetups {
        setups: Vec<crate::MachineSetupView>,
    },
    Pending {
        pending: Vec<PendingEnroll>,
        /// 등록 창이 열려 있으면 닫히는 시각(ms, FRONT_API 1.80). 닫혔으면 생략
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional, type = "number")]
        open_until_ms: Option<u64>,
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
    Instance {
        project: Option<ProjectView>,
    },
    Project(ProjectView),
    ProjectChange(ProjectChange),
    ProjectOpened(Box<ProjectOpened>),
    ProjectFiles {
        checks: Vec<ProjectFilesCheck>,
    },
    Profiles {
        machine: String,
        profiles: Vec<ProfileView>,
    },
    ProfileResolved {
        profile: ProfileView,
    },
    Threads {
        threads: Vec<ThreadView>,
        /// 다음 페이지가 있으면 이 커서를 after로 보낸다.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        next_after: Option<String>,
    },
    Thread(Box<ThreadView>),
    /// FRONT_API 1.44. thread 작업 요약
    ThreadSummary(ThreadSummaryView),
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
    /// `ticker_tick`: 이 host 실행에서 끝난 tick 수(방금 돈 것까지)
    Ticker {
        runs: u64,
    },
    Routines {
        routines: Vec<RoutineView>,
        /// 지운 자동화(FRONT_API 1.76). 옛 host는 생략한다
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[ts(optional, as = "Option<Vec<crate::AutomationTrashEntry>>")]
        trash: Vec<crate::AutomationTrashEntry>,
    },
    /// `automation_read`·`automation_save`의 답
    AutomationDoc {
        doc: crate::AutomationDoc,
    },
    /// `automation_validate`의 답: 합친 원문과 검사 결과
    AutomationChecked {
        text: String,
        check: crate::AutomationCheck,
    },
    /// `automation_run`의 답. `accepted`가 거짓이면 같은 key를 이미 받았다
    AutomationRan {
        accepted: bool,
        detail: String,
    },
    /// `project_settings`·`project_repo_edit`의 답
    ProjectSettings(crate::ProjectSettingsView),
    WorkspaceAccess(crate::WorkspaceAccessView),
    Safety(crate::SafetyView),
    ProfileAdmin(crate::ProfileAdminView),
    ProfilePlan(crate::ProfilePlanView),
    /// profile을 바꾼 뒤: 알림 글과 경고, 새 목록
    ProfileApplied {
        message: String,
        warnings: Vec<String>,
        view: crate::ProfileAdminView,
    },
    WorkRoot(crate::WorkRootView),
    WorkRootChecked(crate::WorkRootCheckView),
    Hook(crate::HookView),
    /// `thread_resolve_preview`의 답
    ResolvePreview(crate::ResolvePreview),
    /// FRONT_API 1.105. `thread_unpark { dry_run: true }`
    UnparkPreview(crate::UnparkPreview),
    AutomationConverted {
        files: Vec<String>,
        /// 쓰지 않은 계획이다(FRONT_API 1.58). `files`는 실제 변환과 같은 모양(새 파일과 `.converted/` 보존 사본)이다.
        #[serde(default)]
        dry_run: bool,
        /// 계획 hash. 실행 요청에 실어 보내면 그 사이 바뀐 계획을 거부한다.
        #[serde(default)]
        plan_sha256: String,
    },
    AutomationExecutions {
        runs: Vec<crate::AutomationExecution>,
        next: Option<String>,
    },
    AutomationExecution {
        run: crate::AutomationExecution,
    },
    RoutineRuns {
        runs: Vec<crate::RoutineRun>,
        #[serde(default)]
        executions: Vec<crate::AutomationExecution>,
        /// 파일 원문(FRONT_API 1.58). 임무 조건·깨진 파일·옛 host는 없다.
        #[serde(default)]
        source: Option<crate::RoutineSource>,
    },
    Library(LibraryView),
    /// `thread_diff`의 답
    ThreadDiff {
        project: String,
        thread: String,
        diff: crate::DiffView,
    },
    LibraryLink(LinkView),
    /// `library_resolve`의 답: 그 cap이 가리키는 보고서
    LibraryResolved {
        project: String,
        name: String,
    },
    LibraryReports {
        reports: Vec<LibraryReport>,
    },
    LibraryImported {
        reports: Vec<LibraryReport>,
        skipped: Vec<LibrarySkip>,
    },
    LibraryPruned {
        removed: Vec<String>,
    },
    LibraryPrunePreview(LibraryPrunePlan),
    LibraryRotated,
    LibraryUsage {
        /// FRONT_API 1.114. size·max_size에서 제외한 보관함 용량.
        #[serde(default)]
        #[ts(type = "number")]
        archived_size: u64,
        #[ts(type = "number")]
        size: u64,
        reports: u32,
        live: u32,
        problems: u32,
        #[serde(default)]
        #[ts(type = "number")]
        max_size: u64,
        #[serde(default)]
        #[ts(type = "number")]
        max_days: u64,
        #[serde(default)]
        #[ts(type = "number")]
        protected_size: u64,
    },
    LibraryLinks {
        links: Vec<LinkView>,
    },
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

/// front가 알리는 복사 결과. 본문·clipboard 내용·OS 오류 문자열은 host로 보내지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "ev", rename_all = "snake_case")]
pub enum FrontEvent {
    /// 머신·pane 목록이 바뀌었다(전체 목록)
    State { machines: Vec<MachineView>, panes: Vec<PaneView> },
    /// workspace layout이 바뀌었다
    Workspace(Workspace),
    /// 흐름 사건(새로 기록될 때). 놓쳤으면 `Events`로 cursor부터 다시 읽는다.
    Flow(FlowEvent),
    /// thread 기록이 바뀌었다. JSON은 그대로 두고 큰 본체를 별도로 보관한다.
    Thread(Box<ThreadView>),
    /// 확인창 질문이 생기거나 답해졌다(알림 대상)
    Question(QuestionView),
    /// 공유 설정 문서 전체. 웹 events 연결이 열릴 때와 바뀔 때마다 온다(herdr-web-ui `preferences` 사건)
    Preferences(SharedPreferences),
    /// 상태 사건 연결의 `ping`에 대한 답(herdr-web-ui ws.ts 「Liveness」: 그 ping의 답만 연결이 살아 있음을 말한다)
    Pong { id: u32 },
    /// host가 주는 웹 빌드(주 묶음 파일 이름, 예 `index-BsC_6E06`). 웹 events 연결이 열릴 때마다 온다. 제 빌드와 다르면 웹은
    /// 「다시 열기」 줄을 띄운다(배포 뒤 홈 화면 앱이 옛 웹을 계속 돌리는 문제, herdr-web-ui `reloadReason` "updated", plan §8.166)
    Web { build: String },
    /// FRONT_API 1.43. 보고서 목록이 바뀌었다(보내기·열기·지우기·thread 사본). 웹 events 연결에만, 열 때와 바뀔 때 보낸다.
    /// `unseen`은 열지 않은 사용자 보고서 수(활동 막대 보고서 배지)
    Reports { unseen: u32 },
    /// FRONT_API 1.78. 「답할 것」의 확인 필요 post나 답한 것 기록이 바뀌었다(웹은 `asks`를 다시 읽는다). 웹 events 연결에만, 열 때와
    /// 바뀔 때 보낸다. `requests`는 열린 확인 필요 post 수
    Asks { requests: u32 },
    /// FRONT_API 1.77. 알림함(inbox)이 바뀌었다(항목이 더해지거나 끝났다). 웹 events 연결에만, 열 때와 바뀔 때 보낸다.
    /// 웹은 `inbox`(`peek`)를 다시 읽는다(배지·알림함 패널)
    Inbox,
    /// FRONT_API 1.79. host 계정이 바뀌었다(로그인 진행·끝, 기본, 메일·상태). 웹 events 연결에만, 열 때와 바뀔 때 보낸다.
    /// 웹은 `account_list`·`account_defaults`와 보던 로그인의 `account_login_status`를 다시 읽는다
    Accounts,
    /// FRONT_API 1.111. 원장 저장 뒤 웹 지표를 다시 읽는다. 웹 events 연결에만 보낸다.
    WorkspaceMetrics,
    /// FRONT_API 1.80. 머신 등록 창·대기 목록이 바뀌었다(열기·닫기·새 요청·승인·거부). 웹 events 연결에만, 열 때와 바뀔 때 보낸다.
    /// 웹은 `enroll_pending`을 다시 읽는다
    Enroll,
    /// FRONT_API 1.60. 프로젝트 TASKS.md가 바뀌었다(ticker가 크기·mtime을 보고 내용 hash로 확인한다). 웹은 `tasks`를 다시 읽는다.
    /// 옛 CLI는 모르는 사건을 풀지 못하므로 1.60 이상과 합의한 연결에만 보낸다.
    Tasks { project: String, sha256: String },
    /// 설치 앱 아이콘의 host 공통 수. 웹 events 연결에서 처음과 값이 바뀔 때만 보낸다.
    AppBadge { count: u32 },
    /// 웹 연결 처음과 배포 내구 상태가 바뀔 때 주는 최신 요약. 조회만 하며 진행기를 재개하지 않는다.
    Rollout { result: Outcome<Option<crate::RolloutCard>> },
    /// 앱 안 알림(웹 Droplet, plan §8.185). 기록하지 않는다: 놓친 알림은 다시 오지 않는다(FRONT_API 1.34,
    /// 옛 front는 모르는 사건을 버린다)
    Notice(NoticeView),
    /// `chat_watch`로 청한 pane의 대화 기록이 바뀌었다(herdr-web-ui `conversation-changed`). 웹은 대화를 다시 읽는다.
    /// `signature`는 뜻 없는 이름이다(REST ETag가 아니다)
    ChatChanged { pane: String, signature: String },
    /// `chat_watch`로 청한 pane의 기록 변화를 지금 알릴 수 있는가(그 머신 agent가 `chat_watch.v1`이고 붙어 있다). 청하면 한 번,
    /// 바뀌면 다시 온다. 아니면 웹은 2초마다 읽는다(참고 conversationRefresh.ts push 없는 판)
    ChatWatching { pane: String, live: bool },
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
        /// coordinator의 다음 CLI 연결에 보일 활성 post 음소거. 옛 host는 비어 있다.
        #[serde(default)]
        warnings: Vec<String>,
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
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "terminal_compression")]
        #[ts(optional)]
        compression: Option<TerminalCompression>,
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
        #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "terminal_compression")]
        #[ts(optional)]
        compression: Option<TerminalCompression>,
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
        /// 대기 메시지 「지금 보내기」(`SteerQueued`)를 안다(1.45). 옛 host는 이 칸이 없어 웹이 단추를 숨긴다
        #[serde(default)]
        steer_queued: bool,
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
    /// `SubmitCancel`의 답(plan §8.247): 그 submit이 어디서 멈췄나. 멈췄으면 뒤에 오는 `Submitted`는 실패다
    SubmitCancelled {
        id: u32,
        stage: SubmitStage,
    },
    /// `SteerQueued`의 답(plan §8.271). `sent`면 steering 키가 pane에 들어갔다(글이 실제로 읽혔는지는 대화 기록이 말한다).
    /// 아니면 `reason`이 까닭이고, `Failed`의 `message`는 host 오류 글이다
    SteerQueued {
        id: u32,
        sent: bool,
        reason: Option<SteerRefusal>,
        message: Option<String>,
    },
    Error(ErrorBody),
}

/// 대기 메시지 「지금 보내기」를 하지 않은 까닭(`TermServerMsg::SteerQueued`, plan §8.271)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SteerRefusal {
    /// agent가 작업 중이 아니다(대기 글은 곧 읽힌다)
    NotWorking,
    /// agent가 질문·확인창에 답을 기다린다
    Question,
    /// harness 입력창에 보내지 않은 글이 있다(steering 키가 그 글까지 보낸다)
    Draft,
    /// 화면에 대기 글이 없다(이미 읽혔을 수 있다)
    NoQueue,
    /// 입력 상자·작업 표시를 확인하지 못했다
    UnknownScreen,
    /// 대기 글을 지금 보내는 키가 없는 프로그램이다
    Unsupported,
    /// 보내다 실패했다(권한·pane 끝남·연결)
    Failed,
    /// agent가 대화를 정리하는 중이다(plan §8.309, 1.61): harness가 정리가 끝날 때까지 대기 글을 읽지 않는다
    Compacting,
}

/// 멈춘 submit이 어디까지 갔나(`SubmitCancelled`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SubmitStage {
    /// pane에 아무것도 쓰지 않았고 쓰지 않는다
    NotSent,
    /// 글은 pane에 갔거나 가는 중이고 Enter는 보내지 않는다(harness 입력창에 글이 남을 수 있다)
    NoEnter,
    /// Enter까지 보냈거나 이미 끝난 submit이다: 멈추지 않는다
    TooLate,
}

/// 웹 터미널 WebSocket에서 브라우저가 보내는 메시지
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum TermClientMsg {
    Input {
        data: String,
    },
    /// front에서 입력줄을 추측하지 않고 host의 공통 Ctrl+Enter 판정을 쓴다.
    CtrlEnter {
        #[serde(default)]
        trace: Option<String>,
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
        #[serde(default)]
        trace: Option<String>,
        #[serde(default)]
        meta: crate::InputSubmitMeta,
        /// 입력창의 Ctrl+Enter(사용자 10-19 15:3x, plan §8.239 후속): 글 뒤 Enter 대신 host가 steering 키를 고른다(작업 중인 Claude는
        /// sendNow, 그 밖은 Enter). 옛 host는 이 칸을 버려 보통 보내기가 된다
        #[serde(default)]
        steer: bool,
    },
    /// 같은 WebSocket에서 받은 submit의 UI 사건. 원문·임의의 실패 문자열은 받지 않는다.
    InputTrace {
        id: String,
        event: crate::InputTraceEvent,
        method: Option<crate::InputReceiptMethod>,
    },
    /// 대기 메시지 「지금 보내기」(plan §8.271): harness 대기열에 이미 있는 글을 지금 턴에 넣는 키만 보낸다(글은 다시 보내지
    /// 않는다). host가 입력창이 비고 대기 글이 화면에 있을 때만 키를 고르고(Claude sendNow, Codex Esc) `SteerQueued`로 답한다
    SteerQueued {
        id: u32,
        #[serde(default)]
        trace: Option<String>,
    },
    /// 진행 중인 `Submit`을 멈춰 달라(plan §8.247): 전달 전에 막혔을 때 입력창이 빠져나가는 길이다.
    /// host는 submit을 기다리는 동안에도 이것은 바로 읽고 `SubmitCancelled`로 답한다(다른 frame은 submit 뒤 차례대로).
    SubmitCancel {
        id: u32,
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
    /// 이 화면의 터미널 기본 색이 바뀌었다(테마를 바꿈). 처음 색은 pane WebSocket 주소의 `fg`·`bg`로 보낸다(plan §8.187).
    /// `rrggbb`이고 앞의 `#`은 있어도 된다
    Colors {
        fg: String,
        bg: String,
    },
}

/// 상태 사건 WebSocket(`/api/v1/events`)에서 브라우저가 보내는 메시지
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum EventsClientMsg {
    /// 이 화면을 지금 쓰고 있다(보이고 focus가 있다, herdr-web-ui `presence`). 연결이 끊기면 쓰지 않는 것이다.
    /// 나머지는 판정 근거 기록에만 쓴다(10-18 실기, 옛 웹은 보내지 않는다).
    Presence {
        active: bool,
        /// 무엇이 알리게 했나(focus·blur·visibilitychange·pagehide·pageshow·freeze·resume·open·heartbeat)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        why: Option<String>,
        /// 그때 `document.visibilityState === "visible"`
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        visible: Option<bool>,
        /// 그때 `document.hasFocus()`
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        focus: Option<bool>,
        /// 이 웹 빌드(주 묶음 파일 이름). 배포 뒤에도 옛 페이지가 도는지 본다
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        build: Option<String>,
    },
    /// 사건 하나를 앱 안 알림으로 어떻게 했나(판정 근거 기록, host는 흐름 사건으로 남긴다). `target`은 알림 자리
    /// (`thread=…`·`pane=…`), `kind`는 `blocked`·`done`, `outcome`은 웹이 정한 한 줄(띄움·보는 중·가려짐·꺼짐 등)
    Alert { project: Option<String>, target: String, kind: String, outcome: String },
    /// 사람이 이 기기에서 방금 눌렀거나 쳤거나 화면을 켰다(보이고 focus가 됐다·앱을 열었다). host는 마지막으로 이것을 보낸
    /// 기기(또는 pane에 입력한 기기)를 「쓰는 기기」로 두고, 그 기기가 지금 쓰는 화면일 때만 Web Push를 건너뛴다
    /// (사용자 결정 10-18, plan §8.105. 참고는 쓰는 화면이 하나라도 있으면 건너뛴다)
    Touch,
    /// 연결이 살아 있는지 묻는다. host는 같은 `id`의 `pong`으로 답한다(herdr-web-ui ws.ts `probe`·server `ping`).
    /// 휴대폰이 잠들었다 깨거나 망이 바뀌면 열린 것처럼 보이는 죽은 연결이 몇 분씩 남는다.
    Ping { id: u32 },
    /// 이 화면이 그 pane의 대화를 보고 있다(herdr-web-ui `conversation-watch`). 켜 둔 동안 host는 `chat_watching`과
    /// `chat_changed`를 보낸다. 연결이 끊기면 끈 것이다
    ChatWatch { pane: String, enabled: bool },
}

impl AgentRequest {
    /// 1.9 agent와 파일 전송을 계속한다. 새 기능이 없는 상대에게 새 태그를 보내지 않는다.
    pub fn legacy_work_file(self) -> Self {
        match self {
            Self::WorkFileInboxStatus { work_dir } => Self::MissionInboxStatus { work_dir },
            Self::WorkFileList { work_dir } => Self::MissionStateList { work_dir },
            Self::WorkFileRead { work_dir, path, offset } => Self::MissionStateRead { work_dir, path, offset },
            Self::WorkFileWriteChunk { work_dir, path, offset, data, eof, sha256 } => {
                Self::MissionStateWrite { work_dir, path, offset, data, eof, sha256 }
            }
            other => other,
        }
    }
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
            responsibility: None,
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
        for req in [
            FrontRequest::LibraryLink { project: "p".into(), thread: "t-0001".into(), days: None },
            FrontRequest::LibraryLinks { project: None },
            FrontRequest::LibraryLinkRevoke { id: "id".into() },
        ] {
            assert!(req.needs_control());
        }
        // reader가 보낼 수 있는 요청은 읽기뿐이어야 한다.
        assert!(!FrontRequest::PaneRead { pane: "p".into() }.needs_control());
        // thread pane 읽기는 보기, 키로 답하기는 조작이다(세션 안 CLI는 보기만, §8.25)
        let (project, thread) = ("p".to_string(), "t-0001".to_string());
        assert!(
            !FrontRequest::ThreadRead { project: project.clone(), thread: thread.clone(), lines: None }.needs_control()
        );
        assert!(FrontRequest::ThreadKeys { project, thread, keys: vec!["enter".into()], text: None }.needs_control());
        assert!(
            !FrontRequest::PaneChat { pane: "p".into(), thinking: false, compact: true, commands: true, before: None }
                .needs_control()
        );
        // 파일 보기는 조작 기기만(herdr-web-ui도 보기 전용 기기는 fs/ 403)
        assert!(FrontRequest::PaneFile { pane: "p".into(), path: "a.txt".into() }.needs_control());
        // diff는 소스를 보인다: 보기 기기는 못 받는다(plan §8.305)
        assert!(
            FrontRequest::ThreadDiff { project: "p".into(), thread: "t-0001".into(), query: Default::default() }
                .needs_control()
        );
        assert!(
            FrontRequest::ThreadFile { project: "p".into(), thread: "t-0001".into(), path: "a.txt".into() }
                .needs_control()
        );
        assert!(FrontRequest::PaneDir { pane: "p".into(), path: "src".into(), hidden: false }.needs_control());
        assert!(FrontRequest::PanePaste { pane: "p".into(), text: "a".into() }.needs_control());
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
        // 「직접 설치」 안내는 host 주소·지문을 보이므로 조작 기기만(웹 허용, plan §8.383)
        assert!(FrontRequest::EnrollGuide.needs_control() && !FrontRequest::EnrollGuide.local_only());
        // SSH 머신 추가(plan §8.384): 목록은 Reader도, 시작·신뢰·고름·멈춤은 조작 기기
        assert!(!FrontRequest::MachineSetupList.needs_control());
        assert!(FrontRequest::MachineSetupStart { target: "mac-b".into() }.needs_control());
        assert!(
            FrontRequest::MachineSetupTrust { job: "j".into(), host_key_sha256: "SHA256:x".into() }.needs_control()
        );
        assert!(!FrontRequest::MachineSetupStart { target: "mac-b".into() }.local_only());
        let apply =
            FrontRequest::MachineSetupApply { job: "j".into(), plan_sha256: "0".repeat(64), service_password: None };
        assert!(apply.needs_control() && !apply.local_only());
        assert!(!FrontRequest::Threads { project: None, role: None, query: Default::default() }.needs_control());
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
        // 설정 관리(W5c)는 읽기도 조작 기기만: host 설정 경로·profile 인자·머신 폴더를 보인다
        assert!(FrontRequest::ProfileAdmin.needs_control());
        assert!(FrontRequest::SafetyShow { project: "p".into(), scope: Default::default() }.needs_control());
        assert!(FrontRequest::WorkRootShow { machine: "m".into() }.needs_control());
        assert!(FrontRequest::HookStatus { machine: "m".into() }.needs_control());
        assert!(!FrontRequest::Questions { all: true }.needs_control());
        assert!(
            FrontRequest::QuestionAnswer { id: "q".into(), choice: "yes".into(), menu: None, pick: None }
                .needs_control()
        );
        assert!(
            FrontRequest::QuestionAnswer {
                id: "q".into(),
                choice: String::new(),
                menu: Some("m".into()),
                pick: Some(MenuPick::Custom { text: "x".into() })
            }
            .needs_control()
        );
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
            FrontRequest::Threads { project: None, role: None, query: Default::default() }
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
        // 1.28 front의 예·아니오 답(선택 창 답 없음)과 1.28 host의 질문(선택 창 없음)
        assert_eq!(
            serde_json::from_value::<FrontRequest>(json!({"op":"question_answer","id":"q","choice":"yes"})).unwrap(),
            FrontRequest::QuestionAnswer { id: "q".into(), choice: "yes".into(), menu: None, pick: None }
        );
        let v = json!({"id":"q","pane":"p","machine":"mac-a","project":null,"thread":null,"question_kind":"permission",
            "text":"t","options":["yes","no"],"state":"open","answer":null,"created_at_ms":1});
        assert_eq!(serde_json::from_value::<QuestionView>(v).unwrap().menu, None);
        let v = json!({"op":"question_answer","id":"q","menu":"m","pick":{"by":"options","indices":[0,2]}});
        assert!(matches!(
            serde_json::from_value::<FrontRequest>(v).unwrap(),
            FrontRequest::QuestionAnswer { pick: Some(MenuPick::Options { .. }), .. }
        ));
    }
    #[test]
    fn chat_pages_keep_legacy_requests_and_responses_readable() {
        #[derive(Deserialize)]
        #[serde(tag = "op", rename_all = "snake_case")]
        enum OldRequest {
            PaneChat { pane: String, thinking: bool },
        }
        let old = r#"{"op":"pane_chat","pane":"p","thinking":false}"#;
        let request: FrontRequest = serde_json::from_str(old).unwrap();
        assert!(matches!(request, FrontRequest::PaneChat { before: None, compact: false, commands: false, .. }));
        let new = FrontRequest::PaneChat {
            pane: "p".into(),
            thinking: true,
            compact: true,
            commands: true,
            before: Some("cursor".into()),
        };
        let OldRequest::PaneChat { pane, thinking } =
            serde_json::from_value(serde_json::to_value(new).unwrap()).unwrap();
        assert_eq!(pane, "p");
        assert!(thinking);
        let old_reply =
            r#"{"pane":"p","harness":"claude","messages":[{"role":"assistant","text":"答"}],"truncated":true}"#;
        let legacy: ChatView = serde_json::from_str(old_reply).unwrap();
        assert!(legacy.before.is_none() && legacy.history.is_none() && legacy.messages[0].id.is_none());
        #[derive(Deserialize)]
        struct OldView {
            pane: String,
            messages: Vec<OldMessage>,
            truncated: bool,
        }
        #[derive(Deserialize)]
        struct OldMessage {
            text: String,
        }
        let new = ChatView { before: Some("cursor".into()), history: Some("file".into()), ..legacy };
        let old: OldView = serde_json::from_value(serde_json::to_value(new).unwrap()).unwrap();
        assert_eq!(old.pane, "p");
        assert_eq!(old.messages[0].text, "答");
        assert!(old.truncated);
        let agent: AgentResponse = serde_json::from_str(r#"{"kind":"chat","messages":[],"truncated":true}"#).unwrap();
        assert!(matches!(agent, AgentResponse::Chat { before: None, history: None, .. }));
    }
}
