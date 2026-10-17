//! 오케스트레이션 계약(M1-3): 역할, 프로젝트·thread, profile 해석, 보고 회수, 흐름 사건(§8).
//!
//! - thread 작업 폴더·worktree는 그 머신의 agent가 만든다. host는 git을 직접 실행하지 않고 기록·사본만 갖는다.
//! - 흐름 사건은 프로젝트별 순번(seq)을 가진 durable 기록이다. 구독자는 cursor(마지막 seq)부터 다시 읽는다.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::msg::B64;

/// agent 세션의 역할(§8.13). 새 역할을 더해도 이전 기록이 깨지지 않게 문자열로 저장한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Coordinator,
    #[default]
    Worker,
    /// 사후 평가자. 목록 기본값에서 빠지고 `role` 필터로만 보인다.
    Evaluator,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Coordinator => "coordinator",
            Role::Worker => "worker",
            Role::Evaluator => "evaluator",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "coordinator" => Some(Role::Coordinator),
            "worker" => Some(Role::Worker),
            "evaluator" => Some(Role::Evaluator),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThreadKind {
    /// repo의 새 worktree와 branch(기본)
    #[default]
    Worktree,
    /// repo 없는 작업 폴더(agent 작업 root 아래)
    Dir,
}

/// agent.toml profile을 그 머신에서 해석한 결과. host는 thread 기록에 그대로 저장하고 restart·resume에 다시 쓴다(§7.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Launch {
    pub profile: String,
    /// claude·codex·shell·custom
    pub harness: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    /// 같은 대화를 이어 띄우는 argv(모를 때는 빈 배열). daemon 금지 인자 등은 argv와 같다.
    pub resume_argv: Vec<String>,
    /// harness 세션 ID(미리 정할 수 있는 harness만, 예: claude `--session-id`)
    pub session_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProfileView {
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub description: String,
    /// profile이 붙이는 인자(옛 agent는 보내지 않는다)
    #[serde(default)]
    pub args: Vec<String>,
    /// agent.toml에 없는 기본 profile(claude·codex): 그 머신에 설치·로그인한 것만 온다
    #[serde(default)]
    pub builtin: bool,
}

/// host → agent: thread 자리를 만든다(worktree 또는 작업 폴더)와 brief.md 쓰기, profile 해석
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPrepare {
    pub project: String,
    pub thread: String,
    pub title: String,
    pub kind: ThreadKind,
    /// worktree일 때 그 머신의 repo 경로
    pub repo: Option<String>,
    /// 기준 ref(없으면 origin/HEAD, 현재 branch 순)
    pub base: Option<String>,
    pub profile: String,
    pub brief: String,
    /// thread를 띄울 때 safety(plan §8.115). `launch_safety.v1`을 알린 agent에만 보낸다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety: Option<LaunchSafety>,
}

/// host → agent: thread 인자에 더할 safety(herdr-projects `profiles::launch_args`·`Safety::launch_args`). host는 profile의
/// harness를 모르므로 agent가 profile을 풀고 붙인다.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LaunchSafety {
    /// harness의 권한 건너뛰기 인자(claude `--dangerously-skip-permissions`, codex `--dangerously-bypass-approvals-and-sandbox`)
    pub yolo: bool,
    /// 프로젝트 기본 profile과 같은 harness의 기본 profile에만 붙인다(사람이 정의한 profile은 제 인자를 쓴다)
    pub thread_agent_args: Vec<String>,
    /// 프로젝트 기본 profile 이름: 그 harness가 `thread_agent_args`의 harness다(못 풀면 claude)
    pub default_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadReady {
    /// agent가 시작할 폴더(worktree 또는 작업 폴더)
    pub cwd: String,
    /// `<cwd>/.aaa/<project>-<thread>/`(brief.md·report.md·library/)
    pub work_dir: String,
    pub branch: Option<String>,
    /// 기준 commit
    pub base: Option<String>,
    pub launch: Launch,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibFile {
    pub path: String,
    #[ts(type = "number")]
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
    /// 복사 상한을 넘어 agent에 남긴 일반 파일의 크기(이것만 나중에 지연 스트리밍으로 볼 수 있다)
    #[serde(default)]
    #[ts(type = "number | null")]
    pub lazy_size: Option<u64>,
}

/// thread 결과물 파일 하나(host 사본 또는 agent에 남긴 것)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryFile {
    /// library 안 상대 경로
    pub path: String,
    #[ts(type = "number")]
    pub size: u64,
    /// host에 사본이 없다: 볼 때 agent에서 스트리밍한다(작업 폴더가 남아 있을 때만)
    pub on_agent: bool,
}

