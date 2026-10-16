//! 머신별 기본 위치(기획서 §7.1). `AA_CONFIG_DIR`·`AA_STATE_DIR`로 덮어쓴다.

use std::path::PathBuf;

pub fn home() -> PathBuf {
    #[cfg(windows)]
    let h = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let h = std::env::var_os("HOME");
    h.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// 설정(TOML) 폴더
pub fn config_dir() -> PathBuf {
    if let Some(d) = env_dir("AA_CONFIG_DIR") {
        return d;
    }
    #[cfg(windows)]
    {
        // roaming %APPDATA%는 쓰지 않는다(머신별 값이 동기화되면 안 된다).
        env_dir("LOCALAPPDATA").unwrap_or_else(home).join("aa").join("config")
    }
    #[cfg(target_os = "linux")]
    {
        env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config")).join("aa")
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        home().join(".config").join("aa")
    }
}

/// 상태(SQLite·로그·키·socket) 폴더
pub fn state_dir() -> PathBuf {
    if let Some(d) = env_dir("AA_STATE_DIR") {
        return d;
    }
    #[cfg(windows)]
    {
        env_dir("LOCALAPPDATA").unwrap_or_else(home).join("aa").join("state")
    }
    #[cfg(target_os = "linux")]
    {
        env_dir("XDG_STATE_HOME").unwrap_or_else(|| home().join(".local").join("state")).join("aa")
    }
    #[cfg(target_os = "macos")]
    {
        home().join("Library").join("Application Support").join("aa")
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        home().join(".local").join("state").join("aa")
    }
}
