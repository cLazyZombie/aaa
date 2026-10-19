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
    /// 사람이 이미 띄운 agent pane을 thread로 기록했다(`thread adopt`, herdr-projects `Kind::Adopted`). 자리를 만들지 않고
    /// 그 pane이 있는 폴더에서 일한다. pane은 사람 것이라 다시 띄우지도 끝내지도 않는다
    Adopted,
}

/// host profile에 그 머신 실행 경로·환경·hook을 적용한 결과. host는 thread 기록에 그대로 저장하고 restart·resume에 다시 쓴다(§7.2).
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
    /// host.toml에 없는 기본 profile(claude·codex): 그 머신에 설치·로그인한 것만 온다
    #[serde(default)]
    pub builtin: bool,
}

/// host → agent: thread 자리를 만든다(worktree 또는 작업 폴더)와 brief.md 쓰기, profile 해석
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPrepare {
    /// host가 푼 profile. host_profiles.v1 기능이 있는 agent에만 보낸다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_profile: Option<ResolvedProfile>,
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
    /// thread를 띄울 때 safety(plan §8.163). `launch_safety.v1`을 알린 agent에만 보낸다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety: Option<LaunchSafety>,
}

/// host에서 해석한 실행 설정. command·env는 옛 shell/custom profile의 호환 필드이며 머신 덮어쓰기가 우선이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedProfile {
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub args: Vec<String>,
    pub command: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
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

/// agent journal에서 회수한 자리. 아직 launch를 만들지 못한 준비도 정리에 넘길 수 있다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPlacement {
    pub cwd: String,
    pub work_dir: String,
    pub branch: Option<String>,
    pub base: Option<String>,
}

/// 사람이 띄운 agent pane을 thread로 데려온다(`thread adopt`): 그 세션이 지금 있는 폴더에 작업 폴더를 만들고 brief.md를 쓴다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadAdopt {
    pub project: String,
    pub thread: String,
    /// 데려올 세션(pane) ID
    pub session: String,
    pub brief: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadAdopted {
    /// 세션이 지금 있는 폴더
    pub cwd: String,
    /// `<cwd>/.aaa/<project>-<thread>/`(brief.md·report.md·library/)
    pub work_dir: String,
    /// cwd가 git repo 안이면 그 top
    pub repo: Option<String>,
    /// 지금 branch(떼어 낸 HEAD면 None)
    pub branch: Option<String>,
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
    /// 파일 보기가 고를 MIME·종류(파일 보기 `FileInfo`와 같은 표). 옛 목록 파일에는 없어 host가 줄 때 채운다
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub kind: crate::msg::FileKind,
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
    /// FRONT_API 1.36. 조작 권한에만 보이는 보고서 공유 링크. 앱 안 보기는 만료·폐기와 무관하다.
    #[serde(default)]
    pub link: Option<LinkView>,
}

/// FRONT_API 1.36. 보고서 하나의 host 사본만 읽는 서명 링크(§8.197).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LinkView {
    pub id: String,
    pub project: String,
    pub thread: String,
    pub base: String,
    /// 공개했을 때만 있는 절대 HTTPS 주소. loopback은 밖 알림에 싣지 않는다.
    pub url: Option<String>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
    #[ts(type = "number | null")]
    pub last_opened_at_ms: Option<u64>,
    pub revoked: bool,
    /// host에 회수한 일반 파일의 합계 바이트
    #[ts(type = "number")]
    pub size: u64,
}

/// FRONT_API 1.38. thread 결과물과 별도로 올린 보고서의 전체 목록 한 줄.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryReport {
    pub project: String,
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub updated_at_ms: u64,
    pub live: bool,
    /// 읽기 실패·상한 초과도 목록에서 감추지 않는다.
    pub problem: Option<String>,
    pub link: Option<LinkView>,
    /// FRONT_API 1.43. coordinator가 사용자에게 보낸 웹 보고서(`library publish --to-user`, 「내 보고서」). 아니면 작업 보고다.
    #[serde(default)]
    pub to_user: bool,
    /// FRONT_API 1.43. `--to-user`로 보낼 때 준 제목
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    /// FRONT_API 1.43. 사용자 보고서를 보낸(다시 보낸) 뒤 아직 열지 않았다
    #[serde(default)]
    pub unseen: bool,
    /// FRONT_API 1.43. 끝나지 않은(resolve 전) thread의 사본이라 지울 수 없다
    #[serde(default)]
    pub busy: bool,
}

