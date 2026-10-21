//! 조작 기기 passkey(관리 GUI W7): 소프트웨어 인증기(ring ES256·Ed25519 키, 손으로 만든 CBOR)로 등록·승인을 끝까지 돌린다.
use std::sync::Arc;
use std::time::Instant;

use aaa_proto::{DeviceRole, ErrorCode, FrontRequest, FrontResponse, PasskeyAssertion, PasskeyRegistration};
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair};

use super::{ALG_EDDSA, ALG_ES256, B64, cbor, rp_of, sha256};
use crate::Host;

const PUBLIC: &str = "https://mac-a.example.ts.net:8443";
const RP: &str = "mac-a.example.ts.net";
const LOOPBACK: &str = "http://127.0.0.1:7431";

fn open(root: &std::path::Path) -> Arc<Host> {
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    Host::open(cfg, root.join("host")).unwrap()
}

fn pair(h: &Host, role: DeviceRole) -> String {
    let (code, _) = h.mint_pairing(role, Instant::now());
    let (token, _) = h.redeem_pairing(&code, "iPhone · Safari").unwrap();
    h.device_role(&token).unwrap().0
}

/// 소프트웨어 인증기: 키 하나와 credential ID
enum Key {
    Es(EcdsaKeyPair),
    Ed(Ed25519KeyPair),
}

struct Authenticator {
    key: Key,
    id: Vec<u8>,
}

impl Authenticator {
    fn new(alg: i64) -> Self {
        let rng = SystemRandom::new();
        let key = if alg == ALG_ES256 {
            let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
            Key::Es(EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).unwrap())
        } else {
            Key::Ed(Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&rng).unwrap().as_ref()).unwrap())
        };
        Authenticator { key, id: rand::random::<[u8; 16]>().to_vec() }
    }

    fn cose(&self) -> Vec<u8> {
        let bytes32 = |b: &[u8]| [&[0x58, 0x20][..], b].concat();
        match &self.key {
            // {1: 2, 3: -7, -1: 1, -2: x, -3: y}
            Key::Es(k) => {
                let p = k.public_key().as_ref();
                [
                    &[0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21][..],
                    &bytes32(&p[1..33]),
                    &[0x22],
                    &bytes32(&p[33..]),
                ]
                .concat()
            }
            // {1: 1, 3: -8, -1: 6, -2: x}
            Key::Ed(k) => {
                [&[0xa4, 0x01, 0x01, 0x03, 0x27, 0x20, 0x06, 0x21][..], &bytes32(k.public_key().as_ref())].concat()
            }
        }
    }

    fn sign(&self, msg: &[u8]) -> Vec<u8> {
        match &self.key {
            Key::Es(k) => k.sign(&SystemRandom::new(), msg).unwrap().as_ref().to_vec(),
            Key::Ed(k) => k.sign(msg).as_ref().to_vec(),
        }
    }

    fn create(&self, challenge: &str, origin: &str, rp: &str, flags: u8) -> PasskeyRegistration {
        let client = client("webauthn.create", challenge, origin);
        let id_len = (self.id.len() as u16).to_be_bytes();
        let data = [&sha256(rp.as_bytes())[..], &[flags], &[0; 4], &[0; 16], &id_len, &self.id, &self.cose()].concat();
        // {"fmt": "none", "attStmt": {}, "authData": data}
        let mut att = vec![0xa3, 0x63];
        att.extend(b"fmt");
        att.push(0x64);
        att.extend(b"none");
        att.push(0x67);
        att.extend(b"attStmt");
        att.extend([0xa0, 0x68]);
        att.extend(b"authData");
        att.extend([0x59]);
        att.extend((data.len() as u16).to_be_bytes());
        att.extend(&data);
        PasskeyRegistration {
            op_id: String::new(),
            credential_id: B64.encode(&self.id),
            client_data_json: B64.encode(client),
            attestation_object: B64.encode(att),
        }
    }

    fn get(&self, op_id: &str, challenge: &str, origin: &str, rp: &str, flags: u8, count: u32) -> PasskeyAssertion {
        let client = client("webauthn.get", challenge, origin);
        let data = [&sha256(rp.as_bytes())[..], &[flags], &count.to_be_bytes()].concat();
        let sig = self.sign(&[&data[..], &sha256(&client)].concat());
        PasskeyAssertion {
            op_id: op_id.into(),
            credential_id: B64.encode(&self.id),
            client_data_json: B64.encode(client),
            authenticator_data: B64.encode(data),
            signature: B64.encode(sig),
        }
    }
}

