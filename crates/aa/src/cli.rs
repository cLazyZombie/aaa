//! 명령줄. 역할 실행(`host`·`agent`·`session-host`)과 front 명령을 한 binary에 둔다.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};

use crate::service;
use aa_doctor as doctor;
use aa_proto::{B64, DeviceRole, FrontRequest, FrontResponse, LayoutChange, SplitDir};

#[derive(Parser)]
#[command(name = "aa", version, about = "여러 머신의 agent 오케스트레이션")]
struct Cli {
    /// 설정 폴더(기본: AA_CONFIG_DIR 또는 OS 기본 위치)
    #[arg(long, global = true)]
    config_dir: Option<PathBuf>,
    /// 상태 폴더(기본: AA_STATE_DIR 또는 OS 기본 위치)
    #[arg(long, global = true)]
    state_dir: Option<PathBuf>,
    /// 기계가 읽는 JSON으로 출력
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// host backend 실행(보통 OS 감독자가 띄운다)
    Host,
    /// agent backend 실행, 또는 host에 등록(enroll)
    Agent(AgentArgs),
    /// (내부) 세션 하나의 PTY를 소유하는 프로세스
    #[command(hide = true)]
    SessionHost {
        #[arg(long)]
        dir: PathBuf,
    },
    /// host 상태
    Status,
    /// pane(세션) 목록·실행·읽기·입력
    Pane {
        #[command(subcommand)]
        cmd: PaneCmd,
    },
    /// workspace layout(탭·나누기). 정본은 host에 있다
    Ws {
        #[command(subcommand)]
        cmd: WsCmd,
    },
    /// TUI front: host의 workspace를 터미널에서 보고 조작한다(같은 머신)
    Attach {
        /// workspace 이름(기본 main)
        #[arg(long, short)]
        workspace: Option<String>,
    },
    /// 머신 등록·목록·폐기(host에서)
    Machine {
        #[command(subcommand)]
        cmd: MachineCmd,
    },
    /// host 웹: pairing 코드, tailscale serve HTTPS 공개
    Web {
        #[command(subcommand)]
        cmd: WebCmd,
    },
    /// 자가 점검(계보·키체인·첫 실행 지연)
    Doctor(DoctorArgs),
    /// macOS 감독자(SMAppService LaunchAgent) 등록·해제·상태
    Service {
        #[command(subcommand)]
        cmd: ServiceCmd,
    },
    /// 설정 확인·예시
    Config {
        #[command(subcommand)]
        cmd: ConfigCmd,
    },
}

#[derive(Args)]
struct AgentArgs {
    #[command(subcommand)]
    cmd: Option<AgentCmd>,
    /// host 주소("local" 또는 <이름>:<포트>). 기본: agent.toml
    #[arg(long)]
    host: Option<String>,
    /// 머신 이름. 기본: agent.toml 또는 hostname
    #[arg(long)]
    name: Option<String>,
    /// 개발 모드 표시(host가 자식으로 띄운 로컬 agent)
    #[arg(long)]
    dev_mode: bool,
}

#[derive(Subcommand)]
enum AgentCmd {
    /// host의 등록 창에 이 머신을 등록한다(확인 코드를 host에서 입력해 승인)
    Enroll {
        /// host 주소 <이름 또는 주소>:<포트>
        host: String,
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Subcommand)]
enum PaneCmd {
    /// pane 목록
    List,
    /// 새 pane(세션)을 띄운다. 명령이 없으면 그 머신 사용자의 기본 셸
    Run {
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(long)]
        cols: Option<u16>,
        #[arg(long)]
        rows: Option<u16>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// 보이는 화면을 글자로 읽는다
    Read {
        pane: String,
    },
    /// 입력을 보낸다(문자열 그대로, --enter면 끝에 Enter)
    Send {
        pane: String,
        #[arg(long)]
        text: String,
        #[arg(long)]
        enter: bool,
    },
    Resize {
        pane: String,
        cols: u16,
        rows: u16,
    },
    Kill {
        pane: String,
    },
    /// 끝난(중단·유실·종료) pane을 같은 spec으로 다시 띄운다(layout 자리도 바꾼다)
    Resume {
        pane: String,
    },
    /// 끝난 pane 기록을 지운다
    Forget {
        pane: String,
    },
}

