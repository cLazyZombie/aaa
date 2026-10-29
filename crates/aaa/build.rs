//! 커밋 식별자는 최종 실행 파일에만 넣어 공통 backend의 컴파일 캐시를 보존한다.
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    let root = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    watch_git(&root, "HEAD");
    watch_git(&root, "index");
    if let Some(path) = git_path(&root, "packed-refs").filter(|path| path.exists()) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    if let Some(name) = git(&root, &["symbolic-ref", "-q", "HEAD"]) {
        watch_git(&root, &name);
    }
    let sha = git(&root, &["rev-parse", "HEAD"])
        .filter(|s| matches!(s.len(), 40 | 64) && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "unknown".into());
    let files = git(&root, &["ls-files", "-z", "--", "*.rs"]).unwrap_or_default();
    let mut paths: Vec<_> = files.split('\0').filter(|s| !s.is_empty()).collect();
    paths.sort_unstable();
    let source = format!("pub const BUILD_SHA: &str = {sha:?};\npub const SOURCE_FILES: &[&str] = &{paths:?};\n");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("panic_build.rs");
    if std::fs::read_to_string(&output).ok().as_deref() != Some(&source) {
        std::fs::write(output, source).unwrap();
    }
}

fn watch_git(root: &Path, name: &str) {
    if let Some(mut path) = git_path(root, name) {
        while !path.exists() {
            if !path.pop() {
                return;
            }
        }
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn git_path(root: &Path, name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(git(root, &["rev-parse", "--git-path", name])?);
    Some(if path.is_absolute() { path } else { root.join(path) })
}
