//! 답한 질문을 보고의 다른 문장 때문에 새 질문으로 만들지 않는다.
use crate::{
    Host,
    orch::{ThreadRec, internal},
};
use aaa_proto::{AskKind, AskRecord, ErrorBody, QuestionAnswerView, ThreadQuestion};

pub(crate) struct RepeatedQuestions {
    pub items: Vec<ThreadQuestion>,
    pub answer: Option<QuestionAnswerView>,
    pub ids: Vec<String>,
}

fn same_question(a: &ThreadQuestion, b: &ThreadQuestion) -> bool {
    let normalize = |q: &ThreadQuestion| {
        let mut q = q.clone();
        q.id = None;
        q.source_number = None;
        for (i, c) in q.choices.iter_mut().enumerate() {
            c.letter = Some(c.letter_at(i));
        }
        q
    };
    normalize(a) == normalize(b)
}

impl Host {
    fn same_question_files(&self, rec: &ThreadRec, item: &ThreadQuestion, old: &AskRecord) -> Result<bool, ErrorBody> {
        let dir = self.project_dir(&rec.project)?;
        for text in item.body.iter().chain(item.after.iter()).chain(item.choices.iter().filter_map(|c| c.body.as_ref()))
        {
            for rel in crate::asks::library_refs(text) {
                let Ok((current, _)) = crate::library::local_file(&dir, &rec.id, &rel) else { return Ok(false) };
                let previous = dir.join("asks").join(&old.id).join("library").join(rel);
                // 같은 URL의 그림 변경·누락·읽기 실패는 이미 답한 질문으로 흡수하지 않는다.
                match (std::fs::read(current), std::fs::read(previous)) {
                    (Ok(a), Ok(b)) if a == b => {}
                    _ => return Ok(false),
                }
            }
        }
        Ok(true)
    }

