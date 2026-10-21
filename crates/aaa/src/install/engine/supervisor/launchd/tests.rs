//! 실제 파일·내구 journal owner. launchctl 호출만 대체하며 실제 등록은 필수 macOS CI가 소유한다.
#[cfg(target_os = "macos")]
mod macos {
    use super::super::*;
    use crate::install::engine::transaction::tests::Lab;
    use std::os::unix::fs::PermissionsExt;

    fn prepared(lab: &Lab) -> (InstallPlan, String) {
        let mut plan = lab.plan("state");
        let launcher = lab.0.join("launcher & 한글");
        private::write(&launcher, b"immutable launcher owner fixture").unwrap();
        std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755)).unwrap();
        plan.choices.service = Some(InstallService::Launchd);
        plan.choices.launcher = Some(launcher.to_string_lossy().into_owned());
        plan = aaa_config::install::plan(plan.choices, plan.context).unwrap();
        lab.configure(plan)
    }
    #[derive(Default)]
    struct Os {
        creates: usize,
        fail_after_create: bool,
    }
    impl Manager for Os {
        fn query(&mut self, spec: &Spec) -> Result<Option<Observation>> {
            match private::read(&path(spec), 128 * 1024) {
                Ok(bytes) if bytes == plist(spec).as_bytes() => {
                    Ok(Some(Observation { configuration: Configuration::desired(spec), running: false, pid: 0 }))
                }
                Ok(_) => Err(Fault::conflict()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(Fault::io(e)),
            }
        }
        fn create(&mut self, spec: &Spec, password: Option<&Password>) -> Result<()> {
            assert!(password.is_none());
            self.creates += 1;
            native::install_plist(spec)?;
            if self.fail_after_create {
                return Err(failure("service_operation_failed", 1));
            }
            Ok(())
        }
        fn start(&mut self, spec: &Spec) -> Result<Observation> {
            self.query(spec)?.ok_or_else(Fault::conflict)
        }
        fn stop(&mut self, _: &Spec) -> Result<()> {
            Ok(())
        }
        fn remove(&mut self, spec: &Spec) -> Result<()> {
            std::fs::remove_file(path(spec)).map_err(Fault::io)
        }
        fn credentials(&mut self, _: &Spec, _: &Password) -> Result<()> {
            Err(Fault::conflict())
        }
        fn process(&mut self, _: &Spec, _: u32) -> Result<Option<Process>> {
            Ok(None)
        }
    }
    #[test]
    fn launchd_registration_resumes_the_actual_file_transaction_without_replacing_foreign_plists() {
        let lab = Lab::new();
        let (plan, id) = prepared(&lab);
        let mut job = Job::prepare(&plan, &id, InstallRole::Agent, lab.0.clone(), BTreeMap::new()).unwrap();
        let mut os = Os { fail_after_create: true, ..Default::default() };
        assert!(job.register(&mut os, None).is_err());
        assert!(job.record.phase == ServicePhase::Registering);
        let bytes = std::fs::read(path(&job.spec)).unwrap();
        drop(job);
        let mut job = Job::prepare(&plan, &id, InstallRole::Agent, lab.0.clone(), BTreeMap::new()).unwrap();
        job.register(&mut os, None).unwrap();
        assert_eq!(os.creates, 1);
        assert!(job.record.owned);
        assert_eq!(std::fs::read(path(&job.spec)).unwrap(), bytes);
        assert_eq!(job.start(&mut os).unwrap_err().code, "service_ready_unverified");
        assert!(!job.record.logon_right_may_remain);
        let file = path(&job.spec);
        private::write(&file, b"foreign edit").unwrap();
        assert!(job.remove(&mut os).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"foreign edit");
        assert!(native::install_plist(&job.spec).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), b"foreign edit");
    }
    #[test]
    fn launchd_plist_preserves_arguments_environment_and_instance_and_does_not_accept_fake_ready() {
        let lab = Lab::new();
        let (plan, id) = prepared(&lab);
        let mut env = BTreeMap::new();
        env.insert("PATH".into(), "/opt/example/bin:/usr/bin".into());
        let mut job = Job::prepare(&plan, &id, InstallRole::Agent, lab.0.clone(), env).unwrap();
        let mut os = Os::default();
        job.register(&mut os, None).unwrap();
        let out = std::process::Command::new("/usr/bin/plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(path(&job.spec))
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(v["Label"], "dev.aaa.lab.agent");
        assert_eq!(v["ProgramArguments"][0], plan.choices.launcher.as_deref().unwrap());
        assert_eq!(v["ProgramArguments"][1], plan.context.bin_path);
        assert_eq!(v["ProgramArguments"][2], "service-run");
        assert_eq!(v["EnvironmentVariables"]["AAA_INSTANCE"], "lab");
        assert_eq!(v["EnvironmentVariables"]["AAA_STATE_DIR"], plan.context.state_dir);
        assert_eq!(v["LimitLoadToSessionType"], "Aqua");
        assert_eq!(v["ProcessType"], "Interactive");
        let proof = Ready {
            schema: 1,
            context_sha256: job.spec.sha(),
            account_sha256: hash(job.spec.account.as_bytes()),
            profile_sha256: hash(job.spec.profile.to_string_lossy().as_bytes()),
            binary_sha256: job.spec.binary_sha256.clone(),
            pid: std::process::id(),
            connected: true,
            process_created: 1,
            session: 1,
            elevated: false,
            boot: aaa_common::util::boot_id(),
        };
        private::write(&job.spec.directory().join("ready.json"), &serde_json::to_vec(&proof).unwrap()).unwrap();
        assert_eq!(job.start(&mut os).unwrap_err().code, "service_ready_unverified");
        assert!(native::process(&job.spec, std::process::id(), std::process::id()).is_err());
        let other = job.spec.profile.join("Library/LaunchAgents/dev.aaa.other.agent.plist");
        private::write(&other, b"other instance").unwrap();
        let launcher = job.spec.launchd.as_ref().unwrap().launcher.clone();
        let old = std::fs::read(&launcher).unwrap();
        job.remove(&mut os).unwrap();
        assert_eq!(std::fs::read(&other).unwrap(), b"other instance");
        assert_eq!(std::fs::read(launcher).unwrap(), old);
    }
}
