//! TUI front(`aaa attach`). 상태를 갖지 않는 얇은 client다(§4.5).
//!
//! - host가 준 workspace layout을 칸으로 나누고, 각 pane은 host가 보내는 cell grid를 그린다.
//! - 가진 것은 지금 탭·포커스·prefix 입력 상태뿐이다. lease·pane 상태는 host가 보낸 목록을 그대로 쓴다.
//! - 키 입력은 포커스 pane에 대화형 입력(`PaneInput`)으로 보낸다. 다른 front가 쓰던 pane이어도 들어가고, 이 화면이
//!   「지금 쓰는 화면」이 되어 PTY 크기를 다시 건다(herdr처럼 명시적 가져오기가 필요 없다, §8.33).
//! - 터미널 focus를 얻으면(FocusGained) 보이는 pane의 크기를 다시 건다. focus가 없을 때는 크기를 걸지 않는다.
//! - Ctrl+V(또는 빈 붙여넣기)에서 이 기기 clipboard에 그림이 있으면 pane 머신으로 올린다(`clip`, §8.28).
//!   그림이 없으면 Ctrl+V는 원래대로 0x16을 보낸다.

pub mod clip;
pub mod keys;

use std::collections::{HashMap, HashSet};
use std::process::ExitCode;
use std::time::Duration;

use aaa_proto::{
    B64, FrontEvent, FrontMsg, FrontRequest, GridFrame, GridRun, LayoutChange, Outcome, PaneView, SessionState,
    SplitDir, Workspace,
};
use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

use aaa_common::front::{FrontClient, FrontSender};
use aaa_proto::layout::{self, Rect};

