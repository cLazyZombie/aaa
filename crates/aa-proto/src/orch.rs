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
    /// `<cwd>/.aa/<project>-<thread>/`(brief.md·report.md·library/)
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

    #[test]
    fn names_and_slug() {
        assert!(valid_project("aa-dev_1") && !valid_project("../x") && !valid_project("A") && !valid_project(""));
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
