//! 오케스트레이션(M1-3a, §8): 프로젝트 폴더, thread 시작·brief 전달·보고 회수·ack·resolve, 흐름 사건.
//!
//! - host는 git을 실행하지 않는다. worktree·작업 폴더는 그 머신의 agent가 만들고(`ThreadPrepare`), host는 기록과 사본만 갖는다.
//! - brief 안내는 화면이 3초 멈춘 뒤 한 번 보낸다. 전달을 확인하지 못하면 `unknown`으로 남기고 **자동으로 다시 보내지 않는다**(§8.5 a).
//! - report.md hash가 바뀌면 agent에서 본문과 library를 받아 `threads/<id>.md`·`library/<id>/`에 원자적으로 둔다.
//! - 흐름 사건은 프로젝트별 순번으로 DB에 쌓고, 연결된 front에 알린다. 구독자는 cursor부터 다시 읽는다.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use aa_proto::{
    AgentRequest, AgentResponse, BriefState, DecisionView, ErrorBody, ErrorCode, FlowEvent, FrontResponse, Launch,
    ProjectView, Role, SpawnSpec, ThreadGroup, ThreadKind, ThreadStatus, ThreadView, parse_decisions, parse_report,
    render_decision, valid_project, valid_thread, waits_on_user,
};
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::{Host, err};

/// host가 저장하는 thread 기록(DB `threads.data` JSON). 필드를 더해도 이전 기록을 읽게 기본값을 둔다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ThreadRec {
    pub project: String,
    pub id: String,
    pub title: String,
    pub machine_id: String,
    pub profile: String,
    #[serde(default)]
    pub role: Role,
    #[serde(default)]
    pub kind: ThreadKind,
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
    #[serde(default)]
    pub launch: Option<Launch>,
    #[serde(default)]
    pub report_hash: Option<String>,
    #[serde(default)]
    pub acked_hash: Option<String>,
    #[serde(default)]
    pub report_state: Option<String>,
    #[serde(default)]
    pub next: Vec<String>,
    #[serde(default)]
    pub copy: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    /// brief에 넣은 결정 기록 번호
    #[serde(default)]
    pub decisions_rev: Option<u32>,
    /// 사후 평가자: 마지막으로 넘긴 사건 seq, 넘긴 묶음 수, 마지막 묶음 시각(§8.13)
    #[serde(default)]
    pub eval_cursor: u64,
    #[serde(default)]
    pub eval_batches: u32,
    #[serde(default)]
    pub eval_last_batch_ms: Option<u64>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// thread 시작 인자
pub struct StartArgs {
    pub project: String,
    pub title: String,
    pub machine: Option<String>,
    pub profile: String,
    pub kind: ThreadKind,
    pub repo: Option<String>,
    pub base: Option<String>,
    pub task: String,
    pub role: Role,
}

/// 새 프로젝트의 AGENTS.md(coordinator 안내). `{p}`는 프로젝트 이름이다.
const COORDINATOR_GUIDE: &str = "# 프로젝트 coordinator

이 폴더를 cwd로 쓰는 agent가 이 프로젝트의 coordinator다. 일은 thread에 맡기고, coordinator는 고르고 확인하고 정리한다.

## 매 턴
- 먼저 `aa context {p}`를 본다: TASKS.md, 열린 thread와 Next 줄, `start_threads`, 맡길 수 있는 owner, inbox 수.
- `[aa inbox] …` 알림을 받으면 `aa inbox list {p}`로 읽고, 처리한 항목은 `aa inbox done {p} <id…>`로 끝낸다.

## thread
- 시작: `start_threads`가 propose면 무엇을 누구에게 맡길지 제안하고 사용자 승인을 기다린다. auto면 바로 시작하고 알린다.
  사용자가 이름을 들어 맡기라고 한 것은 승인이다. aa inbox 알림은 승인이 아니다.
- TASKS.md task로 시작: `aa thread start {p} --from-task \"<제목>\" [--kind dir | --repo <경로>]`. owner가 profile·machine을 정한다.
- 그 밖: `aa thread start {p} --title … --profile … [--machine …] --task-file …`
- 보고 확인: `aa thread show {p} <id>`, `aa thread library {p} <id>`, 다 봤으면 `aa thread ack {p} <id>`, 끝나면 `aa thread resolve {p} <id>`.
- 다음 지시: `aa thread next {p} <id>`로 Next 목록을 보고 `--line N`으로 그 줄을 보낸다. 줄을 더할 때는 `--add \"…\"`.
- 그 밖의 메시지: `aa thread message {p} <id> --text …`. 확인창 질문은 `aa question list`·`aa question answer`.

## TASKS.md(coordinator만 고친다)
- 형식: `- [ ] 제목 (owner)`. owner는 비움(미할당)·`me`(사용자 할 일)·`profile`·`@machine`·`profile@machine`.
- owner를 쓰기 전에 `aa assignable {p} --check <owner>`로 확인한다.
- thread에 맡기면 줄 끝에 ` · <thread id>`를 붙인다. 끝나거나 그만두면 줄을 지운다(메모도 함께).
- 추가·할당·완료 같은 변경은 사용자가 요청할 때만 한다.

## 결정·병합
- 사용자 결정은 `aa decision add {p} --text …`로 남긴다(다음 thread brief에 들어간다).
- 병합 순서는 `aa merge enqueue|list|done {p} <id>`로 정한다.
";

/// 화면이 이만큼 멈춰야 brief 안내를 보낸다
const SETTLE: Duration = Duration::from_secs(3);
/// 화면이 멈추기를 기다리는 상한
const SETTLE_LIMIT: Duration = Duration::from_secs(120);
/// 확인창 질문에 사람이 답하기를 기다리는 상한
const TRUST_LIMIT: Duration = Duration::from_secs(30 * 60);

