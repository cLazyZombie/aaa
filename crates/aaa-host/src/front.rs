//! Front API의 같은 머신 경로 서버(UDS, peer UID 확인 → Local 역할). 클라이언트는 `aaa_common::front`다.
//! front는 backend를 띄우지 않는다: host가 없으면 「host 없음」만 알린다(§4.2 2).
//!
//! 연결 하나가 front 하나다. 「지금 쓰는 화면」(lease)은 이 연결에 묶이고, 다른 front가 입력하면 옮겨 가며, 연결이 끝나면 풀린다.

use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{
    DeviceRole, ErrorBody, ErrorCode, FRONT_API, FrontEvent, FrontMsg, FrontRequest, FrontResponse, MAX_FRAME,
    negotiate,
};
use anyhow::{Context, Result, bail};

use super::Host;
use crate::caller::Caller;
use aaa_common::conn::{FrameRx, FrameTx, Incoming, spawn_writer};

/// 응답 frame(JSON). frame 상한(`MAX_FRAME`)을 넘는 응답은 `too_large` 오류로 바꾼다. 그대로 보내면 쓰기 task가
/// 연결을 닫아 front는 까닭 없이 끊긴다(t-0334 pilot 결함 2: 끝낸 inbox가 많을 때 `inbox list --all`).
pub(crate) fn res_frame(id: String, result: Result<FrontResponse, ErrorBody>) -> Vec<u8> {
    res_frame_within(id, result, MAX_FRAME)
}

