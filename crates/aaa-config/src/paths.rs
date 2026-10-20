//! 머신별 기본 위치(기획서 §7.1). `AAA_CONFIG_DIR`·`AAA_STATE_DIR`로 덮어쓴다.
//! 이름을 aa에서 aaa로 바꿨다(결정 18). 옛 기본 위치는 읽지 않고, 옮기는 명령만 안내한다(`legacy_moves`).
//! 인스턴스(`--instance <이름>`·`AAA_INSTANCE`, plan §8.294)는 모든 기본 이름의 `aaa`를 `aaa-<이름>`으로 바꾼다:
//! 설정·상태·홈·작업 root·로그가 지금 자리 옆의 형제 폴더가 된다. 이름이 없으면 지금 경로와 한 글자도 다르지 않다.

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

/// 이름 상한. 형제 폴더라 꼬리는 `-<이름>`만 늘고, 가장 긴 unix socket(상태 폴더 아래 `run/<접두><ID>.sock`)이
/// SUN_LEN(104) 안에 들어가는지는 `aaa init`이 실제 경로로 다시 잰다
pub const INSTANCE_MAX: usize = 16;

/// 지금 인스턴스 이름(`AAA_INSTANCE`). 없으면 기본 인스턴스다. CLI가 시작 때 `check_instance`로 거른다
pub fn instance() -> Option<String> {
    std::env::var("AAA_INSTANCE").ok().filter(|v| !v.is_empty())
}

/// 인스턴스 이름 규칙: 소문자로 시작하는 소문자·숫자·`-`, 16자 이하. 폴더·launchd label·쿠키 이름에 그대로 들어간다
pub fn check_instance(name: &str) -> Result<(), String> {
    let ok = name.len() <= INSTANCE_MAX
        && name.starts_with(|c: char| c.is_ascii_lowercase())
        && !name.ends_with('-')
        && !name.contains("--")
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        return Err(format!(
            "인스턴스 이름 `{name}`을 쓸 수 없다: 소문자로 시작하는 소문자·숫자·`-`, {INSTANCE_MAX}자 이하(예: work)"
        ));
    }
    if name == "default" {
        return Err("`default`는 인스턴스 이름으로 쓰지 않는다: 기본 인스턴스는 --instance 없이 쓴다".into());
    }
    Ok(())
}

/// 기본 이름 `base`에 인스턴스 꼬리를 붙인다(`aaa` → `aaa-work`, `.aaa` → `.aaa-work`). 기본 인스턴스는 그대로다
pub fn instance_name(base: &str) -> String {
    named(base, instance().as_deref())
}

/// `instance_name`의 규칙(시험은 env 대신 이름을 준다)
pub fn named(base: &str, instance: Option<&str>) -> String {
    match instance {
        Some(name) => format!("{base}-{name}"),
        None => base.to_string(),
    }
}

/// 사람에게 보이는 명령에 붙일 꼬리(` --instance work`). 기본 인스턴스는 빈 글이다
pub fn instance_flag() -> String {
    instance().map(|name| format!(" --instance {name}")).unwrap_or_default()
}

/// 설정(TOML) 폴더
pub fn config_dir() -> PathBuf {
    env_dir("AAA_CONFIG_DIR").unwrap_or_else(|| default_config_dir(&instance_name("aaa")))
}

/// 프로젝트 홈의 기본 root(host.toml `[projects] root`가 비었을 때): `~/.aaa/projects`
pub fn projects_root_default() -> PathBuf {
    projects_root_default_for(instance().as_deref())
}

/// 다른 인스턴스(`None`은 기본 인스턴스)의 기본 자리. `aaa init`이 겹침을 본다
pub fn projects_root_default_for(instance: Option<&str>) -> PathBuf {
    home().join(named(".aaa", instance)).join("projects")
}
pub fn default_config_dir_for(instance: Option<&str>) -> PathBuf {
    default_config_dir(&named("aaa", instance))
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
    env_dir("AAA_STATE_DIR").unwrap_or_else(|| default_state_dir(&instance_name("aaa")))
}

/// 환경을 바꾸지 않고 다른 인스턴스의 상태 기본 위치를 계산한다(install 읽기 계획).
pub fn default_state_dir_for(instance: Option<&str>) -> PathBuf {
    default_state_dir(&named("aaa", instance))
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
    // 인스턴스는 결정 18 뒤에 생겼다: 옮길 옛 자리가 없다
    if instance().is_some() {
        return out;
    }
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
    /// 이름 없는 기본 인스턴스는 지금 이름과 한 글자도 다르지 않다(plan §8.294)
    #[test]
    fn instance_names_leave_the_default_alone() {
        for base in ["aaa", ".aaa", "aaa-work"] {
            assert_eq!(super::named(base, None), base);
        }
        assert_eq!(super::named("aaa", Some("work")), "aaa-work");
        assert_eq!(super::named(".aaa", Some("work")), ".aaa-work");
        assert_eq!(super::named("aaa-work", Some("work")), "aaa-work-work");
    }

    #[test]
    fn instance_name_rules() {
        for ok in ["work", "w2", "lab-b", "abcdefghijklmnop"] {
            assert_eq!(super::check_instance(ok), Ok(()), "{ok}");
        }
        for bad in ["", "Work", "2w", "-w", "w-", "a--b", "w/x", "../x", "w.x", "한글", "abcdefghijklmnopq", "default"]
        {
            assert!(super::check_instance(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn legacy_names() {
        let (old, new) = (super::default_state_dir("aa"), super::default_state_dir("aaa"));
        assert_ne!(old, new);
    }
}
