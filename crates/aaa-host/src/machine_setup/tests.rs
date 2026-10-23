//! SSH 머신 추가 M2(plan §8.384). 순수 규칙(대상·`ssh -G`·지문·감지 출력)과, 실험실 sshd(비특권·loopback·비밀번호 끔)를 직접 띄워
//! host 요청으로 접속 → 지문 믿음 → 감지 → 아는 키·바뀐 키까지 도는 시험. sshd가 없는 머신(Windows runner 등)에서는 실험실 시험을
//! 건너뛰고 그렇게 적는다.
use super::*;
use aaa_proto::{DeviceRole, FrontRequest, FrontResponse, MachineSetupSettings};

#[test]
fn targets_are_aliases_or_user_host_port_without_passwords_or_options() {
    let ok = |raw: &str, dest: &str, port: Option<u16>| {
        assert_eq!(parse_target(raw).unwrap(), Target { dest: dest.into(), port }, "{raw}")
    };
    ok("mac-b", "mac-b", None);
    ok(" me@mac-b.example.com ", "me@mac-b.example.com", None);
    ok("me@mac-b:2222", "me@mac-b", Some(2222));
    ok("[::1]:2200", "::1", Some(2200));
    ok("me@[fe80::1%en0]", "me@fe80::1%en0", None);
    for bad in
        ["", "-oProxyCommand=sh", "me:pw@mac-b", "mac b", "mac-b:0", "mac-b:99999", "'mac-b'", "@mac-b", "[::1", "a;b"]
    {
        assert_eq!(parse_target(bad).unwrap_err().code, ErrorCode::BadRequest, "{bad}");
    }
}

#[test]
fn ssh_g_output_gives_user_host_port_and_proxy() {
    let out = "user me\nhostname 192.0.2.10\nport 2222\nproxyjump none\nidentityfile ~/.ssh/id_ed25519\n";
    assert_eq!(
        parse_resolved(out),
        Some(SshResolved { user: "me".into(), host: "192.0.2.10".into(), port: 2222, proxy_jump: false })
    );
    let jump = "user me\nhostname mac-b\nport 22\nproxyjump bastion\n";
    assert!(parse_resolved(jump).unwrap().proxy_jump);
    assert_eq!(parse_resolved("hostname x\n"), None);
}

#[test]
fn host_key_fingerprints_match_ssh_keygen() {
    // ssh-keygen -lf로 잰 값(실험 키, 비밀 아님)
    let line = "[127.0.0.1]:52222 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPj444TFfnMdtJzlZZRqJCV2eFEsY52BX6UURo4JEyMW";
    let k = key_of_line(line).unwrap();
    assert_eq!(k.kind, "ssh-ed25519");
    assert_eq!(k.sha256, "SHA256:XS8tVtM7fzXfOUb+FmRERqhvfdELdG0+MXBolcaZxAQ");
    assert_eq!(key_of_line("@revoked * ssh-ed25519 AAAA"), None);
    assert_eq!(key_of_line("host ssh-ed25519 !!!"), None);
}

#[test]
fn detection_output_maps_os_and_cpu() {
    let posix = format!("motd\n{MARK}\nDarwin\narm64\n/Users/<me>\n/bin/zsh\naaa 0.1.2\n{END}\n");
    let d = parse_detect(&posix).unwrap();
    assert_eq!((d.os.as_str(), d.arch.as_str(), d.raw.as_str()), ("macos", "aarch64", "Darwin arm64"));
    assert_eq!(
        (d.home.as_deref(), d.shell.as_deref(), d.aaa.as_deref()),
        (Some("/Users/<me>"), Some("/bin/zsh"), Some("aaa 0.1.2"))
    );
    let linux = format!("{MARK}\nLinux\nx86_64\n/home/<me>\n-\n-\n{END}");
    let d = parse_detect(&linux).unwrap();
    assert_eq!((d.os.as_str(), d.arch.as_str(), d.shell, d.aaa), ("linux", "x86_64", None, None));
    let ps = format!("{MARK}\r\nWindows\r\nAMD64\r\nC:\\Users\\me\r\n-\r\n-\r\n{END}\r\n");
    let d = parse_detect(&ps).unwrap();
    assert_eq!((d.os.as_str(), d.arch.as_str()), ("windows", "x86_64"));
    assert_eq!(parse_detect(&format!("{MARK}\nMINGW64_NT-10.0\nx86_64\n-\n-\n-\n{END}")).unwrap().os, "windows");
    // cmd.exe가 sh를 모르면 표지가 없다 → PowerShell, 모르는 OS → 사람이 고름
    assert_eq!(parse_detect("'sh' is not recognized as an internal or external command"), None);
    assert_eq!(parse_detect(&format!("{MARK}\nFreeBSD\namd64\n-\n-\n-\n{END}")), None);
    // 지원하지 않는 CPU는 감지는 되고 대상 표에서 빠진다
    let arm = parse_detect(&format!("{MARK}\nWindows\nARM64\n-\n-\n-\n{END}")).unwrap();
    assert!(!TARGETS.contains(&(arm.os.as_str(), arm.arch.as_str())));
}

#[test]
fn scripts_run_only_fixed_text() {
    let s = posix_script();
    assert!(s.starts_with("sh -c '") && s.ends_with('\'') && s.matches('\'').count() == 2, "{s}");
    let p = powershell_script();
    let b64 = p.rsplit(' ').next().unwrap();
    let raw = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
    let text =
        String::from_utf16(&raw.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect::<Vec<_>>()).unwrap();
    assert!(text.contains("PROCESSOR_ARCHITECTURE") && text.contains(MARK) && text.contains(END), "{text}");
}

#[test]
fn ssh_errors_become_plain_reasons() {
    assert!(why("ssh: Could not resolve hostname mac-z: nodename nor servname provided").contains("host 이름"));
    assert!(why("ssh: connect to host 127.0.0.1 port 1: Connection refused").contains("거부"));
    assert!(why("me@mac-b: Permission denied (publickey).").contains("비밀번호를 받지 않는다"));
    assert_eq!(why("\nsomething odd\n"), "ssh: something odd");
}

// ───────────── 실험실 sshd ─────────────

#[cfg(unix)]
fn sshd_bin() -> Option<PathBuf> {
    let fixed = PathBuf::from("/usr/sbin/sshd");
    if fixed.is_file() {
        return Some(fixed);
    }
    std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join("sshd")).find(|f| f.is_file()))
}

#[cfg(unix)]
struct Lab {
    root: PathBuf,
    sshd: std::process::Child,
    port: u16,
    host_pub: String,
}

#[cfg(unix)]
impl Drop for Lab {
    fn drop(&mut self) {
        // 이 시험이 띄운 sshd(자식)만 끝낸다
        let _ = self.sshd.kill();
        let _ = self.sshd.wait();
    }
}

#[cfg(unix)]
fn keygen(path: &Path) {
    let ok = std::process::Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-f"])
        .arg(path)
        .status()
        .unwrap()
        .success();
    assert!(ok, "ssh-keygen");
}

#[cfg(unix)]
fn lab() -> Option<Lab> {
    let Some(sshd) = sshd_bin() else {
        eprintln!("sshd가 없어 실험실 SSH 시험을 건너뛴다");
        return None;
    };
    let root = std::env::temp_dir().join(format!("aaa-ssh-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    keygen(&root.join("hostkey"));
    keygen(&root.join("userkey"));
    std::fs::copy(root.join("userkey.pub"), root.join("authorized_keys")).unwrap();
    // 빈 포트를 얻어 sshd에 준다(sshd는 고른 포트를 알리지 않는다)
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let cfg = format!(
        "ListenAddress 127.0.0.1:{port}\nHostKey {0}/hostkey\nPidFile {0}/sshd.pid\nAuthorizedKeysFile {0}/authorized_keys\n\
         PasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\n",
        root.display()
    );
    std::fs::write(root.join("sshd_config"), cfg).unwrap();
    // host가 쓸 ssh 설정: 별칭 lab(키는 실험실 것만), lab-down(닫힌 포트)
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let client = format!(
        "Host lab\n  HostName 127.0.0.1\n  Port {port}\n  IdentityFile {0}/userkey\n  IdentitiesOnly yes\n\
         Host lab-down\n  HostName 127.0.0.1\n  Port {closed}\n  IdentityFile {0}/userkey\n  IdentitiesOnly yes\n",
        root.display()
    );
    std::fs::write(root.join("ssh_config"), client).unwrap();
    let mut child = std::process::Command::new(sshd)
        .arg("-f")
        .arg(root.join("sshd_config"))
        .args(["-D", "-e"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // 「Server listening」 줄이 나오면 받을 준비가 됐다(나머지 stderr는 계속 읽어 버린다)
    let err = child.stderr.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        use std::io::BufRead;
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            if line.contains("Server listening") {
                let _ = tx.send(Ok(()));
            }
        }
        let _ = tx.send(Err(()));
    });
    let host_pub = std::fs::read_to_string(root.join("hostkey.pub")).unwrap();
    let lab = Lab { root, sshd: child, port, host_pub };
    match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(())) => Some(lab),
        other => panic!("실험실 sshd가 뜨지 않았다: {other:?}"),
    }
}

#[cfg(unix)]
fn host_for(lab: &Lab) -> Arc<Host> {
    let mut cfg = aaa_config::HostConfig::default();
    cfg.projects.root = lab.root.join("projects").to_string_lossy().into();
    cfg.notify.web_push = false;
    cfg.host.ssh_config = lab.root.join("ssh_config").to_string_lossy().into();
    Host::open(cfg, lab.root.join("host")).unwrap()
}

const WEB: &str = "device:phone";

#[cfg(unix)]
#[test]
fn a_trusted_connection_reaches_powershell_after_the_posix_command_exits_255() {
    use std::os::unix::fs::PermissionsExt;
    let Some(lab) = lab() else { return };
    let shell = lab.root.join("detect-shell");
    let events = lab.root.join("detect-events");
    // 실제 SSH 인증·host key 확인은 유지하고 Windows cmd의 명령 계약만 강제한다.
    std::fs::write(
        &shell,
        format!(
            r#"#!/bin/sh
case "$SSH_ORIGINAL_COMMAND" in
  'echo AAA-SSH-1')
    echo connect >> '{events}'
    echo AAA-SSH-1 ;;
  'sh -c '*)
    echo posix >> '{events}'
    echo 'The system cannot find the path specified.' >&2
    exit 255 ;;
  'powershell -NoProfile -NonInteractive -EncodedCommand '*)
    echo powershell >> '{events}'
    printf 'AAA-DETECT-1\nWindows\nAMD64\nC:\\Users\\me\n-\n-\nAAA-DETECT-END\n' ;;
  *) exit 255 ;;
