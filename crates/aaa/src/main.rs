//! aaa 단일 실행 파일. 역할(`aaa host`·`aaa agent`·`aaa session-host`·CLI)은 각 crate에 있고 여기서 고른다.

mod cli;
mod deployment;
mod init;
mod install;
mod integration;
mod launchd;
mod migrate;
mod person;
mod rollout;
mod service;
mod settings;
mod update;
mod work_root;

fn main() -> std::process::ExitCode {
    std::panic::set_hook(Box::new(|_| eprintln!("aaa 내부 오류: 비밀을 보호하기 위해 panic 본문을 출력하지 않는다")));
    cli::main()
}
