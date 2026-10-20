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
