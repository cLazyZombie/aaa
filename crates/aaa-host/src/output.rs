//! 웹 터미널 출력 흐름 제어. herdr-web-ui server/output-window.ts의 `OutputWindow`를 그대로 옮겼다
//! (상수·판정·시험이 같다). 브라우저는 xterm이 다 쓴 뒤에 받은 바이트 끝(offset)을 ACK하고, host는 보낸 것과
//! ACK한 것의 차이(pending)와 보내기 줄(buffered)로 이 연결이 따라오는지 본다. 따라오지 못하면 4008로 닫고
//! 다시 보내지 않는다(「다시 연결」은 사람이 누른다).
//!
//! 참고와 다른 점(plan §8.62): 참고의 `pause`는 함께 쓰는 PTY를 멈춘다. aaa의 PTY는 다른 머신의 session host에 있어
//! 이 연결이 멈출 수 없으므로 `pause`는 「그대로 보내되 시계는 돈다」이다. 그래서 막힌 채 쌓이는 것은
//! `OUTPUT_HARD_BYTES`가 끊는다. 출력은 binary frame으로 가 JSON 이스케이프가 없으므로 wire 크기는 바이트 수 그대로다.

/// pending이 이만큼이면 막힘(참고와 같다)
pub const OUTPUT_HIGH_BYTES: u64 = 256 * 1024;
/// 막힌 뒤 pending이 이만큼으로 내려오면 풀림
pub const OUTPUT_LOW_BYTES: u64 = 64 * 1024;
/// 보내기 줄·pending이 이만큼이면 바로 닫는다
pub const OUTPUT_HARD_BYTES: u64 = 1024 * 1024;
/// 막힌 채 ACK가 나아가지 않으면 이만큼 뒤에 4008로 닫는다
pub const OUTPUT_STALL_MS: u64 = 2000;
/// 붙는 화면(replay)에 시간을 주는 가장 느린 연결: 32KiB/s(약 256kbit/s)
pub const OUTPUT_JOIN_BYTES_PER_SECOND: u64 = 32 * 1024;
/// 붙는 화면이 replay를 ACK하기까지 가장 긴 시간
pub const OUTPUT_JOIN_MAX_MS: u64 = 15_000;
/// 한 화면이 막힌 채 있을 수 있는 가장 긴 시간(ACK를 조금씩 하더라도)
pub const OUTPUT_PAUSE_MAX_MS: u64 = 10_000;
/// 출력을 따라오지 못한 연결을 닫는 WebSocket close code(참고 shared/terminal-flow.ts)
pub const OUTPUT_STALLED_CLOSE_CODE: u16 = 4008;

/// 한 연결을 두고 할 일: 계속 보낸다, 막혔다(참고는 PTY를 멈춘다, aaa는 계속 보낸다), 닫는다(4008)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Flow,
    Pause,
    Stop,
}

#[derive(Debug)]
pub struct OutputWindow {
    /// 이 구독(snapshot 하나)의 번호. 늦게 온 앞 구독의 ACK가 새 창을 풀지 않게 snapshot마다 바꾼다.
    pub id: u32,
    pub sent: u64,
    pub acknowledged: u64,
    pub blocked: bool,
    /// 막힌 뒤 ACK가 나아가지 않은 때부터
    stalled_since: Option<u64>,
    /// 막혀 있기 시작한 때부터(ACK가 와도)
    pausing_since: Option<u64>,
    /// 붙는 화면의 replay가 가는 중이고 아직 ACK가 없다: 그 예산이 여기서 끝난다
    pub join_until: Option<u64>,
    /// 보냈고 아직 ACK가 없는 frame: 끝 offset과 wire 크기, `head`부터 오래된 순
    in_flight: Vec<(u64, u64)>,
    head: usize,
}

impl OutputWindow {
    pub fn new(id: u32) -> Self {
        Self {
            id,
            sent: 0,
            acknowledged: 0,
            blocked: false,
            stalled_since: None,
            pausing_since: None,
            join_until: None,
            in_flight: Vec::new(),
            head: 0,
        }
    }

    pub fn pending(&self) -> u64 {
        self.sent - self.acknowledged
    }

