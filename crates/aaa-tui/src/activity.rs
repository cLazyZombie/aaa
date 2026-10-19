//! t-0390(8c143c0f)의 활동 막대 배치. TUI의 workspace·compact는 공통 영역으로 유지한다.
use super::*;
use ratatui::widgets::{Paragraph, Widget, Wrap};

pub(crate) const WIDTH: u16 = 4;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Panel {
    #[default]
    Projects,
    Machines,
    Agents,
}
impl Panel {
    pub fn index(self) -> usize {
        match self {
            Self::Projects => 0,
            Self::Machines => 1,
            Self::Agents => 2,
        }
    }
    pub fn name(self) -> &'static str {
        ["프로젝트", "머신", "에이전트"][self.index()]
    }
    pub fn key(self) -> &'static str {
        ["projects", "machines", "agents"][self.index()]
    }
    pub fn from_index(i: usize) -> Self {
        [Self::Projects, Self::Machines, Self::Agents][i.min(2)]
    }
}
#[derive(Default)]
pub(crate) struct Reports {
    pub shown: bool,
    pub rows: Vec<aaa_proto::LibraryReport>,
    pub selected: usize,
    pub view: Option<aaa_proto::LibraryView>,
    pub scroll: u16,
}

impl App {
    /// 상태 정본이 아닌 이 기기의 표시 설정이다. 새 client마다 바뀌는 front id로 저장하지 않는다.
    pub(crate) fn load_activity(&mut self) {
        let path = aaa_config::paths::state_dir().join("tui-sidebar.json");
        if let Ok(data) = std::fs::read(&path)
            && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&data)
        {
            self.panel = match v["panel"].as_str() {
                Some("machines") => Panel::Machines,
                Some("agents") => Panel::Agents,
                _ => Panel::Projects,
            };
            if let Some(c) = v["collapsed"].as_bool() {
                self.sidebar_collapsed = Some(c);
                self.sidebar_collapsed_manual = true;
            }
        }
        self.activity_path = Some(path);
    }
    pub(crate) fn remember_activity(&mut self) {
        let Some(path) = &self.activity_path else { return };
        let value = serde_json::json!({ "panel": self.panel.key(), "collapsed": self.collapsed() });
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&tmp, value.to_string())?;
            std::fs::rename(tmp, path)
        })();
        if let Err(e) = result {
            self.set_error(format!("사이드바 배치를 기억하지 못했다: {e}"));
        }
    }
    pub(crate) fn choose_panel(&mut self, panel: Panel) {
        if self.panel == panel {
            self.toggle_list();
        } else {
            self.panel_tops[self.panel.index()] = self.list_top;
            self.panel = panel;
            self.list_top = self.panel_tops[panel.index()];
            self.list_sel = None;
            self.sidebar_collapsed = Some(false);
            self.sidebar_collapsed_manual = true;
            self.remember_activity();
        }
    }
    pub(crate) fn project_rows(&self) -> Vec<list::Row> {
        let panes: Vec<_> = self.pane_order.iter().filter_map(|id| self.panes.get(id).cloned()).collect();
        let mut rows: Vec<_> = list::rows(&self.machines, &panes, &self.threads, &self.folded, self.focus.as_deref())
            .into_iter()
            .take_while(|r| !matches!(r, list::Row::Header(t) if t == "특수"))
            .collect();
        let mut pinned = Vec::new();
        for pin in &self.sidebar_pins {
            if let Some(p) = panes.iter().find(|p| format!("{}:{}", p.machine_id, p.id) == *pin) {
                let target = self
                    .threads
                    .values()
                    .find(|t| t.pane.as_deref() == Some(&p.id))
                    .map(|t| list::Row::Thread(list::thread_key(t)))
                    .unwrap_or_else(|| list::Row::Pane(p.id.clone()));
                if let Some(at) = rows.iter().position(|r| *r == target) {
                    pinned.push(rows.remove(at));
                    if matches!(rows.get(at), Some(list::Row::ThreadSub(_))) {
                        pinned.push(rows.remove(at));
                    }
                }
            }
        }
        if !pinned.is_empty() {
            pinned.insert(0, list::Row::Header("고정".into()));
            pinned.extend(rows);
            rows = pinned;
        }
        if rows.is_empty() {
            rows.push(list::Row::Empty);
        }
        rows
    }
    pub(crate) fn panel_rows(&self) -> Vec<list::Row> {
        match self.panel {
            Panel::Projects => self.project_rows(),
            Panel::Machines => self
                .machines
                .iter()
                .flat_map(|m| {
                    let mut rows = vec![list::Row::Machine(m.id.clone())];
                    if m.connection == aaa_proto::Connection::Disconnected {
                        rows.push(list::Row::Header("  끊김".into()));
                    } else {
                        rows.extend(m.problems.iter().map(|p| list::Row::Header(format!("  {}", p.text))));
                    }
                    rows
                })
                .collect(),
            Panel::Agents => {
                let mut agents = Vec::new();
                for m in &self.machines {
                    let mut group: Vec<_> = self
                        .pane_order
                        .iter()
                        .filter_map(|id| self.panes.get(id))
                        .filter(|p| p.machine_id == m.id && self.agent_in_panel(p))
                        .collect();
                    if self.agent_by_activity {
                        group.sort_by_key(|p| std::cmp::Reverse(list::urgency(list::pane_dot(p))));
                    }
                    agents.extend(group);
                }
                agents.sort_by_key(|p| {
                    self.sidebar_pins
                        .iter()
                        .position(|s| *s == format!("{}:{}", p.machine_id, p.id))
                        .unwrap_or(usize::MAX)
                });
                if agents.is_empty() {
                    vec![list::Row::Empty]
                } else {
                    agents.into_iter().map(|p| list::Row::Pane(p.id.clone())).collect()
                }
            }
        }
    }
    fn agent_in_panel(&self, p: &PaneView) -> bool {
        p.state.is_running()
            && p.role != Some(aaa_proto::Role::Evaluator)
            && (p.agent.is_some()
                || self.threads.values().any(|t| t.pane.as_deref() == Some(&p.id))
                || p.argv.first().is_some_and(|s| s.contains("claude") || s.contains("codex")))
    }
    /// 알림·단축키가 연 pane은 드러내되 접힘을 풀거나 PTY 크기를 바꾸지 않는다.
    pub(crate) fn reveal_panel(&mut self, pane: &str) {
        self.reports.shown = false;
        let Some(p) = self.panes.get(pane) else { return };
        let projects = p.role != Some(aaa_proto::Role::Evaluator);
        let agents = self.agent_in_panel(p);
        if (self.panel == Panel::Projects && projects) || (self.panel == Panel::Agents && agents) {
            return;
        }
        let next = if projects {
            Panel::Projects
        } else if agents {
            Panel::Agents
        } else {
            return;
        };
        self.panel_tops[self.panel.index()] = self.list_top;
        self.panel = next;
        self.list_top = self.panel_tops[next.index()];
        self.list_sel = None;
        self.remember_activity();
    }
    pub(crate) fn special_items(&self) -> Vec<(String, Option<String>)> {
        let mut items: Vec<_> = self
            .missions
            .iter()
            .map(|m| {
                (
                    format!(
                        "{}/{} · {}{}",
                        m.project,
                        m.title,
                        m.state,
                        m.error.as_ref().map(|s| format!(" · {s}")).unwrap_or_default()
                    ),
                    m.thread.as_ref().and_then(|t| t.pane.clone()),
                )
            })
            .collect();
        for id in &self.pane_order {
            if let Some(p) = self.panes.get(id)
                && p.role == Some(aaa_proto::Role::Evaluator)
                && !items.iter().any(|(_, id)| id.as_deref() == Some(p.id.as_str()))
            {
                items.push((list::pane_title(p, &self.threads), Some(p.id.clone())));
            }
        }
        items
    }
    pub(crate) async fn activate_activity(&mut self, at: usize, tx: &mut FrontSender) -> Result<()> {
        match at {
            0..=2 => self.choose_panel(Panel::from_index(at)),
            3 => {
                self.mode = Mode::Terminal;
                self.activity_focus = None;
                self.reports.shown = !self.reports.shown;
                if self.reports.shown && self.host_at_least(38) {
                    let id = tx.send(FrontRequest::LibraryReports { project: None }).await?;
                    self.pending.insert(id, Pending::Reports);
                }
            }
            4 => self.act(Action::Settings, tx).await?,
            5 => {
                self.overlay = Some(Overlay::Special(0));
                if self.host_at_least(38) {
                    let id = tx.send(FrontRequest::MissionList { project: None }).await?;
                    self.pending.insert(id, Pending::Missions);
                }
            }
            _ => self.act(Action::Help, tx).await?,
        }
        Ok(())
    }
    pub(crate) fn render_activity(&self, buf: &mut Buffer, r: Rect, compact: bool) {
        if r.w == 0 || r.h == 0 {
            return;
        }
        let p = &self.palette;
        let bar = Rect { w: WIDTH.min(r.w), ..r };
        fill(buf, bar, Style::default().fg(p.overlay0).bg(p.panel_bg));
        buf.set_stringn(bar.x, bar.y, "aaa", bar.w as usize, Style::default().fg(p.accent).bg(p.panel_bg));
        if bar.h > 1 {
            buf.set_stringn(
                bar.x,
                bar.y + 1,
                if self.reader { "●읽" } else { "●" },
                bar.w as usize,
                Style::default().fg(p.green).bg(p.panel_bg),
            );
        }
        let project_count = self
            .threads
            .values()
            .filter(|t| {
                t.role != aaa_proto::Role::Evaluator
                    && matches!(t.group, aaa_proto::ThreadGroup::WaitingOnYou | aaa_proto::ThreadGroup::ReadyForReview)
            })
            .count();
        let agent_count = self
            .panes
            .values()
            .filter(|p| {
                self.agent_in_panel(p)
                    && p.connection == aaa_proto::Connection::Connected
                    && p.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question)
            })
            .count();
        let compact_height = r.h < 14;
        let controls = [
            (if compact_height { 2 } else { 3 }, "P", project_count),
            (if compact_height { 3 } else { 5 }, "M", 0),
            (if compact_height { 4 } else { 7 }, "A", agent_count),
            (r.h.saturating_sub(3), "R", 0),
            (r.h.saturating_sub(1), "⚙", 0),
            (9, "S", self.special_items().len()),
        ];
        if compact && bar.w == WIDTH {
            for y in bar.y..bar.y + bar.h {
                buf.set_string(bar.x + WIDTH - 1, y, "│", Style::default().fg(p.surface_dim).bg(p.panel_bg));
            }
        }
        let mut hits = self.hits.borrow_mut();
        for (at, (dy, label, count)) in controls.into_iter().enumerate() {
            if dy >= r.h || (at >= 3 && dy <= if compact_height { 4 } else { 7 }) || (at == 5 && r.h < 14) {
                continue;
            }
            let y = r.y + dy;
            let active = (at < 3 && !compact && self.panel.index() == at) || (at == 3 && self.reports.shown);
            let focused = self.mode == Mode::Navigate && self.activity_focus == Some(at);
            let st = Style::default().fg(if active || focused { p.accent } else { p.overlay0 }).bg(if focused {
                p.active_row_bg
            } else {
                p.panel_bg
            });
            let text = format!(
                "{}{}{}",
                if active { "▏" } else { " " },
                label,
                if count > 0 { if count < 10 { count.to_string() } else { "+".into() } } else { String::new() }
            );
            buf.set_stringn(bar.x, y, text, bar.w as usize, st);
            hits.activity.push((Rect { y, h: 1, ..bar }, at));
        }
    }
    pub(crate) fn render_panel_head(&self, buf: &mut Buffer, r: Rect) {
        if r.h < 2 || r.w == 0 {
            return;
        }
        let p = &self.palette;
        buf.set_stringn(
            r.x,
            r.y,
            format!(" {}", self.panel.name()),
            r.w as usize,
            Style::default().fg(p.text).add_modifier(Modifier::BOLD),
        );
        let label = format!(" 특수 {}", self.special_items().len());
        buf.set_stringn(r.x, r.y + 1, label, r.w.saturating_sub(2) as usize, Style::default().fg(p.accent));
        let mut hits = self.hits.borrow_mut();
        hits.activity.push((Rect { y: r.y + 1, h: 1, ..r }, 5));
        if r.w > 2 {
            buf.set_string(r.x + r.w - 1, r.y, "?", Style::default().fg(p.overlay0));
            hits.activity.push((Rect { x: r.x + r.w - 2, w: 2, h: 1, ..r }, 6));
        }
    }
    pub(crate) fn render_special(&self, buf: &mut Buffer, area: Rect, selected: usize) {
        let Some(r) = help::popup(area, 76, 22) else { return };
        let p = &self.palette;
        let i = help::panel(buf, r, p.overlay0, p.panel_bg);
        buf.set_stringn(
            i.x,
            i.y,
            " 특수 임무 · 평가자 (Enter 열기 · Esc 닫기)",
            i.w as usize,
            Style::default().fg(p.accent),
        );
        let items = self.special_items();
        let skip = (selected + 1).saturating_sub(i.h.saturating_sub(2) as usize);
        if items.is_empty() {
            buf.set_stringn(i.x, i.y + 2, " 등록된 임무·평가자가 없다", i.w as usize, Style::default().fg(p.overlay0));
        }
        for (at, (text, _)) in items.iter().enumerate().skip(skip).take(i.h.saturating_sub(2) as usize) {
            let line = Rect { y: i.y + 2 + (at - skip) as u16, h: 1, ..i };
            fill(buf, line, Style::default().bg(if at == selected { p.active_row_bg } else { p.panel_bg }));
            buf.set_stringn(line.x, line.y, text, line.w as usize, Style::default().fg(p.text));
            self.hits.borrow_mut().special_rows.push((line, at));
        }
    }
    pub(crate) fn render_reports(&self, buf: &mut Buffer, r: Rect) {
        let p = &self.palette;
        fill(buf, r, Style::default().fg(p.text).bg(p.panel_bg));
        let title = " 보고서 · ↑↓ 선택 · Enter 읽기 · PgUp/PgDn 본문 · Esc 돌아가기";
        buf.set_stringn(r.x, r.y, title, r.w as usize, Style::default().fg(p.accent));
        if r.h < 3 {
            return;
        }
        let list_w = (r.w / 3).max(12).min(r.w);
        let list_h = r.h - 2;
        let skip = (self.reports.selected + 1).saturating_sub(list_h as usize);
        for (at, report) in self.reports.rows.iter().enumerate().skip(skip).take(list_h as usize) {
            let line = Rect { x: r.x, y: r.y + 2 + (at - skip) as u16, w: list_w, h: 1 };
            let label = format!(
                "{}/{}{}",
                report.project,
                report.name,
                report.problem.as_ref().map(|s| format!(" · {s}")).unwrap_or_default()
            );
            let st =
                Style::default().fg(p.text).bg(if at == self.reports.selected { p.active_row_bg } else { p.panel_bg });
            fill(buf, line, st);
            buf.set_stringn(line.x, line.y, label, line.w as usize, st);
            self.hits.borrow_mut().report_rows.push((line, at));
        }
        let detail = Rect { x: r.x + list_w, y: r.y + 2, w: r.w - list_w, h: list_h };
        if let Some(v) = &self.reports.view {
            let text = format!(
                "{}\n\n결과물\n{}",
                v.report.as_deref().unwrap_or("보고 글이 없다"),
                v.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>().join("\n")
            );
            Paragraph::new(text)
                .wrap(Wrap { trim: false })
                .scroll((self.reports.scroll, 0))
                .style(Style::default().fg(p.text).bg(p.panel_bg))
                .render(ratatui::layout::Rect::new(detail.x, detail.y, detail.w, detail.h), buf);
        }
    }
    pub(crate) async fn open_report(&mut self, tx: &mut FrontSender) -> Result<()> {
        if let Some(r) = self.reports.rows.get(self.reports.selected) {
            let id = tx.send(FrontRequest::LibraryOpen { project: r.project.clone(), name: r.name.clone() }).await?;
            self.pending.insert(id, Pending::LibraryReport);
            self.reports.scroll = 0;
        }
        Ok(())
    }
}
