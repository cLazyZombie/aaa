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
