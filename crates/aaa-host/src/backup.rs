//! SQLite 온라인 snapshot → gzip → age. 평문은 host의 private staging에만 둔다.
//! 목적지에는 암호문만 쓰며 새 사본의 내구 쓰기가 끝난 뒤 같은 host의 옛 사본을 정리한다.
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};

use aaa_common::{private_file, util};
use aaa_config::BackupSection;
use age::secrecy::ExposeSecret;
use anyhow::{Context, Result, bail, ensure};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use rusqlite::{
    Connection, OpenFlags, OptionalExtension,
    backup::{Backup, StepResult},
};

use crate::Host;

const LAST_ATTEMPT: &str = "db-backup:last-attempt-ms";
const LAST_SUCCESS: &str = "db-backup:last-success-ms";
const LAST_ERROR: &str = "db-backup:last-error";
const DAY_MS: u64 = 86_400_000;
const SUFFIX: &str = ".db.gz.age";

#[derive(Debug, serde::Serialize)]
pub struct Status {
    pub last_attempt_ms: Option<u64>,
    pub last_success_ms: Option<u64>,
    pub last_error: Option<String>,
}

/// host를 띄우거나 DB schema를 바꾸지 않고 마지막 정기 백업 결과를 읽는다.
pub fn status(state: &Path) -> Result<Status> {
    let db = readonly(&state.join("host.db"))?;
    let value = |key: &str| -> Result<Option<String>> {
        Ok(db.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional()?)
    };
    Ok(Status {
        last_attempt_ms: value(LAST_ATTEMPT)?.map(|s| s.parse()).transpose()?,
        last_success_ms: value(LAST_SUCCESS)?.map(|s| s.parse()).transpose()?,
        last_error: value(LAST_ERROR)?.filter(|s| !s.is_empty()),
    })
}

struct Cleanup(Vec<PathBuf>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            if let Err(e) = fs::remove_file(path)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(path=%path.display(), "백업 임시 파일 정리 실패: {e}");
            }
        }
    }
}

fn sqlite_files(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_path_buf()];
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        out.push(name.into());
    }
    out
}

fn readonly(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .with_context(|| format!("SQLite 읽기 연결 {}", path.display()))
}

fn check(conn: &Connection) -> Result<()> {
    let mut statement = conn.prepare("PRAGMA integrity_check")?;
    let results = statement.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    ensure!(results == ["ok"], "SQLite 무결성 검사 실패: {}", results.join("; "));
    let version: String = conn.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get(0))?;
    ensure!(version == crate::db::SCHEMA_VERSION.to_string(), "지원하지 않는 host DB schema {version}");
    Ok(())
}

fn snapshot(source: &Path, target: &Path) -> Result<()> {
    snapshot_with(source, target, || {})
}

fn snapshot_with(source: &Path, target: &Path, pinned: impl FnOnce()) -> Result<()> {
    let source = readonly(source)?;
    source.busy_timeout(Duration::ZERO)?;
    // 읽기 snapshot을 고정한다. 다른 연결의 쓰기로 backup_step이 처음부터 다시 시작하지 않는다.
    source.execute_batch("BEGIN; SELECT name FROM sqlite_schema LIMIT 1;")?;
    pinned();
    let mut dest = Connection::open(target).context("snapshot 쓰기 연결")?;
    let started = Instant::now();
    {
        let backup = Backup::new(&source, &mut dest).context("SQLite 온라인 백업 시작")?;
        loop {
            ensure!(started.elapsed() < Duration::from_secs(300), "SQLite 온라인 백업의 300초 작업 상한 초과");
            match backup.step(256).context("SQLite 온라인 백업 page 복사")? {
                StepResult::Done => break,
                StepResult::More => {}
                StepResult::Busy => bail!("SQLite 온라인 백업: SQLITE_BUSY(5), 잠긴 연결을 확인한다"),
                StepResult::Locked => bail!("SQLite 온라인 백업: SQLITE_LOCKED(6), 잠긴 연결을 확인한다"),
                _ => bail!("SQLite 온라인 백업: 알 수 없는 step 결과"),
            }
        }
    }
    source.execute_batch("ROLLBACK")?;
    // 원본이 WAL이어도 전달 파일에는 sidecar가 필요 없게 한다.
    dest.pragma_update(None, "journal_mode", "DELETE")?;
    check(&dest).context("snapshot 검증")?;
    dest.close().map_err(|(_, e)| e).context("snapshot 닫기")?;
    private_file::sync_file(target).context("snapshot 내구 쓰기")
}

fn recipient(text: &str) -> Result<age::x25519::Recipient> {
    text.parse().map_err(|e| anyhow::anyhow!("백업 age X25519 공개키 해석 실패: {e}"))
}