    /// `wire`는 frame이 선로에서 차지하는 크기: 가장 오래된 미확인 frame이 건너가야 ACK가 오므로 마감이 그만큼 늘어난다
    pub fn write(&mut self, bytes: u64, wire: u64) -> u64 {
        self.sent += bytes;
        if wire > 0 {
            self.in_flight.push((self.sent, wire));
        }
        if self.pending() >= OUTPUT_HIGH_BYTES {
            self.blocked = true;
        }
        self.sent
    }

    /// 늦게 붙는 화면에 replay `wire` 바이트를 보내려 한다. 첫 ACK는 frame 전체가 건너가 읽힌 뒤에야 오므로 그때까지
    /// frame 크기만큼의 예산을 준다. `ahead`는 그 앞에 이미 줄 선 것(먼저 건너간다).
    pub fn join(&mut self, wire: u64, ahead: u64, now: u64) {
        self.join_until = Some(now + OUTPUT_JOIN_MAX_MS.min(OUTPUT_STALL_MS + crossing_ms(ahead + wire)));
    }

    /// 중복·순서가 바뀐 ACK는 두 번 풀지 않고 정지 시계도 다시 시작하지 않는다
    pub fn acknowledge(&mut self, id: u32, offset: u64) -> bool {
        if id != self.id || offset > self.sent {
            return false;
        }
        if offset > self.acknowledged {
            self.acknowledged = offset;
            self.stalled_since = None;
            self.join_until = None;
            while self.head < self.in_flight.len() && self.in_flight[self.head].0 <= offset {
                self.head += 1;
            }
            if self.head > 64 && self.head * 2 > self.in_flight.len() {
                self.in_flight.drain(..self.head);
                self.head = 0;
            }
        }
        if self.pending() <= OUTPUT_LOW_BYTES {
            self.blocked = false;
        }
        true
    }

    /// 정지는 frame이 줄 선 때가 아니라 ACK가 나아가는지로 잰다. `buffered`는 이 연결의 보내기 줄이다.
    /// 마감은 OUTPUT_STALL_MS에 가장 오래된 미확인 frame이 느린 연결을 건너는 시간을 더한 것이다.
    pub fn check(&mut self, buffered: u64, now: u64) -> Verdict {
        if buffered >= OUTPUT_HARD_BYTES {
            return Verdict::Stop;
        }
        if !self.blocked && (buffered < OUTPUT_HIGH_BYTES || self.pending() <= OUTPUT_LOW_BYTES) {
            self.stalled_since = None;
            self.pausing_since = None;
            return Verdict::Flow;
        }
        let stalled_since = *self.stalled_since.get_or_insert(now);
        if let Some(until) = self.join_until {
            // 예산 안에서 붙는 화면은 자기 창만 막는다(OUTPUT_HARD_BYTES가 끝)
            if now < until {
                return Verdict::Flow;
            }
            // 예산이 지나면 보통 화면이다
            self.join_until = None;
        }
        let pausing_since = *self.pausing_since.get_or_insert(now);
        let oldest = self.in_flight.get(self.head).map(|&(_, wire)| wire);
        let deadline =
            OUTPUT_STALL_MS + oldest.map_or(0, |wire| (OUTPUT_JOIN_MAX_MS - OUTPUT_STALL_MS).min(crossing_ms(wire)));
        if now - stalled_since >= deadline || now - pausing_since >= OUTPUT_PAUSE_MAX_MS {
            Verdict::Stop
        } else {
            Verdict::Pause
        }
    }
}

/// `wire` 바이트가 예산이 허락하는 가장 느린 연결을 건너는 시간
fn crossing_ms(wire: u64) -> u64 {
    (wire * 1000).div_ceil(OUTPUT_JOIN_BYTES_PER_SECOND)
}

/// herdr-web-ui server/output-window.test.ts의 「terminal output credit」을 옮겼다(ReplayBuffer 시험은 aaa-vt replay가 맡는다).
/// 참고의 `wireBytes`(JSON 이스케이프)는 aaa에서 바이트 수 그대로라, 그 크기를 쓰는 시험은 wire를 직접 준다.
#[cfg(test)]
mod tests {
    use super::*;

