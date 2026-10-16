//! TASKS.md 읽기(0.2.35 의미 그대로, 부록 A). aa는 읽기만 하고 쓰는 쪽은 coordinator다.
//!
//! - task 줄: `- [ ] 제목 (owner) · t-0001`. `- [x]`도 task다(완료는 줄을 지워 표시한다).
//! - owner: 없음·`agent`는 미할당, `me`는 사용자, `profile`·`@machine`·`profile@machine`은 agent, 그 밖의 글은 사람.
//!   옛 표기 `(agent → t-0007)`도 읽는다.
//! - 메모: task 줄보다 깊게 들여쓴 줄(사이의 빈 줄 허용).

use aa_proto::TaskView;

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

fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

fn is_task(l: &str) -> bool {
    let t = l.trim_start();
    t.starts_with("- [") && t.get(4..5) == Some("]")
}

/// 줄 본문 → (제목, owner, thread)
fn split_line(text: &str) -> (String, Option<String>, Option<String>) {
    let mut rest = text.trim().to_string();
    let mut thread = None;
    if let Some((head, tail)) = rest.rsplit_once('·') {
        let t = tail.trim();
        if t.starts_with("t-") && t.len() > 2 && t[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            thread = Some(t.to_string());
            rest = head.trim().to_string();
        }
    }
    let mut owner = None;
    if rest.ends_with(')')
        && let Some(open) = rest.rfind('(')
    {
        let inner = rest[open + 1..rest.len() - 1].trim().to_string();
        rest = rest[..open].trim().to_string();
        // 옛 표기: (agent → t-0007)
        if let Some((o, t)) = inner.split_once('→') {
            thread = thread.or_else(|| Some(t.trim().to_string()));
            owner = Some(o.trim().to_string());
        } else {
            owner = Some(inner);
        }
    }
    (rest, owner, thread)
}

pub(crate) fn parse(text: &str) -> Vec<TaskView> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut list = String::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        if let Some(h) = l.strip_prefix("## ") {
            list = h.trim().to_string();
            i += 1;
            continue;
        }
        if !is_task(l) {
            i += 1;
            continue;
        }
        let t = l.trim_start();
        let done = matches!(&t[3..4], "x" | "X");
        let (title, owner, thread) = split_line(&t[5..]);
        let base = indent(l);
        let mut notes = Vec::new();
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
            j += 1;
        }
        out.push(TaskView { list: list.clone(), title, owner, thread, notes, done });
        // 멈춘 줄(다음 task·제목, 들여쓴 하위 task 포함)부터 다시 본다
        i = j;
    }
    out
}

/// 제목이 같은(대소문자 무시) task
pub(crate) fn find<'a>(tasks: &'a [TaskView], title: &str) -> Option<&'a TaskView> {
    let want = title.trim().to_lowercase();
    tasks.iter().find(|t| t.title.to_lowercase() == want)
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
    }
}
