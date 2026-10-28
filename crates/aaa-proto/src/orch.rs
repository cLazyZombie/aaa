//! 오케스트레이션 계약(M1-3): 역할, 프로젝트·thread, profile 해석, 보고 회수, 흐름 사건(§8).
//!
//! - thread 작업 폴더·worktree는 그 머신의 agent가 만든다. host는 git을 직접 실행하지 않고 기록·사본만 갖는다.
//! - 흐름 사건은 프로젝트별 순번(seq)을 가진 durable 기록이다. 구독자는 cursor(마지막 seq)부터 다시 읽는다.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::msg::B64;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibFile {
    pub path: String,
    #[ts(type = "number")]
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
    /// 복사 상한을 넘어 agent에 남긴 일반 파일의 크기(이것만 나중에 지연 스트리밍으로 볼 수 있다)
    #[serde(default)]
    #[ts(type = "number | null")]
    pub lazy_size: Option<u64>,
}

/// thread 결과물 파일 하나(host 사본 또는 agent에 남긴 것)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryFile {
    /// library 안 상대 경로
    pub path: String,
    #[ts(type = "number")]
    pub size: u64,
    /// host에 사본이 없다: 볼 때 agent에서 스트리밍한다(작업 폴더가 남아 있을 때만)
    pub on_agent: bool,
    /// 파일 보기가 고를 MIME·종류(파일 보기 `FileInfo`와 같은 표). 옛 목록 파일에는 없어 host가 줄 때 채운다
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub kind: crate::msg::FileKind,
}

/// 웹에서 보는 보고서·결과물. `base`는 짧은 수명의 읽기 전용 경로(`/v/<cap>/`)이고 그 아래에 `path`를 붙인다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryView {
    /// FRONT_API 1.95. 같은 이름·thread의 보고 판(최신 번호부터).
    #[serde(default)]
    pub versions: Vec<String>,
    /// FRONT_API 1.95. 보고의 특정 판을 여는 프로젝트별 영속 번호(r-0001부터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,

    pub project: String,
    pub thread: String,
    /// 회수한 report.md(없으면 None)
    pub report: Option<String>,
    pub files: Vec<LibraryFile>,
    pub base: String,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
    /// FRONT_API 1.36. 조작 권한에만 보이는 보고서 공유 링크. 앱 안 보기는 만료·폐기와 무관하다.
    #[serde(default)]
    pub link: Option<LinkView>,
    /// FRONT_API 1.56. 보고서를 낸 thread(`LibraryReport.from_thread`와 같다). 열람 머리에 보인다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub from_thread: Option<String>,
}

/// FRONT_API 1.36. 보고서 하나의 host 사본만 읽는 서명 링크(§8.197).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LinkView {
    /// FRONT_API 1.95. 보고의 특정 판을 여는 프로젝트별 영속 번호(r-0001부터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,

    pub id: String,
    pub project: String,
    pub thread: String,
    pub base: String,
    /// 공개했을 때만 있는 절대 HTTPS 주소. loopback은 밖 알림에 싣지 않는다.
    pub url: Option<String>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
    #[ts(type = "number | null")]
    pub last_opened_at_ms: Option<u64>,
    pub revoked: bool,
    /// host에 회수한 일반 파일의 합계 바이트
    #[ts(type = "number")]
    pub size: u64,
}

/// FRONT_API 1.38. thread 결과물과 별도로 올린 보고서의 전체 목록 한 줄.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryReport {
    /// FRONT_API 1.122. 연결된 할 일 저장소 이름, 없으면 원본 thread 저장소의 마지막 이름. 모르면 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repo: Option<String>,

    /// FRONT_API 1.114. 보관함의 불변 판. 자동 정리와 상한 계산에서 제외한다.
    #[serde(default)]
    pub archived: bool,
    /// FRONT_API 1.95. 특정 판 사본의 보존 용량. size는 기존 최신 사본 크기다.
    #[serde(default)]
    #[ts(type = "number")]
    pub history_size: u64,
    /// FRONT_API 1.95. 같은 이름·thread의 보고 판(최신 번호부터).
    #[serde(default)]
    pub versions: Vec<String>,
    /// FRONT_API 1.95. 보고의 특정 판을 여는 프로젝트별 영속 번호(r-0001부터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,

    pub project: String,
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub updated_at_ms: u64,
    pub live: bool,
    /// 읽기 실패·상한 초과도 목록에서 감추지 않는다.
    pub problem: Option<String>,
    pub link: Option<LinkView>,
    /// FRONT_API 1.43. 완료 보고 또는 사용자에게 보낸 보고서(「내 보고서」). 자동 표시는 같은 사본을 쓴다.
    #[serde(default)]
    pub to_user: bool,
    /// FRONT_API 1.43. `--to-user`로 보낼 때 준 제목
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub title: Option<String>,
    /// FRONT_API 1.43. 사용자 보고서를 보낸(다시 보낸) 뒤 아직 열지 않았다
    #[serde(default)]
    pub unseen: bool,
    /// FRONT_API 1.43. 진행 중 thread 또는 완료 보관이 참조하는 사본이라 지울 수 없다
    #[serde(default)]
    pub busy: bool,
    /// FRONT_API 1.56. 보고서를 낸 thread(`t-NNNN`, 사용자 요청 10-20). 근원을 모르면 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub from_thread: Option<String>,
}

/// 보고서 하나(프로젝트·이름). FRONT_API 1.43 `LibraryDelete`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryRef {
    pub project: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibrarySkip {
    pub name: String,
    pub reason: String,
}

/// report.md 본문과 library 목록(파일 본문은 `FileRead`로 나눠 받는다)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportSnapshot {
    pub hash: String,
    pub report: B64,
    pub files: Vec<LibFile>,
    pub skipped: Vec<Skipped>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cleanup {
    pub worktree_removed: bool,
    pub branch_deleted: bool,
    pub notes: Vec<String>,
}

/// cleanup_probe.v1: 같은 저장소에서 정리할 자리·branch의 부재를 묶어 확인한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupTarget {
    pub cwd: String,
    pub branch: Option<String>,
}

// ───────────────────────── harness 상태 사건(M1-3b, §8.3) ─────────────────────────

/// agent harness가 지금 하는 일(hook·notify·`aaa report`로 안다. polling하지 않는다)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivity {
    /// harness가 시작했다(첫 입력 전)
    Started,
    /// 턴 진행 중
    Working,
    /// 턴이 끝나 다음 입력을 기다린다
    Idle,
    /// 확인창·질문에 답을 기다린다
    Question,
    Error,
    Exited,
}

/// agent adapter가 정규화한 harness 사건 하나(agent → host)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessEvent {
    /// aaa 세션(pane) ID
    pub session: String,
    /// claude·codex·report(`aaa report`)
    pub harness: String,
    /// session_start·turn_start·turn_end·question·idle·error·exited·progress·released(셸 앞 agent를 놓음)·
    /// compacting(대화 정리 중인 작업 중, plan §8.309: 옛 host는 모르는 kind로 버린다)
    pub kind: String,
    /// harness 자기 세션 ID(resume에 쓴다)
    pub harness_session: Option<String>,
    /// 사람이 읽는 한 줄(알림 문구, 질문 본문 요약)
    pub message: Option<String>,
    /// 질문 종류(permission·trust …)
    pub question_kind: Option<String>,
    pub percent: Option<u8>,
    pub at_ms: u64,
    /// Claude hook이 알린 대화 기록 파일(§8.27). host는 pane에만 기억하고 웹·로그에 내보내지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript: Option<String>,
    /// `kind = "plan"`일 때만: agent가 대화 기록에서 읽은 계획(None이면 지금 계획이 없다, 차이 목록 S5)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<PlanSummary>,
    /// `kind = "plan"`일 때만: 아직 도는 배경 작업(하위 agent·배경 명령) 수. 0이면 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<u32>,
    /// `kind = "plan"`일 때만: 대화 기록에서 읽은 컨텍스트 사용량(plan §8.321). 아직 응답이 없으면 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextUsage>,
    /// `kind = "plan"`: 지금 프로세스의 대화 기록에서 마지막으로 확인한 모델. 없으면 아직 모른다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_observation: Option<ModelObservation>,
    /// `turn_reply`: 기록에서 확인한 완료 답변. 상태·승인 여부를 바꾸지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_reply: Option<TurnReply>,
}

/// 같은 턴의 질문 카드 접수와 대조할 로컬 harness 기록의 시각과 답변.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnReply {
    pub started_ms: u64,
    pub finished_ms: u64,
    pub text: String,
}

/// 대화 기록의 모델 선택. 시작 profile의 effort를 관측한 모델에 섞지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ModelObservation {
    pub model: String,
    pub effort: Option<String>,
    #[ts(type = "number")]
    pub at_ms: u64,
}

/// harness 세션이 지금 쓴 컨텍스트(plan §8.321). agent가 대화 기록의 마지막 응답에서 읽는다(Claude `usage`, Codex `token_count`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContextUsage {
    /// 마지막 응답에 실린 컨텍스트 토큰(정리가 막 끝났으면 정리 뒤 크기)
    #[ts(type = "number")]
    pub used: u64,
    /// 창 크기. 기록으로 알 수 없으면 null(웹은 %가 아니라 토큰 수로 보인다)
    #[ts(type = "number | null")]
    pub window: Option<u64>,
    /// 사용량 %(그 harness 터미널과 같은 식: Claude 사용/창 반올림, Codex 기준 12k를 뺀 「left」의 나머지). 창을 모르거나
    /// 사용이 창보다 크면 null
    pub percent: Option<u8>,
    /// 이 기록의 마지막 정리(compaction) 전후 크기. 정리가 없었거나 뒤 크기를 아직 모르면 null
    pub compaction: Option<ContextCompaction>,
}

/// 정리 전후 컨텍스트 크기(Claude `compactMetadata.preTokens`·`postTokens`, Codex 정리 앞뒤 `token_count`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ContextCompaction {
    #[ts(type = "number")]
    pub before: u64,
    #[ts(type = "number")]
    pub after: u64,
}

/// pane 계획의 요약(herdr-web-ui `PlanSummary`): 끝난 단계 수·모든 단계 수·지금 하는 단계
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PlanSummary {
    pub done: u32,
    pub total: u32,
    pub current: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AgentStatus {
    pub activity: AgentActivity,
    #[ts(type = "number")]
    pub since_ms: u64,
    pub harness: String,
    pub message: Option<String>,
    /// `aaa report --percent`
    pub percent: Option<u8>,
    /// `aaa report --activity`
    pub progress: Option<String>,
    /// 마지막 `aaa report` 시각. 5분이 지나면 진행 보고로 「작업 중」을 정하지 않는다(herdr-projects `ACTIVITY_TTL_MS`).
    #[serde(default)]
    #[ts(type = "number | null")]
    pub progress_at_ms: Option<u64>,
    /// 지금 활동 글이 처음 보고된 때(FRONT_API 1.104). 같은 활동의 진행률·정기 보고는 시작을 바꾸지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub progress_since_ms: Option<u64>,
    pub harness_session: Option<String>,
    /// 마지막으로 쉼(`Idle`)이 된 뒤 사람이 이 pane을 봤다. false면 「끝남(안 봄)」이다(herdr `PaneState::seen`:
    /// 작업·질문에서 쉼으로 끝날 때 보는 화면이 없으면 false, 그 pane을 보면 true).
    #[serde(default = "seen_default")]
    pub seen: bool,
    /// harness가 기록에 남긴 계획(Claude TaskCreate·TaskUpdate, Codex update_plan). 없으면 null(차이 목록 S5)
    #[serde(default)]
    pub plan: Option<PlanSummary>,
    /// 아직 도는 배경 작업 수(Claude 하위 agent·배경 명령). 없으면 null
    #[serde(default)]
    pub background: Option<u32>,
    /// 쉬지만 그 턴이 띄운 배경 작업을 기다린다(herdr-web-ui `background_wait`, host `bgwait.rs`, 차이 목록 S1): 목록은 끝남·쉼
    /// 대신 BG다. 옛 host는 보내지 않는다(없으면 false)
    #[serde(default)]
    pub background_wait: bool,
    /// 작업 중이지만 harness가 대화를 정리(compaction)하는 중이다(화면 감지 사건 `compacting`, plan §8.309). 그동안 harness는 친 글과
    /// 대기 글을 읽지 않고, 웹은 보낸 글을 이 기기에 붙잡았다가 끝나면 보낸다. 옛 host는 보내지 않는다(없으면 false, 1.61)
    #[serde(default)]
    pub compacting: bool,
    /// 컨텍스트 사용량(plan §8.321, 1.67). 옛 host·첫 응답 전에는 null
    #[serde(default)]
    pub context: Option<ContextUsage>,
    /// 마지막으로 확인한 실행 모델(FRONT_API 1.109). 시작 profile과 별개이며 옛 agent는 보내지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model_observation: Option<ModelObservation>,
}

fn seen_default() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    Open,
    Answered,
    /// 답하기 전에 harness가 넘어갔다(다른 곳에서 답함, 세션 끝남)
    Gone,
}

/// 화면에서 읽은 선택 창(herdr-web-ui `InteractivePrompt`, plan §8.172). `id`는 내용(커서·체크 상태 빼고)의 hash라,
/// 답할 때 화면을 다시 읽어 다르면 거절한다(그 사이 바뀐 질문에 잘못 답하지 않는다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionMenu {
    pub id: String,
    /// question(AskUserQuestion)·approval(도구 승인)·plan(계획 승인)·menu(여러 질문의 검토)
    pub kind: String,
    pub title: String,
    pub question: String,
    /// 참고 글(명령·계획·검토할 답)
    pub body: Option<String>,
    pub options: Vec<MenuOption>,
    pub multi_select: bool,
    /// 「직접 쓰기」 자리. `options.len()`이면 그 줄은 `options`에 없다(Claude 질문의 「Type something.」)
    pub custom_option_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MenuOption {
    pub label: String,
    pub description: Option<String>,
}

