//! 수동 실행하는 독립 ConPTY 재현. 실제 설치·사용자 콘솔에는 연결하지 않는다.
use super::*;
use std::io::Write;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};
use windows_sys::Win32::System::Console::*;

const NAME: &str = "authorization::terminal_snapshot::native::repro::cooked_read_event_orders";

pub(super) fn os_version() -> (u32, u8) {
    use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOEXW;
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn RtlGetVersion(version: *mut OSVERSIONINFOEXW) -> i32;
    }
    let mut version = OSVERSIONINFOEXW::default();
    version.dwOSVersionInfoSize = std::mem::size_of_val(&version) as u32;
    assert_eq!(unsafe { RtlGetVersion(&mut version) }, 0);
    (version.dwBuildNumber, version.wProductType)
}

fn event() -> Arc<OwnedHandle> {
    Arc::new(owned(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) }, "repro_event").unwrap())
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
fn input() -> OwnedHandle {
    use windows_sys::Win32::Storage::FileSystem::*;
    owned(
        unsafe {
            CreateFileW(
                wide(OsStr::new("CONIN$")).as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        },
        "repro_input",
    )
    .unwrap()
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

fn send_marker(round: usize) {
    println!("fixture_input_{round}");
    std::io::stdout().flush().unwrap();
}

fn pending(thread: HANDLE, deadline: Instant) {
    loop {
        let mut pending = 0;
        assert_ne!(unsafe { GetThreadIOPendingFlag(thread, &mut pending) }, 0);
        if pending != 0 {
            return;
        }
        assert!(Instant::now() < deadline, "read의 kernel I/O pending을 관측하지 못했다");
        // 정해진 시간을 자지 않고 OS의 실제 상태로 끝낸다. pending은 서버 대기 등록 완료 증거가 아니다.
        unsafe { SwitchToThread() };
    }
}

fn fixture(order: &str, trace: bool) {
    // panic 때 대기 중 reader의 handle을 닫지 않고 자기 fixture 프로세스를 종료한다.
    std::panic::set_hook(Box::new(|info| {
        eprintln!("console repro fixture failure: {info}");
        std::process::exit(1);
    }));
    let input = input();
    let raw = input.as_raw_handle() as usize;
    let mut original = 0;
    assert_ne!(unsafe { GetConsoleMode(input.as_raw_handle(), &mut original) }, 0);
    let mode =
        (original | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT) & !(ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
    assert_ne!(unsafe { SetConsoleMode(input.as_raw_handle(), mode) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    for round in 0..14 {
        let start = event();
        let done = event();
        let (tid_tx, tid_rx) = mpsc::channel();
        let (read_tx, read_rx) = mpsc::channel();
        let reader_start = start.clone();
        let reader_done = done.clone();
        let reader = std::thread::spawn(move || {
            tid_tx.send(unsafe { GetCurrentThreadId() }).unwrap();
            wait(&reader_start, deadline);
            let control = CONSOLE_READCONSOLE_CONTROL {
                nLength: std::mem::size_of::<CONSOLE_READCONSOLE_CONTROL>() as u32,
                nInitialChars: 0,
                dwCtrlWakeupMask: (1 << 3) | (1 << 26),
                dwControlKeyState: 0,
            };
            let mut buffer = vec![0u16; 16384];
            let mut count = 0;
            let ok = unsafe {
                ReadConsoleW(raw as HANDLE, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut count, &control)
            };
            let code = if ok == 0 { unsafe { GetLastError() } } else { 0 };
            buffer.truncate(count as usize);
            signal(&reader_done);
            read_tx.send((ok, code, buffer)).unwrap();
        });
        let tid = tid_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
        let thread = owned(unsafe { OpenThread(THREAD_QUERY_INFORMATION, 0, tid) }, "repro_thread").unwrap();
        assert_eq!(unsafe { GetProcessIdOfThread(thread.as_raw_handle()) }, std::process::id());
        let observer = if order == "peek_armed" {
            let observer_done = done.clone();
            let (ready_tx, ready_rx) = mpsc::channel();
            let observer = std::thread::spawn(move || {
                let handles = [observer_done.as_raw_handle(), raw as HANDLE];
                ready_tx.send(()).unwrap();
                loop {
                    match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, remaining(deadline)) } {
                        WAIT_OBJECT_0 => return false,
                        value if value == WAIT_OBJECT_0 + 1 => {
                            if has_enter(raw as HANDLE) {
                                return true;
                            }
                            assert!(Instant::now() < deadline, "Peek 사건 상한");
                            unsafe { SwitchToThread() };
                        }
                        other => panic!("Peek wait: result={other}, os_code={}", unsafe { GetLastError() }),
                    }
                }
            });
            ready_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
            Some(observer)
        } else {
            None
        };
        if order == "prequeued" {
            send_marker(round);
            loop {
                assert_eq!(unsafe { WaitForSingleObject(input.as_raw_handle(), remaining(deadline)) }, WAIT_OBJECT_0);
                if has_enter(input.as_raw_handle()) {
                    break;
                }
                assert!(Instant::now() < deadline, "입력 도착 사건 상한");
                unsafe { SwitchToThread() };
            }
            println!("repro_event round={round} prequeued_enter=true");
            signal(&start);
        } else {
            signal(&start);
            pending(thread.as_raw_handle(), deadline);
            println!("repro_event round={round} reader_io_pending=true peek_armed={}", observer.is_some());
            send_marker(round);
        }
        if trace {
            // 제품의 긴 진단 출력과 같이 cooked read와 동시에 개행·한글·SGR을 출력한다.
            println!(
                "\x1b[2mDEBUG install_console_input_prepared read_id={round} input_mode={mode} 준비한 독립 콘솔의 입력·읽기·제어 키 관측\x1b[0m"
            );
            std::io::stdout().flush().unwrap();
        }
        let (ok, code, value) = read_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
        assert_ne!(ok, 0, "ReadConsoleW os_code={code}");
        assert_eq!(value, [48, 13], "fixture의 정해진 한 줄을 정확히 반환한다");
        reader.join().unwrap();
        let enter_seen = observer.map(|observer| observer.join().unwrap());
        println!("repro_event round={round} read_returned=true observer_enter={enter_seen:?}");
    }
    assert_ne!(unsafe { SetConsoleMode(input.as_raw_handle(), original) }, 0);
}

#[test]
#[ignore = "수동 진단: OS·conhost 판을 확인하고 --ignored --exact로 한 번 실행한다"]
fn cooked_read_event_orders() {
    if let Ok(order) = std::env::var("AAA_TEST_CONSOLE_REPRO_ORDER") {
        assert!(["prequeued", "read_pending", "peek_armed"].contains(&order.as_str()));
        fixture(&order, std::env::var_os("AAA_TEST_CONSOLE_REPRO_TRACE").is_some());
        return;
    }
    let version = file_version(&console_host_image().unwrap()).unwrap();
    let (build, product_type) = os_version();
    eprintln!("console_repro os_build={build} product_type={product_type} conhost_version={version:?}");
    for (name, actual) in [
        ("AAA_TEST_CONSOLE_EXPECTED_BUILD", build),
        ("AAA_TEST_CONSOLE_EXPECTED_PRODUCT_TYPE", u32::from(product_type)),
    ] {
        if let Ok(expected) = std::env::var(name) {
            assert_eq!(actual, expected.parse::<u32>().unwrap(), "OS가 다른 환경을 같은 환경 재현으로 세지 않는다");
        }
    }
    if let Ok(expected) = std::env::var("AAA_TEST_CONSOLE_EXPECTED_VERSION") {
        let expected: Vec<u16> = expected.split('.').map(|part| part.parse().unwrap()).collect();
        assert_eq!(version.as_slice(), expected, "conhost 판이 다른 환경을 같은 환경 재현으로 세지 않는다");
    }
    for order in ["prequeued", "read_pending", "peek_armed"] {
        for trace in [false, true] {
            let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
            command.args(["--exact", NAME, "--ignored", "--nocapture"]);
            command.env("AAA_TEST_CONSOLE_REPRO_ORDER", order);
            command.env_remove("AAA_TEST_CONSOLE_REPRO_TRACE");
            if trace {
                command.env("AAA_TEST_CONSOLE_REPRO_TRACE", "1");
            }
            let markers: Vec<_> = (0..14).map(|round| format!("fixture_input_{round}")).collect();
            let steps: Vec<_> = markers.iter().map(|marker| (marker.as_str(), "0\r")).collect();
            let (code, text) =
                super::super::super::drive_terminal_as(command, &steps, |_, _| {}, &std::env::current_exe().unwrap());
            assert_eq!(code, 0, "order={order} trace={trace}: {text}");
            assert!(super::super::super::terminal_fixture_succeeded(&text), "order={order} trace={trace}: {text}");
            eprintln!("console_repro order={order} trace={trace} reads=14 exit=0");
        }
    }
}
