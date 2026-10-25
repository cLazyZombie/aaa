//! 요청마다 작업자 네 개만 쓴다. 한 보고서 family의 독립 경로를 함께 옮기며 다음 family는 모두 끝난 뒤 시작한다.
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;

type Operation = dyn Fn(&Path, &Path, bool) -> io::Result<()> + Send + Sync;
type Job = (usize, PathBuf, PathBuf, bool);

pub(super) struct Moves {
    tx: Option<mpsc::SyncSender<Job>>,
    results: mpsc::Receiver<(usize, io::Result<()>)>,
    workers: Vec<JoinHandle<()>>,
}

impl Moves {
    pub(super) fn new() -> io::Result<Self> {
        Self::with_operation(Arc::new(|source, dest, file_only| {
            let meta = match std::fs::symlink_metadata(source) {
                Ok(meta) => meta,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
                Err(error) => return Err(error),
            };
            if file_only && meta.is_dir() {
                return Err(io::Error::other("일반 파일 자리가 디렉터리다"));
            }
            std::fs::rename(source, dest)
        }))
    }

    fn with_operation(operation: Arc<Operation>) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Job>(4);
        let (result_tx, results) = mpsc::channel();
        let rx = Arc::new(Mutex::new(rx));
        let mut moves = Self { tx: Some(tx), results, workers: Vec::new() };
        for _ in 0..4 {
            let (rx, result_tx, operation) = (rx.clone(), result_tx.clone(), operation.clone());
            let worker = std::thread::Builder::new().name("aaa-report-detach".into()).spawn(move || {
                loop {
                    let job = rx.lock().unwrap().recv();
                    let Ok((index, source, dest, file_only)) = job else {
                        break;
                    };
                    let result = operation(&source, &dest, file_only);
                    if result_tx.send((index, result)).is_err() {
                        break;
                    }
                }
            })?;
            moves.workers.push(worker);
        }
        Ok(moves)
    }

    /// 실패해도 그 보고서의 모든 작업이 끝나기 전에는 반환하지 않는다. 오류 순서는 입력 순서다.
    pub(super) fn run(&mut self, paths: &[(PathBuf, PathBuf, bool)]) -> io::Result<Vec<io::Result<()>>> {
        for (index, (source, dest, file_only)) in paths.iter().enumerate() {
            self.tx
                .as_ref()
                .unwrap()
                .send((index, source.clone(), dest.clone(), *file_only))
                .map_err(io::Error::other)?;
        }
        let mut results: Vec<_> = (0..paths.len()).map(|_| None).collect();
        for _ in paths {
            let (index, result) = self.results.recv().map_err(io::Error::other)?;
            results[index] = Some(result);
        }
        Ok(results.into_iter().map(Option::unwrap).collect())
    }
}

impl Drop for Moves {
    fn drop(&mut self) {
        self.tx.take();
        for worker in self.workers.drain(..) {
            if worker.join().is_err() {
                tracing::warn!("보고서 경로 이동 작업자 종료 실패: thread panic");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_paths_overlap_and_failure_waits_for_the_other_path() {
        let (started_tx, started_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let (release0, gate0) = mpsc::channel();
        let (release1, gate1) = mpsc::channel();
        let gates = [Mutex::new(gate0), Mutex::new(gate1)];
        let operation: Arc<Operation> = Arc::new(move |source, _, _| {
            let index = source.to_str().unwrap().parse::<usize>().unwrap();
            let _ = started_tx.send(index);
            gates[index].lock().unwrap().recv().map_err(io::Error::other)?;
            let _ = done_tx.send(index);
            if index == 0 {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "실험실 이동 거부"))
            } else {
                Ok(())
            }
        });
        let (result_tx, result_rx) = mpsc::channel();
        let request = std::thread::spawn(move || {
            let mut moves = Moves::with_operation(operation).unwrap();
            let paths =
                [(PathBuf::from("0"), PathBuf::from("a"), false), (PathBuf::from("1"), PathBuf::from("b"), true)];
            let _ = result_tx.send(moves.run(&paths));
        });
        let deadline = std::time::Duration::from_secs(10);
        let first = started_rx.recv_timeout(deadline).unwrap();
        let second = started_rx.recv_timeout(deadline).unwrap();
        assert_ne!(first, second, "두 경로가 함께 시작한다");
        release0.send(()).unwrap();
        assert_eq!(done_rx.recv_timeout(deadline).unwrap(), 0);
        assert!(
            matches!(result_rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
            "다른 이동이 남으면 오류를 먼저 반환하지 않는다"
        );
        release1.send(()).unwrap();
        let results = result_rx.recv_timeout(deadline).unwrap().unwrap();
        assert_eq!(results[0].as_ref().unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert!(results[0].as_ref().unwrap_err().to_string().contains("실험실 이동 거부"));
        assert!(results[1].is_ok());
        request.join().unwrap();
    }
}
