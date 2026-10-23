use super::*;
use crate::conn::{FrameRx, Incoming, spawn_writer};
use aaa_proto::{Snapshot, TerminalChunk};

fn snapshot(bytes: Vec<u8>) -> LinkMsg {
    LinkMsg::Res {
        id: "subscribe".into(),
        result: Outcome::Ok(AgentResponse::Subscribed {
            session: "sample".into(),
            offset: 90210,
            snapshot: Some(Snapshot {
                session: "sample".into(),
                offset: 90210,
                cols: 120,
                rows: 36,
                replay: B64(bytes),
                history_more: true,
                compression: None,
            }),
        }),
    }
}
fn payload(msg: &LinkMsg) -> (&[u8], Option<TerminalCompression>) {
    match msg {
        LinkMsg::Res { result: Outcome::Ok(AgentResponse::Subscribed { snapshot: Some(s), .. }), .. } => {
            (&s.replay.0, s.compression)
        }
        _ => panic!("snapshot 없음"),
    }
}

#[test]
fn exact_roundtrips_and_gain_thresholds() {
    assert!(!worthwhile(2048, 1793));
    assert!(worthwhile(2048, 1792));
    assert!(!worthwhile(4096, 3585));
    assert!(worthwhile(4096, 3584));
    assert!(!worthwhile(256, 257));
    let mut seed = 0x12345678u64;
    let noise: Vec<u8> = (0..32768)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    for bytes in
        [b"plain item 0001\r\n".repeat(2000), "\x1b[31m한글 줄 접힘\x1b[m\r\n".as_bytes().repeat(2000), noise.clone()]
    {
        for codec in [TerminalCodec::Gzip, TerminalCodec::Zstd] {
            let original = snapshot(bytes.clone());
            let packed: LinkMsg = serde_json::from_slice(&encode(original.clone(), Some(codec)).unwrap()).unwrap();
            let (data, compression) = payload(&packed);
            if let Some(meta) = compression {
                assert_eq!(inflate(data, meta, codec).unwrap(), bytes);
                if codec == TerminalCodec::Zstd {
                    assert_eq!(decode_link(packed, true).unwrap(), original);
                }
            } else {
                assert_eq!(bytes, noise);
            }
        }
    }
    for len in [4095, 4096] {
        let msg: LinkMsg =
            serde_json::from_slice(&encode(snapshot(vec![b'a'; len]), Some(TerminalCodec::Zstd)).unwrap()).unwrap();
        assert_eq!(payload(&msg).1.is_some(), len == 4096);
    }
}

#[test]
fn raw_and_frame_caps_apply_before_compression() {
    for codec in [None, Some(TerminalCodec::Gzip), Some(TerminalCodec::Zstd)] {
        assert!(encode(snapshot(vec![0; TERMINAL_PAYLOAD_MAX + 1]), codec).is_err());
        // 768 KiB의 base64만 1 MiB라 metadata가 들어갈 수 없다.
        assert!(encode(snapshot(vec![0; TERMINAL_PAYLOAD_MAX]), codec).is_err());
        assert!(encode(snapshot(vec![0; TERMINAL_PAYLOAD_MAX - 1024]), codec).is_ok());
    }
    assert!(decode_link(snapshot(vec![0; TERMINAL_PAYLOAD_MAX + 1]), false).is_err());
}

#[test]
fn malformed_crc_truncation_trailing_length_and_bombs_are_rejected() {
    for codec in [TerminalCodec::Gzip, TerminalCodec::Zstd] {
        let mut msg: LinkMsg =
            serde_json::from_slice(&encode(snapshot(vec![b'a'; 65536]), Some(codec)).unwrap()).unwrap();
        let (data, meta) = msg.payload().unwrap();
        let meta = meta.unwrap();
        assert!(inflate(&data.0, TerminalCompression { raw_len: 65535, ..meta }, codec).is_err());
        assert!(inflate(&data.0, TerminalCompression { raw_len: 65537, ..meta }, codec).is_err());
        assert!(
            inflate(&data.0, TerminalCompression { raw_len: TERMINAL_PAYLOAD_MAX as u32 + 1, ..meta }, codec).is_err()
        );
        assert!(
            inflate(
                &data.0,
                meta,
                if codec == TerminalCodec::Gzip { TerminalCodec::Zstd } else { TerminalCodec::Gzip }
            )
            .is_err()
        );
        for cut in 0..data.0.len() {
            assert!(inflate(&data.0[..cut], meta, codec).is_err(), "잘린 {codec:?}/{cut}");
        }
        let mut trailing = data.0.clone();
        trailing.push(0);
        assert!(inflate(&trailing, meta, codec).is_err());
        let mut concatenated = data.0.clone();
        concatenated.extend_from_slice(&data.0);
        assert!(inflate(&concatenated, meta, codec).is_err());
        assert!(inflate(&vec![0; TERMINAL_PAYLOAD_MAX + 1], meta, codec).is_err());
        if codec == TerminalCodec::Gzip {
            let mut crc = data.0.clone();
            let n = crc.len();
            crc[n - 8] ^= 1;
            assert!(inflate(&crc, meta, codec).is_err());
        }
    }
    // header만으로 큰 window/dictionary/skippable frame을 거부한다.
    let meta = TerminalCompression { codec: TerminalCodec::Zstd, raw_len: 1 };
    for header in
        [vec![0x28, 0xb5, 0x2f, 0xfd, 0, 0xff], vec![0x28, 0xb5, 0x2f, 0xfd, 1, 0], vec![0x50, 0x2a, 0x4d, 0x18, 0, 0]]
    {
        assert!(inflate(&header, meta, TerminalCodec::Zstd).is_err());
    }
    // 실제 16 MiB 폭탄도 raw_len 공간 밖으로 쓰지 못한다.
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(1));
    gzip.write_all(&vec![0; 16 << 20]).unwrap();
    let gzip = gzip.finish().unwrap();
    assert!(
        inflate(&gzip, TerminalCompression { codec: TerminalCodec::Gzip, raw_len: 1 }, TerminalCodec::Gzip).is_err()
    );
    let zstd = zstd::bulk::compress(&vec![0; 16 << 20], 1).unwrap();
    assert!(inflate(&zstd, meta, TerminalCodec::Zstd).is_err());
}

