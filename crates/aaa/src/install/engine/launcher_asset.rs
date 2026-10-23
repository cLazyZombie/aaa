//! 같은 release의 고정 4파일 package. 압축 해제기·임의 경로·기존 launcher 대체는 없다.
use super::*;
use base64::Engine as _;

pub(super) const FILES: [&str; 4] = [
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
    files: std::collections::BTreeMap<String, String>,
}
pub(super) fn source(plan: &InstallPlan) -> Result<PathBuf> {
    let binary = plan.choices.from.as_ref().map(PathBuf::from).unwrap_or(std::env::current_exe().map_err(Fault::io)?);
    let arch = match plan.context.platform.arch {
        InstallArch::Aarch64 => "aarch64",
        InstallArch::X86_64 => "x86_64",
    };
    Ok(binary.parent().ok_or_else(Fault::conflict)?.join(format!("aaa-launcher-macos-{arch}.json")))
}
pub(super) struct AssetFile {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub mode: Option<u32>,
}

fn preserved(launcher: &Path) -> Result<Vec<AssetFile>> {
    let path = transaction::target(launcher)?;
    let bytes = std::fs::read(&path).map_err(|source_error| {
        aaa_common::diagnostic::context(
            source_error,
            Fault { code: "installation_launcher_required", exit: 3, cause: None },
        )
    })?;
    // before == after라도 승인 검토 뒤 바뀐 launcher를 새 context로 채택하면 안 된다.
    Ok(vec![AssetFile { path, bytes, mode: None }])
}

pub(super) fn proposals(plan: &InstallPlan) -> Result<Vec<AssetFile>> {
    if plan.choices.service != Some(InstallService::Launchd) {
        return Ok(Vec::new());
    }
    let launcher = plan.choices.launcher.as_deref().ok_or(Fault {
        code: "installation_launcher_required",
        exit: 3,
        cause: None,
    })?;
    let launcher = Path::new(launcher);
    if plan.choices.launcher_policy != Some(InstallLauncherPolicy::Install) {
        return preserved(launcher);
    }
    let existing = std::fs::read(launcher)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok();
    let asset = source(plan)?;
    match asset.symlink_metadata() {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() && m.len() <= 4 * 1024 * 1024 => {}
        _ if existing.is_some() => return preserved(launcher),
        _ => return Err(Fault { code: "installation_launcher_asset_required", exit: 3, cause: None }),
    };
    let package: Package = serde_json::from_slice(&std::fs::read(asset).map_err(Fault::io)?)
        .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))?;
    let arch = match plan.context.platform.arch {
        InstallArch::Aarch64 => "aarch64",
        InstallArch::X86_64 => "x86_64",
    };
    // I1은 --from을 실행해 판을 알아내지 않는다. 같은 실행 파일의 사본인 경우에만
    // 현재 installer의 알려진 판을 package 대조에 쓴다. 임의 원본의 판은 추정하지 않는다.
    let version = if plan.context.version == "unverified" {
        let current = std::env::current_exe().map_err(Fault::io)?;
        if hash(&std::fs::read(current).map_err(Fault::io)?) != plan.context.binary_sha256 {
            return Err(Fault { code: "installation_launcher_binary_unverified", exit: 3, cause: None });
        }
        aaa_common::util::software().trim_start_matches("aaa ").to_owned()
    } else {
        plan.context.version.clone()
    };
    if package.schema != 1
        || package.version != version
        || package.platform != format!("macos-{arch}")
        || package.binary_sha256 != plan.context.binary_sha256
        || package.files.len() != FILES.len()
        || FILES.iter().any(|name| !package.files.contains_key(*name))
    {
        return Err(Fault::conflict());
    }
    let payload: Vec<Vec<u8>> = FILES
        .iter()
        .map(|name| {
            base64::engine::general_purpose::STANDARD
                .decode(&package.files[*name])
                .map_err(|source_error| aaa_common::diagnostic::context(source_error, Fault::conflict()))
        })
        .collect::<Result<_>>()?;
    if payload.iter().any(Vec::is_empty) {
        return Err(Fault::conflict());
    }
    if existing.as_ref().is_some_and(|bytes| bytes != &payload[0]) {
        return preserved(launcher);
    }
    let root = launcher.parent().and_then(Path::parent).and_then(Path::parent).ok_or_else(Fault::conflict)?;
    if root.join(FILES[0]) != launcher || root.extension().is_none_or(|ext| ext != "app") {
        return Err(Fault::conflict());
    }
    FILES
        .into_iter()
        .zip(payload)
        .map(|(name, bytes)| {
            let path = root.join(name);
            let before = std::fs::read(&path)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok();
            if before.as_ref().is_some_and(|old| *old != bytes) {
                return Err(Fault::conflict());
            }
            Ok(AssetFile { path, bytes, mode: Some(if name == FILES[0] { 0o755 } else { 0o644 }) })
        })
        .collect()
}
