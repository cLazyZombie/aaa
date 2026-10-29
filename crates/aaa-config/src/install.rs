//! 설치 I0의 순수 계약 검증. 파일/환경/프로세스를 읽거나 쓰지 않는다.
//! 질문과 CLI는 같은 InstallChoices를 만들고 이 함수를 통과해야 한다.
use aaa_proto::install::*;
use sha2::{Digest, Sha256};

fn invalid(code: &str) -> InstallError {
    InstallError {
        code: code.into(),
        message: "설치 선택의 필수값과 상충 항목을 확인하세요".into(),
        details: vec![],
        disk_space: None,
    }
}
fn require(ok: bool, code: &str) -> Result<(), InstallError> {
    if ok { Ok(()) } else { Err(invalid(code)) }
}
fn text_ok(s: &str) -> bool {
    !s.is_empty() && !s.chars().any(char::is_control) && !s.contains("$(") && !s.contains('`')
}
fn absolute(s: &str, os: InstallOs) -> bool {
    if !text_ok(s) {
        return false;
    }
    match os {
        InstallOs::Windows => {
            let b = s.as_bytes();
            (b.len() > 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\'))
                || (s.starts_with("\\\\") && s[2..].split('\\').filter(|s| !s.is_empty()).count() >= 2)
        }
        _ => s.starts_with('/'),
    }
}
fn digest_ok(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit())
}

/// 질문의 허용 여부만 확인한다. 권한·W0·승인 검증은 이 함수로 대신할 수 없다.
pub fn validate_invocation(i: &InstallInvocation, tty: bool) -> Result<(), InstallError> {
    require(!(i.json && i.json_stream), "output_mode_conflict")?;
    require(!(i.authorization_stdin && i.authorization_file.is_some()), "authorization_source_conflict")?;
    require(!(i.authorization_stdin && i.secrets_stdin), "stdin_source_conflict")?;
    require(!(i.resume.is_some() && i.status.is_some()), "resume_status_conflict")?;
    let automated = i.non_interactive || i.json || i.json_stream;
    require(tty || automated, "non_interactive_required")?;
    require(!automated || i.prompt.is_none(), "prompt_in_non_interactive")?;
    require(!automated || i.dry_run || i.status.is_some() || i.yes, "yes_required")?;
    require(
        i.status.is_none()
            || (!i.dry_run
                && !i.yes
                && i.resume.is_none()
                && !i.authorization_stdin
                && i.authorization_file.is_none()
                && !i.secrets_stdin
                && i.service_password_ref.is_none()
                && i.choices == InstallChoices::default()),
        "status_is_read_only",
    )?;
    let source = i.choices.service_password_source;
    // 재개는 저장된 서명 계획에서 입력 정책을 읽는다. 보호 채널을 고르려고 계획 선택을 바꾸지 않는다.
    let resume_source = i.resume.is_some() && source.is_none();
    require(
        (!i.secrets_stdin || source == Some(InstallPasswordSource::Stdin) || resume_source)
            && (i.dry_run || source != Some(InstallPasswordSource::Stdin) || i.secrets_stdin),
        "password_stdin_source_required",
    )?;
    require(
        (i.service_password_ref.is_none() || source == Some(InstallPasswordSource::Broker))
            && (i.dry_run || source != Some(InstallPasswordSource::Broker) || i.service_password_ref.is_some()),
        "password_broker_ref_required",
    )?;
    require(!automated || source != Some(InstallPasswordSource::Prompt), "private_tty_required")?;
    for value in
        [&i.resume, &i.status, &i.events_file, &i.authorization_file, &i.service_password_ref].into_iter().flatten()
    {
        require(text_ok(value), "unsafe_control_argument")?;
    }
    Ok(())
}

