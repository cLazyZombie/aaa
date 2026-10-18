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

/// 흐름 사건·판정 근거에 적는 기기 이름. 기기 ID는 ULID(앞 10자 시각 + 뒤 16자 무작위)라 앞 7자는 33초쯤 안에 짝지은
/// 기기끼리 같다(10-18 실기: 휴대폰과 PC가 한 이름). 무작위 끝 7자를 쓴다(harness `short_id`도 무작위 쪽, plan §8.165).
/// ULID가 아닌 ID(옛 기록·시험)는 앞 7자
pub(crate) fn device_short(id: &str) -> String {
    if id.len() == 26 && id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        id[19..].to_string()
    } else {
        id.chars().take(7).collect()
    }
}

/// 웹 화면들: 기기마다 쓰는 events 연결 수, 열린 pane 터미널 연결(front → 기기·pane), 쓰는 기기
#[derive(Default)]
pub(crate) struct Viewing {
    active: HashMap<String, usize>,
    terms: HashMap<String, (String, String)>,
    /// 사람이 마지막으로 누르거나 치거나 화면을 켠 기기(사용자 결정 10-18 「쓰는 기기」, plan §8.105).
    /// 다른 기기가 그러면 바로 바뀐다. 가려지면 그대로 두되 쓰는 화면이 아니므로 push가 간다
    user: Option<String>,
    /// 쓰는 기기를 마지막으로 알린 때와 길(events `touch`·pane 입력)
    user_at: (u64, &'static str),
    /// events 연결마다의 판정 근거(10-18 실기: push를 보내고 말고를 정한 까닭을 흐름 사건에 남긴다)
    conns: HashMap<u64, ConnRec>,
    next_conn: u64,
}

/// 한 events 연결이 host에 알린 것과 그 때(ms). 0은 아직 없음
#[derive(Clone, Default)]
pub(crate) struct ConnRec {
    dev: String,
    opened: u64,
    /// 「쓰는 화면」으로 센다(presence true)
    active: bool,
    presence_at: u64,
    /// 마지막 presence의 까닭과 화면 값(웹이 보낸 것)
    presence_why: String,
    /// 마지막 앱 글(Text frame). 멈춘 페이지는 보내지 못한다
    text_at: u64,
    /// 마지막 WebSocket pong. 브라우저 네트워크 층이 답하므로 페이지가 멈춰도 올 수 있다
    pong_at: u64,
    /// 그 페이지의 웹 빌드(presence `build`, 옛 웹은 없다)
    build: Option<String>,
}

impl Viewing {
    /// 쓰는 기기(짧은 이름, `device_short`)
    pub(crate) fn user(&self) -> Option<String> {
        self.user.as_deref().map(device_short)
    }

    /// 쓰는 기기가 지금 쓰는 화면(보이고 focus)이다: 그때만 Web Push를 건너뛴다
    pub(crate) fn user_in_use(&self) -> bool {
        self.user.as_deref().is_some_and(|d| self.device_active(d))
    }

    pub(crate) fn is_user(&self, dev: &str) -> bool {
        self.user.as_deref() == Some(dev)
    }

    /// 지금 쓰는 화면이 있는 기기(짧은 이름, 정렬). push를 보내지 않은 까닭을 흐름 사건에 남길 때 쓴다
    pub(crate) fn devices_in_use(&self) -> Vec<String> {
        let mut v: Vec<String> = self.active.iter().filter(|(_, n)| **n > 0).map(|(d, _)| device_short(d)).collect();
        v.sort();
        v
    }

