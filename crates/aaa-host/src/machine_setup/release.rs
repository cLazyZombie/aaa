//! 다른 종류(OS·CPU) 머신에 넣을 같은 판 release 실행 파일(웹 머신 추가 M3, 계획 t-0471 §1.5). `aaa update`(crates/aaa/src/update/release.rs)와
//! 같은 규칙이다: 정확한 판의 manifest만 받고(latest 아님), asset 주소는 그 판의 공개 GitHub release 파일이어야 하며, 받은 파일의
//! SHA-256이 manifest와 같아야 쓴다. curl은 개인 설정·token 없이 HTTPS redirect만 따른다. 받은 파일은 host 상태 폴더
//! `ssh/release/v<판>/<os>-<arch>/aaa`(0700 폴더)에 두고 다음 작업에 SHA를 다시 맞춰 쓴다.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// 운영 release 주소(시험은 임시 HTTP 서버로 바꾼다)
pub(crate) const RELEASES: &str = "https://github.com/cLazyZombie/aaa/releases";

#[derive(Deserialize)]
struct Manifest {
    version: String,
    assets: BTreeMap<String, Asset>,
}

#[derive(Deserialize)]
struct Asset {
    url: String,
    sha256: String,
}

/// 받아 확인한 파일
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fetched {
    pub path: PathBuf,
    pub sha256: String,
}

fn hex_ok(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn file_sha(path: &Path) -> Option<String> {
    std::fs::read(path).ok().map(|b| hex::encode(Sha256::digest(b)))
}

/// `x.y.z`(앞자리 0 없음)만 판으로 받는다
pub(crate) fn version_ok(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && (p.len() == 1 || !p.starts_with('0')))
}

/// manifest에서 `key`(`macos-x86_64` 등) asset을 고른다: 판·주소·SHA 꼴을 본다
fn select(raw: &[u8], base: &str, version: &str, key: &str) -> Result<(String, String), String> {
    let m: Manifest = serde_json::from_slice(raw).map_err(|_| "release manifest를 읽지 못했다".to_string())?;
    if m.version.strip_prefix('v').unwrap_or(&m.version) != version {
        return Err(format!("release manifest 판({})이 host 판({version})과 다르다", m.version));
    }
    let asset = m.assets.get(key).ok_or_else(|| format!("aaa {version} release에 {key} 실행 파일이 없다"))?;
    let prefix = format!("{base}/download/v{version}/");
    let file = asset.url.strip_prefix(&prefix).unwrap_or("");
    if file.is_empty() || !file.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)) {
        return Err(format!("{key}: asset 주소가 그 판의 공개 release 파일이 아니다"));
    }
    if !hex_ok(&asset.sha256) {
        return Err(format!("{key}: manifest SHA-256 꼴이 아니다"));
    }
    Ok((asset.url.clone(), asset.sha256.to_ascii_lowercase()))
}

fn curl(url: &str, limit: &str) -> tokio::process::Command {
    let mut c = tokio::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" });
    c.args([
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
    .arg(url)
    .stdin(std::process::Stdio::null())
    .kill_on_drop(true);
    c
}

async fn run(mut c: tokio::process::Command, limit: Duration) -> Result<Vec<u8>, String> {
    match tokio::time::timeout(limit, c.output()).await {
        Ok(Ok(o)) if o.status.success() => Ok(o.stdout),
        Ok(Ok(o)) => Err(String::from_utf8_lossy(&o.stderr).trim().chars().take(200).collect()),
        Ok(Err(e)) => Err(format!("curl을 실행하지 못했다: {e}")),
        Err(_) => Err("시간 안에 받지 못했다".into()),
    }
}

/// `base`(운영은 [`RELEASES`])에서 aaa `version`의 `key` 실행 파일을 받아 `cache`에 둔다. 이미 같은 SHA로 있으면 그대로 쓴다
pub(crate) async fn fetch(cache: &Path, base: &str, version: &str, key: &str) -> Result<Fetched, String> {
    if !version_ok(version) {
        return Err(format!("release 판 꼴이 아니다: {version}"));
    }
    let manifest = run(curl(&format!("{base}/download/v{version}/manifest.json"), "20"), Duration::from_secs(30))
        .await
        .map_err(|e| format!("release manifest 받기: {e}"))?;
    let (url, sha256) = select(&manifest, base, version, key)?;
    let dir = cache.join(format!("v{version}")).join(key);
    let path = dir.join("aaa");
    if file_sha(&path).as_deref() == Some(sha256.as_str()) {
        return Ok(Fetched { path, sha256 });
    }
    aaa_common::util::ensure_private_dir(cache).map_err(|e| e.to_string())?;
    aaa_common::util::ensure_private_dir(&dir).map_err(|e| e.to_string())?;
    let part = dir.join("aaa.part");
    let _ = std::fs::remove_file(&part);
    let mut c = curl(&url, "300");
    c.arg("--output").arg(&part);
    run(c, Duration::from_secs(330)).await.map_err(|e| format!("release 실행 파일 받기: {e}"))?;
    let got = file_sha(&part).unwrap_or_default();
    if got != sha256 {
        let _ = std::fs::remove_file(&part);
        return Err(format!("받은 release 실행 파일의 SHA-256이 manifest와 다르다(기대 {sha256}, 받음 {got})"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(Fetched { path, sha256 })
}
