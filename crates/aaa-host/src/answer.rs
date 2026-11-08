//! thread 질문 카드의 답(plan §8.329). thread가 보고 `## Questions`에 남긴 질문에 웹 카드·`aaa thread answer`로 답한다.
//!
//! - 답은 그 보고(hash)의 질문에만 맞는다. 새 보고에서 질문을 다시 읽는다. 전달을 마친 같은 질문은 원래 ID와 답을 잇고, 새 질문은 답을 비운다(orch.rs 사본).
//! - 한 번만 받는다: 기록에 먼저 자리를 잡고(`question_answer`) 보낸다. 다른 기기가 늦게 누르면 「이미 답함」이다. 보내지
//!   못하면 자리를 풀어 다시 답할 수 있다.
//! - 보내는 길은 `thread message`와 같다(확인창·입력창 글·brief 전 거부, task 기록). coordinator를 거치지 않는다.
//! - 웹에서 답하면 coordinator inbox에 「질문에 답함」을 남긴다: coordinator가 같은 질문을 다시 묻거나 중계하지 않는다.
//! - 직접 쓴 답은 여러 줄이고 그림 줄(`![그림](uploads/<이름>)`, task 첨부와 같은 프로젝트 uploads/)을 실을 수 있다(t-1635).
//!   기록·웹은 `uploads/…` 그대로 두고, 보낼 글만 읽는 쪽이 여는 경로로 바꾼다: thread에는 그 머신 작업 폴더 `attachments/`에
//!   복사한 사본(task 첨부와 같은 `WorkAttachWrite`), coordinator inbox에는 host 프로젝트 `uploads/`의 절대 경로다.

use std::sync::Arc;

use aaa_proto::{ErrorBody, ErrorCode, MessageMode, QuestionAnswerView, ThreadQuestion, ThreadView};

use crate::{Host, err};

/// 고른 것을 확인하고 thread에 보낼 글과 짧은 꼴(「1A 2B」)을 짓는다. 모든 질문은 고르거나 직접 쓴 답으로 대신한다
pub(crate) fn compose(
    items: &[ThreadQuestion],
    picks: &[Option<u32>],
    text: Option<&str>,
    comments: &[Option<String>],
) -> Result<(String, String), String> {
    if picks.len() != items.len() {
        return Err(format!("질문은 {}개다(받은 답 {}개)", items.len(), picks.len()));
    }
    if !comments.is_empty() && comments.len() != items.len() {
        return Err(format!("질문은 {}개다(받은 의견 {}개)", items.len(), comments.len()));
    }
    let text = text.map(str::trim).filter(|t| !t.is_empty());
    let mut short = Vec::new();
    let mut lines = Vec::new();
    for (index, (q, pick)) in items.iter().zip(picks).enumerate() {
        let number = q.source_number.unwrap_or(index as u32 + 1);
        let comment = comments.get(index).and_then(|c| c.as_deref()).map(str::trim).filter(|c| !c.is_empty());
        match pick {
            Some(i) => {
                let c = q
                    .choices
                    .get(*i as usize)
                    .ok_or_else(|| format!("{}번 질문에 {}번 선택지가 없다", number, i + 1))?;
                short.push(format!(
                    "{}{}{}",
                    number,
                    c.letter_at(*i as usize),
                    if comment.is_some() { "+의견" } else { "" }
                ));
                lines.push(format!(
                    "{}{}. {}{} — {}",
                    number,
                    c.letter_at(*i as usize),
                    q.id.as_ref().map(|id| format!("{id} ")).unwrap_or_default(),
                    q.text,
                    c.label
                ));
            }
            None if comment.is_some() => short.push(format!("{number}직접")),
            None if text.is_some() => {}
            None => return Err(format!("{}번 질문에 답이 없다(고르거나 직접 쓴다)", number)),
        }
        if let Some(comment) = comment {
            lines.push(format!("{number}번 직접 쓴 답{}: {comment}", if pick.is_some() { "(의견)" } else { "" }));
        }
    }
    if let Some(t) = text {
        short.push("직접".into());
        lines.push(format!("직접 쓴 답: {t}"));
    }
    let message = format!("보고의 질문에 대한 사용자 답({})\n{}", short.join(" "), lines.join("\n"));
    Ok((message, short.join(" ")))
}

