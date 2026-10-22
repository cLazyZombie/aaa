//! ci.v1: agent가 GitHub 응답을 줄여 host에 보낸다. 본문·인증 머리는 전송하지 않는다.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
pub enum CiQuery {
    Runs { etag: Option<String>, since: Option<String>, page: u32 },
    Jobs { run: u64, attempt: u32, page: u32 },
    Log { run: u64, job: u64 },
    Compare { base: String, head: String, page: u32 },
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CiRun {
    pub id: u64,
    pub workflow: u64,
    pub name: String,
    pub number: u64,
    pub attempt: u32,
    pub branch: String,
    pub sha: String,
    pub status: String,
    pub conclusion: String,
    pub created_ms: u64,
    pub owners: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CiJob {
    pub id: u64,
    pub name: String,
    pub steps: Vec<String>,
    /// 실패 로그 전체에서 뽑은 실패 시험 이름(없으면 panic 위치·끝나지 않은 시험). 옛 agent는 비운다.
    #[serde(default)]
    pub failures: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CiCommit {
    pub sha: String,
    pub owners: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CiAnswer {
    pub origin: String,
    pub not_modified: bool,
    pub etag: Option<String>,
    pub next: bool,
    pub runs: Vec<CiRun>,
    pub jobs: Vec<CiJob>,
    pub commits: Vec<CiCommit>,
    pub log: String,
    pub truncated: bool,
    /// Log 응답만: 꼬리로 자르기 전 전체 로그에서 뽑은 실패 표지
    #[serde(default)]
    pub failures: Vec<String>,
}