esac
"#,
            events = events.display(),
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o700)).unwrap();
    let public = std::fs::read_to_string(lab.root.join("userkey.pub")).unwrap();
    std::fs::write(lab.root.join("authorized_keys"), format!("command=\"{}\" {public}", shell.display())).unwrap();
    let host = host_for(&lab);
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let job = start(&host, "lab").await;
        let trust = leave(&host, &job.job, MachineSetupPhase::Connecting).await;
        assert_eq!(trust.phase, MachineSetupPhase::Trust);
        assert!(!events.exists(), "키를 믿기 전에 원격 명령을 실행했다");
        host.setup_trust(&job.job, &trust.host_keys[0].sha256, WEB).await.unwrap();
        let ready = leave(&host, &job.job, MachineSetupPhase::Detecting).await;
        assert_eq!(ready.phase, MachineSetupPhase::Ready, "{:?}", ready.error);
        assert_eq!(ready.detected.unwrap().os, "windows");
        assert_eq!(std::fs::read_to_string(&events).unwrap(), "connect\nposix\npowershell\n");
    });
}

#[cfg(unix)]
async fn start(h: &Arc<Host>, target: &str) -> MachineSetupView {
    match h.call(DeviceRole::Controller, WEB, FrontRequest::MachineSetupStart { target: target.into() }).await.unwrap()
    {
        FrontResponse::MachineSetup { setup } => setup,
        other => panic!("{other:?}"),
    }
}

/// 작업이 그 단계를 떠날 때까지 `enroll` 사건을 기다린다
#[cfg(unix)]
async fn leave(h: &Arc<Host>, job: &str, phase: MachineSetupPhase) -> MachineSetupView {
    let mut rx = h.enroll_changed.subscribe();
    let wait = async {
        loop {
            let v = h.setup_load(job).unwrap().view;
            if v.phase != phase {
                return v;
            }
            rx.changed().await.unwrap();
        }
    };
    tokio::time::timeout(Duration::from_secs(60), wait).await.expect("SSH 작업이 그 단계를 떠나지 않았다")
}

#[cfg(unix)]
fn pub_fingerprint(pub_line: &str) -> String {
    key_of_line(&format!("x {pub_line}")).unwrap().sha256
}

#[cfg(unix)]
#[test]
fn a_lab_machine_is_reached_trusted_detected_and_a_changed_key_stops() {
    let Some(lab) = lab() else { return };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let h = host_for(&lab);
        // 처음 보는 키: 인증 전에 받은 키를 보이고 멈춘다
        let s = start(&h, "lab").await;
        assert_eq!(s.phase, MachineSetupPhase::Connecting);
        let s = leave(&h, &s.job, MachineSetupPhase::Connecting).await;
        assert_eq!(s.phase, MachineSetupPhase::Trust, "{:?}", s.error);
        assert_eq!(s.host_key_state.as_deref(), Some("new"));
        let want = pub_fingerprint(&lab.host_pub);
        assert_eq!(s.host_keys.iter().map(|k| k.sha256.clone()).collect::<Vec<_>>(), vec![want.clone()]);
        let ssh = s.ssh.clone().unwrap();
        assert_eq!((ssh.host.as_str(), ssh.port, ssh.proxy_jump), ("127.0.0.1", lab.port, false));
        assert!(!h.state.join("ssh/known_hosts").exists(), "믿기 전에 known_hosts에 쓰지 않는다");
        // 세션 안 agent·Reader는 믿지 못한다, 다른 지문은 거부
        let trust = |sha: &str| FrontRequest::MachineSetupTrust { job: s.job.clone(), host_key_sha256: sha.into() };
        assert_eq!(
            h.call(DeviceRole::Local, "local:cli@coordinator", trust(&want)).await.unwrap_err().code,
            ErrorCode::Forbidden
        );
        assert_eq!(h.call(DeviceRole::Reader, "device:r", trust(&want)).await.unwrap_err().code, ErrorCode::Forbidden);
        assert_eq!(
            h.call(DeviceRole::Controller, WEB, trust("SHA256:nope")).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        // 믿으면 aaa 전용 known_hosts에 그 줄만 넣고 감지한다
        let FrontResponse::MachineSetup { setup } = h.call(DeviceRole::Controller, WEB, trust(&want)).await.unwrap()
        else {
            panic!()
        };
        assert_eq!(setup.phase, MachineSetupPhase::Detecting);
        let known = std::fs::read_to_string(h.state.join("ssh/known_hosts")).unwrap();
        assert_eq!(known.lines().count(), 1);
        let s = leave(&h, &s.job, MachineSetupPhase::Detecting).await;
        assert_eq!(s.phase, MachineSetupPhase::Ready, "{:?}", s.error);
        let d = s.detected.unwrap();
        // 실험실은 이 머신 자신이다: 원격 감지 = 이 빌드의 OS·CPU
        assert_eq!((d.os.as_str(), d.arch.as_str(), d.chosen), (std::env::consts::OS, std::env::consts::ARCH, false));
        assert!(d.home.is_some(), "HOME");
        // 같은 대상을 다시: 이제 아는 키라 바로 감지
        let again = start(&h, "lab").await;
        let again = leave(&h, &again.job, MachineSetupPhase::Connecting).await;
        let again = if again.phase == MachineSetupPhase::Detecting {
            leave(&h, &again.job, MachineSetupPhase::Detecting).await
        } else {
            again
        };
        assert_eq!((again.phase, again.host_key_state.as_deref()), (MachineSetupPhase::Ready, Some("known")));
        // 닫힌 포트: 까닭을 보이고 끝
        let down = start(&h, "lab-down").await;
        let down = leave(&h, &down.job, MachineSetupPhase::Connecting).await;
        assert_eq!(down.phase, MachineSetupPhase::Failed);
        assert!(down.error.as_deref().unwrap().contains("거부"), "{:?}", down.error);
        // 바뀐 키: aaa known_hosts의 줄을 다른 키로 바꾸면 멈춘다(fail-closed)
        let other = lab.root.join("otherkey");
        keygen(&other);
        let other_pub = std::fs::read_to_string(other.with_extension("pub")).unwrap();
        let host_part = known.split_whitespace().next().unwrap();
        let mut parts = other_pub.split_whitespace();
        std::fs::write(
            h.state.join("ssh/known_hosts"),
            format!("{host_part} {} {}\n", parts.next().unwrap(), parts.next().unwrap()),
        )
        .unwrap();
        let changed = start(&h, "lab").await;
        let changed = leave(&h, &changed.job, MachineSetupPhase::Connecting).await;
        assert_eq!((changed.phase, changed.host_key_state.as_deref()), (MachineSetupPhase::Failed, Some("changed")));
        // 목록(Reader도)·멈춤
        let FrontResponse::MachineSetups { setups } =
            h.call(DeviceRole::Reader, "device:r", FrontRequest::MachineSetupList).await.unwrap()
        else {
            panic!()
        };
        assert_eq!(setups.len(), 4);
        assert_eq!(setups[0].job, changed.job, "최근 것부터");
        h.call(DeviceRole::Controller, WEB, FrontRequest::MachineSetupCancel { job: changed.job.clone() })
            .await
            .unwrap();
        assert_eq!(h.setup_list().unwrap().len(), 3);
        Arc::downgrade(&h)
    });
    drop(rt);
    assert!(weak.upgrade().is_none(), "runtime을 끝냈는데 Host가 남아 있다");
    let root = lab.root.clone();
    drop(lab);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn choosing_os_needs_the_choose_phase_and_a_release_target() {
    let root = std::env::temp_dir().join(format!("aaa-ssh-choose-{}", aaa_common::util::new_id()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        // 감지 실패로 멈춘 작업을 직접 둔다(원격 없이 고름 규칙만 본다)
        let mut job = Job {
            view: MachineSetupView {
                job: "j1".into(),
                target: "mac-b".into(),
                phase: MachineSetupPhase::Choose,
                ssh: None,
                host_keys: vec![],
                host_key_state: Some("known".into()),
                detected: None,
                error: Some("OS·CPU를 알아내지 못했다".into()),
                created_ms: 1,
                updated_ms: 1,
                settings: None,
                review: None,
                install: None,
            },
            key_lines: vec![],
            remote_dir: None,
        };
        h.setup_save(&mut job);
        let choose = |os: &str, arch: &str| FrontRequest::MachineSetupChoose {
            job: "j1".into(),
            os: os.into(),
            arch: arch.into(),
        };
        assert_eq!(
            h.call(DeviceRole::Controller, WEB, choose("windows", "aarch64")).await.unwrap_err().code,
            ErrorCode::BadRequest
        );
        let FrontResponse::MachineSetup { setup } =
            h.call(DeviceRole::Controller, WEB, choose("linux", "aarch64")).await.unwrap()
        else {
            panic!()
        };
        assert_eq!(setup.phase, MachineSetupPhase::Ready);
        assert!(setup.detected.unwrap().chosen);
        assert_eq!(
            h.call(DeviceRole::Controller, WEB, choose("linux", "x86_64")).await.unwrap_err().code,
            ErrorCode::Conflict
        );
        // 진행 중에 host가 내려간 작업은 다시 열 때 멈췄다고 적는다
        let mut stuck = job.clone();
        stuck.view.job = "j2".into();
        stuck.view.phase = MachineSetupPhase::Connecting;
        h.setup_save(&mut stuck);
        h.setup_recover();
        assert_eq!(h.setup_load("j2").unwrap().view.phase, MachineSetupPhase::Failed);
        Arc::downgrade(&h)
    });
    drop(rt);
    assert!(weak.upgrade().is_none());
    std::fs::remove_dir_all(root).unwrap();
}

/// 계획 M2의 확인 항목: GitHub windows-2025 runner에 OpenSSH 서버(sshd.exe)가 있는가. 실험실 시험은 Unix만 돌고, 여기서는 있는지만
/// 적는다(실패하지 않는다). 그 결과로 M4(Windows 설치) 시험 방식(실험실 sshd 또는 가짜 ssh 대본 + 수동 인수)을 정한다
#[cfg(windows)]
#[test]
fn windows_runner_reports_whether_an_openssh_server_exists() {
    let system = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let fixed = system.join(r"System32\OpenSSH\sshd.exe");
    let in_path = std::env::var_os("PATH")
        .and_then(|p| std::env::split_paths(&p).map(|d| d.join("sshd.exe")).find(|f| f.is_file()));
    let found = if fixed.is_file() { Some(fixed) } else { in_path };
    eprintln!("M2 Windows sshd 판정: {}", found.map_or("sshd.exe 없음".into(), |f| format!("있음 {}", f.display())));
}

// ───────────── binary 전송(M3) ─────────────