/// 웹에서 보는 보고서·결과물. `base`는 짧은 수명의 읽기 전용 경로(`/v/<cap>/`)이고 그 아래에 `path`를 붙인다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryView {
    pub project: String,
    pub thread: String,
    /// 회수한 report.md(없으면 None)
    pub report: Option<String>,
    pub files: Vec<LibraryFile>,
    pub base: String,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
}

/// report.md 본문과 library 목록(파일 본문은 `FileRead`로 나눠 받는다)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSnapshot {
    pub hash: String,
    pub report: B64,
    pub files: Vec<LibFile>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cleanup {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub notes: Vec<String>,
}

// ───────────────────────── harness 상태 사건(M1-3b, §8.3) ─────────────────────────

/// agent harness가 지금 하는 일(hook·notify·`aaa report`로 안다. polling하지 않는다)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivity {
    /// harness가 시작했다(첫 입력 전)
    Started,
    /// 턴 진행 중
    Working,
    /// 턴이 끝나 다음 입력을 기다린다
    Idle,
    /// 확인창·질문에 답을 기다린다
    Question,
    Error,
    Exited,
}

/// agent adapter가 정규화한 harness 사건 하나(agent → host)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessEvent {
    /// aaa 세션(pane) ID
    pub session: String,
    /// claude·codex·report(`aaa report`)
    pub harness: String,
    /// session_start·turn_start·turn_end·question·idle·error·exited·progress
    pub kind: String,
    /// harness 자기 세션 ID(resume에 쓴다)
    pub harness_session: Option<String>,
    /// 사람이 읽는 한 줄(알림 문구, 질문 본문 요약)
    pub message: Option<String>,
    /// 질문 종류(permission·trust …)
    pub question_kind: Option<String>,
    pub percent: Option<u8>,
    pub at_ms: u64,
    /// Claude hook이 알린 대화 기록 파일(§8.27). host는 pane에만 기억하고 웹·로그에 내보내지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AgentStatus {
    pub activity: AgentActivity,
    #[ts(type = "number")]
    pub since_ms: u64,
    pub harness: String,
    pub message: Option<String>,
    /// `aaa report --percent`
    pub percent: Option<u8>,
    /// `aaa report --activity`
    pub progress: Option<String>,
    /// 마지막 `aaa report` 시각. 5분이 지나면 진행 보고로 「작업 중」을 정하지 않는다(herdr-projects `ACTIVITY_TTL_MS`).
    #[serde(default)]
    #[ts(type = "number | null")]
    pub progress_at_ms: Option<u64>,
    pub harness_session: Option<String>,
    /// 마지막으로 쉼(`Idle`)이 된 뒤 사람이 이 pane을 봤다. false면 「끝남(안 봄)」이다(herdr `PaneState::seen`:
    /// 작업·질문에서 쉼으로 끝날 때 보는 화면이 없으면 false, 그 pane을 보면 true).
    #[serde(default = "seen_default")]
    pub seen: bool,
}

fn seen_default() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    Open,
    Answered,
    /// 답하기 전에 harness가 넘어갔다(다른 곳에서 답함, 세션 끝남)
    Gone,
}

