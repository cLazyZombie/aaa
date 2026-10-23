//! 현지 읽기 adapter. init/probe/lock/journal/enroll/service 적용 함수를 부르지 않는다.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{engine, error, platform};
use aaa_proto::install::*;
use serde_json::{Value, json};

type Result<T> = std::result::Result<T, Box<InstallResult>>;
fn path_text(p: &Path) -> Result<String> {
    p.to_str().map(str::to_owned).ok_or_else(|| error("path_not_utf8", 2))
}
fn check_directory(p: &Path, code: &str) -> Result<()> {
    match p.metadata() {
        Ok(meta) if !meta.is_dir() => Err(error(code, 2)),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(error("invalid_path", 2)),
    }
}
pub(super) fn real_dir(p: &Path) -> Result<PathBuf> {
    let resolved = aaa_agent::work_root::check(p, None).map_err(|_| error("invalid_path", 2))?;
    check_directory(&resolved, "invalid_path")?;
    Ok(resolved)
}
/// 파일 자체를 작업 폴더 검사에 넘기지 않고 원래 경로와 해석한 경로의 부모 mount를 확인한다.
fn real_file(p: &Path) -> Result<PathBuf> {
    if !p.is_absolute() || p.components().any(|part| part == std::path::Component::ParentDir) {
        return Err(error("invalid_path", 2));
    }
    let parent = real_dir(p.parent().ok_or_else(|| error("invalid_path", 2))?)?;
    let path = parent.join(p.file_name().ok_or_else(|| error("invalid_path", 2))?);
    match path.symlink_metadata() {
        Ok(_) => {
            let resolved = path.canonicalize().map_err(|_| error("invalid_path", 2))?;
            if !resolved.is_file() {
                return Err(error("invalid_path", 2));
            }
            real_dir(resolved.parent().ok_or_else(|| error("invalid_path", 2))?)?;
            Ok(resolved)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(_) => Err(error("invalid_path", 2)),
    }
}
fn digest(bytes: &[u8]) -> String {
    hex::encode(ring::digest::digest(&ring::digest::SHA256, bytes))
}
/// 입력/설정에 있는 device/FIFO와 무한 stream을 읽지 않는다. 큰 binary는 순차 hash한다.
fn file_hash(p: &Path) -> Result<Option<String>> {
    let meta = match p.metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && p.symlink_metadata().is_err() => return Ok(None),
        Err(_) => return Err(error("file_unreadable", 2)),
    };
    if !meta.is_file() || meta.len() > 1024 * 1024 * 1024 {
        return Err(error("unsupported_file", 2));
    }
    let mut file = std::fs::File::open(p).map_err(|_| error("file_unreadable", 2))?.take(1024 * 1024 * 1024 + 1);
    let mut sha = ring::digest::Context::new(&ring::digest::SHA256);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|_| error("file_unreadable", 2))?;
        if n == 0 {
            break;
        }
        sha.update(&buf[..n]);
    }
    Ok(Some(hex::encode(sha.finish())))
}