/// argv/질문 순서·출력 모드·yes·dry-run은 계획 hash에 포함하지 않는다.
/// 역할별 공통 계약을 먼저 검증한다. 경로 실재·mount·계정·권한·pin은 I1 이후 adapter가 검증한다.
pub fn plan(mut c: InstallChoices, mut context: InstallPlanContext) -> Result<InstallPlan, InstallError> {
    let role = c.role.ok_or_else(|| invalid("role_required"))?;
    require(c.name.as_deref().is_some_and(|s| !s.is_empty()), "name_required")?;
    if let Some(instance) = &c.instance {
        require(crate::paths::check_instance(instance).is_ok(), "invalid_instance")?;
    }
    // 입력 원문이 오류/Debug에 새지 않게 고정 code만 반환한다.
    let raw = serde_json::to_value(&c).map_err(|_| invalid("invalid_choices"))?;
    fn safe(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::String(s) => s.is_empty() || text_ok(s),
            serde_json::Value::Array(a) => a.iter().all(safe),
            serde_json::Value::Object(m) => m.values().all(safe),
            _ => true,
        }
    }
    require(safe(&raw), "unsafe_argument")?;
    require(!(c.version.is_some() && c.from.is_some()), "binary_source_conflict")?;
    require(c.from.is_some() == c.sha256.is_some(), "source_digest_required")?;
    require(
        c.sha256.as_deref().is_none_or(digest_ok)
            && digest_ok(&context.binary_sha256)
            && digest_ok(&context.effective_config_sha256),
        "invalid_sha256",
    )?;
    require(
        c.version.as_deref().is_none_or(|v| text_ok(v) && !matches!(v, "latest" | "stable" | "nightly")),
        "exact_version_required",
    )?;
    require(
        c.expected_os.is_none_or(|v| v == context.platform.os)
            && c.expected_arch.is_none_or(|v| v == context.platform.arch),
        "platform_mismatch",
    )?;
    require(
        context.platform.os != InstallOs::Windows || context.platform.arch == InstallArch::X86_64,
        "unsupported_platform",
    )?;
    let os = context.platform.os;
    for path in [
        &c.bin_path,
        &c.config_dir,
        &c.state_dir,
        &c.from,
        &c.projects_root,
        &c.work_root,
        &c.work_mount,
        &c.default_cwd,
        &c.claude_path,
        &c.codex_path,
        &c.claude_home,
        &c.codex_home,
        &c.launcher,
    ]
    .into_iter()
    .flatten()
    {
        require(absolute(path, os), "absolute_path_required")?;
    }
    for path in [&context.bin_path, &context.config_dir, &context.state_dir, &context.projects_root] {
        require(absolute(path, os), "invalid_context_path")?;
    }
    require(
        text_ok(&context.version) && text_ok(&context.target_account) && text_ok(&context.default_shell),
        "invalid_context",
    )?;
    let service = c.service.ok_or_else(|| invalid("service_required"))?;
    c.service = Some(match service {
        InstallService::Auto => match os {
            InstallOs::Macos => InstallService::Launchd,
            InstallOs::Linux => InstallService::SystemdSystem,
            InstallOs::Windows => InstallService::WindowsScm,
        },
        s => s,
    });
    require(
        matches!(
            (os, c.service.unwrap()),
            (_, InstallService::None)
                | (InstallOs::Macos, InstallService::Launchd)
                | (InstallOs::Linux, InstallService::SystemdSystem | InstallService::SystemdUser)
                | (InstallOs::Windows, InstallService::WindowsScm)
        ),
        "unsupported_service",
    )?;
    require(c.linger.is_none() || c.service == Some(InstallService::SystemdUser), "linger_requires_user_unit")?;
    require(
        os == InstallOs::Macos || (c.launcher.is_none() && c.launcher_policy.is_none()),
        "launcher_requires_macos",
    )?;
    require(os == InstallOs::Windows || c.service_password_source.is_none(), "password_requires_windows")?;
    require(c.service != Some(InstallService::None) || c.service_account.is_none(), "account_requires_service")?;
    require(!(c.work_mount.is_some() && c.work_storage.is_some()), "work_storage_conflict")?;
    require(!(c.no_repo && !c.repo.is_empty()), "repo_conflict")?;
    let local = match role {
        InstallRole::Host => {
            c.local_agent.get_or_insert(InstallToggle::Enabled);
            let project = c.project_mode.ok_or_else(|| invalid("project_mode_required"))?;
            if project != InstallProjectMode::Skip {
                require(c.project.as_deref().is_some_and(text_ok), "project_required")?;
                require(c.no_repo || !c.repo.is_empty(), "repo_choice_required")?;
            } else {
                require(
                    c.project.is_none()
                        && c.repo.is_empty()
                        && !c.no_repo
                        && c.goal.is_none()
                        && c.projects_root.is_none(),
                    "project_skip_conflict",
                )?;
            }
            require(c.host.is_none() && c.enroll.is_none() && c.wait_timeout.is_none(), "agent_options_on_host")?;
            let web = c.web.ok_or_else(|| invalid("web_required"))?;
            let serve = c.tailscale_serve.ok_or_else(|| invalid("serve_choice_required"))?;
            let pairing = c.pairing.ok_or_else(|| invalid("pairing_required"))?;
            require(
                !(web == InstallWeb::Off
                    && (serve == InstallServe::On || pairing == InstallPairing::Controller || c.web_listen.is_some())),
                "web_off_conflict",
            )?;
            require((serve == InstallServe::On) == c.serve_port.is_some(), "serve_port_required_or_forbidden")?;
            require(c.serve_port != Some(0), "invalid_serve_port")?;
            require(c.agent_listen.is_some(), "agent_listen_required")?;
            let listen: std::net::SocketAddr =
                c.agent_listen.as_deref().unwrap().parse().map_err(|_| invalid("invalid_agent_listen"))?;
            require(listen.port() != 0, "invalid_agent_listen")?;
            if web == InstallWeb::Local {
                require(c.instance.is_none() || c.web_listen.is_some(), "instance_web_port_required")?;
                c.web_listen.get_or_insert("127.0.0.1:7431".into());
                let web: std::net::SocketAddr =
                    c.web_listen.as_deref().unwrap().parse().map_err(|_| invalid("invalid_web_listen"))?;
                require(
                    web.ip().is_loopback() && web.port() != 0 && web.port() != listen.port(),
                    "invalid_web_listen",
                )?;
            }
            c.open_coordinator.get_or_insert(InstallOpen::No);
            c.local_agent == Some(InstallToggle::Enabled)
        }
        InstallRole::Agent => {
            require(
                c.local_agent.is_none()
                    && c.project_mode.is_none()
                    && c.project.is_none()
                    && c.projects_root.is_none()
                    && c.goal.is_none()
                    && !c.no_repo
                    && c.agent_listen.is_none()
                    && c.web_listen.is_none()
                    && c.web.is_none()
                    && c.tailscale_serve.is_none()
                    && c.serve_port.is_none()
                    && c.pairing.is_none()
                    && c.open_coordinator.is_none()
                    && c.profile_file.is_empty()
                    && c.thread_profile.is_none()
                    && c.coordinator_profile.is_none()
                    && c.allow_thread_profile.is_empty()
                    && c.allow_coordinator_profile.is_empty(),
                "host_options_on_agent",
            )?;
            let host = c.host.as_deref().ok_or_else(|| invalid("host_required"))?;
            let (name, port) = host.rsplit_once(':').ok_or_else(|| invalid("invalid_host"))?;
            require(
                text_ok(name) && !name.contains(['/', '@', ' ']) && port.parse::<u16>().is_ok_and(|p| p > 0),
                "invalid_host",
            )?;
            require(c.network.is_some(), "network_required")?;
            require(c.enroll.is_some(), "enroll_required")?;
            c.wait_timeout.get_or_insert(600);
            require(c.wait_timeout != Some(0), "invalid_wait_timeout")?;
            require(c.repo.iter().all(|p| absolute(p, os)), "agent_repo_must_be_local")?;
            true
        }
    };
    if local {
        require(c.work_root.is_some(), "work_root_required")?;
        require(c.work_mount.is_some() || c.work_storage.is_some(), "work_storage_required")?;
        require(!c.harness.is_empty(), "harness_required")?;
    } else {
        require(
            c.work_root.is_none()
                && c.work_mount.is_none()
                && c.work_storage.is_none()
                && c.min_free_gb.is_none()
                && c.default_cwd.is_none()
                && c.shell.is_none()
                && c.harness.is_empty()
                && c.harness_env.is_empty()
                && c.claude_path.is_none()
                && c.codex_path.is_none()
                && c.claude_home.is_none()
                && c.codex_home.is_none()
                && c.login.is_none()
                && c.hooks.is_none(),
            "local_agent_disabled_conflict",
        )?;
    }
    c.harness.sort();
    c.harness.dedup();
    if role == InstallRole::Host && c.harness.len() > 1 {
        require(c.thread_profile.is_some() && c.coordinator_profile.is_some(), "profile_defaults_required")?;
    }
    require(!(c.harness.contains(&InstallHarness::None) && c.harness.len() > 1), "harness_none_conflict")?;
    require(
        c.harness.contains(&InstallHarness::Codex) || (c.codex_path.is_none() && c.codex_home.is_none()),
        "codex_not_selected",
    )?;
    require(
        c.harness.contains(&InstallHarness::Claude) || (c.claude_path.is_none() && c.claude_home.is_none()),
        "claude_not_selected",
    )?;
    if c.harness.contains(&InstallHarness::Codex) {
        require(c.hooks.is_some(), "hooks_required")?;
    } else if local {
        c.hooks.get_or_insert(InstallHooks::Preserve);
    }
    require(c.harness.contains(&InstallHarness::Codex) || c.hooks != Some(InstallHooks::Install), "codex_hooks_only")?;
    c.harness_env.sort_by(|a, b| (a.harness, &a.key).cmp(&(b.harness, &b.key)));
    for (index, env) in c.harness_env.iter().enumerate() {
        require(c.harness.contains(&env.harness) && env.harness != InstallHarness::None, "env_harness_not_selected")?;
        require(matches!(env.key.as_str(), "LANG" | "LC_ALL" | "LC_CTYPE" | "TZ" | "TERM"), "env_key_not_allowed")?;
        require(
            index == 0 || (env.harness, &env.key) != (c.harness_env[index - 1].harness, &c.harness_env[index - 1].key),
            "duplicate_env_key",
        )?;
    }
    c.repo.sort();
    c.repo.dedup();
    c.allow_thread_profile.sort();
    c.allow_thread_profile.dedup();
    c.allow_coordinator_profile.sort();
    c.allow_coordinator_profile.dedup();
    c.mode.get_or_insert(InstallMode::Fresh);
    if local {
        c.min_free_gb.get_or_insert(20);
        c.default_cwd = c.default_cwd.or_else(|| c.work_root.clone());
        c.shell.get_or_insert(context.default_shell.clone());
        c.login.get_or_insert(InstallLogin::Require);
    }
    c.start.get_or_insert(InstallStart::Now);
    c.startup.get_or_insert(InstallStartup::Auto);
    c.path.get_or_insert(InstallPathPolicy::Preserve);
    if c.service != Some(InstallService::None) {
        c.service_account.get_or_insert(InstallAccount::CurrentUser);
    }
    if os == InstallOs::Macos {
        c.launcher_policy.get_or_insert(InstallLauncherPolicy::Preserve);
    }
    if c.service == Some(InstallService::SystemdUser) {
        c.linger.get_or_insert(InstallLinger::Require);
    }
    c.bin_path.get_or_insert(context.bin_path.clone());
    c.config_dir.get_or_insert(context.config_dir.clone());
    c.state_dir.get_or_insert(context.state_dir.clone());
    require(c.config_dir != c.state_dir, "config_state_collision")?;
    // 선택한 override도 context에 반영해 같은 실효 입력을 단일 표현으로 만든다.
    context.bin_path = c.bin_path.clone().unwrap();
    context.config_dir = c.config_dir.clone().unwrap();
    context.state_dir = c.state_dir.clone().unwrap();
    context.binary_sha256.make_ascii_lowercase();
    context.effective_config_sha256.make_ascii_lowercase();
    if let Some(s) = &mut c.sha256 {
        s.make_ascii_lowercase();
    }
    if c.version.as_deref() == Some(&context.version) {
        c.version = None;
    }
    c.expected_os = None;
    c.expected_arch = None;
    Ok(InstallPlan { schema: InstallPlanSchema::V1, context, choices: c })
}

