//! prefix 뒤 명령 키를 영문 입력기로 받는다(macOS Text Input Sources, `[tui] prefix_ascii`).
//!
//! 지금 입력기가 영문 자판 배치가 아니면(한글 두벌식 등 입력기) 영문 배치(ABC 등 사용자의 영문 입력기)를 고르고,
//! [`Ascii`]가 사라질 때(명령 키 처리·취소·시간 초과·TUI 종료·panic) 원래 입력기로 되돌린다.
//! 지속 설정이 아니라 지금 입력기만 바꾼다. ssh 세션에서는 바꾸지 않는다(그 Mac의 입력기가 아니다).

/// 영문 입력기로 바꿔 둔 동안의 표. 없앨 때 원래 입력기로 되돌린다.
pub struct Ascii {
    #[cfg(target_os = "macos")]
    prev: mac::Source,
}

impl Ascii {
    /// 지금 입력기가 영문 자판 배치가 아니면 영문 배치로 바꾸고 되돌릴 표를 준다. 이미 영문이거나 바꿀 수 없으면 None.
    pub fn switch() -> Option<Ascii> {
        if remote_session() {
            return None;
        }
        Self::switch_here()
    }

    /// ssh 여부를 보지 않고 이 Mac의 입력기를 바꾼다
    fn switch_here() -> Option<Ascii> {
        #[cfg(target_os = "macos")]
        {
            mac::switch().map(|prev| Ascii { prev })
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }
}

impl Drop for Ascii {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        mac::select(&self.prev);
    }
}

/// ssh로 들어온 셸: 입력기는 앞에 앉은 기기의 것이라 이 Mac에서 바꾸지 않는다
fn remote_session() -> bool {
    ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"].iter().any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()))
}

