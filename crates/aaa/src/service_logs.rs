//! 감독자가 연결한 역할 로그의 stdout·stderr를 한 writer로 모아 회전한다.
//! launcher는 기존 fd를 그대로 가지며, worker만 새 파일로 이어 쓴다.

use aaa_config::LogsSection;
use aaa_proto::install::InstallRole;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const MAX_LINE: usize = 256 * 1024;
const TRUNCATED: &[u8] = " [로그 줄 잘림]\n".as_bytes();
const CHECK_AGE: Duration = Duration::from_secs(3600);
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(60);
const STD_FDS: [RawFd; 2] = [libc::STDOUT_FILENO, libc::STDERR_FILENO];

pub(crate) struct Capture {
    original: [File; 2],
    socket: UnixStream,
    drain: Option<JoinHandle<File>>,
}

pub(crate) fn direct(role: InstallRole) -> Option<Capture> {
    let name = match role {
        InstallRole::Host => "host",
        InstallRole::Agent => "agent",
    };
    #[cfg(target_os = "macos")]
    let path = crate::launchd::log_path(name);
    #[cfg(not(target_os = "macos"))]
    let path = aaa_config::paths::state_dir().join(format!("{name}.log"));
    start(&path, role)
}

pub(crate) fn start(path: &Path, role: InstallRole) -> Option<Capture> {
    // 두 fd 모두 이 역할의 실제 파일에 연결된 경우만 관리한다. 일반 CLI, 터미널,
    // 파이프와 사용자가 별도 이름으로 리다이렉트한 파일은 감독자의 파일이 아니다.
    let expected = fs::symlink_metadata(path).ok()?;
    let original = [duplicate(STD_FDS[0]).ok()?, duplicate(STD_FDS[1]).ok()?];
    if !owned_file(&expected)
        || !original.iter().all(|file| file.metadata().is_ok_and(|meta| same_file(&meta, &expected)))
    {
        return None;
    }
    let config = aaa_config::paths::config_dir();
    let policy = match role {
        InstallRole::Host => aaa_config::load_host(&config).map(|loaded| loaded.config.logs),
        InstallRole::Agent => aaa_config::load_agent(&config).map(|loaded| loaded.config.logs),
    }
    .unwrap_or_default();
    match Capture::open(path, expected, original, policy) {
        Ok(capture) => Some(capture),
        Err(error) => {
            eprintln!("aaa: 운영 로그 회전 시작 실패: {error}");
            None
        }
    }
}

fn duplicate(fd: RawFd) -> io::Result<File> {
    // SAFETY: fcntl은 닫힌 fd를 거부하고 성공하면 호출자가 소유할 새 fd를 반환한다.
    let copy = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
    if copy < 0 { Err(io::Error::last_os_error()) } else { Ok(unsafe { File::from_raw_fd(copy) }) }
}