/// 보고서 하나(프로젝트·이름). FRONT_API 1.43 `LibraryDelete`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryRef {
    pub project: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibrarySkip {
    pub name: String,
    pub reason: String,
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
    /// session_start·turn_start·turn_end·question·idle·error·exited·progress·released(셸 앞 agent를 놓음)
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
    /// `kind = "plan"`일 때만: agent가 대화 기록에서 읽은 계획(None이면 지금 계획이 없다, 차이 목록 S5)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanSummary>,
    /// `kind = "plan"`일 때만: 아직 도는 배경 작업(하위 agent·배경 명령) 수. 0이면 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<u32>,
}

/// pane 계획의 요약(herdr-web-ui `PlanSummary`): 끝난 단계 수·모든 단계 수·지금 하는 단계
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PlanSummary {
    pub done: u32,
    pub total: u32,
    pub current: Option<String>,
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
    /// harness가 기록에 남긴 계획(Claude TaskCreate·TaskUpdate, Codex update_plan). 없으면 null(차이 목록 S5)
    #[serde(default)]
    pub plan: Option<PlanSummary>,
    /// 아직 도는 배경 작업 수(Claude 하위 agent·배경 명령). 없으면 null
    #[serde(default)]
    pub background: Option<u32>,
    /// 쉬지만 그 턴이 띄운 배경 작업을 기다린다(herdr-web-ui `background_wait`, host `bgwait.rs`, 차이 목록 S1): 목록은 끝남·쉼
    /// 대신 BG다. 옛 host는 보내지 않는다(없으면 false)
    #[serde(default)]
    pub background_wait: bool,
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

/// 화면에서 읽은 선택 창(herdr-web-ui `InteractivePrompt`, plan §8.172). `id`는 내용(커서·체크 상태 빼고)의 hash라,
/// 답할 때 화면을 다시 읽어 다르면 거절한다(그 사이 바뀐 질문에 잘못 답하지 않는다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionMenu {
    pub id: String,
    /// question(AskUserQuestion)·approval(도구 승인)·plan(계획 승인)·menu(여러 질문의 검토)
    pub kind: String,
    pub title: String,
    pub question: String,
    /// 참고 글(명령·계획·검토할 답)
    pub body: Option<String>,
    pub options: Vec<MenuOption>,
    pub multi_select: bool,
    /// 「직접 쓰기」 자리. `options.len()`이면 그 줄은 `options`에 없다(Claude 질문의 「Type something.」)
    pub custom_option_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MenuOption {
    pub label: String,
    pub description: Option<String>,
}

/// 선택 창 답(참고 `PromptAnswer`): 하나 고르기·여러 개 고르기·직접 쓰기 가운데 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum MenuPick {
    Option { index: u32 },
    Options { indices: Vec<u32> },
    Custom { text: String },
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
    /// 화면에서 읽은 선택 창(못 읽었으면 None: 그때는 `options`의 예·아니오로 답한다)
    #[serde(default)]
    pub menu: Option<QuestionMenu>,
}

/// 앱 안 알림의 종류(plan §8.185). 휴대폰 push는 아니다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// 배포의 최종 성공/중단. 웹 앱 안 알림만 보낸다.
    RolloutFinished,
    RolloutFailed,
    /// 보는 화면 없이 agent의 턴이 끝났다(herdr `ToastKind::Finished`: 작업·질문에서 쉼으로, 안 봄)
    Finished,
    /// post 남용 한도에 걸린 출처가 음소거됐다. 앱 안에서만 알린다.
    PostMuted,
    /// 규칙이 명시한 앱 안 안내. 외부 push가 아니다.
    Automation,
    /// thread의 새 보고(herdr-projects 「review · new report」 알림, host 「new report」 항목과 같은 때)
    Report,
}