/// 답 글이 가리키는 프로젝트 uploads 파일 이름들(`](uploads/<이름>)`, 처음 나온 차례, 겹치지 않게)
pub(crate) fn upload_refs(text: &str) -> Vec<String> {
    crate::task_attach::refs(text).into_iter().filter_map(|t| t.strip_prefix("uploads/").map(str::to_string)).collect()
}

/// 글의 `](uploads/<이름>)`을 `](<dir><sep><이름>)`으로 바꾼다. `dir`이 Windows 경로(`\`만 쓴다)면 그 구분자를 쓴다
pub(crate) fn rebase_uploads(text: &str, names: &[String], dir: &str) -> String {
    let sep = if dir.contains('\\') && !dir.contains('/') { '\\' } else { '/' };
    let dir = dir.trim_end_matches(sep);
    names.iter().fold(text.to_string(), |out, n| out.replace(&format!("](uploads/{n})"), &format!("]({dir}{sep}{n})")))
}

/// 빈 의견은 null로 정리하되 질문 배열 위치와 본문 줄바꿈을 보존한다.
pub(crate) fn normalize_comments(comments: Vec<Option<String>>) -> Vec<Option<String>> {
    comments.into_iter().map(|c| c.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())).collect()
}

/// inbox의 300자 요약에도 어느 질문에 쓴 의견인지 보인다. 전체 원문은 body에 남긴다.
pub(crate) fn preview(items: &[ThreadQuestion], answer: &QuestionAnswerView) -> String {
    let mut parts = vec![answer.summary.clone()];
    for (n, item) in items.iter().enumerate() {
        if let Some(text) = answer.comments.get(n).and_then(|c| c.as_deref()) {
            let number = item.source_number.unwrap_or(n as u32 + 1);
            let choice = answer
                .picks
                .get(n)
                .copied()
                .flatten()
                .and_then(|p| item.choices.get(p as usize).map(|c| c.letter_at(p as usize)));
            parts.push(match choice {
                Some(letter) => format!("{number}{letter} + 의견: {text}"),
                None => format!("{number} 직접: {text}"),
            });
        }
    }
    if let Some(text) = &answer.text {
        parts.push(format!("직접: {text}"));
    }
    parts.join(" · ")
}

impl Host {
    /// 답한 곳의 이름: 웹 기기는 그 기기 이름(없으면 「웹」), 그 밖(CLI·coordinator)은 「CLI」
    pub(crate) fn answered_by(&self, actor: &str) -> String {
        match actor.strip_prefix("device:") {
            Some(id) => self
                .db
                .devices()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                .and_then(|ds| ds.into_iter().find(|d| d.id == id))
                .map(|d| d.label)
                .filter(|l| !l.trim().is_empty())
                .unwrap_or_else(|| "웹".into()),
            None => "CLI".into(),
        }
    }

