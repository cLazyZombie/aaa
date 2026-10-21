//! coordinator 일 흐름(부록 A, 0.2.35 의미): `context`, `thread next`, `assignable`, `thread start --from-task`.
//!
//! - TASKS.md가 원본이다. 완료는 줄을 지워 표시한다. 웹 할 일 화면은 `tasks`(TaskBoard)로 읽고 ticker가 파일 변화를 `tasks` 사건으로
//!   알린다(할 일 보기 T1, plan §8.306). 웹 조작 기기·그 프로젝트 coordinator·aaa 밖 터미널은 `task_edit`로 고치고(바뀐 블록만, 블록 hash
//!   충돌 감지, 할 일 편집 T2, plan §8.311), 모든 변경을 `task_log`에 남긴다. 파일 직접 편집도 되며 ticker가 「파일 직접」으로 적는다.
//! - `start_threads`(safety, propose·auto)는 coordinator가 따르는 안내 값이다. aaa가 강제하지 않는다(0.2.35와 같다).
//! - 권한을 건너뛰는 실행 인자는 사람만 바꾸는 host.toml `[safety.*]`(yolo·thread_agent_args)와 그 머신 agent.toml의 profile
//!   `args`에 둔다(coordinator가 바꿀 수 없다, plan §8.163).

use crate::orch::internal;

use std::path::Path;
use std::sync::Arc;

use aaa_proto::{
    ErrorBody, ErrorCode, FrontResponse, MessageMode, OwnerChoice, ProjectContext, TaskAction, TaskBoard, TaskConflict,
    TaskLogFilter, TaskNotesFile, TaskOp, TaskThread, TaskView, ThreadGroup, ThreadStatus,
};

use crate::file_rev::{self, Edit};
use crate::orch::StartArgs;
use crate::task_edit::{self, Fail, Removed};
use crate::tasks::{self, Owner};
use crate::{Host, err, front};

fn bad(m: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, m)
}

fn next_file(dir: &Path, id: &str) -> std::path::PathBuf {
    dir.join("threads").join(format!("{id}.next.md"))
}

/// `- `·`* `·`N. ` 표시를 뗀 줄
fn item(l: &str) -> Option<String> {
    let t = l.trim();
    if t.is_empty() {
        return None;
    }
    let s = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| t.split_once(". ").filter(|(n, _)| n.chars().all(|c| c.is_ascii_digit())).map(|(_, r)| r))
        .unwrap_or(t);
    Some(s.to_string())
}

impl Host {
    /// 보고서 `## Next` 줄 + `threads/<id>.next.md`
    fn next_lines(&self, dir: &Path, report_next: &[String], id: &str) -> Vec<String> {
        let mut lines = report_next.to_vec();
        if let Ok(t) = std::fs::read_to_string(next_file(dir, id)) {
            lines.extend(t.lines().filter_map(item));
        }
        lines
    }

    pub(crate) async fn thread_next(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        line: Option<u32>,
        add: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        let rec = self.thread_show(project, id)?;
        if line.is_some() && add.is_some() {
            return Err(bad("--line과 --add는 함께 쓰지 않는다"));
        }
        if let Some(text) = add {
            let text = text.trim();
            if text.is_empty() || text.contains('\n') {
                return Err(bad("Next 줄은 비지 않은 한 줄이다"));
            }
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(next_file(&dir, id))
                .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
            writeln!(f, "- {text}").map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
            self.db.audit(actor, "thread_next_add", &format!("{project}/{id}"));
        }
        let lines = self.next_lines(&dir, &rec.next, id);
        let message = match line {
            None => None,
            Some(n) => {
                let Some(text) = n.checked_sub(1).and_then(|i| lines.get(i as usize)) else {
                    return Err(bad(format!("{id}에 Next {n}번째 줄이 없다({}줄)", lines.len())));
                };
                Some(self.thread_message(project, id, text.clone(), MessageMode::Queue, actor).await?)
            }
        };
        Ok(FrontResponse::Next { lines, message })
    }

    /// 머신마다 맡길 수 있는 profile(그 프로젝트 safety `thread_profiles` ∩ 그 머신 agent.toml과 설치·로그인한 기본
    /// profile). 허용 목록에 이름이 있는 기본 profile은 설치를 몰라도 넣는다(참고 `usable`). 끊긴 머신은 모른다.
    pub(crate) async fn assignable(&self, project: &str) -> Vec<OwnerChoice> {
        let safety = self.safety(project);
        let mut out = Vec::new();
        for m in self.machine_views() {
            let mut profiles = Vec::new();
            let connected = m.connection == aaa_proto::Connection::Connected;
            if connected
                && self.machine_feature(&m.id, "host_profiles.v1")
                && let Ok(FrontResponse::Profiles { profiles: ps, .. }) = self.profiles(Some(&m.name)).await
            {
                profiles = ps.into_iter().map(|p| p.name).filter(|n| safety.profile_allowed(n)).collect();
                if self.machine_feature(&m.id, "profiles.v2") {
                    for b in safety.thread_profiles.iter().flatten() {
                        if aaa_config::profile_edit::builtin(b).is_some() && !profiles.contains(b) {
                            profiles.push(b.clone());
                        }
                    }
                }
            }
            out.push(OwnerChoice { machine: m.name, connected, profiles });
        }
        out
    }

