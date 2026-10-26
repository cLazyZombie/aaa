//! 실제 OS 객체의 DACL을 Authz로 판정한다. 계정·정책·권한을 바꾸지 않으며 그룹 조회도 하지 않는다.
use std::{fs::File, os::windows::io::AsRawHandle, path::Path};
use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, HANDLE, LUID, LocalFree},
    Security::{
        Authorization::*, DACL_SECURITY_INFORMATION, GROUP_SECURITY_INFORMATION, GetSecurityDescriptorControl,
        GetSecurityDescriptorDacl, GetSecurityDescriptorLength, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, SE_DACL_PROTECTED,
    },
    Storage::FileSystem::{
        FILE_APPEND_DATA, FILE_FLAG_BACKUP_SEMANTICS, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_DATA,
        FILE_WRITE_DATA, WRITE_DAC, WRITE_OWNER,
    },
};

struct Allocation(*mut core::ffi::c_void);
impl Drop for Allocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct Manager(AUTHZ_RESOURCE_MANAGER_HANDLE);
impl Drop for Manager {
    fn drop(&mut self) {
        unsafe {
            AuthzFreeResourceManager(self.0);
        }
    }
}
struct Context(AUTHZ_CLIENT_CONTEXT_HANDLE);
impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            AuthzFreeContext(self.0);
        }
    }
}
fn descriptor(handle: HANDLE) -> Allocation {
    let mut sd = std::ptr::null_mut();
    assert_eq!(
        unsafe {
            GetSecurityInfo(
                handle,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut sd,
            )
        },
        0
    );
    Allocation(sd)
}
fn access(sd: &Allocation, sid: &str, desired: u32) -> bool {
    let text: Vec<_> = sid.encode_utf16().chain(Some(0)).collect();
    let mut raw_sid = std::ptr::null_mut();
    assert_ne!(unsafe { ConvertStringSidToSidW(text.as_ptr(), &mut raw_sid) }, 0);
    let sid = Allocation(raw_sid);
    let mut manager = Manager(std::ptr::null_mut());
    assert_ne!(
        unsafe {
            AuthzInitializeResourceManager(AUTHZ_RM_FLAG_NO_AUDIT, None, None, None, std::ptr::null(), &mut manager.0)
        },
        0
    );
    let mut context = Context(std::ptr::null_mut());
    assert_ne!(
        unsafe {
            AuthzInitializeContextFromSid(
                AUTHZ_SKIP_TOKEN_GROUPS,
                sid.0,
                manager.0,
                std::ptr::null(),
                LUID::default(),
                std::ptr::null(),
                &mut context.0,
            )
        },
        0
    );
    let request = AUTHZ_ACCESS_REQUEST { DesiredAccess: desired, ..Default::default() };
    let (mut granted, mut error) = (0, 0);
    let mut reply = AUTHZ_ACCESS_REPLY {
        ResultListLength: 1,
        GrantedAccessMask: &mut granted,
        Error: &mut error,
        ..Default::default()
    };
    assert_ne!(
        unsafe {
            AuthzAccessCheck(
                0,
                context.0,
                &request,
                std::ptr::null_mut(),
                sd.0,
                std::ptr::null(),
                0,
                &mut reply,
                std::ptr::null_mut(),
            )
        },
        0
    );
    if error == ERROR_ACCESS_DENIED {
        return false;
    }
    assert_eq!(error, 0);
    granted & desired == desired
}

