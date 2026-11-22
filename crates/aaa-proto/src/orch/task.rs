//! 할 일 보기·편집·기록과 시작 미리보기 타입.

use super::*;

/// TASKS.md의 task 한 줄. 내용은 Markdown, 표시 번호는 host DB가 정본이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskView {
    /// 프로젝트별 영구 표시 ID(`d-0001`). 내부 연결은 기존 ULID를 유지한다.
    #[serde(default)]
    pub short_id: Option<String>,
    /// 제목과 독립된 내부 ULID. 기존 task는 host의 식별 이주 때 주석으로 부여한다.
    #[serde(default)]
    pub id: Option<String>,
    /// 속한 `## ` 목록 이름
    pub list: String,
    pub title: String,
    /// 괄호 안 owner 원문(`me`, `claude`, `codex@mac-b`, 사람 이름 등)
    pub owner: Option<String>,
    /// agent에게 맡긴 task의 모델(profile) 칸(FRONT_API 1.75, plan §8.335): owner `모델@머신`의 앞. 맨 이름은 host가 아는 profile일
    /// 때만 모델이다(아니면 사람). 비면 시작할 때 고른다. 옛 host는 생략한다
    #[serde(default)]
    pub profile: Option<String>,
    /// agent에게 맡긴 task의 머신 칸(FRONT_API 1.75): owner `모델@머신`의 뒤. 비면 그 저장소가 있는 머신 중 덜 붐비는 곳이다
    #[serde(default)]
    pub machine: Option<String>,
    /// owner 뒤 대괄호의 저장소 칸(FRONT_API 1.73, plan §8.330): `project.toml` repos 경로의 마지막 이름이나 `작업 폴더`. 비면 정하지
    /// 않은 task다(「바로 시작」 창에서 고른다). 옛 host는 생략한다
    #[serde(default)]
    pub repo: Option<String>,
    /// ` · t-0001`로 붙은 thread
    pub thread: Option<String>,
    /// task 줄 아래 들여쓴 메모
    pub notes: Vec<String>,
    pub done: bool,
    /// task 줄 번호(1부터, FRONT_API 1.60). 옛 host는 생략한다.
    #[serde(default)]
    pub line: u32,
    /// task 줄 원문(앞 공백 제외)
    #[serde(default)]
    pub raw: String,
    /// task 블록(task 줄과 메모 줄, 블록 끝 빈 줄 제외, 줄 끝 문자 없이 `\n`으로 이음)의 sha256. 편집(T2)의 충돌 anchor다.
    #[serde(default)]
    pub block_sha256: String,
    /// 들여쓴 하위 task면 상위 task의 `line`
    #[serde(default)]
    pub parent: Option<u32>,
    /// 형식 어긋남(owner 칸이 owner 꼴이 아님, 제목 안 thread 번호 등). 있으면 화면이 원문을 보이고 판단하지 않는다.
    #[serde(default)]
    pub problems: Vec<String>,
    /// 형식 어긋남 task의 「정리 제안」(FRONT_API 1.62): host가 만든 정규형 후보. 사람이 「이대로 저장」해야 바뀐다.
    #[serde(default)]
    pub suggestion: Option<TaskSuggestion>,
    /// 「확인 필요」의 까닭(FRONT_API 1.63, 할 일 T3): 이어진 thread가 끝났는데 완료로 판정하지 못했다(보고 표지·버림·하위 task
    /// 남음), 하위 task가 모두 끝났다 등. host가 지우지 않고 사람이 「끝남으로 지우기」·「다시 맡기기」·「연결만 풀기」로 정한다
    #[serde(default)]
    pub attention: Option<String>,
    /// FRONT_API 1.160: 확인 카드가 판단할 마지막 보고·착지·thread 상태. 옛 host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub attention_evidence: Option<TaskAttentionEvidence>,
    /// 자동 병합(FRONT_API 1.143): on이면 `[병합 준비됨]` 뒤 coordinator가 사용자에게 묻지 않고 착지 창을 준다. host DB 값이며
    /// 프로젝트 기본값 없이 off에서 시작한다. 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(optional, as = "Option<bool>")]
    pub auto_merge: bool,
}

