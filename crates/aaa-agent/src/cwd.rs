//! pane의 지금 작업 폴더(새 탭·나누기의 `new_cwd = "follow"`, plan §8.76).
//!
//! herdr 0.9.5 `src/pane.rs` `follow_cwd`(앞 프로세스 그룹 leader의 cwd, 없으면 셸 cwd)와
//! `src/platform/{macos,linux,windows}.rs`의 `foreground_process_group_id`·`process_cwd`·`pane_process_cwd`를 옮겼다.
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.
//! PTY는 session host만 갖고 있어(옛 session host와 IPC를 바꾸지 않는다) herdr의 pid 경로를 쓴다:
//! child pid의 제어 터미널 앞 그룹(`tpgid`)을 읽는다. herdr가 먼저 보는 OSC 7 보고 위치는 aaa가 따로 모으지 않는다.

use std::path::PathBuf;

/// 셸(child)과 그 앞에서 도는 프로그램 기준의 cwd. 못 읽으면 None(그러면 host는 띄울 때 cwd를 쓴다).
pub fn follow_cwd(child_pid: u32) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        // herdr `usable_process_cwd`: 앞 그룹 leader는 지금 있는 폴더일 때만 믿는다
        foreground_process_group_id(child_pid)
            .and_then(process_cwd)
            .filter(|p| p.is_absolute() && p.is_dir())
            .or_else(|| process_cwd(child_pid).filter(|p| p.is_absolute()))
    }
    #[cfg(windows)]
    {
        win::pane_process_cwd(child_pid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = child_pid;
        None
    }
}

/// 제어 터미널의 앞 프로세스 그룹(macOS `proc_bsdinfo.e_tpgid`, Linux `/proc/<pid>/stat`의 tpgid)
#[cfg(target_os = "macos")]
pub(crate) fn foreground_process_group_id(pid: u32) -> Option<u32> {
    if pid == 0 {
        return None;
    }
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: info는 쓸 수 있는 proc_bsdinfo이고 size가 그 크기다
    let ret = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    (ret == size && info.e_tpgid > 0).then_some(info.e_tpgid)
}

