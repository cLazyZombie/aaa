//! `aaa update`(herdr 0.9.5 `update.rs` `self_update`·`download_update`·`install_downloaded_update`, 차이 목록 4장 A
//! 「업데이트 명령」 지금 504줄, plan §11·§8.180).
//!
//! - 공개 GitHub release의 manifest·OS/arch asset을 받아 SHA-256을 확인하거나 주어진 파일(`--from`)로 바꾼다.
//!   manifest asset 위치는 §8.210. macOS는 기존 배포와 같은 ad-hoc 서명이며 고정 서명·서명 manifest는 없다.
//! - 바꿀 자리는 지금 도는 실행 파일이다(herdr `current_exe`). 운영은 `~/.local/bin/aaa`로 친다(install-runbook 1번).
//!   링크면 거부한다: 링크가 가리키는 파일(빌드 폴더일 수 있다)을 바꾸지 않는다.
//! - 사람만(감독자 명령과 같은 문, `person.rs`): 일반 aaa 세션 안·터미널 아님은 거부하고 yes를 받는다. host가 승인한 coordinator 배포 실행기는 별도 typed 경로다. `--dry-run`은 바꾸지 않아 누구나.
//! - 순서(plan §11 「교체」): 같은 폴더에 staging(`.aaa-update-<pid>.tmp`, herdr와 같은 자리라 rename이 원자적) →
//!   self-check(주어진 파일을 실행해 `--version`이 `aaa x.y.z`, `config check`가 이 Mac 설정을 받는지) → version 고정 검사 →
//!   지금 것을 `.aaa-old/`에 보관(hard link, 실행 파일 자리는 비지 않는다) → rename. 제자리 쓰기는 없다.
//! - Windows(접속만 하는 PC의 `%USERPROFILE%\.local\bin\aaa.exe`, install-runbook 11번)도 같은 순서다. 도는 exe 자리에 rename이
//!   된다(std rename의 POSIX 방식, Windows CI 시험 `replaces_a_running_exe`). herdr Windows는 release 폴더마다 풀고 junction·PATH를
//!   바꾸는 설치 스크립트(`distribution/install.ps1`)를 쓴다: aaa는 고정 경로 하나라 따르지 않았다.
//! - 도는 host·agent는 옛 이미지로 계속 돈다. 다시 띄우기는 사람이 `aaa service restart`로 한다(자동 재시작 없음, plan §11).
//! - 참고와 다름: herdr는 도는 서버를 멈출지·넘길지(`--handoff`) 묻는다. aaa는 session host가 세션을 가져 agent·host를 다시
//!   띄워도 이어진다(결정 3): 바꾼 뒤 다시 띄우기를 알린다. brew·mise·nix 설치 감지는 없다(aaa는 고정 경로 하나).
//!   herdr처럼 최신 release가 같거나 옛 version이면 받지 않는다. --version은 명시한 판(같으면 무동작),
//!   --from은 같은 version의 로컬 빌드도 sha256이 다르면 교체한다.

mod release;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Duration;

use aaa_config::parse_version;
use anyhow::{Context, Result, bail};
use sha2::Digest as _;

/// self-check 한 번의 한도(실패 한도다: 정상 실행 파일은 바로 끝난다)
const CHECK_LIMIT: Duration = Duration::from_secs(30);
const OLD_DIR: &str = ".aaa-old";

/// 바꿀 자리: 지금 도는 실행 파일과 그 옆 보관 폴더
struct Place {
    exe: PathBuf,
    dir: PathBuf,
}

