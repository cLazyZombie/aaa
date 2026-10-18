//! frame 연결 도우미: 읽는 쪽과, 여러 task가 함께 쓰는 쓰기 task.

use aaa_proto::{FrameError, FrameKind, TerminalChunk, take_frame, write_frame};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::mpsc;

pub enum Incoming {
    Control(Vec<u8>),
    Terminal(TerminalChunk),
}

enum Out {
    Frame(FrameKind, Vec<u8>),
}

/// 쓰기 handle. 복제해서 여러 task가 쓴다. 연결이 끊기면 send가 실패한다.
#[derive(Clone)]
pub struct FrameTx {
    tx: mpsc::Sender<Out>,
}

#[derive(Debug, thiserror::Error)]
#[error("연결이 닫혔다")]
pub struct Closed;

impl FrameTx {
    pub async fn json<T: Serialize>(&self, v: &T) -> Result<(), Closed> {
        self.control(serde_json::to_vec(v).expect("JSON 직렬화")).await
    }

    /// 이미 JSON으로 만든 제어 메시지(크기를 먼저 본 응답 등)
    pub async fn control(&self, bytes: Vec<u8>) -> Result<(), Closed> {
        self.tx.send(Out::Frame(FrameKind::Control, bytes)).await.map_err(|_| Closed)
    }

    pub async fn terminal(&self, c: &TerminalChunk) -> Result<(), Closed> {
        self.tx.send(Out::Frame(FrameKind::Terminal, c.encode())).await.map_err(|_| Closed)
    }

    /// 기다리지 않는다. 쓰기 대기열이 차 있으면 false(느린 상대: 재동기화가 필요하다).
    pub fn try_terminal(&self, c: &TerminalChunk) -> Result<bool, Closed> {
        match self.tx.try_send(Out::Frame(FrameKind::Terminal, c.encode())) {
            Ok(()) => Ok(true),
            Err(mpsc::error::TrySendError::Full(_)) => Ok(false),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(Closed),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// 쓰기 task를 띄운다. 연결이 실패하거나 모든 handle이 사라지면 끝난다.
pub fn spawn_writer<W: AsyncWrite + Unpin + Send + 'static>(mut w: W, max: usize) -> FrameTx {
    let (tx, mut rx) = mpsc::channel::<Out>(512);
    tokio::spawn(async move {
        while let Some(Out::Frame(kind, payload)) = rx.recv().await {
            if let Err(e) = write_frame(&mut w, kind, &payload, max).await {
                // 상한을 넘는 frame은 보낸 쪽 결함이다: 연결이 닫히므로 상대가 까닭을 알 수 없어 경고로 남긴다
                if matches!(e, FrameError::TooLarge { .. }) {
                    tracing::warn!("frame을 보내지 못해 연결을 닫는다: {e}");
                } else {
                    tracing::debug!("frame 쓰기 실패: {e}");
                }
                break;
            }
        }
        use tokio::io::AsyncWriteExt;
        let _ = w.shutdown().await;
    });
    FrameTx { tx }
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
