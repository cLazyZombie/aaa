//! TUI front(`aa attach`). 상태를 갖지 않는 얇은 client다(§4.5).
//!
//! - host가 준 workspace layout을 칸으로 나누고, 각 pane은 host가 보내는 cell grid를 그린다.
//! - 가진 것은 지금 탭·포커스·prefix 입력 상태뿐이다. lease·pane 상태는 host가 보낸 목록을 그대로 쓴다.
//! - 키 입력은 포커스 pane에 대화형 입력(`PaneInput`)으로 보낸다. 다른 front가 조작 중이면 거부를 표시한다.

pub mod keys;

use std::collections::{HashMap, HashSet};
use std::process::ExitCode;
use std::time::Duration;

use aa_proto::{
    B64, ErrorBody, FrontEvent, FrontMsg, FrontRequest, GridFrame, GridRun, LayoutChange, Outcome, PaneView,
    SessionState, SplitDir, Workspace,
};
use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

use aa_common::front::{FrontClient, FrontSender};
use aa_proto::layout::{self, Rect};

pub fn run(workspace: Option<String>) -> Result<ExitCode> {
    let cfg = aa_config::load_host(&aa_config::paths::config_dir()).map(|l| l.config).unwrap_or_default();
    let prefix = keys::parse_prefix(&cfg.tui.prefix)
        .with_context(|| format!("[tui] prefix `{}`를 읽지 못했다(C-<글자> 형식)", cfg.tui.prefix))?;
    let state = aa_config::paths::state_dir();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    rt.block_on(async move {
        let label = format!("tui@{}", aa_common::util::hostname());
        let client = FrontClient::connect_as(&state, &label).await?;
        let mut terminal = ratatui::init();
        let r = App::new(
            workspace.unwrap_or_else(|| aa_proto::DEFAULT_WORKSPACE.to_string()),
            prefix,
            client.front.clone(),
        )
        .run(&mut terminal, client)
        .await;
        ratatui::restore();
        r
    })?;
    Ok(ExitCode::SUCCESS)
}

#[derive(Default)]
struct Grid {
    cols: u16,
    rows: u16,
    lines: Vec<Vec<GridRun>>,
    cursor: (u16, u16, bool),
}

impl Grid {
    fn apply(&mut self, f: GridFrame) {
        if f.full || self.rows != f.rows || self.cols != f.cols {
            self.lines = vec![Vec::new(); f.rows as usize];
        }
        self.cols = f.cols;
        self.rows = f.rows;
        for l in f.lines {
            if let Some(slot) = self.lines.get_mut(l.row as usize) {
                *slot = l.runs;
            }
        }
        self.cursor = (f.cursor_row, f.cursor_col, f.cursor_visible);
    }
}

enum Pending {
    /// 응답이 오면 오류만 보인다
    Report(&'static str),
    /// 입력 거부(lease_held 등)는 상태 줄에 보인다
    Input,
    /// 새로 만든 pane에 포커스를 옮긴다
    Focus,
}

struct App {
    ws_name: String,
    prefix: (char, u8),
    me: String,
    ws: Workspace,
    panes: HashMap<String, PaneView>,
    grids: HashMap<String, Grid>,
    watching: HashSet<String>,
    tab: usize,
    focus: Option<String>,
    in_prefix: bool,
    message: Option<String>,
    pending: HashMap<String, Pending>,
    /// 마지막으로 보낸 크기(같은 크기를 거듭 보내지 않는다)
    sized: HashMap<String, (u16, u16)>,
    quit: bool,
}

impl App {
    fn new(ws_name: String, prefix: (char, u8), me: String) -> App {
        App {
            ws: Workspace { name: ws_name.clone(), tabs: vec![] },
            ws_name,
            prefix,
            me,
            panes: HashMap::new(),
            grids: HashMap::new(),
            watching: HashSet::new(),
            tab: 0,
            focus: None,
            in_prefix: false,
            message: None,
            pending: HashMap::new(),
            sized: HashMap::new(),
            quit: false,
        }
    }