/// 확인창 질문(§8.4). 답은 host가 세션·상태를 다시 확인한 뒤 adapter 키 입력으로 보낸다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionView {
    pub id: String,
    pub pane: String,
    pub machine: String,
    pub project: Option<String>,
    pub thread: Option<String>,
    /// permission·trust(FrontResponse 태그 `kind`와 겹치지 않게 이름을 바꿨다)
    pub question_kind: String,
    pub text: String,
    /// 고를 수 있는 답(예: yes·no)
    pub options: Vec<String>,
    pub state: QuestionState,
    pub answer: Option<String>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageMode {
    /// 지금 보낸다(작업 중이면 Esc로 먼저 멈춘다)
    Interrupt,
    /// 바로 입력한다. 작업 중이면 harness가 다음 턴으로 줄 세운다(herdr-projects `thread prompt`). 이름은 옛 판 그대로다
    #[default]
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    /// 기록했고 아직 보내는 중이다
    Queued,
    /// harness가 입력을 받았다(turn_start)
    Accepted,
    /// 받았는지 모른다. 자동으로 다시 보내지 않는다(§8.5 a)
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageView {
    pub id: String,
    pub project: String,
    pub thread: String,
    pub mode: MessageMode,
    pub state: DeliveryState,
    pub text: String,
    #[ts(type = "number")]
    pub created_at_ms: u64,
}

// ───────────────────────── front 쪽 보기 ─────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectView {
    pub name: String,
    pub path: String,
    pub threads: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    /// 자리 만들기·agent 시작 중
    Starting,
    Open,
    Failed,
    Resolved,
}

impl ThreadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThreadStatus::Starting => "starting",
            ThreadStatus::Open => "open",
            ThreadStatus::Failed => "failed",
            ThreadStatus::Resolved => "resolved",
        }
    }

    pub fn parse(s: &str) -> Option<ThreadStatus> {
        match s {
            "starting" => Some(ThreadStatus::Starting),
            "open" => Some(ThreadStatus::Open),
            "failed" => Some(ThreadStatus::Failed),
            "resolved" => Some(ThreadStatus::Resolved),
            _ => None,
        }
    }
}

/// brief 전달 상태. `unknown`이면 자동으로 다시 보내지 않는다(§8.5 a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BriefState {
    Pending,
    Delivered,
    Unknown,
}

impl BriefState {
    pub fn as_str(&self) -> &'static str {
        match self {
            BriefState::Pending => "pending",
            BriefState::Delivered => "delivered",
            BriefState::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<BriefState> {
        match s {
            "pending" => Some(BriefState::Pending),
            "delivered" => Some(BriefState::Delivered),
            "unknown" => Some(BriefState::Unknown),
            _ => None,
        }
    }
}

/// 목록에서 보이는 묶음(coordinator가 무엇을 해야 하나)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadGroup {
    /// 사람이나 coordinator가 할 일이 있다(시작 실패·5분 넘은 시작, 닫힌 pane, brief 전달 불명, 30초 넘은 확인창,
    /// `Waiting for you` 진행 보고). 보고 내용으로는 정하지 않는다(herdr-projects `group`, plan §8.78).
    WaitingOnYou,
    /// 아직 확인(ack)하지 않은 새 보고
    ReadyForReview,
    Working,
    /// 열려 있지만 harness가 쉬고 있고 새 보고도 진행 보고도 없다(herdr-projects `Group::Idle`)
    Idle,
    Resolved,
}

