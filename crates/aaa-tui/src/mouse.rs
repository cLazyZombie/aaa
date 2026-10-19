//! 마우스(묶음 3d). herdr 0.9.5 `src/client/shell/mouse.rs`(맞히기 순서·누름/뗌·끌기·나눔 끌기·탭 끌기),
//! `src/client/shell/tabs.rs`(`tab_drop_index_at`·`tab_drop_indicator_x`), `word_selection.rs`(두 번 누름 낱말),
//! `src/ui/panes.rs` `automatic_selection_style`(선택 색)을 옮겼다. 그린 자리는 그릴 때 `Hits`에 적고, 사건은 그 자리로 맞힌다.
//!
//! - 탭 막대: 누르고 그 자리에서 떼면 그 탭, 한 칸 넘게 끌면 옮길 자리에 accent `│`를 두고 떼면 옮긴다(`MoveTab`),
//!   휠은 앞·뒤 탭, ` + ` 새 탭, ` < ` ` > ` 넘기기.
//! - 사이드바: 줄을 누르고 떼면 연다, `«`·`»` 접기·펴기, 마지막 열(경계)을 끌면 폭(`[tui] sidebar_min_width`~`sidebar_max_width`, 기본 18~72)·두 번 누르면 설정 폭, 휠은 한 줄씩.
//! - 나눔 경계: 끌면 그 나눔의 비율(`SplitRatio`, 0.1~0.9, 33ms마다·뗄 때).
//! - pane: 누르면 포커스, 안쪽을 끌면 보이는 화면 글을 고르고 떼면 복사(`copy_on_select`), 두 번 누르면 낱말.
//!   프로그램이 마우스 보고를 켰으면(`GridFrame.modes`) 누름·끌기·뗌·움직임·휠을 그 모드의 인코딩으로 넘긴다.
//!   보고를 끈 alt 화면이면 휠을 위·아래 화살표로 보낸다(herdr `WheelRouting::AlternateScroll`).
//! - 링크: ctrl+왼쪽 누름이면 host에 그 칸의 링크를 묻고(`PaneLink`), http(s)면 이 기기에서 연다(`open.rs`). 아니면
//!   그 누름을 보통 누름으로 다시 넣는다(herdr `pane.link.activate`·`ReplayMouse`, plan §8.194).
//! - 창: 이름·닫기 확인은 단추가 아닌 곳을 누르면 닫고(안쪽도), 단축키 창은 밖을 누를 때만 닫는다.
//! - 오른쪽 누름: 탭 이름표면 탭 메뉴, pane이면 pane 메뉴(herdr `context_menu.rs`). 메뉴에서 「오른쪽 누름을 pane에 보내기」를
//!   켠 보고 pane에는 수정키 없는 오른쪽 누름을 넘긴다. 메뉴는 누르거나 위·아래·Enter로 고르고, 밖을 누르거나 Esc면 닫는다.
//!
//! 보통 화면의 휠 기록 넘기기는 copy 모드 묶음(5)이다.
//!
//! herdr(https://github.com/herdrdev/herdr, Apache License 2.0)의 코드를 옮겨 aaa에 맞게 고쳤다.

use std::time::Duration;

use aaa_proto::layout::Rect;
use aaa_proto::{
    FrontRequest, GridRun, GridScroll, LayoutChange, MouseEncoding, MouseMode, PaneModes, ScrollBy, SplitDir, TextPoint,
};
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier, Style};
use tokio::time::Instant;

use crate::bind::Action;
use crate::chrome::{PaneBox, SplitHit};
use crate::list::Row;
use crate::theme::Palette;
use crate::{
    App, DIVIDER_DOUBLE_CLICK, FrontSender, Mode, Overlay, Pending, clip, help, keys, open, scrollbar, unicode_width,
};

/// 링크를 묻는 누름(herdr `PendingEndpointKind::PaneLinkActivate`): 요청 id, 누른 pane과 그 안쪽 칸, 링크가 아니면 다시 넣을
/// 누름과 그 뒤 왼쪽 끌기·놓기
#[derive(Debug, Clone)]
pub struct LinkPress {
    pub id: String,
    pane: String,
    inner: Rect,
    events: Vec<MouseEvent>,
}

#[cfg(test)]
impl LinkPress {
    pub fn new(id: &str, b: &PaneBox, press: MouseEvent) -> Self {
        LinkPress { id: id.into(), pane: b.pane.clone(), inner: b.inner, events: vec![press] }
    }
}

/// `PaneLink`를 아는 host Front API minor
const LINK_MINOR: u16 = 35;

/// 두 번 누름으로 보는 사이(herdr `DOUBLE_CLICK_WINDOW`)
pub const DOUBLE_CLICK: Duration = Duration::from_millis(350);
/// 두 번 눌러 고른 낱말을 보이는 시간(herdr word highlight)
pub const WORD_SHOWN: Duration = Duration::from_millis(500);
/// 나눔을 끄는 동안 비율을 보내는 사이(herdr 33ms)
pub const SPLIT_SEND: Duration = Duration::from_millis(33);
/// 끌어 고르다 가장자리에 닿았을 때 한 줄씩 넘기는 사이(herdr `SELECTION_AUTOSCROLL_INTERVAL`)
pub const SELECTION_AUTOSCROLL: Duration = Duration::from_millis(30);

/// pane 밖으로 `d`줄 끌었을 때 바로 넘기는 줄 수(herdr `selection_edge_scroll_lines`)
pub fn edge_lines(d: u16) -> i64 {
    (i64::from(d) * 3).clamp(3, 15)
}

/// 화면 맨 위 줄의 번호(기록 맨 앞 줄이 0, herdr `viewport_top`)
pub fn view_top(s: GridScroll) -> u32 {
    s.max.saturating_sub(s.offset)
}

/// 창의 단추
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Save,
    Clear,
    Cancel,
    Close,
}

/// 그린 자리(마지막으로 그린 화면)
#[derive(Debug, Clone, Default)]
pub struct Hits {
    /// 탭 이름표 자리와 탭 차례
    pub tabs: Vec<(Rect, usize)>,
    pub tab_bar: Option<Rect>,
    pub new_tab: Option<Rect>,
    /// spaces 구역 맨 아랫줄 ` new`와 오른쪽 `menu`
    pub new_workspace: Option<Rect>,
    /// 바닥 줄 ` · 머신`: 새 탭·workspace를 열 머신을 다음 붙은 머신으로
    pub pick_machine: Option<Rect>,
    pub global_menu: Option<Rect>,
    /// 설정 창의 구역 이름표·고르는 줄·단추
    pub settings: Option<crate::settings::Hits>,
    pub scroll_left: Option<Rect>,
    pub scroll_right: Option<Rect>,
    /// 탭 막대가 지금 몇째 탭부터 보이나와 가장 많이 밀 수 있는 값
    pub tab_scroll: usize,
    pub max_tab_scroll: usize,
    /// 사이드바(펼침·접힘·띄운 목록) 전체 칸
    pub sidebar: Option<Rect>,
    /// 목록 줄: 줄 칸과 그 줄
    pub rows: Vec<(Rect, Row)>,
    /// 펼친 목록의 첫 보이는 줄과 보이는 줄 수(휠로 민다)
    pub list_skip: usize,
    pub list_h: u16,
    pub toggle: Option<Rect>,
    /// 펼친 사이드바의 오른쪽 경계(끌면 폭)
    pub divider: Option<Rect>,
    /// 펼친 사이드바의 spaces·agents 구역 경계 줄(끌면 두 구역 비율, herdr `sidebar_section_divider`)
    pub section_divider: Option<Rect>,
    /// 좁은 화면 머리의 `목록` 단추
    pub header_button: Option<Rect>,
    pub panes: Vec<PaneBox>,
    /// 끌 수 있는 나눔 경계(바깥 나눔이 앞)
    pub splits: Vec<SplitHit>,
    /// 보이는 agent 알림 카드(누르면 그 pane, herdr `notification_toast`)
    pub card: Option<Rect>,
    /// 단축키 창
    pub popup: Option<Rect>,
    /// 이름·닫기 확인 창의 단추
    pub buttons: Vec<(Rect, Button)>,
    /// 오른쪽 누름 메뉴의 줄과 항목 차례
    pub menu_rows: Vec<(Rect, usize)>,
}

pub fn contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

impl Hits {
    pub fn pane_at(&self, x: u16, y: u16) -> Option<&PaneBox> {
        self.panes.iter().find(|b| contains(b.rect, x, y))
    }

    pub fn row_at(&self, x: u16, y: u16) -> Option<&Row> {
        self.rows.iter().find(|(r, _)| contains(*r, x, y)).map(|(_, row)| row)
    }

    pub fn tab_at(&self, x: u16, y: u16) -> Option<usize> {
        self.tabs.iter().find(|(r, _)| contains(*r, x, y)).map(|(_, i)| *i)
    }

    pub fn button_at(&self, x: u16, y: u16) -> Option<Button> {
        self.buttons.iter().find(|(r, _)| contains(*r, x, y)).map(|(_, b)| *b)
    }

    pub fn on(&self, r: Option<Rect>, x: u16, y: u16) -> bool {
        r.is_some_and(|r| contains(r, x, y))
    }
}

/// pane 화면의 한 칸(줄, 열). 0부터.
pub type Cell = (u16, u16);

/// pane 글 자리(줄, 열): 줄은 기록 맨 앞 줄이 0이고 화면 줄이 그 뒤에 잇는다(herdr selection 절대 줄, `TextPoint`)
pub type Point = (u32, u16);

/// 고른 글 범위(끝 칸 포함). 기록 줄 번호로 두어 기록을 넘겨도 고른 글을 따라간다(herdr `Selection`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub pane: String,
    pub anchor: Point,
    pub head: Point,
    /// 두 번 누름 낱말: 이 때까지만 보인다
    pub until: Option<Instant>,
}

impl Selection {
    /// 읽는 차례로(`start` ≤ `end`)
    pub fn range(&self) -> (Point, Point) {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }

    /// 화면 맨 위 줄이 `top`일 때 화면 칸 (row, col)을 골랐나
    pub fn covers(&self, top: u32, row: u16, col: u16) -> bool {
        let at = (top + u32::from(row), col);
        let (start, end) = self.range();
        at >= start && at <= end
    }
}

