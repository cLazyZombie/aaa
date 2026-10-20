//! 작업 root 보기·점검·전환(`aaa work-root`와 웹 「머신」의 work_root.manage.v1이 같이 쓴다, 관리 GUI W5c).
//! 로컬 머신 설정만 바꾼다. 준비 기록·이전 root 보존이 설정 rename보다 먼저다. 서비스는 재시작하지 않는다.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};

use super::{RootInfo, RootTransaction, inspect, probe};

/// 다음 시작 설정과 마지막 agent 시작 기록(연결 증명은 아니다)
#[derive(Debug, Clone)]
pub struct Shown {
    pub path: PathBuf,
    pub mount: Option<PathBuf>,
    pub last_start: Option<(PathBuf, Option<PathBuf>)>,
    pub previous: Vec<RootInfo>,
    pub restart_required: bool,
    pub config_file: PathBuf,
}

/// 점검을 통과한 경로
#[derive(Debug, Clone)]
pub struct Checked {
    pub path: PathBuf,
    pub real_path: PathBuf,
    pub mount: Option<PathBuf>,
}

/// 전환 결과
#[derive(Debug, Clone)]
pub struct Switched {
    pub shown: Shown,
    pub changed: bool,
    pub imported: usize,
}

fn config(dir: &Path) -> Result<aaa_config::AgentConfig> {
    aaa_config::load_agent(dir).map(|c| c.config).map_err(|e| anyhow::anyhow!("{}", e.render()))
}
fn paths(cfg: &aaa_config::AgentConfig) -> Result<(PathBuf, Option<PathBuf>)> {
    let home = aaa_config::paths::home();
    Ok((
        aaa_config::work_root::resolve(&cfg.agent.work_root, &home).map_err(anyhow::Error::msg)?,
        aaa_config::work_root::mount(&cfg.agent.work_mount, &home).map_err(anyhow::Error::msg)?,
    ))
}
fn target(
    cfg: &aaa_config::AgentConfig,
    path: Option<&str>,
    mount: Option<&str>,
) -> Result<(PathBuf, Option<PathBuf>)> {
    let (current, current_mount) = paths(cfg)?;
    let root = path
        .map(|s| aaa_config::work_root::resolve(s, &aaa_config::paths::home()))
        .transpose()
        .map_err(anyhow::Error::msg)?
        .unwrap_or_else(|| current.clone());
    let mount = match mount {
        Some(value) => aaa_config::work_root::mount(value, &aaa_config::paths::home()).map_err(anyhow::Error::msg)?,
        None if root == current => current_mount,
        None => None,
    };
    Ok((root, mount))
}

pub fn show(dir: &Path, state: &Path) -> Result<Shown> {
    let cfg = config(dir)?;
    let (path, mut mount) = paths(&cfg)?;
    let saved = inspect(state)?;
    if mount.is_none() {
        mount = saved.as_ref().and_then(|s| s.roots.iter().find(|r| r.path == path)).and_then(|r| r.mount.clone());
    }
    let last_start = saved.as_ref().and_then(|s| s.applied.clone().map(|p| (p, s.applied_mount.clone())));
    let restart_required = saved.as_ref().is_none_or(|s| s.applied.as_ref() != Some(&path) || s.applied_mount != mount);
    let previous =
        saved.as_ref().into_iter().flat_map(|s| s.roots.iter()).filter(|r| r.path != path).cloned().collect();
    Ok(Shown { path, mount, last_start, previous, restart_required, config_file: dir.join("agent.local.toml") })
}

/// 실제 mount와 쓰기·읽기·삭제를 점검한다. 없는 root는 만든다. `path`가 없으면 현재 설정의 경로
pub fn check(dir: &Path, state: &Path, path: Option<&str>, mount: Option<&str>) -> Result<Checked> {
    let cfg = config(dir)?;
    let (path, mount) = target(&cfg, path, mount)?;
    let real_path = probe(state, &path, mount.as_deref())?;
    Ok(Checked { path, real_path, mount })
}

