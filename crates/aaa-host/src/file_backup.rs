//! 첨부(프로젝트 `uploads/`)의 복원 단위. tar 묶음을 기존 DB 백업의 gzip·age 절차로 보관한다.
//! 옛 아이디어 노트(`ideas/`)는 지웠다(t-1959): 묶지 않고, 옛 백업을 복원할 때 그 항목과 번호 원장은 건너뛴다.
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use aaa_common::private_file;
use aaa_config::HostConfig;
use anyhow::{Context, Result, ensure};

use crate::Host;
use crate::backup;
use crate::orch::internal;

/// 옛 백업의 아이디어 번호 원장(복원 때 건너뛴다)
const OLD_INDEX: &str = "idea-index.json";
const SUFFIX: &str = ".tar.gz.age";
const MAX_FILE: u64 = 32 * 1024 * 1024;

/// 설치 예약과 달리 std 파일 잠금을 쓴다. 쓰기는 기다리고 백업은 겹치면 실패한다.
pub(crate) struct FilesLock(File);
impl Drop for FilesLock {
    fn drop(&mut self) {
        if let Err(error) = self.0.unlock() {
            tracing::error!(%error, "첨부 snapshot 잠금 해제 실패");
        }
    }
}
pub(crate) fn files_lock(state: &Path, wait: bool) -> Result<FilesLock> {
    Ok(FilesLock(private_file::coordinated_lock(&state.join("files.lock"), wait).context("첨부 저장/snapshot 잠금")?))
}

/// 첨부 쓰기는 한 번에 하나
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 프로세스 안 순서와 프로세스 사이 snapshot 경계를 함께 잡는다.
pub(crate) struct Writing {
    _local: std::sync::MutexGuard<'static, ()>,
    _file: FilesLock,
}
impl Host {
    pub(crate) fn files_writing(&self) -> Result<Writing, aaa_proto::ErrorBody> {
        let local = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let file = files_lock(&self.state, true).map_err(internal)?;
        Ok(Writing { _local: local, _file: file })
    }
}

struct Plain(PathBuf);
impl Drop for Plain {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            tracing::error!(%error, "파일 백업 평문 정리 실패");
        }
    }
}

fn projects_root(cfg: &HostConfig) -> PathBuf {
    if cfg.projects.root.is_empty() {
        aaa_config::paths::projects_root_default()
    } else {
        PathBuf::from(&cfg.projects.root)
    }
}

pub fn status(state: &Path) -> Result<backup::Status> {
    backup::file_status(state)
}

/// host를 새로 띄우거나 운영 설정을 바꾸지 않는다. 평문은 private staging에만 둔다.
pub fn run(state: &Path, cfg: &HostConfig, now: u64) -> Result<PathBuf> {
    let effective = cfg.file_backup.effective(&cfg.backup);
    backup::recipient(&effective.recipient)?;
    ensure!((1..=365).contains(&effective.keep), "파일 백업 keep은 1~365개여야 한다");
    let destination = Path::new(&effective.directory);
    ensure!(destination.is_absolute() && destination.is_dir(), "백업 목적지는 미리 존재하는 절대 경로 폴더여야 한다");
    let staging = state.join("file-backup-staging");
    private_file::directory(&staging)?;
    let _owner =
        FilesLock(private_file::coordinated_lock(&staging.join("lock"), false).context("파일 백업 중복 실행 잠금")?);
    let plain = staging.join("snapshot.tar");
    match fs::remove_file(&plain) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).context("이전 파일 snapshot 정리"),
    }
    let _cleanup = Plain(plain.clone());
    snapshot(state, &projects_root(cfg), &plain)?;
    // 큰 압축·암호화·NAS 쓰기 동안에는 저장 잠금을 보유하지 않는다.
    backup::publish(state, &effective, now, &plain, "files", SUFFIX)
}

fn snapshot(state: &Path, root: &Path, plain: &Path) -> Result<()> {
    let _local = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let _files = files_lock(state, false)?;
    let mut tar = tar::Builder::new(private_file::new_file(plain)?);
    // 없는 root를 빈 백업으로 보고하지 않는다. 읽기 오류도 그대로 실패한다.
    for entry in fs::read_dir(root).context("백업 프로젝트 root 읽기")? {
        let entry = entry?;
        let name = entry.file_name().into_string().map_err(|_| anyhow::anyhow!("프로젝트 이름 UTF-8 오류"))?;
        if !aaa_proto::valid_project(&name) {
            continue;
        }
        ensure!(entry.file_type()?.is_dir(), "백업 프로젝트가 일반 폴더가 아니다: {name}");
        let project = entry.path();
        if !project.join("PROJECT.md").try_exists()? {
            continue;
        }
        let relative = Path::new("projects").join(&name);
        append_tree(&mut tar, &project.join("PROJECT.md"), &relative.join("PROJECT.md"))?;
        let path = project.join("uploads");
        match path.symlink_metadata() {
            Ok(_) => append_tree(&mut tar, &path, &relative.join("uploads"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("백업 폴더 조회 {}", path.display())),
        }
    }
    let file = tar.into_inner().context("tar snapshot 마감")?;
    file.sync_all().context("tar snapshot 내구 쓰기")
}

fn append<W: Write>(tar: &mut tar::Builder<W>, relative: &Path, size: u64, reader: impl Read) -> Result<()> {
    let name = relative
        .components()
        .map(|c| c.as_os_str().to_str().ok_or_else(|| anyhow::anyhow!("백업 경로 UTF-8 오류")))
        .collect::<Result<Vec<_>>>()?
        .join("/");
    ensure!(safe_entry(&name), "백업에 허용하지 않는 경로: {}", relative.display());
    ensure!(size <= MAX_FILE, "백업 파일이 32MiB를 넘는다: {}", relative.display());
    let mut header = tar::Header::new_ustar();
    header.set_size(size);
    header.set_mode(0o600);
    header.set_cksum();
    tar.append_data(&mut header, &name, reader).with_context(|| format!("tar 파일 추가 {}", relative.display()))?;
    Ok(())
}

fn append_tree<W: Write>(tar: &mut tar::Builder<W>, source: &Path, relative: &Path) -> Result<()> {
    let meta = source.symlink_metadata()?;
    ensure!(!meta.is_symlink(), "백업 원본 symlink는 허용하지 않는다: {}", source.display());
    if meta.is_dir() {
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let name = entry.file_name();
            // 저장의 임시 파일은 복원 단위가 아니다.
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            append_tree(tar, &entry.path(), &relative.join(name))?;
        }
    } else {
        ensure!(meta.is_file(), "백업 원본이 일반 파일이 아니다: {}", source.display());
        append(tar, relative, meta.len(), File::open(source)?)?;
    }
    Ok(())
}