/// 끌어 고르다 pane 위·아래 가장자리(밖 포함)에 닿았다: `at`마다 한 줄씩 그쪽으로 넘긴다(herdr selection autoscroll).
/// 마지막 마우스 자리와 그때의 pane 칸을 둔다(칸이 바뀌면 멈춘다).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoScroll {
    pub pane: String,
    pub up: bool,
    pub x: u16,
    pub y: u16,
    pub inner: Rect,
    pub at: Instant,
}

/// 누른 채 끄는 것
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drag {
    /// 탭 이름표를 눌렀다(그 자리에서 떼면 그 탭). 누른 칸에서 한 칸 넘게 끌면 `moving`, `insert`는 옮겨 넣을 자리.
    Tab { index: usize, from: (u16, u16), moving: bool, insert: Option<usize> },
    /// 나눔 경계를 끈다(herdr `ClientChromeDrag::PaneSplit`): 누른 칸과 경계 자리의 차이 `grab`, 마지막으로 보낸 비율과 때
    Split { hit: SplitHit, tab: String, grab: i32, sent: Option<u16>, at: Option<Instant> },
    /// 마우스 보고를 켠 pane에 넘긴 누름(뗄 때까지 그 pane이 끌기·뗌을 받는다, 밖으로 나가도)
    Pane { pane: String, button: MouseButton },
    /// pane scrollbar thumb를 `grab`째 줄로 잡고 끈다(herdr `ClientChromeDrag::PaneScrollbar`): 마지막으로 보낸 위치와 때
    Scrollbar { pane: String, grab: u16, sent: Option<usize>, at: Option<Instant> },
    /// 목록 줄을 눌렀다(같은 줄에서 떼면 연다)
    Row(Row),
    /// 사이드바 경계를 끈다
    Sidebar,
    /// 사이드바 구역 경계를 끈다
    SidebarSection,
    /// pane 글을 고른다. `moved`는 누른 칸을 벗어났나(herdr: 벗어나야 보인다). `offset`은 고르는 동안 보낸 기록 위치
    /// (herdr처럼 화면 frame을 기다리지 않고 그 위치로 고른 끝을 잡는다, 보낸 적이 없으면 화면의 위치)
    Select { pane: String, anchor: Point, moved: bool, offset: Option<u32> },
}

/// 오른쪽 누름 메뉴의 대상(herdr `ClientContextMenuTarget`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuTarget {
    Tab {
        index: usize,
    },
    /// `source`는 메뉴를 열 때 포커스였던 다른 pane(자리 바꾸기), `passthrough`는 오른쪽 누름을 pane에 보내나,
    /// `labeled`는 사람이 붙인 이름이 있나(herdr `has_manual_label`)
    Pane {
        pane: String,
        source: Option<String>,
        passthrough: bool,
        labeled: bool,
    },
    /// 전체 메뉴(herdr `GlobalMenu`, spaces 맨 아랫줄 오른쪽 `menu`)
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    NewTab,
    Rename,
    Close,
    RenamePane,
    ClearPaneName,
    Swap,
    SplitRight,
    SplitDown,
    Zoom,
    RightClick,
    ClosePane,
    Settings,
    Keybinds,
    ReloadConfig,
    Detach,
}

/// 오른쪽 누름 메뉴(herdr `ClientContextMenuOverlay`): 누른 자리에 띄우고 첫 항목을 고른다
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub target: MenuTarget,
    pub x: u16,
    pub y: u16,
    pub highlighted: usize,
}

impl Menu {
    /// 항목(herdr `ClientContextMenuOverlay::items`)
    pub fn items(&self) -> Vec<(&'static str, MenuAction)> {
        use MenuAction::*;
        match &self.target {
            MenuTarget::Tab { .. } => vec![("새 탭", NewTab), ("이름 바꾸기", Rename), ("닫기", Close)],
            MenuTarget::Pane { source, passthrough, labeled, .. } => {
                let mut items = vec![("pane 이름 바꾸기", RenamePane)];
                if *labeled {
                    items.push(("pane 이름 지우기", ClearPaneName));
                }
                if source.is_some() {
                    items.push(("포커스 pane과 자리 바꾸기", Swap));
                }
                items.extend([
                    ("오른쪽으로 나누기", SplitRight),
                    ("아래로 나누기", SplitDown),
                    ("크게 보기", Zoom),
                    (
                        if *passthrough {
                            "오른쪽 누름은 aaa 메뉴로"
                        } else {
                            "오른쪽 누름을 pane에 보내기"
                        },
                        RightClick,
                    ),
                    ("pane 닫기", ClosePane),
                ]);
                items
            }
            // herdr `global_menu_items`(새 판 알림 「what's new」은 aaa에 없다)
            MenuTarget::Global => {
                vec![("설정", Settings), ("단축키", Keybinds), ("설정 다시 읽기", ReloadConfig), ("나가기", Detach)]
            }
        }
    }

    /// 전체 메뉴: `menu` 단추의 오른쪽 끝에 맞춰 그 위에 띄운다(herdr `render_global_menu`)
    pub fn global(launcher: Rect) -> Menu {
        let mut m = Menu { target: MenuTarget::Global, x: 0, y: 0, highlighted: 0 };
        let items = m.items();
        let w = (items.iter().map(|(l, _)| unicode_width(l) as u16).max().unwrap_or(0) + 4).max(14);
        let h = items.len() as u16 + 2;
        m.x = (launcher.x + launcher.w).saturating_sub(w);
        m.y = launcher.y.saturating_sub(h);
        m
    }

    /// herdr `move_context_menu_selection`: 끝에서 멈춘다
    pub fn step(&mut self, down: bool) {
        let last = self.items().len().saturating_sub(1);
        self.highlighted = if down { (self.highlighted + 1).min(last) } else { self.highlighted.saturating_sub(1) };
    }
}

/// herdr `render_context_menu`: 폭은 가장 긴 항목 + 4(14 이상), 높이는 항목 + 2, 누른 자리에서 화면 안으로 민다.
/// accent 테두리·`panel_bg`, 고른 줄은 accent 바탕 굵게. 창 칸과 (줄 칸, 항목 차례)를 준다.
pub fn render_menu(buf: &mut Buffer, area: Rect, menu: &Menu, p: &Palette) -> (Rect, Vec<(Rect, usize)>) {
    let items = menu.items();
    let widest = items.iter().map(|(l, _)| unicode_width(l) as u16).max().unwrap_or(0);
    let w = (widest + 4).max(14).min(area.w.max(1));
    let h = (items.len() as u16 + 2).min(area.h.max(1));
    let x = menu.x.min(area.x + area.w.saturating_sub(w));
    let y = menu.y.min(area.y + area.h.saturating_sub(h));
    let rect = Rect { x, y, w, h };
    if w < 2 || h < 2 {
        return (rect, Vec::new());
    }
    let inner = help::panel(buf, rect, p.accent, p.panel_bg);
    let mut rows = Vec::new();
    for (i, (label, _)) in items.iter().enumerate() {
        let row = Rect { x: inner.x, y: inner.y + i as u16, w: inner.w, h: 1 };
        if row.y >= inner.y + inner.h {
            break;
        }
        let style = if i == menu.highlighted {
            Style::default().fg(crate::theme::contrast(p)).bg(p.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.text).bg(p.panel_bg)
        };
        buf.set_style(ratatui::layout::Rect::new(row.x, row.y, row.w, 1), style);
        buf.set_stringn(row.x, row.y, label, row.w as usize, style);
        rows.push((row, i));
    }
    (rect, rows)
}

/// pane 프로그램에 넘길 마우스 사건
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Report {
    Press(MouseButton),
    Release(MouseButton),
    /// 누른 채 움직임
    Drag(MouseButton),
    /// 누르지 않고 움직임(1003)
    Move,
    /// 휠(true면 아래)
    Wheel(bool),
}

/// xterm 마우스 보고 bytes(herdr는 ghostty 인코더). `col`·`row`는 pane 안쪽 0부터. 그 모드가 받지 않는 사건이거나
/// 좌표를 그 인코딩으로 쓸 수 없으면 None.
/// - 9(누름만): 누름·휠, 수정키 없음. 1000: + 뗌. 1002: + 누른 채 움직임. 1003: + 모든 움직임.
/// - 버튼 0 왼쪽·1 가운데·2 오른쪽, 뗌 3(SGR은 그 버튼에 `m`), 움직임 +32, 휠 64·65, shift +4·alt +8·ctrl +16.
/// - 기본 X10은 `ESC [ M` 뒤 32를 더한 세 byte(223칸까지), 1005는 그 값을 UTF-8 글자로(2015칸까지), 1006은 `ESC [ < b ; x ; y M/m`.
pub fn report(m: PaneModes, r: Report, col: u16, row: u16, mods: KeyModifiers) -> Option<Vec<u8>> {
    use MouseMode::{AnyMotion, ButtonMotion, PressRelease};
    let takes = match r {
        Report::Press(_) | Report::Wheel(_) => m.mouse != MouseMode::None,
        Report::Release(_) => matches!(m.mouse, PressRelease | ButtonMotion | AnyMotion),
        Report::Drag(_) => matches!(m.mouse, ButtonMotion | AnyMotion),
        Report::Move => m.mouse == AnyMotion,
    };
    if !takes {
        return None;
    }
    let button = |b: MouseButton| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let sgr = m.mouse_encoding == MouseEncoding::Sgr;
    let mut code: u32 = match r {
        Report::Press(b) => button(b),
        Report::Release(b) if sgr => button(b),
        Report::Release(_) => 3,
        Report::Drag(b) => 32 + button(b),
        Report::Move => 32 + 3,
        Report::Wheel(down) => 64 + u32::from(down),
    };
    if m.mouse != MouseMode::Press {
        code += 4 * u32::from(mods.contains(KeyModifiers::SHIFT))
            + 8 * u32::from(mods.contains(KeyModifiers::ALT))
            + 16 * u32::from(mods.contains(KeyModifiers::CONTROL));
    }
    let (x, y) = (u32::from(col) + 1, u32::from(row) + 1);
    match m.mouse_encoding {
        MouseEncoding::Sgr => {
            let end = if matches!(r, Report::Release(_)) { 'm' } else { 'M' };
            Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes())
        }
        MouseEncoding::Default => {
            (x <= 223 && y <= 223).then(|| vec![0x1b, b'[', b'M', (32 + code) as u8, (32 + x) as u8, (32 + y) as u8])
        }
        MouseEncoding::Utf8 => {
            if x > 2015 || y > 2015 {
                return None;
            }
            let mut out = b"\x1b[M".to_vec();
            for v in [32 + code, 32 + x, 32 + y] {
                out.extend(char::from_u32(v)?.to_string().as_bytes());
            }
            Some(out)
        }
    }
}

