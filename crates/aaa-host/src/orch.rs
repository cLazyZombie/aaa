//! 오케스트레이션(M1-3a, §8): 프로젝트 폴더, thread 시작·brief 전달·보고 회수·ack·resolve, 흐름 사건.
//!
//! - host는 git을 실행하지 않는다. worktree·작업 폴더는 그 머신의 agent가 만들고(`ThreadPrepare`), host는 기록과 사본만 갖는다.
//! - brief.md 글은 brief_text.rs, 안내 줄 보내기(준비 판정·화면 확인 뒤 최대 3번)는 brief.rs가 맡는다.
//! - report.md hash가 바뀌면 agent에서 본문과 library를 받아 `threads/<id>.md`·`library/<id>/`에 원자적으로 둔다.
//! - 흐름 사건은 프로젝트별 순번으로 DB에 쌓고, 연결된 front에 알린다. 구독자는 cursor부터 다시 읽는다.

mod projects;
pub use projects::*;
mod profiles;
mod restart;
mod start;
#[cfg(test)]
use restart::*;
mod report;
mod resolve;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use aaa_proto::{
    AgentActivity, AgentRequest, AgentResponse, AgentStatus, BriefState, CodexSpeed, ErrorBody, ErrorCode, FlowEvent,
    FrontResponse, Launch, ProjectView, Role, SpawnSpec, ThreadGroup, ThreadKind, ThreadStatus, ThreadView, next_lines,
    valid_project, valid_thread,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::{Host, err};

#[cfg(test)]
pub(crate) type StartAdmittedHook = Arc<dyn Fn(&str) + Send + Sync>;

/// Codex의 마지막 CLI 설정 덮어쓰기를 읽는다. `--` 뒤는 prompt 인자다.
fn codex_launch_speed(argv: &[String]) -> Option<CodexSpeed> {
    let mut tier = None;
    let mut args = argv.iter().skip(1).map(String::as_str).take_while(|arg| *arg != "--");
    while let Some(arg) = args.next() {
        let assignment = match arg {
            "-c" | "--config" => args.next(),
            _ => arg.strip_prefix("--config=").or_else(|| arg.strip_prefix("-c").map(|s| s.trim_start_matches('='))),
        };
        if let Some((key, raw)) = assignment.and_then(|arg| arg.split_once('='))
            && key.trim() == "service_tier"
        {
            // Codex는 TOML로 읽을 수 없는 계층 값을 그대로 문자열로 사용한다.
            let raw = raw.trim();
            let parsed = toml::from_str::<toml::Table>(&format!("service_tier = {raw}")).ok();
            let value = parsed.as_ref().and_then(|t| t.get("service_tier")).and_then(toml::Value::as_str);
            tier = match value.unwrap_or(raw) {
                "ultrafast" => Some(CodexSpeed::Ultrafast),
                "fast" | "priority" => Some(CodexSpeed::Fast),
                "standard" | "default" | "auto" | "flex" => Some(CodexSpeed::Standard),
                _ => None,
            };
        }
    }
    tier
}

impl crate::PaneRt {
    pub(crate) fn thread_speed(&self) -> Option<String> {
        if !self.info.state.is_running() || self.agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Exited) {
            return None;
        }
        let speed = self.codex_speed.map(|(speed, _)| speed).or_else(|| {
            if crate::adopt::harness_of(self.info.harness.as_deref(), &self.info.argv) == Some("codex") {
                codex_launch_speed(&self.info.argv)
            } else {
                None
            }
        });
        match speed {
            Some(speed) => speed.badge().map(str::to_owned),
            None => self.agent.as_ref()?.model_observation.as_ref()?.speed.clone(),
        }
    }
}

