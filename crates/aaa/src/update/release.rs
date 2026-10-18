//! herdr `update.rs`의 manifest → OS/arch 선택 → curl → SHA-256 → staging.
//! 외부 주소를 받는 CLI·환경 변수는 없다. 운영 manifest/asset은 공개 GitHub release만 허용한다.

use std::collections::BTreeMap;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::{Place, Staged, parse_version, sha256};

const RELEASES: &str = "https://github.com/cLazyZombie/aaa/releases";

pub(super) fn requested_version(value: Option<&str>) -> Result<Option<String>> {
    value
        .map(|s| {
            let v = s.strip_prefix('v').unwrap_or(s);
            let (m, n, p) = parse_version(v).with_context(|| format!("release 판은 x.y.z 또는 vx.y.z로 준다: {s}"))?;
            let canonical = format!("{m}.{n}.{p}");
            if v != canonical {
                bail!("release 판은 앞자리 0 없는 x.y.z로 준다: {s}");
            }
            Ok(canonical)
        })
        .transpose()
}

pub(super) fn manifest_url(version: Option<&str>) -> String {
    match version {
        Some(v) => format!("{RELEASES}/download/v{v}/manifest.json"),
        None => format!("{RELEASES}/latest/download/manifest.json"),
    }
}

#[derive(Deserialize)]
pub(super) struct Manifest {
    pub(super) version: String,
    assets: BTreeMap<String, Asset>,
}

#[derive(Clone, Deserialize)]
struct Asset {
    url: String,
    sha256: String,
}

pub(super) struct Selected {
    pub(super) version: String,
    asset: Asset,
}

impl Manifest {
    pub(super) fn select(
        &self,
        os: &str,
        arch: &str,
        current: &str,
        requested: Option<&str>,
    ) -> Result<Option<Selected>> {
        let version = requested_version(Some(&self.version))?.unwrap();
        if let Some(want) = requested
            && version != want
        {
            bail!("요청한 release 판은 {want}인데 manifest는 {version}이다");
        }
        let latest = parse_version(&version).unwrap();
        let current = parse_version(current).context("지금 실행 파일 판을 해석하지 못했다")?;
        // 기본 최신판은 내리지 않는다. 명시한 옛 판은 사람이 고른 것이므로 고정 검사 뒤 받는다.
        if latest == current || (requested.is_none() && latest < current) {
            return Ok(None);
        }
        let key = format!("{os}-{arch}");
        let asset = self.assets.get(&key).with_context(|| format!("manifest에 {key} 실행 파일이 없다"))?;
        let prefix = format!("{RELEASES}/download/v{version}/");
        let file = asset.url.strip_prefix(&prefix).unwrap_or("");
        if file.is_empty() || !file.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)) {
            bail!("{key}: asset 주소는 해당 판의 공개 GitHub release 파일이어야 한다");
        }
        if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
            bail!("{key}: manifest SHA-256은 64자리 hex여야 한다");
        }
        Ok(Some(Selected { version, asset: asset.clone() }))
    }
}

/// herdr와 같이 curl을 쓴다. 개인 curl 설정·token·재시도는 쓰지 않고 HTTPS redirect만 받는다.
/// URL은 호출자가 정한다: 운영 진입점은 RELEASES와 검증한 asset만, 시험은 임시 HTTP 서버만 쓴다.
fn curl(url: &str, limit: &str) -> Command {
    let mut cmd = Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    cmd.args([
        "-q",
        "--fail",
        "--silent",
        "--show-error",
        "--location",
        "--proto",
        "=http,https",
        "--proto-redir",
        "=https",
        "--connect-timeout",
        "10",
        "--max-time",
        limit,
        "--user-agent",
        concat!("aaa/", env!("CARGO_PKG_VERSION")),
    ])
    .arg(url);
    cmd
}

pub(super) fn fetch_manifest(url: &str) -> Result<Manifest> {
    let output = curl(url, "20").output().context("manifest용 curl을 실행하지 못했다")?;
    if !output.status.success() {
        bail!("manifest 내려받기 실패: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    serde_json::from_slice(&output.stdout).context("release manifest JSON을 읽지 못했다")
}

impl Selected {
    pub(super) fn download(&self, place: &Place) -> Result<Staged> {
        download(&self.asset.url, &self.asset.sha256, place)
    }
}

fn download(url: &str, expected: &str, place: &Place) -> Result<Staged> {
    let staged = Staged::new(place)?;
    let output =
        curl(url, "120").arg("--output").arg(staged.path()).output().context("실행 파일용 curl을 실행하지 못했다")?;
    if !output.status.success() {
        bail!("실행 파일 내려받기 실패: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let actual = sha256(staged.path())?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("내려받은 실행 파일 SHA-256 불일치: 기대 {expected}, 실제 {actual}");
    }
    staged.make_executable()?;
    Ok(staged)
}

#[cfg(test)]
mod tests;
