//! 시험과 함께 컴파일하는 외부 프로세스 수집기. CIM·셸·실행 중 컴파일을 쓰지 않는다.
use super::{ChildSnapshot, ConsoleHost, Frame, HostThread, Measurement, Snapshot, Thread, WaitNode};
#[path = "snapshot_console.rs"]
mod console;
#[path = "console_repro.rs"]
mod repro;
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
        LibraryLoader::*,
        ProcessStatus::GetModuleFileNameExW,
        SystemInformation::{GetSystemDirectoryW, IMAGE_FILE_MACHINE_AMD64},
        Threading::*,
    },
    UI::WindowsAndMessaging::RT_VERSION,
};

#[derive(Debug, serde::Serialize)]
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
impl<T> From<Result<T, Failure>> for Measurement<T> {
    fn from(result: Result<T, Failure>) -> Self {
        match result {
            Ok(value) => Self::Ok { value },
            Err(error) => Self::Error { phase: error.phase.into(), os_code: error.os_code },
        }
    }
}

fn file_version(path: &Path) -> Result<[u16; 4], Failure> {
    let path = wide(path.as_os_str());
    // GetFileVersionInfoW는 manifest 호환성에 따라 실제 10.0을 6.2로 돌려줄 수 있다.
    // 실행·import 초기화 없이 파일의 RT_VERSION data만 매핑한다. 제품 manifest는 바꾸지 않는다.
    let module = unsafe {
        LoadLibraryExW(path.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE)
    };
    if module.is_null() {
        return Err(Failure::os("console_host_version_map"));
    }
    struct ResourceModule(HMODULE);
    impl Drop for ResourceModule {
        fn drop(&mut self) {
            unsafe { FreeLibrary(self.0) };
        }
    }
    let module = ResourceModule(module);
    // Win32의 정수 resource ID다. 포인터로 역참조하지 않는다.
    let resource = unsafe { FindResourceW(module.0, std::ptr::without_provenance(1), RT_VERSION) };
    if resource.is_null() {
        return Err(Failure::os("console_host_version_find"));
    }
    let size = unsafe { SizeofResource(module.0, resource) };
    if size == 0 {
        return Err(Failure::os("console_host_version_size"));
    }
    if size > 1024 * 1024 {
        return Err(Failure::rejected("console_host_version_limit"));
    }
    let data = unsafe { LoadResource(module.0, resource) };
    if data.is_null() {
        return Err(Failure::os("console_host_version_load"));
    }
    let bytes = unsafe { LockResource(data) };
    if bytes.is_null() {
        return Err(Failure::rejected("console_host_version_lock"));
    }
    // SizeofResource가 보증한 범위이며 module은 파싱이 끝난 뒤에만 해제한다.
    version_resource(unsafe { std::slice::from_raw_parts(bytes.cast(), size as usize) })
}

fn version_resource(bytes: &[u8]) -> Result<[u16; 4], Failure> {
    // VS_VERSIONINFO의 key(16 UTF-16 units) 뒤 DWORD 정렬된 VS_FIXEDFILEINFO 52바이트다.
    if bytes.len() < 92 {
        return Err(Failure::rejected("console_host_version_schema"));
    }
    let word = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    let length = usize::from(word(0));
    let key = "VS_VERSION_INFO\0".encode_utf16();
    if length < 92
        || length > bytes.len()
        || word(2) != 52
        || word(4) != 0
        || !key.enumerate().all(|(i, unit)| word(6 + 2 * i) == unit)
    {
        return Err(Failure::rejected("console_host_version_schema"));
    }
    let dword = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if dword(40) != 0xfeef04bd {
        return Err(Failure::rejected("console_host_version_signature"));
    }
    Ok([(dword(48) >> 16) as u16, dword(48) as u16, (dword(52) >> 16) as u16, dword(52) as u16])
}