#[derive(Subcommand)]
enum WsCmd {
    /// workspace의 탭·나누기
    Show {
        #[arg(long, short)]
        workspace: Option<String>,
    },
    /// 새 탭에 새 pane(명령이 없으면 기본 셸)
    Tab {
        #[arg(long, short)]
        workspace: Option<String>,
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// pane 자리를 나눠 새 pane을 둔다(h: 좌우, v: 위아래)
    Split {
        pane: String,
        #[arg(long, value_parser = ["h", "v"], default_value = "h")]
        dir: String,
        #[arg(long, short)]
        workspace: Option<String>,
        #[arg(long)]
        machine: Option<String>,
        #[arg(long)]
        cwd: Option<String>,
        #[arg(last = true)]
        argv: Vec<String>,
    },
    /// 이미 있는 pane을 새 탭으로 넣는다
    Add {
        pane: String,
        #[arg(long, short)]
        workspace: Option<String>,
    },
    /// layout에서 뺀다(--kill이면 세션도 끝낸다)
    Close {
        pane: String,
        #[arg(long, short)]
        workspace: Option<String>,
        #[arg(long)]
        kill: bool,
    },
}

#[derive(Subcommand)]
enum MachineCmd {
    List,
    /// 등록 창을 연다(예: --open 10m)
    Enroll {
        #[arg(long, default_value = "10m")]
        open: String,
    },
    /// 승인을 기다리는 등록
    Pending,
    /// agent 화면의 확인 코드로 승인한다
    Approve {
        name: String,
        #[arg(long)]
        code: String,
    },
    Revoke {
        name: String,
    },
}

#[derive(Subcommand)]
enum WebCmd {
    /// 웹 pairing 코드(5분, 1회용)
    Pair {
        /// 보기 권한만 준다
        #[arg(long)]
        reader: bool,
    },
    /// tailscale serve로 HTTPS 공개(off: aa가 만든 공개만 끈다)
    Publish {
        #[arg(value_parser = ["off"])]
        off: Option<String>,
    },
    Status,
}

#[derive(Args)]
struct DoctorArgs {
    #[command(subcommand)]
    cmd: Option<DoctorCmd>,
}

#[derive(Subcommand)]
enum DoctorCmd {
    /// 지금 프로세스 계보에서 측정한다(P0 E1·E2)
    Probe {
        /// 첫 실행 표본 수(새 실행 파일 수)
        #[arg(long, default_value_t = 30)]
        samples: u32,
        /// 실제로 읽어 볼 키체인 generic password의 service 이름
        #[arg(long)]
        keychain_service: Option<String>,
        /// 읽기·쓰기를 확인할 폴더(외장 볼륨)
        #[arg(long)]
        volume_dir: Option<String>,
        /// 결과에 붙일 조건 이름
        #[arg(long)]
        label: Option<String>,
        /// 결과 JSON을 이 파일에 쓴다
        #[arg(long)]
        out: Option<PathBuf>,
        /// warm·처리량을 비교할 기준선 결과 JSON
        #[arg(long)]
        baseline: Option<PathBuf>,
        /// 처리량 측정을 건너뛴다
        #[arg(long)]
        no_throughput: bool,
        /// 전체 디스크 접근(FDA)이 필요한 파일을 읽어 본다
        #[arg(long)]
        fda_check: bool,
        /// 결과를 상태 폴더 probe/results/<label>-<시각>.json에도 쓴다(LaunchAgent 실행용)
        #[arg(long)]
        out_state: bool,
        /// 음성 대조: 책임을 버린(disclaim) 자식으로 다시 실행해 잰다. 느리게 나와야 정상이다.
        #[arg(long)]
        disclaim: bool,
    },
    /// 결과 JSON 여러 개를 한 표로
    Summary { files: Vec<PathBuf> },
    /// 다른 프로세스의 책임 프로세스(E6: codex app-server·빌드 캐시 daemon 등)
    Lineage {
        /// 명령줄에 이 문자열이 들어 있는 프로세스를 찾는다(여러 번)
        #[arg(long = "match", default_values_t = ["app-server".to_string(), "kache".to_string(), "sccache".to_string(), "gradle".to_string(), "bazel".to_string()])]
        patterns: Vec<String>,
    },
}

#[derive(Subcommand)]
enum ServiceCmd {
    /// 바꾸지 않고 무엇을 등록하는지 보인다
    Plan {
        #[arg(long, default_value = service::AGENT_LABEL)]
        label: String,
    },
    /// 등록한다(시스템 설정 변경: 사용자가 직접 실행)
    Register {
        #[arg(long, default_value = service::AGENT_LABEL)]
        label: String,
    },
    Unregister {
        #[arg(long, default_value = service::AGENT_LABEL)]
        label: String,
    },
    Status {
        #[arg(long, default_value = service::AGENT_LABEL)]
        label: String,
    },
    /// 시스템 설정의 로그인 항목 화면을 연다
    OpenSettings,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// host.toml·agent.toml(+ .local.toml)을 검증한다
    Check,
    /// 예시 설정을 출력한다
    Example {
        #[arg(value_parser = ["host", "agent"])]
        role: String,
    },
    /// schema 갱신 계획(지금은 schema 1뿐)
    Migrate,
}

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    if let Some(d) = &cli.config_dir {
        // 하위 프로세스(session host 등)도 같은 위치를 쓰게 한다.
        unsafe { std::env::set_var("AA_CONFIG_DIR", d) };
    }
    if let Some(d) = &cli.state_dir {
        unsafe { std::env::set_var("AA_STATE_DIR", d) };
    }
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("aa: {e:#}");
            ExitCode::from(1)
        }
    }
}

