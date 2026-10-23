//! pane 글 API: 읽기(`pane read`), 글·키 보내기(`pane send-text`·`send-keys`·`run`), 출력 기다리기(`pane wait-output`).
//! herdr 0.9.5 `src/app/api/panes.rs`·`src/app/api_helpers.rs`·`src/api/wait.rs`의 규칙을 옮겼다.
//!
//! host는 모든 pane의 화면 사본(mirror)을 갖는다. 읽기와 입력 모드(bracketed paste·DECCKM·modifyOtherKeys)는 이 사본에서 본다.
//! 그래서 session IPC를 바꾸지 않는다(몇 주 도는 옛 session host와도 그대로 맞는다).
//!
//! 참고와 다른 점: herdr `wait-output`은 정해진 간격으로 다시 읽는다(`CONNECTION_POLL_INTERVAL`). aaa는 사본에 출력이 들어올
//! 때(pane 사건)마다 다시 본다. 같은 결과를 시간 간격 없이 낸다.

use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{
    AgentRequest, B64, ErrorBody, ErrorCode, FrontResponse, OutputMatch, ReadFormat, ReadSource, ReadView,
    ThreadPaneView,
};
use tokio::sync::broadcast::{self, error::RecvError};

use crate::{Host, err};

/// herdr agent prompt의 AGENT_PROMPT_SUBMIT_DELAY. Codex가 글·paste 끝을 처리하기 전에 Enter가
/// 도착하면 Enter를 paste 안 줄바꿈으로 삼는다. 웹·thread message·keys가 같은 간격을 쓴다.
pub(crate) async fn submission_gap() {
    let ms = std::env::var("AAA_TEST_SUBMIT_GAP_MS")
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|v| {
            v.parse::<u64>()
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .unwrap_or(300);
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

/// herdr `match_output`(src/api/subscriptions.rs): 맞은 첫 줄
fn match_output(text: &str, matcher: &OutputMatch, regex: Option<&regex::Regex>) -> Option<String> {
    match matcher {
        OutputMatch::Substring { value } => text.lines().find(|l| l.contains(value.as_str())).map(str::to_string),
        OutputMatch::Regex { .. } => regex.and_then(|re| text.lines().find(|l| re.is_match(l)).map(str::to_string)),
    }
}

impl Host {
    /// 웹의 Ctrl+Enter(steering, 줄바꿈 없음). detection과 색을 같은 mirror에서 읽어 회색 suggestion과 초안을 구별한다.
    /// 빈 입력도 현재 작업과 대기 메시지가 확인되면 보낸다. 쉬거나 입력 상태를 모르면 키를 보내지 않는다.
    pub(crate) fn ctrl_enter_data(&self, id: &str, trace: Option<&str>) -> Vec<u8> {
        let mut inner = self.inner.lock().unwrap();
        let Some(pane) = inner.panes.get_mut(id) else {
            return Vec::new();
        };
        let declared = pane.info.harness.is_some()
            || pane.info.argv.first().is_some_and(|arg| {
                matches!(arg.rsplit(['/', '\\']).next().unwrap_or(arg).trim_end_matches(".exe"), "claude" | "codex")
            });
        // 아직 상태 사건이 오지 않은 직접 실행 agent도 판정 모름으로 둔다(보내지 않는다).
        let harness = pane.agent.as_ref().map(|a| a.harness.as_str()).or(declared.then_some("unknown"));
        let blocked = pane.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question);
        // 작업 중에만 steering한다(사용자 10-19 15:1x 정정). 상태 사건이 아직 없으면 모름이라 보내지 않는다
        let working = pane.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Working);
        let Some(mirror) = pane.mirror.as_mut() else {
            return if harness.is_some() { Vec::new() } else { b"\r".to_vec() };
        };
        let colors = mirror.vt.read(ReadSource::Visible, None, ReadFormat::Ansi).text;
        // Codex 커서가 입력 줄에 없으면(여러 줄 편집·목록) 모름이다. 줄 안의 칸은 빈 입력 판정에만 쓴다(글 끝에 있는 것이 보통이다)
        let (codex_row_elsewhere, cursor_home) = if harness == Some("codex") {
            let plain = crate::prompt_box::plain(&colors);
            let lines: Vec<&str> = plain.lines().collect();
            let prompt_row = lines.iter().rposition(|line| line.trim_start().starts_with('›'));
            let (row, col, _) = mirror.vt.cursor();
            (prompt_row != Some(usize::from(row)), col <= 2)
        } else {
            (false, false)
        };
        let live =
            if blocked || !mirror.synced || codex_row_elsewhere { String::new() } else { mirror.vt.detection_text() };
        let keys = crate::ctrl_enter::ctrl_enter_keys(harness, &live, Some(&colors), cursor_home, working);
        let detail = format!(
            "keys={keys:?} working={working} question={blocked} synced={} cursor_home={cursor_home} cursor_elsewhere={codex_row_elsewhere} {}",
            mirror.synced,
            crate::ctrl_enter::diagnostic(harness, &live, Some(&colors))
        );
        let modes = mirror.vt.modes();
        let data = keys.iter().flat_map(|key| modes.key(key).unwrap_or_default()).collect();
        drop(inner);
        let trace = trace.map(str::to_owned).unwrap_or_else(|| format!("ctrl-{}", aaa_common::util::new_id()));
        self.input_event(id, &trace, "ctrl_enter", &detail);
        data
    }

    /// 대기 메시지 「지금 보내기」의 키(plan §8.271). `ctrl_enter_data`와 같은 mirror·상태를 읽되 `queued_steer_keys`의 좁은 판정을 쓴다.
    /// 판정과 까닭은 입력 사건(`steer_queued`)으로 남긴다. 호출한 쪽은 이 결과를 받자마자 쓴다(그 사이 다른 판정·기다림 없음).
    pub(crate) fn steer_queued_data(&self, id: &str, trace: &str) -> Result<Vec<u8>, aaa_proto::SteerRefusal> {
        use aaa_proto::SteerRefusal;
        let mut inner = self.inner.lock().unwrap();
        let (result, detail) = (|| {
            let Some(pane) = inner.panes.get_mut(id) else {
                return (Err(SteerRefusal::Failed), "reason=pane_missing".to_owned());
            };
            let harness = pane.agent.as_ref().map(|a| a.harness.as_str());
            let question = pane.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question);
            let working = pane.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Working);
            if question {
                return (Err(SteerRefusal::Question), format!("harness={harness:?} question=true"));
            }
            // 대화 정리 중(plan §8.309): harness가 정리가 끝날 때까지 대기 글을 읽지 않아 키가 아무 일도 하지 않는다(Claude 2.1.304
            // 대기열은 정리 중 `wait`, 10-20 운영 사건: 누른 뒤 10초 경고만 떴다). t3code도 정리 중 steering을 거절한다
            // (`Orchestrator.ts` 「Wait for context compaction to finish before steering the thread.」)
            if pane.agent.as_ref().is_some_and(|a| a.compacting) {
                return (Err(SteerRefusal::Compacting), format!("harness={harness:?} compacting=true"));
            }
            let Some(mirror) = pane.mirror.as_mut() else {
                return (Err(SteerRefusal::UnknownScreen), format!("harness={harness:?} reason=mirror_missing"));
            };
            let colors = mirror.vt.read(ReadSource::Visible, None, ReadFormat::Ansi).text;
            let (codex_row_elsewhere, cursor_home) = if harness == Some("codex") {
                let plain = crate::prompt_box::plain(&colors);
                let lines: Vec<&str> = plain.lines().collect();
                let prompt_row = lines.iter().rposition(|line| line.trim_start().starts_with('›'));
                let (row, col, _) = mirror.vt.cursor();
                (prompt_row != Some(usize::from(row)), col <= 2)
            } else {
                (false, false)
            };
            let live = if !mirror.synced || codex_row_elsewhere { String::new() } else { mirror.vt.detection_text() };
            let keys = crate::ctrl_enter::queued_steer_keys(harness, &live, Some(&colors), cursor_home, working);
            let modes = mirror.vt.modes();
            let result =
                keys.map(|keys| keys.iter().flat_map(|key| modes.key(key).unwrap_or_default()).collect::<Vec<u8>>());
            let detail = format!(
                "keys={:?} reason={:?} working={working} synced={} cursor_home={cursor_home} cursor_elsewhere={codex_row_elsewhere} {}",
                keys.unwrap_or_default(),
                keys.err(),
                mirror.synced,
                crate::ctrl_enter::diagnostic(harness, &live, Some(&colors))
            );
            match result {
                // 키를 이 pane 모드로 바꾸지 못했다(있을 수 없는 키 이름): 보내지 않는다
                Ok(data) if data.is_empty() => (Err(SteerRefusal::Unsupported), detail),
                other => (other, detail),
            }
        })();
        drop(inner);
        self.input_event(id, trace, "steer_queued", &detail);
        result
    }

    /// 글 제출의 마지막 키. Ctrl+Enter는 같은 보내기 ID에 선택 키와 화면 근거도 남긴다.
    /// `steer`가 아니면 현재 작업 상태와 관계없이 기존 보통 보내기(Enter)를 유지한다.
    pub(crate) fn submit_key_data(&self, pane: &str, steer: bool, trace: Option<&str>) -> Vec<u8> {
        if !steer && trace.is_none() {
            return b"\r".to_vec();
        }
        let mut inner = self.inner.lock().unwrap();
        let (data, detail) = (|| {
            let Some(pane) = inner.panes.get_mut(pane) else {
                return (b"\r".to_vec(), "keys=[\"enter\"] reason=pane_missing".to_owned());
            };
            let harness = pane.agent.as_ref().map(|a| a.harness.as_str());
            let working = pane.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Working);
            let Some(mirror) = pane.mirror.as_mut() else {
                return (b"\r".to_vec(), format!("keys=[\"enter\"] working={working} reason=mirror_missing"));
            };
            let colors = mirror.vt.read(ReadSource::Visible, None, ReadFormat::Ansi).text;
            let live = if mirror.synced { mirror.vt.detection_text() } else { String::new() };
            let keys = if steer {
                crate::ctrl_enter::steer_submit_keys(harness, &live, Some(&colors), working)
            } else {
                &["enter"]
            };
            let modes = mirror.vt.modes();
            let data: Vec<u8> = if steer {
                keys.iter().flat_map(|key| modes.key(key).unwrap_or_default()).collect()
            } else {
                b"\r".to_vec()
            };
            let fallback = data.is_empty();
            let keys = if fallback { &["enter"][..] } else { keys };
            let detail = format!(
                "phase=submit keys={keys:?} steer={steer} working={working} synced={} fallback={fallback} {}",
                mirror.synced,
                crate::ctrl_enter::diagnostic(harness, &live, Some(&colors))
            );
            (if fallback { b"\r".to_vec() } else { data }, detail)
        })();
        drop(inner);
        if let Some(trace) = trace {
            self.input_event(pane, trace, "ctrl_enter", &detail);
        }
        data
    }

    /// 화면 사본에서 읽는다. 사본이 없으면 None.
    pub(crate) fn read_view(
        &self,
        id: &str,
        source: ReadSource,
        lines: Option<u32>,
        format: ReadFormat,
    ) -> Option<ReadView> {
        let mut i = self.inner.lock().unwrap();
        let m = i.panes.get_mut(id)?.mirror.as_mut()?;
        let r = m.vt.read(source, lines, format);
        Some(ReadView { pane: id.to_string(), source, format, text: r.text, truncated: r.truncated })
    }

    /// `pane read`: 사본이 아직 없으면(붙자마자) 그 머신 agent가 보이는 화면을 준다
    pub(crate) async fn pane_read_text(
        self: &Arc<Self>,
        pane: &str,
        source: ReadSource,
        lines: Option<u32>,
        format: ReadFormat,
    ) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        if let Some(v) = self.read_view(&id, source, lines, format) {
            return Ok(FrontResponse::Read(v));
        }
        let conn = self.live_conn(&mid)?;
        match conn.request(AgentRequest::Screen { session: id.clone() }).await? {
            aaa_proto::AgentResponse::Screen(s) => {
                let mut rows = s.lines;
                while rows.last().is_some_and(|l| l.trim().is_empty()) {
                    rows.pop();
                }
                let keep = lines.map(|n| n as usize).unwrap_or(rows.len()).min(rows.len());
                let truncated = keep < rows.len();
                let text = rows[rows.len() - keep..].iter().map(|l| format!("{l}\n")).collect();
                // 사본이 없으니 기록은 못 읽는다: 보이는 화면이라고 밝힌다
                Ok(FrontResponse::Read(ReadView {
                    pane: id,
                    source: ReadSource::Visible,
                    format: ReadFormat::Text,
                    text,
                    truncated,
                }))
            }
            other => Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
        }
    }

    /// thread의 pane(herdr-projects threads.rs `pane_agent`). 끝났거나 pane이 없거나 세션이 끝났으면 거부한다.
    /// 참고는 pane에 agent가 보여야 읽고 쓴다(맨 셸에 치지 않는다). aaa thread pane은 harness 프로세스 자체라(셸 아래에서
    /// 돌지 않는다) 세션이 살아 있으면 그 harness다. hook 없는 harness는 상태가 없어도 읽고 쓴다.
    fn thread_pane(&self, project: &str, id: &str) -> Result<(crate::orch::ThreadRec, String, String), ErrorBody> {
        let rec = self.load(project, id)?;
        let pane = rec
            .pane
            .clone()
            .filter(|_| !matches!(rec.status, aaa_proto::ThreadStatus::Resolved | aaa_proto::ThreadStatus::Parked))
            .ok_or_else(|| {
                if rec.status == aaa_proto::ThreadStatus::Parked {
                    err(
                        ErrorCode::BadRequest,
                        format!("{id}는 보류 중이다. `aaa thread unpark {project} {id}`로 이어 한다"),
                    )
                } else {
                    err(ErrorCode::BadRequest, format!("{id}에 pane이 없다"))
                }
            })?;
        let (pid, mid) = self.resolve_pane(&pane)?;
        if !self.pane_view_of(&pid, aaa_proto::DeviceRole::Local).is_some_and(|v| v.state.is_running()) {
            return Err(err(
                ErrorCode::SessionExited,
                format!("{id}의 세션이 끝났다. `aaa thread restart {project} {id}`가 다시 띄운다"),
            ));
        }
        Ok((rec, pid, mid))
    }

    /// `thread read`: thread pane이 지금 보이는 것(herdr-projects threads.rs `read`). `lines`면 기록 끝 N줄이다.
    pub(crate) async fn thread_read(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        lines: Option<u32>,
    ) -> Result<FrontResponse, ErrorBody> {
        let (_, pid, _) = self.thread_pane(project, id)?;
        let agent = self.pane_agent(&pid).map(|a| a.activity);
        let source = if lines.is_some() { ReadSource::Recent } else { ReadSource::Visible };
        let FrontResponse::Read(view) = self.pane_read_text(&pid, source, lines, ReadFormat::Text).await? else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        Ok(FrontResponse::ThreadPane(ThreadPaneView {
            project: project.to_string(),
            thread: id.to_string(),
            pane: pid,
            agent,
            read: Some(view),
            typed: 0,
            pressed: Vec::new(),
        }))
    }

    /// `thread keys`: 글을 치고(Enter 없음) 키를 누른다(herdr-projects threads.rs `keys`). 아무것도 기록하지 않는다:
    /// 키는 그것이 답한 화면에서만 뜻이 있다. `trust_screens = user`면 신뢰 창에는 사용자만 답한다.
    pub(crate) async fn thread_keys(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        keys: &[String],
        text: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let text = text.filter(|t| !t.is_empty());
        if keys.is_empty() && text.is_none() {
            return Err(err(ErrorCode::BadRequest, "키나 --text가 하나는 있어야 한다"));
        }
        if keys.iter().any(|k| k.trim().is_empty()) {
            return Err(err(ErrorCode::BadRequest, "빈 키 이름이 있다"));
        }
        let (rec, pid, mid) = self.thread_pane(project, id)?;
        let agent = self.pane_agent(&pid).map(|a| a.activity);
        let check_trust = || async {
            if self.trust_screens(project) == crate::trust_screen::USER {
                let kind = rec.launch.as_ref().map(|l| l.harness.clone()).unwrap_or_default();
                let screen = self.screen(&mid, &pid).await.ok_or_else(|| {
                    err(ErrorCode::Unavailable, format!("{id}의 pane 화면을 읽지 못해 보내지 않는다"))
                })?;
                if let Some(phrase) = crate::trust_screen::detect(&kind, &screen.ansi) {
                    return Err(err(
                        ErrorCode::BadRequest,
                        crate::trust_screen::refusal(project, id, &pid, phrase, true),
                    ));
                }
            }
            Ok(())
        };
        check_trust().await?;
        // 모르는 키 때문에 앞부분 글만 입력되는 일을 막는다(herdr encode_api_keys).
        let modes = self.key_modes(&pid);
        for k in keys {
            modes.key(k).ok_or_else(|| err(ErrorCode::BadRequest, format!("모르는 키: {k}")))?;
        }
        let typed = text.as_deref().map_or(0, |t| t.chars().count() as u32);
        if let Some(t) = text {
            if keys.is_empty() {
                self.pane_send_text(&pid, t, actor).await?;
            } else {
                self.pane_input_bytes(&pid, &mid, modes.paste(&t)).await?;
                submission_gap().await;
                // 새 간격 중에 뜬 폴더 신뢰 창을 뒤따르는 Enter로 승인하지 않는다.
                check_trust().await?;
            }
        }
        if !keys.is_empty() {
            self.pane_send_keys(&pid, keys, actor).await?;
        }
        Ok(FrontResponse::ThreadPane(ThreadPaneView {
            project: project.to_string(),
            thread: id.to_string(),
            pane: pid,
            agent,
            read: None,
            typed,
            pressed: keys.to_vec(),
        }))
    }

    /// pane 프로그램이 켠 입력 모드(mirror에서 본다). mirror가 아직 없으면 모두 꺼짐.
    pub(crate) fn key_modes(&self, id: &str) -> aaa_vt::KeyModes {
        let i = self.inner.lock().unwrap();
        i.panes.get(id).and_then(|p| p.mirror.as_ref()).map(|m| m.vt.modes()).unwrap_or_default()
    }

    async fn pane_input_bytes(self: &Arc<Self>, id: &str, mid: &str, data: Vec<u8>) -> Result<(), ErrorBody> {
        let conn = self.live_conn(mid)?;
        conn.request(AgentRequest::Input { session: id.to_string(), data: B64(data) }).await.map(|_| ())
    }

    /// `pane send-text`: 그대로(감싸지 않는다, herdr `handle_pane_send_text`)
    pub(crate) async fn pane_send_text(
        self: &Arc<Self>,
        pane: &str,
        text: String,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        self.check_running(&id)?;
        self.db.audit(actor, "pane_send_text", &format!("{id} {} bytes", text.len()));
        self.pane_input_bytes(&id, &mid, text.into_bytes()).await.map(|_| FrontResponse::Done)
    }

    /// `pane send-keys`: 모두 해석한 뒤 키마다 따로 쓴다(herdr `handle_pane_send_keys`). 모르는 키가 있으면 아무것도 보내지 않는다.
    pub(crate) async fn pane_send_keys(
        self: &Arc<Self>,
        pane: &str,
        keys: &[String],
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        self.check_running(&id)?;
        if keys.is_empty() {
            return Err(err(ErrorCode::BadRequest, "보낼 키가 없다"));
        }
        let modes = self.key_modes(&id);
        let mut encoded = Vec::with_capacity(keys.len());
        for k in keys {
            encoded.push(modes.key(k).ok_or_else(|| err(ErrorCode::BadRequest, format!("모르는 키: {k}")))?);
        }
        self.db.audit(actor, "pane_send_keys", &format!("{id} {}", keys.join(" ")));
        for bytes in encoded.into_iter().filter(|b| !b.is_empty()) {
            self.pane_input_bytes(&id, &mid, bytes).await?;
        }
        Ok(FrontResponse::Done)
    }

    /// `pane run`: 글 + Enter를 한 번에 쓴다. 프로그램이 bracketed paste를 켰으면 글을 감싼다(herdr `encode_api_input`).
    pub(crate) async fn pane_submit(
        self: &Arc<Self>,
        pane: &str,
        text: &str,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        self.check_running(&id)?;
        let modes = self.key_modes(&id);
        let mut bytes = if text.is_empty() { Vec::new() } else { modes.paste(text) };
        bytes.extend(modes.key("enter").unwrap_or_else(|| b"\r".to_vec()));
        self.db.audit(actor, "pane_run", &format!("{id} {} bytes", text.len()));
        self.pane_input_bytes(&id, &mid, bytes).await.map(|_| FrontResponse::Done)
    }

    /// pane 사건 구독. 요청을 보내기 전에 잡아 그 사이 사건을 놓치지 않는다.
    pub(crate) fn pane_events(&self, id: &str) -> Result<broadcast::Receiver<crate::PaneEvent>, ErrorBody> {
        let i = self.inner.lock().unwrap();
        Ok(i.panes.get(id).ok_or_else(|| err(ErrorCode::NotFound, format!("pane이 없다: {id}")))?.events.subscribe())
    }

    /// pane 상태가 `done`이 될 때까지 사건마다 다시 본다(시한 `REQUEST_TIMEOUT`). pane이 사라져도 끝낸다.
    pub(crate) async fn wait_pane(
        &self,
        id: &str,
        mut rx: broadcast::Receiver<crate::PaneEvent>,
        done: impl Fn(&crate::PaneRt) -> bool,
        timeout_msg: &str,
    ) -> Result<(), ErrorBody> {
        let reached = async {
            loop {
                if self.inner.lock().unwrap().panes.get(id).is_none_or(&done) {
                    return;
                }
                if let Err(RecvError::Closed) = rx.recv().await {
                    return;
                }
            }
        };
        tokio::time::timeout(crate::REQUEST_TIMEOUT, reached)
            .await
            .map_err(|source_error| aaa_common::diagnostic::context(source_error, err(ErrorCode::Timeout, timeout_msg)))
    }

    /// `pane close`(herdr `close_pane`): 세션을 끝내고, 끝났다는 사건이 오면 layout과 기록에서 지운다.
    /// agent가 끊겨 있으면 끝낼 수 없다(읽기 전용). 이미 끝난 pane은 바로 지운다.
    pub(crate) async fn pane_close(self: &Arc<Self>, pane: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        let conn = self.live_conn(&mid)?;
        let rx = self.pane_events(&id)?;
        if self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.info.state.is_running()) {
            self.db.audit(actor, "pane_close", &id);
            self.kill_session(&conn, &id).await?;
            // 끝났다는 상태가 올 때까지(agent가 session host 종료를 알린다)
            self.wait_pane(&id, rx, |p| !p.info.state.is_running(), "끝내기를 보냈지만 세션이 끝나지 않았다").await?;
        }
        self.forget(&id, &mid, actor).await?;
        Ok(FrontResponse::Done)
    }

    /// `pane wait-output`: 맞을 때까지, 출력이 사본에 들어올 때마다 다시 본다. pane이 끝나도 시한까지는 기다린다(herdr와 같다).
    pub(crate) async fn pane_wait_output(
        self: &Arc<Self>,
        pane: &str,
        matcher: OutputMatch,
        source: ReadSource,
        lines: Option<u32>,
        timeout_ms: Option<u64>,
    ) -> Result<FrontResponse, ErrorBody> {
        let regex = match &matcher {
            OutputMatch::Regex { value } => Some(
                regex::Regex::new(value).map_err(|e| err(ErrorCode::BadRequest, format!("정규식이 틀렸다: {e}")))?,
            ),
            OutputMatch::Substring { .. } => None,
        };
        let (id, _) = self.resolve_pane(pane)?;
        // 사건 구독을 먼저 잡고 읽는다: 그 사이에 온 출력을 놓치지 않는다
        let mut rx = self.pane_events(&id)?;
        let deadline = timeout_ms.map(|ms| tokio::time::Instant::now() + Duration::from_millis(ms));
        loop {
            if let Some(read) = self.read_view(&id, source, lines, ReadFormat::Text)
                && let Some(matched_line) = match_output(&read.text, &matcher, regex.as_ref())
            {
                return Ok(FrontResponse::OutputMatched { matched_line, read });
            }
            let next = rx.recv();
            let got = match deadline {
                Some(d) => match tokio::time::timeout_at(d, next).await {
                    Ok(r) => r,
                    Err(source_error) => {
                        aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                        return Err(err(ErrorCode::Timeout, "기다린 글이 나오지 않았다(시한 지남)"));
                    }
                },
                None => next.await,
            };
            match got {
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return Err(err(ErrorCode::NotFound, format!("pane이 사라졌다: {pane}"))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_output_returns_the_first_matching_line() {
        let text = "a ready\nb done\nc ready\n";
        let sub = OutputMatch::Substring { value: "ready".into() };
        assert_eq!(match_output(text, &sub, None).as_deref(), Some("a ready"));
        let re = OutputMatch::Regex { value: "^b".into() };
        let r = regex::Regex::new("^b").unwrap();
        assert_eq!(match_output(text, &re, Some(&r)).as_deref(), Some("b done"));
        assert_eq!(match_output(text, &OutputMatch::Substring { value: "zzz".into() }, None), None);
    }
}
