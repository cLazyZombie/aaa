//! 실제 target client ↔ host TLS. Ed25519, pin, grant 소비, 응답 유실 조회를 stub하지 않는다.
use super::*;
use aaa_common::{install_crypto, private_file, tls, util};
use aaa_proto::{DeviceRole, FrontRequest, FrontResponse, install::*};
use sha2::{Digest, Sha256};
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[tokio::test]
async fn verified_host_grant_uses_tls_pin_and_keeps_consumption_bound_to_same_machine() {
    let lab = Lab::new();
    let state = lab.0.join("agent");
    private_file::directory(&state).unwrap();
    let agent = tls::load_or_create_private(&state, "aaa-agent").unwrap();
    let machine = util::new_id();
    let mut cfg = aaa_config::HostConfig::default();
    cfg.notify.web_push = false;
    cfg.host.agent_listen = "127.0.0.1:0".into();
    let host = aaa_host::Host::open(cfg, lab.0.join("host")).unwrap();
    aaa_host::link::start(&host).await.unwrap();
    let address = host.agent_listen.lock().unwrap().clone().unwrap();
    let path = |p: &str| lab.0.join(p).to_string_lossy().into_owned();
    let choices = InstallChoices {
        role: Some(InstallRole::Agent),
        name: Some("agent-a".into()),
        instance: Some("lab".into()),
        config_dir: Some(path("config")),
        state_dir: Some(state.to_string_lossy().into_owned()),
        work_root: Some(path("work")),
        work_storage: Some(InstallStorage::Internal),
        min_free_gb: Some(1),
        host: Some(address.clone()),
        network: Some(InstallNetwork::Direct),
        enroll: Some(InstallEnroll::Grant),
        service: Some(InstallService::None),
        start: Some(InstallStart::Deferred),
        hooks: Some(InstallHooks::Preserve),
        harness: vec![InstallHarness::None],
        ..Default::default()
    };
    let context = InstallPlanContext {
        platform: InstallPlatform {
            os: if cfg!(windows) {
                InstallOs::Windows
            } else if cfg!(target_os = "macos") {
                InstallOs::Macos
            } else {
                InstallOs::Linux
            },
            arch: if cfg!(target_arch = "aarch64") { InstallArch::Aarch64 } else { InstallArch::X86_64 },
        },
        version: "0.1.2".into(),
        binary_sha256: digest(b"binary"),
        bin_path: path("bin/aaa"),
        config_dir: path("config"),
        state_dir: state.to_string_lossy().into_owned(),
        projects_root: path("projects"),
        default_shell: if cfg!(windows) { "cmd.exe".into() } else { "/bin/sh".into() },
        effective_config_sha256: digest(b"config"),
        target_account: private_file::account().unwrap(),
    };
    let plan = aaa_config::install::plan(choices, context).unwrap();
    let permissions = vec![InstallPermission {
        operation: InstallOperation::EnrollAgent,
        target: format!("host:{address}/agent:agent-a"),
        before_sha256: None,
        after_sha256: Some(aaa_config::install::plan_sha256(&plan)),
    }];
    let request = InstallApprovalRequest {
        installation_id: None,
        expected_plan_sha256: aaa_config::install::plan_sha256(&plan),
        expected_scope_sha256: digest(&install_crypto::canonical(&permissions).unwrap()),
        permissions,
        plan,
        agent_identity: Some(agent.fingerprint),
    };
    let FrontResponse::InstallApproval { approval } = host
        .call(DeviceRole::Controller, "device:person", FrontRequest::InstallApprove { request: Box::new(request) })
        .await
        .unwrap()
    else {
        panic!("approval view")
    };
    let package = aaa_host::installation::export(&host.state, &approval.installation_id).unwrap();
    let grant = package.authorization.enrollment.unwrap();
    let mut wrong = grant.clone();
    wrong.host_identity = digest(b"different host");
    let wrong_pin = aaa_agent::link::enroll_install(&state, &address, &machine, "agent-a", &wrong).await.unwrap_err();
    assert!(
        format!("{wrong_pin:#}").contains("host 인증서 fingerprint가 등록 때와 다르다"),
        "다른 grant guard가 아니라 TLS pin에서 거부해야 한다: {wrong_pin:#}"
    );
    assert!(!state.join("host.json").exists());
    assert!(host.db.machines().unwrap().is_empty());
    let pin = aaa_agent::link::enroll_install(&state, &address, &machine, "agent-a", &grant).await.unwrap();
    assert_eq!(pin.fingerprint, grant.host_identity);
    let rows = host.db.machines().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, machine);
    // 검증 SDK는 pin을 쓰지 않는다. 엔진의 승인된 before-image 대조 뒤만 저장한다.
    assert!(!state.join("host.json").exists());
    let recovered = aaa_agent::link::enroll_install(&state, &address, &machine, "agent-a", &grant).await.unwrap();
    assert_eq!(recovered.fingerprint, pin.fingerprint);
    assert_eq!(host.db.machines().unwrap().len(), 1);
    assert!(aaa_agent::link::enroll_install(&state, &address, &util::new_id(), "agent-a", &grant).await.is_err());
    host.call(DeviceRole::Local, "local:cli", FrontRequest::MachineRevoke { name: "agent-a".into() }).await.unwrap();
    assert!(aaa_agent::link::enroll_install(&state, &address, &machine, "agent-a", &grant).await.is_err());
    assert!(host.db.machines().unwrap().is_empty());
}
