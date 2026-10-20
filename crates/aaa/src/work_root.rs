//! 로컬 머신 설정만 바꾼다. 준비 기록·이전 root 보존이 설정 rename보다 먼저다.
use std::process::ExitCode;

use aaa_agent::work_root::admin;
use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

#[derive(Subcommand)]
pub enum Command {
    /// 다음 시작 설정과 마지막 agent 시작 기록을 본다(연결 증명은 아니다)
    Show,
    /// 실제 mount와 쓰기·읽기·삭제를 점검한다. 없는 root는 만든다
    Check {
        /// 비우면 현재 설정의 경로
        #[arg(long)]
        path: Option<String>,
        /// 반드시 연결되어 있어야 하는 실제 mount 지점
        #[arg(long)]
        mount: Option<String>,
    },
    /// 이전 작업은 그대로 두고 다음 agent 시작의 root를 저장한다
    Set {
        path: String,
        /// 비우면 macOS /Volumes는 자동 검사하며, 같은 root의 기존 mount는 유지한다
        #[arg(long)]
        mount: Option<String>,
    },
}

fn shown_json(s: &admin::Shown) -> Value {
    let applied = s.last_start.as_ref().map(|(path, mount)| json!({"path":path,"mount":mount}));
    json!({"configured":{"path":s.path,"mount":s.mount},"last_start":applied,
        "previous_roots":s.previous,"restart_required":s.restart_required,"config_file":s.config_file})
}

pub fn run(json_output: bool, cmd: Command) -> Result<ExitCode> {
    let dir = aaa_config::paths::config_dir();
    let state = aaa_config::paths::state_dir().join("agent");
    let result = (|| -> Result<Value> {
        let value =
            match cmd {
                Command::Show => shown_json(&admin::show(&dir, &state)?),
                Command::Check { path, mount } => {
                    let c = admin::check(&dir, &state, path.as_deref(), mount.as_deref())?;
                    json!({"ok":true,"path":c.path,"real_path":c.real_path,"mount":c.mount})
                }
                Command::Set { path, mount } => {
                    let done = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()?
                        .block_on(admin::set(&dir, &state, &path, mount.as_deref()))?;
                    let mut value = shown_json(&done.shown);
                    value["changed"] = done.changed.into();
                    value["imported_preparations"] = done.imported.into();
                    value
                }
            };
        Ok(value)
    })();
    let value = match result {
        Ok(value) => value,
        Err(e) if json_output => {
            eprintln!("{}", json!({"error":format!("{e:#}")}));
            return Ok(ExitCode::FAILURE);
        }
        Err(e) => return Err(e),
    };
    if json_output {
        println!("{}", serde_json::to_string(&value)?);
    } else if value.get("configured").is_some() {
        println!("다음 시작: {}", value["configured"]["path"].as_str().unwrap_or(""));
        println!("지정 mount: {}", value["configured"]["mount"].as_str().unwrap_or("자동 판정"));
        println!("마지막 시작 기록: {}", value["last_start"]["path"].as_str().unwrap_or("없음"));
        for root in value["previous_roots"].as_array().into_iter().flatten() {
            println!("이전 root: {}", root["path"].as_str().unwrap_or(""));
        }
        if let Some(count) = value["imported_preparations"].as_u64() {
            println!("가져온 준비 기록: {count}");
        }
        if value["restart_required"] == true {
            println!("agent 재시작 필요. 서비스는 재시작하지 않았다. 마지막 시작 기록은 현재 연결 증명이 아니다.");
        } else {
            println!("설정과 마지막 시작 기록이 같다. 현재 연결 상태도 host에서 확인한다.");
        }
    } else {
        println!("작업 root 점검 통과: {}", value["path"].as_str().unwrap_or(""));
    }
    Ok(ExitCode::SUCCESS)
}