/// host가 저장하는 thread 기록(DB `threads.data` JSON). 필드를 더해도 이전 기록을 읽게 기본값을 둔다.
fn old_launch_unknown() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ThreadRec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_allocation: Option<crate::account_pool::Allocation>,
    /// Spawn 전 영속화. 응답 유실 때 새 기동을 막는다.
    #[serde(default = "old_launch_unknown")]
    pub launch_sent: bool,
    #[serde(default)]
    pub launch_existing_slot: bool,
    /// 제거된 기능의 원본 데이터는 변환 전까지 그대로 보존한다. 실행에는 쓰지 않는다.
    #[serde(flatten, deserialize_with = "without_pr_state")]
    pub legacy: std::collections::BTreeMap<String, serde_json::Value>,
    pub project: String,
    pub id: String,
    pub title: String,
    pub machine_id: String,
    pub profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<aaa_proto::TaskReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// 이름 변경 전 시작한 작업의 표시 이름. 인증·배정은 account의 현재 이름으로 찾는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_label: Option<String>,
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub kind: ThreadKind,
    #[serde(default)]
    pub holds: Vec<String>,
    #[serde(default)]
    pub started_by: Option<String>,
    #[serde(default)]
    pub over_limit: Option<aaa_proto::ThreadLimitOverride>,
    /// 보류 요청·기록(park.rs). 정리를 맡겼으면 `parked_at_ms`가 없다. 이어 하면 지운다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub park: Option<aaa_proto::ThreadPark>,
    /// brief.md 안내 대신 칠 한 줄(보류를 풀고 같은 대화를 이을 때). 화면에서 받았는지도 이 줄로 본다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_line: Option<String>,
    /// 다른 머신으로 옮기며 남긴 앞 자리(park.rs). 지우지 않았고, 그 머신으로 돌아가면 다시 쓴다
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub left_places: Vec<crate::park::LeftPlace>,
    /// 기획 대화 thread면 그 기획 branch `<저장소>/<주제>`(design_chat.rs, t-1894 2-2). 저장소 머신에 띄우고
    /// 할 일·실행 한도·coordinator 요약·자동 끝내기에서 뺀다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub design: Option<String>,
    #[serde(default)]
    pub automation: Option<String>,
    #[serde(default)]
    pub automation_run: Option<crate::automation_thread::Run>,
    pub repo: Option<String>,
    pub base: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub work_dir: Option<String>,
    #[serde(default)]
    pub pane: Option<String>,
    pub status: ThreadStatus,
    pub brief: BriefState,
    /// brief 줄을 치거나 Enter를 보낸 수(herdr-projects `brief_attempts`). 0보다 크면 입력창에 줄이 남았을 수 있어
    /// 다시 보내기 전에 화면을 본다(brief.rs).
    #[serde(default)]
    pub brief_attempts: u32,
    /// brief를 보냈다고 확인한 때. 그 뒤 잠시(host.toml `[projects] delivered_working_secs`) 새 보고가 오기 전까지 작업 중이다
    #[serde(default)]
    pub delivered_at_ms: Option<u64>,
    /// 첫 brief 전송 직전의 보고 hash. 받음 확인보다 먼저 회수한 새 보고도 유예를 끝낸다.
    /// 재전송·늦은 화면 확인은 이 기준을 바꾸지 않는다.
    #[serde(default)]
    pub delivered_report: Option<String>,
    #[serde(default)]
    pub launch: Option<Launch>,
    #[serde(default)]
    pub report_hash: Option<String>,
    /// agent가 알렸으나 아직 회수하지 못한 본문 판.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_report_hash: Option<String>,
    /// 원본 판 식별자와 별개로, 저장 전에 인증을 가린 본문 사본의 무결성을 확인한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_copy_hash: Option<String>,
    /// 검증해 회수한 보고와 같은 갱신에서 저장한다. 옛 기록은 host 시작 때 사본으로 보충한다.
    #[serde(default)]
    pub report_stage: Option<aaa_proto::ThreadStage>,
    #[serde(default)]
    pub acked_hash: Option<String>,
    #[serde(default)]
    pub next: Vec<String>,
    /// 지금 보고의 `## Questions`(plan §8.329). 사람을 기다리는 표지의 보고일 때만 읽고, 새 보고마다 답을 비운다
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<aaa_proto::ThreadQuestion>,
    /// 보고 문법 누락·글자 불일치. 질문이 전부 빠져도 경고 카드가 남는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_warning: Option<String>,
    /// 같은 보고의 경고 전달 기록. 재기동 뒤에도 중복 경고를 만들지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_warning_sent: Option<String>,
    /// 근거(그림·영상·보고서 링크)가 질문 칸 밖에만 있다는 진단. 형식 경고와 달리 답 연결·닫힘 판단에 쓰지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_evidence: Option<aaa_proto::QuestionEvidenceGap>,
    /// 사람을 기다리는 보고가 가리키는데 host 사본에 없는 library 파일(`library/` 뒤 경로). library만 새로 받아도 다시 본다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub question_missing: Vec<String>,
    /// 그 질문에 받은 답(answer.rs)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question_answer: Option<aaa_proto::QuestionAnswerView>,
    #[serde(default)]
    pub copy: Option<String>,
    /// 연속 실패 수와 다음 자동 회수 시각. 성공하면 지우고, 두 번째 실패에 시스템 경고를 한 번 낸다.
    #[serde(default)]
    pub copy_failures: u32,
    #[serde(default)]
    pub copy_retry_at_ms: u64,
    /// 마지막 사본에서 옮기지 못한 library 파일과 까닭(「새 보고」 항목에 붙인다)
    #[serde(default)]
    pub copy_notes: Vec<String>,
    /// ticker가 마지막으로 본 묶음(herdr-projects `last_group`). 바뀌면 전이 항목을 낸다(transitions.rs). None은 아직 보지 않았다.
    #[serde(default)]
    pub last_group: Option<ThreadGroup>,
    /// 「새 보고」 항목을 낸 보고 hash(herdr-projects `last_review_item_hash`). 같은 보고로 두 번 내지 않는다.
    #[serde(default)]
    pub review_item_hash: Option<String>,
    /// 묶음이 마지막으로 바뀐 시각(herdr-projects `last_state_change`). auto-resolve가 잰다
    #[serde(default)]
    pub group_changed_ms: Option<u64>,
    /// 보고 hash가 마지막으로 바뀐 시각(herdr-projects `last_report_change`)
    #[serde(default)]
    pub report_changed_ms: Option<u64>,
    #[serde(default)]
    pub error: Option<String>,
    /// resolve 때 남긴 worktree·branch(`project sweep`이 다시 정리한다). None은 모른다(이 필드 전에 끝난 thread)
    #[serde(default)]
    pub leftover: Option<crate::lifecycle::Leftover>,
    /// resolve의 의도. 정리 응답을 저장하기 전에는 지우지 않는다(끊기면 같은 resolve로 이어간다).
    #[serde(default)]
    pub resolve_pending: Option<ResolveOpts>,
    /// 끝난 thread의 이어진 task 판정(할 일 T3, task_clean.rs). 이 칸 전에 끝난 thread는 `why`가 없어 판정하지 않는다
    #[serde(default)]
    pub task_clean: TaskClean,
    /// 이어진 할 일의 ULID(자동 병합 조회, plan §8.530). 이 칸 전에 띄운 thread는 없어 할 일 목록의 연결로 찾는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    /// `--from-task`로 넘길 task 첨부(plan §8.327): 작업 폴더 `attachments/`에 쓸 이름과 host 원본. 세션을 띄울 때마다 쓴다
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<crate::task_attach::TaskFile>,
    #[serde(default)]
    pub prepare_started_ms: Option<u64>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl ThreadRec {
    pub(crate) fn independent_review_profile(&self) -> Option<&str> {
        let review = self.review.as_ref()?;
        let profile = review.profile.as_deref().unwrap_or(&self.profile);
        (profile != self.profile || review.account.is_some()).then_some(profile)
    }

    pub(crate) fn protects_report_copy(&self) -> bool {
        self.status != ThreadStatus::Resolved || self.resolve_pending.is_some()
    }

    pub(crate) fn holds_slot(&self) -> bool {
        matches!(self.status, ThreadStatus::Open | ThreadStatus::Starting) || self.resolve_pending.is_some()
    }

    /// 대화 전용 thread(기획 대화): 할 일·실행 한도·coordinator 요약·자동 끝내기·알림에서 빼고 쉬면 조용히 보류한다
    pub(crate) fn chat_only(&self) -> bool {
        self.design.is_some()
    }

    pub(crate) fn holds_worker_slot(&self) -> bool {
        self.role == Role::Worker
            && self.automation.is_none()
            && !self.chat_only()
            && self.holds_slot()
            && self.execution_stage() != Some(aaa_proto::ThreadStage::MergeReady)
    }

    pub(crate) fn execution_stage(&self) -> Option<aaa_proto::ThreadStage> {
        (self.role == Role::Worker
            && self.report_hash.is_some()
            && (self.status == ThreadStatus::Open || self.resolve_pending.is_some()))
        .then_some(self.report_stage)
        .flatten()
    }

    /// 로컬 profile의 공식 인증 env도 agent에서 받은 뒤 저장하기 전에 가린다.
    fn remember_auth_env(&self) {
        if let Some(launch) = &self.launch {
            for (key, value) in &launch.env {
                if !value.contains("********") {
                    aaa_common::redact::remember_env(key, value);
                }
            }
        }
    }

    /// 막 기록하는 thread(시작 중). 번호는 DB가 붙인다
    pub(crate) fn starting(
        project: &str,
        title: &str,
        machine_id: &str,
        profile: &str,
        role: Role,
        kind: ThreadKind,
        now: u64,
    ) -> ThreadRec {
        ThreadRec {
            account_allocation: None,
            legacy: Default::default(),
            project: project.to_string(),
            id: String::new(),
            title: title.to_string(),
            machine_id: machine_id.to_string(),
            profile: profile.to_string(),
            review: None,
            account: None,
            account_label: None,
            role,
            kind,
            holds: vec![],
            started_by: None,
            over_limit: None,
            park: None,
            brief_line: None,
            left_places: Vec::new(),
            design: None,
            automation: None,
            automation_run: None,
            repo: None,
            base: None,
            branch: None,
            cwd: None,
            work_dir: None,
            pane: None,
            launch_sent: false,
            launch_existing_slot: false,
            status: ThreadStatus::Starting,
            brief: BriefState::Pending,
            brief_attempts: 0,
            delivered_at_ms: None,
            delivered_report: None,
            launch: None,
            report_hash: None,
            pending_report_hash: None,
            report_copy_hash: None,
            report_stage: None,
            acked_hash: None,
            next: vec![],
            questions: Vec::new(),
            question_answer: None,
            question_warning: None,
            question_warning_sent: None,
            question_evidence: None,
            question_missing: Vec::new(),
            copy: None,
            copy_failures: 0,
            copy_retry_at_ms: 0,
            copy_notes: vec![],
            // 시작 중은 작업 중이다(`thread_group` 2번 줄). 처음 셈을 기다리지 않고 적어, 첫 셈 전에 온 보고도 새 보고다.
            last_group: Some(ThreadGroup::Working),
            review_item_hash: None,
            group_changed_ms: Some(now),
            report_changed_ms: None,
            error: None,
            leftover: None,
            resolve_pending: None,
            task_clean: TaskClean::default(),
            task_id: None,
            attachments: Vec::new(),
            prepare_started_ms: Some(now),
            created_at_ms: now,
            updated_at_ms: now,
        }
    }
}

