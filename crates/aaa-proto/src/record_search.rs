//! host 보관 작업 기록의 본문 검색과 불변 원문 판(FRONT_API 1.140).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Report,
    Question,
    Task,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecordSearchQuery {
    pub query: String,
    pub project: Option<String>,
    pub kind: Option<RecordKind>,
    pub after: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecordHit {
    /// 프로젝트 안에서만 유일한 내용 해시. 원문 열기에는 project도 반드시 보낸다.
    pub key: String,
    pub project: String,
    pub kind: RecordKind,
    pub id: String,
    pub title: String,
    pub source: String,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub excerpt: String,
    pub report_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecordDocument {
    pub hit: RecordHit,
    /// 검색된 판의 본문. 최신 본문으로 대체하지 않는다.
    pub body: String,
    /// 다음 UTF-8 바이트 위치. null이면 본문 끝이다.
    #[ts(type = "number | null")]
    pub next: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RecordSearchPage {
    pub hits: Vec<RecordHit>,
    pub next: Option<String>,
}
