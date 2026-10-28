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

// 대량 들여오기에서 뒤 항목이 실패해도 앞 항목의 갱신이 남으면 안 된다.
#[test]
fn import_batch_commits_together_and_rolls_back_updated_rows_on_failure() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let source = item("request", "demo", "new report");
    db.inbox_put(&source).unwrap();
    let mut updated = source.clone();
    updated.done = true;
    updated.done_at_ms = Some(3);
    updated.body = "들여온 본문".into();
    let next = item("next", "demo", "idle");
    db.conn
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER reject_import BEFORE INSERT ON inbox WHEN NEW.id='next'
         BEGIN SELECT RAISE(ABORT, 'inbox import row failure'); END;",
        )
        .unwrap();
    let error = db.inbox_put_many(&[updated.clone(), next.clone()]).unwrap_err();
    assert!(error.to_string().contains("inbox import row failure"));
    assert_eq!(db.inbox_list(Some("demo"), true).unwrap(), [source]);
    let (done, at): (bool, Option<u64>) = db
        .conn
        .lock()
        .unwrap()
        .query_row("SELECT done,done_at FROM inbox WHERE id='request'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap();
    assert!(!done && at.is_none(), "실패한 묶음의 SQL 열도 되돌린다");
    db.conn.lock().unwrap().execute_batch("DROP TRIGGER reject_import").unwrap();
    db.inbox_put_many(&[updated.clone(), next.clone()]).unwrap();
    let all = db.inbox_list(Some("demo"), true).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all.iter().find(|i| i.id == updated.id), Some(&updated));
    assert_eq!(all.iter().find(|i| i.id == next.id), Some(&next));
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

// 구판 자료·파일 키를 바꾸지 않고 한 번만 채우며 prune/재시작/프로젝트 변경에도 같은 번호다.
#[test]
fn inbox_aliases_backfill_without_rewriting_and_survive_prune_restart_and_rename() {
    let root = std::env::temp_dir().join(format!("aaa-inbox-ids-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("host.db");
    let db = Db::open(&path).unwrap();
    // 별칭 도입 전 DB를 만든다. 이행 후 JSON 바이트도 그대로여야 한다.
    db.conn.lock().unwrap().execute_batch("DROP TRIGGER inbox_assign_id; DROP TABLE inbox_ids;").unwrap();
    let mut old = item("old-long-id", "demo", "new report");
    old.done = true;
    old.done_at_ms = Some(2);
    let mut second = item("new-long-id", "demo", "idle");
    second.created_at_ms = 2;
    db.inbox_put_many(&[second.clone(), old.clone()]).unwrap();
    let bytes: String =
        db.conn.lock().unwrap().query_row("SELECT data FROM inbox WHERE id=?1", [&old.id], |r| r.get(0)).unwrap();
    drop(db);
    let db = Db::open(&path).unwrap();
    let mut display = vec![old.clone(), second.clone()];
    db.inbox_display_ids(&mut display).unwrap();
    assert_eq!((&*display[0].id, &*display[1].id), ("i-0001", "i-0002"));
    assert_eq!(
        db.conn
            .lock()
            .unwrap()
            .query_row("SELECT data FROM inbox WHERE id=?1", [&old.id], |r| r.get::<_, String>(0))
            .unwrap(),
        bytes
    );
    for alias in ["i-2", "i-0002", "i-00002"] {
        assert_eq!(db.inbox_long_id("demo", alias).unwrap().as_deref(), Some(second.id.as_str()));
        assert!(db.inbox_long_id("elsewhere", alias).unwrap().is_none());
    }
    assert_eq!(db.inbox_long_id("demo", &old.id).unwrap(), Some(old.id.clone()));
    db.inbox_prune(3).unwrap();
    let third = item("third-long-id", "demo", "idle");
    db.inbox_insert(&third).unwrap();
    assert_eq!(db.inbox_long_id("demo", "i-3").unwrap(), Some(third.id));
    // 프로젝트마다 같은 번호가 있어도 범위를 넘지 않는다.
    let other = item("other-project-long-id", "other", "idle");
    db.inbox_put(&other).unwrap();
    assert_eq!(db.inbox_long_id("other", "i-1").unwrap(), Some(other.id));
    db.project_rename("demo", "renamed", str::to_owned).unwrap();
    assert!(db.inbox_long_id("demo", "i-2").unwrap().is_none());
    assert_eq!(db.inbox_long_id("renamed", "i-2").unwrap(), Some(second.id));
    let mut renamed = db.inbox_list(Some("renamed"), true).unwrap();
    db.inbox_display_ids(&mut renamed).unwrap();
    assert!(renamed.iter().all(|i| i.project == "renamed"));
    assert_eq!(db.project_export("renamed").unwrap()["inbox_ids"].as_array().unwrap().len(), 3);
    db.project_forget("other").unwrap();
    assert_eq!(db.project_export("other").unwrap()["inbox_ids"], serde_json::json!([]));
    // 네 자리 위로 늘어나며 새 열기에서 재배정하지 않는다.
    db.conn.lock().unwrap().execute("INSERT INTO inbox_ids VALUES('renamed','reserved',9999)", []).unwrap();
    db.inbox_insert(&item("rollover", "renamed", "idle")).unwrap();
    drop(db);
    let db = Db::open(&path).unwrap();
    assert_eq!(db.inbox_long_id("renamed", "i-10000").unwrap().as_deref(), Some("rollover"));
    assert_eq!(db.inbox_long_id("renamed", "i-1").unwrap().as_deref(), Some("old-long-id"));
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

// 동시에 넣은 항목과 종료 알림도 원장 한 곳을 쓰고, 실패 transaction은 번호까지 되돌린다.
#[test]
fn inbox_aliases_commit_with_every_insert_and_reject_legacy_collisions() {
    let db = std::sync::Arc::new(Db::open(Path::new(":memory:")).unwrap());
    let gate = std::sync::Arc::new(std::sync::Barrier::new(4));
    let jobs: Vec<_> = (0..4)
        .map(|n| {
            let db = db.clone();
            let gate = gate.clone();
            std::thread::spawn(move || {
                gate.wait();
                db.inbox_insert(&item(&format!("parallel-{n}"), "demo", "idle")).unwrap();
            })
        })
        .collect();
    for job in jobs {
        job.join().unwrap();
    }
    let mut display = db.inbox_list(Some("demo"), true).unwrap();
    db.inbox_display_ids(&mut display).unwrap();
    assert_eq!(display.iter().map(|i| &i.id).collect::<std::collections::HashSet<_>>().len(), 4);
    db.inbox_finish("demo", "parallel-0", 3, Some(&item("notice", "demo", "request handled"))).unwrap();
    assert_eq!(db.inbox_long_id("demo", "i-5").unwrap().as_deref(), Some("notice"));
    db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_alias BEFORE INSERT ON inbox_ids WHEN NEW.item_id='fail' BEGIN SELECT RAISE(ABORT, 'alias failure'); END;").unwrap();
    assert!(db.inbox_put_many(&[item("rollback", "demo", "idle"), item("fail", "demo", "idle")]).is_err());
    assert!(db.inbox_long_id("demo", "rollback").unwrap().is_none());
    assert!(db.inbox_long_id("demo", "i-6").unwrap().is_none());
    db.inbox_insert(&item("i-1", "demo", "idle")).unwrap();
    assert!(db.inbox_long_id("demo", "i-1").unwrap_err().to_string().contains("겹친다"));
    assert_eq!(db.inbox_long_id("demo", "i-6").unwrap().as_deref(), Some("i-1"));
}
