//! agent가 붙고 끊길 때와 세션 사건·출력 조각을 host 상태에 반영한다.

use super::*;

impl Host {
    // ───────────── agent 연결 ─────────────

    pub fn next_gen(&self) -> u64 {
        let mut i = self.inner.lock().unwrap();
        i.generation += 1;
        i.generation
    }

    /// hello를 받아들였다: 머신·pane 목록을 갱신하고 모든 세션을 구독한다.
    pub fn agent_connected(self: &Arc<Self>, hello: &aaa_proto::AgentHello, local: bool, conn: AgentConn) {
        let mut hello = hello.clone();
        if let Ok(Some(machine)) = self.db.machine_by_id(&hello.machine_id) {
            hello.machine_name = machine.name;
        }
        let hello = &hello;
        self.sync_machine_label(&hello.machine_id, &conn, &hello.features);
        aaa_common::diagnostic::ignored(self.db.touch_machine(hello), "보조 작업 실패 뒤 기존 처리를 계속한다");
        let generation = conn.generation;
        // 연결을 공개하기 전에 상태 복원 울타리를 친다. host 재시작 뒤에는 실행 중인 pane도 아직 agent=None이다.
        // Hello의 모든 harness 상태를 재생하기 전 ticker가 보면 쉼(no agent)라는 거짓 전이가 생긴다.
        // restore_after_reboot이 재부팅 복원까지 이어 맡고, 되살릴 pane이 없으면 바로 푼다.
        self.restoring.lock().unwrap().insert(hello.machine_id.clone());
        let subscribe: Vec<(String, Option<u64>)>;
        // agent가 내려간 사이 스스로 끝난 셸 pane(host는 그 전에 실행 중을 봤다)
        let mut exited = Vec::new();
        {
            let mut i = self.inner.lock().unwrap();
            if let Some(old) = i.machines.get(&hello.machine_id).and_then(|m| m.conn.clone()) {
                old.fail_all();
            }
            // 지금 화면 색부터 차례로 보낸다(inner lock 안이라 바뀐 색이 이 앞에 끼지 않는다)
            let current = i.colors.current;
            let pushes = hello
                .features
                .iter()
                .any(|f| f == "term_colors.v1")
                .then(|| colors::spawn_pushes(conn.clone(), current));
            let m = i.machines.entry(hello.machine_id.clone()).or_insert(MachineRt {
                name: hello.machine_name.clone(),
                local,
                os: None,
                boot_id: None,
                dev_mode: false,
                conn: None,
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: vec![],
                identity: None,
                agent_pid: None,
                colors: None,
                build: MachineBuild::default(),
            });
            m.colors = pushes;
            m.build = MachineBuild::of(hello);
            m.os = Some(hello.os.clone());
            m.boot_id = Some(hello.boot_id.clone());
            m.dev_mode = hello.dev_mode;
            m.features = hello.features.clone();
            m.identity = hello.identity.clone();
            m.conn = Some(conn);
            // 다시 붙은 agent는 설정이 바뀌었을 수 있다. 새 보고를 기다린다.
            m.health = None;
            m.health_sent = None;
            m.last_seen_ms = Some(util::now_ms());
            let ids: std::collections::HashSet<_> = hello.sessions.iter().map(|s| s.id.clone()).collect();
            i.panes.retain(|id, p| p.machine_id != hello.machine_id || ids.contains(id));
            for s in &hello.sessions {
                let p = i.panes.entry(s.id.clone()).or_insert_with(|| PaneRt {
                    machine_id: hello.machine_id.clone(),
                    info: s.clone(),
                    mirror: None,
                    events: broadcast::channel(1024).0,
                    lease: None,
                    agent: None,
                    codex_speed: None,
                    turn_from: None,
                    turn_reported: false,
                    alert: None,
                    transcript: None,
                    answered: None,
                    restore_error: None,
                    kill_sent: false,
                    replaced: Vec::new(),
                });
                if exited_by_itself(&p.info, s, p.kill_sent) {
                    exited.push(s.id.clone());
                }
                p.codex_speed = None;
                p.info = s.clone();
                if let Some(m) = p.mirror.as_mut() {
                    m.synced = false;
                }
                aaa_common::diagnostic::ignored(
                    p.events.send(PaneEvent::State),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
            }
            subscribe = hello
                .sessions
                .iter()
                .map(|s| (s.id.clone(), i.panes.get(&s.id).and_then(|p| p.mirror.as_ref()).map(|m| m.offset)))
                .collect();
        }
        self.bump();
        for id in exited {
            self.spawn_close_exited(id, hello.machine_id.clone());
        }
        // host가 내려간 사이 끝났거나 지워진 이 머신 pane의 질문을 닫는다(agent가 세션을 모두 알렸다)
        let open = self.questions(false);
        let gone: Vec<String> = {
            let i = self.inner.lock().unwrap();
            open.into_iter()
                .filter(|q| q.machine == hello.machine_name)
                .filter(|q| !i.panes.get(&q.pane).is_some_and(|p| p.info.state.is_running()))
                .map(|q| q.pane)
                .collect()
        };
        for pane in gone {
            self.close_questions_of(&pane);
        }
        // agent가 남겨 둔 세션별 마지막 harness 상태(재부팅 복원이 harness 세션 ID를 바로 안다)
        for ev in &hello.harness {
            self.on_harness(&hello.machine_id, ev.clone());
        }
        for (sid, from) in subscribe {
            self.replay_harness(&sid);
            self.spawn_subscribe(hello.machine_id.clone(), sid, from);
        }
        self.rewatch(hello.machine_id.clone());
        self.sync_chat_watches_if_wanted();
        self.restore_after_reboot(&hello.machine_id, generation);
        self.bump();
        self.tick_wake.notify_one();
    }

    pub fn agent_disconnected(&self, machine_id: &str, generation: u64) {
        let mut i = self.inner.lock().unwrap();
        let Some(m) = i.machines.get_mut(machine_id) else { return };
        if m.conn.as_ref().map(|c| c.generation) != Some(generation) {
            return; // 이미 새 연결로 바뀌었다
        }
        if let Some(c) = m.conn.take() {
            c.fail_all();
        }
        m.colors = None;
        m.last_seen_ms = Some(util::now_ms());
        for p in i.panes.values_mut().filter(|p| p.machine_id == machine_id) {
            if let Some(mm) = p.mirror.as_mut() {
                mm.synced = false;
            }
            aaa_common::diagnostic::ignored(p.events.send(PaneEvent::State), "수신자가 끝난 뒤에도 현재 작업은 마친다");
        }
        drop(i);
        self.chat_machine_gone(machine_id);
        self.bump();
    }

    pub(super) fn conn_of(&self, machine_id: &str) -> Option<AgentConn> {
        self.inner.lock().unwrap().machines.get(machine_id).and_then(|m| m.conn.clone())
    }

    pub(super) fn spawn_subscribe(self: &Arc<Self>, machine_id: String, session: String, from: Option<u64>) {
        let me = self.clone();
        tokio::spawn(async move {
            let Some(conn) = me.conn_of(&machine_id) else { return };
            if let Err(e) = conn.request(AgentRequest::Subscribe { session: session.clone(), from }).await {
                tracing::debug!(%session, "구독 실패: {e}");
            }
        });
    }

    /// 링크 reader가 순서대로 부른다: 구독 응답은 출력 chunk보다 먼저 mirror에 반영된다.
    pub fn on_subscribed(&self, session: &str, snapshot: Option<aaa_proto::Snapshot>, offset: u64) {
        let mut i = self.inner.lock().unwrap();
        let Some(p) = i.panes.get_mut(session) else { return };
        match snapshot {
            // 끝난 세션인데 agent에 마지막 화면 파일이 없다(재부팅 등으로 session host가 기록 없이 사라짐):
            // host가 가진 마지막 화면을 그대로 둔다(§4.4 「마지막 snapshot과 tail」).
            Some(s) if s.replay.0.is_empty() && !p.info.state.is_running() && p.mirror.is_some() => {
                if let Some(m) = p.mirror.as_mut() {
                    m.synced = true;
                }
            }
            Some(s) => {
                let mut vt = Vt::new(s.rows, s.cols);
                vt.feed(&s.replay.0);
                p.mirror = Some(Mirror { vt, offset: s.offset, synced: true, source_more: s.history_more });
                aaa_common::diagnostic::ignored(
                    p.events.send(PaneEvent::Reset),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
            }
            None => match p.mirror.as_mut() {
                Some(m) if m.offset == offset => m.synced = true,
                _ => tracing::warn!(%session, "구독 offset이 mirror와 다르다"),
            },
        }
    }

    pub fn on_chunk(self: &Arc<Self>, machine_id: &str, c: TerminalChunk) {
        let resync = {
            let mut i = self.inner.lock().unwrap();
            let Some(p) = i.panes.get_mut(&c.session) else { return };
            let Some(m) = p.mirror.as_mut() else { return };
            if !m.synced {
                return;
            }
            let end = c.offset + c.data.len() as u64;
            if end <= m.offset {
                return;
            }
            if c.offset > m.offset {
                m.synced = false;
                Some(m.offset)
            } else {
                let data = c.data[(m.offset - c.offset) as usize..].to_vec();
                let start = m.offset;
                m.vt.feed(&data);
                m.offset = end;
                aaa_common::diagnostic::ignored(
                    p.events.send(PaneEvent::Output(start, Arc::new(data))),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
                None
            }
        };
        if let Some(from) = resync {
            tracing::warn!(session = %c.session, "출력에 빈틈: 다시 구독한다");
            self.spawn_subscribe(machine_id.to_string(), c.session, Some(from));
        }
    }

    pub fn on_session(self: &Arc<Self>, machine_id: &str, info: SessionInfo, started: bool) {
        let closing = {
            let mut i = self.inner.lock().unwrap();
            let p = i.panes.entry(info.id.clone()).or_insert_with(|| PaneRt {
                machine_id: machine_id.to_string(),
                info: info.clone(),
                mirror: None,
                events: broadcast::channel(1024).0,
                lease: None,
                agent: None,
                codex_speed: None,
                turn_from: None,
                turn_reported: false,
                alert: None,
                transcript: None,
                answered: None,
                restore_error: None,
                kill_sent: false,
                replaced: Vec::new(),
            });
            if let Some(m) = p.mirror.as_mut()
                && (p.info.cols, p.info.rows) != (info.cols, info.rows)
            {
                m.vt.resize(info.rows, info.cols);
            }
            let exited = exited_by_itself(&p.info, &info, p.kill_sent);
            p.info = info.clone();
            aaa_common::diagnostic::ignored(p.events.send(PaneEvent::State), "수신자가 끝난 뒤에도 현재 작업은 마친다");
            exited
        };
        if info.harness.as_deref() == Some("agy")
            && matches!(info.state, SessionState::Exited { .. } | SessionState::Interrupted)
            && let Err(error) = self.db.release_agy_pane(machine_id, &info.id)
        {
            tracing::warn!("종료된 agy의 구독 자리 반환 실패: {error:#}");
        }
        if closing {
            self.spawn_close_exited(info.id.clone(), machine_id.to_string());
        }
        self.bump();
        if !info.state.is_running() {
            self.close_questions_of(&info.id);
        }
        self.replay_harness(&info.id);
        // 끝난 세션은 thread 묶음을 바꾼다(transitions.rs): 다음 주기를 기다리지 않는다. 끝나기 직전에 쓴 보고를 먼저 본다.
        if !info.state.is_running() {
            match self.thread_of_pane(&info.id) {
                Some(r) if r.status == aaa_proto::ThreadStatus::Open => self.refresh_report(&r.project, &r.id),
                _ => self.tick_wake.notify_one(),
            }
        }
        if started {
            self.spawn_subscribe(machine_id.to_string(), info.id, None);
        }
    }

    pub fn on_resync(self: &Arc<Self>, machine_id: &str, session: &str) {
        let from = {
            let mut i = self.inner.lock().unwrap();
            i.panes.get_mut(session).and_then(|p| p.mirror.as_mut()).map(|m| {
                m.synced = false;
                m.offset
            })
        };
        self.spawn_subscribe(machine_id.to_string(), session.to_string(), from);
    }
}
