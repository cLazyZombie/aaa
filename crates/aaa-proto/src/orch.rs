//! 오케스트레이션 계약(M1-3): 역할, 프로젝트·thread, profile 해석, 보고 회수, 흐름 사건(§8).
//!
//! - thread 작업 폴더·worktree는 그 머신의 agent가 만든다. host는 git을 직접 실행하지 않고 기록·사본만 갖는다.
//! - 흐름 사건은 프로젝트별 순번(seq)을 가진 durable 기록이다. 구독자는 cursor(마지막 seq)부터 다시 읽는다.

mod library;
pub use library::*;
mod agent_status;
pub use agent_status::*;
mod question;
pub use question::*;
mod thread;
pub use thread::*;
mod brief;
pub use brief::*;
mod routine;
pub use routine::*;
mod task;
use serde::{Deserialize, Serialize};
pub use task::*;
use ts_rs::TS;

use crate::msg::B64;

pub const AGY_WORKER_FEATURE: &str = "agy_worker.v1";
pub const AGY_SUBSCRIPTION_PREFIX: &str = "agy_subscription.v1:";

/// agent 세션의 역할(§8.13). 새 역할을 더해도 이전 기록이 깨지지 않게 문자열로 저장한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Coordinator,
    #[default]
    Worker,
    /// 사후 평가자. 목록 기본값에서 빠지고 `role` 필터로만 보인다.
    Evaluator,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Coordinator => "coordinator",
            Role::Worker => "worker",
            Role::Evaluator => "evaluator",
        }
    }

    pub fn parse(s: &str) -> Option<Role> {
        match s {
            "coordinator" => Some(Role::Coordinator),
            "worker" => Some(Role::Worker),
            "evaluator" => Some(Role::Evaluator),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThreadKind {
    /// repo의 새 worktree와 branch(기본)
    #[default]
    Worktree,
    /// repo 없는 작업 폴더(agent 작업 root 아래)
    Dir,
    /// 사람이 이미 띄운 agent pane을 thread로 기록했다(`thread adopt`, herdr-projects `Kind::Adopted`). 자리를 만들지 않고
    /// 그 pane이 있는 폴더에서 일한다. pane은 사람 것이라 다시 띄우지도 끝내지도 않는다
    Adopted,
}

/// env를 Debug로 찍을 때 값 없이 「«env N개: 이름…»」만 보인다(키 관리자 K0, plan §8.307). 로그·오류 문구가 Debug를 쓴다.
pub struct EnvNames<'a>(pub Vec<&'a str>);

impl<'a> EnvNames<'a> {
    pub fn of(names: impl IntoIterator<Item = &'a String>) -> Self {
        EnvNames(names.into_iter().map(String::as_str).collect())
    }
}

impl std::fmt::Debug for EnvNames<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "«env {}개: {}»", self.0.len(), self.0.join(", "))
    }
}

/// host profile에 그 머신 실행 경로·환경·hook을 적용한 결과. host는 thread 기록에 그대로 저장하고 restart·resume에 다시 쓴다(§7.2).
/// Debug는 env 값을 찍지 않는다([`EnvNames`]).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Launch {
    pub profile: String,
    /// claude·codex·shell·custom
    pub harness: String,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    /// 같은 대화를 이어 띄우는 argv(모를 때는 빈 배열). daemon 금지 인자 등은 argv와 같다.
    pub resume_argv: Vec<String>,
    /// harness 세션 ID(미리 정할 수 있는 harness만, 예: claude `--session-id`)
    pub session_key: Option<String>,
}

impl std::fmt::Debug for Launch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Launch { profile, harness, argv, env, resume_argv, session_key } = self;
        f.debug_struct("Launch")
            .field("profile", profile)
            .field("harness", harness)
            .field("argv", argv)
            .field("env", &EnvNames::of(env.iter().map(|(k, _)| k)))
            .field("resume_argv", resume_argv)
            .field("session_key", session_key)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProfileView {
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub description: String,
    /// profile이 붙이는 인자(옛 agent는 보내지 않는다)
    #[serde(default)]
    pub args: Vec<String>,
    /// host.toml에 없는 기본 profile(claude·codex): 그 머신에 설치·로그인한 것만 온다
    #[serde(default)]
    pub builtin: bool,
}

/// host → agent: thread 자리를 만든다(worktree 또는 작업 폴더)와 brief.md 쓰기, profile 해석
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPrepare {
    /// host가 푼 profile. host_profiles.v1 기능이 있는 agent에만 보낸다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_profile: Option<ResolvedProfile>,
    pub project: String,
    pub thread: String,
    pub title: String,
    pub kind: ThreadKind,
    /// worktree일 때 그 머신의 repo 경로
    pub repo: Option<String>,
    /// 기준 ref(없으면 origin/HEAD, 현재 branch 순)
    pub base: Option<String>,
    pub profile: String,
    pub brief: String,
    /// thread를 띄울 때 safety(plan §8.163). `launch_safety.v1`을 알린 agent에만 보낸다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safety: Option<LaunchSafety>,
}

