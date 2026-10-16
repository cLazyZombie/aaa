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
}

/// agent.toml profile을 그 머신에서 해석한 결과. host는 thread 기록에 그대로 저장하고 restart·resume에 다시 쓴다(§7.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProfileView {
    pub name: String,
    pub harness: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub description: String,
}

/// host → agent: thread 자리를 만든다(worktree 또는 작업 폴더)와 brief.md 쓰기, profile 해석
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPrepare {
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
}

/// 웹에서 보는 보고서·결과물. `base`는 짧은 수명의 읽기 전용 경로(`/v/<cap>/`)이고 그 아래에 `path`를 붙인다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LibraryView {
    pub project: String,
    pub thread: String,
    /// 회수한 report.md(없으면 None)
    pub report: Option<String>,
    pub files: Vec<LibraryFile>,
    pub base: String,
    #[ts(type = "number")]
    pub expires_at_ms: u64,
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
    /// session_start·turn_start·turn_end·question·idle·error·exited·progress
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
    pub harness_session: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    Open,
    Answered,
    /// 답하기 전에 harness가 넘어갔다(다른 곳에서 답함, 세션 끝남)
    Gone,
}

/// 확인창 질문(§8.4). 답은 host가 세션·상태를 다시 확인한 뒤 adapter 키 입력으로 보낸다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionView {
    pub id: String,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageMode {
    /// 지금 보낸다(작업 중이면 먼저 멈춘다)
    Interrupt,
    /// host 대기열에 두고 턴이 끝나면 보낸다
    #[default]
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
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
    pub name: String,
    pub path: String,
    pub threads: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ThreadStatus {
    /// 자리 만들기·agent 시작 중
    Starting,
    Open,
    Failed,
    Resolved,
}

impl ThreadStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ThreadStatus::Starting => "starting",
            ThreadStatus::Open => "open",
            ThreadStatus::Failed => "failed",
            ThreadStatus::Resolved => "resolved",
        }
    }

    pub fn parse(s: &str) -> Option<ThreadStatus> {
        match s {
            "starting" => Some(ThreadStatus::Starting),
            "open" => Some(ThreadStatus::Open),
            "failed" => Some(ThreadStatus::Failed),
            "resolved" => Some(ThreadStatus::Resolved),
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
    /// 사람이나 coordinator가 할 일이 있다(시작 실패, brief 전달 불명, `[사용자 작업 필요]`·`[결정 필요]` 보고)
    WaitingOnYou,
    /// 아직 확인(ack)하지 않은 새 보고
    ReadyForReview,
    Working,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadView {
    pub project: String,
    pub id: String,
    pub title: String,
    pub machine: String,
    pub profile: String,
    pub role: Role,
    /// (FrontResponse 태그 `kind`와 겹치지 않게 이름을 달리한다)
    pub thread_kind: ThreadKind,
    pub status: ThreadStatus,
    pub group: ThreadGroup,
    pub brief: BriefState,
    pub pane: Option<String>,
    pub cwd: Option<String>,
    pub work_dir: Option<String>,
    pub branch: Option<String>,
    pub report_hash: Option<String>,
    pub acked_hash: Option<String>,
    /// report.md 첫 줄 상태 태그(예: `진행 중`, `완료`)
    pub report_state: Option<String>,
    /// report.md `## Next` 줄
    pub next: Vec<String>,
    /// 마지막 사본 결과: complete·partial·failed
    pub copy: Option<String>,
    pub error: Option<String>,
    /// agent harness 상태(hook으로 안다)
    pub agent: Option<Box<AgentStatus>>,
    /// thread를 시작할 때 brief에 넣은 결정 기록 번호(가장 큰 번호). 지금 번호보다 작으면 옛 결정으로 일한다.
    pub decisions_rev: Option<u32>,
    /// 프로젝트의 지금 결정 번호
    pub decisions_now: Option<u32>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    #[ts(type = "number")]
    pub updated_at_ms: u64,
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
    pub role: Option<Role>,
    pub profile: Option<String>,
    pub machine: Option<String>,
    /// 근거 경로(host 프로젝트 폴더 기준, 예: `threads/t-0001.md`)
    pub evidence: Option<String>,
    /// 사람이 읽는 한 줄
    pub detail: String,
}

/// report.md 해석: 첫 줄 상태 태그와 `## Next` 줄.
/// 0.2.35에는 없던 규칙이다. 운영 brief가 쓰는 형식(`[진행 중]`·`[사용자 작업 필요]`·`[결정 필요]`·`[완료]`)을 읽는다.
pub fn parse_report(text: &str) -> (Option<String>, Vec<String>) {
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let state = first.strip_prefix('[').and_then(|r| r.split_once(']')).map(|(s, _)| s.trim().to_string());
    let mut next = Vec::new();
    let mut in_next = false;
    for line in text.lines() {
        let t = line.trim();
        if let Some(h) = t.strip_prefix("## ") {
            in_next = h.trim() == "Next";
            continue;
        }
        if t.starts_with("# ") {
            in_next = false;
            continue;
        }
        if in_next && !t.is_empty() {
            let item = t
                .strip_prefix("- ")
                .or_else(|| t.strip_prefix("* "))
                .or_else(|| t.split_once(". ").filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())).map(|(_, r)| r))
                .unwrap_or(t);
            next.push(item.to_string());
        }
    }
    (state, next)
}

