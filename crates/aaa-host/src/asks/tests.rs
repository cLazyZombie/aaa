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
fn history_pages_keep_index_links_bounds_and_deleted_cursor() {
    let (h, _lab) = host();
    let mut original = Vec::new();
    for i in 0..121 {
        let record: AskRecord = serde_json::from_value(serde_json::json!({
            "id": format!("record-{i:04}"), "kind": "question", "subject": "t-0001", "title": "이력",
            "asked_ms": 1, "done_ms": 100, "by": "시험", "summary": "1A",
            "items": [{"id": format!("q-{i:04}"), "text": "질문", "choices": []}],
            "body": "본문".repeat(2000)
        }))
        .unwrap();
        h.db.ask_put("demo", &record).unwrap();
        original.push(record);
    }
    h.db.ask_put("other", &original[0]).unwrap();
    let FrontResponse::Asks { done, done_index, done_next, .. } = h.asks_view_mode("demo", true).unwrap() else {
        panic!()
    };
    let index = done_index.unwrap();
    assert_eq!(index.len(), 121);
    assert_eq!(index[0].question_ids, ["q-0120"]);
    assert_eq!(index[0].subject, "t-0001");
    assert!(serde_json::to_vec(&index).unwrap().len() < 40000);
    assert!(done.len() < 50, "바이트 경계가 먼저 걸린다");
    assert!(serde_json::to_vec(&done).unwrap().len() < 263000);
    let mut ids = done.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
    let mut cursor = done_next;
    // 방금 읽은 마지막 행을 지워도 다음 페이지가 건너뛰거나 처음으로 되돌아가지 않는다.
    h.db.ask_delete("demo", Some(&[done.last().unwrap().id.clone()])).unwrap();
    while let Some(before) = cursor {
        let FrontResponse::AskHistory { done, next } = h.ask_history("demo", Some(&before), None).unwrap() else {
            panic!()
        };
        assert!(done.len() <= 50);
        ids.extend(done.iter().map(|r| r.id.clone()));
        cursor = next;
    }
    assert_eq!(ids, original.iter().rev().map(|r| r.id.clone()).collect::<Vec<_>>());
    let FrontResponse::AskHistory { done, next } = h.ask_history("demo", None, Some("record-0000")).unwrap() else {
        panic!()
    };
    assert_eq!(done, [original[0].clone()]);
    assert!(next.is_none());
    let (done, _) = h.db.ask_page("other", None, Some("record-0001")).unwrap();
    assert!(done.is_empty());
    assert!(h.ask_history("other", None, Some("record-0000")).is_err(), "다른 프로젝트 접근 거부");
    assert!(h.ask_history("demo", Some("bad"), None).is_err());
    assert!(h.ask_history("demo", Some("1:1"), Some("record-0000")).is_err());
    let FrontResponse::Asks { done, done_index, .. } = h.asks_view_mode("demo", false).unwrap() else { panic!() };
    assert_eq!(done.len(), 120);
    assert!(done_index.is_none(), "구판 전체 조회 유지");
    for r in &original {
        let mut small = r.clone();
        small.body = None;
        h.db.ask_put("other", &small).unwrap();
    }
    let (small, next) = h.db.ask_page("other", None, None).unwrap();
    assert_eq!(small.len(), 50, "작은 기록은 개수 상한");
    assert!(next.is_some());
}

/// 조작 뒤의 답은 쪽 있는 꼴이다(t-1767): 기록이 쌓여 쪽 없는 전체가 frame 상한(1MiB)을 넘어도 지우기 응답은 그 절반 안에 든다.
#[test]
fn operation_answers_stay_paged_with_large_history() {
    let (h, _lab) = host();
    for i in 0..121 {
        let record: AskRecord = serde_json::from_value(serde_json::json!({
            "id": format!("record-{i:04}"), "kind": "request", "subject": format!("item-{i}"), "title": "이력",
            "asked_ms": 1, "done_ms": 100 + i, "by": "시험", "summary": "답함", "items": [], "body": "본문".repeat(2000)
        }))
        .unwrap();
        h.db.ask_put("demo", &record).unwrap();
    }
    assert!(serde_json::to_vec(&h.asks_view_mode("demo", false).unwrap()).unwrap().len() > 1 << 20);
    let after = h.ask_forget("demo", &["record-0000".to_string()], false, "test");
    let bytes = crate::front::res_frame("forget".into(), after);
    assert!(bytes.len() < 1 << 19, "{}", bytes.len());
    let aaa_proto::FrontMsg::Res { result, .. } = serde_json::from_slice(&bytes).unwrap() else { panic!() };
    let FrontResponse::Asks { done, done_index, done_next, .. } = result.into_result().unwrap() else { panic!() };
    assert_eq!(done_index.unwrap().len(), 120, "색인은 남은 기록 전부");
    assert!(done.len() < 50 && done_next.is_some(), "원문은 첫 쪽만");
}

