//! 조작 기기 passkey(관리 GUI W7): 공개 주소(HTTPS)로 연 조작 기기는 승인 필요 동작마다 그 기기 passkey의
//! 사용자 확인(UV)을 한 번 한다. passkey가 없으면 거부하고 등록을 안내한다(답 1A). 등록은 짝지은 뒤 10분 안에서만 한다:
//! 훔친 cookie가 제 passkey를 다는 길을 막는다. 그 뒤 추가·교체는 다시 짝짓기다(답 2A). 웹에서 짝짓기 코드를 만드는 것도
//! UV 동작이라 passkey 없는 cookie는 새 기기를 들이지 못한다. RP ID는 공개 주소의 도메인이고, loopback(`127.0.0.1`) 화면은
//! IP라 passkey를 쓸 수 없어 지금처럼 확인 창만으로 승인한다(host Mac 앞의 사람, 기획의 마지막 수단).
//! 승인 challenge는 op ID와 host가 다시 계산한 요청 hash만 묶고, host는 begin 때 받아 둔 요청을 서명이 맞을 때 한 번만
//! 실행한다: 화면이 승인 뒤 다른 요청을 보내지 못하고, hash 칸이 없는 동작(기기 폐기·공개 끄기 등)도 같은 방식으로 묶인다.
//! 서명 횟수는 적기만 하고 거부에 쓰지 않는다(동기화되는 passkey는 늘 0이다). credential ID·서명은 기록에 쓰지 않는다.
mod cbor;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aaa_common::util;
use aaa_proto::{
    DeviceRole, ErrorBody, ErrorCode, FrontRequest, FrontResponse, PASSKEY_REGISTER_WINDOW_MS, PasskeyAssertion,
    PasskeyChallenge, PasskeyCreation, PasskeyRegistration, PasskeyState,
};
use base64::Engine;
use cbor::Cbor;
use ring::signature::{self, UnparsedPublicKey};
use sha2::{Digest, Sha256};

use crate::{Host, err};

/// begin에서 finish까지(브라우저 창 시한 2분과 같다)
const OP_TTL_MS: u64 = 2 * 60 * 1000;
/// 한꺼번에 기다리는 op 수의 상한(오래된 것부터 지운다)
const MAX_OPS: usize = 64;
/// 받는 credential ID 길이 상한(WebAuthn: 1023바이트)
const MAX_CREDENTIAL: usize = 1023;
/// COSE 알고리즘: ES256(P-256)·EdDSA(Ed25519)
pub(crate) const ALG_ES256: i64 = -7;
pub(crate) const ALG_EDDSA: i64 = -8;

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_AT: u8 = 0x40;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::URL_SAFE_NO_PAD;

enum Kind {
    Register,
    Approve(Box<FrontRequest>),
}

struct Op {
    device: String,
    origin: String,
    challenge: [u8; 32],
    expires_ms: u64,
    kind: Kind,
}

#[derive(Default)]
pub(crate) struct Ops(Mutex<HashMap<String, Op>>);

impl Ops {
    fn put(&self, id: &str, op: Op) {
        let mut m = self.0.lock().unwrap();
        let now = util::now_ms();
        m.retain(|_, o| o.expires_ms > now);
        while m.len() >= MAX_OPS {
            let Some(oldest) = m.iter().min_by_key(|(_, o)| o.expires_ms).map(|(k, _)| k.clone()) else { break };
            m.remove(&oldest);
        }
        m.insert(id.into(), op);
    }

    /// 한 번만 꺼낸다: 실패해도 다시 쓰지 못한다
    fn take(&self, id: &str, device: &str, origin: &str) -> Result<Op, ErrorBody> {
        let op = self.0.lock().unwrap().remove(id).ok_or_else(stale)?;
        if op.device != device || !op.origin.eq_ignore_ascii_case(origin) || op.expires_ms <= util::now_ms() {
            return Err(stale());
        }
        Ok(op)
    }
}

fn stale() -> ErrorBody {
    err(ErrorCode::BadRequest, "passkey 확인이 끝났거나 시한이 지났다. 다시 한다")
}

fn bad(msg: &str) -> ErrorBody {
    err(ErrorCode::Forbidden, format!("passkey 확인 실패: {msg}"))
}

