//! 외부 SCM만 모형이다. 실제 I2 파일/잠금/private journal과 같은 ID 재개를 소유한다.
use super::super::transaction::tests::{Lab, tree};
use super::*;

#[derive(Default)]
struct Os {
    config: Option<Configuration>,
    creates: usize,
    updates: usize,
    removes: usize,
    fail_after_create: bool,
    credentials_fail: bool,
}
impl Manager for Os {
    fn query(&mut self, _: &Spec) -> Result<Option<Observation>> {
        Ok(self.config.clone().map(|configuration| Observation { configuration, running: true, pid: 17 }))
    }
    fn create(&mut self, spec: &Spec, password: &Password) -> Result<()> {
        assert!(!password.0.is_empty());
        self.creates += 1;
        self.config = Some(Configuration::desired(spec));
        if self.fail_after_create {
            return Err(failure("service_operation_failed", 1));
        }
        Ok(())
    }
    fn start(&mut self, spec: &Spec) -> Result<Observation> {
        self.query(spec)?.ok_or_else(Fault::conflict)
    }
    fn remove(&mut self, _: &Spec) -> Result<()> {
        self.removes += 1;
        self.config = None;
        Ok(())
    }
    fn credentials(&mut self, _: &Spec, _: &Password) -> Result<()> {
        if self.credentials_fail {
            return Err(failure("service_operation_failed", 1));
        }
        self.updates += 1;
        Ok(())
    }
    fn process_matches(&mut self, _: &Spec, _: u32) -> Result<bool> {
        Ok(false)
    }
}
fn reopen(lab: &Lab, plan: &InstallPlan, id: &str) -> Result<Job> {
    if cfg!(windows) {
        Job::prepare(plan, id, InstallRole::Agent, lab.0.clone(), BTreeMap::new())
    } else {
        Job::from_configured_files(plan, id, InstallRole::Agent, lab.0.clone(), BTreeMap::new())
    }
}
fn prepare(lab: &Lab) -> (Job, InstallPlan, String) {
    let (plan, id) = lab.configured_service();
    let job = reopen(lab, &plan, &id).unwrap();
    (job, plan, id)
}
fn password() -> Password {
    Password::new("fake-service-password-never-record".encode_utf16().collect()).unwrap()
}

#[test]
fn interrupted_registration_reconciles_actual_manager_without_creating_twice_or_saving_password() {
    let lab = Lab::new();
    let (mut job, plan, id) = prepare(&lab);
    let mut os = Os { fail_after_create: true, ..Default::default() };
    assert!(job.register(&mut os, Some(&password())).is_err());
    assert!(job.record.phase == ServicePhase::Registering);
    assert!(job.record.logon_right_may_remain);
    drop(job);
    let mut job = reopen(&lab, &plan, &id).unwrap();
    // OS 등록 직후 journal 전 중단이다. 암호를 다시 받거나 API를 다시 호출하지 않는다.
    job.register(&mut os, None).unwrap();
    assert_eq!(os.creates, 1);
    assert_eq!(os.updates, 0);
    assert!(job.record.owned);
    assert_eq!(job.start(&mut os).unwrap_err().code, "service_ready_unverified");
    assert!(job.record.phase == ServicePhase::Starting, "SCM RUNNING만으로 ready가 아니다");
    for (_, (bytes, _, _)) in tree(&lab.0) {
        assert!(!String::from_utf8_lossy(&bytes).contains("fake-service-password-never-record"));
        let utf16: Vec<_> = "fake-service-password-never-record".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(!bytes.windows(utf16.len()).any(|slice| slice == utf16));
    }
    drop(job);
    let before = tree(&lab.0);
    let state = Path::new(&plan.context.state_dir);
    let result = super::super::status(state, plan.choices.instance.as_deref(), &id).unwrap();
    assert!(result.status == InstallStatus::NeedsUser);
    assert!(result.checks.iter().any(|c| c.name == "service_agent_ready" && c.state == InstallCheckState::NotRun));
    assert_eq!(tree(&lab.0), before, "조회가 OS 잠금/기록/보정을 만들면 안 된다");
}

#[test]
fn existing_service_password_is_preserved_and_foreign_or_changed_service_is_not_removed() {
    let lab = Lab::new();
    let (mut job, plan, id) = prepare(&lab);
    let mut os = Os { config: Some(Configuration::desired(&job.spec)), ..Default::default() };
    job.register(&mut os, None).unwrap();
    assert!(!job.record.owned);
    assert_eq!(os.creates, 0);
    assert_eq!(os.updates, 0);
    assert_eq!(job.remove(&mut os).unwrap_err().code, "service_not_owned");
    assert_eq!(os.removes, 0);
    drop(job);
    let other = Lab::new();
    let (mut owned, _, _) = prepare(&other);
    let mut os = Os::default();
    owned.register(&mut os, Some(&password())).unwrap();
    os.config.as_mut().unwrap().account = "another-account".into();
    assert!(owned.remove(&mut os).is_err());
    assert_eq!(os.removes, 0);
    drop(owned);
    // 같은 ID/manifest라도 요청 계정/설정을 바꾸면 이전 작업을 채택하지 않는다.
    let mut changed = plan.clone();
    changed.context.target_account = "another-account".into();
    assert!(reopen(&lab, &changed, &id).is_err());
}

