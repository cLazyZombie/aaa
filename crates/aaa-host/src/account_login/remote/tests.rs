use super::*;

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
    use aaa_proto::{DeviceRole, FrontRequest};
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
        let mut host = Host::open(cfg, root.join("host")).unwrap();
        Arc::get_mut(&mut host).unwrap().usage = crate::usage::UsageService::new(crate::usage::UsageContext {
            home: root.join("unrelated-home"),
            env: Default::default(),
            darwin: true,
            origin: Some("http://127.0.0.1:0".into()),
            keychain: Arc::new(|_, _| crate::usage::KeychainRead::Missing),
            run: Arc::new(|_| None),
            now: Arc::new(util::now_ms),
        });
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

    // 필요: 새 조회 로그인·코드 제출·정리·금고 결속이 기존 실행 인증을 바꾸지 않는다.
    // 실제 실패와 같은 종료 코드 1을 가짜 CLI로 재현한다. 공급자 원문은 어떤 응답에도 싣지 않는다.
    #[tokio::test]
    async fn usage_cli_failure_is_not_relabelled_as_identity_or_vault_failure() {
        let l = lab();
        l.host.register_account("work", "claude", &aaa_test_support::token("failure-execution")).unwrap();
        let generation = l.host.accounts.generation("work").unwrap();
        let secret = aaa_test_support::token("untrusted-provider-error");
        for (text, expected) in [
            ("Token exchange failed (403): Forbidden", "Claude 인증 요청이 거부됐습니다"),
            ("unrecognized provider error", "공식 Claude CLI 인증이 실패했습니다"),
        ] {
            let program = script(
                &l,
                &format!("printf '%s\\n' '{URL}'\nIFS= read -r code\nprintf '%s\\n' '{text} {secret}' >&2\nexit 1"),
            );
            let FrontResponse::AccountLogin { login } = l
                .host
                .start_remote_with_usage("work", "claude", "device:owner", program, true, Some("me@example.com"))
                .unwrap()
            else {
                panic!()
            };
            prompt(&l, &login.id).await;
            l.host.account_login_code(&login.id, SecretText(code()), "device:owner").unwrap();
            let result = finish(&l, &login.id).await;
            assert_eq!(result.state, "failed");
            assert_eq!(result.error.as_deref(), Some(expected));
            assert!(!serde_json::to_string(&result).unwrap().contains(&secret));
            assert!(!l.host.state.join("usage-accounts").exists(), "CLI 인증 실패 때 금고를 생성하지 않는다");
            assert_eq!(l.host.accounts.generation("work").unwrap(), generation);
            cleaned(&l, &login.id);
        }
    }

    #[tokio::test]
    async fn usage_completion_distinguishes_read_identity_cleanup_account_and_vault() {
        let l = lab();
        l.host.register_account("work", "claude", &aaa_test_support::token("stage-execution")).unwrap();
        let context = l.host.prepare_usage_login("work", Some("me@example.com")).unwrap();
        let view = || {
            serde_json::from_value::<AccountLoginView>(serde_json::json!({"id":"test-login","account":"work","harness":"claude","state":"waiting","usage_only":true,"expect_email":"me@example.com"})).unwrap()
        };
        let identity = || Identity { email: Some("me@example.com".into()), ..Default::default() };
        let mut v = view();
        complete_usage(
            &l.host,
            &mut v,
            &context,
            UsageCompletion {
                secret: Err(error("조회 인증 읽기 실패")),
                identity: Identity::default(),
                cli_error: None,
                cleaned: true,
                cancelled: false,
            },
        );
        assert_eq!(v.error.as_deref(), Some("조회 인증 읽기 실패"));
        let mut v = view();
        complete_usage(
            &l.host,
            &mut v,
            &context,
            UsageCompletion {
                secret: Ok("test".into()),
                identity: identity(),
                cli_error: None,
                cleaned: false,
                cancelled: false,
            },
        );
        assert_eq!(v.error.as_deref(), Some("조회 인증 임시 자료 정리를 확인하지 못했습니다"));
        let mut v = view();
        complete_usage(
            &l.host,
            &mut v,
            &context,
            UsageCompletion {
                secret: Ok("test".into()),
                identity: Identity::default(),
                cli_error: None,
                cleaned: true,
                cancelled: false,
            },
        );
        assert_eq!(v.error.as_deref(), Some("공식 Claude CLI의 로그인 계정을 확인하지 못했습니다"));
        std::fs::rename(l.host.state.join("secrets.key"), l.host.state.join("held.key")).unwrap();
        let mut v = view();
        complete_usage(
            &l.host,
            &mut v,
            &context,
            UsageCompletion {
                secret: Ok("test".into()),
                identity: identity(),
                cli_error: None,
                cleaned: true,
                cancelled: false,
            },
        );
        assert_eq!(v.error.as_deref(), Some("조회 인증을 금고에 저장하지 못했습니다"));
        std::fs::rename(l.host.state.join("held.key"), l.host.state.join("secrets.key")).unwrap();
        l.host.accounts.import("work", "claude", &aaa_test_support::token("new-stage-execution")).unwrap();
        let mut v = view();
        complete_usage(
            &l.host,
            &mut v,
            &context,
            UsageCompletion {
                secret: Ok("test".into()),
                identity: identity(),
                cli_error: None,
                cleaned: true,
                cancelled: false,
            },
        );
        assert_eq!(v.error.as_deref(), Some("인증 중 대상 계정이 바뀌었습니다"));
    }

    #[tokio::test]
    async fn unknown_execution_identity_requires_owner_expected_email_and_preserves_metadata() {
        let l = lab();
        l.host.register_account("work", "claude", &aaa_test_support::token("unknown-execution")).unwrap();
        let generation = l.host.accounts.generation("work").unwrap();
        assert!(l.host.account_usage_login_remote("work", "device:owner", None).is_err());
        let raw = serde_json::json!({"claudeAiOauth":{"accessToken":aaa_test_support::token("unknown-query-access"),"refreshToken":aaa_test_support::token("unknown-query-refresh"),"expiresAt":util::now_ms()+3_600_000,"scopes":["user:profile"]}}).to_string();
        for expected in ["other@example.com", "me@example.com"] {
            let program = script(
                &l,
                &format!(
                    "printf '%s\\n' '{URL}'\nIFS= read -r code\nprintf '%s' '{raw}' > \"$CLAUDE_CONFIG_DIR/.credentials.json\"\nprintf '%s' '{{\"oauthAccount\":{{\"emailAddress\":\"me@example.com\"}}}}' > \"$CLAUDE_CONFIG_DIR/.claude.json\""
                ),
            );
            let FrontResponse::AccountLogin { login } = l
                .host
                .start_remote_with_usage("work", "claude", "device:owner", program, true, Some(expected))
                .unwrap()
            else {
                panic!()
            };
            prompt(&l, &login.id).await;
            l.host.account_login_code(&login.id, SecretText(code()), "device:owner").unwrap();
            assert_eq!(
                finish(&l, &login.id).await.state,
                if expected == "me@example.com" { "succeeded" } else { "failed" }
            );
            assert_eq!(l.host.accounts.generation("work").unwrap(), generation);
            assert!(l.host.account_meta("work").email.is_none(), "조회 연결이 실행 인증 신원을 단정해 덮지 않는다");
            cleaned(&l, &login.id);
        }
    }

    #[tokio::test]
    async fn usage_login_keeps_execution_and_rejects_other_unknown_and_changed_accounts() {
        let l = lab();
        l.host.register_account("work", "claude", &aaa_test_support::token("execution-owned")).unwrap();
        let identity = Identity { email: Some("me@example.com".into()), ..Default::default() };
        l.host.account_logged_in("work", &identity, false);
        let execution = l.host.accounts.credential("work").unwrap();
        let generation = l.host.accounts.generation("work").unwrap();
        let raw = serde_json::json!({"claudeAiOauth":{"accessToken":aaa_test_support::token("query-access"),
            "refreshToken":aaa_test_support::token("query-refresh"), "expiresAt":util::now_ms()+3_600_000,
            "scopes":["user:profile","user:inference"]}})
        .to_string();
        for email in ["me@example.com", "other@example.com", "", "me@example.com"] {
            let program = script(
                &l,
                &format!(
                    "[ \"$*\" = 'auth login --claudeai' ]\nprintf '%s\\n' '{URL}'\nIFS= read -r code\nprintf '%s' '{raw}' > \"$CLAUDE_CONFIG_DIR/.credentials.json\"\nprintf '%s' '{{\"oauthAccount\":{{\"emailAddress\":\"{email}\"}}}}' > \"$CLAUDE_CONFIG_DIR/.claude.json\""
                ),
            );
            let FrontResponse::AccountLogin { login } =
                l.host.start_remote_with_usage("work", "claude", "device:owner", program, true, None).unwrap()
            else {
                panic!()
            };
            assert_eq!(login.usage_only, Some(true));
            assert_eq!(prompt(&l, &login.id).await.kind, "paste_code");
            if email == "me@example.com" && l.host.accounts.usage_credential(&generation).is_ok() {
                l.host.accounts.import("work", "claude", &aaa_test_support::token("new-execution-owned")).unwrap();
            }
            l.host.account_login_code(&login.id, SecretText(code()), "device:owner").unwrap();
            let result = finish(&l, &login.id).await;
            if email == "me@example.com" && l.host.accounts.generation("work").unwrap() == generation {
                assert_eq!(result.state, "succeeded");
                assert!(l.host.accounts.usage_credential(&generation).is_ok());
            } else {
                assert_eq!(result.state, "failed");
            }
            assert_ne!(result.state, "mismatch", "조회 로그인에는 실행 인증 교체 선택이 없다");
            if l.host.accounts.generation("work").unwrap() == generation {
                assert_eq!(l.host.accounts.credential("work").unwrap().secret.0, execution.secret.0);
            }
            cleaned(&l, &login.id);
        }
        assert_ne!(l.host.accounts.generation("work").unwrap(), generation);
        assert!(l.host.accounts.usage_credential(&generation).is_err());
        assert_eq!(execution.secret.0, aaa_test_support::token("execution-owned"));
    }

    #[tokio::test]
    async fn usage_login_requires_identity_and_cancel_leaves_execution_unchanged() {
        let l = lab();
        l.host.register_account("work", "claude", &aaa_test_support::token("execution-cancel")).unwrap();
        assert!(l.host.account_usage_login_remote("work", "device:owner", None).is_err());
        l.host.account_logged_in(
            "work",
            &Identity { email: Some("me@example.com".into()), ..Default::default() },
            false,
        );
        let generation = l.host.accounts.generation("work").unwrap();
        let FrontResponse::AccountLogin { login } =
            l.host.start_remote_with_usage("work", "claude", "device:owner", claude_script(&l), true, None).unwrap()
        else {
            panic!()
        };
        prompt(&l, &login.id).await;
        l.host.account_login_cancel(&login.id, "device:owner").await.unwrap();
        assert_eq!(finish(&l, &login.id).await.state, "cancelled");
        assert_eq!(l.host.accounts.generation("work").unwrap(), generation);
        assert!(l.host.accounts.usage_credential(&generation).is_err());
        cleaned(&l, &login.id);
        assert_eq!(
            l.host
                .call(
                    DeviceRole::Controller,
                    "device:owner",
                    FrontRequest::AccountUsageLogin { name: "work".into(), remote: false, email: None }
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert_eq!(
            l.host
                .call(
                    DeviceRole::Reader,
                    "device:reader",
                    FrontRequest::AccountUsageLogin { name: "work".into(), remote: true, email: None }
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
        assert_eq!(
            l.host
                .call(
                    DeviceRole::Local,
                    "local:session-test",
                    FrontRequest::AccountUsageLogin { name: "work".into(), remote: true, email: None }
                )
                .await
                .unwrap_err()
                .code,
            ErrorCode::Forbidden
        );
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