impl Place {
    fn here() -> Result<Place> {
        Self::at(std::env::current_exe().context("지금 실행 파일을 찾지 못했다")?)
    }
    fn at(exe: PathBuf) -> Result<Place> {
        if std::fs::symlink_metadata(&exe).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            bail!(
                "{}는 링크다: 링크가 가리키는 파일을 바꾸지 않는다. 실행 파일을 고정 경로에 파일로 두고(install-runbook 1번) 그 경로로 친다",
                exe.display()
            );
        }
        let dir = exe.parent().context("실행 파일 폴더를 모른다")?.to_path_buf();
        Ok(Place { exe, dir })
    }

    fn old(&self) -> PathBuf {
        self.dir.join(OLD_DIR)
    }

    /// 같은 시각에 여러 판을 보관해도 생성 순서가 이름 차례가 되게 한다.
    fn backup_path(&self, now: u64, version: &str) -> PathBuf {
        let stamp = aaa_common::util::utc_compact(now);
        let prefix = format!("aaa-{stamp}.{:03}.", now % 1000);
        let serial = self
            .kept()
            .iter()
            .filter_map(|p| p.file_name()?.to_str()?.strip_prefix(&prefix)?.split_once('-')?.0.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        self.old().join(format!("{prefix}{serial:020}-{version}"))
    }

    /// 보관한 옛 실행 파일, 새것부터(이름 `aaa-<UTC>-<version>`이라 이름 차례가 시각 차례다)
    fn kept(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(self.old())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("aaa-")))
            .collect();
        v.sort();
        v.reverse();
        v
    }

    /// 되돌릴 실제 보관 파일을 고른다. 이름이 없으면 지금과 다른 가장 새 파일이다.
    fn rollback_source(&self, name: Option<&str>) -> Result<PathBuf> {
        let kept = self.kept();
        if let Some(n) = name {
            return kept
                .into_iter()
                .find(|p| p.file_name().and_then(|x| x.to_str()) == Some(n))
                .with_context(|| format!("보관한 것 가운데 `{n}`가 없다(`aaa update list`)"));
        }
        let cur = sha256(&self.exe)?;
        kept.into_iter().find(|p| sha256(p).is_ok_and(|s| s != cur)).with_context(|| {
            format!("되돌릴 옛 실행 파일이 없다({}가 비었거나 모두 지금과 같다)", self.old().display())
        })
    }
}

/// 보관 이름의 version(`aaa-20261018T120500Z-0.1.0` → `0.1.0`)
fn kept_version(p: &Path) -> String {
    p.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.rsplit_once('-'))
        .map(|(_, v)| v.to_string())
        .unwrap_or_default()
}

fn sha256(p: &Path) -> Result<String> {
    let bytes = std::fs::read(p).with_context(|| format!("{}를 읽지 못했다", p.display()))?;
    Ok(hex::encode(sha2::Sha256::digest(bytes)))
}

/// 실행해 보고 (성공했나, 표준 출력·오류)를 돌려준다. 한도를 넘기면 그 child를 끝내고 실패로 본다.
fn run_limited(bin: &Path, args: &[&str]) -> Result<(bool, String)> {
    let mut child = Command::new(bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("{}를 실행하지 못했다(이 Mac에서 도는 실행 파일인가?)", bin.display()))?;
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut text);
        let mut e = String::new();
        let _ = std::io::Read::read_to_string(&mut err, &mut e);
        let _ = tx.send(text + &e);
    });
    match rx.recv_timeout(CHECK_LIMIT) {
        Ok(text) => Ok((child.wait()?.success(), text)),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            Ok((false, format!("{}초 안에 끝나지 않았다", CHECK_LIMIT.as_secs())))
        }
    }
}

/// self-check(plan §11): 실행되고 aaa version을 말하며, 이 Mac 설정을 받는다. 그 version을 돌려준다.
fn self_check(bin: &Path) -> Result<String> {
    let (ok, text) = run_limited(bin, &["--version"])?;
    let version = text.trim().strip_prefix("aaa ").filter(|v| ok && parse_version(v).is_some());
    let Some(version) = version else {
        bail!("self-check 실패: `--version`이 `aaa x.y.z`가 아니다(aaa 실행 파일인가?): {}", text.trim());
    };
    let (ok, text) = run_limited(bin, &["config", "check"])?;
    if !ok {
        bail!("self-check 실패: 새 실행 파일이 이 Mac 설정을 받지 않는다(`config check`):\n{}", text.trim_end());
    }
    Ok(version.to_string())
}

/// 이 Mac의 `[update]`: (고정한 version과 그 파일, 보관 수). host.toml·agent.toml 중 있는 것 모두
fn settings() -> Result<(Vec<(String, String)>, u32)> {
    let dir = aaa_config::paths::config_dir();
    let host = aaa_config::load_host(&dir).map_err(|e| anyhow::anyhow!("host 설정 오류:\n{}", e.render()))?;
    let agent = aaa_config::load_agent(&dir).map_err(|e| anyhow::anyhow!("agent 설정 오류:\n{}", e.render()))?;
    let mut pins = Vec::new();
    let mut keep = None::<u32>;
    for (name, files, u) in
        [("host.toml", &host.files, &host.config.update), ("agent.toml", &agent.files, &agent.config.update)]
    {
        if files.is_empty() {
            continue;
        }
        if !u.version.is_empty() {
            pins.push((name.to_string(), u.version.clone()));
        }
        keep = Some(keep.map_or(u.keep, |k| k.max(u.keep)));
    }
    Ok((pins, keep.unwrap_or(aaa_config::UpdateSection::default().keep)))
}

