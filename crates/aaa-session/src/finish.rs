//! 격리 계정 thread에 머신 사용자의 finish 파일만 제공한다.

use aaa_common::private_file;
use anyhow::{Context, Result, ensure};
use std::fs::{self, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

pub(super) fn refresh(home: &Path, harness: &str, env: &[(String, String)]) {
    let (key, default) = match harness {
        "claude" => ("CLAUDE_CONFIG_DIR", ".claude"),
        "codex" => ("CODEX_HOME", ".codex"),
        _ => return,
    };
    let local_home = local_home(key, default, env);
    for (parent, name) in [("skills", "finish"), ("agents", "finish-reviewer.md")] {
        if parent == "agents" && harness != "claude" {
            break;
        }
        if let Err(error) = refresh_entry(home, &local_home, parent, name) {
            tracing::warn!(harness, entry = %format_args!("{parent}/{name}"), error = %format_args!("{error:#}"), "finish 전달 실패");
        }
    }
}

fn local_home(key: &str, default: &str, env: &[(String, String)]) -> PathBuf {
    let explicit = env.iter().rev().find(|(name, _)| name == key);
    let configured = explicit.map_or_else(|| std::env::var_os(key), |(_, value)| Some(value.into()));
    configured.filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| {
        let home_key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        std::env::var_os(home_key).map_or_else(|| PathBuf::from("."), PathBuf::from).join(default)
    })
}

fn refresh_entry(home: &Path, local_home: &Path, parent: &str, name: &str) -> Result<()> {
    let home = home.canonicalize().context("격리 홈 확인")?;
    let local_home = match local_home.canonicalize() {
        Ok(path) => {
            ensure!(
                !path.starts_with(&home) && !home.starts_with(path.join(parent).join(name)),
                "사용자 원본과 격리 홈이 겹친다"
            );
            Ok(path)
        }
        Err(error) => Err(error).context("사용자 설정 홈 확인"),
    };
    let is_skill = parent == "skills";
    let dest_dir = home.join(parent);
    let dest = dest_dir.join(name);
    let owned = home.join(format!(".aaa-finish-{parent}"));
    let pending = owned.join("pending");
    let staged = pending.join(name);

    let dest_dir_exists = optional_metadata(&dest_dir).context("격리 홈의 전달 폴더 확인")?;
    if let Some(meta) = &dest_dir_exists {
        ensure!(plain(meta, true), "격리 홈의 전달 폴더가 일반 폴더가 아니다");
    }
    let managed = optional_metadata(&owned).context("aaa 전달 상태 확인")?;
    if let Some(meta) = &managed {
        ensure!(plain(meta, true), "aaa 전달 상태 폴더가 일반 폴더가 아니다");
        clear_pending(&pending).context("미완료 finish 사본 정리")?;
    }
    if let Some(meta) = optional_metadata(&dest).context("격리 홈의 기존 항목 확인")? {
        ensure!(managed.is_some() && plain(&meta, is_skill), "격리 홈에 별도 항목이 이미 있다");
        // 원본이 사라졌거나 읽을 수 없어도 지난 시작의 사본은 내린다.
        private_file::new_directory(&pending).context("이전 finish 사본 정리 폴더 만들기")?;
        fs::rename(&dest, &staged).context("이전 finish 사본 내리기")?;
        clear_pending(&pending).context("이전 finish 사본 정리")?;
    }
    if managed.is_some() {
        fs::remove_dir(&owned).context("이전 finish 전달 상태 정리")?;
    }

    let source_dir = local_home?.join(parent);
    require_plain(&source_dir, true).context("원본 상위 폴더 확인")?;
    let source = source_dir.join(name);
    require_plain(&source, is_skill).context("원본 확인")?;
    if is_skill {
        require_plain(&source.join("SKILL.md"), false).context("원본 SKILL.md 확인")?;
    }
    if dest_dir_exists.is_none() {
        private_file::new_directory(&dest_dir).context("격리 홈의 전달 폴더 만들기")?;
    }
    private_file::new_directory(&owned).context("aaa 전달 상태 폴더 만들기")?;
    private_file::new_directory(&pending).context("finish 사본 준비 폴더 만들기")?;
    let result = copy_entry(&source, &staged, is_skill).and_then(|()| {
        // 스킬 검색 경로에서는 완성된 사본만 보인다.
        fs::rename(&staged, &dest).context("finish 사본 배치")
    });
    let cleanup = clear_pending(&pending).context("finish 준비 폴더 정리");
    if result.is_err() && cleanup.is_ok() {
        aaa_common::diagnostic::ignored(fs::remove_dir(&owned), "빈 finish 전달 상태 정리");
    }
    result.and(cleanup)
}

fn clear_pending(path: &Path) -> Result<()> {
    if let Some(meta) = optional_metadata(path)? {
        ensure!(plain(&meta, true), "finish 준비 폴더가 일반 폴더가 아니다");
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

fn optional_metadata(path: &Path) -> io::Result<Option<Metadata>> {
    match path.symlink_metadata() {
        Ok(meta) => Ok(Some(meta)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn plain(meta: &Metadata, directory: bool) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
        if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return false;
        }
    }
    if directory { meta.is_dir() } else { meta.is_file() }
}

fn require_plain(path: &Path, directory: bool) -> Result<()> {
    ensure!(plain(&path.symlink_metadata()?, directory), "일반 파일 또는 폴더가 필요하다");
    Ok(())
}

fn copy_entry(source: &Path, dest: &Path, directory: bool) -> Result<()> {
    require_plain(source, directory).context("finish 원본 항목 확인")?;
    if directory {
        private_file::new_directory(dest).context("finish 사본 폴더 만들기")?;
        for entry in fs::read_dir(source).context("finish 원본 폴더 읽기")? {
            let entry = entry.context("finish 원본 항목 읽기")?;
            copy_entry(&entry.path(), &dest.join(entry.file_name()), entry.file_type()?.is_dir())?;
        }
        return Ok(());
    }

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let mut input = options.open(source).context("finish 원본 파일 열기")?;
    let meta = input.metadata().context("finish 원본 파일 확인")?;
    ensure!(plain(&meta, false), "finish 원본이 일반 파일이 아니다");
    let mut output = private_file::new_file(dest).context("finish 사본 파일 만들기")?;
    io::copy(&mut input, &mut output).context("finish 파일 복사")?;
    let mut perms = output.metadata()?.permissions();
    perms.set_readonly(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o400 | (meta.permissions().mode() & 0o100));
    }
    output.set_permissions(perms).context("finish 사본 읽기 전용 설정")
}
