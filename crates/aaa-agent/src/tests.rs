use super::*;

/// tab create와 저장된 resume 모두 session host에 넘기는 spec에서 daemon을 막는다.
/// 없는 실행 파일에서 멈추므로 실제 harness·서비스·프로세스 신호는 쓰지 않는다.
#[tokio::test]
async fn codex_spawn_spec_stays_in_the_pane() {
    let title = r#"tui.terminal_title=["run-state","activity","project-name","fast-mode"]"#;
    for (argv, expected) in [
        (vec!["codex"], vec!["codex", "--no-daemon", "-c", title]),
        (
            vec!["C:\\tools\\codex.exe", "resume", "test-id"],
            vec!["C:\\tools\\codex.exe", "--no-daemon", "resume", "test-id", "-c", title],
        ),
        (vec!["codex", "--no-daemon"], vec!["codex", "--no-daemon", "-c", title]),
        (vec!["codex", "--", "--no-daemon"], vec!["codex", "--no-daemon", "-c", title, "--", "--no-daemon"]),
        (vec!["sh", "-c", "codex"], vec!["sh", "-c", "codex"]),
    ] {
        let state = std::env::temp_dir().join(format!("aaa-spawn-{}", util::new_id()));
        let mut a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
        Arc::get_mut(&mut a).unwrap().exe = state.join("missing-session-host");
        let (spec, _) = session(None, &argv, SessionState::Running);
        let error = a.spawn_session(spec.spec).await.unwrap_err();
        assert_eq!(error.to_string(), "session host 실행");
        let dirs: Vec<_> = std::fs::read_dir(state.join("sessions")).unwrap().collect();
        assert_eq!(dirs.len(), 1);
        let file: SpecFile = util::read_json(&dirs[0].as_ref().unwrap().path().join("spec.json")).unwrap();
        assert_eq!(file.spec.argv, expected, "session host에 공유 daemon을 허용하는 argv를 넘겼다");
        assert!(file.spec.env.iter().any(|(k, v)| k == hooks::ENV_TOKEN && !v.is_empty()));
        std::fs::remove_dir_all(state).unwrap();
    }
}

/// 읽기를 멈춘 옛 host의 전송 큐를 그대로 둬도 연결 변경은 그 전송 대기를 끝내야 한다.
/// harness_event는 전송까지 순서 울타리를 잡으므로, 옛 대기가 남으면 다음 Welcome도 진행하지 못한다.
#[tokio::test]
async fn pending_host_send_lets_go_when_the_connection_changes() {
    use std::future::Future;
    use std::task::Poll;
    let state = std::env::temp_dir().join(format!("aach-{}", util::new_id()));
    let a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
    let (old, _old_out) = aaa_common::conn::frame_channel();
    a.set_host(Some(old.clone()));
    for seq in 0..512 {
        old.json(&LinkMsg::Ping { seq }).await.unwrap();
    }
    // agent의 일반 전달도 B64 글자 충돌로 panic하거나 응답을 잃지 않아야 한다.
    use base64::Engine;
    let encoded_secret = aaa_test_support::token("agent-b64");
    aaa_common::redact::remember(&encoded_secret);
    let snapshot = base64::engine::general_purpose::STANDARD.decode(format!("{}AAAA", &encoded_secret[..16])).unwrap();
    let message = LinkMsg::Res {
        id: "snapshot-request".into(),
        result: Outcome::Ok(AgentResponse::Subscribed {
            session: "snapshot-session".into(),
            offset: snapshot.len() as u64,
            snapshot: Some(aaa_proto::Snapshot {
                compression: None,
                session: "snapshot-session".into(),
                offset: snapshot.len() as u64,
                cols: 80,
                rows: 24,
                replay: B64(snapshot),
                history_more: false,
            }),
        }),
    };
    let mut sending = std::pin::pin!(a.send_host(&message));
    std::future::poll_fn(|cx| {
        assert!(sending.as_mut().poll(cx).is_pending(), "가득 찬 옛 전송 큐에서 기다려야 한다");
        Poll::Ready(())
    })
    .await;
    a.set_host(None);
    tokio::time::timeout(Duration::from_secs(3), sending).await.expect("옛 전송 대기가 새 연결을 막았다");
    let (new, mut out) = aaa_common::conn::frame_channel();
    a.set_host(Some(new));
    a.send_host(&message).await;
    let (_, bytes) = out.recv().await.unwrap();
    assert_eq!(serde_json::from_slice::<LinkMsg>(&bytes).unwrap(), message);
    std::fs::remove_dir_all(state).unwrap();
}