    /// 전달을 마쳐 보존한 답과 비교한다. 임시 claim·다른 thread·post는 근거가 아니다.
    pub(crate) fn repeated_answer(
        &self,
        rec: &ThreadRec,
        items: &[ThreadQuestion],
    ) -> Result<Option<RepeatedQuestions>, ErrorBody> {
        if items.is_empty() {
            return Ok(None);
        }
        let mut history = self.db.ask_list(&rec.project).map_err(internal)?;
        history.retain(|a| a.kind == AskKind::Question && a.subject == rec.id && a.answer.is_some());
        history.sort_by_key(|a| std::cmp::Reverse(a.done_ms));
        let mut matched = Vec::new();
        let mut pending = Vec::new();
        let mut picks = Vec::new();
        let mut comments = Vec::new();
        let mut legacy_texts = Vec::new();
        let mut answer = None;
        let mut ids = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let mut found = None;
            for old in &history {
                if let Some((n, q)) = old
                    .items
                    .iter()
                    .enumerate()
                    .find(|(_, q)| same_question(item, q) && !ids.contains(&q.id.clone().unwrap_or_default()))
                {
                    if !self.same_question_files(rec, item, old)? {
                        continue;
                    }
                    found = Some((old, n, q));
                    break;
                }
            }
            if let Some((old, n, q)) = found {
                let previous = old.answer.as_ref().unwrap();
                answer.get_or_insert_with(|| previous.clone());
                picks.push(previous.picks.get(n).copied().flatten());
                comments.push(previous.comments.get(n).cloned().flatten());
                if let Some(text) = &previous.text
                    && !legacy_texts.contains(text)
                {
                    legacy_texts.push(text.clone());
                }
                let mut matched_question = q.clone();
                matched_question.source_number.get_or_insert(n as u32 + 1);
                matched.push(matched_question);
                if let Some(id) = &q.id {
                    ids.push(id.clone());
                }
            } else {
                let mut q = item.clone();
                q.source_number = Some(index as u32 + 1);
                pending.push(q);
            }
        }
        if matched.is_empty() {
            return Ok(None);
        }
        if pending.is_empty() {
            if let Some(a) = &mut answer {
                a.picks = picks;
                a.text = (!legacy_texts.is_empty()).then(|| legacy_texts.join("\n"));
                a.comments = if comments.iter().any(Option::is_some) { comments } else { Vec::new() };
                if !a.comments.is_empty() {
                    a.summary = crate::answer::compose(&matched, &a.picks, a.text.as_deref(), &a.comments)
                        .map_err(|e| crate::err(aaa_proto::ErrorCode::Internal, e))?
                        .1;
                }
            }
            Ok(Some(RepeatedQuestions { items: matched, answer, ids }))
        } else {
            // 답한 줄은 역사 카드에 남고, 새 질문만 원래 번호로 열린다.
            Ok(Some(RepeatedQuestions { items: pending, answer: None, ids }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{Role, ThreadKind, ThreadStatus};

    /// 같은 제목·URL만으로 새 판단을 닫지 않으며 전달 완료 기록만 재사용한다.
    #[test]
    fn repeated_question_requires_committed_answer_same_thread_and_unchanged_material() {
        let root = std::env::temp_dir().join(format!("aaa-question-repeat-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("p", Default::default(), "test").unwrap();
        let dir = h.project_dir("p").unwrap();
        std::fs::create_dir_all(dir.join("library/t-0001")).unwrap();
        std::fs::write(dir.join("library/t-0001/a.png"), b"original").unwrap();
        let mut rec = ThreadRec::starting("p", "질문", "m", "fake", Role::Worker, ThreadKind::Dir, 0);
        rec.id = "t-0001".into();
        rec.status = ThreadStatus::Open;
        rec.report_hash = Some("old".into());
        let items =
            aaa_proto::report_questions("## Questions\n### 1. 시안\n![시안](library/a.png)\n- A 적용\n- B 보류\n");
        rec.questions = h.identify_questions("p", "thread", &rec.id, "old", &items).unwrap();
        let answer = QuestionAnswerView {
            comments: Vec::new(),
            picks: vec![Some(0)],
            text: None,
            summary: "1A".into(),
            by: "CLI".into(),
            at_ms: 10,
        };
        rec.question_answer = Some(answer.clone());
        assert!(h.repeated_answer(&rec, &items).unwrap().is_none(), "전달 중 claim은 답함이 아니다");
        h.record_answer(&rec, &answer);
        let repeated = h.repeated_answer(&rec, &items).unwrap().unwrap();
        assert_eq!(repeated.items[0].id, rec.questions[0].id);
        assert_eq!(repeated.answer, Some(answer));
        let mut changed = items.clone();
        changed[0].body = Some("새 판단 내용".into());
        assert!(h.repeated_answer(&rec, &changed).unwrap().is_none());
        std::fs::write(dir.join("library/t-0001/a.png"), b"changed").unwrap();
        assert!(h.repeated_answer(&rec, &items).unwrap().is_none(), "같은 URL의 다른 그림은 새 판단이다");
        std::fs::remove_file(dir.join("library/t-0001/a.png")).unwrap();
        assert!(h.repeated_answer(&rec, &items).unwrap().is_none());
        rec.id = "t-0002".into();
        assert!(h.repeated_answer(&rec, &items).unwrap().is_none());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 재보고가 질문을 재배열해도 의견이 다른 질문으로 옮겨 붙으면 안 된다.
    #[test]
    fn repeated_question_reorders_comments_with_their_original_questions() {
        let root = std::env::temp_dir().join(format!("aaa-comment-repeat-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("p", Default::default(), "test").unwrap();
        let mut rec = ThreadRec::starting("p", "질문", "m", "fake", Role::Worker, ThreadKind::Dir, 0);
        rec.id = "t-0001".into();
        rec.report_hash = Some("old".into());
        let items =
            aaa_proto::report_questions("## Questions\n### 1. 자리\n- A 유지\n### 2. 색\n- A 단색\n- B 두 색\n");
        rec.questions = h.identify_questions("p", "thread", &rec.id, "old", &items).unwrap();
        let answer = QuestionAnswerView {
            comments: vec![Some("첫째 원문".into()), Some("둘째 의견".into())],
            picks: vec![None, Some(1)],
            text: None,
            summary: "1직접 2B+의견".into(),
            by: "CLI".into(),
            at_ms: 10,
        };
        h.record_answer(&rec, &answer);
        let repeated = h.repeated_answer(&rec, &[items[1].clone(), items[0].clone()]).unwrap().unwrap();
        let answer = repeated.answer.unwrap();
        assert_eq!(answer.picks, vec![Some(1), None]);
        assert_eq!(answer.comments, vec![Some("둘째 의견".into()), Some("첫째 원문".into())]);
        assert_eq!(answer.summary, "2B+의견 1직접");
        // 새 의견 기록과 옛 공통 직접 답 기록이 함께 재보고돼도 원문을 버리거나 재검증으로 거부하지 않는다.
        rec.report_hash = Some("legacy".into());
        let mut legacy_question = rec.questions[1].clone();
        legacy_question.source_number = Some(2);
        rec.questions = vec![legacy_question];
        h.record_answer(
            &rec,
            &QuestionAnswerView {
                comments: Vec::new(),
                picks: vec![None],
                text: Some("옛 공통 답 원문".into()),
                summary: "직접".into(),
                by: "CLI".into(),
                at_ms: 20,
            },
        );
        let repeated = h.repeated_answer(&rec, &items).unwrap().unwrap();
        let answer = repeated.answer.unwrap();
        assert_eq!(answer.text.as_deref(), Some("옛 공통 답 원문"));
        assert_eq!(answer.comments, vec![Some("첫째 원문".into()), None]);
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }
}
