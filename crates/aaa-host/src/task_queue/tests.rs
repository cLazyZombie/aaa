use super::*;
use crate::orch::ThreadRec;
use aaa_proto::{Role, TaskAction, ThreadKind};

struct Lab {
    h: Arc<Host>,
    root: std::path::PathBuf,
}
impl Lab {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-queue-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("demo", Default::default(), "local:cli").unwrap();
        Self { h, root }
    }
    fn text(&self) -> String {
        std::fs::read_to_string(self.h.project_dir("demo").unwrap().join("TASKS.md")).unwrap()
    }
    fn write(&self, text: &str) {
        std::fs::write(self.h.project_dir("demo").unwrap().join("TASKS.md"), text).unwrap();
    }
    fn rec() -> ThreadRec {
        ThreadRec::starting("demo", "일", "m", "claude", Role::Worker, ThreadKind::Dir, 1)
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn identity_preserves_other_bytes_and_survives_edits() {
    let source = "# 메모\r\n\r\n## Backlog\r\n- [ ] 일 (claude@mac-a) [작업 폴더]\r\n  설명 ![그림](uploads/example.png)\r\n\r\n끝";
    let got = identify(source).unwrap();
    assert_eq!(
        got.lines().filter(|l| !l.contains("<!-- aaa-task:")).collect::<Vec<_>>(),
        source.lines().filter(|l| !l.starts_with("- [ ]")).collect::<Vec<_>>()
    );
    assert_eq!(got.matches("\r\n").count(), source.matches("\r\n").count());
    assert_eq!(identify(&got).unwrap(), got);
    let t = tasks::parse(&got).remove(0);
    assert!(t.id.is_some());
    assert_eq!(t.title, "일");
    assert_eq!(t.owner.as_deref(), Some("claude@mac-a"));
    let changed = crate::task_edit::apply(
        &got,
        &[TaskOp::Update {
            at: at(&t),
            title: Some("이름 변경".into()),
            owner: None,
            person: false,
            profile: None,
            machine: None,
            notes: None,
            thread: None,
            done: None,
            repo: None,
        }],
        &empty_ctx(),
    )
    .unwrap();
    assert_eq!(tasks::parse(&changed.text)[0].id, t.id);
    let line = got.lines().find(|l| l.starts_with("- [ ]")).unwrap();
    assert!(identify(&format!("{line}\n{line}\n")).is_err());
}

#[test]
fn selected_fields_reject_coordinator_and_allow_user_actor() {
    let lab = Lab::new();
    let old = "- [ ] 일 (claude@mac-a) [작업 폴더]\n";
    for new in [
        "- [ ] 일 (codex@mac-a) [작업 폴더]\n",
        "- [ ] 일 (claude@mac-b) [작업 폴더]\n",
        "- [ ] 이름 변경 (codex@mac-a) [작업 폴더]\n",
        "- [ ] 일 [작업 폴더]\n",
    ] {
        assert_eq!(lab.h.protect_task_choices(old, new, "coordinator:p").unwrap_err().code, ErrorCode::Forbidden);
        assert!(lab.h.protect_task_choices(old, new, "device:user").is_ok());
    }
    assert!(lab.h.protect_task_choices(old, &format!("{old}  메모\n"), "coordinator:p").is_ok());
}

#[test]
fn simultaneous_claims_enforce_six_slots_and_resolve_pending() {
    let lab = Lab::new();
    let db = &lab.h.db;
    let gate = std::sync::Barrier::new(12);
    let accepted = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..12)
            .map(|_| {
                scope.spawn(|| {
                    gate.wait();
                    db.thread_insert_admitted("demo", None, None, Some(6), None, |id| {
                        let mut r = Lab::rec();
                        r.id = id.into();
                        serde_json::to_string(&r).unwrap()
                    })
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).filter(Result::is_ok).count()
    });
    assert_eq!(accepted, 6);
    assert_eq!(lab.h.open_worker_threads("demo"), 6);
    let first = lab.h.db.threads(Some("demo")).unwrap()[0].clone();
    let mut r: ThreadRec = serde_json::from_str(&first).unwrap();
    r.status = ThreadStatus::Resolved;
    r.resolve_pending = Some(Default::default());
    lab.h.db.thread_save("demo", &r.id, "resolved", &serde_json::to_string(&r).unwrap()).unwrap();
    let insert = || {
        db.thread_insert_admitted("demo", None, None, Some(6), None, |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            serde_json::to_string(&r).unwrap()
        })
    };
    assert!(insert().is_err());
    assert_eq!(lab.h.open_worker_threads("demo"), 6);
    r.resolve_pending = None;
    lab.h.db.thread_save("demo", &r.id, "resolved", &serde_json::to_string(&r).unwrap()).unwrap();
    assert!(insert().is_ok());
}

