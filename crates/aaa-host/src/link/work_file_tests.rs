//! 실제 Agent link·DB·회수 owner를 연결한다. 파일은 목록 뒤 고정하고 가림 충돌만 재현한다.
use super::*;
use crate::orch::{ResolveOpts, ResolveWhy, ThreadRec};
use aaa_proto::{
    AgentActivity, AgentRequest, B64, BriefState, LibFile, ReportSnapshot, Role, ThreadKind, ThreadStatus,
};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};

#[tokio::test]
async fn work_file_memory_keeps_original_bytes_before_hash_verification() {
    memory_copy(false, false).await;
}

#[tokio::test]
async fn legacy_memory_keeps_original_bytes_before_hash_verification() {
    memory_copy(true, false).await;
}

#[tokio::test]
async fn invalid_memory_keeps_completion_but_blocks_resolve_until_repaired() {
    memory_copy(false, true).await;
}

/// 취소된 작업 전송의 가림 예외와 응답 대기는 연결에 남아서는 안 된다.
#[tokio::test]
async fn cancelled_work_file_requests_release_their_response_policy() {
    let (tx, mut frames) = aaa_common::conn::frame_channel();
    let conn = AgentConn::new(tx, 1);
    for request in [
        AgentRequest::WorkFileList { work_dir: "/work".into() },
        AgentRequest::WorkFileRead { work_dir: "/work".into(), path: "memory/memory.md".into(), offset: 0 },
        AgentRequest::MissionStateList { work_dir: "/work".into() },
        AgentRequest::MissionStateRead { work_dir: "/work".into(), path: "memory/memory.md".into(), offset: 0 },
    ] {
        let waiting = tokio::spawn({
            let conn = conn.clone();
            async move { conn.request(request).await }
        });
        frames.recv().await.unwrap();
        assert_eq!(conn.pending.lock().unwrap().len(), 1);
        waiting.abort();
        assert!(waiting.await.unwrap_err().is_cancelled());
        assert!(conn.pending.lock().unwrap().is_empty());
    }
}

