//! `aaa.install.v1` 공유 계약. CLI·질문·SSH는 같은 choices를 제출한다.
//! I0는 데이터/순수 검증만 제공한다. 승인서 구조는 권한 검증이나 등록 기능이 아니다.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! choice {
    ($name:ident { $($variant:ident),+ }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, TS)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name { $($variant),+ }
    };
}
macro_rules! state {
    ($name:ident { $($variant:ident),+ }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),+ }
    };
}
choice!(InstallRole { Host, Agent });
choice!(InstallOs { Macos, Linux, Windows });
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum InstallArch {
    Aarch64,
    X86_64,
}
choice!(InstallMode { Fresh, Repair, Reinstall });
choice!(InstallToggle { Enabled, Disabled });
choice!(InstallProjectMode { Create, Use, Skip });
choice!(InstallStorage { Internal });
choice!(InstallHarness { Claude, Codex, None });
choice!(InstallLogin { Require, Defer });
choice!(InstallHooks { Install, Preserve, Skip });
choice!(InstallService { Auto, Launchd, SystemdSystem, SystemdUser, WindowsScm, None });
choice!(InstallAccount { CurrentUser });
choice!(InstallPasswordSource { Prompt, Stdin, Broker });
choice!(InstallStartup { Auto, Manual });
choice!(InstallStart { Now, Deferred });
choice!(InstallLinger { Require, Preserve });
choice!(InstallPathPolicy { Preserve, User });
choice!(InstallLauncherPolicy { Install, Preserve });
choice!(InstallNetwork { Tailscale, Direct });
choice!(InstallWeb { Local, Off });
choice!(InstallServe { On, Off });
choice!(InstallPairing { Controller, Skip });
choice!(InstallEnroll { Wait, Grant, Skip });
choice!(InstallOpen { Yes, No });
choice!(InstallPrompt { Auto, Plain });
state!(InstallStatus { Planned, Completed, Unchanged, Configured, NeedsUser, Failed, Cancelled });
state!(InstallChangeState { Planned, Applied, Unchanged, Pending, Reverted, Failed });
state!(InstallCheckState { Passed, Failed, NotRun, NotRequested, Unknown });
state!(InstallRollbackState { NotRequested, Completed, Partial, Failed });
state!(InstallPhase {
    Detect,
    Preflight,
    Plan,
    Authorize,
    Stage,
    Configure,
    Hooks,
    WorkRoot,
    Service,
    Enroll,
    Web,
    Pairing,
    Verify,
    Rollback
});
state!(InstallEventState { Pending, Running, Completed, Unchanged, Skipped, NeedsUser, Failed, Reverted });
state!(InstallOperation {
    WriteFile,
    StageBinary,
    StageLauncher,
    ConfigureHook,
    PrepareWorkRoot,
    RegisterService,
    StartService,
    PublishWeb,
    EnrollAgent,
    CreateProject,
    ModifyPath
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub enum InstallSchema {
    #[serde(rename = "aaa.install.v1")]
    V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub enum InstallPlanSchema {
    #[serde(rename = "aaa.install.plan.v1")]
    V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub enum InstallEventSchema {
    #[serde(rename = "aaa.install.event.v1")]
    V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub enum InstallAuthorizationSchema {
    #[serde(rename = "aaa.install.authorization.v1")]
    V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub enum InstallGrantSchema {
    #[serde(rename = "aaa.install.enroll-grant.v1")]
    V1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallPlatform {
    pub os: InstallOs,
    pub arch: InstallArch,
}

/// 원시 입력에는 비밀값을 받는 필드가 없다. CLI는 kebab-case 이름을 snake_case 필드로 옮긴다.
/// 기본값과 필수/상충 판정은 aaa-config::install::plan 한 곳에서 한다.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(default, deny_unknown_fields)]
pub struct InstallChoices {
    pub role: Option<InstallRole>,
    pub instance: Option<String>,
    pub config_dir: Option<String>,
    pub state_dir: Option<String>,
    pub mode: Option<InstallMode>,
    pub name: Option<String>,
    pub local_agent: Option<InstallToggle>,
    pub bin_path: Option<String>,
    pub version: Option<String>,
    pub from: Option<String>,
    pub sha256: Option<String>,
    pub expected_os: Option<InstallOs>,
    pub expected_arch: Option<InstallArch>,
    pub project_mode: Option<InstallProjectMode>,
    pub project: Option<String>,
    pub projects_root: Option<String>,
    pub goal: Option<String>,
    pub work_root: Option<String>,
    pub work_mount: Option<String>,
    pub work_storage: Option<InstallStorage>,
    pub min_free_gb: Option<u32>,
    pub default_cwd: Option<String>,
    pub shell: Option<String>,
    pub claude_path: Option<String>,
    pub codex_path: Option<String>,
    pub claude_home: Option<String>,
    pub codex_home: Option<String>,
    pub login: Option<InstallLogin>,
    pub hooks: Option<InstallHooks>,
    pub thread_profile: Option<String>,
    pub coordinator_profile: Option<String>,
    pub service: Option<InstallService>,
    pub service_account: Option<InstallAccount>,
    pub service_password_source: Option<InstallPasswordSource>,
    pub startup: Option<InstallStartup>,
    pub start: Option<InstallStart>,
    pub linger: Option<InstallLinger>,
    pub path: Option<InstallPathPolicy>,
    pub launcher: Option<String>,
    pub launcher_policy: Option<InstallLauncherPolicy>,
    pub host: Option<String>,
    pub agent_listen: Option<String>,
    pub web_listen: Option<String>,
    pub network: Option<InstallNetwork>,
    pub web: Option<InstallWeb>,
    pub tailscale_serve: Option<InstallServe>,
    pub serve_port: Option<u16>,
    pub pairing: Option<InstallPairing>,
    pub enroll: Option<InstallEnroll>,
    pub wait_timeout: Option<u32>,
    pub open_coordinator: Option<InstallOpen>,
    pub repo: Vec<String>,
    pub harness: Vec<InstallHarness>,
    pub harness_env: Vec<InstallHarnessEnv>,
    pub profile_file: Vec<String>,
    pub allow_thread_profile: Vec<String>,
    pub allow_coordinator_profile: Vec<String>,
    pub no_repo: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallHarnessEnv {
    pub harness: InstallHarness,
    pub key: String,
    pub value: String,
}

/// 파일 내용/handle은 이 객체에 포함하지 않는다. I6가 private 채널을 소비한다.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(default, deny_unknown_fields)]
pub struct InstallInvocation {
    pub choices: InstallChoices,
    pub non_interactive: bool,
    pub yes: bool,
    pub dry_run: bool,
    pub json: bool,
    pub json_stream: bool,
    pub prompt: Option<InstallPrompt>,
    pub events_file: Option<String>,
    pub authorization_stdin: bool,
    pub authorization_file: Option<String>,
    pub secrets_stdin: bool,
    pub service_password_ref: Option<String>,
    pub resume: Option<String>,
    pub status: Option<String>,
}

/// 탐지 adapter가 읽은 값. I0는 파일·env·OS·network를 읽지 않는다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallPlanContext {
    pub platform: InstallPlatform,
    pub version: String,
    pub binary_sha256: String,
    pub bin_path: String,
    pub config_dir: String,
    pub state_dir: String,
    pub projects_root: String,
    pub default_shell: String,
    pub effective_config_sha256: String,
    pub target_account: String,
}

/// private 계획. 공유 결과/일반 journal에 직렬화하지 않는다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallPlan {
    pub schema: InstallPlanSchema,
    pub context: InstallPlanContext,
    pub choices: InstallChoices,
}

/// 공개 오류에는 입력 원문·OS 하위 오류를 싣지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS, thiserror::Error)]
#[error("{code}: {message}")]
#[serde(deny_unknown_fields)]
pub struct InstallError {
    pub code: String,
    pub message: String,
    pub details: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallChange {
    pub kind: String,
    pub action: String,
    pub state: InstallChangeState,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallCheck {
    pub name: String,
    pub state: InstallCheckState,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallNeedsUser {
    pub code: String,
    pub target: String,
    pub message: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallRollback {
    pub state: InstallRollbackState,
    pub remaining: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallResume {
    pub installation_id: String,
}

/// 사실을 확인하지 못한 단계는 unknown이며 설정 기록만으로 completed를 만들지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum InstallOutcomeState {
    Pending,
    Running,
    Completed,
    Unchanged,
    NotRequested,
    NeedsUser,
    Failed,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum InstallOwnership {
    ThisInstance,
    External,
    Unknown,
    NotRequested,
}
state!(InstallCredentialRetainer { WindowsScm });
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallServiceSummary {
    pub manager: InstallService,
    pub account_mode: Option<InstallAccount>,
    pub requested_user_matched: InstallCheckState,
    pub registered: InstallCheckState,
    pub running: InstallCheckState,
    pub application_ready: InstallCheckState,
    pub startup: Option<InstallStartup>,
    pub credential_retained_by: Option<InstallCredentialRetainer>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallServeSummary {
    pub requested: bool,
    pub state: InstallOutcomeState,
    pub serve_port: Option<u16>,
    pub target_port: Option<u16>,
    pub reused: Option<bool>,
    pub ownership: InstallOwnership,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallPairingSummary {
    pub state: InstallOutcomeState,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallWebSummary {
    pub local_listen: Option<String>,
    pub tailscale_serve: InstallServeSummary,
    pub pairing: InstallPairingSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallResult {
    pub schema: InstallSchema,
    pub installation_id: Option<String>,
    pub plan_sha256: Option<String>,
    pub status: InstallStatus,
    pub exit_code: u8,
    pub role: Option<InstallRole>,
    pub instance: Option<String>,
    pub platform: Option<InstallPlatform>,
    pub version: Option<String>,
    pub binary_sha256: Option<String>,
    pub changes: Vec<InstallChange>,
    pub checks: Vec<InstallCheck>,
    pub needs_user: Vec<InstallNeedsUser>,
    pub error: Option<InstallError>,
    pub rollback: InstallRollback,
    pub resume: Option<InstallResume>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<InstallServiceSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<InstallWebSummary>,
}

impl InstallResult {
    /// 문법 오류도 같은 envelope. 입력/하위 오류 원문을 이 생성자에 넘기지 않는다.
    pub fn invalid() -> Self {
        Self {
            schema: InstallSchema::V1,
            installation_id: None,
            plan_sha256: None,
            status: InstallStatus::Failed,
            exit_code: 2,
            role: None,
            instance: None,
            platform: None,
            version: None,
            binary_sha256: None,
            changes: vec![],
            checks: vec![],
            needs_user: vec![],
            error: Some(InstallError {
                code: "invalid_arguments".into(),
                message: "설치 인자를 확인하세요".into(),
                details: vec![],
            }),
            rollback: InstallRollback { state: InstallRollbackState::NotRequested, remaining: vec![] },
            resume: None,
            service: None,
            web: None,
        }
    }
    pub fn valid_exit(&self) -> bool {
        match self.status {
            InstallStatus::Planned
            | InstallStatus::Completed
            | InstallStatus::Unchanged
            | InstallStatus::Configured => self.exit_code == 0,
            InstallStatus::NeedsUser => self.exit_code == 3 && !self.needs_user.is_empty(),
            InstallStatus::Cancelled => self.exit_code == 130,
            InstallStatus::Failed => matches!(self.exit_code, 1 | 2 | 4 | 5 | 6) && self.error.is_some(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallProgress {
    #[ts(type = "number")]
    pub done: u64,
    #[ts(type = "number")]
    pub total: u64,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum InstallStreamItem {
    Event {
        schema: InstallEventSchema,
        installation_id: Option<String>,
        #[ts(type = "number")]
        seq: u64,
        phase: InstallPhase,
        state: InstallEventState,
        code: String,
        message: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        progress: Option<InstallProgress>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        needs_user: Option<InstallEventNeedsUser>,
    },
    Result {
        schema: InstallEventSchema,
        installation_id: Option<String>,
        #[ts(type = "number")]
        seq: u64,
        result: Box<InstallResult>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallEventNeedsUser {
    pub kind: String,
    pub resume_id: Option<String>,
}

/// 승인된 작업의 typed 범위. 자유 shell·safety·기기 추가 권한은 표현할 수 없다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallPermission {
    pub operation: InstallOperation,
    pub target: String,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
}

/// 서명 전 claims. 이 구조체를 deserialize했다는 사실은 승인이 아니다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallAuthorizationClaims {
    pub schema: InstallAuthorizationSchema,
    pub installation_id: String,
    pub plan_sha256: String,
    pub platform: InstallPlatform,
    pub target_account: String,
    pub instance: Option<String>,
    pub binary_sha256: String,
    pub permissions: Vec<InstallPermission>,
    #[ts(type = "number")]
    pub issued_at_ms: u64,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
    pub nonce: String,
    pub issuer: String,
}
/// private 입력. Debug를 구현하지 않으며 public 결과나 TS 출력에 포함하지 않는다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallAuthorization {
    pub claims: InstallAuthorizationClaims,
    pub signature: String,
    pub enrollment: Option<InstallEnrollmentGrant>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallEnrollmentGrant {
    pub schema: InstallGrantSchema,
    pub installation_id: String,
    pub plan_sha256: String,
    pub agent_identity: String,
    pub host_identity: String,
    pub issuer: String,
    pub expires_at_ms: u64,
    pub nonce: String,
    pub signature: String,
}

/// Controller가 검토한 원격 제안. Debug/일반 로그에는 private 계획을 싣지 않는다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallApprovalRequest {
    pub installation_id: Option<String>,
    pub plan: InstallPlan,
    pub expected_plan_sha256: String,
    pub permissions: Vec<InstallPermission>,
    pub expected_scope_sha256: String,
    pub agent_identity: Option<String>,
}
impl std::fmt::Debug for InstallApprovalRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InstallApprovalRequest([private])")
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallApprovalView {
    pub installation_id: String,
    pub plan_sha256: String,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
}
/// URL·코드·계정 없이 공개 가능한 설치 공개 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct InstallPublication {
    pub serve_port: u16,
    pub target_port: u16,
    pub reused: bool,
    pub owned: bool,
    pub mapping: InstallCheckState,
    pub public_https: InstallCheckState,
}
/// 검증 SSH helper의 읽기 제안. 일반 stdout/Front 상태에는 원래 경로·계정을 싣지 않는다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallProposal {
    pub plan: InstallPlan,
    pub permissions: Vec<InstallPermission>,
    pub result: InstallResult,
}

/// 검증된 SSH bootstrap 전용 private 입력. 일반 apply에서 이 키를 읽어 root를 만들지 않는다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallBootstrapPackage {
    pub issuer_public_key: String,
    pub plan: InstallPlan,
    pub authorization: InstallAuthorization,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallEnrollmentBinding {
    pub installation_id: String,
    pub plan_sha256: String,
}

/// TLS peer 신원과 결속한 설치 grant 요청. grant 자체는 Debug/TS 출력 대상이 아니다.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallEnrollRequest {
    pub proto: crate::Version,
    pub machine_id: String,
    pub machine_name: String,
    pub grant: InstallEnrollmentGrant,
}
impl std::fmt::Debug for InstallEnrollRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InstallEnrollRequest([private])")
    }
}

/// schema/fixture 판정용 묶음. 실제 CLI stdout envelope에는 이 wrapper를 쓰지 않는다.
#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum InstallContract {
    Invocation(Box<InstallInvocation>),
    Plan(Box<InstallPlan>),
    Result(Box<InstallResult>),
    Stream(Box<InstallStreamItem>),
    Authorization(Box<InstallAuthorization>),
    Grant(Box<InstallEnrollmentGrant>),
}

pub fn schema() -> String {
    serde_json::to_string_pretty(&schemars::schema_for!(InstallContract)).expect("설치 schema") + "\n"
}

pub fn typescript() -> String {
    let cfg = ts_rs::Config::default();
    let declarations = [
        InstallRole::decl(&cfg),
        InstallOs::decl(&cfg),
        InstallArch::decl(&cfg),
        InstallMode::decl(&cfg),
        InstallToggle::decl(&cfg),
        InstallProjectMode::decl(&cfg),
        InstallStorage::decl(&cfg),
        InstallHarness::decl(&cfg),
        InstallLogin::decl(&cfg),
        InstallHooks::decl(&cfg),
        InstallService::decl(&cfg),
        InstallAccount::decl(&cfg),
        InstallPasswordSource::decl(&cfg),
        InstallStartup::decl(&cfg),
        InstallStart::decl(&cfg),
        InstallLinger::decl(&cfg),
        InstallPathPolicy::decl(&cfg),
        InstallLauncherPolicy::decl(&cfg),
        InstallNetwork::decl(&cfg),
        InstallWeb::decl(&cfg),
        InstallServe::decl(&cfg),
        InstallPairing::decl(&cfg),
        InstallEnroll::decl(&cfg),
        InstallOpen::decl(&cfg),
        InstallPrompt::decl(&cfg),
        InstallStatus::decl(&cfg),
        InstallChangeState::decl(&cfg),
        InstallCheckState::decl(&cfg),
        InstallRollbackState::decl(&cfg),
        InstallPhase::decl(&cfg),
        InstallEventState::decl(&cfg),
        InstallOperation::decl(&cfg),
        InstallSchema::decl(&cfg),
        InstallPlanSchema::decl(&cfg),
        InstallEventSchema::decl(&cfg),
        InstallAuthorizationSchema::decl(&cfg),
        InstallGrantSchema::decl(&cfg),
        InstallPlatform::decl(&cfg),
        InstallChoices::decl(&cfg),
        InstallHarnessEnv::decl(&cfg),
        InstallInvocation::decl(&cfg),
        InstallPlanContext::decl(&cfg),
        InstallPlan::decl(&cfg),
        InstallError::decl(&cfg),
        InstallChange::decl(&cfg),
        InstallCheck::decl(&cfg),
        InstallNeedsUser::decl(&cfg),
        InstallRollback::decl(&cfg),
        InstallResume::decl(&cfg),
        InstallOutcomeState::decl(&cfg),
        InstallOwnership::decl(&cfg),
        InstallCredentialRetainer::decl(&cfg),
        InstallServiceSummary::decl(&cfg),
        InstallServeSummary::decl(&cfg),
        InstallPairingSummary::decl(&cfg),
        InstallWebSummary::decl(&cfg),
        InstallResult::decl(&cfg),
        InstallProgress::decl(&cfg),
        InstallStreamItem::decl(&cfg),
        InstallEventNeedsUser::decl(&cfg),
        InstallPermission::decl(&cfg),
        InstallAuthorizationClaims::decl(&cfg),
        InstallApprovalRequest::decl(&cfg),
        InstallApprovalView::decl(&cfg),
        InstallPublication::decl(&cfg),
    ];
    declarations.into_iter().map(|s| format!("export {s}\n\n")).collect()
}

#[cfg(test)]
mod tests;