fn add_file(files: &mut BTreeMap<String, Option<String>>, path: &Path) -> Result<bool> {
    let hash = file_hash(path)?;
    let exists = hash.is_some();
    files.insert(path_text(path)?, hash);
    Ok(exists)
}
fn check(result: &mut InstallResult, name: &str, state: InstallCheckState) {
    result.checks.push(InstallCheck { name: name.into(), state });
}
fn change(result: &mut InstallResult, kind: &str, action: &str) {
    result.changes.push(InstallChange { kind: kind.into(), action: action.into(), state: InstallChangeState::Planned });
}
fn needs(result: &mut InstallResult, code: &str, target: &str) {
    result.needs_user.push(InstallNeedsUser {
        code: code.into(),
        target: target.into(),
        message: "적용 전에 필요한 도구·인증·권한을 준비하세요".into(),
    });
}
fn find_program(name: &str) -> Option<PathBuf> {
    let p = Path::new(name);
    if p.is_absolute() {
        return aaa_config::profile_edit::executable_available(name, None).then(|| p.to_owned());
    }
    if p.components().count() != 1 {
        return None;
    }
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        if !dir.is_absolute() {
            continue;
        }
        for suffix in if cfg!(windows) { &[".exe", ".cmd", ""][..] } else { &[""][..] } {
            let p = dir.join(format!("{name}{suffix}"));
            if p.to_str().is_some_and(|s| aaa_config::profile_edit::executable_available(s, None)) {
                return Some(p);
            }
        }
    }
    None
}
fn default_shell() -> String {
    std::env::var(if cfg!(windows) { "COMSPEC" } else { "SHELL" })
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| if cfg!(windows) { "cmd.exe".into() } else { "/bin/sh".into() })
}
fn isolated(c: &InstallChoices, config: &Path, state: &Path) -> Result<()> {
    if config.starts_with(state) || state.starts_with(config) {
        return Err(error("config_state_collision", 2));
    }
    if c.instance.is_some() {
        let default_config = real_dir(&aaa_config::paths::default_config_dir_for(None))?;
        let default_state = real_dir(&aaa_config::paths::default_state_dir_for(None))?;
        if config.starts_with(&default_config)
            || default_config.starts_with(config)
            || state.starts_with(&default_state)
            || default_state.starts_with(state)
        {
            return Err(error("instance_path_collision", 2));
        }
    }
    // 다른 기존 instance의 symlink override도 실제 경로로 확인한다.
    if let Some(parent) = config.parent()
        && let Ok(entries) = std::fs::read_dir(parent)
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if p == config || !p.is_dir() {
                continue;
            }
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
            if (name == "aaa" || name.starts_with("aaa-")) && real_dir(&p)? == config {
                return Err(error("instance_path_collision", 2));
            }
        }
    }
    Ok(())
}
fn unsupported(e: InstallError) -> Box<InstallResult> {
    let exit = if matches!(e.code.as_str(), "unsupported_platform" | "unsupported_service" | "platform_mismatch") {
        5
    } else {
        2
    };
    let mut r = error(&e.code, exit);
    r.error = Some(e);
    r
}
fn binary_matches(path: &Path, platform: &InstallPlatform) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else { return false };
    let mut header = [0u8; 4096];
    let Ok(n) = f.read(&mut header) else { return false };
    if n < 64 {
        return false;
    }
    match platform.os {
        InstallOs::Macos => {
            header[..4] == [0xcf, 0xfa, 0xed, 0xfe]
                && u32::from_le_bytes(header[4..8].try_into().unwrap())
                    == if platform.arch == InstallArch::Aarch64 { 0x100000c } else { 0x1000007 }
        }
        InstallOs::Linux => {
            header[..6] == [0x7f, b'E', b'L', b'F', 2, 1]
                && u16::from_le_bytes(header[18..20].try_into().unwrap())
                    == if platform.arch == InstallArch::Aarch64 { 183 } else { 62 }
        }
        InstallOs::Windows => {
            let at = u32::from_le_bytes(header[60..64].try_into().unwrap()) as usize;
            header[..2] == *b"MZ"
                && at.checked_add(6).is_some_and(|end| end <= n)
                && header[at..at + 4] == *b"PE\0\0"
                && header[at + 4..at + 6] == [0x64, 0x86]
        }
    }
}

