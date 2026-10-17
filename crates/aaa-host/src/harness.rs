//! harness 상태·확인창 질문·메시지 API(M1-3b, §8.3~8.5).
//!
//! - agent가 보낸 사건(`HarnessEvent`)으로 pane별 상태를 바꾸고 front에 알린다. polling하지 않는다.
//!   상태(working·idle·question)는 agent의 화면 감지가 낸다(herdr 0.9.5, plan §8.110). hook 사건은 harness 세션 ID와
//!   진행 보고 정리에만 쓴다. 옛 agent의 hook 상태 사건(turn_start·turn_end 등)도 그대로 받는다.
//! - 질문은 한 pane에 열린 것을 하나로 묶는다(brief 안내 중 화면에서 연 질문과 화면 감지 질문이 같은 창이다).
//!   답은 질문이 아직 열려 있고 세션이 살아 있는지 다시 본 뒤 adapter 키(기본 선택 Enter, 거절 Esc)로 보낸다.
//! - 입력(brief 안내·메시지)은 harness가 작업 중이나 질문으로 보이면 accepted다(herdr-projects `agent_prompt_confirmed`:
//!   `--until working --until blocked`). 상태를 모르는 harness는 화면 변화로 본다.
//!   일하는 중인 harness에 보낸 메시지는 harness가 다음 턴으로 줄 세우므로, 보낸 뒤에도 일하는 중이면 accepted다.
//!   확인하지 못하면 unknown으로 남기고 **자동으로 다시 보내지 않는다**(§8.5 a).

use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{
    AgentActivity, AgentRequest, AgentStatus, B64, BriefState, DeliveryState, ErrorBody, ErrorCode, HarnessEvent,
    MessageMode, MessageView, QuestionState, QuestionView, ThreadStatus,
};
use tokio::sync::oneshot;

use crate::orch::ThreadRec;
use crate::{Host, err};

/// 질문에 답할 때 선택 창이 화면에 그려지기를 기다리는 시간
const MENU_WAIT: Duration = Duration::from_secs(3);
/// 입력을 받았다는 사건: 화면 감지의 작업 중·질문(herdr-projects `--until working --until blocked`), 옛 agent의 turn_start
const ACCEPTED: &[&str] = &["working", "question", "turn_start"];

/// 아직 모르는 pane의 harness 사건을 이만큼 보관한다
const HOLD_FOR: Duration = Duration::from_secs(60);
const HOLD_MAX: usize = 32;

/// pane 등록 전에 온 harness 사건(세션 ID → (받은 시각, 머신, 사건))
pub(crate) type Held = std::collections::HashMap<String, Vec<(Instant, String, HarnessEvent)>>;

/// 사건 kind를 기다리는 대기자(pane별)
/// 사건이 오면 hook 처리 안에서 바로 할 일(기다리던 쪽이 깨어나기 전에, 다음 사건보다 먼저)
pub(crate) type OnFire = Box<dyn FnOnce() + Send>;
pub(crate) type Waiters =
    std::collections::HashMap<String, Vec<(&'static [&'static str], oneshot::Sender<String>, Option<OnFire>)>>;

/// 선택 창이 떠 있다는 표지(Claude Code·Codex). 이 화면에는 안내·메시지를 입력하지 않는다.
/// Codex 0.163 명령 승인 창의 맨 아래 줄은 「Press enter to confirm or esc to cancel」이다(herdr codex.toml
/// `live_strong_blocker`가 보는 글과 같다).
const MODAL_MARKS: &[&str] = &[
    "Enter to confirm",
    "Esc to cancel",
    "Press enter to confirm or esc to cancel",
    "Do you trust",
    "trust this folder",
    "Allow Codex to work in this folder",
    "Yes, proceed",
];

/// 선택 창 표지는 화면 맨 아래 몇 줄(빈 줄 제외) 안에 있어야 한다. 위쪽 출력(예: 소스를 cat한 결과)에 같은 글이
/// 있어도 선택 창으로 보지 않는다. Claude Code 선택 창은 맨 아래에 그려지고 그동안 상태 줄이 없다.
const MARK_ROWS: usize = 3;
/// 선택 창 본문·선택지를 찾는 범위(표지 줄에서 위로)
const DIALOG_ROWS: usize = 24;

/// 화면 맨 아래 선택 창 영역(표지 줄까지). 없으면 None.
fn dialog(lines: &[String]) -> Option<&[String]> {
    let filled: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| !l.trim().is_empty()).map(|(i, _)| i).collect();
    let mark =
        filled.iter().rev().take(MARK_ROWS).copied().find(|&i| MODAL_MARKS.iter().any(|m| lines[i].contains(m)))?;
    Some(&lines[mark.saturating_sub(DIALOG_ROWS)..=mark])
}