    /// owner의 맨 이름이 모델(profile)인지 볼 이름(plan §8.335): host가 정의한 profile, 기본 harness, 프로젝트 `thread_profiles`.
    /// 머신 연결과 상관없다(끊긴 머신에 맡긴 `(codex-fast)`가 사람으로 읽히지 않게). 쓸 수 있는지는 `check_owner`가 따로 본다
    pub(crate) fn known_profiles(&self, project: &str) -> Vec<String> {
        let mut out: Vec<String> = aaa_config::profile_edit::BUILTINS.iter().map(|b| b.to_string()).collect();
        if let Ok(cfg) = self.profile_config() {
            out.extend(cfg.profiles.into_keys());
        }
        out.extend(self.safety(project).thread_profiles.into_iter().flatten().filter(|p| p != "*"));
        out.sort();
        out.dedup();
        out
    }

    /// TASKS.md owner로 써도 되는가(0.2.35 `assignable --check`). 맨 profile은 기본 머신(host 로컬)을 본다.
    pub(crate) fn check_owner(&self, project: &str, owners: &[OwnerChoice], raw: &str) -> Result<String, ErrorBody> {
        let raw = raw.trim().trim_start_matches('(').trim_end_matches(')').trim();
        let mut all: Vec<String> = owners.iter().flat_map(|o| o.profiles.clone()).collect();
        all.extend(self.known_profiles(project));
        let local = self.machine_views().into_iter().find(|m| m.local).map(|m| m.name);
        let on = |machine: &str| owners.iter().find(|o| o.machine == machine);
        match Owner::parse(Some(raw), &all) {
            Owner::Unassigned => Ok(format!("`{raw}`: 미할당(thread를 시작할 때 --profile을 고른다)")),
            Owner::Me => Ok("`me`: 사용자의 할 일".into()),
            Owner::Person(p) => {
                Err(bad(format!("`{p}`는 사람 이름으로 읽힌다. agent owner는 profile·@machine·profile@machine이다")))
            }
            Owner::Agent { profile, machine } => {
                let m = machine.or(local).ok_or_else(|| bad("로컬 agent가 없다. @machine을 붙인다"))?;
                let o = on(&m).ok_or_else(|| bad(format!("모르는 머신: {m}")))?;
                if let Some(p) = profile {
                    if !o.connected {
                        return Err(bad(format!("{m}이 연결되지 않아 profile 목록을 모른다")));
                    }
                    if !o.profiles.contains(&p) {
                        return Err(bad(format!(
                            "{m}에서 쓸 수 있는 profile이 아니다: {p}({})",
                            o.profiles.join(", ")
                        )));
                    }
                }
                Ok(format!("`{raw}`는 쓸 수 있는 owner다"))
            }
        }
    }

    /// task 없이 시작할 때 TASKS.md의 비슷한 열린 task를 알린다. 파일이 없으면 평소처럼 시작한다.
    pub(crate) fn task_start_warning(&self, project: &str, title: &str) -> Option<String> {
        let dir = self.project_dir(project).ok()?;
        let text = std::fs::read_to_string(dir.join("TASKS.md")).ok()?;
        tasks::start_warning(&tasks::parse(&text), title)
    }

    /// `thread start --from-task`: owner를 profile·machine 기본값으로, 메모를 할 일에 붙인다
    /// 돌려주는 것은 경고 한 줄(이미 열린 thread가 맡은 task: 참고에는 그 거부가 없어 막지 않는다)과 읽은 task다(시작한 뒤 그 블록에
    /// thread를 잇는다). `block`(화면이 본 블록 hash)을 주면 지금 파일의 블록이 같을 때만 받는다: 본 것과 다른 일을 맡기지 않는다.
    pub(crate) async fn apply_from_task(
        &self,
        a: &mut StartArgs,
        title: &str,
        block: Option<&str>,
    ) -> Result<(Option<String>, TaskView), ErrorBody> {
        let list = self.task_board(&a.project)?.tasks;
        let t = tasks::find(&list, title).ok_or_else(|| bad(format!("TASKS.md에 없는 task: {title}")))?.clone();
        if block.is_some_and(|b| b != t.block_sha256) {
            return Err(err(
                ErrorCode::Unavailable,
                format!("「{}」 task가 그사이 바뀌었다. 할 일 화면에서 최신을 보고 다시 시작한다", t.title),
            ));
        }
        // 끝 괄호의 상태 메모를 사람 owner로 오인해 거부하지 않는다: 정확한 까닭으로 거부한다
        if tasks::owner_misread(&t) {
            return Err(bad(format!("owner 칸 형식 어긋남, 고친 뒤 시작: {}", t.title)));
        }
        let mut warning = None;
        if let Some(tid) = &t.thread
            && self.thread_show(&a.project, tid).is_ok_and(|r| r.status == ThreadStatus::Open)
        {
            warning = Some(format!("열린 {tid}가 이미 이 task를 맡고 있다. 그래도 시작했다"));
        }
        let mut all: Vec<String> = self.assignable(&a.project).await.into_iter().flat_map(|o| o.profiles).collect();
        all.extend(self.known_profiles(&a.project));
        match Owner::parse(t.owner.as_deref(), &all) {
            Owner::Me => return Err(bad(format!("사용자의 할 일(me)이다: {}", t.title))),
            Owner::Person(p) => return Err(bad(format!("사람({p})이 맡은 task다: {}", t.title))),
            Owner::Unassigned => {}
            Owner::Agent { profile, machine } => {
                if let Some(w) = profile {
                    if a.profile.is_empty() {
                        a.profile = w;
                    } else if a.profile != w {
                        return Err(bad(format!("task owner와 --profile이 다르다: {w} / {}", a.profile)));
                    }
                }
                if let Some(w) = machine {
                    match &a.machine {
                        None => a.machine = Some(w),
                        Some(h) if *h != w => return Err(bad(format!("task owner와 --machine이 다르다: {w} / {h}"))),
                        _ => {}
                    }
                }
            }
        }
        if a.title.trim().is_empty() {
            a.title = t.title.clone();
        }
        if a.task.trim().is_empty() {
            a.task = t.title.clone();
        }
        if !t.notes.is_empty() {
            a.task = format!("{}\n\n## Notes from the task list\n\n{}\n", a.task.trim_end(), t.notes.join("\n"));
        }
        // 메모의 그림은 그 머신 작업 폴더 `attachments/`로 넘긴다(plan §8.327): 할 일 글도 그 경로를 가리킨다
        let (attachments, missing) = self.task_attachments(&a.project, &mut a.task);
        a.attachments = attachments;
        let warning = match (warning, missing.into_iter().next()) {
            (Some(w), Some(m)) => Some(format!("{w}. {m}")),
            (w, m) => w.or(m),
        };
        Ok((warning, t))
    }

