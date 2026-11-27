//! 질문·알림·메시지 타입과 보고서 질문 절 해석.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    Open,
    Answered,
    /// 답하기 전에 harness가 넘어갔다(다른 곳에서 답함, 세션 끝남)
    Gone,
}

/// 화면에서 읽은 선택 창(herdr-web-ui `InteractivePrompt`, plan §8.172). `id`는 내용(커서·체크 상태 빼고)의 hash라,
/// 답할 때 화면을 다시 읽어 다르면 거절한다(그 사이 바뀐 질문에 잘못 답하지 않는다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionMenu {
    pub id: String,
    /// question(AskUserQuestion)·approval(도구 승인)·plan(계획 승인)·menu(여러 질문의 검토)
    pub kind: String,
    pub title: String,
    pub question: String,
    /// 참고 글(명령·계획·검토할 답)
    pub body: Option<String>,
    pub options: Vec<MenuOption>,
    pub multi_select: bool,
    /// 「직접 쓰기」 자리. `options.len()`이면 그 줄은 `options`에 없다(Claude 질문의 「Type something.」)
    pub custom_option_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MenuOption {
    pub label: String,
    pub description: Option<String>,
}

/// 선택 창 답(참고 `PromptAnswer`): 하나 고르기·여러 개 고르기·직접 쓰기 가운데 하나
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "by", rename_all = "snake_case")]
pub enum MenuPick {
    Option { index: u32 },
    Options { indices: Vec<u32> },
    Custom { text: String },
}

/// 확인창 질문(§8.4). 답은 host가 세션·상태를 다시 확인한 뒤 adapter 키 입력으로 보낸다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionView {
    pub id: String,
    /// 프로젝트별 짧은 별칭. id는 기존 확인창 참조로 유지한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub short_id: Option<String>,
    pub pane: String,
    pub machine: String,
    pub project: Option<String>,
    pub thread: Option<String>,
    /// permission·trust(FrontResponse 태그 `kind`와 겹치지 않게 이름을 바꿨다)
    pub question_kind: String,
    pub text: String,
    /// 고를 수 있는 답(예: yes·no)
    pub options: Vec<String>,
    pub state: QuestionState,
    pub answer: Option<String>,
    #[ts(type = "number")]
    pub created_at_ms: u64,
    /// 화면에서 읽은 선택 창(못 읽었으면 None: 그때는 `options`의 예·아니오로 답한다)
    #[serde(default)]
    pub menu: Option<QuestionMenu>,
}

/// 앱 안 알림의 종류(plan §8.185). 휴대폰 push는 아니다
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    /// host 내부 감시의 이상·회복. 웹 앱 안 알림.
    HostIssue,
    HostRecovered,
    /// 배포의 최종 성공/중단. 웹 앱 안 알림만 보낸다.
    RolloutFinished,
    RolloutFailed,
    /// 보는 화면 없이 agent의 턴이 끝났다(herdr `ToastKind::Finished`: 작업·질문에서 쉼으로, 안 봄)
    Finished,
    /// post 남용 한도에 걸린 출처가 음소거됐다. 앱 안에서만 알린다.
    PostMuted,
    /// 규칙이 명시한 앱 안 안내. 외부 push가 아니다.
    Automation,
    /// thread의 새 보고(herdr-projects 「review · new report」 알림, host 「new report」 항목과 같은 때)
    Report,
}

