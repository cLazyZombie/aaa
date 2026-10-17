//! 왼쪽 목록(웹 목록과 같은 모양, §8.35): Thread 묶음과 머신별 pane, 상태 배지, 새 셸.
//!
//! host가 보낸 머신·pane 목록과 thread 사건으로만 만든다(TUI는 저장하지 않는다). 고르면 그 pane을 탭으로 연다.

use std::collections::HashMap;

use aaa_proto::layout::Rect;
use aaa_proto::{AgentActivity, Connection, MachineView, PaneView, Role, SessionState, ThreadGroup, ThreadView};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

use crate::{ACCENT, BLOCKED, DIM, DONE, FAINT, ON_CHIP, WARN, WORKING, activity_text, unicode_width};

/// 목록 폭(구분선 제외). 이보다 좁은 터미널(NARROW)에서는 접힌 채 시작한다.
pub const WIDTH: u16 = 32;
pub const NARROW: u16 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Header(String),
    /// thread 키(`project/id`)
    Thread(String),
    Machine(String),
    /// 그 머신에 새 셸을 연다(머신 ID)
    NewShell(String),
    Pane(String),
    Empty,
}

impl Row {
    pub fn selectable(&self) -> bool {
        matches!(self, Row::Thread(_) | Row::NewShell(_) | Row::Pane(_))
    }
}

pub fn thread_key(t: &ThreadView) -> String {
    format!("{}/{}", t.project, t.id)
}

/// 웹 목록과 같은 순서: 끝나지 않은 thread(평가자 제외, 최근 바뀐 순) → 머신마다 새 셸·pane(평가자 제외)
pub fn rows(machines: &[MachineView], panes: &[PaneView], threads: &HashMap<String, ThreadView>) -> Vec<Row> {
    let mut out = vec![];
    let mut shown: Vec<&ThreadView> =
        threads.values().filter(|t| t.group != ThreadGroup::Resolved && t.role != Role::Evaluator).collect();
    shown.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms).then_with(|| thread_key(a).cmp(&thread_key(b))));
    if !shown.is_empty() {
        out.push(Row::Header(format!("Thread {}", shown.len())));
        out.extend(shown.into_iter().map(|t| Row::Thread(thread_key(t))));
    }
    for m in machines {
        out.push(Row::Machine(m.id.clone()));
        if m.connection == Connection::Connected {
            out.push(Row::NewShell(m.id.clone()));
        }
        let mine: Vec<&PaneView> =
            panes.iter().filter(|p| p.machine_id == m.id && p.role != Some(Role::Evaluator)).collect();
        if mine.is_empty() {
            out.push(Row::Empty);
        }
        out.extend(mine.into_iter().map(|p| Row::Pane(p.id.clone())));
    }
    out
}

/// 다음(앞·뒤) 고를 수 있는 줄. 없으면 그대로.
pub fn step(rows: &[Row], from: usize, down: bool) -> usize {
    let mut i = from;
    loop {
        let next = if down { i.checked_add(1).filter(|&n| n < rows.len()) } else { i.checked_sub(1) };
        match next {
            Some(n) if rows[n].selectable() => return n,
            Some(n) => i = n,
            None => return from,
        }
    }
}

/// 고를 수 있는 첫 줄
pub fn first(rows: &[Row]) -> usize {
    rows.iter().position(Row::selectable).unwrap_or(0)
}

pub fn group_label(g: ThreadGroup) -> &'static str {
    match g {
        ThreadGroup::WaitingOnYou => "사람 차례",
        ThreadGroup::ReadyForReview => "검토 대기",
        ThreadGroup::Working => "작업 중",
        ThreadGroup::Resolved => "끝남",
    }
}

fn group_color(g: ThreadGroup) -> Color {
    match g {
        ThreadGroup::WaitingOnYou => WARN,
        ThreadGroup::ReadyForReview => ACCENT,
        ThreadGroup::Working => WORKING,
        ThreadGroup::Resolved => DIM,
    }
}

/// pane 줄 오른쪽 배지(웹 목록과 같은 말). 실행 중이고 harness 상태가 없으면 없다.
pub fn pane_badge(p: &PaneView) -> Option<(String, Color)> {
    if p.state.is_running() && p.connection == Connection::Disconnected {
        return Some(("끊김".into(), WARN));
    }
    match &p.state {
        SessionState::Running => p.agent.as_ref().map(|a| {
            let pct = a.percent.map(|n| format!(" {n}%")).unwrap_or_default();
            let color = match a.activity {
                AgentActivity::Working => WORKING,
                AgentActivity::Question | AgentActivity::Error => BLOCKED,
                _ => DIM,
            };
            (format!("{}{pct}", activity_text(a.activity)), color)
        }),
        SessionState::Exited { code } => Some((code.map_or_else(|| "종료".into(), |c| format!("종료({c})")), FAINT)),
        SessionState::Interrupted => Some(("중단".into(), WARN)),
        SessionState::Lost => Some(("유실".into(), WARN)),
    }
}

/// pane 제목: 그 pane에서 도는 thread가 있으면 thread 제목, 아니면 이름·명령
pub fn pane_title(p: &PaneView, threads: &HashMap<String, ThreadView>) -> String {
    if let Some(t) = threads.values().find(|t| t.pane.as_deref() == Some(p.id.as_str())) {
        return t.title.clone();
    }
    if !p.name.is_empty() {
        return p.name.clone();
    }
    if !p.argv.is_empty() {
        return p.argv.join(" ");
    }
    p.id.chars().take(8).collect()
}

