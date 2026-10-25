//! 표시 번호의 입력과 정렬. 저장값은 바꾸지 않으며 프로젝트 경계는 호출자가 보존한다.
use std::cmp::Ordering;

/// aaa의 순번 ID만 읽는다. 무작위 q/m-ID·ULID·제목·경로는 번호로 해석하지 않는다.
pub fn short_id_number(value: &str, prefix: char) -> Option<i64> {
    if !matches!(prefix, 't' | 'd' | 'r' | 'q' | 'a' | 'p') {
        return None;
    }
    let digits = value.strip_prefix(prefix)?.strip_prefix('-')?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // 불필요한 0은 길이 제한·정수 변환 전에 제거한다.
    let digits = digits.trim_start_matches('0');
    let number: i64 = digits.parse().ok()?;
    (number > 0).then_some(number)
}

/// 기존 정본 형식: 최소 네 자리, post는 기존 최소 여덟 자리. 상위 자리는 자르지 않는다.
pub fn canonical_short_id(value: &str, prefix: char) -> Option<String> {
    let number = short_id_number(value, prefix)?;
    let width = if prefix == 'p' { 8 } else { 4 };
    Some(format!("{prefix}-{number:0width$}"))
}

/// 같은 종류의 번호는 숫자순, 그 밖의 이름은 기존 문자열순이다.
pub fn cmp_short_ids(a: &str, b: &str) -> Ordering {
    if let Some(prefix) = a.chars().next()
        && b.starts_with(prefix)
    {
        return match (short_id_number(a, prefix), short_id_number(b, prefix)) {
            (Some(a), Some(b)) => a.cmp(&b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => a.cmp(b),
        };
    }
    a.cmp(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_aliases_keep_the_existing_format_and_integer_precision() {
        for prefix in ['t', 'd', 'r', 'q', 'a', 'p'] {
            for digits in ["965", "0965", "00965", "00000000965"] {
                let canonical = if prefix == 'p' { format!("{prefix}-00000965") } else { format!("{prefix}-0965") };
                assert_eq!(canonical_short_id(&format!("{prefix}-{digits}"), prefix), Some(canonical));
            }
            assert_eq!(short_id_number(&format!("{prefix}-10000"), prefix), Some(10000));
            assert_eq!(short_id_number(&format!("{prefix}-9223372036854775807"), prefix), Some(i64::MAX));
            for digits in ["", "0", "0000", "+1", "-1", "1x", "１", "9223372036854775808"] {
                assert_eq!(canonical_short_id(&format!("{prefix}-{digits}"), prefix), None);
            }
        }
        for value in ["demo/t-965", "t-965.md", " t-965", "t-965/", "t-965x", "T-965", "q-12abc34567"] {
            assert_eq!(canonical_short_id(value, 't'), None);
        }
        assert_eq!(canonical_short_id("m-0000000123", 'm'), None);
        assert_eq!(cmp_short_ids("t-9999", "t-10000"), Ordering::Less);
        assert_eq!(cmp_short_ids("r-99999", "r-100000"), Ordering::Less);
        assert_eq!(cmp_short_ids("t-965", "t-00965"), Ordering::Equal);
    }
}
