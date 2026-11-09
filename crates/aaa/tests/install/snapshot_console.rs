//! 실패 뒤 helper의 콘솔 연결만 바꾼다. 입력·mode·살아 있는 호스트 thread는 바꾸지 않는다.
use super::super::{ConsoleInput, InputWait, ScreenSize};
use super::{Failure, console_host, console_host_image, owned, verify};
use std::os::windows::io::{AsRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
    System::{
        Console::*,
        Threading::{GetCurrentProcess, WaitForSingleObject},
    },
};

struct Attached;
impl Drop for Attached {
    fn drop(&mut self) {
        // helper만 분리한다. 대상 CLI는 계속 같은 콘솔에 붙어 있다.
        unsafe { FreeConsole() };
    }
}

pub(super) fn capture(
    client: HANDLE,
    pid: u32,
    owner: u32,
    mark: &mut impl FnMut(&'static str) -> Result<(), Failure>,
) -> Result<ConsoleInput, Failure> {
    let host_pid = console_host(client)?;
    // CLI와 host의 검증된 process handle을 수집 끝까지 유지한다.
    let _host = verify(host_pid, owner, &console_host_image()?)?;
    if unsafe { FreeConsole() } == 0 {
        return Err(Failure::os("console_input_detach"));
    }
    if unsafe { AttachConsole(pid) } == 0 {
        return Err(Failure::os("console_input_attach"));
    }
    let _attached = Attached;
    if console_host(unsafe { GetCurrentProcess() })? != host_pid {
        return Err(Failure::rejected("console_input_host_mismatch"));
    }
    let mut clients = [0; 3];
    let count = unsafe { GetConsoleProcessList(clients.as_mut_ptr(), clients.len() as u32) };
    if count == 0 {
        return Err(Failure::os("console_input_clients"));
    }
    // 다른 client가 붙은 공유 콘솔의 입력은 보지 않는다. PID 목록도 출력하지 않는다.
    if count != 2 || !clients[..2].contains(&pid) || !clients[..2].contains(&std::process::id()) {
        return Err(Failure::rejected("console_input_shared"));
    }
    sample_observed(open_input()?.as_raw_handle(), mark)
}

fn open_input() -> Result<OwnedHandle, Failure> {
    let name: Vec<_> = "CONIN$\0".encode_utf16().collect();
    owned(
        unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        },
        "console_input_open",
    )
}

fn sample(input: HANDLE) -> Result<ConsoleInput, Failure> {
    sample_observed(input, &mut |_| Ok(()))
}
fn sample_observed(
    input: HANDLE,
    mark: &mut impl FnMut(&'static str) -> Result<(), Failure>,
) -> Result<ConsoleInput, Failure> {
    mark("console_input_mode")?;
    let mut input_mode = 0;
    if unsafe { GetConsoleMode(input, &mut input_mode) } == 0 {
        return Err(Failure::os("console_input_mode"));
    }
    mark("console_input_peek")?;
    let mut records = [INPUT_RECORD::default(); 128];
    let mut count = 0;
    if unsafe { PeekConsoleInputW(input, records.as_mut_ptr(), records.len() as u32, &mut count) } == 0 {
        return Err(Failure::os("console_input_peek"));
    }
    if count as usize > records.len() {
        return Err(Failure::rejected("console_input_size"));
    }
    let keys = || {
        records[..count as usize]
            .iter()
            .filter(|record| record.EventType == KEY_EVENT as u16)
            .map(|record| unsafe { record.Event.KeyEvent })
            .filter(|key| key.bKeyDown != 0)
    };
    // 기존 큐 조회 뒤에 부가 상태를 읽으며, 각각의 API 진입을 journal에 남긴다.
    mark("console_input_wait")?;
    let result = unsafe { WaitForSingleObject(input, 0) };
    let conin_wait = InputWait { result, os_code: (result == WAIT_FAILED).then(|| unsafe { GetLastError() }) };
    mark("console_selection_info")?;
    let selection_flags = selection_flags().into();
    mark("console_screen_size")?;
    let screen_size = screen_size().into();
    // 원문·길이는 내보내지 않는다. Peek 뒤에도 대상의 원래 입력을 그대로 둔다.
    Ok(ConsoleInput {
        input_mode,
        records_present: count != 0,
        prefix_key_down: keys().next().is_some(),
        prefix_enter_down: keys().any(|key| unsafe { key.uChar.UnicodeChar } == 13),
        prefix_cancel_down: keys().any(|key| unsafe { key.uChar.UnicodeChar } == 3),
        prefix_eof_down: keys().any(|key| unsafe { key.uChar.UnicodeChar } == 26),
        conin_wait,
        selection_flags,
        screen_size,
    })
}

fn selection_flags() -> Result<u32, Failure> {
    let mut selection = CONSOLE_SELECTION_INFO::default();
    if unsafe { GetConsoleSelectionInfo(&mut selection) } == 0 {
        return Err(Failure::os("console_selection_info"));
    }
    // 선택 영역 좌표·화면 글자는 내보내지 않는다.
    Ok(selection.dwFlags)
}

fn screen_size() -> Result<ScreenSize, Failure> {
    let name: Vec<_> = "CONOUT$\0".encode_utf16().collect();
    let output = owned(
        unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        },
        "console_output_open",
    )?;
    screen_size_on(output.as_raw_handle())
}