fn init_log(default: &str) {
    let filter = tracing_subscriber::EnvFilter::try_from_env("AA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    use std::io::IsTerminal;
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_target(false)
        .try_init();
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread().enable_all().build()?)
}

fn run(cli: Cli) -> Result<ExitCode> {
    let json = cli.json;
    match cli.cmd {
        Cmd::Host => {
            init_log("info");
            let cfg = load_or_exit(aa_config::load_host(&aa_config::paths::config_dir()))?;
            runtime()?.block_on(aa_host::run(cfg, aa_config::paths::state_dir()))?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Agent(a) => {
            init_log("info");
            agent_cmd(a)
        }
        Cmd::SessionHost { dir } => {
            init_log("info");
            aa_session::host::main(dir)?;
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Status => front(json, FrontRequest::Status),
        Cmd::Pane { cmd } => front(
            json,
            match cmd {
                PaneCmd::List => FrontRequest::Panes,
                PaneCmd::Run { machine, name, cwd, cols, rows, argv } => {
                    FrontRequest::PaneRun { machine, name, argv, cwd, cols, rows }
                }
                PaneCmd::Read { pane } => FrontRequest::PaneRead { pane },
                PaneCmd::Send { pane, text, enter } => {
                    let mut data = text.into_bytes();
                    if enter {
                        data.push(b'\r');
                    }
                    FrontRequest::PaneSend { pane, data: B64(data) }
                }
                PaneCmd::Resize { pane, cols, rows } => FrontRequest::PaneResize { pane, cols, rows },
                PaneCmd::Kill { pane } => FrontRequest::PaneKill { pane },
                PaneCmd::Resume { pane } => FrontRequest::PaneResume { pane },
                PaneCmd::Forget { pane } => FrontRequest::PaneForget { pane },
            },
        ),
        Cmd::Ws { cmd } => front(
            json,
            match cmd {
                WsCmd::Show { workspace } => FrontRequest::Workspace { name: workspace },
                WsCmd::Tab { workspace, machine, name, cwd, argv } => {
                    FrontRequest::LayoutApply { workspace, change: LayoutChange::NewTab { machine, name, argv, cwd } }
                }
                WsCmd::Split { pane, dir, workspace, machine, cwd, argv } => FrontRequest::LayoutApply {
                    workspace,
                    change: LayoutChange::Split {
                        pane,
                        dir: if dir == "v" { SplitDir::Vertical } else { SplitDir::Horizontal },
                        machine,
                        argv,
                        cwd,
                    },
                },
                WsCmd::Add { pane, workspace } => {
                    FrontRequest::LayoutApply { workspace, change: LayoutChange::AddTab { pane } }
                }
                WsCmd::Close { pane, workspace, kill } => {
                    FrontRequest::LayoutApply { workspace, change: LayoutChange::Close { pane, kill } }
                }
            },
        ),
        Cmd::Attach { workspace } => {
            init_log("warn");
            aa_tui::run(workspace)
        }
        Cmd::Machine { cmd } => front(
            json,
            match cmd {
                MachineCmd::List => FrontRequest::Machines,
                MachineCmd::Enroll { open } => FrontRequest::EnrollOpen { minutes: parse_minutes(&open)? },
                MachineCmd::Pending => FrontRequest::EnrollPending,
                MachineCmd::Approve { name, code } => FrontRequest::EnrollApprove { name, code },
                MachineCmd::Revoke { name } => FrontRequest::MachineRevoke { name },
            },
        ),
        Cmd::Web { cmd } => front(
            json,
            match cmd {
                WebCmd::Pair { reader } => {
                    FrontRequest::WebPair { role: if reader { DeviceRole::Reader } else { DeviceRole::Controller } }
                }
                WebCmd::Publish { off: Some(_) } => FrontRequest::WebPublishOff,
                WebCmd::Publish { off: None } => FrontRequest::WebPublish,
                WebCmd::Status => FrontRequest::WebStatus,
            },
        ),
        Cmd::Doctor(a) => doctor_cmd(a, json),
        Cmd::Service { cmd } => service_cmd(cmd),
        Cmd::Config { cmd } => config_cmd(cmd, json),
    }
}

fn doctor_cmd(a: DoctorArgs, json: bool) -> Result<ExitCode> {
    match a.cmd {
        None => {
            let r = doctor::probe(&doctor::ProbeOptions { samples: 3, throughput: false, ..Default::default() });
            print_report(&r, json)?;
            Ok(ExitCode::SUCCESS)
        }
        Some(DoctorCmd::Probe { disclaim: true, .. }) => {
            #[cfg(target_os = "macos")]
            {
                let args: Vec<String> = std::env::args().filter(|a| a != "--disclaim").collect();
                let code = doctor::macos::rerun_disclaimed(&args)?;
                Ok(ExitCode::from(code as u8))
            }
            #[cfg(not(target_os = "macos"))]
            anyhow::bail!("--disclaim은 macOS 전용이다")
        }
        Some(DoctorCmd::Probe {
            samples,
            keychain_service,
            volume_dir,
            label,
            out,
            baseline,
            no_throughput,
            fda_check,
            out_state,
            disclaim: false,
        }) => {
            let mut r = doctor::probe(&doctor::ProbeOptions {
                samples,
                keychain_service,
                volume_dir,
                label,
                throughput: !no_throughput,
                activity_held: false,
                fda_check,
            });
            if let Some(b) = baseline {
                let base: doctor::ProbeReport =
                    aa_common::util::read_json(&b).with_context(|| format!("기준선 {}", b.display()))?;
                let worse = doctor::compare_throughput(&base, &r);
                if !worse.is_empty() {
                    r.reasons.extend(worse.into_iter().map(|w| format!("처리량 gate 실패: {w}")));
                    r.pass = false;
                }
            }
            let mut outs: Vec<PathBuf> = out.into_iter().collect();
            if out_state {
                let name = format!("{}-{}.json", r.label.clone().unwrap_or("probe".into()), r.created_at_ms);
                outs.push(aa_config::paths::state_dir().join("probe/results").join(name));
            }
            for out in &outs {
                if let Some(dir) = out.parent() {
                    std::fs::create_dir_all(dir)?;
                }
                std::fs::write(out, serde_json::to_vec_pretty(&r)?)?;
            }
            print_report(&r, json)?;
            Ok(if r.pass { ExitCode::SUCCESS } else { ExitCode::from(2) })
        }
        Some(DoctorCmd::Summary { files }) => {
            let mut rows = Vec::new();
            for f in files {
                let r: doctor::ProbeReport =
                    aa_common::util::read_json(&f).with_context(|| format!("{}", f.display()))?;
                rows.push((f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), r));
            }
            print!("{}", doctor::summary_table(&rows));
            Ok(ExitCode::SUCCESS)
        }
        Some(DoctorCmd::Lineage { patterns }) => {
            #[cfg(target_os = "macos")]
            {
                let rows = doctor::macos::daemons(&patterns);
                if json {
                    let v: Vec<_> = rows
                        .iter()
                        .map(|(pid, cmd, rp, path)| serde_json::json!({"pid":pid,"command":cmd,"responsible_pid":rp,"responsible_path":path}))
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&v)?);
                } else if rows.is_empty() {
                    println!("해당 프로세스 없음: {patterns:?}");
                } else {
                    for (pid, cmd, rp, path) in rows {
                        println!("{pid}\t책임 {}\t{}\n\t{cmd}", rp.unwrap_or(-1), path.unwrap_or_default());
                    }
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (patterns, json);
                println!("macOS에서만 의미가 있다");
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn load_or_exit<T>(r: Result<aa_config::Loaded<T>, aa_config::ConfigErrors>) -> Result<T> {
    match r {
        Ok(l) => {
            for w in &l.warnings {
                eprintln!("{w}");
            }
            Ok(l.config)
        }
        Err(e) => anyhow::bail!("설정을 읽지 못해 시작하지 않는다:\n{}", e.render()),
    }
}

fn parse_minutes(s: &str) -> Result<u32> {
    let t = s.trim_end_matches('m');
    t.parse::<u32>().with_context(|| format!("분 단위로 적는다(예: 10m): {s}"))
}

fn agent_cmd(a: AgentArgs) -> Result<ExitCode> {
    let cfg = load_or_exit(aa_config::load_agent(&aa_config::paths::config_dir()))?;
    let root = aa_config::paths::state_dir();
    let state = root.join("agent");
    let ident = aa_agent::identity::load_or_create(&state)?;
    let name = a
        .name
        .clone()
        .filter(|n| !n.is_empty())
        .or_else(|| Some(cfg.agent.name.clone()).filter(|n| !n.is_empty()))
        .unwrap_or_else(aa_common::util::hostname);
    let rt = runtime()?;
    if let Some(AgentCmd::Enroll { host, name: n }) = a.cmd {
        let name = n.unwrap_or(name);
        let pin = rt.block_on(aa_agent::link::enroll(&state, &host, &ident.machine_id, &name))?;
        println!("등록됐다: {name} → {} (host fingerprint {}…)", pin.address, &pin.fingerprint[..16]);
        println!("agent.toml의 [agent] host = \"{}\"로 두고 `aa agent`를 실행한다", pin.address);
        return Ok(ExitCode::SUCCESS);
    }
    let host = a
        .host
        .clone()
        .unwrap_or_else(|| if cfg.agent.host.is_empty() { "local".into() } else { cfg.agent.host.clone() });
    let target = if host == "local" {
        aa_agent::link::Target::Local(root.join("host").join("agent.sock"))
    } else {
        let pin = aa_agent::identity::load_pin(&state)
            .context("이 머신은 아직 등록되지 않았다: `aa agent enroll <host:port>`")?;
        let id = aa_common::tls::load_or_create(&state, "aa-agent")?;
        aa_agent::link::Target::Remote { addr: host.clone(), pin: pin.fingerprint, id }
    };
    let default_cwd = Some(cfg.agent.default_cwd.clone())
        .filter(|c| !c.is_empty())
        .or_else(|| std::env::var("HOME").ok().or_else(|| std::env::var("USERPROFILE").ok()));
    rt.block_on(async move {
        let agent = aa_agent::Agent::new(state, ident.machine_id, name, a.dev_mode, default_cwd)?;
        tracing::info!(machine = %agent.name, id = %agent.machine_id, %target, "agent 시작");
        agent.recover().await?;
        aa_agent::start_selfcheck(&agent, &cfg.selfcheck);
        let max = std::time::Duration::from_secs(cfg.reconnect.max_backoff_s);
        tokio::select! {
            _ = aa_agent::link::run(agent.clone(), target, max) => {}
            _ = aa_host::shutdown_signal() => tracing::info!("agent 종료(세션은 session host가 유지한다)"),
        }
        anyhow::Ok(())
    })?;
    Ok(ExitCode::SUCCESS)
}

fn front(json: bool, req: FrontRequest) -> Result<ExitCode> {
    let rt = runtime()?;
    let res = rt.block_on(async {
        let mut c = aa_common::front::FrontClient::connect(&aa_config::paths::state_dir()).await?;
        c.call(req).await
    })?;
    match res {
        Ok(r) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", render_front(&r));
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            if json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({ "error": e }))?);
            } else {
                eprintln!("aa: {e}");
            }
            Ok(ExitCode::from(3))
        }
    }
}

