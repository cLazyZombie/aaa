//! 기획(t-1894, FRONT_API 1.170): 저장소 안 기획 폴더(`aaa.toml [[workspaces]]`의 `design`)의 Markdown 문서 트리.
//! 정본은 저장소 origin 기본 branch(`origin/main` 등)다. repo 머신 agent(`design.v1`)가 작업 파일·index를 바꾸지 않고 git 객체에서
//! 읽는다(fetch만 한다). 2단계(FRONT_API 1.171, agent `design_write.v1`)는 기획 branch `design/<주제>`의 전용 worktree에 쓰고
//! commit해 origin에 올린다. main에는 착지 창을 거친 것만 들어간다(사용자 결정 11-16).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 트리에 싣는 문서 수 상한(넘으면 `truncated`)
pub const DESIGN_MAX_DOCS: usize = 2000;
/// 문서 한 개 글 상한(바이트, 넘으면 읽지 않는다). CLI front socket의 한 frame(1MiB)에 JSON 이스케이프까지 들어가게 둔다
pub const DESIGN_MAX_TEXT: u64 = 512 * 1024;
/// 다른 머신 대화 thread의 기획 저장 원격 전달을 기다리는 시간(초). host는 main 쓰기(받기·검사·push를 다시 하기까지 1200초,
/// t-2002)와 앞선 저장의 차례를 기다릴 수 있다
pub const DESIGN_RELAY_SAVE_SECS: u64 = 1300;
/// 그림·영상 한 개 상한(바이트). 조각마다 blob을 처음부터 풀므로 크게 두지 않는다
pub const DESIGN_MAX_FILE: u64 = 16 * 1024 * 1024;
/// 원격 기획 thread가 CLI 그림을 host로 중계할 수 있는 기능
pub const DESIGN_ATTACH_RELAY_FEATURE: &str = "design_attach_relay.v1";
/// host의 원격 CLI 중계와 저장소 agent의 그림 삭제 요청 지원
pub const DESIGN_DETACH_FEATURE: &str = "design_detach.v1";

/// main 직접 저장(t-2002): 기획 쓰기를 origin 기본 branch에 바로 commit·push하는 agent 기능(`DesignMain*`·`DesignLog`).
/// 옛 agent는 이 요청을 모른다고 거절한다(옛 요청에 표시를 더하면 옛 agent가 조용히 기획 branch에 쓴다)
pub const DESIGN_MAIN_FEATURE: &str = "design_main.v1";
/// main 직접 저장에서 push가 non-fast-forward로 거절될 때 새 main 위에서 다시 하는 횟수(처음 포함)
pub const DESIGN_MAIN_TRIES: u32 = 3;
/// 문서 이력 한 번에 읽는 판 수 상한
pub const DESIGN_LOG_MAX: u32 = 100;

/// main 직접 저장의 파일 하나: 경로·읽은 blob(새 파일은 없음)·바꾸는 내용. 내용(`change`)은 꼭 있어야 한다(빠진 칸을
/// 지우기로 읽지 않는다)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesignMainFile {
    pub path: String,
    #[serde(default)]
    pub base: Option<String>,
    pub change: DesignMainChange,
}

/// main 직접 저장의 내용: 글, 먼저 넘긴 조각 파일(`DesignMainStage`의 전송 ID·sha256), 지우기
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DesignMainChange {
    Text {
        text: String,
    },
    Staged {
        sha256: String,
        /// 조각을 모은 전송(같은 그림을 동시에 붙여도 서로의 조각을 건드리지 않는다)
        #[serde(default)]
        upload: String,
        /// 이름을 준 그림(`--name`): 그 이름의 다른 그림이 있으면 바꾼다. 없으면 이름이 내용 hash라 같은 내용만 이미 붙은 것이다
        #[serde(default)]
        replace: bool,
    },
    Delete,
}

/// main 직접 저장의 답에 싣는 파일별 새 blob(지운 파일은 없음)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesignMainBlob {
    pub path: String,
    pub blob: Option<String>,
}

/// 문서 이력 한 줄(FRONT_API, t-2002): main이 실제로 가졌던 판(`--first-parent`)만, 새것부터
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignLogEntry {
    pub commit: String,
    #[ts(type = "number")]
    pub committed_ms: u64,
    /// commit 첫 줄
    pub subject: String,
    /// 기획 저장이면 `기획: <요약> (<누가>)`의 누가(대화 thread ID·사람·coordinator), 아니면 commit 작성자
    pub by: String,
    /// 기획 저장이 아닌 commit(착지 병합·새벽 갱신 등)
    pub other: bool,
}

