//! 사람이 읽는 질문 ID. 원래 thread·post·확인창 ID는 내부 대상/호환 별칭으로 남긴다.
use crate::{Host, err, orch::internal};
use aaa_proto::{ErrorBody, ErrorCode, FrontResponse, QuestionAnswerView, QuestionState, QuestionView, ThreadQuestion};
use std::sync::Arc;

pub(crate) struct Target {
    pub project: String,
    pub id: String,
    pub kind: String,
    pub subject: String,
    pub revision: String,
    pub position: usize,
    pub draft: Option<QuestionAnswerView>,
}

impl Host {
    pub(crate) fn identify_questions(
        &self,
        project: &str,
        kind: &str,
        subject: &str,
        revision: &str,
        items: &[ThreadQuestion],
    ) -> Result<Vec<ThreadQuestion>, ErrorBody> {
        items
            .iter()
            .enumerate()
            .map(|(position, q)| {
                let mut q = q.clone();
                q.id = Some(self.db.question_id(project, kind, subject, revision, position).map_err(internal)?);
                Ok(q)
            })
            .collect()
    }

    pub(crate) fn question_draft(&self, project: &str, items: &[ThreadQuestion]) -> Option<QuestionAnswerView> {
        let answers: Vec<_> = items
            .iter()
            .map(|q| {
                q.id.as_ref()
                    .and_then(|id| self.db.question_target(Some(project), id).ok().flatten())
                    .and_then(|t| t.draft)
            })
            .collect();
        if answers.iter().all(Option::is_none) {
            return None;
        }
        let picks = answers.iter().map(|a| a.as_ref().and_then(|a| a.picks.first().copied().flatten())).collect();
        let text = answers
            .iter()
            .enumerate()
            .filter_map(|(n, a)| a.as_ref()?.text.as_ref().map(|t| format!("{} {t}", n + 1)))
            .collect::<Vec<_>>()
            .join("\n");
        let last = answers.iter().flatten().max_by_key(|a| a.at_ms)?;
        Some(QuestionAnswerView {
            picks,
            text: (!text.is_empty()).then_some(text),
            summary: "일부 답함".into(),
            by: last.by.clone(),
            at_ms: last.at_ms,
        })
    }

    /// 확인창 목록도 같은 짧은 별칭으로 읽는다. 내부 확인창 ID·menu는 바꾸지 않는다.
    pub(crate) fn question_catalog(&self, all: bool) -> Result<Vec<QuestionView>, ErrorBody> {
        let mut out = self.questions(all);
        for q in &mut out {
            let project = q.project.clone().or_else(|| self.sole_project().ok().flatten()).unwrap_or_default();
            q.short_id = Some(self.db.question_id(&project, "harness", &q.id, "", 0).map_err(internal)?);
        }
        for t in self.threads_of(None, None)? {
            if t.status == aaa_proto::ThreadStatus::Resolved && !all {
                continue;
            }
            if let Some(q) = t.questions.as_ref().filter(|q| all || q.answer.is_none()) {
                for (n, item) in q.items.iter().enumerate() {
                    out.push(catalog_item(
                        &t.project,
                        "thread",
                        &t.id,
                        item,
                        q.answer.as_ref().or(q.draft.as_ref()),
                        n,
                        t.updated_at_ms,
                    ));
                }
            }
        }
        for project in self.projects_all().into_iter().map(|p| p.name) {
            for r in self.user_requests(&project)? {
                if let Some(q) = r.questions.as_ref() {
                    for (n, item) in q.items.iter().enumerate() {
                        out.push(catalog_item(&project, "coordinator", "", item, q.draft.as_ref(), n, r.created_ms));
                    }
                } else if let Some(id) = r.question_id {
                    out.push(catalog_item(
                        &project,
                        "coordinator",
                        "",
                        &ThreadQuestion { id: Some(id), text: r.title, body: None, choices: Vec::new() },
                        None,
                        0,
                        r.created_ms,
                    ));
                }
            }
            if all {
                for r in self.db.ask_list(&project).map_err(internal)? {
                    if r.items.is_empty()
                        && let Some(id) = r.question_id.as_ref()
                        && !out.iter().any(|q| &q.id == id)
                    {
                        let mut q = catalog_item(
                            &project,
                            "coordinator",
                            "",
                            &ThreadQuestion {
                                id: Some(id.clone()),
                                text: r.title.clone(),
                                body: None,
                                choices: Vec::new(),
                            },
                            None,
                            0,
                            r.asked_ms,
                        );
                        q.state = QuestionState::Gone;
                        out.push(q);
                    }
                    for (n, item) in r.items.iter().enumerate().filter(|(_, q)| q.id.is_some()) {
                        if out.iter().any(|q| Some(&q.id) == item.id.as_ref()) {
                            continue;
                        }
                        let mut q = catalog_item(
                            &project,
                            if r.kind == aaa_proto::AskKind::Question { "thread" } else { "coordinator" },
                            if r.kind == aaa_proto::AskKind::Question { &r.subject } else { "" },
                            item,
                            r.answer.as_ref(),
                            n,
                            r.asked_ms,
                        );
                        q.state = if r.answer.is_some() { QuestionState::Answered } else { QuestionState::Gone };
                        out.push(q);
                    }
                }
            }
        }
        for q in &mut out {
            if q.pane.is_empty()
                && let Some(t) = self.db.question_target(q.project.as_deref(), &q.id).map_err(internal)?
                && let Some(a) = t.draft
            {
                q.state = QuestionState::Answered;
                q.answer = Some(a.summary);
            }
        }
        if !all {
            out.retain(|q| q.state == QuestionState::Open);
        }
        Ok(out)
    }

