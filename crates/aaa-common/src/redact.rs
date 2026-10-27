//! 등록한 인증의 조각이 기록·화면으로 나가지 않게 하는 프로세스 수명 가림.
//! 인증 전송·금고에는 적용하지 않고, 일반 입출력 경계에서 쓴다.
use base64::Engine;
use std::collections::HashSet;
use std::io::Write;
use std::sync::{LazyLock, RwLock};

struct Rules {
    needles: HashSet<[u8; 8]>,
    starts: [u64; 1024],
    originals: HashSet<String>,
    prefixes: HashSet<Vec<u8>>,
}
impl Default for Rules {
    fn default() -> Self {
        Self { needles: HashSet::new(), starts: [0; 1024], originals: HashSet::new(), prefixes: HashSet::new() }
    }
}
impl Rules {
    fn contains(&self, window: &[u8]) -> bool {
        // 두 바이트 후보가 없으면 8바이트 해시를 계산하지 않는다. 후보여도 원래 집합으로 정확히 확인한다.
        let start = u16::from_le_bytes([window[0], window[1]]) as usize;
        self.starts[start / 64] & (1 << (start % 64)) != 0
            && self.needles.contains(<&[u8; 8]>::try_from(window).unwrap())
    }
}
static RULES: LazyLock<RwLock<Rules>> = LazyLock::new(Default::default);

/// 이 프로세스에 실제 가림 규칙이 있는지. 비밀 원문은 노출하지 않는다.
pub fn active() -> bool {
    !RULES.read().unwrap_or_else(|e| e.into_inner()).needles.is_empty()
}

pub fn remember(value: &str) {
    if value.len() < 12 {
        return;
    }
    let mut forms = vec![value.as_bytes().to_vec()];
    if let Ok(json) = serde_json::to_string(value) {
        forms.push(json.as_bytes()[1..json.len() - 1].to_vec());
    }
    let url: String = value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    forms.push(url.into_bytes());
    for engine in [&base64::engine::general_purpose::STANDARD, &base64::engine::general_purpose::URL_SAFE] {
        for pad in 0..3 {
            let mut bytes = vec![b'#'; pad];
            bytes.extend_from_slice(value.as_bytes());
            let encoded = engine.encode(bytes);
            if encoded.len() > 8 {
                forms.push(encoded.as_bytes()[4..encoded.len() - 4].to_vec());
            }
        }
    }
    let mut rules = RULES.write().unwrap_or_else(|e| e.into_inner());
    rules.originals.insert(value.to_string());
    for form in forms {
        for n in form.windows(8) {
            let n: [u8; 8] = n.try_into().unwrap();
            if rules.needles.insert(n) {
                let start = u16::from_le_bytes([n[0], n[1]]) as usize;
                rules.starts[start / 64] |= 1 << (start % 64);
                for i in 1..8 {
                    rules.prefixes.insert(n[..i].to_vec());
                }
            }
        }
    }
}

/// 인증한 session IPC로 agent 재접속 시 복원한다. 이 값은 기록하지 않는다.
pub fn snapshot() -> String {
    let rules = RULES.read().unwrap_or_else(|e| e.into_inner());
    // 조각 표 전체보다 작은 원문 집합을 SecretText 봉투로 인증 IPC에만 전달한다.
    serde_json::to_string(&rules.originals).expect("가림 규칙 직렬화")
}
pub fn restore(snapshot: &str) -> Result<(), serde_json::Error> {
    for value in serde_json::from_str::<Vec<String>>(snapshot)? {
        remember(&value);
    }
    Ok(())
}

/// 공식 인증 env는 기록에 쓰기 전에 기억한다. 실행 값 자체는 바꾸지 않는다.
pub fn remember_env(key: &str, value: &str) {
    if matches!(
        key,
        "CLAUDE_CODE_OAUTH_TOKEN" | "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN" | "OPENAI_API_KEY" | "CODEX_API_KEY"
    ) {
        remember(value);
    }
}

/// auth.json은 토큰 값만 등록한다. 키 이름·계정 id·시각을 가림 규칙으로 삼지 않는다.
pub fn remember_auth(harness: &str, value: &str) {
    if harness == "claude" {
        remember(value);
        return;
    }
    fn visit(v: &serde_json::Value) {
        match v {
            serde_json::Value::Object(o) => {
                for (k, v) in o {
                    if (k.to_ascii_lowercase().contains("token") || k.to_ascii_lowercase().ends_with("key"))
                        && let Some(s) = v.as_str()
                    {
                        remember(s);
                    } else {
                        visit(v)
                    }
                }
            }
            serde_json::Value::Array(a) => {
                for v in a {
                    visit(v)
                }
            }
            _ => {}
        }
    }
    if let Ok(v) = serde_json::from_str(value) {
        visit(&v);
    }
}

