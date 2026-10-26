//! 실행할 profile·고정 계정의 구독 창만 판정한다. 외부 조회는 ticker가 맡는다.
use super::Limits;
use crate::{Host, auto_start::Decision};
use aaa_proto::{AccountQuota, ErrorBody, UsageProviderId, UsageReport, UsageWindowKind};

#[derive(Debug, PartialEq)]
pub(super) enum Check {
    Allowed,
    Low,
    Unknown(String),
}
impl Check {
    pub(super) fn blocked(&self) -> bool {
        !matches!(self, Self::Allowed)
    }
}

fn thresholds(limits: &Limits) -> [(u32, Option<f64>); 2] {
    [(300, limits.usage_session_below), (10080, limits.usage_week_below)]
}

fn registered(limits: &Limits, quota: &AccountQuota, now: u64) -> Check {
    if crate::account_pool::below_usage_threshold(quota, [limits.usage_session_below, limits.usage_week_below], now) {
        Check::Low
    } else {
        Check::Allowed
    }
}

/// 계정 미지정의 로컬 실행만 기존 로그인 캐시를 쓴다. 다른 공급자·다중 계정은 대입하지 않는다.
fn local(limits: &Limits, report: &UsageReport, harness: &str) -> Check {
    let provider = match harness {
        "claude" => UsageProviderId::Claude,
        "codex" => UsageProviderId::Codex,
        _ => return Check::Unknown("profile의 구독 공급자를 모름".into()),
    };
    let accounts: Vec<_> = report.providers.iter().filter(|p| p.id == provider).collect();
    if accounts.len() != 1 || accounts[0].problem.is_some() {
        return Check::Unknown(format!("로컬 {harness} 로그인 계정 불명 또는 조회 실패"));
    }
    let mut low = false;
    for (duration, threshold) in thresholds(limits) {
        let Some(threshold) = threshold else { continue };
        let kind = if duration == 300 { UsageWindowKind::Session } else { UsageWindowKind::Week };
        let windows: Vec<_> = accounts[0].windows.iter().filter(|w| w.kind == kind && w.scope.is_none()).collect();
        if windows.len() != 1 || !windows[0].used_percent.is_finite() {
            return Check::Unknown(format!("로컬 {harness} · {kind:?} 창 불명"));
        }
        low |= 100.0 - windows[0].used_percent < threshold;
    }
    if low { Check::Low } else { Check::Allowed }
}

