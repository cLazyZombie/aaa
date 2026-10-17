//! Web Push(M3, §8.26). 사람이 볼 inbox 항목(질문·보고 준비 등)을 「알림 켜기」한 브라우저에 알림으로 보낸다.
//!
//! - VAPID(RFC 8292, ES256) 키는 host 상태 폴더 `push/vapid.p8`(0600)에 한 번 만든다. 공개키는 웹이 구독할 때 쓴다.
//! - 본문은 RFC 8291(aes128gcm)로 기기 키에 맞춰 암호화한다. 중간의 push 서비스(Google·Apple·Mozilla·Microsoft)는 읽지 못한다.
//! - 구독 주소(endpoint)는 기기가 준다. 허용한 push 서비스 호스트의 https만 받는다(host가 아무 주소로나 요청하지 않게).
//!   시험용 loopback http는 `push_hosts`에 그 이름을 적었을 때만 받는다.
//! - endpoint 경로는 그 기기에 보내는 열쇠다. 로그·흐름 사건·오류에 넣지 않는다.
//! - 404·410이면 브라우저가 구독을 버린 것이라 지운다. 다른 실패는 다시 보내지 않는다(webhook과 같다).
//! - 웹 화면을 지금 쓰고 있으면(어느 기기든 보이고 focus가 있으면, events WS `presence`) 어느 기기에도 보내지 않는다
//!   (herdr-web-ui push.ts `inUse`). 보낼 때 묻는다. 시험 알림과 webhook(텔레그램)은 이와 상관없다.
//! - 알림 한 자리는 thread 하나다(새 것이 옛 것을 바꾼다, herdr-web-ui `paneNotificationTag`). 여러 thread·머신 항목이
//!   섞이면 프로젝트 한 자리다.

use std::path::Path;
use std::time::Duration;

use aaa_proto::InboxItem;
use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair as _};

use crate::Host;
use crate::db::PushSub;

const SEND_TIMEOUT: Duration = Duration::from_secs(10);
/// push 서비스가 기기에 못 전하면 이만큼(초) 들고 있다가 버린다
const TTL_SECS: u32 = 24 * 3600;
/// 암호문 한 통 상한(4096바이트)에 머리(86)·tag(16)·구분(1)을 빼고도 남게 줄인다
const BODY_CAP: usize = 2400;

pub(crate) struct Vapid {
    key: EcdsaKeyPair,
    /// uncompressed P-256 공개키 65바이트
    public: Vec<u8>,
}

impl Vapid {
    /// `<state>/push/vapid.p8`을 읽거나 처음이면 만든다
    pub(crate) fn load_or_create(state: &Path) -> Result<Vapid> {
        let dir = state.join("push");
        let path = dir.join("vapid.p8");
        let rng = SystemRandom::new();
        let pkcs8 = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                aaa_common::util::ensure_private_dir(&dir)?;
                let doc = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
                    .map_err(|_| anyhow!("VAPID 키 생성 실패"))?;
                aaa_common::util::write_private(&path, doc.as_ref())?;
                doc.as_ref().to_vec()
            }
            Err(e) => return Err(e).context("VAPID 키 읽기"),
        };
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &pkcs8, &rng)
            .map_err(|e| anyhow!("VAPID 키가 깨졌다({}): {e}", path.display()))?;
        let public = key.public_key().as_ref().to_vec();
        Ok(Vapid { key, public })
    }

    /// 웹이 `applicationServerKey`로 쓰는 공개키(base64url)
    pub(crate) fn public_b64(&self) -> String {
        B64URL.encode(&self.public)
    }

    /// `Authorization: vapid t=<JWT>, k=<공개키>`(RFC 8292). aud는 endpoint의 origin이다.
    fn authorization(&self, audience: &str, subject: &str, now_s: u64) -> Result<String> {
        let header = B64URL.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = serde_json::json!({ "aud": audience, "exp": now_s + 12 * 3600, "sub": subject });
        let input = format!("{header}.{}", B64URL.encode(claims.to_string()));
        let sig = self.key.sign(&SystemRandom::new(), input.as_bytes()).map_err(|_| anyhow!("VAPID 서명 실패"))?;
        Ok(format!("vapid t={input}.{}, k={}", B64URL.encode(sig.as_ref()), self.public_b64()))
    }
}