pub(crate) fn assert_private(handle: HANDLE, system: bool) {
    let sd = descriptor(handle);
    let mut control = 0;
    let mut revision = 0;
    assert_ne!(unsafe { GetSecurityDescriptorControl(sd.0, &mut control, &mut revision) }, 0);
    assert_ne!(control & SE_DACL_PROTECTED, 0, "상위 폴더의 공개 ACE를 물려받지 않는다");
    let (mut present, mut defaulted) = (0, 0);
    let mut dacl = std::ptr::null_mut();
    assert_ne!(unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) }, 0);
    assert_ne!(present, 0);
    assert!(!dacl.is_null());
    assert_eq!(unsafe { (*dacl).AceCount }, if system { 2 } else { 1 });
    assert!(access(&sd, &crate::private_file::account().unwrap(), FILE_GENERIC_READ | FILE_GENERIC_WRITE));
    assert_eq!(access(&sd, "S-1-5-18", FILE_GENERIC_READ | FILE_GENERIC_WRITE), system, "SYSTEM 허용 여부");
    for sid in ["S-1-1-0", "S-1-5-11", "S-1-5-32-545", "S-1-5-32-544", "S-1-5-21-101-202-303-1001"] {
        // 묶은 권한만 검사하면 read-only 누출도 전체 거부로 잘못 통과하므로 권한별로 판정한다.
        for desired in [FILE_READ_DATA, FILE_WRITE_DATA, FILE_APPEND_DATA, WRITE_DAC, WRITE_OWNER] {
            assert!(!access(&sd, sid, desired), "다른 사용자·그룹의 읽기·쓰기·권한 변경 거부");
        }
    }
}

fn open(path: &Path, directory: bool) -> File {
    use std::os::windows::fs::OpenOptionsExt;
    File::options().read(true).custom_flags(if directory { FILE_FLAG_BACKUP_SEMANTICS } else { 0 }).open(path).unwrap()
}
fn snapshot(path: &Path) -> Vec<u8> {
    let sd = descriptor(open(path, true).as_raw_handle());
    unsafe { std::slice::from_raw_parts(sd.0.cast::<u8>(), GetSecurityDescriptorLength(sd.0) as usize).to_vec() }
}
/// 시험 소유 객체만 넓혀 기존 판의 ACL을 재현한다.
fn public_acl(path: &Path, directory: bool) {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::{
        Security::GetSecurityDescriptorDacl,
        Storage::FileSystem::{READ_CONTROL, WRITE_DAC},
    };
    let file = File::options()
        .access_mode(READ_CONTROL | WRITE_DAC)
        .custom_flags(if directory { FILE_FLAG_BACKUP_SEMANTICS } else { 0 })
        .open(path)
        .unwrap();
    let text: Vec<_> = "D:P(A;;FA;;;WD)".encode_utf16().chain(Some(0)).collect();
    let mut sd = std::ptr::null_mut();
    assert_ne!(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(text.as_ptr(), 1, &mut sd, std::ptr::null_mut())
        },
        0
    );
    let sd = Allocation(sd);
    let (mut present, mut defaulted) = (0, 0);
    let mut dacl = std::ptr::null_mut();
    assert_ne!(unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut dacl, &mut defaulted) }, 0);
    assert_eq!(
        unsafe {
            SetSecurityInfo(
                file.as_raw_handle(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null_mut(),
            )
        },
        0
    );
    assert!(
        access(&descriptor(file.as_raw_handle()), "S-1-1-0", FILE_GENERIC_READ | FILE_GENERIC_WRITE),
        "공개 fixture인지 확인"
    );
}