    /// 시작한 thread를 그 task 줄 끝에 잇는다(`ThreadStart.link_task`, 할 일 T3). 시작할 때 읽은 블록 hash로 찾고, 그사이 바뀌었거나
    /// 고칠 권한이 없으면 시작은 그대로 두고 경고 한 줄을 돌려준다
    pub(crate) async fn link_started(&self, project: &str, t: &TaskView, thread: &str, actor: &str) -> Option<String> {
        let at = aaa_proto::TaskAnchor {
            list: t.list.clone(),
            title: t.title.clone(),
            block_sha256: t.block_sha256.clone(),
            line: t.line,
        };
        let op = TaskOp::Update {
            at,
            title: None,
            owner: None,
            person: false,
            profile: None,
            machine: None,
            notes: None,
            thread: Some(thread.to_string()),
            done: None,
            repo: None,
        };
        let why = match self.task_edit(project, vec![op], None, None, actor).await {
            Ok(FrontResponse::TaskEdited { conflict: None, .. }) => return None,
            Ok(FrontResponse::TaskEdited { conflict: Some(c), .. }) => c.reason,
            Ok(_) => "모르는 응답".into(),
            Err(e) => e.message,
        };
        Some(format!("{thread}를 「{}」 task에 잇지 못했다({why}). 할 일 화면 「task에 잇기」로 잇는다", t.title))
    }

    /// 할 일 「바로 시작」 확인창(`thread_start_preview`): `thread start --from-task`와 같은 규칙으로 채운 값과 저장소·한도
    pub(crate) async fn thread_start_preview(
        &self,
        project: &str,
        from_task: &str,
        block: Option<&str>,
    ) -> Result<FrontResponse, ErrorBody> {
        let mut a = StartArgs {
            over_limit: None,
            reservation: None,
            account: None,
            holds: vec![],
            project: project.to_string(),
            title: String::new(),
            machine: None,
            profile: String::new(),
            kind: aaa_proto::ThreadKind::Dir,
            repo: None,
            base: None,
            task: String::new(),
            role: aaa_proto::Role::Worker,
            attachments: Vec::new(),
        };
        let (warning, t) = self.apply_from_task(&mut a, from_task, block).await?;
        if a.profile.is_empty() {
            a.profile = self.default_thread_profile(project);
        }
        let (model, effort, profile_revision) = crate::task_queue::model_selection(self, &a.profile)?;
        let set = self.settings(project);
        let open_threads = self.open_worker_threads(project);
        let (machine_order, local_machine) = self.machine_order();
        Ok(FrontResponse::ThreadStartPreview(aaa_proto::ThreadStartPreview {
            model,
            effort,
            profile_revision,
            title: a.title,
            task_block: t.block_sha256,
            task: a.task,
            profile: a.profile,
            machine: a.machine,
            repos: set.repos.into_iter().map(|r| aaa_proto::RepoChoice { path: r.path, machine: r.machine }).collect(),
            open_threads,
            max_parallel_threads: set.max_parallel_threads,
            warnings: warning.into_iter().collect(),
            repo: t.repo,
            local_machine,
            machine_order,
        }))
    }

    /// 식별 주석만 추가한 쓰기는 사용자 내용의 변경 기록으로 중복 관측하지 않는다.
    pub(crate) fn task_identity_seen(&self, project: &str, before: &str, after: &file_rev::Rev) {
        if let Some(seen) = self.tasks_seen.lock().unwrap().get_mut(project)
            && seen.sha256 == before
        {
            seen.len = u64::MAX;
            seen.mtime = None;
            seen.sha256 = after.sha256.clone();
            seen.text = after.text.clone();
        }
    }