/// version 고정(plan §11): 고정했으면 그 version이어야 하고, 고정하지 않았으면 major를 바꾸지 않는다
fn pin_check(new: &str, pins: &[(String, String)]) -> Result<()> {
    for (file, pin) in pins {
        if new != pin {
            bail!(
                "새 실행 파일은 {new}인데 {file} `[update] version = \"{pin}\"`로 고정돼 있다. 바꾸려면 사람이 그 값을 고친다"
            );
        }
    }
    let current = env!("CARGO_PKG_VERSION");
    let major = |v: &str| parse_version(v).map(|(m, _, _)| m);
    if pins.is_empty() && major(new) != major(current) {
        bail!(
            "major가 바뀐다({current} → {new}). 저절로 올리지 않는다(plan §11): 바꾸려면 host.toml이나 agent.toml에 `[update] version = \"{new}\"`로 고정한다"
        );
    }
    Ok(())
}

/// 같은 폴더에 둔 새 실행 파일. 바꾸지 못하고 끝나면 지운다(herdr `DownloadedUpdate`의 Drop)
struct Staged(Option<PathBuf>);

impl Staged {
    fn new(place: &Place) -> Result<Staged> {
        let path = place.dir.join(format!(".aaa-update-{}.tmp", std::process::id()));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("{}에 staging을 만들지 못했다(기존 파일은 덮지 않는다)", path.display()))?;
        Ok(Staged(Some(path)))
    }

    fn copy_from(src: &Path, place: &Place) -> Result<Staged> {
        let staged = Self::new(place)?;
        std::fs::copy(src, staged.path()).with_context(|| {
            format!("{}를 {}에 두지 못했다(그 폴더에 쓸 수 있나?)", src.display(), place.dir.display())
        })?;
        staged.make_executable()?;
        Ok(staged)
    }

    fn make_executable(&self) -> Result<()> {
        #[cfg(unix)]
        std::fs::set_permissions(self.path(), std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
        Ok(())
    }

    fn path(&self) -> &Path {
        self.0.as_deref().unwrap()
    }

    /// 지금 것을 보관하고 이것을 그 자리로 rename한다. 보관 이름을 돌려준다
    fn install(mut self, place: &Place, current_version: &str) -> Result<PathBuf> {
        let old = place.old();
        std::fs::create_dir_all(&old).with_context(|| format!("{}를 만들지 못했다", old.display()))?;
        let kept = place.backup_path(aaa_common::util::now_ms(), current_version);
        // 실행 파일 자리가 비지 않게 링크로 보관한다(같은 폴더 아래라 같은 볼륨). 링크를 못 걸면 복사한다
        if std::fs::hard_link(&place.exe, &kept).is_err() {
            std::fs::copy(&place.exe, &kept)
                .with_context(|| format!("지금 실행 파일을 {}에 보관하지 못했다", kept.display()))?;
        }
        let path = self.0.take().unwrap();
        // Windows도 도는 exe 자리에 그대로 rename한다: std rename이 POSIX 방식(FILE_RENAME_FLAG_POSIX_SEMANTICS)으로 바꾼다
        // (Windows 시험 `replaces_a_running_exe`)
        if let Err(e) = std::fs::rename(&path, &place.exe) {
            let _ = std::fs::remove_file(&path);
            bail!("{}를 바꾸지 못했다: {e}", place.exe.display());
        }
        Ok(kept)
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// 새것부터 `keep`개만 남긴다
fn prune(place: &Place, keep: u32) -> Vec<PathBuf> {
    let mut gone = Vec::new();
    for p in place.kept().into_iter().skip(keep as usize) {
        if std::fs::remove_file(&p).is_ok() {
            gone.push(p);
        }
    }
    gone
}

fn short(sha: &str) -> &str {
    &sha[..12.min(sha.len())]
}

const RESTART: &str = "도는 host·agent는 옛 실행 파일로 계속 돈다. 새것으로 다시 띄우려면 ✋ `aaa service restart --role agent`(host Mac은 `--role host`도). 세션은 이어진다";

/// self-check를 마친 바꿀 것
struct Swap<'a> {
    /// yes 물음에 보이는 일
    what: String,
    staged: Staged,
    new_version: String,
    from: &'a Path,
}

/// 바꾸기와 되돌리기가 함께 쓰는 끝 단계: 보이고, 묻고, 바꾸고, 보관을 줄인다
fn finish(json: bool, place: &Place, swap: Swap, dry_run: bool, keep: u32, approval: Approval) -> Result<ExitCode> {
    let Swap { what, staged, new_version, from } = swap;
    let current =
        if approval == Approval::Coordinator { self_check(&place.exe)? } else { env!("CARGO_PKG_VERSION").to_string() };
    let (cur_sha, new_sha) = (sha256(&place.exe)?, sha256(staged.path())?);
    if cur_sha == new_sha {
        if json {
            println!(
                "{}",
                serde_json::json!({"exe": place.exe, "changed": false, "version": current, "sha256": cur_sha})
            );
        } else {
            println!("이미 같은 실행 파일이다(aaa {current} · sha256 {}): 바꾸지 않았다", short(&cur_sha));
        }
        return Ok(ExitCode::SUCCESS);
    }
    let plan = format!(
        "바꿀 실행 파일: {}\n지금: aaa {current} · sha256 {}\n새것: aaa {new_version} · sha256 {} ({})\n보관: {}/ (새것부터 {keep}개)",
        place.exe.display(),
        short(&cur_sha),
        short(&new_sha),
        from.display(),
        place.old().display(),
    );
    if dry_run {
        if json {
            println!(
                "{}",
                serde_json::json!({"exe": place.exe, "changed": false, "dry_run": true, "from": from,
                "version": current, "sha256": cur_sha, "new_version": new_version, "new_sha256": new_sha})
            );
        } else {
            println!("{plan}\n검사만 했다(--dry-run): self-check·version 고정 통과, 바꾸지 않았다");
        }
        return Ok(ExitCode::SUCCESS);
    }
    println!("{plan}");
    if approval == Approval::Person && crate::person::ask(&format!("{what}. 계속하려면 yes를 입력: "))? != "yes"
    {
        bail!("그만뒀다(바꾸지 않았다)");
    }
    let kept = staged.install(place, &current)?;
    let pruned = prune(place, keep);
    if json {
        println!(
            "{}",
            serde_json::json!({"exe": place.exe, "changed": true, "from": from, "old_version": current,
            "old_sha256": cur_sha, "version": new_version, "sha256": new_sha, "kept": kept, "pruned": pruned})
        );
    } else {
        println!("바꿨다: aaa {current} → aaa {new_version} · 옛것은 {}", kept.display());
        for p in &pruned {
            println!("  보관 수({keep})를 넘어 지웠다: {}", p.display());
        }
        println!("{RESTART}. 되돌리기: `aaa update rollback`");
    }
    Ok(ExitCode::SUCCESS)
}

/// `aaa update [--version <판> | --from <파일>] [--dry-run]`
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Approval {
    Person,
    Coordinator,
}

pub fn update(json: bool, from: Option<PathBuf>, version: Option<String>, dry_run: bool) -> Result<ExitCode> {
    update_approved(json, from, version, dry_run, Approval::Person, None)
}
pub(crate) fn update_approved(
    json: bool,
    from: Option<PathBuf>,
    version: Option<String>,
    dry_run: bool,
    approval: Approval,
    target: Option<PathBuf>,
) -> Result<ExitCode> {
    if from.is_some() && version.is_some() {
        bail!("--from과 --version은 함께 쓰지 않는다");
    }
    let requested = release::requested_version(version.as_deref())?;
    if !dry_run && approval == Approval::Person {
        crate::person::require("실행 파일 바꾸기")?;
    }
    let place = match target {
        Some(exe) => Place::at(exe)?,
        None => Place::here()?,
    };
    if let Some(from) = &from
        && !from.is_file()
    {
        bail!("{}가 파일이 아니다", from.display());
    }
    let current_version =
        if approval == Approval::Coordinator { self_check(&place.exe)? } else { env!("CARGO_PKG_VERSION").to_string() };
    let (pins, keep) = settings()?;
    let (staged, from, expected) = if let Some(from) = from {
        (Staged::copy_from(&from, &place)?, from, None)
    } else {
        let manifest = release::fetch_manifest(&release::manifest_url(requested.as_deref()))?;
        let Some(asset) =
            manifest.select(std::env::consts::OS, std::env::consts::ARCH, &current_version, requested.as_deref())?
        else {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"exe": place.exe, "changed": false,
                    "version": current_version, "release_version": manifest.version})
                );
            } else {
                println!("이미 최신판이다(aaa {} · release {}): 바꾸지 않았다", current_version, manifest.version);
            }
            return Ok(ExitCode::SUCCESS);
        };
        let staged = asset.download(&place)?;
        let from = staged.path().to_path_buf();
        (staged, from, Some(asset.version))
    };
    let new_version = self_check(staged.path())?;
    if let Some(expected) = expected
        && new_version != expected
    {
        bail!("self-check 실패: manifest 판은 {expected}인데 실행 파일은 {new_version}이다");
    }
    pin_check(&new_version, &pins)?;
    let what = format!("{}를 aaa {new_version}로 바꾼다", place.exe.display());
    finish(json, &place, Swap { what, staged, new_version, from: &from }, dry_run, keep, approval)
}