impl Host {
    pub(super) async fn automation_start_usage(
        &self,
        project: &str,
        decision: &Decision,
        limits: &Limits,
    ) -> Result<Check, ErrorBody> {
        if limits.usage_session_below.is_none() && limits.usage_week_below.is_none() {
            return Ok(Check::Allowed);
        }
        let (harness, account, local_machine) = match decision {
            Decision::Ready(choice) => {
                let profile = self.resolved_profile(Some(project), &choice.machine, &choice.profile)?;
                if self.pool_harness_enabled(&profile.harness)? {
                    let accounts = self.accounts.list()?;
                    let mut low = false;
                    for m in self.account_pool()?.members {
                        if !accounts.iter().any(|a| a.name == m.account && a.harness == profile.harness) {
                            continue;
                        }
                        if crate::account_meta::health(&self.account_meta(&m.account), aaa_common::util::now_ms())
                            .broken()
                        {
                            continue;
                        }
                        let Ok(quota) = self.cached_account_quota(&m.account, &profile.harness) else {
                            return Ok(Check::Allowed);
                        };
                        if registered(limits, &quota, aaa_common::util::now_ms()) != Check::Low {
                            return Ok(Check::Allowed);
                        }
                        low = true;
                    }
                    return Ok(if low { Check::Low } else { Check::Allowed });
                }
                let account = self.default_account(&profile.harness)?;
                (profile.harness, account, self.machine_is_local(&choice.machine))
            }
            Decision::Reuse(id) => {
                let rec = self.load(project, id)?;
                let harness = rec.launch.as_ref().map(|l| l.harness.clone()).map(Ok).unwrap_or_else(|| {
                    self.resolved_profile(Some(project), &rec.machine_id, &rec.profile).map(|p| p.harness)
                })?;
                // 기본 계정이 바뀌어도 열린 thread의 인증은 바뀌지 않는다.
                (harness, rec.account, self.machine_is_local(&rec.machine_id))
            }
            _ => return Ok(Check::Allowed),
        };
        if let Some(account) = account {
            let quota = match self.cached_account_quota(&account, &harness) {
                Ok(quota) => quota,
                Err(error) => {
                    tracing::debug!(%harness, code = ?error.code, "자동화 사용량 판단 불가, 시작 허용: {}", error.message);
                    return Ok(Check::Allowed);
                }
            };
            return Ok(registered(limits, &quota, aaa_common::util::now_ms()));
        }
        if !local_machine {
            return Ok(Check::Allowed);
        }
        Ok(match local(limits, &self.usage.cached().await, &harness) {
            Check::Unknown(_) => Check::Allowed,
            check => check,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::*;
    const NOW: u64 = 1_000_000;
    fn quota() -> AccountQuota {
        AccountQuota {
            account: "work".into(),
            harness: "codex".into(),
            problem: None,
            retry_at_ms: None,
            windows: [(300, 95.0), (10080, 90.0)]
                .into_iter()
                .map(|(duration, used)| AccountQuotaWindow {
                    window: QuotaWindow {
                        limit_id: "codex".into(),
                        window_id: format!("w-{duration}"),
                        scope: None,
                        duration_mins: Some(duration),
                        used_percent: used,
                        observed_at_ms: NOW,
                        resets_at_ms: Some(NOW + 3_600_000),
                    },
                    machine: "host".into(),
                    source: QuotaSource::CodexRpc,
                    received_at_ms: NOW,
                    freshness: QuotaFreshness::Fresh,
                    problem: None,
                    conflict: false,
                })
                .collect(),
        }
    }
    fn limits() -> Limits {
        Limits { usage_session_below: Some(5.0), usage_week_below: Some(10.0), ..Default::default() }
    }
    #[test]
    fn quota_gate_uses_remaining_percent_of_observed_windows() {
        let mut q = quota();
        assert_eq!(registered(&limits(), &q, NOW), Check::Allowed, "문턱과 같으면 실행");
        q.windows[1].window.used_percent = 90.1;
        assert_eq!(registered(&limits(), &q, NOW), Check::Low);
        q.windows.remove(0);
        assert_eq!(registered(&limits(), &q, NOW), Check::Low, "5시간이 없어도 주간 문턱은 적용한다");
        q.windows[0].window.used_percent = 19.0;
        assert_eq!(registered(&Limits { usage_week_below: Some(10.0), ..Default::default() }, &q, NOW), Check::Allowed);
    }
    #[test]
    fn quota_gate_rejects_stale_reset_invalid_conflicting_and_scoped_windows() {
        for case in 0..7 {
            let mut q = quota();
            let w = &mut q.windows[0];
            match case {
                0 => w.window.observed_at_ms = NOW - aaa_common::quota::FRESH_MS - 1,
                1 => w.window.resets_at_ms = Some(NOW),
                2 => w.window.used_percent = f64::NAN,
                3 => w.conflict = true,
                4 => w.problem = Some(QuotaProblem::Expired),
                5 => w.window.scope = Some("spark".into()),
                _ => w.freshness = QuotaFreshness::Unknown,
            }
            assert_eq!(registered(&limits(), &q, NOW), Check::Allowed, "case {case}: 판단 불가는 실행");
        }
    }
    #[test]
    fn quota_gate_local_fallback_does_not_mix_providers_or_accounts() {
        let account = |id, key: &str, used| ProviderUsage {
            id,
            key: key.into(),
            account: None,
            plan: None,
            problem: None,
            checked_at: None,
            windows: [UsageWindowKind::Session, UsageWindowKind::Week]
                .into_iter()
                .map(|kind| UsageWindow { kind, scope: None, used_percent: used, resets_at: None, starts_at: None })
                .collect(),
        };
        let mut report = UsageReport {
            providers: vec![
                account(UsageProviderId::Claude, "claude", 100.0),
                account(UsageProviderId::Codex, "codex", 50.0),
            ],
        };
        assert_eq!(local(&limits(), &report, "codex"), Check::Allowed);
        report.providers.pop();
        assert!(matches!(local(&limits(), &report, "codex"), Check::Unknown(_)));
        report.providers =
            vec![account(UsageProviderId::Codex, "one", 50.0), account(UsageProviderId::Codex, "two", 50.0)];
        assert!(matches!(local(&limits(), &report, "codex"), Check::Unknown(_)));
    }
}
