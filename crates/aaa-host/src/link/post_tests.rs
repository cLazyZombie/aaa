use super::*;
use aaa_proto::{PostInput, PostType};
/// 첫 메시지를 기다리는 연결의 TLS key를 현재 DB의 다른 key로 바꾸지 않는다.
#[tokio::test]
async fn install_agent_query_keeps_the_handshake_key_after_same_id_rekey() {
    let path = std::env::temp_dir().join(format!("aaa-install-peer-{}", util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.notify.web_push = false;
    let host = Host::open(cfg, path.clone()).unwrap();
    let old = "a".repeat(64);
    let replacement = "b".repeat(64);
    let machine_id = util::new_id();
    host.db.upsert_machine(&machine_id, "agent-a", Some(&old), false).unwrap();
    for rekey in [false, true] {
        // 이 immutable pair는 acceptor가 실제 TLS 인증서에서 만든 연결 신원이다.
        let known = Some((machine_id.clone(), old.clone()));
        if rekey {
            host.db.upsert_machine(&machine_id, "agent-a", Some(&replacement), false).unwrap();
        }
        let (stream, peer) = tokio::io::duplex(4096);
        let host = host.clone();
        let running =
            tokio::spawn(async move { serve_connection(host, Box::new(stream), known, None, &mut None).await });
        let (r, w) = tokio::io::split(peer);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        tx.json(&LinkMsg::InstallEnrollProbe { proto: AGENT_LINK }).await.unwrap();
        if rekey {
            // stale 연결에는 다른 key의 기존 등록 결과도 주지 않는다.
            assert!(
                tokio::time::timeout(Duration::from_secs(10), rx.control::<LinkMsg>())
                    .await
                    .unwrap()
                    .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_err()
            );
            assert!(
                running
                    .await
                    .unwrap()
                    .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_err()
            );
        } else {
            assert!(matches!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::InstallEnrollCapabilities { .. }));
            tx.json(&LinkMsg::InstallAgentQuery {
                proto: AGENT_LINK,
                machine_id: machine_id.clone(),
                machine_name: "agent-a".into(),
            })
            .await
            .unwrap();
            assert_eq!(
                rx.control::<LinkMsg>().await.unwrap(),
                LinkMsg::InstallAgentKnown { machine_id: machine_id.clone() }
            );
            running.await.unwrap().unwrap();
        }
    }
    drop(host);
    std::fs::remove_dir_all(path).unwrap();
}
#[tokio::test]
async fn stable_report_transfer_verifies_before_masking_and_keeps_a_verified_copy() {
    use aaa_proto::{AgentRequest, B64, BriefState, LibFile, ReportSnapshot, Role, ThreadKind, ThreadStatus};
    use sha2::{Digest, Sha256};
    let path = std::env::temp_dir().join(format!("aaa-report-transfer-{}", util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = path.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let host = Host::open(cfg, path.clone()).unwrap();
    host.project_create("p", Default::default(), "test").unwrap();
    let secret = &*aaa_test_support::token("fixture-report-auth-value-0638");
    aaa_common::redact::remember(secret);
    let mut bytes = vec![b'x'; 1024 * 1024 + 3176];
    bytes[..4].copy_from_slice(b"MZ\0\xff");
    // 조각 경계의 인증 조각까지 포함해 원본 전체 hash를 먼저 대조한다.
    for at in [23, 512 * 1024 - 4, 1024 * 1024 + 17] {
        bytes[at..at + secret.len()].copy_from_slice(secret.as_bytes());
    }
    let clean = aaa_common::redact::bytes(&bytes);
    assert_ne!(Sha256::digest(&bytes), Sha256::digest(&clean), "안 바뀐 원본도 전송 가림 뒤 해시가 달라지는 재현");
    let body = format!("[병합 준비됨] 사본 시험\n{secret}\n\n## Next\n");
    let revision = hex::encode(Sha256::digest(body.as_bytes()));
    let expected = bytes.clone();
    let (stream, peer) = tokio::io::duplex(65536);
    let running = tokio::spawn(serve(host.clone(), Box::new(stream), None, None));
    let (r, w) = tokio::io::split(peer);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    tx.json(&LinkMsg::Hello(AgentHello {
        proto: AGENT_LINK,
        machine_id: "report-agent".into(),
        machine_name: "mac-a".into(),
        boot_id: "boot".into(),
        os: "test".into(),
        arch: "test".into(),
        software: "test".into(),
        features: vec!["terminal.v1".into(), "report_copy.v2".into()],
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
    tx.json(&LinkMsg::Ping { seq: 638 }).await.unwrap();
    assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 638 });
    let mut rec =
        crate::orch::ThreadRec::starting("p", "보고", "report-agent", "codex", Role::Worker, ThreadKind::Dir, 0);
    rec.id = "t-0001".into();
    rec.status = ThreadStatus::Open;
    rec.brief = BriefState::Delivered;
    rec.work_dir = Some("/work/.aaa/p-t-0001".into());
    host.db.thread_import("p", &rec.id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
    let fake = tokio::spawn(async move {
        while let Ok(msg) = rx
            .control::<LinkMsg>()
            .await
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "수신 루프 종료 원인"))
        {
            match msg {
                LinkMsg::Req { id, body: request, .. } => {
                    let response = match request {
                        AgentRequest::ReportCollect { .. } => AgentResponse::Report(ReportSnapshot {
                            hash: revision.clone(),
                            report: B64(body.as_bytes().to_vec()),
                            files: vec![],
                            skipped: vec![],
                        }),
                        AgentRequest::ReportTransfer { .. } => AgentResponse::ReportTransfer(ReportSnapshot {
                            hash: revision.clone(),
                            report: B64(body.as_bytes().to_vec()),
                            files: vec![LibFile {
                                path: "stable.bin".into(),
                                size: bytes.len() as u64,
                                sha256: hex::encode(Sha256::digest(&bytes)),
                            }],
                            skipped: vec![],
                        }),
                        AgentRequest::ReportFileRead { offset, len, .. } => {
                            let end = (offset as usize + len as usize).min(bytes.len());
                            AgentResponse::ReportFileChunk {
                                data: B64(bytes[offset as usize..end].to_vec()),
                                eof: end == bytes.len(),
                            }
                        }
                        _ => AgentResponse::Done,
                    };
                    tx.json(&LinkMsg::Res { id, result: Outcome::Ok(response) }).await.unwrap();
                }
                LinkMsg::Ping { seq } => {
                    tx.json(&LinkMsg::Pong { seq }).await.unwrap();
                }
                _ => {}
            }
        }
    });
    host.refresh_report("p", "t-0001");
    tokio::time::timeout(Duration::from_secs(15), async {
        while host.report_refresh.lock().unwrap().contains_key("p/t-0001") {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let rec = host.load("p", "t-0001").unwrap();
    assert_eq!(rec.copy.as_deref(), Some("complete"));
    let dir = host.project_dir("p").unwrap();
    let copied = std::fs::read(dir.join("library/t-0001/stable.bin")).unwrap();
    assert_eq!(Sha256::digest(&copied), Sha256::digest(&expected), "바이너리 사본은 원본 hash를 유지한다");
    assert_eq!(copied, expected);
    assert_ne!(expected, clean);
    let stored = host.verified_report(&rec).unwrap();
    assert!(!stored.contains(secret));
    assert_eq!(rec.report_stage, Some(aaa_proto::ThreadStage::MergeReady));
    host.backfill_report_stages().unwrap();
    assert_eq!(host.load("p", "t-0001").unwrap().report_stage, rec.report_stage);
    let rid = host.thread_report_id("p", "t-0001", rec.report_hash.as_deref()).unwrap();
    assert_eq!(std::fs::read(dir.join("library").join(&rid).join("stable.bin")).unwrap(), expected);
    assert_eq!(std::fs::read_to_string(dir.join("threads").join(format!("{rid}.md"))).unwrap(), stored);
    let warnings = host.db.inbox_list(Some("p"), true).unwrap();
    let warnings: Vec<_> = warnings.iter().filter(|i| i.item_kind == "report-secret-warning").collect();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].body.contains("stable.bin"));
    assert!(!warnings[0].body.contains(secret));
    fake.abort();
    // 취소 완료로 peer를 닫고 연결 정리까지 기다려야 Windows에서도 DB 핸들이 풀린다.
    assert!(fake.await.unwrap_err().is_cancelled());
    running.await.unwrap().unwrap();
    drop(host);
    std::fs::remove_dir_all(path).unwrap();
}

#[tokio::test]
async fn deployment_bytes_survive_auth_redaction_and_a_partial_tail() {
    use aaa_proto::{AgentRequest, B64, DEPLOY_CHUNK};
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let path = std::env::temp_dir().join(format!("aaa-link-tail-{}", util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = path.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let host = Host::open(cfg, path.clone()).unwrap();
    let secret = &*aaa_test_support::token("fixture-deployment-auth-value-0525");
    aaa_common::redact::remember_auth("codex", &serde_json::json!({"access_token":secret}).to_string());
    let encoded = base64::engine::general_purpose::STANDARD.encode(secret);
    let mut bytes: Vec<u8> = (0..DEPLOY_CHUNK * 2 + 3176).map(|i| (i % 251) as u8).collect();
    // 인증과 겹치는 바이트를 온전한 조각·마지막 부분 조각에 둔다. 실제 인증은 읽지 않는다.
    for at in [23, DEPLOY_CHUNK * 2 + 17] {
        bytes[at..at + secret.len()].copy_from_slice(secret.as_bytes());
        bytes[at + 100..at + 100 + encoded.len()].copy_from_slice(encoded.as_bytes());
    }
    let expected = hex::encode(Sha256::digest(&bytes));
    let (stream, peer) = tokio::io::duplex(1024);
    let running = tokio::spawn(serve(host.clone(), Box::new(stream), None, None));
    let (r, w) = tokio::io::split(peer);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    tx.json(&LinkMsg::Hello(AgentHello {
        proto: AGENT_LINK,
        machine_id: "transfer-agent".into(),
        machine_name: "mac-a".into(),
        boot_id: "boot".into(),
        os: "test".into(),
        arch: "test".into(),
        software: "fixture".into(),
        features: vec!["terminal.v1".into(), "deploy_stage.v1".into()],
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
    tx.json(&LinkMsg::Ping { seq: 525 }).await.unwrap();
    assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 525 });
    let conn = host.live_conn("transfer-agent").unwrap();
    let mut received = Vec::new();
    for chunk in bytes.chunks(DEPLOY_CHUNK) {
        let offset = received.len() as u64;
        let eof = offset + chunk.len() as u64 == bytes.len() as u64;
        let request = AgentRequest::DeployFetch {
            path: "/build/aaa".into(),
            offset,
            len: DEPLOY_CHUNK as u32,
            sha256: expected.clone(),
        };
        let (response, ()) = tokio::join!(conn.request(request.clone()), async {
            let LinkMsg::Req { id, body, .. } = rx.control::<LinkMsg>().await.unwrap() else {
                panic!("요청이어야 한다")
            };
            assert_eq!(body, request);
            tx.json(&LinkMsg::Res {
                id,
                result: Outcome::Ok(AgentResponse::DeployChunk {
                    offset,
                    data: B64(chunk.to_vec()),
                    eof,
                    total: bytes.len() as u64,
                }),
            })
            .await
            .unwrap();
        });
        let AgentResponse::DeployChunk { offset: got, data, eof: done, total } = response.unwrap() else {
            panic!("조각이어야 한다")
        };
        assert_eq!((got, done, total, data.0.len()), (offset, eof, bytes.len() as u64, chunk.len()));
        received.extend(data.0);
    }
    // 파일 보기 응답은 여전히 가린다. 전송 보존 때문에 표시 경계의 가림을 끄면 안 된다.
    let (visible, ()) = tokio::join!(
        conn.request(AgentRequest::FileView {
            path: "/build/readme.txt".into(),
            offset: 0,
            len: DEPLOY_CHUNK as u32,
            page: false,
        }),
        async {
            let LinkMsg::Req { id, .. } = rx.control::<LinkMsg>().await.unwrap() else {
                panic!("요청이어야 한다")
            };
            tx.json(&LinkMsg::Res {
                id,
                result: Outcome::Ok(AgentResponse::FileChunk { data: B64(secret.as_bytes().to_vec()), eof: true }),
            })
            .await
            .unwrap();
        }
    );
    let AgentResponse::FileChunk { data, .. } = visible.unwrap() else { panic!("보기 조각이어야 한다") };
    assert!(!data.0.windows(8).any(|s| secret.as_bytes().windows(8).any(|n| s == n)), "파일 보기에서 인증이 노출됐다");
    // host 일반 응답의 B64 글자만 겹쳐도 reader가 살아 있어야 한다.
    let encoded_secret = aaa_test_support::token("host-b64");
    aaa_common::redact::remember(&encoded_secret);
    let snapshot = base64::engine::general_purpose::STANDARD.decode(format!("{}AAAA", &encoded_secret[..16])).unwrap();
    let (visible, ()) = tokio::join!(
        conn.request(AgentRequest::FileView {
            path: "/build/snapshot.bin".into(),
            offset: 0,
            len: DEPLOY_CHUNK as u32,
            page: false,
        }),
        async {
            let LinkMsg::Req { id, .. } = rx.control::<LinkMsg>().await.unwrap() else {
                panic!("요청이어야 한다")
            };
            tx.json(&LinkMsg::Res {
                id,
                result: Outcome::Ok(AgentResponse::FileChunk { data: B64(snapshot.clone()), eof: true }),
            })
            .await
            .unwrap();
        }
    );
    assert!(matches!(visible.unwrap(), AgentResponse::FileChunk { data, .. } if data.0 == snapshot));
    tx.json(&LinkMsg::Ping { seq: 526 }).await.unwrap();
    assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 526 });
    drop(conn);
    drop(tx);
    drop(rx);
    running.await.unwrap().unwrap();
    drop(host);
    std::fs::remove_dir_all(path).unwrap();
    assert_eq!(hex::encode(Sha256::digest(&received)), expected, "전송 EOF SHA-256/크기 불일치");
    assert_eq!(received, bytes);
}
#[tokio::test]
async fn post_ack_is_never_sent_to_agent_without_feature() {
    let path = std::env::temp_dir().join(format!("aap-host-{}", util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = path.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let host = Host::open(cfg, path.clone()).unwrap();
    let (stream, peer) = tokio::io::duplex(1 << 20);
    let running = tokio::spawn(serve(host, Box::new(stream), None, None));
    let (r, w) = tokio::io::split(peer);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    tx.json(&LinkMsg::Hello(AgentHello {
        proto: AGENT_LINK,
        machine_id: "old-agent".into(),
        machine_name: "mac-a".into(),
        boot_id: "boot".into(),
        os: "test".into(),
        arch: "test".into(),
        software: "old".into(),
        features: vec!["terminal.v1".into()],
        dev_mode: true,
        sha256: None,
        selfcheck: None,
        sessions: vec![],
        identity: None,
        harness: vec![],
    }))
    .await
    .unwrap();
    assert!(
        matches!(rx.control::<LinkMsg>().await.unwrap(),LinkMsg::Welcome(w) if w.features.iter().any(|f|f=="post.v1"))
    );
    tx.json(&LinkMsg::Event(AgentEvent::Post {
        session: "s".into(),
        input: PostInput {
            post_id: format!("{:032x}", 1),
            to: "coordinator".into(),
            post_type: PostType::Data,
            topic: "test".into(),
            title: "기록".into(),
            body: "본문".into(),
            needs_user: false,
            refs: vec![],
        },
    }))
    .await
    .unwrap();
    tx.json(&LinkMsg::Ping { seq: 43 }).await.unwrap();
    // 같은 연결의 Pong은 앞 사건 처리가 끝났다는 울타리다. Ack를 먼저 보내면 여기서 실패한다.
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), rx.control::<LinkMsg>()).await.unwrap().unwrap(),
        LinkMsg::Pong { seq: 43 }
    );
    drop(tx);
    drop(rx);
    running.await.unwrap().unwrap();
    std::fs::remove_dir_all(path).unwrap();
}
