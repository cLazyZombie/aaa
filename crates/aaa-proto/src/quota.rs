//! 등록 계정의 구독 잔량. 원문·메일·인증 경로는 경계를 넘기지 않는다(A2).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuotaSource {
    CodexRpc,
    ClaudeStatusline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuotaProblem {
    Unknown,
    Expired,
    RateLimited,
    Locked,
    Disconnected,
    Unsupported,
    Failed,
    ClockSkew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuotaFreshness {
    Fresh,
    Stale,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct QuotaWindow {
    pub limit_id: String,
    /// 기간을 모르는 서로 다른 창도 합치지 않기 위한 제공자 창 이름.
    pub window_id: String,
    pub scope: Option<String>,
    pub duration_mins: Option<u32>,
    pub used_percent: f64,
    #[ts(type = "number")]
    pub observed_at_ms: u64,
    #[ts(type = "number | null")]
    pub resets_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct QuotaSnapshot {
    /// 로그인·등록 때 새로 만드는 세대. 토큰의 해시가 아니다.
    pub generation: String,
    pub source: QuotaSource,
    #[ts(type = "number")]
    pub sampled_at_ms: u64,
    pub windows: Vec<QuotaWindow>,
    pub problem: Option<QuotaProblem>,
    #[ts(type = "number | null")]
    pub retry_at_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AccountQuotaWindow {
    #[serde(flatten)]
    pub window: QuotaWindow,
    pub machine: String,
    pub source: QuotaSource,
    #[ts(type = "number")]
    pub received_at_ms: u64,
    pub freshness: QuotaFreshness,
    pub problem: Option<QuotaProblem>,
    pub conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct AccountQuota {
    pub account: String,
    pub harness: String,
    pub windows: Vec<AccountQuotaWindow>,
    pub problem: Option<QuotaProblem>,
    #[ts(type = "number | null")]
    pub retry_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct AccountQuotaReport {
    pub accounts: Vec<AccountQuota>,
}
