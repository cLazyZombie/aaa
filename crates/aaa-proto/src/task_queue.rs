//! 예약의 실행 승인판과 완료 보관. task 내용의 정본은 Markdown이다.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QueueState {
    Queued,
    Check,
    Starting,
    Running,
    Finished,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum QueueOp {
    List,
    /// 같은 예약 갱신도 승인한 최신 블록을 명시한다. 모델 기본값도 확인창에서 해석해 보낸다.
    Reserve {
        task: String,
        block: String,
        profile: String,
        profile_revision: String,
    },
    Cancel {
        task_id: String,
    },
    /// 0이면 맨 앞, 그 밖은 1부터의 예약 순번.
    Move {
        task_id: String,
        position: u32,
    },
}

/// 예약 내용 승인과 별도로 마지막으로 신뢰한 선택 변경의 출처를 보존한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskChoicePin {
    pub owner: Option<String>,
    pub profile: String,
    pub machine: Option<String>,
    pub actor: String,
    #[ts(type = "number")]
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskReservation {
    pub selection: TaskChoicePin,
    pub task_id: String,
    pub task: crate::TaskView,
    pub approved_block: String,
    pub profile: String,
    pub model: String,
    pub effort: String,
    pub profile_revision: String,
    pub machine: Option<String>,
    pub position: u32,
    pub state: QueueState,
    pub reason: String,
    /// 선점과 함께 발급한 기동 ID. 외부 기동의 exactly-once 보증은 아니다.
    pub launch_id: Option<String>,
    pub thread: Option<String>,
    pub actor: String,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ArchivedTask {
    pub task_id: String,
    pub task: crate::TaskView,
    /// 복원할 블록 원문. 첨부 참조는 프로젝트의 같은 경로다.
    pub body: String,
    pub reason: String,
    pub report: Option<String>,
    #[ts(type = "number")]
    pub completed_ms: u64,
    pub restored: bool,
    /// 본문 제거 뒤의 파일 판과 대조하는 복구 영수증.
    pub committed: bool,
    pub removed_sha256: String,
    pub history: Vec<crate::TaskLogEntry>,
}
