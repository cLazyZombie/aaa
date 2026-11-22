//! session-host 연결: 붙기, 출력 pump, 끊김 뒤 다시 붙기.

use super::*;

pub(super) enum PumpCmd {
    /// host 요청을 session host로 보낸다. 응답은 pump가 host에 보낸다.
    Host { host_id: String, body: SessionRequest },
    /// host 연결이 끊겼다: 구독을 멈춘다.
    HostGone,
    /// 끝난 세션을 지운다(forget): session host가 더 응답하는 동안을 기다리지 않고 연결을 닫는다.
    Close,
    /// 화면 색이 바뀌었다: session host에 알린다(응답은 버린다)
    Colors(aaa_proto::TermColors),
}

pub(super) struct Entry {
    pub(super) info: SessionInfo,
    pub(super) pump: Option<mpsc::Sender<PumpCmd>>,
}

pub(super) type SessionRx = FrameRx<tokio::io::ReadHalf<aaa_common::ipc::LocalStream>>;

/// 인사를 마친 session host 연결
pub(super) struct SessionConn {
    pub(super) tx: FrameTx,
    pub(super) rx: SessionRx,
    pub(super) proto: aaa_proto::Version,
    pub(super) info: SessionInfo,
}

/// session host에 붙지 못한 까닭
pub(super) enum ConnectError {
    /// 받는 쪽이 없다(socket이 없거나 아무도 listen하지 않는다): session host가 사라졌다
    Gone(anyhow::Error),
    /// session host가 거부했다(token·판). 다시 해도 같다(herdr `ClientEndpointStatus::Attention`)
    Rejected(anyhow::Error),
    /// 그 밖(인사 시한, 인사 중 끊김): 다시 해 볼 만하다
    Retry(anyhow::Error),
}

impl ConnectError {
    pub(super) fn into_error(self) -> anyhow::Error {
        match self {
            ConnectError::Gone(e) | ConnectError::Rejected(e) | ConnectError::Retry(e) => e,
        }
    }
}

/// pump가 session host 연결을 놓은 까닭
pub(super) enum Dropped {
    /// forget이 닫았거나 세션 항목이 사라졌다: 다시 붙지 않는다
    Closed,
    /// 연결이 끝났다(까닭). session host가 끝나지 않았으면 다시 붙는다
    Lost(String),
}

/// session host에 연결해 인사한다(token·판)
pub(super) async fn connect_session(paths: &SessionPaths, spec: &SpecFile) -> Result<SessionConn, ConnectError> {
    let stream = aaa_common::ipc::connect(&paths.sock).await.map_err(|e| {
        let gone = matches!(e.kind(), std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused);
        let e = anyhow::Error::new(e).context("session socket 연결");
        if gone { ConnectError::Gone(e) } else { ConnectError::Retry(e) }
    })?;
    let (r, w) = tokio::io::split(stream);
    let tx = spawn_writer(w, MAX_FRAME);
    let mut rx = FrameRx::new(r, MAX_FRAME);
    let hello = SessionMsg::Hello { proto: SESSION_IPC, token: spec.token.clone() };
    tx.json(&hello).await.map_err(|e| ConnectError::Retry(e.into()))?;
    let welcome = match tokio::time::timeout(Duration::from_secs(5), rx.control()).await {
        Ok(Ok(w)) => w,
        Ok(Err(e)) => return Err(ConnectError::Retry(e.context("welcome 대기"))),
        Err(e) => return Err(ConnectError::Retry(anyhow::Error::new(e).context("welcome 대기"))),
    };
    match welcome {
        SessionMsg::Welcome { proto, info, account_rules } => {
            if let Some(rules) = account_rules {
                aaa_common::redact::restore(&rules.0)
                    .map_err(|_| ConnectError::Rejected(anyhow::anyhow!("인증 가림 규칙 오류")))?;
            }
            Ok(SessionConn { tx, rx, proto, info })
        }
        SessionMsg::Reject(e) => Err(ConnectError::Rejected(anyhow::anyhow!("session host가 거부했다: {e}"))),
        other => Err(ConnectError::Retry(anyhow::anyhow!("예상하지 못한 응답: {other:?}"))),
    }
}