#[tokio::test]
async fn reservation_edits_require_new_approval_and_cancel_survives_reopen() {
    let lab = Lab::new();
    lab.write("## Backlog\n- [ ] 일 (claude@mac-offline) [작업 폴더]\n  설명과 완료 범위\n");
    let t = lab.h.task_board("demo").unwrap().tasks.remove(0);
    lab.h
        .task_queue(
            "demo",
            QueueOp::Reserve {
                task: t.title,
                block: t.block_sha256,
                profile: "claude".into(),
                profile_revision: model_selection(&lab.h, "claude").unwrap().2,
            },
            "device:user",
        )
        .await
        .unwrap();
    let row = lab.h.db.queue_list("demo").unwrap().remove(0);
    assert_eq!(row.state, S::Queued);
    lab.h.queue_pass("demo").await.unwrap();
    assert!(lab.h.db.threads(Some("demo")).unwrap().is_empty());
    lab.write(&lab.text().replace("설명과 완료 범위", "다른 일"));
    lab.h.queue_pass("demo").await.unwrap();
    assert_eq!(lab.h.db.queue_list("demo").unwrap()[0].state, S::Check);
    lab.h.task_queue("demo", QueueOp::Cancel { task_id: row.task_id.clone() }, "device:user").await.unwrap();
    let reopened = Host::open(lab.h.cfg.clone(), lab.h.state.clone()).unwrap();
    assert_eq!(reopened.db.queue_list("demo").unwrap()[0].state, S::Cancelled);
    assert_eq!(lab.h.db.queue_list("demo").unwrap()[0].state, S::Cancelled);
    assert!(lab.h.db.threads(Some("demo")).unwrap().is_empty());
}

