//! 보고 첨부의 텍스트만 가린다. 바이너리의 서명·hash·파일 구조는 바꾸지 않는다.
use std::borrow::Cow;

use aaa_proto::FileKind;

/// 알려진 바이너리 형식은 내용이 ASCII여도 보존한다. 모르는 이름은 파일 전체가
/// UTF-8 글이고 이진 제어 문자가 없을 때만 텍스트로 취급한다. 앞부분만 보고
/// 뒤쪽 NUL·비UTF8을 놓치거나, 손실 변환으로 바이너리를 텍스트로 만들지 않는다.
fn is_text(path: &str, bytes: &[u8]) -> bool {
    if let Some(mime) = aaa_proto::mime_of(path)
        && FileKind::of_mime(mime) != FileKind::Text
        && mime != "image/svg+xml"
    {
        return false;
    }
    std::str::from_utf8(bytes).is_ok() && !bytes.iter().any(|b| matches!(b, 0..=8 | 14..=26 | 28..=31 | 127))
}

pub(crate) fn prepare<'a>(path: &str, bytes: &'a [u8]) -> (Cow<'a, [u8]>, Option<String>) {
    if is_text(path, bytes) {
        return (Cow::Owned(aaa_common::redact::bytes(bytes)), None);
    }
    let warning = aaa_common::redact::has_secret_fragment(bytes).then(|| {
        format!(
            "{path}: 등록된 비밀값의 8바이트 조각과 일치한다(오탐 가능). 바이너리는 원본 그대로 복사했다. 공유 전에 원본의 비밀값 포함 여부를 확인한다. 압축·암호화 내용은 검사하지 않는다."
        )
    });
    (Cow::Borrowed(bytes), warning)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn binary_hashes_survive_secret_fragments_and_text_like_extensions() {
        use base64::Engine;
        let secret = aaa_test_support::token("report-binary");
        aaa_common::redact::remember(&secret);
        let binary = [b"MZ\0\xff".as_slice(), secret.as_bytes()].concat();
        for name in ["setup.exe", "archive.zip", "image.png", "file", "misnamed.log"] {
            let (copy, warning) = prepare(name, &binary);
            assert_eq!(Sha256::digest(&copy), Sha256::digest(&binary), "{name}");
            assert_eq!(copy.as_ref(), binary);
            let warning = warning.unwrap();
            assert!(warning.starts_with(name));
            assert!(!warning.contains(secret.as_str()));
        }
        // UTF-8과 NUL 검사만으로는 ASCII인 바이너리·압축 파일을 보호하지 못한다.
        for name in ["data.bin", "data.tar", "data.pdf", "data.wav"] {
            let (copy, warning) = prepare(name, secret.as_bytes());
            assert_eq!(copy.as_ref(), secret.as_bytes());
            assert!(warning.is_some());
        }
        // 원문 전체가 아닌 조각·인코딩 형태도 경고하며 바이트를 치환하지 않는다.
        for payload in [
            secret.as_bytes()[8..16].to_vec(),
            base64::engine::general_purpose::STANDARD.encode(secret.as_bytes()).into_bytes(),
        ] {
            let binary = [b"\0".as_slice(), &payload].concat();
            let (copy, warning) = prepare("encoded.bin", &binary);
            assert_eq!(copy.as_ref(), binary);
            assert!(warning.is_some());
        }
        // prefix sniff 뒤에 오는 비UTF8·제어 문자도 바이너리로 보존한다.
        for suffix in [0, 1, 255] {
            let binary = [vec![b'x'; 8192], secret.as_bytes().to_vec(), vec![suffix]].concat();
            assert_eq!(prepare("unknown", &binary).0.as_ref(), binary);
        }
    }

    #[test]
    fn reports_logs_svg_and_unknown_utf8_text_still_mask_secrets() {
        let secret = aaa_test_support::token("report-text");
        aaa_common::redact::remember(&secret);
        let text = format!("한글\t\x1b[31m{secret}\x1b[0m\r\n");
        let expected = aaa_common::redact::bytes(text.as_bytes());
        assert_ne!(expected, text.as_bytes());
        for name in ["report.md", "stable.log", "index.html", "data.json", "image.svg", "unknown"] {
            let (copy, warning) = prepare(name, text.as_bytes());
            assert_eq!(copy.as_ref(), expected, "{name}");
            assert!(warning.is_none());
        }
        assert!(prepare("safe.bin", b"\0\xff").1.is_none());
    }
}