/// 앱 안 알림 하나(host가 front마다 보낸다, `FrontEvent::Notice`). 입력 필요는 따로 없고 `Question` 사건이 맡는다.
/// 같은 턴의 끝남과 새 보고는 새 보고 하나다(plan §8.185).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NoticeView {
    pub kind: NoticeKind,
    /// 갈 pane(새 보고는 그 thread의 pane, 없으면 null)
    pub pane: Option<String>,
    pub machine: String,
    pub project: Option<String>,
    pub thread: Option<String>,
    /// thread 제목 또는 pane 이름
    pub title: String,
    /// harness 이름(claude·codex, 끝남만)
    pub agent: Option<String>,
    /// 끝난 턴의 길이(작업·질문으로 들어선 때부터). host가 그 턴의 시작을 못 봤으면 null
    #[ts(type = "number | null")]
    pub turn_ms: Option<u64>,
    /// 긴 턴(herdr-web-ui `LONG_TURN_MS` 60초 이상이거나 길이를 모름): 웹 앱 안 알림은 이것만 띄운다
    pub long: bool,
    #[ts(type = "number")]
    pub at_ms: u64,
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
    /// FRONT_API 1.33. 옛 host는 보내지 않는다(active)
    #[serde(default)]
    #[ts(optional, as = "Option<ProjectStatus>")]
    pub status: ProjectStatus,
    /// `project open`이 profile 없이 쓸 coordinator profile(project.toml → host.toml `[defaults]` → claude로 푼 것).
    /// FRONT_API 1.33. 옛 host는 보내지 않는다(빈 글)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub coordinator_profile: String,
    /// coordinator가 쓸 수 있는 profile(safety `coordinator_profiles`, 사람만 바꾼다). 없으면 모두다. 웹 「coordinator 열기」가
    /// 고르기 목록을 거른다(참고 popup `profile_picker`의 `usable`). FRONT_API 1.33
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub coordinator_profiles: Option<Vec<String>>,
}

/// 프로젝트 상태(herdr-projects `project::Status`). paused·archived는 ticker가 건너뛰고 `thread start`를 거부한다.
/// archived는 목록에서 숨고 `project open`을 거부한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    #[default]
    Active,
    Paused,
    Archived,
}

impl ProjectStatus {
    pub fn is_active(&self) -> bool {
        *self == ProjectStatus::Active
    }
}

/// `project pause|resume|archive|unarchive`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAction {
    Pause,
    Resume,
    Archive,
    Unarchive,
}

/// 프로젝트 관리 명령(상태·삭제·이름·청소)의 결과: 바뀐 뒤 프로젝트와 사람이 읽을 줄
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectChange {
    pub project: ProjectView,
    pub notes: Vec<String>,
}

/// `doctor files`의 프로젝트 한 줄(herdr-projects `doctor`의 `files <slug>`, FRONT_API 1.34)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectFilesCheck {
    pub project: String,
    /// 본 때 고칠 것(AGENTS.md·uploads/·routines/pr-followup.md). 비면 맞다
    pub problems: Vec<String>,
    /// `fix`로 고쳤다
    pub fixed: bool,
    /// 고치지 못한 까닭
    pub error: Option<String>,
}

