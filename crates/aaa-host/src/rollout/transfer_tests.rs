//! 시작한 지 120초가 넘은 전송을 DB에서 재개한다. 조각 영수증과 다음 요청을 사건으로 기다린다.
use super::*;
use aaa_common::conn::{FrameRx, spawn_writer};

#[test]
fn slow_transfer_resumes_and_renews_its_deadline_on_new_bytes() {
    web_tests::in_lab(|h| async move {
        let mut r = tests::record();
        r.actor = "person:1".into();
        r.spec.machines = vec!["m".into()];
        r.spec.source_machine = Some("m".into());
        r.steps = steps(&r.spec.machines, true);
        let now = aaa_common::util::now_ms();
        r.steps[0].started_ms = now - 180_000;
        r.steps[0].transferred_bytes = 1;
        r.steps[0].total_bytes = 4;
        r.updated_ms = now - 90_000;
        h.db.start_rollout(&r.id, &serde_json::to_string(&r).unwrap()).unwrap();

        let (writer, reader) = tokio::io::duplex(65_536);
        let conn = crate::AgentConn::new(spawn_writer(writer, MAX_FRAME), 2);
        h.inner.lock().unwrap().machines.get_mut("m").unwrap().conn = Some(conn.clone());
        let mut rx = FrameRx::new(reader, MAX_FRAME);
        let driver = h.drive_rollout();
        let agent = async {
            let mut offset = 1;
            loop {
                let LinkMsg::Req { id, body, deadline_ms } = rx.control::<LinkMsg>().await.unwrap() else {
                    panic!("배포 요청을 기다렸다")
                };
                let stage = |offset, complete| AgentResponse::DeployStage {
                    stage: DeployStageView {
                        rollout: r.id.clone(),
                        project: r.project.clone(),
                        sha256: r.spec.sha256.clone(),
                        total: 4,
                        offset,
                        complete,
                        version: complete.then(|| r.version.clone().unwrap()),
                    },
                };
                let reply = match body {
                    AgentRequest::DeployStageStatus { rollout } => {
                        assert_eq!(rollout, r.id);
                        Outcome::Ok(stage(offset, false))
                    }
                    AgentRequest::DeployFetch { offset: requested, .. } => {
                        assert_eq!(requested, offset);
                        assert_eq!(h.load_rollout(&r.id).unwrap().steps[0].transferred_bytes, offset);
                        Outcome::Ok(AgentResponse::DeployChunk {
                            offset,
                            data: B64(vec![42]),
                            total: 4,
                            eof: offset == 3,
                        })
                    }
                    AgentRequest::DeployStage { offset: requested, data, eof, .. } => {
                        assert_eq!((requested, data.0, eof), (offset, vec![42], offset == 3));
                        let wait = deadline_ms.unwrap();
                        if offset == 1 {
                            assert!((1..=30_000).contains(&wait), "같은 offset을 조회해 한도를 갱신했다: {wait}");
                        } else {
                            assert!((90_000..=120_000).contains(&wait), "새 조각을 확인한 뒤 남은 시간: {wait}");
                        }
                        offset += 1;
                        if requested == 2 {
                            // 대상은 저장했지만 응답이 끊겼다. 재연결 통지를 받은 진행기가 같은 ID로 상태를 다시 읽는다.
                            h.rollout_wake.notify_one();
                            Outcome::Err(err(ErrorCode::AgentDisconnected, "조각 응답 유실"))
                        } else {
                            Outcome::Ok(stage(offset, eof))
                        }
                    }
                    AgentRequest::DeployStatus { id } => {
                        assert_eq!(id, r.steps[1].job_id);
                        let saved = h.load_rollout(&r.id).unwrap();
                        assert!(saved.steps[0].completed);
                        assert_eq!((saved.cursor, saved.steps[0].transferred_bytes), (1, 4));
                        assert_eq!(saved.steps[0].started_ms, r.steps[0].started_ms);
                        Outcome::Err(err(ErrorCode::BadRequest, "실험실 사전 검사 종료"))
                    }
                    AgentRequest::DeployStageClean { .. } => {
                        assert_eq!(offset, 4, "전송을 끝내기 전에 정리했다");
                        conn.pending
                            .lock()
                            .unwrap()
                            .remove(&id)
                            .unwrap()
                            .send(Outcome::Ok(AgentResponse::Done))
                            .unwrap();
                        break;
                    }
                    _ => panic!("예상 밖의 요청"),
                };
                conn.pending.lock().unwrap().remove(&id).unwrap().send(reply).unwrap();
            }
        };
        tokio::time::timeout(Duration::from_secs(5), async {
            let (result, ()) = tokio::join!(driver, agent);
            result.unwrap();
        })
        .await
        .unwrap();
        assert_eq!(h.load_rollout(&r.id).unwrap().error.as_deref(), Some("실험실 사전 검사 종료"));
    });
}

#[test]
fn a_stalled_transfer_stops_at_120_seconds_even_after_resume() {
    web_tests::in_lab(|h| async move {
        for (phase, transferred, update_age) in
            [(RolloutPhase::Transfer, 0, 0), (RolloutPhase::Transfer, 1, 120_000), (RolloutPhase::Preflight, 0, 0)]
        {
            let mut r = tests::record();
            let now = aaa_common::util::now_ms();
            r.actor = "person:1".into();
            r.steps[0] = step("m", phase);
            r.steps[0].started_ms = now - 180_000;
            r.steps[0].transferred_bytes = transferred;
            r.updated_ms = now - update_age;
            let deadline = step_deadline(&r.steps[0], r.updated_ms);
            assert_eq!(deadline, if transferred > 0 { now } else { now - 60_000 });
            h.db.start_rollout(&r.id, &serde_json::to_string(&r).unwrap()).unwrap();
            tokio::time::timeout(Duration::from_secs(2), h.drive_rollout()).await.unwrap().unwrap();
            let saved = h.load_rollout(&r.id).unwrap();
            assert_eq!((saved.state, saved.cursor), (RolloutState::Stopped, 0));
            let reason = saved.error.unwrap();
            if phase == RolloutPhase::Transfer {
                assert!(reason.contains("120초") && reason.contains(&format!("확인 {transferred}바이트")), "{reason}");
            } else {
                assert!(reason.contains("Preflight"), "{reason}");
            }
        }
    });
}
