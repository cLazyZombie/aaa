//! Windows 계정 암호는 실제 console에서만 읽고 원래 mode를 모든 종료 경로에서 돌린다.
use super::*;
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::Console::*,
};
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
struct Secret(Vec<u16>);
impl Drop for Secret {
    fn drop(&mut self) {
        for unit in &mut self.0 {
            unsafe { std::ptr::write_volatile(unit, 0) };
        }
        std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
    }
}
fn required() -> Fault {
    Fault { code: "installation_private_password_input_required", exit: 3 }
}
pub(in crate::install::engine) fn prompt() -> Result<supervisor::Password> {
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(required());
    }
    let mut original = 0;
    if unsafe { GetConsoleMode(handle, &mut original) } == 0 {
        return Err(required());
    }
    let _mode = Mode { handle, original };
    let mode =
        original & !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
    if unsafe { SetConsoleMode(handle, mode) } == 0 {
        return Err(required());
    }
    let mut secret = Secret(Vec::with_capacity(1024));
    print!("설치 사용자 계정 암호(Windows Hello PIN 아님, 입력은 표시되지 않습니다): ");
    std::io::stdout().flush().map_err(|_| required())?;
    loop {
        let mut unit = 0u16;
        let mut read = 0;
        let ok = unsafe { ReadConsoleW(handle, (&mut unit as *mut u16).cast(), 1, &mut read, std::ptr::null()) };
        if ok == 0 || read != 1 {
            return Err(Fault { code: "installation_cancelled", exit: 130 });
        }
        match unit {
            3 | 4 | 26 | 27 => return Err(Fault { code: "installation_cancelled", exit: 130 }),
            13 | 10 => {
                println!();
                if secret.0.is_empty() {
                    return Err(required());
                }
                return supervisor::Password::new(std::mem::take(&mut secret.0));
            }
            8 | 127 => {
                if !secret.0.is_empty() {
                    let index = secret.0.len() - 1;
                    let low = secret.0[index];
                    unsafe { std::ptr::write_volatile(&mut secret.0[index], 0) };
                    secret.0.remove(index);
                    if (0xDC00..=0xDFFF).contains(&low) && !secret.0.is_empty() {
                        let index = secret.0.len() - 1;
                        unsafe { std::ptr::write_volatile(&mut secret.0[index], 0) };
                        secret.0.remove(index);
                    }
                }
            }
            0 => return Err(required()),
            _ => {
                if secret.0.len() >= 1024 {
                    return Err(required());
                }
                secret.0.push(unit);
            }
        }
        unsafe { std::ptr::write_volatile(&mut unit, 0) };
    }
}

#[cfg(test)]
#[path = "../../../../tests/support/child_exit.rs"]
mod child_exit;
#[cfg(test)]
#[path = "../../../../tests/support/terminal_queries.rs"]
mod terminal_queries;