    /// 참고 시험의 색 글 replay: ESC가 JSON에서 6바이트가 되는 크기를 그대로 쓴다(시간 예산 모양을 같게)
    fn colored(kib: u64, code: &str) -> (u64, u64) {
        let unit = format!("\x1b[{code}m{}\x1b[0m ", if code == "31" { "red" } else { "grn" });
        let n = kib * 1024 / 13;
        let bytes = unit.len() as u64 * n;
        let wire = serde_json::to_string(&unit.repeat(n as usize)).unwrap().len() as u64;
        (bytes, wire)
    }

    #[test]
    fn holds_high_water_backpressure_until_the_low_watermark_without_duplicate_credit() {
        let mut w = OutputWindow::new(1);
        w.write(OUTPUT_HIGH_BYTES, 0);
        assert!(w.blocked);
        w.acknowledge(w.id, OUTPUT_HIGH_BYTES - OUTPUT_LOW_BYTES - 1);
        assert!(w.blocked);
        w.acknowledge(w.id, OUTPUT_HIGH_BYTES - OUTPUT_LOW_BYTES);
        assert!(!w.blocked);
        let pending = w.pending();
        w.acknowledge(w.id, 0);
        w.acknowledge(w.id, OUTPUT_HIGH_BYTES - OUTPUT_LOW_BYTES);
        assert_eq!(w.pending(), pending);
    }

    #[test]
    fn rejects_stale_subscriptions_and_impossible_offsets() {
        let old = OutputWindow::new(1);
        let mut current = OutputWindow::new(2);
        current.write(100, 0);
        assert!(!current.acknowledge(old.id, 100));
        assert!(!current.acknowledge(current.id, 101));
        assert_eq!(current.pending(), 100);
        assert!(current.acknowledge(current.id, 100));
        assert_eq!(current.pending(), 0);
    }

    #[test]
    fn closes_a_blocked_client_only_after_the_stall_deadline_without_ack_progress() {
        let mut w = OutputWindow::new(1);
        w.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(w.check(0, 0), Verdict::Pause);
        assert_eq!(w.check(0, OUTPUT_STALL_MS - 1), Verdict::Pause);
        assert_eq!(w.check(0, OUTPUT_STALL_MS), Verdict::Stop);
    }

    #[test]
    fn restarts_the_stall_clock_on_every_ack_that_moves_never_on_a_duplicate() {
        let mut w = OutputWindow::new(1);
        w.write(OUTPUT_HIGH_BYTES * 2, 0);
        assert_eq!(w.check(0, 0), Verdict::Pause);
        assert!(w.acknowledge(w.id, 64 * 1024));
        assert!(w.blocked);
        assert_eq!(w.check(0, 1500), Verdict::Pause);
        assert_eq!(w.check(0, 3000), Verdict::Pause);
        w.acknowledge(w.id, 64 * 1024);
        assert_eq!(w.check(0, 3499), Verdict::Pause);
        assert_eq!(w.check(0, 1500 + OUTPUT_STALL_MS), Verdict::Stop);
    }

    #[test]
    fn lets_a_joiner_on_a_slow_link_acknowledge_a_large_replay_after_3s() {
        let mut w = OutputWindow::new(1);
        let replay = 200 * 1024;
        w.join(replay, 0, 0);
        w.write(replay, 0);
        w.write(OUTPUT_HIGH_BYTES - replay, 0);
        assert!(w.blocked);
        for at in [0, OUTPUT_STALL_MS, 3000] {
            assert_eq!(w.check(0, at), Verdict::Flow);
        }
        assert!(w.acknowledge(w.id, replay));
        assert_eq!(w.join_until, None);
        assert!(!w.blocked);
        assert_eq!(w.check(0, 3000), Verdict::Flow);
        w.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(w.check(0, 3100), Verdict::Pause);
        assert_eq!(w.check(0, 3100 + OUTPUT_STALL_MS), Verdict::Stop);
    }