/// inbox 항목(0.2.35 inbox 계승, §8.9). DB가 원본이고 `projects/<p>/inbox/<id>.md`는 투영이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct InboxItem {
    /// `<UTC 시각>-<종류>-<대상>-<순번>`
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

/// routine 하나(`projects/<p>/routines/<name>.md`, 0.2.35 계승)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoutineView {
    pub name: String,
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
}

/// 병합 대기열 한 칸(§8.8). M1-3은 순번과 사건만 두고 단계 실행은 M3에서 한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeEntry {
    pub project: String,
    pub thread: String,
    /// 1부터. 1이 맨 앞(지금 병합할 차례)이다.
    pub position: u32,
    pub title: String,
    pub machine: String,
    pub branch: Option<String>,
    #[ts(type = "number")]
    pub enqueued_at_ms: u64,
}

/// 병합 단계 하나의 결과(§8.24). 단계 이름은 `확인`·`fetch`·`원격 일치`·`병합`·`tree 같음`·`push`·`사용자 checkout`·`정리`다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeStep {
    pub name: String,
    pub ok: bool,
    /// 사람이 읽는 설명(실패면 까닭)
    pub note: String,
    #[ts(type = "number")]
    pub ms: u64,
}

/// agent가 병합 단계를 돌린 결과. `commit`은 push한 병합 commit이다(push까지 갔을 때만).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct LandOutcome {
    pub ok: bool,
    pub steps: Vec<MergeStep>,
    pub commit: Option<String>,
}

/// `aaa merge land` 결과
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MergeLandView {
    pub project: String,
    pub thread: String,
    pub ok: bool,
    pub steps: Vec<MergeStep>,
    pub commit: Option<String>,
    /// 「main 이동」을 대기열 메시지로 알린 thread
    pub notified: Vec<String>,
    /// 병합 뒤 대기열
    pub queue: Vec<MergeEntry>,
}

/// 결정 기록 한 건(`projects/<p>/decisions.md`의 `## <번호> · <시각>[ · <번호>번 대체]` 묶음)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DecisionView {
    pub num: u32,
    /// 적은 시각(파일에 적힌 글 그대로)
    pub at: String,
    pub text: String,
    /// 이 결정이 대체한 번호
    pub replaces: Option<u32>,
    /// 나중 결정이 이 결정을 대체했다
    pub replaced_by: Option<u32>,
}