/// 새 개인키를 사용자 전용 파일에 쓴다. 기존 파일을 덮어쓰지 않고 공개키만 돌려준다.
pub fn keygen(path: &Path) -> Result<String> {
    let identity = age::x25519::Identity::generate();
    let mut cleanup = Cleanup(Vec::new());
    let mut file = private_file::new_file(path).context("복원 개인키 파일 생성(기존 파일 덮어쓰기 금지)")?;
    cleanup.0.push(path.to_path_buf());
    writeln!(file, "{}", identity.to_string().expose_secret())?;
    file.sync_all()?;
    drop(file);
    private_file::sync_parent(path)?;
    cleanup.0.clear();
    Ok(identity.to_public().to_string())
}

/// 이미 존재하는 목적지 아래 host별 폴더를 쓴다. NAS가 없을 때 mount 자리의 상위 폴더를 만들지 않는다.
pub fn run(state: &Path, cfg: &BackupSection, now: u64) -> Result<PathBuf> {
    ensure!((1..=365).contains(&cfg.keep), "백업 keep은 1~365개여야 한다");
    let public = recipient(&cfg.recipient)?;
    let root = Path::new(&cfg.directory);
    ensure!(root.is_absolute(), "백업 목적지는 절대 경로여야 한다: {}", root.display());
    let metadata = fs::metadata(root).with_context(|| format!("백업 목적지 조회 {}", root.display()))?;
    ensure!(metadata.is_dir(), "백업 목적지가 폴더가 아니다: {}", root.display());
    let staging = state.join("db-backup-staging");
    private_file::directory(&staging).context("백업 private staging 준비")?;
    let _lock = private_file::lock(&staging.join("lock")).context("백업 중복 실행 잠금")?;
    let plain = staging.join("snapshot.db");
    // 이전 비정상 종료의 평문도 자기 staging 안에서만 지운다. 잠금을 가진 실행만 접근한다.
    for path in sqlite_files(&plain) {
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("이전 snapshot 정리 {}", path.display())),
        }
    }
    let _cleanup = Cleanup(sqlite_files(&plain));
    drop(private_file::new_file(&plain).context("private snapshot 생성")?);
    let source = state.join("host.db");
    snapshot(&source, &plain)?;
    let conn = readonly(&plain)?;
    let host_id: String = conn.query_row("SELECT value FROM meta WHERE key='host_id'", [], |r| r.get(0))?;
    ensure!(
        !host_id.is_empty()
            && host_id.len() <= 128
            && host_id.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
        "백업 host ID가 파일 이름으로 안전하지 않다"
    );
    drop(conn);
    let directory = root.join(format!("aaa-host-{host_id}"));
    match fs::create_dir(&directory) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && directory.symlink_metadata()?.is_dir() => {}
        Err(e) => return Err(e).with_context(|| format!("host 백업 폴더 생성 {}", directory.display())),
    }
    let name = format!("host-{now:020}-{}{SUFFIX}", util::new_id());
    let final_path = directory.join(&name);
    let partial = directory.join(format!(".{name}.partial"));
    let _partial_cleanup = Cleanup(vec![partial.clone()]);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)
        .with_context(|| format!("암호문 임시 파일 생성 {}", partial.display()))?;
    let encryptor = age::Encryptor::with_recipients(std::iter::once(&public as &dyn age::Recipient))?;
    let writer = encryptor.wrap_output(file).context("age 암호화 시작")?;
    let mut gzip = GzEncoder::new(writer, Compression::fast());
    std::io::copy(&mut File::open(&plain)?, &mut gzip).context("snapshot 압축·암호화")?;
    let file = gzip.finish()?.finish().context("age 인증 태그 마감")?;
    file.sync_all().context("암호문 내구 쓰기")?;
    drop(file);
    fs::rename(&partial, &final_path).context("완성 백업 공개")?;
    private_file::sync_parent(&final_path).context("백업 폴더 내구 쓰기")?;
    rotate(&directory, cfg.keep as usize, &final_path).context("이전 백업 회전")?;
    Ok(final_path)
}

fn owned_name(name: &str) -> bool {
    let Some(stem) = name.strip_prefix("host-").and_then(|n| n.strip_suffix(SUFFIX)) else { return false };
    let Some((stamp, id)) = stem.split_once('-') else { return false };
    stamp.len() == 20
        && stamp.bytes().all(|b| b.is_ascii_digit())
        && id.len() == 26
        && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

fn rotate(directory: &Path, keep: usize, current: &Path) -> Result<()> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry.path() != current && entry.file_type()?.is_file() && owned_name(&entry.file_name().to_string_lossy()) {
            files.push(entry.path());
        }
    }
    files.sort();
    // 시계가 과거로 돌아가도 방금 완성한 사본을 정리 대상으로 고르지 않는다.
    let remove = files.len().saturating_sub(keep.saturating_sub(1));
    for path in files.into_iter().take(remove) {
        fs::remove_file(&path).with_context(|| format!("백업 회전 삭제 {}", path.display()))?;
    }
    private_file::sync_parent(&directory.join("entry"))?;
    Ok(())
}

