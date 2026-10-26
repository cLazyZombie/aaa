//! 협상한 큰 응답만 분할한다. frame 상한과 요청 시한은 그대로이며, 재전송하지 않는다.
use aaa_proto::{LinkMsg, MAX_FRAME};
use anyhow::{Result, ensure};
use std::collections::HashMap;

pub const FEATURE: &str = "response_parts.v1";
/// base64와 JSON 포장을 더해도 1MiB frame 안에 든다.
pub const PART_BYTES: usize = 384 << 10;
/// 한 응답과 연결 전체의 미완성 응답 합계 상한. frame 상한을 대신하지 않는다.
pub const RESPONSE_BYTES: usize = 16 << 20;

#[derive(Default)]
pub struct ResponseParts {
    pending: HashMap<String, Assembly>,
}

struct Assembly {
    total: usize,
    bytes: Vec<u8>,
}

impl ResponseParts {
    /// 요청이 끝났거나 취소된 조각은 다음 수신 사건에서 버린다. 재연결에는 이 객체를 넘기지 않는다.
    pub fn retain(&mut self, mut live: impl FnMut(&str) -> bool) {
        self.pending.retain(|id, _| live(id));
    }

    pub fn remove(&mut self, id: &str) {
        self.pending.remove(id);
    }

    /// 오류는 이 응답에만 적용한다. 성공한 전체 Res만 기존 해석·가림·적용 경계에 넘긴다.
    pub fn push(&mut self, id: &str, offset: u32, total: u32, data: Vec<u8>) -> Result<Option<LinkMsg>> {
        let result = self.append(id, offset as usize, total as usize, data);
        if result.is_err() {
            self.remove(id);
        }
        result
    }

