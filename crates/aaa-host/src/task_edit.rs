//! 할 일 편집(T2, plan §8.311): TASKS.md 글에 동작을 적용하는 순수 함수. 파일·기록·권한은 `coord.rs`가 한다(t-0469 §2.2·§2.3).
//!
//! - 바꾼 task 블록만 새로 쓰고 그 밖의 줄(`#` 제목·목록 설명·빈 줄·모르는 줄)은 바이트 그대로 둔다. 줄 끝 문자(LF·CRLF)도 그대로다.
//!   새 줄은 파일의 첫 줄 끝 문자를 쓴다.
//! - 정규형 `- [ ] 제목 (owner) · t-NNNN`, 메모는 task 줄 들여쓰기 + 2칸. 칸이 바뀌지 않은 task 줄·메모 줄은 원문 그대로 둔다.
//! - 대상은 줄 번호가 아니라 블록 hash로 찾는다(앞 동작이 줄을 민다). 못 찾으면 충돌이다: 지운 task가 저장으로 되살아나지 않는다
//!   (t3code `requireExisting`과 같은 뜻). 다른 task가 그사이 바뀐 것은 충돌이 아니다.
//! - task를 지우거나 옮기면 하위 task와, 그 사이에 깊게 들여쓴 줄(어느 task의 메모도 아닌 줄)까지 함께 간다. 기록에는 그 묶음 전체를
//!   남겨 되살리면 그대로 돌아온다.

use aaa_proto::{TaskAction, TaskAnchor, TaskOp, TaskView};

use crate::tasks::{self, indent, is_task};

/// 기록 한 줄(coord.rs가 actor·시각을 붙여 DB에 쓴다)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Record {
    pub action: TaskAction,
    pub list: String,
    pub from_list: Option<String>,
    pub title: String,
    pub before: Option<String>,
    pub after: Option<String>,
    pub thread: Option<String>,
    /// 되살린 기록 항목
    pub restored: Option<i64>,
}

impl Record {
    fn new(action: TaskAction, list: &str, title: &str) -> Record {
        Record {
            action,
            list: list.to_string(),
            from_list: None,
            title: title.to_string(),
            before: None,
            after: None,
            thread: None,
            restored: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fail {
    /// 대상 task가 최신 파일에 없다(바뀌었거나 지워졌다). 아무것도 쓰지 않는다
    Conflict { op: usize, reason: String, title: String },
    /// 저장 검사에 걸렸다
    Bad(String),
}

/// 기록에서 되살릴 항목(삭제 기록의 목록·블록 원문)
pub(crate) struct Removed {
    pub list: String,
    pub text: String,
}

/// 동작이 묻는 바깥 판정: owner 검사(`assignable --check`, 사람이면 「사람으로 저장」), 저장소 칸 값(경로는 이름으로 바꾼다)과
/// 되살릴 기록. `profiles`는 host가 아는 profile 이름이다(모델 칸만 바꿀 때 지금 owner의 맨 이름이 모델인지 본다)
pub(crate) struct Ctx<'a> {
    pub owner_ok: &'a dyn Fn(&str, bool) -> Result<(), String>,
    pub profiles: &'a [String],
    pub repo_ok: &'a dyn Fn(&str) -> Result<String, String>,
    pub removed: &'a dyn Fn(i64) -> Result<Removed, String>,
}

#[derive(Debug)]
pub(crate) struct Applied {
    pub text: String,
    pub records: Vec<Record>,
}

/// 줄 끝 문자를 뗀 줄
fn plain(l: &str) -> &str {
    let l = l.strip_suffix('\n').unwrap_or(l);
    l.strip_suffix('\r').unwrap_or(l)
}

fn blank(l: &str) -> bool {
    plain(l).trim().is_empty()
}

fn leading_ws(l: &str) -> &str {
    let p = plain(l);
    &p[..indent(p)]
}

/// 한 task 줄의 자리: 블록 끝(task 줄 + 메모)과 하위 묶음 끝(하위 task·깊은 줄 포함, 끝 빈 줄 제외)
#[derive(Debug, Clone)]
struct Span {
    idx: usize,
    block_end: usize,
    sub_end: usize,
    view: TaskView,
}

impl Span {
    fn ws<'a>(&self, d: &'a Doc) -> &'a str {
        leading_ws(&d.lines[self.idx])
    }
}

/// `##` 목록 자리: 머리 줄(이름 없는 목록은 없음)과 그 뒤 줄 범위 [start, end)
#[derive(Debug, Clone)]
struct Region {
    name: String,
    head: Option<usize>,
    start: usize,
    end: usize,
}

struct Doc {
    /// 줄 끝 문자를 붙인 줄
    lines: Vec<String>,
    eol: &'static str,
}

impl Doc {
    fn new(text: &str) -> Doc {
        let lines: Vec<String> = text.split_inclusive('\n').map(str::to_string).collect();
        let eol = if lines.first().is_some_and(|l| l.ends_with("\r\n")) { "\r\n" } else { "\n" };
        Doc { lines, eol }
    }

    fn text(&self) -> String {
        self.lines.concat()
    }

    fn spans(&self) -> Vec<Span> {
        let (_, views) = tasks::parse_board(&self.text());
        let at: Vec<usize> = (0..self.lines.len()).filter(|&i| is_task(plain(&self.lines[i]))).collect();
        debug_assert_eq!(at.len(), views.len());
        at.into_iter()
            .zip(views)
            .map(|(idx, view)| {
                let base = indent(plain(&self.lines[idx]));
                let (mut block_end, mut sub_end) = (idx, idx);
                let mut in_block = true;
                for j in idx + 1..self.lines.len() {
                    let l = plain(&self.lines[j]);
                    if l.trim().is_empty() {
                        continue;
                    }
                    if indent(l) <= base || l.trim_start().starts_with('#') {
                        break;
                    }
                    if is_task(l) {
                        in_block = false;
                    }
                    if in_block {
                        block_end = j;
                    }
                    sub_end = j;
                }
                Span { idx, block_end, sub_end, view }
            })
            .collect()
    }

    fn regions(&self) -> Vec<Region> {
        let mut out = vec![Region { name: String::new(), head: None, start: 0, end: self.lines.len() }];
        for (i, l) in self.lines.iter().enumerate() {
            if let Some(h) = plain(l).strip_prefix("## ") {
                out.last_mut().unwrap().end = i;
                out.push(Region { name: h.trim().to_string(), head: Some(i), start: i + 1, end: self.lines.len() });
            }
        }
        out
    }

    fn has_list(&self, name: &str) -> bool {
        name.is_empty() || self.regions().iter().any(|r| r.head.is_some() && r.name == name)
    }

    /// 줄 범위를 원문(줄 끝 문자 없이 `\n`으로 이음)으로
    fn plain_text(&self, from: usize, to: usize) -> String {
        self.lines[from..=to].iter().map(|l| plain(l)).collect::<Vec<_>>().join("\n")
    }

    /// 마지막 줄에 줄 끝 문자가 없으면 붙인다(그 뒤에 줄을 더하기 전에)
    fn close_last(&mut self) {
        let eol = self.eol;
        if let Some(last) = self.lines.last_mut()
            && !last.ends_with('\n')
        {
            last.push_str(eol);
        }
    }

    fn insert(&mut self, at: usize, block: Vec<String>) {
        if at > 0 && !self.lines[at - 1].ends_with('\n') {
            let eol = self.eol;
            self.lines[at - 1].push_str(eol);
        }
        self.lines.splice(at..at, block);
    }

