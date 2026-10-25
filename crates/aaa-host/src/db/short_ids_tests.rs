use super::*;

#[test]
fn all_sequential_ids_cross_9999_durably_and_project_local_aliases_resolve() {
    let root = std::env::temp_dir().join(format!("aaa-id-boundary-{}", aaa_common::util::new_id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("host.db");
    let db = Db::open(&path).unwrap();
    db.thread_import("demo", "t-9998", "resolved", "t-9998").unwrap();
    {
        let c = db.conn.lock().unwrap();
        c.execute("INSERT INTO task_ids VALUES('demo','seed',9998)", []).unwrap();
        c.execute("INSERT INTO automation_ids VALUES('demo','seed',9998)", []).unwrap();
        c.execute("INSERT INTO question_ids(project,seq,id,kind,subject,revision,position) VALUES('demo',9998,'q-9998','thread','seed','v',0)", []).unwrap();
    }
    assert_eq!(db.report_reserve("demo", "seed", "v", 9997).unwrap(), "r-9998");
    for n in [9999, 10000, 10001] {
        let key = format!("key-{n}");
        assert_eq!(db.thread_insert_next("demo", str::to_owned).unwrap(), format!("t-{n}"));
        assert_eq!(db.task_short_id("demo", &key).unwrap(), format!("d-{n}"));
        assert_eq!(db.automation_short_id("demo", &key).unwrap(), format!("a-{n}"));
        assert_eq!(db.question_id("demo", "thread", &key, "v", 0).unwrap(), format!("q-{n}"));
        let report = db.report_reserve("demo", &key, "v", 0).unwrap();
        assert_eq!(report, format!("r-{n}"));
        for alias in [format!("d-{n}"), format!("d-0{n}"), format!("d-00{n}")] {
            assert_eq!(db.task_long_id("demo", &alias).unwrap(), Some(key.clone()));
            assert!(db.task_long_id("other", &alias).unwrap().is_none());
        }
        for alias in [format!("q-{n}"), format!("q-0{n}")] {
            assert_eq!(db.question_target(Some("demo"), &alias).unwrap().unwrap().id, format!("q-{n}"));
            assert!(db.question_target(Some("other"), &alias).unwrap().is_none());
        }
        assert_eq!(db.automation_long_id("demo", n).unwrap(), Some(key));
    }
    assert_eq!(db.threads(Some("demo")).unwrap(), ["t-9998", "t-9999", "t-10000", "t-10001"]);
    assert_eq!(
        db.report_sources().unwrap().into_iter().map(|(_, id)| id).collect::<Vec<_>>(),
        ["t-9998", "t-9999", "t-10000", "t-10001"]
    );
    db.question_id("other", "thread", "first", "v", 0).unwrap();
    assert_eq!(db.task_short_id("other", "first").unwrap(), "d-0001");
    assert_eq!(db.automation_short_id("other", "first").unwrap(), "a-0001");
    assert_eq!(db.report_reserve("other", "first", "v", 0).unwrap(), "r-0001");
    db.question_id("other", "thread", "seed", "v", 0).unwrap();
    // 다른 프로젝트의 같은 q 번호는 프로젝트 생략 조회에서 거부한다.
    db.conn.lock().unwrap().execute("INSERT INTO question_ids(project,seq,id,kind,subject,revision,position) VALUES('other',10000,'q-10000','thread','same','v',0)", []).unwrap();
    assert!(db.question_target(None, "q-010000").is_err());
    drop(db);
    let db = Db::open(&path).unwrap();
    assert_eq!(db.task_short_id("demo", "key-10000").unwrap(), "d-10000");
    assert_eq!(db.thread_insert_next("demo", str::to_owned).unwrap(), "t-10002");
    assert_eq!(db.report_reserve("demo", "later", "v", 0).unwrap(), "r-10002");
    assert_eq!(db.question_id("demo", "thread", "later", "v", 0).unwrap(), "q-10002");
    assert_eq!(db.automation_short_id("demo", "later").unwrap(), "a-10002");
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn post_numbers_keep_eight_digits_and_cross_their_own_width_boundary() {
    let root = std::env::temp_dir().join(format!("aaa-post-boundary-{}", aaa_common::util::new_id()));
    std::fs::create_dir(&root).unwrap();
    let db = Db::open(&root.join("host.db")).unwrap();
    let insert = |n: i64| {
        db.conn.lock().unwrap().execute("INSERT INTO posts(seq,post_id,project,sender,machine,content_sha,created_ms,result) VALUES(?1,?2,'demo','coordinator','mac-a','hash',1,?3)",
            params![n, format!("{n:032x}"), serde_json::to_string(&aaa_proto::PostResult::Accepted { id: format!("p-{n:08}") }).unwrap()]).unwrap();
    };
    for (previous, next) in [(9998, 9999), (9999, 10000), (99999998, 99999999), (99999999, 100000000)] {
        insert(previous);
        assert_eq!(db.post_next_id().unwrap(), format!("p-{next:08}"));
        for alias in [format!("p-{previous}"), format!("p-0{previous}"), format!("p-{previous:08}")] {
            assert!(db.post_by_ref(&alias).unwrap().is_some());
        }
    }
    drop(db);
    std::fs::remove_dir_all(root).unwrap();
}
