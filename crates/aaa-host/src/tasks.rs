//! TASKS.md 읽기(0.2.35 의미 그대로, 부록 A)와 쓰는 꼴(할 일 편집 T2, plan §8.311). 고치기는 `task_edit.rs`가 한다.
//!
//! - task 줄: `- [ ] 제목 (owner) [저장소] · t-0001`. `- [x]`도 task다(완료는 줄을 지워 표시한다).
//! - 저장소 칸(plan §8.330): owner 뒤 대괄호에 `project.toml` repos 경로의 마지막 이름(`[aaa]`)이나 `[작업 폴더]`(저장소 없는 작업
//!   폴더)를 쓴다. 비면 정하지 않은 것이다. 이름·`작업 폴더` 꼴이 아닌 끝 대괄호는 제목 글이다.
//! - owner: 없음·`agent`는 미할당, `me`는 사용자, `profile`·`@machine`·`profile@machine`은 agent, 그 밖의 글은 사람.
//!   옛 표기 `(agent → t-0007)`도 읽는다.
//! - 메모: task 줄보다 깊게 들여쓴 줄(사이의 빈 줄 허용).
//! - 형식 어긋남(할 일 보기 T1, plan §8.306): 끝 괄호에 상태 메모를 쓴 줄(`(t-0001 S4 뒤, 사용자 지시 시)`)을 사람 owner로
//!   오독하지 않는다. owner 꼴이 아니면 owner로 쓰지 않고 괄호째 제목에 두며 `problems`에 까닭을 단다. 판단하지 않고 원문을 보인다.
//! - 쓰는 꼴(정규형): `- [ ] 제목 (owner) [저장소] · t-0001`. 저장 때 제목을 검사하고(`check_title`), 쓴 줄을 다시 읽어 같은 값인지 본다.
//!   형식 어긋남 task에는 「정리 제안」(`suggest`)을 단다: 사람이 보고 「이대로 저장」해야 바뀐다.

use aaa_proto::{TaskListView, TaskNotesSection, TaskSuggestion, TaskView};
use sha2::Digest as _;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Owner {
    Unassigned,
    Me,
    Agent { profile: Option<String>, machine: Option<String> },
    Person(String),
}

