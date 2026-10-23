//! 이미지 전용 hook. 일반 Front 쓰기 권한을 열지 않고 자기 세션 요청만 host로 전달한다.
use crate::{Agent, hooks::HookMsg};
use aaa_common::{
    caller::{Caller, judge},
    util,
};
use aaa_proto::{AgentEvent, ImageRequest, ImageResponse, LinkMsg, Outcome};
use anyhow::{Result, ensure};
use std::time::Duration;

pub async fn send(request: ImageRequest) -> Result<ImageResponse> {
    let reply = crate::hooks::send("imagegen", serde_json::to_value(request)?).await?;
    let text = reply
        .as_deref()
        .and_then(|s| s.strip_prefix("imagegen.v1 "))
        .ok_or_else(|| anyhow::anyhow!("이 agent는 이미지 생성을 모른다(imagegen.v1 필요)"))?;
    let result: Outcome<ImageResponse> = serde_json::from_str(text)?;
    result.into_result().map_err(|e| anyhow::anyhow!(e.message))
}

impl Agent {
    pub(crate) async fn on_image(&self, msg: HookMsg, pid: Option<u32>) -> Result<String> {
        let panes: Vec<_> = self
            .sessions()
            .into_iter()
            .filter(|s| s.state.is_running())
            .filter_map(|s| s.pid.map(|p| (s.id, p)))
            .collect();
        ensure!(
            pid.is_some_and(|p| judge(p, &panes, &[]) == Caller::Pane(msg.session.clone())),
            "이미지 요청 프로세스가 이 세션 안에 없다(peer pid)"
        );
        ensure!(
            self.host_image.load(std::sync::atomic::Ordering::Relaxed) && self.host_tx().is_some(),
            "연결된 host의 imagegen.v1 지원이 필요하다"
        );
        let request: ImageRequest = serde_json::from_value(msg.payload).map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, anyhow::anyhow!("이미지 요청 형식을 확인한다"))
        })?;
        let id = util::new_id();
        let (tx, rx) = tokio::sync::oneshot::channel();
        {
            let mut waiters = self.image_waiters.lock().unwrap();
            ensure!(waiters.len() < 64, "동시 이미지 요청 한도");
            waiters.insert(id.clone(), tx);
        }
        self.send_host(&LinkMsg::Event(AgentEvent::Image { request_id: id.clone(), session: msg.session, request }))
            .await;
        let result = tokio::time::timeout(Duration::from_millis(2500), rx).await;
        self.image_waiters.lock().unwrap().remove(&id);
        let result = result.map_err(|source_error| {
            aaa_common::diagnostic::context(
                source_error,
                anyhow::anyhow!("이미지 요청 응답을 받지 못했다. 같은 작업 ID로 상태를 확인한다"),
            )
        })??;
        Ok(format!("imagegen.v1 {}", serde_json::to_string(&result)?))
    }
}