/// 화면에 선택 창이 있으면 (질문 종류, 본문)
pub(crate) fn modal_of(lines: &[String]) -> Option<(&'static str, String)> {
    let d = dialog(lines)?;
    let all = d.join(" ");
    let kind = if all.contains("trust") || all.contains("Trust") { "trust" } else { "confirm" };
    // 본문: 선택지에 가장 가까운 물음표 줄(없으면 표지 줄)
    let text = d
        .iter()
        .rev()
        .map(|l| l.trim())
        .find(|l| l.contains('?'))
        .unwrap_or_else(|| d.last().map(|l| l.trim()).unwrap_or_default());
    Some((kind, text.chars().take(300).collect()))
}

/// 화면 선택 창의 선택지: (선택됨, 글)
fn menu(lines: &[String]) -> Vec<(bool, String)> {
    let mut groups: Vec<Vec<(bool, String)>> = Vec::new();
    let mut prev: Option<usize> = None;
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        let (sel, rest) = match ['❯', '›', '>'].iter().find_map(|c| t.strip_prefix(*c)) {
            Some(r) => (true, r.trim_start()),
            None => (false, t),
        };
        // `1. Yes` 같은 번호를 뗀다
        let rest = match rest.split_once(". ") {
            Some((n, r)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => r,
            _ => rest,
        };
        if !(rest.starts_with("Yes") || rest.starts_with("No")) {
            continue;
        }
        if prev != Some(i.wrapping_sub(1)) {
            groups.push(Vec::new());
        }
        groups.last_mut().unwrap().push((sel, rest.trim_end().to_string()));
        prev = Some(i);
    }
    groups.into_iter().rev().find(|g| g.iter().any(|(s, _)| *s)).unwrap_or_default()
}

/// 질문 답 → harness 키(adapter). 「아니오」는 취소(Esc)다. 「예」는 화면 선택 창에서 Yes로 시작하는 첫 선택지까지
/// 화살표로 옮긴 뒤 Enter다(Claude Code 폴더 신뢰 창은 기본 선택이 「No, exit」라 Enter만 보내면 안 된다).
pub(crate) fn answer_keys(lines: &[String], choice: &str) -> Result<Vec<u8>, String> {
    match choice {
        "no" => return Ok(b"\x1b".to_vec()),
        "yes" => {}
        other => return Err(format!("모르는 답: {other}")),
    }
    let m = dialog(lines).map(menu).unwrap_or_default();
    let cur = m.iter().position(|(s, _)| *s).ok_or("화면에서 선택 창을 찾지 못했다")?;
    let want = m.iter().position(|(_, t)| t.starts_with("Yes")).ok_or("화면 선택 창에 Yes 선택지가 없다")?;
    let step: &[u8] = if want > cur { b"\x1b[B" } else { b"\x1b[A" };
    let mut keys = step.repeat(want.abs_diff(cur));
    keys.push(b'\r');
    Ok(keys)
}

/// 입력이 실패했다. `typed`면 글을 이미 쳤을 수 있다(다시 치기 전에 화면을 본다).
#[derive(Debug)]
pub(crate) struct SubmitErr {
    pub typed: bool,
    pub msg: String,
}

/// 사건이 바꿀 활동(없으면 그대로).
/// - harness 세션 ID(SessionStart·Codex notify)는 상태를 바꾸지 않는다(compact·clear 뒤에도 화면 감지가 낸 상태가
///   맞다). 처음 보는 pane만 「시작」으로 둔다.
/// - 지금 상태가 된 때보다 오래된 사건은 바꾸지 않는다. agent가 다시 붙을 때 보내는 기억(agent `set_host`)이 그 사이
///   화면 감지가 낸 새 상태보다 늦게 올 수 있다(사건 때는 모두 그 pane agent의 시계다, plan §8.110).
fn next_activity(prev: Option<&AgentStatus>, ev: &HarnessEvent) -> Option<AgentActivity> {
    if prev.is_some_and(|a| ev.at_ms < a.since_ms) {
        return None;
    }
    match ev.kind.as_str() {
        "session_start" | "session" => prev.is_none().then_some(AgentActivity::Started),
        "turn_start" | "working" => Some(AgentActivity::Working),
        "turn_end" | "idle" => Some(AgentActivity::Idle),
        "question" => Some(AgentActivity::Question),
        "error" => Some(AgentActivity::Error),
        "exited" => Some(AgentActivity::Exited),
        _ => None,
    }
}

