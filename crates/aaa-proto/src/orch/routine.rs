//! inbox 항목과 routine·자동화 보기 타입.

use super::*;

/// inbox 항목(0.2.35 inbox 계승, §8.9). DB가 원본이고 `projects/<p>/inbox/<id>.md`는 투영이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InboxItem {
    /// 저장 ID. 질문·inbox·파일·API 연결에서 같은 값을 쓴다.
    pub id: String,
    /// 표시 전용 프로젝트별 `i-NNNN`. 구형 host 응답에는 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub short_id: Option<String>,
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

impl InboxItem {
    /// CLI·알림의 표시만 줄인다. 요청·연결 키는 id를 그대로 쓴다.
    pub fn display_id(&self) -> &str {
        self.short_id.as_deref().unwrap_or(&self.id)
    }
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
    /// 프로젝트별 영속 번호(a-0001부터). 같은 규칙 이름의 수정·복원·형식 변환에도 유지한다. 옛 host는 생략한다.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub short_id: Option<String>,
    /// 웹에서 읽기만 하는 시스템 자동화의 이유. 없으면 일반 자동화(옛 host도 생략).
    #[serde(default)]
    pub web_read_only_reason: Option<String>,
    pub title: String,
    pub name: String,
    pub trigger: String,
    pub action: String,
    /// automation·routine·legacy. legacy는 변환 안내용 옛 파일이다.
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
    #[serde(default)]
    pub start_approval_sha256: Option<String>,
    #[serde(default)]
    pub start_approved: Option<bool>,
    #[serde(default)]
    pub start_scope: Option<String>,
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
    /// 내구 입력 큐(FRONT_API 1.58). 옛 host는 생략한다.
    #[serde(default)]
    pub queue: Option<AutomationQueue>,
    /// 자동화 기억의 host 사본. 옛 host는 생략한다.
    pub memory: Option<AutomationMemory>,
    /// 입력 처리·완료 보고·Idle가 갖춰진 thread의 끝남 예정 시각.
    pub closing: Vec<AutomationClosing>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationMemory {
    pub text: String,
    #[ts(type = "number")]
    pub bytes: u64,
    pub truncated: bool,
    pub thread: Option<String>,
    #[ts(type = "number | null")]
    pub updated_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationClosing {
    pub thread: String,
    #[ts(type = "number")]
    pub close_at_ms: u64,
}

/// 규칙의 내구 입력 큐. 보낸 뒤 결과를 모르는 `do.message` 묶음은 사람이 확인하고 푼다(`automation_unblock`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationQueue {
    /// 아직 보내지 않은 입력 수
    pub pending: u32,
    pub inflight: Option<AutomationInflight>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationInflight {
    /// 사람이 본 묶음. 풀 때 이 값을 실어 보낸다.
    pub id: String,
    pub events: u32,
    /// 묶음 첫 입력이 들어온 때
    #[ts(type = "number")]
    pub at_ms: u64,
    /// `do.message` 대상 thread(주인 thread면 그 사건의 thread)
    pub thread: Option<String>,
    /// 보냈는지 모른다: 자동으로 다시 보내지 않고 확인 필요로 기다린다
    pub unknown: bool,
}

/// 상세 화면의 파일 원문(`routine_runs` 답, FRONT_API 1.58). 각 글은 64KiB에서 자른다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RoutineSource {
    pub file: String,
    pub text: String,
    pub sha256: String,
    /// `text_file`·`brief_file`이 가리키는 본문
    pub body_file: Option<String>,
    pub body: String,
    /// 원문(`text`)을 잘랐다
    pub truncated: bool,
    /// 본문(`body`)을 잘랐다
    pub body_truncated: bool,
}

/// 편집할 자동화 파일 하나(관리 GUI W2, FRONT_API 1.76). 원문과 판(sha256)을 함께 준다. 저장은 그 판일 때만 쓴다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationDoc {
    /// 웹에서 읽기만 하는 시스템 자동화의 이유. 없으면 일반 자동화(옛 host도 생략).
    #[serde(default)]
    pub web_read_only_reason: Option<String>,
    pub name: String,
    /// `automations/<이름>.toml`, 옛 형식이면 `routines/<이름>.md`
    pub file: String,
    pub text: String,
    pub sha256: String,
    /// 옛 `.md` 규칙: 웹은 읽기만 하고 저장·삭제를 거부한다
    pub legacy: bool,
    /// `text_file`·`brief_file`이 가리키는 본문 파일과 그 판(없는 파일이면 `body_sha256`이 없다)
    pub body_file: Option<String>,
    pub body: Option<String>,
    pub body_sha256: Option<String>,
    pub check: AutomationCheck,
}

/// 초안 검사 결과. `form`은 원문을 정본 Rust 타입으로 읽은 값(TOML과 같은 모양)이며 TOML 문법·타입 오류면 없다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationCheck {
    #[ts(type = "Record<string, unknown> | null")]
    pub form: Option<serde_json::Value>,
    /// `파일:줄:열 칸: 이유`. 하나라도 있으면 저장하지 않는다
    pub errors: Vec<String>,
    /// 저장은 되지만 실행 때 막히는 것(시작 후보 머신·저장소·profile을 지금 풀지 못함 등)
    pub warnings: Vec<String>,
    pub review: Option<AutomationReview>,
}

/// 검토 화면: 이 초안을 저장하면 무엇이 일어나는가
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationReview {
    pub enabled: bool,
    /// 조건·동작 낱말(목록 줄과 같다)
    pub trigger: String,
    pub action: String,
    /// safety `start_threads`(propose·auto): 자동 시작이 바로 띄우는지, 제안 한 줄만 남기는지
    pub start_threads: String,
    pub start_scope: Option<String>,
    pub start_approval_sha256: Option<String>,
    /// 지금 승인한 범위 안인가(시작 동작만)
    pub start_approved: Option<bool>,
    /// 명령 조건의 명령·머신과 그 hash(승인 대상)
    pub command: String,
    pub machine: Option<String>,
    pub command_sha256: String,
    pub command_approved: bool,
    /// safety `routine_commands`
    pub commands_allowed: bool,
}

/// 지운 자동화(`automations/.trash/`). 되살리면 꺼진 채로 돌아온다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationTrashEntry {
    /// 웹에서 읽기만 하는 시스템 자동화의 이유. 없으면 일반 자동화(옛 host도 생략).
    #[serde(default)]
    pub web_read_only_reason: Option<String>,
    /// `.trash/` 안 파일 이름(되살리기 요청에 쓴다)
    pub id: String,
    pub name: String,
    pub title: String,
    #[ts(type = "number")]
    pub deleted_at_ms: u64,
}
