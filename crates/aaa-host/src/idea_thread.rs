//! 아이디어 전용 thread(t-1260, 기획 t-1255 결정 1·16·19~22).
//!
//! - 아이디어마다 하나, 늘 host 머신(로컬 agent)의 작업 폴더 thread로 띄우고 실행 한도에 세지 않는다. 할 일을 남기지 않는다.
//! - 처음 말할 때 띄운다. 이야기가 없으면(쉼이 `[projects] idea_rest_secs` 넘게 이어지면) ticker가 조용히 보류한다(알림함 항목 없음).
//!   다시 말하거나 아이디어 화면을 열면 같은 대화로 잇는다(park.rs). 잇지 못하면 새 thread가 노트를 읽고 이어 간다(차선책).
//! - 보낸 말은 thread가 brief를 받은 뒤에 간다(harness.rs `thread_message`): 그 전까지 메타 `pending`에 두고 배경 일이 보낸다.
//! - 사람이 노트를 고친 요약은 다음 말 앞에 「(노트 고침: …)」으로 붙인다(결정 16). 겹친 사람 글은 thread에 합치기를 맡긴다(결정 11).
//! - 진행 모델(t-1581): 사람이 고른 profile을 메타에 둔다. 전용 thread의 profile과 다르면 다음 말 때 앞 thread의 답이 끝나기를
//!   기다려 끝내고, 고른 profile의 새 thread가 노트를 읽고 이어 간다(profile을 바꾸면 새 대화: `thread restart --profile`과 같다).

use std::sync::Arc;

use aaa_proto::{
    AgentActivity, DeviceRole, ErrorBody, ErrorCode, FrontRequest, FrontResponse, IdeaAuthor, IdeaNote, IdeaStatus,
    MessageMode, Role, ThreadKind, ThreadStatus,
};

use crate::ideas::{Folder, Saved};
use crate::orch::{ResolveOpts, ResolveWhy, StartArgs, ThreadRec, bad, internal};
use crate::{Host, err};

/// 보낸 말 한 개 상한(웹 입력창과 같다)
const MAX_SAY: usize = 100_000;
/// thread가 brief를 받기를 기다리는 상한. 넘으면 말은 남기고 다음 말·화면 열기 때 다시 보낸다
const DELIVER_WAIT: std::time::Duration = std::time::Duration::from_secs(600);

/// 쉬는 아이디어 thread를 내릴 때인가: 열려 있고, 받은 말을 다 보냈고, agent가 `rest_ms` 넘게 쉬었다
pub(crate) fn should_rest(r: &ThreadRec, agent: Option<&aaa_proto::AgentStatus>, now: u64, rest_ms: u64) -> bool {
    r.idea.is_some()
        && r.status == ThreadStatus::Open
        && r.park.is_none()
        && r.brief == aaa_proto::BriefState::Delivered
        && agent.is_some_and(|a| a.activity == AgentActivity::Idle && now.saturating_sub(a.since_ms) >= rest_ms)
}

/// 고른 진행 모델이 전용 thread의 profile과 달라 새 thread로 바꿔야 하나. 고르지 않았으면 프로젝트 기본이 바뀌어도 그대로 둔다
pub(crate) fn model_switch(chosen: Option<&str>, thread_profile: &str) -> bool {
    chosen.is_some_and(|p| p != thread_profile)
}

/// 전용 thread를 왜 새로 띄우나
#[derive(Debug, Clone, PartialEq, Eq)]
enum Begin {
    /// 처음 말했다
    New,
    /// 앞 thread를 잇지 못했다
    Again,
    /// 사람이 진행 모델을 바꿨다(앞 thread의 profile)
    Model(String),
}

/// 새 thread에 줄 할 일 글(brief 끝). 이어 받는 thread는 앞 대화를 못 보므로 노트부터 읽는다
fn task_text(short: &str, title: &str, dir: &std::path::Path, begin: &Begin) -> String {
    let start = match begin {
        Begin::Again => {
            "앞 대화를 이을 수 없어 새로 시작했다. 노트가 지금까지의 정리본이다: 먼저 읽고, 사용자의 다음 말을 기다린다.".to_string()
        }
        Begin::Model(from) => format!(
            "사용자가 진행 모델을 바꿔({from}에서) 새 대화로 시작했다. 앞 대화는 넘어오지 않는다. 노트가 지금까지의 정리본이다: \
             먼저 읽고 사용자의 다음 말에 답한다."
        ),
        Begin::New => "새 아이디어다. 노트를 읽고 사용자의 말을 기다린다.".to_string(),
    };
    format!(
        "아이디어 {short} 「{title}」의 전용 thread다. {start}\n\n- 노트 읽기: `aaa idea show {short}`\n- 노트 저장: \
         `aaa idea save {short} --base <판> --file <파일> --summary '<바뀐 곳>'`\n- 그림 붙이기(노트 끝에 새 판): \
         `aaa idea attach {short} <파일>... --alt '<설명>'`(생성·내려받은 그림도 작업 폴더에 둔 뒤 붙인다. 노트 가운데에 넣으려면 \
         `--store-only`로 받은 이름을 `![설명](<이름>)`로 적어 저장)\n- 노트 폴더(그림·영상, 읽기만): {}\n",
        dir.display()
    )
}