impl Host {
    /// 이 pane의 다음 사건 중 `kinds`에 드는 것을 기다린다
    fn wait_kind(&self, pane: &str, kinds: &'static [&'static str]) -> oneshot::Receiver<String> {
        self.wait_kind_then(pane, kinds, None)
    }

    /// `on_fire`는 그 사건을 처리하는 자리에서 바로 돈다: 기다리던 task가 깨어나 기록하면 그 사이 처리된 다음 사건
    /// (예: 메시지를 받자마자 뜬 확인창)보다 기록이 늦을 수 있다
    fn wait_kind_then(
        &self,
        pane: &str,
        kinds: &'static [&'static str],
        on_fire: Option<OnFire>,
    ) -> oneshot::Receiver<String> {
        let (tx, rx) = oneshot::channel();
        self.waiters.lock().unwrap().entry(pane.to_string()).or_default().push((kinds, tx, on_fire));
        rx
    }

    fn hold_harness(&self, mid: &str, ev: HarnessEvent) {
        let mut h = self.held.lock().unwrap();
        h.retain(|_, v| {
            v.retain(|(at, _, _)| at.elapsed() < HOLD_FOR);
            !v.is_empty()
        });
        let v = h.entry(ev.session.clone()).or_default();
        if v.len() < HOLD_MAX {
            v.push((Instant::now(), mid.to_string(), ev));
        }
    }

    /// pane이 등록됐다: 그 전에 온 harness 사건을 순서대로 적용한다
    pub(crate) fn replay_harness(self: &Arc<Self>, session: &str) {
        let held = self.held.lock().unwrap().remove(session).unwrap_or_default();
        for (_, mid, ev) in held {
            self.on_harness(&mid, ev);
        }
    }

    pub(crate) fn pane_agent(&self, pane: &str) -> Option<AgentStatus> {
        self.inner.lock().unwrap().panes.get(pane).and_then(|p| p.agent.clone())
    }

    pub(crate) fn thread_of_pane(&self, pane: &str) -> Option<ThreadRec> {
        let rows = self.db.threads(None).ok()?;
        rows.iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .find(|r| r.pane.as_deref() == Some(pane) && r.status != ThreadStatus::Resolved)
    }

    /// agent가 harness 사건을 보냈다
    pub(crate) fn on_harness(self: &Arc<Self>, mid: &str, ev: HarnessEvent) {
        let (activity, answered_block) = {
            let mut i = self.inner.lock().unwrap();
            let Some(p) = i.panes.get_mut(&ev.session) else {
                // 세션 안 hook이 세션 시작 사건보다 먼저 올 수 있다: pane이 등록되면 다시 적용한다
                drop(i);
                self.hold_harness(mid, ev);
                return;
            };
            if p.machine_id != mid {
                return;
            }
            if ev.transcript.is_some() {
                p.transcript = ev.transcript.clone();
            }
            let prev = p.agent.clone();
            let mut st = prev.clone().unwrap_or(AgentStatus {
                activity: AgentActivity::Started,
                since_ms: ev.at_ms,
                harness: ev.harness.clone(),
                message: None,
                percent: None,
                progress: None,
                progress_at_ms: None,
                harness_session: None,
                seen: true,
            });
            if ev.harness != "report" {
                st.harness = ev.harness.clone();
            }
            if ev.harness_session.is_some() {
                st.harness_session = ev.harness_session.clone();
            }
            let next = next_activity(prev.as_ref(), &ev);
            if ev.kind == "progress" {
                st.percent = ev.percent;
                st.progress = ev.message.clone();
                st.progress_at_ms = Some(ev.at_ms);
            }
            // 새 harness 세션: 옛 진행 보고는 이 세션을 말하지 않는다(herdr-projects progress `respond` SessionStart)
            if ev.kind == "session_start" {
                st.percent = None;
                st.progress = None;
                st.progress_at_ms = None;
            }
            // 사용자가 답했다: 옛 「Waiting for you」는 더 맞지 않다(herdr-projects `respond` UserPromptSubmit)
            if matches!(ev.kind.as_str(), "prompt_submit" | "turn_start")
                && st.progress.as_deref() == Some(crate::orch::WAITING)
            {
                st.progress = None;
                st.progress_at_ms = None;
            }
            // 이 막힘(상태가 질문에서 넘어가지 않았다)에서 이미 답했다: 늦게 온 질문 사건은 답한 그 질문이다
            let answered_block = next == Some(AgentActivity::Question)
                && prev.as_ref().is_some_and(|a| a.activity == AgentActivity::Question)
                && p.answered;
            if next.is_some_and(|a| a != AgentActivity::Question) {
                p.answered = false;
            }
            if let Some(a) = next {
                if a != st.activity {
                    st.since_ms = ev.at_ms;
                }
                let sees = self.someone_sees(&ev.session, p.lease.as_ref());
                st.seen = crate::seen::seen_after(prev.as_ref().map(|x| x.activity), a, st.seen, sees);
                st.activity = a;
                st.message = ev.message.clone();
            }
            p.agent = Some(st);
            let _ = p.events.send(crate::PaneEvent::State);
            (next, answered_block)
        };
        self.bump();
        // harness 상태·진행 보고는 thread 묶음과 nudge를 바꿀 수 있다(transitions.rs, inbox.rs). 참고는 15초마다 보지만
        // aaa는 사건마다 다시 세어, 짧은 턴도 전이로 남는다. thread는 보고를 먼저 다시 본다(orch.rs `refresh_report`).
        if activity.is_some() || ev.kind == "progress" {
            match self.thread_of_pane(&ev.session) {
                Some(r) if r.status == ThreadStatus::Open => self.refresh_report(&r.project, &r.id),
                _ => self.tick_wake.notify_one(),
            }
        }
        // 기다리던 사건. 할 일(on_fire)은 잠금을 놓고 돌린 뒤 깨운다(기다리던 쪽이 그 기록을 본 뒤에 이어 가게)
        let mut fired = Vec::new();
        if let Some(list) = self.waiters.lock().unwrap().get_mut(&ev.session) {
            let mut keep = Vec::new();
            for (kinds, tx, on_fire) in list.drain(..) {
                if kinds.contains(&ev.kind.as_str()) {
                    // 이미 그만 기다린 쪽(시간 넘김)의 할 일은 하지 않는다
                    if !tx.is_closed() {
                        fired.push((tx, on_fire));
                    }
                } else if !tx.is_closed() {
                    keep.push((kinds, tx, on_fire));
                }
            }
            *list = keep;
        }
        for (tx, on_fire) in fired {
            if let Some(f) = on_fire {
                f();
            }
            let _ = tx.send(ev.kind.clone());
        }
        // 질문: 열리거나, harness가 넘어가면 닫힌다. 답한 확인창 뒤에 늦게 온 질문 사건(옛 agent의 Claude
        // Notification `permission_prompt`가 답보다 늦게 도착)은 그 질문을 다시 열지 않는다. 참고 herdr 0.9.5는 상태 hook을
        // 아예 버리고(`integration/assets/claude/herdr-agent-state.sh`: `session` 말고는 exit 0) 화면 감지는 막힘에 들어설
        // 때만 알리므로, 한 막힘에 질문은 하나다. 옛 agent는 도구마다 PreToolUse·PostToolUse(작업 중)를 내므로 다음
        // 확인창은 새 막힘이다(t-0333 m13 사건 16, plan §8.99·§8.110)
        match activity {
            Some(AgentActivity::Question) if answered_block => {}
            Some(AgentActivity::Question) => {
                let kind = ev.question_kind.clone().unwrap_or_else(|| "permission".into());
                self.ask(&ev.session, &kind, ev.message.clone().unwrap_or_default());
            }
            Some(AgentActivity::Working) | Some(AgentActivity::Idle) | Some(AgentActivity::Exited) => {
                self.close_questions_of(&ev.session);
            }
            _ => {}
        }
        if let Some(rec) = self.thread_of_pane(&ev.session) {
            let _ = self.thread_events.send(self.thread_view(&rec));
        }
    }

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
        if q.state == QuestionState::Answered
            && let Some(p) = self.inner.lock().unwrap().panes.get_mut(&q.pane)
        {
            p.answered = true;
        }
        let state = match q.state {
            QuestionState::Open => "open",
            QuestionState::Answered => "answered",
            QuestionState::Gone => "gone",
        };
        if let Err(e) = self.db.rec_put("questions", &q.id, state, &serde_json::to_string(q).unwrap(), (&q.pane, "")) {
            tracing::error!("질문 저장 실패: {e:#}");
        }
        let _ = self.question_events.send(q.clone());
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
            id: format!("q-{}", aaa_common::util::new_id()[..10].to_ascii_lowercase()),
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
            .filter_map(|j| serde_json::from_str(j).ok())
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
        // hook 사건이 화면보다 먼저 올 수 있다: 선택 창이 그려지기를 잠깐 기다린다
        let until = Instant::now() + MENU_WAIT;
        let keys = loop {
            let lines = self.screen_lines(&mid, &pid).await.unwrap_or_default();
            match answer_keys(&lines, choice) {
                Ok(k) => break k,
                Err(e) if Instant::now() >= until => {
                    return Err(err(ErrorCode::BadRequest, format!("{e}. 터미널에서 직접 답한다")));
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(200)).await,
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

    // ───────────── 입력(brief 안내·메시지) ─────────────

    /// 글을 입력하고 Enter. harness가 받았다고 알리면 true. 프로그램이 bracketed paste를 켰으면 글을 감싼다.
    pub(crate) async fn send_prompt(self: &Arc<Self>, mid: &str, pane: &str, text: &str) -> Result<bool, String> {
        self.submit(mid, pane, Some(text), None).await
    }

    /// 입력창에 남은 글을 Enter로만 보낸다(다시 치지 않는다). 받았다고 알리면 true.
    pub(crate) async fn send_enter(self: &Arc<Self>, mid: &str, pane: &str) -> Result<bool, String> {
        self.submit(mid, pane, None, None).await
    }

    async fn submit(
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
        let before = |msg: String| SubmitErr { typed: false, msg };
        let after = |msg: String| SubmitErr { typed: true, msg };
        let conn = self.live_conn(mid).map_err(|e| before(e.message))?;
        // Enter 뒤 harness가 입력을 받았다고 알리기를 기다리는 시간(설정 `accept_wait_ms`, 기본 10초)
        let accept_wait = Duration::from_millis(self.cfg.projects.accept_wait_ms);
        // 선택 창에 글을 치면 Enter가 엉뚱한 선택지를 고른다(Claude 폴더 신뢰 창의 기본은 「No, exit」)
        if let Some((_, q)) = self.screen_lines(mid, pane).await.as_deref().and_then(modal_of) {
            return Err(before(format!("선택 창이 떠 있어 입력하지 않았다: {q}")));
        }
        let agent = self.pane_agent(pane);
        let hooked = agent.as_ref().is_some_and(|a| a.harness != "report");
        // 일하는 중인 harness는 받은 글을 다음 턴으로 줄 세운다(Claude Code 대기 메시지). herdr `agent prompt --wait`도
        // 보낸 직후 working·blocked면 받은 것으로 친다(api/wait.rs `prompt_activity_observed`)
        let busy = hooked && agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Working);
        let (hook_accept, mut screen_accept) = if hooked && !busy { (on_accept, None) } else { (None, on_accept) };
        let accepted = self.wait_kind_then(pane, ACCEPTED, hook_accept);
        // 프로그램이 bracketed paste를 켰을 때만 감싸고 Enter도 그 터미널 모드대로 쓴다(herdr `encode_api_submission_parts`,
        // src/app/api_helpers.rs). 켜지 않은 프로그램에 감싸 보내면 `^[[200~`가 글로 남는다.
        let modes = self.key_modes(pane);
        if let Some(text) = text {
            conn.request(AgentRequest::Input { session: pane.to_string(), data: B64(modes.paste(text.trim_end())) })
                .await
                .map_err(|e| after(e.message))?;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
        let typed = self.screen_lines(mid, pane).await;
        let enter = modes.key("enter").unwrap_or_else(|| b"\r".to_vec());
        conn.request(AgentRequest::Input { session: pane.to_string(), data: B64(enter) })
            .await
            .map_err(|e| after(e.message))?;
        if busy {
            let still = matches!(
                self.pane_agent(pane).map(|a| a.activity),
                Some(AgentActivity::Working | AgentActivity::Question)
            );
            // 그 사이 턴이 끝났으면 받은 글로 새 턴이 시작되기를 기다린다
            let ok = still || tokio::time::timeout(accept_wait, accepted).await.is_ok_and(|r| r.is_ok());
            if ok && let Some(f) = screen_accept.take() {
                f();
            }
            return Ok(ok);
        }
        if hooked {
            return Ok(tokio::time::timeout(accept_wait, accepted).await.is_ok_and(|r| r.is_ok()));
        }
        // hook이 없는 harness: Enter 뒤 화면이 바뀌면 받았다고 본다
        let until = Instant::now() + accept_wait;
        while Instant::now() < until {
            tokio::time::sleep(Duration::from_millis(250)).await;
            if self.screen_lines(mid, pane).await != typed {
                if let Some(f) = screen_accept.take() {
                    f();
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// `threads/<id>.task.md`의 `## Follow-ups` 아래에 시각과 함께 덧붙인다(herdr-projects `append_follow_up`)
    fn append_follow_up(&self, project: &str, id: &str, text: &str) -> std::io::Result<()> {
        let dir = self.project_dir(project).map_err(|e| std::io::Error::other(e.message))?;
        let path = dir.join("threads").join(format!("{id}.task.md"));
        let _guard = self.follow_up_lock.lock().unwrap();
        let mut task = std::fs::read_to_string(&path).unwrap_or_default();
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
        crate::orch::write_atomic(&path, task.as_bytes())
    }

    fn save_message(&self, m: &MessageView) {
        let state = match m.state {
            DeliveryState::Queued => "queued",
            DeliveryState::Accepted => "accepted",
            DeliveryState::Unknown => "unknown",
        };
        let _ = self.db.rec_put("messages", &m.id, state, &serde_json::to_string(m).unwrap(), (&m.project, &m.thread));
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
        if self.pane_agent(&pid).map(|a| a.activity) == Some(AgentActivity::Question) {
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
        if let Some(screen) = self.screen(&mid, &pid).await {
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
            id: format!("m-{}", aaa_common::util::new_id()[..10].to_ascii_lowercase()),
            project: project.to_string(),
            thread: id.to_string(),
            mode,
            state: DeliveryState::Queued,
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
            _ if activity == Some(AgentActivity::Question) => {
                m.state = DeliveryState::Unknown;
                self.save_message(&m);
                self.flow(
                    &r,
                    "message_unknown",
                    None,
                    format!("{} (확인창이 떠 보내지 않았다. 다시 보내지 않는다)", m.id),
                );
            }
            MessageMode::Interrupt => {
                if activity == Some(AgentActivity::Working) {
                    let stopped = self.wait_kind(&pid, &["turn_end", "idle"]);
                    let conn = self.live_conn(&mid)?;
                    conn.request(AgentRequest::Input { session: pid.clone(), data: B64(b"\x1b".to_vec()) }).await?;
                    // 멈추면 입력 상자가 보인다: 화면 감지가 idle을 알린다(Claude 「Interrupted」 뒤 `live_prompt_box`).
                    // 멈춘 뒤 다음 입력을 받을 수 있게 되기를 기다리는 상한은 설정 `stop_wait_ms`(기본 15초)다.
                    let _ = tokio::time::timeout(Duration::from_millis(self.cfg.projects.stop_wait_ms), stopped).await;
                }
                self.deliver(&mut m, &mid, &pid).await;
            }
            MessageMode::Queue => self.deliver(&mut m, &mid, &pid).await,
        }
        Ok(m)
    }

    async fn deliver(self: &Arc<Self>, m: &mut MessageView, mid: &str, pane: &str) {
        // 「받았다」는 받았다고 안 그 자리에서 기록한다: 받자마자 뜬 확인창(question_shown)보다 앞에 남게(CI m13)
        let on_accept: Option<OnFire> = self.load(&m.project, &m.thread).ok().map(|rec| {
            let me = self.clone();
            let id = m.id.clone();
            Box::new(move || me.flow(&rec, "message_accepted", None, id)) as OnFire
        });
        let r = self.submit(mid, pane, Some(&m.text), on_accept).await;
        m.state = if r == Ok(true) { DeliveryState::Accepted } else { DeliveryState::Unknown };
        self.save_message(m);
        // 입력한 글은(받았는지 몰라도) task 기록에 남겨 다시 시작하면 brief와 함께 읽게 한다(herdr-projects
        // thread.rs `append_follow_up`). 입력 전에 거부됐으면(Err) 남기지 않는다.
        if r.is_ok()
            && let Err(e) = self.append_follow_up(&m.project, &m.thread, &m.text)
        {
            tracing::warn!(project = %m.project, thread = %m.thread, "Follow-ups를 쓰지 못했다: {e}");
        }
        if m.state != DeliveryState::Accepted
            && let Ok(rec) = self.load(&m.project, &m.thread)
        {
            let why = match r {
                Ok(_) => " (받았다는 사건·화면 변화가 없었다. 다시 보내지 않는다)".to_string(),
                Err(e) => format!(" ({e})"),
            };
            self.flow(&rec, "message_unknown", None, format!("{}{why}", m.id));
        }
    }

    /// 옛 host가 턴 끝을 기다리며 대기열에 둔 메시지, 또는 보내던 중 host가 끝난 메시지: 보냈는지 모른다고 남긴다
    /// (다시 보내지 않는다, §8.5 a). 이 판에는 대기열이 없어 기록 뒤 바로 보낸다.
    pub(crate) fn forget_queued_messages(&self) {
        let left: Vec<MessageView> = self
            .db
            .rec_list("messages", Some("queued"))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str(j).ok())
            .collect();
        for mut m in left {
            m.state = DeliveryState::Unknown;
            self.save_message(&m);
            if let Ok(rec) = self.load(&m.project, &m.thread) {
                self.flow(
                    &rec,
                    "message_unknown",
                    None,
                    format!("{} (host가 다시 시작돼 보냈는지 모른다. 다시 보내지 않는다)", m.id),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(t: &str) -> Vec<String> {
        t.lines().map(|l| l.to_string()).collect()
    }

    /// Claude Code 2.1의 실제 폴더 신뢰 창(경로만 바꿨다). 기본 선택이 「No, exit」다.
    const CLAUDE_TRUST: &str = "\
 Accessing workspace:

 /work/demo/t-0001

 Quick safety check: Is this a project you created or one you trust? (Like your own code, a well-known open source
 project, or work from your team). If not, take a moment to review what's in this folder first.

 Claude Code'll be able to read, edit, and execute files here.

 Security guide

 ❯ No, exit
   Yes, I trust this folder

 Enter to confirm · Esc to cancel";

    #[test]
    fn claude_trust_screen() {
        let l = lines(CLAUDE_TRUST);
        let (kind, text) = modal_of(&l).unwrap();
        assert_eq!(kind, "trust");
        assert!(text.starts_with("Quick safety check: Is this a project"), "{text}");
        assert_eq!(answer_keys(&l, "yes").unwrap(), b"\x1b[B\r");
        assert_eq!(answer_keys(&l, "no").unwrap(), b"\x1b");
    }

    /// Claude Code 2.1의 실제 권한 창(기본 선택 Yes, 번호 붙음, 「Esc to cancel」만 있다)
    #[test]
    fn claude_permission_screen() {
        let l = lines(
            "\
 Do you want to create report.md?
 ❯ 1. Yes
   2. Yes, and switch to accept edits (auto-approve file edits and common file commands) for this session (shift+tab)
   3. No
 Esc to cancel · Tab to amend",
        );
        assert_eq!(modal_of(&l).unwrap(), ("confirm", "Do you want to create report.md?".to_string()));
        assert_eq!(answer_keys(&l, "yes").unwrap(), b"\r");
    }

    /// 위쪽 출력(소스 cat 등)에 선택 창 글이 있어도, 맨 아래가 평소 입력 상자면 선택 창이 아니다
    #[test]
    fn dialog_text_in_scrollback_is_not_a_modal() {
        let l = lines(
            "\
⏺ Bash(cat harness.rs)
  const MODAL_MARKS = [\"Enter to confirm\", \"Esc to cancel\"];
   ❯ 1. Yes
     2. No
 Esc to cancel · Tab to amend
⏺ 위 파일을 읽었다.
✻ Brewed for 2s
────────────────────────────────
❯
────────────────────────────────
  Haiku 5.5 medium | ctx 95% left
  ⏸ manual mode on

",
        );
        assert!(modal_of(&l).is_none());
        assert!(answer_keys(&l, "yes").is_err());
    }

    /// Codex 0.163의 실제 명령 승인 창(경로만 바꿨다). 선택 커서는 `›`, 맨 아래 줄은 소문자 「enter」·「esc」다.
    #[test]
    fn codex_approval_screen() {
        let l = lines(
            "\
› .aaa/demo-t-0001/brief.md를 읽고 그대로 하라.

• Failed (exit 1) touch probe.txt
  └ touch: probe.txt: Operation not permitted

• Running touch probe.txt

  Would you like to run the following command?

  Environment: local

  $ touch probe.txt

› 1. Yes, proceed (y)
  2. Yes, and don't ask again for commands that start with `touch probe.txt` (p)
  3. No, and tell Codex what to do differently (esc)

  Press enter to confirm or esc to cancel
",
        );
        assert_eq!(modal_of(&l).unwrap(), ("confirm", "Would you like to run the following command?".to_string()));
        assert_eq!(answer_keys(&l, "yes").unwrap(), b"\r");
        assert_eq!(answer_keys(&l, "no").unwrap(), b"\x1b");
    }

    #[test]
    fn numbered_menu_and_missing_menu() {
        let l = lines(
            "Do you want to proceed?\n  1. Yes\n  2. Yes, and don't ask again\n❯ 3. No, and tell Claude\nEsc to cancel\n",
        );
        assert_eq!(answer_keys(&l, "yes").unwrap(), b"\x1b[A\x1b[A\r");
        let l = lines("Do you want to proceed?\n❯ 1. Yes\n  2. No\nEsc to cancel\n");
        assert_eq!(answer_keys(&l, "yes").unwrap(), b"\r");
        // 선택 창이 없으면 Enter를 보내지 않는다
        assert!(answer_keys(&lines("fake> \nNo idea"), "yes").is_err());
        assert!(modal_of(&lines("fake> got: x")).is_none());
        assert!(answer_keys(&[], "maybe").is_err());
    }

    /// agent가 다시 붙으며 보낸 옛 기억(옛 agent의 `turn_start`)이 먼저 도착한 화면 감지 idle을 덮지 않는다(t-0334 결함 1)
    #[test]
    fn older_events_do_not_change_the_activity() {
        let ev = |kind: &str, at_ms| HarnessEvent {
            session: "s".into(),
            harness: "claude".into(),
            kind: kind.into(),
            harness_session: None,
            message: None,
            question_kind: None,
            percent: None,
            at_ms,
            transcript: None,
        };
        let idle = AgentStatus {
            activity: AgentActivity::Idle,
            since_ms: 2_000,
            harness: "claude".into(),
            message: None,
            percent: None,
            progress: None,
            progress_at_ms: None,
            harness_session: None,
            seen: true,
        };
        assert_eq!(next_activity(Some(&idle), &ev("turn_start", 1_000)), None);
        assert_eq!(next_activity(Some(&idle), &ev("working", 2_000)), Some(AgentActivity::Working));
        // host가 처음 듣는 pane(host 재시작)은 옛 기억으로 상태를 정한다
        assert_eq!(next_activity(None, &ev("turn_start", 1_000)), Some(AgentActivity::Working));
        assert_eq!(next_activity(None, &ev("session_start", 1_000)), Some(AgentActivity::Started));
        assert_eq!(next_activity(Some(&idle), &ev("session_start", 3_000)), None);
    }

    struct Tmp(std::path::PathBuf);
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 머신 `m`의 Claude pane `s` 하나가 있는 host(agent 연결 없음)
    fn host_with_pane() -> (Arc<Host>, Tmp) {
        let dir = std::env::temp_dir().join(format!("aaa-harness-test-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, dir.clone()).unwrap();
        let info = aaa_proto::SessionInfo {
            id: "s".into(),
            name: "claude".into(),
            argv: vec!["claude".into()],
            cwd: None,
            pid: None,
            started_at_ms: 0,
            boot_id: "b".into(),
            cols: 80,
            rows: 24,
            state: aaa_proto::SessionState::Running,
            role: None,
            label: None,
            harness: Some("claude".into()),
        };
        h.on_session("m", info, false);
        (h, Tmp(dir))
    }

    /// 답한 확인창 뒤 늦게 온 질문 사건은 그 질문을 다시 열지 않는다(t-0333 m13 사건 16·plan §8.99: 부하에서
    /// PermissionRequest → 답 → 1.3초 뒤 Notification `permission_prompt`). 막힘이 끝난 뒤(작업 중: 화면 감지나 옛 agent의
    /// PostToolUse·PreToolUse)의 질문 사건은 다음 확인창의 새 질문이다.
    #[tokio::test]
    async fn a_late_question_event_does_not_reopen_the_answered_question() {
        let (h, _t) = host_with_pane();
        let ev = |kind: &str, at_ms| HarnessEvent {
            session: "s".into(),
            harness: "claude".into(),
            kind: kind.into(),
            harness_session: None,
            message: Some("Bash make test".into()),
            question_kind: Some("permission".into()),
            percent: None,
            at_ms,
            transcript: None,
        };
        h.on_harness("m", ev("question", 1_000));
        let mut q = h.open_question_of("s").expect("질문");
        // 답의 마지막 걸음(`answer`: 키를 보낸 뒤 Answered로 적는다)
        q.state = QuestionState::Answered;
        q.answer = Some("yes".into());
        h.save_question(&q);
        h.on_harness("m", ev("question", 2_300));
        assert_eq!(h.open_question_of("s").map(|x| x.id), None, "답한 질문을 다시 열었다");
        assert_eq!(h.questions(true).len(), 1);
        h.on_harness("m", ev("working", 3_000));
        h.on_harness("m", ev("question", 4_000));
        let again = h.open_question_of("s").expect("다음 확인창의 질문");
        assert_ne!(again.id, q.id);
    }
}
