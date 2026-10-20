//! SCM/LSA는 이 고정 adapter에서만 변경한다. raw 오류/계정/암호를 Fault에 복사하지 않는다.
use super::*;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authentication::Identity::*, Authorization::*, *},
    Storage::FileSystem::DELETE,
    System::{Services::*, Threading::*},
    UI::Shell::GetUserProfileDirectoryW,
};

pub(super) struct Kernel(pub HANDLE);
impl Drop for Kernel {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Service(SC_HANDLE);
impl Drop for Service {
    fn drop(&mut self) {
        unsafe {
            CloseServiceHandle(self.0);
        }
    }
}
struct Policy(LSA_HANDLE);
impl Drop for Policy {
    fn drop(&mut self) {
        unsafe {
            LsaClose(self.0);
        }
    }
}
pub(super) fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn w(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn denied() -> Fault {
    failure("service_policy_unverified", 3)
}
fn error(code: u32) -> Fault {
    match code {
        ERROR_ACCESS_DENIED | ERROR_PRIVILEGE_NOT_HELD | ERROR_ELEVATION_REQUIRED => {
            failure("administrator_approval_required", 3)
        }
        ERROR_LOGON_FAILURE
        | ERROR_SERVICE_LOGON_FAILED
        | ERROR_INVALID_SERVICE_ACCOUNT
        | ERROR_ACCOUNT_RESTRICTION
        | ERROR_PASSWORD_EXPIRED
        | ERROR_ACCOUNT_DISABLED
        | ERROR_INVALID_PASSWORD
        | ERROR_ACCOUNT_LOCKED_OUT => failure("service_account_auth_unavailable", 3),
        ERROR_LOGON_TYPE_NOT_GRANTED => failure("service_logon_denied", 3),
        ERROR_SERVICE_EXISTS | ERROR_DUPLICATE_SERVICE_NAME => Fault::conflict(),
        _ => failure("service_operation_failed", 1),
    }
}
fn last() -> Fault {
    error(unsafe { GetLastError() })
}
fn manager(access: u32) -> Result<Service> {
    let handle = unsafe { OpenSCManagerW(std::ptr::null(), std::ptr::null(), access) };
    if handle.is_null() { Err(last()) } else { Ok(Service(handle)) }
}
fn open(spec: &Spec, access: u32) -> Result<Option<Service>> {
    let scm = manager(SC_MANAGER_CONNECT)?;
    let handle = unsafe { OpenServiceW(scm.0, w(&spec.name()).as_ptr(), access) };
    if !handle.is_null() {
        return Ok(Some(Service(handle)));
    }
    let code = unsafe { GetLastError() };
    if code == ERROR_SERVICE_DOES_NOT_EXIST { Ok(None) } else { Err(error(code)) }
}
unsafe fn text(ptr: *const u16) -> Result<String> {
    if ptr.is_null() {
        return Err(Fault::conflict());
    }
    for n in 0..32768 {
        if unsafe { *ptr.add(n) } == 0 {
            return String::from_utf16(unsafe { std::slice::from_raw_parts(ptr, n) }).map_err(|_| Fault::conflict());
        }
    }
    Err(Fault::conflict())
}
unsafe fn sid_text(sid: PSID) -> Result<String> {
    let mut value = std::ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(sid, &mut value) } == 0 {
        return Err(last());
    }
    let result = unsafe { text(value) };
    unsafe {
        LocalFree(value.cast());
    }
    result
}
fn account_sid(name: &str) -> Result<String> {
    let mut sid_len = 0;
    let mut domain_len = 0;
    let mut use_ = 0;
    unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            w(name).as_ptr(),
            std::ptr::null_mut(),
            &mut sid_len,
            std::ptr::null_mut(),
            &mut domain_len,
            &mut use_,
        );
    }
    if sid_len == 0 || sid_len > 65536 || domain_len > 32768 {
        return Err(denied());
    }
    let mut sid = vec![0usize; (sid_len as usize).div_ceil(std::mem::size_of::<usize>())];
    let mut domain = vec![0u16; domain_len as usize];
    if unsafe {
        LookupAccountNameW(
            std::ptr::null(),
            w(name).as_ptr(),
            sid.as_mut_ptr().cast(),
            &mut sid_len,
            domain.as_mut_ptr(),
            &mut domain_len,
            &mut use_,
        )
    } == 0
    {
        return Err(last());
    }
    if use_ != SidTypeUser {
        return Err(denied());
    }
    unsafe { sid_text(sid.as_mut_ptr().cast()) }
}
fn account_name(sid: &str) -> Result<String> {
    let mut raw = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(w(sid).as_ptr(), &mut raw) } == 0 {
        return Err(denied());
    }
    let result = (|| {
        let mut name_len = 0;
        let mut domain_len = 0;
        let mut use_ = 0;
        unsafe {
            LookupAccountSidW(
                std::ptr::null(),
                raw,
                std::ptr::null_mut(),
                &mut name_len,
                std::ptr::null_mut(),
                &mut domain_len,
                &mut use_,
            );
        }
        if name_len == 0 || name_len > 32768 || domain_len > 32768 {
            return Err(denied());
        }
        let mut name = vec![0u16; name_len as usize];
        let mut domain = vec![0u16; domain_len as usize];
        if unsafe {
            LookupAccountSidW(
                std::ptr::null(),
                raw,
                name.as_mut_ptr(),
                &mut name_len,
                domain.as_mut_ptr(),
                &mut domain_len,
                &mut use_,
            )
        } == 0
            || use_ != SidTypeUser
        {
            return Err(denied());
        }
        let name = unsafe { text(name.as_ptr()) }?;
        let domain = unsafe { text(domain.as_ptr()) }?;
        let result = format!("{domain}\\{name}");
        if account_sid(&result)? != sid {
            return Err(denied());
        }
        Ok(result)
    })();
    unsafe {
        LocalFree(raw);
    }
    result
}
fn token_information(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> Result<Vec<usize>> {
    let mut size = 0;
    unsafe {
        GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut size);
    }
    if size == 0 || size > 1024 * 1024 {
        return Err(denied());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    if unsafe { GetTokenInformation(token, class, buffer.as_mut_ptr().cast(), size, &mut size) } == 0 {
        return Err(last());
    }
    Ok(buffer)
}
pub(super) struct Identity {
    pub sid: String,
    pub profile: PathBuf,
    pub session: u32,
    pub elevated: bool,
}
pub(super) fn identity(pid: u32) -> Result<Identity> {
    let process = Kernel(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
    if process.0.is_null() {
        return Err(last());
    }
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(process.0, TOKEN_QUERY, &mut token) } == 0 {
        return Err(last());
    }
    let token = Kernel(token);
    let user = token_information(token.0, TokenUser)?;
    let sid = unsafe { sid_text((*user.as_ptr().cast::<TOKEN_USER>()).User.Sid) }?;
    let mut len = 32768;
    let mut profile = vec![0u16; len as usize];
    if unsafe { GetUserProfileDirectoryW(token.0, profile.as_mut_ptr(), &mut len) } == 0 {
        return Err(last());
    }
    let profile = PathBuf::from(unsafe { text(profile.as_ptr()) }?);
    let session = token_information(token.0, TokenSessionId)?;
    let elevation = token_information(token.0, TokenElevation)?;
    Ok(Identity {
        sid,
        profile,
        session: unsafe { *session.as_ptr().cast::<u32>() },
        elevated: unsafe { (*elevation.as_ptr().cast::<TOKEN_ELEVATION>()).TokenIsElevated != 0 },
    })
}
fn policy(access: i32) -> Result<Policy> {
    let mut attributes: LSA_OBJECT_ATTRIBUTES = unsafe { std::mem::zeroed() };
    attributes.Length = std::mem::size_of::<LSA_OBJECT_ATTRIBUTES>() as u32;
    let mut handle: LSA_HANDLE = 0;
    let status = unsafe { LsaOpenPolicy(std::ptr::null(), &attributes, access as u32, &mut handle) };
    if status != 0 {
        return Err(error(unsafe { LsaNtStatusToWinError(status) }));
    }
    Ok(Policy(handle))
}
fn rights(policy: &Policy, sid: PSID) -> Result<Vec<String>> {
    let mut list = std::ptr::null_mut();
    let mut count = 0;
    let status = unsafe { LsaEnumerateAccountRights(policy.0, sid, &mut list, &mut count) };
    if status != 0 {
        let code = unsafe { LsaNtStatusToWinError(status) };
        return if code == ERROR_FILE_NOT_FOUND { Ok(vec![]) } else { Err(denied()) };
    }
    let result = (|| {
        if count > 1024 || list.is_null() && count != 0 {
            return Err(denied());
        }
        let mut values = Vec::new();
        for i in 0..count {
            let item = unsafe { &*list.add(i as usize) };
            if item.Length % 2 != 0 || item.Length > 32768 {
                return Err(denied());
            }
            values.push(
                String::from_utf16(unsafe { std::slice::from_raw_parts(item.Buffer, item.Length as usize / 2) })
                    .map_err(|_| denied())?,
            );
        }
        Ok(values)
    })();
    if !list.is_null() {
        unsafe {
            LsaFreeMemory(list.cast());
        }
    }
    result
}

/// 네트워크 logon token으로 요청자의 실제 SID/그룹을 확인한다. Deny를 만나면 LSA를 쓰지 않는다.
/// Network logon 자체가 정책으로 금지되면 needs_user다. 그 결과를 서비스 logon 통과라 하지 않는다.
fn logon_right(spec: &Spec, name: &str, password: &Password) -> Result<()> {
    let mut token = std::ptr::null_mut();
    let (domain, user) = name.rsplit_once('\\').ok_or_else(denied)?;
    if unsafe {
        LogonUserW(
            w(user).as_ptr(),
            w(domain).as_ptr(),
            password.0.as_ptr(),
            LOGON32_LOGON_NETWORK,
            LOGON32_PROVIDER_DEFAULT,
            &mut token,
        )
    } == 0
    {
        return Err(last());
    }
    let token = Kernel(token);
    let user = token_information(token.0, TokenUser)?;
    let user_sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    if unsafe { sid_text(user_sid) }? != spec.account {
        return Err(denied());
    }
    let groups = token_information(token.0, TokenGroups)?;
    let groups = unsafe { &*groups.as_ptr().cast::<TOKEN_GROUPS>() };
    if groups.GroupCount > 4096 {
        return Err(denied());
    }
    let groups = unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize) };
    let read = policy(POLICY_LOOKUP_NAMES)?;
    let mut allow = false;
    for sid in std::iter::once(user_sid).chain(groups.iter().map(|g| g.Sid)) {
        for right in rights(&read, sid)? {
            if right == "SeDenyServiceLogonRight" {
                return Err(failure("service_logon_denied", 3));
            }
            allow |= right == "SeServiceLogonRight";
        }
    }
    if !allow {
        let write = policy(POLICY_LOOKUP_NAMES | POLICY_CREATE_ACCOUNT)?;
        let mut value = w("SeServiceLogonRight");
        let right = LSA_UNICODE_STRING {
            Length: ((value.len() - 1) * 2) as u16,
            MaximumLength: (value.len() * 2) as u16,
            Buffer: value.as_mut_ptr(),
        };
        let status = unsafe { LsaAddAccountRights(write.0, user_sid, &right, 1) };
        if status != 0 {
            return Err(error(unsafe { LsaNtStatusToWinError(status) }));
        }
    }
    Ok(())
}
fn configuration(handle: &Service) -> Result<Configuration> {
    let mut size = 0;
    unsafe {
        QueryServiceConfigW(handle.0, std::ptr::null_mut(), 0, &mut size);
    }
    if size == 0 || size > 128 * 1024 {
        return Err(last());
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
    let ptr = buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    if unsafe { QueryServiceConfigW(handle.0, ptr, size, &mut size) } == 0 {
        return Err(last());
    }
    let value = unsafe { &*ptr };
    if value.dwStartType != SERVICE_AUTO_START && value.dwStartType != SERVICE_DEMAND_START {
        return Err(Fault::conflict());
    }
    Ok(Configuration {
        account: account_sid(&unsafe { text(value.lpServiceStartName) }?)?,
        command: unsafe { text(value.lpBinaryPathName) }?,
        automatic: value.dwStartType == SERVICE_AUTO_START,
        own_process: value.dwServiceType == SERVICE_WIN32_OWN_PROCESS,
    })
}
fn status(handle: &Service) -> Result<SERVICE_STATUS_PROCESS> {
    let mut value: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
    let mut needed = 0;
    if unsafe {
        QueryServiceStatusEx(
            handle.0,
            SC_STATUS_PROCESS_INFO,
            (&mut value as *mut SERVICE_STATUS_PROCESS).cast(),
            std::mem::size_of_val(&value) as u32,
            &mut needed,
        )
    } == 0
    {
        return Err(last());
    }
    Ok(value)
}
fn wait_state(handle: &Service, desired: u32) -> Result<SERVICE_STATUS_PROCESS> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let event = Kernel(unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) });
    if event.0.is_null() {
        return Err(last());
    }
    // 조건 기반 OS 대기다. 테스트를 정한 시간 재우거나 시한을 늘리지 않는다.
    loop {
        let value = status(handle)?;
        if value.dwCurrentState == desired {
            return Ok(value);
        }
        if desired == SERVICE_RUNNING && value.dwCurrentState == SERVICE_STOPPED {
            return Err(
                if value.dwWin32ExitCode == ERROR_SERVICE_SPECIFIC_ERROR && value.dwServiceSpecificExitCode == 3 {
                    failure("service_backend_needs_user", 3)
                } else {
                    error(value.dwWin32ExitCode)
                },
            );
        }
        if std::time::Instant::now() >= deadline {
            return Err(failure("service_transition_pending", 3));
        }
        unsafe {
            WaitForSingleObject(event.0, 25);
        }
    }
}
pub(super) struct Scm;
impl Manager for Scm {
    fn query(&mut self, spec: &Spec) -> Result<Option<Observation>> {
        let Some(handle) = open(spec, SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS)? else { return Ok(None) };
        let value = status(&handle)?;
        Ok(Some(Observation {
            configuration: configuration(&handle)?,
            running: value.dwCurrentState == SERVICE_RUNNING,
            pid: value.dwProcessId,
        }))
    }
    fn create(&mut self, spec: &Spec, password: &Password) -> Result<()> {
        spec.validate()?;
        let name = account_name(&spec.account)?;
        let manager = manager(SC_MANAGER_CREATE_SERVICE)?;
        logon_right(spec, &name, password)?;
        let handle = unsafe {
            CreateServiceW(
                manager.0,
                w(&spec.name()).as_ptr(),
                w(&spec.name()).as_ptr(),
                SERVICE_QUERY_CONFIG | SERVICE_QUERY_STATUS,
                SERVICE_WIN32_OWN_PROCESS,
                if spec.automatic { SERVICE_AUTO_START } else { SERVICE_DEMAND_START },
                SERVICE_ERROR_NORMAL,
                w(&spec.command()).as_ptr(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                w(&name).as_ptr(),
                password.0.as_ptr(),
            )
        };
        if handle.is_null() {
            return Err(last());
        }
        drop(Service(handle));
        Ok(())
    }
    fn start(&mut self, spec: &Spec) -> Result<Observation> {
        let handle =
            open(spec, SERVICE_START | SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)?.ok_or_else(Fault::conflict)?;
        if configuration(&handle)? != Configuration::desired(spec) {
            return Err(Fault::conflict());
        }
        if status(&handle)?.dwCurrentState != SERVICE_RUNNING
            && unsafe { StartServiceW(handle.0, 0, std::ptr::null()) } == 0
        {
            let code = unsafe { GetLastError() };
            if code != ERROR_SERVICE_ALREADY_RUNNING {
                return Err(error(code));
            }
        }
        let value = wait_state(&handle, SERVICE_RUNNING)?;
        Ok(Observation { configuration: configuration(&handle)?, running: true, pid: value.dwProcessId })
    }
    fn remove(&mut self, spec: &Spec) -> Result<()> {
        let Some(handle) = open(spec, SERVICE_STOP | SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG | DELETE)? else {
            return Ok(());
        };
        if configuration(&handle)? != Configuration::desired(spec) {
            return Err(Fault::conflict());
        }
        if status(&handle)?.dwCurrentState != SERVICE_STOPPED {
            let mut value: SERVICE_STATUS = unsafe { std::mem::zeroed() };
            if unsafe { ControlService(handle.0, SERVICE_CONTROL_STOP, &mut value) } == 0 {
                return Err(last());
            }
            wait_state(&handle, SERVICE_STOPPED)?;
        }
        if unsafe { DeleteService(handle.0) } == 0 {
            return Err(last());
        }
        // handle을 닫아 SCM 삭제가 완료되도록 한다. LSA 권한은 다른 서비스가 쓸 수 있어 제거하지 않는다.
        Ok(())
    }
    fn credentials(&mut self, spec: &Spec, password: &Password) -> Result<()> {
        let handle = open(spec, SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG)?.ok_or_else(Fault::conflict)?;
        if configuration(&handle)? != Configuration::desired(spec) {
            return Err(Fault::conflict());
        }
        if unsafe {
            ChangeServiceConfigW(
                handle.0,
                SERVICE_NO_CHANGE,
                SERVICE_NO_CHANGE,
                SERVICE_NO_CHANGE,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
                password.0.as_ptr(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(last());
        }
        Ok(())
    }
    fn process_matches(&mut self, spec: &Spec, pid: u32) -> Result<bool> {
        let identity = identity(pid)?;
        let handle = Kernel(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) });
        if handle.0.is_null() {
            return Err(last());
        }
        let mut name = vec![0u16; 32768];
        let mut size = name.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle.0, 0, name.as_mut_ptr(), &mut size) } == 0 {
            return Err(last());
        }
        let path = PathBuf::from(String::from_utf16(&name[..size as usize]).map_err(|_| Fault::conflict())?);
        Ok(identity.sid == spec.account
            && identity.session == 0
            && identity.profile.canonicalize().map_err(Fault::io)? == spec.profile.canonicalize().map_err(Fault::io)?
            && path.canonicalize().map_err(Fault::io)? == spec.executable.canonicalize().map_err(Fault::io)?)
    }
}