/// 원격 등록과 installer가 같은 계획의 한 agent/host 작업만 승인한다.
pub fn enrollment_permission(plan: &InstallPlan) -> Option<InstallPermission> {
    (plan.choices.role == Some(InstallRole::Agent) && plan.choices.enroll != Some(InstallEnroll::Skip)).then(|| {
        InstallPermission {
            operation: InstallOperation::EnrollAgent,
            target: format!(
                "host:{}/agent:{}",
                plan.choices.host.as_deref().unwrap_or_default(),
                plan.choices.name.as_deref().unwrap_or_default()
            ),
            before_sha256: None,
            after_sha256: Some(plan_sha256(plan)),
        }
    })
}

/// v1 canonical bytes = UTF-8 compact JSON, 객체 key 재귀 사전순, 배열은 의미 순서 보존.
/// OS path의 case/symlink/..는 실물 확인 없이 합치지 않는다. I1이 검증한 실효 경로를 제출한다.
pub fn plan_sha256(plan: &InstallPlan) -> String {
    fn canonical(v: serde_json::Value) -> serde_json::Value {
        match v {
            serde_json::Value::Object(m) => {
                let sorted: std::collections::BTreeMap<_, _> = m.into_iter().map(|(k, v)| (k, canonical(v))).collect();
                serde_json::to_value(sorted).expect("canonical map")
            }
            serde_json::Value::Array(a) => serde_json::Value::Array(a.into_iter().map(canonical).collect()),
            v => v,
        }
    }
    let value = canonical(serde_json::to_value(plan).expect("설치 계획"));
    hex::encode(Sha256::digest(serde_json::to_vec(&value).expect("canonical 설치 계획")))
}

#[cfg(test)]
mod tests;
