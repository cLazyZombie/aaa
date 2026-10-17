//! TUI front(`aaa attach`). 상태를 갖지 않는 얇은 client다(§4.5).
//!
//! - host가 준 workspace layout을 칸으로 나누고, 각 pane은 host가 보내는 cell grid를 그린다.
//! - 가진 것은 지금 탭·포커스·prefix 입력 상태뿐이다. lease·pane 상태는 host가 보낸 목록을 그대로 쓴다.
//! - 키 입력은 포커스 pane에 대화형 입력(`PaneInput`)으로 보낸다. 다른 front가 쓰던 pane이어도 들어가고, 이 화면이
//!   「지금 쓰는 화면」이 되어 PTY 크기를 다시 건다(herdr처럼 명시적 가져오기가 필요 없다, §8.33).
//! - 터미널 focus를 얻으면(FocusGained) 보이는 pane의 크기를 다시 건다. focus가 없을 때는 크기를 걸지 않는다.
//! - Ctrl+V(또는 빈 붙여넣기)에서 이 기기 clipboard에 그림이 있으면 pane 머신으로 올린다(`clip`, §8.28).
//!   그림이 없으면 Ctrl+V는 원래대로 0x16을 보낸다.
//! - 키는 herdr 0.9.5 모드를 따른다(`src/client/shell/input.rs` `route_key`): Terminal(pane에 바로) → prefix 키로
//!   Prefix(명령 키 하나, 시간 제한 없음, Esc·모르는 키는 취소) → `w`로 Navigate(목록 고르기·pane 포커스).

pub mod bind;
pub mod clip;
mod help;
pub mod input_source;
pub mod keys;
pub mod list;

use std::collections::{HashMap, HashSet};
use std::process::ExitCode;
use std::time::Duration;

use bind::Action;
use keys::Combo;

use aaa_proto::{
    B64, FrontEvent, FrontMsg, FrontRequest, GridFrame, GridRun, LayoutChange, MachineView, Outcome, PaneView,
    SessionState, SplitDir, ThreadView, Workspace,
};
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};

use aaa_common::front::{FrontClient, FrontSender};
use aaa_proto::layout::{self, Rect};

/// 오류 막대를 보이는 시간(herdr `ENDPOINT_ERROR_TIMEOUT_SECS`)
const ERROR_SHOWN: Duration = Duration::from_secs(5);

/// host.toml `[tui]`(읽지 못하면 기본값)
fn tui_config() -> aaa_config::TuiSection {
    aaa_config::load_host(&aaa_config::paths::config_dir()).map(|l| l.config.tui).unwrap_or_default()
}

