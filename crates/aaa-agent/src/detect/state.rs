//! 언제 상태를 알리나. herdr 0.9.5 `src/pane/agent_detection.rs`와 `src/pane.rs` 감지 루프(947~1240줄)를 옮겼다.
//!
//! - 300ms마다 화면을 본다. 작업 중 → 평범한 idle(입력 상자가 보이지 않는 idle)은 100ms 간격으로 세 번 더 확인하거나
//!   700ms가 지나야 알린다(작업 사이 잠깐 비는 화면을 idle로 알리지 않는다).
//! - harness가 막 시작했으면 3초 동안 보지 않는다(시작 화면).
//! - idle(Codex는 모름 포함)이고 화면이 그대로면 읽지 않는다.
//! - 상태나 「보이는 표지」가 바뀔 때만 알린다. 확인창이 그대로 떠 있으면 800ms마다 다시 알린다(refresh).
//!
//! herdr와 다른 것: 프로세스 판별(agent 바뀜·끝남)은 없다. aaa 세션은 harness 하나를 띄우고 끝나면 세션도 끝난다
//! (끝남은 agent가 세션 종료로 따로 알린다). 시각은 인자로 받는다(시험이 시계를 넘긴다).

use std::time::{Duration, Instant};

use super::{Detection, DetectionInput, Harness, State, detect};

/// 정리 표지로 작업 중이 된 감지의 규칙 이름(log용, 참고 규칙에 없다)
pub const COMPACTING_RULE: &str = "aaa_compacting";
/// 평소 간격(herdr 감지 루프 `from_millis(300)`)
pub const POLL: Duration = Duration::from_millis(300);
/// 작업 중 → idle 확인 중 간격(`AGENT_PENDING_IDLE_RECHECK`)
pub const POLL_PENDING_IDLE: Duration = Duration::from_millis(100);
const PENDING_IDLE_CONFIRMATIONS: u8 = 3;
const PENDING_IDLE_CAP: Duration = Duration::from_millis(700);
const STABLE_VISIBLE_SIGNAL_REFRESH: Duration = Duration::from_millis(800);
/// harness가 시작한 뒤 화면을 보지 않는 시간(`AGENT_STARTUP_GRACE_WINDOW`)
pub const STARTUP_GRACE: Duration = Duration::from_secs(3);

/// 시작 유예. `AAA_TEST_DETECT_GRACE_MS`는 시험에서만 줄인다(가짜 harness 화면은 뜨자마자 그대로다: 웹 E2E 11은 상태마다
/// 3초를 기다렸다. 15는 기본값으로 돈다)
fn startup_grace() -> Duration {
    std::env::var("AAA_TEST_DETECT_GRACE_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map_or(STARTUP_GRACE, Duration::from_millis)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PublishState {
    state: State,
    visible_idle: bool,
    visible_blocker: bool,
    visible_working: bool,
    /// 대화 정리 중(aaa가 더한 표지, `super::compacting`): 작업 중 안에서 바뀌어도 알린다
    compacting: bool,
}

#[derive(Debug, Default)]
struct PendingIdle {
    started_at: Option<Instant>,
    confirmations: u8,
}

impl PendingIdle {
    fn active(&self) -> bool {
        self.started_at.is_some()
    }

    fn clear(&mut self) {
        self.started_at = None;
        self.confirmations = 0;
    }

    /// herdr `should_hold_working_to_idle`
    fn hold(&mut self, previous: PublishState, next: PublishState, now: Instant) -> bool {
        let plain_idle = previous.state == State::Working
            && next.state == State::Idle
            && !next.visible_idle
            && !next.visible_blocker;
        if !plain_idle {
            self.clear();
            return false;
        }
        let Some(started_at) = self.started_at else {
            self.started_at = Some(now);
            self.confirmations = 0;
            return true;
        };
        if now.duration_since(started_at) >= PENDING_IDLE_CAP {
            self.clear();
            return false;
        }
        self.confirmations = self.confirmations.saturating_add(1);
        if self.confirmations >= PENDING_IDLE_CONFIRMATIONS {
            self.clear();
            return false;
        }
        true
    }
}

/// 이번에 할 일
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    /// 보지 않는다(시작 유예)
    Wait,
    /// 화면을 읽는다. `unchanged_since`가 있으면 그 판(화면 변화 순번)과 같을 때 읽지 않아도 된다.
    Read { unchanged_since: Option<u64> },
}