/// 나눔을 끈 자리의 비율(herdr `pane_split_ratio`: (포인터 + grab − 원점) / 길이, 0.1~0.9). 천분율로 반올림한다:
/// 1000칸보다 좁으면 `split_rect`의 반올림이 그 칸을 그대로 되돌린다.
pub fn split_ratio(hit: &SplitHit, grab: i32, x: u16, y: u16) -> u16 {
    let (pointer, origin, len) = match hit.dir {
        SplitDir::Horizontal => (i32::from(x), i32::from(hit.area.x), i32::from(hit.area.w.max(1))),
        SplitDir::Vertical => (i32::from(y), i32::from(hit.area.y), i32::from(hit.area.h.max(1))),
    };
    let at = pointer + grab - origin;
    ((at * 1000 + len / 2).div_euclid(len)).clamp(100, 900) as u16
}

/// 끌던 탭을 넣을 자리(herdr `tab_drop_index_at`): 탭 줄 위에서만. ` < `·` > `는 맨 앞·맨 끝, 이름표 가운데보다 왼쪽이면 그 앞.
pub fn drop_index(hits: &Hits, x: u16, y: u16, count: usize) -> Option<usize> {
    let (first_rect, first) = *hits.tabs.first()?;
    let (last_rect, last) = *hits.tabs.last()?;
    if y != first_rect.y {
        return None;
    }
    if hits.on(hits.scroll_left, x, y) {
        return Some(0);
    }
    if hits.on(hits.scroll_right, x, y) {
        return Some(count);
    }
    let left = if first == 0 { first_rect.x } else { hits.scroll_left.map_or(first_rect.x, |r| r.x + r.w) };
    let right = if last + 1 >= count {
        last_rect.x + last_rect.w
    } else {
        hits.scroll_right.map_or(last_rect.x + last_rect.w, |r| r.x.saturating_sub(1))
    };
    if x <= left {
        return Some(first);
    }
    if x >= right {
        return Some(last + 1);
    }
    for (r, i) in &hits.tabs {
        if x < r.x + r.w / 2 {
            return Some(*i);
        }
        if x < r.x + r.w {
            return Some(i + 1);
        }
    }
    Some(last + 1)
}

/// 넣을 자리 표시 `│`의 열(herdr `tab_drop_indicator_x`)
pub fn drop_x(hits: &Hits, insert: usize, count: usize) -> Option<u16> {
    let (first_rect, first) = *hits.tabs.first()?;
    let (last_rect, last) = *hits.tabs.last()?;
    if insert == 0 {
        return Some(if first == 0 { first_rect.x } else { hits.scroll_left.map_or(first_rect.x, |r| r.x + r.w) });
    }
    if let Some((r, _)) = hits.tabs.iter().find(|(_, i)| *i == insert) {
        return Some(r.x.saturating_sub(1));
    }
    if insert >= count {
        return Some(if last + 1 >= count {
            last_rect.x + last_rect.w
        } else {
            hits.scroll_right.map_or(last_rect.x + last_rect.w, |r| r.x.saturating_sub(1))
        });
    }
    None
}

/// 한 줄의 칸들: (열, 글자, 폭). 넓은 글자는 두 칸이고 뒤 칸은 없다.
pub fn cells(runs: &[GridRun]) -> Vec<(u16, char, u16)> {
    let mut out = Vec::new();
    let mut col = 0u16;
    for run in runs {
        for ch in run.t.chars() {
            let w = unicode_width(ch.encode_utf8(&mut [0; 4])) as u16;
            if w == 0 {
                continue;
            }
            out.push((col, ch, w));
            col += w;
        }
    }
    out
}

/// 고른 범위의 글(herdr `PaneSelectionRead`): 줄마다 끝 빈칸을 빼고 줄바꿈으로 잇는다. 넓은 글자는 걸치면 넣는다.
pub fn text(lines: &[Vec<GridRun>], (start, end): (Cell, Cell)) -> String {
    let mut out: Vec<String> = Vec::new();
    for row in start.0..=end.0 {
        let from = if row == start.0 { start.1 } else { 0 };
        let to = if row == end.0 { end.1 } else { u16::MAX };
        let line: String = lines
            .get(row as usize)
            .map(|runs| {
                cells(runs).into_iter().filter(|(c, _, w)| c + w > from && *c <= to).map(|(_, ch, _)| ch).collect()
            })
            .unwrap_or_default();
        out.push(line.trim_end().to_string());
    }
    out.join("\n")
}

/// 낱말을 가르는 글자(herdr `word_bounds_at_column`의 구분자 토큰). `:`·`/`·`.`은 가르지 않아 URL·경로가 한 낱말이다.
fn separator(c: char) -> bool {
    c.is_whitespace() || "\"'`()[]{}<>,;|".contains(c)
}

/// `col` 칸의 낱말 범위(열, 끝 칸 포함). 빈칸·구분자 위면 None.
pub fn word_at(runs: &[GridRun], col: u16) -> Option<(u16, u16)> {
    let cs = cells(runs);
    let at = cs.iter().position(|(c, _, w)| col >= *c && col < c + w)?;
    if separator(cs[at].1) {
        return None;
    }
    let mut a = at;
    while a > 0 && !separator(cs[a - 1].1) {
        a -= 1;
    }
    let mut b = at;
    while b + 1 < cs.len() && !separator(cs[b + 1].1) {
        b += 1;
    }
    Some((cs[a].0, cs[b].0 + cs[b].2 - 1))
}

/// 선택 칸의 색(herdr `automatic_selection_style`): 바탕을 흰색(어두운 바탕)·검정(밝은 바탕) 쪽으로 28% 섞고, 글은 대비가 큰 쪽.
/// aaa는 터미널 바탕색을 묻지 않아 herdr의 대신 값(`panel_bg`, 없으면 `surface_dim`)에서 섞는다.
pub fn selection_colors(p: &Palette) -> (Color, Color) {
    let base = if p.panel_bg == Color::Reset { p.surface_dim } else { p.panel_bg };
    let Color::Rgb(r, g, b) = base else {
        // 이름 색 테마(terminal): 섞지 않고 그 색에 대비 글
        return (base, crate::theme::contrast(p));
    };
    let target = if luminance((r, g, b)) < 0.5 { (255, 255, 255) } else { (0, 0, 0) };
    let mix = |a: u8, t: u8| (f32::from(a) + (f32::from(t) - f32::from(a)) * 0.28).round() as u8;
    let bg = (mix(r, target.0), mix(g, target.1), mix(b, target.2));
    let l = luminance(bg);
    let fg = if (l + 0.05) / 0.05 > 1.05 / (l + 0.05) { Color::Rgb(0, 0, 0) } else { Color::Rgb(255, 255, 255) };
    (Color::Rgb(bg.0, bg.1, bg.2), fg)
}