fn name_like(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('-') && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

impl Owner {
    /// `profiles`에 없는 맨 이름은 사람이다(0.2.35 `is_person`)
    pub(crate) fn parse(raw: Option<&str>, profiles: &[String]) -> Owner {
        let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else { return Owner::Unassigned };
        match s {
            "agent" => return Owner::Unassigned,
            "me" => return Owner::Me,
            _ => {}
        }
        if let Some((p, m)) = s.split_once('@') {
            if (p.is_empty() || name_like(p)) && name_like(m) {
                return Owner::Agent { profile: (!p.is_empty()).then(|| p.to_string()), machine: Some(m.to_string()) };
            }
        } else if name_like(s) && profiles.iter().any(|p| p == s) {
            return Owner::Agent { profile: Some(s.to_string()), machine: None };
        }
        Owner::Person(s.to_string())
    }
}

/// owner 칸이 owner로 읽히지 않은 task(`--from-task`가 이 까닭으로 거부한다)
pub(crate) const OWNER_SHAPE: &str = "owner 칸이 owner 꼴이 아님";
/// 이 길이를 넘는 owner 칸은 상태 메모다
const OWNER_MAX_CHARS: usize = 40;
/// 이 길이를 넘는 제목은 메모로 옮길 후보다
const TITLE_MAX_CHARS: usize = 120;
/// 저장소 칸의 「저장소 없는 작업 폴더」
pub(crate) const REPO_DIR: &str = "작업 폴더";
/// 「나중」 파일은 이만큼까지 읽는다
pub(crate) const NOTES_MAX_BYTES: u64 = 512 * 1024;

pub(crate) fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

pub(crate) fn is_task(l: &str) -> bool {
    let t = l.trim_start();
    t.starts_with("- [") && t.get(4..5) == Some("]")
}

/// 글 안 `t-` + 숫자 `digits`자리 이상(앞 글자가 낱말 글자가 아닐 때)을 차례로
fn thread_numbers(s: &str, digits: usize) -> impl Iterator<Item = &str> {
    s.match_indices("t-").filter_map(move |(at, _)| {
        let before = s[..at].chars().next_back();
        if before.is_some_and(|c| c.is_alphanumeric()) {
            return None;
        }
        let n = s[at + 2..].chars().take_while(char::is_ascii_digit).count();
        (n >= digits).then(|| &s[at..at + 2 + n])
    })
}

/// 처음 찾은 thread 번호
fn thread_number(s: &str, digits: usize) -> Option<&str> {
    thread_numbers(s, digits).next()
}

/// 원문 앞부분(문제 글에 싣는다)
fn head(s: &str) -> String {
    let mut out: String = s.chars().take(30).collect();
    if s.chars().count() > 30 {
        out.push('…');
    }
    out
}

/// owner 칸 글이 owner 꼴인가. 아니면 까닭(problems 한 줄)
fn owner_shape(o: &str) -> Result<(), String> {
    let bad = o.contains(['(', ')', ',', '·']) || thread_number(o, 1).is_some() || o.chars().count() > OWNER_MAX_CHARS;
    if bad { Err(format!("{OWNER_SHAPE}: {}", head(o))) } else { Ok(()) }
}

/// 저장소 칸 값 꼴인가: 저장소 이름이나 `작업 폴더`
pub(crate) fn repo_like(s: &str) -> bool {
    s == REPO_DIR || name_like(s)
}

/// 끝 대괄호 칸 `… [이름]`의 (앞 글, 이름). 이름 꼴이 아니면 없음(제목 글이다)
fn repo_tail(s: &str) -> Option<(&str, &str)> {
    let inner = s.strip_suffix(']')?;
    let open = inner.rfind(" [")?;
    let name = &inner[open + 2..];
    repo_like(name).then(|| (inner[..open].trim_end(), name))
}

/// 읽은 task 줄 본문
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Line {
    pub title: String,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub thread: Option<String>,
    pub problems: Vec<String>,
}

/// 줄 본문 → 제목·owner·저장소·thread·형식 어긋남
pub(crate) fn split_line(text: &str) -> Line {
    let mut rest = text.trim().to_string();
    let mut thread = None;
    let mut problems = Vec::new();
    if let Some((head, tail)) = rest.rsplit_once('·') {
        let t = tail.trim();
        if t.starts_with("t-") && t.len() > 2 && t[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            thread = Some(t.to_string());
            rest = head.trim().to_string();
        }
    }
    let mut repo = None;
    if let Some((front, name)) = repo_tail(&rest) {
        repo = Some(name.to_string());
        rest = front.to_string();
    }
    let mut owner = None;
    if rest.ends_with(')')
        && let Some(open) = rest.rfind('(')
    {
        let inner = rest[open + 1..rest.len() - 1].trim().to_string();
        // 옛 표기: (agent → t-0007)
        let (o, arrow) = match inner.split_once('→') {
            Some((o, t)) => (o.trim().to_string(), Some(t.trim().to_string())),
            None => (inner, None),
        };
        match owner_shape(&o) {
            // 괄호째 제목에 둔다(원문을 그대로 보인다)
            Err(why) => problems.push(why),
            Ok(()) => {
                rest = rest[..open].trim().to_string();
                if arrow.is_some() {
                    thread = thread.or(arrow);
                }
                match o.as_str() {
                    "미할당" | "없음" => problems.push(format!("owner `{o}`는 미할당으로 읽음(정규형은 비움)")),
                    _ => owner = Some(o),
                }
            }
        }
    }
    if thread.is_none()
        && let Some(n) = thread_number(&rest, 4)
    {
        problems.push(format!("제목 안에 thread 번호가 묻힘: {n}"));
    }
    if rest.chars().count() > TITLE_MAX_CHARS {
        problems.push("제목이 김(메모로 옮길 후보)".into());
    }
    Line { title: rest, owner, repo, thread, problems }
}

/// 그 이름의 목록 자리(없으면 더한다)
fn at(lists: &mut Vec<TaskListView>, name: &str) -> usize {
    match lists.iter().position(|l| l.name == name) {
        Some(i) => i,
        None => {
            lists.push(TaskListView { name: name.to_string(), notes: Vec::new() });
            lists.len() - 1
        }
    }
}

pub(crate) fn sha256(s: &str) -> String {
    hex::encode(sha2::Sha256::digest(s.as_bytes()))
}

/// TASKS.md → (목록, task). 목록은 `##` 차례이고 task가 없는 목록도 든다. 같은 이름의 `##`는 하나로 본다.
pub(crate) fn parse_board(text: &str) -> (Vec<TaskListView>, Vec<TaskView>) {
    let lines: Vec<&str> = text.lines().collect();
    let mut lists: Vec<TaskListView> = Vec::new();
    let mut out = Vec::new();
    let mut list = String::new();
    // 상위 task 차례(들여쓰기, 줄 번호): 제목·목록 설명 줄에서 끊긴다
    let mut stack: Vec<(usize, u32)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if let Some(h) = l.strip_prefix("## ") {
            list = h.trim().to_string();
            at(&mut lists, &list);
            stack.clear();
            i += 1;
            continue;
        }
        if !is_task(l) {
            let t = l.trim();
            if t.starts_with('#') {
                stack.clear();
            } else if !t.is_empty() && indent(l) == 0 {
                // 목록 설명(체크박스 없는 줄)
                let k = at(&mut lists, &list);
                lists[k].notes.push(t.to_string());
                stack.clear();
            }
            i += 1;
            continue;
        }
        let t = l.trim_start();
        let done = matches!(&t[3..4], "x" | "X");
        let Line { title, owner, repo, thread, problems } = split_line(&t[5..]);
        let base = indent(l);
        let line = i as u32 + 1;
        while stack.last().is_some_and(|(d, _)| *d >= base) {
            stack.pop();
        }
        let parent = stack.last().map(|(_, n)| *n);
        stack.push((base, line));
        let mut notes = Vec::new();
        let mut end = i;
        let mut j = i + 1;
        while j < lines.len() {
            let n = lines[j];
            if n.trim().is_empty() {
                j += 1;
                continue;
            }
            if indent(n) <= base || is_task(n) || n.trim_start().starts_with('#') {
                break;
            }
            notes.push(n.trim().to_string());
            end = j;
            j += 1;
        }
        at(&mut lists, &list);
        out.push(TaskView {
            list: list.clone(),
            title,
            owner,
            repo,
            thread,
            notes,
            done,
            line,
            raw: t.to_string(),
            block_sha256: sha256(&lines[i..=end].join("\n")),
            parent,
            problems,
            suggestion: None,
            attention: None,
        });
        // 멈춘 줄(다음 task·제목, 들여쓴 하위 task 포함)부터 다시 본다
        i = j;
    }
    (lists, out)
}

