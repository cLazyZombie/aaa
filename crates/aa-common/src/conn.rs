//! frame 연결 도우미: 읽는 쪽과, 여러 task가 함께 쓰는 쓰기 task.

use aa_proto::{FrameError, FrameKind, TerminalChunk, read_frame, write_frame};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncRead, AsyncWrite};
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
        let bytes = serde_json::to_vec(v).expect("JSON 직렬화");
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
                tracing::debug!("frame 쓰기 실패: {e}");
                break;
            }
        }
        use tokio::io::AsyncWriteExt;
        let _ = w.shutdown().await;
    });
    FrameTx { tx }
}

pub struct FrameRx<R> {
    r: R,
    max: usize,
}

impl<R: AsyncRead + Unpin> FrameRx<R> {
    pub fn new(r: R, max: usize) -> Self {
        FrameRx { r, max }
    }

    pub fn set_max(&mut self, max: usize) {
        self.max = max;
    }

    pub async fn next(&mut self) -> Result<Option<Incoming>, FrameError> {
        let Some(f) = read_frame(&mut self.r, self.max).await? else { return Ok(None) };
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
