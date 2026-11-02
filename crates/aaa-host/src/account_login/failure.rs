//! CLI 원문은 버리고 고정된 실패 분류만 남긴다. URL·코드·토큰·메일·오류 본문을 반환하거나 기록하지 않는다.
use super::{ErrorBody, ErrorCode, MAX_OUTPUT, plain};

pub(super) fn claude(output: &[u8], exit_code: Option<u32>) -> ErrorBody {
    let output_text = plain(output).unwrap_or_default().to_ascii_lowercase();
    // 앞선 잘못된 입력 안내가 마지막 실제 실패를 덮지 않게 한다.
    let text = output_text.rsplit_once("login failed:").map_or(output_text.as_str(), |(_, last)| last);
    let status = regex::Regex::new(r"token exchange failed \(([1-5][0-9]{2})\)")
        .ok()
        .and_then(|pattern| pattern.captures(text))
        .and_then(|captures| captures[1].parse::<u16>().ok());
    let (reason, message) = if output.len() as u64 > MAX_OUTPUT {
        ("output_limit", "Claude 인증 출력이 상한을 넘었습니다")
    } else if text.contains("state mismatch") || text.contains("invalid state parameter") {
        ("state_mismatch", "Claude 인증 상태 확인이 실패했습니다")
    } else if text.contains("invalid authorization code")
        || text.contains("invalid code. please make sure")
        || status == Some(401)
    {
        ("invalid_code", "Claude 인증 코드를 확인하지 못했습니다")
    } else if status == Some(403) {
        ("access_denied", "Claude 인증 요청이 거부됐습니다")
    } else if status == Some(429) {
        ("rate_limited", "Claude 인증 요청이 제한됐습니다")
    } else if ["fetch failed", "network error", "econnreset", "enotfound", "etimedout"].iter().any(|s| text.contains(s))
    {
        ("network_failed", "Claude 인증 서버 연결이 실패했습니다")
    } else if [
        "keychain access denied",
        "keychain is locked",
        "failed to save credentials",
        "could not save credentials",
    ]
    .iter()
    .any(|s| text.contains(s))
    {
        ("credential_store_failed", "공식 Claude CLI가 인증을 저장하지 못했습니다")
    } else if text.contains("eacces") || text.contains("eperm") {
        ("permission_denied", "공식 Claude CLI 인증 권한 확인이 실패했습니다")
    } else if text.contains("emfile") || text.contains("enfile") {
        ("descriptor_limit", "공식 Claude CLI가 인증 파일을 열지 못했습니다")
    } else if text.contains("enospc") {
        ("storage_full", "공식 Claude CLI의 인증 저장 공간이 부족합니다")
    } else if ["cert_has_expired", "unable_to_verify_leaf_signature", "self_signed_cert_in_chain"]
        .iter()
        .any(|s| text.contains(s))
    {
        ("tls_failed", "Claude 인증 서버의 연결 인증을 확인하지 못했습니다")
    } else if text.contains("login interrupted") || text.contains("input ended before a code was pasted") {
        ("interrupted", "Claude 인증 입력이 종료됐습니다")
    } else if status.is_some() {
        ("exchange_failed", "Claude 인증 교환이 실패했습니다")
    } else {
        ("unknown", "공식 Claude CLI 인증이 실패했습니다")
    };
    tracing::warn!(stage = "cli", reason, exit_code, http_status = status, "Claude 로그인 실패 분류");
    ErrorBody::new(ErrorCode::BadRequest, message)
}

pub(super) fn finish(status: Option<(bool, Option<u32>)>, output: &[u8]) -> Result<(), ErrorBody> {
    if matches!(status, Some((true, _))) && output.len() as u64 <= MAX_OUTPUT {
        Ok(())
    } else {
        Err(claude(output, status.and_then(|(_, code)| code)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cli_output_only_yields_fixed_messages_and_unknown_stays_unknown() {
        let secret = aaa_test_support::token("cli-error-secret");
        for (raw, expected) in [
            ("Authentication failed: Invalid authorization code", "Claude 인증 코드를 확인하지 못했습니다"),
            ("Token exchange failed (403): Forbidden", "Claude 인증 요청이 거부됐습니다"),
            ("Token exchange failed (429): Too Many Requests", "Claude 인증 요청이 제한됐습니다"),
            ("Login failed: fetch failed", "Claude 인증 서버 연결이 실패했습니다"),
            (
                "Invalid code. Please make sure the full code was copied.\nLogin failed: fetch failed",
                "Claude 인증 서버 연결이 실패했습니다",
            ),
            ("Login failed: failed to save credentials", "공식 Claude CLI가 인증을 저장하지 못했습니다"),
            ("unexpected provider text", "공식 Claude CLI 인증이 실패했습니다"),
        ] {
            let value = format!("{raw}\n{secret}\nhttps://example.com/?code={secret}\nme@example.com");
            let error = finish(Some((false, Some(1))), value.as_bytes()).unwrap_err();
            assert_eq!(error.message, expected);
            let response = serde_json::to_string(&error).unwrap();
            assert!(!response.contains(&secret) && !response.contains("example.com"));
        }
        assert!(finish(Some((true, Some(0))), b"Login successful").is_ok());
    }
}
