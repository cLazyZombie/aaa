//! 답한 질문 카드 정정(t-2117, FRONT_API 1.191). 「답한 것」 기록의 답을 고친다.
//!
//! - 기한: thread 카드는 그 thread가 끝나기 전까지(열린 동안). coordinator 카드(확인 필요 post)는 중계한 thread가 끝나기
//!   전까지, 없으면 기록이 남아 있는 동안(14일)이다(사용자 11-21 3A).
//! - 처음 답은 지우지 않는다: 지금 답을 `revisions`에 덧붙이고 새 답을 `answer`로 둔다(herdr-projects의 덧붙이기만 하는
//!   follow-up 기록과 같다).
//! - 여러 기기: 고치기 시작할 때 본 지금 답의 시각(`base_at_ms`)이 기록과 다르면 거절한다(먼저 고친 쪽이 이긴다).
//!   자리를 먼저 바꾸고 보낸다. 보내지 못하면 그 자리를 되돌린다.
//! - 알림: thread 카드는 그 thread에 「답 정정」 글을 보낸다(일반 thread message 길). coordinator inbox에는 두 카드 모두
//!   `question corrected`를 남긴다. 둘 다 지금 답 뒤 일어난 일(새 보고·coordinator 요청)을 함께 싣는다(사용자 11-21 2A).
//! - 전달 대기(t-2111): 답이 아직 thread에 들어가지 않았으면 기록이 없어 고칠 길도 없다. 전달된 뒤 기록이 생기면 고친다.

use std::sync::Arc;

use aaa_proto::{
    AskCorrection, AskKind, AskRecord, ErrorBody, ErrorCode, FrontResponse, MessageMode, QuestionAnswerView,
    ThreadQuestion, ThreadStatus,
};

use crate::orch::internal;
use crate::{Host, err};

/// 「답할 것」 기록 보존 기간 안내(asks.rs `ASK_KEEP_DAYS`)
const KEEP_NOTE: &str = "답한 것 기록이 남아 있는 동안(답한 뒤 14일) 고칠 수 있다";

/// 질문 하나의 답 줄과 짧은 꼴(「1A」·「2직접」). 원래 번호를 지킨다
fn one(item: &ThreadQuestion, n: usize, pick: Option<u32>, comment: Option<&str>) -> Option<(String, String)> {
    let mut q = item.clone();
    q.source_number = Some(item.source_number.unwrap_or(n as u32 + 1));
    let comments = [comment.map(str::to_string)];
    let (message, short) = crate::answer::compose(std::slice::from_ref(&q), &[pick], None, &comments).ok()?;
    let body = message.split_once('\n').map(|(_, b)| b.to_string()).unwrap_or_default();
    Some((body, short))
}

/// 바뀐 질문만 고른다: (이전 줄, 고친 줄, 이전 짧은 꼴, 고친 짧은 꼴, 그대로인 짧은 꼴들)
struct Diff {
    before: Vec<String>,
    after: Vec<String>,
    arrows: Vec<String>,
    same: Vec<String>,
}

fn diff(items: &[ThreadQuestion], old: &QuestionAnswerView, new: &QuestionAnswerView) -> Diff {
    let mut d = Diff { before: Vec::new(), after: Vec::new(), arrows: Vec::new(), same: Vec::new() };
    for (n, item) in items.iter().enumerate() {
        let get = |a: &QuestionAnswerView| {
            (a.picks.get(n).copied().flatten(), a.comments.get(n).and_then(|c| c.as_deref()).map(str::to_string))
        };
        let (op, oc) = get(old);
        let (np, nc) = get(new);
        let o = one(item, n, op, oc.as_deref());
        let x = one(item, n, np, nc.as_deref());
        match (o, x) {
            (Some((ol, os)), Some((nl, ns))) if (op, &oc) != (np, &nc) => {
                d.before.push(ol);
                d.after.push(nl);
                d.arrows.push(format!("{os}→{ns}"));
            }
            (_, Some((_, ns))) if (op, &oc) == (np, &nc) => d.same.push(ns),
            (o, x) => {
                // 한쪽이 공통 답(text)으로만 대신한 질문: 줄 없이 짧은 꼴(「1직접」, 웹 asksModel `shortAnswer`와 같다)만
                let number = item.source_number.unwrap_or(n as u32 + 1);
                let short = |v: Option<(String, String)>| v.map(|v| v.1).unwrap_or_else(|| format!("{number}직접"));
                if (op, &oc) != (np, &nc) {
                    d.arrows.push(format!("{}→{}", short(o), short(x)));
                } else {
                    d.same.push(short(x));
                }
            }
        }
    }
    if old.text != new.text {
        let show = |t: &Option<String>| t.clone().unwrap_or_else(|| "(없음)".into());
        d.before.push(format!("직접 쓴 답: {}", show(&old.text)));
        d.after.push(format!("직접 쓴 답: {}", show(&new.text)));
        d.arrows.push("직접 쓴 답 고침".into());
    }
    d
}