    #[test]
    fn never_holds_for_a_joiner_and_bounds_the_time_it_gets() {
        let mut w = OutputWindow::new(1);
        w.join(OUTPUT_HARD_BYTES, 0, 0);
        w.write(OUTPUT_HIGH_BYTES + 1024, 0);
        assert_eq!(w.check(OUTPUT_HIGH_BYTES, 1000), Verdict::Flow);
        assert_eq!(w.join_until, Some(OUTPUT_JOIN_MAX_MS));
        assert_eq!(w.check(0, OUTPUT_JOIN_MAX_MS - 1), Verdict::Flow);
        assert_eq!(w.check(0, OUTPUT_JOIN_MAX_MS), Verdict::Stop);
        let mut full = OutputWindow::new(2);
        full.join(1024, 0, 0);
        assert_eq!(full.check(OUTPUT_HARD_BYTES, 0), Verdict::Stop);
    }

    #[test]
    fn gives_a_joiner_whose_small_replay_is_acknowledged_nothing_beyond_the_usual_deadline() {
        let mut w = OutputWindow::new(1);
        w.join(1024, 0, 0);
        w.write(1024, 0);
        w.acknowledge(w.id, 1024);
        w.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(w.check(0, 10_000), Verdict::Pause);
        assert_eq!(w.check(0, 10_000 + OUTPUT_STALL_MS), Verdict::Stop);
        let mut silent = OutputWindow::new(2);
        silent.join(1024, 0, 0);
        silent.write(1024 + OUTPUT_HIGH_BYTES, 0);
        assert_eq!(silent.check(0, 0), Verdict::Flow);
        assert_eq!(silent.check(0, OUTPUT_STALL_MS + 32), Verdict::Stop);
    }

    #[test]
    fn stops_a_client_that_stays_blocked_by_acknowledging_a_byte_at_a_time() {
        let mut w = OutputWindow::new(1);
        w.write(OUTPUT_HIGH_BYTES, 0);
        let mut at = 0;
        let mut acknowledged = 0;
        assert_eq!(w.check(0, at), Verdict::Pause);
        while at + OUTPUT_STALL_MS - 100 < OUTPUT_PAUSE_MAX_MS {
            at += OUTPUT_STALL_MS - 100;
            acknowledged += 1;
            w.acknowledge(w.id, acknowledged);
            assert_eq!(w.check(0, at), Verdict::Pause);
        }
        assert_eq!(w.check(0, OUTPUT_PAUSE_MAX_MS), Verdict::Stop);
        let mut honest = OutputWindow::new(2);
        honest.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(honest.check(0, 0), Verdict::Pause);
        honest.acknowledge(honest.id, OUTPUT_HIGH_BYTES - OUTPUT_LOW_BYTES);
        assert_eq!(honest.check(0, OUTPUT_PAUSE_MAX_MS - 1), Verdict::Flow);
        honest.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(honest.check(0, OUTPUT_PAUSE_MAX_MS), Verdict::Pause);
        assert_eq!(honest.check(0, OUTPUT_PAUSE_MAX_MS + 1), Verdict::Pause);
    }

    #[test]
    fn sizes_a_joiners_budget_by_its_frame_on_the_wire() {
        let (bytes, wire) = colored(128, "31");
        assert!(wire as f64 > bytes as f64 * 1.5);
        let mut w = OutputWindow::new(1);
        w.join(wire, 0, 0);
        w.write(bytes, 0);
        w.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(w.check(0, 0), Verdict::Flow);
        let crossing = OUTPUT_STALL_MS + (wire * 1000).div_ceil(OUTPUT_JOIN_BYTES_PER_SECOND) - 1;
        assert!(crossing < OUTPUT_JOIN_MAX_MS);
        assert_eq!(w.check(0, crossing), Verdict::Flow);
        assert!(w.acknowledge(w.id, bytes));
    }

    #[test]
    fn gives_a_joiner_that_was_never_blocked_during_its_budget_the_usual_deadline_later() {
        let mut w = OutputWindow::new(1);
        w.join(1024, 0, 0);
        w.write(1024, 0);
        assert_eq!(w.check(0, 0), Verdict::Flow);
        w.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(w.check(0, 20_000), Verdict::Pause);
        assert_eq!(w.check(0, 20_000 + OUTPUT_STALL_MS - 1), Verdict::Pause);
        assert_eq!(w.check(0, 20_000 + OUTPUT_STALL_MS), Verdict::Stop);
    }