/// session host 프로세스가 있다(`meta.json`의 pid). 좀비이거나 pid가 다시 쓰였으면 있다고 하지만, 그때는 다시 붙을 때
/// 받는 쪽이 없어(`ConnectError::Gone`) 안다
pub(super) fn session_host_alive(paths: &SessionPaths) -> bool {
    util::read_json::<MetaFile>(&paths.meta())
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok_and(|m| util::pid_alive(m.host_pid))
}

/// 끊긴 뒤 `attempt`번째로 다시 붙기 전 간격: 0.5초부터 두 배씩, 많아야 30초(herdr `client/endpoint/supervisor.rs`
/// `retry_delay`·Local의 `MAX_LOCAL_RETRY_DELAY`)
pub(super) fn reattach_delay(attempt: u32) -> Duration {
    const INITIAL: Duration = Duration::from_millis(500);
    const MAX: Duration = Duration::from_secs(30);
    INITIAL.saturating_mul(1u32.checked_shl(attempt.saturating_sub(1).min(8)).unwrap_or(u32::MAX)).min(MAX)
}

/// pump의 화면 감지: 띄울 때 정한 harness의 감지 차례(`watch`), 또는 셸 pane 앞 agent 찾기(`front`, 셸 pid와 상태).
/// 앞에서 agent를 잡으면 그 harness로 `watch`를 만든다. `detect_ipc`: session host가 `Detect`를 받는다.
/// `colors_ipc`: session host가 `Colors`를 받는다(감지는 아니지만 같은 session host 판으로 정한다).
pub(super) struct Detecting {
    pub(super) watch: Option<detect::Watch>,
    pub(super) front: Option<(u32, detect::foreground::Presence)>,
    pub(super) detect_ipc: bool,
    pub(super) colors_ipc: bool,
}

/// 셸 pane 앞의 agent를 한 차례 본다. herdr의 Windows 프로세스 트리 조사는 옮기지 않았다(unix만, plan §8.110).
pub(super) fn foreground_tick(
    p: &mut detect::foreground::Presence,
    shell_pid: u32,
    now: std::time::Instant,
) -> Option<detect::foreground::Change> {
    #[cfg(unix)]
    {
        p.tick(now, cwd::foreground_process_group_id(shell_pid), |g| detect::foreground::probe(shell_pid, g))
    }
    #[cfg(not(unix))]
    {
        let _ = (p, shell_pid, now);
        None
    }
}

/// session host가 SESSION_IPC 1.1 이상일 때만 `Detect`를 보낸다(그 전 session host는 모르는 요청을 받으면 연결을 끊는다)
pub(super) fn detect_ipc(proto: aaa_proto::Version) -> bool {
    proto.major == SESSION_IPC.major && proto.minor >= aaa_proto::SESSION_IPC_DETECT
}

pub(super) fn colors_ipc(proto: aaa_proto::Version) -> bool {
    proto.major == SESSION_IPC.major && proto.minor >= aaa_proto::SESSION_IPC_COLORS
}

/// 화면으로 상태를 볼 세션이면 감지 차례
pub(super) fn watch_of(spec: &SpecFile, info: &SessionInfo, detect_ipc: bool) -> Option<detect::Watch> {
    if !info.state.is_running() {
        return None;
    }
    let harness = detect::Harness::of_session(spec.spec.harness.as_deref(), &spec.spec.argv)?;
    let now = std::time::Instant::now();
    // 띄운 지 오래된 세션(agent가 다시 떠 붙었다)은 시작 유예가 이미 끝났다
    let age = Duration::from_millis(util::now_ms().saturating_sub(info.started_at_ms));
    let started = now.checked_sub(age).unwrap_or(now);
    Some(detect::Watch::new(harness, started, detect_ipc, now))
}

/// `at`까지 잔다(None이면 깨지 않는다)
pub(super) async fn sleep_until(at: Option<std::time::Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(tokio::time::Instant::from_std(t)).await,
        None => std::future::pending().await,
    }
}

