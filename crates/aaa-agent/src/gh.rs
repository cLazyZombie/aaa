//! PR·CI가 쓰는 gh 실행 경계. 자격 증명은 gh에 맡기고 파일·토큰을 읽지 않는다.
use anyhow::{Context, Result, bail};
use std::{path::PathBuf, process::ExitStatus};

/// PR만 조용히 생략할 수 있는 실행 환경. CI는 이 오류도 계속 알린다.
#[derive(Debug)]
pub(crate) enum Unavailable {
    Missing,
    Authentication,
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Missing => "gh 실행 파일 없음: 조회 머신 agent의 PATH·gh 설치를 확인한다",
            Self::Authentication => "인증 실패(GH_TOKEN은 선택): agent gh 로그인·HOME·설정 경로 확인",
        })
    }
}
impl std::error::Error for Unavailable {}

fn find(
    dirs: impl IntoIterator<Item = PathBuf>,
    defaults: impl IntoIterator<Item = PathBuf>,
) -> Result<PathBuf, Unavailable> {
    let name = if cfg!(windows) { "gh.exe" } else { "gh" };
    dirs.into_iter()
        .chain(defaults)
        .map(|dir| dir.join(name))
        .find(|path| aaa_config::profile_edit::executable_available(&path.to_string_lossy(), None))
        .ok_or(Unavailable::Missing)
}

pub(crate) fn command() -> Result<tokio::process::Command> {
    let cwd = std::env::current_dir().ok();
    let paths = std::env::var_os("PATH").unwrap_or_default();
    let dirs = std::env::split_paths(&paths)
        .filter_map(|p| if p.is_absolute() { Some(p) } else { cwd.as_ref().map(|cwd| cwd.join(p)) });
    // launchd의 짧은 PATH도 Homebrew 표준 설치를 찾는다. 사용자 PATH가 언제나 먼저다.
    let defaults: &[&str] = if cfg!(target_os = "macos") { &["/opt/homebrew/bin", "/usr/local/bin"] } else { &[] };
    let exe = find(dirs, defaults.iter().map(PathBuf::from))?;
    let mut cmd = tokio::process::Command::new(exe);
    // 모든 요청은 URL·--repo·API 경로로 대상을 지정한다. origin을 읽은 뒤 worktree가 사라져도 gh 실행은 된다.
    let exe = std::env::current_exe().context("agent 실행 경로를 읽지 못했다")?;
    let root = exe.ancestors().last().context("agent 실행 경로의 root가 없다")?;
    cmd.current_dir(root).env("GH_PROMPT_DISABLED", "1");
    Ok(cmd)
}

pub(crate) fn check(status: ExitStatus) -> Result<()> {
    if status.success() {
        return Ok(());
    }
    // stderr에는 토큰·개인 경로·외부 응답이 섞일 수 있다. 종료 코드만으로 진단하고 원문은 전달하지 않는다.
    if status.code() == Some(4) {
        return Err(Unavailable::Authentication.into());
    }
    match status.code() {
        Some(code) => bail!("요청 실패(종료 코드 {code}): 조회 머신의 네트워크·권한·API 상태를 확인한다"),
        None => bail!("조회 머신에서 gh가 신호로 끝났다"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_search_keeps_path_priority_and_finds_a_service_fallback() {
        let dir = std::env::temp_dir().join(format!("aaa-gh-path-{}", aaa_common::util::new_id()));
        let custom = dir.join("custom");
        let standard = dir.join("standard");
        let name = if cfg!(windows) { "gh.exe" } else { "gh" };
        for p in [&custom, &standard] {
            std::fs::create_dir_all(p).unwrap();
            std::fs::write(p.join(name), "fixture").unwrap();
            #[cfg(unix)]
            std::fs::set_permissions(p.join(name), std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        }
        let missing = dir.join("missing");
        assert_eq!(find([custom.clone()], [standard.clone()]).unwrap(), custom.join(name));
        assert_eq!(find([missing.clone()], [standard.clone()]).unwrap(), standard.join(name));
        assert!(matches!(find([missing], []), Err(Unavailable::Missing)));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