/// 다른 폐지 기능의 보존 자료는 유지하되 PR의 현재 추적 상태는 다시 저장하지 않는다.
fn without_pr_state<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<std::collections::BTreeMap<String, serde_json::Value>, D::Error> {
    let mut fields = std::collections::BTreeMap::<String, serde_json::Value>::deserialize(deserializer)?;
    for name in ["pr", "pr_state", "pr_review", "merged_seen_ms"] {
        fields.remove(name);
    }
    Ok(fields)
}

/// 왜 끝내나: 직접 요청 또는 설정한 쉼 기간이 지났다
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolveWhy {
    Manual,
    Auto { days: Option<u32> },
}

impl ResolveWhy {
    /// 기록에 남기는 이름(`TaskClean.why`)
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ResolveWhy::Manual => "manual",
            ResolveWhy::Auto { .. } => "auto",
        }
    }
}

/// 끝난 thread가 이어진 task를 어떻게 정리하나(할 일 T3, plan §8.316, t-0469 §4 P2). resolve를 시작할 때 `why`·`discard`를 적고,
/// ticker가 한 번 판정한 뒤 `judged_ms`를 찍는다. 다시 열면 지운다(다음 끝남을 다시 판정한다)
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TaskClean {
    /// 끝난 까닭(`ResolveWhy::as_str`)
    #[serde(default)]
    pub why: Option<String>,
    /// 마지막 사본 없이·집에 오지 않은 것을 버리고 끝냈다
    #[serde(default)]
    pub discard: bool,
    #[serde(default)]
    pub judged_ms: Option<u64>,
    /// repo 머신 agent가 실제 origin/main의 조상으로 확인한 커밋.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_main: Option<String>,
    /// 정리 요청을 보낸 뒤 branch가 이미 사라졌을 때 원격 main 포함을 다시 확인할 커밋.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup_branch_head: Option<String>,
    /// 완료로 판정해 지운 task 제목(되살린 task의 「확인 필요」 까닭을 가른다)
    #[serde(default)]
    pub cleaned: Vec<String>,
    /// 하위 task가 모두 끝나 이 thread를 이어 「확인 필요」로 올린 상위 task 제목
    #[serde(default)]
    pub parents: Vec<String>,
    #[serde(default)]
    pub followups: Vec<aaa_proto::ThreadFollowup>,
    /// 같은 줄을 이전 미처리 항목과 합친 판도 재수집 때 다시 만들지 않는다.
    #[serde(default)]
    pub followup_revisions: Vec<String>,
}