    #[test]
    fn never_stops_an_idle_window_for_a_backlog_it_has_nothing_outstanding_in() {
        let mut idle = OutputWindow::new(1);
        idle.write(10_000, 0);
        idle.acknowledge(idle.id, 10_000);
        assert_eq!(idle.check(300 * 1024, 0), Verdict::Flow);
        assert_eq!(idle.check(300 * 1024, OUTPUT_STALL_MS + 500), Verdict::Flow);
        assert_eq!(idle.check(300 * 1024, OUTPUT_PAUSE_MAX_MS + 500), Verdict::Flow);
        assert_eq!(idle.check(OUTPUT_HARD_BYTES, OUTPUT_PAUSE_MAX_MS + 600), Verdict::Stop);
        let mut busy = OutputWindow::new(2);
        busy.write(OUTPUT_LOW_BYTES + 1, 0);
        assert_eq!(busy.check(300 * 1024, 0), Verdict::Pause);
        assert_eq!(busy.check(300 * 1024, OUTPUT_STALL_MS), Verdict::Stop);
    }

    #[test]
    fn gives_a_joiner_time_for_what_is_queued_ahead_of_its_replay() {
        let (bytes, wire) = colored(128, "31");
        let ahead = 200 * 1024;
        let mut w = OutputWindow::new(1);
        w.join(wire, ahead, 0);
        w.write(bytes, 0);
        w.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(w.check(0, 0), Verdict::Flow);
        let own_frame_only = OUTPUT_STALL_MS + (wire * 1000).div_ceil(OUTPUT_JOIN_BYTES_PER_SECOND);
        let with_ahead =
            OUTPUT_JOIN_MAX_MS.min(OUTPUT_STALL_MS + ((ahead + wire) * 1000).div_ceil(OUTPUT_JOIN_BYTES_PER_SECOND));
        assert_eq!(w.join_until, Some(with_ahead));
        assert_eq!(w.check(0, own_frame_only + 500), Verdict::Flow);
        assert_eq!(w.check(0, with_ahead), Verdict::Stop);
    }

    #[test]
    fn gives_a_large_frame_still_in_flight_after_the_replay_ack_the_time_it_needs_to_cross() {
        let (replay, replay_wire) = colored(128, "31");
        let (live, live_wire) = colored(64, "32");
        let mut w = OutputWindow::new(1);
        w.join(replay_wire, 0, 0);
        let replay_end = w.write(replay, replay_wire);
        let first_end = w.write(live, live_wire);
        let last_end = w.write(live, live_wire);
        w.write(4096, 0);
        assert!(w.blocked);
        assert_eq!(w.check(0, 0), Verdict::Flow);
        assert!(w.acknowledge(w.id, replay_end));
        assert!(w.blocked);
        assert_eq!(w.check(0, 4001), Verdict::Pause);
        assert!(live_wire * 1000 / OUTPUT_JOIN_BYTES_PER_SECOND > OUTPUT_STALL_MS);
        assert_eq!(w.check(0, 4001 + OUTPUT_STALL_MS + 100), Verdict::Pause);
        assert!(w.acknowledge(w.id, first_end));
        assert_eq!(w.check(0, 7000), Verdict::Pause);
        assert!(w.acknowledge(w.id, last_end));
        assert_eq!(w.check(0, 9000), Verdict::Flow);
        let mut silent = OutputWindow::new(2);
        silent.write(live, live_wire);
        silent.write(OUTPUT_HIGH_BYTES, 0);
        assert_eq!(silent.check(0, 0), Verdict::Pause);
        let deadline = OUTPUT_STALL_MS + (live_wire * 1000).div_ceil(OUTPUT_JOIN_BYTES_PER_SECOND);
        assert_eq!(silent.check(0, deadline - 1), Verdict::Pause);
        assert_eq!(silent.check(0, deadline), Verdict::Stop);
    }
}