/// 선택 창 답(참고 `PromptAnswer`): 하나 고르기·여러 개 고르기·직접 쓰기 가운데 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum MenuPick {
    Option { index: u32 },
    Options { indices: Vec<u32> },
    Custom { text: String },
}

/// 확인창 질문(§8.4). 답은 host가 세션·상태를 다시 확인한 뒤 adapter 키 입력으로 보낸다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionView {
    pub id: String,
    /// 프로젝트별 짧은 별칭. id는 기존 확인창 참조로 유지한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub short_id: Option<String>,
    pub pane: String,
    pub machine: String,
    pub project: Option<String>,
    pub thread: Option<String>,
    /// permission·trust(FrontResponse 태그 `kind`와 겹치지 않게 이름을 바꿨다)
    pub question_kind: String,
    pub text: String,
    /// 고를 수 있는 답(예: yes·no)
    pub options: Vec<String>,
    pub state: QuestionState,
    pub answer: Option<String>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    /// 화면에서 읽은 선택 창(못 읽었으면 None: 그때는 `options`의 예·아니오로 답한다)
    #[serde(default)]
    pub menu: Option<QuestionMenu>,
}

/// 앱 안 알림의 종류(plan §8.185). 휴대폰 push는 아니다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// host 내부 감시의 이상·회복. 웹 앱 안 알림.
    HostIssue,
    HostRecovered,
    /// 배포의 최종 성공/중단. 웹 앱 안 알림만 보낸다.
    RolloutFinished,
    RolloutFailed,
    /// 보는 화면 없이 agent의 턴이 끝났다(herdr `ToastKind::Finished`: 작업·질문에서 쉼으로, 안 봄)
    Finished,
    /// post 남용 한도에 걸린 출처가 음소거됐다. 앱 안에서만 알린다.
    PostMuted,
    /// 규칙이 명시한 앱 안 안내. 외부 push가 아니다.
    Automation,
    /// thread의 새 보고(herdr-projects 「review · new report」 알림, host 「new report」 항목과 같은 때)
    Report,
}

/// 앱 안 알림 하나(host가 front마다 보낸다, `FrontEvent::Notice`). 입력 필요는 따로 없고 `Question` 사건이 맡는다.
/// 같은 턴의 끝남과 새 보고는 새 보고 하나다(plan §8.185).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NoticeView {
    pub kind: NoticeKind,
    /// 갈 pane(새 보고는 그 thread의 pane, 없으면 null)
    pub pane: Option<String>,
    pub machine: String,
    pub project: Option<String>,
    pub thread: Option<String>,
    /// thread 제목 또는 pane 이름
    pub title: String,
    /// harness 이름(claude·codex, 끝남만)
    pub agent: Option<String>,
    /// 끝난 턴의 길이(작업·질문으로 들어선 때부터). host가 그 턴의 시작을 못 봤으면 null
    #[ts(type = "number | null")]
    pub turn_ms: Option<u64>,
    /// 긴 턴(herdr-web-ui `LONG_TURN_MS` 60초 이상이거나 길이를 모름): 웹 앱 안 알림은 이것만 띄운다
    pub long: bool,
    #[ts(type = "number")]
    pub at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageMode {
    /// 지금 보낸다(작업 중이면 Esc로 먼저 멈춘다)
    Interrupt,
    /// 바로 입력한다. 작업 중이면 harness가 다음 턴으로 줄 세운다(herdr-projects `thread prompt`). 이름은 옛 판 그대로다
    #[default]
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    /// 기록했고 아직 보내는 중이다
    Queued,
    /// harness가 입력을 받았다(turn_start)
    Accepted,
    /// 받았는지 모른다. 자동으로 다시 보내지 않는다(§8.5 a)
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageView {
    pub id: String,
    pub project: String,
    pub thread: String,
    pub mode: MessageMode,
    pub state: DeliveryState,
    pub text: String,
    #[ts(type = "number")]
    pub created_at_ms: u64,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    /// 자리 만들기·agent 시작 중
    Starting,
    Open,
    Failed,
    Resolved,
    /// 보류(FRONT_API 1.105): 자리·pane을 돌려주고 worktree·대화 ID·보고를 남긴다. `thread unpark`로 이어 한다
    Parked,
}

impl ThreadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThreadStatus::Starting => "starting",
            ThreadStatus::Open => "open",
            ThreadStatus::Failed => "failed",
            ThreadStatus::Resolved => "resolved",
            ThreadStatus::Parked => "parked",
        }
    }

    pub fn parse(s: &str) -> Option<ThreadStatus> {
        match s {
            "starting" => Some(ThreadStatus::Starting),
            "open" => Some(ThreadStatus::Open),
            "failed" => Some(ThreadStatus::Failed),
            "resolved" => Some(ThreadStatus::Resolved),
            "parked" => Some(ThreadStatus::Parked),
            _ => None,
        }
    }
}

/// brief 전달 상태. `unknown`이면 자동으로 다시 보내지 않는다(§8.5 a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BriefState {
    Pending,
    Delivered,
    Unknown,
}

impl BriefState {
    pub fn as_str(&self) -> &'static str {
        match self {
            BriefState::Pending => "pending",
            BriefState::Delivered => "delivered",
            BriefState::Unknown => "unknown",
        }
    }

    pub fn parse(s: &str) -> Option<BriefState> {
        match s {
            "pending" => Some(BriefState::Pending),
            "delivered" => Some(BriefState::Delivered),
            "unknown" => Some(BriefState::Unknown),
            _ => None,
        }
    }
}

/// 목록에서 보이는 묶음(coordinator가 무엇을 해야 하나)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadGroup {
    /// 사람이나 coordinator가 할 일이 있다(시작 실패·5분 넘은 시작, 닫힌 pane, brief 전달 불명, 30초 넘은 확인창,
    /// `Waiting for you` 진행 보고). 보고 내용으로는 정하지 않는다(herdr-projects `group`, plan §8.78).
    WaitingOnYou,
    /// 아직 확인(ack)하지 않은 새 보고. 옛 PR 병합 묶음은 검토로 읽는다.
    #[serde(alias = "landing")]
    ReadyForReview,
    Working,
    /// 열려 있지만 harness가 쉬고 있고 새 보고도 진행 보고도 없다(herdr-projects `Group::Idle`)
    Idle,
    /// 보류(FRONT_API 1.105): 자리·pane 없이 이어 하기를 기다린다
    Parked,
    Resolved,
}

impl ThreadGroup {
    /// 사람이 읽는 이름(CLI·웹과 같은 말)
    pub fn label(self) -> &'static str {
        match self {
            ThreadGroup::WaitingOnYou => "확인 필요",
            ThreadGroup::ReadyForReview => "검토 대기",
            ThreadGroup::Working => "작업 중",
            ThreadGroup::Idle => "쉼",
            ThreadGroup::Parked => "보류",
            ThreadGroup::Resolved => "끝남",
        }
    }

    /// 보이는 차례(herdr-projects `Group::rank`·`DISPLAY_ORDER`): 확인 필요 1 → 검토 2 → 작업 4 → 쉼 5 → 보류 6 → 끝남 7.
    /// 목록(웹)·popup·overview가 같은 차례다. 묶음을 정하는 우선순위(host `group`)와는 따로다.
    pub fn rank(self) -> u8 {
        match self {
            ThreadGroup::WaitingOnYou => 1,
            ThreadGroup::ReadyForReview => 2,
            ThreadGroup::Working => 4,
            ThreadGroup::Idle => 5,
            ThreadGroup::Parked => 6,
            ThreadGroup::Resolved => 7,
        }
    }
}

/// 회수한 보고 첫 줄의 작업 단계. thread 묶음·알림 의도와 별개다(FRONT_API 1.94).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStage {
    MergeReady,
    Landing,
}

impl ThreadStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::MergeReady => "병합 대기",
            Self::Landing => "병합 중",
        }
    }
}

/// 보류 기록(FRONT_API 1.105, plan §8.426). `parked_at_ms`가 없으면 thread에게 정리를 맡기고 `[보류 준비됨]` 보고를 기다린다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadPark {
    pub actor: String,
    #[ts(type = "number")]
    pub requested_at_ms: u64,
    /// 사람이 남긴 까닭(이어 할 때 brief에 들어간다)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note: Option<String>,
    /// coordinator가 인용한 사용자 지시
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub user_request: Option<String>,
    /// 정리를 맡기지 않고 바로 보류했다
    #[serde(default)]
    pub immediate: bool,
    /// 정리를 맡긴 때의 보고 hash. 그 뒤 새 `[보류 준비됨]` 보고만 정리 끝으로 읽는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_before: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub parked_at_ms: Option<u64>,
    /// 보류한 머신 이름(보일 때 쓴다)
    #[serde(default)]
    pub machine: String,
    /// 보류할 때 본 git: HEAD commit, origin의 같은 branch, 커밋 안 한 파일 수. 모르면 `git_unknown`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub head: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pushed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub uncommitted: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub git_unknown: Option<String>,
    /// 같은 머신에서 같은 대화를 이을 수 있다(대화 ID와 resume 방법을 안다)
    #[serde(default)]
    pub resumable: bool,
}

/// `thread unpark --dry-run`의 머신별 길(FRONT_API 1.105). 같은 머신은 같은 대화, 다른 머신은 push된 branch로 새 worktree와
/// 요약 brief의 새 대화다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UnparkOption {
    pub machine: String,
    pub same: bool,
    pub connected: bool,
    /// `resume`(같은 대화)·`new`(새 대화와 요약 brief)
    pub conversation: String,
    /// 다른 머신의 저장소 경로(작업 폴더 thread는 없다)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repo: Option<String>,
    /// 고를 수 없으면 까닭
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UnparkPreview {
    pub project: String,
    pub thread: String,
    pub options: Vec<UnparkOption>,
    /// 지금 자리를 차지한 사용자 thread 수와 한도. 다 찼으면 이어 하기가 거부된다
    pub open: u32,
    pub limit: u32,
}

/// 사용자가 직접 허용한 한도 초과 시작. 시작 시각·요청자·coordinator가 인용한 지시를 보존한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadLimitOverride {
    pub actor: String,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub user_request: Option<String>,
}

/// 검증한 보고 사본 첫 줄의 알림 의도. 묶음·진행률과 별개다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReportAttention {
    Completed,
    NeedsUser,
}

/// 목록은 100건(최대 200건)씩 읽는다. after는 응답의 next_after를 그대로 돌려준다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadListQuery {
    /// 끝난 thread를 뺀다(준비·실패·보류는 포함).
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub open: bool,
    #[serde(default)]
    #[ts(optional)]
    pub status: Option<ThreadStatus>,
    #[serde(default)]
    #[ts(optional)]
    pub limit: Option<u32>,
    #[serde(default)]
    #[ts(optional)]
    pub after: Option<String>,
    /// 웹의 질문 상세가 필요할 때만 켠다. 기본 목록도 CLI가 표시하는 필드는 보존한다.
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub details: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadView {
    /// 연결된 pane의 실제 책임 경로. 옛 agent·비 macOS·pane 없는 thread는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub responsibility: Option<crate::Responsibility>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account_selection: Option<crate::AccountSelection>,
    /// 명시적으로 실행 한도를 무시한 시작의 출처. 옛 기록은 생략한다(FRONT_API 1.98).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub over_limit: Option<ThreadLimitOverride>,
    /// 보류 요청·보류 기록(FRONT_API 1.105). 보류하지 않은 thread와 옛 host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub park: Option<Box<ThreadPark>>,
    /// 이어진 할 일의 자동 병합(FRONT_API 1.143). off·할 일 없는 thread·옛 host는 생략한다
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<bool>")]
    pub auto_merge: bool,
    /// FRONT_API 1.95. 보고의 특정 판을 여는 프로젝트별 영속 번호(r-0001부터).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,

    pub project: String,
    pub id: String,
    pub title: String,
    pub machine: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub account: Option<String>,
    pub role: Role,
    /// (FrontResponse 태그 `kind`와 겹치지 않게 이름을 달리한다)
    pub thread_kind: ThreadKind,
    /// 명시한 실행 대기 표지. timing이면 그 머신의 측정이 끝날 때까지 자동 시작이 기다릴 수 있다.
    #[serde(default)]
    pub holds: Vec<String>,
    /// 이 thread를 띄운 자동화 규칙 이름(`do.start`, FRONT_API 1.64, plan t-0475 A1). 사람이 띄운 thread와 옛 host는 생략한다.
    /// 웹 「작업」 목록이 줄 왼쪽 호박색 막대와 `자동화 thread` 표지로 보인다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub automation: Option<String>,
    /// 그 규칙의 제목(FRONT_API 1.64). 규칙 파일이 없어졌거나 제목이 비면 이름이다. `automation`이 없으면 없다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub automation_title: Option<String>,
    /// 최초 기동 당시 사건·일정의 설명(FRONT_API 1.91). 옛 thread·옛 host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub automation_trigger: Option<String>,
    pub status: ThreadStatus,
    /// 끝남 기록 뒤 아직 정리 응답을 확정하지 못했다. 같은 resolve로 이어간다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cleanup_pending: Option<bool>,
    pub group: ThreadGroup,
    pub brief: BriefState,
    pub pane: Option<String>,
    pub cwd: Option<String>,
    pub work_dir: Option<String>,
    pub branch: Option<String>,
    pub report_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_attention: Option<ReportAttention>,
    /// 회수한 보고의 작업 단계. 옛 host·표지 없는 보고·끝난 thread는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stage: Option<ThreadStage>,
    pub acked_hash: Option<String>,
    /// report.md `## Next` 줄
    pub next: Vec<String>,
    /// 완료 보고에서 남긴 후속과 명시적 처리 기록(FRONT_API 1.131).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<ThreadFollowup>>")]
    pub followups: Vec<ThreadFollowup>,
    /// 마지막 사본 결과: complete·partial·failed
    pub copy: Option<String>,
    pub error: Option<String>,
    /// agent harness 상태(hook으로 안다)
    pub agent: Option<Box<AgentStatus>>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number")]
    pub updated_at_ms: u64,
    /// `thread start`가 알리는 경고(시작은 했다, 예: 동시 thread 한도). 다른 응답에는 없다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub warnings: Vec<String>,
    /// 그 프로젝트의 상태(FRONT_API 1.33, 진행 중이면 없다). 상태를 바꾸면 그 프로젝트 thread 사건을 다시 내서 front가
    /// 다시 묻지 않고 멈춤·보관을 안다(웹 sidebar의 「멈춤」, 보관한 프로젝트 숨김)
    #[serde(default, skip_serializing_if = "ProjectStatus::is_active")]
    #[ts(as = "Option<ProjectStatus>", optional)]
    pub project_status: ProjectStatus,
    /// 작업 요약 표지(FRONT_API 1.44, plan §8.259): 요약을 쓴 때와 그 뒤 반영 안 된 지시 수. 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub summary: Option<SummaryMark>,
    /// thread가 일하는 저장소의 그 머신 경로(FRONT_API 1.44): worktree thread는 시작할 때 고른 repo, 데려온 thread는 그 폴더의
    /// git top. 작업 폴더 thread와 옛 기록은 없다. 웹 「작업」 트리가 마지막 폴더 이름으로 묶는다(머신마다 경로가 달라도 한 폴더)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub repo: Option<String>,
    /// 사용량 한도(FRONT_API 1.44, plan §8.267): 이 thread harness의 구독 창이 다 찼으면 그 창이 다시 차오르는 때(ms).
    /// host 머신의 thread이고 그 구독에 계정이 하나일 때만 안다. 다 차지 않았거나 모르면 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub limited_until_ms: Option<u64>,
    /// 보고 `## Questions`의 질문과 답(FRONT_API 1.72, plan §8.329): 웹 보고서 패널 「답할 것」 카드와 대화창 칩이 쓴다.
    /// 질문이 없는 보고와 옛 host는 생략한다
    // 사건(`FrontEvent::Thread`) 크기를 늘리지 않게 Box에 둔다(clippy large_enum_variant)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub questions: Option<Box<ThreadQuestions>>,
}

