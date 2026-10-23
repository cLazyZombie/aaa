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
    MenuPick, MessageMode, MessageView, QuestionMenu, QuestionState, QuestionView, ThreadStatus,
};
use tokio::sync::oneshot;

use crate::orch::ThreadRec;
use crate::{Host, err};

/// 질문에 답할 때 선택 창이 화면에 그려지기를 기다리는 시간
const MENU_WAIT: Duration = Duration::from_secs(3);
/// 선택 창 답: 옮긴 커서가 화면에 보이기를 기다리는 상한(참고 herdr-web-ui `SETTLE_MS`)과 키 사이 틈(참고 30ms)
const SETTLE: Duration = Duration::from_millis(1500);
const STEP_GAP: Duration = Duration::from_millis(30);
/// 입력을 받았다는 사건: 화면 감지의 작업 중·질문(herdr-projects `--until working --until blocked`), 옛 agent의 turn_start.
/// 대화 정리 중(`compacting`)도 작업 중이다: 받은 글이 턴을 열자마자 자동 정리가 먼저 돌 수 있다(10-20 운영 coordinator)
const ACCEPTED: &[&str] = &["working", "compacting", "question", "turn_start"];

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
    "enter continue · esc back",
    "Press enter to continue",
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
        if menu_choice(rest).is_none() {
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

/// 의미 답과 화면 글의 대응. Codex 신뢰 창은 Yes/No 대신 Trust and continue/Quit를 쓴다.
fn menu_choice(text: &str) -> Option<&'static str> {
    match text.trim() {
        "Trust and continue" => Some("yes"),
        "Quit" => Some("no"),
        t if t.starts_with("Yes") => Some("yes"),
        t if t.starts_with("No") => Some("no"),
        _ => None,
    }
}

/// 질문 답 → harness 키(adapter). 「아니오」는 취소(Esc), Codex 신뢰 창은 Quit 선택이다.
/// 「예」는 화면 선택 창에서 긍정 선택지까지
/// 화살표로 옮긴 뒤 Enter다(Claude Code 폴더 신뢰 창은 기본 선택이 「No, exit」라 Enter만 보내면 안 된다).
pub(crate) fn answer_keys(lines: &[String], choice: &str) -> Result<Vec<u8>, String> {
    match choice {
        "yes" | "no" => {}
        other => return Err(format!("모르는 답: {other}")),
    }
    let m = dialog(lines).map(menu).unwrap_or_default();
    if choice == "no" && !m.iter().any(|(_, t)| t == "Trust and continue") {
        return Ok(b"\x1b".to_vec());
    }
    let cur = m.iter().position(|(s, _)| *s).ok_or("화면에서 선택 창을 찾지 못했다")?;
    let want = m.iter().position(|(_, t)| menu_choice(t) == Some(choice)).ok_or("화면 선택 창에 답 선택지가 없다")?;
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
        "turn_start" | "working" | "compacting" => Some(AgentActivity::Working),
        "turn_end" | "idle" => Some(AgentActivity::Idle),
        "question" => Some(AgentActivity::Question),
        "error" => Some(AgentActivity::Error),
        "exited" => Some(AgentActivity::Exited),
        _ => None,
    }
}