/// decisions.md를 읽는다. 사람이 고친 파일도 읽게 `## <번호>`로 시작하는 제목만 본다.
pub fn parse_decisions(md: &str) -> Vec<DecisionView> {
    let mut out: Vec<DecisionView> = Vec::new();
    for line in md.lines() {
        if let Some(h) = line.strip_prefix("## ") {
            let mut parts = h.split('·').map(str::trim);
            if let Some(num) = parts.next().and_then(|n| n.trim_end_matches('.').parse::<u32>().ok()) {
                let at = parts.next().unwrap_or_default().to_string();
                let replaces = parts.find_map(|p| p.strip_suffix("번 대체").and_then(|n| n.trim().parse().ok()));
                out.push(DecisionView { num, at, text: String::new(), replaces, replaced_by: None });
                continue;
            }
        }
        if let Some(d) = out.last_mut() {
            d.text.push_str(line);
            d.text.push('\n');
        }
    }
    for d in &mut out {
        d.text = d.text.trim().to_string();
    }
    let pairs: Vec<(u32, u32)> = out.iter().filter_map(|d| d.replaces.map(|r| (r, d.num))).collect();
    for (old, by) in pairs {
        if let Some(d) = out.iter_mut().find(|d| d.num == old) {
            d.replaced_by = Some(by);
        }
    }
    out
}