/// 확인 필요 카드에서 고르는 처리(FRONT_API 1.160).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskAttentionAction {
    Complete,
    Reassign,
    Unlink,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskAttentionEvidence {
    pub thread: String,
    pub status: ThreadStatus,
    pub report_hash: Option<String>,
    pub report_first_line: Option<String>,
    pub landing_lines: Vec<String>,
}

/// 「정리 제안」(할 일 편집 T2): 끝 괄호 글 → 메모, 본문 속 열린 thread 번호 → 연결, `미할당` → owner 비움
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskSuggestion {
    pub title: String,
    pub owner: Option<String>,
    pub thread: Option<String>,
    /// 제안한 뒤의 메모 전체(지금 메모 앞에 옮긴 글을 둔다)
    pub notes: Vec<String>,
    /// 제안대로 쓴 task 줄(앞 공백 없이)
    pub line: String,
    /// 무엇을 바꾸나(한 줄씩)
    pub changes: Vec<String>,
}

/// 할 일 편집의 대상 task(FRONT_API 1.62). `block_sha256`으로 찾고(같은 hash가 여럿이면 `line`에 가까운 것), 그 블록이 없으면
/// `revision_conflict`다(지운 task가 저장으로 되살아나지 않는다). `block_sha256`이 비면 제목(대소문자 무시)으로 찾는다(CLI).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskAnchor {
    pub list: String,
    pub title: String,
    pub block_sha256: String,
    pub line: u32,
}

/// 할 일 편집 동작 하나(FRONT_API 1.62, `task_edit`). 한 요청의 동작은 차례로 적용해 한 번에 쓴다(하나라도 실패하면 쓰지 않는다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TaskOp {
    /// 목록 끝(또는 `before` 앞, `parent`의 하위 끝)에 더한다. 없는 목록은 파일 끝에 만든다
    Add {
        list: String,
        title: String,
        /// 비움 = 미할당
        #[serde(default)]
        owner: Option<String>,
        /// owner가 사람 이름이다(「사람으로 저장」). 아니면 `assignable --check`와 같은 판정을 거친다
        #[serde(default)]
        person: bool,
        /// 모델(profile) 칸(FRONT_API 1.75). 머신 칸과 함께 owner `모델@머신`으로 적는다. `owner`(me·사람)와 함께 주면 거부한다
        #[serde(default)]
        profile: Option<String>,
        /// 머신 칸(FRONT_API 1.75)
        #[serde(default)]
        machine: Option<String>,
        #[serde(default)]
        notes: Vec<String>,
        #[serde(default)]
        parent: Option<TaskAnchor>,
        #[serde(default)]
        before: Option<TaskAnchor>,
        /// 이어 줄 thread(` · t-NNNN`, FRONT_API 1.63 「task로 만들기」). 옛 host는 버린다
        #[serde(default)]
        thread: Option<String>,
        /// 저장소 칸(FRONT_API 1.73): 저장소 이름·경로나 `작업 폴더`. 경로는 host가 이름으로 바꿔 적는다. 비움 = 정하지 않음
        #[serde(default)]
        repo: Option<String>,
    },
    /// 준 칸만 바꾼다(`None`은 그대로, owner·저장소·thread의 빈 글은 비움). 바뀌지 않은 줄은 바이트 그대로 둔다
    Update {
        at: TaskAnchor,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        owner: Option<String>,
        #[serde(default)]
        person: bool,
        /// 모델 칸(FRONT_API 1.75): `None`은 그대로, 빈 글은 비움. 머신 칸만 바꾸면 모델은 그대로 둔다
        #[serde(default)]
        profile: Option<String>,
        /// 머신 칸(FRONT_API 1.75, `profile`과 같은 규칙)
        #[serde(default)]
        machine: Option<String>,
        #[serde(default)]
        notes: Option<Vec<String>>,
        #[serde(default)]
        thread: Option<String>,
        #[serde(default)]
        done: Option<bool>,
        /// 저장소 칸(FRONT_API 1.73, Add의 `repo`와 같은 값)
        #[serde(default)]
        repo: Option<String>,
    },
    /// 기존 메모 원문 끝에 줄을 더한다(FRONT_API 1.134). 블록 hash 충돌·기록은 Update와 같다.
    AppendNotes {
        at: TaskAnchor,
        notes: Vec<String>,
    },
    /// task 블록을 하위 task와 함께 지운다(기록에 남아 되살릴 수 있다)
    Delete {
        at: TaskAnchor,
    },
    /// 하위 task와 함께 옮긴다: `before` 앞, 아니면 `parent`의 하위 끝, 아니면 `to_list` 끝
    Move {
        at: TaskAnchor,
        to_list: String,
        #[serde(default)]
        before: Option<TaskAnchor>,
        #[serde(default)]
        parent: Option<TaskAnchor>,
    },
    /// 새 `##` 목록(파일 끝, 또는 `after` 목록 뒤)
    ListAdd {
        name: String,
        #[serde(default)]
        after: Option<String>,
    },
    ListRename {
        name: String,
        to: String,
    },
    /// `name`의 task와 설명 줄을 `into` 끝으로 옮기고 `name` 머리를 지운다
    ListMerge {
        name: String,
        into: String,
    },
    /// task가 없는 목록만 지운다(설명 줄은 기록에 남는다)
    ListDelete {
        name: String,
    },
    /// 목록을 `before` 목록 앞으로(없으면 파일 끝으로) 옮긴다
    ListMove {
        name: String,
        #[serde(default)]
        before: Option<String>,
    },
    /// 기록의 삭제 항목을 원래 목록 끝(또는 `list`)에 다시 넣는다. 같은 제목이 있으면 거부한다
    Restore {
        #[ts(type = "number")]
        log_id: i64,
        #[serde(default)]
        list: Option<String>,
    },
    /// 원문 편집(고급): 파일 전체 sha256이 같을 때만 통째로 쓴다. 다른 동작과 함께 보내지 않는다
    RawSave {
        expected_sha256: String,
        text: String,
    },
}

