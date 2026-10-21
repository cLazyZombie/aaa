use super::*;
use aaa_proto::FrontRequest;
use std::sync::Arc;
struct Lab(PathBuf);
impl Lab {
    fn new() -> Self {
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!("aaa-install-host-{}", util::new_id()));
        private_file::directory(&dir).unwrap();
        Self(dir)
    }
    fn host(&self) -> Arc<Host> {
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        Host::open(cfg, self.0.join("host")).unwrap()
    }
    fn request(&self) -> InstallApprovalRequest {
        let path = |p: &str| self.0.join(p).to_string_lossy().into_owned();
        let choices = InstallChoices {
            role: Some(InstallRole::Agent),
            name: Some("agent-a".into()),
            instance: Some("lab".into()),
            config_dir: Some(path("config")),
            state_dir: Some(path("state")),
            work_root: Some(path("work")),
            work_storage: Some(InstallStorage::Internal),
            min_free_gb: Some(1),
            host: Some("example.ts.net:7430".into()),
            network: Some(InstallNetwork::Direct),
            enroll: Some(InstallEnroll::Grant),
            service: Some(InstallService::None),
            start: Some(InstallStart::Deferred),
            hooks: Some(InstallHooks::Preserve),
            harness: vec![InstallHarness::None],
            ..Default::default()
        };
        let context = InstallPlanContext {
            platform: InstallPlatform { os: InstallOs::Windows, arch: InstallArch::X86_64 },
            version: "0.1.2".into(),
            binary_sha256: digest(b"binary"),
            bin_path: path("bin/aaa.exe"),
            config_dir: path("config"),
            state_dir: path("state"),
            projects_root: path("projects"),
            default_shell: "cmd.exe".into(),
            effective_config_sha256: digest(b"config"),
            target_account: "S-1-5-21-1000".into(),
        };
        // Windows 경로 검증은 대상 OS 규칙을 쓰므로 이 host fixture도 Windows 절대 경로를 사용한다.
        let mut choices = choices;
        choices.config_dir = Some("C:/InstallLab/config".into());
        choices.state_dir = Some("C:/InstallLab/state".into());
        choices.work_root = Some("C:/InstallLab/work".into());
        let context = InstallPlanContext {
            bin_path: "C:/InstallLab/bin/aaa.exe".into(),
            config_dir: "C:/InstallLab/config".into(),
            state_dir: "C:/InstallLab/state".into(),
            projects_root: "C:/InstallLab/projects".into(),
            ..context
        };
        let plan = aaa_config::install::plan(choices, context).unwrap();
        let permissions = vec![InstallPermission {
            operation: InstallOperation::EnrollAgent,
            target: "host:example.ts.net:7430/agent:agent-a".into(),
            before_sha256: None,
            after_sha256: Some(aaa_config::install::plan_sha256(&plan)),
        }];
        InstallApprovalRequest {
            installation_id: None,
            expected_plan_sha256: aaa_config::install::plan_sha256(&plan),
            expected_scope_sha256: digest(&install_crypto::canonical(&permissions).unwrap()),
            permissions,
            plan,
            agent_identity: Some(digest(b"agent")),
        }
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
async fn approve(
    host: &Arc<Host>,
    role: DeviceRole,
    actor: &str,
    request: InstallApprovalRequest,
) -> Result<InstallApprovalView, ErrorBody> {
    match host.call(role, actor, FrontRequest::InstallApprove { request: Box::new(request) }).await? {
        FrontResponse::InstallApproval { approval } => Ok(approval),
        other => panic!("unexpected public response: {other:?}"),
    }
}
#[tokio::test]
async fn only_outside_people_issue_bound_approvals_and_refresh_cannot_widen_them() {
    let lab = Lab::new();
    let host = lab.host();
    let request = lab.request();
    for (role, actor) in [
        (DeviceRole::Reader, "device:reader"),
        (DeviceRole::Local, "worker:job"),
        (DeviceRole::Local, "evaluator:job"),
        (DeviceRole::Local, "detached:job"),
        (DeviceRole::Local, "local:cli@coordinator"),
        (DeviceRole::Local, "local:cli@worker"),
        (DeviceRole::Controller, "worker:device"),
    ] {
        assert_eq!(
            approve(&host, role, actor, request.clone()).await.unwrap_err().code,
            ErrorCode::Forbidden,
            "{actor}"
        );
        assert!(!root(&host.state).exists());
    }
    let mut changed = request.clone();
    changed.expected_plan_sha256 = digest(b"different plan");
    assert_eq!(
        approve(&host, DeviceRole::Controller, "device:person", changed).await.unwrap_err().code,
        ErrorCode::BadRequest
    );
    assert!(!root(&host.state).exists());
    let mut bad_name = request.clone();
    bad_name.plan.choices.name = Some("agent a".into());
    bad_name.plan = aaa_config::install::plan(bad_name.plan.choices, bad_name.plan.context).unwrap();
    bad_name.expected_plan_sha256 = aaa_config::install::plan_sha256(&bad_name.plan);
    bad_name.permissions = vec![aaa_config::install::enrollment_permission(&bad_name.plan).unwrap()];
    bad_name.expected_scope_sha256 = digest(&install_crypto::canonical(&bad_name.permissions).unwrap());
    assert_eq!(
        approve(&host, DeviceRole::Controller, "device:person", bad_name).await.unwrap_err().code,
        ErrorCode::BadRequest
    );
    assert!(!root(&host.state).exists(), "backend가 거부할 머신 이름을 발급 전에 검증한다");
    let view = approve(&host, DeviceRole::Controller, "device:person", request.clone()).await.unwrap();
    let package = export(&host.state, &view.installation_id).unwrap();
    let public = URL_SAFE_NO_PAD.decode(&package.issuer_public_key).unwrap();
    assert!(install_crypto::verify_authorization(
        &public,
        &package.authorization.claims,
        &package.authorization.signature
    ));
    assert!(install_crypto::verify_enrollment(&public, package.authorization.enrollment.as_ref().unwrap()));
    let public_response = serde_json::to_string(&view).unwrap();
    assert!(!public_response.contains(&package.authorization.signature));
    assert!(!public_response.contains(&package.authorization.enrollment.as_ref().unwrap().signature));
    let saved =
        private_file::read(&root(&host.state).join(format!("{}.json", view.installation_id)), 2 * 1024 * 1024).unwrap();
    for identity_change in [false, true] {
        let mut changed = request.clone();
        changed.installation_id = Some(view.installation_id.clone());
        if identity_change {
            changed.agent_identity = Some(digest(b"another agent"));
        } else {
            changed.permissions[0].target.push_str("/another");
            changed.expected_scope_sha256 = digest(&install_crypto::canonical(&changed.permissions).unwrap());
        }
        assert_eq!(
            approve(&host, DeviceRole::Local, "local:cli", changed).await.unwrap_err().code,
            ErrorCode::BadRequest
        );
        assert_eq!(
            private_file::read(&root(&host.state).join(format!("{}.json", view.installation_id)), 2 * 1024 * 1024)
                .unwrap(),
            saved
        );
    }
    let mut refresh = request;
    refresh.installation_id = Some(view.installation_id.clone());
    let refreshed = approve(&host, DeviceRole::Local, "local:cli", refresh).await.unwrap();
    assert_eq!(refreshed.installation_id, view.installation_id);
    assert_ne!(
        export(&host.state, &view.installation_id).unwrap().authorization.claims.nonce,
        package.authorization.claims.nonce
    );
}

#[tokio::test]
async fn grant_registration_is_atomic_bound_to_tls_identity_and_lost_response_lookup_respects_revocation() {
    let lab = Lab::new();
    let host = lab.host();
    let request = lab.request();
    let view = approve(&host, DeviceRole::Controller, "device:person", request.clone()).await.unwrap();
    let package = export(&host.state, &view.installation_id).unwrap();
    let grant = package.authorization.enrollment.unwrap();
    let req = InstallEnrollRequest {
        proto: aaa_proto::AGENT_LINK,
        machine_id: util::new_id(),
        machine_name: "agent-a".into(),
        grant,
    };
    let now = util::now_ms();
    let peer = request.agent_identity.unwrap();
    let mut tampered = req.clone();
    tampered.grant.signature = URL_SAFE_NO_PAD.encode([0; 64]);
    assert!(host.install_enroll(&peer, &tampered, now).is_err());
    assert!(host.install_enroll(&digest(b"other TLS peer"), &req, now).is_err());
    assert!(host.install_enroll(&peer, &req, req.grant.expires_at_ms).is_err());
    assert!(host.db.machines().unwrap().is_empty());
    assert!(host.install_lookup(&peer, &view.installation_id, &view.plan_sha256).unwrap().is_none());
    assert_eq!(host.install_enroll(&peer, &req, now).unwrap(), req.machine_id);
    let views = host.machine_views();
    assert_eq!(views.len(), 1, "backend Hello 전에도 승인해 등록한 머신이 Disconnected로 보여야 한다");
    assert_eq!(views[0].id, req.machine_id);
    assert_eq!(views[0].connection, aaa_proto::Connection::Disconnected);

    // 신뢰를 소비한 commit 뒤 응답 유실: 발급서 만료와 무관한 읽기 조회는 같은 실제 peer만 된다.
    assert_eq!(
        host.install_lookup(&peer, &view.installation_id, &view.plan_sha256).unwrap(),
        Some(req.machine_id.clone())
    );
    assert!(
        host.install_lookup(&digest(b"other TLS peer"), &view.installation_id, &view.plan_sha256).unwrap().is_none()
    );
    assert_eq!(host.install_enroll(&peer, &req, now).unwrap(), req.machine_id);
    let mut another = req.clone();
    another.machine_id = util::new_id();
    assert!(host.install_enroll(&peer, &another, now).is_err());
    assert_eq!(host.db.machines().unwrap().len(), 1);
    host.db.revoke_machine("agent-a").unwrap();
    assert!(host.install_lookup(&peer, &view.installation_id, &view.plan_sha256).unwrap().is_none());
    assert!(host.install_enroll(&peer, &req, now).is_err());
    assert!(host.db.machines().unwrap().is_empty());
}
