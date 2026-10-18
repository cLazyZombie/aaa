//! 화면에서 읽는 선택 창(plan §8.171, 차이 목록 2장 Q2).
//!
//! herdr-web-ui `server/prompt.ts`(aa624cb, MIT)의 읽기와 답 키(`answerKeys`)를 옮겼다: Claude Code 질문(AskUserQuestion,
//! 하나·여러 개·직접 쓰기)·여러 질문의 검토·도구 승인·계획 승인, Codex 승인. 참고처럼 hook 내용이 아니라 화면 글에서 읽고,
//! 메뉴의 안내 줄이 화면 끝에 있을 때만 살아 있는 창으로 본다(`tail_active`). `id`는 내용의 hash(커서·체크 상태 빼고)라
//! 터미널에서 커서를 옮겨도 같은 질문이다.
//!
//! 뺀 것(plan §8.171): Codex 질문(질문 대기열·rollout 읽기), omp·omo·hermes·pi, 미리보기 상자가 있는 질문, 초안이 든
//! 직접 쓰기 줄, 모델·effort 목록, 번호 없는 메뉴(폴더 신뢰 등은 지금처럼 예·아니오로 답한다).

use std::sync::LazyLock;

use aaa_proto::{MenuOption, MenuPick, QuestionMenu};
use regex::Regex;
use sha2::{Digest, Sha256};

fn re(s: &str) -> Regex {
    Regex::new(s).unwrap()
}

static NUMBERED: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*([›>❯])?\s*(\d+)\.\s+(.+)$"));
static SELECTED: LazyLock<Regex> = LazyLock::new(|| re(r"^[❯›>]\s*"));
static CHECK_BOX: LazyLock<Regex> = LazyLock::new(|| re(r"^\[[ xX✓✔]\]\s*"));
static CHECKED: LazyLock<Regex> = LazyLock::new(|| re(r"^\[[xX✓✔]\]"));
static MULTI_ROW: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:[›>❯]\s*)?\d+\.\s+\[[ xX✓✔]\]"));
// 여러 질문은 탭 사이를 옮긴다: 「Tab/Arrow keys to navigate」
static CLAUDE_ASK_HINT: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)enter to select.*(?:↑/↓|tab/arrow keys) to navigate.*esc to cancel"));
static CLAUDE_PREVIEW_HINT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\bn to add notes\b"));
// 질문 탭 막대(`←  ☒ Route  ☐ Author  ✔ Submit  →`, 좁은 pane은 오른쪽이 잘린다)
static CLAUDE_TABS: LazyLock<Regex> = LazyLock::new(|| re(r"^←\s+[☐☒☑✔]"));
static CHIP: LazyLock<Regex> = LazyLock::new(|| re(r"^[☐☒☑✔]\s+(\S.*)$"));
static CHIP_LIKE: LazyLock<Regex> = LazyLock::new(|| re(r"^[☐☒☑✔]\s+\S"));
static SUBMIT_ROW: LazyLock<Regex> = LazyLock::new(|| re(r"^(?:[›>❯]\s*)?Submit$"));
static TYPE_SOMETHING: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^Type something\.?$"));
static SOLID_RULE: LazyLock<Regex> = LazyLock::new(|| re(r"^[─━]{8,}$"));
static SUBMIT_QUESTION: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^Ready to submit your answers\?$"));
static PLAN_QUESTION: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)Claude has written up a plan and is ready to execute\. Would you like to proceed\?"));
static TELL_CLAUDE: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^Tell Claude what to change$"));
static DO_YOU_WANT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^Do you want to .+\?$"));
static ESC_TO_CANCEL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)esc to\s+cancel"));
static TOOL_CALL: LazyLock<Regex> = LazyLock::new(|| re(r"^●\s+[\w.:-]+(?:\s[\w.:-]+)*(?:\s\(MCP\))?\("));
static CODEX_APPROVAL_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)(?:Would you like to (?:run|make|apply|continue|grant)|Allow Codex to|Approve (?:this )?(?:app )?tool call|Do you trust the contents|Trust this folder\?|Enable full access)",
    )
});
static REJECT_ROW: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:No|Reject|Cancel|Deny)\b"));
static CLAUDE_TASKS_HEAD: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(\d+) tasks \(\d+ done, (?:\d+ in progress, )?\d+ open\)$"));
static CLAUDE_TASK_ROW: LazyLock<Regex> = LazyLock::new(|| re(r"^[◻◼✔]\s"));
static CLAUDE_TASKS_MORE: LazyLock<Regex> = LazyLock::new(|| re(r"^…\s\+\d+ pending$"));
static LABELED_RULE: LazyLock<Regex> = LazyLock::new(|| re(r"^─{3,}\s.*─$"));
static CLAUDE_HINT_TAIL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)\besc to (?:cancel|exit|go back)\b"));
static CLAUDE_SUBMIT_TAIL: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(?:[›>❯]\s*)?\d+\.\s+Cancel$"));
// Claude Code 2.1.29x 승인은 「Esc to cancel」로 끝난다(뒤에 배경 agent 멈춤 조합이 붙기도). 뒤에 아무 글이나 오면 옛 창이다
static CLAUDE_APPROVAL_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)esc to cancel.*(?:tab|ctrl\+e)|ctrl\+e to explain|^esc to cancel(?:\s*·\s*ctrl\+x ctrl\+k twice to stop background agents)?$",
    )
});
static CLAUDE_PLAN_TAIL: LazyLock<Regex> =
    LazyLock::new(|| re(r"(?i)ctrl\+g to edit|shift\+tab to approve with this feedback"));
