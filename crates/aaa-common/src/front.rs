//! Front API 같은 머신 경로(UDS·Named Pipe)의 클라이언트. CLI와 TUI(`aaa attach`)가 쓴다.
//! front는 host 내부에 의존하지 않고 이 계약(aaa-proto)으로만 host와 말한다(결정 13).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aaa_proto::{
    ATTACH_CHUNK, B64, ErrorBody, FRONT_API, FrontEvent, FrontMsg, FrontRequest, FrontResponse, MAX_FRAME,
};
use anyhow::{Context, Result, bail};

use crate::conn::{FrameRx, FrameTx, Incoming, spawn_writer};

/// host가 여는 Front socket 위치(상태 폴더 아래 `host/front.sock`)
pub fn socket(state_root: &Path) -> PathBuf {
    state_root.join("host").join("front.sock")
}

/// CLI·TUI 쪽 클라이언트. 요청 응답을 기다리는 동안 온 사건은 모아 둔다.
pub struct FrontClient {
    tx: FrameTx,
    rx: FrameRx<tokio::io::ReadHalf<crate::ipc::LocalStream>>,
    seq: u64,
    events: VecDeque<FrontEvent>,
    /// host가 준 이 연결의 front ID
    pub front: String,
}

impl FrontClient {
    pub async fn connect(state_root: &Path) -> Result<FrontClient> {
        Self::connect_as(state_root, "cli").await
    }

    pub async fn connect_as(state_root: &Path, client: &str) -> Result<FrontClient> {
        let path = socket(state_root);
        let s = crate::ipc::connect(&path)
            .await
            .with_context(|| format!("host가 없다({}). host 머신에서 `aaa host`가 실행 중인지 본다", path.display()))?;
        let (r, w) = tokio::io::split(s);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        // aaa 세션 안에서 실행했으면 그 세션을 알린다(사후 평가자 세션은 보기만 한다, §8.13)
        let session = std::env::var("AAA_SESSION_ID").ok().filter(|s| !s.is_empty());
        tx.json(&FrontMsg::Hello { proto: FRONT_API, client: client.into(), session }).await?;
        let front = match tokio::time::timeout(Duration::from_secs(10), rx.control::<FrontMsg>()).await?? {
            FrontMsg::Welcome { front, .. } => front,
            FrontMsg::Reject(e) => bail!("host가 거부했다: {e}"),
            other => bail!("예상하지 못한 응답: {other:?}"),
        };
        Ok(FrontClient { tx, rx, seq: 0, events: VecDeque::new(), front })
    }

    pub async fn call(&mut self, body: FrontRequest) -> Result<Result<FrontResponse, ErrorBody>> {
        let id = self.send(body).await?;
        loop {
            match self.rx.control::<FrontMsg>().await? {
                FrontMsg::Res { id: rid, result } if rid == id => return Ok(result.into_result()),
                FrontMsg::Event(e) => self.events.push_back(e),
                _ => continue,
            }
        }
    }

    /// 응답을 기다리지 않고 보낸다(응답은 `next`로 받는다). 요청 ID를 돌려준다.
    pub async fn send(&mut self, body: FrontRequest) -> Result<String> {
        self.seq += 1;
        let id = format!("q{}", self.seq);
        self.tx.json(&FrontMsg::Req { id: id.clone(), body }).await?;
        Ok(id)
    }

    /// 보내는 쪽과 받는 쪽으로 나눈다. 받기는 task가 하므로 select!에서 기다려도 frame이 잘리지 않는다.
    pub fn split(self) -> (FrontSender, tokio::sync::mpsc::UnboundedReceiver<FrontMsg>) {
        let (etx, erx) = tokio::sync::mpsc::unbounded_channel();
        for e in self.events {
            let _ = etx.send(FrontMsg::Event(e));
        }
        let mut rx = self.rx;
        tokio::spawn(async move {
            while let Ok(Some(m)) = rx.next().await {
                if let Incoming::Control(b) = m
                    && let Ok(msg) = serde_json::from_slice::<FrontMsg>(&b)
                    && etx.send(msg).is_err()
                {
                    break;
                }
            }
        });
        (FrontSender { tx: self.tx, seq: self.seq, front: self.front }, erx)
    }

    /// 다음 메시지(사건 또는 응답). 모아 둔 사건을 먼저 준다. 연결이 끝나면 None.
    pub async fn next(&mut self) -> Result<Option<FrontMsg>> {
        if let Some(e) = self.events.pop_front() {
            return Ok(Some(FrontMsg::Event(e)));
        }
        loop {
            match self.rx.next().await? {
                None => return Ok(None),
                Some(Incoming::Control(b)) => return Ok(Some(serde_json::from_slice(&b)?)),
                Some(Incoming::Terminal(_)) => continue,
            }
        }
    }
}

/// `FrontClient::split`의 보내는 쪽
pub struct FrontSender {
    tx: FrameTx,
    seq: u64,
    pub front: String,
}

impl FrontSender {
    pub async fn send(&mut self, body: FrontRequest) -> Result<String> {
        self.seq += 1;
        let id = format!("q{}", self.seq);
        self.tx.json(&FrontMsg::Req { id: id.clone(), body }).await?;
        Ok(id)
    }
}

/// 그림 한 개를 `PaneAttach` 조각 요청들로 나눈다(TUI Ctrl+V·`aaa pane attach`). 차례대로 보낸다.
pub fn attach_requests(pane: &str, bytes: &[u8]) -> Vec<FrontRequest> {
    use sha2::Digest;
    let sha256 = hex::encode(sha2::Sha256::digest(bytes));
    let total = bytes.len() as u64;
    bytes
        .chunks(ATTACH_CHUNK)
        .enumerate()
        .map(|(i, c)| FrontRequest::PaneAttach {
            pane: pane.to_string(),
            offset: (i * ATTACH_CHUNK) as u64,
            total,
            sha256: sha256.clone(),
            data: B64(c.to_vec()),
        })
        .collect()
}