struct Lab(std::path::PathBuf);
impl Lab {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("aaa-acl-{}", crate::util::new_id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Lab {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn state_and_tls_are_private_on_creation_and_legacy_reload() {
    let lab = Lab::new();
    let ancestor = snapshot(&lab.0);
    // 기존 표준/승격 token의 기본 owner로 만든 폴더도 현재 사용자 소유로 좁힌다.
    let legacy = lab.0.join("legacy");
    std::fs::create_dir(&legacy).unwrap();
    crate::util::ensure_private_dir(&legacy).unwrap();
    crate::private_file::verify_directory(&legacy).unwrap();
    assert_private(open(&legacy, true).as_raw_handle(), true);
    let state = lab.0.join("새 폴더").join("state");
    let id = crate::tls::load_or_create(&state, "aaa-agent").unwrap();
    let objects = [
        (state.clone(), true),
        (state.join("tls"), true),
        (state.join("tls/key.der"), false),
        (state.join("tls/cert.der"), false),
    ];
    for (path, directory) in &objects {
        assert_private(open(path, *directory).as_raw_handle(), true);
    }
    // 기존 키를 다시 읽는 경로도 ACL을 좁히고 같은 신원을 보존해야 한다.
    for (path, directory) in &objects {
        public_acl(path, *directory);
    }
    let loaded = crate::tls::load_or_create(&state, "aaa-agent").unwrap();
    assert_eq!(id.key_der, loaded.key_der);
    assert_eq!(id.cert_der, loaded.cert_der);
    for (path, directory) in &objects {
        assert_private(open(path, *directory).as_raw_handle(), true);
    }
    assert_eq!(snapshot(&lab.0), ancestor, "기존 상위 폴더의 ACL은 바꾸지 않는다");

    let record = state.join("record.json");
    crate::util::write_private(&record, b"first").unwrap();
    assert_private(open(&record, false).as_raw_handle(), true);
    public_acl(&record, false);
    crate::util::write_private(&record, b"second").unwrap();
    assert_eq!(crate::util::read_private(&record).unwrap(), b"second");
    assert_private(open(&record, false).as_raw_handle(), true);
}

#[test]
fn private_acl_recheck_avoids_security_writes_but_repairs_external_changes() {
    use windows_sys::Win32::Storage::FileSystem::{FILE_BASIC_INFO, FileBasicInfo, GetFileInformationByHandleEx};
    fn changed_at(path: &Path, directory: bool) -> i64 {
        let mut info: FILE_BASIC_INFO = unsafe { std::mem::zeroed() };
        assert_ne!(
            unsafe {
                GetFileInformationByHandleEx(
                    open(path, directory).as_raw_handle(),
                    FileBasicInfo,
                    (&mut info as *mut FILE_BASIC_INFO).cast(),
                    std::mem::size_of::<FILE_BASIC_INFO>() as u32,
                )
            },
            0
        );
        info.ChangeTime
    }
    let lab = Lab::new();
    let dir = lab.0.join("private");
    crate::util::ensure_private_dir(&dir).unwrap();
    let file = dir.join("record");
    crate::util::write_private(&file, b"unchanged").unwrap();
    for (path, directory) in [(&dir, true), (&file, false)] {
        let before = changed_at(path, directory);
        for _ in 0..3 {
            drop(crate::private_file::protect(path, directory).unwrap());
        }
        assert_eq!(changed_at(path, directory), before, "같은 보안 descriptor를 다시 쓰지 않는다");
        public_acl(path, directory);
        drop(crate::private_file::protect(path, directory).unwrap());
        assert_private(open(path, directory).as_raw_handle(), true);
    }
    assert_eq!(std::fs::read(file).unwrap(), b"unchanged");
}

#[test]
fn inherited_thread_journal_directory_is_normalized_without_changing_parent() {
    let lab = Lab::new();
    let state = lab.0.join("state");
    crate::util::ensure_private_dir(&state).unwrap();
    let before = snapshot(&state);
    // journal::write는 기존 private state 아래에 std::fs로 폴더를 만든다.
    // DACL은 상속해도 승격·서비스 token의 기본 owner는 Administrators일 수 있다.
    let journal = state.join("thread-preparations");
    std::fs::create_dir(&journal).unwrap();
    for _ in 0..2 {
        crate::util::ensure_private_dir(&journal).unwrap();
        crate::private_file::verify_directory(&journal).unwrap();
        assert_private(open(&journal, true).as_raw_handle(), true);
    }
    crate::util::write_private(&journal.join("thread.json"), b"ready").unwrap();
    assert_eq!(crate::util::read_private(&journal.join("thread.json")).unwrap(), b"ready");
    assert_eq!(snapshot(&state), before, "기존 state의 소유권·ACL은 유지한다");
}

#[test]
fn secret_hard_links_and_wrong_kinds_are_rejected_without_rewriting() {
    let lab = Lab::new();
    let state = lab.0.join("state");
    let id = crate::tls::load_or_create(&state, "aaa-agent").unwrap();
    let key = state.join("tls/key.der");
    let alias = lab.0.join("alias");
    std::fs::hard_link(&key, &alias).unwrap();
    assert!(crate::tls::load_or_create(&state, "aaa-agent").is_err());
    assert!(crate::util::write_private(&key, b"replacement").is_err());
    assert_eq!(std::fs::read(&alias).unwrap(), id.key_der);
    assert!(crate::util::ensure_private_dir(&key).is_err());
    assert!(crate::util::read_private(&state).is_err());
}
