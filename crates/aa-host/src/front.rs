//! Front API의 같은 머신 경로 서버(UDS, peer UID 확인 → Local 역할). 클라이언트는 `aa_common::front`다.
//! front는 backend를 띄우지 않는다: host가 없으면 「host 없음」만 알린다(§4.2 2).
//!
//! 연결 하나가 front 하나다. 조작 lease는 이 연결에 묶이고 연결이 끝나면 풀린다.
//! `PaneWatch`로 pane 화면(cell grid)을, 연결하면 workspace layout 변경을 사건으로 받는다.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aa_proto::{
    DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontEvent, FrontMsg, FrontRequest, FrontResponse, GridFrame,
    GridLine, MAX_FRAME, negotiate,
};
use anyhow::{Context, Result, bail};

use super::{Host, PaneEvent};
use aa_common::conn::{FrameRx, FrameTx, Incoming, spawn_writer};

/// 화면 frame 사이 최소 간격(초당 약 30장)
const GRID_INTERVAL: Duration = Duration::from_millis(33);

pub async fn start(host: &Arc<Host>) -> Result<()> {
    let path = host.state.join("front.sock");
    let listener = aa_common::ipc::LocalListener::bind(&path).context("front socket")?;
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

async fn serve(host: Arc<Host>, s: aa_common::ipc::LocalStream) -> Result<()> {
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
    // pane을 바꾸는 요청(입력·크기·가져오기 등)은 받은 순서대로 하나씩 처리한다. TUI는 키마다 응답을 기다리지 않고
    // 보내므로, 동시에 처리하면 키 순서가 뒤바뀐다. 조회 같은 나머지 요청은 동시에 처리한다.
    let (ordered_tx, mut ordered_rx) = tokio::sync::mpsc::unbounded_channel::<(String, FrontRequest)>();
    let ordered_task = {
        let (host, tx, actor, front) = (host.clone(), tx.clone(), actor.clone(), front.clone());
        tokio::spawn(async move {
            while let Some((id, body)) = ordered_rx.recv().await {
                let result = host.call_from(DeviceRole::Local, &actor, Some(&front), body).await;
                if tx.json(&FrontMsg::Res { id, result: result.into() }).await.is_err() {
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
                body @ (FrontRequest::PaneInput { .. }
                | FrontRequest::PaneSend { .. }
                | FrontRequest::PaneResize { .. }
                | FrontRequest::PaneTake { .. }
                | FrontRequest::PaneRelease { .. }
                | FrontRequest::PaneKill { .. }) => {
                    let _ = ordered_tx.send((id, body));
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
    ordered_task.abort();
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
