//! 성공한 사용량 응답에서 고정된 JSON 유형만 기록한다. 값·키 목록·원문은 보관하지 않는다.
use serde_json::{Map, Value};

fn kind(value: Option<&Value>) -> &'static str {
    match value {
        None => "missing",
        Some(Value::Null) => "null",
        Some(Value::Bool(_)) => "boolean",
        Some(Value::Number(_)) => "number",
        Some(Value::String(_)) => "string",
        Some(Value::Array(_)) => "array",
        Some(Value::Object(_)) => "object",
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Shape {
    five_hour: &'static str,
    utilization: &'static str,
    resets_at: &'static str,
}

fn shape(body: &Map<String, Value>) -> Shape {
    let window = body.get("five_hour");
    Shape {
        five_hour: kind(window),
        utilization: kind(window.and_then(|v| v.get("utilization"))),
        resets_at: kind(window.and_then(|v| v.get("resets_at"))),
    }
}

pub(super) fn record(body: &Map<String, Value>) {
    let d = shape(body);
    tracing::info!(
        source = "claude_oauth",
        five_hour_type = d.five_hour,
        utilization_type = d.utilization,
        resets_at_type = d.resets_at,
        "Claude 5시간 응답 진단"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_null_and_types_are_distinct_without_any_provider_values() {
        let secret = aaa_test_support::token("window-shape-private");
        for (window, expected) in [
            (Value::Null, "null"),
            (json!(true), "boolean"),
            (json!(0), "number"),
            (json!(secret), "string"),
            (json!([secret]), "array"),
            (json!({"utilization":secret,"resets_at":secret}), "object"),
        ] {
            let body = json!({"five_hour":window,"unrelated":secret});
            let d = shape(body.as_object().unwrap());
            assert_eq!(d.five_hour, expected);
            let safe = format!("{d:?}");
            assert!(!safe.contains(&secret));
            for bytes in secret.as_bytes().windows(8) {
                assert!(!safe.as_bytes().windows(8).any(|part| part == bytes));
            }
        }
        assert_eq!(shape(json!({}).as_object().unwrap()).five_hour, "missing");
        let d = shape(json!({"five_hour":{"utilization":0,"resets_at":null}}).as_object().unwrap());
        assert_eq!(d.utilization, "number");
        assert_eq!(d.resets_at, "null");
    }
}