    /// [from, to] 줄을 뗀다. 앞뒤가 모두 빈 줄이면 빈 줄 하나도 함께 뗀다(빈 줄이 겹치지 않게)
    fn remove(&mut self, from: usize, to: usize) -> Vec<String> {
        let out: Vec<String> = self.lines.drain(from..=to).collect();
        let before_blank = from > 0 && blank(&self.lines[from - 1]);
        if before_blank && from < self.lines.len() && blank(&self.lines[from]) {
            self.lines.remove(from);
        } else if before_blank && from == self.lines.len() {
            self.lines.remove(from - 1);
        }
        out
    }

    /// 그 목록의 끝(마지막 같은 이름 머리의 마지막 글 줄 뒤). 없는 목록은 파일 끝에 머리를 만든다
    fn list_end(&mut self, name: &str) -> usize {
        if !self.has_list(name) {
            self.add_list_at_end(name);
        }
        let r =
            self.regions().into_iter().rev().find(|r| r.name == name && (r.head.is_some() || name.is_empty())).unwrap();
        (r.start..r.end).rev().find(|&i| !blank(&self.lines[i])).map_or(r.start, |i| i + 1)
    }

    fn add_list_at_end(&mut self, name: &str) {
        self.close_last();
        if self.lines.last().is_some_and(|l| !blank(l)) {
            self.lines.push(self.eol.to_string());
        }
        self.lines.push(format!("## {name}{}", self.eol));
    }

    fn with_eol(&self, lines: Vec<String>) -> Vec<String> {
        lines.into_iter().map(|l| format!("{l}{}", self.eol)).collect()
    }
}

/// 줄들의 들여쓰기를 바꾼다: 첫 줄의 앞 공백 `from`을 `to`로(그보다 깊은 줄은 같은 만큼)
fn reindent(lines: Vec<String>, to: &str) -> Vec<String> {
    let from = lines.first().map(|l| leading_ws(l).to_string()).unwrap_or_default();
    lines
        .into_iter()
        .map(|l| {
            if blank(&l) {
                return l;
            }
            match l.strip_prefix(from.as_str()) {
                Some(rest) => format!("{to}{rest}"),
                None => format!("{to}{}", l.trim_start_matches([' ', '\t'])),
            }
        })
        .collect()
}

/// 메모 글: 줄마다 끝 공백을 떼고 앞뒤 빈 줄을 뗀다. task 꼴·`#` 줄은 거부한다(다시 읽으면 하위 task·머리가 된다)
fn check_notes(notes: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = notes.iter().flat_map(|n| n.split('\n')).map(|l| l.trim_end().to_string()).collect();
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    let lead = out.iter().take_while(|l| l.is_empty()).count();
    out.drain(..lead);
    for l in &out {
        if is_task(l.trim_start()) {
            return Err(format!("메모 줄이 task 꼴이다(하위 task로 더한다): {l}"));
        }
        if l.trim_start().starts_with('#') {
            return Err(format!("메모 줄이 `#`로 시작하면 목록 머리로 읽힌다: {l}"));
        }
    }
    Ok(out)
}

/// 파서가 읽는 메모(앞뒤 공백을 뗀 빈 줄 아닌 줄)와 같은가
fn same_notes(new: &[String], old: &[String]) -> bool {
    let n: Vec<&str> = new.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    n == old.iter().map(String::as_str).collect::<Vec<_>>()
}

fn check_list_name(name: &str) -> Result<String, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("목록 이름이 비었다".into());
    }
    if n.contains(['\n', '\r']) {
        return Err("목록 이름은 한 줄이다".into());
    }
    Ok(n.to_string())
}

/// 동작 하나의 실패를 그 차례로
fn at_op(i: usize) -> impl Fn(String) -> Fail {
    move |reason| Fail::Bad(if i == 0 { reason } else { format!("{}번째 동작: {reason}", i + 1) })
}

/// 한 요청 안에서 앞 동작이 바꾼 블록의 hash(옛 → 새). 같은 task를 고친 뒤 옮기는 요청이 둘째 동작에서도 그 task를 찾는다
type Renames = Vec<(String, String)>;

fn renamed(a: &TaskAnchor, renames: &Renames) -> TaskAnchor {
    let mut a = a.clone();
    for _ in 0..renames.len() {
        match renames.iter().find(|(old, _)| *old == a.block_sha256) {
            Some((_, new)) if *new != a.block_sha256 => a.block_sha256 = new.clone(),
            _ => break,
        }
    }
    a
}

/// 대상 task를 찾는다: hash가 같은 블록(여럿이면 같은 목록, 그다음 `line`에 가까운 것), hash가 없으면 제목
fn find(spans: &[Span], a: &TaskAnchor, op: usize) -> Result<usize, Fail> {
    if a.block_sha256.is_empty() {
        let want = a.title.trim().to_lowercase();
        let same: Vec<usize> = (0..spans.len())
            .filter(|&i| {
                spans[i].view.id.as_deref() == Some(a.title.trim()) || spans[i].view.title.to_lowercase() == want
            })
            .collect();
        return match same.as_slice() {
            [one] => Ok(*one),
            [] => Err(Fail::Conflict { op, reason: format!("없는 task: {}", a.title), title: a.title.clone() }),
            _ => Err(Fail::Bad(format!("같은 제목 task가 {}개다: {}", same.len(), a.title))),
        };
    }
    (0..spans.len())
        .filter(|&i| spans[i].view.block_sha256 == a.block_sha256)
        .min_by_key(|&i| (spans[i].view.list != a.list, spans[i].view.line.abs_diff(a.line)))
        .ok_or_else(|| Fail::Conflict {
            op,
            reason: format!("「{}」 task가 그사이 바뀌었거나 지워졌다", a.title),
            title: a.title.clone(),
        })
}

/// 넣을 자리와 들여쓰기: `before` 앞, `parent`의 하위 끝, 목록 끝
fn target(
    d: &mut Doc,
    list: &str,
    before: Option<&TaskAnchor>,
    parent: Option<&TaskAnchor>,
    op: usize,
) -> Result<(usize, String, String), Fail> {
    let spans = d.spans();
    if let Some(b) = before {
        let s = &spans[find(&spans, b, op)?];
        return Ok((s.idx, s.ws(d).to_string(), s.view.list.clone()));
    }
    if let Some(p) = parent {
        let s = &spans[find(&spans, p, op)?];
        return Ok((s.sub_end + 1, format!("{}  ", s.ws(d)), s.view.list.clone()));
    }
    Ok((d.list_end(list), String::new(), list.to_string()))
}

/// 같은 제목(대소문자 무시)이 파일 안에 둘 이상인가
fn duplicate(d: &Doc, title: &str) -> bool {
    let want = title.to_lowercase();
    d.spans().iter().filter(|s| s.view.title.to_lowercase() == want).count() > 1
}