static CODEX_APPROVAL_TAIL: LazyLock<Regex> = LazyLock::new(|| {
    re(
        r"(?i)press enter to confirm|esc to cancel|enter continue.*esc back|^(?:[›>❯]\s*)?\d+\.\s+(?:No|Reject|Cancel|Deny)\b",
    )
});

/// 그 창의 답 키를 아는 읽기(참고 `Responder`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Responder {
    ClaudeQuestion,
    ClaudeSubmit,
    ClaudeApproval,
    ClaudePlan,
    CodexApproval,
}

/// 읽은 창: 카드에 보일 것(`menu`)과 답 키를 만들 때 쓰는 것
#[derive(Debug, Clone)]
pub(crate) struct Parsed {
    pub menu: QuestionMenu,
    responder: Responder,
    /// 커서가 있는 줄(커서가 지나는 줄 가운데. 여러 개 고르기는 Submit 줄도 센다)
    pub selected: usize,
    checked: Vec<usize>,
    /// 직접 쓰기 줄(커서가 지나는 줄 가운데)
    custom_row: Option<usize>,
    /// 이 줄은 Esc 하나로 답한다(Codex의 거절)
    reject_with_escape: Option<usize>,
}

/// 답 한 걸음. `Up`·`Down`은 옮기기만이라 되돌릴 수 있고, 나머지는 저지른다
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    Up,
    Down,
    Key(&'static [u8]),
    Text(String),
}

pub(crate) const ENTER: &[u8] = b"\r";
const ESC: &[u8] = b"\x1b";
const RIGHT: &[u8] = b"\x1b[C";
const BACKTAB: &[u8] = b"\x1b[Z";

impl Step {
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Step::Up => b"\x1b[A".to_vec(),
            Step::Down => b"\x1b[B".to_vec(),
            Step::Key(k) => k.to_vec(),
            Step::Text(t) => t.as_bytes().to_vec(),
        }
    }
}

#[derive(Debug, Clone)]
struct Row {
    number: u32,
    label: String,
    selected: bool,
    checked: bool,
    description: Option<String>,
    line: usize,
}

fn clean(line: &str) -> String {
    let mut s = line.trim();
    if let Some(r) = s.strip_prefix('│') {
        s = r.trim_start();
    }
    if let Some(r) = s.strip_suffix('│') {
        s = r.trim_end();
    }
    s.trim().to_string()
}

