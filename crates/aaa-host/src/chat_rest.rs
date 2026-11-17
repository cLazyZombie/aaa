//! 대화 전용 thread(기획 대화)의 쉼(t-1260 결정 20, t-1894 2-2).
//!
//! - 기획 대화 thread는 이야기가 없으면(쉼이 `[projects] chat_rest_secs` 넘게 이어지면) ticker가 조용히 보류한다(알림함 항목 없음).
//!   보낼 말이 남았으면 내리지 않는다.

use std::sync::Arc;

use aaa_proto::{AgentActivity, ThreadStatus};

use crate::Host;
use crate::orch::ThreadRec;

/// 쉬는 대화 전용 thread를 내릴 때인가: 열려 있고, 받은 말을 다 보냈고, agent가 `rest_ms` 넘게 쉬었다
pub(crate) fn should_rest(r: &ThreadRec, agent: Option<&aaa_proto::AgentStatus>, now: u64, rest_ms: u64) -> bool {
    r.chat_only()
        && r.status == ThreadStatus::Open
        && r.park.is_none()
        && r.brief == aaa_proto::BriefState::Delivered
        && agent.is_some_and(|a| a.activity == AgentActivity::Idle && now.saturating_sub(a.since_ms) >= rest_ms)
}

impl Host {
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

    /// ticker: 이야기가 없는 대화 전용 thread(기획 대화)를 조용히 보류한다(결정 20). thread 기록은 한 번만 읽는다
    pub(crate) async fn chat_rest_pass(self: &Arc<Self>, project: &str) {
        let rest_ms = self.cfg.projects.chat_rest_secs.saturating_mul(1000);
        let now = aaa_common::util::now_ms();
        let resting: Vec<ThreadRec> = self
            .chat_threads(project)
            .into_iter()
            .filter(|r| r.status == ThreadStatus::Open)
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
                note: Some("기획 대화가 쉬어 내렸다".into()),
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
    use aaa_proto::{Role, ThreadKind};

    fn rec() -> ThreadRec {
        let mut r = ThreadRec::starting("p", "기획 대화", "m", "claude", Role::Worker, ThreadKind::Dir, 0);
        r.design = Some("pt/boss-dex".into());
        r.status = ThreadStatus::Open;
        r.brief = aaa_proto::BriefState::Delivered;
        r
    }

    fn agent(activity: AgentActivity, since_ms: u64) -> aaa_proto::AgentStatus {
        serde_json::from_value(serde_json::json!({ "activity": activity, "since_ms": since_ms, "harness": "claude" }))
            .unwrap()
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
        plain.design = None;
        assert!(!should_rest(&plain, Some(&agent(AgentActivity::Idle, 0)), u64::MAX, 1));
        let mut briefing = r;
        briefing.brief = aaa_proto::BriefState::Pending;
        assert!(!should_rest(&briefing, Some(&agent(AgentActivity::Idle, 0)), u64::MAX, 1));
    }
}