/// 이 화면(origin)의 RP ID: 공개 주소의 도메인. loopback IP 화면은 없다
pub(crate) fn rp_of(origin: &str) -> Option<String> {
    let rest = origin.split_once("://")?.1;
    let name =
        rest.rsplit_once(':').map_or(rest, |(n, port)| if port.bytes().all(|b| b.is_ascii_digit()) { n } else { rest });
    (!name.is_empty() && name.parse::<std::net::IpAddr>().is_err()).then(|| name.to_ascii_lowercase())
}

/// 공개 주소의 조작 기기에 UV를 요구하는 승인 필요 동작(plan 관리 GUI §4와 W0 목록). 범위를 줄이는 동작(닫기·취소·제
/// 기기 폐기)과 읽기는 묻지 않는다
pub(crate) fn needs_uv(req: &FrontRequest, device: &str) -> bool {
    use FrontRequest as R;
    match req {
        R::DeviceRevoke { id } => id != device,
        R::WebPair { .. }
        | R::EnrollOpen { .. }
        | R::EnrollApprove { .. }
        | R::MachineRevoke { .. }
        | R::MachineSetupStart { .. }
        | R::MachineSetupTrust { .. }
        | R::WebPublish
        | R::WebPublishOff
        | R::LibraryRotate
        | R::LibraryUploadPublish { .. }
        | R::AutomationApprove { .. }
        | R::RoutineApprove { .. }
        | R::SafetySet { .. }
        | R::ProfileApply { .. }
        | R::WorkRootSet { .. }
        | R::HookSet { .. }
        | R::InstallApprove { .. }
        | R::RolloutStart { .. } => true,
        _ => false,
    }
}

fn sha256(b: &[u8]) -> [u8; 32] {
    Sha256::digest(b).into()
}

fn unb64(s: &str, what: &str) -> Result<Vec<u8>, ErrorBody> {
    B64.decode(s.trim_end_matches('='))
        .map_err(|e| aaa_common::diagnostic::context(e, bad(&format!("{what}가 base64url이 아니다"))))
}

/// clientDataJSON: 종류·challenge·origin이 맞아야 한다(다른 origin에 끼운 화면은 거부)
fn client_data(raw: &[u8], kind: &str, challenge: &[u8; 32], origin: &str) -> Result<(), ErrorBody> {
    let v: serde_json::Value = serde_json::from_slice(raw)
        .map_err(|e| aaa_common::diagnostic::context(e, bad("clientDataJSON을 읽지 못했다")))?;
    if v["type"] != kind {
        return Err(bad("확인 종류가 다르다"));
    }
    if v["challenge"].as_str() != Some(B64.encode(challenge).as_str()) {
        return Err(bad("challenge가 다르다"));
    }
    if !v["origin"].as_str().is_some_and(|o| o.eq_ignore_ascii_case(origin)) {
        return Err(bad("화면 주소가 다르다"));
    }
    if v["crossOrigin"] == true {
        return Err(bad("다른 주소에 끼운 화면이다"));
    }
    Ok(())
}

/// authenticatorData 앞머리: RP hash·사용자 있음(UP)·사용자 확인(UV). 서명 횟수를 돌려준다
fn auth_head(data: &[u8], rp_id: &str) -> Result<(u8, u32), ErrorBody> {
    if data.len() < 37 {
        return Err(bad("authenticatorData가 짧다"));
    }
    if data[..32] != sha256(rp_id.as_bytes()) {
        return Err(bad("passkey 이름(RP ID)이 다르다"));
    }
    let flags = data[32];
    if flags & FLAG_UP == 0 || flags & FLAG_UV == 0 {
        return Err(bad("사용자 확인(UV)을 하지 않았다"));
    }
    Ok((flags, u32::from_be_bytes(data[33..37].try_into().unwrap())))
}

/// COSE 공개키에서 알고리즘과 검증 키
pub(crate) fn cose_key(cose: &[u8]) -> Result<(i64, Vec<u8>), &'static str> {
    let (key, _) = cbor::decode(cose)?;
    let at = |k: i64| key.get(&if k < 0 { Cbor::Nint((-1 - k) as u64) } else { Cbor::Uint(k as u64) });
    let bytes = |k: i64| match at(k) {
        Some(Cbor::Bytes(b)) if b.len() == 32 => Ok(b.clone()),
        _ => Err("COSE 좌표가 32바이트가 아니다"),
    };
    let int = |k: i64| at(k).and_then(Cbor::int);
    match (int(1), int(3), int(-1)) {
        // EC2·P-256: 압축하지 않은 점(0x04 x y)
        (Some(2), Some(ALG_ES256), Some(1)) => Ok((ALG_ES256, [vec![4], bytes(-2)?, bytes(-3)?].concat())),
        // OKP·Ed25519
        (Some(1), Some(ALG_EDDSA), Some(6)) => Ok((ALG_EDDSA, bytes(-2)?)),
        _ => Err("받지 않는 passkey 알고리즘이다(ES256·EdDSA만)"),
    }
}