/// 기획 저장 commit 첫 줄 `기획: <요약> (<누가>)`을 (요약, 누가)로 나눈다
pub fn design_commit_subject(subject: &str) -> Option<(&str, &str)> {
    let rest = subject.strip_prefix("기획: ")?;
    let rest = rest.strip_suffix(')')?;
    let (summary, by) = rest.rsplit_once(" (")?;
    (!by.is_empty() && !by.contains('(')).then_some((summary, by))
}

/// 저장에 함께 넣는 다른 글 파일(FRONT_API 1.186)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignSaveFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub base: Option<String>,
    pub text: String,
}

/// 기획 branch 주제(`design/<slug>`)의 글자 수 상한
pub const DESIGN_SLUG_MAX: usize = 48;

/// 기획 branch 주제인가: 영문 소문자·숫자·`-`, 1~48자, `-`로 시작·끝나거나 `--`가 들지 않는다(branch·폴더 이름에 그대로 쓴다)
pub fn design_slug_ok(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= DESIGN_SLUG_MAX
        && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && !slug.contains("--")
        // 웹 판 고르기의 「main」과 겹치지 않게
        && slug != "main"
}

/// 기획 branch 이름
pub fn design_branch(slug: &str) -> String {
    format!("design/{slug}")
}

/// 기획 branch가 main과 달리 고친 파일 하나(기획 폴더 안)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignChange {
    pub path: String,
    /// `A`(새 파일)·`M`(고침)·`D`(지움)
    pub status: String,
    /// 더하고 뺀 줄 수(FRONT_API 1.173, 바이너리·옛 agent는 없음)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub added: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub removed: Option<u32>,
}

/// 기획 대화에 끼우는 host 사건 한 줄(FRONT_API 1.173): 대화 시작·저장·올림·사람 고침·main에 반영
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignEvent {
    /// `open`·`save`·`push`·`edit`·`apply`, 반영 예약 `apply_wait`·`apply_cancel`·`apply_fail`(FRONT_API 1.178)
    pub kind: String,
    /// 한 줄(「문서 2 고침 · 1 새로」 같은 꼴은 웹이 짓는다: 여기는 경로·판 같은 자료)
    pub text: String,
    pub commit: Option<String>,
    /// 이 사건 전에 대화에 보낸 마지막 사람 말의 앞부분(그 말의 답 뒤에 끼운다). 없으면 대화 처음
    pub after: Option<String>,
    #[ts(type = "number")]
    pub at_ms: u64,
}

/// 기획 branch의 상태
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DesignBranchState {
    /// worktree가 있고 쓸 수 있다
    #[default]
    Open,
    /// 오래 손대지 않아 worktree를 돌려줬다(branch는 남는다). 다시 열면 이어 쓴다
    Rest,
}

/// 기획 branch 하나(host가 기억한다)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignBranchView {
    pub repo: String,
    /// `design/<slug>`의 slug
    pub slug: String,
    /// 주제(사람이 읽는 이름)
    pub title: String,
    pub state: DesignBranchState,
    /// 만든 이
    pub by: String,
    #[ts(type = "number")]
    pub created_ms: u64,
    /// 마지막으로 쓴 때(쉼을 정한다)
    #[ts(type = "number")]
    pub updated_ms: u64,
    /// 마지막 commit
    pub head: Option<String>,
    /// origin에 올린 commit
    pub pushed: Option<String>,
    /// 마지막으로 올리지 못한 까닭
    pub push_error: Option<String>,
    /// main과 달리 고친 파일
    pub changed: Vec<DesignChange>,
    /// main보다 앞선 commit 수
    pub ahead: u32,
    /// `changed`·`ahead`를 agent가 그 branch에서 읽어 실었다(FRONT_API 1.179, t-1950). 거짓이면 둘 다 기록만이라 0이 「main과 같음」이
    /// 아니다(머신 끊김·시한·옛 host)
    #[serde(default)]
    pub read: bool,
    /// 기획 대화 thread(FRONT_API 1.172, 아직 말하지 않았으면 없음)
    pub thread: Option<String>,
    /// thread에 아직 보내지 못한 말 수
    pub pending: u32,
    /// 대화 thread를 띄우거나 말을 보내지 못한 까닭
    pub chat_error: Option<String>,
    /// 갈라진 main 판(FRONT_API 1.173)
    pub base: Option<String>,
    /// 마지막으로 올린 때(0이면 올린 적 없음)
    #[ts(type = "number")]
    pub pushed_ms: u64,
    /// 대화 thread profile(없으면 프로젝트 기본)
    pub profile: Option<String>,
    /// 「main에 반영」을 맡은 thread
    pub apply: Option<String>,
    /// 자리(프로젝트 실행 한도)를 기다리는 「main에 반영」 예약(FRONT_API 1.178, t-1955). 자리가 나면 host가 반영 thread를 띄운다
    pub apply_wait: Option<DesignApplyWait>,
    /// 대화에 끼우는 host 사건(오래된 차례)
    pub events: Vec<DesignEvent>,
    /// 사람이 고쳤고 아직 대화 thread에 알리지 않은 문서(다음 말 앞에 붙는다)
    pub edited: Vec<String>,
}