/// 두 경로 모두 같은 열린 post를 읽은 뒤 종료하도록 고정해 E2E53의 경합을 재현한다.
#[test]
fn question_answer_and_reconcile_handle_each_post_once() {
    let (h, _lab) = host();
    let mut rec = ThreadRec::starting("demo", "시안", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
    rec.id = "t-0001".into();
    rec.report_hash = Some("hash".into());
    rec.questions = aaa_proto::report_questions("## Questions\n### 1. 자리\n- A 아래 (추천)\n- B 위\n");
    let answer = QuestionAnswerView {
        delivery: None,
        comments: Vec::new(),
        picks: vec![Some(0)],
        text: None,
        summary: "1A".into(),
        by: "웹".into(),
        at_ms: 5,
    };
    h.db.thread_import("demo", &rec.id, rec.status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
    for n in 1..=2 {
        let input = PostInput {
            post_id: format!("{n:032x}"),
            to: "user".into(),
            post_type: PostType::Request,
            topic: "user-action".into(),
            title: format!("알림 {n} aaa:question/t-0001"),
            body: "카드에서 답한다".into(),
            needs_user: true,
            refs: vec![],
        };
        assert!(matches!(h.front_post("demo", input, "local:cli@coord").unwrap(), PostResult::Accepted { .. }));
    }
    rec.question_answer = Some(answer.clone());
    h.save(&mut rec);
    let requests = h.question_requests("demo", &rec.id).unwrap();
    assert_eq!(requests.len(), 2);
    h.record_answer(&rec, &answer);
    *h.inbox_finish_barrier.lock().unwrap() = Some(Arc::new(std::sync::Barrier::new(2)));
    std::thread::scope(|scope| {
        let answering = scope.spawn(|| h.handle_question_requests("demo", requests.clone(), "웹", "질문 카드로 답해"));
        let reconciling = scope.spawn(|| h.reconcile_question_requests("demo"));
        answering.join().unwrap().unwrap();
        reconciling.join().unwrap().unwrap();
    });
    *h.inbox_finish_barrier.lock().unwrap() = None;
    let inbox = h.db.inbox_list(Some("demo"), true).unwrap();
    assert!(requests.iter().all(|r| inbox.iter().any(|i| i.id == r.id && i.done)));
    let handled: Vec<_> = inbox.iter().filter(|i| i.event == "request handled").collect();
    assert_eq!(handled.len(), 2, "각 post의 처리 기록은 한 번만 남는다: {handled:?}");
    for request in &requests {
        assert_eq!(handled.iter().filter(|i| i.summary.contains(&request.summary)).count(), 1);
    }
    assert_eq!(h.db.ask_list("demo").unwrap().len(), 1, "답한 질문은 하나다");
    h.handle_question_requests("demo", requests, "웹", "질문 카드로 답해").unwrap();
    h.reconcile_question_requests("demo").unwrap();
    assert_eq!(h.db.inbox_list(Some("demo"), true).unwrap().iter().filter(|i| i.event == "request handled").count(), 2);
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

    assert_eq!(h.db.inbox_list(Some("demo"), true).unwrap().iter().filter(|i| i.event == "request handled").count(), 1);
    assert!(
        aaa_proto::FrontRequest::PostHandled { project: "demo".into(), post_id: "p-00000001".into() }.needs_control()
    );
}

#[test]
fn old_question_requests_close_from_a_new_report_without_fabricating_an_answer() {
    let (h, _lab) = host();
    let mut rec = ThreadRec::starting("demo", "시안", "m", "fake", Role::Worker, ThreadKind::Dir, 1);
    rec.id = "t-0001".into();
    rec.report_hash = Some("new".into());
    rec.questions = aaa_proto::report_questions("## Questions\n1. 자리\n- A 아래 (추천)\n- B 위\n");
    h.db.thread_import("demo", &rec.id, rec.status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
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
    // 열린 카드에 붙은 알림은 그 카드의 새 보고가 질문을 닫을 때만 정리한다.
    let created = h.db.post_inbox("demo", &id).unwrap().unwrap().created_at_ms;
    rec.report_changed_ms = Some(created - 1);
    h.save(&mut rec);
    h.reconcile_question_requests("demo").unwrap();
    assert!(!h.db.post_inbox("demo", &id).unwrap().unwrap().done);
    rec.questions.clear();
    rec.report_changed_ms = Some(created + 1);
    h.save(&mut rec);
    h.user_requests("demo").unwrap(); // 조회가 연결 알림의 소유권을 바꾸면 안 된다.
    h.reconcile_question_requests("demo").unwrap();
    assert!(h.db.post_inbox("demo", &id).unwrap().unwrap().done);
    assert!(h.load("demo", &rec.id).unwrap().question_answer.is_none());
    assert!(h.db.ask_list("demo").unwrap().is_empty());

    // 구판의 카드 답 뒤 남은 알림: 임시 전송 자리는 성공으로 보지 않고, 성공 기록이 있을 때만 닫는다.
    rec.questions = aaa_proto::report_questions("## Questions\n1. 색\n- A 파랑 (추천)\n- B 회색\n");
    h.save(&mut rec);
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
        delivery: None,
        comments: Vec::new(),
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
    let answer = QuestionAnswerView {
        delivery: None,
        comments: Vec::new(),
        picks: vec![Some(0)],
        text: None,
        summary: "1A".into(),
        by: "웹".into(),
        at_ms: 1,
    };
    rec.question_answer = Some(answer.clone());
    h.db.thread_import("demo", &rec.id, rec.status.as_str(), &serde_json::to_string(&rec).unwrap()).unwrap();
    h.record_answer(&rec, &answer);
    let post = |n| {
        let result = h.seed_legacy_user_post(
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
        );
        let PostResult::Accepted { id } = result else { panic!("{result:?}") };
        h.reconcile_question_requests("demo").unwrap();
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
            title: if n >= 4 { title.split(" aaa:question/").next().unwrap() } else { title }.into(),
            body: if n >= 4 {
                "## Questions\n### 1. 별개 확인\n- A 진행\n- B 보류\n"
            } else {
                "카드에서 답한다"
            }
            .into(),
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
    assert!(h.thread_answer("demo", &rec.id, "hash", vec![Some(0)], None, Vec::new(), "device:phone").await.is_err());
    assert!(h.load("demo", &rec.id).unwrap().question_answer.is_none());
    assert_eq!(h.question_requests("demo", &rec.id).unwrap().len(), 2);
    assert!(h.db.ask_list("demo").unwrap().is_empty());
    // 뒤늦은 다음 알림·다른 host·카드 없는 요청은 자동 처리 대상이 아니다.
    post(&h, 3, "뒤늦은 알림 aaa:question/t-0001");
    post(&h, 4, "아침 정리 aaa:question/t-9999");
    let (other, _other_lab) = host();
    post(&other, 5, "다른 host aaa:question/t-0001");
    let answer = QuestionAnswerView {
        delivery: None,
        comments: Vec::new(),
        picks: vec![Some(0)],
        text: None,
        summary: "1A".into(),
        by: "웹".into(),
        at_ms: 5,
    };
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
    assert_eq!(h.db.inbox_list(Some("demo"), true).unwrap().iter().filter(|i| i.event == "request handled").count(), 2);
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
        body: "## Questions\n### 1. 배포 승인\nmain **검증 통과**. 승인하면 tag를 만든다\n- A 승인\n- B 보류\n".into(),
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
    let FrontResponse::Asks { requests, done, keep_days, .. } = h.asks_view_mode("demo", false).unwrap() else {
        panic!()
    };
    assert_eq!((requests.len(), done.len(), keep_days), (0, 1, ASK_KEEP_DAYS));
    assert_eq!(
        (done[0].kind, done[0].summary.as_str(), done[0].by.as_str()),
        (AskKind::Request, "답함으로 표시", "웹")
    );
    assert_eq!(
        done[0].body.as_deref(),
        Some("## Questions\n### 1. 배포 승인\nmain **검증 통과**. 승인하면 tag를 만든다\n- A 승인\n- B 보류\n")
    );
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
    std::fs::write(dir.join("library/t-0001/after.png"), b"after").unwrap();
    let choice = |label: &str, body: Option<&str>| QuestionChoice {
        letter: None,
        label: label.into(),
        description: None,
        recommended: false,
        body: body.map(str::to_string),
    };
    rec.questions = vec![ThreadQuestion {
        source_number: None,
        after: Some("![뒤 설명](library/after.png)".into()),
        id: None,
        text: "시안".into(),
        body: Some("![지금](library/now.png)".into()),
        choices: vec![choice("A", Some("![A](library/mock/a.png)")), choice("B", None)],
    }];
    // 답에 붙인 그림(t-1635): 프로젝트 uploads의 원본을 사본 폴더 uploads/에 둔다
    let shot = "0123456789abcdef.png";
    std::fs::create_dir_all(dir.join("uploads")).unwrap();
    std::fs::write(dir.join("uploads").join(shot), b"shot").unwrap();
    let answer = QuestionAnswerView {
        delivery: None,
        comments: vec![Some(format!("궤적이 꺾인다\n![그림](uploads/{shot})"))],
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
    std::fs::remove_file(dir.join("uploads").join(shot)).unwrap();
    assert_eq!(std::fs::read(h.ask_file("demo", &id, &format!("uploads/{shot}")).unwrap()).unwrap(), b"shot");
    for bad in ["uploads/fedcba9876543210.png", "uploads/../report.md", "uploads/0123456789ABCDEF.png"] {
        assert!(h.ask_file("demo", &id, bad).is_err(), "{bad}");
    }
    assert_eq!(
        std::fs::read(h.ask_file("demo", &id, "report.md").unwrap()).unwrap(),
        "[결정 필요] 질문 1개\n".as_bytes()
    );
    assert_eq!(std::fs::read(h.ask_file("demo", &id, "library/mock/a.png").unwrap()).unwrap(), b"a");
    assert!(h.ask_file("demo", &id, "library/now.png").is_ok());
    assert_eq!(std::fs::read(h.ask_file("demo", &id, "library/after.png").unwrap()).unwrap(), b"after");
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