    /// 할 일 화면(`tasks`): TASKS.md 목록·task, task 없는 열린 thread, 「나중」 파일. 읽은 파일 상태를 ticker 기억에 심는다
    /// (host가 뜬 직후 첫 tick 전에 읽고 그사이 바뀐 것을 놓치지 않게)
    pub(crate) fn task_board(&self, project: &str) -> Result<TaskBoard, ErrorBody> {
        let dir = self.project_dir(project)?;
        self.task_identify(project)?;
        let (text, seen) = tasks_file(&dir);
        let notes = notes_stamps(&dir, &self.settings(project).task_notes);
        self.tasks_seen.lock().unwrap().entry(project.to_string()).or_insert(TasksSeen { notes, ..seen.clone() });
        let (lists, mut tasks) = tasks::parse_board(&text);
        self.fill_task_ids(project, &mut tasks)?;
        tasks::fill_agent(&mut tasks, &self.known_profiles(project));
        let linked: std::collections::HashSet<String> = tasks.iter().filter_map(|t| t.thread.clone()).collect();
        let open: Vec<_> =
            self.threads_of(Some(project), None)?.into_iter().filter(|t| t.group != ThreadGroup::Resolved).collect();
        // 「정리 제안」의 연결 후보는 이 프로젝트의 열린 thread 번호다
        let open_ids: Vec<String> = open.iter().map(|t| t.id.clone()).collect();
        for t in &mut tasks {
            t.suggestion = tasks::suggest(t, &open_ids);
        }
        self.fill_attention(project, &mut tasks);
        // 끝 대괄호가 저장소 꼴이라 칸으로 읽었지만 project.toml에 없는 이름(손으로 쓴 `[WIP]` 등): 시작 전에 화면에서 보이게
        let repos = crate::task_repo::repo_names(&self.settings(project).repos);
        for t in &mut tasks {
            if let Some(r) = t.repo.as_deref().filter(|r| *r != tasks::REPO_DIR && !repos.iter().any(|x| x == r)) {
                t.problems.push(format!("project.toml repos에 없는 저장소 칸: [{r}]"));
            }
        }
        // 임무 thread는 특수 패널의 일이고, 자동화 thread는 출처(규칙)가 있어 task가 필요 없다(plan t-0475 §5.3): 할 일과 잇지 않는다
        let unlinked_threads = open
            .into_iter()
            .filter(|t| t.automation.is_none() && !linked.contains(&t.id))
            .map(|t| TaskThread { id: t.id, title: t.title, group: t.group, stage: t.stage })
            .collect();
        let notes_files = self.settings(project).task_notes.iter().map(|p| notes_file(&dir, p)).collect();
        Ok(TaskBoard {
            queue: self.db.queue_list(project).map_err(crate::orch::internal)?,
            project: project.to_string(),
            sha256: seen.sha256,
            lists,
            tasks,
            unlinked_threads,
            notes_files,
            text,
            repos,
        })
    }

    /// 할 일을 고쳐도 되는 actor(t-0469 §5): 조작 권한은 요청 판정(`needs_control`)이 이미 봤다. aaa 세션 안에서는 그 프로젝트의
    /// coordinator pane만 된다(thread pane·남의 프로젝트 pane·사람의 셸 pane 거부). 기록에 남길 actor를 돌려준다.
    pub(crate) fn task_writer(&self, project: &str, actor: &str) -> Result<String, ErrorBody> {
        if !front::in_session(actor) {
            return Ok(actor.to_string());
        }
        match self.coordinator_project_of_actor(actor) {
            Some(p) if p == project => {
                Ok(format!("coordinator:{}", actor.rsplit_once('@').map(|(_, pane)| pane).unwrap_or_default()))
            }
            _ => Err(err(
                ErrorCode::Forbidden,
                "aaa 세션 안에서는 그 프로젝트 coordinator만 할 일을 고친다. thread는 보고 `## Next`·post로 제안한다",
            )),
        }
    }

