//! 실제 std pipe의 비원자적 창을 dyld interpose로 제어한다. 운영 실행 파일에는 hook이 없다.
use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{OnceLock, mpsc},
    time::Duration,
};

const LIMIT: Duration = Duration::from_secs(30);
const FIXTURE: &str = "child_process::tests::subprocess_fixture";
static WINDOW: OnceLock<(mpsc::Sender<i32>, std::sync::Mutex<mpsc::Receiver<()>>)> = OnceLock::new();

extern "C" fn pipe_window(_read: i32, write: i32) {
    let (entered, release) = WINDOW.get().unwrap();
    entered.send(write).unwrap();
    // callback의 panic은 자식 실험을 즉시 실패시킨다. 고정 sleep이나 재시도는 없다.
    release.lock().unwrap().recv_timeout(LIMIT).unwrap();
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

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[tokio::test]
async fn pipe_window_cannot_leak_to_another_child_or_serialize_its_lifetime() {
    // dylib는 자기 worktree target에만 둔다. socket은 macOS UDS 길이 제한 안에 둔다.
    let root = Root(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(format!("pipe-window-{}", crate::util::new_id())),
    );
    std::fs::create_dir_all(&root.0).unwrap();
    let dylib = root.0.join("window.dylib");
    let built = Command::new("cc")
        .args(["-dynamiclib", "-std=c11"])
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/child_process/pipe_window.c"))
        .arg("-o")
        .arg(&dylib)
        .capture_output()
        .unwrap();
    assert!(built.status.success(), "cc: {}", String::from_utf8_lossy(&built.stderr));
    for mode in ["raw", "std", "tokio", "observe"] {
        let out = tokio::time::timeout(
            LIMIT,
            tokio::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", FIXTURE, "--ignored", "--nocapture"])
                .env("AAA_PIPE_ROLE", "worker")
                .env("AAA_PIPE_MODE", mode)
                .env("DYLD_INSERT_LIBRARIES", &dylib)
                .kill_on_drop(true)
                .output_guarded(),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(
            out.status.success(),
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        eprintln!("{mode}: {}", String::from_utf8_lossy(&out.stdout));
    }
}

fn fixture_command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", FIXTURE, "--ignored", "--nocapture"]).env("AAA_PIPE_ROLE", role);
    command
}

fn capture_command(socket: &std::path::Path) -> Command {
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", "exec \"$AAA_PIPE_EXE\" --exact child_process::tests::subprocess_fixture --ignored --nocapture"])
        .env("AAA_PIPE_EXE", std::env::current_exe().unwrap())
        .env("AAA_PIPE_ROLE", "capture")
        .env("AAA_PIPE_SOCKET", socket);
    command
}

