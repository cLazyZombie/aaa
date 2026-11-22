//! pane 입력: brief·안내·메시지를 치고 받았는지 확인한다.

use super::*;

impl Host {
    // ───────────── 입력(brief 안내·메시지) ─────────────

    /// 글을 입력하고 Enter. harness가 받았다고 알리면 true. 프로그램이 bracketed paste를 켰으면 글을 감싼다.
    pub(crate) async fn send_prompt(self: &Arc<Self>, mid: &str, pane: &str, text: &str) -> Result<bool, String> {
        self.submit(mid, pane, Some(text), None).await
    }

    pub(crate) async fn send_nudge(self: &Arc<Self>, mid: &str, pane: &str, text: &str) -> Result<bool, SubmitErr> {
        let id = aaa_common::util::new_id();
        let mut submission = Submission::new(&id);
        submission.nudge = true;
        self.submit_traced(mid, pane, Some(text), None, &mut submission).await
    }

    /// 입력창에 남은 글을 Enter로만 보낸다(다시 치지 않는다). 받았다고 알리면 true.
    pub(crate) async fn send_enter(self: &Arc<Self>, mid: &str, pane: &str) -> Result<bool, String> {
        self.submit(mid, pane, None, None).await
    }

    pub(super) async fn submit(
        self: &Arc<Self>,
        mid: &str,
        pane: &str,
        text: Option<&str>,
        on_accept: Option<OnFire>,
    ) -> Result<bool, String> {
        self.submit_checked(mid, pane, text, on_accept).await.map_err(|e| e.msg)
    }

    /// `on_accept`는 받았다고 알게 된 그 자리에서 한 번 돈다(Ok(true)일 때만): hook이면 그 hook 처리 안에서.
    /// 실패는 입력하기 전인지(`typed = false`, herdr-projects `refused_before_typing`) 함께 알린다.
    pub(crate) async fn submit_checked(
        self: &Arc<Self>,
        mid: &str,
        pane: &str,
        text: Option<&str>,
        on_accept: Option<OnFire>,
    ) -> Result<bool, SubmitErr> {
        let id = aaa_common::util::new_id();
        self.submit_traced(mid, pane, text, on_accept, &mut Submission::new(&id)).await
    }

    pub(super) async fn submit_traced(
        self: &Arc<Self>,
        mid: &str,
        pane: &str,
        text: Option<&str>,
        on_accept: Option<OnFire>,
        submission: &mut Submission<'_>,
    ) -> Result<bool, SubmitErr> {
        let trace = submission.id;
        self.input_event(
            pane,
            trace,
            "preparing",
            &text.map(|t| crate::input_log::submit_meta(t, false)).unwrap_or_else(|| "enter_only=true".into()),
        );
        let result = self.submit_steps(mid, pane, text, on_accept, submission).await;
        match &result {
            Ok(false) => self.input_event(pane, trace, "unknown", "수락 증거 없음; 재전송하지 않음"),
            Err(error) => self.input_event(pane, trace, "failed", &format!("typed={}", error.typed)),
            Ok(true) => {}
        }
        result
    }

