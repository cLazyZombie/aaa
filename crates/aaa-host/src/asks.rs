//! 「답할 것」(plan §8.345): 웹 왼쪽 막대의 한 패널에 thread 질문·coordinator 확인 필요 post·할 일 확인 필요를 모으고, 답한 것은
//! 그때의 원문으로 남긴다.
//!
//! - 열린 질문은 thread 기록(`ThreadView.questions`), 할 일은 `TaskView.attention`, 확인 필요 post는 끝내지 않은 inbox 항목이다
//!   (설치 앱 배지 `app_badge_count`와 같은 기준).
//! - 답한 것(`asks` 표): 질문은 답할 때의 질문·본문·선택지·답과, 본문이 가리킨 `library/…` 파일·보고 글을
//!   `<프로젝트>/asks/<id>/`에 사본으로 둔다. 새 보고가 질문을 비우고 library를 바꿔도 남는다. 확인 필요 post는 끝낼 때(웹
//!   「답함으로 표시」·coordinator `inbox done`), 할 일은 확인 필요가 풀릴 때 남는다.
//! - 지우기는 host 기록이라 모든 기기에서 사라진다. 답한 지 [`ASK_KEEP_DAYS`]일이 지나면 ticker가 지운다(자동 결정 원장도 같은
//!   기준, `auto_prune_pass`).
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

