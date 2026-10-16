//! Front API의 같은 머신 경로(UDS, peer UID 확인 → Local 역할). CLI가 쓴다.
//! front는 backend를 띄우지 않는다: host가 없으면 「host 없음」만 알린다(§4.2 2).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aa_proto::{
    DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontMsg, FrontRequest, FrontResponse, MAX_FRAME, negotiate,
};
use anyhow::{Context, Result, bail};

use super::Host;
use crate::conn::{FrameRx, FrameTx, Incoming, spawn_writer};

pub fn socket(state_root: &Path) -> PathBuf {
    state_root.join("host").join("front.sock")
}

pub async fn start(host: &Arc<Host>) -> Result<()> {
    let path = host.state.join("front.sock");
    let listener = crate::ipc::LocalListener::bind(&path).context("front socket")?;
    let host = host.clone();
    tokio::spawn(async move {
        while let Ok(s) = listener.accept().await {
            let host = host.clone();
            tokio::spawn(async move {
                if let Err(e) = serve(host, s).await {
                    tracing::debug!("front 연결 끝: {e:#}");
                }
            });
        }
    });
    Ok(())
}

async fn serve(host: Arc<Host>, s: crate::ipc::LocalStream) -> Result<()> {
    let (r, w) = tokio::io::split(s);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let hello: FrontMsg = tokio::time::timeout(Duration::from_secs(10), rx.control()).await??;
    let FrontMsg::Hello { proto, client } = hello else { bail!("hello가 와야 한다") };
    let agreed = match negotiate(FRONT_API, proto) {
        Ok(v) => v,
        Err(e) => {
            tx.json(&FrontMsg::Reject(ErrorBody::new(ErrorCode::UnsupportedVersion, e.to_string()))).await?;
            bail!("{e}");
        }
    };
    tx.json(&FrontMsg::Welcome { proto: agreed, host_id: host.host_id.clone(), role: DeviceRole::Local }).await?;
    let actor = format!("local:{client}");
    while let Some(m) = rx.next().await? {
        let Incoming::Control(b) = m else { continue };
        if let FrontMsg::Req { id, body } = serde_json::from_slice(&b)? {
            let (host, tx, actor) = (host.clone(), tx.clone(), actor.clone());
            tokio::spawn(async move {
                let result = host.call(DeviceRole::Local, &actor, body).await;
                let _ = tx.json(&FrontMsg::Res { id, result: result.into() }).await;
            });
        }
    }
    Ok(())
}

/// CLI 쪽 클라이언트
pub struct FrontClient {
    tx: FrameTx,
    rx: FrameRx<tokio::io::ReadHalf<crate::ipc::LocalStream>>,
    seq: u64,
}

impl FrontClient {
    pub async fn connect(state_root: &Path) -> Result<FrontClient> {
        let path = socket(state_root);
        let s = crate::ipc::connect(&path)
            .await
            .with_context(|| format!("host가 없다({}). host 머신에서 `aa host`가 실행 중인지 본다", path.display()))?;
        let (r, w) = tokio::io::split(s);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        tx.json(&FrontMsg::Hello { proto: FRONT_API, client: "cli".into() }).await?;
        match tokio::time::timeout(Duration::from_secs(10), rx.control::<FrontMsg>()).await?? {
            FrontMsg::Welcome { .. } => {}
            FrontMsg::Reject(e) => bail!("host가 거부했다: {e}"),
            other => bail!("예상하지 못한 응답: {other:?}"),
        }
        Ok(FrontClient { tx, rx, seq: 0 })
    }

    pub async fn call(&mut self, body: FrontRequest) -> Result<Result<FrontResponse, ErrorBody>> {
        self.seq += 1;
        let id = format!("q{}", self.seq);
        self.tx.json(&FrontMsg::Req { id: id.clone(), body }).await?;
        loop {
            match self.rx.control::<FrontMsg>().await? {
                FrontMsg::Res { id: rid, result } if rid == id => return Ok(result.into_result()),
                _ => continue,
            }
        }
    }
}
