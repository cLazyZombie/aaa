//! 대화 기록 변화 push(차이 목록 C3, plan §8.191). herdr-web-ui(aa624cb) `server/index.ts`의 연결별 `conversationInterests`·
//! `releaseConversation`과 `server/conversation-monitor.ts`를 나눠 옮겼다: 기록 파일은 그 머신에 있어 감시는 agent가 하고
//! (`AgentRequest::ChatWatch`, 250ms stat), host는 웹 상태 사건 연결들의 관심을 세어 관심 있는 pane만 agent에 켜 두고, 바뀜은
//! 그 pane을 청한 연결에만 보낸다(`FrontEvent::ChatChanged`).
//!
//! 참고와 다른 곳: 참고는 그 pane에 붙은(attach) 연결만 청할 수 있고 서버 기능 `conversation-watch`로 push가 되는지 알린다.
//! aaa는 pane 보기 권한이 있는 상태 사건 연결이 청하고, 되는지는 pane마다 `ChatWatching`으로 알린다(그 머신 agent가
//! `chat_watch.v1`이고 붙어 있고, pane이 Claude·Codex일 때).

use std::collections::HashMap;
use std::sync::Arc;

use aaa_proto::AgentRequest;

/// 상태 사건 연결들에 나누는 알림
#[derive(Debug, Clone)]
pub(crate) enum ChatNote {
    Changed { pane: String, signature: String },
    Live { pane: String, live: bool },
}

/// agent에 켜 둔 감시: 이것이 바뀌면 다시 켠다(Claude 기록 위치는 첫 hook 사건에, Codex thread-id는 첫 턴 뒤에 온다)
#[derive(Debug, Clone, PartialEq)]
struct Watch {
    machine: String,
    generation: u64,
    harness: String,
    transcript: Option<String>,
    harness_session: Option<String>,
}

/// agent에 보낼 요청: 머신·그 연결 세대·요청
type Order = (String, u64, AgentRequest);

#[derive(Default)]
pub(crate) struct ChatWatch {
    /// pane마다 청한 연결 수
    interest: HashMap<String, u32>,
    /// agent에 켜 둔 감시(pane별)
    on: HashMap<String, Watch>,
}

impl ChatWatch {
    /// 청한 연결 수를 하나 더하거나 뺀다
    fn want(&mut self, pane: &str, on: bool) {
        if on {
            *self.interest.entry(pane.to_string()).or_default() += 1;
        } else if let Some(n) = self.interest.get_mut(pane) {
            *n -= 1;
            if *n == 0 {
                self.interest.remove(pane);
            }
        }
    }

    /// 바라는 감시로 맞춘다: 켜 둔 것을 바꾸고 agent에 보낼 요청(머신·세대·요청)과 바뀐 push 가능 여부를 돌려준다
    fn settle(&mut self, wanted: HashMap<String, Watch>) -> (Vec<Order>, Vec<(String, bool)>) {
        let mut sends = Vec::new();
        let mut live = Vec::new();
        let gone: Vec<String> = self.on.keys().filter(|p| !wanted.contains_key(*p)).cloned().collect();
        for pane in gone {
            let w = self.on.remove(&pane).expect("켜 둔 감시");
            let off = AgentRequest::ChatWatch {
                key: pane.clone(),
                harness: w.harness,
                transcript: None,
                harness_session: None,
                enabled: false,
            };
            sends.push((w.machine, w.generation, off));
            live.push((pane, false));
        }
        for (pane, w) in wanted {
            if self.on.get(&pane) == Some(&w) {
                continue;
            }
            let on = AgentRequest::ChatWatch {
                key: pane.clone(),
                harness: w.harness.clone(),
                transcript: w.transcript.clone(),
                harness_session: w.harness_session.clone(),
                enabled: true,
            };
            sends.push((w.machine.clone(), w.generation, on));
            if self.on.insert(pane.clone(), w).is_none() {
                live.push((pane, true));
            }
        }
        (sends, live)
    }
}

impl crate::Host {
    /// 이 연결이 그 pane의 대화를 보기 시작하거나 그만둔다
    pub(crate) fn chat_watch(self: &Arc<Self>, pane: &str, on: bool) {
        self.chats.lock().unwrap().want(pane, on);
        self.sync_chat_watches();
    }

    /// 그 pane의 기록 변화를 지금 알릴 수 있는가
    pub(crate) fn chat_live(&self, pane: &str) -> bool {
        self.chats.lock().unwrap().on.contains_key(pane)
    }

    pub(crate) fn chat_notes(&self) -> tokio::sync::broadcast::Receiver<ChatNote> {
        self.chat_notes.subscribe()
    }

    /// agent가 감시하는 기록이 바뀌었다(켜 둔 그 머신의 것만 받는다)
    pub(crate) fn on_chat_changed(&self, mid: &str, key: String, signature: String) {
        if self.chats.lock().unwrap().on.get(&key).is_some_and(|w| w.machine == mid) {
            let _ = self.chat_notes.send(ChatNote::Changed { pane: key, signature });
        }
    }

