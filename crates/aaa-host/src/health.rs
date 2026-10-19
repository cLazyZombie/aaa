//! 머신 건강(§8.10). agent 보고에 시계 차이를 더해 판정하고, 이상이면 그 머신의 새 세션을 거부한다.
//!
//! - 이상은 연결 중인 머신만 본다(끊긴 머신은 `outage`가 맡는다). 첫 보고 전에는 막지 않는다.
//! - 이상이 생기거나 바뀌거나 풀리면, 그 머신에 열린 thread가 있는 프로젝트 inbox에 남긴다(알림도 같은 항목을 본다).
//! - agent는 15초마다 CPU·메모리를 다시 보낸다(§8.245). 값만 바뀐 보고는 화면에 보일 만큼 움직였을 때만 front에 다시 보낸다.

use std::sync::Arc;

use aaa_proto::{ErrorBody, ErrorCode, HealthProblem, MachineHealth};

use crate::{Host, MachineRt, err};

/// host와 이만큼 넘게 어긋난 시계는 이상으로 본다
const MAX_SKEW_MS: i64 = 60_000;

/// 화면에 다시 보낼 CPU·MEM 움직임(반올림 %p, 이보다 커야 한다)과, 덜 움직여도 다시 보내는 간격
const STATS_STEP: u8 = 2;
const STATS_RESEND_MS: u64 = 60_000;

/// 마지막으로 화면에 보낸 CPU·MEM(반올림 %)과 그때 host 시각
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HealthSent {
    cpu: Option<u8>,
    mem: Option<u8>,
    at_ms: u64,
}

/// 사용 메모리 몫(반올림 %)
fn mem_pct(h: &MachineHealth) -> Option<u8> {
    match (h.mem_used_bytes, h.mem_total_bytes) {
        (Some(used), Some(total)) if total > 0 => Some((used.min(total) as f64 * 100.0 / total as f64).round() as u8),
        _ => None,
    }
}

/// 값 칸(잰 때·CPU·메모리·부하·디스크 여유)을 빼면 같은 보고인가. 다르면 이상·주의·toolchain·전원 같은 것이 바뀌었다
fn same_report(a: &MachineHealth, b: &MachineHealth) -> bool {
    let calm = |h: &MachineHealth| MachineHealth {
        at_ms: 0,
        cpu_pct: None,
        mem_used_bytes: None,
        load_x100: None,
        disk_free_bytes: None,
        ..h.clone()
    };
    calm(a) == calm(b)
}

/// 새 보고를 받았을 때 (이상 판정을 깨울까, front에 다시 보낼까).
/// 첫 보고·값 칸 말고 바뀐 것·시계 이상이 생기거나 풀림은 둘 다. 값만 바뀌었으면 CPU·MEM이 2%p 넘게 움직였거나
/// 마지막으로 보낸 지 60초가 지났을 때만 보낸다(그사이 다른 까닭으로 보낸 목록에도 지금 값이 실린다).
pub(crate) fn worth_sending(
    old: Option<&(MachineHealth, i64)>,
    sent: Option<HealthSent>,
    new: &MachineHealth,
    skew: i64,
    now_ms: u64,
) -> (bool, bool) {
    let changed =
        old.is_none_or(|(o, s)| !same_report(o, new) || (s.abs() > MAX_SKEW_MS) != (skew.abs() > MAX_SKEW_MS));
    let far = |a: Option<u8>, b: Option<u8>| match (a, b) {
        (Some(a), Some(b)) => a.abs_diff(b) > STATS_STEP,
        (a, b) => a.is_some() != b.is_some(),
    };
    let moved = sent.is_none_or(|s| {
        far(s.cpu, new.cpu_pct) || far(s.mem, mem_pct(new)) || now_ms.saturating_sub(s.at_ms) >= STATS_RESEND_MS
    });
    (changed, changed || moved)
}

/// 새 세션을 거부할 이유(연결 중일 때만)
pub(crate) fn problems(m: &MachineRt) -> Vec<HealthProblem> {
    let Some((h, skew)) = m.health.as_ref().filter(|_| m.conn.is_some()) else { return vec![] };
    let mut out = h.problems.clone();
    if skew.abs() > MAX_SKEW_MS {
        out.push(HealthProblem {
            code: "clock".into(), text: format!("시계가 host와 {}초 어긋난다", skew / 1000)
        });
    }
    out
}

pub(crate) fn texts(p: &[HealthProblem]) -> String {
    p.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("; ")
}

impl Host {
    pub fn on_health(self: &Arc<Self>, mid: &str, h: MachineHealth) {
        let now = aaa_common::util::now_ms();
        let skew = h.at_ms as i64 - now as i64;
        let (changed, send) = {
            let mut i = self.inner.lock().unwrap();
            let Some(m) = i.machines.get_mut(mid) else { return };
            let (changed, send) = worth_sending(m.health.as_ref(), m.health_sent, &h, skew, now);
            if send {
                m.health_sent = Some(HealthSent { cpu: h.cpu_pct, mem: mem_pct(&h), at_ms: now });
            }
            m.health = Some((h, skew));
            (changed, send)
        };
        if send {
            self.bump();
        }
        if changed {
            self.tick_wake.notify_one();
        }
    }