    async fn run(mut self, terminal: &mut ratatui::DefaultTerminal, client: FrontClient) -> Result<()> {
        let (mut tx, mut rx) = client.split();
        let id = tx.send(FrontRequest::Workspace { name: Some(self.ws_name.clone()) }).await?;
        self.pending.insert(id, Pending::Report("workspace 읽기"));
        let mut keys_in = EventStream::new();
        let mut dirty = true;
        loop {
            if dirty {
                let size = terminal.size()?;
                self.sync(&mut tx, Rect { x: 0, y: 0, w: size.width, h: size.height }).await?;
                terminal.draw(|f| {
                    let area = f.area();
                    let cursor =
                        self.render(f.buffer_mut(), Rect { x: area.x, y: area.y, w: area.width, h: area.height });
                    if let Some((x, y)) = cursor {
                        f.set_cursor_position((x, y));
                    }
                })?;
                dirty = false;
            }
            if self.quit {
                return Ok(());
            }
            tokio::select! {
                m = rx.recv() => match m {
                    Some(m) => { self.on_msg(m); dirty = true; }
                    None => anyhow::bail!("host 연결이 끊겼다"),
                },
                e = keys_in.next() => match e {
                    Some(Ok(Event::Key(k))) if k.kind != KeyEventKind::Release => { self.on_key(k, &mut tx).await?; dirty = true; }
                    Some(Ok(Event::Paste(s))) => { self.input(&mut tx, s.into_bytes()).await?; }
                    Some(Ok(Event::Resize(..))) => dirty = true,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                },
                // 화면 frame이 몰려와도 한 번에 그린다
                _ = tokio::time::sleep(Duration::from_millis(16)), if dirty => {}
            }
            // 쌓인 메시지를 마저 처리해 한 번만 그린다
            while let Ok(m) = rx.try_recv() {
                self.on_msg(m);
                dirty = true;
            }
        }
    }

    fn on_msg(&mut self, m: FrontMsg) {
        match m {
            FrontMsg::Event(FrontEvent::Workspace(w)) if w.name == self.ws_name => self.set_ws(w),
            FrontMsg::Event(FrontEvent::Workspace(_)) => {}
            FrontMsg::Event(FrontEvent::State { panes, .. }) => {
                self.panes = panes.into_iter().map(|p| (p.id.clone(), p)).collect();
            }
            FrontMsg::Event(FrontEvent::Grid(f)) => self.grids.entry(f.pane.clone()).or_default().apply(f),
            FrontMsg::Res { id, result } => {
                let why = self.pending.remove(&id);
                match (result, why) {
                    (Outcome::Ok(aa_proto::FrontResponse::Workspace(w)), Some(Pending::Focus)) => {
                        // 새 pane은 마지막 탭(새 탭) 또는 나눈 자리의 새 leaf다
                        let before: HashSet<String> = self.all_panes();
                        self.set_ws(w);
                        if let Some(new) = self.all_panes().difference(&before).next().cloned() {
                            self.select_pane(&new);
                        }
                    }
                    (Outcome::Ok(aa_proto::FrontResponse::Workspace(w)), _) => self.set_ws(w),
                    (Outcome::Ok(_), _) => {}
                    (Outcome::Err(e), Some(Pending::Input)) => self.message = Some(lease_message(&e)),
                    (Outcome::Err(e), Some(Pending::Report(what))) => {
                        self.message = Some(format!("{what} 실패: {}", e.message))
                    }
                    (Outcome::Err(e), _) => self.message = Some(e.message),
                }
            }
            _ => {}
        }
    }

    fn all_panes(&self) -> HashSet<String> {
        self.ws.tabs.iter().flat_map(|t| layout::leaves(&t.root)).collect()
    }

    fn set_ws(&mut self, w: Workspace) {
        self.ws = w;
        if self.tab >= self.ws.tabs.len() {
            self.tab = self.ws.tabs.len().saturating_sub(1);
        }
        let leaves = self.current_leaves();
        if !self.focus.as_ref().is_some_and(|f| leaves.contains(f)) {
            self.focus = leaves.first().cloned();
        }
    }

    fn select_pane(&mut self, pane: &str) {
        if let Some(i) = self.ws.tabs.iter().position(|t| layout::leaves(&t.root).iter().any(|p| p == pane)) {
            self.tab = i;
            self.focus = Some(pane.to_string());
        }
    }