// NOTE_EXIT를 먼저 구독한 뒤 자식을 풀어, output 미완료를 실제 자식 종료 뒤에 검사한다.
fn wait_capture_exit(stream: &mut UnixStream) {
    let mut line = String::new();
    BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
    let pid: usize = line.trim().parse().unwrap();
    let fd = unsafe { libc::kqueue() };
    assert!(fd >= 0);
    let queue = unsafe { OwnedFd::from_raw_fd(fd) };
    let event = libc::kevent {
        ident: pid,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    assert_eq!(unsafe { libc::kevent(queue.as_raw_fd(), &event, 1, std::ptr::null_mut(), 0, std::ptr::null()) }, 0);
    stream.write_all(b"x").unwrap();
    let mut result = event;
    let timeout = libc::timespec { tv_sec: LIMIT.as_secs() as _, tv_nsec: 0 };
    assert_eq!(unsafe { libc::kevent(queue.as_raw_fd(), std::ptr::null(), 0, &mut result, 1, &timeout) }, 1);
    assert_eq!(result.flags & libc::EV_ERROR, 0);
    assert_ne!(result.fflags & libc::NOTE_EXIT, 0);
}

#[test]
#[ignore = "부모 시험이 격리 환경과 dylib를 준비해 실행한다"]
fn subprocess_fixture() {
    match std::env::var("AAA_PIPE_ROLE").unwrap().as_str() {
        "capture" => {
            let mut socket = UnixStream::connect(std::env::var_os("AAA_PIPE_SOCKET").unwrap()).unwrap();
            socket.set_read_timeout(Some(LIMIT)).unwrap();
            writeln!(socket, "{}", std::process::id()).unwrap();
            socket.read_exact(&mut [0]).unwrap();
            std::io::stdout().write_all(b"capture-out\n").unwrap();
            std::io::stderr().write_all(b"capture-err\n").unwrap();
            std::process::exit(7);
        }
        "holder" => {
            let fd: i32 = std::env::var("AAA_PIPE_FD").unwrap().parse().unwrap();
            let inherited = unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0;
            println!("INHERITED={inherited}");
            std::io::stdout().flush().unwrap();
            std::io::stdin().read_exact(&mut [0]).unwrap();
            std::process::exit(0);
        }
        "worker" => run_worker(),
        role => panic!("알 수 없는 역할 {role}"),
    }
}

fn run_worker() {
    let mode = std::env::var("AAA_PIPE_MODE").unwrap();
    let raw = mode == "raw";
    let root = Root(PathBuf::from("/tmp").join(format!("aaa-pipe-{}", crate::util::new_id())));
    std::fs::create_dir(&root.0).unwrap();
    let socket_path = root.0.join("s");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    WINDOW.set((entered_tx, std::sync::Mutex::new(release_rx))).unwrap();
    // reactor 초기화가 아닌, 다음 Command의 stdout pipe만 붙잡는다.
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"aaa_pipe_arm".as_ptr()) };
    assert!(!symbol.is_null(), "실험실 dylib가 로드되어야 한다");
    let arm: unsafe extern "C" fn(extern "C" fn(i32, i32)) = unsafe { std::mem::transmute(symbol) };
    unsafe { arm(pipe_window) };
    let (done_tx, done_rx) = mpsc::channel();
    let capture_mode = mode.clone();
    let capture = std::thread::spawn(move || {
        let mut command = capture_command(&socket_path);
        let text = match capture_mode.as_str() {
            "raw" | "std" => {
                let output = if capture_mode == "raw" { command.output() } else { command.capture_output() }.unwrap();
                assert_eq!(output.status.code(), Some(7));
                assert!(output.stdout.ends_with(b"capture-out\n"));
                assert_eq!(output.stderr, b"capture-err\n");
                "출력·종료 코드 보존".to_string()
            }
            "tokio" => runtime.block_on(async {
                let output = tokio::process::Command::from(command).output_guarded().await.unwrap();
                assert_eq!(output.status.code(), Some(7));
                assert!(output.stdout.ends_with(b"capture-out\n"));
                assert_eq!(output.stderr, b"capture-err\n");
                "비동기 출력·종료 코드 보존".to_string()
            }),
            "observe" => runtime.block_on(async {
                let record = crate::harness_version::observe(tokio::process::Command::from(command), "fixture").await;
                assert!(
                    record.contains("capture-out") && record.contains("capture-err") && record.contains("7"),
                    "{record}"
                );
                record
            }),
            _ => unreachable!(),
        };
        done_tx.send(text).unwrap();
    });
    let fd = entered_rx.recv_timeout(LIMIT).unwrap();
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
    let (mut control, child_io) = UnixStream::pair().unwrap();
    control.set_read_timeout(Some(LIMIT)).unwrap();
    let mut command = fixture_command("holder");
    command
        .env("AAA_PIPE_FD", fd.to_string())
        .stdin(Stdio::from(OwnedFd::from(child_io.try_clone().unwrap())))
        .stdout(Stdio::from(OwnedFd::from(child_io)))
        .stderr(Stdio::null());
    let holder = if raw {
        // 수정 전: 실제 pipe 창 안에서 오래 사는 자식을 exec시켜 누출을 확정한다.
        let child = OwnedChild(command.spawn().unwrap());
        let mut line = String::new();
        let mut reader = BufReader::new(control.try_clone().unwrap());
        while !line.starts_with("INHERITED=") {
            line.clear();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
        }
        assert_eq!(line.trim(), "INHERITED=true");
        release_tx.send(()).unwrap();
        child
    } else {
        // 같은 실제 창을 잡은 동안 생성 배제가 이미 적용됐는지 확인한 뒤 해제한다.
        assert!(SPAWN.try_lock().is_err(), "pipe 생성부터 exec까지 같은 잠금이어야 한다");
        let holder = std::thread::spawn(move || OwnedChild(command.spawn_guarded().unwrap()));
        release_tx.send(()).unwrap();
        let child = holder.join().unwrap();
        let mut line = String::new();
        let mut reader = BufReader::new(control.try_clone().unwrap());
        while !line.starts_with("INHERITED=") {
            line.clear();
            assert_ne!(reader.read_line(&mut line).unwrap(), 0);
        }
        assert_eq!(line.trim(), "INHERITED=false");
        child
    };
    let mut holder = holder;
    let (connected_tx, connected_rx) = mpsc::channel();
    let accept = std::thread::spawn(move || connected_tx.send(listener.accept()).unwrap());
    let (mut stream, _) = connected_rx.recv_timeout(LIMIT).unwrap().unwrap();
    accept.join().unwrap();
    stream.set_read_timeout(Some(LIMIT)).unwrap();
    wait_capture_exit(&mut stream);
    assert!(holder.0.try_wait().unwrap().is_none(), "무관한 자식은 아직 입력 대기 중");
    if raw {
        assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)), "종료한 명령의 output이 EOF를 못 받는다");
        println!("수정 전: FD 상속 확인, 캡처 자식 종료 뒤에도 output 미완료");
        control.write_all(b"x").unwrap();
        assert!(holder.0.wait().unwrap().success());
        println!("holder 종료 뒤에만 {}", done_rx.recv_timeout(LIMIT).unwrap());
    } else {
        println!("수정 후: FD 상속 없음, holder 생존 중 {}", done_rx.recv_timeout(LIMIT).unwrap());
        assert!(holder.0.try_wait().unwrap().is_none(), "출력 수집은 무관한 자식 종료를 기다리지 않는다");
        control.write_all(b"x").unwrap();
        assert!(holder.0.wait().unwrap().success());
    }
    capture.join().unwrap();
}