/// 「main에 반영」 예약(FRONT_API 1.178): 반영 요청이 프로젝트 실행 한도에 걸려 자리를 기다린다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignApplyWait {
    /// 요청한 이(반영 thread를 띄울 때도 이 이름으로 띄운다)
    pub by: String,
    #[ts(type = "number")]
    pub requested_ms: u64,
    /// 지금 기다리는 까닭(자리·올리기·머신 연결)
    pub reason: String,
}

/// 기획 폴더 안 Markdown 문서 한 개
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignDocEntry {
    /// 저장소 기준 경로(`docs/game-loop/README.md`)
    pub path: String,
    /// 첫 `# ` 제목(없으면 빈 글)
    pub title: String,
    #[ts(type = "number")]
    pub bytes: u64,
    /// 블록 표지 수(FRONT_API 1.174, t-1894 3단계): `> [!계획]`(할 일로 넘긴 것 포함)
    pub plans: u32,
    /// `> [!미정]`
    pub open: u32,
    /// 할 일로 넘긴 계획 블록의 d-ID(`> [!계획] d-NNNN`, 나온 차례)
    pub tasks: Vec<String>,
    /// 기획서 ID(`k-NNNN`, FRONT_API 1.181, t-1957). agent는 문서 안 표지를 싣고, host는 원장으로 맞춘 번호를 싣는다(표지가 없는
    /// 문서도 번호가 있다). 옛 agent가 읽은 트리면 없다
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    /// `id`가 문서 안 표지에 적혀 있다. 거짓이면 host 원장의 번호이고 다음 저장이나 「한 번에 달기」가 적는다
    pub id_marked: bool,
    /// 문서 안 표지의 번호(agent가 읽은 그대로). `id`와 다르면 복사·다른 저장소의 표지라 원장이 다른 번호를 줬다: 표지는 고쳐 쓰지
    /// 않으므로 한 번에 달기도 건너뛴다
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id_file: Option<String>,
}

/// agent가 읽은 기획 폴더(한 commit 기준)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignTree {
    /// 읽은 commit(full SHA)
    pub commit: String,
    /// commit 시각(ms)
    #[ts(type = "number")]
    pub committed_ms: u64,
    /// 읽은 ref(`origin/main`, origin이 없으면 `HEAD`)
    pub base: String,
    /// 경로 차례
    pub docs: Vec<DesignDocEntry>,
    /// 문서가 상한보다 많아 뒤를 뺐다
    pub truncated: bool,
    /// origin을 받지 못해 가지고 있던 ref로 읽었다(까닭)
    pub fetch_error: Option<String>,
    /// 기획 branch를 읽었으면 main과 달리 고친 파일(기획 폴더 안, 경로 차례)
    pub changed: Vec<DesignChange>,
    /// 기획 branch를 읽었으면 main보다 앞선 commit 수
    pub ahead: u32,
    /// 문서마다 표지 수를 셌다(FRONT_API 1.174). 거짓이면 수를 모른다: 옛 agent, 기획 branch 트리, 세다 실패·시한
    pub marks: bool,
    /// 문서마다 ID 표지를 읽었다(FRONT_API 1.181, agent `design_ids.v1`). 거짓이면(옛 agent) host가 번호를 주지 않는다
    pub ids: bool,
}

/// 프로젝트 저장소 하나의 기획 폴더
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignRepoView {
    /// 저장소 이름(경로 끝 폴더 이름)
    pub repo: String,
    /// 저장소가 있는 머신 이름
    pub machine: String,
    /// 기획 폴더(저장소 기준, `docs`). 없으면 이 저장소는 기획 폴더를 정하지 않았다
    pub folder: Option<String>,
    /// 읽은 트리. 못 읽었으면 없고 `error`가 까닭이다
    pub tree: Option<DesignTree>,
    /// 못 읽은 까닭, 또는 마지막으로 읽은 트리를 보이는 까닭(머신 연결 끊김 등)
    pub error: Option<String>,
    /// 이 저장소의 기획 branch(FRONT_API 1.171, 만든 차례)
    pub branches: Vec<DesignBranchView>,
    /// main 문서에서 바로 맡겨 아직 main에 표지가 없는 블록(FRONT_API 1.176, t-1894 3-3). 표지는 구현 branch 첫 commit에 있다
    pub handoffs: Vec<DesignHandoffNote>,
}

