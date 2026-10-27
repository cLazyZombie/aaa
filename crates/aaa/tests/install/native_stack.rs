//! 격리 실험 전용. 직접 만든 자식의 함수 이름만 보존하며 dump·입력·환경은 쓰지 않는다.
use std::fmt::Write as _;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::{
    Foundation::*,
    System::{
        Diagnostics::{Debug::*, ToolHelp::*},
        Threading::*,
    },
};

pub fn capture(child: &dyn portable_pty::Child) -> String {
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _serial = SERIAL.lock().unwrap();
    let mut out = String::new();
    let Some(pid) = child.process_id() else { return "owned child PID missing".into() };
    let Some(process) = child.as_raw_handle() else { return "owned child handle missing".into() };
    unsafe {
        SymSetOptions(
            SYMOPT_DEFERRED_LOADS
                | SYMOPT_UNDNAME
                | SYMOPT_FAIL_CRITICAL_ERRORS
                | SYMOPT_NO_PROMPTS
                | SYMOPT_IGNORE_NT_SYMPATH,
        );
        let mut image = vec![0u16; 32768];
        let mut length = image.len() as u32;
        if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, image.as_mut_ptr(), &mut length) == 0 {
            return format!("owned image query: {}", std::io::Error::last_os_error());
        }
        image.truncate(length as usize);
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let image = std::path::PathBuf::from(std::ffi::OsString::from_wide(&image));
        let Some(parent) = image.parent() else { return "owned image parent missing".into() };
        let search: Vec<u16> = parent.as_os_str().encode_wide().chain([0]).collect();
        if SymInitializeW(process, search.as_ptr(), 1) == 0 {
            return format!("SymInitialize: {}", std::io::Error::last_os_error());
        }
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            SymCleanup(process);
            return format!("thread snapshot: {}", std::io::Error::last_os_error());
        }
        let snap = OwnedHandle::from_raw_handle(snap);
        let mut entry = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut more = Thread32First(snap.as_raw_handle(), &mut entry);
        while more != 0 {
            if entry.th32OwnerProcessID == pid {
                let _ = writeln!(out, "thread {}", entry.th32ThreadID);
                let raw = OpenThread(
                    THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION,
                    0,
                    entry.th32ThreadID,
                );
                if raw.is_null() {
                    let _ = writeln!(out, "OpenThread: {}", std::io::Error::last_os_error());
                } else {
                    let thread = OwnedHandle::from_raw_handle(raw);
                    // Thread ID 재사용 때에도 다른 프로세스의 thread를 멈추지 않는다.
                    if GetProcessIdOfThread(raw) != pid || WaitForSingleObject(process, 0) != WAIT_TIMEOUT {
                        let _ = writeln!(out, "owned thread ended before capture");
                        more = Thread32Next(snap.as_raw_handle(), &mut entry);
                        continue;
                    }
                    if SuspendThread(raw) == u32::MAX {
                        let _ = writeln!(out, "SuspendThread: {}", std::io::Error::last_os_error());
                    } else {
                        #[repr(align(16))]
                        struct Aligned(CONTEXT);
                        let mut context = Aligned(CONTEXT { ContextFlags: CONTEXT_FULL_AMD64, ..Default::default() });
                        if GetThreadContext(raw, &mut context.0) == 0 {
                            let _ = writeln!(out, "GetThreadContext: {}", std::io::Error::last_os_error());
                        } else {
                            let mut frame = STACKFRAME64::default();
                            frame.AddrPC =
                                ADDRESS64 { Offset: context.0.Rip, Mode: AddrModeFlat, ..Default::default() };
                            frame.AddrStack =
                                ADDRESS64 { Offset: context.0.Rsp, Mode: AddrModeFlat, ..Default::default() };
                            frame.AddrFrame =
                                ADDRESS64 { Offset: context.0.Rbp, Mode: AddrModeFlat, ..Default::default() };
                            for _ in 0..48 {
                                if StackWalk64(
                                    0x8664,
                                    process,
                                    raw,
                                    &mut frame,
                                    (&mut context.0 as *mut CONTEXT).cast(),
                                    None,
                                    Some(SymFunctionTableAccess64),
                                    Some(SymGetModuleBase64),
                                    None,
                                ) == 0
                                    || frame.AddrPC.Offset == 0
                                {
                                    break;
                                }
                                let mut symbol = SYMBOL_INFO_PACKAGE::default();
                                symbol.si.SizeOfStruct = std::mem::size_of::<SYMBOL_INFO>() as u32;
                                symbol.si.MaxNameLen = 2000;
                                let mut offset = 0;
                                if SymFromAddr(process, frame.AddrPC.Offset, &mut offset, &mut symbol.si) != 0 {
                                    let name = std::slice::from_raw_parts(
                                        symbol.si.Name.as_ptr().cast::<u8>(),
                                        symbol.si.NameLen.min(1999) as usize,
                                    );
                                    let _ = writeln!(out, "  {}+{offset}", String::from_utf8_lossy(name));
                                } else {
                                    let _ = writeln!(out, "  unresolved");
                                }
                            }
                        }
                        // 모든 정상/오류 경로에서 직접 소유한 자식 thread만 다시 진행시킨다.
                        if ResumeThread(raw) == u32::MAX {
                            let _ = writeln!(out, "ResumeThread: {}", std::io::Error::last_os_error());
                        }
                    }
                    drop(thread);
                }
            }
            more = Thread32Next(snap.as_raw_handle(), &mut entry);
        }
        SymCleanup(process);
    }
    out
}