impl Host {
    /// 아이디어의 await 경로를 공용 dispatcher에서 분리해 다른 요청의 poll 스택이 함께 커지지 않게 한다.
    /// 공용 입구의 권한 검사·요청 가림을 통과한 요청만 받는다.
    pub(crate) async fn call_idea(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        req: FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        match req {
            FrontRequest::IdeaList { project } => Ok(FrontResponse::Ideas { ideas: self.idea_list(&project)? }),
            FrontRequest::IdeaRead { project, idea, rev } => {
                let f = self.idea_folder(&project, &idea)?;
                Ok(FrontResponse::Idea { note: self.idea_read_note(&project, &f, rev).await? })
            }
            FrontRequest::IdeaSeen { project, idea, rev } => {
                if actor.starts_with("worker:")
                    || actor.starts_with("evaluator:")
                    || crate::front::in_session(actor)
                    || actor.starts_with("detached:")
                {
                    return Err(err(ErrorCode::Forbidden, "thread는 사람의 읽음 상태를 바꾸지 못한다"));
                }
                self.idea_seen(&project, &idea, rev)?;
                Ok(FrontResponse::Done)
            }
            FrontRequest::IdeaCreate { project, title, text } => {
                Ok(FrontResponse::Idea { note: self.idea_create(&project, &title, &text, None, actor)? })
            }
            FrontRequest::IdeaSave { project, idea, base_rev, text, summary } => {
                self.idea_save(role, actor, &project, &idea, base_rev, &text, summary).await
            }
            FrontRequest::IdeaSay { project, idea, text } => {
                Ok(FrontResponse::Idea { note: self.idea_say(&project, &idea, &text, actor).await? })
            }
            FrontRequest::IdeaWake { project, idea } => {
                Ok(FrontResponse::Idea { note: self.idea_wake(&project, &idea, actor).await? })
            }
            FrontRequest::IdeaHandoff { project, idea, base_rev, tasks } => {
                Ok(FrontResponse::Idea { note: self.idea_handoff(&project, &idea, base_rev, tasks, actor).await? })
            }
            FrontRequest::IdeaCopy { project, idea } => {
                Ok(FrontResponse::Idea { note: self.idea_copy(&project, &idea, actor)? })
            }
            FrontRequest::IdeaModel { project, idea, profile } => {
                // 정해진 모델 변경은 사람만(thread_restart --profile과 같다): thread 세션 안 CLI·평가자는 바꾸지 못한다
                if actor.starts_with("worker:")
                    || actor.starts_with("evaluator:")
                    || crate::front::in_session(actor)
                    || actor.starts_with("detached:")
                {
                    return Err(err(ErrorCode::Forbidden, "진행 모델은 사용자가 웹이나 aaa 밖 CLI에서 직접 바꾼다"));
                }
                if !role.can_control() {
                    return Err(err(
                        ErrorCode::Forbidden,
                        format!("보기 권한으로는 진행 모델을 바꿀 수 없다{}", crate::front::reader_reason(actor)),
                    ));
                }
                Ok(FrontResponse::Idea { note: self.idea_model(&project, &idea, &profile, actor).await? })
            }
            FrontRequest::IdeaAttach { project, idea, names, paths, alt, section, store_only } => {
                let f = self.idea_folder(&project, &idea)?;
                let by = self.idea_writer(role, actor, &f)?;
                // 경로는 host 머신의 CLI만 준다: 웹 기기는 먼저 올린 이름으로 붙인다. 전용 thread는 자기 작업 폴더와 프로젝트 uploads/만
                let roots = if paths.is_empty() {
                    None
                } else if actor.starts_with("device:") {
                    return Err(err(ErrorCode::Forbidden, "웹은 파일을 올린 뒤 이름으로 붙인다"));
                } else if by == IdeaAuthor::Thread {
                    let work = f
                        .meta()?
                        .thread
                        .and_then(|t| self.load(&project, &t).ok())
                        .and_then(|r| r.work_dir)
                        .ok_or_else(|| bad("전용 thread의 작업 폴더를 모른다"))?;
                    Some(vec![std::path::PathBuf::from(work), self.project_dir(&project)?.join("uploads")])
                } else {
                    None
                };
                let label = self.idea_actor_label(actor);
                let (names, note) = self.idea_attach(
                    &project,
                    &f,
                    by,
                    &label,
                    actor,
                    &names,
                    &paths,
                    roots.as_deref(),
                    alt.as_deref(),
                    section.as_deref(),
                    store_only.unwrap_or(false),
                )?;
                Ok(FrontResponse::IdeaAttached { names, note })
            }
            _ => Err(internal("아이디어 요청이 아니다")),
        }
    }