#[test]
fn service_phase_keeps_file_resource_locks_and_removal_and_credentials_resume_preserve_context() {
    let lab = Lab::new();
    let (mut job, plan, id) = prepare(&lab);
    assert!(reopen(&lab, &plan, &id).is_err());
    let mut os = Os::default();
    job.register(&mut os, Some(&password())).unwrap();
    os.credentials_fail = true;
    assert!(job.credentials(&mut os, &password()).is_err());
    assert!(job.record.credentials_pending);
    drop(job);
    let mut job = reopen(&lab, &plan, &id).unwrap();
    os.credentials_fail = false;
    job.credentials(&mut os, &password()).unwrap();
    assert_eq!(os.updates, 1);
    assert!(!job.record.credentials_pending);
    job.remove(&mut os).unwrap();
    job.remove(&mut os).unwrap();
    assert_eq!(os.removes, 1);
    assert!(Path::new(&plan.context.bin_path).is_file(), "서비스 제거가 사용자 파일/작업을 지우지 않는다");
    assert!(job.record.logon_right_may_remain, "LSA 권한 제거를 추정하지 않는다");
}

#[test]
fn ready_record_cannot_substitute_for_actual_pid_account_context_or_process_observation() {
    let lab = Lab::new();
    let (mut job, _, _) = prepare(&lab);
    let mut os = Os::default();
    job.register(&mut os, Some(&password())).unwrap();
    let proof = Ready {
        schema: 1,
        context_sha256: job.spec.sha(),
        account_sha256: hash(job.spec.account.as_bytes()),
        profile_sha256: hash(job.spec.profile.to_string_lossy().as_bytes()),
        binary_sha256: job.spec.binary_sha256.clone(),
        pid: 17,
        session: 0,
        elevated: false,
        boot: aaa_common::util::boot_id(),
    };
    private::write(&job.spec.directory().join("ready.json"), &serde_json::to_vec(&proof).unwrap()).unwrap();
    assert_eq!(
        job.start(&mut os).unwrap_err().code,
        "service_ready_unverified",
        "proof 파일만으로 process를 만들어 내지 않는다"
    );
    assert!(job.record.phase == ServicePhase::Starting);
    let mut spec = job.spec.clone();
    spec.environment.insert("AAA_ALLOW_INSTALL".into(), "yes".into());
    assert!(spec.validate().is_err());
    assert!(Password::new(vec![1, 0, 2]).is_err());
    let mut bytes = vec![1, 2, 3];
    wipe(&mut bytes);
    assert_eq!(bytes, vec![0, 0, 0]);
}

#[cfg(windows)]
#[test]
fn native_windows_reads_real_identity_and_scm_and_preserves_quoted_unicode_arguments() {
    use windows_sys::Win32::{Foundation::LocalFree, UI::Shell::CommandLineToArgvW};
    let lab = Lab::new();
    let (job, _, _) = prepare(&lab);
    let identity = native::identity(std::process::id()).unwrap();
    assert_eq!(identity.sid, crate::install::platform::account().unwrap());
    assert!(identity.profile.is_dir());
    let mut spec = job.spec.clone();
    spec.instance = Some(format!("i5-{}", aaa_common::util::new_id().to_lowercase()));
    assert!(native::Scm.query(&spec).unwrap().is_none());
    assert!(
        !native::Scm.process_matches(&spec, std::process::id()).unwrap(),
        "다른 실행 파일 process를 이 설치의 서비스 증거로 쓰지 않는다"
    );
    // 실제 OS argv 파서가 임의 셸 실행 없이 공백·한글·따옴표·끝 역슬래시를 보존한다.
    let args = [r"C:\한글 공백\aaa.exe", "service-run", "a\"b\\", "&%!()", ""];
    let command = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let mut count = 0;
    let ptr = unsafe { CommandLineToArgvW(native::wide(std::ffi::OsStr::new(&command)).as_ptr(), &mut count) };
    assert!(!ptr.is_null());
    let mut values = Vec::new();
    for i in 0..count as usize {
        let value = unsafe { *ptr.add(i) };
        let mut n = 0;
        while unsafe { *value.add(n) } != 0 {
            n += 1;
        }
        values.push(String::from_utf16(unsafe { std::slice::from_raw_parts(value, n) }).unwrap());
    }
    unsafe {
        LocalFree(ptr.cast());
    }
    assert_eq!(values, args);
}
