//! 「끝남(안 봄)」: agent가 일을 끝냈는데 사람이 아직 그 pane을 보지 않았다(herdr 0.9.5 `PaneState::seen`).
//!
//! herdr 규칙(`src/app/actions.rs` `apply_pane_state_change`·`mark_active_tab_seen`):
//! - 쉼이 아닌 상태가 되면 봤다.
//! - 작업·질문에서 쉼으로 끝날 때, 그 pane이 지금 보이는 탭이고 바깥 터미널에 focus가 있으면 봤다. 아니면 안 봤다.
//! - 그 pane을 앞으로 가져오거나(focus) 바깥 터미널이 focus를 얻으면 봤다.
//!
//! aaa의 「보는 화면」: 웹은 그 pane 터미널을 연 연결이 있고 그 기기의 events 연결이 「쓰는 화면」(보이고 focus, `presence`)일 때,
//! TUI는 그 pane의 lease(지금 쓰는 대화형 front)를 쥐고 있을 때다.
//! herdr-web-ui는 웹에서 pane을 열어도 herdr의 「봤음」을 바꾸지 않는다(웹 터미널은 herdr TUI focus가 아니다). 웹으로만 보는
//! 사람에게는 「끝남」이 영영 남으므로 aaa는 웹 화면도 보는 화면으로 센다(참고와 다르게 둔 것, plan §8.72).
use std::collections::HashMap;

use aaa_proto::AgentActivity;

use crate::{FrontRef, Host, PaneEvent};

/// 웹 화면들: 기기마다 쓰는 events 연결 수, 열린 pane 터미널 연결(front → 기기·pane), 쓰는 기기
#[derive(Default)]
pub(crate) struct Viewing {
    active: HashMap<String, usize>,
    terms: HashMap<String, (String, String)>,
    /// 사람이 마지막으로 누르거나 치거나 화면을 켠 기기(사용자 결정 10-18 「쓰는 기기」, plan §8.105).
    /// 다른 기기가 그러면 바로 바뀐다. 가려지면 그대로 두되 쓰는 화면이 아니므로 push가 간다
    user: Option<String>,
}

impl Viewing {
    /// 쓰는 기기(앞 7자)
    pub(crate) fn user(&self) -> Option<String> {
        self.user.as_ref().map(|d| d.chars().take(7).collect())
    }

    /// 쓰는 기기가 지금 쓰는 화면(보이고 focus)이다: 그때만 Web Push를 건너뛴다
    pub(crate) fn user_in_use(&self) -> bool {
        self.user.as_deref().is_some_and(|d| self.device_active(d))
    }

    pub(crate) fn is_user(&self, dev: &str) -> bool {
        self.user.as_deref() == Some(dev)
    }

    /// 지금 쓰는 화면이 있는 기기(앞 7자, 정렬). push를 보내지 않은 까닭을 흐름 사건에 남길 때 쓴다
    pub(crate) fn devices_in_use(&self) -> Vec<String> {
        let mut v: Vec<String> =
            self.active.iter().filter(|(_, n)| **n > 0).map(|(d, _)| d.chars().take(7).collect()).collect();
        v.sort();
        v
    }

    fn device_active(&self, dev: &str) -> bool {
        self.active.get(dev).is_some_and(|n| *n > 0)
    }

    /// 그 pane을 지금 보는 웹 화면이 있다
    pub(crate) fn web_sees(&self, pane: &str) -> bool {
        self.terms.values().any(|(dev, p)| p == pane && self.device_active(dev))
    }

    fn is_web(&self, front: &str) -> bool {
        self.terms.contains_key(front)
    }
}

/// 상태가 바뀐 뒤 「봤음」: herdr `apply_pane_state_change`
pub(crate) fn seen_after(prev: Option<AgentActivity>, next: AgentActivity, was_seen: bool, someone_sees: bool) -> bool {
    use AgentActivity::*;
    match next {
        Idle if matches!(prev, Some(Working) | Some(Question)) => someone_sees,
        Idle => was_seen,
        _ => true,
    }
}

impl Host {
    /// 그 pane을 지금 보는 화면(웹·TUI)이 있다. `lease`는 그 pane의 lease다(호출하는 쪽이 잠금 안에서 준다).
    pub(crate) fn someone_sees(&self, pane: &str, lease: Option<&FrontRef>) -> bool {
        let v = self.viewing.lock().unwrap();
        v.web_sees(pane) || lease.is_some_and(|f| f.interactive() && !v.is_web(&f.id))
    }

