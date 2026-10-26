//! frame 연결 도우미: 읽는 쪽과, 여러 task가 함께 쓰는 쓰기 task.

use crate::terminal_payload::{PENDING_BYTES, PayloadMessage};
use aaa_proto::{FrameError, FrameKind, TerminalChunk, take_frame, write_frame};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::mpsc;

pub enum Incoming {
    Control(Vec<u8>),
    Terminal(TerminalChunk),
}

enum Out {
    Frame(FrameKind, Vec<u8>),
    Payload(Box<aaa_proto::LinkMsg>, tokio::sync::OwnedSemaphorePermit),
    Stop,
}

/// 쓰기 handle. 복제해서 여러 task가 쓴다. 연결이 끊기면 send가 실패한다.
#[derive(Clone)]
pub struct FrameTx {
    tx: mpsc::Sender<Out>,
    options: Arc<FrameOptions>,
    payload_bytes: Arc<tokio::sync::Semaphore>,
}

struct FrameOptions {
    max: usize,
    response_parts: AtomicBool,
    terminal_zstd: AtomicBool,
}

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("연결이 닫혔다")]
    Closed,
    #[error("frame 크기 {len}이 상한 {max}을 넘는다")]
    TooLarge { len: usize, max: usize },
}

impl FrameTx {
    /// 양쪽이 response_parts.v1을 알린 agent→host 연결에만 켠다.
    pub fn set_response_parts(&self, enabled: bool) {
        self.options.response_parts.store(enabled, Ordering::Relaxed);
    }

    fn check_size(&self, payload: usize) -> Result<(), SendError> {
        if payload >= self.options.max {
            return Err(SendError::TooLarge { len: payload.saturating_add(1), max: self.options.max });
        }
        Ok(())
    }
    /// 원격 Hello/Welcome의 기능 교집합에서만 켠다. 매 연결의 초기값은 false다.
    pub fn set_terminal_zstd(&self, enabled: bool) {
        self.options.terminal_zstd.store(enabled, Ordering::Relaxed);
    }

    /// 터미널 응답의 codec은 writer가 앞뒤 출력과 같은 차례에 실행한다.
    pub async fn link(&self, msg: &aaa_proto::LinkMsg) -> Result<(), SendError> {
        let mut msg = msg.clone();
        let Some((data, _)) = msg.payload() else { return self.link_control(&msg).await };
        let bytes = data.0.len();
        if bytes > aaa_proto::TERMINAL_PAYLOAD_MAX {
            crate::diagnostic::ignored(self.tx.send(Out::Stop).await, "수신자가 끝난 뒤에도 현재 작업은 마친다");
            return Err(SendError::Closed);
        }
        let Ok(permit) = self
            .payload_bytes
            .clone()
            .try_acquire_many_owned(bytes.max(1) as u32)
            .inspect_err(|error| crate::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            crate::diagnostic::ignored(self.tx.send(Out::Stop).await, "수신자가 끝난 뒤에도 현재 작업은 마친다");
            return Err(SendError::Closed);
        };
        self.tx.send(Out::Payload(Box::new(msg), permit)).await.map_err(|_| SendError::Closed)
    }

    async fn link_control(&self, msg: &aaa_proto::LinkMsg) -> Result<(), SendError> {
        use crate::response_parts::{PART_BYTES, RESPONSE_BYTES};
        use aaa_proto::{B64, ErrorBody, ErrorCode, LinkMsg, Outcome};
        let bytes = serde_json::to_vec(msg).expect("JSON 직렬화");
        if bytes.len() < self.options.max {
            return self.control(bytes).await;
        }
        let LinkMsg::Res { id, .. } = msg else {
            tracing::warn!(bytes = bytes.len() + 1, max = self.options.max, "응답 외 큰 메시지 전송 거부(연결 유지)");
            return self.control(bytes).await;
        };
        let kind = serde_json::to_value(msg)
            .ok()
            .and_then(|v| v.pointer("/result/ok/kind").and_then(|v| v.as_str()).map(str::to_owned))
            .unwrap_or_else(|| "error".into());
        if self.options.response_parts.load(Ordering::Relaxed) && bytes.len() <= RESPONSE_BYTES {
            tracing::debug!(%id, kind, bytes = bytes.len(), "응답 분할 전송");
            for (index, chunk) in bytes.chunks(PART_BYTES).enumerate() {
                self.json(&LinkMsg::ResPart {
                    id: id.clone(),
                    offset: (index * PART_BYTES) as u32,
                    total: bytes.len() as u32,
                    data: B64(chunk.to_vec()),
                })
                .await?;
            }
            return Ok(());
        }
        let message = format!(
            "agent {kind} 응답 {}바이트가 전송 상한을 넘는다(frame {}, 분할 응답 {}, 분할 협상 {}). 범위를 줄이거나 양쪽을 갱신한다",
            bytes.len() + 1,
            self.options.max,
            RESPONSE_BYTES,
            self.options.response_parts.load(Ordering::Relaxed),
        );
        tracing::warn!(%id, kind, bytes = bytes.len() + 1, max = self.options.max, "큰 응답을 too_large로 거부(연결 유지)");
        self.json(&LinkMsg::Res { id: id.clone(), result: Outcome::Err(ErrorBody::new(ErrorCode::TooLarge, message)) })
            .await
    }