#[tokio::test]
async fn compaction_goes_to_an_old_host_as_working() {
    let base = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = base.join(format!("aach-{}", util::new_id()));
    let a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
    let (tx, mut out) = aaa_common::conn::frame_channel();
    a.set_host(Some(tx));
    let ev = aaa_proto::HarnessEvent {
        session: "s".into(),
        harness: "claude".into(),
        kind: "compacting".into(),
        harness_session: None,
        message: None,
        question_kind: None,
        percent: None,
        at_ms: util::now_ms(),
        transcript: None,
        plan: None,
        background: None,
        context: None,
        model_observation: None,
        codex_speed: None,
        provider_turn: None,
        turn_reply: None,
    };
    let mut sent = async |host_compacting: bool| {
        a.host_compacting.store(host_compacting, std::sync::atomic::Ordering::Relaxed);
        a.harness_event(ev.clone()).await;
        let (_, bytes) = out.recv().await.unwrap();
        match serde_json::from_slice(&bytes).unwrap() {
            LinkMsg::Event(AgentEvent::Harness(ev)) => ev.kind,
            other => panic!("{other:?}"),
        }
    };
    // 옛 host(`compacting.v1` 없음)는 모르는 kind를 버리므로 작업 중으로 보낸다
    assert_eq!(sent(false).await, "working");
    assert_eq!(sent(true).await, "compacting");
    // 다시 알릴 사건은 원래 kind다: 새 host에 다시 붙으면 정리 중으로 보인다
    assert!(a.harness_events().iter().any(|e| e.kind == "compacting"));
    std::fs::remove_dir_all(state).unwrap();
}

#[tokio::test]
async fn codex_start_before_pane_registration_waits_for_registration() {
    use std::task::Poll;
    use tokio::io::AsyncWriteExt;
    let base = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() };
    let state = base.join(format!("aach-{}", util::new_id()));
    let a = Agent::new(state.clone(), "m".into(), "mac-a".into(), true, None).unwrap();
    let (mut spec, mut info) = session(Some("codex"), &["codex"], SessionState::Running);
    spec.spec.role = Some(aaa_proto::Role::Worker);
    info.pid = Some(std::process::id());
    let paths = SessionPaths::new(&state, "s");
    util::ensure_private_dir(&paths.dir).unwrap();
    util::write_json(&paths.spec(), &spec).unwrap();
    let (tx, mut out) = aaa_common::conn::frame_channel();
    a.set_host(Some(tx));
    let msg = hooks::HookMsg {
        session: "s".into(),
        token: hooks::hook_token(&spec.token),
        harness: "codex".into(),
        payload: serde_json::json!({"hook_event_name":"SessionStart","source":"startup","session_id":"019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0"}),
    };
    let (mut writer, reader) = tokio::io::duplex(8192);
    writer.write_all(&serde_json::to_vec(&msg).unwrap()).await.unwrap();
    let mut reader: aaa_common::ipc::LocalStream = Box::new(reader);
    let mut pending = std::pin::pin!(a.on_hook(&mut reader, Some(std::process::id())));
    // 입력은 이미 완성됐다. 등록 전 poll에서 거부하면 실제 시작 race다.
    std::future::poll_fn(|cx| {
        assert!(pending.as_mut().poll(cx).is_pending(), "등록 전에 정상 hook을 거부했다");
        Poll::Ready(())
    })
    .await;
    // 실제 attach가 pane을 등록하고 깨운다. 시험이 등록 완료 신호를 대신 공급하지 않는다.
    let listener = aaa_common::ipc::LocalListener::bind(&paths.sock).unwrap();
    let fake = tokio::spawn(async move {
        let (r, w) = tokio::io::split(listener.accept_peer().await.unwrap().0);
        let tx = spawn_writer(w, MAX_FRAME);
        let mut rx = FrameRx::new(r, MAX_FRAME);
        assert!(matches!(rx.control::<SessionMsg>().await.unwrap(), SessionMsg::Hello { .. }));
        tx.json(&SessionMsg::Welcome { proto: SESSION_IPC, info, account_rules: None }).await.unwrap();
        std::future::pending::<()>().await;
        drop((tx, rx, listener));
    });
    a.attach(&paths, &spec).await.unwrap();
    let hooks::Reply::Text(reply) = pending.await.unwrap().unwrap() else { panic!("hook 안내 형식") };
    assert!(reply.contains("report --percent 25"), "{reply}");
    let (_, bytes) = out.recv().await.unwrap();
    let event: LinkMsg = serde_json::from_slice(&bytes).unwrap();
    assert!(matches!(event, LinkMsg::Event(AgentEvent::Harness(ev)) if ev.kind == "session_start"));
    fake.abort();
    std::fs::remove_dir_all(state).unwrap();
}

