//! 화면 색(plan §8.187). 프로그램의 색 질의(OSC 10/11)에 session host가 화면 테마 색으로 답하도록 화면이 알려 준 색을 agent에
//! 보낸다. 지금 색은 가장 최근에 쓴(색을 알림·입력) 화면의 것이다(herdr 0.9.5 foreground client의 host theme,
//! `src/server/headless.rs` `ClientShellHostTheme`·`promote_client_to_foreground`).

use std::collections::HashMap;

use aaa_proto::{AgentRequest, TermColors};
use tokio::sync::{mpsc, oneshot};

use crate::{AgentConn, Host, Inner};

/// agent에 보낼 색 하나. `done`은 agent가 답했거나 보내지 못했을 때 울린다
pub(crate) struct ColorPush {
    colors: TermColors,
    done: Option<oneshot::Sender<()>>,
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
pub(crate) fn spawn_pushes(conn: AgentConn, current: Option<TermColors>) -> mpsc::UnboundedSender<ColorPush> {
    let (tx, mut rx) = mpsc::unbounded_channel::<ColorPush>();
    if let Some(colors) = current {
        aaa_common::diagnostic::ignored(
            tx.send(ColorPush { colors, done: None }),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
    }
    tokio::spawn(async move {
        while let Some(p) = rx.recv().await {
            if let Err(e) = conn.request(AgentRequest::TermColors(p.colors)).await {
                tracing::debug!("화면 색을 보내지 못했다: {}", e.message);
            }
            if let Some(done) = p.done {
                aaa_common::diagnostic::ignored(done.send(()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
            }
        }
    });
    tx
}

/// 지금 색을 모든 agent에 보낸다(inner lock 안에서: 보내는 차례가 바뀐 차례다)
pub(crate) fn push_all(i: &Inner) {
    let Some(colors) = i.colors.current else { return };
    for tx in i.machines.values().filter_map(|m| m.colors.as_ref()) {
        aaa_common::diagnostic::ignored(
            tx.send(ColorPush { colors, done: None }),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
    }
}

impl Host {
    /// 화면이 색을 알렸다(pane WebSocket을 열 때·테마를 바꿀 때). 지금 색이 바뀌면 모든 agent에 보내고, `pane`의 agent에는
    /// 바뀌지 않았어도 보낸다. 돌려준 것이 울리면 그 agent는 지금 색을 받아 session host 쪽으로 넘겼다: 화면은 그 뒤에 pane을
    /// 보고 입력하므로 그 화면이 본 뒤의 색 질의에는 이 색으로 답한다.
    pub(crate) fn front_colors(&self, front: &str, pane: &str, colors: TermColors) -> Option<oneshot::Receiver<()>> {
        let mut i = self.inner.lock().unwrap();
        if i.colors.touch(front, Some(colors)) {
            push_all(&i);
        }
        let current = i.colors.current?;
        let machine = i.panes.get(pane)?.machine_id.clone();
        let tx = i.machines.get(&machine)?.colors.as_ref()?;
        let (done, wait) = oneshot::channel();
        tx.send(ColorPush { colors: current, done: Some(done) })
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        Some(wait)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
