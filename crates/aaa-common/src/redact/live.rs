//! 대화형 PTY용 순차 가림(q-1285): 앞 최대 7바이트는 먼저 보일 수 있다.
//! 다음 글자를 기다리지 않고, 원문 8바이트 조각이 완성되는 바이트부터 가린다.
//! 일반 기록·오류의 완성된 자료 가림(Stream/bytes)은 그대로 둔다.
use super::{RULES, Rules};

#[derive(Clone, Default)]
struct Window {
    bytes: [u8; 8],
    len: usize,
}
impl Window {
    fn push(&mut self, byte: u8, rules: &Rules) -> bool {
        if self.len == self.bytes.len() {
            self.bytes.copy_within(1.., 0);
            self.bytes[7] = byte;
        } else {
            self.bytes[self.len] = byte;
            self.len += 1;
        }
        self.len == 8 && rules.needles.contains(&self.bytes)
    }
}

#[derive(Default)]
enum Escape {
    #[default]
    Ground,
    Start,
    Csi,
}

/// 원문 창은 가린 뒤에도 보존해 겹치는 인증 조각을 계속 검사한다.
/// CSI와 CR/LF를 뺀 창도 청크 사이에 유지한다. escape는 기다리지 않고 전달하며
/// 그 문법 상태만 기억하므로 ANSI 끝 글자도 다음 입력까지 붙잡히지 않는다.
#[derive(Default)]
pub struct LiveStream {
    raw: Window,
    visible: Window,
    escape: Escape,
    before_escape: Window,
}
impl LiveStream {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        let rules = RULES.read().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::with_capacity(bytes.len());
        for &byte in bytes {
            let mut mask = self.raw.push(byte, &rules);
            let visible = match (&self.escape, byte) {
                (_, 27) => {
                    self.before_escape = self.visible.clone();
                    self.escape = Escape::Start;
                    true
                }
                (Escape::Start, b'[') => {
                    self.visible = self.before_escape.clone();
                    self.escape = Escape::Csi;
                    false
                }
                (Escape::Csi, _) => {
                    if (0x40..=0x7e).contains(&byte) {
                        self.escape = Escape::Ground;
                    }
                    false
                }
                _ => {
                    self.escape = Escape::Ground;
                    !matches!(byte, b'\r' | b'\n')
                }
            };
            if visible {
                mask |= self.visible.push(byte, &rules);
            }
            out.push(if mask { b'*' } else { byte });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::redact::{bytes, remember};

    #[test]
    fn iphone_ascii_send_sequence_is_visible_before_the_next_key() {
        for ch in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".chars() {
            remember(&format!("{ch}~~~~~~~~~~~"));
        }
        let mut live = LiveStream::default();
        // 실기에서 관측한 send 순서. keydown은 각 ASCII를 즉시 send한다.
        for ch in "status aaa123! 한글\x7f\r".chars() {
            let mut encoded = [0; 4];
            let input = ch.encode_utf8(&mut encoded).as_bytes();
            assert_eq!(live.push(input), input, "다음 키 없이 {ch:?}가 나와야 한다");
        }
        assert!(live.push(b"").is_empty());
    }

    #[test]
    fn overlapping_secret_windows_are_masked_without_waiting_or_forgetting_raw_context() {
        let secret = b"live-output-canary-1002";
        remember(std::str::from_utf8(secret).unwrap());
        let expected = [secret[..7].to_vec(), vec![b'*'; secret.len() - 7]].concat();
        // chunk 경계와 무관한 결과. 이전의 완성된 자료 가림은 앞 7바이트도 가린다.
        assert_eq!(bytes(secret), vec![b'*'; secret.len()]);
        for split in 0..=secret.len() {
            let mut live = LiveStream::default();
            let mut out = live.push(&secret[..split]);
            assert_eq!(out.len(), split);
            out.extend(live.push(&secret[split..]));
            assert_eq!(out, expected, "split={split}");
        }
        let mut live = LiveStream::default();
        let out: Vec<_> = secret.iter().flat_map(|b| live.push(&[*b])).collect();
        assert_eq!(out, expected);
    }

    #[test]
    fn encoded_forms_and_non_csi_escape_keep_eight_byte_protection() {
        use base64::Engine;
        let secret = "live-\"canary/1002?value";
        remember(secret);
        let json = serde_json::to_string(secret).unwrap();
        let url: String = secret
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        let forms = [
            secret.to_string(),
            json[1..json.len() - 1].to_string(),
            url,
            base64::engine::general_purpose::STANDARD.encode(secret),
            base64::engine::general_purpose::URL_SAFE.encode(secret),
        ];
        for form in forms {
            let reference = LiveStream::default().push(form.as_bytes());
            assert_ne!(reference, form.as_bytes());
            let rules = RULES.read().unwrap();
            assert!(!reference.windows(8).any(|w| rules.needles.contains(<&[u8; 8]>::try_from(w).unwrap())));
            drop(rules);
            for split in 0..=form.len() {
                let mut live = LiveStream::default();
                let mut out = live.push(&form.as_bytes()[..split]);
                out.extend(live.push(&form.as_bytes()[split..]));
                assert_eq!(out, reference);
            }
        }
        // CSI가 아닌 ESC도 원문의 일부다. 앞서 내보낸 ESC를 다음 청크에서 잊지 않는다.
        let secret = b"abcdefg\x1bXYZ12345";
        remember(std::str::from_utf8(secret).unwrap());
        let mut live = LiveStream::default();
        assert_eq!(live.push(&secret[..8]), b"abcdefg*");
        assert_eq!(live.push(&secret[8..]), vec![b'*'; secret.len() - 8]);
    }

    #[test]
    fn ansi_and_newlines_do_not_split_secret_detection_or_hold_terminal_controls() {
        let secret = b"vWxYzKpQmNrS1002";
        remember(std::str::from_utf8(secret).unwrap());
        let mut input = Vec::new();
        let mut expected = Vec::new();
        for (i, byte) in secret.iter().enumerate() {
            input.push(*byte);
            expected.push(if i < 7 { *byte } else { b'*' });
            for control in [b"\x1b[31m".as_slice(), b"\r\n", b"\x1b[0m"] {
                input.extend(control);
                expected.extend(control);
            }
        }
        for split in 0..=input.len() {
            let mut live = LiveStream::default();
            let mut out = live.push(&input[..split]);
            assert_eq!(out.len(), split);
            out.extend(live.push(&input[split..]));
            assert_eq!(out, expected, "split={split}");
        }
        let mut live = LiveStream::default();
        let out: Vec<_> = input.iter().flat_map(|b| live.push(&[*b])).collect();
        assert_eq!(out, expected);
    }
}
