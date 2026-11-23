//! 운영 로그에는 요청 본문과 오류 문구 대신 고정 작업명과 결과 코드만 쓴다.

use super::*;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub(super) const ANSWER_TRACE: &str = "x-aaa-answer-trace";
const RECEIPT_TTL: Duration = Duration::from_secs(120);
const RECEIPT_CAP: usize = 256;

pub(super) fn ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

pub(super) struct Operation {
    name: &'static str,
    target: Option<String>,
    trace: Option<String>,
}

impl Operation {
    pub(super) fn of(req: &FrontRequest, role: DeviceRole) -> Option<Self> {
        use FrontRequest::*;
        let (name, target, answer) = match req {
            ThreadAnswer { thread, .. } => ("thread_answer", Some(thread), true),
            AskAnswer { item, .. } => ("ask_answer", Some(item), true),
            AskCorrect { id, .. } => ("ask_correct", Some(id), true),
            QuestionReply { id, .. } => ("question_reply", Some(id), true),
            ThreadMessage { thread, .. } => ("thread_message", Some(thread), false),
            ThreadStart { project, .. } => ("thread_start", Some(project), false),
            ThreadShow { thread, .. } => ("thread_show", Some(thread), false),
            Threads { .. } => ("threads", None, false),
            Asks { .. } => ("asks", None, false),
            AskHistory { id, .. } => ("ask_history", id.as_ref(), false),
            _ => return None,
        };
        Some(Self {
            name,
            target: target.map(|id| crate::operation_log_id(id)),
            trace: (answer && role.can_control()).then(aaa_common::util::new_id),
        })
    }

    pub(super) fn span(&self) -> tracing::Span {
        self.trace.as_ref().map_or_else(tracing::Span::none, |id| tracing::info_span!("card_answer", trace = %id))
    }

    pub(super) fn identify_result(&mut self, result: &Result<aaa_proto::FrontResponse, ErrorBody>) {
        if self.name == "thread_start"
            && let Ok(aaa_proto::FrontResponse::Thread(thread)) = result
        {
            self.target = Some(crate::operation_log_id(&thread.id));
        }
    }

    pub(super) fn complete(
        mut self,
        host: &Host,
        device: String,
        [started, dispatched, handled]: [Instant; 3],
        reason: Option<ErrorCode>,
        response: &mut Response,
    ) {
        let now = Instant::now();
        let total_ms = ms(now - started);
        let dispatch_ms = ms(handled - dispatched);
        let prepare_ms = ms(dispatched - started);
        let response_ms = ms(now - handled);
        let trace = self.trace.take();
        let result = if reason.is_some() { "failed" } else { "ok" };
        if total_ms >= 500 || reason.is_some() {
            tracing::info!(
                op = self.name,
                target = self.target.as_deref(),
                trace = trace.as_deref(),
                total_ms,
                prepare_ms,
                dispatch_ms,
                response_ms,
                result,
                ?reason,
                "web_rpc"
            );
        }
        if let Some(trace) = trace {
            response.headers_mut().insert(ANSWER_TRACE, HeaderValue::from_str(&trace).unwrap());
            host.answer_receipts.lock().unwrap().insert(AnswerReceipt {
                trace,
                device,
                at: now,
                op: self.name,
                target: self.target.take(),
                host_ms: total_ms,
                reason,
            });
        }
    }
}

struct AnswerReceipt {
    trace: String,
    device: String,
    at: Instant,
    op: &'static str,
    target: Option<String>,
    host_ms: u64,
    reason: Option<ErrorCode>,
}

#[derive(Default)]
pub(crate) struct AnswerReceipts(VecDeque<AnswerReceipt>);

impl AnswerReceipts {
    fn prune(&mut self, now: Instant) {
        self.0.retain(|r| now.saturating_duration_since(r.at) < RECEIPT_TTL);
    }

    fn insert(&mut self, receipt: AnswerReceipt) {
        self.prune(Instant::now());
        if self.0.len() == RECEIPT_CAP {
            self.0.pop_front();
        }
        self.0.push_back(receipt);
    }

    fn take(&mut self, device: &str, trace: &str, now: Instant) -> Option<AnswerReceipt> {
        self.prune(now);
        let index = self.0.iter().position(|r| r.device == device && r.trace == trace)?;
        self.0.remove(index)
    }

    #[cfg(test)]
    pub(super) fn contains(&self, device: &str, trace: &str) -> bool {
        self.0.iter().any(|r| r.device == device && r.trace == trace)
    }
}

impl AnswerReceipt {
    fn report(self, timing: aaa_proto::WebAnswerTiming) {
        let Self { trace, op, target, host_ms, reason, .. } = self;
        let aaa_proto::WebAnswerTiming { response_ms, render_ms, .. } = timing;
        if reason.is_some() || response_ms >= 500 || render_ms >= 250 || host_ms >= 500 {
            let result = if reason.is_some() { "failed" } else { "ok" };
            tracing::info!(%trace, op, target = target.as_deref(), host_ms, response_ms, render_ms, result, ?reason, "web_answer");
        }
    }
}

pub(super) async fn answer_timing(
    State(host): State<Arc<Host>>,
    headers: HeaderMap,
    Json(timing): Json<aaa_proto::WebAnswerTiming>,
) -> StatusCode {
    let Some((device, role)) = device(&host, &headers) else { return StatusCode::UNAUTHORIZED };
    if !role.can_control() {
        return StatusCode::FORBIDDEN;
    }
    if timing.trace.len() != 26 || timing.response_ms > 120_000 || timing.render_ms > 120_000 {
        return StatusCode::BAD_REQUEST;
    }
    let receipt = host.answer_receipts.lock().unwrap().take(&device, &timing.trace, Instant::now());
    if let Some(receipt) = receipt {
        receipt.report(timing);
    }
    StatusCode::NO_CONTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_receipts_evict_old_entries_and_expire_at_two_minutes() {
        let now = Instant::now();
        let mut receipts = AnswerReceipts::default();
        for n in 0..=256 {
            receipts.insert(AnswerReceipt {
                trace: n.to_string(),
                device: "device-a".into(),
                at: now,
                op: "thread_answer",
                target: None,
                host_ms: 1,
                reason: None,
            });
        }
        assert_eq!(receipts.0.len(), 256);
        assert!(receipts.take("device-a", "0", now).is_none());
        assert!(receipts.take("device-a", "1", now + Duration::from_millis(119_999)).is_some());
        assert!(receipts.take("device-a", "2", now + Duration::from_millis(120_000)).is_none());
        assert!(receipts.0.is_empty());
    }
}
