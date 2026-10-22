//! web/dist가 있으면 host 웹 자산으로 포함한다(없으면 「웹 미빌드」 안내만 보인다).

use std::env;
use std::fmt::Write as _;
use std::io::Write as _;
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
    let mut src = String::from("pub static WEB_ASSETS: &[Asset] = &[\n");
    for (index, rel) in files.iter().enumerate() {
        let at = format!("/../../web/dist/{rel}");
        let bytes = std::fs::read(dist.join(rel)).unwrap();
        // gzip header의 시각·이름을 넣지 않아 같은 자산은 같은 bytes가 된다.
        let mut encoder = flate2::GzBuilder::new().mtime(0).write(Vec::new(), flate2::Compression::best());
        encoder.write_all(&bytes).unwrap();
        let compressed = encoder.finish().unwrap();
        let gzip = compressed.len() + 32 < bytes.len();
        let data = if gzip {
            let file = format!("web-asset-{index}.gz");
            std::fs::write(out.join(&file), compressed).unwrap();
            format!("include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{file}\"))")
        } else {
            format!("include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {at:?}))")
        };
        writeln!(src, "    Asset {{ name: {rel:?}, data: {data}, gzip: {gzip} }},").unwrap();
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