fn is_divider(line: &str) -> bool {
    let v = clean(line);
    !v.is_empty() && v.chars().all(|c| c.is_whitespace() || "╭╮╰╯├┤┬┴┼─━═╌▔".contains(c))
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_last(lines: &[String], mut pred: impl FnMut(usize, &str) -> bool) -> Option<usize> {
    (0..lines.len()).rev().find(|&i| pred(i, &lines[i]))
}

/// 한 줄과 그 뒤 두 줄을 하나로: 좁은 pane은 안내 줄을 접는다
fn wrapped(lines: &[String], index: usize) -> String {
    lines[index..lines.len().min(index + 3)]
        .iter()
        .map(|l| clean(l))
        .filter(|l| !l.is_empty() && !is_divider(l))
        .collect::<Vec<_>>()
        .join(" ")
}

fn numbered_rows(lines: &[String], start: usize, end: usize) -> Vec<Row> {
    let mut rows = Vec::new();
    for (i, line) in lines.iter().enumerate().take(end).skip(start) {
        let Some(m) = NUMBERED.captures(line.trim()) else { continue };
        let label = m[3].trim();
        rows.push(Row {
            number: m[2].parse().unwrap_or(0),
            checked: CHECKED.is_match(label),
            label: CHECK_BOX.replace(label, "").trim().to_string(),
            selected: m.get(1).is_some(),
            description: None,
            line: i,
        });
    }
    // 설명은 다음 줄까지, 좁은 pane이 접은 것은 빈 줄·구분선까지
    for k in 0..rows.len() {
        let next = rows.get(k + 1).map(|r| r.line).unwrap_or(end);
        let mut words = Vec::new();
        for line in &lines[rows[k].line + 1..next] {
            let l = clean(line);
            if !l.is_empty() && !is_divider(&l) {
                words.push(l);
            } else if !words.is_empty() {
                break;
            }
        }
        if !words.is_empty() {
            rows[k].description = Some(normalize(&words.join(" ")));
        }
    }
    rows
}

/// 위에서부터 읽은 번호 줄 가운데 메뉴의 것: 커서 줄과 같은 칸의 줄에서 1부터 다시 세는 마지막 묶음(위에 남은 보낸
/// 메시지의 「❯ 1. …」나 설명 안 번호 줄은 빠진다)
fn menu_rows(lines: &[String], start: usize, end: usize) -> Vec<Row> {
    let rows = numbered_rows(lines, start, end);
    let column = |r: &Row| lines[r.line].chars().position(|c| c.is_ascii_digit());
    let anchor = rows.iter().rev().find(|r| r.selected).or(rows.last()).map(column);
    let menu: Vec<Row> = rows.iter().filter(|r| anchor.is_none_or(|a| column(r) == a)).cloned().collect();
    let first = menu.iter().rposition(|r| r.number == 1).unwrap_or(0);
    menu[first..].to_vec()
}

fn sequential(rows: &[Row]) -> bool {
    !rows.is_empty() && rows.iter().enumerate().all(|(i, r)| r.number as usize == i + 1)
}

fn nearest_question(lines: &[String], before: usize) -> Option<String> {
    static SKIP: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^Planning:|^[←→].*Submit|^[☐☑✔]\s+\S|^Question \d+/\d+"));
    static SELECTED_COUNT: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^\(\d+\s+selected\)\s*"));
    (before.saturating_sub(14)..before).rev().map(|i| clean(&lines[i])).find_map(|l| {
        (!l.is_empty() && !is_divider(&l) && !SKIP.is_match(&l))
            .then(|| SELECTED_COUNT.replace(&l, "").trim().to_string())
    })
}

/// 질문 탭 막대: 그 줄, 잘리지 않았나, 탭마다 (이름, 답함)
struct Tabs {
    line: usize,
    whole: bool,
    tabs: Vec<(String, bool)>,
}

/// 선택지 위의 질문 탭 막대(막대 위 실선을 넘지 않는다)
fn claude_tabs(lines: &[String], before: usize) -> Option<Tabs> {
    for i in (before.saturating_sub(60)..before).rev() {
        let line = clean(&lines[i]);
        if SOLID_RULE.is_match(&line) {
            return None;
        }
        if !CLAUDE_TABS.is_match(&line) {
            continue;
        }
        let whole = line.ends_with('→');
        let inner = line.trim_start_matches('←').trim_end_matches('→');
        let mut tabs = Vec::new();
        let mut cur: Option<(char, String)> = None;
        for c in inner.chars() {
            if "☐☒☑✔".contains(c) {
                tabs.extend(cur.take());
                cur = Some((c, String::new()));
            } else if let Some((_, s)) = cur.as_mut() {
                s.push(c);
            }
        }
        tabs.extend(cur);
        let tabs = tabs.into_iter().filter(|(m, _)| *m != '✔').map(|(m, s)| (s.trim().to_string(), m != '☐')).collect();
        return Some(Tabs { line: i, whole, tabs });
    }
    None
}

/// 질문 하나의 머리 칩(`☐ Dataset`)
fn claude_chip(lines: &[String], first_row: usize) -> Option<String> {
    for i in (first_row.saturating_sub(40)..first_row).rev() {
        let line = clean(&lines[i]);
        if is_divider(&line) || CLAUDE_TABS.is_match(&line) {
            return None;
        }
        if let Some(m) = CHIP.captures(&line) {
            return Some(m[1].trim().to_string());
        }
    }
    None
}

/// 선택지 위의 질문(좁은 pane이 여러 줄로 접은 것을 다시 잇는다)
fn claude_question_text(lines: &[String], tabs: Option<usize>, first_row: usize) -> Option<String> {
    // 탭 막대 줄과 30줄 위는 넣지 않는다(참고 `index > max(tabsIndex, firstRow - 30)`)
    let floor = tabs.map_or(-1, |t| t as isize).max(first_row as isize - 30);
    let mut text: Vec<String> = Vec::new();
    for i in ((floor + 1).max(0) as usize..first_row).rev() {
        let line = clean(&lines[i]);
        if line.is_empty() {
            if !text.is_empty() {
                break;
            }
            continue;
        }
        if CHIP_LIKE.is_match(&line) || is_divider(&line) || CLAUDE_TABS.is_match(&line) {
            break;
        }
        text.insert(0, line);
    }
    (!text.is_empty()).then(|| normalize(&text.join(" ")))
}

fn finish(
    responder: Responder,
    mut menu: QuestionMenu,
    selected: usize,
    checked: Vec<usize>,
    custom_row: Option<usize>,
    reject_with_escape: Option<usize>,
) -> Parsed {
    // 자세한 글을 다 hash한 뒤 보일 길이로 줄인다: 명령·계획·설명이 바뀌면 다른 질문이다
    menu.id = String::new();
    let digest = Sha256::digest(serde_json::to_vec(&menu).unwrap());
    menu.id = hex::encode(&digest[..6]);
    if let Some(b) = menu.body.as_mut()
        && b.chars().count() > 12_000
    {
        *b = b.chars().take(12_000).collect();
    }
    Parsed { menu, responder, selected, checked, custom_row, reject_with_escape }
}

fn parse_claude_question(lines: &[String]) -> Option<Parsed> {
    let hint = find_last(lines, |i, _| CLAUDE_ASK_HINT.is_match(&wrapped(lines, i)))?;
    // 미리보기 상자가 있는 질문은 옮기지 않았다(참고 `withoutPreview`)
    if CLAUDE_PREVIEW_HINT.is_match(&wrapped(lines, hint)) {
        return None;
    }
    let rows = menu_rows(lines, hint.saturating_sub(64), hint);
    // 여러 개 고르기는 직접 쓰기 줄과 「Chat about this」 사이에 번호 없는 Submit 줄을 그린다(커서가 지난다)
    let submit_line = (rows.len() >= 2)
        .then(|| {
            lines[rows[rows.len() - 2].line + 1..rows[rows.len() - 1].line]
                .iter()
                .map(|l| clean(l))
                .find(|l| SUBMIT_ROW.is_match(l))
        })
        .flatten();
    let on_submit = submit_line.as_deref().is_some_and(|l| SELECTED.is_match(l));
    if !sequential(&rows) || rows.iter().filter(|r| r.selected).count() != usize::from(!on_submit) {
        return None;
    }
    let chat = rows.iter().position(|r| r.label == "Chat about this")?;
    let custom = rows.iter().position(|r| TYPE_SOMETHING.is_match(&r.label))?;
    if chat != rows.len() - 1 || custom + 1 != chat || custom < 1 {
        return None;
    }
    let tabs = claude_tabs(lines, rows[0].line);
    let question = claude_question_text(lines, tabs.as_ref().map(|t| t.line), rows[0].line)
        .or_else(|| nearest_question(lines, rows[0].line))?;
    let chip = if tabs.is_none() { claude_chip(lines, rows[0].line) } else { None };
    let options = &rows[..custom];
    let multi = options.iter().any(|r| MULTI_ROW.is_match(&lines[r.line]));
    if on_submit && !multi {
        return None;
    }
    let submit = multi && submit_line.is_some();
    let at = rows.iter().position(|r| r.selected);
    let selected = match at {
        None => rows.len() - 1,
        Some(a) if submit && a == rows.len() - 1 => a + 1,
        Some(a) => a,
    };
    let current = tabs.as_ref().and_then(|t| t.tabs.iter().position(|(_, answered)| !answered));
    // 좁은 pane이 자른 막대는 질문이 몇인지 모른다
    let title = match (&tabs, current) {
        (Some(t), Some(c)) if t.whole && t.tabs.len() > 1 => format!("{} · {}/{}", t.tabs[c].0, c + 1, t.tabs.len()),
        (Some(t), Some(c)) => t.tabs[c].0.clone(),
        _ => chip.unwrap_or_else(|| if multi { "여러 개 고르기".into() } else { "질문".into() }),
    };
    let menu = QuestionMenu {
        id: String::new(),
        kind: "question".into(),
        title,
        question,
        body: None,
        options: options
            .iter()
            .map(|r| MenuOption { label: r.label.clone(), description: r.description.clone() })
            .collect(),
        multi_select: multi,
        custom_option_index: (!multi).then_some(custom as u32),
    };
    let checked = options.iter().enumerate().filter(|(_, r)| r.checked).map(|(i, _)| i).collect();
    Some(finish(Responder::ClaudeQuestion, menu, selected, checked, Some(custom), None))
}

/// 여러 질문 뒤 Claude가 답을 보이고 보낼지 묻는 검토(탭부터 그 질문까지 구분선 없는 판 하나)
fn parse_claude_submit(lines: &[String]) -> Option<Parsed> {
    let q = find_last(lines, |_, l| SUBMIT_QUESTION.is_match(&clean(l)))?;
    let tabs = find_last(&lines[..q], |_, l| CLAUDE_TABS.is_match(&clean(l)));
    let panel = &lines[tabs.map_or(0, |t| t + 1)..q];
    if panel.iter().any(|l| is_divider(l)) {
        return None;
    }
    let rows = numbered_rows(lines, q + 1, lines.len());
    if !sequential(&rows) || rows.len() < 2 || rows.iter().filter(|r| r.selected).count() != 1 {
        return None;
    }
    if tabs.is_none()
        && (!panel.iter().any(|l| clean(l).starts_with("→ "))
            || rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>() != ["Submit answers", "Cancel"])
    {
        return None;
    }
    let body = panel
        .iter()
        .map(|l| clean(l))
        .filter(|l| !l.is_empty() && !is_divider(l) && !l.eq_ignore_ascii_case("Review your answers"))
        .collect::<Vec<_>>()
        .join("\n");
    let menu = QuestionMenu {
        id: String::new(),
        kind: "menu".into(),
        title: "답 검토".into(),
        question: clean(&lines[q]),
        body: (!body.is_empty()).then_some(body),
        options: rows.iter().map(|r| MenuOption { label: r.label.clone(), description: None }).collect(),
        multi_select: false,
        custom_option_index: None,
    };
    let selected = rows.iter().position(|r| r.selected).unwrap_or(0);
    Some(finish(Responder::ClaudeSubmit, menu, selected, Vec::new(), None, None))
}

fn parse_claude_approval(lines: &[String]) -> Option<Parsed> {
    if let Some(plan) = find_last(lines, |i, _| PLAN_QUESTION.is_match(&wrapped(lines, i))) {
        let rows = numbered_rows(lines, plan + 1, lines.len());
        if !sequential(&rows) || rows.len() < 3 || rows.iter().filter(|r| r.selected).count() != 1 {
            return None;
        }
        let custom = rows.iter().position(|r| TELL_CLAUDE.is_match(&r.label));
        let start = find_last(&lines[..plan], |_, l| clean(l).contains("Ready to code?")).unwrap_or(0);
        let body =
            lines[start..plan].iter().filter(|l| !is_divider(l)).map(|l| clean(l)).collect::<Vec<_>>().join("\n");
        let menu = QuestionMenu {
            id: String::new(),
            kind: "plan".into(),
            title: "Ready to code?".into(),
            question: clean(&lines[plan]),
            body: (!body.is_empty()).then_some(body),
            options: rows.iter().map(|r| MenuOption { label: r.label.clone(), description: None }).collect(),
            multi_select: false,
            custom_option_index: custom.map(|c| c as u32),
        };
        let selected = rows.iter().position(|r| r.selected).unwrap_or(0);
        return Some(finish(Responder::ClaudePlan, menu, selected, Vec::new(), custom, None));
    }
    let required = find_last(lines, |_, l| clean(l).contains("This command requires approval"));
    let dangerous = find_last(lines, |_, l| clean(l).starts_with("Dangerous rm operation"));
    let approval = required.max(dangerous);
    // 「Do you want to proceed?」「Do you want to create hello.txt?」「Do you want to make this edit to a.ts?」
    let q = find_last(lines, |_, l| DO_YOU_WANT.is_match(&clean(l)))?;
    // 선택지는 안내 줄에서 끝난다: 마지막 줄 아래 줄은 그 이름이 접힌 것이다
    let hint = find_last(lines, |i, _| ESC_TO_CANCEL.is_match(&wrapped(lines, i))).filter(|&h| h > q);
    let end = hint.unwrap_or(lines.len());
    let rows = numbered_rows(lines, q + 1, end);
    if !sequential(&rows) || rows.len() < 2 || rows.iter().filter(|r| r.selected).count() != 1 {
        return None;
    }
    // 마지막 줄 아래, 마지막 안내 위에 다른 안내가 있으면 이 판은 답했고 지금 끝은 다른 물음이다
    let last = rows[rows.len() - 1].line;
    if hint.is_some_and(|h| {
        ESC_TO_CANCEL.is_match(&lines[last + 1..h].iter().map(|l| clean(l)).collect::<Vec<_>>().join(" "))
    }) {
        return None;
    }
    let (title, body) = match approval {
        Some(a) if a < q => {
            let title = nearest_question(lines, a).unwrap_or_else(|| "명령 승인".into());
            let body_end = if dangerous > required { q } else { a };
            let body = lines[a.saturating_sub(8)..body_end]
                .iter()
                .map(|l| clean(l))
                .filter(|l| !l.is_empty() && !is_divider(l))
                .collect::<Vec<_>>()
                .join("\n");
            (title, body)
        }
        _ => {
            // Claude Code 2.1에는 표지가 없다: 도구 호출(`● Write(a.ts)`) 아래 첫 실선 밑 판이 도구 이름(「Bash command」)으로
            // 열리고 명령·설명이 온다. 호출이 화면 밖이면 가장 가까운 실선
            let call = find_last(&lines[..q], |_, l| TOOL_CALL.is_match(&clean(l)));
            let rules: Vec<usize> =
                (call.map_or(0, |c| c + 1)..q).filter(|&i| SOLID_RULE.is_match(&clean(&lines[i]))).collect();
            let rule = if call.is_some() { rules.first() } else { rules.last() }.copied();
            // 긴 명령이 판의 실선·도구 이름을 화면 위로 밀어냈다: 번호 줄과 그 아래 안내, 위에 남은 판(`│` 명령 칸과 그
            // 아래 점선)이 승인임을 말한다
            let above: Vec<String> = lines[..q].iter().map(|l| l.trim().to_string()).collect();
            let dash = find_last(&above, |_, l| is_divider(l));
            let scrolled = rule.is_none()
                && call.is_none()
                && hint.is_some_and(|h| clean(&lines[h]).to_ascii_lowercase().starts_with("esc to cancel"))
                && dash.is_some_and(|d| {
                    d >= 1
                        && q - d <= 8
                        && above[d].chars().count() >= 8
                        && above[d].chars().all(|c| c == '╌')
                        && above[d - 1].starts_with("│ ")
                        && above[d + 1..].iter().all(|l| !l.starts_with(['●', '⏺', '❯', '>', '›']))
                });
            if !scrolled && rule.is_none_or(|r| q - r > 60) {
                return None;
            }
            let from = if scrolled { 0 } else { rule.map_or(0, |r| r + 1) };
            let panel: Vec<String> = lines[from..q]
                .iter()
                .filter(|l| !l.trim().to_ascii_lowercase().starts_with("tip:"))
                .map(|l| clean(l))
                .filter(|l| !l.is_empty() && !is_divider(l))
                .collect();
            if panel.is_empty() {
                return None;
            }
            if scrolled {
                ("명령 승인".into(), panel.join("\n"))
            } else {
                (panel[0].clone(), panel[1..].join("\n"))
            }
        }
    };
    // 승인 선택지에는 설명이 없다: 아래 줄은 좁은 pane이 접은 이름이다
    let options = rows
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let next = rows.get(k + 1).map(|n| n.line).unwrap_or(end);
            let mut words = vec![r.label.clone()];
            words.extend(lines[r.line + 1..next].iter().map(|l| clean(l)).filter(|l| !l.is_empty() && !is_divider(l)));
            MenuOption { label: normalize(&words.join(" ")), description: None }
        })
        .collect();
    let menu = QuestionMenu {
        id: String::new(),
        kind: "approval".into(),
        title,
        question: clean(&lines[q]),
        body: (!body.is_empty()).then_some(body),
        options,
        multi_select: false,
        custom_option_index: None,
    };
    let selected = rows.iter().position(|r| r.selected).unwrap_or(0);
    Some(finish(Responder::ClaudeApproval, menu, selected, Vec::new(), None, None))
}

