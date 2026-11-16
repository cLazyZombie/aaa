//! 옛 아이디어 요청과 대화 전용 thread의 쉼(t-1260, t-1894 4단계).
//!
//! - 아이디어는 저장소 기획 문서로 옮겼다(q-3982 3A·q-3985 6A): 노트 읽기(목록·보기·읽음)와 「기획으로 옮기기」만 남고, 만들기·저장·말·
//!   깨우기·할 일로 넘기기·첨부·모델·복사 요청은 `RETIRED`로 거절한다(옛 CLI·웹이 보내도 알아듣는 까닭을 받는다).
//! - 열린 아이디어 전용 thread는 ticker(`idea_rest_pass`)가 끝낸다(q-3983 4A). 기록과 대화는 남는다.
//! - 대화 전용 thread(기획 대화, 남은 아이디어 thread)는 이야기가 없으면(쉼이 `[projects] idea_rest_secs` 넘게 이어지면) ticker가
//!   조용히 보류한다(알림함 항목 없음).

use std::sync::Arc;

use aaa_proto::{AgentActivity, DeviceRole, ErrorBody, ErrorCode, FrontRequest, FrontResponse, Role, ThreadStatus};

use crate::orch::{ResolveOpts, ResolveWhy, ThreadRec, internal};
use crate::{Host, err};

/// 거둔 아이디어 쓰기 요청의 답
pub(crate) const RETIRED: &str = "옛 아이디어는 읽기 전용이다(아이디어는 저장소 기획 문서로 옮겼다): 기획은 `aaa plan`·웹 「기획」에서 \
                                  다듬고, 옛 아이디어는 웹 「기획으로 옮기기」로 그 저장소 기획 대화에 넘긴다";

/// 끝내지 못한 옛 아이디어 thread를 다시 끝내 보는 간격
const END_RETRY_MS: u64 = 5 * 60_000;

/// 쉬는 대화 전용 thread(아이디어·기획 대화)를 내릴 때인가: 열려 있고, 받은 말을 다 보냈고, agent가 `rest_ms` 넘게 쉬었다
pub(crate) fn should_rest(r: &ThreadRec, agent: Option<&aaa_proto::AgentStatus>, now: u64, rest_ms: u64) -> bool {
    r.chat_only()
        && r.status == ThreadStatus::Open
        && r.park.is_none()
        && r.brief == aaa_proto::BriefState::Delivered
        && agent.is_some_and(|a| a.activity == AgentActivity::Idle && now.saturating_sub(a.since_ms) >= rest_ms)
}