pub(super) fn session(harness: Option<&str>, argv: &[&str], state: SessionState) -> (SpecFile, SessionInfo) {
    let spec = SpawnSpec {
        name: "s".into(),
        argv: argv.iter().map(|a| a.to_string()).collect(),
        cwd: None,
        env: vec![],
        cols: 80,
        rows: 24,
        role: None,
        harness: harness.map(str::to_string),
        profile: None,
        resolved_profile: None,
    };
    let info = SessionInfo {
        responsibility: None,
        id: "s".into(),
        name: "s".into(),
        argv: spec.argv.clone(),
        cwd: None,
        pid: Some(1),
        // 오래전에 띄운 세션(시작 유예가 끝났다)
        started_at_ms: 1,
        boot_id: "b".into(),
        cols: 80,
        rows: 24,
        state,
        role: None,
        label: None,
        harness: spec.harness.clone(),
    };
    (
        SpecFile {
            id: "s".into(),
            token: aaa_test_support::token("session-hydrate"),
            boot_id: "b".into(),
            created_at_ms: 1,
            spec,
            label: None,
            colors: None,
            account_run: None,
            account_generation: None,
        },
        info,
    )
}

/// 감지 요청: 유예가 끝난 차례 다음의 첫 요청
fn first_request(w: &mut detect::Watch) -> SessionRequest {
    let now = std::time::Instant::now();
    assert!(w.due(now).is_none());
    w.due(w.next_at().unwrap()).unwrap().1
}

#[test]
fn detection_asks_old_session_hosts_only_for_the_screen() {
    let v = |minor| detect_ipc(aaa_proto::Version { major: SESSION_IPC.major, minor });
    let running = SessionState::Running;
    let (spec, info) = session(Some("codex"), &["/opt/bin/codex", "-c", "x"], running.clone());
    // SESSION_IPC 1.0 session host는 `Detect`를 모르고 연결을 끊는다: 보이는 화면만 묻는다
    let mut old = watch_of(&spec, &info, v(0)).unwrap();
    assert_eq!(old.harness(), detect::Harness::Codex);
    assert_eq!(first_request(&mut old), SessionRequest::Screen);
    let mut new = watch_of(&spec, &info, v(aaa_proto::SESSION_IPC_DETECT)).unwrap();
    assert_eq!(first_request(&mut new), SessionRequest::Detect { unchanged_since: None });
    // profile harness가 shell·custom이면 실행 파일 이름으로, 그것도 모르면 보지 않는다
    let (spec, info) = session(Some("custom"), &["/x/claude", "--settings", "f"], running.clone());
    assert_eq!(watch_of(&spec, &info, v(1)).map(|w| w.harness()), Some(detect::Harness::Claude));
    let (spec, info) = session(None, &["/bin/sh"], running);
    assert!(watch_of(&spec, &info, v(1)).is_none());
    let (spec, info) = session(Some("claude"), &["claude"], SessionState::Exited { code: Some(0) });
    assert!(watch_of(&spec, &info, v(1)).is_none(), "끝난 세션은 보지 않는다");
}

