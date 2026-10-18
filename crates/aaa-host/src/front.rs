//! Front API의 같은 머신 경로 서버(UDS, peer UID 확인 → Local 역할). 클라이언트는 `aaa_common::front`다.
//! front는 backend를 띄우지 않는다: host가 없으면 「host 없음」만 알린다(§4.2 2).
//!
//! 연결 하나가 front 하나다. 「지금 쓰는 화면」(lease)은 이 연결에 묶이고, 다른 front가 입력하면 옮겨 가며, 연결이 끝나면 풀린다.
//! `PaneWatch`로 pane 화면(cell grid)을, 연결하면 workspace layout 변경을 사건으로 받는다.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{
    DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontEvent, FrontMsg, FrontRequest, FrontResponse, GridFrame,
    GridLine, GridRun, MAX_FRAME, PaneModes, negotiate,
};
use anyhow::{Context, Result, bail};

use super::{Host, PaneEvent};
use crate::caller::Caller;
use aaa_common::conn::{FrameRx, FrameTx, Incoming, spawn_writer};

/// 화면 frame 사이 최소 간격(초당 약 30장)
const GRID_INTERVAL: Duration = Duration::from_millis(33);

/// 응답 frame(JSON). frame 상한(`MAX_FRAME`)을 넘는 응답은 `too_large` 오류로 바꾼다. 그대로 보내면 쓰기 task가
/// 연결을 닫아 front는 까닭 없이 끊긴다(t-0334 pilot 결함 2: 끝낸 inbox가 많을 때 `inbox list --all`).
pub(crate) fn res_frame(id: String, result: Result<FrontResponse, ErrorBody>) -> Vec<u8> {
    res_frame_within(id, result, MAX_FRAME)
}

/// frame 길이는 종류 byte까지 센다(`write_frame`: payload + 1 > max면 거부). payload가 `max`와 같으면 이미 넘는다(t-0340 감사 D2).
fn res_frame_within(id: String, result: Result<FrontResponse, ErrorBody>, max: usize) -> Vec<u8> {
    let msg = FrontMsg::Res { id, result: result.into() };
    let bytes = serde_json::to_vec(&msg).expect("JSON 직렬화");
    if bytes.len() < max {
        return bytes;
    }
    let FrontMsg::Res { id, result } = msg else { unreachable!() };
    // 드문 일이라 종류 이름을 얻으려고 한 번 더 직렬화한다
    let kind = serde_json::to_value(&result)
        .ok()
        .and_then(|v| v.get("ok").and_then(|r| r.get("kind")).and_then(|k| k.as_str()).map(str::to_string))
        .unwrap_or_else(|| "?".into());
    tracing::warn!(kind, bytes = bytes.len(), "응답이 frame 상한을 넘어 오류로 답한다");
    let e = ErrorBody::new(
        ErrorCode::TooLarge,
        format!(
            "{kind} 응답이 {} KiB라 frame 상한 {} KiB를 넘는다. 범위를 좁혀 다시 묻는다",
            bytes.len() >> 10,
            max >> 10
        ),
    );
    serde_json::to_vec(&FrontMsg::Res { id, result: Err::<FrontResponse, _>(e).into() }).expect("JSON 직렬화")
}

/// 보기 권한인 까닭(CLI 연결의 actor로 가린다, plan §8.162). 자기 스크립트가 왜 막혔는지 알게 한다.
pub(crate) fn reader_reason(actor: &str) -> &'static str {
    if actor.starts_with("detached:") {
        ": 터미널도 aaa pane도 없는 떼어 낸 프로세스라 보기만 한다(사람 터미널에서 다시 실행한다)"
    } else if actor.starts_with("worker:") || actor.starts_with("evaluator:") {
        ": thread 세션 안의 CLI는 보기만 한다"
    } else {
        ""
    }
}

/// aaa 세션(pane) 안에서 실행한 CLI인가(`local:<client>@<세션>`). coordinator가 여기에 든다.
pub(crate) fn in_session(actor: &str) -> bool {
    actor.strip_prefix("local:").is_some_and(|rest| rest.contains('@'))
}

pub async fn start(host: &Arc<Host>) -> Result<()> {
    let path = host.state.join("front.sock");
    let listener = aaa_common::ipc::LocalListener::bind(&path).context("front socket")?;
    let host = host.clone();
    tokio::spawn(async move {
        while let Ok((s, pid)) = listener.accept_peer().await {
            let host = host.clone();
            tokio::spawn(async move {
                if let Err(e) = serve(host, s, pid).await {
                    tracing::debug!("front 연결 끝: {e:#}");
                }
            });
        }
    });
    Ok(())
}