/// 발급/실행기는 공개 결과의 hash를 역조립하지 않고 같은 현지 planner의 private 계획을 받는다.
pub(super) fn inspect(i: &InstallInvocation) -> Result<(InstallResult, InstallPlan)> {
    let mut c = i.choices.clone();
    let home = aaa_config::paths::home();
    let platform = platform::platform()?;
    let config = c
        .config_dir
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| aaa_config::paths::default_config_dir_for(c.instance.as_deref()));
    let state = c
        .state_dir
        .clone()
        .map(PathBuf::from)
        .unwrap_or_else(|| aaa_config::paths::default_state_dir_for(c.instance.as_deref()));
    let binary = home.join(".local/bin").join(if cfg!(windows) { "aaa.exe" } else { "aaa" });
    let version = aaa_common::util::software().trim_start_matches("aaa ").to_string();
    // 먼저 공통 선택 계약을 검사한다. filesystem 오류가 필수값/상충 오류보다 앞서지 않는다.
    let mut context = InstallPlanContext {
        platform,
        version: version.clone(),
        binary_sha256: "0".repeat(64),
        bin_path: path_text(&binary)?,
        config_dir: path_text(&config)?,
        state_dir: path_text(&state)?,
        projects_root: path_text(&aaa_config::paths::projects_root_default_for(c.instance.as_deref()))?,
        default_shell: default_shell(),
        effective_config_sha256: "0".repeat(64),
        target_account: "unobserved".into(),
    };
    let shape = aaa_config::install::plan(c.clone(), context.clone()).map_err(unsupported)?;
    if !aaa_common::util::machine_name_ok(c.name.as_deref().unwrap_or_default()) {
        return Err(error("invalid_machine_name", 2));
    }
    let config = real_dir(&config)?;
    let state = real_dir(&state)?;
    isolated(&c, &config, &state)?;
    c.config_dir = Some(path_text(&config)?);
    c.state_dir = Some(path_text(&state)?);
    c.bin_path = Some(path_text(&real_file(c.bin_path.as_deref().map(Path::new).unwrap_or(&binary))?)?);
    // 기존 설정도 loader를 부르기 전에 일반 파일/크기를 확인한다.
    for name in ["host.toml", "host.local.toml", "agent.toml", "agent.local.toml"] {
        let p = config.join(name);
        if let Ok(meta) = p.metadata()
            && (!meta.is_file() || meta.len() > 1024 * 1024)
        {
            return Err(error("invalid_existing_config", 2));
        }
    }
    let host = aaa_config::load_host(&config).map_err(|_| error("invalid_existing_config", 2))?;
    let agent = aaa_config::load_agent(&config).map_err(|_| error("invalid_existing_config", 2))?;
    let has_local = c.role == Some(InstallRole::Agent) || c.local_agent != Some(InstallToggle::Disabled);
    if has_local && c.min_free_gb.is_none() && !agent.files.is_empty() {
        c.min_free_gb =
            Some(u32::try_from(agent.config.health.min_free_gb).map_err(|_| error("invalid_existing_config", 2))?);
    }
    let mut files = BTreeMap::new();
    let mut existing = false;
    for role in ["host", "agent"] {
        for suffix in [".toml", ".local.toml"] {
            existing |= add_file(&mut files, &config.join(format!("{role}{suffix}")))?;
        }
    }
    for file in [
        "tls/cert.der",
        "tls/key.der",
        "host.db",
        "agent/identity.json",
        "agent/host.json",
        "agent/tls/cert.der",
        "agent/tls/key.der",
        "agent/work-roots.json",
        "install/receipt.json",
    ] {
        existing |= add_file(&mut files, &state.join(file))?;
    }
    let mut result = *error("unplanned", 2);
    result.error = None;
    result.status = InstallStatus::Planned;
    result.exit_code = 0;
    result.role = c.role;
    result.instance = c.instance.clone();
    result.platform = Some(context.platform.clone());
    context.target_account = platform::account().ok_or_else(|| error("account_unavailable", 5))?;
    let current = std::env::current_exe().map_err(|_| error("binary_unreadable", 2))?;
    let selected = c.from.as_deref().map(Path::new).unwrap_or(&current);
    context.binary_sha256 = file_hash(selected)?.ok_or_else(|| error("binary_unreadable", 2))?;
    if c.sha256.as_ref().is_some_and(|sha| !sha.eq_ignore_ascii_case(&context.binary_sha256)) {
        return Err(error("binary_digest_mismatch", 2));
    }
    if c.version.as_ref().is_some_and(|v| v.trim_start_matches('v') != version.trim_start_matches('v')) {
        return Err(error("release_resolution_not_available", 5));
    }
    if c.version.is_some() {
        c.version = None;
    }
    if c.from.is_some() && !binary_matches(selected, &context.platform) {
        return Err(error("binary_platform_mismatch", 5));
    }
    context.version = if c.from.is_some() { "unverified".into() } else { version };
    check(&mut result, "binary_digest", InstallCheckState::Passed);
    check(
        &mut result,
        "binary_version",
        if c.from.is_some() { InstallCheckState::Unknown } else { InstallCheckState::Passed },
    );
    let target = Path::new(c.bin_path.as_deref().unwrap());
    if let Some(old) = file_hash(target)? {
        if old != context.binary_sha256 {
            return Err(error("binary_target_conflict", 4));
        }
        files.insert(path_text(target)?, Some(old));
        result.changes.push(InstallChange {
            kind: "binary".into(),
            action: "preserve".into(),
            state: InstallChangeState::Unchanged,
        });
    } else {
        change(&mut result, "binary", "stage");
    }
    let mut observations = json!({});
    if has_local {
        aaa_agent::work_root::inspect(&state.join("agent")).map_err(|_| error("invalid_work_root_ownership", 2))?;
        let root = Path::new(c.work_root.as_deref().unwrap());
        let mount = c.work_mount.as_deref().map(Path::new);
        let resolved = aaa_agent::work_root::check(root, mount).map_err(|_| error("work_mount_unavailable", 2))?;
        check_directory(&resolved, "work_root_not_directory")?;
        if context.platform.os == InstallOs::Macos && resolved.starts_with("/Volumes") && mount.is_none() {
            return Err(error("external_mount_required", 2));
        }
        check(&mut result, "work_root_ownership", InstallCheckState::Unknown);
        check(&mut result, "work_volume_stable_identity", InstallCheckState::Unknown);
        if resolved.parent().is_none() || mount.is_some_and(|m| m.canonicalize().is_ok_and(|m| m == resolved)) {
            return Err(error("work_root_is_volume_root", 2));
        }
        let (identity, free, writable) =
            platform::storage(&resolved).ok_or_else(|| error("work_storage_unavailable", 2))?;
        let needed = u64::from(c.min_free_gb.unwrap_or(20)).saturating_mul(1024 * 1024 * 1024);
        if free < needed {
            return Err(error("work_space_insufficient", 2));
        }
        if writable == Some(false) {
            return Err(error("work_storage_not_writable", 2));
        }
        observations["work_storage"] = json!({"real_path":resolved,"identity":identity});
        c.work_root = Some(path_text(&resolved)?);
        if let Some(mount) = mount {
            c.work_mount = Some(path_text(&mount.canonicalize().map_err(|_| error("work_mount_unavailable", 2))?)?);
        }
        check(&mut result, "work_mount", InstallCheckState::Passed);
        check(&mut result, "work_free_space", InstallCheckState::Passed);
        check(
            &mut result,
            "write_permission_preflight",
            if writable == Some(true) { InstallCheckState::Passed } else { InstallCheckState::Unknown },
        );
        change(&mut result, "work_root", "prepare");
    }
    for name in ["write_probe", "host_connected", "service_ready", "enrollment", "pairing", "authorization"] {
        check(&mut result, name, InstallCheckState::NotRun);
    }
    for harness in &c.harness {
        let (name, path, config_home) = match harness {
            InstallHarness::Claude => (
                "claude",
                &c.claude_path,
                c.claude_home
                    .clone()
                    .map(PathBuf::from)
                    .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
                    .unwrap_or_else(|| home.join(".claude")),
            ),
            InstallHarness::Codex => (
                "codex",
                &c.codex_path,
                c.codex_home.clone().map(PathBuf::from).unwrap_or_else(crate::integration::codex_home),
            ),
            InstallHarness::None => continue,
        };
        let override_command = agent.config.harnesses.get(name).and_then(|h| h.command.first());
        let found = find_program(path.as_deref().or(override_command.map(String::as_str)).unwrap_or(name));
        let state = if found.is_some() {
            InstallCheckState::Passed
        } else {
            needs(&mut result, "harness_missing", name);
            InstallCheckState::Failed
        };
        check(&mut result, &format!("{name}_binary"), state);
        if let Some(program) = found {
            add_file(&mut files, &program)?;
        }
        for file in ["config.toml", "hooks.json", "aaa-codex-integration.json"] {
            add_file(&mut files, &config_home.join(file))?;
        }
        observations[format!("{name}_home")] = json!(real_dir(&config_home)?);
        check(&mut result, &format!("{name}_version"), InstallCheckState::Unknown);
        check(&mut result, &format!("{name}_login"), InstallCheckState::Unknown);
        check(&mut result, &format!("{name}_hook_trust"), InstallCheckState::Unknown);
    }
    let mut profile_names: std::collections::BTreeSet<_> = host.config.profiles.keys().cloned().collect();
    profile_names.extend(["claude".to_string(), "codex".to_string()]);
    for profile in &c.profile_file {
        let path = real_file(Path::new(profile))?;
        if file_hash(&path)?.is_none() || path.metadata().map_or(true, |m| m.len() > 1024 * 1024) {
            return Err(error("profile_unreadable", 2));
        }
        let text = std::fs::read_to_string(&path).map_err(|_| error("profile_unreadable", 2))?;
        profile_names.extend(super::profiles::names(&text)?);
        add_file(&mut files, &path)?;
    }
    for name in c
        .thread_profile
        .iter()
        .chain(c.coordinator_profile.iter())
        .chain(c.allow_thread_profile.iter())
        .chain(c.allow_coordinator_profile.iter())
    {
        if name != "*" && !profile_names.contains(name) {
            return Err(error("unknown_profile", 2));
        }
    }
    for repo in &c.repo {
        if let Some((_, machine)) = repo.rsplit_once('@')
            && c.role == Some(InstallRole::Host)
            && !machine.is_empty()
        {
            check(&mut result, "remote_repo", InstallCheckState::Unknown);
            continue;
        }
        repo_check(Path::new(repo))?;
        check(&mut result, "repo", InstallCheckState::Passed);
        observations[format!("repo:{repo}")] = json!(real_dir(Path::new(repo))?);
    }
    let roles = if c.role == Some(InstallRole::Host) && has_local {
        &["host", "agent"][..]
    } else if c.role == Some(InstallRole::Host) {
        &["host"][..]
    } else {
        &["agent"][..]
    };
    let service = super::services::read(shape.choices.service.unwrap(), roles, c.instance.as_deref(), &home);
    for path in &service.files {
        existing |= add_file(&mut files, path)?;
    }
    existing |= service.present == Some(true);
    check(&mut result, "service_manager", service.manager);
    observations["service_inventory"] = json!({"manager":service.manager,"present":service.present});
    if context.platform.os == InstallOs::Macos && shape.choices.service == Some(InstallService::Launchd) {
        c.launcher.get_or_insert_with(|| {
            home.join("Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher").to_string_lossy().into_owned()
        });
    }
    if let Some(launcher) = &c.launcher {
        let path = real_file(Path::new(launcher))?;
        add_file(&mut files, &path)?;
        c.launcher = Some(path_text(&path)?);
    }
    if shape.choices.service == Some(InstallService::Launchd) {
        if c.launcher_policy == Some(InstallLauncherPolicy::Install) {
            let provisional = aaa_config::install::plan(c.clone(), context.clone()).map_err(unsupported)?;
            let asset = engine::launcher_asset_source(&provisional)
                .map_err(|_| error("installation_launcher_asset_required", 3))?;
            add_file(&mut files, &asset)?;
            if !asset.is_file() && !c.launcher.as_ref().is_some_and(|p| Path::new(p).is_file()) {
                result.needs_user.push(InstallNeedsUser {
                    code: "installation_launcher_asset_required".into(),
                    target: "launcher".into(),
                    message: "같은 release의 launcher JSON asset을 설치 원본 실행 파일 옆에 준비하세요".into(),
                });
            }
        }
        observations["service_environment"] = json!(
            std::env::vars()
                .filter(|(key, _)| matches!(key.as_str(), "PATH" | "LANG" | "LC_ALL" | "LC_CTYPE" | "TZ" | "TERM"))
                .collect::<std::collections::BTreeMap<_, _>>()
        );
        if !c.launcher.as_ref().is_some_and(|p| Path::new(p).is_file())
            && c.launcher_policy != Some(InstallLauncherPolicy::Install)
        {
            result.needs_user.push(InstallNeedsUser {
                code: "installation_launcher_required".into(),
                target: "launcher".into(),
                message: "같은 release의 검증된 launcher를 준비하거나 기존 launcher 경로를 지정하세요".into(),
            });
        }
    }
    if c.service != Some(InstallService::None) {
        change(&mut result, "service", "register_after_authorization");
    }
    check(
        &mut result,
        "service_ownership",
        if c.service == Some(InstallService::None) {
            InstallCheckState::NotRequested
        } else {
            InstallCheckState::Unknown
        },
    );
    if c.tailscale_serve == Some(InstallServe::On) {
        serve_check(&mut result, &c, &mut observations)?;
    }
    check(&mut result, "ports", InstallCheckState::Unknown); // bind/probe는 하지 않는다.
    if existing && c.mode.unwrap_or(InstallMode::Fresh) == InstallMode::Fresh {
        return Err(error("existing_installation_conflict", 4));
    }
    change(
        &mut result,
        if c.role == Some(InstallRole::Host) { "host_config" } else { "agent_config" },
        if existing { "merge_preserving_existing" } else { "create" },
    );
    if c.role == Some(InstallRole::Host) && c.project_mode != Some(InstallProjectMode::Skip) {
        change(&mut result, "project", "prepare");
    }
    if c.hooks == Some(InstallHooks::Install) {
        change(&mut result, "codex_hooks", "merge_preserving_existing");
    }
    if c.path == Some(InstallPathPolicy::User) {
        change(&mut result, "user_path", "add_owned_entry");
    }
    // 실효 설정과 선택 자원 내용은 private hash에만 반영한다. 원문/경로/계정은 공개 결과에 싣지 않는다.
    context.effective_config_sha256 = digest(
        &serde_json::to_vec(&json!({"host":host.config,"agent":agent.config,"files":files,"observed":observations}))
            .map_err(|_| error("invalid_existing_config", 2))?,
    );
    let plan = aaa_config::install::plan(c, context).map_err(unsupported)?;
    result.plan_sha256 = Some(aaa_config::install::plan_sha256(&plan));
    result.version = (plan.context.version != "unverified").then_some(plan.context.version.clone());
    result.binary_sha256 = Some(plan.context.binary_sha256.clone());
    check(&mut result, "effective_config", InstallCheckState::Passed);
    if !result.needs_user.is_empty() {
        result.status = InstallStatus::NeedsUser;
        result.exit_code = 3;
    }
    Ok((result, plan))
}

