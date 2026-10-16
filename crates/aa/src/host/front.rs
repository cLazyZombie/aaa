//! Front API의 같은 머신 경로(UDS, peer UID 확인 → Local 역할). CLI와 TUI(`aa attach`)가 쓴다.
//! front는 backend를 띄우지 않는다: host가 없으면 「host 없음」만 알린다(§4.2 2).
//!
//! 연결 하나가 front 하나다. 조작 lease는 이 연결에 묶이고 연결이 끝나면 풀린다.
//! `PaneWatch`로 pane 화면(cell grid)을, 연결하면 workspace layout 변경을 사건으로 받는다.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use aa_proto::{
    DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontEvent, FrontMsg, FrontRequest, FrontResponse, GridFrame,
    GridLine, MAX_FRAME, negotiate,
};
use anyhow::{Context, Result, bail};

use super::{Host, PaneEvent};
use crate::conn::{FrameRx, FrameTx, Incoming, spawn_writer};

/// 화면 frame 사이 최소 간격(초당 약 30장)
const GRID_INTERVAL: Duration = Duration::from_millis(33);

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
    let front = host.new_front(client.clone());
    tx.json(&FrontMsg::Welcome {
        proto: agreed,
        host_id: host.host_id.clone(),
        role: DeviceRole::Local,
        front: front.id.clone(),
    })
    .await?;
    let actor = format!("local:{client}");

    // workspace layout 변경을 이 연결에 보낸다
    let layout_task = {
        let (tx, mut ev) = (tx.clone(), host.layout_events.subscribe());
        tokio::spawn(async move {
            while let Ok(w) = ev.recv().await {
                if tx.json(&FrontMsg::Event(FrontEvent::Workspace(w))).await.is_err() {
                    break;
                }
            }
        })
    };
    // 머신·pane 목록(상태·lease)이 바뀌면 전체 목록을 보낸다
    let state_task = {
        let (tx, host, mut changed) = (tx.clone(), host.clone(), host.changed.subscribe());
        tokio::spawn(async move {
            loop {
                let ev =
                    FrontEvent::State { machines: host.machine_views(), panes: host.pane_views(DeviceRole::Local) };
                if tx.json(&FrontMsg::Event(ev)).await.is_err() || changed.changed().await.is_err() {
                    break;
                }
            }
        })
    };
    let mut watches: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    let result: Result<()> = async {
        while let Some(m) = rx.next().await? {
            let Incoming::Control(b) = m else { continue };
            let FrontMsg::Req { id, body } = serde_json::from_slice(&b)? else { continue };
            match body {
                FrontRequest::PaneWatch { pane } => {
                    let result = match host.pane_view_of(&pane, DeviceRole::Local) {
                        Some(_) => {
                            if let Some(old) = watches.remove(&pane) {
                                old.abort();
                            }
                            watches.insert(pane.clone(), tokio::spawn(watch_grid(host.clone(), pane, tx.clone())));
                            Ok(FrontResponse::Done)
                        }
                        None => Err(ErrorBody::new(ErrorCode::NotFound, format!("pane이 없다: {pane}"))),
                    };
                    tx.json(&FrontMsg::Res { id, result: result.into() }).await?;
                }
                FrontRequest::PaneUnwatch { pane } => {
                    if let Some(t) = watches.remove(&pane) {
                        t.abort();
                    }
                    tx.json(&FrontMsg::Res { id, result: Ok::<_, ErrorBody>(FrontResponse::Done).into() }).await?;
                }
                body => {
                    let (host, tx, actor, front) = (host.clone(), tx.clone(), actor.clone(), front.clone());
                    tokio::spawn(async move {
                        let result = host.call_from(DeviceRole::Local, &actor, Some(&front), body).await;
                        let _ = tx.json(&FrontMsg::Res { id, result: result.into() }).await;
                    });
                }
            }
        }
        Ok(())
    }
    .await;
    layout_task.abort();
    state_task.abort();
    for (_, t) in watches {
        t.abort();
    }
    host.front_gone(&front.id);
    result
}

