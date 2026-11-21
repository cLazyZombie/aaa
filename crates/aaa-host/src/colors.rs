//! 화면 색(plan §8.187). 프로그램의 색 질의(OSC 10/11)에 session host가 화면 테마 색으로 답하도록 화면이 알려 준 색을 agent에
//! 보낸다. 지금 색은 가장 최근에 쓴(색을 알림·입력) 화면의 것이다(herdr 0.9.5 foreground client의 host theme,
//! `src/server/headless.rs` `ClientShellHostTheme`·`promote_client_to_foreground`).

use std::collections::HashMap;
use std::time::Instant;

use aaa_proto::{AgentRequest, TermColors};
use tokio::sync::{mpsc, watch};

use crate::{AgentConn, Host, Inner};

struct ColorPush {
    colors: TermColors,
    queued: Instant,
    done: watch::Sender<Option<ColorReceipt>>,
}

#[derive(Clone, Copy)]
pub(crate) struct ColorReceipt {
    pub ok: bool,
    pub queue_us: u64,
    pub request_us: u64,
}

/// 같은 색 요청은 agent 응답 하나를 함께 기다린다. 실패한 요청은 다음 알림에서 다시 보낼 수 있다.
pub(crate) type ColorWait = watch::Receiver<Option<ColorReceipt>>;

pub(crate) struct ColorPushes {
    tx: mpsc::UnboundedSender<ColorPush>,
    last: Option<(TermColors, ColorWait)>,
}

impl ColorPushes {
    fn push(&mut self, colors: TermColors) -> Option<ColorWait> {
        if let Some((previous, wait)) = &self.last
            && *previous == colors
        {
            let listening = wait.has_changed().is_ok();
            let status = wait.borrow().map(|receipt| receipt.ok);
            if status == Some(true) || (status.is_none() && listening) {
                return Some(wait.clone());
            }
        }
        let (done, wait) = watch::channel(None);
        self.tx
            .send(ColorPush { colors, queued: Instant::now(), done })
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "화면 색 전송 task 종료"))
            .ok()?;
        self.last = Some((colors, wait.clone()));
        Some(wait)
    }
}

/// 화면마다 알려 준 색과 마지막으로 쓴 차례. 화면이 모두 떠나도 마지막 색을 둔다(herdr도 foreground client가 없으면 theme을
/// 그대로 둔다, `sync_foreground_client_state`)
#[derive(Default)]
pub(crate) struct ColorFronts {
    seq: u64,
    fronts: HashMap<String, (u64, TermColors)>,
    pub(crate) current: Option<TermColors>,
}

impl ColorFronts {
    /// 이 화면이 썼다(`colors`면 그 색을 알렸다). 지금 색이 바뀌었는가
    pub(crate) fn touch(&mut self, front: &str, colors: Option<TermColors>) -> bool {
        self.seq += 1;
        match colors {
            Some(c) => {
                self.fronts.insert(front.to_string(), (self.seq, c));
            }
            None => {
                let Some(e) = self.fronts.get_mut(front) else { return false };
                e.0 = self.seq;
            }
        }
        self.settle()
    }

    /// 이 화면이 떠났다. 지금 색이 바뀌었는가
    pub(crate) fn gone(&mut self, front: &str) -> bool {
        self.fronts.remove(front).is_some() && self.settle()
    }

    fn settle(&mut self) -> bool {
        let Some(latest) = self.fronts.values().max_by_key(|(at, _)| *at).map(|(_, c)| *c) else { return false };
        let changed = self.current != Some(latest);
        self.current = Some(latest);
        changed
    }
}

/// agent 연결 하나에 색을 차례로 보낸다(앞 색이 뒤 색을 덮지 않는다). 받는 쪽(`MachineRt.colors`)을 놓으면 끝난다.
pub(crate) fn spawn_pushes(conn: AgentConn, current: Option<TermColors>) -> ColorPushes {
    let (tx, mut rx) = mpsc::unbounded_channel::<ColorPush>();
    let mut pushes = ColorPushes { tx, last: None };
    if let Some(colors) = current {
        pushes.push(colors);
    }
    tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            let started = Instant::now();
            let result = conn.request(AgentRequest::TermColors(p.colors)).await;
            if let Err(e) = &result {
                tracing::debug!("화면 색을 보내지 못했다: {}", e.message);
            }
            p.done.send_replace(Some(ColorReceipt {
                queue_us: started.duration_since(p.queued).as_micros() as u64,
                request_us: started.elapsed().as_micros() as u64,
                ok: result.is_ok(),
            }));
        }
    });
    pushes
}