pub fn run(workspace: Option<String>) -> Result<ExitCode> {
    let cfg = aaa_config::load_host(&aaa_config::paths::config_dir()).map(|l| l.config).unwrap_or_default();
    let prefix = keys::parse_prefix(&cfg.tui.prefix)
        .with_context(|| format!("[tui] prefix `{}`를 읽지 못했다(C-<글자> 형식)", cfg.tui.prefix))?;
    let state = aaa_config::paths::state_dir();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    rt.block_on(async move {
        let label = format!("tui@{}", aaa_common::util::hostname());
        let client = FrontClient::connect_as(&state, &label).await?;
        let mut terminal = ratatui::init();
        // 터미널 focus 사건(FocusGained·FocusLost): 돌아온 화면이 크기를 다시 건다. 비정상 종료(panic)에도 끈다.
        let _focus = FocusReport::on();
        let r = App::new(
            workspace.unwrap_or_else(|| aaa_proto::DEFAULT_WORKSPACE.to_string()),
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

/// 터미널 focus 사건 보고를 켜고, 이 값이 사라질 때(정상·오류·panic 종료) 끈다
struct FocusReport;

impl FocusReport {
    fn on() -> FocusReport {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableFocusChange);
        FocusReport
    }
}

impl Drop for FocusReport {
    fn drop(&mut self) {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableFocusChange);
    }
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
    /// 입력·크기 오류는 상태 줄에 보인다
    Input,
    /// 새로 만든 pane에 포커스를 옮긴다
    Focus,
    /// 그림 붙여넣기 조각(몇 번째 붙여넣기인지)
    Attach(u64),
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
    /// 그림 붙여넣기 번호와 실패를 이미 알린 번호(남은 조각의 거부는 다시 알리지 않는다)
    attach_seq: u64,
    attach_failed: Option<u64>,
    /// 터미널 focus(focus 사건을 안 보내는 터미널은 늘 true)
    focused: bool,
    /// focus를 다시 얻었다: 다음 sync에서 보이는 pane의 크기를 다시 건다
    reclaim: bool,
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
            attach_seq: 0,
            attach_failed: None,
            focused: true,
            reclaim: false,
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
                    // 그림만 든 clipboard를 붙이면 빈 붙여넣기로 오는 터미널이 있다
                    Some(Ok(Event::Paste(s))) if s.is_empty() => { self.paste_image(&mut tx).await?; dirty = true; }
                    Some(Ok(Event::Paste(s))) => { self.input(&mut tx, s.into_bytes()).await?; }
                    Some(Ok(Event::Resize(..))) => dirty = true,
                    Some(Ok(Event::FocusGained)) => { self.focused = true; self.reclaim = true; self.sized.clear(); dirty = true; }
                    Some(Ok(Event::FocusLost)) => self.focused = false,
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
                    (Outcome::Ok(aaa_proto::FrontResponse::Workspace(w)), Some(Pending::Focus)) => {
                        // 새 pane은 마지막 탭(새 탭) 또는 나눈 자리의 새 leaf다
                        let before: HashSet<String> = self.all_panes();
                        self.set_ws(w);
                        if let Some(new) = self.all_panes().difference(&before).next().cloned() {
                            self.select_pane(&new);
                        }
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Workspace(w)), _) => self.set_ws(w),
                    (Outcome::Ok(aaa_proto::FrontResponse::Attached { path, inserted, note }), _) => {
                        self.message = Some(match (inserted, note) {
                            (true, _) => format!("그림을 붙였다(입력창에 경로, Enter 없음): {path}"),
                            (false, Some(n)) => format!("그림을 저장했다: {path} — {n}"),
                            (false, None) => format!("그림을 저장했다: {path}"),
                        })
                    }
                    (Outcome::Ok(_), _) => {}
                    (Outcome::Err(e), Some(Pending::Attach(n))) => {
                        if self.attach_failed != Some(n) {
                            self.attach_failed = Some(n);
                            self.message = Some(format!("그림 붙여넣기 실패: {}", e.message));
                        }
                    }
                    (Outcome::Err(e), Some(Pending::Input)) => self.message = Some(e.message.clone()),
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

    /// 보이는 pane만 화면을 구독한다. 이 화면이 지금 쓰는 화면이거나 비어 있는 포커스 pane은 칸 크기에 맞춘다.
    /// focus를 다시 얻었으면 보이는 pane 모두 크기를 다시 건다(다른 화면이 쓰던 것도). focus가 없으면 걸지 않는다.
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
        let reclaim = std::mem::take(&mut self.reclaim);
        if !self.focused {
            return Ok(());
        }
        for (pane, r) in rects {
            let Some(view) = self.panes.get(&pane) else { continue };
            if !view.state.is_running() || view.connection == aaa_proto::Connection::Disconnected {
                continue;
            }
            let mine = view.lease.as_ref().is_some_and(|l| l.front == self.me);
            let free_focus = view.lease.is_none() && self.focus.as_deref() == Some(pane.as_str());
            let want = (r.w.max(2), r.h.max(1));
            let differs = (view.cols, view.rows) != want && self.sized.get(&pane) != Some(&want);
            if ((mine || free_focus) && differs) || (reclaim && !mine) {
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

    /// clipboard에 그림이 있으면 포커스 pane으로 올리고 true. 없으면 아무것도 보내지 않고 false.
    async fn paste_image(&mut self, tx: &mut FrontSender) -> Result<bool> {
        let Some(pane) = self.focus.clone() else { return Ok(false) };
        let Some(png) = tokio::task::spawn_blocking(clip::image_png).await.ok().flatten() else { return Ok(false) };
        self.attach_seq += 1;
        self.message = Some(format!("그림 보내는 중({}KB)…", png.len().div_ceil(1024)));
        for req in aaa_common::front::attach_requests(&pane, &png) {
            let id = tx.send(req).await?;
            self.pending.insert(id, Pending::Attach(self.attach_seq));
        }
        Ok(true)
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
            if k.code == KeyCode::Char('v') && k.modifiers == KeyModifiers::CONTROL && self.paste_image(tx).await? {
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
                self.pending.insert(id, Pending::Report("이 화면 크기로"));
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
                "c 새 탭 · % 좌우 · \" 위아래 · 화살표/o 이동 · n/p/1-9 탭 · t 이 화면 크기로 · r 재개 · x 닫기 · d 나가기 · (prefix 없이) Ctrl+V 그림"
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
        // 탭 줄: 이름표 · workspace · 탭(지금 탭은 강조색 바탕) … 오른쪽에 열린 질문 수
        let right_edge = area.x + area.w;
        let put = |buf: &mut Buffer, x: u16, s: &str, st: Style| {
            buf.set_stringn(x, area.y, s, right_edge.saturating_sub(x) as usize, st).0
        };
        let mut x = put(buf, area.x, " aaa ", Style::default().fg(ON_CHIP).bg(ACCENT).add_modifier(Modifier::BOLD));
        x = put(buf, x, &format!(" {} ", self.ws_name), Style::default().fg(DIM));
        for (i, t) in self.ws.tabs.iter().enumerate() {
            let first = layout::leaves(&t.root).into_iter().next();
            let name = first.as_ref().and_then(|p| self.panes.get(p)).map(|p| p.name.clone()).unwrap_or_default();
            let label = format!(" {}:{} ", i + 1, if t.name.is_empty() { name } else { t.name.clone() });
            let style = if i == self.tab {
                Style::default().fg(ON_CHIP).bg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(DIM)
            };
            x = put(buf, x, &label, style);
        }
        let asking = self.panes.values().filter(|p| is_asking(p)).count();
        if asking > 0 {
            let label = format!(" ? 질문 {asking} ");
            let w = unicode_width(&label) as u16;
            if x + w < right_edge {
                put(buf, right_edge - w, &label, Style::default().fg(ON_CHIP).bg(BLOCKED).add_modifier(Modifier::BOLD));
            }
        }
        let body = Self::body(area);
        let mut cursor = None;
        match self.ws.tabs.get(self.tab) {
            None => {
                let lines = [
                    ("탭이 없다".to_string(), Style::default().add_modifier(Modifier::BOLD)),
                    (
                        format!("{} 다음 c로 새 셸을 연다 · ?로 도움말", prefix_label(self.prefix.0)),
                        Style::default().fg(DIM),
                    ),
                ];
                for (n, (msg, st)) in lines.iter().enumerate() {
                    let w = unicode_width(msg).min(body.w as usize) as u16;
                    let y = body.y + body.h / 2 + n as u16;
                    buf.set_stringn(body.x + (body.w - w) / 2, y.saturating_sub(1), msg, w as usize, *st);
                }
            }
            Some(tab) => {
                let (rects, lines) = layout::rects(&tab.root, body);
                // 포커스 pane과 맞닿은 구분선만 강조색으로 그린다(여럿일 때 어디가 포커스인지 보인다)
                let focus_rect = rects.iter().find(|(p, _)| Some(p) == self.focus.as_ref()).map(|(_, r)| *r);
                for (dir, r) in lines {
                    let ch = if dir == SplitDir::Horizontal { "│" } else { "─" };
                    for dy in 0..r.h {
                        for dx in 0..r.w {
                            let (cx, cy) = (r.x + dx, r.y + dy);
                            let near = focus_rect.is_some_and(|f| {
                                if dir == SplitDir::Horizontal {
                                    (cx + 1 == f.x || cx == f.x + f.w) && cy >= f.y && cy < f.y + f.h
                                } else {
                                    (cy + 1 == f.y || cy == f.y + f.h) && cx >= f.x && cx < f.x + f.w
                                }
                            });
                            buf.set_string(cx, cy, ch, Style::default().fg(if near { ACCENT } else { FAINT }));
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
        // 상태 줄: 색 조각들(머신 · pane · 상태 …), 오른쪽에 prefix 안내
        let y = area.y + area.h.saturating_sub(1);
        for dx in 0..area.w {
            buf.set_string(area.x + dx, y, " ", Style::default());
        }
        let mut x = area.x;
        for (text, st) in self.status_segments() {
            let room = right_edge.saturating_sub(x) as usize;
            if room == 0 {
                break;
            }
            x = buf.set_stringn(x, y, &text, room, st).0;
        }
        if !self.in_prefix {
            let hint = format!(" {} ? 도움말 ", prefix_label(self.prefix.0));
            let w = unicode_width(&hint) as u16;
            if x + w + 1 < right_edge {
                buf.set_stringn(right_edge - w, y, &hint, w as usize, Style::default().fg(FAINT));
            }
        }
        cursor
    }

    fn render_pane(&self, buf: &mut Buffer, pane: &str, r: Rect, focused: bool) -> Option<(u16, u16)> {
        let view = self.panes.get(pane);
        if let Some(g) = self.grids.get(pane) {
            // PTY 크기가 칸과 다를 수 있다(다른 front가 쓰는 중): 왼쪽 위에 맞추고 칸 밖은 자른다
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

    /// 상태 줄 조각. 글을 이어 붙이면 예전 상태 줄과 같은 내용이다(시험은 글로 본다).
    fn status_segments(&self) -> Vec<(String, Style)> {
        let mut out: Vec<(String, Style)> = Vec::new();
        if self.in_prefix {
            out.push((
                format!(" {} ", prefix_label(self.prefix.0)),
                Style::default().fg(ON_CHIP).bg(WARN).add_modifier(Modifier::BOLD),
            ));
            out.push((
                " … c 새 탭 · % 좌우 · \" 위아래 · 화살표 이동 · d 나가기 (? 도움말)".into(),
                Style::default().fg(DIM),
            ));
            return out;
        }
        if let Some(p) = self.focus.as_ref().and_then(|f| self.panes.get(f)) {
            out.push((
                format!(" {} ", p.machine),
                Style::default().fg(ON_CHIP).bg(ACCENT).add_modifier(Modifier::BOLD),
            ));
            out.push((
                format!(" {} ", if p.name.is_empty() { &p.id } else { &p.name }),
                Style::default().add_modifier(Modifier::BOLD).add_modifier(Modifier::REVERSED),
            ));
            out.push((format!(" {}", state_text(p)), Style::default().fg(state_color(p))));
            match &p.lease {
                Some(l) if l.front == self.me => out.push((" · 이 화면 크기".into(), Style::default().fg(DIM))),
                // 입력하거나 이 터미널로 돌아오면 이 화면 크기로 바뀐다
                Some(l) => out.push((format!(" · {} 크기", l.label), Style::default().fg(WARN))),
                None => {}
            }
        }
        // 사후 평가자는 탭에 없어도 여기 작게 보인다(열기: aaa evaluator open <project>)
        let mut evals: Vec<&PaneView> =
            self.panes.values().filter(|p| p.role == Some(aaa_proto::Role::Evaluator)).collect();
        evals.sort_by(|a, b| a.name.cmp(&b.name));
        for p in evals {
            let (mark, c) = match (&p.state, p.agent.as_ref().map(|a| a.activity)) {
                (st, _) if !st.is_running() => ("끝남", WARN),
                (_, Some(aaa_proto::AgentActivity::Working)) => ("평가 중", WORKING),
                _ => ("대기", DONE),
            };
            let project = p.name.split('/').next().unwrap_or(&p.name);
            out.push((" · 평가 ".into(), Style::default().fg(DIM)));
            out.push(("●".into(), Style::default().fg(c)));
            out.push((format!(" {project} {mark}"), Style::default().fg(DIM)));
        }
        if let Some(m) = &self.message {
            out.push((format!(" · {m}"), Style::default().fg(WARN)));
        }
        out
    }
}

/// 색(256색 번호: 밝은 터미널·어두운 터미널 모두에서 읽힌다). 웹 token과 같은 뜻으로 쓴다.
const ACCENT: Color = Color::Indexed(141);
const WORKING: Color = Color::Indexed(75);
const BLOCKED: Color = Color::Indexed(210);
const DONE: Color = Color::Indexed(114);
const WARN: Color = Color::Indexed(221);
const DIM: Color = Color::Indexed(245);
const FAINT: Color = Color::Indexed(240);
/// 색 바탕 위 글자. ANSI 0(검정)은 굵게 쓰면 많은 터미널이 밝은 회색으로 바꾸므로 256색 검정(16)을 쓴다.
const ON_CHIP: Color = Color::Indexed(16);

fn is_asking(p: &PaneView) -> bool {
    p.state.is_running() && p.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question)
}

fn state_color(p: &PaneView) -> Color {
    use aaa_proto::AgentActivity::*;
    if p.connection == aaa_proto::Connection::Disconnected {
        return WARN;
    }
    match (&p.state, p.agent.as_ref().map(|a| a.activity)) {
        (SessionState::Running, Some(Working)) => WORKING,
        (SessionState::Running, Some(Question | Error)) => BLOCKED,
        (SessionState::Running, Some(Idle)) => DONE,
        (SessionState::Running, _) => DIM,
        (SessionState::Exited { .. }, _) => DIM,
        _ => WARN,
    }
}

fn prefix_label(c: char) -> String {
    format!("C-{c}")
}

fn state_text(p: &PaneView) -> String {
    if p.connection == aaa_proto::Connection::Disconnected {
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

fn activity_text(a: aaa_proto::AgentActivity) -> &'static str {
    use aaa_proto::AgentActivity::*;
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
        (_, aaa_proto::Connection::Disconnected) => " 끊김·읽기 전용 ".to_string(),
        (SessionState::Running, _)
            if p.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question) =>
        {
            " 질문 — aaa question ".to_string()
        }
        (SessionState::Running, _) => return None,
        (SessionState::Interrupted, _) | (SessionState::Lost, _) => {
            format!(" {} — 재개: prefix r ", if p.state == SessionState::Lost { "유실" } else { "중단(재부팅)" })
        }
        (SessionState::Exited { code }, _) => format!(" 종료({}) ", code.map(|c| c.to_string()).unwrap_or("?".into())),
    };
    let w = unicode_width(&text).min(r.w as usize) as u16;
    // 색은 상태를 따른다(질문 빨강·끊김과 중단 호박·종료 회색). 포커스가 아니면 조금 흐리게.
    let bg = match (&p.state, p.connection) {
        (_, aaa_proto::Connection::Disconnected) => WARN,
        (SessionState::Running, _) => BLOCKED,
        (SessionState::Exited { .. }, _) => FAINT,
        _ => WARN,
    };
    let mut style = Style::default().fg(ON_CHIP).bg(bg).add_modifier(Modifier::BOLD);
    if !focused {
        style = style.remove_modifier(Modifier::BOLD).add_modifier(Modifier::DIM);
    }
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