/// thread·coordinator에 보내는 정정 글
pub(crate) fn correction_message(
    items: &[ThreadQuestion],
    old: &QuestionAnswerView,
    new: &QuestionAnswerView,
    since: &[String],
) -> Option<(String, String)> {
    let d = diff(items, old, new);
    if d.arrows.is_empty() {
        return None;
    }
    let arrows = d.arrows.join(" ");
    let mut lines = vec![format!("보고의 질문에 대한 사용자 답 정정({arrows})")];
    lines.push(format!("이전 답({}):", old.summary));
    lines.extend(d.before);
    lines.push(format!("고친 답({}):", new.summary));
    lines.extend(d.after);
    if !d.same.is_empty() {
        lines.push(format!("그대로: {}", d.same.join(" ")));
    }
    lines.push(
        "고친 답이 앞 답을 대신한다. 앞 답으로 이미 한 일이 있으면 고친 답에 맞추거나 되돌리고, 무엇을 했는지 보고에 적는다."
            .into(),
    );
    if !since.is_empty() {
        lines.push("앞 답 뒤 일어난 일:".into());
        lines.extend(since.iter().map(|s| format!("- {s}")));
    }
    Some((lines.join("\n"), arrows))
}

impl Host {
    /// coordinator 확인 필요 post가 이어지는 thread: 그 post가 중계한 thread 질문. 제목에 나온 t-ID로 짐작하지 않는다
    /// (구조화 질문 post는 제 질문이라 thread 기한을 따르지 않는다)
    pub(crate) fn request_link(&self, item: &aaa_proto::InboxItem) -> Option<String> {
        self.request_question_thread(item)
    }