/// 끝이 비밀의 접두부이면 다음 chunk까지 잡는다. ANSI·개행을 넘는 조각도 가린다.
#[derive(Default)]
pub struct Stream {
    pending: Vec<u8>,
    masked: Vec<bool>,
}
impl Stream {
    pub fn push(&mut self, bytes: &[u8], end: bool) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        self.masked.resize(self.pending.len(), false);
        let rules = RULES.read().unwrap_or_else(|e| e.into_inner());
        if rules.needles.is_empty() {
            self.masked.clear();
            return std::mem::take(&mut self.pending);
        }
        // 원문과 가림 표시를 분리한다. 겹치는 8바이트 창의 끝을 다음 청크에서도 검사해야 한다.
        for (at, needle) in self.pending.windows(8).enumerate() {
            if rules.contains(needle) {
                self.masked[at..at + 8].fill(true);
            }
        }
        let mut visible = Vec::new();
        let mut incomplete = None;
        // 평문은 원문 창과 보이는 글의 창이 같다. 중복 검색과 바이트별 위치 배열을 만들지 않는다.
        // 이전 청크의 미완성 CSI가 남았다면 ESC가 포함돼 기존 경로로 이어진다.
        if self.pending.iter().any(|byte| matches!(byte, 27 | b'\r' | b'\n')) {
            let mut i = 0;
            while i < self.pending.len() {
                if self.pending[i] == 27 && self.pending.get(i + 1).is_none() {
                    incomplete = Some(i);
                    break;
                }
                if self.pending[i] == 27 && self.pending.get(i + 1) == Some(&b'[') {
                    let start = i;
                    i += 2;
                    while i < self.pending.len() && !(0x40..=0x7e).contains(&self.pending[i]) {
                        i += 1;
                    }
                    if i == self.pending.len() {
                        incomplete = Some(start);
                        break;
                    }
                    i += 1;
                    continue;
                }
                if !matches!(self.pending[i], b'\r' | b'\n') {
                    visible.push((i, self.pending[i]));
                }
                i += 1;
            }
            let text: Vec<u8> = visible.iter().map(|x| x.1).collect();
            for (at, n) in text.windows(8).enumerate() {
                if rules.contains(n) {
                    for (raw, _) in &visible[at..at + 8] {
                        self.masked[*raw] = true;
                    }
                }
            }
        }
        let mut cut = self.pending.len();
        if !end {
            for n in 1..=7.min(visible.len()) {
                let start = visible.len() - n;
                let suffix: Vec<u8> = visible[start..].iter().map(|(_, byte)| *byte).collect();
                if rules.prefixes.contains(&suffix) {
                    cut = cut.min(visible[start].0);
                }
            }
            for n in 1..=7.min(self.pending.len()) {
                if rules.prefixes.contains(&self.pending[self.pending.len() - n..]) {
                    cut = cut.min(self.pending.len() - n);
                }
            }
            if let Some(at) = incomplete {
                cut = cut.min(at);
            }
        }
        self.pending
            .drain(..cut)
            .zip(self.masked.drain(..cut))
            .map(|(byte, masked)| if masked { b'*' } else { byte })
            .collect()
    }
}

pub fn bytes(value: &[u8]) -> Vec<u8> {
    Stream::default().push(value, true)
}

/// 바이너리를 바꾸지 않고 등록된 원문·인코딩 형태의 8바이트 조각 일치를 알린다.
/// 우연히 겹친 바이트도 검출하므로 차단 판정이 아닌 파일 단위 경고에만 쓴다.
/// 압축 해제·복호화·ANSI 해석은 하지 않는다.
pub fn has_secret_fragment(value: &[u8]) -> bool {
    let rules = RULES.read().unwrap_or_else(|e| e.into_inner());
    !rules.needles.is_empty() && value.windows(8).any(|needle| rules.contains(needle))
}

pub fn text(value: &str) -> String {
    String::from_utf8_lossy(&bytes(value.as_bytes())).into_owned()
}

mod live;
pub use live::LiveStream;

