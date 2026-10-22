//! imagegen.v1: thread가 자기 이미지 작업만 요청한다. 인증·실행 경로는 싣지 않는다.
use serde::{Deserialize, Serialize};

pub const IMAGE_CHUNK: usize = 64 * 1024;
pub const IMAGE_MAX_BYTES: u64 = 50 * 1024 * 1024;
pub const IMAGE_MAX_COUNT: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageFile {
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageBegin {
    pub id: String,
    pub prompt: String,
    pub profile: String,
    pub account: Option<String>,
    pub transparent: bool,
    pub inputs: Vec<ImageFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ImageRequest {
    List,
    Begin(ImageBegin),
    Input { id: String, index: u32, offset: u64, data: crate::B64 },
    Start { id: String },
    Forget { id: String },
    Status { id: String, wait: bool },
    Read { id: String, index: u32, offset: u64 },
}
impl ImageRequest {
    pub fn id(&self) -> Option<&str> {
        Some(match self {
            Self::List => return None,
            Self::Begin(b) => &b.id,
            Self::Input { id, .. }
            | Self::Start { id }
            | Self::Forget { id }
            | Self::Status { id, .. }
            | Self::Read { id, .. } => id,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageState {
    Draft,
    Queued,
    Running,
    Waiting,
    Succeeded,
    Failed,
    Unknown,
    Forgotten,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageView {
    pub id: String,
    pub state: ImageState,
    pub account: Option<String>,
    pub reason: String,
    pub outputs: Vec<ImageFile>,
    /// Codex가 실제로 알린 토큰 합계. 이미지 과금액·남은 %를 추정하지 않는다.
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ImageResponse {
    Jobs { jobs: Vec<ImageView> },
    Job(ImageView),
    Chunk { data: crate::B64, eof: bool },
}