    /// `task_edit`(할 일 편집 T2, plan §8.311): 동작을 최신 파일에 차례로 적용해 한 번에 쓴다(t-0469 §2.2·§2.3). 대상 블록이
    /// 없으면 쓰지 않고 `conflict`와 그 task의 지금 원문을 준다. 쓴 뒤 기록·ticker 기억·`tasks` 사건.
    pub(crate) async fn task_edit(
        &self,
        project: &str,
        mut ops: Vec<TaskOp>,
        base_sha256: Option<String>,
        user_request: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        let by = self.answered_by(actor);
        let actor = self.task_writer(project, actor)?;
        if actor.starts_with("coordinator:") && ops.iter().any(|o| matches!(o, TaskOp::RawSave { .. })) {
            return Err(err(
                ErrorCode::Forbidden,
                "coordinator는 원문 편집으로 선택 보호를 우회하지 않는다. task 편집 명령을 쓴다",
            ));
        }

        if ops.is_empty() {
            return Err(bad("동작이 없다"));
        }
        let user_request = user_request.as_deref().map(str::trim);
        if let Some(reason) = user_request {
            if reason.is_empty() || reason.len() > 2000 || reason.chars().any(char::is_control) {
                return Err(bad("사용자 지시 근거는 비지 않은 한 줄(2000바이트 이하)로 쓴다"));
            }
            if !matches!(ops.as_slice(), [TaskOp::Update {
                profile, machine, owner: None, person: false, title: None, notes: None,
                thread: None, done: None, repo: None, ..
            }] if profile.is_some() || machine.is_some())
            {
                return Err(bad("--user-request는 한 task의 --profile·--machine 변경에만 쓴다. 다른 칸은 따로 고친다"));
            }
        }
        // 「답할 것」의 할 일 확인 필요(plan §8.345): 이 편집으로 풀린 것을 기록에 남긴다
        let attention_before: Vec<(String, String, Option<String>)> = self
            .task_board(project)
            .map(|b| b.tasks.into_iter().filter_map(|t| Some((t.title.clone(), t.attention?, t.thread))).collect())
            .unwrap_or_default();
        self.task_op_ids(project, &mut ops)?;
        // owner 판정에 쓸 목록(쓰기 lock 밖에서 한 번, 머신마다 묻는다)
        let checks_owner = ops.iter().any(|o| match o {
            TaskOp::Add { owner, profile, machine, .. } | TaskOp::Update { owner, profile, machine, .. } => {
                // 칸 하나를 비워도 남은 칸(지금 머신 등)을 다시 판정한다
                owner.as_deref().is_some_and(|o| !o.trim().is_empty()) || profile.is_some() || machine.is_some()
            }
            _ => false,
        });
        let owners = if checks_owner { self.assignable(project).await } else { Vec::new() };
        // 예약 dispatch와 같은 문으로 직렬화하여 옛 값으로 시작하는 틈을 막는다.
        let _queue_guard = if user_request.is_some() { Some(self.queue_gate.lock().await) } else { None };
        let owner_ok = |o: &str, person: bool| -> Result<(), String> {
            if person {
                return tasks::check_person(o);
            }
            self.check_owner(project, &owners, o).map(|_| ()).map_err(|e| {
                if matches!(Owner::parse(Some(o), &[]), Owner::Person(_)) && !o.contains('@') {
                    format!("{}. 사람에게 맡기는 일이면 「사람으로 저장」을 고른다", e.message)
                } else {
                    e.message
                }
            })
        };
        let removed = |id: i64| -> Result<Removed, String> {
            let e = self.db.task_log_get(project, id).map_err(|e| e.to_string())?.ok_or(format!("없는 기록: {id}"))?;
            if !e.action.removes() {
                return Err(format!("삭제 기록이 아니다: {id}"));
            }
            if let Some(by) = e.restored_by {
                return Err(format!("이미 되살렸다(기록 {by})"));
            }
            Ok(Removed { list: e.list, text: e.before.unwrap_or_default() })
        };
        let repo_ok = |r: &str| self.task_repo_ok(project, r);
        let profiles = self.known_profiles(project);
        let ctx = task_edit::Ctx { owner_ok: &owner_ok, profiles: &profiles, repo_ok: &repo_ok, removed: &removed };
        let (conflict, log_ids) =
            self.task_write_requested(project, &dir, &ops, base_sha256.as_deref(), &actor, &ctx, false, user_request)?;
        let board = self.task_board(project)?;
        for (title, why, thread) in &attention_before {
            let now = tasks::find(&board.tasks, title);
            if now.is_some_and(|t| t.attention.is_some()) {
                continue;
            }
            let summary = match now {
                None => "끝남으로 지움",
                Some(t) if t.thread.is_none() => "연결만 풂",
                Some(t) if t.thread != *thread => "다시 맡김",
                Some(_) => "처리함",
            };
            self.record_task(project, title, why, summary, &by);
        }
        let conflict = conflict.map(|(op, reason, title)| TaskConflict {
            op: op as u32,
            reason,
            latest: (!title.is_empty()).then(|| tasks::find(&board.tasks, &title).cloned().map(Box::new)).flatten(),
        });
        Ok(FrontResponse::TaskEdited { board, conflict, log_ids })
    }

    /// 할 일 쓰기(편집 `task_edit`과 끝난 thread의 자동 정리가 함께 쓴다): 동작을 최신 파일에 차례로 적용해 한 번에 쓰고, 기록·ticker
    /// 기억·`tasks` 사건을 남긴다. 대상이 없으면 쓰지 않고 충돌(동작 차례, 까닭, 제목)을 돌려준다. `auto`면 지운 기록을 「자동 정리」로 남긴다
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn task_write(
        &self,
        project: &str,
        dir: &Path,
        ops: &[TaskOp],
        base_sha256: Option<&str>,
        actor: &str,
        ctx: &task_edit::Ctx,
        auto: bool,
    ) -> Result<(Option<WriteConflict>, Vec<i64>), ErrorBody> {
        self.task_write_requested(project, dir, ops, base_sha256, actor, ctx, auto, None)
    }

