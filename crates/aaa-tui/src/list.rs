//! 왼쪽 목록(웹 목록과 같은 모양, §8.35): Thread 묶음과 머신별 pane, 상태 배지, 새 셸.
//!
//! host가 보낸 머신·pane 목록과 thread 사건으로만 만든다(TUI는 저장하지 않는다). 고르면 그 pane을 탭으로 연다.

use std::collections::HashMap;

use aaa_proto::layout::Rect;
use aaa_proto::{AgentActivity, Connection, MachineView, PaneView, Role, SessionState, ThreadGroup, ThreadView};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

use crate::theme::Palette;
use crate::{activity_text, unicode_width};

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

/// 웹 목록과 같은 순서: 끝나지 않은 thread(평가자 제외, 묶음 차례 → 같은 묶음은 최근 바뀐 순) → 머신마다 새 셸·pane
/// (평가자 제외) → 사후 평가자 pane(예전 상태 줄에 보이던 것, 웹 목록에는 없다)
pub fn rows(machines: &[MachineView], panes: &[PaneView], threads: &HashMap<String, ThreadView>) -> Vec<Row> {
    let mut out = vec![];
    let mut shown: Vec<&ThreadView> =
        threads.values().filter(|t| t.group != ThreadGroup::Resolved && t.role != Role::Evaluator).collect();
    // herdr-projects 사이드바는 pane을 묶음 rank(`hp_rank` 표지)로 세운다: 사람 차례가 먼저다(`Group::DISPLAY_ORDER`)
    shown.sort_by(|a, b| {
        (a.group.rank().cmp(&b.group.rank()))
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| thread_key(a).cmp(&thread_key(b)))
    });
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
    let mut evals: Vec<&PaneView> = panes.iter().filter(|p| p.role == Some(Role::Evaluator)).collect();
    evals.sort_by(|a, b| a.name.cmp(&b.name));
    if !evals.is_empty() {
        out.push(Row::Header("평가".into()));
        out.extend(evals.into_iter().map(|p| Row::Pane(p.id.clone())));
    }
    out
}

/// herdr `AgentStatus` 자리(상태 점의 모양과 색, `status_icon`·`status_color`)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dot {
    Working,
    Blocked,
    Done,
    Idle,
    Unknown,
}

impl Dot {
    /// herdr `status_icon`(Dots)
    pub fn icon(self) -> &'static str {
        match self {
            Dot::Working | Dot::Blocked | Dot::Done => "●",
            Dot::Idle => "○",
            Dot::Unknown => "·",
        }
    }

    /// herdr `status_color`
    pub fn color(self, p: &Palette) -> Color {
        match self {
            Dot::Working => p.yellow,
            Dot::Blocked => p.red,
            Dot::Done => p.teal,
            Dot::Idle => p.green,
            Dot::Unknown => p.overlay0,
        }
    }
}

/// pane 상태 점: harness가 일하면 working, 질문·오류는 blocked, 기다리면 idle, 셸·끝난 pane은 unknown
pub fn pane_dot(p: &PaneView) -> Dot {
    if !p.state.is_running() {
        return Dot::Unknown;
    }
    match p.agent.as_ref().map(|a| a.activity) {
        Some(AgentActivity::Working) => Dot::Working,
        Some(AgentActivity::Question | AgentActivity::Error) => Dot::Blocked,
        Some(AgentActivity::Idle | AgentActivity::Started) => Dot::Idle,
        Some(AgentActivity::Exited) | None => Dot::Unknown,
    }
}

/// thread 상태 점: 사람 차례 blocked, 검토 대기 done(herdr 「끝났고 아직 안 봄」 자리), 작업 중 working
pub fn thread_dot(t: &ThreadView) -> Dot {
    match t.group {
        ThreadGroup::WaitingOnYou => Dot::Blocked,
        ThreadGroup::ReadyForReview | ThreadGroup::Landing => Dot::Done,
        ThreadGroup::Working => Dot::Working,
        ThreadGroup::Idle => Dot::Idle,
        ThreadGroup::Resolved => Dot::Unknown,
    }
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
        ThreadGroup::Landing => "병합 중",
        ThreadGroup::Working => "작업 중",
        ThreadGroup::Idle => "쉼",
        ThreadGroup::Resolved => "끝남",
    }
}

