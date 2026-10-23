//! 일회성 Windows CI의 SYSTEM OpenSSH → 실제 install → 사용자 SCM → TLS 등록 owner.
//! 가짜 계정 암호는 CI 부모의 anonymous stdin pipe에서만 받고 출력·파일·환경 변수에 넣지 않는다.
#![cfg(windows)]
use super::*;
use aaa_proto::{DeviceRole, FrontRequest, FrontResponse, MachineSetupPassword, MachineSetupSettings};
use std::io::Read;

async fn leave(h: &Arc<Host>, id: &str, phase: MachineSetupPhase) -> aaa_proto::MachineSetupView {
    let mut changed = h.enroll_changed.subscribe();
    tokio::time::timeout(Duration::from_secs(900), async {
        loop {
            let view = h.setup_load(id).unwrap().view;
            if view.phase != phase {
                return view;
            }
            changed.changed().await.unwrap();
        }
    })
    .await
    .expect("Windows SSH 단계 완료 사건")
}

fn no_secret(root: &Path, password: &MachineSetupPassword) {
    for entry in std::fs::read_dir(root).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            no_secret(&path, password);
        } else if path.is_file() {
            let bytes = std::fs::read(path).unwrap();
            assert!(
                !bytes.windows(password.0.len()).any(|w| w == password.0.as_bytes()),
                "영속 기록에 보호 입력이 있다"
            );
        }
    }
}

#[test]
#[ignore = "일회성 Windows CI 전용: scripts/ci-windows-sshd.ps1이 SYSTEM sshd·가짜 사용자·private stdin을 준비한다"]
fn system_sshd_installs_a_current_user_scm_agent_and_registers_it() {
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    assert!(std::env::var_os("AAA_SESSION_ID").is_none());
    let lab = PathBuf::from(std::env::var_os("AAA_WINDOWS_SSH_LAB").expect("CI 실험실"));
    let binary = PathBuf::from(std::env::var_os("AAA_WINDOWS_SSH_BINARY").expect("CI 실행 파일"));
    let mut text = String::new();
    std::io::stdin().read_to_string(&mut text).unwrap();
    let password = MachineSetupPassword(text);
    let wrong_password = MachineSetupPassword(aaa_common::util::new_id());
    assert!(!password.0.is_empty());
    let version = std::process::Command::new(&binary).arg("--version").output().unwrap();
    assert!(version.status.success());
    install::tests_binary::set(Some((binary, String::from_utf8(version.stdout).unwrap().trim().to_string())));
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    runtime.block_on(async {
        let mut config = aaa_config::HostConfig::default();
        config.host.agent_listen = "127.0.0.1:0".into();
        config.host.ssh_config = lab.join("ssh_config").to_string_lossy().into();
        config.projects.root = lab.join("projects").to_string_lossy().into();
        config.notify.web_push = false;
        let host = Host::open(config, lab.join("host")).unwrap();
        crate::link::start(&host).await.unwrap();
        let job = match host
            .call(DeviceRole::Controller, "device:ci", FrontRequest::MachineSetupStart { target: "ci".into() })
            .await
            .unwrap()
        {
            FrontResponse::MachineSetup { setup } => setup.job,
            _ => panic!("SSH 시작 응답"),
        };
        let view = leave(&host, &job, MachineSetupPhase::Connecting).await;
        assert_eq!(view.phase, MachineSetupPhase::Trust, "시스템 SSH 접속 단계: {:?}", view.error);
        host.call(
            DeviceRole::Controller,
            "device:ci",
            FrontRequest::MachineSetupTrust { job: job.clone(), host_key_sha256: view.host_keys[0].sha256.clone() },
        )
        .await
        .unwrap();
        let view = leave(&host, &job, MachineSetupPhase::Detecting).await;
        assert_eq!(view.phase, MachineSetupPhase::Ready, "Windows 감지 단계: {:?}", view.error);
        let detected = view.detected.unwrap();
        assert_eq!(detected.os, "windows");
        let settings = MachineSetupSettings {
            name: "ci-pc".into(),
            host: host.agent_listen.lock().unwrap().clone().unwrap(),
            work_root: format!("{}/aaa-work", detected.home.unwrap().replace('\\', "/")),
            work_mount: None,
            harness: vec![],
            login: aaa_proto::install::InstallLogin::Defer,
        };
        host.call(DeviceRole::Controller, "device:ci", FrontRequest::MachineSetupPlan { job: job.clone(), settings })
            .await
            .unwrap();
        let view = leave(&host, &job, MachineSetupPhase::Planning).await;
        // 이 단계에서 install-probe가 SYSTEM System32/OpenSSH/sshd.exe 계보를 실제 검증했다.
        assert_eq!(view.phase, MachineSetupPhase::Review, "Windows 검증 SSH 계획 단계: {:?}", view.error);
        let review = view.review.unwrap();
        assert!(review.service_password);
        let apply = |password| FrontRequest::MachineSetupApply {
            job: job.clone(),
            plan_sha256: review.plan_sha256.clone(),
            service_password: password,
        };
        for actor in ["local:thread@session", "local:coordinator@session", "worker:session"] {
            assert_eq!(
                host.call(DeviceRole::Local, actor, apply(Some(password.clone()))).await.unwrap_err().code,
                ErrorCode::Forbidden
            );
        }
        // 암호 없는 첫 시도는 같은 설치 ID에서 멈춰야 한다. 이어서 전용 입력을 넣어 재개한다.
        host.call(DeviceRole::Controller, "device:ci", apply(None)).await.unwrap();
        let pending = leave(&host, &job, MachineSetupPhase::Installing).await;
        assert_eq!(pending.phase, MachineSetupPhase::NeedsUser, "현지 입력 대기 단계");
        let original = pending.install.unwrap().installation_id.unwrap();
        host.call(DeviceRole::Controller, "device:ci", apply(Some(wrong_password.clone()))).await.unwrap();
        let rejected = leave(&host, &job, MachineSetupPhase::Installing).await;
        assert_eq!(rejected.phase, MachineSetupPhase::NeedsUser, "잘못된 암호 뒤 재입력 대기");
        let rejected = rejected.install.unwrap();
        assert_eq!(rejected.installation_id.as_deref(), Some(original.as_str()));
        assert!(
            rejected.result.as_ref().unwrap().needs_user.iter().any(|n| n.code == "service_account_auth_unavailable")
        );
        host.call(DeviceRole::Controller, "device:ci", apply(Some(password.clone()))).await.unwrap();
        let installed = leave(&host, &job, MachineSetupPhase::Installing).await;
        assert_eq!(installed.phase, MachineSetupPhase::Installed, "실제 Windows SCM 적용: {:?}", installed.error);
        let result = installed.install.unwrap();
        assert_eq!(result.installation_id.as_deref(), Some(original.as_str()), "같은 설치 ID 재개");
        assert_eq!(result.machine.as_deref(), Some("ci-pc"));
        let machine = host.resolve_machine(Some("ci-pc")).unwrap();
        assert!(host.conn_of(&machine).is_some(), "SCM agent의 실제 TLS 연결");
        let views = serde_json::to_string(&host.setup_list().unwrap()).unwrap();
        assert!(!views.contains(&password.0), "조회에 보호 입력이 있다");
    });
    no_secret(&lab, &password);
    no_secret(&lab, &wrong_password);
}
