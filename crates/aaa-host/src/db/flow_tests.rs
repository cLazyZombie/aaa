use super::*;

/// 웹 알림함 「흐름 사건」(FRONT_API 1.77 `tail`): 가장 최근 N개를 오래된 것부터 보인다
#[test]
fn flow_tail_returns_the_latest_in_order() {
    let db = Db::open(Path::new(":memory:")).unwrap();
    for n in 1..=5u64 {
        db.flow_insert(&aaa_proto::FlowEvent {
            project: "demo".into(),
            seq: 0,
            at_ms: n,
            boot_id: "b".into(),
            kind: format!("k{n}"),
            thread: None,
            repo: None,
            actor: None,
            role: None,
            profile: None,
            machine: None,
            evidence: None,
            detail: String::new(),
        })
        .unwrap();
    }
    let kinds = |v: Vec<aaa_proto::FlowEvent>| v.into_iter().map(|e| e.kind).collect::<Vec<_>>();
    assert_eq!(kinds(db.flow_tail(Some("demo"), 0, 2).unwrap()), ["k4", "k5"]);
    assert_eq!(kinds(db.flow_after(Some("demo"), 0, 2).unwrap()), ["k1", "k2"]);
    assert!(db.flow_tail(Some("other"), 0, 2).unwrap().is_empty());
}
