//! 세션 하나의 감지 차례(agent pump가 돌린다). herdr 감지 루프(`src/pane.rs` 947~1240줄)는 한 task 안에서 화면을 읽고
//! 평가하지만, aaa에서는 화면을 session host가 가지므로 「읽을 요청을 낸다(`due`) → 응답을 평가한다(`answer`)」로 나눴다.
//!
//! - 응답을 기다리는 동안은 다음 차례를 세지 않는다(요청은 한 번에 하나). 다음 차례는 응답을 평가한 뒤 `interval`이다.
//! - SESSION_IPC 1.1 session host에는 `Detect`(화면 끝 글·OSC·화면 순번)를, 그 전 session host에는 `Screen`(보이는 화면)을
//!   보낸다. 옛 session host는 모르는 요청을 받으면 연결을 끊는다.
//! - herdr는 상태와 「보이는 표지」가 바뀔 때 알리지만 aaa host는 상태만 안다: 상태나 질문 본문이 바뀔 때만 사건을 낸다.

use std::time::Instant;

use aaa_proto::{AgentResponse, Outcome, SessionRequest};

use super::{DetectionInput, Detector, Harness, Published, State, Tick, question_of, text_from_screen};

/// 알릴 사건(`HarnessEvent`의 kind·본문·질문 종류)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// working·idle·question
    pub kind: &'static str,
    pub message: Option<String>,
    pub question_kind: Option<&'static str>,
    /// 맞은 감지 규칙(log용)
    pub rule: Option<String>,
}

#[derive(Debug)]
pub struct Watch {
    detector: Detector,
    /// session host가 `Detect`를 받는가(SESSION_IPC 1.1부터)
    detect_ipc: bool,
    /// 응답을 기다리는 요청 ID
    in_flight: Option<String>,
    n: u64,
    next: Instant,
    /// 마지막으로 알린 (상태, 질문 본문)
    sent: Option<(State, Option<String>)>,
}

impl Watch {
    /// `started_at`: harness를 띄운 때(시작 유예 기준), `now`: 첫 차례
    pub fn new(harness: Harness, started_at: Instant, detect_ipc: bool, now: Instant) -> Watch {
        Watch { detector: Detector::new(harness, started_at), detect_ipc, in_flight: None, n: 0, next: now, sent: None }
    }

    pub fn harness(&self) -> Harness {
        self.detector.harness()
    }

    /// 다음 차례 시각. 응답을 기다리는 동안은 None이다.
    pub fn next_at(&self) -> Option<Instant> {
        self.in_flight.is_none().then_some(self.next)
    }

    /// 이 응답 ID가 감지 요청의 것인가
    pub fn owns(&self, id: &str) -> bool {
        self.in_flight.as_deref() == Some(id)
    }

    /// 차례가 왔다: session host에 보낼 요청(ID, 본문). 볼 차례가 아니면 None이고 다음 차례를 잡는다.
    pub fn due(&mut self, now: Instant) -> Option<(String, SessionRequest)> {
        if self.in_flight.is_some() {
            return None;
        }
        match self.detector.tick(now) {
            Tick::Wait => {
                self.next = now + self.detector.interval();
                None
            }
            Tick::Read { unchanged_since } => {
                self.n += 1;
                let id = format!("d{}", self.n);
                let body =
                    if self.detect_ipc { SessionRequest::Detect { unchanged_since } } else { SessionRequest::Screen };
                self.in_flight = Some(id.clone());
                Some((id, body))
            }
        }
    }

    /// 감지 요청의 응답을 평가한다. 알릴 것이 있으면 돌려준다.
    pub fn answer(&mut self, now: Instant, result: Outcome<AgentResponse>) -> Option<Report> {
        self.in_flight = None;
        let found = match result {
            // 화면이 그대로다(herdr `should_skip_idle_screen_scan`): 평가하지 않는다
            Outcome::Ok(AgentResponse::Detect(d)) => d.text.and_then(|text| {
                let input = DetectionInput { screen: &text, osc_title: &d.osc_title, osc_progress: &d.osc_progress };
                self.detector.observe(now, Some(d.seq), input).map(|p| (p, text))
            }),
            Outcome::Ok(AgentResponse::Screen(s)) => {
                let text = text_from_screen(&s.lines, s.cursor_row);
                let input = DetectionInput { screen: &text, osc_title: "", osc_progress: "" };
                self.detector.observe(now, None, input).map(|p| (p, text))
            }
            other => {
                tracing::debug!("감지 화면을 받지 못했다: {other:?}");
                None
            }
        };
        self.next = now + self.detector.interval();
        let (published, text) = found?;
        self.report(published, &text)
    }