/// 편집이 최신 파일과 맞지 않았다(`revision_conflict`). 아무것도 쓰지 않았다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskConflict {
    /// 맞지 않은 동작의 차례(0부터)
    pub op: u32,
    pub reason: String,
    /// 그 task의 지금 원문(같은 제목으로 찾은 것, 지워졌으면 없음)
    // Box로 응답 enum의 크기를 제한한다. JSON·TypeScript 모양은 같다.
    pub latest: Option<Box<TaskView>>,
}

/// 할 일 기록의 동작
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskAction {
    Add,
    Update,
    Delete,
    Move,
    ListAdd,
    ListRename,
    ListMerge,
    ListDelete,
    ListMove,
    Restore,
    Raw,
    /// 끝난 thread의 task를 host가 지웠다(T3)
    AutoClean,
    Completed,
    /// 자동 병합 on/off 변경(FRONT_API 1.143). before·after는 `off`·`on`이다
    AutoMerge,
}

impl TaskAction {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskAction::Add => "add",
            TaskAction::Update => "update",
            TaskAction::Delete => "delete",
            TaskAction::Move => "move",
            TaskAction::ListAdd => "list_add",
            TaskAction::ListRename => "list_rename",
            TaskAction::ListMerge => "list_merge",
            TaskAction::ListDelete => "list_delete",
            TaskAction::ListMove => "list_move",
            TaskAction::Restore => "restore",
            TaskAction::Raw => "raw",
            TaskAction::AutoClean => "auto_clean",
            TaskAction::Completed => "completed",
            TaskAction::AutoMerge => "auto_merge",
        }
    }

    pub fn parse(s: &str) -> Option<TaskAction> {
        [
            TaskAction::Add,
            TaskAction::Update,
            TaskAction::Delete,
            TaskAction::Move,
            TaskAction::ListAdd,
            TaskAction::ListRename,
            TaskAction::ListMerge,
            TaskAction::ListDelete,
            TaskAction::ListMove,
            TaskAction::Restore,
            TaskAction::Raw,
            TaskAction::AutoClean,
            TaskAction::Completed,
            TaskAction::AutoMerge,
        ]
        .into_iter()
        .find(|a| a.as_str() == s)
    }

    /// 되살릴 수 있는 항목인가(task 블록이 빠졌다)
    pub fn removes(self) -> bool {
        matches!(self, TaskAction::Delete | TaskAction::AutoClean)
    }
}