#[tokio::test]
async fn archive_write_failure_preserves_active_then_restore_keeps_id_without_queue() {
    let lab = Lab::new();
    lab.write("## Backlog\n- [ ] 일 [작업 폴더] · t-0001\n  완료 범위와 [문서](uploads/example.md) [근거](library/evidence/notes.md)\n");
    lab.h.task_identify("demo").unwrap();
    let original = lab.text();
    let t = tasks::parse(&original).remove(0);
    let id = lab
        .h
        .db
        .thread_insert_next("demo", |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            r.status = ThreadStatus::Resolved;
            r.task_clean.why = Some("merged".into());
            serde_json::to_string(&r).unwrap()
        })
        .unwrap();
    lab.h.update_rec("demo", &id, |r| r.status = ThreadStatus::Resolved).unwrap();
    // 아카이브 저장 경계의 실제 SQLite 쓰기 실패. 원래 활성 파일/완료 판정을 보존한다.
    lab.h.db.test_archive_fault(true);
    lab.h.task_clean_pass("demo", &lab.h.project_dir("demo").unwrap());
    assert_eq!(lab.text(), original);
    assert!(lab.h.db.archive_list("demo").unwrap().is_empty());
    assert!(lab.h.load("demo", &id).unwrap().task_clean.judged_ms.is_none());
    lab.h.db.test_archive_fault(false);
    lab.h.task_clean_pass("demo", &lab.h.project_dir("demo").unwrap());
    assert!(tasks::parse(&lab.text()).is_empty());
    let a = lab.h.db.archive_list("demo").unwrap().remove(0);
    assert_eq!(a.task_id, t.id.unwrap());
    assert!(a.body.contains("uploads/example.md"));
    assert_eq!(a.reason, "PR 병합");
    assert_eq!(a.report.as_deref(), Some("threads/t-0001.md"));
    // 완료 결과와 메모가 참조한 보고서의 실제 파일은 정리/삭제에서 보호한다.
    let dir = lab.h.project_dir("demo").unwrap();
    for name in ["t-0001", "evidence", "unrelated"] {
        let copy = dir.join("library").join(name);
        std::fs::create_dir_all(&copy).unwrap();
        std::fs::write(copy.join("notes.md"), "결과").unwrap();
        let files = vec![aaa_proto::LibraryFile {
            path: "notes.md".into(),
            size: 6,
            on_agent: false,
            mime: "text/markdown".into(),
            kind: aaa_proto::FileKind::Text,
        }];
        std::fs::write(crate::library::manifest_path(&dir, name), serde_json::to_vec(&files).unwrap()).unwrap();
    }
    let future = aaa_common::util::now_ms() + 3 * 86_400_000;
    let result = lab.h.library_prune_at(Some("demo"), 1, future, "device:user").unwrap();
    assert!(matches!(result, FrontResponse::LibraryPruned { removed } if removed == ["demo/unrelated"]));
    for name in ["t-0001", "evidence"] {
        assert_eq!(std::fs::read_to_string(dir.join("library").join(name).join("notes.md")).unwrap(), "결과");
        assert!(
            lab.h
                .library_delete(&[aaa_proto::LibraryRef { project: "demo".into(), name: name.into() }], "device:user")
                .is_err()
        );
    }
    lab.h.db.task_log_prune("demo", u64::MAX / 2).unwrap();
    assert_eq!(lab.h.db.archive_list("demo").unwrap()[0].task_id, a.task_id);
    lab.h.task_archive("demo", Some(&a.task_id), "device:user").await.unwrap();
    let restored = tasks::parse(&lab.text()).remove(0);
    assert_eq!(restored.id.as_deref(), Some(a.task_id.as_str()));
    assert!(restored.thread.is_none());
    assert!(lab.h.db.queue_list("demo").unwrap().is_empty());
    assert!(
        matches!(lab.h.library_prune_at(Some("demo"), 1, future, "device:user").unwrap(), FrontResponse::LibraryPruned { removed } if removed.is_empty())
    );
}

#[tokio::test]
async fn archive_recompletion_preserves_old_result_after_history_prune() {
    let lab = Lab::new();
    let complete_thread = || {
        lab.h
            .db
            .thread_insert_next("demo", |id| {
                let mut r = Lab::rec();
                r.id = id.into();
                r.status = ThreadStatus::Resolved;
                r.task_clean.why = Some("merged".into());
                serde_json::to_string(&r).unwrap()
            })
            .unwrap()
    };
    let first = complete_thread();
    lab.write(&format!("## Backlog\n- [ ] 일 [작업 폴더] · {first}\n  [옛 근거](library/evidence/notes.md)\n"));
    lab.h.task_clean_pass("demo", &lab.h.project_dir("demo").unwrap());
    let old = lab.h.db.archive_list("demo").unwrap().remove(0);
    lab.h.db.task_log_prune("demo", u64::MAX / 2).unwrap();
    lab.h.task_archive("demo", Some(&old.task_id), "device:user").await.unwrap();
    let next = complete_thread();
    lab.write(&format!(
        "## Backlog\n- [ ] 새 이름 [작업 폴더] · {next} <!-- aaa-task:{} -->\n  새 완료 범위\n",
        old.task_id
    ));
    lab.h.task_clean_pass("demo", &lab.h.project_dir("demo").unwrap());
    let again = lab.h.db.archive_list("demo").unwrap().remove(0);
    assert_eq!(again.task_id, old.task_id);
    assert_eq!(again.task.title, "새 이름");
    assert!(again.history.iter().any(|h| h.action == TaskAction::Completed
        && h.before.as_deref() == Some(old.body.as_str())
        && h.thread.as_deref() == Some(first.as_str())));
    for name in [first.as_str(), "evidence"] {
        let report: aaa_proto::LibraryReport = serde_json::from_value(serde_json::json!({"project":"demo","name":name,"size":1,"updated_at_ms":0,"live":false,"problem":null,"link":null})).unwrap();
        assert!(lab.h.protected_copy(&report));
    }
}

