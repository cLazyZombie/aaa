//! Windows 콘솔 화면과 SCM·LSA·레지스트리 조작. 암호는 `Secret`으로만 넘기고 오류·로그에 쓰지 않는다.
use std::io::{BufRead, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authentication::Identity::*, Authorization::*, *},
    Storage::FileSystem::DELETE,
    System::{
        Console::*,
        Registry::*,
        Services::*,
        SystemInformation::{ComputerNameDnsHostname, GetComputerNameExW},
        Threading::*,
    },
    UI::{
        Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SW_SHOWNORMAL, SendMessageTimeoutW, WM_SETTINGCHANGE},
    },
};

use super::Args;
use super::flow::{
    Account, Cancelled, Layout, Logon, Right, Secret, ServiceDef, ServiceInfo, Setup, System, Tone, Ui, code_lines,
};

static CANCEL: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn on_ctrl(kind: u32) -> windows_sys::core::BOOL {
    if kind == CTRL_C_EVENT || kind == CTRL_BREAK_EVENT {
        CANCEL.store(true, Ordering::SeqCst);
        return 1;
    }
    0
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn wide_os(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
unsafe fn text(ptr: *const u16) -> String {
    if ptr.is_null() {
        return String::new();
    }
    let mut n = 0;
    while unsafe { *ptr.add(n) } != 0 && n < 32768 {
        n += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, n) })
}
fn last_error() -> u32 {
    unsafe { GetLastError() }
}
fn os_error(code: u32) -> String {
    format!("{} (Windows 오류 {code})", std::io::Error::from_raw_os_error(code as i32))
}

/// 콘솔 화면. VT가 되면 색과 제자리 진행 줄을 쓴다
struct Terminal {
    vt: bool,
    color: bool,
    spinning: bool,
    frame: usize,
    last_plain: Option<Instant>,
}

impl Terminal {
    fn new() -> Terminal {
        let out = unsafe { GetStdHandle(STD_OUTPUT_HANDLE) };
        let mut mode = 0;
        let vt = unsafe { GetConsoleMode(out, &mut mode) } != 0
            && unsafe { SetConsoleMode(out, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) } != 0;
        unsafe { SetConsoleCtrlHandler(Some(on_ctrl), 1) };
        Terminal {
            vt,
            color: vt && std::env::var_os("NO_COLOR").is_none(),
            spinning: false,
            frame: 0,
            last_plain: None,
        }
    }
    fn paint(&self, code: &str, text: &str) -> String {
        if self.color { format!("\x1b[{code}m{text}\x1b[0m") } else { text.to_string() }
    }
    fn clear_spin(&mut self) {
        if self.spinning && self.vt {
            print!("\r\x1b[2K");
            aaa_common::diagnostic::ignored(std::io::stdout().flush(), "보조 작업 실패 뒤 기존 처리를 계속한다");
        }
        self.spinning = false;
    }
    fn read_line(&mut self) -> Result<String> {
        if let Some(line) = read_console_line()? {
            return Ok(line);
        }
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) => Err(Cancelled.into()),
            Ok(_) => Ok(line.trim().to_string()),
            Err(e) => Err(e.into()),
        }
    }
}