mod wire;

/// 가림 실패의 이유만 남긴다. serde 오류에는 원문이 들어갈 수 있어 보관하지 않는다.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("가림 직렬화 거부")]
    Serialize,
    #[error("가림 뒤 형식 복원 거부")]
    Shape,
}
impl serde::ser::Error for Error {
    fn custom<T: std::fmt::Display>(_: T) -> Self {
        Self::Serialize
    }
}

/// 타입을 읽는 동안 B64와 글을 구별한다. 형식이 깨진 값을 원문으로 되돌리지 않는다.
pub fn to_value<T: serde::Serialize + ?Sized>(input: &T) -> Result<serde_json::Value, Error> {
    let result = input.serialize(wire::Guard);
    if let Err(reason) = &result {
        tracing::warn!(%reason, "가림 실패: 메시지·기록을 거부한다");
    }
    result
}

pub fn json<T: serde::Serialize + serde::de::DeserializeOwned>(input: T) -> Result<T, Error> {
    let value = to_value(&input)?;
    serde_json::from_value(value).map_err(|_| {
        tracing::warn!(reason = "형식 복원", "가림 실패: 메시지·기록을 거부한다");
        Error::Shape
    })
}

/// tracing은 이벤트 하나의 writer에 전체 줄을 쓴다. flush/drop 때만 stderr에 내보낸다.
#[derive(Default)]
pub struct LogWriter(Vec<u8>);
impl Write for LogWriter {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        let b = bytes(&self.0);
        self.0.clear();
        std::io::stderr().write_all(&b)
    }
}
impl Drop for LogWriter {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::B64;

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Msg {
        text: String,
        raw: B64,
    }

    // c3137ed8의 운영 회귀를 실제 protocol B64 타입으로 검사한다.
    #[test]
    fn json_keeps_b64_bytes_when_only_the_encoded_text_matches() {
        let encoded_secret = aaa_test_support::token("redact-b64");
        remember(&encoded_secret);
        let raw = base64::engine::general_purpose::STANDARD.decode(format!("{}AAAA", &encoded_secret[..16])).unwrap();
        let out =
            std::panic::catch_unwind(|| json(Msg { text: format!("key {encoded_secret}"), raw: B64(raw.clone()) }));
        assert!(out.is_ok(), "B64 가림이 panic했다");
        let out = out.unwrap().unwrap();
        assert_eq!(out.raw.0, raw);
        assert!(!out.text.contains(&encoded_secret[..8]));
        // 모든 B64처럼 보이는 글을 건너뛰면 원문 비밀이 유출된다. 실제 B64만 예외다.
        let secret = &*aaa_test_support::token("redact-b64-actual");
        remember(secret);
        let out = json(Msg { text: secret.into(), raw: B64(raw.clone()) }).unwrap();
        assert_eq!(out.raw.0, raw);
        assert!(!out.text.contains(&secret[..8]));
    }

    #[test]
    fn opaque_credentials_do_not_redact_business_fields_in_the_same_process() {
        let values: Vec<_> =
            ["automation", "owner", "access_token", "refresh_token"].map(aaa_test_support::token).into();
        for value in &values {
            remember(value);
        }
        let input =
            serde_json::json!({"automation":{"owner":"owner","access_token":values[0],"refresh_token":values[1]}});
        let output = json(input).unwrap();
        assert_eq!(output["automation"]["owner"], "owner");
        for field in ["access_token", "refresh_token"] {
            let masked = output["automation"][field].as_str().expect("필드 이름 보존");
            assert!(masked.bytes().all(|b| b == b'*'));
        }
    }

    #[test]
    fn json_masks_actual_binary_secrets_including_short_fragments() {
        let secret = &*aaa_test_support::token("binary-fixture-secret-0525");
        remember(secret);
        for raw in [secret.as_bytes(), &secret.as_bytes()[..8]] {
            let out = json(B64(raw.to_vec())).unwrap();
            assert_eq!(out.0, vec![b'*'; raw.len()]);
            // 일반 JSON serializer의 B64 wire는 그대로다.
            let encoded = serde_json::to_string(&out).unwrap();
            assert_eq!(serde_json::from_str::<B64>(&encoded).unwrap(), out);
            let encoded_text = base64::engine::general_purpose::STANDARD.encode(raw);
            let clean_text = json(encoded_text).unwrap();
            assert_eq!(base64::engine::general_purpose::STANDARD.decode(clean_text).unwrap(), vec![b'*'; raw.len()]);
        }
    }

