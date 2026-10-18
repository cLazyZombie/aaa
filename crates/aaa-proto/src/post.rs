//! post는 자료다. 지시 종류나 보낸 이 자기 신고 칸은 없다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum PostType {
    Notice,
    Data,
    Request,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PostInput {
    pub post_id: String,
    pub to: String,
    pub post_type: PostType,
    pub topic: String,
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub needs_user: bool,
    #[serde(default)]
    pub refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum PostResult {
    Accepted { id: String },
    Refused { reason: String, existing: Option<String> },
    Queued { post_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PostSender {
    pub thread: Option<String>,
    pub role: crate::Role,
    pub mission: Option<String>,
    pub machine: String,
    pub session: Option<String>,
    pub verified: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PostEnvelope {
    pub id: String,
    pub project: String,
    pub from: PostSender,
    pub input: PostInput,
    pub caused_by: Vec<String>,
    pub body_sha256: String,
    #[ts(type = "number")]
    pub created_ms: u64,
    pub state: String,
}