/// `aaa update rollback [<이름>] [--dry-run]`: 보관한 옛 실행 파일로 되돌린다. 이름이 없으면 지금과 다른 가장 새 것
pub fn rollback(json: bool, name: Option<String>, dry_run: bool) -> Result<ExitCode> {
    rollback_approved(json, name, dry_run, Approval::Person, None)
}
pub(crate) fn rollback_approved(
    json: bool,
    name: Option<String>,
    dry_run: bool,
    approval: Approval,
    target: Option<PathBuf>,
) -> Result<ExitCode> {
    if !dry_run && approval == Approval::Person {
        crate::person::require("실행 파일 되돌리기")?;
    }
    let place = match target {
        Some(exe) => Place::at(exe)?,
        None => Place::here()?,
    };
    let chosen = place.rollback_source(name.as_deref())?;
    let (pins, keep) = settings()?;
    let staged = Staged::copy_from(&chosen, &place)?;
    let version = self_check(staged.path())?;
    // 되돌리기는 빠져나가는 길이라 version 고정으로 막지 않고 알리기만 한다
    if let Err(e) = pin_check(&version, &pins) {
        println!("주의: {e}");
    }
    let what = format!("{}를 보관한 aaa {version}로 되돌린다", place.exe.display());
    finish(json, &place, Swap { what, staged, new_version: version, from: &chosen }, dry_run, keep, approval)
}