    #[allow(clippy::too_many_arguments)] // Front API 답의 선택·옛 공통 답·질문별 의견을 같은 경계에서 검증한다.
    pub(crate) async fn thread_answer(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        report_hash: &str,
        picks: Vec<Option<u32>>,
        text: Option<String>,
        comments: Vec<Option<String>>,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let rec = self.load(project, id)?;
        if rec.questions.is_empty() {
            return Err(err(ErrorCode::BadRequest, format!("{id}에는 답할 질문이 없다")));
        }
        if rec.report_hash.as_deref() != Some(report_hash) {
            return Err(err(ErrorCode::BadRequest, "질문이 바뀌었다(새 보고가 왔다). 다시 읽고 답한다"));
        }
        if self.default_review_for(&rec)
            && picks.first() == Some(&Some(1))
            && text.as_deref().is_none_or(|t| t.trim().is_empty())
            && comments.first().and_then(|c| c.as_deref()).is_none_or(|t| t.trim().is_empty())
        {
            return Err(err(ErrorCode::BadRequest, "고칠 점을 글로 적어 답한다"));
        }
        let (message, summary) =
            compose(&rec.questions, &picks, text.as_deref(), &comments).map_err(|e| err(ErrorCode::BadRequest, e))?;
        // 답에 붙인 그림: 자리를 잡기 전에 thread 머신으로 넘긴다(못 넘기면 답을 기록하지 않고 거절한다). 같은 내용은 같은 이름이라
        // 다시 답해도 덮어쓸 뿐이다
        let images = upload_refs(&message);
        let (to_thread, to_inbox) = if images.is_empty() {
            (message.clone(), message.clone())
        } else {
            let there = self.answer_images_to_thread(&rec, &images).await?;
            let here = self.project_dir(project)?.join("uploads");
            (rebase_uploads(&message, &images, &there), rebase_uploads(&message, &images, &here.to_string_lossy()))
        };
        let requests = self.question_requests(project, id)?;
        let by = self.answered_by(actor);
        let answer = QuestionAnswerView {
            comments: normalize_comments(comments),
            picks: picks.clone(),
            text: text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
            summary: summary.clone(),
            by: by.clone(),
            at_ms: aaa_common::util::now_ms(),
        };
        let answer_preview = preview(&rec.questions, &answer);
        let question_ids = rec.questions.iter().filter_map(|q| q.id.as_deref()).collect::<Vec<_>>().join(" ");
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
        if let Err(e) = self.thread_message(project, id, to_thread, MessageMode::Queue, actor).await {
            // 보내지 못했다: 다시 답할 수 있게 자리를 푼다(그사이 새 보고면 이미 비었다)
            aaa_common::diagnostic::ignored(
                self.update_rec(project, id, |r| {
                    if r.question_answer.as_ref().is_some_and(|a| a.at_ms == answer.at_ms && a.by == answer.by) {
                        r.question_answer = None;
                    }
                }),
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
            return Err(e);
        }
        self.db.audit(actor, "thread_answer", &format!("{project}/{id} {summary}"));
        let rec = self.load(project, id)?;
        // 답할 때의 원문을 남긴다(plan §8.345): 새 보고가 질문을 비워도 「답한 것」과 대화 칩이 읽는다
        if let Some(a) = rec.question_answer.as_ref().filter(|a| a.at_ms == answer.at_ms) {
            self.record_answer(&rec, a);
        }
        self.handle_question_requests(project, requests, &by, "질문 카드로 답해")?;
        self.flow(&rec, "question_answered", Some(format!("threads/{id}.md")), format!("{summary} · {by}"));
        if actor.starts_with("device:") {
            self.inbox_add_body(
                project,
                "thread-state",
                id,
                "question answered",
                &format!("{id} {question_ids} 질문에 답함({by}): {answer_preview}"),
                &to_inbox,
            );
        }
        Ok(self.thread_view(&rec))
    }

    /// 답 그림(프로젝트 `uploads/<이름>`)을 thread 머신 작업 폴더 `attachments/`에 쓰고 그 폴더 경로(그 머신의 것)를 돌려준다.
    /// 원본은 task 첨부처럼 이름(내용 hash)과 내용이 같은 일반 파일이어야 한다
    async fn answer_images_to_thread(
        self: &Arc<Self>,
        rec: &crate::orch::ThreadRec,
        names: &[String],
    ) -> Result<String, ErrorBody> {
        use crate::task_attach::{FEATURE, MAX_BYTES, work_attach_write};
        use sha2::{Digest, Sha256};
        let machine = self.machine_name(&rec.machine_id);
        if !self.machine_feature(&rec.machine_id, FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                format!(
                    "{machine} agent가 답 그림을 받지 못한다({FEATURE} 없음). 그 머신 aaa를 갱신하거나 그림을 떼고 답한다"
                ),
            ));
        }
        let work_dir = rec.work_dir.clone().ok_or_else(|| err(ErrorCode::Internal, "작업 폴더 기록이 없다"))?;
        let uploads = self.project_dir(&rec.project)?.join("uploads");
        let conn = self.live_conn(&rec.machine_id)?;
        for name in names {
            let path = uploads.join(name);
            let file = path
                .symlink_metadata()
                .map_err(|e| err(ErrorCode::NotFound, format!("답 그림 uploads/{name}를 읽지 못했다: {e}")))?;
            if !file.is_file() || file.len() == 0 || file.len() > MAX_BYTES {
                return Err(err(
                    ErrorCode::BadRequest,
                    format!("답 그림 uploads/{name}는 1바이트~16MiB 일반 파일이 아니다"),
                ));
            }
            let bytes = std::fs::read(&path)
                .map_err(|e| err(ErrorCode::NotFound, format!("답 그림 uploads/{name}를 읽지 못했다: {e}")))?;
            if !hex::encode(Sha256::digest(&bytes)).starts_with(name.split('.').next().unwrap_or_default()) {
                return Err(err(
                    ErrorCode::BadRequest,
                    format!("답 그림 uploads/{name}의 이름과 내용 해시가 다르다. 다시 붙인다"),
                ));
            }
            work_attach_write(&conn, &work_dir, name, &bytes).await.map_err(|e| {
                err(e.code, format!("답 그림 uploads/{name}를 {machine}의 작업 폴더로 넘기지 못했다: {}", e.message))
            })?;
        }
        let sep = if work_dir.contains('\\') && !work_dir.contains('/') { "\\" } else { "/" };
        Ok(format!("{}{sep}attachments", work_dir.trim_end_matches(['/', '\\'])))
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