/// pane 화면을 바뀐 줄만 보낸다. 처음과 화면이 다시 잡힐 때(Reset)는 전체를 보낸다.
async fn watch_grid(host: Arc<Host>, pane: String, tx: FrameTx) {
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};
    let Some((mut rx, ..)) = host.attach_pane(&pane) else { return };
    let mut sent: Vec<u64> = Vec::new();
    let mut full = true;
    loop {
        if let Some(frame) = host.grid_frame(&pane, &mut sent, full) {
            full = false;
            if (!frame.lines.is_empty() || frame.full || frame.cursor_changed)
                && tx.json(&FrontMsg::Event(FrontEvent::Grid(frame.into_frame()))).await.is_err()
            {
                return;
            }
        }
        // 다음 변경을 기다리고, 짧은 시간 동안 들어온 변경은 한 장으로 묶는다
        match rx.recv().await {
            Ok(PaneEvent::Reset) | Err(RecvError::Lagged(_)) => full = true,
            Ok(_) => {}
            Err(RecvError::Closed) => return,
        }
        tokio::time::sleep(GRID_INTERVAL).await;
        loop {
            match rx.try_recv() {
                Ok(PaneEvent::Reset) | Err(TryRecvError::Lagged(_)) => full = true,
                Ok(_) => {}
                Err(_) => break,
            }
        }
    }
}

/// 보낼 frame과 커서가 움직였는지
pub struct GridDiff {
    frame: GridFrame,
    pub lines: Vec<GridLine>,
    pub full: bool,
    pub cursor_changed: bool,
}

impl GridDiff {
    fn into_frame(self) -> GridFrame {
        GridFrame { lines: self.lines, ..self.frame }
    }
}

/// 직전에 보낸 줄 hash와 커서. `sent[rows]`에 커서를 함께 넣는다.
impl Host {
    pub fn grid_frame(&self, pane: &str, sent: &mut Vec<u64>, full: bool) -> Option<GridDiff> {
        use std::hash::{Hash, Hasher};
        let i = self.inner.lock().unwrap();
        let p = i.panes.get(pane)?;
        let (rows, cols, cursor, src) = match &p.mirror {
            Some(m) => {
                let (rows, cols) = m.vt.size();
                let lines: Vec<_> = (0..rows).map(|r| m.vt.grid_row(r)).collect();
                (rows, cols, m.vt.cursor(), lines)
            }
            None => (p.info.rows, p.info.cols, (0, 0, false), vec![Vec::new(); p.info.rows as usize]),
        };
        drop(i);
        let n = rows as usize;
        let full = full || sent.len() != n + 1;
        if full {
            sent.clear();
            sent.resize(n + 1, u64::MAX);
        }
        let mut lines = Vec::new();
        for (r, runs) in src.into_iter().enumerate() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            for run in &runs {
                (&run.t, run.fg, run.bg, run.a).hash(&mut h);
            }
            let hash = h.finish();
            if sent[r] != hash {
                sent[r] = hash;
                lines.push(GridLine { row: r as u16, runs });
            }
        }
        let (cursor_row, cursor_col, cursor_visible) = cursor;
        let c = (u64::from(cursor_row) << 32) | (u64::from(cursor_col) << 1) | u64::from(cursor_visible);
        let cursor_changed = sent[n] != c;
        sent[n] = c;
        let frame = GridFrame {
            pane: pane.to_string(),
            cols,
            rows,
            cursor_row,
            cursor_col,
            cursor_visible,
            full,
            lines: vec![],
        };
        Some(GridDiff { frame, lines, full, cursor_changed })
    }
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
            .with_context(|| format!("host가 없다({}). host 머신에서 `aa host`가 실행 중인지 본다", path.display()))?;
        let (r, w) = tokio::io::split(s);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        tx.json(&FrontMsg::Hello { proto: FRONT_API, client: client.into() }).await?;
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
