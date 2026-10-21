use super::*;
use aaa_proto::{PostInput, PostResult, PostType};
use std::path::PathBuf;

struct Lab {
    h: Arc<Host>,
    path: PathBuf,
}
impl Lab {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("aaa-qids-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = path.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, path.clone()).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        Self { h, path }
    }
    fn post(&self, n: u32, body: &str) -> (String, aaa_proto::UserRequestView) {
        let result = self
            .h
            .front_post(
                "demo",
                PostInput {
                    post_id: format!("{n:032x}"),
                    to: "user".into(),
                    post_type: PostType::Request,
                    topic: "user-action".into(),
                    title: format!("진행 안 {n}"),
                    body: body.into(),
                    needs_user: true,
                    refs: vec![],
                },
                "local:cli@coord",
            )
            .unwrap();
        let PostResult::Accepted { id } = result else { panic!("{result:?}") };
        let item = self.h.db.post_inbox("demo", &id).unwrap().unwrap();
        let r = self.h.user_requests("demo").unwrap().into_iter().find(|r| r.id == item.id).unwrap();
        (id, r)
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
const BODY: &str = "## Questions\n### 1. 자리\n![시안](library/preview/a.png)\n\n| 안 | 위치 |\n| --- | --- |\n| A | 아래 |\n\n- A 아래 (추천) — 손에 닿음\n- B 위 — 지금과 같음\n### 2. 색\n- A 파랑 (추천)\n- B 회색\n";

#[test]
fn ids_are_project_local_persistent_and_not_reused_after_history_deletion() {
    let lab = Lab::new();
    let h = &lab.h;
    let mut joins = Vec::new();
    let gate = Arc::new(std::sync::Barrier::new(8));
    for _ in 0..8 {
        let h = h.clone();
        let gate = gate.clone();
        joins.push(std::thread::spawn(move || {
            gate.wait();
            h.db.question_id("demo", "thread", "t-0001", "first", 0).unwrap()
        }));
    }
    for join in joins {
        assert_eq!(join.join().unwrap(), "q-1000");
    }
    assert_eq!(h.db.question_id("demo", "thread", "t-0001", "next", 0).unwrap(), "q-1001");
    assert_eq!(h.db.question_id("other", "post", "post", "", 0).unwrap(), "q-1000");
    h.ask_forget("demo", &[], true, "local:cli").unwrap();
    let db = crate::db::Db::open(&lab.path.join("reopen.sqlite")).unwrap();
    assert_eq!(db.question_id("demo", "post", "p1", "", 0).unwrap(), "q-1000");
    drop(db);
    let db = crate::db::Db::open(&lab.path.join("reopen.sqlite")).unwrap();
    assert_eq!(db.question_id("demo", "post", "p1", "", 0).unwrap(), "q-1000");
    assert_eq!(db.question_id("demo", "post", "p2", "", 0).unwrap(), "q-1001");
    assert_eq!(h.db.question_id("demo", "post", "p2", "", 0).unwrap(), "q-1002");
}

#[tokio::test]
async fn structured_post_answers_by_id_keep_each_question_and_close_once_with_full_text() {
    let lab = Lab::new();
    let h = &lab.h;
    let dir = h.project_dir("demo").unwrap();
    std::fs::create_dir_all(dir.join("library/preview")).unwrap();
    std::fs::write(dir.join("library/preview/a.png"), b"png").unwrap();
    let (post, r) = lab.post(1, BODY);
    let q = r.questions.unwrap();
    let item = h.db.post_inbox("demo", &post).unwrap().unwrap();
    let alert = crate::notify::alert_text(&crate::notify::Place::Project("demo".into()), &[item], &Default::default());
    assert_eq!(alert.title, "demo · q-1000 q-1001");
    assert_eq!(alert.url, "/#question=q-1000");
    assert_eq!(q.items[0].id.as_deref(), Some("q-1000"));
    assert_eq!(q.items[1].id.as_deref(), Some("q-1001"));
    assert_eq!(q.items[0].choices[0].description.as_deref(), Some("손에 닿음"));
    assert!(h.ask_answer("demo", &r.id, "stale", vec![Some(0), Some(1)], None, "device:phone").is_err());
    assert!(h.ask_answer("demo", &r.id, &q.report_hash, vec![Some(99), Some(1)], None, "device:phone").is_err());
    assert!(h.request_file("demo", &r.id, &q.report_hash, "library/preview/a.png").is_ok());
    for path in ["report.md", "library/../brief.md", "library/.env"] {
        assert!(h.request_file("demo", &r.id, &q.report_hash, path).is_err());
    }
    assert!(matches!(
        h.reply_by_id("demo", "q-1000", Some("B"), None, "device:phone").await.unwrap(),
        FrontResponse::Done
    ));
    assert!(h.reply_by_id("demo", "q-1000", Some("A"), None, "device:phone").await.is_err());
    assert_eq!(
        h.user_requests("demo").unwrap()[0].questions.as_ref().unwrap().draft.as_ref().unwrap().picks,
        vec![Some(1), None]
    );
    assert!(h.db.ask_list("demo").unwrap().is_empty());
    let text = "사용자 답 끝까지 보존 ".repeat(80);
    h.reply_by_id("demo", "q-1001", None, Some(text.clone()), "device:phone").await.unwrap();
    assert!(h.user_requests("demo").unwrap().is_empty());
    let done = h.db.ask_list("demo").unwrap();
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].answer.as_ref().unwrap().picks, vec![Some(1), None]);
    assert!(done[0].answer.as_ref().unwrap().text.as_ref().unwrap().contains(text.trim()));
    assert_eq!(done[0].items, q.items);
    assert!(h.db.question_target(Some("demo"), "q-1000").unwrap().unwrap().draft.is_none());
    assert!(h.db.question_target(Some("demo"), "q-1001").unwrap().unwrap().draft.is_none());
    std::fs::remove_file(dir.join("library/preview/a.png")).unwrap();
    assert_eq!(std::fs::read(h.ask_file("demo", &done[0].id, "library/preview/a.png").unwrap()).unwrap(), b"png");
    let inbox = h.db.inbox_list(Some("demo"), false).unwrap();
    let answer = inbox.iter().find(|i| i.event == "question answered" && i.body.contains(text.trim())).unwrap();
    assert!(answer.summary.contains("q-1000 q-1001"));
    assert!(answer.body.contains("q-1000 자리 — 위"));
    assert!(matches!(
        h.post_status_view("demo", &post, "local:cli").unwrap(),
        FrontResponse::Post { handled: Some(true), .. }
    ));
    assert!(h.reply_by_id("demo", "q-1001", Some("A"), None, "device:phone").await.is_err());
}

