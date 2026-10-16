//! macOS: doctor probe가 매번 새 실행 파일을 만들 때 쓰는 작은 Mach-O를 지금 소스로 빌드한다.

use std::env;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let probe = out.join("probe-target");
    println!("cargo:rerun-if-changed=probe/target.c");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let compiler = cc::Build::new().get_compiler();
        let status = compiler
            .to_command()
            .arg(manifest.join("probe/target.c"))
            .arg("-O0")
            .arg("-o")
            .arg(&probe)
            .status()
            .expect("C 컴파일러 실행");
        assert!(status.success(), "probe target 빌드 실패");
    } else {
        std::fs::write(&probe, b"").unwrap();
    }
}