/// 알릴 상태
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub state: State,
    pub visible_idle: bool,
    pub visible_blocker: bool,
    pub visible_working: bool,
    /// 대화 정리 중(작업 중일 때만)
    pub compacting: bool,
    pub rule: Option<String>,
    /// 상태·표지가 바뀌었다(false면 그대로 떠 있는 확인창의 refresh)
    pub changed: bool,
}

/// pane 하나의 감지 상태
#[derive(Debug)]
pub struct Detector {
    harness: Harness,
    published: PublishState,
    last_visible_signal_refresh: Option<Instant>,
    pending_idle: PendingIdle,
    grace_until: Option<Instant>,
    last_scan_seq: Option<u64>,
}

impl Detector {
    /// `started_at`: harness를 띄운 때. 시작 유예(3초) 안이면 남은 시간 동안 보지 않는다.
    pub fn new(harness: Harness, started_at: Instant) -> Detector {
        Detector {
            harness,
            published: PublishState {
                state: State::Unknown,
                visible_idle: false,
                visible_blocker: false,
                visible_working: false,
                compacting: false,
            },
            last_visible_signal_refresh: None,
            pending_idle: PendingIdle::default(),
            grace_until: Some(started_at + startup_grace()),
            last_scan_seq: None,
        }
    }

    pub fn harness(&self) -> Harness {
        self.harness
    }

    /// 지금 알린 상태
    pub fn state(&self) -> State {
        self.published.state
    }

    /// 다음 확인까지 기다릴 시간
    pub fn interval(&self) -> Duration {
        if self.pending_idle.active() { POLL_PENDING_IDLE } else { POLL }
    }

    /// 이번 차례에 화면을 읽을까(herdr 루프의 시작 유예와 `decide_detection_screen_read`)
    pub fn tick(&mut self, now: Instant) -> Tick {
        if let Some(until) = self.grace_until {
            self.pending_idle.clear();
            if now < until {
                return Tick::Wait;
            }
            // 유예가 끝난 차례도 보지 않고, 다음 차례부터 화면이 같아도 한 번은 읽는다
            self.grace_until = None;
            self.last_scan_seq = None;
            return Tick::Wait;
        }
        let stable = self.published.state == State::Idle
            || (self.published.state == State::Unknown && self.harness == Harness::Codex);
        let skippable = stable && !self.pending_idle.active();
        Tick::Read { unchanged_since: if skippable { self.last_scan_seq } else { None } }
    }

    /// 「보이는 막힘」을 다시 알리는 차례(`STABLE_VISIBLE_SIGNAL_REFRESH`)를 `now`부터 다시 센다
    pub fn hold_refresh(&mut self, now: Instant) {
        self.last_visible_signal_refresh = Some(now);
    }

    /// 읽은 화면을 평가한다. 알릴 것이 있으면 돌려준다(herdr `decide_screen_detection_publish`).
    /// `seq`는 그 화면의 변화 순번이다(모르면 None: 그러면 다음에도 늘 읽는다).
    pub fn observe(&mut self, now: Instant, seq: Option<u64>, input: DetectionInput<'_>) -> Option<Published> {
        self.last_scan_seq = seq;
        let mut detection = detect(self.harness, input);
        // 정리 표지가 보이면 작업 중이다(plan §8.305). 참고 규칙은 정리 화면을 작업 중과 쉼 사이에서 오간다: Claude spinner가
        // 「(5s · ↓ 168 tokens)」면 `live_turn_working`, 요약 글이 아직 오지 않은 「(0s)」면 입력 상자(`live_prompt_box`)로 쉼이고,
        // 그동안 창 제목도 작업 중 spinner가 아니다(10-20 실측 「✳ …」). 확인창·기록 보기는 그대로 둔다
        let compacting = detection.state != State::Blocked
            && !detection.skip_state_update
            && super::compacting(self.harness, input.screen);
        if compacting {
            detection = Detection {
                state: State::Working,
                skip_state_update: false,
                visible_idle: false,
                visible_blocker: false,
                visible_working: true,
                rule: Some(COMPACTING_RULE.to_string()),
            };
        }
        self.publish_marked(now, detection, compacting)
    }

    #[cfg(test)]
    fn publish(&mut self, now: Instant, detection: Detection) -> Option<Published> {
        self.publish_marked(now, detection, false)
    }

