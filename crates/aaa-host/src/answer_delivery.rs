//! 사용량 한도 화면에서 입력을 시작하지 못한 카드 답. 같은 보고·pane에서만 한 번 입력한다.

use std::sync::Arc;

use aaa_proto::{
    AgentActivity, AnswerDelivery, BriefState, DeliveryState, ErrorBody, ErrorCode, MessageView, ThreadStatus,
};
use serde::{Deserialize, Serialize};

use crate::orch::{ThreadRec, internal};
use crate::{Host, err};

const WAITING: &str = "answer_waiting";
pub(crate) const WAITING_EVENT: &str = "question answer waiting";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AnswerTarget {
    report: String,
    project: String,
    thread: String,
    report_changed_ms: Option<u64>,
    pub(crate) machine: String,
    pub(crate) pane: String,
    pane_started_ms: u64,
    pane_boot_id: String,
    harness_session: Option<String>,
    answer_at_ms: u64,
}

impl AnswerTarget {
    pub(crate) fn same_report(&self, rec: &ThreadRec) -> bool {
        rec.report_hash.as_deref() == Some(self.report.as_str()) && rec.report_changed_ms == self.report_changed_ms
    }

    fn same_answer(&self, rec: &ThreadRec) -> bool {
        self.same_report(rec) && rec.question_answer.as_ref().is_some_and(|a| a.at_ms == self.answer_at_ms)
    }

