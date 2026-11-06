//! 운영 host를 띄우지 않는 파일 백업·새 폴더 복원 명령.
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Command {
    /// 마지막 정기 파일 백업 결과를 읽는다.
    Status,
    /// 별도 켜기 설정과 관계없이 지금 한 번 백업한다.
    Run,
    /// 새 폴더에 복원한다. 기존 경로는 덮어쓰지 않는다.
    Restore {
        backup: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}

pub fn run(command: Command, json: bool) -> Result<()> {
    let state = aaa_config::paths::state_dir().join("host");
    let value = match command {
        Command::Status => serde_json::to_value(aaa_host::file_backup::status(&state)?)?,
        Command::Run => {
            let cfg = aaa_config::load_host(&aaa_config::paths::config_dir())?.config;
            let path = aaa_host::file_backup::run(&state, &cfg, aaa_common::util::now_ms())?;
            serde_json::json!({"backup": path})
        }
        Command::Restore { backup, identity, output } => {
            aaa_host::file_backup::restore(&backup, &identity, &output)?;
            serde_json::json!({"restored": output})
        }
    };
    if json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        println!("{}", serde_json::to_string_pretty(&value)?);
    }
    Ok(())
}
