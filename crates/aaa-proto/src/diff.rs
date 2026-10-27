//! diff.v1: thread worktree의 바뀐 파일과 patch를 그 머신 agent가 읽기 전용으로 읽는다(plan §8.305).
//! 범위는 t3code 4b1024da `GitVcsDriverCore.ts:2854-3005`의 둘이다: 작업 트리 대 base와의 merge-base(커밋 + 미커밋)와
//! 작업 트리 대 HEAD(미커밋). 둘 다 무시하지 않은 새 파일을 담는다. 턴 단위는 없다(t-0403).
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 무엇과 비교하나
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DiffScope {
    /// thread branch가 base(보통 `origin/main`)에서 갈라진 곳 대비 지금 작업 트리(t3code 「Changes」)
    #[default]
    Branch,
    /// HEAD 대비 지금 작업 트리(t3code 「Uncommitted」)
    Uncommitted,
}

/// 한 파일(이름이 바뀌었으면 옛 이름도). 큰 비교에서 파일 하나의 patch만 읽을 때 쓴다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiffFileRef {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub old_path: Option<String>,
}

/// `AgentRequest::ThreadDiff`와 웹 `thread_diff`가 묻는 것
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct DiffQuery {
    #[serde(default)]
    #[ts(optional, as = "Option<DiffScope>")]
    pub scope: DiffScope,
    /// `--ignore-all-space`(t3code 패널 기본 켬, 카드 숫자는 끔)
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub ignore_whitespace: bool,
    /// patch도 읽는다. 끄면 파일 목록과 숫자만(카드)
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub patch: bool,
    /// 이 파일의 patch만(파일 목록은 비운다)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub file: Option<DiffFileRef>,
    /// 파일 하나의 patch에 바뀌지 않은 줄도 모두 담는다(맥락 펼치기 지연 읽기).
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub full_context: bool,
}

/// git이 보는 파일 상태(`--name-status` 글자). 무시하지 않은 새 파일은 `untracked`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DiffStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
}

/// 바뀐 파일 하나(`--numstat`·`--name-status`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiffFile {
    /// 저장소 맨 위 기준 경로(`/`)
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub old_path: Option<String>,
    pub status: DiffStatus,
    /// 바이너리면 없다(numstat `-`)
    #[ts(type = "number | null")]
    pub additions: Option<u64>,
    #[ts(type = "number | null")]
    pub deletions: Option<u64>,
}

/// 읽은 비교. `patch`는 물었을 때만이다
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct DiffView {
    pub scope: DiffScope,
    /// 비교 기준의 이름: branch 범위는 base ref(`origin/main`), 미커밋은 `HEAD`
    pub base: String,
    /// 비교한 commit(merge-base 또는 HEAD). 첫 커밋 전이면 빈 tree
    pub base_sha: String,
    /// 지금 branch 이름(분리된 HEAD면 `HEAD`)
    pub head: String,
    /// 바뀐 파일(경로 차례). 너무 많으면 없다(`truncated`)
    pub files: Option<Vec<DiffFile>>,
    pub patch: Option<String>,
    /// patch를 상한에서 잘랐거나 파일 목록이 모자라다
    pub truncated: bool,
    /// 새 파일이 상한보다 많아 새 파일은 뺐다
    pub untracked_omitted: bool,
    /// 전체 맥락 요청을 처리했고 patch가 잘리지 않았다. 옛 agent는 생략한다.
    #[serde(default)]
    #[ts(optional, as = "Option<bool>")]
    pub full_context: bool,
}
