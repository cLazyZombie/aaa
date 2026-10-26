//! 실제 host reader의 협상·재조립·가림과 잘못된 조각 뒤 같은 연결의 요청을 검사한다.
use super::*;
use aaa_common::response_parts::{FEATURE, PART_BYTES};
use aaa_proto::{AgentRequest, B64, CiAnswer, ReportSnapshot, Skipped};
use sha2::{Digest, Sha256};

#[tokio::test]
async fn large_report_and_bad_parts_keep_requests_on_one_link() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let path = std::env::temp_dir().join(format!("aaa-parts-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = path.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let host = Host::open(cfg, path.join("host")).unwrap();
        let secret = aaa_test_support::CANARY;
        aaa_common::redact::remember(secret);
        let snapshot = ReportSnapshot {
            hash: hex::encode(Sha256::digest(secret.as_bytes())),
            report: B64(secret.as_bytes().to_vec()),
            files: vec![],
            skipped: (0..14781)
                .map(|i| Skipped {
                    path: format!("verify/run-{i:05}/diagnostics/browser-console-result.log"),
                    reason: "복사 상한(0 MB)을 넘는다. agent에 남는다".into(),
                    lazy_size: Some(8192),
                })
                .collect(),
        };
        assert!(serde_json::to_vec(&snapshot).unwrap().len() > 2 * MAX_FRAME);
        let expected = snapshot.clone();
        let (stream, peer) = tokio::io::duplex(65536);
        let running = tokio::spawn(serve(host.clone(), Box::new(stream), None, None));
        let (reader, writer) = tokio::io::split(peer);
        let tx = spawn_writer(writer, MAX_FRAME);
        let mut rx = FrameRx::new(reader, MAX_FRAME);
        tx.json(&LinkMsg::Hello(AgentHello {
            proto: AGENT_LINK,
            machine_id: "m".into(),
            machine_name: "mac-a".into(),
            boot_id: "boot".into(),
            os: "test".into(),
            arch: "test".into(),
            software: "test".into(),
            sha256: None,
            features: vec!["terminal.v1".into(), "report_hash.v1".into(), "report_copy.v2".into(), FEATURE.into()],
            dev_mode: true,
            selfcheck: None,
            sessions: vec![],
            identity: None,
            harness: vec![],
        }))
        .await
        .unwrap();
        let LinkMsg::Welcome(welcome) = rx.control().await.unwrap() else { panic!("welcome가 없다") };
        assert!(welcome.features.iter().any(|f| f == FEATURE));
        tx.set_response_parts(true);
        tx.json(&LinkMsg::Ping { seq: 1131 }).await.unwrap();
        assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 1131 });
        let conn = host.live_conn("m").unwrap();
        let fake = tokio::spawn(async move {
            while let Ok(message) = rx.control::<LinkMsg>().await {
                match message {
                    LinkMsg::Req { id, body, .. } => {
                        let response = match body {
                            AgentRequest::ReportHash { .. } => {
                                AgentResponse::ReportHash { hash: snapshot.hash.clone() }
                            }
                            AgentRequest::ReportTransfer { .. } => AgentResponse::ReportTransfer(snapshot.clone()),
                            AgentRequest::CiQuery { .. } => {
                                // 인증 조각이 분할 경계를 가로질러도, 조립 뒤 가린다.
                                let mut ci = CiAnswer { log: secret.into(), ..Default::default() };
                                let prefix = serde_json::to_vec(&LinkMsg::Res {
                                    id: id.clone(),
                                    result: Outcome::Ok(AgentResponse::Ci(ci.clone())),
                                })
                                .unwrap();
                                let at = prefix.windows(secret.len()).position(|w| w == secret.as_bytes()).unwrap();
                                ci.log = format!(
                                    "{}{}{}",
                                    "x".repeat(PART_BYTES - at - secret.len() / 2),
                                    secret,
                                    "x".repeat(MAX_FRAME)
                                );
                                AgentResponse::Ci(ci)
                            }
                            AgentRequest::Cwd { .. } => {
                                tx.json(&LinkMsg::ResPart {
                                    id,
                                    offset: 1,
                                    total: MAX_FRAME as u32,
                                    data: B64(vec![b'x']),
                                })
                                .await
                                .unwrap();
                                continue;
                            }
                            _ => AgentResponse::Done,
                        };
                        tx.link(&LinkMsg::Res { id, result: Outcome::Ok(response) }).await.unwrap();
                    }
                    LinkMsg::Ping { seq } => {
                        tx.json(&LinkMsg::Pong { seq }).await.unwrap();
                    }
                    _ => {}
                }
            }
        });
        assert_eq!(
            conn.request(AgentRequest::ReportHash { work_dir: "/work/.aaa/p-t-0001".into() }).await.unwrap(),
            AgentResponse::ReportHash { hash: expected.hash.clone() }
        );
        // report_copy.v2는 원본 hash 확인 전에 가리지 않는다. 분할도 이 경계를 유지해야 한다.
        let received = conn
            .request(AgentRequest::ReportTransfer {
                work_dir: "/work/.aaa/p-t-0001".into(),
                max_library_bytes: 200 << 20,
            })
            .await
            .unwrap();
        assert_eq!(received, AgentResponse::ReportTransfer(expected));
        let AgentResponse::Ci(ci) = conn
            .request(AgentRequest::CiQuery {
                cwd: "/repo".into(),
                query: aaa_proto::CiQuery::Jobs { run: 1, attempt: 1, page: 1 },
            })
            .await
            .unwrap()
        else {
            panic!("CI 응답이 아니다")
        };
        assert!(!ci.log.contains(secret));
        let error = conn.request(AgentRequest::Cwd { session: "s".into() }).await.unwrap_err();
        assert_eq!(error.code, ErrorCode::BadRequest);
        assert!(error.message.contains("분할 응답 시작이 없다"));
        assert_eq!(
            conn.request(AgentRequest::Input { session: "s".into(), data: B64(b"next".to_vec()) }).await.unwrap(),
            AgentResponse::Done
        );
        assert_eq!(host.live_conn("m").unwrap().generation, conn.generation);
        fake.abort();
        assert!(fake.await.unwrap_err().is_cancelled());
        running.await.unwrap().unwrap();
        drop(conn);
        drop(host);
        std::fs::remove_dir_all(path).unwrap();
    })
    .await
    .expect("실제 host 응답 분할 회귀의 5초 상한");
}