fn bad(msg: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, msg)
}

fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}

/// 프로젝트의 지금 결정 번호(가장 큰 번호)
fn decisions_rev(dir: &Path) -> Option<u32> {
    parse_decisions(&std::fs::read_to_string(dir.join("decisions.md")).ok()?).iter().map(|d| d.num).max()
}

/// 임시 파일에 쓴 뒤 rename으로 바꾼다
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp-{}", path.file_name().unwrap().to_string_lossy(), aa_common::util::new_id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// 다시 시작한 thread의 brief 맨 앞
const RESTART_NOTE: &str = "\
> 이 thread는 다시 시작했다. 같은 작업 폴더(와 branch)에 이전 시도가 있다.
> 먼저 지금 상태(파일·커밋·report.md)를 확인하고 이어서 한다.

";

/// thread 규칙(brief 앞부분). 보고 형식은 운영 coordinator가 읽는 형식이다.
const THREAD_GUIDE: &str = "\
# thread 규칙

- 이 파일은 aa host가 만든 작업 지시다. 아래 「할 일」을 하고 결과를 보고한다.
- 보고는 `{report}`에 **매번 통째로 다시 쓴다**. coordinator는 이 파일만 읽는다(채팅으로 보고하지 않는다).
  - 첫 줄은 상태 하나다: `[진행 중]`, `[사용자 작업 필요]`, `[결정 필요]`, `[완료]`
  - `## Report`(무엇을 했고 어떻게 확인했나), `## Next`(필수, 한 줄에 행동 하나), 선택 `## Remember`
- 결과물(스크린샷·로그·보고서)은 `{library}`에 둔다. symlink는 복사되지 않는다.
- host는 report.md가 바뀔 때 library를 함께 복사한다. 결과물을 먼저 두고 report.md를 마지막에 쓴다.
- 작업 폴더 밖의 다른 작업 파일·프로세스는 건드리지 않는다.
";

impl Host {
    pub(crate) fn projects_root(&self) -> PathBuf {
        if self.cfg.projects.root.is_empty() {
            aa_config::paths::home().join(".aa").join("projects")
        } else {
            PathBuf::from(&self.cfg.projects.root)
        }
    }

    pub(crate) fn project_dir(&self, name: &str) -> Result<PathBuf, ErrorBody> {
        if !valid_project(name) {
            return Err(bad(format!("프로젝트 이름 형식이 아니다(영문 소문자·숫자·-·_): {name}")));
        }
        let dir = self.projects_root().join(name);
        if !dir.join("PROJECT.md").is_file() {
            return Err(err(ErrorCode::NotFound, format!("프로젝트가 없다: {name}(`aa project create {name}`)")));
        }
        Ok(dir)
    }

    fn project_view(&self, name: &str, dir: &Path) -> ProjectView {
        let threads = self.db.threads(Some(name)).map(|v| v.len() as u32).unwrap_or(0);
        ProjectView { name: name.to_string(), path: dir.to_string_lossy().to_string(), threads }
    }

    /// 프로젝트 폴더를 만든다. PROJECT.md를 마지막에 써서, 중간에 멈춘 폴더는 프로젝트로 치지 않는다.
    pub(crate) fn project_create(&self, name: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        if !valid_project(name) {
            return Err(bad(format!("프로젝트 이름 형식이 아니다(영문 소문자·숫자·-·_): {name}")));
        }
        let dir = self.projects_root().join(name);
        if dir.join("PROJECT.md").exists() {
            return Err(err(ErrorCode::NameTaken, format!("이미 있다: {}", dir.display())));
        }
        let mk = || -> std::io::Result<()> {
            for sub in ["memory", "routines", "threads", "library", "inbox", "uploads"] {
                std::fs::create_dir_all(dir.join(sub))?;
            }
            let put = |f: &str, body: &str| -> std::io::Result<()> {
                if !dir.join(f).exists() {
                    write_atomic(&dir.join(f), body.as_bytes())?;
                }
                Ok(())
            };
            put("MEMORY.md", "# Memory\n")?;
            put("TASKS.md", "# Tasks\n\n## Backlog\n")?;
            put("decisions.md", "# 결정 기록\n")?;
            put("project.toml", "schema = 1\n")?;
            put("AGENTS.md", &COORDINATOR_GUIDE.replace("{p}", name))?;
            put("PROJECT.md", &format!("# {name}\n\n프로젝트 지침을 여기에 쓴다.\n"))
        };
        mk().map_err(internal)?;
        self.db.audit(actor, "project_create", name);
        Ok(FrontResponse::Project(self.project_view(name, &dir)))
    }