    /// 아이디어마다의 작업 lock(띄우기·잇기를 한 번에 하나)
    fn idea_lock(&self, project: &str, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.thread_op_lock(project, &format!("idea:{id}"))
    }

    /// 전용 thread를 쓸 수 있게 한다: 열린 것은 그대로, 쉬느라 내린 것은 같은 대화로 잇고, 없거나 잇지 못하면(`start`일 때만)
    /// 새로 띄운다. 새 thread는 기록만 만들고 돌려준다(세션은 배경에서 뜬다). 띄울 것이 없으면 None
    async fn idea_thread_ready(
        self: &Arc<Self>,
        project: &str,
        f: &Folder,
        actor: &str,
        start: bool,
    ) -> Result<Option<String>, ErrorBody> {
        let lock = self.idea_lock(project, &f.id);
        let _one = lock.lock().await;
        let meta = f.meta()?;
        let mut begin = Begin::New;
        if let Some(id) = meta.thread.as_deref() {
            match self.load(project, id) {
                Ok(rec) if model_switch(meta.profile.as_deref(), &rec.profile) => {
                    // 화면 열기는 옛 모델의 thread를 잇지 않는다: 다음 말이 새 모델로 바꾼다
                    if !start {
                        return Ok(None);
                    }
                    self.idea_retire(project, &rec, actor).await?;
                    begin = Begin::Model(rec.profile.clone());
                }
                Ok(rec) => match rec.status {
                    ThreadStatus::Open | ThreadStatus::Starting => return Ok(Some(id.to_string())),
                    ThreadStatus::Parked => match self.thread_unpark(project, id, None, None, false, actor).await {
                        Ok(_) => return Ok(Some(id.to_string())),
                        Err(e) => {
                            tracing::warn!(
                                project,
                                thread = id,
                                "아이디어 thread를 잇지 못해 새로 띄운다: {}",
                                e.message
                            );
                            begin = Begin::Again;
                        }
                    },
                    ThreadStatus::Failed | ThreadStatus::Resolved => begin = Begin::Again,
                },
                Err(e) => {
                    tracing::warn!(project, thread = id, "아이디어 thread 기록을 읽지 못해 새로 띄운다: {}", e.message);
                    begin = Begin::Again;
                }
            }
        }
        // 화면 열기는 쉬는 thread만 잇는다: 새 thread는 처음 말할 때 띄운다
        if !start {
            return Ok(None);
        }
        let short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
        let args = StartArgs {
            project: project.to_string(),
            title: format!("아이디어 {short} {}", meta.title),
            // 결정 21: 늘 host 머신(잇기 쉽게). 로컬 agent가 없으면 resolve_machine이 까닭을 준다
            machine: None,
            // 고르지 않았으면 비워 프로젝트 기본 thread profile을 쓴다
            profile: meta.profile.clone().unwrap_or_default(),
            kind: ThreadKind::Dir,
            task: task_text(&short, &meta.title, &f.dir, &begin),
            role: Role::Worker,
            idea: Some(f.id.clone()),
            ..Default::default()
        };
        let view = self.thread_start_mode(args, actor, true).await?;
        {
            let _w = self.idea_writing()?;
            let mut meta = f.meta()?;
            meta.thread = Some(view.id.clone());
            f.write_meta(&meta)?;
        }
        self.ideas_bump(project);
        Ok(Some(view.id))
    }

