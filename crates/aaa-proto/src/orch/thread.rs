//! thread 상태·보기·요약·흐름 사건 타입.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    /// 자리 만들기·agent 시작 중
    Starting,
    Open,
    Failed,
    Resolved,
    /// 보류(FRONT_API 1.105): 자리·pane을 돌려주고 worktree·대화 ID·보고를 남긴다. `thread unpark`로 이어 한다
    Parked,
}

impl ThreadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThreadStatus::Starting => "starting",
            ThreadStatus::Open => "open",
            ThreadStatus::Failed => "failed",
            ThreadStatus::Resolved => "resolved",
            ThreadStatus::Parked => "parked",
        }
    }

    pub fn parse(s: &str) -> Option<ThreadStatus> {
        match s {
            "starting" => Some(ThreadStatus::Starting),
            "open" => Some(ThreadStatus::Open),
            "failed" => Some(ThreadStatus::Failed),
            "resolved" => Some(ThreadStatus::Resolved),
            "parked" => Some(ThreadStatus::Parked),
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
    /// 아직 확인(ack)하지 않은 새 보고. 옛 PR 병합 묶음은 검토로 읽는다.
    #[serde(alias = "landing")]
    ReadyForReview,
    Working,
    /// 열려 있지만 harness가 쉬고 있고 새 보고도 진행 보고도 없다(herdr-projects `Group::Idle`)
    Idle,
    /// 보류(FRONT_API 1.105): 자리·pane 없이 이어 하기를 기다린다
    Parked,
    Resolved,
}

impl ThreadGroup {
    /// 사람이 읽는 이름(CLI·웹과 같은 말)
    pub fn label(self) -> &'static str {
        match self {
            ThreadGroup::WaitingOnYou => "확인 필요",
            ThreadGroup::ReadyForReview => "검토 대기",
            ThreadGroup::Working => "작업 중",
            ThreadGroup::Idle => "쉼",
            ThreadGroup::Parked => "보류",
            ThreadGroup::Resolved => "끝남",
        }
    }

    /// 보이는 차례(herdr-projects `Group::rank`·`DISPLAY_ORDER`): 확인 필요 1 → 검토 2 → 작업 4 → 쉼 5 → 보류 6 → 끝남 7.
    /// 목록(웹)·popup·overview가 같은 차례다. 묶음을 정하는 우선순위(host `group`)와는 따로다.
    pub fn rank(self) -> u8 {
        match self {
            ThreadGroup::WaitingOnYou => 1,
            ThreadGroup::ReadyForReview => 2,
            ThreadGroup::Working => 4,
            ThreadGroup::Idle => 5,
            ThreadGroup::Parked => 6,
            ThreadGroup::Resolved => 7,
        }
    }
}

/// 회수한 보고 첫 줄의 작업 단계. thread 묶음·알림 의도와 별개다(FRONT_API 1.94).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStage {
    MergeReady,
    Landing,
}

impl ThreadStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::MergeReady => "병합 대기",
            Self::Landing => "병합 중",
        }
    }
}

/// 보류 기록(FRONT_API 1.105, plan §8.426). `parked_at_ms`가 없으면 thread에게 정리를 맡기고 `[보류 준비됨]` 보고를 기다린다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadPark {
    pub actor: String,
    #[ts(type = "number")]
    pub requested_at_ms: u64,
    /// 사람이 남긴 까닭(이어 할 때 brief에 들어간다)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
    /// coordinator가 인용한 사용자 지시
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub user_request: Option<String>,
    /// 정리를 맡기지 않고 바로 보류했다
    #[serde(default)]
    pub immediate: bool,
    /// 정리를 맡긴 때의 보고 hash. 그 뒤 새 `[보류 준비됨]` 보고만 정리 끝으로 읽는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub parked_at_ms: Option<u64>,
    /// 보류한 머신 이름(보일 때 쓴다)
    #[serde(default)]
    pub machine: String,
    /// 보류할 때 본 git: HEAD commit, origin의 같은 branch, 커밋 안 한 파일 수. 모르면 `git_unknown`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pushed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uncommitted: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub git_unknown: Option<String>,
    /// 같은 머신에서 같은 대화를 이을 수 있다(대화 ID와 resume 방법을 안다)
    #[serde(default)]
    pub resumable: bool,
}

