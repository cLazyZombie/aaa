//! 계정 메일·plan·인증 시각·상태(관리 GUI W5a, plan §8.351). 금고 파일 모양은 바꾸지 않고 host DB meta `account-meta:<이름>`에
//! 둔다(키 관리는 별도 일이다). 메일·plan은 조작 기기와 host의 세션 밖 CLI에만 보인다: 감사 기록·알림함·로그에는 별칭만 남긴다.
//! 「끊김」은 제공자가 Codex 인증 갱신을 거부했거나 알려진 만료 시각이 지난 것뿐이다. 상태를 보려고 제공자에게 따로 묻지 않는다.
//! Codex 금고 인증은 access token 만료 하루 전에 host가 미리 갱신하고(t-1688), 그 결과도 같은 판정에 쓴다.
//! 이어진 갱신 실패는 원인(aaa 안·망·제공자 상태와 오류 코드)과 멈춘 thread를 기록해 알리고, 회복하면 멈췄던 thread를 알린다.
use std::path::Path;

use aaa_common::util;
use aaa_proto::{AccountHealth, AccountView, DeviceRole};
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::Host;

/// 만료 이만큼 전부터 「곧 만료」
pub(crate) const EXPIRING_MS: u64 = 7 * 24 * 3600 * 1000;
/// Codex access token 만료 이만큼 전에 host가 미리 갱신한다(t-1688 답 1B)
pub(crate) const REFRESH_LEAD_MS: u64 = 24 * 3600 * 1000;
/// 선제 갱신이 실패하면 이만큼 뒤에 다시 한다(CLIProxyAPI auto_refresh_loop와 같은 5분)
pub(crate) const REFRESH_RETRY_MS: u64 = 5 * 60 * 1000;
/// pane이 멈추지 않은 갱신 실패는 이만큼 이어지면 알린다(한 번의 일시 실패는 다음 시도가 덮는다)
const REFRESH_ALERT_ATTEMPTS: u32 = 3;
/// 공식 CLI 결과에서 읽는 파일 상한(설정 파일이다)
const MAX_FILE: u64 = 1 << 20;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AccountMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_auth_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_auth_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_ms: Option<u64>,
    /// 제공자가 갱신을 거부한 때. 갱신·로그인이 성공하면 지운다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected_ms: Option<u64>,
    /// 알림함에 낸 상태(같은 상태를 두 번 내지 않는다)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alerted: Option<AccountHealth>,
    /// Codex 금고 인증의 access token 만료 시각(JWT `exp`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_expires_ms: Option<u64>,
    /// host가 제공자에게서 새 인증을 받은 마지막 시각
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refreshed_ms: Option<u64>,
    /// 이어진 갱신 실패(thread 목록은 `stalled`에 둔다). 갱신·로그인이 성공하면 지운다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_failure: Option<aaa_proto::AccountRefreshFailure>,
    /// 갱신 실패로 멈춘 thread. 회복을 알린 뒤 지운다
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stalled: Vec<String>,
    /// 갱신 실패 알림을 냈다
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refresh_alerted: bool,
}

/// 로그인 결과에서 읽은 것(인증값은 담지 않는다)
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Identity {
    pub email: Option<String>,
    pub plan: Option<String>,
    pub expires_ms: Option<u64>,
}

pub(crate) fn health(m: &AccountMeta, now: u64) -> AccountHealth {
    if m.rejected_ms.is_some() {
        AccountHealth::Rejected
    } else if m.expires_ms.is_some_and(|e| e <= now) {
        AccountHealth::Expired
    } else if m.expires_ms.is_some_and(|e| e - now <= EXPIRING_MS) {
        AccountHealth::Expiring
    } else {
        AccountHealth::Ok
    }
}

/// 알림할 갱신 실패인가: 제공자 거부(끊김)는 상태 알림이 맡는다. pane이 멈췄거나 실패가 이어지면 알린다
fn refresh_alert(m: &AccountMeta) -> bool {
    m.refresh_failure
        .as_ref()
        .is_some_and(|f| !f.relogin && (!m.stalled.is_empty() || f.attempts >= REFRESH_ALERT_ATTEMPTS))
}

/// Codex auth.json의 `tokens.access_token`(JWT) `exp`(초)를 ms로. JWT가 아니거나 `exp`가 없으면 없음(서명은 보지 않는다)
pub(crate) fn access_expires_ms(auth: &str) -> Option<u64> {
    let auth: serde_json::Value = serde_json::from_str(auth).ok()?;
    let jwt = auth.get("tokens")?.get("access_token")?.as_str()?;
    let body = jwt.split('.').nth(1)?.trim_end_matches('=');
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(body).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    claims.get("exp")?.as_u64()?.checked_mul(1000)
}