    fn current_leaves(&self) -> Vec<String> {
        self.ws.tabs.get(self.tab).map(|t| layout::leaves(&t.root)).unwrap_or_default()
    }

    fn body(area: Rect) -> Rect {
        // 맨 위 한 줄은 탭, 맨 아래 한 줄은 상태 줄
        Rect { x: area.x, y: area.y + 1, w: area.w, h: area.h.saturating_sub(2) }
    }

    /// 보이는 pane만 화면을 구독하고, 이 화면이 lease를 가졌거나 비어 있는 포커스 pane은 칸 크기에 맞춘다.
    async fn sync(&mut self, tx: &mut FrontSender, area: Rect) -> Result<()> {
        let visible: HashSet<String> = self.current_leaves().into_iter().collect();
        for p in visible.difference(&self.watching.clone()) {
            let id = tx.send(FrontRequest::PaneWatch { pane: p.clone() }).await?;
            self.pending.insert(id, Pending::Report("화면 구독"));
            self.watching.insert(p.clone());
        }
        for p in self.watching.clone().difference(&visible) {
            tx.send(FrontRequest::PaneUnwatch { pane: p.clone() }).await?;
            self.watching.remove(p);
            self.grids.remove(p);
        }
        let Some(tab) = self.ws.tabs.get(self.tab) else { return Ok(()) };
        let (rects, _) = layout::rects(&tab.root, Self::body(area));
        for (pane, r) in rects {
            let Some(view) = self.panes.get(&pane) else { continue };
            if !view.state.is_running() || view.connection == aa_proto::Connection::Disconnected {
                continue;
            }
            let mine = view.lease.as_ref().is_some_and(|l| l.front == self.me);
            let free_focus = view.lease.is_none() && self.focus.as_deref() == Some(pane.as_str());
            let want = (r.w.max(2), r.h.max(1));
            if (mine || free_focus) && (view.cols, view.rows) != want && self.sized.get(&pane) != Some(&want) {
                self.sized.insert(pane.clone(), want);
                let id = tx.send(FrontRequest::PaneResize { pane, cols: want.0, rows: want.1 }).await?;
                self.pending.insert(id, Pending::Input);
            }
        }
        Ok(())
    }

    async fn input(&mut self, tx: &mut FrontSender, data: Vec<u8>) -> Result<()> {
        let Some(pane) = self.focus.clone() else { return Ok(()) };
        if data.is_empty() {
            return Ok(());
        }
        self.message = None;
        let id = tx.send(FrontRequest::PaneInput { pane, data: B64(data) }).await?;
        self.pending.insert(id, Pending::Input);
        Ok(())
    }

    fn focused_machine(&self) -> Option<String> {
        self.focus.as_ref().and_then(|f| self.panes.get(f)).map(|p| p.machine_id.clone())
    }

    async fn layout(&mut self, tx: &mut FrontSender, change: LayoutChange, focus_new: bool) -> Result<()> {
        let id = tx.send(FrontRequest::LayoutApply { workspace: Some(self.ws_name.clone()), change }).await?;
        self.pending.insert(id, if focus_new { Pending::Focus } else { Pending::Report("layout") });
        Ok(())
    }

