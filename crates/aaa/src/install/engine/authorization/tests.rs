use super::*;
use crate::install::engine::transaction::tests::{Lab, tree};

fn permissions(plan: &InstallPlan) -> Vec<InstallPermission> {
    vec![InstallPermission {
        operation: InstallOperation::WriteFile,
        target: Path::new(&plan.context.config_dir).join("agent.toml").to_string_lossy().into_owned(),
        before_sha256: None,
        after_sha256: Some(hash(b"desired configuration")),
    }]
}

#[test]
fn issued_authorization_is_bound_to_live_plan_permissions_time_and_private_pin() {
    let lab = Lab::new();
    let plan = lab.plan("state");
    let state = Path::new(&plan.context.state_dir);
    assert!(Authority::open(state).is_err());
    assert!(!state.exists(), "검증은 발급자나 state를 만들지 않는다");
    let issuer = Authority::initialize(state).unwrap();
    let allowed = permissions(&plan);
    let authorization = issuer.issue(&plan, allowed.clone(), 1000, 100).unwrap();
    assert!(issuer.verify(&authorization, &plan, &allowed, 1000).is_ok());
    let issued = Authority::open(state).unwrap().issued(&authorization.claims.installation_id).unwrap();
    assert!(issued.plan == plan && bytes(&issued.authorization).unwrap() == bytes(&authorization).unwrap());
    let other = lab.plan("other-state");
    let mut changed_config = plan.clone();
    changed_config.context.effective_config_sha256 = hash(b"concurrent existing configuration");
    let before = tree(&lab.0);
    assert!(issuer.verify(&authorization, &changed_config, &allowed, 1000).is_err());
    assert!(issuer.verify(&authorization, &other, &permissions(&other), 1000).is_err());
    let mut different = allowed.clone();
    different[0].after_sha256 = Some(hash(b"different configuration"));
    assert!(issuer.verify(&authorization, &plan, &different, 1000).is_err());
    assert!(issuer.begin(&authorization, &plan, &allowed, 999, None).is_err());
    assert!(issuer.begin(&authorization, &plan, &allowed, 1100, None).is_err());
    assert_eq!(tree(&lab.0), before, "검증 실패가 lock/소비 기록을 바꾸면 안 된다");

    // 발급 기록의 hash만 변조해도 실제 서명 검사를 통과할 수 없다.
    let mut forged = authorization.clone();
    forged.signature = URL_SAFE_NO_PAD.encode([0u8; 64]);
    let record = Record {
        authorization_sha256: hash(&bytes(&forged).unwrap()),
        installation_id: forged.claims.installation_id.clone(),
        consumed: false,
    };
    private::write(&issuer.record_path(&forged.claims.nonce).unwrap(), &bytes(&record).unwrap()).unwrap();
    let before = tree(&lab.0);
    assert!(issuer.begin(&forged, &plan, &allowed, 1000, None).is_err());
    assert_eq!(tree(&lab.0), before);
}

