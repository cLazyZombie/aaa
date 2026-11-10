//! 시험 부모와 소유 CLI 사이의 제한된 읽기 상태 형식. 입력·경로·handle 값은 보내지 않는다.
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Cause {
    PtyDeadline,
    ControlledFixture,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub format: u8,
    pub cause: Cause,
    pub decided_unix_us: u64,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Prepared,
    Call,
    Returned,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Api<T> {
    Ok { value: T },
    Error { os_code: i32 },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sample {
    pub process_id: u32,
    pub read_id: u64,
    pub read_thread: u32,
    pub phase_before: Phase,
    pub phase_after: Phase,
    pub sampled_unix_us: u64,
    pub query_elapsed_us: u64,
    pub original_stdin_wait: Api<u32>,
    pub reader_io_pending: Api<bool>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Outcome {
    OriginalStdin { sample: Sample },
    NoActiveRead,
    InvalidRequest,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reply {
    pub format: u8,
    pub cause: Cause,
    pub decided_unix_us: u64,
    pub outcome: Outcome,
}
pub(crate) async fn read<T: serde::de::DeserializeOwned>(
    stream: &mut aaa_common::ipc::LocalStream,
) -> std::io::Result<T> {
    let size = stream.read_u32_le().await?;
    if size > 4096 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    let mut data = vec![0; size as usize];
    stream.read_exact(&mut data).await?;
    serde_json::from_slice(&data).map_err(|_| std::io::ErrorKind::InvalidData.into())
}
pub(crate) async fn write<T: Serialize>(stream: &mut aaa_common::ipc::LocalStream, value: &T) -> std::io::Result<()> {
    let data = serde_json::to_vec(value).map_err(|_| std::io::ErrorKind::InvalidData)?;
    if data.len() > 4096 {
        return Err(std::io::ErrorKind::InvalidData.into());
    }
    stream.write_u32_le(data.len() as u32).await?;
    stream.write_all(&data).await?;
    stream.flush().await
}