fn repo_check(path: &Path) -> Result<()> {
    let path = real_dir(path)?;
    if !path.is_dir() {
        return Err(error("repo_unavailable", 2));
    }
    let rt =
        tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| error("repo_unavailable", 2))?;
    rt.block_on(async {
        let mut cmd = tokio::process::Command::new("git");
        cmd.args(["--no-optional-locks", "-c", "core.fsmonitor=false", "-c", "safe.directory="])
            .arg("-C")
            .arg(path)
            .args(["rev-parse", "--git-dir"])
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        match tokio::time::timeout(Duration::from_secs(10), cmd.output()).await {
            Ok(Ok(out)) if out.status.success() => Ok(()),
            _ => Err(error("repo_unavailable", 2)),
        }
    })
}
fn serve_check(result: &mut InstallResult, c: &InstallChoices, observations: &mut Value) -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| error("network_detection_unavailable", 5))?;
    let port = c
        .web_listen
        .as_deref()
        .unwrap_or("127.0.0.1:7431")
        .parse::<std::net::SocketAddr>()
        .map_err(|_| error("invalid_web_listen", 2))?
        .port();
    #[cfg(windows)]
    let exact = rt
        .block_on(aaa_host::tailscale::conditional_snapshot(port, c.serve_port.unwrap()))
        .map(|native| native.snapshot);
    #[cfg(not(windows))]
    let exact = {
        let out = rt.block_on(aaa_host::tailscale::read());
        aaa_host::tailscale::exact(out.as_ref(), port, c.serve_port.unwrap())
    };
    match exact {
        Ok(exact) => {
            check(result, "tailscale", InstallCheckState::Passed);
            observations["tailscale"] = json!({"before_sha256":exact.before_sha256().map_err(|_| error("tailscale_status_unknown", 3))?,"serve_port":exact.port,"target_port":exact.target_port,"reused":exact.reused});
        }
        Err(fault) if fault.conflict => return Err(error(fault.code, 4)),
        Err(fault) => {
            check(result, "tailscale", InstallCheckState::Unknown);
            needs(result, fault.code, "tailscale");
            observations["tailscale"] = json!({"state":fault.code});
        }
    }
    change(result, "tailscale_serve", "publish_after_authorization");
    Ok(())
}