/// 맡긴 할 일 하나(FRONT_API 1.176): main 판에는 아직 표지가 없을 수 있다. 웹은 main 판의 같은 블록(덩어리 글)에 할 일 칩을 단다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignHandoffNote {
    /// `d-NNNN`
    pub task: String,
    /// 저장소 기준 문서 경로
    pub path: String,
    /// 맡긴 블록의 글(덩어리 그대로, 표지의 d-ID는 뺌: main 판과 같은 글)
    pub blocks: Vec<String>,
    /// 기획 branch에서 맡겼으면 그 주제(표지가 그 branch에 있다). 비면 main 문서에서 맡김(표지는 구현 branch에)
    pub branch: String,
}

/// 문서 하나(읽기 전용)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignDoc {
    /// 프로젝트(그림 주소 `/api/v1/projects/<p>/design/<저장소>/<경로>`)
    pub project: String,
    pub repo: String,
    /// 저장소 기준 경로
    pub path: String,
    pub title: String,
    pub text: String,
    /// 파일 blob SHA(다음 단계 저장의 기준 판)
    pub blob: String,
    /// 읽은 commit(그림 주소에 싣는다)
    pub commit: String,
    /// 기획 branch에서 읽었으면 그 slug(FRONT_API 1.171)
    pub branch: Option<String>,
    #[ts(type = "number")]
    pub committed_ms: u64,
    /// 기획서 ID(FRONT_API 1.181): 문서 안 표지, 없으면 host 원장의 번호
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
}

/// 지금 기획 트리(main 판)에 있는 문서의 ID 하나(FRONT_API 1.181, `design_ids`의 답). 웹은 이 목록에 있는 `k-NNNN`만 링크한다
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignIdView {
    /// `k-NNNN`
    pub id: String,
    pub repo: String,
    /// 저장소 기준 경로
    pub path: String,
    pub title: String,
}

/// 「한 번에 달기」로 agent에 넘기는 문서 하나와 그 ID(agent `design_ids.v1`)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignIdMark {
    pub path: String,
    pub id: String,
}

/// 구현 맡기기에서 고른 표지 블록(FRONT_API 1.175): 표지 블록끼리의 차례(0부터)와 그 첫 줄(앞뒤 빈칸 뺌).
/// host는 같은 차례의 블록 첫 줄이 같은지 다시 본다(웹과 host의 덩어리 규칙이 어긋나도 다른 블록에 적지 않는다)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignPick {
    pub ordinal: u32,
    pub head: String,
}

/// 구현 맡기기의 결과(FRONT_API 1.175)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct DesignHandoffView {
    /// 만든 할 일(`d-NNNN`)
    pub task: String,
    /// 표지를 적은 commit(못 적었으면 없음)
    pub commit: Option<String>,
    /// origin에 올렸다
    pub pushed: bool,
    /// 예약했다
    pub reserved: bool,
    /// 할 일과 표지 commit은 됐지만 올리기·예약 중 못한 것(창에 보인다. 올리기 전에는 그 할 일을 시작하지 않는다)
    pub warning: Option<String>,
}

/// 표지 블록 하나(t-1894 3단계): 첫 줄 번호(`\n`으로 나눈 0부터), 미정인가, 넘긴 계획의 d-ID(정규형), 속한 절(가장 가까운
/// 앞 제목, 없으면 빈 글)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignMark {
    pub line: usize,
    /// 블록 끝 다음 줄 번호(블록은 `line..end`)
    pub end: usize,
    pub open: bool,
    pub task: Option<String>,
    pub section: String,
}

/// 웹(JS `\s`·`trim`)과 같은 빈칸: Rust `char::is_whitespace`와 달리 U+FEFF(BOM)는 빈칸이고 U+0085는 아니다
fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// 웹 `String.prototype.trim`과 같은 앞뒤 빈칸 빼기(구현 맡기기의 고른 블록 첫 줄 대조)
pub fn design_trim(s: &str) -> &str {
    s.trim_matches(js_space)
}

/// `d-` 뒤 숫자의 정규형 할 일 번호(웹 `canonicalShortId`와 같다: 앞 0을 빼고 네 자리 이상, 0이나 i64를 넘으면 번호가 아님)
pub fn design_task_id(digits: &str) -> Option<String> {
    let n = digits.chars().take_while(char::is_ascii_digit).count();
    let value = digits[..n].trim_start_matches('0');
    if value.is_empty() || value.len() > 19 || (value.len() == 19 && value > "9223372036854775807") {
        return None;
    }
    Some(format!("d-{value:0>4}"))
}