    pub async fn json<T: Serialize>(&self, v: &T) -> Result<(), SendError> {
        self.control(serde_json::to_vec(v).expect("JSON 직렬화")).await
    }

    /// 이미 JSON으로 만든 제어 메시지(크기를 먼저 본 응답 등)
    pub async fn control(&self, bytes: Vec<u8>) -> Result<(), SendError> {
        self.check_size(bytes.len())?;
        self.tx.send(Out::Frame(FrameKind::Control, bytes)).await.map_err(|_| SendError::Closed)
    }

    pub async fn terminal(&self, c: &TerminalChunk) -> Result<(), SendError> {
        let bytes = c.encode();
        self.check_size(bytes.len())?;
        self.tx.send(Out::Frame(FrameKind::Terminal, bytes)).await.map_err(|_| SendError::Closed)
    }

    /// 기다리지 않는다. 쓰기 대기열이 차 있으면 false(느린 상대: 재동기화가 필요하다).
    pub fn try_terminal(&self, c: &TerminalChunk) -> Result<bool, SendError> {
        let bytes = c.encode();
        self.check_size(bytes.len())?;
        match self.tx.try_send(Out::Frame(FrameKind::Terminal, bytes)) {
            Ok(()) => Ok(true),
            Err(mpsc::error::TrySendError::Full(_)) => Ok(false),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(SendError::Closed),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// 쓰기 task를 띄운다. 연결이 실패하거나 모든 handle이 사라지면 끝난다.
pub fn spawn_writer<W: AsyncWrite + Unpin + Send + 'static>(mut w: W, max: usize) -> FrameTx {
    let (tx, mut rx) = mpsc::channel::<Out>(512);
    let options =
        Arc::new(FrameOptions { max, response_parts: AtomicBool::new(false), terminal_zstd: AtomicBool::new(false) });
    let mode = options.clone();
    tokio::spawn(async move {
        while let Some(out) = rx.recv().await {
            let (kind, payload, _permit) = match out {
                Out::Stop => break,
                Out::Frame(kind, payload) => (kind, payload, None),
                Out::Payload(msg, permit) => {
                    let codec = mode.terminal_zstd.load(Ordering::Relaxed).then_some(aaa_proto::TerminalCodec::Zstd);
                    match crate::terminal_payload::encode_async(*msg, codec).await {
                        Ok(bytes) => (FrameKind::Control, bytes, Some(permit)),
                        Err(source_error) => {
                            tracing::warn!(error = %crate::diagnostic::Cause::capture(&source_error), "터미널 전송 본문 거부");
                            break;
                        }
                    }
                }
            };
            if let Err(e) = write_frame(&mut w, kind, &payload, max).await {
                // 길이 검사는 wire 쓰기 전 끝난다. 잘못된 한 frame이 다른 요청까지 끊지 않는다.
                if matches!(e, FrameError::TooLarge { .. }) {
                    tracing::warn!("frame을 보내지 않고 연결을 유지한다: {e}");
                    continue;
                } else {
                    tracing::debug!("frame 쓰기 실패: {e}");
                }
                break;
            }
        }
        use tokio::io::AsyncWriteExt;
        crate::diagnostic::ignored(w.shutdown().await, "정리 실패가 본 작업의 결과를 덮지 않게 한다");
    });
    FrameTx { tx, options, payload_bytes: Arc::new(tokio::sync::Semaphore::new(PENDING_BYTES)) }
}

/// 쓰기 task 없이 handle과 그 줄을 만든다. 줄을 비우는 쪽이 frame을 옮긴다(WebSocket 위 Front API는 제어 frame 하나를
/// 글 메시지 하나로 보낸다, plan §6.2). handle이 모두 사라지면 줄이 끝난다.
pub fn frame_channel() -> (FrameTx, FrameOut) {
    let (tx, rx) = mpsc::channel::<Out>(512);
    (
        FrameTx {
            tx,
            options: Arc::new(FrameOptions {
                max: aaa_proto::MAX_FRAME,
                response_parts: AtomicBool::new(false),
                terminal_zstd: AtomicBool::new(false),
            }),
            payload_bytes: Arc::new(tokio::sync::Semaphore::new(PENDING_BYTES)),
        },
        FrameOut { rx },
    )
}

/// `frame_channel`의 받는 쪽
pub struct FrameOut {
    rx: mpsc::Receiver<Out>,
}

impl FrameOut {
    /// 다음 frame. handle이 모두 사라지면 None
    pub async fn recv(&mut self) -> Option<(FrameKind, Vec<u8>)> {
        match self.rx.recv().await? {
            Out::Stop => None,
            Out::Frame(kind, payload) => Some((kind, payload)),
            Out::Payload(msg, _permit) => crate::terminal_payload::encode_async(*msg, None)
                .await
                .inspect_err(|error| crate::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
                .ok()
                .map(|bytes| (FrameKind::Control, bytes)),
        }
    }
}

/// 한 번에 읽어 오는 크기(session host의 터미널 frame은 32KiB까지)
const READ_CHUNK: usize = 64 << 10;

pub struct FrameRx<R> {
    r: R,
    max: usize,
    /// 아직 frame이 되지 않은 바이트. `next`는 `select!` 안에서 취소될 수 있어(agent pump) 읽은 것을 여기 둔다
    buf: Vec<u8>,
}

impl<R: AsyncRead + Unpin> FrameRx<R> {
    pub fn new(r: R, max: usize) -> Self {
        FrameRx { r, max, buf: Vec::new() }
    }

    pub fn set_max(&mut self, max: usize) {
        self.max = max;
    }

    /// 다음 frame. 연결이 frame 경계에서 닫히면 `Ok(None)`. 취소에 안전하다: 기다리는 곳은 `read_buf`(취소되면 읽은 것이
    /// 없다)뿐이고 읽은 바이트는 `buf`에 남아, 다음 `next`가 이어 모은다
    pub async fn next(&mut self) -> Result<Option<Incoming>, FrameError> {
        let f = loop {
            if let Some(f) = take_frame(&mut self.buf, self.max)? {
                break f;
            }
            self.buf.reserve(READ_CHUNK);
            if self.r.read_buf(&mut self.buf).await? == 0 {
                return if self.buf.is_empty() { Ok(None) } else { Err(FrameError::Truncated) };
            }
        };
        Ok(Some(match f.kind {
            FrameKind::Control => Incoming::Control(f.payload),
            FrameKind::Terminal => Incoming::Terminal(TerminalChunk::decode(&f.payload)?),
        }))
    }

    /// 다음 제어 메시지를 읽는다(handshake 단계용). 터미널 frame이 오면 오류다.
    pub async fn control<T: DeserializeOwned>(&mut self) -> anyhow::Result<T> {
        match self.next().await? {
            Some(Incoming::Control(b)) => Ok(serde_json::from_slice(&b)?),
            Some(Incoming::Terminal(_)) => anyhow::bail!("handshake 중 터미널 frame"),
            None => anyhow::bail!("상대가 연결을 닫았다"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    /// frame이 반쯤 온 동안 `select!`의 다른 갈래가 이겨도(agent pump: host 요청·HostGone·감지 timer) 다음 `next`가 그 frame을
    /// 온전히 받는다. 옛 `read_frame`은 읽던 앞부분을 버려, 다음 읽기가 터미널 글을 길이로 읽고 끝났다(07 「host를 다시 띄운 뒤
    /// 앞 기록」에서 살아 있는 세션이 「세션이 끝났다」가 됨)
    #[tokio::test]
    async fn next_keeps_a_frame_cut_by_another_select_branch() {
        let chunk = TerminalChunk { session: "s1".into(), offset: 7, data: b"h-00001\r\n".repeat(40) };
        let mut wire = Vec::new();
        write_frame(&mut wire, FrameKind::Terminal, &chunk.encode(), 1 << 20).await.unwrap();
        write_frame(&mut wire, FrameKind::Control, b"{\"t\":1}", 1 << 20).await.unwrap();
        // 길이·종류·payload 어디서 끊겨도
        for cut in 1..wire.len() {
            let (mut a, b) = tokio::io::duplex(1 << 20);
            let mut rx = FrameRx::new(b, 1 << 20);
            a.write_all(&wire[..cut]).await.unwrap();
            let first = tokio::select! {
                biased;
                r = rx.next() => Some(r),
                _ = std::future::ready(()) => None,
            };
            // 바로 다 읽을 수 있는 앞 frame은 이 판에 끝날 수 있다(그때도 같은 frame이어야 한다)
            let mut got = Vec::new();
            if let Some(r) = first {
                got.push(r.unwrap().unwrap());
            }
            a.write_all(&wire[cut..]).await.unwrap();
            drop(a);
            while let Some(m) = rx.next().await.unwrap_or_else(|e| panic!("{cut}바이트에서 끊긴 뒤: {e}")) {
                got.push(m);
            }
            assert!(
                matches!(got.as_slice(), [Incoming::Terminal(c), Incoming::Control(j)] if *c == chunk && j.as_slice() == b"{\"t\":1}"),
                "{cut}바이트에서 끊긴 뒤 받은 frame {}개",
                got.len()
            );
        }
    }
}
