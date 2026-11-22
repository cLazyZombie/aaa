//! Front API 클라이언트(UDS·Named Pipe).
//! CLI는 host의 같은 머신 socket으로만 연결한다.
//! front는 host 내부에 의존하지 않고 이 계약(aaa-proto)으로만 host와 말한다(결정 13).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::Duration;

use aaa_proto::{
    ATTACH_CHUNK, B64, DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontEvent, FrontMsg, FrontRequest, FrontResponse,
    MAX_FRAME, Version,
};
use anyhow::{Context, Result, bail};
use tokio::time::{Instant, timeout};

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

const IO_TIMEOUT: Duration = Duration::from_secs(10);
const UNSENT: &str = "이번 요청 미전송";
const UNKNOWN: &str = "결과 미확정 · 다시 실행하기 전에 상태를 확인한다";

#[derive(Debug, thiserror::Error)]
#[error("연결 준비 실패 ({elapsed_ms}ms) · {UNSENT} · host socket({path})")]
struct ConnectError {
    elapsed_ms: u128,
    path: PathBuf,
    #[source]
    source: std::io::Error,
}

impl ConnectError {
    fn is_absent(&self) -> bool {
        matches!(self.source.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused)
    }
}

/// 로컬 host가 없는 경우에만 agent 경로를 고를 수 있다. 인사 실패는 호출자가 보아야 한다.
pub fn host_absent(error: &anyhow::Error) -> bool {
    error.downcast_ref::<ConnectError>().is_some_and(ConnectError::is_absent)
}

/// 조작 권한이 필요 없는 요청도 기록을 바꾸는 경우가 있다.
fn may_change_state(body: &FrontRequest) -> bool {
    body.needs_control()
        || body.design_relay_write()
        || matches!(
            body,
            FrontRequest::LibraryOpen { .. }
                | FrontRequest::LibraryMarkRead { .. }
                | FrontRequest::InstallPublish { .. }
        )
}

impl FrontClient {
    pub async fn connect(state_root: &Path) -> Result<FrontClient> {
        Self::connect_as(state_root, CLI_CLIENT).await
    }

    pub async fn connect_as(state_root: &Path, client: &str) -> Result<FrontClient> {
        let path = socket(state_root);
        let started = Instant::now();
        let connection = timeout(IO_TIMEOUT, crate::ipc::connect(&path))
            .await
            .with_context(|| format!("연결 준비 시간 초과 ({}ms) · {UNSENT}", started.elapsed().as_millis()))?;
        let s = connection.map_err(|source| {
            let error = ConnectError { elapsed_ms: started.elapsed().as_millis(), path, source };
            if error.is_absent() {
                anyhow::Error::new(error).context("host가 없다. host 머신에서 `aaa host`가 실행 중인지 본다")
            } else {
                anyhow::Error::new(error)
            }
        })?;
        let (r, w) = tokio::io::split(s);
        let tx = spawn_writer(w, MAX_FRAME);
        let rx = Inbound::Local(FrameRx::new(r, MAX_FRAME));
        // aaa 세션 안에서 실행했으면 그 세션을 알린다(사후 평가자 세션은 보기만 한다, §8.13)
        let session = std::env::var("AAA_SESSION_ID")
            .inspect_err(|error| crate::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .filter(|s| !s.is_empty());
        Self::hello(tx, rx, client, session).await
    }

    async fn hello(tx: FrameTx, mut rx: Inbound, client: &str, session: Option<String>) -> Result<FrontClient> {
        let started = Instant::now();
        let hello = timeout(IO_TIMEOUT, async {
            tx.json(&FrontMsg::Hello { proto: FRONT_API, client: client.into(), session }).await?;
            rx.control().await
        })
        .await
        .with_context(|| format!("첫 응답 시간 초과 ({}ms) · {UNSENT}", started.elapsed().as_millis()))?
        .with_context(|| format!("첫 응답 실패 ({}ms) · {UNSENT}", started.elapsed().as_millis()))?;
        let (front, proto, role) = match hello {
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
        let start = match &body {
            FrontRequest::ThreadStartAccount { start, .. } => &**start,
            other => other,
        };
        if matches!(start, FrontRequest::ThreadStart { over_limit: true, .. }) && self.proto.minor < 98 {
            bail!("한도 무시 시작에는 Front API 1.98 이상 host가 필요하다");
        }

        let change = may_change_state(&body);
        let started = Instant::now();
        let id = timeout(IO_TIMEOUT, Box::pin(self.send(body)))
            .await
            .with_context(|| format!("요청 전송 시간 초과 ({}ms) · {UNSENT}", started.elapsed().as_millis()))?
            .with_context(|| format!("요청 전송 실패 ({}ms) · {UNSENT}", started.elapsed().as_millis()))?;
        let started = Instant::now();
        let status = if change { format!(" · {UNKNOWN}") } else { String::new() };
        loop {
            let response =
                self.rx.control().await.with_context(|| {
                    format!("요청 처리 응답 수신 실패 ({}ms){status}", started.elapsed().as_millis())
                })?;
            match response {
                FrontMsg::Res { id: rid, result } if rid == id => {
                    return Ok(result.into_result().map_err(|mut error| {
                        if error.code == ErrorCode::Timeout {
                            error.message = format!(
                                "요청 처리 시간 초과 ({}ms){status} · {}",
                                started.elapsed().as_millis(),
                                error.message
                            );
                        }
                        error
                    }));
                }
                FrontMsg::Event(e) => self.events.push_back(e),
                _ => continue,
            }
        }
    }

    /// 응답을 기다리지 않고 보낸다(응답은 `next`로 받는다). 요청 ID를 돌려준다.
    pub async fn send(&mut self, body: FrontRequest) -> Result<String> {
        if matches!(&body, FrontRequest::MachineSetupApply { service_password: Some(_), .. }) {
            bail!("서비스 암호는 CLI로 보내지 않는다. 사람이 웹 확인 창의 전용 입력칸에 직접 넣는다");
        }
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

#[cfg(test)]
mod tests;
