//! 계정 pane 시작 오류는 원인 사슬을 보존하고 인증을 가린다.
use aaa_proto::{ErrorBody, ErrorCode};

pub(super) fn error(error: anyhow::Error) -> ErrorBody {
    let message = aaa_common::redact::text(&format!("계정 pane을 시작하지 못했다: {error:#}"));
    tracing::warn!(reason = %message, "계정 pane 시작 실패");
    ErrorBody::new(ErrorCode::BadRequest, message)
}