fn luminance((r, g, b): (u8, u8, u8)) -> f32 {
    let ch = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

/// Windows Terminal의 선택 복사(Ctrl+C·Ctrl+Shift+C)와 macOS Cmd+C.
pub(super) fn copy_key(mods: KeyModifiers, ch: Option<char>) -> bool {
    matches!(ch, Some('c' | 'C')) && matches!(mods - KeyModifiers::SHIFT, KeyModifiers::CONTROL | KeyModifiers::SUPER)
}

impl App {
    /// 마우스 사건(herdr `handle_mouse`). 창이 열려 있으면 창이 먼저다.
    pub(crate) async fn on_mouse(&mut self, m: MouseEvent, tx: &mut FrontSender) -> Result<()> {
        if !self.mouse_capture {
            return Ok(());
        }
        if !self.replaying_link && self.link_gesture(m) {
            return Ok(());
        }
        if matches!(m.kind, MouseEventKind::Down(_)) {
            self.reported_selection = None;
            self.app_copy = None;
        }
        let (x, y, mods) = (m.column, m.row, m.modifiers);
        let hits = self.hits.borrow().clone();
        if m.kind == MouseEventKind::Down(MouseButton::Left) && self.link_press(m, &hits, tx).await? {
            return Ok(());
        }
        if self.overlay.is_some() {
            self.overlay_mouse(m, &hits, tx).await?;
        } else {
            match m.kind {
                MouseEventKind::Down(MouseButton::Left) => self.press(x, y, mods, &hits, tx).await?,
                MouseEventKind::Down(MouseButton::Right) => self.press_right(x, y, mods, &hits, tx).await?,
                MouseEventKind::Down(b) => self.press_other(x, y, b, mods, &hits, tx).await?,
                MouseEventKind::Drag(b) => self.drag_to(x, y, b, mods, &hits, tx).await?,
                MouseEventKind::Up(b) => self.release(x, y, b, mods, &hits, tx).await?,
                MouseEventKind::Moved => self.moved(x, y, mods, &hits, tx).await?,
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                    self.wheel(x, y, m.kind == MouseEventKind::ScrollDown, mods, &hits, tx).await?
                }
                _ => {}
            }
        }
        // 탭이 바뀌면 포커스를 그 탭으로(키와 같다)
        let leaves = self.current_leaves();
        if !self.focus.as_ref().is_some_and(|f| leaves.contains(f)) {
            self.focus = leaves.first().cloned();
        }
        self.reconcile_ascii();
        Ok(())
    }

    /// 링크를 묻거나 연 누름의 왼쪽 끌기·놓기(herdr `url_click_consumes_until_up`·`fallback_events`): 연 뒤면 놓을 때까지 버리고,
    /// 묻는 중이면 답이 올 때까지 모은다. 다룬 사건이면 참.
    pub(crate) fn link_gesture(&mut self, m: MouseEvent) -> bool {
        if self.link_consume {
            match m.kind {
                MouseEventKind::Drag(MouseButton::Left) => return true,
                MouseEventKind::Up(MouseButton::Left) => {
                    self.link_consume = false;
                    return true;
                }
                MouseEventKind::Down(MouseButton::Left) => self.link_consume = false,
                _ => {}
            }
        }
        if !matches!(m.kind, MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)) {
            return false;
        }
        let Some(l) = self.link_press.as_mut() else { return false };
        if l.events.iter().any(|e| e.kind == MouseEventKind::Up(MouseButton::Left)) {
            return false;
        }
        l.events.push(m);
        true
    }

    /// ctrl+왼쪽 누름이 pane 안쪽이면 그 칸의 링크를 묻는다(herdr처럼 ctrl만, alt+누름은 늘 보통 누름). 물었으면 참.
    /// herdr처럼 터미널 모드에 창이 없을 때만이고, 묻는 동안 키·다른 사건은 그대로 간다.
    async fn link_press(&mut self, m: MouseEvent, hits: &Hits, tx: &mut FrontSender) -> Result<bool> {
        if self.replaying_link
            || self.link_press.is_some()
            || self.overlay.is_some()
            || self.mode != Mode::Terminal
            || !m.modifiers.contains(KeyModifiers::CONTROL)
            || self.host_minor < LINK_MINOR
        {
            return Ok(false);
        }
        let (x, y) = (m.column, m.row);
        let Some(b) = hits.panes.iter().find(|b| contains(b.inner, x, y)) else { return Ok(false) };
        let top = self.grids.get(&b.pane).map_or(0, |g| view_top(g.scroll));
        let at = TextPoint { row: top + u32::from(y - b.inner.y), col: x - b.inner.x };
        self.last_click = None;
        let id = tx.send(FrontRequest::PaneLink { pane: b.pane.clone(), at }).await?;
        self.link_press = Some(LinkPress { id, pane: b.pane.clone(), inner: b.inner, events: vec![m] });
        Ok(true)
    }

    /// 링크 답(herdr `PendingEndpointKind::PaneLinkActivate` 답): http(s)면 이 기기에서 열고 남은 끌기·놓기를 버린다. 링크가
    /// 아니거나 오류면 모은 누름을 보통 누름으로 다시 넣는다. 그 사이 모드·창·pane 자리가 바뀌었으면 다시 넣지 않고 버린다.
    pub(crate) fn link_done(&mut self, result: aaa_proto::Outcome<aaa_proto::FrontResponse>) {
        let Some(l) = self.link_press.take() else { return };
        let released = l.events.iter().any(|e| e.kind == MouseEventKind::Up(MouseButton::Left));
        let still = self.mouse_capture
            && self.mode == Mode::Terminal
            && self.overlay.is_none()
            && self.hits.borrow().panes.iter().any(|b| b.pane == l.pane && b.inner == l.inner);
        match result {
            aaa_proto::Outcome::Ok(aaa_proto::FrontResponse::Link { url: Some(url), .. })
                if still && open::safe_web_url(&url) =>
            {
                self.link_consume = !released;
                if let Err(e) = open::open_url(&url) {
                    self.set_error(format!("링크를 열지 못했다: {e}"));
                }
            }
            _ if still => self.link_replay = l.events,
            _ => self.link_consume = !released,
        }
    }

    /// herdr: 이름·닫기 확인 창은 단추가 아닌 곳을 누르면(안쪽도) 닫는다. 단축키 창은 밖을 누를 때만 닫고 휠로 3줄씩 민다.
    async fn overlay_mouse(&mut self, m: MouseEvent, hits: &Hits, tx: &mut FrontSender) -> Result<()> {
        let (x, y) = (m.column, m.row);
        // herdr: 메뉴는 움직이면 그 줄을 고르고, 줄을 누르면 그 항목, 밖을 누르면 닫는다
        if let Some(Overlay::Menu(menu)) = self.overlay.as_mut() {
            let row = hits.menu_rows.iter().find(|(r, _)| contains(*r, x, y)).map(|(_, i)| *i);
            match (m.kind, row) {
                (MouseEventKind::Moved, Some(i)) => menu.highlighted = i,
                (MouseEventKind::Down(MouseButton::Left), Some(i)) => {
                    if let Some(Overlay::Menu(menu)) = self.overlay.take() {
                        self.menu_activate(menu, i, tx).await?;
                    }
                }
                (MouseEventKind::Down(MouseButton::Left), None) => self.overlay = None,
                _ => {}
            }
            return Ok(());
        }
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => match self.overlay.as_mut() {
                Some(Overlay::Help(_)) => {
                    if !hits.on(hits.popup, x, y) {
                        self.overlay = None;
                    }
                }
                Some(Overlay::Rename(r)) => match hits.button_at(x, y) {
                    Some(Button::Save) => {
                        if let Some(Overlay::Rename(r)) = self.overlay.take() {
                            self.save_rename(r, tx).await?;
                        }
                    }
                    Some(Button::Clear) => r.input.clear(),
                    _ => self.overlay = None,
                },
                Some(Overlay::Confirm(_)) => match hits.button_at(x, y) {
                    Some(Button::Close) => {
                        if let Some(Overlay::Confirm(c)) = self.overlay.take() {
                            self.confirm(c, tx).await?;
                        }
                    }
                    _ => self.overlay = None,
                },
                // herdr: 구역 이름표는 그 구역, 줄은 고르기(테마는 미리 보기, 상태 표시는 바로 적용), 적용 단추, 닫기 단추·창 밖은
                // 되돌리고 닫기
                Some(Overlay::Settings(s)) => {
                    let Some(sh) = hits.settings.as_ref() else { return Ok(()) };
                    if let Some((_, section)) = sh.tabs.iter().find(|(r, _)| contains(*r, x, y)) {
                        let section = *section;
                        self.settings_section(section);
                    } else if let Some((_, i)) = sh.choices.iter().find(|(r, _)| contains(*r, x, y)) {
                        s.selected = *i;
                        match s.section {
                            crate::settings::Section::Theme => self.settings_preview(),
                            crate::settings::Section::Indicators => self.settings_apply(),
                        }
                    } else if contains(sh.apply, x, y) {
                        self.settings_apply();
                    } else if contains(sh.close, x, y) || !contains(sh.popup, x, y) {
                        self.settings_cancel();
                    }
                }
                // 메뉴는 위에서 다뤘다
                Some(Overlay::Menu(_)) | None => {}
            },
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let area = Rect { x: 0, y: 0, w: self.width, h: self.height };
                let prefixes = self.prefix_label();
                if let Some(Overlay::Help(h)) = self.overlay.as_mut() {
                    let max = help::max_scroll(area, &prefixes, &h.query);
                    h.scroll = if m.kind == MouseEventKind::ScrollDown {
                        (h.scroll + 3).min(max)
                    } else {
                        h.scroll.saturating_sub(3)
                    };
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 마우스 보고를 켠 pane의 안쪽이면 그 pane(herdr `hit.mouse_reporting && inner_rect`)
    fn reporting(&self, hits: &Hits, x: u16, y: u16) -> Option<PaneBox> {
        let b = hits.pane_at(x, y)?;
        let on = self.grids.get(&b.pane).is_some_and(|g| g.modes.mouse != MouseMode::None);
        (on && contains(b.inner, x, y)).then(|| b.clone())
    }

    /// pane 프로그램에 마우스 사건을 넘긴다. 좌표는 안쪽 0부터, 왼쪽·위로만 묶는다(herdr `pane_mouse_position`).
    async fn report_to(
        &mut self,
        tx: &mut FrontSender,
        b: &PaneBox,
        r: Report,
        x: u16,
        y: u16,
        mods: KeyModifiers,
    ) -> Result<()> {
        let Some(modes) = self.grids.get(&b.pane).map(|g| g.modes) else { return Ok(()) };
        match report(modes, r, x.saturating_sub(b.inner.x), y.saturating_sub(b.inner.y), mods) {
            // herdr: 누르지 않은 움직임 보고는 기록 위치를 그대로 둔다
            Some(bytes) if r == Report::Move => self.send_input(tx, b.pane.clone(), bytes).await,
            Some(bytes) => self.input_to(tx, b.pane.clone(), bytes).await,
            None => Ok(()),
        }
    }

    /// 왼쪽 누름(herdr 맞히기 순서: 사이드바 접기 → 경계 → 전체 메뉴 → 새 workspace → 새 탭 → `<` `>` → 탭 → 목록 줄 →
    /// 나눔 경계 → pane)
    async fn press(&mut self, x: u16, y: u16, mods: KeyModifiers, hits: &Hits, tx: &mut FrontSender) -> Result<()> {
        // herdr mouse.rs: 터미널 모드에서 pane이 있는 알림 카드를 누르면 그 pane으로(카드를 거둔다)
        if self.mode == Mode::Terminal
            && hits.on(hits.card, x, y)
            && self.cards.visible().is_some_and(|c| c.pane.is_some())
        {
            return self.open_card(tx).await;
        }
        self.selection = None;
        self.reported_selection = None;
        if self.mode == Mode::Prefix {
            self.mode = Mode::Terminal;
        }
        if hits.on(hits.toggle, x, y) {
            self.toggle_list();
            return Ok(());
        }
        // herdr: 구역 경계를 누르면 그 줄부터 끌어 두 구역 비율을 바꾼다
        if hits.on(hits.section_divider, x, y) {
            self.drag = Some(Drag::SidebarSection);
            self.drag_section_to(y, hits.sidebar);
            return Ok(());
        }
        // herdr: 경계를 두 번 누르면 설정 폭으로, 아니면 누른 열부터 끈다
        if hits.on(hits.divider, x, y) {
            let now = Instant::now();
            if self.divider_click.take().is_some_and(|t| now.duration_since(t) <= DIVIDER_DOUBLE_CLICK) {
                self.reset_sidebar_width();
            } else {
                self.drag = Some(Drag::Sidebar);
                self.drag_sidebar_to(x, hits.sidebar.map_or(0, |s| s.x));
                self.divider_click = Some(now);
            }
            return Ok(());
        }
        if hits.on(hits.header_button, x, y) {
            // 좁은 화면 `목록` 단추(herdr mobile switch): 탐색 모드를 켜고 끈다
            self.mode = if self.mode == Mode::Navigate { Mode::Terminal } else { Mode::Navigate };
            self.list_sel = None;
            return Ok(());
        }
        if let Some(launcher) = hits.global_menu.filter(|r| contains(*r, x, y)) {
            self.overlay = Some(Overlay::Menu(Menu::global(launcher)));
            return Ok(());
        }
        if hits.on(hits.new_workspace, x, y) {
            return self.act(Action::NewWorkspace, tx).await;
        }
        if hits.on(hits.pick_machine, x, y) {
            self.next_machine();
            return Ok(());
        }
        if hits.on(hits.new_tab, x, y) {
            return self.act(Action::NewTab, tx).await;
        }
        if hits.on(hits.scroll_left, x, y) {
            self.tab_scroll = Some((hits.tab_scroll.saturating_sub(1), self.tab));
            return Ok(());
        }
        if hits.on(hits.scroll_right, x, y) {
            self.tab_scroll = Some(((hits.tab_scroll + 1).min(hits.max_tab_scroll), self.tab));
            return Ok(());
        }
        if let Some(index) = hits.tab_at(x, y) {
            self.drag = Some(Drag::Tab { index, from: (x, y), moving: false, insert: None });
            return Ok(());
        }
        if let Some(row) = hits.row_at(x, y) {
            self.drag = Some(Drag::Row(row.clone()));
            return Ok(());
        }
        // 목록 밖을 누르면 탐색을 끝낸다
        if self.mode == Mode::Navigate {
            self.mode = Mode::Terminal;
            self.list_sel = None;
        }
        // herdr: 기록이 있는 pane scrollbar를 누르면 그 pane에 포커스, thumb면 끌기, 아니면 thumb 가운데가 그 줄로 뛴다
        if let Some((pane, m, track)) = hits
            .panes
            .iter()
            .filter(|b| b.gutter.is_some_and(|g| contains(g, x, y)))
            .find_map(|b| self.scroll_track(hits, &b.pane))
        {
            self.focus = Some(pane.clone());
            match scrollbar::grab(m, track, y) {
                Some(grab) => self.drag = Some(Drag::Scrollbar { pane, grab, sent: None, at: None }),
                None => self.scroll_to(tx, pane, scrollbar::offset_from_row(m, track, y)).await?,
            }
            return Ok(());
        }
        // herdr: 나눔 경계를 누르면 끌기(포커스는 그대로)
        if let Some(hit) = hits.splits.iter().find(|s| contains(s.hit, x, y)).cloned() {
            let Some(tab) = self.ws.tabs.get(self.tab).map(|t| t.id.clone()) else { return Ok(()) };
            if self.host_at_least(14) {
                let pointer = if hit.dir == SplitDir::Horizontal { x } else { y };
                self.drag =
                    Some(Drag::Split { grab: i32::from(hit.pos) - i32::from(pointer), hit, tab, sent: None, at: None });
            }
            return Ok(());
        }
        let Some(b) = hits.pane_at(x, y).cloned() else { return Ok(()) };
        self.focus = Some(b.pane.clone());
        if !mods.contains(KeyModifiers::SHIFT) && self.reporting(hits, x, y).is_some() {
            let top = self.grids.get(&b.pane).map_or(0, |g| view_top(g.scroll));
            let at = (top + u32::from(y - b.inner.y), x - b.inner.x);
            self.reported_selection = Some(Selection { pane: b.pane.clone(), anchor: at, head: at, until: None });
            self.report_to(tx, &b, Report::Press(MouseButton::Left), x, y, mods).await?;
            self.drag = Some(Drag::Pane { pane: b.pane, button: MouseButton::Left });
            return Ok(());
        }
        if !contains(b.inner, x, y) {
            return Ok(());
        }
        let cell = (y - b.inner.y, x - b.inner.x);
        let top = self.grids.get(&b.pane).map_or(0, |g| view_top(g.scroll));
        let at = (top + u32::from(cell.0), cell.1);
        let now = Instant::now();
        let double = self.last_click.as_ref().is_some_and(|(t, p, c)| {
            now.duration_since(*t) <= DOUBLE_CLICK && *p == b.pane && c.0 == cell.0 && c.1.abs_diff(cell.1) <= 1
        });
        if double {
            // 두 번 누름: 낱말을 고르고 복사한 뒤 잠깐 보인다(herdr word selection)
            self.last_click = None;
            let word =
                self.grids.get(&b.pane).and_then(|g| g.lines.get(cell.0 as usize)).and_then(|l| word_at(l, cell.1));
            if let Some((a, z)) = word {
                let sel = Selection { pane: b.pane, anchor: (at.0, a), head: (at.0, z), until: Some(now + WORD_SHOWN) };
                self.copy_selection(tx, &sel).await?;
                self.selection = Some(sel);
            }
            return Ok(());
        }
        self.last_click = Some((now, b.pane.clone(), cell));
        self.drag = Some(Drag::Select { pane: b.pane, anchor: at, moved: false, offset: None });
        Ok(())
    }

    /// 오른쪽 누름(herdr): 「오른쪽 누름을 pane에 보내기」를 켠 보고 pane이면 수정키 없는 누름을 넘기고, 아니면 탭 이름표는 탭 메뉴,
    /// pane(테두리 포함)은 pane 메뉴. 사이드바 줄 메뉴(herdr workspace 메뉴)는 없다.
    async fn press_right(
        &mut self,
        x: u16,
        y: u16,
        mods: KeyModifiers,
        hits: &Hits,
        tx: &mut FrontSender,
    ) -> Result<()> {
        if self.drag.is_some() {
            return Ok(());
        }
        if let Some(b) = self.reporting(hits, x, y)
            && self.right_click.contains(&b.pane)
            && mods.is_empty()
        {
            return self.press_other(x, y, MouseButton::Right, mods, hits, tx).await;
        }
        self.selection = None;
        if self.mode != Mode::Terminal {
            self.mode = Mode::Terminal;
            self.list_sel = None;
        }
        let target = if let Some(index) = hits.tab_at(x, y) {
            MenuTarget::Tab { index }
        } else if let Some(b) = hits.pane_at(x, y) {
            let source = self.focus.clone().filter(|f| *f != b.pane);
            MenuTarget::Pane {
                pane: b.pane.clone(),
                source,
                passthrough: self.right_click.contains(&b.pane),
                labeled: self.panes.get(&b.pane).is_some_and(|p| p.label.is_some()),
            }
        } else {
            return Ok(());
        };
        self.overlay = Some(Overlay::Menu(Menu { target, x, y, highlighted: 0 }));
        Ok(())
    }

    /// 메뉴 항목을 한다(herdr `activate_context_menu_item`). 탭 메뉴는 그 탭으로 옮긴 뒤 키와 같은 동작,
    /// pane 메뉴는 그 pane에 포커스를 두고 키와 같은 동작(자리 바꾸기는 포커스였던 pane과, 뒤 포커스는 그 pane).
    pub(crate) async fn menu_activate(&mut self, menu: Menu, index: usize, tx: &mut FrontSender) -> Result<()> {
        let Some((_, action)) = menu.items().get(index).copied() else { return Ok(()) };
        match menu.target {
            MenuTarget::Tab { index } => {
                if index >= self.ws.tabs.len() {
                    return Ok(());
                }
                self.tab = index;
                let leaves = self.current_leaves();
                if !self.focus.as_ref().is_some_and(|f| leaves.contains(f)) {
                    self.focus = leaves.first().cloned();
                }
                let a = match action {
                    MenuAction::NewTab => Action::NewTab,
                    MenuAction::Rename => Action::RenameTab,
                    _ => Action::CloseTab,
                };
                self.act(a, tx).await
            }
            MenuTarget::Pane { pane, source, .. } => match action {
                // herdr: 그 pane의 이름 창(포커스는 그대로), 지우기는 바로 보낸다
                MenuAction::RenamePane => {
                    self.open_rename_pane(pane);
                    Ok(())
                }
                MenuAction::ClearPaneName => self.rename_pane(tx, pane, None).await,
                MenuAction::Swap => {
                    let Some(source) = source else { return Ok(()) };
                    if self.host_at_least(10) {
                        self.layout(tx, LayoutChange::Swap { pane: source.clone(), with: pane }, false).await?;
                        self.focus = Some(source);
                    }
                    Ok(())
                }
                MenuAction::RightClick => {
                    if !self.right_click.remove(&pane) {
                        self.right_click.insert(pane);
                    }
                    Ok(())
                }
                _ => {
                    self.focus = Some(pane);
                    let a = match action {
                        MenuAction::SplitRight => Action::SplitVertical,
                        MenuAction::SplitDown => Action::SplitHorizontal,
                        MenuAction::Zoom => Action::Zoom,
                        _ => Action::ClosePane,
                    };
                    self.act(a, tx).await
                }
            },
            MenuTarget::Global => {
                let a = match action {
                    MenuAction::Settings => Action::Settings,
                    MenuAction::Keybinds => Action::Help,
                    MenuAction::ReloadConfig => Action::ReloadConfig,
                    _ => Action::Detach,
                };
                self.act(a, tx).await
            }
        }
    }

    /// 그 pane의 scrollbar 칸과 기록 위치(기록이 있을 때만)
    fn scroll_track(&self, hits: &Hits, pane: &str) -> Option<(String, scrollbar::Metrics, Rect)> {
        let track = hits.panes.iter().find(|b| b.pane == pane)?.gutter?;
        let g = self.grids.get(pane)?;
        (g.scroll.max > 0).then(|| (pane.to_string(), scrollbar::Metrics::new(g.scroll, g.rows), track))
    }

    /// 이 화면의 그 pane 기록 위치를 맨 아래에서 `offset`줄로(herdr `set_scroll_offset_from_bottom`)
    pub(crate) async fn scroll_to(&mut self, tx: &mut FrontSender, pane: String, offset: usize) -> Result<()> {
        if !self.host_at_least(26) {
            return Ok(());
        }
        // copy 모드가 셈하는 위치도 같이(scrollbar로 옮겨도 다음 키가 그 자리에서 시작한다)
        if let Some(c) = self.copy.as_mut().filter(|c| c.pane == pane) {
            c.offset = offset as u32;
        }
        let by = ScrollBy::Offset { lines: offset as u32 };
        let id = tx.send(FrontRequest::PaneScroll { pane, by }).await?;
        self.pending.insert(id, Pending::Report("기록 넘기기"));
        Ok(())
    }

    /// 가운데·오른쪽 누름: 마우스 보고 pane이면 넘긴다
    async fn press_other(
        &mut self,
        x: u16,
        y: u16,
        button: MouseButton,
        mods: KeyModifiers,
        hits: &Hits,
        tx: &mut FrontSender,
    ) -> Result<()> {
        if self.drag.is_some() {
            return Ok(());
        }
        if let Some(b) = self.reporting(hits, x, y) {
            self.selection = None;
            self.focus = Some(b.pane.clone());
            self.report_to(tx, &b, Report::Press(button), x, y, mods).await?;
            self.drag = Some(Drag::Pane { pane: b.pane, button });
        }
        Ok(())
    }

    async fn drag_to(
        &mut self,
        x: u16,
        y: u16,
        button: MouseButton,
        mods: KeyModifiers,
        hits: &Hits,
        tx: &mut FrontSender,
    ) -> Result<()> {
        match self.drag.clone() {
            Some(Drag::Pane { pane, button: held }) if held == button => {
                if let Some(b) = hits.panes.iter().find(|b| b.pane == pane).cloned() {
                    if held == MouseButton::Left
                        && let Some(sel) = self.reported_selection.as_mut()
                        && b.inner.w > 0
                        && b.inner.h > 0
                    {
                        let row = y.clamp(b.inner.y, b.inner.y + b.inner.h - 1) - b.inner.y;
                        let col = x.clamp(b.inner.x, b.inner.x + b.inner.w - 1) - b.inner.x;
                        let top = self.grids.get(&pane).map_or(0, |g| view_top(g.scroll));
                        sel.head = (top + u32::from(row), col);
                    }
                    self.report_to(tx, &b, Report::Drag(held), x, y, mods).await?;
                }
            }
            _ if button != MouseButton::Left => {}
            // herdr: 폭 = 열 + 1, `[tui] sidebar_min_width`~`sidebar_max_width`
            Some(Drag::Sidebar) => self.drag_sidebar_to(x, hits.sidebar.map_or(0, |s| s.x)),
            Some(Drag::SidebarSection) => self.drag_section_to(y, hits.sidebar),
            // herdr: 33ms마다 바뀐 위치만 보낸다
            Some(Drag::Scrollbar { pane, grab, sent, at }) => {
                let Some((pane, m, track)) = self.scroll_track(hits, &pane) else { return Ok(()) };
                let offset = scrollbar::offset_from_drag_row(m, track, y, grab);
                let now = Instant::now();
                if sent != Some(offset) && at.is_none_or(|t| now.duration_since(t) >= SPLIT_SEND) {
                    self.drag = Some(Drag::Scrollbar { pane: pane.clone(), grab, sent: Some(offset), at: Some(now) });
                    self.scroll_to(tx, pane, offset).await?;
                }
            }
            Some(Drag::Split { hit, tab, grab, sent, at }) => {
                let ratio = split_ratio(&hit, grab, x, y);
                let now = Instant::now();
                if sent != Some(ratio) && at.is_none_or(|t| now.duration_since(t) >= SPLIT_SEND) {
                    let path = hit.path.clone();
                    self.drag = Some(Drag::Split { hit, tab: tab.clone(), grab, sent: Some(ratio), at: Some(now) });
                    self.layout(tx, LayoutChange::SplitRatio { tab, path, ratio }, false).await?;
                }
            }
            // herdr: 누른 칸에서 한 칸 넘게 끌면 옮기기, 그 뒤로는 줄 밖이면 넣을 자리가 없다
            Some(Drag::Tab { index, from, moving, .. }) => {
                let insert = drop_index(hits, x, y, self.ws.tabs.len());
                let moving = moving || (x.abs_diff(from.0).max(y.abs_diff(from.1)) >= 1 && insert.is_some());
                self.drag = Some(Drag::Tab { index, from, moving, insert: if moving { insert } else { None } });
            }
            // 누른 칸을 벗어나야 고른다. pane 밖으로 끌면 가장자리 칸으로 묶는다. herdr: pane 위·아래 밖이면 그 거리만큼
            // 바로 넘기고(3~15줄), 가장자리 줄을 포함해 그쪽에 있는 동안 30ms마다 한 줄씩 더 넘긴다.
            Some(Drag::Select { pane, anchor, moved, offset }) => {
                let Some(i) = hits.panes.iter().find(|b| b.pane == pane).map(|b| b.inner) else { return Ok(()) };
                if i.w == 0 || i.h == 0 {
                    return Ok(());
                }
                let bottom = i.y + i.h - 1;
                let (up, jump) = match y {
                    y if y < i.y => (Some(true), edge_lines(i.y - y)),
                    y if y > bottom => (Some(false), -edge_lines(y - bottom)),
                    y if y == i.y => (Some(true), 0),
                    y if y == bottom => (Some(false), 0),
                    _ => (None, 0),
                };
                self.autoscroll = up.map(|up| AutoScroll {
                    pane: pane.clone(),
                    up,
                    x,
                    y,
                    inner: i,
                    at: Instant::now() + SELECTION_AUTOSCROLL,
                });
                self.select_to(tx, pane, anchor, moved, offset, jump, (x, y), i).await?;
            }
            _ => {}
        }
        Ok(())
    }

    async fn release(
        &mut self,
        x: u16,
        y: u16,
        button: MouseButton,
        mods: KeyModifiers,
        hits: &Hits,
        tx: &mut FrontSender,
    ) -> Result<()> {
        // 누른 단추를 뗄 때만 끝난다
        let held = match &self.drag {
            Some(Drag::Pane { button, .. }) => *button,
            _ => MouseButton::Left,
        };
        if button != held {
            return Ok(());
        }
        self.autoscroll = None;
        match self.drag.take() {
            Some(Drag::Pane { pane, button }) => {
                if let Some(b) = hits.panes.iter().find(|b| b.pane == pane).cloned() {
                    self.report_to(tx, &b, Report::Release(button), x, y, mods).await?;
                }
            }
            // 끄는 동안 마지막으로 보낸 값과 다르면 뗀 자리 값을 보낸다
            Some(Drag::Scrollbar { pane, grab, sent, .. }) => {
                if let Some((pane, m, track)) = self.scroll_track(hits, &pane) {
                    let offset = scrollbar::offset_from_drag_row(m, track, y, grab);
                    if sent != Some(offset) {
                        self.scroll_to(tx, pane, offset).await?;
                    }
                }
            }
            Some(Drag::Split { hit, tab, grab, sent, .. }) => {
                let ratio = split_ratio(&hit, grab, x, y);
                if sent != Some(ratio) {
                    self.layout(tx, LayoutChange::SplitRatio { tab, path: hit.path, ratio }, false).await?;
                }
            }
            // herdr `TabMove`: 보던 탭은 `set_ws`가 id로 따라간다
            Some(Drag::Tab { index, moving: true, .. }) => {
                let count = self.ws.tabs.len();
                if let Some(insert) = drop_index(hits, x, y, count)
                    && let Some(tab) = self.ws.tabs.get(index).map(|t| t.id.clone())
                    && self.host_at_least(14)
                {
                    self.layout(tx, LayoutChange::MoveTab { tab, index: insert as u16 }, false).await?;
                }
            }
            // herdr: 사이드바 폭 끌기를 마치면 남긴다
            Some(Drag::Sidebar) => self.sidebar_drag_done(),
            Some(Drag::SidebarSection) => self.section_drag_done(),
            // herdr: 누른 이름표에서 떼면 그 탭(`TabFocus`)
            Some(Drag::Tab { index, .. }) if hits.tab_at(x, y) == Some(index) => self.tab = index,
            // 누른 줄에서 떼면 연다(탐색 중이면 탐색도 끝낸다)
            Some(Drag::Row(row)) if hits.row_at(x, y) == Some(&row) => {
                if self.mode == Mode::Navigate {
                    self.mode = Mode::Terminal;
                    self.list_sel = None;
                }
                self.open_row(tx, row).await?;
            }
            Some(Drag::Select { pane, moved: true, .. }) => {
                self.last_click = None;
                if let Some(sel) = self.selection.clone().filter(|s| s.pane == pane)
                    && self.copy_on_select
                {
                    self.copy_selection(tx, &sel).await?;
                    // 고른 글은 다음 키·누름까지 남긴다. 자동 복사 직후 Ctrl+C도 복사 키다.
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// 누르지 않은 움직임: 1003을 켠 pane에만(herdr `MouseEventKind::Moved`)
    async fn moved(&mut self, x: u16, y: u16, mods: KeyModifiers, hits: &Hits, tx: &mut FrontSender) -> Result<()> {
        if self.drag.is_none()
            && self.selection.is_none()
            && !mods.contains(KeyModifiers::SHIFT)
            && let Some(b) = self.reporting(hits, x, y)
        {
            self.report_to(tx, &b, Report::Move, x, y, mods).await?;
        }
        Ok(())
    }

    /// 휠: 탭 막대는 앞·뒤 탭, 펼친 목록은 한 줄씩. pane 안쪽은 포커스를 옮기고 마우스 보고면 넘기고, 보고를 끈 alt 화면이면
    /// 화살표 하나, 아니면 이 화면의 기록을 넘긴다(herdr `apply_scroll`). herdr처럼 scrollbar 칸 위 휠은 아무것도 하지 않는다.
    async fn wheel(
        &mut self,
        x: u16,
        y: u16,
        down: bool,
        mods: KeyModifiers,
        hits: &Hits,
        tx: &mut FrontSender,
    ) -> Result<()> {
        // herdr `scroll_in_progress_selection`: 끌어 고르는 중 휠은 그 pane 기록을 넘기며 고른 끝을 다시 잡는다(프로그램에는 안 감)
        if let Some(Drag::Select { pane, anchor, moved, offset }) = self.drag.clone() {
            if let Some(i) = hits.panes.iter().find(|b| b.pane == pane).map(|b| b.inner)
                && i.w > 0
                && i.h > 0
            {
                let lines = i64::from(self.scroll_lines);
                self.select_to(tx, pane, anchor, moved, offset, if down { -lines } else { lines }, (x, y), i).await?;
            }
            return Ok(());
        }
        if hits.on(hits.tab_bar, x, y) {
            return self.act(if down { Action::NextTab } else { Action::PreviousTab }, tx).await;
        }
        if hits.on(hits.sidebar, x, y) {
            if hits.list_h > 0 {
                let max = self.list_rows().len().saturating_sub(hits.list_h as usize);
                let top = if down { hits.list_skip + 1 } else { hits.list_skip.saturating_sub(1) };
                self.list_top = Some(top.min(max));
            }
            return Ok(());
        }
        let Some(b) = hits.panes.iter().find(|b| contains(b.inner, x, y)).cloned() else { return Ok(()) };
        if self.focus.as_deref() != Some(b.pane.as_str()) {
            self.focus = Some(b.pane.clone());
        }
        let Some(modes) = self.grids.get(&b.pane).map(|g| g.modes) else { return Ok(()) };
        if modes.mouse != MouseMode::None {
            self.report_to(tx, &b, Report::Wheel(down), x, y, mods).await?;
        } else if modes.alt_screen {
            let k = KeyEvent::new(if down { KeyCode::Down } else { KeyCode::Up }, KeyModifiers::NONE);
            self.input_to(tx, b.pane, keys::encode_in(&k, modes.app_cursor)).await?;
        } else if let Some((offset, max)) = self
            .copy
            .as_ref()
            .filter(|c| c.pane == b.pane)
            .and_then(|c| Some((c.offset, self.grids.get(&b.pane)?.scroll.max)))
        {
            // copy 모드 pane은 copy 모드가 셈하는 위치에서 넘긴다(다음 키가 그 자리에서 시작한다)
            let lines = u32::from(self.scroll_lines);
            let next = if down { offset.saturating_sub(lines) } else { (offset + lines).min(max) };
            self.scroll_to(tx, b.pane, next as usize).await?;
        } else if self.host_at_least(26) {
            // herdr `apply_scroll`: 보통 화면은 이 화면의 기록을 `mouse_scroll_lines`줄씩 넘긴다(프로그램에는 아무것도 안 감)
            let lines = u32::from(self.scroll_lines);
            let by = if down { ScrollBy::Down { lines } } else { ScrollBy::Up { lines } };
            let id = tx.send(FrontRequest::PaneScroll { pane: b.pane, by }).await?;
            self.pending.insert(id, Pending::Report("기록 넘기기"));
        }
        Ok(())
    }

    /// 고르는 중 기록 위치를 `delta`줄(+면 위로) 옮기고 화면 자리 `at`까지 고른다(herdr `drag_selection_with_metrics`).
    /// 기록 위치가 바뀌었으면 true.
    #[allow(clippy::too_many_arguments)]
    async fn select_to(
        &mut self,
        tx: &mut FrontSender,
        pane: String,
        anchor: Point,
        moved: bool,
        offset: Option<u32>,
        delta: i64,
        (x, y): (u16, u16),
        i: Rect,
    ) -> Result<bool> {
        let Some(scroll) = self.grids.get(&pane).map(|g| g.scroll) else { return Ok(false) };
        let now = offset.unwrap_or(scroll.offset).min(scroll.max);
        let next = (i64::from(now) + delta).clamp(0, i64::from(scroll.max)) as u32;
        let changed = next != now;
        if changed {
            self.scroll_to(tx, pane.clone(), next as usize).await?;
        }
        let cell = (y.clamp(i.y, i.y + i.h - 1) - i.y, x.clamp(i.x, i.x + i.w - 1) - i.x);
        let head = (scroll.max - next + u32::from(cell.0), cell.1);
        let moved = moved || head != anchor;
        let offset = if changed { Some(next) } else { offset };
        self.drag = Some(Drag::Select { pane: pane.clone(), anchor, moved, offset });
        if moved {
            self.selection = Some(Selection { pane, anchor, head, until: None });
        }
        Ok(changed)
    }

    /// 가장자리 자동 넘기기 한 번(herdr `tick_selection_autoscroll`): 한 줄 넘기고 마지막 마우스 자리까지 고른다.
    /// 더 넘길 수 없거나, 끌기가 끝났거나, pane 칸이 바뀌면 멈춘다.
    pub(crate) async fn autoscroll_tick(&mut self, tx: &mut FrontSender) -> Result<()> {
        let Some(a) = self.autoscroll.take() else { return Ok(()) };
        let Some(Drag::Select { pane, anchor, moved, offset }) = self.drag.clone() else { return Ok(()) };
        let inner = self.hits.borrow().panes.iter().find(|b| b.pane == pane).map(|b| b.inner);
        if pane != a.pane || inner != Some(a.inner) {
            return Ok(());
        }
        if self.select_to(tx, pane, anchor, moved, offset, if a.up { 1 } else { -1 }, (a.x, a.y), a.inner).await? {
            self.autoscroll = Some(AutoScroll { at: Instant::now() + SELECTION_AUTOSCROLL, ..a });
        }
        Ok(())
    }

    /// 고른 글을 복사한다(herdr `PaneSelectionRead`). 새 host(1.27부터)는 화면 사본에서 기록까지 읽어 응답이 오면 복사하고,
    /// 옛 host면 이 화면에 보이는 줄만 바로 복사한다.
    pub(crate) async fn copy_selection(&mut self, tx: &mut FrontSender, sel: &Selection) -> Result<()> {
        let ((r1, c1), (r2, c2)) = sel.range();
        if self.host_at_least(27) {
            let (from, to) = (TextPoint { row: r1, col: c1 }, TextPoint { row: r2, col: c2 });
            let id = tx.send(FrontRequest::PaneSelectionRead { pane: sel.pane.clone(), from, to }).await?;
            self.pending.insert(id, Pending::Copy);
            return Ok(());
        }
        let Some(g) = self.grids.get(&sel.pane) else { return Ok(()) };
        let (top, rows) = (view_top(g.scroll), u32::from(g.rows));
        if r2 < top || r1 >= top + rows || rows == 0 {
            return Ok(());
        }
        let from = if r1 < top { (0, 0) } else { ((r1 - top) as u16, c1) };
        let to = if r2 >= top + rows { ((rows - 1) as u16, u16::MAX) } else { ((r2 - top) as u16, c2) };
        let t = text(&g.lines, (from, to));
        self.copied(&t, false);
        Ok(())
    }

    /// 복사하고 알린다(herdr 「copied to clipboard」)
    pub(crate) fn copied(&mut self, t: &str, truncated: bool) {
        if t.trim().is_empty() {
            return;
        }
        let result = clip::copy_text(t);
        if let Some(pane) = self.focus.clone() {
            let outcome = match &result {
                Ok(clip::CopyResult::Copied) => aaa_proto::ClipboardOutcome::Copied,
                Ok(clip::CopyResult::Requested) => aaa_proto::ClipboardOutcome::Requested,
                Err(_) => aaa_proto::ClipboardOutcome::Failed,
            };
            self.copy_outcomes.push((pane, outcome));
        }
        match result {
            Ok(clip::CopyResult::Copied) if truncated => self.set_message("클립보드에 복사했다(너무 길어 앞쪽만)"),
            Ok(clip::CopyResult::Copied) => self.set_message("클립보드에 복사했다"),
            Ok(clip::CopyResult::Requested) if truncated => {
                self.set_message("터미널에 복사 요청을 보냈다(너무 길어 앞쪽만)")
            }
            Ok(clip::CopyResult::Requested) => self.set_message("터미널에 복사 요청을 보냈다"),
            Err(e) => self.set_error(format!("복사하지 못했다: {e}")),
        }
    }

    /// 고른 채 둔 글(`copy_on_select` 끔)은 ctrl+c·cmd+c로 복사하고, 다른 키는 지우고 그대로 처리한다(herdr)
    pub(crate) async fn selection_key(
        &mut self,
        tx: &mut FrontSender,
        mods: KeyModifiers,
        ch: Option<char>,
    ) -> Result<bool> {
        let app_copy = self.app_copy.take().filter(|(pane, _)| self.focus.as_deref() == Some(pane.as_str()));
        if copy_key(mods, ch)
            && let Some((_, text)) = app_copy
        {
            self.reported_selection = None;
            self.selection = None;
            self.drag = None;
            self.autoscroll = None;
            self.copied(&text, false);
            return Ok(true);
        }
        let reported = self
            .reported_selection
            .take()
            .filter(|s| s.anchor != s.head && self.focus.as_deref() == Some(s.pane.as_str()));
        let Some(sel) = self.selection.take().or(reported) else { return Ok(false) };
        let copy = copy_key(mods, ch);
        if copy {
            self.drag = None;
            self.autoscroll = None;
            self.copy_selection(tx, &sel).await?;
        }
        Ok(copy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(s: &str) -> Vec<GridRun> {
        vec![GridRun { t: s.into(), fg: 0, bg: 0, a: 0 }]
    }

    /// 넓은 글자는 두 칸, 걸치면 넣고, 줄 끝 빈칸은 뺀다
    #[test]
    fn selection_text_by_columns() {
        let lines = vec![runs("ab 한글 cd   "), runs("둘째 줄"), runs("셋째")];
        assert_eq!(text(&lines, ((0, 3), (0, 6))), "한글");
        assert_eq!(text(&lines, ((0, 4), (0, 5))), "한글");
        assert_eq!(text(&lines, ((0, 8), (1, 1))), "cd\n둘");
        assert_eq!(text(&lines, ((0, 0), (2, 100))), "ab 한글 cd\n둘째 줄\n셋째");
        let s = Selection { pane: "p".into(), anchor: (1, 3), head: (0, 5), until: None };
        assert_eq!(s.range(), ((0, 5), (1, 3)));
        assert!(s.covers(0, 0, 9) && s.covers(0, 1, 0) && !s.covers(0, 1, 4) && !s.covers(0, 0, 4));
        // 기록 줄 번호로 고르므로 화면 맨 위 줄(top)이 바뀌면 고른 칸도 따라 움직인다
        let s = Selection { pane: "p".into(), anchor: (12, 2), head: (11, 0), until: None };
        assert!(
            s.covers(10, 1, 7)
                && s.covers(10, 2, 2)
                && !s.covers(10, 2, 3)
                && s.covers(11, 0, 0)
                && !s.covers(12, 1, 0)
        );
        assert_eq!(view_top(GridScroll { offset: 5, max: 30 }), 25);
        // herdr `selection_edge_scroll_lines`: 밖으로 나간 줄 × 3, 3~15
        assert_eq!([1, 2, 5, 9].map(edge_lines), [3, 6, 15, 15]);
    }

    /// 두 번 누름 낱말: 구분자로 가르고 URL·경로는 한 낱말(herdr `word_bounds_at_column`)
    #[test]
    fn word_like_herdr() {
        let r = runs("see https://example.com/a?b=1 (src/main.rs) 한글낱말");
        assert_eq!(word_at(&r, 0), Some((0, 2)));
        assert_eq!(word_at(&r, 3), None);
        assert_eq!(word_at(&r, 10), Some((4, 28)));
        assert_eq!(word_at(&r, 30), None);
        assert_eq!(word_at(&r, 36), Some((31, 41)));
        assert_eq!(word_at(&r, 45), Some((44, 51)));
        assert_eq!(text(std::slice::from_ref(&r), ((0, 31), (0, 41))), "src/main.rs");
    }

    /// 선택 색: catppuccin `panel_bg`(어두움)를 흰색 쪽으로 28%, 글은 대비가 큰 검정
    #[test]
    fn selection_colors_like_herdr() {
        let p = Palette::catppuccin();
        let (bg, fg) = selection_colors(&p);
        let Color::Rgb(r, g, b) = p.panel_bg else { panic!() };
        let mix = |a: u8| (f32::from(a) + (255.0 - f32::from(a)) * 0.28).round() as u8;
        assert_eq!(bg, Color::Rgb(mix(r), mix(g), mix(b)));
        assert!(matches!(fg, Color::Rgb(0, 0, 0) | Color::Rgb(255, 255, 255)));
    }

    /// xterm 마우스 보고: 모드가 받는 사건만, 인코딩마다 모양이 다르다
    #[test]
    fn reports_like_xterm() {
        let none = KeyModifiers::NONE;
        let modes = |mouse, mouse_encoding| PaneModes { mouse, mouse_encoding, ..PaneModes::default() };
        let sgr = modes(MouseMode::ButtonMotion, MouseEncoding::Sgr);
        let left = MouseButton::Left;
        assert_eq!(report(sgr, Report::Press(left), 0, 0, none).unwrap(), b"\x1b[<0;1;1M");
        assert_eq!(report(sgr, Report::Release(left), 0, 0, none).unwrap(), b"\x1b[<0;1;1m");
        assert_eq!(report(sgr, Report::Drag(left), 2, 1, none).unwrap(), b"\x1b[<32;3;2M");
        assert_eq!(report(sgr, Report::Wheel(true), 4, 5, KeyModifiers::CONTROL).unwrap(), b"\x1b[<81;5;6M");
        assert_eq!(report(sgr, Report::Press(MouseButton::Right), 0, 0, KeyModifiers::SHIFT).unwrap(), b"\x1b[<6;1;1M");
        // 움직임은 1003만
        assert_eq!(report(sgr, Report::Move, 0, 0, none), None);
        let any = modes(MouseMode::AnyMotion, MouseEncoding::Sgr);
        assert_eq!(report(any, Report::Move, 0, 0, none).unwrap(), b"\x1b[<35;1;1M");
        // 1000은 누른 채 움직임을 보내지 않고, 9는 뗌도 수정키도 없다
        let x10 = modes(MouseMode::PressRelease, MouseEncoding::Default);
        assert_eq!(report(x10, Report::Drag(left), 0, 0, none), None);
        assert_eq!(report(x10, Report::Press(MouseButton::Right), 9, 4, none).unwrap(), b"\x1b[M\"*%");
        assert_eq!(report(x10, Report::Release(MouseButton::Right), 9, 4, none).unwrap(), b"\x1b[M#*%");
        let press = modes(MouseMode::Press, MouseEncoding::Default);
        assert_eq!(report(press, Report::Release(left), 0, 0, none), None);
        assert_eq!(report(press, Report::Press(left), 0, 0, KeyModifiers::SHIFT).unwrap(), b"\x1b[M !!");
        // X10 바이트는 223칸까지, 1005는 UTF-8 글자로
        assert_eq!(report(x10, Report::Press(left), 222, 0, none).unwrap(), [0x1b, b'[', b'M', 32, 255, 33]);
        assert_eq!(report(x10, Report::Press(left), 223, 0, none), None);
        let utf8 = modes(MouseMode::PressRelease, MouseEncoding::Utf8);
        assert_eq!(report(utf8, Report::Press(left), 300, 0, none).unwrap(), b"\x1b[M \xc5\x8d!");
        assert_eq!(report(PaneModes::default(), Report::Press(left), 0, 0, none), None);
    }

    /// herdr `pane_split_ratio`: 누른 칸과 경계의 차이를 지키고 0.1~0.9로 묶는다. 천분율 반올림은 `split_rect`가 그 칸으로 되돌린다.
    #[test]
    fn split_ratio_like_herdr() {
        let hit = SplitHit {
            path: vec![],
            dir: SplitDir::Horizontal,
            area: Rect { x: 26, y: 1, w: 64, h: 29 },
            pos: 58,
            hit: Rect { x: 57, y: 1, w: 2, h: 29 },
        };
        let grab = 58 - 57;
        assert_eq!(split_ratio(&hit, grab, 57, 5), 500);
        let r = split_ratio(&hit, grab, 63, 5);
        assert_eq!(r, 594);
        assert_eq!((64 * u32::from(r) + 500) / 1000, 38);
        assert_eq!(split_ratio(&hit, grab, 26, 5), 100);
        assert_eq!(split_ratio(&hit, grab, 200, 5), 900);
    }

    /// herdr `tab_drop_index_at`·`tab_drop_indicator_x`: 이름표 가운데 기준, 표시는 그 이름표 앞 빈칸
    #[test]
    fn tab_drop_like_herdr() {
        let hits = Hits {
            tabs: [26, 35, 44].iter().enumerate().map(|(i, x)| (Rect { x: *x, y: 0, w: 8, h: 1 }, i)).collect(),
            ..Hits::default()
        };
        let at = |x| drop_index(&hits, x, 0, 3);
        assert_eq!(
            [at(20), at(29), at(30), at(38), at(39), at(47), at(48), at(51), at(60)],
            [Some(0), Some(0), Some(1), Some(1), Some(2), Some(2), Some(3), Some(3), Some(3)]
        );
        assert_eq!(drop_index(&hits, 30, 1, 3), None);
        assert_eq!([0, 1, 2, 3].map(|i| drop_x(&hits, i, 3)), [Some(26), Some(34), Some(43), Some(52)]);
    }

    /// herdr `ClientContextMenuOverlay::items`·`render_context_menu`: 폭은 가장 긴 항목 + 4(14 이상), 누른 자리에서 화면 안으로,
    /// 고른 줄은 accent 바탕, 위·아래는 끝에서 멈춤
    #[test]
    fn menu_like_herdr() {
        // 전체 메뉴(herdr `global_menu_items`·`render_global_menu`): `menu` 단추 오른쪽 끝에 맞춰 그 위에, 폭은 가장 긴 항목 + 4
        let global = Menu::global(Rect { x: 19, y: 14, w: 6, h: 1 });
        assert_eq!(
            global.items().iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            ["설정", "단축키", "설정 다시 읽기", "나가기"]
        );
        assert_eq!((global.x, global.y), (25 - 18, 14 - 6));
        let tab = Menu { target: MenuTarget::Tab { index: 1 }, x: 70, y: 0, highlighted: 0 };
        assert_eq!(tab.items().iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["새 탭", "이름 바꾸기", "닫기"]);
        let mut pane = Menu {
            target: MenuTarget::Pane { pane: "p".into(), source: Some("q".into()), passthrough: false, labeled: true },
            x: 5,
            y: 28,
            highlighted: 0,
        };
        assert_eq!(
            pane.items().iter().take(3).map(|i| i.1).collect::<Vec<_>>(),
            [MenuAction::RenamePane, MenuAction::ClearPaneName, MenuAction::Swap]
        );
        assert_eq!(pane.items().len(), 8);
        pane.step(false);
        assert_eq!(pane.highlighted, 0);
        for _ in 0..9 {
            pane.step(true);
        }
        assert_eq!(pane.highlighted, 7);
        let solo = Menu {
            target: MenuTarget::Pane { pane: "p".into(), source: None, passthrough: true, labeled: false },
            ..pane.clone()
        };
        assert_eq!(
            solo.items().iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            [
                "pane 이름 바꾸기",
                "오른쪽으로 나누기",
                "아래로 나누기",
                "크게 보기",
                "오른쪽 누름은 aaa 메뉴로",
                "pane 닫기"
            ]
        );

        let p = Palette::catppuccin();
        let area = Rect { x: 0, y: 0, w: 80, h: 30 };
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 80, 30));
        // 「이름 바꾸기」 11칸 + 4 = 15, 오른쪽 끝에서 안으로
        let (r, rows) = render_menu(&mut buf, area, &tab, &p);
        assert_eq!(r, Rect { x: 65, y: 0, w: 15, h: 5 });
        assert_eq!(rows.iter().map(|(r, i)| (r.x, r.y, *i)).collect::<Vec<_>>(), [(66, 1, 0), (66, 2, 1), (66, 3, 2)]);
        assert_eq!((buf[(66, 1)].bg, buf[(66, 2)].bg), (p.accent, p.panel_bg));
        assert_eq!(buf[(65, 0)].symbol(), "┌");
        // 「오른쪽 누름을 pane에 보내기」 27칸 + 4, 높이 항목 8 + 2, 아래 끝에서 위로
        let (r, _) = render_menu(&mut buf, area, &pane, &p);
        assert_eq!(r, Rect { x: 5, y: 20, w: 31, h: 10 });
    }
}