/// thread가 구현 전에 모아 묻는 질문 묶음(plan §8.329). 그 보고에 묶인다: 새 보고가 오면 새 질문이고 답도 비운다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadQuestions {
    /// 형식 확인 필요: 질문·선택지 누락이나 글자 순서 불일치를 알린다. 답 전송은 허용한다. 옛 host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub format_warning: Option<String>,
    /// FRONT_API 1.96. Questions 없는 검수 대기에 host가 만든 기본 검수 카드. B는 고칠 점을 글로 적는다.
    #[serde(default)]
    pub review: bool,
    /// 이 질문이 든 보고 사본의 hash. 답은 이 hash가 지금 보고일 때만 받는다
    pub report_hash: String,
    pub items: Vec<ThreadQuestion>,
    /// 답했으면 그 답(웹 카드·`aaa thread answer`). 모든 기기가 같은 값을 본다
    pub answer: Option<QuestionAnswerView>,
    /// 본문 그림의 주소 앞부분(FRONT_API 1.78, plan §8.345): 본문의 `library/<경로>`는 `<files_base><경로>`로 읽는다.
    /// 그 보고의 host 사본에서만 주고, 새 보고가 오면 옛 주소는 열리지 않는다. 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub files_base: Option<String>,
    /// 짧은 질문 ID로 일부만 답한 경우의 답. 전체 답은 answer에 남는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub draft: Option<QuestionAnswerView>,
}

/// 질문 하나: 글과 선택지(앞에서부터 A·B·C…)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadQuestion {
    /// 답한 질문을 제외한 카드도 원문의 질문 번호로 답한다(FRONT_API 1.132).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_number: Option<u32>,
    /// 선택지 뒤의 Markdown 설명·그림(FRONT_API 1.99). 선택지 그림과 섞거나 버리지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after: Option<String>,
    /// 프로젝트별 영속 순번(q-1000부터). 옛 보고 파서·host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub text: String,
    pub choices: Vec<QuestionChoice>,
    /// `### N. 질문` 머리 아래의 Markdown 본문(FRONT_API 1.78, plan §8.345): 그림·표·code·링크. 옛 꼴과 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionChoice {
    /// 보고에 쓴 선택지 글자(FRONT_API 1.132). 옛 자료는 배열 위치의 A부터 읽는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub letter: Option<String>,
    pub label: String,
    /// 「 — 」 뒤의 까닭 한 줄
    pub description: Option<String>,
    /// 「(추천)」 표지가 있었다
    pub recommended: bool,
    /// 선택지 줄 아래 두 칸 들여쓴 Markdown(시안 그림 등, FRONT_API 1.78). 없으면 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
}

impl QuestionChoice {
    pub fn letter_at(&self, index: usize) -> String {
        self.letter.clone().unwrap_or_else(|| char::from(b'A' + index as u8).to_string())
    }
}

impl ThreadQuestion {
    /// 표시 글자를 배열 위치로 바꾼다. picks의 기존 위치 계약은 유지한다.
    pub fn choice_index(&self, letter: &str) -> Option<u32> {
        self.choices
            .iter()
            .enumerate()
            .find_map(|(i, c)| c.letter_at(i).eq_ignore_ascii_case(letter).then_some(i as u32))
    }
}

/// 받은 답. `summary`는 「1A 2B」 꼴(직접 쓴 답이 있으면 「직접」)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionAnswerView {
    /// 질문마다 고른 선택지 번호(0부터). 고르지 않고 직접 쓴 질문은 null
    pub picks: Vec<Option<u32>>,
    /// 직접 쓴 답(질문 번호를 붙여 쓴다, 예: 「2 둘 다 보이게」)
    pub text: Option<String>,
    /// 질문 배열 순서의 직접 답·의견. 빈 배열은 옛 공통 text만 쓰는 답이다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Option<String>>>")]
    pub comments: Vec<Option<String>>,
    pub summary: String,
    /// 답한 곳(웹 기기 이름·`CLI`)
    pub by: String,
    #[ts(type = "number")]
    pub at_ms: u64,
}

/// 「답할 것」 기록의 종류(plan §8.345): thread 질문, coordinator 확인 필요 post, 할 일 확인 필요
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    Question,
    Request,
    Task,
}

/// 답한 것 하나(plan §8.345, FRONT_API 1.78). 답할 때의 원문을 보존한다: 새 보고가 질문을 비워도 남고, host가 14일 뒤 지운다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AskRecord {
    /// FRONT_API 1.96. 답할 때의 보고 판. 새 보고 뒤에도 같은 원문을 연다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub question_id: Option<String>,
    pub kind: AskKind,
    /// 질문은 thread 번호, 확인 필요 post는 inbox 항목 ID, 할 일은 task 제목
    pub subject: String,
    /// thread 제목·post 제목·task 제목
    pub title: String,
    /// 물은 때(보고를 받은 때·post를 받은 때). 모르면 답한 때
    #[ts(type = "number")]
    pub asked_ms: u64,
    #[ts(type = "number")]
    pub done_ms: u64,
    /// 답한 곳(웹 기기 이름·`CLI`·`coordinator`)
    pub by: String,
    /// 「1A 2B」·「답함으로 표시」·「끝남으로 지움」
    pub summary: String,
    /// 질문 원문(본문·선택지 본문 포함)과 답
    #[serde(default)]
    pub items: Vec<ThreadQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub answer: Option<QuestionAnswerView>,
    /// 확인 필요 post 본문·할 일 확인 필요 까닭(Markdown 글)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
    /// 보존한 파일의 주소 앞부분: `<files_base>library/<경로>`가 그림, `<files_base>report.md`가 그때 보고다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub files_base: Option<String>,
}

/// 열린 coordinator 확인 필요 post(`post user --request --needs-user`, plan §8.345). inbox 항목이 끝나지 않은 동안 「답할 것」이다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UserRequestView {
    /// 평문 요청도 포함한 짧은 질문 ID(여러 질문이면 첫 ID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub question_id: Option<String>,
    /// inbox 항목 ID(「답함으로 표시」가 끝낸다)
    pub id: String,
    pub title: String,
    /// post 본문(Markdown 글). 본문이 지워졌으면 inbox 요약
    pub body: String,
    #[ts(type = "number")]
    pub created_ms: u64,
    /// post 본문의 `## Questions`(FRONT_API 1.87). 없으면 기존 평문 카드다. report_hash는 본문 hash다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub questions: Option<ThreadQuestions>,
}

/// 질문당 선택지·질문 수 상한(글자 A–I, 숫자 1–9)
pub const QUESTION_MAX: usize = 9;
/// 질문 본문 한도(바이트). thread 사건은 모든 기기에 가므로 본문을 줄인다. 넘으면 잘랐다고 꼬리에 적는다
pub const QUESTION_BODY_MAX: usize = 16 * 1024;
/// 선택지 본문 한도(바이트)
pub const CHOICE_BODY_MAX: usize = 4 * 1024;

/// code fence 줄(``` 또는 ~~~로 시작)
fn fence_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// 질문·선택지 맨 앞의 Markdown 강조를 푼다. 본문 속 별표와 밑줄은 보존한다.
fn unmark_leading(text: &str) -> String {
    let mut out = text.trim().to_string();
    while let Some(mark) = ["**", "__", "*", "_"].into_iter().find(|m| out.starts_with(m)) {
        let Some(end) = out[mark.len()..].find(mark).map(|i| i + mark.len()) else { break };
        out.replace_range(end..end + mark.len(), "");
        out.replace_range(..mark.len(), "");
    }
    out
}

/// A~Z 뒤에 글자·숫자가 이어지는 단어(A를·A안·t3code)는 후보가 아니다.
/// 후보는 엄격한 선택지 파싱과 별도로 센다. 잘못된 구두점·강조 때문에 버린 줄도 경고한다.
fn choice_letter(text: &str) -> Option<char> {
    let item = text.trim().strip_prefix("- ").or_else(|| text.trim().strip_prefix("* "))?;
    let item = item.trim_start_matches(['*', '_']);
    let mut chars = item.chars();
    let letter = chars.next()?;
    (letter.is_ascii_alphabetic() && chars.next().is_none_or(|c| !c.is_alphanumeric()))
        .then_some(letter.to_ascii_uppercase())
}

/// `- A 선택지 (추천) — 까닭`, `- **A(추천)** …` 등. 앞 공백은 부르는 쪽이 정한다.
fn choice_line(text: &str) -> Option<QuestionChoice> {
    let item = unmark_leading(text.strip_prefix("- ").or_else(|| text.strip_prefix("* "))?);
    let mut chars = item.chars();
    let (Some(letter), Some(sep)) = (chars.next(), chars.next()) else { return None };
    if !letter.is_ascii_alphabetic() || !matches!(sep, ' ' | ')' | '.' | '(') {
        return None;
    }
    let rest = &item[letter.len_utf8()..];
    if sep == '(' && !["(추천)", "(Recommended)", "(recommended)"].iter().any(|m| rest.starts_with(m)) {
        return None;
    }
    let rest = rest.trim_start_matches([' ', ')', '.']).trim();
    let (label, description) = match rest.split_once(" — ") {
        Some((l, d)) => (l.trim(), Some(d.trim().to_string()).filter(|d| !d.is_empty())),
        None => (rest, None),
    };
    let mut recommended = false;
    let mut label = label.to_string();
    for mark in ["(추천)", "(Recommended)", "(recommended)"] {
        if label.contains(mark) {
            recommended = true;
            label = label.replace(mark, "");
        }
    }
    let label = unmark_leading(&label).split_whitespace().collect::<Vec<_>>().join(" ");
    (!label.is_empty()).then_some(QuestionChoice {
        letter: Some(letter.to_ascii_uppercase().to_string()),
        label,
        description,
        recommended,
        body: None,
    })
}

