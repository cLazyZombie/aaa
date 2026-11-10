use super::read_state_probe::{Api, Cause, Outcome, Phase, Probe};
use super::read_state_product as product_state;
use super::*;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::sync::Arc;
use windows_sys::Win32::{
    Foundation::*,
    System::{Console::*, Threading::*},
};

fn run(
    mut command: CommandBuilder,
    steps: &[(&str, &str)],
    image: &Path,
    mut observe: impl FnMut(&mut dyn portable_pty::Child, &str, &Probe),
) -> (u32, String) {
    let probe = Probe::new();
    probe.configure(&mut command);
    let pair = native_pty_system().openpty(PtySize { rows: 36, cols: 120, pixel_width: 0, pixel_height: 0 }).unwrap();
    drive_terminal_inner(pair, command, steps, |child, text| observe(child, text, &probe), image, &probe)
}
fn take(probe: &Probe, pid: u32, phase: Phase, wait: u32, pending: bool) -> String {
    let reply = probe
        .capture(pid, Cause::ControlledFixture, read_state_probe::unix_us(), Instant::now() + Duration::from_secs(5))
        .unwrap();
    let encoded = serde_json::to_string(&reply).unwrap();
    eprintln!("Controlled original stdin read state: {encoded}");
    let Outcome::OriginalStdin { sample } = &reply.outcome else {
        panic!("활성 read 표본이 필요하다: {reply:?}");
    };
    assert_eq!((sample.phase_before, sample.phase_after), (phase, phase));
    assert!(matches!(sample.original_stdin_wait, Api::Ok { value } if value == wait));
    assert!(matches!(sample.reader_io_pending, Api::Ok { value } if value == pending));
    encoded
}

struct CallEvent {
    name: String,
    handle: OwnedHandle,
}
impl CallEvent {
    fn new() -> Self {
        let name = format!("Local\\aaa-test-read-call-{}", aaa_common::util::new_id());
        let wide: Vec<_> = name.encode_utf16().chain(Some(0)).collect();
        let handle = unsafe { CreateEventW(std::ptr::null(), 1, 0, wide.as_ptr()) };
        assert!(!handle.is_null());
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        assert_ne!(unsafe { GetLastError() }, ERROR_ALREADY_EXISTS);
        Self { name, handle }
    }
    fn wait(&self, deadline: Instant) {
        assert_eq!(
            unsafe {
                WaitForSingleObject(
                    self.handle.as_raw_handle(),
                    deadline.saturating_duration_since(Instant::now()).as_millis() as u32,
                )
            },
            WAIT_OBJECT_0,
            "읽기 직전 로그 I/O 반환 사건"
        );
    }
}

