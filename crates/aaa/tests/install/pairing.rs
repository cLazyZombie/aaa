//! 첫 Controller는 실제 W0 pairing을 redeem해야 한다. 코드 표시 ACK는 완료가 아니다.
use super::authorization::{redact_private, terminal_observed, terminal_steps};
use super::*;
#[test]
fn first_controller_pairing_waits_for_real_redemption_and_status_observes_revocation() {
    let lab = Lab::new();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let host = rt.block_on(async {
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        aaa_common::private_file::directory(&lab.0.join("state")).unwrap();
        let host = aaa_host::Host::open(cfg, lab.0.join("state/host")).unwrap();
        aaa_host::front::start(&host).await.unwrap();
        host
    });
    let mut args: Vec<String> = [
        "install",
        "--role",
        "host",
        "--name",
        "host-a",
        "--local-agent",
        "disabled",
        "--project-mode",
        "skip",
        "--agent-listen",
        "127.0.0.1:17430",
        "--web",
        "local",
        "--web-listen",
        "127.0.0.1:17431",
        "--tailscale-serve",
        "off",
        "--pairing",
        "controller",
        "--service",
        "none",
        "--start",
        "deferred",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    let approval = lab.0.join("approval.json");
    let mut issue = args.clone();
    issue.extend([
        "--prompt".into(),
        "plain".into(),
        "authorize".into(),
        "--out".into(),
        approval.to_string_lossy().into_owned(),
    ]);
    let (code, text) = terminal_steps(&lab, &issue, &[("정확히 yes", "yes\r")], false);
    assert_eq!(code, 0, "{}", redact_private(&text));
    let auth: Value = serde_json::from_slice(&std::fs::read(&approval).unwrap()).unwrap();
    let id = auth["claims"]["installation_id"].as_str().unwrap();
    args.extend(["--authorization-file".into(), approval.to_string_lossy().into_owned()]);
    let mut automated = args.clone();
    automated.extend(["--non-interactive".into(), "--yes".into(), "--json-stream".into()]);
    let output = lab.run(&automated);
    let lines: Vec<Value> =
        String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let result = &lines.last().unwrap()["result"];
    assert_eq!(result["exit_code"], 3, "{result}");
    assert!(
        result["needs_user"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["code"] == "installation_first_device_pairing_required")
    );
    let mut token = None;
    let mut actual_code = None;
    let resume = vec!["install".into(), "--resume".into(), id.into()];
    let (code, text) = terminal_observed(&lab, &resume, &[], false, |text| {
        if token.is_some() {
            return;
        }
        if let Some((_, rest)) = text.split_once("첫 기기 Controller pairing 코드: ") {
            let code: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if code.len() == 6 {
                let redeemed = host.redeem_pairing(&code, "설치 owner").unwrap();
                assert_eq!(redeemed.1, aaa_proto::DeviceRole::Controller);
                token = Some(redeemed.0);
                actual_code = Some(code);
            }
        }
    });
    assert_eq!(code, 0, "{}", redact_private(&text));
    let token = token.expect("실제 pairing redeem 사건이 있어야 한다");
    let actual_code = actual_code.unwrap();
    assert!(host.redeem_pairing(&actual_code, "again").is_err());
    let observed = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(observed["exit_code"], 0, "{observed}");
    assert!(!serde_json::to_string(&observed).unwrap().contains(&token));
    let (device, _) = host.device_role(&token).unwrap();
    host.revoke_device(&device).unwrap();
    let before = snapshot(&lab.0);
    let observed = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(observed["exit_code"], 3, "{observed}");
    assert!(
        observed["needs_user"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["code"] == "installation_first_device_pairing_required")
    );
    assert!(snapshot(&lab.0) == before, "Controller 없음 조회는 새 코드/기기를 만들지 않는다");
}
