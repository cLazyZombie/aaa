//! thread brief 안내를 한 번만, 정말 받을 수 있을 때만 보낸다. herdr-projects 0.2.35 `src/brief.rs`(`deliver`·`step`)를
//! 옮겼다(plan §8.77). 보내는 쪽은 둘이다: 시작·다시 시작 뒤 도는 배경 task(`Sender::Ticker`)와 `aaa thread brief`(`Manual`).
//!
//! - **준비:** harness가 입력을 받을 상태(started·idle)이고, 읽을 수 있는 입력창이면 비어 있고, ticker는 상태와 화면이
//!   `SETTLE`(3초) 그대로일 때. 시작 화면이 잠깐 idle로 읽혀도 글을 받지 않는다.
//! - **보냈다:** harness가 받았다는 사건(hook, 없으면 Enter 뒤 화면 변화)을 본 때다. 입력이 끝난 것이 아니다.
//! - **다시 보내기 전에는 화면이 정한다:** 대화에 그 줄이 보이고 입력창에 없으면 받은 것, 입력창에 남았으면 Enter만,
//!   입력창이 비었고 줄이 어디에도 없으면 다시 친다. 그 밖에는 기다린다. 남은 글 위에 다시 치지 않는다.
//! - ticker는 `MAX_ATTEMPTS`(3)번까지 보내고, 그 뒤에는 brief를 `unknown`으로 두고 흐름 사건 `brief_unknown`을 낸다
//!   (inbox 「brief not delivered」). `aaa thread brief`는 그 뒤에도 보낸다.
//!
//! 참고와 다르게 둔 것:
//! - herdr는 모든 agent의 상태를 화면으로 안다. aaa는 hook(과 D8의 화면 감지)으로만 알아서, 상태가 없는 harness는
//!   입력을 받을 상태로 보고 화면이 멈추기만 기다린다(전과 같다).
//! - 참고의 claim lease(60초)는 보내는 프로세스가 죽는 경우를 위한 것이다. aaa는 host 프로세스 안의 task라 메모리
//!   claim만 둔다(host가 다시 뜨면 비어 있다).
//! - 확인창(`harness::modal_of`)이 보이면 입력하지 않고 질문으로 올린다(aaa 원격 응답, §8.15).

use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{AgentActivity, BriefState, ErrorBody, ErrorCode, ThreadStatus};

use crate::prompt_box;
use crate::{Host, err};

pub(crate) const SETTLE: Duration = Duration::from_secs(3);
pub(crate) const MAX_ATTEMPTS: u32 = 3;
/// 배경 task가 화면을 다시 보는 간격
const LOOK_EVERY: Duration = Duration::from_millis(250);

