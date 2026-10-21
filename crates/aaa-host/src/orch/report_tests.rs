//! 보고 회수의 전송·재시도·저장 경계. fake는 Agent link 응답만 만들고 상태 판정은 host가 한다.
use super::*;
use aaa_common::conn::{FrameRx, spawn_writer};
use aaa_proto::{B64, LibFile, LinkMsg, Outcome, ReportSnapshot};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Lab {
    h: Arc<Host>,
    root: PathBuf,
    requests: Arc<AtomicUsize>,
    fake: tokio::task::JoinHandle<()>,
}
impl Drop for Lab {
    fn drop(&mut self) {
        self.fake.abort();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
impl Lab {
    fn new(failures: usize, file_bytes: &'static [u8]) -> Self {
        Self::with_file(failures, file_bytes, "test.log", b"old")
    }

    fn with_file(failures: usize, file_bytes: &'static [u8], name: &'static str, manifest: &'static [u8]) -> Self {
        Self::with_report(failures, file_bytes, name, manifest, "[진행 중] 시험 중\n")
    }

    fn with_report(
        failures: usize,
        file_bytes: &'static [u8],
        name: &'static str,
        manifest: &'static [u8],
        report: &'static str,
    ) -> Self {
        let root = std::env::temp_dir().join(format!("aaa-copy-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("p", Default::default(), "test").unwrap();
        let mut r = ThreadRec::starting("p", "보고", "m", "codex", Role::Worker, ThreadKind::Dir, 0);
        r.id = "t-0001".into();
        r.status = ThreadStatus::Open;
        r.brief = BriefState::Delivered;
        r.work_dir = Some("/work/.aaa/p-t-0001".into());
        r.report_hash = Some("previous".into());
        r.acked_hash = r.report_hash.clone();
        h.db.thread_import("p", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
        let (writer, reader) = tokio::io::duplex(65536);
        let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
        h.inner.lock().unwrap().machines.insert(
            "m".into(),
            crate::MachineRt {
                name: "mac-a".into(),
                local: true,
                os: None,
                boot_id: None,
                dev_mode: true,
                conn: Some(conn.clone()),
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: vec![],
                identity: None,
                agent_pid: None,
                colors: None,
                build: Default::default(),
            },
        );
        let requests = Arc::new(AtomicUsize::new(0));
        let seen = requests.clone();
        let fake = tokio::spawn(async move {
            let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
            while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                let reply = match body {
                    AgentRequest::ReportCollect { max_library_bytes, .. } => {
                        if max_library_bytes != 0 && seen.fetch_add(1, Ordering::SeqCst) < failures {
                            Outcome::Err(internal("시험 agent 복사 실패"))
                        } else {
                            let report = report.as_bytes().to_vec();
                            Outcome::Ok(AgentResponse::Report(ReportSnapshot {
                                hash: hex::encode(sha2::Sha256::digest(&report)),
                                report: B64(report),
                                files: if !file_bytes.is_empty() && max_library_bytes != 0 {
                                    vec![LibFile {
                                        path: name.into(),
                                        size: manifest.len() as u64,
                                        sha256: hex::encode(sha2::Sha256::digest(manifest)),
                                    }]
                                } else {
                                    vec![]
                                },
                                skipped: vec![],
                            }))
                        }
                    }
                    AgentRequest::FileRead { offset, len, .. } => {
                        // manifest 뒤 append: 요청한 범위만 읽는 실제 agent와 같은 응답이다.
                        let file = file_bytes;
                        let start = offset as usize;
                        let end = (start + len as usize).min(file.len());
                        Outcome::Ok(AgentResponse::FileChunk {
                            data: B64(file[start..end].to_vec()),
                            eof: end == file.len(),
                        })
                    }
                    AgentRequest::WorkFileWrite { path, data, .. } => {
                        assert!(path.starts_with("inbox/question-format-"));
                        assert!(path.strip_prefix("inbox/").unwrap().strip_suffix(".json").unwrap().len() <= 64);
                        let notice: serde_json::Value = serde_json::from_slice(&data.0).unwrap();
                        assert_eq!(notice["kind"], "question-format-warning");
                        Outcome::Ok(AgentResponse::Done)
                    }
                    other => panic!("예상 밖 요청: {other:?}"),
                };
                if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(reply);
                }
            }
        });
        Self { h, root, requests, fake }
    }
}

#[tokio::test]
async fn question_format_warning_keeps_even_empty_cards_blocks_all_answers_and_notifies_once() {
    for report in [
        "[결정 필요] 질문 1개: 진행\n## Questions\n### 1. 진행\n- **A 진행\n- B 보류\n- C 멈춤\n",
        "[검수 대기] 질문 1개: 진행\n## Questions\n### 1. 진행\n- A 진행\n",
    ] {
        let lab = Lab::with_report(0, b"", "unused", b"", report);
        lab.h.collect_if_changed("p", "t-0001").await;
        let view = lab.h.thread_show("p", "t-0001").unwrap();
        let q = view.questions.unwrap();
        assert!(q.format_warning.as_deref().unwrap().contains("형식 확인 필요"));
        assert!(!q.review, "명시한 질문이 깨지면 기본 검수 카드로 대신하지 않는다");
        assert_eq!(view.copy.as_deref(), Some("complete"));
        let err = lab.h.thread_answer("p", "t-0001", &q.report_hash, vec![Some(0)], None, "test").await.unwrap_err();
        assert!(err.message.contains("형식 확인 필요"));
        if let Some(id) = q.items.first().and_then(|q| q.id.as_deref()) {
            let err = lab.h.reply_by_id("p", id, Some("A"), None, "test").await.unwrap_err();
            assert!(err.message.contains("형식 확인 필요"));
            assert!(lab.h.db.question_target(Some("p"), id).unwrap().unwrap().draft.is_none());
        }
        lab.h.collect("p", "t-0001", true).await.unwrap();
        let items = lab.h.inbox("p", true, true).unwrap().items;
        let warnings: Vec<_> = items.iter().filter(|i| i.item_kind == "question-format-warning").collect();
        assert_eq!(warnings.len(), 1, "{items:?}");
        assert!(!warnings[0].body.contains("전달 실패"));
        assert!(lab.h.load("p", "t-0001").unwrap().question_answer.is_none());
    }
}

#[tokio::test]
async fn report_sandbox_warning_does_not_fail_copy_or_repeat_for_the_same_snapshot() {
    let html = b"<script>fetch('/api/v1/me'); localStorage.x</script>";
    let lab = Lab::with_file(0, html, "index.html", html);
    lab.h.collect_if_changed("p", "t-0001").await;
    assert_eq!(lab.h.load("p", "t-0001").unwrap().copy.as_deref(), Some("complete"));
    let warnings = || {
        lab.h
            .inbox("p", true, true)
            .unwrap()
            .items
            .into_iter()
            .filter(|item| item.item_kind == "report-sandbox-warning")
            .collect::<Vec<_>>()
    };
    let items = warnings();
    assert_eq!(items.len(), 1);
    assert!(items[0].body.contains("index.html") && items[0].body.contains("fetch"), "{items:?}");
    lab.h.collect("p", "t-0001", true).await.unwrap();
    assert_eq!(warnings().len(), 1);
}

#[tokio::test]
async fn copy_failure_retries_without_new_report_and_warns_once() {
    for failures in [1, usize::MAX] {
        let lab = Lab::new(failures, b"");
        lab.h.collect_if_changed("p", "t-0001").await;
        let r = lab.h.load("p", "t-0001").unwrap();
        assert_eq!(lab.requests.load(Ordering::SeqCst), 2, "복사 실패를 한 번 다시 받아야 한다");
        if failures == 1 {
            assert_eq!(r.copy.as_deref(), Some("complete"));
            assert!(lab.h.inbox("p", true, true).unwrap().items.is_empty());
        } else {
            assert_eq!(r.report_hash.as_deref(), Some("previous"), "이전 사본을 보존한다");
            assert_eq!(r.last_group, Some(ThreadGroup::Working));
            // hash 변화·새 harness 사건 없이 ticker만으로 재수집해야 한다.
            lab.h.retry_report_copies("p", u64::MAX);
            while lab.h.report_refresh.lock().unwrap().contains_key("p/t-0001") {
                tokio::task::yield_now().await;
            }
            let items = lab.h.inbox("p", true, true).unwrap().items;
            assert_eq!(items.len(), 1, "계속 실패해도 경고는 한 번이다: {items:?}");
            assert_eq!(items[0].item_kind, "report-copy-failed");
            assert!(!items[0].summary.contains("사람"));
            assert!(lab.requests.load(Ordering::SeqCst) > 2);
        }
    }
}

#[tokio::test]
async fn appended_library_log_copies_only_the_hashed_prefix() {
    let lab = Lab::new(0, b"old appended");
    lab.h.collect_if_changed("p", "t-0001").await;
    let r = lab.h.load("p", "t-0001").unwrap();
    assert_eq!(r.copy.as_deref(), Some("complete"));
    assert_eq!(std::fs::read(lab.h.project_dir("p").unwrap().join("library/t-0001/test.log")).unwrap(), b"old");
}

#[tokio::test]
async fn rewritten_library_log_keeps_the_previous_verified_copy() {
    let lab = Lab::new(0, b"bad appended");
    let dir = lab.h.project_dir("p").unwrap().join("library/t-0001");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test.log"), "verified").unwrap();
    lab.h.collect_if_changed("p", "t-0001").await;
    assert_eq!(lab.h.load("p", "t-0001").unwrap().copy.as_deref(), Some("failed"));
    assert_eq!(std::fs::read(dir.join("test.log")).unwrap(), b"verified");
}

#[tokio::test]
async fn host_report_write_failure_is_retried_after_the_storage_recovers() {
    let lab = Lab::new(0, b"");
    let path = lab.h.project_dir("p").unwrap().join("threads/t-0001.md");
    // 최종 report rename을 막는다. FileRead가 성공해도 host 저장 실패는 같은 시스템 오류다.
    std::fs::create_dir_all(&path).unwrap();
    lab.h.collect_if_changed("p", "t-0001").await;
    assert_eq!(lab.h.load("p", "t-0001").unwrap().copy.as_deref(), Some("failed"));
    assert_eq!(lab.h.inbox("p", true, true).unwrap().items.len(), 1);
    std::fs::remove_dir(&path).unwrap();
    lab.h.collect_if_changed("p", "t-0001").await;
    let r = lab.h.load("p", "t-0001").unwrap();
    assert_eq!(r.copy.as_deref(), Some("complete"));
    assert_eq!(r.copy_failures, 0);
    assert_eq!(r.copy_retry_at_ms, 0);
    assert!(path.is_file());
}

#[tokio::test]
async fn broken_library_does_not_freeze_the_report_or_its_ack() {
    let lab = Lab::new(0, b"bad appended");
    lab.h.collect_if_changed("p", "t-0001").await;
    let rec = lab.h.load("p", "t-0001").unwrap();
    assert_eq!(rec.copy.as_deref(), Some("failed"));
    assert_ne!(rec.report_hash.as_deref(), Some("previous"));
    assert_eq!(lab.h.verified_report(&rec).as_deref(), Some("[진행 중] 시험 중\n"));
    let dir = lab.h.project_dir("p").unwrap();
    let files = lab.h.manifest(&dir, "t-0001");
    assert_eq!(files.len(), 1);
    assert!(files[0].on_agent, "실패한 파일을 검증한 사본으로 표시하면 안 된다");
    let revision = lab.h.thread_report_id("p", "t-0001", rec.report_hash.as_deref()).unwrap();
    assert!(!dir.join("library").join(&revision).join("test.log").exists());
    lab.h.thread_ack("p", "t-0001", "test").unwrap();
    let acked = lab.h.load("p", "t-0001").unwrap().acked_hash;
    lab.h.collect_if_changed("p", "t-0001").await;
    assert_eq!(lab.h.load("p", "t-0001").unwrap().acked_hash, acked);
    assert_eq!(
        lab.h.inbox("p", true, true).unwrap().items.iter().filter(|i| i.item_kind == "report-copy-failed").count(),
        1
    );
}

#[tokio::test]
async fn another_thread_collects_while_a_report_transfer_is_held() {
    let lab = Lab::new(0, b"");
    let mut other = lab.h.load("p", "t-0001").unwrap();
    other.id = "t-0002".into();
    lab.h.db.thread_import("p", &other.id, "open", &serde_json::to_string(&other).unwrap()).unwrap();
    let lock = lab.h.report_collect_lock("p", "t-0001");
    let held = lock.lock().await;
    lab.h.refresh_report("p", "t-0001");
    // 2번의 실제 회수를 끝낸 뒤에도 1번의 소유권은 그대로다. 고정 sleep·성능 상한이 아니다.
    tokio::time::timeout(std::time::Duration::from_secs(10), lab.h.collect_if_changed("p", "t-0002")).await.unwrap();
    assert_eq!(lab.h.load("p", "t-0002").unwrap().copy.as_deref(), Some("complete"));
    assert_eq!(lab.h.load("p", "t-0001").unwrap().report_hash.as_deref(), Some("previous"));
    drop(held);
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while lab.h.report_refresh.lock().unwrap().contains_key("p/t-0001") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(lab.h.load("p", "t-0001").unwrap().copy.as_deref(), Some("complete"));
}

#[tokio::test]
async fn same_revision_recovery_repairs_the_body_and_keeps_ack() {
    let lab = Lab::new(0, b"");
    lab.h.collect_if_changed("p", "t-0001").await;
    lab.h.thread_ack("p", "t-0001", "test").unwrap();
    let previous = lab.h.load("p", "t-0001").unwrap();
    let path = lab.h.project_dir("p").unwrap().join("threads/t-0001.md");
    std::fs::write(&path, "obsolete").unwrap();
    lab.h
        .update_rec("p", "t-0001", |r| {
            r.copy = Some("failed".into());
            r.report_copy_hash = None;
        })
        .unwrap();
    lab.h.collect_if_changed("p", "t-0001").await;
    let rec = lab.h.load("p", "t-0001").unwrap();
    assert_eq!(rec.copy.as_deref(), Some("complete"));
    assert_eq!(rec.report_hash, previous.report_hash);
    assert_eq!(rec.acked_hash, previous.acked_hash);
    assert_eq!(lab.h.verified_report(&rec).as_deref(), Some("[진행 중] 시험 중\n"));
}