async fn one_reservation(lab: &Lab) -> TaskReservation {
    lab.write("## Backlog\n- [ ] 예약 (claude@mac-offline) [작업 폴더]\n  독립 설명과 완료 범위\n");
    let t = lab.h.task_board("demo").unwrap().tasks.remove(0);
    lab.h
        .task_queue(
            "demo",
            QueueOp::Reserve {
                task: t.title,
                block: t.block_sha256,
                profile: "claude".into(),
                profile_revision: model_selection(&lab.h, "claude").unwrap().2,
            },
            "device:user",
        )
        .await
        .unwrap();
    lab.h.db.queue_list("demo").unwrap().remove(0)
}

#[tokio::test]
async fn slot_released_during_queue_scan_does_not_bypass_the_moved_front() {
    let lab = Lab::new();
    lab.write("## Backlog\n- [ ] 먼저 만든 예약 (claude@mac-offline) [작업 폴더]\n  독립 설명과 완료 범위\n- [ ] 맨 앞으로 옮길 예약 (claude@mac-offline) [작업 폴더]\n  독립 설명과 완료 범위\n");
    for title in ["먼저 만든 예약", "맨 앞으로 옮길 예약"] {
        let t = lab.h.task_board("demo").unwrap().tasks.into_iter().find(|t| t.title == title).unwrap();
        lab.h
            .task_queue(
                "demo",
                QueueOp::Reserve {
                    task: t.title,
                    block: t.block_sha256,
                    profile: "claude".into(),
                    profile_revision: model_selection(&lab.h, "claude").unwrap().2,
                },
                "device:user",
            )
            .await
            .unwrap();
    }
    let moved = lab.h.db.queue_list("demo").unwrap()[1].task_id.clone();
    lab.h.task_queue("demo", QueueOp::Move { task_id: moved.clone(), position: 1 }, "device:user").await.unwrap();
    let mut releasing = String::new();
    for _ in 0..lab.h.settings("demo").max_parallel_threads {
        releasing = lab
            .h
            .db
            .thread_insert_next("demo", |id| {
                let mut r = Lab::rec();
                r.id = id.into();
                r.status = ThreadStatus::Open;
                serde_json::to_string(&r).unwrap()
            })
            .unwrap();
    }
    lab.h
        .db
        .queue_change("demo", "host", |rows| {
            for r in rows {
                r.reason = "자리 판정 전".into();
            }
            Ok(())
        })
        .unwrap();
    lab.h.db.test_queue_release_on_wait("demo", &moved, &releasing);
    // 앞 예약은 자리가 없다고 판단했다. 바로 그 기록 경계에서 resolve가 자리를 돌려준다.
    // 같은 순회의 뒤 예약만 머신 선택/기동으로 넘어가면 앞 차례를 추월한 것이다.
    lab.h.queue_pass("demo").await.unwrap();
    assert_eq!(lab.h.load("demo", &releasing).unwrap().status, ThreadStatus::Resolved);
    let rows = lab.h.db.queue_list("demo").unwrap();
    assert_eq!(rows[0].task_id, moved);
    assert!(rows.iter().all(|r| r.state == S::Queued && r.reason == "자리 대기"), "{rows:#?}");
    // 다음 순회는 새 자리를 앞에서부터 다시 판정한다. 끊긴 머신의 예약은 계속 건너뛸 수 있다.
    lab.h.queue_pass("demo").await.unwrap();
    assert!(lab.h.db.queue_list("demo").unwrap().iter().all(|r| r.reason.contains("머신 연결")));
}