#[test]
fn the_binary_is_the_running_file_for_the_same_kind_and_a_release_otherwise() {
    use binary::{Source, pick};
    let exe = Path::new("/opt/aaa/bin/aaa");
    assert_eq!(
        pick(("macos", "aarch64"), "aaa 0.1.2", ("macos", "aarch64"), exe),
        Source::Running { path: exe.into() }
    );
    assert_eq!(
        pick(("macos", "aarch64"), "aaa 0.1.2", ("linux", "x86_64"), exe),
        Source::Release { version: "0.1.2".into(), key: "linux-x86_64".into() }
    );
    // 개발 판 host는 다른 종류에 넣을 같은 판 파일이 없다(같은 종류는 자기 파일로 된다)
    assert_eq!(
        pick(("macos", "aarch64"), "aaa 0.1.2+abc", ("macos", "aarch64"), exe),
        Source::Running { path: exe.into() }
    );
    let Source::Refuse(why) = pick(("macos", "aarch64"), "aaa 0.1.2+abc", ("macos", "x86_64"), exe) else { panic!() };
    assert!(why.contains("개발 판") && why.contains("macos·x86_64"), "{why}");
}

#[test]
fn upload_output_is_read_between_marks_and_only_safe_temp_dirs_are_removed() {
    use binary::{Placed, cleanup_script, mismatch, parse_upload, safe_dir, upload_script};
    let sha = "a".repeat(64);
    let out = format!(
        "motd\nAAA-UP-1\n/var/folders/x_/T//aaa-setup.Ab12Cd34\n{}\naaa 0.1.2\nAAA-UP-END\n",
        sha.to_uppercase()
    );
    let placed = parse_upload(&out).unwrap();
    assert_eq!(
        placed,
        Placed {
            dir: "/var/folders/x_/T//aaa-setup.Ab12Cd34".into(),
            sha256: sha.clone(),
            version: "aaa 0.1.2".into()
        }
    );
    assert_eq!(mismatch(&placed, &sha, "aaa 0.1.2"), None);
    assert!(mismatch(&placed, &"b".repeat(64), "aaa 0.1.2").unwrap().contains("SHA-256"));
    assert!(mismatch(&placed, &sha, "aaa 0.1.3").unwrap().contains("판이 다르다"));
    let silent = Placed { version: String::new(), ..placed.clone() };
    assert!(mismatch(&silent, &sha, "aaa 0.1.2").unwrap().contains("실행되지 않음"));
    for bad in [
        "/tmp/aaa-setup.",
        "tmp/aaa-setup.x1",
        "/tmp/../aaa-setup.x1",
        "/tmp/other.x1",
        "/tmp/aaa-setup.x1/..",
        "/tmp/a b/aaa-setup.x1",
        "/tmp/aaa-setup.x1\"; rm -rf ~; \"",
        "/tmp/aaa-setup.x$1",
    ] {
        assert!(!safe_dir(bad), "{bad}");
        assert_eq!(cleanup_script(bad), None, "{bad}");
    }
    assert_eq!(parse_upload(&out.replace(&sha.to_uppercase(), "zz")), None);
    assert_eq!(parse_upload("AAA-UP-1\n/tmp/x\nAAA-UP-END\n"), None);
    assert_eq!(cleanup_script("/tmp/aaa-setup.x1").unwrap(), "sh -c 'rm -rf -- \"/tmp/aaa-setup.x1\"'");
    // 원격 명령은 고정 글이다(사람이 넣은 값이 들어가지 않는다)
    assert!(upload_script().starts_with("sh -c 'umask 077; d=$(mktemp -d"));
    assert_eq!(upload_script().matches('\'').count(), 2);
}

/// 실험실 sshd로: host가 보낸 파일이 원격 전용 임시 폴더(0700)에 같은 SHA로 놓이고 `--version`이 돈다. 기대와 다르면 그 폴더를
/// 지우고 까닭을 돌려준다. 실험실은 같은 머신이라 원격 폴더를 직접 본다
#[cfg(unix)]
#[test]
fn the_binary_reaches_a_private_remote_temp_dir_and_a_mismatch_removes_it() {
    use std::os::unix::fs::PermissionsExt;
    let Some(lab) = lab() else { return };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let h = host_for(&lab);
        let job = start(&h, "lab").await;
        let job = leave(&h, &job.job, MachineSetupPhase::Connecting).await;
        let sha = job.host_keys[0].sha256.clone();
        h.call(
            DeviceRole::Controller,
            WEB,
            FrontRequest::MachineSetupTrust { job: job.job.clone(), host_key_sha256: sha },
        )
        .await
        .unwrap();
        assert_eq!(leave(&h, &job.job, MachineSetupPhase::Detecting).await.phase, MachineSetupPhase::Ready);
        // 보낼 파일: `--version`에 판을 답하는 작은 실행 파일
        let file = lab.root.join("fake-aaa");
        std::fs::write(&file, "#!/bin/sh\necho 'aaa 9.9.9'\n").unwrap();
        let sha = hex::encode(Sha256::digest(std::fs::read(&file).unwrap()));
        let ssh = h.ssh();
        let t = parse_target("lab").unwrap();
        let placed = ssh.upload(&t, &file, &sha, "aaa 9.9.9", "macos").await.unwrap();
        assert_eq!(placed.sha256, sha);
        let dir = Path::new(&placed.dir);
        assert_eq!(std::fs::read(dir.join("aaa")).unwrap(), std::fs::read(&file).unwrap());
        assert_eq!(std::fs::metadata(dir).unwrap().permissions().mode() & 0o777, 0o700);
        assert!(ssh.cleanup(&t, &placed.dir).await.unwrap().ok);
        assert!(!dir.exists());
        // SHA·판이 기대와 다르면 놓은 폴더를 지우고 까닭을 준다
        let before = remote_setup_dirs(&file);
        let e = ssh.upload(&t, &file, &"0".repeat(64), "aaa 9.9.9", "macos").await.unwrap_err();
        assert!(e.contains("SHA-256"), "{e}");
        let e = ssh.upload(&t, &file, &sha, "aaa 0.1.2", "macos").await.unwrap_err();
        assert!(e.contains("판이 다르다"), "{e}");
        assert_eq!(remote_setup_dirs(&file), before, "어긋난 전송은 원격 임시 폴더를 남기지 않는다");
        // 닫힌 포트는 ssh 까닭
        let e = ssh.upload(&parse_target("lab-down").unwrap(), &file, &sha, "aaa 9.9.9", "macos").await.unwrap_err();
        assert!(e.contains("거부"), "{e}");
    });
}

/// 실험실 원격(같은 머신)의 `aaa-setup.*` 임시 폴더 중 `file`과 같은 `aaa`를 가진 것(나란히 도는 다른 시험의 폴더는 내용이 다르다)
#[cfg(unix)]
fn remote_setup_dirs(file: &Path) -> Vec<PathBuf> {
    let want = std::fs::read(file).unwrap();
    let mut found = vec![];
    for base in [std::env::var_os("TMPDIR").map(PathBuf::from), Some(PathBuf::from("/tmp"))].into_iter().flatten() {
        for e in std::fs::read_dir(&base).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with("aaa-setup.")
                && std::fs::read(e.path().join("aaa")).is_ok_and(|b| b == want)
            {
                found.push(e.path());
            }
        }
    }
    found.sort();
    found
}

// ───────────── SSH 설치·등록(M3) ─────────────

#[test]
fn settings_become_agent_choices_under_the_detected_home() {
    use aaa_proto::install::*;
    let settings = MachineSetupSettings {
        name: "mac-b".into(),
        host: "mac-a.example.ts.net:7430".into(),
        work_root: "/Volumes/<disk>/aaa-work".into(),
        work_mount: Some("/Volumes/<disk>".into()),
        harness: vec![InstallHarness::Codex, InstallHarness::Claude, InstallHarness::Codex],
        login: InstallLogin::Defer,
    };
    let mac = MachineDetected {
        os: "macos".into(),
        arch: "aarch64".into(),
        raw: "Darwin arm64".into(),
        chosen: false,
        home: Some("/Users/<me>/".into()),
        shell: Some("/bin/zsh".into()),
        aaa: None,
    };
    let c = install::choices(&settings, &mac).unwrap();
    assert_eq!(c.role, Some(InstallRole::Agent));
    assert_eq!(c.state_dir.as_deref(), Some("/Users/<me>/Library/Application Support/aaa"));
    assert_eq!(c.bin_path.as_deref(), Some("/Users/<me>/.local/bin/aaa"));
    assert_eq!(c.harness, vec![InstallHarness::Claude, InstallHarness::Codex]);
    assert_eq!((c.work_mount.as_deref(), c.work_storage), (Some("/Volumes/<disk>"), None));
    assert_eq!(
        (c.enroll, c.expected_os, c.expected_arch),
        (Some(InstallEnroll::Grant), Some(InstallOs::Macos), Some(InstallArch::Aarch64))
    );
    // 현재 사용자 LaunchAgent로 바로 띄운다(install I3). launcher 경로는 원격 install이 그 HOME의 기본 자리로 채운다
    assert_eq!(
        (c.service, c.start, c.launcher_policy, c.launcher.as_deref()),
        (Some(InstallService::Launchd), Some(InstallStart::Now), Some(InstallLauncherPolicy::Install), None)
    );
    let inside =
        install::choices(&MachineSetupSettings { work_mount: None, harness: vec![], ..settings.clone() }, &mac)
            .unwrap();
    assert_eq!((inside.work_storage, inside.harness), (Some(InstallStorage::Internal), vec![InstallHarness::None]));
    // 감독자가 아직 없는 OS(Linux I4·Windows)는 설치하지 않는다, HOME 없음은 다시 감지
    let other = MachineDetected { os: "linux".into(), home: Some("/home/<me>".into()), ..mac.clone() };
    assert!(install::choices(&settings, &other).err().unwrap().contains("macOS부터"));
    assert!(
        install::choices(&settings, &MachineDetected { home: None, ..mac.clone() }).err().unwrap().contains("HOME")
    );
    // agent socket이 한도를 넘는 긴 HOME은 계획 전에 멈춘다(29자부터)
    let long = |n: usize| MachineDetected { home: Some(format!("/{}", "u".repeat(n - 1))), ..mac.clone() };
    assert!(install::choices(&settings, &long(28)).is_ok());
    assert!(install::choices(&settings, &long(29)).err().unwrap().contains("socket"));
    for bad in [
        MachineSetupSettings { name: "mac b".into(), ..settings.clone() },
        MachineSetupSettings { host: "a b:1".into(), ..settings.clone() },
        MachineSetupSettings { work_root: "aaa-work".into(), ..settings.clone() },
        MachineSetupSettings { work_root: "/a/../b".into(), ..settings.clone() },
        MachineSetupSettings { harness: vec![InstallHarness::None, InstallHarness::Claude], ..settings.clone() },
    ] {
        assert!(install::check_settings(&bad).is_err(), "{bad:?}");
    }
}