#[cfg(test)]
mod tests {
    use super::*;
    use portable_pty::{CommandBuilder, PtySize, native_pty_system};
    use std::{
        io::{Read, Write},
        sync::mpsc,
        time::{Duration, Instant},
    };
    const CANARY: &str = "password-console-canary-DO-NOT-LOG";
    enum Event {
        Output(Vec<u8>),
        ReaderDone,
        ChildExited(std::io::Result<()>),
    }
    #[test]
    fn console_masks_and_restores_after_success_cancel_and_eof() {
        if let Ok(case) = std::env::var("AAA_INSTALL_PASSWORD_CHILD") {
            let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
            let mut initial = 0;
            assert_ne!(unsafe { GetConsoleMode(handle, &mut initial) }, 0, "test input must be a real ConPTY console");
            let _restore = Mode { handle, original: initial };
            let cooked = initial | ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT;
            assert_ne!(unsafe { SetConsoleMode(handle, cooked) }, 0);
            let result = prompt();
            let mut restored = 0;
            assert_ne!(unsafe { GetConsoleMode(handle, &mut restored) }, 0);
            assert_eq!(restored, cooked, "모든 경로에서 원래 console mode를 돌린다");
            if case == "success" {
                let password = result.unwrap_or_else(|_| panic!("console password success"));
                password
                    .send(&mut std::io::sink(), |_, bytes| {
                        assert!(bytes == CANARY.as_bytes(), "보호 입력이 정확해야 한다");
                        Ok(())
                    })
                    .unwrap();
            } else {
                assert!(matches!(result, Err(Fault { exit: 130, .. })), "cancel/EOF closes without a credential");
            }
            println!("console-restored");
            return;
        }
        for (case, suffix) in [("success", "\r"), ("cancel", "\x03"), ("eof", "\x1a")] {
            let pair =
                native_pty_system().openpty(PtySize { rows: 24, cols: 100, pixel_width: 0, pixel_height: 0 }).unwrap();
            let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
            command.args([
                "--exact",
                "install::engine::input::password::tests::console_masks_and_restores_after_success_cancel_and_eof",
                "--nocapture",
            ]);
            command.env("AAA_INSTALL_PASSWORD_CHILD", case);
            let mut child = pair.slave.spawn_command(command).unwrap();
            let exit = child_exit::ChildExit::new(child.as_mut()).unwrap();
            drop(pair.slave);
            let mut master = Some(pair.master);
            let mut reader = master.as_ref().unwrap().try_clone_reader().unwrap();
            let mut writer = master.as_ref().unwrap().take_writer().unwrap();
            let (send, receive) = mpsc::channel();
            let exited = send.clone();
            let waiting = std::thread::spawn(move || {
                let _ = exited.send(Event::ChildExited(exit.wait()));
            });
            let reading = std::thread::spawn(move || {
                let mut bytes = [0u8; 2048];
                while let Ok(n) = reader.read(&mut bytes) {
                    if n == 0 || send.send(Event::Output(bytes[..n].to_vec())).is_err() {
                        break;
                    }
                }
                let _ = send.send(Event::ReaderDone);
            });
            let deadline = Instant::now() + Duration::from_secs(20);
            let mut terminal = terminal_queries::TerminalQueries::new(24, 100);
            let mut output = Vec::new();
            let mut sent = false;
            let mut status = None;
            let mut reader_done = false;
            while status.is_none() || !reader_done {
                match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(Event::Output(bytes)) => {
                        let visible = if status.is_none() {
                            terminal.feed(&bytes, &mut writer).unwrap()
                        } else {
                            terminal.feed(&bytes, &mut std::io::sink()).unwrap()
                        };
                        output.extend(visible);
                        if status.is_none()
                            && !sent
                            && String::from_utf8_lossy(&output).contains("입력은 표시되지 않습니다")
                        {
                            writer.write_all(CANARY.as_bytes()).unwrap();
                            writer.write_all(suffix.as_bytes()).unwrap();
                            writer.flush().unwrap();
                            sent = true;
                        }
                    }
                    Ok(Event::ReaderDone) => reader_done = true,
                    Ok(Event::ChildExited(result)) => {
                        result.expect("owned console child exit event");
                        status = Some(child.wait().unwrap());
                        drop(master.take());
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => panic!("owned console event channel closed"),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if status.is_none() {
                            child.kill().unwrap();
                            child.wait().unwrap();
                        }
                        let output = String::from_utf8_lossy(&output).replace(CANARY, "[private]");
                        panic!(
                            "owned console password event deadline, case={case}, sent={sent}, child_exit={status:?}, reader_done={reader_done}, output={output}"
                        );
                    }
                }
            }
            let status = status.unwrap();
            drop(writer);
            reading.join().unwrap();
            waiting.join().unwrap();
            let output = String::from_utf8_lossy(&output);
            assert!(sent, "보호 prompt 전에 입력을 보내지 않는다");
            assert!(!output.contains(CANARY), "실제 입력한 credential이 TTY에 echo되면 안 된다");
            assert_eq!(status.exit_code(), 0, "owned console child failed, case={case}");
            assert!(output.contains("console-restored"));
        }
    }
}
