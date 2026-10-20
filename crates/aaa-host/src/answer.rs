//! thread 질문 카드의 답(plan §8.329). thread가 보고 `## Questions`에 남긴 질문에 웹 카드·`aaa thread answer`로 답한다.
//!
//! - 답은 그 보고(hash)의 질문에만 맞는다. 새 보고가 오면 질문을 다시 읽고 답을 비운다(orch.rs 사본).
//! - 한 번만 받는다: 기록에 먼저 자리를 잡고(`question_answer`) 보낸다. 다른 기기가 늦게 누르면 「이미 답함」이다. 보내지
//!   못하면 자리를 풀어 다시 답할 수 있다.
//! - 보내는 길은 `thread message`와 같다(확인창·입력창 글·brief 전 거부, task 기록). coordinator를 거치지 않는다.
//! - 웹에서 답하면 coordinator inbox에 「질문에 답함」을 남긴다: coordinator가 같은 질문을 다시 묻거나 중계하지 않는다.

use std::sync::Arc;

use aaa_proto::{ErrorBody, ErrorCode, MessageMode, QuestionAnswerView, ThreadQuestion, ThreadView};

use crate::{Host, err};

/// 답 글자(A·B·C…)
fn letter(i: u32) -> char {
    char::from(b'A' + i as u8)
}

/// 고른 것을 확인하고 thread에 보낼 글과 짧은 꼴(「1A 2B」)을 짓는다. 모든 질문은 고르거나 직접 쓴 답으로 대신한다
pub(crate) fn compose(
    items: &[ThreadQuestion],
    picks: &[Option<u32>],
    text: Option<&str>,
) -> Result<(String, String), String> {
    if picks.len() != items.len() {
        return Err(format!("질문은 {}개다(받은 답 {}개)", items.len(), picks.len()));
    }
    let text = text.map(str::trim).filter(|t| !t.is_empty());
    let mut short = Vec::new();
    let mut lines = Vec::new();
    for (n, (q, pick)) in items.iter().zip(picks).enumerate() {
        match pick {
            Some(i) => {
                let c = q
                    .choices
                    .get(*i as usize)
                    .ok_or_else(|| format!("{}번 질문에 {}번 선택지가 없다", n + 1, i + 1))?;
                short.push(format!("{}{}", n + 1, letter(*i)));
                lines.push(format!("{}{}. {} — {}", n + 1, letter(*i), q.text, c.label));
            }
            None if text.is_some() => {}
            None => return Err(format!("{}번 질문에 답이 없다(고르거나 직접 쓴다)", n + 1)),
        }
    }
    if let Some(t) = text {
        short.push("직접".into());
        lines.push(format!("직접 쓴 답: {t}"));
    }
    let message = format!("보고의 질문에 대한 사용자 답({})\n{}", short.join(" "), lines.join("\n"));
    Ok((message, short.join(" ")))
}

impl Host {
    /// 답한 곳의 이름: 웹 기기는 그 기기 이름(없으면 「웹」), 그 밖(CLI·coordinator)은 「CLI」
    fn answered_by(&self, actor: &str) -> String {
        match actor.strip_prefix("device:") {
            Some(id) => self
                .db
                .devices()
                .ok()
                .and_then(|ds| ds.into_iter().find(|d| d.id == id))
                .map(|d| d.label)
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| "웹".into()),
            None => "CLI".into(),
        }
    }

    pub(crate) async fn thread_answer(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        report_hash: &str,
        picks: Vec<Option<u32>>,
        text: Option<String>,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let rec = self.load(project, id)?;
        if rec.questions.is_empty() {
            return Err(err(ErrorCode::BadRequest, format!("{id}에는 답할 질문이 없다")));
        }
        if rec.report_hash.as_deref() != Some(report_hash) {
            return Err(err(ErrorCode::BadRequest, "질문이 바뀌었다(새 보고가 왔다). 다시 읽고 답한다"));
        }
        let (message, summary) =
            compose(&rec.questions, &picks, text.as_deref()).map_err(|e| err(ErrorCode::BadRequest, e))?;
        let by = self.answered_by(actor);
        let answer = QuestionAnswerView {
            picks: picks.clone(),
            text: text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
            summary: summary.clone(),
            by: by.clone(),
            at_ms: aaa_common::util::now_ms(),
        };
        // 자리를 먼저 잡는다: 두 기기가 함께 눌러도 한 번만 보낸다
        let mut taken: Option<QuestionAnswerView> = None;
        self.update_rec(project, id, |r| {
            if r.report_hash.as_deref() != Some(report_hash) {
                return;
            }
            match &r.question_answer {
                Some(old) => taken = Some(old.clone()),
                None => r.question_answer = Some(answer.clone()),
            }
        })?;
        if let Some(old) = taken {
            return Err(err(ErrorCode::BadRequest, format!("이미 답함({}): {}", old.by, old.summary)));
        }
        if let Err(e) = self.thread_message(project, id, message, MessageMode::Queue, actor).await {
            // 보내지 못했다: 다시 답할 수 있게 자리를 푼다(그사이 새 보고면 이미 비었다)
            let _ = self.update_rec(project, id, |r| {
                if r.question_answer.as_ref().is_some_and(|a| a.at_ms == answer.at_ms && a.by == answer.by) {
                    r.question_answer = None;
                }
            });
            return Err(e);
        }
        self.db.audit(actor, "thread_answer", &format!("{project}/{id} {summary}"));
        let rec = self.load(project, id)?;
        self.flow(&rec, "question_answered", Some(format!("threads/{id}.md")), format!("{summary} · {by}"));
        if actor.starts_with("device:") {
            self.inbox_add(
                project,
                "thread-state",
                id,
                "question answered",
                &format!("{id} 질문에 답함({by}): {summary}"),
            );
        }
        Ok(self.thread_view(&rec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::report_questions;

    fn qs() -> Vec<ThreadQuestion> {
        report_questions(
            "## Questions\n1. 로그인 단추 자리\n- A 오른쪽 아래 (추천)\n- B 가운데\n2. 눈 아이콘\n- A 넣는다 (추천)\n- B 넣지 않는다\n",
        )
    }

    /// 고른 것은 「1A 2B」와 질문·선택지 글로, 직접 쓴 답은 고르지 않은 질문을 대신한다. 빠진 답·없는 선택지·수가 다른 답은 거절한다
    #[test]
    fn compose_checks_every_question_and_writes_the_short_form() {
        let (msg, short) = compose(&qs(), &[Some(0), Some(1)], None).unwrap();
        assert_eq!(short, "1A 2B");
        assert_eq!(
            msg,
            "보고의 질문에 대한 사용자 답(1A 2B)\n1A. 로그인 단추 자리 — 오른쪽 아래\n2B. 눈 아이콘 — 넣지 않는다"
        );
        let (msg, short) = compose(&qs(), &[Some(0), None], Some(" 2 둘 다 보이게 ")).unwrap();
        assert_eq!(short, "1A 직접");
        assert!(msg.ends_with("직접 쓴 답: 2 둘 다 보이게"), "{msg}");
        assert!(compose(&qs(), &[Some(0), None], None).unwrap_err().contains("2번 질문에 답이 없다"));
        assert!(compose(&qs(), &[Some(0), None], Some("  ")).is_err());
        assert!(compose(&qs(), &[Some(0), Some(5)], None).unwrap_err().contains("2번 질문에 6번 선택지가 없다"));
        assert!(compose(&qs(), &[Some(0)], None).unwrap_err().contains("질문은 2개다"));
    }
}