fn parse_codex_approval(lines: &[String]) -> Option<Parsed> {
    let h = find_last(lines, |_, l| CODEX_APPROVAL_HEADER.is_match(l) && !NUMBERED.is_match(l.trim()))?;
    let rows = numbered_rows(lines, h + 1, lines.len());
    if !sequential(&rows) || rows.len() < 2 || rows.iter().filter(|r| r.selected).count() != 1 {
        return None;
    }
    // 「Trust this folder? Codex can read, …」: 물음이 카드 머리, 설명은 본문으로
    let header = clean(&lines[h]);
    let (heading, rest) = match header.split_once("? ") {
        Some((a, b)) if !b.trim().is_empty() => (format!("{a}?"), b.trim().to_string()),
        _ => (header.clone(), String::new()),
    };
    let body = std::iter::once(rest)
        .chain(lines[h + 1..rows[0].line].iter().map(|l| clean(l)))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let menu = QuestionMenu {
        id: String::new(),
        kind: "approval".into(),
        title: heading.clone(),
        question: heading,
        body: (!body.is_empty()).then_some(body),
        options: rows.iter().map(|r| MenuOption { label: r.label.clone(), description: None }).collect(),
        multi_select: false,
        custom_option_index: None,
    };
    let selected = rows.iter().position(|r| r.selected).unwrap_or(0);
    let reject = rows.iter().position(|r| REJECT_ROW.is_match(&r.label));
    Some(finish(Responder::CodexApproval, menu, selected, Vec::new(), None, reject))
}

