//! 기존 launcher는 보존한다. 새 설치는 같은 배포 binary에 묶인 고정 4파일 package만 받는다.
use super::super::flow::{InstallFile, Layout};
use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const FILES: [&str; 4] = [
    "Contents/MacOS/aaa-launcher",
    "Contents/Info.plist",
    "Contents/Resources/AppIcon.icns",
    "Contents/_CodeSignature/CodeResources",
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    schema: u32,
    version: String,
    platform: String,
    binary_sha256: String,
    files: BTreeMap<String, String>,
}

pub(super) fn prepare(layout: &Layout, launcher: &Path) -> Result<Vec<InstallFile>> {
    use std::os::unix::fs::PermissionsExt;
    ensure!(launcher.is_absolute(), "launcher는 절대 경로여야 합니다");
    if let Ok(meta) = launcher.symlink_metadata() {
        ensure!(
            meta.is_file() && !meta.file_type().is_symlink() && meta.permissions().mode() & 0o111 != 0,
            "launcher가 일반 실행 파일이 아닙니다: {}",
            launcher.display()
        );
        return Ok(Vec::new());
    }
    let root = launcher.parent().and_then(Path::parent).and_then(Path::parent).context("launcher app 경로")?;
    ensure!(
        root.extension().is_some_and(|e| e == "app") && root.join(FILES[0]) == launcher,
        "새 launcher는 .app/Contents/MacOS/aaa-launcher 경로에 둡니다"
    );
    ensure!(!root.exists(), "기존 launcher app은 바꾸지 않습니다: {}", root.display());
    let package = layout
        .source
        .parent()
        .context("설치기 폴더")?
        .join(format!("aaa-launcher-macos-{}.json", std::env::consts::ARCH));
    let meta = package.symlink_metadata().with_context(|| format!(
        "launcher 준비 파일 {}가 없습니다. 같은 release의 launcher JSON을 설치기 옆에 두거나 --launcher로 기존 실행 파일을 지정하세요", package.display()))?;
    ensure!(
        meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 4 * 1024 * 1024,
        "launcher package 파일 형식·크기 오류"
    );
    let package: Package = serde_json::from_slice(&std::fs::read(&package)?).context("launcher package 읽기")?;
    ensure!(
        package.schema == 1
            && package.version == aaa_common::util::software().trim_start_matches("aaa ")
            && package.platform == format!("macos-{}", std::env::consts::ARCH)
            && package.binary_sha256 == crate::update::sha256(&layout.source)?
            && package.files.len() == FILES.len()
            && FILES.iter().all(|name| package.files.contains_key(*name)),
        "launcher package의 판·CPU·실행 파일 SHA·파일 목록이 설치기와 다릅니다"
    );
    FILES
        .into_iter()
        .map(|name| {
            let bytes = base64::engine::general_purpose::STANDARD.decode(&package.files[name])?;
            ensure!(!bytes.is_empty(), "launcher package에 빈 파일이 있습니다: {name}");
            Ok(InstallFile {
                path: PathBuf::from(root).join(name),
                bytes,
                mode: if name == FILES[0] { 0o755 } else { 0o644 },
            })
        })
        .collect()
}