#[tokio::test]
async fn claim_and_cancel_race_has_one_persistent_result() {
    let lab = Lab::new();
    let row = one_reservation(&lab).await;
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let claim = scope.spawn(|| {
            barrier.wait();
            lab.h.db.thread_insert_admitted(
                "demo",
                None,
                None,
                Some(1),
                Some((&row.task_id, &row.approved_block)),
                |id| {
                    let mut r = Lab::rec();
                    r.id = id.into();
                    serde_json::to_string(&r).unwrap()
                },
            )
        });
        let cancel = scope.spawn(|| {
            barrier.wait();
            lab.h
                .db
                .queue_change("demo", "device:user", |rows| {
                    let r = &mut rows[0];
                    if r.state == S::Queued {
                        r.state = S::Cancelled;
                    }
                    Ok(())
                })
                .unwrap();
        });
        let result = claim.join().unwrap();
        cancel.join().unwrap();
        let q = lab.h.db.queue_list("demo").unwrap().remove(0);
        match q.state {
            S::Cancelled => {
                assert!(result.is_err());
                assert!(lab.h.db.threads(Some("demo")).unwrap().is_empty());
            }
            S::Starting => {
                assert_eq!(q.thread.as_deref(), Some(result.unwrap().0.as_str()));
                assert_eq!(lab.h.db.threads(Some("demo")).unwrap().len(), 1);
            }
            s => panic!("경주 결과가 아니다: {s:?}"),
        }
    });
}

#[tokio::test]
async fn lost_launch_reply_reconciles_only_exact_machine_name_and_cwd() {
    let lab = Lab::new();
    let row = one_reservation(&lab).await;
    let (tid, _) = lab
        .h
        .db
        .thread_insert_admitted("demo", None, None, Some(1), Some((&row.task_id, &row.approved_block)), |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            r.cwd = Some("/work/owned".into());
            r.launch_sent = true;
            serde_json::to_string(&r).unwrap()
        })
        .unwrap();
    let recovered = Host::open(lab.h.cfg.clone(), lab.h.state.clone()).unwrap();
    recovered.queue_pass("demo").await.unwrap();
    assert_eq!(recovered.load("demo", &tid).unwrap().status, ThreadStatus::Starting);
    assert_eq!(recovered.open_worker_threads("demo"), 1);
    let info = aaa_proto::SessionInfo {
        id: "wrong".into(),
        name: format!("demo/{tid}"),
        argv: vec!["fake".into()],
        cwd: Some("/work/other".into()),
        pid: None,
        started_at_ms: 1,
        boot_id: "boot".into(),
        cols: 120,
        rows: 32,
        state: aaa_proto::SessionState::Running,
        role: Some(Role::Worker),
        label: None,
        harness: None,
    };
    recovered.agent_connected(
        &aaa_proto::AgentHello {
            proto: aaa_proto::AGENT_LINK,
            machine_id: "m".into(),
            machine_name: "mac-a".into(),
            boot_id: "boot".into(),
            os: "test".into(),
            arch: "test".into(),
            software: "test".into(),
            features: vec![],
            dev_mode: false,
            sha256: None,
            selfcheck: None,
            sessions: vec![info.clone()],
            harness: vec![],
        },
        false,
        crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), aaa_proto::MAX_FRAME), 1),
    );
    recovered.queue_pass("demo").await.unwrap();
    assert!(recovered.load("demo", &tid).unwrap().pane.is_none());
    let mut right = info;
    right.id = "owned-pane".into();
    right.cwd = Some("/work/owned".into());
    recovered.on_session("m", right, false);
    recovered.queue_pass("demo").await.unwrap();
    let r = recovered.load("demo", &tid).unwrap();
    assert_eq!(r.pane.as_deref(), Some("owned-pane"));
    assert_eq!(r.status, ThreadStatus::Open);
    assert_eq!(recovered.db.threads(Some("demo")).unwrap().len(), 1);
    let q = recovered.db.queue_list("demo").unwrap().remove(0);
    assert_eq!(q.thread.as_deref(), Some(tid.as_str()));
    assert_eq!(q.state, S::Running);
    let repeated = recovered
        .task_queue(
            "demo",
            QueueOp::Reserve {
                task: row.task.title,
                block: row.approved_block,
                profile: row.profile,
                profile_revision: row.profile_revision,
            },
            "device:user",
        )
        .await
        .unwrap();
    let FrontResponse::TaskQueue { entries } = repeated else { panic!("다른 응답") };
    assert_eq!(entries[0].thread.as_deref(), Some(tid.as_str()));
    recovered.task_queue("demo", QueueOp::Cancel { task_id: row.task_id }, "device:user").await.unwrap();
    assert_eq!(recovered.load("demo", &tid).unwrap().pane.as_deref(), Some("owned-pane"));
}