/// `thread resolve`의 고르기(herdr-projects `ResolveArgs`). 저절로 끝낼 때는 모두 끈 것이다(`resolve_after_copy`)
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct ResolveOpts {
    pub keep_worktree: bool,
    /// 마지막 사본 없이 끝낸다(worktree는 남긴다)
    pub skip_copy: bool,
    /// 집에 다 오지 않았어도 worktree를 지운다
    pub discard_uncopied: bool,
}

/// thread 시작 인자
pub(crate) struct StartOrigin {
    pub usage_minimum: [Option<f64>; 2],
    pub key: String,
    pub rule: String,
    pub run: crate::automation_thread::Run,
    pub admission: crate::db::StartAdmission,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct StartArgs {
    /// 자동으로 고른 workspace. 계정 배정 대기 뒤 실제 기동에도 현재 허용을 다시 확인한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) automatic_workspace: Option<String>,
    /// 일반 기동의 원본 할 일. 자동화는 비우며, 옛 풀 대기 기록도 읽는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) start_task: Option<aaa_proto::TaskView>,
    pub(crate) pool: Option<crate::account_pool::PoolRequest>,
    pub over_limit: Option<aaa_proto::ThreadLimitOverride>,
    pub(crate) reservation: Option<(String, String)>,
    /// 머신 독점 예약을 무시한 시작(누가·사용자 지시 근거, `reservation_override`). 없으면 예약이 막는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) machine_override: Option<String>,
    pub account: Option<String>,
    pub holds: Vec<String>,
    pub project: String,
    pub title: String,
    pub machine: Option<String>,
    pub profile: String,
    pub kind: ThreadKind,
    pub repo: Option<String>,
    pub base: Option<String>,
    pub task: String,
    pub role: Role,
    /// 할 일 글이 `attachments/<이름>`으로 가리키는 task 첨부(`--from-task`만 채운다, plan §8.327)
    pub attachments: Vec<crate::task_attach::TaskFile>,
    /// 기획 대화 thread(design_chat.rs, `<저장소>/<주제>`): 할 일을 남기지 않고 실행 한도에 세지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) design: Option<String>,
}

/// `project create`의 옵션(herdr-projects `new`)
#[derive(Debug, Default)]
pub(crate) struct ProjectOptions {
    pub goal: String,
    /// `PATH` 또는 `PATH@MACHINE`
    pub repos: Vec<String>,
    pub thread_profile: Option<String>,
    pub coordinator_profile: Option<String>,
}

/// project.toml 읽고 고쳐 쓰기를 한 번에 하나씩(두 `project set`이 서로의 값을 지우지 않게)
static PROJECT_TOML: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 프로젝트의 AGENTS.md(coordinator 안내)와 그 표지. 글은 `guide`에 있다(매 턴 규칙과 `aaa guide <주제>`, plan §8.491).
pub(crate) use crate::guide::{GUIDE_MARK, coordinator_guide};

pub(crate) fn bad(msg: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, msg)
}

pub(crate) fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}

/// `Starting`이 이보다 길면 확인 필요(herdr-projects `STARTING_TIMEOUT_SECS`)
const STARTING_TIMEOUT_MS: u64 = 300_000;
/// 확인창이 이보다 오래 떠 있으면 확인 필요. 금방 답한 확인창은 보이지 않는다(`BLOCKED_DEBOUNCE_SECS`).
const BLOCKED_DEBOUNCE_MS: u64 = 30_000;
/// brief를 아직 못 보냈는데 harness가 입력을 받을 상태가 아닌 채 이만큼 지나면 확인 필요(`NOT_READY_SECS`)
const NOT_READY_MS: u64 = 60_000;
/// 진행 보고가 「작업 중」을 정하는 시간(herdr-projects progress `ACTIVITY_TTL_MS`)
const ACTIVITY_TTL_MS: u64 = 300_000;
// brief를 보낸 뒤 작업 중으로 치는 시간은 host.toml `[projects] delivered_working_secs`(기본 15초)다. herdr-projects ticker는
// 「안내를 보낸 tick은 작업 중」(`delivered`)이고 다음 tick(`TICK` 15초)에야 묶음을 센다: 방금 일을 받은 agent는 아직 쉼으로
// 읽히기 때문이다. aaa는 사건마다 세므로 그 tick을 시간으로 옮겼다. 그 사이 새 보고가 오면 더 기다리지 않는다.
/// agent가 사람을 기다린다고 알리는 진행 보고 문구(`aaa report --activity 'Waiting for you'`, herdr-projects `WAITING`)
pub(crate) const WAITING: &str = "Waiting for you";

/// thread 묶음. 위에서부터 처음 맞는 줄이 이긴다. herdr-projects `thread::group()`(src/thread.rs)을 옮겼다.
/// prompt_pending은 brief `Pending`, brief_stuck은 `Unknown`(aaa는 다시 보내지 않는다), harness 상태는 pane agent 활동,
/// blocked는 `Question`이다. 보고 사본 실패는 시스템 오류이며 harness 상태와 분리한다.
/// 보고 내용(첫 줄 태그)으로는 묶음을 정하지 않는다(D9, plan §8.78).
/// 진행을 보고하는 agent의 새 보고는 `Done`일 때만 검토 대기다(5번 줄, plan §8.85).
pub(crate) fn thread_group(r: &ThreadRec, pane_exists: bool, agent: Option<&AgentStatus>, now_ms: u64) -> ThreadGroup {
    let delivered_ms = aaa_config::ProjectsSection::default().delivered_working_secs * 1000;
    thread_group_with(r, pane_exists, agent, now_ms, delivered_ms, STARTING_TIMEOUT_MS)
}

