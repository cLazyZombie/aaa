//! SSH 머신 추가 M2(plan §8.375). 순수 규칙(대상·`ssh -G`·지문·감지 출력)과, 실험실 sshd(비특권·loopback·비밀번호 끔)를 직접 띄워
//! host 요청으로 접속 → 지문 믿음 → 감지 → 아는 키·바뀐 키까지 도는 시험. sshd가 없는 머신(Windows runner 등)에서는 실험실 시험을
//! 건너뛰고 그렇게 적는다.
use super::*;
use aaa_proto::{DeviceRole, FrontRequest, FrontResponse};

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
            },
            key_lines: vec![],
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