/// 콘솔 한 줄. std의 stdin은 Ctrl+C로 끊긴 ReadConsoleW를 다시 읽어 Enter까지 기다리므로 직접 읽는다.
/// 콘솔이 아니면 None
fn read_console_line() -> Result<Option<String>> {
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut original = 0;
    if handle.is_null() || handle == INVALID_HANDLE_VALUE || unsafe { GetConsoleMode(handle, &mut original) } == 0 {
        return Ok(None);
    }
    let _restore = ConsoleMode { handle, original };
    unsafe { SetConsoleMode(handle, original | ENABLE_PROCESSED_INPUT | ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT) };
    let mut line: Vec<u16> = Vec::new();
    loop {
        let mut buffer = [0u16; 512];
        let mut read = 0u32;
        let ok = unsafe {
            ReadConsoleW(handle, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut read, std::ptr::null())
        };
        let aborted = ok == 0 || read == 0;
        if aborted {
            // Ctrl+C 처리기는 다른 thread에서 늦게 돌 수 있다
            let started = Instant::now();
            while !CANCEL.load(Ordering::SeqCst) && started.elapsed() < Duration::from_millis(300) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        if CANCEL.load(Ordering::SeqCst) {
            println!();
            return Err(Cancelled.into());
        }
        if ok == 0 {
            match last_error() {
                ERROR_OPERATION_ABORTED => continue,
                code => bail!("콘솔 입력을 읽지 못했습니다: {}", os_error(code)),
            }
        }
        if read == 0 {
            return Err(Cancelled.into());
        }
        line.extend_from_slice(&buffer[..read as usize]);
        if line.last() == Some(&10) || line.len() > 4096 {
            break;
        }
    }
    let text = String::from_utf16_lossy(&line);
    // Ctrl+Z Enter는 콘솔의 EOF다
    if text.starts_with('\u{1a}') {
        return Err(Cancelled.into());
    }
    Ok(Some(text.trim().to_string()))
}

impl Ui for Terminal {
    fn say(&mut self, tone: Tone, line: &str) {
        self.clear_spin();
        let out = match tone {
            Tone::Plain => line.to_string(),
            Tone::Title => self.paint("1", line),
            Tone::Step => format!("\n{}", self.paint("1;36", line)),
            Tone::Ok => format!("      {} {line}", self.paint("32", "✓")),
            Tone::Warn => format!("      {} {}", self.paint("33", "!"), self.paint("33", line)),
            Tone::Fail => format!("      {} {}", self.paint("31", "✗"), self.paint("31", line)),
            Tone::Hint => format!("      {}", self.paint("2", line)),
            Tone::Code => format!("      {}", self.paint("1;35", line)),
            Tone::Info => format!("      {line}"),
        };
        println!("{out}");
    }
    fn ask(&mut self, label: &str, default: Option<&str>) -> Result<String> {
        self.clear_spin();
        let shown = match default {
            Some(d) if !d.is_empty() => format!("{label} [{}]: ", self.paint("36", d)),
            Some(_) => format!("{label}: "),
            None => format!("{label}: "),
        };
        print!("  {} {shown}", self.paint("1;32", "?"));
        std::io::stdout().flush()?;
        let answer = self.read_line()?;
        Ok(if answer.is_empty() { default.unwrap_or_default().to_string() } else { answer })
    }
    fn secret(&mut self, label: &str) -> Result<Secret> {
        self.clear_spin();
        print!("  {} {label} {}: ", self.paint("1;32", "?"), self.paint("2", "(화면에 보이지 않음)"));
        std::io::stdout().flush()?;
        read_secret()
    }
    fn spin(&mut self, label: &str, elapsed: Duration) {
        let secs = elapsed.as_secs();
        let clock = format!("{}:{:02}", secs / 60, secs % 60);
        if self.vt {
            const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            self.frame = (self.frame + 1) % FRAMES.len();
            print!("\r\x1b[2K      {} {label} {}", self.paint("36", FRAMES[self.frame]), self.paint("2", &clock));
            aaa_common::diagnostic::ignored(std::io::stdout().flush(), "보조 작업 실패 뒤 기존 처리를 계속한다");
            self.spinning = true;
        } else if self.last_plain.is_none_or(|at| at.elapsed() >= Duration::from_secs(10)) {
            println!("      … {label} {clock}");
            self.last_plain = Some(Instant::now());
        }
    }
    fn spin_end(&mut self) {
        self.clear_spin();
        self.last_plain = None;
    }
    fn cancelled(&self) -> bool {
        CANCEL.load(Ordering::SeqCst)
    }
    fn pause(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}

struct ConsoleMode {
    handle: HANDLE,
    original: u32,
}
impl Drop for ConsoleMode {
    fn drop(&mut self) {
        unsafe { SetConsoleMode(self.handle, self.original) };
    }
}

/// echo·줄 입력을 끄고 한 글자씩 읽는다. Ctrl+C·Esc·EOF는 취소, 원래 콘솔 모드는 모든 경로에서 돌린다
fn read_secret() -> Result<Secret> {
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut original = 0;
    if handle.is_null() || handle == INVALID_HANDLE_VALUE || unsafe { GetConsoleMode(handle, &mut original) } == 0 {
        bail!("숨김 입력에는 대화형 콘솔이 필요합니다(SSH라면 `ssh -t`로 접속하세요)");
    }
    let _restore = ConsoleMode { handle, original };
    let raw =
        original & !(ENABLE_ECHO_INPUT | ENABLE_LINE_INPUT | ENABLE_PROCESSED_INPUT | ENABLE_VIRTUAL_TERMINAL_INPUT);
    if unsafe { SetConsoleMode(handle, raw) } == 0 {
        bail!("콘솔 입력 모드를 바꾸지 못했습니다: {}", os_error(last_error()));
    }
    let mut units: Vec<u16> = Vec::with_capacity(256);
    let wipe = |units: &mut Vec<u16>| {
        for unit in units.iter_mut() {
            unsafe { std::ptr::write_volatile(unit, 0) };
        }
        units.clear();
    };
    loop {
        let mut unit = 0u16;
        let mut read = 0;
        let ok = unsafe { ReadConsoleW(handle, (&mut unit as *mut u16).cast(), 1, &mut read, std::ptr::null()) };
        if ok == 0 || read != 1 {
            wipe(&mut units);
            println!();
            return Err(Cancelled.into());
        }
        match unit {
            3 | 4 | 26 | 27 => {
                wipe(&mut units);
                println!();
                return Err(Cancelled.into());
            }
            13 | 10 => {
                println!();
                let secret = Secret::new(units.clone());
                wipe(&mut units);
                return Ok(secret);
            }
            8 | 127 => {
                if let Some(low) = units.pop()
                    && (0xDC00..=0xDFFF).contains(&low)
                {
                    units.pop();
                }
            }
            0 => {}
            _ if units.len() < 1024 => units.push(unit),
            _ => {}
        }
        unsafe { std::ptr::write_volatile(&mut unit, 0) };
    }
}

fn stdin_is_console() -> bool {
    let handle = unsafe { GetStdHandle(STD_INPUT_HANDLE) };
    let mut mode = 0;
    !handle.is_null() && handle != INVALID_HANDLE_VALUE && unsafe { GetConsoleMode(handle, &mut mode) } != 0
}
fn over_ssh() -> bool {
    std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_CLIENT").is_some()
}
/// 이 콘솔 창을 이 프로세스 혼자 쓴다(탐색기 더블클릭): 끝에 Enter를 기다려 결과를 읽게 한다
fn owns_console() -> bool {
    let mut list = [0u32; 4];
    unsafe { GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) == 1 }
}

fn layout() -> Result<Layout> {
    let instance = aaa_config::paths::instance();
    let config = aaa_config::paths::config_dir();
    let state = aaa_config::paths::state_dir();
    let explicit_dirs = std::env::var_os("AAA_CONFIG_DIR").is_some() || std::env::var_os("AAA_STATE_DIR").is_some();
    let home = aaa_config::paths::home();
    let exe = if instance.is_none() && !explicit_dirs {
        home.join(".local").join("bin").join("aaa.exe")
    } else {
        state.parent().unwrap_or(&state).join("bin").join("aaa.exe")
    };
    let source = std::env::current_exe().context("이 설치기 경로")?;
    Ok(Layout { instance, source, exe, config, state, explicit_dirs, home })
}

pub(super) fn run(args: &Args) -> ExitCode {
    let mut term = Terminal::new();
    let pause_at_end = owns_console() && !over_ssh();
    let code = run_in(args, &mut term);
    if pause_at_end {
        term.spin_end();
        print!("\nEnter를 누르면 이 창을 닫습니다.");
        aaa_common::diagnostic::ignored(std::io::stdout().flush(), "보조 작업 실패 뒤 기존 처리를 계속한다");
        let mut line = String::new();
        aaa_common::diagnostic::ignored(
            std::io::stdin().lock().read_line(&mut line),
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
    }
    code
}

fn run_in(args: &Args, term: &mut Terminal) -> ExitCode {
    let layout = match layout() {
        Ok(layout) => layout,
        Err(e) => {
            term.say(Tone::Fail, &format!("{e:#}"));
            return ExitCode::FAILURE;
        }
    };
    let version = aaa_common::util::software();
    let mut sys = WinSystem;
    if !args.status {
        if !stdin_is_console() {
            term.say(Tone::Fail, "대화형 콘솔에서 실행해야 합니다(질문과 숨김 암호 입력이 있습니다).");
            term.say(Tone::Hint, "SSH라면 `ssh -t <PC> <설치기 경로>`처럼 -t를 붙이세요.");
            return ExitCode::from(2);
        }
        match sys.elevated() {
            Ok(true) => {}
            Ok(false) => return elevate(term),
            Err(e) => {
                term.say(Tone::Fail, &format!("관리자 권한을 확인하지 못했습니다: {e:#}"));
                return ExitCode::FAILURE;
            }
        }
    }
    let mut setup = Setup::new(&mut sys, term, layout, version);
    let result = if args.status {
        setup.status()
    } else if args.uninstall {
        setup.uninstall(args)
    } else {
        setup.install(args)
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.downcast_ref::<Cancelled>().is_some() => ExitCode::from(130),
        Err(e) => {
            // 흐름이 이미 원인과 되돌린 것을 보였다. 여기서는 짧게 남긴다
            tracing::debug!("setup 실패: {e:#}");
            ExitCode::FAILURE
        }
    }
}

/// 관리자 창에서 같은 인자로 다시 실행한다(UAC). SSH에서는 UAC 창을 띄울 수 없다
fn elevate(term: &mut Terminal) -> ExitCode {
    term.say(Tone::Plain, "Windows 서비스 등록과 「서비스로 로그온」 권한에는 관리자 권한이 필요합니다.");
    if over_ssh() {
        term.say(Tone::Fail, "이 SSH 세션은 관리자 권한이 아닙니다.");
        term.say(
            Tone::Hint,
            "관리자 계정으로 SSH 접속하거나, 이 PC에서 「관리자 권한으로 실행」한 PowerShell에서 실행하세요.",
        );
        return ExitCode::from(2);
    }
    let answer = match term.ask("관리자 창을 열어 계속할까요? (Y/n)", Some("Y")) {
        Ok(answer) => answer,
        Err(source_error) => {
            aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
            return ExitCode::from(130);
        }
    };
    if !matches!(answer.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes" | "ㅛ" | "예" | "네") {
        return ExitCode::from(130);
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(e) => {
            term.say(Tone::Fail, &format!("설치기 경로: {e}"));
            return ExitCode::FAILURE;
        }
    };
    let params: Vec<String> = std::env::args_os().skip(1).map(|a| windows_quote(&a.to_string_lossy())).collect();
    let file = wide_os(exe.as_os_str());
    let verb = wide("runas");
    let params = wide(&params.join(" "));
    let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
    info.fMask = SEE_MASK_NOCLOSEPROCESS;
    info.lpVerb = verb.as_ptr();
    info.lpFile = file.as_ptr();
    info.lpParameters = params.as_ptr();
    info.nShow = SW_SHOWNORMAL;
    if unsafe { ShellExecuteExW(&mut info) } == 0 {
        let code = last_error();
        if code == ERROR_CANCELLED {
            term.say(Tone::Warn, "관리자 권한을 받지 못해 멈춥니다. 아무것도 바꾸지 않았습니다.");
            return ExitCode::from(130);
        }
        term.say(Tone::Fail, &format!("관리자 창을 열지 못했습니다: {}", os_error(code)));
        return ExitCode::FAILURE;
    }
    term.say(Tone::Hint, "관리자 창에서 계속합니다. 그 창이 끝나면 여기로 결과를 알려 드립니다.");
    let process = info.hProcess;
    let mut code = 1u32;
    unsafe {
        WaitForSingleObject(process, INFINITE);
        GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
    }
    if code == 0 {
        term.say(Tone::Ok, "관리자 창에서 설치를 마쳤습니다.");
    } else {
        term.say(Tone::Warn, &format!("관리자 창이 끝났습니다(종료 코드 {code})."));
    }
    ExitCode::from(code.min(255) as u8)
}

fn windows_quote(value: &str) -> String {
    if !value.is_empty() && !value.contains([' ', '\t', '"']) {
        return value.into();
    }
    format!("\"{}\"", value.replace('"', "\\\""))
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
struct Sc(SC_HANDLE);
impl Drop for Sc {
    fn drop(&mut self) {
        unsafe { CloseServiceHandle(self.0) };
    }
}
struct Lsa(LSA_HANDLE);
impl Drop for Lsa {
    fn drop(&mut self) {
        unsafe { LsaClose(self.0) };
    }
}
/// ConvertStringSidToSidW가 준 SID(LocalFree)
struct Sid(PSID);
impl Drop for Sid {
    fn drop(&mut self) {
        unsafe { LocalFree(self.0) };
    }
}
fn sid(text: &str) -> Result<Sid> {
    let mut raw = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide(text).as_ptr(), &mut raw) } == 0 {
        bail!("SID를 읽지 못했습니다: {}", os_error(last_error()));
    }
    Ok(Sid(raw))
}

fn token_info(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>> {
    let mut size = 0;
    unsafe { GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut size) };
    if size == 0 || size > 1024 * 1024 {
        bail!("token 정보를 읽지 못했습니다: {}", os_error(last_error()));
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token, class, buffer.as_mut_ptr().cast(), size, &mut size) } == 0 {
        bail!("token 정보를 읽지 못했습니다: {}", os_error(last_error()));
    }
    Ok(buffer)
}
fn token_sid(token: HANDLE) -> Result<String> {
    let user = token_info(token, TokenUser)?;
    let raw = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let mut out = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(raw, &mut out) } == 0 {
        bail!("SID를 글로 바꾸지 못했습니다: {}", os_error(last_error()));
    }
    let value = unsafe { text(out) };
    unsafe { LocalFree(out.cast()) };
    Ok(value)
}
fn own_token() -> Result<Handle> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        bail!("이 프로세스 token: {}", os_error(last_error()));
    }
    Ok(Handle(token))
}

