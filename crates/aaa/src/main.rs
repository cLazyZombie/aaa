//! aaa 단일 실행 파일. 역할(`aaa host`·`aaa agent`·`aaa session-host`·`aaa attach`·CLI)은 각 crate에 있고 여기서 고른다.

mod cli;
mod launchd;
mod migrate;
mod person;
mod service;
mod settings;
mod update;

fn main() -> std::process::ExitCode {
    cli::main()
}
