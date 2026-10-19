//! 입력 사건은 host DB에 최대 10,000행·30일·20MiB, 상세 1,024자 이내로 보관한다.
//! stdout은 기존 host 감독자의 로그 정책을 그대로 따른다. 조회에는 원문이 없다.
use crate::{Host, err};
use aaa_proto::{AgentActivity, ErrorBody, ErrorCode, FrontResponse, InputEvent};

impl Host {
    pub(crate) fn input_event(&self, pane: &str, id: &str, event: &str, detail: &str) {
        let agent = self.pane_agent(pane);
        let thread = self.thread_of_pane(pane).map(|t| format!("{}/{}", t.project, t.id));
        let row = InputEvent {
            at_ms: aaa_common::util::now_ms(),
            id: if id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b':') {
                id.to_owned()
            } else {
                aaa_common::util::new_id()
            },
            pane: pane.to_owned(),
            thread,
            harness: agent.as_ref().map(|a| a.harness.clone()),
            working: agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Working),
            event: event.to_owned(),
            detail: detail.chars().take(1024).collect(),
        };
        tracing::info!(input_id = %row.id, pane, harness = ?row.harness, working = row.working, event, detail = %row.detail, "입력 사건");
        if let Err(e) = self.db.input_event(&row) {
            tracing::warn!("입력 사건 저장 실패: {e}");
        }
    }

    pub(crate) fn debug_input(
        &self,
        pane: Option<String>,
        thread: Option<String>,
        since_ms: u64,
    ) -> Result<FrontResponse, ErrorBody> {
        let pane = pane.map(|p| self.resolve_pane(&p).map(|(id, _)| id)).transpose()?;
        let events = self
            .db
            .input_events(pane.as_deref(), thread.as_deref(), since_ms)
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        Ok(FrontResponse::InputEvents { events })
    }
}

/// 로컬/Controller의 submit만 이 장부를 쓴다. 이름·글·경로는 상세에 넣지 않는다.
pub(crate) fn submit_meta(data: &str, steer: bool) -> String {
    use sha2::{Digest, Sha256};
    let paste = data.starts_with("\x1b[200~") && data.ends_with("\x1b[201~");
    let text = if paste { &data[6..data.len() - 6] } else { data };
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    format!(
        "bytes={} chars={} sha256={:x} multiline={} paste={} mention={} attachment={} steer={}",
        text.len(),
        text.chars().count(),
        Sha256::digest(text.as_bytes()),
        text.contains('\n'),
        paste,
        text.split_whitespace().any(|s| s.starts_with('@')),
        text.split_whitespace().any(|s| s.starts_with('@')
            && [".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|ext| s.to_ascii_lowercase().ends_with(ext))),
        steer
    )
}

pub(crate) fn valid_trace(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