/// 앱 안 알림 하나(host가 front마다 보낸다, `FrontEvent::Notice`). 입력 필요는 따로 없고 `Question` 사건이 맡는다.
/// 같은 턴의 끝남과 새 보고는 새 보고 하나다(plan §8.185).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct NoticeView {
    pub kind: NoticeKind,
    /// 갈 pane(새 보고는 그 thread의 pane, 없으면 null)
    pub pane: Option<String>,
    pub machine: String,
    pub project: Option<String>,
    pub thread: Option<String>,
    /// thread 제목 또는 pane 이름
    pub title: String,
    /// harness 이름(claude·codex, 끝남만)
    pub agent: Option<String>,
    /// 끝난 턴의 길이(작업·질문으로 들어선 때부터). host가 그 턴의 시작을 못 봤으면 null
    #[ts(type = "number | null")]
    pub turn_ms: Option<u64>,
    /// 긴 턴(herdr-web-ui `LONG_TURN_MS` 60초 이상이거나 길이를 모름): 웹 앱 안 알림은 이것만 띄운다
    pub long: bool,
    #[ts(type = "number")]
    pub at_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageMode {
    /// 지금 보낸다(작업 중이면 Esc로 먼저 멈춘다)
    Interrupt,
    /// 바로 입력한다. 작업 중이면 harness가 다음 턴으로 줄 세운다(herdr-projects `thread prompt`). 이름은 옛 판 그대로다
    #[default]
    Queue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    /// 기록했고 아직 보내는 중이다
    Queued,
    /// harness가 입력을 받았다(turn_start)
    Accepted,
    /// 받았는지 모른다. 자동으로 다시 보내지 않는다(§8.5 a)
    Unknown,
}

/// PTY에 쓴 단계. harness 수락 여부는 DeliveryState에 따로 남긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MessageInput {
    NotWritten,
    PasteUnconfirmed,
    DraftWritten,
    EnterUnconfirmed,
    Submitted,
}

impl MessageInput {
    pub fn label(self) -> &'static str {
        match self {
            Self::NotWritten => "입력 안 함",
            Self::PasteUnconfirmed => "붙여넣기 쓰기 확인 못함",
            Self::DraftWritten => "본문 입력됨 · Enter 미전송",
            Self::EnterUnconfirmed => "Enter 쓰기 확인 못함",
            Self::Submitted => "Enter 입력됨",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageView {
    pub id: String,
    pub project: String,
    pub thread: String,
    pub mode: MessageMode,
    pub state: DeliveryState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub input: Option<MessageInput>,
    pub text: String,
    #[ts(type = "number")]
    pub created_at_ms: u64,
}

/// thread가 구현 전에 모아 묻는 질문 묶음(plan §8.329). 답은 현재 보고 hash에 묶이며, 같은 미답 질문의 번호·부분 답은 보고 갱신에도 유지한다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadQuestions {
    /// 형식 확인 필요: 질문·선택지 누락이나 글자 순서 불일치를 알린다. 답 전송은 허용한다. 옛 host는 생략한다.
    /// 줄마다 경고 하나다. `근거 확인 필요`(근거가 질문 칸 밖에만 있음)·`파일 확인 필요`(가리키는 library 파일이 그 판에 없음)로
    /// 시작하는 줄은 형식 오류가 아니다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub format_warning: Option<String>,
    /// FRONT_API 1.96. Questions 없는 검수 대기에 host가 만든 기본 검수 카드. B는 고칠 점을 글로 적는다.
    #[serde(default)]
    pub review: bool,
    /// 이 질문이 든 보고 사본의 hash. 답은 이 hash가 지금 보고일 때만 받는다
    pub report_hash: String,
    pub items: Vec<ThreadQuestion>,
    /// 답했으면 그 답(웹 카드·`aaa thread answer`). 모든 기기가 같은 값을 본다
    pub answer: Option<QuestionAnswerView>,
    /// 본문 그림의 주소 앞부분(FRONT_API 1.78, plan §8.345): 본문의 `library/<경로>`는 `<files_base><경로>`로 읽는다.
    /// 그 보고의 host 사본에서만 주고, 새 보고가 오면 옛 주소는 열리지 않는다. 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub files_base: Option<String>,
    /// 짧은 질문 ID로 일부만 답한 경우의 답. 전체 답은 answer에 남는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub draft: Option<QuestionAnswerView>,
}

/// 질문 하나: 글과 선택지(앞에서부터 A·B·C…)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ThreadQuestion {
    /// 답한 질문을 제외한 카드도 원문의 질문 번호로 답한다(FRONT_API 1.132).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_number: Option<u32>,
    /// 선택지 뒤의 Markdown 설명·그림(FRONT_API 1.99). 선택지 그림과 섞거나 버리지 않는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub after: Option<String>,
    /// 프로젝트별 영속 순번(q-1000부터). 옛 보고 파서·host는 생략한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub id: Option<String>,
    pub text: String,
    pub choices: Vec<QuestionChoice>,
    /// `### N. 질문` 머리 아래의 Markdown 본문(FRONT_API 1.78, plan §8.345): 그림·표·code·링크. 옛 꼴과 옛 host는 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionChoice {
    /// 보고에 쓴 선택지 글자(FRONT_API 1.132). 옛 자료는 배열 위치의 A부터 읽는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub letter: Option<String>,
    pub label: String,
    /// 「 — 」 뒤의 까닭 한 줄
    pub description: Option<String>,
    /// 「(추천)」 표지가 있었다
    pub recommended: bool,
    /// 선택지 줄 아래 두 칸 들여쓴 Markdown(시안 그림 등, FRONT_API 1.78). 없으면 생략한다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
}