/// 카드 없는 thread 참조로 접수한 독립 요청은 자기 q-ID로 연다. 원래 post 제목은 보존한다.
pub(crate) fn standalone_question_title(title: &str, id: &str) -> String {
    if !title.split_whitespace().any(|w| crate::notify::app_ref(w).is_some_and(|(kind, _)| kind == "question")) {
        return title.into();
    }
    title
        .split_whitespace()
        .map(|w| {
            if crate::notify::app_ref(w).is_some_and(|(kind, _)| kind == "question") {
                format!("aaa:question/{id}")
            } else {
                w.into()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 열린 질문 본문 그림의 주소 앞부분. 그 보고 hash일 때만 열린다
pub(crate) fn question_files_base(project: &str, thread: &str, hash: &str) -> String {
    format!("/api/v1/projects/{project}/questions/{thread}/{hash}/")
}

fn record_files_base(project: &str, id: &str) -> String {
    format!("/api/v1/projects/{project}/asks/{id}/")
}

/// 답 글(공통 답·질문별 의견)이 가리키는 프로젝트 `uploads/` 그림을 답 사본 폴더 `uploads/`에 복사한다(t-1635). 답한 카드는
/// 그 사본을 사본 주소(`files_base`)로 읽는다: 웹 주소는 기기별 읽기 경로라 프로젝트 uploads 주소를 만들 수 없고, uploads를
/// 정리해도 답할 때의 그림이 남는다. symlink·일반 파일이 아닌 원본은 넘기지 않는다
pub(crate) fn snapshot_answer_images(dir: &Path, root: &Path, answer: &QuestionAnswerView) -> std::io::Result<()> {
    let mut names: Vec<String> = Vec::new();
    for text in answer.text.iter().chain(answer.comments.iter().flatten()) {
        for name in crate::answer::upload_refs(text) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    if names.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(root.join("uploads"))?;
    for name in names {
        let src = dir.join("uploads").join(&name);
        if !std::fs::symlink_metadata(&src)?.is_file() {
            return Err(std::io::Error::other(format!("uploads/{name}가 일반 파일이 아니다")));
        }
        std::fs::copy(&src, root.join("uploads").join(&name))?;
    }
    Ok(())
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
pub(crate) fn file_under(root: &Path, path: &str) -> Result<PathBuf, ErrorBody> {
    let missing = || err(ErrorCode::NotFound, format!("파일이 없다: {path}"));
    let ok = path == "report.md" || path.strip_prefix("library/").is_some_and(crate::library::safe_rel);
    if !ok {
        return Err(err(ErrorCode::BadRequest, "report.md나 library/ 아래 경로만 연다"));
    }
    let mut at = root.to_path_buf();
    for part in Path::new(path).components() {
        let Component::Normal(part) = part else { return Err(missing()) };
        at.push(part);
        let m = std::fs::symlink_metadata(&at).map_err(|e| aaa_common::diagnostic::context(e, missing()))?;
        if m.file_type().is_symlink() {
            return Err(missing());
        }
    }
    if !std::fs::symlink_metadata(&at).map_err(|e| aaa_common::diagnostic::context(e, missing()))?.is_file() {
        return Err(missing());
    }
    Ok(at)
}

/// 빠진 파일 진단: 경로 앞 3개와 고칠 방법. `where_`는 그 판에서 파일을 찾는 곳이다.
pub(crate) fn missing_files_message(missing: &[String], where_: &str) -> String {
    let shown: Vec<_> = missing.iter().take(3).map(|p| format!("library/{p}")).collect();
    let more = missing.len().saturating_sub(shown.len());
    format!(
        "파일 확인 필요: 본문이 가리키는 library 파일 {}개가 {where_}에 없다({}{}). 카드·보고서에 「이미지를 표시할 수 없음」만 보인다. 파일을 그 경로에 두거나(이전 판 파일이면 지금 library로 복사) 경로를 있는 파일로 고친다.",
        missing.len(),
        shown.join(", "),
        if more > 0 { format!(" 외 {more}개") } else { String::new() }
    )
}

impl Host {
    /// post 본문이 가리키는 프로젝트 library 파일 중 없는 것(카드 `request_file`과 같은 곳을 본다)
    pub(crate) fn missing_project_library(&self, project: &str, text: &str) -> Vec<String> {
        let Ok(dir) = self.project_dir(project) else { return Vec::new() };
        aaa_proto::library_paths(text)
            .into_iter()
            .filter(|rel| file_under(&dir, &format!("library/{rel}")).is_err())
            .collect()
    }

    /// thread 보고가 가리키는 library 파일 중 host 사본에 없는 것(카드 `question_file`과 같은 곳을 본다)
    pub(crate) fn missing_thread_library(&self, project: &str, thread: &str, text: &str) -> Vec<String> {
        let Ok(dir) = self.project_dir(project) else { return Vec::new() };
        aaa_proto::library_paths(text)
            .into_iter()
            .filter(|rel| !crate::library::safe_rel(rel) || crate::library::local_file(&dir, thread, rel).is_err())
            .collect()
    }

    pub(crate) fn request_question_thread(&self, item: &InboxItem) -> Option<String> {
        let id = crate::notify::request_question(item)?;
        // 질문 원장이 이미 독립 post로 식별했으면 제목의 옛 thread 링크는 종료 근거가 아니다.
        // 조회 실패도 자동 처리의 허가로 삼지 않는다.
        if self
            .db
            .has_post_questions(&item.project, &item.id)
            .inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "post 질문 소유권을 읽지 못해 자동 연결하지 않는다")
            })
            .unwrap_or(true)
        {
            return None;
        }
        match self
            .db
            .question_target(Some(&item.project), &id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
        {
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
        if let Some(thread) = self.request_question_thread(item) {
            if let Ok(rec) = self.load(&item.project, &thread)
                && let Some(hash) = rec.report_hash.as_ref()
                && !rec.questions.is_empty()
            {
                return Ok(self
                    .identify_questions(&item.project, "thread", &thread, hash, &rec.questions)?
                    .into_iter()
                    .filter_map(|q| q.id)
                    .collect());
            }
            // 접수 때 연결된 알림은 카드가 닫힌 뒤 조회해도 독립 질문으로 바꾸지 않는다.
            // 최초 접수(아직 post가 없음)에서만 카드 없는 참조를 독립 요청으로 식별한다.
            if self.db.post_of_item(&item.project, &item.id).map_err(internal)?.is_some() {
                return Ok(Vec::new());
            }
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
        self.request_question_thread(item)
            .and_then(|id| {
                self.load(&item.project, &id)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .is_some_and(|r| {
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
        self.inbox_finish_with_notice(project, items, by, false, |i| {
            Some(format!("{why} 확인 필요 post도 처리했다({by}): {}", i.summary))
        })
        .map_err(internal)?;
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
                let Ok(rec) = self.load(project, &thread).inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
                }) else {
                    return history.next().is_some();
                };
                if rec.questions.is_empty() {
                    // 이미 닫힌 카드에 늦게 온 알림도 새 판단 요청으로 만들지 않는다.
                    rec.report_changed_ms.is_some_and(|at| at >= i.created_at_ms) || history.next().is_some()
                } else if let Some(answer) = rec.question_answer.as_ref() {
                    // 고친 기록(t-2117)도 처음 답이 이력에 남아 같은 답으로 본다
                    history.any(|r| {
                        r.answer.iter().chain(&r.revisions).any(|a| a.at_ms == answer.at_ms && a.by == answer.by)
                    })
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
                report_id: self.thread_report_id(&rec.project, &rec.id, rec.report_hash.as_deref()),
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
                revisions: Vec::new(),
                thread: None,
                correction: None,
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
            let questions = if let Some(p) = post.as_ref().filter(|_| self.request_question_thread(&i).is_none()) {
                let parsed = aaa_proto::parse_report_questions(&p.input.body);
                let items = self.identify_questions(project, "post", &i.id, &p.body_sha256, &parsed.items)?;
                (!items.is_empty() || parsed.warning.is_some()).then(|| aaa_proto::ThreadQuestions {
                    format_warning: parsed.warning,
                    review: false,
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
                title: match (post.as_ref(), question_id.as_deref()) {
                    (Some(p), Some(id)) if self.request_question_thread(&i).is_none() => {
                        standalone_question_title(&p.input.title, id)
                    }
                    (Some(p), _) => p.input.title.clone(),
                    _ => i.summary.clone(),
                },
                question_id,
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

    /// 조작(답함 표시·답·지우기·자동 모드) 뒤의 답: 쪽 있는 「답할 것」(원문 첫 쪽과 가벼운 색인)이다(t-1767).
    /// 웹은 이 본문을 쓰지 않고 `asks` 사건으로 다시 읽는다. 쪽 없는 전체 기록은 기록이 쌓이면 수 MiB라 휴대폰이 받아 버리고,
    /// front socket의 frame 상한도 넘어 CLI가 처리 뒤에 오류를 받았다.
    pub(crate) fn asks_after(&self, project: &str) -> Result<FrontResponse, ErrorBody> {
        self.asks_view_mode(project, true)
    }

    pub(crate) fn asks_view_mode(&self, project: &str, paged: bool) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        let (auto_decisions, auto_next) = self.auto_decision_page(project, None, None)?;
        let (done, done_next) = if paged {
            self.db.ask_page(project, None, None).map_err(internal)?
        } else {
            (self.db.ask_list(project).map_err(internal)?, None)
        };
        let done = self.with_corrections(project, done);
        Ok(FrontResponse::Asks {
            auto_mode: self.auto_mode(project)?,
            auto_decisions,
            auto_next,
            auto_exceptions: self.auto_exception_count(project)?,
            requests: self.user_requests(project)?,
            done,
            done_next,
            done_index: if paged { Some(self.db.ask_index(project).map_err(internal)?) } else { None },
            keep_days: ASK_KEEP_DAYS,
        })
    }

    pub(crate) fn ask_history(
        &self,
        project: &str,
        before: Option<&str>,
        id: Option<&str>,
    ) -> Result<FrontResponse, ErrorBody> {
        self.project_dir(project)?;
        if before.is_some() && id.is_some() {
            return Err(err(ErrorCode::BadRequest, "페이지와 단건 조회를 함께 지정할 수 없다"));
        }
        let cursor = before
            .map(|s| {
                let (ms, row) = s.split_once(':').ok_or(())?;
                let ms = ms.parse::<i64>().map_err(|_| ())?;
                let row = row.parse::<i64>().map_err(|_| ())?;
                if ms < 0 || row <= 0 {
                    return Err(());
                }
                Ok((ms, row))
            })
            .transpose()
            .map_err(|()| err(ErrorCode::BadRequest, "답변 이력 커서가 올바르지 않다"))?;
        let (done, next) = self.db.ask_page(project, cursor, id).map_err(internal)?;
        Ok(FrontResponse::AskHistory { done: self.with_corrections(project, done), next })
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
        let title = self
            .db
            .post_of_item(project, item)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .map(|p| p.input.title)
            .unwrap_or(i.summary.clone());
        let by = self.answered_by(actor);
        let n = self.inbox_finish_with_notice(project, vec![i.clone()], &by, true, |_| {
            Some(format!("사용자가 확인 필요 post를 답함으로 표시했다({by}): 「{title}」. 답이 채팅에 없으면 다시 묻지 말고 처리된 것으로 본다"))
        }).map_err(internal)?;
        self.db.audit(actor, "inbox_done", &format!("{project} {n}"));
        self.asks_after(project)
    }

    /// 같은 질문 검증·답 조합 규칙을 쓰되 coordinator inbox로 전한다. 한 트랜잭션으로 답 기록과 요청 종료를 묶는다.
    #[allow(clippy::too_many_arguments)] // thread와 같은 Front API 답 입력을 받는다.
    pub(crate) fn ask_answer(
        &self,
        project: &str,
        item: &str,
        body_hash: &str,
        picks: Vec<Option<u32>>,
        text: Option<String>,
        comments: Vec<Option<String>>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.ask_answer_apply(project, item, body_hash, picks, text, comments, actor)?;
        self.asks_after(project)
    }

    /// 답 저장·알림까지의 실제 처리 결과. 자동 답은 뒤의 목록 조회 실패와 구분한다.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn ask_answer_apply(
        &self,
        project: &str,
        item: &str,
        body_hash: &str,
        picks: Vec<Option<u32>>,
        text: Option<String>,
        comments: Vec<Option<String>>,
        actor: &str,
    ) -> Result<(), ErrorBody> {
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
        let parsed = aaa_proto::parse_report_questions(&post.input.body);
        let mut parsed = parsed.items;
        if parsed.is_empty() {
            parsed.push(aaa_proto::ThreadQuestion {
                source_number: None,
                after: None,
                id: None,
                text: post.input.title.clone(),
                body: Some(post.input.body.clone()),
                choices: Vec::new(),
            });
        }
        let items = self.identify_questions(project, "post", item, &post.body_sha256, &parsed)?;
        let (message, summary) = crate::answer::compose(&items, &picks, text.as_deref(), &comments)
            .map_err(|e| err(ErrorCode::BadRequest, e))?;
        // 답 그림은 coordinator(host)가 여는 프로젝트 uploads/의 절대 경로로 적는다(기록은 `uploads/…` 그대로, t-1635)
        let images = crate::answer::upload_refs(&message);
        let message = if images.is_empty() {
            message
        } else {
            let uploads = self.project_dir(project)?.join("uploads");
            crate::answer::rebase_uploads(&message, &images, &uploads.to_string_lossy())
        };
        let by = self.answered_by(actor);
        let now = aaa_common::util::now_ms();
        let id = aaa_common::util::new_id();
        let dir = self.project_dir(project)?;
        let root = dir.join("asks").join(&id);
        if let Err(e) = self.snapshot_request(&dir, &root, &post.input.body) {
            aaa_common::diagnostic::ignored(
                std::fs::remove_dir_all(&root),
                "정리 실패가 본 작업의 결과를 덮지 않게 한다",
            );
            return Err(internal(e));
        }
        let record = AskRecord {
            report_id: None,
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
                delivery: None,
                comments: crate::answer::normalize_comments(comments),
                picks,
                text: text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()),
                summary,
                by: by.clone(),
                at_ms: now,
            }),
            body: Some(post.input.body),
            files_base: Some(record_files_base(project, &id)),
            revisions: Vec::new(),
            thread: self.request_link(i, &post.input.title),
            correction: None,
        };
        if let Some(answer) = &record.answer
            && let Err(e) = snapshot_answer_images(&dir, &root, answer)
        {
            tracing::warn!("답 그림 사본을 다 두지 못했다({project}/{item}): {e:#}");
        }
        let notice = InboxItem {
            short_id: None,
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
                record
                    .answer
                    .as_ref()
                    .map(|answer| crate::answer::preview(&record.items, answer))
                    .unwrap_or_else(|| record.summary.clone())
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
            aaa_common::diagnostic::ignored(
                std::fs::remove_dir_all(&root),
                "정리 실패가 본 작업의 결과를 덮지 않게 한다",
            );
            return saved.and_then(|_| Err(err(ErrorCode::BadRequest, "이미 답함")));
        }
        // DB 종료 뒤 파일 이동과 상태 사건도 기존 inbox 경로를 따른다. 답 기록은 위에서 이미 남겼다.
        let mut done = i.clone();
        done.done = true;
        done.done_at_ms = Some(now);
        self.inbox_finish_publish(&done, &by, false);
        self.inbox_publish(&notice);
        self.db.audit(actor, "ask_answer", &format!("{project}/{item}"));
        Ok(())
    }

    /// coordinator가 미리 publish한 `library/<이름>/<파일>`만 복사한다. 외부·symlink는 열지 않는다.
    pub(crate) fn snapshot_request(&self, dir: &Path, root: &Path, body: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        std::fs::write(root.join("report.md"), body)?;
        let mut total = 0u64;
        for rel in library_refs(body) {
            let path = format!("library/{rel}");
            let Ok(from) = file_under(dir, &path).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
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
            aaa_common::diagnostic::ignored(
                std::fs::remove_dir_all(dir.join("asks").join(id)),
                "정리 실패가 본 작업의 결과를 덮지 않게 한다",
            );
        }
        self.db.audit(actor, "ask_forget", &format!("{project} {}", gone.len()));
        if !gone.is_empty() {
            self.asks_bump();
        }
        self.asks_after(project)
    }

    /// 답한 지 오래된 기록과 그 사본을 지운다(ticker)
    pub(crate) fn ask_prune_pass(&self, now_ms: u64) {
        let keep = u64::from(ASK_KEEP_DAYS) * 86_400_000;
        let Ok(gone) = self
            .db
            .ask_prune(now_ms.saturating_sub(keep))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        for (project, id) in &gone {
            if let Ok(dir) = self.project_dir(project) {
                aaa_common::diagnostic::ignored(
                    std::fs::remove_dir_all(dir.join("asks").join(id)),
                    "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                );
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
        let Ok(dir) = self
            .project_dir(project)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let id = aaa_common::util::new_id();
        let root = dir.join("asks").join(&id);
        if let Err(e) = self.snapshot_question(&dir, rec, &root) {
            tracing::warn!("답한 질문 사본을 다 두지 못했다({}/{}): {e:#}", project, rec.id);
        }
        if let Err(e) = snapshot_answer_images(&dir, &root, answer) {
            tracing::warn!("답 그림 사본을 다 두지 못했다({}/{}): {e:#}", project, rec.id);
        }
        self.ask_save(
            project,
            &AskRecord {
                report_id: self.thread_report_id(project, &rec.id, rec.report_hash.as_deref()),
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
                revisions: Vec::new(),
                thread: None,
                correction: None,
            },
        );
        if let Err(e) =
            self.db.question_clear_drafts(project, "thread", &rec.id, rec.report_hash.as_deref().unwrap_or_default())
        {
            tracing::warn!("질문 초안을 정리하지 못했다: {e:#}");
        }
    }

    pub(crate) fn snapshot_question(&self, dir: &Path, rec: &ThreadRec, root: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        // 답한 보고 판(r-ID)의 불변 사본을 먼저 쓴다: 답을 보내는 사이 새 보고가 와도 그때 보고·그림을 남긴다(t-2117)
        let version = self.thread_report_id(&rec.project, &rec.id, rec.report_hash.as_deref());
        let report = version
            .as_ref()
            .map(|v| dir.join("threads").join(format!("{v}.md")))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| dir.join("threads").join(format!("{}.md", rec.id)));
        if report.is_file() {
            std::fs::copy(&report, root.join("report.md"))?;
        }
        let mut refs = Vec::new();
        for q in &rec.questions {
            for text in q.body.iter().chain(q.choices.iter().filter_map(|c| c.body.as_ref())).chain(q.after.iter()) {
                for r in library_refs(text) {
                    if !refs.contains(&r) {
                        refs.push(r);
                    }
                }
            }
        }
        let mut total = 0u64;
        for rel in refs {
            let found = version
                .as_deref()
                .and_then(|v| crate::library::local_file(dir, v, &rel).ok())
                .map_or_else(|| crate::library::local_file(dir, &rec.id, &rel), Ok);
            let Ok((from, size)) = found.inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
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
        let post = self
            .db
            .post_of_item(&item.project, &item.id)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten();
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
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
            })
            .unwrap_or_default();
        let id = aaa_common::util::new_id();
        let files_base = if !items.is_empty() {
            self.project_dir(&item.project)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                .and_then(|dir| {
                    self.snapshot_request(&dir, &dir.join("asks").join(&id), &body)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
                .map(|_| record_files_base(&item.project, &id))
        } else {
            None
        };
        self.ask_save(
            &item.project,
            &AskRecord {
                report_id: None,
                question_id: post
                    .as_ref()
                    .and_then(|p| {
                        self.post_question_ids(item, p)
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()
                    })
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
                revisions: Vec::new(),
                thread: None,
                correction: None,
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
                report_id: None,
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
                revisions: Vec::new(),
                thread: None,
                correction: None,
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
            let m = std::fs::symlink_metadata(&p).map_err(|source_error| {
                aaa_common::diagnostic::context(source_error, err(ErrorCode::NotFound, "보고 사본이 없다"))
            })?;
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
        let Some(request) =
            open.iter().find(|r| r.id == item && r.questions.as_ref().is_some_and(|q| q.report_hash == hash))
        else {
            return Err(err(ErrorCode::NotFound, "열린 post 질문이 아니다"));
        };
        if !path.starts_with("library/") {
            return Err(err(ErrorCode::BadRequest, "library/ 아래 경로만 연다"));
        }
        // 질문에 넣은 보고서 폴더만 읽는다. 같은 프로젝트의 다른 보고서를 cap으로 열지 않는다.
        let rel = &path["library/".len()..];
        if !library_refs(&request.body).iter().any(|r| match r.split_once('/') {
            Some((folder, _)) => rel.starts_with(&format!("{folder}/")),
            None => rel == r,
        }) {
            return Err(err(ErrorCode::NotFound, "질문에 첨부한 자료가 아니다"));
        }
        file_under(&self.project_dir(project)?, path)
    }

    /// 답한 것 사본의 파일. 사본 폴더 안 `uploads/<이름>`(답에 붙인 그림의 사본, t-1635)도 연다
    pub(crate) fn ask_file(&self, project: &str, id: &str, path: &str) -> Result<PathBuf, ErrorBody> {
        let dir = self.project_dir(project)?;
        if !crate::library::safe_rel(id) || id.contains('/') {
            return Err(err(ErrorCode::BadRequest, "기록 ID가 아니다"));
        }
        let root = dir.join("asks").join(id);
        if let Some(name) = path.strip_prefix("uploads/").filter(|n| crate::task_attach::valid_name(n)) {
            let at = root.join("uploads").join(name);
            let plain = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| !m.file_type().is_symlink());
            return if plain(&root.join("uploads")) && plain(&at) && at.is_file() {
                Ok(at)
            } else {
                Err(err(ErrorCode::NotFound, format!("파일이 없다: {path}")))
            };
        }
        file_under(&root, path)
    }
}

#[cfg(test)]
mod tests;
