//! herdr 0.9.5 `src/platform/windows.rs::write_clipboard`의 창 소유자·UTF-16·메모리 소유권 경로
//! (Apache-2.0). NULL 소유자로 여는 arboard의 읽기 경로와 달리, 쓰기는 console 창을 지정한다.

use windows_sys::Win32::Foundation::GlobalFree;
use windows_sys::Win32::System::Console::GetConsoleWindow;
use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData};
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};

pub(super) fn native_text(text: &str) -> Result<(), String> {
    if text.contains('\0') {
        return Err("NUL 문자가 든 글은 복사할 수 없다".into());
    }
    let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = utf16.len().checked_mul(size_of::<u16>()).ok_or("복사할 글이 너무 길다")?;
    // SAFETY: 유효한 console 창으로 열고 모든 반환 경로에서 닫는다. UTF-16은 NUL로 끝나며,
    // SetClipboardData 성공 뒤에는 OS가 메모리를 소유한다. 실패한 메모리만 직접 해제한다.
    unsafe {
        let owner = GetConsoleWindow();
        if owner.is_null() {
            return Err("clipboard 소유자로 쓸 console 창이 없다".into());
        }
        if OpenClipboard(owner) == 0 {
            return Err(format!("OpenClipboard: {}", std::io::Error::last_os_error()));
        }
        let _clipboard = ClipboardGuard;
        // 메모리를 준비한 뒤 비운다: 할당 실패로 원래 clipboard를 잃지 않는다.
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if memory.is_null() {
            return Err(format!("GlobalAlloc: {}", std::io::Error::last_os_error()));
        }
        let locked = GlobalLock(memory);
        if locked.is_null() {
            let e = std::io::Error::last_os_error();
            GlobalFree(memory);
            return Err(format!("GlobalLock: {e}"));
        }
        std::ptr::copy_nonoverlapping(utf16.as_ptr(), locked.cast::<u16>(), utf16.len());
        GlobalUnlock(memory);
        let result = if EmptyClipboard() == 0 {
            Err(format!("EmptyClipboard: {}", std::io::Error::last_os_error()))
        } else if SetClipboardData(13 /* CF_UNICODETEXT */, memory).is_null() {
            Err(format!("SetClipboardData: {}", std::io::Error::last_os_error()))
        } else {
            Ok(())
        };
        if result.is_err() {
            GlobalFree(memory);
        }
        result
    }
}

struct ClipboardGuard;
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: OpenClipboard 성공 뒤에만 만들며 한 번만 닫는다.
        unsafe { CloseClipboard() };
    }
}