pub fn run(workspace: Option<String>) -> Result<ExitCode> {
    let cfg = tui_config();
    let state = aaa_config::paths::state_dir();
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    rt.block_on(async move {
        let label = format!("tui@{}", aaa_common::util::hostname());
        let client = FrontClient::connect_as(&state, &label).await?;
        let mut terminal = ratatui::init();
        // 터미널 focus 사건(FocusGained·FocusLost): 돌아온 화면이 크기를 다시 건다. 비정상 종료(panic)에도 끈다.
        let _focus = FocusReport::on();
        let mut app =
            App::new(workspace.unwrap_or_else(|| aaa_proto::DEFAULT_WORKSPACE.to_string()), client.front.clone());
        app.apply_config(&cfg);
        let r = app.run(&mut terminal, client).await;
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

/// herdr `ClientShellMode`(Copy·Resize는 다음 묶음)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// 키가 포커스 pane으로 간다
    Terminal,
    /// prefix를 받았다: 다음 키 하나가 명령이다
    Prefix,
    /// 목록 탐색(`prefix w`): ↑↓ 목록, Enter 열기, h/j/k/l·←→·Tab pane 포커스, Esc 돌아가기
    Navigate,
}

/// 화면 위에 뜨는 창
enum Overlay {
    Help(Help),
}

/// 단축키 창(herdr `ClientHelpOverlay`)
#[derive(Default)]
struct Help {
    query: String,
    search: bool,
    scroll: usize,
}

struct App {
    ws_name: String,
    /// prefix 키들(`[tui] prefix`)
    prefixes: Vec<Combo>,
    me: String,
    ws: Workspace,
    panes: HashMap<String, PaneView>,
    /// host가 보낸 순서(목록에 그 순서로 보인다)
    pane_order: Vec<String>,
    machines: Vec<MachineView>,
    /// thread 기록(`project/id`). 처음에 목록을 읽고, 바뀌면 사건으로 온다.
    threads: HashMap<String, ThreadView>,
    /// 왼쪽 목록을 펼쳤나. 정하지 않았으면 터미널 폭으로 정한다(좁으면 접힘).
    list_open: Option<bool>,
    /// 탐색 모드에서 고른 줄(줄이 늘거나 줄어도 같은 것을 가리킨다)
    list_sel: Option<list::Row>,
    /// 마지막으로 그린 터미널 크기
    width: u16,
    height: u16,
    grids: HashMap<String, Grid>,
    watching: HashSet<String>,
    tab: usize,
    focus: Option<String>,
    mode: Mode,
    overlay: Option<Overlay>,
    /// 알림 글(상태 줄)
    message: Option<String>,
    /// 오류(ERROR 막대, ERROR_SHOWN 뒤 사라진다)
    error: Option<(String, tokio::time::Instant)>,
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
    /// 모드 동안 영문 입력기로 바꾼다(`[tui] switch_ascii_input_source_in_prefix`)
    switch_ascii: bool,
    /// prefix 뒤 두벌식 자모를 같은 자리 영문 키로 읽는다(`[tui] prefix_jamo_keys`)
    jamo_keys: bool,
    /// 영문 입력기를 원하는 상태인가(herdr `ascii_input_source_active`)
    ascii_on: bool,
    /// 영문으로 바꿔 둔 입력기(없애면 되돌린다: 모드 끝·종료·panic)
    ascii: Option<input_source::Ascii>,
    quit: bool,
}

impl App {
    fn new(ws_name: String, me: String) -> App {
        App {
            ws: Workspace { name: ws_name.clone(), tabs: vec![] },
            ws_name,
            prefixes: vec![keys::DEFAULT_PREFIX],
            me,
            panes: HashMap::new(),
            pane_order: vec![],
            machines: vec![],
            threads: HashMap::new(),
            list_open: None,
            list_sel: None,
            width: 0,
            height: 0,
            grids: HashMap::new(),
            watching: HashSet::new(),
            tab: 0,
            focus: None,
            mode: Mode::Terminal,
            overlay: None,
            message: None,
            error: None,
            pending: HashMap::new(),
            sized: HashMap::new(),
            attach_seq: 0,
            attach_failed: None,
            focused: true,
            reclaim: false,
            switch_ascii: false,
            jamo_keys: false,
            ascii_on: false,
            ascii: None,
            quit: false,
        }
    }

    /// `[tui]` 설정을 적용한다(시작·`prefix shift+r`). 읽지 못한 prefix 값은 빼고 오류로 알린다(herdr처럼).
    fn apply_config(&mut self, cfg: &aaa_config::TuiSection) {
        let (prefixes, bad) = keys::parse_prefixes(&cfg.prefix.values());
        self.prefixes = prefixes;
        self.switch_ascii = cfg.switch_ascii();
        self.jamo_keys = cfg.prefix_jamo_keys;
        if !bad.is_empty() {
            self.set_error(format!("[tui] prefix 값을 읽지 못했다: {} (예: \"ctrl+b\")", bad.join(", ")));
        }
        self.reconcile_ascii();
    }

    fn set_error(&mut self, e: impl Into<String>) {
        self.error = Some((e.into(), tokio::time::Instant::now() + ERROR_SHOWN));
    }

    fn is_prefix(&self, c: Combo) -> bool {
        self.prefixes.contains(&c)
    }

    /// 도움말·안내에 보이는 prefix 이름(여럿이면 ` / `로)
    fn prefix_label(&self) -> String {
        self.prefixes.iter().map(|c| keys::label(*c)).collect::<Vec<_>>().join(" / ")
    }

    /// herdr `wants_ascii_input`: 명령 키를 받는 모드나 창이 열린 동안
    fn wants_ascii(&self) -> bool {
        self.overlay.is_some() || self.mode != Mode::Terminal
    }

    /// herdr `reconcile_input_source`. 다른 창에 focus가 있는 동안은 바꾸지도 되돌리지도 않고 돌아온 뒤 맞춘다.
    fn reconcile_ascii(&mut self) {
        if !self.focused {
            return;
        }
        let desired = self.switch_ascii && self.wants_ascii();
        if desired != self.ascii_on {
            self.ascii_on = desired;
            self.ascii = if desired { input_source::Ascii::switch() } else { None };
        }
    }

    async fn run(mut self, terminal: &mut ratatui::DefaultTerminal, client: FrontClient) -> Result<()> {
        let (mut tx, mut rx) = client.split();
        let id = tx.send(FrontRequest::Workspace { name: Some(self.ws_name.clone()) }).await?;
        self.pending.insert(id, Pending::Report("workspace 읽기"));
        let id = tx.send(FrontRequest::Threads { project: None, role: None }).await?;
        self.pending.insert(id, Pending::Report("thread 목록"));
        let mut keys_in = EventStream::new();
        let mut dirty = true;
        loop {
            if dirty {
                let size = terminal.size()?;
                self.width = size.width;
                self.height = size.height;
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
                    // 돌아오면 크기를 다시 걸고, 다른 창에 있는 동안 미룬 입력기 바꾸기·되돌리기를 맞춘다
                    Some(Ok(Event::FocusGained)) => { self.focused = true; self.reclaim = true; self.sized.clear(); self.reconcile_ascii(); dirty = true; }
                    Some(Ok(Event::FocusLost)) => self.focused = false,
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                },
                // 화면 frame이 몰려와도 한 번에 그린다
                _ = tokio::time::sleep(Duration::from_millis(16)), if dirty => {}
                // 오류 막대는 잠깐만 보인다
                _ = tokio::time::sleep_until(self.error.as_ref().map_or_else(tokio::time::Instant::now, |e| e.1)), if self.error.is_some() => {
                    self.error = None;
                    dirty = true;
                }
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
            FrontMsg::Event(FrontEvent::State { machines, panes }) => {
                self.machines = machines;
                self.pane_order = panes.iter().map(|p| p.id.clone()).collect();
                self.panes = panes.into_iter().map(|p| (p.id.clone(), p)).collect();
            }
            FrontMsg::Event(FrontEvent::Thread(t)) => {
                self.threads.insert(list::thread_key(&t), t);
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
                    (Outcome::Ok(aaa_proto::FrontResponse::Threads { threads }), _) => {
                        self.threads = threads.into_iter().map(|t| (list::thread_key(&t), t)).collect();
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Attached { path, inserted, note }), _) => {
                        self.message = Some(match (inserted, note) {
                            (true, _) => format!("그림 경로를 넣었다(Enter 없음): {path}"),
                            (false, Some(n)) => format!("그림을 저장했다: {path} — {n}"),
                            (false, None) => format!("그림을 저장했다: {path}"),
                        })
                    }
                    (Outcome::Ok(_), _) => {}
                    (Outcome::Err(e), Some(Pending::Attach(n))) => {
                        if self.attach_failed != Some(n) {
                            self.attach_failed = Some(n);
                            self.message = None;
                            self.set_error(format!("그림 붙여넣기 실패: {}", e.message));
                        }
                    }
                    (Outcome::Err(e), Some(Pending::Input)) => self.set_error(e.message),
                    (Outcome::Err(e), Some(Pending::Report(what))) => {
                        self.set_error(format!("{what} 실패: {}", e.message))
                    }
                    (Outcome::Err(e), _) => self.set_error(e.message),
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

    /// 왼쪽 목록이 보이나(정하지 않았으면 좁은 터미널에서는 접힘)
    fn list_shown(&self, w: u16) -> bool {
        self.list_open.unwrap_or(w >= list::NARROW) && w > list::WIDTH + 20
    }

    /// pane 칸 영역: 맨 위 한 줄은 탭, 맨 아래 한 줄은 상태 줄, 목록이 보이면 그 오른쪽(구분선 다음)
    fn body(&self, area: Rect) -> Rect {
        let side = if self.list_shown(area.w) { list::WIDTH + 1 } else { 0 };
        Rect { x: area.x + side, y: area.y + 1, w: area.w - side, h: area.h.saturating_sub(2) }
    }

    fn list_rect(area: Rect) -> Rect {
        Rect { x: area.x, y: area.y + 1, w: list::WIDTH, h: area.h.saturating_sub(2) }
    }

    fn list_rows(&self) -> Vec<list::Row> {
        let panes: Vec<PaneView> = self.pane_order.iter().filter_map(|id| self.panes.get(id).cloned()).collect();
        list::rows(&self.machines, &panes, &self.threads)
    }

    /// 고른 줄의 자리(없어졌으면 지금 pane 줄, 그것도 없으면 첫 줄)
    fn list_index(&self, rows: &[list::Row]) -> usize {
        let at = |r: &list::Row| rows.iter().position(|x| x == r);
        self.list_sel
            .as_ref()
            .and_then(at)
            .or_else(|| self.focus.clone().and_then(|f| at(&list::Row::Pane(f))))
            .unwrap_or_else(|| list::first(rows))
    }

    /// 목록 펴기·접기(herdr `ToggleSidebar`: 포커스는 그대로)
    fn toggle_list(&mut self) {
        self.list_open = Some(!self.list_shown(self.width));
    }

    /// 목록에서 고른 줄을 연다: pane이면 그 pane이 있는 탭으로(없으면 새 탭으로 넣는다), 새 셸이면 새 탭
    async fn open_row(&mut self, tx: &mut FrontSender, row: list::Row) -> Result<()> {
        let pane = match row {
            list::Row::Pane(p) => p,
            list::Row::Thread(k) => match self.threads.get(&k).and_then(|t| t.pane.clone()) {
                Some(p) => p,
                None => {
                    self.set_error("이 thread에는 열 pane이 없다");
                    return Ok(());
                }
            },
            list::Row::NewShell(machine) => {
                let change = LayoutChange::NewTab { machine: Some(machine), name: None, argv: vec![], cwd: None };
                return self.layout(tx, change, true).await;
            }
            _ => return Ok(()),
        };
        self.open_pane(tx, pane).await
    }

    /// pane을 연다: 탭에 있으면 그 탭·pane으로, 없으면 새 탭으로 넣는다
    async fn open_pane(&mut self, tx: &mut FrontSender, pane: String) -> Result<()> {
        if let Some(i) = self.ws.tabs.iter().position(|t| layout::leaves(&t.root).contains(&pane)) {
            self.tab = i;
            self.focus = Some(pane);
            return Ok(());
        }
        self.layout(tx, LayoutChange::AddTab { pane }, true).await
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
        let (rects, _) = layout::rects(&tab.root, self.body(area));
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

    /// 키 하나. herdr `route_key`처럼 창 → 모드 순으로 고른다. 끝나면 입력기를 모드에 맞춘다.
    async fn on_key(&mut self, k: KeyEvent, tx: &mut FrontSender) -> Result<()> {
        let r = self.route_key(k, tx).await;
        // 탭이 바뀌면 포커스를 그 탭으로
        let leaves = self.current_leaves();
        if !self.focus.as_ref().is_some_and(|f| leaves.contains(f)) {
            self.focus = leaves.first().cloned();
        }
        self.reconcile_ascii();
        r
    }

    async fn route_key(&mut self, k: KeyEvent, tx: &mut FrontSender) -> Result<()> {
        if self.overlay.is_some() {
            self.on_overlay_key(k);
            return Ok(());
        }
        let c = keys::of(&k);
        match self.mode {
            Mode::Terminal => {
                if self.is_prefix(c) {
                    self.mode = Mode::Prefix;
                    return Ok(());
                }
                if c == Combo::new(KeyCode::Char('v'), KeyModifiers::CONTROL) && self.paste_image(tx).await? {
                    return Ok(());
                }
                self.input(tx, keys::encode(&k)).await
            }
            Mode::Prefix => {
                self.mode = Mode::Terminal;
                // prefix를 두 번 누르면 그 키를 pane에 보낸다
                if self.is_prefix(c) {
                    return self.input(tx, keys::combo_bytes(c)).await;
                }
                // Esc와 모르는 키는 아무것도 보내지 않고 취소한다
                match bind::prefix_action(self.command_key(c)) {
                    Some(a) => self.act(a, tx).await,
                    None => Ok(()),
                }
            }
            Mode::Navigate => self.on_navigate_key(c, tx).await,
        }
    }

    /// prefix 뒤 키. 설정을 켰으면 두벌식 자모를 같은 자리 영문 키로 읽는다.
    fn command_key(&self, c: Combo) -> Combo {
        match c.code {
            KeyCode::Char(ch) if self.jamo_keys && c.mods.is_empty() => match keys::jamo_key(ch) {
                Some(m) => keys::normalize(KeyCode::Char(m), KeyModifiers::NONE),
                None => c,
            },
            _ => c,
        }
    }

    /// herdr `route_navigate_key`: ↑↓ 목록, Enter·1-9 열기, Tab·←→·h/j/k/l pane 포커스(탐색은 그대로),
    /// 다른 prefix 명령은 하고 탐색을 끝낸다. Esc·prefix는 취소.
    async fn on_navigate_key(&mut self, c: Combo, tx: &mut FrontSender) -> Result<()> {
        if c.code == KeyCode::Esc || self.is_prefix(c) {
            self.mode = Mode::Terminal;
            self.list_sel = None;
            return Ok(());
        }
        let rows = self.list_rows();
        let i = self.list_index(&rows);
        if c.mods.is_empty() {
            match c.code {
                KeyCode::Up => {
                    self.list_sel = rows.get(list::step(&rows, i, false)).cloned();
                    return Ok(());
                }
                KeyCode::Down => {
                    self.list_sel = rows.get(list::step(&rows, i, true)).cloned();
                    return Ok(());
                }
                KeyCode::Enter => {
                    self.mode = Mode::Terminal;
                    self.list_sel = None;
                    if let Some(r) = rows.get(i).cloned() {
                        self.open_row(tx, r).await?;
                    }
                    return Ok(());
                }
                KeyCode::Char(d @ '1'..='9') => {
                    if let Some(r) = rows.iter().filter(|r| r.selectable()).nth(d as usize - '1' as usize).cloned() {
                        self.mode = Mode::Terminal;
                        self.list_sel = None;
                        self.open_row(tx, r).await?;
                    }
                    return Ok(());
                }
                KeyCode::Tab => return self.act(Action::CycleNext, tx).await,
                KeyCode::BackTab => return self.act(Action::CyclePrevious, tx).await,
                KeyCode::Left => return self.act(Action::FocusLeft, tx).await,
                KeyCode::Right => return self.act(Action::FocusRight, tx).await,
                _ => {}
            }
        }
        match bind::prefix_action(self.command_key(c)) {
            Some(a @ (Action::FocusLeft | Action::FocusDown | Action::FocusUp | Action::FocusRight)) => {
                self.act(a, tx).await
            }
            Some(a) => {
                self.mode = Mode::Terminal;
                self.list_sel = None;
                self.act(a, tx).await
            }
            None => Ok(()),
        }
    }

    /// 창이 열려 있을 때의 키(herdr `route_overlay_key`의 Help)
    fn on_overlay_key(&mut self, k: KeyEvent) {
        let area = Rect { x: 0, y: 0, w: self.width, h: self.height };
        let prefixes = self.prefix_label();
        self.help_key(k);
        if let Some(Overlay::Help(h)) = self.overlay.as_mut() {
            h.scroll = h.scroll.min(help::max_scroll(area, &prefixes, &h.query));
        }
    }

    fn help_key(&mut self, k: KeyEvent) {
        let Some(Overlay::Help(h)) = self.overlay.as_mut() else { return };
        let page = 10;
        if h.search {
            match k.code {
                KeyCode::Esc => {
                    h.search = false;
                    h.query.clear();
                    h.scroll = 0;
                }
                KeyCode::Enter => self.overlay = None,
                KeyCode::Backspace => {
                    h.query.pop();
                    h.scroll = 0;
                }
                KeyCode::Char('u') if k.modifiers.contains(KeyModifiers::CONTROL) => {
                    h.query.clear();
                    h.scroll = 0;
                }
                KeyCode::Up => h.scroll = h.scroll.saturating_sub(1),
                KeyCode::Down => h.scroll += 1,
                KeyCode::PageUp => h.scroll = h.scroll.saturating_sub(page),
                KeyCode::PageDown => h.scroll += page,
                KeyCode::Char(ch) if !k.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
                    h.query.push(ch);
                    h.scroll = 0;
                }
                _ => {}
            }
            return;
        }
        match k.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => self.overlay = None,
            KeyCode::Char('/') => h.search = true,
            KeyCode::Up | KeyCode::Char('k') => h.scroll = h.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => h.scroll += 1,
            KeyCode::PageUp => h.scroll = h.scroll.saturating_sub(page),
            KeyCode::PageDown => h.scroll += page,
            _ => {}
        }
    }

    /// prefix 명령(herdr `record_binding`)
    async fn act(&mut self, a: Action, tx: &mut FrontSender) -> Result<()> {
        let focus = self.focus.clone();
        match a {
            Action::Help => self.overlay = Some(Overlay::Help(Help::default())),
            Action::Detach => self.quit = true,
            Action::ReloadConfig => {
                let cfg = tui_config();
                self.apply_config(&cfg);
                if self.error.is_none() {
                    self.message = Some("설정을 다시 읽었다".into());
                }
            }
            Action::OpenNotificationTarget => {
                // 질문이 열린 pane(host 순서). 지금 pane이 질문 중이면 다음 것으로.
                let asking: Vec<String> =
                    self.pane_order.iter().filter(|id| self.panes.get(*id).is_some_and(is_asking)).cloned().collect();
                let at = focus.as_ref().and_then(|f| asking.iter().position(|p| p == f));
                let next = match at {
                    Some(i) => asking.get((i + 1) % asking.len()).cloned(),
                    None => asking.first().cloned(),
                };
                if let Some(p) = next {
                    self.open_pane(tx, p).await?;
                }
            }
            Action::WorkspacePicker => {
                self.mode = Mode::Navigate;
                self.list_sel = None;
            }
            Action::NewTab => {
                let machine = self.focused_machine();
                self.layout(tx, LayoutChange::NewTab { machine, name: None, argv: vec![], cwd: None }, true).await?;
            }
            Action::PreviousTab if !self.ws.tabs.is_empty() => {
                self.tab = (self.tab + self.ws.tabs.len() - 1) % self.ws.tabs.len()
            }
            Action::NextTab if !self.ws.tabs.is_empty() => self.tab = (self.tab + 1) % self.ws.tabs.len(),
            Action::SwitchTab(i) if i < self.ws.tabs.len() => self.tab = i,
            Action::PreviousTab | Action::NextTab | Action::SwitchTab(_) => {}
            // herdr split_vertical(v)는 오른쪽에, split_horizontal(-)은 아래에 새 pane을 둔다
            Action::SplitVertical | Action::SplitHorizontal => {
                let Some(pane) = focus else { return Ok(()) };
                let dir = if a == Action::SplitVertical { SplitDir::Horizontal } else { SplitDir::Vertical };
                self.layout(tx, LayoutChange::Split { pane, dir, machine: None, argv: vec![], cwd: None }, true)
                    .await?;
            }
            Action::ClosePane => {
                let Some(pane) = focus else { return Ok(()) };
                self.layout(tx, LayoutChange::Close { pane, kill: true }, false).await?;
            }
            Action::ToggleSidebar => self.toggle_list(),
            Action::FocusLeft => self.move_focus(Dir::Left),
            Action::FocusDown => self.move_focus(Dir::Down),
            Action::FocusUp => self.move_focus(Dir::Up),
            Action::FocusRight => self.move_focus(Dir::Right),
            Action::CycleNext | Action::CyclePrevious => {
                let leaves = self.current_leaves();
                if !leaves.is_empty() {
                    let at = focus.and_then(|f| leaves.iter().position(|p| *p == f)).unwrap_or(0);
                    let n = leaves.len();
                    let next = if a == Action::CycleNext { (at + 1) % n } else { (at + n - 1) % n };
                    self.focus = leaves.get(next).cloned();
                }
            }
        }
        Ok(())
    }

    /// 지금 탭의 pane 칸들(그린 화면 그대로)
    fn pane_rects(&self) -> Vec<(String, Rect)> {
        let Some(tab) = self.ws.tabs.get(self.tab) else { return vec![] };
        layout::rects(&tab.root, self.body(Rect { x: 0, y: 0, w: self.width, h: self.height })).0
    }

    /// 방향으로 이웃 pane에 포커스한다(herdr `layout::find_in_direction`: 그 방향에 있고 겹치는 pane 중
    /// 가장자리가 가깝고, 많이 겹치고, 가운데가 가까운 것)
    fn move_focus(&mut self, dir: Dir) {
        let rects = self.pane_rects();
        let Some(cur) = rects.iter().find(|(p, _)| Some(p) == self.focus.as_ref()).map(|(_, r)| *r) else { return };
        if let Some(p) = find_in_direction(cur, dir, &rects) {
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
        let body = self.body(area);
        let list_shown = self.list_shown(area.w);
        if list_shown {
            self.render_list(buf, area);
        }
        let mut cursor = None;
        match self.ws.tabs.get(self.tab) {
            None => {
                let lines = [
                    ("탭이 없다".to_string(), Style::default().add_modifier(Modifier::BOLD)),
                    (format!("{} 다음 c로 새 탭 · ?로 단축키", self.prefix_label()), Style::default().fg(DIM)),
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
        // 탐색 모드인데 목록이 접혀 있으면 pane 위에 띄워 보인다(pane 크기는 그대로)
        if self.mode == Mode::Navigate && !list_shown {
            self.render_list(buf, area);
        }
        // 상태 줄: 오류나 모드가 있으면 herdr 모드 막대(`render_mode_bar`), 아니면 색 조각들(머신 · pane · 상태 …)
        let y = area.y + area.h.saturating_sub(1);
        for dx in 0..area.w {
            buf.set_string(area.x + dx, y, " ", Style::default());
        }
        let mut x = area.x;
        let segments = self.mode_bar().unwrap_or_else(|| self.status_segments());
        for (text, st) in segments {
            let room = right_edge.saturating_sub(x) as usize;
            if room == 0 {
                break;
            }
            x = buf.set_stringn(x, y, &text, room, st).0;
        }
        if let Some(Overlay::Help(h)) = &self.overlay {
            return help::render(buf, area, h, &self.prefix_label());
        }
        cursor
    }

    fn render_list(&self, buf: &mut Buffer, area: Rect) {
        let rows = self.list_rows();
        let view = list::View {
            machines: &self.machines,
            panes: &self.panes,
            threads: &self.threads,
            current: self.focus.as_deref(),
            sel: self.list_index(&rows),
            focused: self.mode == Mode::Navigate,
        };
        list::render(buf, Self::list_rect(area), &rows, &view);
    }

    /// herdr `render_mode_bar`: 오류가 있으면 ERROR, prefix·탐색 모드면 그 안내. 평소에는 없다.
    fn mode_bar(&self) -> Option<Vec<(String, Style)>> {
        let base = Style::default().fg(DIM);
        let key = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
        let chip = Style::default().fg(ON_CHIP).bg(ACCENT).add_modifier(Modifier::BOLD);
        let s = |t: &str, st: Style| (t.to_string(), st);
        if let Some((e, _)) = &self.error {
            return Some(vec![s(" ERROR ", chip), (format!(" {e}"), base)]);
        }
        let help = bind::rhs(Action::Help);
        match self.mode {
            Mode::Terminal => None,
            Mode::Prefix => Some(vec![
                s(" PREFIX ", chip),
                s(" ", base),
                s("esc", key),
                s(" 취소  ", base),
                (self.prefix_label(), key),
                s(" prefix 보내기  ", base),
                (bind::rhs(Action::WorkspacePicker), key),
                s(" 목록 탐색  ", base),
                (help, key),
                s(" 단축키", base),
            ]),
            Mode::Navigate => Some(vec![
                s(" NAVIGATE ", chip),
                s(" esc 돌아가기  ", base),
                s("↑/↓", key),
                s(" 목록  ", base),
                s("enter", key),
                s(" 열기  ", base),
                s("tab", key),
                s(" pane  ", base),
                (help, key),
                s(" 단축키", base),
            ]),
        }
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
pub(crate) const ACCENT: Color = Color::Indexed(141);
pub(crate) const WORKING: Color = Color::Indexed(75);
pub(crate) const BLOCKED: Color = Color::Indexed(210);
pub(crate) const DONE: Color = Color::Indexed(114);
pub(crate) const WARN: Color = Color::Indexed(221);
pub(crate) const DIM: Color = Color::Indexed(245);
pub(crate) const FAINT: Color = Color::Indexed(240);
/// 도움말의 키 글자(herdr palette `mauve` 자리)
pub(crate) const MAUVE: Color = Color::Indexed(176);
/// 색 바탕 위 글자. ANSI 0(검정)은 굵게 쓰면 많은 터미널이 밝은 회색으로 바꾸므로 256색 검정(16)을 쓴다.
pub(crate) const ON_CHIP: Color = Color::Indexed(16);

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

pub(crate) fn activity_text(a: aaa_proto::AgentActivity) -> &'static str {
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
        // herdr에는 재개 키가 없다(복원 때 자동). 사람이 다시 띄우는 길은 CLI다.
        (SessionState::Interrupted, _) | (SessionState::Lost, _) => format!(
            " {} — aaa pane resume {} ",
            if p.state == SessionState::Lost { "유실" } else { "중단(재부팅)" },
            p.id.chars().take(8).collect::<String>()
        ),
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

pub(crate) fn unicode_width(s: &str) -> usize {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// herdr 0.9.5 `src/layout.rs` `find_in_direction`: 그 방향에 있고 다른 축으로 겹치는 pane 중
/// (가장자리 거리, 덜 겹침, 가운데 거리, 차례)가 가장 작은 것
fn find_in_direction(cur: Rect, dir: Dir, panes: &[(String, Rect)]) -> Option<String> {
    let overlap = |a: u16, al: u16, b: u16, bl: u16| a < b + bl && a + al > b;
    let amount = |a: u16, al: u16, b: u16, bl: u16| (a + al).min(b + bl).saturating_sub(a.max(b));
    let center = |a: u16, al: u16, b: u16, bl: u16| (a * 2 + al).abs_diff(b * 2 + bl);
    panes
        .iter()
        .enumerate()
        .filter(|(_, (_, r))| *r != cur)
        .filter(|(_, (_, r))| match dir {
            Dir::Left => r.x + r.w <= cur.x && overlap(r.y, r.h, cur.y, cur.h),
            Dir::Right => r.x >= cur.x + cur.w && overlap(r.y, r.h, cur.y, cur.h),
            Dir::Up => r.y + r.h <= cur.y && overlap(r.x, r.w, cur.x, cur.w),
            Dir::Down => r.y >= cur.y + cur.h && overlap(r.x, r.w, cur.x, cur.w),
        })
        .min_by_key(|(i, (_, r))| {
            let edge = match dir {
                Dir::Left => cur.x.saturating_sub(r.x + r.w),
                Dir::Right => r.x.saturating_sub(cur.x + cur.w),
                Dir::Up => cur.y.saturating_sub(r.y + r.h),
                Dir::Down => r.y.saturating_sub(cur.y + cur.h),
            };
            let (ov, ce) = match dir {
                Dir::Left | Dir::Right => (amount(r.y, r.h, cur.y, cur.h), center(r.y, r.h, cur.y, cur.h)),
                Dir::Up | Dir::Down => (amount(r.x, r.w, cur.x, cur.w), center(r.x, r.w, cur.x, cur.w)),
            };
            (edge, std::cmp::Reverse(ov), ce, *i)
        })
        .map(|(_, (p, _))| p.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: u16, y: u16, w: u16, h: u16) -> Rect {
        Rect { x, y, w, h }
    }

    /// prefix 뒤 두벌식 자모는 설정을 켰을 때만 같은 자리 영문 키다(`[tui] prefix_jamo_keys`, 기본 끔)
    #[test]
    fn jamo_command_keys_only_when_enabled() {
        let mut app = App::new("main".into(), "me".into());
        let jamo = Combo::plain('ㅊ');
        assert_eq!(app.command_key(jamo), jamo);
        assert_eq!(bind::prefix_action(app.command_key(jamo)), None);
        app.jamo_keys = true;
        assert_eq!(bind::prefix_action(app.command_key(jamo)), Some(Action::NewTab));
        assert_eq!(app.command_key(Combo::plain('ㅆ')), Combo::shift('t'));
    }

    #[test]
    fn focus_moves_to_the_overlapping_neighbour_like_herdr() {
        // ┌a──┬b──┐
        // │   ├c──┤
        // └───┴───┘   a 오른쪽은 b·c 둘 다 닿는다: 더 많이 겹친 쪽(같으면 가운데가 가까운 쪽, 그다음 차례)
        let panes = vec![
            ("a".to_string(), r(0, 0, 40, 20)),
            ("b".to_string(), r(41, 0, 39, 9)),
            ("c".to_string(), r(41, 10, 39, 10)),
        ];
        assert_eq!(find_in_direction(panes[0].1, Dir::Right, &panes).as_deref(), Some("c"));
        assert_eq!(find_in_direction(panes[1].1, Dir::Down, &panes).as_deref(), Some("c"));
        assert_eq!(find_in_direction(panes[2].1, Dir::Up, &panes).as_deref(), Some("b"));
        assert_eq!(find_in_direction(panes[2].1, Dir::Left, &panes).as_deref(), Some("a"));
        // 그 방향에 없으면 그대로
        assert_eq!(find_in_direction(panes[0].1, Dir::Left, &panes), None);
        assert_eq!(find_in_direction(panes[0].1, Dir::Up, &panes), None);
    }
}