fn state_label(s: &aa_proto::SessionState) -> String {
    match s {
        aa_proto::SessionState::Running => "실행".into(),
        aa_proto::SessionState::Exited { code } => {
            format!("종료({})", code.map(|c| c.to_string()).unwrap_or("?".into()))
        }
        aa_proto::SessionState::Interrupted => "중단(재부팅)".into(),
        aa_proto::SessionState::Lost => "유실".into(),
    }
}

fn render_front(r: &FrontResponse) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let machines = |s: &mut String, ms: &[aa_proto::MachineView]| {
        for m in ms {
            let _ = writeln!(
                s,
                "{}\t{}\t{}{}{}",
                m.name,
                if m.connection == aa_proto::Connection::Connected { "연결" } else { "끊김" },
                m.os.clone().unwrap_or_default(),
                if m.local { "\t로컬" } else { "" },
                if m.dev_mode { "\t개발 모드" } else { "" }
            );
        }
    };
    match r {
        FrontResponse::Status(h) => {
            let _ = writeln!(
                s,
                "host {} · {} · {}",
                h.host_id,
                h.software,
                if h.dev_mode { "개발 모드(로컬 agent를 host가 띄움)" } else { "운영 모드" }
            );
            let _ = writeln!(
                s,
                "Agent link: {} · 웹: {}",
                h.agent_listen.clone().unwrap_or("-".into()),
                h.web_listen.clone().unwrap_or("-".into())
            );
            machines(&mut s, &h.machines);
        }
        FrontResponse::Machines { machines: ms } => machines(&mut s, ms),
        FrontResponse::Panes { panes } => {
            for p in panes {
                let _ = writeln!(
                    s,
                    "{}\t{}\t{}\t{}{}\t{}",
                    p.id,
                    p.machine,
                    p.name,
                    state_label(&p.state),
                    if p.connection == aa_proto::Connection::Disconnected { "·끊김(읽기 전용)" } else { "" },
                    p.argv.join(" ")
                );
            }
        }
        FrontResponse::Pane(p) => {
            let _ = writeln!(s, "{}\t{}\t{}\t{}", p.id, p.machine, p.name, state_label(&p.state));
        }
        FrontResponse::Screen(t) => {
            let mut lines = t.lines.clone();
            while lines.last().is_some_and(|l| l.is_empty()) {
                lines.pop();
            }
            for l in lines {
                let _ = writeln!(s, "{l}");
            }
        }
        FrontResponse::Pending { pending } => {
            if pending.is_empty() {
                let _ = writeln!(s, "대기 중인 등록이 없다");
            }
            for p in pending {
                let _ = writeln!(
                    s,
                    "{}\tfingerprint {}…\t`aa machine approve {} --code <agent 화면의 코드>`",
                    p.name, p.fingerprint_prefix, p.name
                );
            }
        }
        FrontResponse::PairCode { code, role, expires_in_s } => {
            let _ = writeln!(s, "pairing 코드: {code} ({role:?}, {}분 유효, 1회용)", expires_in_s / 60);
        }
        FrontResponse::Publish(p) => {
            let _ = writeln!(
                s,
                "tailscale: {}{}",
                p.tailscale,
                p.dns_name.as_ref().map(|d| format!(" ({d})")).unwrap_or_default()
            );
            let _ = writeln!(s, "주소: {}", p.url.clone().unwrap_or("-".into()));
            if let Some(port) = p.owned_port {
                let _ = writeln!(s, "aa가 만든 serve 포트: {port} → 127.0.0.1:{}", p.web_port);
            }
            if let Some(m) = &p.message {
                let _ = writeln!(s, "{m}");
            }
        }
        FrontResponse::Workspace(w) => {
            if w.tabs.is_empty() {
                let _ = writeln!(s, "workspace {}: 탭 없음(`aa ws tab`으로 만든다)", w.name);
            }
            for (i, t) in w.tabs.iter().enumerate() {
                let _ = writeln!(s, "탭 {}\t{}", i + 1, render_node(&t.root));
            }
        }
        FrontResponse::Done => s.push_str("완료\n"),
    }
    s
}