fn group_color(g: ThreadGroup, p: &Palette) -> Color {
    match g {
        ThreadGroup::WaitingOnYou => p.red,
        ThreadGroup::ReadyForReview => p.teal,
        // herdr-projects popup `group_color`
        ThreadGroup::Landing => p.green,
        ThreadGroup::Working => p.yellow,
        ThreadGroup::Idle | ThreadGroup::Resolved => p.overlay0,
    }
}

/// pane 줄 제목에 남길 칸(이보다 좁아지면 배지의 계획·배경 수를 뺀다)
const TITLE_MIN: usize = 8;

/// pane 줄 오른쪽 배지(웹 목록과 같은 말). 실행 중이고 harness 상태가 없으면 없다. 계획 n/m·배경 작업 수는 `room`칸 안에
/// 들 때만 붙인다(좁은 목록에서 제목보다 먼저 빠진다).
pub fn pane_badge(p: &PaneView, pal: &Palette, room: usize) -> Option<(String, Color)> {
    if p.state.is_running() && p.connection == Connection::Disconnected {
        return Some(("끊김".into(), pal.peach));
    }
    match &p.state {
        SessionState::Running => p.agent.as_ref().map(|a| {
            let mut pct = a.percent.map(|n| format!(" {n}%")).unwrap_or_default();
            // 계획 n/m·배경 작업 수(웹 Agents 보기 배지와 같은 값, 차이 목록 S5). 폭이 애매한 기호 대신 글로 쓴다
            let extra = [
                a.plan.as_ref().filter(|p| p.total > 0).map(|p| format!(" {}/{}", p.done, p.total)),
                a.background.filter(|n| *n > 0).map(|n| format!(" 배경{n}")),
            ];
            for e in extra.into_iter().flatten() {
                if unicode_width(&format!("{}{pct}{e}", activity_text(a.activity))) <= room {
                    pct.push_str(&e);
                }
            }
            let color = match a.activity {
                AgentActivity::Working => pal.yellow,
                AgentActivity::Question | AgentActivity::Error => pal.red,
                _ => pal.overlay0,
            };
            (format!("{}{pct}", activity_text(a.activity)), color)
        }),
        SessionState::Exited { code } => {
            Some((code.map_or_else(|| "종료".into(), |c| format!("종료({c})")), pal.overlay0))
        }
        SessionState::Interrupted => Some(("중단".into(), pal.peach)),
        SessionState::Lost => Some(("유실".into(), pal.peach)),
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
    /// 휠로 민 첫 줄(없으면 고른 줄이 보이게 내린다)
    pub top: Option<usize>,
    pub pal: &'a Palette,
}

/// 줄들을 그리고 (줄 y, 줄 차례)들과 첫 보이는 줄을 준다(사이드바 바탕·구분선은 부르는 쪽).
/// 줄이 넘치면 고른 줄이 보이게 내린다(휠로 민 자리가 있으면 그 자리).
/// herdr 사이드바처럼 탐색 중 고른 줄은 `selection_bg`(없으면 `active_row_bg`), 지금 pane 줄은 `active_row_bg` 바탕이고
/// 이름은 `text` 굵게(아니면 `subtext0`)다(`workspace_selection_background`·`workspace_active_background`·`render_workspace_rows`).
pub fn render(buf: &mut Buffer, r: Rect, rows: &[Row], v: &View) -> (Vec<(u16, usize)>, usize) {
    let p = v.pal;
    let selection = if p.selection_bg == Color::Reset { p.active_row_bg } else { p.selection_bg };
    let active = if v.focused && p.selection_bg == Color::Reset { p.sidebar_bg } else { p.active_row_bg };
    let skip = match v.top {
        Some(t) => t.min(rows.len().saturating_sub(r.h as usize)),
        None => (v.sel + 1).saturating_sub(r.h as usize),
    };
    let mut placed = Vec::new();
    for (n, row) in rows.iter().enumerate().skip(skip).take(r.h as usize) {
        let y = r.y + (n - skip) as u16;
        placed.push((y, n));
        let sel = n == v.sel && v.focused;
        let current = is_current(row, v);
        let bg = if sel {
            Some(selection)
        } else if current {
            Some(active)
        } else {
            None
        };
        // 배지가 쓸 수 있는 칸: 앞 점(3칸)·사이·끝 칸(2칸)과 제목 TITLE_MIN칸을 뺀 나머지
        let room = (r.w as usize).saturating_sub(5 + TITLE_MIN);
        let (left, right) = line(row, v, current || sel, room);
        // 오른쪽 글(상태)은 이름과 한 칸 띄우고 끝 한 칸을 비운다. 넘치는 이름은 herdr `truncate_end`처럼 「…」로 자른다
        // (herdr `resolved_token_spans`: 토큰 사이 구분자, 잘린 토큰 끝 「…」)
        let rw = right.as_ref().map(|(s, _)| unicode_width(s) as u16).unwrap_or(0);
        let end = r.x + r.w.saturating_sub(if rw > 0 { rw + 2 } else { 0 });
        let overflow = left.iter().map(|(t, _)| unicode_width(t)).sum::<usize>() > (end - r.x) as usize;
        let mut x = r.x;
        for (t, st) in &left {
            if x >= end {
                break;
            }
            let room = (end - x) as usize;
            let t = if overflow { crate::chrome::truncate_end(t, room) } else { t.clone() };
            x = buf.set_stringn(x, y, &t, room, *st).0;
        }
        if let Some((t, st)) = right {
            buf.set_stringn(end + 1, y, &t, rw as usize, st);
        }
        if let Some(c) = bg {
            for dx in 0..r.w {
                if let Some(cell) = buf.cell_mut((r.x + dx, y)) {
                    cell.set_bg(c);
                }
            }
        }
    }
    (placed, skip)
}

/// 이 줄이 지금 포커스 pane(또는 그 pane의 thread)인가
fn is_current(row: &Row, v: &View) -> bool {
    let Some(cur) = v.current else { return false };
    match row {
        Row::Pane(p) => p == cur,
        Row::Thread(k) => v.threads.get(k).and_then(|t| t.pane.as_deref()) == Some(cur),
        _ => false,
    }
}

/// 줄의 왼쪽 조각들과 오른쪽 표시. herdr 기본 줄(`state_icon` + 이름, 머신은 ` ▾ 이름`과 연결 점)을 한 줄로 쓴다.
/// 글과 그 모양
type Span = (String, Style);

fn line(row: &Row, v: &View, highlighted: bool, room: usize) -> (Vec<Span>, Option<Span>) {
    let pal = v.pal;
    let name = if highlighted {
        Style::default().fg(pal.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(pal.subtext0)
    };
    let faint = Style::default().fg(pal.overlay0);
    let dot = |d: Dot| (format!(" {} ", d.icon()), Style::default().fg(d.color(pal)));
    match row {
        Row::Header(t) => (vec![(format!(" {t}"), faint.add_modifier(Modifier::BOLD))], None),
        Row::Thread(k) => match v.threads.get(k) {
            Some(t) => {
                let pct = match (t.group, t.agent.as_ref().and_then(|a| a.percent)) {
                    (ThreadGroup::Working, Some(n)) => format!(" {n}%"),
                    _ => String::new(),
                };
                (
                    vec![dot(thread_dot(t)), (t.title.clone(), name)],
                    Some((format!("{}{pct}", group_label(t.group)), Style::default().fg(group_color(t.group, pal)))),
                )
            }
            None => (vec![dot(Dot::Unknown), (k.clone(), name)], None),
        },
        Row::Machine(id) => {
            let m = v.machines.iter().find(|m| &m.id == id);
            let label = m.map_or(id.as_str(), |m| m.name.as_str());
            // herdr `render_endpoint_row`: 이 머신(host)은 표시 없음, 붙은 머신은 초록 점, 끊긴 머신은 노랑 반달
            let signal = match m {
                Some(m) if m.connection != Connection::Connected => Some(("◐ 끊김".to_string(), pal.yellow)),
                Some(m) if !m.local => Some(("●".to_string(), pal.green)),
                _ => None,
            };
            (
                vec![(format!(" ▾ {label}"), Style::default().fg(pal.text).add_modifier(Modifier::BOLD))],
                signal.map(|(t, c)| (t, Style::default().fg(c))),
            )
        }
        Row::NewShell(_) => (vec![(" + 새 셸".into(), faint)], None),
        Row::Empty => (vec![("   pane 없음".into(), faint)], None),
        Row::Pane(id) => match v.panes.get(id) {
            Some(p) => {
                let st = if p.state.is_running() { name } else { faint };
                (
                    vec![dot(pane_dot(p)), (pane_title(p, v.threads), st)],
                    pane_badge(p, pal, room).map(|(b, c)| (b, Style::default().fg(c))),
                )
            }
            None => (vec![dot(Dot::Unknown), (id.clone(), name)], None),
        },
    }
}

/// 접힌 사이드바(herdr `render_collapsed_sidebar`): 고를 수 있는 thread·pane 줄마다 번호와 상태 점
pub fn render_compact(buf: &mut Buffer, r: Rect, rows: &[Row], v: &View) -> Vec<(u16, usize)> {
    let p = v.pal;
    let selection = if p.selection_bg == Color::Reset { p.active_row_bg } else { p.selection_bg };
    let active = if v.focused && p.selection_bg == Color::Reset { p.sidebar_bg } else { p.active_row_bg };
    let items: Vec<(usize, &Row)> =
        rows.iter().enumerate().filter(|(_, r)| matches!(r, Row::Thread(_) | Row::Pane(_))).collect();
    let sel_at = items.iter().position(|(n, _)| *n == v.sel).unwrap_or(0);
    let skip = (sel_at + 1).saturating_sub(r.h as usize);
    let mut placed = Vec::new();
    for (i, (n, row)) in items.iter().enumerate().skip(skip).take(r.h as usize) {
        let y = r.y + (i - skip) as u16;
        placed.push((y, *n));
        let sel = *n == v.sel && v.focused;
        let current = is_current(row, v);
        let (bg, num) = if sel {
            (Some(selection), Style::default().fg(p.overlay1))
        } else if current {
            (Some(active), Style::default().fg(p.text))
        } else {
            (None, Style::default().fg(p.overlay0))
        };
        let d = match row {
            Row::Thread(k) => v.threads.get(k).map_or(Dot::Unknown, thread_dot),
            Row::Pane(id) => v.panes.get(id).map_or(Dot::Unknown, pane_dot),
            _ => Dot::Unknown,
        };
        buf.set_stringn(r.x, y, format!("{:<2}", i + 1), r.w.min(2) as usize, num);
        if r.w > 2 {
            buf.set_stringn(r.x + 2, y, d.icon(), (r.w - 2) as usize, Style::default().fg(d.color(p)));
        }
        if let Some(c) = bg {
            for dx in 0..r.w {
                if let Some(cell) = buf.cell_mut((r.x + dx, y)) {
                    cell.set_bg(c);
                }
            }
        }
    }
    placed
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

    fn thread(id: &str, group: ThreadGroup, updated_at_ms: u64) -> ThreadView {
        ThreadView {
            project: "p".into(),
            id: id.into(),
            title: id.into(),
            machine: "m".into(),
            profile: "claude".into(),
            role: Role::Worker,
            thread_kind: aaa_proto::ThreadKind::Dir,
            status: aaa_proto::ThreadStatus::Open,
            group,
            brief: aaa_proto::BriefState::Delivered,
            pane: None,
            cwd: None,
            work_dir: None,
            branch: None,
            pr: None,
            report_hash: None,
            acked_hash: None,
            next: vec![],
            copy: None,
            error: None,
            agent: None,
            created_at_ms: 0,
            updated_at_ms,
            warnings: vec![],
            project_status: Default::default(),
        }
    }

    /// thread 줄은 묶음 차례(herdr-projects `DISPLAY_ORDER`: 사람 차례 → 검토 → 병합 중 → 작업 → 쉼)로, 같은 묶음 안은
    /// 최근 바뀐 것이 먼저다(차이 목록 3장 C1). 끝난 thread는 목록에 없다
    #[test]
    fn threads_follow_the_display_order() {
        let threads: HashMap<String, ThreadView> = [
            thread("t-idle", ThreadGroup::Idle, 9),
            thread("t-work-old", ThreadGroup::Working, 1),
            thread("t-work-new", ThreadGroup::Working, 8),
            thread("t-landing", ThreadGroup::Landing, 2),
            thread("t-review", ThreadGroup::ReadyForReview, 3),
            thread("t-you", ThreadGroup::WaitingOnYou, 4),
            thread("t-done", ThreadGroup::Resolved, 10),
        ]
        .into_iter()
        .map(|t| (thread_key(&t), t))
        .collect();
        let got: Vec<Row> = rows(&[], &[], &threads);
        let want: Vec<Row> = std::iter::once(Row::Header("Thread 6".into()))
            .chain(
                ["t-you", "t-review", "t-landing", "t-work-new", "t-work-old", "t-idle"]
                    .map(|id| Row::Thread(format!("p/{id}"))),
            )
            .collect();
        assert_eq!(got, want);
    }

    /// 잘린 이름은 「…」로 끝나고 오른쪽 글과 한 칸 띄운다, 끝 한 칸은 빈다(t-0337 QA Q13, herdr `truncate_end`·구분자)
    #[test]
    fn long_name_is_cut_with_ellipsis_before_the_right_text() {
        let machines = [MachineView {
            id: "m".into(),
            name: "studio-very-long-machine".into(),
            local: false,
            connection: Connection::Disconnected,
            os: None,
            boot_id: None,
            dev_mode: false,
            last_seen_ms: None,
            health: None,
            clock_skew_ms: None,
            problems: vec![],
        }];
        let (panes, threads) = (HashMap::new(), HashMap::new());
        let pal = Palette::catppuccin();
        let v = View {
            machines: &machines,
            panes: &panes,
            threads: &threads,
            current: None,
            sel: 0,
            focused: false,
            top: None,
            pal: &pal,
        };
        let line = |w: u16| {
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, w, 1));
            render(&mut buf, Rect { x: 0, y: 0, w, h: 1 }, &[Row::Machine("m".into())], &v);
            // 넓은 글자 뒤 칸은 건너뛴다(터미널에 보이는 대로)
            let (mut out, mut x) = (String::new(), 0);
            while x < w {
                let sym = buf[(x, 0)].symbol().to_string();
                x += (unicode_width(&sym) as u16).max(1);
                out.push_str(&sym);
            }
            out
        };
        assert_eq!(line(20), " ▾ studio-v… ◐ 끊김 ");
        // 넉넉하면 그대로
        assert_eq!(line(40), format!(" ▾ studio-very-long-machine{}◐ 끊김 ", " ".repeat(6)));
    }

    #[test]
    fn pane_badge_carries_plan_and_background_counts() {
        let pal = Palette::catppuccin();
        let mut p = PaneView {
            id: "p".into(),
            machine_id: "m".into(),
            machine: "m".into(),
            name: "p".into(),
            argv: vec![],
            cwd: None,
            state: SessionState::Running,
            connection: Connection::Connected,
            readonly: false,
            cols: 80,
            rows: 24,
            lease: None,
            role: None,
            agent: Some(Box::new(aaa_proto::AgentStatus {
                activity: AgentActivity::Working,
                since_ms: 1,
                harness: "claude".into(),
                message: None,
                percent: Some(40),
                progress: None,
                progress_at_ms: None,
                harness_session: None,
                seen: true,
                plan: Some(aaa_proto::PlanSummary { done: 1, total: 3, current: None }),
                background: Some(2),
            })),
            label: None,
            restore_error: None,
            replaced: Vec::new(),
        };
        assert_eq!(pane_badge(&p, &pal, 40).unwrap().0, "작업 중 40% 1/3 배경2");
        // 좁으면 계획·배경 수가 제목보다 먼저 빠진다(들어가는 것만)
        assert_eq!(pane_badge(&p, &pal, 15).unwrap().0, "작업 중 40% 1/3");
        assert_eq!(pane_badge(&p, &pal, 14).unwrap().0, "작업 중 40%");
        assert_eq!(pane_badge(&p, &pal, 0).unwrap().0, "작업 중 40%");
        // 계획·배경 작업이 없으면 예전 그대로
        let a = p.agent.as_mut().unwrap();
        (a.plan, a.background, a.percent) = (None, None, None);
        assert_eq!(pane_badge(&p, &pal, 40).unwrap().0, "작업 중");
    }
}
