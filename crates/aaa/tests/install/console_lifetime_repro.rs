//! 같은 main thread·질문별 모드 복구·실제 제품 관측기 수명의 수동 ConPTY 진단.
use super::*;
use std::io::Write;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tracing_subscriber::prelude::*;
use windows_sys::Win32::System::Console::*;

// 별도 관측기 구현으로 바꾸지 않고 실패한 제품의 소스를 그대로 컴파일한다.
#[path = "../../src/install/prompt/windows_trace.rs"]
mod product_observer;
#[path = "console_lifetime_trace.rs"]
mod trace;

const NAME: &str = "authorization::terminal_snapshot::native::lifetime_repro::cooked_read_product_lifetimes";
const INPUTS: [&str; 14] = [
    "fresh",
    "host",
    "host-a",
    "disabled",
    "skip",
    ":back",
    ":back",
    ":back",
    ":back",
    "agent",
    "agent-final",
    "0",
    "require",
    "preserve",
];
const LABELS: [&str; 14] = [
    "설치 방식 [fresh / repair / reinstall]",
    "이 머신의 역할 [host / agent]",
    "머신 표시 이름",
    "host에 로컬 agent도 설치 [enabled / disabled]",
    "프로젝트 [create / use / skip]",
    "사용자 PATH [preserve / user]",
    "프로젝트 [create / use / skip]",
    "host에 로컬 agent도 설치 [enabled / disabled]",
    "머신 표시 이름",
    "이 머신의 역할 [host / agent]",
    "머신 표시 이름",
    "남겨 둘 여유 GB",
    "harness 로그인 검사 [require / defer]",
    "사용자 PATH [preserve / user]",
];

fn event() -> Arc<OwnedHandle> {
    Arc::new(owned(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) }, "lifetime_event").unwrap())
}
fn signal(event: &OwnedHandle) {
    assert_ne!(unsafe { SetEvent(event.as_raw_handle()) }, 0);
}
fn remaining(deadline: Instant) -> u32 {
    u32::try_from(deadline.saturating_duration_since(Instant::now()).as_millis()).unwrap_or(u32::MAX - 1)
}
fn wait(event: &OwnedHandle, deadline: Instant) {
    assert_eq!(unsafe { WaitForSingleObject(event.as_raw_handle(), remaining(deadline)) }, WAIT_OBJECT_0);
}
fn marker(id: u64) {
    println!("\r\nfixture_input_{id}:ready");
    std::io::stdout().flush().unwrap();
}
fn has_enter(input: HANDLE) -> bool {
    let mut records = [INPUT_RECORD::default(); 128];
    let mut count = 0;
    assert_ne!(unsafe { PeekConsoleInputW(input, records.as_mut_ptr(), records.len() as u32, &mut count) }, 0);
    records[..count as usize].iter().any(|record| {
        record.EventType == KEY_EVENT as u16
            && unsafe { record.Event.KeyEvent.bKeyDown != 0 && record.Event.KeyEvent.uChar.UnicodeChar == 13 }
    })
}
struct Round {
    id: u64,
    order: String,
    deadline: Instant,
    observer_ready: Arc<OwnedHandle>,
    observer_release: Arc<OwnedHandle>,
    peeked_enter: Arc<OwnedHandle>,
    read_call: Arc<OwnedHandle>,
    read_release: Arc<OwnedHandle>,
    returned: Arc<OwnedHandle>,
}
impl Round {
    fn new(id: u64, order: &str, deadline: Instant) -> Self {
        Self {
            id,
            order: order.into(),
            deadline,
            observer_ready: event(),
            observer_release: event(),
            peeked_enter: event(),
            read_call: event(),
            read_release: event(),
            returned: event(),
        }
    }
}
struct Mode {
    input: HANDLE,
    original: u32,
}
impl Drop for Mode {
    fn drop(&mut self) {
        assert_ne!(unsafe { SetConsoleMode(self.input, self.original) }, 0);
    }
}