#[test]
fn remote_install_commands_are_fixed_text_with_checked_values() {
    let dir = "/tmp/aaa-setup.Ab12";
    let sha = "a".repeat(64);
    assert_eq!(
        install::probe_script(dir, "macos", &sha).unwrap(),
        format!(
            "sh -c '\"{dir}/aaa\" --state-dir \"$HOME/Library/Application Support/aaa\" install-probe --executor-sha256 {sha}'"
        )
    );
    assert!(
        install::bootstrap_script(dir, "linux", &sha, &"b".repeat(64))
            .unwrap()
            .contains("\"$HOME/.local/state/aaa\" install-bootstrap")
    );
    assert!(
        install::apply_script(dir, "macos", "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .unwrap()
            .ends_with("install --resume 01ARZ3NDEKTSV4RRFFQ69G5FAV --non-interactive --yes --json-stream'")
    );
    assert_eq!(install::probe_script("/tmp/x", "macos", &sha), None);
    assert_eq!(install::probe_script(dir, "macos", "zz"), None);
    assert_eq!(install::probe_script(dir, "windows", &sha), None);
    assert_eq!(install::apply_script(dir, "macos", "1;rm"), None);
    assert_eq!(install::bootstrap_script(dir, "macos", &sha, "x"), None);
    let line = r#"{"type":"event","schema":"aaa.install.event.v1","installation_id":null,"seq":1,"phase":"configure","state":"running","code":"c","message":"설정 쓰는 중"}"#;
    assert!(matches!(install::stream_line(line), install::Line::Event(m) if m == "설정 쓰는 중"));
    assert!(matches!(install::stream_line("not json"), install::Line::Other));
}

/// 원격 aaa 대본: 실제 ssh(실험실 sshd)로 보낸 파일이지만 install은 미리 만든 답을 낸다. 받은 인자·stdin을 남긴다.
/// 적용(`install --resume`)은 FIFO `go`가 열릴 때까지 기다린다(시험이 그사이 등록을 한다)
#[cfg(unix)]
fn fake_remote(root: &Path) -> PathBuf {
    fake_remote_as(root, "aaa 9.9.9")
}

#[cfg(unix)]
fn fake_remote_as(root: &Path, version: &str) -> PathBuf {
    let remote = root.join("remote");
    std::fs::create_dir_all(&remote).unwrap();
    let r = remote.display();
    let script = format!(
        r#"#!/bin/sh
L="{r}"
case "$*" in
  --version) echo '{version}'; exit 0;;
  *install-probe*)
    printf '%s\n' "$@" > "$L/probe.args"; cat > "$L/probe.in"
    for f in "$(dirname "$0")"/aaa-launcher-*.json; do if [ -f "$f" ]; then cp "$f" "$L/probe.launcher"; fi; done
    if [ -f "$L/probe.exit" ]; then exit "$(cat "$L/probe.exit")"; fi
    cat "$L/probe.out"; exit 0;;
  *install-bootstrap*)
    n=$(ls "$L" | grep -c '^boot.*\.in$'); cat > "$L/boot$n.in"
    id=$(grep -o '"installation_id":"[A-Z0-9]*"' "$L/boot$n.in" | head -n 1 | cut -d'"' -f4)
    plan=$(grep -o '"plan_sha256":"[0-9a-f]*"' "$L/boot$n.in" | head -n 1 | cut -d'"' -f4)
    printf '{{"installation_id":"%s","plan_sha256":"%s","agent_identity":"%s"}}\n' "$id" "$plan" "$(cat "$L/agent")"
    exit 0;;
  *"install --resume"*)
    printf '%s\n' "$@" > "$L/apply.args"; cat "$L/go" > /dev/null
    id=$(grep -A1 -- '--resume' "$L/apply.args" | tail -n 1)
    sed "s/@ID@/$id/g" "$L/apply.out"; exit 0;;
esac
exit 9
"#
    );
    let file = root.join("fake-aaa");
    std::fs::write(&file, script).unwrap();
    assert!(std::process::Command::new("mkfifo").arg(remote.join("go")).status().unwrap().success());
    file
}

/// host 실행 파일(시험은 대본) 옆에 그 파일에 묶인 launcher package(install은 내용·서명을 보지만 host는 묶음 칸만 본다)
#[cfg(unix)]
fn fake_launcher(binary: &Path, version: &str) -> PathBuf {
    let arch = std::env::consts::ARCH;
    let file = binary.with_file_name(format!("aaa-launcher-macos-{arch}.json"));
    let package = serde_json::json!({
        "schema": 1,
        "version": version,
        "platform": format!("macos-{arch}"),
        "binary_sha256": hex::encode(Sha256::digest(std::fs::read(binary).unwrap())),
        "files": {},
    });
    std::fs::write(&file, serde_json::to_vec(&package).unwrap()).unwrap();
    file
}

/// 실험실은 이 머신이라 Linux CI에서도 감지 OS와 host를 같은 종류 macOS로 둔다(대본 원격은 OS를 보지 않는다. host가 다른 종류로
/// 보고 release를 받으러 가지 않게)
#[cfg(unix)]
fn as_mac(h: &Arc<Host>, job: &str) -> MachineDetected {
    install::tests_binary::set_here(Some(("macos", std::env::consts::ARCH)));
    h.setup_update(job, MachineSetupPhase::Ready, |j| j.view.detected.as_mut().unwrap().os = "macos".into())
        .unwrap()
        .view
        .detected
        .unwrap()
}

/// 대본이 낼 install 계획: host가 보낼 choices로 실제 install planner를 돌린 것
#[cfg(unix)]
fn fake_proposal(c: aaa_proto::install::InstallChoices, sha: &str, os: &str) -> aaa_proto::install::InstallProposal {
    use aaa_proto::install::*;
    let context = InstallPlanContext {
        platform: InstallPlatform {
            os: if os == "macos" { InstallOs::Macos } else { InstallOs::Linux },
            arch: if cfg!(target_arch = "aarch64") { InstallArch::Aarch64 } else { InstallArch::X86_64 },
        },
        version: "9.9.9".into(),
        binary_sha256: sha.into(),
        bin_path: c.bin_path.clone().unwrap(),
        config_dir: "/Users/<me>/.config/aaa".into(),
        state_dir: c.state_dir.clone().unwrap(),
        projects_root: "/Users/<me>/.aaa/projects".into(),
        default_shell: "/bin/zsh".into(),
        effective_config_sha256: "c".repeat(64),
        target_account: "501".into(),
    };
    // 원격 install이 launcher 기본 자리를 채운다(detect)
    let mut c = c;
    if c.service == Some(InstallService::Launchd) {
        c.launcher
            .get_or_insert_with(|| "/Users/<me>/Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher".into());
    }
    let plan = aaa_config::install::plan(c, context).unwrap();
    let hash = aaa_config::install::plan_sha256(&plan);
    let mut result = InstallResult::invalid();
    result.status = InstallStatus::Planned;
    result.exit_code = 0;
    result.error = None;
    result.plan_sha256 = Some(hash);
    result.changes = vec![InstallChange {
        kind: "agent_config".into(),
        action: "create".into(),
        state: InstallChangeState::Planned,
    }];
    InstallProposal { permissions: vec![aaa_config::install::enrollment_permission(&plan).unwrap()], plan, result }
}