/// host에서 해석한 실행 설정. command·env는 옛 shell/custom profile의 호환 필드이며 머신 덮어쓰기가 우선이다.
/// Debug는 env 값을 찍지 않는다([`EnvNames`]).
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedProfile {
    pub harness: String,
    pub model: String,
    pub effort: String,
    pub args: Vec<String>,
    pub command: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
}

impl std::fmt::Debug for ResolvedProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ResolvedProfile { harness, model, effort, args, command, env } = self;
        f.debug_struct("ResolvedProfile")
            .field("harness", harness)
            .field("model", model)
            .field("effort", effort)
            .field("args", args)
            .field("command", command)
            .field("env", &EnvNames::of(env.keys()))
            .finish()
    }
}

/// host → agent: thread 인자에 더할 safety(herdr-projects `profiles::launch_args`·`Safety::launch_args`). host는 profile의
/// harness를 모르므로 agent가 profile을 풀고 붙인다.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LaunchSafety {
    /// harness의 권한 건너뛰기 인자(claude `--dangerously-skip-permissions`, codex `--dangerously-bypass-approvals-and-sandbox`)
    pub yolo: bool,
    /// 프로젝트 기본 profile과 같은 harness의 기본 profile에만 붙인다(사람이 정의한 profile은 제 인자를 쓴다)
    pub thread_agent_args: Vec<String>,
    /// 프로젝트 기본 profile 이름: 그 harness가 `thread_agent_args`의 harness다(못 풀면 claude)
    pub default_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadReady {
    /// agent가 시작할 폴더(worktree 또는 작업 폴더)
    pub cwd: String,
    /// `<cwd>/.aaa/<project>-<thread>/`(brief.md·report.md·library/)
    pub work_dir: String,
    pub branch: Option<String>,
    /// 기준 commit
    pub base: Option<String>,
    pub launch: Launch,
}

/// agent journal에서 회수한 자리. 아직 launch를 만들지 못한 준비도 정리에 넘길 수 있다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPlacement {
    pub cwd: String,
    pub work_dir: String,
    pub branch: Option<String>,
    pub base: Option<String>,
}

/// 사람이 띄운 agent pane을 thread로 데려온다(`thread adopt`): 그 세션이 지금 있는 폴더에 작업 폴더를 만들고 brief.md를 쓴다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadAdopt {
    pub project: String,
    pub thread: String,
    /// 데려올 세션(pane) ID
    pub session: String,
    pub brief: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadAdopted {
    /// 세션이 지금 있는 폴더
    pub cwd: String,
    /// `<cwd>/.aaa/<project>-<thread>/`(brief.md·report.md·library/)
    pub work_dir: String,
    /// cwd가 git repo 안이면 그 top
    pub repo: Option<String>,
    /// 지금 branch(떼어 낸 HEAD면 None)
    pub branch: Option<String>,
}

// ───────────────────────── front 쪽 보기 ─────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectView {
    #[serde(default)]
    #[ts(optional, as = "Option<crate::AutoMode>")]
    pub auto_mode: crate::AutoMode,
    pub name: String,
    pub path: String,
    pub threads: u32,
    /// FRONT_API 1.33. 옛 host는 보내지 않는다(active)
    #[serde(default)]
    #[ts(optional, as = "Option<ProjectStatus>")]
    pub status: ProjectStatus,
    /// `project open`이 profile 없이 쓸 coordinator profile(project.toml → host.toml `[defaults]` → claude로 푼 것).
    /// FRONT_API 1.33. 옛 host는 보내지 않는다(빈 글)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    #[ts(optional, as = "Option<String>")]
    pub coordinator_profile: String,
    /// coordinator가 쓸 수 있는 profile(safety `coordinator_profiles`, 사람만 바꾼다). 없으면 모두다. 웹 「coordinator 열기」가
    /// 고르기 목록을 거른다(참고 popup `profile_picker`의 `usable`). FRONT_API 1.33
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub coordinator_profiles: Option<Vec<String>>,
}

/// 프로젝트 상태(herdr-projects `project::Status`). paused·archived는 ticker가 건너뛰고 `thread start`를 거부한다.
/// archived는 목록에서 숨고 `project open`을 거부한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectStatus {
    #[default]
    Active,
    Paused,
    Archived,
}

impl ProjectStatus {
    pub fn is_active(&self) -> bool {
        *self == ProjectStatus::Active
    }
}