/// 브라우저가 주는 base64url(가끔 padding이 붙는다)
fn b64url(s: &str) -> Result<Vec<u8>> {
    B64URL.decode(s.trim_end_matches('=')).context("base64url 형식이 아니다")
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

/// 구독 주소 검사. 통과하면 JWT aud로 쓸 origin을 돌려준다. 오류 글에 주소를 넣지 않는다.
pub(crate) fn check_endpoint(url: &str, hosts: &[String]) -> Result<String, String> {
    let (tls, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err("https 주소가 아니다".into());
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') || authority.is_empty() || url.len() > 2048 {
        return Err("구독 주소 형식이 아니다".into());
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(""),
        None => authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority),
    }
    .to_ascii_lowercase();
    let listed = hosts.iter().any(|h| {
        let h = h.trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();
        host == h || host.ends_with(&format!(".{h}"))
    });
    if !listed {
        return Err("허용하지 않은 push 서비스다(notify.push_hosts)".into());
    }
    if !tls && !is_loopback(&host) {
        return Err("https 주소가 아니다".into());
    }
    Ok(format!("{}://{authority}", if tls { "https" } else { "http" }))
}

/// HKDF-SHA256 출력 길이
struct Len(usize);

impl ring::hkdf::KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Result<Vec<u8>> {
    let prk = ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA256, salt).extract(ikm);
    let mut out = vec![0u8; len];
    prk.expand(&[info], Len(len)).and_then(|o| o.fill(&mut out)).map_err(|_| anyhow!("HKDF 실패"))?;
    Ok(out)
}

/// RFC 8291 키 유도: ECDH 비밀·기기 auth·양쪽 공개키·salt → (CEK 16바이트, NONCE 12바이트)
fn derive(ecdh: &[u8], auth: &[u8], ua_pub: &[u8], as_pub: &[u8], salt: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_pub);
    key_info.extend_from_slice(as_pub);
    let ikm = hkdf(auth, ecdh, &key_info, 32)?;
    let cek = hkdf(salt, &ikm, b"Content-Encoding: aes128gcm\0", 16)?;
    let nonce = hkdf(salt, &ikm, b"Content-Encoding: nonce\0", 12)?;
    Ok((cek, nonce))
}

/// 한 레코드짜리 aes128gcm 본문(RFC 8188): salt(16) · rs(4) · idlen(1) · keyid(as 공개키 65) · 암호문
fn seal(cek: &[u8], nonce: &[u8], salt: &[u8], as_pub: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
    let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, cek).map_err(|_| anyhow!("CEK"))?);
    let mut data = payload.to_vec();
    data.push(2); // 마지막 레코드 구분, padding 없음
    let nonce = Nonce::try_assume_unique_for_key(nonce).map_err(|_| anyhow!("NONCE"))?;
    key.seal_in_place_append_tag(nonce, Aad::empty(), &mut data).map_err(|_| anyhow!("암호화 실패"))?;
    let mut out = Vec::with_capacity(86 + data.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&4096u32.to_be_bytes());
    out.push(as_pub.len() as u8);
    out.extend_from_slice(as_pub);
    out.extend_from_slice(&data);
    Ok(out)
}

/// 기기 공개키(p256dh)·auth로 payload를 암호화한다. 보낼 때마다 새 임시 키와 salt를 쓴다.
pub(crate) fn encrypt(payload: &[u8], p256dh: &str, auth: &str) -> Result<Vec<u8>> {
    use ring::agreement::{ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey, agree_ephemeral};
    let (ua_pub, auth) = (b64url(p256dh)?, b64url(auth)?);
    if ua_pub.len() != 65 || auth.len() != 16 {
        bail!("기기 키 길이가 맞지 않다");
    }
    let rng = SystemRandom::new();
    let mine = EphemeralPrivateKey::generate(&ECDH_P256, &rng).map_err(|_| anyhow!("임시 키 생성 실패"))?;
    let as_pub = mine.compute_public_key().map_err(|_| anyhow!("임시 공개키"))?.as_ref().to_vec();
    let ecdh = agree_ephemeral(mine, &UnparsedPublicKey::new(&ECDH_P256, &ua_pub), |k| k.to_vec())
        .map_err(|_| anyhow!("기기 공개키가 P-256 점이 아니다"))?;
    let mut salt = [0u8; 16];
    rng.fill(&mut salt).map_err(|_| anyhow!("난수"))?;
    let (cek, nonce) = derive(&ecdh, &auth, &ua_pub, &as_pub, &salt)?;
    seal(&cek, &nonce, &salt, &as_pub, payload)
}