#[test]
fn version_resource_reads_fixed_numbers_and_rejects_truncation_or_wrong_root() {
    let mut bytes = vec![0u8; 92];
    bytes[..2].copy_from_slice(&92u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&52u16.to_le_bytes());
    for (i, unit) in "VS_VERSION_INFO\0".encode_utf16().enumerate() {
        bytes[6 + 2 * i..8 + 2 * i].copy_from_slice(&unit.to_le_bytes());
    }
    bytes[40..44].copy_from_slice(&0xfeef04bdu32.to_le_bytes());
    bytes[48..52].copy_from_slice(&(10u32 << 16).to_le_bytes());
    bytes[52..56].copy_from_slice(&((26100u32 << 16) | 9549).to_le_bytes());
    assert_eq!(version_resource(&bytes).unwrap(), [10, 0, 26100, 9549]);
    for end in 0..92 {
        assert_eq!(version_resource(&bytes[..end]).unwrap_err().phase, "console_host_version_schema");
    }
    for at in [0, 2, 4, 6, 36, 40] {
        let mut bad = bytes.clone();
        bad[at] ^= 1;
        assert!(version_resource(&bad).is_err(), "잘못된 길이·key·signature를 숫자로 해석하지 않는다");
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
    verify_access(pid, owner, expected, 0)
}
fn verify_access(pid: u32, owner: u32, expected: &Path, extra: u32) -> Result<OwnedHandle, Failure> {
    let process = owned(
        unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE | extra, 0, pid) },
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
    children: &[u32],
    mut phase: impl FnMut(&'static str) -> Result<(), std::io::Error>,
) -> Result<Snapshot, Failure> {
    capture_one(pid, owner, exe, children, &mut phase)
}

// GetConsoleWindow의 message-only HWND는 CLI 자신 소유일 수 있어 호스트 신원으로 쓰지 않는다.
// 커널의 읽기 전용 연관 정보로 PID를 얻고 별도로 부모·시스템 이미지 경로를 확인한다.
fn console_host(process: HANDLE) -> Result<u32, Failure> {
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(
            process: HANDLE,
            class: u32,
            info: *mut usize,
            length: u32,
            returned: *mut u32,
        ) -> i32;
    }
    let mut host = 0usize;
    let mut returned = 0;
    let status = unsafe {
        NtQueryInformationProcess(process, 49, &mut host, std::mem::size_of::<usize>() as u32, &mut returned)
    };
    if status < 0 {
        return Err(Failure { phase: "console_host_query", error_type: "NtStatus", os_code: status as u32 });
    }
    if returned != std::mem::size_of::<usize>() as u32 {
        return Err(Failure::rejected("console_host_query_size"));
    }
    // ProcessConsoleHostProcess의 하위 두 비트는 PID가 아닌 flag다.
    u32::try_from(host & !3).ok().filter(|pid| *pid != 0).ok_or_else(|| Failure::rejected("console_host_missing"))
}

fn console_host_image() -> Result<std::path::PathBuf, Failure> {
    let mut directory = vec![0; 32768];
    let count = unsafe { GetSystemDirectoryW(directory.as_mut_ptr(), directory.len() as u32) };
    if count == 0 || count as usize >= directory.len() {
        return Err(Failure::os("console_host_directory"));
    }
    directory.truncate(count as usize);
    Ok(std::path::PathBuf::from(OsString::from_wide(&directory)).join("conhost.exe"))
}