/// ticker가 치기 전에 화면이 그대로여야 하는 시간. `AAA_TEST_BRIEF_SETTLE_MS`는 시험에서만 줄인다(가짜 agent 화면은 뜨자마자
/// 멈춘다: 웹 E2E는 thread마다 3초를 기다렸다)
fn settle() -> Duration {
    std::env::var("AAA_TEST_BRIEF_SETTLE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(SETTLE, Duration::from_millis)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sender {
    Ticker,
    Manual,
}

/// 한 걸음에 보는 것
pub(crate) struct Look<'a> {
    pub activity: Option<AgentActivity>,
    /// launch의 harness(prompt_box kind)
    pub kind: &'a str,
    /// `ScreenText.ansi`
    pub screen: &'a str,
    /// 늘 한 화면 줄에 들어가는 안내 줄의 앞부분(`.aaa/<p>-<t>/brief.md`)
    pub marker: &'a str,
    pub attempts: u32,
    pub sender: Sender,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    /// 아직 아니다. `fresh`면 settle 기억을 지운다(다시 처음부터 잰다).
    Wait { why: String, fresh: bool },
    /// 받았다(화면에 줄이 있고 입력창에 없다)
    Delivered,
    /// 신뢰 창이다. Enter가 그것을 받아들이므로 아무것도 입력하지 않는다.
    Trust(&'static str),
    /// 시도가 다 됐다(ticker만)
    GiveUp(String),
    /// 입력창에 남은 줄에 Enter만 보낸다
    Enter,
    /// 줄을 친다(ticker는 settle 뒤)
    Type,
}

fn squeeze(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

fn wait(why: impl Into<String>, fresh: bool) -> Step {
    Step::Wait { why: why.into(), fresh }
}

fn state_name(a: Option<AgentActivity>) -> &'static str {
    match a {
        None => "상태를 모름",
        Some(AgentActivity::Started) => "started",
        Some(AgentActivity::Working) => "working",
        Some(AgentActivity::Idle) => "idle",
        Some(AgentActivity::Question) => "question",
        Some(AgentActivity::Error) => "error",
        Some(AgentActivity::Exited) => "exited",
    }
}

/// herdr-projects brief.rs `step`의 판정(부수 효과 없이). settle은 부르는 쪽이 잰다.
pub(crate) fn step(l: &Look) -> Step {
    if l.activity == Some(AgentActivity::Question) {
        return wait("확인창에 답을 기다린다", true);
    }
    // 상태를 모르는 harness는 화면이 멈추기만 기다린다(모듈 설명)
    let ready = l.activity.is_none_or(|a| matches!(a, AgentActivity::Started | AgentActivity::Idle));
    if l.attempts == 0 && !ready {
        return wait(format!("agent가 {}", state_name(l.activity)), true);
    }
    if let Some(phrase) = crate::trust_screen::detect(l.kind, l.screen) {
        return Step::Trust(phrase);
    }
    let known = prompt_box::knows(l.kind);
    let in_box = prompt_box::box_text(l.kind, l.screen);
    let line = squeeze(l.marker);
    let shown = squeeze(&prompt_box::plain(l.screen)).contains(&line);
    let out_of_tries = l.sender == Sender::Ticker && l.attempts >= MAX_ATTEMPTS;
    let give_up = || Step::GiveUp(format!("{MAX_ATTEMPTS}번 보냈지만 받았다는 확인이 없었다"));

    if l.attempts > 0 {
        let in_box_has_line = in_box.as_deref().is_some_and(|t| squeeze(t).contains(&line));
        // 화면에 있고 입력창에 없다: 받았다. 입력창을 못 읽는 harness는 일하기 시작했을 때만 받은 것으로 친다.
        let taken = shown && !in_box_has_line && (in_box.as_deref() == Some("") || (!known && !ready));
        match in_box.as_deref() {
            _ if taken => return Step::Delivered,
            // 입력창에 남았다: 다시 치지 않고 Enter만
            Some(_) if in_box_has_line && ready => {
                return if out_of_tries { give_up() } else { Step::Enter };
            }
            // 입력창이 비었고 줄이 어디에도 없다: 사라졌으니 다시 친다
            Some("") if ready => {}
            // 이 harness의 입력창은 읽을 수 없다: 사람만 정한다
            None if !known => {
                if l.sender == Sender::Manual {
                    if shown {
                        return Step::Delivered;
                    }
                } else {
                    return Step::GiveUp("입력창을 읽을 수 없어 앞 시도가 남았는지 확인하지 못한다".into());
                }
            }
            _ => {
                return wait(
                    format!("agent가 {}이고 앞 시도가 입력창에 남았을 수 있다", state_name(l.activity)),
                    false,
                );
            }
        }
    } else if known && in_box.as_deref() != Some("") {
        let why = if in_box.is_none() {
            "입력창이 화면에 없다(메뉴나 시작 화면?)"
        } else {
            "입력창에 글이 있다"
        };
        return wait(why, true);
    }
    if out_of_tries {
        return give_up();
    }
    Step::Type
}

/// 화면 글의 작은 고정 hash(FNV-1a, herdr-projects brief.rs `fnv`)
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf29ce484222325, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3))
}

