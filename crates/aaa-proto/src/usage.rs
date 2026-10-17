//! 구독 사용량(plan 한도). herdr-web-ui `shared/protocol.ts`(aa624cb) 281~327줄의 `Usage` 형식을 그대로 옮겼다.
//!
//! `GET /api/v1/usage[?refresh=1]` → `UsageReport`: host 머신의 CLI가 로그인한 구독의 한도(5시간 세션·주·월)를
//! 그 CLI 자신의 로그인으로 읽는다(host `usage.rs`). 이 머신에 로그인한 구독만, 계정마다 한 번 나온다.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// GET /api/v1/usage가 CLI 로그인으로 한도를 읽는 구독. 참고(claude·codex·cursor·copilot·grok·antigravity·opencode) 가운데
/// Claude·Codex만 둔다(사용자 요청 10-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UsageProviderId {
    Claude,
    Codex,
}

/// 한도가 세는 기간: 굴러가는 세션(5시간)·하루·주·결제 달
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UsageWindowKind {
    Session,
    Day,
    Week,
    Month,
}

/// plan의 한도 하나: 얼마나 썼고 언제 다시 시작하는지
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct UsageWindow {
    pub kind: UsageWindowKind,
    /// plan이 따로 거는 모델·몫("Sonnet", "Premium"). plan 전체 한도면 None
    pub scope: Option<String>,
    /// 0~100
    pub used_percent: f64,
    /// ISO 8601. 구독이 말하지 않으면 None
    pub resets_at: Option<String>,
    /// 이 창이 세기 시작한 때(ISO 8601). 구독이 그것도 창 길이도 말하지 않으면 None
    pub starts_at: Option<String>,
}

/// 숫자가 없거나 오래된 까닭: 로그인이 끝났거나 거절됨, 구독이 늦추라고 함, 요청 실패,
/// 로그인을 담은 macOS keychain을 이 host 세션에서 읽지 못함
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum UsageProblem {
    Expired,
    RateLimited,
    Failed,
    Locked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ProviderUsage {
    pub id: UsageProviderId,
    /// 한 구독의 한 계정. 읽을 때마다 같다: 구독과 계정 id, 누구 것인지 말하지 않는 로그인이면 구독과 찾은 곳(hash).
    /// 한 머신이 한 구독의 두 계정에 로그인했으면 둘 다 나온다.
    pub key: String,
    /// 주인이 아는 계정 이름(email, GitHub login). 모르면 None
    pub account: Option<String>,
    /// 구독이 말하는 plan 이름("max", "pro")
    pub plan: Option<String>,
    /// 마지막으로 읽은 숫자. 뒤에 실패해도 남고, 그때는 `problem`이 까닭을 말한다
    pub windows: Vec<UsageWindow>,
    pub problem: Option<UsageProblem>,
    /// `windows`를 읽은 때(ISO 8601). 한 번도 못 읽었으면 None
    pub checked_at: Option<String>,
}

/// GET /api/v1/usage: 이 머신의 CLI가 로그인한 구독만, 계정마다 한 번
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, TS)]
pub struct UsageReport {
    pub providers: Vec<ProviderUsage>,
}
