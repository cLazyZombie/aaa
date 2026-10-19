//! 특수 임무 보기(FRONT_API 1.38). 파일은 후보이며 applied_text가 실제 지시다.
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