#[cfg(target_os = "macos")]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    if pid == 0 {
        return None;
    }
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: info는 쓸 수 있는 proc_vnodepathinfo이고 size가 그 크기다
    let ret = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if ret != size {
        return None;
    }
    // vip_path는 [[c_char; 32]; 32](MAXPATHLEN 1024칸)
    // SAFETY: 같은 메모리를 바이트로 읽는다(크기 MAXPATHLEN)
    let path =
        unsafe { std::slice::from_raw_parts(info.pvi_cdir.vip_path.as_ptr() as *const u8, libc::MAXPATHLEN as usize) };
    let nul = path.iter().position(|&b| b == 0)?;
    (nul > 0).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&path[..nul])))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub(crate) fn foreground_process_group_id(pid: u32) -> Option<u32> {
    // "pid (comm) state ppid pgrp session tty_nr tpgid ...": comm에 빈칸·괄호가 있을 수 있어 마지막 ')' 뒤부터 센다
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    let rest = stat.get(stat.rfind(')')? + 2..)?;
    let tpgid: i32 = rest
        .split_whitespace()
        .nth(5)?
        .parse()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    (tpgid > 0).then_some(tpgid as u32)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn process_cwd(pid: u32) -> Option<PathBuf> {
    if pid == 0 {
        return None;
    }
    std::fs::read_link(format!("/proc/{pid}/cwd"))
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
}

/// herdr `src/platform/windows.rs` `pane_process_cwd`·`process_cwd_from_handle`: 다른 프로세스의 PEB에서
/// `RTL_USER_PROCESS_PARAMETERS.CurrentDirectory`를 읽는다(WoW64 x86 셸은 32비트 PEB). Scoop shim이면 그 아래 셸 하나.
#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::mem::{MaybeUninit, size_of};
    use std::path::{Path, PathBuf};
    use std::ptr::null_mut;

    use windows_sys::Wdk::System::Threading::{
        NtQueryInformationProcess, ProcessBasicInformation, ProcessWow64Information,
    };
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, STATUS_SUCCESS, UNICODE_STRING};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Kernel::STRING32;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_BASIC_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
        QueryFullProcessImageNameW,
    };

    struct Handle(HANDLE);

    impl Handle {
        fn open(pid: u32) -> Option<Handle> {
            if pid == 0 {
                return None;
            }
            // SAFETY: 실패하면 null을 돌려준다
            let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
            (!h.is_null()).then_some(Handle(h))
        }
    }

    impl Drop for Handle {
        fn drop(&mut self) {
            // SAFETY: open이 연 handle이다
            unsafe { CloseHandle(self.0) };
        }
    }

    pub fn pane_process_cwd(child_pid: u32) -> Option<PathBuf> {
        let process = Handle::open(child_pid)?;
        let exe = PathBuf::from(executable_path(process.0)?);
        // Scoop shim은 띄운 자리에 머문다. 셸이 옮겨 다니므로 shim 아래 셸 하나를 본다
        if exe.with_extension("shim").is_file() {
            return process_cwd(shim_shell_pid(child_pid)?);
        }
        cwd_from_handle(process.0)
    }

    fn process_cwd(pid: u32) -> Option<PathBuf> {
        let process = Handle::open(pid)?;
        cwd_from_handle(process.0)
    }

    fn executable_path(process: HANDLE) -> Option<String> {
        let mut path = vec![0_u16; 32_768];
        let mut len = path.len() as u32;
        // SAFETY: path는 len칸 버퍼다
        if unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len) } == 0 {
            return None;
        }
        String::from_utf16(&path[..len as usize])
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
    }

    /// shim 바로 아래 셸 프로세스가 하나뿐이면 그것(herdr `shim_shell_pid`)
    fn shim_shell_pid(child_pid: u32) -> Option<u32> {
        // SAFETY: 실패하면 INVALID_HANDLE_VALUE
        let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let snap = Handle(snap);
        let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut shells = Vec::new();
        // SAFETY: entry.dwSize를 채웠다
        let mut ok = unsafe { Process32FirstW(snap.0, &mut entry) } != 0;
        while ok {
            if entry.th32ParentProcessID == child_pid {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if is_shell(&name) {
                    shells.push(entry.th32ProcessID);
                }
            }
            // SAFETY: 같은 snapshot과 entry
            ok = unsafe { Process32NextW(snap.0, &mut entry) } != 0;
        }
        (shells.len() == 1).then(|| shells[0])
    }

    /// herdr `is_pane_shell_process_name`
    fn is_shell(name: &str) -> bool {
        let n = Path::new(name).file_name().and_then(|s| s.to_str()).unwrap_or(name);
        let n = n.trim_start_matches('-').trim_end_matches(".exe").to_ascii_lowercase();
        matches!(
            n.as_str(),
            "sh" | "bash"
                | "dash"
                | "zsh"
                | "fish"
                | "ksh"
                | "mksh"
                | "csh"
                | "tcsh"
                | "elvish"
                | "xonsh"
                | "nu"
                | "pwsh"
                | "powershell"
                | "cmd"
        )
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Peb {
        reserved1: [u8; 2],
        being_debugged: u8,
        reserved2: [u8; 1],
        reserved3: [*mut c_void; 2],
        ldr: *mut c_void,
        process_parameters: *mut Parameters,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CurDir {
        dos_path: UNICODE_STRING,
        handle: HANDLE,
    }

    /// RTL_USER_PROCESS_PARAMETERS 앞부분(CurrentDirectory까지)
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Parameters {
        maximum_length: u32,
        length: u32,
        flags: u32,
        debug_flags: u32,
        console_handle: HANDLE,
        console_flags: u32,
        standard_input: HANDLE,
        standard_output: HANDLE,
        standard_error: HANDLE,
        current_directory: CurDir,
    }

    // x86 PEB·RTL_USER_PROCESS_PARAMETERS 앞부분: 64비트에서 읽을 때 원격 포인터는 32비트다
    #[cfg(target_pointer_width = "64")]
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Peb32 {
        reserved: [u32; 4],
        process_parameters: u32,
    }

    #[cfg(target_pointer_width = "64")]
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Parameters32 {
        reserved: [u32; 9],
        current_directory: STRING32,
    }

    fn cwd_from_handle(process: HANDLE) -> Option<PathBuf> {
        read_unicode_string(process, cwd_descriptor(process)?).map(PathBuf::from).filter(|p| p.is_absolute())
    }

    fn cwd_descriptor(process: HANDLE) -> Option<UNICODE_STRING> {
        #[cfg(target_pointer_width = "64")]
        {
            let mut peb32 = 0_usize;
            // SAFETY: 출력은 ULONG_PTR 크기의 쓸 수 있는 칸이다
            let status = unsafe {
                NtQueryInformationProcess(
                    process,
                    ProcessWow64Information,
                    (&mut peb32 as *mut usize).cast(),
                    size_of::<usize>() as u32,
                    null_mut(),
                )
            };
            if status != STATUS_SUCCESS {
                return None;
            }
            if peb32 != 0 {
                // WoW64 셸은 native PEB에 C:\Windows가 보일 수 있다: 그 프로세스 폭의 PEB를 읽는다
                let peb = read_value::<Peb32>(process, peb32 as *const c_void)?;
                let params = read_value::<Parameters32>(process, peb.process_parameters as usize as *const c_void)?;
                let cwd = params.current_directory;
                return Some(UNICODE_STRING {
                    Length: cwd.Length,
                    MaximumLength: cwd.MaximumLength,
                    Buffer: cwd.Buffer as usize as *mut u16,
                });
            }
        }
        let mut basic = MaybeUninit::<PROCESS_BASIC_INFORMATION>::uninit();
        // SAFETY: 출력은 PROCESS_BASIC_INFORMATION 크기다
        let status = unsafe {
            NtQueryInformationProcess(
                process,
                ProcessBasicInformation,
                basic.as_mut_ptr().cast::<c_void>(),
                size_of::<PROCESS_BASIC_INFORMATION>() as u32,
                null_mut(),
            )
        };
        if status != STATUS_SUCCESS {
            return None;
        }
        // SAFETY: 성공하면 채워져 있다
        let basic = unsafe { basic.assume_init() };
        if basic.PebBaseAddress.is_null() {
            return None;
        }
        let peb = read_value::<Peb>(process, basic.PebBaseAddress.cast::<c_void>())?;
        if peb.process_parameters.is_null() {
            return None;
        }
        Some(read_value::<Parameters>(process, peb.process_parameters.cast())?.current_directory.dos_path)
    }

    fn read_value<T: Copy>(process: HANDLE, address: *const c_void) -> Option<T> {
        if address.is_null() {
            return None;
        }
        let mut value = MaybeUninit::<T>::uninit();
        let mut read = 0;
        // SAFETY: value는 T 크기의 쓸 수 있는 칸이다
        let ok = unsafe {
            ReadProcessMemory(process, address, value.as_mut_ptr().cast::<c_void>(), size_of::<T>(), &mut read)
        } != 0;
        // SAFETY: T 크기만큼 다 읽었다
        (ok && read == size_of::<T>()).then(|| unsafe { value.assume_init() })
    }

    fn read_unicode_string(process: HANDLE, s: UNICODE_STRING) -> Option<String> {
        if s.Buffer.is_null() || s.Length == 0 || !s.Length.is_multiple_of(2) {
            return None;
        }
        let mut buf = vec![0_u16; usize::from(s.Length / 2)];
        let mut read = 0;
        // SAFETY: buf는 s.Length 바이트다
        let ok = unsafe {
            ReadProcessMemory(
                process,
                s.Buffer.cast::<c_void>(),
                buf.as_mut_ptr().cast::<c_void>(),
                usize::from(s.Length),
                &mut read,
            )
        } != 0;
        if !ok || read != usize::from(s.Length) {
            return None;
        }
        String::from_utf16(&buf)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    /// pane처럼 PTY 안에서 셸을 띄워 cd한 뒤 위치를 읽는다(셸이 제어 터미널의 앞 그룹이다).
    /// 신호를 보내지 않는다: 줄을 보내면 셸이 `read`를 마치고 스스로 끝난다.
    #[test]
    fn follows_the_shells_current_directory() {
        let dir = std::env::temp_dir().join(format!("aaa-cwd-{}", std::process::id()));
        let deep = dir.join("깊은 폴더");
        std::fs::create_dir_all(&deep).unwrap();
        let pty = portable_pty::native_pty_system();
        let pair = pty.openpty(portable_pty::PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).unwrap();
        #[cfg(unix)]
        let cmd = {
            let mut c = portable_pty::CommandBuilder::new("/bin/sh");
            c.args(["-c", "cd \"$1\" && echo ready && read _", "sh"]);
            c.arg(&deep);
            c
        };
        #[cfg(windows)]
        let cmd = {
            // cmd가 cd한 뒤 표지를 쓰고 줄을 기다린다. 경로는 환경 변수로 넘기고 따옴표를 쓰지 않는다: portable-pty는
            // 공백·따옴표가 든 인자를 `"…"`로 감싸며 안의 `"`를 `\"`로 바꾸는데(CommandLineToArgvW 규칙), cmd.exe는 `\"`를
            // 모른다(CI ccbabcb: 「The filename, directory name, or volume label syntax is incorrect.」). cmd의 cd는 공백을
            // 구분자로 보지 않는다. `&&` 앞에 공백을 두지 않아 경로 끝에 공백이 붙지 않게 한다.
            let mut c = portable_pty::CommandBuilder::new("cmd");
            c.args(["/d", "/q", "/c", "cd /d %AAA_CWD_TEST%&& echo ready&& set /p _="]);
            c.env("AAA_CWD_TEST", &deep);
            c
        };
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        // 출력은 끝까지 읽는다(session host처럼): 아무도 안 읽으면 남은 출력 때문에 셸이 끝나지 못한다(macOS).
        // 닫을 때도 EOF까지 받는다: ConPTY는 닫기(ClosePseudoConsole) 중에도 출력을 쓰므로 받는 쪽이 먼저 멈추면 안 된다.
        let mut out = pair.master.try_clone_reader().unwrap();
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            while let Ok(n @ 1..) = out.read(&mut buf) {
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut writer = pair.master.take_writer().unwrap();
        // 단계마다 안전 상한(30초)을 둔다. 기다림의 끝은 사건(표지·종료·EOF)이고, 상한은 멈췄을 때 어느 단계에서 무엇을
        // 받았는지 남기고 실패하려는 것이다(windows CI는 이 시험에서 끝없이 기다려 job 한도로 취소됐다).
        let limit = std::time::Duration::from_secs(30);
        let shown = |b: &[u8]| {
            let s = String::from_utf8_lossy(b).escape_debug().to_string();
            s.chars().rev().take(1500).collect::<Vec<_>>().into_iter().rev().collect::<String>()
        };
        let within = |what: &str, f: Box<dyn FnOnce() + Send>| {
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                f();
                let _ = done_tx.send(());
            });
            match done_rx.recv_timeout(limit) {
                Ok(()) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    panic!("{what}: {}초 안에 끝나지 않았다", limit.as_secs())
                }
                Err(_) => panic!("{what}: 도중에 실패했다(위 panic)"),
            }
        };
        // 표지(ready)가 보이면 cd가 끝났다. Windows ConPTY(portable-pty는 INHERIT_CURSOR로 연다)는 시작할 때 커서 위치를
        // 묻고(DSR 6, `ESC [ 6 n`) 답이 올 때까지 자식의 출력과 종료를 막는다: session host처럼 한 번 답한다
        // (aaa-vt `QueryScanner`). Unix 셸은 묻지 않는다.
        let mut seen = Vec::new();
        let mut answered = false;
        let until = std::time::Instant::now() + limit;
        while !String::from_utf8_lossy(&seen).contains("ready") {
            let left = until.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(left) {
                Ok(chunk) => seen.extend_from_slice(&chunk),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    panic!("표지(ready)가 {}초 안에 오지 않았다(DSR 답 {answered}): {}", limit.as_secs(), shown(&seen))
                }
                Err(_) => panic!("셸이 표지 전에 끝났다: {}", shown(&seen)),
            }
            if !answered && seen.windows(4).any(|w| w == b"\x1b[6n") {
                std::io::Write::write_all(&mut writer, b"\x1b[1;1R").unwrap();
                answered = true;
            }
        }
        let pid = child.process_id().unwrap();
        let got = follow_cwd(pid);
        std::io::Write::write_all(&mut writer, b"\r\n").unwrap();
        within("입력 줄을 받은 셸의 종료", Box::new(move || drop(child.wait().unwrap())));
        drop(writer);
        within("PTY 닫기", Box::new(move || drop(pair.master)));
        within("출력 EOF", Box::new(move || reader.join().unwrap()));
        drop(rx);
        let want = deep.canonicalize().unwrap();
        assert_eq!(got.map(|p| p.canonicalize().unwrap()), Some(want));
        std::fs::remove_dir_all(&dir).unwrap();
        // 없는 pid
        assert_eq!(follow_cwd(0), None);
    }
}
