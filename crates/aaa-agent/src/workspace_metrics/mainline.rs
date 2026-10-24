//! main checkout과 push된 HEAD를 확인한다. URL 원문(인증 정보 포함 가능)은 전송하지 않는다.
use super::*;
use anyhow::ensure;

pub(super) fn remote_id(url: &str) -> Option<String> {
    let url = url.trim();
    let (authority, path) = if let Some((scheme, rest)) = url.split_once("://") {
        if !matches!(scheme, "https" | "http" | "ssh" | "git") {
            return None;
        }
        rest.split_once('/')?
    } else {
        let (host, path) = url.split_once(':')?;
        if !host.contains('@') {
            return None;
        }
        (host, path)
    };
    let host = authority.rsplit('@').next()?.to_ascii_lowercase();
    let path = path.trim_matches('/').strip_suffix(".git").unwrap_or(path.trim_matches('/'));
    if host.is_empty() || path.is_empty() || path.contains(['?', '#']) {
        return None;
    }
    Some(hex::encode(Sha256::digest(format!("{host}/{path}").as_bytes())))
}

pub(super) struct Mainline {
    pub remote: String,
    pub commit: String,
    /// 최신 HEAD 변경 이후에 시작한 실행만 허용한다. reflog 초 경계는 보수적으로 제외한다.
    pub stable_since_ms: u64,
}
impl Mainline {
    pub fn read(cwd: &Path) -> Result<Self> {
        let remote = remote_id(&lines::text(cwd, &["remote", "get-url", "origin"])?).context("origin 신원 미확인")?;
        ensure!(lines::text(cwd, &["symbolic-ref", "--quiet", "--short", "HEAD"])? == "main", "main checkout 아님");
        let commit = lines::text(cwd, &["rev-parse", "HEAD"])?;
        ensure!(commit == lines::text(cwd, &["rev-parse", "refs/remotes/origin/main"])?, "origin/main push 확인 대기");
        let log = lines::text(cwd, &["reflog", "show", "-1", "--format=%H%n%gD", "--date=unix", "HEAD"])?;
        let mut rows = log.lines();
        ensure!(rows.next() == Some(&commit), "HEAD 변경 기록 불일치");
        let at = rows
            .next()
            .and_then(|s| s.strip_prefix("HEAD@{"))
            .and_then(|s| s.strip_suffix('}'))
            .context("HEAD 변경 기록 없음")?
            .parse::<u64>()?;
        Ok(Self { remote, commit, stable_since_ms: at.saturating_add(1).saturating_mul(1000) })
    }
    pub fn accepts(&self, start_ms: u64, end_ms: u64) -> bool {
        start_ms >= self.stable_since_ms && start_ms <= end_ms
    }
}
