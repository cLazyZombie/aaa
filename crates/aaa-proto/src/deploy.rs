//! host가 출처와 safety를 판정한 배포 작업. 임의 셸 명령은 받지 않는다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum DeployAction {
    Update {
        from: Option<String>,
        version: Option<String>,
        dry_run: bool,
    },
    CheckedUpdate {
        from: String,
        sha256: String,
        dry_run: bool,
    },
    UpdateStaged {
        rollout: String,
        dry_run: bool,
    },
    Rollback {
        name: Option<String>,
        dry_run: bool,
    },
    /// rollout에 먼저 저장한 배포 전 SHA만 복구한다. 최신 보관본 추정은 하지 않는다.
    CheckedRollback {
        sha256: String,
    },
    Restart {
        service: DeployService,
    },
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
    /// 재시작만 하는 rollout(관리 GUI W6c, 답 1A): 대상 한 머신의 이 서비스를 지금 판 그대로 다시 띄우고 guard로 확인한다.
    /// 후보·전송·교체가 없고 `from`·`sha256`은 host가 재시작 전 실행 신원으로 채운다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<DeployService>,
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
    #[serde(default)]
    pub baseline: Option<RuntimeIdentity>,
    #[serde(default)]
    pub guard: Option<DeployGuardSpec>,
    #[serde(default)]
    pub guard_ready: bool,
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
    Recovering,
    Stopped,
    StoppedRolledBack,
    RecoveryFailed,
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
    /// 없는 옛 rollout은 L1/L2 정책으로만 재개한다.
    #[serde(default)]
    pub recovery_enabled: bool,
    #[serde(default)]
    pub recovery: Option<Box<DeployGuardView>>,
    #[serde(default)]
    #[ts(type = "number")]
    pub recovery_started_ms: u64,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RuntimeIdentity {
    pub sha256: String,
    pub software: String,
    pub instance: String,
    #[serde(default)]
    #[ts(type = "number")]
    pub started_ms: u64,
    /// host가 실제로 연 웹 주소(loopback, 관리 GUI W6c, FRONT_API 1.100). 같은 머신 agent에만 싣는다. host 감시는 이 주소의
    /// `/api/v1/me`가 답해야 새 판 준비로 본다. 없으면(웹을 끈 설정·옛 host·agent 신원) 실행 신원만 본다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<String>,
}

