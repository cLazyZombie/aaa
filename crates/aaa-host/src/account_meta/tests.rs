use super::*;
use crate::accounts::RefreshOutcome;
use aaa_proto::{ErrorCode, FrontRequest, FrontResponse};
use std::sync::Arc;

const DAY: u64 = 24 * 3600 * 1000;

/// 시험용 Codex auth.json: id_token 본문에 메일·plan(서명은 가짜다)
fn codex_auth(email: &str, plan: &str) -> String {
    let claims = serde_json::json!({"email": email, "https://api.openai.com/auth": {"chatgpt_plan_type": plan}});
    let body = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());
    serde_json::json!({"tokens": {
        "access_token": "test-access-0123456789",
        "refresh_token": "test-refresh-0123456789",
        "id_token": format!("e30.{body}.c2ln"),
    }})
    .to_string()
}

struct Lab {
    root: std::path::PathBuf,
    h: Arc<Host>,
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn lab(project: bool) -> Lab {
    let root = std::env::temp_dir().join(format!("aaa-account-meta-{}", util::new_id()));
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    let h = Host::open(cfg, root.join("host")).unwrap();
    if project {
        h.project_create("demo", Default::default(), "local:cli").unwrap();
    }
    Lab { root, h }
}

fn account_items(h: &Host) -> Vec<aaa_proto::InboxItem> {
    h.db.inbox_list(Some("demo"), true).unwrap().into_iter().filter(|i| i.item_kind == "account").collect()
}

#[test]
fn codex_and_claude_logins_tell_email_plan_and_expiry_only_when_the_cli_does() {
    let id = codex_identity(&codex_auth("work@example.com", "plus"));
    assert_eq!(
        (id.email.as_deref(), id.plan.as_deref(), id.expires_ms),
        (Some("work@example.com"), Some("plus"), None)
    );
    // id_token이 없거나 깨졌으면 모른다
    assert_eq!(codex_identity(r#"{"tokens":{"access_token":"x"}}"#), Identity::default());
    assert_eq!(codex_identity(r#"{"tokens":{"id_token":"a.!!!.b"}}"#), Identity::default());

    let dir = std::env::temp_dir().join(format!("aaa-claude-id-{}", util::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let now = 1_000 * DAY;
    let out = "✓ Long-lived authentication token created successfully!\nYour OAuth token (valid for 1 year):\n";
    // 메일을 남긴 파일이 없으면 메일은 모르고, 유효 기간만 읽는다
    assert_eq!(
        claude_identity(out, &[&dir], now),
        Identity { expires_ms: Some(now + 365 * DAY), ..Default::default() }
    );
    std::fs::write(dir.join(".claude.json"), r#"{"oauthAccount":{"emailAddress":"me@example.com"}}"#).unwrap();
    assert_eq!(claude_identity("valid for 30 days", &[&dir], now).email.as_deref(), Some("me@example.com"));
    assert_eq!(claude_identity("valid for 30 days", &[&dir], now).expires_ms, Some(now + 30 * DAY));
    // 읽지 못하는 기간은 만료를 지어내지 않는다
    assert_eq!(claude_identity("valid for a while", &[], now).expires_ms, None);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn health_is_rejection_first_then_known_expiry() {
    let now = 100 * DAY;
    let at = |e: Option<u64>, r: Option<u64>| {
        health(&AccountMeta { expires_ms: e, rejected_ms: r, ..Default::default() }, now)
    };
    assert_eq!(at(None, None), AccountHealth::Ok, "만료를 모르면 정상");
    assert_eq!(at(Some(now + 8 * DAY), None), AccountHealth::Ok);
    assert_eq!(at(Some(now + 7 * DAY), None), AccountHealth::Expiring);
    assert_eq!(at(Some(now), None), AccountHealth::Expired);
    assert_eq!(at(Some(now + 30 * DAY), Some(now)), AccountHealth::Rejected);
}

#[tokio::test]
async fn people_log_in_from_the_web_but_only_controllers_and_local_cli_see_emails() {
    let lab = lab(false);
    let h = &lab.h;
    h.accounts.import("work", "codex", &codex_auth("work@example.com", "plus")).unwrap();
    h.account_meta_backfill();
    let list = |role, actor: &'static str| {
        let h = h.clone();
        async move {
            match h.call(role, actor, FrontRequest::AccountList).await.unwrap() {
                FrontResponse::Accounts { accounts } => accounts,
                other => panic!("{other:?}"),
            }
        }
    };
    // 이 기록 전에 들어온 Codex 계정도 저장된 인증에서 메일·plan을 채우고, 날짜는 모른다(사용자 답 3A)
    let seen = list(DeviceRole::Controller, "device:phone").await;
    assert_eq!((seen[0].email.as_deref(), seen[0].plan.as_deref()), (Some("work@example.com"), Some("plus")));
    assert_eq!((seen[0].first_auth_ms, seen[0].last_auth_ms, seen[0].health), (None, None, Some(AccountHealth::Ok)));
    assert_eq!(list(DeviceRole::Local, "local:cli").await[0].email.as_deref(), Some("work@example.com"));
    for (role, actor) in [(DeviceRole::Reader, "device:tv"), (DeviceRole::Local, "local:cli@coordinator")] {
        let hidden = list(role, actor).await;
        assert_eq!((hidden[0].email.as_deref(), hidden[0].plan.as_deref()), (None, None), "{actor}");
        assert_eq!(hidden[0].health, Some(AccountHealth::Ok), "상태는 보인다");
    }
    // 로그인·기본은 사람이: 웹 조작 기기는 경계를 지나고(가짜 harness 이름이라 BadRequest), 세션 안·보기 전용은 거부
    let login = FrontRequest::AccountLogin { name: "x".into(), harness: "nope".into() };
    assert_eq!(
        h.call(DeviceRole::Controller, "device:phone", login.clone()).await.unwrap_err().code,
        ErrorCode::BadRequest
    );
    for (role, actor) in [
        (DeviceRole::Reader, "device:tv"),
        (DeviceRole::Local, "local:cli@coordinator"),
        (DeviceRole::Local, "worker:t-0001"),
        (DeviceRole::Controller, "local:cli@coordinator"),
    ] {
        for req in [
            login.clone(),
            FrontRequest::AccountDefault { harness: "codex".into(), account: Some("work".into()) },
            FrontRequest::AccountLoginConfirm { id: "missing".into(), switch: true },
        ] {
            let e = h.call(role, actor, req.clone()).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::Forbidden, "{actor}: {req:?}");
        }
    }
    h.call(
        DeviceRole::Controller,
        "device:phone",
        FrontRequest::AccountDefault { harness: "codex".into(), account: Some("work".into()) },
    )
    .await
    .unwrap();
    assert_eq!(h.default_account("codex").unwrap().as_deref(), Some("work"));
    // 인증 파일을 넣고 빼는 것은 여전히 host 터미널만
    let remove = FrontRequest::AccountRemove { name: "work".into() };
    assert_eq!(h.call(DeviceRole::Controller, "device:phone", remove).await.unwrap_err().code, ErrorCode::Forbidden);
}

#[test]
fn broken_and_expiring_accounts_raise_one_alert_without_email_and_logging_in_clears_it() {
    let lab = lab(true);
    let h = &lab.h;
    h.accounts.import("work", "codex", &codex_auth("work@example.com", "plus")).unwrap();
    h.account_logged_in("work", &codex_identity(&codex_auth("work@example.com", "plus")), false);
    assert!(account_items(h).is_empty(), "정상은 알리지 않는다");
    // 제공자가 갱신을 거부했다 → 끊김 한 번. 망 실패는 끊김이 아니다
    h.account_refresh_seen("work", RefreshOutcome::Failed);
    assert!(account_items(h).is_empty());
    h.account_refresh_seen("work", RefreshOutcome::Rejected);
    h.account_refresh_seen("work", RefreshOutcome::Rejected);
    h.account_health_pass();
    let items = account_items(h);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!((items[0].subject.as_str(), items[0].event.as_str(), items[0].done), ("work", "끊김", false));
    assert!(items[0].summary.starts_with("확인 필요: ") && !items[0].summary.contains('@'), "{}", items[0].summary);
    // 다시 로그인하면 끊김 항목을 끝낸다
    h.account_logged_in("work", &Identity::default(), false);
    let items = account_items(h);
    assert!(items.iter().all(|i| i.done), "{items:?}");
    assert_eq!(h.account_meta("work").email.as_deref(), Some("work@example.com"), "모르는 메일은 옛 값을 남긴다");

    // 알려진 만료가 7일 안이면 「곧 만료」, 지나면 「끊김」으로 바뀐다(옛 항목은 끝낸다)
    // 공개 저장소 검사가 token 꼴 글을 막는다: 가짜 값도 실행 중에 만든다
    h.accounts.import("ci", "claude", &format!("sk-ant-{}-test-only-0123456789", "oat01")).unwrap();
    let soon = util::now_ms() + 3 * DAY;
    h.account_logged_in("ci", &Identity { expires_ms: Some(soon), ..Default::default() }, false);
    let open: Vec<_> = account_items(h).into_iter().filter(|i| !i.done).collect();
    assert_eq!(open.iter().map(|i| (i.subject.as_str(), i.event.as_str())).collect::<Vec<_>>(), [("ci", "곧 만료")]);
    let mut m = h.account_meta("ci");
    m.expires_ms = Some(util::now_ms() - 1);
    h.save_account_meta("ci", &m);
    h.account_health_pass();
    h.account_health_pass();
    let open: Vec<_> = account_items(h).into_iter().filter(|i| !i.done).collect();
    assert_eq!(open.iter().map(|i| (i.subject.as_str(), i.event.as_str())).collect::<Vec<_>>(), [("ci", "끊김")]);
    // 계정을 지우면 그 기록과 항목도 정리한다
    h.account_meta_remove("ci");
    assert!(account_items(h).iter().all(|i| i.done));
    assert_eq!(h.account_meta("ci"), AccountMeta::default());
}

#[test]
fn a_switched_email_restarts_the_first_auth_date() {
    let lab = lab(false);
    let h = &lab.h;
    h.accounts.import("work", "codex", &codex_auth("old@example.com", "plus")).unwrap();
    let mut m = AccountMeta { email: Some("old@example.com".into()), first_auth_ms: Some(5), ..Default::default() };
    h.save_account_meta("work", &m);
    h.account_logged_in("work", &codex_identity(&codex_auth("old@example.com", "pro")), false);
    m = h.account_meta("work");
    assert_eq!((m.first_auth_ms, m.plan.as_deref()), (Some(5), Some("pro")), "같은 메일은 최초 시각을 둔다");
    h.account_logged_in("work", &codex_identity(&codex_auth("new@example.com", "plus")), true);
    m = h.account_meta("work");
    assert_eq!(m.email.as_deref(), Some("new@example.com"));
    assert!(m.first_auth_ms.unwrap() > 5 && m.first_auth_ms == m.last_auth_ms);
}

#[tokio::test]
async fn an_old_version_served_from_the_vault_does_not_clear_a_rejection() {
    let lab = lab(true);
    let h = &lab.h;
    h.accounts.import("work", "codex", &codex_auth("work@example.com", "plus")).unwrap();
    h.accounts.import("work", "codex", &codex_auth("work@example.com", "plus")).unwrap();
    h.account_refresh_seen("work", RefreshOutcome::Rejected);
    // 옛 판(1) 요청에는 제공자에게 묻지 않고 금고의 판 2를 준다: 거부된 인증이 다시 살아난 것이 아니다
    let request = aaa_proto::SecretText(r#"{"grant_type":"refresh_token"}"#.into());
    let (served, outcome) = h.accounts.refresh("work", 1, &request).await;
    assert_eq!((served.version, outcome), (2, RefreshOutcome::Served));
    h.account_refresh_seen("work", outcome);
    assert_eq!(health(&h.account_meta("work"), util::now_ms()), AccountHealth::Rejected);
    assert_eq!(account_items(h).iter().filter(|i| !i.done).count(), 1, "끊김 항목이 그대로 하나");
}
