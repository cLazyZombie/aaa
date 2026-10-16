//! aa 단일 실행 파일. 역할(`aa host`·`aa agent`·`aa session-host`·`aa attach`·CLI)은 각 crate에 있고 여기서 고른다.

mod cli;
mod service;

fn main() -> std::process::ExitCode {
    cli::main()
}