    pub(crate) async fn reply_by_id(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        choice: Option<&str>,
        text: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let target = self.db.question_target((!project.is_empty()).then_some(project), id).map_err(internal)?;
        let Some(target) = target else {
            // 숫자로만 된 옛 무작위 확인창 ID도 기존 경로로 받는다.
            return self
                .answer(id, choice.or(text.as_deref()).unwrap_or_default(), actor)
                .await
                .map(FrontResponse::Question);
        };
        if target.kind == "harness" {
            return self.answer(&target.subject, choice.or(text.as_deref()).unwrap_or_default(), actor).await.map(
                |mut q| {
                    q.short_id = Some(id.into());
                    FrontResponse::Question(q)
                },
            );
        }
        if target.draft.is_some() {
            return Err(err(ErrorCode::BadRequest, "이미 답한 질문이다"));
        }
        let project = &target.project;
        let (items, hash) = if target.kind == "thread" {
            let t = self.thread_show(project, &target.subject)?;
            let q = t
                .questions
                .filter(|q| q.answer.is_none() && q.report_hash == target.revision)
                .ok_or_else(|| err(ErrorCode::BadRequest, "이미 닫혔거나 바뀐 질문이다"))?;
            (q.items, q.report_hash)
        } else {
            let r = self
                .user_requests(project)?
                .into_iter()
                .find(|r| r.id == target.subject)
                .ok_or_else(|| err(ErrorCode::BadRequest, "이미 닫힌 질문이다"))?;
            match r.questions {
                Some(q) if q.report_hash == target.revision => (q.items, q.report_hash),
                Some(_) => return Err(err(ErrorCode::BadRequest, "질문이 바뀌었다")),
                None => (
                    vec![ThreadQuestion {
                        id: Some(id.into()),
                        text: r.title,
                        body: Some(r.body),
                        choices: Vec::new(),
                    }],
                    target.revision.clone(),
                ),
            }
        };
        let item = items.get(target.position).ok_or_else(|| err(ErrorCode::NotFound, "질문이 없다"))?;
        let choice = choice.map(str::trim).filter(|s| !s.is_empty());
        let pick = choice
            .filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic())
            .map(|s| u32::from(s.as_bytes()[0].to_ascii_uppercase() - b'A'));
        if pick.is_some_and(|p| p as usize >= item.choices.len()) {
            return Err(err(ErrorCode::BadRequest, "없는 선택지다"));
        }
        let text = text.or_else(|| if pick.is_none() { choice.map(str::to_string) } else { None });
        let text = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        if pick.is_none() && text.is_none() {
            return Err(err(ErrorCode::BadRequest, "선택지 글자나 직접 답이 필요하다"));
        }
        let answer = QuestionAnswerView {
            picks: vec![pick],
            text,
            summary: choice.unwrap_or("직접").to_string(),
            by: self.answered_by(actor),
            at_ms: aaa_common::util::now_ms(),
        };
        if !self.db.question_claim_draft(&target, &answer).map_err(internal)? {
            return Err(err(ErrorCode::BadRequest, "이미 답한 질문이다"));
        }
        let complete = items.iter().all(|q| {
            q.id.as_ref().is_some_and(|id| {
                self.db.question_target(Some(project), id).ok().flatten().is_some_and(|t| t.draft.is_some())
            })
        });
        if !complete {
            self.asks_bump();
            if target.kind == "thread" {
                let rec = self.load(project, &target.subject)?;
                if let Some(pane) = rec.pane {
                    self.thread_changed(&pane);
                }
            }
            self.inbox_add_body(
                project,
                "thread-state",
                "coordinator",
                "question answered",
                &format!("{id} 질문에 일부 답함({}): {}", answer.by, answer.summary),
                answer.text.as_deref().unwrap_or_default(),
            );
            return Ok(FrontResponse::Done);
        }
        let draft = self.question_draft(project, &items).ok_or_else(|| err(ErrorCode::Internal, "답 기록이 없다"))?;
        let result = if target.kind == "thread" {
            self.thread_answer(project, &target.subject, &hash, draft.picks, draft.text, actor)
                .await
                .map(FrontResponse::Thread)
        } else {
            self.ask_answer(project, &target.subject, &hash, draft.picks, draft.text, actor)
        };
        if result.is_err() {
            self.db.question_set_draft(&target, target.draft.as_ref()).map_err(internal)?;
        }
        result
    }
}

fn catalog_item(
    project: &str,
    kind: &str,
    thread: &str,
    item: &ThreadQuestion,
    answer: Option<&QuestionAnswerView>,
    n: usize,
    at: u64,
) -> QuestionView {
    let pick = answer.and_then(|a| a.picks.get(n)).copied().flatten();
    QuestionView {
        short_id: item.id.clone(),
        id: item.id.clone().unwrap_or_default(),
        pane: String::new(),
        machine: "host".into(),
        project: Some(project.into()),
        thread: (!thread.is_empty()).then(|| thread.into()),
        question_kind: kind.into(),
        text: item.text.clone(),
        options: item
            .choices
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{} {}", char::from(b'A' + i as u8), c.label))
            .collect(),
        state: if pick.is_some() || answer.is_some_and(|a| a.summary != "일부 답함") {
            QuestionState::Answered
        } else {
            QuestionState::Open
        },
        answer: pick.map(|p| char::from(b'A' + p as u8).to_string()),
        created_at_ms: at,
        menu: None,
    }
}

#[cfg(test)]
mod tests;