/// Claude가 창 아래 그리는 세션 구분선과 할 일 목록을 뗀다(그 위가 창의 끝이면)
fn without_claude_tasks(shown: &[String], hint_ends: impl Fn(&[String], usize) -> bool) -> Vec<String> {
    let mut end = shown.len();
    if let Some(head) = find_last(shown, |_, l| CLAUDE_TASKS_HEAD.is_match(l)) {
        let total: usize = CLAUDE_TASKS_HEAD.captures(&shown[head]).unwrap()[1].parse().unwrap_or(0);
        let (mut rows, mut in_progress, mut list) = (0, false, true);
        let mut i = head + 1;
        while i < shown.len() && list {
            let line = &shown[i];
            if CLAUDE_TASK_ROW.is_match(line) {
                rows += 1;
                in_progress = line.starts_with('◼');
            } else if CLAUDE_TASKS_MORE.is_match(line) {
                list = i == shown.len() - 1;
            } else if in_progress && line.ends_with('…') && !line.starts_with(['❯', '>', '›', '●', '⏺']) {
                in_progress = false;
            } else {
                list = false;
            }
            i += 1;
        }
        if list && rows > 0 && rows <= total {
            end = head;
        }
    }
    if end > 0 && LABELED_RULE.is_match(&shown[end - 1]) {
        end -= 1;
    }
    if end < shown.len() && hint_ends(shown, end) { shown[..end].to_vec() } else { shown.to_vec() }
}