/// `thread unpark --dry-run`의 머신별 길(FRONT_API 1.105). 같은 머신은 같은 대화, 다른 머신은 push된 branch로 새 worktree와
/// 요약 brief의 새 대화다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UnparkOption {
    pub machine: String,
    pub same: bool,
    pub connected: bool,
    /// `resume`(같은 대화)·`new`(새 대화와 요약 brief)
    pub conversation: String,
    /// 다른 머신의 저장소 경로(작업 폴더 thread는 없다)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repo: Option<String>,
    /// 고를 수 없으면 까닭
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UnparkPreview {
    pub project: String,
    pub thread: String,
    pub options: Vec<UnparkOption>,
    /// 지금 자리를 차지한 사용자 thread 수와 한도. 다 찼으면 이어 하기가 거부된다
    pub open: u32,
    pub limit: u32,
}

/// 사용자가 직접 허용한 한도 초과 시작. 시작 시각·요청자·coordinator가 인용한 지시를 보존한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadLimitOverride {
    pub actor: String,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub user_request: Option<String>,
}

/// 검증한 보고 사본 첫 줄의 알림 의도. 묶음·진행률과 별개다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReportAttention {
    Completed,
    NeedsUser,
}

/// 목록은 100건(최대 200건)씩 읽는다. after는 응답의 next_after를 그대로 돌려준다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadListQuery {
    /// 끝난 thread를 뺀다(준비·실패·보류는 포함).
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub open: bool,
    #[serde(default)]
    #[ts(optional)]
    pub status: Option<ThreadStatus>,
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub after: Option<String>,
    /// 웹의 질문 상세가 필요할 때만 켠다. 기본 목록도 CLI가 표시하는 필드는 보존한다.
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub details: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadView {
    /// 기획 대화 thread면 그 기획 branch(`<저장소>/<주제>`, FRONT_API 1.172). 작업 목록·할 일에 보이지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub design: Option<String>,
    /// 연결된 pane의 실제 책임 경로. 옛 agent·비 macOS·pane 없는 thread는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub responsibility: Option<crate::Responsibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account_selection: Option<crate::AccountSelection>,
    /// 명시적으로 실행 한도를 무시한 시작의 출처. 옛 기록은 생략한다(FRONT_API 1.98).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub over_limit: Option<ThreadLimitOverride>,
    /// 보류 요청·보류 기록(FRONT_API 1.105). 보류하지 않은 thread와 옛 host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub park: Option<Box<ThreadPark>>,
    /// 이어진 할 일의 자동 병합(FRONT_API 1.143). off·할 일 없는 thread·옛 host는 생략한다
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<bool>")]
    pub auto_merge: bool,
    /// 실행 중인 pane의 번개 표지(`fast`·`ultrafast`). 표준 또는 미확인일 때는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub speed: Option<String>,
    /// FRONT_API 1.95. 보고의 특정 판을 여는 프로젝트별 영속 번호(r-0001부터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,

    pub project: String,
    pub id: String,
    pub title: String,
    pub machine: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account: Option<String>,
    pub role: Role,
    /// (FrontResponse 태그 `kind`와 겹치지 않게 이름을 달리한다)
    pub thread_kind: ThreadKind,
    /// 명시한 실행 대기 표지. timing이면 그 머신의 측정이 끝날 때까지 자동 시작이 기다릴 수 있다.
    #[serde(default)]
    pub holds: Vec<String>,
    /// 이 thread를 띄운 자동화 규칙 이름(`do.start`, FRONT_API 1.64, plan t-0475 A1). 사람이 띄운 thread와 옛 host는 생략한다.
    /// 웹 「작업」 목록이 줄 왼쪽 호박색 막대와 `자동화 thread` 표지로 보인다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub automation: Option<String>,
    /// 그 규칙의 제목(FRONT_API 1.64). 규칙 파일이 없어졌거나 제목이 비면 이름이다. `automation`이 없으면 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub automation_title: Option<String>,
    /// 최초 기동 당시 사건·일정의 설명(FRONT_API 1.91). 옛 thread·옛 host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub automation_trigger: Option<String>,
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
    pub report_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_attention: Option<ReportAttention>,
    /// 회수한 보고의 작업 단계. 옛 host·표지 없는 보고·끝난 thread는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stage: Option<ThreadStage>,
    pub acked_hash: Option<String>,
    /// report.md `## Next` 줄
    pub next: Vec<String>,
    /// 완료 보고에서 남긴 후속과 명시적 처리 기록(FRONT_API 1.131).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<ThreadFollowup>>")]
    pub followups: Vec<ThreadFollowup>,
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
    /// agy는 소진이 확인돼도 리셋 시각이 없을 수 있다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limited: Option<bool>,
    /// 보고 `## Questions`의 질문과 답(FRONT_API 1.72, plan §8.329): 웹 보고서 패널 「답할 것」 카드와 대화창 칩이 쓴다.
    /// 질문이 없는 보고와 옛 host는 생략한다
    // 사건(`FrontEvent::Thread`) 크기를 늘리지 않게 Box에 둔다(clippy large_enum_variant)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub questions: Option<Box<ThreadQuestions>>,
}