#[test]
fn two_devices_submit_one_answer_and_plain_chat_handling_keeps_the_alias() {
    let lab = Lab::new();
    let h = &lab.h;
    let (post, r) = lab.post(1, BODY);
    let stale = h.db.post_inbox("demo", &post).unwrap().unwrap();
    let hash = r.questions.as_ref().unwrap().report_hash.clone();
    let gate = Arc::new(std::sync::Barrier::new(2));
    let joins: Vec<_> = (0..2)
        .map(|n| {
            let h = h.clone();
            let id = r.id.clone();
            let hash = hash.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                h.ask_answer("demo", &id, &hash, vec![Some(n), Some(0)], None, "device:phone").is_ok()
            })
        })
        .collect();
    assert_eq!(joins.into_iter().filter_map(|j| j.join().ok()).filter(|v| *v).count(), 1);
    assert_eq!(h.db.ask_list("demo").unwrap().len(), 1);
    // 두 요청이 같은 열린 항목을 이미 읽은 경우도 DB 종료 조건으로 막는다.
    let mut duplicate = h.db.ask_list("demo").unwrap()[0].clone();
    duplicate.id = "duplicate-record".into();
    let mut notice =
        h.db.inbox_list(Some("demo"), false).unwrap().into_iter().find(|i| i.event == "question answered").unwrap();
    notice.id = "duplicate-notice".into();
    assert!(!h.db.request_answer(&stale, &duplicate, &notice).unwrap());
    assert_eq!(
        h.db.inbox_list(Some("demo"), false).unwrap().iter().filter(|i| i.event == "question answered").count(),
        1
    );
    assert!(matches!(
        h.post_status_view("demo", &post, "local:cli").unwrap(),
        FrontResponse::Post { handled: Some(true), .. }
    ));
    let (plain, r) = lab.post(2, "평문 요청");
    assert!(r.questions.is_none());
    assert_eq!(r.question_id.as_deref(), Some("q-1002"));
    h.post_handled("demo", "q-1002", "local:cli").unwrap();
    assert!(h.question_catalog(true).unwrap().iter().any(|q| q.id == "q-1002" && q.state == QuestionState::Gone));
    assert!(matches!(
        h.post_status_view("demo", &plain, "local:cli").unwrap(),
        FrontResponse::Post { handled: Some(true), .. }
    ));
    assert_eq!(
        h.db.ask_list("demo").unwrap().iter().find(|r| r.title == "진행 안 2").unwrap().question_id.as_deref(),
        Some("q-1002")
    );
}