fn client(kind: &str, challenge: &str, origin: &str) -> Vec<u8> {
    serde_json::to_vec(
        &serde_json::json!({ "type": kind, "challenge": challenge, "origin": origin, "crossOrigin": false }),
    )
    .unwrap()
}

/// UP·UV·AT / UP·UV
const MADE: u8 = 0x45;
const USED: u8 = 0x05;

fn register(h: &Host, dev: &str, a: &Authenticator) {
    let c = h.passkey_register_begin(dev, DeviceRole::Controller, PUBLIC).unwrap();
    assert_eq!((c.rp_id.as_str(), c.user_id.as_str()), (RP, B64.encode(dev).as_str()));
    let mut reg = a.create(&c.challenge, PUBLIC, RP, MADE);
    reg.op_id = c.op_id;
    h.passkey_register(dev, DeviceRole::Controller, PUBLIC, reg).unwrap();
}

#[test]
fn a_public_controller_approves_only_with_its_passkey_and_the_bound_request_runs_once() {
    let root = std::env::temp_dir().join(format!("aaa-passkey-{}", aaa_common::util::new_id()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let h = open(&root);
        let dev = pair(&h, DeviceRole::Controller);
        let other = pair(&h, DeviceRole::Controller);
        let code = FrontRequest::WebPair { role: DeviceRole::Controller };
        let ctl = DeviceRole::Controller;
        // host Mac 화면(loopback)은 passkey가 없다: 지금처럼 바로 승인
        assert!(h.passkey_state(&dev, ctl, LOOPBACK).is_none());
        h.passkey_gate(&dev, ctl, LOOPBACK, &code).unwrap();
        // 공개 주소: passkey가 없으면 거부하고 등록을 안내한다(답 1A). 읽기와 제 기기 폐기는 묻지 않는다
        let state = h.passkey_state(&dev, ctl, PUBLIC).unwrap();
        assert!(!state.registered && state.register_until_ms.is_some() && state.rp_id == RP);
        let e = h.passkey_gate(&dev, ctl, PUBLIC, &code).unwrap_err();
        assert_eq!(e.code, ErrorCode::PasskeyRequired);
        assert!(e.message.contains("먼저 passkey를 등록"), "{e}");
        h.passkey_gate(&dev, ctl, PUBLIC, &FrontRequest::Devices).unwrap();
        h.passkey_gate(&dev, ctl, PUBLIC, &FrontRequest::DeviceRevoke { id: dev.clone() }).unwrap();
        assert!(h.passkey_gate(&dev, ctl, PUBLIC, &FrontRequest::DeviceRevoke { id: other.clone() }).is_err());

        for alg in [ALG_ES256, ALG_EDDSA] {
            let a = Authenticator::new(alg);
            register(&h, &dev, &a);
            assert!(h.passkey_state(&dev, ctl, PUBLIC).unwrap().registered);
            assert_eq!(h.db.passkeys(&dev, RP).unwrap().len(), 1, "다시 등록하면 그 기기 passkey를 바꾼다");
            let e = h.passkey_gate(&dev, ctl, PUBLIC, &code).unwrap_err();
            assert!(e.message.contains("사용자 확인이 필요"), "{e}");

            // challenge는 op ID와 host가 다시 계산한 요청 hash만 묶는다
            let c = h.passkey_approve_begin(&dev, ctl, PUBLIC, code.clone()).unwrap();
            assert_eq!(c.allow, vec![B64.encode(&a.id)]);
            let want = sha256(&[c.op_id.as_bytes(), &sha256(&serde_json::to_vec(&code).unwrap())].concat());
            assert_eq!(c.challenge, B64.encode(want));
            let ok = a.get(&c.op_id, &c.challenge, PUBLIC, RP, USED, 0);
            let r = h.passkey_approve(&dev, ctl, PUBLIC, ok.clone()).await.unwrap();
            assert!(matches!(r, FrontResponse::PairCode { role: DeviceRole::Controller, .. }), "{r:?}");
            // 같은 서명을 다시 보내도 실행하지 않는다
            let again = h.passkey_approve(&dev, ctl, PUBLIC, ok).await.unwrap_err();
            assert!(again.message.contains("시한이 지났다"), "{again}");
        }

        // 하나라도 어긋나면 실행하지 않고 그 op도 버린다
        let a = Authenticator::new(ALG_ES256);
        register(&h, &dev, &a);
        let stranger = Authenticator::new(ALG_ES256);
        type Bend<'a> = Box<dyn Fn(&str, &str) -> PasskeyAssertion + 'a>;
        let cases: Vec<(&str, Bend<'_>, &str)> = vec![
            ("UV 없음", Box::new(|op, ch| a.get(op, ch, PUBLIC, RP, 0x01, 0)), "사용자 확인(UV)"),
            ("다른 이름", Box::new(|op, ch| a.get(op, ch, "evil.example.ts.net", RP, USED, 0)), "화면 주소"),
            ("다른 RP", Box::new(|op, ch| a.get(op, ch, PUBLIC, "evil.example.ts.net", USED, 0)), "RP ID"),
            ("다른 challenge", Box::new(|op, _| a.get(op, &B64.encode([7; 32]), PUBLIC, RP, USED, 0)), "challenge"),
            (
                "남의 키 서명",
                Box::new(|op, ch| {
                    let mut x = stranger.get(op, ch, PUBLIC, RP, USED, 0);
                    x.credential_id = B64.encode(&a.id);
                    x
                }),
                "서명",
            ),
            ("모르는 credential", Box::new(|op, ch| stranger.get(op, ch, PUBLIC, RP, USED, 0)), "passkey가 아니다"),
        ];
        for (what, bend, want) in cases {
            let c = h.passkey_approve_begin(&dev, ctl, PUBLIC, code.clone()).unwrap();
            let e = h.passkey_approve(&dev, ctl, PUBLIC, bend(&c.op_id, &c.challenge)).await.unwrap_err();
            assert!(e.message.contains(want), "{what}: {e}");
            let fresh = a.get(&c.op_id, &c.challenge, PUBLIC, RP, USED, 0);
            assert!(h.passkey_approve(&dev, ctl, PUBLIC, fresh).await.is_err(), "{what}: 실패한 op를 다시 썼다");
        }
        // op는 begin한 기기·화면만 마친다
        let c = h.passkey_approve_begin(&dev, ctl, PUBLIC, code.clone()).unwrap();
        let mine = a.get(&c.op_id, &c.challenge, PUBLIC, RP, USED, 0);
        assert!(h.passkey_approve(&other, ctl, PUBLIC, mine).await.is_err());
        // UV를 묻지 않는 동작은 passkey 승인으로 보내지 않는다
        let e = h.passkey_approve_begin(&dev, ctl, PUBLIC, FrontRequest::Devices).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest);
        assert!(h.passkey_approve_begin(&dev, ctl, LOOPBACK, code.clone()).is_err());
        // 기기를 폐기하면 passkey도 지운다
        let c = h.passkey_approve_begin(&dev, ctl, PUBLIC, FrontRequest::DeviceRevoke { id: other.clone() }).unwrap();
        let ok = a.get(&c.op_id, &c.challenge, PUBLIC, RP, USED, 3);
        assert!(matches!(h.passkey_approve(&dev, ctl, PUBLIC, ok).await.unwrap(), FrontResponse::Done));
        assert!(h.db.device(&other).unwrap().is_none());
        assert!(h.passkey_register_begin(&dev, ctl, PUBLIC).is_ok());
        assert!(h.revoke_device(&dev).unwrap());
        assert!(h.db.passkeys(&dev, RP).unwrap().is_empty());
    });
}

