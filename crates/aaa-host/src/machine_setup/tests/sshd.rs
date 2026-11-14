//! 실험실 sshd의 포트 인계·준비 사건·실패 근거. 제품 SSH 경로에는 쓰지 않는다.
use std::io::{BufRead, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

#[derive(Debug)]
pub(super) struct Sshd {
    child: Child,
}

impl Drop for Sshd {
    fn drop(&mut self) {
        // panic 경로에서도 이 시험이 띄운 자식 PID만 끝내고 회수한다.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub(super) fn event(root: &Path, text: &str) {
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(root.join("sshd.events")).unwrap();
    writeln!(file, "{text}").unwrap();
}

pub(super) fn reserve_port() -> (u16, Option<tokio::net::TcpSocket>) {
    // Linux는 listen 전의 SO_REUSEADDR 주소를 sshd(SO_REUSEADDR)와 공유할 수 있다.
    // 닫았다 다시 bind하면 그사이 다른 실험실·SSH 클라이언트가 이 포트를 가져간다.
    // macOS는 이 공유를 허용하지 않으므로 기존 인계를 보존한다.
    #[cfg(target_os = "linux")]
    {
        let socket = tokio::net::TcpSocket::new_v4().unwrap();
        socket.set_reuseaddr(true).unwrap();
        socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
        (socket.local_addr().unwrap().port(), Some(socket))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        (listener.local_addr().unwrap().port(), None)
    }
}

fn failure(root: &Path, reason: &str) -> String {
    eprintln!("실험실 기록을 남겼다: {}", root.display());
    format!(
        "실험실 sshd가 뜨지 않았다: {reason}\n사건:\n{}stderr:\n{}",
        std::fs::read_to_string(root.join("sshd.events")).unwrap(),
        std::fs::read_to_string(root.join("sshd.stderr")).unwrap(),
    )
}

pub(super) fn start(sshd: &Path, root: &Path, port: u16) -> Result<Sshd, String> {
    let mut log = std::fs::File::create(root.join("sshd.stderr")).unwrap();
    event(root, &format!("config-check command={} -t -f <lab>/sshd_config", sshd.display()));
    let checked = Command::new(sshd)
        .arg("-t")
        .arg("-f")
        .arg(root.join("sshd_config"))
        .output()
        .map_err(|error| failure(root, &format!("설정 검사 실행: {error}")))?;
    log.write_all(&checked.stderr).unwrap();
    event(root, &format!("config-check status={}", checked.status));
    if !checked.status.success() {
        return Err(failure(root, "설정 검사 실패"));
    }
    event(root, &format!("spawn command={} -f <lab>/sshd_config -D -e port={port}", sshd.display()));
    let mut child = Command::new(sshd)
        .arg("-f")
        .arg(root.join("sshd_config"))
        .args(["-D", "-e"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| failure(root, &format!("자식 실행: {error}")))?;
    event(root, &format!("spawned pid={}", child.id()));
    let stderr = child.stderr.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let ready = format!("Server listening on 127.0.0.1 port {port}.");
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stderr).lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    let _ = tx.send(Err(format!("stderr 읽기: {error}")));
                    return;
                }
            };
            if let Err(error) = writeln!(log, "{line}") {
                let _ = tx.send(Err(format!("stderr 보존: {error}")));
                return;
            }
            if line.trim() == ready {
                let _ = tx.send(Ok(()));
            }
        }
        let _ = tx.send(Err("ready 전 stderr EOF".into()));
    });
    // 상한은 기존 20초 그대로다. 준비 줄·EOF·읽기 오류 사건이 즉시 끝낸다.
    let reason = match rx.recv_timeout(Duration::from_secs(20)) {
        Ok(Ok(())) => {
            event(root, "ready matching-listen-address");
            return Ok(Sshd { child });
        }
        Ok(Err(reason)) => reason,
        Err(error) => format!("ready 수신: {error}"),
    };
    // EOF 뒤에는 sshd의 실제 종료 코드·신호를 회수한다. 시한/읽기 실패면 자기 자식만 정리한다.
    if reason != "ready 전 stderr EOF" {
        event(root, &format!("before-cleanup status={:?}", child.try_wait()));
        let _ = child.kill();
    }
    event(root, &format!("exit status={:?}", child.wait()));
    Err(failure(root, &reason))
}