/// 등록 응답에서 (credential ID, COSE 공개키, 서명 횟수)
fn registration(
    reg: &PasskeyRegistration,
    challenge: &[u8; 32],
    origin: &str,
    rp_id: &str,
) -> Result<(Vec<u8>, Vec<u8>, u32), ErrorBody> {
    client_data(&unb64(&reg.client_data_json, "clientDataJSON")?, "webauthn.create", challenge, origin)?;
    let (att, _) = cbor::decode(&unb64(&reg.attestation_object, "attestationObject")?).map_err(bad)?;
    // attestation은 「none」만 청한다. 인증기 출처는 믿지 않으므로 형식(fmt·attStmt)은 보지 않는다
    let Some(Cbor::Bytes(data)) = att.get(&Cbor::Text("authData".into())) else {
        return Err(bad("authData가 없다"));
    };
    let (flags, count) = auth_head(data, rp_id)?;
    if flags & FLAG_AT == 0 || data.len() < 55 {
        return Err(bad("새 credential이 없다"));
    }
    let len = u16::from_be_bytes([data[53], data[54]]) as usize;
    let id = data.get(55..55 + len).ok_or_else(|| bad("credential ID가 잘렸다"))?;
    if len == 0 || len > MAX_CREDENTIAL || id != unb64(&reg.credential_id, "credential ID")?.as_slice() {
        return Err(bad("credential ID가 다르다"));
    }
    let rest = &data[55 + len..];
    let (_, used) = cbor::decode(rest).map_err(bad)?;
    let cose = rest[..used].to_vec();
    cose_key(&cose).map_err(bad)?;
    Ok((id.to_vec(), cose, count))
}

/// 승인 서명: authenticatorData ‖ SHA-256(clientDataJSON)
fn verify(cose: &[u8], data: &[u8], client: &[u8], sig: &[u8]) -> Result<(), ErrorBody> {
    let (alg, key) = cose_key(cose).map_err(bad)?;
    let msg = [data, &sha256(client)].concat();
    let algorithm: &dyn signature::VerificationAlgorithm =
        if alg == ALG_ES256 { &signature::ECDSA_P256_SHA256_ASN1 } else { &signature::ED25519 };
    UnparsedPublicKey::new(algorithm, key)
        .verify(&msg, sig)
        .map_err(|e| aaa_common::diagnostic::context(e, bad("서명이 맞지 않는다")))
}

impl Host {
    fn rp_name(&self) -> String {
        aaa_config::paths::named("aaa", aaa_config::paths::instance().as_deref())
    }

    /// 짝지은 지 10분이 안 된 조작 기기면 그 끝
    fn register_until(&self, device: &str, role: DeviceRole) -> Option<u64> {
        if role != DeviceRole::Controller {
            return None;
        }
        let until = self.db.device(device).ok()??.created_ms + PASSKEY_REGISTER_WINDOW_MS;
        (until > util::now_ms()).then_some(until)
    }

    fn device_passkeys(&self, device: &str, rp_id: &str) -> Result<Vec<(String, Vec<u8>, u32)>, ErrorBody> {
        self.db.passkeys(device, rp_id).map_err(|e| err(ErrorCode::Internal, e.to_string()))
    }

    /// `/api/v1/me`의 passkey 상태. loopback 화면은 없다
    pub fn passkey_state(&self, device: &str, role: DeviceRole, origin: &str) -> Option<PasskeyState> {
        let rp_id = rp_of(origin)?;
        let registered = self.db.passkeys(device, &rp_id).is_ok_and(|k| !k.is_empty());
        Some(PasskeyState { register_until_ms: self.register_until(device, role), registered, rp_id })
    }