/// restart 작업 ID를 guard ID로 쓴다. 복구 ID도 실행 전에 host DB에 고정한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeployGuardSpec {
    pub id: String,
    pub rollout: String,
    pub project: String,
    pub service: DeployService,
    pub host_machine: bool,
    pub baseline: RuntimeIdentity,
    pub expected_sha256: String,
    pub expected_software: String,
    pub old_host_instance: String,
    pub install_job: Option<String>,
    pub rollback_job: String,
    pub restart_job: String,
    pub host_restart_job: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeployGuardPhase {
    Armed,
    AwaitingNew,
    NewReady,
    Restoring,
    Restarting,
    AwaitingOld,
    VerifiedNew,
    Restored,
    Failed,
    Cancelled,
}
impl DeployGuardPhase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::VerifiedNew | Self::Restored | Self::Failed | Self::Cancelled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeployGuardView {
    pub spec: DeployGuardSpec,
    pub phase: DeployGuardPhase,
    pub worker_pid: u32,
    #[ts(type = "number")]
    pub deadline_ms: u64,
    pub install_started: bool,
    pub activated: bool,
    pub restore_started: bool,
    pub restart_index: u8,
    pub restart_started: bool,
    #[ts(type = "number")]
    pub host_restart_ms: u64,
    #[ts(type = "number")]
    pub agent_restart_ms: u64,
    pub observed: Vec<DeployService>,
    pub installed_sha256: String,
    pub error: Option<String>,
    /// 실행 신원은 맞고 웹 응답을 기다리는 host 웹 주소(W6c). 답하면 NewReady, 시한이 지나면 복구
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_web: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum DeployGuardControl {
    Status,
    BeginInstall { job_id: String },
    Activate,
    Observe { service: DeployService, identity: RuntimeIdentity },
    Commit,
    Recover { reason: String },
    Cancel,
}

/// 웹 조회용 배포 요약. 원본 경로·실행기/PID·actor·내부 작업 ID는 싣지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RolloutCard {
    pub id: String,
    pub state: RolloutState,
    pub version: Option<String>,
    pub sha256: String,
    pub commit: Option<String>,
    /// 재시작만 하는 rollout의 서비스(W6c). 교체 rollout은 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<DeployService>,
    pub cursor: u32,
    pub steps: Vec<RolloutCardStep>,
    pub error: Option<String>,
    pub recovery_phase: Option<DeployGuardPhase>,
    pub recovery_sha256: Option<String>,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RolloutCardStep {
    pub machine: String,
    pub phase: RolloutPhase,
    pub completed: bool,
    #[ts(type = "number")]
    pub started_ms: u64,
    #[ts(type = "number")]
    pub transferred_bytes: u64,
    #[ts(type = "number")]
    pub total_bytes: u64,
}

/// 빌드 머신에 이미 있는 후보(옆에 `provenance.json`이 있는 `aaa`)를 그 머신 agent가 읽고 dry-run으로 self-check한 결과
/// (agent 기능 deploy_candidate.v1, 관리 GUI W6a). 웹은 실행 파일을 올리지 않고 이것만 고른다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeployCandidateView {
    /// 빌드 머신 이름
    pub machine: String,
    /// 받은 그대로의 후보 경로(`<절대 경로>/aaa` 또는 `~/<폴더>/aaa`)
    pub path: String,
    pub sha256: String,
    /// provenance의 40자리 커밋(없으면 None)
    pub commit: Option<String>,
    /// self-check가 답한 판(`0.1.2+<커밋 12자리>`)
    pub version: String,
    #[ts(type = "number")]
    pub total_bytes: u64,
    /// provenance의 빌드 시각(RFC 3339, 없으면 None)
    pub built_at: Option<String>,
}
/// rollout 대상 한 머신의 시작 전 점검. `problem`이 있으면 시작하지 않는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RolloutTargetView {
    pub machine: String,
    /// host 머신(늘 맨 앞)
    pub host: bool,
    pub problem: Option<String>,
}
/// 웹 「배포」의 시작 전 점검: 후보 dry-run과 대상 순서(host 먼저). 대상 머신의 사전 검사는 rollout이 교체 전에 다시 한다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RolloutPlanView {
    pub candidate: DeployCandidateView,
    pub targets: Vec<RolloutTargetView>,
    /// 진행 중인 rollout이 있어 지금은 시작하지 못한다
    pub busy: bool,
}
/// 개별 배포 작업의 종류. 후보 경로·보관 이름 같은 작업 원문은 웹에 싣지 않는다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeployWork {
    Restart { service: DeployService },
    Update,
    Rollback,
}
impl From<&DeployAction> for DeployWork {
    fn from(a: &DeployAction) -> Self {
        match a {
            DeployAction::Restart { service } => Self::Restart { service: *service },
            DeployAction::Rollback { .. } | DeployAction::CheckedRollback { .. } => Self::Rollback,
            DeployAction::Update { .. } | DeployAction::CheckedUpdate { .. } | DeployAction::UpdateStaged { .. } => {
                Self::Update
            }
        }
    }
}
/// 웹 조회용 개별 배포 작업 요약(재시작). 실행기 PID·결과 원문·actor는 싣지 않는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DeployJobCard {
    pub id: String,
    pub machine: String,
    pub work: DeployWork,
    /// 그 머신 agent에게 물은 상태. 연결이 끊겼거나 기록을 읽지 못하면 None
    pub state: Option<DeployState>,
    /// 실패·중단일 때 화면에 보일 까닭(원문은 coordinator의 배포 기록에만 있다)
    pub error: Option<String>,
    /// 웹 조작 기기가 시작했다(아니면 coordinator)
    pub web: bool,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn previous_rollout_records_keep_their_non_recovery_policy() {
        let r: RolloutView = serde_json::from_value(serde_json::json!({
            "id":"old", "project":"demo", "actor":"coordinator",
            "spec":{"from":"/tmp/candidate/aaa", "sha256":"old", "commit":null,"machines":["mac-a"]},
            "state":"running", "steps":[{"machine":"mac-a","phase":"host_restart","job_id":"same-job",
                "completed":false,"result":null,"host_instance":"old-host","started_ms":1}],
            "cursor":0,"version":"0.1.2","error":null,"finish":null,"created_ms":1,"updated_ms":1
        }))
        .unwrap();
        assert!(!r.recovery_enabled);
        assert!(r.recovery.is_none());
        assert_eq!(r.steps[0].job_id, "same-job");
        assert!(r.steps[0].guard.is_none() && r.steps[0].baseline.is_none() && !r.steps[0].guard_ready);
        assert!(r.spec.source_machine.is_none());
    }
}
