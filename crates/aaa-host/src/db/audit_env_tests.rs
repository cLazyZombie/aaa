use super::*;

fn bytes_have(dir: &Path, needle: &str) -> Vec<String> {
    ["host.db", "host.db-wal", "host.db-shm"]
        .iter()
        .filter(|f| {
            std::fs::read(dir.join(f))
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes()))
        })
        .map(|f| f.to_string())
        .collect()
}

/// 옛 판이 남긴 pane_run 감사 기록(SpawnSpec 직렬화 전체)을 연 Db가 env 이름만 남기고, 지운 값이 빈 page·WAL에 남지 않게
/// VACUUM한다(plan §8.307). 정리 전 DB 사본으로 시험하고, 다시 열어도 그대로다.
#[test]
fn open_scrubs_old_pane_run_env_values_and_vacuums() {
    let root = std::env::temp_dir().join(format!("aaa-audit-env-{}", std::process::id()));
    let (old, copy) = (root.join("old"), root.join("copy"));
    aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&root), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::create_dir_all(&copy).unwrap();
    let (value, gone) = (&*aaa_test_support::token("audit-env"), &*aaa_test_support::token("deleted-env"));
    let spec = |v: &str| aaa_proto::SpawnSpec {
        name: "demo/t-0001".into(),
        argv: vec!["/bin/sh".into()],
        cwd: None,
        env: vec![("GH_TOKEN".into(), v.into()), ("AAA_THREAD".into(), "t-0001".into())],
        cols: 120,
        rows: 32,
        role: None,
        harness: None,
        profile: Some("api".into()),
        resolved_profile: Some(aaa_proto::ResolvedProfile {
            harness: "claude".into(),
            model: String::new(),
            effort: String::new(),
            args: vec![],
            command: vec![],
            env: [("ANTHROPIC_API_KEY".to_string(), v.to_string())].into(),
        }),
    };
    {
        // 고치기 전 판이 쓰던 그대로: spec 전체를 detail에 둔다. 지운 행의 값도 빈 page에 남아 있다
        let db = Db::open(&old.join("host.db")).unwrap();
        let c = db.conn.lock().unwrap();
        let put = |v: &str| {
            c.execute(
                "INSERT INTO audit(ts,actor,action,detail) VALUES(1,'local','pane_run',?1)",
                [serde_json::to_string(&spec(v)).unwrap()],
            )
            .unwrap()
        };
        put(value);
        put(gone);
        c.execute("DELETE FROM audit WHERE detail LIKE '%' || ?1 || '%'", [gone]).unwrap();
        c.execute("INSERT INTO audit(ts,actor,action,detail) VALUES(2,'local','pane_run','p1 5 bytes')", []).unwrap();
        c.execute("INSERT INTO audit(ts,actor,action,detail) VALUES(3,'local','web_publish','')", []).unwrap();
    }
    for f in ["host.db", "host.db-wal", "host.db-shm"] {
        aaa_common::diagnostic::ignored(
            std::fs::copy(old.join(f), copy.join(f)),
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
    }
    assert!(!bytes_have(&copy, value).is_empty() && !bytes_have(&copy, gone).is_empty(), "정리 전 사본에는 값이 있다");

    let db = Db::open(&copy.join("host.db")).unwrap();
    let rows = || -> Vec<(String, String)> {
        let c = db.conn.lock().unwrap();
        let mut st = c.prepare("SELECT action, detail FROM audit ORDER BY ts").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    };
    let got = rows();
    assert_eq!(got.len(), 3, "{got:?}");
    let v: serde_json::Value = serde_json::from_str(&got[0].1).unwrap();
    assert_eq!(v["env"], serde_json::json!(["GH_TOKEN", "AAA_THREAD"]), "{v}");
    assert_eq!(v["resolved_profile"]["env"], serde_json::json!(["ANTHROPIC_API_KEY"]), "{v}");
    assert_eq!((v["name"].as_str(), v["profile"].as_str()), (Some("demo/t-0001"), Some("api")), "나머지는 그대로");
    assert_eq!(got[1..], [("pane_run".into(), "p1 5 bytes".into()), ("web_publish".into(), String::new())]);
    assert_eq!(db.meta("audit_vacuum").unwrap(), None, "VACUUM을 끝냈다");
    // 열린 채로도 파일(DB·WAL)에 값이 없다: 시작 때 정리를 마쳤다
    assert_eq!(bytes_have(&copy, value), Vec::<String>::new());
    assert_eq!(bytes_have(&copy, gone), Vec::<String>::new());
    drop(db);
    let db = Db::open(&copy.join("host.db")).unwrap();
    let again: Vec<(String, String)> = {
        let c = db.conn.lock().unwrap();
        let mut st = c.prepare("SELECT action, detail FROM audit ORDER BY ts").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
    };
    assert_eq!(again, got, "다시 열어도 그대로");
    drop(db);
    aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&root), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
}
