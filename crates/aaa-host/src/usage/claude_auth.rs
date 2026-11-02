//! 조회 전용 OAuth. 공식 CLI가 새로 발급한 인증만 받아 host가 단독으로 갱신한다.
//! 개인 홈/기본 Keychain이나 실행용 setup-token을 읽지 않는다.
use super::*;
use aaa_proto::{AccountCredential, ErrorBody, ErrorCode, QuotaProblem, QuotaSnapshot, QuotaSource};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub(crate) struct Auth {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_ms: u64,
    pub scopes: Vec<String>,
}

fn fail() -> ErrorBody {
    ErrorBody::new(ErrorCode::BadRequest, "조회 인증을 읽지 못했습니다. Claude 구독 계정으로 다시 로그인하세요")
}
impl Auth {
    pub(crate) fn parse(raw: &str) -> Result<Self, ErrorBody> {
        let auth: Self = serde_json::from_str(raw).map_err(|_| fail())?;
        let valid_token =
            |s: &str| (12..=16_384).contains(&s.len()) && !s.chars().any(|c| c.is_whitespace() || c.is_control());
        if !valid_token(&auth.access_token)
            || !valid_token(&auth.refresh_token)
            || auth.expires_ms == 0
            || !auth.scopes.iter().any(|s| s == "user:profile")
        {
            return Err(fail());
        }
        aaa_common::redact::remember(&auth.access_token);
        aaa_common::redact::remember(&auth.refresh_token);
        Ok(auth)
    }
}

fn service(home: &Path) -> String {
    let hash = hex::encode(Sha256::digest(home.to_string_lossy().as_bytes()));
    format!("Claude Code-credentials-{}", &hash[..8])
}

fn login_read_error(reason: &'static str) -> ErrorBody {
    tracing::warn!(stage = "credential_read", reason, "조회 로그인 인증 읽기 실패");
    let message = match reason {
        "missing" => "공식 Claude CLI가 조회 인증을 남기지 않았습니다",
        "keychain_locked" => "조회 인증의 Keychain을 읽지 못했습니다",
        "file_read_failed" => "조회 인증 파일을 읽지 못했습니다",
        _ => "공식 Claude CLI의 조회 인증 형식을 확인하지 못했습니다",
    };
    ErrorBody::new(ErrorCode::BadRequest, message)
}

/// 유일한 임시 로그인 폴더의 파일 또는 그 폴더 전용 Keychain만 읽는다.
fn login_secret(ctx: &UsageContext, home: &Path) -> Result<String, ErrorBody> {
    let path = home.join(".credentials.json");
    let raw = if path.symlink_metadata().is_ok_and(|m| m.is_file() && m.len() <= 128 * 1024) {
        std::fs::read_to_string(path).map_err(|_| login_read_error("file_read_failed"))?
    } else if ctx.darwin {
        match (ctx.keychain)(&service(home), None) {
            KeychainRead::Found(raw) => raw,
            KeychainRead::Missing => return Err(login_read_error("missing")),
            KeychainRead::Locked => return Err(login_read_error("keychain_locked")),
        }
    } else {
        return Err(login_read_error("missing"));
    };
    let v: Value = serde_json::from_str(&raw).map_err(|_| login_read_error("invalid_json"))?;
    let oauth = &v["claudeAiOauth"];
    let auth = serde_json::json!({
        "access_token": oauth["accessToken"], "refresh_token": oauth["refreshToken"],
        "expires_ms": oauth["expiresAt"], "scopes": oauth["scopes"],
    })
    .to_string();
    Auth::parse(&auth).map_err(|_| login_read_error("invalid_credentials"))?;
    Ok(auth)
}

/// 개인 항목과 이름을 공유하지 않는다. 취소·실패도 새 임시 항목을 정리한다.
fn cleanup(ctx: &UsageContext, home: &Path) -> bool {
    if !ctx.darwin {
        return true;
    }
    let service = service(home);
    match (ctx.keychain)(&service, None) {
        KeychainRead::Missing => true,
        KeychainRead::Locked => {
            tracing::warn!(stage = "cleanup", reason = "keychain_locked", "조회 로그인 정리 실패");
            false
        }
        KeychainRead::Found(_) => {
            let ok = (ctx.run)(&["security", "delete-generic-password", "-s", &service]).is_some();
            if !ok {
                tracing::warn!(stage = "cleanup", reason = "delete_failed", "조회 로그인 정리 실패");
            }
            ok
        }
    }
}

