//! aaa 단일 실행 파일. 역할(`aaa host`·`aaa agent`·`aaa session-host`·CLI)은 각 crate에 있고 여기서 고른다.

#[cfg(all(target_os = "macos", not(target_arch = "aarch64")))]
compile_error!("aaa는 macOS에서 Apple Silicon(arm64)만 지원합니다");

mod agy_config;
mod cli;
mod db_backup;
mod deployment;
mod file_backup;
mod image;
mod init;
mod install;
use aaa_agent::integration;
mod launchd;
mod migrate;
mod person;
mod plan_preview;
mod restart;
mod rollout;
mod service;
#[cfg(unix)]
mod service_logs;
mod settings;
mod setup;
mod update;
mod work_root;

fn main() -> std::process::ExitCode {
    #[cfg(not(test))]
    aaa_common::panic_log::install(build_metadata::BUILD_SHA, build_metadata::SOURCE_FILES);
    cli::main()
}

#[cfg(not(test))]
mod build_metadata {
    include!(concat!(env!("OUT_DIR"), "/panic_build.rs"));
}
