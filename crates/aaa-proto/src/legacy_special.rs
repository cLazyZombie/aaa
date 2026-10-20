//! FRONT_API 한 판 동안만 남기는 제거된 임무·평가자 응답 DTO. 실행·파일 schema는 없다.
use crate::ThreadView;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 확인창만 화면의 Working/Idle로 자동 회복한다. 나머지는 명시 restart를 기다린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MissionBlockReason {
    Question,
    MessageUnknown,
    WakeUnknown,
    BriefUnknown,
    RestartFailed,
    LegacyUnknown,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MissionView {
    pub project: String,
    pub name: String,
    pub title: String,
    pub thread: Option<Box<crate::ThreadView>>,
    pub state: String,
    #[serde(default)]
    pub blocked_reason: Option<MissionBlockReason>,
    pub applied_sha: Option<String>,
    /// reload로 예약한 판. restart만 요청했으면 현재 적용 판이다.
    #[serde(default)]
    pub pending_sha: Option<String>,
    /// 적용 전이면 실제 대기 이유. 요청 수락과 적용 완료를 구분한다.
    #[serde(default)]
    pub apply_wait: Option<String>,
    pub file_sha: Option<String>,
    pub approval_sha256: String,
    pub approved: bool,
    pub stale: bool,
    pub file_text: String,
    pub applied_text: String,
    /// TOML의 brief_file에서 읽은 글. 옛 md에서는 비워 중복 표시하지 않는다.
    #[serde(default)]
    pub brief_text: String,
    #[serde(default)]
    pub applied_brief_text: String,
    pub diff: String,
    pub error: Option<String>,
    pub wakes_today: u32,
    /// 0이면 하루 깨움 상한이 없다.
    pub max_wakes: u32,
    /// 누적 작업 분. 깨움당 시간 제한은 wake_work_minutes에 적용한다.
    pub work_minutes: u64,
    #[serde(default)]
    pub max_minutes: u32,
    #[serde(default)]
    pub wake_work_minutes: u64,
    pub input_bytes: u64,
    pub posts: u32,
    pub handled_posts: u32,
    pub pending: u32,
    #[ts(type = "number | null")]
    pub next_reset_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub usage_wait_until_ms: Option<u64>,
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
