//! 저장소 workspace 지표. 수집은 보고 회수에서, 조회는 host 원장에서만 한다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MetricCounts {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub filtered: u32,
    pub unit: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MetricTiming {
    pub name: String,
    #[ts(type = "number")]
    pub duration_ms: u64,
    #[ts(type = "number | null")]
    pub start_ms: Option<u64>,
    pub outcome: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MetricRanking {
    pub items: Vec<MetricTiming>,
    pub unit: String,
    pub complete: bool,
    pub missing_reason: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MetricRun {
    #[serde(default)]
    pub commit: String,
    pub run_id: String,
    #[ts(type = "number")]
    pub observed_at_ms: u64,
    pub machine: String,
    pub profile: Option<String>,
    pub source: String,
    pub outcome: String,
    #[ts(type = "number | null")]
    pub verification_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub build_ms: Option<u64>,
    pub tests: Option<MetricCounts>,
    pub e2e_files: Option<u32>,
    pub e2e_unit: Option<String>,
    pub e2e_jobs: Option<u32>,
    pub slow_tests: MetricRanking,
    pub slow_e2e: MetricRanking,
    pub stages: Vec<MetricTiming>,
    #[ts(type = "number | null")]
    pub artifact_bytes: Option<u64>,
    pub artifact_name: Option<String>,
    pub notes: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CodeLines {
    pub commit: String,
    pub tree: String,
    #[ts(type = "number")]
    pub committed_at_ms: u64,
    #[ts(type = "number")]
    pub observed_at_ms: u64,
    pub branch: String,
    pub rules_version: u32,
    pub counting_unit: String,
    pub total_lines: u32,
    pub by_language: std::collections::BTreeMap<String, u32>,
    pub breakdown: Option<std::collections::BTreeMap<String, u32>>,
    pub split_status: String,
    pub scanned_files: u32,
    pub changed_files: u32,
    pub source: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceCollected {
    #[serde(default)]
    pub remote_id: Option<String>,
    pub runs: Vec<MetricRun>,
    pub code_lines: Option<CodeLines>,
    pub notes: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceMachine {
    pub name: String,
    pub connected: bool,
    pub supported: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RepoWorkspace {
    pub id: String,
    pub project: String,
    pub name: String,
    pub machines: Vec<WorkspaceMachine>,
    #[ts(type = "number | null")]
    pub last_collected_ms: Option<u64>,
    pub collection_notes: Vec<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct WorkspaceQuery {
    pub workspace_id: Option<String>,
    pub machine: Option<String>,
    pub profile: Option<String>,
    pub days: Option<u32>,
    /// run 또는 day. 날짜는 UTC offset 기준이며 일광절약 변경은 별도 조건이다.
    pub granularity: Option<String>,
    pub utc_offset_minutes: Option<i32>,
    pub code_axis: Option<String>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MetricPoint {
    #[ts(type = "number")]
    pub at_ms: u64,
    pub machine: String,
    pub profile: Option<String>,
    pub run_id: String,
    #[ts(type = "number")]
    pub value: u64,
    pub samples: u32,
    pub outcome: String,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceDashboard {
    pub workspaces: Vec<RepoWorkspace>,
    pub workspace_id: Option<String>,
    pub runs: Vec<MetricRun>,
    pub counts_run: Option<Box<MetricRun>>,
    pub code_lines: Vec<CodeLines>,
    pub verification_trend: Vec<MetricPoint>,
    pub build_trend: Vec<MetricPoint>,
    pub artifact_trend: Vec<MetricPoint>,
    pub profiles: Vec<String>,
    #[ts(type = "number")]
    pub queried_at_ms: u64,
    pub truncated: bool,
}