fn control(input: usize, tid: u32, rounds: mpsc::Receiver<Arc<Round>>) {
    let input = input as HANDLE;
    let thread = owned(unsafe { OpenThread(THREAD_QUERY_INFORMATION, 0, tid) }, "lifetime_thread").unwrap();
    assert_eq!(unsafe { GetProcessIdOfThread(thread.as_raw_handle()) }, std::process::id());
    for round in rounds {
        match round.order.as_str() {
            "prompt" => {} // main thread가 질문을 flush한 뒤 marker를 출력한다.
            "prequeued" => {
                marker(round.id);
                loop {
                    wait_input(input, round.deadline);
                    if has_enter(input) {
                        break;
                    }
                    assert!(Instant::now() < round.deadline, "선입력 Enter 사건 상한");
                    unsafe { SwitchToThread() };
                }
                println!("lifetime_event read_id={} prequeued_enter=true", round.id);
                signal(&round.read_release);
            }
            "peek_before_read" => {
                wait(&round.observer_ready, round.deadline);
                marker(round.id);
                wait(&round.peeked_enter, round.deadline);
                println!("lifetime_event read_id={} observer_enter_before_read=true", round.id);
                signal(&round.read_release);
            }
            "read_pending" | "peek_after_pending" => {
                wait(&round.read_call, round.deadline);
                loop {
                    let mut pending = 0;
                    assert_ne!(unsafe { GetThreadIOPendingFlag(thread.as_raw_handle(), &mut pending) }, 0);
                    if pending != 0 {
                        break;
                    }
                    assert!(Instant::now() < round.deadline, "읽기 I/O pending 사건 상한");
                    unsafe { SwitchToThread() };
                }
                println!("lifetime_event read_id={} reader_io_pending=true", round.id);
                if round.order == "peek_after_pending" {
                    wait(&round.observer_ready, round.deadline);
                    signal(&round.observer_release);
                    println!("lifetime_event read_id={} observer_released_after_pending=true", round.id);
                }
                marker(round.id);
            }
            _ => unreachable!(),
        }
        if unsafe { WaitForSingleObject(round.returned.as_raw_handle(), remaining(round.deadline)) } != WAIT_OBJECT_0 {
            let mut pending = 0;
            let pending_ok = unsafe { GetThreadIOPendingFlag(thread.as_raw_handle(), &mut pending) };
            let mut mode = 0;
            let mode_ok = unsafe { GetConsoleMode(input, &mut mode) };
            let input_wait = unsafe { WaitForSingleObject(input, 0) };
            eprintln!(
                "lifetime_stalled read_id={} pending_ok={pending_ok} pending={pending} mode_ok={mode_ok} mode={mode} stdin_wait={input_wait} enter_prefix={}",
                round.id,
                has_enter(input)
            );
            panic!("읽기 반환의 기존 fixture 5초 안전 상한");
        }
    }
}
fn wait_input(input: HANDLE, deadline: Instant) {
    assert_eq!(unsafe { WaitForSingleObject(input, remaining(deadline)) }, WAIT_OBJECT_0);
}

