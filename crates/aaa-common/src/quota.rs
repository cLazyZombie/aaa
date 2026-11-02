//! 구독 창 정규화와 신선도. 로컬 파일 시각·수신 시각으로 관측 시각을 바꾸지 않는다.
use aaa_proto::{QuotaFreshness, QuotaWindow};
use serde_json::Value;

pub mod claude;

pub const FRESH_MS: u64 = 120_000;
/// 리셋을 모르는 창만 쓰는 보수적인 표시·배정 상한.
pub const UNKNOWN_RESET_MS: u64 = 600_000;
pub const CLOCK_SKEW_MS: u64 = 30_000;

pub fn freshness(w: &QuotaWindow, now: u64) -> QuotaFreshness {
    if !valid(w)
        || w.observed_at_ms > now.saturating_add(CLOCK_SKEW_MS)
        || w.resets_at_ms.is_some_and(|r| r <= now)
        || (w.resets_at_ms.is_none() && now.saturating_sub(w.observed_at_ms) > UNKNOWN_RESET_MS)
    {
        QuotaFreshness::Unknown
    } else if now.saturating_sub(w.observed_at_ms) > FRESH_MS {
        QuotaFreshness::Stale
    } else {
        QuotaFreshness::Fresh
    }
}

/// 새 작업의 한도·문턱 판정은 리셋 전, 최대 한 창 길이 동안 마지막 관측을 쓴다.
/// 잘못된 먼 리셋이 영구 차단을 만들지 않게 관측 시각 기준 창 길이로도 제한한다.
pub fn usable_for_allocation(w: &QuotaWindow, now: u64) -> bool {
    freshness(w, now) != QuotaFreshness::Unknown
        && now.saturating_sub(w.observed_at_ms) <= w.duration_mins.map_or(UNKNOWN_RESET_MS, |n| u64::from(n) * 60_000)
}

pub fn valid(w: &QuotaWindow) -> bool {
    safe_id(&w.limit_id)
        && safe_id(&w.window_id)
        && w.scope.as_ref().is_none_or(|s| safe_id(s))
        && w.used_percent.is_finite()
        && (0.0..=100.0).contains(&w.used_percent)
        && w.observed_at_ms > 0
        && w.observed_at_ms <= 8_640_000_000_000_000
        && w.duration_mins.is_none_or(|n| n > 0)
        && w.resets_at_ms
            .is_none_or(|r| r <= 8_640_000_000_000_000 && r >= w.observed_at_ms.saturating_sub(CLOCK_SKEW_MS))
}

// 공급자 문자열에서 메일·URL·제어 문자·원문 오류를 받을 수 없게 한다.
fn safe_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_ .".contains(&b))
}
fn used(v: &Value) -> Option<f64> {
    v.as_f64().filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
}
fn seconds(v: Option<&Value>) -> Option<u64> {
    v?.as_u64()?.checked_mul(1000).filter(|t| *t <= 8_640_000_000_000_000)
}