/// 실험실 sshd + 원격 install 대본으로: 설정 → aaa 전송·확인 → 원격 계획(화면엔 공개 요약만) → 다른 hash·보기 기기·세션 안은 거부 →
/// 사람 적용 → host 서명 → bootstrap(신원 없음) → 신원에 묶은 grant로 다시 → 적용 중 grant 등록 → 설치됨. 원격 임시 폴더·계획 파일 정리
#[cfg(unix)]
#[test]
fn a_detected_machine_is_planned_signed_bootstrapped_installed_and_registered() {
    use aaa_proto::install::*;
    use std::os::unix::fs::PermissionsExt;
    let Some(lab) = lab() else { return };
    let fake = fake_remote(&lab.root);
    let remote = lab.root.join("remote");
    let sha = hex::encode(Sha256::digest(std::fs::read(&fake).unwrap()));
    let launcher = fake_launcher(&fake, "9.9.9");
    install::tests_binary::set(Some((fake.clone(), "aaa 9.9.9".into())));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let h = host_for(&lab);
        let job = start(&h, "lab").await;
        let job = leave(&h, &job.job, MachineSetupPhase::Connecting).await;
        let key = job.host_keys[0].sha256.clone();
        h.call(DeviceRole::Controller, WEB, FrontRequest::MachineSetupTrust { job: job.job.clone(), host_key_sha256: key })
            .await
            .unwrap();
        let ready = leave(&h, &job.job, MachineSetupPhase::Detecting).await;
        assert_eq!(ready.phase, MachineSetupPhase::Ready);
        let detected = as_mac(&h, &job.job);
        let settings = MachineSetupSettings {
            name: "lab-b".into(),
            host: "127.0.0.1:7430".into(),
            work_root: lab.root.join("work").to_string_lossy().into(),
            work_mount: None,
            harness: vec![],
            login: InstallLogin::Defer,
        };
        let choices = install::choices(&settings, &detected).unwrap();
        let proposal = fake_proposal(choices.clone(), &sha, &detected.os);
        let plan_sha = aaa_config::install::plan_sha256(&proposal.plan);
        std::fs::write(remote.join("probe.out"), serde_json::to_vec(&proposal).unwrap()).unwrap();
        let agent = tls_like_identity();
        std::fs::write(remote.join("agent"), &agent).unwrap();
        let result = {
            let mut r = proposal.result.clone();
            r.status = InstallStatus::Configured;
            r.installation_id = Some("@ID@".into());
            r.changes[0].state = InstallChangeState::Applied;
            r
        };
        let lines = [
            serde_json::json!({"type":"event","schema":"aaa.install.event.v1","installation_id":"@ID@","seq":1,"phase":"configure","state":"running","code":"agent_config","message":"agent 설정을 쓴다"}),
            serde_json::to_value(InstallStreamItem::Result {
                schema: InstallEventSchema::V1,
                installation_id: Some("@ID@".into()),
                seq: 2,
                result: Box::new(result),
            })
            .unwrap(),
        ];
        let ndjson: String = lines.iter().map(|l| format!("{l}\n")).collect();
        std::fs::write(remote.join("apply.out"), ndjson).unwrap();

        // 계획: 보낸 파일 확인 → 원격 install 계획. 화면에는 hash·공개 결과만
        let plan = |s: MachineSetupSettings| FrontRequest::MachineSetupPlan { job: job.job.clone(), settings: s };
        assert_eq!(
            h.call(DeviceRole::Controller, WEB, plan(MachineSetupSettings { name: "labA x".into(), ..settings.clone() }))
                .await
                .unwrap_err()
                .code,
            ErrorCode::BadRequest
        );
        h.call(DeviceRole::Controller, WEB, plan(settings.clone())).await.unwrap();
        let review = leave(&h, &job.job, MachineSetupPhase::Planning).await;
        assert_eq!(review.phase, MachineSetupPhase::Review, "{:?}", review.error);
        let summary = review.review.clone().unwrap();
        assert_eq!((summary.plan_sha256.as_str(), summary.binary_sha256.as_str()), (plan_sha.as_str(), sha.as_str()));
        assert_eq!(summary.binary_version, "aaa 9.9.9");
        assert_eq!(summary.operations, vec!["enroll_agent".to_string()]);
        let sent: InstallChoices = serde_json::from_slice(&std::fs::read(remote.join("probe.in")).unwrap()).unwrap();
        assert!(sent == choices, "host가 보낸 choices");
        assert_eq!((sent.service, sent.start), (Some(InstallService::Launchd), Some(InstallStart::Now)));
        assert_eq!(
            std::fs::read(remote.join("probe.launcher")).unwrap(),
            std::fs::read(&launcher).unwrap(),
            "원격 실행 파일 옆에 같은 launcher package가 있다"
        );
        let args = std::fs::read_to_string(remote.join("probe.args")).unwrap();
        assert!(args.lines().any(|l| Some(l) == choices.state_dir.as_deref()), "원격 --state-dir = choices state_dir: {args}");
        let view = serde_json::to_string(&h.setup_list().unwrap()).unwrap();
        assert!(!view.contains("Application Support") && !view.contains(".local/state"), "계획 원문(경로)은 화면에 없다");
        let private = install::plan_file(&h.state, &job.job);
        assert_eq!(std::fs::metadata(&private).unwrap().permissions().mode() & 0o777, 0o600);
        let placed = h.setup_load(&job.job).unwrap().remote_dir.unwrap();
        assert!(Path::new(&placed).join("aaa").is_file());

        // 적용: hash가 다르면·보기 기기·세션 안은 거부
        let apply = |p: &str| FrontRequest::MachineSetupApply { job: job.job.clone(), plan_sha256: p.into(), service_password: None };
        assert_eq!(h.call(DeviceRole::Controller, WEB, apply(&"0".repeat(64))).await.unwrap_err().code, ErrorCode::Conflict);
        assert!(h.call(DeviceRole::Reader, "device:tv", apply(&plan_sha)).await.is_err());
        assert!(h.call(DeviceRole::Local, "local:demo@01ARZ3NDEKTSV4RRFFQ69G5FAV", apply(&plan_sha)).await.is_err());
        assert_eq!(h.setup_load(&job.job).unwrap().view.phase, MachineSetupPhase::Review);
        h.call(DeviceRole::Controller, WEB, apply(&plan_sha)).await.unwrap();
        // 적용이 시작되면(bootstrap 둘 뒤) 원격 worker 대신 grant로 등록한다
        let mut rx = h.enroll_changed.subscribe();
        let id = tokio::time::timeout(Duration::from_secs(60), async {
            loop {
                let v = h.setup_load(&job.job).unwrap().view;
                assert_eq!(v.phase, MachineSetupPhase::Installing, "{:?}", v.error);
                if let Some(i) = v.install.as_ref()
                    && i.last_event.as_deref() == Some("원격 install 적용 중")
                {
                    return i.installation_id.clone().unwrap();
                }
                rx.changed().await.unwrap();
            }
        })
        .await
        .expect("적용 단계");
        let first: InstallBootstrapPackage = serde_json::from_slice(&std::fs::read(remote.join("boot0.in")).unwrap()).unwrap();
        let second: InstallBootstrapPackage = serde_json::from_slice(&std::fs::read(remote.join("boot1.in")).unwrap()).unwrap();
        assert!(first.authorization.enrollment.is_none(), "첫 package에는 등록 grant가 없다");
        let grant = second.authorization.enrollment.clone().expect("둘째 package의 grant");
        assert_eq!((grant.agent_identity.as_str(), grant.installation_id.as_str()), (agent.as_str(), id.as_str()));
        assert_eq!(second.authorization.claims.plan_sha256, plan_sha);
        let request = InstallEnrollRequest {
            proto: aaa_proto::AGENT_LINK,
            machine_id: aaa_common::util::new_id(),
            machine_name: "lab-b".into(),
            grant: grant.clone(),
        };
        h.install_enroll(&agent, &request, aaa_common::util::now_ms()).unwrap();
        std::fs::write(remote.join("go"), b"go\n").unwrap();
        let done = leave(&h, &job.job, MachineSetupPhase::Installing).await;
        assert_eq!(done.phase, MachineSetupPhase::Installed, "{:?}", done.error);
        let i = done.install.unwrap();
        assert_eq!(i.machine.as_deref(), Some("lab-b"));
        assert_eq!(i.result.unwrap().status, InstallStatus::Configured);
        let args = std::fs::read_to_string(remote.join("apply.args")).unwrap();
        assert!(args.contains(&format!("--resume\n{id}\n")), "{args}");
        assert!(!Path::new(&placed).exists(), "원격 임시 폴더를 지웠다");
        assert!(!private.exists(), "계획 파일을 지웠다");
        Arc::downgrade(&h)
    });
    drop(rt);
    install::tests_binary::set(None);
    assert!(weak.upgrade().is_none());
}

/// 대본 원격이 알리는 agent TLS 신원(64자 hex)
#[cfg(unix)]
fn tls_like_identity() -> String {
    hex::encode(Sha256::digest(aaa_common::util::new_id().as_bytes()))
}

/// 원격 install이 이 SSH를 믿지 않으면(exit 3, 비특권 sshd) 까닭을 보이고 원격 임시 폴더·계획을 남기지 않는다
#[cfg(unix)]
#[test]
fn a_remote_install_that_refuses_this_ssh_fails_with_the_reason_and_leaves_nothing() {
    use aaa_proto::install::InstallLogin;
    let Some(lab) = lab() else { return };
    let fake = fake_remote(&lab.root);
    fake_launcher(&fake, "9.9.9");
    std::fs::write(lab.root.join("remote/probe.exit"), "3").unwrap();
    install::tests_binary::set(Some((fake.clone(), "aaa 9.9.9".into())));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let h = host_for(&lab);
        let job = start(&h, "lab").await;
        let job = leave(&h, &job.job, MachineSetupPhase::Connecting).await;
        let key = job.host_keys[0].sha256.clone();
        h.call(
            DeviceRole::Controller,
            WEB,
            FrontRequest::MachineSetupTrust { job: job.job.clone(), host_key_sha256: key },
        )
        .await
        .unwrap();
        assert_eq!(leave(&h, &job.job, MachineSetupPhase::Detecting).await.phase, MachineSetupPhase::Ready);
        as_mac(&h, &job.job);
        let settings = MachineSetupSettings {
            name: "lab-c".into(),
            host: "127.0.0.1:7430".into(),
            work_root: lab.root.join("work").to_string_lossy().into(),
            work_mount: None,
            harness: vec![],
            login: InstallLogin::Defer,
        };
        h.call(DeviceRole::Controller, WEB, FrontRequest::MachineSetupPlan { job: job.job.clone(), settings })
            .await
            .unwrap();
        let failed = leave(&h, &job.job, MachineSetupPhase::Planning).await;
        assert_eq!(failed.phase, MachineSetupPhase::Failed);
        assert!(failed.error.as_deref().unwrap().contains("시스템 SSH 서버"), "{:?}", failed.error);
        assert_eq!(h.setup_load(&job.job).unwrap().remote_dir, None);
        assert_eq!(remote_setup_dirs(&fake), Vec::<PathBuf>::new(), "원격 임시 폴더를 지웠다");
        assert!(!install::plan_file(&h.state, &job.job).exists());
        Arc::downgrade(&h)
    });
    drop(rt);
    install::tests_binary::set(None);
    assert!(weak.upgrade().is_none());
}

// ───────────── 다른 종류 머신용 release(M3) ─────────────

