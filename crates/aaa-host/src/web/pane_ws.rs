//! pane 터미널 WebSocket: 출력 보내기, 입력·제출 받기.

use super::*;

/// pane WebSocket 주소의 선택. `flow=ack`면 출력 ACK를 보내는 화면이다(herdr-web-ui attach `flow_control: "ack"`).
/// `fg`·`bg`(`rrggbb`)는 그 화면의 터미널 기본 색이다(plan §8.187).
#[derive(Deserialize)]
pub(super) struct PaneWsQuery {
    pub(super) compression: Option<String>,
    pub(super) flow: Option<String>,
    pub(super) state: Option<String>,
    pub(super) fg: Option<String>,
    pub(super) bg: Option<String>,
    pub(super) trace: Option<String>,
    pub(super) attach_id: Option<String>,
}

// 인증한 조작 기기가 선택한 연결의 시간·크기만 운영 로그에 남긴다.
macro_rules! attach_event {
    ($trace:expr, $($args:tt)*) => {
        if $trace {
            tracing::info!($($args)*);
        } else {
            tracing::debug!($($args)*);
        }
    };
}

#[derive(Clone, Copy)]
pub(super) struct PaneFlow {
    pub(super) acks: bool,
    pub(super) gzip: bool,
    pub(super) grid: bool,
    pub(super) colors: Option<aaa_proto::TermColors>,
    pub(super) requested: std::time::Instant,
    pub(super) trace: bool,
    ingress: std::time::Instant,
    checked: std::time::Instant,
    auth_ms: u64,
    lookup_ms: u64,
    state_wait_ms: u64,
}

pub(super) async fn pane_ws(
    State(host): State<Arc<Host>>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<PaneWsQuery>,
    ingress: Option<Extension<PaneIngress>>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let requested = std::time::Instant::now();
    let Some((dev, role)) = device(&host, &headers) else { return unauthorized() };
    let authenticated = std::time::Instant::now();
    let trace = role.can_control() && q.trace.as_deref() == Some("attach");
    let attach_id = q.attach_id.filter(|id| trace && crate::input_log::valid_trace(id));
    // 업그레이드 전에는 존재만 확인한다. 구판 웹의 전체 상태는 붙은 뒤에 만든다.
    let lookup_started = std::time::Instant::now();
    let state = host.inner.lock().unwrap();
    let acquired = std::time::Instant::now();
    let exists = state.panes.contains_key(&id);
    drop(state);
    let checked = std::time::Instant::now();
    let ingress_started = ingress.as_ref().map_or(requested, |i| i.started);
    if !exists {
        tracing::info!(pane = %crate::operation_log_id(&id), total_ms = operation_log::ms(checked - ingress_started),
            result = "failed", reason = "pane_not_found", "pane_attach");
        return (StatusCode::NOT_FOUND, "pane이 없다").into_response();
    }
    // 그림 조각(512KiB, base64)이 들어갈 만큼만 받는다
    // ACK를 하지 않는 화면(다시 띄우기 전에 받은 옛 웹)은 보내기 줄로만 잰다
    let colors = q.fg.zip(q.bg).and_then(|(fg, bg)| aaa_proto::TermColors::from_hex(&fg, &bg));
    let flow = PaneFlow {
        acks: q.flow.as_deref() == Some("ack"),
        gzip: q.compression.as_deref() == Some("gzip.v1"),
        grid: q.state.as_deref() == Some("grid.v1"),
        colors,
        requested,
        trace,
        ingress: ingress_started,
        checked,
        auth_ms: operation_log::ms(authenticated - requested),
        lookup_ms: operation_log::ms(checked - lookup_started),
        state_wait_ms: operation_log::ms(acquired - lookup_started),
    };
    let lookup_us = (checked - lookup_started).as_micros() as u64;
    let state_wait_us = (acquired - lookup_started).as_micros() as u64;
    let state_hold_us = (checked - acquired).as_micros() as u64;
    let before_handler_us = ingress.as_ref().map(|i| requested.duration_since(i.started).as_micros() as u64);
    attach_event!(
        flow.trace,
        pane = %id,
        attach_id = attach_id.as_deref(),
        origin_us = ingress.as_ref().map(|i| i.origin_us),
        before_handler_us,
        auth_us = authenticated.duration_since(requested).as_micros() as u64,
        lookup_us,
        state_wait_us,
        state_hold_us,
        response_wall_ms = aaa_common::util::now_ms(),
        "웹 터미널 요청"
    );
    let failed_pane = id.clone();
    ws.max_message_size(1 << 20)
        .on_failed_upgrade(move |_| {
            tracing::info!(pane = %failed_pane, total_ms = operation_log::ms(ingress_started.elapsed()),
                result = "failed", reason = "upgrade_failed", "pane_attach");
        })
        .on_upgrade(move |socket| pane_loop(host, id, dev, role, flow, attach_id, socket))
}

struct PaneStartup<'a> {
    flow: PaneFlow,
    pane: &'a str,
    front: &'a str,
    upgrade_ms: u64,
    copy_ms: u64,
    snapshot_ms: u64,
    queued_at: Option<std::time::Instant>,
    reason: &'static str,
}

impl Drop for PaneStartup<'_> {
    fn drop(&mut self) {
        let total_ms = operation_log::ms(self.flow.ingress.elapsed());
        let result = match self.reason {
            "snapshot_queued" | "snapshot_acknowledged" => "ok",
            "closed_before_snapshot_ack" => "closed",
            _ => "failed",
        };
        if total_ms >= 500 || result == "failed" {
            tracing::info!(
                pane = self.pane,
                front = self.front,
                total_ms,
                before_handler_ms = operation_log::ms(self.flow.requested - self.flow.ingress),
                auth_ms = self.flow.auth_ms,
                lookup_ms = self.flow.lookup_ms,
                state_wait_ms = self.flow.state_wait_ms,
                upgrade_ms = self.upgrade_ms,
                copy_ms = self.copy_ms,
                snapshot_ms = self.snapshot_ms,
                ack_ms = self.queued_at.map(|at| operation_log::ms(at.elapsed())),
                result,
                reason = self.reason,
                "pane_attach"
            );
        }
    }
}

