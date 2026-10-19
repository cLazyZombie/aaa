//! thread의 사용량 한도 표지(plan §8.267, t-0403). t3code(MIT, 1451e7cb2)는 provider SDK가 주는 구조화된 신호
//! (Claude 응답 `error == "rate_limit"`·429, Codex `usageLimitExceeded`)로 run을 `usage_limit` 실패로 닫고 목록 줄을
//! 「Limited」로 칠한다(`apps/web/src/components/Sidebar.logic.ts:939`). aaa는 harness를 PTY로 돌려 그 신호가 없다.
//! 대신 host가 이미 읽어 둔 구독 사용량(usage.rs `cached`, 임무의 「사용량 대기」와 같은 재료, mission.rs `usage_until`)에서
//! 이 thread harness의 창이 다 찼고 아직 다시 차오르지 않았으면 그때까지 한도로 본다.
//!
//! - host 머신의 thread만이다: 사용량은 host 머신 CLI 로그인만 읽는다. 한 구독에 계정이 둘 이상이면 어느 것인지 단정하지 않는다.
//! - ticker가 사용량을 새로 읽지 않는다(§8.45: client가 물을 때만). 웹이 사용량을 보이는 동안 숫자가 새로워진다.
//!   오래된 숫자로 한도를 새로 알아채지 못할 수는 있어도, 다시 차오르는 때가 지나면 표지가 걷혀 잘못 남지 않는다.
use std::collections::HashMap;
use std::sync::Mutex;

use aaa_proto::{UsageProviderId as P, UsageReport};

/// (프로젝트, thread id) → 한도가 풀리는 때(ms)
pub(crate) type LimitMap = Mutex<HashMap<(String, String), u64>>;

/// 다 찼다고 보는 사용률(%). 구독 API는 다 쓴 창을 100으로 말한다
const FULL_PERCENT: f64 = 100.0;

/// 이 harness·model의 구독 창 가운데 다 찼고 `now` 뒤에 다시 차오르는 것의 가장 늦은 때. 계정이 하나가 아니거나
/// 다 찬 창이 없으면 None. plan이 따로 거는 모델 몫(`scope`)은 그 모델일 때만 본다(mission.rs `usage_until`과 같다)
pub(crate) fn limited_until(harness: &str, model: &str, report: &UsageReport, now: u64) -> Option<u64> {
    let provider = match harness {
        "claude" => P::Claude,
        "codex" => P::Codex,
        _ => return None,
    };
    let model = model.to_lowercase();
    let mut found = report.providers.iter().filter(|p| p.id == provider);
    let first = found.next()?;
    if found.next().is_some() {
        return None;
    }
    first
        .windows
        .iter()
        .filter(|w| w.used_percent >= FULL_PERCENT)
        .filter(|w| w.scope.as_ref().is_none_or(|s| model.contains(&s.to_lowercase())))
        .filter_map(|w| aaa_common::util::parse_iso(w.resets_at.as_deref()?))
        .filter(|until| *until > now)
        .max()
}

/// 다 찬 창이 하나라도 있나: 없으면 thread마다 profile을 풀지 않는다
fn any_full(report: &UsageReport, now: u64) -> bool {
    report.providers.iter().flat_map(|p| &p.windows).any(|w| {
        w.used_percent >= FULL_PERCENT
            && w.resets_at.as_deref().and_then(aaa_common::util::parse_iso).is_some_and(|until| until > now)
    })
}

impl crate::Host {
    pub(crate) fn usage_limit(&self, project: &str, id: &str) -> Option<u64> {
        self.usage_limits.lock().unwrap().get(&(project.to_string(), id.to_string())).copied()
    }

