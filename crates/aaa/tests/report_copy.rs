//! 실제 개발 host·CLI·Agent link에서 시스템 복사 오류가 사람 대기가 되지 않는지 본다.
#![cfg(unix)]
mod lab;
use aaa_common::conn::{FrameRx, spawn_writer};
use aaa_proto::*;
use lab::{Lab, wait_for};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_report_agent_keeps_working_and_recovers_without_a_new_hash() {
    let mut lab = Lab::new();
    let projects = lab.root.join("projects");
    let node = lab.node("host", Some(&format!(
        "schema = 1\n[host]\nagent_listen = \"\"\n[web]\nlisten = \"\"\n[local_agent]\nmode = \"none\"\n[projects]\nroot = {:?}\ntick_secs = 1\n[notify]\nweb_push = false\n",
        projects.display().to_string()
    )));
    lab.spawn("host", &node, &["host"]);
    let limit = Duration::from_secs(15);
    wait_for("개발 host", limit, || node.json(&["status"]).ok());
    node.ok(&["project", "create", "demo"]);
    let stream = aaa_common::ipc::connect(&node.state.join("host/agent.sock")).await.unwrap();
    let (reader, writer) = tokio::io::split(stream);
    let tx = spawn_writer(writer, MAX_FRAME);
    let mut rx = FrameRx::new(reader, MAX_FRAME);
    tx.json(&LinkMsg::Hello(AgentHello {
        proto: AGENT_LINK,
        machine_id: "copy-agent".into(),
        machine_name: "mac-a".into(),
        boot_id: "copy-boot".into(),
        os: "test".into(),
        arch: "test".into(),
        software: "test".into(),
        features: vec!["terminal.v1".into(), "adopt.v1".into()],
        dev_mode: true,
        selfcheck: None,
        harness: vec![HarnessEvent {
            session: "s".into(),
            harness: "codex".into(),
            kind: "working".into(),
            at_ms: aaa_common::util::now_ms(),
            harness_session: None,
            message: None,
            question_kind: None,
            percent: None,
            transcript: None,
            plan: None,
            background: None,
            context: None,
        }],
        sessions: vec![SessionInfo {
            id: "s".into(),
            name: "codex".into(),
            argv: vec!["codex".into()],
            cwd: Some("/work".into()),
            pid: None,
            started_at_ms: 0,
            boot_id: "copy-boot".into(),
            cols: 80,
            rows: 24,
            state: SessionState::Running,
            role: Some(Role::Worker),
            label: None,
            harness: Some("codex".into()),
        }],
    }))
    .await
    .unwrap();
    assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Welcome(_)));
    let failing = Arc::new(AtomicBool::new(true));
    let attempts = Arc::new(AtomicUsize::new(0));
    let (fail, count) = (failing.clone(), attempts.clone());
    let sender = tx.clone();
    let fake = tokio::spawn(async move {
        while let Ok(msg) = rx.control::<LinkMsg>().await {
            match msg {
                LinkMsg::Req { id, body, .. } => {
                    let result = match body {
                        AgentRequest::ThreadAdopt(_) => Outcome::Ok(AgentResponse::ThreadAdopted(ThreadAdopted {
                            cwd: "/work".into(),
                            work_dir: "/work/.aaa/demo-t-0001".into(),
                            repo: None,
                            branch: None,
                        })),
                        AgentRequest::ReportCollect { max_library_bytes, .. } => {
                            if max_library_bytes != 0 && {
                                count.fetch_add(1, Ordering::SeqCst);
                                fail.load(Ordering::SeqCst)
                            } {
                                Outcome::Err(ErrorBody::new(
                                    ErrorCode::Internal,
                                    "test.log: 받는 동안 바뀌었다(hash 다름)",
                                ))
                            } else {
                                let report = "[진행 중] 계속 일한다\n".as_bytes().to_vec();
                                use sha2::Digest;
                                Outcome::Ok(AgentResponse::Report(ReportSnapshot {
                                    hash: hex::encode(sha2::Sha256::digest(&report)),
                                    report: B64(report),
                                    files: vec![],
                                    skipped: vec![],
                                }))
                            }
                        }
                        AgentRequest::Screen { .. } => Outcome::Ok(AgentResponse::Screen(ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: vec!["Working".into()],
                            cursor_row: 0,
                            cursor_col: 0,
                            ansi: "Working".into(),
                        })),
                        _ => Outcome::Ok(AgentResponse::Done),
                    };
                    sender.json(&LinkMsg::Res { id, result }).await.unwrap();
                }
                LinkMsg::Ping { seq } => {
                    sender.json(&LinkMsg::Pong { seq }).await.unwrap();
                }
                _ => {}
            }
        }
    });
    let adopted = node.json(&["thread", "adopt", "demo", "--pane", "s", "--title", "보고 회수 시험"]).unwrap();
    assert_eq!(adopted["group"], "working", "{adopted}");
    tx.json(&LinkMsg::Event(AgentEvent::ReportChanged { thread: "demo/t-0001".into(), hash: Some("changed".into()) }))
        .await
        .unwrap();
    let inbox = || node.json(&["inbox", "list", "demo", "--all", "--peek"]).unwrap();
    let warned = wait_for("시스템 복사 경고", limit, || {
        let v = inbox();
        v["items"].as_array().unwrap().iter().any(|i| i["item_kind"] == "report-copy-failed").then_some(v)
    });
    assert_eq!(
        warned["items"].as_array().unwrap().iter().filter(|i| i["item_kind"] == "report-copy-failed").count(),
        1
    );
    // 새 report 변경 사건 없이 다음 ticker가 다시 받는다. 요청 수가 늘어난 뒤 tick으로 음성 단언을 울타리 친다.
    wait_for("ticker 자동 재수집", limit, || (attempts.load(Ordering::SeqCst) >= 4).then_some(()));
    node.ticks(1);
    let v = node.json(&["thread", "show", "demo", "t-0001"]).unwrap();
    assert_eq!(v["group"], "working", "{v}");
    assert_eq!(v["copy"], "failed", "{v}");
    let items = inbox();
    let items = items["items"].as_array().unwrap();
    assert_eq!(items.iter().filter(|i| i["item_kind"] == "report-copy-failed").count(), 1);
    assert!(!items.iter().any(|i| i["event"] == "waiting on you"), "{items:?}");
    failing.store(false, Ordering::SeqCst);
    let copied = wait_for("같은 hash 재수집 성공", limit, || {
        let v = node.json(&["thread", "show", "demo", "t-0001"]).unwrap();
        (v["copy"] == "complete").then_some(v)
    });
    assert_eq!(copied["group"], "working", "{copied}");
    assert!(projects.join("demo/threads/t-0001.md").is_file());
    fake.abort();
}