/// 끝낼 옛 아이디어 thread: 아이디어 표지가 있고 아직 끝나거나 실패하지 않았다
fn idea_thread_open(r: &ThreadRec) -> bool {
    r.idea.is_some() && !matches!(r.status, ThreadStatus::Resolved | ThreadStatus::Failed)
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
                // thread 세션(다른 머신에서 전달된 것 포함)은 읽기만 한다: 옛 그림 이행(노트 다시 쓰기·agent 읽기)은 사람이 열 때 한다
                if actor.starts_with("worker:") {
                    return Ok(FrontResponse::Idea { note: self.idea_note(&project, &f, rev)? });
                }
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
            FrontRequest::IdeaMove { project, idea, rev, repo, slug, title, place } => {
                Box::pin(self.idea_move(role, actor, &project, &idea, rev, &repo, &slug, title, &place)).await
            }
            FrontRequest::IdeaCreate { .. }
            | FrontRequest::IdeaSave { .. }
            | FrontRequest::IdeaSay { .. }
            | FrontRequest::IdeaWake { .. }
            | FrontRequest::IdeaHandoff { .. }
            | FrontRequest::IdeaCopy { .. }
            | FrontRequest::IdeaModel { .. }
            | FrontRequest::IdeaAttach { .. } => Err(err(ErrorCode::Conflict, RETIRED)),
            _ => Err(internal("아이디어 요청이 아니다")),
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

    fn chat_threads(&self, project: &str) -> Vec<ThreadRec> {
        self.db
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
            .filter(ThreadRec::chat_only)
            .collect()
    }

    /// 열린 옛 아이디어 thread를 끝낸다(q-3983 4A). agent가 없거나 실패하면 `END_RETRY_MS` 뒤에 다시 한다.
    /// 대화 전용 thread라 끝남 알림·후속 확인 카드는 만들지 않는다
    async fn idea_threads_end(self: &Arc<Self>, project: &str, chats: &[ThreadRec], now: u64) {
        for r in chats.iter().filter(|r| idea_thread_open(r)) {
            {
                let key = format!("end:{project}/{}", r.id);
                let mut tried = self.idea_move_checked.lock().unwrap();
                if tried.get(&key).is_some_and(|at| now.saturating_sub(*at) < END_RETRY_MS) {
                    continue;
                }
                tried.insert(key, now);
            }
            match Box::pin(self.thread_resolve(project, &r.id, ResolveOpts::default(), ResolveWhy::Manual, "host"))
                .await
            {
                Ok(_) => {
                    tracing::info!(project, thread = %r.id, "옛 아이디어 thread를 끝냈다(아이디어는 기획으로 옮겼다)");
                    self.db.audit("host", "idea_thread_end", &format!("{project}/{}", r.id));
                }
                Err(e) => tracing::warn!(project, thread = %r.id, "옛 아이디어 thread를 끝내지 못했다: {}", e.message),
            }
        }
    }

    /// ticker: 열린 옛 아이디어 thread를 끝내고, 이야기가 없는 대화 전용 thread(기획 대화)를 조용히 보류한다(결정 20).
    /// thread 기록은 한 번만 읽는다
    pub(crate) async fn idea_rest_pass(self: &Arc<Self>, project: &str) {
        let rest_ms = self.cfg.projects.idea_rest_secs.saturating_mul(1000);
        let now = aaa_common::util::now_ms();
        let chats = self.chat_threads(project);
        self.idea_threads_end(project, &chats, now).await;
        let resting: Vec<ThreadRec> = chats
            .into_iter()
            .filter(|r| r.idea.is_none() && r.status == ThreadStatus::Open)
            .filter(|r| {
                let (live, agent) = self.thread_live(r);
                live && should_rest(r, agent.as_ref(), now, rest_ms)
            })
            .collect();
        for r in resting {
            // 보낼 말이 남았으면 내리지 않는다
            if r.design.as_deref().is_some_and(|d| self.design_chat_waiting(project, d)) {
                continue;
            }
            let request = aaa_proto::ThreadPark {
                actor: "ticker".into(),
                requested_at_ms: now,
                note: Some(
                    if r.design.is_some() {
                        "기획 대화가 쉬어 내렸다"
                    } else {
                        "아이디어 대화가 쉬어 내렸다"
                    }
                    .into(),
                ),
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
                tracing::warn!(project, thread = %r.id, "쉬는 대화 thread를 내리지 못했다: {}", e.message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::ThreadKind;

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

    /// 거둔 쓰기 요청은 누가 보내도 까닭과 함께 거절하고 노트는 그대로다. 읽기는 남고, 옮기기는 thread 세션·보기 기기가 못 한다
    #[tokio::test]
    async fn idea_writes_are_retired_and_reads_stay() {
        let (h, state) = crate::ideas::tests::host_with_fixture_ideas();
        let p = || "demo".to_string();
        let i = || "n-0001".to_string();
        let retired = [
            FrontRequest::IdeaCreate { project: p(), title: "새".into(), text: String::new() },
            FrontRequest::IdeaSave { project: p(), idea: i(), base_rev: 1, text: "x".into(), summary: None },
            FrontRequest::IdeaSay { project: p(), idea: i(), text: "x".into() },
            FrontRequest::IdeaWake { project: p(), idea: i() },
            FrontRequest::IdeaHandoff { project: p(), idea: i(), base_rev: 1, tasks: vec![] },
            FrontRequest::IdeaCopy { project: p(), idea: i() },
            FrontRequest::IdeaModel { project: p(), idea: i(), profile: "claude".into() },
            FrontRequest::IdeaAttach {
                project: p(),
                idea: i(),
                names: vec![],
                paths: vec![],
                alt: None,
                section: None,
                store_only: None,
            },
        ];
        let before = h.idea_list("demo").unwrap();
        for req in retired {
            // 옛 아이디어 thread가 하던 저장·첨부(보기 권한)도 까닭을 받는다. 그 밖의 쓰기는 보기 권한이면 권한에서 막힌다
            let thread_wrote = matches!(req, FrontRequest::IdeaSave { .. } | FrontRequest::IdeaAttach { .. });
            let e = h.call(DeviceRole::Reader, "worker:pane-i", req.clone()).await.unwrap_err();
            if thread_wrote {
                assert_eq!((e.code, e.message.as_str()), (ErrorCode::Conflict, RETIRED), "{req:?}");
            } else {
                assert_eq!(e.code, ErrorCode::Forbidden, "{req:?}: {e:?}");
            }
            let e = h.call(DeviceRole::Local, "local:jo@cli", req.clone()).await.unwrap_err();
            assert_eq!((e.code, e.message.as_str()), (ErrorCode::Conflict, RETIRED), "{req:?}");
        }
        assert_eq!(h.idea_list("demo").unwrap(), before, "노트·메타는 그대로");
        let read = FrontRequest::IdeaRead { project: p(), idea: i(), rev: None };
        assert!(matches!(h.call(DeviceRole::Reader, "device:viewer", read).await.unwrap(), FrontResponse::Idea { .. }));
        let mv = FrontRequest::IdeaMove {
            project: p(),
            idea: i(),
            rev: 1,
            repo: "pt".into(),
            slug: "idea-n-0001".into(),
            title: None,
            place: String::new(),
        };
        for (role, actor) in [(DeviceRole::Reader, "worker:pane-i"), (DeviceRole::Reader, "device:viewer")] {
            let e = h.call(role, actor, mv.clone()).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::Forbidden, "{actor}: {e:?}");
        }
        let _ = std::fs::remove_dir_all(&state);
    }

    /// 끝낼 것은 열린(보류·시작 중 포함) 옛 아이디어 thread뿐이다
    #[test]
    fn ends_only_open_idea_threads() {
        let mut r = rec();
        assert!(idea_thread_open(&r));
        r.status = ThreadStatus::Parked;
        assert!(idea_thread_open(&r));
        for s in [ThreadStatus::Resolved, ThreadStatus::Failed] {
            r.status = s;
            assert!(!idea_thread_open(&r));
        }
        let mut design = rec();
        (design.idea, design.design) = (None, Some("pt/boss-dex".into()));
        assert!(!idea_thread_open(&design), "기획 대화는 끝내지 않는다");
    }

    /// 결정 20의 경계: 쉼이 정한 시간을 넘긴 열린 대화 전용 thread만 내리고, 일하는 중·확인창·일반 thread는 두지 않는다
    #[test]
    fn rests_only_idle_chat_threads_after_the_window() {
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