    /// 지금 공개 주소의 passkey가 있는 기기(기기 목록 표시)
    pub(crate) fn passkey_devices(&self) -> std::collections::HashSet<String> {
        let rp_id = self
            .public_origin()
            .and_then(|o| rp_of(&o))
            .or_else(|| crate::test_localhost_public().then(|| "localhost".to_string()));
        let Some(rp_id) = rp_id else { return Default::default() };
        self.db.passkey_devices(&rp_id).unwrap_or_default()
    }

    /// 웹 `call` 앞: 공개 주소의 조작 기기가 UV 없이 보낸 승인 필요 동작은 거부한다
    pub fn passkey_gate(
        &self,
        device: &str,
        role: DeviceRole,
        origin: &str,
        req: &FrontRequest,
    ) -> Result<(), ErrorBody> {
        let Some(rp_id) = rp_of(origin) else { return Ok(()) };
        if !role.can_control() || !needs_uv(req, device) {
            return Ok(());
        }
        if !self.device_passkeys(device, &rp_id)?.is_empty() {
            return Err(err(ErrorCode::PasskeyRequired, "이 동작은 이 기기 passkey의 사용자 확인이 필요하다"));
        }
        Err(err(
            ErrorCode::PasskeyRequired,
            if self.register_until(device, role).is_some() {
                "이 기기에 passkey가 없어 승인할 수 없다. 설정 → 폰 및 기기에서 먼저 passkey를 등록한다(짝지은 뒤 10분 안)"
            } else {
                "이 기기에 passkey가 없어 승인할 수 없다. host Mac 화면이나 passkey가 있는 기기에서 짝짓기 코드를 받아 이 브라우저를 \
                 다시 짝지은 뒤 10분 안에 passkey를 등록한다"
            },
        ))
    }

    /// 등록 시작: 공개 주소로 연 조작 기기가 짝지은 뒤 10분 안일 때만
    pub fn passkey_register_begin(
        &self,
        device: &str,
        role: DeviceRole,
        origin: &str,
    ) -> Result<PasskeyCreation, ErrorBody> {
        let rp_id = rp_of(origin)
            .ok_or_else(|| err(ErrorCode::BadRequest, "passkey는 공개 주소(HTTPS)로 연 화면에서 등록한다"))?;
        if role != DeviceRole::Controller {
            return Err(err(ErrorCode::Forbidden, "passkey는 조작 기기만 등록한다"));
        }
        self.register_until(device, role).ok_or_else(|| {
            err(
                ErrorCode::Forbidden,
                "passkey는 짝지은 뒤 10분 안에만 등록한다. host Mac 화면이나 passkey가 있는 기기에서 짝짓기 코드를 받아 다시 짝짓는다",
            )
        })?;
        let label = self.db.device(device).ok().flatten().map(|d| d.label).unwrap_or_default();
        let exclude = self.device_passkeys(device, &rp_id)?.into_iter().map(|(id, ..)| id).collect();
        let challenge = rand::random::<[u8; 32]>();
        let op_id = util::new_id();
        self.passkey_ops.put(
            &op_id,
            Op {
                device: device.into(),
                origin: origin.into(),
                challenge,
                expires_ms: util::now_ms() + OP_TTL_MS,
                kind: Kind::Register,
            },
        );
        Ok(PasskeyCreation {
            op_id,
            challenge: B64.encode(challenge),
            rp_id,
            rp_name: self.rp_name(),
            user_id: B64.encode(device.as_bytes()),
            user_name: format!("{} · {label}", self.rp_name()),
            exclude,
        })
    }

    /// 등록을 마친다. 이 기기의 같은 이름 passkey는 새것으로 바뀐다
    pub fn passkey_register(
        &self,
        device: &str,
        role: DeviceRole,
        origin: &str,
        reg: PasskeyRegistration,
    ) -> Result<(), ErrorBody> {
        let op = self.passkey_ops.take(&reg.op_id, device, origin)?;
        if !matches!(op.kind, Kind::Register) {
            return Err(stale());
        }
        let rp_id = rp_of(origin).ok_or_else(stale)?;
        if self.register_until(device, role).is_none() {
            return Err(err(ErrorCode::Forbidden, "passkey는 짝지은 뒤 10분 안에만 등록한다. 다시 짝짓는다"));
        }
        let (id, cose, count) = registration(&reg, &op.challenge, origin, &rp_id)?;
        let put = self
            .db
            .passkey_put(&B64.encode(id), device, &rp_id, &cose, count)
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        if !put {
            return Err(bad("다른 기기의 credential이다"));
        }
        self.db.audit(&format!("device:{device}"), "passkey_registered", &rp_id);
        self.bump();
        Ok(())
    }

