//! 실제 std pipe의 비원자적 창을 dyld interpose로 제어한다. 운영 실행 파일에는 hook이 없다.
use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, OwnedFd},
        unix::net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    sync::{OnceLock, mpsc},
    time::Duration,
};
use tokio::io::AsyncReadExt as _;

const LIMIT: Duration = Duration::from_secs(2);
const WORKER_LIMIT: Duration = Duration::from_secs(5);
const FIXTURE: &str = "child_process::tests::subprocess_fixture";
static WINDOW: OnceLock<(mpsc::Sender<i32>, std::sync::Mutex<mpsc::Receiver<()>>)> = OnceLock::new();

extern "C" fn pipe_window(_read: i32, write: i32) {
    let (entered, release) = WINDOW.get().unwrap();
    trace(&format!("pipe 창 진입 fd={write}"));
    entered.send(write).unwrap();
    // callback의 panic은 자식 실험을 즉시 실패시킨다. 고정 sleep이나 재시도는 없다.
    release.lock().unwrap().recv_timeout(LIMIT).expect("pipe 창 해제");
    trace("pipe 창 해제됨");
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
    // SIP가 켜진 Mac에서도 보호된 /bin/sh가 DYLD 변수를 지워 결함을 숨기지 않게 한다.
    // 자기 실험실 사본만 ad-hoc 서명하며 시스템 실행 파일·설정은 건드리지 않는다.
    let shell = root.0.join("sh");
    // 병렬 spawn이 쓰는 중인 실행 파일 FD를 상속하지 않도록 복사도 자식이 한다.
    let copied = Command::new("/bin/cp").arg("/bin/sh").arg(&shell).capture_output().unwrap();
    assert!(copied.status.success(), "cp: {}", String::from_utf8_lossy(&copied.stderr));
    let signed = Command::new("codesign").args(["--force", "--sign", "-"]).arg(&shell).capture_output().unwrap();
    assert!(signed.status.success(), "codesign: {}", String::from_utf8_lossy(&signed.stderr));
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
        let trace_path = root.0.join(format!("{mode}.trace"));
        let mut command = tokio::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", FIXTURE, "--ignored", "--nocapture"])
            .env("AAA_PIPE_ROLE", "worker")
            .env("AAA_PIPE_MODE", mode)
            .env("AAA_PIPE_TRACE", &trace_path)
            .env("AAA_PIPE_SHELL", &shell)
            .env("DYLD_INSERT_LIBRARIES", &dylib)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn_guarded().unwrap();
        let pid = child.id();
        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (mut status, mut stdout_done, mut stderr_done) = (None, false, false);
        let result = tokio::time::timeout(WORKER_LIMIT, async {
            tokio::try_join!(
                async {
                    status = Some(child.wait().await?);
                    Ok::<_, std::io::Error>(())
                },
                async {
                    stdout.read_to_end(&mut out).await?;
                    stdout_done = true;
                    Ok(())
                },
                async {
                    stderr.read_to_end(&mut err).await?;
                    stderr_done = true;
                    Ok(())
                },
            )
        })
        .await;
        // 정리 오류가 최초 대기/읽기 실패를 가리지 않게 같은 진단에 남긴다.
        let cleanup = if !matches!(result, Ok(Ok(_))) && status.is_none() { Some(child.kill().await) } else { None };
        let diagnostic =
            std::fs::read_to_string(&trace_path).unwrap_or_else(|error| format!("trace 읽기 실패: {error}"));
        let progress = format!(
            "mode={mode} worker={pid:?} exit={status:?} stdout_eof={stdout_done} stderr_eof={stderr_done} cleanup={cleanup:?}"
        );
        eprintln!("{progress} trace={mode}.trace\n{diagnostic}");
        result
            .unwrap_or_else(|error| panic!("{progress} 종료/출력 대기: {error}\n{diagnostic}"))
            .unwrap_or_else(|error| panic!("{progress} 출력 수집: {error}\n{diagnostic}"));
        let out = Output { status: status.unwrap(), stdout: out, stderr: err };
        assert!(
            out.status.success(),
            "{mode}: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        eprintln!("{mode}: {}", String::from_utf8_lossy(&out.stdout));
    }
}

// argv·환경·인증은 기록하지 않는다. 바깥 output 대기가 실패해도 단계 기록은 남는다.
fn trace(stage: &str) {
    let path = std::env::var_os("AAA_PIPE_TRACE").expect("격리 진단 파일");
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
    file.write_all(format!("pid={} {stage}\n", std::process::id()).as_bytes()).unwrap();
}

fn fixture_command(role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE, "--ignored", "--nocapture", "--test-threads=1"])
        .env("AAA_PIPE_ROLE", role)
        .env_remove("DYLD_INSERT_LIBRARIES");
    command
}

fn capture_command(socket: &std::path::Path) -> Command {
    let mut command = Command::new(std::env::var_os("AAA_PIPE_SHELL").expect("실험실 shell 사본"));
    command
        .args(["-c", "exec \"$AAA_PIPE_EXE\" --exact child_process::tests::subprocess_fixture --ignored --nocapture"])
        .env("AAA_PIPE_EXE", std::env::current_exe().unwrap())
        .env("AAA_PIPE_ROLE", "capture")
        .env("AAA_PIPE_SOCKET", socket)
        // hook은 worker의 pipe 창에만 필요하다. arm64e shell에 arm64 dylib를 물려주면
        // SIP가 DYLD를 지우지 않는 환경에서 fixture 시작 전에 dyld가 SIGABRT를 낸다.
        .env_remove("DYLD_INSERT_LIBRARIES");
    command
}

