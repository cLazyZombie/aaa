//! 실제 판·경로 목록은 작은 독립 owner가 검사한다. 큰 CLI unit에 SHA 의존성을 넣지 않는다.
include!(concat!(env!("OUT_DIR"), "/panic_build.rs"));

#[test]
fn panic_sha_matches_the_current_checkout() {
    let output = std::process::Command::new("git")
        .args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(BUILD_SHA, String::from_utf8(output.stdout).unwrap().trim());
    assert!(SOURCE_FILES.contains(&"crates/aaa/src/main.rs"));
}