    /// 지금 답 뒤 일어난 일: 그 thread의 새 보고 판(첫 줄)과 그 thread가 보낸 coordinator 요청
    fn ask_since(&self, project: &str, thread: &str, r: &AskRecord, after_ms: u64) -> Vec<String> {
        let mut out = Vec::new();
        if r.kind == AskKind::Question
            && let Some(answered) = r.report_id.as_deref()
            && let Ok(dir) = self.project_dir(project)
        {
            // 판은 최신부터 온다. 답한 판까지의 더 새 판이 답 뒤 보고다. 답한 판이 목록에 없으면(지운 판) 어느 판이 뒤인지
            // 모르므로 싣지 않는다
            let versions = self.report_version_names(project, thread);
            let mut newer: Vec<String> = if versions.iter().any(|id| id == answered) {
                versions.into_iter().take_while(|id| id != answered).collect()
            } else {
                Vec::new()
            };
            newer.reverse();
            for id in newer.into_iter().take(5) {
                let first = std::fs::read_to_string(dir.join("threads").join(format!("{id}.md")))
                    .ok()
                    .and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()).map(str::to_string))
                    .unwrap_or_default();
                out.push(format!("새 보고 {id}: {}", first.chars().take(80).collect::<String>()));
            }
        }
        if let Ok(posts) = self.db.post_list(project) {
            let mut sent: Vec<_> = posts
                .into_iter()
                .filter(|p| p.from.thread.as_deref() == Some(thread) && p.created_ms > after_ms)
                .collect();
            sent.sort_by_key(|p| p.created_ms);
            out.extend(sent.into_iter().take(5).map(|p| format!("coordinator에 보낸 요청: {}", p.input.title)));
        }
        out
    }

    /// 이 기록의 답을 지금 고칠 수 있는지(웹 표시와 정정 처리가 같은 판정을 쓴다)
    pub(crate) fn ask_correction(&self, project: &str, r: &AskRecord) -> Option<AskCorrection> {
        let answer = r.answer.as_ref()?;
        let thread = match r.kind {
            AskKind::Question => Some(r.subject.clone()),
            AskKind::Request => r.thread.clone(),
            AskKind::Task => return None,
        };
        let Some(thread) = thread else {
            return Some(AskCorrection { open: true, note: KEEP_NOTE.into(), since: Vec::new() });
        };
        let closed = |note: String| Some(AskCorrection { open: false, note, since: Vec::new() });
        let Ok(rec) = self.load(project, &thread) else {
            return closed(format!("{thread} 기록이 없어 답을 고칠 수 없다. 바꿀 것은 새 할 일로 맡긴다"));
        };
        match rec.status {
            ThreadStatus::Open => Some(AskCorrection {
                open: true,
                note: format!("{thread}이 끝나기 전까지 답을 고칠 수 있다"),
                since: self.ask_since(project, &thread, r, answer.at_ms),
            }),
            ThreadStatus::Parked => {
                closed(format!("{thread}이 보류 중이라 지금은 고칠 수 없다. 이어 하면 다시 고칠 수 있다"))
            }
            ThreadStatus::Starting => closed(format!("{thread}이 다시 시작하는 중이라 지금은 고칠 수 없다")),
            ThreadStatus::Resolved | ThreadStatus::Failed => {
                closed(format!("{thread}이 끝나 답을 고칠 수 없다. 바꿀 것은 새 할 일로 맡긴다"))
            }
        }
    }

    /// 보낼 기록마다 정정 상태를 채운다(저장하지 않는다)
    pub(crate) fn with_corrections(&self, project: &str, mut done: Vec<AskRecord>) -> Vec<AskRecord> {
        for r in &mut done {
            r.correction = self.ask_correction(project, r);
        }
        done
    }

    #[allow(clippy::too_many_arguments)] // 답과 같은 Front API 입력(선택·공통 답·질문별 의견)에 기준 시각을 더한다
    pub(crate) async fn ask_correct(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        base_at_ms: u64,
        picks: Vec<Option<u32>>,
        text: Option<String>,
        comments: Vec<Option<String>>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (mut page, _) = self.db.ask_page(project, None, Some(id)).map_err(internal)?;
        let mut r =
            page.pop().ok_or_else(|| err(ErrorCode::NotFound, "답한 것 기록이 없다(지웠거나 보존 기간이 지났다)"))?;
        r.correction = None;
        let old = r.answer.clone().ok_or_else(|| err(ErrorCode::BadRequest, "답이 없는 기록은 고칠 수 없다"))?;
        let state =
            self.ask_correction(project, &r).ok_or_else(|| err(ErrorCode::BadRequest, "이 기록은 고칠 수 없다"))?;
        if !state.open {
            return Err(err(ErrorCode::BadRequest, state.note));
        }
        if old.at_ms != base_at_ms {
            return Err(err(
                ErrorCode::BadRequest,
                format!("다른 곳({})이 먼저 고쳤다: {}. 다시 읽고 고친다", old.by, old.summary),
            ));
        }
        let (_, summary) = crate::answer::compose(&r.items, &picks, text.as_deref(), &comments)
            .map_err(|e| err(ErrorCode::BadRequest, e))?;
        let by = self.answered_by(actor);
        let new = QuestionAnswerView {
            delivery: None,
            comments: crate::answer::normalize_comments(comments),
            picks,
            text: text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
            summary: summary.clone(),
            by: by.clone(),
            at_ms: aaa_common::util::now_ms().max(base_at_ms + 1),
        };
        let (message, arrows) = correction_message(&r.items, &old, &new, &state.since)
            .ok_or_else(|| err(ErrorCode::BadRequest, "바뀐 답이 없다"))?;
        let thread = match r.kind {
            AskKind::Question => Some(r.subject.clone()),
            _ => None,
        };
        // 답 그림: thread에는 그 머신 작업 폴더 사본, coordinator에는 host 프로젝트 uploads/ 경로
        let images = crate::answer::upload_refs(&message);
        let (to_thread, to_inbox) = if images.is_empty() {
            (message.clone(), message.clone())
        } else {
            let here = self.project_dir(project)?.join("uploads");
            let to_inbox = crate::answer::rebase_uploads(&message, &images, &here.to_string_lossy());
            let to_thread = match &thread {
                Some(t) => {
                    let rec = self.load(project, t)?;
                    let there = self.answer_images_to_thread(&rec, &images).await?;
                    crate::answer::rebase_uploads(&message, &images, &there)
                }
                None => to_inbox.clone(),
            };
            (to_thread, to_inbox)
        };
        // 자리를 먼저 바꾼다: 두 기기가 함께 고쳐도 하나만 이긴다
        let mut next = r.clone();
        next.revisions.push(old.clone());
        next.answer = Some(new.clone());
        next.summary = summary.clone();
        next.by = by.clone();
        if !self.db.ask_swap(project, &next, base_at_ms).map_err(internal)? {
            return Err(err(ErrorCode::BadRequest, "다른 곳이 먼저 고쳤다. 다시 읽고 고친다"));
        }
        if let Some(t) = &thread
            && let Err(e) = self.thread_message_inner(project, t, to_thread, MessageMode::Queue, actor, None).await
        {
            // 보내지 못했다: 고친 자리를 되돌린다(그사이 또 고쳤으면 그대로 둔다)
            aaa_common::diagnostic::ignored(
                self.db.ask_swap(project, &r, new.at_ms),
                "정정 되돌리기 실패는 원래 오류를 덮지 않는다",
            );
            self.asks_bump();
            return Err(err(e.code, format!("{t}에 정정을 보내지 못해 고치지 않았다: {}", e.message)));
        }
        if let Ok(dir) = self.project_dir(project) {
            let root = dir.join("asks").join(&r.id);
            if let Err(e) = crate::asks::snapshot_answer_images(&dir, &root, &new) {
                tracing::warn!("고친 답 그림 사본을 다 두지 못했다({project}/{id}): {e:#}");
            }
        }
        let ids = r.items.iter().filter_map(|q| q.id.as_deref()).collect::<Vec<_>>().join(" ");
        let subject = thread.clone().unwrap_or_else(|| "coordinator".into());
        let head = [thread.clone().unwrap_or_default(), ids].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>();
        let head = if head.is_empty() { String::new() } else { format!("{} ", head.join(" ")) };
        let summary_line: String = format!("{head}답 정정({by}): {arrows}").chars().take(300).collect();
        self.inbox_add_body(project, "thread-state", &subject, "question corrected", &summary_line, &to_inbox);
        if let Some(t) = &thread
            && let Ok(rec) = self.load(project, t)
        {
            self.flow(&rec, "question_corrected", Some(format!("threads/{t}.md")), format!("{arrows} · {by}"));
        }
        self.db.audit(actor, "ask_correct", &format!("{project}/{id} {arrows}"));
        self.asks_bump();
        self.asks_after(project)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::ThreadRec;
    use aaa_proto::{Role, ThreadKind, report_questions};
    use std::path::PathBuf;

    struct Lab(PathBuf);
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn host() -> (Arc<Host>, Lab) {
        let path = std::env::temp_dir().join(format!("aaa-correct-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = path.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, path.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        (h, Lab(path))
    }
    fn thread(h: &Host, status: ThreadStatus) -> ThreadRec {
        if h.load("demo", "t-0001").is_ok() {
            h.update_rec("demo", "t-0001", |r| r.status = status).unwrap();
            return h.load("demo", "t-0001").unwrap();
        }
        let mut rec = ThreadRec::starting("demo", "로그인 화면", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
        rec.id = "t-0001".into();
        rec.status = status;
        rec.report_hash = Some("hash".into());
        rec.questions = report_questions(
            "## Questions\n### 1. 나누기\n- A 세 할 일 (추천)\n- B 한 번에\n### 2. 눈 아이콘\n- A 넣는다\n- B 뺀다\n",
        );
        h.db.thread_import("demo", &rec.id, rec.status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
        rec
    }
    fn only(h: &Host) -> AskRecord {
        let mut all = h.db.ask_list("demo").unwrap();
        assert_eq!(all.len(), 1, "{all:?}");
        all.pop().unwrap()
    }

    /// thread 카드: 열린 동안만 고친다(보류·끝남은 까닭과 함께 잠근다). 기준 시각이 다르면(다른 기기가 먼저 고침) 거절하고,
    /// thread에 정정을 보내지 못하면 고친 자리를 되돌려 처음 답이 그대로 남는다
    #[tokio::test]
    async fn thread_cards_follow_the_thread_and_never_half_apply() {
        let (h, _lab) = host();
        let rec = thread(&h, ThreadStatus::Open);
        h.record_answer(&rec, &answer(vec![Some(0), Some(0)], vec![], "1A 2A"));
        let r = only(&h);
        let state = h.ask_correction("demo", &r).unwrap();
        assert!(state.open && state.note == "t-0001이 끝나기 전까지 답을 고칠 수 있다", "{state:?}");
        let stale = h.ask_correct("demo", &r.id, 0, vec![Some(1), Some(0)], None, vec![], "device:phone").await;
        assert!(stale.unwrap_err().message.contains("먼저 고쳤다"));
        let same = h.ask_correct("demo", &r.id, 1, vec![Some(0), Some(0)], None, vec![], "device:phone").await;
        assert_eq!(same.unwrap_err().message, "바뀐 답이 없다");
        // pane이 없어 보내지 못한다: 기록은 처음 답 그대로
        let failed = h.ask_correct("demo", &r.id, 1, vec![Some(1), Some(0)], None, vec![], "device:phone").await;
        let message = failed.unwrap_err().message;
        assert!(message.contains("t-0001에 정정을 보내지 못해 고치지 않았다"), "{message}");
        let kept = only(&h);
        assert_eq!((kept.summary.as_str(), kept.revisions.len(), kept.answer.unwrap().at_ms), ("1A 2A", 0, 1));
        for (status, note) in [
            (ThreadStatus::Parked, "t-0001이 보류 중이라 지금은 고칠 수 없다. 이어 하면 다시 고칠 수 있다"),
            (ThreadStatus::Resolved, "t-0001이 끝나 답을 고칠 수 없다. 바꿀 것은 새 할 일로 맡긴다"),
            (ThreadStatus::Failed, "t-0001이 끝나 답을 고칠 수 없다. 바꿀 것은 새 할 일로 맡긴다"),
        ] {
            thread(&h, status);
            let state = h.ask_correction("demo", &r).unwrap();
            assert!(!state.open && state.note == note, "{status:?}: {state:?}");
            let locked = h.ask_correct("demo", &r.id, 1, vec![Some(1), Some(0)], None, vec![], "device:phone").await;
            assert_eq!(locked.unwrap_err().message, note);
        }
        // 목록 조회가 정정 상태를 채우되 저장하지 않는다
        let FrontResponse::Asks { done, .. } = h.asks_after("demo").unwrap() else { panic!("asks") };
        assert!(done[0].correction.as_ref().is_some_and(|c| !c.open));
        assert!(h.db.ask_list("demo").unwrap()[0].correction.is_none());
    }

    /// coordinator 카드: 연결된 thread가 없으면 기록이 남아 있는 동안 고친다. 처음 답은 이력에 남고 coordinator inbox에
    /// 정정 알림이 남는다. 두 기기가 같은 기준으로 고치면 하나만 이긴다. 연결된 thread가 끝나면 잠근다
    #[tokio::test]
    async fn coordinator_cards_keep_history_notify_and_follow_their_thread() {
        let (h, _lab) = host();
        let first = QuestionAnswerView { at_ms: 7, ..answer(vec![Some(0)], vec![], "1A") };
        let record = AskRecord {
            report_id: None,
            question_id: None,
            id: "rec-1".into(),
            kind: AskKind::Request,
            subject: "item-1".into(),
            title: "배포 승인".into(),
            asked_ms: 1,
            done_ms: 7,
            by: "iPhone".into(),
            summary: "1A".into(),
            items: report_questions("## Questions\n### 1. 배포\n- A 승인\n- B 보류\n"),
            answer: Some(first.clone()),
            body: None,
            files_base: None,
            revisions: Vec::new(),
            thread: None,
            correction: None,
        };
        h.db.ask_put("demo", &record).unwrap();
        let state = h.ask_correction("demo", &record).unwrap();
        assert!(state.open && state.note == KEEP_NOTE, "{state:?}");
        h.ask_correct("demo", "rec-1", 7, vec![Some(1)], None, vec![], "device:desk").await.unwrap();
        let fixed = only(&h);
        assert_eq!((fixed.summary.as_str(), fixed.revisions.clone()), ("1B", vec![first]));
        let now = fixed.answer.clone().unwrap();
        assert!(now.picks == vec![Some(1)] && now.at_ms > 7, "{now:?}");
        let inbox = h.db.inbox_list(Some("demo"), false).unwrap();
        let notice = inbox.iter().find(|i| i.event == "question corrected").expect("정정 알림");
        assert_eq!((notice.subject.as_str(), notice.summary.as_str()), ("coordinator", "답 정정(웹): 1A→1B"));
        assert!(
            notice.body.starts_with(
                "보고의 질문에 대한 사용자 답 정정(1A→1B)\n이전 답(1A):\n1A. 배포 — 승인\n고친 답(1B):\n1B. 배포 — 보류"
            ),
            "{}",
            notice.body
        );
        let late = h.ask_correct("demo", "rec-1", 7, vec![Some(0)], None, vec![], "device:phone").await;
        assert!(late.unwrap_err().message.contains("먼저 고쳤다"));
        assert!(!h.db.ask_swap("demo", &record, 7).unwrap(), "기준이 지난 바꾸기는 하지 않는다");
        thread(&h, ThreadStatus::Resolved);
        let linked = AskRecord { thread: Some("t-0001".into()), ..fixed };
        assert!(!h.ask_correction("demo", &linked).unwrap().open);
        let gone = AskRecord { thread: Some("t-0009".into()), ..linked };
        assert_eq!(
            h.ask_correction("demo", &gone).unwrap().note,
            "t-0009 기록이 없어 답을 고칠 수 없다. 바꿀 것은 새 할 일로 맡긴다"
        );
    }

    fn answer(picks: Vec<Option<u32>>, comments: Vec<Option<String>>, summary: &str) -> QuestionAnswerView {
        QuestionAnswerView {
            delivery: None,
            picks,
            text: None,
            comments,
            summary: summary.into(),
            by: "iPhone".into(),
            at_ms: 1,
        }
    }

    /// 바뀐 질문만 이전·고친 줄로 싣고, 그대로인 답은 짧게, 답 뒤 일어난 일을 끝에 단다
    #[test]
    fn correction_lists_only_changed_questions_and_what_happened_since() {
        let items = report_questions(
            "## Questions\n### 1. 나누기\n- A 세 할 일 (추천)\n- B 한 번에\n### 2. 눈 아이콘\n- A 넣는다\n- B 뺀다\n",
        );
        let old = answer(vec![Some(0), Some(0)], vec![], "1A 2A");
        let new = answer(vec![Some(1), Some(0)], vec![], "1B 2A");
        let (message, arrows) =
            correction_message(&items, &old, &new, &["새 보고 r-0002: [진행 중] 1A대로".into()]).unwrap();
        assert_eq!(arrows, "1A→1B");
        assert_eq!(
            message,
            "보고의 질문에 대한 사용자 답 정정(1A→1B)\n이전 답(1A 2A):\n1A. 나누기 — 세 할 일\n고친 답(1B 2A):\n1B. 나누기 — 한 번에\n\
             그대로: 2A\n고친 답이 앞 답을 대신한다. 앞 답으로 이미 한 일이 있으면 고친 답에 맞추거나 되돌리고, 무엇을 했는지 보고에 적는다.\n\
             앞 답 뒤 일어난 일:\n- 새 보고 r-0002: [진행 중] 1A대로"
        );
        assert!(correction_message(&items, &old, &old, &[]).is_none(), "바뀐 것이 없으면 보내지 않는다");
        let commented = answer(vec![Some(0), Some(0)], vec![None, Some("크게".into())], "1A 2A+의견");
        let (message, arrows) = correction_message(&items, &old, &commented, &[]).unwrap();
        assert_eq!(arrows, "2A→2A+의견");
        assert!(message.contains("2번 직접 쓴 답(의견): 크게"), "{message}");
        // 공통 직접 답이 대신한 질문은 웹(asksModel `shortAnswer`)과 같은 「2직접」으로 적고, 그대로면 그대로에 둔다
        let typed =
            QuestionAnswerView { text: Some("둘 다".into()), ..answer(vec![Some(0), None], vec![], "1A 직접") };
        let picked = answer(vec![Some(0), Some(1)], vec![], "1A 2B");
        let (message, arrows) = correction_message(&items, &typed, &picked, &[]).unwrap();
        assert_eq!(arrows, "2직접→2B 직접 쓴 답 고침");
        assert!(message.contains("그대로: 1A\n"), "{message}");
        let retyped = QuestionAnswerView { text: Some("셋 다".into()), ..typed.clone() };
        let (message, arrows) = correction_message(&items, &typed, &retyped, &[]).unwrap();
        assert_eq!(arrows, "직접 쓴 답 고침");
        assert!(message.contains("그대로: 1A 2직접"), "{message}");
    }
}
