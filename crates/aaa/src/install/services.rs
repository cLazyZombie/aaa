//! 감독자 읽기만. 등록·reload·시작·권한 상승을 호출하지 않는다.
use aaa_proto::install::{InstallCheckState, InstallService};
use std::path::{Path, PathBuf};

pub struct Inventory {
    pub files: Vec<PathBuf>,
    pub manager: InstallCheckState,
    pub present: Option<bool>,
}

pub fn read(service: InstallService, roles: &[&str], instance: Option<&str>, home: &Path) -> Inventory {
    native(service, roles, instance, home)
}

#[cfg(target_os = "macos")]
fn native(service: InstallService, roles: &[&str], instance: Option<&str>, home: &Path) -> Inventory {
    let files = roles
        .iter()
        .map(|role| {
            let label = instance.map_or_else(|| format!("dev.aaa.{role}"), |i| format!("dev.aaa.{i}.{role}"));
            home.join("Library/LaunchAgents").join(format!("{label}.plist"))
        })
        .collect();
    // plist 유무는 읽지만 launchctl의 실제 등록/ready 상태를 추정하지 않는다.
    Inventory {
        files,
        manager: if service == InstallService::None {
            InstallCheckState::NotRequested
        } else {
            InstallCheckState::Unknown
        },
        present: None,
    }
}

#[cfg(any(windows, target_os = "linux"))]
fn unit_name(instance: Option<&str>, role: &str) -> String {
    instance.map_or_else(|| format!("aaa-{role}"), |i| format!("aaa-{i}-{role}"))
}

#[cfg(target_os = "linux")]
fn native(service: InstallService, roles: &[&str], instance: Option<&str>, home: &Path) -> Inventory {
    let user = service == InstallService::SystemdUser;
    let mut dirs: Vec<PathBuf> = if user {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        vec![
            config.join("systemd/user"),
            data.join("systemd/user"),
            "/etc/systemd/user".into(),
            "/usr/lib/systemd/user".into(),
            "/lib/systemd/user".into(),
        ]
    } else {
        [
            "/etc/systemd/system",
            "/run/systemd/system",
            "/usr/local/lib/systemd/system",
            "/usr/lib/systemd/system",
            "/lib/systemd/system",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    };
    if service == InstallService::None {
        // 설정만 준비해도 같은 instance의 기존 user unit을 덮는 계획은 만들지 않는다.
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"));
        dirs.extend([
            config.join("systemd/user"),
            data.join("systemd/user"),
            "/etc/systemd/user".into(),
            "/usr/lib/systemd/user".into(),
            "/lib/systemd/user".into(),
        ]);
    }
    let files = roles
        .iter()
        .flat_map(|role| dirs.iter().map(move |dir| dir.join(format!("{}.service", unit_name(instance, role)))))
        .collect();
    if service == InstallService::None {
        return Inventory { files, manager: InstallCheckState::NotRequested, present: None };
    }
    let manager = tokio::runtime::Builder::new_current_thread().enable_all().build().ok().and_then(|rt| {
        let binary = ["/usr/bin/systemctl", "/bin/systemctl"].into_iter().find(|p| Path::new(p).is_file())?;
        rt.block_on(async {
            let mut present = false;
            for role in roles {
                let mut cmd = tokio::process::Command::new(binary);
                cmd.arg(if user { "--user" } else { "--system" })
                    .args(["show", "--no-pager", "--no-ask-password", "--property=LoadState", "--value"])
                    .arg(format!("{}.service", unit_name(instance, role)))
                    .env("SYSTEMD_PAGERSECURE", "1")
                    .env("SYSTEMD_COLORS", "0")
                    .stdin(std::process::Stdio::null())
                    .kill_on_drop(true);
                let out =
                    tokio::time::timeout(std::time::Duration::from_millis(2500), cmd.output()).await.ok()?.ok()?;
                if !out.status.success() {
                    return None;
                }
                match std::str::from_utf8(&out.stdout).ok()?.trim() {
                    "not-found" => {}
                    "loaded" | "masked" | "error" | "bad-setting" | "merged" => present = true,
                    _ => return None,
                }
            }
            Some(present)
        })
    });
    Inventory {
        files,
        manager: if manager.is_some() { InstallCheckState::Passed } else { InstallCheckState::Unknown },
        present: manager,
    }
}

#[cfg(windows)]
fn native(_: InstallService, roles: &[&str], instance: Option<&str>, _: &Path) -> Inventory {
    use windows_sys::Win32::{
        Foundation::{ERROR_SERVICE_DOES_NOT_EXIST, GetLastError},
        System::Services::{
            CloseServiceHandle, OpenSCManagerW, OpenServiceW, SC_MANAGER_CONNECT, SERVICE_QUERY_STATUS,
        },
    };
    let mut result = Inventory { files: vec![], manager: InstallCheckState::Unknown, present: None };
    // SAFETY: 고정한 현지 서비스 이름을 조회한다. CREATE/CHANGE/START/STOP 권한을 요청하지 않는다.
    unsafe {
        let manager = OpenSCManagerW(std::ptr::null(), std::ptr::null(), SC_MANAGER_CONNECT);
        if manager.is_null() {
            return result;
        }
        let mut present = false;
        let mut read_ok = true;
        for role in roles {
            let name: Vec<_> = unit_name(instance, role).encode_utf16().chain(Some(0)).collect();
            let handle = OpenServiceW(manager, name.as_ptr(), SERVICE_QUERY_STATUS);
            if handle.is_null() {
                if GetLastError() != ERROR_SERVICE_DOES_NOT_EXIST {
                    read_ok = false;
                }
            } else {
                present = true;
                CloseServiceHandle(handle);
            }
        }
        CloseServiceHandle(manager);
        if read_ok {
            result.manager = InstallCheckState::Passed;
            result.present = Some(present);
        }
    }
    result
}
