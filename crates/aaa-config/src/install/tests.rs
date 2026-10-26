use super::*;
fn input() -> (InstallChoices, InstallPlanContext) {
    let v: serde_json::Value = serde_json::from_str(include_str!("../../../../docs/install/agent-input.json")).unwrap();
    (serde_json::from_value(v["choices"].clone()).unwrap(), serde_json::from_value(v["context"].clone()).unwrap())
}
fn error(c: InstallChoices, x: InstallPlanContext) -> String {
    plan(c, x).err().expect("거부해야 함").code
}

#[test]
fn prompt_and_automation_share_canonical_plan() {
    let (question, x) = input();
    let argv: InstallInvocation = serde_json::from_value(serde_json::json!({
        "choices": question, "non_interactive": true, "yes": true, "json_stream": true, "dry_run": true
    }))
    .unwrap();
    let prompted = plan(question, x.clone()).unwrap();
    let automated = plan(argv.choices, x).unwrap();
    assert!(prompted == automated);
    let frozen: InstallPlan = serde_json::from_str(include_str!("../../../../docs/install/agent-plan.json")).unwrap();
    assert!(prompted == frozen);
    assert_eq!(plan_sha256(&prompted), include_str!("../../../../docs/install/agent-plan.sha256").trim());
    let mut explicit = prompted.choices.clone();
    explicit.harness.push(InstallHarness::Codex);
    explicit.expected_os = Some(InstallOs::Macos);
    explicit.version = Some("0.1.2".into());
    assert_eq!(plan_sha256(&plan(explicit, prompted.context.clone()).unwrap()), plan_sha256(&prompted));
    assert!(plan(prompted.choices.clone(), prompted.context.clone()).unwrap() == prompted);
}

#[test]
fn effective_choices_and_target_are_bound_to_plan_hash() {
    let (c, x) = input();
    let p = plan(c.clone(), x.clone()).unwrap();
    let base = plan_sha256(&p);
    // 대상과 실효 선택 변경은 승인 hash를 바꾼다. 기대 hash는 owner 밖에서 계산한다.
    fn mutate(v: &mut serde_json::Value, path: &[String]) {
        if path.is_empty() {
            *v = match v {
                serde_json::Value::String(s) => serde_json::Value::String(format!("{s}x")),
                _ => serde_json::Value::String("changed".into()),
            };
        } else if let Ok(i) = path[0].parse::<usize>() {
            mutate(&mut v[i], &path[1..]);
        } else {
            mutate(&mut v[&path[0]], &path[1..]);
        }
    }
    let mut changed = c.clone();
    changed.name = Some("mac-c".into());
    assert_ne!(plan_sha256(&plan(changed, x.clone()).unwrap()), base);
    for key in ["binary_sha256", "effective_config_sha256", "target_account", "config_dir", "state_dir"] {
        let mut v = serde_json::to_value(&x).unwrap();
        mutate(&mut v, &[key.into()]);
        // digest는 다른 유효값으로 바꾼다
        if key.ends_with("sha256") {
            v[key] = serde_json::json!("e".repeat(64));
        }
        let target = serde_json::from_value(v).unwrap();
        assert_ne!(plan_sha256(&plan(c.clone(), target).unwrap()), base, "{key}");
    }
}

#[test]
fn missing_conflicting_and_unsafe_inputs_fail_without_echoing_values() {
    let (base, x) = input();
    let cases: Vec<(&str, InstallChoices)> = vec![
        ("role_required", InstallChoices { role: None, ..base.clone() }),
        ("name_required", InstallChoices { name: None, ..base.clone() }),
        ("service_required", InstallChoices { service: None, ..base.clone() }),
        ("work_root_required", InstallChoices { work_root: None, ..base.clone() }),
        ("work_storage_required", InstallChoices { work_storage: None, ..base.clone() }),
        ("harness_required", InstallChoices { harness: vec![], ..base.clone() }),
        ("hooks_required", InstallChoices { hooks: None, ..base.clone() }),
        ("host_required", InstallChoices { host: None, ..base.clone() }),
        ("network_required", InstallChoices { network: None, ..base.clone() }),
        ("enroll_required", InstallChoices { enroll: None, ..base.clone() }),
        ("host_options_on_agent", InstallChoices { pairing: Some(InstallPairing::Skip), ..base.clone() }),
        ("work_storage_conflict", InstallChoices { work_mount: Some("/srv".into()), ..base.clone() }),
        (
            "harness_none_conflict",
            InstallChoices { harness: vec![InstallHarness::None, InstallHarness::Codex], ..base.clone() },
        ),
        ("absolute_path_required", InstallChoices { work_root: Some("relative".into()), ..base.clone() }),
        ("unsafe_argument", InstallChoices { name: Some("$(sensitive-example)".into()), ..base.clone() }),
        (
            "env_key_not_allowed",
            InstallChoices {
                harness_env: vec![InstallHarnessEnv {
                    harness: InstallHarness::Codex,
                    key: "API_TOKEN".into(),
                    value: aaa_test_support::token("fake-secret-do-not-echo"),
                }],
                ..base.clone()
            },
        ),
        ("unsupported_service", InstallChoices { service: Some(InstallService::WindowsScm), ..base.clone() }),
        ("platform_mismatch", InstallChoices { expected_os: Some(InstallOs::Linux), ..base.clone() }),
        ("source_digest_required", InstallChoices { from: Some("/opt/new/aaa".into()), ..base.clone() }),
    ];
    for (expected, c) in cases {
        let e = plan(c, x.clone()).err().unwrap();
        assert_eq!(e.code, expected);
        let rendered = format!("{e:?} {e} {}", serde_json::to_string(&e).unwrap());
        assert!(
            !rendered.contains(&aaa_test_support::token("fake-secret-do-not-echo"))
                && !rendered.contains("sensitive-example")
        );
    }
}