pub(crate) fn parse(text: &str) -> Vec<TaskView> {
    parse_board(text).1
}

/// 저장할 제목(앞뒤 공백을 뗀 것). 다시 읽을 때 owner·thread로 오독되는 꼴을 거부한다(t-0469 §2.2)
pub(crate) fn check_title(title: &str) -> Result<String, String> {
    let t = title.trim();
    if t.is_empty() {
        return Err("제목이 비었다".into());
    }
    if t.contains(['\n', '\r']) {
        return Err("제목은 한 줄이다(나머지는 메모에 쓴다)".into());
    }
    if t.ends_with(')') {
        return Err("제목이 `)`로 끝나면 owner로 읽힌다: 끝 괄호 글은 메모로 옮긴다".into());
    }
    if let Some((_, tail)) = t.rsplit_once('·')
        && tail.trim_start().starts_with("t-")
    {
        return Err("제목 끝의 `· t-…`는 thread로 읽힌다: 메모로 옮긴다".into());
    }
    if is_task(t) {
        return Err("제목이 `[ ]`로 시작하면 안 된다".into());
    }
    Ok(t.to_string())
}

/// thread 칸 값(`t-` + 영문·숫자·`-`)
pub(crate) fn check_thread(id: &str) -> Result<String, String> {
    let t = id.trim();
    if t.len() > 2 && t.starts_with("t-") && t[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        Ok(t.to_string())
    } else {
        Err(format!("thread 번호 꼴이 아니다: {t}"))
    }
}

/// task 줄의 칸들(제목 말고). 빈 글은 칸이 없는 것이다
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Fields<'a> {
    pub owner: Option<&'a str>,
    pub repo: Option<&'a str>,
    pub thread: Option<&'a str>,
}