/// 한 걸음의 결과
enum Done {
    Delivered(&'static str),
    Stuck(String),
    /// 계속 본다(까닭)
    Not(String),
}

impl Host {
    /// 배경 task: brief를 받거나, 시도가 다 되거나, 세션·thread가 끝날 때까지 본다. thread마다 하나만 돈다.
    pub(crate) fn spawn_brief(self: &Arc<Self>, project: &str, id: &str) {
        let key = format!("{project}/{id}");
        if !self.brief_loops.lock().unwrap().insert(key.clone()) {
            return;
        }
        let me = self.clone();
        let (p, t) = (project.to_string(), id.to_string());
        tokio::spawn(async move {
            me.brief_loop(&p, &t).await;
            me.brief_loops.lock().unwrap().remove(&key);
        });
    }

    async fn brief_loop(self: &Arc<Self>, project: &str, id: &str) {
        // settle: (상태가 바뀐 때, 화면 hash)와 그것을 처음 본 때
        let mut seen: Option<((u64, u64), Instant)> = None;
        // 이 확인창으로 연 질문. 확인창 하나에 질문은 한 번만 연다.
        let mut asked: Option<String> = None;
        loop {
            let Ok(rec) = self.load(project, id) else { return };
            if rec.status != ThreadStatus::Open || rec.brief != BriefState::Pending {
                return;
            }
            let Some(pane) = rec.pane.clone() else { return };
            if !self.pane_view_of(&pane, aaa_proto::DeviceRole::Local).is_some_and(|p| p.state.is_running()) {
                // 세션이 끝났다: 묶음이 확인 필요로 보이고, `thread restart`가 다시 띄운다
                return;
            }
            match self.brief_step(project, id, Sender::Ticker, &mut seen, &mut asked).await {
                Ok(Done::Not(_)) | Err(_) => {}
                Ok(Done::Delivered(_) | Done::Stuck(_)) => return,
            }
            tokio::time::sleep(LOOK_EVERY).await;
        }
    }

    /// 한 걸음(claim 아래). 받았으면 brief를 delivered로, 시도가 다 됐으면 unknown으로 저장하고 흐름 사건을 낸다.
    async fn brief_step(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        sender: Sender,
        seen: &mut Option<((u64, u64), Instant)>,
        asked: &mut Option<String>,
    ) -> Result<Done, ErrorBody> {
        let key = format!("{project}/{id}");
        if !self.brief_claims.lock().unwrap().insert(key.clone()) {
            return Err(err(ErrorCode::Unavailable, format!("{id}의 brief를 다른 쪽이 지금 보내는 중이다")));
        }
        let out = self.brief_step_claimed(project, id, sender, seen, asked).await;
        self.brief_claims.lock().unwrap().remove(&key);
        let rec = self.load(project, id)?;
        match &out {
            Ok(Done::Delivered(how)) => {
                let rec = self.update_rec(project, id, |r| {
                    r.brief = BriefState::Delivered;
                    r.delivered_at_ms = Some(aaa_common::util::now_ms());
                    r.delivered_report = r.report_hash.clone();
                })?;
                let evidence = rec.work_dir.as_ref().map(|w| format!("{w}/brief.md"));
                self.flow(&rec, "brief_delivered", evidence, format!("{how}(시도 {}번)", rec.brief_attempts));
            }
            Ok(Done::Stuck(why)) if rec.brief == BriefState::Pending => {
                let rec = self.update_rec(project, id, |r| r.brief = BriefState::Unknown)?;
                self.flow(&rec, "brief_unknown", None, why.clone());
            }
            _ => {}
        }
        // brief 상태는 묶음을 바꾼다(transitions.rs)
        self.tick_wake.notify_one();
        out
    }

