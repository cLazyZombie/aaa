//! 고정 backend를 실행하고 실제 준비 사건 뒤에만 private proof를 공개한다.
use super::*;

fn publish(spec: &Spec, session: u32, elevated: bool, created: u64, connected: bool) -> Result<()> {
    let ready = Ready {
        schema: 1,
        context_sha256: spec.sha(),
        account_sha256: hash(spec.account.as_bytes()),
        profile_sha256: hash(spec.profile.to_string_lossy().as_bytes()),
        binary_sha256: spec.binary_sha256.clone(),
        pid: std::process::id(),
        connected,
        process_created: created,
        session,
        elevated,
        boot: aaa_common::util::boot_id(),
    };
    private::write(&spec.directory().join("ready.json"), &serde_json::to_vec(&ready).map_err(|_| Fault::conflict())?)
        .map_err(Fault::io)
}

pub(super) fn run(
    spec: &Spec,
    session: u32,
    elevated: bool,
    created: u64,
    stop: tokio::sync::watch::Sender<bool>,
    running_state: impl Fn() -> bool,
) -> Result<()> {
    if *stop.borrow() {
        return Ok(());
    }
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(Fault::io)?;
    let (ready, receiver) = tokio::sync::oneshot::channel();
    let (connection, mut state) = tokio::sync::watch::channel(false);
    let (ended, mut done) = tokio::sync::oneshot::channel();
    let role = spec.role;
    let mut stop_rx = stop.subscribe();
    let backend_stop = stop_rx.clone();
    let backend = std::thread::Builder::new()
        .name("aaa-service-role".into())
        .spawn(move || {
            let result = crate::cli::supervised_role(role, ready, connection, backend_stop);
            let _ = ended.send(());
            result
        })
        .map_err(Fault::io)?;
    let prepared = rt.block_on(async {
        tokio::select! {
            biased;
            _ = stop_rx.wait_for(|stopped| *stopped) => false,
            result = tokio::time::timeout(std::time::Duration::from_secs(30), receiver) => matches!(result, Ok(Ok(()))),
        }
    });
    let mut fault = None;
    if !prepared && !*stop.borrow() {
        fault = Some(failure("service_backend_not_ready", 3));
    }
    let mut announce = || {
        let connected = role == InstallRole::Host || *state.borrow_and_update();
        publish(spec, session, elevated, created, connected).is_ok() && running_state()
    };
    if prepared && !*stop.borrow() && !announce() && !*stop.borrow() {
        fault = Some(failure("service_status_failed", 1));
    }
    if fault.is_none() && !*stop.borrow() {
        rt.block_on(async {
            loop {
                tokio::select! {
                    biased;
                    _ = stop_rx.wait_for(|stopped| *stopped) => break,
                    _ = &mut done => break,
                    changed = state.changed(), if role == InstallRole::Agent => {
                        if changed.is_err() { break }
                        let connected = *state.borrow_and_update();
                        if (publish(spec, session, elevated, created, connected).is_err() || !running_state()) && !*stop.borrow() {
                            fault = Some(failure("service_status_failed", 1));
                            break;
                        }
                    }
                }
            }
        });
    }
    if fault.is_some() {
        let _ = stop.send(true);
    }
    let result = backend.join();
    // 다음 process의 proof를 지우지 않도록 현재 context/PID를 확인한다.
    let path = spec.directory().join("ready.json");
    if private::read(&path, 128 * 1024)
        .ok()
        .and_then(|b| serde_json::from_slice::<Ready>(&b).ok())
        .is_some_and(|proof| proof.pid == std::process::id() && proof.context_sha256 == spec.sha())
    {
        std::fs::remove_file(&path).map_err(Fault::io)?;
        private::sync_parent(&path).map_err(Fault::io)?;
    }
    if let Some(fault) = fault {
        return Err(fault);
    }
    match result {
        Ok(Ok(code)) if code == std::process::ExitCode::SUCCESS && *stop.borrow() => Ok(()),
        _ => Err(failure("service_backend_failed", 1)),
    }
}