impl UsageService {
    pub(crate) fn claude_login_secret(&self, home: &Path) -> Result<String, ErrorBody> {
        login_secret(&self.ctx, home)
    }
    pub(crate) fn claude_login_cleanup(&self, home: &Path) -> bool {
        cleanup(&self.ctx, home)
    }

    /// 새 access token과 회전한 refresh token은 암호화 금고에 먼저 확정한다.
    pub(crate) async fn claude_usage_refresh(
        &self,
        raw: &str,
        now: u64,
    ) -> Result<Option<String>, (Option<u16>, Option<u64>)> {
        let mut auth = Auth::parse(raw).map_err(|_| (Some(401), None))?;
        if auth.expires_ms > now.saturating_add(60_000) {
            return Ok(None);
        }
        let body = serde_json::json!({
            "grant_type":"refresh_token", "refresh_token":auth.refresh_token,
            "client_id":"9d1c250a-e61b-44d9-88ed-5944d1962f5e", "scope":auth.scopes.join(" "),
        })
        .to_string();
        let value = request_json(
            &self.ctx,
            "POST",
            "https://platform.claude.com/v1/oauth/token",
            &[("Content-Type", "application/json".into()), ("User-Agent", USER_AGENT.into())],
            body.as_bytes(),
        )
        .await
        .map_err(|e| match e {
            ReadError::Http { status, retry_after_ms } => (Some(status), retry_after_ms),
            _ => (None, None),
        })?;
        let token = value["access_token"].as_str().filter(|s| s.len() >= 12).ok_or((None, None))?;
        aaa_common::redact::remember(token);
        auth.access_token = token.into();
        if let Some(token) = value.get("refresh_token") {
            let token = token.as_str().filter(|s| s.len() >= 12).ok_or((None, None))?;
            aaa_common::redact::remember(token);
            auth.refresh_token = token.into();
        }
        let seconds = value["expires_in"].as_u64().filter(|s| *s > 60 && *s <= 366 * 86400).ok_or((None, None))?;
        auth.expires_ms = now.saturating_add(seconds * 1000);
        if let Some(scopes) = value.get("scope") {
            auth.scopes = scopes.as_str().ok_or((Some(401), None))?.split_whitespace().map(str::to_string).collect();
        }
        let raw = serde_json::to_string(&auth).map_err(|_| (None, None))?;
        Auth::parse(&raw).map_err(|_| (Some(401), None))?;
        Ok(Some(raw))
    }

    pub(crate) async fn claude_usage_quota(
        &self,
        accounts: &crate::accounts::Accounts,
        credential: &AccountCredential,
        generation: String,
        now: u64,
    ) -> (QuotaSnapshot, Option<u16>) {
        let refreshed = self.claude_usage_refresh(&credential.secret.0, now).await;
        let failure = match refreshed {
            Ok(raw) => {
                let raw = raw.as_deref().unwrap_or(&credential.secret.0);
                if raw != credential.secret.0
                    && accounts.store_usage(&generation, raw, Some(credential.version)).is_err()
                {
                    (None, None)
                } else {
                    match Auth::parse(raw) {
                        Ok(auth) => return self.claude_account_quota(generation, &auth.access_token, now).await,
                        Err(_) => (Some(401), None),
                    }
                }
            }
            Err(e) => e,
        };
        let (status, retry) = failure;
        (
            QuotaSnapshot {
                input_missing: None,
                generation,
                source: QuotaSource::ClaudeOauth,
                sampled_at_ms: now,
                windows: vec![],
                problem: Some(if matches!(status, Some(400 | 401 | 403)) {
                    QuotaProblem::Unsupported
                } else {
                    QuotaProblem::Failed
                }),
                retry_at_ms: (status == Some(429)).then(|| now.saturating_add(RETRY_MS.max(retry.unwrap_or(RETRY_MS)))),
            },
            status,
        )
    }
}