/// decisions.md 끝에 붙일 한 건
pub fn render_decision(d: &DecisionView) -> String {
    let rep = d.replaces.map(|r| format!(" · {r}번 대체")).unwrap_or_default();
    format!("\n## {} · {}{rep}\n\n{}\n", d.num, d.at, d.text.trim())
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

/// 보고 상태 태그가 사람의 일을 뜻하는가
pub fn waits_on_user(state: Option<&str>) -> bool {
    matches!(state, Some("사용자 작업 필요") | Some("결정 필요"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 응답·사건 enum 태그(`kind`·`ev`)와 안에 든 구조체 필드 이름이 겹치면 읽을 수 없다
    #[test]
    fn tagged_views_round_trip() {
        use crate::{FrontEvent, FrontResponse};
        let q = QuestionView {
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
    fn decisions_parse_and_render() {
        let mut md = String::from("# 결정 기록\n\n머리말\n");
        md.push_str(&render_decision(&DecisionView {
            num: 1,
            at: "2026-10-16 12:00Z".into(),
            text: "A로 한다".into(),
            replaces: None,
            replaced_by: None,
        }));
        md.push_str("\n## 2. · 사람이 쓴 시각\n\n여러 줄\n- 둘째 줄\n");
        md.push_str(&render_decision(&DecisionView {
            num: 3,
            at: "2026-10-16 12:05Z".into(),
            text: "B로 바꾼다".into(),
            replaces: Some(1),
            replaced_by: None,
        }));
        let d = parse_decisions(&md);
        assert_eq!(d.iter().map(|d| d.num).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!((d[0].text.as_str(), d[0].replaced_by), ("A로 한다", Some(3)));
        assert_eq!((d[1].at.as_str(), d[1].text.as_str()), ("사람이 쓴 시각", "여러 줄\n- 둘째 줄"));
        assert_eq!(d[2].replaces, Some(1));
        assert!(parse_decisions("# 결정 기록\n\n## 메모\n").is_empty());
    }

    #[test]
    fn names_and_slug() {
        assert!(valid_project("aaa-dev_1") && !valid_project("../x") && !valid_project("A") && !valid_project(""));
        assert!(valid_thread("t-0001") && valid_thread("t-12345") && !valid_thread("t-1") && !valid_thread("t-00a1"));
        assert_eq!(slug("Fix the Login bug!"), "fix-the-login-bug");
        assert_eq!(slug("한글 제목"), "");
        assert_eq!(slug("M1-3 Thread: 시작"), "m1-3-thread");
    }

    #[test]
    fn report_tag_and_next() {
        let r = "[사용자 작업 필요]\n\n## Report\n본문\n## Next\n- 인증서 만들기\n* 결과 알려 주기\n1. 셋째\n\n## Remember\n- 이건 아님\n";
        let (s, n) = parse_report(r);
        assert_eq!(s.as_deref(), Some("사용자 작업 필요"));
        assert_eq!(n, ["인증서 만들기", "결과 알려 주기", "셋째"]);
        assert!(waits_on_user(s.as_deref()));
        assert_eq!(parse_report("그냥 글\n").0, None);
        assert!(!waits_on_user(Some("완료")));
    }
}

/// TASKS.md의 task 한 줄(0.2.35 형식). aaa는 읽기만 한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskView {
    /// 속한 `## ` 목록 이름
    pub list: String,
    pub title: String,
    /// 괄호 안 owner 원문(`me`, `claude`, `codex@mac-b`, 사람 이름 등)
    pub owner: Option<String>,
    /// ` · t-0001`로 붙은 thread
    pub thread: Option<String>,
    /// task 줄 아래 들여쓴 메모
    pub notes: Vec<String>,
    pub done: bool,
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
    /// 사후 평가자(지정했을 때만)
    #[serde(default)]
    pub evaluator: Option<Box<EvaluatorView>>,
}

// ───────────────────────── 사후 평가자(M3, §8.13) ─────────────────────────

/// 사후 평가자 상태. 평가자 thread는 thread 목록에 섞지 않고 이것으로 따로 본다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvaluatorView {
    pub project: String,
    pub thread: ThreadView,
    /// 세션이 돌고 있다(아니면 `thread restart`로 다시 띄운다)
    pub running: bool,
    /// 마지막으로 넘긴 사건 seq
    #[ts(type = "number")]
    pub cursor: u64,
    /// 아직 넘기지 않은 사건 수(평가자 자신의 사건 제외)
    pub pending: u32,
    /// 넘긴 사건 묶음 수
    pub batches: u32,
    #[ts(type = "number | null")]
    pub last_batch_ms: Option<u64>,
    /// 받은 평가 기록 수
    pub evaluations: u32,
    /// 채택·보류를 정하지 않은 개선안 수
    pub undecided: u32,
}

/// 평가자가 쓰는 평가 기록(`library/evaluations/<이름>.json`). host가 검사해 받는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvaluationDoc {
    /// 평가한 thread ID
    #[serde(default)]
    pub targets: Vec<String>,
    /// 근거로 읽은 사건 묶음 파일 이름
    #[serde(default)]
    pub batches: Vec<String>,
    /// 흐름 효율 판정 한두 줄
    pub summary: String,
    /// 단계별 시간
    #[serde(default)]
    pub stages: Vec<EvalStage>,
    /// 재작업(횟수·원인)
    #[serde(default)]
    pub rework: Vec<String>,
    /// 막힌 곳(기다린 것과 그 시간)
    #[serde(default)]
    pub blocked: Vec<String>,
    #[serde(default)]
    pub tool_issues: Vec<String>,
    /// 결함을 누가 어느 단계에서 잡았나
    #[serde(default)]
    pub defects: Vec<EvalDefect>,
    #[serde(default)]
    pub proposals: Vec<Proposal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvalStage {
    pub thread: String,
    pub stage: String,
    /// 걸린 시간
    #[serde(default)]
    pub minutes: Option<Minutes>,
}

/// 분. 평가자가 쓴 소수는 반올림해 받는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
pub struct Minutes(pub u32);

impl<'de> Deserialize<'de> for Minutes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let m = f64::deserialize(d)?;
        if !(0.0..=1e6).contains(&m) {
            return Err(serde::de::Error::custom(format!("minutes 범위 밖: {m}")));
        }
        Ok(Minutes(m.round() as u32))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvalDefect {
    pub what: String,
    /// 잡은 쪽(사람·coordinator·thread·CI·E2E …)
    #[serde(default)]
    pub found_by: String,
    #[serde(default)]
    pub stage: String,
}

/// 개선안 하나. `decision`은 host가 채운다(평가자가 쓴 값은 버린다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Proposal {
    pub id: String,
    pub text: String,
    /// 예상 효과
    #[serde(default)]
    pub effect: String,
    /// 대상 도구
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub decision: Option<ProposalDecision>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProposalDecision {
    pub adopted: bool,
    pub by: String,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub note: Option<String>,
    /// 채택이면 남긴 결정 기록 번호
    pub decision_num: Option<u32>,
}

/// 받은 평가 기록 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EvaluationView {
    pub project: String,
    /// 평가자가 정한 파일 이름(프로젝트 안에서 하나)
    pub id: String,
    /// 평가자 thread ID
    pub evaluator: String,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    /// 사람이 읽는 본문(프로젝트 폴더 기준 `evaluations/<id>.md`)
    pub file: String,
    pub doc: EvaluationDoc,
}

/// 평가 기록 이름(파일 이름): 영문·숫자·`-`·`_`, 64자까지
pub fn valid_evaluation_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}
