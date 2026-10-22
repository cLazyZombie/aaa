//! launchd → 고정 launcher → aaa 자식만 backend를 시작한다.
use super::*;

fn read_spec(context: &Path, sha: &str) -> Result<Spec> {
    if !context.is_absolute() || !digest_ok(sha) {
        return Err(Fault::conflict());
    }
    let bytes = private::read(context, 128 * 1024).map_err(Fault::io)?;
    if hash(&bytes) != sha {
        return Err(Fault::conflict());
    }
    let spec: Spec = serde_json::from_slice(&bytes).map_err(|_| Fault::conflict())?;
    spec.validate()?;
    spec.guard_paths()?;
    spec.verify_files()?;
    if spec.launchd.is_none()
        || spec.context() != context
        || spec.sha() != sha
        || hash(spec.account.as_bytes()) != account_hash()?
    {
        return Err(Fault::conflict());
    }
    Ok(spec)
}
pub(super) fn dispatch(context: &Path, sha: &str) -> std::process::ExitCode {
    match entry(context, sha) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(fault) => {
            eprintln!("{}", fault.code);
            std::process::ExitCode::from(fault.exit)
        }
    }
}
fn entry(context: &Path, sha: &str) -> Result<()> {
    let (parent, _) =
        native::identity(unsafe { libc::getppid() } as u32).map_err(|_| failure("service_controller_required", 3))?;
    if parent.pbi_ppid != 1 || parent.pbi_uid != unsafe { libc::geteuid() } {
        return Err(failure("service_controller_required", 3));
    }
    let spec = read_spec(context, sha)?;
    native::process(&spec, std::process::id(), parent.pbi_pid)?;
    // 외부 명령의 reader/signal/runtime thread를 만들기 전에 부모 image/receipt를 대조하고,
    // 아직 단일 thread인 새 service process의 환경만 설정한다.
    unsafe {
        for (key, _) in std::env::vars_os() {
            std::env::remove_var(key);
        }
        for (key, value) in environment(&spec) {
            std::env::set_var(key, value);
        }
    }
    std::env::set_current_dir(&spec.profile).map_err(Fault::io)?;
    native::verify_launcher(&spec)?;
    let (session, process) = native::own_process(&spec)?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(Fault::io)?;
    let (stop, _) = tokio::sync::watch::channel(false);
    let (finished, mut done) = tokio::sync::oneshot::channel::<()>();
    let signal_stop = stop.clone();
    // backend 시작 전에 signal 구독을 고정한다. 고정 sleep이나 signal 무시는 없다.
    let (mut term, mut interrupt, mut restart) = {
        let _entered = rt.enter();
        (
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).map_err(Fault::io)?,
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).map_err(Fault::io)?,
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined2()).map_err(Fault::io)?,
        )
    };
    let registration =
        crate::restart::register(spec.role(), Some((spec.executable.clone(), spec.binary_sha256.clone())))
            .map_err(|_| failure("service_restart_not_ready", 3))?;
    crate::restart::signals_ready().map_err(Fault::io)?;
    let restart_context = context.to_path_buf();
    let restart_sha = sha.to_string();
    let signals = std::thread::spawn(move || {
        rt.block_on(async {
            loop {
                tokio::select! {
                    _ = term.recv() => { let _ = signal_stop.send(true); break; },
                    _ = interrupt.recv() => { let _ = signal_stop.send(true); break; },
                    _ = restart.recv(), if registration.is_some() => {
                        // 설치 서비스의 승인 SHA가 달라졌으면 새 승인이 먼저다. 원래 backend를 살려 둔다.
                        if read_spec(&restart_context, &restart_sha).is_ok() {
                            crate::restart::request();
                            let _ = signal_stop.send(true);
                            break;
                        }
                        eprintln!("승인된 설치 파일과 달라 worker 재시작을 거부했다. 새 install 승인이 필요하다");
                    },
                    _ = &mut done => break
                }
            }
        })
    });
    let result = runner::run(&spec, session, process.elevated, process.created, stop, || true);
    let _ = finished.send(());
    let _ = signals.join();
    result
}