fn fixture(order: &str, observer: bool, output: bool) {
    // ReadConsoleW 중인 main을 unwind하지 않고 자기 fixture만 끝낸다.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("lifetime_fixture_failure: {info}");
        std::process::exit(1);
    }));
    let current = Arc::new(Mutex::new(None));
    tracing_subscriber::registry()
        .with(trace::Hooks(current.clone()))
        .with(
            output.then(|| {
                tracing_subscriber::fmt::layer().with_target(false).with_ansi(true).with_writer(std::io::stderr)
            }),
        )
        .init();
    let thread_id = unsafe { GetCurrentThreadId() };
    let owner: u32 = std::env::var("AAA_TEST_CONSOLE_LIFETIME_OWNER").unwrap().parse().unwrap();
    let _client = verify(std::process::id(), owner, &std::env::current_exe().unwrap()).unwrap();
    let host_pid = console_host(unsafe { GetCurrentProcess() }).unwrap();
    let _host = verify(host_pid, owner, &console_host_image().unwrap()).unwrap();
    println!("lifetime_identity child_and_host_verified=true");
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    assert!(!input.is_null() && input != INVALID_HANDLE_VALUE);
    let (tx, rx) = mpsc::channel();
    let raw = input as usize;
    let controller = std::thread::spawn(move || control(raw, thread_id, rx));
    let deadline = Instant::now() + Duration::from_secs(5);
    for (index, expected) in INPUTS.iter().enumerate() {
        let id = index as u64 + 1;
        assert_eq!(unsafe { GetCurrentThreadId() }, thread_id);
        let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        assert_eq!(handle, input);
        let mut original = 0;
        assert_ne!(unsafe { GetConsoleMode(handle, &mut original) }, 0);
        assert_eq!(original, 503, "자연 실패의 원래 콘솔 모드와 대조한다");
        let mode_guard = Mode { input: handle, original };
        let mode = (original | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT)
            & !(ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
        assert_eq!(mode, 502);
        assert_ne!(unsafe { SetConsoleMode(handle, mode) }, 0);
        let control = CONSOLE_READCONSOLE_CONTROL {
            nLength: std::mem::size_of::<CONSOLE_READCONSOLE_CONTROL>() as u32,
            nInitialChars: 0,
            dwCtrlWakeupMask: (1 << 3) | (1 << 26),
            dwControlKeyState: 0,
        };
        let mut value = vec![0u16; 16384];
        let mut count = 0;
        let round = Arc::new(Round::new(id, order, deadline));
        *current.lock().unwrap() = Some(round.clone());
        let observation = product_observer::Observation::start(handle, id);
        assert_eq!(observation.is_some(), observer);
        tracing::debug!(
            read_id = id,
            read_thread = thread_id,
            original_mode = original,
            input_mode = mode,
            "install_console_input_prepared"
        );
        print!("{} (출처: 새 설치 기본값/감지 후보, 기본값: fixture): ", LABELS[index]);
        std::io::stdout().flush().unwrap();
        tx.send(round.clone()).unwrap();
        if order == "prompt" {
            marker(id);
        }
        tracing::debug!(
            read_id = id,
            read_thread = thread_id,
            "install_console_read_call thread_id={thread_id} [console-read-call]"
        );
        if matches!(order, "prequeued" | "peek_before_read") {
            wait(&round.read_release, deadline);
        }
        signal(&round.read_call);
        let started = Instant::now();
        let ok = unsafe { ReadConsoleW(handle, value.as_mut_ptr().cast(), value.len() as u32, &mut count, &control) };
        let os_code = if ok == 0 { unsafe { GetLastError() } } else { 0 };
        tracing::debug!(
            read_id = id,
            read_thread = thread_id,
            success = ok != 0,
            os_code,
            elapsed_ms = started.elapsed().as_millis(),
            "install_console_read_returned"
        );
        signal(&round.returned);
        drop(observation);
        assert_ne!(ok, 0, "ReadConsoleW os_code={os_code}");
        value.truncate(count as usize);
        let expected: Vec<u16> = expected.encode_utf16().chain([13]).collect();
        assert_eq!(value, expected, "정한 fixture 응답을 정확히 반환한다");
        drop(mode_guard);
        let mut restored = 0;
        assert_ne!(unsafe { GetConsoleMode(handle, &mut restored) }, 0);
        assert_eq!(restored, original);
        println!("lifetime_event read_id={id} read_returned=true mode_restored=true thread_same=true");
    }
    drop(tx);
    controller.join().unwrap();
    println!("lifetime_complete reads=14 mode_restores=14 reader_threads=1");
}