/// 앞뒤 빈 줄을 뗀 본문(없으면 None). `max`를 넘으면 글자 경계에서 자르고, 열린 code fence를 닫은 뒤 잘랐다고 적는다
fn body_of(lines: &[String], max: usize) -> Option<String> {
    let text = lines.join("\n");
    let text = text.trim_matches('\n').trim_end();
    if text.trim().is_empty() {
        return None;
    }
    if text.len() <= max {
        return Some(text.to_string());
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = text[..cut].trim_end().to_string();
    if out.lines().filter(|l| fence_line(l)).count() % 2 == 1 {
        out.push_str("\n```");
    }
    out.push_str("\n\n*(본문이 길어 잘렸다. 나머지는 보고서에서 본다)*");
    Some(out)
}

/// 본문·선택지 목록·그 뒤의 설명을 분리한다. 선택지 그림은 들여쓴 줄로 그 선택지에 남기고,
/// 목록 뒤의 열 0 설명·그림은 after로 보존한다. code fence 안 줄은 선택지가 아니다.
fn rich_question(text: String, block: &[String]) -> ThreadQuestion {
    let mut fenced = vec![false; block.len()];
    let mut inside = false;
    for (i, l) in block.iter().enumerate() {
        if fence_line(l) {
            fenced[i] = true;
            inside = !inside;
        } else {
            fenced[i] = inside;
        }
    }
    let is_choice = |i: usize| !fenced[i] && choice_line(&block[i]).is_some();
    let mut runs = Vec::new();
    let mut active: Option<(usize, usize, usize)> = None;
    for (i, line) in block.iter().enumerate() {
        if is_choice(i) {
            let run = active.get_or_insert((i, i, 0));
            run.1 = i + 1;
            run.2 += 1;
        } else if line.trim().is_empty() || line.starts_with(' ') || line.starts_with('\t') {
            if let Some(run) = active.as_mut() {
                run.1 = i + 1;
            }
        } else if let Some(run) = active.take() {
            runs.push(run);
        }
    }
    if let Some(run) = active {
        runs.push(run);
    }
    // 본문의 보통 글머리 하나보다 끝의 A/B 목록을 우선한다. 여러 목록이면 진단의 후보 수 대조가 경고한다.
    let (start, end, _) =
        runs.iter().rev().find(|r| r.2 >= 2).or(runs.last()).copied().unwrap_or((block.len(), block.len(), 0));
    let mut choices: Vec<QuestionChoice> = Vec::new();
    let mut extra: Vec<Vec<String>> = Vec::new();
    for (i, l) in block.iter().enumerate().take(end).skip(start) {
        if is_choice(i) {
            choices.push(choice_line(l).expect("선택지 줄"));
            extra.push(Vec::new());
        } else if let Some(lines) = extra.last_mut() {
            let indent = l.len() - l.trim_start_matches(' ').len();
            lines.push(l[indent.min(2)..].to_string());
        }
    }
    for (c, lines) in choices.iter_mut().zip(&extra) {
        c.body = body_of(lines, CHOICE_BODY_MAX);
    }
    choices.truncate(QUESTION_MAX);
    ThreadQuestion {
        source_number: None,
        id: None,
        text,
        choices,
        body: body_of(&block[..start], QUESTION_BODY_MAX),
        after: body_of(&block[end..], QUESTION_BODY_MAX),
    }
}

/// 보고를 읽은 결과와 누락·글자 불일치 진단. 경고는 진단용이며 읽은 선택지·직접 답 전송을 막지 않는다.
#[derive(Debug, Default)]
pub struct ParsedReportQuestions {
    pub items: Vec<ThreadQuestion>,
    pub warning: Option<String>,
}

/// `### N. 질문`과 `N. **질문**` 모두 본문과 끝의 선택지 목록을 읽는다.
/// 후보 줄은 별도로 세어 누락·상한 초과·A부터 빠짐없이 유일하지 않은 글자를 경고한다.
pub fn parse_report_questions(report: &str) -> ParsedReportQuestions {
    let mut out = ParsedReportQuestions::default();
    let mut inside = false;
    let mut fenced = false;
    let mut current: Option<(String, Vec<String>, bool, usize)> = None;
    let mut question_count = 0;
    let mut source_choices = 0;
    let mut card_choices = 0;
    let mut bad_order = false;
    let finish = |current: &mut Option<(String, Vec<String>, bool, usize)>,
                  out: &mut ParsedReportQuestions,
                  source_choices: &mut usize,
                  card_choices: &mut usize,
                  bad_order: &mut bool| {
        let Some((text, block, rich, _)) = current.take() else { return };
        let mut code = false;
        let letters: Vec<_> = block
            .iter()
            .filter_map(|l| {
                if fence_line(l) {
                    code = !code;
                    return None;
                }
                (!code).then(|| choice_letter(l)).flatten()
            })
            .collect();
        *source_choices += letters.len();
        // 추천을 앞에 쓴 B/A/C도 글자가 빠짐없이 유일하면 뜻이 명확하다.
        let mut ordered = letters.clone();
        ordered.sort_unstable();
        *bad_order |= ordered.iter().enumerate().any(|(i, c)| *c as usize != 'A' as usize + i);
        // 옛 번호 목록은 들여쓴 선택지도 지원한다.
        let block = if rich {
            block
        } else {
            block
                .into_iter()
                .map(|l| if choice_letter(&l).is_some() { l.trim_start().to_string() } else { l })
                .collect()
        };
        let mut q = rich_question(unmark_leading(&text), &block);
        let mut seen_choice = false;
        let mut code = false;
        for line in &block {
            if fence_line(line) {
                code = !code;
                continue;
            }
            if code {
                continue;
            }
            if choice_line(line).is_some() {
                seen_choice = true;
            } else if seen_choice && (line.starts_with("- ") || line.starts_with("* ")) {
                *bad_order = true;
            }
        }
        // 같은 글자 둘은 답 대상을 정할 수 없다. 직접 답 카드에 원문과 진단을 남긴다.
        if q.choices.iter().enumerate().any(|(i, c)| q.choices[..i].iter().any(|p| p.letter == c.letter)) {
            q.choices.clear();
            q.body = body_of(&block, QUESTION_BODY_MAX);
            q.after = None;
        }
        *bad_order |= q.choices.len() < 2;
        if out.items.len() < QUESTION_MAX {
            *card_choices += q.choices.len();
            out.items.push(q);
        }
    };
    for line in report.lines() {
        if !fenced && let Some(head) = line.strip_prefix("## ") {
            finish(&mut current, &mut out, &mut source_choices, &mut card_choices, &mut bad_order);
            inside = head.trim() == "Questions";
            continue;
        }
        if !inside {
            continue;
        }
        if !fenced {
            let head = line.strip_prefix("### ").unwrap_or(line).trim();
            if let Some((n, rest)) = head.split_once(". ")
                && !n.is_empty()
                && n.chars().all(|c| c.is_ascii_digit())
                && !rest.trim().is_empty()
                // ### 본문의 번호 목록은 본문으로 둔다. 선택지가 끝난 뒤의 N.은 다음 질문이다.
                && (line.starts_with("### ") || current.as_ref().is_none_or(|(_, _, rich, tail_choices)| !rich || *tail_choices >= 2))
            {
                finish(&mut current, &mut out, &mut source_choices, &mut card_choices, &mut bad_order);
                question_count += 1;
                bad_order |= n.parse::<usize>().ok() != Some(question_count);
                current = Some((rest.trim().to_string(), Vec::new(), line.starts_with("### "), 0));
                continue;
            }
            if line.starts_with("# ") {
                finish(&mut current, &mut out, &mut source_choices, &mut card_choices, &mut bad_order);
                inside = false;
                continue;
            }
        }
        if fence_line(line) {
            fenced = !fenced;
        }
        if let Some((_, block, _, tail_choices)) = current.as_mut() {
            if !fenced && choice_line(line).is_some() {
                *tail_choices += 1;
            } else if fence_line(line) || !(line.trim().is_empty() || line.starts_with(' ') || line.starts_with('\t')) {
                *tail_choices = 0;
            }
            block.push(line.to_string());
        } else if !fenced && choice_letter(line).is_some() {
            source_choices += 1;
        }
    }
    finish(&mut current, &mut out, &mut source_choices, &mut card_choices, &mut bad_order);
    // 첫 줄이 선언한 수 역시 검사한다. 일반 보고 글의 숫자는 질문 수로 읽지 않는다.
    let declared = report
        .lines()
        .next()
        .and_then(|l| l.split_once("질문 "))
        .and_then(|(_, r)| r.split_once("개"))
        .and_then(|(n, _)| n.parse::<usize>().ok());
    if question_count != out.items.len()
        || source_choices != card_choices
        || bad_order
        || declared.is_some_and(|n| n != out.items.len())
    {
        out.warning = Some(format!(
            "형식 확인 필요: 질문 {question_count}개 중 {}개, 선택지 {source_choices}개 중 {card_choices}개를 읽었다. 질문 번호·A부터 이어지는 선택지 글자·질문당 선택지 2~9개를 확인한다{}.",
            out.items.len(),
            declared.map(|n| format!("(선언한 질문 {n}개)")).unwrap_or_default()
        ));
    }
    if out.warning.is_some() && out.items.is_empty() {
        out.items.push(ThreadQuestion {
            source_number: None,
            id: None,
            text: "직접 답".into(),
            choices: Vec::new(),
            body: body_of(&report.lines().map(str::to_string).collect::<Vec<_>>(), QUESTION_BODY_MAX),
            after: None,
        });
    }
    out
}

/// 진단이 필요 없는 기존 호출용. 카드·답 전송 경계는 `parse_report_questions`의 warning을 함께 확인한다.
pub fn report_questions(report: &str) -> Vec<ThreadQuestion> {
    parse_report_questions(report).items
}

/// thread 작업 요약 표지(plan §8.259). 줄·머리 단추가 요약을 읽지 않고 「낡았다」를 안다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SummaryMark {
    /// 요약을 썼거나 정상 진행 보고를 받은 마지막 때. 둘 다 없으면 None
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
    /// 요약·정상 진행 보고에 아직 반영 안 된 task.md Follow-ups 지시 수
    pub pending: u32,
}

/// 작업 요약의 한 줄: 앞의 「누가 언제」(`사용자 15:0x`)와 줄 긋기(`~~…~~`, 뒤에 뒤집혀 무효)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SummaryEntry {
    /// 사용자·coordinator·agent
    pub who: Option<String>,
    /// 적힌 그대로의 시각(`15:0x`, `12:2x~14:5x`)
    pub at: Option<String>,
    pub text: String,
    pub dropped: bool,
}

/// task.md Follow-ups 하나(첫 줄만)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FollowUpLine {
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
    pub text: String,
}

/// 큰 작업 하나의 진행 상태. 완료 개수는 소요 시간 비율이 아니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SummaryStepState {
    Pending,
    Working,
    Done,
}

/// 요약의 큰 작업. 작성한 순서를 유지하며 완료한 항목도 남긴다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SummaryStep {
    pub text: String,
    pub state: SummaryStepState,
}

/// thread 작업 요약(`threads/<id>.summary.md`, plan §8.259). coordinator가 쓴 요약과 aaa가 붙인 것(agent 보고의
/// `## Decisions`, 요약 뒤 지시, 요약이 없을 때 할 일 첫 줄)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadSummaryView {
    pub project: String,
    pub thread: String,
    /// 요약 파일 글(없으면 None)
    pub text: Option<String>,
    pub goal: Option<String>,
    /// 목표가 바뀌었을 때 처음 시킨 것
    pub origin: Option<String>,
    /// 전체 큰 작업(최대 5개, FRONT_API 1.86). 옛 요약·옛 host는 목록이 없다.
    #[serde(default)]
    #[ts(as = "Option<Vec<SummaryStep>>", optional)]
    pub progress: Vec<SummaryStep>,
    pub now: Vec<String>,
    pub decided: Vec<SummaryEntry>,
    pub changed: Vec<SummaryEntry>,
    pub done: Vec<String>,
    /// thread 보고 `## Decisions` 줄(agent가 정한 것)
    pub decisions: Vec<String>,
    #[ts(type = "number | null")]
    pub at_ms: Option<u64>,
    /// 마지막 갱신 쪽(coordinator·사람·agent). 진행은 정상 thread 보고를 우선한다.
    pub by: Option<String>,
    /// Follow-ups 전체 수
    pub follow_ups: u32,
    /// 요약 뒤 지시 수와 그 마지막 몇 개(요약이 없으면 Follow-ups 마지막 몇 개)
    pub pending: u32,
    pub pending_lines: Vec<FollowUpLine>,
    /// 요약이 없을 때 보이는 할 일 첫 줄
    pub task: Option<String>,
}

/// 흐름 사건(host가 낸다, §8.3). 프로젝트별 순번과 host 시각·boot ID를 가진다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FlowEvent {
    pub project: String,
    #[ts(type = "number")]
    pub seq: u64,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub boot_id: String,
    /// thread_started·brief_delivered·brief_unknown·report_ready·review_acked·thread_resolved·thread_failed …
    pub kind: String,
    pub thread: Option<String>,
    /// thread 없는 감시 사건의 저장소. 옛 기록은 없음이다.
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub actor: Option<String>,
    pub role: Option<Role>,
    pub profile: Option<String>,
    pub machine: Option<String>,
    /// 근거 경로(host 프로젝트 폴더 기준, 예: `threads/t-0001.md`)
    pub evidence: Option<String>,
    /// 사람이 읽는 한 줄
    pub detail: String,
}

/// 완료 보고의 후속 한 줄. 원래 판과 처리 기록은 다음 보고로 지우지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadFollowup {
    pub id: String,
    pub revision: String,
    pub line: String,
    /// 같은 프로젝트의 후속 할 일 표시 ID. 버림일 때는 없다.
    pub task: Option<String>,
    /// 버림·정리 완료의 명시적 사유.
    pub reason: Option<String>,
    pub actor: Option<String>,
    #[ts(type = "number | null")]
    pub handled_ms: Option<u64>,
}

/// report.md `## Next` 절의 줄: 한 줄에 권하는 행동 하나, 목록 표시는 빼고 빈 줄은 버린다
/// (herdr-projects `thread::next_lines`를 옮겼다). 첫 줄 상태 태그는 읽지 않는다(plan §8.78).
pub fn next_lines(report: &str) -> Vec<String> {
    section_lines(report, "Next")
}

/// report.md `## <name>` 절의 목록 줄(`next_lines`와 같은 규칙). `## Decisions`도 이것으로 읽는다(plan §8.259)
pub fn section_lines(report: &str, name: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut inside = false;
    for line in report.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            inside = head.trim() == name;
            continue;
        }
        if !inside || line.starts_with('#') {
            if line.starts_with("# ") {
                inside = false;
            }
            continue;
        }
        let text = line.trim();
        let text = text
            .strip_prefix("- ")
            .or_else(|| text.strip_prefix("* "))
            .or_else(|| {
                text.split_once(". ").filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())).map(|(_, rest)| rest)
            })
            .unwrap_or(text)
            .trim();
        if !text.is_empty() {
            lines.push(text.to_string());
        }
    }
    lines
}

/// brief.md에서 그 글을 쓰는 agent가 채우는 자리(plan §8.78). host는 agent가 작업 폴더를 만들기 전에 brief를 짓는다.
/// 작업 폴더(`<cwd>/.aaa/<project>-<thread>`)의 절대 경로
pub const BRIEF_WORK_DIR: &str = "{{aaa:work_dir}}";
/// 그 머신 aaa 실행 파일(진행 보고 명령)
pub const BRIEF_EXE: &str = "{{aaa:exe}}";
/// memory=true 자동화의 재독 안내. host가 세션 명세에 넣으며 hook 요청의 환경·경로는 믿지 않는다.
pub const AUTOMATION_REENTRY_ENV: &str = "AAA_AUTOMATION_REENTRY";
/// 임무 inbox의 목록 조회 한 명령. agent가 채우는 경로와 allow-list가 같은 quoting을 쓴다.
pub const BRIEF_INBOX_LIST: &str = "{{aaa:inbox_list}}";
pub const BRIEF_GIT_READS: &str = "{{aaa:git_reads}}";

