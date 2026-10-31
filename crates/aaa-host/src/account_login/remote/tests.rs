use super::*;
use aaa_proto::{DeviceRole, FrontRequest};

const URL: &str = "https://claude.com/cai/oauth/authorize?code=true&state=fixture-state&code_challenge=fixture-challenge&code_challenge_method=S256&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback";

#[test]
fn manual_link_requires_complete_official_pkce_url() {
    assert!(claude_url(URL.as_bytes()).is_none(), "미완성 chunk는 기다린다");
    for suffix in ["\n", "\x07", "\x1b\\", "\u{3000}"] {
        assert_eq!(claude_url(format!("\x1b]8;;{URL}{suffix}잘린 주소\x1b]8;;\x07").as_bytes()).as_deref(), Some(URL));
    }
    for bad in [
        URL.replace("claude.com", "example.com"),
        URL.replace("claude.com/cai", "claude.com@example.com/cai"),
        URL.replace("code=true", "code=false"),
        URL.replace("S256", "plain"),
        URL.replace("platform.claude.com", "localhost"),
        format!("{URL}&state=other"),
        format!("{URL}&redirect_uri=other"),
    ] {
        assert!(claude_url(format!("{bad}\n").as_bytes()).is_none());
    }
    assert_eq!(claude_url(format!("https://bad.invalid/\n{URL}\n").as_bytes()).as_deref(), Some(URL));
}