fn capture_one(
    pid: u32,
    owner: u32,
    exe: &Path,
    children: &[u32],
    phase: &mut impl FnMut(&'static str) -> Result<(), std::io::Error>,
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
    mark("console_input")?;
    // 심볼·스택 조회 전에 큐를 본다. 이 helper는 이미 확정된 실패 뒤에만 수집한다.
    let input = console::capture(process.as_raw_handle(), pid, owner, &mut mark);
    let views = capture_threads(process.as_raw_handle(), pid, exe, phase)?;
    let mut snapshot = Snapshot {
        format: 1,
        process_id: pid,
        threads: views,
        console_host: None,
        console_host_error: String::new(),
        console_host_code: None,
        console_input: None,
        console_input_error: String::new(),
        console_input_code: None,
        children: Vec::new(),
    };
    match input {
        Ok(input) => snapshot.console_input = Some(input),
        Err(error) => {
            snapshot.console_input_error = error.phase.into();
            snapshot.console_input_code = Some(error.os_code);
        }
    }
    phase("console_host").map_err(|error| Failure {
        phase: "write_progress",
        error_type: "IoError",
        os_code: error.raw_os_error().unwrap_or(0) as u32,
    })?;
    match capture_host(process.as_raw_handle(), owner) {
        Ok(host) => snapshot.console_host = Some(host),
        Err(error) => {
            snapshot.console_host_error = error.phase.into();
            snapshot.console_host_code = Some(error.os_code);
        }
    }
    // CLI handle을 보관한 채 같은 이미지의 직계 자식만 별도로 검증한다.
    // worker는 DETACHED_PROCESS라 CLI 콘솔 스택에 나타나지 않는다.
    snapshot.children = capture_children(pid, exe, children, phase);
    Ok(snapshot)
}

fn capture_threads(
    process: HANDLE,
    pid: u32,
    exe: &Path,
    phase: &mut impl FnMut(&'static str) -> Result<(), std::io::Error>,
) -> Result<Vec<Thread>, Failure> {
    let mut mark = |name| {
        phase(name).map_err(|error| Failure {
            phase: "write_progress",
            error_type: "IoError",
            os_code: error.raw_os_error().unwrap_or(0) as u32,
        })
    };
    mark("initialize_symbols")?;
    let search = wide(exe.parent().unwrap_or(Path::new(".")).as_os_str());
    unsafe {
        SymSetOptions(SYMOPT_DEFERRED_LOADS | SYMOPT_UNDNAME | SYMOPT_FAIL_CRITICAL_ERRORS | SYMOPT_NO_PROMPTS);
    }
    if unsafe { SymInitializeW(process, search.as_ptr(), 1) } == 0 {
        return Err(Failure::os("initialize_symbols"));
    }
    let symbols = Symbols(process);
    let threads = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }, "thread_snapshot")?;
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut found = unsafe { Thread32First(threads.as_raw_handle(), &mut entry) };
    let mut views = Vec::new();
    while found != 0 && views.len() < 32 {
        if entry.th32OwnerProcessID == pid {
            // 실제 thread 수집 전후를 남긴다. private 메모리·입력·경로는 기록하지 않는다.
            mark("walk_thread")?;
            views.push(read_thread(process, pid, entry.th32ThreadID));
            mark("thread_resumed")?;
        }
        found = unsafe { Thread32Next(threads.as_raw_handle(), &mut entry) };
    }
    if found == 0 && unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(Failure::os("thread_next"));
    }
    mark("cleanup_symbols")?;
    drop(symbols);
    Ok(views)
}

fn capture_children(
    owner: u32,
    exe: &Path,
    children: &[u32],
    phase: &mut impl FnMut(&'static str) -> Result<(), std::io::Error>,
) -> Vec<ChildSnapshot> {
    children
        .iter()
        .take(4)
        .map(|&pid| {
            let result =
                verify(pid, owner, exe).and_then(|process| capture_threads(process.as_raw_handle(), pid, exe, phase));
            let mut child = ChildSnapshot { process_id: pid, threads: Vec::new(), error: String::new(), os_code: 0 };
            match result {
                Ok(threads) => child.threads = threads,
                Err(error) => {
                    child.error = error.phase.into();
                    child.os_code = error.os_code;
                }
            }
            child
        })
        .collect()
}

// helper가 수집 중 강제 종료되면 SuspendThread의 해제가 실행되지 않을 수 있다.
// 호출자가 검증한 같은 process handle을 보관하고 그 실패 때만 소유 worker를 종료한다.
pub(super) struct ChildGuard {
    pub(super) pid: u32,
    process: OwnedHandle,
}
impl ChildGuard {
    pub(super) fn terminate(&self) {
        if unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } == WAIT_TIMEOUT {
            let ok = unsafe { TerminateProcess(self.process.as_raw_handle(), 1) };
            let os_code = if ok == 0 { unsafe { GetLastError() } } else { 0 };
            let wait = unsafe { WaitForSingleObject(self.process.as_raw_handle(), 5000) };
            eprintln!(
                "PTY snapshot child cleanup: pid={}, terminated={}, os_code={os_code}, wait={wait}",
                self.pid,
                ok != 0
            );
        }
    }
}