fn render_node(n: &aa_proto::LayoutNode) -> String {
    match n {
        aa_proto::LayoutNode::Pane { pane } => pane.clone(),
        aa_proto::LayoutNode::Split { dir, a, b, .. } => {
            let sep = if *dir == SplitDir::Horizontal { " | " } else { " / " };
            format!("({}{sep}{})", render_node(a), render_node(b))
        }
    }
}

fn print_report(r: &doctor::ProbeReport, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(r)?);
    } else {
        print!("{}", doctor::render(r));
    }
    Ok(())
}

fn service_cmd(cmd: ServiceCmd) -> Result<ExitCode> {
    match cmd {
        ServiceCmd::Plan { label } => print!("{}", service::plan(&label)?),
        ServiceCmd::Register { label } => println!("{label}: {}", service::register(&label)?),
        ServiceCmd::Unregister { label } => println!("{label}: {}", service::unregister(&label)?),
        ServiceCmd::Status { label } => println!("{label}: {}", service::status(&label)?),
        ServiceCmd::OpenSettings => service::open_settings(),
    }
    Ok(ExitCode::SUCCESS)
}

fn config_cmd(cmd: ConfigCmd, json: bool) -> Result<ExitCode> {
    let dir = aa_config::paths::config_dir();
    match cmd {
        ConfigCmd::Example { role } => {
            print!("{}", aa_config::example(&role));
            Ok(ExitCode::SUCCESS)
        }
        ConfigCmd::Migrate => {
            for stem in ["host", "agent"] {
                match aa_config::migrate_plan(&dir, stem) {
                    Ok(m) => println!("{m}"),
                    Err(e) => println!("{}", e.render()),
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ConfigCmd::Check => {
            let mut ok = true;
            let mut report = Vec::new();
            let host = aa_config::load_host(&dir).map(|l| (l.files, l.warnings));
            let agent = aa_config::load_agent(&dir).map(|l| (l.files, l.warnings));
            for (name, r) in [("host", host), ("agent", agent)] {
                match r {
                    Ok((files, warnings)) => {
                        report.push(serde_json::json!({"role":name,"ok":true,
                            "files":files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>(),
                            "warnings":warnings.iter().map(|w| w.to_string()).collect::<Vec<_>>()}));
                        if !json {
                            let used = if files.is_empty() {
                                "파일 없음(기본값)".to_string()
                            } else {
                                files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ")
                            };
                            println!("{name}: 정상 · {used}");
                            for w in warnings {
                                println!("  {w}");
                            }
                        }
                    }
                    Err(e) => {
                        ok = false;
                        report.push(serde_json::json!({"role":name,"ok":false,"errors":e.0.iter().map(|d| d.to_string()).collect::<Vec<_>>()}));
                        if !json {
                            println!("{name}: 오류\n{}", e.render());
                        }
                    }
                }
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            }
            Ok(if ok { ExitCode::SUCCESS } else { ExitCode::from(2) })
        }
    }
}
