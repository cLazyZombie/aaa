//! agent 알림 카드(herdr 0.9.5 `src/client/shell/notification_policy.rs`, plan §8.185). 한 번에 하나만 보이고 나머지는
//! 차례를 기다린다. 받은 알림은 잠깐(`DELAY`) 뒤 다시 본다: 그 pane이 아직 그 상태이고 지금 탭이 아니면 띄운다.
//!
//! - 받기(`receive_notification`): 같은 pane의 기다리는·줄 선·보이는 카드를 지운다. 새 보고(herdr-projects 알림, herdr
//!   `Custom`)는 지우지도 지워지지도 않고 기다리지 않는다.
//! - 다시 보기(`tick_notifications`·`notification_validation`): 입력 필요는 아직 질문 중일 때, 끝남은 아직 끝남(안 봄)일 때만.
//!   끝남인데 아직 작업 중으로 보이면(상태 사건이 늦음) `RECHECK`마다 `GRACE`까지 다시 본다. 모르는 pane도 기다린다.
//! - 지금 탭의 pane이면 띄우지 않는다(`notification_target_is_active`: pane이 아니라 탭으로 거른다).
//! - 줄은 많아야 `MAX_QUEUED`, 넘치면 가장 오래된 것을 버린다. 보이는 시간은 입력 필요 8초·끝남 5초·새 보고 5초.
//! - `prefix o`·카드 누름(`focus_visible_notification`): 보이는 카드를 지우고 다음을 올린 뒤 그 pane으로 간다.
use std::collections::VecDeque;
use std::time::Duration;

use tokio::time::Instant;

/// herdr `[ui.toast] delay_seconds` 기본값
pub(crate) const DELAY: Duration = Duration::from_secs(1);
/// herdr `COMPLETION_EVIDENCE_GRACE`·`COMPLETION_RECHECK_INTERVAL`
const GRACE: Duration = Duration::from_secs(1);
const RECHECK: Duration = Duration::from_millis(50);
/// herdr `MAX_QUEUED_NOTIFICATIONS`
const MAX_QUEUED: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// herdr `NeedsAttention`(점 빨강)
    NeedsInput,
    /// herdr `Finished`(점 파랑)
    Finished,
    /// thread 새 보고(herdr-projects 알림 = herdr `Custom`, 점 accent)
    Report,
}

