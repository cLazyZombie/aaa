//! 실제 CLI/PTY·private 발급 파일 owner. 시험용 발급 flag나 엔진 export를 쓰지 않는다.
use super::*;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use sha2::Digest;
use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(super) fn redact_private(text: &str) -> String {
    text.lines()
        .map(|line| {
            for marker in ["확인 코드: ", "pairing 코드: "] {
                if let Some((prefix, _)) = line.split_once(marker) {
                    return format!("{prefix}{marker}[private]");
                }
            }
            line.replace(CANARY, "[private]")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn terminal(lab: &Lab, args: &[String], answer: Option<&str>, session: bool) -> (u32, String) {
    let steps: Vec<_> = answer.into_iter().map(|answer| ("정확히 yes", answer)).collect();
    terminal_steps(lab, args, &steps, session)
}
pub(super) fn terminal_steps(lab: &Lab, args: &[String], steps: &[(&str, &str)], session: bool) -> (u32, String) {
    terminal_observed(lab, args, steps, session, |_| {})
}
pub(super) fn terminal_observed(
    lab: &Lab,
    args: &[String],
    steps: &[(&str, &str)],
    session: bool,
    mut observe: impl FnMut(&str),
) -> (u32, String) {
    terminal_controlled(lab, args, steps, session, |_, text| observe(text))
}
pub(super) fn terminal_controlled(
    lab: &Lab,
    args: &[String],
    steps: &[(&str, &str)],
    session: bool,
    mut observe: impl FnMut(&mut dyn portable_pty::Child, &str),
) -> (u32, String) {
    let mut command = {
        #[cfg(target_os = "macos")]
        {
            // 기존 lab::terminal_command와 같은 XNU 제어 TTY drain 경계다.
            let mut command = CommandBuilder::new("/bin/sh");
            command.args(["-c", "exec 3<>/dev/tty || exit 125; exec 3>&-; exec \"$@\"", "aaa-test-pty"]);
            command.arg(BIN);
            command
        }
        #[cfg(not(target_os = "macos"))]
        {
            CommandBuilder::new(BIN)
        }
    };
    command.env_clear();
    let inherited = lab.command(&[]);
    for (name, value) in inherited.get_envs() {
        if let Some(value) = value {
            command.env(name, value);
        }
    }
    command.env("TERM", "xterm-256color");
    if session {
        command.env("AAA_SESSION_ID", "session-example");
    }
    command.args(args);
    let pair = native_pty_system().openpty(PtySize { rows: 36, cols: 120, pixel_width: 0, pixel_height: 0 }).unwrap();
    struct OwnedChild {
        child: Box<dyn portable_pty::Child + Send + Sync>,
        finished: bool,
    }
    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if !self.finished {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }
    let mut child = OwnedChild { child: pair.slave.spawn_command(command).unwrap(), finished: false };
    let pid = child.child.process_id().expect("소유한 PTY 자식 PID");
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut buf = [0; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut seen = Vec::new();
    let mut answered = 0;
    let mut cursor = 0;
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(bytes) => {
                seen.extend(bytes);
                observe(child.child.as_mut(), &String::from_utf8_lossy(&seen));
                if let Some((needle, keys)) = steps.get(answered) {
                    let text = String::from_utf8_lossy(&seen[cursor..]);
                    if let Some(at) = text.find(needle) {
                        cursor += at + needle.len();
                        writer.write_all(keys.as_bytes()).unwrap();
                        writer.flush().unwrap();
                        answered += 1;
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 직접 띄운 실제 child handle만 종료한다. 다른 PID/이름으로 찾지 않는다.
                child.child.kill().unwrap();
                child.child.wait().unwrap();
                panic!("install 승인 PTY({pid}) 사건 상한: {}", redact_private(&String::from_utf8_lossy(&seen)));
            }
        }
    }
    let status = child.child.wait().unwrap();
    child.finished = true;
    drop(writer);
    drop(pair.master);
    reader.join().unwrap();
    let text = String::from_utf8_lossy(&seen).to_string();
    assert!(!text.contains(CANARY), "승인 비밀 원문 출력");
    assert!(
        answered == steps.len(),
        "승인 물음 없이 끝나면 검토 성공이 아니다: status={status:?}, output={}",
        redact_private(&text)
    );
    (status.exit_code(), text)
}
fn authorize_args(lab: &Lab, output: &Path, plain: bool) -> Vec<String> {
    let mut args = lab.args();
    args.retain(|a| a != "--dry-run");
    if plain {
        args.extend(["--prompt".into(), "plain".into()]);
    }
    args.extend(["authorize".into(), "--out".into(), output.to_string_lossy().into_owned()]);
    args
}

#[test]
fn external_tty_reviews_the_same_real_plan_and_issues_private_bound_authorization() {
    for plain in [true, false] {
        let lab = Lab::new();
        let output = lab.0.join("approval.json");
        let (code, text) = terminal(&lab, &authorize_args(&lab, &output, plain), Some("yes\r"), false);
        assert_eq!(code, 0, "{}", redact_private(&text));
        let value: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        let claims = &value["claims"];
        assert_eq!(claims["instance"], lab.instance());
        assert!(claims["permissions"].as_array().unwrap().iter().any(|p| p["operation"] == "stage_binary"));
        assert!(text.contains(claims["plan_sha256"].as_str().unwrap()));
        assert!(!text.contains(value["signature"].as_str().unwrap()), "승인서는 공개 출력에 넣지 않는다");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&output).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(!lab.0.join("config").exists());
        assert!(!lab.0.join("작업 root").exists());
        assert!(!lab.0.join("state/install/jobs").exists(), "발급은 적용이 아니다");
    }
}

#[test]
fn cancelled_or_automated_or_session_review_creates_no_issuer_or_approval() {
    let lab = Lab::new();
    let output = lab.0.join("approval.json");
    let args = authorize_args(&lab, &output, true);
    let before = snapshot(&lab.0);
    let (code, _) = terminal(&lab, &args, Some("no\r"), false);
    assert_eq!(code, 3);
    assert_eq!(snapshot(&lab.0), before);
    let (code, _) = terminal(&lab, &args, Some(" yes \r"), false);
    assert_eq!(code, 3, "정확한 yes만 승인이다. plain이 앞뒤 공백을 지워 승인하면 안 된다");
    assert_eq!(snapshot(&lab.0), before);
    let (code, _) = terminal(&lab, &args, None, true);
    assert_eq!(code, 3);
    assert_eq!(snapshot(&lab.0), before);
    // OS의 실제 console EOF를 보낸다. Windows stdio는 Ctrl+Z, Unix canonical TTY는 Ctrl+D다.
    let eof = if cfg!(windows) { "\x1a" } else { "\x04" };
    let (code, _) = terminal(&lab, &args, Some(eof), false);
    assert_eq!(code, 130, "동의 전 EOF는 cancelled/130이다");
    assert_eq!(snapshot(&lab.0), before, "취소는 issuer와 승인 파일을 만들지 않는다");
    let (code, _) = terminal(&lab, &args, Some("\x03"), false);
    assert_eq!(code, 130, "외부 plain Ctrl+C도 cancelled/130 계약이다");
    assert_eq!(snapshot(&lab.0), before);
    let mut automated = args;
    automated.splice(1..1, ["--non-interactive".into(), "--yes".into(), "--json".into()]);
    assert_eq!(lab.run(&automated).status.code(), Some(2));
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn issued_approval_runs_the_real_file_worker_and_status_keeps_the_public_id() {
    let lab = Lab::new();
    let approval = lab.0.join("approval.json");
    assert_eq!(terminal(&lab, &authorize_args(&lab, &approval, true), Some("yes\r"), false).0, 0);
    let authorization: Value = serde_json::from_slice(&std::fs::read(&approval).unwrap()).unwrap();
    let id = authorization["claims"]["installation_id"].as_str().unwrap();
    let before = snapshot(&lab.0);
    let issued_status = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(issued_status["exit_code"], 0, "{issued_status}");
    assert_eq!(issued_status["status"], "planned");
    assert!(snapshot(&lab.0) == before, "발급만 된 설치의 status도 읽기 전용이다");
    let worker_sha = hex::encode(sha2::Sha256::digest(std::fs::read(BIN).unwrap()));
    for kind in [2u8, 99u8] {
        let before = snapshot(&lab.0);
        let mut framed = b"aaa.install.input.v1\0".to_vec();
        let auth = serde_json::to_vec(&authorization).unwrap();
        for (kind, bytes) in [(1u8, auth.as_slice()), (kind, CANARY.as_bytes())] {
            framed.push(kind);
            framed.extend((bytes.len() as u32).to_le_bytes());
            framed.extend(bytes);
        }
        let state = lab.0.join("state").canonicalize().unwrap();
        let mut worker = lab
            .command(&[
                "--state-dir",
                state.to_str().unwrap(),
                "install-worker",
                "--id",
                id,
                "--executor-sha256",
                &worker_sha,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut stdin = worker.stdin.take().unwrap();
            stdin.write_all(&framed).unwrap();
        }
        let output = worker.wait_with_output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(2),
            "worker must reject unused password / malformed private frame before nonce/file writes, kind={kind}"
        );
        assert!(output.stdout.is_empty() && output.stderr.is_empty(), "private input errors have no raw output");
        assert_eq!(snapshot(&lab.0), before, "private input rejection must leave approval/nonce/tree untouched");
    }
    let mut args = lab.args();
    args.retain(|a| a != "--dry-run");
    args.extend([
        "--non-interactive".into(),
        "--yes".into(),
        "--json".into(),
        "--authorization-file".into(),
        approval.to_string_lossy().into_owned(),
    ]);
    #[cfg(unix)]
    {
        // UDS 주소가 이미 일반 사용자 파일이면 지우지 않는다. Named Pipe에는 unlink 경로가 없다.
        let state = lab.0.join("state").canonicalize().unwrap();
        let key = hex::encode(sha2::Sha256::digest(format!("{}:{id}", state.display()).as_bytes()));
        let directory = PathBuf::from("/tmp").canonicalize().unwrap().join(format!(
            "aaa-i-{}-{}",
            unsafe { libc::getuid() },
            &key[..20]
        ));
        aaa_common::private_file::directory(&directory).unwrap();
        let socket = directory.join("b.sock");
        aaa_common::private_file::write(&socket, CANARY.as_bytes()).unwrap();
        let result = json(&lab.run(&args));
        assert_eq!(result["exit_code"], 4, "{result}");
        assert!(
            aaa_common::private_file::read(&socket, 65536).unwrap() == CANARY.as_bytes(),
            "기존 일반 파일은 bootstrap socket이 아니다"
        );
        std::fs::remove_file(socket).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
    let event_file = lab.0.join("events.ndjson");
    let mut stream_args = args.clone();
    let at = stream_args.iter().position(|a| a == "--json").unwrap();
    stream_args[at] = "--json-stream".into();
    stream_args.extend(["--events-file".into(), event_file.to_string_lossy().into_owned()]);
    let output = lab.run(&stream_args);
    let lines: Vec<Value> =
        String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(output.status.code(), Some(0), "worker 진입 결과: {}", lines.last().unwrap());
    let trace: Vec<Value> = String::from_utf8(aaa_common::private_file::read(&event_file, 1024 * 1024).unwrap())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines, trace, "private 사건 사본은 stdout과 같은 기록이다");
    assert_eq!(lines.iter().filter(|line| line["type"] == "result").count(), 1);
    assert!(
        lines
            .iter()
            .any(|line| line["type"] == "event" && line["phase"] == "configure" && line["state"] == "completed")
    );
    for (index, line) in lines.iter().enumerate() {
        assert_eq!(line["seq"], index as u64 + 1);
        assert_eq!(line["installation_id"], id);
    }
    let result = lines.last().unwrap()["result"].clone();
    assert_eq!(output.status.code(), Some(result["exit_code"].as_u64().unwrap() as i32));
    assert_eq!(result["exit_code"], 0, "발급받은 정확한 승인은 실제 고정 worker에 닿아야 한다: {result}");
    assert_eq!(result["installation_id"], id);
    assert_eq!(result["status"], "configured");
    assert_eq!(result["service"]["manager"], "none");
    assert_eq!(result["service"]["application_ready"], "not_requested");
    assert_eq!(result["web"]["tailscale_serve"]["state"], "not_requested");
    assert_eq!(result["web"]["pairing"]["state"], "not_requested");
    assert!(lab.0.join("작업 root/.aaa-root-owner.json").is_file());
    let before = snapshot(&lab.0);
    let observed = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(observed["installation_id"], id, "{observed}");
    assert_eq!(observed["exit_code"], 0);
    assert_eq!(snapshot(&lab.0), before, "읽기 status는 worker/lock/권한을 만들지 않는다");
    let repeated = json(&lab.run(&args));
    assert_ne!(repeated["exit_code"], 0, "사용 nonce를 새로운 apply로 쓰지 않는다");
    assert_eq!(snapshot(&lab.0), before);
    let resumed = json(&lab.cmd(&["install", "--resume", id, "--yes", "--json"]));
    assert_eq!(resumed["exit_code"], 0);
    assert_eq!(resumed["installation_id"], id);
    for public in [&result, &observed, &resumed] {
        assert!(
            !serde_json::to_string(public).unwrap().contains(authorization["signature"].as_str().unwrap()),
            "실제 전송한 서명은 공개 결과에 없다"
        );
    }
}

#[test]
fn ssh_environment_and_helper_hash_flags_do_not_bootstrap_a_new_trust_root() {
    let lab = Lab::new();
    let before = snapshot(&lab.0);
    let state = lab.0.join("state");
    let out = lab
        .command(&[
            "--state-dir",
            state.to_str().unwrap(),
            "install-bootstrap",
            "--executor-sha256",
            &"0".repeat(64),
            "--issuer-sha256",
            &"0".repeat(64),
        ])
        .env("SSH_CONNECTION", "example peer")
        .env("SSH_CLIENT", "example peer")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(snapshot(&lab.0), before, "자기 신고 SSH env와 입력 hash는 pin 발급 권한이 아니다");
}

#[test]
fn private_tty_enrollment_waits_on_the_real_tls_connection_and_codes_never_enter_public_events() {
    private_enrollment(false);
}
#[test]
fn pin_edited_while_sas_waits_is_preserved_as_a_conflict() {
    private_enrollment(true);
}
fn private_enrollment(edit_pin: bool) {
    use aaa_proto::{DeviceRole, FrontRequest};
    let lab = Lab::new();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
    let host = rt.block_on(async {
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.host.agent_listen = "127.0.0.1:0".into();
        let host = aaa_host::Host::open(cfg, lab.0.join("control-host")).unwrap();
        aaa_host::link::start(&host).await.unwrap();
        host.call(DeviceRole::Local, "local:cli", FrontRequest::EnrollOpen { minutes: 10 }).await.unwrap();
        host
    });
    let address = host.agent_listen.lock().unwrap().clone().unwrap();
    let relay = rt.block_on(super::network::ReplyLoss::start(address));
    let address = relay.address.clone();
    let mut base = lab.args();
    base.retain(|a| a != "--dry-run");
    let at = base.iter().position(|a| a == "--host").unwrap();
    base[at + 1] = address;
    let at = base.iter().position(|a| a == "--enroll").unwrap();
    base[at + 1] = "wait".into();
    let approval = lab.0.join("approval.json");
    let mut issue = base.clone();
    issue.extend([
        "--prompt".into(),
        "plain".into(),
        "authorize".into(),
        "--out".into(),
        approval.to_string_lossy().into_owned(),
    ]);
    assert_eq!(terminal(&lab, &issue, Some("yes\r"), false).0, 0);
    let authorization: Value = serde_json::from_slice(&std::fs::read(&approval).unwrap()).unwrap();
    let id = authorization["claims"]["installation_id"].as_str().unwrap();
    let mut automated = base.clone();
    automated.extend([
        "--authorization-file".into(),
        approval.to_string_lossy().into_owned(),
        "--non-interactive".into(),
        "--yes".into(),
        "--json-stream".into(),
    ]);
    let output = lab.run(&automated);
    assert_eq!(output.status.code(), Some(3));
    let public_attempt: Vec<Value> =
        String::from_utf8(output.stdout).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    let result = &public_attempt.last().unwrap()["result"];
    assert_eq!(result["installation_id"], id);
    assert!(
        result["needs_user"].as_array().unwrap().iter().any(|n| n["code"] == "installation_private_display_required")
    );
    let aaa_proto::FrontResponse::Pending { pending, .. } =
        rt.block_on(host.call(DeviceRole::Local, "local:cli", FrontRequest::EnrollPending)).unwrap()
    else {
        panic!("pending view")
    };
    assert!(pending.is_empty(), "JSON 모드는 SAS/TLS 수동 대기를 시작하지 않는다");
    let event_file = lab.0.join("tty-events.ndjson");
    let apply = vec![
        "install".into(),
        "--resume".into(),
        id.into(),
        "--events-file".into(),
        event_file.to_string_lossy().into_owned(),
    ];
    let mut approved = false;
    let mut actual_code = String::new();
    let (code, text) = terminal_observed(&lab, &apply, &[], false, |text| {
        if approved {
            return;
        }
        if let Some((_, rest)) = text.split_once("확인 코드: ") {
            let code = rest.chars().take(7).filter(char::is_ascii_digit).collect::<String>();
            if code.len() == 6 {
                // host가 commit하기 전에 응답 방향을 먼저 멈췄다는 사건을 확인한다.
                if edit_pin {
                    aaa_common::private_file::write(&lab.0.join("state/agent/host.json"), CANARY.as_bytes()).unwrap();
                } else {
                    rt.block_on(relay.pause());
                }
                rt.block_on(host.call(
                    DeviceRole::Local,
                    "local:cli",
                    FrontRequest::EnrollApprove { name: "mac-a".into(), code: code.clone() },
                ))
                .unwrap();
                if !edit_pin {
                    rt.block_on(relay.disconnect());
                }
                approved = true;
                actual_code = code;
            }
        }
    });
    if edit_pin {
        assert!(approved, "실제 SAS/host 승인을 지나야 한다");
        assert_eq!(code, 4, "{}", redact_private(&text));
        assert!(
            aaa_common::private_file::read(&lab.0.join("state/agent/host.json"), 65536).unwrap() == CANARY.as_bytes(),
            "동의 뒤 편집된 pin을 보존해야 한다"
        );
        return;
    }
    assert_eq!(code, 3, "{}", redact_private(&text));
    assert!(approved, "실제 SAS 표시·host 승인 사건이 있어야 한다");
    assert!(
        !lab.0.join("state/agent/host.json").exists(),
        "host commit 뒤 실제 TLS 응답을 잃어 pin을 아직 저장하지 못해야 한다"
    );
    let mut recover = apply.clone();
    let at = recover.iter().position(|a| a == "--events-file").unwrap();
    recover[at + 1] = lab.0.join("recovered-events.ndjson").to_string_lossy().into_owned();
    let (code, text) = terminal_steps(&lab, &recover, &[], false);
    assert_eq!(code, 0, "{}", redact_private(&text));
    assert!(
        !text.contains("확인 코드:"),
        "이미 소비한 등록은 같은 ID·TLS 신원으로 조회하며 옛 SAS를 재사용하지 않는다"
    );
    let machines = host.db.machines().unwrap();
    assert_eq!(machines.len(), 1);
    let pin = aaa_agent::identity::load_pin(&lab.0.join("state/agent")).unwrap();
    assert_eq!(pin.fingerprint, host.identity.fingerprint);
    let agent_identity: aaa_agent::identity::AgentIdentity = serde_json::from_slice(
        &aaa_common::private_file::read(&lab.0.join("state/agent/identity.json"), 65536).unwrap(),
    )
    .unwrap();
    assert_eq!(machines[0].id, agent_identity.machine_id);
    let cert = aaa_common::private_file::read(&lab.0.join("state/agent/tls/cert.der"), 1024 * 1024).unwrap();
    assert_eq!(machines[0].fingerprint, Some(aaa_common::tls::fingerprint(&cert)));
    let public = String::from_utf8(aaa_common::private_file::read(&event_file, 1024 * 1024).unwrap()).unwrap();
    for line in public.lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        for key in ["message", "code"] {
            assert!(!value[key].as_str().unwrap_or_default().contains(&actual_code), "SAS가 공개 사건에 들어갔다");
        }
        assert!(value.get("sas").is_none());
    }
    // 새 repair 승인에서도 이미 신뢰한 같은 key/name을 조회한다. 새 SAS/등록을 요구하지 않는다.
    let mut repair = base.clone();
    repair.extend(["--mode".into(), "repair".into()]);
    let approval2 = lab.0.join("repair-approval.json");
    let mut issue2 = repair.clone();
    issue2.extend([
        "--prompt".into(),
        "plain".into(),
        "authorize".into(),
        "--out".into(),
        approval2.to_string_lossy().into_owned(),
    ]);
    assert_eq!(terminal(&lab, &issue2, Some("yes\r"), false).0, 0);
    repair.extend([
        "--authorization-file".into(),
        approval2.to_string_lossy().into_owned(),
        "--non-interactive".into(),
        "--yes".into(),
        "--json".into(),
    ]);
    let output = lab.run(&repair);
    let repaired = json(&output);
    assert_eq!(repaired["exit_code"], 0, "{repaired}");
    assert_eq!(repaired["status"], "unchanged", "{repaired}");
    assert_eq!(host.db.machines().unwrap().len(), 1);
    let before = snapshot(&lab.0);
    let observed = json(&lab.cmd(&["install", "--status", id, "--json"]));
    assert_eq!(observed["exit_code"], 0, "{observed}");
    assert_eq!(snapshot(&lab.0), before);
}

#[test]
fn unknown_required_harness_auth_cannot_be_completed_and_explicit_defer_reuses_the_real_file_job() {
    let lab = Lab::new();
    let mut base = lab.args();
    base.retain(|a| a != "--dry-run");
    let at = base.iter().position(|a| a == "--harness").unwrap();
    base[at + 1] = "claude".into();
    base.extend(["--claude-path".into(), BIN.into(), "--login".into(), "require".into()]);
    let approval = lab.0.join("required-approval.json");
    let mut issue = base.clone();
    issue.extend([
        "--prompt".into(),
        "plain".into(),
        "authorize".into(),
        "--out".into(),
        approval.to_string_lossy().into_owned(),
    ]);
    assert_eq!(terminal(&lab, &issue, Some("yes\r"), false).0, 0);
    let authorization: Value = serde_json::from_slice(&std::fs::read(&approval).unwrap()).unwrap();
    let first_id = authorization["claims"]["installation_id"].as_str().unwrap();
    let mut apply = base.clone();
    apply.extend([
        "--authorization-file".into(),
        approval.to_string_lossy().into_owned(),
        "--non-interactive".into(),
        "--yes".into(),
        "--json".into(),
    ]);
    let result = json(&lab.run(&apply));
    assert_eq!(result["exit_code"], 3, "{result}");
    assert!(result["needs_user"].as_array().unwrap().iter().any(|n| n["code"] == "harness_auth_runtime_unverified"));
    let before = snapshot(&lab.0);
    let observed = json(&lab.cmd(&["install", "--status", first_id, "--json"]));
    assert_eq!(observed["exit_code"], 3, "{observed}");
    assert!(snapshot(&lab.0) == before, "인증 unknown status가 설정/토큰을 만들지 않는다");
    let at = base.iter().position(|a| a == "--login").unwrap();
    base[at + 1] = "defer".into();
    base.extend(["--mode".into(), "repair".into()]);
    let approval = lab.0.join("deferred-approval.json");
    let mut issue = base.clone();
    issue.extend([
        "--prompt".into(),
        "plain".into(),
        "authorize".into(),
        "--out".into(),
        approval.to_string_lossy().into_owned(),
    ]);
    assert_eq!(terminal(&lab, &issue, Some("yes\r"), false).0, 0);
    let authorization: Value = serde_json::from_slice(&std::fs::read(&approval).unwrap()).unwrap();
    let id = authorization["claims"]["installation_id"].as_str().unwrap();
    base.extend([
        "--authorization-file".into(),
        approval.to_string_lossy().into_owned(),
        "--non-interactive".into(),
        "--yes".into(),
        "--json".into(),
    ]);
    let result = json(&lab.run(&base));
    assert_eq!(result["exit_code"], 0, "{result}");
    assert_eq!(result["status"], "unchanged", "파일/서비스 문맥을 다시 만들지 않는 명시적 configured 목표다");
    assert!(
        result["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|check| check["name"] == "claude_login" && check["state"] == "not_requested")
    );
    let record: Value = serde_json::from_slice(
        &aaa_common::private_file::read(&lab.0.join("state/install/executions").join(format!("{id}.json")), 128 * 1024)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(record["files"], first_id, "인증 처리 선택만 바뀌면 같은 실제 파일 receipt를 재사용한다");
}