    pub(crate) fn projects(&self) -> Vec<ProjectView> {
        let root = self.projects_root();
        let mut out: Vec<ProjectView> = std::fs::read_dir(&root)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                (valid_project(&name) && e.path().join("PROJECT.md").is_file())
                    .then(|| self.project_view(&name, &e.path()))
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

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

    pub(crate) fn save(&self, rec: &mut ThreadRec) {
        rec.updated_at_ms = aa_common::util::now_ms();
        let json = serde_json::to_string(&rec).expect("thread 기록 직렬화");
        if let Err(e) = self.db.thread_save(&rec.project, &rec.id, rec.status.as_str(), &json) {
            tracing::error!(thread = %rec.id, "thread 기록 저장 실패: {e:#}");
        }
        let _ = self.thread_events.send(self.thread_view(rec));
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
        self.save(&mut r);
        Ok(r)
    }

    pub(crate) fn machine_name(&self, mid: &str) -> String {
        self.inner.lock().unwrap().machines.get(mid).map(|m| m.name.clone()).unwrap_or_else(|| mid.to_string())
    }

    pub(crate) fn thread_view(&self, r: &ThreadRec) -> ThreadView {
        let new_report = r.report_hash.is_some() && r.report_hash != r.acked_hash;
        let group = match r.status {
            ThreadStatus::Resolved => ThreadGroup::Resolved,
            ThreadStatus::Failed => ThreadGroup::WaitingOnYou,
            _ if new_report && waits_on_user(r.report_state.as_deref()) => ThreadGroup::WaitingOnYou,
            _ if new_report => ThreadGroup::ReadyForReview,
            _ if r.brief == BriefState::Unknown || r.copy.as_deref() == Some("failed") => ThreadGroup::WaitingOnYou,
            // 열린 thread인데 세션이 끝났다(종료·재부팅): 다시 시작하거나 끝낼지 사람이 정한다
            ThreadStatus::Open
                if r.pane.as_deref().is_some_and(|p| {
                    !self.pane_view_of(p, aa_proto::DeviceRole::Local).is_some_and(|v| v.state.is_running())
                }) =>
            {
                ThreadGroup::WaitingOnYou
            }
            _ => ThreadGroup::Working,
        };
        ThreadView {
            project: r.project.clone(),
            id: r.id.clone(),
            title: r.title.clone(),
            machine: self.machine_name(&r.machine_id),
            profile: r.profile.clone(),
            role: r.role,
            thread_kind: r.kind,
            status: r.status,
            group,
            brief: r.brief,
            pane: r.pane.clone(),
            cwd: r.cwd.clone(),
            work_dir: r.work_dir.clone(),
            branch: r.branch.clone(),
            report_hash: r.report_hash.clone(),
            acked_hash: r.acked_hash.clone(),
            report_state: r.report_state.clone(),
            next: r.next.clone(),
            agent: r.pane.as_deref().and_then(|p| self.pane_agent(p)).map(Box::new),
            decisions_rev: r.decisions_rev,
            decisions_now: self.project_dir(&r.project).ok().and_then(|d| decisions_rev(&d)),
            copy: r.copy.clone(),
            error: r.error.clone(),
            created_at_ms: r.created_at_ms,
            updated_at_ms: r.updated_at_ms,
        }
    }

    /// thread 목록. 기본(`role` 없음)은 사후 평가자를 뺀다(§8.13).
    pub(crate) fn threads_of(&self, project: Option<&str>, role: Option<Role>) -> Result<Vec<ThreadView>, ErrorBody> {
        let rows = self.db.threads(project).map_err(internal)?;
        Ok(rows
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| match role {
                None => r.role != Role::Evaluator,
                Some(want) => r.role == want,
            })
            .map(|r| self.thread_view(&r))
            .collect())
    }

    pub(crate) fn thread_show(&self, project: &str, id: &str) -> Result<ThreadView, ErrorBody> {
        self.load(project, id).map(|r| self.thread_view(&r))
    }

    /// 흐름 사건을 기록하고 알린다
    pub(crate) fn flow(&self, r: &ThreadRec, kind: &str, evidence: Option<String>, detail: impl Into<String>) {
        self.flow_event(FlowEvent {
            project: r.project.clone(),
            seq: 0,
            at_ms: aa_common::util::now_ms(),
            boot_id: self.boot_id.clone(),
            kind: kind.to_string(),
            thread: Some(r.id.clone()),
            role: Some(r.role),
            profile: Some(r.profile.clone()),
            machine: Some(self.machine_name(&r.machine_id)),
            evidence,
            detail: detail.into(),
        });
    }

    /// thread에 묶이지 않은 프로젝트 사건(결정 기록 등)
    pub(crate) fn flow_project(&self, project: &str, kind: &str, evidence: Option<String>, detail: impl Into<String>) {
        self.flow_event(FlowEvent {
            project: project.to_string(),
            seq: 0,
            at_ms: aa_common::util::now_ms(),
            boot_id: self.boot_id.clone(),
            kind: kind.to_string(),
            thread: None,
            role: None,
            profile: None,
            machine: None,
            evidence,
            detail: detail.into(),
        });
    }

    fn flow_event(&self, mut e: FlowEvent) {
        let kind = e.kind.clone();
        let kind = kind.as_str();
        match self.db.flow_insert(&e) {
            Ok(seq) => {
                e.seq = seq;
                self.inbox_from_flow(&e);
                self.evaluator_on_flow(&e);
                let _ = self.flow_events.send(e);
            }
            Err(err) => tracing::error!(kind, "흐름 사건 저장 실패: {err:#}"),
        }
    }

    pub(crate) fn events(
        &self,
        project: Option<&str>,
        after: u64,
        limit: Option<u32>,
    ) -> Result<Vec<FlowEvent>, ErrorBody> {
        if let Some(p) = project
            && !valid_project(p)
        {
            return Err(bad("프로젝트 이름 형식이 아니다"));
        }
        self.db.flow_after(project, after, limit.unwrap_or(500).min(5000)).map_err(internal)
    }

    pub(crate) async fn profiles(&self, machine: Option<&str>) -> Result<FrontResponse, ErrorBody> {
        let mid = self.resolve_machine(machine)?;
        let conn = self.live_conn(&mid)?;
        match conn.request(AgentRequest::Profiles).await? {
            AgentResponse::Profiles { profiles } => {
                Ok(FrontResponse::Profiles { machine: self.machine_name(&mid), profiles })
            }
            other => Err(internal(format!("예상하지 못한 응답: {other:?}"))),
        }
    }

    pub(crate) fn profile_allowed(&self, name: &str) -> bool {
        self.cfg.projects.allow_profiles.iter().any(|a| a == "*" || a == name)
    }

