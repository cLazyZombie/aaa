//! 「답할 것」(plan §8.333): 웹 왼쪽 막대의 한 패널에 thread 질문·coordinator 확인 필요 post·할 일 확인 필요를 모으고, 답한 것은
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
    pub(crate) fn asks_bump(&self) {
        self.asks_changed.send_modify(|v| *v += 1);
    }

    /// 열린 확인 필요 post(오래된 것부터)
    pub(crate) fn user_requests(&self, project: &str) -> Result<Vec<UserRequestView>, ErrorBody> {
        let items = self.db.inbox_list(Some(project), false).map_err(internal)?;
        let mut out: Vec<UserRequestView> = items
            .into_iter()
            .filter(is_user_request)
            .map(|i| {
                let post = self.db.post_of_item(project, &i.id).ok().flatten();
                UserRequestView {
                    title: post.as_ref().map(|p| p.input.title.clone()).unwrap_or_else(|| i.summary.clone()),
                    body: post.map(|p| p.input.body).unwrap_or_else(|| i.summary.clone()),
                    created_ms: i.created_at_ms,
                    id: i.id,
                }
            })
            .collect();
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
        let title = self.db.post_of_item(project, item).ok().flatten().map(|p| p.input.title).unwrap_or(i.summary.clone());
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

    pub(crate) fn ask_forget(&self, project: &str, ids: &[String], all: bool, actor: &str) -> Result<FrontResponse, ErrorBody> {
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
                id: id.clone(),
                kind: AskKind::Question,
                subject: rec.id.clone(),
                title: rec.title.clone(),
                asked_ms: rec.report_changed_ms.unwrap_or(answer.at_ms),
                done_ms: answer.at_ms,
                by: answer.by.clone(),
                summary: answer.summary.clone(),
                items: rec.questions.clone(),
                answer: Some(answer.clone()),
                body: None,
                files_base: Some(record_files_base(project, &id)),
            },
        );
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
        let post = self.db.post_of_item(&item.project, &item.id).ok().flatten();
        self.ask_save(
            &item.project,
            &AskRecord {
                id: aaa_common::util::new_id(),
                kind: AskKind::Request,
                subject: item.id.clone(),
                title: post.as_ref().map(|p| p.input.title.clone()).unwrap_or_else(|| item.summary.clone()),
                asked_ms: item.created_at_ms,
                done_ms: now_ms,
                by: by.to_string(),
                summary: "답함으로 표시".into(),
                items: Vec::new(),
                answer: None,
                body: Some(post.map(|p| p.input.body).unwrap_or_else(|| item.summary.clone())),
                files_base: None,
            },
        );
    }

    /// 할 일의 확인 필요가 풀렸다(끝남으로 지우기·연결만 풀기·다시 맡기기)
    pub(crate) fn record_task(&self, project: &str, title: &str, attention: &str, summary: &str, by: &str) {
        let now = aaa_common::util::now_ms();
        self.ask_save(
            project,
            &AskRecord {
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
    pub(crate) fn question_file(&self, project: &str, thread: &str, hash: &str, path: &str) -> Result<PathBuf, ErrorBody> {
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
        let rel = path.strip_prefix("library/").ok_or_else(|| err(ErrorCode::BadRequest, "library/ 아래 경로만 연다"))?;
        if !crate::library::safe_rel(rel) {
            return Err(err(ErrorCode::BadRequest, "library 상대 경로가 아니다"));
        }
        Ok(crate::library::local_file(&dir, thread, rel)?.0)
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
        assert_eq!((done[0].kind, done[0].summary.as_str(), done[0].by.as_str()), (AskKind::Request, "답함으로 표시", "웹"));
        assert_eq!(done[0].body.as_deref(), Some("main **검증 통과**. 승인하면 tag를 만든다"));
        let inbox = h.db.inbox_list(Some("demo"), false).unwrap();
        assert!(inbox.iter().any(|i| i.event == "request handled" && i.summary.contains("배포 승인 필요")), "{inbox:?}");
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
            text: "시안".into(),
            body: Some("![지금](library/now.png)".into()),
            choices: vec![choice("A", Some("![A](library/mock/a.png)")), choice("B", None)],
        }];
        let answer = QuestionAnswerView { picks: vec![Some(0)], text: None, summary: "1A".into(), by: "iPhone".into(), at_ms: 5 };
        h.record_answer(&rec, &answer);
        let done = h.db.ask_list("demo").unwrap();
        assert_eq!((done.len(), done[0].subject.as_str(), done[0].summary.as_str()), (1, "t-0001", "1A"));
        assert_eq!(done[0].items, rec.questions);
        let id = done[0].id.clone();
        assert_eq!(done[0].files_base.as_deref(), Some(format!("/api/v1/projects/demo/asks/{id}/").as_str()));
        // 새 보고가 원본을 바꾸고 지워도 사본이 남는다. 가리키지 않은 파일은 두지 않는다
        std::fs::write(dir.join("threads/t-0001.md"), "[완료]\n").unwrap();
        std::fs::remove_dir_all(dir.join("library/t-0001")).unwrap();
        assert_eq!(std::fs::read(h.ask_file("demo", &id, "report.md").unwrap()).unwrap(), "[결정 필요] 질문 1개\n".as_bytes());
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
