//! `aaa agent`: agent backend 실행과 감독.

use super::*;

#[derive(Args)]
pub(super) struct AgentArgs {
    #[command(subcommand)]
    pub(super) cmd: Option<AgentCmd>,
    /// host 주소("local" 또는 <이름>:<포트>). 기본: agent.toml
    #[arg(long)]
    pub(super) host: Option<String>,
    /// 머신 이름. 기본: agent.toml 또는 hostname
    #[arg(long)]
    pub(super) name: Option<String>,
    /// 개발 모드 표시(host가 자식으로 띄운 로컬 agent)
    #[arg(long)]
    pub(super) dev_mode: bool,
}

#[derive(Subcommand)]
pub(super) enum AgentCmd {
    /// host의 등록 창에 이 머신을 등록한다(확인 코드를 host에서 입력해 승인)
    Enroll {
        /// host 주소 <이름 또는 주소>:<포트>
        host: String,
        #[arg(long)]
        name: Option<String>,
    },
}

pub(super) fn agent_cmd(a: AgentArgs) -> Result<ExitCode> {
    agent_controlled(a, None, None, None)
}

pub(super) fn agent_controlled(
    a: AgentArgs,
    ready: Option<tokio::sync::oneshot::Sender<()>>,
    connection: Option<tokio::sync::watch::Sender<bool>>,
    stop: Option<tokio::sync::watch::Receiver<bool>>,
) -> Result<ExitCode> {
    let cfg = load_or_exit(aaa_config::load_agent(&aaa_config::paths::config_dir()))?;
    let root = aaa_config::paths::state_dir();
    // Unix 인스턴스의 실제 socket 길이를 검사한다. Windows IPC는 경로를 hash한 Named Pipe다.
    #[cfg(unix)]
    if aaa_config::paths::instance().is_some() {
        crate::init::socket_fits(&root)?;
    }
    let state = root.join("agent");
    let ident = aaa_agent::identity::load_or_create(&state)?;
    let name = a
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| Some(cfg.agent.name.clone()).filter(|n| !n.is_empty()))
        .unwrap_or_else(|| aaa_common::util::machine_name_from(&aaa_common::util::hostname()));
    let rt = runtime()?;
    if let Some(AgentCmd::Enroll { host, name: n }) = a.cmd {
        let name = n.unwrap_or(name);
        let pin = rt.block_on(aaa_agent::link::enroll(&state, &host, &ident.machine_id, &name))?;
        println!("등록됐다: {name} → {} (host fingerprint {}…)", pin.address, &pin.fingerprint[..16]);
        println!(
            "{}의 [agent] host = \"{}\"로 두고 `aaa{} agent`를 실행한다",
            aaa_config::paths::config_dir().join("agent.toml").display(),
            pin.address,
            aaa_config::paths::instance_flag()
        );
        return Ok(ExitCode::SUCCESS);
    }
    let host = a
        .host
        .clone()
        .unwrap_or_else(|| if cfg.agent.host.is_empty() { "local".into() } else { cfg.agent.host.clone() });
    let target = if host == "local" {
        aaa_agent::link::Target::Local(root.join("host").join("agent.sock"))
    } else {
        let pin = aaa_agent::identity::load_pin(&state).with_context(|| {
            format!(
                "이 머신은 아직 등록되지 않았다: `aaa{} agent enroll <host:port>`",
                aaa_config::paths::instance_flag()
            )
        })?;
        let id = aaa_common::tls::load_or_create(&state, "aaa-agent")?;
        aaa_agent::link::Target::Remote { addr: host.clone(), pin: pin.fingerprint, id }
    };
    let default_cwd = Some(cfg.agent.default_cwd.clone()).filter(|c| !c.is_empty()).or_else(|| {
        std::env::var("HOME")
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .or_else(|| {
                std::env::var("USERPROFILE")
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
    });
    rt.block_on(async move {
        let agent = aaa_agent::Agent::new(state.clone(), ident.machine_id, name, a.dev_mode, default_cwd)?;
        agent.configure_deployment(crate::deployment::execute)?;
        let home = aaa_config::paths::home();
        let work_root = aaa_config::work_root::resolve(&cfg.agent.work_root, &home).map_err(anyhow::Error::msg)?;
        let work_mount = aaa_config::work_root::mount(&cfg.agent.work_mount, &home).map_err(anyhow::Error::msg)?;
        let roots = aaa_agent::work_root::activate(&state, &work_root, work_mount.as_deref())?;
        let work_mount = roots.roots.iter().find(|r| r.path == work_root).and_then(|r| r.mount.clone());
        let file_roots: Vec<PathBuf> = if cfg.agent.file_roots.is_empty() {
            let mut v = vec![aaa_config::paths::home(), work_root.clone(), std::env::temp_dir()];
            v.extend(roots.roots.iter().map(|r| r.path.clone()));
            v.extend(agent.default_cwd.as_ref().map(PathBuf::from));
            if cfg!(unix) {
                v.push(PathBuf::from("/tmp"));
            }
            v
        } else {
            // `~`·`~/…`는 집 기준
            let home = aaa_config::paths::home();
            let at_home = |r: &str| match r.strip_prefix("~") {
                Some("") => home.clone(),
                Some(rest) if rest.starts_with('/') => home.join(&rest[1..]),
                _ => PathBuf::from(r),
            };
            cfg.agent.file_roots.iter().map(|r| at_home(r)).collect()
        };
        agent.configure_file_roots(file_roots);
        let threads = aaa_agent::threads::ThreadsConfig {
            work_root,
            work_mount,
            state_dir: Some(state.clone()),
            exe: std::env::current_exe()?,
            profiles: cfg.profiles.clone(),
            harnesses: cfg.harnesses.clone(),
            agy: cfg.agy.clone(),
            config_dir: Some(aaa_config::paths::config_dir()),
        };
        aaa_agent::threads::protect_recorded_worktrees(&threads).await?;
        agent.configure_threads(threads);
        tracing::info!(machine = %agent.name.lock().unwrap(), id = %agent.machine_id, %target, "agent 시작");
        aaa_agent::threads::recover_cleanup_locks(&state).await?;
        agent.recover().await?;
        agent.start_hooks()?;
        agent.start_plans();
        aaa_agent::start_selfcheck(&agent, &cfg.selfcheck);
        agent.start_health(cfg.health.clone());
        let max = std::time::Duration::from_secs(cfg.reconnect.max_backoff_s);
        if let Some(ready) = ready {
            aaa_common::diagnostic::ignored(ready.send(()), "수신자가 끝난 뒤에도 현재 작업은 마친다");
        }
        let supervised = stop.is_some();
        tokio::select! {
            _ = aaa_agent::link::run_with_state(agent.clone(), target, max, connection) => {}
            _ = crate::restart::shutdown("agent"), if !supervised => tracing::info!("agent 종료(세션은 session host가 유지한다)"),
            _ = supervised_stop(stop) => tracing::info!("서비스 agent 종료(세션은 session host가 유지한다)"),
        }
        anyhow::Ok(())
    })?;
    Ok(ExitCode::SUCCESS)
}

pub(super) async fn supervised_stop(stop: Option<tokio::sync::watch::Receiver<bool>>) {
    let Some(mut stop) = stop else { return std::future::pending().await };
    loop {
        if *stop.borrow_and_update()
            || stop
                .changed()
                .await
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
        {
            return;
        }
    }
}

/// OS 감독자의 고정 host/agent 역할만 실행한다. 임의 argv/셸을 받지 않는다.
#[cfg(any(windows, unix, test))]
pub(crate) fn supervised_role(
    role: aaa_proto::install::InstallRole,
    ready: tokio::sync::oneshot::Sender<()>,
    connection: tokio::sync::watch::Sender<bool>,
    stop: tokio::sync::watch::Receiver<bool>,
) -> Result<ExitCode> {
    use aaa_common::panic_log::{ProcessKind, set_process};
    set_process(match role {
        aaa_proto::install::InstallRole::Host => ProcessKind::Host,
        aaa_proto::install::InstallRole::Agent => ProcessKind::Agent,
    });
    init_log("info");
    aaa_common::resource::disable_core_dump("supervised service");
    legacy_guard()?;
    match role {
        aaa_proto::install::InstallRole::Host => {
            runtime()?.block_on(aaa_host::run(
                aaa_config::paths::config_dir(),
                aaa_config::paths::state_dir(),
                Some(ready),
                supervised_stop(Some(stop)),
            ))?;
            Ok(ExitCode::SUCCESS)
        }
        aaa_proto::install::InstallRole::Agent => agent_controlled(
            AgentArgs { cmd: None, host: None, name: None, dev_mode: false },
            Some(ready),
            Some(connection),
            Some(stop),
        ),
    }
}