    async fn on_key(&mut self, k: KeyEvent, tx: &mut FrontSender) -> Result<()> {
        if !self.in_prefix {
            if keys::is_prefix(&k, self.prefix.0) {
                self.in_prefix = true;
                return Ok(());
            }
            return self.input(tx, keys::encode(&k)).await;
        }
        self.in_prefix = false;
        let focus = self.focus.clone();
        match k.code {
            // prefix를 두 번 누르면 그 키를 pane에 보낸다
            KeyCode::Char(c)
                if k.modifiers.contains(KeyModifiers::CONTROL) && c.to_ascii_lowercase() == self.prefix.0 =>
            {
                self.input(tx, vec![self.prefix.1]).await?;
            }
            KeyCode::Char('d') => self.quit = true,
            KeyCode::Char('c') => {
                let machine = self.focused_machine();
                self.layout(tx, LayoutChange::NewTab { machine, name: None, argv: vec![], cwd: None }, true).await?;
            }
            KeyCode::Char('%') | KeyCode::Char('|') | KeyCode::Char('"') | KeyCode::Char('-') => {
                let Some(pane) = focus else { return Ok(()) };
                let dir = if matches!(k.code, KeyCode::Char('%') | KeyCode::Char('|')) {
                    SplitDir::Horizontal
                } else {
                    SplitDir::Vertical
                };
                self.layout(tx, LayoutChange::Split { pane, dir, machine: None, argv: vec![], cwd: None }, true)
                    .await?;
            }
            KeyCode::Char('x') => {
                let Some(pane) = focus else { return Ok(()) };
                self.layout(tx, LayoutChange::Close { pane, kill: true }, false).await?;
            }
            KeyCode::Char('t') => {
                let Some(pane) = focus else { return Ok(()) };
                let id = tx.send(FrontRequest::PaneTake { pane: pane.clone() }).await?;
                self.pending.insert(id, Pending::Report("가져오기"));
                self.sized.remove(&pane);
            }
            KeyCode::Char('r') => {
                let Some(pane) = focus else { return Ok(()) };
                let id = tx.send(FrontRequest::PaneResume { pane }).await?;
                self.pending.insert(id, Pending::Report("재개"));
            }
            KeyCode::Char('n') if !self.ws.tabs.is_empty() => self.tab = (self.tab + 1) % self.ws.tabs.len(),
            KeyCode::Char('p') if !self.ws.tabs.is_empty() => {
                self.tab = (self.tab + self.ws.tabs.len() - 1) % self.ws.tabs.len()
            }
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if i < self.ws.tabs.len() {
                    self.tab = i;
                }
            }
            KeyCode::Char('o') => {
                let leaves = self.current_leaves();
                if let Some(i) = focus.and_then(|f| leaves.iter().position(|p| *p == f)) {
                    self.focus = leaves.get((i + 1) % leaves.len()).cloned();
                }
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => self.move_focus(k.code),
            KeyCode::Char('?') => self.message = Some(
                "c 새 탭 · % 좌우 · \" 위아래 · 화살표/o 이동 · n/p/1-9 탭 · t 가져오기 · r 재개 · x 닫기 · d 나가기"
                    .into(),
            ),
            _ => {}
        }
        // 탭이 바뀌면 포커스를 그 탭으로
        let leaves = self.current_leaves();
        if !self.focus.as_ref().is_some_and(|f| leaves.contains(f)) {
            self.focus = leaves.first().cloned();
        }
        Ok(())
    }

    /// 화면 위치로 이웃 pane을 찾는다(전체 화면 크기는 상관없다: 비율이 같다)
    fn move_focus(&mut self, dir: KeyCode) {
        let Some(tab) = self.ws.tabs.get(self.tab) else { return };
        let (rects, _) = layout::rects(&tab.root, Rect { x: 0, y: 0, w: 1000, h: 1000 });
        let Some(cur) = rects.iter().find(|(p, _)| Some(p) == self.focus.as_ref()).map(|(_, r)| *r) else { return };
        let (cx, cy) = (cur.x as i32 + cur.w as i32 / 2, cur.y as i32 + cur.h as i32 / 2);
        let best = rects
            .iter()
            .filter(|(p, _)| Some(p) != self.focus.as_ref())
            .filter_map(|(p, r)| {
                let (x, y) = (r.x as i32 + r.w as i32 / 2, r.y as i32 + r.h as i32 / 2);
                let ok = match dir {
                    KeyCode::Left => r.x + r.w <= cur.x,
                    KeyCode::Right => r.x >= cur.x + cur.w,
                    KeyCode::Up => r.y + r.h <= cur.y,
                    _ => r.y >= cur.y + cur.h,
                };
                ok.then(|| ((x - cx).abs() + (y - cy).abs(), p.clone()))
            })
            .min();
        if let Some((_, p)) = best {
            self.focus = Some(p);
        }
    }

