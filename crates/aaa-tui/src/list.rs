//! 왼쪽 목록(웹 목록과 같은 모양, §8.35·§8.195·차이 목록 L1·U4~U8). herdr 사이드바처럼 위는 workspace(` spaces`, 맨 아랫줄
//! ` new`), 아래는 ` agents` 구역이다. agents는 프로젝트(사용자 말의 workspace)를 먼저 묶는다(t-0352 시안 2안): 프로젝트 머리
//! (접기, 셈), 그 아래 thread 두 줄(첫 줄 상태 점·번호·제목, 둘째 줄 왼쪽 `state_line`·오른쪽 머신 이름), thread에 딸리지 않은
//! pane은 맨 아래 「기타」 묶음(pane 줄에 머신 이름), 맨 아래 사후 평가자. 머신은 트리 단계가 아니라 글자로만 적는다.
//!
//! host가 보낸 머신·pane 목록과 thread 사건으로만 만든다(TUI는 저장하지 않는다). 고르면 그 pane을 탭으로 연다.

use std::collections::{HashMap, HashSet};

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
    /// thread 줄의 둘째 줄(상태 말과 머신, 고르지 않고 누르면 그 thread)
    ThreadSub(String),
    /// 프로젝트 묶음 머리(프로젝트 이름): 고르면 접고 편다
    Project(String),
    /// thread에 딸리지 않은 pane 묶음 머리(「기타」): 고르면 접고 편다
    Other,
    Pane(String),
    Machine(String),
    Empty,
    /// spaces 구역의 workspace(이름)
    Space(String),
}

impl Row {
    pub fn selectable(&self) -> bool {
        matches!(self, Row::Thread(_) | Row::Project(_) | Row::Other | Row::Pane(_) | Row::Space(_) | Row::Machine(_))
    }

    /// 여는 줄(탐색 숫자 키가 세는 줄): 묶음 머리는 접기라 빼고 센다
    pub fn opens(&self) -> bool {
        matches!(self, Row::Thread(_) | Row::Pane(_) | Row::Space(_) | Row::Machine(_))
    }

    /// 누르면 고르는 줄: thread 둘째 줄은 그 thread
    pub fn target(&self) -> Row {
        match self {
            Row::ThreadSub(k) => Row::Thread(k.clone()),
            r => r.clone(),
        }
    }

    /// 접기 열쇠: 프로젝트 머리는 `project:<이름>`, 「기타」는 `other`
    pub fn fold_key(&self) -> Option<String> {
        match self {
            Row::Project(p) => Some(format!("project:{p}")),
            Row::Other => Some("other".into()),
            _ => None,
        }
    }
}

pub fn thread_key(t: &ThreadView) -> String {
    format!("{}/{}", t.project, t.id)
}

/// thread가 도는 머신(웹 `threadMachine`): pane이 있으면 pane의 머신, 없으면 thread의 머신 이름·ID로 찾는다
fn thread_machine<'a>(t: &ThreadView, machines: &'a [MachineView], pane: Option<&PaneView>) -> Option<&'a MachineView> {
    match pane {
        Some(p) => machines.iter().find(|m| m.id == p.machine_id),
        None => machines.iter().find(|m| t.machine == m.name || t.machine == m.id),
    }
}

/// 목록에 thread 줄로 보이는가: 끝난 것·평가자는 뺀다(웹 `machineRoster`)
fn listed(t: &ThreadView) -> bool {
    t.group != ThreadGroup::Resolved && t.role != Role::Evaluator
}

/// herdr-projects `project_line`(웹 `projectLine`): 「확인 필요 1 · 검토 대기 1 · 작업 중 3 · 병합 중 1」, 없으면 「쉼」.
/// 참고의 「need you」 한 수(확인 필요·검토)는 thread 줄 글과 같은 말로 나눠 보인다(QA t-0362 O1, plan §8.203). 0인 쪽은 뺀다
pub fn project_line(groups: &[ThreadGroup]) -> String {
    let mut parts = vec![];
    for g in [ThreadGroup::WaitingOnYou, ThreadGroup::ReadyForReview, ThreadGroup::Working, ThreadGroup::Landing] {
        let label = group_label(g);
        let n = groups.iter().filter(|x| **x == g).count();
        if n > 0 {
            parts.push(format!("{label} {n}"));
        }
    }
    if parts.is_empty() { "쉼".to_string() } else { parts.join(" · ") }
}