    /// 진행 모델을 고른다: host 머신이 맡을 수 있는 profile(`assignable`과 같은 출처, 프로젝트 허용 목록 반영)만 받는다.
    /// 지금 thread는 건드리지 않는다. 다음 말 때(아직 보내지 못한 말이 있으면 바로) `idea_thread_ready`가 바꾼다
    pub(crate) async fn idea_model(
        self: &Arc<Self>,
        project: &str,
        idea: &str,
        profile: &str,
        actor: &str,
    ) -> Result<IdeaNote, ErrorBody> {
        let profile = profile.trim();
        if profile.is_empty() {
            return Err(bad("profile이 비었다"));
        }
        let f = self.idea_folder(project, idea)?;
        if f.meta()?.status == IdeaStatus::Done {
            return Err(bad("할 일로 넘겨 잠긴 아이디어다"));
        }
        self.check_profile_allowed(project, profile)?;
        let host = self
            .machine_views()
            .into_iter()
            .find(|m| m.local)
            .map(|m| m.name)
            .ok_or_else(|| bad("host 머신 agent가 없어 진행 모델을 고를 수 없다"))?;
        let owners = self.assignable(project).await;
        let usable = owners.into_iter().find(|o| o.machine == host).map(|o| o.profiles).unwrap_or_default();
        if !usable.iter().any(|p| p == profile) {
            return Err(bad(format!(
                "host 머신 {host}이 맡을 수 있는 profile이 아니다: {profile}(쓸 수 있는 것: {})",
                if usable.is_empty() { "없음".to_string() } else { usable.join(", ") }
            )));
        }
        let waiting = {
            let _w = self.idea_writing()?;
            let mut meta = f.meta()?;
            meta.profile = Some(profile.to_string());
            f.write_meta(&meta)?;
            !meta.pending.is_empty()
        };
        self.db.audit(actor, "idea_model", &format!("{project}/{idea} {profile}"));
        self.ideas_bump(project);
        // 아직 보내지 못한 말은 앞 모델에 보내지 않는다(배달 일이 멈춘다): 새 모델로 바꾸고 보낸다
        if waiting {
            self.spawn_idea_work(project, &f.id, true, actor);
        }
        self.idea_read_note(project, &f, None).await
    }

    /// 진행 모델을 바꾸려고 앞 thread를 끝낸다. 답하는 중이면 그 답이 끝나기를 기다린다(넘으면 말은 남기고 다음에 다시 한다).
    /// 이미 끝났거나 실패한 thread는 그대로 둔다
    async fn idea_retire(self: &Arc<Self>, project: &str, rec: &ThreadRec, actor: &str) -> Result<(), ErrorBody> {
        if matches!(rec.status, ThreadStatus::Resolved | ThreadStatus::Failed) {
            return Ok(());
        }
        if rec.status == ThreadStatus::Open
            && let Some(pane) = rec.pane.as_deref()
            && let Ok(mut rx) = self.pane_events(pane)
        {
            let busy = |h: &Host| {
                h.pane_view_of(pane, DeviceRole::Local).is_some_and(|v| v.state.is_running())
                    && h.pane_agent(pane).is_some_and(|a| a.activity == AgentActivity::Working)
            };
            let idle = async {
                while busy(self) {
                    if let Err(tokio::sync::broadcast::error::RecvError::Closed) = rx.recv().await {
                        return;
                    }
                }
            };
            if tokio::time::timeout(DELIVER_WAIT, idle).await.is_err() {
                return Err(err(
                    ErrorCode::Unavailable,
                    format!("{}가 아직 답하는 중이라 진행 모델을 바꾸지 못했다. 말은 남겨 두었다", rec.id),
                ));
            }
        }
        self.thread_resolve(project, &rec.id, ResolveOpts::default(), ResolveWhy::Manual, actor).await?;
        tracing::info!(project, thread = %rec.id, from = %rec.profile, "아이디어 진행 모델을 바꾸려고 앞 thread를 끝냈다");
        Ok(())
    }

    /// 사람이 전용 thread에 말한다(웹 대화 입력창). 고친 노트 요약을 앞에 붙이고, thread가 준비되면 보낸다
    pub(crate) async fn idea_say(
        self: &Arc<Self>,
        project: &str,
        idea: &str,
        text: &str,
        actor: &str,
    ) -> Result<IdeaNote, ErrorBody> {
        if text.trim().is_empty() {
            return Err(bad("글이 비었다"));
        }
        if text.len() > MAX_SAY {
            return Err(err(ErrorCode::TooLarge, format!("한 번에 {}자까지 보낸다", MAX_SAY)));
        }
        let f = self.idea_folder(project, idea)?;
        if f.meta()?.status == IdeaStatus::Done {
            return Err(bad("할 일로 넘겨 잠긴 아이디어다. 복사해서 새 아이디어로 다듬는다"));
        }
        {
            let _w = self.idea_writing()?;
            let mut meta = f.meta()?;
            let edited = std::mem::take(&mut meta.edited);
            // 웹은 보낼 글에 이미 머리를 붙인다(대화 끝의 보낸 말이 기록과 같게): 두 번 붙이지 않는다
            let line = if edited.is_empty() || text.starts_with("(노트 고침:") {
                text.to_string()
            } else {
                format!("(노트 고침: {})\n{text}", edited.join("; "))
            };
            meta.pending.push(line);
            meta.error = None;
            f.write_meta(&meta)?;
        }
        self.db.audit(actor, "idea_say", &format!("{project}/{idea} {} bytes", text.len()));
        self.ideas_bump(project);
        // 띄우기·잇기는 배경에서 한다(실제 harness는 몇 초 걸린다): 말은 남겨 두고 준비되면 보낸다
        self.spawn_idea_work(project, &f.id, true, actor);
        self.idea_read_note(project, &f, None).await
    }

