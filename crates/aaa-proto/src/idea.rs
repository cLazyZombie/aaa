//! 아이디어(t-1260, FRONT_API 1.147): 대화로 여러 번 다듬는 노트와 그 전용 thread. host 프로젝트 `ideas/<ULID>/`에 노트(`note.md`)·
//! 판(`revs/<판>.md`)·그림을 두고, 짧은 ID `n-NNNN`으로 부른다. 노트는 판을 주고 저장한다: 기준 판이 지금 판과 다르면 쓰지 않는다.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 다듬는 중, 또는 할 일로 넘겨 완료(보관·잠금)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum IdeaStatus {
    #[default]
    Open,
    Done,
}

impl IdeaStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            IdeaStatus::Open => "다듬는 중",
            IdeaStatus::Done => "완료",
        }
    }
}

/// 판을 남긴 쪽
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum IdeaAuthor {
    /// 사람이 직접 고쳤다(웹·세션 밖 CLI)
    #[default]
    User,
    /// 전용 thread가 고쳤다
    Thread,
}

/// 아이디어 목록 한 줄
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct IdeaSummary {
    /// 폴더 이름(ULID)
    pub id: String,
    /// `n-0001`
    pub short_id: String,
    pub title: String,
    pub status: IdeaStatus,
    /// 지금 판(1부터)
    pub rev: u32,
    /// 아직 사람이 열지 않은 thread 저장 판이 있다(host에 보존).
    pub unseen: bool,
    #[ts(type = "number")]
    pub created_ms: u64,
    #[ts(type = "number")]
    pub updated_ms: u64,
    /// 넘긴 할 일(`d-NNNN`)
    pub tasks: Vec<String>,
    /// 전용 thread(`t-NNNN`). 아직 말하지 않았으면 없다
    pub thread: Option<String>,
    /// 복사해 온 아이디어(`n-NNNN`)
    pub copied_from: Option<String>,
    /// 진행 모델: 다음 말을 맡을 profile(고른 것, 없으면 프로젝트 기본 thread profile). FRONT_API 1.163, 옛 host는 없다
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub profile: Option<String>,
}

/// 남은 판 하나
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct IdeaRevView {
    pub rev: u32,
    #[ts(type = "number")]
    pub at_ms: u64,
    pub by: IdeaAuthor,
    /// 바뀐 곳 요약(사람이 고친 판은 host가 절마다 센다, thread 판은 thread가 준 한 줄)
    pub summary: Option<String>,
}

/// 노트 하나(지금 판 또는 고른 판)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct IdeaNote {
    /// 기존 미디어 주소를 옮기지 못한 까닭. 글은 계속 읽을 수 있다.
    pub media_error: Option<String>,
    /// 프로젝트(그림 주소 `/api/v1/projects/<p>/ideas/<id>/files/<이름>`)
    pub project: String,
    pub idea: IdeaSummary,
    /// 이 글의 판(옛 판을 읽으면 `idea.rev`보다 작다)
    pub rev: u32,
    pub text: String,
    /// 최근 판부터
    pub history: Vec<IdeaRevView>,
    /// 사람이 고친 것을 아직 thread에 알리지 않았다(다음 말 앞에 「(노트 고침: …)」이 붙는다)
    pub edited: Option<String>,
    /// 사람이 고친 것과 thread 판이 겹쳐 thread가 합치는 중이다(그 사람 판의 기준 판)
    pub merging: Option<u32>,
    /// 전용 thread가 준비되기를 기다리는 말(보낸 차례). 준비되면 host가 보내고 지운다
    pub pending: Vec<String>,
    /// 전용 thread를 띄우지 못한 까닭(말은 남는다. 다음 말이나 화면 열기 때 다시 띄운다)
    pub error: Option<String>,
    /// 이 글이 가리키는 그림·영상(처음 나온 차례, FRONT_API 1.162). 옛 host는 비운다
    pub media: Vec<IdeaMediaView>,
    /// 아이디어 폴더의 그림·영상 전체 크기(옛 판이 가리키는 것 포함, 1.162)
    #[ts(type = "number")]
    pub media_bytes: u64,
}

/// 그림·영상을 들인 길(FRONT_API 1.162)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum IdeaMediaSource {
    /// 출처 기록이 없다(1.162 전에 들인 것)
    #[default]
    Unknown,
    /// 웹에서 올림(붙여 넣기·끌어 놓기·파일 고르기)
    Web,
    /// `aaa idea attach`로 host 파일을 붙임
    File,
    /// 전용 thread의 `library/`에서 옮김
    Library,
    /// 프로젝트 `uploads/`(대화·할 일 첨부)에서 옮김
    Uploads,
    /// 다른 아이디어를 복사할 때 함께 옴
    Copy,
}

/// 노트의 그림·영상 하나와 그 출처(FRONT_API 1.162)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct IdeaMediaView {
    /// 폴더 안 이름(`<16 hex>.<ext>`)
    pub name: String,
    #[ts(type = "number")]
    pub bytes: u64,
    pub source: IdeaMediaSource,
    /// 들인 쪽: `사람`·`coordinator`·`t-NNNN`. 기록이 없으면 빈 글
    pub by: String,
    /// 원래 파일 이름이나 경로(있을 때)
    pub file: Option<String>,
    #[ts(type = "number")]
    pub at_ms: u64,
}

/// 할 일로 넘길 초안 하나
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct IdeaTaskDraft {
    pub title: String,
    /// 저장소 칸(할 일 편집과 같은 값). 비우면 정하지 않음
    pub repo: Option<String>,
    /// 메모(여러 줄 Markdown)
    pub notes: String,
}

/// 노트를 저장하지 못했다: 기준 판 뒤에 다른 판이 있다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct IdeaConflict {
    /// 저장하려던 글의 기준 판과 그 글
    pub base_rev: u32,
    pub base_text: String,
    /// 지금 판
    pub current: IdeaNote,
}
