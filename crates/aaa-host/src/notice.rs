//! 앱 안 알림(plan §8.185): 다른 agent의 턴이 끝났다·thread에 새 보고가 왔다를 front(TUI 카드·웹 Droplet)에 알린다.
//! 휴대폰 push는 아니다: inbox 항목을 만들지 않으므로 notify.rs·push.rs가 보지 않는다.
//!
//! - 끝남(herdr 0.9.5 `src/app/actions.rs` 77~93 `is_background_completion_transition`: 작업·질문에서 쉼으로)은 그때 그 pane을
//!   보는 화면이 없을 때만이다. herdr 화면은 `Finished`를 그 pane이 아직 「끝남(안 봄)」일 때만 띄운다
//!   (`src/client/shell/notification_policy.rs` 319~370 `validate_state`, `src/app/api_helpers.rs` 100~102). aaa에서 그것은
//!   `seen.rs`의 「봤음」이다(보는 웹 화면·TUI lease).
//! - 턴 길이는 작업·질문으로 들어선 때부터 잰다(herdr-web-ui `src/lib/droplet.ts` 82~92 `trackTurn`: 질문 동안도 턴이다).
//!   host가 그 턴의 시작을 못 봤으면 모른다(null). 웹은 긴 턴(60초 이상이거나 모름, 같은 파일 33·42~47)만 띄운다.
//! - thread pane의 끝남은 그 턴이 낸 보고 확인(orch.rs `refresh_report`)과 ticker의 묶음 셈이 끝날 때까지 미룬다. 같은 턴에
//!   새 보고가 왔으면 새 보고 알림 하나만 간다(brief 지시, 참고와 다름: herdr에서는 herdr의 끝남 카드와 herdr-projects의
//!   「new report」 카드가 따로 뜬다).
//! - 입력 필요는 여기서 만들지 않는다: front가 `Question` 사건(질문이 열림)으로 띄운다.
use aaa_proto::{AgentActivity, NoticeKind, NoticeView};

use crate::Host;
use crate::orch::ThreadRec;

