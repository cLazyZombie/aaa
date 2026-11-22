//! harness 사건과 agent 상태(진행·모델·context).

use super::*;

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
    /// session_start·turn_start·turn_end·question·idle·error·exited·progress·released(셸 앞 agent를 놓음)·
    /// compacting(대화 정리 중인 작업 중, plan §8.309: 옛 host는 모르는 kind로 버린다)
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
    /// `kind = "plan"`일 때만: 대화 기록에서 읽은 컨텍스트 사용량(plan §8.321). 아직 응답이 없으면 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextUsage>,
    /// `kind = "plan"`: 지금 프로세스의 대화 기록에서 마지막으로 확인한 모델. 없으면 아직 모른다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_observation: Option<ModelObservation>,
    /// `codex_speed`: 대화 기록과 별개인 Codex 제목의 처리 속도 관측.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_speed: Option<CodexSpeed>,
    /// `turn_reply`: 기록에서 확인한 완료 답변. 상태·승인 여부를 바꾸지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_reply: Option<TurnReply>,
    /// `provider_turn`: 현재 실행의 native turn 관측. 화면 상태와 별개다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_turn: Option<ProviderTurn>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexSpeed {
    Standard,
    Fast,
    Ultrafast,
}

impl CodexSpeed {
    pub fn badge(self) -> Option<&'static str> {
        match self {
            Self::Standard => None,
            Self::Fast => Some("fast"),
            Self::Ultrafast => Some("ultrafast"),
        }
    }
}

/// native 기록의 turn 경계. 시작·성공도 보내 이전 오류를 걷고 늦은 오류를 거른다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProviderTurn {
    /// agent가 확인한 harness 실행 시작 시각(실행 세대).
    #[ts(type = "number")]
    pub generation_ms: u64,
    pub harness_session: String,
    #[ts(type = "number")]
    pub started_ms: u64,
    /// 이 관측의 native 시각. 시작이면 started_ms, 완료면 finished_ms와 같다.
    #[ts(type = "number")]
    pub at_ms: u64,
    #[ts(type = "number | null")]
    pub finished_ms: Option<u64>,
    pub error: Option<ProviderFailure>,
}

/// 원문 로그·요청 내용·경로는 경계를 넘기지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProviderFailure {
    pub kind: ProviderFailureKind,
    /// 알려진 native 코드만 싣는다. 모르는 코드는 None이다.
    pub code: Option<String>,
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFailureKind {
    Overloaded,
    Authentication,
    Allowance,
    Model,
    RateLimit,
    Unknown,
}

/// 같은 턴의 질문 카드 접수와 대조할 로컬 harness 기록의 시각과 답변.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnReply {
    pub started_ms: u64,
    pub finished_ms: u64,
    pub text: String,
}

/// 대화 기록의 모델 선택. 시작 profile의 effort를 관측한 모델에 섞지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ModelObservation {
    pub model: String,
    pub effort: Option<String>,
    #[ts(type = "number")]
    pub at_ms: u64,
    /// 처리 속도 등급(FRONT_API 1.159): `fast`·`ultrafast`. 표준이거나 아직 모르면 없다. Claude는 상태줄 `fast_mode`,
    /// Codex는 OSC 제목의 `fast-mode` 항목에서 읽는다. 옛 agent는 보내지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub speed: Option<String>,
}

/// harness 세션이 지금 쓴 컨텍스트(plan §8.321). agent가 대화 기록의 마지막 응답에서 읽는다(Claude `usage`, Codex `token_count`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContextUsage {
    /// 마지막 응답에 실린 컨텍스트 토큰(정리가 막 끝났으면 정리 뒤 크기)
    #[ts(type = "number")]
    pub used: u64,
    /// 창 크기. 기록으로 알 수 없으면 null(웹은 %가 아니라 토큰 수로 보인다)
    #[ts(type = "number | null")]
    pub window: Option<u64>,
    /// 사용량 %(그 harness 터미널과 같은 식: Claude 사용/창 반올림, Codex 기준 12k를 뺀 「left」의 나머지). 창을 모르거나
    /// 사용이 창보다 크면 null
    pub percent: Option<u8>,
    /// 이 기록의 마지막 정리(compaction) 전후 크기. 정리가 없었거나 뒤 크기를 아직 모르면 null
    pub compaction: Option<ContextCompaction>,
}

/// 정리 전후 컨텍스트 크기(Claude `compactMetadata.preTokens`·`postTokens`, Codex 정리 앞뒤 `token_count`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContextCompaction {
    #[ts(type = "number")]
    pub before: u64,
    #[ts(type = "number")]
    pub after: u64,
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
    /// 지금 활동 글이 처음 보고된 때(FRONT_API 1.104). 같은 활동의 진행률·정기 보고는 시작을 바꾸지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub progress_since_ms: Option<u64>,
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
    /// 작업 중이지만 harness가 대화를 정리(compaction)하는 중이다(화면 감지 사건 `compacting`, plan §8.309). 그동안 harness는 친 글과
    /// 대기 글을 읽지 않고, 웹은 보낸 글을 이 기기에 붙잡았다가 끝나면 보낸다. 옛 host는 보내지 않는다(없으면 false, 1.61)
    #[serde(default)]
    pub compacting: bool,
    /// 컨텍스트 사용량(plan §8.321, 1.67). 옛 host·첫 응답 전에는 null
    #[serde(default)]
    pub context: Option<ContextUsage>,
    /// 마지막으로 확인한 실행 모델(FRONT_API 1.109). 시작 profile과 별개이며 옛 agent는 보내지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model_observation: Option<ModelObservation>,
    /// native turn의 마지막 관측. 옛 agent는 보내지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub provider_turn: Option<ProviderTurn>,
}

pub(super) fn seen_default() -> bool {
    true
}