/// `thread_group`과 같고, brief 뒤 작업 중으로 치는 시간(밀리초)을 설정에서 받는다(`thread_view`)
pub(crate) fn thread_group_with(
    r: &ThreadRec,
    pane_exists: bool,
    agent: Option<&AgentStatus>,
    now_ms: u64,
    delivered_ms: u64,
    starting_ms: u64,
) -> ThreadGroup {
    use AgentActivity as A;
    let state = agent.map(|a| a.activity);
    let state_ms = agent.map_or(0, |a| now_ms.saturating_sub(a.since_ms));
    let has_report = r.report_hash.is_some();
    let prompt_pending = r.brief != BriefState::Delivered;
    // herdr-projects `Live::self_waiting`·`self_working`
    let reported = agent.filter(|a| a.progress_at_ms.is_some());
    let self_waiting = reported.is_some_and(|a| a.progress.as_deref() == Some(WAITING)) && state != Some(A::Working);
    let self_working = reported.is_some_and(|a| {
        a.percent != Some(100)
            && a.progress.as_deref() != Some(WAITING)
            && now_ms.saturating_sub(a.progress_at_ms.unwrap_or(0)) < ACTIVITY_TTL_MS
    });
    // 1
    if r.resolve_pending.is_some() {
        return if r.error.as_deref().is_some_and(|e| e.starts_with("정리 미완료")) {
            ThreadGroup::WaitingOnYou
        } else {
            ThreadGroup::Working
        };
    }
    if r.status == ThreadStatus::Resolved {
        return ThreadGroup::Resolved;
    }
    if r.status == ThreadStatus::Parked {
        return ThreadGroup::Parked;
    }
    // 2
    if r.status == ThreadStatus::Starting {
        return if now_ms.saturating_sub(r.prepare_started_ms.unwrap_or(r.created_at_ms)) < starting_ms {
            ThreadGroup::Working
        } else {
            ThreadGroup::WaitingOnYou
        };
    }
    // 3: 시작 실패, 닫힌 pane, 확인창에 걸린 시작. 보고를 쓴 뒤 닫힌 pane은 끝난 일이다.
    let ready = matches!(state, Some(A::Started | A::Idle));
    let stuck_launch =
        prompt_pending && (r.brief == BriefState::Unknown || (state.is_some() && !ready && state_ms >= NOT_READY_MS));
    if r.status == ThreadStatus::Failed || (!pane_exists && !has_report) || stuck_launch {
        return ThreadGroup::WaitingOnYou;
    }
    // 4: 확인창이 떠 있거나, agent가 사람을 기다린다고 알렸다
    if (state == Some(A::Question) && state_ms >= BLOCKED_DEBOUNCE_MS) || self_waiting {
        return ThreadGroup::WaitingOnYou;
    }
    // 사본을 못 읽어도 마지막으로 알던 묶음을 보존한다. 실제 작업·확인창 사건은 위·아래에서 그대로 센다.
    // 옛 버전이 copy failed로 만든 WaitingOnYou는 다시 live 상태로 셈해 오표시를 회복한다.
    if r.copy.as_deref() == Some("failed")
        && state != Some(A::Working)
        && !self_working
        && let Some(group) = r.last_group.filter(|g| *g != ThreadGroup::WaitingOnYou)
    {
        return group;
    }
    // 5: 새 보고. harness가 작업 중이거나 진행 보고가 미완료이면 중간 보고로 본다.
    let unfinished = pane_exists && reported.is_some_and(|a| a.percent != Some(100));
    let new_report = has_report && r.report_hash != r.acked_hash;
    if new_report && !prompt_pending && state != Some(A::Working) && !unfinished {
        return ThreadGroup::ReadyForReview;
    }
    // 6: harness나 진행 보고로 작업 중. brief 전송 뒤 새 보고가 없으면 받음 확인 뒤 유예 동안 작업 중이다.
    let just_delivered = pane_exists
        && r.delivered_at_ms.is_some_and(|t| now_ms.saturating_sub(t) < delivered_ms)
        && r.report_hash == r.delivered_report;
    if matches!(state, Some(A::Working | A::Question)) || prompt_pending || self_working || just_delivered {
        return ThreadGroup::Working;
    }
    // 7
    ThreadGroup::Idle
}