fn manager(access: u32) -> Result<Sc> {
    let handle = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), access) };
    if handle.is_null() {
        let code = last_error();
        if code == ERROR_ACCESS_DENIED {
            bail!("서비스 관리자에 관리자 권한이 필요합니다");
        }
        bail!("서비스 관리자를 열지 못했습니다: {}", os_error(code));
    }
    Ok(Sc(handle))
}
fn open_service(name: &str, access: u32) -> Result<Option<Sc>> {
    let scm = manager(SC_MANAGER_CONNECT)?;
    let handle = unsafe { OpenServiceW(scm.0, wide(name).as_ptr(), access) };
    if !handle.is_null() {
        return Ok(Some(Sc(handle)));
    }
    match last_error() {
        ERROR_SERVICE_DOES_NOT_EXIST => Ok(None),
        code => bail!("서비스 {name}을 열지 못했습니다: {}", os_error(code)),
    }
}
fn query_status(handle: &Sc) -> Result<SERVICE_STATUS_PROCESS> {
    let mut status: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
    let mut needed = 0;
    if unsafe {
        QueryServiceStatusEx(
            handle.0,
            SC_STATUS_PROCESS_INFO,
            (&mut status as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of::<SERVICE_STATUS_PROCESS>() as u32,
            &mut needed,
        )
    } == 0
    {
        bail!("서비스 상태를 읽지 못했습니다: {}", os_error(last_error()));
    }
    Ok(status)
}
fn query_config(handle: &Sc) -> Result<(bool, String, String)> {
    let mut needed = 0;
    unsafe { QueryServiceConfigW(handle.0, std::ptr::null_mut(), 0, &mut needed) };
    if needed == 0 || needed > 1024 * 1024 {
        bail!("서비스 설정을 읽지 못했습니다: {}", os_error(last_error()));
    }
    let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    let config = buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    if unsafe { QueryServiceConfigW(handle.0, config, needed, &mut needed) } == 0 {
        bail!("서비스 설정을 읽지 못했습니다: {}", os_error(last_error()));
    }
    let config = unsafe { &*config };
    Ok((config.dwStartType == SERVICE_AUTO_START, unsafe { text(config.lpBinaryPathName) }, unsafe {
        text(config.lpServiceStartName)
    }))
}
/// 원하는 상태가 될 때까지 기다린다. 다른 끝 상태(STOPPED 등)면 그 상태를 돌려준다
fn wait_for(handle: &Sc, desired: u32, limit: Duration) -> Result<SERVICE_STATUS_PROCESS> {
    let started = Instant::now();
    loop {
        let status = query_status(handle)?;
        let pending = matches!(
            status.dwCurrentState,
            SERVICE_START_PENDING | SERVICE_STOP_PENDING | SERVICE_CONTINUE_PENDING | SERVICE_PAUSE_PENDING
        );
        if status.dwCurrentState == desired || !pending || started.elapsed() >= limit {
            return Ok(status);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn lsa(access: u32) -> Result<Lsa> {
    let mut attributes: LSA_OBJECT_ATTRIBUTES = unsafe { std::mem::zeroed() };
    attributes.Length = std::mem::size_of::<LSA_OBJECT_ATTRIBUTES>() as u32;
    let mut handle: LSA_HANDLE = 0;
    let status = unsafe { LsaOpenPolicy(std::ptr::null(), &attributes, access, &mut handle) };
    if status != 0 {
        let code = unsafe { LsaNtStatusToWinError(status) };
        if code == ERROR_ACCESS_DENIED {
            bail!("로컬 보안 정책에 관리자 권한이 필요합니다");
        }
        bail!("로컬 보안 정책을 열지 못했습니다: {}", os_error(code));
    }
    Ok(Lsa(handle))
}
const SERVICE_LOGON: &str = "SeServiceLogonRight";
const DENY_SERVICE_LOGON: &str = "SeDenyServiceLogonRight";

fn account_rights(policy: &Lsa, sid: &Sid) -> Result<Vec<String>> {
    let mut list = std::ptr::null_mut();
    let mut count = 0;
    let status = unsafe { LsaEnumerateAccountRights(policy.0, sid.0, &mut list, &mut count) };
    if status != 0 {
        let code = unsafe { LsaNtStatusToWinError(status) };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(vec![]);
        }
        bail!("계정 권한을 읽지 못했습니다: {}", os_error(code));
    }
    let mut values = Vec::new();
    for i in 0..count.min(1024) {
        let item = unsafe { &*list.add(i as usize) };
        let units = unsafe { std::slice::from_raw_parts(item.Buffer, item.Length as usize / 2) };
        values.push(String::from_utf16_lossy(units));
    }
    if !list.is_null() {
        unsafe { LsaFreeMemory(list.cast()) };
    }
    Ok(values)
}

fn configure(handle: &Sc, def: &ServiceDef) -> Result<()> {
    let mut description = wide(&def.description);
    let info = SERVICE_DESCRIPTIONW { lpDescription: description.as_mut_ptr() };
    let delayed = SERVICE_DELAYED_AUTO_START_INFO { fDelayedAutostart: 1 };
    // 실패하면 5초·15초·60초 뒤 다시 띄우고 하루 동안 문제가 없으면 횟수를 지운다(Tailscale·windows-service 예와 같은 결)
    let mut actions = [
        SC_ACTION { Type: SC_ACTION_RESTART, Delay: 5_000 },
        SC_ACTION { Type: SC_ACTION_RESTART, Delay: 15_000 },
        SC_ACTION { Type: SC_ACTION_RESTART, Delay: 60_000 },
    ];
    let failure = SERVICE_FAILURE_ACTIONSW {
        dwResetPeriod: 86_400,
        lpRebootMsg: std::ptr::null_mut(),
        lpCommand: std::ptr::null_mut(),
        cActions: actions.len() as u32,
        lpsaActions: actions.as_mut_ptr(),
    };
    let on_error = SERVICE_FAILURE_ACTIONS_FLAG { fFailureActionsOnNonCrashFailures: 1 };
    for (level, data) in [
        (SERVICE_CONFIG_DESCRIPTION, (&info as *const SERVICE_DESCRIPTIONW).cast::<core::ffi::c_void>()),
        (SERVICE_CONFIG_DELAYED_AUTO_START_INFO, (&delayed as *const SERVICE_DELAYED_AUTO_START_INFO).cast()),
        (SERVICE_CONFIG_FAILURE_ACTIONS, (&failure as *const SERVICE_FAILURE_ACTIONSW).cast()),
        (SERVICE_CONFIG_FAILURE_ACTIONS_FLAG, (&on_error as *const SERVICE_FAILURE_ACTIONS_FLAG).cast()),
    ] {
        if unsafe { ChangeServiceConfig2W(handle.0, level, data) } == 0 {
            bail!("서비스 설정({level})을 쓰지 못했습니다: {}", os_error(last_error()));
        }
    }
    Ok(())
}

/// 모든 Win32 서비스 이름
fn all_services() -> Result<Vec<String>> {
    let scm = manager(SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE)?;
    let mut needed = 0;
    let mut count = 0;
    let mut resume = 0;
    unsafe {
        EnumServicesStatusExW(
            scm.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            std::ptr::null_mut(),
            0,
            &mut needed,
            &mut count,
            &mut resume,
            std::ptr::null(),
        )
    };
    if needed == 0 || needed > 16 * 1024 * 1024 {
        bail!("서비스 목록을 읽지 못했습니다: {}", os_error(last_error()));
    }
    // 두 호출 사이에 서비스가 늘 수 있어 여유를 둔다
    let size = needed + 16 * 1024;
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    resume = 0;
    if unsafe {
        EnumServicesStatusExW(
            scm.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            buffer.as_mut_ptr().cast(),
            size,
            &mut needed,
            &mut count,
            &mut resume,
            std::ptr::null(),
        )
    } == 0
    {
        bail!("서비스 목록을 읽지 못했습니다: {}", os_error(last_error()));
    }
    let entries =
        unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>(), count as usize) };
    Ok(entries.iter().map(|e| unsafe { text(e.lpServiceName) }).collect())
}

struct WinSystem;

impl WinSystem {
    fn elevated(&mut self) -> Result<bool> {
        let token = own_token()?;
        let elevation = token_info(token.0, TokenElevation)?;
        Ok(unsafe { (*elevation.as_ptr().cast::<TOKEN_ELEVATION>()).TokenIsElevated != 0 })
    }
}

impl System for WinSystem {
    fn account(&mut self) -> Result<Account> {
        let mut size = 0u32;
        unsafe { GetUserNameExW(NameSamCompatible, std::ptr::null_mut(), &mut size) };
        if size == 0 || size > 4096 {
            bail!("이 계정 이름을 읽지 못했습니다: {}", os_error(last_error()));
        }
        let mut buffer = vec![0u16; size as usize + 1];
        if !unsafe { GetUserNameExW(NameSamCompatible, buffer.as_mut_ptr(), &mut size) } {
            bail!("이 계정 이름을 읽지 못했습니다: {}", os_error(last_error()));
        }
        let sam = String::from_utf16_lossy(&buffer[..size as usize]);
        let token = own_token()?;
        Ok(Account { sam, sid: token_sid(token.0)?, profile: aaa_config::paths::home() })
    }

    fn computer(&mut self) -> String {
        let mut buffer = [0u16; 256];
        let mut size = buffer.len() as u32;
        if unsafe { GetComputerNameExW(ComputerNameDnsHostname, buffer.as_mut_ptr(), &mut size) } == 0 {
            return aaa_common::util::hostname();
        }
        String::from_utf16_lossy(&buffer[..size as usize])
    }

    fn service(&mut self, name: &str) -> Result<Option<ServiceInfo>> {
        let Some(handle) = open_service(name, SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS)? else { return Ok(None) };
        let status = query_status(&handle)?;
        let (automatic, command, account) = query_config(&handle)?;
        Ok(Some(ServiceInfo {
            name: name.into(),
            running: status.dwCurrentState == SERVICE_RUNNING,
            pid: status.dwProcessId,
            automatic,
            account,
            command,
        }))
    }

    fn aaa_services(&mut self) -> Result<Vec<ServiceInfo>> {
        let names: Vec<String> =
            all_services()?.into_iter().filter(|n| n.to_ascii_lowercase().starts_with("aaa")).collect();
        Ok(names
            .iter()
            .filter_map(|n| {
                self.service(n)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .flatten()
            })
            .collect())
    }

    fn reach(&mut self, addr: &str) -> Result<()> {
        use std::net::ToSocketAddrs;
        let targets: Vec<_> = addr.to_socket_addrs().with_context(|| "이름을 찾지 못했습니다")?.collect();
        let mut last = None;
        for target in targets {
            match std::net::TcpStream::connect_timeout(&target, Duration::from_secs(5)) {
                Ok(_) => return Ok(()),
                Err(e) => last = Some(e),
            }
        }
        match last {
            Some(e) => Err(e.into()),
            None => bail!("주소가 없습니다"),
        }
    }

    fn enroll(&mut self, ui: &mut dyn Ui, state: &Path, addr: &str, name: &str) -> Result<()> {
        let ident = aaa_agent::identity::load_or_create(state)?;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        rt.block_on(async {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let request = aaa_agent::link::enroll_with(state, addr, &ident.machine_id, name, move |code, _| {
                aaa_common::diagnostic::ignored(tx.send(code.to_string()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
            });
            tokio::pin!(request);
            let started = Instant::now();
            let mut shown: Option<Instant> = None;
            let mut tick = tokio::time::interval(Duration::from_millis(120));
            loop {
                tokio::select! {
                    result = &mut request => {
                        ui.spin_end();
                        return result.map(|_| ());
                    }
                    Some(code) = rx.recv() => {
                        ui.spin_end();
                        for (tone, line) in code_lines(name, &code) {
                            ui.say(tone, &line);
                        }
                        shown = Some(Instant::now());
                    }
                    _ = tick.tick() => {
                        if ui.cancelled() {
                            ui.spin_end();
                            // request를 버리면 연결이 닫혀 host가 대기 요청을 지운다
                            return Err(Cancelled.into());
                        }
                        match shown {
                            Some(at) => ui.spin("host에서 승인을 기다리는 중 (최대 15분)", at.elapsed()),
                            None => ui.spin("host에 연결하는 중", started.elapsed()),
                        }
                    }
                }
            }
        })
    }

    fn logon(&mut self, account: &Account, secret: &Secret, service: bool) -> Result<Logon> {
        let (domain, user) = account.sam.rsplit_once('\\').unwrap_or((".", account.sam.as_str()));
        let mut token = std::ptr::null_mut();
        let kind = if service { LOGON32_LOGON_SERVICE } else { LOGON32_LOGON_NETWORK };
        if unsafe {
            LogonUserW(
                wide(user).as_ptr(),
                wide(domain).as_ptr(),
                secret.as_ptr(),
                kind,
                LOGON32_PROVIDER_DEFAULT,
                &mut token,
            )
        } == 0
        {
            return Ok(match last_error() {
                ERROR_LOGON_FAILURE => Logon::WrongPassword,
                ERROR_LOGON_TYPE_NOT_GRANTED => Logon::NotGranted,
                ERROR_ACCOUNT_RESTRICTION => Logon::Other("빈 암호이거나 계정 제한이 있습니다(1327)".into()),
                ERROR_PASSWORD_EXPIRED | ERROR_PASSWORD_MUST_CHANGE => {
                    Logon::Other("암호가 만료됐습니다. Windows에서 암호를 바꾼 뒤 다시 하세요".into())
                }
                ERROR_ACCOUNT_LOCKED_OUT => Logon::Other("계정이 잠겼습니다. 잠금이 풀린 뒤 다시 하세요".into()),
                ERROR_ACCOUNT_DISABLED => Logon::Other("계정이 꺼져 있습니다".into()),
                code => Logon::Other(os_error(code)),
            });
        }
        let token = Handle(token);
        if token_sid(token.0)? != account.sid {
            return Ok(Logon::Other("로그온한 계정이 이 계정과 다릅니다".into()));
        }
        Ok(Logon::Ok)
    }

    fn logon_right(&mut self, account: &Account) -> Result<Right> {
        let policy = lsa(POLICY_LOOKUP_NAMES as u32)?;
        let rights = account_rights(&policy, &sid(&account.sid)?)?;
        Ok(if rights.iter().any(|r| r == DENY_SERVICE_LOGON) {
            Right::Denied
        } else if rights.iter().any(|r| r == SERVICE_LOGON) {
            Right::Granted
        } else {
            Right::Missing
        })
    }

    fn account_services(&mut self, account: &Account) -> Result<Vec<String>> {
        let user = account.sam.rsplit('\\').next().unwrap_or(&account.sam).to_ascii_lowercase();
        let mut names = Vec::new();
        for name in all_services()? {
            let Ok(Some(handle)) = open_service(&name, SERVICE_QUERY_CONFIG).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            let Ok((_, _, start_name)) = query_config(&handle).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            if start_name.rsplit('\\').next().is_some_and(|u| u.to_ascii_lowercase() == user) {
                names.push(name);
            }
        }
        Ok(names)
    }

    fn set_logon_right(&mut self, account: &Account, on: bool) -> Result<()> {
        let policy = lsa((POLICY_LOOKUP_NAMES | POLICY_CREATE_ACCOUNT) as u32)?;
        let account_sid = sid(&account.sid)?;
        let mut value = wide(SERVICE_LOGON);
        let right = LSA_UNICODE_STRING {
            Length: ((value.len() - 1) * 2) as u16,
            MaximumLength: (value.len() * 2) as u16,
            Buffer: value.as_mut_ptr(),
        };
        let status = unsafe {
            if on {
                LsaAddAccountRights(policy.0, account_sid.0, &right, 1)
            } else {
                LsaRemoveAccountRights(policy.0, account_sid.0, false, &right, 1)
            }
        };
        if status != 0 {
            bail!("「서비스로 로그온」 권한: {}", os_error(unsafe { LsaNtStatusToWinError(status) }));
        }
        Ok(())
    }

    fn create_service(&mut self, def: &ServiceDef, account: &Account, secret: &Secret) -> Result<()> {
        let scm = manager(SC_MANAGER_CREATE_SERVICE)?;
        let handle = unsafe {
            CreateServiceW(
                scm.0,
                wide(&def.name).as_ptr(),
                wide(&def.display).as_ptr(),
                // 실패 동작의 다시 시작(SC_ACTION_RESTART)을 쓰려면 SERVICE_START가 있어야 한다
                SERVICE_CHANGE_CONFIG | SERVICE_QUERY_STATUS | SERVICE_START | DELETE,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_AUTO_START,
                SERVICE_ERROR_NORMAL,
                wide(&def.command).as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                wide(&account.sam).as_ptr(),
                secret.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(match last_error() {
                ERROR_SERVICE_EXISTS | ERROR_DUPLICATE_SERVICE_NAME => {
                    anyhow::anyhow!("같은 이름의 서비스 {}가 이미 있습니다", def.name)
                }
                ERROR_SERVICE_MARKED_FOR_DELETE => anyhow::anyhow!(
                    "서비스 {}가 지워지는 중입니다. 서비스(services.msc) 창을 닫고 다시 실행하세요",
                    def.name
                ),
                ERROR_INVALID_SERVICE_ACCOUNT => anyhow::anyhow!("서비스 계정 {}를 쓸 수 없습니다", account.sam),
                code => anyhow::anyhow!(os_error(code)),
            });
        }
        let handle = Sc(handle);
        if let Err(e) = configure(&handle, def) {
            unsafe { DeleteService(handle.0) };
            return Err(e);
        }
        Ok(())
    }

    fn start_service(&mut self, name: &str) -> Result<u32> {
        let handle = open_service(name, SERVICE_START | SERVICE_QUERY_STATUS)?
            .with_context(|| format!("서비스 {name}이 없습니다"))?;
        if unsafe { StartServiceW(handle.0, 0, std::ptr::null()) } == 0 {
            match last_error() {
                ERROR_SERVICE_ALREADY_RUNNING => {}
                ERROR_SERVICE_LOGON_FAILED => bail!(
                    "Windows가 이 계정으로 서비스를 띄우지 못했습니다(1069: 암호나 「서비스로 로그온」 권한). \
                     이벤트 뷰어 › Windows 로그 › 시스템의 7038 이벤트에 까닭이 있습니다"
                ),
                code => bail!("{}", os_error(code)),
            }
        }
        let status = wait_for(&handle, SERVICE_RUNNING, Duration::from_secs(60))?;
        if status.dwCurrentState != SERVICE_RUNNING {
            let code = if status.dwWin32ExitCode == ERROR_SERVICE_SPECIFIC_ERROR {
                format!("서비스 코드 {}", status.dwServiceSpecificExitCode)
            } else {
                os_error(status.dwWin32ExitCode)
            };
            bail!("서비스가 시작하자마자 멈췄습니다({code})");
        }
        Ok(status.dwProcessId)
    }

    fn stop_service(&mut self, name: &str) -> Result<()> {
        let Some(handle) = open_service(name, SERVICE_STOP | SERVICE_QUERY_STATUS)? else { return Ok(()) };
        let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        if unsafe { ControlService(handle.0, SERVICE_CONTROL_STOP, &mut status) } == 0 {
            match last_error() {
                ERROR_SERVICE_NOT_ACTIVE => return Ok(()),
                code => bail!("서비스를 멈추지 못했습니다: {}", os_error(code)),
            }
        }
        let status = wait_for(&handle, SERVICE_STOPPED, Duration::from_secs(45))?;
        if status.dwCurrentState != SERVICE_STOPPED {
            bail!("서비스가 45초 안에 멈추지 않았습니다");
        }
        Ok(())
    }

    fn delete_service(&mut self, name: &str) -> Result<()> {
        let Some(handle) = open_service(name, DELETE)? else { return Ok(()) };
        if unsafe { DeleteService(handle.0) } == 0 && last_error() != ERROR_SERVICE_MARKED_FOR_DELETE {
            bail!("서비스를 지우지 못했습니다: {}", os_error(last_error()));
        }
        drop(handle);
        // 다른 프로그램(services.msc 등)이 handle을 쥐고 있으면 그 창을 닫을 때 지워진다
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            if open_service(name, SERVICE_QUERY_STATUS)?.is_none() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        bail!("서비스 {name}이 지움 표시만 됐습니다. 서비스(services.msc) 창을 닫으면 지워집니다")
    }

    fn user_path(&mut self) -> Result<Option<String>> {
        let (key, value) = (wide("Environment"), wide("Path"));
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
        let mut size = 0u32;
        let code = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if code != ERROR_SUCCESS || size > 1024 * 1024 {
            bail!("사용자 PATH를 읽지 못했습니다: {}", os_error(code));
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
        let code = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            bail!("사용자 PATH를 읽지 못했습니다: {}", os_error(code));
        }
        Ok(Some(unsafe { text(buffer.as_ptr()) }))
    }

    fn set_user_path(&mut self, value: Option<&str>) -> Result<()> {
        let (key, name) = (wide("Environment"), wide("Path"));
        let code = match value {
            Some(value) => {
                let data = wide(value);
                unsafe {
                    RegSetKeyValueW(
                        HKEY_CURRENT_USER,
                        key.as_ptr(),
                        name.as_ptr(),
                        REG_EXPAND_SZ,
                        data.as_ptr().cast(),
                        (data.len() * 2) as u32,
                    )
                }
            }
            None => unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) },
        };
        if code != ERROR_SUCCESS && !(value.is_none() && code == ERROR_FILE_NOT_FOUND) {
            bail!("사용자 PATH를 쓰지 못했습니다: {}", os_error(code));
        }
        // 열린 탐색기·새 터미널이 바뀐 PATH를 읽게 알린다(SSH 세션에서는 데스크톱에 닿지 않을 수 있다)
        let environment = wide("Environment");
        let mut result = 0usize;
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                0,
                environment.as_ptr() as LPARAM,
                SMTO_ABORTIFHUNG,
                5000,
                &mut result,
            )
        };
        Ok(())
    }
}

/// `setup-service`가 쓰는 사용자 환경과 프로필(서비스 프로세스 자신의 token)
pub(crate) fn service_environment() -> Result<(Vec<(String, String)>, PathBuf)> {
    let token = own_token()?;
    let mut block = std::ptr::null_mut();
    if unsafe { windows_sys::Win32::System::Environment::CreateEnvironmentBlock(&mut block, token.0, 0) } == 0 {
        bail!("사용자 환경을 만들지 못했습니다: {}", os_error(last_error()));
    }
    let mut values = Vec::new();
    let ptr = block.cast::<u16>();
    let mut offset = 0;
    while offset < 1024 * 1024 && unsafe { *ptr.add(offset) } != 0 {
        let item = unsafe { text(ptr.add(offset)) };
        offset += item.encode_utf16().count() + 1;
        if let Some((key, value)) = item.split_once('=')
            && !key.is_empty()
        {
            values.push((key.to_string(), value.to_string()));
        }
    }
    unsafe { windows_sys::Win32::System::Environment::DestroyEnvironmentBlock(block) };
    let profile = values
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("USERPROFILE"))
        .map(|(_, v)| PathBuf::from(v))
        .context("사용자 환경에 USERPROFILE이 없습니다")?;
    Ok((values, profile))
}