/// 그 창의 안내가 화면 끝에 있다(답한 뒤 아래로 출력이 이어진 옛 창은 아니다)
fn tail_active(p: &Parsed, lines: &[String]) -> bool {
    let visible: Vec<String> = lines.iter().map(|l| clean(l)).filter(|l| !l.is_empty() && !is_divider(l)).collect();
    let shown = match p.responder {
        Responder::ClaudeSubmit => without_claude_tasks(&visible, |l, end| {
            CLAUDE_SUBMIT_TAIL.is_match(l.get(end.wrapping_sub(1)).map_or("", |s| s))
        }),
        Responder::CodexApproval => visible,
        _ => without_claude_tasks(&visible, |l, end| {
            CLAUDE_HINT_TAIL.is_match(l.get(end.wrapping_sub(1)).map_or("", |s| s))
        }),
    };
    // 좁은 pane은 안내를 접는다: 앞 줄은 마지막 줄까지 이어지는 맞춤일 때만 센다
    let ends = |r: &Regex| {
        (1..=3usize).any(|span| {
            span <= shown.len()
                && r.is_match(&shown[shown.len() - span..].join(" "))
                && (span == 1 || !r.is_match(&shown[shown.len() - span..shown.len() - 1].join(" ")))
        })
    };
    match p.responder {
        Responder::ClaudeQuestion => ends(&CLAUDE_ASK_HINT),
        Responder::ClaudeSubmit => shown.last().is_some_and(|l| CLAUDE_SUBMIT_TAIL.is_match(l)),
        Responder::ClaudeApproval => ends(&CLAUDE_APPROVAL_TAIL),
        Responder::ClaudePlan => ends(&CLAUDE_PLAN_TAIL),
        Responder::CodexApproval => ends(&CODEX_APPROVAL_TAIL),
    }
}

/// 화면의 살아 있는 선택 창(없거나 모르는 창이면 None: 예·아니오로 답한다)
pub(crate) fn parse(lines: &[String]) -> Option<Parsed> {
    type Reader = fn(&[String]) -> Option<Parsed>;
    let readers: [Reader; 4] =
        [parse_claude_question, parse_claude_submit, parse_claude_approval, parse_codex_approval];
    readers.iter().filter_map(|r| r(lines)).find(|p| tail_active(p, lines))
}

fn moves(delta: isize) -> Vec<Step> {
    vec![if delta > 0 { Step::Down } else { Step::Up }; delta.unsigned_abs()]
}

/// 답 → 키 걸음(참고 `answerKeys`). 옮기기는 커서가 있는 줄에서 센다
pub(crate) fn answer_steps(p: &Parsed, pick: &MenuPick) -> Result<Vec<Step>, String> {
    let menu = &p.menu;
    let from = p.selected as isize;
    match pick {
        MenuPick::Custom { text } => {
            let text = text.trim();
            if text.is_empty() {
                return Err("직접 쓴 답이 비었다".into());
            }
            // 줄바꿈·Esc 같은 조종 글자는 그 창의 키가 된다: 한 줄 글만 받는다
            if text.chars().any(char::is_control) {
                return Err("직접 쓴 답은 한 줄 글이어야 한다".into());
            }
            let row = match p.custom_row {
                Some(r) if !menu.multi_select && menu.custom_option_index.is_some() => r,
                _ => return Err("이 질문은 직접 쓴 답을 받지 않는다".into()),
            };
            // Claude 질문·계획은 그 줄에 커서가 오면 바로 글을 받는다(Enter를 먼저 누르지 않는다)
            let mut steps = moves(row as isize - from);
            steps.push(Step::Text(text.to_string()));
            steps.push(Step::Key(if p.responder == Responder::ClaudePlan { BACKTAB } else { ENTER }));
            Ok(steps)
        }
        MenuPick::Options { indices } => {
            if !menu.multi_select || indices.is_empty() {
                return Err("이 질문은 여러 개를 고르지 않는다".into());
            }
            if indices.iter().any(|&i| i as usize >= menu.options.len()) {
                return Err("보이는 선택지 밖이다".into());
            }
            let mut steps = Vec::new();
            let mut cursor = from;
            for i in 0..menu.options.len() {
                if indices.contains(&(i as u32)) != p.checked.contains(&i) {
                    steps.extend(moves(i as isize - cursor));
                    steps.push(Step::Key(ENTER));
                    cursor = i as isize;
                }
            }
            // →로 다음 질문이나 검토로 간다(Submit을 누르지 않는다: 다음 질문에서 Enter는 첫 선택지를 고른다)
            steps.push(Step::Key(RIGHT));
            Ok(steps)
        }
        MenuPick::Option { index } => {
            let i = *index as usize;
            if i >= menu.options.len() || menu.custom_option_index == Some(*index) || menu.multi_select {
                return Err("고를 수 있는 선택지가 아니다".into());
            }
            if p.reject_with_escape == Some(i) {
                return Ok(vec![Step::Key(ESC)]);
            }
            let mut steps = moves(i as isize - from);
            steps.push(Step::Key(ENTER));
            Ok(steps)
        }
    }
}