#[cfg(unix)]
mod process {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    struct Lab {
        root: std::path::PathBuf,
        host: Arc<Host>,
    }
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn lab() -> Lab {
        let root = std::env::temp_dir().join(format!("aaa-remote-login-{}", util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let host = Host::open(cfg, root.join("host")).unwrap();
        Lab { root, host }
    }
    fn script(l: &Lab, body: &str) -> std::path::PathBuf {
        let p = l.root.join(util::new_id());
        std::fs::write(&p, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
    fn start(l: &Lab, harness: &str, program: std::path::PathBuf) -> String {
        let FrontResponse::AccountLogin { login } =
            l.host.start_remote("work", harness, "device:owner", program).unwrap()
        else {
            panic!()
        };
        login.id
    }
    async fn prompt(l: &Lab, id: &str) -> AccountLoginPrompt {
        let mut changes = l.host.accounts_changed.subscribe();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let FrontResponse::AccountLoginInteraction { prompt: Some(prompt), .. } =
                    l.host.account_login_interaction(Some(id), "device:owner").unwrap()
                {
                    break prompt;
                }
                changes.changed().await.unwrap();
            }
        })
        .await
        .expect("인증 프롬프트 사건")
    }
    async fn finish(l: &Lab, id: &str) -> AccountLoginView {
        let done = l.host.accounts.logins.lock().unwrap()[id].finished.clone();
        tokio::time::timeout(Duration::from_secs(3), login_finished(done)).await.unwrap().unwrap();
        let FrontResponse::AccountLogin { login } = l.host.account_login_status(id, "device:owner").unwrap() else {
            panic!()
        };
        login
    }
    fn cleaned(l: &Lab, id: &str) {
        assert!(!l.host.state.join("account-logins").join(id).exists());
    }
    fn code() -> String {
        format!("{}#{}", aaa_test_support::token("manual-code"), aaa_test_support::token("manual-state"))
    }
    fn claude_script(l: &Lab) -> std::path::PathBuf {
        let token = format!("sk-ant-oat01-{}", aaa_test_support::token("remote-claude"));
        script(
            l,
            &format!(
                "printf '%s\\n' '{URL}'\nIFS= read -r code\n[ \"$code\" = '{}' ]\nprintf '%s\\n' '{token}' 'valid for 1 year'",
                code()
            ),
        )
    }

    #[tokio::test]
    async fn claude_code_is_owner_only_one_shot_and_unknown_identity_requires_confirmation() {
        let l = lab();
        l.host.register_account("work", "claude", &format!("sk-ant-oat01-{}", aaa_test_support::token("old"))).unwrap();
        let id = start(&l, "claude", claude_script(&l));
        let p = prompt(&l, &id).await;
        assert_eq!(p.kind, "paste_code");
        assert!(!format!("{p:?}").contains("fixture-state"));
        for req in [
            FrontRequest::AccountLoginInteraction { id: Some(id.clone()) },
            FrontRequest::AccountLoginStatus { id: id.clone() },
            FrontRequest::AccountLoginCancel { id: id.clone() },
            FrontRequest::AccountLoginCode { id: id.clone(), code: SecretText(aaa_test_support::token("other-code")) },
        ] {
            assert_eq!(
                l.host.call(DeviceRole::Controller, "device:other", req.clone()).await.unwrap_err().code,
                ErrorCode::Forbidden
            );
            assert_eq!(
                l.host.call(DeviceRole::Reader, "device:owner", req).await.unwrap_err().code,
                ErrorCode::Forbidden
            );
        }
        assert!(matches!(
            l.host.account_login_interaction(None, "device:other").unwrap(),
            FrontResponse::AccountLoginInteraction { login: None, prompt: None, .. }
        ));
        assert!(l.host.start_remote("another", "claude", "device:owner", claude_script(&l)).is_err());
        assert!(l.host.account_login_code(&id, SecretText(format!("{}\n", code())), "device:owner").is_err());
        l.host.account_login_code(&id, SecretText(code()), "device:owner").unwrap();
        assert!(l.host.account_login_code(&id, SecretText(aaa_test_support::token("again")), "device:owner").is_err());
        assert_eq!(finish(&l, &id).await.state, "mismatch");
        cleaned(&l, &id);
        assert!(l.host.account_login_confirm(&id, true, "device:other").is_err());
        l.host.account_login_confirm(&id, true, "device:owner").unwrap();
        assert_eq!(l.host.accounts.list().unwrap()[0].version, 2);
    }

    #[tokio::test]
    async fn cancellation_and_revocation_reap_children_and_discard_pending_tokens() {
        let l = lab();
        let id = start(&l, "claude", claude_script(&l));
        prompt(&l, &id).await;
        // 만료 시각을 주입해 실제 5분을 기다리지 않고 노출·제출 경계를 검사한다.
        l.host.accounts.logins.lock().unwrap()[&id].remote.as_ref().unwrap().lock().unwrap().deadline =
            std::time::Instant::now();
        assert!(matches!(
            l.host.account_login_interaction(Some(&id), "device:owner").unwrap(),
            FrontResponse::AccountLoginInteraction { prompt: None, .. }
        ));
        assert!(l.host.account_login_code(&id, SecretText(code()), "device:owner").is_err());
        l.host.account_login_cancel(&id, "device:owner").await.unwrap();
        assert_eq!(finish(&l, &id).await.state, "cancelled");
        cleaned(&l, &id);
        assert!(l.host.accounts.list().unwrap().is_empty());
        let id = start(&l, "claude", claude_script(&l));
        prompt(&l, &id).await;
        l.host.revoke_account_logins("owner");
        assert_eq!(finish(&l, &id).await.state, "cancelled");
        cleaned(&l, &id);
        l.host.register_account("work", "claude", &format!("sk-ant-oat01-{}", aaa_test_support::token("old"))).unwrap();
        let id = start(&l, "claude", claude_script(&l));
        prompt(&l, &id).await;
        l.host.account_login_code(&id, SecretText(code()), "device:owner").unwrap();
        assert_eq!(finish(&l, &id).await.state, "mismatch");
        l.host.revoke_account_logins("owner");
        assert!(l.host.account_login_confirm(&id, true, "device:owner").is_err());
        assert_eq!(l.host.accounts.list().unwrap()[0].version, 1);
    }

    #[tokio::test]
    async fn codex_uses_device_rpc_and_commits_only_matching_completion() {
        let l = lab();
        let auth = serde_json::json!({"tokens":{"access_token":aaa_test_support::token("remote-access"),"refresh_token":aaa_test_support::token("remote-refresh")}}).to_string();
        let program = script(
            &l,
            &format!(
                r#"
IFS= read -r init
printf '%s\n' '{{"id":1,"result":{{}}}}'
IFS= read -r initialized
IFS= read -r login
case "$login" in *chatgptDeviceCode*) ;; *) exit 1;; esac
printf '%s\n' '{{"id":2,"result":{{"loginId":"fixture-login","verificationUrl":"https://auth.openai.com/codex/device","userCode":"ABCD-1234"}}}}'
printf '%s\n' '{{"method":"account/login/completed","params":{{"loginId":"other","success":false}}}}'
printf '%s' '{auth}' > "$CODEX_HOME/auth.json"
printf '%s\n' '{{"method":"account/login/completed","params":{{"loginId":"fixture-login","success":true}}}}'
IFS= read -r wait
"#
            ),
        );
        let id = start(&l, "codex", program);
        assert_eq!(finish(&l, &id).await.state, "succeeded");
        cleaned(&l, &id);
        assert_eq!(l.host.accounts.list().unwrap().len(), 1);
        assert!(matches!(
            l.host.account_login_interaction(Some(&id), "device:owner").unwrap(),
            FrontResponse::AccountLoginInteraction { prompt: None, .. }
        ));
    }
    #[tokio::test]
    async fn codex_rejects_untrusted_link_without_exposing_provider_output() {
        let l = lab();
        let program = script(
            &l,
            r#"
IFS= read -r init
printf '%s\n' '{"id":1,"result":{}}'
IFS= read -r initialized
IFS= read -r login
printf '%s\n' '{"id":2,"result":{"loginId":"fixture","verificationUrl":"https://evil.invalid/private-value","userCode":"ABCD"}}'
IFS= read -r wait
"#,
        );
        let id = start(&l, "codex", program);
        let view = finish(&l, &id).await;
        assert_eq!(view.state, "failed");
        assert!(!serde_json::to_string(&view).unwrap().contains("private-value"));
        assert_eq!(view.error.as_deref(), Some("Codex 인증 주소·코드가 올바르지 않습니다"));
        assert!(l.host.accounts.list().unwrap().is_empty());
        cleaned(&l, &id);
    }
}
