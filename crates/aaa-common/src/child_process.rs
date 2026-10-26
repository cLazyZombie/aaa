//! macOS의 pipe() → fcntl(CLOEXEC) 사이에 다른 자식이 FD를 물려받지 않게 한다.
//! 모든 제품 spawn(PTY·직접 posix_spawn 포함)이 같은 잠금을 사용해야 한다.
//! 잠금 안에서는 생성만 하고, wait·출력 읽기·await는 반드시 밖에서 한다.

use std::{
    io,
    process::{Child, Command, ExitStatus, Output, Stdio},
};

#[cfg(target_os = "macos")]
static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// macOS에서만 자식 생성 구간을 직렬화한다. Linux의 pipe2·Windows std의 생성 잠금은 그대로 쓴다.
/// fork 뒤 pre_exec 안에서 호출하면 안 된다(부모가 잡은 잠금을 물려받는다).
pub fn with_spawn_lock<T>(spawn: impl FnOnce() -> T) -> T {
    #[cfg(target_os = "macos")]
    let _guard = { SPAWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner) };
    spawn()
}

#[cfg(all(test, target_os = "macos"))]
mod tests;

pub trait StdCommandExt {
    fn spawn_guarded(&mut self) -> io::Result<Child>;
    fn status_guarded(&mut self) -> io::Result<ExitStatus>;
    /// stdin을 닫고 stdout·stderr를 모두 수집한다. 명시적 stdio 재지정이므로
    /// 입력을 전달하거나 일부 출력만 수집할 때는 spawn_guarded를 쓴다.
    fn capture_output(&mut self) -> io::Result<Output>;
}

impl StdCommandExt for Command {
    fn spawn_guarded(&mut self) -> io::Result<Child> {
        with_spawn_lock(|| self.spawn())
    }

    fn status_guarded(&mut self) -> io::Result<ExitStatus> {
        let mut child = self.spawn_guarded()?;
        // std Command::status처럼 수집하지 않는 pipe는 기다리기 전에 닫는다.
        drop(child.stdin.take());
        drop(child.stdout.take());
        drop(child.stderr.take());
        child.wait()
    }

    fn capture_output(&mut self) -> io::Result<Output> {
        self.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        self.spawn_guarded()?.wait_with_output()
    }
}

pub trait TokioCommandExt {
    fn spawn_guarded(&mut self) -> io::Result<tokio::process::Child>;
    fn status_guarded(&mut self) -> impl Future<Output = io::Result<ExitStatus>> + Send + use<Self>;
    fn output_guarded(&mut self) -> impl Future<Output = io::Result<Output>> + Send + use<Self>;
}

impl TokioCommandExt for tokio::process::Command {
    fn spawn_guarded(&mut self) -> io::Result<tokio::process::Child> {
        with_spawn_lock(|| self.spawn())
    }

    fn status_guarded(&mut self) -> impl Future<Output = io::Result<ExitStatus>> + Send + use<> {
        let child = self.spawn_guarded();
        async move { child?.wait().await }
    }

    fn output_guarded(&mut self) -> impl Future<Output = io::Result<Output>> + Send + use<> {
        // Tokio Command::output과 같이 출력만 재지정하고 설정한 stdin·kill_on_drop은 유지한다.
        self.stdout(Stdio::piped()).stderr(Stdio::piped());
        let child = self.spawn_guarded();
        async move { child?.wait_with_output().await }
    }
}
