//! 왼쪽 목록(웹 목록과 같은 모양, §8.35·차이 목록 L1·U4~U8). herdr 사이드바처럼 위는 workspace(` spaces`, 맨 아랫줄 ` new`),
//! 아래는 ` agents` 구역이고, agents에는 머신마다 머리(고르면 새 탭·workspace를 열 머신), 그 머신의 프로젝트 머리와 thread 두 줄
//! (herdr-projects `thread_display` 「id · 제목」, 둘째 줄 `state_line`), thread에 딸리지 않은 pane, 상태 점과 배지.
//!
//! host가 보낸 머신·pane 목록과 thread 사건으로만 만든다(TUI는 저장하지 않는다). 고르면 그 pane을 탭으로 연다.

use std::collections::HashMap;

use aaa_config::StatusIndicators;
use aaa_proto::layout::Rect;
use aaa_proto::{
    AgentActivity, Connection, MachineView, PaneView, Role, SessionState, ThreadGroup, ThreadStatus, ThreadView,
};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

use crate::theme::Palette;
use crate::{activity_text, unicode_width};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Header(String),
    /// thread 키(`project/id`)
    Thread(String),
    /// thread 줄의 둘째 줄(상태 말, 고르지 않고 누르면 그 thread)
    ThreadSub(String),
    /// 머신 머리(머신 ID): 고르면 새 탭·workspace를 그 머신에 연다(herdr endpoint 줄 누르기)
    Machine(String),
    /// 그 머신의 프로젝트 묶음 머리(머신 ID, 프로젝트)
    Project(String, String),
    Pane(String),
    Empty,
    /// spaces 구역의 workspace(이름)
    Space(String),
}

impl Row {
    pub fn selectable(&self) -> bool {
        matches!(self, Row::Thread(_) | Row::Machine(_) | Row::Pane(_) | Row::Space(_))
    }

    /// 누르면 고르는 줄: thread 둘째 줄은 그 thread
    pub fn target(&self) -> Row {
        match self {
            Row::ThreadSub(k) => Row::Thread(k.clone()),
            r => r.clone(),
        }
    }
}

pub fn thread_key(t: &ThreadView) -> String {
    format!("{}/{}", t.project, t.id)
}

/// 그 머신의 thread인가(웹 `threadMachine`): pane이 있으면 pane의 머신, 없으면 thread의 머신 이름·ID
fn on_machine(t: &ThreadView, m: &MachineView, pane: Option<&PaneView>) -> bool {
    match pane {
        Some(p) => p.machine_id == m.id,
        None => t.machine == m.name || t.machine == m.id,
    }
}

/// 목록에 thread 줄로 보이는가: 끝난 것·평가자는 뺀다(웹 `machineRoster`)
fn listed(t: &ThreadView) -> bool {
    t.group != ThreadGroup::Resolved && t.role != Role::Evaluator
}

/// herdr-projects `project_line`(웹 `projectLine`): 「사람 차례 2 · 작업 중 3 · 병합 중 1」, 없으면 「쉼」
pub fn project_line(groups: &[ThreadGroup]) -> String {
    let need = groups.iter().filter(|g| matches!(g, ThreadGroup::WaitingOnYou | ThreadGroup::ReadyForReview)).count();
    let mut parts = vec![];
    if need > 0 {
        parts.push(format!("사람 차례 {need}"));
    }
    for (g, label) in [(ThreadGroup::Working, "작업 중"), (ThreadGroup::Landing, "병합 중")] {
        let n = groups.iter().filter(|x| **x == g).count();
        if n > 0 {
            parts.push(format!("{label} {n}"));
        }
    }
    if parts.is_empty() { "쉼".to_string() } else { parts.join(" · ") }
}

/// 웹 목록(`machineRoster`)과 같은 차례: 머신마다 머리, 그 머신 thread를 묶음 rank·최근 바뀐 순으로 늘어놓고 처음 나온
/// 프로젝트 차례로 묶은 프로젝트 머리와 thread 두 줄, 그다음 thread에 딸리지 않은 pane(평가자 제외). 끝난·평가자 thread는 뺀다.
/// 맨 아래는 사후 평가자 pane(웹 목록에는 없다).
pub fn rows(machines: &[MachineView], panes: &[PaneView], threads: &HashMap<String, ThreadView>) -> Vec<Row> {
    let mut out = vec![];
    let mut live: Vec<&ThreadView> = threads.values().filter(|t| listed(t)).collect();
    // herdr-projects `Group::rank`(`DISPLAY_ORDER`): 사람 차례 → 검토 대기 → 병합 중 → 작업 중 → 쉼
    live.sort_by(|a, b| {
        a.group
            .rank()
            .cmp(&b.group.rank())
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| thread_key(a).cmp(&thread_key(b)))
    });
    for m in machines {
        out.push(Row::Machine(m.id.clone()));
        let pane_of = |t: &ThreadView| t.pane.as_deref().and_then(|id| panes.iter().find(|p| p.id == id));
        let mine: Vec<&&ThreadView> = live.iter().filter(|t| on_machine(t, m, pane_of(t))).collect();
        let mut projects: Vec<&str> = vec![];
        for t in &mine {
            if !projects.contains(&t.project.as_str()) {
                projects.push(&t.project);
            }
        }
        for project in &projects {
            out.push(Row::Project(m.id.clone(), project.to_string()));
            for t in mine.iter().filter(|t| t.project == *project) {
                out.push(Row::Thread(thread_key(t)));
                out.push(Row::ThreadSub(thread_key(t)));
            }
        }
        let thread_panes: Vec<&str> = mine.iter().filter_map(|t| t.pane.as_deref()).collect();
        let loose: Vec<&PaneView> = panes
            .iter()
            .filter(|p| {
                p.machine_id == m.id && p.role != Some(Role::Evaluator) && !thread_panes.contains(&p.id.as_str())
            })
            .collect();
        if mine.is_empty() && loose.is_empty() {
            out.push(Row::Empty);
        }
        out.extend(loose.into_iter().map(|p| Row::Pane(p.id.clone())));
    }
    let mut evals: Vec<&PaneView> = panes.iter().filter(|p| p.role == Some(Role::Evaluator)).collect();
    evals.sort_by(|a, b| a.name.cmp(&b.name));
    if !evals.is_empty() {
        out.push(Row::Header("평가".into()));
        out.extend(evals.into_iter().map(|p| Row::Pane(p.id.clone())));
    }
    out
}