/// frame 길이는 종류 byte까지 센다(`write_frame`: payload + 1 > max면 거부). payload가 `max`와 같으면 이미 넘는다(t-0340 감사 D2).
fn res_frame_within(id: String, result: Result<FrontResponse, ErrorBody>, max: usize) -> Vec<u8> {
    let clean = aaa_common::redact::json(result)
        .unwrap_or_else(|_| Err(ErrorBody::new(ErrorCode::Unavailable, "응답 가림 실패로 전송을 거부했다")));
    let msg = FrontMsg::Res { id, result: clean.into() };
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

/// front 연결에서 오는 제어 frame(JSON). 같은 머신 socket의 제어 frame을 읽는다
pub(crate) trait FrontIn {
    /// 다음 제어 frame. 연결이 끝나면 None. 취소에 안전해야 한다(기기 취소와 함께 기다린다)
    async fn next_control(&mut self) -> Result<Option<Vec<u8>>>;
}

impl<R: tokio::io::AsyncRead + Unpin> FrontIn for FrameRx<R> {
    async fn next_control(&mut self) -> Result<Option<Vec<u8>>> {
        loop {
            match self.next().await? {
                None => return Ok(None),
                Some(Incoming::Control(b)) => return Ok(Some(b)),
                Some(Incoming::Terminal(_)) => continue,
            }
        }
    }
}

/// 한 front 연결의 신원(handshake 뒤)
struct Conn {
    /// 요청 권한
    role: DeviceRole,
    actor: String,
    front: crate::FrontRef,
    /// 합의한 FRONT_API(새 사건은 아는 판에만 보낸다)
    proto: aaa_proto::Version,
}

/// 보기 권한인 까닭(CLI 연결의 actor로 가린다, plan §8.164). 자기 스크립트가 왜 막혔는지 알게 한다.
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
    let listener = Arc::new(listener);
    host.clone().supervise(crate::supervisor::Task::Front, move || {
        let (host, listener) = (host.clone(), listener.clone());
        async move {
            host.task_progress(crate::supervisor::Task::Front);
            loop {
                let (s, pid) = match listener.accept_peer().await {
                    Ok(pair) => pair,
                    Err(e) => {
                        host.resource_error("front accept", &e);
                        tracing::warn!(error = %e, "front accept 실패");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let host = host.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve(host, s, pid).await {
                        tracing::debug!("front 연결 끝: {e:#}");
                    }
                });
            }
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
    if client == "tui" || client.starts_with("tui@") {
        let e = ErrorBody::new(ErrorCode::Unavailable, aaa_common::front::TUI_REMOVED);
        tx.json(&FrontMsg::Reject(e)).await?;
        return Ok(());
    }
    let agreed = match negotiate(FRONT_API, proto) {
        Ok(v) => v,
        Err(e) => {
            tx.json(&FrontMsg::Reject(ErrorBody::new(ErrorCode::UnsupportedVersion, e.to_string()))).await?;
            bail!("{e}");
        }
    };
    let mut front = host.new_front(client.clone());
    front.peer_pid = pid;
    // thread 세션 안의 CLI는 보기만 한다(§8.25): 일반 thread(worker)와 사후 평가자는 다른 thread·pane에 지시하지 않는다(§8.13).
    // coordinator와 thread가 아닌 pane(사람의 셸)은 그대로다. 어느 pane 안인지는 host가 연결한 프로세스로 정한다(plan §8.164):
    // CLI가 알린 세션(`AAA_SESSION_ID`)은 권한을 줄이는 쪽으로만 쓴다(변수를 지우면 권한이 늘던 구멍, t-0340 감사).
    // rollout 대기는 host 시작 직후에도 접속한다. local hello의 pane 자료보다 먼저 신원을 고정하지 않는다.
    // 그 자료는 같은 inner lock에서 전부 들어오며, 연결이 끊겨도 보존된다. 권한은 이 뒤의 기존 peer PID 판정 그대로다.
    if matches!(client.as_str(), "deploy" | "deploy-wait") {
        let mut changes = host.changed.subscribe();
        if !host.local_machine_connected() {
            tracing::info!("배포 상태 조회: 로컬 agent 자료 기다림");
        }
        let ready = tokio::time::timeout(Duration::from_secs(120), async {
            while !host.local_machine_connected() {
                changes.changed().await?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .await;
        if !matches!(ready, Ok(Ok(()))) {
            tx.json(&FrontMsg::Reject(ErrorBody::new(ErrorCode::Unavailable, "로컬 agent 자료가 준비되지 않았다")))
                .await?;
            return Ok(());
        }
    }
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
    // 데려온 pane(`thread adopt`)은 띄울 때 역할이 없다: 끝나지 않은 thread면 그 역할이다
    let role_of = |s: &str| {
        host.pane_view_of(s, DeviceRole::Local).and_then(|p| p.role).or_else(|| host.thread_of_pane(s).map(|r| r.role))
    };
    let scoped_role =
        |r: Option<aaa_proto::Role>| matches!(r, Some(aaa_proto::Role::Evaluator | aaa_proto::Role::Worker));
    let (found_role, claimed_role) = (found.as_deref().and_then(role_of), session.as_deref().and_then(role_of));
    let scoped = scoped_role(found_role) || scoped_role(claimed_role) || caller == Caller::Detached;
    let role = if scoped { DeviceRole::Reader } else { DeviceRole::Local };
    // 기록·판단에 쓸 세션: 좁은 쪽(thread)을 먼저, 그다음 판정한 pane, 그다음 알린 세션
    let (sess, sess_role) = if scoped_role(found_role) || (found.is_some() && !scoped_role(claimed_role)) {
        (found, found_role)
    } else {
        (session, claimed_role)
    };
    // aaa 세션 안 CLI(coordinator·사람의 셸 pane)는 `@<세션>`을 붙인다: 사람만 하는 일(신뢰 창 답 등)을 가른다
    // client·session은 자기 신고다. @를 actor 경계로 주입하거나 coordinator ID를 신고해도 권한을 얻지 않는다.
    let client = client.replace('@', "%40");
    // 사람 배포 출처는 self-reported client/session이 아니라 실제 peer/pane/제어 터미널로 만든다.
    // 다른 client는 항상 local: 아래에 있으므로 person: 이름을 위장할 수 없다.
    let person = matches!(client.as_str(), "deploy" | "deploy-wait")
        && caller == Caller::Outside
        && sess.is_none()
        && pid.is_some_and(|p| aaa_common::process::tty(p).is_some());
    let actor = match (&sess, sess_role) {
        _ if person => format!("person:{}", pid.unwrap()),
        (Some(s), Some(r @ (aaa_proto::Role::Evaluator | aaa_proto::Role::Worker))) => format!("{}:{s}", r.as_str()),
        _ if caller == Caller::Detached => format!("detached:{client}"),
        (Some(s), _) if caller == Caller::Pane(s.clone()) => format!("local:{client}@{s}"),
        (Some(_), _) => format!("local:{client}@unverified"),
        _ => format!("local:{client}"),
    };
    let warnings = host
        .coordinator_project_of_actor(&actor)
        .map(|p| host.post_warnings(&p))
        .transpose()
        .map_err(|e| anyhow::anyhow!(e.message))?
        .unwrap_or_default();
    tx.json(&FrontMsg::Welcome {
        proto: agreed,
        host_id: host.host_id.clone(),
        role,
        front: front.id.clone(),
        warnings,
    })
    .await?;
    serve_conn(host, tx, rx, Conn { role, actor, front, proto: agreed }).await.map(|_| ())
}

/// handshake 뒤 한 front 연결: 사건을 보내고 요청을 처리한다
async fn serve_conn(host: Arc<Host>, tx: FrameTx, mut rx: impl FrontIn, conn: Conn) -> Result<()> {
    let Conn { role, actor, front, proto } = conn;
    // `tasks` 사건(1.60): 옛 CLI는 모르는 사건을 풀지 못해 연결이 끊긴다
    let tasks_known = (proto.major, proto.minor) >= (1, 60);

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
        let (tx, mut flow, mut threads, mut questions, mut notices, mut tasks) = (
            tx.clone(),
            host.flow_events.subscribe(),
            host.thread_events.subscribe(),
            host.question_events.subscribe(),
            host.notice_events.subscribe(),
            host.task_events.subscribe(),
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
                        Ok(t) => FrontEvent::Thread(Box::new(t)),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    },
                    q = questions.recv() => match q {
                        Ok(q) => FrontEvent::Question(q),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    },
                    n = notices.recv() => match n {
                        // 새 앱 전용 종류를 옛 CLI의 enum으로 보내지 않는다. CLI는 Welcome 경고를 읽는다.
                        Ok(n) if matches!(n.kind, aaa_proto::NoticeKind::HostIssue | aaa_proto::NoticeKind::HostRecovered | aaa_proto::NoticeKind::PostMuted | aaa_proto::NoticeKind::RolloutFinished | aaa_proto::NoticeKind::RolloutFailed) => continue,
                        Ok(n) => FrontEvent::Notice(n),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    },
                    t = tasks.recv() => match t {
                        Ok((project, sha256)) if tasks_known => FrontEvent::Tasks { project, sha256 },
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
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
    // pane을 바꾸는 요청(입력·크기·가져오기 등)은 받은 순서대로 하나씩 처리한다. front는 키마다 응답을 기다리지 않고
    // 보내므로, 동시에 처리하면 키 순서가 뒤바뀐다. 조회 같은 나머지 요청은 동시에 처리한다.
    let (ordered_tx, ordered_rx) = tokio::sync::mpsc::unbounded_channel::<(String, FrontRequest)>();
    let ordered_task = {
        let (host, tx, actor, front) = (host.clone(), tx.clone(), actor.clone(), front.clone());
        tokio::spawn(async move {
            // 받는 중인 그림(웹 터미널 WebSocket과 같은 규칙). 실패한 첨부의 남은 조각은 응답만 하고 버린다.
            let mut upload: Option<crate::attach::Upload> = None;
            let mut upload_failed = false;
            let mut ordered = OrderedInput::new(ordered_rx);
            while let Some((ids, body)) = ordered.next().await {
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
                for id in ids {
                    if tx.control(res_frame(id, result.clone())).await.is_err() {
                        return;
                    }
                }
            }
        })
    };
    let mut waits: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let result: Result<()> = async {
        loop {
            let Some(b) = rx.next_control().await? else { return Ok(()) };
            let (id, body) = match read_req(&b)? {
                ReqFrame::Req(id, body) => (id, *body),
                ReqFrame::Unknown(id, e) => {
                    tx.control(res_frame(id, Err(e))).await?;
                    continue;
                }
                ReqFrame::Other => continue,
            };
            match body {
                body @ FrontRequest::PaneWaitOutput { .. } => {
                    let (host, tx, actor, front) = (host.clone(), tx.clone(), actor.clone(), front.clone());
                    waits.retain(|t| !t.is_finished());
                    waits.push(tokio::spawn(async move {
                        let result = host.call_from(role, &actor, Some(&front), body).await;
                        let _ = tx.control(res_frame(id, result)).await;
                    }));
                }
                body @ (FrontRequest::PaneInput { .. }
                | FrontRequest::PaneCtrlEnter { .. }
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
    }
    .await;
    layout_task.abort();
    state_task.abort();
    ordered_task.abort();
    flow_task.abort();
    for t in waits {
        t.abort();
    }
    host.front_gone(&front.id);
    result
}

/// front가 보낸 control frame 하나
enum ReqFrame {
    Req(String, Box<FrontRequest>),
    /// 이 host가 모르는 요청(옛 front가 보낸 없앤 요청, 새 front의 새 요청): 그 요청에 이 오류로 답한다
    Unknown(String, ErrorBody),
    Other,
}

/// 모르는 요청이어도 `id`가 있으면 연결을 끊지 않는다(t-0340 보안 감사: 1.11 front의 `decision_add`가 답 없이 끊겼다).
/// `id`도 없는 frame은 front가 아니다: 끊는다.
fn read_req(b: &[u8]) -> Result<ReqFrame, serde_json::Error> {
    match serde_json::from_slice::<FrontMsg>(b) {
        Ok(FrontMsg::Req { id, body }) => Ok(ReqFrame::Req(id, Box::new(body))),
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

/// herdr `push_target_event`: 이미 받은 같은 pane의 입력만 한 번에 쓴다. 입력·크기 변경 순서는 그대로이며,
/// agent 확인을 기다리는 동안 쌓인 마우스 사건마다 왕복을 하나씩 더하지 않는다. 새 입력을 기다려 묶지는 않는다.
struct OrderedInput {
    rx: tokio::sync::mpsc::UnboundedReceiver<(String, FrontRequest)>,
    held: Option<(String, FrontRequest)>,
}

impl OrderedInput {
    fn new(rx: tokio::sync::mpsc::UnboundedReceiver<(String, FrontRequest)>) -> Self {
        Self { rx, held: None }
    }

    async fn next(&mut self) -> Option<(Vec<String>, FrontRequest)> {
        let (id, mut body) = match self.held.take() {
            Some(req) => req,
            None => self.rx.recv().await?,
        };
        let mut ids = vec![id];
        if let FrontRequest::PaneInput { pane, data } = &mut body {
            // frame 상한보다 충분히 작게 묶는다. 너무 큰 붙여넣기도 다음 요청을 삼키지 않는다.
            while data.0.len() < 64 * 1024 && ids.len() < 256 {
                let Ok((id, next)) = self.rx.try_recv() else { break };
                match next {
                    FrontRequest::PaneInput { pane: p, data: d }
                        if p == *pane && data.0.len() + d.0.len() <= 64 * 1024 =>
                    {
                        data.0.extend(d.0);
                        ids.push(id);
                    }
                    other => {
                        self.held = Some((id, other));
                        break;
                    }
                }
            }
        }
        Some((ids, body))
    }
}

/// herdr headless `note_interactive_input`·`can_render_interactive`와 같은 입력 직후 렌더 창/예산.
/// 보통 출력은 계속 묶지만, 방금 입력한 화면의 응답에는 묶기 간격을 무조건 더하지 않는다.
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_wire_excludes_web_only_rollout_notices() {
        use aaa_proto::{NoticeKind, NoticeView};
        let root = std::env::temp_dir().join(format!("aaa-local-notice-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        let (client, server) = tokio::io::duplex(MAX_FRAME);
        let (sr, sw) = tokio::io::split(server);
        let front = h.new_front("cli".into());
        let task = tokio::spawn(serve_conn(
            h.clone(),
            spawn_writer(sw, MAX_FRAME),
            FrameRx::new(sr, MAX_FRAME),
            Conn { role: DeviceRole::Local, actor: "local:cli".into(), front, proto: FRONT_API },
        ));
        let mut rx = FrameRx::new(client, MAX_FRAME);
        assert!(matches!(rx.control::<FrontMsg>().await.unwrap(), FrontMsg::Event(FrontEvent::State { .. })));
        for kind in [NoticeKind::RolloutFinished, NoticeKind::RolloutFailed, NoticeKind::Finished] {
            h.send_notice(NoticeView {
                kind,
                pane: None,
                machine: String::new(),
                project: None,
                thread: None,
                title: "wire barrier".into(),
                agent: None,
                turn_ms: None,
                long: true,
                at_ms: 1,
            });
        }
        let msg = tokio::time::timeout(Duration::from_secs(2), rx.control::<FrontMsg>()).await.unwrap().unwrap();
        assert!(
            matches!(msg, FrontMsg::Event(FrontEvent::Notice(NoticeView { kind: NoticeKind::Finished, .. }))),
            "N-1 CLI가 모르는 웹 알림을 받았다: {msg:?}"
        );
        drop(rx);
        task.await.unwrap().unwrap();
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 같은 pane의 쌓인 입력은 손실 없이 한 번에 전송하고, resize·다른 pane을 넘어서 묶지 않는다.
    #[tokio::test]
    async fn queued_input_batches_preserve_bytes_boundaries_and_receipts() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let input = |pane: &str, data: &[u8]| FrontRequest::PaneInput {
            pane: pane.into(),
            data: aaa_proto::B64(data.to_vec()),
        };
        for (id, req) in [
            ("press", input("p", b"\x1b[<0;1;1M")),
            ("drag", input("p", b"\x1b[<32;8;2M")),
            ("up", input("p", b"\x1b[<0;8;2m")),
            ("resize", FrontRequest::PaneResize { pane: "p".into(), cols: 90, rows: 30 }),
            ("key", input("p", b"x")),
            ("other", input("q", b"y")),
            ("full", input("q", &vec![b'z'; 64 * 1024])),
            ("tail", input("q", b"!")),
        ] {
            tx.send((id.into(), req)).unwrap();
        }
        drop(tx);
        let mut ordered = OrderedInput::new(rx);
        let (ids, req) = ordered.next().await.unwrap();
        assert_eq!(ids, ["press", "drag", "up"]);
        assert_eq!(req, input("p", b"\x1b[<0;1;1M\x1b[<32;8;2M\x1b[<0;8;2m"));
        let (ids, req) = ordered.next().await.unwrap();
        assert_eq!(ids, ["resize"]);
        assert!(matches!(req, FrontRequest::PaneResize { .. }));
        for (id, pane, text) in [
            ("key", "p", b"x".as_slice()),
            ("other", "q", b"y"),
            ("full", "q", &vec![b'z'; 64 * 1024]),
            ("tail", "q", b"!"),
        ] {
            assert_eq!(ordered.next().await, Some((vec![id.into()], input(pane, text))));
        }
        assert!(ordered.next().await.is_none());

        // 빈 입력도 유효한 요청이다. byte 상한만으로는 그 입력을 끝없이 모을 수 있으므로 요청 수도 묶는다.
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for n in 0..300 {
            tx.send((n.to_string(), input("p", b""))).unwrap();
        }
        drop(tx);
        let mut ordered = OrderedInput::new(rx);
        let (first, req) = ordered.next().await.unwrap();
        assert_eq!(first.len(), 256);
        assert_eq!(req, input("p", b""));
        let (last, req) = ordered.next().await.unwrap();
        assert_eq!(last.len(), 44);
        assert_eq!(
            first.into_iter().chain(last).collect::<Vec<_>>(),
            (0..300).map(|n| n.to_string()).collect::<Vec<_>>()
        );
        assert_eq!(req, input("p", b""));
        assert!(ordered.next().await.is_none());
    }

    /// 없앤 요청(1.12의 `decision_add`)·모르는 요청에는 그 id로 오류를 답하고, id도 없는 것만 끊는다(t-0340 보안 감사)
    #[test]
    fn unknown_requests_get_an_error_answer() {
        let known = br#"{"t":"req","id":"1","body":{"op":"projects"}}"#;
        assert!(
            matches!(read_req(known), Ok(ReqFrame::Req(id, body)) if id == "1" && matches!(*body, FrontRequest::Projects { all: false }))
        );
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
