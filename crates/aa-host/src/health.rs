//! 머신 건강(§8.10). agent 보고에 시계 차이를 더해 판정하고, 이상이면 그 머신의 새 세션을 거부한다.
//!
//! - 이상은 연결 중인 머신만 본다(끊긴 머신은 `outage`가 맡는다). 첫 보고 전에는 막지 않는다.
//! - 이상이 생기거나 바뀌거나 풀리면, 그 머신에 열린 thread가 있는 프로젝트 inbox에 남긴다(알림도 같은 항목을 본다).

use std::sync::Arc;

use aa_proto::{ErrorBody, ErrorCode, MachineHealth};

use crate::{Host, MachineRt, err};

/// host와 이만큼 넘게 어긋난 시계는 이상으로 본다
const MAX_SKEW_MS: i64 = 60_000;

/// 새 세션을 거부할 이유(연결 중일 때만)
pub(crate) fn problems(m: &MachineRt) -> Vec<String> {
    let Some((h, skew)) = m.health.as_ref().filter(|_| m.conn.is_some()) else { return vec![] };
    let mut out = h.problems.clone();
    if skew.abs() > MAX_SKEW_MS {
        out.push(format!("시계가 host와 {}초 어긋난다", skew / 1000));
    }
    out
}

impl Host {
    pub fn on_health(self: &Arc<Self>, mid: &str, h: MachineHealth) {
        let skew = h.at_ms as i64 - aa_common::util::now_ms() as i64;
        {
            let mut i = self.inner.lock().unwrap();
            let Some(m) = i.machines.get_mut(mid) else { return };
            m.health = Some((h, skew));
        }
        self.bump();
        self.tick_wake.notify_one();
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
        Err(err(
            ErrorCode::Unavailable,
            format!("머신 {name} 점검 이상으로 새 세션을 시작하지 않는다: {}", p.join("; ")),
        ))
    }

    /// ticker: 이상이 생기거나 바뀌거나 풀리면 inbox에 남긴다
    pub(crate) fn check_health_items(&self, projects: &[String]) {
        for m in self.machine_views() {
            if m.connection != aa_proto::Connection::Connected || m.health.is_none() {
                continue;
            }
            let now = m.problems.join("; ");
            let before = self.health_marks.lock().unwrap().insert(m.id.clone(), now.clone()).unwrap_or_default();
            if now == before {
                continue;
            }
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