/// 웹 목록(`roster`)과 같은 차례. 모든 머신의 thread를 묶음 rank·최근 바뀐 순으로 늘어놓고 처음 나온 프로젝트 차례로 묶어
/// 프로젝트 머리와 thread 두 줄, 그다음 thread에 딸리지 않은 pane의 「기타」 묶음(머신 차례, 머신 안은 받은 차례. 웹은 사람이
/// 옮긴 차례 `paneOrder`가 먼저인데 TUI는 그것을 받지 않는다), 맨 아래 사후 평가자 pane(웹 목록에는 없다). 끝난·평가자 thread는 뺀다. `folded`의 묶음(`Row::fold_key`)은 머리만
/// 남기되, 지금 pane(`current`)의 thread는 접혀도 남긴다(웹 접힌 묶음의 고른 줄). 줄이 하나도 없으면 「pane 없음」.
pub fn rows(
    machines: &[MachineView],
    panes: &[PaneView],
    threads: &HashMap<String, ThreadView>,
    folded: &HashSet<String>,
    current: Option<&str>,
) -> Vec<Row> {
    let mut out = vec![];
    let mut live: Vec<&ThreadView> = threads.values().filter(|t| listed(t)).collect();
    // herdr-projects `Group::rank`(`DISPLAY_ORDER`): 확인 필요 → 검토 대기 → 병합 중 → 작업 중 → 쉼
    live.sort_by(|a, b| {
        a.group
            .rank()
            .cmp(&b.group.rank())
            .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
            .then_with(|| thread_key(a).cmp(&thread_key(b)))
    });
    let mut projects: Vec<&str> = vec![];
    for t in &live {
        if !projects.contains(&t.project.as_str()) {
            projects.push(&t.project);
        }
    }
    let thread_panes: Vec<&str> = live.iter().filter_map(|t| t.pane.as_deref()).collect();
    let coordinators: Vec<&PaneView> = panes
        .iter()
        .filter(|p| {
            p.coordinator_project.is_some()
                && p.role != Some(Role::Evaluator)
                && !thread_panes.contains(&p.id.as_str())
                && machines.iter().any(|m| m.id == p.machine_id)
        })
        .collect();
    for p in &coordinators {
        let project = p.coordinator_project.as_deref().unwrap();
        if !projects.contains(&project) {
            projects.push(project);
        }
    }
    for project in projects {
        let head = Row::Project(project.to_string());
        let shut = head.fold_key().is_some_and(|k| folded.contains(&k));
        out.push(head);
        out.extend(
            coordinators
                .iter()
                .filter(|p| {
                    p.coordinator_project.as_deref() == Some(project) && (!shut || current == Some(p.id.as_str()))
                })
                .map(|p| Row::Pane(p.id.clone())),
        );
        for t in live.iter().filter(|t| t.project == project) {
            if shut && (current.is_none() || t.pane.as_deref() != current) {
                continue;
            }
            out.push(Row::Thread(thread_key(t)));
            out.push(Row::ThreadSub(thread_key(t)));
        }
    }
    let mut loose: Vec<&PaneView> = vec![];
    for m in machines {
        loose.extend(panes.iter().filter(|p| {
            p.machine_id == m.id
                && p.role != Some(Role::Evaluator)
                && !thread_panes.contains(&p.id.as_str())
                && p.coordinator_project.is_none()
        }));
    }
    if !loose.is_empty() {
        let shut = folded.contains("other");
        out.push(Row::Other);
        out.extend(
            loose.into_iter().filter(|p| !shut || current == Some(p.id.as_str())).map(|p| Row::Pane(p.id.clone())),
        );
    }
    if out.is_empty() {
        out.push(Row::Empty);
    }
    let mut evals: Vec<&PaneView> = panes.iter().filter(|p| p.role == Some(Role::Evaluator)).collect();
    evals.sort_by(|a, b| a.name.cmp(&b.name));
    if !evals.is_empty() {
        out.push(Row::Header("특수".into()));
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

/// thread 상태 점: 확인 필요 blocked, 검토 대기 done(herdr 「끝났고 아직 안 봄」 자리), 작업 중 working
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
pub(crate) fn urgency(d: Dot) -> u8 {
    match d {
        Dot::Blocked => 4,
        Dot::Done => 3,
        Dot::Working => 2,
        Dot::Idle => 1,
        Dot::Unknown => 0,
    }
}

/// thread 줄 점: thread 묶음 점과 그 pane 상태 점(herdr-projects 줄은 herdr가 pane agent 상태로 그리는 `state_icon`,
/// 웹 thread 줄은 오른쪽 `PaneState`) 중 급한 쪽. 확인창이 뜨면 묶음이 확인 필요로 바뀌기(30초) 전에도 빨강이고,
/// 확인 필요 thread의 harness가 쉬고 있어도 초록이 되지 않는다.
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
        ThreadGroup::WaitingOnYou => "확인 필요",
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
    /// 접은 묶음(`Row::fold_key`): 머리가 `▸`
    pub folded: &'a HashSet<String>,
}

/// 제목·상태 두 줄은 같은 항목이다. 목록이 바뀌어 저장된 top이 둘째 줄을 가리켜도 제목으로 맞춘다.
fn item_start(rows: &[Row], n: usize) -> usize {
    match (n.checked_sub(1).and_then(|p| rows.get(p)), rows.get(n)) {
        (Some(Row::Thread(a)), Some(Row::ThreadSub(b))) if a == b => n - 1,
        _ => n,
    }
}

fn item_end(rows: &[Row], n: usize) -> usize {
    match (rows.get(n), rows.get(n.saturating_add(1))) {
        (Some(Row::Thread(a)), Some(Row::ThreadSub(b))) if a == b => n + 2,
        _ => n.saturating_add(1),
    }
}

fn scroll_limit(rows: &[Row], height: u16) -> usize {
    let last = rows.len().saturating_sub(height as usize);
    let start = item_start(rows, last);
    // 두 줄 경계 때문에 아래 빈칸 한 줄을 허용한다. 한 줄 화면에서는 마지막 제목을 남긴다.
    if height > 1 && start < last { last + 1 } else { start }
}

/// 휠 한 번에 항목 하나. thread는 두 줄, 묶음 머리·pane은 한 줄씩 넘긴다.
pub fn scroll(rows: &[Row], top: usize, height: u16, down: bool) -> usize {
    let top = item_start(rows, top.min(scroll_limit(rows, height)));
    let next = if down { item_end(rows, top) } else { item_start(rows, top.saturating_sub(1)) };
    item_start(rows, next.min(scroll_limit(rows, height)))
}

/// 줄들을 그리고 (줄 y, 줄 차례)들과 첫 보이는 줄을 준다(사이드바 바탕·구분선은 부르는 쪽).
/// 줄이 넘치면 고른 줄이 보이게 내린다(휠로 민 자리가 있으면 그 자리).
/// herdr 사이드바처럼 탐색 중 고른 줄은 `selection_bg`(없으면 `active_row_bg`), 지금 pane 줄은 `active_row_bg` 바탕이고
/// 이름은 `text` 굵게(아니면 `subtext0`)다(`workspace_selection_background`·`workspace_active_background`·`render_workspace_rows`).
pub fn render(buf: &mut Buffer, r: Rect, rows: &[Row], v: &View) -> (Vec<(u16, usize)>, usize) {
    let p = v.pal;
    let selection = if p.selection_bg == Color::Reset { p.active_row_bg } else { p.selection_bg };
    let active = if v.focused && p.selection_bg == Color::Reset { p.sidebar_bg } else { p.active_row_bg };
    // 선택을 따라갈 때는 제목·상태가 모두 들어가는 항목 경계로 내린다.
    let sel_end = v.sel.map_or(0, |n| {
        let n = item_start(rows, n);
        if r.h == 1 { n.saturating_add(1) } else { item_end(rows, n) }
    });
    let skip = match v.top {
        Some(t) => item_start(rows, t.min(scroll_limit(rows, r.h))),
        None => {
            let top = sel_end.saturating_sub(r.h as usize);
            if item_start(rows, top) < top { top + 1 } else { top }
        }
    };
    let mut placed = Vec::new();
    for (n, row) in rows.iter().enumerate().skip(skip).take(r.h as usize) {
        // 둘째 줄이 들어갈 자리가 없으면 다음 항목 전체를 다음 스크롤에 보인다.
        // 한 줄뿐인 극소 화면에서는 상태 대신 제목을 우선한다.
        if r.h > 1 && item_end(rows, n) > skip + r.h as usize {
            break;
        }
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
        let mut l = line(row, v, current || sel, room);
        let w = r.w as usize;
        let lw = |l: &Parts| l.left.iter().map(|(t, _)| unicode_width(t)).sum::<usize>();
        let left = lw(&l);
        if l.keep_left {
            // 왼쪽(상태 말)이 먼저다: 오른쪽(머신)은 남는 칸에 「…」로 줄이고, 네 칸도 안 남으면 뺀다
            if let Some((t, _)) = l.right.as_mut() {
                let room = w.saturating_sub(left + 2);
                if room < 4 {
                    l.right = None;
                } else {
                    *t = crate::chrome::truncate_end(t, room);
                }
            }
        } else if let Some((t, _)) = l.right.as_mut() {
            // 오른쪽 글(프로젝트 셈 「확인 필요 2 · 작업 중 1」)이 이름 자리를 먹으면 ` · ` 단위로 뒤를 덜어 낸다
            while left + unicode_width(t) + 2 > w {
                match t.rsplit_once(" · ") {
                    Some((head, _)) => *t = head.to_string(),
                    None => break,
                }
            }
        }
        // 오른쪽 글은 이름과 한 칸 띄우고 끝 한 칸을 비운다
        let rw = l.right.as_ref().map(|(s, _)| unicode_width(s) as u16).unwrap_or(0);
        let end = r.x + r.w.saturating_sub(if rw > 0 { rw + 2 } else { 0 });
        let room = (end - r.x) as usize;
        // 넘치면 먼저 줄이는 조각(제목)을 「…」로 줄인다. 두 칸도 안 남으면 그 조각을 뺀다
        let over = lw(&l).saturating_sub(room);
        if let Some(i) = l.flex.filter(|_| over > 0) {
            let t = &l.left[i].0;
            let keep = unicode_width(t).saturating_sub(over);
            l.left[i].0 = if keep < 2 { String::new() } else { crate::chrome::truncate_end(t, keep) };
        }
        // 그래도 넘치는 이름은 herdr `truncate_end`처럼 「…」로 자른다(herdr `resolved_token_spans`: 잘린 토큰 끝 「…」)
        let overflow = lw(&l) > room;
        let mut x = r.x;
        for (t, st) in &l.left {
            if x >= end {
                break;
            }
            let room = (end - x) as usize;
            let t = if overflow { crate::chrome::truncate_end(t, room) } else { t.clone() };
            x = buf.set_stringn(x, y, &t, room, *st).0;
        }
        if let Some((t, st)) = l.right {
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

/// 글과 그 모양
type Span = (String, Style);

/// 한 줄의 조각: 왼쪽 조각들, 넘치면 먼저 줄일 왼쪽 조각(제목), 오른쪽 표시, 왼쪽을 지키고 오른쪽을 줄이는가(thread 둘째 줄)
struct Parts {
    left: Vec<Span>,
    flex: Option<usize>,
    right: Option<Span>,
    keep_left: bool,
}

impl Parts {
    fn new(left: Vec<Span>, right: Option<Span>) -> Parts {
        Parts { left, flex: None, right, keep_left: false }
    }
}

/// 머신 글자(머신 이름, 끊겼으면 「이름 끊김」 노랑)
fn machine_label(m: Option<&MachineView>, fallback: &str, pal: &Palette) -> Span {
    match m {
        Some(m) if m.connection != Connection::Connected => {
            (format!("{} 끊김", m.name), Style::default().fg(pal.yellow))
        }
        Some(m) => (m.name.clone(), Style::default().fg(pal.overlay0)),
        None => (fallback.to_string(), Style::default().fg(pal.overlay0)),
    }
}

/// 줄의 조각. herdr 기본 줄(`state_icon` + 이름)을 한 줄로 쓴다. 묶음 머리는 ` ▾ 이름`(접혔으면 `▸`)과 오른쪽 셈.
fn line(row: &Row, v: &View, highlighted: bool, room: usize) -> Parts {
    let pal = v.pal;
    let name = if highlighted {
        Style::default().fg(pal.text).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(pal.subtext0)
    };
    let faint = Style::default().fg(pal.overlay0);
    let dot = |d: Dot| (format!(" {} ", d.icon(v.style)), Style::default().fg(d.color(pal)));
    let head = |label: &str| {
        let arrow = if row.fold_key().is_some_and(|k| v.folded.contains(&k)) { "▸" } else { "▾" };
        vec![
            (format!(" {arrow} "), Style::default().fg(pal.overlay1)),
            (label.to_string(), Style::default().fg(pal.text).add_modifier(Modifier::BOLD)),
        ]
    };
    match row {
        Row::Header(t) => Parts::new(vec![(format!(" {t}"), faint.add_modifier(Modifier::BOLD))], None),
        // 프로젝트 머리(웹 project-group): 굵은 이름, 오른쪽은 모든 머신의 이 프로젝트 thread 셈(웹 `projectLine`)
        Row::Project(project) => {
            let groups: Vec<ThreadGroup> =
                v.threads.values().filter(|t| t.project == *project && listed(t)).map(|t| t.group).collect();
            let need = groups.iter().any(|g| matches!(g, ThreadGroup::WaitingOnYou | ThreadGroup::ReadyForReview));
            let color = if need { group_color(ThreadGroup::WaitingOnYou, pal) } else { pal.overlay0 };
            Parts {
                flex: Some(1),
                ..Parts::new(head(project), Some((project_line(&groups), Style::default().fg(color))))
            }
        }
        // 「기타」 머리: 오른쪽은 그 묶음의 pane 수
        Row::Other => {
            let n = v
                .panes
                .values()
                .filter(|p| {
                    p.role != Some(Role::Evaluator)
                        && p.coordinator_project.is_none()
                        && v.machines.iter().any(|m| m.id == p.machine_id)
                })
                .filter(|p| !v.threads.values().any(|t| listed(t) && t.pane.as_deref() == Some(p.id.as_str())))
                .count();
            Parts::new(head("기타"), Some((format!("pane {n}"), faint)))
        }
        // thread 첫 줄: 상태 점, 번호(흐리게), 제목(오른쪽 글이 없어 넘치면 끝에서 「…」로 자른다)
        Row::Thread(k) => match v.threads.get(k) {
            Some(t) => Parts::new(
                vec![
                    dot(thread_row_dot(t, v.panes)),
                    (t.id.clone(), Style::default().fg(pal.overlay1)),
                    (" ".into(), name),
                    (t.title.clone(), name),
                ],
                None,
            ),
            None => Parts::new(vec![dot(Dot::Unknown), (k.clone(), name)], None),
        },
        // thread 둘째 줄: 왼쪽 상태 말(묶음 색, 늘 보인다), 오른쪽 머신 이름(넘치면 먼저 줄인다)
        Row::ThreadSub(k) => match v.threads.get(k) {
            Some(t) => {
                let pane = t.pane.as_deref().and_then(|id| v.panes.get(id));
                let open = pane.is_some_and(|p| p.state.is_running());
                let m = thread_machine(t, v.machines, pane);
                Parts {
                    keep_left: true,
                    ..Parts::new(
                        vec![(
                            format!("   {}", state_line(t, open, v.now_ms)),
                            Style::default().fg(group_color(t.group, pal)),
                        )],
                        Some(machine_label(m, &t.machine, pal)),
                    )
                }
            }
            None => Parts::new(vec![], None),
        },
        Row::Machine(id) => match v.machines.iter().find(|m| &m.id == id) {
            Some(m) => Parts::new(
                vec![(
                    format!(
                        " {} {}{} + 새 pane",
                        if m.connection == Connection::Connected { "●" } else { "×" },
                        m.name,
                        if m.local { " [host]" } else { "" }
                    ),
                    name,
                )],
                None,
            ),
            None => Parts::new(vec![], None),
        },
        Row::Empty => Parts::new(vec![("   pane 없음".into(), faint)], None),
        Row::Space(_) => Parts::new(vec![], None),
        // pane 줄: 상태 점, 제목(넘치면 먼저 줄인다), 「기타」 묶음이면 머신 이름, 오른쪽 배지
        Row::Pane(id) => match v.panes.get(id) {
            Some(p) => {
                let st = if p.state.is_running() { name } else { faint };
                let mut left = vec![dot(pane_dot(p)), (pane_title(p, v.threads), st)];
                if p.role != Some(Role::Evaluator) {
                    let (m, style) = machine_label(v.machines.iter().find(|m| m.id == p.machine_id), &p.machine, pal);
                    left.push((format!(" {m}"), style));
                }
                Parts {
                    flex: Some(1),
                    ..Parts::new(left, pane_badge(p, pal, room).map(|(b, c)| (b, Style::default().fg(c))))
                }
            }
            None => Parts::new(vec![dot(Dot::Unknown), (id.clone(), name)], None),
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

/// spaces 구역 맨 아랫줄의 누를 칸: 새 workspace ` new`, 머신이 여럿이면 그 뒤 ` · 머신`(누르면 다음 붙은 머신, §8.195),
/// 전체 메뉴 `menu`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footer {
    pub new: Rect,
    pub machine: Option<Rect>,
    pub menu: Rect,
}

/// spaces 구역 맨 아랫줄(herdr `render_sidebar`·`render_endpoint_workspaces` footer, 마우스를 받을 때만): 왼쪽 새 workspace
/// ` new`(머신이 여럿이면 ` new · 머신`), 오른쪽 전체 메뉴 `menu`(`overlay0`). herdr: ` new`는 5칸, `menu`는 오른쪽 6칸.
/// 머신 머리 줄이 없어진 aaa는 ` · 머신` 칸을 따로 준다(그 머신을 바꾸는 자리).
pub fn render_spaces_footer(buf: &mut Buffer, r: Rect, machine: Option<&str>, p: &Palette) -> Option<Footer> {
    if r.h == 0 || r.w == 0 {
        return None;
    }
    let tail = machine.map(|m| format!(" · {m}")).unwrap_or_default();
    let y = r.y + r.h - 1;
    let faint = Style::default().fg(p.overlay0);
    buf.set_stringn(r.x, y, format!(" new{tail}"), r.w as usize, faint);
    let text = 4.min(r.w);
    buf.set_stringn(r.x + r.w - text, y, "menu", text as usize, faint);
    let menu = 6.min(r.w);
    let new = Rect { x: r.x, y, w: 5.min(r.w), h: 1 };
    // ` new` 칸(5칸)이 ` · 머신`의 앞 빈칸을 덮는다: 머신 칸은 그 뒤 `· 이름`
    let machine = machine.map(|_| Rect {
        x: r.x + new.w,
        y,
        w: (unicode_width(&tail) as u16).saturating_sub(1).min(r.w - new.w),
        h: 1,
    });
    Some(Footer { new, machine, menu: Rect { x: r.x + r.w - menu, y, w: menu, h: 1 } })
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

    /// 고를 수 있는 줄은 thread 첫 줄·묶음 머리·pane이다(둘째 줄·「pane 없음」은 건너뛴다)
    #[test]
    fn step_skips_headers_and_stops_at_ends() {
        let r = rows_of(&[
            Row::Project("p".into()),
            Row::Thread("p/t-0001".into()),
            Row::ThreadSub("p/t-0001".into()),
            Row::Empty,
            Row::Other,
            Row::Pane("x".into()),
        ]);
        assert_eq!(first(&r), 0);
        assert_eq!(step(&r, 0, true), 1);
        assert_eq!(step(&r, 1, true), 4);
        assert_eq!(step(&r, 4, true), 5);
        assert_eq!(step(&r, 5, true), 5);
        assert_eq!(step(&r, 5, false), 4);
        assert_eq!(step(&r, 4, false), 1);
        assert_eq!(step(&r, 0, false), 0);
    }

    /// 화면에 보이는 대로의 줄들(넓은 글자 뒤 칸은 건너뛰고 끝 빈칸은 뺀다)
    fn shown(buf: &Buffer, w: u16, h: u16) -> Vec<String> {
        (0..h)
            .map(|y| {
                let (mut out, mut x) = (String::new(), 0);
                while x < w {
                    let sym = buf[(x, y)].symbol().to_string();
                    x += (unicode_width(&sym) as u16).max(1);
                    out.push_str(&sym);
                }
                out.trim_end().to_string()
            })
            .collect()
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
            coordinator_project: None,
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
            mission: None,
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
            report_attention: None,
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

    /// 프로젝트를 먼저 묶는다(t-0352 2안, 웹 `roster`와 같은 차례): 모든 머신의 thread를 rank(확인 필요 → 검토 대기 →
    /// 작업 중 → 쉼)·최근 바뀐 순으로 늘어놓고 처음 나온 프로젝트 차례로 묶는다. 같은 프로젝트가 머신마다 나뉘지 않는다.
    /// 프로젝트 차례는 이름순이 아니다. thread pane은 pane 줄로 다시 나오지 않고, 끝난·평가자 thread는 빠진다. thread에
    /// 딸리지 않은 pane은 맨 아래 「기타」에 머신 차례로(머신 안은 받은 차례). 접은 묶음은 머리만 남기되 지금 pane의 thread는 남긴다. 줄이 없으면 「pane 없음」.
    #[test]
    fn rows_group_threads_by_project_across_machines() {
        let machines = [
            machine("m1", "mac-a", Connection::Connected),
            machine("m2", "mac-b", Connection::Connected),
            machine("m3", "mac-c", Connection::Disconnected),
        ];
        let mut coordinator = pane("coord", "m1", Some(Role::Coordinator));
        coordinator.coordinator_project = Some("demo".into());
        let mut only = pane("only", "m1", Some(Role::Coordinator));
        only.coordinator_project = Some("solo".into());
        let mut ended = pane("p4", "m2", None);
        ended.state = SessionState::Exited { code: Some(0) };
        let panes = [
            coordinator,
            only,
            pane("p1", "m1", Some(Role::Worker)),
            ended,
            pane("p2", "m1", None),
            pane("p3", "m2", Some(Role::Worker)),
            pane("p5", "m2", None),
            pane("pe", "m1", Some(Role::Evaluator)),
        ];
        let mut evaluator = thread("demo", "t-0006", ThreadGroup::WaitingOnYou, 900, "mac-a", Some("pe"));
        evaluator.role = Role::Evaluator;
        let threads: HashMap<String, ThreadView> = [
            thread("demo", "t-0001", ThreadGroup::Working, 100, "mac-a", Some("p1")),
            thread("demo", "t-0002", ThreadGroup::WaitingOnYou, 50, "mac-a", None),
            // 다른 머신(mac-b pane)의 같은 프로젝트 thread도 같은 묶음
            thread("demo", "t-0003", ThreadGroup::ReadyForReview, 10, "mac-a", Some("p3")),
            thread("alpha", "t-0004", ThreadGroup::Idle, 300, "m1", None),
            thread("demo", "t-0007", ThreadGroup::Working, 200, "mac-c", None),
            thread("demo", "t-0005", ThreadGroup::Resolved, 999, "mac-a", None),
            evaluator,
        ]
        .into_iter()
        .map(|t| (thread_key(&t), t))
        .collect();
        let t = |k: &str| [Row::Thread(k.into()), Row::ThreadSub(k.into())];
        let open = HashSet::new();
        let want: Vec<Row> = [
            vec![Row::Project("demo".into()), Row::Pane("coord".into())],
            t("demo/t-0002").to_vec(),
            t("demo/t-0003").to_vec(),
            t("demo/t-0007").to_vec(),
            t("demo/t-0001").to_vec(),
            vec![Row::Project("alpha".into())],
            t("alpha/t-0004").to_vec(),
            vec![Row::Project("solo".into()), Row::Pane("only".into())],
            vec![Row::Other, Row::Pane("p2".into()), Row::Pane("p4".into()), Row::Pane("p5".into())],
            vec![Row::Header("특수".into()), Row::Pane("pe".into())],
        ]
        .concat();
        assert_eq!(rows(&machines, &panes, &threads, &open, None), want);
        // 접은 묶음: 머리만, 지금 pane(p1)의 thread와 지금 pane(p5)은 남는다
        let folded: HashSet<String> = ["project:demo".to_string(), "other".to_string()].into();
        let want: Vec<Row> = [
            vec![Row::Project("demo".into())],
            t("demo/t-0001").to_vec(),
            vec![Row::Project("alpha".into())],
            t("alpha/t-0004").to_vec(),
            vec![Row::Project("solo".into()), Row::Pane("only".into())],
            vec![Row::Other],
            vec![Row::Header("특수".into()), Row::Pane("pe".into())],
        ]
        .concat();
        assert_eq!(rows(&machines, &panes, &threads, &folded, Some("p1")), want);
        assert!(rows(&machines, &panes, &threads, &folded, Some("coord")).contains(&Row::Pane("coord".into())));
        assert!(rows(&machines, &panes, &threads, &folded, Some("p5")).contains(&Row::Pane("p5".into())));
        assert_eq!(rows(&machines, &[], &HashMap::new(), &open, None), vec![Row::Empty]);
    }

    /// herdr-projects `state_line`(웹 `stateLine`): 묶음 말과 사실 하나
    #[test]
    fn state_line_like_herdr_projects() {
        let now = 10 * 60_000;
        let mut t = thread("demo", "t-0001", ThreadGroup::WaitingOnYou, now - 60_000, "mac-a", Some("p1"));
        assert_eq!(state_line(&t, false, now), "확인 필요 · pane 닫힘");
        t.agent = agent(AgentActivity::Question, Some(40), Some(now));
        assert_eq!(state_line(&t, true, now), "확인 필요 · 확인창");
        t.status = ThreadStatus::Failed;
        assert_eq!(state_line(&t, true, now), "확인 필요 · 실패");
        t.status = ThreadStatus::Open;
        t.agent = agent(AgentActivity::Idle, Some(40), Some(now));
        assert_eq!(state_line(&t, true, now), "확인 필요 · ~40%");
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

    /// 2안 줄 모양: 프로젝트 머리 오른쪽은 모든 머신의 그 프로젝트 thread 셈(herdr-projects `project_line`), thread 첫 줄은
    /// 점·번호·제목, 둘째 줄은 왼쪽 상태 말(묶음 색)·오른쪽 머신 이름. 고른 thread는 둘째 줄까지 칠한다. 좁으면 머신이 먼저
    /// 「…」로 줄고 빠진다(상태 말은 남는다). 끊긴 머신은 「이름 끊김」 노랑.
    #[test]
    fn thread_rows_render_two_lines_project_first() {
        let mut machines = [machine("m1", "mac-a", Connection::Connected)];
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
        let open = HashSet::new();
        let list = rows(&machines, &panes.values().cloned().collect::<Vec<_>>(), &threads, &open, None);
        let pal = Palette::catppuccin();
        let sel = list.iter().position(|r| *r == Row::Thread("demo/t-0001".into())).unwrap();
        let draw_at = |machines: &[MachineView], rows: &[Row], w: u16, h: u16, sel, top| {
            let v = View {
                machines,
                panes: &panes,
                threads: &threads,
                current: None,
                sel,
                focused: true,
                top,
                pal: &pal,
                style: StatusIndicators::Dots,
                now_ms: 2_000,
                folded: &open,
            };
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, w, h));
            let (placed, _) = render(&mut buf, Rect { x: 0, y: 0, w, h }, rows, &v);
            (buf, placed)
        };
        // 선택을 따라 내려가도 상태 줄에서 시작하지 않는다. 휠로 옮긴 둘째 줄 위치도 제목에 붙인다.
        for (h, selected, top, want) in [
            (3, Some(sel), None, vec!["t-0001", "작업 중", ""]),
            (3, None, Some(2), vec!["t-0002", "확인 필요", ""]),
            (2, None, Some(usize::MAX), vec!["t-0001", "작업 중"]),
            (1, Some(sel), None, vec!["t-0001"]),
            (1, None, Some(usize::MAX), vec!["t-0001"]),
            (4, None, Some(0), vec!["demo", "t-0002", "확인 필요", ""]),
            (0, Some(sel), None, vec![]),
        ] {
            let (buf, placed) = draw_at(&machines, &list, 36, h, selected, top);
            let got = shown(&buf, 36, h);
            assert_eq!(placed.len(), want.iter().filter(|s| !s.is_empty()).count(), "빈칸에 hit 영역이 남았다");
            for (line, needle) in got.iter().zip(want) {
                if needle.is_empty() {
                    assert!(line.is_empty(), "반쪽 thread가 남았다: {got:?}");
                } else {
                    assert!(line.contains(needle), "{h}줄 sel={selected:?} top={top:?}: {got:?}");
                }
            }
            // 실제 그린 두 줄 모두 같은 thread를 연다(빈칸에는 hit 영역을 남기지 않는다).
            for pair in placed.windows(2) {
                let a = &list[pair[0].1];
                let b = &list[pair[1].1];
                if matches!(b, Row::ThreadSub(_)) {
                    assert_eq!(a.target(), b.target());
                }
            }
        }
        let draw = |machines: &[MachineView], rows: &[Row], w: u16, sel| {
            draw_at(machines, rows, w, rows.len() as u16, sel, None).0
        };
        let buf = draw(&machines, &list, 36, Some(sel));
        assert_eq!(
            shown(&buf, 36, list.len() as u16),
            vec![
                " ▾ demo     확인 필요 1 · 작업 중 1",
                " ● t-0002 t-0002-job",
                // pane이 없는 확인 필요는 웹처럼 「pane 닫힘」
                "   확인 필요 · pane 닫힘      mac-a",
                " ● t-0001 t-0001-job",
                "   작업 중 · ~40%             mac-a",
            ]
        );
        assert_eq!(buf[(3, 4)].fg, pal.yellow);
        assert_eq!(buf[(3, 2)].fg, pal.red);
        assert_eq!(buf[(30, 2)].fg, pal.overlay0);
        assert_eq!(buf[(3, 1)].fg, pal.overlay1);
        // 점은 pane이 돌면 pane 상태(확인창 빨강), pane이 없으면 묶음(확인 필요 빨강)
        assert_eq!((buf[(1, 3)].symbol(), buf[(1, 3)].fg), ("●", pal.red));
        assert_eq!((buf[(1, 1)].symbol(), buf[(1, 1)].fg), ("●", pal.red));
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
        assert_eq!((buf[(0, 3)].bg, buf[(0, 4)].bg), (selection, selection));
        assert_ne!(buf[(0, 2)].bg, selection);

        // 좁으면 머신이 먼저 「…」로 줄고(네 칸까지) 빠진다. 상태 말은 남는다
        let sub = &list[2..3];
        let one = |w: u16| shown(&draw(&machines, sub, w, None), w, 1).remove(0);
        assert_eq!(one(31), "   확인 필요 · pane 닫힘 mac-a");
        assert_eq!(one(30), "   확인 필요 · pane 닫힘 mac…");
        assert_eq!(one(29), "   확인 필요 · pane 닫힘");
        assert!(one(18).starts_with("   확인 필요"), "{}", one(18));
        // 26칸 사이드바(구분선 빼고 25칸)에서 머리 셈은 ` · ` 단위로 덜어 이름을 남긴다
        assert_eq!(shown(&draw(&machines, &list[0..1], 25, None), 25, 1)[0], " ▾ demo      확인 필요 1");
        // 끊긴 머신은 「이름 끊김」 노랑
        machines[0].connection = Connection::Disconnected;
        let buf = draw(&machines, sub, 36, None);
        assert_eq!(shown(&buf, 36, 1)[0], "   확인 필요 · pane 닫힘 mac-a 끊김");
        assert_eq!(buf[(25, 0)].fg, pal.yellow);
    }

    /// thread 줄은 묶음 차례(herdr-projects `DISPLAY_ORDER`: 확인 필요 → 검토 → 병합 중 → 작업 → 쉼)로, 같은 묶음 안은
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
        // 프로젝트 머리 하나, 그 아래 thread 줄들(둘째 줄은 뺀다)
        let got: Vec<Row> = rows(&[machine("m", "mac-a", Connection::Connected)], &[], &threads, &HashSet::new(), None)
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
            "확인 필요 1 · 검토 대기 1 · 작업 중 2 · 병합 중 1"
        );
        assert_eq!(state_line(&threads["p/t-landing"], true, 0), "병합 중 · 보고");
    }

    /// 프로젝트 머리 셈(웹 `projectLine`, plan §8.203): 확인 필요와 검토 대기는 줄 글과 같은 말로 나눠 세고 0인 쪽은 뺀다
    /// (QA t-0362 O1). 머리를 빨갛게 하는 「사람이 볼 thread」는 그대로 둘 다다(herdr-projects needs you)
    #[test]
    fn project_line_splits_need_and_review() {
        use ThreadGroup::*;
        assert_eq!(project_line(&[WaitingOnYou, ReadyForReview]), "확인 필요 1 · 검토 대기 1");
        assert_eq!(project_line(&[ReadyForReview, ReadyForReview]), "검토 대기 2");
        assert_eq!(project_line(&[WaitingOnYou, Working]), "확인 필요 1 · 작업 중 1");
        assert_eq!(project_line(&[Idle]), "쉼");
        // 그린 머리: 기본 폭(36칸)은 다 보이고, 좁으면 ` · ` 단위로 뒤에서 덜어 이름을 남긴다(급한 「확인 필요」가 남는다)
        let machines = [machine("m1", "mac-a", Connection::Connected)];
        let threads: HashMap<String, ThreadView> = [
            thread("heads", "t-0001", ReadyForReview, 0, "mac-a", None),
            thread("heads", "t-0002", WaitingOnYou, 0, "mac-a", None),
        ]
        .into_iter()
        .map(|t| (thread_key(&t), t))
        .collect();
        let (panes, open, pal) = (HashMap::new(), HashSet::new(), Palette::catppuccin());
        let head = |w: u16| {
            let v = View {
                machines: &machines,
                panes: &panes,
                threads: &threads,
                current: None,
                sel: None,
                focused: true,
                top: None,
                pal: &pal,
                style: StatusIndicators::Dots,
                now_ms: 0,
                folded: &open,
            };
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, w, 1));
            render(&mut buf, Rect { x: 0, y: 0, w, h: 1 }, &[Row::Project("heads".into())], &v);
            (shown(&buf, w, 1).remove(0), buf[(w - 2, 0)].fg)
        };
        assert_eq!(head(35), (" ▾ heads 확인 필요 1 · 검토 대기 1".to_string(), pal.red));
        let narrow = head(25).0;
        assert!(
            narrow.starts_with(" ▾ heads ") && narrow.ends_with(" 확인 필요 1") && !narrow.contains("검토"),
            "{narrow}"
        );
    }

    /// 「기타」 pane 줄은 제목 뒤에 머신 이름. 넘치면 제목이 먼저 「…」로 줄고 머신·배지는 남는다, 오른쪽 글과 한 칸 띄우고
    /// 끝 한 칸은 빈다(t-0337 QA Q13, herdr `truncate_end`·구분자)
    #[test]
    fn pane_row_cuts_the_title_before_the_machine() {
        let machines = [machine("m", "mac-b", Connection::Connected)];
        let mut p = pane("p", "m", None);
        p.name = "very-long-pane-title".into();
        p.state = SessionState::Exited { code: Some(0) };
        let panes: HashMap<String, PaneView> = [(p.id.clone(), p)].into();
        let (threads, open) = (HashMap::new(), HashSet::new());
        let pal = Palette::catppuccin();
        let v = View {
            machines: &machines,
            panes: &panes,
            threads: &threads,
            current: None,
            sel: None,
            focused: false,
            top: None,
            pal: &pal,
            style: StatusIndicators::Dots,
            now_ms: 0,
            folded: &open,
        };
        let line = |w: u16| {
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, w, 1));
            render(&mut buf, Rect { x: 0, y: 0, w, h: 1 }, &[Row::Pane("p".into())], &v);
            shown(&buf, w, 1).remove(0)
        };
        assert_eq!(line(30), " · very-long-p… mac-b 종료(0)");
        // 넉넉하면 그대로
        assert_eq!(line(40), " · very-long-pane-title mac-b   종료(0)");
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
            coordinator_project: None,
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
