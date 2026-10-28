//! 아이디어 전용 thread(t-1260, 기획 t-1255 결정 1·16·19~22).
//!
//! - 아이디어마다 하나, 늘 host 머신(로컬 agent)의 작업 폴더 thread로 띄우고 실행 한도에 세지 않는다. 할 일을 남기지 않는다.
//! - 처음 말할 때 띄운다. 이야기가 없으면(쉼이 `[projects] idea_rest_secs` 넘게 이어지면) ticker가 조용히 보류한다(알림함 항목 없음).
//!   다시 말하거나 아이디어 화면을 열면 같은 대화로 잇는다(park.rs). 잇지 못하면 새 thread가 노트를 읽고 이어 간다(차선책).
//! - 보낸 말은 thread가 brief를 받은 뒤에 간다(harness.rs `thread_message`): 그 전까지 메타 `pending`에 두고 배경 일이 보낸다.
//! - 사람이 노트를 고친 요약은 다음 말 앞에 「(노트 고침: …)」으로 붙인다(결정 16). 겹친 사람 글은 thread에 합치기를 맡긴다(결정 11).

use std::sync::Arc;

use aaa_proto::{
    AgentActivity, ErrorBody, ErrorCode, FrontResponse, IdeaAuthor, IdeaNote, IdeaStatus, MessageMode, Role,
    ThreadKind, ThreadStatus,
};

use crate::ideas::{Folder, Saved, WRITING};
use crate::orch::{StartArgs, ThreadRec, bad, internal};
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

/// 새 thread에 줄 할 일 글(brief 끝). 이어 받는 thread는 앞 대화를 못 보므로 노트부터 읽는다
fn task_text(short: &str, title: &str, dir: &std::path::Path, again: bool) -> String {
    let start = if again {
        "앞 대화를 이을 수 없어 새로 시작했다. 노트가 지금까지의 정리본이다: 먼저 읽고, 사용자의 다음 말을 기다린다."
    } else {
        "새 아이디어다. 노트를 읽고 사용자의 말을 기다린다."
    };
    format!(
        "아이디어 {short} 「{title}」의 전용 thread다. {start}\n\n- 노트 읽기: `aaa idea show {short}`\n- 노트 저장: \
         `aaa idea save {short} --base <판> --file <파일> --summary '<바뀐 곳>'`\n- 노트 폴더(그림·영상, 읽기만): {}\n",
        dir.display()
    )
}

impl Host {
    /// 아이디어마다의 작업 lock(띄우기·잇기를 한 번에 하나)
    fn idea_lock(&self, project: &str, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.thread_op_lock(project, &format!("idea:{id}"))
    }