impl ThreadGroup {
    /// 사람이 읽는 이름(CLI·TUI·웹과 같은 말)
    pub fn label(self) -> &'static str {
        match self {
            ThreadGroup::WaitingOnYou => "사람 차례",
            ThreadGroup::ReadyForReview => "검토 대기",
            ThreadGroup::Working => "작업 중",
            ThreadGroup::Idle => "쉼",
            ThreadGroup::Resolved => "끝남",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadView {
    pub project: String,
    pub id: String,
    pub title: String,
    pub machine: String,
    pub profile: String,
    pub role: Role,
    /// (FrontResponse 태그 `kind`와 겹치지 않게 이름을 달리한다)
    pub thread_kind: ThreadKind,
    pub status: ThreadStatus,
    pub group: ThreadGroup,
    pub brief: BriefState,
    pub pane: Option<String>,
    pub cwd: Option<String>,
    pub work_dir: Option<String>,
    pub branch: Option<String>,
    /// 추적하는 PR(§8.80)
    pub pr: Option<Box<ThreadPr>>,
    pub report_hash: Option<String>,
    pub acked_hash: Option<String>,
    /// report.md `## Next` 줄
    pub next: Vec<String>,
    /// 마지막 사본 결과: complete·partial·failed
    pub copy: Option<String>,
    pub error: Option<String>,
    /// agent harness 상태(hook으로 안다)
    pub agent: Option<Box<AgentStatus>>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number")]
    pub updated_at_ms: u64,
    /// `thread start`가 알리는 경고(시작은 했다, 예: 동시 thread 한도). 다른 응답에는 없다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub warnings: Vec<String>,
}

/// thread가 추적하는 PR: 보고 `PR:` 줄이나 branch로 찾은 주소와 마지막으로 본 상태(OPEN·MERGED·CLOSED)·리뷰 결정(§8.80)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadPr {
    pub url: String,
    pub state: Option<String>,
    pub review: Option<String>,
}

/// 흐름 사건(host가 낸다, §8.3). 프로젝트별 순번과 host 시각·boot ID를 가진다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FlowEvent {
    pub project: String,
    #[ts(type = "number")]
    pub seq: u64,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub boot_id: String,
    /// thread_started·brief_delivered·brief_unknown·report_ready·review_acked·thread_resolved·thread_failed …
    pub kind: String,
    pub thread: Option<String>,
    pub role: Option<Role>,
    pub profile: Option<String>,
    pub machine: Option<String>,
    /// 근거 경로(host 프로젝트 폴더 기준, 예: `threads/t-0001.md`)
    pub evidence: Option<String>,
    /// 사람이 읽는 한 줄
    pub detail: String,
}

/// report.md `## Next` 절의 줄: 한 줄에 권하는 행동 하나, 목록 표시는 빼고 빈 줄은 버린다
/// (herdr-projects `thread::next_lines`를 옮겼다). 첫 줄 상태 태그는 읽지 않는다(plan §8.78).
pub fn next_lines(report: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in report.lines() {
        if line.starts_with("## ") {
            inside = line.trim() == "## Next";
            continue;
        }
        if !inside || line.starts_with('#') {
            if line.starts_with('#') {
                inside = false;
            }
            continue;
        }
        let text = line.trim();
        let text = text
            .strip_prefix("- ")
            .or_else(|| text.strip_prefix("* "))
            .or_else(|| {
                text.split_once(". ").filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())).map(|(_, rest)| rest)
            })
            .unwrap_or(text)
            .trim();
        if !text.is_empty() {
            lines.push(text.to_string());
        }
    }
    lines
}

/// brief.md에서 그 글을 쓰는 agent가 채우는 자리(plan §8.78). host는 agent가 작업 폴더를 만들기 전에 brief를 짓는다.
/// 작업 폴더(`<cwd>/.aaa/<project>-<thread>`)의 절대 경로
pub const BRIEF_WORK_DIR: &str = "{{aaa:work_dir}}";
/// 그 머신 aaa 실행 파일(진행 보고 명령)
pub const BRIEF_EXE: &str = "{{aaa:exe}}";

/// brief 자리를 채운다. 실행 파일 경로에 빈칸이 있으면 큰따옴표로 감싼다.
pub fn fill_brief(brief: &str, work_dir: &str, exe: &str) -> String {
    let exe = if exe.contains(char::is_whitespace) { format!("\"{exe}\"") } else { exe.to_string() };
    brief.replace(BRIEF_WORK_DIR, work_dir).replace(BRIEF_EXE, &exe)
}

/// inbox 항목(0.2.35 inbox 계승, §8.9). DB가 원본이고 `projects/<p>/inbox/<id>.md`는 투영이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InboxItem {
    /// `<UTC 시각>-<종류>-<대상>-<순번>`
    pub id: String,
    pub project: String,
    /// report·question·delivery·thread·session·outage·health·config-error·routine·routine-approval
    pub item_kind: String,
    /// thread ID, 머신 이름, 파일 이름 등
    pub subject: String,
    /// 짧은 사건 이름(nudge 문구에 쓴다)
    pub event: String,
    /// 사람이 읽는 한 줄(nudge 문구에는 넣지 않는다)
    pub summary: String,
    /// 긴 본문(routine 안내·명령 출력). 투영 파일의 front matter 아래에 쓴다.
    #[serde(default)]
    pub body: String,
    pub done: bool,
    /// coordinator(또는 사람)가 `aaa inbox`로 본 적이 있다. 처음 보는 항목만 알린다.
    pub seen: bool,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number | null")]
    pub done_at_ms: Option<u64>,
}