impl QuestionChoice {
    pub fn letter_at(&self, index: usize) -> String {
        self.letter.clone().unwrap_or_else(|| char::from(b'A' + index as u8).to_string())
    }
}

impl ThreadQuestion {
    /// 표시 글자를 배열 위치로 바꾼다. picks의 기존 위치 계약은 유지한다.
    pub fn choice_index(&self, letter: &str) -> Option<u32> {
        self.choices
            .iter()
            .enumerate()
            .find_map(|(i, c)| c.letter_at(i).eq_ignore_ascii_case(letter).then_some(i as u32))
    }
}

/// 한도 대기 때문에 pane에 아직 쓰지 않은 카드 답의 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AnswerDelivery {
    Waiting,
    Held,
}

/// 받은 답. `summary`는 「1A 2B」 꼴(직접 쓴 답이 있으면 「직접」)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct QuestionAnswerView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivery: Option<AnswerDelivery>,
    /// 질문마다 고른 선택지 번호(0부터). 고르지 않고 직접 쓴 질문은 null
    pub picks: Vec<Option<u32>>,
    /// 직접 쓴 답(질문 번호를 붙여 쓴다, 예: 「2 둘 다 보이게」)
    pub text: Option<String>,
    /// 질문 배열 순서의 직접 답·의견. 빈 배열은 옛 공통 text만 쓰는 답이다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<Option<String>>>")]
    pub comments: Vec<Option<String>>,
    pub summary: String,
    /// 답한 곳(웹 기기 이름·`CLI`)
    pub by: String,
    #[ts(type = "number")]
    pub at_ms: u64,
}

/// 「답할 것」 기록의 종류(plan §8.345): thread 질문, coordinator 확인 필요 post, 할 일 확인 필요
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AskKind {
    Question,
    Request,
    Task,
}

/// 답한 것 하나(plan §8.345, FRONT_API 1.78). 답할 때의 원문을 보존한다: 새 보고가 질문을 비워도 남고, host가 14일 뒤 지운다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AskRecord {
    /// FRONT_API 1.96. 답할 때의 보고 판. 새 보고 뒤에도 같은 원문을 연다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub report_id: Option<String>,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub question_id: Option<String>,
    pub kind: AskKind,
    /// 질문은 thread 번호, 확인 필요 post는 inbox 항목 ID, 할 일은 task 제목
    pub subject: String,
    /// thread 제목·post 제목·task 제목
    pub title: String,
    /// 물은 때(보고를 받은 때·post를 받은 때). 모르면 답한 때
    #[ts(type = "number")]
    pub asked_ms: u64,
    #[ts(type = "number")]
    pub done_ms: u64,
    /// 답한 곳(웹 기기 이름·`CLI`·`coordinator`)
    pub by: String,
    /// 「1A 2B」·「답함으로 표시」·「끝남으로 지움」
    pub summary: String,
    /// 질문 원문(본문·선택지 본문 포함)과 답
    #[serde(default)]
    pub items: Vec<ThreadQuestion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub answer: Option<QuestionAnswerView>,
    /// 확인 필요 post 본문·할 일 확인 필요 까닭(Markdown 글)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub body: Option<String>,
    /// 보존한 파일의 주소 앞부분: `<files_base>library/<경로>`가 그림, `<files_base>report.md`가 그때 보고다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub files_base: Option<String>,
    /// FRONT_API 1.191(t-2117). 고치기 전의 답들(처음 답부터). `answer`가 지금 답이다. 지우지 않고 덧붙이기만 한다
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<QuestionAnswerView>>")]
    pub revisions: Vec<QuestionAnswerView>,
    /// FRONT_API 1.191. coordinator 확인 필요 post가 중계한 thread(답할 때 정한다). 정정 기한이 이 thread를 따른다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub thread: Option<String>,
    /// FRONT_API 1.191. 지금 답을 고칠 수 있는지. host가 읽을 때마다 정하고 저장하지 않는다. 없으면 고치는 길을 보이지 않는다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub correction: Option<AskCorrection>,
}