    #[allow(clippy::too_many_arguments)]
    fn task_write_requested(
        &self,
        project: &str,
        dir: &Path,
        ops: &[TaskOp],
        base_sha256: Option<&str>,
        actor: &str,
        ctx: &task_edit::Ctx,
        auto: bool,
        user_request: Option<&str>,
    ) -> Result<(Option<WriteConflict>, Vec<i64>), ErrorBody> {
        let log_actor = user_request.map(|reason| format!("{actor} (사용자 지시: {reason})"));
        let log_actor = log_actor.as_deref().unwrap_or(actor);
        let path = dir.join("TASKS.md");
        let lock = self.file_locks.of(&path);
        let guard = lock.lock().unwrap();
        let mut conflict: Option<WriteConflict> = None;
        let saved = file_rev::save(&path, |rev| {
            conflict = None;
            let expected = base_sha256.or(match ops {
                [TaskOp::RawSave { expected_sha256, .. }] => Some(expected_sha256.as_str()),
                _ => None,
            });
            if expected.is_some_and(|want| want != rev.sha256) {
                conflict = Some((0, "파일이 그사이 바뀌었다(전체 판이 다르다)".into(), String::new()));
                return Ok(Edit::Keep(Vec::new()));
            }
            match task_edit::apply(&rev.text, ops, ctx) {
                Ok(a) if a.text == rev.text => Ok(Edit::Keep(Vec::new())),
                Ok(mut a) => {
                    a.text = crate::task_queue::identify(&a.text)?;
                    let identified = tasks::parse(&a.text);
                    for r in &mut a.records {
                        if r.action == TaskAction::Raw {
                            r.after = Some(a.text.clone());
                            continue;
                        }
                        if let Some(after) = &mut r.after {
                            *after = after
                                .split_inclusive('\n')
                                .map(|line| {
                                    if !tasks::is_task(line) || crate::task_queue::split_id(line).1.is_some() {
                                        return line.to_string();
                                    }
                                    let body = line.trim_end_matches(['\r', '\n']);
                                    let title = tasks::split_line(&body.trim_start()[5..]).title;
                                    let id = identified.iter().find(|t| t.title == title).and_then(|t| t.id.as_deref());
                                    format!("{body}{}{}", crate::task_queue::id_suffix(id), &line[body.len()..])
                                })
                                .collect();
                        }
                    }
                    if user_request.is_some() {
                        self.check_requested_task_choices(project, &rev.text, &a.text)?;
                    } else {
                        self.protect_task_choices(&rev.text, &a.text, actor)?;
                    }
                    for r in a.records.iter().filter(|_| user_request.is_none()) {
                        if r.action == TaskAction::Update {
                            self.protect_task_choices(
                                r.before.as_deref().unwrap_or_default(),
                                r.after.as_deref().unwrap_or_default(),
                                actor,
                            )?;
                        }
                    }
                    // 끝난 thread의 자동 정리는 기록 화면에서 「자동 정리」로 거른다(되살리기는 삭제와 같다)
                    if auto {
                        for r in a.records.iter_mut().filter(|r| r.action == TaskAction::Delete) {
                            let task = tasks::parse(&rev.text)
                                .into_iter()
                                .find(|t| t.title == r.title)
                                .ok_or_else(|| internal("완료 task가 없다"))?;
                            let id = task.id.clone().ok_or_else(|| internal("완료 보관에는 task ID가 필요하다"))?;
                            let report = task.thread.as_ref().map(|t| format!("threads/{t}.md"));
                            let reason = task
                                .thread
                                .as_ref()
                                .and_then(|tid| self.load(project, tid).ok())
                                .map(|rec| {
                                    if rec.task_clean.why.as_deref() == Some("merged") {
                                        "PR 병합"
                                    } else {
                                        "보고 [완료]"
                                    }
                                })
                                .unwrap_or("완료 판정")
                                .to_string();
                            let marker = format!("<!-- aaa-task:{id} -->");
                            let mut history = self
                                .db
                                .task_log_list(project, None, u32::MAX, None)
                                .map_err(internal)?
                                .into_iter()
                                .filter(|h| {
                                    h.title == task.title
                                        || h.before.as_ref().is_some_and(|s| s.contains(&marker))
                                        || h.after.as_ref().is_some_and(|s| s.contains(&marker))
                                })
                                .collect::<Vec<_>>();
                            // 복원 뒤 다시 완료해도 이전 완료·결과와 보관 안의 유일한 이력을 남긴다.
                            if let Some(old) =
                                self.db.archive_list(project).map_err(internal)?.into_iter().find(|a| a.task_id == id)
                            {
                                for h in old.history {
                                    if !history.iter().any(|n| n.id == h.id && n.at_ms == h.at_ms) {
                                        history.push(h);
                                    }
                                }
                                history.push(aaa_proto::TaskLogEntry {
                                    id: 0,
                                    at_ms: old.completed_ms,
                                    actor: "host".into(),
                                    by: "host 완료 보관".into(),
                                    action: TaskAction::Completed,
                                    list: old.task.list,
                                    from_list: None,
                                    title: old.task.title,
                                    before: Some(old.body),
                                    after: None,
                                    thread: old.task.thread,
                                    restored_by: None,
                                });
                            }
                            self.db
                                .archive_save(
                                    project,
                                    &aaa_proto::ArchivedTask {
                                        task_id: id,
                                        task,
                                        body: r.before.clone().unwrap_or_default(),
                                        reason,
                                        report,
                                        completed_ms: aaa_common::util::now_ms(),
                                        restored: false,
                                        committed: false,
                                        removed_sha256: tasks::sha256(&a.text),
                                        history,
                                    },
                                )
                                .map_err(internal)?;
                            r.action = TaskAction::Completed;
                        }
                    }
                    Ok(Edit::Write(a.text, a.records))
                }
                Err(Fail::Conflict { op, reason, title }) => {
                    conflict = Some((op, reason, title));
                    Ok(Edit::Keep(Vec::new()))
                }
                Err(Fail::Bad(m)) => Err(bad(m)),
            }
        })
        .map_err(|e| match e {
            file_rev::SaveError::Edit(e) => e,
            file_rev::SaveError::Io(e) => err(ErrorCode::Internal, format!("TASKS.md: {e}")),
        })?;
        let mut log_ids = Vec::new();
        if let Some(after) = &saved.after {
            if user_request.is_some() {
                self.record_requested_task_choices(project, &saved.before.text, &after.text, log_actor)?;
            } else {
                self.record_task_choices(project, &saved.before.text, &after.text, actor)?;
            }
            if auto {
                self.archive_recover(project)?;
            }
            let now = aaa_common::util::now_ms();
            // ticker가 아직 못 본 직접 편집이 이 쓰기에 묻히지 않게 먼저 「파일 직접」으로 적는다
            let unseen = self
                .tasks_seen
                .lock()
                .unwrap()
                .get(project)
                .filter(|seen| seen.sha256 != saved.before.sha256)
                .map(|seen| task_edit::diff(&seen.text, &saved.before.text))
                .unwrap_or_default();
            for r in &unseen {
                if let Err(e) = self.db.task_log_add(project, now, FILE_ACTOR, r) {
                    tracing::warn!(error = %e, "할 일 기록 실패");
                }
            }
            for r in &saved.out {
                match self.db.task_log_add(project, now, log_actor, r) {
                    Ok(id) => log_ids.push(id),
                    Err(e) => tracing::warn!(error = %e, "할 일 기록 실패"),
                }
            }
            // 자기 쓰기를 다음 tick이 「파일 직접」으로 적지 않게 기억을 새 판으로 둔다. 크기·mtime은 비워 다음 tick이 다시 재게 한다
            // (rename과 이 기억 사이에 끼어든 직접 편집은 내용 hash가 달라 그대로 잡힌다)
            // 기억이 아직 없으면(host가 뜬 뒤 첫 tick 전) 새 판으로 심는다: 쓰기 전에 읽은 `tasks`가 옛 글을 심지 못하게
            let notes = notes_stamps(dir, &self.settings(project).task_notes);
            let mut seen = self.tasks_seen.lock().unwrap();
            let seen = seen.entry(project.to_string()).or_insert_with(|| TasksSeen {
                len: u64::MAX,
                mtime: None,
                sha256: String::new(),
                text: String::new(),
                notes,
            });
            (seen.len, seen.mtime, seen.sha256, seen.text) = (u64::MAX, None, after.sha256.clone(), after.text.clone());
            drop(guard);
            let _ = self.task_events.send((project.to_string(), after.sha256.clone()));
            self.tick_wake.notify_one();
            self.db.audit(
                actor,
                if user_request.is_some() {
                    "task_choices_user_request"
                } else if auto {
                    "task_auto_clean"
                } else {
                    "task_edit"
                },
                &format!(
                    "{project} ops={} sha={}→{}{}",
                    ops.len(),
                    saved.before.sha256,
                    after.sha256,
                    user_request.map(|r| format!(" 사용자 지시: {r}")).unwrap_or_default()
                ),
            );
        } else {
            drop(guard);
        }
        Ok((conflict, log_ids))
    }