async fn memory_copy(legacy: bool, invalid: bool) {
    let path = std::env::temp_dir().join(format!("aaa-memory-copy-{}", util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = path.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let host = Host::open(cfg, path.clone()).unwrap();
    host.project_create("p", Default::default(), "test").unwrap();
    let secret = aaa_test_support::token("memory-transfer-fixture");
    aaa_common::redact::remember(&secret);
    let mut bytes = vec![b'x'; 384 * 1024 + 127];
    let boundary = 384 * 1024 - 4;
    bytes[boundary..boundary + secret.len()].copy_from_slice(secret.as_bytes());
    assert_ne!(bytes, aaa_common::redact::bytes(&bytes), "고정된 원본도 가림은 hash를 바꾼다");
    let expected = bytes.clone();
    let fault = Arc::new(AtomicBool::new(invalid));
    let corrupt = fault.clone();
    let report = "[완료] 점검 끝\n\n## Next\n".as_bytes();
    let revision = hex::encode(Sha256::digest(report));
    let (stream, peer) = tokio::io::duplex(65536);
    let running = tokio::spawn(serve(host.clone(), Box::new(stream), None, None));
    let (r, w) = tokio::io::split(peer);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    tx.json(&LinkMsg::Hello(AgentHello {
        proto: AGENT_LINK,
        machine_id: "memory-agent".into(),
        machine_name: "mac-a".into(),
        boot_id: "boot".into(),
        os: "test".into(),
        arch: "test".into(),
        software: "test".into(),
        features: vec!["terminal.v1".into(), if legacy { "mission.v1" } else { "work_files.v1" }.into()],
        dev_mode: true,
        sha256: None,
        selfcheck: None,
        sessions: vec![],
        identity: None,
        harness: vec![],
    }))
    .await
    .unwrap();
    assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Welcome(_)));
    tx.json(&LinkMsg::Ping { seq: 1 }).await.unwrap();
    assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 1 });
    let mut rec = ThreadRec::starting("p", "자동 점검", "memory-agent", "codex", Role::Worker, ThreadKind::Dir, 0);
    rec.id = "t-0001".into();
    rec.status = ThreadStatus::Open;
    rec.brief = BriefState::Delivered;
    rec.work_dir = Some("/work/.aaa/p-t-0001".into());
    rec.automation = Some("check".into());
    rec.automation_run = Some(crate::automation_thread::Run {
        memory: true,
        memory_common: Some(Default::default()),
        close_idle_ms: Some(30),
        ..Default::default()
    });
    host.db.thread_import("p", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    let dest = host.project_dir("p").unwrap().join("automations/check.memory");
    std::fs::create_dir_all(dest.join("history")).unwrap();
    std::fs::write(dest.join("history/host.md"), b"host history").unwrap();
    let fake = tokio::spawn(async move {
        while let Ok(msg) = rx.control::<LinkMsg>().await {
            match msg {
                LinkMsg::Req { id, body, .. } => {
                    let response = match body {
                        AgentRequest::ReportCollect { .. } => AgentResponse::Report(ReportSnapshot {
                            hash: hex::encode(Sha256::digest(report)),
                            report: B64(report.to_vec()),
                            files: vec![],
                            skipped: vec![],
                        }),
                        AgentRequest::WorkFileList { .. } | AgentRequest::MissionStateList { .. } => {
                            let files = vec![LibFile {
                                path: "memory/memory.md".into(),
                                size: bytes.len() as u64,
                                sha256: hex::encode(Sha256::digest(&bytes)),
                            }];
                            if legacy {
                                AgentResponse::MissionFiles { files }
                            } else {
                                AgentResponse::WorkFiles { files }
                            }
                        }
                        AgentRequest::WorkFileRead { offset, .. } | AgentRequest::MissionStateRead { offset, .. } => {
                            let end = (offset as usize + 384 * 1024).min(bytes.len());
                            let mut chunk = bytes[offset as usize..end].to_vec();
                            if corrupt.load(Ordering::SeqCst) && offset == 0 {
                                chunk[0] = b'y';
                            }
                            AgentResponse::FileChunk { data: B64(chunk), eof: end == bytes.len() }
                        }
                        AgentRequest::FileView { .. } => {
                            AgentResponse::FileChunk { data: B64(secret.as_bytes().to_vec()), eof: true }
                        }
                        AgentRequest::WorkFileWrite { .. } => AgentResponse::Done,
                        other => panic!("예상 밖 요청: {other:?}"),
                    };
                    tx.json(&LinkMsg::Res { id, result: Outcome::Ok(response) }).await.unwrap();
                }
                LinkMsg::Ping { seq } => tx.json(&LinkMsg::Pong { seq }).await.unwrap(),
                _ => {}
            }
        }
    });
    let result = host.collect("p", &rec.id, false).await;
    if invalid {
        assert!(result.unwrap_err().message.contains("기억 hash"));
    } else {
        result.unwrap();
    }
    let copied = host.load("p", &rec.id).unwrap();
    assert_eq!(copied.report_hash.as_deref(), Some(revision.as_str()), "기억 실패도 새 보고를 숨기면 안 된다");
    assert_eq!(copied.acked_hash, copied.report_hash);
    assert_eq!(host.verified_report(&copied).unwrap().as_bytes(), report);
    assert!(
        crate::automation_thread::close_at(&copied, Some("[완료]"), Some((AgentActivity::Idle, 1)), false).is_some()
    );
    if invalid {
        assert_eq!(copied.copy.as_deref(), Some("failed"));
        assert!(!dest.join("memory.md").exists(), "실패한 기억을 적용하지 않는다");
        let error =
            host.thread_resolve("p", &rec.id, ResolveOpts::default(), ResolveWhy::Manual, "test").await.unwrap_err();
        assert!(error.message.contains("마지막 사본이 실패") && error.message.contains("기억 hash"));
        assert_eq!(host.load("p", &rec.id).unwrap().status, ThreadStatus::Open);
        // 파일 검증 결함을 해제한 사건 뒤 같은 보고 판의 정상 사본을 적용한다.
        fault.store(false, Ordering::SeqCst);
    }
    host.collect("p", &rec.id, true).await.unwrap();
    assert_eq!(host.load("p", &rec.id).unwrap().copy.as_deref(), Some("complete"));
    assert_eq!(std::fs::read(dest.join("memory.md")).unwrap(), expected);
    assert_eq!(std::fs::read(dest.join("history/host.md")).unwrap(), b"host history");
    let conn = host.live_conn("memory-agent").unwrap();
    let AgentResponse::FileChunk { data, .. } = conn
        .request(AgentRequest::FileView { path: "/work/visible.md".into(), offset: 0, len: 1024, page: false })
        .await
        .unwrap()
    else {
        panic!("보기 응답이어야 한다")
    };
    assert!(data.0.iter().all(|b| *b == b'*'), "같은 FileChunk라도 파일 보기는 계속 가린다");
    drop(conn);
    fake.abort();
    assert!(fake.await.unwrap_err().is_cancelled());
    running.await.unwrap().unwrap();
    drop(host);
    std::fs::remove_dir_all(path).unwrap();
}
