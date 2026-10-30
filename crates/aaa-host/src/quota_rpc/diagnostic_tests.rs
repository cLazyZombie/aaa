use super::*;
use std::{fmt::Write as _, sync::Mutex};
use tracing::{
    Event, Metadata, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

#[derive(Clone, Default)]
struct Log(Arc<Mutex<String>>);
impl Visit for Log {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        writeln!(self.0.lock().unwrap(), "{}={value:?}", field.name()).unwrap();
    }
}
impl Subscriber for Log {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, attributes: &Attributes<'_>) -> Id {
        attributes.record(&mut self.clone());
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, values: &Record<'_>) {
        values.record(&mut self.clone());
    }
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        event.record(&mut self.clone());
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

// 필요: 원인 진단에 단계·상태·로컬 차단은 남기되 제공자가 돌려준 인증 조각은 기록하지 않는다.
#[test]
fn rpc_failure_logs_keep_stage_status_and_local_refusal_without_provider_material() {
    let secrets = ["diagnostic-access", "diagnostic-code", "diagnostic-refresh"].map(aaa_test_support::token);
    let url = format!("https://example.invalid/callback?code={}", secrets[1]);
    let reply = json!({"error": {
        "code": -32603,
        "message": format!("{} {url}", secrets[0]),
        "data": {"status": 401, "refresh_token": secrets[2]}
    }});
    let log = Log::default();
    tracing::subscriber::with_default(log.clone(), || {
        assert_eq!(error(&reply, 1000, "initialize", false), (QuotaProblem::Expired, None));
        assert_eq!(error(&reply, 1000, "account/rateLimits/read", true), (QuotaProblem::Expired, None));
        let message_only = json!({"error": {"message": format!("401 {url}")}});
        assert_eq!(error(&message_only, 1000, "account/rateLimits/read", false), (QuotaProblem::Expired, None));
    });
    let text = log.0.lock().unwrap();
    for required in ["initialize", "account/rateLimits/read", "Some(401)", "http_status=None", "refresh_blocked=true"] {
        assert!(text.contains(required), "진단 필드가 빠졌다: {required}");
    }
    assert!(!text.contains("example.invalid") && !text.contains("callback?code="));
    for secret in secrets {
        assert!(
            !secret.as_bytes().windows(8).any(|piece| text.as_bytes().windows(8).any(|logged| logged == piece)),
            "제공자 원문 인증의 조각을 기록하지 않는다"
        );
    }
}