#[test]
fn a_passkey_is_registered_only_within_ten_minutes_of_pairing_on_the_public_address() {
    let root = std::env::temp_dir().join(format!("aaa-passkey-{}", aaa_common::util::new_id()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let _rt = rt.enter();
    let h = open(&root);
    let ctl = DeviceRole::Controller;
    let dev = pair(&h, ctl);
    let a = Authenticator::new(ALG_ES256);
    // loopback·보기 기기는 등록하지 않는다
    assert_eq!(h.passkey_register_begin(&dev, ctl, LOOPBACK).unwrap_err().code, ErrorCode::BadRequest);
    let reader = pair(&h, DeviceRole::Reader);
    assert_eq!(h.passkey_register_begin(&reader, DeviceRole::Reader, PUBLIC).unwrap_err().code, ErrorCode::Forbidden);
    // 인증기가 UV를 하지 않았거나 다른 credential을 보냈으면 등록하지 않는다
    for (flags, swap) in [(0x41, false), (MADE, true)] {
        let c = h.passkey_register_begin(&dev, ctl, PUBLIC).unwrap();
        let mut reg = a.create(&c.challenge, PUBLIC, RP, flags);
        reg.op_id = c.op_id;
        if swap {
            reg.credential_id = B64.encode([9; 16]);
        }
        assert!(h.passkey_register(&dev, ctl, PUBLIC, reg).is_err());
    }
    // 시작한 뒤 시한이 지나면 마치지 못한다
    let c = h.passkey_register_begin(&dev, ctl, PUBLIC).unwrap();
    let late = aaa_common::util::now_ms() - aaa_proto::PASSKEY_REGISTER_WINDOW_MS - 1;
    h.db.set_device_created(&dev, late);
    let mut reg = a.create(&c.challenge, PUBLIC, RP, MADE);
    reg.op_id = c.op_id;
    assert_eq!(h.passkey_register(&dev, ctl, PUBLIC, reg).unwrap_err().code, ErrorCode::Forbidden);
    // 10분이 지난 기기(W7 전에 짝지은 기기 포함)는 다시 짝짓는다(답 2A)
    let e = h.passkey_register_begin(&dev, ctl, PUBLIC).unwrap_err();
    assert!(e.message.contains("다시 짝짓는다"), "{e}");
    assert!(h.passkey_state(&dev, ctl, PUBLIC).unwrap().register_until_ms.is_none());
    let gate = h.passkey_gate(&dev, ctl, PUBLIC, &FrontRequest::WebPublishOff).unwrap_err();
    assert!(gate.message.contains("다시 짝지은 뒤 10분 안에"), "{gate}");
    assert!(h.db.passkeys(&dev, RP).unwrap().is_empty());
    // 다른 기기가 쓰는 credential은 가져오지 못한다
    let fresh = pair(&h, ctl);
    register(&h, &fresh, &a);
    assert!(!h.db.passkey_put(&B64.encode(&a.id), &dev, RP, &a.cose(), 0).unwrap());
}

#[test]
fn the_rp_id_is_the_public_domain_and_cbor_reads_only_bounded_definite_values() {
    assert_eq!(rp_of(PUBLIC).as_deref(), Some(RP));
    assert_eq!(rp_of("https://mac-a.example.ts.net").as_deref(), Some(RP));
    assert_eq!(rp_of("http://localhost:7431").as_deref(), Some("localhost"));
    assert_eq!(rp_of(LOOPBACK), None);
    assert_eq!(rp_of(""), None);
    // 정해진 길이만: 길이 없는 배열·잘린 바이트·남은 크기보다 큰 길이·너무 깊은 중첩은 거부
    for bad in [&[0x9f, 0x01, 0xff][..], &[0x58, 0x20, 0x00], &[0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]] {
        assert!(cbor::decode(bad).is_err(), "{bad:x?}");
    }
    assert!(cbor::decode(&[0x81; 40]).is_err());
    let (v, used) = cbor::decode(&[0xa1, 0x20, 0x43, 1, 2, 3, 0xff]).unwrap();
    assert_eq!(used, 6, "뒤 바이트는 읽지 않는다");
    assert_eq!(v.get(&cbor::Cbor::Nint(0)), Some(&cbor::Cbor::Bytes(vec![1, 2, 3])));
}
