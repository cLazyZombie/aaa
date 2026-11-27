//! review.v1. 요청·원장에는 비밀이 없다. 인증은 host의 실행 요청에만 싣는다.
use crate::SecretText;
use serde::{Deserialize, Serialize};

pub const REVIEW_FEATURE: &str = "review.v1";
pub const REVIEW_RUN_SECS: u64 = 600;
pub const REVIEW_MAX_RESULT: usize = 32 * 1024;

pub fn review_id_ok(id: &str) -> bool {
    id.len() == 26 && id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewTarget {
    pub base: String,
    pub commit: String,
    pub tree: String,
}

impl ReviewTarget {
    pub fn valid(&self) -> bool {
        matches!(self.commit.len(), 40 | 64)
            && [&self.base, &self.commit, &self.tree].into_iter().all(|sha| {
                sha.len() == self.commit.len() && sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewStart {
    pub id: String,
    pub profile: String,
    pub cwd: String,
    pub target: ReviewTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ReviewRequest {
    Start(ReviewStart),
    Status { id: String, wait: bool },
    Fetch { id: String },
    Cancel { id: String },
}

impl ReviewRequest {
    pub fn id(&self) -> &str {
        match self {
            Self::Start(start) => &start.id,
            Self::Status { id, .. } | Self::Fetch { id } | Self::Cancel { id } => id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Waiting,
    Queued,
    Running,
    Done,
    Failed,
    Cancelled,
    Unknown,
}

impl ReviewState {
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewView {
    pub id: String,
    pub profile: String,
    pub target: ReviewTarget,
    pub state: ReviewState,
    pub account: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewSeverity {
    P0,
    P1,
    P2,
    P3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFinding {
    pub path: String,
    pub line: u32,
    pub severity: ReviewSeverity,
    pub title: String,
    pub evidence: String,
    pub failure_path: String,
    pub confidence: f32,
    pub verification: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewResult {
    pub model: String,
    pub findings: Vec<ReviewFinding>,
}

impl ReviewResult {
    pub fn valid(&self) -> bool {
        !self.model.is_empty()
            && self.model.len() <= 128
            && !self.model.chars().any(char::is_control)
            && self.findings.len() <= 50
            && self.findings.iter().all(|f| {
                f.line > 0
                    && !f.path.chars().any(char::is_control)
                    && crate::design_clean_path(&f.path).as_deref() == Some(&f.path)
                    && (0.0..=1.0).contains(&f.confidence)
                    && [&f.title, &f.evidence, &f.failure_path, &f.verification]
                        .into_iter()
                        .all(|s| !s.trim().is_empty())
            })
            && serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= REVIEW_MAX_RESULT)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewResponse {
    pub job: ReviewView,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub result: Option<ReviewResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "harness", rename_all = "snake_case")]
pub enum ReviewAuth {
    Codex { access_token: SecretText, account_id: SecretText, plan: Option<String> },
    Claude { token: SecretText },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewRun {
    pub id: String,
    pub repo: String,
    pub thread_cwd: String,
    pub requested_cwd: String,
    pub target: ReviewTarget,
    pub model: String,
    pub effort: String,
    pub auth: ReviewAuth,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ReviewExecution {
    Done(ReviewResult),
    Failed { reason: String, used: bool },
    Cancelled { used: bool },
    Unknown { reason: String },
}
