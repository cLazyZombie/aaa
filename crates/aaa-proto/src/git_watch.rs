//! git.v1: repo 머신 agent의 읽기 전용 head 조회·객체 메타데이터.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
pub enum GitQuery {
    Heads {
        branches: Vec<String>,
    },
    /// git_merged.v1: 실제 origin/main의 조상인 thread branch 또는 보고의 병합 커밋.
    MergedIntoMain {
        branch: Option<String>,
        commits: Vec<String>,
    },
    Change {
        branch: String,
        before: Option<String>,
        after: Option<String>,
    },
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitAnswer {
    pub origin_hash: String,
    pub heads: BTreeMap<String, Option<String>>,
    pub change: Option<GitChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_commit: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitChange {
    pub branch: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub kind: String,
    pub commits: Vec<GitCommit>,
    pub changed_files: u64,
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitCommit {
    pub sha: String,
    pub subject: String,
    pub owners: Vec<String>,
}
