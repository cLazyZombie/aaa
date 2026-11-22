//! 상태 사건 WebSocket(`/events`)과 쓰는 화면 lease.

use super::*;

pub(super) async fn events_ws(State(host): State<Arc<Host>>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    let Some((dev, role)) = device(&host, &headers) else { return unauthorized() };
    ws.on_upgrade(move |socket| events_loop(host, dev, role, socket))
}

/// 머신·pane 목록(바뀔 때마다 전체)과 확인창 질문·thread 사건. 질문·thread는 연결한 뒤 목록을 한 번 읽고 이 사건으로 따라간다.
pub(super) async fn events_loop<S>(host: Arc<Host>, dev: String, role: DeviceRole, mut socket: S)
where
    S: futures_util::Stream<Item = Result<Message, axum::Error>>
        + futures_util::Sink<Message, Error = axum::Error>
        + Unpin,
{
    let mut changed = host.changed.subscribe();
    let mut questions = host.question_events.subscribe();
    let mut threads = host.thread_events.subscribe();
    let mut notices = host.notice_events.subscribe();
    let mut tasks = host.task_events.subscribe();
    let mut preferences = host.preferences.subscribe();
    let mut reports = host.reports_changed.subscribe();
    let mut workspace_metrics = host.workspace_metrics_changed.subscribe();
    let mut asks = host.asks_changed.subscribe();
    let mut inbox = host.inbox_changed.subscribe();
    let mut accounts = host.accounts_changed.subscribe();
    let mut enroll = host.enroll_changed.subscribe();
    let mut rollouts = host.rollout_changed.subscribe();
    let mut revoked = host.device_revoked.subscribe();
    let mut chat_notes = host.chat_notes();
    // 이 연결이 대화를 청한 pane(차이 목록 C3). 연결이 끝나면 그만둔다
    let mut chats = ChatInterest { host: host.clone(), panes: HashSet::new() };
    let mut presence = Presence { conn: host.conn_open(&dev), host: host.clone(), dev: dev.clone(), active: false };
    let conn = presence.conn;
    let mut send_state = true;
    // 공유 설정 문서는 연결을 연다: 다시 붙은 화면(또는 읽기가 경주에 진 화면)도 마지막 문서를 갖는다(herdr-web-ui machine-api.ts)
    let mut send_preferences = true;
    // 보고서 안 본 수(활동 막대 배지, plan §8.256): 열 때와 목록이 바뀔 때
    let mut send_reports = true;
    // 「답할 것」(plan §8.345): 열 때와 확인 필요 post·답한 것이 바뀔 때
    let mut send_asks = true;
    let mut send_inbox = true;
    let mut send_accounts = true;
    let mut send_enroll = true;
    let mut send_rollout = true;
    let mut send_badge = true;
    let mut last_badge = None;
    let close_revoked = |mut socket: S| async move {
        let frame = CloseFrame { code: DEVICE_REVOKED_CLOSE_CODE, reason: DEVICE_REVOKED_REASON.into() };
        aaa_common::diagnostic::ignored(
            socket.send(Message::Close(Some(frame))).await,
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
    };
    // 듣기 시작한 뒤에 다시 본다(pane_session과 같다)
    if host.is_revoked(&dev) {
        return close_revoked(socket).await;
    }
    // 살아 있는지: 브라우저는 WebSocket ping에 저절로 pong한다. 휴대폰이 잠들거나 망이 바뀌어 반쯤 열린 연결은 보내기가
    // 실패하지 않고 몇 분씩 남는다. 그동안 이 연결의 「쓰는 화면」이 남아 어느 기기에도 push가 가지 않고, 그 화면은 사건을
    // 받지 못한다(10-18 iPhone 실기). 아무것도 받지 못한 채 EVENTS_DEAD_AFTER가 지나면 끊는다(herdr-web-ui는 Bun
    // WebSocket의 ping·idleTimeout과 웹 ws.ts 「Liveness」 probe가 같은 일을 한다).
    let (every, dead_after) = events_liveness();
    let mut ping = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    let mut heard = tokio::time::Instant::now();
    // 「쓰는 화면」 lease: 웹은 쓰는 동안 10초마다 presence를 다시 알린다. 이 연결에서 앱 글(Text)이 lease 넘게 없으면 페이지가
    // 멈춘 것이라(iOS가 잠재운 앱) 쓰는 화면에서 뺀다. 연결은 두고, 다음 presence가 오면 다시 센다. WebSocket pong은
    // 브라우저 네트워크 층이 답해 멈춘 페이지에서도 오므로 lease를 늘리지 않는다(10-18 iPhone 실기, plan §8.133)
    let lease = presence_lease();
    let mut app_heard = tokio::time::Instant::now();
    // 이 host가 주는 웹 빌드: 연결마다 알린다. 다시 보이거나(10초 넘게 가려지면 다시 붙는다) host가 다시 뜨면 새 연결이라 웹이
    // 제 빌드와 견준다(참고는 보이는 동안 상태를 묻는다, plan §8.166)
    if let Some(build) = web_build() {
        let text = serde_json::to_string(&FrontEvent::Web { build }).unwrap();
        if socket
            .send(Message::Text(text.into()))
            .await
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
        {
            return;
        }
    }
    loop {
        if send_badge || send_state || send_reports {
            send_badge = false;
            if let Ok(count) = host.app_badge_count()
                && last_badge != Some(count)
            {
                last_badge = Some(count);
                let text = serde_json::to_string(&FrontEvent::AppBadge { count }).unwrap();
                if socket
                    .send(Message::Text(text.into()))
                    .await
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_err()
                {
                    break;
                }
            }
        }
        if send_preferences {
            send_preferences = false;
            let doc = preferences.borrow_and_update().clone();
            let text = serde_json::to_string(&FrontEvent::Preferences(doc)).unwrap();
            if socket
                .send(Message::Text(text.into()))
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                break;
            }
        }
        if send_reports {
            send_reports = false;
            reports.borrow_and_update();
            let unseen = host.db.library_unseen_count().unwrap_or(0);
            let text = serde_json::to_string(&FrontEvent::Reports { unseen }).unwrap();
            if socket
                .send(Message::Text(text.into()))
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                break;
            }
        }
        if send_asks {
            send_asks = false;
            asks.borrow_and_update();
            let text = serde_json::to_string(&FrontEvent::Asks { requests: host.user_request_count() }).unwrap();
            if socket
                .send(Message::Text(text.into()))
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                break;
            }
        }
        if send_inbox {
            send_inbox = false;
            inbox.borrow_and_update();
            let text = serde_json::to_string(&FrontEvent::Inbox).unwrap();
            if socket
                .send(Message::Text(text.into()))
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                break;
            }
        }
        if send_accounts {
            send_accounts = false;
            accounts.borrow_and_update();
            let text = serde_json::to_string(&FrontEvent::Accounts).unwrap();
            if socket
                .send(Message::Text(text.into()))
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                break;
            }
        }
        if send_enroll {
            send_enroll = false;
            enroll.borrow_and_update();
            let text = serde_json::to_string(&FrontEvent::Enroll).unwrap();
            if socket
                .send(Message::Text(text.into()))
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                break;
            }
        }
        if send_rollout {
            send_rollout = false;
            rollouts.borrow_and_update();
            let result = match host.rollout_card() {
                Ok(card) => Outcome::Ok(card),
                Err(e) => Outcome::Err(e),
            };
            let text = serde_json::to_string(&FrontEvent::Rollout { result }).unwrap();
            if socket
                .send(Message::Text(text.into()))
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                break;
            }
        }
        if send_state {
            let ev = FrontEvent::State { machines: host.machine_views(), panes: host.pane_views(role) };
            if socket
                .send(Message::Text(serde_json::to_string(&ev).unwrap().into()))
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
            {
                break;
            }
        }
        tokio::select! {
            biased;
            r = revoked.recv() => {
                send_state = false;
                let mine = match r {
                    Ok(gone) => gone == dev,
                    Err(RecvError::Lagged(_)) => host.is_revoked(&dev),
                    Err(RecvError::Closed) => false,
                };
                if mine {
                    return close_revoked(socket).await;
                }
            }
            // 밀린 알림·만료 tick보다 준비된 수신을 먼저 읽는다. 부하 뒤 pong이 이미 와 있어도
            // timer부터 고르면 살아 있는 연결을 무응답으로 끊고 목록을 다시 읽게 된다.
            m = socket.next() => {
                send_state = false;
                heard = tokio::time::Instant::now();
                match m {
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(Message::Pong(_))) => host.conn_pong(conn),
                    Some(Ok(Message::Text(t))) => {
                        app_heard = tokio::time::Instant::now();
                        host.conn_text(conn);
                        match serde_json::from_str::<EventsClientMsg>(&t) {
                            Ok(EventsClientMsg::Presence { active, why, visible, focus, build }) => {
                                host.conn_presence(conn, presence_why(active, why, visible, focus), build);
                                presence.set(active)
                            }
                            Ok(EventsClientMsg::Touch) => host.touch_device(&dev, "touch"),
                            Ok(EventsClientMsg::Alert { project, target, kind, outcome }) => {
                                host.alert_outcome(&dev, conn, project.as_deref(), &target, &kind, &outcome)
                            }
                            Ok(EventsClientMsg::Ping { id }) => {
                                let text = serde_json::to_string(&FrontEvent::Pong { id }).unwrap();
                                if socket.send(Message::Text(text.into())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                                    break;
                                }
                            }
                            // 보기 범위(§8.25)는 대화 읽기(`pane_chat`)와 같다
                            Ok(EventsClientMsg::ChatWatch { pane, enabled }) => {
                                if !enabled {
                                    if chats.panes.remove(&pane) {
                                        host.chat_watch(&pane, false);
                                    }
                                } else if host.pane_view_of(&pane, role).is_some() {
                                    if chats.panes.insert(pane.clone()) {
                                        host.chat_watch(&pane, true);
                                    }
                                    let ev = FrontEvent::ChatWatching { live: host.chat_live(&pane), pane };
                                    if socket.send(Message::Text(serde_json::to_string(&ev).unwrap().into())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                                        break;
                                    }
                                }
                            }
                            Err(source_error) => { aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");}
                        }
                    }
                    _ => {}
                }
            }
            r = changed.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break }
                send_state = true;
                send_rollout = true;
            }
            q = questions.recv() => {
                send_state = false;
                send_badge = true;
                match q {
                    Ok(q) => {
                        let text = serde_json::to_string(&FrontEvent::Question(q)).unwrap();
                        if socket.send(Message::Text(text.into())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                            break;
                        }
                    }
                    // 놓쳤으면 끊는다. 웹은 다시 붙으며 질문 목록을 새로 읽는다.
                    Err(source_error) => { aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다"); break },
                }
            }
            t = threads.recv() => {
                send_state = false;
                send_badge = true;
                match t {
                    Ok(mut t) => {
                        attachment_thread(&host, &dev, &mut t);
                        let text = serde_json::to_string(&FrontEvent::Thread(Box::new(t))).unwrap();
                        if socket.send(Message::Text(text.into())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                            break;
                        }
                    }
                    Err(source_error) => { aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다"); break },
                }
            }
            // 앱 안 알림(notice.rs). 기록하지 않으므로 놓친 것은 버린다(끊지 않는다)
            n = notices.recv() => {
                send_state = false;
                match n {
                    Ok(n) => {
                        let text = serde_json::to_string(&FrontEvent::Notice(n)).unwrap();
                        if socket.send(Message::Text(text.into())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
            // TASKS.md가 바뀌었다(할 일 화면이 다시 읽는다). 놓쳤으면 끊는다: 웹은 다시 붙으며 새로 읽는다
            t = tasks.recv() => {
                send_state = false;
                match t {
                    Ok((project, sha256)) => {
                        let text = serde_json::to_string(&FrontEvent::Tasks { project, sha256 }).unwrap();
                        if socket.send(Message::Text(text.into())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) | Err(RecvError::Closed) => break,
                }
            }
            r = preferences.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break }
                send_state = false;
                send_preferences = true;
            }
            r = rollouts.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break; }
                send_rollout = true;
            }
            r = workspace_metrics.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break; }
                send_state = false;
                let text = serde_json::to_string(&FrontEvent::WorkspaceMetrics).unwrap();
                if socket.send(Message::Text(text.into())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break; }
            }
            r = reports.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break }
                send_state = false;
                send_reports = true;
            }
            r = asks.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break }
                send_state = false;
                send_asks = true;
            }
            r = inbox.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break }
                send_state = false;
                send_inbox = true;
            }
            r = accounts.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break }
                send_state = false;
                send_accounts = true;
            }
            r = enroll.changed() => {
                if r.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() { break }
                send_state = false;
                send_enroll = true;
            }
            n = chat_notes.recv() => {
                send_state = false;
                let evs = match n {
                    Ok(ChatNote::Changed { pane, signature }) if chats.panes.contains(&pane) => {
                        vec![FrontEvent::ChatChanged { pane, signature }]
                    }
                    Ok(ChatNote::Live { pane, live }) if chats.panes.contains(&pane) => {
                        vec![FrontEvent::ChatWatching { pane, live }]
                    }
                    Ok(_) | Err(RecvError::Closed) => vec![],
                    // 놓친 바뀜이 있을 수 있다: 청한 pane을 모두 다시 읽게 한다
                    Err(RecvError::Lagged(_)) => chats
                        .panes
                        .iter()
                        .flat_map(|p| {
                            [
                                FrontEvent::ChatWatching { pane: p.clone(), live: host.chat_live(p) },
                                FrontEvent::ChatChanged { pane: p.clone(), signature: "lagged".into() },
                            ]
                        })
                        .collect(),
                };
                let mut gone = false;
                for ev in evs {
                    if socket.send(Message::Text(serde_json::to_string(&ev).unwrap().into())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                        gone = true;
                        break;
                    }
                }
                if gone {
                    break;
                }
            }
            _ = ping.tick() => {
                send_state = false;
                if heard.elapsed() >= dead_after {
                    tracing::info!("상태 사건 연결이 {}초 동안 답하지 않아 끊는다", dead_after.as_secs());
                    break;
                }
                if socket.send(Message::Ping(Default::default())).await.as_ref().inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")).is_err() {
                    break;
                }
            }
            _ = tokio::time::sleep_until(app_heard + lease), if presence.active => {
                send_state = false;
                tracing::info!("쓰는 화면 lease 끝: 앱 글이 {}초 동안 없다", lease.as_secs());
                host.conn_presence(conn, format!("lease 끝(앱 글 {}초 없음)", lease.as_secs()), None);
                presence.set(false);
            }
        }
    }
}

