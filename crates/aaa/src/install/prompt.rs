//! 일반 질문의 줄 입력. Windows도 Ctrl+C/EOF를 v1 cancelled/130으로 돌린다.
use super::*;
pub(super) fn line(question: &str) -> Result<String, Box<InstallResult>> {
    #[cfg(unix)]
    {
        unix::line(question)
    }
    #[cfg(windows)]
    windows::line(question)
}
// 모드를 준비한 뒤에만 응답을 요청한다. 표시 직후 들어온 취소도 같은 경계를 지난다.
fn show(question: &str) -> Result<(), Box<InstallResult>> {
    print!("{question}");
    std::io::stdout()
        .flush()
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, error("installation_prompt_closed", 3)))
}
#[cfg(unix)]
mod unix {
    use super::*;
    struct Mode(libc::termios);
    impl Drop for Mode {
        fn drop(&mut self) {
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.0);
            }
        }
    }
    pub(super) fn line(question: &str) -> Result<String, Box<InstallResult>> {
        let mut original = std::mem::MaybeUninit::<libc::termios>::uninit();
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, original.as_mut_ptr()) } != 0 {
            return Err(error("installation_prompt_closed", 3));
        }
        let original = unsafe { original.assume_init() };
        let _mode = Mode(original);
        let mut mode = original;
        // canonical 편집/EOF는 OS에 맡기며 Ctrl+C만 line 종료 사건으로 받아 결과를 만든다.
        mode.c_lflag = (mode.c_lflag | libc::ICANON) & !libc::ISIG;
        mode.c_cc[libc::VEOL] = 3;
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &mode) } != 0 {
            return Err(error("installation_prompt_closed", 3));
        }
        show(question)?;
        // read_line은 VEOL의 Ctrl+C 뒤에도 LF를 더 기다린다. 실제 canonical read 한 덩어리를 읽는다.
        use std::io::Read;
        let mut value = vec![0u8; 65536];
        let count = std::io::stdin().lock().read(&mut value).map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, error("installation_prompt_closed", 3))
        })?;
        value.truncate(count);
        if value.is_empty() || value.contains(&3) {
            return Err(error("installation_cancelled", 130));
        }
        let value = String::from_utf8(value).map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, error("installation_prompt_invalid", 2))
        })?;
        Ok(value.trim_end_matches(['\r', '\n']).into())
    }
}
#[cfg(windows)]
mod windows {
    use super::*;
    use windows_sys::Win32::{Foundation::*, System::Console::*};
    struct Mode {
        handle: HANDLE,
        original: u32,
    }
    impl Drop for Mode {
        fn drop(&mut self) {
            unsafe {
                SetConsoleMode(self.handle, self.original);
            }
        }
    }
    pub(super) fn line(question: &str) -> Result<String, Box<InstallResult>> {
        let unavailable = || error("installation_prompt_closed", 3);
        let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
        let mut original = 0;
        if handle.is_null() || handle == INVALID_HANDLE_VALUE || unsafe { GetConsoleMode(handle, &mut original) } == 0 {
            return Err(unavailable());
        }
        let _mode = Mode { handle, original };
        // OS cooked editing/echo를 사용한다. processed Ctrl+C의 process 종료에 계약을 맡기지 않는다.
        let mode = (original | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT)
            & !(ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
        if unsafe { SetConsoleMode(handle, mode) } == 0 {
            return Err(unavailable());
        }
        let control = CONSOLE_READCONSOLE_CONTROL {
            nLength: std::mem::size_of::<CONSOLE_READCONSOLE_CONTROL>() as u32,
            nInitialChars: 0,
            dwCtrlWakeupMask: (1 << 3) | (1 << 26),
            dwControlKeyState: 0,
        };
        let mut value = vec![0u16; 16384];
        let mut count = 0;
        show(question)?;
        let ok = unsafe { ReadConsoleW(handle, value.as_mut_ptr().cast(), value.len() as u32, &mut count, &control) };
        if ok == 0 {
            return Err(if unsafe { GetLastError() } == ERROR_OPERATION_ABORTED {
                error("installation_cancelled", 130)
            } else {
                unavailable()
            });
        }
        if count as usize > value.len() {
            return Err(unavailable());
        }
        value.truncate(count as usize);
        if value.is_empty() || value.iter().any(|unit| matches!(unit, 3 | 26)) {
            return Err(error("installation_cancelled", 130));
        }
        let value = String::from_utf16(&value).map_err(|source_error| {
            aaa_common::diagnostic::context(source_error, error("installation_prompt_invalid", 2))
        })?;
        Ok(value.trim_end_matches(['\r', '\n']).into())
    }
}