#[test]
fn original_stdin_probe_observes_the_owned_cli_read_and_preserves_real_approval() {
    let lab = Lab::new();
    let approval = lab.0.join("read-state-approval.json");
    let args = authorize_args(&lab, &approval, true);
    let inherited = lab.command(&[]);
    let mut command = CommandBuilder::new(BIN);
    command.env_clear();
    for (name, value) in inherited.get_envs() {
        if let Some(value) = value {
            command.env(name, value);
        }
    }
    command.env("AAA_LOG", "aaa::install::prompt=debug");
    command.env("AAA_TEST_INSTALL_CONSOLE_TRACE", "1");
    let call_event = CallEvent::new();
    command.env("AAA_TEST_INSTALL_READ_CALL_EVENT", &call_event.name);
    command.args(&args);
    let mut captured = false;
    let (code, text) = run(command, &[("[console-read-call]", "yes\r")], Path::new(BIN), |child, text, probe| {
        if captured || !text.contains("[console-read-call]") {
            return;
        }
        let tid: u32 = text
            .rsplit("install_console_read_call thread_id=")
            .next()
            .unwrap()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap();
        let thread = unsafe { OpenThread(THREAD_QUERY_INFORMATION, 0, tid) };
        assert!(!thread.is_null());
        let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
        assert_eq!(unsafe { GetProcessIdOfThread(thread.as_raw_handle()) }, child.process_id().unwrap());
        let deadline = Instant::now() + Duration::from_secs(5);
        // thread의 pending에는 로그 출력도 포함된다. 그 출력이 끝난 뒤의 pending만 read로 판정한다.
        call_event.wait(deadline);
        loop {
            let mut pending = 0;
            assert_ne!(unsafe { GetThreadIOPendingFlag(thread.as_raw_handle(), &mut pending) }, 0);
            if pending != 0 {
                break;
            }
            assert!(Instant::now() < deadline, "소유 CLI read의 I/O pending 사건 상한");
            unsafe { SwitchToThread() };
        }
        let encoded = take(probe, child.process_id().unwrap(), Phase::Call, WAIT_TIMEOUT, true);
        let value: Value = serde_json::from_str(&encoded).unwrap();
        assert_eq!(value["outcome"]["sample"]["read_id"], 1);
        assert_eq!(value["outcome"]["sample"]["read_thread"], tid);
        assert!(!encoded.contains(CANARY) && !encoded.contains(&lab.0.to_string_lossy().to_string()));
        captured = true;
    });
    assert_eq!(code, 0, "{}", redact_private(&text));
    assert!(captured && approval.is_file());
    let issued: Value =
        serde_json::from_slice(&aaa_common::private_file::read(&approval, 128 * 1024).unwrap()).unwrap();
    assert!(text.contains(issued["claims"]["plan_sha256"].as_str().unwrap()));
}

const FIXTURE: &str =
    "authorization::read_state_tests::original_stdin_probe_does_not_consume_prequeued_input_or_change_mode";

#[test]
fn original_stdin_probe_does_not_consume_prequeued_input_or_change_mode() {
    if let Ok(case) = std::env::var("AAA_TEST_READ_STATE_FIXTURE") {
        fixture(&case);
        return;
    }
    for case in ["queued", "pending"] {
        let call_event = CallEvent::new();
        let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
        command.args(["--exact", FIXTURE, "--nocapture"]);
        command.env("AAA_TEST_READ_STATE_FIXTURE", case);
        command.env("AAA_TEST_INSTALL_CONSOLE_TRACE", "1");
        command.env("AAA_TEST_INSTALL_READ_CALL_EVENT", &call_event.name);
        let steps = [(if case == "queued" { "[queue-input]" } else { "[state-ready]" }, "fixture-value\r")];
        let mut captured = false;
        let (code, text) = run(command, &steps, &std::env::current_exe().unwrap(), |child, text, probe| {
            if captured || !text.contains("[state-ready]") {
                return;
            }
            if case == "queued" {
                assert_eq!(unsafe { WaitForSingleObject(call_event.handle.as_raw_handle(), 0) }, WAIT_TIMEOUT);
            } else {
                call_event.wait(Instant::now() + Duration::from_secs(5));
            }
            let encoded = take(
                probe,
                child.process_id().unwrap(),
                if case == "queued" { Phase::Prepared } else { Phase::Call },
                if case == "queued" { WAIT_OBJECT_0 } else { WAIT_TIMEOUT },
                case == "pending",
            );
            let value: Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(value["outcome"]["sample"]["read_id"], 41);
            assert!(!encoded.contains("fixture-value") && !encoded.contains("conin_wait"));
            captured = true;
        });
        assert_eq!(code, 0, "case={case}: {}", redact_private(&text));
        assert!(captured && text.contains("read_state_fixture_complete"));
        assert!(terminal_fixture_succeeded(&text));
        assert_eq!(unsafe { WaitForSingleObject(call_event.handle.as_raw_handle(), 0) }, WAIT_OBJECT_0);
    }
}

