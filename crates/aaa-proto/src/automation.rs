//! 자동화 v2 파일·실행 조회 계약. 실행·파일 접근·권한 판정은 host가 맡는다.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct AutomationDefinition {
    pub schema: u32,
    #[serde(default)]
    pub title: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub trigger: AutomationTrigger,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observe: Option<AutomationObserve>,
    #[serde(default)]
    pub condition: AutomationCondition,
    pub action: AutomationAction,
    #[serde(default)]
    pub policy: AutomationPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationTrigger {
    Schedule {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        every: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        daily: Option<String>,
        #[serde(default)]
        catch_up: AutomationCatchUp,
    },
    Event {
        source: AutomationEventSource,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        repo: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        machine: Option<String>,
    },
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCatchUp {
    #[default]
    Once,
    Skip,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AutomationEventSource {
    Thread,
    Ci,
    Git,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationObserve {
    Command(AutomationCommand),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct AutomationCommand {
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(default, deny_unknown_fields)]
pub struct AutomationCondition {
    /// 일정에 필요한 열린 일반 thread 최소 개수. 자동화·완료·보류 thread는 제외한다.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1))]
    pub min_open_threads: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repos: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub branches: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub workflows: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub include_automation: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub output_changed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_commits: Option<u32>,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(default, deny_unknown_fields)]
pub struct AutomationPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "duration_option")]
    pub min_gap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_per_day: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_running: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_session_below: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_week_below: Option<f64>,
}
fn duration_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(v) => Ok(Some(v)),
        serde_json::Value::Number(v) if v.as_u64() == Some(0) => Ok(Some("0m".into())),
        _ => Err(serde::de::Error::custom("기간 문자열 또는 0을 쓴다")),
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationAction {
    Notice(AutomationNotice),
    Message(AutomationMessage),
    Thread(AutomationThread),
    Command(AutomationCommand),
    Proposal,
}
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(default, deny_unknown_fields)]
pub struct AutomationNotice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_file: Option<String>,
    pub notify: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct AutomationMessage {
    pub thread: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_file: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct AutomationThread {
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub profiles: Vec<String>,
    /// 비우거나 생략하면 자동 머신(workspace 허용·독점 예약·착지 중·비중으로 host가 고른다)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub machines: Vec<String>,
    #[serde(default)]
    pub reuse: AutomationReuse,
    #[serde(default)]
    pub memory: bool,
    #[serde(default)]
    pub reports: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_idle: Option<String>,
    #[serde(default)]
    pub profile_pick: AutomationProfilePick,
    #[serde(default)]
    pub machine_pick: AutomationMachinePick,
    #[serde(default)]
    pub wait: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "duration_option")]
    pub wait_max: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_file: Option<String>,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AutomationReuse {
    #[default]
    New,
    Open,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AutomationProfilePick {
    #[default]
    First,
    WeekLeft,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AutomationMachinePick {
    #[default]
    Idlest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunState {
    Queued,
    Waiting,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AutomationStage {
    Trigger,
    Observe,
    Condition,
    Policy,
    Action,
    FollowUp,
    Legacy,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AutomationReceipt {
    NoticeCreated,
    MessageDelivered,
    ThreadStarted,
    ProposalCreated,
    CommandExited,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AutomationRunEvent {
    #[ts(type = "number")]
    pub at_ms: u64,
    pub stage: AutomationStage,
    pub state: AutomationRunState,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AutomationExecution {
    pub id: String,
    pub name: String,
    pub input_id: String,
    pub revision: String,
    pub source: String,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number")]
    pub updated_at_ms: u64,
    pub state: AutomationRunState,
    pub stage: AutomationStage,
    pub reason: String,
    pub receipt: Option<AutomationReceipt>,
    pub thread: Option<String>,
    pub task: Option<String>,
    pub report: Option<String>,
    pub input_file: Option<String>,
    pub processed: bool,
    pub outcome: Option<String>,
    pub legacy: bool,
    pub events: Vec<AutomationRunEvent>,
}

/// peer+token으로 인증된 자동화 thread의 자기 입력 처리 신고. 실행 권한을 부여하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AutomationCompletion {
    pub run: String,
    pub outcome: AutomationOutcome,
    #[serde(default)]
    pub report: Option<String>,
}
/// 같은 결과를 여러 입력에 원자적으로 기록한다. 단일 입력의 옛 JSON과 구별한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AutomationCompletionBatch {
    pub runs: Vec<String>,
    pub outcome: AutomationOutcome,
    #[serde(default)]
    pub report: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AutomationOutcome {
    Completed,
    Failed,
    NeedsDecision,
}
