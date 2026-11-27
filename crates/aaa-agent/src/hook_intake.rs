//! harness hook 사건을 받아 상태·안내로 바꾼다.

use super::*;

impl Agent {
    /// 세션 안의 `aaa hook`·`aaa report`가 보내는 사건을 받는다(로컬 socket, 같은 사용자만)
    pub fn start_hooks(self: &Arc<Self>) -> Result<()> {
        let listener = aaa_common::ipc::LocalListener::bind(&hooks::socket(&self.state)).context("hook socket")?;
        let me = self.clone();
        tokio::spawn(async move {
            while let Ok((mut s, pid)) = listener
                .accept_peer()
                .await
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "수신 루프 종료 원인"))
            {
                let me = me.clone();
                tokio::spawn(async move {
                    let reply = hooks::reply(me.on_hook(&mut s, pid).await);
                    aaa_common::diagnostic::ignored(
                        tokio::io::AsyncWriteExt::write_all(&mut s, reply.as_bytes()).await,
                        "보조 작업 실패 뒤 기존 처리를 계속한다",
                    );
                    aaa_common::diagnostic::ignored(
                        tokio::io::AsyncWriteExt::shutdown(&mut s).await,
                        "보조 작업 실패 뒤 기존 처리를 계속한다",
                    );
                });
            }
        });
        Ok(())
    }

    /// hook 사건 하나. 돌려준 것은 `aaa hook`이 Claude·Codex에 출력할 진행 안내다.
    pub(super) async fn on_hook(
        self: &Arc<Self>,
        s: &mut aaa_common::ipc::LocalStream,
        pid: Option<u32>,
    ) -> Result<Option<hooks::Reply>> {
        let msg = hooks::read_msg(s).await?;
        if msg.harness == "deploy_guard" {
            let guard = deploy::guard::read(&self.state, &msg.session)?;
            if pid != Some(guard.worker_pid) {
                bail!("감시 프로세스의 알림이 아니다");
            }
            if self.host_recovery.load(std::sync::atomic::Ordering::Relaxed) {
                self.send_host(&aaa_proto::LinkMsg::Event(aaa_proto::AgentEvent::DeployGuardChanged {
                    id: guard.spec.id,
                }))
                .await;
            }
            return Ok(None);
        }
        if msg.session.is_empty() || !msg.session.chars().all(|c| c.is_ascii_alphanumeric()) {
            if msg.harness == "post" {
                self.reject_post(&msg, "세션 ID 형식이 아니다").await;
            }
            bail!("세션 ID 형식이 아니다");
        }
        let spec: SpecFile = match util::read_json(&SessionPaths::new(&self.state, &msg.session).spec()) {
            Ok(spec) => spec,
            Err(e) => {
                if msg.harness == "post" {
                    self.reject_post(&msg, "세션 기록이 없다").await;
                    bail!("세션 기록이 없다");
                }
                return Err(e);
            }
        };
        if hooks::hook_token(&spec.token) != msg.token {
            if msg.harness == "post" {
                self.reject_post(&msg, "hook token이 맞지 않는다").await;
            }
            bail!("hook token이 맞지 않는다");
        }
        if msg.harness == "thread_read" {
            return self.on_thread_read(msg, pid).await.map(Box::new).map(hooks::Reply::ThreadRead).map(Some);
        }
        if msg.harness == "imagegen" {
            return self.on_image(msg, pid).await.map(hooks::Reply::Image).map(Some);
        }
        if msg.harness == "review" {
            return self.on_review(msg, pid).await.map(hooks::Reply::Review).map(Some);
        }
        if msg.harness == "post" {
            let result = self.on_post(msg.clone(), pid).await;
            if let Err(e) = &result {
                self.reject_post(&msg, &e.to_string()).await;
            }
            return result.map(hooks::Reply::Text).map(Some);
        }
        let agy_hook = matches!(
            msg.harness.as_str(),
            "agy-status" | "agy-session-start" | "agy-post-tool-use" | "agy-pre-invocation" | "agy-stop"
        );
        if matches!(msg.harness.as_str(), "codex" | "quota") || agy_hook {
            // 옛 notify는 harness를 선언하지 않은 aaa 셸에서도 왔다. token·peer 검사는 계속 적용한다.
            if msg.harness == "quota" {
                anyhow::ensure!(spec.spec.harness.as_deref() == Some("claude"), "Claude 상태줄이 아니다");
            } else if agy_hook {
                anyhow::ensure!(
                    spec.spec.harness.as_deref() == Some("agy") && spec.spec.role == Some(aaa_proto::Role::Worker),
                    "agy로 띄운 aaa worker가 아니다"
                );
            } else if msg.payload.get("type").and_then(|v| v.as_str()) != Some("agent-turn-complete") {
                anyhow::ensure!(
                    detect::Harness::of_session(spec.spec.harness.as_deref(), &spec.spec.argv)
                        == Some(detect::Harness::Codex),
                    "Codex로 띄운 aaa 세션이 아니다"
                );
            }
            // SessionStart는 attach의 pane 등록보다 먼저 올 수 있다. 재시도·sleep 대신 등록 사건을 기다린다.
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    let ready = self.session_ready.notified();
                    tokio::pin!(ready);
                    ready.as_mut().enable();
                    if self.sessions.lock().unwrap().contains_key(&msg.session) {
                        break;
                    }
                    ready.await;
                }
            })
            .await
            .context("hook 세션 등록 대기 시간 초과")?;
            // env·token을 복사한 세션 밖 프로세스는 전역 hook을 가장할 수 없다(post와 같은 peer 판정).
            let panes: Vec<_> = self
                .sessions()
                .into_iter()
                .filter(|s| s.state.is_running())
                .filter_map(|s| s.pid.map(|p| (s.id, p)))
                .collect();
            anyhow::ensure!(
                pid.is_some_and(|p| aaa_common::caller::judge(p, &panes, &[])
                    == aaa_common::caller::Caller::Pane(msg.session.clone())),
                "hook peer가 이 세션 안에 없다"
            );
        }
        if agy_hook {
            let now = util::now_ms();
            if msg.harness == "agy-status" {
                self.on_agy_status(&msg.session, &spec.spec.argv, &msg.payload, now).await;
                return Ok(None);
            }
            let root = {
                let started = self.harness_started.lock().unwrap();
                let root = started.get(&msg.session).cloned();
                if root.is_none()
                    && msg.harness == "agy-pre-invocation"
                    && let Some(ev) = agy::initial_receipt(&msg.session, &msg.payload, now)
                {
                    self.progress
                        .lock()
                        .unwrap()
                        .entry(msg.session.clone())
                        .or_default()
                        .agy_initial_receipt
                        .get_or_insert_with(|| Box::new(ev));
                }
                root
            };
            if msg.harness == "agy-pre-invocation"
                && let Some(ev) = agy::pre_invocation(&msg.session, root.as_deref(), &msg.payload, now)
            {
                self.harness_event(ev).await;
            }
            #[cfg(target_os = "macos")]
            if msg.harness == "agy-post-tool-use"
                && agy::root_hook(root.as_deref(), &msg.payload).is_some()
                && agy::sandbox_progress::report_tool_succeeded(&msg.payload, &self.exe)
            {
                self.accept_agy_sandbox_report(&msg.session, now).await;
            }
            let prefix = aaa_proto::fill_brief(aaa_proto::BRIEF_EXE, "", &self.exe.to_string_lossy());
            let reply = agy::hook_reply(
                &msg.harness,
                root.as_deref(),
                &msg.payload,
                self.progress.lock().unwrap().entry(msg.session).or_default(),
                &prefix,
                now,
            );
            return Ok(Some(hooks::Reply::Text(reply)));
        }
        if msg.harness == "quota" {
            self.observe_claude_effort(&msg.session, &msg.payload, util::now_ms());
            return self.record_quota(&spec, &msg.payload).map(hooks::Reply::Text).map(Some);
        }
        let now = util::now_ms();
        if matches!(msg.harness.as_str(), "claude" | "codex")
            && msg.payload.get("hook_event_name").and_then(|v| v.as_str()) == Some("SessionStart")
            && matches!(msg.payload.get("source").and_then(|v| v.as_str()), Some("startup" | "resume"))
        {
            self.harness_process_started(&msg.session, now);
        }
        if msg.harness == "claude" && msg.payload.get("hook_event_name").and_then(|v| v.as_str()) == Some("PostToolUse")
        {
            self.observe_claude_effort(&msg.session, &msg.payload, now);
        }
        if let Some(ev) = hooks::normalize(&msg.session, &msg.harness, &msg.payload, now) {
            if msg.harness == "report" {
                let mut map = self.progress.lock().unwrap();
                let rec = map.entry(msg.session.clone()).or_default();
                (rec.reported_at, rec.percent) = (now, ev.percent);
            }
            self.harness_event(ev).await;
        }
        // aaa가 hook 설정을 주는 thread 세션만 안내한다(herdr-projects `managed`: coordinator·thread pane)
        let managed = spec.spec.role.is_some();
        Ok(if managed && matches!(msg.harness.as_str(), "claude" | "codex") {
            let coordinator = spec.spec.role == Some(aaa_proto::Role::Coordinator);
            let project = spec
                .spec
                .cwd
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|p| p.to_str())
                .unwrap_or_default();
            let auto_context = if coordinator
                && matches!(hooks::guide_kind(&msg.payload), Some("SessionStart" | "UserPromptSubmit"))
            {
                Some(self.coordinator_auto_context(project).await)
            } else {
                None
            };
            // 턴 도중 도착한 inbox: coordinator 맨 위 agent의 도구 결과마다 host에 묻는다(t-1627). 다른 harness 세션의
            // 사건이면 묻지 않는다: 물으면 host가 알린 것으로 적는데 안내는 버려진다
            let inbox =
                if coordinator && hooks::inbox_hook(&msg.payload) && !self.foreign_hook(&msg.session, &msg.payload) {
                    self.coordinator_inbox_notice(project, &msg.session).await
                } else {
                    None
                };
            let reentry = spec
                .spec
                .env
                .iter()
                .find(|(key, _)| key == aaa_proto::AUTOMATION_REENTRY_ENV)
                .map(|(_, text)| text.as_str());
            self.guide(
                &msg.session,
                &msg.payload,
                hooks::Reader { thread: !coordinator, reentry },
                now,
                auto_context.as_ref().map(|(state, text)| (state.as_str(), text.as_str())),
                inbox.as_deref(),
            )
            .map(hooks::Reply::Text)
        } else {
            None
        })
    }

    /// 상태줄은 대화 ID와 부팅·대기 상태를 확인한다. 승인창의 종류와 내용은 화면 감지가 확인한다.
    pub(super) async fn on_agy_status(
        self: &Arc<Self>,
        session: &str,
        argv: &[String],
        payload: &serde_json::Value,
        now: u64,
    ) {
        let matches_account = agy::matches_account(self.threads_cfg.lock().unwrap().agy.as_ref(), payload);
        if matches_account
            && self.host_agy_quota.load(std::sync::atomic::Ordering::Relaxed)
            && let Some(sample) = agy::quota_sample(session, payload, now)
        {
            let changed = {
                let mut samples = self.agy_quota.lock().unwrap();
                let changed = !samples.get(session).is_some_and(|old| {
                    old.buckets == sample.buckets && old.plan == sample.plan && now.saturating_sub(old.at_ms) < 60_000
                });
                if changed {
                    samples.insert(session.into(), sample.clone());
                }
                changed
            };
            if changed {
                self.send_host(&LinkMsg::Event(AgentEvent::AgyQuota(sample))).await;
            }
        }
        let Some((mut kind, mut message)) = agy::status_activity(payload) else { return };
        let has_identity = payload.get("email").and_then(serde_json::Value::as_str).is_some_and(|s| !s.is_empty());
        if has_identity && !matches_account {
            kind = "error";
            message = Some("agy 로그인이 등록한 개인 구독과 다르다");
        }
        let id = agy::status_conversation(payload);
        let resume = argv.windows(2).find_map(|pair| (pair[0] == "--conversation").then_some(pair[1].as_str()));
        let root = self.harness_started.lock().unwrap().get(session).cloned();
        if has_identity && !matches_account {
            // 등록한 계정이 아니면 대화 ID와 입력 접수도 넘기지 않는다.
            self.harness_started.lock().unwrap().remove(session);
            self.progress.lock().unwrap().entry(session.into()).or_default().agy_initial_receipt = None;
        } else if matches_account
            && let Some(expected) = root.as_deref().or(resume)
            && id != Some(expected)
        {
            kind = "agy_starting";
            message = Some("agy 대화 ID 확인 중");
        } else if matches_account
            && root.is_none()
            && let Some(id) = id
        {
            self.harness_event(agy::event(session, "session_start", Some(id), None, now)).await;
            let pending = self.progress.lock().unwrap().get_mut(session).and_then(|p| p.agy_initial_receipt.take());
            if let Some(ev) = pending.filter(|e| e.harness_session.as_deref() == Some(id)) {
                self.harness_event(*ev).await;
            }
        }
        let already = self.harness_last.lock().unwrap().get(session).is_some_and(|list| {
            list.iter().any(|e| {
                Slot::of(e) == Some(Slot::State)
                    && e.harness == "agy"
                    && e.kind == kind
                    && e.message.as_deref() == message
            })
        });
        if !already {
            self.harness_event(agy::event(session, kind, None, message.map(str::to_owned), now)).await;
        }
    }

    #[cfg(target_os = "macos")]
    pub(super) async fn accept_agy_sandbox_report(self: &Arc<Self>, session: &str, now: u64) {
        let ev = {
            let mut map = self.progress.lock().unwrap();
            let state = map.entry(session.into()).or_default();
            let Some(report) = agy::sandbox_progress::read(session) else { return };
            if state.agy_report_seen.as_ref() == Some(&report.nonce) {
                return;
            }
            let payload = serde_json::json!({"percent": report.percent, "activity": report.activity});
            let Some(ev) = hooks::normalize(session, "report", &payload, now) else { return };
            state.agy_report_seen = Some(report.nonce);
            (state.reported_at, state.percent) = (now, ev.percent);
            ev
        };
        self.harness_event(ev).await;
    }

    /// Claude·Codex hook에 넣을 진행 안내(herdr-projects progress.rs `hook`). 이 세션이 시작한 harness 세션과 다른 세션의 사건에는
    /// 넣지 않는다(`foreign_session`). 실행 파일 경로는 brief와 같은 꼴이다(`fill_brief`).
    /// coordinator auto mode 상태는 SessionStart(compaction 포함)와 상태가 바뀐 뒤 첫 입력에만 넣는다: hook 글은 compaction
    /// 전까지 매 요청에 다시 실리고, 같은 상태는 이미 대화에 있다.
    pub(super) fn guide(
        &self,
        session: &str,
        p: &serde_json::Value,
        reader: hooks::Reader,
        now: u64,
        auto_context: Option<(&str, &str)>,
        inbox: Option<&str>,
    ) -> Option<String> {
        // `aaa report`의 PostToolUse는 진행 안내를 넣지 않지만 inbox는 붙인다
        let Some(kind) = hooks::guide_kind(p) else {
            return inbox.map(|text| hooks::output("PostToolUse", &hooks::inbox_context(text)));
        };
        if kind != "SessionStart" && self.foreign_hook(session, p) {
            return None;
        }
        let prefix = aaa_proto::fill_brief(aaa_proto::BRIEF_EXE, "", &self.exe.to_string_lossy());
        let mut map = self.progress.lock().unwrap();
        let rec = map.entry(session.to_string()).or_default();
        let progress = hooks::respond(rec, kind, &prefix, now, reader, hooks::typed_by_aaa(p));
        let text = hooks::with_inbox(hooks::with_auto_mode(rec, progress, auto_context), inbox)?;
        Some(hooks::output(kind, &text))
    }

    /// 이 pane이 시작한 harness 세션과 다른 세션의 hook 사건인가(SessionStart 전에는 모른다: 버리지 않는다)
    pub(super) fn foreign_hook(&self, session: &str, p: &serde_json::Value) -> bool {
        let id = p.get("session_id").and_then(|v| v.as_str());
        matches!((self.harness_started.lock().unwrap().get(session), id), (Some(started), Some(id)) if started != id)
    }

    /// coordinator hook에 붙일 inbox 한 줄을 host에 묻는다(t-1627). 도구마다 부르므로 짧게 기다리고, host가 없거나 옛 판이면
    /// 조용히 넘긴다: 턴 끝 nudge가 남는다.
    pub(super) async fn coordinator_inbox_notice(&self, project: &str, pane: &str) -> Option<String> {
        let root = self.state.parent().unwrap_or(&self.state);
        let request = aaa_proto::FrontRequest::InboxHookNotice { project: project.into(), pane: pane.into() };
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let mut client = aaa_common::front::FrontClient::connect(root).await?;
            match client.call(request).await?? {
                aaa_proto::FrontResponse::InboxHookNotice { text } => Ok::<_, anyhow::Error>(text),
                _ => anyhow::bail!("inbox hook 조회의 응답 종류가 다르다"),
            }
        })
        .await;
        match result {
            Ok(Ok(text)) => text,
            other => {
                tracing::info!(%project, %pane, "coordinator hook inbox 조회 실패(턴 끝 알림에 맡긴다): {other:?}");
                None
            }
        }
    }

    /// coordinator는 host 머신에 있다. 재시작·compaction 뒤에도 캐시 대신 host의 정본을 읽는다. (상태, 안내)를 돌려준다:
    /// 예외 목록은 켜져 있을 때만 싣는다(꺼져 있으면 쓸 데가 없고, 켜지면 그 입력에 다시 넣는다).
    pub(super) async fn coordinator_auto_context(&self, project: &str) -> (String, String) {
        let root = self.state.parent().unwrap_or(&self.state);
        let result = tokio::time::timeout(Duration::from_secs(2), async {
            let mut client = aaa_common::front::FrontClient::connect(root).await?;
            match client.call(aaa_proto::FrontRequest::AutoModeGet { project: project.into() }).await?? {
                aaa_proto::FrontResponse::AutoMode(mode) => Ok::<_, anyhow::Error>(mode),
                _ => anyhow::bail!("auto mode 조회의 응답 종류가 다르다"),
            }
        })
        .await;
        match result {
            Ok(Ok(mode)) if mode.on => (
                "on".into(),
                format!(
                    "aaa coordinator auto_mode: on · 자동 결정 {}. 일반 질문은 `aaa guide auto-mode`대로 스스로 정한다. harness 권한과 무관하며 사용자 전용 승인을 대신하지 않는다. 늘 사용자에게 물을 예외: {}",
                    mode.decisions,
                    aaa_proto::AUTO_MODE_EXCEPTIONS
                ),
            ),
            Ok(Ok(_)) => ("off".into(), "aaa coordinator auto_mode: off. 켜지면 다음 입력에 알린다.".into()),
            other => {
                tracing::warn!("coordinator hook auto mode 조회 실패: {other:?}");
                (
                    "unknown".into(),
                    "aaa coordinator auto_mode: 확인 못함. 자동 결정 전에 aaa context로 host 상태와 예외 목록을 확인한다. 이전 상태를 추측하지 않는다.".into(),
                )
            }
        }
    }
}