/// routine 하나(`projects/<p>/routines/<name>.md`, 0.2.35 계승)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoutineView {
    pub name: String,
    /// `every 30m`·`daily 09:00`(파일에 적힌 글)
    pub schedule: String,
    /// 비면 안내만 하는 routine
    pub command: String,
    /// 명령을 실행할 머신(없으면 host 머신)
    pub machine: Option<String>,
    pub enabled: bool,
    /// host.toml `[projects] routine_commands`가 이 프로젝트를 허용한다
    pub commands_allowed: bool,
    /// 지금 명령(머신 포함)이 승인돼 있다
    pub approved: bool,
    pub command_sha256: String,
    #[ts(type = "number | null")]
    pub last_run_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub next_run_ms: Option<u64>,
    /// 안내 항목을 아직 처리하지 않아 건너뛴 횟수
    pub skipped: u32,
    /// coordinator가 없어 건너뛴 횟수(돌면 0)
    pub no_coordinator: u32,
    /// 파일을 읽지 못했다
    pub error: Option<String>,
}

/// 병합 대기열 한 칸(§8.8). M1-3은 순번과 사건만 두고 단계 실행은 M3에서 한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeEntry {
    pub project: String,
    pub thread: String,
    /// 1부터. 1이 맨 앞(지금 병합할 차례)이다.
    pub position: u32,
    pub title: String,
    pub machine: String,
    pub branch: Option<String>,
    #[ts(type = "number")]
    pub enqueued_at_ms: u64,
}

/// 병합 단계 하나의 결과(§8.24). 단계 이름은 `확인`·`fetch`·`원격 일치`·`병합`·`tree 같음`·`push`·`사용자 checkout`·`정리`다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeStep {
    pub name: String,
    pub ok: bool,
    /// 사람이 읽는 설명(실패면 까닭)
    pub note: String,
    #[ts(type = "number")]
    pub ms: u64,
}

/// agent가 병합 단계를 돌린 결과. `commit`은 push한 병합 commit이다(push까지 갔을 때만).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LandOutcome {
    pub ok: bool,
    pub steps: Vec<MergeStep>,
    pub commit: Option<String>,
}

/// `aaa merge land` 결과
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeLandView {
    pub project: String,
    pub thread: String,
    pub ok: bool,
    pub steps: Vec<MergeStep>,
    pub commit: Option<String>,
    /// 「main 이동」을 대기열 메시지로 알린 thread
    pub notified: Vec<String>,
    /// 병합 뒤 대기열
    pub queue: Vec<MergeEntry>,
}

/// 프로젝트 이름: 영문 소문자·숫자·`-`·`_`(경로에 쓰므로 엄격히 본다)
pub fn valid_project(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !s.starts_with('-')
}

/// thread ID: `t-` 다음 숫자 4자리 이상
pub fn valid_thread(s: &str) -> bool {
    s.strip_prefix("t-").is_some_and(|n| n.len() >= 4 && n.chars().all(|c| c.is_ascii_digit()))
}

pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut s: String = out.trim_matches('-').chars().take(40).collect();
    while s.ends_with('-') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 응답·사건 enum 태그(`kind`·`ev`)와 안에 든 구조체 필드 이름이 겹치면 읽을 수 없다
    #[test]
    fn tagged_views_round_trip() {
        use crate::{FrontEvent, FrontResponse};
        let q = QuestionView {
            id: "q-1".into(),
            pane: "p".into(),
            machine: "mac-a".into(),
            project: Some("demo".into()),
            thread: Some("t-0001".into()),
            question_kind: "trust".into(),
            text: "Do you trust".into(),
            options: vec!["yes".into(), "no".into()],
            state: QuestionState::Open,
            answer: None,
            created_at_ms: 1,
        };
        let m = MessageView {
            id: "m-1".into(),
            project: "demo".into(),
            thread: "t-0001".into(),
            mode: MessageMode::Interrupt,
            state: DeliveryState::Accepted,
            text: "x".into(),
            created_at_ms: 1,
        };
        for r in [FrontResponse::Question(q.clone()), FrontResponse::Message(m)] {
            let j = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<FrontResponse>(&j).unwrap(), r, "{j}");
        }
        let e = FrontEvent::Question(q);
        let j = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<FrontEvent>(&j).unwrap(), e, "{j}");
    }

    #[test]
    fn names_and_slug() {
        assert!(valid_project("aaa-dev_1") && !valid_project("../x") && !valid_project("A") && !valid_project(""));
        assert!(valid_thread("t-0001") && valid_thread("t-12345") && !valid_thread("t-1") && !valid_thread("t-00a1"));
        assert_eq!(slug("Fix the Login bug!"), "fix-the-login-bug");
        assert_eq!(slug("한글 제목"), "");
        assert_eq!(slug("M1-3 Thread: 시작"), "m1-3-thread");
    }

    /// herdr-projects `next_lines_come_from_the_next_section_only`를 옮겼다
    #[test]
    fn next_lines_come_from_the_next_section_only() {
        let report = "PR: https://github.com/o/r/pull/1\n## Report\n- not this\n## Next\n- Merge the PR\n* Fix CI\n3. Confirm assumption X\n\n## Remember\n- nor this\n";
        assert_eq!(next_lines(report), ["Merge the PR", "Fix CI", "Confirm assumption X"]);
        assert!(next_lines("## Report\nnothing\n").is_empty());
        assert!(next_lines("## Next\n").is_empty());
        // `#` 머리가 오면 Next가 끝난다
        assert_eq!(next_lines("## Next\n- 하나\n# 다른 절\n- 둘\n"), ["하나"]);
    }

    #[test]
    fn fill_brief_puts_the_machine_paths() {
        let b = format!("`{BRIEF_WORK_DIR}/report.md` `{BRIEF_EXE} report --unknown`");
        assert_eq!(
            fill_brief(&b, "/w/.aaa/p-t-0001", "/opt/aaa"),
            "`/w/.aaa/p-t-0001/report.md` `/opt/aaa report --unknown`"
        );
        assert_eq!(fill_brief(BRIEF_EXE, "", r"C:\Program Files\aaa.exe"), r#""C:\Program Files\aaa.exe""#);
    }
}

/// TASKS.md의 task 한 줄(0.2.35 형식). aaa는 읽기만 한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskView {
    /// 속한 `## ` 목록 이름
    pub list: String,
    pub title: String,
    /// 괄호 안 owner 원문(`me`, `claude`, `codex@mac-b`, 사람 이름 등)
    pub owner: Option<String>,
    /// ` · t-0001`로 붙은 thread
    pub thread: Option<String>,
    /// task 줄 아래 들여쓴 메모
    pub notes: Vec<String>,
    pub done: bool,
}

/// 머신 하나에서 thread를 맡길 수 있는 profile(host 허용 목록과 그 머신 agent.toml의 교집합)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OwnerChoice {
    pub machine: String,
    pub connected: bool,
    /// 끊긴 머신은 비어 있다(모른다)
    pub profiles: Vec<String>,
}

/// coordinator가 매 턴 처음 보는 프로젝트 요약(0.2.35 `context`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectContext {
    pub project: String,
    pub path: String,
    /// project.toml `start_threads`: propose(제안하고 승인을 기다린다) 또는 auto. aaa는 강제하지 않는다.
    pub start_threads: String,
    pub tasks: Vec<TaskView>,
    /// 열린 thread. `next`는 보고서 `## Next`에 `aaa thread next --add`로 더한 줄까지 붙인 목록이다.
    pub threads: Vec<ThreadView>,
    pub assignable: Vec<OwnerChoice>,
    /// 끝내지 않은 inbox 항목 수
    pub inbox_open: u32,
    /// 끝내지 않은 inbox 항목(herdr-projects `context`의 Inbox 절). 지시가 아니라 자료다.
    /// 응답 크기 한도 안에서 오래된 것부터 싣는다: `inbox_open`보다 적을 수 있다.
    #[serde(default)]
    pub inbox: Vec<InboxItem>,
    /// 사후 평가자(지정했을 때만)
    #[serde(default)]
    pub evaluator: Option<Box<EvaluatorView>>,
}