impl Kind {
    fn shown_for(self) -> Duration {
        Duration::from_secs(match self {
            Kind::NeedsInput => 8,
            Kind::Finished | Kind::Report => 5,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Card {
    pub kind: Kind,
    /// 갈 pane
    pub pane: Option<String>,
    pub title: String,
    pub body: String,
}

impl Card {
    /// 같은 pane 바꿔 넣기에 드는 pane(새 보고는 herdr `Custom`처럼 pane 없는 것으로 친다)
    fn dedupe_pane(&self) -> Option<&str> {
        if self.kind == Kind::Report { None } else { self.pane.as_deref() }
    }
}

/// 다시 본 결과(herdr `NotificationValidation`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Check {
    Current,
    Awaiting,
    Stale,
}

struct Pending {
    card: Card,
    deadline: Instant,
    expires_at: Instant,
    validate: bool,
}

#[derive(Default)]
pub(crate) struct Cards {
    pending: Vec<Pending>,
    queue: VecDeque<Card>,
    visible: Option<(Card, Instant)>,
}

impl Cards {
    pub(crate) fn visible(&self) -> Option<&Card> {
        self.visible.as_ref().map(|(c, _)| c)
    }

    /// 다음에 할 일이 있는 때(보이는 카드가 사라지거나 기다리는 알림을 다시 볼 때)
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        self.visible.as_ref().map(|(_, d)| *d).into_iter().chain(self.pending.iter().map(|p| p.deadline)).min()
    }

    fn show(&mut self, card: Card, now: Instant) {
        let until = now + card.kind.shown_for();
        self.visible = Some((card, until));
    }

    fn promote(&mut self, now: Instant) {
        if let Some(c) = self.queue.pop_front() {
            self.show(c, now);
        }
    }

    fn enqueue(&mut self, card: Card, now: Instant) {
        if self.visible.is_none() {
            return self.show(card, now);
        }
        if self.queue.len() == MAX_QUEUED {
            self.queue.pop_front();
        }
        self.queue.push_back(card);
    }

    /// 알림을 받는다. 다시 그려야 하면 true(보이던 카드를 지웠다)
    pub(crate) fn receive(&mut self, card: Card, now: Instant) -> bool {
        let delay = if card.kind == Kind::Report { Duration::ZERO } else { DELAY };
        let mut cleared = false;
        if let Some(pane) = card.dedupe_pane().map(str::to_string) {
            self.pending.retain(|p| p.card.dedupe_pane() != Some(&pane));
            self.queue.retain(|c| c.dedupe_pane() != Some(&pane));
            if self.visible.as_ref().is_some_and(|(c, _)| c.dedupe_pane() == Some(&pane)) {
                self.visible = None;
                self.promote(now);
                cleared = true;
            }
        }
        let validate = !delay.is_zero() || card.kind == Kind::Finished;
        self.pending.push(Pending { card, deadline: now + delay, expires_at: now + GRACE, validate });
        cleared
    }

    /// 때가 된 일을 한다. `check`는 그 pane이 아직 그 상태인지, `active`는 지금 탭의 pane인지 본다. 다시 그려야 하면 true
    pub(crate) fn tick(
        &mut self,
        now: Instant,
        check: impl Fn(&Card) -> Check,
        active: impl Fn(&Card) -> bool,
    ) -> bool {
        let mut repaint = false;
        if self.visible.as_ref().is_some_and(|(_, until)| now >= *until) {
            self.visible = None;
            self.promote(now);
            repaint = true;
        }
        for mut p in std::mem::take(&mut self.pending) {
            if p.deadline > now {
                self.pending.push(p);
                continue;
            }
            if p.validate {
                match check(&p.card) {
                    Check::Current => {}
                    Check::Awaiting if now < p.expires_at => {
                        p.deadline = (now + RECHECK).min(p.expires_at);
                        self.pending.push(p);
                        continue;
                    }
                    Check::Awaiting | Check::Stale => continue,
                }
            }
            if p.card.kind == Kind::Report || !active(&p.card) {
                self.enqueue(p.card, now);
                repaint = true;
            }
        }
        repaint
    }

    /// 보이는 카드를 거두고 다음을 올린다(`prefix o`·누름). 갈 pane은 그 카드의 것
    pub(crate) fn take(&mut self, now: Instant) -> Option<Card> {
        let (card, _) = self.visible.take()?;
        self.promote(now);
        Some(card)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(kind: Kind, pane: &str) -> Card {
        Card { kind, pane: Some(pane.into()), title: format!("{kind:?} {pane}"), body: String::new() }
    }

    fn titles(c: &Cards) -> (Option<String>, Vec<String>) {
        (c.visible().map(|v| v.title.clone()), c.queue.iter().map(|v| v.title.clone()).collect())
    }

    const OK: fn(&Card) -> Check = |_| Check::Current;
    const NOT_ACTIVE: fn(&Card) -> bool = |_| false;

    #[test]
    fn a_card_shows_after_the_delay_and_hides_after_its_time() {
        let t0 = Instant::now();
        let mut c = Cards::default();
        c.receive(card(Kind::Finished, "p1"), t0);
        assert!(!c.tick(t0 + DELAY - Duration::from_millis(1), OK, NOT_ACTIVE));
        assert!(c.visible().is_none(), "herdr: delay_seconds 뒤에 띄운다");
        assert!(c.tick(t0 + DELAY, OK, NOT_ACTIVE));
        assert_eq!(c.visible().unwrap().title, "Finished p1");
        // 끝남 5초, 입력 필요 8초
        assert!(!c.tick(t0 + DELAY + Duration::from_millis(4_999), OK, NOT_ACTIVE));
        assert!(c.tick(t0 + DELAY + Duration::from_secs(5), OK, NOT_ACTIVE));
        assert!(c.visible().is_none());
        c.receive(card(Kind::NeedsInput, "p2"), t0);
        c.tick(t0 + DELAY, OK, NOT_ACTIVE);
        c.tick(t0 + DELAY + Duration::from_millis(7_999), OK, NOT_ACTIVE);
        assert!(c.visible().is_some());
        c.tick(t0 + DELAY + Duration::from_secs(8), OK, NOT_ACTIVE);
        assert!(c.visible().is_none());
    }

    #[test]
    fn the_active_tab_and_stale_states_show_nothing() {
        let t0 = Instant::now();
        let mut c = Cards::default();
        c.receive(card(Kind::Finished, "here"), t0);
        c.receive(card(Kind::Finished, "seen"), t0);
        c.receive(card(Kind::Report, "here"), t0);
        let check = |k: &Card| if k.pane.as_deref() == Some("seen") { Check::Stale } else { Check::Current };
        let active = |k: &Card| k.pane.as_deref() == Some("here");
        c.tick(t0 + DELAY, check, active);
        // 지금 탭의 끝남은 거르고, 그 사이 본 pane도 거르고, 새 보고는 지금 탭이어도 띄운다(herdr Custom)
        assert_eq!(titles(&c), (Some("Report here".into()), vec![]));
    }

    #[test]
    fn a_finish_still_reading_working_is_rechecked_until_the_grace_ends() {
        let t0 = Instant::now();
        let mut c = Cards::default();
        c.receive(card(Kind::Finished, "p"), t0);
        let awaiting = |_: &Card| Check::Awaiting;
        c.tick(t0 + DELAY, awaiting, NOT_ACTIVE);
        assert!(c.visible().is_none());
        assert!(c.next_deadline().is_none(), "기다림 한도(1초)가 지연과 같아 다시 보지 않고 버린다(herdr 그대로)");
        // 지연이 없는 새 보고 옆의 끝남은 한도 안에서 다시 본다
        let mut c = Cards::default();
        c.pending.push(Pending {
            card: card(Kind::Finished, "p"),
            deadline: t0,
            expires_at: t0 + GRACE,
            validate: true,
        });
        c.tick(t0, awaiting, NOT_ACTIVE);
        assert_eq!(c.next_deadline(), Some(t0 + RECHECK));
        c.tick(t0 + RECHECK, OK, NOT_ACTIVE);
        assert!(c.visible().is_some());
    }

    #[test]
    fn one_card_at_a_time_queue_of_eight_and_same_pane_replaces() {
        let t0 = Instant::now();
        let mut c = Cards::default();
        for i in 0..10 {
            c.receive(card(Kind::Finished, &format!("p{i}")), t0);
        }
        c.tick(t0 + DELAY, OK, NOT_ACTIVE);
        let (shown, queued) = titles(&c);
        assert_eq!(shown.as_deref(), Some("Finished p0"));
        // 줄은 8개, 넘치면 가장 오래된 것을 버린다
        assert_eq!(queued.len(), 8);
        assert_eq!(queued.first().map(String::as_str), Some("Finished p2"));
        // 같은 pane의 새 알림은 보이던 카드를 지우고 다음을 올린다
        assert!(c.receive(card(Kind::NeedsInput, "p0"), t0 + DELAY));
        assert_eq!(c.visible().unwrap().title, "Finished p2");
        // 줄 선 같은 pane도 지운다
        c.receive(card(Kind::NeedsInput, "p5"), t0 + DELAY);
        assert!(!titles(&c).1.contains(&"Finished p5".to_string()));
        // 새 보고는 같은 pane이어도 지우지 않고 지워지지 않는다
        c.receive(card(Kind::Report, "p2"), t0 + DELAY);
        assert_eq!(c.visible().unwrap().title, "Finished p2");
        c.tick(t0 + DELAY, OK, NOT_ACTIVE);
        c.receive(card(Kind::Finished, "p2"), t0 + DELAY);
        assert!(titles(&c).1.contains(&"Report p2".to_string()));
    }

    #[test]
    fn taking_the_card_promotes_the_next() {
        let t0 = Instant::now();
        let mut c = Cards::default();
        c.receive(card(Kind::Finished, "a"), t0);
        c.receive(card(Kind::NeedsInput, "b"), t0);
        c.tick(t0 + DELAY, OK, NOT_ACTIVE);
        assert_eq!(c.take(t0 + DELAY).unwrap().pane.as_deref(), Some("a"));
        assert_eq!(c.visible().unwrap().pane.as_deref(), Some("b"));
        assert_eq!(c.take(t0 + DELAY).unwrap().pane.as_deref(), Some("b"));
        assert!(c.take(t0 + DELAY).is_none(), "카드가 없으면 아무것도 하지 않는다(herdr)");
    }
}