    fn report(&mut self, p: Published, text: &str) -> Option<Report> {
        let (kind, question) = match p.state {
            State::Working => ("working", None),
            State::Idle => ("idle", None),
            State::Blocked => ("question", Some(question_of(p.rule.as_deref(), text))),
            // 처음 상태뿐이다(기록 보기 화면은 상태를 바꾸지 않는다). host에는 「모름」이 없다: 알리지 않는다.
            State::Unknown => return None,
        };
        let message = question.as_ref().map(|(_, m)| m.clone());
        let now = (p.state, message.clone());
        if self.sent.as_ref() == Some(&now) {
            return None;
        }
        self.sent = Some(now);
        Some(Report { kind, message, question_kind: question.map(|(k, _)| k), rule: p.rule })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use aaa_proto::{DetectScreen, ErrorBody, ErrorCode, ScreenText};

    use super::super::{POLL, STARTUP_GRACE};
    use super::*;

    // 아래 세 화면은 herdr 0.9.5 `agent explain --file … --agent claude`로 idle(live_prompt_box)·
    // working(live_turn_working)·blocked(bash_permission_prompt)임을 확인했다
    const CLAUDE_IDLE: &str = "\
╭───────────────────────────────╮
│ ✻ Welcome to Claude Code!     │
╰───────────────────────────────╯
────────────────────────────────
❯
────────────────────────────────
  ? for shortcuts
";

    const CLAUDE_WORKING: &str = "\
⏺ 파일을 읽는다.

✻ Thinking… (3s · ↓ 12 tokens · esc to interrupt)

────────────────────────────────
❯
────────────────────────────────
  ? for shortcuts
";

    const CLAUDE_PERMISSION: &str = "\
⏺ Bash(ls)

────────────────────────────────
 Bash command

   ls
   List files

 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and don't ask again for ls commands in /work/demo
   3. No, and tell Claude what to do differently (esc)

 Esc to cancel · Tab to amend · ctrl+e to explain
";

    fn detect(seq: u64, text: Option<&str>) -> Outcome<AgentResponse> {
        Outcome::Ok(AgentResponse::Detect(DetectScreen {
            seq,
            text: text.map(str::to_string),
            osc_title: String::new(),
            osc_progress: String::new(),
        }))
    }

    /// 시작 유예가 끝나 첫 요청을 낸 Watch
    fn started(detect_ipc: bool, t0: Instant) -> (Watch, String, SessionRequest) {
        let mut w = Watch::new(Harness::Claude, t0 - STARTUP_GRACE, detect_ipc, t0);
        assert!(w.due(t0).is_none(), "유예가 끝난 차례는 보지 않는다");
        let at = w.next_at().unwrap();
        assert_eq!(at, t0 + POLL);
        let (id, body) = w.due(at).unwrap();
        (w, id, body)
    }

    #[test]
    fn old_session_hosts_get_screen_requests() {
        let t0 = Instant::now();
        let (_, _, body) = started(true, t0);
        assert_eq!(body, SessionRequest::Detect { unchanged_since: None });
        let (mut w, id, body) = started(false, t0);
        assert_eq!(body, SessionRequest::Screen);
        // 보이는 화면으로도 판정한다
        let screen = ScreenText {
            session: "s".into(),
            cols: 40,
            rows: 10,
            lines: CLAUDE_WORKING.lines().map(str::to_string).collect(),
            cursor_row: 3,
            cursor_col: 2,
            ansi: String::new(),
        };
        assert!(w.owns(&id));
        let r = w.answer(t0, Outcome::Ok(AgentResponse::Screen(screen))).unwrap();
        assert_eq!(r.kind, "working");
    }

    #[test]
    fn one_request_at_a_time_and_unchanged_screens_are_not_evaluated() {
        let t0 = Instant::now();
        let (mut w, id, _) = started(true, t0);
        assert_eq!(w.next_at(), None, "응답을 기다리는 동안은 차례를 세지 않는다");
        assert!(w.due(t0 + Duration::from_secs(5)).is_none());
        assert!(!w.owns("h1"));
        let r = w.answer(t0, detect(7, Some(CLAUDE_IDLE))).unwrap();
        assert_eq!((r.kind, r.message), ("idle", None));
        assert_eq!(w.next_at(), Some(t0 + POLL));
        // idle이고 화면이 그대로면 글 없이 답한다: 평가하지 않아 상태가 그대로다
        let (id2, body) = w.due(t0 + POLL).unwrap();
        assert_ne!(id, id2);
        assert_eq!(body, SessionRequest::Detect { unchanged_since: Some(7) });
        assert!(w.answer(t0 + POLL, detect(7, None)).is_none());
        // 오류 응답도 다음 차례를 잡는다
        w.due(t0 + POLL * 2).unwrap();
        let err = Outcome::Err(ErrorBody::new(ErrorCode::SessionExited, "끝"));
        assert!(w.answer(t0 + POLL * 2, err).is_none());
        assert_eq!(w.next_at(), Some(t0 + POLL * 3));
        let r = w.due(t0 + POLL * 3).map(|_| w.answer(t0 + POLL * 3, detect(8, Some(CLAUDE_WORKING))));
        assert_eq!(r.flatten().map(|r| r.kind), Some("working"));
    }

    #[test]
    fn questions_carry_the_dialog_line_and_repeat_only_when_it_changes() {
        let t0 = Instant::now();
        let (mut w, _, _) = started(true, t0);
        let r = w.answer(t0, detect(1, Some(CLAUDE_PERMISSION))).unwrap();
        assert_eq!(r.kind, "question");
        assert_eq!(r.question_kind, Some("permission"));
        assert_eq!(r.message.as_deref(), Some("Do you want to proceed?"));
        // 같은 확인창의 refresh(800ms)는 다시 알리지 않는다: 답한 뒤 창이 잠깐 남아도 질문을 새로 열지 않는다
        let t1 = t0 + Duration::from_secs(1);
        w.due(t1).unwrap();
        assert!(w.answer(t1, detect(2, Some(CLAUDE_PERMISSION))).is_none());
        // 다른 확인창이 바로 이어 뜨면 알린다
        let next = CLAUDE_PERMISSION.replace("Do you want to proceed?", "Do you want to make this edit?");
        let t2 = t1 + Duration::from_secs(1);
        w.due(t2).unwrap();
        let r = w.answer(t2, detect(3, Some(&next))).unwrap();
        assert!(r.message.unwrap().contains("make this edit?"));
    }
}
