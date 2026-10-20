//! panic 로그에는 저장소가 아는 상대 경로와 검증한 판 SHA만 넣는다.
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..");
    // worktree의 .git은 파일이다. 실제 HEAD·index·현재 ref·packed-refs를 감시한다.
    for name in ["HEAD", "index", "packed-refs"] {
        watch_git(&root, name);
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
    // 생성물에 홈 절대 경로를 넣지 않는다. git 없는 소스 배포는 external/unknown으로 숨긴다.
    let source = format!("pub const BUILD_SHA: &str = {sha:?};\nconst SOURCE_FILES: &[&str] = &{paths:?};\n");
    std::fs::write(PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("panic_build.rs"), source).unwrap();
}

fn watch_git(root: &Path, name: &str) {
    if let Some(path) = git(root, &["rev-parse", "--git-path", name]) {
        let path = PathBuf::from(path);
        let path = if path.is_absolute() { path } else { root.join(path) };
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
