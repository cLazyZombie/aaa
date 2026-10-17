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
    // 생성 파일에 이 빌드의 절대 경로를 넣지 않는다: 경로는 컴파일할 때 그 crate의 CARGO_MANIFEST_DIR로 펼친다
    // (herdr `src/sound.rs`의 상대 include_bytes!, `protocol/endpoint.rs`의 env!("CARGO_MANIFEST_DIR")와 같다).
    // build script 실행 결과를 캐시하는 도구(kache 1.2.2 `build_script_run`)가 같은 dist의 다른 worktree에 이 파일을
    // 되돌려 놓아도 그 worktree 자신의 web/dist를 읽는다. 전에는 canonicalize()한 절대 경로라 처음 빌드한 worktree를
    // 읽었고, 그 worktree가 지워지면(resolve) 남은 worktree의 빌드가 「couldn't read」로 깨졌다(t-0327 재현).
    let mut src = String::from("pub static WEB_ASSETS: &[(&str, &[u8])] = &[\n");
    for rel in &files {
        writeln!(
            src,
            "    ({rel:?}, include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {:?}))),",
            format!("/../../web/dist/{rel}")
        )
        .unwrap();
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
