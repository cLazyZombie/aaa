//! SCM/LSA는 이 고정 adapter에서만 변경한다. raw 오류/계정/암호를 Fault에 복사하지 않는다.
use super::*;
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::{
    Foundation::*,
    Security::{Authentication::Identity::*, Authorization::*, *},
    Storage::FileSystem::DELETE,
    System::{
        Environment::*,
        Services::*,
        SystemServices::{SE_GROUP_ENABLED, SE_GROUP_USE_FOR_DENY_ONLY},
        Threading::*,
    },
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
        ERROR_SERVICE_MARKED_FOR_DELETE => failure("service_removal_pending", 3),
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
    pub image: PathBuf,
    pub session: u32,
    pub elevated: bool,
    pub created: u64,
}
pub(super) fn identity(pid: u32) -> Result<Identity> {
    let process = Kernel(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) });
    if process.0.is_null() {
        return Err(last());
    }
    process_identity(process.0)
}
// PID를 다시 열지 않는다. 신원·생성 시각·image는 같은 살아 있는 process object의 관찰이다.
fn process_identity(process: HANDLE) -> Result<Identity> {
    if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
        return Err(failure("service_process_unverified", 3));
    }
    let mut created_time: FILETIME = unsafe { std::mem::zeroed() };
    let mut exit_time: FILETIME = unsafe { std::mem::zeroed() };
    let mut kernel_time: FILETIME = unsafe { std::mem::zeroed() };
    let mut user_time: FILETIME = unsafe { std::mem::zeroed() };
    if unsafe { GetProcessTimes(process, &mut created_time, &mut exit_time, &mut kernel_time, &mut user_time) } == 0 {
        return Err(last());
    }
    let created = (u64::from(created_time.dwHighDateTime) << 32) | u64::from(created_time.dwLowDateTime);
    let mut token = std::ptr::null_mut();
    let mut image = vec![0u16; 32768];
    let mut size = image.len() as u32;
    if unsafe { QueryFullProcessImageNameW(process, 0, image.as_mut_ptr(), &mut size) } == 0 {
        return Err(last());
    }
    let image = PathBuf::from(String::from_utf16(&image[..size as usize]).map_err(|_| Fault::conflict())?);
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
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
    if unsafe { WaitForSingleObject(process, 0) } != WAIT_TIMEOUT {
        return Err(failure("service_process_unverified", 3));
    }
    Ok(Identity {
        sid,
        profile,
        image,
        session: unsafe { *session.as_ptr().cast::<u32>() },
        elevated: unsafe { (*elevation.as_ptr().cast::<TOKEN_ELEVATION>()).TokenIsElevated != 0 },
        created,
    })
}
struct Environment(*mut core::ffi::c_void);
impl Drop for Environment {
    fn drop(&mut self) {
        unsafe {
            DestroyEnvironmentBlock(self.0);
        }
    }
}
/// SCM/helper의 환경 대신 실제 서비스 token의 사용자 환경을 읽는다. 비밀 이름은 전달하지 않는다.
pub(super) fn user_environment() -> Result<BTreeMap<String, String>> {
    let mut token = std::ptr::null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_DUPLICATE, &mut token) } == 0 {
        return Err(last());
    }
    let token = Kernel(token);
    let mut block = std::ptr::null_mut();
    if unsafe { CreateEnvironmentBlock(&mut block, token.0, 0) } == 0 {
        return Err(denied());
    }
    let block = Environment(block);
    let ptr = block.0.cast::<u16>();
    let mut offset = 0;
    let mut values = BTreeMap::new();
    while offset < 1024 * 1024 && unsafe { *ptr.add(offset) } != 0 {
        let item = unsafe { text(ptr.add(offset)) }?;
        offset += item.encode_utf16().count() + 1;
        let Some((key, value)) = item.split_once('=') else {
            return Err(denied());
        };
        let key = key.to_ascii_uppercase();
        if matches!(
            key.as_str(),
            "SYSTEMROOT"
                | "WINDIR"
                | "COMSPEC"
                | "PATHEXT"
                | "TEMP"
                | "TMP"
                | "USERNAME"
                | "USERDOMAIN"
                | "USERDNSDOMAIN"
                | "LOGONSERVER"
                | "USERPROFILE"
                | "HOMEDRIVE"
                | "HOMEPATH"
                | "APPDATA"
                | "LOCALAPPDATA"
                | "PROGRAMDATA"
                | "PROGRAMFILES"
                | "PROGRAMFILES(X86)"
                | "PROGRAMW6432"
                | "COMMONPROGRAMFILES"
                | "COMMONPROGRAMFILES(X86)"
                | "COMMONPROGRAMW6432"
                | "PATH"
                | "LANG"
                | "LC_ALL"
                | "LC_CTYPE"
                | "TZ"
                | "TERM"
                | "CODEX_HOME"
                | "CLAUDE_CONFIG_DIR"
        ) {
            values.insert(key, value.to_owned());
        }
    }
    if offset >= 1024 * 1024
        || ["USERPROFILE", "APPDATA", "LOCALAPPDATA", "PATH", "SYSTEMROOT", "TEMP", "TMP"]
            .iter()
            .any(|key| values.get(*key).is_none_or(String::is_empty))
    {
        return Err(denied());
    }
    Ok(values)
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
fn account_token(spec: &Spec, name: &str, password: &Password) -> Result<Kernel> {
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
    Ok(token)
}
fn logon_right(spec: &Spec, name: &str, password: &Password) -> Result<()> {
    let token = account_token(spec, name, password)?;
    let user = token_information(token.0, TokenUser)?;
    let user_sid = unsafe { (*user.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    let groups = token_information(token.0, TokenGroups)?;
    let groups = unsafe { &*groups.as_ptr().cast::<TOKEN_GROUPS>() };
    if groups.GroupCount > 4096 {
        return Err(denied());
    }
    let groups = unsafe { std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize) };
    let read = policy(POLICY_LOOKUP_NAMES)?;
    let mut decision = LogonRights::default();
    decision.observe(&rights(&read, user_sid)?, true)?;
    for group in groups {
        let eligible = group.Attributes & (SE_GROUP_ENABLED as u32) != 0
            && group.Attributes & (SE_GROUP_USE_FOR_DENY_ONLY as u32) == 0;
        decision.observe(&rights(&read, group.Sid)?, eligible)?;
    }
    // Network token에는 없는 Service logon SID의 Deny도 읽고, 서비스 전체 정책을 우회하지 않는다.
    let mut service_sid = [0usize; 16];
    let mut size = std::mem::size_of_val(&service_sid) as u32;
    if unsafe { CreateWellKnownSid(WinServiceSid, std::ptr::null_mut(), service_sid.as_mut_ptr().cast(), &mut size) }
        == 0
    {
        return Err(last());
    }
    decision.observe(&rights(&read, service_sid.as_mut_ptr().cast())?, false)?;
    if decision.grant_required() {
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
// Unsubscribe는 진행 중 callback이 끝날 때까지 기다린다. event보다 먼저 해제한다.
struct Changes {
    subscription: PSC_NOTIFICATION_REGISTRATION,
    _event: Kernel,
}
impl Drop for Changes {
    fn drop(&mut self) {
        unsafe { UnsubscribeServiceChangeNotifications(self.subscription) };
    }
}
unsafe extern "system" fn changed(_: u32, context: *const core::ffi::c_void) {
    // callback은 OS 사건을 알리기만 한다. SCM RPC/파일 I/O는 기다리는 thread가 수행한다.
    unsafe {
        SetEvent(context as HANDLE);
    }
}
fn changes(handle: &Service) -> Result<Changes> {
    let event = Kernel(unsafe { CreateEventW(std::ptr::null(), 0, 0, std::ptr::null()) });
    if event.0.is_null() {
        return Err(last());
    }
    let mut subscription = 0;
    let code = unsafe {
        SubscribeServiceChangeNotifications(
            handle.0,
            SC_EVENT_STATUS_CHANGE,
            Some(changed),
            event.0.cast_const(),
            &mut subscription,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(error(code));
    }
    Ok(Changes { subscription, _event: event })
}
fn wait_state(handle: &Service, desired: u32) -> Result<SERVICE_STATUS_PROCESS> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    // 구독 뒤 조회해 두 호출 사이의 상태 변화를 놓치지 않는다. 고정 간격 polling은 없다.
    let changes = changes(handle)?;
    loop {
        let value = status(handle)?;
        if value.dwCurrentState == desired {
            return Ok(value);
        }
        if desired == SERVICE_RUNNING && value.dwCurrentState == SERVICE_STOPPED {
            return Err(if value.dwWin32ExitCode == ERROR_SERVICE_SPECIFIC_ERROR {
                match value.dwServiceSpecificExitCode {
                    3 => failure("service_backend_needs_user", 3),
                    4 => Fault::conflict(),
                    _ => failure("service_operation_failed", 1),
                }
            } else {
                error(value.dwWin32ExitCode)
            });
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(failure("service_transition_pending", 3));
        }
        match unsafe { WaitForSingleObject(changes._event.0, remaining.as_millis().min(u32::MAX as u128) as u32) } {
            WAIT_OBJECT_0 => {}
            WAIT_TIMEOUT => return Err(failure("service_transition_pending", 3)),
            _ => return Err(last()),
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
    fn stop(&mut self, spec: &Spec) -> Result<()> {
        let Some(handle) = open(spec, SERVICE_STOP | SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG)? else {
            return Ok(());
        };
        if configuration(&handle)? != Configuration::desired(spec) {
            return Err(Fault::conflict());
        }
        if status(&handle)?.dwCurrentState != SERVICE_STOPPED {
            let mut value: SERVICE_STATUS = unsafe { std::mem::zeroed() };
            if unsafe { ControlService(handle.0, SERVICE_CONTROL_STOP, &mut value) } == 0 {
                let code = unsafe { GetLastError() };
                if code != ERROR_SERVICE_NOT_ACTIVE {
                    return Err(error(code));
                }
            }
            wait_state(&handle, SERVICE_STOPPED)?;
        }
        Ok(())
    }
    fn remove(&mut self, spec: &Spec) -> Result<()> {
        self.stop(spec)?;
        let Some(handle) = open(spec, SERVICE_QUERY_CONFIG | DELETE)? else {
            return Ok(());
        };
        if configuration(&handle)? != Configuration::desired(spec) {
            return Err(Fault::conflict());
        }
        if unsafe { DeleteService(handle.0) } == 0 {
            return Err(last());
        }
        Ok(())
    }
    fn credentials(&mut self, spec: &Spec, password: &Password) -> Result<()> {
        let handle = open(spec, SERVICE_CHANGE_CONFIG | SERVICE_QUERY_CONFIG)?.ok_or_else(Fault::conflict)?;
        if configuration(&handle)? != Configuration::desired(spec) {
            return Err(Fault::conflict());
        }
        let name = account_name(&spec.account)?;
        let _verified = account_token(spec, &name, password)?;
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
                w(&name).as_ptr(),
                password.0.as_ptr(),
                std::ptr::null(),
            )
        } == 0
        {
            return Err(last());
        }
        Ok(())
    }
    fn process(&mut self, spec: &Spec, pid: u32) -> Result<Option<Process>> {
        let identity = identity(pid)?;
        Ok((identity.sid == spec.account
            && identity.session == 0
            && identity.profile.canonicalize().map_err(Fault::io)?
                == spec.profile.canonicalize().map_err(Fault::io)?
            && identity.image.canonicalize().map_err(Fault::io)?
                == spec.executable.canonicalize().map_err(Fault::io)?)
        .then_some(Process { pid, created: identity.created, elevated: identity.elevated }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_observation_rejects_completed_process_with_its_handle_still_open() {
        use std::io::Read;
        use std::os::windows::io::AsRawHandle;
        use std::process::{Command, Stdio};
        // 같은 시험 실행 파일의 목록만 출력하고 성공 종료한다. 시험 본체는 실행하지 않아 재귀하지 않는다.
        let mut child = Command::new(std::env::current_exe().unwrap())
            .arg("--list")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // wait 중에도 pipe를 읽어 많은 실패 출력이 자식의 종료를 막지 않게 한다.
        let mut stderr = child.stderr.take().unwrap();
        let stderr = std::thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr.read_to_end(&mut bytes).unwrap();
            bytes
        });
        let status = child.wait().unwrap();
        let stderr = stderr.join().unwrap();
        if !status.success() {
            eprintln!(
                "test executable exit fixture failed: status={status:?}, code={:?}, hex={:?}, stderr_first_line={:?}",
                status.code(),
                status.code().map(|code| format!("0x{:08X}", code as u32)),
                String::from_utf8_lossy(&stderr).lines().next().unwrap_or("")
            );
        }
        assert!(status.success(), "test executable exit fixture failed");
        let handle = child.as_raw_handle();
        // 잘못된 handle이 아닌 실제 종료한 process가 부정 입력이어야 한다.
        assert_eq!(unsafe { WaitForSingleObject(handle, 0) }, WAIT_OBJECT_0);
        assert_eq!(process_identity(handle).err().unwrap().code, "service_process_unverified");
    }
    #[test]
    fn status_change_subscription_reads_and_unsubscribes_from_a_real_windows_service() {
        let manager = manager(SC_MANAGER_CONNECT).unwrap();
        // Windows Event Log는 읽기 probe다. 이 owner는 등록/start/stop/정책 변경을 하지 않는다.
        let handle = Service(unsafe { OpenServiceW(manager.0, w("EventLog").as_ptr(), SERVICE_QUERY_STATUS) });
        assert!(!handle.0.is_null());
        let environment = user_environment().unwrap();
        let identity = identity(std::process::id()).unwrap();
        assert!(
            Path::new(&environment["USERPROFILE"]).canonicalize().unwrap() == identity.profile.canonicalize().unwrap()
        );
        let current = status(&handle).unwrap();
        let observed = wait_state(&handle, current.dwCurrentState).unwrap();
        assert_eq!(observed.dwCurrentState, current.dwCurrentState);
    }
}