/// 할 일 기록 한 줄(host DB `task_log`, FRONT_API 1.62)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskLogEntry {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub at_ms: u64,
    /// 누가(원문): `device:<id>`, `coordinator:<pane>`, `local:…`, `file`(파일 직접), `host`(자동 정리)
    pub actor: String,
    /// 누가(보이는 이름): 기기 이름·coordinator·터미널·파일 직접·host 자동 정리
    pub by: String,
    pub action: TaskAction,
    pub list: String,
    /// 옮김·목록 동작의 앞 목록(옮기기 전 목록, 이름 바꾸기·합치기 전 이름)
    pub from_list: Option<String>,
    pub title: String,
    /// 바뀌기 전 블록 원문(하위 task 포함, 더하기에는 없다)
    pub before: Option<String>,
    pub after: Option<String>,
    pub thread: Option<String>,
    /// 되살린 기록이면 그 항목 id
    #[ts(type = "number | null")]
    pub restored_by: Option<i64>,
}

/// 기록 거르기
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum TaskLogFilter {
    /// 삭제·자동 정리(되살릴 수 있는 것)
    Deleted,
    /// 자동 정리만
    Auto,
    /// 복원 동작 또는 되살린 삭제 기록
    Restored,
}

/// TASKS.md의 `##` 목록 하나(FRONT_API 1.60). task가 없는 목록도 있다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskListView {
    /// `##` 머리 이름. 첫 `##` 앞의 task·설명 줄은 이름 없는 목록(`""`)이다
    pub name: String,
    /// 머리 밑 체크박스 없는 줄(목록 설명). task 블록 안의 메모는 들지 않는다
    pub notes: Vec<String>,
}

/// 할 일 화면의 thread 한 줄(id·제목·묶음)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskThread {
    pub id: String,
    pub title: String,
    pub group: ThreadGroup,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub stage: Option<ThreadStage>,
}

/// `tasks` 답(FRONT_API 1.60): 할 일 화면이 읽는 TASKS.md 전체
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct TaskBoard {
    #[serde(default)]
    pub queue: Vec<crate::TaskReservation>,
    pub project: String,
    /// 파일 전체 바이트의 sha256(파일이 없으면 빈 글의 hash)
    pub sha256: String,
    pub lists: Vec<TaskListView>,
    pub tasks: Vec<TaskView>,
    /// 열린 thread 중 어느 task의 `thread`에도 없는 것
    pub unlinked_threads: Vec<TaskThread>,
    /// 파일 원문(원문 편집, FRONT_API 1.62). 옛 host는 생략한다
    pub text: String,
    /// task 저장소 칸에 쓸 수 있는 이름(FRONT_API 1.73): `project.toml` repos 경로의 마지막 이름, 겹치지 않게 적힌 차례
    pub repos: Vec<String>,
}

