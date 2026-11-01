//! 시험과 함께 컴파일하는 외부 프로세스 수집기. CIM·셸·실행 중 컴파일을 쓰지 않는다.
use super::{Frame, Snapshot, Thread};
use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use windows_sys::Win32::{
    Foundation::*,
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    Storage::FileSystem::GetFullPathNameW,
    System::{
        Diagnostics::{Debug::*, ToolHelp::*},
        ProcessStatus::GetModuleFileNameExW,
        SystemInformation::IMAGE_FILE_MACHINE_AMD64,
        Threading::*,
    },
};

#[derive(serde::Serialize)]
pub(super) struct Failure {
    phase: &'static str,
    error_type: &'static str,
    os_code: u32,
}
impl Failure {
    fn rejected(phase: &'static str) -> Self {
        Self { phase, error_type: "Rejected", os_code: 0 }
    }
    fn os(phase: &'static str) -> Self {
        Self { phase, error_type: "Win32Error", os_code: unsafe { GetLastError() } }
    }
}
fn owned(handle: HANDLE, phase: &'static str) -> Result<OwnedHandle, Failure> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        Err(Failure::os(phase))
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
    }
}
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn full_path(path: &OsStr) -> Result<Vec<u16>, Failure> {
    let path = wide(path);
    let mut buffer = vec![0; 32768];
    let count =
        unsafe { GetFullPathNameW(path.as_ptr(), buffer.len() as u32, buffer.as_mut_ptr(), std::ptr::null_mut()) };
    if count == 0 || count as usize >= buffer.len() {
        return Err(Failure::os("normalize_child_image"));
    }
    buffer.truncate(count as usize);
    Ok(buffer)
}

// 같은 handle을 신원 검사부터 수집 완료까지 보관해 PID 재사용으로 대상을 바꾸지 않는다.
fn verify(pid: u32, owner: u32, expected: &Path) -> Result<OwnedHandle, Failure> {
    let process = owned(
        unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE, 0, pid) },
        "open_process",
    )?;
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 0) } {
        WAIT_TIMEOUT => {}
        WAIT_OBJECT_0 => return Err(Failure::rejected("child_exited")),
        _ => return Err(Failure::os("child_wait")),
    }
    let processes = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }, "process_snapshot")?;
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut found = unsafe { Process32FirstW(processes.as_raw_handle(), &mut entry) };
    loop {
        if found == 0 {
            return Err(if unsafe { GetLastError() } == ERROR_NO_MORE_FILES {
                Failure::rejected("child_missing")
            } else {
                Failure::os("process_next")
            });
        }
        if entry.th32ProcessID == pid {
            if entry.th32ParentProcessID != owner {
                return Err(Failure::rejected("child_owner_mismatch"));
            }
            break;
        }
        found = unsafe { Process32NextW(processes.as_raw_handle(), &mut entry) };
    }
    let mut image = vec![0; 32768];
    let mut size = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process.as_raw_handle(), 0, image.as_mut_ptr(), &mut size) } == 0 {
        return Err(Failure::os("child_image_unavailable"));
    }
    image.truncate(size as usize);
    let actual = full_path(&OsString::from_wide(&image))?;
    let expected = full_path(expected.as_os_str())?;
    if unsafe {
        CompareStringOrdinal(actual.as_ptr(), actual.len() as i32, expected.as_ptr(), expected.len() as i32, 1)
    } != CSTR_EQUAL
    {
        return Err(Failure::rejected("child_image_mismatch"));
    }
    Ok(process)
}

