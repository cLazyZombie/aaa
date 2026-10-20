//! 삭제된 TUI는 로컬 명령·옛 Front hello에서 명시적으로 거절한다. 실제 CLI·host 경계가 owner다.
mod lab;

use aaa_common::front::FrontClient;
use lab::{Lab, wait_for};
use std::time::Duration;

#[test]
fn local_tui_entries_are_removed_before_connecting() {
    let lab = Lab::new();
    let node = lab.node("client", None);
    assert!(std::fs::read_dir(&node.state).unwrap().next().is_none());
    for args in [vec!["attach"], vec!["tui"], vec!["attach", "--remote", "https://mac-a.example.ts.net:8443"]] {
        let out = node.run(&args);
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{args:?}");
        assert!(message.contains("TUI는 삭제됐다") && message.contains("aaa web status"), "{args:?}: {message}");
    }
    for args in [
        vec!["mission", "list"],
        vec!["mission", "start", "watch"],
        vec!["evaluator", "open", "demo"],
        vec!["evaluation", "list"],
    ] {
        let out = node.run(&args);
        let message = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success() && message.contains("없어짐") && message.contains("aaa automation convert"),
            "{args:?}: {message}"
        );
    }
    assert!(std::fs::read_dir(&node.state).unwrap().next().is_none(), "host 접속·pairing 전에 끝낸다");
}

#[test]
fn host_rejects_legacy_tui_and_keeps_cli_fronts() {
    let mut lab = Lab::new();
    let h = lab.node(
        "host",
        Some("schema = 1\n[host]\nagent_listen = \"\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n"),
    );
    lab.spawn("host", &h, &["host"]);
    wait_for("host 준비", Duration::from_secs(20), || h.json(&["status"]).ok());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        for label in ["tui", "tui@mac-a"] {
            let error = FrontClient::connect_as(&h.state, label).await.err().expect("옛 TUI를 받았다");
            assert!(error.to_string().contains("TUI는 삭제됐다"), "{error:#}");
        }
        let mut cli = FrontClient::connect(&h.state).await.unwrap();
        assert!(cli.call(aaa_proto::FrontRequest::Status).await.unwrap().is_ok());
    });
}
