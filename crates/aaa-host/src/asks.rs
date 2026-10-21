//! 「답할 것」(plan §8.345): 웹 왼쪽 막대의 한 패널에 thread 질문·coordinator 확인 필요 post·할 일 확인 필요를 모으고, 답한 것은
//! 그때의 원문으로 남긴다.
//!
//! - 열린 질문은 thread 기록(`ThreadView.questions`), 할 일은 `TaskView.attention`, 확인 필요 post는 끝내지 않은 inbox 항목이다
//!   (설치 앱 배지 `app_badge_count`와 같은 기준).
//! - 답한 것(`asks` 표): 질문은 답할 때의 질문·본문·선택지·답과, 본문이 가리킨 `library/…` 파일·보고 글을
//!   `<프로젝트>/asks/<id>/`에 사본으로 둔다. 새 보고가 질문을 비우고 library를 바꿔도 남는다. 확인 필요 post는 끝낼 때(웹
//!   「답함으로 표시」·coordinator `inbox done`), 할 일은 확인 필요가 풀릴 때 남는다.
//! - 지우기는 host 기록이라 모든 기기에서 사라진다. 답한 지 [`ASK_KEEP_DAYS`]일이 지나면 ticker가 지운다.
//! - 본문 그림은 웹 앱과 같은 origin의 인증된 주소로 준다(`/api/v1/projects/<p>/questions/<thread>/<hash>/…`,
//!   `/api/v1/projects/<p>/asks/<id>/…`). 그 아래는 `report.md`와 `library/<상대 경로>`뿐이고 symlink는 열지 않는다.

use std::path::{Component, Path, PathBuf};

use aaa_proto::{
    AskKind, AskRecord, ErrorBody, ErrorCode, FrontResponse, InboxItem, QuestionAnswerView, UserRequestView,
};

use crate::{Host, err, orch::ThreadRec};

/// 답한 것을 남기는 기간(사용자 결정 10-21 2A)
pub(crate) const ASK_KEEP_DAYS: u32 = 14;
/// 답한 질문 하나가 사본으로 두는 파일 합계 상한
const SNAPSHOT_MAX: u64 = 64 << 20;

fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}

/// 사용자에게 간 확인 필요 post의 inbox 항목인가(`post user --request --needs-user`)
pub(crate) fn is_user_request(i: &InboxItem) -> bool {
    i.item_kind == "post" && i.event == "post request for user"
}

/// 열린 질문 본문 그림의 주소 앞부분. 그 보고 hash일 때만 열린다
pub(crate) fn question_files_base(project: &str, thread: &str, hash: &str) -> String {
    format!("/api/v1/projects/{project}/questions/{thread}/{hash}/")
}

fn record_files_base(project: &str, id: &str) -> String {
    format!("/api/v1/projects/{project}/asks/{id}/")
}

/// 본문 Markdown이 가리키는 `library/…` 상대 경로(그림·링크 대상). 대상 끝의 제목(`"…"`)·조각(`#…`)은 뗀다
pub(crate) fn library_refs(markdown: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = markdown;
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        let (target, end) = match rest.strip_prefix('<') {
            Some(t) => (t, t.find(['>', '#', '?', '\n']).unwrap_or(t.len())),
            None => (rest, rest.find([')', ' ', '"', '#', '?', '\n']).unwrap_or(rest.len())),
        };
        let path = target[..end].trim_start_matches("./");
        if let Some(rel) = path.strip_prefix("library/")
            && crate::library::safe_rel(rel)
            && !out.iter().any(|p: &String| p == rel)
        {
            out.push(rel.to_string());
        }
    }
    out
}

/// `root` 아래의 `report.md`나 `library/<상대 경로>`(symlink는 거부)
fn file_under(root: &Path, path: &str) -> Result<PathBuf, ErrorBody> {
    let missing = || err(ErrorCode::NotFound, format!("파일이 없다: {path}"));
    let ok = path == "report.md" || path.strip_prefix("library/").is_some_and(crate::library::safe_rel);
    if !ok {
        return Err(err(ErrorCode::BadRequest, "report.md나 library/ 아래 경로만 연다"));
    }
    let mut at = root.to_path_buf();
    for part in Path::new(path).components() {
        let Component::Normal(part) = part else { return Err(missing()) };
        at.push(part);
        let m = std::fs::symlink_metadata(&at).map_err(|_| missing())?;
        if m.file_type().is_symlink() {
            return Err(missing());
        }
    }
    if !std::fs::symlink_metadata(&at).map_err(|_| missing())?.is_file() {
        return Err(missing());
    }
    Ok(at)
}

impl Host {
    pub(crate) fn request_question_thread(&self, item: &InboxItem) -> Option<String> {
        let id = crate::notify::request_question(item)?;
        match self.db.question_target(Some(&item.project), &id).ok().flatten() {
            Some(t) if t.kind == "thread" => Some(t.subject),
            Some(_) => None,
            None => Some(id),
        }
    }

    pub(crate) fn post_question_ids(
        &self,
        item: &InboxItem,
        post: &aaa_proto::PostEnvelope,
    ) -> Result<Vec<String>, ErrorBody> {
        if let Some(thread) = self.request_question_thread(item)
            && let Ok(rec) = self.load(&item.project, &thread)
            && let Some(hash) = rec.report_hash.as_ref()
            && !rec.questions.is_empty()
        {
            return Ok(self
                .identify_questions(&item.project, "thread", &thread, hash, &rec.questions)?
                .into_iter()
                .filter_map(|q| q.id)
                .collect());
        }
        let items = aaa_proto::report_questions(&post.input.body);
        if items.is_empty() {
            return Ok(vec![
                self.db.question_id(&item.project, "post", &item.id, &post.body_sha256, 0).map_err(internal)?,
            ]);
        }
        Ok(self
            .identify_questions(&item.project, "post", &item.id, &post.body_sha256, &items)?
            .into_iter()
            .filter_map(|q| q.id)
            .collect())
    }
    /// 같은 프로젝트의 열린 질문 카드가 정본이다. post 처리만으로 질문의 답을 만들지 않는다.
    pub(crate) fn request_has_open_question(&self, item: &InboxItem) -> bool {
        self.request_question_thread(item).and_then(|id| self.load(&item.project, &id).ok()).is_some_and(|r| {
            r.status != aaa_proto::ThreadStatus::Resolved
                && r.report_hash.is_some()
                && !r.questions.is_empty()
                && r.question_answer.is_none()
        })
    }

