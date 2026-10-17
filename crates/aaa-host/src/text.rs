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

/// herdr `match_output`(src/api/subscriptions.rs): 맞은 첫 줄
fn match_output(text: &str, matcher: &OutputMatch, regex: Option<&regex::Regex>) -> Option<String> {
    match matcher {
        OutputMatch::Substring { value } => text.lines().find(|l| l.contains(value.as_str())).map(str::to_string),
        OutputMatch::Regex { .. } => regex.and_then(|re| text.lines().find(|l| re.is_match(l)).map(str::to_string)),
    }
}

impl Host {
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
            .filter(|_| rec.status != aaa_proto::ThreadStatus::Resolved)
            .ok_or_else(|| err(ErrorCode::BadRequest, format!("{id}에 pane이 없다")))?;
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
        if self.trust_screens(project) == crate::trust_screen::USER {
            let kind = rec.launch.as_ref().map(|l| l.harness.clone()).unwrap_or_default();
            let screen = self
                .screen(&mid, &pid)
                .await
                .ok_or_else(|| err(ErrorCode::Unavailable, format!("{id}의 pane 화면을 읽지 못해 보내지 않는다")))?;
            if let Some(phrase) = crate::trust_screen::detect(&kind, &screen.ansi) {
                return Err(err(ErrorCode::BadRequest, crate::trust_screen::refusal(project, id, &pid, phrase, true)));
            }
        }
        let typed = text.as_deref().map_or(0, |t| t.chars().count() as u32);
        if let Some(t) = text {
            self.pane_send_text(&pid, t, actor).await?;
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
        tokio::time::timeout(crate::REQUEST_TIMEOUT, reached).await.map_err(|_| err(ErrorCode::Timeout, timeout_msg))
    }

    /// `pane close`(herdr `close_pane`): 세션을 끝내고, 끝났다는 사건이 오면 layout과 기록에서 지운다.
    /// agent가 끊겨 있으면 끝낼 수 없다(읽기 전용). 이미 끝난 pane은 바로 지운다.
    pub(crate) async fn pane_close(self: &Arc<Self>, pane: &str, actor: &str) -> Result<FrontResponse, ErrorBody> {
        let (id, mid) = self.resolve_pane(pane)?;
        let conn = self.live_conn(&mid)?;
        let rx = self.pane_events(&id)?;
        if self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.info.state.is_running()) {
            self.db.audit(actor, "pane_close", &id);
            conn.request(AgentRequest::Kill { session: id.clone() }).await?;
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
                    Err(_) => return Err(err(ErrorCode::Timeout, "기다린 글이 나오지 않았다(시한 지남)")),
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