fn set_fd(from: RawFd, to: RawFd) -> io::Result<()> {
    loop {
        // SAFETY: 호출자가 from을 소유하며, to는 이 프로세스의 고정 stdout/stderr 자리다.
        if unsafe { libc::dup2(from, to) } >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn restore(original: &[File; 2], active: Option<&File>) {
    aaa_common::child_process::with_spawn_lock(|| {
        for (old, fd) in original.iter().zip(STD_FDS) {
            if let Err(error) = set_fd(active.unwrap_or(old).as_raw_fd(), fd) {
                let _ = writeln!(&*old, "aaa: 운영 로그 출력 복원 실패: {error}");
            }
        }
    });
}

impl Capture {
    fn open(path: &Path, expected: Metadata, original: [File; 2], policy: LogsSection) -> io::Result<Self> {
        let file = RotatingFile::open(path, &expected, policy)?;
        let (input, socket) = aaa_common::child_process::with_spawn_lock(UnixStream::pair)?;
        input.set_read_timeout(Some(Duration::from_secs(60)))?;
        let connected = aaa_common::child_process::with_spawn_lock(|| {
            set_fd(socket.as_raw_fd(), STD_FDS[0])?;
            set_fd(socket.as_raw_fd(), STD_FDS[1])
        });
        if let Err(error) = connected {
            restore(&original, None);
            return Err(error);
        }
        let drain = std::thread::Builder::new().name("aaa-service-log".into()).spawn(move || file.drain(input));
        match drain {
            Ok(drain) => Ok(Self { original, socket, drain: Some(drain) }),
            Err(error) => {
                restore(&original, None);
                Err(error)
            }
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        // 자식이 상속한 출력 fd가 열려 있더라도 EOF를 보낸다. worker exec 전에
        // 새 활성 파일을 stdout·stderr에 연결하여 다음 worker가 다시 소유할 수 있게 한다.
        let _ = io::stdout().flush();
        let _ = self.socket.shutdown(Shutdown::Write);
        let active = self.drain.take().and_then(|drain| drain.join().ok());
        restore(&self.original, active.as_ref());
    }
}

fn same_file(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

fn owned_file(meta: &Metadata) -> bool {
    meta.is_file() && meta.nlink() == 1 && meta.uid() == unsafe { libc::geteuid() }
}

fn append() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.append(true).mode(0o600).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    options
}

struct RotatingFile {
    path: PathBuf,
    archive_prefix: String,
    active: File,
    bytes: u64,
    max_bytes: u64,
    keep_files: usize,
    keep_ms: u64,
    last_rotation_ms: u64,
    next_rotation_attempt: Instant,
    next_maintenance: Instant,
    next_warning: Instant,
    _lock: File,
}

impl RotatingFile {
    fn open(path: &Path, expected: &Metadata, policy: LogsSection) -> io::Result<Self> {
        let parent = path.parent().ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let dir = fs::metadata(parent)?;
        // Unix setup은 umask에 따라 그룹 쓰기 가능한 상태 폴더를 만든다.
        // 열린 파일의 소유자·inode를 확인하고, 누구나 쓰는 폴더만 거부한다.
        if !dir.is_dir() || dir.uid() != expected.uid() || dir.mode() & 0o002 != 0 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "로그 폴더 소유권 또는 쓰기 권한이 다르다"));
        }
        let name = path.file_name().and_then(|name| name.to_str()).ok_or(io::ErrorKind::InvalidInput)?;
        let lock = append().create(true).open(parent.join(format!(".{name}.rotate.lock")))?;
        let meta = lock.metadata()?;
        if !owned_file(&meta) || meta.mode() & 0o077 != 0 {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "로그 회전 잠금이 사용자 전용 파일이 아니다"));
        }
        // SAFETY: 유효한 파일을 잠그고 writer 종료까지 보유한다.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
            return Err(io::Error::last_os_error());
        }
        let active = append().open(path)?;
        let meta = active.metadata()?;
        if !owned_file(&meta) || !same_file(&meta, expected) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "로그 파일이 감독자가 연 파일과 다르다"));
        }
        let now = Instant::now();
        Ok(Self {
            path: path.to_owned(),
            archive_prefix: format!("{name}.rotated-"),
            active,
            bytes: meta.len(),
            max_bytes: u64::from(policy.max_file_mib) * 1024 * 1024,
            keep_files: policy.keep_files as usize,
            keep_ms: u64::from(policy.keep_days) * 86_400_000,
            last_rotation_ms: 0,
            next_rotation_attempt: now,
            next_maintenance: now,
            next_warning: now,
            _lock: lock,
        })
    }

    fn parent(&self) -> &Path {
        self.path.parent().expect("열 때 확인한 로그 폴더")
    }

    fn rotate(&mut self, now_ms: u64) -> io::Result<()> {
        let on_disk = fs::symlink_metadata(&self.path)?;
        if !owned_file(&on_disk) || !same_file(&self.active.metadata()?, &on_disk) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "활성 로그가 바뀌었다"));
        }
        let at = now_ms.max(self.last_rotation_ms.saturating_add(1));
        let archived = self.parent().join(format!("{}{at}-{}", self.archive_prefix, aaa_common::util::new_id()));
        fs::rename(&self.path, &archived)?;
        match append().create_new(true).open(&self.path) {
            Ok(active) => {
                self.active = active;
                self.bytes = 0;
                self.last_rotation_ms = at;
                self.maintain(now_ms);
                Ok(())
            }
            Err(error) => {
                // 이름이 이미 생겼으면 덮지 않는다. 열어 둔 파일에는 계속 쓸 수 있다.
                if matches!(fs::symlink_metadata(&self.path), Err(e) if e.kind() == io::ErrorKind::NotFound)
                    && let Err(restore) = fs::rename(&archived, &self.path)
                {
                    self.warn("활성 이름 복구", &restore);
                }
                Err(error)
            }
        }
    }

    fn prune(&mut self, now_ms: u64) -> io::Result<()> {
        let mut archives = Vec::new();
        let active = self.active.metadata()?;
        for entry in fs::read_dir(self.parent())? {
            let entry = entry?;
            let name = entry.file_name();
            let Some((at, id)) = name
                .to_str()
                .and_then(|name| name.strip_prefix(&self.archive_prefix))
                .and_then(|suffix| suffix.split_once('-'))
            else {
                continue;
            };
            let Ok(at) = at.parse::<u64>() else { continue };
            if id.len() != 26 || !id.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase()) {
                continue;
            }
            let path = entry.path();
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if owned_file(&meta) && !same_file(&meta, &active) {
                archives.push((at, path));
            }
        }
        archives.sort_unstable_by(|a, b| b.cmp(a));
        if let Some((latest, _)) = archives.first() {
            self.last_rotation_ms = self.last_rotation_ms.max(*latest);
        }
        let mut first_error = None;
        for (index, (at, path)) in archives.into_iter().enumerate() {
            if (index >= self.keep_files || (self.keep_ms > 0 && now_ms.saturating_sub(at) >= self.keep_ms))
                && let Err(error) = fs::remove_file(&path)
                && error.kind() != io::ErrorKind::NotFound
            {
                first_error.get_or_insert(error);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    fn maintain(&mut self, now_ms: u64) {
        self.next_maintenance = Instant::now() + CHECK_AGE;
        if let Err(error) = self.prune(now_ms) {
            self.warn("회전 사본 정리", &error);
        }
    }

    fn warn(&mut self, stage: &str, error: &io::Error) {
        if Instant::now() < self.next_warning {
            return;
        }
        self.next_warning = Instant::now() + RETRY_AFTER_ERROR;
        let message = format!("aaa: 운영 로그 {stage} 실패: {error}\n");
        let _ = self.write_bytes(message.as_bytes());
    }

    fn write_bytes(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            match self.active.write(bytes) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.bytes = self.bytes.saturating_add(n as u64);
                    bytes = &bytes[n..];
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }

    fn rotate_if_needed(&mut self, extra: usize) {
        let now = Instant::now();
        if self.bytes > 0
            && self.bytes.saturating_add(extra as u64) > self.max_bytes
            && now >= self.next_rotation_attempt
            && let Err(error) = self.rotate(aaa_common::util::now_ms())
        {
            self.next_rotation_attempt = now + RETRY_AFTER_ERROR;
            self.warn("회전", &error);
        }
    }

    fn record(&mut self, line: &[u8]) {
        self.rotate_if_needed(line.len());
        if let Err(error) = self.write_bytes(line) {
            self.warn("쓰기", &error);
        }
    }

    fn drain(mut self, mut input: UnixStream) -> File {
        self.maintain(aaa_common::util::now_ms());
        self.rotate_if_needed(0); // 첫 적용에서 이미 큰 파일은 통째로 보관한다.
        let mut text = Lines::default();
        let mut buf = [0; 8192];
        loop {
            if Instant::now() >= self.next_maintenance {
                self.maintain(aaa_common::util::now_ms());
            }
            match input.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => text.accept(&buf[..n], |line| self.record(line)),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) => {}
                Err(error) => {
                    self.warn("출력 읽기", &error);
                    break;
                }
            }
        }
        text.finish(|line| self.record(line));
        self.active
    }
}

#[derive(Default)]
struct Lines {
    bytes: Vec<u8>,
    truncated: bool,
}

impl Lines {
    fn accept(&mut self, bytes: &[u8], mut record: impl FnMut(&[u8])) {
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            let copied = part.len().min(MAX_LINE.saturating_sub(self.bytes.len()));
            self.bytes.extend_from_slice(&part[..copied]);
            self.truncated |= copied < part.len();
            if part.ends_with(b"\n") {
                self.finish(&mut record);
            }
        }
    }

    fn finish(&mut self, mut record: impl FnMut(&[u8])) {
        if self.truncated {
            self.bytes.truncate(MAX_LINE - TRUNCATED.len());
            self.bytes.extend_from_slice(TRUNCATED);
        }
        if !self.bytes.is_empty() {
            record(&self.bytes);
            self.bytes.clear();
        }
        self.truncated = false;
    }
}

#[cfg(test)]
mod tests;
