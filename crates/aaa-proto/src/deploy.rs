//! host가 출처와 safety를 판정한 배포 작업. 임의 셸 명령은 받지 않는다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DeployAction {
    Update { from: Option<String>, version: Option<String>, dry_run: bool },
    Rollback { name: Option<String>, dry_run: bool },
    Restart { service: DeployService },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeployService {
    Host,
    Agent,
}
impl DeployService {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Host => "host",
            Self::Agent => "agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeployState {
    Queued,
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeployJob {
    pub id: String,
    pub project: String,
    pub action: DeployAction,
    pub state: DeployState,
    pub instance: String,
    pub executor_pid: u32,
    pub expected_sha256: String,
    pub result: Option<String>,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
}
