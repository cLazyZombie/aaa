use super::*;
use crate::install::engine::transaction::tests::{Lab, tree};

#[test]
fn consumed_approval_has_a_durable_public_id_before_file_writes_and_resumes_after_preparation() {
    let lab = Lab::new();
    let plan = lab.plan("state");
    let state = PathBuf::from(&plan.context.state_dir);
    let target = PathBuf::from(&plan.context.bin_path);
    let issuer = Authority::initialize(&state).unwrap();
    let approval = issuer.issue(&plan, transaction::permissions(&plan).unwrap(), 1000, 100).unwrap();
    let id = approval.claims.installation_id.clone();
    let mut run = Execution::begin(plan.clone(), &approval, 1001, false).unwrap();
    let before = tree(&lab.0);
    let status = observe(&state, Some("lab"), &id).unwrap().unwrap();
    assert_eq!(status.installation_id.as_deref(), Some(id.as_str()));
    assert_eq!(status.status, InstallStatus::NeedsUser);
    assert_eq!(status.resume.unwrap().installation_id, id);
    assert_eq!(tree(&lab.0), before, "status가 없는 lock/intent를 만들면 안 된다");
    assert!(!target.exists());
    let prepared = run.prepare_files().unwrap();
    assert!(!target.exists(), "내구 ID 저장까지 실제 대상은 적용하지 않는다");
    drop(prepared);
    drop(run);
    let mut resumed = Execution::begin(plan.clone(), &approval, 1002, true).unwrap();
    let result = resumed.files().unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.installation_id.as_deref(), Some(id.as_str()));
    assert_eq!(std::fs::read(&target).unwrap(), b"verified executable bytes");
    drop(resumed);
    let before = tree(&lab.0);
    let status = observe(&state, Some("lab"), &id).unwrap().unwrap();
    assert_eq!(status.exit_code, 0);
    assert_eq!(status.installation_id.as_deref(), Some(id.as_str()));
    assert_eq!(tree(&lab.0), before);
    assert!(Execution::begin(plan, &approval, 1002, false).is_err(), "소비된 승인을 새 apply로 쓰지 않는다");
    assert_eq!(tree(&lab.0), before);
}

#[test]
fn invalid_approval_and_changed_intent_are_rejected_without_creating_or_rewriting_jobs() {
    let lab = Lab::new();
    let plan = lab.plan("state");
    let state = PathBuf::from(&plan.context.state_dir);
    let issuer = Authority::initialize(&state).unwrap();
    let approval = issuer.issue(&plan, transaction::permissions(&plan).unwrap(), 1000, 100).unwrap();
    let id = approval.claims.installation_id.clone();
    let mut forged = approval.clone();
    forged.signature = "not-a-signature".into();
    let before = tree(&lab.0);
    assert!(Execution::begin(plan.clone(), &forged, 1001, false).is_err());
    assert_eq!(tree(&lab.0), before);
    assert!(read(&state, &id).unwrap().is_none(), "위조 승인은 durable execution을 만들지 않는다");
    drop(Execution::begin(plan.clone(), &approval, 1001, false).unwrap());
    let mut record = read(&state, &id).unwrap().unwrap();
    record.plan_sha256 = hash(b"different plan");
    private::write(&record_path(&state, &id).unwrap(), &serde_json::to_vec(&record).unwrap()).unwrap();
    let before = tree(&lab.0);
    assert!(Execution::begin(plan, &approval, 1002, true).is_err());
    assert_eq!(tree(&lab.0), before);
    assert!(observe(&state, Some("other"), &id).is_err());
    assert_eq!(tree(&lab.0), before);
}

#[test]
fn status_rejects_an_execution_intent_repointed_to_a_different_real_completed_job() {
    let lab = Lab::new();
    let plan = lab.plan("state");
    let state = PathBuf::from(&plan.context.state_dir);
    let issuer = Authority::initialize(&state).unwrap();
    let first = issuer.issue(&plan, transaction::permissions(&plan).unwrap(), 1000, 100).unwrap();
    let mut execution = Execution::begin(plan.clone(), &first, 1001, false).unwrap();
    execution.files().unwrap();
    drop(execution);
    let mut choices = plan.choices.clone();
    choices.name = Some("mac-b".into());
    choices.mode = Some(InstallMode::Repair);
    let changed = aaa_config::install::plan(choices, plan.context.clone()).unwrap();
    let second = issuer.issue(&changed, transaction::permissions(&changed).unwrap(), 1001, 100).unwrap();
    let mut execution = Execution::begin(changed, &second, 1002, false).unwrap();
    assert_eq!(execution.files().unwrap().exit_code, 0);
    drop(execution);
    let id = &first.claims.installation_id;
    let mut record = read(&state, id).unwrap().unwrap();
    record.files = Some(second.claims.installation_id.clone());
    private::write(&record_path(&state, id).unwrap(), &serde_json::to_vec(&record).unwrap()).unwrap();
    let before = tree(&lab.0);
    assert!(observe(&state, Some("lab"), id).is_err(), "다른 실제 완료 job도 원래 승인의 완료 증거가 아니다");
    assert!(Execution::begin(plan, &first, 1003, true).is_err());
    assert_eq!(tree(&lab.0), before);
}