/// 답 정정 상태(FRONT_API 1.191, t-2117). thread 카드는 그 thread가 끝나기 전까지, coordinator 카드는 중계한 thread가
/// 끝나기 전까지(없으면 기록이 남아 있는 동안) 고칠 수 있다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AskCorrection {
    pub open: bool,
    /// 「t-0001이 끝나기 전까지 고칠 수 있다」·「t-0001이 끝나 고칠 수 없다…」
    pub note: String,
    /// 지금 답 뒤 일어난 일(새 보고·coordinator 요청). 고치기 확인 창과 정정 알림에 싣는다
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(optional, as = "Option<Vec<String>>")]
    pub since: Vec<String>,
}

/// 답한 것의 가벼운 색인(FRONT_API 1.161). 배지·직접 링크·목록에는 원문이 필요 없다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AskRecordSummary {
    pub id: String,
    pub question_id: Option<String>,
    pub kind: AskKind,
    pub subject: String,
    pub title: String,
    #[ts(type = "number")]
    pub done_ms: u64,
    pub by: String,
    pub summary: String,
    pub question_ids: Vec<String>,
    pub answer_summary: Option<String>,
}

/// 열린 coordinator 확인 필요 post(`post user --request --needs-user`, plan §8.345). inbox 항목이 끝나지 않은 동안 「답할 것」이다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct UserRequestView {
    /// 평문 요청도 포함한 짧은 질문 ID(여러 질문이면 첫 ID).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub question_id: Option<String>,
    /// inbox 항목 ID(「답함으로 표시」가 끝낸다)
    pub id: String,
    pub title: String,
    /// post 본문(Markdown 글). 본문이 지워졌으면 inbox 요약
    pub body: String,
    #[ts(type = "number")]
    pub created_ms: u64,
    /// post 본문의 `## Questions`(FRONT_API 1.87). 없으면 기존 평문 카드다. report_hash는 본문 hash다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub questions: Option<ThreadQuestions>,
}

/// 질문당 선택지·질문 수 상한(글자 A–I, 숫자 1–9)
pub const QUESTION_MAX: usize = 9;

/// 질문 본문 한도(바이트). thread 사건은 모든 기기에 가므로 본문을 줄인다. 넘으면 잘랐다고 꼬리에 적는다
pub const QUESTION_BODY_MAX: usize = 16 * 1024;

/// 선택지 본문 한도(바이트)
pub const CHOICE_BODY_MAX: usize = 4 * 1024;

/// code fence 줄(``` 또는 ~~~로 시작)
pub(super) fn fence_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// 질문·선택지 맨 앞의 Markdown 강조를 푼다. 본문 속 별표와 밑줄은 보존한다.
pub(super) fn unmark_leading(text: &str) -> String {
    let mut out = text.trim().to_string();
    while let Some(mark) = ["**", "__", "*", "_"].into_iter().find(|m| out.starts_with(m)) {
        let Some(end) = out[mark.len()..].find(mark).map(|i| i + mark.len()) else { break };
        out.replace_range(end..end + mark.len(), "");
        out.replace_range(..mark.len(), "");
    }
    out
}

fn bullet_text(text: &str) -> Option<&str> {
    text.strip_prefix("- ").or_else(|| text.strip_prefix("* "))
}

