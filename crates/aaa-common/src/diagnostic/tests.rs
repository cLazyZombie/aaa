use super::*;
use aaa_proto::{ErrorBody, ErrorCode, Outcome};
use std::sync::{Arc, Mutex};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id, Record},
};

#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);
struct Fields(String);
impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.0.push_str(&format!("{}={value:?} ", field.name()));
    }
}
impl Subscriber for Log {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &Record<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

#[test]
fn io_cause_survives_context_wire_and_log_without_private_context() {
    let log = Log::default();
    let private = "private-auth-file-contents";
    let source = anyhow::Error::new(std::io::Error::from_raw_os_error(5)).context(private);
    let error = tracing::subscriber::with_default(log.clone(), || {
        context(source, ErrorBody::new(ErrorCode::Unavailable, "인증 파일 읽기 실패"))
    });
    let bytes = serde_json::to_vec(&Outcome::<()>::Err(error)).unwrap();
    let decoded: Outcome<()> = serde_json::from_slice(&bytes).unwrap();
    let error = decoded.into_result().unwrap_err();
    assert_eq!(error.code, ErrorCode::Unavailable);
    assert!(error.message.contains("os error 5"), "{}", error.message);
    assert!(!error.message.contains(private));
    let events = log.0.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("os error 5"));
    assert!(!events[0].contains(private));
}

#[test]
fn parser_details_never_copy_the_rejected_secret_value() {
    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Mode {
        Allowed,
    }
    let private = "private-token-marker";
    let source = serde_json::from_str::<Mode>(&format!("\"{private}\"")).unwrap_err();
    assert!(source.to_string().contains(private), "시험은 원문을 포함한 serde 오류를 써야 한다");
    let log = Log::default();
    let error = tracing::subscriber::with_default(log.clone(), || {
        context(source, ErrorBody::new(ErrorCode::BadRequest, "인증 형식 오류"))
    });
    assert!(error.message.contains("JSON data (line 1, column"));
    assert!(!error.message.contains(private));
    assert!(!log.0.lock().unwrap().join("\n").contains(private));
}

#[test]
fn ignored_send_error_does_not_format_its_private_payload() {
    let log = Log::default();
    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    drop(rx);
    tracing::subscriber::with_default(log.clone(), || {
        ignored(tx.send("private-send-payload".into()), "수신자 종료 뒤 정리 계속");
    });
    let events = log.0.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].contains("수신자 종료 뒤 정리 계속"));
    assert!(!events[0].contains("private-send-payload"));
}

#[test]
fn internal_error_body_is_not_a_proof_that_its_message_is_safe() {
    let log = Log::default();
    let error = tracing::subscriber::with_default(log.clone(), || {
        context(
            ErrorBody::new(ErrorCode::BadRequest, "invalid type: private-config-value"),
            ErrorBody::new(ErrorCode::BadRequest, "작업 준비 실패"),
        )
    });
    assert_eq!(error.code, ErrorCode::BadRequest);
    assert!(error.message.contains("하위 오류 bad_request"));
    assert!(!error.message.contains("private-config-value"));
    assert!(!log.0.lock().unwrap().join("\n").contains("private-config-value"));
}