fn root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("aaa-ssh-{}", aaa_common::util::new_id()));
    std::fs::create_dir(&root).unwrap();
    root
}

fn config(root: &Path, port: u16) {
    super::keygen(&root.join("hostkey"));
    std::fs::write(
        root.join("sshd_config"),
        format!(
            "ListenAddress 127.0.0.1:{port}\nHostKey {}/hostkey\nPidFile {}/sshd.pid\nUsePAM no\nPasswordAuthentication no\nKbdInteractiveAuthentication no\n",
            root.display(), root.display()
        ),
    ).unwrap();
}

#[test]
fn an_occupied_port_reports_bind_stderr_and_the_child_exit_status() {
    use std::os::unix::process::ExitStatusExt;
    let Some(sshd) = super::sshd_bin() else { return };
    let root = root();
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = occupied.local_addr().unwrap().port();
    config(&root, port);
    let error = start(&sshd, &root, port).unwrap_err();
    assert!(error.contains("config-check status=exit status: 0"), "{error}");
    assert!(error.contains("Address already in use"), "{error}");
    let expected = std::process::ExitStatus::from_raw(255 << 8);
    assert!(error.contains(&format!("exit status=Ok({expected:?})")), "{error}");
    assert!(error.contains("ready 전 stderr EOF"), "{error}");
    assert!(!error.contains("ready matching-listen-address"), "{error}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_bad_config_reports_the_check_error_without_starting_a_daemon() {
    let Some(sshd) = super::sshd_bin() else { return };
    let root = root();
    std::fs::write(root.join("sshd_config"), "InvalidLabSetting yes\n").unwrap();
    let error = start(&sshd, &root, 12345).unwrap_err();
    assert!(error.contains("설정 검사 실패"), "{error}");
    assert!(error.contains("Bad configuration option: InvalidLabSetting"), "{error}");
    assert!(error.contains("config-check status=exit status: 255"), "{error}");
    assert!(!error.contains("spawn command="), "{error}");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn an_early_signal_reports_stderr_and_the_termination_signal() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::ExitStatusExt;
    let root = root();
    let stub = root.join("sshd-stub");
    std::fs::write(
        &stub,
        "#!/bin/sh\nif [ \"$1\" = -t ]; then exit 0; fi\necho lab-before-signal >&2\nkill -TERM $$\n",
    )
    .unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
    let error = start(&stub, &root, 12345).unwrap_err();
    assert!(error.contains("lab-before-signal"), "{error}");
    let status = std::process::ExitStatus::from_raw(15);
    assert!(error.contains(&format!("exit status=Ok({status:?})")), "{error}");
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn a_reserved_port_rejects_competitors_until_the_real_sshd_is_listening() {
    let Some(sshd) = super::sshd_bin() else { return };
    let root = root();
    let (port, reservation) = reserve_port();
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    // std::TcpListener는 Unix에서 SO_REUSEADDR를 켜므로 sshd와 같은 공유자가 된다.
    // 재사용을 끈 경쟁자와 빈 포트 할당에서 주소가 예약돼 있는지를 본다.
    let competing = tokio::net::TcpSocket::new_v4().unwrap();
    competing.set_reuseaddr(false).unwrap();
    let competing = competing.bind(address).unwrap_err();
    assert_eq!(competing.raw_os_error(), Some(98));
    // listen하지 않는 예약 소켓이므로 준비 전 연결을 받아 버리지 않는다.
    assert_eq!(std::net::TcpStream::connect(address).unwrap_err().raw_os_error(), Some(111));
    config(&root, port);
    let child = start(&sshd, &root, port).unwrap();
    // 예약을 아직 쥔 상태에서 실제 sshd의 배너를 읽는다.
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    stream.write_all(b"SSH-2.0-aaa-lab\r\n").unwrap();
    let mut banner = String::new();
    std::io::BufReader::new(stream).read_line(&mut banner).unwrap();
    drop(child);
    assert!(banner.starts_with("SSH-2.0-OpenSSH_"), "{banner}");
    assert!(std::fs::read_to_string(root.join("sshd.events")).unwrap().contains("ready matching-listen-address"));
    drop(reservation);
    // sshd의 세션 자식이 stderr를 닫기 전일 수 있어 실행 중 기록은 남긴다.
}