    #[test]
    fn encoded_fragments_and_nonsecret_prefix_candidates_keep_their_redaction_contract() {
        let token = aaa_test_support::token("candidate-fragments");
        let secret = format!("{token}:\"\\한글");
        remember(&secret);
        let escaped = serde_json::to_string(&secret).unwrap();
        // 원문·JSON·URL에서 실제로 달라지는 부분도 8바이트 조각만으로 가려야 한다.
        let mut forms = vec![
            secret.as_bytes().to_vec(),
            escaped.as_bytes()[1..escaped.len() - 1].to_vec(),
            format!("{token}%3A%22%5C%ED%95%9C%EA%B8%80").into_bytes(),
        ];
        for engine in [&base64::engine::general_purpose::STANDARD, &base64::engine::general_purpose::URL_SAFE] {
            for pad in 0..3 {
                let raw = [vec![b'#'; pad], secret.as_bytes().to_vec()].concat();
                let encoded = engine.encode(raw);
                forms.push(encoded.as_bytes()[4..encoded.len() - 4].to_vec());
            }
        }
        for form in forms {
            for fragment in form.windows(8) {
                assert!(has_secret_fragment(fragment));
                assert_eq!(bytes(fragment), b"********");
            }
        }
        // 앞 두 바이트만 같은 일반 바이너리를 비밀로 바꾸지 않는다.
        let candidate = [&secret.as_bytes()[..2], &[255; 6]].concat();
        assert!(!has_secret_fragment(&candidate));
        assert_eq!(bytes(&candidate), candidate);
    }

    #[test]
    fn failed_redaction_rejects_messages_and_records_without_the_original_or_error_text() {
        use serde::{Deserialize, de::Error as _};
        fn strict<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
            let value = String::deserialize(d)?;
            if value.contains('*') {
                return Err(D::Error::custom("fixture rejects mask"));
            }
            Ok(value)
        }
        #[derive(serde::Serialize, serde::Deserialize)]
        struct Strict(#[serde(deserialize_with = "strict")] String);
        let secret = &*aaa_test_support::token("reject-fixture-secret-0525");
        remember(secret);
        let result = std::panic::catch_unwind(|| json(Strict(secret.into())));
        assert!(result.is_ok(), "가림 실패가 panic했다");
        assert!(matches!(result.unwrap(), Err(Error::Shape)), "원문을 통과시켰다");

        #[derive(serde::Deserialize)]
        struct BadSerialize;
        impl serde::Serialize for BadSerialize {
            fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
                Err(serde::ser::Error::custom(aaa_test_support::token("reject-fixture-secret-0525")))
            }
        }
        assert!(matches!(json(BadSerialize), Err(Error::Serialize)));
        let root = std::env::temp_dir().join(format!("aaa-redact-{}", crate::util::new_id()));
        std::fs::create_dir_all(&root).unwrap();
        let record = root.join("record.json");
        std::fs::write(&record, b"previous safe record").unwrap();
        let error = crate::util::write_json(&record, &BadSerialize).unwrap_err();
        assert!(!error.to_string().contains(secret));
        assert_eq!(std::fs::read(&record).unwrap(), b"previous safe record");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stream_keeps_no_secret_across_any_ansi_or_chunk_boundary_and_flushes_the_tail() {
        let secret = &*aaa_test_support::token("redaction-stream");
        remember(secret);
        for input in [
            format!("before {secret} after-prefix"),
            format!("before {}\x1b[31m{}\x1b[0m after-prefix", &secret[..10], &secret[10..]),
            format!("before {}\r\n{} after-prefix", &secret[..10], &secret[10..]),
        ] {
            let input = input.as_bytes();
            let expected = bytes(input);
            let visible = String::from_utf8_lossy(&expected)
                .replace("\x1b[31m", "")
                .replace("\x1b[0m", "")
                .replace(['\r', '\n'], "");
            assert_eq!(visible, format!("before {} after-prefix", "*".repeat(secret.len())));
            for cut in 0..=input.len() {
                let mut stream = Stream::default();
                let mut out = stream.push(&input[..cut], false);
                out.extend(stream.push(&input[cut..], false));
                out.extend(stream.push(&[], true));
                assert_eq!(out, expected, "chunk cut {cut}");
                assert!(out.ends_with(b" after-prefix"));
            }
        }
    }
}