struct RestoreCleanup(Option<PathBuf>);
impl Drop for RestoreCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0
            && let Err(error) = fs::remove_dir_all(path)
        {
            tracing::error!(%error, "불완전한 파일 복원 폴더 정리 실패");
        }
    }
}

/// 새 폴더에만 복원한다. 인증 완료 뒤 regular entry만 직접 쓰며 tar unpack은 호출하지 않는다.
pub fn restore(backup_path: &Path, identity: &Path, output: &Path) -> Result<()> {
    private_file::new_directory(output).context("새 파일 복원 폴더 생성(기존 경로 덮어쓰기 금지)")?;
    let mut cleanup = RestoreCleanup(Some(output.to_path_buf()));
    let plain = output.join(".snapshot.tar");
    backup::decrypt_file(backup_path, identity, &plain)?;
    let mut tar = tar::Archive::new(File::open(&plain)?);
    for entry in tar.entries()? {
        let mut entry = entry?;
        ensure!(entry.header().entry_type().is_file(), "복원 tar에 일반 파일 이외 항목이 있다");
        ensure!(entry.size() <= MAX_FILE, "복원 파일이 32MiB를 넘는다");
        // Windows Path로 바꾸기 전에 tar의 원래 구분자와 경로를 검사한다.
        let path = String::from_utf8(entry.path_bytes().into_owned()).context("복원 경로 UTF-8")?;
        ensure!(safe_entry(&path), "허용하지 않는 복원 경로: {path}");
        // 옛 백업의 아이디어 노트·번호 원장은 복원하지 않는다(t-1959)
        if old_idea_entry(&path) {
            continue;
        }
        let target = output.join(path);
        private_file::parents(target.parent().unwrap())?;
        let mut file = private_file::new_file(&target).context("새 복원 항목 생성(중복 경로 금지)")?;
        std::io::copy(&mut entry, &mut file)?;
        file.sync_all()?;
        private_file::sync_parent(&target)?;
    }
    let mut tail = tar.into_inner();
    let mut buffer = [0u8; 8192];
    loop {
        let n = tail.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        ensure!(buffer[..n].iter().all(|b| *b == 0), "tar 뒤에 추가 데이터가 있다");
    }
    private_file::directory(&output.join("projects"))?;
    fs::remove_file(plain)?;
    private_file::sync_parent(&output.join("entry"))?;
    private_file::sync_parent(output)?;
    cleanup.0 = None;
    Ok(())
}

/// 옛 백업의 아이디어 항목(번호 원장·`projects/<p>/ideas/…`)
fn old_idea_entry(text: &str) -> bool {
    text == OLD_INDEX || matches!(text.split('/').collect::<Vec<_>>().as_slice(), ["projects", _, "ideas", ..])
}

fn safe_entry(text: &str) -> bool {
    if text == OLD_INDEX {
        return true;
    }
    let parts: Vec<_> = text.split('/').collect();
    if parts.iter().any(|p| {
        p.is_empty()
            || *p == "."
            || *p == ".."
            || p.contains(['\\', ':'])
            || p.ends_with(['.', ' '])
            || p.chars().any(char::is_control)
            || matches!(
                p.split('.').next().unwrap_or("").to_ascii_uppercase().as_str(),
                "CON"
                    | "PRN"
                    | "AUX"
                    | "NUL"
                    | "COM1"
                    | "COM2"
                    | "COM3"
                    | "COM4"
                    | "COM5"
                    | "COM6"
                    | "COM7"
                    | "COM8"
                    | "COM9"
                    | "LPT1"
                    | "LPT2"
                    | "LPT3"
                    | "LPT4"
                    | "LPT5"
                    | "LPT6"
                    | "LPT7"
                    | "LPT8"
                    | "LPT9"
            )
    }) {
        return false;
    }
    matches!(parts.as_slice(), ["projects", project, "PROJECT.md"] if aaa_proto::valid_project(project))
        || matches!(parts.as_slice(), ["projects", project, kind, rest @ ..]
            if aaa_proto::valid_project(project) && ["ideas", "uploads"].contains(kind) && !rest.is_empty())
}

#[cfg(test)]
mod tests;
