//! 감독자가 실행한 agent. 시작·연결·정지 사건을 같은 setup 상태 파일로 전달한다.
use super::super::{ServiceKind, flow::ServiceStatus};
use super::service::{Manager, Spec};
use anyhow::{Context, Result, ensure};
use std::process::ExitCode;
use std::time::Duration;

pub(crate) fn dispatch() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("aaa setup-service: {error:#}");
            ExitCode::FAILURE
        }
    }
}
fn run() -> Result<ExitCode> {
    let state = aaa_config::paths::state_dir();
    let spec: Spec =
        serde_json::from_slice(&std::fs::read(state.join("setup-unix.json")).context("서비스 기록 읽기")?)?;
    spec.validate()?;
    ensure!(
        spec.uid == unsafe { libc::getuid() }
            && spec.state == state
            && spec.config == aaa_config::paths::config_dir()
            && spec.home == aaa_config::paths::home()
            && spec.instance == aaa_config::paths::instance()
            && spec.exe.canonicalize()? == std::env::current_exe()?.canonicalize()?,
        "서비스 실행 계정·경로·인스턴스가 설치 기록과 다릅니다"
    );
    let manager = Manager { spec };
    let owner = if manager.spec.kind == ServiceKind::Launchd {
        (unsafe { libc::getppid() }) as u32
    } else {
        std::process::id()
    };
    let observed = manager.query()?.context("서비스 관리자가 이 서비스를 실행하지 않았습니다")?;
    ensure!(observed.running && observed.pid == owner, "setup-service는 등록된 감독자가 실행해야 합니다");
    let (stop, stop_rx) = tokio::sync::watch::channel(false);
    let (ready, ready_rx) = tokio::sync::oneshot::channel();
    let (connection, mut connected) = tokio::sync::watch::channel(false);
    let (ended, mut done) = tokio::sync::oneshot::channel();
    let backend = std::thread::Builder::new().name("aaa-setup-agent".into()).spawn(move || {
        let result = crate::cli::supervised_role(aaa_proto::install::InstallRole::Agent, ready, connection, stop_rx);
        let _ = ended.send(());
        result
    })?;
    let path = state.join("setup-service.json");
    let write_status = |connected| -> Result<()> {
        aaa_common::util::write_json(
            &path,
            &ServiceStatus { pid: owner, connected, changed_ms: aaa_common::util::now_ms() },
        )
        .context("서비스 연결 상태 기록")
    };
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let outcome: Result<()> = rt.block_on(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
        tokio::select! {
            _ = term.recv() => return Ok(()),
            _ = interrupt.recv() => return Ok(()),
            result = tokio::time::timeout(Duration::from_secs(60), ready_rx) => {
                result.context("agent 준비 60초 만료")?.context("agent 준비 전에 종료됨")?;
            }
        }
        write_status(*connected.borrow_and_update())?;
        loop {
            tokio::select! {
                _ = term.recv() => return Ok(()),
                _ = interrupt.recv() => return Ok(()),
                _ = &mut done => anyhow::bail!("agent가 서비스 정지 요청 전에 종료됐습니다"),
                changed = connected.changed() => {
                    changed.context("agent 연결 상태 채널 종료")?;
                    write_status(*connected.borrow_and_update())?;
                }
            }
        }
    });
    let _ = stop.send(true);
    let result = backend.join().map_err(|_| anyhow::anyhow!("agent 서비스 thread panic"))?;
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => eprintln!("서비스 연결 상태 정리 실패: {e}"),
    }
    outcome?;
    result
}