    /// 그 머신 agent 연결이 끊겼다: 그 감시는 agent가 이미 끝냈다(새 연결이 오면 다시 켠다)
    pub(crate) fn chat_machine_gone(&self, mid: &str) {
        let mut c = self.chats.lock().unwrap();
        let gone: Vec<String> = c.on.iter().filter(|(_, w)| w.machine == mid).map(|(p, _)| p.clone()).collect();
        for pane in gone {
            c.on.remove(&pane);
            let _ = self.chat_notes.send(ChatNote::Live { pane, live: false });
        }
    }

    /// 청한 pane들의 감시를 지금 상태(pane의 harness·기록 위치, 머신 연결·기능)에 맞춘다. 맞추기는 하나씩 차례로 한다:
    /// 켜기·끄기 요청이 뒤바뀌어 agent에 닿지 않게
    pub(crate) fn sync_chat_watches(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            let _turn = me.chat_sync.lock().await;
            let (sends, live, conns) = {
                let i = me.inner.lock().unwrap();
                let mut c = me.chats.lock().unwrap();
                let mut wanted = HashMap::new();
                for pane in c.interest.keys() {
                    let Some(p) = i.panes.get(pane) else { continue };
                    let Some(a) = p.agent.as_ref().filter(|a| matches!(a.harness.as_str(), "claude" | "codex")) else {
                        continue;
                    };
                    let Some(m) = i.machines.get(&p.machine_id) else { continue };
                    let Some(conn) = m.conn.as_ref().filter(|_| m.features.iter().any(|f| f == "chat_watch.v1")) else {
                        continue;
                    };
                    wanted.insert(
                        pane.clone(),
                        Watch {
                            machine: p.machine_id.clone(),
                            generation: conn.generation,
                            harness: a.harness.clone(),
                            transcript: p.transcript.clone(),
                            harness_session: a.harness_session.clone(),
                        },
                    );
                }
                let (sends, live) = c.settle(wanted);
                let conns: HashMap<String, crate::AgentConn> =
                    i.machines.iter().filter_map(|(id, m)| Some((id.clone(), m.conn.clone()?))).collect();
                (sends, live, conns)
            };
            for (pane, live) in live {
                let _ = me.chat_notes.send(ChatNote::Live { pane, live });
            }
            for (machine, generation, req) in sends {
                // 끊긴 연결의 감시는 agent가 이미 끝냈다
                let Some(conn) = conns.get(&machine).filter(|c| c.generation == generation) else { continue };
                if let Err(e) = conn.request(req).await {
                    tracing::debug!(%machine, "대화 기록 감시 요청 실패: {}", e.message);
                }
            }
        });
    }

    /// 청한 pane이 있을 때만 맞춘다(harness 사건마다 부른다)
    pub(crate) fn sync_chat_watches_if_wanted(self: &Arc<Self>) {
        if !self.chats.lock().unwrap().interest.is_empty() {
            self.sync_chat_watches();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn watch(transcript: Option<&str>) -> Watch {
        Watch {
            machine: "m".into(),
            generation: 1,
            harness: "claude".into(),
            transcript: transcript.map(str::to_string),
            harness_session: None,
        }
    }

    fn enabled(r: &AgentRequest) -> Option<(bool, Option<String>)> {
        match r {
            AgentRequest::ChatWatch { enabled, transcript, .. } => Some((*enabled, transcript.clone())),
            _ => None,
        }
    }

    #[test]
    fn interest_counts_connections_and_the_watch_follows_the_transcript() {
        let mut c = ChatWatch::default();
        c.want("p", true);
        c.want("p", true);
        c.want("p", false);
        assert_eq!(c.interest.get("p"), Some(&1), "한 연결이 그만둬도 다른 연결이 본다");

        let (sends, live) = c.settle(HashMap::from([("p".into(), watch(None))]));
        assert_eq!(sends.iter().map(|s| enabled(&s.2)).collect::<Vec<_>>(), [Some((true, None))]);
        assert_eq!(live, [("p".to_string(), true)]);
        let (sends, live) = c.settle(HashMap::from([("p".into(), watch(None))]));
        assert!(sends.is_empty() && live.is_empty(), "같으면 다시 보내지 않는다");

        // Claude가 첫 hook으로 기록 위치를 알렸다: 같은 key로 다시 켠다(push 가능 여부는 그대로)
        let (sends, live) = c.settle(HashMap::from([("p".into(), watch(Some("/t.jsonl")))]));
        assert_eq!(sends.iter().map(|s| enabled(&s.2)).collect::<Vec<_>>(), [Some((true, Some("/t.jsonl".into())))]);
        assert!(live.is_empty());

        c.want("p", false);
        assert!(c.interest.is_empty());
        let (sends, live) = c.settle(HashMap::new());
        assert_eq!(sends.iter().map(|s| enabled(&s.2)).collect::<Vec<_>>(), [Some((false, None))]);
        assert_eq!(live, [("p".to_string(), false)]);
    }
}