/// 구독 등록 요청이 맞는지 본다(키를 실제로 풀어 본다)
pub(crate) fn check_keys(p256dh: &str, auth: &str) -> Result<(), String> {
    let ua = b64url(p256dh).map_err(|e| e.to_string())?;
    let au = b64url(auth).map_err(|e| e.to_string())?;
    if ua.len() != 65 || ua[0] != 4 || au.len() != 16 {
        return Err("기기 키 형식이 아니다(p256dh 65바이트, auth 16바이트)".into());
    }
    Ok(())
}

/// 알림 내용. 한 thread 항목만이면 제목은 「프로젝트 · thread 제목」(`title`이 비면 thread ID), 자리(tag)는 그 thread,
/// 누르면 그 thread 보기로 연다. 아니면 「aaa · 프로젝트」와 프로젝트 자리다.
pub(crate) fn payload(project: &str, items: &[InboxItem], title: Option<&str>) -> serde_json::Value {
    let one = items
        .first()
        .map(|i| i.subject.as_str())
        .filter(|s| items.iter().all(|i| i.subject == *s && i.item_kind == "thread-state"));
    let (url, head, tag) = match one {
        Some(t) => (
            format!("/#thread={}", url_escape(&format!("{project}/{t}"))),
            format!("{project} · {}", title.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(t)),
            format!("aaa-{project}/{t}"),
        ),
        None => ("/".into(), format!("aaa · {project}"), format!("aaa-{project}")),
    };
    let mut body = String::new();
    for i in items {
        let summary: String = i.summary.chars().take(120).collect();
        let line = format!("{} {}: {summary}", crate::inbox::who(i), i.event);
        if body.len() + line.len() + 1 > BODY_CAP {
            body.push('…');
            break;
        }
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(&line);
    }
    serde_json::json!({ "title": head, "body": body, "url": url, "tag": tag })
}

