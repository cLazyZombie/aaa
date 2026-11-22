//! service·config 명령.

use super::*;

#[derive(Subcommand)]
pub(super) enum ServiceCmd {
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
    /// 운영 감독자(결정 14: LaunchAgent → aaa-launcher → aaa <role>)를 바꾸지 않고 보인다: plist·경로·launchd 상태
    Show {
        #[arg(long, value_parser = ["host", "agent"])]
        role: String,
        /// aaa 실행 파일(기본: 지금 실행 중인 aaa). 다시 빌드해도 바뀌지 않는 고정 경로를 쓴다.
        #[arg(long)]
        aaa: Option<PathBuf>,
        /// launcher 실행 파일(기본: ~/Applications/aaa-launcher.app/Contents/MacOS/aaa-launcher)
        #[arg(long)]
        launcher: Option<PathBuf>,
    },
    /// 운영 감독자를 설치한다(~/Library/LaunchAgents에 plist를 쓰고 launchctl bootstrap). 사람이 터미널에서 직접 한다.
    Install {
        #[arg(long, value_parser = ["host", "agent"])]
        role: String,
        #[arg(long)]
        aaa: Option<PathBuf>,
        #[arg(long)]
        launcher: Option<PathBuf>,
        /// 이미 있으면 내리고 바꾼다
        #[arg(long)]
        replace: bool,
    },
    /// 운영 감독자를 내리고 plist를 지운다. 사람이 터미널에서 직접 한다.
    Uninstall {
        #[arg(long, value_parser = ["host", "agent"])]
        role: String,
    },
    /// 감독자가 aaa를 끝내고 다시 띄운다(launchctl kickstart -k, 업그레이드 뒤). 세션은 이어진다. 사람이 터미널에서 직접 한다.
    Restart {
        #[arg(long, value_parser = ["host", "agent"])]
        role: String,
        #[arg(long)]
        machine: Option<String>,
    },
    /// 감독자에서 내린다(launchctl bootout, herdr `server stop`). plist는 남아 다음 로그인에 뜬다. 사람이 터미널에서 직접 한다.
    Stop {
        #[arg(long, value_parser = ["host", "agent"])]
        role: String,
    },
    /// 멈춘 감독자를 다시 올린다(launchctl bootstrap). 사람이 터미널에서 직접 한다.
    Start {
        #[arg(long, value_parser = ["host", "agent"])]
        role: String,
    },
}

#[derive(Subcommand)]
pub(super) enum ConfigCmd {
    /// host.toml·agent.toml(+ .local.toml)을 검증한다
    Check,
    /// 이 머신에 Gemini 실행 경로와 구독을 등록한다(aaa 세션 밖 터미널에서 로그인 주소를 숨겨 입력)
    Agy {
        /// 설치한 공식 agy 실행 파일의 절대 경로
        #[arg(long, value_name = "PATH")]
        command: String,
        /// 같은 Google 구독을 쓰는 머신끼리 공유하는 별칭
        #[arg(long, value_name = "NAME")]
        subscription: String,
    },
    /// 예시 설정을 출력한다
    Example {
        #[arg(value_parser = ["host", "agent"])]
        role: String,
    },
    /// schema 갱신 계획(지금은 schema 1뿐)
    Migrate,
}

pub(super) fn launchd_spec(
    role: String,
    aaa: Option<PathBuf>,
    launcher: Option<PathBuf>,
) -> Result<crate::launchd::Spec> {
    let aaa = match aaa {
        Some(p) => p,
        None => std::env::current_exe()?.canonicalize()?,
    };
    Ok(crate::launchd::Spec {
        role,
        launcher: launcher.unwrap_or_else(crate::launchd::default_launcher),
        aaa,
        env: crate::launchd::captured_env(),
    })
}

pub(super) fn service_cmd(cmd: ServiceCmd, json: bool) -> Result<ExitCode> {
    match cmd {
        ServiceCmd::Plan { label } => print!("{}", service::plan(&label)?),
        ServiceCmd::Register { label } => println!("{label}: {}", service::register(&label)?),
        ServiceCmd::Unregister { label } => println!("{label}: {}", service::unregister(&label)?),
        ServiceCmd::Status { label } => println!("{label}: {}", service::status(&label)?),
        ServiceCmd::OpenSettings => service::open_settings(),
        ServiceCmd::Show { role, aaa, launcher } => {
            print!("{}", crate::launchd::show(&launchd_spec(role, aaa, launcher)?)?)
        }
        ServiceCmd::Install { role, aaa, launcher, replace } => {
            print!("{}", crate::launchd::install(&launchd_spec(role, aaa, launcher)?, replace)?)
        }
        ServiceCmd::Uninstall { role } => print!("{}", crate::launchd::uninstall(&role)?),
        ServiceCmd::Restart { role, machine } => {
            let service = if role == "host" { aaa_proto::DeployService::Host } else { aaa_proto::DeployService::Agent };
            if let Some(code) = crate::deployment::route(json, machine, aaa_proto::DeployAction::Restart { service })? {
                return Ok(code);
            }
            print!("{}", crate::launchd::restart(&role)?);
        }
        ServiceCmd::Stop { role } => print!("{}", crate::launchd::stop(&role)?),
        ServiceCmd::Start { role } => print!("{}", crate::launchd::start(&role)?),
    }
    Ok(ExitCode::SUCCESS)
}

/// 이름 변경(결정 18) 전 기본 폴더만 있으면 새 폴더로 시작하지 않는다. 새 장치 키·빈 DB로 뜨면 등록이 끊긴다.
pub(super) fn legacy_guard() -> Result<()> {
    let moves = aaa_config::paths::legacy_moves();
    if moves.is_empty() {
        return Ok(());
    }
    let lines: Vec<String> =
        moves.iter().map(|(old, new)| format!("  mv \"{}\" \"{}\"", old.display(), new.display())).collect();
    anyhow::bail!(
        "이름이 aa에서 aaa로 바뀌었다(결정 18). 옛 폴더가 있고 새 폴더가 없다.\n\
         옛 감독자를 내린 뒤(`~/.local/bin/aa service uninstall --role agent`, `--role host`) 옮긴다:\n{}",
        lines.join("\n")
    )
}

pub(super) fn config_cmd(cmd: ConfigCmd, json: bool) -> Result<ExitCode> {
    let dir = aaa_config::paths::config_dir();
    match cmd {
        ConfigCmd::Agy { command, subscription } => crate::agy_config::configure(&dir, command, subscription, json),
        ConfigCmd::Example { role } => {
            print!("{}", aaa_config::example(&role));
            Ok(ExitCode::SUCCESS)
        }
        ConfigCmd::Migrate => {
            for stem in ["host", "agent"] {
                match aaa_config::migrate_plan(&dir, stem) {
                    Ok(m) => println!("{m}"),
                    Err(e) => println!("{}", e.render()),
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ConfigCmd::Check => {
            legacy_guard()?;
            let mut ok = true;
            let mut report = Vec::new();
            let host = aaa_config::load_host(&dir).map(|l| (l.files, l.warnings));
            let agent = aaa_config::load_agent(&dir).map(|l| (l.files, l.warnings));
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
