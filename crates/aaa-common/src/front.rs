//! Front API 클라이언트(UDS·Named Pipe).
//! CLI는 host의 같은 머신 socket으로만 연결한다.
//! front는 host 내부에 의존하지 않고 이 계약(aaa-proto)으로만 host와 말한다(결정 13).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aaa_proto::{
    ATTACH_CHUNK, B64, DeviceRole, ErrorBody, FRONT_API, FrontEvent, FrontMsg, FrontRequest, FrontResponse, MAX_FRAME,
    Version,
};
use anyhow::{Context, Result, bail};

use crate::conn::{FrameRx, FrameTx, Incoming, spawn_writer};

/// host가 여는 Front socket 위치(상태 폴더 아래 `host/front.sock`)
pub fn socket(state_root: &Path) -> PathBuf {
    state_root.join("host").join("front.sock")
}

/// host에서 오는 같은 머신 socket의 frame
enum Inbound {
    Local(FrameRx<tokio::io::ReadHalf<crate::ipc::LocalStream>>),
}

impl Inbound {
    /// 다음 제어 메시지(JSON). 연결이 끝나면 None. 취소에 안전하다
    async fn next(&mut self) -> Result<Option<Vec<u8>>> {
        match self {
            Inbound::Local(rx) => loop {
                match rx.next().await? {
                    None => return Ok(None),
                    Some(Incoming::Control(b)) => return Ok(Some(b)),
                    Some(Incoming::Terminal(_)) => continue,
                }
            },
        }
    }

    async fn control(&mut self) -> Result<FrontMsg> {
        let b = self.next().await?.context("host가 연결을 닫았다")?;
        Ok(serde_json::from_slice(&b)?)
    }
}

/// CLI 쪽 클라이언트. 요청 응답을 기다리는 동안 온 사건은 모아 둔다.
pub struct FrontClient {
    tx: FrameTx,
    rx: Inbound,
    seq: u64,
    events: VecDeque<FrontEvent>,
    /// host가 준 이 연결의 front ID
    pub front: String,
    /// host와 합의한 Front API 판(옛 host면 minor가 낮다: 새 요청을 보내기 전에 본다)
    pub proto: Version,
    /// host가 정한 이 연결의 권한(thread 세션 안 CLI·보기 기기는 Reader)
    pub role: DeviceRole,
}

/// 삭제된 TUI를 실행하거나 옛 바이너리가 접속할 때의 안내.
pub const TUI_REMOVED: &str = "TUI는 삭제됐다. 웹 UI를 쓴다. host에서 `aaa web status`로 주소를 확인한다.";

/// 한 번 보내고 끝나는 CLI 연결의 이름. 대화형 front(웹)와 달리 「지금 쓰는 화면」이 되지 않는다.
pub const CLI_CLIENT: &str = "cli";

impl FrontClient {
    pub async fn connect(state_root: &Path) -> Result<FrontClient> {
        Self::connect_as(state_root, CLI_CLIENT).await
    }

    pub async fn connect_as(state_root: &Path, client: &str) -> Result<FrontClient> {
        let path = socket(state_root);
        let s = crate::ipc::connect(&path)
            .await
            .with_context(|| format!("host가 없다({}). host 머신에서 `aaa host`가 실행 중인지 본다", path.display()))?;
        let (r, w) = tokio::io::split(s);
        let tx = spawn_writer(w, MAX_FRAME);
        let rx = Inbound::Local(FrameRx::new(r, MAX_FRAME));
        // aaa 세션 안에서 실행했으면 그 세션을 알린다(사후 평가자 세션은 보기만 한다, §8.13)
        let session = std::env::var("AAA_SESSION_ID").ok().filter(|s| !s.is_empty());
        Self::hello(tx, rx, client, session).await
    }

    async fn hello(tx: FrameTx, mut rx: Inbound, client: &str, session: Option<String>) -> Result<FrontClient> {
        tx.json(&FrontMsg::Hello { proto: FRONT_API, client: client.into(), session }).await?;
        let (front, proto, role) = match tokio::time::timeout(Duration::from_secs(10), rx.control()).await?? {
            FrontMsg::Welcome { front, proto, role, warnings, .. } => {
                for w in warnings {
                    eprintln!("⚠ {w}");
                }
                (front, proto, role)
            }
            FrontMsg::Reject(e) => bail!("host가 거부했다: {e}"),
            other => bail!("예상하지 못한 응답: {other:?}"),
        };
        Ok(FrontClient { tx, rx, seq: 0, events: VecDeque::new(), front, proto, role })
    }

    pub async fn call(&mut self, body: FrontRequest) -> Result<Result<FrontResponse, ErrorBody>> {
        let id = self.send(body).await?;
        loop {
            match self.rx.control().await? {
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

    /// 다음 메시지(사건 또는 응답). 모아 둔 사건을 먼저 준다. 연결이 끝나면 None.
    pub async fn next(&mut self) -> Result<Option<FrontMsg>> {
        if let Some(e) = self.events.pop_front() {
            return Ok(Some(FrontMsg::Event(e)));
        }
        match self.rx.next().await? {
            None => Ok(None),
            Some(b) => Ok(Some(serde_json::from_slice(&b)?)),
        }
    }
}

/// 그림 한 개를 `aaa pane attach` 조각 요청들로 나눈다. 차례대로 보낸다.
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