/// 5분 넘게 진행 보고가 없으면 작업 중 줄이 「n분 조용」(herdr-projects `QUIET_SECS`, 웹 `QUIET_MS`)
pub const QUIET_MS: u64 = 300_000;

/// PR 주소 끝의 번호(herdr-projects `pr_number`, 웹 `prNumber`)
fn pr_number(url: &str) -> Option<&str> {
    let n = url.rsplit('/').next().unwrap_or("");
    (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())).then_some(n)
}

/// thread 둘째 줄 상태 말: 묶음 말과 사실 하나(herdr-projects `state_line`, 웹 `stateLine`). 진행률은 100 미만만 「~n%」.
/// 검토·병합 중의 사실은 따라가는 PR이 있으면 「PR #n」, 없으면 「보고」다(웹과 같게, 차이 목록 L3 241줄 TUI 몫, plan §8.190).
pub fn state_line(t: &ThreadView, pane_open: bool, now_ms: u64) -> String {
    let a = t.agent.as_deref();
    let pct = a.and_then(|a| a.percent).filter(|n| *n < 100).map(|n| format!("~{n}%"));
    let fact = match t.group {
        ThreadGroup::WaitingOnYou if !pane_open => Some("pane 닫힘".to_string()),
        ThreadGroup::WaitingOnYou if t.status == ThreadStatus::Failed => Some("실패".to_string()),
        ThreadGroup::WaitingOnYou if a.is_some_and(|a| a.activity == AgentActivity::Question) => {
            Some("확인창".to_string())
        }
        ThreadGroup::WaitingOnYou => pct,
        ThreadGroup::ReadyForReview | ThreadGroup::Landing => Some(
            t.pr.as_deref()
                .and_then(|pr| pr_number(&pr.url))
                .map_or_else(|| "보고".to_string(), |n| format!("PR #{n}")),
        ),
        ThreadGroup::Working => {
            let reported = a.and_then(|a| a.progress_at_ms);
            let fresh = reported.is_some_and(|at| now_ms.saturating_sub(at) < QUIET_MS);
            let since = now_ms.saturating_sub(reported.unwrap_or(t.updated_at_ms));
            match pct {
                Some(p) if fresh => Some(p),
                _ if since >= QUIET_MS => Some(format!("{}분 조용", since / 60_000)),
                _ => None,
            }
        }
        _ => None,
    };
    let word = group_label(t.group);
    fact.map_or_else(|| word.to_string(), |f| format!("{word} · {f}"))
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
    /// herdr `status_icon`: 색 점(`dots`) 또는 상태마다 다른 기호(`symbols`, `[tui] status_indicators`)
    pub fn icon(self, style: StatusIndicators) -> &'static str {
        match (style, self) {
            (StatusIndicators::Dots, Dot::Working | Dot::Blocked | Dot::Done) => "●",
            (StatusIndicators::Symbols, Dot::Blocked) => "×",
            (StatusIndicators::Symbols, Dot::Working) => "◐",
            (StatusIndicators::Symbols, Dot::Done) => "✓",
            (_, Dot::Idle) => "○",
            (_, Dot::Unknown) => "·",
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

/// 점이 얼마나 급한가(herdr-web-ui `rollupStatus`: blocked > done > working > idle)
fn urgency(d: Dot) -> u8 {
    match d {
        Dot::Blocked => 4,
        Dot::Done => 3,
        Dot::Working => 2,
        Dot::Idle => 1,
        Dot::Unknown => 0,
    }
}

/// thread 줄 점: thread 묶음 점과 그 pane 상태 점(herdr-projects 줄은 herdr가 pane agent 상태로 그리는 `state_icon`,
/// 웹 thread 줄은 오른쪽 `PaneState`) 중 급한 쪽. 확인창이 뜨면 묶음이 사람 차례로 바뀌기(30초) 전에도 빨강이고,
/// 사람 차례 thread의 harness가 쉬고 있어도 초록이 되지 않는다.
pub fn thread_row_dot(t: &ThreadView, panes: &HashMap<String, PaneView>) -> Dot {
    let group = thread_dot(t);
    match t.pane.as_deref().and_then(|id| panes.get(id)).map(pane_dot) {
        Some(d) if urgency(d) > urgency(group) => d,
        _ => group,
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
    /// 탐색 중 고른 줄(spaces 구역의 줄을 고르면 없다)
    pub sel: Option<usize>,
    pub focused: bool,
    /// 휠로 민 첫 줄(없으면 고른 줄이 보이게 내린다)
    pub top: Option<usize>,
    pub pal: &'a Palette,
    /// 상태 점 모양(`[tui] status_indicators`)
    pub style: StatusIndicators,
    /// 지금 시각(「n분 조용」)
    pub now_ms: u64,
}

/// 줄들을 그리고 (줄 y, 줄 차례)들과 첫 보이는 줄을 준다(사이드바 바탕·구분선은 부르는 쪽).
/// 줄이 넘치면 고른 줄이 보이게 내린다(휠로 민 자리가 있으면 그 자리).
/// herdr 사이드바처럼 탐색 중 고른 줄은 `selection_bg`(없으면 `active_row_bg`), 지금 pane 줄은 `active_row_bg` 바탕이고
/// 이름은 `text` 굵게(아니면 `subtext0`)다(`workspace_selection_background`·`workspace_active_background`·`render_workspace_rows`).
pub fn render(buf: &mut Buffer, r: Rect, rows: &[Row], v: &View) -> (Vec<(u16, usize)>, usize) {
    let p = v.pal;
    let selection = if p.selection_bg == Color::Reset { p.active_row_bg } else { p.selection_bg };
    let active = if v.focused && p.selection_bg == Color::Reset { p.sidebar_bg } else { p.active_row_bg };
    // 고른 thread 줄은 둘째 줄까지 보이게
    let sel_end = match v.sel {
        Some(n) if matches!(rows.get(n + 1), Some(Row::ThreadSub(_))) => n + 2,
        Some(n) => n + 1,
        None => 0,
    };
    let skip = match v.top {
        Some(t) => t.min(rows.len().saturating_sub(r.h as usize)),
        None => sel_end.saturating_sub(r.h as usize),
    };
    let mut placed = Vec::new();
    for (n, row) in rows.iter().enumerate().skip(skip).take(r.h as usize) {
        let y = r.y + (n - skip) as u16;
        placed.push((y, n));
        let sel = v.focused && v.sel.and_then(|n| rows.get(n)).is_some_and(|s| *s == row.target());
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
        let (left, mut right) = line(row, v, current || sel, room);
        // 오른쪽 글(프로젝트 셈 「사람 차례 2 · 작업 중 1」)이 이름 자리를 먹으면 ` · ` 단위로 뒤를 덜어 낸다
        let lw: usize = left.iter().map(|(t, _)| unicode_width(t)).sum();
        if let Some((t, _)) = right.as_mut() {
            while lw + unicode_width(t) + 2 > r.w as usize {
                match t.rsplit_once(" · ") {
                    Some((head, _)) => *t = head.to_string(),
                    None => break,
                }
            }
        }
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
        Row::Thread(k) | Row::ThreadSub(k) => v.threads.get(k).and_then(|t| t.pane.as_deref()) == Some(cur),
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
    let dot = |d: Dot| (format!(" {} ", d.icon(v.style)), Style::default().fg(d.color(pal)));
    match row {
        Row::Header(t) => (vec![(format!(" {t}"), faint.add_modifier(Modifier::BOLD))], None),
        // 프로젝트 머리(웹 project-group): 굵은 이름, 오른쪽은 그 머신 이 프로젝트 thread 셈(웹 둘째 줄 `projectLine`)
        Row::Project(machine, project) => {
            let groups: Vec<ThreadGroup> = match v.machines.iter().find(|m| m.id == *machine) {
                Some(m) => v
                    .threads
                    .values()
                    .filter(|t| t.project == *project && listed(t))
                    .filter(|t| on_machine(t, m, t.pane.as_deref().and_then(|id| v.panes.get(id))))
                    .map(|t| t.group)
                    .collect(),
                None => vec![],
            };
            let need = groups.iter().any(|g| matches!(g, ThreadGroup::WaitingOnYou | ThreadGroup::ReadyForReview));
            let color = if need { group_color(ThreadGroup::WaitingOnYou, pal) } else { pal.overlay0 };
            (
                vec![(format!(" {project}"), Style::default().fg(pal.text).add_modifier(Modifier::BOLD))],
                Some((project_line(&groups), Style::default().fg(color))),
            )
        }
        // thread 줄(herdr-projects `thread_display`): 상태 점과 「id · 제목」, 둘째 줄에 상태 말(묶음 색)
        Row::Thread(k) => match v.threads.get(k) {
            Some(t) => (
                vec![("  ".into(), name), dot(thread_row_dot(t, v.panes)), (format!("{} · {}", t.id, t.title), name)],
                None,
            ),
            None => (vec![("  ".into(), name), dot(Dot::Unknown), (k.clone(), name)], None),
        },
        Row::ThreadSub(k) => match v.threads.get(k) {
            Some(t) => {
                let open = t.pane.as_deref().and_then(|id| v.panes.get(id)).is_some_and(|p| p.state.is_running());
                (
                    vec![(
                        format!("     {}", state_line(t, open, v.now_ms)),
                        Style::default().fg(group_color(t.group, pal)),
                    )],
                    None,
                )
            }
            None => (vec![], None),
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
        Row::Empty => (vec![("   pane 없음".into(), faint)], None),
        Row::Space(_) => (vec![], None),
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

/// spaces 구역의 workspace 한 줄(herdr `ClientShellWorkspace`의 이름과 `agent_status`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Space {
    pub name: String,
    pub dot: Dot,
}

/// workspace 상태 점: 그 pane들 중 가장 급한 pane 상태(herdr `agent_status`, `status_priority` 차례)
pub fn space_dot<'a>(panes: impl IntoIterator<Item = &'a PaneView>) -> Dot {
    panes.into_iter().map(pane_dot).max_by_key(|d| urgency(*d)).unwrap_or(Dot::Unknown)
}

/// 펼친 사이드바의 두 구역 높이(herdr `sidebar_section_heights`): 6줄 밑이면 반씩(위가 하나 더), 아니면 `split` 비율로
/// 나누되 위·아래 다 3줄 넘게
pub fn section_heights(total: u16, split: f32) -> (u16, u16) {
    if total == 0 {
        return (0, 0);
    }
    if total < 6 {
        let top = total.div_ceil(2);
        return (top, total - top);
    }
    let top = ((f32::from(total) * split.clamp(0.1, 0.9)).round() as u16).clamp(3, total - 3);
    (top, total - top)
}

/// spaces 구역(herdr `render_sidebar` 위 구역·`render_workspace_rows`): ` spaces` 머리와 빈 줄, workspace마다
/// ` <점> 이름`, 맨 아랫줄은 비운다(` new`·`menu` 자리, `render_spaces_footer`). 지금 workspace는 `active_row_bg`·이름 `text` 굵게(아니면
/// `subtext0`), 탐색 중 고른 줄은 `selection_bg`. 넘치면 고른 줄(없으면 지금 줄)이 보이게 내린다. (줄 y, 이름)들을 준다.
#[allow(clippy::too_many_arguments)]
pub fn render_spaces(
    buf: &mut Buffer,
    r: Rect,
    spaces: &[Space],
    current: &str,
    sel: Option<&str>,
    navigating: bool,
    p: &Palette,
    style: StatusIndicators,
) -> Vec<(u16, String)> {
    if r.h == 0 || r.w == 0 {
        return vec![];
    }
    buf.set_stringn(r.x, r.y, " spaces", r.w as usize, Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD));
    let body = Rect { y: r.y + 2, h: r.h.saturating_sub(3), ..r };
    let selection = if p.selection_bg == Color::Reset { p.active_row_bg } else { p.selection_bg };
    let active = if navigating && p.selection_bg == Color::Reset { p.sidebar_bg } else { p.active_row_bg };
    let keep = spaces
        .iter()
        .position(|s| Some(s.name.as_str()) == sel)
        .or_else(|| spaces.iter().position(|s| s.name == current));
    let skip = keep.map_or(0, |k| (k + 1).saturating_sub(body.h as usize));
    let mut placed = vec![];
    for (i, sp) in spaces.iter().enumerate().skip(skip).take(body.h as usize) {
        let y = body.y + (i - skip) as u16;
        placed.push((y, sp.name.clone()));
        let focused = sp.name == current;
        let selected = navigating && Some(sp.name.as_str()) == sel;
        let name = if focused {
            Style::default().fg(p.text).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.subtext0)
        };
        // herdr: 첫 줄은 한 칸 들여 상태 점, 이름. 끝 두 칸은 비운다
        let end = r.x + r.w.saturating_sub(2);
        let x = buf
            .set_stringn(
                r.x + 1,
                y,
                sp.dot.icon(style),
                end.saturating_sub(r.x + 1) as usize,
                Style::default().fg(sp.dot.color(p)),
            )
            .0;
        let x = buf.set_stringn(x, y, " ", end.saturating_sub(x) as usize, name).0;
        let room = end.saturating_sub(x) as usize;
        buf.set_stringn(x, y, crate::chrome::truncate_end(&sp.name, room), room, name);
        let bg = if selected {
            Some(selection)
        } else if focused {
            Some(active)
        } else {
            None
        };
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

/// spaces 구역 맨 아랫줄(herdr `render_sidebar`·`render_endpoint_workspaces` footer, 마우스를 받을 때만): 왼쪽 새 workspace
/// ` new`(머신이 여럿이면 ` new · 머신`), 오른쪽 전체 메뉴 `menu`(`overlay0`). 누를 칸 둘을 준다(herdr: ` new`는 5칸, 머신이
/// 붙으면 글 폭, `menu`는 오른쪽 6칸).
pub fn render_spaces_footer(buf: &mut Buffer, r: Rect, machine: Option<&str>, p: &Palette) -> Option<(Rect, Rect)> {
    if r.h == 0 || r.w == 0 {
        return None;
    }
    let (label, w) = match machine {
        Some(m) => {
            let label = format!(" new · {m}");
            let w = unicode_width(&label) as u16;
            (label, w)
        }
        None => (" new".to_string(), 5),
    };
    let y = r.y + r.h - 1;
    let faint = Style::default().fg(p.overlay0);
    buf.set_stringn(r.x, y, &label, r.w as usize, faint);
    let text = 4.min(r.w);
    buf.set_stringn(r.x + r.w - text, y, "menu", text as usize, faint);
    let menu = 6.min(r.w);
    Some((Rect { x: r.x, y, w: w.min(r.w), h: 1 }, Rect { x: r.x + r.w - menu, y, w: menu, h: 1 }))
}

/// agents 구역 머리(herdr `render_agent_panel_header`): 구분선 `─`(`surface_dim`), ` agents`(`overlay0` 굵게)와 오른쪽에
/// 보기 이름. aaa 목록은 herdr-projects 기본 보기(`default_view` label 「projects」)에 해당해 accent 굵게 쓴다.
/// 목록 칸(머리 세 줄 아래)을 준다.
pub fn render_agents_header(buf: &mut Buffer, r: Rect, p: &Palette) -> Rect {
    if r.h == 0 {
        return Rect { h: 0, ..r };
    }
    buf.set_stringn(r.x, r.y, "─".repeat(r.w as usize), r.w as usize, Style::default().fg(p.surface_dim));
    if r.h >= 2 {
        buf.set_stringn(
            r.x,
            r.y + 1,
            " agents",
            r.w as usize,
            Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD),
        );
        let label = "projects";
        let w = (unicode_width(label) as u16).min(r.w);
        buf.set_stringn(
            r.x + r.w - w,
            r.y + 1,
            label,
            w as usize,
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        );
    }
    Rect { y: r.y + 3.min(r.h), h: r.h.saturating_sub(3), ..r }
}

/// 접힌 사이드바(herdr `render_collapsed_sidebar`): 고를 수 있는 thread·pane 줄마다 번호와 상태 점
pub fn render_compact(buf: &mut Buffer, r: Rect, rows: &[Row], v: &View) -> Vec<(u16, usize)> {
    let p = v.pal;
    let selection = if p.selection_bg == Color::Reset { p.active_row_bg } else { p.selection_bg };
    let active = if v.focused && p.selection_bg == Color::Reset { p.sidebar_bg } else { p.active_row_bg };
    let items: Vec<(usize, &Row)> =
        rows.iter().enumerate().filter(|(_, r)| matches!(r, Row::Thread(_) | Row::Pane(_))).collect();
    let sel_at = items.iter().position(|(n, _)| Some(*n) == v.sel).unwrap_or(0);
    let skip = (sel_at + 1).saturating_sub(r.h as usize);
    let mut placed = Vec::new();
    for (i, (n, row)) in items.iter().enumerate().skip(skip).take(r.h as usize) {
        let y = r.y + (i - skip) as u16;
        placed.push((y, *n));
        let sel = Some(*n) == v.sel && v.focused;
        let current = is_current(row, v);
        let (bg, num) = if sel {
            (Some(selection), Style::default().fg(p.overlay1))
        } else if current {
            (Some(active), Style::default().fg(p.text))
        } else {
            (None, Style::default().fg(p.overlay0))
        };
        let d = match row {
            Row::Thread(k) => v.threads.get(k).map_or(Dot::Unknown, |t| thread_row_dot(t, v.panes)),
            Row::Pane(id) => v.panes.get(id).map_or(Dot::Unknown, pane_dot),
            _ => Dot::Unknown,
        };
        buf.set_stringn(r.x, y, format!("{:<2}", i + 1), r.w.min(2) as usize, num);
        if r.w > 2 {
            buf.set_stringn(r.x + 2, y, d.icon(v.style), (r.w - 2) as usize, Style::default().fg(d.color(p)));
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

    /// herdr `status_icon`: 색 점은 `● ● ● ○ ·`, 기호는 `× ◐ ✓ ○ ·`(확인 대기·작업·끝남·쉼·모름)
    #[test]
    fn indicators_like_herdr() {
        let all = [Dot::Blocked, Dot::Working, Dot::Done, Dot::Idle, Dot::Unknown];
        let icons = |s| all.map(|d| d.icon(s)).join(" ");
        assert_eq!(icons(StatusIndicators::Dots), "● ● ● ○ ·");
        assert_eq!(icons(StatusIndicators::Symbols), "× ◐ ✓ ○ ·");
    }

    #[test]
    fn step_skips_headers_and_stops_at_ends() {
        let r = rows_of(&[
            Row::Machine("m".into()),
            Row::Project("m".into(), "p".into()),
            Row::Thread("p/t-0001".into()),
            Row::ThreadSub("p/t-0001".into()),
            Row::Empty,
            Row::Machine("n".into()),
            Row::Pane("x".into()),
        ]);
        assert_eq!(first(&r), 0);
        assert_eq!(step(&r, 0, true), 2);
        assert_eq!(step(&r, 2, true), 5);
        assert_eq!(step(&r, 5, true), 6);
        assert_eq!(step(&r, 6, true), 6);
        assert_eq!(step(&r, 6, false), 5);
        assert_eq!(step(&r, 5, false), 2);
        assert_eq!(step(&r, 0, false), 0);
    }

    fn machine(id: &str, name: &str, connection: Connection) -> MachineView {
        MachineView {
            id: id.into(),
            name: name.into(),
            local: false,
            connection,
            os: None,
            boot_id: None,
            dev_mode: false,
            last_seen_ms: None,
            health: None,
            clock_skew_ms: None,
            problems: vec![],
        }
    }

    fn pane(id: &str, machine_id: &str, role: Option<Role>) -> PaneView {
        PaneView {
            id: id.into(),
            machine_id: machine_id.into(),
            machine: machine_id.into(),
            name: id.into(),
            argv: vec![],
            cwd: None,
            state: SessionState::Running,
            connection: Connection::Connected,
            readonly: false,
            cols: 80,
            rows: 24,
            lease: None,
            role,
            agent: None,
            label: None,
            restore_error: None,
            replaced: Vec::new(),
        }
    }

    fn thread(
        project: &str,
        id: &str,
        group: ThreadGroup,
        updated_at_ms: u64,
        machine: &str,
        pane: Option<&str>,
    ) -> ThreadView {
        ThreadView {
            project: project.into(),
            id: id.into(),
            title: format!("{id}-job"),
            machine: machine.into(),
            profile: "claude".into(),
            role: Role::Worker,
            thread_kind: aaa_proto::ThreadKind::Worktree,
            status: if group == ThreadGroup::Resolved { ThreadStatus::Resolved } else { ThreadStatus::Open },
            group,
            brief: aaa_proto::BriefState::Delivered,
            pane: pane.map(Into::into),
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

    fn agent(
        activity: AgentActivity,
        percent: Option<u8>,
        progress_at_ms: Option<u64>,
    ) -> Option<Box<aaa_proto::AgentStatus>> {
        Some(Box::new(aaa_proto::AgentStatus {
            activity,
            since_ms: 0,
            harness: "claude".into(),
            message: None,
            percent,
            progress: None,
            progress_at_ms,
            harness_session: None,
            seen: true,
            plan: None,
            background: None,
            background_wait: false,
        }))
    }

    /// 웹 `machineRoster`와 같은 차례(차이 목록 L1): 머신마다 머리, 그 머신 thread를 rank(사람 차례 → 검토 대기 →
    /// 작업 중 → 쉼)·최근 바뀐 순으로 늘어놓고 처음 나온 프로젝트 차례로 묶는다. thread의 머신은 pane이 있으면 pane의 머신,
    /// 없으면 thread 머신 이름·ID. thread pane은 pane 줄로 다시 나오지 않고, 끝난·평가자 thread는 빠진다.
    #[test]
    fn rows_group_threads_by_machine_and_project_like_web() {
        let machines = [
            machine("m1", "mac-a", Connection::Connected),
            machine("m2", "mac-b", Connection::Connected),
            machine("m3", "mac-c", Connection::Disconnected),
        ];
        let panes = [
            pane("p1", "m1", Some(Role::Worker)),
            pane("p2", "m1", None),
            pane("p3", "m2", Some(Role::Worker)),
            pane("pe", "m1", Some(Role::Evaluator)),
        ];
        let mut evaluator = thread("demo", "t-0006", ThreadGroup::WaitingOnYou, 900, "mac-a", Some("pe"));
        evaluator.role = Role::Evaluator;
        let threads: HashMap<String, ThreadView> = [
            thread("demo", "t-0001", ThreadGroup::Working, 100, "mac-a", Some("p1")),
            thread("demo", "t-0002", ThreadGroup::WaitingOnYou, 50, "mac-a", None),
            // pane이 mac-b에 있으면 thread 머신 이름이 달라도 mac-b
            thread("demo", "t-0003", ThreadGroup::ReadyForReview, 10, "mac-a", Some("p3")),
            thread("other", "t-0004", ThreadGroup::Idle, 300, "m1", None),
            thread("demo", "t-0007", ThreadGroup::Working, 200, "mac-a", None),
            thread("demo", "t-0005", ThreadGroup::Resolved, 999, "mac-a", None),
            evaluator,
        ]
        .into_iter()
        .map(|t| (thread_key(&t), t))
        .collect();
        let t = |k: &str| Row::Thread(k.into());
        let sub = |k: &str| Row::ThreadSub(k.into());
        assert_eq!(
            rows(&machines, &panes, &threads),
            vec![
                Row::Machine("m1".into()),
                Row::Project("m1".into(), "demo".into()),
                t("demo/t-0002"),
                sub("demo/t-0002"),
                t("demo/t-0007"),
                sub("demo/t-0007"),
                t("demo/t-0001"),
                sub("demo/t-0001"),
                Row::Project("m1".into(), "other".into()),
                t("other/t-0004"),
                sub("other/t-0004"),
                Row::Pane("p2".into()),
                Row::Machine("m2".into()),
                Row::Project("m2".into(), "demo".into()),
                t("demo/t-0003"),
                sub("demo/t-0003"),
                Row::Machine("m3".into()),
                Row::Empty,
                Row::Header("평가".into()),
                Row::Pane("pe".into()),
            ]
        );
    }

    /// herdr-projects `state_line`(웹 `stateLine`): 묶음 말과 사실 하나
    #[test]
    fn state_line_like_herdr_projects() {
        let now = 10 * 60_000;
        let mut t = thread("demo", "t-0001", ThreadGroup::WaitingOnYou, now - 60_000, "mac-a", Some("p1"));
        assert_eq!(state_line(&t, false, now), "사람 차례 · pane 닫힘");
        t.agent = agent(AgentActivity::Question, Some(40), Some(now));
        assert_eq!(state_line(&t, true, now), "사람 차례 · 확인창");
        t.status = ThreadStatus::Failed;
        assert_eq!(state_line(&t, true, now), "사람 차례 · 실패");
        t.status = ThreadStatus::Open;
        t.agent = agent(AgentActivity::Idle, Some(40), Some(now));
        assert_eq!(state_line(&t, true, now), "사람 차례 · ~40%");
        t.group = ThreadGroup::ReadyForReview;
        assert_eq!(state_line(&t, true, now), "검토 대기 · 보고");
        // 따라가는 PR이 있으면 그 번호(웹 `stateLine`과 같다). 번호로 끝나지 않는 주소는 「보고」
        t.pr = Some(Box::new(aaa_proto::ThreadPr {
            url: "https://github.com/o/r/pull/42".into(),
            state: Some("OPEN".into()),
            review: None,
        }));
        assert_eq!(state_line(&t, true, now), "검토 대기 · PR #42");
        t.group = ThreadGroup::Landing;
        assert_eq!(state_line(&t, true, now), "병합 중 · PR #42");
        t.pr.as_mut().unwrap().url = "https://github.com/o/r/pulls".into();
        assert_eq!(state_line(&t, true, now), "병합 중 · 보고");
        t.pr = None;
        t.group = ThreadGroup::ReadyForReview;
        t.group = ThreadGroup::Working;
        // 진행률은 5분 안에 보고했을 때만, 100은 쓰지 않는다
        assert_eq!(state_line(&t, true, now), "작업 중 · ~40%");
        t.agent = agent(AgentActivity::Working, Some(100), Some(now));
        assert_eq!(state_line(&t, true, now), "작업 중");
        t.agent = agent(AgentActivity::Working, Some(40), Some(now - 7 * 60_000));
        assert_eq!(state_line(&t, true, now), "작업 중 · 7분 조용");
        // 보고가 없으면 thread가 바뀐 때부터 센다
        t.agent = None;
        assert_eq!(state_line(&t, true, now), "작업 중");
        t.updated_at_ms = now - QUIET_MS;
        assert_eq!(state_line(&t, true, now), "작업 중 · 5분 조용");
        t.group = ThreadGroup::Idle;
        assert_eq!(state_line(&t, true, now), "쉼");
    }

    /// 프로젝트 머리 오른쪽은 그 머신 thread 셈(herdr-projects `project_line`), thread는 「id · 제목」과 둘째 줄 상태 말,
    /// 고른 thread는 둘째 줄까지 칠한다
    #[test]
    fn thread_rows_render_two_lines_like_herdr_projects() {
        let machines = [machine("m1", "mac-a", Connection::Connected)];
        // thread는 아직 작업 중이지만 그 pane에 확인창이 떴다
        let mut asking = pane("p1", "m1", Some(Role::Worker));
        asking.agent = agent(AgentActivity::Question, None, None);
        let panes: HashMap<String, PaneView> = [asking].into_iter().map(|p| (p.id.clone(), p)).collect();
        let mut working = thread("demo", "t-0001", ThreadGroup::Working, 0, "mac-a", Some("p1"));
        working.agent = agent(AgentActivity::Working, Some(40), Some(1_000));
        let threads: HashMap<String, ThreadView> =
            [working, thread("demo", "t-0002", ThreadGroup::WaitingOnYou, 0, "mac-a", None)]
                .into_iter()
                .map(|t| (thread_key(&t), t))
                .collect();
        let list = rows(&machines, &panes.values().cloned().collect::<Vec<_>>(), &threads);
        let pal = Palette::catppuccin();
        let sel = list.iter().position(|r| *r == Row::Thread("demo/t-0001".into())).unwrap();
        let v = View {
            machines: &machines,
            panes: &panes,
            threads: &threads,
            current: None,
            sel: Some(sel),
            focused: true,
            top: None,
            pal: &pal,
            style: StatusIndicators::Dots,
            now_ms: 2_000,
        };
        let w = 36;
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, w, list.len() as u16));
        render(&mut buf, Rect { x: 0, y: 0, w, h: list.len() as u16 }, &list, &v);
        let text = |y: u16| {
            let (mut out, mut x) = (String::new(), 0);
            while x < w {
                let sym = buf[(x, y)].symbol().to_string();
                x += (unicode_width(&sym) as u16).max(1);
                out.push_str(&sym);
            }
            out.trim_end().to_string()
        };
        let shown: Vec<String> = (0..list.len() as u16).map(text).collect();
        assert_eq!(
            shown,
            vec![
                " ▾ mac-a                          ●",
                " demo       사람 차례 1 · 작업 중 1",
                // pane이 없는 사람 차례는 웹처럼 「pane 닫힘」
                "   ● t-0002 · t-0002-job",
                "     사람 차례 · pane 닫힘",
                "   ● t-0001 · t-0001-job",
                "     작업 중 · ~40%",
            ]
        );
        assert_eq!(buf[(5, 5)].fg, pal.yellow);
        assert_eq!(buf[(5, 3)].fg, pal.red);
        // 점은 pane이 돌면 pane 상태(확인창 빨강), pane이 없으면 묶음(사람 차례 빨강)
        assert_eq!((buf[(3, 4)].symbol(), buf[(3, 4)].fg), ("●", pal.red));
        assert_eq!((buf[(3, 2)].symbol(), buf[(3, 2)].fg), ("●", pal.red));
        let mut working = panes.clone();
        working.get_mut("p1").unwrap().agent = agent(AgentActivity::Working, None, None);
        assert_eq!(thread_row_dot(&threads["demo/t-0001"], &working), Dot::Working);
        working.get_mut("p1").unwrap().agent = None;
        assert_eq!(thread_row_dot(&threads["demo/t-0001"], &working), Dot::Working);
        // harness가 쉬어도 묶음이 더 급하면 묶음 점
        working.get_mut("p1").unwrap().agent = agent(AgentActivity::Idle, None, None);
        assert_eq!(thread_row_dot(&threads["demo/t-0001"], &working), Dot::Working);
        let mut waiting = threads["demo/t-0001"].clone();
        waiting.group = ThreadGroup::WaitingOnYou;
        assert_eq!(thread_row_dot(&waiting, &working), Dot::Blocked);
        let selection = if pal.selection_bg == Color::Reset { pal.active_row_bg } else { pal.selection_bg };
        assert_eq!((buf[(0, 4)].bg, buf[(0, 5)].bg), (selection, selection));
        assert_ne!(buf[(0, 3)].bg, selection);

        // 26칸 사이드바(구분선 빼고 25칸)에서는 셈을 ` · ` 단위로 덜어 이름을 남긴다
        let w = 25;
        let mut narrow = Buffer::empty(ratatui::layout::Rect::new(0, 0, w, 1));
        render(&mut narrow, Rect { x: 0, y: 0, w, h: 1 }, &list[1..2], &View { sel: Some(0), focused: false, ..v });
        let (mut out, mut x) = (String::new(), 0);
        while x < w {
            let sym = narrow[(x, 0)].symbol().to_string();
            x += (unicode_width(&sym) as u16).max(1);
            out.push_str(&sym);
        }
        assert_eq!(out.trim_end(), " demo        사람 차례 1");
    }

    /// thread 줄은 묶음 차례(herdr-projects `DISPLAY_ORDER`: 사람 차례 → 검토 → 병합 중 → 작업 → 쉼)로, 같은 묶음 안은
    /// 최근 바뀐 것이 먼저다(차이 목록 3장 C1). 끝난 thread는 목록에 없다
    #[test]
    fn threads_follow_the_display_order() {
        let t = |id: &str, g, at| thread("p", id, g, at, "mac-a", None);
        let threads: HashMap<String, ThreadView> = [
            t("t-idle", ThreadGroup::Idle, 9),
            t("t-work-old", ThreadGroup::Working, 1),
            t("t-work-new", ThreadGroup::Working, 8),
            t("t-landing", ThreadGroup::Landing, 2),
            t("t-review", ThreadGroup::ReadyForReview, 3),
            t("t-you", ThreadGroup::WaitingOnYou, 4),
            t("t-done", ThreadGroup::Resolved, 10),
        ]
        .into_iter()
        .map(|t| (thread_key(&t), t))
        .collect();
        // 머신 머리 아래 프로젝트 머리 하나, 그 아래 thread 줄들(둘째 줄은 뺀다)
        let got: Vec<Row> = rows(&[machine("m", "mac-a", Connection::Connected)], &[], &threads)
            .into_iter()
            .filter(|r| matches!(r, Row::Thread(_)))
            .collect();
        let want: Vec<Row> = ["t-you", "t-review", "t-landing", "t-work-new", "t-work-old", "t-idle"]
            .map(|id| Row::Thread(format!("p/{id}")))
            .into_iter()
            .collect();
        assert_eq!(got, want);
        assert_eq!(
            project_line(&threads.values().filter(|t| listed(t)).map(|t| t.group).collect::<Vec<_>>()),
            "사람 차례 2 · 작업 중 2 · 병합 중 1"
        );
        assert_eq!(state_line(&threads["p/t-landing"], true, 0), "병합 중 · 보고");
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
            sel: Some(0),
            focused: false,
            top: None,
            pal: &pal,
            style: StatusIndicators::Dots,
            now_ms: 0,
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
                background_wait: false,
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
