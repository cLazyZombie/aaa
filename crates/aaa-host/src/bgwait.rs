//! 배경 작업을 기다리는 쉼(차이 목록 S1 `waiting`, plan §8.189). herdr-web-ui(aa624cb) `server/background-wait.ts` `BackgroundWait`를
//! 옮겼다.
//!
//! Claude Code 턴이 하위 agent나 배경 명령(시험·빌드)을 남긴 채 끝나면 pane은 쉼(안 봤으면 끝남)이 되지만, 그 일의 알림이 사람의
//! 입력 없이 다음 턴을 연다. 그래서 쉬는 pane의 턴이 띄운 일이 아직 돌거나 끝난 지 `GRACE_MS`가 안 됐으면 「배경」이다: 목록은
//! 끝남 대신 BG라고 한다. 턴보다 먼저 끝난 일은 아무것도 붙잡지 않는다. 한도(`LIMIT_MS`)는 사람이 입력한 뒤 첫 쉼부터 세고,
//! 저절로 이어진 턴마다 새로 세지 않는다. 처음부터 쉬던 pane은 붙잡지 않는다.
//!
//! 참고와 다른 곳: 배경 수는 agent `plan.rs`가 센 「지금 Claude 프로세스가 띄워 아직 도는 것」이다(참고는 그 턴이 띄운 것만).
//! 참고처럼 이 판정으로 「작업 끝남」 알림도 미룬다(notice.rs `alert_activity`, 참고 `alertStatus`): 기다림이 끝날 때 한 번이다.

use std::collections::HashMap;

use aaa_proto::AgentActivity;

/// 일이 끝난 뒤 쉼을 더 붙잡는 시간: Claude는 알림을 받고 2초쯤 뒤 이어 간다
pub(crate) const GRACE_MS: u64 = 60_000;
/// 쉼을 붙잡는 가장 긴 시간. 기록으로는 시험 묶음을 기다리는 턴과 dev server를 띄워 둔 턴을 가를 수 없어, 서버만 띄운 턴은
/// 이만큼 붙잡힌 뒤 끝남이 된다
pub(crate) const LIMIT_MS: u64 = 30 * 60_000;

#[derive(Default)]
struct Pane {
    busy: bool,
    /// 본 쉼들. 사람 입력 뒤 첫 쉼이 한도를 가진다
    rests: Vec<u64>,
    prompt_at: Option<u64>,
    /// 그 턴이 띄워 아직 도는 하위 agent·배경 명령 수
    running: u32,
    /// 쉬는 동안 그 마지막 것이 끝난 때
    ended_at: Option<u64>,
    waiting: bool,
}

pub(crate) struct BackgroundWait {
    panes: HashMap<String, Pane>,
    grace: u64,
    limit: u64,
}

impl Default for BackgroundWait {
    fn default() -> Self {
        Self::new(GRACE_MS, LIMIT_MS)
    }
}

impl BackgroundWait {
    pub(crate) fn new(grace: u64, limit: u64) -> Self {
        Self { panes: HashMap::new(), grace, limit }
    }

    /// 상태가 바뀌었다. 기다림이 바뀌었으면 true
    pub(crate) fn status(&mut self, pane: &str, activity: AgentActivity, now: u64) -> bool {
        let p = self.panes.entry(pane.to_string()).or_default();
        if matches!(activity, AgentActivity::Working | AgentActivity::Question) {
            p.busy = true;
            p.ended_at = None;
        } else if p.busy {
            // 끝남과 쉼은 한 쉼이다: 본 끝남은 새 쉼이 아니다
            p.busy = false;
            p.rests.push(now);
        }
        self.settle(pane, now)
    }

    /// 사람이 입력했다(UserPromptSubmit): 한도를 새로 센다
    pub(crate) fn prompt(&mut self, pane: &str, at: u64, now: u64) -> bool {
        let p = self.panes.entry(pane.to_string()).or_default();
        p.prompt_at = Some(at);
        p.ended_at = None;
        self.settle(pane, now)
    }