    /// push를 보내거나 건너뛴 근거: 쓰는 기기와 그 기기의 events 연결마다 알린 것(몇 초 전), 그 밖 연결 수.
    /// 기기 ID는 앞 7자만 쓴다
    pub(crate) fn basis(&self, now: u64) -> String {
        let ago =
            |at: u64| if at == 0 { "없음".to_string() } else { format!("{}초 전", now.saturating_sub(at) / 1000) };
        let Some(user) = self.user.as_deref() else {
            return format!(
                "쓰는 기기 없음 · 연결 {}개(쓰는 중 {})",
                self.conns.len(),
                self.conns.values().filter(|c| c.active).count()
            );
        };
        let mut mine: Vec<(&u64, &ConnRec)> = self.conns.iter().filter(|(_, c)| c.dev == user).collect();
        mine.sort_by_key(|(id, _)| **id);
        let mut out = format!("쓰는 기기 {}({} {})", device_short(user), self.user_at.1, ago(self.user_at.0));
        if mine.is_empty() {
            out.push_str(" · 그 기기 연결 없음");
        }
        for (id, c) in &mine {
            out.push_str(&format!(
                " · 연결#{id} {}(presence {} {}, 앱 글 {}, pong {}, 연결 {}, 웹 {})",
                if c.active { "쓰는 중" } else { "안 씀" },
                ago(c.presence_at),
                if c.presence_why.is_empty() { "-" } else { &c.presence_why },
                ago(c.text_at),
                ago(c.pong_at),
                ago(c.opened),
                c.build.as_deref().unwrap_or("?"),
            ));
        }
        let others: Vec<&ConnRec> = self.conns.values().filter(|c| c.dev != user).collect();
        let mut busy: Vec<String> = others.iter().filter(|c| c.active).map(|c| device_short(&c.dev)).collect();
        busy.sort();
        busy.dedup();
        out.push_str(&format!(
            " · 그 밖 연결 {}개(쓰는 기기 {})",
            others.len(),
            if busy.is_empty() { "-".into() } else { busy.join(", ") }
        ));
        out
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

    /// 이 기기의 events 연결(`conn`)이 쓰는 화면이 됐거나(true) 아니게 됐다(false)
    pub(crate) fn view_presence(&self, conn: u64, dev: &str, active: bool) {
        let panes: Vec<String> = {
            let mut v = self.viewing.lock().unwrap();
            if let Some(c) = v.conns.get_mut(&conn) {
                c.active = active;
            }
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

    /// 사람이 이 기기에서 방금 눌렀거나 쳤거나 화면을 켰다: 이 기기가 쓰는 기기다. `how`는 근거 기록용(touch·입력)
    pub(crate) fn touch_device(&self, dev: &str, how: &'static str) {
        let mut v = self.viewing.lock().unwrap();
        if !v.is_user(dev) {
            v.user = Some(dev.to_string());
        }
        v.user_at = (aaa_common::util::now_ms(), how);
    }

    /// 웹이 사건 하나를 앱 안 알림으로 어떻게 했나(판정 근거). 아는 프로젝트면 흐름 사건 `alert_outcome`, 아니면 log만.
    /// 웹이 보낸 글이므로 길이와 글자를 줄인다
    /// `conn`의 웹 빌드를 끝에 붙인다(옛 웹은 「웹 ?」)
    pub(crate) fn alert_outcome(
        &self,
        dev: &str,
        conn: u64,
        project: Option<&str>,
        target: &str,
        kind: &str,
        outcome: &str,
    ) {
        let clip = |s: &str, n: usize| s.chars().filter(|c| !c.is_control()).take(n).collect::<String>();
        let build = self.viewing.lock().unwrap().conns.get(&conn).and_then(|c| c.build.clone());
        let detail = format!(
            "기기 {} · {} · {} · {} · 웹 {}",
            device_short(dev),
            clip(kind, 16),
            clip(target, 120),
            clip(outcome, 120),
            build.as_deref().unwrap_or("?")
        );
        match project.filter(|p| self.project_dir(p).is_ok()) {
            Some(p) => self.flow_project(p, "alert_outcome", None, detail),
            None => tracing::info!("앱 안 알림 {detail}"),
        }
    }

    /// events 연결 하나를 판정 근거에 올린다. 돌려준 번호로 아래를 알리고 끝나면 `conn_close`
    pub(crate) fn conn_open(&self, dev: &str) -> u64 {
        let mut v = self.viewing.lock().unwrap();
        v.next_conn += 1;
        let id = v.next_conn;
        v.conns.insert(id, ConnRec { dev: dev.to_string(), opened: aaa_common::util::now_ms(), ..Default::default() });
        id
    }

    pub(crate) fn conn_close(&self, id: u64) {
        self.viewing.lock().unwrap().conns.remove(&id);
    }

    /// 그 연결에서 앱 글(Text frame)을 받았다
    pub(crate) fn conn_text(&self, id: u64) {
        if let Some(c) = self.viewing.lock().unwrap().conns.get_mut(&id) {
            c.text_at = aaa_common::util::now_ms();
        }
    }

    /// 그 연결에서 WebSocket pong을 받았다
    pub(crate) fn conn_pong(&self, id: u64) {
        if let Some(c) = self.viewing.lock().unwrap().conns.get_mut(&id) {
            c.pong_at = aaa_common::util::now_ms();
        }
    }

    /// 그 연결이 presence를 알렸다(`why`는 웹이 보낸 까닭·화면 값). 쓰는 화면인지는 `view_presence`가 정한다
    pub(crate) fn conn_presence(&self, id: u64, why: String, build: Option<String>) {
        if let Some(c) = self.viewing.lock().unwrap().conns.get_mut(&id) {
            c.presence_at = aaa_common::util::now_ms();
            c.presence_why = why;
            if let Some(b) = build {
                c.build = Some(
                    b.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).take(40).collect(),
                );
            }
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

    /// 같은 33초 안에 짝지은 두 기기도 이름이 다르다: 앞 시각이 아니라 뒤 무작위 7자
    #[test]
    fn device_short_is_the_random_tail() {
        assert_eq!(device_short("01KPX3V9ZQ8M2C4D6F7GHJK0NP"), "GHJK0NP");
        assert_ne!(device_short("01KPX3V9ZQAAAAAAAAAAAAAAAA"), device_short("01KPX3V9ZQBBBBBBBBBBBBBBBB"));
        assert_eq!(device_short("phone-1234"), "phone-1");
    }

    #[test]
    fn basis_names_the_user_device_connections() {
        let mut v = Viewing::default();
        assert_eq!(v.basis(10_000), "쓰는 기기 없음 · 연결 0개(쓰는 중 0)");
        let rec = |dev: &str, active: bool| ConnRec {
            dev: dev.into(),
            opened: 1_000,
            active,
            presence_at: 2_000,
            presence_why: "true focus 보임·focus".into(),
            text_at: 9_000,
            pong_at: 0,
            build: Some("index-abc".into()),
        };
        v.conns.insert(2, rec("phone-1234", true));
        v.conns.insert(1, rec("desk-5678", true));
        v.conns.insert(3, ConnRec { active: false, ..rec("desk-5678", false) });
        v.user = Some("phone-1234".into());
        v.user_at = (4_000, "touch");
        assert_eq!(
            v.basis(10_000),
            "쓰는 기기 phone-1(touch 6초 전) · 연결#2 쓰는 중(presence 8초 전 true focus 보임·focus, 앱 글 1초 전, \
             pong 없음, 연결 9초 전, 웹 index-abc) · 그 밖 연결 2개(쓰는 기기 desk-56)"
        );
        // 쓰는 기기의 연결이 없다(닫혔다)
        v.conns.remove(&2);
        assert!(v.basis(10_000).contains(" · 그 기기 연결 없음 · "), "{}", v.basis(10_000));
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