#[test]
#[ignore = "수동 진단: 같은 Server 2025 이미지에서 각 행을 한 번만 실행한다"]
fn cooked_read_product_lifetimes() {
    if let Ok(order) = std::env::var("AAA_TEST_CONSOLE_LIFETIME_ORDER") {
        assert!(
            ["prompt", "prequeued", "read_pending", "peek_before_read", "peek_after_pending"].contains(&order.as_str())
        );
        fixture(
            &order,
            std::env::var_os("AAA_TEST_CONSOLE_LIFETIME_OBSERVER").is_some(),
            std::env::var_os("AAA_TEST_CONSOLE_LIFETIME_OUTPUT").is_some(),
        );
        return;
    }
    let version = file_version(&console_host_image().unwrap()).unwrap();
    let (build, product_type) = super::repro::os_version();
    assert_eq!(
        (build, product_type, version),
        (26100, 3, [10, 0, 26100, 33648]),
        "같은 Server 2025·conhost 판에서만 실행한다"
    );
    let conhost = std::fs::read(console_host_image().unwrap()).unwrap();
    use sha2::Digest;
    eprintln!(
        "lifetime_environment os_build={build} product_type={product_type} conhost_version={version:?} conhost_disk_sha256={}",
        hex::encode(sha2::Sha256::digest(&conhost))
    );
    let mut failures = Vec::new();
    let mut rows = 0;
    for order in ["prompt", "prequeued", "read_pending", "peek_before_read", "peek_after_pending"] {
        for observer in [false, true] {
            if !observer && order.starts_with("peek_") {
                continue; // Peek를 제어하는 두 행은 관측기가 있는 조건이다.
            }
            for output in [false, true] {
                rows += 1;
                let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
                command.args(["--exact", NAME, "--ignored", "--nocapture"]);
                command.env("AAA_TEST_CONSOLE_LIFETIME_ORDER", order);
                command.env("AAA_TEST_CONSOLE_LIFETIME_OWNER", std::process::id().to_string());
                for name in [
                    "AAA_TEST_CONSOLE_LIFETIME_OBSERVER",
                    "AAA_TEST_CONSOLE_LIFETIME_OUTPUT",
                    "AAA_TEST_INSTALL_CONSOLE_TRACE",
                ] {
                    command.env_remove(name);
                }
                if observer {
                    command.env("AAA_TEST_CONSOLE_LIFETIME_OBSERVER", "1");
                    command.env("AAA_TEST_INSTALL_CONSOLE_TRACE", "1");
                }
                if output {
                    command.env("AAA_TEST_CONSOLE_LIFETIME_OUTPUT", "1");
                }
                let markers: Vec<_> = (1..=14).map(|id| format!("fixture_input_{id}:ready")).collect();
                let answers: Vec<_> = INPUTS.iter().map(|value| format!("{value}\r")).collect();
                // 숫자 뒤 구분자로 1이 10·11·12와 겹치지 않게 한다.
                let steps: Vec<_> =
                    markers.iter().zip(&answers).map(|(marker, answer)| (marker.as_str(), answer.as_str())).collect();
                let result = std::panic::catch_unwind(|| {
                    super::super::super::drive_terminal_as(
                        command,
                        &steps,
                        |_, _| {},
                        &std::env::current_exe().unwrap(),
                    )
                });
                let passed = match result {
                    Ok((code, text)) => {
                        let mut terminal = aaa_vt::Vt::new(36, 120);
                        terminal.feed(text.as_bytes());
                        let screen = terminal.unwrapped_all();
                        // 성공 행도 read별 사건을 남긴다. fixture의 숫자·불리언 사건 줄만 내보낸다.
                        for line in screen.lines().filter(|line| line.starts_with("lifetime_event ")) {
                            assert!(line.chars().all(|c| c.is_ascii_alphanumeric() || " _=".contains(c)));
                            eprintln!("lifetime_row_event order={order} observer={observer} output={output} {line}");
                        }
                        let completed = (1..=14).all(|id| {
                            screen.contains(&format!(
                                "lifetime_event read_id={id} read_returned=true mode_restored=true thread_same=true"
                            ))
                        });
                        let passed = code == 0
                            && completed
                            && screen.contains("lifetime_complete reads=14 mode_restores=14 reader_threads=1")
                            && super::super::super::terminal_fixture_succeeded(&text);
                        if !passed {
                            eprintln!(
                                "lifetime_row_failure order={order} observer={observer} output={output} exit={code}: {text}"
                            );
                        }
                        passed
                    }
                    Err(_) => false, // driver가 이미 실패 증거를 남겼다. 같은 행을 재실행하지 않는다.
                };
                eprintln!("lifetime_row order={order} observer={observer} output={output} passed={passed}");
                if !passed {
                    failures.push((order, observer, output));
                }
            }
        }
    }
    assert_eq!(rows, 16);
    assert!(failures.is_empty(), "행렬의 실패를 통과로 바꾸지 않는다: {failures:?}");
}