#[cfg(test)]
mod digest_tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn file_and_memory_digests_match_independent_vectors_and_fresh_bytes() {
        struct Directory(PathBuf);
        impl Drop for Directory {
            fn drop(&mut self) {
                std::fs::remove_dir_all(&self.0).unwrap();
            }
        }
        let lab = Directory(std::env::temp_dir().join(format!("aaa-digest-{}", aaa_common::util::new_id())));
        std::fs::create_dir(&lab.0).unwrap();
        let path = lab.0.join("digest-input");
        assert_eq!(file_hash(&path).unwrap(), None);
        for size in [0, 3, 65535, 65536, 65537, 131079] {
            let bytes: Vec<_> = (0..size).map(|i| (i % 251) as u8).collect();
            std::fs::write(&path, &bytes).unwrap();
            let expected = hex::encode(Sha256::digest(&bytes));
            assert_eq!(file_hash(&path).unwrap(), Some(expected.clone()));
            assert_eq!(digest(&bytes), expected);
        }
        for bytes in [b"abc", b"abd"] {
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(file_hash(&path).unwrap(), Some(hex::encode(Sha256::digest(bytes))));
        }
        assert_eq!(file_hash(&lab.0).unwrap_err().error.as_ref().map(|e| e.code.as_str()), Some("unsupported_file"));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(1024 * 1024 * 1024 + 1).unwrap();
        assert_eq!(file_hash(&path).unwrap_err().error.as_ref().map(|e| e.code.as_str()), Some("unsupported_file"));
    }
}
