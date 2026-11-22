use super::*;
#[test]
fn rollout_terminal_state_and_inbox_commit_together() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    db.start_rollout("r", "running").unwrap();
    let item = aaa_proto::InboxItem {
        short_id: None,
        id: "rollout-r".into(),
        project: "demo".into(),
        item_kind: "rollout".into(),
        subject: "r".into(),
        event: "rollout finished".into(),
        summary: "배포 완료".into(),
        body: String::new(),
        done: false,
        seen: false,
        created_at_ms: 1,
        done_at_ms: None,
    };
    db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_rollout_inbox BEFORE INSERT ON inbox WHEN NEW.kind='rollout' BEGIN SELECT RAISE(ABORT, 'inbox write failed'); END;").unwrap();
    assert!(
        db.save_rollout_record("r", "done", Some(&item))
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
    );
    assert_eq!(db.meta("rollout:r").unwrap().as_deref(), Some("running"), "inbox 없이 완료만 commit하면 안 된다");
    assert_eq!(db.meta("rollout:notice:r").unwrap(), None);
    db.conn.lock().unwrap().execute_batch("DROP TRIGGER fail_rollout_inbox;").unwrap();
    assert!(db.save_rollout_record("r", "done", Some(&item)).unwrap());
    assert_eq!(db.meta("rollout:r").unwrap().as_deref(), Some("done"));
    assert_eq!(db.inbox_list(Some("demo"), true).unwrap(), [item]);
    assert!(!db.save_rollout_record("r", "done", None).unwrap());
}