pub fn work_inbox_list_command(work_dir: &str) -> String {
    let path = format!("{}/inbox", work_dir.replace('\\', "/").trim_end_matches('/'));
    format!("ls -1 -- '{}'", path.replace('\'', "'\\''"))
}

/// 자기 worktree와 읽기 옵션을 고정한다. 가변 ref/path는 옵션 종료 표지 뒤에서만 받는다.
pub fn work_git_read_commands(work_dir: &str) -> Vec<String> {
    let path = work_dir.replace('\\', "/");
    let Some((cwd, leaf)) = path.rsplit_once("/.aaa/") else { return vec![] };
    if cwd.is_empty() || leaf.is_empty() || leaf.contains('/') || cwd.contains('*') {
        return vec![];
    }
    let git = format!(
        "git -C '{}' --no-pager --no-optional-locks -c core.fsmonitor=false -c log.showSignature=false",
        cwd.replace('\'', "'\\''")
    );
    vec![
        format!("{git} log --no-ext-diff --no-textconv --oneline --max-count=100 --end-of-options"),
        format!("{git} show --no-ext-diff --no-textconv --format=fuller --end-of-options"),
        format!("{git} status --short --"),
    ]
}

/// brief 자리를 채운다. 실행 파일 경로에 빈칸이 있으면 큰따옴표로 감싼다.
pub fn fill_brief(brief: &str, work_dir: &str, exe: &str) -> String {
    let exe = if exe.contains(char::is_whitespace) { format!("\"{exe}\"") } else { exe.to_string() };
    brief
        .replace(
            BRIEF_GIT_READS,
            &work_git_read_commands(work_dir).iter().map(|s| format!("- `{s}`")).collect::<Vec<_>>().join("\n"),
        )
        .replace(BRIEF_INBOX_LIST, &work_inbox_list_command(work_dir))
        .replace(BRIEF_WORK_DIR, work_dir)
        .replace(BRIEF_EXE, &exe)
}

/// 진행 보고 안내(herdr-projects `progress::guidance`를 옮김). thread brief(host)와 Claude SessionStart hook의 안내(agent)가
/// 함께 쓴다(참고도 둘 다 이 글이다). `prefix`는 그 머신 aaa 실행 파일이다.
pub fn progress_guidance(prefix: &str) -> String {
    format!(
        "사용자의 지금 일 전체가 얼마나 됐는지 `{prefix} report`로 알린다. 이것은 타이머나 도구 수가 아니라 너의 어림이다. 보고가 실패해도 실제 일을 멈추지 않는다: 짧은 진단 한 줄만 남기고 다시 하지 않고 이어 간다.\n\n\
         5 단위로 어림한 백분율과 두~네 단어의 활동을 보고한다. 활동은 `코드 읽기`, `변경 시험`, `Waiting for you`처럼 쓴다:\n\n\
         `{prefix} report --percent 25 --activity '코드 읽기'`\n\
         `{prefix} report --unknown --activity '일 가늠'`\n\n\
         새 일을 시작할 때, 의미 있는 고비를 넘긴 뒤, 활동이 바뀔 때, 막혔을 때, 그리고 내용 있는 답을 하기 전마다 보고한다. 사용자에게 무언가 물으려고 멈출 때는 늘 `--activity 'Waiting for you'`로 보고한다(이 말 그대로). 일하는 동안에는 자연스러운 도구 경계에서 1분에 한 번쯤 보고한다. 알림을 채우려고 진행을 지어내지 않는다. 일이 더 있다는 걸 알게 되면 어림을 낮춘다. 범위가 분명하지 않은 동안은 `--unknown`을 쓴다.\n\n\
         thread에서는 작업 시작 때 brief가 지정한 report.md에 `## 진행` 목록을 쓰고, 진행 목록 항목을 끝내거나 시작할 때마다 그 절만이라도 다시 쓴다. 턴의 마지막 답변까지 미루지 않고 위 진행 보고 명령도 함께 실행한다. 완료한 항목은 원래 순서에 남기고 표지·Report·Next 등 다른 절은 보존한다. 진행 보고 명령만으로는 작업 요약 카드의 목록이 갱신되지 않는다.\n\n\
         `--percent 100`은 요청받은 결과 전체와 그 확인이 다 끝났을 때만 쓴다. `Done`으로 보인다. 그 뒤 일을 더 받으면 새로 낮은 백분율을 보고한다.\n\n\
         외부 푸시(웹 푸시·webhook)는 host가 그 coordinator pane으로 검증한 사용자 post만 보낸다. thread 보고·확인창·세션·머신 사건은 앱 안 알림과 coordinator inbox에만 남는다. coordinator는 맡긴 일 전체와 확인이 끝났을 때, 또는 사람이 직접 해야 하는 인증·터미널 승인·GUI 조작·사용자 결정이 남았을 때만 보낸다. 중간 착지·진행에는 보내지 않는다. thread 표지는 앱 안 알림과 coordinator 판단의 근거이며 푸시를 보내지 않는다. thread는 report.md 첫 줄에 `[완료]`, `[결정 필요]`·`[검수 대기]`·`[확인 필요]`, `[착지]`·`[진행 중]`으로 이를 구분한다. 자동 검증·다른 agent 검토는 사람 대기가 아니다. coordinator는 사용자에게 질문하며 멈추기 전에 위 Waiting for you 보고와 coordinator 안내의 `post user --request --needs-user --topic user-action`를 함께 남긴다. 전체 완료 알림은 `post user --notice --topic task-complete`로 한 번 보낸다.\n\n\
         이 pane의 맨 위 agent만 보고한다. subagent와 도우미는 보고하지 않는다. report 명령은 끝날 때까지 기다리고 배경으로 돌리지 않는다."
    )
}

/// inbox 항목(0.2.35 inbox 계승, §8.9). DB가 원본이고 `projects/<p>/inbox/<id>.md`는 투영이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InboxItem {
    /// Front 응답은 프로젝트별 `i-NNNN` 별칭. DB·투영 파일은 기존 긴 ID를 유지한다.
    pub id: String,
    pub project: String,
    /// report·question·delivery·thread·session·outage·health·config-error·routine·routine-approval
    pub item_kind: String,
    /// thread ID, 머신 이름, 파일 이름 등
    pub subject: String,
    /// 짧은 사건 이름(nudge 문구에 쓴다)
    pub event: String,
    /// 사람이 읽는 한 줄(nudge 문구에는 넣지 않는다)
    pub summary: String,
    /// 긴 본문(routine 안내·명령 출력). 투영 파일의 front matter 아래에 쓴다.
    #[serde(default)]
    pub body: String,
    pub done: bool,
    /// coordinator(또는 사람)가 `aaa inbox`로 본 적이 있다. 처음 보는 항목만 알린다.
    pub seen: bool,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number | null")]
    pub done_at_ms: Option<u64>,
}

/// 자동화 한 차례의 결과. 사건 자료·명령 출력 본문은 넣지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoutineRun {
    pub id: String,
    #[ts(type = "number")]
    pub at_ms: u64,
    /// done / skipped / waiting / failed
    pub status: String,
    pub detail: String,
    pub thread: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RoutineView {
    /// 프로젝트별 영속 번호(a-0001부터). 같은 규칙 이름의 수정·복원·형식 변환에도 유지한다. 옛 host는 생략한다.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub short_id: Option<String>,
    /// 웹에서 읽기만 하는 시스템 자동화의 이유. 없으면 일반 자동화(옛 host도 생략).
    #[serde(default)]
    pub web_read_only_reason: Option<String>,
    pub title: String,
    pub name: String,
    pub trigger: String,
    pub action: String,
    /// automation·routine·legacy. legacy는 변환 안내용 옛 파일이다.
    pub origin: String,
    pub file: String,
    pub last_result: Option<RoutineRun>,
    pub last_skip: Option<RoutineRun>,
    pub today: u32,
    pub max_per_day: Option<u32>,
    /// 최근 순서, 최대 20건. 전체 50건은 routine_runs로 읽는다.
    pub recent: Vec<RoutineRun>,
    /// `every 30m`·`daily 09:00`(파일에 적힌 글)
    pub schedule: String,
    /// 비면 안내만 하는 routine
    pub command: String,
    /// 명령을 실행할 머신(없으면 host 머신)
    pub machine: Option<String>,
    pub enabled: bool,
    /// host.toml `[projects] routine_commands`가 이 프로젝트를 허용한다
    pub commands_allowed: bool,
    /// 지금 명령(머신 포함)이 승인돼 있다
    pub approved: bool,
    pub command_sha256: String,
    #[serde(default)]
    pub start_approval_sha256: Option<String>,
    #[serde(default)]
    pub start_approved: Option<bool>,
    #[serde(default)]
    pub start_scope: Option<String>,
    #[ts(type = "number | null")]
    pub last_run_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub next_run_ms: Option<u64>,
    /// 안내 항목을 아직 처리하지 않아 건너뛴 횟수
    pub skipped: u32,
    /// coordinator가 없어 건너뛴 횟수(돌면 0)
    pub no_coordinator: u32,
    /// 파일을 읽지 못했다
    pub error: Option<String>,
    /// 내구 입력 큐(FRONT_API 1.58). 옛 host는 생략한다.
    #[serde(default)]
    pub queue: Option<AutomationQueue>,
    /// 자동화 기억의 host 사본. 옛 host는 생략한다.
    pub memory: Option<AutomationMemory>,
    /// 입력 처리·완료 보고·Idle가 갖춰진 thread의 끝남 예정 시각.
    pub closing: Vec<AutomationClosing>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationMemory {
    pub text: String,
    #[ts(type = "number")]
    pub bytes: u64,
    pub truncated: bool,
    pub thread: Option<String>,
    #[ts(type = "number | null")]
    pub updated_at_ms: Option<u64>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationClosing {
    pub thread: String,
    #[ts(type = "number")]
    pub close_at_ms: u64,
}

/// 규칙의 내구 입력 큐. 보낸 뒤 결과를 모르는 `do.message` 묶음은 사람이 확인하고 푼다(`automation_unblock`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationQueue {
    /// 아직 보내지 않은 입력 수
    pub pending: u32,
    pub inflight: Option<AutomationInflight>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationInflight {
    /// 사람이 본 묶음. 풀 때 이 값을 실어 보낸다.
    pub id: String,
    pub events: u32,
    /// 묶음 첫 입력이 들어온 때
    #[ts(type = "number")]
    pub at_ms: u64,
    /// `do.message` 대상 thread(주인 thread면 그 사건의 thread)
    pub thread: Option<String>,
    /// 보냈는지 모른다: 자동으로 다시 보내지 않고 확인 필요로 기다린다
    pub unknown: bool,
}

/// 상세 화면의 파일 원문(`routine_runs` 답, FRONT_API 1.58). 각 글은 64KiB에서 자른다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RoutineSource {
    pub file: String,
    pub text: String,
    pub sha256: String,
    /// `text_file`·`brief_file`이 가리키는 본문
    pub body_file: Option<String>,
    pub body: String,
    /// 원문(`text`)을 잘랐다
    pub truncated: bool,
    /// 본문(`body`)을 잘랐다
    pub body_truncated: bool,
}

/// 편집할 자동화 파일 하나(관리 GUI W2, FRONT_API 1.76). 원문과 판(sha256)을 함께 준다. 저장은 그 판일 때만 쓴다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationDoc {
    /// 웹에서 읽기만 하는 시스템 자동화의 이유. 없으면 일반 자동화(옛 host도 생략).
    #[serde(default)]
    pub web_read_only_reason: Option<String>,
    pub name: String,
    /// `automations/<이름>.toml`, 옛 형식이면 `routines/<이름>.md`
    pub file: String,
    pub text: String,
    pub sha256: String,
    /// 옛 `.md` 규칙: 웹은 읽기만 하고 저장·삭제를 거부한다
    pub legacy: bool,
    /// `text_file`·`brief_file`이 가리키는 본문 파일과 그 판(없는 파일이면 `body_sha256`이 없다)
    pub body_file: Option<String>,
    pub body: Option<String>,
    pub body_sha256: Option<String>,
    pub check: AutomationCheck,
}

/// 초안 검사 결과. `form`은 원문을 정본 Rust 타입으로 읽은 값(TOML과 같은 모양)이며 TOML 문법·타입 오류면 없다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationCheck {
    #[ts(type = "Record<string, unknown> | null")]
    pub form: Option<serde_json::Value>,
    /// `파일:줄:열 칸: 이유`. 하나라도 있으면 저장하지 않는다
    pub errors: Vec<String>,
    /// 저장은 되지만 실행 때 막히는 것(시작 후보 머신·저장소·profile을 지금 풀지 못함 등)
    pub warnings: Vec<String>,
    pub review: Option<AutomationReview>,
}

/// 검토 화면: 이 초안을 저장하면 무엇이 일어나는가
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationReview {
    pub enabled: bool,
    /// 조건·동작 낱말(목록 줄과 같다)
    pub trigger: String,
    pub action: String,
    /// safety `start_threads`(propose·auto): 자동 시작이 바로 띄우는지, 제안 한 줄만 남기는지
    pub start_threads: String,
    pub start_scope: Option<String>,
    pub start_approval_sha256: Option<String>,
    /// 지금 승인한 범위 안인가(시작 동작만)
    pub start_approved: Option<bool>,
    /// 명령 조건의 명령·머신과 그 hash(승인 대상)
    pub command: String,
    pub machine: Option<String>,
    pub command_sha256: String,
    pub command_approved: bool,
    /// safety `routine_commands`
    pub commands_allowed: bool,
}

/// 지운 자동화(`automations/.trash/`). 되살리면 꺼진 채로 돌아온다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AutomationTrashEntry {
    /// 웹에서 읽기만 하는 시스템 자동화의 이유. 없으면 일반 자동화(옛 host도 생략).
    #[serde(default)]
    pub web_read_only_reason: Option<String>,
    /// `.trash/` 안 파일 이름(되살리기 요청에 쓴다)
    pub id: String,
    pub name: String,
    pub title: String,
    #[ts(type = "number")]
    pub deleted_at_ms: u64,
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
    pub task_notes: Vec<String>,
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