/// 상태 사건 연결이 대화를 청한 pane들. 연결이 어떻게 끝나든 그만둔다
pub(super) struct ChatInterest {
    pub(super) host: Arc<Host>,
    pub(super) panes: HashSet<String>,
}

impl Drop for ChatInterest {
    fn drop(&mut self) {
        for pane in self.panes.drain() {
            self.host.chat_watch(&pane, false);
        }
    }
}

/// 옛 TUI 전용 endpoint. 기존 기기 인증은 유지하고 명시적인 삭제 안내만 보낸다.
pub(super) async fn front_ws(State(host): State<Arc<Host>>, headers: HeaderMap, ws: WebSocketUpgrade) -> Response {
    if device(&host, &headers).is_none() {
        return unauthorized();
    }
    ws.max_message_size(aaa_proto::MAX_FRAME).on_upgrade(|mut socket| async move {
        let message =
            aaa_proto::FrontMsg::Reject(ErrorBody::new(ErrorCode::Unavailable, aaa_common::front::TUI_REMOVED));
        if let Ok(text) = serde_json::to_string(&message) {
            aaa_common::diagnostic::ignored(
                socket.send(Message::Text(text.into())).await,
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
        aaa_common::diagnostic::ignored(socket.close().await, "정리 실패가 본 작업의 결과를 덮지 않게 한다");
    })
}

/// 상태 사건 연결의 WebSocket ping 간격과, 아무것도 받지 못하면 끊는 시간(15초·45초: ping 두 번을 놓치고도 남는다).
/// `AAA_TEST_EVENTS_PING_MS`는 시험에서만 줄인다(끊는 시간은 그 세 배).
pub(super) fn events_liveness() -> (std::time::Duration, std::time::Duration) {
    let ms = std::env::var("AAA_TEST_EVENTS_PING_MS")
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|v| {
            v.parse::<u64>()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .unwrap_or(15_000);
    (std::time::Duration::from_millis(ms), std::time::Duration::from_millis(ms * 3))
}

/// 「쓰는 화면」 lease(25초: 웹의 10초 presence를 두 번 놓치고도 남는다). `AAA_TEST_PRESENCE_LEASE_MS`는 시험에서만 줄인다
/// 포함한 웹의 빌드 이름: index.html이 여는 주 묶음(`/assets/index-XXXX.js`)의 파일 이름. 웹 `api.ts` `BUILD`와 같은 꼴이다.
/// 웹을 빌드하지 않았으면 없다. 시험은 `AAA_TEST_WEB_BUILD`로 다른 빌드를 알리게 한다(새 웹이 나온 host를 흉내 낸다)
pub(super) fn web_build() -> Option<String> {
    if let Ok(v) = std::env::var("AAA_TEST_WEB_BUILD") {
        return Some(v);
    }
    let html = assets::WEB_ASSETS
        .iter()
        .find(|asset| asset.name == "index.html")?
        .identity()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    main_bundle(&String::from_utf8_lossy(&html))
}

/// index.html의 `<script … src="/assets/NAME.js">`에서 NAME
pub(super) fn main_bundle(html: &str) -> Option<String> {
    html.split("<script").skip(1).find_map(|tag| {
        let tag = &tag[..tag.find('>')?];
        let src = tag.split("src=\"").nth(1)?.split('"').next()?;
        let name = src.strip_prefix("/assets/")?.strip_suffix(".js")?;
        (!name.is_empty() && !name.contains('/')).then(|| name.to_string())
    })
}

pub(super) fn presence_lease() -> std::time::Duration {
    let ms = std::env::var("AAA_TEST_PRESENCE_LEASE_MS")
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|v| {
            v.parse::<u64>()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .unwrap_or(25_000);
    std::time::Duration::from_millis(ms)
}

/// presence 판정 근거 한 줄: 웹이 보낸 까닭과 화면 값(옛 웹은 없다)
pub(super) fn presence_why(active: bool, why: Option<String>, visible: Option<bool>, focus: Option<bool>) -> String {
    let flag =
        |v: Option<bool>, yes: &str, no: &str| v.map_or("?".to_string(), |b| if b { yes } else { no }.to_string());
    let why: String =
        why.unwrap_or_else(|| "?".into()).chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(24).collect();
    format!(
        "{} {why} {}·{}",
        if active { "true" } else { "false" },
        flag(visible, "보임", "가려짐"),
        flag(focus, "focus", "focus 없음")
    )
}

/// 한 events 연결이 「쓰는 화면」인지. 바뀔 때만 host 수를 고치고, 연결이 어떻게 끝나든 drop에서 뺀다.
pub(super) struct Presence {
    /// 판정 근거의 연결 번호(seen.rs `conn_open`)
    pub(super) conn: u64,
    pub(super) host: Arc<Host>,
    /// 이 연결의 기기(그 기기가 연 pane 터미널을 「보는 화면」으로 센다, seen.rs)
    pub(super) dev: String,
    pub(super) active: bool,
}

impl Presence {
    pub(super) fn set(&mut self, active: bool) {
        use std::sync::atomic::Ordering::SeqCst;
        if active != self.active {
            self.active = active;
            if active {
                self.host.in_use.fetch_add(1, SeqCst);
            } else {
                self.host.in_use.fetch_sub(1, SeqCst);
            }
            self.host.view_presence(self.conn, &self.dev, active);
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.set(false);
        self.host.conn_close(self.conn);
    }
}