#[tokio::test]
async fn coordinator_raw_save_cannot_rename_away_a_selection() {
    let lab = Lab::new();
    lab.write("## Backlog\n- [ ] 사용자 선택 (claude@mac-a)\n- [ ] 다른 일\n");
    let board = lab.h.task_board("demo").unwrap();
    let changed = board.text.replace("사용자 선택 (claude@mac-a)", "이름 변경 (codex@mac-b)");
    let e = lab
        .h
        .task_edit(
            "demo",
            vec![TaskOp::RawSave { expected_sha256: board.sha256, text: changed }],
            None,
            "coordinator:verified-pane",
        )
        .await
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert_eq!(lab.text(), board.text);
}

#[tokio::test]
async fn direct_pre_send_interruption_releases_only_after_the_job_is_gone() {
    let lab = Lab::new();
    let id = lab
        .h
        .db
        .thread_insert_next("demo", |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            serde_json::to_string(&r).unwrap()
        })
        .unwrap();
    let guard = lab.h.claim_thread("demo", &id).unwrap();
    lab.h.recover_launches("demo");
    assert_eq!(lab.h.load("demo", &id).unwrap().status, ThreadStatus::Starting);
    drop(guard);
    lab.h.recover_launches("demo");
    assert_eq!(lab.h.load("demo", &id).unwrap().status, ThreadStatus::Failed);
    assert_eq!(lab.h.open_worker_threads("demo"), 0);
    let old = lab
        .h
        .db
        .thread_insert_next("demo", |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            let mut value = serde_json::to_value(&r).unwrap();
            value.as_object_mut().unwrap().remove("launch_sent");
            value.to_string()
        })
        .unwrap();
    lab.h.recover_launches("demo");
    let r = lab.h.load("demo", &old).unwrap();
    assert!(r.launch_sent, "옛 기록은 송신 여부를 모른다");
    assert_eq!(r.status, ThreadStatus::Starting);
    assert_eq!(lab.h.open_worker_threads("demo"), 1);
}

#[tokio::test]
async fn stale_model_approval_cannot_migrate_or_unlink_task() {
    let lab = Lab::new();
    lab.write("## Backlog\n- [ ] 예약 (claude@mac-offline) [작업 폴더]\n  독립 설명과 완료 범위\n");
    let t = lab.h.task_board("demo").unwrap().tasks.remove(0);
    let before = lab.text();
    let error = lab
        .h
        .task_queue(
            "demo",
            QueueOp::Reserve {
                task: t.title,
                block: t.block_sha256,
                profile: "claude".into(),
                profile_revision: "stale".into(),
            },
            "device:user",
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(lab.text(), before);
    assert!(lab.h.db.queue_list("demo").unwrap().is_empty());
}

#[tokio::test]
async fn archive_recovers_the_file_removed_before_final_db_receipt() {
    let lab = Lab::new();
    lab.write("## Backlog\n- [ ] 보관 경계 [작업 폴더] · t-0001\n  독립 설명과 완료 범위\n");
    lab.h.task_identify("demo").unwrap();
    let id = lab
        .h
        .db
        .thread_insert_next("demo", |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            r.status = ThreadStatus::Resolved;
            r.task_clean.why = Some("merged".into());
            serde_json::to_string(&r).unwrap()
        })
        .unwrap();
    lab.h.update_rec("demo", &id, |r| r.status = ThreadStatus::Resolved).unwrap();
    lab.h.db.test_archive_finalize_fault(true);
    lab.h.task_clean_pass("demo", &lab.h.project_dir("demo").unwrap());
    assert!(tasks::parse(&lab.text()).is_empty());
    let pending = lab.h.db.archive_list("demo").unwrap().remove(0);
    assert!(!pending.committed);
    assert!(pending.body.contains("보관 경계"));
    assert!(lab.h.load("demo", &id).unwrap().task_clean.judged_ms.is_none());
    lab.h.db.test_archive_finalize_fault(false);
    let recovered = Host::open(lab.h.cfg.clone(), lab.h.state.clone()).unwrap();
    let response = recovered.task_archive("demo", None, "device:user").await.unwrap();
    let FrontResponse::TaskArchive { entries } = response else { panic!("다른 응답") };
    assert_eq!(entries.len(), 1);
    assert!(entries[0].committed);
    assert_eq!(entries[0].task_id, pending.task_id);
    recovered.task_archive("demo", Some(&pending.task_id), "device:user").await.unwrap();
    let t = tasks::parse(&lab.text()).remove(0);
    assert_eq!(t.id.as_deref(), Some(pending.task_id.as_str()));
    assert!(t.thread.is_none());
}

