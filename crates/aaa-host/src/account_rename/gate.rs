//! 중첩된 시작 경로가 read를 다시 잡아도 writer 대기열에 막히지 않는 이전 잠금.
//! writer는 reader 종료 사건을 기다리고, read가 모두 끝난 순간에만 배타권을 얻는다.
use tokio::sync::{Notify, RwLock, RwLockReadGuard, RwLockWriteGuard};

#[derive(Default)]
pub(crate) struct Gate {
    lock: RwLock<()>,
    changed: Notify,
}
pub(crate) struct Read<'a> {
    guard: Option<RwLockReadGuard<'a, ()>>,
    changed: &'a Notify,
}
pub(crate) struct Write<'a> {
    guard: Option<RwLockWriteGuard<'a, ()>>,
    changed: &'a Notify,
}
impl Drop for Read<'_> {
    fn drop(&mut self) {
        self.guard.take();
        self.changed.notify_waiters();
    }
}
impl Drop for Write<'_> {
    fn drop(&mut self) {
        self.guard.take();
        self.changed.notify_waiters();
    }
}
impl Gate {
    pub(crate) async fn read(&self) -> Read<'_> {
        Read { guard: Some(self.lock.read().await), changed: &self.changed }
    }
    pub(crate) async fn write(&self) -> Write<'_> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            // 검사와 대기 등록 사이 마지막 reader가 끝나도 깨움을 놓치지 않는다.
            changed.as_mut().enable();
            if let Ok(guard) = self.lock.try_write() {
                return Write { guard: Some(guard), changed: &self.changed };
            }
            changed.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn writer_waits_for_nested_readers_and_wakes_the_next_writer() {
        let gate = Gate::default();
        let first = gate.read().await;
        let writer = gate.write();
        tokio::pin!(writer);
        assert!(futures_util::poll!(&mut writer).is_pending());
        // 실제 writer가 대기에 든 뒤에도 같은 시작 경로의 중첩 read는 끝나야 한다.
        let nested = tokio::select! {
            _ = &mut writer => panic!("reader가 있는데 writer가 들어왔다"),
            nested = gate.read() => nested,
        };
        drop(first);
        drop(nested);
        let exclusive = writer.await;
        let next = gate.write();
        tokio::pin!(next);
        assert!(futures_util::poll!(&mut next).is_pending());
        drop(exclusive);
        drop(next.await);
    }
}