    /// 할 일 기록(최신부터). `by`를 보이는 이름으로 바꾼다
    pub(crate) fn task_log(
        &self,
        project: &str,
        before: Option<i64>,
        limit: Option<u32>,
        filter: Option<TaskLogFilter>,
        task: Option<&str>,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        let limit = limit.unwrap_or(50).clamp(1, 500) as usize;
        let selector = task.map(|s| self.task_selector(project, s)).transpose()?;
        let mut entries = self
            .db
            .task_log_list(project, before, if selector.is_some() { u32::MAX } else { limit as u32 + 1 }, filter)
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        if let Some(want) = selector {
            let marker = format!("<!-- aaa-task:{want} -->");
            entries.retain(|e| {
                e.title.eq_ignore_ascii_case(&want)
                    || e.before.iter().chain(e.after.iter()).any(|b| b.contains(&marker))
            });
        }
        let next = (entries.len() > limit).then(|| {
            entries.truncate(limit);
            entries.last().map(|e| e.id)
        });
        let devices = self.db.devices().unwrap_or_default();
        for e in &mut entries {
            e.by = task_actor_label(&e.actor, &devices);
        }
        Ok(FrontResponse::TaskLog { entries, next: next.flatten() })
    }

    pub(crate) fn task_log_prune(
        &self,
        project: &str,
        before_ms: u64,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        self.task_writer(project, actor)?;
        let n = self.db.task_log_prune(project, before_ms).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        self.db.audit(actor, "task_log_prune", &format!("{project} before={before_ms} n={n}"));
        self.task_log(project, None, None, None, None)
    }

    /// ticker: TASKS.md의 (크기, mtime)이 기억과 다르면 내용 hash를 다시 재고, 바뀌었으면 `tasks` 사건을 낸다(파일 감시는 두지
    /// 않는다). 「나중」 파일(`task_notes`)은 목록과 (크기, mtime)이 바뀌면 같은 사건을 낸다. 처음 보는 프로젝트는 기억만 한다.
    /// 파일 직접 편집(사람·agent가 파일을 고침)은 task마다의 차이를 「파일 직접」으로 기록한다(t-0469 §2.3). host가 쓴 판은 쓰기가
    /// 기억을 새 판으로 두므로 여기서 다시 적지 않는다: 그래서 쓰기와 같은 파일 lock 안에서 본다.
    pub(crate) fn tasks_pass(&self, project: &str, dir: &Path) {
        let path = dir.join("TASKS.md");
        let lock = self.file_locks.of(&path);
        let _guard = lock.lock().unwrap();
        let stamp = file_stamp(&path);
        let notes = notes_stamps(dir, &self.settings(project).task_notes);
        let mut seen = self.tasks_seen.lock().unwrap();
        let Some(old) = seen.get_mut(project) else {
            seen.insert(project.to_string(), TasksSeen { notes, ..tasks_file(dir).1 });
            return;
        };
        let notes_changed = old.notes != notes;
        old.notes = notes;
        let mut changed = notes_changed;
        let mut edits = Vec::new();
        if (old.len, old.mtime) != stamp {
            let now = tasks_file(dir).1;
            if now.sha256 != old.sha256 {
                changed = true;
                edits = task_edit::diff(&old.text, &now.text);
            }
            (old.len, old.mtime, old.sha256, old.text) = (now.len, now.mtime, now.sha256, now.text);
        }
        let sha256 = old.sha256.clone();
        drop(seen);
        let now = aaa_common::util::now_ms();
        for r in &edits {
            if let Err(e) = self.db.task_log_add(project, now, FILE_ACTOR, r) {
                tracing::warn!(error = %e, "할 일 기록 실패");
            }
        }
        if changed {
            let _ = self.task_events.send((project.to_string(), sha256));
        }
    }