/// `project open`의 결과(herdr-projects `open`, FRONT_API 1.33)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectOpened {
    pub project: ProjectView,
    /// 띄운 coordinator, 또는 이미 돌던 것 중 가장 최근에 상태가 바뀐 것
    pub pane: crate::PaneView,
    /// 새로 띄웠다(false면 돌던 coordinator)
    pub started: bool,
    /// `pane` 말고도 도는 coordinator 수
    pub others: u32,
    /// coordinator를 띄운 profile(돌던 것이면 그 pane의 harness)
    pub profile: String,
    /// 그 pane이 든 workspace(layout에 없으면 빈 글)
    pub workspace: String,
    pub notes: Vec<String>,
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
    /// 아직 확인(ack)하지 않은 새 보고, 또는 보고가 있고 PR이 열려 있다
    ReadyForReview,
    /// PR이 열려 있고 승인됐다(herdr-projects `Group::Landing`). thread가 자기 도구로 병합한다(D11)
    Landing,
    Working,
    /// 열려 있지만 harness가 쉬고 있고 새 보고도 진행 보고도 없다(herdr-projects `Group::Idle`)
    Idle,
    Resolved,
}

impl ThreadGroup {
    /// 사람이 읽는 이름(CLI·웹과 같은 말)
    pub fn label(self) -> &'static str {
        match self {
            ThreadGroup::WaitingOnYou => "확인 필요",
            ThreadGroup::ReadyForReview => "검토 대기",
            ThreadGroup::Landing => "병합 중",
            ThreadGroup::Working => "작업 중",
            ThreadGroup::Idle => "쉼",
            ThreadGroup::Resolved => "끝남",
        }
    }

    /// 보이는 차례(herdr-projects `Group::rank`·`DISPLAY_ORDER`): 확인 필요 1 → 검토 2 → 병합 중 3 → 작업 4 → 쉼 5 → 끝남 6.
    /// 목록(웹)·popup·overview가 같은 차례다. 묶음을 정하는 우선순위(host `group`)와는 따로다.
    pub fn rank(self) -> u8 {
        match self {
            ThreadGroup::WaitingOnYou => 1,
            ThreadGroup::ReadyForReview => 2,
            ThreadGroup::Landing => 3,
            ThreadGroup::Working => 4,
            ThreadGroup::Idle => 5,
            ThreadGroup::Resolved => 6,
        }
    }
}

/// 검증한 보고 사본 첫 줄의 알림 의도. 묶음·진행률과 별개다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReportAttention {
    Completed,
    NeedsUser,
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
    /// 명시한 실행 대기 표지. timing이면 그 머신의 측정이 끝날 때까지 자동 시작이 기다릴 수 있다.
    #[serde(default)]
    pub holds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mission: Option<String>,
    pub status: ThreadStatus,
    /// 끝남 기록 뒤 아직 정리 응답을 확정하지 못했다. 같은 resolve로 이어간다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cleanup_pending: Option<bool>,
    pub group: ThreadGroup,
    pub brief: BriefState,
    pub pane: Option<String>,
    pub cwd: Option<String>,
    pub work_dir: Option<String>,
    pub branch: Option<String>,
    /// 추적하는 PR(§8.80)
    pub pr: Option<Box<ThreadPr>>,
    pub report_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_attention: Option<ReportAttention>,
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
    /// 그 프로젝트의 상태(FRONT_API 1.33, 진행 중이면 없다). 상태를 바꾸면 그 프로젝트 thread 사건을 다시 내서 front가
    /// 다시 묻지 않고 멈춤·보관을 안다(웹 sidebar의 「멈춤」, 보관한 프로젝트 숨김)
    #[serde(default, skip_serializing_if = "ProjectStatus::is_active")]
    #[ts(as = "Option<ProjectStatus>", optional)]
    pub project_status: ProjectStatus,
    /// 작업 요약 표지(FRONT_API 1.44, plan §8.259): 요약을 쓴 때와 그 뒤 반영 안 된 지시 수. 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub summary: Option<SummaryMark>,
    /// thread가 일하는 저장소의 그 머신 경로(FRONT_API 1.44): worktree thread는 시작할 때 고른 repo, 데려온 thread는 그 폴더의
    /// git top. 작업 폴더 thread와 옛 기록은 없다. 웹 「작업」 트리가 마지막 폴더 이름으로 묶는다(머신마다 경로가 달라도 한 폴더)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repo: Option<String>,
    /// 사용량 한도(FRONT_API 1.44, plan §8.267): 이 thread harness의 구독 창이 다 찼으면 그 창이 다시 차오르는 때(ms).
    /// host 머신의 thread이고 그 구독에 계정이 하나일 때만 안다. 다 차지 않았거나 모르면 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub limited_until_ms: Option<u64>,
}

