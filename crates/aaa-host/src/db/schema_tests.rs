use super::*;

// 필요: 묶음 DDL이 실패한 이행을 남기거나 내구 설정을 바꾸지 않게 한다.
#[test]
fn schema_upgrade_commits_once_and_rolls_back_on_failure() {
    let root = std::env::temp_dir().join(format!("aaa-db-schema-{}", aaa_common::util::new_id()));
    std::fs::create_dir(&root).unwrap();
    let path = root.join("host.db");
    let old = Connection::open(&path).unwrap();
    old.execute_batch(
        "CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            INSERT INTO meta VALUES('schema','999');",
    )
    .unwrap();
    assert!(Db::open(&path).is_err(), "모르는 schema는 거부한다");
    assert_eq!(
        old.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='threads'", [], |r| r
            .get::<_, u32>(0))
            .unwrap(),
        0,
        "실패 전의 DDL도 되돌린다"
    );
    old.execute("UPDATE meta SET value=?1 WHERE key='schema'", [SCHEMA_VERSION.to_string()]).unwrap();
    drop(old);
    let db = Db::open(&path).unwrap();
    {
        let conn = db.conn.lock().unwrap();
        assert!(conn.is_autocommit(), "schema transaction은 open 안에서 끝낸다");
        assert_eq!(conn.pragma_query_value(None, "synchronous", |r| r.get::<_, u32>(0)).unwrap(), 2);
        assert_eq!(conn.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0)).unwrap(), "wal");
    }
    db.set_meta("after-open", "durable").unwrap();
    drop(db);
    let reopened = Db::open(&path).unwrap();
    assert_eq!(reopened.meta("after-open").unwrap().as_deref(), Some("durable"));
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

fn schema(conn: &Connection) -> Vec<(String, String, String)> {
    conn.prepare("SELECT type,name,COALESCE(sql,'') FROM sqlite_schema ORDER BY type,name")
        .unwrap()
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

// 필요: 실제 index/ALTER 실패 뒤 기존 스키마·행·정상 재열기를 지킨다.
#[test]
fn schema_failure_rolls_back_create_and_alter_then_preserves_rows_on_reopen() {
    for fault in ["index", "alter"] {
        let root = std::env::temp_dir().join(format!("aaa-schema-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("host.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
                 INSERT INTO meta VALUES('keep','사용자 원문');",
        )
        .unwrap();
        if fault == "index" {
            // 앞선 CREATE 뒤 실제 index SQL이 실패한다. 기존 행은 그 실패와 무관하다.
            conn.execute_batch(
                "CREATE TABLE posts(seq INTEGER PRIMARY KEY,post_id TEXT,sender TEXT,created_ms INTEGER);
                     INSERT INTO posts VALUES(1,'original','worker',1);",
            )
            .unwrap();
        } else {
            // CREATE 묶음은 통과하고 앞선 추가 열 뒤 실제 ALTER SQL이 실패한다.
            conn.execute_batch("CREATE VIEW library_reports AS SELECT '원문' AS live;").unwrap();
        }
        let before = schema(&conn);
        drop(conn);
        let error = Db::open(&path).err().expect("불완전한 기존 스키마를 정상으로 열 수 없다");
        assert!(error.to_string().contains(if fault == "index" { "project" } else { "view" }), "{error:#}");
        let conn = Connection::open(&path).unwrap();
        assert_eq!(schema(&conn), before, "{fault}: 실패한 초기화가 반쪽 스키마를 남겼다");
        assert_eq!(
            conn.query_row("SELECT value FROM meta WHERE key='keep'", [], |row| row.get::<_, String>(0)).unwrap(),
            "사용자 원문"
        );
        if fault == "index" {
            assert_eq!(
                conn.query_row("SELECT post_id FROM posts", [], |row| row.get::<_, String>(0)).unwrap(),
                "original"
            );
            conn.execute("DROP TABLE posts", []).unwrap();
        } else {
            conn.execute("DROP VIEW library_reports", []).unwrap();
        }
        drop(conn);
        let db = Db::open(&path).unwrap();
        assert_eq!(db.meta("keep").unwrap().as_deref(), Some("사용자 원문"));
        let committed = schema(&db.conn.lock().unwrap());
        drop(db);
        let reopened = Db::open(&path).unwrap();
        assert_eq!(schema(&reopened.conn.lock().unwrap()), committed);
        assert_eq!(reopened.meta("keep").unwrap().as_deref(), Some("사용자 원문"));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }
}
