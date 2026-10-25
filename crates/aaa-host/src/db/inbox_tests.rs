use super::*;
use aaa_proto::InboxItem;

fn item(id: &str, project: &str, event: &str) -> InboxItem {
    InboxItem {
        id: id.into(),
        project: project.into(),
        item_kind: "post".into(),
        subject: "coordinator".into(),
        event: event.into(),
        summary: "확인 필요".into(),
        body: "원문".into(),
        done: false,
        seen: false,
        created_at_ms: 1,
        done_at_ms: None,
    }
}

#[test]
fn finish_and_notice_commit_once_without_overwriting_current_data() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let source = item("request", "demo", "post request for user");
    let other = item("other", "elsewhere", "post request for user");
    db.inbox_insert(&source).unwrap();
    db.inbox_insert(&other).unwrap();
    let notice = item("request-handled", "demo", "request handled");
    // 다른 프로젝트·없는 항목은 종료나 알림을 만들지 않는다.
    assert!(db.inbox_finish("elsewhere", &source.id, 2, Some(&notice)).unwrap().is_none());
    assert!(db.inbox_finish("demo", "missing", 2, Some(&notice)).unwrap().is_none());
    assert_eq!(db.inbox_list(None, true).unwrap().len(), 2);
    // 호출자가 읽은 뒤 바뀐 seen·본문도 보존한다.
    let mut current = source.clone();
    current.body = "갱신한 원문".into();
    db.inbox_put(&current).unwrap();
    db.inbox_mark_seen("demo", &source.id).unwrap();
    let done = db.inbox_finish("demo", &source.id, 3, Some(&notice)).unwrap().unwrap();
    assert!(done.done && done.seen);
    assert_eq!(done.body, "갱신한 원문");
    assert_eq!(done.done_at_ms, Some(3));
    // 처리된 항목의 읽음 표시와 두 번째 종료가 done·종료 시각·알림을 바꾸지 않는다.
    db.inbox_mark_seen("demo", &source.id).unwrap();
    assert!(db.inbox_finish("demo", &source.id, 4, Some(&notice)).unwrap().is_none());
    let all = db.inbox_list(None, true).unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(all.iter().find(|i| i.id == source.id).unwrap(), &done);
    assert_eq!(all.iter().find(|i| i.id == other.id).unwrap(), &other);
    let (sql_done, sql_at): (bool, u64) = db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT done,done_at FROM inbox WHERE id=?1", [&source.id], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(sql_done);
    assert_eq!(sql_at, 3);
}

#[test]
fn notice_failure_rolls_back_the_request_completion() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let source = item("request", "demo", "post request for user");
    let notice = item("request-handled", "demo", "request handled");
    db.inbox_insert(&source).unwrap();
    db.conn
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_handled BEFORE INSERT ON inbox WHEN NEW.event='request handled'
         BEGIN SELECT RAISE(FAIL, '처리 알림 저장 실패 주입'); END;",
        )
        .unwrap();
    let error = db.inbox_finish("demo", &source.id, 2, Some(&notice)).unwrap_err();
    assert!(error.to_string().contains("처리 알림 저장 실패 주입"));
    assert_eq!(db.inbox_list(Some("demo"), false).unwrap(), vec![source]);
    assert_eq!(db.inbox_list(Some("demo"), true).unwrap().len(), 1);
    let (done, at): (bool, Option<u64>) = db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT done,done_at FROM inbox WHERE id='request'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(!done && at.is_none());
}

#[test]
fn same_time_thread_numbers_use_numeric_order_before_the_reply_budget() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    for id in ["t-10000", "t-9999", "t-10001"] {
        let mut record = item(&format!("20261025-thread-state-{id}"), "demo", "new report");
        record.subject = id.into();
        db.inbox_insert(&record).unwrap();
    }
    let all = db.inbox_list(Some("demo"), false).unwrap();
    assert_eq!(all.iter().map(|i| i.subject.as_str()).collect::<Vec<_>>(), ["t-9999", "t-10000", "t-10001"]);
    let budget = serde_json::to_vec(&all[0]).unwrap().len();
    let page = crate::inbox::fit_inbox(all, budget);
    assert_eq!(page.items.iter().map(|i| i.subject.as_str()).collect::<Vec<_>>(), ["t-9999"]);
}
