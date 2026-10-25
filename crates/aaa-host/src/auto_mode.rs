//! host가 소유하는 coordinator auto mode와 자동 결정 원장. 웹은 사본을 그릴 뿐이다.
use crate::{Host, err, orch::internal};
use aaa_common::util::now_ms;
use aaa_proto::{AutoDecision, AutoDecisionNote, AutoMode, ErrorBody, ErrorCode, FrontResponse, ThreadQuestion};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Default, Serialize, Deserialize)]
struct Document {
    mode: AutoMode,
    decisions: Vec<AutoDecision>,
}
fn key(project: &str) -> String {
    format!("auto-mode:{project}")
}
fn decode(raw: Option<String>) -> anyhow::Result<Document> {
    raw.map(|s| serde_json::from_str(&s)).transpose().map(|v| v.unwrap_or_default()).map_err(Into::into)
}
fn nonempty(s: &str, field: &str) -> Result<(), ErrorBody> {
    if s.trim().is_empty() || s.len() > 16384 {
        return Err(err(ErrorCode::BadRequest, format!("{field}: 비어 있거나 16KiB를 넘는다")));
    }
    Ok(())
}
impl Host {
    fn auto_document(&self, project: &str) -> Result<Document, ErrorBody> {
        decode(self.db.meta(&key(project)).map_err(internal)?).map_err(internal)
    }
    pub(crate) fn auto_mode(&self, project: &str) -> Result<AutoMode, ErrorBody> {
        Ok(self.auto_document(project)?.mode)
    }
    pub(crate) fn auto_decisions(&self, project: &str) -> Result<Vec<AutoDecision>, ErrorBody> {
        let mut records = self.auto_document(project)?.decisions;
        records.reverse();
        Ok(records)
    }
    /// 장시간 켜 둬도 일반 질문·CLI 응답의 1MiB frame을 자동 기록이 채우지 않는다.
    pub(crate) fn auto_decision_page(
        &self,
        project: &str,
        before: Option<&str>,
        id: Option<&str>,
    ) -> Result<(Vec<AutoDecision>, Option<String>), ErrorBody> {
        let records = self.auto_decisions(project)?;
        if let Some(id) = id {
            return Ok((records.into_iter().filter(|d| d.id == id).collect(), None));
        }
        let start = match before {
            Some(id) => records
                .iter()
                .position(|d| d.id == id)
                .map(|n| n + 1)
                .ok_or_else(|| err(ErrorCode::BadRequest, "자동 결정 목록의 기준 ID가 없다"))?,
            None => 0,
        };
        let mut page = Vec::new();
        let mut bytes = 0;
        let mut next = None;
        for d in records.into_iter().skip(start) {
            let size = serde_json::to_vec(&d).map_err(internal)?.len();
            if !page.is_empty() && (page.len() >= 50 || bytes + size > 128 * 1024) {
                next = page.last().map(|r: &AutoDecision| r.id.clone());
                break;
            }
            bytes += size;
            page.push(d);
        }
        Ok((page, next))
    }
    pub(crate) fn auto_mode_set(&self, project: &str, on: bool, actor: &str) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        let mut changed = false;
        let by = self.answered_by(actor);
        self.db
            .update_meta(&key(project), |raw| {
                let mut d = decode(raw)?;
                if d.mode.on != on {
                    changed = true;
                    d.mode.on = on;
                    d.mode.by = by;
                    if on {
                        d.mode.since_ms = now_ms();
                        d.mode.ended_ms = 0;
                        d.mode.run += 1;
                        d.mode.decisions = 0;
                    } else {
                        d.mode.ended_ms = now_ms();
                    }
                }
                Ok(serde_json::to_string(&d)?)
            })
            .map_err(internal)?;
        if changed {
            let mode = if on { "on" } else { "off" };
            self.db.audit(actor, "auto_mode", &format!("{project} {mode}"));
            self.inbox_add_body(
                project,
                "auto-mode",
                "coordinator",
                &format!("auto-mode {mode}"),
                &format!("auto_mode: {mode} · 사용자 변경"),
                &format!(
                    "coordinator만 적용. harness 권한은 그대로다. 매 턴 aaa context를 다시 읽는다. 예외: {}",
                    aaa_proto::AUTO_MODE_EXCEPTIONS
                ),
            );
            self.asks_bump();
        }
        self.asks_view(project)
    }
    fn auto_authorize(&self, project: &str, actor: &str) -> Result<(), ErrorBody> {
        if self.coordinator_project_of_actor(actor).as_deref() != Some(project) {
            return Err(err(ErrorCode::Forbidden, "해당 프로젝트의 검증된 coordinator pane만 자동으로 답한다"));
        }
        if !self.auto_mode(project)?.on {
            return Err(err(ErrorCode::BadRequest, "auto mode가 꺼져 있다"));
        }
        Ok(())
    }
    /// 저장·켜짐 확인·중복 예약을 같은 SQLite transaction에서 한다. 끄기 뒤 새 답은 들어가지 않는다.
    pub(crate) fn auto_reserve(&self, project: &str, mut decision: AutoDecision) -> Result<(), ErrorBody> {
        self.db
            .update_meta(&key(project), |raw| {
                let mut d = decode(raw)?;
                anyhow::ensure!(d.mode.on, "auto mode가 꺼져 있다");
                anyhow::ensure!(
                    !d.decisions.iter().any(|r| r.id == decision.id),
                    "이미 자동 답을 시도했다. 전달 기록을 확인하고 자동 재전송하지 않는다"
                );
                decision.run = d.mode.run;
                d.decisions.push(decision);
                Ok(serde_json::to_string(&d)?)
            })
            .map_err(internal)?;
        self.asks_bump();
        Ok(())
    }
    pub(crate) fn auto_finish(&self, project: &str, id: &str, failure: Option<String>) -> Result<(), ErrorBody> {
        self.db
            .update_meta(&key(project), |raw| {
                let mut d = decode(raw)?;
                let r = d
                    .decisions
                    .iter_mut()
                    .find(|r| r.id == id)
                    .ok_or_else(|| anyhow::anyhow!("자동 결정 원장이 없다: {id}"))?;
                if r.outcome == "pending" {
                    r.outcome = if failure.is_some() { "failed" } else { "answered" }.into();
                    r.error = failure;
                    if r.run == d.mode.run && r.outcome == "answered" {
                        d.mode.decisions += 1;
                    }
                }
                Ok(serde_json::to_string(&d)?)
            })
            .map_err(internal)?;
        self.asks_bump();
        Ok(())
    }
    pub(crate) async fn auto_answer(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        choice: Option<String>,
        text: Option<String>,
        reason: String,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.auto_authorize(project, actor)?;
        nonempty(&reason, "까닭")?;
        let t = self
            .db
            .question_target(Some(project), id)
            .map_err(internal)?
            .ok_or_else(|| err(ErrorCode::NotFound, "자동 답은 현재 질문 ID(q-NNNN)에만 보낸다"))?;
        let id = t.id.as_str();
        if t.kind == "harness" {
            return Err(err(ErrorCode::Forbidden, "확인창·권한·신뢰 질문은 사용자만 답한다"));
        }
        if t.draft.is_some() {
            return Err(err(ErrorCode::BadRequest, "이미 답한 질문이다"));
        }
        let (items, _files_base) = if t.kind == "thread" {
            let qs = self
                .thread_show(project, &t.subject)?
                .questions
                .filter(|q| q.answer.is_none() && q.report_hash == t.revision)
                .ok_or_else(|| err(ErrorCode::BadRequest, "질문이 바뀌었거나 닫혔다"))?;
            (qs.items, qs.files_base)
        } else {
            let request = self
                .user_requests(project)?
                .into_iter()
                .find(|r| r.id == t.subject)
                .ok_or_else(|| err(ErrorCode::BadRequest, "이미 닫힌 질문이다"))?;
            if let Some(qs) = request.questions {
                if qs.report_hash != t.revision {
                    return Err(err(ErrorCode::BadRequest, "질문이 바뀌었다"));
                }
                (qs.items, qs.files_base)
            } else {
                (
                    vec![ThreadQuestion {
                        id: Some(id.into()),
                        text: request.title,
                        body: Some(request.body),
                        after: None,
                        choices: vec![],
                    }],
                    None,
                )
            }
        };
        let question = items.get(t.position).cloned().ok_or_else(|| err(ErrorCode::NotFound, "질문 위치가 없다"))?;
        if aaa_proto::auto_question_exception(&question) {
            return Err(err(ErrorCode::Forbidden, "사용자 전용 질문이다. 예외 카드에서 사용자가 답해야 한다"));
        }
        let value = choice.as_deref().or(text.as_deref()).unwrap_or_default().trim();
        nonempty(value, "답")?;
        let pick = (value.len() == 1 && value.as_bytes()[0].is_ascii_alphabetic())
            .then(|| u32::from(value.as_bytes()[0].to_ascii_uppercase() - b'A'));
        if pick.is_some_and(|n| n as usize >= question.choices.len()) {
            return Err(err(ErrorCode::BadRequest, "없는 선택지다"));
        }
        let decision_id = format!("auto-{id}");
        if self.auto_decisions(project)?.iter().any(|d| d.id == decision_id) {
            return Err(err(ErrorCode::BadRequest, "이미 자동 답을 시도했다. 기록을 확인한다"));
        }
        let dir = self.project_dir(project)?;
        let snapshot = dir.join("asks").join(&decision_id);
        if t.kind == "thread" {
            self.snapshot_question(&dir, &self.load(project, &t.subject)?, &snapshot).map_err(internal)?;
        } else {
            let post = self
                .db
                .post_of_item(project, &t.subject)
                .map_err(internal)?
                .ok_or_else(|| err(ErrorCode::NotFound, "질문의 post가 없다"))?;
            self.snapshot_request(&dir, &snapshot, &post.input.body).map_err(internal)?;
        }
        let files_base = Some(format!("/api/v1/projects/{project}/asks/{decision_id}/"));
        self.auto_reserve(
            project,
            AutoDecision {
                id: decision_id.clone(),
                run: 0,
                subject: if t.kind == "thread" { t.subject } else { "coordinator".into() },
                question,
                choice: value.into(),
                pick,
                reason: reason.trim().into(),
                action: "질문 답을 기록함(묶음 질문은 모두 답한 뒤 전달)".into(),
                reversible: None,
                decided_by: "coordinator(auto)".into(),
                at_ms: now_ms(),
                outcome: "pending".into(),
                error: None,
                reviewed: false,
                reconsideration: None,
                files_base,
            },
        )?;
        let result = self.reply_by_id_inner(project, id, choice.as_deref(), text, actor, true).await;
        self.auto_finish(project, &decision_id, result.as_ref().err().map(|e| e.message.clone()))?;
        result?;
        self.asks_view(project)
    }
    pub(crate) fn auto_decision_review(
        &self,
        project: &str,
        id: &str,
        answer: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        if let Some(s) = &answer {
            nonempty(s, "새 답")?;
        }
        let mut changed = false;
        self.db
            .update_meta(&key(project), |raw| {
                let mut d = decode(raw)?;
                let r = d
                    .decisions
                    .iter_mut()
                    .find(|r| r.id == id)
                    .ok_or_else(|| anyhow::anyhow!("자동 결정이 없다: {id}"))?;
                changed = if answer.is_some() { r.reconsideration != answer } else { !r.reviewed };
                r.reviewed = true;
                if answer.is_some() {
                    r.reconsideration = answer.clone();
                }
                Ok(serde_json::to_string(&d)?)
            })
            .map_err(internal)?;
        if changed {
            if let Some(answer) = answer {
                self.inbox_add_body(project, "auto-decision", "coordinator", "auto decision reconsider", &format!("{id} 사용자가 다른 답으로 다시 정함"),
                    &format!("새 답: {answer}\n이미 실행한 작업은 host가 되돌리지 않았다. 기존 결정과 실행 상태를 확인하고 되돌릴 수 있는지 판단한다."));
            }
            self.db.audit(actor, "auto_decision_review", &format!("{project} {id}"));
            self.asks_bump();
        }
        let (decisions, next) = self.auto_decision_page(project, None, Some(id))?;
        Ok(FrontResponse::AutoDecisions { decisions, next })
    }
    pub(crate) fn auto_exception_count(&self, project: &str) -> Result<u32, ErrorBody> {
        let mut ids = std::collections::BTreeSet::new();
        for q in self.questions(false).iter().filter(|q| q.project.as_deref() == Some(project)) {
            ids.insert(format!("harness:{}", q.id));
        }
        let mut add = |q: &ThreadQuestion| -> Result<(), ErrorBody> {
            if aaa_proto::auto_question_exception(q)
                && let Some(id) = q.id.as_ref()
                && self.db.question_target(Some(project), id).map_err(internal)?.is_some_and(|t| t.draft.is_none())
            {
                ids.insert(id.clone());
            }
            Ok(())
        };
        for t in self.threads_of(Some(project), None)?.iter().filter(|t| t.status != aaa_proto::ThreadStatus::Resolved)
        {
            if let Some(q) = t.questions.as_ref().filter(|q| q.answer.is_none()) {
                for q in &q.items {
                    add(q)?;
                }
            }
        }
        for r in self.user_requests(project)? {
            if let Some(q) = r.questions {
                for q in &q.items {
                    add(q)?;
                }
            } else {
                add(&ThreadQuestion {
                    id: r.question_id,
                    text: r.title,
                    body: Some(r.body),
                    after: None,
                    choices: vec![],
                })?;
            }
        }
        Ok(ids.len() as u32)
    }
    pub(crate) fn auto_post_decision(
        &self,
        project: &str,
        input: &aaa_proto::PostInput,
        id: &str,
        actor: &str,
    ) -> Result<AutoDecision, ErrorBody> {
        self.auto_authorize(project, actor)?;
        let note: AutoDecisionNote = serde_json::from_str(&input.body)
            .map_err(|e| err(ErrorCode::BadRequest, format!("auto-decision JSON 본문: {e}")))?;
        for (name, value) in
            [("질문", &note.question), ("답", &note.answer), ("까닭", &note.reason), ("진행함", &note.action)]
        {
            nonempty(value, name)?;
        }
        let question = ThreadQuestion { id: None, text: note.question, body: None, after: None, choices: vec![] };
        if aaa_proto::auto_question_exception(&question) {
            return Err(err(ErrorCode::Forbidden, "사용자 전용 질문은 auto-decision으로 대신할 수 없다"));
        }
        Ok(AutoDecision {
            id: id.into(),
            run: 0,
            subject: "coordinator".into(),
            question,
            choice: note.answer,
            pick: None,
            reason: note.reason,
            action: note.action,
            reversible: Some(note.reversible),
            decided_by: "coordinator(auto)".into(),
            at_ms: now_ms(),
            outcome: "pending".into(),
            error: None,
            reviewed: false,
            reconsideration: None,
            files_base: None,
        })
    }
}

#[cfg(test)]
mod tests;
