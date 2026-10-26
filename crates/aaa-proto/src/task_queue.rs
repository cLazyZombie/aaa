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
        /// 앞 할 일(ID·제목). 그 할 일이 완료 보관될 때까지 이 예약은 시작하지 않는다(FRONT_API 1.113). 생략하면 바로 자리 대기
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        after: Option<String>,
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
    /// 앞 할 일의 ULID(FRONT_API 1.113). 완료 보관되기 전에는 자리 대기에 들지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after: Option<String>,
    /// 앞 할 일의 표시 ID. 저장값 대신 host가 조회 때 채운다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after_short_id: Option<String>,
    /// 연결된 thread의 현재 작업 단계. 예약 저장값 대신 host가 조회 때 채운다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stage: Option<crate::ThreadStage>,
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
    /// 내구 보관의 전체 이력. 목록 응답에서는 비워 중복 스냅샷이 frame을 채우지 않게 한다.
    pub history: Vec<crate::TaskLogEntry>,
    /// 처음 thread를 이은 시각(FRONT_API 1.126). 저장값이 아니라 host가 목록 응답 때 이력으로 채운다. thread 없이 끝난 일은 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number")]
    pub started_ms: Option<u64>,
}

/// 기록 검색은 활성 할 일과 같은 ID·제목·메모·담당 범위다. repo의 빈 글은 정하지 않음이다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskHistorySearch {
    pub query: String,
    pub repo: Option<String>,
}

/// 실제 보관 근거와 복원 영수증으로 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskArchiveFilter {
    PrMerged,
    MainMerged,
    ReportDone,
    Restored,
}

/// 완료 목록의 정렬(FRONT_API 1.126). 모두 내림차순이며 같은 값은 큰 번호가 위다. 시작 기록이 없는 일은 시작 날짜순의 끝이다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskArchiveSort {
    #[default]
    Completed,
    Started,
    Number,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskHistoryRepo {
    pub key: String,
    pub label: String,
    pub n: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskArchiveQuery {
    pub search: Option<TaskHistorySearch>,
    pub filter: Option<TaskArchiveFilter>,
    /// 완료 목록의 정확한 모델·머신 값. 빈 글은 미지정이다.
    pub profile: Option<String>,
    pub machine: Option<String>,
    /// 완료 시각 범위: 시작 포함, 끝 제외(밀리초).
    #[ts(type = "number | null")]
    pub since_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub until_ms: Option<u64>,
    /// 생략하면 끝난 날짜순이다. 옛 host는 모르는 칸을 버리고 보관 ID 순서로 준다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sort: Option<TaskArchiveSort>,
    /// 앞 페이지 응답의 `next` 그대로. 정렬 값을 담은 host 커서라 정렬을 바꾸면 처음부터 다시 읽는다.
    pub after: Option<String>,
    pub limit: Option<u32>,
}