    /// 그 pane이 띄워 아직 도는 하위 agent·배경 명령 수. 기다림이 바뀌었으면 true
    pub(crate) fn running(&mut self, pane: &str, running: u32, now: u64) -> bool {
        let p = self.panes.entry(pane.to_string()).or_default();
        if running == 0 && p.running > 0 && !p.busy {
            p.ended_at = Some(now);
        }
        p.running = running;
        self.settle(pane, now)
    }

    /// 지난번 뒤로 기다림이 끝난(유예·한도가 다 된) pane들
    pub(crate) fn tick(&mut self, now: u64) -> Vec<String> {
        let ids: Vec<String> = self.panes.keys().cloned().collect();
        ids.into_iter().filter(|id| self.settle(id, now)).collect()
    }

    pub(crate) fn waiting(&self, pane: &str) -> bool {
        self.panes.get(pane).is_some_and(|p| p.waiting)
    }

    /// 기다리는 pane 가운데 가장 이른, 기다림이 저절로 끝날 수 있는 때(ticker가 그때 깬다)
    pub(crate) fn next_due(&self) -> Option<u64> {
        self.panes
            .values()
            .filter(|p| p.waiting)
            .flat_map(|p| {
                let rest =
                    p.rests.iter().copied().find(|&at| p.prompt_at.is_none_or(|q| at >= q)).map(|at| at + self.limit);
                let grace = (p.running == 0).then_some(p.ended_at.map(|at| at + self.grace)).flatten();
                [rest, grace]
            })
            .flatten()
            .min()
    }

    /// 새 harness 수명(다시 띄움·끝남): 그 수명의 기다림을 버린다
    pub(crate) fn forget(&mut self, pane: &str) {
        self.panes.remove(pane);
    }

    fn settle(&mut self, pane: &str, now: u64) -> bool {
        let (grace, limit) = (self.grace, self.limit);
        let Some(p) = self.panes.get_mut(pane) else { return false };
        // 지난 쉼 하나는 남긴다: 모두 지우면 이어진 턴에 옛 입력의 한도가 새로 생긴다
        while p.rests.len() > 1 && now.saturating_sub(p.rests[1]) >= limit {
            p.rests.remove(0);
        }
        let rest_at = p.rests.iter().copied().find(|&at| p.prompt_at.is_none_or(|q| at >= q));
        let waiting = !p.busy
            && rest_at.is_some_and(|at| now.saturating_sub(at) < limit)
            && (p.running > 0 || p.ended_at.is_some_and(|at| now.saturating_sub(at) < grace));
        let changed = waiting != p.waiting;
        p.waiting = waiting;
        changed
    }
}

impl crate::Host {
    /// 기다림이 저절로 끝날 수 있는 가장 이른 때(ticker가 그때 깬다)
    pub(crate) fn background_wait_due(&self) -> Option<std::time::Instant> {
        let due = self.bgwait.lock().unwrap().next_due()?;
        let left = due.saturating_sub(aaa_common::util::now_ms());
        Some(std::time::Instant::now() + std::time::Duration::from_millis(left))
    }