    /// 그 pane들을 봤다. 바뀐 것이 있으면 상태 사건을 낸다
    pub(crate) fn mark_seen(&self, panes: &[String]) {
        let mut changed = false;
        {
            let mut i = self.inner.lock().unwrap();
            for id in panes {
                let Some(p) = i.panes.get_mut(id) else { continue };
                if let Some(a) = p.agent.as_mut().filter(|a| !a.seen) {
                    a.seen = true;
                    let _ = p.events.send(PaneEvent::State);
                    changed = true;
                }
            }
        }
        if changed {
            self.bump();
            for id in panes {
                self.thread_changed(id);
            }
        }
    }

    /// 이 기기의 events 연결이 쓰는 화면이 됐거나(true) 아니게 됐다(false)
    pub(crate) fn view_presence(&self, dev: &str, active: bool) {
        let panes: Vec<String> = {
            let mut v = self.viewing.lock().unwrap();
            let n = v.active.entry(dev.to_string()).or_default();
            if active {
                *n += 1;
            } else {
                *n = n.saturating_sub(1);
            }
            if *n == 0 {
                v.active.remove(dev);
            }
            if !active || !v.device_active(dev) {
                return;
            }
            v.terms.values().filter(|(d, _)| d == dev).map(|(_, p)| p.clone()).collect()
        };
        self.mark_seen(&panes);
    }

    /// 사람이 이 기기에서 방금 눌렀거나 쳤거나 화면을 켰다: 이 기기가 쓰는 기기다
    pub(crate) fn touch_device(&self, dev: &str) {
        let mut v = self.viewing.lock().unwrap();
        if !v.is_user(dev) {
            v.user = Some(dev.to_string());
        }
    }

    /// 웹 터미널 연결이 그 pane을 연다(`Some`)·닫는다(`None`)
    pub(crate) fn view_term(&self, front: &str, dev: &str, pane: Option<&str>) {
        let see = {
            let mut v = self.viewing.lock().unwrap();
            match pane {
                Some(p) => {
                    v.terms.insert(front.to_string(), (dev.to_string(), p.to_string()));
                    v.device_active(dev).then(|| p.to_string())
                }
                None => {
                    v.terms.remove(front);
                    None
                }
            }
        };
        if let Some(p) = see {
            self.mark_seen(&[p]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentActivity::*;

    #[test]
    fn a_turn_that_ends_unwatched_is_unseen_until_looked_at() {
        // herdr: 작업·질문에서 쉼으로 끝날 때 보는 화면이 없으면 「끝남(안 봄)」
        assert!(!seen_after(Some(Working), Idle, true, false));
        assert!(!seen_after(Some(Question), Idle, true, false));
        // 보는 화면이 있으면 봤다
        assert!(seen_after(Some(Working), Idle, true, true));
        // 쉼이 아닌 상태는 늘 봤다
        for next in [Working, Question, Started, Error, Exited] {
            assert!(seen_after(Some(Idle), next, false, false), "{next:?}");
        }
        // 끝남이 아닌 쉼(시작 뒤 쉼, 쉼 → 쉼)은 앞 값을 둔다
        assert!(!seen_after(Some(Idle), Idle, false, true));
        assert!(seen_after(Some(Started), Idle, true, false));
        assert!(seen_after(None, Idle, true, false));
    }

    #[test]
    fn push_waits_only_for_the_device_last_touched() {
        let mut v = Viewing::default();
        v.active.insert("desk".into(), 1);
        // 쓰는 화면이 있어도 아무도 만지지 않았으면 쓰는 기기가 없다
        assert!(!v.user_in_use());
        v.user = Some("desk".into());
        assert!(v.user_in_use());
        // 휴대폰에서 쳤다: 데스크톱이 보이고 focus여도 쓰는 기기는 휴대폰, 휴대폰이 가려지면 push가 간다
        v.user = Some("phone".into());
        assert!(!v.user_in_use());
        v.active.insert("phone".into(), 1);
        assert!(v.user_in_use());
        v.active.remove("phone");
        assert!(!v.user_in_use());
        assert_eq!(v.user().as_deref(), Some("phone"));
    }

    #[test]
    fn a_web_screen_sees_a_pane_only_while_its_device_is_in_use() {
        let mut v = Viewing::default();
        v.terms.insert("f1".into(), ("phone".into(), "p1".into()));
        assert!(!v.web_sees("p1"), "쓰는 화면이 아닌 기기(가려짐·focus 없음)는 보지 않는다");
        v.active.insert("phone".into(), 1);
        assert!(v.web_sees("p1"));
        assert!(!v.web_sees("p2"), "다른 pane은 보지 않는다");
        v.active.insert("desk".into(), 1);
        assert!(!v.web_sees("p2"), "쓰는 기기라도 그 pane 터미널을 열지 않았으면 보지 않는다");
    }
}
