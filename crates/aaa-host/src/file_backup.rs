//! 아이디어·첨부의 복원 단위. tar 묶음을 기존 DB 백업의 gzip·age 절차로 보관한다.
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use aaa_common::private_file;
use aaa_config::HostConfig;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

use crate::{backup, ideas};

const INDEX: &str = "idea-index.json";
const SUFFIX: &str = ".tar.gz.age";
const MAX_FILE: u64 = 32 * 1024 * 1024;

/// 설치 예약과 달리 std 파일 잠금을 쓴다. 쓰기는 기다리고 백업은 겹치면 실패한다.
pub(crate) struct FilesLock(File);
impl Drop for FilesLock {
    fn drop(&mut self) {
        if let Err(error) = self.0.unlock() {
            tracing::error!(%error, "아이디어 snapshot 잠금 해제 실패");
        }
    }
}
pub(crate) fn files_lock(state: &Path, wait: bool) -> Result<FilesLock> {
    Ok(FilesLock(
        private_file::coordinated_lock(&state.join("idea-files.lock"), wait)
            .context("아이디어·첨부 저장/snapshot 잠금")?,
    ))
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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema: u32,
    /// 번호를 재사용하지 않도록 삭제된 노트의 예약 번호도 보존한다.
    ideas: Vec<(String, String, i64)>,
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
    let _local = ideas::WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let _files = files_lock(state, false)?;
    let conn = backup::readonly(&state.join("host.db"))?;
    let mut statement = conn.prepare("SELECT project,idea_id,number FROM idea_ids ORDER BY project,number")?;
    let index = Index {
        schema: 1,
        ideas: statement.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<rusqlite::Result<_>>()?,
    };
    let mut tar = tar::Builder::new(private_file::new_file(plain)?);
    let bytes = serde_json::to_vec(&index)?;
    append(&mut tar, Path::new(INDEX), bytes.len() as u64, bytes.as_slice())?;
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
        validate_project(&project)?;
        let relative = Path::new("projects").join(&name);
        append_tree(&mut tar, &project.join("PROJECT.md"), &relative.join("PROJECT.md"))?;
        for dir in ["ideas", "uploads"] {
            let path = project.join(dir);
            match path.symlink_metadata() {
                Ok(_) => append_tree(&mut tar, &path, &relative.join(dir))?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e).with_context(|| format!("백업 폴더 조회 {}", path.display())),
            }
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

fn validate_project(project: &Path) -> Result<()> {
    let root = project.join("ideas");
    if !root.try_exists()? {
        return Ok(());
    }
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !ideas::valid_id(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let folder = entry.path();
        ensure!(entry.file_type()?.is_dir(), "아이디어가 일반 폴더가 아니다: {}", folder.display());
        let meta: ideas::Meta = serde_json::from_slice(&fs::read(folder.join("idea.json"))?)
            .with_context(|| format!("아이디어 메타 검증 {}", folder.display()))?;
        let note = fs::read(folder.join("note.md")).context("현재 노트 읽기")?;
        ensure!(
            note == fs::read(folder.join(format!("revs/{:04}.md", meta.rev)))?,
            "현재 노트와 판 기록이 다르다: {}",
            folder.display()
        );
        validate_media(&folder, std::str::from_utf8(&note)?)?;
        for name in meta.media.keys() {
            ensure!(ideas::media_name(name), "아이디어 미디어 이름이 올바르지 않다");
            ensure!(folder.join(name).symlink_metadata()?.is_file(), "메타데이터의 그림이 없다: {name}");
        }
        for rev in &meta.history {
            let text = fs::read_to_string(folder.join(format!("revs/{:04}.md", rev.rev)))?;
            validate_media(&folder, &text)?;
        }
    }
    Ok(())
}

fn validate_media(folder: &Path, text: &str) -> Result<()> {
    for name in ideas::media_refs(text) {
        ensure!(folder.join(&name).symlink_metadata()?.is_file(), "노트·판 기록의 그림이 없다: {name}");
    }
    for (_, target) in ideas::media_targets(text) {
        let relative = target.strip_prefix("./").unwrap_or(&target);
        ensure!(
            !relative.starts_with("library/")
                && !relative.starts_with("uploads/")
                && !relative.starts_with("file://")
                && !Path::new(relative).is_absolute(),
            "아이디어에 아직 폴더 밖 로컬 링크가 있다. 해당 판을 열어 미디어 이행을 확인한다: {}",
            folder.display()
        );
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
    let index: Index = serde_json::from_slice(&fs::read(output.join(INDEX))?)?;
    ensure!(index.schema == 1, "지원하지 않는 아이디어 백업 schema {}", index.schema);
    private_file::directory(&output.join("projects"))?;
    let host = output.join("host");
    private_file::new_directory(&host)?;
    // 전체 host DB 백업이 아니다. 새 복원용 DB에는 아이디어 번호 원장만 넣는다.
    // SQLite의 기본 생성은 Windows 승격 token의 기본 owner를 쓸 수 있다.
    // 개인키·평문 snapshot과 같이 처음부터 private 파일로 만든다.
    drop(private_file::new_file(&host.join("host.db")).context("복원 번호 DB private 파일 생성")?);
    drop(crate::db::Db::open(&host.join("host.db"))?);
    let mut db = rusqlite::Connection::open(host.join("host.db"))?;
    let tx = db.transaction()?;
    for (project, idea, number) in index.ideas {
        ensure!(
            aaa_proto::valid_project(&project) && ideas::valid_id(&idea) && number > 0,
            "아이디어 번호 원장이 올바르지 않다"
        );
        tx.execute(
            "INSERT INTO idea_ids(project,idea_id,number) VALUES(?1,?2,?3)",
            rusqlite::params![project, idea, number],
        )?;
    }
    tx.commit()?;
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA journal_mode=DELETE;")?;
    drop(db);
    if output.join("projects").try_exists()? {
        for project in fs::read_dir(output.join("projects"))? {
            validate_project(&project?.path())?;
        }
    }
    fs::remove_file(plain)?;
    private_file::sync_file(&host.join("host.db")).context("복원 번호 DB private 검증·내구 쓰기")?;
    private_file::sync_parent(&host.join("host.db"))?;
    private_file::sync_parent(&output.join("entry"))?;
    private_file::sync_parent(output)?;
    cleanup.0 = None;
    Ok(())
}

fn safe_entry(text: &str) -> bool {
    if text == INDEX {
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
