//! harness 상태·확인창 질문·메시지 API(M1-3b, §8.3~8.5).
//!
//! - agent가 보낸 사건(`HarnessEvent`)으로 pane별 상태를 바꾸고 front에 알린다. polling하지 않는다.
//!   상태(working·idle·question)는 agent의 화면 감지가 낸다(herdr 0.9.5, plan §8.110). hook 사건은 harness 세션 ID와
//!   진행 보고 정리에만 쓴다. 옛 agent의 hook 상태 사건(turn_start·turn_end 등)도 그대로 받는다.
//! - 질문은 한 pane에 열린 것을 하나로 묶는다(brief 안내 중 화면에서 연 질문과 화면 감지 질문이 같은 창이다).
//!   답은 질문이 아직 열려 있고 세션이 살아 있는지 다시 본 뒤 adapter 키(기본 선택 Enter, 거절 Esc)로 보낸다.
//! - 입력(brief 안내·메시지)은 harness가 작업 중이나 질문으로 보이면 accepted다(herdr-projects `agent_prompt_confirmed`:
//!   `--until working --until blocked`). 상태를 모르는 harness는 화면 변화로 본다.
//!   일하는 중인 harness에 보낸 메시지는 같은 실행의 새 본문·큐 영수증으로 확인한다. 이전 Working은 근거가 아니다.
//!   확인하지 못하면 unknown으로 남기고 **자동으로 다시 보내지 않는다**(§8.5 a).

mod input;
mod questions;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{
    AgentActivity, AgentRequest, AgentStatus, B64, BriefState, DeliveryState, ErrorBody, ErrorCode, HarnessEvent,
    MenuPick, MessageInput, MessageMode, MessageView, QuestionMenu, QuestionState, QuestionView, ThreadStatus,
};
use tokio::sync::oneshot;

use crate::answer_delivery::{CardSend, PendingAnswer};
use crate::orch::ThreadRec;
use crate::prompt_receipt::{PreviousMessage, ReceiptProbe};
use crate::{Host, err};

struct Submission<'a> {
    id: &'a str,
    message: bool,
    nudge: bool,
    nudge_input: NudgeInput,
    previous_message: PreviousMessage,
    input: MessageInput,
    late: Option<LateReceipt>,
    card: Option<CardSend<'a>>,
    limit_wait: bool,
    resume_claimed: bool,
}

struct LateReceipt {
    watch: crate::InputWatch,
    probe: ReceiptProbe,
    waiting_at: Option<u64>,
}

impl<'a> Submission<'a> {
    fn new(id: &'a str) -> Self {
        Self {
            id,
            message: false,
            nudge: false,
            nudge_input: NudgeInput::default(),
            previous_message: PreviousMessage::None,
            input: MessageInput::NotWritten,
            late: None,
            card: None,
            limit_wait: false,
            resume_claimed: false,
        }
    }
}

#[derive(Default)]
pub(crate) struct NudgeInput {
    pub(crate) id: String,
    pub(crate) reason: Option<&'static str>,
    pub(crate) pasted: Option<Instant>,
    pub(crate) entered: Option<Instant>,
}

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
    "Navigate · f full diff",
    "Navigate · tab Amend",
    "Navigate · enter Confirm",
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