/// FRONT_API 1.40. 자동 정리와 dry-run이 공유하는 판정 결과(크기는 실시간 원본 제외).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryPrunePlan {
    /// FRONT_API 1.114. 상한 밖에서 따로 보존하는 사본 용량.
    #[serde(default)]
    #[ts(type = "number")]
    pub archived_size: u64,
    #[ts(type = "number")]
    pub size: u64,
    #[ts(type = "number")]
    pub remaining_size: u64,
    #[ts(type = "number")]
    pub protected_size: u64,
    #[ts(type = "number")]
    pub max_size: u64,
    #[ts(type = "number")]
    pub max_days: u64,
    pub candidates: Vec<LibraryPruneEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryPruneEntry {
    pub project: String,
    pub name: String,
    #[ts(type = "number")]
    pub size: u64,
    /// 「기한」 또는 「용량」. 기한 정리 뒤 용량을 판정한다.
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// env를 담는 형의 Debug(로그·오류 문구)는 이름만 찍고 값은 찍지 않는다. 이 형을 품은 메시지(AgentRequest·ThreadPrepare 등)도 같다.
    #[test]
    fn debug_shows_env_names_without_values() {
        let value = &*aaa_test_support::token("aaa-canary-debug-0123456789abcdef");
        let env = vec![("GH_TOKEN".to_string(), value.to_string()), ("LANG".to_string(), value.to_string())];
        let resolved = ResolvedProfile {
            harness: "claude".into(),
            model: String::new(),
            effort: String::new(),
            args: vec![],
            command: vec![],
            env: env.iter().cloned().collect(),
        };
        let launch = Launch {
            profile: "p".into(),
            harness: "claude".into(),
            argv: vec!["claude".into()],
            env: env.clone(),
            resume_argv: vec![],
            session_key: None,
        };
        let spec = crate::SpawnSpec {
            name: "n".into(),
            argv: vec![],
            cwd: None,
            env,
            cols: 80,
            rows: 24,
            role: None,
            harness: None,
            profile: Some("p".into()),
            resolved_profile: Some(resolved.clone()),
        };
        let prepare = ThreadPrepare {
            resolved_profile: Some(resolved),
            project: "demo".into(),
            thread: "t-0001".into(),
            title: String::new(),
            kind: ThreadKind::Dir,
            repo: None,
            base: None,
            profile: "p".into(),
            brief: String::new(),
            safety: None,
        };
        let request = crate::AgentRequest::Spawn(spec.clone());
        for text in [format!("{launch:?}"), format!("{spec:?}"), format!("{prepare:#?}"), format!("{request:?}")] {
            assert!(!text.contains(value) && !text.contains("0123456789"), "{text}");
            assert!(text.contains("«env 2개: GH_TOKEN, LANG»"), "{text}");
        }
    }

    /// N-1 host의 wire 응답에는 L3 기록 필드가 없다.
    #[test]
    fn routine_view_reads_previous_front_response() {
        let old = r#"{"name":"ping","schedule":"every 10m","command":"","machine":null,"enabled":true,"commands_allowed":false,"approved":false,"command_sha256":"","last_run_ms":1,"next_run_ms":600001,"skipped":2,"no_coordinator":0,"error":null}"#;
        let view: RoutineView = serde_json::from_str(old).unwrap();
        assert_eq!(view.name, "ping");
        assert_eq!(view.schedule, "every 10m");
        assert_eq!(view.next_run_ms, Some(600001));
        assert!(view.recent.is_empty() && view.last_result.is_none());
    }

    /// 응답·사건 enum 태그(`kind`·`ev`)와 안에 든 구조체 필드 이름이 겹치면 읽을 수 없다
    #[test]
    fn tagged_views_round_trip() {
        use crate::{FrontEvent, FrontResponse};
        let q = QuestionView {
            short_id: None,
            id: "q-1".into(),
            pane: "p".into(),
            machine: "mac-a".into(),
            project: Some("demo".into()),
            thread: Some("t-0001".into()),
            question_kind: "trust".into(),
            text: "Do you trust".into(),
            options: vec!["yes".into(), "no".into()],
            state: QuestionState::Open,
            answer: None,
            created_at_ms: 1,
            menu: None,
        };
        let m = MessageView {
            id: "m-1".into(),
            project: "demo".into(),
            thread: "t-0001".into(),
            mode: MessageMode::Interrupt,
            state: DeliveryState::Accepted,
            text: "x".into(),
            created_at_ms: 1,
        };
        for r in [FrontResponse::Question(q.clone()), FrontResponse::Message(m)] {
            let j = serde_json::to_string(&r).unwrap();
            assert_eq!(serde_json::from_str::<FrontResponse>(&j).unwrap(), r, "{j}");
        }
        let e = FrontEvent::Question(q);
        let j = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<FrontEvent>(&j).unwrap(), e, "{j}");
    }

    #[test]
    fn names_and_slug() {
        assert!(valid_project("aaa-dev_1") && !valid_project("../x") && !valid_project("A") && !valid_project(""));
        assert!(valid_thread("t-0001") && valid_thread("t-12345") && !valid_thread("t-1") && !valid_thread("t-00a1"));
        assert_eq!(slug("Fix the Login bug!"), "fix-the-login-bug");
        assert_eq!(slug("한글 제목"), "");
        assert_eq!(slug("M1-3 Thread: 시작"), "m1-3-thread");
    }

    /// herdr-projects `next_lines_come_from_the_next_section_only`를 옮겼다
    #[test]
    fn next_lines_come_from_the_next_section_only() {
        let report = "[완료] 착지\n## Report\n- not this\n## Next\n- 확인한다\n* Fix CI\n3. Confirm assumption X\n\n## Remember\n- nor this\n";
        assert_eq!(next_lines(report), ["확인한다", "Fix CI", "Confirm assumption X"]);
        assert!(next_lines("## Report\nnothing\n").is_empty());
        assert!(next_lines("## Next\n").is_empty());
        // `#` 머리가 오면 Next가 끝난다
        assert_eq!(next_lines("## Next\n- 하나\n# 다른 절\n- 둘\n"), ["하나"]);
    }

    /// 번호 줄이 질문, 글자 줄이 선택지다. 선택지가 모자라도 질문 제목은 보존하고 진단한다.
    #[test]
    fn report_questions_read_numbered_questions_and_lettered_choices() {
        let report = "[결정 필요] 질문 2개: 배치\n\n## Report\n- 1. 이것은 질문이 아니다\n\n## Questions\n\
            1. 로그인 단추 자리\n   - A 오른쪽 아래 (추천) — 지금 화면과 같음\n   - B) 가운데 큰 단추 — 휴대폰에서 누르기 쉬움\n   - C. 입력칸 바로 아래\n\
            2. 선택지 하나뿐\n   - A 그대로\n\
            3. 비밀번호 보기\n   - A 넣는다(Recommended)\n   - B 넣지 않는다\n\
            - Z 질문 밖 줄은 앞 질문 것이다\n\n## Next\n- 1. 다음 절\n";
        let qs = report_questions(report);
        assert_eq!(qs.len(), 3, "선택지가 모자란 질문 제목도 남긴다: {qs:?}");
        assert_eq!(qs[1].text, "선택지 하나뿐");
        assert_eq!(qs[1].choices.len(), 1);
        assert!(parse_report_questions(report).warning.is_some());
        assert_eq!(qs[0].text, "로그인 단추 자리");
        let c = &qs[0].choices;
        assert_eq!(
            (c[0].label.as_str(), c[0].description.as_deref(), c[0].recommended),
            ("오른쪽 아래", Some("지금 화면과 같음"), true)
        );
        assert_eq!((c[1].label.as_str(), c[1].recommended), ("가운데 큰 단추", false));
        assert_eq!((c[2].label.as_str(), c[2].description.as_deref()), ("입력칸 바로 아래", None));
        assert_eq!(qs[2].text, "비밀번호 보기");
        assert_eq!((qs[2].choices[0].label.as_str(), qs[2].choices[0].recommended), ("넣는다", true));
        assert_eq!(qs[2].choices.len(), 3, "글자 줄은 그 앞 질문에 붙는다");
        assert!(report_questions("## Report\n1. 질문\n- A 하나\n- B 둘\n").is_empty());
    }

    #[test]
    fn report_questions_keep_emphasized_letters_and_numbered_m3_questions() {
        // host가 보존한 t-0578 답변 당시 Questions 원문이다.
        let report = include_str!("../tests/fixtures/m3-questions.md");
        let qs = report_questions(report);
        assert_eq!(qs.len(), 2, "{qs:?}");
        assert_eq!(qs[0].choices.len(), 3);
        assert_eq!(qs[1].choices.len(), 2);
        assert!(qs[0].choices[0].recommended);
        assert!(qs[0].choices[0].label.starts_with("install I3이 main에 올 때까지"));
        assert!(qs[0].choices[2].label.starts_with("install 순서를 바꿔"));
        assert!(qs[1].text.starts_with("M3 끝까지 시험할 곳:"));
    }

    #[test]
    fn report_questions_do_not_count_korean_words_or_alphanumeric_names() {
        for report in [
            include_str!("../tests/fixtures/r-1931-questions.md"),
            include_str!("../tests/fixtures/r-1994-questions.md"),
        ] {
            let parsed = parse_report_questions(report);
            assert!(parsed.warning.is_none(), "{parsed:?}");
            assert_eq!(parsed.items.len(), 1);
            assert_eq!(parsed.items[0].choices.len(), 2);
            assert!(parsed.items[0].choices[0].recommended);
        }
        for word in ["A를 달성", "A안 추천", "B안", "t3code", "A1 항목"] {
            assert_eq!(choice_letter(&format!("- {word}")), None, "{word}");
        }
    }

    #[test]
    fn report_questions_keep_a_direct_answer_card_when_no_heading_is_read() {
        let report = "[결정 필요] 질문 1개: 진행\n## Questions\n### 진행할까요\n- A 진행\n- B 보류\n";
        let parsed = parse_report_questions(report);
        assert!(parsed.warning.is_some());
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].text, "직접 답");
        assert!(parsed.items[0].choices.is_empty());
        assert!(parsed.items[0].body.as_deref().unwrap().contains("### 진행할까요"));
    }

    #[test]
    fn report_questions_accept_choice_emphasis_and_recommendation_spacing() {
        for a in ["**A(추천)**", "**A** (추천)", "*A* (추천)", "A(추천)", "A (추천)", "__A(추천)__", "_A_ (추천)"]
        {
            let report = format!("## Questions\n### 1. 진행\n- {a} 진행한다\n- B 멈춘다\n");
            let parsed = parse_report_questions(&report);
            assert!(parsed.warning.is_none(), "{a}: {parsed:?}");
            assert_eq!(parsed.items[0].choices[0].label, "진행한다", "{a}");
            assert!(parsed.items[0].choices[0].recommended, "{a}");
        }
    }

    #[test]
    fn report_questions_warn_instead_of_silently_relabeling_or_dropping() {
        for report in [
            "## Questions\n### 1. 진행\n- **A 진행\n- B 보류\n- C 멈춤\n",
            "## Questions\n### 1. 진행\n- B 보류\n- C 멈춤\n",
            "## Questions\n### 1. 진행\n- A 진행\n- C 멈춤\n",
            "## Questions\n### 1. 진행\n- A 진행\n- A 보류\n",
            "## Questions\n### 1. 진행\n- A 진행\n### 2. 색\n- A 파랑\n- B 회색\n",
            "## Questions\n### 1. 진행\n- A 진행\n- B 보류\n### 둘째 질문\n- A 파랑\n- B 회색\n",
            "## Questions\n### 1. 진행\n- A 진행\n- B 보류\n#### 2. 잘못된 질문 머리\n- A 파랑\n- B 회색\n",
            "[결정 필요] 질문 2개: 색\n## Questions\n1. 색\n- A 파랑\n- B 회색\n",
            "## Questions\n질문 제목만\n- A 파랑\n- B 회색\n",
            "## Questions\n2. 색\n- A 파랑\n- B 회색\n",
        ] {
            let parsed = parse_report_questions(report);
            assert!(parsed.warning.as_deref().is_some_and(|w| w.contains("형식 확인 필요")), "{report}: {parsed:?}");
        }
        let too_many = (0..10).map(|i| format!("- {} 선택\n", (b'A' + i) as char)).collect::<String>();
        assert!(parse_report_questions(&format!("## Questions\n1. 많음\n{too_many}")).warning.is_some());
        let ten = (1..=10).map(|i| format!("### {i}. 질문\n- A 진행\n- B 보류\n")).collect::<String>();
        assert!(parse_report_questions(&format!("## Questions\n{ten}")).warning.is_some());
        let code = "## Questions\n### 1. 진행\n```md\n2. 코드 속 번호\n- X 코드 속 글자\n```\n- A 진행\n- B 보류\n";
        assert!(parse_report_questions(code).warning.is_none());
        assert!(parse_report_questions("## Report\n- A 본문\n- B 본문\n").warning.is_none());
        assert!(parse_report_questions(include_str!("../tests/fixtures/m3-questions.md")).warning.is_none());
    }

    #[test]
    fn reordered_choices_keep_source_letters_and_ambiguous_letters_are_diagnosed() {
        let report = "## Questions\n### 1. 진행\n- B 새 안 (추천)\n- A 원래\n- C 보류\n";
        let parsed = parse_report_questions(report);
        assert!(parsed.warning.is_none(), "{parsed:?}");
        let q = &parsed.items[0];
        assert_eq!(q.choices.iter().enumerate().map(|(i, c)| c.letter_at(i)).collect::<Vec<_>>(), ["B", "A", "C"]);
        assert_eq!(q.choice_index("b"), Some(0));
        assert_eq!(q.choice_index("A"), Some(1));
        assert_eq!(q.choice_index("D"), None);
        for broken in [report.replace("- C 보류", "- B 보류"), report.replace("- C 보류", "- 보류")] {
            assert!(parse_report_questions(&broken).warning.is_some());
        }
        let duplicate = parse_report_questions(&report.replace("- C 보류", "- B 보류"));
        assert!(duplicate.items[0].choices.is_empty());
        let legacy: QuestionChoice =
            serde_json::from_str(r#"{"label":"옛 자료","description":null,"recommended":false}"#).unwrap();
        assert_eq!(legacy.letter_at(1), "B");
    }

    #[test]
    fn rich_question_keeps_numbered_body_lists_and_accepts_a_numbered_next_question() {
        let report = "## Questions\n### 1. 순서\n1. 먼저 읽는다\n2. 다음 확인한다\n\n- A 진행 (추천)\n- B 보류\n\n2. **시험할 곳**\n- A 여기\n- B 실험실\n";
        let parsed = parse_report_questions(report);
        assert!(parsed.warning.is_none(), "{parsed:?}");
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].text, "순서");
        assert_eq!(parsed.items[0].body.as_deref(), Some("1. 먼저 읽는다\n2. 다음 확인한다"));
        assert_eq!(parsed.items[1].text, "시험할 곳");
    }

    #[test]
    fn t0621_trailing_icon_notes_do_not_drop_the_first_question() {
        let report = include_str!("../tests/fixtures/t0621-questions.md");
        let parsed = parse_report_questions(report);
        assert_eq!(parsed.items.len(), 2, "{parsed:?}");
        assert_eq!(parsed.items[0].choices.len(), 4);
        assert_eq!(parsed.items[1].choices.len(), 3);
        assert!(parsed.warning.is_none(), "{parsed:?}");
        let q = &parsed.items[0];
        assert_eq!(q.text, "단추 아이콘");
        assert!(q.after.as_deref().unwrap().contains("E·F안 그림:"));
        assert!(q.after.as_deref().unwrap().contains("![E안](library/icon-e.png)"));
        assert!(q.after.as_deref().unwrap().contains("![F안](library/icon-f.png)"));
        assert_eq!(q.choices[3].body.as_deref(), Some("![D안](library/icon-d.png)"));
        let broken = report.replace("- B BotMessageSquare", "- **B BotMessageSquare");
        assert!(parse_report_questions(&broken).warning.is_some(), "줄이 빠지면 경고한다");
    }

    #[test]
    fn t0621_table_rows_and_indented_pictures_do_not_limit_choices_to_four() {
        let original = include_str!("../tests/fixtures/t0621-questions.md");
        let start = original.find("\nE·F안 그림:").unwrap();
        let end = original[start..].find("\n### 2.").unwrap() + start;
        let no_after = format!("{}{}", &original[..start], &original[end..]);
        let four_rows = original
            .lines()
            .filter(|l| !l.starts_with("| E |") && !l.starts_with("| F |"))
            .collect::<Vec<_>>()
            .join("\n");
        let no_choice_images = original.lines().filter(|l| !l.starts_with("  ![")).collect::<Vec<_>>().join("\n");
        let tail_text_only = original
            .lines()
            .filter(|l| !l.starts_with("![E안]") && !l.starts_with("![F안]"))
            .collect::<Vec<_>>()
            .join("\n");
        let six = no_after.replace(
            "\n### 2.",
            "\n- E TowerControl\n  ![E](library/icon-e.png)\n- F Headset\n  ![F](library/icon-f.png)\n\n### 2.",
        );
        for (report, count) in
            [(original, 4), (&four_rows, 4), (&no_choice_images, 4), (&tail_text_only, 4), (&no_after, 4), (&six, 6)]
        {
            let parsed = parse_report_questions(report);
            assert_eq!(parsed.items.len(), 2, "{parsed:?}");
            assert_eq!(parsed.items[0].choices.len(), count, "{parsed:?}");
            assert!(parsed.warning.is_none(), "{parsed:?}");
        }
        let bad = parse_report_questions("## Questions\n### 1. 단추 아이콘\n- A Bot\n");
        assert_eq!(bad.items[0].text, "단추 아이콘");
        assert_eq!(bad.items[0].choices.len(), 1);
        assert!(bad.warning.is_some());
        let ten = (b'A'..=b'J').map(|c| format!("- {} 아이콘\n", c as char)).collect::<String>();
        let over = parse_report_questions(&format!("## Questions\n### 1. 단추 아이콘\n{ten}"));
        assert_eq!(over.items[0].text, "단추 아이콘");
        assert!(over.warning.is_some(), "상한 초과도 제목과 경고를 남긴다");
    }

    #[test]
    fn report_questions_read_rich_bodies_and_choice_pictures() {
        let report = "[결정 필요] 질문 2개: 시안\n\n## Questions\n### 1. 왼쪽 막대 시안\n지금 모양:\n\n![지금](library/now.png)\n\n\
            | 안 | 차이 |\n| --- | --- |\n| A | 한 화면 |\n\n- 보통 글머리\n- I think 이것도 본문\n\n```text\n- A 코드 안 줄\n```\n\n\
            - A 따로 된 화면 (추천) — 한 번에 답함\n  ![A안](library/A.png)\n\n  둘째 줄\n- B 사이드바 목록 — 목록이 옆에\n  ![B안](library/B.png)\n\n\
            ### 2. 정리 기간\n- A 14일 (추천)\n- B 30일\n\n## Next\n- 다음\n";
        let qs = report_questions(report);
        assert_eq!(qs.len(), 2, "{qs:?}");
        assert_eq!(qs[0].text, "왼쪽 막대 시안");
        let body = qs[0].body.as_deref().unwrap();
        assert!(
            body.starts_with("지금 모양:")
                && body.contains("![지금](library/now.png)")
                && body.contains("| A | 한 화면 |")
        );
        assert!(
            body.contains("- I think 이것도 본문") && body.ends_with("```"),
            "본문 글머리·code 안 줄은 본문이다: {body}"
        );
        let c = &qs[0].choices;
        assert_eq!(c.len(), 2);
        assert_eq!(
            (c[0].label.as_str(), c[0].recommended, c[0].description.as_deref()),
            ("따로 된 화면", true, Some("한 번에 답함"))
        );
        assert_eq!(c[0].body.as_deref(), Some("![A안](library/A.png)\n\n둘째 줄"));
        assert_eq!(c[1].body.as_deref(), Some("![B안](library/B.png)"));
        assert_eq!((qs[1].text.as_str(), qs[1].body.as_deref(), qs[1].choices.len()), ("정리 기간", None, 2));
        // 한도: 글자 경계에서 자르고 열린 fence를 닫는다
        let long =
            format!("## Questions\n### 1. 긴 본문\n```\n{}\n```\n- A 하나\n- B 둘\n", "가".repeat(QUESTION_BODY_MAX));
        let b = report_questions(&long)[0].body.clone().unwrap();
        assert!(
            b.len() < QUESTION_BODY_MAX + 100 && b.contains("```\n\n*(본문이 길어 잘렸다"),
            "{}",
            &b[b.len() - 80..]
        );
    }

    #[test]
    fn fill_brief_puts_the_machine_paths() {
        let b = format!("`{BRIEF_WORK_DIR}/report.md` `{BRIEF_EXE} report --unknown`");
        assert_eq!(
            fill_brief(&b, "/w/.aaa/p-t-0001", "/opt/aaa"),
            "`/w/.aaa/p-t-0001/report.md` `/opt/aaa report --unknown`"
        );
        assert_eq!(fill_brief(BRIEF_EXE, "", r"C:\Program Files\aaa.exe"), r#""C:\Program Files\aaa.exe""#);
        assert_eq!(fill_brief(BRIEF_INBOX_LIST, "/work/it's a folder", ""), "ls -1 -- '/work/it'\\''s a folder/inbox'");
        assert_eq!(fill_brief(BRIEF_INBOX_LIST, r"C:\work dir\task", ""), "ls -1 -- 'C:/work dir/task/inbox'");
        let reads = fill_brief(BRIEF_GIT_READS, "/work/it's a folder/.aaa/demo-t-0001", "");
        assert!(reads.contains("git -C '/work/it'\\''s a folder' --no-pager --no-optional-locks"));
        assert!(reads.contains("--end-of-options") && reads.contains("status --short --"));
        assert!(work_git_read_commands("/work/other").is_empty());
        assert!(work_git_read_commands("/work/*/.aaa/demo-t-0001").is_empty());
    }
}