/// thread 작업 요약 표지(plan §8.259). 줄·머리 단추가 요약을 읽지 않고 「낡았다」를 안다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SummaryMark {
    /// 요약을 썼거나 정상 진행 보고를 받은 마지막 때. 둘 다 없으면 None
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
    /// 요약·정상 진행 보고에 아직 반영 안 된 task.md Follow-ups 지시 수
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

/// 큰 작업 하나의 진행 상태. 완료 개수는 소요 시간 비율이 아니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SummaryStepState {
    Pending,
    Working,
    Done,
}

/// 요약의 큰 작업. 작성한 순서를 유지하며 완료한 항목도 남긴다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SummaryStep {
    pub text: String,
    pub state: SummaryStepState,
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
    /// 전체 큰 작업(최대 6개, FRONT_API 1.86). 옛 요약·옛 host는 목록이 없다.
    #[serde(default)]
    #[ts(as = "Option<Vec<SummaryStep>>", optional)]
    pub progress: Vec<SummaryStep>,
    pub now: Vec<String>,
    pub decided: Vec<SummaryEntry>,
    pub changed: Vec<SummaryEntry>,
    pub done: Vec<String>,
    /// thread 보고 `## Decisions` 줄(agent가 정한 것)
    pub decisions: Vec<String>,
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
    /// 마지막 갱신 쪽(coordinator·사람·agent). 진행은 정상 thread 보고를 우선한다.
    pub by: Option<String>,
    /// Follow-ups 전체 수
    pub follow_ups: u32,
    /// 요약 뒤 지시 수와 그 마지막 몇 개(요약이 없으면 Follow-ups 마지막 몇 개)
    pub pending: u32,
    pub pending_lines: Vec<FollowUpLine>,
    /// 요약이 없을 때 보이는 할 일 첫 줄
    pub task: Option<String>,
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

/// 완료 보고의 후속 한 줄. 원래 판과 처리 기록은 다음 보고로 지우지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadFollowup {
    pub id: String,
    pub revision: String,
    pub line: String,
    /// 같은 프로젝트의 후속 할 일 표시 ID. 버림일 때는 없다.
    pub task: Option<String>,
    /// 버림·정리 완료의 명시적 사유.
    pub reason: Option<String>,
    pub actor: Option<String>,
    #[ts(type = "number | null")]
    pub handled_ms: Option<u64>,
}

/// report.md `## Next` 절의 줄: 한 줄에 권하는 행동 하나, 목록 표시는 빼고 빈 줄은 버린다
/// (herdr-projects `thread::next_lines`를 옮겼다). 첫 줄 상태 태그는 읽지 않는다(plan §8.78).
pub fn next_lines(report: &str) -> Vec<String> {
    section_lines(report, "Next")
}

/// report.md `## <name>` 절의 목록 줄. 코드 울타리 안의 예시·절 제목은 제외한다.
/// `## Decisions`도 이것으로 읽는다(plan §8.259).
pub fn section_lines(report: &str, name: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut inside = false;
    let mut fence: Option<(u8, usize)> = None;
    for line in report.lines() {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed.as_bytes().first().copied();
        let width = trimmed.bytes().take_while(|b| Some(*b) == marker).count();
        if let Some((opened, minimum)) = fence {
            if indent < 4 && marker == Some(opened) && width >= minimum && trimmed[width..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if indent < 4
            && matches!(marker, Some(b'`' | b'~'))
            && width >= 3
            && (marker == Some(b'~') || !trimmed[width..].contains('`'))
        {
            fence = Some((marker.unwrap(), width));
            continue;
        }
        if let Some(head) = line.strip_prefix("## ") {
            inside = head.trim() == name;
            continue;
        }
        if !inside || line.starts_with('#') {
            if line.starts_with("# ") {
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