    fn append(&mut self, id: &str, offset: usize, total: usize, data: Vec<u8>) -> Result<Option<LinkMsg>> {
        ensure!((MAX_FRAME..=RESPONSE_BYTES).contains(&total), "분할 응답 전체 길이 상한: {total}");
        ensure!(!data.is_empty() && data.len() <= PART_BYTES, "분할 응답 조각 길이 오류: {}", data.len());
        ensure!(offset <= total && data.len() <= total - offset, "분할 응답 범위 오류");
        if offset == 0 {
            ensure!(!self.pending.contains_key(id), "분할 응답 시작 중복");
            let reserved: usize = self.pending.values().map(|v| v.total).sum();
            ensure!(total <= RESPONSE_BYTES - reserved, "연결의 미완성 응답 합계 상한");
            // 선언한 전체 길이를 미리 할당하지 않는다.
            self.pending.insert(id.into(), Assembly { total, bytes: Vec::new() });
        }
        let assembly = self.pending.get_mut(id).ok_or_else(|| anyhow::anyhow!("분할 응답 시작이 없다"))?;
        ensure!(assembly.total == total && assembly.bytes.len() == offset, "분할 응답 길이·순서 불일치");
        assembly.bytes.extend_from_slice(&data);
        if assembly.bytes.len() != total {
            return Ok(None);
        }
        let assembly = self.pending.remove(id).unwrap();
        let msg: LinkMsg = serde_json::from_slice(&assembly.bytes)
            .map_err(|e| crate::diagnostic::context(e, anyhow::anyhow!("분할 응답 JSON 해석 실패")))?;
        ensure!(matches!(&msg, LinkMsg::Res { id: inner, .. } if inner == id), "분할 응답 종류·ID 불일치");
        Ok(Some(msg))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conn::{FrameRx, SendError, spawn_writer};
    use aaa_proto::{AgentResponse, B64, ErrorCode, LibFile, Outcome, ReportSnapshot, Skipped};

    fn incident_reply() -> LinkMsg {
        // 운영 사고와 같은 파일 수·frame 길이. 실제 경로·보고 원문은 쓰지 않는다.
        let mut snapshot = ReportSnapshot {
            hash: "0".repeat(64),
            report: B64(vec![b'x'; 9820]),
            files: (0..35).map(|i| LibFile { path: format!("empty-{i}"), size: 0, sha256: "0".repeat(64) }).collect(),
            skipped: (0..14746)
                .map(|i| Skipped {
                    path: format!("run-{i:05}/result.log"),
                    reason: "복사 상한(0 MB)을 넘는다. agent에 남는다".into(),
                    lazy_size: Some(8192),
                })
                .collect(),
        };
        let size = serde_json::to_vec(&LinkMsg::Res {
            id: "c0".into(),
            result: Outcome::Ok(AgentResponse::Report(snapshot.clone())),
        })
        .unwrap()
        .len()
            + 1;
        let padding = 2_232_861 - size;
        let count = snapshot.skipped.len();
        for (i, file) in snapshot.skipped.iter_mut().enumerate() {
            file.path.push_str(&"x".repeat(padding / count + usize::from(i < padding % count)));
        }
        LinkMsg::Res { id: "c0".into(), result: Outcome::Ok(AgentResponse::Report(snapshot)) }
    }

    /// 같은 사고 크기의 응답이 분할되거나 명시 오류가 되고, 다음 요청·ping이 같은 연결에서 살아야 한다.
    #[tokio::test]
    async fn incident_size_reply_and_legacy_refusal_keep_the_connection() {
        let original = incident_reply();
        assert_eq!(serde_json::to_vec(&original).unwrap().len() + 1, 2_232_861);
        for negotiated in [true, false] {
            let (writer, reader) = tokio::io::duplex(65536);
            let tx = spawn_writer(writer, MAX_FRAME);
            tx.set_response_parts(negotiated);
            let sending = tokio::spawn(async move {
                tx.link(&incident_reply()).await.unwrap();
                // 크기 오류는 wire에 한 byte도 쓰지 않고 호출자에게 바로 돌아온다.
                assert!(matches!(tx.control(vec![0; MAX_FRAME]).await, Err(SendError::TooLarge { .. })));
                tx.json(&LinkMsg::Pong { seq: 42 }).await.unwrap();
                tx.link(&LinkMsg::Res { id: "c1".into(), result: Outcome::Ok(AgentResponse::Done) }).await.unwrap();
            });
            let mut rx = FrameRx::new(reader, MAX_FRAME);
            let mut parts = ResponseParts::default();
            let mut count = 0;
            let response = loop {
                match rx.control::<LinkMsg>().await.unwrap() {
                    LinkMsg::ResPart { id, offset, total, data } => {
                        count += 1;
                        if let Some(msg) = parts.push(&id, offset, total, data.0).unwrap() {
                            break msg;
                        }
                    }
                    msg => break msg,
                }
            };
            if negotiated {
                assert_eq!(response, original);
                assert_eq!(count, 6);
            } else {
                assert!(
                    matches!(response, LinkMsg::Res { result: Outcome::Err(e), .. } if e.code == ErrorCode::TooLarge)
                );
                assert_eq!(count, 0);
            }
            assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 42 });
            assert_eq!(
                rx.control::<LinkMsg>().await.unwrap(),
                LinkMsg::Res { id: "c1".into(), result: Outcome::Ok(AgentResponse::Done) }
            );
            sending.await.unwrap();
        }
    }

    /// 불완전 응답의 보관 상한·취소·순서·ID 검증으로 다른 요청의 결과를 바꾸거나 메모리를 쌓지 못하게 한다.
    #[test]
    fn parts_reject_corruption_and_release_cancelled_or_failed_assemblies() {
        let mut parts = ResponseParts::default();
        assert!(parts.push("large", 0, RESPONSE_BYTES as u32 + 1, vec![0]).is_err());
        assert!(parts.push("large", 0, RESPONSE_BYTES as u32, vec![0]).unwrap().is_none());
        assert!(parts.push("other", 0, MAX_FRAME as u32, vec![0]).is_err());
        parts.retain(|_| false);
        assert!(parts.pending.is_empty());
        assert!(parts.push("late", 1, MAX_FRAME as u32, vec![0]).is_err());
        assert!(parts.push("bad", 0, MAX_FRAME as u32, vec![0]).unwrap().is_none());
        assert!(parts.push("bad", 0, MAX_FRAME as u32, vec![0]).is_err());
        assert!(parts.pending.is_empty());
        assert!(parts.push("bad", 0, MAX_FRAME as u32, vec![]).is_err());
        assert!(parts.push("bad", 0, MAX_FRAME as u32, vec![0; PART_BYTES + 1]).is_err());
        assert!(parts.push("bad", 0, MAX_FRAME as u32, vec![0]).unwrap().is_none());
        assert!(parts.push("bad", 2, MAX_FRAME as u32, vec![0]).is_err());
        assert!(parts.pending.is_empty());
        for msg in [incident_reply(), LinkMsg::Pong { seq: 5 }] {
            let mut bytes = serde_json::to_vec(&msg).unwrap();
            bytes.resize(bytes.len().max(MAX_FRAME), b' ');
            for (index, chunk) in bytes.chunks(PART_BYTES).enumerate() {
                let result = parts.push("wrong-id", (index * PART_BYTES) as u32, bytes.len() as u32, chunk.to_vec());
                if (index + 1) * PART_BYTES >= bytes.len() {
                    assert!(result.is_err());
                } else {
                    assert!(result.unwrap().is_none());
                }
            }
            assert!(parts.pending.is_empty());
        }
    }
}
