//! host가 출처와 safety를 판정한 배포 작업. 임의 셸 명령은 받지 않는다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DeployAction {
    Update { from: Option<String>, version: Option<String>, dry_run: bool },
    CheckedUpdate { from: String, sha256: String, dry_run: bool },
    UpdateStaged { rollout: String, dry_run: bool },
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

/// host DB의 rollout 정본. 작업 ID는 전송 전에 저장하고 재개 때도 그대로 쓴다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RolloutSpec {
    pub from: String,
    /// Some이면 이 머신의 빌드 후보를 Agent link로 가져온다. None은 L1 미리 복사 경로다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_machine: Option<String>,
    pub sha256: String,
    pub commit: Option<String>,
    pub machines: Vec<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RolloutPhase {
    Transfer,
    Preflight,
    HostUpdate,
    HostRestart,
    AgentUpdate,
    AgentRestart,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RolloutStep {
    pub machine: String,
    pub phase: RolloutPhase,
    pub job_id: String,
    pub completed: bool,
    pub result: Option<String>,
    /// 재시작 전에 기록한 host instance. OS boot ID와 별개다.
    pub host_instance: Option<String>,
    #[ts(type = "number")]
    pub started_ms: u64,
    #[serde(default)]
    #[ts(type = "number")]
    pub transferred_bytes: u64,
    #[serde(default)]
    #[ts(type = "number")]
    pub total_bytes: u64,
}

/// 조각은 base64를 포함해 1MiB frame보다 작고, 한 번에 하나만 보낸다.
pub const DEPLOY_CHUNK: usize = 256 * 1024;
pub const DEPLOY_MAX_BYTES: u64 = 256 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeployStageView {
    pub rollout: String,
    pub project: String,
    pub sha256: String,
    #[ts(type = "number")]
    pub total: u64,
    #[ts(type = "number")]
    pub offset: u64,
    pub complete: bool,
    pub version: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RolloutState {
    Running,
    Stopped,
    Done,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RolloutView {
    pub id: String,
    pub project: String,
    pub actor: String,
    pub spec: RolloutSpec,
    pub state: RolloutState,
    pub steps: Vec<RolloutStep>,
    pub cursor: u32,
    pub version: Option<String>,
    pub error: Option<String>,
    pub finish: Option<String>,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
}
