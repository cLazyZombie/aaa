//! macOS: 승인된 파일 receipt → 고정 launcher/LaunchAgent → 실제 backend proof.
use super::*;

#[cfg(target_os = "macos")]
mod native;
#[cfg(target_os = "macos")]
mod runtime;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Context {
    launcher: PathBuf,
    launcher_sha256: String,
}
impl Context {
    pub(super) fn from_plan(plan: &InstallPlan) -> Result<Option<Self>> {
        if plan.choices.service != Some(InstallService::Launchd) {
            return Ok(None);
        }
        let launcher = plan.choices.launcher.as_deref().ok_or_else(|| failure("installation_launcher_required", 3))?;
        let launcher = transaction::target(Path::new(launcher))?;
        let bytes = std::fs::read(&launcher).map_err(|_| failure("installation_launcher_required", 3))?;
        let this = Self { launcher, launcher_sha256: hash(&bytes) };
        this.validate()?;
        Ok(Some(this))
    }
    pub(super) fn validate(&self) -> Result<()> {
        if !self.launcher.is_absolute()
            || !digest_ok(&self.launcher_sha256)
            || self.launcher.components().any(|c| c == std::path::Component::ParentDir)
        {
            return Err(Fault::conflict());
        }
        Ok(())
    }
    pub(super) fn guard(&self) -> Result<()> {
        if transaction::target(&self.launcher)? != self.launcher
            || hash(&std::fs::read(&self.launcher).map_err(Fault::io)?) != self.launcher_sha256
        {
            return Err(Fault::conflict());
        }
        Ok(())
    }
}
fn label(spec: &Spec) -> String {
    crate::launchd::label_in(spec.instance.as_deref(), spec.role())
}
#[cfg(target_os = "macos")]
fn path(spec: &Spec) -> PathBuf {
    spec.profile.join("Library/LaunchAgents").join(format!("{}.plist", label(spec)))
}
fn argv(spec: &Spec) -> Vec<String> {
    vec![
        spec.launchd.as_ref().expect("launchd context").launcher.to_string_lossy().into_owned(),
        spec.executable.to_string_lossy().into_owned(),
        "service-run".into(),
        "--context".into(),
        spec.context().to_string_lossy().into_owned(),
        "--context-sha256".into(),
        spec.sha(),
    ]
}
fn environment(spec: &Spec) -> BTreeMap<String, String> {
    let mut env = spec.environment.clone();
    for (key, value) in [("HOME", &spec.profile), ("AAA_CONFIG_DIR", &spec.config), ("AAA_STATE_DIR", &spec.state)] {
        env.insert(key.into(), value.to_string_lossy().into_owned());
    }
    if let Some(instance) = &spec.instance {
        env.insert("AAA_INSTANCE".into(), instance.clone());
    }
    env
}
pub(super) fn plist(spec: &Spec) -> String {
    let env: Vec<_> = environment(spec).into_iter().collect();
    crate::launchd::render(&label(spec), &argv(spec), &env, &spec.directory().join("service.log"), spec.automatic)
}

#[cfg(target_os = "macos")]
pub(super) fn apply(plan: &InstallPlan, id: &str, lease: &authorization::Lease) -> Result<bool> {
    if plan.context.platform.os != InstallOs::Macos || plan.choices.service != Some(InstallService::Launchd) {
        return Err(failure("unsupported_service_platform", 5));
    }
    native::available()?;
    let profile = aaa_config::paths::home().canonicalize().map_err(Fault::io)?;
    let mut env = BTreeMap::new();
    for key in ["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TERM"] {
        if let Ok(value) = std::env::var(key) {
            env.insert(key.into(), value);
        }
    }
    for (key, value) in [("CODEX_HOME", &plan.choices.codex_home), ("CLAUDE_CONFIG_DIR", &plan.choices.claude_home)] {
        if let Some(value) = value {
            env.insert(key.into(), value.clone());
        }
    }
    let journal = read_journal(Path::new(&plan.context.state_dir), id, plan.choices.instance.as_deref())?;
    let installed = transaction::installed_plan(Path::new(&plan.context.state_dir), &journal)?;
    let mut changed = false;
    for role in roles(plan) {
        let mut job = Job::prepare(&installed, id, role, profile.clone(), env.clone())?;
        native::verify_launcher(&job.spec)?;
        let mut manager = native::Launchd;
        let observed = manager.query(&job.spec)?;
        let same = observed.as_ref().is_some_and(|o| o.configuration == Configuration::desired(&job.spec));
        let ready = same
            && if plan.choices.start == Some(InstallStart::Now) {
                job.record.phase == ServicePhase::Ready
                    && observed.as_ref().is_some_and(|o| verify_ready(&job.spec, &mut manager, o).is_ok())
            } else {
                matches!(job.record.phase, ServicePhase::Registered | ServicePhase::Ready)
            };
        if ready {
            continue;
        }
        lease.valid_at(aaa_common::util::now_ms())?;
        job.register(&mut manager, None)?;
        if plan.choices.start == Some(InstallStart::Now) {
            lease.valid_at(aaa_common::util::now_ms())?;
            job.start(&mut manager)?;
        }
        changed = true;
    }
    Ok(changed)
}
#[cfg(target_os = "macos")]
pub(super) fn observe(spec: &Spec, record: &Record, plan: &InstallPlan, result: &mut InstallResult) -> Result<()> {
    if record.phase == ServicePhase::Removed {
        record_status(spec, record, InstallCheckState::NotRequested, result);
        return Ok(());
    }
    if plan.choices.start == Some(InstallStart::Deferred) && record.phase == ServicePhase::Registered {
        match native::Launchd.query(spec) {
            Ok(Some(o)) if o.configuration == Configuration::desired(spec) => {
                result.checks.push(InstallCheck {
                    name: format!("service_{}_registered", spec.role()),
                    state: InstallCheckState::Passed,
                });
                result.checks.push(InstallCheck {
                    name: format!("service_{}_ready", spec.role()),
                    state: InstallCheckState::NotRequested,
                });
                result.changes.push(InstallChange {
                    kind: "service".into(),
                    action: "register".into(),
                    state: if record.owned { InstallChangeState::Applied } else { InstallChangeState::Unchanged },
                });
                Ok(())
            }
            Ok(Some(_)) => Err(Fault::conflict()),
            Ok(None) => {
                incomplete(result, &spec.id, "service_registration_missing");
                Ok(())
            }
            Err(e) if e.exit == 3 => {
                incomplete(result, &spec.id, e.code);
                Ok(())
            }
            Err(e) => Err(e),
        }
    } else {
        observe_service(spec, record, &mut native::Launchd, result)
    }
}
#[cfg(target_os = "macos")]
pub(super) fn dispatch(context: &Path, sha: &str) -> std::process::ExitCode {
    runtime::dispatch(context, sha)
}

#[cfg(test)]
mod tests;