/// Claude의 한도 메뉴와 자동 재개 대기 화면. 옛 출력이 남아도 새 입력란이 열리면 대기가 끝난 것이다.
pub(crate) fn limit_wait_screen(lines: &[String]) -> bool {
    let bottom: Vec<_> = lines.iter().rev().filter(|line| !line.trim().is_empty()).take(8).collect();
    let shown = bottom.iter().rev().map(|line| line.trim()).collect::<Vec<_>>().join(" ");
    let wait_option =
        shown.contains("Wait here, then continue automatically") || shown.contains("Stop and wait for limit to reset");
    (wait_option && (dialog(lines).is_some() || shown.contains("Enter to select")))
        || (shown.contains("Claude Code will continue automatically")
            && shown.contains("Press esc to cancel the wait")
            && !crate::inbox::prompt_empty(lines))
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

/// 원격 시계는 host 시각과 비교하지 않는다. 같은 agent의 실행 세대·native 시각끼리만 대조한다.
fn provider_turn_after(st: &AgentStatus, ev: &HarnessEvent, pane_started_ms: u64) -> Option<aaa_proto::ProviderTurn> {
    let n = ev.provider_turn.as_ref()?;
    if !matches!(ev.harness.as_str(), "claude" | "codex")
        || ev.harness != st.harness
        || Some(n.harness_session.as_str()) != st.harness_session.as_deref()
        || ev.harness_session.as_deref() != st.harness_session.as_deref()
        || !aaa_common::util::harness_session_ok(&n.harness_session)
        || n.generation_ms < pane_started_ms
        || n.started_ms < n.generation_ms
        || n.at_ms < n.started_ms
        || ev.at_ms != n.at_ms
        || n.finished_ms.is_some_and(|end| end != n.at_ms)
        || (n.error.is_some() && n.finished_ms.is_none())
    {
        return None;
    }
    if let Some(old) = &st.provider_turn
        && (n.generation_ms < old.generation_ms
            || n.at_ms < old.at_ms
            || (n.at_ms == old.at_ms
                && (old.finished_ms.is_some() || n.finished_ms.is_none() || old.at_ms > old.started_ms))
            || (n.generation_ms == old.generation_ms && n.started_ms < old.started_ms))
    {
        return None;
    }
    Some(n.clone())
}

/// hook의 제출·실행 경계를 빈 native 관측으로 남겨 이전 완료 오류의 재전송을 막는다.
fn provider_boundary(st: &AgentStatus, at_ms: u64, generation_ms: u64) -> Option<aaa_proto::ProviderTurn> {
    Some(aaa_proto::ProviderTurn {
        generation_ms,
        harness_session: st.harness_session.clone()?,
        started_ms: generation_ms,
        at_ms,
        finished_ms: None,
        error: None,
    })
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
        "agy_starting" if ev.harness == "agy" => Some(AgentActivity::Started),
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
        let rows = self
            .db
            .threads_for_pane(pane)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        rows.iter()
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .find(|r| r.pane.as_deref() == Some(pane) && r.status != ThreadStatus::Resolved)
    }

    /// agent가 harness 사건을 보냈다
    pub(crate) fn on_harness(self: &Arc<Self>, mid: &str, mut ev: HarnessEvent) {
        if ev.harness == "agy" && ev.kind == "agy_receipt" {
            self.agy_brief_receipt(mid, &ev);
            return;
        }
        if ev.kind == "turn_reply" {
            if let Err(error) = self.check_question_card(mid, &ev) {
                tracing::warn!(pane = %ev.session, "질문 카드 누락 검사 실패: {error:#}");
            }
            return;
        }
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
            if ev.kind == "codex_speed" {
                if ev.harness == "codex"
                    && let Some(speed) = ev.codex_speed
                    && p.info.state.is_running()
                    && ev.at_ms >= p.info.started_at_ms
                    && p.codex_speed.is_none_or(|(_, at_ms)| ev.at_ms >= at_ms)
                {
                    p.codex_speed = Some((speed, ev.at_ms));
                    drop(i);
                    self.thread_changed(&ev.session);
                }
                return;
            }
            // native 표시만 갱신한다. 화면 감지의 활동·질문·승인은 바꾸지 않는다.
            if ev.kind == "provider_turn"
                && p.agent.as_ref().and_then(|st| provider_turn_after(st, &ev, p.info.started_at_ms)).is_none()
            {
                return;
            }
            // 셸 pane 앞의 agent가 끝나 셸로 돌아왔다(agent의 앞 프로세스 찾기, herdr `ClearAgent`): 다시 agent 없는 셸이다.
            // 재부팅 복원도 그 셸에 대화 이어가기를 치지 않는다(herdr는 놓은 agent를 세션에 남기지 않는다)
            if ev.kind == "released" {
                self.bgwait.lock().unwrap().forget(&ev.session);
                p.agent = None;
                p.codex_speed = None;
                p.turn_from = None;
                p.turn_reported = false;
                p.alert = None;
                p.answered = None;
                p.transcript = None;
                aaa_common::diagnostic::ignored(
                    p.events.send(crate::PaneEvent::State),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
                drop(i);
                self.bump();
                self.thread_changed(&ev.session);
                self.close_questions_of(&ev.session);
                self.wake_nudge();
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
                provider_turn: None,
            });
            if ev.harness != "report" {
                st.harness = ev.harness.clone();
            }
            if ev.kind != "provider_turn" && (ev.kind == "session_start" || ev.harness_session.is_some()) {
                st.harness_session = ev.harness_session.clone();
            }
            if ev.kind == "provider_turn" {
                st.provider_turn = provider_turn_after(&st, &ev, p.info.started_at_ms);
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
                let generation = ev.provider_turn.as_ref().map_or(ev.at_ms, |n| n.generation_ms);
                st.provider_turn = provider_boundary(&st, generation, generation);
            }
            if ev.kind == "session" && ev.harness_session != prev.as_ref().and_then(|a| a.harness_session.clone()) {
                st.model_observation = None;
                let generation = ev.provider_turn.as_ref().map_or(p.info.started_at_ms, |n| n.generation_ms);
                st.provider_turn = provider_boundary(&st, generation, generation);
            }
            if ev.kind == "prompt_submit" && st.provider_turn.as_ref().is_none_or(|n| ev.at_ms >= n.at_ms) {
                let generation = st.provider_turn.as_ref().map_or(p.info.started_at_ms, |n| n.generation_ms);
                st.provider_turn = provider_boundary(&st, ev.at_ms, generation);
            }
            // 사용자가 답했다: 옛 「Waiting for you」는 더 맞지 않다(herdr-projects `respond` UserPromptSubmit)
            if matches!(ev.kind.as_str(), "prompt_submit" | "turn_start")
                && st.progress.as_deref() == Some(crate::orch::WAITING)
            {
                st.progress = None;
                st.progress_at_ms = None;
                st.progress_since_ms = None;
            }
            // brief 화면 읽기가 먼저 질문을 열고 답할 수 있다. 아직 Question을 관측하지 않았다면
            // 종류·본문이 같은 첫 사건은 답한 그 창이다. 이미 관측한 막힘의 재질문은 agent 계약을 따른다.
            let was_question = prev.as_ref().is_some_and(|a| a.activity == AgentActivity::Question);
            let answered_block = next == Some(AgentActivity::Question)
                && p.answered.as_ref().is_some_and(|(kind, text)| {
                    if was_question {
                        !anew
                    } else {
                        kind == ev.question_kind.as_deref().unwrap_or("permission")
                            && text == ev.message.as_deref().unwrap_or_default()
                    }
                });
            if next.is_some_and(|a| a != AgentActivity::Question)
                || (next == Some(AgentActivity::Question) && !was_question && !answered_block)
            {
                p.answered = None;
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
            aaa_common::diagnostic::ignored(
                p.events.send(crate::PaneEvent::State),
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
            (next, answered_block, p.info.state.is_running(), finished)
        };
        if matches!(activity, Some(AgentActivity::Idle | AgentActivity::Started)) {
            self.wake_nudge();
        }
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
            aaa_common::diagnostic::ignored(tx.send(ev.kind.clone()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
        }
        // 질문: 열리거나, harness가 넘어가면 닫힌다. 답한 확인창 뒤에 늦게 온 질문 사건(옛 agent의 Claude
        // Notification `permission_prompt`가 답보다 늦게 도착)은 그 질문을 다시 열지 않는다. 참고 herdr 0.9.5는 상태 hook을
        // 아예 버리고(`integration/assets/claude/herdr-agent-state.sh`: `session` 말고는 exit 0) 화면 감지는 막힘에 들어설
        // 때만 알린다. 옛 agent는 도구마다 PreToolUse·PostToolUse(작업 중)를 내므로 다음 확인창은 새 막힘이다(t-0333 m13
        // 사건 16, plan §8.99·§8.110). `ask_anew.v1` agent는 화면 감지만 질문을 내고, 막힘에 입력이 간 뒤 남은 확인창을
        // 새 물음으로 다시 알린다(herdr-web-ui `askingEnded`, QA t-0337 Q18). 단 brief 화면 읽기로 이미 답한
        // 창의 첫 관측은 같은 종류·본문이면 기존 답에 묶는다(plan §8.529).
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
        self.thread_changed(&ev.session);
    }
}

#[cfg(test)]
mod tests;