    /// ticker 한 번: 열린 thread마다 한도를 다시 셈하고, 바뀐 thread만 다시 알린다
    pub(crate) async fn usage_limit_pass(&self, project: &str) {
        let threads = self.open_threads(project);
        let report = self.usage.cached().await;
        let now = aaa_common::util::now_ms();
        let full = any_full(&report, now);
        let mut seen = Vec::new();
        for r in &threads {
            let next = if full && self.machine_is_local(&r.machine_id) {
                self.resolved_profile(Some(project), &r.machine_id, &r.profile)
                    .ok()
                    .and_then(|p| limited_until(&p.harness, &p.model, &report, now))
            } else {
                None
            };
            let key = (project.to_string(), r.id.clone());
            seen.push(key.clone());
            let prev = {
                let mut map = self.usage_limits.lock().unwrap();
                match next {
                    Some(until) => map.insert(key, until),
                    None => map.remove(&key),
                }
            };
            if prev != next {
                tracing::debug!(project, thread = %r.id, ?next, "사용량 한도 표지");
                let _ = self.thread_events.send(self.thread_view(r));
            }
        }
        self.usage_limits.lock().unwrap().retain(|k, _| k.0 != project || seen.contains(k));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{ProviderUsage, UsageProblem, UsageWindow, UsageWindowKind as W};

    const NOW: u64 = 1_800_000_000_000;

    fn window(kind: W, used: f64, scope: Option<&str>, resets_ms: Option<u64>) -> UsageWindow {
        UsageWindow {
            kind,
            scope: scope.map(str::to_string),
            used_percent: used,
            resets_at: resets_ms.map(aaa_common::util::utc_iso),
            starts_at: None,
        }
    }

    fn account(id: P, key: &str, windows: Vec<UsageWindow>) -> ProviderUsage {
        ProviderUsage { id, key: key.into(), account: None, plan: None, windows, problem: None, checked_at: None }
    }

    #[test]
    fn full_window_limits_until_it_resets() {
        let hour = 3_600_000;
        let r = UsageReport {
            providers: vec![account(
                P::Claude,
                "a",
                vec![
                    window(W::Session, 100.0, None, Some(NOW + hour)),
                    window(W::Week, 40.0, None, Some(NOW + 9 * hour)),
                ],
            )],
        };
        assert_eq!(limited_until("claude", "opus", &r, NOW), Some(NOW + hour));
        // 다시 차오르는 때가 지나면 오래된 숫자라도 한도가 아니다
        assert_eq!(limited_until("claude", "opus", &r, NOW + hour), None);
        // 다른 harness는 그 구독을 보지 않는다
        assert_eq!(limited_until("codex", "gpt-5", &r, NOW), None);
        assert_eq!(limited_until("bash", "", &r, NOW), None);
    }

    #[test]
    fn not_full_or_unknown_reset_is_not_limited() {
        let r = UsageReport {
            providers: vec![account(
                P::Codex,
                "a",
                vec![window(W::Session, 99.9, None, Some(NOW + 1)), window(W::Week, 100.0, None, None)],
            )],
        };
        assert_eq!(limited_until("codex", "gpt-5", &r, NOW), None);
    }

    #[test]
    fn two_full_windows_wait_for_the_later() {
        let r = UsageReport {
            providers: vec![account(
                P::Claude,
                "a",
                vec![window(W::Session, 100.0, None, Some(NOW + 10)), window(W::Week, 100.0, None, Some(NOW + 20))],
            )],
        };
        assert_eq!(limited_until("claude", "sonnet", &r, NOW), Some(NOW + 20));
    }

    #[test]
    fn scoped_window_only_for_that_model() {
        let r = UsageReport {
            providers: vec![account(P::Claude, "a", vec![window(W::Week, 100.0, Some("Sonnet"), Some(NOW + 10))])],
        };
        assert_eq!(limited_until("claude", "claude-sonnet-5-5", &r, NOW), Some(NOW + 10));
        assert_eq!(limited_until("claude", "opus", &r, NOW), None);
    }

    #[test]
    fn two_accounts_are_not_guessed() {
        let full = || vec![window(W::Session, 100.0, None, Some(NOW + 10))];
        let mut stale = account(P::Claude, "b", full());
        stale.problem = Some(UsageProblem::Failed);
        let r = UsageReport { providers: vec![account(P::Claude, "a", full()), stale] };
        assert_eq!(limited_until("claude", "opus", &r, NOW), None);
        // 한 계정의 오래된(읽기 실패) 숫자는 그대로 쓴다: 다시 차오르는 때까지는 줄지 않는다
        let r = UsageReport { providers: vec![r.providers[1].clone()] };
        assert_eq!(limited_until("claude", "opus", &r, NOW), Some(NOW + 10));
        assert!(any_full(&r, NOW));
        assert!(!any_full(&r, NOW + 10));
    }
}