/// thread 작업 요약 표지(plan §8.259). 줄·머리 단추가 요약을 읽지 않고 「낡았다」를 안다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SummaryMark {
    /// 요약을 마지막으로 쓴 때. 요약 파일이 없으면 None
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
    /// 요약을 쓴 뒤 task.md Follow-ups에 쌓인 지시 수(요약이 없으면 Follow-ups 전부)
    pub pending: u32,
}

/// 작업 요약의 한 줄: 앞의 「누가 언제」(`사용자 15:0x`)와 줄 긋기(`~~…~~`, 뒤에 뒤집혀 무효)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SummaryEntry {
    /// 사용자·coordinator·agent
    pub who: Option<String>,
    /// 적힌 그대로의 시각(`15:0x`, `12:2x~14:5x`)
    pub at: Option<String>,
    pub text: String,
    pub dropped: bool,
}

/// task.md Follow-ups 하나(첫 줄만)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FollowUpLine {
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
    pub text: String,
}

/// thread 작업 요약(`threads/<id>.summary.md`, plan §8.259). coordinator가 쓴 요약과 aaa가 붙인 것(agent 보고의
/// `## Decisions`, 요약 뒤 지시, 요약이 없을 때 할 일 첫 줄)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadSummaryView {
    pub project: String,
    pub thread: String,
    /// 요약 파일 글(없으면 None)
    pub text: Option<String>,
    pub goal: Option<String>,
    /// 목표가 바뀌었을 때 처음 시킨 것
    pub origin: Option<String>,
    pub now: Vec<String>,
    pub decided: Vec<SummaryEntry>,
    pub changed: Vec<SummaryEntry>,
    pub done: Vec<String>,
    /// thread 보고 `## Decisions` 줄(agent가 정한 것)
    pub decisions: Vec<String>,
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
    /// 요약을 쓴 쪽(coordinator·사람)
    pub by: Option<String>,
    /// Follow-ups 전체 수
    pub follow_ups: u32,
    /// 요약 뒤 지시 수와 그 마지막 몇 개(요약이 없으면 Follow-ups 마지막 몇 개)
    pub pending: u32,
    pub pending_lines: Vec<FollowUpLine>,
    /// 요약이 없을 때 보이는 할 일 첫 줄
    pub task: Option<String>,
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
    /// thread 없는 감시 사건의 저장소. 옛 기록은 없음이다.
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
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
    section_lines(report, "Next")
}

