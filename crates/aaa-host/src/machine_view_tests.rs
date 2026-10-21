//! 머신 상세(웹 머신 추가 M0, plan §8.372): hello의 CPU 종류·판·SHA·기능·자가 점검이 `MachineView`에 실리고, 앞의 셋은 host를 다시 열어도
//! 남는다. 판이 host와 다르면 표시한다.
use super::*;
use aaa_proto::AgentHello;

fn hello(software: &str, selfcheck: Option<serde_json::Value>) -> AgentHello {
    AgentHello {
        proto: aaa_proto::AGENT_LINK,
        machine_id: "m-b".into(),
        machine_name: "mac-b".into(),
        boot_id: "boot".into(),
        os: "macos".into(),
        arch: "aarch64".into(),
        software: software.into(),
        sha256: Some("ab".repeat(32)),
        features: vec!["terminal.v1".into(), "work_root.manage.v1".into()],
        dev_mode: false,
        selfcheck,
        sessions: vec![],
        harness: vec![],
    }
}

fn open(root: &std::path::Path) -> Arc<Host> {
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    Host::open(cfg, root.join("host")).unwrap()
}

fn view(h: &Host) -> MachineView {
    h.machine_views().into_iter().find(|m| m.name == "mac-b").unwrap()
}

#[test]
fn the_machine_view_carries_the_last_hello_build_and_keeps_it_after_a_host_restart() {
    let root = std::env::temp_dir().join(format!("aaa-machine-view-{}", aaa_common::util::new_id()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        // 등록만 하고 아직 붙지 않은 머신(옛 DB 행 포함): 판을 모른다
        let h = open(&root);
        h.db.upsert_machine("m-b", "mac-b", Some("fp"), false).unwrap();
        Arc::downgrade(&h)
    });
    drop(rt);
    assert!(weak.upgrade().is_none());
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let h = open(&root);
        let before = view(&h);
        assert_eq!((before.arch, before.software, before.sha256), (None, None, None));
        assert!(!before.software_differs && before.features.is_empty() && before.selfcheck.is_none());
        let check = serde_json::json!({ "pass": false, "reasons": ["키체인이 잠겼다"], "fda": false });
        h.agent_connected(
            &hello(&aaa_common::util::software(), Some(check)),
            false,
            crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), aaa_proto::MAX_FRAME), 1),
        );
        let on = view(&h);
        assert_eq!(on.arch.as_deref(), Some("aarch64"));
        assert_eq!(on.software, Some(aaa_common::util::software()));
        assert_eq!(on.sha256, Some("ab".repeat(32)));
        assert!(!on.software_differs, "같은 판");
        assert_eq!(on.features, ["terminal.v1", "work_root.manage.v1"]);
        assert_eq!(
            on.selfcheck,
            Some(aaa_proto::MachineSelfcheck { pass: false, reasons: vec!["키체인이 잠겼다".into()] })
        );
        // 웹이 받는 꼴: 옛 host와 같은 칸 이름에 새 칸만 더한다
        let json = serde_json::to_value(&on).unwrap();
        assert_eq!(json["software_differs"], serde_json::Value::Null, "false는 싣지 않는다");
        assert_eq!(json["selfcheck"]["reasons"][0], "키체인이 잠겼다");
        // 다른 판으로 다시 붙으면 표시한다. 모르는 꼴의 자가 점검은 보이지 않는다
        h.agent_connected(
            &hello("aaa 0.0.1", Some(serde_json::json!({ "ok": true }))),
            false,
            crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), aaa_proto::MAX_FRAME), 2),
        );
        let older = view(&h);
        assert!(older.software_differs);
        assert_eq!(older.selfcheck, None);
        h.agent_disconnected("m-b", 2);
        Arc::downgrade(&h)
    });
    drop(rt);
    assert!(weak.upgrade().is_none(), "runtime을 끝냈는데 Host가 남아 있다(DB가 열린 채다)");
    // host를 다시 열면: 판·SHA·CPU 종류는 DB에서, 기능·자가 점검은 다음 연결 때
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let h = open(&root);
        let after = view(&h);
        assert_eq!(after.connection, aaa_proto::Connection::Disconnected);
        assert_eq!((after.arch.as_deref(), after.software.as_deref()), (Some("aarch64"), Some("aaa 0.0.1")));
        assert_eq!(after.sha256, Some("ab".repeat(32)));
        assert!(after.software_differs);
        assert!(after.features.is_empty() && after.selfcheck.is_none());
        Arc::downgrade(&h)
    });
    drop(rt);
    assert!(weak.upgrade().is_none());
    std::fs::remove_dir_all(root).unwrap();
}
