//! 공개 경로에서 분리한 사본만 전용 작업자가 지운다. 중단된 batch는 기동 때 다시 읽는다.
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;

use aaa_proto::{ErrorBody, ErrorCode};

use crate::{Host, err};

const PENDING: &str = ".library-delete-";

enum Work {
    Remove(PathBuf),
    #[cfg(test)]
    Flush(mpsc::Sender<()>),
}

pub(crate) struct Reaper {
    tx: Option<mpsc::Sender<Work>>,
    worker: Option<JoinHandle<()>>,
}

impl Reaper {
    fn new() -> std::io::Result<Self> {
        Self::with_remove(|path| {
            directory(path.parent().unwrap())?;
            directory(path)?;
            std::fs::remove_dir_all(path)
        })
    }

    pub(crate) fn with_remove(remove: impl Fn(&Path) -> std::io::Result<()> + Send + 'static) -> std::io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::Builder::new().name("aaa-report-delete".into()).spawn(move || {
            while let Ok(work) = rx.recv() {
                match work {
                    Work::Remove(path) => {
                        let started = std::time::Instant::now();
                        match remove(&path) {
                            Ok(()) => tracing::info!(path = %path.display(), elapsed_us = started.elapsed().as_micros() as u64, "보고서 삭제 대기 정리 완료"),
                            Err(error) => tracing::warn!(path = %path.display(), os_error = ?error.raw_os_error(), "보고서 삭제 대기 정리 실패(다음 기동에 회수): {error}"),
                        }
                    }
                    #[cfg(test)]
                    Work::Flush(done) => { let _ = done.send(()); }
                }
            }
        })?;
        Ok(Self { tx: Some(tx), worker: Some(worker) })
    }

    pub(super) fn submit(&self, path: PathBuf) {
        if let Err(error) = self.tx.as_ref().unwrap().send(Work::Remove(path.clone())) {
            tracing::warn!(path = %path.display(), "보고서 삭제 대기 전달 실패(다음 기동에 회수): {error}");
        }
    }

    #[cfg(test)]
    pub(crate) fn flush(&self) {
        let (tx, rx) = mpsc::channel();
        self.tx.as_ref().unwrap().send(Work::Flush(tx)).unwrap();
        rx.recv().unwrap();
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        // Host를 소유하지 않는 작업자다. 마지막 Host 해제 때 자식의 실제 종료를 확인한다.
        self.tx.take();
        if self.worker.take().unwrap().join().is_err() {
            tracing::warn!("보고서 삭제 작업자 종료 실패: thread panic, 남은 대기는 다음 기동에 회수한다");
        }
    }
}

fn queue_error(path: &Path, error: std::io::Error) -> ErrorBody {
    err(
        ErrorCode::Unavailable,
        format!("보고서 삭제 대기 {} 실패: {error} (OS {:?})", path.display(), error.raw_os_error()),
    )
}

/// 내부 대기 폴더를 symlink/junction으로 바꾸어 외부 경로에 쓰거나 회수하지 않는다.
fn directory(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    let safe = meta.is_dir() && !meta.file_type().is_symlink();
    if safe { Ok(()) } else { Err(std::io::Error::other("삭제 대기 폴더가 일반 디렉터리가 아니다")) }
}

impl Host {
    pub(super) fn report_delete_reaper(&self) -> Result<&Reaper, ErrorBody> {
        if self.library_reaper.get().is_none() {
            let worker = Reaper::new()
                .map_err(|error| err(ErrorCode::Unavailable, format!("보고서 삭제 작업자 시작 실패: {error}")))?;
            // 요청은 library_ops 안에서 온다. 기동 회수와 동시에 들어와도 중복 작업자는 종료된다.
            let _ = self.library_reaper.set(worker);
        }
        Ok(self.library_reaper.get().unwrap())
    }

    pub(crate) fn report_delete_root(&self, project: &Path) -> PathBuf {
        // 같은 프로젝트 폴더를 읽는 다른 state/시험 host의 대기는 건드리지 않는다.
        project.join(format!("{PENDING}{}", hex::encode(self.host_id.as_bytes())))
    }

    pub(super) fn report_delete_batch(&self, project: &Path) -> Result<PathBuf, ErrorBody> {
        let root = self.report_delete_root(project);
        match std::fs::create_dir(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(queue_error(&root, error)),
        }
        directory(&root).map_err(|error| queue_error(&root, error))?;
        let batch = root.join(hex::encode(rand::random::<[u8; 16]>()));
        std::fs::create_dir(&batch).map_err(|error| queue_error(&batch, error))?;
        // 함께 rename하는 경로가 같은 목적지 디렉터리 잠금을 다투지 않게 나눈다.
        for slot in 0..4 {
            let dest = batch.join(slot.to_string());
            if let Err(error) = std::fs::create_dir(&dest) {
                self.report_delete_reaper()?.submit(batch);
                return Err(queue_error(&dest, error));
            }
        }
        Ok(batch)
    }

    pub(crate) fn library_delete_recover(&self) {
        for (_, project) in self.project_directories() {
            let root = self.report_delete_root(&project);
            if let Err(error) = self.recover_report_batches(&root) {
                tracing::warn!("보고서 삭제 대기 기동 회수 실패: {}", error.message);
            }
        }
    }

    fn recover_report_batches(&self, root: &Path) -> Result<(), ErrorBody> {
        match directory(root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(queue_error(root, error)),
        }
        for entry in std::fs::read_dir(root).map_err(|error| queue_error(root, error))? {
            let entry = entry.map_err(|error| queue_error(root, error))?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // 불명한 파일·폴더는 소유했다고 추측하지 않는다.
            if name.len() != 32 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
                continue;
            }
            let path = entry.path();
            if let Err(error) = directory(&path) {
                tracing::warn!(path = %path.display(), "보고서 삭제 대기의 불명한 항목을 남긴다: {error}");
                continue;
            }
            self.report_delete_reaper()?.submit(path);
        }
        Ok(())
    }
}