/// 지금 입력기 ID(시험·진단용). macOS 밖이면 None.
pub fn current_id() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        mac::current().and_then(|s| s.id())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// ID로 입력기를 고른다(시험용). 고르지 못하면 false.
pub fn select_id(id: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        mac::find(id).is_some_and(|s| mac::select(&s))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = id;
        false
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use std::ffi::{CStr, c_char, c_void};

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFDictionaryRef = *const c_void;
    type CFIndex = isize;

    #[link(name = "Carbon", kind = "framework")]
    unsafe extern "C" {
        static kTISPropertyInputSourceIsASCIICapable: CFStringRef;
        static kTISPropertyInputSourceID: CFStringRef;
        static kTISPropertyInputSourceType: CFStringRef;
        static kTISTypeKeyboardLayout: CFStringRef;
        fn TISCopyCurrentKeyboardInputSource() -> CFTypeRef;
        fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> CFTypeRef;
        fn TISGetInputSourceProperty(source: CFTypeRef, key: CFStringRef) -> *const c_void;
        fn TISSelectInputSource(source: CFTypeRef) -> i32;
        fn TISCreateInputSourceList(properties: CFDictionaryRef, include_all: u8) -> CFArrayRef;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
        fn CFRetain(cf: CFTypeRef) -> CFTypeRef;
        fn CFBooleanGetValue(b: *const c_void) -> u8;
        fn CFStringGetCStringPtr(s: CFStringRef, encoding: u32) -> *const c_char;
        fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: CFIndex, encoding: u32) -> u8;
        fn CFArrayGetCount(a: CFArrayRef) -> CFIndex;
        fn CFArrayGetValueAtIndex(a: CFArrayRef, i: CFIndex) -> *const c_void;
    }

    const UTF8: u32 = 0x0800_0100;

    /// 입력기 하나(CF 소유권을 가진다)
    pub struct Source(CFTypeRef);

    impl Drop for Source {
        fn drop(&mut self) {
            // SAFETY: Copy·Create·Retain으로 받은 소유권 하나를 놓는다
            unsafe { CFRelease(self.0) }
        }
    }

    impl Source {
        fn owned(p: CFTypeRef) -> Option<Source> {
            (!p.is_null()).then_some(Source(p))
        }

        /// 영문 글자를 그대로 내는 자판 배치(ABC·Dvorak 등)인가. 한글·일본어 같은 입력기(input mode)는
        /// macOS가 ASCII 가능으로 보고하지만 조합 중 글자를 붙잡으므로 아니다.
        fn plain_ascii_layout(&self) -> bool {
            // SAFETY: 살아 있는 입력기, 값은 CFBoolean·CFString(Get이라 소유권 없음)
            unsafe {
                let ascii = TISGetInputSourceProperty(self.0, kTISPropertyInputSourceIsASCIICapable);
                let kind = TISGetInputSourceProperty(self.0, kTISPropertyInputSourceType);
                !ascii.is_null()
                    && CFBooleanGetValue(ascii) != 0
                    && !kind.is_null()
                    && CFEqual(kind, kTISTypeKeyboardLayout) != 0
            }
        }

        pub fn id(&self) -> Option<String> {
            // SAFETY: 값은 CFString(Get이라 소유권 없음). 포인터가 없으면 버퍼로 옮긴다
            unsafe {
                let s = TISGetInputSourceProperty(self.0, kTISPropertyInputSourceID);
                if s.is_null() {
                    return None;
                }
                let p = CFStringGetCStringPtr(s, UTF8);
                if !p.is_null() {
                    return Some(CStr::from_ptr(p).to_string_lossy().into_owned());
                }
                let mut buf = [0 as c_char; 256];
                (CFStringGetCString(s, buf.as_mut_ptr(), buf.len() as CFIndex, UTF8) != 0)
                    .then(|| CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
            }
        }
    }

    pub fn current() -> Option<Source> {
        // SAFETY: Copy 함수라 소유권을 받는다
        Source::owned(unsafe { TISCopyCurrentKeyboardInputSource() })
    }

    pub fn select(s: &Source) -> bool {
        // SAFETY: 살아 있는 입력기
        unsafe { TISSelectInputSource(s.0) == 0 }
    }

    /// 영문 자판 배치가 아니면(한글 등 입력기) 영문 배치로 바꾸고 원래 것을 돌려준다
    pub fn switch() -> Option<Source> {
        let cur = current()?;
        if cur.plain_ascii_layout() {
            return None;
        }
        // SAFETY: Copy 함수라 소유권을 받는다
        let ascii = Source::owned(unsafe { TISCopyCurrentASCIICapableKeyboardLayoutInputSource() })?;
        select(&ascii).then_some(cur)
    }

    /// 진단(시험용): 지금 입력기와 영문 배치
    #[cfg(test)]
    pub fn describe() -> String {
        let cur = current();
        // SAFETY: Copy 함수라 소유권을 받는다
        let ascii = Source::owned(unsafe { TISCopyCurrentASCIICapableKeyboardLayoutInputSource() });
        format!(
            "지금 {:?} plain_ascii={:?} · 영문 배치 {:?}",
            cur.as_ref().and_then(|c| c.id()),
            cur.as_ref().map(|c| c.plain_ascii_layout()),
            ascii.as_ref().and_then(|a| a.id())
        )
    }

    /// 켜 둔 입력기 가운데 ID가 같은 것
    pub fn find(id: &str) -> Option<Source> {
        // SAFETY: Create 함수라 목록 소유권을 받고, 고른 항목은 Retain해 따로 갖는다
        unsafe {
            let list = TISCreateInputSourceList(std::ptr::null(), 0);
            if list.is_null() {
                return None;
            }
            let mut found = None;
            for i in 0..CFArrayGetCount(list) {
                let s = Source(CFRetain(CFArrayGetValueAtIndex(list, i)));
                if s.id().as_deref() == Some(id) {
                    found = Some(s);
                    break;
                }
            }
            CFRelease(list);
            found
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn remote_session_is_detected() {
        // 이 판정이 참이면 switch는 아무것도 바꾸지 않는다(ssh로 들어온 셸)
        let ssh = ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
            .iter()
            .any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty()));
        assert_eq!(remote_session(), ssh);
        if ssh {
            assert!(Ascii::switch().is_none());
        }
    }

    /// 실제 입력기를 바꾼다: 사용자 화면을 방해하지 않게 `AAA_IME_NATIVE=1`일 때만, 짧게 돈다.
    /// 한글 두벌식을 잠깐 골라 → prefix처럼 영문으로 바꾸고 → 되돌리면 한글 → 처음 입력기로 돌려 둔다.
    #[test]
    fn switch_and_restore_native() {
        if std::env::var_os("AAA_IME_NATIVE").is_none() {
            // 통과로 보이지 않게 남긴다(clip.rs native_clipboard_roundtrip과 같다, t-0327 넘김 (다))
            eprintln!("건너뜀: AAA_IME_NATIVE=1일 때만 이 Mac의 입력기를 바꿔 본다");
            return;
        }
        let first = current_id().expect("지금 입력기");
        // 단언이 실패해도 처음 입력기로 돌려 둔다
        struct Back(String);
        impl Drop for Back {
            fn drop(&mut self) {
                select_id(&self.0);
            }
        }
        let _back = Back(first.clone());
        let korean = "com.apple.inputmethod.Korean.2SetKorean";
        assert!(select_id(korean), "두벌식 입력기가 켜져 있지 않다");
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(current_id().as_deref(), Some(korean));
        // 이 시험은 ssh 셸에서도 돌 수 있어 ssh 판정은 건너뛴다(그 판정은 remote_session_is_detected)
        let guard = Ascii::switch_here().unwrap_or_else(|| panic!("영문으로 바꾸지 않았다: {}", mac::describe()));
        std::thread::sleep(std::time::Duration::from_millis(100));
        let ascii = current_id().unwrap();
        assert_ne!(ascii, korean);
        drop(guard);
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert_eq!(current_id().as_deref(), Some(korean), "되돌리지 않았다");
        // 이미 영문이면 바꾸지 않는다
        assert!(select_id(&ascii));
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(Ascii::switch_here().is_none());
        eprintln!("입력기: 처음 {first} → 두벌식 → prefix 영문 {ascii} → 되돌림 두벌식 → 처음으로");
    }
}