    /// brief.md: thread 규칙 + 프로젝트 지침 + 결정 기록 + memory(상한) + 할 일
    fn brief(&self, dir: &Path, r: &ThreadRec, task: &str) -> String {
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_default();
        let work = format!(".aa/{}-{}", r.project, r.id);
        let mut out = format!(
            "# 프로젝트\n\n- 프로젝트: {}\n- thread: {} — {}\n- 보고: `{work}/report.md`\n- 결과물: `{work}/library/`\n\n",
            r.project, r.id, r.title
        );
        if r.role == Role::Evaluator {
            out.push_str(&crate::evaluator::EVALUATOR_GUIDE.replace("{work}", &work));
        } else {
            out.push_str(
                &THREAD_GUIDE
                    .replace("{report}", &format!("{work}/report.md"))
                    .replace("{library}", &format!("{work}/library/")),
            );
        }
        out.push_str("\n# 프로젝트 지침\n\n");
        out.push_str(read("PROJECT.md").trim());
        out.push_str("\n\n# 결정 기록\n\n");
        out.push_str(read("decisions.md").trim());
        // memory: MEMORY.md 다음 memory/*.md(이름순, symlink 제외). 상한을 넘는 파일은 이름만 적는다.
        out.push_str("\n\n# 프로젝트 memory\n\n");
        let cap = self.cfg.projects.brief_memory_chars;
        let mut used = 0usize;
        let mut files = vec![dir.join("MEMORY.md")];
        let mut mem: Vec<PathBuf> = std::fs::read_dir(dir.join("memory"))
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .collect();
        mem.sort();
        files.extend(mem);
        for f in files {
            let Ok(text) = std::fs::read_to_string(&f) else { continue };
            let n = text.chars().count();
            let rel = f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().to_string();
            if used + n > cap {
                out.push_str(&format!("- `{rel}`: 상한({cap}자)을 넘어 넣지 않았다. 필요하면 직접 읽는다.\n"));
            } else {
                used += n;
                out.push_str(&format!("## {rel}\n\n{}\n\n", text.trim()));
            }
        }
        out.push_str("\n# 할 일\n\n");
        out.push_str(task.trim());
        out.push('\n');
        out
    }

    /// thread 시작: 기록 → agent가 자리 만들기·profile 해석 → 세션 시작 → 보고 감시 → (배경) brief 안내
    pub(crate) fn decisions(&self, project: &str) -> Result<Vec<DecisionView>, ErrorBody> {
        let dir = self.project_dir(project)?;
        Ok(parse_decisions(&std::fs::read_to_string(dir.join("decisions.md")).unwrap_or_default()))
    }

    /// 결정을 decisions.md 끝에 덧붙인다. 번호는 지금 가장 큰 번호 + 1이다.
    pub(crate) fn decision_add(
        &self,
        project: &str,
        text: &str,
        replaces: Option<u32>,
        actor: &str,
    ) -> Result<DecisionView, ErrorBody> {
        let dir = self.project_dir(project)?;
        if text.trim().is_empty() {
            return Err(bad("결정 내용이 비었다"));
        }
        let _guard = self.decision_lock.lock().unwrap();
        let path = dir.join("decisions.md");
        let mut md = std::fs::read_to_string(&path).unwrap_or_else(|_| "# 결정 기록\n".into());
        let have = parse_decisions(&md);
        if let Some(r) = replaces
            && !have.iter().any(|d| d.num == r)
        {
            return Err(bad(format!("대체할 결정이 없다: {r}번")));
        }
        let d = DecisionView {
            num: have.iter().map(|d| d.num).max().unwrap_or(0) + 1,
            at: aa_common::util::utc_text(aa_common::util::now_ms()),
            text: text.trim().to_string(),
            replaces,
            replaced_by: None,
        };
        if !md.ends_with('\n') {
            md.push('\n');
        }
        md.push_str(&render_decision(&d));
        write_atomic(&path, md.as_bytes()).map_err(internal)?;
        self.db.audit(actor, "decision_add", &format!("{project} {}", d.num));
        let rep = replaces.map(|r| format!(" ({r}번 대체)")).unwrap_or_default();
        let first = d.text.lines().next().unwrap_or_default().chars().take(80).collect::<String>();
        self.flow_project(project, "decision_added", Some("decisions.md".into()), format!("{}번{rep}: {first}", d.num));
        Ok(d)
    }