    /// 아이디어 화면을 열었다: 쉬느라 내린 thread를 조용히 잇고, 남은 말이 있으면 보낸다. 아직 thread가 없으면 아무것도 하지 않는다
    pub(crate) async fn idea_wake(
        self: &Arc<Self>,
        project: &str,
        idea: &str,
        actor: &str,
    ) -> Result<IdeaNote, ErrorBody> {
        let f = self.idea_folder(project, idea)?;
        let meta = f.meta()?;
        if meta.status == IdeaStatus::Open && meta.thread.is_some() {
            self.spawn_idea_work(project, &f.id, false, actor);
        }
        self.idea_read_note(project, &f, None).await
    }

    /// 배경 일: thread를 쓸 수 있게 하고(`start`면 없을 때 새로), 남은 말을 brief 뒤 차례로 보낸다. 띄우지 못하면 까닭을 노트 메타에
    /// 남겨 화면이 보인다(말은 남는다: 다음 말·화면 열기 때 다시 한다). 기다림이 길면 말을 남겨 두고 끝낸다
    fn spawn_idea_work(self: &Arc<Self>, project: &str, id: &str, start: bool, actor: &str) {
        let (host, project, id, actor) = (self.clone(), project.to_string(), id.to_string(), actor.to_string());
        tokio::spawn(async move {
            // 아이디어마다 하나씩 차례로: 앞 일이 보내는 동안 온 말도 그 뒤에 이어 보낸다
            let lock = host.thread_op_lock(&project, &format!("idea-say:{id}"));
            let _one = lock.lock_owned().await;
            let Ok(f) = host.idea_folder(&project, &id) else { return };
            match host.idea_thread_ready(&project, &f, &actor, start).await {
                Ok(Some(_)) => {}
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!(project, idea = %id, "아이디어 thread를 띄우지 못했다: {}", e.message);
                    {
                        let Ok(_w) = host
                            .idea_writing()
                            .inspect_err(|e| tracing::error!("아이디어 오류 저장 잠금: {}", e.message))
                        else {
                            return;
                        };
                        if let Ok(mut meta) = f.meta() {
                            meta.error = Some(e.message);
                            aaa_common::diagnostic::ignored(f.write_meta(&meta), "화면에 까닭을 못 남겨도 로그에 있다");
                        }
                    }
                    host.ideas_bump(&project);
                    return;
                }
            }
            if let Err(e) = tokio::time::timeout(DELIVER_WAIT, host.idea_deliver(&project, &id)).await {
                tracing::warn!(project, idea = %id, "아이디어 thread가 준비되지 않아 말을 남겨 두었다: {e}");
            }
        });
    }

    async fn idea_deliver(self: &Arc<Self>, project: &str, id: &str) {
        let mut events = self.thread_events.subscribe();
        loop {
            let Ok(f) = self.idea_folder(project, id) else { return };
            let Ok(meta) = f.meta() else { return };
            let Some(next) = meta.pending.first().cloned() else { return };
            let Some(thread) = meta.thread.clone() else { return };
            let rec = self.load(project, &thread);
            // 그사이 진행 모델을 바꿨다: 앞 모델 thread에 보내지 않고 끝낸다. 뒤에 줄 선 배경 일이 thread를 바꾼 뒤 보낸다
            if rec.as_ref().is_ok_and(|r| model_switch(meta.profile.as_deref(), &r.profile)) {
                return;
            }
            let ready =
                rec.is_ok_and(|r| r.status == ThreadStatus::Open && r.brief == aaa_proto::BriefState::Delivered);
            if ready {
                match self.thread_message(project, &thread, next.clone(), MessageMode::Queue, "idea").await {
                    Ok(_) => {
                        let Ok(_w) = self
                            .idea_writing()
                            .inspect_err(|e| tracing::error!("아이디어 전달 기록 잠금: {}", e.message))
                        else {
                            return;
                        };
                        if let Ok(mut meta) = f.meta()
                            && meta.pending.first() == Some(&next)
                        {
                            meta.pending.remove(0);
                            aaa_common::diagnostic::ignored(f.write_meta(&meta), "다음 셈에 다시 보낸다");
                        }
                        drop(_w);
                        self.ideas_bump(project);
                        continue;
                    }
                    // 확인창·쓰던 글 같은 막힘: 다음 thread 사건 뒤에 다시 본다
                    Err(e) => tracing::info!(project, thread, "아이디어 말을 아직 보내지 못했다: {}", e.message),
                }
            }
            match events.recv().await {
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    }

    /// `idea_save`: 사람(조작 권한)이나 그 아이디어의 전용 thread만. thread의 겹침은 두 판을 돌려주고, 사람의 겹침은 thread에 맡긴다
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn idea_save(
        self: &Arc<Self>,
        role: aaa_proto::DeviceRole,
        actor: &str,
        project: &str,
        idea: &str,
        base_rev: u32,
        text: &str,
        summary: Option<String>,
    ) -> Result<FrontResponse, ErrorBody> {
        let f = self.idea_folder(project, idea)?;
        let by = self.idea_writer(role, actor, &f)?;
        let meta = f.meta()?;
        let text = if meta.rev == base_rev && meta.status == IdeaStatus::Open {
            self.idea_import_media(project, &f, text, &self.idea_actor_label(actor)).await?
        } else {
            text.to_string()
        };
        match self.idea_write(project, &f, base_rev, &text, summary, by, actor)? {
            Ok(note) => Ok(FrontResponse::Idea { note }),
            Err(Saved::Conflict(base_rev, base_text)) => self.idea_conflict(project, &f, base_rev, base_text),
            Err(Saved::Merge { base_rev, mine, current }) => {
                let short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
                let ask = format!(
                    "[aaa 노트 합치기] 사용자가 노트 판 {base_rev}을 기준으로 고친 글이 그사이 판 {current}과 겹쳤다. 사용자 글: {} . \
                     `aaa idea show {short}`(지금 판 {current})에 사용자 고침을 다시 얹어 `aaa idea save {short} --base {current} …`로 \
                     저장한다. 사용자 고침은 그대로 두고, 「바뀐 것」에 합친 것을 적는다.",
                    mine.display()
                );
                {
                    let _w = self.idea_writing()?;
                    let mut meta = f.meta()?;
                    meta.pending.push(ask);
                    f.write_meta(&meta)?;
                }
                // 사람 글은 이미 따로 두었다: 띄우기·보내기는 배경에서 한다(못 띄우면 까닭이 노트 메타에 남는다)
                self.spawn_idea_work(project, &f.id, true, actor);
                Ok(FrontResponse::Idea { note: self.idea_note(project, &f, None)? })
            }
            Err(Saved::Same | Saved::Written) => Err(internal("저장 결과가 맞지 않는다")),
        }
    }

    /// 그림 출처에 남길 들인 쪽: 전용 thread는 그 ID, coordinator pane 안 CLI는 `coordinator`, 나머지는 `사람`
    pub(crate) fn idea_actor_label(&self, actor: &str) -> String {
        if let Some(session) = actor.strip_prefix("worker:") {
            return self.thread_of_pane(session).map_or_else(|| "thread".into(), |r| r.id);
        }
        let coordinator = actor.strip_prefix("local:").and_then(|rest| rest.split_once('@')).is_some_and(|(_, s)| {
            self.pane_view_of(s, DeviceRole::Local).and_then(|p| p.role) == Some(Role::Coordinator)
                || self.thread_of_pane(s).is_some_and(|r| r.role == Role::Coordinator)
        });
        if coordinator { "coordinator".into() } else { "사람".into() }
    }

    /// 누가 쓰나: 조작 권한은 사람, 보기 권한은 그 아이디어의 전용 thread 세션만
    fn idea_writer(&self, role: aaa_proto::DeviceRole, actor: &str, f: &Folder) -> Result<IdeaAuthor, ErrorBody> {
        if let Some(session) = actor.strip_prefix("worker:") {
            let rec = self.thread_of_pane(session);
            return if rec.as_ref().and_then(|r| r.idea.as_deref()) == Some(f.id.as_str()) {
                Ok(IdeaAuthor::Thread)
            } else {
                Err(err(ErrorCode::Forbidden, "thread 세션 안에서는 자기 아이디어 노트만 저장한다"))
            };
        }
        if !role.can_control() {
            return Err(err(
                ErrorCode::Forbidden,
                format!("보기 권한으로는 노트를 고칠 수 없다{}", crate::front::reader_reason(actor)),
            ));
        }
        Ok(IdeaAuthor::User)
    }

    /// ticker: 이야기가 없는 아이디어 thread를 조용히 보류한다(결정 20)
    pub(crate) async fn idea_rest_pass(self: &Arc<Self>, project: &str) {
        let rest_ms = self.cfg.projects.idea_rest_secs.saturating_mul(1000);
        let now = aaa_common::util::now_ms();
        let resting: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .filter(|r| r.idea.is_some() && r.status == ThreadStatus::Open)
            .filter(|r| {
                let (live, agent) = self.thread_live(r);
                live && should_rest(r, agent.as_ref(), now, rest_ms)
            })
            .collect();
        for r in resting {
            // 보낼 말이 남았으면 내리지 않는다
            let waiting = r
                .idea
                .as_deref()
                .and_then(|i| self.idea_folder(project, i).ok())
                .and_then(|f| f.meta().ok())
                .is_some_and(|m| !m.pending.is_empty());
            if waiting {
                continue;
            }
            let request = aaa_proto::ThreadPark {
                actor: "ticker".into(),
                requested_at_ms: now,
                note: Some("아이디어 대화가 쉬어 내렸다".into()),
                user_request: None,
                immediate: true,
                report_before: r.report_hash.clone(),
                parked_at_ms: None,
                machine: self.machine_name(&r.machine_id),
                head: None,
                pushed: None,
                uncommitted: None,
                git_unknown: None,
                resumable: false,
            };
            if let Err(e) = self.park_close(project, &r.id, Some(request), "ticker").await {
                tracing::warn!(project, thread = %r.id, "쉬는 아이디어 thread를 내리지 못했다: {}", e.message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec() -> ThreadRec {
        let mut r = ThreadRec::starting("p", "아이디어 n-0001", "m", "claude", Role::Worker, ThreadKind::Dir, 0);
        r.idea = Some("01JD0000000000000000000000".into());
        r.status = ThreadStatus::Open;
        r.brief = aaa_proto::BriefState::Delivered;
        r
    }

    fn agent(activity: AgentActivity, since_ms: u64) -> aaa_proto::AgentStatus {
        serde_json::from_value(serde_json::json!({ "activity": activity, "since_ms": since_ms, "harness": "claude" }))
            .unwrap()
    }

    /// 노트 저장 권한: thread 세션 안 CLI(보기 권한)는 자기 아이디어 노트만 저장하고 겹치면 두 판을 받는다. 다른 thread·보기 기기는
    /// 못 고치고, thread 세션은 아이디어를 새로 만들지 못한다
    #[tokio::test]
    async fn only_the_ideas_own_thread_saves_from_a_session() {
        use aaa_proto::{DeviceRole, FrontRequest, FrontResponse};
        let state = std::env::temp_dir().join(format!("aaa-idea-auth-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = state.join("projects").display().to_string();
        let h = Host::open(cfg, state.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        let note = h.idea_create("demo", "알림", "", None, "test").unwrap();
        for (id, pane, idea) in [("t-0001", "pane-i", Some(note.idea.id.clone())), ("t-0002", "pane-o", None)] {
            let mut r = ThreadRec::starting("demo", "x", "m", "claude", Role::Worker, ThreadKind::Dir, 0);
            (r.id, r.status, r.pane, r.idea) = (id.into(), ThreadStatus::Open, Some(pane.into()), idea);
            h.db.thread_import("demo", id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
        }
        let save = |base_rev| FrontRequest::IdeaSave {
            project: "demo".into(),
            idea: "n-0001".into(),
            base_rev,
            text: "# 알림\n- thread 고침\n".into(),
            summary: Some("한 줄 더함".into()),
        };
        let own = h.call(DeviceRole::Reader, "worker:pane-i", save(1)).await.unwrap();
        assert!(
            matches!(own, FrontResponse::Idea { note } if note.rev == 2 && note.history[0].by == IdeaAuthor::Thread)
        );
        let stale = h.call(DeviceRole::Reader, "worker:pane-i", save(1)).await.unwrap();
        assert!(matches!(stale, FrontResponse::IdeaConflict(c) if c.base_rev == 1 && c.current.rev == 2));
        let seen = || FrontRequest::IdeaSeen { project: "demo".into(), idea: "n-0001".into(), rev: 2 };
        assert_eq!(h.call(DeviceRole::Reader, "worker:pane-i", seen()).await.unwrap_err().code, ErrorCode::Forbidden);
        assert!(h.idea_list("demo").unwrap()[0].unseen, "thread가 읽어도 사람의 새 글은 남는다");
        h.call(DeviceRole::Reader, "device:viewer", seen()).await.unwrap();
        assert!(!h.idea_list("demo").unwrap()[0].unseen);
        for actor in ["worker:pane-o", "device:viewer", "evaluator:pane-i"] {
            let e = h.call(DeviceRole::Reader, actor, save(2)).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::Forbidden, "{actor}");
        }
        let create = FrontRequest::IdeaCreate { project: "demo".into(), title: "새".into(), text: String::new() };
        assert_eq!(h.call(DeviceRole::Reader, "worker:pane-i", create).await.unwrap_err().code, ErrorCode::Forbidden);
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 진행 모델(t-1581): 고른 profile이 전용 thread와 다를 때만 바꾸고, 고르지 않았으면 프로젝트 기본이 바뀌어도 잇는다.
    /// 새 thread의 할 일 글은 앞 모델과 새 대화임을 알린다
    #[test]
    fn switches_only_when_the_chosen_model_differs() {
        assert!(model_switch(Some("sonnet-high"), "opus-high"));
        assert!(!model_switch(Some("opus-high"), "opus-high"));
        assert!(!model_switch(None, "opus-high"));
        let dir = std::path::Path::new("/ideas/x");
        let text = task_text("n-0007", "알림", dir, &Begin::Model("opus-high".into()));
        assert!(text.contains("진행 모델을 바꿔(opus-high에서) 새 대화로 시작했다"), "{text}");
        assert!(!task_text("n-0007", "알림", dir, &Begin::New).contains("진행 모델"));
    }

    /// 진행 모델 바꾸기는 사람만: thread 세션·보기 기기는 거절하고, 고르지 않은 아이디어는 프로젝트 기본 모델을 보인다.
    /// host 머신 agent가 없으면 고를 수 있는 profile을 모르니 저장하지 않는다
    #[tokio::test]
    async fn only_people_choose_the_idea_model() {
        use aaa_proto::{DeviceRole, FrontRequest};
        let state = std::env::temp_dir().join(format!("aaa-idea-model-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = state.join("projects").display().to_string();
        let h = Host::open(cfg, state.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        h.project_set("demo", "thread_profile", "claude", "test").unwrap();
        let note = h.idea_create("demo", "알림", "", None, "test").unwrap();
        assert_eq!(note.idea.profile.as_deref(), Some("claude"), "고르지 않으면 프로젝트 기본");
        // 고르지 않았으면 열린 전용 thread가 다음 말을 받는다: 프로젝트 기본과 달라도 그 thread의 profile을 보인다
        let mut r = ThreadRec::starting("demo", "x", "m", "codex", Role::Worker, ThreadKind::Dir, 0);
        (r.id, r.status, r.idea) = ("t-0001".into(), ThreadStatus::Open, Some(note.idea.id.clone()));
        h.db.thread_import("demo", "t-0001", "open", &serde_json::to_string(&r).unwrap()).unwrap();
        let f = h.idea_folder("demo", "n-0001").unwrap();
        let mut meta = f.meta().unwrap();
        meta.thread = Some("t-0001".into());
        f.write_meta(&meta).unwrap();
        assert_eq!(h.idea_list("demo").unwrap()[0].profile.as_deref(), Some("codex"), "열린 thread의 profile");
        let model =
            || FrontRequest::IdeaModel { project: "demo".into(), idea: "n-0001".into(), profile: "codex".into() };
        for (role, actor) in [
            (DeviceRole::Reader, "worker:pane-i"),
            (DeviceRole::Controller, "local:user@session-1"),
            (DeviceRole::Reader, "device:viewer"),
        ] {
            let e = h.call(role, actor, model()).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::Forbidden, "{actor}");
        }
        let e = h.call(DeviceRole::Controller, "device:phone", model()).await.unwrap_err();
        assert!(e.message.contains("host 머신 agent가 없어"), "{}", e.message);
        assert_eq!(h.idea_list("demo").unwrap()[0].profile.as_deref(), Some("codex"), "거절하면 그대로");
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 결정 20의 경계: 쉼이 정한 시간을 넘긴 열린 아이디어 thread만 내리고, 일하는 중·확인창·일반 thread는 두지 않는다
    #[test]
    fn rests_only_idle_idea_threads_after_the_window() {
        let r = rec();
        assert!(should_rest(&r, Some(&agent(AgentActivity::Idle, 1_000)), 1_000 + 1_800_000, 1_800_000));
        assert!(!should_rest(&r, Some(&agent(AgentActivity::Idle, 1_000)), 1_000 + 1_799_999, 1_800_000));
        assert!(!should_rest(&r, Some(&agent(AgentActivity::Working, 0)), u64::MAX, 1));
        assert!(!should_rest(&r, Some(&agent(AgentActivity::Question, 0)), u64::MAX, 1));
        assert!(!should_rest(&r, None, u64::MAX, 1));
        let mut plain = r.clone();
        plain.idea = None;
        assert!(!should_rest(&plain, Some(&agent(AgentActivity::Idle, 0)), u64::MAX, 1));
        let mut briefing = r;
        briefing.brief = aaa_proto::BriefState::Pending;
        assert!(!should_rest(&briefing, Some(&agent(AgentActivity::Idle, 0)), u64::MAX, 1));
    }
}
