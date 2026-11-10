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
    /// 사본 교체 전에 읽는다. 자료가 없거나 읽지 못하면 같은 자료로 간주하지 않는다.
    pub(crate) fn pending_question_materials(
        &self,
        rec: &crate::orch::ThreadRec,
    ) -> Result<Vec<Option<Vec<String>>>, ErrorBody> {
        let dir = self.project_dir(&rec.project)?;
        Ok(rec
            .questions
            .iter()
            .map(|q| {
                let mut hashes = Vec::new();
                for text in q.body.iter().chain(q.after.iter()).chain(q.choices.iter().filter_map(|c| c.body.as_ref()))
                {
                    for rel in crate::asks::library_refs(text) {
                        let (path, _) = crate::library::local_file(&dir, &rec.id, &rel).ok()?;
                        hashes.push(aaa_common::util::sha256_file(&path).ok()?);
                    }
                }
                Some(hashes)
            })
            .collect())
    }

    /// 미완료 묶음에서 같은 질문만 한 번씩 대응한다. 순서·원문 번호가 바뀌어도 답은 질문을 따른다.
    pub(crate) fn carry_pending_questions(
        &self,
        previous: &crate::orch::ThreadRec,
        materials: &[Option<Vec<String>>],
        revision: &str,
        items: &mut [ThreadQuestion],
    ) -> Result<(), ErrorBody> {
        let current = crate::orch::ThreadRec { questions: items.to_vec(), ..previous.clone() };
        let current_materials = self.pending_question_materials(&current)?;
        let mut retained = Vec::new();
        for (position, item) in items.iter_mut().enumerate() {
            if item.id.is_some() {
                continue; // 전달 완료 답의 재사용은 repeated_answer가 맡는다.
            }
            if let Some((_, old)) = previous.questions.iter().enumerate().find(|(n, old)| {
                old.id.as_ref().is_some_and(|id| !retained.iter().any(|(kept, _)| kept == id))
                    && crate::question_repeat::same_question(item, old)
                    && materials.get(*n).is_some_and(|m| m.is_some() && current_materials[position] == *m)
            }) {
                let id = old.id.clone().unwrap();
                item.id = Some(id.clone());
                retained.push((id, position));
            }
        }
        self.db
            .refresh_thread_question_ids(
                &previous.project,
                &previous.id,
                previous.report_hash.as_deref().unwrap_or_default(),
                revision,
                &retained,
            )
            .map_err(internal)
    }

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
                if q.id.is_none() {
                    q.id = Some(self.db.question_id(project, kind, subject, revision, position).map_err(internal)?);
                }
                Ok(q)
            })
            .collect()
    }

    pub(crate) fn question_draft(&self, project: &str, items: &[ThreadQuestion]) -> Option<QuestionAnswerView> {
        let answers: Vec<_> = items
            .iter()
            .map(|q| {
                q.id.as_ref()
                    .and_then(|id| {
                        self.db
                            .question_target(Some(project), id)
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()
                            .flatten()
                    })
                    .and_then(|t| t.draft)
            })
            .collect();
        if answers.iter().all(Option::is_none) {
            return None;
        }
        let picks = answers.iter().map(|a| a.as_ref().and_then(|a| a.picks.first().copied().flatten())).collect();
        let comments = answers.iter().map(|a| a.as_ref().and_then(|a| a.text.clone())).collect();
        let last = answers.iter().flatten().max_by_key(|a| a.at_ms)?;
        Some(QuestionAnswerView {
            picks,
            text: None,
            comments,
            summary: "일부 답함".into(),
            by: last.by.clone(),
            at_ms: last.at_ms,
        })
    }

    /// 확인창 목록도 같은 짧은 별칭으로 읽는다. 내부 확인창 ID·menu는 바꾸지 않는다.
    pub(crate) fn question_catalog(&self, all: bool) -> Result<Vec<QuestionView>, ErrorBody> {
        let mut out = self.questions(all);
        for q in &mut out {
            let project = q
                .project
                .clone()
                .or_else(|| {
                    self.sole_project()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                        .flatten()
                })
                .unwrap_or_default();
            q.short_id = Some(self.db.question_id(&project, "harness", &q.id, "", 0).map_err(internal)?);
        }
        let threads = if all { self.threads_of(None, None)? } else { self.unresolved_threads_of(None, None)? };
        for t in threads {
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
                        &ThreadQuestion {
                            source_number: None,
                            after: None,
                            id: Some(id),
                            text: r.title,
                            body: None,
                            choices: Vec::new(),
                        },
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
                                source_number: None,
                                after: None,
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
        self.reply_by_id_inner(project, id, choice, text, actor, false).await
    }

    pub(crate) async fn reply_by_id_inner(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        choice: Option<&str>,
        text: Option<String>,
        actor: &str,
        auto: bool,
    ) -> Result<FrontResponse, ErrorBody> {
        let target = self.db.question_target((!project.is_empty()).then_some(project), id).map_err(internal)?;
        let Some(target) = target else {
            // 숫자로만 된 옛 무작위 확인창 ID도 기존 경로로 받는다.
            return self
                .answer(id, choice.or(text.as_deref()).unwrap_or_default(), actor)
                .await
                .map(FrontResponse::Question);
        };
        let id = target.id.as_str();
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
                        source_number: None,
                        after: None,
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
        if item.id.as_deref() != Some(id) {
            return Err(err(ErrorCode::BadRequest, "이미 닫혔거나 바뀐 질문이다"));
        }
        let choice = choice.map(str::trim).filter(|s| !s.is_empty());
        let pick = choice
            .filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_alphabetic())
            .map(|s| item.choice_index(s).ok_or_else(|| err(ErrorCode::BadRequest, "없는 선택지다")))
            .transpose()?;
        if pick.is_some_and(|p| p as usize >= item.choices.len()) {
            return Err(err(ErrorCode::BadRequest, "없는 선택지다"));
        }
        let text = text.or_else(|| if pick.is_none() { choice.map(str::to_string) } else { None });
        let text = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
        if pick.is_none() && text.is_none() {
            return Err(err(ErrorCode::BadRequest, "선택지 글자나 직접 답이 필요하다"));
        }
        let answer = QuestionAnswerView {
            comments: Vec::new(),
            picks: vec![pick],
            text,
            summary: choice.unwrap_or("직접").to_string(),
            by: if auto { "coordinator(auto)".into() } else { self.answered_by(actor) },
            at_ms: aaa_common::util::now_ms(),
        };
        if !self.db.question_claim_draft(&target, &answer).map_err(internal)? {
            return Err(err(ErrorCode::BadRequest, "이미 답한 질문이다"));
        }
        let complete = items.iter().all(|q| {
            q.id.as_ref().is_some_and(|id| {
                self.db
                    .question_target(Some(project), id)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .flatten()
                    .is_some_and(|t| t.draft.is_some())
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
            return Ok(if auto {
                FrontResponse::QuestionAnswered { id: id.into(), complete: false }
            } else {
                FrontResponse::Done
            });
        }
        let draft = self.question_draft(project, &items).ok_or_else(|| err(ErrorCode::Internal, "답 기록이 없다"))?;
        let result = if target.kind == "thread" {
            self.thread_answer(project, &target.subject, &hash, draft.picks, draft.text, draft.comments, actor)
                .await
                .map(Box::new)
                .map(FrontResponse::Thread)
        } else if auto {
            self.ask_answer_apply(project, &target.subject, &hash, draft.picks, draft.text, draft.comments, actor)
                .map(|()| FrontResponse::Done)
        } else {
            self.ask_answer(project, &target.subject, &hash, draft.picks, draft.text, draft.comments, actor)
        };
        if result
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
        {
            self.db.question_set_draft(&target, target.draft.as_ref()).map_err(internal)?;
        }
        result.map(
            |response| {
                if auto { FrontResponse::QuestionAnswered { id: id.into(), complete: true } } else { response }
            },
        )
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
    let comment = answer.and_then(|a| a.comments.get(n)).and_then(|c| c.as_deref());
    let letter = pick.and_then(|p| item.choices.get(p as usize).map(|c| c.letter_at(p as usize)));
    QuestionView {
        short_id: item.id.clone(),
        id: item.id.clone().unwrap_or_default(),
        pane: String::new(),
        machine: "host".into(),
        project: Some(project.into()),
        thread: (!thread.is_empty()).then(|| thread.into()),
        question_kind: kind.into(),
        text: item.text.clone(),
        options: item.choices.iter().enumerate().map(|(i, c)| format!("{} {}", c.letter_at(i), c.label)).collect(),
        state: if pick.is_some() || comment.is_some() || answer.is_some_and(|a| a.summary != "일부 답함") {
            QuestionState::Answered
        } else {
            QuestionState::Open
        },
        answer: match (letter, comment) {
            (Some(letter), Some(text)) => Some(format!("{letter} + 의견: {text}")),
            (None, Some(text)) => Some(format!("직접: {text}")),
            (letter, None) => letter,
        },
        created_at_ms: at,
        menu: None,
    }
}

#[cfg(test)]
mod tests;