    /// 답을 보내기 전에 연결된 post를 잡는다. 전송을 기다리는 사이 새 질문에 온 post까지 끝내지 않는다.
    pub(crate) fn question_requests(&self, project: &str, thread: &str) -> Result<Vec<InboxItem>, ErrorBody> {
        Ok(self
            .db
            .inbox_list(Some(project), false)
            .map_err(internal)?
            .into_iter()
            .filter(|i| self.request_question_thread(i).as_deref() == Some(thread))
            .collect())
    }

    /// 카드 답을 보낸 뒤 연결 알림도 끝낸다. 「답한 것」에는 질문 원문과 답 하나만 남긴다.
    pub(crate) fn handle_question_requests(
        &self,
        project: &str,
        items: Vec<InboxItem>,
        by: &str,
        why: &str,
    ) -> Result<(), ErrorBody> {
        // 그 사이 coordinator가 inbox done한 항목은 다시 기록하거나 알리지 않는다.
        let open = self.db.inbox_list(Some(project), false).map_err(internal)?;
        let items: Vec<_> = items.into_iter().filter(|i| open.iter().any(|o| o.id == i.id)).collect();
        if items.is_empty() {
            return Ok(());
        }
        self.inbox_finish(project, items.clone(), by, false).map_err(internal)?;
        for i in items {
            self.inbox_add(
                project,
                "thread-state",
                "coordinator",
                "request handled",
                &format!("{why} 확인 필요 post도 처리했다({by}): {}", i.summary),
            );
        }
        Ok(())
    }

    /// 구판에서 남긴 알림도 복구한다. 전송 중의 임시 answer claim은 답 기록으로 보지 않는다.
    pub(crate) fn reconcile_question_requests(&self, project: &str) -> Result<(), ErrorBody> {
        let done = self.db.ask_list(project).map_err(internal)?;
        let items = self.db.inbox_list(Some(project), false).map_err(internal)?;
        let finished = items
            .into_iter()
            .filter(|i| {
                let Some(thread) = self.request_question_thread(i) else { return false };
                let mut history = done.iter().filter(|r| r.kind == AskKind::Question && r.subject == thread);
                let Ok(rec) = self.load(project, &thread) else { return history.next().is_some() };
                if rec.questions.is_empty() {
                    // 이미 닫힌 카드에 늦게 온 알림도 새 판단 요청으로 만들지 않는다.
                    rec.report_changed_ms.is_some_and(|at| at >= i.created_at_ms) || history.next().is_some()
                } else if let Some(answer) = rec.question_answer.as_ref() {
                    history.any(|r| r.answer.as_ref().is_some_and(|a| a.at_ms == answer.at_ms && a.by == answer.by))
                } else {
                    // 같은 thread의 다음 질문이 열렸으면 옛 답으로 닫지 않는다.
                    false
                }
            })
            .collect();
        self.handle_question_requests(project, finished, "host", "닫힌 질문을 확인해")
    }

    /// 선택한 답은 없고 새 보고가 질문을 닫았다. 이전 질문 글만 보존해 승인·선택을 만들어 내지 않는다.
    pub(crate) fn record_question_closed(&self, rec: &ThreadRec) {
        self.ask_save(
            &rec.project,
            &AskRecord {
                question_id: None,
                id: aaa_common::util::new_id(),
                kind: AskKind::Question,
                subject: rec.id.clone(),
                title: rec.title.clone(),
                asked_ms: rec.report_changed_ms.unwrap_or(rec.updated_at_ms),
                done_ms: aaa_common::util::now_ms(),
                by: "host".into(),
                summary: "새 보고로 질문 닫힘".into(),
                items: self
                    .identify_questions(
                        &rec.project,
                        "thread",
                        &rec.id,
                        rec.report_hash.as_deref().unwrap_or_default(),
                        &rec.questions,
                    )
                    .unwrap_or_else(|_| rec.questions.clone()),
                answer: None,
                body: None,
                files_base: None,
            },
        );
    }

    pub(crate) fn asks_bump(&self) {
        self.asks_changed.send_modify(|v| *v += 1);
    }

    /// 열린 확인 필요 post(오래된 것부터)
    pub(crate) fn user_requests(&self, project: &str) -> Result<Vec<UserRequestView>, ErrorBody> {
        let items = self.db.inbox_list(Some(project), false).map_err(internal)?;
        let mut out = Vec::new();
        for i in items.into_iter().filter(is_user_request) {
            let post = self.db.post_of_item(project, &i.id).map_err(internal)?;
            let question_id = post
                .as_ref()
                .map(|p| self.post_question_ids(&i, p))
                .transpose()?
                .and_then(|ids| ids.into_iter().next());
            let questions = if let Some(p) = post.as_ref().filter(|_| !self.request_has_open_question(&i)) {
                let items = self.identify_questions(
                    project,
                    "post",
                    &i.id,
                    &p.body_sha256,
                    &aaa_proto::report_questions(&p.input.body),
                )?;
                (!items.is_empty()).then(|| aaa_proto::ThreadQuestions {
                    report_hash: p.body_sha256.clone(),
                    draft: self.question_draft(project, &items),
                    items,
                    answer: None,
                    files_base: Some(format!("/api/v1/projects/{project}/requests/{}/{}/", i.id, p.body_sha256)),
                })
            } else {
                None
            };
            out.push(UserRequestView {
                question_id,
                title: post.as_ref().map(|p| p.input.title.clone()).unwrap_or_else(|| i.summary.clone()),
                body: post.map(|p| p.input.body).unwrap_or_else(|| i.summary.clone()),
                created_ms: i.created_at_ms,
                id: i.id,
                questions,
            });
        }
        out.sort_by_key(|r| r.created_ms);
        Ok(out)
    }