// ───────────────────────── 사후 평가자(M3, §8.13) ─────────────────────────

/// 사후 평가자 상태. 평가자 thread는 thread 목록에 섞지 않고 이것으로 따로 본다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvaluatorView {
    pub project: String,
    pub thread: ThreadView,
    /// 세션이 돌고 있다(아니면 `thread restart`로 다시 띄운다)
    pub running: bool,
    /// 마지막으로 넘긴 사건 seq
    #[ts(type = "number")]
    pub cursor: u64,
    /// 아직 넘기지 않은 사건 수(평가자 자신의 사건 제외)
    pub pending: u32,
    /// 넘긴 사건 묶음 수
    pub batches: u32,
    #[ts(type = "number | null")]
    pub last_batch_ms: Option<u64>,
    /// 받은 평가 기록 수
    pub evaluations: u32,
    /// 채택·보류를 정하지 않은 개선안 수
    pub undecided: u32,
}

/// 평가자가 쓰는 평가 기록(`library/evaluations/<이름>.json`). host가 검사해 받는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvaluationDoc {
    /// 평가한 thread ID
    #[serde(default)]
    pub targets: Vec<String>,
    /// 근거로 읽은 사건 묶음 파일 이름
    #[serde(default)]
    pub batches: Vec<String>,
    /// 흐름 효율 판정 한두 줄
    pub summary: String,
    /// 단계별 시간
    #[serde(default)]
    pub stages: Vec<EvalStage>,
    /// 재작업(횟수·원인)
    #[serde(default)]
    pub rework: Vec<String>,
    /// 막힌 곳(기다린 것과 그 시간)
    #[serde(default)]
    pub blocked: Vec<String>,
    #[serde(default)]
    pub tool_issues: Vec<String>,
    /// 결함을 누가 어느 단계에서 잡았나
    #[serde(default)]
    pub defects: Vec<EvalDefect>,
    #[serde(default)]
    pub proposals: Vec<Proposal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvalStage {
    pub thread: String,
    pub stage: String,
    /// 걸린 시간
    #[serde(default)]
    pub minutes: Option<Minutes>,
}

/// 분. 평가자가 쓴 소수는 반올림해 받는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
pub struct Minutes(pub u32);

impl<'de> Deserialize<'de> for Minutes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = f64::deserialize(d)?;
        if !(0.0..=1e6).contains(&m) {
            return Err(serde::de::Error::custom(format!("minutes 범위 밖: {m}")));
        }
        Ok(Minutes(m.round() as u32))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvalDefect {
    pub what: String,
    /// 잡은 쪽(사람·coordinator·thread·CI·E2E …)
    #[serde(default)]
    pub found_by: String,
    #[serde(default)]
    pub stage: String,
}

/// 개선안 하나. `decision`은 host가 채운다(평가자가 쓴 값은 버린다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Proposal {
    pub id: String,
    pub text: String,
    /// 예상 효과
    #[serde(default)]
    pub effect: String,
    /// 대상 도구
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub decision: Option<ProposalDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProposalDecision {
    pub adopted: bool,
    pub by: String,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub note: Option<String>,
}

/// 받은 평가 기록 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvaluationView {
    pub project: String,
    /// 평가자가 정한 파일 이름(프로젝트 안에서 하나)
    pub id: String,
    /// 평가자 thread ID
    pub evaluator: String,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    /// 사람이 읽는 본문(프로젝트 폴더 기준 `evaluations/<id>.md`)
    pub file: String,
    pub doc: EvaluationDoc,
}

/// 평가 기록 이름(파일 이름): 영문·숫자·`-`·`_`, 64자까지
pub fn valid_evaluation_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