/// 코드 울타리 줄(```·~~~ 셋 이상)이면 그 글자와 길이
fn fence_of(line: &str) -> Option<(char, usize)> {
    let t = line.trim_start_matches(js_space);
    let c = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = t.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// 열린 울타리 `(c, len)`을 이 줄이 닫는가: 같은 글자로 연 것보다 짧지 않은 울타리만
fn fence_closes(line: &str, (c, len): (char, usize)) -> bool {
    fence_of(line).is_some_and(|(c2, len2)| c2 == c && len2 >= len) && design_trim(line).chars().all(|x| x == c)
}

/// 문서의 표지 블록(나온 차례). 웹(`designModel.ts`의 `mdBlocks`·`calloutOf`)과 같은 규칙이다: CRLF도 같고,
/// 빈 줄·제목·코드 울타리로 나눈 덩어리 중 모든 줄이 인용(`>`)이고 첫 줄이 `> [!계획]`·`> [!미정]`인 것만 표지다.
/// 울타리 안의 표기 예시는 표지가 아니고, d-ID는 `[!계획]` 뒤만 읽는다. 두 쪽은 공용 견본(`web/test/fixtures/design-marks.md`)으로 맞춘다
pub fn design_marks(text: &str) -> Vec<DesignMark> {
    /// 제목 줄의 글(닫는 `#`은 뺀다, 웹 `headingOf`와 같다)
    fn heading(line: &str) -> Option<String> {
        let n = line.chars().take_while(|c| *c == '#').count();
        if !(1..=6).contains(&n) || !line[n..].chars().next().is_some_and(js_space) {
            return None;
        }
        let t = design_trim(&line[n..]);
        let closed = t.trim_end_matches('#');
        let t = if closed.is_empty() {
            ""
        } else if closed.ends_with(js_space) {
            closed.trim_end_matches(js_space)
        } else {
            t
        };
        Some(t.to_string())
    }
    fn tally(block: &[(usize, &str)], section: &str, out: &mut Vec<DesignMark>) {
        let Some((line, first)) = block.first() else { return };
        let end = block.last().map_or(*line, |(n, _)| n + 1);
        if !block.iter().all(|(_, l)| l.trim_start_matches(js_space).starts_with('>')) {
            return;
        }
        let head = first.trim_start_matches(js_space)[1..].trim_start_matches(js_space);
        if let Some(rest) = head.strip_prefix("[!계획]") {
            let task = rest.trim_start_matches(js_space).strip_prefix("d-").and_then(design_task_id);
            out.push(DesignMark { line: *line, end, open: false, task, section: section.to_string() });
        } else if head.starts_with("[!미정]") {
            out.push(DesignMark { line: *line, end, open: true, task: None, section: section.to_string() });
        }
    }
    let mut out = Vec::new();
    let mut block: Vec<(usize, &str)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut section = String::new();
    for (n, line) in text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).enumerate() {
        let mark = fence_of(line);
        if let Some(open) = fence {
            if fence_closes(line, open) {
                fence = None;
            }
            continue;
        }
        // ID 표지 줄(t-1957)은 홀로 한 덩어리다: 웹 `mdBlocks`와 같다(제목 아래 빈 줄 없이 붙은 표지 블록을 삼키지 않는다)
        if design_id_line(line).is_some() {
            tally(&block, &section, &mut out);
            block.clear();
            continue;
        }
        let title = heading(line);
        if mark.is_some() || design_trim(line).is_empty() || title.is_some() {
            tally(&block, &section, &mut out);
            block.clear();
            fence = mark;
            if let Some(title) = title {
                section = title;
            }
            continue;
        }
        block.push((n, line));
    }
    tally(&block, &section, &mut out);
    out
}

