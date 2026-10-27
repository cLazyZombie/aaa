//! report.md의 제자리 쓰기와 원자적 교체를 관측한다. 사건은 hash를 다시 읽을 계기일 뿐이다.
//! 감시 오류·놓친 사건에는 기존 1초 점검이 남으며 library 내부는 감시하지 않는다.

use anyhow::Context;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{Interest, unix::AsyncFd};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    identity: (u64, u64),
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
    regular: bool,
}

struct Watch {
    path: PathBuf,
    queue: AsyncFd<File>,
    parent: File,
    file: Option<((u64, u64), File)>,
}

impl Watch {
    fn new(dir: &Path) -> anyhow::Result<Self> {
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_EVTONLY | libc::O_NOFOLLOW | libc::O_DIRECTORY)
            .open(dir)
            .context("보고 부모 열기")?;
        let queue = aaa_common::child_process::with_spawn_lock(|| {
            // SAFETY: 생성한 fd만 File로 소유하며 모든 실패와 drop에서 닫는다.
            let raw = unsafe { libc::kqueue() };
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            let queue = unsafe { File::from_raw_fd(raw) };
            // SAFETY: spawn 잠금 안에서 열린 새 fd의 CLOEXEC만 설정한다.
            if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(queue)
        })
        .context("보고 kqueue 생성·CLOEXEC 설정")?;
        // SAFETY: 이 AsyncFd가 queue를 독점하며 등록 뒤 열린 fd는 바뀌지 않는다.
        let queue = unsafe { AsyncFd::register_with_interest(queue, Interest::READABLE) }
            .context("보고 kqueue READABLE 등록")?;
        let watch = Self { path: dir.join("report.md"), queue, parent, file: None };
        watch.add(&watch.parent).context("보고 부모 vnode 등록")?;
        Ok(watch)
    }

    fn add(&self, file: &File) -> io::Result<()> {
        let change = libc::kevent {
            ident: file.as_raw_fd() as libc::uintptr_t,
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: libc::NOTE_WRITE
                | libc::NOTE_EXTEND
                | libc::NOTE_ATTRIB
                | libc::NOTE_RENAME
                | libc::NOTE_DELETE
                | libc::NOTE_REVOKE,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: 유효한 change 한 개를 넘기며 결과 버퍼와 대기는 없다.
        if unsafe {
            libc::kevent(self.queue.get_ref().as_raw_fd(), &change, 1, std::ptr::null_mut(), 0, std::ptr::null())
        } < 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn stamp(&self) -> io::Result<Option<Stamp>> {
        match std::fs::symlink_metadata(&self.path) {
            Ok(m) => Ok(Some(Stamp {
                identity: (m.dev(), m.ino()),
                size: m.len(),
                modified: (m.mtime(), m.mtime_nsec()),
                changed: (m.ctime(), m.ctime_nsec()),
                regular: m.file_type().is_file(),
            })),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn prepare(&mut self) -> anyhow::Result<Option<Stamp>> {
        let stamp = self.stamp().context("보고 metadata 확인")?;
        let identity = stamp.filter(|s| s.regular).map(|s| s.identity);
        if identity != self.file.as_ref().map(|(id, _)| *id) {
            // 닫으면 옛 vnode 등록도 사라진다. symlink와 일반 파일 아닌 경로는 열지 않는다.
            self.file = None;
            if identity.is_some() {
                let file = OpenOptions::new()
                    .read(true)
                    .custom_flags(libc::O_EVTONLY | libc::O_NOFOLLOW)
                    .open(&self.path)
                    .context("보고 vnode 열기")?;
                let m = file.metadata().context("열린 보고 identity 확인")?;
                self.add(&file).context("보고 파일 vnode 등록")?;
                self.file = Some(((m.dev(), m.ino()), file));
            }
        }
        Ok(stamp)
    }

    async fn event(&self) -> io::Result<()> {
        loop {
            let mut ready = self.queue.readable().await?;
            match ready.try_io(|queue| {
                let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
                let now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
                // SAFETY: event 한 개를 받을 버퍼가 있고 대기는 0이다.
                let n = unsafe { libc::kevent(queue.as_raw_fd(), std::ptr::null(), 0, event.as_mut_ptr(), 1, &now) };
                if n < 0 {
                    return Err(io::Error::last_os_error());
                }
                if n == 0 {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                // SAFETY: kevent가 결과 한 개를 썼다.
                let event = unsafe { event.assume_init() };
                if event.flags & libc::EV_ERROR != 0 {
                    return Err(io::Error::from_raw_os_error(event.data as i32));
                }
                if event.ident == self.parent.as_raw_fd() as libc::uintptr_t
                    && event.fflags & (libc::NOTE_RENAME | libc::NOTE_DELETE | libc::NOTE_REVOKE) != 0
                {
                    return Err(io::Error::other("감시하던 보고 부모가 바뀌었다"));
                }
                Ok(())
            }) {
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(result) => return result,
                Err(_) => {}
            }
        }
    }

    async fn changed(&self, before: Option<Stamp>) -> io::Result<()> {
        loop {
            // 등록·hash 읽기 사이의 생성/교체도 놓치지 않는다.
            if self.stamp()? != before {
                return Ok(());
            }
            self.event().await?;
        }
    }
}

pub(super) struct Wake {
    watch: Option<Watch>,
    before: Option<Stamp>,
}

impl Wake {
    pub(super) fn new(dir: &Path) -> Self {
        let watch = Watch::new(dir)
            .inspect_err(|e| {
                tracing::warn!(path = %dir.join("report.md").display(), error = %format!("{e:#}"), "보고 vnode 준비 실패, 기존 1초 점검 유지");
            })
            .ok();
        Self { watch, before: None }
    }

    pub(super) fn prepare(&mut self) {
        if let Some(watch) = &mut self.watch {
            match watch.prepare() {
                Ok(stamp) => self.before = stamp,
                Err(e) => {
                    tracing::warn!(path = %watch.path.display(), error = %format!("{e:#}"), "보고 vnode 갱신 실패, 기존 1초 점검 유지");
                    self.watch = None;
                }
            }
        }
    }

    pub(super) async fn wait(&mut self) {
        let tick = tokio::time::sleep(Duration::from_secs(1));
        if let Some(watch) = &self.watch {
            let result = tokio::select! {
                _ = tick => Ok(()),
                result = watch.changed(self.before) => result,
            };
            if let Err(e) = result {
                tracing::warn!(path = %watch.path.display(), error = %e, "보고 vnode 대기 실패, 기존 1초 점검 유지");
                self.watch = None;
            }
        } else {
            tick.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;
    use std::future::Future;

    async fn pending(mut f: std::pin::Pin<&mut impl Future>) -> bool {
        std::future::poll_fn(|cx| std::task::Poll::Ready(f.as_mut().poll(cx).is_pending())).await
    }

    /// 실제 vnode로 생성·제자리 쓰기·rename·삭제를 보며, library와 symlink는 보고 본문으로 읽지 않는다.
    #[tokio::test]
    async fn report_writes_and_replacements_wake_without_following_library_or_symlinks() {
        let root = std::env::temp_dir().join(format!("aarw-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        let report = root.join("report.md");
        let mut watch = Watch::new(&root).unwrap();
        let before = watch.prepare().unwrap();
        assert!(before.is_none());
        let mut waiting = Box::pin(watch.changed(before));
        assert!(pending(waiting.as_mut()).await);
        std::fs::create_dir(root.join("library")).unwrap();
        std::fs::write(root.join("library/evidence"), "unrelated").unwrap();
        drop(tokio::time::timeout(Duration::from_secs(3), watch.queue.readable()).await.unwrap().unwrap());
        assert!(pending(waiting.as_mut()).await, "library 사건이 보고 변경이 됐다");
        std::fs::write(&report, "one").unwrap();
        tokio::time::timeout(Duration::from_secs(3), waiting).await.unwrap().unwrap();

        for (label, text) in [("in-place", "two"), ("atomic", "three")] {
            let before = watch.prepare().unwrap();
            let mut waiting = Box::pin(watch.changed(before));
            assert!(pending(waiting.as_mut()).await);
            if label == "atomic" {
                std::fs::write(root.join("replacement"), text).unwrap();
                std::fs::rename(root.join("replacement"), &report).unwrap();
            } else {
                std::fs::write(&report, text).unwrap();
            }
            tokio::time::timeout(Duration::from_secs(3), waiting).await.unwrap().unwrap();
            assert_eq!(super::super::threads::report_hash(&root), Some(hex::encode(sha2::Sha256::digest(text))));
        }
        let before = watch.prepare().unwrap();
        let mut waiting = Box::pin(watch.changed(before));
        assert!(pending(waiting.as_mut()).await);
        std::fs::remove_file(&report).unwrap();
        tokio::time::timeout(Duration::from_secs(3), waiting).await.unwrap().unwrap();
        assert!(watch.prepare().unwrap().is_none());
        std::os::unix::fs::symlink(root.join("library/evidence"), &report).unwrap();
        watch.prepare().unwrap();
        assert!(watch.file.is_none());
        assert!(super::super::threads::report_hash(&root).is_none());
        drop(watch);
        std::fs::remove_dir_all(root).unwrap();
    }

    /// 무관한 vnode 사건이 있어도 원래 1초 점검을 미루거나 보고 변경으로 끝내지 않는다.
    #[tokio::test(start_paused = true)]
    async fn unrelated_vnode_events_preserve_the_one_second_backstop() {
        let root = std::env::temp_dir().join(format!("aarw-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("report.md"), "unchanged").unwrap();
        let mut wake = Wake::new(&root);
        wake.prepare();
        assert!(wake.watch.is_some());
        let start = tokio::time::Instant::now();
        let mut waiting = Box::pin(wake.wait());
        assert!(pending(waiting.as_mut()).await);
        std::fs::write(root.join("unrelated"), "changed").unwrap();
        tokio::time::advance(Duration::from_millis(999)).await;
        assert!(pending(waiting.as_mut()).await, "무관한 사건이 1초 점검 전에 끝냈다");
        tokio::time::advance(Duration::from_millis(1)).await;
        waiting.await;
        assert_eq!(tokio::time::Instant::now() - start, Duration::from_secs(1));
        drop(wake);
        std::fs::remove_dir_all(root).unwrap();
    }
}