struct Replied(Arc<OwnedHandle>);
impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Replied {
    fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
        struct Message(bool);
        impl tracing::field::Visit for Message {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" && format!("{value:?}") == "install_console_read_state_replied" {
                    self.0 = true;
                }
            }
        }
        let mut message = Message(false);
        event.record(&mut message);
        if message.0 {
            assert_ne!(unsafe { SetEvent(self.0.as_raw_handle()) }, 0);
        }
    }
}
fn fixture(case: &str) {
    use tracing_subscriber::prelude::*;
    std::panic::set_hook(Box::new(|info| {
        eprintln!("read-state fixture failure: {info}");
        std::process::exit(1);
    }));
    let deadline = Instant::now() + Duration::from_secs(5);
    let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    assert!(!event.is_null());
    let replied = Arc::new(unsafe { OwnedHandle::from_raw_handle(event) });
    tracing_subscriber::registry().with(Replied(replied.clone())).init();
    let input = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut original = 0;
    assert_ne!(unsafe { GetConsoleMode(input, &mut original) }, 0);
    let mode =
        (original | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT) & !(ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
    assert_ne!(unsafe { SetConsoleMode(input, mode) }, 0);
    let state = product_state::ReadGuard::register(input, 41).expect("시험 전용 collector 등록");
    let tid = unsafe { GetCurrentThreadId() };
    let raw = input as usize;
    let queued = case == "queued";
    let controller = std::thread::spawn(move || {
        let input = raw as HANDLE;
        if queued {
            println!("[queue-input]");
            std::io::stdout().flush().unwrap();
            loop {
                let wait_ms = deadline.saturating_duration_since(Instant::now()).as_millis() as u32;
                assert_eq!(unsafe { WaitForSingleObject(input, wait_ms) }, WAIT_OBJECT_0);
                let mut records = [INPUT_RECORD::default(); 128];
                let mut count = 0;
                assert_ne!(unsafe { PeekConsoleInputW(input, records.as_mut_ptr(), 128, &mut count) }, 0);
                if records[..count as usize].iter().any(|r| {
                    r.EventType == KEY_EVENT as u16
                        && unsafe { r.Event.KeyEvent.bKeyDown != 0 && r.Event.KeyEvent.uChar.UnicodeChar == 13 }
                }) {
                    break;
                }
                assert!(Instant::now() < deadline);
                unsafe { SwitchToThread() };
            }
        } else {
            let thread = unsafe { OpenThread(THREAD_QUERY_INFORMATION, 0, tid) };
            assert!(!thread.is_null());
            let thread = unsafe { OwnedHandle::from_raw_handle(thread) };
            loop {
                let mut pending = 0;
                assert_ne!(unsafe { GetThreadIOPendingFlag(thread.as_raw_handle(), &mut pending) }, 0);
                if pending != 0 {
                    break;
                }
                assert!(Instant::now() < deadline);
                unsafe { SwitchToThread() };
            }
        }
        println!("[state-ready]");
        std::io::stdout().flush().unwrap();
    });
    if queued {
        assert_eq!(
            unsafe {
                WaitForSingleObject(
                    replied.as_raw_handle(),
                    deadline.saturating_duration_since(Instant::now()).as_millis() as u32,
                )
            },
            WAIT_OBJECT_0
        );
    }
    state.call();
    let control = CONSOLE_READCONSOLE_CONTROL {
        nLength: std::mem::size_of::<CONSOLE_READCONSOLE_CONTROL>() as u32,
        nInitialChars: 0,
        dwCtrlWakeupMask: (1 << 3) | (1 << 26),
        dwControlKeyState: 0,
    };
    let mut value = vec![0u16; 16384];
    let mut count = 0;
    assert_ne!(unsafe { ReadConsoleW(input, value.as_mut_ptr().cast(), 16384, &mut count, &control) }, 0);
    state.returned();
    value.truncate(count as usize);
    assert_eq!(value, "fixture-value\r".encode_utf16().collect::<Vec<_>>());
    let mut after = 0;
    assert_ne!(unsafe { GetConsoleMode(input, &mut after) }, 0);
    assert_eq!(after, mode);
    drop(state);
    controller.join().unwrap();
    assert_ne!(unsafe { SetConsoleMode(input, original) }, 0);
    println!("read_state_fixture_complete");
}