    fn thread_matches(&self, rec: &ThreadRec) -> bool {
        self.same_answer(rec)
            && rec.status == ThreadStatus::Open
            && rec.brief == BriefState::Delivered
            && rec.machine_id == self.machine
            && rec.pane.as_deref() == Some(self.pane.as_str())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PendingAnswer {
    #[serde(flatten)]
    pub(crate) message: MessageView,
    pub(crate) answer_target: AnswerTarget,
}

#[derive(Clone, Copy)]
pub(crate) enum CardSend<'a> {
    Initial(&'a AnswerTarget, &'a str),
    Resume(&'a PendingAnswer),
}

impl CardSend<'_> {
    fn target(&self) -> &AnswerTarget {
        match self {
            Self::Initial(target, _) => target,
            Self::Resume(pending) => &pending.answer_target,
        }
    }
}

impl Host {
    pub(crate) fn answer_target(
        &self,
        rec: &ThreadRec,
        report: &str,
        answer_at_ms: u64,
    ) -> Result<AnswerTarget, ErrorBody> {
        let pane = rec.pane.as_ref().ok_or_else(|| err(ErrorCode::BadRequest, "thread 세션이 없다"))?;
        self.check_running(pane)?;
        let inner = self.inner.lock().unwrap();
        let record = inner.panes.get(pane).ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))?;
        Ok(AnswerTarget {
            report: report.into(),
            project: rec.project.clone(),
            thread: rec.id.clone(),
            report_changed_ms: rec.report_changed_ms,
            machine: rec.machine_id.clone(),
            pane: pane.clone(),
            pane_started_ms: record.info.started_at_ms,
            pane_boot_id: record.info.boot_id.clone(),
            harness_session: record.agent.as_ref().and_then(|agent| agent.harness_session.clone()),
            answer_at_ms,
        })
    }

    // 연결이 끊겼거나 재연결 뒤 아직 pane 목록을 못 받았으면 기다린다. 다른 실행이나 종료를 확인하면 보류한다.
    fn answer_pane_matches(&self, target: &AnswerTarget) -> Option<bool> {
        let inner = self.inner.lock().unwrap();
        if inner.panes.values().any(|pane| pane.replaced.iter().any(|old| old == &target.pane)) {
            return Some(false);
        }
        let pane = inner.panes.get(&target.pane)?;
        let agent = pane.agent.as_ref();
        if pane.machine_id != target.machine
            || pane.info.started_at_ms != target.pane_started_ms
            || pane.info.boot_id != target.pane_boot_id
            || !pane.info.state.is_running()
            || pane.kill_sent
            || agent.is_some_and(|a| a.activity == AgentActivity::Exited)
        {
            return Some(false);
        }
        match target.harness_session.as_deref() {
            Some(expected) => agent?.harness_session.as_deref().map(|current| current == expected),
            None => Some(true),
        }
    }

    pub(crate) fn card_submission_current(&self, card: CardSend<'_>, machine: &str, pane: &str) -> bool {
        let target = card.target();
        target.machine == machine
            && target.pane == pane
            && self.load(&target.project, &target.thread).is_ok_and(|r| target.thread_matches(&r))
            && self.answer_pane_matches(target) == Some(true)
    }

    fn save_pending_answer(&self, pending: &PendingAnswer, state: &str) -> Result<(), ErrorBody> {
        let m = &pending.message;
        let data = serde_json::to_string(pending).map_err(internal)?;
        self.db.rec_put("messages", &m.id, state, &data, (&m.project, &m.thread)).map_err(internal)
    }

    /// 같은 메시지 ID를 유지한다. 재개 전 이 행을 옮겨야 host 재시작 때 이미 시작한 입력을 다시 보내지 않는다.
    pub(crate) fn claim_pending_answer(&self, pending: &PendingAnswer) -> Result<bool, ErrorBody> {
        self.db.advance_waiting_answer(&pending.message.id, "queued").map_err(internal)
    }

    pub(crate) fn defer_card_answer(
        &self,
        m: &MessageView,
        target: &AnswerTarget,
        inbox_body: &str,
    ) -> Result<(), ErrorBody> {
        let pending = PendingAnswer { message: m.clone(), answer_target: target.clone() };
        self.save_pending_answer(&pending, WAITING)?;
        self.set_answer_delivery(&pending, Some(AnswerDelivery::Waiting));
        self.inbox_add_body(
            &m.project,
            "thread-state",
            &m.thread,
            WAITING_EVENT,
            &format!("{}: {} · 카드 답 전달 대기", m.thread, m.id),
            inbox_body,
        );
        Ok(())
    }

    fn set_answer_delivery(&self, pending: &PendingAnswer, delivery: Option<AnswerDelivery>) {
        let m = &pending.message;
        if let Err(e) = self.update_rec(&m.project, &m.thread, |r| {
            if pending.answer_target.same_answer(r)
                && let Some(answer) = &mut r.question_answer
            {
                answer.delivery = delivery;
            }
        }) {
            tracing::warn!(message = %m.id, "카드 답 전달 상태를 저장하지 못했다: {}", e.message);
        }
    }

    pub(crate) fn finish_pending_answer(&self, pending: &PendingAnswer, state: DeliveryState) {
        let m = &pending.message;
        self.set_answer_delivery(pending, None);
        let (event, summary) = if state == DeliveryState::Accepted {
            ("question answered", "카드 답 전달됨")
        } else {
            ("question answer unconfirmed", "카드 답 전달 확인 필요")
        };
        self.inbox_answer_delivery(m, event, summary);
    }

    fn hold_pending_answer(&self, pending: &PendingAnswer, reason: &str) -> Result<(), ErrorBody> {
        if !self.db.advance_waiting_answer(&pending.message.id, "answer_held").map_err(internal)? {
            return Ok(());
        }
        self.set_answer_delivery(pending, Some(AnswerDelivery::Held));
        self.inbox_answer_delivery(&pending.message, "question answer held", &format!("카드 답 전달 보류 · {reason}"));
        Ok(())
    }

    pub(crate) async fn pending_answer_pass(self: &Arc<Self>, project: &str) {
        let rows = match self.db.pending_answers(project) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!(project, "대기 중인 카드 답을 읽지 못했다: {e:#}");
                return;
            }
        };
        for row in rows {
            let pending = match serde_json::from_str(&row) {
                Ok(pending) => pending,
                Err(e) => {
                    tracing::warn!(project, "대기 중인 카드 답의 형식 오류: {e}");
                    continue;
                }
            };
            if let Err(e) = self.resume_pending_answer(&pending).await {
                tracing::warn!(project, message = %pending.message.id, "대기 중인 카드 답을 확인하지 못했다: {}", e.message);
            }
        }
    }

    async fn resume_pending_answer(self: &Arc<Self>, pending: &PendingAnswer) -> Result<(), ErrorBody> {
        let m = &pending.message;
        let target = &pending.answer_target;
        let rec = match self.load(&m.project, &m.thread) {
            Ok(rec) => rec,
            Err(e) if e.code == ErrorCode::NotFound => return self.hold_pending_answer(pending, "thread가 사라졌다"),
            Err(e) => return Err(e),
        };
        let same_pane = self.answer_pane_matches(target);
        if !target.thread_matches(&rec) || same_pane == Some(false) {
            return self.hold_pending_answer(pending, "보고나 실행 중인 pane이 바뀌었다");
        }
        if same_pane != Some(true) || !self.card_prompt_ready(&target.machine, &target.pane).await {
            return Ok(());
        }
        let Ok(_guard) = self.deliver_lock.try_lock() else { return Ok(()) };
        let mut message = m.clone();
        // 대기 표지는 입력을 시작하기 직전에만 제거한다. 화면이 다시 막힌 경우 다음 tick에서 본다.
        self.deliver(&mut message, &target.machine, &target.pane, Some(CardSend::Resume(pending))).await?;
        if message.state != DeliveryState::Queued {
            if let Some(answer) = &rec.question_answer {
                let mut answer = answer.clone();
                answer.delivery = None;
                self.record_answer(&rec, &answer);
            }
            self.reconcile_question_requests(&m.project)?;
        }
        Ok(())
    }

    pub(crate) async fn pending_report_current(self: &Arc<Self>, pending: &PendingAnswer) -> Result<bool, ErrorBody> {
        let m = &pending.message;
        let target = &pending.answer_target;
        let rec = self.load(&m.project, &m.thread)?;
        let conn = self.live_conn(&target.machine)?;
        if self.current_report_hash(&rec, &conn).await? == target.report {
            return Ok(true);
        }
        self.hold_pending_answer(pending, "새 보고가 왔다")?;
        self.refresh_report(&m.project, &m.thread);
        Ok(false)
    }

    async fn card_prompt_ready(&self, machine: &str, pane: &str) -> bool {
        let Some(screen) = self.screen(machine, pane).await else { return false };
        self.card_screen_ready(pane, &screen)
    }

    pub(crate) fn card_screen_ready(&self, pane: &str, screen: &aaa_proto::ScreenText) -> bool {
        self.pane_agent(pane).is_some_and(|agent| {
            matches!(agent.activity, AgentActivity::Idle | AgentActivity::Working)
                && crate::harness::modal_of(&screen.lines).is_none()
                && !crate::harness::limit_wait_screen(&screen.lines)
                && crate::prompt_box::check(&agent.harness, &screen.ansi) == crate::prompt_box::Draft::Empty
        })
    }
}