fn bullet_start(text: &str) -> Option<&str> {
    Some(bullet_text(text)?.trim_start().trim_start_matches(['*', '_']))
}

fn starts_with_short_id(item: &str) -> bool {
    let id = item
        .trim_start_matches('`')
        .split(|c: char| !c.is_alphanumeric() && !matches!(c, '-' | '_'))
        .next()
        .unwrap_or_default();
    id.chars().next().is_some_and(|prefix| crate::short_id_number(id, prefix).is_some())
}

/// 짧은 ID로 시작하는 설명과 단어는 후보에서 뺀다. 잘못된 구두점이나 강조가 있는 선택지도 찾는다.
pub(super) fn choice_letter(text: &str) -> Option<char> {
    let item = bullet_start(text.trim())?;
    if starts_with_short_id(item) {
        return None;
    }
    let mut chars = item.chars();
    let letter = chars.next()?;
    (letter.is_ascii_alphabetic() && chars.next().is_none_or(|c| !c.is_alphanumeric()))
        .then_some(letter.to_ascii_uppercase())
}

/// `- A 선택지 (추천) — 까닭`, `- **A(추천)** …` 등. 앞 공백은 부르는 쪽이 정한다.
pub(super) fn choice_line(text: &str) -> Option<QuestionChoice> {
    let item = unmark_leading(bullet_text(text)?);
    let mut chars = item.chars();
    let (Some(letter), Some(sep)) = (chars.next(), chars.next()) else { return None };
    if !letter.is_ascii_alphabetic() || !matches!(sep, ' ' | ')' | '.' | '(') {
        return None;
    }
    let rest = &item[letter.len_utf8()..];
    if sep == '(' && !["(추천)", "(Recommended)", "(recommended)"].iter().any(|m| rest.starts_with(m)) {
        return None;
    }
    let rest = rest.trim_start_matches([' ', ')', '.']).trim();
    let (label, description) = match rest.split_once(" — ") {
        Some((l, d)) => (l.trim(), Some(d.trim().to_string()).filter(|d| !d.is_empty())),
        None => (rest, None),
    };
    let mut recommended = false;
    let mut label = label.to_string();
    for mark in ["(추천)", "(Recommended)", "(recommended)"] {
        if label.contains(mark) {
            recommended = true;
            label = label.replace(mark, "");
        }
    }
    let label = unmark_leading(&label).split_whitespace().collect::<Vec<_>>().join(" ");
    (!label.is_empty()).then_some(QuestionChoice {
        letter: Some(letter.to_ascii_uppercase().to_string()),
        label,
        description,
        recommended,
        body: None,
    })
}

/// 앞뒤 빈 줄을 뗀 본문(없으면 None). `max`를 넘으면 글자 경계에서 자르고, 열린 code fence를 닫은 뒤 잘랐다고 적는다
pub(super) fn body_of(lines: &[String], max: usize) -> Option<String> {
    let text = lines.join("\n");
    let text = text.trim_matches('\n').trim_end();
    if text.trim().is_empty() {
        return None;
    }
    if text.len() <= max {
        return Some(text.to_string());
    }
    let mut cut = max;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = text[..cut].trim_end().to_string();
    if out.lines().filter(|l| fence_line(l)).count() % 2 == 1 {
        out.push_str("\n```");
    }
    out.push_str("\n\n*(본문이 길어 잘렸다. 나머지는 보고서에서 본다)*");
    Some(out)
}

