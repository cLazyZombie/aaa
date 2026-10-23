//! 터미널 본문만 압축한다. 각 메시지는 독립 context이며 원문/주소는 계측하지 않는다.
use aaa_proto::{
    AgentResponse, B64, LinkMsg, MAX_FRAME, Outcome, TERMINAL_PAYLOAD_MAX, TermServerMsg, TerminalCodec,
    TerminalCompression,
};
use anyhow::{Result, bail, ensure};
use serde::Serialize;
use std::io::{Read, Write};
use tokio::sync::Semaphore;

pub const MIN_BYTES: usize = 4096;
pub const PENDING_BYTES: usize = 1 << 20;
// spawn_blocking에 일을 무제한 넣지 않는다. permit은 취소된 작업이 실제 끝날 때 반환한다.
static JOBS: Semaphore = Semaphore::const_new(2);
static BYTES: Semaphore = Semaphore::const_new(PENDING_BYTES);

pub trait PayloadMessage: Serialize + Send + 'static {
    fn payload(&mut self) -> Option<(&mut B64, &mut Option<TerminalCompression>)>;
}
impl PayloadMessage for LinkMsg {
    fn payload(&mut self) -> Option<(&mut B64, &mut Option<TerminalCompression>)> {
        let LinkMsg::Res { result: Outcome::Ok(body), .. } = self else { return None };
        match body {
            AgentResponse::Subscribed { snapshot: Some(s), .. } => Some((&mut s.replay, &mut s.compression)),
            AgentResponse::History(h) => Some((&mut h.data, &mut h.compression)),
            _ => None,
        }
    }
}
impl PayloadMessage for TermServerMsg {
    fn payload(&mut self) -> Option<(&mut B64, &mut Option<TerminalCompression>)> {
        match self {
            TermServerMsg::Snapshot { replay, compression, .. } => Some((replay, compression)),
            TermServerMsg::History { data, compression, .. } => Some((data, compression)),
            _ => None,
        }
    }
}

/// metadata를 포함한 최종 JSON에 두 이득 조건을 모두 적용한다.
pub fn worthwhile(identity: usize, compressed: usize) -> bool {
    identity.checked_sub(compressed).is_some_and(|saved| saved >= 256 && saved * 8 >= identity)
}

pub fn encode<M: PayloadMessage>(mut msg: M, codec: Option<TerminalCodec>) -> Result<Vec<u8>> {
    let raw_len = match msg.payload() {
        Some((data, compression)) => {
            ensure!(compression.is_none(), "이미 압축된 터미널 본문");
            ensure!(data.0.len() <= TERMINAL_PAYLOAD_MAX, "터미널 본문 상한");
            data.0.len()
        }
        None => return Ok(serde_json::to_vec(&msg)?),
    };
    let identity = serde_json::to_vec(&msg)?;
    ensure!(identity.len() < MAX_FRAME, "원시 터미널 JSON frame 상한");
    let Some(codec) = codec.filter(|_| raw_len >= MIN_BYTES) else {
        tracing::debug!(
            raw_len,
            wire_len = identity.len(),
            reason = if codec.is_none() { "identity_peer" } else { "small" },
            "터미널 전송"
        );
        return Ok(identity);
    };
    let started = std::time::Instant::now();
    let (data, compression) = msg.payload().unwrap();
    let packed = match codec {
        TerminalCodec::Gzip => {
            let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(1));
            encoder.write_all(&data.0)?;
            encoder.finish()?
        }
        TerminalCodec::Zstd => zstd::bulk::compress(&data.0, 1)?,
    };
    ensure!(packed.len() <= TERMINAL_PAYLOAD_MAX, "압축 본문 상한");
    data.0 = packed;
    *compression = Some(TerminalCompression { codec, raw_len: raw_len as u32 });
    let packed = serde_json::to_vec(&msg)?;
    let used = worthwhile(identity.len(), packed.len());
    tracing::debug!(
        ?codec,
        raw_len,
        identity_len = identity.len(),
        wire_len = if used { packed.len() } else { identity.len() },
        codec_us = started.elapsed().as_micros() as u64,
        reason = if used { "compressed" } else { "no_gain" },
        "터미널 전송"
    );
    Ok(if used { packed } else { identity })
}

async fn blocking<T: Send + 'static>(bytes: usize, work: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    ensure!(bytes <= PENDING_BYTES, "터미널 대기 바이트 상한");
    // 대기열 초과는 연결 오류다. 무한 대기하거나 별도 작업으로 재시도하지 않는다.
    let bytes = BYTES.try_acquire_many(bytes.max(1) as u32).map_err(|_| anyhow::anyhow!("터미널 대기열 상한"))?;
    let slot = JOBS.acquire().await?;
    tokio::task::spawn_blocking(move || {
        let (_bytes, _slot) = (bytes, slot);
        work()
    })
    .await?
}

