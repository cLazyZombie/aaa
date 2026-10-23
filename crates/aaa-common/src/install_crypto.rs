//! CLI와 host의 Ed25519 canonical bytes. 입력 공개키 자체를 신뢰 root로 삼지 않는다.
use aaa_proto::install::{InstallAuthorizationClaims, InstallEnrollmentGrant};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::{
    rand::SystemRandom,
    signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey},
};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
#[error("설치 서명 입력이 올바르지 않습니다")]
pub struct Invalid;

pub fn canonical(value: &impl Serialize) -> Result<Vec<u8>, Invalid> {
    fn sorted(value: serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(values) => {
                let values: std::collections::BTreeMap<_, _> =
                    values.into_iter().map(|(k, v)| (k, sorted(v))).collect();
                serde_json::to_value(values).expect("canonical map")
            }
            serde_json::Value::Array(values) => values.into_iter().map(sorted).collect(),
            value => value,
        }
    }
    serde_json::to_vec(&sorted(serde_json::to_value(value).map_err(|_| Invalid)?)).map_err(|_| Invalid)
}

pub struct Signer(Ed25519KeyPair);
impl Signer {
    pub fn generate() -> Result<Vec<u8>, Invalid> {
        Ok(Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| Invalid)?.as_ref().to_vec())
    }
    pub fn open(document: &[u8]) -> Result<Self, Invalid> {
        Ok(Self(Ed25519KeyPair::from_pkcs8(document).map_err(|_| Invalid)?))
    }
    pub fn public_key(&self) -> &[u8] {
        self.0.public_key().as_ref()
    }
    pub fn sign_authorization(&self, claims: &InstallAuthorizationClaims) -> Result<String, Invalid> {
        Ok(URL_SAFE_NO_PAD.encode(self.0.sign(&canonical(claims)?).as_ref()))
    }
    pub fn sign_enrollment(&self, grant: &InstallEnrollmentGrant) -> Result<String, Invalid> {
        Ok(URL_SAFE_NO_PAD.encode(self.0.sign(&grant_bytes(grant)?).as_ref()))
    }
}
fn verify(public: &[u8], data: &[u8], signature: &str) -> bool {
    let Ok(signature) = URL_SAFE_NO_PAD
        .decode(signature)
        .inspect_err(|error| crate::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return false;
    };
    UnparsedPublicKey::new(&ED25519, public)
        .verify(data, &signature)
        .as_ref()
        .inspect_err(|error| crate::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok()
}
pub fn verify_authorization(public: &[u8], claims: &InstallAuthorizationClaims, signature: &str) -> bool {
    canonical(claims)
        .inspect_err(|error| crate::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok_and(|data| verify(public, &data, signature))
}
fn grant_bytes(grant: &InstallEnrollmentGrant) -> Result<Vec<u8>, Invalid> {
    let mut value = serde_json::to_value(grant).map_err(|_| Invalid)?;
    value.as_object_mut().ok_or(Invalid)?.remove("signature");
    canonical(&value)
}
pub fn verify_enrollment(public: &[u8], grant: &InstallEnrollmentGrant) -> bool {
    grant_bytes(grant)
        .inspect_err(|error| crate::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
        .is_ok_and(|data| verify(public, &data, &grant.signature))
}
