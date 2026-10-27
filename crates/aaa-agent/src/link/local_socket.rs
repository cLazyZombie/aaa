//! macOS 로컬 host socket의 실제 교체를 재접속 대기에 알린다.
//! 부모 디렉터리를 보되 무관한 파일·옛 사건으로는 backoff를 깨우지 않는다.

use anyhow::Context;
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use tokio::io::{Interest, unix::AsyncFd};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Stamp(u64, u64, std::time::SystemTime);

pub(super) struct Watch {
    path: PathBuf,
    queue: AsyncFd<File>,
    // vnode 감시는 열린 디렉터리의 수명에 묶인다.
    _parent: File,
}

impl Watch {
    pub(super) fn new(path: &Path) -> anyhow::Result<Self> {
        let parent = File::open(path.parent().context("socket 부모 경로가 없다")?).context("socket 부모 열기")?;
        // kqueue에는 원자적 CLOEXEC 생성 옵션이 없다. 이 저장소의 spawn 잠금으로
        // 생성과 FD_CLOEXEC 사이에 자식이 fd를 상속하는 것을 막는다.
        let queue = aaa_common::child_process::with_spawn_lock(|| {
            // SAFETY: 새 fd만 소유권으로 받고 모든 실패에서 File이 닫는다.
            let raw = unsafe { libc::kqueue() };
            if raw < 0 {
                return Err(io::Error::last_os_error());
            }
            let queue = unsafe { File::from_raw_fd(raw) };
            // SAFETY: 열린 queue fd의 close-on-exec만 설정한다.
            if unsafe { libc::fcntl(raw, libc::F_SETFD, libc::FD_CLOEXEC) } < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(queue)
        })
        .context("kqueue 생성·CLOEXEC 설정")?;
        let change = libc::kevent {
            ident: parent.as_raw_fd() as libc::uintptr_t,
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_CLEAR,
            fflags: libc::NOTE_WRITE | libc::NOTE_RENAME | libc::NOTE_DELETE | libc::NOTE_REVOKE,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: change는 호출 동안 유효하며 결과 버퍼는 요청하지 않는다.
        if unsafe { libc::kevent(queue.as_raw_fd(), &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) } < 0 {
            return Err(io::Error::last_os_error()).context("socket 부모 vnode 등록");
        }
        // kqueue fd는 읽기 감시만 지원한다. File은 이 비공개 AsyncFd가 독점하고
        // drop까지 같은 열린 fd를 유지한다.
        // SAFETY: 위 소유권 때문에 reactor에 등록한 fd의 수명이 고정된다.
        let queue = unsafe { AsyncFd::register_with_interest(queue, Interest::READABLE) }
            .context("kqueue reactor READABLE 등록")?;
        Ok(Self { path: path.into(), queue, _parent: parent })
    }

    pub(super) fn stamp(&self) -> Option<Stamp> {
        match std::fs::metadata(&self.path) {
            // birth time은 inode 재사용을 구별하고 chmod를 새 listener로 오인하지 않는다.
            Ok(m) if m.file_type().is_socket() => m
                .created()
                .inspect_err(|e| tracing::debug!(path = %self.path.display(), error = %e, "socket 생성 시각 확인 실패"))
                .ok()
                .map(|at| Stamp(m.dev(), m.ino(), at)),
            Ok(_) => None,
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    tracing::debug!(path = %self.path.display(), error = %e, "로컬 socket 교체 확인 실패");
                }
                None
            }
        }
    }

    async fn changed(&self) -> io::Result<()> {
        loop {
            let mut ready = self.queue.readable().await?;
            let result = ready.try_io(|queue| {
                let mut event = std::mem::MaybeUninit::<libc::kevent>::uninit();
                let now = libc::timespec { tv_sec: 0, tv_nsec: 0 };
                // SAFETY: event 한 개의 버퍼가 있고 대기는 0이므로 runtime을 막지 않는다.
                let count =
                    unsafe { libc::kevent(queue.as_raw_fd(), std::ptr::null(), 0, event.as_mut_ptr(), 1, &now) };
                if count < 0 {
                    return Err(io::Error::last_os_error());
                }
                if count == 0 {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                // SAFETY: kevent가 event 한 개를 썼다.
                let event = unsafe { event.assume_init() };
                if event.flags & libc::EV_ERROR != 0 {
                    return Err(io::Error::from_raw_os_error(event.data as i32));
                }
                if event.fflags & (libc::NOTE_RENAME | libc::NOTE_DELETE | libc::NOTE_REVOKE) != 0 {
                    return Err(io::Error::other("감시하던 socket 부모가 바뀌었다"));
                }
                Ok(())
            });
            match result {
                Ok(Err(e)) if e.kind() == io::ErrorKind::Interrupted => continue,
                Ok(result) => return result,
                Err(_) => {}
            }
        }
    }

    pub(super) async fn replaced(&self, before: Option<Stamp>) -> io::Result<()> {
        loop {
            // connect와 대기 등록 사이의 생성도 놓치지 않는다. 파일이 없어지거나
            // 같은 socket이 거부한 경우에는 기존 실패 backoff를 그대로 기다린다.
            if self.stamp().is_some_and(|now| Some(now) != before) {
                return Ok(());
            }
            self.changed().await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::future::Future;

    async fn pending(future: std::pin::Pin<&mut impl Future<Output = io::Result<()>>>) -> bool {
        let mut future = future;
        std::future::poll_fn(|cx| std::task::Poll::Ready(future.as_mut().poll(cx).is_pending())).await
    }

    /// 실제 vnode 알림과 UDS 교체를 사용한다. 무관한 파일·삭제·일반 파일은
    /// 재접속 계기가 아니며 새 listener에서만 대기를 끝내야 한다.
    #[tokio::test]
    async fn socket_replacement_wakes_but_other_directory_changes_do_not() {
        let root = std::env::temp_dir().join(format!("aals-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("host.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let watch = Watch::new(&path).unwrap();
        let before = watch.stamp();
        assert!(before.is_some());
        let mut waiting = Box::pin(watch.replaced(before));
        assert!(pending(waiting.as_mut()).await);
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o400)).unwrap();
        assert!(watch.stamp() == before, "권한 변경을 새 listener로 셌다");
        std::fs::write(root.join("unrelated"), "changed").unwrap();
        drop(tokio::time::timeout(std::time::Duration::from_secs(3), watch.queue.readable()).await.unwrap().unwrap());
        assert!(pending(waiting.as_mut()).await, "무관한 vnode 사건이 대기를 끝냈다");
        assert!(watch.stamp() == before, "같은 socket은 다시 시도하지 않는다");
        drop(listener);
        std::fs::remove_file(&path).unwrap();
        std::fs::write(&path, "not a socket").unwrap();
        drop(tokio::time::timeout(std::time::Duration::from_secs(3), watch.queue.readable()).await.unwrap().unwrap());
        assert!(pending(waiting.as_mut()).await, "일반 파일이 대기를 끝냈다");
        assert!(watch.stamp().is_none(), "일반 파일을 listener로 셌다");
        std::fs::remove_file(&path).unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(3), waiting).await.unwrap().unwrap();
        assert!(watch.stamp().is_some_and(|now| Some(now) != before));
        drop(watch);
        drop(listener);
        std::fs::remove_dir_all(root).unwrap();
    }
}