    pub(crate) async fn thread_start(self: &Arc<Self>, a: StartArgs, actor: &str) -> Result<ThreadView, ErrorBody> {
        let dir = self.project_dir(&a.project)?;
        if a.title.trim().is_empty() {
            return Err(bad("제목이 비었다"));
        }
        if a.task.trim().is_empty() {
            return Err(bad("할 일이 비었다"));
        }
        if !self.profile_allowed(&a.profile) {
            return Err(err(
                ErrorCode::Forbidden,
                format!("host.toml [projects] allow_profiles에 없는 profile: {}", a.profile),
            ));
        }
        if a.kind == ThreadKind::Worktree && a.repo.is_none() {
            return Err(bad("worktree thread에는 --repo(그 머신의 repo 경로)가 필요하다. repo 없이 하려면 --kind dir"));
        }
        let mid = self.resolve_machine(a.machine.as_deref())?;
        let conn = self.live_conn(&mid)?;
        self.check_capacity(&mid, None)?;
        self.check_health(&mid)?;
        let now = aa_common::util::now_ms();
        let mut rec = ThreadRec {
            project: a.project.clone(),
            id: String::new(),
            title: a.title.trim().to_string(),
            machine_id: mid.clone(),
            profile: a.profile.clone(),
            role: a.role,
            kind: a.kind,
            repo: a.repo.clone(),
            base: a.base.clone(),
            branch: None,
            cwd: None,
            work_dir: None,
            pane: None,
            status: ThreadStatus::Starting,
            brief: BriefState::Pending,
            launch: None,
            report_hash: None,
            acked_hash: None,
            report_state: None,
            next: vec![],
            copy: None,
            error: None,
            decisions_rev: None,
            eval_cursor: 0,
            eval_batches: 0,
            eval_last_batch_ms: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        let id = self
            .db
            .thread_insert_next(&a.project, |id| {
                let mut r = rec.clone();
                r.id = id.to_string();
                serde_json::to_string(&r).unwrap()
            })
            .map_err(internal)?;
        rec.id = id.clone();
        self.db.audit(actor, "thread_start", &format!("{}/{id} {} @{}", a.project, a.profile, self.machine_name(&mid)));
        write_atomic(&dir.join("threads").join(format!("{id}.task.md")), a.task.as_bytes()).map_err(internal)?;
        let brief = self.brief(&dir, &rec, &a.task);
        rec.decisions_rev = decisions_rev(&dir);
        self.flow(&rec, "thread_started", Some(format!("threads/{id}.task.md")), rec.title.clone());

        let fail = |me: &Arc<Host>, rec: &mut ThreadRec, e: ErrorBody| -> ErrorBody {
            rec.status = ThreadStatus::Failed;
            rec.error = Some(e.message.clone());
            me.save(rec);
            me.flow(rec, "thread_failed", None, e.message.clone());
            e
        };
        let prep = aa_proto::ThreadPrepare {
            project: a.project.clone(),
            thread: id.clone(),
            title: rec.title.clone(),
            kind: a.kind,
            repo: a.repo.clone(),
            base: a.base.clone(),
            profile: a.profile.clone(),
            brief,
        };
        let ready = match conn.request(AgentRequest::ThreadPrepare(prep)).await {
            Ok(AgentResponse::ThreadReady(r)) => r,
            Ok(other) => return Err(fail(self, &mut rec, internal(format!("예상하지 못한 응답: {other:?}")))),
            Err(e) => return Err(fail(self, &mut rec, e)),
        };
        rec.cwd = Some(ready.cwd.clone());
        rec.work_dir = Some(ready.work_dir.clone());
        rec.branch = ready.branch.clone();
        rec.base = ready.base.clone();
        rec.launch = Some(ready.launch.clone());
        self.save(&mut rec);

        if let Err(e) = self.launch_session(&mut rec, &ready.launch.argv, actor).await {
            return Err(fail(self, &mut rec, e));
        }
        let me = self.clone();
        let (p, t) = (a.project.clone(), id.clone());
        tokio::spawn(async move { me.deliver_brief(&p, &t).await });
        Ok(self.thread_view(&rec))
    }

    pub(crate) async fn screen_lines(&self, mid: &str, pane: &str) -> Option<Vec<String>> {
        let conn = self.conn_of(mid)?;
        match conn.request(AgentRequest::Screen { session: pane.to_string() }).await {
            Ok(AgentResponse::Screen(s)) => Some(s.lines),
            _ => None,
        }
    }

    /// brief 안내 한 번: 화면이 멈추면 입력하고 Enter. harness가 받았다는 사건(hook이 없으면 화면 변화)으로 전달을 확인한다.
    /// 처음 실행 확인창(폴더 신뢰 등)이 보이면 입력하지 않고 질문으로 올려 사람의 답을 기다린다.
    /// 확인 못 하면 unknown(다시 보내지 않는다).
    pub(crate) async fn deliver_brief(self: &Arc<Self>, project: &str, id: &str) {
        let Ok(mut rec) = self.load(project, id) else { return };
        let (Some(pane), mid) = (rec.pane.clone(), rec.machine_id.clone()) else { return };
        let line = format!(".aa/{project}-{id}/brief.md를 읽고 그대로 하라.");
        let outcome: Result<(), String> = async {
            let mut start = Instant::now();
            let mut last: Option<Vec<String>> = None;
            let mut since = Instant::now();
            // (질문 id, 연 시각). 확인창 하나에 질문은 한 번만 연다.
            let mut trust: Option<(String, Instant)> = None;
            loop {
                // 질문이 열려 있는 동안은 사람을 더 오래 기다린다(답한 뒤부터 다시 잰다)
                let asking = trust.as_ref().is_some_and(|(q, _)| self.questions(false).iter().any(|x| &x.id == q));
                if let Some((_, at)) = trust.as_ref().filter(|_| asking) {
                    if at.elapsed() > TRUST_LIMIT {
                        return Err(
                            "harness 확인창(폴더 신뢰 등)에 답이 없어 입력하지 않았다. 답한 뒤 `aa thread brief`로 보낸다".into(),
                        );
                    }
                    start = Instant::now();
                } else if start.elapsed() > SETTLE_LIMIT {
                    return Err(format!("{}초 동안 화면이 멈추지 않았다", SETTLE_LIMIT.as_secs()));
                }
                if !self.pane_view_of(&pane, aa_proto::DeviceRole::Local).is_some_and(|p| p.state.is_running()) {
                    return Err("세션이 끝났다".into());
                }
                let lines = self.screen_lines(&mid, &pane).await;
                // 처음 실행 확인창(폴더 신뢰 등)에는 입력하지 않고 질문으로 올린다
                if let Some((kind, text)) = lines.as_deref().and_then(crate::harness::modal_of) {
                    if trust.is_none() {
                        trust = Some((self.ask(&pane, kind, text).id, Instant::now()));
                    }
                    since = Instant::now();
                } else if lines != last {
                    // 확인창이 사라졌다(답했거나 터미널에서 직접 골랐다)
                    if asking
                        && let Some(mut q) =
                            self.questions(false).into_iter().find(|x| Some(&x.id) == trust.as_ref().map(|t| &t.0))
                    {
                        q.state = aa_proto::QuestionState::Gone;
                        self.save_question(&q);
                    }
                    last = lines;
                    since = Instant::now();
                } else if last.is_some() && since.elapsed() >= SETTLE {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            match self.send_prompt(&mid, &pane, &line).await? {
                true => Ok(()),
                false => Err("Enter 뒤 받았다는 사건·화면 변화가 없었다".into()),
            }
        }
        .await;
        let Ok(mut fresh) = self.load(project, id) else { return };
        match outcome {
            Ok(()) => {
                fresh.brief = BriefState::Delivered;
                self.save(&mut fresh);
                self.flow(&fresh, "brief_delivered", rec.work_dir.take().map(|w| format!("{w}/brief.md")), line);
            }
            Err(why) => {
                fresh.brief = BriefState::Unknown;
                self.save(&mut fresh);
                self.flow(&fresh, "brief_unknown", None, why);
            }
        }
    }

    /// thread 세션을 띄우고 보고 감시를 건다(시작·다시 시작 공통)
    async fn launch_session(
        self: &Arc<Self>,
        rec: &mut ThreadRec,
        argv: &[String],
        actor: &str,
    ) -> Result<(), ErrorBody> {
        let launch = rec.launch.clone().ok_or_else(|| internal("launch 기록이 없다"))?;
        let mut env = launch.env.clone();
        env.push(("AA_PROJECT".into(), rec.project.clone()));
        env.push(("AA_THREAD".into(), rec.id.clone()));
        let spec = SpawnSpec {
            name: format!("{}/{}", rec.project, rec.id),
            argv: argv.to_vec(),
            cwd: rec.cwd.clone(),
            env,
            cols: 120,
            rows: 32,
            role: Some(rec.role),
        };
        let pane = self.spawn_on(&rec.machine_id, spec, actor).await?;
        *rec = self.update_rec(&rec.project, &rec.id, |r| {
            r.pane = Some(pane);
            r.status = ThreadStatus::Open;
        })?;
        if let (Ok(conn), Some(wd)) = (self.live_conn(&rec.machine_id), rec.work_dir.clone()) {
            let _ = conn
                .request(AgentRequest::ThreadWatch { thread: format!("{}/{}", rec.project, rec.id), work_dir: wd })
                .await;
        }
        Ok(())
    }

    /// thread agent를 멈춘다(Esc 한 번, 0.2.35 `thread stop`과 같다). 세션·작업 폴더는 그대로다.
    pub(crate) async fn thread_stop(&self, project: &str, id: &str, actor: &str) -> Result<ThreadView, ErrorBody> {
        let rec = self.load(project, id)?;
        let pane = rec.pane.clone().ok_or_else(|| bad("thread 세션이 없다"))?;
        self.check_running(&pane)?;
        let conn = self.live_conn(&rec.machine_id)?;
        conn.request(AgentRequest::Input { session: pane, data: aa_proto::B64(b"\x1b".to_vec()) }).await?;
        self.db.audit(actor, "thread_stop", &format!("{project}/{id}"));
        self.flow(&rec, "thread_stopped", None, "Esc");
        Ok(self.thread_view(&rec))
    }

    /// 끝난 thread 세션을 다시 띄운다. 실행 중이면 거부한다(먼저 `aa pane kill`).
    /// - 기본: task(Follow-ups 포함)·프로젝트 지침·결정·memory를 다시 읽어 brief를 새로 쓰고 새 대화로 띄운 뒤 안내한다.
    /// - resume: 저장된 대화를 잇는다(brief 안내 없음). harness 세션 ID를 모르면 거부한다.
    pub(crate) async fn thread_restart(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        resume: bool,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let mut rec = self.load(project, id)?;
        if rec.status != ThreadStatus::Open {
            return Err(bad(format!("열린 thread만 다시 시작한다(지금 {})", rec.status.as_str())));
        }
        if let Some(p) = &rec.pane
            && self.pane_view_of(p, aa_proto::DeviceRole::Local).is_some_and(|v| v.state.is_running())
        {
            return Err(bad(format!("세션이 아직 실행 중이다. 끝내려면 `aa pane kill {p}`")));
        }
        let launch = rec.launch.clone().ok_or_else(|| internal("launch 기록이 없다"))?;
        let conn = self.live_conn(&rec.machine_id)?;
        self.check_capacity(&rec.machine_id, Some((project, id)))?;
        self.check_health(&rec.machine_id)?;
        let argv = if resume {
            let key = rec
                .pane
                .as_deref()
                .and_then(|p| self.pane_agent(p))
                .and_then(|a| a.harness_session)
                .or(launch.session_key.clone());
            if launch.resume_argv.is_empty() {
                return Err(bad("이 harness는 대화를 이어 띄우는 방법이 없다"));
            }
            let needs_key = launch.resume_argv.iter().any(|a| a.contains("{session}"));
            match key {
                Some(k) => launch.resume_argv.iter().map(|a| a.replace("{session}", &k)).collect::<Vec<_>>(),
                None if needs_key => return Err(bad("harness 세션 ID를 모른다. 새로 시작한다(--resume 없이)")),
                None => launch.resume_argv.clone(),
            }
        } else {
            let dir = self.project_dir(project)?;
            let task = std::fs::read_to_string(dir.join("threads").join(format!("{id}.task.md"))).map_err(internal)?;
            let mut brief = self.brief(&dir, &rec, &task);
            brief.insert_str(0, RESTART_NOTE);
            let work_dir = rec.work_dir.clone().ok_or_else(|| internal("작업 폴더 기록이 없다"))?;
            let relaunched = match conn.request(AgentRequest::ThreadRebrief { work_dir, brief, launch }).await? {
                AgentResponse::Relaunch(l) => l,
                other => return Err(internal(format!("예상하지 못한 응답: {other:?}"))),
            };
            let rev = decisions_rev(&dir);
            let launch = relaunched.clone();
            rec = self.update_rec(project, id, |r| {
                r.decisions_rev = rev;
                r.brief = BriefState::Pending;
                r.launch = Some(launch);
            })?;
            relaunched.argv
        };
        self.db.audit(actor, "thread_restart", &format!("{project}/{id} resume={resume}"));
        self.launch_session(&mut rec, &argv, actor).await?;
        self.flow(&rec, "thread_restarted", None, if resume { "resume" } else { "새 대화·brief 다시 씀" });
        if !resume {
            let me = self.clone();
            let (p, t) = (project.to_string(), id.to_string());
            tokio::spawn(async move { me.deliver_brief(&p, &t).await });
        }
        Ok(self.thread_view(&rec))
    }

    /// 사람이 명시적으로 다시 보낸다(자동 재전송은 하지 않는다)
    pub(crate) async fn thread_brief(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let mut rec = self.load(project, id)?;
        if rec.status != ThreadStatus::Open {
            return Err(bad("열린 thread만 brief를 보낼 수 있다"));
        }
        rec.brief = BriefState::Pending;
        self.save(&mut rec);
        self.db.audit(actor, "thread_brief", &format!("{project}/{id}"));
        let me = self.clone();
        let (p, t) = (project.to_string(), id.to_string());
        tokio::spawn(async move { me.deliver_brief(&p, &t).await });
        Ok(self.thread_view(&rec))
    }

    /// agent가 report.md hash 변화를 알렸다
    pub(crate) fn on_report_changed(self: &Arc<Self>, mid: &str, key: &str, hash: Option<String>) {
        let Some((project, id)) = key.split_once('/') else { return };
        let Ok(rec) = self.load(project, id) else { return };
        if rec.machine_id != mid || rec.status != ThreadStatus::Open || hash.is_none() || hash == rec.report_hash {
            return;
        }
        let me = self.clone();
        let (p, t) = (project.to_string(), id.to_string());
        tokio::spawn(async move {
            let _guard = me.collect_lock.lock().await;
            if let Err(e) = me.collect(&p, &t).await {
                tracing::warn!(thread = %t, "보고 사본 실패: {}", e.message);
            }
        });
    }

    /// report.md와 library를 받아 host 프로젝트 폴더에 둔다. 실패하면 hash를 바꾸지 않는다(다음 변화·정리 때 다시 한다).
    async fn collect(self: &Arc<Self>, project: &str, id: &str) -> Result<(), ErrorBody> {
        let rec = self.load(project, id)?;
        let dir = self.project_dir(project)?;
        let work_dir = rec.work_dir.clone().ok_or_else(|| bad("작업 폴더가 없다"))?;
        let conn = self.live_conn(&rec.machine_id)?;
        // Ok(None): 이미 받은 보고와 같다
        type Got = Option<(String, Vec<String>, String, Vec<u8>)>;
        let result: Result<Got, ErrorBody> = async {
            let snap = match conn
                .request(AgentRequest::ReportCollect {
                    work_dir: work_dir.clone(),
                    max_library_bytes: self.cfg.projects.library_copy_mb << 20,
                })
                .await?
            {
                AgentResponse::Report(s) => s,
                other => return Err(internal(format!("예상하지 못한 응답: {other:?}"))),
            };
            if Some(&snap.hash) == rec.report_hash.as_ref() && rec.copy.as_deref() != Some("failed") {
                return Ok(None);
            }
            // library: 임시 폴더에 받고 hash를 확인한 뒤 바꿔 넣는다
            let lib_root = dir.join("library");
            let tmp = lib_root.join(format!(".{id}.tmp-{}", aa_common::util::new_id()));
            let fetch = async {
                std::fs::create_dir_all(&tmp).map_err(internal)?;
                for f in &snap.files {
                    let target = tmp.join(&f.path);
                    if !Path::new(&f.path).components().all(|c| matches!(c, std::path::Component::Normal(_))) {
                        return Err(bad(format!("이상한 library 경로: {}", f.path)));
                    }
                    std::fs::create_dir_all(target.parent().unwrap()).map_err(internal)?;
                    let mut out = Vec::with_capacity(f.size as usize);
                    loop {
                        let r = conn
                            .request(AgentRequest::FileRead {
                                work_dir: work_dir.clone(),
                                path: f.path.clone(),
                                offset: out.len() as u64,
                                len: 512 * 1024,
                            })
                            .await?;
                        let AgentResponse::FileChunk { data, eof } = r else {
                            return Err(internal("예상하지 못한 응답"));
                        };
                        out.extend_from_slice(&data.0);
                        if eof || data.0.is_empty() {
                            break;
                        }
                    }
                    if hex::encode(sha2::Sha256::digest(&out)) != f.sha256 {
                        return Err(internal(format!("{}: 받는 동안 바뀌었다(hash 다름)", f.path)));
                    }
                    std::fs::write(&target, &out).map_err(internal)?;
                }
                Ok(())
            }
            .await;
            if let Err(e) = fetch {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(e);
            }
            let dest = lib_root.join(id);
            if dest.exists() {
                std::fs::remove_dir_all(&dest).map_err(internal)?;
            }
            std::fs::rename(&tmp, &dest).map_err(internal)?;
            crate::library::write_manifest(&dir, id, &snap).map_err(internal)?;
            let skipped: Vec<String> = snap.skipped.iter().map(|s| format!("{}: {}", s.path, s.reason)).collect();
            let copy = if skipped.is_empty() { "complete" } else { "partial" };
            Ok(Some((snap.hash, skipped, copy.to_string(), snap.report.0)))
        }
        .await;
        match result {
            Ok(None) => Ok(()),
            Ok(Some((hash, skipped, copy, report))) => {
                write_atomic(&dir.join("threads").join(format!("{id}.md")), &report).map_err(internal)?;
                let text = String::from_utf8_lossy(&report);
                let (state, next) = parse_report(&text);
                let rec = self.update_rec(project, id, |r| {
                    r.report_hash = Some(hash);
                    r.report_state = state.clone();
                    r.next = next;
                    r.copy = Some(copy.clone());
                })?;
                let mut detail = format!("[{}] 사본 {copy}", state.as_deref().unwrap_or("상태 없음"));
                if !skipped.is_empty() {
                    detail.push_str(&format!(", 건너뜀: {}", skipped.join("; ")));
                }
                self.flow(&rec, "report_ready", Some(format!("threads/{id}.md")), detail);
                if rec.role == Role::Evaluator {
                    self.evaluator_ingest(&rec);
                }
                Ok(())
            }
            Err(e) => {
                let rec = self.update_rec(project, id, |r| r.copy = Some("failed".into()))?;
                self.flow(&rec, "copy_failed", None, e.message.clone());
                Err(e)
            }
        }
    }

    pub(crate) fn thread_ack(&self, project: &str, id: &str, actor: &str) -> Result<ThreadView, ErrorBody> {
        let mut rec = self.load(project, id)?;
        let Some(h) = rec.report_hash.clone() else { return Err(bad("아직 보고가 없다")) };
        if rec.acked_hash.as_ref() != Some(&h) {
            rec.acked_hash = Some(h);
            self.save(&mut rec);
            self.db.audit(actor, "thread_ack", &format!("{project}/{id}"));
            self.flow(
                &rec,
                "review_acked",
                Some(format!("threads/{id}.md")),
                rec.report_state.clone().unwrap_or_default(),
            );
        }
        Ok(self.thread_view(&rec))
    }

    /// 끝낸다: 마지막 사본(실패면 거부) → 세션 종료 → 감시 해제 → worktree 정리(force 없음, 병합된 branch만 삭제)
    pub(crate) async fn thread_resolve(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        keep_worktree: bool,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let rec = self.load(project, id)?;
        if rec.status == ThreadStatus::Resolved {
            return Ok(self.thread_view(&rec));
        }
        let conn = self.live_conn(&rec.machine_id)?;
        if let Some(w) = &rec.work_dir {
            let has_report = matches!(
                conn.request(AgentRequest::ReportCollect { work_dir: w.clone(), max_library_bytes: 0 }).await,
                Ok(AgentResponse::Report(_))
            );
            if has_report {
                let _guard = self.collect_lock.lock().await;
                self.collect(project, id)
                    .await
                    .map_err(|e| bad(format!("마지막 사본이 실패해 resolve하지 않는다: {}", e.message)))?;
            }
        }
        let rec = self.load(project, id)?;
        if let Some(pane) = &rec.pane
            && self.pane_view_of(pane, aa_proto::DeviceRole::Local).is_some_and(|p| p.state.is_running())
        {
            let _ = conn.request(AgentRequest::Kill { session: pane.clone() }).await;
        }
        let _ = conn.request(AgentRequest::ThreadUnwatch { thread: format!("{project}/{id}") }).await;
        let mut notes = Vec::new();
        if let Some(cwd) = &rec.cwd {
            let repo = if rec.kind == ThreadKind::Worktree { rec.repo.clone() } else { None };
            match conn
                .request(AgentRequest::ThreadCleanup {
                    repo,
                    cwd: cwd.clone(),
                    branch: rec.branch.clone(),
                    base: rec.base.clone(),
                    keep_worktree,
                })
                .await
            {
                Ok(AgentResponse::Cleanup(c)) => {
                    notes.push(format!(
                        "worktree {}, branch {}",
                        if c.worktree_removed { "삭제" } else { "유지" },
                        if c.branch_deleted { "삭제" } else { "유지" }
                    ));
                    notes.extend(c.notes);
                }
                Ok(_) => {}
                Err(e) => notes.push(format!("정리 실패: {}", e.message)),
            }
        }
        let rec = self.update_rec(project, id, |r| r.status = ThreadStatus::Resolved)?;
        self.db.audit(actor, "thread_resolve", &format!("{project}/{id}"));
        self.merge_forget(&rec);
        self.flow(
            &rec,
            "thread_resolved",
            rec.report_hash.as_ref().map(|_| format!("threads/{id}.md")),
            notes.join("; "),
        );
        Ok(self.thread_view(&rec))
    }

    /// agent가 다시 붙었다: 그 머신의 열린 thread 보고 감시를 다시 건다(놓친 변화는 등록 즉시 hash로 알 수 있다)
    pub(crate) fn rewatch(self: &Arc<Self>, mid: String) {
        self.evaluator_reconnected(&mid);
        let me = self.clone();
        tokio::spawn(async move {
            let Some(conn) = me.conn_of(&mid) else { return };
            let Ok(rows) = me.db.threads(None) else { return };
            for r in rows.iter().filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok()) {
                if r.machine_id == mid
                    && r.status == ThreadStatus::Open
                    && let Some(w) = r.work_dir
                {
                    let _ = conn
                        .request(AgentRequest::ThreadWatch { thread: format!("{}/{}", r.project, r.id), work_dir: w })
                        .await;
                }
            }
        });
    }
}