#[tokio::test]
async fn cancelling_a_failed_launch_does_not_turn_back_into_check_on_tick() {
    let lab = Lab::new();
    let row = one_reservation(&lab).await;
    let (id, _) = lab
        .h
        .db
        .thread_insert_admitted("demo", None, None, Some(1), Some((&row.task_id, &row.approved_block)), |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            serde_json::to_string(&r).unwrap()
        })
        .unwrap();
    lab.h
        .update_rec("demo", &id, |r| {
            r.status = ThreadStatus::Failed;
            r.error = Some("명확한 기동 실패".into());
        })
        .unwrap();
    lab.h.queue_pass("demo").await.unwrap();
    assert_eq!(lab.h.db.queue_list("demo").unwrap()[0].state, S::Check);
    lab.h.task_queue("demo", QueueOp::Cancel { task_id: row.task_id }, "device:user").await.unwrap();
    lab.h.queue_pass("demo").await.unwrap();
    assert_eq!(lab.h.db.queue_list("demo").unwrap()[0].state, S::Cancelled);
    assert_eq!(lab.h.db.threads(Some("demo")).unwrap().len(), 1);
}

#[tokio::test]
async fn approved_default_model_is_protected_even_while_the_task_field_is_empty() {
    let lab = Lab::new();
    lab.write("## Backlog\n- [ ] 기본 선택 [작업 폴더]\n  독립 설명과 완료 범위\n");
    let t = lab.h.task_board("demo").unwrap().tasks.remove(0);
    lab.h
        .task_queue(
            "demo",
            QueueOp::Reserve {
                task: t.title,
                block: t.block_sha256,
                profile: "claude".into(),
                profile_revision: model_selection(&lab.h, "claude").unwrap().2,
            },
            "device:user",
        )
        .await
        .unwrap();
    let before = lab.text();
    let t = tasks::parse(&before).remove(0);
    let op = TaskOp::Update {
        at: at(&t),
        title: None,
        owner: None,
        person: false,
        profile: Some("codex".into()),
        machine: None,
        notes: None,
        thread: None,
        done: None,
        repo: None,
    };
    let error = lab
        .h
        .task_write(
            "demo",
            &lab.h.project_dir("demo").unwrap(),
            &[op],
            None,
            "coordinator:verified-pane",
            &empty_ctx(),
            false,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(lab.text(), before);
    assert_eq!(lab.h.db.queue_list("demo").unwrap()[0].profile, "claude");
}

#[tokio::test]
async fn raw_selection_rewrite_cannot_be_reapproved_by_coordinator_but_user_edit_can() {
    let lab = Lab::new();
    let row = one_reservation(&lab).await;
    let original = lab.text();
    lab.write(&original.replace("claude@mac-offline", "codex@mac-offline"));
    let changed = lab.h.task_board("demo").unwrap().tasks.remove(0);
    let error = lab
        .h
        .task_queue(
            "demo",
            QueueOp::Reserve {
                task: changed.title,
                block: changed.block_sha256,
                profile: "codex".into(),
                profile_revision: model_selection(&lab.h, "codex").unwrap().2,
            },
            "coordinator:verified-pane",
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Forbidden);
    assert_eq!(lab.h.db.queue_list("demo").unwrap()[0].profile, "claude");
    lab.write(&original);
    let t = tasks::parse(&original).remove(0);
    let op = TaskOp::Update {
        at: at(&t),
        title: None,
        owner: None,
        person: false,
        profile: Some("codex".into()),
        machine: None,
        notes: None,
        thread: None,
        done: None,
        repo: None,
    };
    lab.h
        .task_write("demo", &lab.h.project_dir("demo").unwrap(), &[op], None, "device:user", &empty_ctx(), false)
        .unwrap();
    let proof = lab.h.db.queue_list("demo").unwrap().remove(0);
    assert_eq!(proof.selection.actor, "device:user");
    assert_eq!(proof.selection.profile, "codex");
    assert_eq!(proof.profile, "claude", "선택 기록만 바뀌며 과거 실행 승인은 바뀌지 않는다");
    lab.h.queue_pass("demo").await.unwrap();
    assert_eq!(lab.h.db.queue_list("demo").unwrap()[0].state, S::Check);
    let current = lab.h.task_board("demo").unwrap().tasks.remove(0);
    lab.h
        .task_queue(
            "demo",
            QueueOp::Reserve {
                task: current.title,
                block: current.block_sha256,
                profile: "codex".into(),
                profile_revision: model_selection(&lab.h, "codex").unwrap().2,
            },
            "coordinator:verified-pane",
        )
        .await
        .unwrap();
    let updated = lab.h.db.queue_list("demo").unwrap().remove(0);
    assert_eq!(updated.task_id, row.task_id);
    assert_eq!(updated.profile, "codex");
    assert_eq!(updated.selection.actor, "device:user");
    assert_eq!(updated.state, S::Queued);
}

#[tokio::test]
async fn launch_errors_keep_unknown_and_existing_slots_but_release_known_new_failures() {
    let lab = Lab::new();
    let id = lab
        .h
        .db
        .thread_insert_next("demo", |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            serde_json::to_string(&r).unwrap()
        })
        .unwrap();
    for (sent, code, held, status) in [
        (false, ErrorCode::AgentDisconnected, false, ThreadStatus::Failed),
        (true, ErrorCode::AgentDisconnected, false, ThreadStatus::Starting),
        (true, ErrorCode::Timeout, false, ThreadStatus::Starting),
        (true, ErrorCode::BadRequest, false, ThreadStatus::Failed),
        (false, ErrorCode::BadRequest, true, ThreadStatus::Open),
    ] {
        lab.h
            .update_rec("demo", &id, |r| {
                r.status = ThreadStatus::Starting;
                r.launch_sent = sent;
            })
            .unwrap();
        let rec = lab.h.note_launch_failure("demo", &id, &err(code, "기동 오류 근거"), held).unwrap();
        assert_eq!(rec.status, status);
        assert_eq!(rec.error.as_deref(), Some("기동 오류 근거"));
        assert_eq!(lab.h.open_worker_threads("demo"), u32::from(status != ThreadStatus::Failed));
    }
}

#[tokio::test]
async fn open_restart_claim_keeps_its_original_slot_across_pre_send_interruption() {
    let lab = Lab::new();
    let id = lab
        .h
        .db
        .thread_insert_next("demo", |id| {
            let mut r = Lab::rec();
            r.id = id.into();
            r.status = ThreadStatus::Open;
            serde_json::to_string(&r).unwrap()
        })
        .unwrap();
    lab.h.update_rec("demo", &id, |r| r.status = ThreadStatus::Open).unwrap();
    assert_eq!(lab.h.open_worker_threads("demo"), 1);
    lab.h.db.thread_restart_claim("demo", &id, 1).unwrap();
    let claimed = lab.h.load("demo", &id).unwrap();
    assert!(claimed.launch_existing_slot);
    assert!(!claimed.launch_sent);
    assert_eq!(claimed.status, ThreadStatus::Starting);
    assert_eq!(lab.h.open_worker_threads("demo"), 1);
    lab.h.recover_launches("demo");
    assert_eq!(lab.h.load("demo", &id).unwrap().status, ThreadStatus::Open);
    assert_eq!(lab.h.open_worker_threads("demo"), 1);
}