struct Symbols(HANDLE);
impl Drop for Symbols {
    fn drop(&mut self) {
        unsafe { SymCleanup(self.0) };
    }
}
struct Suspended(HANDLE);
impl Suspended {
    fn resume(&mut self, view: &mut Thread) {
        if !self.0.is_null() {
            if unsafe { ResumeThread(self.0) } == u32::MAX {
                view.error = "resume_thread".into();
                view.os_code = unsafe { GetLastError() } as i32;
            }
            self.0 = std::ptr::null_mut();
        }
    }
}
impl Drop for Suspended {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { ResumeThread(self.0) };
        }
    }
}
fn clean(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|c| if c.is_ascii_alphanumeric() || "._:$<>[], &()*+-!".contains(c) { c } else { '_' })
        .collect()
}
fn describe(process: HANDLE, address: u64) -> Option<Frame> {
    let module = unsafe { SymGetModuleBase64(process, address) };
    if module == 0 || address < module {
        return None;
    }
    let mut name = vec![0; 32768];
    let size =
        unsafe { GetModuleFileNameExW(process, module as usize as HANDLE, name.as_mut_ptr(), name.len() as u32) };
    let name = OsString::from_wide(&name[..size as usize]);
    #[repr(C)]
    struct Symbol {
        info: SYMBOL_INFO,
        tail: [u8; 512],
    }
    let mut symbol: Symbol = unsafe { std::mem::zeroed() };
    symbol.info.SizeOfStruct = std::mem::size_of::<SYMBOL_INFO>() as u32;
    symbol.info.MaxNameLen = 512;
    let mut displacement = 0;
    let function = if unsafe { SymFromAddr(process, address, &mut displacement, &mut symbol.info) } != 0 {
        let len = (symbol.info.NameLen as usize).min(256);
        // Name와 뒤의 tail은 같은 repr(C) 할당 안에 있다.
        let bytes = unsafe { std::slice::from_raw_parts(symbol.info.Name.as_ptr().cast::<u8>(), len) };
        clean(&String::from_utf8_lossy(bytes), 256)
    } else {
        String::new()
    };
    Some(Frame {
        module: clean(&Path::new(&name).file_name().unwrap_or_default().to_string_lossy(), 80),
        offset: format!("0x{:x}", address - module),
        symbol: function,
    })
}
fn read_thread(process: HANDLE, pid: u32, tid: u32) -> Thread {
    let mut view = Thread { id: tid, frames: Vec::new(), error: String::new(), os_code: 0 };
    let result = (|| -> Result<(), Failure> {
        let thread = owned(
            unsafe { OpenThread(THREAD_QUERY_INFORMATION | THREAD_GET_CONTEXT | THREAD_SUSPEND_RESUME, 0, tid) },
            "open_thread",
        )?;
        if unsafe { GetProcessIdOfThread(thread.as_raw_handle()) } != pid {
            return Err(Failure::rejected("thread_owner"));
        }
        if unsafe { SuspendThread(thread.as_raw_handle()) } == u32::MAX {
            return Err(Failure::os("suspend_thread"));
        }
        let mut suspended = Suspended(thread.as_raw_handle());
        // windows-sys 0.60의 repr(C) CONTEXT는 align(16)을 선언하지 않는다.
        #[repr(C, align(16))]
        struct AlignedContext(CONTEXT);
        let mut aligned = AlignedContext(unsafe { std::mem::zeroed() });
        let context = &mut aligned.0;
        context.ContextFlags = CONTEXT_FULL_AMD64;
        if unsafe { GetThreadContext(thread.as_raw_handle(), context) } == 0 {
            return Err(Failure::os("thread_context"));
        }
        let mut frame: STACKFRAME64 = unsafe { std::mem::zeroed() };
        frame.AddrPC.Offset = context.Rip;
        frame.AddrPC.Mode = AddrModeFlat;
        frame.AddrFrame.Offset = context.Rbp;
        frame.AddrFrame.Mode = AddrModeFlat;
        frame.AddrStack.Offset = context.Rsp;
        frame.AddrStack.Mode = AddrModeFlat;
        if let Some(first) = describe(process, context.Rip) {
            view.frames.push(first);
        }
        let mut last = context.Rip;
        for n in 0..32 {
            if unsafe {
                StackWalk64(
                    IMAGE_FILE_MACHINE_AMD64 as u32,
                    process,
                    thread.as_raw_handle(),
                    &mut frame,
                    (context as *mut CONTEXT).cast(),
                    None,
                    Some(SymFunctionTableAccess64),
                    Some(SymGetModuleBase64),
                    None,
                )
            } == 0
            {
                break;
            }
            let pc = frame.AddrPC.Offset;
            if pc == 0 {
                break;
            }
            if pc == last {
                if n == 0 {
                    continue;
                }
                break;
            }
            last = pc;
            if let Some(next) = describe(process, pc) {
                view.frames.push(next);
            }
        }
        if view.frames.is_empty() {
            view.error = "empty_stack".into();
        }
        suspended.resume(&mut view);
        Ok(())
    })();
    if let Err(error) = result {
        view.error = error.phase.into();
        view.os_code = error.os_code as i32;
    }
    view
}

pub(super) fn capture(
    pid: u32,
    owner: u32,
    exe: &Path,
    mut phase: impl FnMut(&'static str) -> Result<(), std::io::Error>,
) -> Result<Snapshot, Failure> {
    let mut mark = |name| {
        phase(name).map_err(|error| Failure {
            phase: "write_progress",
            error_type: "IoError",
            os_code: error.raw_os_error().unwrap_or(0) as u32,
        })
    };
    mark("verify_child")?;
    let process = verify(pid, owner, exe)?;
    mark("initialize_symbols")?;
    let search = wide(exe.parent().unwrap_or(Path::new(".")).as_os_str());
    unsafe {
        SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_UNDNAME | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS);
    }
    if unsafe { SymInitializeW(process.as_raw_handle(), search.as_ptr(), 1) } == 0 {
        return Err(Failure::os("initialize_symbols"));
    }
    let symbols = Symbols(process.as_raw_handle());
    let threads = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }, "thread_snapshot")?;
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut found = unsafe { Thread32First(threads.as_raw_handle(), &mut entry) };
    let mut views = Vec::new();
    while found != 0 && views.len() < 32 {
        if entry.th32OwnerProcessID == pid {
            // 실제 thread 수집 전후를 남긴다. private 메모리·입력·경로는 기록하지 않는다.
            mark("walk_thread")?;
            views.push(read_thread(process.as_raw_handle(), pid, entry.th32ThreadID));
            mark("thread_resumed")?;
        }
        found = unsafe { Thread32Next(threads.as_raw_handle(), &mut entry) };
    }
    if found == 0 && unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(Failure::os("thread_next"));
    }
    mark("cleanup_symbols")?;
    drop(symbols);
    Ok(Snapshot { format: 1, process_id: pid, threads: views })
}
