//! host가 소유하는 coordinator auto mode와 자동 결정 원장. 웹은 사본을 그릴 뿐이다.
use crate::{Host, asks::ASK_KEEP_DAYS, err, orch::internal};
use aaa_common::util::now_ms;
use aaa_proto::{AutoDecision, AutoDecisionNote, AutoMode, ErrorBody, ErrorCode, FrontResponse, ThreadQuestion};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Default, Serialize, Deserialize)]
struct Document {
    mode: AutoMode,
    decisions: Vec<AutoDecision>,
    /// 지운 기록 중 답을 전달하지 못했거나 전달 여부를 모르는 ID. 지운 뒤에도 같은 질문에 자동 재전송하지 않는다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    forgotten: Vec<String>,
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
                    !d.decisions.iter().any(|r| r.id == decision.id) && !d.forgotten.contains(&decision.id),
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
                        source_number: None,
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
            .then(|| question.choice_index(value).ok_or_else(|| err(ErrorCode::BadRequest, "없는 선택지다")))
            .transpose()?;
        if pick.is_some_and(|n| n as usize >= question.choices.len()) {
            return Err(err(ErrorCode::BadRequest, "없는 선택지다"));
        }
        let decision_id = format!("auto-{id}");
        let doc = self.auto_document(project)?;
        if doc.decisions.iter().any(|d| d.id == decision_id) || doc.forgotten.contains(&decision_id) {
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
    /// 자동 결정 기록을 지운다(모든 기기). 이미 실행한 일은 되돌리지 않는다.
    pub(crate) fn auto_decision_forget(
        &self,
        project: &str,
        ids: &[String],
        all: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        if ids.is_empty() && !all {
            return Err(err(ErrorCode::BadRequest, "지울 자동 결정 ID나 all이 필요하다"));
        }
        let gone = self.auto_decision_remove(project, |r| all || ids.contains(&r.id))?;
        if gone == 0 {
            return self.asks_view(project);
        }
        self.db.audit(actor, "auto_decision_forget", &format!("{project} {gone}"));
        self.asks_bump();
        self.asks_view(project)
    }
    /// 결정한 지 [`ASK_KEEP_DAYS`]일이 지난 자동 결정을 지우고(ticker, 「답한 것」과 같은 기준), 질문이 닫혀 더는 자동 답을 받을 수
    /// 없는 `forgotten` ID를 뺀다.
    pub(crate) fn auto_prune_pass(&self, now_ms: u64) {
        let cutoff = now_ms.saturating_sub(u64::from(ASK_KEEP_DAYS) * 86_400_000);
        let Ok(projects) = self
            .db
            .meta_suffixes("auto-mode:")
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "다음 ticker에서 다시 정리한다"))
        else {
            return;
        };
        let mut changed = false;
        for project in projects {
            // 지울 것이 없으면 원장을 다시 쓰지 않는다(ticker마다 도는 쓰기를 피한다).
            let pruned = self.auto_document(&project).and_then(|d| {
                let gone = if d.decisions.iter().any(|r| r.at_ms < cutoff) {
                    self.auto_decision_remove(&project, |r| r.at_ms < cutoff)?
                } else {
                    0
                };
                let left = self.auto_forgotten_prune(&project)?;
                Ok(gone > 0 || left)
            });
            match pruned {
                Ok(c) => changed |= c,
                Err(e) => tracing::warn!(project=%project, "자동 결정 보존 기간 정리 실패: {}", e.message),
            }
        }
        if changed {
            self.asks_bump();
        }
    }
    /// 고른 결정을 원장에서 빼고 질문 사본과, 자동 답만으로 된 「답한 것」 기록도 지워 다른 목록에 다시 나타나지 않게 한다. 답을
    /// 전달하지 못했거나 전달 여부를 모르는 ID는 `forgotten`에 남겨 같은 질문에 자동 재전송하지 않는다(§8.479). 지운 수를 준다.
    fn auto_decision_remove(&self, project: &str, pick: impl Fn(&AutoDecision) -> bool) -> Result<usize, ErrorBody> {
        let mut gone = Vec::new();
        let mut kept = std::collections::BTreeSet::new();
        self.db
            .update_meta(&key(project), |raw| {
                let mut d = decode(raw)?;
                let (out, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut d.decisions).into_iter().partition(&pick);
                for r in out.iter().filter(|r| r.outcome != "answered") {
                    if !d.forgotten.contains(&r.id) {
                        d.forgotten.push(r.id.clone());
                    }
                }
                kept = keep.iter().filter(|r| r.outcome == "answered").filter_map(|r| r.question.id.clone()).collect();
                d.decisions = keep;
                gone = out;
                Ok(serde_json::to_string(&d)?)
            })
            .map_err(internal)?;
        if gone.is_empty() {
            return Ok(0);
        }
        // 지운 프로젝트에 남은 원장도 정리한다. 폴더가 없으면 지울 사본도 없다.
        let Ok(dir) = self.project_dir(project) else {
            return Ok(gone.len());
        };
        for r in &gone {
            aaa_common::diagnostic::ignored(
                std::fs::remove_dir_all(dir.join("asks").join(&r.id)),
                "정리 실패가 본 작업의 결과를 덮지 않게 한다",
            );
        }
        // 웹은 질문이 모두 자동 답인 「답한 것」을 자동 결정 줄로만 보인다. 그 결정을 지우면 기록도 지운다.
        let removed: std::collections::BTreeSet<String> =
            gone.iter().filter(|r| r.outcome == "answered").filter_map(|r| r.question.id.clone()).collect();
        let records: Vec<String> = self
            .db
            .ask_list(project)
            .map_err(internal)?
            .into_iter()
            .filter(|a| {
                let qs: Vec<&str> = a.items.iter().filter_map(|q| q.id.as_deref()).collect();
                !a.items.is_empty()
                    && qs.len() == a.items.len()
                    && qs.iter().all(|q| removed.contains(*q) || kept.contains(*q))
                    && qs.iter().any(|q| removed.contains(*q))
            })
            .map(|a| a.id)
            .collect();
        if !records.is_empty() {
            for id in self.db.ask_delete(project, Some(&records)).map_err(internal)? {
                aaa_common::diagnostic::ignored(
                    std::fs::remove_dir_all(dir.join("asks").join(id)),
                    "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                );
            }
        }
        Ok(gone.len())
    }
    /// `forgotten`은 그 질문이 아직 자동 답을 받을 수 있을 때만 쓸모가 있다. 질문 ID는 재사용하지 않으므로, 답했거나 판이 바뀌었거나
    /// 닫힌 질문의 ID와 post 번호로 만든 결정(`auto-decision` post, 재전송 경로 없음)의 ID는 뺀다. 뺐으면 true.
    fn auto_forgotten_prune(&self, project: &str) -> Result<bool, ErrorBody> {
        let forgotten = self.auto_document(project)?.forgotten;
        if forgotten.is_empty() {
            return Ok(false);
        }
        let open = match self.project_dir(project) {
            Ok(_) => self.auto_open_questions(project)?,
            Err(_) => Default::default(),
        };
        let stale = |id: &String| !id.strip_prefix("auto-").is_some_and(|q| open.contains(q));
        if !forgotten.iter().any(stale) {
            return Ok(false);
        }
        let mut changed = false;
        self.db
            .update_meta(&key(project), |raw| {
                let mut d = decode(raw)?;
                let before = d.forgotten.len();
                d.forgotten.retain(|id| !stale(id));
                changed = d.forgotten.len() != before;
                Ok(serde_json::to_string(&d)?)
            })
            .map_err(internal)?;
        Ok(changed)
    }
    /// 지금 답을 기다리는 thread 질문·사용자 요청의 질문 ID(`auto_answer`가 받는 범위). 끝난 thread도 다시 열릴 수 있어 넣는다.
    fn auto_open_questions(&self, project: &str) -> Result<std::collections::BTreeSet<String>, ErrorBody> {
        let mut ids = std::collections::BTreeSet::new();
        for t in self.threads_of(Some(project), None)? {
            if let Some(q) = t.questions.as_ref().filter(|q| q.answer.is_none()) {
                // ID 발급이 실패하면 thread_view는 ID 없는 원문을 준다. 열린 질문을 놓쳐 재전송을 막는 기록을 지우지 않게 멈춘다.
                if q.items.iter().any(|q| q.id.is_none()) {
                    return Err(err(
                        ErrorCode::Internal,
                        format!("{} 질문 ID를 읽지 못해 forgotten을 그대로 둔다", t.id),
                    ));
                }
                ids.extend(q.items.iter().filter_map(|q| q.id.clone()));
            }
        }
        for r in self.user_requests(project)? {
            match r.questions {
                Some(q) => ids.extend(q.items.into_iter().filter_map(|q| q.id)),
                None => ids.extend(r.question_id),
            }
        }
        Ok(ids)
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
                    source_number: None,
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
        let question = ThreadQuestion {
            source_number: None,
            id: None,
            text: note.question,
            body: None,
            after: None,
            choices: vec![],
        };
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