#[test]
fn consumption_survives_reopen_and_only_explicit_same_id_resume_is_allowed() {
    let lab = Lab::new();
    let plan = lab.plan("state");
    let state = Path::new(&plan.context.state_dir);
    let issuer = Authority::initialize(state).unwrap();
    let allowed = transaction::permissions(&plan).unwrap();
    assert!(allowed.len() > 1, "단계별 부분 범위를 구분할 실제 파일 작업이 있어야 한다");
    let authorization = issuer.issue(&plan, allowed.clone(), 1000, 100).unwrap();
    let lease = issuer.begin(&authorization, &plan, &allowed, 1000, None).unwrap();
    assert_eq!(lease.id(), authorization.claims.installation_id);
    assert!(lease.permits(&plan, &allowed[..1]).is_ok(), "단계는 전체 승인 중 자기 작업 범위만 검사한다");
    assert_eq!(
        issuer.begin(&authorization, &plan, &allowed, 1000, Some(lease.id())).err().unwrap().exit,
        4,
        "실행 중 lease는 새 승인 요구가 아니라 busy/conflict다"
    );
    drop(lease);
    let reopened = Authority::open(state).unwrap();
    let before = tree(&lab.0);
    assert!(reopened.begin(&authorization, &plan, &allowed, 1000, None).is_err());
    assert!(reopened.begin(&authorization, &plan, &allowed, 1000, Some("different-job")).is_err());
    assert_eq!(tree(&lab.0), before);
    let resumed =
        reopened.begin(&authorization, &plan, &allowed, 1000, Some(&authorization.claims.installation_id)).unwrap();
    assert_eq!(resumed.id(), authorization.claims.installation_id);
    assert_eq!(tree(&lab.0), before, "읽기 재개 검증은 소비 기록을 다시 쓰지 않는다");
    drop(resumed);
    let renewed = reopened.refresh(&authorization.claims.installation_id, 1100, 100).unwrap();
    assert_eq!(renewed.claims.installation_id, authorization.claims.installation_id);
    assert_ne!(renewed.claims.nonce, authorization.claims.nonce);
    assert!(
        reopened.begin(&authorization, &plan, &allowed, 1100, Some(&authorization.claims.installation_id)).is_err()
    );
    let renewed_lease = reopened.begin(&renewed, &plan, &allowed, 1100, Some(&renewed.claims.installation_id)).unwrap();
    assert_eq!(renewed_lease.id(), authorization.claims.installation_id);
}

#[test]
fn linked_pin_and_changed_signer_are_untrusted_without_rewriting_them() {
    let lab = Lab::new();
    let plan = lab.plan("state");
    let state = Path::new(&plan.context.state_dir);
    let issuer = Authority::initialize(state).unwrap();
    let allowed = permissions(&plan);
    let authorization = issuer.issue(&plan, allowed.clone(), 1000, 100).unwrap();
    let key = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    private::write(&issuer.root.join("issuer.pk8"), key.as_ref()).unwrap();
    let before = tree(&lab.0);
    assert!(issuer.issue(&plan, allowed.clone(), 1000, 100).is_err());
    assert_eq!(tree(&lab.0), before);
    std::fs::hard_link(issuer.root.join("issuer.pin"), lab.0.join("pin-alias")).unwrap();
    let before = tree(&lab.0);
    assert!(Authority::open(state).is_err());
    assert!(issuer.begin(&authorization, &plan, &allowed, 1000, None).is_err());
    assert_eq!(tree(&lab.0), before);
}

#[test]
fn file_executor_rechecks_actual_scope_before_any_transaction_write() {
    for changed_after_approval in [false, true] {
        let lab = Lab::new();
        let plan = lab.plan("state");
        let state = PathBuf::from(&plan.context.state_dir);
        let allowed = transaction::permissions(&plan).unwrap();
        let issuer = Authority::initialize(&state).unwrap();
        let authorization = issuer.issue(&plan, allowed.clone(), 1000, 100).unwrap();
        let lease = issuer.begin(&authorization, &plan, &allowed, 1000, None).unwrap();
        if changed_after_approval {
            let config = Path::new(&plan.context.config_dir);
            private::directory(config).unwrap();
            private::write(&config.join("agent.toml"), b"schema = 1\n# concurrently edited\n").unwrap();
            let before = tree(&lab.0);
            assert!(transaction::AuthorizedTransaction::create(plan, &lease).is_err());
            assert_eq!(tree(&lab.0), before, "잘못된 scope로 journal/잠금/파일을 만들지 않는다");
        } else {
            let target = plan.context.bin_path.clone();
            let mut transaction = transaction::AuthorizedTransaction::create(plan, &lease).unwrap();
            let file_id = transaction.id().to_owned();
            assert!(!Path::new(&target).exists(), "worker가 ID를 기록하기 전에는 파일 적용을 시작하지 않는다");
            let configured = transaction.run().unwrap();
            assert_eq!(configured.installation_id.as_deref(), Some(file_id.as_str()));
            assert_eq!(configured.status, InstallStatus::Configured);
            assert_eq!(configured.exit_code, 0);
            assert_eq!(std::fs::read(target).unwrap(), b"verified executable bytes");
            let observed = status(&state, Some("lab"), configured.installation_id.as_deref().unwrap()).unwrap();
            assert_eq!(observed.status, InstallStatus::Configured);
        }
    }
}

