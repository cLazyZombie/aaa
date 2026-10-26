use super::*;
use aaa_common::conn::{FrameRx, spawn_writer};
use aaa_proto::{AGENT_LINK, AgentHello, Cleanup, LinkMsg, MAX_FRAME, Outcome};
use std::sync::Mutex;

struct Lab {
    h: Arc<Host>,
    root: std::path::PathBuf,
    requests: Arc<Mutex<Vec<usize>>>,
    worker: tokio::task::JoinHandle<()>,
}
impl Lab {
    fn new(count: usize, feature: bool, race: bool, fail_probe: bool) -> Self {
        let root = std::env::temp_dir().join(format!("aaa-sweep-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        // 이력 준비는 한 번 확정한다. 아래 실제 sweep의 저장·경합·실패 경계는 그대로 둔다.
        h.db.conn_for_test().execute_batch("BEGIN").unwrap();
        for i in 0..count {
            let id = format!("t-{:04}", i + 1);
            let mut r = ThreadRec::starting("demo", &id, "m", "fake", aaa_proto::Role::Worker, ThreadKind::Worktree, 1);
            r.id = id.clone();
            r.status = ThreadStatus::Resolved;
            r.repo = Some("/work/repo".into());
            r.cwd = Some(format!("/work/slot-{i}"));
            r.leftover = Some(Leftover { worktree: true, branch: false });
            h.db.thread_import("demo", &id, "resolved", &serde_json::to_string(&r).unwrap()).unwrap();
        }
        h.db.conn_for_test().execute_batch("COMMIT").unwrap();
        let (a, b) = tokio::io::duplex(2 << 20);
        let (ar, aw) = tokio::io::split(a);
        drop(ar);
        let conn = crate::AgentConn::new(spawn_writer(aw, MAX_FRAME), 1);
        let pending = conn.pending.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let host = h.clone();
        let worker = tokio::spawn(async move {
            let mut rx = FrameRx::new(b, MAX_FRAME);
            while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                let reply = match body {
                    AgentRequest::ThreadCleanupProbe { targets, .. } => {
                        observed.lock().unwrap().push(targets.len());
                        if race {
                            host.update_rec("demo", "t-0001", |r| r.status = ThreadStatus::Open).unwrap();
                        }
                        if fail_probe {
                            Outcome::Err(err(ErrorCode::Unavailable, "부재 확인 실패"))
                        } else {
                            Outcome::Ok(AgentResponse::CleanupAbsent {
                                absent: targets.iter().map(|t| t.cwd != format!("/work/slot-{}", count - 1)).collect(),
                            })
                        }
                    }
                    AgentRequest::ThreadCleanup { .. } => {
                        observed.lock().unwrap().push(0);
                        Outcome::Ok(AgentResponse::Cleanup(Cleanup {
                            worktree_removed: true,
                            branch_deleted: true,
                            notes: vec![],
                        }))
                    }
                    other => panic!("예상 밖 agent 요청: {other:?}"),
                };
                if let Some(tx) = pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(reply);
                }
            }
        });
        h.agent_connected(
            &AgentHello {
                proto: AGENT_LINK,
                machine_id: "m".into(),
                machine_name: "mac-a".into(),
                boot_id: "boot".into(),
                os: "test".into(),
                arch: "test".into(),
                software: "test".into(),
                identity: None,
                features: if feature { vec!["cleanup_probe.v1".into()] } else { vec![] },
                dev_mode: true,
                sha256: None,
                selfcheck: None,
                sessions: vec![],
                harness: vec![],
            },
            true,
            conn,
        );
        Self { h, root, requests, worker }
    }
    async fn sweep(&self) -> String {
        let FrontResponse::ProjectChange(r) = self.h.project_sweep("demo", false, "test").await.unwrap() else {
            panic!()
        };
        r.notes.join("\n")
    }

    async fn close(self) {
        let Self { h, root, worker, .. } = self;
        // abort는 종료 요청뿐이다. 가짜 agent가 가진 Host도 놓은 뒤 DB 파일을 지운다.
        worker.abort();
        if let Err(error) = worker.await {
            assert!(error.is_cancelled(), "가짜 agent 종료 실패: {error}");
        }
        drop(h);
        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|error| panic!("시험 폴더 삭제 실패({}): {error}", root.display()));
    }
}

#[tokio::test]
async fn sweep_batches_absence_but_keeps_real_cleanup_and_dry_run() {
    let lab = Lab::new(601, true, false, false);
    lab.h.project_sweep("demo", true, "test").await.unwrap();
    assert!(lab.requests.lock().unwrap().is_empty());
    let notes = lab.sweep().await;
    assert!(notes.contains("601개 치움, 0개 남김"), "{notes}");
    assert_eq!(*lab.requests.lock().unwrap(), [500, 101, 0]);
    for json in lab.h.db.threads(Some("demo")).unwrap() {
        assert_eq!(serde_json::from_str::<ThreadRec>(&json).unwrap().leftover, Some(Leftover::default()));
    }
    lab.close().await;
}

#[tokio::test]
async fn sweep_does_not_send_new_request_to_old_agent_or_hide_probe_failure() {
    let old = Lab::new(2, false, false, false);
    let notes = old.sweep().await;
    assert!(notes.contains("2개 치움, 0개 남김"), "{notes}");
    assert_eq!(*old.requests.lock().unwrap(), [0, 0]);
    old.close().await;
    let failed = Lab::new(2, true, false, true);
    let notes = failed.sweep().await;
    assert!(notes.contains("2개 치움, 0개 남김"), "{notes}");
    assert_eq!(*failed.requests.lock().unwrap(), [2, 0, 0]);
    failed.close().await;
}

#[tokio::test]
async fn sweep_preserves_a_thread_reopened_during_probe() {
    let lab = Lab::new(2, true, true, false);
    let notes = lab.sweep().await;
    assert!(notes.contains("1개 치움, 1개 남김"), "{notes}");
    let reopened = lab.h.load("demo", "t-0001").unwrap();
    assert_eq!(reopened.status, ThreadStatus::Open);
    assert!(reopened.leftover.unwrap().worktree);
    assert_eq!(*lab.requests.lock().unwrap(), [2, 0]);
    lab.close().await;
}

#[tokio::test]
async fn sweep_reports_marker_save_failure_and_preserves_the_record() {
    let lab = Lab::new(2, true, false, false);
    let db = rusqlite::Connection::open(lab.root.join("host/host.db")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_sweep BEFORE UPDATE ON threads BEGIN SELECT RAISE(FAIL, 'fixture save failure'); END;",
    )
    .unwrap();
    let notes = lab.sweep().await;
    assert!(notes.contains("0개 치움, 2개 남김"), "{notes}");
    assert!(notes.contains("정리 표지 저장 실패") && notes.contains("fixture save failure"), "{notes}");
    for id in ["t-0001", "t-0002"] {
        assert!(lab.h.load("demo", id).unwrap().leftover.unwrap().worktree);
    }
    drop(db);
    lab.close().await;
}