async fn serve(host: Arc<Host>, s: aaa_common::ipc::LocalStream, pid: Option<u32>) -> Result<()> {
    let (r, w) = tokio::io::split(s);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let hello: FrontMsg = tokio::time::timeout(Duration::from_secs(10), rx.control()).await??;
    let FrontMsg::Hello { proto, client, session } = hello else { bail!("hello가 와야 한다") };
    let agreed = match negotiate(FRONT_API, proto) {
        Ok(v) => v,
        Err(e) => {
            tx.json(&FrontMsg::Reject(ErrorBody::new(ErrorCode::UnsupportedVersion, e.to_string()))).await?;
            bail!("{e}");
        }
    };
    let front = host.new_front(client.clone());
    // thread 세션 안의 CLI는 보기만 한다(§8.25): 일반 thread(worker)와 사후 평가자는 다른 thread·pane에 지시하지 않는다(§8.13).
    // coordinator와 thread가 아닌 pane(사람의 셸)은 그대로다. 어느 pane 안인지는 host가 연결한 프로세스로 정한다(plan §8.162):
    // CLI가 알린 세션(`AAA_SESSION_ID`)은 권한을 줄이는 쪽으로만 쓴다(변수를 지우면 권한이 늘던 구멍, t-0340 감사).
    let caller = host.caller_of(pid);
    let found = match &caller {
        Caller::Pane(p) => Some(p.clone()),
        _ => None,
    };
    if let (Some(f), Some(c)) = (&found, &session)
        && f != c
    {
        tracing::warn!(found = %f, claimed = %c, "CLI가 알린 세션과 연결한 프로세스의 pane이 다르다: pane을 따른다");
    }
    let role_of = |s: &str| host.pane_view_of(s, DeviceRole::Local).and_then(|p| p.role);
    let scoped_role =
        |r: Option<aaa_proto::Role>| matches!(r, Some(aaa_proto::Role::Evaluator | aaa_proto::Role::Worker));
    let (found_role, claimed_role) = (found.as_deref().and_then(role_of), session.as_deref().and_then(role_of));
    let scoped = scoped_role(found_role) || scoped_role(claimed_role) || caller == Caller::Detached;
    let role = if scoped { DeviceRole::Reader } else { DeviceRole::Local };
    tx.json(&FrontMsg::Welcome { proto: agreed, host_id: host.host_id.clone(), role, front: front.id.clone() }).await?;
    // 기록·판단에 쓸 세션: 좁은 쪽(thread)을 먼저, 그다음 판정한 pane, 그다음 알린 세션
    let (sess, sess_role) = if scoped_role(found_role) || (found.is_some() && !scoped_role(claimed_role)) {
        (found, found_role)
    } else {
        (session, claimed_role)
    };
    // aaa 세션 안 CLI(coordinator·사람의 셸 pane)는 `@<세션>`을 붙인다: 사람만 하는 일(신뢰 창 답 등)을 가른다
    let actor = match (&sess, sess_role) {
        (Some(s), Some(r @ (aaa_proto::Role::Evaluator | aaa_proto::Role::Worker))) => format!("{}:{s}", r.as_str()),
        _ if caller == Caller::Detached => format!("detached:{client}"),
        (Some(s), _) => format!("local:{client}@{s}"),
        _ => format!("local:{client}"),
    };

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
    // 흐름 사건과 thread 기록 변화(놓치면 `events`로 cursor부터 다시 읽는다)
    let flow_task = {
        let (tx, mut flow, mut threads, mut questions) = (
            tx.clone(),
            host.flow_events.subscribe(),
            host.thread_events.subscribe(),
            host.question_events.subscribe(),
        );
        tokio::spawn(async move {
            loop {
                let ev = tokio::select! {
                    e = flow.recv() => match e {
                        Ok(e) => FrontEvent::Flow(e),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    },
                    t = threads.recv() => match t {
                        Ok(t) => FrontEvent::Thread(t),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    },
                    q = questions.recv() => match q {
                        Ok(q) => FrontEvent::Question(q),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    },
                };
                if tx.json(&FrontMsg::Event(ev)).await.is_err() {
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
            // 받는 중인 그림(웹 터미널 WebSocket과 같은 규칙). 실패한 첨부의 남은 조각은 응답만 하고 버린다.
            let mut upload: Option<crate::attach::Upload> = None;
            let mut upload_failed = false;
            while let Some((id, body)) = ordered_rx.recv().await {
                let result = match body {
                    FrontRequest::PaneAttach { offset, .. } if offset != 0 && upload_failed => {
                        Err(ErrorBody::new(ErrorCode::BadRequest, "앞 조각이 실패한 첨부다"))
                    }
                    FrontRequest::PaneAttach { pane, offset, total, sha256, data } => {
                        let r = host
                            .attach_chunk(role, &actor, &front, &pane, &mut upload, offset, total, &sha256, data.0)
                            .await;
                        upload_failed = r.is_err();
                        r.map(|a| match a {
                            Some(a) => FrontResponse::Attached { path: a.path, inserted: a.inserted, note: a.note },
                            None => FrontResponse::Done,
                        })
                    }
                    body => host.call_from(role, &actor, Some(&front), body).await,
                };
                if tx.control(res_frame(id, result)).await.is_err() {
                    break;
                }
            }
        })
    };
    let mut watches: HashMap<String, tokio::task::JoinHandle<()>> = HashMap::new();
    let mut waits: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let result: Result<()> = async {
        while let Some(m) = rx.next().await? {
            let Incoming::Control(b) = m else { continue };
            let (id, body) = match read_req(&b)? {
                ReqFrame::Req(id, body) => (id, body),
                ReqFrame::Unknown(id, e) => {
                    tx.control(res_frame(id, Err(e))).await?;
                    continue;
                }
                ReqFrame::Other => continue,
            };
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
                    tx.control(res_frame(id, result)).await?;
                }
                FrontRequest::PaneUnwatch { pane } => {
                    if let Some(t) = watches.remove(&pane) {
                        t.abort();
                    }
                    tx.json(&FrontMsg::Res { id, result: Ok::<_, ErrorBody>(FrontResponse::Done).into() }).await?;
                }
                // 출력 기다리기는 연결이 끝나면 멈춘다(herdr `should_stop_connection`)
                body @ FrontRequest::PaneWaitOutput { .. } => {
                    let (host, tx, actor, front) = (host.clone(), tx.clone(), actor.clone(), front.clone());
                    waits.retain(|t| !t.is_finished());
                    waits.push(tokio::spawn(async move {
                        let result = host.call_from(role, &actor, Some(&front), body).await;
                        let _ = tx.control(res_frame(id, result)).await;
                    }));
                }
                body @ (FrontRequest::PaneInput { .. }
                | FrontRequest::PanePaste { .. }
                | FrontRequest::PaneSend { .. }
                | FrontRequest::PaneSendText { .. }
                | FrontRequest::PaneSendKeys { .. }
                | FrontRequest::PaneSubmit { .. }
                | FrontRequest::PaneResize { .. }
                | FrontRequest::PaneTake { .. }
                | FrontRequest::PaneAttach { .. }
                | FrontRequest::PaneRelease { .. }
                | FrontRequest::PaneKill { .. }) => {
                    let _ = ordered_tx.send((id, body));
                }
                body => {
                    let (host, tx, actor, front) = (host.clone(), tx.clone(), actor.clone(), front.clone());
                    tokio::spawn(async move {
                        let result = host.call_from(role, &actor, Some(&front), body).await;
                        let _ = tx.control(res_frame(id, result)).await;
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
    flow_task.abort();
    for (_, t) in watches {
        t.abort();
    }
    for t in waits {
        t.abort();
    }
    host.front_gone(&front.id);
    result
}

/// front가 보낸 control frame 하나
enum ReqFrame {
    Req(String, FrontRequest),
    /// 이 host가 모르는 요청(옛 front가 보낸 없앤 요청, 새 front의 새 요청): 그 요청에 이 오류로 답한다
    Unknown(String, ErrorBody),
    Other,
}

/// 모르는 요청이어도 `id`가 있으면 연결을 끊지 않는다(t-0340 보안 감사: 1.11 front의 `decision_add`가 답 없이 끊겼다).
/// `id`도 없는 frame은 front가 아니다: 끊는다.
fn read_req(b: &[u8]) -> Result<ReqFrame, serde_json::Error> {
    match serde_json::from_slice::<FrontMsg>(b) {
        Ok(FrontMsg::Req { id, body }) => Ok(ReqFrame::Req(id, body)),
        Ok(_) => Ok(ReqFrame::Other),
        Err(e) => {
            let id = serde_json::from_slice::<serde_json::Value>(b)
                .ok()
                .and_then(|v| v.get("id")?.as_str().map(str::to_string));
            match id {
                Some(id) => {
                    let why = format!("이 host(FRONT_API {FRONT_API})가 모르는 요청이다. CLI·host 판을 맞춘다: {e}");
                    Ok(ReqFrame::Unknown(id, ErrorBody::new(ErrorCode::BadRequest, why)))
                }
                None => Err(e),
            }
        }
    }
}

/// pane 화면을 바뀐 줄만 보낸다. 처음과 화면이 다시 잡힐 때(Reset)는 전체를 보낸다.
async fn watch_grid(host: Arc<Host>, pane: String, tx: FrameTx) {
    use tokio::sync::broadcast::error::{RecvError, TryRecvError};
    let Some(crate::Attach { mut rx, .. }) = host.attach_pane(&pane) else { return };
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

/// 직전에 보낸 줄 hash와 커서. `sent[rows]`에 커서를, `sent[rows + 1]`에 크기를 함께 넣는다.
impl Host {
    pub fn grid_frame(&self, pane: &str, sent: &mut Vec<u64>, full: bool) -> Option<GridDiff> {
        let i = self.inner.lock().unwrap();
        let p = i.panes.get(pane)?;
        let (rows, cols, cursor, modes, src) = match &p.mirror {
            Some(m) => {
                let (rows, cols) = m.vt.size();
                let lines: Vec<_> = (0..rows).map(|r| m.vt.grid_row(r)).collect();
                (rows, cols, m.vt.cursor(), m.vt.pane_modes(), lines)
            }
            None => {
                (p.info.rows, p.info.cols, (0, 0, false), PaneModes::default(), vec![Vec::new(); p.info.rows as usize])
            }
        };
        drop(i);
        let (cursor_row, cursor_col, cursor_visible) = cursor;
        // 모드만 바뀌어도(프로그램이 마우스 보고를 켬) 한 장 보내도록 커서 hash 위 비트에 접는다
        let m = (modes.mouse as u64)
            | (modes.mouse_encoding as u64) << 3
            | u64::from(modes.alt_screen) << 5
            | u64::from(modes.app_cursor) << 6;
        let c = (m << 48) | (u64::from(cursor_row) << 32) | (u64::from(cursor_col) << 1) | u64::from(cursor_visible);
        let (lines, full, cursor_changed) = changed_rows(sent, (rows, cols), src, c, full);
        let frame = GridFrame {
            pane: pane.to_string(),
            cols,
            rows,
            cursor_row,
            cursor_col,
            cursor_visible,
            full,
            lines: vec![],
            modes,
        };
        Some(GridDiff { frame, lines, full, cursor_changed })
    }
}

/// 직전에 보낸 것(`sent`)과 견주어 보낼 줄을 고른다. 돌려주는 것은 (줄, 전체인지, 커서가 바뀌었는지).
/// 크기가 바뀌면 줄 수만이 아니라 폭만 바뀌어도 전체를 보낸다: front는 크기가 바뀐 grid를 비우므로, 내용이 같아 다시 보내지
/// 않은 줄이 빈칸으로 남았다(QA t-0337 Q23, herdr는 크기가 바뀐 pane을 통째로 그린다).
fn changed_rows(
    sent: &mut Vec<u64>,
    (rows, cols): (u16, u16),
    src: Vec<Vec<GridRun>>,
    cursor: u64,
    full: bool,
) -> (Vec<GridLine>, bool, bool) {
    use std::hash::{Hash, Hasher};
    let n = rows as usize;
    let size = (u64::from(rows) << 16) | u64::from(cols);
    let full = full || sent.len() != n + 2 || sent[n + 1] != size;
    if full {
        sent.clear();
        sent.resize(n + 2, u64::MAX);
        sent[n + 1] = size;
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
    let cursor_changed = sent[n] != cursor;
    sent[n] = cursor;
    (lines, full, cursor_changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 바뀐 줄만 보내되, 크기가 바뀌면(폭만 바뀌어도) 모든 줄을 다시 보낸다(QA t-0337 Q23)
    #[test]
    fn size_changes_resend_every_row() {
        let screen = |texts: &[&str]| -> Vec<Vec<GridRun>> {
            texts.iter().map(|t| vec![GridRun { t: t.to_string(), fg: 0, bg: 0, a: 0 }]).collect()
        };
        let rows_of = |lines: &[GridLine]| lines.iter().map(|l| l.row).collect::<Vec<_>>();
        let mut sent = Vec::new();
        let (lines, full, _) = changed_rows(&mut sent, (3, 40), screen(&["a", "b", ""]), 7, true);
        assert!(full);
        assert_eq!(rows_of(&lines), [0, 1, 2]);
        // 한 줄만 바뀜
        let (lines, full, cursor) = changed_rows(&mut sent, (3, 40), screen(&["a", "b", "c"]), 7, false);
        assert_eq!((rows_of(&lines), full, cursor), (vec![2], false, false));
        // 폭만 바뀜: 내용이 같아도 모두
        let (lines, full, _) = changed_rows(&mut sent, (3, 60), screen(&["a", "b", "c"]), 7, false);
        assert_eq!((rows_of(&lines), full), (vec![0, 1, 2], true));
        // 줄 수가 바뀜
        let (lines, full, _) = changed_rows(&mut sent, (2, 60), screen(&["a", "b"]), 7, false);
        assert_eq!((rows_of(&lines), full), (vec![0, 1], true));
        // 그대로면 없음, 커서만 바뀌면 커서
        let (lines, full, cursor) = changed_rows(&mut sent, (2, 60), screen(&["a", "b"]), 8, false);
        assert_eq!((lines.len(), full, cursor), (0, false, true));
    }

    /// 없앤 요청(1.12의 `decision_add`)·모르는 요청에는 그 id로 오류를 답하고, id도 없는 것만 끊는다(t-0340 보안 감사)
    #[test]
    fn unknown_requests_get_an_error_answer() {
        let known = br#"{"t":"req","id":"1","body":{"op":"projects"}}"#;
        assert!(matches!(read_req(known), Ok(ReqFrame::Req(id, FrontRequest::Projects)) if id == "1"));
        let removed = br#"{"t":"req","id":"2","body":{"op":"decision_add","project":"p","text":"x"}}"#;
        match read_req(removed) {
            Ok(ReqFrame::Unknown(id, e)) => {
                assert_eq!((id.as_str(), e.code), ("2", ErrorCode::BadRequest));
                assert!(
                    e.message.starts_with(&format!("이 host(FRONT_API {FRONT_API})가 모르는 요청이다")),
                    "{}",
                    e.message
                );
            }
            _ => panic!("모르는 요청에 답하지 않는다"),
        }
        assert!(read_req(b"not json").is_err());
        assert!(read_req(br#"{"t":"req","body":{"op":"nope"}}"#).is_err());
    }

    /// frame 상한을 넘는 응답은 연결을 닫는 대신 `too_large` 오류로 답한다(t-0334 pilot 결함 2)
    #[test]
    fn oversized_response_becomes_an_error() {
        let parse = |b: &[u8]| match serde_json::from_slice::<FrontMsg>(b).unwrap() {
            FrontMsg::Res { id, result } => (id, result.into_result()),
            other => panic!("{other:?}"),
        };
        let (id, r) = parse(&res_frame("7".into(), Ok(FrontResponse::Done)));
        assert_eq!((id.as_str(), r), ("7", Ok(FrontResponse::Done)));
        let item = aaa_proto::InboxItem {
            id: "i".into(),
            project: "p".into(),
            item_kind: "routine".into(),
            subject: "s".into(),
            event: "due".into(),
            summary: String::new(),
            body: "x".repeat(MAX_FRAME),
            done: false,
            seen: false,
            created_at_ms: 0,
            done_at_ms: None,
        };
        let big = FrontResponse::Inbox { items: vec![item], done: None, omitted_open: 0, omitted_done: 0 };
        let bytes = res_frame("8".into(), Ok(big));
        assert!(bytes.len() < 1024, "{}", bytes.len());
        let (id, r) = parse(&bytes);
        let e = r.unwrap_err();
        assert_eq!((id.as_str(), e.code), ("8", ErrorCode::TooLarge));
        assert!(e.message.starts_with("inbox 응답이 1024 KiB라 frame 상한 1024 KiB를 넘는다"), "{}", e.message);
    }

    /// 경계: 종류 byte를 더해 상한과 같으면 보낼 수 있고, payload가 상한과 같으면 `write_frame`이 거부하니 오류로 바꾼다
    #[test]
    fn response_frame_limit_counts_the_kind_byte() {
        let done = res_frame("9".into(), Ok(FrontResponse::Done));
        let n = done.len();
        assert_eq!(res_frame_within("9".into(), Ok(FrontResponse::Done), n + 1), done, "payload + 1 == max는 보낸다");
        let over = res_frame_within("9".into(), Ok(FrontResponse::Done), n);
        let FrontMsg::Res { result, .. } = serde_json::from_slice::<FrontMsg>(&over).unwrap() else { panic!() };
        assert_eq!(result.into_result().unwrap_err().code, ErrorCode::TooLarge, "payload == max는 넘는다");
        let mut buf = Vec::new();
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let mut wrote = |max| {
            rt.block_on(aaa_proto::frame::write_frame(&mut buf, aaa_proto::frame::FrameKind::Control, &done, max))
                .is_ok()
        };
        assert!(wrote(n + 1) && !wrote(n), "write_frame과 같은 경계");
    }
}