#[test]
fn host_choices_and_platform_services_are_explicit() {
    let (_, x) = input();
    let host = InstallChoices {
        role: Some(InstallRole::Host),
        name: Some("mac-a".into()),
        local_agent: Some(InstallToggle::Disabled),
        service: Some(InstallService::Auto),
        project_mode: Some(InstallProjectMode::Skip),
        agent_listen: Some("127.0.0.1:7430".into()),
        web: Some(InstallWeb::Local),
        tailscale_serve: Some(InstallServe::On),
        serve_port: Some(8443),
        pairing: Some(InstallPairing::Controller),
        ..Default::default()
    };
    assert_eq!(plan(host.clone(), x.clone()).unwrap().choices.web_listen.as_deref(), Some("127.0.0.1:7431"));
    assert_eq!(error(InstallChoices { tailscale_serve: None, ..host.clone() }, x.clone()), "serve_choice_required");
    assert_eq!(
        error(InstallChoices { serve_port: None, ..host.clone() }, x.clone()),
        "serve_port_required_or_forbidden"
    );
    assert_eq!(
        error(InstallChoices { web_listen: Some("0.0.0.0:7431".into()), ..host.clone() }, x.clone()),
        "invalid_web_listen"
    );
    assert_eq!(
        error(InstallChoices { instance: Some("work".into()), ..host.clone() }, x.clone()),
        "instance_web_port_required"
    );
    assert_eq!(
        error(InstallChoices { work_root: Some("/srv/work".into()), ..host }, x.clone()),
        "local_agent_disabled_conflict"
    );
    for (os, service) in [
        (InstallOs::Macos, InstallService::Launchd),
        (InstallOs::Linux, InstallService::SystemdSystem),
        (InstallOs::Windows, InstallService::WindowsScm),
    ] {
        let (mut c, mut ctx) = input();
        ctx.platform = InstallPlatform { os, arch: InstallArch::X86_64 };
        if os == InstallOs::Windows {
            for path in [&mut ctx.bin_path, &mut ctx.config_dir, &mut ctx.state_dir, &mut ctx.projects_root] {
                *path = format!("C:{path}");
            }
            c.work_root = Some("C:\\작업 & (aaa)!%\\work".into());
        }
        assert_eq!(plan(c, ctx).unwrap().choices.service, Some(service));
    }
}

#[test]
fn output_and_private_input_modes_never_turn_into_prompts() {
    let read = InstallInvocation { non_interactive: true, dry_run: true, ..Default::default() };
    assert!(validate_invocation(&read, false).is_ok());
    assert_eq!(validate_invocation(&InstallInvocation::default(), false).unwrap_err().code, "non_interactive_required");
    assert_eq!(
        validate_invocation(&InstallInvocation { json: true, json_stream: true, ..read.clone() }, false)
            .unwrap_err()
            .code,
        "output_mode_conflict"
    );
    assert_eq!(
        validate_invocation(&InstallInvocation { dry_run: false, ..read.clone() }, false).unwrap_err().code,
        "yes_required"
    );
    assert_eq!(
        validate_invocation(
            &InstallInvocation {
                authorization_stdin: true,
                authorization_file: Some("/private/approval".into()),
                ..read.clone()
            },
            false
        )
        .unwrap_err()
        .code,
        "authorization_source_conflict"
    );
    assert_eq!(
        validate_invocation(&InstallInvocation { prompt: Some(InstallPrompt::Plain), ..read.clone() }, false)
            .unwrap_err()
            .code,
        "prompt_in_non_interactive"
    );
    let status = InstallInvocation { status: Some("install-example-01".into()), json: true, ..Default::default() };
    assert!(validate_invocation(&status, false).is_ok());
    assert_eq!(
        validate_invocation(&InstallInvocation { yes: true, ..status }, false).unwrap_err().code,
        "status_is_read_only"
    );
    let mut dry_password = read.clone();
    dry_password.choices.service_password_source = Some(InstallPasswordSource::Stdin);
    assert!(validate_invocation(&dry_password, false).is_ok());
    let mut password = read;
    password.choices.service_password_source = Some(InstallPasswordSource::Prompt);
    assert_eq!(validate_invocation(&password, false).unwrap_err().code, "private_tty_required");
}

#[test]
fn protected_resume_uses_the_saved_plan_without_changing_password_choices() {
    let resume = InstallInvocation {
        resume: Some("01ARZ3NDEKTSV4RRFFQ69G5FAV".into()),
        non_interactive: true,
        yes: true,
        json_stream: true,
        secrets_stdin: true,
        ..Default::default()
    };
    assert!(validate_invocation(&resume, false).is_ok());
    let first = InstallInvocation { resume: None, ..resume.clone() };
    assert_eq!(validate_invocation(&first, false).unwrap_err().code, "password_stdin_source_required");
    for source in [InstallPasswordSource::Prompt, InstallPasswordSource::Broker] {
        let mut changed = resume.clone();
        changed.choices.service_password_source = Some(source);
        assert_eq!(validate_invocation(&changed, false).unwrap_err().code, "password_stdin_source_required");
    }
    let conflicted = InstallInvocation { authorization_stdin: true, ..resume };
    assert_eq!(validate_invocation(&conflicted, false).unwrap_err().code, "stdin_source_conflict");
}