#[test]
fn snapshot_cleanup_terminates_the_verified_handle_and_preserves_another_child() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let exe = console_host_image().unwrap().with_file_name("cmd.exe");
    let spawn = || {
        Command::new(&exe)
            .args(["/d", "/q", "/c", "set /p gate="])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    };
    let mut target = spawn();
    let mut other = spawn();
    let process = verify_access(target.id(), std::process::id(), &exe, PROCESS_TERMINATE)
        .unwrap_or_else(|error| panic!("{}", serde_json::to_string(&error).unwrap()));
    let guard = ChildGuard { pid: target.id(), process };
    guard.terminate();
    let status = target.wait().unwrap();
    let other_status = other.try_wait().unwrap();
    other.kill().unwrap();
    other.wait().unwrap();
    assert_eq!(status.code(), Some(1));
    assert!(other_status.is_none(), "다른 자식의 수명은 바꾸지 않는다: {other_status:?}");
}

pub(super) fn owned_children(pid: u32, owner: u32, exe: &Path) -> Result<Vec<ChildGuard>, Failure> {
    let _cli = verify(pid, owner, exe)?;
    let processes = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }, "children_snapshot")?;
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut found = unsafe { Process32FirstW(processes.as_raw_handle(), &mut entry) };
    let mut children = Vec::new();
    while found != 0 && children.len() < 4 {
        let length = entry.szExeFile.iter().position(|unit| *unit == 0).unwrap_or(entry.szExeFile.len());
        let name = OsString::from_wide(&entry.szExeFile[..length]);
        if entry.th32ParentProcessID == pid && Some(name.as_os_str()) == exe.file_name() {
            let child_pid = entry.th32ProcessID;
            let process = verify_access(child_pid, pid, exe, PROCESS_TERMINATE)?;
            children.push(ChildGuard { pid: child_pid, process });
        }
        found = unsafe { Process32NextW(processes.as_raw_handle(), &mut entry) };
    }
    if found == 0 && unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(Failure::os("children_next"));
    }
    Ok(children)
}

// 호스트는 중지하지 않는다. 수집기 5초 상한에 helper가 종료돼도 호스트 thread가 중지된 채 남지 않는다.
fn capture_host(client: HANDLE, owner: u32) -> Result<ConsoleHost, Failure> {
    let pid = console_host(client)?;
    let image = console_host_image()?;
    let host = verify(pid, owner, &image)?;
    let file_version = file_version(&image).into();
    let mut created = FILETIME::default();
    let mut exited = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    if unsafe { GetProcessTimes(host.as_raw_handle(), &mut created, &mut exited, &mut kernel, &mut user) } == 0 {
        return Err(Failure::os("console_host_times"));
    }
    let ticks = |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
    let session = unsafe { OpenThreadWaitChainSession(0, None) };
    let session_error = if session.is_null() { unsafe { GetLastError() } } else { 0 };
    struct Session(*mut core::ffi::c_void);
    impl Drop for Session {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CloseThreadWaitChainSession(self.0) };
            }
        }
    }
    let session = Session(session);
    let threads = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) }, "console_host_threads")?;
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut found = unsafe { Thread32First(threads.as_raw_handle(), &mut entry) };
    let mut views = Vec::new();
    while found != 0 && views.len() < 32 {
        if entry.th32OwnerProcessID == pid {
            let mut nodes = [WAITCHAIN_NODE_INFO::default(); 16];
            let mut count = nodes.len() as u32;
            let mut cycle = 0;
            let ok = !session.0.is_null()
                && unsafe {
                    GetThreadWaitChain(session.0, 0, 0, entry.th32ThreadID, &mut count, nodes.as_mut_ptr(), &mut cycle)
                } != 0;
            let os_code = if session.0.is_null() {
                session_error
            } else if ok {
                0
            } else {
                unsafe { GetLastError() }
            };
            // 오브젝트 이름·경로·다른 프로세스의 식별 정보는 출력하지 않는다.
            let nodes = if ok {
                nodes[..(count as usize).min(nodes.len())]
                    .iter()
                    .map(|node| WaitNode { kind: node.ObjectType, status: node.ObjectStatus })
                    .collect()
            } else {
                Vec::new()
            };
            views.push(HostThread { id: entry.th32ThreadID, nodes, cycle: cycle != 0, os_code });
        }
        found = unsafe { Thread32Next(threads.as_raw_handle(), &mut entry) };
    }
    if found == 0 && unsafe { GetLastError() } != ERROR_NO_MORE_FILES {
        return Err(Failure::os("console_host_thread_next"));
    }
    Ok(ConsoleHost {
        process_id: pid,
        kernel_100ns: ticks(kernel),
        user_100ns: ticks(user),
        threads: views,
        file_version,
    })
}