/// 다시 붙기 간격은 0.5초에서 두 배씩 늘고 30초에서 멈춘다(넘치지 않는다, herdr `retry_backoff_is_bounded`)
#[test]
fn reattach_delay_is_bounded() {
    assert_eq!(reattach_delay(1), Duration::from_millis(500));
    assert_eq!(reattach_delay(2), Duration::from_secs(1));
    assert_eq!(reattach_delay(100), Duration::from_secs(30));
}

/// 가짜 session host가 본 것(연결 번호)
#[derive(Debug)]
enum Seen {
    Hello(u32),
    Req(u32, SessionRequest),
}

/// 가짜 session host가 첫 연결을 끝내는 길. 모두 frame 오류가 아니다.
#[derive(Debug, Clone, Copy, PartialEq)]
enum FirstEnd {
    /// 연결만 닫는다(옛 session host가 모르는 요청을 받았을 때처럼)
    Eof,
    /// agent가 모르는 메시지를 보낸다(새 session host)
    Unknown,
    /// 연결을 닫고 listen도 그만둔다(session host가 사라졌다)
    Gone,
    /// 화면 감지 요청을 받고 답하지 않은 채 닫는다(harness 세션)
    AfterDetect,
}

/// session host 하나를 흉내 낸다: 연결마다 token을 보고 Welcome(실행 중)을 보낸다. 첫 연결은 `first`로 끝내고, 그 뒤 연결은
/// 요청마다 Done으로 답한다(Subscribe는 snapshot 없이 offset 0). `open`이 거짓인 동안은 두째 연결부터 인사만 받고 닫는다.
fn fake_session_host(
    listener: aaa_common::ipc::LocalListener,
    info: SessionInfo,
    token: String,
    first: FirstEnd,
    open: Arc<std::sync::atomic::AtomicBool>,
) -> mpsc::UnboundedReceiver<Seen> {
    let (seen, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut listener = Some(listener);
        for n in 1u32.. {
            let Some(l) = listener.as_ref() else { break };
            let Ok(stream) = l.accept().await else { break };
            let (r, w) = tokio::io::split(stream);
            let tx = spawn_writer(w, MAX_FRAME);
            let mut rx = FrameRx::new(r, MAX_FRAME);
            match rx.control::<SessionMsg>().await {
                Ok(SessionMsg::Hello { token: t, .. }) if t == token => {}
                other => panic!("인사가 아니다: {other:?}"),
            }
            let _ = seen.send(Seen::Hello(n));
            if n > 1 && !open.load(std::sync::atomic::Ordering::SeqCst) {
                continue;
            }
            tx.json(&SessionMsg::Welcome { proto: SESSION_IPC, info: info.clone(), account_rules: None })
                .await
                .unwrap();
            if n == 1 {
                match first {
                    FirstEnd::Eof => {}
                    FirstEnd::Unknown => tx.control(br#"{"t":"later_kind"}"#.to_vec()).await.unwrap(),
                    FirstEnd::Gone => listener = None,
                    FirstEnd::AfterDetect => {
                        while let Ok(Some(Incoming::Control(b))) = rx.next().await {
                            if let Ok(SessionMsg::Req { body, .. }) = serde_json::from_slice::<SessionMsg>(&b) {
                                let _ = seen.send(Seen::Req(1, body));
                                break;
                            }
                        }
                    }
                }
                // 쓰기 handle을 놓으면 보낸 것을 다 쓴 뒤 닫힌다(읽는 쪽은 여기서 놓는다)
                continue;
            }
            let seen = seen.clone();
            tokio::spawn(async move {
                while let Ok(Some(Incoming::Control(b))) = rx.next().await {
                    let Ok(SessionMsg::Req { id, body }) = serde_json::from_slice::<SessionMsg>(&b) else { break };
                    let result = match &body {
                        SessionRequest::Subscribe { .. } => {
                            AgentResponse::Subscribed { session: "s".into(), snapshot: None, offset: 0 }
                        }
                        _ => AgentResponse::Done,
                    };
                    let _ = seen.send(Seen::Req(n, body));
                    if tx.json(&SessionMsg::Res { id, result: Outcome::Ok(result) }).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    rx
}

/// host 쪽 줄에서 다음 link 메시지
async fn next_link(out: &mut aaa_common::conn::FrameOut) -> LinkMsg {
    let (_, b) = out.recv().await.expect("host 줄이 닫혔다");
    serde_json::from_slice(&b).unwrap()
}

/// 실험: 가짜 session host에 붙은 agent와 host 쪽 줄
struct Rig {
    agent: Arc<Agent>,
    state: PathBuf,
    seen: mpsc::UnboundedReceiver<Seen>,
    /// 가짜가 마지막으로 받은 인사의 연결 번호
    hello: u32,
    out: aaa_common::conn::FrameOut,
}

impl Rig {
    /// 가짜가 `first`로 첫 연결을 끝낸 뒤 agent의 다음 연결(Hello 2)과 host로 간 「세션이 끝났다」 중 먼저 온 것
    async fn after_first_connection_ends(
        first: FirstEnd,
        open: bool,
    ) -> (Rig, Arc<std::sync::atomic::AtomicBool>, Result<(), SessionState>) {
        // macOS UDS 경로 한도(104바이트) 때문에 짧은 /tmp에 둔다(m13 실험실과 같다)
        let state = if cfg!(unix) { PathBuf::from("/tmp") } else { std::env::temp_dir() }
            .join(format!("aar-{}-{first:?}-{open}", std::process::id()));
        let _ = std::fs::remove_dir_all(&state);
        let agent = Agent::new(state.clone(), "m".into(), "m".into(), false, None).unwrap();
        let (spec, mut info) = match first {
            // 화면으로 상태를 보는 세션(오래전에 띄워 시작 유예가 끝났다)
            FirstEnd::AfterDetect => session(Some("claude"), &["claude"], SessionState::Running),
            _ => session(None, &["/bin/sh"], SessionState::Running),
        };
        // 앞 프로세스 감지(셸 pid)는 보지 않는다
        info.pid = None;
        let spec = SpecFile { boot_id: agent.boot_id.clone(), ..spec };
        let paths = SessionPaths::new(&state, &spec.id);
        util::ensure_private_dir(&paths.dir).unwrap();
        util::write_json(&paths.spec(), &spec).unwrap();
        // session host는 이 시험 프로세스다(살아 있다)
        let meta = MetaFile { host_pid: std::process::id(), child_pid: None, started_at_ms: 1 };
        util::write_json(&paths.meta(), &meta).unwrap();
        let listener = aaa_common::ipc::LocalListener::bind(&paths.sock).unwrap();
        let gate = Arc::new(std::sync::atomic::AtomicBool::new(open));
        let mut seen = fake_session_host(listener, info, spec.token.clone(), first, gate.clone());
        let (host, mut out) = aaa_common::conn::frame_channel();
        agent.set_host(Some(host));
        agent.recover().await.unwrap();
        assert!(matches!(seen.recv().await, Some(Seen::Hello(1))));
        // 안전 상한(오지 않을 때 멈춘다). 다시 붙기 첫 간격은 0.5초다
        let outcome = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                // 가짜는 Welcome보다 먼저 Hello를 알린다: 먼저 보면 그 뒤에 오는 host 알림(Resync)을 여기서 버리지 않는다
                tokio::select! {
                    biased;
                    Some(s) = seen.recv() => if matches!(s, Seen::Hello(2)) { return Ok(()) },
                    m = next_link(&mut out) => if let LinkMsg::Event(AgentEvent::SessionUpdated(i)) = m
                        && !i.state.is_running()
                    {
                        return Err(i.state);
                    },
                }
            }
        })
        .await
        .expect("다시 붙지도, 놓지도 않았다");
        (Rig { agent, state, seen, hello: 2, out }, gate, outcome)
    }

    /// host로 간 다음 응답(id·결과)
    async fn next_res(&mut self) -> (String, Outcome<AgentResponse>) {
        loop {
            if let LinkMsg::Res { id, result } = next_link(&mut self.out).await {
                return (id, result);
            }
        }
    }

    async fn input(&self, id: &str) {
        let body = AgentRequest::Input { session: "s".into(), data: B64(b"x".to_vec()) };
        self.agent.clone().handle(id.into(), body).await;
    }

    /// 가짜 session host가 받은 다음 요청(연결 번호·요청). 인사는 건너뛴다
    async fn next_req(&mut self) -> (u32, SessionRequest) {
        // 안전 상한(오지 않을 때 멈춘다). 다시 붙기 간격은 길어야 몇 초다
        let next = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match self.seen.recv().await {
                    Some(Seen::Hello(n)) => self.hello = n,
                    Some(Seen::Req(n, body)) => return (n, body),
                    None => panic!("가짜 session host가 끝났다"),
                }
            }
        });
        next.await.expect("session host가 요청을 받지 않았다")
    }

    /// 다시 붙었다: 세션은 실행 중이고, host는 다시 구독하라는 알림을 받고(aaa-host `on_resync`가 마지막 offset부터 다시
    /// 구독한다), 입력은 새 연결로 session host에 간다
    async fn works_again(mut self) {
        assert!(self.agent.sessions().iter().all(|s| s.state.is_running()), "{:?}", self.agent.sessions());
        let resync = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let LinkMsg::Event(AgentEvent::Resync { session }) = next_link(&mut self.out).await {
                    return session;
                }
            }
        });
        assert_eq!(resync.await.expect("다시 구독 알림이 없다"), "s");
        self.input("q1").await;
        let to = loop {
            match self.seen.recv().await {
                Some(Seen::Hello(n)) => self.hello = n,
                Some(Seen::Req(n, SessionRequest::Input { .. })) => break n,
                other => panic!("입력 대신 {other:?}"),
            }
        };
        assert_eq!(to, self.hello, "입력이 마지막 연결로 가지 않았다");
        assert_eq!(self.next_res().await, ("q1".into(), Outcome::Ok(AgentResponse::Done)));
        std::fs::remove_dir_all(&self.state).unwrap();
    }
}