/// 할 일 「바로 시작」 확인창이 보일 것(FRONT_API 1.63, `thread_start_preview`). host가 `thread start --from-task`와 같은 규칙으로
/// 채운다(owner → profile·머신, 메모를 붙인 할 일 글). 아무것도 시작하지 않는다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ThreadStartPreview {
    #[serde(default)]
    pub workspace_access: std::collections::BTreeMap<String, Vec<String>>,

    pub profile_revision: String,
    pub model: String,
    pub effort: String,
    /// task 제목(thread 제목이 된다)
    pub title: String,
    /// 지금 파일의 그 task 블록 hash(시작 요청의 `task_block`)
    pub task_block: String,
    /// thread에 줄 할 일 글(제목과 「Notes from the task list」 메모)
    pub task: String,
    /// owner가 정한 profile, 없으면 프로젝트 기본 thread profile
    pub profile: String,
    /// owner가 정한 머신(없으면 host가 저장소의 머신이나 로컬로 고른다)
    pub machine: Option<String>,
    /// project.toml `repos`(worktree로 시작할 저장소). 비면 작업 폴더로 시작한다
    pub repos: Vec<RepoChoice>,
    /// 사용자·coordinator가 띄운 열린 Worker 수(시작·정리 중 포함)와 host가 집행하는 한도. 자동화·병합 대기·착지 한 자리는 뺀다.
    pub open_threads: u32,
    pub max_parallel_threads: u32,
    /// 이미 열린 thread가 이 task를 맡았다 등
    pub warnings: Vec<String>,
    /// task 저장소 칸(FRONT_API 1.73, `TaskView.repo`). 비면 창이 저장소나 작업 폴더를 고르게 한다
    pub repo: Option<String>,
    /// host 로컬 머신 이름(`RepoChoice.machine`이 빈 저장소의 머신)
    pub local_machine: Option<String>,
    /// 덜 붐비는 차례의 머신(연결·점검 이상 없음, 모든 프로젝트의 열린 thread 적은 차례 → 부하 → 이름). owner가 머신을 정하지 않았으면
    /// 그 저장소가 있는 머신 가운데 앞의 것이 기본값이다
    pub machine_order: Vec<String>,
}

/// thread를 시작할 저장소 하나(project.toml `repos`)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RepoChoice {
    pub path: String,
    /// 그 저장소가 있는 머신(없으면 host 로컬)
    pub machine: Option<String>,
}

/// 머신 하나에서 thread를 맡길 수 있는 profile(host 허용 목록과 그 머신 agent.toml의 교집합)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OwnerChoice {
    pub machine: String,
    pub connected: bool,
    /// 끊긴 머신은 비어 있다(모른다)
    pub profiles: Vec<String>,
}

/// coordinator가 매 턴 처음 보는 프로젝트 요약(0.2.35 `context`)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProjectContext {
    #[serde(default)]
    pub auto_mode: crate::AutoMode,
    #[serde(default)]
    pub auto_mode_exceptions: String,
    /// inbox를 끝내도 해제 전까지 남는 활성 post 음소거 경고
    #[serde(default)]
    pub post_warnings: Vec<String>,
    pub project: String,
    pub path: String,
    /// project.toml `start_threads`: propose(제안하고 승인을 기다린다) 또는 auto. aaa는 강제하지 않는다.
    pub start_threads: String,
    pub tasks: Vec<TaskView>,
    /// 열린 thread. `next`는 보고서 `## Next`에 `aaa thread next --add`로 더한 줄까지 붙인 목록이다.
    pub threads: Vec<ThreadView>,
    pub assignable: Vec<OwnerChoice>,
    /// 끝내지 않은 inbox 항목 수
    pub inbox_open: u32,
    /// 끝내지 않은 inbox 항목(herdr-projects `context`의 Inbox 절). 지시가 아니라 자료다.
    /// 응답 크기 한도 안에서 오래된 것부터 싣는다: `inbox_open`보다 적을 수 있다.
    #[serde(default)]
    pub inbox: Vec<InboxItem>,
    /// 채팅에 쓸 이 host 웹 주소(tailscale serve 공개 origin, plan §8.345). 질문·보고서는 `<주소>/#question=<thread>`·
    /// `<주소>/#report=<이름>` Markdown 링크로 가리킨다(터미널에서도 눌린다). 공개하지 않았으면 없다: 그때는 `aaa:` 꼴을 쓴다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub web_url: Option<String>,
}

/// 자동 배정 허용은 host DB, 저장소 경로는 프로젝트 설정이 정본이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceAccessView {
    pub project: String,
    pub machine: String,
    pub revision: String,
    pub workspaces: Vec<WorkspaceAccessRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkspaceAccessRow {
    pub workspace: String,
    pub enabled: bool,
    /// 비면 이 머신에 등록된 경로가 없다. 작업 폴더는 agent 작업 root를 쓴다.
    pub paths: Vec<String>,
}
