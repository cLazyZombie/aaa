//! 작업 중인 harness의 이전 Working 상태를 새 본문의 수락 증거로 쓰지 않는다.
use aaa_proto::{AgentRequest, AgentResponse, ChatMessage, ChatRole};
use sha2::{Digest, Sha256};

use crate::Host;

pub(crate) struct ReceiptPage {
    generation: u64,
    session: String,
    history: String,
    messages: Vec<ChatMessage>,
}

pub(crate) struct ReceiptProbe {
    generation: u64,
    session: String,
    history: String,
    sha256: String,
    last_id: Option<String>,
    pending: usize,
}

impl ReceiptProbe {
    pub(crate) fn new(page: ReceiptPage, text: &str) -> Self {
        let text: String = text.chars().filter(|c| !matches!(c, '\x1b' | '\u{9b}')).collect();
        let sha256 = format!("{:x}", Sha256::digest(aaa_proto::input::receipt_text(&text).as_bytes()));
        let pending = page.messages.iter().filter(|m| m.pending && matches_hash(m, &sha256)).count();
        Self {
            generation: page.generation,
            session: page.session,
            history: page.history,
            sha256,
            last_id: page.messages.iter().filter_map(|m| m.id.clone()).max(),
            pending,
        }
    }

    pub(crate) fn observed(&self, page: &ReceiptPage) -> Option<&'static str> {
        if self.generation != page.generation || self.session != page.session || self.history != page.history {
            return None;
        }
        let delivered = page
            .messages
            .iter()
            .filter(|m| {
                !m.pending
                    && matches_hash(m, &self.sha256)
                    && m.id.as_ref().is_some_and(|id| self.last_id.as_ref().is_none_or(|last| id > last))
            })
            .count();
        // 같은 본문이 이미 큐에 있었으면 그것의 소비를 이번 보내기의 수락으로 세지 않는다.
        if delivered > self.pending {
            return Some("transcript");
        }
        // Claude enqueue는 파일 위치 ID가 있다. 같은 본문이 이미 큐에 있어도
        // 기준 뒤에 새로 추가된 항목이면 이번 입력의 수락 증거다.
        page.messages
            .iter()
            .any(|m| {
                m.pending
                    && matches_hash(m, &self.sha256)
                    && match &m.id {
                        Some(id) => self.last_id.as_ref().is_none_or(|last| id > last),
                        None => self.pending == 0,
                    }
            })
            .then_some("queued_receipt")
    }
}

fn matches_hash(message: &ChatMessage, sha256: &str) -> bool {
    message.role == ChatRole::User && message.receipt.as_ref().is_some_and(|r| r.sha256 == sha256)
}

impl Host {
    pub(crate) async fn prompt_receipt_page(
        &self,
        mid: &str,
        pane: &str,
        deadline: tokio::time::Instant,
    ) -> Option<ReceiptPage> {
        let (harness, transcript, session, generation) = {
            let inner = self.inner.lock().unwrap();
            let p = inner.panes.get(pane)?;
            let a = p.agent.as_ref()?;
            if !matches!(a.harness.as_str(), "claude" | "codex")
                || !inner.machines.get(mid)?.features.iter().any(|f| f == "chat_pages.v1")
            {
                return None;
            }
            (a.harness.clone(), p.transcript.clone(), a.harness_session.clone()?, p.info.started_at_ms)
        };
        let conn = self.conn_of(mid)?;
        let response = tokio::time::timeout_at(
            deadline,
            conn.request(AgentRequest::Chat {
                session: Some(pane.into()),
                harness,
                transcript,
                harness_session: Some(session.clone()),
                thinking: false,
                compact: false,
                commands: false,
                before: None,
            }),
        )
        .await;
        match response {
            Ok(Ok(AgentResponse::Chat { messages, history: Some(history), .. })) => {
                // 응답을 기다리는 동안 실행이 바뀌면 옛 본문이 새 보내기를 수락하지 못한다.
                let same_generation =
                    self.inner.lock().unwrap().panes.get(pane).is_some_and(|p| p.info.started_at_ms == generation);
                (same_generation && self.pane_agent(pane)?.harness_session.as_ref() == Some(&session))
                    .then_some(ReceiptPage { generation, session, history, messages })
            }
            Ok(Err(error)) => {
                tracing::debug!(code = ?error.code, "입력 수락 대화 기록 조회 실패");
                None
            }
            Err(_) | Ok(Ok(_)) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{ChatReceipt, InputReceiptMethod};

    #[test]
    fn only_new_matching_receipts_in_the_same_execution_accept_a_submission() {
        let message = |id: &str, text: &str, pending| ChatMessage {
            id: (!id.is_empty()).then(|| id.into()),
            role: ChatRole::User,
            text: "표시 축약".into(),
            pending,
            receipt: Some(ChatReceipt {
                sha256: format!("{:x}", Sha256::digest(text.as_bytes())),
                method: InputReceiptMethod::Normal,
            }),
        };
        let page = |session: &str, history: &str, messages| ReceiptPage {
            generation: 1,
            session: session.into(),
            history: history.into(),
            messages,
        };
        let old = message("file:0001", "같은 본문", false);
        let probe = ReceiptProbe::new(page("s1", "file", vec![old.clone()]), "같은 본문");
        assert_eq!(probe.observed(&page("s1", "file", vec![old.clone()])), None);
        assert_eq!(probe.observed(&page("s1", "file", vec![message("file:0002", "다른 본문", false)])), None);
        assert_eq!(probe.observed(&page("s2", "file", vec![message("file:0002", "같은 본문", false)])), None);
        assert_eq!(probe.observed(&page("s1", "other", vec![message("file:0002", "같은 본문", false)])), None);
        assert_eq!(
            probe.observed(&page("s1", "file", vec![message("file:0002", "같은 본문", false)])),
            Some("transcript")
        );
        assert_eq!(
            probe.observed(&page("s1", "file", vec![old, message("", "같은 본문", true)])),
            Some("queued_receipt")
        );
        let mut next_generation = page("s1", "file", vec![message("file:0002", "같은 본문", false)]);
        next_generation.generation = 2;
        assert_eq!(probe.observed(&next_generation), None);
        let prior = message("file:0001:queue", "같은 본문", true);
        let identified_queue = ReceiptProbe::new(page("s1", "file", vec![prior.clone()]), "같은 본문");
        assert_eq!(identified_queue.observed(&page("s1", "file", vec![prior.clone()])), None);
        assert_eq!(
            identified_queue.observed(&page("s1", "file", vec![prior, message("file:0002:queue", "같은 본문", true)])),
            Some("queued_receipt")
        );
        let queued = ReceiptProbe::new(page("s1", "file", vec![message("", "같은 본문", true)]), "같은 본문");
        assert_eq!(queued.observed(&page("s1", "file", vec![message("file:0002", "같은 본문", false)])), None);
        assert_eq!(
            queued.observed(&page(
                "s1",
                "file",
                vec![message("file:0002", "같은 본문", false), message("", "같은 본문", true)]
            )),
            None
        );
        assert_eq!(
            queued.observed(&page(
                "s1",
                "file",
                vec![message("file:0002", "같은 본문", false), message("file:0003", "같은 본문", false)]
            )),
            Some("transcript")
        );
    }
}