    /// 그리고 포커스 pane의 커서 위치를 돌려준다
    fn render(&self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let dim = Style::default().fg(Color::DarkGray);
        // 탭 줄
        let mut x = area.x;
        let ws_label = format!(" {} ", self.ws_name);
        x = buf.set_stringn(x, area.y, &ws_label, area.w as usize, Style::default().add_modifier(Modifier::BOLD)).0;
        for (i, t) in self.ws.tabs.iter().enumerate() {
            let first = layout::leaves(&t.root).into_iter().next();
            let name = first.as_ref().and_then(|p| self.panes.get(p)).map(|p| p.name.clone()).unwrap_or_default();
            let label = format!(" {}:{} ", i + 1, if t.name.is_empty() { name } else { t.name.clone() });
            let style =
                if i == self.tab { Style::default().fg(Color::Black).bg(Color::Cyan) } else { Style::default() };
            let room = (area.x + area.w).saturating_sub(x) as usize;
            x = buf.set_stringn(x, area.y, &label, room, style).0;
        }
        let body = Self::body(area);
        let mut cursor = None;
        match self.ws.tabs.get(self.tab) {
            None => {
                let msg = format!("탭이 없다. {} 다음 c로 새 셸을 연다", prefix_label(self.prefix.0));
                buf.set_stringn(body.x + 1, body.y + 1, &msg, body.w.saturating_sub(2) as usize, dim);
            }
            Some(tab) => {
                let (rects, lines) = layout::rects(&tab.root, body);
                for (dir, r) in lines {
                    let ch = if dir == SplitDir::Horizontal { "│" } else { "─" };
                    for dy in 0..r.h {
                        for dx in 0..r.w {
                            buf.set_string(r.x + dx, r.y + dy, ch, dim);
                        }
                    }
                }
                for (pane, r) in rects {
                    let focused = self.focus.as_deref() == Some(pane.as_str());
                    if let Some(c) = self.render_pane(buf, &pane, r, focused) {
                        cursor = Some(c);
                    }
                }
            }
        }
        // 상태 줄
        let y = area.y + area.h.saturating_sub(1);
        let status = self.status_line();
        let style = Style::default().fg(Color::Black).bg(if self.in_prefix { Color::Yellow } else { Color::Gray });
        for dx in 0..area.w {
            buf.set_string(area.x + dx, y, " ", style);
        }
        buf.set_stringn(area.x, y, &status, area.w as usize, style);
        cursor
    }

    fn render_pane(&self, buf: &mut Buffer, pane: &str, r: Rect, focused: bool) -> Option<(u16, u16)> {
        let view = self.panes.get(pane);
        if let Some(g) = self.grids.get(pane) {
            // PTY 크기가 칸과 다를 수 있다(다른 front가 조작 중): 왼쪽 위에 맞추고 칸 밖은 자른다
            for (row, runs) in g.lines.iter().enumerate().take(r.h as usize) {
                let y = r.y + row as u16;
                let mut x = r.x;
                for run in runs {
                    let room = (r.x + r.w).saturating_sub(x) as usize;
                    if room == 0 {
                        break;
                    }
                    x = buf.set_stringn(x, y, &run.t, room, run_style(run)).0;
                }
            }
            let (cr, cc, visible) = g.cursor;
            if focused && visible && cr < r.h && cc < r.w {
                return badge(buf, view, r, focused).or(Some((r.x + cc, r.y + cr)));
            }
        }
        badge(buf, view, r, focused);
        None
    }

