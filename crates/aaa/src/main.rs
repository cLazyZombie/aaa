//! aaa 단일 실행 파일. 역할(`aaa host`·`aaa agent`·`aaa session-host`·CLI)은 각 crate에 있고 여기서 고른다.

mod cli;
mod db_backup;
mod deployment;
mod image;
mod init;
mod install;
use aaa_agent::integration;
mod launchd;
mod migrate;
mod person;
mod restart;
mod rollout;
mod service;
mod settings;
mod setup;
mod update;
mod work_root;

fn main() -> std::process::ExitCode {
    aaa_common::panic_log::install(build_metadata::BUILD_SHA, build_metadata::SOURCE_FILES);
    cli::main()
}

mod build_metadata {
    include!(concat!(env!("OUT_DIR"), "/panic_build.rs"));

    #[test]
    #[cfg(test)]
    fn panic_sha_matches_the_current_checkout() {
        let output = std::process::Command::new("git")
            .args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(BUILD_SHA, String::from_utf8(output.stdout).unwrap().trim());
        assert!(SOURCE_FILES.contains(&"crates/aaa/src/main.rs"));
    }
}