/// 호출자는 연결별 writer에서 순서대로 호출한다. 상태 잠금은 넘기지 않는다.
pub async fn encode_async<M: PayloadMessage>(mut msg: M, codec: Option<TerminalCodec>) -> Result<Vec<u8>> {
    let Some((data, _)) = msg.payload() else { return Ok(serde_json::to_vec(&msg)?) };
    let bytes = data.0.len();
    blocking(bytes, move || encode(msg, codec)).await
}

/// 단일 zstd frame, dictionary 없음, 1 MiB 이하 window. bulk decoder의 출력 공간도 제한한다.
pub fn inflate(data: &[u8], meta: TerminalCompression, allowed: TerminalCodec) -> Result<Vec<u8>> {
    ensure!(meta.codec == allowed, "허용하지 않은 터미널 codec");
    let raw_len = meta.raw_len as usize;
    ensure!(raw_len <= TERMINAL_PAYLOAD_MAX && data.len() <= TERMINAL_PAYLOAD_MAX, "터미널 해제 상한");
    let out = match meta.codec {
        TerminalCodec::Zstd => {
            ensure!(data.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) && data.len() >= 6, "zstd frame header");
            let descriptor = data[4];
            ensure!(descriptor & 0x1b == 0, "zstd dictionary/reserved header");
            if descriptor & 0x20 == 0 {
                let window = data[5];
                let base = 1u64 << (10 + (window >> 3));
                ensure!(base + base / 8 * u64::from(window & 7) <= 1 << 20, "zstd window 상한");
            }
            ensure!(
                zstd::zstd_safe::find_frame_compressed_size(data).ok() == Some(data.len()),
                "zstd 잘림/남은 데이터"
            );
            if let Some(size) =
                zstd::zstd_safe::get_frame_content_size(data).map_err(|_| anyhow::anyhow!("zstd 크기 header"))?
            {
                ensure!(size == raw_len as u64, "zstd 원본 길이 불일치");
            }
            let mut decoder = zstd::bulk::Decompressor::new()?;
            decoder.set_parameter(zstd::zstd_safe::DParameter::WindowLogMax(20))?;
            let mut out = vec![0; raw_len];
            let n = decoder.decompress_to_buffer(data, out.as_mut_slice())?;
            ensure!(n == raw_len, "zstd 해제 길이 불일치");
            out
        }
        TerminalCodec::Gzip => {
            let mut decoder = flate2::bufread::GzDecoder::new(data);
            let mut out = Vec::with_capacity(raw_len.min(4096));
            decoder.by_ref().take(raw_len as u64 + 1).read_to_end(&mut out)?;
            ensure!(decoder.into_inner().is_empty(), "gzip 남은 데이터");
            out
        }
    };
    ensure!(out.len() == raw_len, "터미널 원본 길이 불일치");
    Ok(out)
}

pub fn decode_link(mut msg: LinkMsg, negotiated: bool) -> Result<LinkMsg> {
    if let Some((data, compression)) = msg.payload() {
        if let Some(meta) = compression.take() {
            ensure!(negotiated, "협상 없는 터미널 압축");
            let started = std::time::Instant::now();
            data.0 = inflate(&data.0, meta, TerminalCodec::Zstd)?;
            tracing::debug!(raw_len = data.0.len(), codec_us = started.elapsed().as_micros() as u64, "터미널 해제");
        } else if data.0.len() > TERMINAL_PAYLOAD_MAX {
            bail!("원시 터미널 본문 상한")
        }
        ensure!(serde_json::to_vec(&msg)?.len() < MAX_FRAME, "원시 터미널 JSON frame 상한");
    }
    Ok(msg)
}

pub async fn decode_link_async(mut msg: LinkMsg, negotiated: bool) -> Result<LinkMsg> {
    let Some((data, compression)) = msg.payload() else { return Ok(msg) };
    let bytes = compression.map_or(data.0.len(), |meta| meta.raw_len as usize).max(data.0.len());
    blocking(bytes, move || decode_link(msg, negotiated)).await.inspect_err(|_| tracing::warn!("터미널 본문 거부"))
}

/// session-host IPC에서 압축 필드를 먼저 거부한다(본문을 VT/감지에 넘기기 전).
pub fn validate_ipc_response(body: &mut Outcome<AgentResponse>) -> Result<()> {
    if let Outcome::Ok(body) = body {
        let payload = match body {
            AgentResponse::Subscribed { snapshot: Some(s), .. } => Some((&s.replay, s.compression)),
            AgentResponse::History(h) => Some((&h.data, h.compression)),
            _ => None,
        };
        if let Some((data, compression)) = payload {
            ensure!(compression.is_none() && data.0.len() <= TERMINAL_PAYLOAD_MAX, "IPC 터미널 본문 거부");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