    /// coordinator용 요약. herdr-projects `context`처럼 처리 안 한 inbox 항목을 보이고, `peek`이 아니면 「봤다」로 표시한다
    /// (본 항목은 다음 nudge에 다시 나오지 않는다).
    pub(crate) async fn context(self: &Arc<Self>, project: &str, peek: bool) -> Result<ProjectContext, ErrorBody> {
        let dir = self.project_dir(project)?;
        let mut tasks = self.task_board(project)?.tasks;
        tasks::fill_agent(&mut tasks, &self.known_profiles(project));
        self.fill_attention(project, &mut tasks);
        let mut threads: Vec<_> =
            self.threads_of(Some(project), None)?.into_iter().filter(|t| t.group != ThreadGroup::Resolved).collect();
        for t in &mut threads {
            t.next = self.next_lines(&dir, &t.next, &t.id);
        }
        let page = self.inbox(project, false, peek)?;
        Ok(ProjectContext {
            post_warnings: self.post_warnings(project)?,
            project: project.to_string(),
            path: dir.display().to_string(),
            start_threads: self.safety(project).start_threads,
            tasks,
            threads,
            assignable: self.assignable(project).await,
            inbox_open: page.items.len() as u32 + page.omitted_open,
            inbox: page.items,
            web_url: self.public_origin(),
        })
    }
}

/// 파일 직접 편집 기록의 actor
const FILE_ACTOR: &str = "file";

/// 기록 actor → 보이는 이름: 기기 이름·coordinator·터미널·파일 직접·host 자동 정리
fn task_actor_label(actor: &str, devices: &[crate::db::DeviceRow]) -> String {
    if let Some((by, reason)) = actor.split_once(" (사용자 지시: ") {
        return format!(
            "{}가 사용자 지시로 변경: {}",
            task_actor_label(by, devices),
            reason.strip_suffix(')').unwrap_or(reason)
        );
    }
    if actor == FILE_ACTOR {
        return "파일 직접".into();
    }
    if actor == "host" {
        return "host 자동 정리".into();
    }
    if let Some(id) = actor.strip_prefix("device:") {
        return devices.iter().find(|d| d.id == id).map_or_else(|| format!("기기 {id}"), |d| d.label.clone());
    }
    if actor.starts_with("coordinator:") {
        return "coordinator".into();
    }
    if actor.starts_with("local:") || actor.starts_with("person:") {
        return "터미널".into();
    }
    actor.to_string()
}

/// ticker가 기억하는 TASKS.md 상태
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TasksSeen {
    len: u64,
    mtime: Option<std::time::SystemTime>,
    sha256: String,
    /// 마지막으로 본 글(파일 직접 편집의 차이를 task마다 적는다)
    text: String,
    /// 「나중」 파일마다 (경로, 크기, mtime)
    notes: Vec<(String, u64, Option<std::time::SystemTime>)>,
}

fn notes_stamps(dir: &Path, files: &[String]) -> Vec<(String, u64, Option<std::time::SystemTime>)> {
    files
        .iter()
        .map(|p| {
            let (len, mtime) = file_stamp(&dir.join(p));
            (p.clone(), len, mtime)
        })
        .collect()
}

fn file_stamp(path: &Path) -> (u64, Option<std::time::SystemTime>) {
    match std::fs::metadata(path) {
        Ok(m) => (m.len(), m.modified().ok()),
        Err(_) => (0, None),
    }
}

/// TASKS.md 글과 그 상태. 크기·mtime을 먼저 읽는다: 그사이 바뀌면 다음 tick이 다시 잰다(없으면 빈 글)
fn tasks_file(dir: &Path) -> (String, TasksSeen) {
    let path = dir.join("TASKS.md");
    let (len, mtime) = file_stamp(&path);
    let bytes = std::fs::read(&path).unwrap_or_default();
    let sha256 = hex::encode(<sha2::Sha256 as sha2::Digest>::digest(&bytes));
    let text = String::from_utf8_lossy(&bytes).into_owned();
    (text.clone(), TasksSeen { len, mtime, sha256, text, notes: Vec::new() })
}

/// 「나중」 파일 하나. 프로젝트 폴더 밖(symlink 포함)·없음·너무 큼은 까닭만 준다
fn notes_file(dir: &Path, rel: &str) -> TaskNotesFile {
    let fail = |e: String| TaskNotesFile { path: rel.to_string(), error: Some(e), sections: Vec::new() };
    if let Err(e) = tasks::check_notes_path(rel) {
        return fail(e);
    }
    let path = dir.join(rel);
    let (Ok(real), Ok(root)) = (path.canonicalize(), dir.canonicalize()) else { return fail("없음".into()) };
    if !real.starts_with(&root) {
        return fail("프로젝트 폴더 밖을 가리킨다(symlink)".into());
    }
    match std::fs::metadata(&real) {
        Ok(m) if !m.is_file() => return fail("파일이 아니다".into()),
        Ok(m) if m.len() > tasks::NOTES_MAX_BYTES => {
            return fail(format!("너무 크다({}KiB 넘음)", tasks::NOTES_MAX_BYTES / 1024));
        }
        Err(e) => return fail(e.to_string()),
        Ok(_) => {}
    }
    match std::fs::read(&real) {
        Ok(b) => TaskNotesFile {
            path: rel.to_string(),
            error: None,
            sections: tasks::parse_notes(&String::from_utf8_lossy(&b)),
        },
        Err(e) => fail(e.to_string()),
    }
}

/// 할 일 쓰기 충돌: 동작 차례, 까닭, 대상 제목(없으면 빈 글)
pub(crate) type WriteConflict = (usize, String, String);