/// `project pause|resume|archive|unarchive`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAction {
    Pause,
    Resume,
    Archive,
    Unarchive,
}

/// 프로젝트 관리 명령(상태·삭제·이름·청소)의 결과: 바뀐 뒤 프로젝트와 사람이 읽을 줄
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectChange {
    pub project: ProjectView,
    pub notes: Vec<String>,
}

/// `doctor files`의 프로젝트 한 줄(herdr-projects `doctor`의 `files <slug>`, FRONT_API 1.34)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectFilesCheck {
    pub project: String,
    /// 본 때 고칠 것(AGENTS.md·uploads/). 비면 맞다
    pub problems: Vec<String>,
    /// `fix`로 고쳤다
    pub fixed: bool,
    /// 고치지 못한 까닭
    pub error: Option<String>,
}

/// `project open`의 결과(herdr-projects `open`, FRONT_API 1.33)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectOpened {
    pub project: ProjectView,
    /// 띄운 coordinator, 또는 이미 돌던 것 중 가장 최근에 상태가 바뀐 것
    pub pane: crate::PaneView,
    /// 새로 띄웠다(false면 돌던 coordinator)
    pub started: bool,
    /// `pane` 말고도 도는 coordinator 수
    pub others: u32,
    /// coordinator를 띄운 profile(돌던 것이면 그 pane의 harness)
    pub profile: String,
    /// 그 pane이 든 workspace(layout에 없으면 빈 글)
    pub workspace: String,
    pub notes: Vec<String>,
}

/// `project_settings`의 답(관리 GUI W4, plan §8.343): 「이 aaa」·「저장소」 설정 화면이 읽는 프로젝트 설정. 값이 잘못된 key는
/// host가 쓰는 기본값이다(`aaa project set`이 고치는 값과 같다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectSettingsView {
    pub project: String,
    /// 설정 파일 이름(`aaa.toml`, 없으면 옛 `project.toml`)
    pub file: String,
    pub status: ProjectStatus,
    pub goal: String,
    pub thread_profile: String,
    pub coordinator_profile: String,
    pub max_parallel_threads: u32,
    pub auto_resolve_days: u32,
    pub nudge: bool,
    pub mute: bool,
    /// 저장소 목록(`[[workspaces]]`·`[[repos]]`). 머신이 없으면 host 머신이다
    pub repos: Vec<RepoChoice>,
    /// host 로컬 머신 이름(`RepoChoice.machine`이 빈 저장소의 머신, 모르면 없다)
    pub local_machine: Option<String>,
}

/// `thread_resolve_preview`의 답(관리 GUI W4): 끝내기 전에 잃을 수 있는 것. host는 기록을 고치지 않고, git은 그 머신 agent가
/// 읽는다(diff.v1). 읽지 못한 것은 `None`(모름)이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ResolvePreview {
    pub project: String,
    pub thread: String,
    /// thread 종류(응답 꼬리표 `kind`와 겹치지 않게 이름이 다르다)
    pub thread_kind: ThreadKind,
    /// 끝내면 지울 수 있는 worktree가 있다(worktree thread이고 작업 폴더 기록이 있다)
    pub worktree: bool,
    pub branch: Option<String>,
    /// branch의 현재 끝이 origin/main에 포함된다
    pub merged: bool,
    /// 마지막 사본 상태(`complete`·`partial`, 없으면 아직 사본 없음)
    pub copy: Option<String>,
    pub copy_notes: Vec<String>,
    /// HEAD 대비 커밋하지 않은 파일 수
    pub uncommitted: Option<u32>,
    /// base(`origin/main` 등) 대비 바뀐 파일 수(커밋하지 않은 것 포함)
    pub changed: Option<u32>,
    /// git을 읽지 못한 까닭(머신 끊김·옛 agent·저장소 없음)
    pub unknown: Option<String>,
    /// 끝내기 창이 보일 위험(한 줄씩)
    pub risks: Vec<String>,
    /// 위험이 있거나 모르면 「worktree 남기기」를 기본으로 켠다
    pub keep_default: bool,
}

/// 프로젝트 이름: 영문 소문자·숫자·`-`·`_`(경로에 쓰므로 엄격히 본다)
pub fn valid_project(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 48
        && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        && !s.starts_with('-')
}

/// thread ID: `t-` 다음 숫자 4자리 이상
pub fn valid_thread(s: &str) -> bool {
    s.strip_prefix("t-").is_some_and(|n| n.len() >= 4 && n.chars().all(|c| c.is_ascii_digit()))
}

pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let mut s: String = out.trim_matches('-').chars().take(40).collect();
    while s.ends_with('-') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests;