/// encodeURIComponent와 같은 규칙(웹이 decodeURIComponent로 읽는다)
fn url_escape(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 한 구독에 보낸 결과
pub(crate) enum Sent {
    Ok,
    /// 브라우저가 구독을 버렸다(404·410): 지웠다
    Gone,
    Failed(String),
}

/// VAPID `sub` 기본값(설정 `notify.push_subject`가 비었을 때)
const DEFAULT_PUSH_SUBJECT: &str = "https://github.com/cLazyZombie/aaa";

impl Host {
    fn push_subject(&self) -> String {
        let s = &self.cfg.notify.push_subject;
        if !s.is_empty() {
            return s.clone();
        }
        // 공개 주소(tailnet 이름)는 push 사업자에게 보내지 않는다(보안 감사 t-0340 B11). 참고 herdr-web-ui server/push.ts
        // `DEFAULT_SUBJECT`처럼 프로젝트 저장소 주소를 쓴다
        DEFAULT_PUSH_SUBJECT.into()
    }

    /// 구독 하나에 보낸다
    pub(crate) async fn push_one(&self, sub: &PushSub, payload: &[u8]) -> Sent {
        let Some(vapid) = &self.vapid else { return Sent::Failed("Web Push가 꺼져 있다".into()) };
        let res: Result<u16, String> = async {
            let aud = check_endpoint(&sub.endpoint, &self.cfg.notify.push_hosts)?;
            let body = encrypt(payload, &sub.p256dh, &sub.auth).map_err(|e| e.to_string())?;
            let now = aaa_common::util::now_ms() / 1000;
            let auth = vapid.authorization(&aud, &self.push_subject(), now).map_err(|e| e.to_string())?;
            let headers = [
                ("Authorization", auth),
                ("Content-Encoding", "aes128gcm".to_string()),
                ("Content-Type", "application/octet-stream".to_string()),
                ("TTL", TTL_SECS.to_string()),
                ("Urgency", "high".to_string()),
            ];
            match tokio::time::timeout(SEND_TIMEOUT, crate::notify::post_raw(&sub.endpoint, &headers, &body)).await {
                Ok(r) => r,
                Err(_) => Err(format!("{}초 안에 끝나지 않았다", SEND_TIMEOUT.as_secs())),
            }
        }
        .await;
        match res {
            Ok(code) if (200..300).contains(&code) => Sent::Ok,
            Ok(404 | 410) => {
                let _ = self.db.push_del(&sub.endpoint, None);
                Sent::Gone
            }
            Ok(code) => Sent::Failed(format!("HTTP {code}")),
            Err(e) => Sent::Failed(e),
        }
    }

    /// 구독한 모든 기기에 보낸다. 흐름 사건에는 개수와 실패 까닭만 남긴다.
    pub(crate) async fn push_items(&self, project: &str, items: &[InboxItem]) {
        if self.vapid.is_none() {
            return;
        }
        // 쓰는 기기(마지막으로 누르거나 치거나 화면을 켠 기기)가 지금 보이고 focus면 그 화면이 이미 바뀐 것을 보인다:
        // 어느 기기에도 보내지 않는다. 다른 기기 창이 떠 있는 것은 까닭이 아니다(사용자 결정 10-18, plan §8.105.
        // 참고 herdr-web-ui server/index.ts 1117줄 `inUse: () => present.size > 0`는 쓰는 화면이 하나라도 있으면 건너뛴다).
        // 모으는 기다림(debounce) 뒤 보내기 직전에 본다
        let skip = {
            let v = self.viewing.lock().unwrap();
            v.user_in_use().then(|| (v.user().unwrap_or_default(), v.devices_in_use()))
        };
        if let Some((user, devices)) = skip {
            let watching = self.in_use.load(std::sync::atomic::Ordering::SeqCst);
            self.flow_project(
                project,
                "push_skipped",
                None,
                format!(
                    "쓰는 기기 {user} · 쓰는 화면 {watching}개(기기 {}) · 항목 {}개",
                    devices.join(", "),
                    items.len()
                ),
            );
            return;
        }
        let subs = match self.db.push_list(None) {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => return,
            Err(e) => {
                tracing::error!("push 구독 읽기 실패: {e:#}");
                return;
            }
        };
        let (mut ok, mut gone, mut failed, mut chose_not) = (0, 0, Vec::new(), 0);
        for s in &subs {
            // 이 기기가 고른 종류만(차이 목록 N3, 참고 notify-policy.ts `alertsAllow`)
            let mine = for_device(s.alerts.unwrap_or_default(), items);
            if mine.is_empty() {
                chose_not += 1;
                continue;
            }
            // 한 thread 항목이면 알림 제목에 그 thread 제목을 쓴다
            let title = mine
                .first()
                .filter(|f| mine.iter().all(|i| i.subject == f.subject))
                .and_then(|f| self.load(project, &f.subject).ok())
                .map(|r| r.title);
            let body = payload(project, &mine, title.as_deref()).to_string();
            match self.push_one(s, body.as_bytes()).await {
                Sent::Ok => ok += 1,
                Sent::Gone => gone += 1,
                Sent::Failed(e) => failed.push(e),
            }
        }
        if ok > 0 {
            self.flow_project(project, "push_sent", None, format!("기기 {ok}개 · 항목 {}개", items.len()));
        }
        if chose_not > 0 {
            self.flow_project(
                project,
                "push_unwanted",
                None,
                format!("기기 {chose_not}개가 고른 알림 종류가 아니다 · 항목 {}개", items.len()),
            );
        }
        if gone > 0 {
            self.flow_project(project, "push_gone", None, format!("버려진 구독 {gone}개를 지웠다"));
        }
        if !failed.is_empty() {
            tracing::warn!(%project, "Web Push 실패 {}개(다시 보내지 않는다): {}", failed.len(), failed[0]);
            self.flow_project(project, "push_failed", None, failed.join("; "));
        }
    }
}

/// 기기가 고른 알림 종류(N3)에 드는 항목. thread 항목만 가른다: 새 보고는 「작업 완료」, 나머지(질문·확인창·기다림·시작 실패)는
/// 「입력 필요」. PR·머신·routine 같은 다른 종류는 host 설정(`notify.kinds`)대로 모든 기기에 간다(참고에 없는 aaa 항목)
pub(crate) fn for_device(a: crate::db::Alerts, items: &[InboxItem]) -> Vec<InboxItem> {
    items
        .iter()
        .filter(|i| i.item_kind != "thread-state" || if i.event == "new report" { a.done } else { a.input })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_alert_kinds() {
        let it = |kind: &str, event: &str| InboxItem {
            id: event.into(),
            project: "demo".into(),
            item_kind: kind.into(),
            subject: "t-0001".into(),
            event: event.into(),
            summary: String::new(),
            body: String::new(),
            done: false,
            seen: false,
            created_at_ms: 0,
            done_at_ms: None,
        };
        let items =
            [it("thread-state", "new report"), it("thread-state", "blocked on a prompt"), it("pr", "PR merged")];
        let ids = |a| for_device(a, &items).into_iter().map(|i| i.id).collect::<Vec<_>>();
        use crate::db::Alerts;
        assert_eq!(ids(Alerts::default()).len(), 3);
        assert_eq!(ids(Alerts { input: false, done: true }), ["new report", "PR merged"]);
        assert_eq!(ids(Alerts { input: true, done: false }), ["blocked on a prompt", "PR merged"]);
        assert_eq!(ids(Alerts { input: false, done: false }), ["PR merged"]);
    }

    fn hosts() -> Vec<String> {
        aaa_config::NotifySection::default().push_hosts
    }

    #[test]
    fn endpoints() {
        let h = hosts();
        assert_eq!(
            check_endpoint("https://fcm.googleapis.com/fcm/send/abc", &h).unwrap(),
            "https://fcm.googleapis.com"
        );
        assert_eq!(check_endpoint("https://web.push.apple.com/QGx", &h).unwrap(), "https://web.push.apple.com");
        assert_eq!(
            check_endpoint("https://wns2-by3p.notify.windows.com:443/w/?token=x", &h).unwrap(),
            "https://wns2-by3p.notify.windows.com:443"
        );
        for bad in [
            "http://fcm.googleapis.com/x",
            "https://evil.example/fcm.googleapis.com",
            "https://fcm.googleapis.com.evil.example/x",
            "https://evilfcm.googleapis.com@127.0.0.1/x",
            "https://notfcm.googleapis.comx/",
            "ftp://fcm.googleapis.com/x",
            "http://127.0.0.1:9/x",
        ] {
            assert!(check_endpoint(bad, &h).is_err(), "{bad}");
        }
        // loopback http는 목록에 적었을 때만
        let mut t = h.clone();
        t.push("127.0.0.1".into());
        assert_eq!(check_endpoint("http://127.0.0.1:9/p/1", &t).unwrap(), "http://127.0.0.1:9");
        assert!(check_endpoint("http://localhost:9/p/1", &t).is_err());
    }

    /// 기기 쪽 복호화(RFC 8291 받는 쪽)로 왕복한다. 독립 구현 확인은 웹 E2E(Node crypto)가 한다.
    #[test]
    fn roundtrip() {
        use ring::agreement::{ECDH_P256, EphemeralPrivateKey, UnparsedPublicKey, agree_ephemeral};
        let rng = SystemRandom::new();
        let ua = EphemeralPrivateKey::generate(&ECDH_P256, &rng).unwrap();
        let ua_pub = ua.compute_public_key().unwrap().as_ref().to_vec();
        let auth = [7u8; 16];
        let msg = r#"{"title":"aaa · demo","body":"t-0001 질문"}"#.as_bytes();
        let body = encrypt(msg, &B64URL.encode(&ua_pub), &B64URL.encode(auth)).unwrap();
        let (salt, rs, idlen) = (&body[..16], u32::from_be_bytes(body[16..20].try_into().unwrap()), body[20]);
        assert_eq!((rs, idlen), (4096, 65));
        let as_pub = &body[21..86];
        let ecdh = agree_ephemeral(ua, &UnparsedPublicKey::new(&ECDH_P256, as_pub), |k| k.to_vec()).unwrap();
        let (cek, nonce) = derive(&ecdh, &auth, &ua_pub, as_pub, salt).unwrap();
        use ring::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
        let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &cek).unwrap());
        let mut ct = body[86..].to_vec();
        let plain = key
            .open_in_place(Nonce::try_assume_unique_for_key(&nonce).unwrap(), Aad::empty(), &mut ct)
            .unwrap()
            .to_vec();
        assert_eq!(plain.last(), Some(&2));
        assert_eq!(&plain[..plain.len() - 1], msg);
        assert!(check_keys(&B64URL.encode(&ua_pub), &B64URL.encode(auth)).is_ok());
        assert!(check_keys(&B64URL.encode(&ua_pub[1..]), &B64URL.encode(auth)).is_err());
    }

    #[test]
    fn vapid_jwt() {
        let dir = std::env::temp_dir().join(format!("aaa-vapid-{}", std::process::id()));
        let v = Vapid::load_or_create(&dir).unwrap();
        // 다시 읽어도 같은 키다
        assert_eq!(Vapid::load_or_create(&dir).unwrap().public, v.public);
        let a = v.authorization("https://fcm.googleapis.com", "mailto:aaa@example.com", 1_000).unwrap();
        let (t, k) = a.strip_prefix("vapid t=").unwrap().split_once(", k=").unwrap();
        assert_eq!(k, v.public_b64());
        let parts: Vec<_> = t.split('.').collect();
        let claims: serde_json::Value = serde_json::from_slice(&b64url(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://fcm.googleapis.com");
        assert_eq!(claims["exp"], 1_000 + 12 * 3600);
        let pk = ring::signature::UnparsedPublicKey::new(&ring::signature::ECDSA_P256_SHA256_FIXED, &v.public);
        pk.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &b64url(parts[2]).unwrap()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(dir.join("push/vapid.p8")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn payloads() {
        let item = |subject: &str, kind: &str| InboxItem {
            id: "x".into(),
            project: "demo".into(),
            item_kind: kind.into(),
            subject: subject.into(),
            event: "blocked on a prompt".into(),
            summary: "확인창".into(),
            body: String::new(),
            done: false,
            seen: false,
            created_at_ms: 0,
            done_at_ms: None,
        };
        // 한 thread: 제목은 「프로젝트 · thread 제목」, 자리는 그 thread
        let p = payload("demo", &[item("t-0001", "thread-state"), item("t-0001", "thread-state")], Some("웹 결과물"));
        assert_eq!(p["url"], "/#thread=demo%2Ft-0001");
        assert_eq!((p["title"].as_str(), p["tag"].as_str()), (Some("demo · 웹 결과물"), Some("aaa-demo/t-0001")));
        // 제목이 없거나 비면 thread ID
        assert_eq!(payload("demo", &[item("t-0001", "thread-state")], Some("  "))["title"], "demo · t-0001");
        assert_eq!(payload("demo", &[item("t-0001", "thread-state")], None)["title"], "demo · t-0001");
        // 머신 항목(세션 끝남)은 thread 자리가 아니다
        assert_eq!(payload("demo", &[item("mac-b", "session")], None)["title"], "aaa · demo");
        // 섞이면 프로젝트 한 자리
        let p = payload("demo", &[item("t-0001", "thread-state"), item("mac-b", "outage")], Some("웹 결과물"));
        assert_eq!(
            (p["url"].as_str(), p["title"].as_str(), p["tag"].as_str()),
            (Some("/"), Some("aaa · demo"), Some("aaa-demo"))
        );
        let p = payload("demo", &[item("t-0001", "thread-state"), item("t-0002", "thread-state")], None);
        assert_eq!((p["title"].as_str(), p["tag"].as_str()), (Some("aaa · demo"), Some("aaa-demo")));
        let many: Vec<_> = (0..200).map(|n| item(&format!("t-{n:04}"), "thread-state")).collect();
        assert!(payload("demo", &many, None)["body"].as_str().unwrap().len() <= BODY_CAP + 3);
    }
}