#[test]
fn a_release_binary_is_fetched_only_from_its_release_with_the_manifest_sha_and_cached() {
    assert!(release::version_ok("0.1.2") && release::version_ok("10.0.0"));
    for bad in ["0.1", "0.01.2", "v0.1.2", "0.1.2+abc", "a.b.c"] {
        assert!(!release::version_ok(bad), "{bad}");
    }
    let binary = b"#!/bin/sh\necho 'aaa 1.2.3'\n".to_vec();
    let sha = hex::encode(Sha256::digest(&binary));
    let manifest = |version: &str, url: &str, sha: &str| {
        serde_json::to_vec(&serde_json::json!({
            "version": version,
            "assets": { "macos-x86_64": { "url": url, "sha256": sha } }
        }))
        .unwrap()
    };
    let root = std::env::temp_dir().join(format!("aaa-release-{}", aaa_common::util::new_id()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        // 정상: 그 판 release 파일, SHA 같음 → 캐시에 둔다. 다시 받으면 asset은 받지 않는다
        let srv = http_with(|base| {
            vec![
                (
                    "/releases/download/v1.2.3/manifest.json".into(),
                    manifest("1.2.3", &format!("{base}/download/v1.2.3/aaa-macos-x86_64"), &sha),
                ),
                ("/releases/download/v1.2.3/aaa-macos-x86_64".into(), binary.clone()),
            ]
        });
        let cache = root.join("ok");
        let got = release::fetch(&cache, &srv.base, "1.2.3", release::Kind::Binary, "macos-x86_64").await.unwrap();
        assert_eq!((std::fs::read(&got.path).unwrap(), got.sha256.as_str()), (binary.clone(), sha.as_str()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(got.path.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let again = release::fetch(&cache, &srv.base, "1.2.3", release::Kind::Binary, "macos-x86_64").await.unwrap();
        assert_eq!(again, got);
        let assets = srv.hits.lock().unwrap().iter().filter(|p| p.ends_with("aaa-macos-x86_64")).count();
        assert_eq!(assets, 1, "같은 SHA 캐시는 다시 받지 않는다");
        // 옛 manifest에는 launcher가 없다
        assert!(
            release::fetch(&cache, &srv.base, "1.2.3", release::Kind::Launcher, "macos-x86_64")
                .await
                .unwrap_err()
                .contains("launcher가 없다")
        );
        // 없는 종류
        assert!(
            release::fetch(&cache, &srv.base, "1.2.3", release::Kind::Binary, "linux-aarch64")
                .await
                .unwrap_err()
                .contains("linux-aarch64")
        );

        // SHA가 manifest와 다르면 쓰지 않고 남기지 않는다
        let bad = http_with(|base| {
            vec![
                (
                    "/releases/download/v1.2.3/manifest.json".into(),
                    manifest("1.2.3", &format!("{base}/download/v1.2.3/aaa-macos-x86_64"), &"0".repeat(64)),
                ),
                ("/releases/download/v1.2.3/aaa-macos-x86_64".into(), binary.clone()),
            ]
        });
        let cache = root.join("bad");
        assert!(
            release::fetch(&cache, &bad.base, "1.2.3", release::Kind::Binary, "macos-x86_64")
                .await
                .unwrap_err()
                .contains("SHA-256")
        );
        assert!(
            !cache.join("v1.2.3/macos-x86_64/aaa").exists() && !cache.join("v1.2.3/macos-x86_64/aaa.part").exists()
        );

        // manifest 판이 다르거나, asset 주소가 그 판의 release 파일이 아니면 받지 않는다
        let other = http_with(|base| {
            vec![(
                "/releases/download/v1.2.3/manifest.json".into(),
                manifest("1.2.4", &format!("{base}/download/v1.2.4/aaa-macos-x86_64"), &sha),
            )]
        });
        assert!(
            release::fetch(&root.join("v"), &other.base, "1.2.3", release::Kind::Binary, "macos-x86_64")
                .await
                .unwrap_err()
                .contains("1.2.4")
        );
        let away = http_with(|_| {
            vec![("/releases/download/v1.2.3/manifest.json".into(), manifest("1.2.3", "https://example.com/aaa", &sha))]
        });
        assert!(
            release::fetch(&root.join("a"), &away.base, "1.2.3", release::Kind::Binary, "macos-x86_64")
                .await
                .unwrap_err()
                .contains("공개 release 파일")
        );
        assert_eq!(away.hits.lock().unwrap().len(), 1, "manifest만 읽고 다른 주소는 받지 않는다");
    });
    std::fs::remove_dir_all(root).unwrap();
}

/// 임시 HTTP 서버(경로 → 본문, `Connection: close`). 주소를 먼저 정하고 그 주소로 만든 파일 목록을 준다. 받은 경로를 차례로 남긴다
struct Http {
    base: String,
    hits: Arc<std::sync::Mutex<Vec<String>>>,
}

fn http_with(files: impl FnOnce(&str) -> Vec<(String, Vec<u8>)>) -> Http {
    let (base, hits, tx) = http_lazy();
    tx.send(files(&base).into_iter().collect()).unwrap();
    Http { base, hits }
}

type Lazy =
    (String, Arc<std::sync::Mutex<Vec<String>>>, std::sync::mpsc::Sender<std::collections::HashMap<String, Vec<u8>>>);

/// 주소를 먼저 내고 파일 목록은 나중에 받는 서버
fn http_lazy() -> Lazy {
    use std::io::{BufRead, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/releases", listener.local_addr().unwrap());
    let hits = Arc::new(std::sync::Mutex::new(vec![]));
    let seen = hits.clone();
    let (tx, rx) = std::sync::mpsc::channel::<std::collections::HashMap<String, Vec<u8>>>();
    std::thread::spawn(move || {
        let files = rx.recv().unwrap();
        for mut s in listener.incoming().flatten() {
            let mut r = std::io::BufReader::new(s.try_clone().unwrap());
            let mut line = String::new();
            if r.read_line(&mut line).is_err() {
                continue;
            }
            let mut h = String::new();
            while r.read_line(&mut h).is_ok_and(|n| n > 2) {
                h.clear();
            }
            let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
            seen.lock().unwrap().push(path.clone());
            let _ = match files.get(&path) {
                Some(body) => s
                    .write_all(
                        format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len())
                            .as_bytes(),
                    )
                    .and_then(|_| s.write_all(body)),
                None => s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
            };
        }
    });
    (base, hits, tx)
}

/// 감지한 CPU가 host와 다르면 같은 판 release 파일을 받아(임시 HTTP 서버) 원격에 보내고 SHA·판을 확인한다. 원격 install 대본은
/// 계획에서 이 SSH를 거부하게 둬(exit 3) 전송·확인까지만 본다
#[cfg(unix)]
#[test]
fn another_cpu_gets_the_same_release_version_from_the_release_and_verifies_it_remotely() {
    use aaa_proto::install::InstallLogin;
    let Some(lab) = lab() else { return };
    let fake = fake_remote_as(&lab.root, "aaa 1.2.3");
    std::fs::write(lab.root.join("remote/probe.exit"), "3").unwrap();
    let asset = std::fs::read(&fake).unwrap();
    let sha = hex::encode(Sha256::digest(&asset));
    let other = if std::env::consts::ARCH == "aarch64" { "x86_64" } else { "aarch64" };
    let key = format!("macos-{other}");
    // 같은 release의 launcher package(그 실행 파일 SHA에 묶임)
    let package = serde_json::to_vec(&serde_json::json!({
        "schema": 1, "version": "1.2.3", "platform": key, "binary_sha256": sha, "files": {}
    }))
    .unwrap();
    let package_sha = hex::encode(Sha256::digest(&package));
    let srv = http_with(|base| {
        vec![
            (
                "/releases/download/v1.2.3/manifest.json".into(),
                serde_json::to_vec(&serde_json::json!({
                    "version": "1.2.3",
                    "assets": { key.clone(): { "url": format!("{base}/download/v1.2.3/aaa-{key}"), "sha256": sha } },
                    "launchers": { key.clone(): {
                        "url": format!("{base}/download/v1.2.3/aaa-launcher-{key}.json"), "sha256": package_sha
                    } }
                }))
                .unwrap(),
            ),
            (format!("/releases/download/v1.2.3/aaa-{key}"), asset.clone()),
            (format!("/releases/download/v1.2.3/aaa-launcher-{key}.json"), package.clone()),
        ]
    });
    // host 자기 파일은 쓰지 않는다(다른 CPU): 판만 release 판으로
    install::tests_binary::set(Some((lab.root.join("not-sent"), "aaa 1.2.3".into())));
    install::tests_binary::set_base(Some(srv.base.clone()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let h = host_for(&lab);
        let job = start(&h, "lab").await;
        let job = leave(&h, &job.job, MachineSetupPhase::Connecting).await;
        let key_sha = job.host_keys[0].sha256.clone();
        h.call(
            DeviceRole::Controller,
            WEB,
            FrontRequest::MachineSetupTrust { job: job.job.clone(), host_key_sha256: key_sha },
        )
        .await
        .unwrap();
        assert_eq!(leave(&h, &job.job, MachineSetupPhase::Detecting).await.phase, MachineSetupPhase::Ready);
        // 실험실은 같은 머신이라 감지 CPU만 다른 종류로 바꾼다
        h.setup_update(&job.job, MachineSetupPhase::Ready, |j| {
            let d = j.view.detected.as_mut().unwrap();
            (d.os, d.arch) = ("macos".into(), other.into());
        })
        .unwrap();
        let settings = MachineSetupSettings {
            name: "lab-x".into(),
            host: "127.0.0.1:7430".into(),
            work_root: lab.root.join("work").to_string_lossy().into(),
            work_mount: None,
            harness: vec![],
            login: InstallLogin::Defer,
        };
        h.call(DeviceRole::Controller, WEB, FrontRequest::MachineSetupPlan { job: job.job.clone(), settings })
            .await
            .unwrap();
        let after = leave(&h, &job.job, MachineSetupPhase::Planning).await;
        assert!(
            after.error.as_deref().unwrap_or("").contains("시스템 SSH 서버"),
            "전송·확인을 지나 원격 계획까지 갔다: {:?}",
            after.error
        );
        let cached = h.state.join(format!("ssh/release/v1.2.3/{key}/aaa"));
        assert_eq!(std::fs::read(&cached).unwrap(), asset);
        assert_eq!(
            std::fs::read(lab.root.join("remote/probe.launcher")).unwrap(),
            package,
            "같은 판 release의 launcher를 원격 실행 파일 옆에 놓았다"
        );
        {
            use std::os::unix::fs::PermissionsExt;
            let kept = h.state.join(format!("ssh/release/v1.2.3/{key}/aaa-launcher-{key}.json"));
            assert_eq!(std::fs::metadata(kept).unwrap().permissions().mode() & 0o777, 0o600, "캐시도 실행 파일 옆");
        }
        let probed: Vec<String> =
            std::fs::read_to_string(lab.root.join("remote/probe.args")).unwrap().lines().map(str::to_string).collect();
        assert!(probed.contains(&sha), "원격 install에 release 파일의 SHA를 executor로 준다: {probed:?}");
        assert_eq!(srv.hits.lock().unwrap().iter().filter(|p| p.ends_with(&format!("aaa-{key}"))).count(), 1);
        Arc::downgrade(&h)
    });
    drop(rt);
    install::tests_binary::set(None);
    install::tests_binary::set_base(None);
    assert!(weak.upgrade().is_none());
}

/// launcher package는 보낼 실행 파일에 묶여야 보낸다(다르면 원격에 아무것도 남기지 않고 멈춤). 개발 판 host 옆에 없으면 보내지 않고,
/// 원격 install이 launcher를 사람 일로 알리면 host가 무엇을 준비할지 덧붙인다
#[cfg(unix)]
#[test]
fn a_launcher_package_must_bind_the_sent_binary_and_a_dev_host_without_one_says_what_to_prepare() {
    use aaa_proto::install::*;
    let Some(lab) = lab() else { return };
    let fake = fake_remote(&lab.root);
    let remote = lab.root.join("remote");
    let sha = hex::encode(Sha256::digest(std::fs::read(&fake).unwrap()));
    // 개발 판 대본(`--version`이 개발 판)은 따로 둔다
    let dev_root = lab.root.join("dev");
    std::fs::create_dir_all(&dev_root).unwrap();
    let dev = fake_remote_as(&dev_root, "aaa 9.9.9+abc");
    let dev_remote = dev_root.join("remote");
    let dev_sha = hex::encode(Sha256::digest(std::fs::read(&dev).unwrap()));
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let h = host_for(&lab);
        let ready = async |name: &str| {
            let job = start(&h, "lab").await;
            let job = leave(&h, &job.job, MachineSetupPhase::Connecting).await;
            let key = job.host_keys.first().map(|k| k.sha256.clone());
            if let Some(key) = key.filter(|_| job.phase == MachineSetupPhase::Trust) {
                h.call(
                    DeviceRole::Controller,
                    WEB,
                    FrontRequest::MachineSetupTrust { job: job.job.clone(), host_key_sha256: key },
                )
                .await
                .unwrap();
            }
            assert_eq!(leave(&h, &job.job, MachineSetupPhase::Detecting).await.phase, MachineSetupPhase::Ready);
            let detected = as_mac(&h, &job.job);
            let settings = MachineSetupSettings {
                name: name.into(),
                host: "127.0.0.1:7430".into(),
                work_root: lab.root.join("work").to_string_lossy().into(),
                work_mount: None,
                harness: vec![],
                login: InstallLogin::Defer,
            };
            (job.job, settings, detected)
        };

        // 다른 실행 파일에 묶인 package: 원격 계획 전에 멈추고 원격 임시 폴더를 지운다
        let launcher = fake_launcher(&fake, "9.9.9");
        let mut wrong: serde_json::Value = serde_json::from_slice(&std::fs::read(&launcher).unwrap()).unwrap();
        wrong["binary_sha256"] = "0".repeat(64).into();
        std::fs::write(&launcher, serde_json::to_vec(&wrong).unwrap()).unwrap();
        install::tests_binary::set(Some((fake.clone(), "aaa 9.9.9".into())));
        let (job, settings, _) = ready("lab-d").await;
        h.call(DeviceRole::Controller, WEB, FrontRequest::MachineSetupPlan { job: job.clone(), settings })
            .await
            .unwrap();
        let failed = leave(&h, &job, MachineSetupPhase::Planning).await;
        assert_eq!(failed.phase, MachineSetupPhase::Failed);
        assert!(failed.error.as_deref().unwrap().contains("묶이지 않았다"), "{:?}", failed.error);
        assert!(!remote.join("probe.in").exists(), "원격 install까지 가지 않았다");
        assert_eq!(remote_setup_dirs(&fake), Vec::<PathBuf>::new(), "원격 임시 폴더를 지웠다");

        // 개발 판 host 옆에 package가 없다: 보내지 않고 원격 install에 맡긴다. 원격이 launcher를 사람 일로 알리면 준비할 것을 보인다
        install::tests_binary::set(Some((dev.clone(), "aaa 9.9.9+abc".into())));
        let (job, settings, detected) = ready("lab-e").await;
        let mut proposal = fake_proposal(install::choices(&settings, &detected).unwrap(), &dev_sha, "macos");
        proposal.result.status = InstallStatus::NeedsUser;
        proposal.result.needs_user = vec![InstallNeedsUser {
            code: "installation_launcher_asset_required".into(),
            target: "launcher".into(),
            message: "같은 release의 launcher JSON asset을 설치 원본 실행 파일 옆에 준비하세요".into(),
        }];
        std::fs::write(dev_remote.join("probe.out"), serde_json::to_vec(&proposal).unwrap()).unwrap();
        h.call(DeviceRole::Controller, WEB, FrontRequest::MachineSetupPlan { job: job.clone(), settings })
            .await
            .unwrap();
        let asked = leave(&h, &job, MachineSetupPhase::Planning).await;
        assert_eq!(asked.phase, MachineSetupPhase::NeedsUser, "{:?}", asked.error);
        let why = asked.error.unwrap();
        assert!(
            why.contains(&format!("aaa-launcher-macos-{}.json", std::env::consts::ARCH)) && why.contains("release 판"),
            "{why}"
        );
        assert!(
            dev_remote.join("probe.in").exists() && !dev_remote.join("probe.launcher").exists(),
            "launcher 없이 계획했다"
        );
        assert_eq!(remote_setup_dirs(&dev), Vec::<PathBuf>::new());

        // release 번호 판인데 옆에 없고 그 release에도 launcher가 없다(옛 release, 또는 같은 번호의 개발 빌드): 보내지 않고 까닭을 덧붙인다
        std::fs::remove_file(&launcher).unwrap();
        let old = http_with(|_| {
            vec![(
                "/releases/download/v9.9.9/manifest.json".into(),
                serde_json::to_vec(&serde_json::json!({ "version": "9.9.9", "assets": {} })).unwrap(),
            )]
        });
        install::tests_binary::set(Some((fake.clone(), "aaa 9.9.9".into())));
        install::tests_binary::set_base(Some(old.base.clone()));
        let (job, settings, detected) = ready("lab-f").await;
        let mut proposal = fake_proposal(install::choices(&settings, &detected).unwrap(), &sha, "macos");
        proposal.result.status = InstallStatus::NeedsUser;
        proposal.result.needs_user = vec![InstallNeedsUser {
            code: "installation_launcher_asset_required".into(),
            target: "launcher".into(),
            message: "같은 release의 launcher JSON asset을 설치 원본 실행 파일 옆에 준비하세요".into(),
        }];
        std::fs::write(remote.join("probe.out"), serde_json::to_vec(&proposal).unwrap()).unwrap();
        h.call(DeviceRole::Controller, WEB, FrontRequest::MachineSetupPlan { job: job.clone(), settings })
            .await
            .unwrap();
        let asked = leave(&h, &job, MachineSetupPhase::Planning).await;
        assert_eq!(asked.phase, MachineSetupPhase::NeedsUser, "{:?}", asked.error);
        let why = asked.error.unwrap();
        assert!(why.contains("(release: aaa 9.9.9 release에") && why.contains("launcher가 없다"), "{why}");
        assert!(!remote.join("probe.launcher").exists());
        assert_eq!(old.hits.lock().unwrap().len(), 1, "manifest만 읽었다");
        Arc::downgrade(&h)
    });
    drop(rt);
    install::tests_binary::set(None);
    install::tests_binary::set_base(None);
    assert!(weak.upgrade().is_none());
}

// ───────────── 끝 확인·harness 로그인 pane(M3) ─────────────

#[cfg(unix)]
#[test]
fn the_check_script_reports_the_work_folder_git_harnesses_and_path() {
    let dir = std::env::temp_dir().join(format!("aaa-check-{}", aaa_common::util::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let argv = check::check_argv();
    let out = std::process::Command::new(&argv[0]).args(&argv[1..]).current_dir(&dir).output().unwrap();
    let lines = check::check_lines(&String::from_utf8_lossy(&out.stdout)).unwrap();
    let real = dir.canonicalize().unwrap();
    assert!(
        lines[0] == format!("작업 폴더 {}", real.display()) || lines[0] == format!("작업 폴더 {}", dir.display()),
        "{lines:?}"
    );
    assert!(lines[1].starts_with("git version") || lines[1] == "git 없음", "{lines:?}");
    assert!(lines[2].starts_with("claude ") && lines[3].starts_with("codex "), "{lines:?}");
    assert!(lines[4].starts_with("PATH "), "{lines:?}");
    // 화면 앞뒤 잡음과 표지 없는 출력
    assert_eq!(check::check_lines("$ x\nAAA-CHECK-1\na\nb\nAAA-CHECK-END\n$ "), Some(vec!["a".into(), "b".into()]));
    assert_eq!(check::check_lines("AAA-CHECK-END"), None);
    assert_eq!(
        check::login_argv(aaa_proto::install::InstallHarness::Codex),
        Some(vec!["codex".into(), "login".into()])
    );
    assert_eq!(check::login_argv(aaa_proto::install::InstallHarness::None), None);
    std::fs::remove_dir_all(dir).unwrap();
}

/// 가짜 agent(duplex 연결)에 등록된 머신: 끝 확인은 작업 root에서 고정 스크립트 pane을 띄우고 그 화면의 표지 사이를 작업에 남긴다.
/// 로그인은 `codex login` pane. 끝나지 않은 작업·보기 기기·연결 안 된 머신은 거부한다
#[test]
fn an_installed_machine_gets_a_check_pane_and_a_login_pane() {
    use aaa_common::conn::{FrameRx, spawn_writer};
    use aaa_proto::{AgentRequest, AgentResponse, LinkMsg, MachineSetupInstall, SessionInfo, SessionState};
    let root = std::env::temp_dir().join(format!("aaa-ssh-check-{}", aaa_common::util::new_id()));
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let weak = rt.block_on(async {
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        let installed = |id: &str, phase: MachineSetupPhase| Job {
            view: MachineSetupView {
                job: id.into(),
                target: "mac-b".into(),
                phase,
                ssh: None,
                host_keys: vec![],
                host_key_state: Some("known".into()),
                detected: None,
                error: None,
                created_ms: 1,
                updated_ms: 1,
                settings: Some(MachineSetupSettings {
                    name: "lab-b".into(),
                    host: "127.0.0.1:7430".into(),
                    work_root: work.to_string_lossy().into(),
                    work_mount: None,
                    harness: vec![],
                    login: aaa_proto::install::InstallLogin::Defer,
                }),
                review: None,
                install: Some(Box::new(MachineSetupInstall {
                    installation_id: None,
                    last_event: None,
                    result: None,
                    machine: Some("lab-b".into()),
                    checks: vec![],
                    check_pane: None,
                })),
            },
            key_lines: vec![],
            remote_dir: None,
        };
        h.setup_save(&mut installed("done", MachineSetupPhase::Installed));
        h.setup_save(&mut installed("review", MachineSetupPhase::Review));
        let check = |job: &str| FrontRequest::MachineSetupCheck { job: job.into() };
        // 머신이 아직 없다(등록 전·폐기)
        assert_eq!(h.call(DeviceRole::Controller, WEB, check("done")).await.unwrap_err().code, ErrorCode::NotFound);
        // 등록됐지만 agent가 연결 안 됨(감독자가 아직 agent를 띄우지 않음)
        let offline = || crate::MachineRt {
            name: "lab-b".into(),
            local: false,
            os: None,
            boot_id: None,
            dev_mode: true,
            conn: None,
            last_seen_ms: None,
            health: None,
            health_sent: None,
            features: vec![],
            agent_pid: None,
            colors: None,
            build: Default::default(),
            identity: None,
        };
        h.inner.lock().unwrap().machines.insert("m1".into(), offline());
        let e = h.call(DeviceRole::Controller, WEB, check("done")).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::AgentDisconnected, "{}", e.message);
        // 연결된 가짜 agent: Spawn을 받아 남기고 세션으로 답한다
        let (writer, reader) = tokio::io::duplex(65536);
        let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
        h.inner
            .lock()
            .unwrap()
            .machines
            .insert("m1".into(), crate::MachineRt { conn: Some(conn.clone()), ..offline() });
        let spawned = Arc::new(std::sync::Mutex::new(Vec::<aaa_proto::SpawnSpec>::new()));
        let seen = spawned.clone();
        tokio::spawn(async move {
            let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
            let mut n = 0;
            while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                let reply = match body {
                    AgentRequest::Spawn(spec) => {
                        n += 1;
                        let info = SessionInfo {
                            responsibility: None,
                            id: format!("s{n}"),
                            name: spec.name.clone(),
                            argv: spec.argv.clone(),
                            cwd: spec.cwd.clone(),
                            pid: None,
                            started_at_ms: 1,
                            boot_id: "b".into(),
                            cols: spec.cols,
                            rows: spec.rows,
                            state: SessionState::Running,
                            role: None,
                            label: None,
                            harness: None,
                        };
                        seen.lock().unwrap().push(spec);
                        AgentResponse::Session(info)
                    }
                    _ => AgentResponse::Done,
                };
                if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                    tx.send(aaa_proto::Outcome::Ok(reply)).unwrap();
                }
            }
        });
        assert_eq!(h.call(DeviceRole::Controller, WEB, check("review")).await.unwrap_err().code, ErrorCode::Conflict);
        assert!(h.call(DeviceRole::Reader, "device:tv", check("done")).await.is_err());
        let FrontResponse::MachineSetup { setup } = h.call(DeviceRole::Controller, WEB, check("done")).await.unwrap()
        else {
            panic!()
        };
        let pane = setup.install.as_ref().unwrap().check_pane.clone().unwrap();
        {
            let specs = spawned.lock().unwrap();
            assert_eq!(
                (specs[0].argv.clone(), specs[0].cwd.clone()),
                (check::check_argv(), Some(work.to_string_lossy().into()))
            );
        }
        // agent가 보낼 화면: 이 머신에서 그 스크립트를 실제로 돌린 출력
        let argv = check::check_argv();
        let out = std::process::Command::new(&argv[0]).args(&argv[1..]).current_dir(&work).output().unwrap();
        let screen = String::from_utf8_lossy(&out.stdout).replace('\n', "\r\n");
        h.on_subscribed(
            &pane,
            Some(aaa_proto::Snapshot {
                session: pane.clone(),
                offset: 0,
                cols: 160,
                rows: 40,
                replay: aaa_proto::B64(screen.into_bytes()),
                history_more: false,
            }),
            0,
        );
        let mut rx = h.enroll_changed.subscribe();
        let checks = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let v = h.setup_load("done").unwrap().view;
                let c = v.install.unwrap().checks;
                if !c.is_empty() {
                    return c;
                }
                rx.changed().await.unwrap();
            }
        })
        .await
        .expect("끝 확인 줄");
        assert!(checks[0].starts_with("작업 폴더 ") && checks.iter().any(|l| l.starts_with("PATH ")), "{checks:?}");
        // harness 로그인 pane
        let login =
            |h: aaa_proto::install::InstallHarness| FrontRequest::MachineSetupLogin { job: "done".into(), harness: h };
        let FrontResponse::Pane(p) =
            h.call(DeviceRole::Controller, WEB, login(aaa_proto::install::InstallHarness::Codex)).await.unwrap()
        else {
            panic!()
        };
        assert_eq!(p.name, "codex 로그인");
        assert_eq!(spawned.lock().unwrap()[1].argv, vec!["codex".to_string(), "login".into()]);
        assert_eq!(
            h.call(DeviceRole::Controller, WEB, login(aaa_proto::install::InstallHarness::None))
                .await
                .unwrap_err()
                .code,
            ErrorCode::BadRequest
        );
        Arc::downgrade(&h)
    });
    drop(rt);
    assert!(weak.upgrade().is_none());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn windows_choices_use_scm_and_remote_defaults_with_a_console_fallback() {
    use aaa_proto::install::*;
    let settings = MachineSetupSettings {
        name: "pc-b".into(),
        host: "mac-a.example.ts.net:7430".into(),
        work_root: r"D:\작업 폴더\aaa-work".into(),
        work_mount: Some("D:/".into()),
        harness: vec![],
        login: InstallLogin::Defer,
    };
    let detected = MachineDetected {
        os: "windows".into(),
        arch: "x86_64".into(),
        raw: "Windows AMD64".into(),
        chosen: false,
        home: Some(r"C:\Users\시험 사용자".into()),
        shell: None,
        aaa: None,
    };
    install::check_settings(&settings).unwrap();
    let choices = install::choices(&settings, &detected).unwrap();
    assert_eq!(choices.service, Some(InstallService::WindowsScm));
    assert_eq!(choices.service_account, Some(InstallAccount::CurrentUser));
    assert_eq!(choices.service_password_source, Some(InstallPasswordSource::Prompt));
    assert!(choices.state_dir.is_none() && choices.bin_path.is_none() && choices.launcher_policy.is_none());
    for root in ["C:relative", r"C:\a\..\b", r"\\server\share", "/a/b", "C:/a\n"] {
        assert!(
            install::choices(&MachineSetupSettings { work_root: root.into(), ..settings.clone() }, &detected).is_err()
        );
    }
    let dir = format!("aaa-setup.{}", "a".repeat(32));
    assert!(windows::safe_dir(&dir));
    for bad in ["aaa-setup.", "aaa-setup.abc/..", "aaa-setup.';exit", "C:/temp/aaa-setup.abc"] {
        assert!(!windows::safe_dir(bad));
    }
    use base64::Engine as _;
    let decode = |s: String| {
        let bytes = base64::engine::general_purpose::STANDARD.decode(s.split_whitespace().last().unwrap()).unwrap();
        String::from_utf16(&bytes.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect::<Vec<_>>())
            .unwrap()
    };
    let apply = decode(install::apply_script(&dir, "windows", "01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap());
    assert!(apply.contains("--secrets-stdin") && apply.contains("$stdinStream.CopyTo($p.StandardInput.BaseStream)"));
    assert!(decode(windows::upload()).contains("Get-FileHash -LiteralPath"));
    assert!(windows::cleanup("aaa-setup.bad").is_none());
    assert_eq!(check::windows_check_argv()[0], "powershell.exe");
}

/// 가짜 암호는 실행 중에만 만들고 원문을 assertion/로그/파일에 넣지 않는다.
#[cfg(unix)]
#[test]
fn service_password_is_web_only_and_never_in_views_or_persistent_records() {
    let Some(lab) = lab() else { return };
    let secret =
        aaa_proto::MachineSetupPassword(format!("{}{}", aaa_common::util::new_id(), aaa_common::util::new_id()));
    let request = || FrontRequest::MachineSetupApply {
        job: aaa_common::util::new_id(),
        plan_sha256: "a".repeat(64),
        service_password: Some(secret.clone()),
    };
    assert!(!format!("{:?}", request()).contains(&secret.0), "요청 Debug에 보호 입력이 있다");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let h = host_for(&lab);
        for (role, actor) in [
            (DeviceRole::Local, "local:cli"),
            (DeviceRole::Local, "local:thread@session"),
            (DeviceRole::Local, "local:coordinator@session"),
            (DeviceRole::Reader, "device:tv"),
            (DeviceRole::Reader, "worker:session"),
            (DeviceRole::Reader, "evaluator:session"),
            (DeviceRole::Reader, "detached:session"),
        ] {
            assert_eq!(h.call(role, actor, request()).await.unwrap_err().code, ErrorCode::Forbidden);
        }
        let view = serde_json::to_string(&h.setup_list().unwrap()).unwrap();
        assert!(!view.contains(&secret.0), "조회에 보호 입력이 있다");
        // 일회 입력은 잘못된 job에도 응답에 되돌아오지 않는다.
        let error = h.call(DeviceRole::Controller, WEB, request()).await.unwrap_err();
        assert!(!serde_json::to_string(&error).unwrap().contains(&secret.0), "오류에 보호 입력이 있다");
    });
    fn scan(root: &Path, needle: &[u8]) {
        for entry in std::fs::read_dir(root).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, needle);
            } else {
                assert!(
                    !std::fs::read(path).unwrap().windows(needle.len()).any(|w| w == needle),
                    "영속 기록에 보호 입력이 있다"
                );
            }
        }
    }
    // host DB/WAL·감사·프로젝트 작업/보고 등 이 시험이 만든 전체 자료를 검사한다.
    scan(&lab.root, secret.0.as_bytes());
}

