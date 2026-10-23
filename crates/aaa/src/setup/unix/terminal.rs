//! Unix 대화형 입력. Ctrl+C는 읽기를 끊고 설치 흐름이 변경을 되돌리게 한다.
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::super::flow::{Cancelled, Secret, Tone, Ui};
use anyhow::{Result, bail};

static CANCEL: AtomicBool = AtomicBool::new(false);
extern "C" fn cancel(_: libc::c_int) {
    CANCEL.store(true, Ordering::SeqCst);
}

pub(super) struct Terminal {
    previous: Vec<(libc::c_int, libc::sigaction)>,
    last_second: Option<u64>,
}
impl Terminal {
    pub(super) fn new() -> Result<Self> {
        CANCEL.store(false, Ordering::SeqCst);
        let mut this = Self { previous: Vec::new(), last_second: None };
        for signal in [libc::SIGINT, libc::SIGTERM] {
            // SA_RESTART 없이 콘솔 read를 끊는다. 종료 때 기존 handler를 복원한다.
            let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
            let mut previous: libc::sigaction = unsafe { std::mem::zeroed() };
            action.sa_sigaction = cancel as *const () as usize;
            unsafe {
                libc::sigemptyset(&mut action.sa_mask);
            }
            if unsafe { libc::sigaction(signal, &action, &mut previous) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            this.previous.push((signal, previous));
        }
        Ok(this)
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        for (signal, action) in &self.previous {
            unsafe {
                libc::sigaction(*signal, action, std::ptr::null_mut());
            }
        }
    }
}
impl Ui for Terminal {
    fn say(&mut self, tone: Tone, line: &str) {
        let prefix = match tone {
            Tone::Ok => "  ✓ ",
            Tone::Warn => "  ! ",
            Tone::Fail => "  ✗ ",
            Tone::Hint | Tone::Info | Tone::Code => "    ",
            _ => "",
        };
        println!("{prefix}{line}");
    }
    fn ask(&mut self, label: &str, default: Option<&str>) -> Result<String> {
        if self.cancelled() {
            return Err(Cancelled.into());
        }
        match default {
            Some(value) => print!("  ? {label} [{value}]: "),
            None => print!("  ? {label}: "),
        }
        std::io::stdout().flush()?;
        let mut bytes = Vec::new();
        loop {
            let mut byte = 0u8;
            let n = unsafe { libc::read(libc::STDIN_FILENO, (&mut byte as *mut u8).cast(), 1) };
            if self.cancelled() || n == 0 {
                return Err(Cancelled.into());
            }
            if n < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            if byte == b'\n' {
                break;
            }
            anyhow::ensure!(bytes.len() < 4096, "입력이 너무 깁니다");
            bytes.push(byte);
        }
        let answer = String::from_utf8(bytes)?;
        let answer = answer.trim();
        Ok(if answer.is_empty() { default.unwrap_or_default() } else { answer }.into())
    }
    fn secret(&mut self, _: &str) -> Result<Secret> {
        bail!("Unix 설치는 Windows 암호를 받지 않습니다")
    }
    fn spin(&mut self, label: &str, elapsed: Duration) {
        let second = elapsed.as_secs();
        if self.last_second != Some(second) && second.is_multiple_of(5) {
            println!("    {label} ({second}초)");
            self.last_second = Some(second);
        }
    }
    fn spin_end(&mut self) {
        self.last_second = None;
    }
    fn cancelled(&self) -> bool {
        CANCEL.load(Ordering::SeqCst)
    }
    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