/// pump가 frame 오류 밖의 까닭으로 끝나도 session host가 살아 있으면 다시 붙는다(herdr Local endpoint 재연결, plan §8.186).
/// session host가 연결만 닫았다(옛 session host가 모르는 요청을 받았을 때, `detect_ipc` 주석)
#[tokio::test]
async fn pump_reattaches_after_the_session_host_closes_the_connection() {
    let (rig, _, outcome) = Rig::after_first_connection_ends(FirstEnd::Eof, true).await;
    assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
    rig.works_again().await;
}

/// session host가 agent가 모르는 메시지를 보냈다(pump의 「session 메시지 해석 실패」)
#[tokio::test]
async fn pump_reattaches_after_an_unknown_session_message() {
    let (rig, _, outcome) = Rig::after_first_connection_ends(FirstEnd::Unknown, true).await;
    assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
    rig.works_again().await;
}

/// 다시 붙는 동안 온 요청에는 「세션에 다시 붙는 중이다」로 답한다(세션은 실행 중 그대로). 인사를 받지 못한 연결은 다음
/// 간격에 다시 해 보고, 붙은 뒤의 요청은 새 연결로 간다.
#[tokio::test]
async fn requests_while_reattaching_are_told_so() {
    let (mut rig, gate, outcome) = Rig::after_first_connection_ends(FirstEnd::Eof, false).await;
    assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
    // 가짜가 인사를 받지 않는 동안은 붙을 수 없다: 이 요청은 다시 붙는 중에 온다
    rig.input("w1").await;
    let (id, result) = rig.next_res().await;
    assert_eq!(id, "w1");
    let Outcome::Err(e) = result else { panic!("다시 붙는 중인데 답했다: {result:?}") };
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::Unavailable, "세션에 다시 붙는 중이다"));
    assert!(rig.agent.sessions().iter().all(|s| s.state.is_running()), "{:?}", rig.agent.sessions());
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    rig.works_again().await;
}

