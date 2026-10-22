//! 임시 Windows 진단: 종료하기 전에 직접 소유한 PTY 자식 사슬만 읽는다.
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
        },
        Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

pub(super) fn log_owned_tree(root: u32) {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        eprintln!("PTY process snapshot: error={:?}", std::io::Error::last_os_error().raw_os_error());
        return;
    }
    let mut entry = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut rows = Vec::new();
    let mut next = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while next {
        let end = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
        let image = String::from_utf16_lossy(&entry.szExeFile[..end]);
        rows.push((entry.th32ProcessID, entry.th32ParentProcessID, image.eq_ignore_ascii_case("aaa.exe")));
        next = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    unsafe { CloseHandle(snapshot) };
    let mut owned = std::collections::BTreeSet::from([root]);
    for _ in 0..64 {
        let before = owned.len();
        for (pid, parent, _) in &rows {
            if owned.contains(parent) {
                owned.insert(*pid);
            }
        }
        if before == owned.len() {
            break;
        }
    }
    for (pid, parent, aaa) in rows.into_iter().filter(|(pid, _, _)| owned.contains(pid)) {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if handle.is_null() {
            eprintln!(
                "PTY owned process: pid={pid}, parent={parent}, aaa={aaa}, error={:?}",
                std::io::Error::last_os_error().raw_os_error()
            );
            continue;
        }
        let (mut created, mut exited, mut kernel, mut user) =
            (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        let mut code = 0;
        let code_ok = unsafe { GetExitCodeProcess(handle, &mut code) } != 0;
        let times_ok = unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) } != 0;
        unsafe { CloseHandle(handle) };
        let ticks = |time: FILETIME| ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
        eprintln!(
            "PTY owned process: pid={pid}, parent={parent}, aaa={aaa}, code={:?}, cpu_100ns={:?}",
            code_ok.then_some(code),
            times_ok.then_some(ticks(kernel) + ticks(user)),
        );
    }
}