    /// 점검에 실패한 머신에는 새 세션을 띄우지 않는다
    pub(crate) fn check_health(&self, mid: &str) -> Result<(), ErrorBody> {
        let (name, p) = {
            let i = self.inner.lock().unwrap();
            match i.machines.get(mid) {
                Some(m) => (m.name.clone(), problems(m)),
                None => return Ok(()),
            }
        };
        if p.is_empty() {
            return Ok(());
        }
        Err(err(ErrorCode::Unavailable, format!("머신 {name} 점검 이상으로 새 세션을 시작하지 않는다: {}", texts(&p))))
    }

    /// ticker: 이상의 종류(code)가 생기거나 바뀌거나 풀리면 inbox에 남긴다. 값(디스크 여유 등)만 바뀐 것은 새 항목이 아니다.
    pub(crate) fn check_health_items(&self, projects: &[String]) {
        for m in self.machine_views() {
            if m.connection != aaa_proto::Connection::Connected || m.health.is_none() {
                continue;
            }
            let mut codes: Vec<&str> = m.problems.iter().map(|p| p.code.as_str()).collect();
            codes.sort();
            let key = codes.join(",");
            let before = self.health_marks.lock().unwrap().insert(m.id.clone(), key.clone()).unwrap_or_default();
            if key == before {
                continue;
            }
            let now = texts(&m.problems);
            for p in projects {
                if !self.open_threads(p).iter().any(|r| r.machine_id == m.id) {
                    continue;
                }
                if now.is_empty() {
                    self.inbox_add(p, "health", &m.name, "ok", &format!("머신 {} 점검이 정상으로 돌아왔다", m.name));
                } else {
                    self.inbox_add(
                        p,
                        "health",
                        &m.name,
                        "problem",
                        &format!("머신 {} 점검 이상(새 세션 거부): {now}", m.name),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(cpu: Option<u8>, mem_pct: Option<u64>) -> MachineHealth {
        MachineHealth {
            at_ms: 1,
            disk_free_bytes: Some(100),
            disk_total_bytes: Some(1000),
            load_x100: Some(250),
            cpus: 8,
            cpu_pct: cpu,
            mem_used_bytes: mem_pct.map(|p| p * 10),
            mem_total_bytes: mem_pct.map(|_| 1000),
            on_ac: Some(true),
            toolchain: vec![],
            problems: vec![],
            warnings: vec![],
        }
    }

    const T: u64 = 1_000_000;

    #[test]
    fn values_only_reports_are_sent_when_they_move_or_after_a_minute() {
        let old = (report(Some(40), Some(60)), 0);
        let sent = Some(HealthSent { cpu: Some(40), mem: Some(60), at_ms: T });
        let at = |r: &MachineHealth, now| worth_sending(Some(&old), sent, r, 0, now);
        assert_eq!(at(&report(Some(42), Some(62)), T + 15_000), (false, false), "2%p는 보내지 않는다");
        // 잰 때·부하·디스크 여유만 바뀐 것도 값이다
        let mut quiet = report(Some(41), Some(59));
        (quiet.at_ms, quiet.load_x100, quiet.disk_free_bytes) = (99, Some(900), Some(90));
        assert_eq!(at(&quiet, T + 15_000), (false, false));
        assert_eq!(at(&report(Some(43), Some(60)), T + 15_000), (false, true), "CPU 3%p");
        assert_eq!(at(&report(Some(40), Some(57)), T + 15_000), (false, true), "MEM 3%p");
        assert_eq!(at(&report(None, Some(60)), T + 15_000), (false, true), "값이 없어졌다");
        assert_eq!(at(&report(Some(40), Some(60)), T + 59_999), (false, false));
        assert_eq!(at(&report(Some(40), Some(60)), T + 60_000), (false, true), "60초 지나면 보낸다");
    }

    #[test]
    fn first_report_problems_and_clock_wake_the_ticker() {
        let r = report(Some(40), Some(60));
        assert_eq!(worth_sending(None, None, &r, 0, T), (true, true), "첫 보고");
        let old = (r.clone(), 0);
        let sent = Some(HealthSent { cpu: Some(40), mem: Some(60), at_ms: T });
        let mut bad = r.clone();
        bad.problems.push(HealthProblem { code: "disk".into(), text: "디스크 여유 1GB < 5GB".into() });
        assert_eq!(worth_sending(Some(&old), sent, &bad, 0, T + 1), (true, true), "이상");
        let mut warn = r.clone();
        warn.warnings.push("배터리 전원".into());
        assert_eq!(worth_sending(Some(&old), sent, &warn, 0, T + 1), (true, true), "주의");
        assert_eq!(worth_sending(Some(&old), sent, &r, MAX_SKEW_MS + 1, T + 1), (true, true), "시계 이상");
        assert_eq!(worth_sending(Some(&old), sent, &r, 500, T + 1), (false, false), "시계가 조금 흔들린 것은 아니다");
    }

    #[test]
    fn memory_share_rounds_and_needs_both_sizes() {
        assert_eq!(mem_pct(&report(None, Some(75))), Some(75));
        let mut h = report(None, None);
        assert_eq!(mem_pct(&h), None);
        (h.mem_used_bytes, h.mem_total_bytes) = (Some(2), Some(3));
        assert_eq!(mem_pct(&h), Some(67));
        h.mem_total_bytes = Some(0);
        assert_eq!(mem_pct(&h), None);
    }
}