/// 한 터미널 연결의 보내기 줄. frame은 쓰기 task가 차례로 보내고, `buffered`는 아직 보내지 못한 바이트다
/// (herdr-web-ui가 보는 WebSocket `getBufferedAmount`). 그래서 느린 브라우저가 이 연결의 사건 처리를 막지 않고,
/// 따라오는지는 ACK와 이 줄로 잰다(plan §8.62).
pub(super) struct Outbox {
    pub(super) tx: tokio::sync::mpsc::UnboundedSender<Message>,
    pub(super) buffered: Arc<AtomicU64>,
    /// 남은 줄을 버리고 이 code로 닫는다(4008 출력 정지, 4401 기기 취소)
    pub(super) stop: tokio::sync::watch::Sender<Option<(u16, &'static str)>>,
    pub(super) payload: Arc<WebPayload>,
    pub(super) front: String,
    pub(super) trace: bool,
}

/// `Outbox`에 보내기만 하는 손잡이(앞 기록 답처럼 연결 loop 밖에서 끝나는 일)
pub(super) struct OutSender {
    pub(super) payload: Arc<WebPayload>,
    pub(super) tx: tokio::sync::mpsc::UnboundedSender<Message>,
    pub(super) buffered: Arc<AtomicU64>,
    pub(super) stop: tokio::sync::watch::Sender<Option<(u16, &'static str)>>,
}

pub(super) struct WebPayload {
    pub(super) gzip: bool,
    pub(super) serial: tokio::sync::Mutex<()>,
    pub(super) pending: Arc<tokio::sync::Semaphore>,
}

impl WebPayload {
    pub(super) async fn prepare(&self, mut msg: TermServerMsg) -> Option<String> {
        use aaa_common::terminal_payload::PayloadMessage;
        let bytes = msg.payload().map_or(0, |(data, _)| data.0.len());
        if bytes > aaa_proto::TERMINAL_PAYLOAD_MAX {
            return None;
        }
        let _bytes = self
            .pending
            .clone()
            .try_acquire_many_owned(bytes.max(1) as u32)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        let _serial = self.serial.lock().await;
        let codec = self.gzip.then_some(aaa_proto::TerminalCodec::Gzip);
        let bytes = aaa_common::terminal_payload::encode_async(msg, codec)
            .await
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        String::from_utf8(bytes)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
    }
}

impl OutSender {
    pub(super) async fn msg(&self, m: TermServerMsg) -> bool {
        let Some(text) = self.payload.prepare(m).await else {
            tracing::warn!("웹 터미널 본문 거부");
            aaa_common::diagnostic::ignored(
                self.stop.send(Some((4002, "terminal payload rejected"))),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
            return false;
        };
        let m = Message::Text(text.into());
        self.buffered.fetch_add(frame_len(&m), Ordering::Relaxed);
        self.tx
            .send(m)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok()
    }
}

/// 앞 기록 답(plan §8.67): 찾지 못했으면 붙이지 않는다
pub(super) fn history_reply(id: u32, o: Option<aaa_vt::Older>) -> TermServerMsg {
    match o {
        Some(o) => TermServerMsg::History {
            compression: None,
            id,
            data: B64(o.data),
            lines: o.lines as u32,
            more: o.more,
            found: true,
        },
        None => {
            TermServerMsg::History { compression: None, id, data: B64(Vec::new()), lines: 0, more: false, found: false }
        }
    }
}

pub(super) fn frame_len(m: &Message) -> u64 {
    match m {
        Message::Text(t) => t.len() as u64,
        Message::Binary(b) => b.len() as u64,
        _ => 0,
    }
}

impl Outbox {
    pub(super) fn new(
        mut sink: futures_util::stream::SplitSink<WebSocket, Message>,
        front: &str,
        flow: PaneFlow,
    ) -> Self {
        let PaneFlow { gzip, requested, trace, .. } = flow;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
        let buffered = Arc::new(AtomicU64::new(0));
        let (stop, mut stopped) = tokio::sync::watch::channel(None);
        let left = buffered.clone();
        let writer_front = front.to_string();
        tokio::spawn(async move {
            let mut first = true;
            loop {
                // 연결이 그냥 끝나 보내는 쪽이 사라진 것(Err)은 정지가 아니다: 4008 없이 끝낸다
                let m = tokio::select! {
                    biased;
                    r = stopped.changed() => if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok() { break } else { return },
                    m = rx.recv() => match m { Some(m) => m, None => return },
                };
                let n = frame_len(&m);
                // 막힌 보내기도 닫기가 끊는다(보내던 frame은 tungstenite 버퍼에 통째로 있어 흐름이 깨지지 않는다)
                let ok = tokio::select! {
                    biased;
                    r = stopped.changed() => if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok() { break } else { return },
                    r = sink.send(m) => r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok(),
                };
                left.fetch_sub(n, Ordering::Relaxed);
                if !ok {
                    return;
                }
                if first {
                    first = false;
                    attach_event!(trace, front = %writer_front, elapsed_us = requested.elapsed().as_micros() as u64, "웹 터미널 첫 프레임 송신");
                }
            }
            // 따라오지 못했거나 기기가 취소됐다: 남은 줄은 버리고 그 code로 닫는다. 그 닫기도 못 가면 연결을 버린다.
            let (code, reason) = stopped.borrow().unwrap_or((OUTPUT_STALLED_CLOSE_CODE, OUTPUT_STALLED_REASON));
            let close = Message::Close(Some(CloseFrame { code, reason: reason.into() }));
            aaa_common::diagnostic::ignored_nested(
                tokio::time::timeout(std::time::Duration::from_secs(2), sink.send(close)).await,
                "웹소켓 닫기 실패 뒤 연결 해제",
            );
        });
        Self {
            tx,
            buffered,
            stop,
            front: front.to_string(),
            trace,
            payload: Arc::new(WebPayload {
                gzip,
                serial: tokio::sync::Mutex::new(()),
                pending: Arc::new(tokio::sync::Semaphore::new(1 << 20)),
            }),
        }
    }

