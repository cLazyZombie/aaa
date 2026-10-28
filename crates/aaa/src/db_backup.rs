//! host 머신에서 실행하는 DB 백업·복원 도구. 운영 DB 교체·host 재시작은 하지 않는다.
use anyhow::Result;
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum Command {
    /// 마지막 정기 백업 시도·성공 시각과 오류를 읽는다.
    Status,
    /// 설정한 목적지에 지금 한 번 암호화 백업한다(정기 차례와 별개).
    Run,
    /// 복원 개인키를 새 private 파일에 저장하고 공개키만 출력한다.
    Keygen {
        #[arg(long)]
        identity: PathBuf,
    },
    /// 새 파일에 복원하고 무결성을 검사한다. 기존 파일은 덮어쓰지 않는다.
    Restore {
        backup: PathBuf,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
}

pub fn run(command: Command, json: bool) -> Result<()> {
    let value = match command {
        Command::Status => {
            let status = aaa_host::backup::status(&aaa_config::paths::state_dir().join("host"))?;
            if !json {
                println!("{}", serde_json::to_string_pretty(&status)?);
            }
            serde_json::to_value(status)?
        }
        Command::Keygen { identity } => {
            let recipient = aaa_host::backup::keygen(&identity)?;
            if !json {
                println!("{recipient}");
            }
            serde_json::json!({"recipient":recipient})
        }
        Command::Run => {
            let config = aaa_config::load_host(&aaa_config::paths::config_dir())?.config;
            let path = aaa_host::backup::run(
                &aaa_config::paths::state_dir().join("host"),
                &config.backup,
                aaa_common::util::now_ms(),
            )?;
            if !json {
                println!("{}", path.display());
            }
            serde_json::json!({"backup":path})
        }
        Command::Restore { backup, identity, output } => {
            aaa_host::backup::restore(&backup, &identity, &output)?;
            if !json {
                println!("복원·무결성 검사 완료: {}", output.display());
            }
            serde_json::json!({"restored":output})
        }
    };
    if json {
        println!("{}", serde_json::to_string(&value)?);
    }
    Ok(())
}