#[test]
fn file_manifest_resumes_under_the_same_consumed_authorization() {
    let lab = Lab::new();
    let plan = lab.plan("state");
    let state = PathBuf::from(&plan.context.state_dir);
    let allowed = transaction::permissions(&plan).unwrap();
    assert!(allowed.iter().any(|p| p.operation == InstallOperation::PrepareWorkRoot));
    let issuer = Authority::initialize(&state).unwrap();
    let missing_root: Vec<_> =
        allowed.iter().filter(|p| p.operation != InstallOperation::PrepareWorkRoot).cloned().collect();
    let narrowed = issuer.issue(&plan, missing_root.clone(), 1000, 100).unwrap();
    let lease = issuer.begin(&narrowed, &plan, &missing_root, 1000, None).unwrap();
    let before = tree(&lab.0);
    assert!(transaction::AuthorizedTransaction::create(plan.clone(), &lease).is_err());
    assert_eq!(tree(&lab.0), before, "작업 root도 별도 승인 범위에 있어야 한다");
    drop(lease);

    let authorization = issuer.issue(&plan, allowed.clone(), 1000, 100).unwrap();
    let lease = issuer.begin(&authorization, &plan, &allowed, 1000, None).unwrap();
    let target = PathBuf::from(&plan.context.bin_path);
    let transaction = transaction::AuthorizedTransaction::create(plan.clone(), &lease).unwrap();
    let file_id = transaction.id().to_owned();
    drop(transaction);
    drop(lease);
    assert!(!target.exists());
    let (saved_plan, saved_scope) = transaction::resume_scope(&state, &file_id, Some("lab")).unwrap();
    assert!(saved_plan == plan && saved_scope == allowed);
    let reopened = Authority::open(&state).unwrap();
    let lease = reopened
        .begin(&authorization, &saved_plan, &saved_scope, 1001, Some(&authorization.claims.installation_id))
        .unwrap();
    let mut resumed = transaction::AuthorizedTransaction::resume(&state, &file_id, Some("lab"), &lease).unwrap();
    let result = resumed.run().unwrap();
    assert_eq!(result.status, InstallStatus::Configured);
    assert_eq!(std::fs::read(target).unwrap(), b"verified executable bytes");
    assert_eq!(result.installation_id.as_deref(), Some(file_id.as_str()));
}

#[test]
fn interrupted_preparation_rejects_untrusted_plan_before_recreating_missing_payload() {
    let lab = Lab::new();
    let plan = lab.plan("state");
    let state = PathBuf::from(&plan.context.state_dir);
    let allowed = transaction::permissions(&plan).unwrap();
    let issuer = Authority::initialize(&state).unwrap();
    let authorization = issuer.issue(&plan, allowed.clone(), 1000, 100).unwrap();
    let lease = issuer.begin(&authorization, &plan, &allowed, 1000, None).unwrap();
    let transaction = transaction::AuthorizedTransaction::create(plan.clone(), &lease).unwrap();
    assert_eq!(transaction.id(), lease.id());
    let dir = journal_dir(&state, transaction.id());
    drop(transaction);
    // 파일 적용 전 준비 순서의 제어 frame: journal/active 저장 전 상태를 복원한다.
    std::fs::remove_file(dir.join("journal.json")).unwrap();
    std::fs::remove_file(state.join("install/active.json")).unwrap();
    std::fs::remove_file(dir.join("payload-0")).unwrap();
    private::write(&dir.join("plan.json"), b"untrusted preparation bytes").unwrap();
    let before = tree(&lab.0);
    assert!(transaction::AuthorizedTransaction::create(plan, &lease).is_err());
    assert!(tree(&lab.0) == before, "불신 준비 파일을 확인하기 전에 다른 payload를 만들면 안 된다");
}