#[cfg(unix)]
#[test]
fn service_password_frames_cross_real_ssh_stdin_and_remote_echo_is_scrubbed() {
    use aaa_proto::{MachineSetupPassword, install::*};
    let Some(lab) = lab() else { return };
    let password =
        MachineSetupPassword(format!("{}{} 한글 ' \\", aaa_common::util::new_id(), aaa_common::util::new_id()));
    let authorization = InstallAuthorization {
        claims: InstallAuthorizationClaims {
            schema: InstallAuthorizationSchema::V1,
            installation_id: aaa_common::util::new_id(),
            plan_sha256: "a".repeat(64),
            platform: InstallPlatform { os: InstallOs::Windows, arch: InstallArch::X86_64 },
            target_account: "test-account".into(),
            instance: None,
            binary_sha256: "b".repeat(64),
            permissions: vec![],
            issued_at_ms: 0,
            expires_at_ms: 1,
            nonce: "invalid-fixture".into(),
            issuer: "invalid-fixture".into(),
        },
        signature: "invalid-fixture".into(),
        enrollment: None,
    };
    let input = secret::framed(&authorization, Some(&password)).unwrap();
    let digest = hex::encode(Sha256::digest(password.0.as_bytes()));
    // 원격은 frame과 실제 EOF를 검사한다. 암호는 파일·argv가 아닌 stdin에서만 받고, 잘못된 원격의 echo도 시험한다.
    let body = format!(r#"import sys,json,hashlib
b=sys.stdin.buffer.read()
assert b.startswith(b"aaa.install.input.v1\x00")
b=b[21:]
frames={{}}
while b:
 k=b[0]; n=int.from_bytes(b[1:5],"little"); frames[k]=b[5:5+n]; b=b[5+n:]
assert set(frames)=={{1,2}}
assert json.loads(frames[1])["claims"]["platform"]["os"]=="windows"
assert hashlib.sha256(frames[2]).hexdigest()=="{digest}"
print(json.dumps({{"type":"event","schema":"aaa.install.event.v1","installation_id":None,"seq":1,"phase":"configure","state":"running","code":"fixture","message":frames[2].decode()}}))
"#).replace("\n+", "\n");
    // 작은따옴표 shell 인용은 피하고 공개한 시험 코드만 base64로 싣는다.
    use base64::Engine as _;
    let code = base64::engine::general_purpose::STANDARD.encode(body.as_bytes());
    let command = format!("python3 -c 'import base64;exec(base64.b64decode(\"{code}\"))'");
    let known = lab.root.join("known");
    std::fs::write(&known, format!("[127.0.0.1]:{} {}\n", lab.port, lab.host_pub)).unwrap();
    let ssh = Ssh { config: Some(lab.root.join("ssh_config")), known };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(async {
        let mut events = vec![];
        let (ran, code) = ssh
            .stream(&parse_target("lab").unwrap(), &command, input, RUN_TIMEOUT, |line| {
                if let install::Line::Event(event) = install::stream_line(line) {
                    events.push(secret::scrub(&event, Some(&password)));
                }
            })
            .await;
        assert!(ran.ok && code == Some(0), "보호 stdin fixture 실패");
        assert_eq!(events, ["[비밀]"]);
        assert!(!ran.stderr.contains(&password.0), "전송 진단에 보호 입력이 있다");
    });
    let mut result = InstallResult::invalid();
    result.error.as_mut().unwrap().message = password.0.clone();
    let clean = secret::result(&result, Some(&password)).unwrap();
    assert!(!serde_json::to_string(&clean).unwrap().contains(&password.0), "설치 오류에 보호 입력이 있다");
}
