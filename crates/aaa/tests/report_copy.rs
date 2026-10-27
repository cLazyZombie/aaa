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
    report_copy_recovers(false, false, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_library_keeps_the_new_report_and_healthy_files_and_recovers_without_a_new_hash() {
    report_copy_recovers(true, false, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failing_report_history_keeps_the_new_report_and_recovers_without_a_new_hash() {
    report_copy_recovers(false, true, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn adoption_preparation_is_not_a_crashed_launch() {
    report_copy_recovers(false, false, true).await;
}

async fn report_copy_recovers(fail_library: bool, fail_history: bool, interleave_adopt: bool) {
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
    let history_db = rusqlite::Connection::open(node.state.join("host/host.db")).unwrap();
    if fail_history {
        // 격리 DB의 판 공개만 실제 SQLite 오류로 막는다. 본문·thread 저장은 정상이다.
        history_db
            .execute_batch(
                "CREATE TRIGGER fail_history BEFORE UPDATE OF ready ON report_ids WHEN NEW.ready=1
                 BEGIN SELECT RAISE(FAIL, '시험 보고 판 저장 실패'); END;",
            )
            .unwrap();
    }
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
        features: vec!["terminal.v1".into(), "adopt.v1".into(), "report_copy.v2".into()],
        dev_mode: true,
        sha256: None,
        selfcheck: None,
        identity: None,
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
            model_observation: None,
            turn_reply: None,
        }],
        sessions: vec![SessionInfo {
            responsibility: None,
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
    let (adopt_seen, adopt_wait) = tokio::sync::oneshot::channel();
    let (adopt_finish, adopt_resume) = tokio::sync::oneshot::channel();
    let mut pause = interleave_adopt.then_some((adopt_seen, adopt_resume));
    let fake = tokio::spawn(async move {
        while let Ok(msg) = rx.control::<LinkMsg>().await {
            match msg {
                LinkMsg::Req { id, body, .. } => {
                    let result = match body {
                        AgentRequest::ThreadAdopt(_) => {
                            if let Some((seen, resume)) = pause.take() {
                                seen.send(()).unwrap();
                                resume.await.unwrap();
                            }
                            Outcome::Ok(AgentResponse::ThreadAdopted(ThreadAdopted {
                                cwd: "/work".into(),
                                work_dir: "/work/.aaa/demo-t-0001".into(),
                                repo: None,
                                branch: None,
                            }))
                        }
                        request @ (AgentRequest::ReportCollect { .. } | AgentRequest::ReportTransfer { .. }) => {
                            let (transfer, max_library_bytes) = match request {
                                AgentRequest::ReportTransfer { max_library_bytes, .. } => (true, max_library_bytes),
                                AgentRequest::ReportCollect { max_library_bytes, .. } => (false, max_library_bytes),
                                _ => unreachable!(),
                            };
                            let failed = max_library_bytes != 0 && {
                                count.fetch_add(1, Ordering::SeqCst);
                                fail.load(Ordering::SeqCst)
                            };
                            if failed && !fail_library && !fail_history {
                                Outcome::Err(ErrorBody::new(ErrorCode::Internal, "시험 manifest 수집 실패"))
                            } else {
                                let report = "[진행 중] 계속 일한다\n".as_bytes().to_vec();
                                use sha2::Digest;
                                let snapshot = ReportSnapshot {
                                    hash: hex::encode(sha2::Sha256::digest(&report)),
                                    report: B64(report),
                                    files: if fail_library && max_library_bytes != 0 {
                                        [("bad.log", b"expected".as_slice()), ("good.txt", b"healthy".as_slice())]
                                            .into_iter()
                                            .map(|(path, bytes)| LibFile {
                                                path: path.into(),
                                                size: bytes.len() as u64,
                                                sha256: hex::encode(sha2::Sha256::digest(bytes)),
                                            })
                                            .collect()
                                    } else {
                                        vec![]
                                    },
                                    skipped: vec![],
                                };
                                Outcome::Ok(if transfer {
                                    AgentResponse::ReportTransfer(snapshot)
                                } else {
                                    AgentResponse::Report(snapshot)
                                })
                            }
                        }
                        AgentRequest::ReportFileRead { path, .. } => {
                            let bytes = if path == "good.txt" {
                                b"healthy".as_slice()
                            } else if fail.load(Ordering::SeqCst) {
                                b"rewritten".as_slice()
                            } else {
                                b"expected".as_slice()
                            };
                            Outcome::Ok(AgentResponse::ReportFileChunk { data: B64(bytes.to_vec()), eof: true })
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
    let adopt_node = node.clone();
    let adopting = tokio::task::spawn_blocking(move || {
        adopt_node.json(&["thread", "adopt", "demo", "--pane", "s", "--title", "보고 회수 시험"])
    });
    if interleave_adopt {
        tokio::time::timeout(limit, adopt_wait).await.expect("agent의 adopt 준비 요청").unwrap();
        node.ticks(1);
        let preparing = node.json(&["thread", "show", "demo", "t-0001"]).unwrap();
        assert_eq!(preparing["status"], "starting", "실제 adopt 준비를 중단된 기동으로 오인했다: {preparing}");
        adopt_finish.send(()).unwrap();
    }
    let adopted = adopting.await.unwrap().unwrap();
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
    if fail_library || fail_history {
        assert_eq!(
            std::fs::read_to_string(projects.join("demo/threads/t-0001.md")).unwrap(),
            "[진행 중] 계속 일한다\n"
        );
        if fail_library {
            assert_eq!(std::fs::read(projects.join("demo/library/t-0001/good.txt")).unwrap(), b"healthy");
            assert!(!projects.join("demo/library/t-0001/bad.log").exists());
        }
        assert!(v["report_hash"].is_string(), "실패 중에도 새 보고를 반영한다: {v}");
    }
    if fail_history {
        assert!(v["report_id"].is_null(), "실패한 판은 공개하지 않는다: {v}");
        let ready: u64 =
            history_db.query_row("SELECT COUNT(*) FROM report_ids WHERE ready=1", [], |r| r.get(0)).unwrap();
        assert_eq!(ready, 0);
        history_db.execute_batch("DROP TRIGGER fail_history").unwrap();
    }
    failing.store(false, Ordering::SeqCst);
    let copied = wait_for("같은 hash 재수집 성공", limit, || {
        let v = node.json(&["thread", "show", "demo", "t-0001"]).unwrap();
        (v["copy"] == "complete").then_some(v)
    });
    assert_eq!(copied["group"], "working", "{copied}");
    if fail_history {
        assert_eq!(copied["report_hash"], v["report_hash"]);
        let revision = copied["report_id"].as_str().unwrap();
        assert_eq!(
            std::fs::read_to_string(projects.join("demo/threads").join(format!("{revision}.md"))).unwrap(),
            "[진행 중] 계속 일한다\n"
        );
    }
    assert!(projects.join("demo/threads/t-0001.md").is_file());
    if fail_library {
        assert_eq!(std::fs::read(projects.join("demo/library/t-0001/bad.log")).unwrap(), b"expected");
        assert_eq!(copied["report_hash"], v["report_hash"]);
    }
    fake.abort();
}