/// thread 이름 몇 개(나머지는 수)
fn thread_list(threads: &[String]) -> String {
    let mut out = threads.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    if threads.len() > 5 {
        out.push_str(&format!(" 외 {}개", threads.len() - 5));
    }
    out
}

/// 메일 꼴만 받는다(공백·제어 문자·지나친 길이 없음)
fn email(v: Option<&serde_json::Value>) -> Option<String> {
    validated_email(v?.as_str()?).ok()
}

pub(crate) fn validated_email(value: &str) -> Result<String, aaa_proto::ErrorBody> {
    let s = value.trim();
    let ok = s.len() <= 254
        && s.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.'))
        && !s.chars().any(|c| c.is_whitespace() || c.is_control());
    if ok {
        Ok(s.to_string())
    } else {
        Err(aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::BadRequest, "메일 주소를 확인하세요"))
    }
}

fn short(v: Option<&serde_json::Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    (!s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_ .".contains(&b)))
        .then(|| s.to_string())
}

/// Codex auth.json의 `tokens.id_token`(JWT) 본문에서 메일·plan을 읽는다. 서명은 확인하지 않는다(화면 표시용이다).
/// 메일은 `email`(옛 판은 `https://api.openai.com/profile`의 `email`), plan은 `https://api.openai.com/auth`의 `chatgpt_plan_type`
pub(crate) fn codex_identity(auth: &str) -> Identity {
    let claims = (|| {
        let auth: serde_json::Value = serde_json::from_str(auth)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        let jwt = auth.get("tokens")?.get("id_token")?.as_str()?;
        let body = jwt.split('.').nth(1)?.trim_end_matches('=');
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(body)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
    })();
    let Some(c) = claims else { return Identity::default() };
    Identity {
        email: email(c.get("email")).or_else(|| email(c.pointer("/https:~1~1api.openai.com~1profile/email"))),
        plan: short(c.pointer("/https:~1~1api.openai.com~1auth/chatgpt_plan_type")),
        expires_ms: None,
    }
}

/// Claude `setup-token`: 출력의 「valid for N 단위」로 만료 시각을, 로그인 폴더의 `.claude.json`에 `oauthAccount.emailAddress`가
/// 있으면 메일을 읽는다. 공식 CLI가 메일을 남기지 않으면 모른다
pub(crate) fn claude_identity(output: &str, dirs: &[&Path], now: u64) -> Identity {
    let mut out = Identity { expires_ms: valid_for_ms(output).map(|d| now.saturating_add(d)), ..Default::default() };
    for dir in dirs {
        let p = dir.join(".claude.json");
        let Ok(m) = p
            .symlink_metadata()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            continue;
        };
        if !m.is_file() || m.len() > MAX_FILE {
            continue;
        }
        let Some(v) = std::fs::read(&p)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .and_then(|b| {
                serde_json::from_slice::<serde_json::Value>(&b)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
        else {
            continue;
        };
        if let Some(e) = email(v.pointer("/oauthAccount/emailAddress")) {
            out.email = Some(e);
            break;
        }
    }
    out
}

/// 「valid for 1 year」·「valid for 30 days」·「valid for 8h」 같은 기간(ms). 읽지 못하면 없음
fn valid_for_ms(text: &str) -> Option<u64> {
    let re =
        regex::Regex::new(r"(?i)valid for\s+(\d{1,5})\s*(years?|yrs?|y|months?|mo|weeks?|w|days?|d|hours?|hrs?|h)\b")
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
    let c = re.captures(text)?;
    let n: u64 = c[1]
        .parse()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    let unit = c[2].to_ascii_lowercase();
    let day = 24 * 3600 * 1000;
    let each = match unit.as_str() {
        "year" | "years" | "yr" | "yrs" | "y" => 365 * day,
        "month" | "months" | "mo" => 30 * day,
        "week" | "weeks" | "w" => 7 * day,
        "day" | "days" | "d" => day,
        _ => 3600 * 1000,
    };
    n.checked_mul(each)
}

/// 메일·plan을 보일 호출자: 조작 기기(웹)와 host의 세션 밖 CLI(사용자 답 5A). 보기 전용 기기와 세션 안 agent에는 별칭만
pub(crate) fn shows_email(role: DeviceRole, actor: &str) -> bool {
    match role {
        DeviceRole::Controller => actor.starts_with("device:"),
        DeviceRole::Local => actor.starts_with("local:") && !crate::front::in_session(actor),
        DeviceRole::Reader => false,
    }
}

pub(crate) fn hide_email(views: &mut [AccountView]) {
    for v in views {
        v.email = None;
        v.plan = None;
        v.plan_override = None;
        v.detected_plan = None;
    }
}

fn key(name: &str) -> String {
    format!("account-meta:{name}")
}

impl Host {
    pub(crate) fn account_plan(
        &self,
        name: &str,
        plan: Option<&str>,
        actor: &str,
    ) -> Result<aaa_proto::FrontResponse, aaa_proto::ErrorBody> {
        let account = self
            .accounts
            .list()?
            .into_iter()
            .find(|a| a.name == name)
            .ok_or_else(|| crate::err(aaa_proto::ErrorCode::BadRequest, "계정을 찾지 못했습니다"))?;
        if plan.is_some_and(|id| {
            !crate::account_plan::options().iter().any(|p| p.id == id && p.harness == account.harness)
        }) {
            return Err(crate::err(aaa_proto::ErrorCode::BadRequest, "이 제공자의 요금제를 선택하세요"));
        }
        let _gate = self.account_meta_gate.lock().unwrap();
        let mut meta = self.account_meta(name);
        meta.plan_override = plan.map(str::to_owned);
        self.store_account_meta(name, &meta)?;
        self.db.audit(actor, "account_plan", name);
        self.tick_wake.notify_one();
        Ok(aaa_proto::FrontResponse::Done)
    }

    pub(crate) fn account_session_limit(&self, name: &str, harness: &str) -> Option<bool> {
        let meta = self.account_meta(name);
        crate::account_plan::resolve(harness, meta.plan_override.as_deref(), meta.plan.as_deref())
            .and_then(|p| p.session_limit)
    }

    /// 자동 배정의 요금제 배수. None은 요금제 미확인이며 배정에서는 1배로 본다.
    pub(crate) fn account_plan_weight(&self, name: &str, harness: &str) -> Option<f64> {
        let meta = self.account_meta(name);
        crate::account_plan::weight(harness, meta.plan_override.as_deref(), meta.plan.as_deref())
    }

    pub(crate) fn account_coordinator_reserve(&self, name: &str, harness: &str) -> f64 {
        let meta = self.account_meta(name);
        crate::account_plan::coordinator_reserve(harness, meta.plan_override.as_deref(), meta.plan.as_deref())
    }

    pub(crate) fn account_meta(&self, name: &str) -> AccountMeta {
        self.db
            .meta(&key(name))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .and_then(|s| {
                serde_json::from_str(&s)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .unwrap_or_default()
    }

    pub(crate) fn store_account_meta(&self, name: &str, meta: &AccountMeta) -> Result<(), aaa_proto::ErrorBody> {
        let text = serde_json::to_string(meta).map_err(|error| {
            aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::Internal, format!("계정 기록 직렬화 실패: {error}"))
        })?;
        self.db.set_meta(&key(name), &text).map_err(|error| {
            aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::Internal, format!("계정 기록 저장 실패: {error:#}"))
        })?;
        self.accounts_bump();
        Ok(())
    }

    fn save_account_meta(&self, name: &str, meta: &AccountMeta) {
        if let Err(error) = self.store_account_meta(name, meta) {
            tracing::warn!(account = %name, "계정 기록을 쓰지 못했다: {}", error.message);
        }
    }

    pub(crate) fn account_email(
        &self,
        name: &str,
        value: &str,
        actor: &str,
    ) -> Result<aaa_proto::FrontResponse, aaa_proto::ErrorBody> {
        let email = validated_email(value)?;
        let jobs = self.accounts.logins.lock().unwrap();
        if jobs.values().any(|job| {
            let view = job.view.lock().unwrap();
            view.account == name && matches!(view.state.as_str(), "waiting" | "mismatch")
        }) {
            return Err(aaa_proto::ErrorBody::new(
                aaa_proto::ErrorCode::Conflict,
                "진행 중인 인증을 먼저 마치거나 취소하세요",
            ));
        }
        if !self.accounts.list()?.iter().any(|a| a.name == name) {
            return Err(aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::BadRequest, "계정을 찾지 못했습니다"));
        }
        let _gate = self.account_meta_gate.lock().unwrap();
        let mut meta = self.account_meta(name);
        meta.email = Some(email);
        self.store_account_meta(name, &meta)?;
        self.db.audit(actor, "account_email", name);
        Ok(aaa_proto::FrontResponse::Done)
    }

    /// 목록에 메일·시각·상태를 더한다. 메일은 부른 쪽이 보일 수 있을 때만 남긴다
    pub(crate) fn account_views(&self, show_email: bool) -> Result<Vec<AccountView>, aaa_proto::ErrorBody> {
        let now = util::now_ms();
        let mut out = self.accounts.list()?;
        for v in &mut out {
            let m = self.account_meta(&v.name);
            v.health = Some(health(&m, now));
            v.email = m.email;
            v.plan = m
                .plan_override
                .as_deref()
                .and_then(|id| {
                    crate::account_plan::options().into_iter().find(|p| p.id == id && p.harness == v.harness)
                })
                .map(|p| p.label)
                .or_else(|| m.plan.clone());
            v.detected_plan = m.plan;
            v.plan_override = m.plan_override;
            v.first_auth_ms = m.first_auth_ms;
            v.last_auth_ms = m.last_auth_ms;
            v.expires_ms = m.expires_ms;
            v.token_expires_ms = m.token_expires_ms;
            v.refreshed_ms = m.refreshed_ms;
            v.refresh_failure = m.refresh_failure.clone().map(|mut f| {
                f.threads = m.stalled.clone();
                f
            });
        }
        if !show_email {
            hide_email(&mut out);
        }
        Ok(out)
    }

    /// 로그인(또는 메일을 바꾼 다시 로그인)이 금고에 들어갔다. 메일을 모르면 아는 것을 남기고, 메일이 바뀌거나 처음 기록이면
    /// 최초 인증 시각을 지금으로 한다(사용자 답 3A: 다시 로그인하면 모두 채운다)
    pub(crate) fn account_logged_in(
        &self,
        name: &str,
        id: &Identity,
        switched: bool,
    ) -> Result<(), aaa_proto::ErrorBody> {
        let now = util::now_ms();
        let gate = self.account_meta_gate.lock().unwrap();
        let mut m = self.account_meta(name);
        if id.email.is_some() {
            m.email = id.email.clone();
        }
        if id.plan.is_some() || switched {
            m.plan = id.plan.clone();
        }
        if switched || m.first_auth_ms.is_none() {
            m.first_auth_ms = Some(now);
        }
        m.last_auth_ms = Some(now);
        m.expires_ms = id.expires_ms;
        m.rejected_ms = None;
        // 새 인증이다: 옛 갱신 실패는 지우고(멈춘 thread는 회복 알림에 남긴다) 만료 예정을 새 인증에서 읽는다
        m.refresh_failure = None;
        m.token_expires_ms = self.accounts.credential(name).ok().and_then(|c| access_expires_ms(&c.secret.0));
        self.store_account_meta(name, &m)?;
        drop(gate);
        self.account_health_pass();
        Ok(())
    }

    /// Codex 갱신 결과(쓰임으로 본 상태). 거부면 끊김, 성공이면 끊김·갱신 실패를 지우고 plan·비어 있는 메일을 새 id_token에서 채운다.
    /// `version`은 갱신을 청한 판이다: 실패는 기록하는 지금도 금고가 그 판일 때만 이 계정의 실패다(그 사이 다시 로그인했으면 옛 인증의 것이다).
    /// `thread`는 갱신을 청한 pane의 thread(pane 요청일 때). 실패면 그 pane은 멈춘다
    pub(crate) fn account_refresh_seen(
        &self,
        name: &str,
        outcome: &crate::accounts::RefreshOutcome,
        version: u64,
        thread: Option<&str>,
    ) {
        use crate::accounts::RefreshOutcome;
        let gate = self.account_meta_gate.lock().unwrap();
        let mut m = self.account_meta(name);
        let before = m.clone();
        let now = util::now_ms();
        match outcome {
            RefreshOutcome::Served => return,
            RefreshOutcome::Renewed => {
                let credential = self.accounts.credential(name).ok();
                let id = credential.as_ref().map(|c| codex_identity(&c.secret.0)).unwrap_or_default();
                m.rejected_ms = None;
                m.refresh_failure = None;
                m.refreshed_ms = Some(now);
                m.token_expires_ms = credential.as_ref().and_then(|c| access_expires_ms(&c.secret.0));
                if m.email.is_none() {
                    m.email = id.email;
                }
                if id.plan.is_some() {
                    m.plan = id.plan;
                }
            }
            RefreshOutcome::Rejected(f) | RefreshOutcome::Failed(f) => {
                let current = self
                    .accounts
                    .list()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .and_then(|l| l.into_iter().find(|a| a.name == name));
                if current.is_none_or(|a| a.version != version) {
                    return;
                }
                let rejected = matches!(outcome, RefreshOutcome::Rejected(_));
                if rejected && m.rejected_ms.is_none() {
                    tracing::warn!(account = %name, "제공자가 계정 인증 갱신을 거부했다: 끊김");
                    m.rejected_ms = Some(now);
                }
                let mut failure = m.refresh_failure.take().unwrap_or_else(|| aaa_proto::AccountRefreshFailure {
                    cause: String::new(),
                    stage: String::new(),
                    status: None,
                    code: None,
                    relogin: false,
                    since_ms: now,
                    last_ms: now,
                    attempts: 0,
                    threads: Vec::new(),
                });
                failure.cause = f.cause.into();
                failure.stage = f.stage.into();
                failure.status = f.status;
                failure.code = f.code.clone();
                failure.relogin = rejected;
                failure.last_ms = now;
                failure.attempts = failure.attempts.saturating_add(1);
                m.refresh_failure = Some(failure);
                let label = thread.map(|t| if t.starts_with("c-") { "coordinator" } else { t });
                if let Some(t) = label.filter(|t| !m.stalled.iter().any(|s| s == t)) {
                    m.stalled.push(t.to_string());
                }
            }
        }
        if m == before {
            return;
        }
        self.save_account_meta(name, &m);
        drop(gate);
        self.account_health_pass();
    }

    /// host 선제 갱신(t-1688 답 1B): Codex 금고 인증의 access token 만료가 하루 안이면 host가 미리 갱신한다. 그러면 도는 pane은
    /// 자기 token 만료 5분 전 갱신 요청에서 제공자에게 묻지 않고 새 인증을 받는다. 실패하면 5분 뒤 다시 하고, 제공자가 거부하면
    /// 다시 로그인할 때까지 멈춘다(CLIProxyAPI auto_refresh_loop의 일정·멈춤 규칙을 따른다). 목록의 만료 예정도 여기서 맞춘다
    pub(crate) fn account_refresh_pass(self: &std::sync::Arc<Self>) {
        let Ok(list) = self
            .accounts
            .list()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let now = util::now_ms();
        for a in list.into_iter().filter(|a| a.harness == "codex") {
            let expires = self.accounts.credential(&a.name).ok().and_then(|c| access_expires_ms(&c.secret.0));
            let m = {
                let _gate = self.account_meta_gate.lock().unwrap();
                let mut m = self.account_meta(&a.name);
                if m.token_expires_ms != expires {
                    m.token_expires_ms = expires;
                    self.save_account_meta(&a.name, &m);
                }
                m
            };
            let Some(expires) = expires else { continue };
            if m.rejected_ms.is_some()
                || expires.saturating_sub(REFRESH_LEAD_MS) > now
                || m.refresh_failure.as_ref().is_some_and(|f| now < f.last_ms.saturating_add(REFRESH_RETRY_MS))
                || !self.watch_claim("account-refresh", &a.name, std::time::Duration::ZERO)
            {
                continue;
            }
            let host = self.clone();
            let name = a.name;
            tokio::spawn(async move {
                let _account_read = host.account_rename_gate.read().await;
                let (version, outcome) = host.accounts.renew_current(&name).await;
                host.account_refresh_seen(&name, &outcome, version, None);
                host.watch_finished("account-refresh", &name);
            });
        }
    }

    pub(crate) fn account_meta_remove(&self, name: &str) {
        let _gate = self.account_meta_gate.lock().unwrap();
        aaa_common::diagnostic::ignored(self.db.del_meta(&key(name)), "보조 작업 실패 뒤 기존 처리를 계속한다");
        if let Ok(project) = self.instance_project() {
            self.inbox_finish_where(&project, |i| i.item_kind == "account" && i.subject == name);
        }
        self.accounts_bump();
    }

    /// 이 기록 전에 등록한 계정(사용자 답 3A): 저장된 Codex 인증에서 메일·plan만 채운다. 날짜는 모른다
    pub(crate) fn account_meta_backfill(&self) {
        let Ok(list) = self
            .accounts
            .list()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let _gate = self.account_meta_gate.lock().unwrap();
        for a in list {
            if a.harness != "codex"
                || self
                    .db
                    .meta(&key(&a.name))
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .flatten()
                    .is_some()
            {
                continue;
            }
            let Ok(c) = self.accounts.credential(&a.name).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            let id = codex_identity(&c.secret.0);
            if id.email.is_some() || id.plan.is_some() {
                self.save_account_meta(&a.name, &AccountMeta { email: id.email, plan: id.plan, ..Default::default() });
            }
        }
    }

    /// 상태가 바뀐 계정을 알림함에 낸다(확인 필요). 같은 상태는 한 번만, 정상으로 돌아오면 그 계정 항목을 끝낸다.
    /// 알림에는 별칭·harness만 싣는다(메일 없음)
    pub(crate) fn account_health_pass(&self) {
        let Ok(list) = self
            .accounts
            .list()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let now = util::now_ms();
        let project = self
            .instance_project()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok();
        for a in list {
            let _gate = self.account_meta_gate.lock().unwrap();
            let mut m = self.account_meta(&a.name);
            let h = health(&m, now);
            let want = (h != AccountHealth::Ok).then_some(h);
            let want_refresh = refresh_alert(&m);
            let failing = m.rejected_ms.is_some() || m.refresh_failure.is_some();
            if m.alerted == want && m.refresh_alerted == want_refresh {
                // 알리기 전에 회복했다: 멈춘 thread 기록만 지운다
                if !failing && !m.stalled.is_empty() {
                    m.stalled.clear();
                    self.save_account_meta(&a.name, &m);
                }
                continue;
            }
            let Some(p) = &project else {
                // 알릴 프로젝트가 없으면 다음 pass에 다시 본다
                continue;
            };
            let was_failing = m.refresh_alerted || m.alerted == Some(AccountHealth::Rejected);
            self.inbox_finish_where(p, |i| i.item_kind == "account" && i.subject == a.name);
            let login = format!(
                "웹 설정 → 계정에서 「다시 로그인」하거나 `aaa account login {} --harness {}`",
                a.name, a.harness
            );
            let threads = if m.stalled.is_empty() {
                String::new()
            } else {
                format!(" 멈춘 thread: {}.", thread_list(&m.stalled))
            };
            if let Some(h) = want {
                let (event, why) = match h {
                    AccountHealth::Expiring => ("곧 만료", "인증 만료가 7일 안이다".to_string()),
                    AccountHealth::Expired => ("끊김", "인증 만료 시각이 지났다".to_string()),
                    _ => (
                        "끊김",
                        match &m.refresh_failure {
                            Some(f) => format!("제공자가 인증 갱신을 거부했다({})", f.reason()),
                            None => "제공자가 인증 갱신을 거부했다".to_string(),
                        },
                    ),
                };
                let summary = format!("확인 필요: 계정 {}({}) {why}. {login}.{threads}", a.name, a.harness);
                self.inbox_add(p, "account", &a.name, event, &summary);
            }
            if want_refresh && let Some(f) = &m.refresh_failure {
                let todo = if f.cause == "aaa" {
                    "다시 로그인으로 풀리지 않을 수 있다. host.log의 「계정 인증 갱신 실패」를 본다"
                } else {
                    "aaa가 5분마다 다시 시도한다. 이어지면 망·OpenAI 상태를 본다"
                };
                let summary = format!(
                    "확인 필요: 계정 {}({}) 인증 갱신 실패: {}, {}번째. {todo}.{threads}",
                    a.name,
                    a.harness,
                    f.reason(),
                    f.attempts
                );
                self.inbox_add(p, "account", &a.name, "갱신 실패", &summary);
            }
            if want.is_none() && !want_refresh && !failing {
                if was_failing && !m.stalled.is_empty() {
                    // 회복 뒤 이어 보내기는 coordinator가 한다(t-1688 답 2A): aaa는 멈췄던 thread만 알린다
                    let summary = format!(
                        "회복: 계정 {}({}) 인증이 회복됐다. 인증 실패로 멈췄던 thread: {}. 「이어서」를 보내면 이어 간다",
                        a.name,
                        a.harness,
                        thread_list(&m.stalled)
                    );
                    self.inbox_add(p, "account", &a.name, "회복", &summary);
                }
                m.stalled.clear();
            }
            m.alerted = want;
            m.refresh_alerted = want_refresh;
            self.save_account_meta(&a.name, &m);
        }
    }
}

#[cfg(test)]
mod tests;