    /// 전용 thread를 쓸 수 있게 한다: 열린 것은 그대로, 쉬느라 내린 것은 같은 대화로 잇고, 없거나 잇지 못하면 새로 띄운다
    async fn idea_thread_ready(self: &Arc<Self>, project: &str, f: &Folder, actor: &str) -> Result<String, ErrorBody> {
        let lock = self.idea_lock(project, &f.id);
        let _one = lock.lock().await;
        let meta = f.meta()?;
        let mut again = false;
        if let Some(id) = meta.thread.as_deref() {
            match self.load(project, id) {
                Ok(rec) => match rec.status {
                    ThreadStatus::Open | ThreadStatus::Starting => return Ok(id.to_string()),
                    ThreadStatus::Parked => match self.thread_unpark(project, id, None, None, false, actor).await {
                        Ok(_) => return Ok(id.to_string()),
                        Err(e) => {
                            tracing::warn!(
                                project,
                                thread = id,
                                "아이디어 thread를 잇지 못해 새로 띄운다: {}",
                                e.message
                            );
                            again = true;
                        }
                    },
                    ThreadStatus::Failed | ThreadStatus::Resolved => again = true,
                },
                Err(e) => {
                    tracing::warn!(project, thread = id, "아이디어 thread 기록을 읽지 못해 새로 띄운다: {}", e.message);
                    again = true;
                }
            }
        }
        let short = self.db.idea_short_id(project, &f.id).map_err(internal)?;
        let args = StartArgs {
            project: project.to_string(),
            title: format!("아이디어 {short} {}", meta.title),
            // 결정 21: 늘 host 머신(잇기 쉽게). 로컬 agent가 없으면 resolve_machine이 까닭을 준다
            machine: None,
            profile: String::new(),
            kind: ThreadKind::Dir,
            task: task_text(&short, &meta.title, &f.dir, again),
            role: Role::Worker,
            idea: Some(f.id.clone()),
            ..Default::default()
        };
        let view = self.thread_start(args, actor).await?;
        {
            let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
            let mut meta = f.meta()?;
            meta.thread = Some(view.id.clone());
            f.write_meta(&meta)?;
        }
        self.ideas_bump(project);
        Ok(view.id)
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
        // thread를 먼저 준비한다: 띄우지 못하면 말을 남기지 않고 까닭을 돌려준다(다시 보내도 겹치지 않게)
        self.idea_thread_ready(project, &f, actor).await?;
        {
            let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
            let mut meta = f.meta()?;
            let edited = std::mem::take(&mut meta.edited);
            // 웹은 보낼 글에 이미 머리를 붙인다(대화 끝의 보낸 말이 기록과 같게): 두 번 붙이지 않는다
            let line = if edited.is_empty() || text.starts_with("(노트 고침:") {
                text.to_string()
            } else {
                format!("(노트 고침: {})\n{text}", edited.join("; "))
            };
            meta.pending.push(line);
            f.write_meta(&meta)?;
        }
        self.db.audit(actor, "idea_say", &format!("{project}/{idea} {} bytes", text.len()));
        self.ideas_bump(project);
        self.spawn_idea_delivery(project, &f.id);
        self.idea_note(project, &f, None)
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
            self.idea_thread_ready(project, &f, actor).await?;
            if !meta.pending.is_empty() {
                self.spawn_idea_delivery(project, &f.id);
            }
        }
        self.idea_note(project, &f, None)
    }

    /// 남은 말을 thread가 brief를 받은 뒤 차례로 보낸다(배경). 기다림이 길면 말을 남겨 두고 끝낸다
    fn spawn_idea_delivery(self: &Arc<Self>, project: &str, id: &str) {
        let (host, project, id) = (self.clone(), project.to_string(), id.to_string());
        tokio::spawn(async move {
            let lock = host.thread_op_lock(&project, &format!("idea-say:{id}"));
            // 이미 보내는 일이 있으면 그 일이 새 말까지 보낸다
            let Ok(_one) = lock.try_lock_owned() else { return };
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
            let ready = self
                .load(project, &thread)
                .is_ok_and(|r| r.status == ThreadStatus::Open && r.brief == aaa_proto::BriefState::Delivered);
            if ready {
                match self.thread_message(project, &thread, next.clone(), MessageMode::Queue, "idea").await {
                    Ok(_) => {
                        let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
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
        match self.idea_write(project, &f, base_rev, text, summary, by, actor)? {
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
                    let _w = WRITING.lock().unwrap_or_else(|e| e.into_inner());
                    let mut meta = f.meta()?;
                    meta.pending.push(ask);
                    f.write_meta(&meta)?;
                }
                // 사람 글은 이미 따로 두었다: thread를 띄우지 못해도 저장은 실패가 아니다(다음 말·화면 열기 때 다시 맡긴다)
                match self.idea_thread_ready(project, &f, actor).await {
                    Ok(_) => self.spawn_idea_delivery(project, &f.id),
                    Err(e) => {
                        tracing::warn!(project, idea = %short, "합치기를 맡길 thread를 띄우지 못했다: {}", e.message)
                    }
                }
                Ok(FrontResponse::Idea { note: self.idea_note(project, &f, None)? })
            }
            Err(Saved::Same | Saved::Written) => Err(internal("저장 결과가 맞지 않는다")),
        }
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
        for actor in ["worker:pane-o", "device:viewer", "evaluator:pane-i"] {
            let e = h.call(DeviceRole::Reader, actor, save(2)).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::Forbidden, "{actor}");
        }
        let create = FrontRequest::IdeaCreate { project: "demo".into(), title: "새".into(), text: String::new() };
        assert_eq!(h.call(DeviceRole::Reader, "worker:pane-i", create).await.unwrap_err().code, ErrorCode::Forbidden);
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