/// 표지 블록의 글(줄 끝 `\r`을 뺀 덩어리 그대로)
pub fn design_mark_text(text: &str, mark: &DesignMark) -> String {
    text.split('\n')
        .skip(mark.line)
        .take(mark.end - mark.line)
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// 블록 글 첫 줄의 `[!계획] d-NNNN`에서 d-ID를 뺀 글(맡기기 전 글과 견준다)
pub fn design_unmark(block: &str) -> String {
    let (first, rest) = match block.split_once('\n') {
        Some((f, r)) => (f, Some(r)),
        None => (block, None),
    };
    let Some(at) = first.find("[!계획]").map(|i| i + "[!계획]".len()) else { return block.to_string() };
    let tail = first[at..].trim_start_matches(js_space);
    let Some(digits) = tail.strip_prefix("d-") else { return block.to_string() };
    let n = digits.chars().take_while(char::is_ascii_digit).count();
    if n == 0 {
        return block.to_string();
    }
    let after = digits[n..].trim_start_matches(js_space);
    let line = if after.is_empty() { first[..at].to_string() } else { format!("{} {after}", &first[..at]) };
    match rest {
        Some(r) => format!("{line}\n{r}"),
        None => line,
    }
}

/// 고른 표지(`design_marks`의 차례, 0부터)마다 첫 줄 `[!계획]` 바로 뒤에 ` <d-ID>`를 적는다(구현 맡기기, t-1894 3-2).
/// 고른 것이 없거나, 넘기지 않은 계획 블록이 아니면 그 까닭으로 거절한다. 다른 줄은 바이트 그대로다
pub fn design_mark_tasks(text: &str, picks: &[usize], id: &str) -> Result<String, String> {
    if picks.is_empty() {
        return Err("넘길 블록을 고르지 않았다".into());
    }
    let marks = design_marks(text);
    let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
    let mut done = std::collections::BTreeSet::new();
    for &pick in picks {
        if !done.insert(pick) {
            continue;
        }
        let Some(mark) = marks.get(pick) else { return Err(format!("{}번째 표지 블록이 없다", pick + 1)) };
        if mark.open {
            return Err("미정 블록은 넘길 수 없다(정한 뒤 넘긴다)".into());
        }
        if let Some(task) = &mark.task {
            return Err(format!("이미 넘긴 블록이다: {task}"));
        }
        let line = &mut lines[mark.line];
        let at = line.find("[!계획]").ok_or("표지 줄을 찾지 못했다")? + "[!계획]".len();
        // 표지 뒤에 글이 붙어 있으면(`[!계획]100층`) 번호와 글 사이도 띄운다: 붙이면 숫자가 이어져 다른 번호가 된다
        let glued = line[at..].chars().next().is_some_and(|c| !js_space(c));
        line.insert_str(at, &if glued { format!(" {id} ") } else { format!(" {id}") });
    }
    Ok(lines.join("\n"))
}

/// 기획서 ID 접두어(t-1957): `k-NNNN`. 번호는 host 원장이 낸다(전역, 재사용하지 않는다)
pub const DESIGN_ID_PREFIX: char = 'k';
/// ID 표지를 찾는 문서 앞부분(바이트). agent 트리는 문서마다 이만큼만 읽으므로 전문을 읽는 쪽도 같은 범위만 본다
pub const DESIGN_ID_BYTES: usize = 8 * 1024;

/// 줄이 ID 표지(`<!-- aaa-id: k-0012 -->`, 앞뒤 빈칸·CR 무시)면 정규형 ID. 번호가 아니면(`k-0`) 표지가 아니다
pub fn design_id_line(line: &str) -> Option<String> {
    let inner = design_trim(line).strip_prefix("<!--")?.strip_suffix("-->")?;
    let id = design_trim(inner).strip_prefix("aaa-id:")?;
    crate::canonical_short_id(design_trim(id), DESIGN_ID_PREFIX)
}

/// 코드 울타리 밖의 ID 표지 줄(줄 번호, ID)
fn design_id_lines(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fence = None;
    for (n, line) in text.split('\n').enumerate() {
        if let Some(open) = fence {
            if fence_closes(line, open) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = fence_of(line) {
            fence = Some(open);
            continue;
        }
        if let Some(id) = design_id_line(line) {
            out.push((n, id));
        }
    }
    out
}

/// 문서의 ID: 앞 [`DESIGN_ID_BYTES`] 안에서 끝난 줄 가운데 코드 울타리 밖 첫 표지. `head`는 문서 앞부분(전문이어도 된다),
/// `total`은 문서 전체 바이트다. 앞부분이 잘렸으면 끝나지 않은 마지막 줄은 보지 않는다: agent 트리(앞부분)와 전문 읽기가 같은 답을 낸다
pub fn design_doc_id(head: &[u8], total: u64) -> Option<String> {
    let head = &head[..head.len().min(DESIGN_ID_BYTES)];
    let head = if total > head.len() as u64 { &head[..head.iter().rposition(|b| *b == b'\n')?] } else { head };
    design_id_lines(&String::from_utf8_lossy(head)).into_iter().next().map(|(_, id)| id)
}

/// 글의 ID(`design_doc_id`를 전문에)
pub fn design_text_id(text: &str) -> Option<String> {
    design_doc_id(text.as_bytes(), text.len() as u64)
}

/// 첫 `# ` 제목. 앞 YAML front matter(`---`)는 건너뛴다
pub fn design_title(text: &str) -> String {
    let mut lines = text.lines();
    let mut first = true;
    let mut front = false;
    for line in lines.by_ref() {
        let line = line.trim_end();
        if first {
            first = false;
            if line == "---" {
                front = true;
                continue;
            }
        }
        if front {
            if line == "---" || line == "..." {
                front = false;
            }
            continue;
        }
        if let Some(t) = line.strip_prefix("# ") {
            return t.trim().trim_end_matches('#').trim().to_string();
        }
    }
    String::new()
}

/// 저장소 기준 상대 경로를 정리한다: `\`·빈 마디·`.`·`..`·절대 경로·NUL을 받지 않는다. 앞 `./`와 끝 `/`는 뗀다
pub fn design_clean_path(path: &str) -> Option<String> {
    let p = path.trim().trim_start_matches("./").trim_end_matches('/');
    if p.is_empty() || p.starts_with('/') || p.contains('\\') || p.contains('\0') {
        return None;
    }
    if p.split('/').any(|s| s.is_empty() || s == "." || s == "..") {
        return None;
    }
    Some(p.to_string())
}

/// `path`가 기획 폴더 `folder` 안인가(둘 다 `design_clean_path`를 거친 값)
pub fn design_inside(folder: &str, path: &str) -> bool {
    path.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    /// 구현 맡기기: 고른 표지 첫 줄에만 d-ID를 적고, 미정·이미 넘긴 것·없는 차례는 거절한다(공용 견본, LF·CRLF)
    #[test]
    fn handoff_marks_only_the_picked_plan_blocks() {
        let raw = include_str!("../../../web/test/fixtures/design-marks.md").replace("\r\n", "\n");
        for text in [raw.clone(), raw.replace('\n', "\r\n")] {
            let before = design_marks(&text);
            assert_eq!(before.len(), 9);
            assert_eq!(design_mark_text(&text, &before[0]), "> [!계획]\n> 100층까지 늘린다.", "블록 글(CR 뺌)");
            assert_eq!(before[0].end - before[0].line, 2);
            let sections: Vec<_> = before.iter().map(|m| m.section.as_str()).collect();
            assert_eq!(
                sections,
                ["견본", "견본", "절", "절", "절", "절", "절", "절", "절"],
                "가장 가까운 앞 제목(닫는 #은 뺀다)"
            );
            let out = design_mark_tasks(&text, &[6, 0, 0], "d-0050").unwrap();
            let after = design_marks(&out);
            let tasks: Vec<_> = after.iter().map(|m| m.task.as_deref()).collect();
            assert_eq!(
                tasks,
                [
                    Some("d-0050"),
                    Some("d-0012"),
                    Some("d-0013"),
                    None,
                    None,
                    Some("d-0014"),
                    Some("d-0050"),
                    Some("d-0012"),
                    Some("d-0015")
                ]
            );
            assert_eq!(out.lines().count(), text.lines().count(), "줄 수 그대로");
            let changed: Vec<_> =
                text.split('\n').zip(out.split('\n')).filter(|(a, b)| a != b).map(|(_, b)| b).collect();
            assert_eq!(changed.len(), 2);
            assert!(changed.iter().all(|l| l.contains("[!계획] d-0050")), "{changed:?}");
            assert_eq!(text.contains('\r'), out.contains('\r'), "CRLF는 그대로");
            assert!(design_mark_tasks(&text, &[3], "d-0050").unwrap_err().contains("미정"));
            assert!(design_mark_tasks(&text, &[1], "d-0050").unwrap_err().contains("이미 넘긴"));
            assert!(design_mark_tasks(&text, &[9], "d-0050").is_err());
            assert!(design_mark_tasks(&text, &[], "d-0050").is_err());
        }
        // 표지 뒤에 글이 붙은 줄: 번호와 글을 띄운다
        let glued = design_mark_tasks("> [!계획]100층까지", &[0], "d-0050").unwrap();
        assert_eq!(glued, "> [!계획] d-0050 100층까지");
        assert_eq!(design_marks(&glued)[0].task.as_deref(), Some("d-0050"));
        // 웹과 같은 빈칸: 맨 앞 BOM 뒤의 표지도 표지이고, U+0085로 시작하는 줄은 인용이 아니다
        assert_eq!(design_marks("\u{feff}> [!계획]\n> 글").len(), 1);
        assert_eq!(design_marks("\u{85}> [!계획]").len(), 0);
        assert_eq!(design_trim("\u{feff} 첫 줄 \u{a0}"), "첫 줄");
        // d-ID 빼기: 맡기기 전 글로 돌아간다
        assert_eq!(design_unmark("> [!계획] d-0006\n> 100층"), "> [!계획]\n> 100층");
        assert_eq!(design_unmark("> [!계획] d-0050 100층까지"), "> [!계획] 100층까지");
        assert_eq!(design_unmark("> [!계획]\n> 그대로"), "> [!계획]\n> 그대로");
        let original = "> [!계획]\n> 글";
        let marked = design_mark_tasks(original, &[0], "d-0007").unwrap();
        assert_eq!(design_unmark(&marked), original);
    }

    use super::*;

    /// 기획서 ID 표지(t-1957): 줄 꼴·울타리 밖·앞 8KiB 안 끝난 줄. 잘린 앞부분과 전문이 같은 답이다
    #[test]
    fn id_marker_is_read_the_same_from_head_and_whole_text() {
        assert_eq!(design_id_line("<!-- aaa-id: k-12 -->").as_deref(), Some("k-0012"));
        assert_eq!(design_id_line("  <!--aaa-id:k-0012-->\r").as_deref(), Some("k-0012"));
        for bad in [
            "<!-- aaa-id: k-0 -->",
            "<!-- aaa-id: d-12 -->",
            "<!-- aaa-id: k-12 x -->",
            "`<!-- aaa-id: k-12 -->`",
            "<!-- aaa-id: k-12 --->",
        ] {
            assert_eq!(design_id_line(bad), None, "{bad}");
        }
        assert_eq!(design_text_id("# 제목\n<!-- aaa-id: k-0005 -->\n\n본문").as_deref(), Some("k-0005"));
        assert_eq!(design_text_id("```\n<!-- aaa-id: k-0005 -->\n```\n"), None, "울타리 안 예시는 표지가 아니다");
        assert_eq!(
            design_text_id("~~~\n~~~\n<!-- aaa-id: k-6 -->\n<!-- aaa-id: k-7 -->").as_deref(),
            Some("k-0006"),
            "첫 표지"
        );
        // 앞 8KiB 경계: 그 안에서 끝난 줄만. agent는 앞부분과 전체 크기를, 전문 읽기는 글 전체를 준다
        let marker = "<!-- aaa-id: k-0009 -->";
        for pad in
            [DESIGN_ID_BYTES - marker.len() - 2, DESIGN_ID_BYTES - marker.len() - 1, DESIGN_ID_BYTES - marker.len()]
        {
            let text = format!("{}\n{marker}\n{}", "a".repeat(pad), "b".repeat(100));
            let head = &text.as_bytes()[..DESIGN_ID_BYTES];
            assert_eq!(design_doc_id(head, text.len() as u64), design_text_id(&text), "pad {pad}");
        }
        let fits = format!("{}\n{marker}\nx", "a".repeat(DESIGN_ID_BYTES - marker.len() - 3));
        assert!(design_text_id(&fits).is_some());
        let over = format!("{}\n{marker}\nx", "a".repeat(DESIGN_ID_BYTES));
        assert_eq!(design_text_id(&over), None, "앞부분 밖");
    }

    #[test]
    fn slugs_are_plain_branch_names() {
        for ok in ["boss-dex", "a", "floors-100", &"x".repeat(48)] {
            assert!(design_slug_ok(ok), "{ok}");
        }
        for bad in ["", "Boss", "boss_dex", "-a", "a-", "a--b", "a/b", "a.b", "보스", "main", &"x".repeat(49)] {
            assert!(!design_slug_ok(bad), "{bad}");
        }
        assert_eq!(design_branch("boss-dex"), "design/boss-dex");
    }

    #[test]
    fn title_is_first_h1_after_front_matter() {
        assert_eq!(design_title("# 게임 루프\n\n본문"), "게임 루프");
        assert_eq!(design_title("---\ntitle: x\n# 아님\n---\n\n## 둘\n# 하나 #\n"), "하나");
        assert_eq!(design_title("본문만\n## 절"), "");
        assert_eq!(design_title("#붙음\n# 띄움"), "띄움");
    }

    #[test]
    fn clean_path_rejects_escape() {
        assert_eq!(design_clean_path("./docs/"), Some("docs".into()));
        assert_eq!(design_clean_path("docs/game-loop/README.md"), Some("docs/game-loop/README.md".into()));
        for bad in ["", "/etc", "docs/../x", "docs//x", "docs/./x", "a\\b", ".."] {
            assert_eq!(design_clean_path(bad), None, "{bad}");
        }
        assert_eq!(design_clean_path("docs/aux.md").as_deref(), Some("docs/aux.md"), "읽기는 받는다");
        assert!(design_inside("docs", "docs/a.md"));
        assert!(!design_inside("docs", "docs2/a.md"));
        assert!(!design_inside("docs", "docs"));
    }
}
