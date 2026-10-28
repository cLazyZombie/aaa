//! 실제 events 루프를 정지 시계·송신 gate로 실행한다. host가 늦게 깨어나도 이미 온 pong을 먼저 읽어야 한다.
use super::*;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, oneshot};

struct Socket {
    incoming: mpsc::UnboundedReceiver<Message>,
    sent: mpsc::UnboundedSender<Message>,
    resume: Option<oneshot::Receiver<()>>,
    holding_state: bool,
}

impl futures_util::Stream for Socket {
    type Item = Result<Message, axum::Error>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.incoming.poll_recv(cx).map(|m| m.map(Ok))
    }
}

impl futures_util::Sink<Message> for Socket {
    type Error = axum::Error;
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
    fn start_send(mut self: Pin<&mut Self>, message: Message) -> Result<(), Self::Error> {
        if let Message::Text(text) = &message
            && serde_json::from_str::<serde_json::Value>(text).unwrap()["ev"] == "state"
            && self.resume.is_some()
        {
            self.holding_state = true;
        }
        self.sent.send(message).unwrap();
        Ok(())
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.holding_state {
            if Pin::new(self.resume.as_mut().unwrap()).poll(cx).is_pending() {
                return Poll::Pending;
            }
            self.resume = None;
            self.holding_state = false;
        }
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

async fn stalled_loop(answer: bool, revoke: bool) {
    let dir = std::env::temp_dir().join(format!("aaa-events-test-{}", aaa_common::util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.notify.web_push = false;
    let host = Host::open(cfg, dir.clone()).unwrap();
    let dev = aaa_test_support::token("events-device");
    let (incoming, receiver) = mpsc::unbounded_channel();
    let (sender, mut sent) = mpsc::unbounded_channel();
    let (resume, gate) = oneshot::channel();
    let socket = Socket { incoming: receiver, sent: sender, resume: Some(gate), holding_state: false };
    let task = tokio::spawn(events_loop(host.clone(), dev.clone(), DeviceRole::Controller, socket));
    // 실제 첫 state 송신에서만 멈춘다. timer를 기다리며 실제 시간을 자지 않는다.
    loop {
        if let Some(Message::Text(text)) = sent.recv().await {
            if serde_json::from_str::<serde_json::Value>(&text).unwrap()["ev"] == "state" {
                break;
            }
        } else {
            panic!("첫 state 전에 events 루프가 끝났다");
        }
    }
    if answer {
        incoming.send(Message::Pong(Default::default())).unwrap();
        incoming.send(Message::Text(r#"{"t":"ping","id":1232}"#.into())).unwrap();
    }
    // 원래 연결의 기한은 그대로 넘긴다. 수신과 알림·만료 tick이 모두 준비된 상태로 깨운다.
    let (every, dead_after) = events_liveness();
    tokio::time::advance(dead_after + every).await;
    host.changed.send_modify(|n| *n += 1);
    if revoke {
        host.device_revoked.send(dev).unwrap();
    }
    resume.send(()).unwrap();
    let mut answered = false;
    let mut revoked = false;
    while let Some(message) = sent.recv().await {
        match message {
            Message::Text(text) => {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                if value["ev"] == "pong" && value["id"] == 1232 {
                    answered = true;
                    // 응답 확인 뒤 클라이언트가 정상 종료한다.
                    incoming.send(Message::Close(None)).unwrap();
                }
            }
            Message::Close(Some(frame)) => revoked = frame.code == DEVICE_REVOKED_CLOSE_CODE,
            _ => {}
        }
    }
    task.await.unwrap();
    drop(host);
    std::fs::remove_dir_all(dir).unwrap();
    assert_eq!(answered, answer && !revoke, "기한 뒤 준비된 pong·앱 ping을 읽기 전에 끊었다");
    assert_eq!(revoked, revoke, "기기 취소는 준비된 수신보다 먼저 처리해야 한다");
}

#[tokio::test(start_paused = true)]
async fn overdue_tick_reads_queued_pong_before_notifications_and_disconnect() {
    stalled_loop(true, false).await;
}

#[tokio::test(start_paused = true)]
async fn overdue_tick_still_disconnects_a_silent_peer() {
    stalled_loop(false, false).await;
}

#[tokio::test(start_paused = true)]
async fn revoked_device_closes_before_queued_input() {
    stalled_loop(true, true).await;
}
