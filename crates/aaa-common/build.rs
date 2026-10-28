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
    for name in ["HEAD", "index"] {
        watch_git(&root, name);
    }
    // 새 checkout에는 packed-refs가 없다. 없는 파일을 감시하면 Cargo가 모든 호출에서
    // build script와 aaa-common의 모든 의존 crate를 다시 빌드한다. loose ref의 생성·삭제는
    // 아래 ref 감시가, detached HEAD의 변경은 HEAD 감시가 맡는다.
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
    // 생성물에 홈 절대 경로를 넣지 않는다. git 없는 소스 배포는 external/unknown으로 숨긴다.
    let source = format!("pub const BUILD_SHA: &str = {sha:?};\nconst SOURCE_FILES: &[&str] = &{paths:?};\n");
    std::fs::write(PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("panic_build.rs"), source).unwrap();
}

fn watch_git(root: &Path, name: &str) {
    if let Some(mut path) = git_path(root, name) {
        // packed ref뿐인 branch와 아직 index가 없는 저장소도 없는 경로를 감시하지 않는다.
        // 가장 가까운 기존 폴더에서 loose ref/index가 생기는 사건을 잡고 다음 빌드부터
        // 그 파일만 감시한다. worktree의 공용 refs도 git이 준 경로로 찾는다.
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
