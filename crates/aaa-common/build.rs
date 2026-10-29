//! panic 로그의 원본 경로는 저장소가 아는 상대 경로로 제한한다. SHA는 실행 파일이 주입한다.
use std::path::{Path, PathBuf};
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.join("../..");
    // tracked Rust 파일 목록만 감시한다. SHA를 공통 crate에 넣으면 매 커밋마다 모든 backend가 재컴파일된다.
    watch_git(&root, "index");
    let files = git(&root, &["ls-files", "-z", "--", "*.rs"]).unwrap_or_default();
    let mut paths: Vec<_> = files.split('\0').filter(|s| !s.is_empty()).collect();
    paths.sort_unstable();
    // 생성물에 홈 절대 경로를 넣지 않는다. git 없는 소스 배포는 external/unknown으로 숨긴다.
    let source = format!("const SOURCE_FILES: &[&str] = &{paths:?};\n");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("panic_build.rs");
    if std::fs::read_to_string(&output).ok().as_deref() != Some(&source) {
        std::fs::write(output, source).unwrap();
    }
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