    #[test]
    fn emphasized_m3_choices_keep_the_original_answer_letters() {
        let items = report_questions(include_str!("../../aaa-proto/tests/fixtures/m3-questions.md"));
        let (message, short) = compose(&items, &[Some(2), Some(1)], None, &[]).unwrap();
        assert_eq!(short, "1C 2B");
        assert!(message.contains("install 순서를 바꿔"));
        assert!(message.contains("이 Mac의 「원격 로그인」을 사용자가 직접 켜고"));
    }

    #[test]
    fn reordered_report_composes_the_source_letter_instead_of_the_position() {
        let q = report_questions("## Questions\n### 1. 진행\n- B 새 안 (추천)\n- A 원래\n- C 보류\n");
        let (message, summary) = compose(&q, &[Some(0)], None, &[]).unwrap();
        assert_eq!(summary, "1B");
        assert!(message.ends_with("1B. 진행 — 새 안"));
        assert_eq!(compose(&q, &[Some(1)], None, &[]).unwrap().1, "1A");
    }

    /// 고른 것은 「1A 2B」와 질문·선택지 글로, 직접 쓴 답은 고르지 않은 질문을 대신한다. 빠진 답·없는 선택지·수가 다른 답은 거절한다
    #[test]
    fn compose_checks_every_question_and_writes_the_short_form() {
        let (msg, short) = compose(&qs(), &[Some(0), Some(1)], None, &[]).unwrap();
        assert_eq!(short, "1A 2B");
        assert_eq!(
            msg,
            "보고의 질문에 대한 사용자 답(1A 2B)\n1A. 로그인 단추 자리 — 오른쪽 아래\n2B. 눈 아이콘 — 넣지 않는다"
        );
        let (msg, short) = compose(&qs(), &[Some(0), None], Some(" 2 둘 다 보이게 "), &[]).unwrap();
        assert_eq!(short, "1A 직접");
        assert!(msg.ends_with("직접 쓴 답: 2 둘 다 보이게"), "{msg}");
        assert!(compose(&qs(), &[Some(0), None], None, &[]).unwrap_err().contains("2번 질문에 답이 없다"));
        assert!(compose(&qs(), &[Some(0), None], Some("  "), &[]).is_err());
        assert!(compose(&qs(), &[Some(0), Some(5)], None, &[]).unwrap_err().contains("2번 질문에 6번 선택지가 없다"));
        assert!(compose(&qs(), &[Some(0)], None, &[]).unwrap_err().contains("질문은 2개다"));
    }

