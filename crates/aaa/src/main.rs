//! aaa 단일 실행 파일. 역할(`aaa host`·`aaa agent`·`aaa session-host`·CLI)은 각 crate에 있고 여기서 고른다.

mod cli;
mod deployment;
mod image;
mod init;
mod install;
use aaa_agent::integration;
mod launchd;
mod migrate;
mod person;
mod rollout;
mod service;
mod settings;
mod update;
mod work_root;

fn main() -> std::process::ExitCode {
    aaa_common::panic_log::install();
    cli::main()
}