/// 지금 색을 모든 agent에 보낸다(inner lock 안에서: 보내는 차례가 바뀐 차례다)
pub(crate) fn push_all(i: &mut Inner, except: Option<&str>) {
    let Some(colors) = i.colors.current else { return };
    for (id, machine) in &mut i.machines {
        if Some(id.as_str()) != except
            && let Some(pushes) = machine.colors.as_mut()
        {
            pushes.push(colors);
        }
    }
}

impl Host {
    /// 화면이 색을 알렸다. 현재 색이 바뀌면 다른 머신에도 보낸다. 선택한 머신의 같은 색이 대기 중이면 완료를 함께 기다리고,
    /// 이미 처리했다면 그 결과를 쓴다. 머신이 다시 연결되면 새 전송 상태에서 시작한다.
    pub(crate) fn front_colors(&self, front: &str, pane: &str, colors: TermColors) -> Option<ColorWait> {
        let mut i = self.inner.lock().unwrap();
        let machine = i.panes.get(pane).map(|p| p.machine_id.clone());
        if i.colors.touch(front, Some(colors)) {
            push_all(&mut i, machine.as_deref());
        }
        i.machines.get_mut(&machine?)?.colors.as_mut()?.push(colors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_a_pending_or_applied_color_and_retries_a_failure() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut pushes = ColorPushes { tx, last: None };
        let dark = TermColors { fg: [1, 2, 3], bg: [4, 5, 6] };
        let light = TermColors { fg: [4, 5, 6], bg: [1, 2, 3] };

        let first = pushes.push(dark).unwrap();
        let same = pushes.push(dark).unwrap();
        let push = rx.try_recv().unwrap();
        assert!(first.same_channel(&same));
        assert!(rx.try_recv().is_err());
        push.done.send_replace(Some(ColorReceipt { ok: true, queue_us: 0, request_us: 0 }));
        drop(push);
        let applied = pushes.push(dark).unwrap();
        assert_eq!(applied.borrow().map(|r| r.ok), Some(true));
        assert!(applied.same_channel(&first));
        assert!(rx.try_recv().is_err());

        let other = pushes.push(light).unwrap();
        let push = rx.try_recv().unwrap();
        assert_eq!(push.colors, light);
        push.done.send_replace(Some(ColorReceipt { ok: false, queue_us: 0, request_us: 0 }));
        drop(push);
        let retry = pushes.push(light).unwrap();
        assert!(!retry.same_channel(&other));
        assert_eq!(rx.try_recv().unwrap().colors, light);
        assert!(!pushes.push(dark).unwrap().same_channel(&first));
        assert_eq!(rx.try_recv().unwrap().colors, dark);
    }

    /// 지금 색은 마지막으로 쓴(알림·입력) 화면의 것이고, 그 화면이 떠나면 남은 화면 중 마지막 것, 모두 떠나면 그대로다
    #[test]
    fn current_colors_follow_the_last_front_used() {
        let dark = TermColors { fg: [0xd8, 0xd0, 0xc3], bg: [0x18, 0x16, 0x13] };
        let light = TermColors { fg: [0x2a, 0x25, 0x1f], bg: [0xfa, 0xf8, 0xf3] };
        let mut c = ColorFronts::default();
        assert!(!c.touch("f0", None), "색을 알리지 않은 화면(TUI)은 바꾸지 않는다");
        assert!(c.touch("f1", Some(dark)));
        assert!(!c.touch("f1", Some(dark)), "같은 색");
        assert!(c.touch("f2", Some(light)));
        assert!(c.touch("f1", None), "f1이 입력했다");
        assert_eq!(c.current, Some(dark));
        assert!(c.gone("f1"));
        assert_eq!(c.current, Some(light));
        assert!(!c.gone("f0"));
        assert!(!c.gone("f2"));
        assert_eq!(c.current, Some(light));
    }
}