    async fn brief_step_claimed(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        sender: Sender,
        seen: &mut Option<((u64, u64), Instant)>,
        asked: &mut Option<String>,
    ) -> Result<Done, ErrorBody> {
        let rec = self.load(project, id)?;
        if rec.brief == BriefState::Delivered {
            return Ok(Done::Delivered("이미 받았다"));
        }
        let pane = rec.pane.clone().ok_or_else(|| err(ErrorCode::BadRequest, "thread 세션이 없다"))?;
        let mid = rec.machine_id.clone();
        let Some(screen) = self.screen(&mid, &pane).await else {
            return Ok(Done::Not("화면을 읽지 못했다".into()));
        };
        // 확인창(폴더 신뢰·권한)에는 입력하지 않고 질문으로 올린다(aaa 원격 응답)
        if let Some((kind, text)) = crate::harness::modal_of(&screen.lines) {
            if asked.is_none() {
                *asked = Some(self.ask(&pane, kind, text).id);
            }
            *seen = None;
            return Ok(Done::Not("확인창에 답을 기다린다".into()));
        }
        if let Some(q) = asked.take()
            && let Some(mut open) = self.questions(false).into_iter().find(|x| x.id == q)
        {
            // 확인창이 사라졌다(답했거나 터미널에서 직접 골랐다)
            open.state = aaa_proto::QuestionState::Gone;
            self.save_question(&open);
        }
        let agent = self.pane_agent(&pane);
        let activity = agent.as_ref().map(|a| a.activity);
        let kind = rec.launch.as_ref().map(|l| l.harness.clone()).unwrap_or_default();
        let marker = format!(".aaa/{project}-{id}/brief.md");
        let look =
            Look { activity, kind: &kind, screen: &screen.ansi, marker: &marker, attempts: rec.brief_attempts, sender };
        let line = format!("{marker}를 읽고 그대로 하라.");
        match step(&look) {
            Step::Wait { why, fresh } => {
                if fresh {
                    *seen = None;
                }
                Ok(Done::Not(why))
            }
            Step::Trust(phrase) => {
                *seen = None;
                let by_user = self.trust_screens(project) == crate::trust_screen::USER;
                Ok(Done::Not(crate::trust_screen::refusal(project, id, &pane, phrase, by_user)))
            }
            Step::Delivered => Ok(Done::Delivered("화면에 받은 줄이 있다")),
            Step::GiveUp(why) => Ok(Done::Stuck(why)),
            Step::Enter => {
                self.bump_brief(project, id, 1)?;
                *seen = None;
                match self.send_enter(&mid, &pane).await {
                    Ok(true) => Ok(Done::Delivered("입력창에 남은 줄을 Enter로 보냈다")),
                    Ok(false) => Ok(Done::Not("입력창에 남은 줄에 Enter를 보냈다".into())),
                    Err(e) => Ok(Done::Not(format!("Enter를 보내지 못했다: {e}"))),
                }
            }
            Step::Type => {
                if sender == Sender::Ticker {
                    let k = (agent.as_ref().map_or(0, |a| a.since_ms), fnv(&prompt_box::plain(&screen.ansi)));
                    match seen {
                        Some((have, _)) if *have == k => {}
                        _ => {
                            *seen = Some((k, Instant::now()));
                            return Ok(Done::Not("화면이 멈추기를 기다린다".into()));
                        }
                    }
                    if seen.as_ref().is_some_and(|(_, at)| at.elapsed() < settle()) {
                        return Ok(Done::Not("화면이 멈추기를 기다린다".into()));
                    }
                }
                // 치기 전에 센다: 도중에 멈춰도 기록은 「입력창에 글이 있을 수 있다」를 말한다
                self.bump_brief(project, id, 1)?;
                *seen = None;
                match self.submit_checked(&mid, &pane, Some(&line), None).await {
                    Ok(true) => Ok(Done::Delivered("안내 줄을 보냈다")),
                    Ok(false) => Ok(Done::Not("받았다는 사건·화면 변화가 없었다. 다시 보내기 전에 화면을 본다".into())),
                    Err(e) if !e.typed => {
                        self.bump_brief(project, id, -1)?;
                        Ok(Done::Not(e.msg))
                    }
                    Err(e) => Ok(Done::Not(format!("받았는지 모른다({}). 다시 보내기 전에 화면을 본다", e.msg))),
                }
            }
        }
    }