/// TASKS.md의 task 한 줄. 내용은 Markdown, 표시 번호는 host DB가 정본이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskView {
    /// 프로젝트별 영구 표시 ID(`d-0001`). 내부 연결은 기존 ULID를 유지한다.
    #[serde(default)]
    pub short_id: Option<String>,
    /// 제목과 독립된 내부 ULID. 기존 task는 host의 식별 이주 때 주석으로 부여한다.
    #[serde(default)]
    pub id: Option<String>,
    /// 속한 `## ` 목록 이름
    pub list: String,
    pub title: String,
    /// 괄호 안 owner 원문(`me`, `claude`, `codex@mac-b`, 사람 이름 등)
    pub owner: Option<String>,
    /// agent에게 맡긴 task의 모델(profile) 칸(FRONT_API 1.75, plan §8.335): owner `모델@머신`의 앞. 맨 이름은 host가 아는 profile일
    /// 때만 모델이다(아니면 사람). 비면 시작할 때 고른다. 옛 host는 생략한다
    #[serde(default)]
    pub profile: Option<String>,
    /// agent에게 맡긴 task의 머신 칸(FRONT_API 1.75): owner `모델@머신`의 뒤. 비면 그 저장소가 있는 머신 중 덜 붐비는 곳이다
    #[serde(default)]
    pub machine: Option<String>,
    /// owner 뒤 대괄호의 저장소 칸(FRONT_API 1.73, plan §8.330): `project.toml` repos 경로의 마지막 이름이나 `작업 폴더`. 비면 정하지
    /// 않은 task다(「바로 시작」 창에서 고른다). 옛 host는 생략한다
    #[serde(default)]
    pub repo: Option<String>,
    /// ` · t-0001`로 붙은 thread
    pub thread: Option<String>,
    /// task 줄 아래 들여쓴 메모
    pub notes: Vec<String>,
    pub done: bool,
    /// task 줄 번호(1부터, FRONT_API 1.60). 옛 host는 생략한다.
    #[serde(default)]
    pub line: u32,
    /// task 줄 원문(앞 공백 제외)
    #[serde(default)]
    pub raw: String,
    /// task 블록(task 줄과 메모 줄, 블록 끝 빈 줄 제외, 줄 끝 문자 없이 `\n`으로 이음)의 sha256. 편집(T2)의 충돌 anchor다.
    #[serde(default)]
    pub block_sha256: String,
    /// 들여쓴 하위 task면 상위 task의 `line`
    #[serde(default)]
    pub parent: Option<u32>,
    /// 형식 어긋남(owner 칸이 owner 꼴이 아님, 제목 안 thread 번호 등). 있으면 화면이 원문을 보이고 판단하지 않는다.
    #[serde(default)]
    pub problems: Vec<String>,
    /// 형식 어긋남 task의 「정리 제안」(FRONT_API 1.62): host가 만든 정규형 후보. 사람이 「이대로 저장」해야 바뀐다.
    #[serde(default)]
    pub suggestion: Option<TaskSuggestion>,
    /// 「확인 필요」의 까닭(FRONT_API 1.63, 할 일 T3): 이어진 thread가 끝났는데 완료로 판정하지 못했다(보고 표지·버림·하위 task
    /// 남음), 하위 task가 모두 끝났다 등. host가 지우지 않고 사람이 「끝남으로 지우기」·「다시 맡기기」·「연결만 풀기」로 정한다
    #[serde(default)]
    pub attention: Option<String>,
    /// 자동 병합(FRONT_API 1.143): on이면 `[병합 준비됨]` 뒤 coordinator가 사용자에게 묻지 않고 착지 창을 준다. host DB 값이며
    /// 프로젝트 기본값 없이 off에서 시작한다. 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<bool>")]
    pub auto_merge: bool,
}

/// 「정리 제안」(할 일 편집 T2): 끝 괄호 글 → 메모, 본문 속 열린 thread 번호 → 연결, `미할당` → owner 비움
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskSuggestion {
    pub title: String,
    pub owner: Option<String>,
    pub thread: Option<String>,
    /// 제안한 뒤의 메모 전체(지금 메모 앞에 옮긴 글을 둔다)
    pub notes: Vec<String>,
    /// 제안대로 쓴 task 줄(앞 공백 없이)
    pub line: String,
    /// 무엇을 바꾸나(한 줄씩)
    pub changes: Vec<String>,
}

/// 할 일 편집의 대상 task(FRONT_API 1.62). `block_sha256`으로 찾고(같은 hash가 여럿이면 `line`에 가까운 것), 그 블록이 없으면
/// `revision_conflict`다(지운 task가 저장으로 되살아나지 않는다). `block_sha256`이 비면 제목(대소문자 무시)으로 찾는다(CLI).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskAnchor {
    pub list: String,
    pub title: String,
    pub block_sha256: String,
    pub line: u32,
}

