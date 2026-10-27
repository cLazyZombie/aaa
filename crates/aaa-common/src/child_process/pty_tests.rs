//! Linux openpty도 원자적인 CLOEXEC가 아니다. 실제 호출을 격리 worker 안에서만 붙잡는다.
use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::{AsFd, OwnedFd},
        unix::net::UnixStream,
    },
    path::PathBuf,
    sync::{OnceLock, mpsc},
    time::Duration,
};

const LIMIT: Duration = Duration::from_secs(2);
const FIXTURE: &str = "child_process::pty_tests::subprocess_fixture";
static WINDOW: OnceLock<(mpsc::Sender<i32>, std::sync::Mutex<mpsc::Receiver<()>>)> = OnceLock::new();

extern "C" fn pty_window(_master: i32, slave: i32) {
    let (entered, release) = WINDOW.get().unwrap();
    entered.send(slave).unwrap();
    release.lock().unwrap().recv_timeout(LIMIT).expect("openpty 창 해제");
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
        }
        self.0.wait().unwrap();
    }
}

#[tokio::test]
async fn pty_open_window_does_not_leak_slave_or_delay_eof_until_another_child_exits() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("pty-window-{}", crate::util::new_id()));
    std::fs::create_dir_all(&root).unwrap();
    let library = root.join("window.so");
    let built = Command::new("cc")
        .args(["-shared", "-fPIC", "-std=c11"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/child_process/pty_window.c"))
        .arg("-ldl")
        .arg("-o")
        .arg(&library)
        .capture_output()
        .unwrap();
    assert!(built.status.success(), "cc: {}", String::from_utf8_lossy(&built.stderr));
    for mode in ["raw", "std", "tokio"] {
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", FIXTURE, "--ignored", "--nocapture"])
            .env("AAA_PTY_ROLE", "worker")
            .env("AAA_PTY_MODE", mode)
            .env("LD_PRELOAD", &library)
            .kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(5), command.output_guarded())
            .await
            .expect("PTY worker 종료")
            .unwrap();
        assert!(
            output.status.success(),
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("{mode}: {}", String::from_utf8_lossy(&output.stdout));
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "위 owner가 LD_PRELOAD로 격리 실행한다"]
fn subprocess_fixture() {
    if std::env::var("AAA_PTY_ROLE").unwrap() == "holder" {
        let fd: i32 = std::env::var("AAA_PTY_FD").unwrap().parse().unwrap();
        let inherited = unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0;
        let mut control = UnixStream::from(std::io::stdin().as_fd().try_clone_to_owned().unwrap());
        control.set_read_timeout(Some(LIMIT)).unwrap();
        writeln!(control, "{} inherited={inherited}", std::process::id()).unwrap();
        control.read_exact(&mut [0]).unwrap();
        return;
    }
    let raw = std::env::var("AAA_PTY_MODE").unwrap() == "raw";
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    WINDOW.set((entered_tx, std::sync::Mutex::new(release_rx))).unwrap();
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"aaa_pty_arm".as_ptr()) };
    assert!(!symbol.is_null(), "openpty interpose가 로드되어야 한다");
    let arm: unsafe extern "C" fn(extern "C" fn(i32, i32)) = unsafe { std::mem::transmute(symbol) };
    unsafe { arm(pty_window) };
    let opener = std::thread::spawn(move || {
        let open = || portable_pty::native_pty_system().openpty(portable_pty::PtySize::default());
        if raw { open() } else { with_spawn_lock(open) }.unwrap()
    });
    let slave = entered_rx.recv_timeout(LIMIT).expect("실제 openpty 창 진입");
    assert_eq!(unsafe { libc::fcntl(slave, libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
    let (mut control, child_io) = UnixStream::pair().unwrap();
    control.set_read_timeout(Some(LIMIT)).unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE, "--ignored"])
        .env_remove("LD_PRELOAD")
        .env("AAA_PTY_ROLE", "holder")
        .env("AAA_PTY_FD", slave.to_string())
        .stdin(Stdio::from(OwnedFd::from(child_io)))
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    if !raw {
        // 다른 spawn이 아직 실행되지 않은 우연을 성공으로 세지 않는다.
        assert!(SPAWN.try_lock().is_err(), "openpty의 CLOEXEC 전 창을 생성 잠금이 보호해야 한다");
    }
    let holder = std::thread::spawn(move || {
        if std::env::var("AAA_PTY_MODE").unwrap() == "tokio" {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async {
                let mut child = tokio::process::Command::from(command).kill_on_drop(true).spawn_guarded().unwrap();
                child.wait().await.unwrap()
            })
        } else {
            let mut child = OwnedChild(if raw { command.spawn() } else { command.spawn_guarded() }.unwrap());
            child.0.wait().unwrap()
        }
    });
    if !raw {
        release_tx.send(()).unwrap();
    }
    let mut line = String::new();
    BufReader::new(control.try_clone().unwrap()).read_line(&mut line).unwrap();
    let (pid, inherited) = line.trim().split_once(' ').unwrap();
    let pid: i32 = pid.parse().unwrap();
    assert_eq!(inherited, if raw { "inherited=true" } else { "inherited=false" });
    if raw {
        release_tx.send(()).unwrap();
    }
    let pair = opener.join().unwrap();
    let mut command = portable_pty::CommandBuilder::new("/bin/sh");
    command.env_remove("LD_PRELOAD");
    command.args(["-c", "printf pty-complete; exit 7"]);
    let mut terminal = with_spawn_lock(|| pair.slave.spawn_command(command)).unwrap();
    drop(pair.slave);
    assert_eq!(terminal.wait().unwrap().exit_code(), 7);
    let mut event = libc::pollfd { fd: pair.master.as_raw_fd().unwrap(), events: libc::POLLIN, revents: 0 };
    assert!(unsafe { libc::poll(&mut event, 1, 0) } >= 0);
    assert_eq!(event.revents & libc::POLLHUP != 0, !raw, "holder가 살아 있는 동안의 EOF 경계");
    assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "상속 대조 자식은 아직 살아 있어야 한다");
    let mut reader = pair.master.try_clone_reader().unwrap();
    if raw {
        control.write_all(b"x").unwrap();
        assert!(holder.join().unwrap().success());
    } else {
        // holder 해제 전에 EOF까지 읽는다. 수집을 조기 종료하거나 출력을 버리지 않는다.
        let mut text = String::new();
        reader.read_to_string(&mut text).unwrap();
        assert_eq!(text, "pty-complete");
        control.write_all(b"x").unwrap();
        assert!(holder.join().unwrap().success());
        println!("inherited=false child=exit7 eof=before-holder-exit");
        return;
    }
    let mut text = String::new();
    reader.read_to_string(&mut text).unwrap();
    assert_eq!(text, "pty-complete");
    println!("inherited=true child=exit7 eof=holder-exit");
}
