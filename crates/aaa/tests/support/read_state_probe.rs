//! 사용자 전용 Named Pipe와 OS peer PID로 소유한 CLI의 한 표본만 받는다.
use super::read_state_product::wire;
use std::path::PathBuf;
use std::time::Instant;
pub(super) use wire::{Api, Cause, Outcome, Phase};
use wire::{Reply, Request};

pub(super) struct Probe {
    runtime: tokio::runtime::Runtime,
    listener: aaa_common::ipc::LocalListener,
    root: PathBuf,
}
pub(super) fn unix_us() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros() as u64
}
impl Probe {
    pub(super) fn new() -> Self {
        let root = std::env::temp_dir().join(format!("aaa-read-state-{}", aaa_common::util::new_id()));
        aaa_common::util::ensure_private_dir(&root).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let listener = {
            let _entered = runtime.enter();
            aaa_common::ipc::LocalListener::bind(&root.join("state.sock")).unwrap()
        };
        Self { runtime, listener, root }
    }
    pub(super) fn configure(&self, command: &mut portable_pty::CommandBuilder) {
        command.env("AAA_TEST_INSTALL_READ_STATE_SOCKET", self.root.join("state.sock"));
        command.env("AAA_TEST_INSTALL_READ_STATE_OWNER", std::process::id().to_string());
    }
    pub(super) fn capture(
        &self,
        pid: u32,
        cause: Cause,
        decided_unix_us: u64,
        deadline: Instant,
    ) -> Result<Reply, String> {
        self.runtime.block_on(async {
            let collect = async {
                let (mut stream, peer) = self.listener.accept_peer().await.map_err(io_error)?;
                if peer != Some(pid) {
                    return Err("read_state_peer_mismatch".into());
                }
                let request = Request { format: 1, cause, decided_unix_us };
                wire::write(&mut stream, &request).await.map_err(io_error)?;
                let reply: Reply = wire::read(&mut stream).await.map_err(io_error)?;
                if reply.format != 1 || reply.cause != cause || reply.decided_unix_us != decided_unix_us {
                    return Err("read_state_reply_mismatch".into());
                }
                if let Outcome::OriginalStdin { sample } = &reply.outcome
                    && (sample.process_id != pid
                        || sample.read_id == 0
                        || sample.read_thread == 0
                        || matches!(sample.original_stdin_wait, Api::Ok { value } if !matches!(value, 0 | 258)))
                {
                    return Err("read_state_sample_fields".into());
                }
                Ok(reply)
            };
            tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), collect)
                .await
                .map_err(|_| "read_state_collection_timeout".to_owned())?
        })
    }
}
fn io_error(error: std::io::Error) -> String {
    format!("read_state_io: kind={:?}, os_code={:?}", error.kind(), error.raw_os_error())
}
impl Drop for Probe {
    fn drop(&mut self) {
        // Windows pipe는 listener 소유 handle의 drop으로 닫힌다. 이 경로에는 자기 빈 디렉터리만 있다.
        let _ = std::fs::remove_dir(&self.root);
    }
}
