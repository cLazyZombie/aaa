use super::*;

#[test]
fn input_log_bounds_retention_and_queries_without_message_bodies() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    let now = 40 * 86400 * 1000;
    let mut row = aaa_proto::InputEvent {
        at_ms: now,
        id: "new".into(),
        pane: "p1".into(),
        thread: Some("project/t-1".into()),
        harness: Some("claude".into()),
        working: true,
        event: "received".into(),
        detail: "bytes=7 sha256=abc".into(),
    };
    {
        let c = db.conn.lock().unwrap();
        c.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10001) INSERT INTO input_events(ts,pane,thread,data) SELECT ?1,'p2','project/t-2',?2 FROM n", params![now, serde_json::to_string(&row).unwrap()]).unwrap();
        c.execute("INSERT INTO input_events(ts,pane,data) VALUES(1,'old',?1)", [serde_json::to_string(&row).unwrap()])
            .unwrap();
    }
    db.input_event(&row).unwrap();
    let c = db.conn.lock().unwrap();
    assert_eq!(c.query_row("SELECT count(*) FROM input_events", [], |r| r.get::<_, u64>(0)).unwrap(), 10000);
    assert_eq!(
        c.query_row("SELECT count(*) FROM input_events WHERE pane='old'", [], |r| r.get::<_, u64>(0)).unwrap(),
        0
    );
    drop(c);
    assert_eq!(db.input_events(Some("p1"), Some("t-1"), now).unwrap(), [row.clone()]);
    assert!(db.input_events(Some("p1"), Some("t-2"), 0).unwrap().is_empty());
    assert!(db.input_events(None, None, now + 1).unwrap().is_empty());
    row.at_ms += 1;
    row.id = "later".into();
    db.input_event(&row).unwrap();
    let rows = db.input_events(None, None, 0).unwrap();
    assert_eq!(rows.len(), 1000);
    assert_eq!(rows.last().unwrap(), &row);
    // 1,000자 한국어 상세도 한 줄 3KiB다: 행 수만으로 20MiB를 넘기지 않게 한다.
    let mut large = row.clone();
    large.detail = "가".repeat(1000);
    db.conn
        .lock()
        .unwrap()
        .execute("UPDATE input_events SET data=?1", [serde_json::to_string(&large).unwrap()])
        .unwrap();
    db.input_event(&row).unwrap();
    let c = db.conn.lock().unwrap();
    let (count, bytes): (u64, u64) = c
        .query_row("SELECT count(*),sum(length(CAST(data AS BLOB))) FROM input_events", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert!(count < 10000 && bytes <= 20 * 1024 * 1024);
    drop(c);
    assert_eq!(db.input_events(Some("p1"), Some("t-1"), now + 1).unwrap().last().unwrap(), &row);
}
