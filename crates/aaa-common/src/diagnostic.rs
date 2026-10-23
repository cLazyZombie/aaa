//! 원문을 포맷하지 않는 오류 진단. 인증 JSON·사용자 입력·자식 argv를 진단에 복제하지 않는다.
use std::error::Error as _;
use std::{any::Any, fmt, panic::Location};

/// 공개 오류와 로그에 함께 쓸 수 있는 값만 보관한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cause {
    pub kind: &'static str,
    pub os_code: Option<i32>,
    pub position: Option<(usize, usize)>,
    pub at: &'static Location<'static>,
}

type Detail = (&'static str, Option<i32>, Option<(usize, usize)>);
fn known(error: &(dyn std::error::Error + 'static)) -> Option<Detail> {
    if let Some(error) = error.downcast_ref::<std::io::Error>() {
        // io::Error::other의 문자열에도 사용자 입력이 들어갈 수 있다.
        let kind = match error.kind() {
            std::io::ErrorKind::NotFound => "NotFound",
            std::io::ErrorKind::PermissionDenied => "PermissionDenied",
            std::io::ErrorKind::AlreadyExists => "AlreadyExists",
            std::io::ErrorKind::InvalidData => "InvalidData",
            std::io::ErrorKind::InvalidInput => "InvalidInput",
            std::io::ErrorKind::UnexpectedEof => "UnexpectedEof",
            std::io::ErrorKind::BrokenPipe => "BrokenPipe",
            std::io::ErrorKind::ConnectionRefused => "ConnectionRefused",
            std::io::ErrorKind::ConnectionReset => "ConnectionReset",
            std::io::ErrorKind::TimedOut => "TimedOut",
            std::io::ErrorKind::WouldBlock => "WouldBlock",
            _ => "I/O",
        };
        return Some((kind, error.raw_os_error(), None));
    }
    if let Some(error) = error.downcast_ref::<serde_json::Error>() {
        let kind = match error.classify() {
            serde_json::error::Category::Io => "JSON I/O",
            serde_json::error::Category::Syntax => "JSON syntax",
            serde_json::error::Category::Data => "JSON data",
            serde_json::error::Category::Eof => "JSON EOF",
        };
        return Some((
            kind,
            error
                .io_error_kind()
                .and_then(|_| error.source())
                .and_then(|s| s.downcast_ref::<std::io::Error>())
                .and_then(std::io::Error::raw_os_error),
            Some((error.line(), error.column())),
        ));
    }
    if let Some(error) = error.downcast_ref::<tokio::task::JoinError>() {
        return Some((if error.is_panic() { "task panic" } else { "task cancelled" }, None, None));
    }
    None
}

impl Cause {
    #[track_caller]
    pub fn capture<E: Any>(error: &E) -> Self {
        let any = error as &dyn Any;
        let detail = if let Some(e) = any.downcast_ref::<std::io::Error>() {
            known(e)
        } else if let Some(e) = any.downcast_ref::<serde_json::Error>() {
            known(e)
        } else if let Some(e) = any.downcast_ref::<tokio::task::JoinError>() {
            known(e)
        } else if let Some(e) = any.downcast_ref::<aaa_proto::ErrorBody>() {
            // 내부 ErrorBody에도 파서의 입력 값이 섞일 수 있다. 원문이 아닌 코드만 보존한다.
            let kind = match e.code {
                aaa_proto::ErrorCode::BadRequest => "하위 오류 bad_request",
                aaa_proto::ErrorCode::Unavailable => "하위 오류 unavailable",
                aaa_proto::ErrorCode::Internal => "하위 오류 internal",
                aaa_proto::ErrorCode::Forbidden => "하위 오류 forbidden",
                aaa_proto::ErrorCode::Unauthorized => "하위 오류 unauthorized",
                aaa_proto::ErrorCode::NotFound => "하위 오류 not_found",
                aaa_proto::ErrorCode::AgentDisconnected => "하위 오류 agent_disconnected",
                _ => "하위 프로토콜 오류",
            };
            Some((kind, None, None))
        } else if let Some(e) = any.downcast_ref::<anyhow::Error>() {
            // 맥락 문자열도 비밀 파일의 파서 오류를 포함할 수 있어 알려진 원인만 추출한다.
            e.chain().filter_map(known).last()
        } else {
            None
        };
        let (kind, os_code, position) = detail.unwrap_or((std::any::type_name::<E>(), None, None));
        Self { kind, os_code, position, at: Location::caller() }
    }
}
impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(code) = self.os_code {
            write!(f, " (os error {code})")?;
        }
        if let Some((line, column)) = self.position {
            write!(f, " (line {line}, column {column})")?;
        }
        write!(f, " [{}:{}]", self.at.file(), self.at.line())
    }
}

pub trait WithCause {
    fn with_cause(self, cause: Cause) -> Self;
}
impl WithCause for aaa_proto::ErrorBody {
    fn with_cause(mut self, cause: Cause) -> Self {
        self.message.push_str(&format!(": {cause}"));
        self
    }
}
impl WithCause for Box<aaa_proto::install::InstallResult> {
    fn with_cause(mut self, cause: Cause) -> Self {
        if let Some(error) = &mut self.error {
            error.details.push(cause.to_string());
        }
        for need in &mut self.needs_user {
            need.message.push_str(&format!(": {cause}"));
        }
        self
    }
}
impl WithCause for anyhow::Error {
    fn with_cause(self, cause: Cause) -> Self {
        self.context(cause.to_string())
    }
}
impl WithCause for String {
    fn with_cause(self, cause: Cause) -> Self {
        format!("{self}: {cause}")
    }
}

/// 오류를 고정 안내로 바꾸는 경계에서 한 번만 기록한다. 정상 종료·없음은 `ignored`를 쓴다.
#[track_caller]
pub fn context<E: Any, T: WithCause>(error: E, mapped: T) -> T {
    let cause = Cause::capture(&error);
    tracing::info!(error = %cause, "하위 오류를 안내에 보존한다");
    mapped.with_cause(cause)
}

/// 본 작업의 실패를 덮지 않는 최선 노력 정리·끊긴 수신자. stage에 계속하는 까닭을 적는다.
#[track_caller]
pub fn ignored<T, E: Any>(result: Result<T, E>, stage: &'static str) {
    if let Err(error) = result {
        observe(&error, stage);
    }
}

/// timeout의 바깥 오류와 실제 작업의 안쪽 오류를 모두 관측한다.
#[track_caller]
pub fn ignored_nested<T, E: Any, O: Any>(result: Result<Result<T, E>, O>, stage: &'static str) {
    match result {
        Ok(inner) => ignored(inner, stage),
        Err(error) => observe(&error, stage),
    }
}

/// Option으로 내려 fallback을 고르는 이유는 호출부에 둔다. 원문은 읽지 않으며 정상 미존재도 debug로만 남긴다.
#[track_caller]
pub fn observe<E: Any>(error: &E, stage: &'static str) {
    tracing::debug!(stage, error = %Cause::capture(error), "보조 작업 실패 후 계속한다");
}

/// PoisonError의 guard처럼 수명이 있는 오류도 내부 데이터·잠금 내용을 포맷하지 않는다.
#[track_caller]
pub fn observe_type<E: ?Sized>(_: &E, stage: &'static str) {
    let at = Location::caller();
    tracing::debug!(
        stage,
        error_type = std::any::type_name::<E>(),
        file = at.file(),
        line = at.line(),
        "보조 작업 실패 후 계속한다"
    );
}

#[cfg(test)]
mod tests;