/// 질문·메시지 ID(`q-…`·`m-…`, 10자). ULID의 무작위 부분에서 뽑는다: 앞 10자는 ms 시각이라 같은 ms에 만든 둘이 같은
/// ID가 되어 둘째가 첫째 기록을 덮었다(저장은 `ON CONFLICT(id) DO UPDATE`). 차례는 기록의 `created_at`이 정한다.
fn short_id(prefix: &str) -> String {
    format!("{prefix}-{}", aaa_common::util::new_id()[16..].to_ascii_lowercase())
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
    pub(crate) fn on_harness(self: &Arc<Self>, mid: &str, mut ev: HarnessEvent) {
        // 옛 agent는 hook의 harness 세션 ID를 거르지 않고 보낸다: host도 UUID 꼴만 받는다(`--resume` 인자가 된다)
        if ev.harness_session.as_deref().is_some_and(|s| !aaa_common::util::harness_session_ok(s)) {
            tracing::warn!(pane = %ev.session, "UUID 꼴이 아닌 harness 세션 ID를 버렸다");
            ev.harness_session = None;
        }
        let (activity, answered_block, running, mut finished) = {
            let mut i = self.inner.lock().unwrap();
            // 이 agent는 답한 뒤 화면에 남은 확인창을 새로 묻는 것으로만 다시 알린다(`ask_anew.v1`)
            let anew = i.machines.get(mid).is_some_and(|m| m.features.iter().any(|f| f == "ask_anew.v1"));
            let Some(p) = i.panes.get_mut(&ev.session) else {
                // 세션 안 hook이 세션 시작 사건보다 먼저 올 수 있다: pane이 등록되면 다시 적용한다
                drop(i);
                self.hold_harness(mid, ev);
                return;
            };
            if p.machine_id != mid {
                return;
            }
            // 셸 pane 앞의 agent가 끝나 셸로 돌아왔다(agent의 앞 프로세스 찾기, herdr `ClearAgent`): 다시 agent 없는 셸이다.
            // 재부팅 복원도 그 셸에 대화 이어가기를 치지 않는다(herdr는 놓은 agent를 세션에 남기지 않는다)
            if ev.kind == "released" {
                self.bgwait.lock().unwrap().forget(&ev.session);
                p.agent = None;
                p.turn_from = None;
                p.turn_reported = false;
                p.alert = None;
                p.answered = false;
                p.transcript = None;
                let _ = p.events.send(crate::PaneEvent::State);
                drop(i);
                self.bump();
                self.close_questions_of(&ev.session);
                self.tick_wake.notify_one();
                return;
            }
            if ev.kind == "session_start" || ev.transcript.is_some() {
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
                progress_since_ms: None,
                harness_session: None,
                seen: true,
                plan: None,
                background: None,
                background_wait: false,
                compacting: false,
                context: None,
                model_observation: None,
            });
            if ev.harness != "report" {
                st.harness = ev.harness.clone();
            }
            if ev.kind == "session_start" || ev.harness_session.is_some() {
                st.harness_session = ev.harness_session.clone();
            }
            let next = next_activity(prev.as_ref(), &ev);
            if matches!(
                ev.kind.as_str(),
                "turn_start" | "working" | "compacting" | "turn_end" | "idle" | "question" | "error" | "exited"
            ) {
                tracing::info!(
                    machine = mid, pane = %ev.session, harness = %ev.harness, kind = %ev.kind,
                    event_at_ms = ev.at_ms, previous = ?prev.as_ref().map(|a| a.activity),
                    previous_since_ms = ?prev.as_ref().map(|a| a.since_ms), applied = ?next,
                    "harness 상태: host 적용 판정"
                );
            }
            if ev.kind == "progress" && st.progress_at_ms.is_none_or(|at| ev.at_ms >= at) {
                if st.progress != ev.message || st.progress_since_ms.is_none() {
                    st.progress_since_ms = ev.message.as_ref().map(|_| ev.at_ms);
                }
                st.percent = ev.percent;
                st.progress = ev.message.clone();
                st.progress_at_ms = Some(ev.at_ms);
            }
            // agent가 대화 기록에서 읽은 계획·배경 작업 수(차이 목록 S5): 상태는 바꾸지 않는다(참고 `pane-status` plan·background_tasks)
            if ev.kind == "plan" {
                st.plan = ev.plan.clone();
                st.background = ev.background;
                st.context = ev.context.clone();
                st.model_observation = ev.model_observation.clone();
            }
            // harness가 끝나면 그 안에서 돌던 배경 작업도 끝났고, 그 세션의 컨텍스트도 더 쓰이지 않는다
            if next == Some(AgentActivity::Exited) {
                st.background = None;
                st.context = None;
            }
            // 새 harness 세션: 옛 진행 보고는 이 세션을 말하지 않는다(herdr-projects progress `respond` SessionStart)
            if ev.kind == "session_start" {
                st.percent = None;
                st.progress = None;
                st.progress_at_ms = None;
                st.progress_since_ms = None;
                st.model_observation = None;
            }
            if ev.kind == "session" && ev.harness_session != prev.as_ref().and_then(|a| a.harness_session.clone()) {
                st.model_observation = None;
            }
            // 사용자가 답했다: 옛 「Waiting for you」는 더 맞지 않다(herdr-projects `respond` UserPromptSubmit)
            if matches!(ev.kind.as_str(), "prompt_submit" | "turn_start")
                && st.progress.as_deref() == Some(crate::orch::WAITING)
            {
                st.progress = None;
                st.progress_at_ms = None;
                st.progress_since_ms = None;
            }
            // 이 막힘(상태가 질문에서 넘어가지 않았다)에서 이미 답했다: 늦게 온 질문 사건은 답한 그 질문이다
            let answered_block = !anew
                && next == Some(AgentActivity::Question)
                && prev.as_ref().is_some_and(|a| a.activity == AgentActivity::Question)
                && p.answered;
            if next.is_some_and(|a| a != AgentActivity::Question) {
                p.answered = false;
            }
            // 알림이 본 앞 상태(BG로 기다리는 쉼은 작업 중, notice.rs `alert_activity`)
            let was_alert =
                p.alert.or_else(|| prev.as_ref().map(|x| crate::notice::alert_activity(x.activity, x.background_wait)));
            if let Some(a) = next {
                if a != st.activity {
                    st.since_ms = ev.at_ms;
                }
                let was = prev.as_ref().map(|x| x.activity);
                let sees = self.someone_sees(&ev.session);
                st.seen = crate::seen::seen_after(was, a, st.seen, sees);
                st.activity = a;
                st.message = ev.message.clone();
                // 정리 중은 작업 중 안의 표지다: 다른 상태 사건이 오면 끝났다(화면 표지가 사라졌다, plan §8.309)
                st.compacting = ev.kind == "compacting";
            }
            // 배경 작업을 기다리는 쉼(bgwait.rs, 참고 server/index.ts가 상태·배경 수·사람 입력마다 BackgroundWait에 알림).
            // host 시계로 잰다: agent 시계가 어긋나도 유예·한도는 이 host에서 맞다
            {
                let now = aaa_common::util::now_ms();
                let mut w = self.bgwait.lock().unwrap();
                if ev.kind == "prompt_submit" {
                    w.prompt(&ev.session, now, now);
                }
                if ev.kind == "plan" || next == Some(AgentActivity::Exited) {
                    w.running(&ev.session, st.background.unwrap_or(0), now);
                }
                match next {
                    Some(AgentActivity::Exited | AgentActivity::Error) => w.forget(&ev.session),
                    Some(a) => {
                        w.status(&ev.session, a, now);
                    }
                    None => {}
                }
                st.background_wait = w.waiting(&ev.session);
            }
            // 보는 화면 없이 끝난 턴(notice.rs): 알림은 BG로 기다리는 쉼을 작업 중으로 보아 그 기다림이 끝날 때 한 번 알린다
            // (참고 alertStatus). 그 턴의 길이와 함께 잠금을 놓은 뒤 알린다
            let now_alert = crate::notice::alert_activity(st.activity, st.background_wait);
            let mut finished = None;
            // 턴 안에서 온 사람 입력은 턴을 끝내지 않는다: 그 입력이 연 일은 곧 화면에 작업 중으로 보인다(참고 herdr는
            // UserPromptSubmit에 작업 중이다). 그 입력이 배경 기다림을 지워도 BG에서 다음 턴으로 이어진다
            let now_alert = if ev.kind == "prompt_submit" && crate::notice::in_turn(was_alert) {
                was_alert.unwrap_or(now_alert)
            } else {
                now_alert
            };
            if was_alert != Some(now_alert) {
                if !crate::notice::in_turn(was_alert) && crate::notice::in_turn(Some(now_alert)) {
                    p.turn_reported = false;
                }
                if !p.turn_reported && crate::notice::finished(was_alert, now_alert, st.seen) {
                    let turn_ms = p.turn_from.map(|from| ev.at_ms.saturating_sub(from));
                    finished = Some(crate::notice::finish_notice(&ev.session, p, &st, turn_ms));
                }
                p.turn_from = crate::notice::turn_from(was_alert, now_alert, p.turn_from, ev.at_ms);
            }
            p.alert = Some(now_alert);
            p.agent = Some(st);
            let _ = p.events.send(crate::PaneEvent::State);
            (next, answered_block, p.info.state.is_running(), finished)
        };
        self.bump();
        // harness 상태·진행 보고는 thread 묶음과 nudge를 바꿀 수 있다(transitions.rs, inbox.rs). 참고는 15초마다 보지만
        // aaa는 사건마다 다시 세어, 짧은 턴도 전이로 남는다. thread는 보고를 먼저 다시 본다(orch.rs `refresh_report`).
        // 끝남은 보고 확인을 건 뒤에 미룬다(notice.rs `turn_finished`): ticker는 확인 중인 thread를 셈하지 않는다
        if activity.is_some() || ev.kind == "progress" {
            let thread = self.thread_of_pane(&ev.session);
            match &thread {
                Some(r) if r.status == ThreadStatus::Open => self.refresh_report(&r.project, &r.id),
                _ => self.tick_wake.notify_one(),
            }
            if let Some(mut n) = finished.take() {
                let open = thread.filter(|r| r.status == ThreadStatus::Open);
                if let Some(r) = &open {
                    n.project = Some(r.project.clone());
                    n.thread = Some(r.id.clone());
                    n.title = r.title.clone();
                }
                self.turn_finished(n, open.as_ref());
            }
        }
        // 상태는 그대로인데 배경 기다림만 바뀐 사건(계획·배경 수)의 끝남
        if let Some(n) = finished {
            self.announce_finish(n);
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
        // 때만 알린다. 옛 agent는 도구마다 PreToolUse·PostToolUse(작업 중)를 내므로 다음 확인창은 새 막힘이다(t-0333 m13
        // 사건 16, plan §8.99·§8.110). `ask_anew.v1` agent는 화면 감지만 질문을 내고, 막힘에 입력이 간 뒤 남은 확인창을
        // 새 물음으로 다시 알린다(herdr-web-ui `askingEnded`, QA t-0337 Q18): 답한 질문 뒤의 질문 사건은 새 질문이다
        // 끝난 세션(재부팅으로 끊김·종료)에는 질문을 열지 않는다: 다시 붙은 agent가 알리는 세션별 마지막 상태
        // (`hello.harness`, 재부팅 복원이 harness 세션 ID를 알려고)가 확인창이면 그 pane 질문을 닫은 뒤 다시 열었다
        // (QA t-0337 Q20). 참고 herdr-web-ui는 살아 있는 blocked pane 화면에서만 질문을 읽는다(`server/prompt.ts`).
        match activity {
            Some(AgentActivity::Question) if answered_block || !running => {}
            Some(AgentActivity::Question) => {
                let kind = ev.question_kind.clone().unwrap_or_else(|| "permission".into());
                let q = self.ask(&ev.session, &kind, ev.message.clone().unwrap_or_default());
                if q.menu.is_none() {
                    self.read_menu(q.id, ev.session.clone());
                }
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
        let _order = self.question_lock.lock().unwrap();
        self.save_question_locked(q);
    }

    fn save_question_locked(&self, q: &QuestionView) {
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
        let mut view = q.clone();
        let project = view.project.clone().or_else(|| self.sole_project().ok().flatten()).unwrap_or_default();
        view.short_id = self.db.question_id(&project, "harness", &q.id, "", 0).ok();
        let _ = self.question_events.send(view);
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

    /// `trust_screens = user`: aaa 세션 안 CLI(coordinator)는 thread pane의 신뢰 창에 답하지 않는다(`thread keys`와 같다).
    /// 사람은 터미널·웹에서 답한다. 예·아니오 답과 화면 선택 창 답이 같은 문을 지난다
    async fn refuse_trust_screen(&self, q: &QuestionView, mid: &str, pid: &str, actor: &str) -> Result<(), ErrorBody> {
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
    fn read_menu(self: &Arc<Self>, id: String, pane: String) {
        let h = self.clone();
        tokio::spawn(async move {
            let Ok((pid, mid)) = h.resolve_pane(&pane) else { return };
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
    fn set_menu(&self, id: &str, menu: Option<QuestionMenu>) {
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
    async fn on_row(&self, mid: &str, pid: &str, menu_id: &str, row: isize) -> bool {
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
        // 친 글에 프로그램이 반응하고 Enter 뒤 harness가 받았다고 알리기까지 기다리는 시간(설정 `accept_wait_ms`, 기본 10초)
        let accept_wait = Duration::from_millis(self.cfg.projects.accept_wait_ms);
        // 선택 창에 글을 치면 Enter가 엉뚱한 선택지를 고른다(Claude 폴더 신뢰 창의 기본은 「No, exit」)
        let shown = self.screen_lines(mid, pane).await;
        if let Some((_, q)) = shown.as_deref().and_then(modal_of) {
            return Err(before(format!("선택 창이 떠 있어 입력하지 않았다: {q}")));
        }
        let agent = self.pane_agent(pane);
        // 확인창(권한·질문)이 떠 있으면 친 글과 Enter를 그 창이 받는다
        if agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Question) {
            return Err(before("확인창이 떠 있어 입력하지 않았다".into()));
        }
        let hooked = agent.as_ref().is_some_and(|a| a.harness != "report");
        // 일하는 중인 harness는 받은 글을 다음 턴으로 줄 세운다(Claude Code 대기 메시지). herdr `agent prompt --wait`도
        // 보낸 직후 working·blocked면 받은 것으로 친다(api/wait.rs `prompt_activity_observed`)
        let busy = hooked && agent.as_ref().is_some_and(|a| a.activity == AgentActivity::Working);
        // herdr-projects progress::respond(UserPromptSubmit)와 같은 정리다. 전역 hook이 없는 Codex도
        // 입력 받음(working·question·화면 변화)을 확인한 바로 그 자리에서 직전 기다림을 끝낸다.
        // 전송 전에 거부되거나 받음이 unknown이면 지우지 않으며, 그 사이 새로 온 보고도 지키도록 시각을 묶는다.
        let waiting_at = agent
            .as_ref()
            .filter(|a| a.progress.as_deref() == Some(crate::orch::WAITING))
            .and_then(|a| a.progress_at_ms);
        // unknown으로 끝난 대기자가 남아도 host 자체를 붙잡지 않는다.
        let me = Arc::downgrade(self);
        let target = pane.to_string();
        let on_accept: Option<OnFire> = Some(Box::new(move || {
            if let Some(me) = me.upgrade() {
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
        let deadline = Instant::now() + accept_wait;
        if let Some(text) = text {
            conn.request(AgentRequest::Input { session: pane.to_string(), data: B64(modes.paste(text.trim_end())) })
                .await
                .map_err(|e| after(e.message))?;
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
            if let Some((_, q)) = now.as_deref().and_then(modal_of) {
                return Err(after(format!("치는 동안 선택 창이 떠 Enter를 보내지 않았다: {q}")));
            }
            if self.pane_agent(pane).is_some_and(|a| a.activity == AgentActivity::Question) {
                return Err(after("치는 동안 확인창이 떠 Enter를 보내지 않았다".into()));
            }
            if text.is_none() || !hooked || now != shown || Instant::now() >= deadline {
                break now;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        };
        let left = || deadline.saturating_duration_since(Instant::now());
        // 받음은 Enter가 일으킨 사건으로만 센다: 보내기 바로 전에 기다린다. 치는 동안 뜬 확인창(질문)도 받음 사건이라
        // 그 전부터 기다리면 Enter를 보내지 않은 글을 받았다고 적는다
        let accepted = self.wait_kind_then(pane, ACCEPTED, hook_accept);
        let enter = modes.key("enter").unwrap_or_else(|| b"\r".to_vec());
        conn.request(AgentRequest::Input { session: pane.to_string(), data: B64(enter) })
            .await
            .map_err(|e| after(e.message))?;
        if busy {
            // Enter 뒤 확인창이면 그 Enter가 창에 갔을 수 있다: 받은 것으로 치지 않는다(t-0340 보안 감사)
            let still = self.pane_agent(pane).map(|a| a.activity) == Some(AgentActivity::Working);
            // 그 사이 턴이 끝났으면 받은 글로 새 턴이 시작되기를 기다린다
            let ok = still || tokio::time::timeout(left(), accepted).await.is_ok_and(|r| r.is_ok());
            if ok && let Some(f) = screen_accept.take() {
                f();
            }
            return Ok(ok);
        }
        if hooked {
            return Ok(tokio::time::timeout(left(), accepted).await.is_ok_and(|r| r.is_ok()));
        }
        // hook이 없는 harness: Enter 뒤 화면이 바뀌면 받았다고 본다
        while Instant::now() < deadline {
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

    fn input_accepted(&self, pane: &str, waiting_at: Option<u64>) {
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
                let _ = self.thread_events.send(self.thread_view(&r));
            }
        }
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
        crate::orch::write_atomic(&path, task.as_bytes())?;
        // 작업 요약 표지: 요약 뒤 지시가 하나 늘었다(plan §8.259)
        self.summary_follow_ups(project, id, &task);
        drop(_guard);
        if let Ok(r) = self.load(project, id) {
            let _ = self.thread_events.send(self.thread_view(&r));
        }
        Ok(())
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
            id: short_id("m"),
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
            && !m.text.starts_with("[aaa ")
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

    /// Codex 0.163.0 폴더 신뢰 창. 의미 답은 현재 커서에서 실제 선택지로 옮겨야 한다.
    #[test]
    fn codex_trust_screen() {
        let screen = include_str!("../../aaa-agent/src/detect/fixtures/codex-trust.txt");
        for (screen, yes, no) in [
            (screen.to_string(), b"\r".as_slice(), b"\x1b[B\r".as_slice()),
            (screen.replace("› 1.", "  1.").replace("  2.", "› 2."), b"\x1b[A\r".as_slice(), b"\r".as_slice()),
        ] {
            let l = lines(&screen);
            assert_eq!(modal_of(&l).unwrap().0, "trust");
            assert_eq!(answer_keys(&l, "yes").unwrap(), yes);
            assert_eq!(answer_keys(&l, "no").unwrap(), no);
        }
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
            plan: None,
            background: None,
            context: None,
            model_observation: None,
        };
        let idle = AgentStatus {
            activity: AgentActivity::Idle,
            since_ms: 2_000,
            harness: "claude".into(),
            message: None,
            percent: None,
            progress: None,
            progress_at_ms: None,
            progress_since_ms: None,
            harness_session: None,
            seen: true,
            plan: None,
            background: None,
            background_wait: false,
            compacting: false,
            context: None,
            model_observation: None,
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
        cfg.projects.root = dir.join("projects").display().to_string();
        let h = Host::open(cfg, dir.clone()).unwrap();
        let info = aaa_proto::SessionInfo {
            responsibility: None,
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

    fn codex_event(kind: &str, at_ms: u64) -> HarnessEvent {
        HarnessEvent {
            session: "s".into(),
            harness: "codex".into(),
            kind: kind.into(),
            at_ms,
            harness_session: None,
            message: None,
            question_kind: None,
            percent: None,
            transcript: None,
            plan: None,
            background: None,
            context: None,
            model_observation: None,
        }
    }

    #[tokio::test]
    async fn shell_harness_replacement_clears_the_previous_chat_identity() {
        let (h, _tmp) = host_with_pane();
        let id = "019a4c5e-1f2b-7c3d-8e4f-a5b6c7d8e9f0";
        let mut first = codex_event("session_start", 1_000);
        first.harness = "claude".into();
        first.harness_session = Some(id.into());
        first.transcript = Some("/example/projects/session.jsonl".into());
        h.on_harness("m", first);
        assert_eq!(h.pane_agent("s").unwrap().harness_session.as_deref(), Some(id));
        h.on_harness("m", codex_event("session_start", 2_000));
        assert_eq!(h.pane_agent("s").unwrap().harness_session, None);
        assert_eq!(h.inner.lock().unwrap().panes["s"].transcript, None);
        let mut second = codex_event("session_start", 3_000);
        second.harness_session = Some(id.into());
        h.on_harness("m", second);
        h.on_harness("m", codex_event("released", 4_000));
        assert!(h.pane_agent("s").is_none());
        assert_eq!(h.inner.lock().unwrap().panes["s"].transcript, None);
    }

    #[tokio::test]
    async fn progress_activity_clock_survives_heartbeats_and_resets_with_activity_and_session() {
        let (h, _tmp) = host_with_pane();
        let report = |at_ms, percent, text: &str| {
            let mut ev = codex_event("progress", at_ms);
            ev.percent = percent;
            ev.message = Some(text.into());
            h.on_harness("m", ev);
            h.inner.lock().unwrap().panes["s"].agent.clone().unwrap()
        };
        assert_eq!(report(1000, Some(20), "구현 중").progress_since_ms, Some(1000));
        let a = report(2000, Some(30), "구현 중");
        assert_eq!((a.progress_since_ms, a.progress_at_ms, a.percent), (Some(1000), Some(2000), Some(30)));
        let a = report(1500, Some(25), "늦은 옛 활동");
        assert_eq!(
            (a.progress.as_deref(), a.progress_since_ms, a.progress_at_ms),
            (Some("구현 중"), Some(1000), Some(2000))
        );
        assert_eq!(report(3000, None, "검증 중").progress_since_ms, Some(3000));
        report(4000, Some(50), crate::orch::WAITING);
        h.on_harness("m", codex_event("prompt_submit", 5000));
        assert_eq!(h.inner.lock().unwrap().panes["s"].agent.as_ref().unwrap().progress_since_ms, None);
        report(6000, Some(60), "수리 중");
        h.on_harness("m", codex_event("session_start", 7000));
        let a = h.inner.lock().unwrap().panes["s"].agent.clone().unwrap();
        assert_eq!((a.progress, a.progress_since_ms, a.progress_at_ms), (None, None, None));
    }

    /// thread keys의 글+Enter는 Codex의 paste 판정 창을 넘긴 뒤 제출한다.
    /// 가상 시계와 실제 Agent link 경계로 바이트·순서를 본다(실제 시간을 재지 않는다).
    #[tokio::test(start_paused = true)]
    async fn thread_keys_separates_paste_from_enter() {
        use aaa_common::conn::{FrameRx, spawn_writer};
        use aaa_proto::{AgentResponse, LinkMsg, Role, Snapshot, ThreadKind};
        for (paste, trust_after_text) in [(false, false), (true, false), (true, true)] {
            let (h, _tmp) = host_with_pane();
            h.project_create("demo", Default::default(), "test").unwrap();
            let mut r = ThreadRec::starting("demo", "입력", "m", "codex", Role::Worker, ThreadKind::Dir, 1);
            r.id = "t-0001".into();
            r.pane = Some("s".into());
            r.status = ThreadStatus::Open;
            r.brief = BriefState::Delivered;
            h.db.thread_import("demo", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
            h.on_subscribed(
                "s",
                Some(Snapshot {
                    session: "s".into(),
                    offset: 0,
                    cols: 80,
                    rows: 24,
                    replay: B64(if paste { b"\x1b[?2004h".to_vec() } else { vec![] }),
                    history_more: false,
                }),
                0,
            );
            let (writer, reader) = tokio::io::duplex(65536);
            let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
            h.inner.lock().unwrap().machines.insert(
                "m".into(),
                crate::MachineRt {
                    name: "mac-a".into(),
                    local: true,
                    os: None,
                    boot_id: None,
                    dev_mode: true,
                    conn: Some(conn.clone()),
                    last_seen_ms: None,
                    health: None,
                    health_sent: None,
                    features: vec![],
                    identity: None,
                    agent_pid: None,
                    colors: None,
                    build: Default::default(),
                },
            );
            let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
            let captured = seen.clone();
            let fake = tokio::spawn(async move {
                let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
                while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                    let reply = match body {
                        AgentRequest::Screen { .. } => {
                            let screen = if trust_after_text && !captured.lock().unwrap().is_empty() {
                                "Trust this folder?\n› 1. Trust and continue\n  2. Open restricted"
                            } else {
                                "› Ready"
                            };
                            AgentResponse::Screen(aaa_proto::ScreenText {
                                session: "s".into(),
                                cols: 80,
                                rows: 24,
                                lines: lines(screen),
                                cursor_row: 0,
                                cursor_col: 0,
                                ansi: screen.into(),
                            })
                        }
                        AgentRequest::Input { data, .. } => {
                            captured.lock().unwrap().push((tokio::time::Instant::now(), data.0));
                            AgentResponse::Done
                        }
                        _ => AgentResponse::Done,
                    };
                    if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                        tx.send(aaa_proto::Outcome::Ok(reply)).unwrap();
                    }
                }
            });
            let result = h.thread_keys("demo", &r.id, &["enter".into()], Some("이어서 계속하라".into()), "test").await;
            fake.abort();
            let got = seen.lock().unwrap();
            if trust_after_text {
                assert!(result.unwrap_err().message.contains("trust_screen"));
                assert_eq!(got.len(), 1, "치는 동안 뜬 신뢰 창에 Enter를 보내면 안 된다");
                continue;
            }
            result.unwrap();
            assert_eq!(got.len(), 2);
            assert!(got[1].0.duration_since(got[0].0) >= Duration::from_millis(300), "Enter가 paste 판정 창 안에 갔다");
            let expected = if paste { "\x1b[200~이어서 계속하라\x1b[201~" } else { "이어서 계속하라" };
            assert_eq!(got[0].1, expected.as_bytes());
            assert_eq!(got[1].1, b"\r");
        }
    }

    /// 실제 메시지·Next 전송 owner를 통과한다. 가짜 Codex는 입력 받음 뒤 잠깐 idle 화면을 낸다.
    /// hook이 없어도 이미 답한 기다림이 되살아나지 않으며, 새 기다림·실제 질문은 숨기지 않는다.
    #[tokio::test]
    async fn codex_message_and_next_do_not_revive_previous_waiting_report() {
        use aaa_common::conn::{FrameRx, spawn_writer};
        use aaa_proto::{AgentResponse, LinkMsg, Role, ThreadGroup, ThreadKind};
        for hooked in [false, true] {
            for next in [false, true] {
                let (h, _tmp) = host_with_pane();
                h.project_create("demo", Default::default(), "test").unwrap();
                let mut r = ThreadRec::starting("demo", "시험", "m", "codex", Role::Worker, ThreadKind::Dir, 1);
                r.id = "t-0001".into();
                r.pane = Some("s".into());
                r.status = ThreadStatus::Open;
                r.brief = BriefState::Delivered;
                r.next = vec!["다음 작업".into()];
                h.db.thread_import("demo", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
                h.on_harness("m", codex_event("idle", 1));
                let mut waiting = codex_event("progress", 2);
                waiting.message = Some(crate::orch::WAITING.into());
                h.on_harness("m", waiting.clone());
                assert_eq!(h.thread_view(&r).group, ThreadGroup::WaitingOnYou);

                let (writer, reader) = tokio::io::duplex(65536);
                let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
                h.inner.lock().unwrap().machines.insert(
                    "m".into(),
                    crate::MachineRt {
                        name: "mac-a".into(),
                        local: true,
                        os: None,
                        boot_id: None,
                        dev_mode: true,
                        conn: Some(conn.clone()),
                        last_seen_ms: None,
                        health: None,
                        health_sent: None,
                        features: vec![],
                        identity: None,
                        agent_pid: None,
                        colors: None,
                        build: Default::default(),
                    },
                );
                let fake_host = h.clone();
                let fake = tokio::spawn(async move {
                    let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
                    let mut screen = "› Ask Codex to do anything\n? for shortcuts".to_string();
                    while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                        let reply = match body {
                            AgentRequest::Screen { .. } => AgentResponse::Screen(aaa_proto::ScreenText {
                                session: "s".into(),
                                cols: 80,
                                rows: 24,
                                lines: lines(&screen),
                                cursor_row: 0,
                                cursor_col: 0,
                                ansi: screen.clone(),
                            }),
                            AgentRequest::Input { data, .. } => {
                                if data.0 == b"\r" {
                                    if hooked {
                                        fake_host.on_harness("m", codex_event("prompt_submit", 3));
                                        assert!(
                                            fake_host.pane_agent("s").unwrap().progress.is_none(),
                                            "hook이 옛 기다림을 지운다"
                                        );
                                    }
                                    // 감지의 working이 받음 신호다. 곧바로 idle로 돌아온 중간 화면도 처리한다.
                                    fake_host.on_harness("m", codex_event("working", 4));
                                    fake_host.on_harness("m", codex_event("idle", 5));
                                } else {
                                    screen = format!("› {}\n? for shortcuts", String::from_utf8_lossy(&data.0));
                                }
                                AgentResponse::Done
                            }
                            _ => AgentResponse::Done,
                        };
                        if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                            tx.send(aaa_proto::Outcome::Ok(reply)).unwrap();
                        }
                    }
                });
                if next {
                    let response = h.thread_next("demo", &r.id, Some(1), None, "test").await.unwrap();
                    assert!(
                        matches!(response, aaa_proto::FrontResponse::Next { message: Some(m), .. } if m.state == DeliveryState::Accepted)
                    );
                } else {
                    let message =
                        h.thread_message("demo", &r.id, "계속".into(), MessageMode::Queue, "test").await.unwrap();
                    assert_eq!(message.state, DeliveryState::Accepted);
                }
                assert_ne!(h.thread_view(&r).group, ThreadGroup::WaitingOnYou, "hook={hooked}, Next={next}: 중간 idle");
                // 보고 확인 task가 끝나야 thread_pass가 이 thread를 셈한다. 완료 알림을 기다린다.
                tokio::time::timeout(Duration::from_secs(5), async {
                    while h.report_refresh.lock().unwrap().contains_key("demo/t-0001") {
                        h.tick_wake.notified().await;
                    }
                })
                .await
                .expect("보고 확인 완료");
                h.thread_pass("demo");
                assert_eq!(h.load("demo", &r.id).unwrap().last_group, Some(ThreadGroup::Idle));
                assert!(
                    !h.db.inbox_list(Some("demo"), false).unwrap().iter().any(|i| i.summary.contains("waiting on you"))
                );
                h.on_harness("m", codex_event("working", 6));
                assert_eq!(h.thread_view(&r).group, ThreadGroup::Working);
                // 새 턴의 새 기다림은 다시 보인다.
                waiting.at_ms = 7;
                h.on_harness("m", waiting);
                h.on_harness("m", codex_event("idle", 8));
                assert_eq!(h.thread_view(&r).group, ThreadGroup::WaitingOnYou);
                h.on_harness("m", codex_event("question", 9));
                assert!(h.open_question_of("s").is_some());
                fake.abort();
            }
        }
    }

    /// 계획 사건(agent plan.rs, 차이 목록 S5)은 계획·배경 수만 바꾸고 상태는 그대로 둔다. harness가 끝나면 배경 수는 지운다
    #[tokio::test]
    async fn plan_events_set_counts_without_changing_the_activity() {
        let (h, _t) = host_with_pane();
        let ev = |kind: &str, at_ms, plan: Option<aaa_proto::PlanSummary>, background| HarnessEvent {
            session: "s".into(),
            harness: "claude".into(),
            kind: kind.into(),
            harness_session: None,
            message: None,
            question_kind: None,
            percent: None,
            at_ms,
            transcript: None,
            plan,
            background,
            context: None,
            model_observation: None,
        };
        h.on_harness("m", ev("working", 1_000, None, None));
        let plan = aaa_proto::PlanSummary { done: 1, total: 3, current: Some("고치는 중".into()) };
        // 컨텍스트 사용량도 같은 사건으로 온다(plan §8.321)
        let context =
            aaa_proto::ContextUsage { used: 43_922, window: Some(200_000), percent: Some(22), compaction: None };
        let model =
            aaa_proto::ModelObservation { model: "gpt-6-astra".into(), effort: Some("high".into()), at_ms: 2_000 };
        h.on_harness(
            "m",
            HarnessEvent {
                context: Some(context.clone()),
                model_observation: Some(model.clone()),
                ..ev("plan", 2_000, Some(plan.clone()), Some(2))
            },
        );
        let a = h.pane_agent("s").unwrap();
        assert_eq!(
            (a.activity, a.since_ms, a.plan, a.background, a.context),
            (AgentActivity::Working, 1_000, Some(plan.clone()), Some(2), Some(context.clone()))
        );
        // 다른 상태 사건은 계획·컨텍스트를 지우지 않는다
        h.on_harness("m", ev("idle", 3_000, None, None));
        let a = h.pane_agent("s").unwrap();
        assert_eq!(a.model_observation, Some(model.clone()));
        assert_eq!(
            (a.activity, a.plan.as_ref(), a.background, a.context.as_ref()),
            (AgentActivity::Idle, Some(&plan), Some(2), Some(&context))
        );
        // harness가 끝나면 배경 작업과 그 세션의 컨텍스트가 지워진다
        h.on_harness("m", ev("exited", 4_000, None, None));
        let a = h.pane_agent("s").unwrap();
        assert_eq!((a.plan.as_ref(), a.background, a.context), (Some(&plan), None, None));
        // 계획이 없어졌다는 사건
        h.on_harness("m", ev("plan", 5_000, None, None));
        assert_eq!(h.pane_agent("s").unwrap().plan, None);
        assert_eq!(h.pane_agent("s").unwrap().model_observation, None);
        h.on_harness("m", HarnessEvent { model_observation: Some(model), ..ev("plan", 6_000, None, None) });
        h.on_harness("m", ev("session_start", 7_000, None, None));
        assert_eq!(h.pane_agent("s").unwrap().model_observation, None);
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
            plan: None,
            background: None,
            context: None,
            model_observation: None,
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

    /// 대화 정리(plan §8.309): 화면 감지의 `compacting`은 작업 중이고 정리 표지를 단다. 같은 작업 중이라 상태가 된 때(목록 차례)는
    /// 그대로다. 다음 상태 사건(작업 중·쉼)이 오면 표지를 지운다. 정리 중에는 대기 메시지 「지금 보내기」를 하지 않는다
    #[tokio::test]
    async fn compacting_is_working_with_a_marker_until_the_next_state() {
        let (h, _t) = host_with_pane();
        let state = |h: &Arc<Host>| h.pane_agent("s").map(|a| (a.activity, a.compacting, a.since_ms));
        h.on_harness("m", codex_event("working", 1_000));
        h.on_harness("m", codex_event("compacting", 2_000));
        assert_eq!(state(&h), Some((AgentActivity::Working, true, 1_000)));
        assert_eq!(h.steer_queued_data("s", "t"), Err(aaa_proto::SteerRefusal::Compacting));
        h.on_harness("m", codex_event("working", 3_000));
        assert_eq!(state(&h), Some((AgentActivity::Working, false, 1_000)));
        h.on_harness("m", codex_event("compacting", 4_000));
        h.on_harness("m", codex_event("idle", 5_000));
        assert_eq!(state(&h), Some((AgentActivity::Idle, false, 5_000)));
    }

    /// 같은 ms에 잇달아 만든 ID도 다르다(전에는 ULID 시각 부분이라 같아져 둘째 질문·메시지가 첫째 기록을 덮었다)
    #[test]
    fn ids_made_in_the_same_millisecond_differ() {
        let ids: std::collections::HashSet<String> = (0..1000).map(|_| short_id("q")).collect();
        assert_eq!(ids.len(), 1000);
    }
    /// host는 파일을 읽지 않는다. Reader의 같은 pane 권한/커서 중계와 구 agent 기능 경계를 소유한다.
    #[tokio::test(start_paused = true)]
    async fn chat_pages_relay_boundaries_without_reinterpreting_them_and_gate_old_agents() {
        use aaa_common::conn::{FrameRx, spawn_writer};
        use aaa_proto::{AgentResponse, ChatMessage, ChatRole, FrontRequest, FrontResponse, LinkMsg};
        let (h, _tmp) = host_with_pane();
        let (writer, reader) = tokio::io::duplex(65536);
        let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
        h.inner.lock().unwrap().machines.insert(
            "m".into(),
            crate::MachineRt {
                name: "mac-a".into(),
                local: true,
                os: None,
                boot_id: None,
                dev_mode: true,
                conn: Some(conn.clone()),
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: vec![],
                identity: None,
                agent_pid: None,
                colors: None,
                build: Default::default(),
            },
        );
        h.on_harness("m", codex_event("idle", 1));
        let denied = h
            .call(
                crate::DeviceRole::Reader,
                "test",
                FrontRequest::PaneChat {
                    pane: "s".into(),
                    thinking: false,
                    compact: false,
                    commands: false,
                    before: Some("older".into()),
                },
            )
            .await
            .unwrap_err();
        assert_eq!(denied.code, ErrorCode::UnsupportedVersion);
        let fake = tokio::spawn(async move {
            let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
            for (asked, cursor, history, text) in [
                (None, None, None, "옛 agent"),
                (None, Some("boundary"), Some("file"), "최신 쪽"),
                (Some("boundary"), None, Some("file"), "첫 쪽"),
                (None, Some("grown-boundary"), Some("file"), "파일이 자람"),
            ] {
                let LinkMsg::Req { id, body: AgentRequest::Chat { before, harness, compact, commands, .. }, .. } =
                    rx.control::<LinkMsg>().await.unwrap()
                else {
                    panic!("chat 요청이 아니다")
                };
                assert_eq!(harness, "codex");
                // 정리된 자리는 그것을 청한 front에만 싣는다(옛 front는 모르는 role로 답을 읽지 못한다)
                assert!(compact);
                // 명령 줄(plan §8.323)도 청한 front에만
                assert!(commands);
                assert_eq!(before.as_deref(), asked);
                let reply = AgentResponse::Chat {
                    messages: vec![ChatMessage {
                        id: history.map(|_| text.into()),
                        role: ChatRole::Assistant,
                        text: text.into(),
                        pending: false,
                        receipt: None,
                    }],
                    truncated: cursor.is_some(),
                    before: cursor.map(str::to_string),
                    history: history.map(str::to_string),
                };
                conn.pending.lock().unwrap().remove(&id).unwrap().send(aaa_proto::Outcome::Ok(reply)).unwrap();
            }
        });
        for (index, (before, expected_cursor, expected_text)) in [
            (None, None, "옛 agent"),
            (None, Some("boundary"), "최신 쪽"),
            (Some("boundary"), None, "첫 쪽"),
            (None, Some("grown-boundary"), "파일이 자람"),
        ]
        .into_iter()
        .enumerate()
        {
            if index == 1 {
                h.inner.lock().unwrap().machines.get_mut("m").unwrap().features.push("chat_pages.v1".into());
            }
            let response = h
                .call(
                    crate::DeviceRole::Reader,
                    "test",
                    FrontRequest::PaneChat {
                        pane: "s".into(),
                        thinking: false,
                        compact: true,
                        commands: true,
                        before: before.map(str::to_string),
                    },
                )
                .await
                .unwrap();
            let FrontResponse::Chat(view) = response else { panic!("chat 응답이 아니다") };
            assert_eq!(view.before.as_deref(), expected_cursor);
            assert_eq!(view.messages[0].text, expected_text);
            assert_eq!(view.truncated, expected_cursor.is_some());
            assert_eq!(view.history.as_deref(), if index == 0 { None } else { Some("file") });
        }
        fake.await.unwrap();
    }
}
