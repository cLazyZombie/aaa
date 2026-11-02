//! 상태줄의 알려진 두 창만 판정한다. 진단에는 입력 숫자·문자열·임의 키를 복제하지 않는다.
use super::{seconds, used, valid};
use aaa_proto::QuotaWindow;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Missing,
    Invalid,
    Present,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Missing,
    InvalidContainer,
    InvalidWindow,
    InvalidPercentage,
    InvalidReset,
    InvalidObservation,
    Accepted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reset {
    Missing,
    Invalid,
    Elapsed,
    Future,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowDiagnostic {
    pub decision: Decision,
    pub reset: Reset,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub received_at_ms: u64,
    pub rate_limits: Container,
    pub five_hour: WindowDiagnostic,
    pub seven_day: WindowDiagnostic,
}

pub struct Reading {
    pub windows: Vec<QuotaWindow>,
    pub diagnostic: Diagnostic,
}

/// 진단과 저장할 창이 같은 파서 분기를 거친다. 없는 창·거부된 창은 이전 숫자와 시각을 보존한다.
pub fn read(v: &Value, now: u64, previous: &[QuotaWindow]) -> Reading {
    let limits = v.get("rate_limits");
    let container = match limits {
        None => Container::Missing,
        Some(value) if value.is_object() => Container::Present,
        Some(_) => Container::Invalid,
    };
    let (five_hour, five_diagnostic) = window(limits, "five_hour", 300, now);
    let (seven_day, seven_diagnostic) = window(limits, "seven_day", 10080, now);
    let mut windows: Vec<_> = previous.iter().filter(|w| valid(w)).cloned().collect();
    for item in [five_hour, seven_day].into_iter().flatten() {
        windows.retain(|w| w.duration_mins != item.duration_mins);
        windows.push(item);
    }
    Reading {
        windows,
        diagnostic: Diagnostic {
            received_at_ms: now,
            rate_limits: container,
            five_hour: five_diagnostic,
            seven_day: seven_diagnostic,
        },
    }
}

fn window(limits: Option<&Value>, key: &str, duration: u32, now: u64) -> (Option<QuotaWindow>, WindowDiagnostic) {
    let mut diagnostic = WindowDiagnostic { decision: Decision::Missing, reset: Reset::Missing };
    let Some(limits) = limits else { return (None, diagnostic) };
    if !limits.is_object() {
        diagnostic.decision = Decision::InvalidContainer;
        return (None, diagnostic);
    }
    let Some(input) = limits.get(key) else { return (None, diagnostic) };
    if !input.is_object() {
        diagnostic.decision = Decision::InvalidWindow;
        return (None, diagnostic);
    }
    let reset = seconds(input.get("resets_at"));
    diagnostic.reset = match (input.get("resets_at"), reset) {
        (None | Some(Value::Null), _) => Reset::Missing,
        (_, None) => Reset::Invalid,
        (_, Some(at)) if at <= now => Reset::Elapsed,
        (_, Some(_)) => Reset::Future,
    };
    let Some(percent) = input.get("used_percentage").and_then(used) else {
        diagnostic.decision = Decision::InvalidPercentage;
        return (None, diagnostic);
    };
    let item = QuotaWindow {
        limit_id: "claude".into(),
        window_id: key.into(),
        scope: None,
        duration_mins: Some(duration),
        used_percent: percent,
        observed_at_ms: now,
        resets_at_ms: reset,
    };
    if !valid(&item) {
        diagnostic.decision =
            if now == 0 || now > 8_640_000_000_000_000 { Decision::InvalidObservation } else { Decision::InvalidReset };
        return (None, diagnostic);
    }
    diagnostic.decision = Decision::Accepted;
    (Some(item), diagnostic)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // 필요: 누락과 거부를 나누면서 진단이 기존 관측 보존·수용 판단을 바꾸지 않아야 한다.
    #[test]
    fn diagnoses_the_parser_decision_and_keeps_previous_windows() {
        let old = read(
            &json!({"rate_limits":{"five_hour":{"used_percentage":40,"resets_at":9999},
                                     "seven_day":{"used_percentage":20,"resets_at":9999}}}),
            1000,
            &[],
        )
        .windows;
        for (input, decision, reset) in [
            (json!({}), Decision::Missing, Reset::Missing),
            (json!({"rate_limits":null}), Decision::InvalidContainer, Reset::Missing),
            (json!({"rate_limits":{"five_hour":null}}), Decision::InvalidWindow, Reset::Missing),
            (
                json!({"rate_limits":{"five_hour":{"used_percentage":"secret"}}}),
                Decision::InvalidPercentage,
                Reset::Missing,
            ),
            (json!({"rate_limits":{"five_hour":{"used_percentage":101}}}), Decision::InvalidPercentage, Reset::Missing),
            (
                json!({"rate_limits":{"five_hour":{"used_percentage":0,"resets_at":1}}}),
                Decision::InvalidReset,
                Reset::Elapsed,
            ),
        ] {
            let reading = read(&input, 120_000, &old);
            assert_eq!(reading.diagnostic.five_hour, WindowDiagnostic { decision, reset });
            assert_eq!(reading.windows, old, "누락·거부는 마지막 숫자와 관측 시각을 보존한다");
        }
        let reading = read(&json!({"rate_limits":{"seven_day":{"used_percentage":9,"resets_at":9999}}}), 120_000, &old);
        assert_eq!(reading.diagnostic.rate_limits, Container::Present);
        assert_eq!(reading.diagnostic.five_hour.decision, Decision::Missing);
        assert_eq!(reading.diagnostic.seven_day.decision, Decision::Accepted);
        assert_eq!(reading.windows[0], old[0]);
        assert_eq!(reading.windows[1].observed_at_ms, 120_000);
        assert_eq!(reading.windows[1].used_percent, 9.);
        assert_eq!(reading.diagnostic.received_at_ms, 120_000);
        // 기존 파서는 리셋이 없거나 형식이 틀리면 None으로 수용한다. 진단에서 그 차이만 알린다.
        for (value, reset) in
            [(json!(null), Reset::Missing), (json!("secret"), Reset::Invalid), (json!(9999), Reset::Future)]
        {
            let reading =
                read(&json!({"rate_limits":{"five_hour":{"used_percentage":0,"resets_at":value}}}), 120_000, &[]);
            assert_eq!(reading.diagnostic.five_hour, WindowDiagnostic { decision: Decision::Accepted, reset });
            assert_eq!(reading.windows[0].used_percent, 0.);
        }
    }
}