pub struct View<'a> {
    pub machines: &'a [MachineView],
    pub panes: &'a HashMap<String, PaneView>,
    pub threads: &'a HashMap<String, ThreadView>,
    /// 지금 열린(포커스) pane
    pub current: Option<&'a str>,
    pub sel: usize,
    pub focused: bool,
}

/// 목록을 그린다(오른쪽 구분선 포함). 줄이 넘치면 고른 줄이 보이게 내린다.
pub fn render(buf: &mut Buffer, r: Rect, rows: &[Row], v: &View) {
    let sep_x = r.x + r.w;
    for y in r.y..r.y + r.h {
        buf.set_string(sep_x, y, "│", Style::default().fg(if v.focused { ACCENT } else { FAINT }));
    }
    let skip = (v.sel + 1).saturating_sub(r.h as usize);
    for (n, row) in rows.iter().enumerate().skip(skip).take(r.h as usize) {
        let y = r.y + (n - skip) as u16;
        let sel = n == v.sel && v.focused;
        let (mut left, right, mut st, badge_st) = line(row, v);
        if sel {
            // 색이 없는 터미널에서도 고른 줄이 보이게 표시한다
            left = left.replacen(' ', "▸", 1);
        }
        let current = matches!(row, Row::Pane(p) if Some(p.as_str()) == v.current)
            || matches!(row, Row::Thread(k) if v.threads.get(k).and_then(|t| t.pane.as_deref()) == v.current && v.current.is_some());
        if current {
            st = st.add_modifier(Modifier::BOLD);
        }
        let base = if sel { Style::default().fg(ON_CHIP).bg(ACCENT) } else { Style::default() };
        for dx in 0..r.w {
            buf.set_string(r.x + dx, y, " ", base);
        }
        let rw = right.as_ref().map(|s| unicode_width(s) as u16 + 1).unwrap_or(0);
        let room = r.w.saturating_sub(rw) as usize;
        buf.set_stringn(r.x, y, &left, room, if sel { base.add_modifier(Modifier::BOLD) } else { st });
        if let Some(b) = right {
            let x = r.x + r.w.saturating_sub(rw);
            buf.set_stringn(x, y, &b, rw as usize, if sel { base } else { badge_st });
        }
    }
}

fn line(row: &Row, v: &View) -> (String, Option<String>, Style, Style) {
    let plain = Style::default();
    match row {
        Row::Header(t) => (format!(" {t}"), None, Style::default().fg(DIM).add_modifier(Modifier::BOLD), plain),
        Row::Thread(k) => match v.threads.get(k) {
            Some(t) => {
                let pct = match (t.group, t.agent.as_ref().and_then(|a| a.percent)) {
                    (ThreadGroup::Working, Some(n)) => format!(" {n}%"),
                    _ => String::new(),
                };
                (
                    format!(" ≡ {}", t.title),
                    Some(format!("{}{pct}", group_label(t.group))),
                    plain,
                    Style::default().fg(group_color(t.group)),
                )
            }
            None => (format!(" ≡ {k}"), None, plain, plain),
        },
        Row::Machine(id) => match v.machines.iter().find(|m| &m.id == id) {
            Some(m) => {
                let on = m.connection == Connection::Connected;
                let tag = if m.local { " host" } else { "" };
                (
                    format!(" ■ {}{tag}", m.name),
                    Some(if on { "●".into() } else { "끊김".into() }),
                    Style::default().add_modifier(Modifier::BOLD),
                    Style::default().fg(if on { DONE } else { WARN }),
                )
            }
            None => (format!(" ■ {id}"), None, plain, plain),
        },
        Row::NewShell(_) => ("   + 새 셸".into(), None, Style::default().fg(DIM), plain),
        Row::Empty => ("   pane 없음".into(), None, Style::default().fg(FAINT), plain),
        Row::Pane(id) => match v.panes.get(id) {
            Some(p) => {
                let st = if p.state.is_running() { plain } else { Style::default().fg(DIM) };
                let (badge, color) = pane_badge(p).map_or((None, DIM), |(b, c)| (Some(b), c));
                (format!("   {}", pane_title(p, v.threads)), badge, st, Style::default().fg(color))
            }
            None => (format!("   {id}"), None, plain, plain),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_of(kinds: &[Row]) -> Vec<Row> {
        kinds.to_vec()
    }

    #[test]
    fn step_skips_headers_and_stops_at_ends() {
        let r = rows_of(&[
            Row::Header("Thread 1".into()),
            Row::Thread("p/t-0001".into()),
            Row::Machine("m".into()),
            Row::NewShell("m".into()),
            Row::Empty,
            Row::Machine("n".into()),
            Row::Pane("x".into()),
        ]);
        assert_eq!(first(&r), 1);
        assert_eq!(step(&r, 1, true), 3);
        assert_eq!(step(&r, 3, true), 6);
        assert_eq!(step(&r, 6, true), 6);
        assert_eq!(step(&r, 6, false), 3);
        assert_eq!(step(&r, 1, false), 1);
    }
}