impl<'a> Fields<'a> {
    fn get(v: Option<&'a str>) -> Option<&'a str> {
        v.filter(|x| !x.is_empty())
    }
}

/// 정규형 task 줄(앞 들여쓰기 `ws`, 줄 끝 문자 없이): `- [ ] 제목 (owner) [저장소] · t-0001`
pub(crate) fn task_line(ws: &str, done: char, title: &str, f: Fields) -> String {
    let owner = Fields::get(f.owner).map(|o| format!(" ({o})")).unwrap_or_default();
    let repo = Fields::get(f.repo).map(|r| format!(" [{r}]")).unwrap_or_default();
    let thread = Fields::get(f.thread).map(|t| format!(" · {t}")).unwrap_or_default();
    format!("{ws}- [{done}] {title}{owner}{repo}{thread}")
}

/// 쓴 줄을 다시 읽은 값이 뜻한 값과 같은가(제목·owner·저장소·thread). 다르면 까닭
pub(crate) fn reads_back(line: &str, title: &str, f: Fields) -> Result<(), String> {
    let t = line.trim_start();
    let Line { title: rt, owner: ro, repo: rr, thread: rth, problems } = split_line(&t[5..]);
    // 제목 끝 `[이름]`은 owner·저장소 칸이 뒤에 오면 제목으로 읽힌다. 칸이 없으면 저장소로 읽히므로 그때만 거부한다
    if rr.as_deref() != Fields::get(f.repo) && repo_tail(title).is_some() {
        return Err(format!(
            "제목 끝 `{}`가 저장소 칸으로 읽힌다: 끝 대괄호 글은 메모로 옮긴다",
            title.rsplit(' ').next().unwrap_or(title)
        ));
    }
    if rt != title
        || ro.as_deref() != Fields::get(f.owner)
        || rr.as_deref() != Fields::get(f.repo)
        || rth.as_deref() != Fields::get(f.thread)
    {
        return Err(format!("`{t}`가 다른 값으로 다시 읽힌다(제목 `{rt}`)"));
    }
    if let Some(p) = problems.iter().find(|p| p.starts_with(OWNER_SHAPE)) {
        return Err(format!("`{t}`: {p}"));
    }
    Ok(())
}

/// owner 칸 글을 사람 이름으로 받아도 되나(「사람으로 저장」): owner 칸에 다시 그대로 읽히는 꼴만
pub(crate) fn check_person(name: &str) -> Result<(), String> {
    let n = name.trim();
    owner_shape(n)?;
    if matches!(n, "me" | "agent" | "미할당" | "없음") || n.contains('@') {
        return Err(format!("`{n}`는 사람 이름으로 쓰지 않는다"));
    }
    Ok(())
}

/// 마지막 괄호 묶음(짝을 맞춘 것)의 여는 괄호 자리. `)`로 끝나지 않거나 짝이 없으면 없음
fn last_group(s: &str) -> Option<usize> {
    if !s.ends_with(')') {
        return None;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices().rev() {
        match c {
            ')' => depth += 1,
            '(' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// 「정리 제안」(t-0469 §2.2): 끝 괄호 글 → 메모, 본문 속 열린 thread 번호 → 연결, `미할당`·`없음` → owner 비움, 긴 제목 → 앞부분과
/// 메모. 형식 어긋남이 없거나 제안이 다시 저장 검사를 통과하지 못하면 없다. 자동으로 고치지 않는다.
pub(crate) fn suggest(t: &TaskView, open_threads: &[String]) -> Option<TaskSuggestion> {
    if t.problems.is_empty() {
        return None;
    }
    let (mut title, mut owner, mut thread) = (t.title.clone(), t.owner.clone(), t.thread.clone());
    let mut moved = Vec::new();
    let mut changes = Vec::new();
    // 끝 괄호 묶음을 차례로 메모로 옮긴다(남은 제목이 다시 `)`로 끝나면 owner로 읽히므로 그것도)
    while owner_misread(t)
        && let Some(open) = last_group(&title)
    {
        let inner = title[open + 1..title.len() - 1].trim().to_string();
        let front = title[..open].trim_end().to_string();
        if front.is_empty() || inner.is_empty() {
            break;
        }
        changes.push(format!("끝 괄호 글을 메모로: {}", head(&inner)));
        moved.insert(0, inner);
        title = front;
    }
    if t.problems.iter().any(|p| p.starts_with("owner `")) && owner.is_none() {
        changes.push("owner `미할당` → 비움(미할당)".into());
    }
    if thread.is_none()
        && let Some(n) = thread_numbers(&t.title, 4).find(|n| open_threads.iter().any(|o| o == n))
    {
        changes.push(format!("본문 속 열린 {n}을 연결"));
        thread = Some(n.to_string());
    }
    if title.chars().count() > TITLE_MAX_CHARS
        && let Some((front, tail)) = [" — ", " - ", ": ", ". "].iter().find_map(|sep| title.split_once(sep))
        && front.chars().count() >= 4
    {
        let (front, tail) = (front.trim().to_string(), tail.trim().to_string());
        changes.push(format!("긴 제목의 뒷부분을 메모로: {}", head(&tail)));
        moved.insert(0, tail);
        title = front;
    }
    if owner.as_deref().is_some_and(|o| owner_shape(o).is_err()) {
        owner = None;
    }
    if changes.is_empty() || check_title(&title).is_err() {
        return None;
    }
    let fields = Fields { owner: owner.as_deref(), repo: t.repo.as_deref(), thread: thread.as_deref() };
    let line = task_line("", ' ', &title, fields);
    reads_back(&line, &title, fields).ok()?;
    moved.extend(t.notes.iter().cloned());
    Some(TaskSuggestion { title, owner, thread, notes: moved, line, changes })
}

/// owner 칸을 owner로 읽지 못한 task인가
pub(crate) fn owner_misread(t: &TaskView) -> bool {
    t.problems.iter().any(|p| p.starts_with(OWNER_SHAPE))
}

/// 제목이 같은(대소문자 무시) task
pub(crate) fn find<'a>(tasks: &'a [TaskView], title: &str) -> Option<&'a TaskView> {
    let want = title.trim().to_lowercase();
    tasks.iter().find(|t| t.title.to_lowercase() == want)
}

/// `task_notes` 경로: 프로젝트 폴더 안 상대 경로만(절대 경로·`..`·빈 값 거부). symlink는 읽을 때 본다
pub(crate) fn check_notes_path(p: &str) -> Result<(), String> {
    use std::path::Component;
    let path = std::path::Path::new(p);
    if p.trim().is_empty() || p.contains('\0') {
        return Err("`task_notes` 경로가 비었다".into());
    }
    if path.is_absolute() || p.starts_with(['/', '\\']) || path.components().any(|c| matches!(c, Component::Prefix(_)))
    {
        return Err(format!("`{p}`: 프로젝트 폴더 안 상대 경로만 받는다"));
    }
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(format!("`{p}`: `..`는 받지 않는다"));
    }
    Ok(())
}

/// 「나중」 Markdown → `##` 절. 첫 `##` 앞의 `#` 제목 줄은 버리고, 절 앞뒤 빈 줄을 뗀다. 줄은 원문 그대로다.
pub(crate) fn parse_notes(text: &str) -> Vec<TaskNotesSection> {
    let mut out = vec![TaskNotesSection::default()];
    for l in text.lines() {
        if let Some(h) = l.strip_prefix("## ") {
            out.push(TaskNotesSection { name: h.trim().to_string(), lines: Vec::new() });
        } else if out.len() == 1 && l.starts_with("# ") {
            continue;
        } else {
            out.last_mut().unwrap().lines.push(l.trim_end().to_string());
        }
    }
    for s in &mut out {
        while s.lines.last().is_some_and(|l| l.is_empty()) {
            s.lines.pop();
        }
        let lead = s.lines.iter().take_while(|l| l.is_empty()).count();
        s.lines.drain(..lead);
    }
    if out[0].lines.is_empty() {
        out.remove(0);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_owners_notes() {
        let md = "# Tasks\n\n## Backlog\n- [ ] Fix (the) login (claude)\n  - 메모 하나\n\n  메모 둘\n- [ ] 배포 (me)\n\
                  - [x] Old (agent → t-0007)\n- [ ] 원격 (codex@mac-b) · t-0012\n  - [ ] 하위 task\n## Later\n- [ ] 사람 (Priya Rao)\n\
                  - [ ] 미할당\n# 끝\n";
        let t = parse(md);
        let v: Vec<_> =
            t.iter().map(|t| (t.list.as_str(), t.title.as_str(), t.owner.as_deref(), t.thread.as_deref())).collect();
        assert_eq!(
            v,
            [
                ("Backlog", "Fix (the) login", Some("claude"), None),
                ("Backlog", "배포", Some("me"), None),
                ("Backlog", "Old", Some("agent"), Some("t-0007")),
                ("Backlog", "원격", Some("codex@mac-b"), Some("t-0012")),
                ("Backlog", "하위 task", None, None),
                ("Later", "사람", Some("Priya Rao"), None),
                ("Later", "미할당", None, None),
            ]
        );
        assert_eq!(t[0].notes, ["- 메모 하나", "메모 둘"]);
        assert!(t[2].done && !t[0].done);
        assert_eq!(find(&t, "fix (THE) login").unwrap().owner.as_deref(), Some("claude"));
        let profiles = vec!["claude".to_string()];
        assert_eq!(
            Owner::parse(Some("claude"), &profiles),
            Owner::Agent { profile: Some("claude".into()), machine: None }
        );
        assert_eq!(
            Owner::parse(Some("@mac-b"), &profiles),
            Owner::Agent { profile: None, machine: Some("mac-b".into()) }
        );
        assert_eq!(Owner::parse(Some("Priya"), &profiles), Owner::Person("Priya".into()));
        assert_eq!(Owner::parse(Some("agent"), &profiles), Owner::Unassigned);
        assert_eq!(Owner::parse(Some("me"), &profiles), Owner::Me);
        assert_eq!(Owner::parse(Some("x@-bad"), &profiles), Owner::Person("x@-bad".into()));
        // 지금 꼴은 형식 어긋남이 없다
        assert!(t.iter().all(|t| t.problems.is_empty()), "{t:#?}");
    }

    /// 운영에서 나온 오독 꼴 셋(같은 꼴의 가짜 글): owner가 아니라 problems로 나온다
    #[test]
    fn misread_owner_shapes_become_problems() {
        let md = "## Backlog\n- [ ] 정리(선택) (t-0001 S4 뒤, 사용자 지시 시)\n- [ ] 실패 조사 (미할당)\n\
                  - [ ] 웹 화면 W1~W8(계획 · t-0002(opus-high), 이어서 W2~W8 차례로)\n- [ ] 사람 일 (Priya Rao)\n";
        let t = parse(md);
        let owners: Vec<_> = t.iter().map(|t| t.owner.as_deref()).collect();
        assert_eq!(owners, [None, None, None, Some("Priya Rao")]);
        assert_eq!(t[0].title, "정리(선택) (t-0001 S4 뒤, 사용자 지시 시)");
        assert_eq!(
            t[0].problems,
            ["owner 칸이 owner 꼴이 아님: t-0001 S4 뒤, 사용자 지시 시", "제목 안에 thread 번호가 묻힘: t-0001"]
        );
        assert!(owner_misread(&t[0]));
        assert_eq!((t[1].title.as_str(), t[1].owner.as_deref()), ("실패 조사", None));
        assert_eq!(t[1].problems, ["owner `미할당`는 미할당으로 읽음(정규형은 비움)"]);
        assert!(!owner_misread(&t[1]));
        assert_eq!(t[2].title, "웹 화면 W1~W8(계획 · t-0002(opus-high), 이어서 W2~W8 차례로)");
        assert_eq!(t[2].thread, None);
        assert!(owner_misread(&t[2]), "{:?}", t[2].problems);
        assert!(t[2].problems.iter().any(|p| p == "제목 안에 thread 번호가 묻힘: t-0002"), "{:?}", t[2].problems);
        assert!(t[3].problems.is_empty());
        // 긴 제목, 40자 넘는 owner 칸
        let long = format!("- [ ] {} (me)\n- [ ] 짧은 제목 ({})\n", "가".repeat(121), "나".repeat(41));
        let t = parse(&long);
        assert_eq!(t[0].problems, ["제목이 김(메모로 옮길 후보)"]);
        assert!(owner_misread(&t[1]) && t[1].owner.is_none());
        // 이어진 thread가 있으면 제목 안 번호는 문제로 보지 않는다(본문 속 다른 thread를 가리킬 수 있다)
        assert!(parse("- [ ] t-0003 뒤 정리 · t-0004\n")[0].problems.is_empty());
        // 낱말 안의 `t-`는 번호가 아니다
        assert!(parse("- [ ] dist-0001 빌드\n")[0].problems.is_empty());
    }

    /// 줄 번호·원문·상위 task·블록 hash·목록 설명
    #[test]
    fn lines_parents_blocks_and_list_notes() {
        let md = "# Tasks\n\n## Backlog\n나중 일은 later.md에 모은다\n\n- [ ] 상위 (me)\n  메모 A\n  - [ ] 하위 하나\n    하위 메모\n\
                  \n  - [ ] 하위 둘\n- [ ] 다음\n## 빈 목록\n## Later\n- [ ] 셋\n";
        let (lists, t) = parse_board(md);
        let names: Vec<_> = lists.iter().map(|l| (l.name.as_str(), l.notes.clone())).collect();
        assert_eq!(
            names,
            [("Backlog", vec!["나중 일은 later.md에 모은다".to_string()]), ("빈 목록", vec![]), ("Later", vec![]),]
        );
        let v: Vec<_> = t.iter().map(|t| (t.title.as_str(), t.line, t.parent)).collect();
        assert_eq!(
            v,
            [
                ("상위", 6, None),
                ("하위 하나", 8, Some(6)),
                ("하위 둘", 11, Some(6)),
                ("다음", 12, None),
                ("셋", 15, None)
            ]
        );
        assert_eq!(t[0].raw, "- [ ] 상위 (me)");
        assert_eq!(t[1].raw, "- [ ] 하위 하나");
        // 블록 = task 줄 + 메모(끝 빈 줄 제외). 하위 task 줄은 상위 블록에 들지 않는다
        assert_eq!(t[0].block_sha256, sha256("- [ ] 상위 (me)\n  메모 A"));
        assert_eq!(t[1].block_sha256, sha256("  - [ ] 하위 하나\n    하위 메모"));
        // 메모만 바뀌면 그 task hash만 바뀐다
        let (_, t2) = parse_board(&md.replace("하위 메모", "하위 메모 고침"));
        let changed: Vec<_> = t.iter().zip(&t2).map(|(a, b)| a.block_sha256 != b.block_sha256).collect();
        assert_eq!(changed, [false, true, false, false, false]);
        // 첫 `##` 앞 task는 이름 없는 목록이고, 줄 끝 문자는 hash에 들지 않는다
        let (lists, t) = parse_board("- [ ] 맨 앞\r\n  메모\r\n## A\n");
        assert_eq!(lists.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["", "A"]);
        assert_eq!(t[0].block_sha256, sha256("- [ ] 맨 앞\n  메모"));
    }

    /// 「정리 제안」: 운영 오독 꼴 셋이 정규형 후보가 되고, 그 후보는 다시 읽어도 같다
    #[test]
    fn suggestions_for_misread_lines() {
        let md = "## Backlog\n- [ ] 정리(선택) (t-0001 S4 뒤, 사용자 지시 시)\n  옛 메모\n- [ ] 실패 조사 (미할당)\n\
                  - [ ] 웹 화면 W1~W8(계획 · t-0002(opus-high), 이어서 W2~W8 차례로)\n- [ ] 사람 일 (Priya Rao)\n";
        let t = parse(md);
        let open = vec!["t-0002".to_string()];
        let s = suggest(&t[0], &open).unwrap();
        assert_eq!((s.title.as_str(), s.owner.as_deref(), s.thread.as_deref()), ("정리", None, None));
        assert_eq!(s.notes, ["선택", "t-0001 S4 뒤, 사용자 지시 시", "옛 메모"]);
        assert_eq!(s.line, "- [ ] 정리");
        let s = suggest(&t[1], &open).unwrap();
        assert_eq!((s.title.as_str(), s.line.as_str()), ("실패 조사", "- [ ] 실패 조사"));
        let s = suggest(&t[2], &open).unwrap();
        assert_eq!((s.title.as_str(), s.thread.as_deref()), ("웹 화면 W1~W8", Some("t-0002")));
        assert_eq!(s.notes, ["계획 · t-0002(opus-high), 이어서 W2~W8 차례로"]);
        assert_eq!(s.line, "- [ ] 웹 화면 W1~W8 · t-0002");
        assert!(suggest(&t[3], &open).is_none(), "형식 어긋남이 없으면 제안도 없다");
        // 열린 thread가 아니면 잇지 않는다
        assert_eq!(suggest(&t[2], &[]).unwrap().thread, None);
        // 제안한 줄은 다시 읽어도 같은 값이다
        for task in &t[..3] {
            let s = suggest(task, &open).unwrap();
            let back = &parse(&format!("{}\n", s.line))[0];
            assert_eq!((&back.title, &back.owner, &back.thread), (&s.title, &s.owner, &s.thread));
            assert!(back.problems.is_empty(), "{back:?}");
        }
    }

    /// 저장소 칸(plan §8.330): owner 뒤 대괄호. 이름·`작업 폴더` 꼴이 아닌 끝 대괄호는 제목 글이다
    #[test]
    fn repo_field_reads_and_writes() {
        let md = "## A\n- [ ] 고치기 (claude) [aaa] · t-0518\n- [ ] 정리 [작업 폴더]\n- [ ] 맡김 (@mac-b) [rc-unity]\n\
                  - [ ] 로그 보기 [긴급]\n- [ ] 순서 [aaa] (claude)\n- [ ] 빈 칸 (me)\n";
        let t = parse(md);
        let v: Vec<_> =
            t.iter().map(|t| (t.title.as_str(), t.owner.as_deref(), t.repo.as_deref(), t.thread.as_deref())).collect();
        assert_eq!(
            v,
            [
                ("고치기", Some("claude"), Some("aaa"), Some("t-0518")),
                ("정리", None, Some(REPO_DIR), None),
                ("맡김", Some("@mac-b"), Some("rc-unity"), None),
                ("로그 보기 [긴급]", None, None, None),
                ("순서 [aaa]", Some("claude"), None, None),
                ("빈 칸", Some("me"), None, None),
            ]
        );
        // 쓴 줄은 다시 읽어도 같은 값이다
        for f in [
            Fields { owner: Some("claude"), repo: Some("aaa"), thread: Some("t-0518") },
            Fields { owner: None, repo: Some(REPO_DIR), thread: None },
            Fields { owner: Some("codex@mac-b"), repo: None, thread: Some("t-1") },
        ] {
            let line = task_line("  ", ' ', "고치기", f);
            reads_back(&line, "고치기", f).unwrap();
        }
        assert_eq!(
            task_line("", ' ', "고치기", Fields { owner: Some("claude"), repo: Some("aaa"), thread: Some("t-0518") }),
            "- [ ] 고치기 (claude) [aaa] · t-0518"
        );
        // 제목 끝 `[이름]`은 뒤에 owner·저장소 칸이 오면 제목이고, 칸이 없으면 저장소로 읽혀 쓰지 않는다
        let v2 = Fields { owner: Some("claude"), ..Fields::default() };
        reads_back(&task_line("", ' ', "배포 [v2]", v2), "배포 [v2]", v2).unwrap();
        let both = Fields { repo: Some("aaa"), ..Fields::default() };
        reads_back(&task_line("", ' ', "배포 [v2]", both), "배포 [v2]", both).unwrap();
        let bare = Fields::default();
        let e = reads_back(&task_line("", ' ', "배포 [v2]", bare), "배포 [v2]", bare).unwrap_err();
        assert!(e.contains("`[v2]`가 저장소 칸으로 읽힌다"), "{e}");
        assert!(check_title("로그 보기 [긴급]").is_ok(), "저장소 꼴이 아닌 끝 대괄호는 제목이다");
        // 「정리 제안」은 저장소 칸을 그대로 둔다
        let s = suggest(&parse("- [ ] 실패 조사 (미할당) [aaa]\n")[0], &[]).unwrap();
        assert_eq!(s.line, "- [ ] 실패 조사 [aaa]");
    }

    #[test]
    fn notes_paths_and_sections() {
        for ok in ["memory/later.md", "later.md", "./a/b.md"] {
            assert!(check_notes_path(ok).is_ok(), "{ok}");
        }
        for (bad, why) in [
            ("../x.md", "`..`"),
            ("memory/../../x.md", "`..`"),
            ("/etc/passwd", "상대 경로"),
            ("\\\\server\\x", "상대 경로"),
            ("", "비었다"),
        ] {
            let e = check_notes_path(bad).unwrap_err();
            assert!(e.contains(why), "{bad}: {e}");
        }
        let s =
            parse_notes("# 나중 작업 모음\n\n소개 줄\n\n## UI\n- 하나 `code`\n\n- 둘\n\n## 빈 절\n\n## 밸런스\n- 셋\n");
        let v: Vec<_> = s.iter().map(|s| (s.name.as_str(), s.lines.clone())).collect();
        assert_eq!(
            v,
            [
                ("", vec!["소개 줄".to_string()]),
                ("UI", vec!["- 하나 `code`".into(), "".into(), "- 둘".into()]),
                ("빈 절", vec![]),
                ("밸런스", vec!["- 셋".into()]),
            ]
        );
        assert_eq!(parse_notes("## A\n- x\n").len(), 1);
    }
}