/// 본문·선택지 목록·그 뒤의 설명을 분리한다. 선택지 그림은 들여쓴 줄로 그 선택지에 남기고,
/// 목록 뒤의 열 0 설명·그림은 after로 보존한다. code fence 안 줄은 선택지가 아니다.
fn rich_question(text: String, block: &[String]) -> (ThreadQuestion, std::ops::Range<usize>) {
    let mut fenced = vec![false; block.len()];
    let mut inside = false;
    for (i, l) in block.iter().enumerate() {
        if fence_line(l) {
            fenced[i] = true;
            inside = !inside;
        } else {
            fenced[i] = inside;
        }
    }
    let is_choice = |i: usize| !fenced[i] && choice_line(&block[i]).is_some();
    let mut runs = Vec::new();
    let mut active: Option<(usize, usize, usize)> = None;
    for (i, line) in block.iter().enumerate() {
        if is_choice(i) {
            let run = active.get_or_insert((i, i, 0));
            run.1 = i + 1;
            run.2 += 1;
        } else if line.trim().is_empty() || line.starts_with(' ') || line.starts_with('\t') {
            if let Some(run) = active.as_mut() {
                run.1 = i + 1;
            }
        } else if let Some(run) = active.take() {
            runs.push(run);
        }
    }
    if let Some(run) = active {
        runs.push(run);
    }
    // 본문의 보통 글머리 하나보다 끝의 A/B 목록을 우선한다. 여러 목록이면 선택한 범위 밖 줄을 진단한다.
    let (start, end, _) =
        runs.iter().rev().find(|r| r.2 >= 2).or(runs.last()).copied().unwrap_or((block.len(), block.len(), 0));
    let mut choices: Vec<QuestionChoice> = Vec::new();
    let mut extra: Vec<Vec<String>> = Vec::new();
    for (i, l) in block.iter().enumerate().take(end).skip(start) {
        if is_choice(i) {
            choices.push(choice_line(l).expect("선택지 줄"));
            extra.push(Vec::new());
        } else if let Some(lines) = extra.last_mut() {
            let indent = l.len() - l.trim_start_matches(' ').len();
            lines.push(l[indent.min(2)..].to_string());
        }
    }
    for (c, lines) in choices.iter_mut().zip(&extra) {
        c.body = body_of(lines, CHOICE_BODY_MAX);
    }
    choices.truncate(QUESTION_MAX);
    let question = ThreadQuestion {
        source_number: None,
        id: None,
        text,
        choices,
        body: body_of(&block[..start], QUESTION_BODY_MAX),
        after: body_of(&block[end..], QUESTION_BODY_MAX),
    };
    (question, start..end)
}

/// 보고를 읽은 결과와 누락·글자 불일치 진단. 경고는 진단용이며 읽은 선택지·직접 답 전송을 막지 않는다.
#[derive(Debug, Default)]
pub struct ParsedReportQuestions {
    pub items: Vec<ThreadQuestion>,
    pub warning: Option<String>,
}

struct QuestionBlock {
    number: usize,
    heading_line: usize,
    text: String,
    lines: Vec<String>,
    rich: bool,
    tail_choices: usize,
}

// 진단은 카드·post 응답·pane 메시지에 실린다. 잘못된 줄이 많은 문서도 그 응답을 받을 수 있게 한다.
const QUESTION_ISSUE_MAX: usize = 64;

#[derive(Default)]
struct QuestionIssues {
    lines: Vec<(usize, String)>,
    omitted: usize,
}

fn format_issue(
    issues: &mut QuestionIssues,
    line: usize,
    location: impl std::fmt::Display,
    fix: impl std::fmt::Display,
) {
    if issues.lines.len() >= QUESTION_ISSUE_MAX {
        issues.omitted += 1;
        return;
    }
    issues.lines.push((line, format!("형식 확인 필요: {location} · {line}번 줄. {fix}")));
}

