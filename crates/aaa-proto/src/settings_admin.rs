//! 웹 설정 관리(관리 GUI W5c, plan §8.356): profile·safety 읽기·편집(host가 host.toml을 `toml_edit`로 고친다), 머신마다 작업
//! root 보기·점검·전환(agent work_root.manage.v1), Codex hook 연동 상태·설치·제거(agent hook.manage.v1).
//! 웹 조작 기기는 임의 명령을 심지 못한다(사용자 답 1A·2A): custom profile의 `command`·`env`는 이 요청에 칸이 없고, safety의
//! 명령·인자 목록(`*_agent_args`, `routine_commands`)은 읽기만 한다. host.toml 글은 웹에 보내지 않는다(env 값이 들어 있다).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// safety·허용 목록을 고칠 표: 이 host 프로젝트(`[safety.<프로젝트>]`) 또는 모든 프로젝트 기본(`[safety.default]`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum SettingsScope {
    #[default]
    Project,
    Global,
}

/// safety 한 줄(`aaa safety show`와 같다)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SafetyRowView {
    pub key: String,
    pub value: String,
    /// 프로젝트·모든 프로젝트·옛 자리·기본값
    pub source: String,
    /// 적힌 값과 다른 까닭(yolo가 start_threads를 auto로 만든다 등)
    pub note: String,
    /// 웹에서 바꿀 수 있는 key(yolo·start_threads·trust_screens·coordinator_deploy)
    pub editable: bool,
}

/// `safety_show`·`safety_set`의 답
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SafetyView {
    pub scope: SettingsScope,
    pub project: String,
    pub rows: Vec<SafetyRowView>,
    /// thread·coordinator 기본 profile(프로젝트 설정 → host.toml [defaults] → claude)
    pub thread_profile: String,
    pub coordinator_profile: String,
    /// thread·coordinator가 쓸 수 있는 profile 실효값(`None`이면 모든 profile). 허용 목록 고치기의 처음 값
    pub thread_profiles: Option<Vec<String>>,
    pub coordinator_profiles: Option<Vec<String>>,
    /// 읽다 버린 값 등
    pub warnings: Vec<String>,
    /// host.toml 판(sha256). 고칠 때 이 값을 돌려준다(그사이 바뀌었으면 conflict)
    pub rev: String,
    /// 바꾼 뒤 알림(적용 시점, host.local.toml이 덮음 등). 읽기에는 없다
    #[serde(default)]
    pub notes: Vec<String>,
}

/// host.toml profile 하나(웹 보기). env는 이름만 싣는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProfileAdminRow {
    pub name: String,
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub description: String,
    pub args: Vec<String>,
    pub env_names: Vec<String>,
    /// host.toml에 표가 없는 기본 profile(claude·codex)
    pub builtin: bool,
    /// custom harness(실행 명령이 있다): 웹에서 고치지 않는다
    pub custom: bool,
    /// harness 실행 파일 뒤에 붙는 인자 전체(model·effort 플래그와 제 인자). custom은 앞 명령을 빼고 보인다
    pub launch: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProfileAdminView {
    pub profiles: Vec<ProfileAdminRow>,
    pub rev: String,
}

/// 웹이 고치는 profile·허용 목록·기본값. harness는 claude·codex·shell만(custom은 host CLI)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum ProfileChangeReq {
    Add {
        name: String,
        harness: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        effort: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        args: Vec<String>,
    },
    Edit {
        name: String,
        #[serde(default)]
        harness: Option<String>,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        effort: Option<String>,
        #[serde(default)]
        description: Option<String>,
        /// 인자 전체를 바꾼다(빈 목록이면 지운다)
        #[serde(default)]
        args: Option<Vec<String>>,
    },
    Remove {
        name: String,
    },
    /// thread·coordinator가 쓸 수 있는 profile(`None`이면 모든 profile)
    Allow {
        scope: SettingsScope,
        /// thread 또는 coordinator
        role: String,
        names: Option<Vec<String>>,
    },
    /// host.toml `[defaults]` thread·coordinator 기본 profile
    Default {
        role: String,
        name: String,
    },
}

/// `profile_plan`의 답: 적용하면 무엇이 되는가(파일 글이 아니라 바뀌는 profile과 실행 인자)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProfilePlanView {
    pub message: String,
    pub before: Option<ProfileAdminRow>,
    pub after: Option<ProfileAdminRow>,
    /// 경고(host.local.toml이 덮음, 허용 목록에 없는 profile 등)
    pub warnings: Vec<String>,
    pub rev: String,
}

/// 머신의 작업 root(`aaa work-root show`와 같다). 마지막 시작 기록은 지금 연결의 증명이 아니다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkRootView {
    pub path: String,
    pub mount: Option<String>,
    pub last_start: Option<String>,
    pub last_start_mount: Option<String>,
    pub previous_roots: Vec<String>,
    /// 다음 시작 설정과 마지막 시작 기록이 다르다(agent를 다시 띄워야 닿는다)
    pub restart_required: bool,
    /// 전환했을 때만: 설정을 바꿨나, 가져온 준비 기록 수
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub changed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub imported: Option<u32>,
}

/// 점검을 통과한 작업 root(쓰기·읽기·삭제와 mount)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkRootCheckView {
    pub path: String,
    pub real_path: String,
    pub mount: Option<String>,
}

/// agent에 보내는 작업 root 일(work_root.manage.v1)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WorkRootOp {
    Show,
    Check { path: Option<String>, mount: Option<String> },
    Set { path: String, mount: Option<String> },
}

/// 머신의 harness hook 연동 상태(hook.manage.v1). Claude는 실행마다 hook을 주어 설치할 것이 없다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HookView {
    /// aaa Codex 전역 hook이 있고 hooks 기능이 켜져 있다
    pub codex_installed: bool,
    /// 상태를 읽지 못한 까닭(Codex 설정 파일이 깨짐 등)
    pub codex_problem: Option<String>,
    /// 설치·제거 뒤 만든 백업 수
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub backups: Option<u32>,
}