    fn status_line(&self) -> String {
        if self.in_prefix {
            return format!(" {} … (? 도움말)", prefix_label(self.prefix.0));
        }
        let mut s = String::new();
        if let Some(p) = self.focus.as_ref().and_then(|f| self.panes.get(f)) {
            s.push_str(&format!(
                " {} · {} · {}",
                p.machine,
                if p.name.is_empty() { &p.id } else { &p.name },
                state_text(p)
            ));
            match &p.lease {
                Some(l) if l.front == self.me => s.push_str(" · 조작 중"),
                Some(l) => s.push_str(&format!(" · {} 조작 중(가져오기 {} t)", l.label, prefix_label(self.prefix.0))),
                None => {}
            }
        }
        // 사후 평가자는 탭에 없어도 여기 작게 보인다(열기: aa evaluator open <project>)
        let mut evals: Vec<&PaneView> =
            self.panes.values().filter(|p| p.role == Some(aa_proto::Role::Evaluator)).collect();
        evals.sort_by(|a, b| a.name.cmp(&b.name));
        for p in evals {
            let mark = match (&p.state, p.agent.as_ref().map(|a| a.activity)) {
                (st, _) if !st.is_running() => "끝남",
                (_, Some(aa_proto::AgentActivity::Working)) => "평가 중",
                _ => "대기",
            };
            let project = p.name.split('/').next().unwrap_or(&p.name);
            s.push_str(&format!(" · 평가 ● {project} {mark}"));
        }
        if let Some(m) = &self.message {
            s.push_str(" · ");
            s.push_str(m);
        }
        s
    }
}

fn prefix_label(c: char) -> String {
    format!("C-{c}")
}

fn lease_message(e: &ErrorBody) -> String {
    if e.code == aa_proto::ErrorCode::LeaseHeld { format!("입력 안 됨: {}", e.message) } else { e.message.clone() }
}

fn state_text(p: &PaneView) -> String {
    if p.connection == aa_proto::Connection::Disconnected {
        return "끊김(읽기 전용)".into();
    }
    match &p.state {
        SessionState::Running => match &p.agent {
            // harness 상태(hook 사건)
            Some(a) => {
                let pct = a.percent.map(|n| format!(" {n}%")).unwrap_or_default();
                format!("실행 · {}{pct}", activity_text(a.activity))
            }
            None => "실행".into(),
        },
        SessionState::Exited { code } => format!("종료({})", code.map(|c| c.to_string()).unwrap_or("?".into())),
        SessionState::Interrupted => "중단(재부팅)".into(),
        SessionState::Lost => "유실".into(),
    }
}

fn activity_text(a: aa_proto::AgentActivity) -> &'static str {
    use aa_proto::AgentActivity::*;
    match a {
        Started => "시작",
        Working => "작업 중",
        Idle => "입력 대기",
        Question => "질문",
        Error => "오류",
        Exited => "끝남",
    }
}

/// 실행 중이 아니거나 끊긴 pane, 확인창 질문이 열린 pane에는 오른쪽 위에 표시를 그린다
fn badge(buf: &mut Buffer, view: Option<&PaneView>, r: Rect, focused: bool) -> Option<(u16, u16)> {
    let p = view?;
    let text = match (&p.state, p.connection) {
        (_, aa_proto::Connection::Disconnected) => " 끊김·읽기 전용 ".to_string(),
        (SessionState::Running, _)
            if p.agent.as_ref().is_some_and(|a| a.activity == aa_proto::AgentActivity::Question) =>
        {
            " 질문 — aa question ".to_string()
        }
        (SessionState::Running, _) => return None,
        (SessionState::Interrupted, _) | (SessionState::Lost, _) => {
            format!(" {} — 재개: prefix r ", if p.state == SessionState::Lost { "유실" } else { "중단(재부팅)" })
        }
        (SessionState::Exited { code }, _) => format!(" 종료({}) ", code.map(|c| c.to_string()).unwrap_or("?".into())),
    };
    let w = unicode_width(&text).min(r.w as usize) as u16;
    let style = Style::default().fg(Color::Black).bg(if focused { Color::Yellow } else { Color::DarkGray });
    buf.set_stringn(r.x + r.w - w, r.y, &text, w as usize, style);
    None
}

fn unicode_width(s: &str) -> usize {
    use ratatui::text::Span;
    Span::raw(s).width()
}

fn color(c: u32) -> Color {
    match c >> 24 {
        1 => Color::Indexed((c & 0xff) as u8),
        2 => Color::Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8),
        _ => Color::Reset,
    }
}

fn run_style(r: &GridRun) -> Style {
    let mut s = Style::default().fg(color(r.fg)).bg(color(r.bg));
    for (bit, m) in [(1, Modifier::BOLD), (2, Modifier::ITALIC), (4, Modifier::UNDERLINED), (8, Modifier::REVERSED)] {
        if r.a & bit != 0 {
            s = s.add_modifier(m);
        }
    }
    s
}