/// 동작을 차례로 적용한다. 하나라도 실패하면 아무것도 쓰지 않도록 글만 돌려준다
pub(crate) fn apply(text: &str, ops: &[TaskOp], ctx: &Ctx) -> Result<Applied, Fail> {
    if ops.iter().any(|o| matches!(o, TaskOp::RawSave { .. })) {
        let [TaskOp::RawSave { text: new, .. }] = ops else {
            return Err(Fail::Bad("원문 편집은 다른 동작과 함께 보내지 않는다".into()));
        };
        let mut r = Record::new(TaskAction::Raw, "", "");
        (r.before, r.after) = (Some(text.to_string()), Some(new.clone()));
        return Ok(Applied { text: new.clone(), records: vec![r] });
    }
    let mut d = Doc::new(text);
    let mut records = Vec::new();
    let mut renames: Renames = Vec::new();
    // 그 줄에서 시작하는 task의 지금 hash(동작 뒤 새 블록)
    let sha_at = |d: &Doc, idx: usize| d.spans().into_iter().find(|s| s.idx == idx).map(|s| s.view.block_sha256);
    // 저장 뒤 중복을 볼 제목
    let mut titles: Vec<String> = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        let bad = at_op(i);
        let op = &remap(op, &renames);
        match op {
            TaskOp::Add { list, title, owner, person, profile, machine, notes, parent, before, thread, repo } => {
                let title = tasks::check_title(title).map_err(&bad)?;
                let owner =
                    tasks::edited_owner(None, owner.as_deref(), profile.as_deref(), machine.as_deref(), ctx.profiles)
                        .map_err(&bad)?
                        .flatten();
                let owner = owner.as_deref();
                if let Some(o) = owner {
                    (ctx.owner_ok)(o, *person && profile.is_none() && machine.is_none()).map_err(&bad)?;
                }
                let thread = match thread.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
                    Some(t) => Some(tasks::check_thread(t).map_err(&bad)?),
                    None => None,
                };
                let repo = match repo.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
                    Some(r) => Some((ctx.repo_ok)(r).map_err(&bad)?),
                    None => None,
                };
                let notes = check_notes(notes).map_err(&bad)?;
                let list = if before.is_none() && parent.is_none() {
                    check_list_name_or_unnamed(list).map_err(&bad)?
                } else {
                    String::new()
                };
                let (at, ws, list) = target(&mut d, &list, before.as_ref(), parent.as_ref(), i)?;
                let fields = tasks::Fields { owner, repo: repo.as_deref(), thread: thread.as_deref() };
                let line = tasks::task_line(&ws, ' ', &title, fields);
                tasks::reads_back(&line, &title, fields).map_err(&bad)?;
                let mut block = vec![line];
                block.extend(notes.iter().map(|n| if n.is_empty() { String::new() } else { format!("{ws}  {n}") }));
                let mut r = Record::new(TaskAction::Add, &list, &title);
                r.after = Some(block.join("\n"));
                r.thread = thread;
                let block = d.with_eol(block);
                d.insert(at, block);
                records.push(r);
                titles.push(title);
            }
            TaskOp::Update { at, title, owner, person, profile, machine, notes, thread, done, repo } => {
                let spans = d.spans();
                let s = spans[find(&spans, at, i)?].clone();
                let v = &s.view;
                let new_title = match title {
                    Some(t) if t.trim() != v.title => tasks::check_title(t).map_err(&bad)?,
                    _ => v.title.clone(),
                };
                let edited = tasks::edited_owner(
                    v.owner.as_deref(),
                    owner.as_deref(),
                    profile.as_deref(),
                    machine.as_deref(),
                    ctx.profiles,
                )
                .map_err(&bad)?;
                let new_owner = match edited {
                    None => v.owner.clone(),
                    Some(None) => None,
                    Some(Some(o)) if Some(o.as_str()) == v.owner.as_deref() => v.owner.clone(),
                    Some(Some(o)) => {
                        (ctx.owner_ok)(&o, *person && profile.is_none() && machine.is_none()).map_err(&bad)?;
                        Some(o)
                    }
                };
                let new_thread = match thread.as_deref().map(str::trim) {
                    None => v.thread.clone(),
                    Some("") => None,
                    Some(t) => Some(tasks::check_thread(t).map_err(&bad)?),
                };
                let new_repo = match repo.as_deref().map(str::trim) {
                    None => v.repo.clone(),
                    Some("") => None,
                    Some(r) if Some(r) == v.repo.as_deref() => v.repo.clone(),
                    Some(r) => Some((ctx.repo_ok)(r).map_err(&bad)?),
                };
                let new_done = done.unwrap_or(v.done);
                let raw = plain(&d.lines[s.idx]).to_string();
                let line_changed = new_title != v.title
                    || new_owner != v.owner
                    || new_repo != v.repo
                    || new_thread != v.thread
                    || new_done != v.done;
                let ws = s.ws(&d).to_string();
                let mut block: Vec<String> = Vec::new();
                if line_changed {
                    // 바뀐 제목은 위에서 검사했다. 기존 제목은 실제 owner·저장소·thread와 함께
                    // reads_back으로 검사한다(명시 owner 앞의 괄호 제목도 그대로 보존한다).
                    // owner로 읽을 수 없던 상태 메모는 owner를 뒤에 붙여 묻어 두지 않는다.
                    if tasks::owner_misread(v) {
                        tasks::check_title(&new_title)
                            .map_err(|e| bad(format!("{e}. 제목을 고치거나 「정리 제안」을 쓴다")))?;
                    }
                    let mark = raw.trim_start().chars().nth(3).unwrap_or(' ');
                    let c = match (new_done, v.done) {
                        (true, true) | (false, false) => mark,
                        (true, false) => 'x',
                        (false, true) => ' ',
                    };
                    let fields = tasks::Fields {
                        owner: new_owner.as_deref(),
                        repo: new_repo.as_deref(),
                        thread: new_thread.as_deref(),
                    };
                    let line = tasks::task_line(&ws, c, &new_title, fields);
                    tasks::reads_back(&line, &new_title, fields)
                        .map_err(|e| bad(format!("{e}. 제목을 고치거나 「정리 제안」을 쓴다")))?;
                    block.push(format!("{line}{}{}", crate::task_queue::id_suffix(v.id.as_deref()), d.eol));
                } else {
                    block.push(d.lines[s.idx].clone());
                }
                let new_notes = match notes {
                    Some(n) => Some(check_notes(n).map_err(&bad)?).filter(|n| !same_notes(n, &v.notes)),
                    None => None,
                };
                match &new_notes {
                    None => block.extend(d.lines[s.idx + 1..=s.block_end].iter().cloned()),
                    Some(n) => block.extend(
                        n.iter().map(|l| if l.is_empty() { d.eol.to_string() } else { format!("{ws}  {l}{}", d.eol) }),
                    ),
                }
                if !line_changed && new_notes.is_none() {
                    continue;
                }
                // 블록 끝 줄에 줄 끝 문자가 없었으면(파일 끝) 그대로 둔다
                if !d.lines[s.block_end].ends_with('\n')
                    && let Some(last) = block.last_mut()
                {
                    *last = plain(last).to_string();
                }
                let mut r = Record::new(TaskAction::Update, &v.list, &new_title);
                r.before = Some(d.plain_text(s.idx, s.block_end));
                r.after = Some(block.iter().map(|l| plain(l)).collect::<Vec<_>>().join("\n"));
                r.thread = new_thread.clone();
                if new_title != v.title {
                    r.from_list = None;
                    titles.push(new_title.clone());
                }
                d.lines.splice(s.idx..=s.block_end, block);
                if let Some(new) = sha_at(&d, s.idx) {
                    renames.push((v.block_sha256.clone(), new));
                }
                records.push(r);
            }
            TaskOp::Delete { at } => {
                let spans = d.spans();
                let s = spans[find(&spans, at, i)?].clone();
                let mut r = Record::new(TaskAction::Delete, &s.view.list, &s.view.title);
                r.before = Some(d.plain_text(s.idx, s.sub_end));
                r.thread = s.view.thread.clone();
                d.remove(s.idx, s.sub_end);
                records.push(r);
            }
            TaskOp::Move { at, to_list, before, parent } => {
                let spans = d.spans();
                let s = spans[find(&spans, at, i)?].clone();
                // 자기 하위로는 옮기지 못한다
                for a in [before, parent].into_iter().flatten() {
                    let t = &spans[find(&spans, a, i)?];
                    if (s.idx..=s.sub_end).contains(&t.idx) {
                        return Err(bad("task를 자기 하위로 옮길 수 없다".into()));
                    }
                }
                let to_list = if before.is_none() && parent.is_none() {
                    check_list_name_or_unnamed(to_list).map_err(&bad)?
                } else {
                    String::new()
                };
                let before_text = d.plain_text(s.idx, s.sub_end);
                let mut moved = d.remove(s.idx, s.sub_end);
                if let Some(last) = moved.last_mut()
                    && !last.ends_with('\n')
                {
                    last.push_str(d.eol);
                }
                let (at, ws, list) = target(&mut d, &to_list, before.as_ref(), parent.as_ref(), i)?;
                let moved = reindent(moved, &ws);
                let mut r = Record::new(TaskAction::Move, &list, &s.view.title);
                r.from_list = Some(s.view.list.clone());
                r.before = Some(before_text);
                r.after = Some(moved.iter().map(|l| plain(l)).collect::<Vec<_>>().join("\n"));
                r.thread = s.view.thread.clone();
                d.insert(at, moved);
                if let Some(new) = sha_at(&d, at) {
                    renames.push((s.view.block_sha256.clone(), new));
                }
                records.push(r);
            }
            TaskOp::ListAdd { name, after } => {
                let name = check_list_name(name).map_err(&bad)?;
                if d.has_list(&name) {
                    return Err(bad(format!("이미 있는 목록: {name}")));
                }
                match after {
                    None => d.add_list_at_end(&name),
                    Some(a) => {
                        if !d.has_list(a) {
                            return Err(bad(format!("없는 목록: {a}")));
                        }
                        let r = d.regions().into_iter().rev().find(|r| r.name == *a).unwrap();
                        let at = r.end;
                        let mut block = Vec::new();
                        if at > 0 && !blank(&d.lines[at - 1]) {
                            block.push(d.eol.to_string());
                        }
                        block.push(format!("## {name}{}", d.eol));
                        if at < d.lines.len() {
                            block.push(d.eol.to_string());
                        }
                        d.insert(at, block);
                    }
                }
                records.push(Record::new(TaskAction::ListAdd, &name, ""));
            }
            TaskOp::ListRename { name, to } => {
                let to = check_list_name(to).map_err(&bad)?;
                if name.is_empty() || !d.has_list(name) {
                    return Err(bad(format!("없는 목록: {name}")));
                }
                if to != *name && d.has_list(&to) {
                    return Err(bad(format!("이미 있는 목록: {to}(합치기를 쓴다)")));
                }
                for r in d.regions() {
                    if r.name == *name
                        && let Some(h) = r.head
                    {
                        let eol = &d.lines[h][plain(&d.lines[h]).len()..];
                        d.lines[h] = format!("## {to}{eol}");
                    }
                }
                let mut r = Record::new(TaskAction::ListRename, &to, "");
                r.from_list = Some(name.clone());
                records.push(r);
            }
            TaskOp::ListMerge { name, into } => {
                if name.is_empty() || !d.has_list(name) {
                    return Err(bad(format!("없는 목록: {name}")));
                }
                if !d.has_list(into) || name == into {
                    return Err(bad(format!("합칠 목록이 없다: {into}")));
                }
                let content = take_regions(&mut d, name);
                let body: Vec<String> =
                    content.into_iter().filter(|l| plain(l).strip_prefix("## ").is_none()).collect();
                let body = trim_blank(body);
                let mut r = Record::new(TaskAction::ListMerge, into, "");
                r.from_list = Some(name.clone());
                r.before = Some(body.iter().map(|l| plain(l)).collect::<Vec<_>>().join("\n"));
                if !body.is_empty() {
                    let at = d.list_end(into);
                    let mut body = body;
                    if let Some(last) = body.last_mut()
                        && !last.ends_with('\n')
                    {
                        last.push_str(d.eol);
                    }
                    d.insert(at, body);
                }
                records.push(r);
            }
            TaskOp::ListDelete { name } => {
                if name.is_empty() || !d.has_list(name) {
                    return Err(bad(format!("없는 목록: {name}")));
                }
                let n = d.spans().iter().filter(|s| s.view.list == *name).count();
                if n > 0 {
                    return Err(bad(format!("task가 {n}개 있는 목록은 지우지 않는다: 옮기거나 합친다")));
                }
                let gone = take_regions(&mut d, name);
                let mut r = Record::new(TaskAction::ListDelete, name, "");
                r.before = Some(trim_blank(gone).iter().map(|l| plain(l)).collect::<Vec<_>>().join("\n"));
                records.push(r);
            }
            TaskOp::ListMove { name, before } => {
                if name.is_empty() || !d.has_list(name) {
                    return Err(bad(format!("없는 목록: {name}")));
                }
                if let Some(b) = before
                    && (b.is_empty() || b == name || !d.has_list(b))
                {
                    return Err(bad(format!("앞에 둘 목록이 없다: {b}")));
                }
                let mut block = trim_blank(take_regions(&mut d, name));
                if let Some(last) = block.last_mut()
                    && !last.ends_with('\n')
                {
                    last.push_str(d.eol);
                }
                match before {
                    None => {
                        d.close_last();
                        if d.lines.last().is_some_and(|l| !blank(l)) {
                            d.lines.push(d.eol.to_string());
                        }
                        d.lines.extend(block);
                    }
                    Some(b) => {
                        let at =
                            d.regions().into_iter().find(|r| r.name == *b && r.head.is_some()).unwrap().head.unwrap();
                        block.push(d.eol.to_string());
                        d.insert(at, block);
                    }
                }
                records.push(Record::new(TaskAction::ListMove, name, ""));
            }
            TaskOp::Restore { log_id, list } => {
                let gone = (ctx.removed)(*log_id).map_err(&bad)?;
                let list = match list {
                    Some(l) => check_list_name_or_unnamed(l).map_err(&bad)?,
                    None => gone.list.clone(),
                };
                let lines: Vec<String> = gone.text.split('\n').map(str::to_string).collect();
                let lines = reindent(lines, "");
                let root = Doc::new(&lines.join("\n"));
                let restored: Vec<String> = root.spans().iter().map(|s| s.view.title.clone()).collect();
                let Some(title) = restored.first().cloned() else {
                    return Err(bad("되살릴 task 줄이 기록에 없다".into()));
                };
                let have = d.spans();
                if root.spans().iter().any(|r| r.view.id.is_some() && have.iter().any(|h| h.view.id == r.view.id)) {
                    return Err(bad("같은 ID의 활성 task가 이미 있다".into()));
                }
                if let Some(t) =
                    restored.iter().find(|t| have.iter().any(|s| s.view.title.to_lowercase() == t.to_lowercase()))
                {
                    return Err(bad(format!("같은 제목 task가 이미 있다: {t}")));
                }
                let at = d.list_end(&list);
                let block = d.with_eol(lines.clone());
                d.insert(at, block);
                let mut r = Record::new(TaskAction::Restore, &list, &title);
                r.after = Some(lines.join("\n"));
                r.restored = Some(*log_id);
                records.push(r);
                titles.extend(restored);
            }
            TaskOp::RawSave { .. } => unreachable!(),
        }
    }
    if let Some(t) = titles.iter().find(|t| duplicate(&d, t)) {
        return Err(Fail::Bad(format!("같은 제목 task가 이미 있다: {t}(`--from-task`가 제목으로 찾는다)")));
    }
    Ok(Applied { text: d.text(), records })
}

