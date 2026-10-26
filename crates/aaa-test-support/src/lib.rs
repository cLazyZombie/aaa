//! Rust·Python·Node 시험이 공유하는 비밀 fixture 규약 v1.
//! 업무 이름은 SHA-256 입력으로만 쓰고 토큰에는 담지 않는다. 운영 인증에 쓰지 않는다.
use sha2::{Digest, Sha256};

/// const 문맥과 외부 스크립트에 쓰는 token("installation")의 공용 산출물.
pub const CANARY: &str = include_str!("../../../scripts/fixtures/secret-canary.txt");

/// 같은 UTF-8 seed는 모든 언어에서 같은 96바이트 canary가 된다.
/// 각 digest byte를 g..v 문자와 00..15 두 숫자로 무손실 인코딩한다.
/// 모든 8바이트 조각에 비-hex 문자와 숫자가 있어 사전 단어·JSON 필드·digest와 겹치지 않는다.
pub fn token(seed: &str) -> String {
    let mut out = String::with_capacity(96);
    for b in Sha256::digest(seed.as_bytes()) {
        out.push((b'g' + b / 16) as char);
        out.push((b'0' + (b % 16) / 10) as char);
        out.push((b'0' + (b % 16) % 10) as char);
    }
    assert_opaque(&out);
    out
}

/// 같은 시험의 반복 로그인·서로 다른 실험실에 독립 canary가 필요할 때 쓴다.
pub fn fresh_token() -> String {
    token(&format!("{:?}", rand::random::<[u8; 32]>()))
}

/// 일반 단어를 토큰으로 전달하면 원문을 오류에 남기지 않고 즉시 실패한다.
pub fn assert_opaque(value: &str) {
    assert!(
        value.len() == 96
            && value.as_bytes().as_chunks::<3>().0.iter().all(|p| {
                (b'g'..=b'v').contains(&p[0])
                    && matches!(p[1], b'0' | b'1')
                    && p[2].is_ascii_digit()
                    && (p[1] == b'0' || p[2] <= b'5')
            }),
        "인증 fixture는 aaa_test_support::token/fresh_token으로 만든 opaque 값이어야 한다"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_vectors_and_business_words_stay_separate() {
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../../scripts/fixtures/secret-vectors.json")).unwrap();
        for vector in vectors.as_array().unwrap() {
            let seed = vector["seed"].as_str().unwrap();
            let value = token(seed);
            assert_eq!(value, vector["token"].as_str().unwrap());
            assert!(!value.contains(seed));
        }
        for word in ["automation", "access_token", "refresh_token", "owner", "fixture-automation-token"] {
            assert!(std::panic::catch_unwind(|| assert_opaque(word)).is_err());
        }
        for value in [format!("{}\n", token("owner")), token("owner")[1..].to_string(), "g99".repeat(32)] {
            assert!(std::panic::catch_unwind(|| assert_opaque(&value)).is_err());
        }
        assert_eq!(CANARY, token("installation"));
        assert_ne!(fresh_token(), fresh_token());
    }
}