impl QuestionBlock {
    fn finish(mut self, items: &mut Vec<ThreadQuestion>, issues: &mut QuestionIssues) {
        // 옛 번호 목록은 들여쓴 선택지도 지원한다.
        if !self.rich {
            for line in &mut self.lines {
                if choice_letter(line).is_some() {
                    *line = line.trim_start().to_string();
                }
            }
        }
        let (mut question, choice_range) = rich_question(unmark_leading(&self.text), &self.lines);
        let location = format!("{}번 질문", self.number);
        let mut issue = |line, fix: std::fmt::Arguments<'_>| format_issue(issues, line, &location, fix);
        let mut seen = [None; QUESTION_MAX];
        let mut fenced = false;
        let mut seen_choice = false;
        for (i, text) in self.lines.iter().enumerate() {
            let line = self.heading_line + i + 1;
            if fence_line(text) {
                fenced = !fenced;
                continue;
            }
            if fenced {
                continue;
            }
            let valid = choice_line(text).is_some();
            seen_choice |= valid;
            let Some(letter) = choice_letter(text) else {
                if seen_choice && bullet_start(text).is_some_and(|item| !starts_with_short_id(item)) {
                    issue(
                        line,
                        format_args!(
                            "선택지라면 줄 머리에 글자를 붙인다(예: `- B 보류`). 설명이라면 선택지 목록 위에 둔다."
                        ),
                    );
                }
                continue;
            };
            let index = (letter as u8 - b'A') as usize;
            let Some(first) = seen.get_mut(index) else {
                issue(line, format_args!("선택지 글자는 A부터 I까지다. {letter}를 고치고 선택지를 9개 이하로 쓴다."));
                continue;
            };
            if let Some(first_line) = *first {
                issue(
                    line,
                    format_args!("{letter} 선택지가 {first_line}번 줄과 중복된다. 각 선택지에 서로 다른 글자를 쓴다."),
                );
            } else {
                *first = Some(line);
            }
            if !valid {
                issue(line, format_args!("{letter} 선택지의 줄 머리를 `- {letter} 선택지` 형식으로 고친다."));
            } else if !choice_range.contains(&i) {
                issue(
                    line,
                    format_args!(
                        "{letter} 선택지를 이 질문의 선택지 목록에 모아 쓴다. 새 질문이면 그 위에 `### N. 질문`을 쓴다."
                    ),
                );
            }
        }
        let mut next = 0;
        for (index, &line) in seen.iter().enumerate() {
            let Some(line) = line else { continue };
            if next < index {
                let missing =
                    (next..index).map(|i| char::from(b'A' + i as u8).to_string()).collect::<Vec<_>>().join("·");
                issue(line, format_args!("{missing} 선택지가 빠졌다. 빠진 선택지를 쓰거나 A부터 글자를 다시 붙인다."));
            }
            next = index + 1;
        }
        let count = question.choices.len();
        if count < 2 {
            issue(
                self.heading_line,
                format_args!("선택지가 {count}개다. `- A 진행`·`- B 보류`처럼 선택지를 2개 이상 쓴다."),
            );
        }
        // 같은 글자 둘은 답 대상을 정할 수 없다. 직접 답 카드에 원문을 남긴다.
        if question.choices.iter().enumerate().any(|(i, c)| question.choices[..i].iter().any(|p| p.letter == c.letter))
        {
            question.choices.clear();
            question.body = body_of(&self.lines, QUESTION_BODY_MAX);
            question.after = None;
        }
        if items.len() < QUESTION_MAX {
            items.push(question);
        }
    }
}

