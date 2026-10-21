//! 개발 빌드의 실제 배포 조각 JSON 왕복 비용을 같은 입력으로 잰다.
use aaa_proto::{AgentResponse, B64, DEPLOY_CHUNK};
use std::hint::black_box;
use std::time::Instant;

fn main() {
    let bytes: Vec<u8> = (0..DEPLOY_CHUNK).map(|i| (i % 251) as u8).collect();
    let message =
        AgentResponse::DeployChunk { offset: 0, data: B64(bytes.clone()), eof: false, total: 64 * 1024 * 1024 };
    let rounds = 256;
    for sample in 1..=3 {
        let start = Instant::now();
        let mut wire = vec![];
        for _ in 0..rounds {
            wire = serde_json::to_vec(black_box(&message)).unwrap();
            black_box(&wire);
        }
        let encode = start.elapsed().as_secs_f64();
        let start = Instant::now();
        for _ in 0..rounds {
            let decoded: AgentResponse = serde_json::from_slice(black_box(&wire)).unwrap();
            let AgentResponse::DeployChunk { data, .. } = decoded else { unreachable!() };
            assert_eq!(data.0, bytes);
        }
        println!(
            "sample={sample} bytes={} rounds={rounds} wire={} encode={encode:.6} decode={:.6}",
            bytes.len(),
            wire.len(),
            start.elapsed().as_secs_f64()
        );
    }
}