impl Agent {
    /// 시작할 때: 세션 폴더를 읽어 살아 있는 session host에 다시 붙고, 나머지는 종료·중단·유실로 분류한다.
    pub async fn recover(self: &Arc<Self>) -> Result<()> {
        let dir = self.state.join("sessions");
        let mut ids: Vec<String> = std::fs::read_dir(&dir)?
            .filter_map(|e| {
                e.inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
            })
            .filter(|e| e.path().join("spec.json").exists())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        ids.sort();
        for id in ids {
            let paths = SessionPaths::new(&self.state, &id);
            let spec: SpecFile = match util::read_json(&paths.spec()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(%id, "spec.json 읽기 실패: {e:#}");
                    continue;
                }
            };
            if let Some(label) = &spec.label {
                self.labels.lock().unwrap().insert(id.clone(), label.clone());
            }
            if let Ok(list) = util::read_json::<Vec<aaa_proto::HarnessEvent>>(&paths.harness()) {
                self.harness_last
                    .lock()
                    .unwrap()
                    .insert(id.clone(), list.into_iter().filter(|e| e.session == id).collect());
            }
            if paths.exit().exists() {
                let info = self.offline_info(&paths, &spec);
                self.sessions.lock().unwrap().insert(id, Entry { info, pump: None });
                continue;
            }
            match self.attach(&paths, &spec).await {
                Ok(_) => tracing::info!(%id, "세션에 다시 붙었다"),
                Err(e) => {
                    let info = self.offline_info(&paths, &spec);
                    tracing::info!(%id, state = ?info.state, "붙지 못했다: {e:#}");
                    self.sessions.lock().unwrap().insert(id, Entry { info, pump: None });
                }
            }
        }
        Ok(())
    }

    /// 화면 색을 받았다: 새 세션에 실을 것으로 두고 모든 pump에 넘긴 뒤 답한다. host는 이 답을 받고서야 그 화면에 pane을
    /// 보이므로 그 화면이 본 뒤의 요청은 pump에서 색 뒤에 선다(plan §8.187)
    pub(super) async fn set_term_colors(&self, c: aaa_proto::TermColors) {
        *self.term_colors.lock().unwrap() = Some(c);
        let pumps: Vec<_> = self.sessions.lock().unwrap().values().filter_map(|e| e.pump.clone()).collect();
        for p in pumps {
            aaa_common::diagnostic::ignored(
                p.send(PumpCmd::Colors(c)).await,
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
    }

    /// session host가 없는 세션의 정보(파일에서)
    pub(super) fn offline_info(&self, paths: &SessionPaths, spec: &SpecFile) -> SessionInfo {
        let meta: Option<MetaFile> = util::read_json(&paths.meta())
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok();
        let exit: Option<ExitFile> = util::read_json(&paths.exit())
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok();
        let state = match &exit {
            Some(x) => SessionState::Exited { code: x.code },
            None if spec.boot_id != self.boot_id => SessionState::Interrupted,
            None => SessionState::Lost,
        };
        self.labeled(SessionInfo {
            responsibility: None,
            id: spec.id.clone(),
            name: spec.spec.name.clone(),
            argv: spec.spec.argv.clone(),
            cwd: spec.spec.cwd.clone(),
            pid: meta.as_ref().and_then(|m| m.child_pid),
            started_at_ms: meta.map(|m| m.started_at_ms).unwrap_or(spec.created_at_ms),
            boot_id: spec.boot_id.clone(),
            cols: exit.as_ref().map(|x| x.cols).unwrap_or(spec.spec.cols),
            rows: exit.as_ref().map(|x| x.rows).unwrap_or(spec.spec.rows),
            state,
            role: spec.spec.role,
            label: None,
            harness: spec.spec.harness.clone(),
        })
    }

    /// session host나 옛 spec이 준 정보에 사람이 붙인 이름을 덮는다
    pub(super) fn labeled(&self, mut info: SessionInfo) -> SessionInfo {
        if let Some(label) = self.labels.lock().unwrap().get(&info.id) {
            info.name = label.clone();
            info.label = Some(label.clone());
        }
        info.responsibility = responsibility::observe(&SessionPaths::new(&self.state, &info.id), &info);
        info
    }

    /// session host에 연결하고 pump를 띄운다.
    pub(super) async fn attach(self: &Arc<Self>, paths: &SessionPaths, spec: &SpecFile) -> Result<SessionInfo> {
        let SessionConn { tx, rx, proto, info } =
            connect_session(paths, spec).await.map_err(ConnectError::into_error)?;
        let info = self.labeled(info);
        let detect_ipc = detect_ipc(proto);
        let colors_ipc = colors_ipc(proto);
        let watch = watch_of(spec, &info, detect_ipc);
        // 띄울 때 harness를 모르는 pane(셸)은 herdr처럼 앞 프로세스 이름으로 agent를 찾는다(plan §8.110)
        let front = info
            .pid
            .filter(|_| cfg!(unix) && watch.is_none() && info.state.is_running())
            .map(|pid| (pid, detect::foreground::Presence::new(std::time::Instant::now())));
        let (ptx, prx) = mpsc::channel(256);
        self.sessions.lock().unwrap().insert(info.id.clone(), Entry { info: info.clone(), pump: Some(ptx) });
        self.session_ready.notify_waiters();
        let me = self.clone();
        let paths = paths.clone();
        let spec = spec.clone();
        tokio::spawn(async move {
            me.pump(paths, spec, tx, rx, prx, Detecting { watch, front, detect_ipc, colors_ipc }).await
        });
        Ok(info)
    }

    /// 세션 하나의 중계. session host 연결이 끊겼는데 session host가 끝나지 않았으면(`exit.json` 없음) 다시 붙는다(`reattach`).
    pub(super) async fn pump(
        self: Arc<Self>,
        paths: SessionPaths,
        spec: SpecFile,
        mut tx: FrameTx,
        mut rx: SessionRx,
        mut cmds: mpsc::Receiver<PumpCmd>,
        detecting: Detecting,
    ) {
        let id = spec.id.clone();
        let Detecting { mut watch, mut front, detect_ipc, colors_ipc } = detecting;
        let mut shell_chat = Some(shell_chat::Tracker::new());
        let mut seq = 0u64;
        let pending = loop {
            let mut pending: HashMap<String, (String, bool)> = HashMap::new();
            let mut forwarding = false;
            // 이 연결의 session host에 지금 화면 색을 알린다(다시 붙은 session host, agent보다 오래 산 session host). 쓰지 못했으면
            // 아래 읽기가 끊김을 본다
            let colors = *self.term_colors.lock().unwrap();
            if let Some(c) = colors.filter(|_| colors_ipc) {
                seq += 1;
                aaa_common::diagnostic::ignored(
                    tx.json(&SessionMsg::Req { id: format!("c{seq}"), body: SessionRequest::Colors(c) }).await,
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
            }
            let dropped = loop {
                let watch_at = watch.as_ref().and_then(detect::Watch::next_at);
                let chat_at = front.as_ref().and_then(|_| shell_chat.as_ref().map(|c| c.next));
                let detect_at =
                    watch_at.into_iter().chain(front.as_ref().map(|(_, p)| p.next_at())).chain(chat_at).min();
                tokio::select! {
                    _ = sleep_until(detect_at) => {
                        let now = std::time::Instant::now();
                        if let Some((pid, _)) = front.as_ref()
                            && shell_chat.as_ref().is_some_and(|c| c.next <= now)
                        {
                            let mut tracker = shell_chat.take().expect("셸 대화 감시");
                            let (pid, codex, claude) = (*pid, self.codex_root_for(Some(&id)), self.claude_root_for(Some(&id)));
                            let result = tokio::task::spawn_blocking(move || {
                                let changed = tracker.poll(pid, &codex, &claude);
                                (tracker, changed)
                            }).await;
                            let Ok((tracker, changed)) = result.inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")) else {
                                break Dropped::Lost("셸 대화 기록 조사 task가 끝났다".into());
                            };
                            shell_chat = Some(tracker);
                            if let Some((h, b)) = changed {
                                self.shell_chat_event(&id, h, b).await;
                            }
                        }
                        if let Some((pid, p)) = front.as_mut()
                            && p.next_at() <= now
                        {
                            match foreground_tick(p, *pid, now) {
                                Some(detect::foreground::Change::Acquired(h)) => {
                                    self.harness_process_started(&id, util::now_ms());
                                    watch = Some(detect::Watch::new(h, now, detect_ipc, now));
                                }
                                Some(detect::foreground::Change::Exited(h)) => {
                                    watch = None;
                                    let r = detect::Report { kind: "idle", message: None, question_kind: None, rule: None };
                                    self.detected(&id, h, r).await;
                                }
                                Some(detect::foreground::Change::Released(h)) => {
                                    watch = None;
                                    let r = detect::Report { kind: "released", message: None, question_kind: None, rule: None };
                                    self.detected(&id, h, r).await;
                                }
                                None => {}
                            }
                        }
                        let due = watch.as_ref().and_then(detect::Watch::next_at).is_some_and(|t| t <= now);
                        if let Some((sid, body)) = watch.as_mut().filter(|_| due).and_then(|w| w.due(now))
                            && tx.json(&SessionMsg::Req { id: sid, body }).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err()
                        {
                            break Dropped::Lost("session host에 쓰지 못했다".into());
                        }
                    }
                    cmd = cmds.recv() => match cmd {
                        Some(PumpCmd::Host { host_id, body }) => {
                            if matches!(body, SessionRequest::Input { .. }) && let Some(w) = watch.as_mut() {
                                w.input_sent(std::time::Instant::now());
                            }
                            seq += 1;
                            let sid = format!("h{seq}");
                            let is_sub = matches!(body, SessionRequest::Subscribe { .. });
                            if matches!(body, SessionRequest::Unsubscribe) { forwarding = false; }
                            pending.insert(sid.clone(), (host_id, is_sub));
                            if tx.json(&SessionMsg::Req { id: sid, body }).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                                break Dropped::Lost("session host에 쓰지 못했다".into());
                            }
                        }
                        Some(PumpCmd::HostGone) => {
                            if forwarding {
                                forwarding = false;
                                seq += 1;
                                aaa_common::diagnostic::ignored(tx.json(&SessionMsg::Req { id: format!("u{seq}"), body: SessionRequest::Unsubscribe }).await, "수신자가 끝난 뒤에도 현재 작업은 마친다");
                            }
                        }
                        Some(PumpCmd::Close) | None => break Dropped::Closed,
                        // 답(`c…`)은 pending에 없어 버린다
                        Some(PumpCmd::Colors(c)) => {
                            seq += 1;
                            if colors_ipc
                                && tx.json(&SessionMsg::Req { id: format!("c{seq}"), body: SessionRequest::Colors(c) }).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err()
                            {
                                break Dropped::Lost("session host에 쓰지 못했다".into());
                            }
                        }
                    },
                    incoming = rx.next() => match incoming {
                        Ok(Some(Incoming::Terminal(chunk))) => {
                            if forwarding {
                                match self.host_tx() {
                                    Some(h) => { if h.terminal(&chunk).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { forwarding = false; } }
                                    None => forwarding = false,
                                }
                            }
                        }
                        Ok(Some(Incoming::Control(b))) => match serde_json::from_slice::<SessionMsg>(&b) {
                            Ok(SessionMsg::Res { id: sid, mut result }) => {
                                if aaa_common::terminal_payload::validate_ipc_response(&mut result).as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                                    break Dropped::Lost("IPC 터미널 본문 거부".into());
                                }
                                if let Some(w) = watch.as_mut().filter(|w| w.owns(&sid)) {
                                    let harness = w.harness();
                                    if harness == detect::Harness::Codex && let Outcome::Ok(AgentResponse::Detect(d)) = &result {
                                        self.detect_codex_speed(&id, &d.osc_title).await;
                                    }
                                    if let Some(r) = w.answer(std::time::Instant::now(), result) {
                                        self.detected(&id, harness, r).await;
                                    }
                                } else if let Some((host_id, is_sub)) = pending.remove(&sid) {
                                    if is_sub && matches!(result, Outcome::Ok(_)) { forwarding = true; }
                                    self.send_host(&LinkMsg::Res { id: host_id, result }).await;
                                }
                            }
                            Ok(SessionMsg::AccountRefresh(mut request)) => {
                                request.session=id.clone();
                                self.send_host(&LinkMsg::Event(AgentEvent::AccountRefresh(request))).await;
                            }
                            Ok(SessionMsg::Updated(info)) => {
                                let info = self.labeled(info);
                                if let Some(e) = self.sessions.lock().unwrap().get_mut(&id) { e.info = info.clone(); }
                                // 종료 기록과 마지막 화면은 이 Updated 전에 확정됐다.
                                // session host가 늦은 연결에 더 응답하는 동안에도 harness는
                                // 끝난 상태다. 감시는 멈추되 pump는 마지막 화면 요청을 잇는다.
                                if !info.state.is_running() {
                                    #[cfg(target_os = "macos")]
                                    if spec.spec.harness.as_deref() == Some("agy") {
                                        agy::sandbox_progress::clean(&id);
                                    }
                                    if let Some(ended) = watch.take() {
                                        self.detected(&id, ended.harness(), detect::Report {
                                            kind: "exited", message: None, question_kind: None, rule: None,
                                        }).await;
                                    }
                                    front = None;
                                    shell_chat = None;
                                }
                                self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info))).await;
                            }
                            Ok(SessionMsg::Resync) => {
                                forwarding = false;
                                self.send_host(&LinkMsg::Event(AgentEvent::Resync { session: id.clone() })).await;
                            }
                            Ok(other) => tracing::debug!(%id, "무시한 session 메시지: {other:?}"),
                            Err(e) => break Dropped::Lost(format!("session 메시지를 해석하지 못했다: {e}")),
                        },
                        Ok(None) => break Dropped::Lost("session host가 연결을 닫았다".into()),
                        // frame 오류는 session host가 끝난 것이 아니다(plan §8.162)
                        Err(e) => break Dropped::Lost(format!("session host frame을 읽지 못했다: {e}")),
                    },
                }
            };
            // 이 연결로 보낸 감지 요청에는 답이 오지 않는다
            if let Some(w) = watch.as_mut() {
                w.connection_lost(std::time::Instant::now());
            }
            // 끝난 session host는 종료 기록(`exit.json`)을 쓴 뒤에 연결을 닫는다
            let why = match dropped {
                Dropped::Lost(_) if paths.exit().exists() => break pending,
                Dropped::Lost(why) if !session_host_alive(&paths) => {
                    tracing::info!(%id, "session host가 없다({why}): 연결을 놓는다");
                    break pending;
                }
                Dropped::Lost(why) => why,
                Dropped::Closed => break pending,
            };
            tracing::warn!(%id, "session host 연결이 끊겼다({why}): 다시 붙는다");
            for (_, (host_id, _)) in pending {
                let err = ErrorBody::new(ErrorCode::Unavailable, format!("session host 연결이 끊겼다: {why}"));
                self.send_host(&LinkMsg::Res { id: host_id, result: Outcome::Err(err) }).await;
            }
            let Some(conn) = self.reattach(&id, &paths, &spec, &mut cmds).await else { break HashMap::new() };
            let info = self.labeled(conn.info);
            (tx, rx) = (conn.tx, conn.rx);
            if let Some(e) = self.sessions.lock().unwrap().get_mut(&id) {
                e.info = info.clone();
            }
            self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info))).await;
            // host는 mirror의 마지막 offset부터 다시 구독한다(session host 보존 범위 안이면 빈틈 없이 bytes만)
            self.send_host(&LinkMsg::Event(AgentEvent::Resync { session: id.clone() })).await;
        };
        // session host가 끝났다(또는 사라졌다, forget이 닫았다): 종료 기록을 기다렸다 읽는다.
        for _ in 0..20 {
            if paths.exit().exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let info = self.offline_info(&paths, &spec);
        // harness가 끝났다(herdr는 프로세스가 사라지면 agent를 놓는다. aaa 세션은 harness 하나라 세션 끝이 그것이다)
        if let Some(w) = &watch
            && !info.state.is_running()
        {
            let r = detect::Report { kind: "exited", message: None, question_kind: None, rule: None };
            self.detected(&id, w.harness(), r).await;
        }
        if let Some(e) = self.sessions.lock().unwrap().get_mut(&id) {
            e.info = info.clone();
            e.pump = None;
        }
        for (_, (host_id, _)) in pending {
            let err = ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다");
            self.send_host(&LinkMsg::Res { id: host_id, result: Outcome::Err(err) }).await;
        }
        self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info))).await;
    }

    /// 끊긴 session host에 다시 붙는다. herdr가 Local endpoint와의 연결이 끊기면 하는 것과 같다(`client/endpoint/supervisor.rs`
    /// `disconnected` → Reconnecting, `reattach_delay` 간격, 붙으면 다음 끊김은 다시 첫 간격부터). 기다리는 동안 온 host 요청에는
    /// 「다시 붙는 중」으로 답한다(herdr는 끊긴 endpoint의 요청을 취소한다). herdr의 Local 서버는 사람이 다시 띄울 수 있지만
    /// session host는 다시 뜨지 않으므로, 끝났거나(`exit.json`) 사라졌거나(받는 쪽 없음) 거부하면 그만둔다. forget이 닫아도 그만둔다.
    pub(super) async fn reattach(
        &self,
        id: &str,
        paths: &SessionPaths,
        spec: &SpecFile,
        cmds: &mut mpsc::Receiver<PumpCmd>,
    ) -> Option<SessionConn> {
        let mut attempt = 0u32;
        loop {
            attempt = attempt.saturating_add(1);
            let until = tokio::time::Instant::now() + reattach_delay(attempt);
            loop {
                tokio::select! {
                    _ = tokio::time::sleep_until(until) => break,
                    cmd = cmds.recv() => match cmd {
                        Some(PumpCmd::Host { host_id, .. }) => {
                            let err = ErrorBody::new(ErrorCode::Unavailable, "세션에 다시 붙는 중이다");
                            self.send_host(&LinkMsg::Res { id: host_id, result: Outcome::Err(err) }).await;
                        }
                        // 다시 붙으면 그때의 색을 보낸다
                        Some(PumpCmd::HostGone) | Some(PumpCmd::Colors(_)) => {}
                        Some(PumpCmd::Close) | None => return None,
                    },
                }
            }
            if paths.exit().exists() {
                return None;
            }
            match connect_session(paths, spec).await {
                Ok(conn) => {
                    tracing::info!(%id, attempt, "세션에 다시 붙었다");
                    return Some(conn);
                }
                Err(ConnectError::Retry(e)) => {
                    let next = reattach_delay(attempt.saturating_add(1));
                    tracing::info!(%id, attempt, "다시 붙지 못했다({e:#}): {}ms 뒤 다시 한다", next.as_millis());
                }
                Err(e) => {
                    tracing::warn!(%id, attempt, "session host에 다시 붙지 못해 놓는다: {:#}", e.into_error());
                    return None;
                }
            }
        }
    }

    pub(super) async fn to_session(&self, id: String, session: &str, body: SessionRequest) {
        let (pump, info) = {
            let map = self.sessions.lock().unwrap();
            match map.get(session) {
                Some(e) => (e.pump.clone(), Some(e.info.clone())),
                None => (None, None),
            }
        };
        let result = match (pump, info) {
            (Some(p), _) => {
                if p.send(PumpCmd::Host { host_id: id.clone(), body })
                    .await
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
                {
                    return;
                }
                Err(ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다"))
            }
            (None, Some(info)) => self.offline_request(&info, body),
            (None, None) => Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}"))),
        };
        self.send_host(&LinkMsg::Res { id, result: result.into() }).await;
    }

    /// 끝난 세션: 마지막 화면 파일로 답한다.
    pub(super) fn offline_request(&self, info: &SessionInfo, body: SessionRequest) -> Result<AgentResponse, ErrorBody> {
        let paths = SessionPaths::new(&self.state, &info.id);
        match body {
            SessionRequest::Subscribe { .. } => {
                let replay = std::fs::read(paths.final_replay())
                    .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
                    .unwrap_or_default();
                let end = util::read_json::<ExitFile>(&paths.exit())
                    .map(|x| x.end_offset)
                    .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
                    .unwrap_or(0);
                Ok(AgentResponse::Subscribed {
                    session: info.id.clone(),
                    snapshot: Some(Snapshot {
                        compression: None,
                        session: info.id.clone(),
                        offset: end,
                        cols: info.cols,
                        rows: info.rows,
                        replay: B64(replay),
                        history_more: false,
                    }),
                    offset: end,
                })
            }
            SessionRequest::Unsubscribe => Ok(AgentResponse::Done),
            SessionRequest::Screen => util::read_json::<ScreenText>(&paths.final_screen())
                .map(AgentResponse::Screen)
                .map_err(|source_error| {
                    aaa_common::diagnostic::context(
                        source_error,
                        ErrorBody::new(ErrorCode::SessionExited, "마지막 화면 기록이 없다"),
                    )
                }),
            _ => Err(ErrorBody::new(ErrorCode::SessionExited, "세션이 끝났다")),
        }
    }
}