fn screen_size_on(output: HANDLE) -> Result<ScreenSize, Failure> {
    let mut screen = CONSOLE_SCREEN_BUFFER_INFO::default();
    if unsafe { GetConsoleScreenBufferInfo(output, &mut screen) } == 0 {
        return Err(Failure::os("console_screen_info"));
    }
    Ok(ScreenSize {
        buffer_cols: screen.dwSize.X,
        buffer_rows: screen.dwSize.Y,
        window_cols: i32::from(screen.srWindow.Right) - i32::from(screen.srWindow.Left) + 1,
        window_rows: i32::from(screen.srWindow.Bottom) - i32::from(screen.srWindow.Top) + 1,
    })
}

#[test]
fn input_queue_view_preserves_keys_mode_and_private_values_in_an_isolated_console() {
    const NAME: &str = "authorization::terminal_snapshot::native::console::input_queue_view_preserves_keys_mode_and_private_values_in_an_isolated_console";
    if std::env::var_os("AAA_TEST_INPUT_QUEUE_FIXTURE").as_deref() != Some(std::ffi::OsStr::new("1")) {
        let mut command = portable_pty::CommandBuilder::new(std::env::current_exe().unwrap());
        command.args(["--exact", NAME, "--nocapture"]);
        command.env("AAA_TEST_INPUT_QUEUE_FIXTURE", "1");
        let (code, text) = super::super::super::drive_terminal(command, &[], |_, _| {});
        assert_eq!(code, 0, "격리한 큐 fixture: {text}");
        assert!(super::super::super::terminal_fixture_succeeded(&text), "실제 fixture 실행: {text}");
        return;
    }
    // 시험 자식의 새 ConPTY만 쓴다. 수집기는 읽기 전용 handle을 별도로 연다.
    let name: Vec<_> = "CONIN$\0".encode_utf16().collect();
    let writer = owned(
        unsafe {
            CreateFileW(
                name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        },
        "fixture_input",
    )
    .unwrap();
    let input = open_input().unwrap();
    let input = input.as_raw_handle();
    assert_ne!(unsafe { FlushConsoleInputBuffer(writer.as_raw_handle()) }, 0);
    let empty = sample(input).unwrap();
    assert!(
        !empty.records_present
            && !empty.prefix_key_down
            && !empty.prefix_enter_down
            && !empty.prefix_cancel_down
            && !empty.prefix_eof_down
    );
    assert_eq!(empty.conin_wait.result, WAIT_TIMEOUT);
    assert!(empty.conin_wait.os_code.is_none());
    assert!(matches!(empty.selection_flags, super::super::Measurement::Ok { value: 0 }));
    assert!(matches!(empty.screen_size, super::super::Measurement::Ok { ref value }
        if value.buffer_cols == 120 && value.buffer_rows == 36 && value.window_cols == 120 && value.window_rows == 36));
    let bad_screen: super::super::Measurement<ScreenSize> = screen_size_on(input).into();
    assert!(matches!(bad_screen, super::super::Measurement::Error { ref phase, os_code }
        if phase == "console_screen_info" && os_code != 0));
    let key = |unit, down| {
        let mut record = INPUT_RECORD { EventType: KEY_EVENT as u16, ..Default::default() };
        record.Event.KeyEvent = KEY_EVENT_RECORD {
            bKeyDown: down,
            wRepeatCount: 1,
            uChar: KEY_EVENT_RECORD_0 { UnicodeChar: unit },
            ..Default::default()
        };
        record
    };
    let cases = [
        crate::CANARY.encode_utf16().chain([13, 3, 26]).map(|unit| key(unit, 1)).collect::<Vec<_>>(),
        (0..128).map(|i| key(120 + i % 2, 1)).chain([key(13, 1)]).collect(),
        vec![key(13, 0), key(3, 0), key(26, 0)],
    ];
    for (case, expected) in cases.iter().enumerate() {
        let mut written = 0;
        assert_ne!(
            unsafe {
                WriteConsoleInputW(writer.as_raw_handle(), expected.as_ptr(), expected.len() as u32, &mut written)
            },
            0
        );
        assert!(written as usize == expected.len());
        let view = sample(input).unwrap();
        assert!(view.records_present);
        assert_eq!(view.conin_wait.result, WAIT_OBJECT_0, "입력이 있는 CONIN$의 zero-time wait");
        assert!(view.conin_wait.os_code.is_none());
        assert_eq!(view.input_mode, empty.input_mode, "관측이 mode를 바꾸지 않는다");
        let mut after_mode = 0;
        assert_ne!(unsafe { GetConsoleMode(input, &mut after_mode) }, 0);
        assert_eq!(after_mode, empty.input_mode, "Peek 뒤 실제 OS mode도 보존한다");
        match case {
            0 => assert!(
                view.prefix_key_down && view.prefix_enter_down && view.prefix_cancel_down && view.prefix_eof_down
            ),
            1 => assert!(
                view.prefix_key_down && !view.prefix_enter_down && !view.prefix_cancel_down && !view.prefix_eof_down,
                "앞 128개 뒤의 Enter를 보았다고 하지 않는다"
            ),
            _ => assert!(
                !view.prefix_key_down && !view.prefix_enter_down && !view.prefix_cancel_down && !view.prefix_eof_down,
                "KEY_UP은 종료 키 관측이 아니다"
            ),
        }
        assert!(!serde_json::to_string(&view).unwrap().contains(crate::CANARY));
        let mut queued = 0;
        assert_ne!(unsafe { GetNumberOfConsoleInputEvents(input, &mut queued) }, 0);
        assert!(queued as usize == expected.len(), "읽기 전에 입력이 전부 남아 있음을 확인한다");
        let mut actual = vec![INPUT_RECORD::default(); expected.len()];
        let mut read = 0;
        assert_ne!(unsafe { ReadConsoleInputW(input, actual.as_mut_ptr(), actual.len() as u32, &mut read) }, 0);
        assert!(read as usize == expected.len(), "Peek가 입력을 소비하지 않는다");
        assert!(
            actual.iter().zip(expected).all(|(actual, expected)| unsafe {
                actual.EventType == expected.EventType
                    && actual.Event.KeyEvent.bKeyDown == expected.Event.KeyEvent.bKeyDown
                    && actual.Event.KeyEvent.uChar.UnicodeChar == expected.Event.KeyEvent.uChar.UnicodeChar
            }),
            "입력 원문과 순서를 그대로 보존한다"
        );
    }
}