    /// 의견이 다른 질문의 미응답을 덮지 않으며 선택과 의견·원래 번호·여러 줄 원문을 함께 전한다.
    #[test]
    fn comments_belong_to_their_question_and_preserve_choice_and_multiline_text() {
        let mut items = qs();
        items[0].source_number = Some(3);
        items[1].source_number = Some(7);
        let comments = vec![Some("오른쪽 유지\n간격은 넓게".into()), Some("둘 다 보이게".into())];
        let (message, summary) = compose(&items, &[Some(0), None], None, &comments).unwrap();
        assert_eq!(summary, "3A+의견 7직접");
        assert!(message.contains("3A. 로그인 단추 자리 — 오른쪽 아래\n3번 직접 쓴 답(의견): 오른쪽 유지\n간격은 넓게\n7번 직접 쓴 답: 둘 다 보이게"));
        assert!(
            compose(&items, &[None, None], None, &[comments[0].clone(), None])
                .unwrap_err()
                .contains("7번 질문에 답이 없다")
        );
        assert!(compose(&items, &[Some(0), None], None, &[None, Some(" \n ".into())]).is_err());
        assert!(compose(&items, &[Some(0), None], None, &comments[..1]).unwrap_err().contains("받은 의견 1개"));
        assert!(compose(&items, &[Some(99), None], None, &comments).is_err());
    }

    /// 답 그림(t-1635): 여러 줄 공통 답의 줄과 그림 줄을 그대로 싣고, 보낼 곳마다 그 쪽이 여는 경로(Unix·Windows)로만 바꾼다.
    /// 프로젝트 uploads 꼴이 아닌 참조(밖으로 나가는 경로·library)는 넘기지도 바꾸지도 않는다
    #[test]
    fn answer_images_are_rebased_per_reader_and_keep_lines() {
        let a = "0123456789abcdef.png";
        let b = "fedcba9876543210.jpg";
        let text = format!(
            "첫 줄\n둘째 줄\n![그림](uploads/{a})\n![그림](uploads/{b})\n![밖](uploads/../x.png)\n![다시](uploads/{a})"
        );
        let (message, _) = compose(&qs(), &[Some(0), None], Some(&text), &[]).unwrap();
        assert!(message.ends_with(&format!("직접 쓴 답: {text}")), "{message}");
        let names = upload_refs(&message);
        assert_eq!(names, [a, b]);
        let unix = rebase_uploads(&message, &names, "/work/t-0001/attachments/");
        assert!(unix.contains(&format!(
            "둘째 줄\n![그림](/work/t-0001/attachments/{a})\n![그림](/work/t-0001/attachments/{b})"
        )));
        assert!(
            unix.contains(&format!("![다시](/work/t-0001/attachments/{a})")) && unix.contains("](uploads/../x.png)"),
            "{unix}"
        );
        let windows = rebase_uploads(&message, &names, r"F:\work\t-0001\attachments");
        assert!(windows.contains(&format!(r"![그림](F:\work\t-0001\attachments\{b})")), "{windows}");
        assert!(upload_refs("[계획](library/t-0001/plan.md)\n![그림](uploads/0123456789ABCDEF.png)").is_empty());
    }
}
