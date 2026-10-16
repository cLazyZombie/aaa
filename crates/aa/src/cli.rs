//! 명령줄. 역할 실행(`host`·`agent`·`session-host`)과 front 명령을 한 binary에 둔다.

use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand};

use aa::doctor;
use aa::service;

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

fn run(cli: Cli) -> Result<ExitCode> {
    let json = cli.json;
    match cli.cmd {
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
                    aa::util::read_json(&b).with_context(|| format!("기준선 {}", b.display()))?;
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
                let r: doctor::ProbeReport = aa::util::read_json(&f).with_context(|| format!("{}", f.display()))?;
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
