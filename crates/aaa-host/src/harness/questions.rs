//! pane 질문: 열기·닫기·답하기와 메뉴 읽기.

use super::*;

impl Host {
    // ───────────── 질문 ─────────────

    pub(crate) fn open_question_of(&self, pane: &str) -> Option<QuestionView> {
        self.questions(false).into_iter().find(|q| q.pane == pane)
    }

    /// 그 pane의 열린 질문을 모두 닫는다(Gone). 확인창이 사라졌거나, 세션이 끝났거나 pane이 없어졌을 때다.
    /// 참고 herdr-web-ui는 질문을 기록하지 않고 살아 있는 blocked pane 화면에서 읽어(`server/prompt.ts`), pane이 끝나거나
    /// 닫히면(`server/collector.ts` `parseStructureFrame`의 pane_exited·pane_closed) 질문도 없다. aaa는 질문을 기록하므로
    /// 그때 닫는다(QA t-0337 Q4, plan §8.110).
    pub(crate) fn close_questions_of(&self, pane: &str) {
        for mut q in self.questions(false).into_iter().filter(|q| q.pane == pane) {
            q.state = QuestionState::Gone;
            self.save_question(&q);
        }
    }

    pub(crate) fn save_question(&self, q: &QuestionView) {
        let _order = self.question_lock.lock().unwrap();
        self.save_question_locked(q);
    }

    pub(super) fn save_question_locked(&self, q: &QuestionView) {
        if q.state == QuestionState::Answered
            && let Some(p) = self.inner.lock().unwrap().panes.get_mut(&q.pane)
        {
            p.answered = Some((q.question_kind.clone(), q.text.clone()));
        }
        let state = match q.state {
            QuestionState::Open => "open",
            QuestionState::Answered => "answered",
            QuestionState::Gone => "gone",
        };
        if let Err(e) = self.db.rec_put("questions", &q.id, state, &serde_json::to_string(q).unwrap(), (&q.pane, "")) {
            tracing::error!("질문 저장 실패: {e:#}");
        }
        let mut view = q.clone();
        let project = view
            .project
            .clone()
            .or_else(|| {
                self.sole_project()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .flatten()
            })
            .unwrap_or_default();
        view.short_id = self
            .db
            .question_id(&project, "harness", &q.id, "", 0)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok();
        aaa_common::diagnostic::ignored(self.question_events.send(view), "수신자가 끝난 뒤에도 현재 작업은 마친다");
    }

    /// 질문을 연다. 같은 pane에 이미 열린 질문이 있으면 본문만 더 자세한 쪽으로 바꾼다.
    pub(crate) fn ask(self: &Arc<Self>, pane: &str, kind: &str, text: String) -> QuestionView {
        if let Some(mut q) = self.open_question_of(pane) {
            if text.chars().count() > q.text.chars().count() {
                q.text = text;
                self.save_question(&q);
            }
            return q;
        }
        let (machine, mid) = {
            let i = self.inner.lock().unwrap();
            let mid = i.panes.get(pane).map(|p| p.machine_id.clone()).unwrap_or_default();
            (i.machines.get(&mid).map(|m| m.name.clone()).unwrap_or_default(), mid)
        };
        let _ = mid;
        let rec = self.thread_of_pane(pane);
        let q = QuestionView {
            short_id: None,
            id: short_id("q"),
            pane: pane.to_string(),
            machine,
            project: rec.as_ref().map(|r| r.project.clone()),
            thread: rec.as_ref().map(|r| r.id.clone()),
            question_kind: kind.to_string(),
            text,
            options: vec!["yes".into(), "no".into()],
            state: QuestionState::Open,
            answer: None,
            created_at_ms: aaa_common::util::now_ms(),
            menu: None,
        };
        self.save_question(&q);
        if let Some(r) = &rec {
            self.flow(r, "question_shown", None, format!("{} {}: {}", q.id, q.question_kind, q.text));
        }
        q
    }