    /// 유예·한도가 다 된 기다림을 풀고 그 pane 상태를 다시 낸다. 알림은 그 기다림을 작업 중으로 보았으므로(notice.rs
    /// `alert_activity`, 참고 server/index.ts `waitsRanOut`) 쉬는 pane이면 이제 그 턴의 끝남이다
    pub(crate) fn settle_background_waits(&self) {
        let now = aaa_common::util::now_ms();
        let ended = self.bgwait.lock().unwrap().tick(now);
        if ended.is_empty() {
            return;
        }
        let mut finished = Vec::new();
        {
            let mut i = self.inner.lock().unwrap();
            let w = self.bgwait.lock().unwrap();
            for id in &ended {
                if let Some(p) = i.panes.get_mut(id) {
                    if let Some(a) = p.agent.as_mut() {
                        let was = p.alert.unwrap_or(crate::notice::alert_activity(a.activity, a.background_wait));
                        a.background_wait = w.waiting(id);
                        let next = crate::notice::alert_activity(a.activity, a.background_wait);
                        p.alert = Some(next);
                        if was != next {
                            let a = a.clone();
                            if crate::notice::finished(Some(was), next, a.seen) {
                                let turn_ms = p.turn_from.map(|from| now.saturating_sub(from));
                                finished.push(crate::notice::finish_notice(id, p, &a, turn_ms));
                            }
                            p.turn_from = crate::notice::turn_from(Some(was), next, p.turn_from, now);
                        }
                    }
                    let _ = p.events.send(crate::PaneEvent::State);
                }
            }
        }
        self.bump();
        for n in finished {
            self.announce_finish(n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentActivity::{Idle, Question, Working};

    // 참고 server/background-wait.test.ts의 Claude 경우들(87·106·121·134·70·58줄의 뜻, 유예 10·한도 100으로 줄임)
    fn bw() -> BackgroundWait {
        BackgroundWait::new(10, 100)
    }

    #[test]
    fn a_turn_that_ends_with_work_running_waits_until_the_grace_after_it_ends() {
        let mut w = bw();
        w.prompt("p", 0, 0);
        w.status("p", Working, 1);
        w.running("p", 2, 2);
        assert!(!w.waiting("p"), "도는 동안은 작업 중이지 기다림이 아니다");
        assert!(w.status("p", Idle, 5));
        assert!(w.waiting("p"));
        assert_eq!(w.next_due(), Some(105), "한도까지");
        assert!(!w.running("p", 1, 6));
        assert!(!w.running("p", 0, 7), "끝나도 유예 동안은 기다린다");
        assert!(w.waiting("p"));
        assert_eq!(w.next_due(), Some(17), "유예 끝");
        assert!(w.tick(16).is_empty());
        assert_eq!(w.tick(17), ["p"]);
        assert!(!w.waiting("p"));
    }

    #[test]
    fn the_next_turn_ends_the_wait_and_work_ended_before_the_rest_holds_nothing() {
        let mut w = bw();
        w.status("p", Working, 0);
        w.running("p", 1, 1);
        w.status("p", Idle, 2);
        assert!(w.waiting("p"));
        // 일의 알림이 다음 턴을 열었다
        assert!(w.status("p", Working, 3));
        assert!(!w.waiting("p"));
        // 턴 안에서 끝난 일은 쉼을 붙잡지 않는다
        w.running("p", 0, 4);
        assert!(!w.status("p", Idle, 5));
        assert!(!w.waiting("p"));
        // 쉬는 동안 일이 새로 돌면 기다리고, 질문은 바쁜 것이라 기다림이 아니다
        assert!(w.running("p", 1, 6));
        assert!(w.status("p", Question, 7));
        assert!(!w.waiting("p"));
    }

    #[test]
    fn a_resting_baseline_holds_nothing() {
        let mut w = bw();
        assert!(!w.running("p", 3, 0));
        assert!(!w.status("p", Idle, 1));
        assert!(!w.waiting("p"));
        assert_eq!(w.next_due(), None);
    }

    #[test]
    fn the_limit_counts_from_the_first_rest_after_the_prompt_not_each_resume() {
        let mut w = bw();
        w.prompt("p", 0, 0);
        w.status("p", Working, 1);
        w.running("p", 1, 2);
        w.status("p", Idle, 10);
        // 저절로 이어진 턴(한 하위 agent가 끝나 알림)과 그 쉼: 한도는 첫 쉼(10)에서 센다
        w.status("p", Working, 50);
        w.status("p", Idle, 60);
        assert!(w.waiting("p"));
        assert_eq!(w.next_due(), Some(110));
        assert_eq!(w.tick(110), ["p"], "서버만 띄운 턴도 한도에서 끝남이 된다");
        // 사람이 새로 입력하면 새 한도
        w.prompt("p", 200, 200);
        w.status("p", Working, 201);
        assert!(w.status("p", Idle, 210));
        assert_eq!(w.next_due(), Some(310));
    }

    #[test]
    fn forget_drops_the_lifetime() {
        let mut w = bw();
        w.status("p", Working, 0);
        w.running("p", 1, 1);
        w.status("p", Idle, 2);
        w.forget("p");
        assert!(!w.waiting("p"));
        assert!(w.tick(3).is_empty());
    }
}