/// 긴 턴(herdr-web-ui `LONG_TURN_MS`). `AAA_TEST_LONG_TURN_MS`는 시험에서만 줄인다(웹 E2E 11)
pub(crate) fn long_turn_ms() -> u64 {
    std::env::var("AAA_TEST_LONG_TURN_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(60_000)
}

fn in_turn(a: Option<AgentActivity>) -> bool {
    matches!(a, Some(AgentActivity::Working | AgentActivity::Question))
}

/// 상태가 바뀐 뒤 턴이 시작된 때: 쉼에서 작업·질문으로 들어서면 지금, 턴 안이면 그대로, 턴 밖이면 없음
pub(crate) fn turn_from(prev: Option<AgentActivity>, next: AgentActivity, from: Option<u64>, at: u64) -> Option<u64> {
    match (in_turn(prev), in_turn(Some(next))) {
        (false, true) => Some(at),
        (true, true) => from,
        _ => None,
    }
}

/// 이 바뀜이 알릴 끝남이다(herdr `Finished`): 작업·질문에서 쉼으로, 그때 보는 화면이 없었다(`seen` false)
pub(crate) fn finished(prev: Option<AgentActivity>, next: AgentActivity, seen: bool) -> bool {
    next == AgentActivity::Idle && in_turn(prev) && !seen
}

/// 끝난 턴이 긴가: 길이를 모르면 긴 것으로 친다(herdr-web-ui `dropletAllows`: 「a turn of unknown length is announced」)
pub(crate) fn is_long(turn_ms: Option<u64>, long_ms: u64) -> bool {
    turn_ms.is_none_or(|t| t >= long_ms)
}

impl Host {
    pub(crate) fn send_notice(&self, n: NoticeView) {
        tracing::debug!(kind = ?n.kind, pane = ?n.pane, "앱 안 알림");
        let _ = self.notice_events.send(n);
    }

    /// 보는 화면 없이 끝난 턴. thread pane이면 ticker의 묶음 셈(`thread_pass`)까지 미룬다: 그 셈이 같은 턴의 새 보고를 알리면
    /// 끝남은 보내지 않는다. 사후 평가자 pane은 알리지 않는다(목록에 섞지 않는다, §8.13).
    /// 멈춘 프로젝트는 ticker가 셈하지 않으므로 바로 보낸다
    pub(crate) fn turn_finished(&self, n: NoticeView, thread: Option<&ThreadRec>) {
        match thread {
            Some(r) if r.role == aaa_proto::Role::Evaluator => {}
            Some(r) if self.active_projects().iter().any(|p| p.name == r.project) => {
                if let Some(pane) = n.pane.clone() {
                    self.turn_ends.lock().unwrap().insert(pane, n);
                }
            }
            _ => self.send_notice(n),
        }
    }

    /// `thread_pass`가 그 thread를 셈했다: 미룬 끝남을 꺼낸다. 같은 셈에서 새 보고를 알렸거나(`reported`) 그 사이 pane이
    /// 끝남(안 봄)이 아니게 됐으면(봤다·다시 일한다) 보내지 않는다(herdr `validate_state`)
    pub(crate) fn settle_turn_end(&self, pane: &str, reported: bool) {
        let Some(n) = self.turn_ends.lock().unwrap().remove(pane) else { return };
        let unseen_idle = self
            .inner
            .lock()
            .unwrap()
            .panes
            .get(pane)
            .and_then(|p| p.agent.as_ref())
            .is_some_and(|a| a.activity == AgentActivity::Idle && !a.seen);
        if !reported && unseen_idle {
            self.send_notice(n);
        }
    }

    /// thread의 새 보고(host 「new report」 항목과 같은 때, herdr-projects steps.rs 244 「review · new report」)
    pub(crate) fn report_notice(&self, project: &str, r: &ThreadRec) {
        self.send_notice(NoticeView {
            kind: NoticeKind::Report,
            pane: r.pane.clone(),
            machine: r.machine_id.clone(),
            project: Some(project.to_string()),
            thread: Some(r.id.clone()),
            title: r.title.clone(),
            agent: None,
            turn_ms: None,
            long: true,
            at_ms: aaa_common::util::now_ms(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentActivity::*;

    #[test]
    fn a_turn_runs_from_work_or_question_to_rest() {
        // 쉼에서 작업으로: 턴이 지금 시작한다. 질문을 지나도 같은 턴이다(herdr-web-ui trackTurn)
        assert_eq!(turn_from(Some(Idle), Working, None, 10), Some(10));
        assert_eq!(turn_from(Some(Working), Question, Some(10), 20), Some(10));
        assert_eq!(turn_from(Some(Question), Working, Some(10), 30), Some(10));
        assert_eq!(turn_from(Some(Working), Idle, Some(10), 40), None);
        // 처음 보는 agent·시작 뒤 바로 질문도 턴의 시작이다
        assert_eq!(turn_from(None, Question, None, 5), Some(5));
        assert_eq!(turn_from(Some(Started), Working, None, 5), Some(5));
        // host가 시작을 못 봤다(다시 붙은 agent가 이미 작업 중): 모른다
        assert_eq!(turn_from(Some(Working), Working, None, 50), None);
        // 끝남·오류도 턴 밖이다
        assert_eq!(turn_from(Some(Working), Exited, Some(10), 60), None);
    }

    #[test]
    fn only_an_unseen_finish_from_a_turn_is_announced() {
        assert!(finished(Some(Working), Idle, false));
        assert!(finished(Some(Question), Idle, false));
        // 보는 화면이 있었다
        assert!(!finished(Some(Working), Idle, true));
        // 시작 뒤 첫 쉼·쉼 → 쉼은 턴의 끝이 아니다(herdr: 첫 Idle은 끝남이 아니다)
        assert!(!finished(Some(Started), Idle, false));
        assert!(!finished(Some(Idle), Idle, false));
        assert!(!finished(None, Idle, false));
        // 쉼이 아닌 곳으로 간 것은 끝남이 아니다
        assert!(!finished(Some(Working), Exited, false));
        assert!(!finished(Some(Working), Question, false));
    }

    #[test]
    fn long_turns_start_at_the_reference_minute() {
        assert!(!is_long(Some(59_999), 60_000));
        assert!(is_long(Some(60_000), 60_000));
        assert!(is_long(None, 60_000), "길이를 모르는 턴은 알린다");
    }
}
