use super::*;
use model::{Value, desired};
use std::os::windows::ffi::OsStrExt;
use windows_sys::Win32::{
    Foundation::*,
    Globalization::*,
    Security::*,
    Storage::FileSystem::{CommitTransaction, CreateTransaction, RollbackTransaction},
    System::{Environment::ExpandEnvironmentStringsForUserW, Registry::*, Threading::*},
};
const TARGET: &str = "HKCU/Environment/Path";
fn required() -> Fault {
    Fault { code: "path_registry_transaction_unavailable", exit: 3 }
}
fn error(code: u32) -> Fault {
    if matches!(
        code,
        ERROR_TRANSACTIONAL_CONFLICT
            | ERROR_TRANSACTION_ALREADY_ABORTED
            | ERROR_TRANSACTION_NOT_ACTIVE
            | ERROR_MORE_DATA
    ) {
        Fault::conflict()
    } else if code == ERROR_ACCESS_DENIED {
        Fault { code: "path_registry_permission_required", exit: 3 }
    } else {
        required()
    }
}
fn w(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
struct Transaction {
    handle: HANDLE,
    key: Key,
    committed: bool,
}
impl Drop for Transaction {
    fn drop(&mut self) {
        unsafe {
            if !self.committed {
                RollbackTransaction(self.handle);
            }
            CloseHandle(self.handle);
        }
    }
}
impl Transaction {
    fn commit(mut self) -> Result<()> {
        if unsafe { CommitTransaction(self.handle) } == 0 {
            return Err(error(unsafe { GetLastError() }));
        }
        self.committed = true;
        Ok(())
    }
}
struct Registry {
    parent: Key,
    name: Vec<u16>,
}
impl Registry {
    fn user_environment() -> Result<Self> {
        Self::at("Environment")
    }
    fn at(name: &str) -> Result<Self> {
        let mut parent = std::ptr::null_mut();
        let code = unsafe { RegOpenCurrentUser(KEY_READ, &mut parent) };
        if code != ERROR_SUCCESS {
            return Err(error(code));
        }
        Ok(Self { parent: Key(parent), name: w(name) })
    }
    fn read(&self) -> Result<Option<Value>> {
        let mut key = std::ptr::null_mut();
        let code = unsafe { RegOpenKeyExW(self.parent.0, self.name.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if code != ERROR_SUCCESS {
            return Err(error(code));
        }
        query(&Key(key))
    }
    fn prepare(&self, expected: &Option<Value>, desired: &Value) -> Result<Transaction> {
        let handle =
            unsafe { CreateTransaction(std::ptr::null_mut(), std::ptr::null_mut(), 0, 0, 0, 10_000, std::ptr::null()) };
        if handle == INVALID_HANDLE_VALUE {
            return Err(error(unsafe { GetLastError() }));
        }
        let mut parent = std::ptr::null_mut();
        let code = unsafe { RegOpenCurrentUser(KEY_READ | KEY_WRITE, &mut parent) };
        if code != ERROR_SUCCESS {
            unsafe {
                RollbackTransaction(handle);
                CloseHandle(handle);
            }
            return Err(error(code));
        }
        let parent = Key(parent);
        let mut key = std::ptr::null_mut();
        let code = unsafe {
            RegCreateKeyTransactedW(
                parent.0,
                self.name.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_QUERY_VALUE | KEY_SET_VALUE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
                handle,
                std::ptr::null(),
            )
        };
        if code != ERROR_SUCCESS {
            unsafe {
                RollbackTransaction(handle);
                CloseHandle(handle);
            }
            return Err(error(code));
        }
        let transaction = Transaction { handle, key: Key(key), committed: false };
        if query(&transaction.key)? != *expected {
            return Err(Fault::conflict());
        }
        set(&transaction.key, desired)?;
        Ok(transaction)
    }
}
fn query(key: &Key) -> Result<Option<Value>> {
    let (mut kind, mut size) = (0, 0);
    let name = w("Path");
    let code =
        unsafe { RegQueryValueExW(key.0, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut size) };
    if code == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if code != ERROR_SUCCESS {
        return Err(error(code));
    }
    if size > 65534 {
        return Err(required());
    }
    let mut bytes = vec![0; size as usize];
    let code = unsafe {
        RegQueryValueExW(
            key.0,
            name.as_ptr(),
            std::ptr::null(),
            &mut kind,
            if bytes.is_empty() { std::ptr::null_mut() } else { bytes.as_mut_ptr() },
            &mut size,
        )
    };
    if code != ERROR_SUCCESS {
        return Err(error(code));
    }
    if size as usize > bytes.len() {
        return Err(Fault::conflict());
    }
    bytes.truncate(size as usize);
    Ok(Some(Value { kind, bytes }))
}
fn set(key: &Key, value: &Value) -> Result<()> {
    let code = unsafe {
        RegSetValueExW(key.0, w("Path").as_ptr(), 0, value.kind, value.bytes.as_ptr(), value.bytes.len() as u32)
    };
    if code != ERROR_SUCCESS {
        return Err(error(code));
    }
    Ok(())
}
fn normalized(value: &[u16]) -> Vec<u16> {
    let value = if value.len() >= 2 && value.first() == Some(&(b'"' as u16)) && value.last() == Some(&(b'"' as u16)) {
        &value[1..value.len() - 1]
    } else {
        value
    };
    let mut value: Vec<_> = value.iter().map(|c| if *c == b'/' as u16 { b'\\' as u16 } else { *c }).collect();
    while value.len() > 3 && value.last() == Some(&(b'\\' as u16)) {
        value.pop();
    }
    value
}
fn equal(entry: &[u16], directory: &[u16], expand: bool) -> Result<bool> {
    let expanded;
    let entry = if expand && entry.contains(&(b'%' as u16)) {
        let mut token = std::ptr::null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY | TOKEN_IMPERSONATE, &mut token) } == 0 {
            return Err(required());
        }
        let mut input = entry.to_vec();
        input.push(0);
        let mut output = vec![0; 32768];
        let ok = unsafe {
            ExpandEnvironmentStringsForUserW(token, input.as_ptr(), output.as_mut_ptr(), output.len() as u32)
        };
        unsafe {
            CloseHandle(token);
        }
        if ok == 0 {
            return Err(required());
        }
        let length = output.iter().position(|c| *c == 0).ok_or_else(required)?;
        output.truncate(length);
        expanded = output;
        expanded.as_slice()
    } else {
        entry
    };
    let left = normalized(entry);
    let right = normalized(directory);
    if left.is_empty() {
        return Ok(false);
    }
    Ok(unsafe { CompareStringOrdinal(left.as_ptr(), left.len() as i32, right.as_ptr(), right.len() as i32, 1) }
        == CSTR_EQUAL)
}
fn directory(plan: &InstallPlan) -> Result<Vec<u16>> {
    Ok(Path::new(&plan.context.bin_path).parent().ok_or_else(Fault::conflict)?.as_os_str().encode_wide().collect())
}
fn value_hash(value: &Option<Value>) -> Result<String> {
    Ok(hash(&aaa_common::install_crypto::canonical(value).map_err(|_| Fault::conflict())?))
}
pub(super) fn permissions(plan: &InstallPlan) -> Result<Vec<InstallPermission>> {
    let before = Registry::user_environment()?.read()?;
    let after = desired(before.as_ref(), &directory(plan)?, equal)?;
    Ok(vec![InstallPermission {
        operation: InstallOperation::ModifyPath,
        target: TARGET.into(),
        before_sha256: Some(value_hash(&before)?),
        after_sha256: Some(value_hash(&Some(after))?),
    }])
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    installation_id: String,
    plan_sha256: String,
    account_sha256: String,
    before: Option<Value>,
    after: Value,
    applied: bool,
}
pub(super) fn apply(plan: &InstallPlan, lease: &authorization::Lease) -> Result<bool> {
    let permission = lease.target(plan, InstallOperation::ModifyPath, TARGET)?;
    let registry = Registry::user_environment()?;
    let before = registry.read()?;
    let after = desired(before.as_ref(), &directory(plan)?, equal)?;
    // 다른 user 항목이 늘었어도 이미 목표 entry가 있으면 아무것도 쓰지 않는다.
    if before.as_ref() == Some(&after) {
        return Ok(false);
    }
    if Some(value_hash(&before)?) != permission.before_sha256
        || Some(value_hash(&Some(after.clone()))?) != permission.after_sha256
    {
        return Err(Fault::conflict());
    }
    let path =
        Path::new(&plan.context.state_dir).join("install/authorization").join(format!("{}.path.json", lease.id()));
    let mut record = Record {
        schema: 1,
        installation_id: lease.id().into(),
        plan_sha256: aaa_config::install::plan_sha256(plan),
        account_sha256: account_hash()?,
        before: before.clone(),
        after: after.clone(),
        applied: false,
    };
    if path.symlink_metadata().is_ok() {
        let old: Record = serde_json::from_slice(&private::read(&path, 1024 * 1024).map_err(Fault::io)?)
            .map_err(|_| Fault::conflict())?;
        if old.schema != 1
            || old.installation_id != record.installation_id
            || old.plan_sha256 != record.plan_sha256
            || old.account_sha256 != record.account_sha256
            || old.before != record.before
            || old.after != record.after
        {
            return Err(Fault::conflict());
        }
    }
    lease.valid_at(aaa_common::util::now_ms())?;
    private::write(&path, &serde_json::to_vec(&record).map_err(|_| Fault::conflict())?).map_err(Fault::io)?;
    let pending = registry.prepare(&before, &after)?;
    lease.valid_at(aaa_common::util::now_ms())?;
    pending.commit()?;
    if registry.read()?.as_ref() != Some(&after) {
        return Err(Fault::conflict());
    }
    record.applied = true;
    private::write(&path, &serde_json::to_vec(&record).map_err(|_| Fault::conflict())?).map_err(Fault::io)?;
    Ok(true)
}
pub(super) fn observe(plan: &InstallPlan, result: &mut InstallResult) -> Result<()> {
    let before = Registry::user_environment()?.read()?;
    let after = desired(before.as_ref(), &directory(plan)?, equal)?;
    let ready = before.as_ref() == Some(&after);
    result.checks.push(InstallCheck {
        name: "user_path".into(),
        state: if ready { InstallCheckState::Passed } else { InstallCheckState::NotRun },
    });
    if !ready && result.exit_code == 0 {
        result.status = InstallStatus::NeedsUser;
        result.exit_code = 3;
        result.error = None;
        result.needs_user.push(InstallNeedsUser {
            code: "installation_user_path_required".into(),
            target: "path".into(),
            message: "현재 사용자의 PATH를 확인한 뒤 같은 ID로 재개하세요".into(),
        });
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn raw(kind: u32, text: &str) -> Value {
        Value { kind, bytes: text.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect() }
    }
    #[test]
    fn windows_path_case_slashes_quotes_and_current_user_expansion_do_not_duplicate_entries() {
        let entry: Vec<u16> = r#""c:/AAA/bin/""#.encode_utf16().collect();
        let directory: Vec<u16> = r"C:\AAA\bin".encode_utf16().collect();
        assert!(equal(&entry, &directory, false).unwrap());
        for kind in [REG_SZ, REG_EXPAND_SZ] {
            let before = raw(kind, r#""c:/AAA/bin/""#);
            assert!(desired(Some(&before), &directory, equal).unwrap() == before);
        }
        let directory: Vec<u16> = std::env::var("USERPROFILE").unwrap().encode_utf16().collect();
        assert!(equal(&r"%USERPROFILE%".encode_utf16().collect::<Vec<_>>(), &directory, true).unwrap());
    }
    #[test]
    fn windows_registry_transaction_preserves_user_edit_on_conflict_and_rolls_back_without_commit() {
        let name = format!("Software\\aaa-install-path-{}", aaa_common::util::new_id());
        let registry = Registry::at(&name).unwrap();
        let before = None;
        let initial = raw(REG_EXPAND_SZ, r"C:\Other;;");
        registry.prepare(&before, &initial).unwrap().commit().unwrap();
        let after = raw(REG_EXPAND_SZ, r"C:\Other;;;C:\AAA\bin");
        let pending = registry.prepare(&Some(initial.clone()), &after).unwrap();
        drop(pending);
        assert!(registry.read().unwrap() == Some(initial.clone()));
        let pending = registry.prepare(&Some(initial), &after).unwrap();
        let mut key = std::ptr::null_mut();
        assert_eq!(
            unsafe { RegOpenKeyExW(registry.parent.0, registry.name.as_ptr(), 0, KEY_SET_VALUE, &mut key) },
            ERROR_SUCCESS
        );
        let key = Key(key);
        let user = raw(REG_SZ, r"C:\UserEdited");
        set(&key, &user).unwrap();
        assert_eq!(pending.commit().err().unwrap().exit, 4);
        assert!(registry.read().unwrap() == Some(user));
        drop(key);
        assert_eq!(unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, registry.name.as_ptr()) }, ERROR_SUCCESS);
    }
}