/// 동작의 대상들을 앞 동작이 바꾼 hash로
fn remap(op: &TaskOp, renames: &Renames) -> TaskOp {
    if renames.is_empty() {
        return op.clone();
    }
    let r = |a: &TaskAnchor| renamed(a, renames);
    let ro = |a: &Option<TaskAnchor>| a.as_ref().map(r);
    match op {
        TaskOp::Add { list, title, owner, person, profile, machine, notes, parent, before, thread, repo } => {
            TaskOp::Add {
                list: list.clone(),
                title: title.clone(),
                owner: owner.clone(),
                person: *person,
                profile: profile.clone(),
                machine: machine.clone(),
                notes: notes.clone(),
                parent: ro(parent),
                before: ro(before),
                thread: thread.clone(),
                repo: repo.clone(),
            }
        }
        TaskOp::Update { at, title, owner, person, profile, machine, notes, thread, done, repo } => TaskOp::Update {
            at: r(at),
            title: title.clone(),
            owner: owner.clone(),
            person: *person,
            profile: profile.clone(),
            machine: machine.clone(),
            notes: notes.clone(),
            thread: thread.clone(),
            done: *done,
            repo: repo.clone(),
        },
        TaskOp::Delete { at } => TaskOp::Delete { at: r(at) },
        TaskOp::Move { at, to_list, before, parent } => {
            TaskOp::Move { at: r(at), to_list: to_list.clone(), before: ro(before), parent: ro(parent) }
        }
        other => other.clone(),
    }
}