/// 할 일 편집 동작 하나(FRONT_API 1.62, `task_edit`). 한 요청의 동작은 차례로 적용해 한 번에 쓴다(하나라도 실패하면 쓰지 않는다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TaskOp {
    /// 목록 끝(또는 `before` 앞, `parent`의 하위 끝)에 더한다. 없는 목록은 파일 끝에 만든다
    Add {
        list: String,
        title: String,
        /// 비움 = 미할당
        #[serde(default)]
        owner: Option<String>,
        /// owner가 사람 이름이다(「사람으로 저장」). 아니면 `assignable --check`와 같은 판정을 거친다
        #[serde(default)]
        person: bool,
        /// 모델(profile) 칸(FRONT_API 1.75). 머신 칸과 함께 owner `모델@머신`으로 적는다. `owner`(me·사람)와 함께 주면 거부한다
        #[serde(default)]
        profile: Option<String>,
        /// 머신 칸(FRONT_API 1.75)
        #[serde(default)]
        machine: Option<String>,
        #[serde(default)]
        notes: Vec<String>,
        #[serde(default)]
        parent: Option<TaskAnchor>,
        #[serde(default)]
        before: Option<TaskAnchor>,
        /// 이어 줄 thread(` · t-NNNN`, FRONT_API 1.63 「task로 만들기」). 옛 host는 버린다
        #[serde(default)]
        thread: Option<String>,
        /// 저장소 칸(FRONT_API 1.73): 저장소 이름·경로나 `작업 폴더`. 경로는 host가 이름으로 바꿔 적는다. 비움 = 정하지 않음
        #[serde(default)]
        repo: Option<String>,
    },
    /// 준 칸만 바꾼다(`None`은 그대로, owner·저장소·thread의 빈 글은 비움). 바뀌지 않은 줄은 바이트 그대로 둔다
    Update {
        at: TaskAnchor,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        owner: Option<String>,
        #[serde(default)]
        person: bool,
        /// 모델 칸(FRONT_API 1.75): `None`은 그대로, 빈 글은 비움. 머신 칸만 바꾸면 모델은 그대로 둔다
        #[serde(default)]
        profile: Option<String>,
        /// 머신 칸(FRONT_API 1.75, `profile`과 같은 규칙)
        #[serde(default)]
        machine: Option<String>,
        #[serde(default)]
        notes: Option<Vec<String>>,
        #[serde(default)]
        thread: Option<String>,
        #[serde(default)]
        done: Option<bool>,
        /// 저장소 칸(FRONT_API 1.73, Add의 `repo`와 같은 값)
        #[serde(default)]
        repo: Option<String>,
    },
    /// 기존 메모 원문 끝에 줄을 더한다(FRONT_API 1.134). 블록 hash 충돌·기록은 Update와 같다.
    AppendNotes {
        at: TaskAnchor,
        notes: Vec<String>,
    },
    /// task 블록을 하위 task와 함께 지운다(기록에 남아 되살릴 수 있다)
    Delete {
        at: TaskAnchor,
    },
    /// 하위 task와 함께 옮긴다: `before` 앞, 아니면 `parent`의 하위 끝, 아니면 `to_list` 끝
    Move {
        at: TaskAnchor,
        to_list: String,
        #[serde(default)]
        before: Option<TaskAnchor>,
        #[serde(default)]
        parent: Option<TaskAnchor>,
    },
    /// 새 `##` 목록(파일 끝, 또는 `after` 목록 뒤)
    ListAdd {
        name: String,
        #[serde(default)]
        after: Option<String>,
    },
    ListRename {
        name: String,
        to: String,
    },
    /// `name`의 task와 설명 줄을 `into` 끝으로 옮기고 `name` 머리를 지운다
    ListMerge {
        name: String,
        into: String,
    },
    /// task가 없는 목록만 지운다(설명 줄은 기록에 남는다)
    ListDelete {
        name: String,
    },
    /// 목록을 `before` 목록 앞으로(없으면 파일 끝으로) 옮긴다
    ListMove {
        name: String,
        #[serde(default)]
        before: Option<String>,
    },
    /// 기록의 삭제 항목을 원래 목록 끝(또는 `list`)에 다시 넣는다. 같은 제목이 있으면 거부한다
    Restore {
        #[ts(type = "number")]
        log_id: i64,
        #[serde(default)]
        list: Option<String>,
    },
    /// 원문 편집(고급): 파일 전체 sha256이 같을 때만 통째로 쓴다. 다른 동작과 함께 보내지 않는다
    RawSave {
        expected_sha256: String,
        text: String,
    },
}

/// 편집이 최신 파일과 맞지 않았다(`revision_conflict`). 아무것도 쓰지 않았다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskConflict {
    /// 맞지 않은 동작의 차례(0부터)
    pub op: u32,
    pub reason: String,
    /// 그 task의 지금 원문(같은 제목으로 찾은 것, 지워졌으면 없음)
    // Box로 응답 enum의 크기를 제한한다. JSON·TypeScript 모양은 같다.
    pub latest: Option<Box<TaskView>>,
}

/// 할 일 기록의 동작
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskAction {
    Add,
    Update,
    Delete,
    Move,
    ListAdd,
    ListRename,
    ListMerge,
    ListDelete,
    ListMove,
    Restore,
    Raw,
    /// 끝난 thread의 task를 host가 지웠다(T3)
    AutoClean,
    Completed,
    /// 자동 병합 on/off 변경(FRONT_API 1.143). before·after는 `off`·`on`이다
    AutoMerge,
}

impl TaskAction {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskAction::Add => "add",
            TaskAction::Update => "update",
            TaskAction::Delete => "delete",
            TaskAction::Move => "move",
            TaskAction::ListAdd => "list_add",
            TaskAction::ListRename => "list_rename",
            TaskAction::ListMerge => "list_merge",
            TaskAction::ListDelete => "list_delete",
            TaskAction::ListMove => "list_move",
            TaskAction::Restore => "restore",
            TaskAction::Raw => "raw",
            TaskAction::AutoClean => "auto_clean",
            TaskAction::Completed => "completed",
            TaskAction::AutoMerge => "auto_merge",
        }
    }

    pub fn parse(s: &str) -> Option<TaskAction> {
        [
            TaskAction::Add,
            TaskAction::Update,
            TaskAction::Delete,
            TaskAction::Move,
            TaskAction::ListAdd,
            TaskAction::ListRename,
            TaskAction::ListMerge,
            TaskAction::ListDelete,
            TaskAction::ListMove,
            TaskAction::Restore,
            TaskAction::Raw,
            TaskAction::AutoClean,
            TaskAction::Completed,
            TaskAction::AutoMerge,
        ]
        .into_iter()
        .find(|a| a.as_str() == s)
    }

    /// 되살릴 수 있는 항목인가(task 블록이 빠졌다)
    pub fn removes(self) -> bool {
        matches!(self, TaskAction::Delete | TaskAction::AutoClean)
    }
}

/// 할 일 기록 한 줄(host DB `task_log`, FRONT_API 1.62)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskLogEntry {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub at_ms: u64,
    /// 누가(원문): `device:<id>`, `coordinator:<pane>`, `local:…`, `file`(파일 직접), `host`(자동 정리)
    pub actor: String,
    /// 누가(보이는 이름): 기기 이름·coordinator·터미널·파일 직접·host 자동 정리
    pub by: String,
    pub action: TaskAction,
    pub list: String,
    /// 옮김·목록 동작의 앞 목록(옮기기 전 목록, 이름 바꾸기·합치기 전 이름)
    pub from_list: Option<String>,
    pub title: String,
    /// 바뀌기 전 블록 원문(하위 task 포함, 더하기에는 없다)
    pub before: Option<String>,
    pub after: Option<String>,
    pub thread: Option<String>,
    /// 되살린 기록이면 그 항목 id
    #[ts(type = "number | null")]
    pub restored_by: Option<i64>,
}

/// 기록 거르기
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskLogFilter {
    /// 삭제·자동 정리(되살릴 수 있는 것)
    Deleted,
    /// 자동 정리만
    Auto,
    /// 복원 동작 또는 되살린 삭제 기록
    Restored,
}

/// TASKS.md의 `##` 목록 하나(FRONT_API 1.60). task가 없는 목록도 있다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskListView {
    /// `##` 머리 이름. 첫 `##` 앞의 task·설명 줄은 이름 없는 목록(`""`)이다
    pub name: String,
    /// 머리 밑 체크박스 없는 줄(목록 설명). task 블록 안의 메모는 들지 않는다
    pub notes: Vec<String>,
}

/// 할 일 화면의 thread 한 줄(id·제목·묶음)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskThread {
    pub id: String,
    pub title: String,
    pub group: ThreadGroup,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stage: Option<ThreadStage>,
}

/// `task_notes` 파일(예: `memory/later.md`)의 `##` 절 하나. 줄은 Markdown 원문 그대로다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskNotesSection {
    /// `##` 머리 이름. 첫 `##` 앞 줄은 이름 없는 절이다
    pub name: String,
    pub lines: Vec<String>,
}

/// 「나중」 탭의 읽기 전용 파일 하나(project.toml `task_notes`)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskNotesFile {
    /// 프로젝트 폴더 기준 상대 경로
    pub path: String,
    /// 읽지 못한 까닭(없음·프로젝트 폴더 밖·너무 큼). 있으면 `sections`는 비어 있다
    pub error: Option<String>,
    pub sections: Vec<TaskNotesSection>,
}

/// `tasks` 답(FRONT_API 1.60): 할 일 화면이 읽는 TASKS.md 전체
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskBoard {
    #[serde(default)]
    pub queue: Vec<crate::TaskReservation>,
    pub project: String,
    /// 파일 전체 바이트의 sha256(파일이 없으면 빈 글의 hash)
    pub sha256: String,
    pub lists: Vec<TaskListView>,
    pub tasks: Vec<TaskView>,
    /// 열린 thread 중 어느 task의 `thread`에도 없는 것
    pub unlinked_threads: Vec<TaskThread>,
    /// project.toml `task_notes`의 파일마다(설정이 비면 비어 있다)
    pub notes_files: Vec<TaskNotesFile>,
    /// 파일 원문(원문 편집, FRONT_API 1.62). 옛 host는 생략한다
    pub text: String,
    /// task 저장소 칸에 쓸 수 있는 이름(FRONT_API 1.73): `project.toml` repos 경로의 마지막 이름, 겹치지 않게 적힌 차례
    pub repos: Vec<String>,
}

/// 할 일 「바로 시작」 확인창이 보일 것(FRONT_API 1.63, `thread_start_preview`). host가 `thread start --from-task`와 같은 규칙으로
/// 채운다(owner → profile·머신, 메모를 붙인 할 일 글). 아무것도 시작하지 않는다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ThreadStartPreview {
    #[serde(default)]
    pub workspace_access: std::collections::BTreeMap<String, Vec<String>>,

    pub profile_revision: String,
    pub model: String,
    pub effort: String,
    /// task 제목(thread 제목이 된다)
    pub title: String,
    /// 지금 파일의 그 task 블록 hash(시작 요청의 `task_block`)
    pub task_block: String,
    /// thread에 줄 할 일 글(제목과 「Notes from the task list」 메모)
    pub task: String,
    /// owner가 정한 profile, 없으면 프로젝트 기본 thread profile
    pub profile: String,
    /// owner가 정한 머신(없으면 host가 저장소의 머신이나 로컬로 고른다)
    pub machine: Option<String>,
    /// project.toml `repos`(worktree로 시작할 저장소). 비면 작업 폴더로 시작한다
    pub repos: Vec<RepoChoice>,
    /// 사용자·coordinator가 띄운 열린 Worker 수(시작·정리 중 포함)와 host가 집행하는 한도. 자동화·병합 대기·착지 한 자리는 뺀다.
    pub open_threads: u32,
    pub max_parallel_threads: u32,
    /// 이미 열린 thread가 이 task를 맡았다 등
    pub warnings: Vec<String>,
    /// task 저장소 칸(FRONT_API 1.73, `TaskView.repo`). 비면 창이 저장소나 작업 폴더를 고르게 한다
    pub repo: Option<String>,
    /// host 로컬 머신 이름(`RepoChoice.machine`이 빈 저장소의 머신)
    pub local_machine: Option<String>,
    /// 덜 붐비는 차례의 머신(연결·점검 이상 없음, 모든 프로젝트의 열린 thread 적은 차례 → 부하 → 이름). owner가 머신을 정하지 않았으면
    /// 그 저장소가 있는 머신 가운데 앞의 것이 기본값이다
    pub machine_order: Vec<String>,
}

/// thread를 시작할 저장소 하나(project.toml `repos`)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RepoChoice {
    pub path: String,
    /// 그 저장소가 있는 머신(없으면 host 로컬)
    pub machine: Option<String>,
}

/// 머신 하나에서 thread를 맡길 수 있는 profile(host 허용 목록과 그 머신 agent.toml의 교집합)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OwnerChoice {
    pub machine: String,
    pub connected: bool,
    /// 끊긴 머신은 비어 있다(모른다)
    pub profiles: Vec<String>,
}

/// coordinator가 매 턴 처음 보는 프로젝트 요약(0.2.35 `context`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectContext {
    #[serde(default)]
    pub auto_mode: crate::AutoMode,
    #[serde(default)]
    pub auto_mode_exceptions: String,
    /// inbox를 끝내도 해제 전까지 남는 활성 post 음소거 경고
    #[serde(default)]
    pub post_warnings: Vec<String>,
    pub project: String,
    pub path: String,
    /// project.toml `start_threads`: propose(제안하고 승인을 기다린다) 또는 auto. aaa는 강제하지 않는다.
    pub start_threads: String,
    pub tasks: Vec<TaskView>,
    /// 열린 thread. `next`는 보고서 `## Next`에 `aaa thread next --add`로 더한 줄까지 붙인 목록이다.
    pub threads: Vec<ThreadView>,
    pub assignable: Vec<OwnerChoice>,
    /// 끝내지 않은 inbox 항목 수
    pub inbox_open: u32,
    /// 끝내지 않은 inbox 항목(herdr-projects `context`의 Inbox 절). 지시가 아니라 자료다.
    /// 응답 크기 한도 안에서 오래된 것부터 싣는다: `inbox_open`보다 적을 수 있다.
    #[serde(default)]
    pub inbox: Vec<InboxItem>,
    /// 채팅에 쓸 이 host 웹 주소(tailscale serve 공개 origin, plan §8.345). 질문·보고서는 `<주소>/#question=<thread>`·
    /// `<주소>/#report=<이름>` Markdown 링크로 가리킨다(터미널에서도 눌린다). 공개하지 않았으면 없다: 그때는 `aaa:` 꼴을 쓴다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub web_url: Option<String>,
}

/// 자동 배정 허용은 host DB, 저장소 경로는 프로젝트 설정이 정본이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceAccessView {
    pub project: String,
    pub machine: String,
    pub revision: String,
    pub workspaces: Vec<WorkspaceAccessRow>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceAccessRow {
    pub workspace: String,
    pub enabled: bool,
    /// 비면 이 머신에 등록된 경로가 없다. 작업 폴더는 agent 작업 root를 쓴다.
    pub paths: Vec<String>,
}