/// 답을 사람이 읽을 글로(질문 기록·감사)
pub(crate) fn describe(menu: &QuestionMenu, pick: &MenuPick) -> String {
    let label = |i: u32| menu.options.get(i as usize).map(|o| format!("{}. {}", i + 1, o.label)).unwrap_or_default();
    match pick {
        MenuPick::Option { index } => label(*index),
        MenuPick::Options { indices } => indices.iter().map(|&i| label(i)).collect::<Vec<_>>().join(", "),
        MenuPick::Custom { text } => format!("직접: {}", text.trim()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(s: &str) -> Vec<String> {
        s.lines().map(str::to_string).collect()
    }

    // 합성 화면(실제 기록을 옮기지 않음): Claude Code 2.1의 질문 하나, 칩·설명·추천·직접 쓰기·「Chat about this」
    const ONE: &str = "● 어떤 저장소를 쓸까?
────────────────────────────────────────
 ☐ 저장소

 결과를 어디에 둘까요?

 ❯ 1. SQLite (Recommended)
      한 파일, 설치 없음
   2. Postgres
      서버가 필요하다
   3. 파일 몇 개
   4. Type something.
────────────────────────────────────────
   5. Chat about this

 Enter to select · ↑/↓ to navigate · Esc to cancel";

    #[test]
    fn one_question_with_descriptions_and_a_typed_answer() {
        let p = parse(&screen(ONE)).expect("질문");
        let m = &p.menu;
        assert_eq!(
            (m.kind.as_str(), m.title.as_str(), m.question.as_str()),
            ("question", "저장소", "결과를 어디에 둘까요?")
        );
        let labels: Vec<_> = m.options.iter().map(|o| (o.label.as_str(), o.description.as_deref())).collect();
        assert_eq!(
            labels,
            [
                ("SQLite (Recommended)", Some("한 파일, 설치 없음")),
                ("Postgres", Some("서버가 필요하다")),
                ("파일 몇 개", None)
            ]
        );
        assert_eq!((m.multi_select, m.custom_option_index, p.selected), (false, Some(3), 0));
        let pick = |p: &Parsed, k| answer_steps(p, &k).unwrap();
        assert_eq!(pick(&p, MenuPick::Option { index: 2 }), [Step::Down, Step::Down, Step::Key(ENTER)]);
        assert_eq!(
            pick(&p, MenuPick::Custom { text: " 둘 다 ".into() }),
            [Step::Down, Step::Down, Step::Down, Step::Text("둘 다".into()), Step::Key(ENTER)]
        );
        // 직접 쓰기 줄은 고르는 선택지가 아니고, 조종 글자는 키가 되므로 받지 않는다
        assert!(answer_steps(&p, &MenuPick::Option { index: 3 }).is_err());
        assert!(answer_steps(&p, &MenuPick::Custom { text: "a\nb".into() }).is_err());
        assert!(answer_steps(&p, &MenuPick::Custom { text: "a\x1b[B".into() }).is_err());
        assert!(answer_steps(&p, &MenuPick::Options { indices: vec![0] }).is_err());
        // 커서가 옮겨도 같은 질문이고, 옮기기는 지금 커서에서 센다
        let moved = ONE.replace(" ❯ 1. SQLite", "   1. SQLite").replace("   3. 파일", " ❯ 3. 파일");
        let q = parse(&screen(&moved)).unwrap();
        assert_eq!((q.menu.id == p.menu.id, q.selected), (true, 2));
        assert_eq!(pick(&q, MenuPick::Option { index: 0 }), [Step::Up, Step::Up, Step::Key(ENTER)]);
        // 선택지가 바뀌면 다른 질문이다
        assert_ne!(parse(&screen(&ONE.replace("Postgres", "MySQL"))).unwrap().menu.id, p.menu.id);
    }

    #[test]
    fn an_answered_menu_under_later_output_is_not_asking() {
        assert!(parse(&screen(&format!("{ONE}\n\n⏺ 고른 것: SQLite\n\n❯ "))).is_none());
        // 할 일 목록과 세션 구분선이 창 아래 있어도 살아 있다
        let tasks = format!("{ONE}\n──── claude ─\n  2 tasks (1 done, 1 open)\n  ✔ 읽기\n  ◻ 쓰기");
        assert!(parse(&screen(&tasks)).is_some());
    }

    // 여러 질문의 둘째(탭 막대), 여러 개 고르기: 체크 상자, 직접 쓰기 아래 번호 없는 Submit
    const MULTI: &str = "←  ☒ 범위  ☐ 대상  ✔ Submit  →

 어디를 시험할까요?

 ❯ 1. [ ] 웹
   2. [✔] TUI
   3. [ ] CLI
   4. [ ] Type something
     Submit
────────────────────────────────────────
   5. Chat about this

 Enter to select · Tab/Arrow keys to navigate · Esc to cancel";

    #[test]
    fn several_questions_tick_the_changes_then_move_on() {
        let p = parse(&screen(MULTI)).expect("여러 개");
        let m = &p.menu;
        assert_eq!((m.title.as_str(), m.question.as_str(), m.multi_select), ("대상 · 2/2", "어디를 시험할까요?", true));
        assert_eq!((m.options.len(), m.custom_option_index, p.checked.clone()), (3, None, vec![1]));
        // 웹·CLI를 켜고 TUI를 끈다: 바뀌는 줄마다 옮기고 Enter, 끝은 →(Submit을 누르지 않는다)
        let steps = answer_steps(&p, &MenuPick::Options { indices: vec![0, 2] }).unwrap();
        assert_eq!(
            steps,
            [Step::Key(ENTER), Step::Down, Step::Key(ENTER), Step::Down, Step::Key(ENTER), Step::Key(RIGHT)]
        );
        assert!(answer_steps(&p, &MenuPick::Option { index: 0 }).is_err());
        assert!(answer_steps(&p, &MenuPick::Options { indices: vec![3] }).is_err());
        // 커서가 Submit 줄에 있으면 커서는 직접 쓰기 다음 자리다
        let on_submit = MULTI.replace(" ❯ 1. [ ] 웹", "   1. [ ] 웹").replace("     Submit", "   ❯ Submit");
        assert_eq!(parse(&screen(&on_submit)).unwrap().selected, 4);
    }

    #[test]
    fn the_review_and_approvals() {
        let review = "←  ☒ 범위  ☒ 대상  ✔ Submit  →

 Review your answers

  ● 어디를 시험할까요?
    → 웹, CLI

 Ready to submit your answers?

 ❯ 1. Submit answers
   2. Cancel";
        let p = parse(&screen(review)).expect("검토");
        assert_eq!((p.menu.kind.as_str(), p.menu.body.as_deref()), ("menu", Some("● 어디를 시험할까요?\n→ 웹, CLI")));
        assert_eq!(answer_steps(&p, &MenuPick::Option { index: 0 }).unwrap(), [Step::Key(ENTER)]);

        let bash = "● Bash(make test)
────────────────────────────────────────
 Bash command

   make test
   Run the tests

 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and don't ask again for make
      commands in this folder
   3. No, and tell Claude what to do differently (esc)

 Esc to cancel · Tab to amend · ctrl+e to explain";
        let p = parse(&screen(bash)).expect("승인");
        let m = &p.menu;
        assert_eq!(
            (m.kind.as_str(), m.title.as_str(), m.body.as_deref()),
            ("approval", "Bash command", Some("make test\nRun the tests"))
        );
        assert_eq!(m.options[1].label, "Yes, and don't ask again for make commands in this folder");
        assert_eq!(
            answer_steps(&p, &MenuPick::Option { index: 2 }).unwrap(),
            [Step::Down, Step::Down, Step::Key(ENTER)]
        );

        let codex = "  Would you like to run the following command?

  $ cargo test

› 1. Yes, proceed (y)
  2. Yes, and don't ask again for this command (a)
  3. No, and tell Codex what to do differently (esc)

  Press enter to confirm or esc to cancel";
        let p = parse(&screen(codex)).expect("Codex 승인");
        assert_eq!(
            (p.menu.title.as_str(), p.menu.body.as_deref()),
            ("Would you like to run the following command?", Some("$ cargo test"))
        );
        // 거절 줄은 Esc 하나다
        assert_eq!(answer_steps(&p, &MenuPick::Option { index: 2 }).unwrap(), [Step::Key(ESC)]);

        let plan = " Ready to code?

 Here is Claude's plan:
 1단계 읽기, 2단계 고치기

 Claude has written up a plan and is ready to execute. Would you like to proceed?

 ❯ 1. Yes, and auto-accept edits
   2. Yes, and manually approve edits
   3. Tell Claude what to change

 ctrl+g to edit in Vim";
        let p = parse(&screen(plan)).expect("계획");
        assert_eq!((p.menu.kind.as_str(), p.menu.custom_option_index), ("plan", Some(2)));
        // 계획을 고쳐 달라는 글은 shift+tab으로 보낸다
        assert_eq!(
            answer_steps(&p, &MenuPick::Custom { text: "테스트 먼저".into() }).unwrap(),
            [Step::Down, Step::Down, Step::Text("테스트 먼저".into()), Step::Key(BACKTAB)]
        );
    }

    #[test]
    fn unknown_menus_stay_yes_or_no() {
        // 번호 없는 폴더 신뢰 창과 실선 없는 확인창은 읽지 않는다(예·아니오 길)
        let trust = " Quick safety check: Do you trust the files in this folder?\n\n ❯ No, exit\n   Yes, I trust this folder\n\n Enter to confirm · Esc to cancel";
        assert!(parse(&screen(trust)).is_none());
        let bare =
            " Bash command\n\n   ls\n\n Do you want to proceed?\n ❯ 1. Yes\n   2. No\n\n Esc to cancel · Tab to amend";
        assert!(parse(&screen(bare)).is_none());
    }
}
