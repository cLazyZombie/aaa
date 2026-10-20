//! 감시 source가 판정한 사건을 발행하는 경계. 조회·판정·깨움 정책은 source가 정한다.
//! 사건 ID는 source가 내구 outbox에 먼저 저장한다. 재발행은 같은 ID로 중복을 막는다.
use crate::{Host, err};
use aaa_proto::{ErrorBody, ErrorCode, FlowEvent};

pub(crate) struct Event<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub at_ms: u64,
    pub owners: &'a [String],
    pub repo: Option<&'a str>,
    pub detail: String,
}
impl Host {
    /// 프로젝트 이력과(있으면) 이미 끝난 주인 thread 이력에 같은 사건을 남긴다.
    /// 입력 전달은 source가 공통 자동화 큐에 확정한다.
    pub(crate) fn publish_watch_event(&self, project: &str, event: Event<'_>) -> Result<(), ErrorBody> {
        let _order = self.flow_order.lock().unwrap_or_else(|p| p.into_inner());
        for owner in std::iter::once(None).chain(event.owners.iter().map(Some)) {
            let mut flow = FlowEvent {
                actor: None,
                project: project.into(),
                seq: 0,
                at_ms: event.at_ms,
                boot_id: self.boot_id.clone(),
                kind: event.kind.into(),
                thread: owner.cloned(),
                repo: event.repo.map(str::to_string),
                role: None,
                profile: None,
                machine: None,
                evidence: owner.map(|id| format!("threads/{id}.md")),
                detail: event.detail.clone(),
            };
            let key = format!("watch_event:{project}:{}:{}", event.id, owner.map(String::as_str).unwrap_or("project"));
            if let Some(seq) =
                self.db.flow_insert_once(&flow, &key).map_err(|e| err(ErrorCode::Internal, e.to_string()))?
            {
                flow.seq = seq;
                let _ = self.flow_events.send(flow);
            }
        }
        Ok(())
    }
}
