//! web/dist가 있으면 host 웹 자산으로 포함한다(없으면 「웹 미빌드」 안내만 보인다).

use std::env;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    let dist = manifest.join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let mut files = Vec::new();
    if dist.is_dir() {
        collect(&dist, &dist, &mut files);
    }
    files.sort();
    // 절대 경로를 적지 않는다: 컴파일 캐시(kache)가 build script 결과를 다른 worktree에서 가져오면 그 경로가 남는다.
    // 경로는 컴파일할 때 이 crate 폴더 기준으로 푼다.
    let mut src = String::from("pub static WEB_ASSETS: &[(&str, &[u8])] = &[\n");
    for rel in &files {
        let at = format!("/../../web/dist/{rel}");
        writeln!(src, "    ({rel:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {at:?}))),").unwrap();
    }
    src.push_str("];\n");
    std::fs::write(out.join("web_assets.rs"), src).unwrap();
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, out);
        } else {
            out.push(path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}
