//! 1) macOS: doctor probe가 매번 새 실행 파일을 만들 때 쓰는 작은 Mach-O를 지금 소스로 빌드한다.
//! 2) web/dist가 있으면 host 웹 자산으로 포함한다(없으면 「웹 미빌드」 안내만 보인다).

use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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

    let dist = manifest.join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = Vec::new();
    if dist.is_dir() {
        collect(&dist, &dist, &mut files);
    }
    files.sort();
    let mut src = String::from("pub static WEB_ASSETS: &[(&str, &[u8])] = &[\n");
    for (rel, abs) in &files {
        writeln!(src, "    ({rel:?}, include_bytes!({abs:?})),").unwrap();
    }
    src.push_str("];\n");
    std::fs::write(out.join("web_assets.rs"), src).unwrap();
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push((rel, path.canonicalize().unwrap().to_string_lossy().into_owned()));
        }
    }
}