// NOTE_EXIT를 먼저 구독한 뒤 자식을 풀어, output 미완료를 실제 자식 종료 뒤에 검사한다.
fn wait_capture_exit(stream: &mut UnixStream) {
    let mut line = String::new();
    BufReader::new(stream.try_clone().unwrap()).read_line(&mut line).unwrap();
    let pid: usize = line.trim().parse().unwrap();
    trace(&format!("capture pid={pid} NOTE_EXIT 구독"));
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
    trace(&format!("capture pid={pid} NOTE_EXIT 수신"));
}

#[test]
#[ignore = "부모 시험이 격리 환경과 dylib를 준비해 실행한다"]
fn subprocess_fixture() {
    trace(&format!("역할 시작 {}", std::env::var("AAA_PIPE_ROLE").unwrap()));
    match std::env::var("AAA_PIPE_ROLE").unwrap().as_str() {
        "capture" => {
            let mut socket = UnixStream::connect(std::env::var_os("AAA_PIPE_SOCKET").unwrap()).unwrap();
            socket.set_read_timeout(Some(LIMIT)).unwrap();
            writeln!(socket, "{}", std::process::id()).unwrap();
            trace("capture 해제 대기");
            socket.read_exact(&mut [0]).unwrap();
            trace("capture 출력 뒤 exit=7");
            std::io::stdout().write_all(b"capture-out\n").unwrap();
            std::io::stderr().write_all(b"capture-err\n").unwrap();
            std::process::exit(7);
        }
        "holder" => {
            let fd: i32 = std::env::var("AAA_PIPE_FD").unwrap().parse().unwrap();
            let inherited = unsafe { libc::fcntl(fd, libc::F_GETFD) } >= 0;
            // stdin은 부모가 넘긴 양방향 소켓이다. libtest의 stdout 접두사와 신호를 섞지 않는다.
            let mut control = UnixStream::from(std::io::stdin().as_fd().try_clone_to_owned().unwrap());
            control.set_read_timeout(Some(LIMIT)).unwrap();
            writeln!(control, "INHERITED={inherited}").unwrap();
            trace(&format!("holder inherited={inherited} 입력 대기"));
            control.read_exact(&mut [0]).unwrap();
            trace("holder exit=0");
            std::process::exit(0);
        }
        "worker" => run_worker(),
        role => panic!("알 수 없는 역할 {role}"),
    }
}

fn run_worker() {
    let mode = std::env::var("AAA_PIPE_MODE").unwrap();
    trace(&format!("worker mode={mode}"));
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
    trace("pipe hook 준비");
    unsafe { arm(pipe_window) };
    let (done_tx, done_rx) = mpsc::channel();
    let capture_mode = mode.clone();
    let capture = std::thread::spawn(move || {
        let mut command = capture_command(&socket_path);
        let text = match capture_mode.as_str() {
            "raw" | "std" => {
                let output = if capture_mode == "raw" { command.output() } else { command.capture_output() }.unwrap();
                trace(&format!(
                    "capture 반환 status={} stdout={:?} stderr={:?}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr),
                ));
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
        trace("capture 출력 수집 완료");
        done_tx.send(text).unwrap();
    });
    let fd = entered_rx.recv_timeout(LIMIT).expect("capture의 pipe 창 진입");
    trace(&format!("worker pipe 창 확인 fd={fd}"));
    assert_eq!(unsafe { libc::fcntl(fd, libc::F_GETFD) } & libc::FD_CLOEXEC, 0);
    let (mut control, child_io) = UnixStream::pair().unwrap();
    control.set_read_timeout(Some(LIMIT)).unwrap();
    let mut command = fixture_command("holder");
    command
        .env("AAA_PIPE_FD", fd.to_string())
        .stdin(Stdio::from(OwnedFd::from(child_io)))
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let holder = if raw {
        // 수정 전: 실제 pipe 창 안에서 오래 사는 자식을 exec시켜 누출을 확정한다.
        let child = OwnedChild(command.spawn().unwrap());
        trace(&format!("raw holder spawn pid={}", child.0.id()));
        let mut line = String::new();
        let mut reader = BufReader::new(control.try_clone().unwrap());
        assert_ne!(reader.read_line(&mut line).expect("holder 준비 신호"), 0);
        assert_eq!(line.trim(), "INHERITED=true");
        release_tx.send(()).unwrap();
        child
    } else {
        // 같은 실제 창을 잡은 동안 생성 배제가 이미 적용됐는지 확인한 뒤 해제한다.
        assert!(SPAWN.try_lock().is_err(), "pipe 생성부터 exec까지 같은 잠금이어야 한다");
        let holder = std::thread::spawn(move || OwnedChild(command.spawn_guarded().unwrap()));
        release_tx.send(()).unwrap();
        let child = holder.join().unwrap();
        trace(&format!("guarded holder spawn pid={}", child.0.id()));
        let mut line = String::new();
        let mut reader = BufReader::new(control.try_clone().unwrap());
        assert_ne!(reader.read_line(&mut line).expect("holder 준비 신호"), 0);
        assert_eq!(line.trim(), "INHERITED=false");
        child
    };
    let mut holder = holder;
    let (connected_tx, connected_rx) = mpsc::channel();
    let accept = std::thread::spawn(move || connected_tx.send(listener.accept()).unwrap());
    trace("capture socket 연결 대기");
    let (mut stream, _) = connected_rx.recv_timeout(LIMIT).expect("capture socket 연결").unwrap();
    trace("capture socket 연결됨");
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
    trace("worker 완료");
}