    fn publish_marked(&mut self, now: Instant, detection: Detection, compacting: bool) -> Option<Published> {
        if detection.skip_state_update {
            self.pending_idle.clear();
            return None;
        }
        let state = detection.state;
        let next = PublishState {
            state,
            visible_idle: detection.visible_idle && state == State::Idle,
            visible_blocker: detection.visible_blocker && state == State::Blocked,
            visible_working: detection.visible_working && state == State::Working,
            compacting: compacting && state == State::Working,
        };
        let previous = self.published;
        let stable_visible_signal = next.visible_blocker && previous.visible_blocker;
        let refresh_due = stable_visible_signal
            && self
                .last_visible_signal_refresh
                .is_none_or(|last| now.duration_since(last) >= STABLE_VISIBLE_SIGNAL_REFRESH);
        if self.pending_idle.hold(previous, next, now) {
            return None;
        }
        let changed = next != previous;
        if !changed && !(refresh_due && next.visible_blocker && previous.visible_blocker) {
            return None;
        }
        self.published = next;
        self.last_visible_signal_refresh = (next.visible_blocker || next.visible_working).then_some(now);
        Some(Published {
            state: next.state,
            visible_idle: next.visible_idle,
            visible_blocker: next.visible_blocker,
            visible_working: next.visible_working,
            compacting: next.compacting,
            rule: detection.rule,
            changed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(state: State, visible: bool) -> Detection {
        Detection {
            state,
            skip_state_update: false,
            visible_idle: visible && state == State::Idle,
            visible_blocker: visible && state == State::Blocked,
            visible_working: visible && state == State::Working,
            rule: None,
        }
    }

    /// 시작 유예가 끝나 읽기 시작한 detector
    fn ready(harness: Harness, t0: Instant) -> Detector {
        let mut d = Detector::new(harness, t0);
        assert_eq!(d.tick(t0), Tick::Wait);
        assert_eq!(d.tick(t0 + STARTUP_GRACE), Tick::Wait, "유예가 끝난 차례도 보지 않는다");
        d
    }

    // herdr agent_detection.rs `pending_idle_holds_working_to_plain_idle_until_confirmed`
    #[test]
    fn working_to_plain_idle_waits_for_three_confirmations() {
        let t0 = Instant::now();
        let mut d = ready(Harness::Claude, t0);
        assert_eq!(d.publish(t0, det(State::Working, true)).map(|p| p.state), Some(State::Working));
        assert_eq!(d.interval(), POLL);
        for i in 0..3 {
            assert!(d.publish(t0 + POLL_PENDING_IDLE * i, det(State::Idle, false)).is_none(), "{i}번째는 붙잡는다");
            assert_eq!(d.interval(), POLL_PENDING_IDLE);
        }
        let p = d.publish(t0 + POLL_PENDING_IDLE * 3, det(State::Idle, false)).unwrap();
        assert_eq!((p.state, p.changed), (State::Idle, true));
        assert_eq!(d.interval(), POLL);
    }

    #[test]
    fn pending_idle_gives_up_holding_at_the_cap() {
        let t0 = Instant::now();
        let mut d = ready(Harness::Codex, t0);
        d.publish(t0, det(State::Working, true));
        assert!(d.publish(t0, det(State::Idle, false)).is_none());
        // 확인이 한 번뿐이어도 700ms가 지나면 알린다
        assert_eq!(d.publish(t0 + PENDING_IDLE_CAP, det(State::Idle, false)).map(|p| p.state), Some(State::Idle));
    }

    // herdr `visible_idle_bypasses_plain_idle_hold`, 작업이 다시 보이면 붙잡기를 푼다
    #[test]
    fn visible_idle_and_resumed_work_skip_the_hold() {
        let t0 = Instant::now();
        let mut d = ready(Harness::Claude, t0);
        d.publish(t0, det(State::Working, true));
        assert_eq!(d.publish(t0, det(State::Idle, true)).map(|p| p.state), Some(State::Idle));
        d.publish(t0, det(State::Working, true));
        assert!(d.publish(t0, det(State::Idle, false)).is_none());
        assert!(d.publish(t0, det(State::Working, true)).is_none(), "그대로 작업 중이면 알리지 않는다");
        assert_eq!(d.interval(), POLL, "붙잡기가 풀렸다");
        // 확인창은 붙잡지 않는다
        assert_eq!(d.publish(t0, det(State::Blocked, true)).map(|p| p.state), Some(State::Blocked));
    }

    #[test]
    fn blocked_refreshes_every_800ms_and_nothing_else_repeats() {
        let t0 = Instant::now();
        let mut d = ready(Harness::Claude, t0);
        assert!(d.publish(t0, det(State::Blocked, true)).unwrap().changed);
        assert!(d.publish(t0 + Duration::from_millis(799), det(State::Blocked, true)).is_none());
        let r = d.publish(t0 + STABLE_VISIBLE_SIGNAL_REFRESH, det(State::Blocked, true)).unwrap();
        assert!(!r.changed, "같은 확인창의 refresh");
        assert!(
            d.publish(t0 + STABLE_VISIBLE_SIGNAL_REFRESH * 2 - Duration::from_millis(1), det(State::Blocked, true))
                .is_none()
        );
        // 「보이지 않는」 blocker(약한 표지)는 refresh하지 않는다
        let mut w = ready(Harness::Codex, t0);
        assert!(w.publish(t0, det(State::Blocked, false)).is_some());
        assert!(w.publish(t0 + Duration::from_secs(5), det(State::Blocked, false)).is_none());
        // idle·작업 중은 바뀔 때만
        d.publish(t0, det(State::Idle, true));
        assert!(d.publish(t0 + Duration::from_secs(5), det(State::Idle, true)).is_none());
    }

    // herdr `screen_read_skips_unchanged_idle_bottom_buffer`·`..._ambiguous_codex_...`·`..._for_transitions...`
    #[test]
    fn idle_screen_is_not_reread_until_it_changes() {
        let t0 = Instant::now();
        let mut d = ready(Harness::Claude, t0);
        assert_eq!(d.tick(t0), Tick::Read { unchanged_since: None }, "모름(Claude)은 늘 읽는다");
        let input = DetectionInput { screen: "x\n", osc_title: "✳ x", osc_progress: "" };
        assert_eq!(d.observe(t0, Some(10), input).map(|p| p.state), Some(State::Idle));
        assert_eq!(d.tick(t0), Tick::Read { unchanged_since: Some(10) });
        d.publish(t0, det(State::Working, true));
        assert_eq!(d.tick(t0), Tick::Read { unchanged_since: None }, "작업 중은 늘 읽는다");
        d.publish(t0, det(State::Idle, false));
        assert_eq!(d.tick(t0), Tick::Read { unchanged_since: None }, "idle 확인 중은 늘 읽는다");
        // 화면 순번을 모르면(옛 session host) 늘 읽는다
        let mut c = ready(Harness::Codex, t0);
        assert_eq!(c.tick(t0), Tick::Read { unchanged_since: None });
        c.observe(t0, None, DetectionInput { screen: "", osc_title: "", osc_progress: "" });
        assert_eq!(c.tick(t0), Tick::Read { unchanged_since: None });
    }

    #[test]
    fn startup_grace_and_skipped_viewer_screens() {
        let t0 = Instant::now();
        let mut d = Detector::new(Harness::Codex, t0);
        assert_eq!(d.tick(t0 + STARTUP_GRACE - Duration::from_millis(1)), Tick::Wait);
        assert_eq!(d.tick(t0 + STARTUP_GRACE), Tick::Wait);
        // Codex 기록 보기(transcript viewer)는 상태를 바꾸지 않는다. 모름인 Codex는 화면이 같으면 읽지 않아도 된다
        // (herdr `should_skip_idle_screen_scan`)
        let viewer = "• 지난 답\n↑/↓ to scroll · pgup/pgdn to page · home/end to jump · q to quit · esc to edit prev\n";
        assert!(d.observe(t0, Some(3), DetectionInput { screen: viewer, osc_title: "", osc_progress: "" }).is_none());
        assert_eq!(d.state(), State::Unknown);
        assert_eq!(d.tick(t0 + STARTUP_GRACE), Tick::Read { unchanged_since: Some(3) });
        d.publish(t0, det(State::Working, true));
        let skip = Detection { skip_state_update: true, ..det(State::Unknown, false) };
        assert!(d.publish(t0, skip).is_none());
        assert_eq!(d.state(), State::Working);
        // 오래전에 띄운 세션에 다시 붙으면 유예가 바로 끝난다
        let mut old = Detector::new(Harness::Claude, t0);
        assert_eq!(old.tick(t0 + Duration::from_secs(600)), Tick::Wait);
        assert_eq!(old.tick(t0 + Duration::from_secs(600)), Tick::Read { unchanged_since: None });
    }
}