#[test]
fn negotiation_ipc_metadata_and_old_wire() {
    let original = snapshot(vec![b'a'; 8192]);
    let identity = encode(original.clone(), None).unwrap();
    assert!(!String::from_utf8_lossy(&identity).contains("compression"));
    assert_eq!(serde_json::from_slice::<LinkMsg>(&identity).unwrap(), original);
    let null_metadata =
        String::from_utf8(identity.clone()).unwrap().replace("\"replay\":", "\"compression\":null,\"replay\":");
    assert!(serde_json::from_str::<LinkMsg>(&null_metadata).is_err());
    let wire = encode(original, Some(TerminalCodec::Zstd)).unwrap();
    let packed: LinkMsg = serde_json::from_slice(&wire).unwrap();
    assert!(decode_link(packed.clone(), false).is_err());
    assert!(decode_link(packed.clone(), true).is_ok());
    if let LinkMsg::Res { mut result, .. } = packed {
        assert!(validate_ipc_response(&mut result).is_err());
    }
    for metadata in [
        r#"{"codec":"other","raw_len":1}"#,
        r#"{"codec":"zstd","raw_len":-1}"#,
        r#"{"codec":"zstd","raw_len":1.5}"#,
        r#"{"codec":"gzip"}"#,
    ] {
        assert!(serde_json::from_str::<TerminalCompression>(metadata).is_err());
    }
    let ping = LinkMsg::Ping { seq: 7 };
    assert_eq!(encode(ping.clone(), Some(TerminalCodec::Zstd)).unwrap(), serde_json::to_vec(&ping).unwrap());
}

#[tokio::test]
async fn real_writer_preserves_snapshot_binary_order_and_raw_offsets() {
    for negotiated in [false, true] {
        let (a, b) = tokio::io::duplex(1 << 20);
        let tx = spawn_writer(a, MAX_FRAME);
        tx.set_terminal_zstd(negotiated);
        let mut rx = FrameRx::new(b, MAX_FRAME);
        let original = snapshot(vec![b'a'; 32768]);
        tx.link(&original).await.unwrap();
        let chunk =
            TerminalChunk { session: "sample".into(), offset: 90210, data: "후속 출력".as_bytes().to_vec() };
        tx.terminal(&chunk).await.unwrap();
        tx.json(&LinkMsg::Pong { seq: 9 }).await.unwrap();
        let packed: LinkMsg = rx.control().await.unwrap();
        assert_eq!(payload(&packed).1.is_some(), negotiated);
        assert_eq!(decode_link(packed, negotiated).unwrap(), original);
        assert!(matches!(rx.next().await.unwrap(), Some(Incoming::Terminal(c)) if c == chunk));
        assert_eq!(rx.control::<LinkMsg>().await.unwrap(), LinkMsg::Pong { seq: 9 });
        drop(tx);
        assert!(rx.next().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn codec_slots_and_cancelled_jobs_keep_their_byte_permits_until_finished() {
    let (started, mut events) = tokio::sync::mpsc::unbounded_channel();
    let mut releases = Vec::new();
    let mut jobs = Vec::new();
    for id in 0..2 {
        let (release, wait) = std::sync::mpsc::channel();
        releases.push(release);
        let started = started.clone();
        jobs.push(tokio::spawn(blocking(16, move || {
            started.send(id).unwrap();
            wait.recv().unwrap();
            Ok(id)
        })));
    }
    let mut active = Vec::new();
    for _ in 0..2 {
        active.push(tokio::time::timeout(std::time::Duration::from_secs(3), events.recv()).await.unwrap().unwrap());
    }
    active.sort();
    assert_eq!(active, [0, 1]);
    assert_eq!(JOBS.available_permits(), 0);
    // spawn_blocking을 이미 시작한 future를 버려도 native 작업의 permit을 먼저 돌려주지 않는다.
    jobs[0].abort();
    assert_eq!(JOBS.available_permits(), 0);
    assert!(blocking(PENDING_BYTES, || Ok(())).await.is_err());
    for release in releases {
        release.send(()).unwrap();
    }
    for job in jobs {
        let _ = job.await;
    }
}
