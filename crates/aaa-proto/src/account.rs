//! 계정 인증은 전용 요청에서만 전송한다. 일반 실행 spec·journal에는 참조만 둔다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(type = "string")]
pub struct SecretText(pub String);

impl std::fmt::Debug for SecretText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[비밀]")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountView {
    pub name: String,
    pub harness: String,
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountCredential {
    pub run: String,
    pub account: String,
    pub harness: String,
    pub version: u64,
    pub secret: SecretText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRefresh {
    pub run: String,
    pub session: String,
    pub id: String,
    pub version: u64,
    pub request: SecretText,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRefreshed {
    pub id: String,
    pub version: u64,
    pub response: Option<SecretText>,
}

/// 인증값을 포함하지 않는 harness별 기본 설정.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountDefault {
    pub harness: String,
    pub account: String,
}

/// 공식 로그인 자식의 원본 출력은 공개하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountLoginView {
    pub id: String,
    pub account: String,
    pub harness: String,
    pub state: String,
}

/// coordinator는 thread와 다른 이름 공간을 쓴다. 경로 탈출은 양쪽 모두 거부한다.
pub fn account_run_parts(run: &str) -> Option<(&str, &str)> {
    let (project, task) = run.split_once('/')?;
    let coordinator = task
        .strip_prefix("c-")
        .is_some_and(|id| id.len() == 26 && id.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()));
    (crate::valid_project(project) && (crate::valid_thread(task) || coordinator)).then_some((project, task))
}
