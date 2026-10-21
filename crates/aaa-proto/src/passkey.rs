//! 조작 기기 passkey(관리 GUI W7): 공개 주소(HTTPS)로 연 조작 기기는 승인 필요 동작마다 사용자 확인(UV)을 한 번 한다(답 1A).
//! 등록은 짝지은 뒤 `PASSKEY_REGISTER_WINDOW_MS` 안에서만 한다(답 2A). host Mac에서 연 loopback 화면(`127.0.0.1`)은 IP라
//! passkey를 쓸 수 없어 지금처럼 확인 창만으로 승인한다. 바이트 칸은 모두 base64url(채움 없음)이다.
//! HTTP: `POST /api/v1/passkey/register/begin`(본문 없음) → [`PasskeyCreation`], `POST /api/v1/passkey/register`
//! ([`PasskeyRegistration`]), `POST /api/v1/passkey/approve/begin`(본문은 승인할 `FrontRequest`) → [`PasskeyChallenge`],
//! `POST /api/v1/passkey/approve`([`PasskeyAssertion`]) → 그 요청의 `FrontResponse`.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 짝지은 뒤 passkey를 등록할 수 있는 시간(답 2A: pairing 코드 TTL과 같은 10분). 지나면 다시 짝짓는다
pub const PASSKEY_REGISTER_WINDOW_MS: u64 = 10 * 60 * 1000;

/// 이 화면(origin)의 passkey 상태(`/api/v1/me`의 `passkey`). loopback 화면에는 없다: 승인에 passkey가 필요 없다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PasskeyState {
    /// passkey가 묶이는 이름(공개 주소의 도메인)
    pub rp_id: String,
    /// 이 기기에 이 이름의 passkey가 있다
    pub registered: bool,
    /// 아직 등록할 수 있으면 그 끝(짝지은 때 + 10분). 지났거나 보기 기기면 없다
    #[ts(type = "number | null")]
    pub register_until_ms: Option<u64>,
}

/// 등록을 시작한다: `navigator.credentials.create`에 넣을 값
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PasskeyCreation {
    pub op_id: String,
    pub challenge: String,
    pub rp_id: String,
    /// passkey 목록에 보일 이름(aaa, 둘째 aaa면 인스턴스 이름을 붙인다)
    pub rp_name: String,
    /// 기기 ID(base64url): 같은 기기의 passkey를 덮는다
    pub user_id: String,
    pub user_name: String,
    /// 이 기기에 이미 있는 credential(같은 인증기에 두 번 만들지 않는다)
    pub exclude: Vec<String>,
}

/// 등록을 마친다: 인증기가 만든 credential(attestation은 「none」만 쓴다)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PasskeyRegistration {
    pub op_id: String,
    pub credential_id: String,
    pub client_data_json: String,
    pub attestation_object: String,
}

/// 승인을 시작한다: challenge는 op ID와 host가 다시 계산한 요청 hash만 묶는다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PasskeyChallenge {
    pub op_id: String,
    pub challenge: String,
    pub rp_id: String,
    /// 이 기기의 credential
    pub allow: Vec<String>,
}

/// 승인을 마친다: host는 begin 때 받아 둔 요청을 이 서명이 맞을 때 한 번만 실행한다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PasskeyAssertion {
    pub op_id: String,
    pub credential_id: String,
    pub client_data_json: String,
    pub authenticator_data: String,
    pub signature: String,
}