impl Host {
    pub(crate) fn load(&self, project: &str, id: &str) -> Result<ThreadRec, ErrorBody> {
        if !valid_thread(id) {
            return Err(bad(format!("thread ID 형식이 아니다: {id}")));
        }
        let json = self
            .db
            .thread_get(project, id)
            .map_err(internal)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("thread가 없다: {project}/{id}")))?;
        serde_json::from_str(&json).map_err(internal)
    }

    /// 그 pane의 thread 사건을 다시 낸다(그 thread의 agent 상태가 바뀌었다)
    pub(crate) fn thread_changed(&self, pane: &str) {
        if let Some(rec) = self.thread_of_pane(pane) {
            aaa_common::diagnostic::ignored(
                self.thread_events.send(self.thread_view(&rec)),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
    }

    pub(crate) fn save(&self, rec: &mut ThreadRec) {
        rec.updated_at_ms = aaa_common::util::now_ms();
        rec.remember_auth_env();
        let json = match aaa_common::redact::json(rec.clone())
            .and_then(|clean| serde_json::to_string(&clean).map_err(|_| aaa_common::redact::Error::Serialize))
        {
            Ok(json) => json,
            Err(_) => return, // 원문 저장·발행 없이 거부한다. 이유는 가림 경계가 기록한다.
        };
        match self.db.thread_save(&rec.project, &rec.id, rec.status.as_str(), &json) {
            // 답한 것 카드의 고치기 가능 여부가 thread 상태를 따른다: 열린 화면이 다시 읽게 한다(t-2117)
            Ok(true) => self.asks_bump(),
            Ok(false) => {}
            Err(e) => tracing::error!(thread = %rec.id, "thread 기록 저장 실패: {e:#}"),
        }
        aaa_common::diagnostic::ignored(
            self.thread_events.send(self.thread_view(rec)),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
    }

    /// 최신 기록을 다시 읽어 고치고 저장한다. 읽은 뒤 agent 응답 등을 기다린 쪽은 이것으로 쓴다
    /// (기다리는 동안 다른 쪽이 고친 필드, 예: 평가자 cursor를 옛 값으로 덮지 않게).
    pub(crate) fn update_rec(
        &self,
        project: &str,
        id: &str,
        f: impl FnOnce(&mut ThreadRec),
    ) -> Result<ThreadRec, ErrorBody> {
        let _guard = self.rec_lock.lock().unwrap();
        let mut r = self.load(project, id)?;
        f(&mut r);
        // 이 성공 뒤 Kill·깨움을 보내는 호출자는 실제 DB 확정을 필요로 한다.
        r.updated_at_ms = aaa_common::util::now_ms();
        r.remember_auth_env();
        let clean = aaa_common::redact::json(r.clone()).map_err(|_| internal("thread 기록 가림 거부"))?;
        let json = serde_json::to_string(&clean)
            .map_err(|e| aaa_common::diagnostic::context(e, internal("thread 기록 직렬화 거부")))?;
        if self.db.thread_save(project, id, r.status.as_str(), &json).map_err(internal)? {
            self.asks_bump();
        }
        aaa_common::diagnostic::ignored(
            self.thread_events.send(self.thread_view(&r)),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
        Ok(r)
    }

    pub(crate) fn machine_name(&self, mid: &str) -> String {
        self.inner.lock().unwrap().machines.get(mid).map(|m| m.name.clone()).unwrap_or_else(|| mid.to_string())
    }

    /// thread pane이 살아 있는가와 그 agent 상태(묶음 셈의 입력)
    pub(crate) fn thread_live(&self, r: &ThreadRec) -> (bool, Option<AgentStatus>) {
        let agent = r.pane.as_deref().and_then(|p| self.pane_agent(p));
        let pane_exists = r
            .pane
            .as_deref()
            .is_some_and(|p| self.pane_view_of(p, aaa_proto::DeviceRole::Local).is_some_and(|v| v.state.is_running()));
        (pane_exists, agent)
    }

    pub(crate) fn thread_view(&self, r: &ThreadRec) -> ThreadView {
        let (pane_exists, agent) = self.thread_live(r);
        let agy_limit = self.agy_limit_for(r, aaa_common::util::now_ms());
        let delivered_ms = self.cfg.projects.delivered_working_secs * 1000;
        let group = thread_group_with(
            r,
            pane_exists,
            agent.as_ref(),
            aaa_common::util::now_ms(),
            delivered_ms,
            (self.cfg.host.thread_prepare_timeout_secs + 60) * 1000,
        );
        // Hello 상태 재생 중 agent=None은 아직 모른다는 뜻이다. 알려 둔 묶음을 쉼으로 바꾸지 않는다.
        let group = if r.status == ThreadStatus::Open
            && agent.is_none()
            && self.restoring.lock().unwrap().contains(&r.machine_id)
        {
            r.last_group.unwrap_or(group)
        } else {
            group
        };
        let (responsibility, speed) = r
            .pane
            .as_ref()
            .and_then(|id| {
                self.inner.lock().unwrap().panes.get(id).map(|p| (p.info.responsibility.clone(), p.thread_speed()))
            })
            .unwrap_or_default();
        ThreadView {
            responsibility,
            speed,
            account_selection: r.account_allocation.as_ref().map(|a| a.selection.clone()),
            design: r.design.clone(),
            over_limit: r.over_limit.clone(),
            park: r.park.clone().map(Box::new),
            auto_merge: self.thread_auto_merge(r),
            project: r.project.clone(),
            id: r.id.clone(),
            title: r.title.clone(),
            machine: self.machine_name(&r.machine_id),
            profile: r.profile.clone(),
            review: r.review.clone(),
            account: r.account_label.clone().or_else(|| r.account.clone()),
            role: r.role,
            thread_kind: r.kind,
            holds: r.holds.clone(),
            automation: r.automation.clone(),
            automation_title: r.automation.as_deref().map(|n| self.automation_title(&r.project, n)),
            automation_trigger: r.automation_run.as_ref().and_then(|run| run.trigger.clone()),
            status: r.status,
            cleanup_pending: r.resolve_pending.is_some().then_some(true),
            group,
            brief: r.brief,
            pane: r.pane.clone(),
            cwd: r.cwd.clone(),
            work_dir: r.work_dir.clone(),
            branch: r.branch.clone(),
            report_hash: r.report_hash.clone(),
            report_id: self.thread_report_id(&r.project, &r.id, r.report_hash.as_deref()),
            report_attention: crate::report_mark::pending_attention(r, self.report_attention(r)),
            stage: r.execution_stage(),
            acked_hash: r.acked_hash.clone(),
            next: r.next.clone(),
            followups: r.task_clean.followups.clone(),
            agent: agent.map(Box::new),
            copy: r.copy.clone(),
            error: r.error.clone(),
            created_at_ms: r.created_at_ms,
            updated_at_ms: r.updated_at_ms,
            warnings: Vec::new(),
            project_status: self.project_status(&r.project),
            summary: if r.id.is_empty() { None } else { self.summary_mark(&r.project, &r.id) },
            repo: r.repo.clone(),
            limited_until_ms: agy_limit.flatten().or_else(|| self.usage_limit(&r.project, &r.id)),
            limited: agy_limit.map(|_| true),
            questions: r.report_hash.clone().filter(|_| !r.questions.is_empty() || r.question_warning.is_some()).map(
                |report_hash| {
                    let items = self
                        .identify_questions(&r.project, "thread", &r.id, &report_hash, &r.questions)
                        .unwrap_or_else(|e| {
                            tracing::error!("질문 ID 발급 실패: {}", e.message);
                            r.questions.clone()
                        });
                    Box::new(aaa_proto::ThreadQuestions {
                        format_warning: crate::report_mark::card_warning(r),
                        review: self.default_review_for(r),
                        draft: self.question_draft(&r.project, &items),
                        files_base: Some(crate::asks::question_files_base(&r.project, &r.id, &report_hash)),
                        report_hash: report_hash.clone(),
                        items,
                        answer: r.question_answer.clone(),
                    })
                },
            ),
        }
    }

    /// thread를 띄운 자동화 규칙의 제목(plan t-0475 A1). 줄마다 부르므로 그 규칙 파일의 `title` 칸만 읽는다. 파일이 없어졌거나
    /// 읽지 못하거나 제목이 비면 이름이다(옛 routine 파일에는 제목이 없다)
    pub(crate) fn automation_title(&self, project: &str, name: &str) -> String {
        self.project_dir(project)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .and_then(|dir| {
                crate::automation::project_text(&dir, &format!("automations/{name}.toml"))
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .and_then(|text| {
                text.parse::<toml::Table>()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .and_then(|t| t.get("title").and_then(|v| v.as_str()).map(str::trim).map(str::to_string))
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| name.to_string())
    }

    /// thread 목록. 기본(`role` 없음)은 사후 평가자를 뺀다(§8.13).
    pub(crate) fn threads_of(&self, project: Option<&str>, role: Option<Role>) -> Result<Vec<ThreadView>, ErrorBody> {
        self.thread_views_for(project, role, false)
    }

    /// 열린 질문·배지는 끝난 원장을 읽거나 상세·질문 번호를 만들지 않는다.
    pub(crate) fn unresolved_threads_of(
        &self,
        project: Option<&str>,
        role: Option<Role>,
    ) -> Result<Vec<ThreadView>, ErrorBody> {
        self.thread_views_for(project, role, true)
    }

    fn thread_views_for(
        &self,
        project: Option<&str>,
        role: Option<Role>,
        unresolved: bool,
    ) -> Result<Vec<ThreadView>, ErrorBody> {
        let rows = if unresolved {
            self.db.thread_page(
                project,
                None,
                &aaa_proto::ThreadListQuery { open: true, ..Default::default() },
                None,
                u32::MAX,
            )
        } else {
            self.db.threads(project)
        }
        .map_err(internal)?;
        Ok(rows
            .iter()
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .filter(|r| match role {
                None => true,
                Some(want) => r.role == want,
            })
            .map(|r| self.thread_view(&r))
            .collect())
    }

    /// 목록은 개수와 직렬화 바이트를 함께 제한한다. 다음 커서는 마지막으로 실은 행을 가리킨다.
    pub(crate) fn threads_page(
        &self,
        project: Option<&str>,
        role: Option<Role>,
        query: &aaa_proto::ThreadListQuery,
    ) -> Result<FrontResponse, ErrorBody> {
        if query.open && query.status.is_some() {
            return Err(err(ErrorCode::BadRequest, "open과 status는 함께 쓸 수 없다"));
        }
        let limit = query.limit.unwrap_or(100).min(200);
        if limit == 0 {
            return Err(err(ErrorCode::BadRequest, "thread 목록 limit은 1 이상이어야 한다"));
        }
        let after: Option<(String, String)> = query
            .after
            .as_deref()
            .map(|value| {
                let bytes = hex::decode(value)
                    .map_err(|e| err(ErrorCode::BadRequest, format!("thread 목록 커서 hex 읽기 실패: {e}")))?;
                serde_json::from_slice(&bytes)
                    .map_err(|e| err(ErrorCode::BadRequest, format!("thread 목록 커서 JSON 읽기 실패: {e}")))
            })
            .transpose()?;
        if after.as_ref().is_some_and(|(p, _)| project.is_some_and(|want| want != p)) {
            return Err(err(ErrorCode::BadRequest, "thread 목록 커서의 프로젝트가 다르다"));
        }
        let rows = self
            .db
            .thread_page(project, role.map(|r| r.as_str()), query, after.as_ref(), limit + 1)
            .map_err(internal)?;
        let mut threads: Vec<ThreadView> = Vec::new();
        let mut bytes = 0;
        let mut next_after = None;
        // frame 포장·커서·웹 첨부 주소를 위해 여유를 둔다. MAX_FRAME 자체는 바꾸지 않는다.
        let budget = aaa_proto::MAX_FRAME / 2;
        for row in rows {
            let mut r: ThreadRec = serde_json::from_str(&row).map_err(internal)?;
            if !query.details {
                r.questions.clear();
                r.question_warning = None;
                r.question_evidence = None;
                r.question_missing.clear();
            }
            let view = self.thread_view(&r);
            let size = serde_json::to_vec(&view).map_err(internal)?.len() + 1;
            if threads.len() == limit as usize || bytes + size > budget {
                let Some(last) = threads.last() else {
                    return Err(err(
                        ErrorCode::TooLarge,
                        format!(
                            "thread 목록의 {}/{} 한 항목이 {}바이트로 페이지 한도를 넘는다. thread show로 상세를 읽는다",
                            view.project, view.id, size
                        ),
                    ));
                };
                next_after = Some(hex::encode(serde_json::to_vec(&(&last.project, &last.id)).map_err(internal)?));
                break;
            }
            bytes += size;
            threads.push(view);
        }
        Ok(FrontResponse::Threads { threads, next_after })
    }

    pub(crate) fn thread_show(&self, project: &str, id: &str) -> Result<ThreadView, ErrorBody> {
        if let Some(v) = self.db.report_target(project, id).map_err(internal)? {
            let mut view = self.thread_view(&self.load(project, &v.source)?);
            let dir = self.project_dir(project)?;
            let report = std::fs::read_to_string(dir.join("threads").join(format!("{id}.md"))).map_err(internal)?;
            view.report_id = Some(v.id);
            view.report_hash = Some(v.revision);
            view.next = next_lines(&report);
            view.questions = None;
            view.report_attention = if crate::report_mark::needs_user(&report) {
                Some(aaa_proto::ReportAttention::NeedsUser)
            } else if crate::report_mark::done(crate::report_mark::marker(&report)) {
                Some(aaa_proto::ReportAttention::Completed)
            } else {
                None
            };
            return Ok(view);
        }
        self.load(project, id).map(|r| self.thread_view(&r))
    }

    /// 흐름 사건을 기록하고 알린다
    pub(crate) fn flow(&self, r: &ThreadRec, kind: &str, evidence: Option<String>, detail: impl Into<String>) {
        self.flow_event(FlowEvent {
            repo: None,
            project: r.project.clone(),
            seq: 0,
            at_ms: aaa_common::util::now_ms(),
            boot_id: self.boot_id.clone(),
            kind: kind.to_string(),
            thread: Some(r.id.clone()),
            actor: (kind == "thread_started").then(|| r.started_by.clone()).flatten(),
            role: Some(r.role),
            profile: Some(r.profile.clone()),
            machine: Some(self.machine_name(&r.machine_id)),
            evidence,
            detail: detail.into(),
        });
    }

    /// thread에 묶이지 않은 프로젝트 사건(평가 채택·보류 등)
    pub(crate) fn flow_project(&self, project: &str, kind: &str, evidence: Option<String>, detail: impl Into<String>) {
        self.flow_event(FlowEvent {
            repo: None,
            project: project.to_string(),
            seq: 0,
            at_ms: aaa_common::util::now_ms(),
            boot_id: self.boot_id.clone(),
            kind: kind.to_string(),
            thread: None,
            actor: None,
            role: None,
            profile: None,
            machine: None,
            evidence,
            detail: detail.into(),
        });
    }

    /// 흐름 사건을 저장하고 알린다. 저장(seq를 정함)부터 broadcast까지를 한 잠금 안에서 해 보내는 차례가 seq 차례다: 따로
    /// 하면 동시에 기록된 두 사건 가운데 나중 seq가 먼저 나갈 수 있고, 따라 읽는 쪽(`aaa events --follow`)은 본 것보다 작은
    /// seq를 버려 그 사건을 잃었다(10-18 ci-local m13 `merged_pr_resolves_the_thread`: seq 21 `thread_resolved`가 22 뒤에 나감).
    /// 이 안에서 다시 흐름 사건을 남기면 안 된다(std 잠금이라 멈춘다): inbox·사후 평가자는 사건을 남기지 않는다.
    fn flow_event(&self, mut e: FlowEvent) {
        let kind = e.kind.clone();
        let kind = kind.as_str();
        let _order = match self.flow_order.try_lock() {
            Ok(g) => g,
            Err(source_error) => {
                aaa_common::diagnostic::observe_type(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                blocking(|| {
                    test_flow_waiting(&e);
                    self.flow_order.lock().unwrap_or_else(|p| p.into_inner())
                })
            }
        };
        match self.db.flow_insert(&e) {
            Ok(seq) => {
                e.seq = seq;
                test_flow_hold(&e);
                self.inbox_from_flow(&e);
                aaa_common::diagnostic::ignored(self.flow_events.send(e), "수신자가 끝난 뒤에도 현재 작업은 마친다");
            }
            Err(err) => tracing::error!(kind, "흐름 사건 저장 실패: {err:#}"),
        }
    }

    pub(crate) fn events(
        &self,
        project: Option<&str>,
        after: u64,
        limit: Option<u32>,
        tail: bool,
    ) -> Result<Vec<FlowEvent>, ErrorBody> {
        if let Some(p) = project
            && !valid_project(p)
        {
            return Err(bad("프로젝트 이름 형식이 아니다"));
        }
        let limit = limit.unwrap_or(500).min(5000);
        if tail {
            return self.db.flow_tail(project, after, limit).map_err(internal);
        }
        self.db.flow_after(project, after, limit).map_err(internal)
    }
}

/// 시험에서만(`AAA_TEST_FLOW_HOLD=<Unix socket>`): 저장한 사건을 알리기 전 시험에 H<detail>을 보내고 R을 기다린다.
/// 시간이 지났다고 조용히 풀면 두 사건이 겹치지 않아도 순서 시험이 통과하므로, 제어 연결 실패는 시험 실패다.
fn test_flow_hold(e: &FlowEvent) {
    if std::env::var_os("AAA_TEST_FLOW_HOLD").is_none() {
        return;
    }
    #[cfg(unix)]
    if e.detail.starts_with("hold:") {
        blocking(|| {
            if let Some(mut control) = test_flow_signal(b'H', e) {
                let mut release = [0];
                std::io::Read::read_exact(&mut control, &mut release).expect("flow 시험의 해제 사건");
                assert_eq!(release, [b'R'], "flow 시험의 해제 표지");
            }
        });
    }
    #[cfg(not(unix))]
    let _ = e;
}

/// 막히는 일을 tokio worker에서 할 때 runtime에 알린다(그 worker에 줄 선 작업을 다른 worker가 맡는다). 여러 스레드 runtime
/// 밖(시험·blocking 스레드)에서는 그냥 한다
fn blocking<R>(f: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(h) if h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => tokio::task::block_in_place(f),
        _ => f(),
    }
}

/// 시험에서만: 어떤 사건이 flow_order를 기다리는지 W<detail>로 알린다.
fn test_flow_waiting(e: &FlowEvent) {
    #[cfg(unix)]
    drop(test_flow_signal(b'W', e));
    #[cfg(not(unix))]
    let _ = e;
}

#[cfg(unix)]
fn test_flow_signal(phase: u8, e: &FlowEvent) -> Option<std::os::unix::net::UnixStream> {
    let path = std::env::var_os("AAA_TEST_FLOW_HOLD")?;
    let mut control = std::os::unix::net::UnixStream::connect(path).expect("flow 시험 제어 연결");
    control.set_read_timeout(Some(std::time::Duration::from_secs(30))).unwrap();
    std::io::Write::write_all(&mut control, format!("{}{}\n", phase as char, e.detail).as_bytes())
        .expect("flow 시험 단계 알림");
    Some(control)
}

/// 두 폴더가 같은 상대 경로에 같은 바이트의 파일을 가졌나(library 사본이 바뀌었는지 볼 때). 없는 폴더는 빈 폴더와 다르다.
fn same_tree(a: &Path, b: &Path) -> bool {
    fn files(root: &Path) -> Option<Vec<(PathBuf, Vec<u8>)>> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()?
            {
                let p = e
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()?
                    .path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push((
                        p.strip_prefix(root)
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()?
                            .to_path_buf(),
                        std::fs::read(&p)
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()?,
                    ));
                }
            }
        }
        out.sort();
        Some(out)
    }
    match (files(a), files(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "orch/report_tests.rs"]
mod report_tests;

#[cfg(test)]
mod list_tests;