    /// 승인 시작: 받은 요청을 두고 그 hash와 op ID만 묶은 challenge를 낸다
    pub fn passkey_approve_begin(
        &self,
        device: &str,
        role: DeviceRole,
        origin: &str,
        req: FrontRequest,
    ) -> Result<PasskeyChallenge, ErrorBody> {
        let rp_id = rp_of(origin).ok_or_else(|| err(ErrorCode::BadRequest, "이 화면은 passkey 없이 승인한다"))?;
        if !role.can_control() || !needs_uv(&req, device) {
            return Err(err(ErrorCode::BadRequest, "passkey로 승인할 동작이 아니다"));
        }
        let allow: Vec<String> = self.device_passkeys(device, &rp_id)?.into_iter().map(|(id, ..)| id).collect();
        if allow.is_empty() {
            // 등록 안내는 gate와 같은 글로
            self.passkey_gate(device, role, origin, &req)?;
        }
        let body = serde_json::to_vec(&req).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        let op_id = util::new_id();
        let challenge = sha256(&[op_id.as_bytes(), &sha256(&body)].concat());
        self.passkey_ops.put(
            &op_id,
            Op {
                device: device.into(),
                origin: origin.into(),
                challenge,
                expires_ms: util::now_ms() + OP_TTL_MS,
                kind: Kind::Approve(Box::new(req)),
            },
        );
        Ok(PasskeyChallenge { op_id, challenge: B64.encode(challenge), rp_id, allow })
    }

    /// 승인을 마친다: 서명이 맞으면 begin 때 받아 둔 요청을 이 기기로 한 번 실행한다
    pub async fn passkey_approve(
        self: &Arc<Self>,
        device: &str,
        role: DeviceRole,
        origin: &str,
        a: PasskeyAssertion,
    ) -> Result<FrontResponse, ErrorBody> {
        let op = self.passkey_ops.take(&a.op_id, device, origin)?;
        let Kind::Approve(req) = op.kind else { return Err(stale()) };
        let rp_id = rp_of(origin).ok_or_else(stale)?;
        let keys = self.device_passkeys(device, &rp_id)?;
        let Some((credential, cose, _)) = keys.iter().find(|(id, ..)| *id == a.credential_id.trim_end_matches('='))
        else {
            return Err(bad("이 기기의 passkey가 아니다"));
        };
        let client = unb64(&a.client_data_json, "clientDataJSON")?;
        let data = unb64(&a.authenticator_data, "authenticatorData")?;
        client_data(&client, "webauthn.get", &op.challenge, origin)?;
        let (_, count) = auth_head(&data, &rp_id)?;
        verify(cose, &data, &client, &unb64(&a.signature, "서명")?)?;
        let _ = self.db.passkey_used(credential, count, util::now_ms());
        let actor = format!("device:{device}");
        self.db.audit(&actor, "passkey_approved", req_op(&req));
        self.call(role, &actor, *req).await
    }
}

/// 기록에 남길 요청 이름(`op`): 본문은 쓰지 않는다
fn req_op(req: &FrontRequest) -> &'static str {
    use FrontRequest as R;
    match req {
        R::DeviceRevoke { .. } => "device_revoke",
        R::WebPair { .. } => "web_pair",
        R::EnrollOpen { .. } => "enroll_open",
        R::EnrollApprove { .. } => "enroll_approve",
        R::MachineRevoke { .. } => "machine_revoke",
        R::MachineSetupStart { .. } => "machine_setup_start",
        R::MachineSetupTrust { .. } => "machine_setup_trust",
        R::WebPublish => "web_publish",
        R::WebPublishOff => "web_publish_off",
        R::LibraryRotate => "library_rotate",
        R::LibraryUploadPublish { .. } => "library_upload_publish",
        R::AutomationApprove { .. } => "automation_approve",
        R::RoutineApprove { .. } => "routine_approve",
        R::SafetySet { .. } => "safety_set",
        R::ProfileApply { .. } => "profile_apply",
        R::WorkRootSet { .. } => "work_root_set",
        R::HookSet { .. } => "hook_set",
        R::InstallApprove { .. } => "install_approve",
        R::RolloutStart { .. } => "rollout_start",
        _ => "other",
    }
}