    /// 열린 확인 필요 post 수(웹 `asks` 사건)
    pub(crate) fn user_request_count(&self) -> u32 {
        self.db.inbox_list(None, false).map(|v| v.iter().filter(|i| is_user_request(i)).count() as u32).unwrap_or(0)
    }

    pub(crate) fn asks_view(&self, project: &str) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        Ok(FrontResponse::Asks {
            requests: self.user_requests(project)?,
            done: self.db.ask_list(project).map_err(internal)?,
            keep_days: ASK_KEEP_DAYS,
        })
    }

    /// 웹 「답함으로 표시」: 그 확인 필요 post를 끝낸다(기록은 `inbox_finish`가 남긴다). coordinator inbox에 한 줄 남긴다
    pub(crate) fn ask_handle(&self, project: &str, item: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let open = self.db.inbox_list(Some(project), false).map_err(internal)?;
        let Some(i) = open.iter().find(|i| i.id == item && is_user_request(i)) else {
            return Err(err(ErrorCode::NotFound, "열린 확인 필요 post가 아니다(이미 처리했을 수 있다)"));
        };
        if self.request_has_open_question(i) {
            return Err(err(ErrorCode::BadRequest, "연결된 질문 카드에서 답한다. 답하면 이 post도 함께 처리된다"));
        }
        let title =
            self.db.post_of_item(project, item).ok().flatten().map(|p| p.input.title).unwrap_or(i.summary.clone());
        self.inbox_done(project, &[item.to_string()], false, actor)?;
        let by = self.answered_by(actor);
        self.inbox_add(
            project,
            "thread-state",
            "coordinator",
            "request handled",
            &format!("사용자가 확인 필요 post를 답함으로 표시했다({by}): 「{title}」. 답이 채팅에 없으면 다시 묻지 말고 처리된 것으로 본다"),
        );
        self.asks_view(project)
    }

    /// 같은 질문 검증·답 조합 규칙을 쓰되 coordinator inbox로 전한다. 한 트랜잭션으로 답 기록과 요청 종료를 묶는다.
    pub(crate) fn ask_answer(
        &self,
        project: &str,
        item: &str,
        body_hash: &str,
        picks: Vec<Option<u32>>,
        text: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let open = self.db.inbox_list(Some(project), false).map_err(internal)?;
        let i = open
            .iter()
            .find(|i| i.id == item && is_user_request(i))
            .ok_or_else(|| err(ErrorCode::NotFound, "열린 확인 필요 post가 아니다(이미 처리했을 수 있다)"))?;
        if self.request_has_open_question(i) {
            return Err(err(ErrorCode::BadRequest, "연결된 질문 카드에서 답한다. 답하면 이 post도 함께 처리된다"));
        }
        let post = self
            .db
            .post_of_item(project, item)
            .map_err(internal)?
            .ok_or_else(|| err(ErrorCode::NotFound, "post 본문이 없다"))?;
        if post.body_sha256 != body_hash {
            return Err(err(ErrorCode::BadRequest, "질문 본문이 다르다. 다시 읽고 답한다"));
        }
        let mut parsed = aaa_proto::report_questions(&post.input.body);
        if parsed.is_empty() {
            parsed.push(aaa_proto::ThreadQuestion {
                id: None,
                text: post.input.title.clone(),
                body: Some(post.input.body.clone()),
                choices: Vec::new(),
            });
        }
        let items = self.identify_questions(project, "post", item, &post.body_sha256, &parsed)?;
        let (message, summary) =
            crate::answer::compose(&items, &picks, text.as_deref()).map_err(|e| err(ErrorCode::BadRequest, e))?;
        let by = self.answered_by(actor);
        let now = aaa_common::util::now_ms();
        let id = aaa_common::util::new_id();
        let dir = self.project_dir(project)?;
        let root = dir.join("asks").join(&id);
        if let Err(e) = self.snapshot_request(&dir, &root, &post.input.body) {
            let _ = std::fs::remove_dir_all(&root);
            return Err(internal(e));
        }
        let record = AskRecord {
            question_id: items.first().and_then(|q| q.id.clone()),
            id: id.clone(),
            kind: AskKind::Request,
            subject: item.to_string(),
            title: post.input.title.clone(),
            asked_ms: i.created_at_ms,
            done_ms: now,
            by: by.clone(),
            summary: summary.clone(),
            items,
            answer: Some(QuestionAnswerView {
                picks,
                text: text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
                summary,
                by: by.clone(),
                at_ms: now,
            }),
            body: Some(post.input.body),
            files_base: Some(record_files_base(project, &id)),
        };
        let notice = InboxItem {
            id: format!("{}-thread-state-coordinator-{}", aaa_common::util::utc_compact(now), id),
            project: project.into(),
            item_kind: "thread-state".into(),
            subject: "coordinator".into(),
            event: "question answered".into(),
            summary: format!(
                "{} {}: 「{}」 질문에 답함({by}): {}",
                record.items.iter().filter_map(|q| q.id.as_deref()).collect::<Vec<_>>().join(" "),
                post.id,
                post.input.title,
                record.summary
            )
            .chars()
            .take(300)
            .collect(),
            body: message,
            done: false,
            seen: false,
            created_at_ms: now,
            done_at_ms: None,
        };
        let saved = self.db.request_answer(i, &record, &notice).map_err(internal);
        if !matches!(saved, Ok(true)) {
            let _ = std::fs::remove_dir_all(&root);
            return saved.and_then(|_| Err(err(ErrorCode::BadRequest, "이미 답함")));
        }
        // DB 종료 뒤 파일 이동과 상태 사건도 기존 inbox 경로를 따른다. 답 기록은 위에서 이미 남겼다.
        self.inbox_finish(project, vec![i.clone()], &by, false).map_err(internal)?;
        self.inbox_publish(&notice);
        self.db.audit(actor, "ask_answer", &format!("{project}/{item}"));
        self.asks_view(project)
    }

    /// coordinator가 미리 publish한 `library/<이름>/<파일>`만 복사한다. 외부·symlink는 열지 않는다.
    fn snapshot_request(&self, dir: &Path, root: &Path, body: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        std::fs::write(root.join("report.md"), body)?;
        let mut total = 0u64;
        for rel in library_refs(body) {
            let path = format!("library/{rel}");
            let Ok(from) = file_under(dir, &path) else { continue };
            let size = std::fs::metadata(&from)?.len();
            if total + size > SNAPSHOT_MAX {
                continue;
            }
            total += size;
            let to = root.join(path);
            std::fs::create_dir_all(to.parent().unwrap_or(root))?;
            std::fs::copy(from, to)?;
        }
        Ok(())
    }

    pub(crate) fn ask_forget(
        &self,
        project: &str,
        ids: &[String],
        all: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        if ids.is_empty() && !all {
            return Err(err(ErrorCode::BadRequest, "지울 기록 ID나 all이 필요하다"));
        }
        let gone = self.db.ask_delete(project, (!all).then_some(ids)).map_err(internal)?;
        for id in &gone {
            let _ = std::fs::remove_dir_all(dir.join("asks").join(id));
        }
        self.db.audit(actor, "ask_forget", &format!("{project} {}", gone.len()));
        if !gone.is_empty() {
            self.asks_bump();
        }
        self.asks_view(project)
    }

    /// 답한 지 오래된 기록과 그 사본을 지운다(ticker)
    pub(crate) fn ask_prune_pass(&self, now_ms: u64) {
        let keep = u64::from(ASK_KEEP_DAYS) * 86_400_000;
        let Ok(gone) = self.db.ask_prune(now_ms.saturating_sub(keep)) else { return };
        for (project, id) in &gone {
            if let Ok(dir) = self.project_dir(project) {
                let _ = std::fs::remove_dir_all(dir.join("asks").join(id));
            }
        }
        if !gone.is_empty() {
            self.asks_bump();
        }
    }

    fn ask_save(&self, project: &str, r: &AskRecord) {
        match self.db.ask_put(project, r) {
            Ok(()) => self.asks_bump(),
            Err(e) => tracing::warn!("답한 것 기록을 남기지 못했다: {e:#}"),
        }
    }

    /// 질문에 답했다: 그때의 질문·답과 보고 글·본문이 가리킨 library 파일을 사본으로 남긴다(thread_answer가 보낸 뒤)
    pub(crate) fn record_answer(&self, rec: &ThreadRec, answer: &QuestionAnswerView) {
        let project = &rec.project;
        let Ok(dir) = self.project_dir(project) else { return };
        let id = aaa_common::util::new_id();
        let root = dir.join("asks").join(&id);
        if let Err(e) = self.snapshot_question(&dir, rec, &root) {
            tracing::warn!("답한 질문 사본을 다 두지 못했다({}/{}): {e:#}", project, rec.id);
        }
        self.ask_save(
            project,
            &AskRecord {
                question_id: None,
                id: id.clone(),
                kind: AskKind::Question,
                subject: rec.id.clone(),
                title: rec.title.clone(),
                asked_ms: rec.report_changed_ms.unwrap_or(answer.at_ms),
                done_ms: answer.at_ms,
                by: answer.by.clone(),
                summary: answer.summary.clone(),
                items: self
                    .identify_questions(
                        &rec.project,
                        "thread",
                        &rec.id,
                        rec.report_hash.as_deref().unwrap_or_default(),
                        &rec.questions,
                    )
                    .unwrap_or_else(|_| rec.questions.clone()),
                answer: Some(answer.clone()),
                body: None,
                files_base: Some(record_files_base(project, &id)),
            },
        );
        if let Err(e) =
            self.db.question_clear_drafts(project, "thread", &rec.id, rec.report_hash.as_deref().unwrap_or_default())
        {
            tracing::warn!("질문 초안을 정리하지 못했다: {e:#}");
        }
    }

    fn snapshot_question(&self, dir: &Path, rec: &ThreadRec, root: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        let report = dir.join("threads").join(format!("{}.md", rec.id));
        if report.is_file() {
            std::fs::copy(&report, root.join("report.md"))?;
        }
        let mut refs = Vec::new();
        for q in &rec.questions {
            for text in q.body.iter().chain(q.choices.iter().filter_map(|c| c.body.as_ref())) {
                for r in library_refs(text) {
                    if !refs.contains(&r) {
                        refs.push(r);
                    }
                }
            }
        }
        let mut total = 0u64;
        for rel in refs {
            let Ok((from, size)) = crate::library::local_file(dir, &rec.id, &rel) else { continue };
            if total + size > SNAPSHOT_MAX {
                tracing::warn!("답한 질문 사본 상한을 넘어 {rel}를 두지 않았다");
                continue;
            }
            total += size;
            let to = root.join("library").join(&rel);
            std::fs::create_dir_all(to.parent().unwrap_or(root))?;
            std::fs::copy(from, to)?;
        }
        Ok(())
    }

    /// 확인 필요 post를 끝냈다(웹 「답함으로 표시」·coordinator `inbox done`)
    pub(crate) fn record_request(&self, item: &InboxItem, by: &str, now_ms: u64) {
        // coordinator가 알림만 inbox done해도 열린 질문과 별도의 「답한 것」을 만들지 않는다.
        if self.request_has_open_question(item) {
            return;
        }
        let post = self.db.post_of_item(&item.project, &item.id).ok().flatten();
        let body = post.as_ref().map(|p| p.input.body.clone()).unwrap_or_else(|| item.summary.clone());
        let items = post
            .as_ref()
            .and_then(|p| {
                self.identify_questions(
                    &item.project,
                    "post",
                    &item.id,
                    &p.body_sha256,
                    &aaa_proto::report_questions(&body),
                )
                .ok()
            })
            .unwrap_or_default();
        let id = aaa_common::util::new_id();
        let files_base = if !items.is_empty() {
            self.project_dir(&item.project)
                .ok()
                .and_then(|dir| self.snapshot_request(&dir, &dir.join("asks").join(&id), &body).ok())
                .map(|_| record_files_base(&item.project, &id))
        } else {
            None
        };
        self.ask_save(
            &item.project,
            &AskRecord {
                question_id: post
                    .as_ref()
                    .and_then(|p| self.post_question_ids(item, p).ok())
                    .and_then(|ids| ids.into_iter().next()),
                id,
                kind: AskKind::Request,
                subject: item.id.clone(),
                title: post.as_ref().map(|p| p.input.title.clone()).unwrap_or_else(|| item.summary.clone()),
                asked_ms: item.created_at_ms,
                done_ms: now_ms,
                by: by.to_string(),
                summary: "답함으로 표시".into(),
                items,
                answer: None,
                body: Some(body),
                files_base,
            },
        );
        if let Some(p) = post.as_ref()
            && let Err(e) = self.db.question_clear_drafts(&item.project, "post", &item.id, &p.body_sha256)
        {
            tracing::warn!("post 질문 초안을 정리하지 못했다: {e:#}");
        }
    }

    /// 할 일의 확인 필요가 풀렸다(끝남으로 지우기·연결만 풀기·다시 맡기기)
    pub(crate) fn record_task(&self, project: &str, title: &str, attention: &str, summary: &str, by: &str) {
        let now = aaa_common::util::now_ms();
        self.ask_save(
            project,
            &AskRecord {
                question_id: None,
                id: aaa_common::util::new_id(),
                kind: AskKind::Task,
                subject: title.to_string(),
                title: title.to_string(),
                asked_ms: now,
                done_ms: now,
                by: by.to_string(),
                summary: summary.to_string(),
                items: Vec::new(),
                answer: None,
                body: Some(attention.to_string()),
                files_base: None,
            },
        );
    }

    /// 열린 질문 본문의 파일(그 보고 hash일 때만): `report.md`는 보고 사본, `library/…`는 thread 결과물 host 사본
    pub(crate) fn question_file(
        &self,
        project: &str,
        thread: &str,
        hash: &str,
        path: &str,
    ) -> Result<PathBuf, ErrorBody> {
        let dir = self.project_dir(project)?;
        let rec = self.load(project, thread)?;
        if rec.report_hash.as_deref() != Some(hash) || rec.questions.is_empty() {
            return Err(err(ErrorCode::NotFound, "질문이 바뀌었다(새 보고가 왔다)"));
        }
        if path == "report.md" {
            let p = dir.join("threads").join(format!("{thread}.md"));
            let m = std::fs::symlink_metadata(&p).map_err(|_| err(ErrorCode::NotFound, "보고 사본이 없다"))?;
            if !m.is_file() {
                return Err(err(ErrorCode::NotFound, "보고 사본이 없다"));
            }
            return Ok(p);
        }
        let rel =
            path.strip_prefix("library/").ok_or_else(|| err(ErrorCode::BadRequest, "library/ 아래 경로만 연다"))?;
        if !crate::library::safe_rel(rel) {
            return Err(err(ErrorCode::BadRequest, "library 상대 경로가 아니다"));
        }
        Ok(crate::library::local_file(&dir, thread, rel)?.0)
    }

    /// 열린 coordinator 질문의 프로젝트 library 파일
    pub(crate) fn request_file(&self, project: &str, item: &str, hash: &str, path: &str) -> Result<PathBuf, ErrorBody> {
        let open = self.user_requests(project)?;
        if !open.iter().any(|r| r.id == item && r.questions.as_ref().is_some_and(|q| q.report_hash == hash)) {
            return Err(err(ErrorCode::NotFound, "열린 post 질문이 아니다"));
        }
        if !path.starts_with("library/") {
            return Err(err(ErrorCode::BadRequest, "library/ 아래 경로만 연다"));
        }
        file_under(&self.project_dir(project)?, path)
    }

    /// 답한 것 사본의 파일
    pub(crate) fn ask_file(&self, project: &str, id: &str, path: &str) -> Result<PathBuf, ErrorBody> {
        let dir = self.project_dir(project)?;
        if !crate::library::safe_rel(id) || id.contains('/') {
            return Err(err(ErrorCode::BadRequest, "기록 ID가 아니다"));
        }
        file_under(&dir.join("asks").join(id), path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{PostInput, PostResult, PostType, QuestionChoice, Role, ThreadKind, ThreadQuestion};
    use std::sync::Arc;

    struct Lab(PathBuf);
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn host() -> (Arc<Host>, Lab) {
        let path = std::env::temp_dir().join(format!("aaa-asks-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = path.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, path.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        (h, Lab(path))
    }

    #[test]
    fn post_handled_and_inbox_done_share_state_without_answering_the_question() {
        let (h, _lab) = host();
        let mut rec = ThreadRec::starting("demo", "시안", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
        rec.id = "t-0001".into();
        rec.report_hash = Some("hash".into());
        rec.questions = aaa_proto::report_questions("## Questions\n1. 자리\n- A 아래 (추천)\n- B 위\n");
        h.db.thread_import("demo", &rec.id, rec.status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
        for n in 1..=2 {
            let nonce = format!("{n:032x}");
            let result = h
                .front_post(
                    "demo",
                    PostInput {
                        post_id: nonce.clone(),
                        to: "user".into(),
                        post_type: PostType::Request,
                        topic: "user-action".into(),
                        title: format!("알림 {n} aaa:question/t-0001"),
                        body: "카드에는 없는 C안".into(),
                        needs_user: true,
                        refs: vec![],
                    },
                    "local:cli",
                )
                .unwrap();
            let PostResult::Accepted { id } = result else { panic!("{result:?}") };
            assert!(matches!(
                h.post_status_view("demo", &id, "local:cli").unwrap(),
                FrontResponse::Post { handled: Some(false), .. }
            ));
            if n == 1 {
                assert!(h.post_handled("demo", &id, "worker:s1").is_err());
                h.post_handled("demo", &id, "local:cli@coord").unwrap();
                h.post_handled("demo", &nonce, "local:cli@coord").unwrap(); // 같은 처리를 두 번 기록하지 않는다
            } else {
                let item = h.db.post_inbox("demo", &id).unwrap().unwrap();
                h.inbox_done("demo", &[item.id], false, "local:cli").unwrap();
            }
            assert!(matches!(
                h.post_status_view("demo", &id, "local:cli").unwrap(),
                FrontResponse::Post { handled: Some(true), .. }
            ));
            assert_eq!(
                h.post_status("demo", &nonce, "local:cli").unwrap(),
                h.post_status("demo", &id, "local:cli").unwrap()
            );
            assert!(h.post_status("other", &id, "local:cli").is_err());
            assert_eq!(h.db.post_list("demo").unwrap().iter().find(|p| p.id == id).unwrap().state, "handled");
        }
        assert!(h.load("demo", &rec.id).unwrap().question_answer.is_none());
        assert!(h.db.ask_list("demo").unwrap().is_empty());

        assert_eq!(
            h.db.inbox_list(Some("demo"), true).unwrap().iter().filter(|i| i.event == "request handled").count(),
            1
        );
        assert!(
            aaa_proto::FrontRequest::PostHandled { project: "demo".into(), post_id: "p-00000001".into() }
                .needs_control()
        );
    }

    #[test]
    fn old_question_requests_close_from_a_new_report_without_fabricating_an_answer() {
        let (h, _lab) = host();
        let mut rec = ThreadRec::starting("demo", "시안", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
        rec.id = "t-0001".into();
        rec.report_hash = Some("new".into());
        let result = h
            .front_post(
                "demo",
                PostInput {
                    post_id: format!("{:032x}", 1),
                    to: "user".into(),
                    post_type: PostType::Request,
                    topic: "user-action".into(),
                    title: "옛 질문 aaa:question/t-0001".into(),
                    body: "질문 알림".into(),
                    needs_user: true,
                    refs: vec![],
                },
                "local:cli",
            )
            .unwrap();
        let PostResult::Accepted { id } = result else { panic!() };
        // 보고 뒤에 온, 카드가 없는 요청은 남긴다. 그 뒤 새 보고가 질문을 닫으면 정리한다.
        let created = h.db.post_inbox("demo", &id).unwrap().unwrap().created_at_ms;
        rec.report_changed_ms = Some(created - 1);
        h.db.thread_import("demo", &rec.id, rec.status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
        h.reconcile_question_requests("demo").unwrap();
        assert!(!h.db.post_inbox("demo", &id).unwrap().unwrap().done);
        rec.report_changed_ms = Some(created + 1);
        h.save(&mut rec);
        h.reconcile_question_requests("demo").unwrap();
        assert!(h.db.post_inbox("demo", &id).unwrap().unwrap().done);
        assert!(h.load("demo", &rec.id).unwrap().question_answer.is_none());
        assert!(h.db.ask_list("demo").unwrap().is_empty());

        // 구판의 카드 답 뒤 남은 알림: 임시 전송 자리는 성공으로 보지 않고, 성공 기록이 있을 때만 닫는다.
        let result = h
            .front_post(
                "demo",
                PostInput {
                    post_id: format!("{:032x}", 2),
                    to: "user".into(),
                    post_type: PostType::Request,
                    topic: "user-action".into(),
                    title: "카드 답 뒤 남은 알림 aaa:question/t-0001".into(),
                    body: "다음 질문".into(),
                    needs_user: true,
                    refs: vec![],
                },
                "local:cli",
            )
            .unwrap();
        let PostResult::Accepted { id } = result else { panic!() };
        let created = h.db.post_inbox("demo", &id).unwrap().unwrap().created_at_ms;
        rec.questions = aaa_proto::report_questions("## Questions\n1. 색\n- A 파랑 (추천)\n- B 회색\n");
        let answer = QuestionAnswerView {
            picks: vec![Some(0)],
            text: None,
            summary: "1A".into(),
            by: "웹".into(),
            at_ms: created + 1,
        };
        rec.question_answer = Some(answer.clone());
        h.save(&mut rec);
        h.reconcile_question_requests("demo").unwrap();
        assert!(!h.db.post_inbox("demo", &id).unwrap().unwrap().done, "전송 자리만 잡았다");
        h.record_answer(&rec, &answer);
        h.reconcile_question_requests("demo").unwrap();
        assert!(h.db.post_inbox("demo", &id).unwrap().unwrap().done, "성공한 카드 답을 확인했다");
    }

    /// 늦은 알림은 끝난 카드에 합치고, 다음 질문이 열렸으면 옛 답으로 처리하지 않는다.
    #[test]
    fn late_question_notifications_follow_the_current_card() {
        let (h, _lab) = host();
        let mut rec = ThreadRec::starting("demo", "시안", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
        rec.id = "t-0001".into();
        rec.report_hash = Some("old".into());
        rec.questions = aaa_proto::report_questions("## Questions\n1. 색\n- A 파랑 (추천)\n- B 회색\n");
        let answer =
            QuestionAnswerView { picks: vec![Some(0)], text: None, summary: "1A".into(), by: "웹".into(), at_ms: 1 };
        rec.question_answer = Some(answer.clone());
        h.db.thread_import("demo", &rec.id, rec.status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
        h.record_answer(&rec, &answer);
        let post = |n| {
            let result = h
                .front_post(
                    "demo",
                    PostInput {
                        post_id: format!("{n:032x}"),
                        to: "user".into(),
                        post_type: PostType::Request,
                        topic: "user-action".into(),
                        title: format!("질문 알림 {n} aaa:question/t-0001"),
                        body: "카드에서 답한다".into(),
                        needs_user: true,
                        refs: vec![],
                    },
                    "local:cli",
                )
                .unwrap();
            let PostResult::Accepted { id } = result else { panic!("{result:?}") };
            h.db.post_inbox("demo", &id).unwrap().unwrap()
        };
        assert!(post(1).done, "이미 답한 카드에 뒤늦게 보낸 알림");
        rec.report_hash = Some("next".into());
        rec.question_answer = None;
        h.save(&mut rec);
        assert!(!post(2).done, "다음 질문의 알림은 옛 답으로 닫지 않는다");
        assert_eq!(h.db.ask_list("demo").unwrap().len(), 1);
    }

    /// 카드가 정본: 전송 실패는 알림을 남기고, 성공 뒤에는 잡아 둔 연결 post만 처리한다.
    #[tokio::test]
    async fn question_requests_follow_the_card_without_a_second_done_record() {
        let (h, _lab) = host();
        let mut rec = ThreadRec::starting("demo", "시안", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
        rec.id = "t-0001".into();
        rec.report_hash = Some("hash".into());
        rec.questions = aaa_proto::report_questions("## Questions\n1. 자리\n- A 아래 (추천)\n- B 위\n");
        h.db.thread_import("demo", &rec.id, rec.status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
        let post = |host: &Host, n: u32, title: &str| {
            let input = PostInput {
                post_id: format!("{n:032x}"),
                to: "user".into(),
                post_type: PostType::Request,
                topic: "user-action".into(),
                title: title.into(),
                body: "카드에서 답한다".into(),
                needs_user: true,
                refs: vec![],
            };
            // 독립 요청은 웹 controller가 보낸다. CLI 발신자의 분당 한도를 소진하는 fixture를 만들지 않는다.
            let actor = if n == 4 { "device:phone" } else { "local:cli@coord" };
            let result = host.front_post("demo", input, actor).unwrap();
            assert!(matches!(result, PostResult::Accepted { .. }), "{n}: {result:?}");
        };
        let baseline_badge = h.app_badge_count().unwrap();
        post(&h, 1, "시안 aaa:question/t-0001");
        post(&h, 2, "시안 보충 aaa:question/t-0001");
        assert_eq!(h.app_badge_count().unwrap(), baseline_badge, "같은 카드의 알림은 배지를 더하지 않는다");
        let requests = h.question_requests("demo", &rec.id).unwrap();
        assert_eq!(requests.len(), 2);
        assert!(h.ask_handle("demo", &requests[0].id, "device:phone").unwrap_err().message.contains("질문 카드"));
        // pane이 없으므로 실제 전달 경로가 실패한다. post와 질문은 다시 답할 수 있어야 한다.
        assert!(h.thread_answer("demo", &rec.id, "hash", vec![Some(0)], None, "device:phone").await.is_err());
        assert!(h.load("demo", &rec.id).unwrap().question_answer.is_none());
        assert_eq!(h.question_requests("demo", &rec.id).unwrap().len(), 2);
        assert!(h.db.ask_list("demo").unwrap().is_empty());
        // 뒤늦은 다음 알림·다른 host·카드 없는 요청은 자동 처리 대상이 아니다.
        post(&h, 3, "뒤늦은 알림 aaa:question/t-0001");
        post(&h, 4, "아침 정리 aaa:question/t-9999");
        let (other, _other_lab) = host();
        post(&other, 5, "다른 host aaa:question/t-0001");
        let answer =
            QuestionAnswerView { picks: vec![Some(0)], text: None, summary: "1A".into(), by: "웹".into(), at_ms: 5 };
        h.record_answer(&rec, &answer);
        h.handle_question_requests("demo", requests.clone(), "웹", "질문 카드로 답해").unwrap();
        let done = h.db.ask_list("demo").unwrap();
        assert_eq!(done.len(), 1);
        assert_eq!((done[0].kind, done[0].summary.as_str()), (AskKind::Question, "1A"));
        assert_eq!(h.user_requests("demo").unwrap().len(), 2);
        assert_eq!(other.user_requests("demo").unwrap().len(), 1);
        let inbox = h.db.inbox_list(Some("demo"), true).unwrap();
        assert!(requests.iter().all(|r| inbox.iter().any(|i| i.id == r.id && i.done)));
        assert_eq!(inbox.iter().filter(|i| i.event == "request handled").count(), 2);
        h.handle_question_requests("demo", requests, "웹", "질문 카드로 답해").unwrap();
        assert_eq!(
            h.db.inbox_list(Some("demo"), true).unwrap().iter().filter(|i| i.event == "request handled").count(),
            2
        );
        let ordinary = h.user_requests("demo").unwrap().into_iter().find(|r| r.title.starts_with("아침")).unwrap();
        h.ask_handle("demo", &ordinary.id, "device:phone").unwrap();
        assert_eq!(h.db.ask_list("demo").unwrap().len(), 2, "카드 없는 post는 따로 기록한다");
        // coordinator가 알림만 끝내도 카드 답을 위조하거나 별도 답한 기록을 만들지 않는다.
        let reminder = h.question_requests("demo", &rec.id).unwrap().remove(0);
        h.inbox_done("demo", &[reminder.id], false, "local:cli").unwrap();
        assert!(h.load("demo", &rec.id).unwrap().question_answer.is_none());
        assert_eq!(h.db.ask_list("demo").unwrap().len(), 2);
    }

    /// 확인 필요 post: 열린 동안 「답할 것」, 「답함으로 표시」면 끝나고 기록·coordinator inbox 한 줄이 남는다. 다시 표시하면 거절
    #[test]
    fn user_request_is_open_until_handled_and_then_recorded() {
        let (h, _lab) = host();
        let mut seen = h.asks_changed.subscribe();
        let input = PostInput {
            post_id: format!("{:032x}", 7),
            to: "user".into(),
            post_type: PostType::Request,
            topic: "user-action".into(),
            title: "배포 승인 필요".into(),
            body: "main **검증 통과**. 승인하면 tag를 만든다".into(),
            needs_user: true,
            refs: vec![],
        };
        assert!(matches!(h.front_post("demo", input, "local:cli@coord").unwrap(), PostResult::Accepted { .. }));
        assert!(seen.has_changed().unwrap(), "확인 필요 post는 asks 사건을 낸다");
        seen.borrow_and_update();
        let open = h.user_requests("demo").unwrap();
        assert_eq!((open.len(), open[0].title.as_str()), (1, "배포 승인 필요"));
        assert!(open[0].body.contains("**검증 통과**"));
        assert_eq!(h.user_request_count(), 1);
        h.ask_handle("demo", &open[0].id, "device:phone").unwrap();
        assert!(seen.has_changed().unwrap());
        let FrontResponse::Asks { requests, done, keep_days } = h.asks_view("demo").unwrap() else { panic!() };
        assert_eq!((requests.len(), done.len(), keep_days), (0, 1, ASK_KEEP_DAYS));
        assert_eq!(
            (done[0].kind, done[0].summary.as_str(), done[0].by.as_str()),
            (AskKind::Request, "답함으로 표시", "웹")
        );
        assert_eq!(done[0].body.as_deref(), Some("main **검증 통과**. 승인하면 tag를 만든다"));
        let inbox = h.db.inbox_list(Some("demo"), false).unwrap();
        assert!(
            inbox.iter().any(|i| i.event == "request handled" && i.summary.contains("배포 승인 필요")),
            "{inbox:?}"
        );
        assert!(h.ask_handle("demo", &open[0].id, "device:phone").is_err(), "이미 처리했다");
    }

    /// 답한 질문: 보고 글·본문이 가리킨 library 파일을 사본으로 남겨, 원본이 바뀌거나 지워져도 연다. 지우기·14일 정리는 사본도 지운다
    #[test]
    fn answered_question_keeps_its_report_and_pictures() {
        let (h, _lab) = host();
        let dir = h.project_dir("demo").unwrap();
        let mut rec = ThreadRec::starting("demo", "시안 고르기", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
        rec.id = "t-0001".into();
        std::fs::create_dir_all(dir.join("threads")).unwrap();
        std::fs::write(dir.join("threads/t-0001.md"), "[결정 필요] 질문 1개\n").unwrap();
        std::fs::create_dir_all(dir.join("library/t-0001/mock")).unwrap();
        std::fs::write(dir.join("library/t-0001/now.png"), b"png").unwrap();
        std::fs::write(dir.join("library/t-0001/mock/a.png"), b"a").unwrap();
        std::fs::write(dir.join("library/t-0001/other.png"), b"x").unwrap();
        let choice = |label: &str, body: Option<&str>| QuestionChoice {
            label: label.into(),
            description: None,
            recommended: false,
            body: body.map(str::to_string),
        };
        rec.questions = vec![ThreadQuestion {
            id: None,
            text: "시안".into(),
            body: Some("![지금](library/now.png)".into()),
            choices: vec![choice("A", Some("![A](library/mock/a.png)")), choice("B", None)],
        }];
        let answer = QuestionAnswerView {
            picks: vec![Some(0)],
            text: None,
            summary: "1A".into(),
            by: "iPhone".into(),
            at_ms: 5,
        };
        h.record_answer(&rec, &answer);
        let done = h.db.ask_list("demo").unwrap();
        assert_eq!((done.len(), done[0].subject.as_str(), done[0].summary.as_str()), (1, "t-0001", "1A"));
        let mut expected = rec.questions.clone();
        expected[0].id = Some("q-1000".into());
        assert_eq!(done[0].items, expected);
        let id = done[0].id.clone();
        assert_eq!(done[0].files_base.as_deref(), Some(format!("/api/v1/projects/demo/asks/{id}/").as_str()));
        // 새 보고가 원본을 바꾸고 지워도 사본이 남는다. 가리키지 않은 파일은 두지 않는다
        std::fs::write(dir.join("threads/t-0001.md"), "[완료]\n").unwrap();
        std::fs::remove_dir_all(dir.join("library/t-0001")).unwrap();
        assert_eq!(
            std::fs::read(h.ask_file("demo", &id, "report.md").unwrap()).unwrap(),
            "[결정 필요] 질문 1개\n".as_bytes()
        );
        assert_eq!(std::fs::read(h.ask_file("demo", &id, "library/mock/a.png").unwrap()).unwrap(), b"a");
        assert!(h.ask_file("demo", &id, "library/now.png").is_ok());
        assert!(h.ask_file("demo", &id, "library/other.png").is_err());
        for bad in ["../threads/t-0001.md", "library/../report.md", "other.txt", "library/.hidden"] {
            assert!(h.ask_file("demo", &id, bad).is_err(), "{bad}");
        }
        assert!(h.ask_file("demo", "../demo", "report.md").is_err());
        // 지우기
        h.ask_forget("demo", std::slice::from_ref(&id), false, "device:phone").unwrap();
        assert!(h.db.ask_list("demo").unwrap().is_empty() && !dir.join("asks").join(&id).exists());
        // 14일 정리
        h.record_answer(&rec, &answer);
        let old = h.db.ask_list("demo").unwrap()[0].id.clone();
        h.record_task("demo", "할 일", "까닭", "끝남으로 지움", "CLI");
        h.ask_prune_pass(5 + u64::from(ASK_KEEP_DAYS) * 86_400_000 + 1);
        let left = h.db.ask_list("demo").unwrap();
        assert!(left.iter().all(|r| r.id != old) && left.len() == 1 && left[0].kind == AskKind::Task, "{left:?}");
        assert!(!dir.join("asks").join(&old).exists());
    }

    #[test]
    fn library_refs_read_link_and_picture_targets() {
        let md = "![지금](library/now.png) [시안](./library/mock/index.html#a) ![밖](https://x/y.png)\n\
                  ![위](library/../secret) ![같음](library/now.png \"제목\") [<꺾쇠>](<library/a b.png>)";
        assert_eq!(library_refs(md), vec!["now.png", "mock/index.html", "a b.png"]);
    }
}
