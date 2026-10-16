//! 머신별 기본 위치(기획서 §7.1). `AAA_CONFIG_DIR`·`AAA_STATE_DIR`로 덮어쓴다.
//! 이름을 aa에서 aaa로 바꿨다(결정 18). 옛 기본 위치는 읽지 않고, 옮기는 명령만 안내한다(`legacy_moves`).

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
    env_dir("AAA_CONFIG_DIR").unwrap_or_else(|| default_config_dir("aaa"))
}

fn default_config_dir(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        // roaming %APPDATA%는 쓰지 않는다(머신별 값이 동기화되면 안 된다).
        env_dir("LOCALAPPDATA").unwrap_or_else(home).join(name).join("config")
    }
    #[cfg(target_os = "linux")]
    {
        env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config")).join(name)
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        home().join(".config").join(name)
    }
}

/// 상태(SQLite·로그·키·socket) 폴더
pub fn state_dir() -> PathBuf {
    env_dir("AAA_STATE_DIR").unwrap_or_else(|| default_state_dir("aaa"))
}

fn default_state_dir(name: &str) -> PathBuf {
    #[cfg(windows)]
    {
        env_dir("LOCALAPPDATA").unwrap_or_else(home).join(name).join("state")
    }
    #[cfg(target_os = "linux")]
    {
        env_dir("XDG_STATE_HOME").unwrap_or_else(|| home().join(".local").join("state")).join(name)
    }
    #[cfg(target_os = "macos")]
    {
        home().join("Library").join("Application Support").join(name)
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        home().join(".local").join("state").join(name)
    }
}

/// 이름 변경(결정 18) 전 기본 위치에서 옮겨야 할 폴더: 새 기본 위치가 없고 옛 위치(`aa`)가 있는 것만.
/// 환경 변수로 위치를 정했으면 보지 않는다.
pub fn legacy_moves() -> Vec<(PathBuf, PathBuf)> {
    let mut out = Vec::new();
    for (env, old, new) in [
        ("AAA_CONFIG_DIR", default_config_dir("aa"), default_config_dir("aaa")),
        ("AAA_STATE_DIR", default_state_dir("aa"), default_state_dir("aaa")),
    ] {
        if env_dir(env).is_none() && old.is_dir() && !new.exists() {
            out.push((old, new));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn legacy_names() {
        let (old, new) = (super::default_state_dir("aa"), super::default_state_dir("aaa"));
        assert_ne!(old, new);
    }
}
