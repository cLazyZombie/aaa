//! 원격 세션의 읽기 전용 CLI를 기존 인증된 agent 연결로 전달한다.
use crate::{Agent, hooks::HookMsg};
use aaa_common::{
    caller::{Caller, judge},
    util,
};
use aaa_proto::{AgentEvent, FrontRequest, FrontResponse, LinkMsg, Outcome};
use anyhow::{Context, Result, ensure};
use std::time::Duration;

pub async fn send(request: FrontRequest) -> Result<Result<FrontResponse, aaa_proto::ErrorBody>> {
    ensure!(request.thread_read_allowed(), "원격 전달을 허용한 읽기 명령이 아니다");
    let reply = crate::hooks::send("thread_read", serde_json::to_value(request)?).await?;
    let text = reply
        .as_deref()
        .and_then(|s| s.strip_prefix("thread_read.v1 "))
        .context("이 agent는 원격 읽기를 지원하지 않는다(thread_read.v1 필요)")?;
    let result: Outcome<FrontResponse> = serde_json::from_str(text).context("원격 읽기 응답 해독")?;
    Ok(result.into_result())
}

impl Agent {
    pub(crate) async fn on_thread_read(&self, msg: HookMsg, pid: Option<u32>) -> Result<Outcome<FrontResponse>> {
        let panes: Vec<_> = self
            .sessions()
            .into_iter()
            .filter(|s| s.state.is_running())
            .filter_map(|s| s.pid.map(|p| (s.id, p)))
            .collect();
        ensure!(
            pid.is_some_and(|p| judge(p, &panes, &[]) == Caller::Pane(msg.session.clone())),
            "읽기 요청 프로세스가 이 세션 안에 없다(peer pid)"
        );
        let request: FrontRequest = serde_json::from_value(msg.payload).context("원격 읽기 요청 해독")?;
        ensure!(request.thread_read_allowed(), "원격 전달을 허용한 읽기 명령이 아니다");
        ensure!(
            self.host_thread_read.load(std::sync::atomic::Ordering::Relaxed) && self.host_tx().is_some(),
            "연결된 host의 thread_read.v1 지원이 필요하다"
        );
        let id = util::new_id();
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut waiters = self.read_waiters.lock().unwrap();
            ensure!(waiters.len() < 64, "동시 읽기 요청 한도");
            waiters.insert(id.clone(), tx);
        }
        self.send_host(&LinkMsg::Event(AgentEvent::ThreadRead {
            request_id: id.clone(),
            session: msg.session,
            request: Box::new(request),
        }))
        .await;
        let result = tokio::time::timeout(Duration::from_secs(25), rx).await;
        self.read_waiters.lock().unwrap().remove(&id);
        result.context("원격 읽기 host 응답 시간 초과")?.context("원격 읽기 host 응답 채널이 닫혔다")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hook_reply_preserves_library_bytes_and_masks_error_text() {
        let secret = aaa_test_support::token("read-library-bytes");
        aaa_common::redact::remember(&secret);
        let raw = [b"\0\xff".as_slice(), secret.as_bytes()].concat();
        let reply = crate::hooks::reply(Ok(Some(crate::hooks::Reply::ThreadRead(Box::new(Outcome::Ok(
            FrontResponse::LibraryChunk { data: aaa_proto::B64(raw.clone()), next: None },
        ))))));
        let parsed: Outcome<FrontResponse> =
            serde_json::from_str(reply.strip_prefix("ok\nthread_read.v1 ").unwrap()).unwrap();
        assert!(matches!(parsed, Outcome::Ok(FrontResponse::LibraryChunk { data, .. }) if data.0 == raw));
        let reply = crate::hooks::reply(Ok(Some(crate::hooks::Reply::ThreadRead(Box::new(Outcome::Err(
            aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::Unavailable, secret.to_string()),
        ))))));
        assert!(!reply.contains(secret.as_str()));
    }
}