    fn bump_brief(&self, project: &str, id: &str, by: i32) -> Result<(), ErrorBody> {
        self.update_rec(project, id, |r| r.brief_attempts = r.brief_attempts.saturating_add_signed(by)).map(|_| ())
    }

    /// `aaa thread brief`: 배경 task의 다음 확인을 기다리지 않고 지금 한 번 보낸다(두 번 보내지 않는다,
    /// herdr-projects threads.rs `brief`). settle과 시도 상한은 없다.
    pub(crate) async fn thread_brief(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        actor: &str,
    ) -> Result<aaa_proto::ThreadView, ErrorBody> {
        let bad = |m: String| err(ErrorCode::BadRequest, m);
        let rec = self.load(project, id)?;
        if rec.brief == BriefState::Delivered {
            return Err(bad(format!(
                "{id}는 이미 brief를 받았다. 더 보낼 것은 `aaa thread message {project} {id} --text …`로 보낸다"
            )));
        }
        if rec.status != ThreadStatus::Open {
            return Err(bad(format!("{id}는 {}다. `aaa thread restart`가 다시 띄운다", rec.status.as_str())));
        }
        let pane = rec.pane.clone().ok_or_else(|| bad("thread 세션이 없다".into()))?;
        self.check_running(&pane)?;
        let activity = self.pane_agent(&pane).map(|a| a.activity);
        if activity == Some(AgentActivity::Question) {
            return Err(bad(format!(
                "{id}의 pane에 확인창이 떠 있다. `aaa thread read {project} {id}`로 보고 답한 뒤 다시 `aaa thread brief`"
            )));
        }
        let ready = activity.is_none_or(|a| matches!(a, AgentActivity::Started | AgentActivity::Idle));
        if !ready && rec.brief_attempts == 0 {
            return Err(bad(format!(
                "{id}의 agent가 {}라 아직 brief를 받을 수 없다. 잠시 뒤 다시 한다",
                state_name(activity)
            )));
        }
        self.db.audit(actor, "thread_brief", &format!("{project}/{id}"));
        match self.brief_step(project, id, Sender::Manual, &mut None, &mut None).await? {
            Done::Delivered(_) => Ok(self.thread_show(project, id)?),
            Done::Not(why) | Done::Stuck(why) => Err(bad(format!(
                "{id}의 brief를 아직 보내지 못했다: {why}. `aaa thread read {project} {id}`로 pane을 본 뒤 다시 `aaa thread brief`"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    //! herdr-projects brief.rs 시험의 판정 줄을 옮겼다(화면은 prompt_box fixture와 같은 모양으로 만든다).
    use super::*;

    const MARK: &str = ".aaa/demo-t-0001/brief.md";

    fn rule() -> String {
        "─".repeat(40)
    }

    /// Claude 입력창: 위 테두리, `❯ <box>`, 아래 테두리. `above`는 대화 줄.
    fn claude(above: &str, boxed: &str) -> String {
        format!("{above}\n{}\n❯ {boxed}\n{}\n", rule(), rule())
    }

    fn look<'a>(screen: &'a str, attempts: u32, activity: Option<AgentActivity>, sender: Sender) -> Look<'a> {
        Look { activity, kind: "claude", screen, marker: MARK, attempts, sender }
    }

    const IDLE: Option<AgentActivity> = Some(AgentActivity::Idle);
    const LINE: &str = ".aaa/demo-t-0001/brief.md를 읽고 그대로 하라.";

    #[test]
    fn first_try_waits_for_a_ready_agent_and_an_empty_box() {
        let empty = claude("", "");
        assert_eq!(step(&look(&empty, 0, IDLE, Sender::Ticker)), Step::Type);
        assert_eq!(step(&look(&empty, 0, Some(AgentActivity::Started), Sender::Ticker)), Step::Type);
        assert!(matches!(step(&look(&empty, 0, Some(AgentActivity::Working), Sender::Ticker)), Step::Wait { .. }));
        assert!(matches!(step(&look(&empty, 0, Some(AgentActivity::Question), Sender::Ticker)), Step::Wait { .. }));
        // 사람이 쓰던 글, 입력창이 없는 화면(메뉴·시작 화면)
        let draft = claude("", "쓰던 글");
        assert_eq!(step(&look(&draft, 0, IDLE, Sender::Ticker)), wait("입력창에 글이 있다", true));
        assert_eq!(
            step(&look("Welcome\n", 0, IDLE, Sender::Ticker)),
            wait("입력창이 화면에 없다(메뉴나 시작 화면?)", true)
        );
        // 흐린 placeholder는 빈 입력창이다
        let placeholder = claude("", "\x1b[2mTry \"fix lint\"\x1b[0m");
        assert_eq!(step(&look(&placeholder, 0, IDLE, Sender::Ticker)), Step::Type);
    }

    #[test]
    fn retries_look_at_the_screen_first() {
        // 대화에 있고 입력창은 비었다: 받았다
        let taken = claude(&format!("❯ {LINE}\n⏺ 읽는 중"), "");
        assert_eq!(step(&look(&taken, 1, IDLE, Sender::Ticker)), Step::Delivered);
        // 입력창에 남았다: Enter만(다시 치지 않는다)
        let left = claude("", LINE);
        assert_eq!(step(&look(&left, 1, IDLE, Sender::Ticker)), Step::Enter);
        assert_eq!(
            step(&look(&left, 1, Some(AgentActivity::Working), Sender::Ticker)),
            wait("agent가 working이고 앞 시도가 입력창에 남았을 수 있다", false)
        );
        // 비었고 줄이 어디에도 없다: 다시 친다
        assert_eq!(step(&look(&claude("", ""), 1, IDLE, Sender::Ticker)), Step::Type);
        // 세 번 뒤에는 ticker가 멈춘다. 사람은 계속 보낼 수 있다.
        assert!(matches!(step(&look(&claude("", ""), 3, IDLE, Sender::Ticker)), Step::GiveUp(_)));
        assert!(matches!(step(&look(&left, 3, IDLE, Sender::Ticker)), Step::GiveUp(_)));
        assert_eq!(step(&look(&claude("", ""), 3, IDLE, Sender::Manual)), Step::Type);
        assert_eq!(step(&look(&left, 5, IDLE, Sender::Manual)), Step::Enter);
    }

    #[test]
    fn an_unreadable_box_is_for_a_person_to_decide() {
        let fake = format!("fake> {LINE}\n");
        let l = |sender| Look { activity: None, kind: "custom", screen: &fake, marker: MARK, attempts: 1, sender };
        assert!(matches!(step(&l(Sender::Ticker)), Step::GiveUp(_)));
        // 사람이 보았고 줄이 화면에 있다
        assert_eq!(step(&l(Sender::Manual)), Step::Delivered);
        let blank = "fake> \n".to_string();
        let l =
            Look { activity: None, kind: "custom", screen: &blank, marker: MARK, attempts: 1, sender: Sender::Manual };
        assert_eq!(step(&l), Step::Type);
        // 처음은 입력창을 몰라도 친다(상태를 모르는 harness는 화면 settle만)
        let l =
            Look { activity: None, kind: "custom", screen: &blank, marker: MARK, attempts: 0, sender: Sender::Ticker };
        assert_eq!(step(&l), Step::Type);
    }

    #[test]
    fn a_trust_screen_gets_nothing() {
        let trust = "Quick safety check: Is this a project you created or one you trust?\n❯ 1. Yes, I trust this folder\n  2. No, exit\n";
        assert_eq!(
            step(&look(trust, 0, IDLE, Sender::Ticker)),
            Step::Trust("Quick safety check: Is this a project you created or one you trust")
        );
    }
}
