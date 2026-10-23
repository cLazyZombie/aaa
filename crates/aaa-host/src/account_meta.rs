//! 계정 메일·plan·인증 시각·상태(관리 GUI W5a, plan §8.351). 금고 파일 모양은 바꾸지 않고 host DB meta `account-meta:<이름>`에
//! 둔다(키 관리는 별도 일이다). 메일·plan은 조작 기기와 host의 세션 밖 CLI에만 보인다: 감사 기록·알림함·로그에는 별칭만 남긴다.
//! 「끊김」은 제공자가 Codex 인증 갱신을 거부했거나 알려진 만료 시각이 지난 것뿐이다. 제공자에게 따로 묻지 않는다.
use std::path::Path;

use aaa_common::util;
use aaa_proto::{AccountHealth, AccountView, DeviceRole};
use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::Host;

/// 만료 이만큼 전부터 「곧 만료」
pub(crate) const EXPIRING_MS: u64 = 7 * 24 * 3600 * 1000;
/// 공식 CLI 결과에서 읽는 파일 상한(설정 파일이다)
const MAX_FILE: u64 = 1 << 20;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AccountMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,
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

/// 메일 꼴만 받는다(공백·제어 문자·지나친 길이 없음)
fn email(v: Option<&serde_json::Value>) -> Option<String> {
    let s = v?.as_str()?.trim();
    let ok = s.len() <= 254
        && s.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.'))
        && !s.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then(|| s.to_string())
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
    }
}

fn key(name: &str) -> String {
    format!("account-meta:{name}")
}

impl Host {
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

    fn save_account_meta(&self, name: &str, m: &AccountMeta) {
        match serde_json::to_string(m) {
            Ok(text) => {
                if let Err(e) = self.db.set_meta(&key(name), &text) {
                    tracing::warn!(account = %name, "계정 기록을 쓰지 못했다: {e:#}");
                }
            }
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                tracing::warn!(account = %name, "계정 기록을 만들지 못했다")
            }
        }
        self.accounts_bump();
    }

    /// 목록에 메일·시각·상태를 더한다. 메일은 부른 쪽이 보일 수 있을 때만 남긴다
    pub(crate) fn account_views(&self, show_email: bool) -> Result<Vec<AccountView>, aaa_proto::ErrorBody> {
        let now = util::now_ms();
        let mut out = self.accounts.list()?;
        for v in &mut out {
            let m = self.account_meta(&v.name);
            v.health = Some(health(&m, now));
            v.email = m.email;
            v.plan = m.plan;
            v.first_auth_ms = m.first_auth_ms;
            v.last_auth_ms = m.last_auth_ms;
            v.expires_ms = m.expires_ms;
        }
        if !show_email {
            hide_email(&mut out);
        }
        Ok(out)
    }

    /// 로그인(또는 메일을 바꾼 다시 로그인)이 금고에 들어갔다. 메일을 모르면 아는 것을 남기고, 메일이 바뀌거나 처음 기록이면
    /// 최초 인증 시각을 지금으로 한다(사용자 답 3A: 다시 로그인하면 모두 채운다)
    pub(crate) fn account_logged_in(&self, name: &str, id: &Identity, switched: bool) {
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
        self.save_account_meta(name, &m);
        drop(gate);
        self.account_health_pass();
    }

    /// Codex 갱신 결과(쓰임으로 본 상태). 거부면 끊김, 성공이면 끊김을 지우고 새 id_token의 plan·메일을 읽는다
    /// `version`은 갱신을 청한 판이다: 거부는 기록하는 지금도 금고가 그 판일 때만 끊김이다(그 사이 다시 로그인했으면 옛 인증의 거부다)
    pub(crate) fn account_refresh_seen(&self, name: &str, outcome: crate::accounts::RefreshOutcome, version: u64) {
        use crate::accounts::RefreshOutcome;
        let gate = self.account_meta_gate.lock().unwrap();
        let mut m = self.account_meta(name);
        match outcome {
            RefreshOutcome::Rejected => {
                let current = self
                    .accounts
                    .list()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .and_then(|l| l.into_iter().find(|a| a.name == name));
                if m.rejected_ms.is_some() || current.is_none_or(|a| a.version != version) {
                    return;
                }
                tracing::warn!(account = %name, "제공자가 계정 인증 갱신을 거부했다: 끊김");
                m.rejected_ms = Some(util::now_ms());
            }
            RefreshOutcome::Renewed => {
                let id = self.accounts.credential(name).map(|c| codex_identity(&c.secret.0)).unwrap_or_default();
                let before = m.clone();
                m.rejected_ms = None;
                if id.email.is_some() {
                    m.email = id.email;
                }
                if id.plan.is_some() {
                    m.plan = id.plan;
                }
                if m == before {
                    return;
                }
            }
            RefreshOutcome::Served | RefreshOutcome::Failed => return,
        }
        self.save_account_meta(name, &m);
        drop(gate);
        self.account_health_pass();
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
            if m.alerted == want {
                continue;
            }
            if let Some(p) = &project {
                self.inbox_finish_where(p, |i| i.item_kind == "account" && i.subject == a.name);
                if let Some(h) = want {
                    let (event, why) = match h {
                        AccountHealth::Expiring => ("곧 만료", "인증 만료가 7일 안이다"),
                        AccountHealth::Expired => ("끊김", "인증 만료 시각이 지났다"),
                        _ => ("끊김", "제공자가 인증 갱신을 거부했다"),
                    };
                    let summary = format!(
                        "확인 필요: 계정 {}({}) {why}. 웹 설정 → 계정에서 「다시 로그인」하거나 `aaa account login {} --harness {}`",
                        a.name, a.harness, a.name, a.harness
                    );
                    self.inbox_add(p, "account", &a.name, event, &summary);
                }
            } else if want.is_some() {
                // 알릴 프로젝트가 없으면 다음 pass에 다시 본다
                continue;
            }
            m.alerted = want;
            self.save_account_meta(&a.name, &m);
        }
    }
}

#[cfg(test)]
mod tests;
