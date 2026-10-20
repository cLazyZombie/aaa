//! 등록한 인증의 조각이 기록·화면으로 나가지 않게 하는 프로세스 수명 가림.
//! 인증 전송·금고에는 적용하지 않고, 일반 입출력 경계에서 쓴다.
use base64::Engine;
use std::collections::HashSet;
use std::io::Write;
use std::sync::{LazyLock, RwLock};

#[derive(Default)]
struct Rules {
    needles: HashSet<[u8; 8]>,
    originals: HashSet<String>,
    prefixes: HashSet<Vec<u8>>,
}
static RULES: LazyLock<RwLock<Rules>> = LazyLock::new(Default::default);

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
}
impl Stream {
    pub fn push(&mut self, bytes: &[u8], end: bool) -> Vec<u8> {
        self.pending.extend_from_slice(bytes);
        let rules = RULES.read().unwrap_or_else(|e| e.into_inner());
        if rules.needles.is_empty() {
            return std::mem::take(&mut self.pending);
        }
        let mut visible = Vec::new();
        let mut i = 0;
        let mut incomplete = None;
        while i < self.pending.len() {
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
            if rules.needles.contains(<&[u8; 8]>::try_from(n).unwrap()) {
                for (raw, _) in &visible[at..at + 8] {
                    self.pending[*raw] = b'*';
                }
            }
        }
        let mut cut = self.pending.len();
        if !end {
            for n in 1..=7.min(text.len()) {
                let start = text.len() - n;
                let suffix: Vec<u8> = visible[start..].iter().map(|(at, _)| self.pending[*at]).collect();
                if rules.prefixes.contains(&suffix) {
                    cut = cut.min(visible[start].0);
                }
            }
            if let Some(at) = incomplete {
                cut = cut.min(at);
            }
        }
        self.pending.drain(..cut).collect()
    }
}

pub fn bytes(value: &[u8]) -> Vec<u8> {
    Stream::default().push(value, true)
}
pub fn text(value: &str) -> String {
    String::from_utf8_lossy(&bytes(value.as_bytes())).into_owned()
}

/// JSON 문자열을 가린다. B64 파일·기록 응답도 디코딩한 바이트에서 검사한다.
pub fn value(v: &mut serde_json::Value) {
    if RULES.read().unwrap_or_else(|e| e.into_inner()).needles.is_empty() {
        return;
    }
    match v {
        serde_json::Value::String(s) => {
            let mut out = text(s);
            if s.len() >= 16
                && s.len().is_multiple_of(4)
                && let Ok(raw) = base64::engine::general_purpose::STANDARD.decode(s.as_bytes())
            {
                let clean = bytes(&raw);
                if clean != raw {
                    out = base64::engine::general_purpose::STANDARD.encode(clean);
                }
            }
            *s = out;
        }
        serde_json::Value::Array(a) => {
            for v in a {
                value(v)
            }
        }
        serde_json::Value::Object(o) => {
            for v in o.values_mut() {
                value(v)
            }
        }
        _ => {}
    }
}

pub fn json<T: serde::Serialize + serde::de::DeserializeOwned>(input: T) -> T {
    if RULES.read().unwrap_or_else(|e| e.into_inner()).needles.is_empty() {
        return input;
    }
    let mut v = serde_json::to_value(input).expect("가림 JSON");
    value(&mut v);
    serde_json::from_value(v).expect("가림 JSON 형식")
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
