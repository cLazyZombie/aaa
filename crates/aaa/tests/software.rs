//! CLI와 setup 진입점도 공통 판 문자열을 그대로 쓴다. 긴 AAA_BUILD_SALT 빌드에서 함께 실행한다.
use std::process::Command;

#[test]
fn cli_and_setup_report_the_same_short_build_version() {
    let salt = aaa_common::util::BUILD_SALT;
    let suffix = if salt.is_empty() {
        String::new()
    } else if (12..=40).contains(&salt.len()) && salt.bytes().all(|b| b.is_ascii_hexdigit()) {
        format!("+{}", &salt[..12])
    } else {
        format!("+{salt}")
    };
    let expected = format!("aaa {}{suffix}\n", env!("CARGO_PKG_VERSION"));
    let root = std::env::temp_dir().join(format!("aaa-version-{}", aaa_common::util::new_id()));
    std::fs::create_dir(&root).unwrap();
    let setup = root.join(if cfg!(windows) { "aaa-setup.exe" } else { "aaa-setup" });
    std::fs::copy(env!("CARGO_BIN_EXE_aaa"), &setup).unwrap();
    for exe in [std::path::Path::new(env!("CARGO_BIN_EXE_aaa")), setup.as_path()] {
        let output = Command::new(exe).arg("--version").output().unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
    std::fs::remove_dir_all(root).unwrap();
}