/// 끊긴 연결로 보낸 감지 요청에는 답이 오지 않는다: 다시 붙은 뒤 새 연결로 다시 묻는다(감지가 멈추지 않는다)
#[tokio::test]
async fn detection_asks_again_after_reattach() {
    let (mut rig, _, outcome) = Rig::after_first_connection_ends(FirstEnd::AfterDetect, true).await;
    assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
    let again = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rig.seen.recv().await {
                Some(Seen::Req(n, SessionRequest::Detect { .. })) if n > 1 => return n,
                Some(_) => {}
                None => panic!("가짜 session host가 끝났다"),
            }
        }
    });
    assert!(again.await.is_ok(), "다시 붙은 뒤 화면 감지를 다시 묻지 않는다");
    std::fs::remove_dir_all(&rig.state).unwrap();
}

/// 화면 색(plan §8.187): 다시 붙는 동안 받은 색은 붙자마자 그 session host에 가고, 붙어 있는 동안 받은 색은 바로 간다.
/// 둘 다 agent가 답하기 전에 pump에 넘겼다
#[tokio::test]
async fn colors_reach_the_session_host_after_and_while_attached() {
    let dark = aaa_proto::TermColors { fg: [0xd8, 0xd0, 0xc3], bg: [0x18, 0x16, 0x13] };
    let light = aaa_proto::TermColors { fg: [0x2a, 0x25, 0x1f], bg: [0xfa, 0xf8, 0xf3] };
    let (mut rig, gate, outcome) = Rig::after_first_connection_ends(FirstEnd::Unknown, false).await;
    assert_eq!(outcome, Ok(()), "살아 있는 session host에 다시 붙지 않고 놓았다");
    rig.agent.clone().handle("c1".into(), AgentRequest::TermColors(dark)).await;
    assert_eq!(rig.next_res().await, ("c1".into(), Outcome::Ok(AgentResponse::Done)));
    gate.store(true, std::sync::atomic::Ordering::SeqCst);
    let (n, body) = rig.next_req().await;
    assert_eq!(body, SessionRequest::Colors(dark), "다시 붙은 session host에 받아 둔 색을 먼저 보내지 않았다");
    rig.agent.clone().handle("c2".into(), AgentRequest::TermColors(light)).await;
    assert_eq!(rig.next_req().await, (n, SessionRequest::Colors(light)));
    std::fs::remove_dir_all(&rig.state).unwrap();
}

/// session host가 사라졌으면(socket이 없다) 다시 붙으려다 놓는다: pump가 Lost로 끝나고 세션도 Lost다
#[tokio::test]
async fn pump_lets_go_once_the_session_host_is_gone() {
    let (rig, _, outcome) = Rig::after_first_connection_ends(FirstEnd::Gone, true).await;
    assert_eq!(outcome, Err(SessionState::Lost));
    assert!(rig.agent.sessions().iter().all(|s| s.state == SessionState::Lost), "{:?}", rig.agent.sessions());
    std::fs::remove_dir_all(&rig.state).unwrap();
}