/// report.md `## <name>` 절의 목록 줄(`next_lines`와 같은 규칙). `## Decisions`도 이것으로 읽는다(plan §8.259)
pub fn section_lines(report: &str, name: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in report.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            inside = head.trim() == name;
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
/// 임무 inbox의 목록 조회 한 명령. agent가 채우는 경로와 allow-list가 같은 quoting을 쓴다.
pub const BRIEF_INBOX_LIST: &str = "{{aaa:inbox_list}}";
pub const BRIEF_GIT_READS: &str = "{{aaa:git_reads}}";

pub fn mission_inbox_list_command(work_dir: &str) -> String {
    let path = format!("{}/inbox", work_dir.replace('\\', "/").trim_end_matches('/'));
    format!("ls -1 -- '{}'", path.replace('\'', "'\\''"))
}

/// 자기 worktree와 읽기 옵션을 고정한다. 가변 ref/path는 옵션 종료 표지 뒤에서만 받는다.
pub fn mission_git_read_commands(work_dir: &str) -> Vec<String> {
    let path = work_dir.replace('\\', "/");
    let Some((cwd, leaf)) = path.rsplit_once("/.aaa/") else { return vec![] };
    if cwd.is_empty() || leaf.is_empty() || leaf.contains('/') || cwd.contains('*') {
        return vec![];
    }
    let git = format!(
        "git -C '{}' --no-pager --no-optional-locks -c core.fsmonitor=false -c log.showSignature=false",
        cwd.replace('\'', "'\\''")
    );
    vec![
        format!("{git} log --no-ext-diff --no-textconv --oneline --max-count=100 --end-of-options"),
        format!("{git} show --no-ext-diff --no-textconv --format=fuller --end-of-options"),
        format!("{git} status --short --"),
    ]
}

/// brief 자리를 채운다. 실행 파일 경로에 빈칸이 있으면 큰따옴표로 감싼다.
pub fn fill_brief(brief: &str, work_dir: &str, exe: &str) -> String {
    let exe = if exe.contains(char::is_whitespace) { format!("\"{exe}\"") } else { exe.to_string() };
    brief
        .replace(
            BRIEF_GIT_READS,
            &mission_git_read_commands(work_dir).iter().map(|s| format!("- `{s}`")).collect::<Vec<_>>().join("\n"),
        )
        .replace(BRIEF_INBOX_LIST, &mission_inbox_list_command(work_dir))
        .replace(BRIEF_WORK_DIR, work_dir)
        .replace(BRIEF_EXE, &exe)
}

/// 진행 보고 안내(herdr-projects `progress::guidance`를 옮김). thread brief(host)와 Claude SessionStart hook의 안내(agent)가
/// 함께 쓴다(참고도 둘 다 이 글이다). `prefix`는 그 머신 aaa 실행 파일이다.
pub fn progress_guidance(prefix: &str) -> String {
    format!(
        "사용자의 지금 일 전체가 얼마나 됐는지 `{prefix} report`로 알린다. 이것은 타이머나 도구 수가 아니라 너의 어림이다. 보고가 실패해도 실제 일을 멈추지 않는다: 짧은 진단 한 줄만 남기고 다시 하지 않고 이어 간다.\n\n\
         5 단위로 어림한 백분율과 두~네 단어의 활동을 보고한다. 활동은 `코드 읽기`, `변경 시험`, `Waiting for you`처럼 쓴다:\n\n\
         `{prefix} report --percent 25 --activity '코드 읽기'`\n\
         `{prefix} report --unknown --activity '일 가늠'`\n\n\
         새 일을 시작할 때, 의미 있는 고비를 넘긴 뒤, 활동이 바뀔 때, 막혔을 때, 그리고 내용 있는 답을 하기 전마다 보고한다. 사용자에게 무언가 물으려고 멈출 때는 늘 `--activity 'Waiting for you'`로 보고한다(이 말 그대로). 일하는 동안에는 자연스러운 도구 경계에서 1분에 한 번쯤 보고한다. 알림을 채우려고 진행을 지어내지 않는다. 일이 더 있다는 걸 알게 되면 어림을 낮춘다. 범위가 분명하지 않은 동안은 `--unknown`을 쓴다.\n\n\
         `--percent 100`은 요청받은 결과 전체와 그 확인이 다 끝났을 때만 쓴다. `Done`으로 보인다. 그 뒤 일을 더 받으면 새로 낮은 백분율을 보고한다.\n\n\
         외부 푸시(웹 푸시·webhook)는 host가 그 coordinator pane으로 검증한 사용자 post만 보낸다. thread 보고·확인창·세션·머신 사건은 앱 안 알림과 coordinator inbox에만 남는다. coordinator는 맡긴 일 전체와 확인이 끝났을 때, 또는 사람이 직접 해야 하는 인증·터미널 승인·GUI 조작·사용자 결정이 남았을 때만 보낸다. 중간 착지·진행에는 보내지 않는다. thread 표지는 앱 안 알림과 coordinator 판단의 근거이며 푸시를 보내지 않는다. thread는 report.md 첫 줄에 `[완료]`, `[결정 필요]`·`[검수 대기]`·`[확인 필요]`, `[착지]`·`[진행 중]`으로 이를 구분한다. 자동 검증·다른 agent 검토는 사람 대기가 아니다. coordinator는 사용자에게 질문하며 멈추기 전에 위 Waiting for you 보고와 coordinator 안내의 `post user --request --needs-user --topic user-action`를 함께 남긴다. 전체 완료 알림은 `post user --notice --topic task-complete`로 한 번 보낸다.\n\n\
         이 pane의 맨 위 agent만 보고한다. subagent와 도우미는 보고하지 않는다. report 명령은 끝날 때까지 기다리고 배경으로 돌리지 않는다."
    )
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

/// 자동화 한 차례의 결과. 사건 자료·명령 출력 본문은 넣지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoutineRun {
    pub id: String,
    #[ts(type = "number")]
    pub at_ms: u64,
    /// done / skipped / waiting / failed
    pub status: String,
    pub detail: String,
    pub thread: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RoutineView {
    pub title: String,
    pub name: String,
    pub trigger: String,
    pub action: String,
    /// routine 또는 mission. mission:<이름>은 임무 파일의 조건 보기다.
    pub origin: String,
    pub file: String,
    pub last_result: Option<RoutineRun>,
    pub last_skip: Option<RoutineRun>,
    pub today: u32,
    pub max_per_day: Option<u32>,
    /// 최근 순서, 최대 20건. 전체 50건은 routine_runs로 읽는다.
    pub recent: Vec<RoutineRun>,
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

    /// N-1 host의 wire 응답에는 L3 기록 필드가 없다.
    #[test]
    fn routine_view_reads_previous_front_response() {
        let old = r#"{"name":"ping","schedule":"every 10m","command":"","machine":null,"enabled":true,"commands_allowed":false,"approved":false,"command_sha256":"","last_run_ms":1,"next_run_ms":600001,"skipped":2,"no_coordinator":0,"error":null}"#;
        let view: RoutineView = serde_json::from_str(old).unwrap();
        assert_eq!(view.name, "ping");
        assert_eq!(view.schedule, "every 10m");
        assert_eq!(view.next_run_ms, Some(600001));
        assert!(view.recent.is_empty() && view.last_result.is_none());
    }

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
            menu: None,
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
        assert_eq!(fill_brief(BRIEF_INBOX_LIST, "/work/it's a folder", ""), "ls -1 -- '/work/it'\\''s a folder/inbox'");
        assert_eq!(fill_brief(BRIEF_INBOX_LIST, r"C:\work dir\task", ""), "ls -1 -- 'C:/work dir/task/inbox'");
        let reads = fill_brief(BRIEF_GIT_READS, "/work/it's a folder/.aaa/demo-t-0001", "");
        assert!(reads.contains("git -C '/work/it'\\''s a folder' --no-pager --no-optional-locks"));
        assert!(reads.contains("--end-of-options") && reads.contains("status --short --"));
        assert!(mission_git_read_commands("/work/other").is_empty());
        assert!(mission_git_read_commands("/work/*/.aaa/demo-t-0001").is_empty());
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
    /// inbox를 끝내도 해제 전까지 남는 활성 post 음소거 경고
    #[serde(default)]
    pub post_warnings: Vec<String>,
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
    #[serde(default)]
    pub missions: Vec<crate::MissionView>,
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

/// FRONT_API 1.40. 자동 정리와 dry-run이 공유하는 판정 결과(크기는 실시간 원본 제외).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryPrunePlan {
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub remaining_size: u64,
    #[ts(type = "number")]
    pub protected_size: u64,
    #[ts(type = "number")]
    pub max_size: u64,
    #[ts(type = "number")]
    pub max_days: u64,
    pub candidates: Vec<LibraryPruneEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryPruneEntry {
    pub project: String,
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    /// 「기한」 또는 「용량」. 기한 정리 뒤 용량을 판정한다.
    pub reason: String,
}