/// `aaa update list`: 보관한 옛 실행 파일
pub fn list(json: bool) -> Result<ExitCode> {
    let place = Place::here()?;
    let cur = sha256(&place.exe)?;
    let rows: Vec<(String, String, String)> = place
        .kept()
        .iter()
        .map(|p| {
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (name, kept_version(p), sha256(p).unwrap_or_default())
        })
        .collect();
    if json {
        let v: Vec<_> = rows
            .iter()
            .map(|(n, v, s)| serde_json::json!({"name": n, "version": v, "sha256": s, "current": *s == cur}))
            .collect();
        println!("{}", serde_json::json!({"exe": place.exe, "sha256": cur, "kept": v}));
        return Ok(ExitCode::SUCCESS);
    }
    println!("실행 파일: {} · aaa {} · sha256 {}", place.exe.display(), env!("CARGO_PKG_VERSION"), short(&cur));
    if rows.is_empty() {
        println!("보관한 옛 실행 파일 없음({})", place.old().display());
    }
    for (n, v, s) in &rows {
        println!("  {n}  aaa {v} · sha256 {}{}", short(s), if *s == cur { " · 지금과 같음" } else { "" });
    }
    Ok(ExitCode::SUCCESS)
}

/// 실행 중 이미지 판과 별개로, 지금 설치된 파일을 직접 검증한다.
pub(crate) fn installed_identity(exe: &std::path::Path) -> Result<String> {
    Ok(serde_json::json!({"version":self_check(exe)?,"sha256":sha256(exe)?}).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pins_hold_and_major_needs_one() {
        let cur = env!("CARGO_PKG_VERSION");
        let (m, n, p) = parse_version(cur).unwrap();
        let pin = |v: &str| vec![("host.toml".to_string(), v.to_string())];
        assert!(pin_check(cur, &[]).is_ok());
        assert!(pin_check(&format!("{m}.{}.0", n + 1), &[]).is_ok(), "minor는 고정 없이 바꾼다");
        let next_major = format!("{}.0.0", m + 1);
        assert!(pin_check(&next_major, &[]).unwrap_err().to_string().contains("major가 바뀐다"));
        assert!(pin_check(&next_major, &pin(&next_major)).is_ok(), "고정하면 major도 바꾼다");
        let e = pin_check(&format!("{m}.{n}.{}", p + 1), &pin(cur)).unwrap_err().to_string();
        assert!(e.contains("고정돼 있다") && e.contains("host.toml"), "{e}");
        let both = [("host.toml".to_string(), cur.to_string()), ("agent.toml".to_string(), next_major.clone())];
        assert!(pin_check(cur, &both).unwrap_err().to_string().contains("agent.toml"), "둘 다 지킨다");
    }

    /// Windows: 도는 실행 파일 자리를 바꾼다. 시험 binary 사본을 `aaa.exe`로 띄워 둔 채 바꾸면 그 자리는 새것, 보관은 바꾸기 전
    /// 것이고 staging은 남지 않는다
    #[cfg(windows)]
    #[test]
    fn replaces_a_running_exe() {
        use std::io::BufRead as _;
        let dir = std::env::temp_dir().join(format!("aaa-update-win-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("aaa.exe");
        let original = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        std::fs::write(&exe, &original).unwrap();
        let mut child = Command::new(&exe)
            .args(["--ignored", "--exact", "update::tests::hold_until_stdin_closes", "--nocapture"])
            .env("AAA_TEST_HOLD", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        // 사본이 돌기 시작했다(이미지가 올라왔다)는 말을 기다린다
        let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut line = String::new();
        while !line.contains("holding") {
            line.clear();
            assert!(out.read_line(&mut line).unwrap() > 0, "사본이 돌지 않았다");
        }
        let place = Place { exe: exe.clone(), dir: dir.clone() };
        let new = dir.join("new-build");
        std::fs::write(&new, b"new build").unwrap();
        let kept = Staged::copy_from(&new, &place).unwrap().install(&place, "0.1.0").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new build");
        assert_eq!(std::fs::read(&kept).unwrap(), original, "보관한 것은 바꾸기 전(도는) 실행 파일");
        assert!(kept.starts_with(place.old()), "{}", kept.display());
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(".aaa-update-"))
            .collect();
        assert!(left.is_empty(), "staging이 남았다: {left:?}");
        drop(child.stdin.take());
        assert!(child.wait().unwrap().success());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `replaces_a_running_exe`가 띄운 사본: 표준 입력이 닫힐 때까지 돈다(그 변수 없이는 바로 끝난다)
    #[cfg(windows)]
    #[test]
    #[ignore = "replaces_a_running_exe가 사본으로 띄운다"]
    fn hold_until_stdin_closes() {
        if std::env::var_os("AAA_TEST_HOLD").is_none() {
            return;
        }
        println!("holding");
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
    }

    /// 모든 OS가 바꾸기·되돌리기를 받는다(Windows도 거부하지 않는다: 접속만 하는 PC의 `aaa.exe`, plan §8.192). 없는 파일이나
    /// 보관에 없는 이름은 그 까닭으로 멈춘다(검사만이라 실행 파일은 건드리지 않는다)
    #[test]
    fn every_os_takes_update_and_rollback() {
        let none = std::env::temp_dir().join(format!("aaa-update-none-{}", std::process::id()));
        let e = update(false, Some(none.clone()), None, true).unwrap_err().to_string();
        assert!(e.contains(&format!("{}가 파일이 아니다", none.display())), "{e}");
        let e = rollback(false, Some("aaa-none".into()), true).unwrap_err().to_string();
        assert!(e.contains("보관한 것 가운데 `aaa-none`가 없다"), "{e}");
    }

    #[test]
    fn kept_names_carry_the_version() {
        assert_eq!(kept_version(Path::new("/x/.aaa-old/aaa-20261018T120500Z-0.1.0")), "0.1.0");
        assert_eq!(kept_version(Path::new("/x/.aaa-old/aaa-20261018T120500Z2-1.2.3")), "1.2.3");
    }
}