#[test]
fn legacy_hook_alias_approval_keeps_its_signature_id_and_scope_through_status_and_resume() {
    let lab = Lab::new();
    let mut plan = lab.plan("legacy-state");
    let codex = lab.0.join("codex");
    private::directory(&codex).unwrap();
    plan.choices.harness = vec![InstallHarness::Codex];
    plan.choices.hooks = Some(InstallHooks::Install);
    plan.choices.login = Some(InstallLogin::Defer);
    plan.choices.codex_home = Some(codex.to_string_lossy().into_owned());
    plan.choices.codex_path = plan.choices.from.clone();
    let plan = aaa_config::install::plan(plan.choices, plan.context).unwrap();
    let state = PathBuf::from(&plan.context.state_dir);
    let canonical_scope = scope::all(&plan).unwrap();
    let mut legacy_scope = canonical_scope.clone();
    // v0.1.4 emitted the lexical integration home in signed hook targets and
    // canonicalized it only when saving the file journal. The issuer still signs the original bytes.
    // Windows canonicalize() adds a verbatim prefix. PathBuf::push normalizes dots
    // under that prefix, so construct the old ordinary Win32 spelling first.
    #[cfg(windows)]
    let lexical_codex = {
        let path = codex.to_str().unwrap();
        if let Some(unc) = path.strip_prefix(r"\\?\UNC\") {
            PathBuf::from(format!(r"\\{unc}"))
        } else {
            PathBuf::from(path.strip_prefix(r"\\?\").expect("canonical Windows path"))
        }
    };
    #[cfg(not(windows))]
    let lexical_codex = codex;
    for permission in &mut legacy_scope {
        if permission.operation == InstallOperation::ConfigureHook {
            let leaf = Path::new(&permission.target).file_name().unwrap();
            permission.target = lexical_codex.join(".").join(leaf).to_string_lossy().into_owned();
        }
    }
    assert!(
        legacy_scope.iter().zip(&canonical_scope).any(|(old, new)| old.target != new.target),
        "alias fixture: legacy={:?}, canonical={:?}",
        legacy_scope.iter().map(|p| &p.target).collect::<Vec<_>>(),
        canonical_scope.iter().map(|p| &p.target).collect::<Vec<_>>()
    );
    let issuer = Authority::initialize(&state).unwrap();
    let approval = issuer.issue(&plan, legacy_scope.clone(), 1000, 100).unwrap();
    let id = approval.claims.installation_id.clone();
    let signature = approval.signature.clone();
    let mut run = Execution::begin(plan.clone(), &approval, 1001, false).unwrap();
    assert_eq!(run.files().unwrap().exit_code, 0);
    assert!(run.lease.permits(&plan, &canonical_scope).is_ok());
    let different = lab.0.join("same-bytes-other-target");
    private::write(&different, b"verified executable bytes").unwrap();
    let mut elsewhere = canonical_scope.clone();
    elsewhere[0].target = different.to_string_lossy().into_owned();
    assert!(run.lease.permits(&plan, &elsewhere).is_err(), "같은 bytes도 다른 파일의 쓰기 권한은 아니다");

    let mut altered = canonical_scope;
    altered[0].after_sha256 = Some(hash(b"another executable"));
    assert!(run.lease.permits(&plan, &altered).is_err(), "경로 동등성이 SHA 변경을 허용하면 안 된다");
    drop(run);
    let before = tree(&lab.0);
    let status = observe(&state, Some("lab"), &id).unwrap().unwrap();
    assert_eq!(status.installation_id.as_deref(), Some(id.as_str()));
    assert_eq!(tree(&lab.0), before, "구판 기록의 status가 서명/기록을 다시 쓰면 안 된다");
    let original = issuer.issued(&id).unwrap();
    assert_eq!(original.authorization.signature, signature);
    assert!(original.authorization.claims.permissions == legacy_scope);
    assert!(!crate::install::engine::resume_requires_consent(&state, &id, 1002).unwrap());
    assert!(crate::install::engine::resume_requires_consent(&state, &id, 3000).unwrap());
    let renewed = issuer.refresh(&id, 3000, 100).unwrap();
    assert_eq!(renewed.claims.installation_id, id);
    assert!(renewed.claims.permissions == legacy_scope);
    assert_ne!(renewed.claims.nonce, approval.claims.nonce);
    let mut resumed = Execution::begin(plan, &renewed, 3001, true).unwrap();
    let result = resumed.files().unwrap();
    assert_eq!(result.exit_code, 0);
    assert_eq!(result.installation_id.as_deref(), Some(id.as_str()));
    assert_eq!(std::fs::read_dir(state.join("install/jobs")).unwrap().count(), 1);
}
