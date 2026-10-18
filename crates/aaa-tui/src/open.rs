//! pane 링크를 TUI가 도는 기기의 기본 브라우저로 연다(herdr `platform::open_url`: macOS `open`, Linux `xdg-open`, Windows
//! `ShellExecuteW` "open"). 원격 머신 pane의 링크도 host가 찾은 주소를 이 기기에서 연다.
//! 시험용 `AAA_TEST_OPEN_URL=<파일>`: 브라우저 대신 그 파일 끝에 주소를 한 줄씩 붙인다.

/// http·https만 연다(herdr `safe_web_url`: host가 걸러도 여는 쪽에서 다시 본다)
pub fn safe_web_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

pub fn open_url(url: &str) -> Result<(), String> {
    if !safe_web_url(url) {
        return Err(format!("http·https 주소만 연다: {url}"));
    }
    if let Some(p) = std::env::var_os("AAA_TEST_OPEN_URL") {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p).map_err(|e| e.to_string())?;
        return writeln!(f, "{url}").map_err(|e| e.to_string());
    }
    native(url)
}

#[cfg(unix)]
fn native(url: &str) -> Result<(), String> {
    use std::process::{Command, Stdio};
    let program = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let mut child = Command::new(program)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    // herdr는 연 프로세스를 거둔다(`detached_process_children`): 좀비로 남기지 않는다
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(windows)]
fn native(url: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (op, url) = (wide("open"), wide(url));
    // SAFETY: 두 글은 NUL로 끝나는 UTF-16이고 부르는 동안 산다. 창 handle·인자·폴더는 없어도 된다.
    let r = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if r as isize > 32 { Ok(()) } else { Err(format!("ShellExecuteW 실패: code {}", r as isize)) }
}