/// `### N. 질문`과 `N. **질문**` 모두 본문과 끝의 선택지 목록을 읽고 원문의 줄 번호로 진단한다.
pub fn parse_report_questions(report: &str) -> ParsedReportQuestions {
    let mut out = ParsedReportQuestions::default();
    let mut issues = QuestionIssues::default();
    let mut section_line = None;
    let mut inside = false;
    let mut fenced = false;
    let mut current: Option<QuestionBlock> = None;
    let mut question_count = 0;
    let mut overflow_line = None;
    for (i, line) in report.trim_start_matches('\u{feff}').lines().enumerate() {
        let line_number = i + 1;
        if !fenced && let Some(head) = line.strip_prefix("## ") {
            if let Some(block) = current.take() {
                block.finish(&mut out.items, &mut issues);
            }
            inside = head.trim() == "Questions";
            if inside {
                section_line.get_or_insert(line_number);
            }
            continue;
        }
        if fence_line(line) {
            fenced = !fenced;
        } else if inside && !fenced {
            let heading = line.strip_prefix("### ");
            let head = heading.unwrap_or(line).trim();
            if let Some((n, rest)) = head.split_once(". ")
                && !n.is_empty()
                && n.bytes().all(|c| c.is_ascii_digit())
                && !rest.trim().is_empty()
                // ### 본문의 번호 목록은 본문으로 둔다. 선택지가 끝난 뒤의 N.은 다음 질문이다.
                && (heading.is_some() || current.as_ref().is_none_or(|b| !b.rich || b.tail_choices >= 2))
            {
                if let Some(block) = current.take() {
                    block.finish(&mut out.items, &mut issues);
                }
                question_count += 1;
                if question_count > QUESTION_MAX {
                    overflow_line.get_or_insert(line_number);
                }
                let number = n.parse::<usize>().ok();
                if number != Some(question_count) {
                    format_issue(
                        &mut issues,
                        line_number,
                        number.map_or_else(|| "질문 머리".into(), |n| format!("{n}번 질문")),
                        format_args!("이 질문의 번호를 {question_count}로 고친다."),
                    );
                }
                current = Some(QuestionBlock {
                    number: number.unwrap_or(question_count),
                    heading_line: line_number,
                    text: rest.trim().to_string(),
                    lines: Vec::new(),
                    rich: heading.is_some(),
                    tail_choices: 0,
                });
                continue;
            }
            if line.starts_with("# ") {
                if let Some(block) = current.take() {
                    block.finish(&mut out.items, &mut issues);
                }
                inside = false;
                continue;
            }
            if current.is_none() && choice_letter(line).is_some() {
                let number = question_count + 1;
                format_issue(
                    &mut issues,
                    line_number,
                    format_args!("{number}번 질문"),
                    format_args!("선택지 위에 `### {number}. 질문` 머리를 쓴다."),
                );
            }
        }
        if let Some(block) = current.as_mut() {
            if !fenced && choice_line(line).is_some() {
                block.tail_choices += 1;
            } else if fence_line(line) || !(line.trim().is_empty() || line.starts_with(' ') || line.starts_with('\t')) {
                block.tail_choices = 0;
            }
            block.lines.push(line.to_string());
        }
    }
    if let Some(block) = current {
        block.finish(&mut out.items, &mut issues);
    }
    if let Some(line) = section_line
        && question_count == 0
        && issues.lines.is_empty()
    {
        format_issue(&mut issues, line, "Questions", "이 절 아래에 `### 1. 질문`과 선택지를 2개 이상 쓴다.");
    }
    if let Some(line) = overflow_line {
        issues.lines.push((line, format!(
            "보고당 질문 최대 {QUESTION_MAX}개: 질문 {question_count}개 중 {}개를 읽었다. {line}번 줄부터 다음 보고로 옮긴다. {QUESTION_MAX}개 이하로 나눠 남은 질문은 기록해 다음 보고로 이어 묻는다.",
            out.items.len()
        )));
    }
    // 첫 줄이 선언한 수 역시 검사한다. 일반 보고 글의 숫자는 질문 수로 읽지 않는다.
    let declared = report
        .lines()
        .next()
        .and_then(|l| l.split_once("질문 "))
        .and_then(|(_, r)| r.split_once("개"))
        .and_then(|(n, _)| n.parse::<usize>().ok());
    if let Some(n) = declared
        && n != question_count
    {
        format_issue(
            &mut issues,
            1,
            "질문 수",
            format_args!("첫 줄에는 {n}개, Questions에는 {question_count}개가 있다. 질문 수를 맞춘다."),
        );
    }
    if !issues.lines.is_empty() {
        issues.lines.sort_by_key(|(line, _)| *line);
        let mut messages: Vec<_> = issues.lines.into_iter().map(|(_, message)| message).collect();
        if issues.omitted > 0 {
            messages
                .push(format!("형식 확인 필요: 오류가 {}개 더 있다. 위 줄을 고친 뒤 다시 검사한다.", issues.omitted));
        }
        out.warning = Some(messages.join("\n"));
        if out.items.is_empty() {
            out.items.push(ThreadQuestion {
                source_number: None,
                id: None,
                text: "직접 답".into(),
                choices: Vec::new(),
                body: body_of(&report.lines().map(str::to_string).collect::<Vec<_>>(), QUESTION_BODY_MAX),
                after: None,
            });
        }
    }
    out
}

/// 진단이 필요 없는 기존 호출용. 카드·답 전송 경계는 `parse_report_questions`의 warning을 함께 확인한다.
pub fn report_questions(report: &str) -> Vec<ThreadQuestion> {
    parse_report_questions(report).items
}