    pub(super) async fn submit_steps(
        self: &Arc<Self>,
        mid: &str,
        pane: &str,
        text: Option<&str>,
        on_accept: Option<OnFire>,
        submission: &mut Submission<'_>,
    ) -> Result<bool, SubmitErr> {
        let trace = submission.id;
        let before = |msg: String| SubmitErr { typed: false, msg };
        let after = |msg: String| SubmitErr { typed: true, msg };
        let conn = self.live_conn(mid).map_err(|e| before(e.message))?;
        // 화면 조회·입력보다 먼저 구독한다. 응답 처리 중 도착한 출력도 놓치지 않는다.
        let mut changes = Some(self.pane_events(pane).map_err(|e| before(e.message))?);
        // 친 글에 프로그램이 반응하고 Enter 뒤 harness가 받았다고 알리기까지 기다리는 시간(설정 `accept_wait_ms`, 기본 10초)
        let accept_wait = Duration::from_millis(self.cfg.projects.accept_wait_ms);
        // 선택 창에 글을 치면 Enter가 엉뚱한 선택지를 고른다(Claude 폴더 신뢰 창의 기본은 「No, exit」)
        let nudge_watch = submission.nudge.then(|| conn.watch_input(pane, true));
        let screen = self.screen(mid, pane).await;
        if submission.nudge {
            let agent = self.pane_agent(pane);
            let empty = screen.as_ref().is_some_and(|s| crate::inbox::nudge_prompt_empty(s, agent.as_ref()));
            if !empty || agent.is_some_and(|a| !matches!(a.activity, AgentActivity::Idle | AgentActivity::Started)) {
                return Err(before("알림을 보내기 전에 입력창이 바뀌었다".into()));
            }
        }
        let shown = screen.map(|s| s.lines);
        if shown.as_deref().is_some_and(limit_wait_screen) {
            submission.limit_wait = true;
            return Err(before("사용량 한도 대기 화면에 입력하지 않았다".into()));
        }
        if let Some((_, q)) = shown.as_deref().and_then(modal_of) {
            return Err(before(format!("선택 창이 떠 있어 입력하지 않았다: {q}")));
        }
        let agent = self.pane_agent(pane);
        // 확인창(권한·질문)이 떠 있으면 친 글과 Enter를 그 창이 받는다
        if agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Question) {
            return Err(before("확인창이 떠 있어 입력하지 않았다".into()));
        }
        let agy = agent.as_ref().is_some_and(|a| a.harness == "agy");
        if agy
            && (text.is_none()
                || !agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Idle)
                || shown.as_ref().is_none_or(|lines| {
                    crate::prompt_box::check("agy", &lines.join("\n")) != crate::prompt_box::Draft::Empty
                }))
        {
            return Err(before("agy의 빈 입력창을 확인하지 못했다".into()));
        }
        let hooked = agent.as_ref().is_some_and(|a| a.harness != "report");
        // 이미 작업 중이면 새 본문의 대화·큐 영수증을 우선한다. 이전 Working 상태는 수락 증거가 아니다.
        let busy = hooked && agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Working);
        // herdr-projects progress::respond(UserPromptSubmit)와 같은 정리다. 전역 hook이 없는 Codex도
        // 입력 받음(working·question·화면 변화)을 확인한 바로 그 자리에서 직전 기다림을 끝낸다.
        // 전송 전에 거부되거나 받음이 unknown이면 지우지 않으며, 그 사이 새로 온 보고도 지키도록 시각을 묶는다.
        let waiting_at = agent
            .as_ref()
            .filter(|a| a.progress.as_deref() == Some(crate::orch::WAITING))
            .and_then(|a| a.progress_at_ms);
        let watch = nudge_watch.or_else(|| submission.message.then(|| conn.watch_input(pane, false)));
        let input_watch = watch.as_ref().map(Arc::downgrade);
        // unknown으로 끝난 대기자가 남아도 host 자체를 붙잡지 않는다.
        let me = Arc::downgrade(self);
        let target = pane.to_string();
        let accepted_trace = trace.to_owned();
        let on_accept: Option<OnFire> = Some(Box::new(move || {
            if input_watch.is_some_and(|watch| !watch.upgrade().is_some_and(|watch| *watch.borrow())) {
                return;
            }
            if let Some(me) = me.upgrade() {
                me.input_event(&target, &accepted_trace, "accepted", "method=post_enter_evidence");
                me.input_accepted(&target, waiting_at);
            }
            if let Some(f) = on_accept {
                f();
            }
        }));
        let (hook_accept, mut screen_accept) = if hooked && !busy { (on_accept, None) } else { (None, on_accept) };
        // 프로그램이 bracketed paste를 켰을 때만 감싸고 Enter도 그 터미널 모드대로 쓴다(herdr `encode_api_submission_parts`,
        // src/app/api_helpers.rs). 켜지 않은 프로그램에 감싸 보내면 `^[[200~`가 글로 남는다.
        let modes = self.key_modes(pane);
        // 비동기 대기와 같은 시계다. 운영의 단조 시각은 그대로이며, 시험은
        // 실제 10초 받음 시한을 줄이지 않고 Tokio 시각으로 경계를 넘긴다.
        let deadline = tokio::time::Instant::now() + accept_wait;
        let probe = if busy || submission.message {
            if let Some(text) = text {
                self.prompt_receipt_page(mid, pane, deadline)
                    .await
                    .map(|page| crate::prompt_receipt::ReceiptProbe::new(page, text))
            } else {
                None
            }
        } else {
            None
        };
        if tokio::time::Instant::now() >= deadline {
            return Err(before("입력 수락 확인 준비가 시한 안에 끝나지 않아 입력하지 않았다".into()));
        }
        let windows_codex = agent.as_ref().is_some_and(|a| a.harness == "codex")
            && self.inner.lock().unwrap().machines.get(mid).is_some_and(|m| m.os.as_deref() == Some("windows"));
        self.input_event(
            pane,
            trace,
            "input_modes",
            &format!(
                "bracketed_paste={} application_cursor={} modify_other_keys={} windows_codex={} receipt={}",
                modes.bracketed_paste,
                modes.application_cursor,
                modes.modify_other_keys,
                windows_codex,
                probe.is_some()
            ),
        );
        if let Some(card) = submission.card {
            let screen =
                self.screen(mid, pane).await.ok_or_else(|| before("카드 답을 보낼 화면을 읽지 못했다".into()))?;
            if limit_wait_screen(&screen.lines) {
                submission.limit_wait = true;
                return Err(before("사용량 한도 대기 화면에 입력하지 않았다".into()));
            }
            if modal_of(&screen.lines).is_some()
                || self.pane_agent(pane).is_some_and(|a| a.activity == AgentActivity::Question)
            {
                return Err(before("카드 답을 보내기 전에 확인창이 열렸다".into()));
            }
            if let CardSend::Resume(pending) = card
                && !self.pending_report_current(pending).await.map_err(|e| before(e.message))?
            {
                return Err(before("새 보고가 와 카드 답을 보류했다".into()));
            }
            if !self.card_submission_current(card, mid, pane) {
                return Err(before("카드 답의 보고나 실행 중인 pane이 바뀌었다".into()));
            }
            if let CardSend::Resume(pending) = card {
                if !self.card_screen_ready(pane, &screen) || watch.as_ref().is_some_and(|w| !*w.borrow()) {
                    return Err(before("카드 답을 다시 보낼 입력창이 준비되지 않았다".into()));
                }
                if !self.claim_pending_answer(pending).map_err(|e| before(e.message))? {
                    return Err(before("이미 처리한 카드 답이다".into()));
                }
                submission.resume_claimed = true;
            }
        }
        if let Some(text) = text {
            submission.input = MessageInput::PasteUnconfirmed;
            conn.request_watched(
                AgentRequest::Input { session: pane.to_string(), data: B64(modes.paste(text.trim_end())) },
                crate::REQUEST_TIMEOUT,
                watch.as_ref(),
                submission.nudge,
            )
            .await
            .map_err(|e| after(e.message))?
            .ok_or_else(|| before("다른 입력이 와 알림을 쓰지 않았다".into()))?;
            submission.input = MessageInput::DraftWritten;
            self.input_event(
                pane,
                trace,
                "paste_written",
                &crate::input_log::submit_meta(&String::from_utf8_lossy(&modes.paste(text.trim_end())), false),
            );
        }
        crate::text::submission_gap().await;
        // 치는 동안 뜬 확인창(t-0340 보안 감사): Enter가 사람이 보지 않은 선택지(권한 창의 기본은 「Yes」)를 고른다.
        // Enter 바로 전에 화면과 harness 상태를 다시 본다. 친 글은 남으니 받았는지 모르는 것으로 둔다(다시 보내지 않는다).
        // 입력 상자에 친 글을 보이는 harness(hook·화면 감지가 있는 Claude 등)는 그 검사를 프로그램이 친 글을 읽은 뒤(화면이
        // 바뀐 뒤)에 한다: 부하 아래 300ms(참고 herdr) 안에 글을 읽지 못한 프로그램은 아직 친 글 앞 상태라, 검사 뒤에 그린
        // 확인창이 Enter를 받았다(ci-local m13 race). 바뀌지 않으면 받음 시한까지 기다려 보낸다(시한은 친 때부터 센다).
        // 그 밖의 프로그램(custom·보고만)은 Enter 전에 글을 보이지 않을 수 있어 참고처럼 300ms 뒤에 본다
        let typed = loop {
            let now = self.screen_lines(mid, pane).await;
            if now.as_deref().is_some_and(limit_wait_screen) {
                return Err(after("치는 동안 사용량 한도 대기 화면이 떠 Enter를 보내지 않았다".into()));
            }
            if let Some((_, q)) = now.as_deref().and_then(modal_of) {
                return Err(after(format!("치는 동안 선택 창이 떠 Enter를 보내지 않았다: {q}")));
            }
            if self.pane_agent(pane).is_some_and(|a| a.activity == AgentActivity::Question) {
                return Err(after("치는 동안 확인창이 떠 Enter를 보내지 않았다".into()));
            }
            if text.is_none() || !hooked || now != shown || tokio::time::Instant::now() >= deadline {
                break now;
            }
            crate::text::pane_change(
                &mut changes,
                deadline.min(tokio::time::Instant::now() + Duration::from_millis(100)),
            )
            .await;
        };
        if agy {
            let normalized = |s: &str| s.chars().filter(|ch| !ch.is_whitespace()).collect::<String>();
            let draft = typed.as_ref().and_then(|lines| crate::prompt_box::box_text("agy", &lines.join("\n")));
            if draft.as_deref().map(normalized) != text.map(normalized)
                || !self.pane_agent(pane).is_some_and(|a| a.activity == AgentActivity::Idle)
            {
                return Err(after("agy 입력창이 붙여넣은 본문과 일치하지 않아 Enter를 보내지 않았다".into()));
            }
        }
        if windows_codex && text.is_some() {
            // ConPTY의 실제 Codex는 긴 입력을 key burst로 읽는다. 비문자 키가 버퍼와
            // Enter 억제 상태를 확정한다. 새 입력 끝의 Right는 본문을 바꾸지 않으며 제출하지 않는다.
            let boundary = modes.key("right").unwrap();
            conn.request_watched(
                AgentRequest::Input { session: pane.to_string(), data: B64(boundary) },
                crate::REQUEST_TIMEOUT,
                watch.as_ref(),
                submission.nudge,
            )
            .await
            .map_err(|e| after(e.message))?
            .ok_or_else(|| after("알림을 쓰는 동안 다른 입력이 와 제출하지 않았다".into()))?;
            self.input_event(pane, trace, "paste_boundary", "key=right; enter_count=0");
            let shown = self.screen_lines(mid, pane).await;
            if shown.as_deref().and_then(modal_of).is_some()
                || self.pane_agent(pane).is_some_and(|a| a.activity == AgentActivity::Question)
            {
                return Err(after("붙여넣기 확정 중 확인창이 떠 Enter를 보내지 않았다".into()));
            }
        }
        if let Some(card) = submission.card
            && !self.card_submission_current(card, mid, pane)
        {
            return Err(after("카드 답의 보고나 pane이 바뀌어 Enter를 보내지 않았다".into()));
        }
        let left = || deadline.saturating_duration_since(tokio::time::Instant::now());
        // 받음은 Enter가 일으킨 사건으로만 센다: 보내기 바로 전에 기다린다. 치는 동안 뜬 확인창(질문)도 받음 사건이라
        // 그 전부터 기다리면 Enter를 보내지 않은 글을 받았다고 적는다
        let uses_receipt = busy && probe.is_some();
        let prior_unknown = submission.previous_message == PreviousMessage::Unconfirmed;
        let accepted = (!uses_receipt && !prior_unknown).then(|| self.wait_kind_then(pane, ACCEPTED, hook_accept));
        let enter = modes.key("enter").unwrap_or_else(|| b"\r".to_vec());
        submission.input = MessageInput::EnterUnconfirmed;
        conn.request_watched(
            AgentRequest::Input { session: pane.to_string(), data: B64(enter) },
            crate::REQUEST_TIMEOUT,
            watch.as_ref(),
            submission.nudge,
        )
        .await
        .map_err(|e| after(e.message))?
        .ok_or_else(|| after("알림을 쓰는 동안 다른 입력이 와 제출하지 않았다".into()))?;
        submission.input = MessageInput::Submitted;
        self.input_event(pane, trace, "enter_written", "key=enter bytes=0d enter_count=1");
        let valid = || watch.as_ref().is_none_or(|watch| *watch.borrow());
        if prior_unknown || !valid() {
            return Ok(false);
        }
        if let Some(probe) = probe.as_ref().filter(|_| uses_receipt) {
            while tokio::time::Instant::now() < deadline && valid() {
                if let Some(page) = self.prompt_receipt_page(mid, pane, deadline).await
                    && let Some(method) = probe.observed(&page)
                    && valid()
                {
                    self.input_event(pane, trace, "receipt", &format!("method={method}"));
                    if let Some(f) = screen_accept.take() {
                        f();
                    }
                    return Ok(true);
                }
                if self.pane_agent(pane).is_some_and(|a| a.activity == AgentActivity::Question) {
                    break;
                }
                crate::text::pane_change(
                    &mut changes,
                    deadline.min(tokio::time::Instant::now() + Duration::from_millis(100)),
                )
                .await;
            }
        } else if hooked {
            // Enter 뒤 확인창이면 그 Enter가 창에 갔을 수 있다: 받은 것으로 치지 않는다(t-0340 보안 감사)
            // 이전부터 Working이라는 사실만으로 새 본문이 수락됐다고 보지 않는다.
            let ok = tokio::time::timeout(left(), accepted.expect("영수증 대신 상태 사건을 기다린다"))
                .await
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok_and(|r| {
                    r.as_ref()
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                        })
                        .is_ok()
                });
            let ok = ok && valid();
            if ok && let Some(f) = screen_accept.take() {
                f();
            }
            if ok {
                return Ok(true);
            }
        } else {
            // hook이 없는 harness: Enter 뒤 화면이 바뀌면 받았다고 본다
            while tokio::time::Instant::now() < deadline && valid() {
                if self.screen_lines(mid, pane).await != typed && valid() {
                    if let Some(f) = screen_accept.take() {
                        f();
                    }
                    return Ok(true);
                }
                crate::text::pane_change(
                    &mut changes,
                    deadline.min(tokio::time::Instant::now() + Duration::from_millis(250)),
                )
                .await;
            }
        }
        if submission.previous_message == PreviousMessage::None
            && let (Some(watch), Some(probe)) = (watch.filter(|watch| *watch.borrow()), probe)
        {
            submission.late = Some(LateReceipt { watch, probe, waiting_at });
        }
        Ok(false)
    }

    pub(super) fn input_accepted(&self, pane: &str, waiting_at: Option<u64>) {
        let changed = {
            let mut i = self.inner.lock().unwrap();
            let st = i.panes.get_mut(pane).and_then(|p| p.agent.as_mut());
            if let Some(st) = st
                && waiting_at.is_some()
                && st.progress_at_ms == waiting_at
                && st.progress.as_deref() == Some(crate::orch::WAITING)
            {
                st.progress = None;
                st.progress_at_ms = None;
                st.progress_since_ms = None;
                true
            } else {
                false
            }
        };
        if changed {
            self.bump();
            self.tick_wake.notify_one();
            if let Some(r) = self.thread_of_pane(pane) {
                aaa_common::diagnostic::ignored(
                    self.thread_events.send(self.thread_view(&r)),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
            }
        }
    }

    /// `threads/<id>.task.md`의 `## Follow-ups` 아래에 시각과 함께 덧붙인다(herdr-projects `append_follow_up`)
    pub(super) fn append_follow_up(&self, project: &str, id: &str, text: &str) -> std::io::Result<()> {
        let dir = self.project_dir(project).map_err(|e| std::io::Error::other(e.message))?;
        let path = dir.join("threads").join(format!("{id}.task.md"));
        let _guard = self.follow_up_lock.lock().unwrap();
        let mut task = std::fs::read_to_string(&path)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
            .unwrap_or_default();
        if !task.ends_with('\n') && !task.is_empty() {
            task.push('\n');
        }
        if !task.lines().any(|l| l.trim() == "## Follow-ups") {
            task.push_str("\n## Follow-ups\n");
        }
        task.push_str(&format!(
            "\n### {}\n\n{}\n",
            aaa_common::util::utc_text(aaa_common::util::now_ms()),
            text.trim()
        ));
        crate::orch::write_atomic(&path, task.as_bytes())?;
        // 작업 요약 표지: 요약 뒤 지시가 하나 늘었다(plan §8.259)
        self.summary_follow_ups(project, id, &task);
        drop(_guard);
        if let Ok(r) = self.load(project, id) {
            aaa_common::diagnostic::ignored(
                self.thread_events.send(self.thread_view(&r)),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
        Ok(())
    }

    pub(super) fn save_message(&self, m: &MessageView) {
        let state = match m.state {
            DeliveryState::Queued => "queued",
            DeliveryState::Accepted => "accepted",
            DeliveryState::Unknown => "unknown",
        };
        aaa_common::diagnostic::ignored(
            self.db.rec_put("messages", &m.id, state, &serde_json::to_string(m).unwrap(), (&m.project, &m.thread)),
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
    }

    /// 메시지 API(herdr-projects threads.rs `prompt`): 작업 중이어도 바로 입력한다(harness가 다음 턴으로 줄 세운다).
    /// interrupt는 작업 중이면 Esc로 멈춘 뒤 보낸다: 참고의 `thread stop` 뒤 `thread prompt`를 멈춤까지 기다려 한 번에 한다(aaa).
    pub(crate) async fn thread_message(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        text: String,
        mode: MessageMode,
        actor: &str,
    ) -> Result<MessageView, ErrorBody> {
        self.thread_message_inner(project, id, text, mode, actor, None).await
    }

    #[allow(clippy::too_many_arguments)] // 카드 답도 같은 메시지 입력·수락 경로를 쓴다.
    pub(crate) async fn thread_message_inner(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        text: String,
        mode: MessageMode,
        actor: &str,
        card: Option<CardSend<'_>>,
    ) -> Result<MessageView, ErrorBody> {
        let rec = self.thread_show(project, id)?;
        if rec.status != ThreadStatus::Open {
            return Err(err(ErrorCode::BadRequest, "열린 thread에만 보낸다"));
        }
        if text.trim().is_empty() {
            return Err(err(ErrorCode::BadRequest, "글이 비었다"));
        }
        // brief보다 먼저 가지 않게 한다(herdr-projects threads.rs `prompt`: prompt_pending이면 거부)
        if rec.brief != BriefState::Delivered {
            return Err(err(
                ErrorCode::BadRequest,
                format!(
                    "{id}는 아직 brief를 받지 않았다. 준비되면 aaa가 보내고, 지금 보내려면 `aaa thread brief {project} {id}`"
                ),
            ));
        }
        let pane = rec.pane.clone().ok_or_else(|| err(ErrorCode::BadRequest, "thread 세션이 없다"))?;
        let (pid, mid) = self.resolve_pane(&pane)?;
        self.check_running(&pid)?;
        // 확인창에 친 글은 그 창이 받는다(Enter가 선택지를 고르고, interrupt의 Esc는 「아니오」다): 먼저 답한다.
        // 기록하기 전에 거부한다(herdr-projects threads.rs `prompt_state` agent_blocked)
        let card_screen = if card.is_some() { self.screen(&mid, &pid).await } else { None };
        let limit_wait = card_screen.as_ref().is_some_and(|screen| limit_wait_screen(&screen.lines));
        if !limit_wait && self.pane_agent(&pid).map(|a| a.activity) == Some(AgentActivity::Question) {
            return Err(err(
                ErrorCode::BadRequest,
                format!(
                    "agent_blocked: {id}가 pane({pid})의 확인창을 기다린다. `aaa thread read {project} {id}`로 보고 \
                     `aaa thread keys {project} {id}`(또는 `aaa question answer`)로 먼저 답한다"
                ),
            ));
        }
        // herdr-projects threads.rs `prompt`: 신뢰 창은 Enter가 받아들이고, 사람이 입력창에 쓰던 글은 보낸 글과 섞여
        // 보내진다. 둘 다 입력하지 않고 기록하기 전에 거부한다. 읽을 수 없는 입력창(모르는 harness)은 전처럼 보낸다.
        let kind = self.load(project, id)?.launch.map(|l| l.harness).unwrap_or_default();
        let screen = if card.is_some() { card_screen } else { self.screen(&mid, &pid).await };
        if let Some(screen) = screen.filter(|_| !limit_wait) {
            if let Some(phrase) = crate::trust_screen::detect(&kind, &screen.ansi) {
                let by_user = self.trust_screens(project) == crate::trust_screen::USER;
                return Err(err(
                    ErrorCode::BadRequest,
                    crate::trust_screen::refusal(project, id, &pid, phrase, by_user),
                ));
            }
            if crate::prompt_box::check(&kind, &screen.ansi) == crate::prompt_box::Draft::Typed {
                return Err(err(
                    ErrorCode::BadRequest,
                    format!(
                        "draft_in_box: {id}의 입력창에 누가 쓰던 글이 있다({pid}). 그 글에 섞이지 않게 보내지 않는다. \
                         입력창이 빈 뒤에 다시 보낸다(`aaa pane read {pid}`로 본다)"
                    ),
                ));
            }
        }
        let mut m = MessageView {
            id: short_id("m"),
            project: project.to_string(),
            thread: id.to_string(),
            mode,
            state: DeliveryState::Queued,
            input: Some(MessageInput::NotWritten),
            text,
            created_at_ms: aaa_common::util::now_ms(),
        };
        self.save_message(&m);
        self.db.audit(actor, "thread_message", &format!("{project}/{id} {} {} bytes", m.id, m.text.len()));
        let r = self.load(project, id)?;
        self.flow(
            &r,
            "message_queued",
            None,
            format!("{} {}", m.id, if mode == MessageMode::Interrupt { "interrupt" } else { "queue" }),
        );
        // 보내기는 host 전체에서 하나씩 한다(같은 pane에 두 글이 섞이지 않게). 머신 몇 대 규모에서는 충분하다.
        let _guard = self.deliver_lock.lock().await;
        let activity = self.pane_agent(&pid).map(|a| a.activity);
        match mode {
            // 잠금을 기다리는 동안 확인창이 떴으면 치지 않는다(질문이 열린 채로 둔다)
            _ if card.is_none() && activity == Some(AgentActivity::Question) => {
                m.state = DeliveryState::Unknown;
                self.save_message(&m);
                self.flow(
                    &r,
                    "message_unknown",
                    Some(m.id.clone()),
                    format!("{} 입력 안 함 · 확인창이 떠 입력하지 않았다", m.id),
                );
            }
            MessageMode::Interrupt => {
                if activity == Some(AgentActivity::Working) {
                    let stopped = self.wait_kind(&pid, &["turn_end", "idle"]);
                    let conn = self.live_conn(&mid)?;
                    conn.request(AgentRequest::Input { session: pid.clone(), data: B64(b"\x1b".to_vec()) }).await?;
                    // 멈추면 입력 상자가 보인다: 화면 감지가 idle을 알린다(Claude 「Interrupted」 뒤 `live_prompt_box`).
                    // 멈춘 뒤 다음 입력을 받을 수 있게 되기를 기다리는 상한은 설정 `stop_wait_ms`(기본 15초)다.
                    aaa_common::diagnostic::ignored_nested(
                        tokio::time::timeout(Duration::from_millis(self.cfg.projects.stop_wait_ms), stopped).await,
                        "중단 확인 실패 뒤 기존 전달 판정을 따른다",
                    );
                }
                self.deliver(&mut m, &mid, &pid, card).await?;
            }
            MessageMode::Queue => self.deliver(&mut m, &mid, &pid, card).await?,
        }
        Ok(m)
    }

    pub(crate) async fn deliver(
        self: &Arc<Self>,
        m: &mut MessageView,
        mid: &str,
        pane: &str,
        card: Option<CardSend<'_>>,
    ) -> Result<(), ErrorBody> {
        // 「받았다」는 받았다고 안 그 자리에서 기록한다: 받자마자 뜬 확인창(question_shown)보다 앞에 남게(CI m13)
        let on_accept: Option<OnFire> = self
            .load(&m.project, &m.thread)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .map(|rec| {
                let me = self.clone();
                let id = m.id.clone();
                Box::new(move || me.flow(&rec, "message_accepted", None, id)) as OnFire
            });
        let mut submission = Submission::new(&m.id);
        submission.message = true;
        submission.card = card;
        submission.previous_message = self.db.previous_message(m).unwrap_or_else(|error| {
            tracing::warn!("앞선 메시지 수락 기록을 읽지 못했다: {error:#}");
            PreviousMessage::Unconfirmed
        });
        let r = self.submit_traced(mid, pane, Some(&m.text), on_accept, &mut submission).await.map_err(|e| e.msg);
        if let Some(CardSend::Initial(target, inbox_body)) = card
            && submission.input == MessageInput::NotWritten
        {
            if !self.card_submission_current(CardSend::Initial(target, inbox_body), mid, pane) {
                m.state = DeliveryState::Unknown;
                self.save_message(m);
                return Err(err(ErrorCode::Conflict, "카드 답의 보고나 실행 중인 pane이 바뀌었다"));
            }
            if submission.limit_wait {
                return self.defer_card_answer(m, target, inbox_body);
            }
        }
        if matches!(card, Some(CardSend::Resume(_))) && !submission.resume_claimed {
            if let Err(e) = r {
                tracing::debug!(message = %m.id, "카드 답 전달 대기 유지: {e}");
            }
            return Ok(());
        }
        m.state = if r == Ok(true) { DeliveryState::Accepted } else { DeliveryState::Unknown };
        m.input = Some(submission.input);
        if let Some(CardSend::Resume(pending)) = card {
            self.finish_pending_answer(pending, m.state);
        }
        self.save_message(m);
        // 입력한 글은(받았는지 몰라도) task 기록에 남겨 다시 시작하면 brief와 함께 읽게 한다(herdr-projects
        // thread.rs `append_follow_up`). 입력 전에 거부됐으면(Err) 남기지 않는다.
        if r.as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_ok()
            && !m.text.starts_with("[aaa ")
            && let Err(e) = self.append_follow_up(&m.project, &m.thread, &m.text)
        {
            tracing::warn!(project = %m.project, thread = %m.thread, "Follow-ups를 쓰지 못했다: {e}");
        }
        if m.state != DeliveryState::Accepted
            && let Ok(rec) = self.load(&m.project, &m.thread)
        {
            let why = match r {
                Ok(_) => submission.input.label().to_string(),
                Err(e) => format!("{} · {e}", submission.input.label()),
            };
            self.flow(&rec, "message_unknown", Some(m.id.clone()), format!("{} {why}", m.id));
            if let Some(late) = submission.late {
                let (me, mid, pane, id) = (self.clone(), mid.to_owned(), pane.to_owned(), m.id.clone());
                tokio::spawn(async move {
                    me.watch_late_message(&mid, &pane, &rec, &id, late).await;
                });
            }
        }
        Ok(())
    }

    pub(super) async fn watch_late_message(
        self: &Arc<Self>,
        mid: &str,
        pane: &str,
        rec: &ThreadRec,
        id: &str,
        late: LateReceipt,
    ) {
        let Ok(mut changes) = self.pane_events(pane) else { return };
        let mut chat = self.chat_notes();
        let mut watch = late.watch.subscribe();
        let push = self
            .inner
            .lock()
            .unwrap()
            .machines
            .get(mid)
            .is_some_and(|m| m.features.iter().any(|f| f == "chat_watch.v1"));
        self.chat_watch(pane, true);
        'observe: while *watch.borrow() && late.probe.still_running(self, pane) {
            let deadline = tokio::time::Instant::now() + crate::REQUEST_TIMEOUT;
            let page = tokio::select! {
                biased;
                _ = watch.wait_for(|valid| !*valid) => break,
                page = self.prompt_receipt_page(mid, pane, deadline) => page,
            };
            if let Some(page) = page {
                if !late.probe.same_execution(&page) {
                    break;
                }
                if let Some(method) = late.probe.observed_late(&page)
                    && *watch.borrow()
                    && late.probe.still_running(self, pane)
                {
                    self.input_event(pane, id, "accepted", &format!("method={method}; late=true"));
                    self.input_accepted(pane, late.waiting_at);
                    self.flow(rec, "message_accepted_late", Some(id.to_owned()), id);
                    break;
                }
            }
            loop {
                tokio::select! {
                    biased;
                    _ = watch.wait_for(|valid| !*valid) => break 'observe,
                    event = changes.recv() => match event {
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break 'observe,
                        Ok(crate::PaneEvent::Output(..)) if push => {}
                        _ => break,
                    },
                    note = chat.recv() => match note {
                        Ok(crate::chatwatch::ChatNote::Changed { pane: changed, .. }) if changed == pane => break,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => break,
                        _ => {}
                    }
                }
            }
        }
        self.chat_watch(pane, false);
    }

    /// 옛 host가 턴 끝을 기다리며 대기열에 둔 메시지, 또는 보내던 중 host가 끝난 메시지: 보냈는지 모른다고 남긴다
    /// (다시 보내지 않는다, §8.5 a). 입력을 시작하지 않은 한도 대기 카드 답은 `answer_waiting`에 남는다.
    pub(crate) fn forget_queued_messages(&self) {
        let left = self.db.rec_list("messages", Some("queued")).unwrap_or_default();
        for row in left {
            let parsed = serde_json::from_str::<MessageView>(&row)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok();
            let Some(mut m) = parsed else { continue };
            if let Ok(pending) = serde_json::from_str::<PendingAnswer>(&row) {
                self.finish_pending_answer(&pending, DeliveryState::Unknown);
            }
            m.state = DeliveryState::Unknown;
            // 중간에 멈춘 host에서 실제로 어디까지 썼는지는 응답으로 확인하지 못했다.
            m.input = None;
            self.save_message(&m);
            if let Ok(rec) = self.load(&m.project, &m.thread) {
                self.flow(
                    &rec,
                    "message_unknown",
                    Some(m.id.clone()),
                    format!("{} host가 다시 시작돼 입력 단계를 확인하지 못했다", m.id),
                );
            }
        }
    }
}