    pub(crate) fn questions(&self, all: bool) -> Vec<QuestionView> {
        self.db
            .rec_list("questions", if all { None } else { Some("open") })
            .unwrap_or_default()
            .iter()
            .filter_map(|j| {
                serde_json::from_str(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .collect()
    }

    /// 답: 질문이 아직 열려 있고 세션이 살아 있는지 다시 본 뒤 키로 보낸다
    pub(crate) async fn answer(
        self: &Arc<Self>,
        id: &str,
        choice: &str,
        actor: &str,
    ) -> Result<QuestionView, ErrorBody> {
        let mut q = self
            .questions(true)
            .into_iter()
            .find(|q| q.id == id)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("질문이 없다: {id}")))?;
        if q.state != QuestionState::Open {
            return Err(err(ErrorCode::BadRequest, format!("이미 닫힌 질문이다({:?})", q.state)));
        }
        if !q.options.iter().any(|o| o == choice) {
            return Err(err(ErrorCode::BadRequest, format!("고를 수 있는 답: {}", q.options.join(", "))));
        }
        let (pid, mid) = self.resolve_pane(&q.pane)?;
        if let Err(e) = self.check_running(&pid) {
            q.state = QuestionState::Gone;
            self.save_question(&q);
            return Err(e);
        }
        self.refuse_trust_screen(&q, &mid, &pid, actor).await?;
        // hook 사건이 화면보다 먼저 올 수 있다: 선택 창이 그려지기를 잠깐 기다린다
        let until = Instant::now() + MENU_WAIT;
        let keys = loop {
            let lines = self.screen_lines(&mid, &pid).await.unwrap_or_default();
            match answer_keys(&lines, choice) {
                Ok(k) => break k,
                Err(e) if Instant::now() >= until => {
                    return Err(err(ErrorCode::BadRequest, format!("{e}. 터미널에서 직접 답한다")));
                }
                Err(source_error) => {
                    aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                    tokio::time::sleep(Duration::from_millis(200)).await
                }
            }
        };
        let conn = self.live_conn(&mid)?;
        conn.request(AgentRequest::Input { session: pid.clone(), data: B64(keys) }).await?;
        // 답한 뒤 상태(도구가 돈다·입력을 기다린다)는 화면 감지가 알린다
        q.state = QuestionState::Answered;
        q.answer = Some(choice.to_string());
        self.save_question(&q);
        self.db.audit(actor, "question_answer", &format!("{} {choice}", q.id));
        if let Some(r) = self.thread_of_pane(&q.pane) {
            self.flow(&r, "question_answered", None, format!("{} → {choice}", q.id));
        }
        Ok(q)
    }

    /// `trust_screens = user`: aaa 세션 안 CLI(coordinator)는 thread pane의 신뢰 창에 답하지 않는다(`thread keys`와 같다).
    /// 사람은 터미널·웹에서 답한다. 예·아니오 답과 화면 선택 창 답이 같은 문을 지난다
    pub(super) async fn refuse_trust_screen(
        &self,
        q: &QuestionView,
        mid: &str,
        pid: &str,
        actor: &str,
    ) -> Result<(), ErrorBody> {
        if crate::front::in_session(actor)
            && let Some(r) = self.thread_of_pane(&q.pane)
            && self.trust_screens(&r.project) == crate::trust_screen::USER
        {
            let kind = r.launch.as_ref().map(|l| l.harness.clone()).unwrap_or_default();
            let screen = self.screen(mid, pid).await.ok_or_else(|| {
                err(ErrorCode::Unavailable, format!("{}의 pane 화면을 읽지 못해 답하지 않는다", r.id))
            })?;
            if let Some(phrase) = crate::trust_screen::detect(&kind, &screen.ansi) {
                return Err(err(
                    ErrorCode::BadRequest,
                    crate::trust_screen::refusal(&r.project, &r.id, pid, phrase, true),
                ));
            }
        }
        Ok(())
    }

    /// 열린 질문에 화면 선택 창을 붙인다(plan §8.172). hook 사건이 화면보다 먼저 올 수 있어 `MENU_WAIT`까지 다시 본다.
    /// 못 읽으면(모르는 창) 그대로 두고 예·아니오로 답한다
    pub(super) fn read_menu(self: &Arc<Self>, id: String, pane: String) {
        let h = self.clone();
        tokio::spawn(async move {
            let Ok((pid, mid)) = h.resolve_pane(&pane).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                return;
            };
            let until = Instant::now() + MENU_WAIT;
            loop {
                let lines = h.screen_lines(&mid, &pid).await.unwrap_or_default();
                if let Some(p) = crate::prompt::parse(&lines) {
                    h.set_menu(&id, Some(p.menu));
                    return;
                }
                if Instant::now() >= until {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
    }

    /// 아직 열린 질문의 선택 창을 바꾼다(그 사이 답했거나 닫혔으면 그대로)
    pub(super) fn set_menu(&self, id: &str, menu: Option<QuestionMenu>) {
        let _order = self.question_lock.lock().unwrap();
        if let Some(mut q) = self.questions(false).into_iter().find(|q| q.id == id)
            && q.menu != menu
        {
            q.menu = menu;
            self.save_question_locked(&q);
        }
    }

    /// 화면 선택 창에 답한다(참고 herdr-web-ui `handlePromptRequest`). 화면을 다시 읽어 카드가 본 그 창(같은 id)일 때만
    /// 키를 보낸다. 옮기기는 되돌릴 수 있지만 처음 저지르는 키(Enter·글·체크)는 화면이 그 창과 옮긴 줄의 커서를 다시
    /// 보인 뒤에 보낸다: 그 사이 터미널에서 답해 다른 창이 떴으면 그 창이 이 Enter를 받는다. 그 뒤 키는 창이 바뀌는
    /// 중이라 보지 않고 보낸다
    pub(crate) async fn answer_menu(
        self: &Arc<Self>,
        id: &str,
        menu_id: &str,
        pick: MenuPick,
        actor: &str,
    ) -> Result<QuestionView, ErrorBody> {
        let mut q = self
            .questions(true)
            .into_iter()
            .find(|q| q.id == id)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("질문이 없다: {id}")))?;
        if q.state != QuestionState::Open {
            return Err(err(ErrorCode::BadRequest, format!("이미 닫힌 질문이다({:?})", q.state)));
        }
        let (pid, mid) = self.resolve_pane(&q.pane)?;
        if let Err(e) = self.check_running(&pid) {
            q.state = QuestionState::Gone;
            self.save_question(&q);
            return Err(e);
        }
        self.refuse_trust_screen(&q, &mid, &pid, actor).await?;
        let changed = || err(ErrorCode::BadRequest, "화면의 질문이 바뀌었다. 새 질문을 보고 다시 고른다");
        let shown = crate::prompt::parse(&self.screen_lines(&mid, &pid).await.unwrap_or_default());
        let Some(p) = shown.filter(|p| p.menu.id == menu_id) else {
            // 카드가 낡았다: 지금 화면의 창(없으면 예·아니오)으로 바꿔 둔다
            let now = crate::prompt::parse(&self.screen_lines(&mid, &pid).await.unwrap_or_default());
            self.set_menu(id, now.map(|p| p.menu));
            return Err(changed());
        };
        let steps = crate::prompt::answer_steps(&p, &pick).map_err(|e| err(ErrorCode::BadRequest, e))?;
        let conn = self.live_conn(&mid)?;
        let (mut cursor, mut moved, mut committed) = (p.selected as isize, false, false);
        for (n, step) in steps.iter().enumerate() {
            let walk = match step {
                crate::prompt::Step::Down => Some(1),
                crate::prompt::Step::Up => Some(-1),
                _ => None,
            };
            if walk.is_none() && !committed && moved && !self.on_row(&mid, &pid, menu_id, cursor).await {
                return Err(changed());
            }
            conn.request(AgentRequest::Input { session: pid.clone(), data: B64(step.bytes()) }).await?;
            match walk {
                Some(d) => {
                    moved = true;
                    cursor = (cursor + d).max(0);
                }
                None => committed = true,
            }
            if n + 1 < steps.len() {
                tokio::time::sleep(STEP_GAP).await;
            }
        }
        let said = crate::prompt::describe(&p.menu, &pick);
        {
            let _order = self.question_lock.lock().unwrap();
            q.state = QuestionState::Answered;
            q.answer = Some(said.clone());
            q.menu = Some(p.menu);
            self.save_question_locked(&q);
        }
        self.db.audit(actor, "question_answer", &format!("{} {said}", q.id));
        if let Some(r) = self.thread_of_pane(&q.pane) {
            self.flow(&r, "question_answered", None, format!("{} → {said}", q.id));
        }
        Ok(q)
    }

    /// 화면이 그 창(같은 id)을 커서가 `row`에 있는 채로 보일 때까지 기다린다(`SETTLE`까지)
    pub(super) async fn on_row(&self, mid: &str, pid: &str, menu_id: &str, row: isize) -> bool {
        let until = Instant::now() + SETTLE;
        loop {
            let lines = self.screen_lines(mid, pid).await.unwrap_or_default();
            if crate::prompt::parse(&lines).is_some_and(|p| p.menu.id == menu_id && p.selected as isize == row) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
