//! `aaa setup`: Windows PC 하나를 agent로 설치하고 서비스로 띄워 host에 연결하는 대화형 설치기.
//!
//! 실행 파일을 `aaa-setup.exe`로 이름 붙여 두면 인자 없이 실행(탐색기 더블클릭·SSH)해도 이 마법사가 뜬다.
//! 질문은 host 주소·이 PC 이름·작업 폴더·Windows 암호뿐이고 모두 기본값이 있다(암호 제외). 한 실행 안에서
//! 등록 승인 → 실행 파일 → 설정 → 서비스 → 연결 확인까지 끝낸다. 중간 입력은 그 자리에서 받는다.
//! 참고한 오픈소스(코드는 가져오지 않음): rustup-init(계획 화면·Enter 기본값·끝의 Enter 대기, MIT/Apache-2.0),
//! Tailscale `up`(코드와 승인 안내 뒤 성공 한 줄, BSD-3-Clause), SyncthingWindowsSetup(넣은 로그온 권한만 회수,
//! MPL-2.0), windows-service-rs(SCM 수명·실패 동작, MIT/Apache-2.0), dist installer.ps1(PATH 처리, MIT/Apache-2.0).
use std::path::PathBuf;
use std::process::ExitCode;

#[cfg(any(windows, test))]
mod flow;
#[cfg(windows)]
pub(crate) mod service;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod win;

#[derive(clap::Args, Debug, Default, Clone)]
pub(crate) struct Args {
    /// host 주소 `이름:포트`(포트가 없으면 7430)
    #[arg(long)]
    pub host: Option<String>,
    /// host 목록에 보일 이 PC 이름
    #[arg(long)]
    pub name: Option<String>,
    /// 작업 폴더(thread·worktree가 생기는 곳)
    #[arg(long)]
    pub work_root: Option<PathBuf>,
    /// 질문의 기본값을 그대로 쓴다(Windows 암호는 그래도 묻는다)
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// 설치 상태와 host 연결을 본다
    #[arg(long, conflicts_with = "uninstall")]
    pub status: bool,
    /// 서비스를 지우고 이 설치기가 넣은 권한·PATH를 되돌린다(등록·설정은 남긴다)
    #[arg(long)]
    pub uninstall: bool,
    /// `--uninstall`과 함께: 이 인스턴스의 설정·상태 폴더도 지운다
    #[arg(long, requires = "uninstall")]
    pub purge: bool,
}

/// 실행 파일 이름이 `aaa-setup…`이고 첫 인자가 다른 명령이 아니면 `aaa setup`으로 본다
/// (`aaa-setup.exe`, `aaa-setup.exe --uninstall`, `aaa-setup.exe --instance work`)
pub(crate) fn implied(args: &[std::ffi::OsString], is_command: impl Fn(&str) -> bool) -> bool {
    let Some(program) = args.first().map(|p| p.to_string_lossy()) else { return false };
    let file = program.rsplit(['\\', '/']).next().unwrap_or_default();
    if !file.to_ascii_lowercase().starts_with("aaa-setup") {
        return false;
    }
    match args.get(1).map(|a| a.to_string_lossy()) {
        None => true,
        Some(first) => !(is_command(&first) || matches!(first.as_ref(), "-h" | "--help" | "-V" | "--version")),
    }
}

#[cfg(windows)]
pub(crate) fn run(args: &Args) -> ExitCode {
    win::run(args)
}

/// host가 승인한 독립 배포 감시에서만 서비스 재시작을 실행한다.
#[cfg(windows)]
pub(crate) fn restart_for_rollout(exe: &std::path::Path) -> anyhow::Result<String> {
    win::restart_for_rollout(exe)
}

#[cfg(not(windows))]
pub(crate) fn run(_args: &Args) -> ExitCode {
    eprintln!(
        "aaa setup은 Windows 설치기다. macOS는 `aaa service install --role agent`(docs/install-runbook.md)를 쓴다."
    );
    ExitCode::from(2)
}