/// 공식 app-server 0.163의 camelCase. limitId별 창을 유지하고 primary/secondary 별칭을 합치지 않는다.
pub fn codex(v: &Value, now: u64) -> Vec<QuotaWindow> {
    let buckets: Vec<(&str, &Value)> =
        if let Some(m) = v.get("rateLimitsByLimitId").and_then(Value::as_object).filter(|m| !m.is_empty()) {
            m.iter().map(|(k, v)| (k.as_str(), v)).collect()
        } else {
            v.get("rateLimits").map(|v| vec![("codex", v)]).unwrap_or_default()
        };
    let mut out = vec![];
    for (id, bucket) in buckets.into_iter().take(32) {
        if !safe_id(id) {
            continue;
        }
        for key in ["primary", "secondary"] {
            let Some(w) = bucket.get(key).filter(|w| w.is_object()) else { continue };
            let Some(p) = w.get("usedPercent").and_then(used) else { continue };
            let item = QuotaWindow {
                limit_id: id.into(),
                window_id: key.into(),
                scope: if id == "codex" { None } else { Some(id.into()) },
                duration_mins: w
                    .get("windowDurationMins")
                    .and_then(Value::as_u64)
                    .and_then(|n| {
                        u32::try_from(n)
                            .inspect_err(|error| {
                                crate::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()
                    })
                    .filter(|n| *n > 0),
                used_percent: p,
                observed_at_ms: now,
                resets_at_ms: seconds(w.get("resetsAt")),
            };
            if valid(&item) {
                out.push(item);
            }
        }
    }
    out
}

/// Claude 상태줄 stdin. 없는 창은 덮어쓰지 않으며 이전 창의 관측 시각도 갱신하지 않는다.
pub fn claude(v: &Value, now: u64, previous: &[QuotaWindow]) -> Vec<QuotaWindow> {
    claude::read(v, now, previous).windows
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn missing_windows_keep_their_own_time_and_reset_never_invents_quota() {
        let old = claude(
            &json!({"rate_limits":{"five_hour":{"used_percentage":40,"resets_at":9999},"seven_day":{"used_percentage":20,"resets_at":19999}}}),
            1000,
            &[],
        );
        let new = claude(&json!({"rate_limits":{"five_hour":{"used_percentage":60,"resets_at":9999}}}), 2000, &old);
        assert_eq!(new.iter().find(|w| w.duration_mins == Some(10080)).unwrap().observed_at_ms, 1000);
        let w = &old[0];
        assert_eq!(freshness(w, 121000), QuotaFreshness::Fresh);
        assert_eq!(freshness(w, 121001), QuotaFreshness::Stale);
        assert_eq!(freshness(w, 601001), QuotaFreshness::Stale);
        assert_eq!(freshness(w, 9_999_000), QuotaFreshness::Unknown);
        assert!(claude(&json!({"rate_limits":{"five_hour":{"used_percentage":"NaN"}}}), 3000, &[]).is_empty());
        let mut w = w.clone();
        w.used_percent = f64::NAN;
        assert_eq!(freshness(&w, 1000), QuotaFreshness::Unknown);
        w.used_percent = 10.;
        w.observed_at_ms = 31_001;
        assert_eq!(freshness(&w, 1000), QuotaFreshness::Unknown);
    }
    // 필요: 쉬는 pane의 관측 유지와 배정 상한을 실제 대기 없이 창별 경계로 검사한다.
    #[test]
    fn observations_last_until_reset_and_allocation_is_bounded_by_window_length() {
        for (key, duration) in [("five_hour", 300_u64), ("seven_day", 10080)] {
            let reset = 1000 + duration * 60_000;
            let mut w =
                claude(&json!({"rate_limits":{key:{"used_percentage":100,"resets_at":reset / 1000}}}), 1000, &[])
                    .remove(0);
            for now in [121_001, 601_001, reset - 1] {
                assert_eq!(freshness(&w, now), QuotaFreshness::Stale);
                assert!(usable_for_allocation(&w, now));
            }
            assert_eq!(freshness(&w, reset), QuotaFreshness::Unknown);
            assert!(!usable_for_allocation(&w, reset));
            assert_eq!(w.used_percent, 100., "리셋으로 남은 양을 만들지 않는다");
            w.resets_at_ms = Some(reset + 60_000);
            assert_eq!(freshness(&w, reset + 1), QuotaFreshness::Stale);
            assert!(!usable_for_allocation(&w, reset + 1), "먼 리셋도 한 창 길이까지만 배정에 쓴다");
            w.resets_at_ms = None;
            assert!(usable_for_allocation(&w, 1000 + UNKNOWN_RESET_MS));
            assert_eq!(freshness(&w, 1001 + UNKNOWN_RESET_MS), QuotaFreshness::Unknown);
        }
    }

    #[test]
    fn codex_keeps_model_buckets_and_unknown_duration() {
        let v = json!({"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":20,"windowDurationMins":300,"resetsAt":9999},"secondary":{"usedPercent":50,"windowDurationMins":10080}},"spark":{"primary":{"usedPercent":70,"windowDurationMins":15}},"token@example.com":{"primary":{"usedPercent":1}}}});
        let w = codex(&v, 1000);
        assert_eq!(w.len(), 3);
        assert_eq!(w[2].scope.as_deref(), Some("spark"));
        assert_eq!(w[2].duration_mins, Some(15));
        assert!(codex(&json!({"rateLimits":{"primary":{"usedPercent":101}}}), 1000).is_empty());
        assert!(codex(&json!({"rateLimits":{"primary":{"usedPercent":"NaN"}}}), 1000).is_empty());
    }
}