/// 복호화·무결성 검사를 거친 새 파일을 만든다. 운영 DB와 기존 파일은 덮어쓰지 않는다.
pub fn restore(backup: &Path, identity_path: &Path, output: &Path) -> Result<()> {
    for path in sqlite_files(output) {
        ensure!(!path.try_exists()?, "복원 대상 또는 SQLite sidecar가 이미 있다: {}", path.display());
    }
    let bytes = private_file::read(identity_path, 16 * 1024).context("복원 개인키 읽기")?;
    let text = std::str::from_utf8(&bytes).context("복원 개인키 UTF-8")?;
    let identities: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| line.parse::<age::x25519::Identity>().map_err(|_| anyhow::anyhow!("age X25519 개인키 형식 오류")))
        .collect::<Result<_>>()?;
    ensure!(!identities.is_empty(), "복원 개인키가 없다");
    let decryptor = age::Decryptor::new(BufReader::new(File::open(backup)?)).context("age 백업 머리 읽기")?;
    let reader = decryptor.decrypt(identities.iter().map(|i| i as &dyn age::Identity)).context("age 복호화 키 확인")?;
    let mut reader = GzDecoder::new(reader);
    let mut cleanup = Cleanup(Vec::new());
    let mut file = private_file::new_file(output).context("새 복원 파일 생성")?;
    cleanup.0 = sqlite_files(output);
    std::io::copy(&mut reader, &mut file).context("백업 복호화·압축 풀기")?;
    // gzip EOF만으로는 age의 마지막 인증 태그 확인을 보장하지 않는다.
    let mut reader = reader.into_inner();
    let extra = std::io::copy(&mut reader, &mut std::io::sink()).context("age 전체 인증 확인")?;
    ensure!(extra == 0, "압축 데이터 뒤에 추가 평문이 있다");
    file.sync_all()?;
    drop(file);
    check(&readonly(output)?).context("복원 DB 검증")?;
    private_file::sync_parent(output)?;
    cleanup.0.clear();
    Ok(())
}

/// 가장 최근의 예정 시각까지만 따라잡는다. 누락한 날 수만큼 몰아서 실행하지 않는다.
fn due(cfg: &BackupSection, last: u64, now: u64) -> bool {
    let Some((h, m)) = cfg.time() else { return false };
    let today = now / DAY_MS * DAY_MS + (u64::from(h) * 60 + u64::from(m)) * 60_000;
    let slot = if today <= now { Some(today) } else { today.checked_sub(DAY_MS) };
    slot.is_some_and(|at| at > last)
}

impl Host {
    pub(crate) fn backup_pass_at(self: &Arc<Self>, now: u64) {
        if !self.cfg.backup.enabled || self.backup_running.load(Ordering::Acquire) {
            return;
        }
        let claim = (|| -> Result<bool> {
            let last = self.db.meta(LAST_ATTEMPT)?.map(|s| s.parse()).transpose()?.unwrap_or(0);
            if !due(&self.cfg.backup, last, now) {
                return Ok(false);
            }
            self.db.set_meta(LAST_ATTEMPT, &now.to_string())?;
            Ok(true)
        })();
        match claim {
            Ok(false) => return,
            Err(e) => {
                self.backup_failed(&e);
                return;
            }
            Ok(true) => {}
        }
        self.backup_running.store(true, Ordering::Release);
        let host = self.clone();
        tokio::spawn(async move {
            let worker = host.clone();
            let result = tokio::task::spawn_blocking(move || run(&worker.state, &worker.cfg.backup, now))
                .await
                .context("DB 백업 worker 종료")
                .and_then(|r| r);
            let result = result.and_then(|path| {
                host.db.set_meta(LAST_SUCCESS, &now.to_string())?;
                host.db.set_meta(LAST_ERROR, "")?;
                tracing::info!(path=%path.display(), "host DB 암호화 백업 완료");
                Ok(())
            });
            if let Err(e) = result {
                host.backup_failed(&e);
            }
            host.backup_running.store(false, Ordering::Release);
            host.bump();
            host.tick_wake.notify_one();
        });
    }

    fn backup_failed(&self, error: &anyhow::Error) {
        let detail = aaa_common::redact::text(&format!("{error:#}"));
        tracing::error!("host DB 백업 실패: {detail}");
        if let Err(e) = self.db.set_meta(LAST_ERROR, &detail) {
            tracing::error!("백업 오류 상태 저장 실패: {e:#}");
        }
        for (project, _) in self.project_directories() {
            match self.db.inbox_open_exists(&project, "system", "host-db-backup", "backup failed") {
                Ok(false) => self.inbox_add_body(
                    &project,
                    "system",
                    "host-db-backup",
                    "backup failed",
                    "host DB 정기 백업에 실패했다",
                    &detail,
                ),
                Ok(true) => {}
                Err(e) => tracing::error!(%project, "백업 실패 inbox 조회 오류: {e:#}"),
            }
        }
    }
}

#[cfg(test)]
mod tests;