    pub(super) fn send(&self, m: Message) -> bool {
        self.buffered.fetch_add(frame_len(&m), Ordering::Relaxed);
        self.tx
            .send(m)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok()
    }

    pub(super) fn msg(&self, m: &TermServerMsg) -> bool {
        self.send(Message::Text(serde_json::to_string(m).unwrap().into()))
    }

    /// 다른 task가 이 줄에 보낼 손잡이(보낸 bytes는 같은 `buffered`에 센다)
    pub(super) fn sender(&self) -> OutSender {
        OutSender {
            tx: self.tx.clone(),
            buffered: self.buffered.clone(),
            payload: self.payload.clone(),
            stop: self.stop.clone(),
        }
    }

    pub(super) fn buffered(&self) -> u64 {
        self.buffered.load(Ordering::Relaxed)
    }

    /// 출력을 따라오지 못했다: 4008로 닫는다. 다시 보내지 않는다(브라우저의 「다시 연결」이 새로 붙는다).
    /// 기록에는 어느 pane인지와 밀린 양(ACK 안 된 것·보내지 못한 것)을 남긴다: 무엇이 쏟아졌는지·누가 느렸는지를 가른다.
    pub(super) fn stop(&self, why: &str, pane: &str, pending: u64) {
        tracing::info!(why, pane, pending, buffered = self.buffered(), "웹 터미널 출력 정지(4008)");
        aaa_common::diagnostic::ignored(
            self.stop.send(Some((OUTPUT_STALLED_CLOSE_CODE, OUTPUT_STALLED_REASON))),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
    }

    /// 기기가 취소됐다: 밀린 출력도 보내지 않고 4401로 닫는다
    pub(super) fn revoked(&self) {
        aaa_common::diagnostic::ignored(
            self.stop.send(Some((DEVICE_REVOKED_CLOSE_CODE, DEVICE_REVOKED_REASON))),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
    }
}

/// snapshot 하나가 새 출력 구독이다: 새 번호의 창을 열고, replay가 느린 연결을 건널 시간을 준다(참고 `join`)
pub(super) async fn open_stream(
    out: &Outbox,
    streams: &mut u32,
    a: &mut crate::Attach,
    readonly: bool,
    now: u64,
) -> Option<OutputWindow> {
    *streams += 1;
    let mut window = OutputWindow::new(*streams);
    let bytes = a.replay.len() as u64;
    let started = std::time::Instant::now();
    let text = out
        .payload
        .prepare(TermServerMsg::Snapshot {
            compression: None,
            replay: B64(std::mem::take(&mut a.replay)),
            cols: a.cols,
            rows: a.rows,
            readonly,
            stream: *streams,
            history_more: a.more,
        })
        .await;
    let Some(text) = text else {
        tracing::warn!("웹 터미널 snapshot 거부");
        aaa_common::diagnostic::ignored(
            out.stop.send(Some((4002, "terminal payload rejected"))),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
        return None;
    };
    let wire = text.len() as u64;
    attach_event!(
        out.trace,
        front = %out.front, stream = *streams, raw_bytes = bytes, wire_bytes = wire,
        elapsed_us = started.elapsed().as_micros() as u64, "웹 터미널 snapshot 준비"
    );
    if bytes > 0 {
        window.join(wire, out.buffered(), now);
        window.write(bytes, wire);
    }
    out.send(Message::Text(text.into())).then_some(window)
}

/// 입력할 수 있는지와 그 이유(끊김 > 종료 > 권한 순)
pub(super) const COLOR_PENDING: &str = "터미널 입력 준비 중";

#[derive(Clone, Copy)]
pub(super) struct TerminalAccess {
    pub(super) connected: bool,
    pub(super) running: bool,
}

impl TerminalAccess {
    pub(super) fn from_pane(state: &crate::Inner, pane: &crate::PaneRt) -> Self {
        Self {
            connected: state.machines.get(&pane.machine_id).is_some_and(|machine| machine.conn.is_some()),
            running: pane.info.state.is_running(),
        }
    }

    pub(super) fn read(host: &Host, id: &str) -> Option<Self> {
        let state = host.inner.lock().unwrap();
        state.panes.get(id).map(|pane| Self::from_pane(&state, pane))
    }

    pub(super) fn reason(self, role: DeviceRole, preparing: bool) -> Option<&'static str> {
        if !self.connected {
            Some("agent와 연결이 끊겼다(읽기 전용)")
        } else if !self.running {
            Some("세션이 끝났다")
        } else if !role.can_control() {
            Some("보기 권한")
        } else if preparing {
            Some(COLOR_PENDING)
        } else {
            None
        }
    }

    pub(super) fn readonly(self, role: DeviceRole, preparing: bool) -> bool {
        self.reason(role, preparing).is_some()
    }

    pub(super) fn message(self, role: DeviceRole, preparing: bool) -> TermServerMsg {
        let reason = self.reason(role, preparing);
        TermServerMsg::Readonly { readonly: reason.is_some(), reason: reason.map(str::to_string) }
    }
}

pub(super) fn terminal_state(
    host: &Host,
    id: &str,
    role: DeviceRole,
    grid: bool,
) -> Option<(TerminalAccess, TermServerMsg)> {
    if grid {
        let state = host.inner.lock().unwrap();
        let pane = state.panes.get(id)?;
        let lease = pane.lease.as_ref().map(|l| aaa_proto::LeaseView { front: l.id.clone(), label: l.label.clone() });
        let grid = TermServerMsg::Grid { cols: pane.info.cols, rows: pane.info.rows, lease };
        Some((TerminalAccess::from_pane(&state, pane), grid))
    } else {
        let pane = host.pane_view_of(id, role)?;
        let access = TerminalAccess {
            connected: pane.connection == aaa_proto::Connection::Connected,
            running: pane.state.is_running(),
        };
        Some((access, TermServerMsg::State { pane: Box::new(pane) }))
    }
}

/// 스냅샷·ACK·기록은 준비 중에도 흐른다. 일찍 온 입력은 나중에 몰아서 보내지 않고 그 요청에 답한다.
pub(super) fn input_before_colors(msg: TermClientMsg) -> TermServerMsg {
    match msg {
        TermClientMsg::Submit { id, .. } => {
            TermServerMsg::Submitted { id, ok: false, message: Some(COLOR_PENDING.into()) }
        }
        TermClientMsg::SteerQueued { id, .. } => TermServerMsg::SteerQueued {
            id,
            sent: false,
            reason: Some(aaa_proto::SteerRefusal::Failed),
            message: Some(COLOR_PENDING.into()),
        },
        _ => TermServerMsg::Error(ErrorBody::new(ErrorCode::Unavailable, COLOR_PENDING)),
    }
}

/// WebSocket 하나가 대화형 front 하나다. 「지금 쓰는 화면」(lease)은 이 연결에 묶이고 끝나면 풀린다.
pub(super) async fn pane_loop(
    host: Arc<Host>,
    id: String,
    dev: String,
    role: DeviceRole,
    flow: PaneFlow,
    attach_id: Option<String>,
    socket: WebSocket,
) {
    let short: String = dev.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    let front = host.new_front(format!("웹({short})"));
    attach_event!(
        flow.trace,
        pane = %id, front = %front.id, attach_id = attach_id.as_deref(),
        elapsed_us = flow.requested.elapsed().as_micros() as u64, "웹 터미널 upgrade"
    );
    // 이 연결이 그 pane을 보는 동안(seen.rs). 가려지면 웹은 lease를 놓을 뿐 연결은 두므로 「보는 중」은 기기의 presence가 정한다
    host.view_term(&front.id, &dev, Some(&id));
    pane_session(&host, &id, &dev, role, &front, flow, socket).await;
    host.view_term(&front.id, &dev, None);
    host.front_gone(&front.id);
}

pub(super) async fn pane_session(
    host: &Arc<Host>,
    id: &str,
    dev: &str,
    role: DeviceRole,
    front: &FrontRef,
    flow: PaneFlow,
    socket: WebSocket,
) {
    let PaneFlow { acks, colors, grid, trace, .. } = flow;
    let mut startup = PaneStartup {
        flow,
        pane: id,
        front: &front.id,
        upgrade_ms: operation_log::ms(flow.checked.elapsed()),
        copy_ms: 0,
        snapshot_ms: 0,
        queued_at: None,
        reason: "revoked",
    };
    let id = id.to_string();
    let actor = format!("device:{dev}");
    // 취소를 듣기 시작한 뒤에 다시 본다: upgrade와 취소가 엇갈려도 놓치지 않는다(t3code SessionStore `awaitInvalidation`)
    let mut revoked = host.device_revoked.subscribe();
    let (sink, mut stream) = socket.split();
    let out = Outbox::new(sink, &front.id, flow);
    if host.is_revoked(dev) {
        out.revoked();
        return;
    }
    // 보이는 화면은 host가 이미 가지고 있다. agent에는 색을 보내고, 입력·크기만 그 답 뒤에 허용한다(plan §8.187).
    let color_started = std::time::Instant::now();
    let mut color_wait =
        colors.and_then(|c| host.front_colors(&front.id, &id, c)).filter(|wait| wait.borrow().is_none());
    let started_attach = std::time::Instant::now();
    startup.reason = "pane_gone";
    let attach = host.attach_pane(&id);
    startup.copy_ms = operation_log::ms(started_attach.elapsed());
    let Some(mut attach) = attach else { return };
    attach_event!(
        trace,
        front = %front.id, elapsed_us = started_attach.elapsed().as_micros() as u64,
        preparing = color_wait.is_some(), "웹 터미널 화면 사본"
    );
    let mut next = attach.offset;
    let mut upload: Option<crate::attach::Upload> = None;
    // 첨부가 실패하면 그 첨부의 남은 조각은 오류 없이 버린다(다음 첨부는 offset 0으로 시작한다)
    let mut upload_failed = false;
    let mut input_traces = std::collections::HashMap::<String, u8>::new();
    let Some((access, state)) = terminal_state(host, &id, role, grid) else { return };
    let started = std::time::Instant::now();
    let now = || started.elapsed().as_millis() as u64;
    let mut streams = 0;
    startup.reason = "front_enqueue_failed";
    if !out.msg(&TermServerMsg::Front { id: front.id.clone(), steer_queued: true }) {
        return;
    }
    if color_wait.as_ref().is_some_and(|wait| wait.borrow().is_some()) {
        color_wait = None;
    }
    let preparing = color_wait.is_some();
    startup.reason = "snapshot_failed";
    let snapshot_started = std::time::Instant::now();
    let mut snapshot_bytes = attach.replay.len() as u64;
    let window = open_stream(&out, &mut streams, &mut attach, access.readonly(role, preparing), now()).await;
    startup.snapshot_ms = operation_log::ms(snapshot_started.elapsed());
    let Some(mut window) = window else {
        return;
    };
    let mut rx = attach.rx;
    startup.reason = "state_enqueue_failed";
    if !out.msg(&access.message(role, preparing)) || !out.msg(&state) {
        return;
    }
    let mut opening = if acks {
        startup.queued_at = Some(std::time::Instant::now());
        startup.reason = "closed_before_snapshot_ack";
        Some(startup)
    } else {
        startup.reason = "snapshot_queued";
        drop(startup);
        None
    };
    // ACK가 아예 오지 않아도(가려진 탭) 정지를 잰다(참고 outputTimer 100ms)
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut queue_since = None;
    if host.is_revoked(dev) {
        out.revoked();
        return;
    }
    // submit을 기다리는 동안 받아 둔 frame(차례대로 먼저 읽는다)
    let mut later: std::collections::VecDeque<Option<Result<Message, axum::Error>>> = std::collections::VecDeque::new();
    'conn: loop {
        let verdict = if acks {
            window.check(out.buffered(), now())
        } else {
            check_unacked(out.buffered(), &mut queue_since, now())
        };
        if verdict == Verdict::Stop {
            if let Some(opening) = &mut opening {
                opening.reason = "snapshot_ack_stalled";
            }
            out.stop("ACK가 나아가지 않는다", &id, window.pending());
            break;
        }
        tokio::select! {
            biased;
            r = revoked.recv() => {
                // 놓쳤으면(Lagged) 이 기기인지 다시 본다
                let mine = match r {
                    Ok(gone) => gone == dev,
                    Err(RecvError::Lagged(_)) => host.is_revoked(dev),
                    Err(RecvError::Closed) => false,
                };
                if mine {
                    out.revoked();
                    break;
                }
            }
            applied = async {
                let r = color_wait.as_mut().unwrap().wait_for(Option::is_some).await;
                r.ok().and_then(|status| *status)
            }, if color_wait.is_some() => {
                // 앞과 같이 색 전송 실패·시한 뒤에는 agent 연결과 pane 권한으로 입력 가능 여부를 판단한다.
                color_wait = None;
                let Some(access) = TerminalAccess::read(host, &id) else { break };
                attach_event!(
                    trace,
                    front = %front.id, elapsed_us = color_started.elapsed().as_micros() as u64,
                    color_ok = applied.is_some_and(|receipt| receipt.ok),
                    color_queue_us = applied.map(|receipt| receipt.queue_us),
                    color_request_us = applied.map(|receipt| receipt.request_us), "웹 터미널 입력 준비"
                );
                if !out.msg(&access.message(role, false)) { break; }
            }
            _ = tick.tick() => {}
            ev = rx.recv() => {
                // 빈틈·지연(Lagged)·agent 재구독(Reset)은 host 안의 일이라 snapshot으로 다시 맞춘다. 브라우저가
                // 느린 것은 위 창이 잰다: 그때는 다시 보내지 않고 닫는다(참고: 과부하는 자동 replay하지 않는다).
                let resync = match ev {
                    Ok(PaneEvent::Output(start, data)) => {
                        let end = start + data.len() as u64;
                        if end <= next { false }
                        else if start > next { true }
                        else {
                            let part = data[(next - start) as usize..].to_vec();
                            next = end;
                            let n = part.len() as u64;
                            if (acks && window.pending() + n > OUTPUT_HARD_BYTES) || out.buffered() >= OUTPUT_HARD_BYTES {
                                out.stop("밀린 출력이 상한을 넘었다", &id, window.pending() + n);
                                break;
                            }
                            window.write(n, n);
                            if !out.send(Message::Binary(part.into())) { break; }
                            false
                        }
                    }
                    Ok(PaneEvent::Reset) | Err(RecvError::Lagged(_)) => true,
                    Ok(PaneEvent::State) => {
                        let Some((access, state)) = terminal_state(host, &id, role, grid) else { break };
                        if !out.msg(&access.message(role, color_wait.is_some())) || !out.msg(&state) { break; }
                        false
                    }
                    Err(RecvError::Closed) => break,
                };
                if resync {
                    let Some(mut a) = host.attach_pane(&id) else { break };
                    next = a.offset;
                    snapshot_bytes = a.replay.len() as u64;
                    let readonly = TerminalAccess::read(host, &id).is_none_or(|access| access.readonly(role, false));
                    let Some(w) = open_stream(&out, &mut streams, &mut a, readonly || color_wait.is_some(), now()).await else { break };
                    rx = a.rx;
                    window = w;
                }
            }
            m = async { match later.pop_front() { Some(m) => m, None => stream.next().await } } => {
                // 취소 사건보다 먼저 와 있던 입력도 받지 않는다(§6.4 「대기 중인 입력에도 바로」)
                if host.is_revoked(dev) {
                    out.revoked();
                    break;
                }
                let req = match m {
                    Some(Ok(Message::Text(t))) => match serde_json::from_str::<TermClientMsg>(&t) {
                        Ok(msg @ (TermClientMsg::Input { .. } | TermClientMsg::CtrlEnter { .. }
                            | TermClientMsg::Resize { .. } | TermClientMsg::Take | TermClientMsg::Attach { .. }
                            | TermClientMsg::Submit { .. } | TermClientMsg::SteerQueued { .. })) if color_wait.is_some() => {
                            if !out.msg(&input_before_colors(msg)) { break; }
                            None
                        }
                        Ok(TermClientMsg::Ack { stream, offset }) => {
                            // 앞 구독(snapshot 전)의 늦은 ACK는 그냥 버린다
                            if stream == window.id {
                                if !window.acknowledge(stream, offset) {
                                    let e = ErrorBody::new(ErrorCode::BadRequest, "ACK offset이 보낸 출력을 넘는다");
                                    if !out.msg(&TermServerMsg::Error(e)) { break; }
                                } else if offset >= snapshot_bytes && let Some(mut opened) = opening.take() {
                                    opened.reason = "snapshot_acknowledged";
                                }
                            }
                            None
                        }
                        Ok(TermClientMsg::Input { data }) => Some(FrontRequest::PaneInput { pane: id.clone(), data: B64(data.into_bytes()) }),
                        Ok(TermClientMsg::CtrlEnter { trace }) => {
                            let trace = trace.filter(|t| crate::input_log::valid_trace(t)).unwrap_or_else(aaa_common::util::new_id);
                            Some(FrontRequest::PaneCtrlEnter { pane: id.clone(), trace: Some(format!("{}:{trace}", front.id)) })
                        },
                        Ok(TermClientMsg::Resize { cols, rows }) if role.can_control() => {
                            Some(FrontRequest::PaneResize { pane: id.clone(), cols: cols.clamp(10, 500), rows: rows.clamp(3, 300) })
                        }
                        Ok(TermClientMsg::InputTrace { id: trace, event, method }) => {
                            if role.can_control() && crate::input_log::valid_trace(&trace)
                                && matches!(event, aaa_proto::InputTraceEvent::CtrlEnterBlocked | aaa_proto::InputTraceEvent::Preparing) && input_traces.len() < 128 {
                                input_traces.entry(trace.clone()).or_insert(0);
                            }
                            if role.can_control() && let Some(count) = input_traces.get_mut(&trace) && *count < 16 {
                                *count += 1;
                                host.input_event(&id, &format!("{}:{trace}", front.id), "ui", &format!("event={event:?} method={method:?}"));
                            }
                            None
                        }
                        Ok(TermClientMsg::Submit { id: sid, data, steer, trace, meta }) => {
                            let bytes = data.len();
                            let trace = trace.filter(|t| crate::input_log::valid_trace(t)).unwrap_or_else(|| sid.to_string());
                            if input_traces.len() >= 128 { input_traces.clear(); }
                            input_traces.insert(trace.clone(), 0);
                            let trace = format!("{}:{trace}", front.id);
                            host.input_event(&id, &trace, "received", &format!("{} pasted_source={} attachments={}", crate::input_log::submit_meta(&data, steer), meta.pasted, meta.attachments));
                            tracing::info!(pane = %id, front = %front.id, submit = sid, bytes, steer, "웹 입력: submit 수신");
                            host.touch_device(dev, "입력");
                            // 글과 Enter를 쓰는 동안에도 이 submit의 멈춤은 바로 답한다(plan §8.247). 다른 frame은 뒤에 차례대로 둔다:
                            // 이 연결의 다음 입력은 Enter 뒤에 간다
                            let stage = AtomicU8::new(SUBMIT_BEFORE);
                            let work = submit_work(host, role, &actor, front, dev, &id, sid, data, steer, meta.ctrl_enter || steer, &trace, &stage);
                            tokio::pin!(work);
                            let mut reading = true;
                            let end = loop {
                                tokio::select! {
                                    biased;
                                    end = &mut work => break end,
                                    m = stream.next(), if reading => {
                                        if let Some(Ok(Message::Text(t))) = &m
                                            && let Ok(TermClientMsg::SubmitCancel { id: cid }) = serde_json::from_str::<TermClientMsg>(t)
                                        {
                                            let answer = if cid == sid { cancel_submit(&stage) } else { SubmitStage::TooLate };
                                            host.input_event(&id, &trace, "cancel", &format!("stage={answer:?}"));
                                            tracing::info!(pane = %id, front = %front.id, submit = cid, ?answer, "웹 입력: 멈춤");
                                            if !out.msg(&TermServerMsg::SubmitCancelled { id: cid, stage: answer }) {
                                                reading = false;
                                            }
                                            continue;
                                        }
                                        reading = !matches!(m, Some(Ok(Message::Close(_))) | None | Some(Err(_)));
                                        later.push_back(m);
                                    }
                                }
                            };
                            let sent = match end {
                                SubmitEnd::Revoked => {
                                    tracing::warn!(pane = %id, front = %front.id, submit = sid, "웹 입력: 기기 폐기로 Enter 취소");
                                    out.revoked();
                                    break 'conn;
                                }
                                SubmitEnd::Stopped(at) => Err(ErrorBody::new(
                                    ErrorCode::BadRequest,
                                    if at == SubmitStage::NotSent { "멈춤: 보내지 않았다" } else { "멈춤: Enter를 보내지 않았다" },
                                )),
                                SubmitEnd::Done(r) => r,
                            };
                            host.db.audit(&actor, "web_submit", &format!("pane={id} front={} submit={sid} bytes={bytes} steer={steer} ok={}", front.id, sent.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok()));
                            host.input_event(&id, &trace, if sent.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok() { "delivered" } else { "failed" }, &format!("code={:?}", sent.as_ref().err().map(|e| &e.code)));
                            let reply = TermServerMsg::Submitted { id: sid, ok: sent.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok(), message: sent.err().map(|e| e.message) };
                            if !out.msg(&reply) {
                                tracing::warn!(pane = %id, front = %front.id, submit = sid, "웹 입력: 확인 응답 연결 닫힘");
                                break 'conn;
                            }
                            None
                        }
                        // 대기 메시지 「지금 보내기」(plan §8.271): 키만 보내고 답한다. submit 중이면 이 frame은 그 뒤(Enter 뒤)에 온다
                        Ok(TermClientMsg::SteerQueued { id: sid, trace }) => {
                            let trace = trace.filter(|t| crate::input_log::valid_trace(t)).unwrap_or_else(aaa_common::util::new_id);
                            let trace = format!("{}:{trace}", front.id);
                            host.touch_device(dev, "입력");
                            let reply = steer_queued(host, role, &actor, front, &id, sid, &trace).await;
                            if !out.msg(&reply) {
                                break 'conn;
                            }
                            None
                        }
                        // 진행 중인 submit이 없을 때의 멈춤: 이미 끝났다
                        Ok(TermClientMsg::SubmitCancel { id: cid }) => {
                            if !out.msg(&TermServerMsg::SubmitCancelled { id: cid, stage: SubmitStage::TooLate }) {
                                break 'conn;
                            }
                            None
                        }
                        Ok(TermClientMsg::History { id: hid, anchor, depth }) => {
                            // 앞 기록 한 덩어리(plan §8.67). 출력 흐름(ACK 창) 밖의 답이라 창에 넣지 않는다:
                            // 한 덩어리는 replay 기록 상한(2000줄·384KiB) 안이다.
                            let anchor: Vec<String> = anchor.into_iter().take(8).collect();
                            match host.older_history(&id, &anchor, depth as usize) {
                                crate::OlderHistory::Ready(o) => {
                                    if !out.sender().msg(history_reply(hid, o)).await {
                                        break;
                                    }
                                }
                                // mirror에 없는 앞 기록: session host에 묻는 동안 이 연결의 출력·입력을 막지 않는다(plan §8.118)
                                crate::OlderHistory::Ask(conn) => {
                                    let out = out.sender();
                                    let session = id.clone();
                                    tokio::spawn(async move {
                                        let o = Host::older_from_agent(&conn, session, anchor, depth as usize).await;
                                        out.msg(history_reply(hid, o)).await;
                                    });
                                }
                            }
                            None
                        }
                        // 테마를 바꿨다: 연 때처럼 agent가 받은 뒤 이 화면의 다음 메시지를 본다
                        Ok(TermClientMsg::Colors { fg, bg }) => {
                            let wait = aaa_proto::TermColors::from_hex(&fg, &bg).and_then(|c| host.front_colors(&front.id, &id, c));
                            if let Some(mut wait) = wait {
                                if color_wait.is_some() {
                                    color_wait = Some(wait);
                                } else {
                                    aaa_common::diagnostic::ignored(wait.wait_for(Option::is_some).await, "보조 작업 실패 뒤 기존 처리를 계속한다");
                                }
                            }
                            None
                        }
                        Ok(TermClientMsg::Take) => Some(FrontRequest::PaneTake { pane: id.clone() }),
                        Ok(TermClientMsg::Release) => Some(FrontRequest::PaneRelease { pane: id.clone() }),
                        Ok(TermClientMsg::Attach { offset, total, sha256, data }) => {
                            if offset != 0 && upload_failed {
                                continue;
                            }
                            let reply = match host.attach_chunk(role, &actor, front, &id, &mut upload, offset, total, &sha256, data.0).await {
                                Ok(None) => None,
                                Ok(Some(a)) => Some(TermServerMsg::Attached { path: a.path, inserted: a.inserted, note: a.note }),
                                Err(e) => Some(TermServerMsg::Error(e)),
                            };
                            upload_failed = matches!(reply, Some(TermServerMsg::Error(_)));
                            if let Some(r) = reply && !out.msg(&r) {
                                break;
                            }
                            None
                        }
                        _ => None,
                    },
                    Some(Ok(Message::Binary(_))) if color_wait.is_some() => {
                        if !out.msg(&TermServerMsg::Error(ErrorBody::new(ErrorCode::Unavailable, COLOR_PENDING))) { break; }
                        None
                    }
                    Some(Ok(Message::Binary(b))) => Some(FrontRequest::PaneInput { pane: id.clone(), data: B64(b.to_vec()) }),
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => None,
                };
                // pane에 친 기기가 쓰는 기기다(plan §8.105). 크기 걸기는 다시 붙을 때도 오므로 세지 않는다(누름은 웹이 touch로 알린다)
                if matches!(req, Some(FrontRequest::PaneInput { .. }) | Some(FrontRequest::PaneTake { .. })) {
                    host.touch_device(dev, "입력");
                }
                if let Some(req) = req
                    && let Err(e) = host.call_from(role, &actor, Some(front), req).await
                    && !out.msg(&TermServerMsg::Error(e))
                {
                    break;
                }
            }
        }
    }
}

/// 대기 메시지 「지금 보내기」 하나(plan §8.271). host 판정(`steer_queued_data`)이 고른 키를 바로 그 뒤에 쓴다: 판정과 쓰기 사이에
/// 다른 기다림이 없어 Codex Esc는 패널을 확인한 같은 mirror 상태에서 나간다(남는 틈은 agent까지 가는 길뿐이다).
/// 보기 권한은 판정 전에 거절한다. 보냄·거절·실패를 모두 입력 사건으로 남긴다(`steer_queued` 판정, `steer_sent`·`steer_failed`)
pub(super) async fn steer_queued(
    host: &Arc<Host>,
    role: DeviceRole,
    actor: &str,
    front: &FrontRef,
    pane: &str,
    sid: u32,
    trace: &str,
) -> TermServerMsg {
    use aaa_proto::SteerRefusal;
    let refused = |reason: SteerRefusal, message: Option<String>| TermServerMsg::SteerQueued {
        id: sid,
        sent: false,
        reason: Some(reason),
        message,
    };
    if !role.can_control() {
        host.input_event(pane, trace, "steer_queued", "reason=Failed role=readonly");
        return refused(SteerRefusal::Failed, Some("보기 권한 기기는 입력할 수 없다".into()));
    }
    let data = match host.steer_queued_data(pane, trace) {
        Ok(data) => data,
        Err(reason) => return refused(reason, None),
    };
    let sent = host
        .call_from(role, actor, Some(front), FrontRequest::PaneInput { pane: pane.to_string(), data: B64(data) })
        .await;
    host.db.audit(
        actor,
        "web_steer_queued",
        &format!(
            "pane={pane} front={} ok={}",
            front.id,
            sent.as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok()
        ),
    );
    match sent {
        Ok(_) => {
            host.input_event(pane, trace, "steer_sent", "agent_ack=true");
            TermServerMsg::SteerQueued { id: sid, sent: true, reason: None, message: None }
        }
        Err(e) => {
            host.input_event(pane, trace, "steer_failed", &format!("code={:?}", e.code));
            refused(SteerRefusal::Failed, Some(e.message))
        }
    }
}

/// submit 하나의 단계(plan §8.247). 일꾼과 멈춤이 compare-exchange로 다투어 둘 중 하나만 이긴다:
/// 멈춤이 이기면 일꾼은 그 다음 쓰기를 하지 않는다
pub(super) const SUBMIT_BEFORE: u8 = 0;

pub(super) const SUBMIT_TYPING: u8 = 1;

pub(super) const SUBMIT_TYPED: u8 = 2;

pub(super) const SUBMIT_ENTERING: u8 = 3;

pub(super) const SUBMIT_STOPPED_BEFORE: u8 = 4;

pub(super) const SUBMIT_STOPPED_NO_ENTER: u8 = 5;

pub(super) enum SubmitEnd {
    Done(Result<(), ErrorBody>),
    Stopped(SubmitStage),
    /// 기다리는 동안 기기가 폐기됐다: Enter를 보내지 않고 연결을 닫는다(t-0340 감사 A9, t3code `awaitInvalidation`)
    Revoked,
}

/// 멈춤 요청: 아직 아무것도 쓰지 않았으면 쓰지 않고, 글을 쓰는 중이거나 썼으면 Enter를 보내지 않는다. Enter를 보내기
/// 시작했으면 늦었다. 같은 submit을 두 번 멈추면 처음 답을 다시 준다
pub(super) fn cancel_submit(stage: &AtomicU8) -> SubmitStage {
    loop {
        let now = stage.load(Ordering::SeqCst);
        let (next, answer) = match now {
            SUBMIT_BEFORE => (SUBMIT_STOPPED_BEFORE, SubmitStage::NotSent),
            SUBMIT_TYPING | SUBMIT_TYPED => (SUBMIT_STOPPED_NO_ENTER, SubmitStage::NoEnter),
            SUBMIT_STOPPED_BEFORE => return SubmitStage::NotSent,
            SUBMIT_STOPPED_NO_ENTER => return SubmitStage::NoEnter,
            _ => return SubmitStage::TooLate,
        };
        if stage
            .compare_exchange(now, next, Ordering::SeqCst, Ordering::SeqCst)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok()
        {
            return answer;
        }
    }
}

/// 일꾼이 다음 단계로 간다. 멈춤이 먼저 이겼으면 false(그 쓰기를 하지 않는다)
pub(super) fn advance_submit(stage: &AtomicU8, from: u8, to: u8) -> bool {
    stage
        .compare_exchange(from, to, Ordering::SeqCst, Ordering::SeqCst)
        .as_ref()
        .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok()
}

/// 글, 잠깐 뒤 Enter(한 덩어리로 보내면 Codex가 붙여넣기로 보고 Enter를 줄바꿈으로 넣는다). 답은 둘 다 pane에 들어간 뒤다.
/// 쓰기마다 그 앞에서 멈춤을 본다: 막힌 agent에 간 글 요청은 거둘 수 없지만 Enter는 보내지 않는다
#[allow(clippy::too_many_arguments)]
pub(super) async fn submit_work(
    host: &Arc<Host>,
    role: DeviceRole,
    actor: &str,
    front: &FrontRef,
    dev: &str,
    pane: &str,
    sid: u32,
    data: String,
    steer: bool,
    ctrl_enter: bool,
    trace: &str,
    stage: &AtomicU8,
) -> SubmitEnd {
    let typed = |data: Vec<u8>| FrontRequest::PaneInput { pane: pane.to_string(), data: B64(data) };
    if !advance_submit(stage, SUBMIT_BEFORE, SUBMIT_TYPING) {
        return SubmitEnd::Stopped(SubmitStage::NotSent);
    }
    let sent = host.call_from(role, actor, Some(front), typed(data.into_bytes())).await;
    tracing::info!(pane, front = %front.id, submit = sid, ok = sent.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok(), code = ?sent.as_ref().err().map(|e| &e.code), phase = "text", "웹 입력: agent 응답");
    if let Err(e) = sent {
        return SubmitEnd::Done(Err(e));
    }
    host.input_event(pane, trace, "written_text", "agent_ack=true");
    if !advance_submit(stage, SUBMIT_TYPING, SUBMIT_TYPED) {
        return SubmitEnd::Stopped(SubmitStage::NoEnter);
    }
    crate::text::submission_gap().await;
    if host.is_revoked(dev) {
        return SubmitEnd::Revoked;
    }
    if !advance_submit(stage, SUBMIT_TYPED, SUBMIT_ENTERING) {
        return SubmitEnd::Stopped(SubmitStage::NoEnter);
    }
    // Ctrl+Enter로 보낸 글: 작업 중인 Claude는 Enter 대신 sendNow로 지금 턴에 넣는다(그 밖은 Enter, plan §8.239 후속)
    let enter = host.submit_key_data(pane, steer, ctrl_enter.then_some(trace));
    let sent = host.call_from(role, actor, Some(front), typed(enter)).await;
    tracing::info!(pane, front = %front.id, submit = sid, ok = sent.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_ok(), code = ?sent.as_ref().err().map(|e| &e.code), phase = if steer { "steer" } else { "enter" }, "웹 입력: agent 응답");
    if sent
        .as_ref()
        .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok()
    {
        host.input_event(pane, trace, "written_submit", if steer { "steer=true" } else { "key=enter" });
    }
    SubmitEnd::Done(sent.map(|_| ()))
}