/// 목록 이름(빈 글은 첫 `##` 앞의 이름 없는 목록)
fn check_list_name_or_unnamed(name: &str) -> Result<String, String> {
    if name.trim().is_empty() { Ok(String::new()) } else { check_list_name(name) }
}

/// 그 이름의 목록 자리(머리 포함)를 모두 떼어 낸다. 앞뒤 빈 줄이 겹치지 않게 한다
fn take_regions(d: &mut Doc, name: &str) -> Vec<String> {
    let mut out = Vec::new();
    while let Some(r) = d.regions().into_iter().find(|r| r.name == name && r.head.is_some()) {
        let (from, to) = (r.head.unwrap(), r.end);
        if to == from {
            break;
        }
        out.extend(d.remove(from, to - 1));
    }
    out
}

fn trim_blank(mut lines: Vec<String>) -> Vec<String> {
    while lines.last().is_some_and(|l| blank(l)) {
        lines.pop();
    }
    let lead = lines.iter().take_while(|l| blank(l)).count();
    lines.drain(..lead);
    lines
}

/// 파일을 직접 고친 차이(ticker가 「파일 직접」으로 기록한다): 블록 hash가 같으면 그대로(목록이 다르면 옮김), 남은 것은 제목이
/// 같으면 수정, 나머지는 더하기·삭제다. 하위 task는 저마다 한 줄이다.
pub(crate) fn diff(old: &str, new: &str) -> Vec<Record> {
    let (od, nd) = (Doc::new(old), Doc::new(new));
    let (os, ns) = (od.spans(), nd.spans());
    let mut new_left: Vec<Option<&Span>> = ns.iter().map(Some).collect();
    let mut out = Vec::new();
    let mut old_left = Vec::new();
    for o in &os {
        let hit = new_left.iter().position(|n| n.is_some_and(|n| n.view.block_sha256 == o.view.block_sha256));
        match hit {
            Some(k) => {
                let n = new_left[k].take().unwrap();
                if n.view.list != o.view.list {
                    let mut r = Record::new(TaskAction::Move, &n.view.list, &n.view.title);
                    r.from_list = Some(o.view.list.clone());
                    r.after = Some(nd.plain_text(n.idx, n.block_end));
                    r.thread = n.view.thread.clone();
                    out.push(r);
                }
            }
            None => old_left.push(o),
        }
    }
    for o in old_left {
        let want = o.view.title.to_lowercase();
        let hit = new_left.iter().position(|n| n.is_some_and(|n| n.view.title.to_lowercase() == want));
        let before = od.plain_text(o.idx, o.block_end);
        match hit {
            Some(k) => {
                let n = new_left[k].take().unwrap();
                let mut r = Record::new(TaskAction::Update, &n.view.list, &n.view.title);
                if n.view.list != o.view.list {
                    r.from_list = Some(o.view.list.clone());
                }
                r.before = Some(before);
                r.after = Some(nd.plain_text(n.idx, n.block_end));
                r.thread = n.view.thread.clone();
                out.push(r);
            }
            None => {
                let mut r = Record::new(TaskAction::Delete, &o.view.list, &o.view.title);
                r.before = Some(before);
                r.thread = o.view.thread.clone();
                out.push(r);
            }
        }
    }
    for n in new_left.into_iter().flatten() {
        let mut r = Record::new(TaskAction::Add, &n.view.list, &n.view.title);
        r.after = Some(nd.plain_text(n.idx, n.block_end));
        r.thread = n.view.thread.clone();
        out.push(r);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{parse, parse_board, sha256};

    fn ok_owner(o: &str, person: bool) -> Result<(), String> {
        match (o, person) {
            ("claude" | "me" | "agent" | "codex@mac-b", _) => Ok(()),
            (_, true) => tasks::check_person(o),
            _ => Err(format!("`{o}`는 사람 이름으로 읽힌다")),
        }
    }

    fn ok_repo(r: &str) -> Result<String, String> {
        match r {
            "aaa" | tasks::REPO_DIR => Ok(r.to_string()),
            "/work/aaa" => Ok("aaa".into()),
            _ => Err(format!("project.toml repos에 없는 저장소다: {r}")),
        }
    }

    fn no_log(_: i64) -> Result<Removed, String> {
        Err("기록 없음".into())
    }

    fn run(text: &str, ops: Vec<TaskOp>) -> Result<Applied, Fail> {
        apply(text, &ops, &Ctx { owner_ok: &ok_owner, profiles: &[], repo_ok: &ok_repo, removed: &no_log })
    }

    fn anchor(text: &str, title: &str) -> TaskAnchor {
        let t = parse(text).into_iter().find(|t| t.title == title).unwrap_or_else(|| panic!("{title}"));
        TaskAnchor { list: t.list, title: t.title, block_sha256: t.block_sha256, line: t.line }
    }

    fn update(at: TaskAnchor) -> TaskOp {
        TaskOp::Update {
            at,
            title: None,
            owner: None,
            person: false,
            profile: None,
            machine: None,
            notes: None,
            thread: None,
            done: None,
            repo: None,
        }
    }

    /// 모델·머신 칸(plan §8.335): 칸마다 고치고(다른 칸은 그대로), 바뀌지 않은 옛 줄은 바이트 그대로, owner(me)와 함께 주면 거부
    #[test]
    fn model_machine_edits() {
        fn ok(o: &str, _: bool) -> Result<(), String> {
            if o == "nobody@mac-b" {
                Err("mac-b에서 쓸 수 있는 profile이 아니다: nobody".into())
            } else {
                Ok(())
            }
        }
        let profiles = vec!["claude".to_string(), "codex-fast".to_string()];
        let go = |text: &str, ops: Vec<TaskOp>| {
            apply(text, &ops, &Ctx { owner_ok: &ok, profiles: &profiles, repo_ok: &ok_repo, removed: &no_log })
        };
        let md = "## A\n- [ ] 일 (codex-fast@mac-a) [aaa] · t-0001\n  메모\n- [ ] 옛 줄 (codex-fast)  \n";
        let set = |title: &str, p: Option<&str>, m: Option<&str>| {
            let mut op = update(anchor(md, title));
            if let TaskOp::Update { profile, machine, .. } = &mut op {
                *profile = p.map(Into::into);
                *machine = m.map(Into::into);
            }
            op
        };
        let line = |out: Applied| out.text.lines().nth(1).unwrap().to_string();
        assert_eq!(line(go(md, vec![set("일", None, Some(""))]).unwrap()), "- [ ] 일 (codex-fast) [aaa] · t-0001");
        assert_eq!(
            line(go(md, vec![set("일", Some("claude"), None)]).unwrap()),
            "- [ ] 일 (claude@mac-a) [aaa] · t-0001"
        );
        assert_eq!(line(go(md, vec![set("일", Some(""), None)]).unwrap()), "- [ ] 일 (@mac-a) [aaa] · t-0001");
        assert_eq!(line(go(md, vec![set("일", Some(""), Some(""))]).unwrap()), "- [ ] 일 [aaa] · t-0001");
        // 메모만 고치면 옛 줄은 바이트 그대로다(끝 공백까지)
        let mut notes = update(anchor(md, "옛 줄"));
        if let TaskOp::Update { notes: n, .. } = &mut notes {
            *n = Some(vec!["새 메모".into()]);
        }
        assert!(go(md, vec![notes]).unwrap().text.contains("- [ ] 옛 줄 (codex-fast)  \n  새 메모\n"));
        // 모델만 적힌 줄에 머신을 더한다
        assert!(
            go(md, vec![set("옛 줄", None, Some("mac-b"))]).unwrap().text.contains("- [ ] 옛 줄 (codex-fast@mac-b)\n")
        );
        // 더하기: 두 칸, 판정 거부, me와 함께 주면 거부
        let add = |owner: Option<&str>, p: Option<&str>, m: Option<&str>| TaskOp::Add {
            list: "A".into(),
            title: "새 일".into(),
            owner: owner.map(Into::into),
            person: false,
            profile: p.map(Into::into),
            machine: m.map(Into::into),
            notes: vec![],
            parent: None,
            before: None,
            thread: None,
            repo: None,
        };
        let out = go(md, vec![add(None, Some("claude"), Some("mac-b"))]).unwrap();
        assert!(out.text.contains("- [ ] 새 일 (claude@mac-b)\n"), "{}", out.text);
        let e = go(md, vec![add(None, Some("nobody"), Some("mac-b"))]).unwrap_err();
        assert!(matches!(&e, Fail::Bad(m) if m.contains("쓸 수 있는 profile이 아니다")), "{e:?}");
        let e = go(md, vec![add(Some("me"), Some("claude"), None)]).unwrap_err();
        assert!(matches!(&e, Fail::Bad(m) if m.contains("함께 주지 않는다")), "{e:?}");
    }

    /// 저장소 칸(plan §8.330): 더하기·바꾸기(`None` 그대로, 빈 글 비움)·경로는 이름으로·모르는 이름 거부
    #[test]
    fn repo_field_edits() {
        let md = "## A\n- [ ] 하나 (claude) · t-1\n";
        let add = |repo: &str| TaskOp::Add {
            list: "A".into(),
            title: "둘".into(),
            owner: Some("me".into()),
            person: false,
            profile: None,
            machine: None,
            notes: vec![],
            parent: None,
            before: None,
            thread: None,
            repo: Some(repo.into()),
        };
        assert_eq!(run(md, vec![add("/work/aaa")]).unwrap().text, format!("{md}- [ ] 둘 (me) [aaa]\n"));
        let e = run(md, vec![add("bbb")]).unwrap_err();
        assert!(matches!(&e, Fail::Bad(m) if m.contains("없는 저장소")), "{e:?}");
        let set = |text: &str, repo: Option<&str>, owner: Option<&str>| {
            run(
                text,
                vec![TaskOp::Update {
                    at: anchor(text, "하나"),
                    title: None,
                    owner: owner.map(Into::into),
                    person: false,
                    profile: None,
                    machine: None,
                    notes: None,
                    thread: None,
                    done: None,
                    repo: repo.map(Into::into),
                }],
            )
            .unwrap()
            .text
        };
        let with = set(md, Some(tasks::REPO_DIR), None);
        assert_eq!(with, "## A\n- [ ] 하나 (claude) [작업 폴더] · t-1\n");
        assert_eq!(set(&with, None, Some("me")), "## A\n- [ ] 하나 (me) [작업 폴더] · t-1\n", "None은 그대로");
        assert_eq!(set(&with, Some(""), None), md, "빈 글은 비움");
        // 끝 대괄호 제목 뒤에 owner가 있는 옛 줄은 그대로 고친다(thread 잇기)
        let old = "## A\n- [ ] 배포 [v2] (claude)\n";
        let link = TaskOp::Update {
            at: anchor(old, "배포 [v2]"),
            title: None,
            owner: None,
            person: false,
            profile: None,
            machine: None,
            notes: None,
            thread: Some("t-2".into()),
            done: None,
            repo: None,
        };
        assert_eq!(run(old, vec![link]).unwrap().text, "## A\n- [ ] 배포 [v2] (claude) · t-2\n");
        let bare = TaskOp::Add {
            list: "A".into(),
            title: "릴리스 [v2]".into(),
            owner: None,
            person: false,
            profile: None,
            machine: None,
            notes: vec![],
            parent: None,
            before: None,
            thread: None,
            repo: None,
        };
        let e = run(md, vec![bare]).unwrap_err();
        assert!(matches!(&e, Fail::Bad(m) if m.contains("저장소 칸으로 읽힌다")), "{e:?}");
    }

    /// 목록 설명·빈 줄·모르는 줄·하위 task·CRLF·마지막 줄바꿈 없음이 든 파일
    const MD: &str = "# Tasks\r\n\r\n## Backlog\r\n나중 일은 later.md에 모은다\r\n\r\n- [ ] 상위 (me)\r\n  메모 A\r\n  - [ ] 하위 하나\r\n    하위 메모\r\n  하위 뒤 상위 들여쓰기 줄\r\n- [ ] 정리 (t-0101 S4 뒤, 사용자 지시 시)\r\n<!-- 모르는 줄 -->\r\n\r\n## Later\r\n- [x] 끝 (claude) · t-0007";

    /// 한 task만 고치면 나머지 바이트는 그대로다
    #[test]
    fn update_keeps_other_bytes() {
        let a = anchor(MD, "하위 하나");
        let notes = Some(vec!["새 메모".into(), "".into(), "  들여쓴 줄".into()]);
        let out = run(
            MD,
            vec![TaskOp::Update {
                at: a,
                title: None,
                owner: None,
                person: false,
                profile: None,
                machine: None,
                notes,
                thread: None,
                done: None,
                repo: None,
            }],
        )
        .unwrap();
        assert_eq!(
            out.text,
            MD.replace("    하위 메모\r\n", "    새 메모\r\n\r\n      들여쓴 줄\r\n"),
            "바뀐 블록만 다르다"
        );
        let r = &out.records[0];
        assert_eq!((r.action, r.list.as_str(), r.title.as_str()), (TaskAction::Update, "Backlog", "하위 하나"));
        assert_eq!(r.before.as_deref(), Some("  - [ ] 하위 하나\n    하위 메모"));
        // 칸이 하나도 안 바뀌면 글이 그대로이고 기록도 없다
        let same = run(
            MD,
            vec![TaskOp::Update {
                at: anchor(MD, "상위"),
                title: Some("상위".into()),
                owner: Some("me".into()),
                person: false,
                profile: None,
                machine: None,
                notes: Some(vec!["메모 A".into()]),
                thread: None,
                done: Some(false),
                repo: None,
            }],
        )
        .unwrap();
        assert_eq!(same.text, MD);
        assert!(same.records.is_empty());
        // 제목만 바꾸면 task 줄만 정규형으로 새로 쓴다(메모·줄 끝 문자 그대로)
        let t = run(
            MD,
            vec![TaskOp::Update {
                at: anchor(MD, "끝"),
                title: Some("끝난 일".into()),
                owner: None,
                person: false,
                profile: None,
                machine: None,
                notes: None,
                thread: None,
                done: None,
                repo: None,
            }],
        )
        .unwrap();
        assert_eq!(t.text, MD.replace("- [x] 끝 (claude) · t-0007", "- [x] 끝난 일 (claude) · t-0007"));
    }

    /// 다른 task가 그사이 바뀌어도 성공하고, 대상이 바뀌거나 지워지면 충돌이다(되살아나지 않는다)
    #[test]
    fn block_anchor_conflicts_only_on_the_target() {
        let a = anchor(MD, "끝");
        let other = MD.replace("메모 A", "메모 A 고침");
        let out = run(&other, vec![TaskOp::Delete { at: a.clone() }]).unwrap();
        assert!(!out.text.contains("끝 (claude)") && out.text.contains("메모 A 고침"));
        let changed = MD.replace("t-0007", "t-0008");
        let e = run(&changed, vec![TaskOp::Delete { at: a.clone() }]).unwrap_err();
        assert!(matches!(&e, Fail::Conflict { op: 0, title, .. } if title == "끝"), "{e:?}");
        let gone = MD.replace("\r\n## Later\r\n- [x] 끝 (claude) · t-0007", "\r\n## Later\r\n");
        let e = run(&gone, vec![update(a)]).unwrap_err();
        assert!(matches!(e, Fail::Conflict { .. }), "지운 task 수정은 충돌: {e:?}");
        // 제목으로 찾기(CLI)
        let by_title = TaskAnchor { title: "상위".into(), ..Default::default() };
        assert!(run(MD, vec![TaskOp::Delete { at: by_title }]).is_ok());
    }

    /// 저장 검사: `)` 끝 제목, ` · t-` 제목, 중복 제목(대소문자 무시), 사람 owner 표시, task 꼴 메모
    #[test]
    fn save_checks() {
        let add = |title: &str, owner: Option<&str>, person: bool, notes: Vec<&str>| TaskOp::Add {
            list: "Backlog".into(),
            title: title.into(),
            owner: owner.map(str::to_string),
            person,
            profile: None,
            machine: None,
            notes: notes.into_iter().map(str::to_string).collect(),
            parent: None,
            before: None,
            thread: None,
            repo: None,
        };
        for (op, why) in [
            (add("상태 메모 (S4 뒤)", None, false, vec![]), "`)`로 끝나면"),
            (add("웹 화면 · t-0002", None, false, vec![]), "thread로 읽힌다"),
            (add("상위", None, false, vec![]), "같은 제목"),
            (add("새 일", Some("Priya"), false, vec![]), "사람 이름으로 읽힌다"),
            (add("새 일", Some("t-0001 뒤, 지시 시"), true, vec![]), "owner 꼴"),
            (add("새 일", None, false, vec!["- [ ] 하위"]), "task 꼴"),
            (add("새 일", None, false, vec!["# 머리"]), "`#`"),
            (add("", None, false, vec![]), "비었다"),
        ] {
            match run(MD, vec![op]) {
                Err(Fail::Bad(e)) => assert!(e.contains(why), "{why}: {e}"),
                other => panic!("{why}: {other:?}"),
            }
        }
        // 대소문자만 다른 제목도 중복이다
        assert!(matches!(run(MD, vec![add("하위 하나", None, false, vec![])]), Err(Fail::Bad(_))));
        let ok = run(MD, vec![add("새 일", Some("Priya Rao"), true, vec!["메모", "", "둘째"])]).unwrap();
        assert!(ok.text.ends_with("- [x] 끝 (claude) · t-0007"), "다른 목록은 그대로");
        assert!(
            ok.text.contains("<!-- 모르는 줄 -->\r\n- [ ] 새 일 (Priya Rao)\r\n  메모\r\n\r\n  둘째\r\n\r\n## Later"),
            "{}",
            ok.text
        );
        // 형식 어긋남 task의 제목을 그대로 둔 채 owner만 바꾸면 다시 읽히지 않으므로 거부한다
        let e = run(
            MD,
            vec![TaskOp::Update {
                at: anchor(MD, "정리 (t-0101 S4 뒤, 사용자 지시 시)"),
                title: None,
                owner: Some("me".into()),
                person: false,
                profile: None,
                machine: None,
                notes: None,
                thread: None,
                done: None,
                repo: None,
            }],
        )
        .unwrap_err();
        assert!(matches!(&e, Fail::Bad(m) if m.contains("정리 제안")), "{e:?}");
    }

    /// 지우기·옮기기는 하위 묶음과 함께, 들여쓰기를 맞추고, 빈 줄이 겹치지 않는다. 기록은 묶음 전체다
    #[test]
    fn delete_and_move_carry_the_subtree() {
        let md = "## A\n- [ ] 하나\n\n- [ ] 둘\n  메모\n  - [ ] 둘의 하위\n\n- [ ] 셋\n## B\n- [ ] 넷\n";
        let out = run(md, vec![TaskOp::Delete { at: anchor(md, "둘") }]).unwrap();
        assert_eq!(out.text, "## A\n- [ ] 하나\n\n- [ ] 셋\n## B\n- [ ] 넷\n");
        assert_eq!(out.records[0].before.as_deref(), Some("- [ ] 둘\n  메모\n  - [ ] 둘의 하위"));
        // 다른 목록 끝으로
        let out = run(md, vec![TaskOp::Move { at: anchor(md, "둘"), to_list: "B".into(), before: None, parent: None }])
            .unwrap();
        assert_eq!(out.text, "## A\n- [ ] 하나\n\n- [ ] 셋\n## B\n- [ ] 넷\n- [ ] 둘\n  메모\n  - [ ] 둘의 하위\n");
        assert_eq!(out.records[0].from_list.as_deref(), Some("A"));
        // 다른 task의 하위로(들여쓰기 +2)
        let out = run(
            md,
            vec![TaskOp::Move {
                at: anchor(md, "둘"),
                to_list: String::new(),
                before: None,
                parent: Some(anchor(md, "넷")),
            }],
        )
        .unwrap();
        assert!(out.text.ends_with("## B\n- [ ] 넷\n  - [ ] 둘\n    메모\n    - [ ] 둘의 하위\n"), "{}", out.text);
        let (_, t) = parse_board(&out.text);
        assert_eq!(
            t.iter().find(|t| t.title == "둘").unwrap().parent,
            t.iter().find(|t| t.title == "넷").map(|t| t.line)
        );
        // 하위를 위로(맨 위로 = 첫 task 앞)
        let out = run(
            md,
            vec![TaskOp::Move {
                at: anchor(md, "둘의 하위"),
                to_list: String::new(),
                before: Some(anchor(md, "하나")),
                parent: None,
            }],
        )
        .unwrap();
        assert!(out.text.starts_with("## A\n- [ ] 둘의 하위\n- [ ] 하나\n"), "{}", out.text);
        // 자기 하위로는 못 옮긴다
        assert!(matches!(
            run(
                md,
                vec![TaskOp::Move {
                    at: anchor(md, "둘"),
                    to_list: String::new(),
                    before: None,
                    parent: Some(anchor(md, "둘의 하위"))
                }]
            ),
            Err(Fail::Bad(_))
        ));
        // 마지막 줄바꿈 없는 파일 끝 task를 옮겨도 줄이 붙지 않는다
        let tail = "## A\n- [ ] 하나\n## B\n- [ ] 끝";
        let out =
            run(tail, vec![TaskOp::Move { at: anchor(tail, "끝"), to_list: "A".into(), before: None, parent: None }])
                .unwrap();
        assert_eq!(out.text, "## A\n- [ ] 하나\n- [ ] 끝\n## B\n");
    }

    /// 목록: 더하기·이름 바꾸기·합치기(설명 줄도)·빈 목록만 지우기·옮기기
    #[test]
    fn list_ops() {
        let md = "# Tasks\n\n## A\nA 설명\n- [ ] 하나\n\n## 빈\n\n## B\n- [ ] 둘\n";
        let out = run(md, vec![TaskOp::ListAdd { name: "새".into(), after: Some("A".into()) }]).unwrap();
        assert_eq!(out.text, "# Tasks\n\n## A\nA 설명\n- [ ] 하나\n\n## 새\n\n## 빈\n\n## B\n- [ ] 둘\n");
        let out = run(md, vec![TaskOp::ListAdd { name: "끝".into(), after: None }]).unwrap();
        assert!(out.text.ends_with("- [ ] 둘\n\n## 끝\n"), "{}", out.text);
        assert!(matches!(run(md, vec![TaskOp::ListAdd { name: "B".into(), after: None }]), Err(Fail::Bad(_))));
        let out = run(md, vec![TaskOp::ListRename { name: "A".into(), to: "가".into() }]).unwrap();
        assert_eq!(out.text, md.replace("## A\n", "## 가\n"));
        assert!(
            matches!(run(md, vec![TaskOp::ListRename { name: "A".into(), to: "B".into() }]), Err(Fail::Bad(m)) if m.contains("합치기"))
        );
        let out = run(md, vec![TaskOp::ListMerge { name: "A".into(), into: "B".into() }]).unwrap();
        assert_eq!(out.text, "# Tasks\n\n## 빈\n\n## B\n- [ ] 둘\nA 설명\n- [ ] 하나\n");
        let (lists, _) = parse_board(&out.text);
        assert_eq!(lists.iter().find(|l| l.name == "B").unwrap().notes, ["A 설명"]);
        assert!(
            matches!(run(md, vec![TaskOp::ListDelete { name: "A".into() }]), Err(Fail::Bad(m)) if m.contains("1개"))
        );
        let out = run(md, vec![TaskOp::ListDelete { name: "빈".into() }]).unwrap();
        assert_eq!(out.text, "# Tasks\n\n## A\nA 설명\n- [ ] 하나\n\n## B\n- [ ] 둘\n");
        let out = run(md, vec![TaskOp::ListMove { name: "B".into(), before: Some("A".into()) }]).unwrap();
        assert_eq!(out.text, "# Tasks\n\n## B\n- [ ] 둘\n\n## A\nA 설명\n- [ ] 하나\n\n## 빈\n");
        let out = run(md, vec![TaskOp::ListMove { name: "A".into(), before: None }]).unwrap();
        assert_eq!(out.text, "# Tasks\n\n## 빈\n\n## B\n- [ ] 둘\n\n## A\nA 설명\n- [ ] 하나\n");
        // 없는 목록에 더하면 파일 끝에 만든다(빈 파일 포함)
        let out = run(
            "",
            vec![TaskOp::Add {
                list: "Backlog".into(),
                title: "첫 일".into(),
                owner: None,
                person: false,
                profile: None,
                machine: None,
                notes: vec![],
                parent: None,
                before: None,
                thread: None,
                repo: None,
            }],
        )
        .unwrap();
        assert_eq!(out.text, "## Backlog\n- [ ] 첫 일\n");
        // 한 요청 안에서 새 목록을 만들고 옮긴다
        let out = run(
            md,
            vec![
                TaskOp::ListAdd { name: "C".into(), after: None },
                TaskOp::Move { at: anchor(md, "둘"), to_list: "C".into(), before: None, parent: None },
            ],
        )
        .unwrap();
        assert!(out.text.ends_with("## B\n\n## C\n- [ ] 둘\n"), "{}", out.text);
    }

    /// 되살리기: 지운 묶음이 원래 목록 끝에 그대로 돌아오고, 같은 제목이 있으면 거부한다
    #[test]
    fn restore_brings_back_the_block() {
        let md = "## A\n- [ ] 하나\n  - [ ] 하위\n- [ ] 둘\n";
        let del = run(md, vec![TaskOp::Delete { at: anchor(md, "하나") }]).unwrap();
        let text = del.records[0].before.clone().unwrap();
        let removed = move |id: i64| {
            if id == 7 { Ok(Removed { list: "A".into(), text: text.clone() }) } else { Err("없는 기록".into()) }
        };
        let ctx = Ctx { owner_ok: &ok_owner, profiles: &[], repo_ok: &ok_repo, removed: &removed };
        let back = apply(&del.text, &[TaskOp::Restore { log_id: 7, list: None }], &ctx).unwrap();
        assert_eq!(back.text, "## A\n- [ ] 둘\n- [ ] 하나\n  - [ ] 하위\n");
        assert_eq!((back.records[0].title.as_str(), back.records[0].restored), ("하나", Some(7)));
        let e = apply(md, &[TaskOp::Restore { log_id: 7, list: None }], &ctx).unwrap_err();
        assert!(matches!(&e, Fail::Bad(m) if m.contains("같은 제목")), "{e:?}");
        // 고른 목록(없으면 만든다)
        let other = apply(&del.text, &[TaskOp::Restore { log_id: 7, list: Some("새".into()) }], &ctx).unwrap();
        assert!(other.text.ends_with("## 새\n- [ ] 하나\n  - [ ] 하위\n"), "{}", other.text);
    }

    /// 파일 직접 편집의 차이: 그대로·옮김·수정·삭제·더하기
    #[test]
    fn file_diff_records() {
        let old = "## A\n- [ ] 하나\n- [ ] 둘\n  메모\n- [ ] 셋\n## B\n";
        let new = "## A\n- [ ] 하나\n- [ ] 둘\n  메모 고침\n## B\n- [ ] 셋\n- [ ] 넷\n";
        let r: Vec<_> = diff(old, new).into_iter().map(|r| (r.action, r.title, r.list, r.from_list)).collect();
        assert_eq!(
            r,
            [
                (TaskAction::Move, "셋".into(), "B".into(), Some("A".into())),
                (TaskAction::Update, "둘".into(), "A".into(), None),
                (TaskAction::Add, "넷".into(), "B".into(), None),
            ]
        );
        let gone = diff(old, "## A\n- [ ] 하나\n## B\n");
        assert_eq!(gone.iter().filter(|r| r.action == TaskAction::Delete).count(), 2);
        assert_eq!(gone[0].before.as_deref(), Some("- [ ] 둘\n  메모"));
        assert!(diff(old, old).is_empty());
    }

    /// 한 요청에서 같은 task를 고친 뒤 옮긴다(둘째 동작은 바뀐 hash를 따라간다)
    #[test]
    fn update_then_move_in_one_request() {
        let md = "## A\n- [ ] 하나\n- [ ] 둘\n## B\n- [ ] 셋\n";
        let a = anchor(md, "하나");
        let out = run(
            md,
            vec![
                TaskOp::Update {
                    at: a.clone(),
                    title: Some("하나 고침".into()),
                    owner: None,
                    person: false,
                    profile: None,
                    machine: None,
                    notes: Some(vec!["메모".into()]),
                    thread: None,
                    done: None,
                    repo: None,
                },
                TaskOp::Move { at: a.clone(), to_list: String::new(), before: None, parent: Some(anchor(md, "셋")) },
                TaskOp::Update {
                    at: a,
                    title: None,
                    owner: Some("me".into()),
                    person: false,
                    profile: None,
                    machine: None,
                    notes: None,
                    thread: None,
                    done: None,
                    repo: None,
                },
            ],
        )
        .unwrap();
        assert_eq!(out.text, "## A\n- [ ] 둘\n## B\n- [ ] 셋\n  - [ ] 하나 고침 (me)\n    메모\n");
        assert_eq!(
            out.records.iter().map(|r| r.action).collect::<Vec<_>>(),
            [TaskAction::Update, TaskAction::Move, TaskAction::Update]
        );
    }

    /// 원문 편집은 홀로만
    #[test]
    fn raw_save_alone() {
        let raw = TaskOp::RawSave { expected_sha256: sha256(MD), text: "## X\n".into() };
        assert_eq!(run(MD, vec![raw.clone()]).unwrap().text, "## X\n");
        assert!(matches!(run(MD, vec![raw, TaskOp::ListAdd { name: "Y".into(), after: None }]), Err(Fail::Bad(_))));
    }
}