/// 이전 작업은 그대로 두고 다음 agent 시작의 root를 저장한다. 저장 전에 같은 점검을 다시 한다(웹이 점검했어도)
pub async fn set(dir: &Path, state: &Path, path: &str, mount: Option<&str>) -> Result<Switched> {
    let settings = Settings::load(dir)?;
    let cfg = config(dir)?;
    settings.unchanged()?;
    let (current, current_mount) = paths(&cfg)?;
    let (path, mount) = target(&cfg, Some(path), mount)?;
    // 잘못된 새 볼륨이면 이전 root나 상태를 등록하기 전 거부한다.
    probe(state, &path, mount.as_deref())?;
    let mut tx = RootTransaction::open(state)?;
    let saved = inspect(state)?.context("작업 root 상태가 없다")?;
    let mut imported = 0;
    // 이미 가져온 옛 root는 빠진 디스크 때문에 재등록/재가져오기하지 않는다.
    if !saved.roots.iter().any(|r| r.path == current && r.imported) {
        imported += tx.register(&current, current_mount.as_deref())?.1;
    }
    let (registered, count) = tx.register(&path, mount.as_deref())?;
    imported += count;
    tx.protect_worktrees().await?;
    let root_text = path.to_str().context("작업 root가 UTF-8 경로가 아니다")?;
    let mount_text = registered
        .mount
        .as_deref()
        .map(|p| p.to_str().context("mount가 UTF-8 경로가 아니다"))
        .transpose()?
        .unwrap_or("");
    let edited = aaa_config::work_root::edit(settings.local.as_deref().unwrap_or(""), root_text, mount_text)
        .map_err(anyhow::Error::msg)?;
    let changed = settings.save(&edited)?;
    Ok(Switched { shown: show(dir, state)?, changed, imported })
}

struct Settings {
    dir: PathBuf,
    base: Option<String>,
    local: Option<String>,
}
fn read(path: &Path, strict: bool) -> Result<Option<String>> {
    let m = match path.symlink_metadata() {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if strict {
        ensure!(
            m.is_file() && !m.permissions().readonly(),
            "{}: 쓰기 가능한 일반 설정 파일이어야 한다",
            path.display()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            ensure!(m.nlink() == 1, "설정 hardlink를 교체하지 않는다");
        }
    }
    ensure!(path.metadata()?.len() <= 4 << 20, "설정 파일이 4MiB를 넘는다");
    Ok(Some(std::fs::read_to_string(path)?))
}
impl Settings {
    fn load(dir: &Path) -> Result<Self> {
        let local_path = dir.join("agent.local.toml");
        let local = read(&local_path, true)?;
        if local.is_some() {
            std::fs::OpenOptions::new().write(true).open(local_path)?;
        }
        Ok(Self { dir: dir.into(), base: read(&dir.join("agent.toml"), false)?, local })
    }
    fn unchanged(&self) -> Result<()> {
        ensure!(
            read(&self.dir.join("agent.toml"), false)? == self.base
                && read(&self.dir.join("agent.local.toml"), true)? == self.local,
            "작업 root 전환 중 설정이 바뀌었다. 저장하지 않았다"
        );
        Ok(())
    }
    fn save(&self, edited: &str) -> Result<bool> {
        self.unchanged()?;
        if self.local.as_deref() == Some(edited) {
            return Ok(false);
        }
        if !self.dir.exists() {
            aaa_common::util::ensure_private_dir(&self.dir)?;
        }
        let stage = self.dir.join(format!(".aaa-work-root-{}.toml", aaa_common::util::new_id()));
        let result = (|| -> Result<()> {
            // 공용 private atomic writer의 fsync·0600을 재사용한다. 마지막 rename 전에 다시 snapshot을 본다.
            aaa_common::util::write_private(&stage, edited.as_bytes())?;
            self.unchanged()?;
            std::fs::rename(&stage, self.dir.join("agent.local.toml"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&stage);
        }
        result?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_edit_is_preserved_instead_of_replaced_by_a_prepared_setting() {
        let dir = std::env::temp_dir().join(format!("aaa-root-edit-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("agent.local.toml");
        std::fs::write(&file, "# original\n[agent]\nname = \"old\"\n").unwrap();
        let settings = Settings::load(&dir).unwrap();
        let edited = aaa_config::work_root::edit(settings.local.as_deref().unwrap(), "/work/next", "").unwrap();
        let other = "# another editor\n[agent]\nname = \"new\"\n";
        std::fs::write(&file, other).unwrap();
        let error = settings.save(&edited).unwrap_err();
        assert!(error.to_string().contains("설정이 바뀌었다"), "{error:#}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), other);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
