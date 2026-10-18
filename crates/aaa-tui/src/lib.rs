//! TUI front(`aaa attach`). 상태를 갖지 않는 얇은 client다(§4.5).
//!
//! - 같은 머신이면 host Front socket으로, 다른 기기면 host 웹 주소의 WebSocket으로 같은 Front API를 쓴다
//!   (`aaa attach --remote`, plan §6.2). 키 설정은 이 기기 것이다(herdr `--remote-keybindings` 기본 `local`).
//!
//! - host가 준 workspace layout을 칸으로 나누고, 각 pane은 host가 보내는 cell grid를 그린다.
//! - 가진 것은 지금 탭·포커스·prefix 입력 상태뿐이다. lease·pane 상태는 host가 보낸 목록을 그대로 쓴다.
//! - 키 입력은 포커스 pane에 대화형 입력(`PaneInput`)으로 보낸다. 다른 front가 쓰던 pane이어도 들어가고, 이 화면이
//!   「지금 쓰는 화면」이 되어 PTY 크기를 다시 건다(herdr처럼 명시적 가져오기가 필요 없다, §8.33).
//! - 터미널 focus를 얻으면(FocusGained) 보이는 pane의 크기를 다시 건다. focus가 없을 때는 크기를 걸지 않는다.
//! - Ctrl+V(또는 빈 붙여넣기)에서 이 기기 clipboard에 그림이 있으면 pane 머신으로 올린다(`clip`, §8.28).
//!   그림이 없으면 Ctrl+V는 원래대로 0x16을 보낸다.
//! - 바깥 터미널의 bracketed paste를 켜서 붙여넣기를 한 덩이로 받는다. 글은 host가 pane 프로그램 모드대로 감싸고
//!   (`PanePaste`), 그림 파일 하나의 경로면(터미널에 끌어 놓은 파일) 그 그림을 올린다(`clip::dropped_image`, §8.147).
//! - 키는 herdr 0.9.5 모드를 따른다(`src/client/shell/input.rs` `route_key`): Terminal(pane에 바로) → prefix 키로
//!   Prefix(명령 키 하나, 시간 제한 없음, Esc·모르는 키는 취소) → `w`로 Navigate(목록 고르기·pane 포커스).
//! - 탭은 herdr처럼: `c` 새 탭은 이름을 먼저 묻고(`prompt`), 이름표는 이름 또는 자리 번호(`tabs`), `T` 이름, `X` 닫기.

pub mod bind;
mod cards;
mod chrome;
pub mod clip;
mod copy;
mod editor;
mod help;
pub mod input_source;
pub mod keys;
pub mod list;
mod mouse;
mod prompt;
mod scrollbar;
mod settings;
mod tabs;
mod theme;

use std::collections::{HashMap, HashSet};
use std::process::ExitCode;
use std::time::Duration;

use bind::Action;
use keys::Combo;

use aaa_proto::{
    B64, FrontEvent, FrontMsg, FrontRequest, GridFrame, GridRun, GridScroll, LayoutChange, MachineView, Outcome,
    PaneDirection, PaneModes, PaneView, ScrollBy, SessionState, SplitDir, ThreadView, Workspace,
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
/// 알림 카드가 보이는 시간
const MESSAGE_SHOWN: Duration = Duration::from_secs(5);
/// 목록을 저절로 다시 그리는 간격(「n분 조용」)
const MINUTE: Duration = Duration::from_secs(60);
/// 접힌 compact 폭(herdr 4). 펼친 폭의 한도는 `[tui] sidebar_min_width`·`sidebar_max_width`다
const COMPACT_WIDTH: u16 = 4;
/// 사이드바 경계 두 번 누르기 간격(herdr `mouse.rs` 경계 두 번 누르기 350ms)
const DIVIDER_DOUBLE_CLICK: Duration = Duration::from_millis(350);
/// host 공유 설정(`preferences`·`preferences_set`)을 아는 Front API minor. 그 요청은 1.5 판 중에 더해졌다(모르는 요청은 연결을
/// 끊으므로 그 뒤 판에만 보낸다)
const PREFERENCES_MINOR: u16 = 6;
/// 좁은 화면 머리의 오른쪽 단추 폭(herdr `MOBILE_BUTTON_WIDTH`)
const MOBILE_BUTTON_WIDTH: u16 = 10;
/// 탭 막대 오른쪽 조각을 두려면 이름표 자리가 이만큼 남아야 한다(herdr `MIN_TAB_STRIP_WIDTH` = 8 + 3 + 3×2)
const MIN_TAB_STRIP_WIDTH: u16 = tabs::MIN_TAB_WIDTH + 3 + 6;

/// 화면 배치(herdr `ClientShellLayout`). 쓰지 않는 칸은 폭이나 높이가 0이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Shell {
    sidebar: Rect,
    tab_bar: Rect,
    /// 좁은 화면 두 줄 머리(herdr `mobile_header`)
    header: Rect,
    panes: Rect,
}

/// host.toml `[tui]`(읽지 못하면 기본값)
fn tui_config() -> aaa_config::TuiSection {
    aaa_config::load_host(&aaa_config::paths::config_dir()).map(|l| l.config.tui).unwrap_or_default()
}

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?)
}

/// 이 화면의 front 이름(lease·기기 목록에 보인다)
fn label() -> String {
    format!("tui@{}", aaa_common::util::hostname())
}

pub fn run(workspace: Option<String>) -> Result<ExitCode> {
    let rt = runtime()?;
    let state = aaa_config::paths::state_dir();
    let client = rt.block_on(FrontClient::connect_as(&state, &label()))?;
    attach(&rt, workspace, client)
}

/// 다른 기기의 host 웹 주소로 붙는다(plan §6.2). 저장한 장치 token이 없거나 host가 받지 않으면 `ask_code`(까닭을 받는다)로
/// host 사람이 낸 pairing 코드를 받아 pairing하고 token을 이 기기 상태 폴더에 둔다. 새로 받은 token도 거부되면 멈춘다.
pub fn run_remote(
    workspace: Option<String>,
    target: &aaa_common::remote::Target,
    ask_code: impl Fn(&str) -> Result<String>,
) -> Result<ExitCode> {
    use aaa_common::remote;
    let rt = runtime()?;
    let file = target.token_file(&aaa_config::paths::state_dir());
    let mut fresh = false;
    let client = loop {
        let why = match remote::load_token(&file) {
            Some(token) => match rt.block_on(FrontClient::connect_remote(target, &token, &label())) {
                Ok(c) => break c,
                Err(e) if e.downcast_ref::<remote::Unauthorized>().is_some() => {
                    remote::forget_token(&file)?;
                    if fresh {
                        anyhow::bail!("{}: pairing으로 받은 token도 host가 받지 않았다", target.origin());
                    }
                    "저장한 기기 token을 host가 받지 않는다(기기를 취소했거나 다른 host다)"
                }
                Err(e) => return Err(e),
            },
            None => "이 기기는 아직 그 host와 pairing하지 않았다",
        };
        let code = ask_code(why)?;
        let name = format!("aaa TUI · {}", aaa_common::util::hostname());
        let token = rt.block_on(remote::pair(target, &code, &name))?;
        remote::save_token(&file, &token)?;
        fresh = true;
    };
    attach(&rt, workspace, client)
}

fn attach(rt: &tokio::runtime::Runtime, workspace: Option<String>, client: FrontClient) -> Result<ExitCode> {
    let cfg = tui_config();
    rt.block_on(async move {
        let host_minor = client.proto.minor;
        let reader = client.role == aaa_proto::DeviceRole::Reader;
        let mut terminal = ratatui::init();
        // 터미널 focus 사건(FocusGained·FocusLost): 돌아온 화면이 크기를 다시 건다. 비정상 종료(panic)에도 끈다.
        let _focus = FocusReport::on();
        // 붙여넣기를 한 덩이로 받는다(herdr client `EnableBracketedPaste`): 끌어 놓은 파일 경로와 여러 줄 글을 낱 키와 가른다
        let _paste = BracketedPaste::on();
        // 마우스 보고는 App이 설정대로 켠다. 끝날 때(panic도) 끈다.
        let _mouse = MouseOff;
        let mut app =
            App::new(workspace.unwrap_or_else(|| aaa_proto::DEFAULT_WORKSPACE.to_string()), client.front.clone());
        app.host_minor = host_minor;
        app.reader = reader;
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

/// 붙여 넣은 글을 보내는 요청. host가 그 프로그램이 켠 bracketed paste대로 감싸고 ESC를 뺀다(Front API 1.23 `pane_paste`,
/// herdr 서버가 client 붙여넣기를 다시 감싸는 것과 같다). 그 전 host에는 전처럼 친 글(`pane_input`)이다: 모르는 요청은 연결을
/// 끊으므로 지금 도는 host를 끊지 않는다.
fn paste_request(host_minor: u16, pane: String, text: String) -> FrontRequest {
    if host_minor < 23 {
        FrontRequest::PaneInput { pane, data: B64(text.into_bytes()) }
    } else {
        FrontRequest::PanePaste { pane, text }
    }
}

/// 바깥 터미널의 bracketed paste(`CSI ? 2004 h`)를 켜고, 이 값이 사라질 때(정상·오류·panic 종료) 끈다.
/// Windows 콘솔은 crossterm이 붙여넣기를 사건으로 주지 않는다(표지가 낱 키로 와 pane에 들어간다): 켜지 않는다.
struct BracketedPaste;

impl BracketedPaste {
    fn on() -> BracketedPaste {
        #[cfg(not(windows))]
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::EnableBracketedPaste);
        BracketedPaste
    }
}

impl Drop for BracketedPaste {
    fn drop(&mut self) {
        #[cfg(not(windows))]
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableBracketedPaste);
    }
}

/// 사라질 때 마우스 보고를 끈다
struct MouseOff;

impl Drop for MouseOff {
    fn drop(&mut self) {
        let _ = crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture);
    }
}

#[derive(Default)]
struct Grid {
    cols: u16,
    rows: u16,
    lines: Vec<Vec<GridRun>>,
    cursor: (u16, u16, bool),
    /// pane 프로그램이 켠 모드(마우스 보고·alt 화면·DECCKM)
    modes: PaneModes,
    /// 이 화면이 보는 기록 위치(휠 기록 넘기기)
    scroll: GridScroll,
}

impl Grid {
    /// `full`이면 모든 줄을 바꾼다. 아니면 온 줄만 바꾸고 나머지는 둔다: 크기가 바뀌었어도 보내지 않은 줄은 host가 보기에
    /// 그대로다. 크기가 바뀐 grid를 비우면 폭만 바뀐 pane이 비었다(QA t-0337 Q23). 지금 host는 크기가 바뀌면 전체를 보내고,
    /// 고치기 전 host는 폭만 바뀌면 바뀐 줄만 보낸다.
    fn apply(&mut self, f: GridFrame) {
        if f.full {
            self.lines.clear();
        }
        self.lines.resize(f.rows as usize, Vec::new());
        self.cols = f.cols;
        self.rows = f.rows;
        for l in f.lines {
            if let Some(slot) = self.lines.get_mut(l.row as usize) {
                *slot = l.runs;
            }
        }
        self.cursor = (f.cursor_row, f.cursor_col, f.cursor_visible);
        self.modes = f.modes;
        self.scroll = f.scroll;
    }
}

enum Pending {
    /// 응답이 오면 오류만 보인다
    Report(&'static str),
    /// 입력·크기 오류는 상태 줄에 보인다
    Input,
    /// 새로 만든 pane에 포커스를 옮긴다(보낼 때 있던 pane들: 같은 layout 사건이 응답보다 먼저 와도 새 pane을 찾는다)
    Focus(HashSet<String>),
    /// 기록 편집기 pane을 열었다(`Focus`처럼 새 pane으로, 그 pane이 닫히면 `from`으로 돌아간다)
    EditScrollback { before: HashSet<String>, from: String },
    /// 새 workspace를 만들었다: 응답이 오면 그 workspace로 가서 새 pane에 포커스
    NewSpace,
    /// 그림 붙여넣기 조각(몇 번째 붙여넣기인지)
    Attach(u64),
    /// 고른 글 읽기: 응답이 오면 복사한다
    Copy,
    /// copy 모드 낱말·문단 이동: 응답이 오면 커서를 그 자리로
    CopyMotion,
    /// copy 모드 찾기(찾은 글·방향·`n`·`N` 다시인지): 응답이 오면 현재 일치로
    CopySearch { query: String, direction: aaa_proto::SearchDirection, repeat: bool },
    /// spaces 구역 workspace 목록: 응답이 오면 이름을 넣고 지금 것이 아닌 workspace의 layout을 읽는다
    Spaces,
    /// 다른 workspace의 layout(상태 점에 쓴다): 응답이 와도 지금 workspace로 바꾸지 않는다
    SpaceLayout,
    /// host 공유 설정: 기억한 사이드바 배치를 쓴다
    Chrome,
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
    /// 크기 모드(`prefix r`, herdr `route_resize_key`): h/j/k/l·화살표로 포커스 pane 가장자리를 옮긴다. Esc·Enter·r 끝
    Resize,
    /// copy 모드(`prefix [`, herdr `route_copy_mode_key`): 키로 기록을 넘기며 고르고 복사한다(`copy.rs`)
    Copy,
}

/// 화면 위에 뜨는 창
enum Overlay {
    Help(Help),
    /// 이름 입력(새 탭·탭 이름)
    Rename(prompt::Rename),
    /// 닫기 확인(마지막 탭·마지막 pane)
    Confirm(prompt::Confirm),
    /// 오른쪽 누름 메뉴(herdr `ContextMenu`)와 전체 메뉴(herdr `GlobalMenu`)
    Menu(mouse::Menu),
    /// 설정 창(herdr `Settings`)
    Settings(settings::Settings),
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
    /// 왼쪽 목록을 접었나(설정을 읽기 전에는 None, `[tui] sidebar_start_collapsed`로 시작)
    sidebar_collapsed: Option<bool>,
    /// 펼친 목록 폭(구분선 포함, `[tui] sidebar_width`, 끌어 바꾸면 그 값). 그릴 때 `sidebar_bounds`로 묶는다
    sidebar_width: u16,
    /// 사용자가 끌어 바꾼 폭인가(herdr `sidebar_width_manual`): 그러면 설정을 다시 읽어도(테마 바꾸기 포함) 폭을 두고 host에 남긴다
    sidebar_width_manual: bool,
    /// 사용자가 펴거나 접었나(herdr `sidebar_collapsed_manual`): host에 남긴다
    sidebar_collapsed_manual: bool,
    /// `[tui] sidebar_width`(경계 두 번 누르기로 돌아갈 값)와 폭 한도(`[tui] sidebar_min_width`·`sidebar_max_width`)
    config_sidebar_width: u16,
    sidebar_bounds: (u16, u16),
    /// host 공유 설정에 아직 보내지 않은 사이드바 배치(바뀐 이름만, null은 설정값으로 되돌림)
    chrome_unsaved: std::collections::BTreeMap<String, serde_json::Value>,
    /// 사이드바 경계를 마지막으로 누른 때(두 번 누르기)
    divider_click: Option<tokio::time::Instant>,
    collapsed_mode: aaa_config::SidebarCollapsedMode,
    /// 이 폭 이하면 두 줄 머리(`[tui] mobile_width_threshold`)
    mobile_width: u16,
    /// 색(`[tui] theme`, herdr 팔레트)
    palette: theme::Palette,
    /// pane 테두리(`[tui] pane_borders`)
    pane_borders: aaa_config::PaneBorders,
    /// 탐색 모드에서 고른 줄(줄이 늘거나 줄어도 같은 것을 가리킨다)
    list_sel: Option<list::Row>,
    /// spaces 구역 workspace들(herdr `ClientShellWorkspace`, layout을 아직 읽지 않았으면 탭 없음)과 읽을 이름
    spaces: HashMap<String, Workspace>,
    space_fetch: Vec<String>,
    /// 마지막으로 그린 터미널 크기
    width: u16,
    height: u16,
    grids: HashMap<String, Grid>,
    watching: HashSet<String>,
    tab: usize,
    focus: Option<String>,
    mode: Mode,
    overlay: Option<Overlay>,
    /// 알림 글(오른쪽 위 카드, MESSAGE_SHOWN 뒤 사라진다)
    message: Option<(String, tokio::time::Instant)>,
    /// 오류(ERROR 막대, ERROR_SHOWN 뒤 사라진다)
    error: Option<(String, tokio::time::Instant)>,
    /// agent 알림 카드(herdr 알림, 오른쪽 아래, cards.rs)
    cards: cards::Cards,
    /// 카드를 낸 질문(같은 질문의 다음 사건은 새 소식이 아니다)
    asked: HashSet<String>,
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
    /// 새 탭 이름을 먼저 묻는다(`[tui] prompt_new_tab_name`)
    prompt_new_tab: bool,
    /// 마지막 탭·pane을 닫기 전에 묻는다(`[tui] confirm_close`)
    confirm_close: bool,
    /// host와 합의한 Front API minor(옛 host는 탭 닫기·이름을 모른다)
    host_minor: u16,
    /// 보기 권한 연결(보기 기기·thread 세션 안): pane 크기를 걸지 않고 입력은 보내지 않은 채 알린다
    reader: bool,
    /// 영문 입력기를 원하는 상태인가(herdr `ascii_input_source_active`)
    ascii_on: bool,
    /// 영문으로 바꿔 둔 입력기(없애면 되돌린다: 모드 끝·종료·panic)
    ascii: Option<input_source::Ascii>,
    /// 새 pane으로 포커스를 옮길 layout 요청(응답 전까지 키·붙여넣기를 `held`에 붙잡는다)
    focus_wait: Option<String>,
    held: Vec<Event>,
    /// 연 기록 편집기 pane과 연 pane(herdr `OverlayPaneState.previous_focus`): 편집기 pane이 닫히면 연 pane으로 돌아간다
    overlay_back: Option<(String, String)>,
    /// 머신 머리 줄로 고른 머신과 그때 포커스 pane(herdr 활성 endpoint): 포커스가 바뀌면 풀린다(`target_machine`)
    machine_pick: Option<(String, Option<String>)>,
    /// copy 모드 이동 요청(응답 전까지 키를 `held`에 붙잡아 다음 키가 옮긴 자리에서 시작한다)과 받은 자리(pane, 커서)
    copy_wait: Option<String>,
    copy_reply: Option<copy::CopyReply>,
    quit: bool,
    /// 마우스(`[tui] mouse_capture`)와 터미널에 걸어 둔 값(다시 걸어야 하면 None)
    mouse_capture: bool,
    capture_on: Option<bool>,
    /// 끌어 고른 글을 떼는 때 복사(`[tui] copy_on_select`)
    copy_on_select: bool,
    /// 휠 한 칸에 넘기는 기록 줄 수(`[tui] mouse_scroll_lines`)
    scroll_lines: u16,
    /// 보통 화면 pane 오른쪽 1칸 scrollbar(`[tui] pane_scrollbars`)
    pane_scrollbars: bool,
    /// 상태 점 모양(`[tui] status_indicators`)
    indicators: aaa_config::StatusIndicators,
    /// 지금 테마 이름(`[tui] theme`, 설정 창이 미리 보는 동안은 그 테마)
    theme_name: String,
    /// 마지막으로 그린 자리(마우스가 맞힌다)
    hits: std::cell::RefCell<mouse::Hits>,
    drag: Option<mouse::Drag>,
    /// 고른 글(pane 화면 칸)
    selection: Option<mouse::Selection>,
    /// 끌어 고르다 가장자리에 닿아 기록을 넘기는 중
    autoscroll: Option<mouse::AutoScroll>,
    /// copy 모드(pane에 붙어 있다)
    copy: Option<copy::CopyMode>,
    /// 마지막 누름(두 번 누름을 본다): 시각, pane, 칸
    last_click: Option<(tokio::time::Instant, String, mouse::Cell)>,
    /// 사용자가 넘긴 탭 막대 자리와 그때의 탭(탭이 바뀌면 다시 지금 탭을 가운데로)
    tab_scroll: Option<(usize, usize)>,
    /// 휠로 민 목록 첫 줄(키로 고르면 다시 고른 줄을 따른다)
    list_top: Option<usize>,
    /// 오른쪽 누름을 받는 pane(메뉴 「오른쪽 누름을 pane에 보내기」, herdr `right_click_passthrough`). 이 화면에만 둔다.
    right_click: HashSet<String>,
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
            sidebar_collapsed: None,
            sidebar_width: aaa_config::SIDEBAR_WIDTH,
            sidebar_width_manual: false,
            sidebar_collapsed_manual: false,
            config_sidebar_width: aaa_config::SIDEBAR_WIDTH,
            sidebar_bounds: aaa_config::SIDEBAR_BOUNDS,
            chrome_unsaved: Default::default(),
            divider_click: None,
            collapsed_mode: aaa_config::SidebarCollapsedMode::Compact,
            mobile_width: 64,
            palette: theme::Palette::catppuccin(),
            pane_borders: aaa_config::PaneBorders::Auto,
            list_sel: None,
            spaces: HashMap::new(),
            space_fetch: vec![],
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
            cards: cards::Cards::default(),
            asked: HashSet::new(),
            pending: HashMap::new(),
            sized: HashMap::new(),
            attach_seq: 0,
            attach_failed: None,
            focused: true,
            reclaim: false,
            switch_ascii: false,
            jamo_keys: false,
            prompt_new_tab: true,
            confirm_close: true,
            host_minor: aaa_proto::FRONT_API.minor,
            reader: false,
            ascii_on: false,
            ascii: None,
            focus_wait: None,
            overlay_back: None,
            machine_pick: None,
            held: vec![],
            copy_wait: None,
            copy_reply: None,
            quit: false,
            mouse_capture: true,
            capture_on: None,
            copy_on_select: true,
            scroll_lines: 3,
            pane_scrollbars: true,
            indicators: aaa_config::StatusIndicators::Dots,
            theme_name: "catppuccin".into(),
            hits: Default::default(),
            drag: None,
            selection: None,
            autoscroll: None,
            copy: None,
            last_click: None,
            tab_scroll: None,
            list_top: None,
            right_click: HashSet::new(),
        }
    }

    /// `[tui]` 설정을 적용한다(시작·`prefix shift+r`). 읽지 못한 prefix 값은 빼고 오류로 알린다(herdr처럼).
    fn apply_config(&mut self, cfg: &aaa_config::TuiSection) {
        let (prefixes, bad) = keys::parse_prefixes(&cfg.prefix.values());
        self.prefixes = prefixes;
        self.switch_ascii = cfg.switch_ascii();
        self.jamo_keys = cfg.prefix_jamo_keys;
        self.prompt_new_tab = cfg.prompt_new_tab_name;
        self.confirm_close = cfg.confirm_close;
        // herdr `reload_client_config`: 폭은 사용자가 끌어 바꾸지 않았을 때만 설정을 따르고, 펴고 접은 상태는 처음에만 설정에서
        // 정한다. 테마를 바꾸는 설정 창도 이 길로 다시 읽으므로 사용자가 키운 폭이 남는다
        self.config_sidebar_width = cfg.sidebar_width;
        if !self.sidebar_width_manual {
            self.sidebar_width = cfg.sidebar_width;
        }
        let bounds = cfg.sidebar_bounds();
        self.sidebar_bounds = bounds.unwrap_or(aaa_config::SIDEBAR_BOUNDS);
        self.collapsed_mode = cfg.sidebar_collapsed_mode;
        self.mobile_width = cfg.mobile_width_threshold;
        self.pane_borders = cfg.pane_borders;
        self.mouse_capture = cfg.mouse_capture;
        self.copy_on_select = cfg.copy_on_select;
        self.scroll_lines = cfg.mouse_scroll_lines.max(1);
        self.pane_scrollbars = cfg.pane_scrollbars;
        self.indicators = cfg.status_indicators;
        self.theme_name = theme::canonical_theme_name(&cfg.theme).unwrap_or("catppuccin").to_string();
        self.sidebar_collapsed.get_or_insert(cfg.sidebar_start_collapsed);
        // herdr처럼 모르는 테마 이름은 알리고 기본(catppuccin)을 쓴다
        self.palette = theme::Palette::from_name(&cfg.theme).unwrap_or_else(theme::Palette::catppuccin);
        if theme::Palette::from_name(&cfg.theme).is_none() {
            self.set_error(format!(
                "[tui] theme 「{}」을 모른다: {} 중 하나",
                cfg.theme,
                theme::THEME_NAMES.join(", ")
            ));
        }
        if !bad.is_empty() {
            self.set_error(format!("[tui] prefix 값을 읽지 못했다: {} (예: \"ctrl+b\")", bad.join(", ")));
        }
        if bounds.is_none() {
            let (min, max) = aaa_config::SIDEBAR_BOUNDS;
            self.set_error(format!(
                "[tui] sidebar_min_width({})가 sidebar_max_width({})보다 크다: {min}~{max}를 쓴다",
                cfg.sidebar_min_width, cfg.sidebar_max_width
            ));
        }
        self.reconcile_ascii();
    }

    /// 마우스 보고를 설정대로 건다(herdr `set_mouse_capture`: 크기가 바뀌면 다시 건다)
    fn sync_capture(&mut self) {
        if self.capture_on == Some(self.mouse_capture) {
            return;
        }
        let r = if self.mouse_capture {
            crossterm::execute!(std::io::stdout(), crossterm::event::EnableMouseCapture)
        } else {
            crossterm::execute!(std::io::stdout(), crossterm::event::DisableMouseCapture)
        };
        if r.is_ok() {
            self.capture_on = Some(self.mouse_capture);
        }
        if !self.mouse_capture {
            self.drag = None;
            self.selection = None;
        }
    }

    fn set_error(&mut self, e: impl Into<String>) {
        self.error = Some((e.into(), tokio::time::Instant::now() + ERROR_SHOWN));
    }

    fn set_message(&mut self, m: impl Into<String>) {
        self.message = Some((m.into(), tokio::time::Instant::now() + MESSAGE_SHOWN));
    }

    fn is_prefix(&self, c: Combo) -> bool {
        self.prefixes.contains(&c)
    }

    /// 도움말·안내에 보이는 prefix 이름(여럿이면 ` / `로)
    fn prefix_label(&self) -> String {
        self.prefixes.iter().map(|c| keys::label(*c)).collect::<Vec<_>>().join(" / ")
    }

    /// herdr `wants_ascii_input`: 명령 키를 받는 모드나 창이 열린 동안. 글을 쓰는 창(이름 입력)은 빼서 한글 이름을 쓸 수 있다.
    fn wants_ascii(&self) -> bool {
        match &self.overlay {
            Some(Overlay::Rename(_)) => false,
            Some(_) => true,
            // copy 모드 찾기 입력 줄도 글을 쓰는 칸이다(herdr)
            None if self.mode == Mode::Copy && self.copy.as_ref().is_some_and(|c| c.prompt.is_some()) => false,
            None => self.mode != Mode::Terminal,
        }
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
        let id = tx.send(FrontRequest::Workspaces).await?;
        self.pending.insert(id, Pending::Spaces);
        let id = tx.send(FrontRequest::Threads { project: None, role: None }).await?;
        self.pending.insert(id, Pending::Report("thread 목록"));
        if self.host_minor >= PREFERENCES_MINOR {
            let id = tx.send(FrontRequest::Preferences).await?;
            self.pending.insert(id, Pending::Chrome);
        }
        let mut keys_in = EventStream::new();
        let mut dirty = true;
        // 목록 thread 둘째 줄 「n분 조용」이 화면에서 늘어나게 1분마다 다시 그린다(웹 `useMinute`)
        let mut minute = tokio::time::interval_at(tokio::time::Instant::now() + MINUTE, MINUTE);
        minute.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if dirty {
                self.save_chrome(&mut tx).await?;
                self.sync_capture();
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
                    None => anyhow::bail!("{}", tx.ended().unwrap_or_else(|| "host 연결이 끊겼다".into())),
                },
                e = keys_in.next() => match e {
                    Some(Ok(e @ (Event::Key(_) | Event::Paste(_) | Event::Mouse(_)))) if self.waiting() => self.held.push(e),
                    Some(Ok(Event::Key(k))) if k.kind != KeyEventKind::Release => { self.on_key(k, &mut tx).await?; dirty = true; }
                    // 그림만 든 clipboard를 붙이면 빈 붙여넣기로 오는 터미널이 있다
                    Some(Ok(Event::Paste(s))) => { self.on_paste(s, &mut tx).await?; dirty = true; }
                    Some(Ok(Event::Mouse(m))) => { self.on_mouse(m, &mut tx).await?; dirty = true; }
                    Some(Ok(Event::Resize(..))) => { self.capture_on = None; dirty = true; }
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
                // 두 번 눌러 고른 낱말은 잠깐만 보인다
                _ = tokio::time::sleep_until(self.selection.as_ref().and_then(|s| s.until).unwrap_or_else(tokio::time::Instant::now)), if self.selection.as_ref().is_some_and(|s| s.until.is_some()) => {
                    self.selection = None;
                    dirty = true;
                }
                // 알림 카드도(herdr 알림처럼)
                _ = tokio::time::sleep_until(self.message.as_ref().map_or_else(tokio::time::Instant::now, |m| m.1)), if self.message.is_some() => {
                    self.message = None;
                    dirty = true;
                }
                // agent 알림 카드: 보이는 카드가 사라지거나 받은 알림을 다시 볼 때(아래 tick_cards)
                _ = tokio::time::sleep_until(self.cards.next_deadline().unwrap_or_else(tokio::time::Instant::now)), if self.cards.next_deadline().is_some() => {}
                _ = minute.tick() => dirty = true,
                // 끌어 고르다 가장자리에 닿아 있으면 30ms마다 한 줄씩(herdr selection autoscroll)
                _ = tokio::time::sleep_until(self.autoscroll.as_ref().map_or_else(tokio::time::Instant::now, |a| a.at)), if self.autoscroll.is_some() => {
                    self.autoscroll_tick(&mut tx).await?;
                    dirty = true;
                }
            }
            // 쌓인 메시지를 마저 처리해 한 번만 그린다
            while let Ok(m) = rx.try_recv() {
                self.on_msg(m);
                dirty = true;
            }
            if self.tick_cards() {
                dirty = true;
            }
            // copy 모드 pane에 포커스가 오가거나 그 pane이 없어지면 모드를 맞춘다
            self.settle_copy();
            // 고른 머신은 포커스가 그대로인 동안만이다(다른 pane을 거쳐 돌아와도 다시 살지 않는다)
            if self.machine_pick.as_ref().is_some_and(|(_, at)| *at != self.focus) {
                self.machine_pick = None;
            }
            self.copy_apply(&mut tx).await?;
            for name in std::mem::take(&mut self.space_fetch) {
                let id = tx.send(FrontRequest::Workspace { name: Some(name) }).await?;
                self.pending.insert(id, Pending::SpaceLayout);
            }
            self.reconcile_ascii();
            // 포커스를 옮기거나 copy 커서가 온 뒤 붙잡아 둔 키를 차례대로(그 사이 새 요청이 다시 붙잡으면 남은 것은 그대로 둔다)
            while !self.waiting() && !self.held.is_empty() {
                match self.held.remove(0) {
                    Event::Key(k) if k.kind != KeyEventKind::Release => self.on_key(k, &mut tx).await?,
                    Event::Paste(s) => self.on_paste(s, &mut tx).await?,
                    Event::Mouse(m) => self.on_mouse(m, &mut tx).await?,
                    _ => {}
                }
                dirty = true;
            }
        }
    }

    /// 붙여넣기: 이름 창이 열려 있으면 그 칸으로(herdr `insert_overlay_text`), 다른 창이면 버린다.
    /// 터미널에 끌어 놓은 그림 파일(그 경로 하나)은 그림으로 올린다(herdr client `read_image_file_from_terminal_drop`).
    /// 그림만 든 clipboard는 빈 붙여넣기로 오는 터미널이 있다. 그 밖은 글이다.
    async fn on_paste(&mut self, s: String, tx: &mut FrontSender) -> Result<()> {
        // herdr `handle_raw_events`·`insert_overlay_text`: 이름 창과 검색 중인 단축키 창은 그 입력에 넣는다. 다른 창이 떴거나
        // Terminal 모드가 아니면(prefix·탐색·크기) 버린다. pane과 그림(`clipboard_image_target`)은 그 밖에서만이다.
        match self.overlay.as_mut() {
            Some(Overlay::Rename(r)) => {
                r.input.insert(&s);
            }
            Some(Overlay::Help(h)) if h.search => {
                let text = editor::single_line(&s);
                if !text.is_empty() {
                    h.query.push_str(&text);
                    h.scroll = 0;
                }
            }
            Some(_) => {}
            // copy 모드에서는 pane에 보내지 않는다(찾기 입력 줄이 열려 있으면 그 줄로, herdr)
            None if self.mode == Mode::Copy => {
                self.prompt_paste(&s);
            }
            None if self.mode != Mode::Terminal => {}
            None if s.is_empty() => {
                self.paste_image(tx).await?;
            }
            None => {
                if let Some(path) = clip::dropped_image(&s)
                    && let Some(bytes) =
                        tokio::task::spawn_blocking(move || clip::read_dropped_image(&path)).await.ok().flatten()
                    && self.send_image(tx, bytes).await?
                {
                    return Ok(());
                }
                self.paste_text(tx, s).await?;
            }
        }
        Ok(())
    }

    /// 붙여 넣은 글을 포커스 pane에(`paste_request`)
    async fn paste_text(&mut self, tx: &mut FrontSender, text: String) -> Result<()> {
        let Some(pane) = self.focus.clone() else { return Ok(()) };
        if text.is_empty() {
            return Ok(());
        }
        if self.refuse_reader() {
            return Ok(());
        }
        // 기록을 보고 있었으면 먼저 맨 아래로(친 글과 같다, herdr `scroll_reset`)
        self.scroll_bottom(tx, &pane).await?;
        self.message = None;
        let id = tx.send(paste_request(self.host_minor, pane, text)).await?;
        self.pending.insert(id, Pending::Input);
        Ok(())
    }

    fn on_msg(&mut self, m: FrontMsg) {
        match m {
            FrontMsg::Event(FrontEvent::Workspace(w)) => {
                self.note_space(&w);
                if w.name == self.ws_name {
                    self.set_ws(w);
                }
            }
            FrontMsg::Event(FrontEvent::State { machines, panes }) => {
                self.machines = machines;
                self.pane_order = panes.iter().map(|p| p.id.clone()).collect();
                self.panes = panes.into_iter().map(|p| (p.id.clone(), p)).collect();
            }
            FrontMsg::Event(FrontEvent::Thread(t)) => {
                self.threads.insert(list::thread_key(&t), t);
            }
            // 알림 카드(plan §8.185): 끝남·새 보고는 host가 알리고, 입력 필요는 새로 열린 질문이다(herdr `NeedsAttention`)
            FrontMsg::Event(FrontEvent::Notice(n)) => {
                let card = self.notice_card(&n);
                self.cards.receive(card, tokio::time::Instant::now());
            }
            FrontMsg::Event(FrontEvent::Question(q)) => {
                if q.state == aaa_proto::QuestionState::Open && self.asked.insert(q.id.clone()) {
                    let card = self.question_card(&q);
                    self.cards.receive(card, tokio::time::Instant::now());
                }
            }
            FrontMsg::Event(FrontEvent::Grid(f)) => {
                let pane = f.pane.clone();
                let g = self.grids.entry(pane.clone()).or_default();
                let (before, alt, size) = (g.scroll, g.modes.alt_screen, (g.cols, g.rows));
                g.apply(f);
                let alt_changed = alt != g.modes.alt_screen;
                let resized = size != (g.cols, g.rows);
                self.follow_grid(&pane, before, alt_changed, resized);
            }
            FrontMsg::Res { id, result } => {
                if self.focus_wait.as_ref() == Some(&id) {
                    self.focus_wait = None;
                }
                if self.copy_wait.as_ref() == Some(&id) {
                    self.copy_wait = None;
                }
                let why = self.pending.remove(&id);
                match (result, why) {
                    (Outcome::Ok(aaa_proto::FrontResponse::Workspace(w)), Some(Pending::Focus(before))) => {
                        // 새 pane은 마지막 탭(새 탭) 또는 나눈 자리의 새 leaf다
                        self.set_ws(w);
                        if let Some(new) = self.all_panes().difference(&before).next().cloned() {
                            self.select_pane(&new);
                        }
                    }
                    (
                        Outcome::Ok(aaa_proto::FrontResponse::Workspace(w)),
                        Some(Pending::EditScrollback { before, from }),
                    ) => {
                        self.set_ws(w);
                        if let Some(new) = self.all_panes().difference(&before).next().cloned() {
                            self.select_pane(&new);
                            self.overlay_back = Some((new, from));
                            self.set_message("기록을 편집기로 열었다");
                        }
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Workspace(w)), Some(Pending::NewSpace)) => {
                        self.note_space(&w);
                        self.enter_space(w);
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Workspace(w)), Some(Pending::SpaceLayout)) => {
                        self.note_space(&w)
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Workspace(w)), _) => {
                        self.note_space(&w);
                        // 응답 전에 다른 workspace로 바꿨으면 그 응답은 상태 점에만 쓴다
                        if w.name == self.ws_name {
                            self.set_ws(w);
                        }
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Preferences(doc)), Some(Pending::Chrome)) => {
                        self.apply_chrome(&doc)
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Workspaces { workspaces }), Some(Pending::Spaces)) => {
                        for w in workspaces {
                            if !self.spaces.contains_key(&w.name) {
                                self.spaces.insert(w.name.clone(), Workspace { name: w.name.clone(), tabs: vec![] });
                                if w.name != self.ws_name && w.panes > 0 {
                                    self.space_fetch.push(w.name);
                                }
                            }
                        }
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Threads { threads }), _) => {
                        self.threads = threads.into_iter().map(|t| (list::thread_key(&t), t)).collect();
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Attached { path, inserted, note }), _) => {
                        self.set_message(match (inserted, note) {
                            (true, _) => format!("그림 경로를 넣었다(Enter 없음): {path}"),
                            (false, Some(n)) => format!("그림을 저장했다: {path} — {n}"),
                            (false, None) => format!("그림을 저장했다: {path}"),
                        })
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::Selection { text, truncated, .. }), Some(Pending::Copy)) => {
                        self.copied(&text, truncated)
                    }
                    (Outcome::Ok(aaa_proto::FrontResponse::CopyCursor { pane, cursor }), Some(Pending::CopyMotion)) => {
                        self.copy_reply = Some(copy::CopyReply::Cursor { pane, cursor: (cursor.row, cursor.col) })
                    }
                    (
                        Outcome::Ok(aaa_proto::FrontResponse::CopySearch {
                            pane,
                            matches,
                            total,
                            current,
                            current_global,
                        }),
                        Some(Pending::CopySearch { query, direction, repeat }),
                    ) => {
                        let found = copy::Found { matches, total, current, current_global };
                        self.copy_reply = Some(copy::CopyReply::Search { pane, query, direction, repeat, found })
                    }
                    (Outcome::Ok(_), _) => {}
                    (Outcome::Err(e), Some(Pending::CopySearch { .. })) => {
                        self.set_error(format!("찾기 실패: {}", e.message))
                    }
                    (Outcome::Err(e), Some(Pending::CopyMotion)) => {
                        self.set_error(format!("copy 이동 실패: {}", e.message))
                    }
                    (Outcome::Err(e), Some(Pending::Copy)) => self.set_error(format!("복사하지 못했다: {}", e.message)),
                    (Outcome::Err(e), Some(Pending::EditScrollback { .. })) => {
                        self.set_error(format!("기록을 편집기로 열지 못했다: {}", e.message))
                    }
                    (Outcome::Err(e), Some(Pending::NewSpace)) => {
                        self.set_error(format!("새 workspace를 만들지 못했다: {}", e.message))
                    }
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

    /// 응답을 기다리며 키·붙여넣기·마우스를 붙잡는 중인가(새 pane 포커스·copy 이동)
    fn waiting(&self) -> bool {
        self.focus_wait.is_some() || self.copy_wait.is_some()
    }

    fn all_panes(&self) -> HashSet<String> {
        self.ws.tabs.iter().flat_map(|t| layout::leaves(&t.root)).collect()
    }

    /// workspace 사건·응답으로 spaces 구역을 맞춘다. host는 닫거나 이름을 바꾼 workspace를 탭 없는 사건으로 알린다
    /// (기본 workspace와 지금 workspace는 남긴다).
    fn note_space(&mut self, w: &Workspace) {
        if w.tabs.is_empty() && w.name != aaa_proto::DEFAULT_WORKSPACE && w.name != self.ws_name {
            self.spaces.remove(&w.name);
            return;
        }
        self.spaces.insert(w.name.clone(), w.clone());
    }

    /// spaces 구역 차례(host `workspace_summaries`처럼 기본 workspace 먼저, 그다음 이름 차례)와 상태 점
    fn space_list(&self) -> Vec<list::Space> {
        let mut names: Vec<&String> = self.spaces.keys().collect();
        if !self.spaces.contains_key(&self.ws_name) {
            names.push(&self.ws_name);
        }
        names.sort_by(|a, b| {
            (a.as_str() != aaa_proto::DEFAULT_WORKSPACE)
                .cmp(&(b.as_str() != aaa_proto::DEFAULT_WORKSPACE))
                .then(a.cmp(b))
        });
        names
            .into_iter()
            .map(|n| {
                let w = if *n == self.ws_name { Some(&self.ws) } else { self.spaces.get(n) };
                let panes: Vec<String> =
                    w.into_iter().flat_map(|w| w.tabs.iter().flat_map(|t| layout::leaves(&t.root))).collect();
                list::Space { name: n.clone(), dot: list::space_dot(panes.iter().filter_map(|id| self.panes.get(id))) }
            })
            .collect()
    }

    /// 탐색 모드가 도는 줄: spaces 구역 workspace들 다음 agents 목록(herdr는 ↑↓가 workspace만 돈다, §8.73)
    fn nav_rows(&self) -> Vec<list::Row> {
        let mut rows: Vec<list::Row> = self.space_list().into_iter().map(|s| list::Row::Space(s.name)).collect();
        rows.extend(self.list_rows());
        rows
    }

    /// 다른 workspace로(herdr `switch_workspace`): 아는 layout으로 바로 그리고 host에 다시 읽는다. 포커스는 front마다라
    /// 그 workspace의 첫 pane으로 간다.
    async fn switch_space(&mut self, tx: &mut FrontSender, name: String) -> Result<()> {
        if name == self.ws_name {
            return Ok(());
        }
        let known = self.spaces.get(&name).cloned().unwrap_or_else(|| Workspace { name: name.clone(), tabs: vec![] });
        self.enter_space(known);
        let id = tx.send(FrontRequest::Workspace { name: Some(name) }).await?;
        self.pending.insert(id, Pending::Report("workspace 읽기"));
        Ok(())
    }

    /// 그 workspace를 지금 workspace로 그린다(첫 탭, 포커스는 그 탭의 첫 pane)
    fn enter_space(&mut self, w: Workspace) {
        self.ws_name = w.name.clone();
        self.ws = Workspace { name: w.name.clone(), tabs: vec![] };
        self.tab = 0;
        self.focus = None;
        self.list_top = None;
        self.set_ws(w);
    }

    /// 새 workspace(herdr `NewWorkspace`, 이름을 묻는 `prompt_new_workspace_name`은 기본 꺼짐): 없는 이름
    /// `workspace`·`workspace-2`…에 `target_machine`의 셸 탭 하나를 띄우고(CLI `workspace create`와 같은 길) 응답이 오면
    /// 그 workspace로 간다. 새 셸 폴더는 새 탭처럼 포커스 pane에서 잇는다(herdr `source_workspace_id`).
    async fn new_space(&mut self, tx: &mut FrontSender) -> Result<()> {
        let taken = |n: &str| n == self.ws_name || self.spaces.contains_key(n);
        let name = std::iter::once("workspace".to_string())
            .chain((2..).map(|i| format!("workspace-{i}")))
            .find(|n| !taken(n))
            .unwrap_or_default();
        let change = LayoutChange::NewTab {
            machine: self.target_machine(),
            name: None,
            argv: vec![],
            cwd: None,
            cols: None,
            rows: None,
            label: None,
            from: self.focus.clone(),
        };
        let id = tx.send(FrontRequest::LayoutApply { workspace: Some(name), change }).await?;
        self.pending.insert(id.clone(), Pending::NewSpace);
        self.focus_wait = Some(id);
        Ok(())
    }

    fn set_ws(&mut self, w: Workspace) {
        // 탭 차례가 바뀌어도(탭 옮기기) 보던 탭에 남는다(herdr `move_tab`은 지금 탭을 그 탭으로 둔다)
        let current = self.ws.tabs.get(self.tab).map(|t| t.id.clone());
        let before = self.current_leaves();
        self.ws = w;
        let same_tab = current.as_ref().and_then(|id| self.ws.tabs.iter().position(|t| &t.id == id));
        match (current.is_some(), same_tab) {
            (_, Some(i)) => self.tab = i,
            // 보던 탭이 없어졌다(닫기·마지막 pane이 끝남): 그 앞 탭(herdr `Workspace::close_tab`·`remove_pane`)
            (true, None) => self.tab = self.tab.saturating_sub(1),
            (false, None) => {}
        }
        if self.tab >= self.ws.tabs.len() {
            self.tab = self.ws.tabs.len().saturating_sub(1);
        }
        // 기록 편집기 pane이 닫혔다: 그 pane을 보고 있었으면 연 pane으로 돌아간다(herdr `restore_overlay_after_exit`)
        if let Some((overlay, from)) = self.overlay_back.clone() {
            let panes = self.all_panes();
            if !panes.contains(&overlay) {
                self.overlay_back = None;
                if self.focus.as_ref() == Some(&overlay) && panes.contains(&from) {
                    self.select_pane(&from);
                }
            }
        }
        let leaves = self.current_leaves();
        if !self.focus.as_ref().is_some_and(|f| leaves.contains(f)) {
            let at = self.focus.as_ref().and_then(|f| before.iter().position(|p| p == f));
            // `pane resume`·재부팅 복원은 같은 자리의 leaf만 새 pane으로 바꾼다(모양 그대로): 그 자리의 새 pane을 따라간다(QA Q19)
            let same_place =
                at.filter(|_| same_tab.is_some() && leaves.len() == before.len()).and_then(|i| leaves.get(i).cloned());
            // 보던 pane이 빠졌다: 차례상 다음 pane, 없으면 앞 pane(herdr `TileLayout::close_focused`)
            let near = || {
                at.and_then(|i| {
                    before[i + 1..].iter().chain(before[..i].iter().rev()).find(|p| leaves.contains(p)).cloned()
                })
            };
            self.focus = same_place.or_else(near).or_else(|| leaves.first().cloned());
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
    fn collapsed(&self) -> bool {
        self.sidebar_collapsed.unwrap_or(false)
    }

    /// 화면 칸 나누기(herdr `ClientShellConfig::layout`). 좁으면(mobile 폭 이하) 두 줄 머리 + pane 칸.
    /// 아니면 왼쪽에 목록(전체 높이, 접으면 compact 4칸·hidden 0칸), 그 오른쪽 맨 위 한 줄이 탭 줄, 나머지가 pane 칸이다.
    /// 상태 줄은 없다: 모드·오류 막대는 pane 칸 맨 아래 줄에 겹쳐 그린다(PTY 크기는 그대로).
    fn shell(&self, area: Rect) -> Shell {
        let none = Rect { w: 0, h: 0, ..area };
        if area.w <= self.mobile_width {
            let hh = area.h.min(2);
            return Shell {
                sidebar: none,
                tab_bar: none,
                header: Rect { h: hh, ..area },
                panes: Rect { y: area.y + hh, h: area.h - hh, ..area },
            };
        }
        let side = match (self.collapsed(), self.collapsed_mode) {
            (false, _) => self.side_width(),
            (true, aaa_config::SidebarCollapsedMode::Compact) => COMPACT_WIDTH,
            (true, aaa_config::SidebarCollapsedMode::Hidden) => 0,
        }
        .min(area.w.saturating_sub(1));
        let main = Rect { x: area.x + side, w: area.w - side, ..area };
        let tab_h = u16::from(area.h > 1);
        Shell {
            sidebar: Rect { w: side, ..area },
            tab_bar: Rect { h: tab_h, ..main },
            header: none,
            panes: Rect { y: main.y + tab_h, h: main.h - tab_h, ..main },
        }
    }

    /// pane 칸 영역
    fn body(&self, area: Rect) -> Rect {
        self.shell(area).panes
    }

    fn list_rows(&self) -> Vec<list::Row> {
        let panes: Vec<PaneView> = self.pane_order.iter().filter_map(|id| self.panes.get(id).cloned()).collect();
        list::rows(&self.machines, &panes, &self.threads)
    }

    /// 고른 줄의 자리(없어졌으면 지금 pane 줄이나 그 pane의 thread 줄, 그것도 없으면 첫 줄)
    fn list_index(&self, rows: &[list::Row]) -> usize {
        let at = |r: &list::Row| rows.iter().position(|x| x == r);
        let current = || {
            let f = self.focus.as_deref()?;
            at(&list::Row::Pane(f.to_string())).or_else(|| {
                rows.iter().position(|r| {
                    matches!(r, list::Row::Thread(k) if self.threads.get(k).and_then(|t| t.pane.as_deref()) == Some(f))
                })
            })
        };
        self.list_sel.as_ref().and_then(at).or_else(current).unwrap_or_else(|| list::first(rows))
    }

    /// agents 목록에서 고른 줄(spaces 구역의 workspace를 골랐으면 없다)
    fn agent_sel(&self, rows: &[list::Row]) -> Option<usize> {
        if matches!(self.list_sel, Some(list::Row::Space(_))) {
            return None;
        }
        Some(self.list_index(rows))
    }

    /// 펼친 목록 폭(herdr `layout`: 한도로 묶는다, 화면 폭은 `shell`이 묶는다)
    fn side_width(&self) -> u16 {
        self.sidebar_width.clamp(self.sidebar_bounds.0, self.sidebar_bounds.1)
    }

    /// 목록 펴기·접기(herdr `ToggleSidebar`: 포커스는 그대로, 사용자가 정한 것으로 남긴다)
    fn toggle_list(&mut self) {
        let collapsed = !self.collapsed();
        self.sidebar_collapsed = Some(collapsed);
        self.sidebar_collapsed_manual = true;
        self.remember_chrome("tuiSidebarCollapsed", collapsed.into());
    }

    /// 끌어 바꾼 폭(herdr `set_sidebar_width_from_column`: 열 + 1을 한도로 묶는다). 남기기는 뗄 때 한다.
    /// 폭을 바꾼 누름은 두 번 누르기의 첫 번이 아니다(herdr는 끈 뒤 350ms 안에 다시 잡아도 두 번 누르기로 읽는다)
    fn drag_sidebar_to(&mut self, x: u16, left: u16) {
        let (min, max) = self.sidebar_bounds;
        let width = (x + 1).saturating_sub(left).clamp(min, max);
        if width != self.side_width() {
            self.divider_click = None;
        }
        self.sidebar_width = width;
        self.sidebar_width_manual = true;
    }

    /// 끌기를 마쳤다: 바뀐 폭을 host에 남긴다(herdr `persist_chrome_preferences`)
    fn sidebar_drag_done(&mut self) {
        if self.sidebar_width_manual {
            self.remember_chrome("tuiSidebarWidth", self.sidebar_width.into());
        }
    }

    /// 경계를 두 번 눌렀다: 설정 폭으로 되돌리고 남긴 폭을 지운다(herdr 경계 두 번 누르기)
    fn reset_sidebar_width(&mut self) {
        self.sidebar_width = self.config_sidebar_width;
        self.sidebar_width_manual = false;
        self.remember_chrome("tuiSidebarWidth", serde_json::Value::Null);
    }

    /// 사이드바 배치 하나를 host 공유 설정에 남길 것으로 둔다(다음 그리기 전에 보낸다). 보기 권한 기기는 남기지 않는다
    fn remember_chrome(&mut self, key: &str, value: serde_json::Value) {
        if !self.reader {
            self.chrome_unsaved.insert(key.to_string(), value);
        }
    }

    /// 남길 사이드바 배치를 보낸다(바뀐 이름만: 다른 TUI가 남긴 다른 값을 덮지 않는다)
    async fn save_chrome(&mut self, tx: &mut FrontSender) -> Result<()> {
        if self.chrome_unsaved.is_empty() {
            return Ok(());
        }
        let settings = std::mem::take(&mut self.chrome_unsaved);
        if self.host_minor >= PREFERENCES_MINOR {
            let id = tx.send(FrontRequest::PreferencesSet { settings, pins: None }).await?;
            self.pending.insert(id, Pending::Report("사이드바 배치 저장"));
        }
        Ok(())
    }

    /// host가 기억한 사이드바 배치(herdr `ClientShellState::new`가 읽는 client 설정): 그동안 사용자가 바꾸지 않은 것만 쓴다
    fn apply_chrome(&mut self, doc: &aaa_proto::SharedPreferences) {
        let get = |k: &str| doc.settings.get(k);
        if !self.sidebar_width_manual
            && let Some(w) = get("tuiSidebarWidth").and_then(|v| v.as_u64()).and_then(|w| u16::try_from(w).ok())
        {
            self.sidebar_width = w;
            self.sidebar_width_manual = true;
        }
        if !self.sidebar_collapsed_manual
            && let Some(c) = get("tuiSidebarCollapsed").and_then(|v| v.as_bool())
        {
            self.sidebar_collapsed = Some(c);
            self.sidebar_collapsed_manual = true;
        }
    }

    /// 목록에서 고른 줄을 연다: pane이면 그 pane이 있는 탭으로(없으면 새 탭으로 넣는다), 머신 머리면 그 머신을 고른다
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
            list::Row::Space(name) => return self.switch_space(tx, name).await,
            list::Row::Machine(id) => {
                self.pick_machine(id);
                return Ok(());
            }
            _ => return Ok(()),
        };
        self.open_pane(tx, pane).await
    }

    /// 카드 둘째 줄(herdr `notification_context`: workspace와, 탭이 여럿이면 탭). aaa는 무엇이 끝났는지 먼저 적는다:
    /// thread면 프로젝트·thread, 아니면 pane 이름과 그 pane이 든 workspace·탭
    fn card_place(&self, pane: &str, project: Option<&str>, thread: Option<&str>) -> String {
        if let (Some(p), Some(t)) = (project, thread) {
            let title = self.threads.get(&format!("{p}/{t}")).map(|t| t.title.as_str()).unwrap_or_default();
            return if title.is_empty() { format!("{p} · {t}") } else { format!("{p} · {t} · {title}") };
        }
        let name = match self.panes.get(pane) {
            Some(v) => list::pane_title(v, &self.threads),
            None => pane.chars().take(8).collect(),
        };
        let spaces = std::iter::once(&self.ws).chain(self.spaces.values().filter(|w| w.name != self.ws_name));
        for w in spaces {
            if let Some(i) = w.tabs.iter().position(|t| layout::leaves(&t.root).iter().any(|p| p == pane)) {
                if w.tabs.len() < 2 {
                    return format!("{name} · {}", w.name);
                }
                let tab = w.tabs[i].name.clone();
                let tab = if tab.is_empty() { (i + 1).to_string() } else { tab };
                return format!("{name} · {} · {tab}", w.name);
            }
        }
        match self.panes.get(pane) {
            Some(v) if !v.machine.is_empty() => format!("{name} · {}", v.machine),
            _ => name,
        }
    }

    /// agent 이름(herdr `agent_label`): host가 알린 harness, 없으면 그 pane 상태의 harness
    fn agent_label(&self, pane: &str, agent: Option<&str>) -> String {
        agent
            .map(str::to_string)
            .or_else(|| self.panes.get(pane).and_then(|v| v.agent.as_ref()).map(|a| a.harness.clone()))
            .filter(|a| !a.is_empty())
            .unwrap_or_else(|| "agent".into())
    }

    /// host 알림 → 카드. 끝남은 herdr 「{agent} finished」, 새 보고는 herdr-projects 「{프로젝트} · {thread}」·
    /// 「review · new report: {제목}」
    fn notice_card(&self, n: &aaa_proto::NoticeView) -> cards::Card {
        let pane = n.pane.clone().unwrap_or_default();
        match n.kind {
            aaa_proto::NoticeKind::Finished => cards::Card {
                kind: cards::Kind::Finished,
                title: format!("{} 끝남", self.agent_label(&pane, n.agent.as_deref())),
                body: self.card_place(&pane, n.project.as_deref(), n.thread.as_deref()),
                pane: n.pane.clone(),
            },
            aaa_proto::NoticeKind::Report => cards::Card {
                kind: cards::Kind::Report,
                title: format!(
                    "{} · {}",
                    n.project.as_deref().unwrap_or_default(),
                    n.thread.as_deref().unwrap_or_default()
                ),
                body: format!("검토 · 새 보고: {}", n.title),
                pane: n.pane.clone(),
            },
        }
    }

    /// 새로 열린 질문 → 입력 필요 카드(herdr 「{agent} needs attention」)
    fn question_card(&self, q: &aaa_proto::QuestionView) -> cards::Card {
        cards::Card {
            kind: cards::Kind::NeedsInput,
            title: format!("{} 입력 필요", self.agent_label(&q.pane, None)),
            body: self.card_place(&q.pane, q.project.as_deref(), q.thread.as_deref()),
            pane: Some(q.pane.clone()),
        }
    }

    /// 카드 때가 됐으면 다시 본다(herdr `notification_validation`·`notification_target_is_active`). 다시 그려야 하면 true
    fn tick_cards(&mut self) -> bool {
        let current: HashSet<String> = self.current_leaves().into_iter().collect();
        let panes = &self.panes;
        let check = |c: &cards::Card| {
            let Some(view) = c.pane.as_deref().and_then(|p| panes.get(p)) else { return cards::Check::Awaiting };
            let agent = view.agent.as_deref();
            match c.kind {
                cards::Kind::NeedsInput if is_asking(view) => cards::Check::Current,
                cards::Kind::Finished
                    if agent.is_some_and(|a| a.activity == aaa_proto::AgentActivity::Idle && !a.seen) =>
                {
                    cards::Check::Current
                }
                cards::Kind::Finished if agent.is_some_and(|a| a.activity == aaa_proto::AgentActivity::Working) => {
                    cards::Check::Awaiting
                }
                cards::Kind::Report => cards::Check::Current,
                _ => cards::Check::Stale,
            }
        };
        let active = |c: &cards::Card| c.pane.as_ref().is_some_and(|p| current.contains(p));
        self.cards.tick(tokio::time::Instant::now(), check, active)
    }

    /// 보이는 카드를 거두고 그 pane을 연다(`prefix o`·카드 누름, herdr `focus_visible_notification`)
    async fn open_card(&mut self, tx: &mut FrontSender) -> Result<()> {
        let Some(card) = self.cards.take(tokio::time::Instant::now()) else { return Ok(()) };
        match card.pane {
            Some(p) if self.panes.contains_key(&p) => self.open_pane(tx, p).await,
            _ => Ok(()),
        }
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
        let boxes = self.shown(tab, self.body(area));
        let reclaim = std::mem::take(&mut self.reclaim);
        // 보기 권한은 크기를 걸 수 없다(host가 거부한다): 쓰는 화면의 크기 그대로 본다
        if !self.focused || self.reader {
            return Ok(());
        }
        // PTY는 테두리 안쪽 칸 크기다(herdr `pane_inner_rect`)
        for chrome::PaneBox { pane, inner: r, .. } in boxes {
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
        self.input_to(tx, pane, data).await
    }

    /// 그 pane에 입력을 보낸다(마우스 사건은 포인터 밑 pane에). 기록을 보고 있었으면 먼저 맨 아래로(herdr `scroll_reset`).
    async fn input_to(&mut self, tx: &mut FrontSender, pane: String, data: Vec<u8>) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        self.scroll_bottom(tx, &pane).await?;
        self.send_input(tx, pane, data).await
    }

    /// 기록 위치는 그대로 두고 보낸다(누르지 않은 움직임 보고, herdr)
    async fn send_input(&mut self, tx: &mut FrontSender, pane: String, data: Vec<u8>) -> Result<()> {
        if self.refuse_reader() {
            return Ok(());
        }
        self.message = None;
        let id = tx.send(FrontRequest::PaneInput { pane, data: B64(data) }).await?;
        self.pending.insert(id, Pending::Input);
        Ok(())
    }

    /// clipboard에 그림이 있으면 포커스 pane으로 올리고 true. 없으면 아무것도 보내지 않고 false.
    async fn paste_image(&mut self, tx: &mut FrontSender) -> Result<bool> {
        if self.focus.is_none() {
            return Ok(false);
        }
        let Some(png) = tokio::task::spawn_blocking(clip::image_png).await.ok().flatten() else { return Ok(false) };
        self.send_image(tx, png).await
    }

    /// 그림 bytes를 포커스 pane으로 올린다(형식은 host가 내용의 signature로 본다). 포커스 pane이 없으면 false.
    async fn send_image(&mut self, tx: &mut FrontSender, bytes: Vec<u8>) -> Result<bool> {
        let Some(pane) = self.focus.clone() else { return Ok(false) };
        if self.refuse_reader() {
            return Ok(true);
        }
        self.scroll_bottom(tx, &pane).await?;
        self.attach_seq += 1;
        self.set_message(format!("그림 보내는 중({}KB)…", bytes.len().div_ceil(1024)));
        for req in aaa_common::front::attach_requests(&pane, &bytes) {
            let id = tx.send(req).await?;
            self.pending.insert(id, Pending::Attach(self.attach_seq));
        }
        Ok(true)
    }

    /// 보기 권한이면 pane에 보내지 않고 오류 막대로 알린다(host에 가서 거부되기를 기다리지 않는다)
    fn refuse_reader(&mut self) -> bool {
        if self.reader {
            self.set_error("보기 권한 화면이라 pane에 입력하지 않는다(조작 기기는 host에서 `aaa web pair`로 더한다)");
        }
        self.reader
    }

    fn focused_machine(&self) -> Option<String> {
        self.focus.as_ref().and_then(|f| self.panes.get(f)).map(|p| p.machine_id.clone())
    }

    /// 새 탭·새 workspace를 여는 머신(herdr 활성 endpoint): 머신 머리 줄로 고른 머신(그 뒤 포커스가 그대로일 때), 아니면
    /// 포커스 pane의 머신. 둘 다 없으면 host가 이 머신(로컬 agent)에 연다.
    fn target_machine(&self) -> Option<String> {
        match &self.machine_pick {
            Some((m, at)) if *at == self.focus => Some(m.clone()),
            _ => self.focused_machine(),
        }
    }

    /// 머신 머리 줄을 눌렀다(herdr `handle_endpoint_machine_click`): 그 머신을 새 탭·workspace를 열 머신으로. 끊긴 머신은
    /// 알리고 고르지 않는다(herdr 「… is not ready」)
    fn pick_machine(&mut self, id: String) {
        let Some(m) = self.machines.iter().find(|m| m.id == id) else { return };
        if m.connection != aaa_proto::Connection::Connected {
            self.set_error(format!("{}에 붙어 있지 않다", m.name));
            return;
        }
        self.set_message(format!("새 탭·workspace는 {}에", m.name));
        self.machine_pick = Some((id, self.focus.clone()));
    }

    /// layout 바꾸기. `focus_new`면 응답이 올 때까지 키를 붙잡아 두었다가 새 pane에 포커스를 옮긴 뒤 보낸다:
    /// herdr는 나누기·새 탭과 그 뒤 키를 서버가 차례대로 처리하므로 바로 친 키도 새 pane으로 간다.
    async fn layout(&mut self, tx: &mut FrontSender, change: LayoutChange, focus_new: bool) -> Result<()> {
        let id = tx.send(FrontRequest::LayoutApply { workspace: Some(self.ws_name.clone()), change }).await?;
        if focus_new {
            self.pending.insert(id.clone(), Pending::Focus(self.all_panes()));
            self.focus_wait = Some(id);
        } else {
            self.pending.insert(id, Pending::Report("layout"));
        }
        Ok(())
    }

    /// 키 하나. herdr `route_key`처럼 창 → 모드 순으로 고른다. 끝나면 입력기를 모드에 맞춘다.
    async fn on_key(&mut self, k: KeyEvent, tx: &mut FrontSender) -> Result<()> {
        let ch = if let KeyCode::Char(c) = k.code { Some(c) } else { None };
        // copy 모드 pane에서는 고르기가 copy 모드 것이다(herdr `copy_or_terminal_mode`)
        let in_copy = self.copy.as_ref().is_some_and(|c| self.focus.as_deref() == Some(c.pane.as_str()));
        if !in_copy && self.selection_key(tx, k.modifiers, ch).await? {
            return Ok(());
        }
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
            return self.on_overlay_key(k, tx).await;
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
                // 화살표는 프로그램이 DECCKM을 켰으면 `ESC O x`(herdr·xterm)
                let app_cursor =
                    self.focus.as_ref().and_then(|f| self.grids.get(f)).is_some_and(|g| g.modes.app_cursor);
                self.input(tx, keys::encode_in(&k, app_cursor)).await
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
            Mode::Resize => self.on_resize_key(c, tx).await,
            Mode::Copy => self.on_copy_key(c, tx).await,
        }
    }

    /// herdr `route_resize_key`: Esc·Enter·크기 모드 키는 끝, h/j/k/l·화살표는 그 방향으로 크기(한 번에 5%), 다른 키는 무시
    async fn on_resize_key(&mut self, c: Combo, tx: &mut FrontSender) -> Result<()> {
        if matches!(c.code, KeyCode::Esc | KeyCode::Enter)
            || bind::prefix_action(self.command_key(c)) == Some(Action::ResizeMode)
        {
            self.mode = Mode::Terminal;
            return Ok(());
        }
        let direction = match c.code {
            KeyCode::Char('h') | KeyCode::Left => PaneDirection::Left,
            KeyCode::Char('j') | KeyCode::Down => PaneDirection::Down,
            KeyCode::Char('k') | KeyCode::Up => PaneDirection::Up,
            KeyCode::Char('l') | KeyCode::Right => PaneDirection::Right,
            _ => return Ok(()),
        };
        let Some(pane) = self.focus.clone() else { return Ok(()) };
        self.layout(tx, LayoutChange::Resize { pane, direction, amount: None }, false).await
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
        let rows = self.nav_rows();
        let i = self.list_index(&rows);
        if c.mods.is_empty() {
            match c.code {
                KeyCode::Up => {
                    self.list_sel = rows.get(list::step(&rows, i, false)).cloned();
                    self.list_top = None;
                    return Ok(());
                }
                KeyCode::Down => {
                    self.list_sel = rows.get(list::step(&rows, i, true)).cloned();
                    self.list_top = None;
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

    /// 창이 열려 있을 때의 키(herdr `route_overlay_key`)
    async fn on_overlay_key(&mut self, k: KeyEvent, tx: &mut FrontSender) -> Result<()> {
        match self.overlay.as_mut() {
            Some(Overlay::Help(_)) => {
                let area = Rect { x: 0, y: 0, w: self.width, h: self.height };
                let prefixes = self.prefix_label();
                self.help_key(k);
                if let Some(Overlay::Help(h)) = self.overlay.as_mut() {
                    h.scroll = h.scroll.min(help::max_scroll(area, &prefixes, &h.query));
                }
            }
            // herdr `route_rename_key`: Enter 저장, Esc 취소, ^c·⌘⌫ 지우기, 나머지는 편집
            Some(Overlay::Rename(r)) => match k.code {
                KeyCode::Enter => {
                    if let Some(Overlay::Rename(r)) = self.overlay.take() {
                        self.save_rename(r, tx).await?;
                    }
                }
                KeyCode::Esc => self.overlay = None,
                KeyCode::Char('c') if k.modifiers == KeyModifiers::CONTROL => r.input.clear(),
                KeyCode::Backspace if k.modifiers.contains(KeyModifiers::SUPER) => r.input.clear(),
                _ => {
                    r.input.key(k.code, k.modifiers);
                }
            },
            Some(Overlay::Confirm(_)) => match k.code {
                KeyCode::Enter => {
                    if let Some(Overlay::Confirm(c)) = self.overlay.take() {
                        self.confirm(c, tx).await?;
                    }
                }
                KeyCode::Esc => self.overlay = None,
                _ => {}
            },
            // herdr `route_settings_key`: Esc 되돌리고 닫기, Tab·→·l 다음 구역, shift+Tab·←·h 앞 구역, ↑↓·k/j 고르기(테마는
            // 미리 보기), Enter·space 적용. 다른 키는 버린다
            Some(Overlay::Settings(s)) => {
                let plain = k.modifiers.is_empty();
                match k.code {
                    KeyCode::Esc => self.settings_cancel(),
                    KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') if plain => {
                        let next = s.next_section(true);
                        self.settings_section(next);
                    }
                    KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h')
                        if k.modifiers.difference(KeyModifiers::SHIFT).is_empty() =>
                    {
                        let next = s.next_section(false);
                        self.settings_section(next);
                    }
                    KeyCode::Up | KeyCode::Down | KeyCode::Char('k') | KeyCode::Char('j') if plain => {
                        s.step(matches!(k.code, KeyCode::Down | KeyCode::Char('j')));
                        self.settings_preview();
                    }
                    KeyCode::Enter | KeyCode::Char(' ') if plain => self.settings_apply(),
                    _ => {}
                }
            }
            // herdr: Esc 닫기, 위·아래 고르기(끝에서 멈춤), Enter 하기. 다른 키는 버린다
            Some(Overlay::Menu(m)) => match k.code {
                KeyCode::Esc => self.overlay = None,
                KeyCode::Up | KeyCode::Down => m.step(k.code == KeyCode::Down),
                KeyCode::Enter => {
                    if let Some(Overlay::Menu(m)) = self.overlay.take() {
                        let i = m.highlighted;
                        self.menu_activate(m, i, tx).await?;
                    }
                }
                _ => {}
            },
            None => {}
        }
        Ok(())
    }

    /// 설정 창 구역을 바꾼다(herdr `select_settings_section`: 그 구역의 지금 값을 고른 채로)
    fn settings_section(&mut self, section: settings::Section) {
        let selected = match section {
            settings::Section::Theme => settings::theme_index(&self.theme_name),
            settings::Section::Indicators => usize::from(self.indicators == aaa_config::StatusIndicators::Symbols),
        };
        if let Some(Overlay::Settings(s)) = self.overlay.as_mut() {
            s.section = section;
            s.selected = selected;
        }
    }

    /// 테마 구역에서 고른 테마를 바로 입혀 본다(herdr `preview_selected_theme`)
    fn settings_preview(&mut self) {
        let Some(Overlay::Settings(s)) = self.overlay.as_ref() else { return };
        if s.section != settings::Section::Theme {
            return;
        }
        if let Some(name) = theme::THEME_NAMES.get(s.selected) {
            self.theme_name = name.to_string();
            self.palette = theme::Palette::from_name(name).unwrap_or_else(theme::Palette::catppuccin);
        }
    }

    /// 설정 창을 닫고 미리 본 테마를 되돌린다(herdr `cancel_settings_overlay`)
    fn settings_cancel(&mut self) {
        let Some(Overlay::Settings(s)) = self.overlay.take() else { return };
        self.palette = theme::Palette::from_name(&s.original_theme).unwrap_or_else(theme::Palette::catppuccin);
        self.theme_name = s.original_theme;
    }

    /// 고른 값을 host.toml `[tui]`에 쓰고 다시 읽는다(herdr `apply_settings_choice`·`save_settings_edit`). 테마는 창을 닫고
    /// 상태 표시는 창을 둔다. 쓰지 못하면 창을 둔 채 알리고, host.local.toml이 그 key를 덮어 값이 그대로면 그렇다고 알린다.
    fn settings_apply(&mut self) {
        let Some(Overlay::Settings(s)) = self.overlay.as_ref() else { return };
        let (key, value) = match s.section {
            settings::Section::Theme => ("theme", theme::THEME_NAMES.get(s.selected).copied().unwrap_or("catppuccin")),
            settings::Section::Indicators => (
                "status_indicators",
                [aaa_config::StatusIndicators::Dots, aaa_config::StatusIndicators::Symbols]
                    .get(s.selected)
                    .map_or("dots", |i| i.as_str()),
            ),
        };
        let close = s.section == settings::Section::Theme;
        let dir = aaa_config::paths::config_dir();
        if let Err(e) = aaa_config::tui_edit::set(&dir, key, value.into()) {
            self.set_error(format!("설정을 저장하지 못했다: {e}"));
            return;
        }
        let cfg = match aaa_config::load_host(&dir) {
            Ok(l) => l.config.tui,
            Err(_) => {
                self.set_error("설정을 저장했지만 host.toml을 다시 읽지 못했다");
                return;
            }
        };
        self.apply_config(&cfg);
        let now = if close { self.theme_name.as_str() } else { self.indicators.as_str() };
        if now != value {
            self.set_error(format!("host.local.toml의 [tui] {key}가 덮고 있어 바뀌지 않았다"));
        }
        if close {
            self.overlay = None;
        } else if let Some(Overlay::Settings(s)) = self.overlay.as_mut() {
            s.original_theme = self.theme_name.clone();
        }
    }

    /// 닫기 확인 창에서 닫기
    async fn confirm(&mut self, c: prompt::Confirm, tx: &mut FrontSender) -> Result<()> {
        let change = match c.target {
            prompt::CloseTarget::Tab(tab) => LayoutChange::CloseTab { tab },
            prompt::CloseTarget::Pane(pane) => LayoutChange::Close { pane, kill: true },
        };
        self.layout(tx, change, false).await
    }

    /// 이름 창 저장(herdr `save_rename_overlay`): 새 탭은 기본 이름 그대로·빈 이름이면 자리 번호,
    /// 탭 이름은 비었거나 자동 이름을 그대로 두면 바꾸지 않는다
    async fn save_rename(&mut self, r: prompt::Rename, tx: &mut FrontSender) -> Result<()> {
        let text = r.input.trim().to_string();
        match r.target {
            prompt::RenameTarget::NewTab { default } => {
                let label = (!text.is_empty() && text != default).then_some(text);
                self.new_tab(tx, label).await
            }
            prompt::RenameTarget::Tab { id, auto, original } => {
                if text.is_empty() || auto && text == original || !self.host_at_least(4) {
                    return Ok(());
                }
                self.layout(tx, LayoutChange::RenameTab { tab: id, name: text }, false).await
            }
            // herdr: pane 이름은 저장하면 늘 보낸다(비우면 host가 지운다)
            prompt::RenameTarget::Pane { id } => self.rename_pane(tx, id, Some(text)).await,
        }
    }

    /// pane 이름 붙이기·지우기(host `pane_rename`, 1.15)
    pub(crate) async fn rename_pane(
        &mut self,
        tx: &mut FrontSender,
        pane: String,
        label: Option<String>,
    ) -> Result<()> {
        if !self.host_at_least(15) {
            return Ok(());
        }
        let id = tx.send(FrontRequest::PaneRename { pane, label }).await?;
        self.pending.insert(id, Pending::Report("pane 이름"));
        Ok(())
    }

    /// pane 이름 창(herdr `open_rename_pane_overlay`): 붙인 이름이 있으면 그 이름, 없으면 빈 칸
    pub(crate) fn open_rename_pane(&mut self, pane: String) {
        let label = self.panes.get(&pane).and_then(|p| p.label.clone());
        self.overlay = Some(Overlay::Rename(prompt::Rename {
            title: "pane 이름 바꾸기",
            input: editor::Editor::new(label.as_deref().unwrap_or_default(), label.is_none()),
            target: prompt::RenameTarget::Pane { id: pane },
        }));
    }

    /// 새 탭: `target_machine`에, 포커스 pane이 지금 있는 폴더에서(host `[terminal] new_cwd`, 다른 머신이면 home)
    async fn new_tab(&mut self, tx: &mut FrontSender, label: Option<String>) -> Result<()> {
        let machine = self.target_machine();
        let change = LayoutChange::NewTab {
            machine,
            name: None,
            argv: vec![],
            cwd: None,
            cols: None,
            rows: None,
            label,
            from: self.focus.clone(),
        };
        self.layout(tx, change, true).await
    }

    /// 새 layout 요청은 그것을 아는 host에만 보낸다(탭 닫기·이름·크기 1.4, zoom·자리 바꾸기 1.10, 나눔 비율·탭 옮기기 1.14).
    /// 옛 host에 보내면 연결이 끊기므로 알리고 보내지 않는다.
    /// 이 화면이 그 pane 기록을 보고 있으면 맨 아래로 돌린다(다음 장면이 오기 전에 또 보내지 않게 여기서도 0으로)
    async fn scroll_bottom(&mut self, tx: &mut FrontSender, pane: &str) -> Result<()> {
        let Some(g) = self.grids.get_mut(pane) else { return Ok(()) };
        if g.scroll.offset == 0 || self.host_minor < 26 {
            return Ok(());
        }
        g.scroll.offset = 0;
        let id = tx.send(FrontRequest::PaneScroll { pane: pane.to_string(), by: ScrollBy::Bottom }).await?;
        self.pending.insert(id, Pending::Report("기록 넘기기"));
        Ok(())
    }

    fn host_at_least(&mut self, minor: u16) -> bool {
        if self.host_minor >= minor {
            return true;
        }
        self.set_error("host가 이 명령보다 옛 판이다: host를 새 aaa로 다시 띄워야 한다");
        false
    }

    /// 탭 이름표(herdr `tab_display_name`: 붙인 이름, 없으면 자리 번호)
    fn tab_label(&self, i: usize) -> String {
        match self.ws.tabs.get(i) {
            Some(t) if !t.name.is_empty() => t.name.clone(),
            _ => (i + 1).to_string(),
        }
    }

    /// 탭 줄에 쓰는 글(herdr `tabs.rs` `tab_label`: zoom이면 ` Z`를 붙인다)
    fn chip_label(&self, i: usize) -> String {
        let label = self.tab_label(i);
        if self.ws.tabs.get(i).is_some_and(|t| t.zoomed) { format!("{label} Z") } else { label }
    }

    /// 그릴 pane 상자(테두리·PTY 칸). zoom이면 포커스 pane 하나가 칸 전체다(herdr `render_panes`).
    /// 방향 포커스·자리 바꾸기는 zoom과 상관없이 나눈 칸으로 찾는다(`pane_rects`).
    /// herdr `terminal_inner_rect_for`: scrollbar를 켜면 alt 화면이 아닌 pane은 오른쪽 1칸(폭이 4보다 클 때)을 scrollbar 칸으로 뺀다
    /// (PTY도 그만큼 좁다, `stable_scrollbar_gutter`: 기록이 없어도 칸은 비워 둔다).
    fn shown(&self, tab: &aaa_proto::Tab, body: Rect) -> Vec<chrome::PaneBox> {
        let zoomed = self.focus.as_deref().filter(|f| tab.zoomed && layout::leaves(&tab.root).iter().any(|p| p == f));
        let mut boxes = chrome::layout(&tab.root, body, self.pane_borders, zoomed);
        if self.pane_scrollbars {
            for b in &mut boxes {
                let alt = self.grids.get(&b.pane).is_some_and(|g| g.modes.alt_screen);
                if !alt && b.inner.w > 4 {
                    b.inner.w -= 1;
                    b.gutter = Some(Rect { x: b.inner.x + b.inner.w, w: 1, ..b.inner });
                }
            }
        }
        boxes
    }

    /// 마지막 탭(이고 pane이 `pane`뿐)인가: herdr `confirm_close`는 workspace가 닫힐 때만 묻는다
    fn closes_everything(&self, pane: Option<&str>) -> bool {
        self.ws.tabs.len() == 1 && pane.is_none_or(|p| self.current_leaves() == [p])
    }

    fn close_confirm(&self, target: prompt::CloseTarget) -> prompt::Confirm {
        let n = self.current_leaves().len();
        prompt::Confirm {
            title: "마지막 탭을 닫을까?".into(),
            detail: format!("{} — pane {n}개(실행 중인 세션은 끝난다)", self.ws_name),
            target,
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
            Action::Settings => {
                self.overlay = Some(Overlay::Settings(settings::Settings {
                    section: settings::Section::Theme,
                    selected: settings::theme_index(&self.theme_name),
                    original_theme: self.theme_name.clone(),
                }))
            }
            Action::Detach => self.quit = true,
            Action::ReloadConfig => {
                let cfg = tui_config();
                self.apply_config(&cfg);
                if self.error.is_none() {
                    self.set_message("설정을 다시 읽었다");
                }
            }
            // herdr `focus_visible_notification`: 보이는 카드를 거두고 그 pane으로. 카드가 없으면 아무것도 하지 않는다
            Action::OpenNotificationTarget => self.open_card(tx).await?,
            // herdr `open_navigate`: 지금 workspace 줄에서 시작한다
            Action::WorkspacePicker => {
                self.mode = Mode::Navigate;
                self.list_sel = Some(list::Row::Space(self.ws_name.clone()));
            }
            // herdr `open_new_tab_overlay`: 기본 이름(탭 수+1)을 골라 둔 채로 묻는다
            Action::NewTab if self.prompt_new_tab => {
                let default = (self.ws.tabs.len() + 1).to_string();
                self.overlay = Some(Overlay::Rename(prompt::Rename {
                    title: "새 탭",
                    input: editor::Editor::new(&default, true),
                    target: prompt::RenameTarget::NewTab { default },
                }));
            }
            Action::NewTab => self.new_tab(tx, None).await?,
            Action::NewWorkspace => self.new_space(tx).await?,
            Action::RenameTab => {
                if let Some(t) = self.ws.tabs.get(self.tab) {
                    let original = self.tab_label(self.tab);
                    self.overlay = Some(Overlay::Rename(prompt::Rename {
                        title: "탭 이름 바꾸기",
                        input: editor::Editor::new(&original, false),
                        target: prompt::RenameTarget::Tab { id: t.id.clone(), auto: t.name.is_empty(), original },
                    }));
                }
            }
            Action::CopyMode => self.enter_copy(tx).await?,
            // herdr `edit_scrollback`: 기록을 그 pane 머신 편집기로 연다(host가 나눠 띄우고 zoom한다). 응답까지 키를 붙잡는다
            Action::EditScrollback => {
                let Some(pane) = focus else { return Ok(()) };
                if self.host_at_least(32) {
                    let req =
                        FrontRequest::PaneEditScrollback { workspace: Some(self.ws_name.clone()), pane: pane.clone() };
                    let id = tx.send(req).await?;
                    self.pending.insert(id.clone(), Pending::EditScrollback { before: self.all_panes(), from: pane });
                    self.focus_wait = Some(id);
                }
            }
            Action::RenamePane => {
                if let Some(pane) = self.focus.clone() {
                    self.open_rename_pane(pane);
                }
            }
            Action::CloseTab => {
                let Some(tab) = self.ws.tabs.get(self.tab).map(|t| t.id.clone()) else { return Ok(()) };
                if self.host_at_least(4) {
                    if self.confirm_close && self.closes_everything(None) {
                        self.overlay = Some(Overlay::Confirm(self.close_confirm(prompt::CloseTarget::Tab(tab))));
                    } else {
                        self.layout(tx, LayoutChange::CloseTab { tab }, false).await?;
                    }
                }
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
                self.layout(
                    tx,
                    LayoutChange::Split { pane, dir, machine: None, argv: vec![], cwd: None, ratio: None },
                    true,
                )
                .await?;
            }
            Action::ClosePane => {
                let Some(pane) = focus else { return Ok(()) };
                if self.confirm_close && self.closes_everything(Some(&pane)) {
                    self.overlay = Some(Overlay::Confirm(self.close_confirm(prompt::CloseTarget::Pane(pane))));
                } else {
                    self.layout(tx, LayoutChange::Close { pane, kill: true }, false).await?;
                }
            }
            Action::ToggleSidebar => self.toggle_list(),
            Action::FocusLeft => self.move_focus(Dir::Left),
            Action::FocusDown => self.move_focus(Dir::Down),
            Action::FocusUp => self.move_focus(Dir::Up),
            Action::FocusRight => self.move_focus(Dir::Right),
            // herdr `pane zoom` 뒤집기: 포커스 pane을 탭 가득(host가 탭마다 기억한다)
            Action::Zoom => {
                let Some(pane) = focus else { return Ok(()) };
                if self.host_at_least(10) {
                    self.layout(tx, LayoutChange::Zoom { pane, zoomed: None }, false).await?;
                }
            }
            Action::ResizeMode => {
                if self.host_at_least(4) {
                    self.mode = Mode::Resize;
                }
            }
            // herdr `swap_pane_*`: 그 방향 이웃(포커스와 같은 찾기)과 자리를 바꾸고 포커스는 옮긴 pane을 따라간다
            Action::SwapLeft | Action::SwapDown | Action::SwapUp | Action::SwapRight => {
                let dir = match a {
                    Action::SwapLeft => Dir::Left,
                    Action::SwapDown => Dir::Down,
                    Action::SwapUp => Dir::Up,
                    _ => Dir::Right,
                };
                let rects = self.pane_rects();
                let Some(cur) = rects.iter().find(|(p, _)| Some(p) == focus.as_ref()).map(|(_, r)| *r) else {
                    return Ok(());
                };
                if let (Some(pane), Some(with)) = (focus, find_in_direction(cur, dir, &rects))
                    && self.host_at_least(10)
                {
                    self.layout(tx, LayoutChange::Swap { pane, with }, false).await?;
                }
            }
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

    /// 지금 탭의 pane 칸들(테두리 포함, zoom이 아닐 때 그린 화면 그대로)
    fn pane_rects(&self) -> Vec<(String, Rect)> {
        let Some(tab) = self.ws.tabs.get(self.tab) else { return vec![] };
        let body = self.body(Rect { x: 0, y: 0, w: self.width, h: self.height });
        chrome::layout(&tab.root, body, self.pane_borders, None).into_iter().map(|b| (b.pane, b.rect)).collect()
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

    /// 그리고 포커스 pane의 커서 위치를 돌려준다. 배치는 herdr `ClientShellConfig::layout`·`render_shell`:
    /// 왼쪽 사이드바(온 높이) · 오른쪽 위 탭 막대 · 그 아래 pane, 좁으면 두 줄 머리. 상태 줄은 없고 모드 막대는 pane 맨 아랫줄에 겹친다.
    fn render(&self, buf: &mut Buffer, area: Rect) -> Option<(u16, u16)> {
        let p = &self.palette;
        let sh = self.shell(area);
        *self.hits.borrow_mut() = mouse::Hits::default();
        if sh.header.h > 0 {
            self.render_header(buf, sh.header);
        }
        if sh.sidebar.w > 0 {
            if self.collapsed() {
                self.render_compact(buf, sh.sidebar);
            } else {
                self.render_sidebar(buf, sh.sidebar);
            }
        }
        if sh.tab_bar.h > 0 {
            self.render_tab_bar(buf, sh.tab_bar);
        }
        let body = sh.panes;
        let mut cursor = None;
        match self.ws.tabs.get(self.tab) {
            None => {
                let lines = [
                    ("탭이 없다".to_string(), Style::default().fg(p.text).add_modifier(Modifier::BOLD)),
                    (format!("{} 다음 c로 새 탭 · ?로 단축키", self.prefix_label()), Style::default().fg(p.overlay0)),
                ];
                for (n, (msg, st)) in lines.iter().enumerate() {
                    let w = unicode_width(msg).min(body.w as usize) as u16;
                    let y = body.y + body.h / 2 + n as u16;
                    buf.set_stringn(body.x + (body.w - w) / 2, y.saturating_sub(1), msg, w as usize, *st);
                }
            }
            Some(tab) => {
                // herdr `render_pane_borders`·`render_pane_border_titles`
                let boxes = self.shown(tab, body);
                let title = |id: &str| self.panes.get(id).and_then(border_title);
                chrome::render(buf, &boxes, self.focus.as_deref(), title, p);
                let zoomed = boxes.len() == 1 && layout::leaves(&tab.root).len() > 1;
                let mut hits = self.hits.borrow_mut();
                hits.splits = chrome::splits(&tab.root, body, &boxes, zoomed);
                hits.panes = boxes.clone();
                drop(hits);
                for b in &boxes {
                    let focused = self.focus.as_deref() == Some(b.pane.as_str());
                    if let Some(c) = self.render_pane(buf, &b.pane, b.inner, focused) {
                        cursor = Some(c);
                    }
                    // herdr `render_pane_scrollbar`: 기록이 있을 때만
                    if let (Some(track), Some(g)) = (b.gutter, self.grids.get(&b.pane)) {
                        scrollbar::render(buf, scrollbar::Metrics::new(g.scroll, g.rows), track, p, focused);
                    }
                }
            }
        }
        // 탐색 모드인데 사이드바가 펼쳐져 있지 않으면 펼친 목록을 pane 위에 띄운다(pane 크기는 그대로).
        // herdr는 접힌 칸의 번호만 강조한다: 이름을 보며 고를 수 있게 aaa는 띄운다(plan §8.73).
        if self.mode == Mode::Navigate && (self.collapsed() || sh.sidebar.w == 0) {
            // pane 칸 위에만(접힌 칸·탭 막대·머리는 그대로), 모드 막대(pane 맨 아랫줄) 위까지
            let w = if sh.header.h > 0 { body.w } else { self.side_width().min(body.w) };
            self.render_sidebar(buf, Rect { w, h: body.h.saturating_sub(1), ..body });
            self.hits.borrow_mut().divider = None;
        }
        if let Some(bar) = self.render_mode_bar(buf, body)
            && cursor.is_some_and(|(_, y)| y == bar.y)
        {
            cursor = None;
        }
        if let Some((m, _)) = &self.message {
            render_notice(buf, area, m, p);
        }
        // agent 알림 카드(herdr composition.rs: 넓으면 오른쪽 아래 카드, 좁으면 맨 아래 한 줄)
        self.hits.borrow_mut().card = self.cards.visible().and_then(|c| {
            if sh.header.h > 0 { render_card_banner(buf, area, c, p) } else { render_card(buf, area, c, p) }
        });
        let mut hits = self.hits.borrow_mut();
        match &self.overlay {
            Some(Overlay::Menu(m)) => {
                let (rect, rows) = mouse::render_menu(buf, area, m, p);
                hits.popup = Some(rect);
                hits.menu_rows = rows;
                return None;
            }
            Some(Overlay::Help(h)) => {
                hits.popup = help::popup(area, 76, 22);
                return help::render(buf, area, h, &self.prefix_label(), p);
            }
            Some(Overlay::Rename(r)) => {
                let bs = [mouse::Button::Save, mouse::Button::Clear, mouse::Button::Cancel];
                hits.buttons = prompt::rename_buttons(area).into_iter().zip(bs).collect();
                return prompt::render_rename(buf, area, r, p);
            }
            Some(Overlay::Settings(s)) => {
                let h = settings::render(buf, area, s, p);
                hits.popup = h.as_ref().map(|h| h.popup);
                hits.settings = h;
                return None;
            }
            Some(Overlay::Confirm(c)) => {
                hits.buttons = prompt::confirm_buttons(area)
                    .into_iter()
                    .zip([mouse::Button::Close, mouse::Button::Cancel])
                    .collect();
                prompt::render_confirm(buf, area, c, p);
                return None;
            }
            None => {}
        }
        cursor
    }

    /// 탭 막대(herdr `render_tab_bar`): `panel_bg` 줄에 이름표들. 지금 탭은 accent 바탕(이름을 붙였으면 굵게),
    /// 다른 탭은 `surface0` 바탕에 붙인 이름 `overlay1`·자동 이름 `overlay0`. 넘치면 지금 탭을 가운데로 밀고 `…`.
    /// 오른쪽 끝은 herdr `tab_bar_right` 자리: 사람 차례·검토 thread 수(herdr-projects `needs_you_line`), 다른 화면이 크기를 쥔 포커스 pane.
    fn render_tab_bar(&self, buf: &mut Buffer, r: Rect) {
        let p = &self.palette;
        fill(buf, r, Style::default().bg(p.panel_bg));
        let status = self.tab_bar_right();
        let status_w: u16 =
            status.iter().map(|(t, _)| unicode_width(t) as u16).sum::<u16>() + status.len().saturating_sub(1) as u16;
        let reserved =
            if status_w > 0 && r.w.saturating_sub(status_w + 1) >= MIN_TAB_STRIP_WIDTH { status_w + 1 } else { 0 };
        let content = Rect { w: r.w - reserved, ..r };
        let labels: Vec<String> = (0..self.ws.tabs.len()).map(|i| self.chip_label(i)).collect();
        let scroll = self.tab_scroll.filter(|(_, t)| *t == self.tab).map(|(s, _)| s);
        let strip = tabs::layout_with(&labels, self.tab, content.x, content.w, self.mouse_capture, scroll);
        let mut hits = self.hits.borrow_mut();
        hits.tab_bar = Some(r);
        hits.tab_scroll = strip.scroll;
        hits.max_tab_scroll = strip.max_scroll;
        let button = |on: bool| Style::default().fg(if on { p.overlay1 } else { p.overlay0 }).bg(p.surface0);
        if let Some(x) = strip.scroll_left {
            buf.set_stringn(x, r.y, " < ", 3, button(strip.scroll > 0));
            hits.scroll_left = Some(Rect { x, y: r.y, w: 3, h: 1 });
        }
        if let Some(x) = strip.scroll_right {
            buf.set_stringn(x, r.y, " > ", 3, button(strip.scroll < strip.max_scroll));
            hits.scroll_right = Some(Rect { x, y: r.y, w: 3, h: 1 });
        }
        if let Some((x, w)) = strip.new_tab {
            buf.set_stringn(x, r.y, " + ", w as usize, Style::default().fg(p.overlay1).bg(p.panel_bg));
            hits.new_tab = Some(Rect { x, y: r.y, w, h: 1 });
        }
        for c in &strip.chips {
            hits.tabs.push((Rect { x: c.x, y: r.y, w: c.w, h: 1 }, c.index));
            let custom = !self.ws.tabs[c.index].name.is_empty();
            let style = if c.index == self.tab {
                let s = Style::default().fg(theme::contrast(p)).bg(p.accent);
                if custom { s.add_modifier(Modifier::BOLD) } else { s }
            } else if custom {
                Style::default().fg(p.overlay1).bg(p.surface0)
            } else {
                Style::default().fg(p.overlay0).bg(p.surface0)
            };
            let name = &labels[c.index];
            let pad = c.w.saturating_sub(unicode_width(name) as u16);
            let left = pad / 2;
            let text = format!("{:l$}{name}{:r$}", "", "", l = left as usize, r = pad.saturating_sub(left) as usize);
            buf.set_stringn(c.x, r.y, &text, c.w as usize, style);
        }
        let more = Style::default().fg(p.overlay0).bg(p.panel_bg);
        if strip.more_left && strip.ellipsis_left < content.x + content.w {
            buf.set_string(strip.ellipsis_left, r.y, "…", more);
        }
        if strip.more_right && content.w > 0 && strip.ellipsis_right >= content.x {
            buf.set_string(strip.ellipsis_right, r.y, "…", more);
        }
        // 끌던 탭을 넣을 자리(herdr `tab_drop_indicator_x`)
        if let Some(mouse::Drag::Tab { moving: true, insert: Some(i), .. }) = &self.drag
            && let Some(x) = mouse::drop_x(&hits, *i, self.ws.tabs.len())
            && content.w > 0
        {
            let x = x.min(content.x + content.w - 1);
            buf.set_string(x, r.y, "│", Style::default().fg(p.accent).bg(p.panel_bg));
        }
        if reserved > 0 {
            let mut x = r.x + r.w - status_w;
            for (i, (t, st)) in status.iter().enumerate() {
                if i > 0 {
                    x = buf.set_stringn(x, r.y, " ", usize::from(u16::MAX), Style::default().bg(p.panel_bg)).0;
                }
                x = buf.set_stringn(x, r.y, t, usize::from(u16::MAX), *st).0;
            }
        }
    }

    /// 탭 막대 오른쪽 조각들(herdr `tab_bar_right` 모양: 강조 조각은 accent 바탕 굵게, 아니면 `overlay1`)
    fn tab_bar_right(&self) -> Vec<(String, Style)> {
        let p = &self.palette;
        let mut out = Vec::new();
        // herdr-projects 탭 줄 `projects: N need you`(sidebar.rs `needs_you_line`): 사람이 볼 thread(사람 차례·검토) 수.
        // 고르는 곳은 참고의 popup(prefix+a) 대신 탐색 목록이다(thread 줄이 사람 차례부터 선다, 차이 목록 4장 D)
        let need = self
            .threads
            .values()
            .filter(|t| {
                matches!(t.group, aaa_proto::ThreadGroup::WaitingOnYou | aaa_proto::ThreadGroup::ReadyForReview)
            })
            .count();
        if need > 0 {
            out.push((
                format!(" ● 사람 차례 {need} "),
                Style::default().fg(theme::contrast(p)).bg(p.red).add_modifier(Modifier::BOLD),
            ));
        }
        // 보기 권한 연결(웹의 「보기 전용」 표시와 같다): 입력·크기·layout 바꾸기를 하지 않는다
        if self.reader {
            out.push((
                " 보기 전용 ".into(),
                Style::default().fg(theme::contrast(p)).bg(p.peach).add_modifier(Modifier::BOLD),
            ));
        }
        // 다른 화면이 이 pane의 PTY 크기를 쥐고 있다(입력하거나 이 터미널로 돌아오면 이 화면 크기로 바뀐다)
        if let Some(l) = self.focus.as_ref().and_then(|f| self.panes.get(f)).and_then(|v| v.lease.as_ref())
            && l.front != self.me
        {
            out.push((format!("{} 크기", l.label), Style::default().fg(p.peach).bg(p.panel_bg)));
        }
        out
    }

    fn list_view<'a>(&'a self, rows: &[list::Row]) -> list::View<'a> {
        list::View {
            machines: &self.machines,
            panes: &self.panes,
            threads: &self.threads,
            current: self.focus.as_deref(),
            sel: self.agent_sel(rows),
            focused: self.mode == Mode::Navigate,
            top: self.list_top,
            pal: &self.palette,
            style: self.indicators,
            now_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64),
        }
    }

    /// 펼친 사이드바(herdr `render_sidebar`): 바탕과 오른쪽 구분선, 위 ` spaces` 구역(workspace 줄), 아래 ` agents` 구역
    /// (구분선·머리·목록), 맨 아래 오른쪽 접기 표시 `«`
    fn render_sidebar(&self, buf: &mut Buffer, r: Rect) {
        let p = &self.palette;
        sidebar_background(buf, r, p);
        let content = Rect { w: r.w.saturating_sub(1), ..r };
        // herdr `expanded_sidebar_sections`: 위 spaces, 아래 agents(비율 0.5, 끌어 바꾸기는 아직 없다)
        let (top_h, rest_h) = list::section_heights(content.h, 0.5);
        let spaces_area = Rect { h: top_h, ..content };
        let agents_area = Rect { y: content.y + top_h, h: rest_h, ..content };
        let sel = match &self.list_sel {
            Some(list::Row::Space(n)) => Some(n.as_str()),
            _ => None,
        };
        let navigating = self.mode == Mode::Navigate;
        let spaces = self.space_list();
        let placed_spaces =
            list::render_spaces(buf, spaces_area, &spaces, &self.ws_name, sel, navigating, p, self.indicators);
        // herdr: 마우스를 받을 때만 ` new`와 `menu`, 머신이 여럿이면 새 workspace를 열 머신 이름을 붙인다(없으면 로컬 agent)
        let footer = self.mouse_capture.then(|| {
            let id = self.target_machine().or_else(|| self.machines.iter().find(|m| m.local).map(|m| m.id.clone()));
            let name = id.and_then(|id| self.machines.iter().find(|m| m.id == id)).map(|m| m.name.as_str());
            list::render_spaces_footer(buf, spaces_area, name.filter(|_| self.machines.len() > 1), p)
        });
        let body = list::render_agents_header(buf, agents_area, p);
        let rows = self.list_rows();
        let mut hits = self.hits.borrow_mut();
        hits.sidebar = Some(r);
        (hits.new_workspace, hits.global_menu) = footer.flatten().unzip();
        hits.rows =
            placed_spaces.into_iter().map(|(y, n)| (Rect { y, h: 1, ..content }, list::Row::Space(n))).collect();
        if body.h > 0 {
            let (placed, skip) = list::render(buf, body, &rows, &self.list_view(&rows));
            // thread 둘째 줄을 누르면 그 thread
            hits.rows.extend(placed.into_iter().map(|(y, n)| (Rect { y, h: 1, ..body }, rows[n].target())));
            hits.list_skip = skip;
            hits.list_h = body.h;
        }
        if r.w > 1 && r.h > 0 {
            buf.set_string(r.x + r.w - 2, r.y + r.h - 1, "«", Style::default().fg(p.overlay0));
            // herdr: 접기 `«`는 (오른쪽−2, 아래−1), 경계는 마지막 열(띄운 목록은 끌지 않는다)
            hits.toggle = Some(Rect { x: r.x + r.w - 2, y: r.y + r.h - 1, w: 1, h: 1 });
            if !self.collapsed() {
                hits.divider = Some(Rect { x: r.x + r.w - 1, w: 1, ..r });
            }
        }
    }

    /// 접힌 사이드바(herdr `render_collapsed_sidebar`): 번호와 상태 점, 맨 아래 가운데 `»`
    fn render_compact(&self, buf: &mut Buffer, r: Rect) {
        let p = &self.palette;
        sidebar_background(buf, r, p);
        let content = Rect { w: r.w.saturating_sub(1), h: r.h.saturating_sub(1), ..r };
        let rows = self.list_rows();
        let placed = list::render_compact(buf, content, &rows, &self.list_view(&rows));
        let mut hits = self.hits.borrow_mut();
        hits.sidebar = Some(r);
        hits.rows = placed.into_iter().map(|(y, n)| (Rect { y, h: 1, ..content }, rows[n].clone())).collect();
        if content.w > 0 && r.h > 0 {
            buf.set_string(content.x + content.w / 2, r.y + r.h - 1, "»", Style::default().fg(p.overlay0));
            hits.toggle = Some(Rect { x: content.x + content.w / 2, y: r.y + r.h - 1, w: 1, h: 1 });
        }
    }

    /// 좁은 화면 두 줄 머리(herdr `render_mobile_header`): 상태 점·workspace·탭 자리, 둘째 줄 agent 요약,
    /// 오른쪽 `목록` 단추 자리(herdr `switch`, 탐색 모드로 연다)
    fn render_header(&self, buf: &mut Buffer, r: Rect) {
        let p = &self.palette;
        fill(buf, r, Style::default().bg(p.panel_bg));
        let bw = MOBILE_BUTTON_WIDTH.min(r.w);
        let button = Rect { x: r.x + r.w - bw, w: bw, ..r };
        self.hits.borrow_mut().header_button = Some(button);
        fill(buf, button, Style::default().bg(p.surface0));
        for y in button.y..button.y + button.h {
            buf.set_string(button.x, y, "│", Style::default().fg(p.surface_dim).bg(p.surface0));
        }
        let label = "목록";
        let ly = if button.h > 1 { button.y + 1 } else { button.y };
        let lx = button.x + 1 + button.w.saturating_sub(1 + unicode_width(label) as u16) / 2;
        buf.set_stringn(
            lx,
            ly,
            label,
            button.w.saturating_sub(1) as usize,
            Style::default().fg(p.text).bg(p.surface0).add_modifier(Modifier::BOLD),
        );
        let panes: Vec<&PaneView> = self.pane_order.iter().filter_map(|id| self.panes.get(id)).collect();
        let dots: Vec<list::Dot> = panes.iter().map(|v| list::pane_dot(v)).collect();
        if dots.contains(&list::Dot::Blocked) && button.w > 1 {
            buf.set_string(
                button.x + button.w - 1,
                button.y,
                list::Dot::Blocked.icon(self.indicators),
                Style::default().fg(p.red).bg(p.surface0),
            );
        }
        let status = Rect { w: button.x.saturating_sub(r.x).saturating_sub(1), ..r };
        let worst = [list::Dot::Blocked, list::Dot::Working, list::Dot::Idle]
            .into_iter()
            .find(|d| dots.contains(d))
            .unwrap_or(list::Dot::Unknown);
        let tab = match self.ws.tabs.len() {
            0 => String::new(),
            1 => format!("탭 {}", self.tab_label(0)),
            n => format!("탭 {} · {}/{n}", self.tab_label(self.tab), self.tab + 1),
        };
        let tab_w = (unicode_width(&tab) as u16 + 1).min(status.w);
        let name_w = status.w.saturating_sub(tab_w);
        buf.set_stringn(
            status.x,
            status.y,
            format!(" {} ", worst.icon(self.indicators)),
            name_w.min(3) as usize,
            Style::default().fg(worst.color(p)).bg(p.panel_bg),
        );
        buf.set_stringn(
            status.x + 3,
            status.y,
            &self.ws_name,
            name_w.saturating_sub(4) as usize,
            Style::default().fg(p.text).bg(p.panel_bg).add_modifier(Modifier::BOLD),
        );
        buf.set_stringn(
            status.x + status.w - tab_w + 1,
            status.y,
            &tab,
            tab_w.saturating_sub(1) as usize,
            Style::default().fg(p.overlay1).bg(p.panel_bg),
        );
        if status.h > 1 {
            self.render_agent_summary(buf, Rect { y: status.y + 1, h: 1, ..status }, &dots);
        }
    }

    /// herdr `render_agent_summary`: 상태별 agent 수(첫 조각만 그 색 굵게, 나머지는 `overlay1`, 넘치면 ` …`)
    fn render_agent_summary(&self, buf: &mut Buffer, r: Rect, dots: &[list::Dot]) {
        let p = &self.palette;
        let counts = [(list::Dot::Blocked, "질문"), (list::Dot::Working, "작업 중"), (list::Dot::Idle, "대기")]
            .map(|(d, label)| (d, label, dots.iter().filter(|x| **x == d).count()));
        let total: usize = counts.iter().map(|c| c.2).sum();
        let base = Style::default().bg(p.panel_bg);
        if total == 0 || counts[..2].iter().all(|c| c.2 == 0) {
            let t = if total == 0 { " agent 없음" } else { " 모두 대기" };
            buf.set_stringn(r.x, r.y, t, r.w as usize, base.fg(p.overlay1));
            return;
        }
        let end = r.x + r.w;
        let mut x = r.x + 1;
        let mut shown = 0;
        for (d, label, n) in counts {
            if n == 0 {
                continue;
            }
            let text = if d == list::Dot::Blocked { format!("◉ {n} {label}") } else { format!("{n} {label}") };
            let sep = if shown == 0 { "" } else { " · " };
            if end.saturating_sub(x) < (unicode_width(sep) + unicode_width(&text)) as u16 {
                if end.saturating_sub(x) >= 2 {
                    buf.set_string(x, r.y, " …", base.fg(p.overlay0));
                }
                return;
            }
            x = buf.set_stringn(x, r.y, sep, usize::from(u16::MAX), base.fg(p.overlay0)).0;
            let st = if shown == 0 { base.fg(d.color(p)).add_modifier(Modifier::BOLD) } else { base.fg(p.overlay1) };
            x = buf.set_stringn(x, r.y, &text, usize::from(u16::MAX), st).0;
            shown += 1;
        }
    }

    /// herdr `render_mode_bar`: 오류가 있으면 ERROR, 모드면 그 안내를 pane 맨 아랫줄에 겹쳐 그린다. 평소에는 없다.
    fn render_mode_bar(&self, buf: &mut Buffer, pane_area: Rect) -> Option<Rect> {
        let segments = self.mode_bar()?;
        if pane_area.w == 0 || pane_area.h == 0 {
            return None;
        }
        let bar = Rect { y: pane_area.y + pane_area.h - 1, h: 1, ..pane_area };
        fill(buf, bar, Style::default().fg(self.palette.overlay0).bg(self.palette.panel_bg));
        if let Some(prompt) = self.copy.as_ref().and_then(|c| c.prompt.as_ref()).filter(|_| self.mode == Mode::Copy)
            && self.error.is_none()
        {
            let p = &self.palette;
            let chip = Style::default().fg(theme::contrast(p)).bg(p.accent).add_modifier(Modifier::BOLD);
            let key = Style::default().fg(p.accent).bg(p.panel_bg).add_modifier(Modifier::BOLD);
            copy::render_prompt(buf, bar, prompt, p, [chip, key, Style::default().fg(p.overlay0).bg(p.panel_bg)]);
            return Some(bar);
        }
        let mut x = bar.x;
        let end = bar.x + bar.w;
        for (text, st) in segments {
            if x >= end {
                break;
            }
            x = buf.set_stringn(x, bar.y, &text, (end - x) as usize, st).0;
        }
        Some(bar)
    }

    /// 모드 막대 조각(herdr `render_mode_bar`의 색: 바탕 `panel_bg`, 글 `overlay0`, 키 accent 굵게,
    /// 모드 이름은 accent(크기 모드는 mauve) 바탕에 `panel_bg` 글자)
    fn mode_bar(&self) -> Option<Vec<(String, Style)>> {
        let p = &self.palette;
        let base = Style::default().fg(p.overlay0).bg(p.panel_bg);
        let key = Style::default().fg(p.accent).bg(p.panel_bg).add_modifier(Modifier::BOLD);
        let chip = Style::default()
            .fg(theme::contrast(p))
            .bg(if self.mode == Mode::Resize && self.error.is_none() { p.mauve } else { p.accent })
            .add_modifier(Modifier::BOLD);
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
            Mode::Resize => Some(vec![
                s(" RESIZE ", chip),
                s("  ", base),
                s("h/l", key),
                s(" 너비  ", base),
                s("j/k", key),
                s(" 높이  ", base),
                s("esc", key),
                s(" 끝", base),
            ]),
            // herdr copy 모드 막대(찾기 입력 줄은 `render_mode_bar`가 따로 그린다)
            Mode::Copy => {
                let selecting = self.copy.as_ref().is_some_and(|c| c.select.is_some());
                let status = self.copy.as_ref().map(|c| c.search.status()).unwrap_or_default();
                let searched = self.copy.as_ref().is_some_and(|c| !c.search.query.is_empty());
                let mut v = vec![
                    s(" COPY ", chip),
                    s(" ", base),
                    s("h/j/k/l w/b/e { }", key),
                    s(" 이동  ", base),
                    s("/ ?", key),
                    s(" 찾기  ", base),
                    s("n/N", key),
                    (format!(" 다시{status}  "), base),
                    s("v/space", key),
                    s(if selecting { " 고르는 중  " } else { " 고르기  " }, base),
                    s("y/enter", key),
                    s(" 복사  ", base),
                ];
                if selecting || searched {
                    v.extend([s("esc", key), s(" 지우기  q 끝", base)]);
                } else {
                    v.extend([s("q/esc", key), s(" 끝", base)]);
                }
                Some(v)
            }
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
            // 찾은 일치(herdr: 나머지 일치, 고른 글, 현재 일치 차례로 칠한다)
            let copying = self.copy.as_ref().filter(|c| c.pane == pane);
            let top = mouse::view_top(g.scroll);
            if let Some(c) = copying {
                copy::paint_matches(buf, r, top, &c.search, &self.palette, false);
            }
            // 고른 글(herdr 선택 색)
            if let Some(sel) = self.selection.as_ref().filter(|s| s.pane == pane) {
                let (bg, fg) = mouse::selection_colors(&self.palette);
                let top = mouse::view_top(g.scroll);
                for row in 0..r.h {
                    for col in 0..r.w {
                        if sel.covers(top, row, col)
                            && let Some(cell) = buf.cell_mut((r.x + col, r.y + row))
                        {
                            cell.set_bg(bg).set_fg(fg);
                        }
                    }
                }
            }
            if let Some(c) = copying {
                copy::paint_matches(buf, r, top, &c.search, &self.palette, true);
            }
            // copy 모드 커서(herdr: accent 바탕 굵게, 터미널 커서는 숨김)
            if let Some(c) = copying
                && let Some(row) = c.cursor.0.checked_sub(mouse::view_top(g.scroll)).filter(|row| *row < u32::from(r.h))
                && c.cursor.1 < r.w
                && let Some(cell) = buf.cell_mut((r.x + c.cursor.1, r.y + row as u16))
            {
                cell.set_bg(self.palette.accent)
                    .set_fg(theme::contrast(&self.palette))
                    .set_style(Style::default().add_modifier(Modifier::BOLD));
            }
            let (cr, cc, visible) = g.cursor;
            if focused && visible && copying.is_none() && cr < r.h && cc < r.w {
                return badge(buf, view, r, focused, &self.palette).or(Some((r.x + cc, r.y + cr)));
            }
        }
        badge(buf, view, r, focused, &self.palette);
        None
    }
}

/// pane 테두리 제목: 붙인 이름만(thread pane `project/id`, `tab create --label`). 이름을 주지 않아 agent가 채운 기본 이름
/// (명령 파일 이름, 없으면 `shell`)은 쓰지 않는다. herdr도 사람이 붙인 이름·agent 제목만 쓰고 셸 pane에는 제목이 없다.
fn border_title(v: &PaneView) -> Option<String> {
    let name = v.name.trim();
    let default = v.argv.first().map_or("shell", |a| a.rsplit(['/', '\\']).next().unwrap_or(a));
    (!name.is_empty() && name != default).then(|| name.to_string())
}

/// 사이드바 바탕과 오른쪽 구분선(herdr `render_sidebar_background`: `sidebar_bg`, `│`는 `surface_dim`)
fn sidebar_background(buf: &mut Buffer, r: Rect, p: &theme::Palette) {
    fill(buf, r, Style::default().bg(p.sidebar_bg));
    if r.w > 0 {
        for y in r.y..r.y + r.h {
            buf.set_string(r.x + r.w - 1, y, "│", Style::default().fg(p.surface_dim).bg(p.sidebar_bg));
        }
    }
}

/// 칸을 빈칸과 그 모양으로 채운다
fn fill(buf: &mut Buffer, r: Rect, st: Style) {
    for y in r.y..r.y + r.h {
        for x in r.x..r.x + r.w {
            buf.set_string(x, y, " ", st);
        }
    }
}

/// 알림 카드(herdr `render_notification_card`, 오른쪽 위): `overlay0` 테두리, `panel_bg` 바탕, `●` 와 굵은 제목
fn render_notice(buf: &mut Buffer, area: Rect, title: &str, p: &theme::Palette) {
    let w = (unicode_width(title) as u16 + 6).min(area.w);
    let h = 3.min(area.h);
    if w < 3 || h < 3 {
        return;
    }
    let r = Rect { x: area.x + area.w - w, y: area.y, w, h };
    let i = help::panel(buf, r, p.overlay0, p.panel_bg);
    let x = buf.set_stringn(i.x, i.y, "●", usize::from(u16::MAX), Style::default().fg(p.accent).bg(p.panel_bg)).0;
    let x = buf.set_stringn(x, i.y, " ", usize::from(u16::MAX), Style::default().bg(p.panel_bg)).0;
    buf.set_stringn(
        x,
        i.y,
        title,
        (i.x + i.w).saturating_sub(x) as usize,
        Style::default().fg(p.text).bg(p.panel_bg).add_modifier(Modifier::BOLD),
    );
}

/// 알림 카드 점 색(herdr `render_visible_notification`: 입력 필요 빨강, 끝남 파랑, 그 밖 accent)
fn card_dot(c: &cards::Card, p: &theme::Palette) -> Color {
    match c.kind {
        cards::Kind::NeedsInput => p.red,
        cards::Kind::Finished => p.blue,
        cards::Kind::Report => p.accent,
    }
}

/// 알림 카드(herdr `render_notification_card`, 기본 자리 오른쪽 아래): `overlay0` 테두리, `panel_bg` 바탕, 폭은 제목·본문 중
/// 넓은 것 + 6, 높이는 본문이 있으면 4. 첫 줄 `●`(종류 색)과 굵은 제목, 둘째 줄은 두 칸 들여 `overlay0` 본문
fn render_card(buf: &mut Buffer, area: Rect, c: &cards::Card, p: &theme::Palette) -> Option<Rect> {
    let w = (unicode_width(&c.title).max(unicode_width(&c.body)) + 6).min(usize::from(area.w)) as u16;
    let h = if c.body.is_empty() { 3 } else { 4 }.min(area.h);
    if w < 3 || h < 3 {
        return None;
    }
    let r = Rect { x: area.x + area.w - w, y: area.y + area.h - h, w, h };
    let i = help::panel(buf, r, p.overlay0, p.panel_bg);
    let end = usize::from(i.x + i.w);
    let x =
        buf.set_stringn(i.x, i.y, "●", end - usize::from(i.x), Style::default().fg(card_dot(c, p)).bg(p.panel_bg)).0;
    let x = buf.set_stringn(x, i.y, " ", end.saturating_sub(usize::from(x)), Style::default().bg(p.panel_bg)).0;
    buf.set_stringn(
        x,
        i.y,
        &c.title,
        end.saturating_sub(usize::from(x)),
        Style::default().fg(p.text).bg(p.panel_bg).add_modifier(Modifier::BOLD),
    );
    if !c.body.is_empty() && i.h > 1 {
        let x = i.x + 2.min(i.w);
        buf.set_stringn(
            x,
            i.y + 1,
            &c.body,
            end.saturating_sub(usize::from(x)),
            Style::default().fg(p.overlay0).bg(p.panel_bg),
        );
    }
    Some(r)
}

/// 좁은 화면의 알림(herdr `render_mobile_notification_banner`): 맨 아래 한 줄 ` ● 제목 · 본문`, `surface0` 바탕.
/// 제목은 짧게(「claude 끝남」·「claude 대기」)
fn render_card_banner(buf: &mut Buffer, area: Rect, c: &cards::Card, p: &theme::Palette) -> Option<Rect> {
    if area.w == 0 || area.h == 0 {
        return None;
    }
    let r = Rect { x: area.x, y: area.y + area.h - 1, w: area.w, h: 1 };
    let bg = p.surface0;
    fill(buf, r, Style::default().bg(bg));
    let title = match c.kind {
        cards::Kind::NeedsInput => {
            c.title.strip_suffix(" 입력 필요").map(|a| format!("{a} 대기")).unwrap_or(c.title.clone())
        }
        _ => c.title.clone(),
    };
    let end = usize::from(r.x + r.w);
    let mut x = r.x;
    for (text, st) in [
        (" ", Style::default().bg(bg)),
        ("●", Style::default().fg(card_dot(c, p)).bg(bg)),
        (" ", Style::default().bg(bg)),
        (title.as_str(), Style::default().fg(p.text).bg(bg).add_modifier(Modifier::BOLD)),
    ] {
        x = buf.set_stringn(x, r.y, text, end.saturating_sub(usize::from(x)), st).0;
    }
    if !c.body.is_empty() {
        let st = Style::default().fg(p.overlay0).bg(bg);
        x = buf.set_stringn(x, r.y, " · ", end.saturating_sub(usize::from(x)), st).0;
        buf.set_stringn(x, r.y, &c.body, end.saturating_sub(usize::from(x)), st);
    }
    Some(r)
}

fn is_asking(p: &PaneView) -> bool {
    p.state.is_running() && p.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question)
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
fn badge(
    buf: &mut Buffer,
    view: Option<&PaneView>,
    r: Rect,
    focused: bool,
    pal: &theme::Palette,
) -> Option<(u16, u16)> {
    let p = view?;
    let text = match (&p.state, p.connection) {
        (_, aaa_proto::Connection::Disconnected) => " 끊김·읽기 전용 ".to_string(),
        (SessionState::Running, _)
            if p.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Question) =>
        {
            " 질문 — aaa question ".to_string()
        }
        (SessionState::Running, _) => return None,
        // herdr에는 재개 키가 없다(복원 때 자동). 자동으로 되살리지 않는 pane(프로그램 pane·복원 실패)은 CLI로 다시 띄운다.
        // 복원 실패 까닭은 herdr-web-ui `RestoreErrorBadge`처럼 함께 보인다. 할 일(명령)을 앞에 두고, 긴 까닭은 pane 폭의
        // 2/3까지만 써서 줄인다(첫 줄 글을 덮지 않고, 잘려도 명령은 보이게)
        (SessionState::Interrupted, _) | (SessionState::Lost, _) => {
            let head = format!(
                " {} — aaa pane resume {} ",
                if p.state == SessionState::Lost { "유실" } else { "중단(재부팅)" },
                p.id.chars().take(8).collect::<String>()
            );
            match p.restore_error.as_deref() {
                None => head,
                Some(why) => {
                    let room = (r.w as usize * 2 / 3).saturating_sub(unicode_width(&head) + 2);
                    let mut short = String::new();
                    for c in why.chars() {
                        if unicode_width(&short) + unicode_width(&c.to_string()) + 1 > room {
                            short.push('…');
                            break;
                        }
                        short.push(c);
                    }
                    if room == 0 { head } else { format!("{head}· {short} ") }
                }
            }
        }
        (SessionState::Exited { code }, _) => format!(" 종료({}) ", code.map(|c| c.to_string()).unwrap_or("?".into())),
    };
    let w = unicode_width(&text).min(r.w as usize) as u16;
    // 색은 상태를 따른다(질문 빨강·끊김과 중단 호박·종료 회색). 포커스가 아니면 조금 흐리게.
    let bg = match (&p.state, p.connection) {
        (_, aaa_proto::Connection::Disconnected) => pal.peach,
        (SessionState::Running, _) => pal.red,
        (SessionState::Exited { .. }, _) => pal.overlay0,
        _ => pal.peach,
    };
    let mut style = Style::default().fg(theme::contrast(pal)).bg(bg).add_modifier(Modifier::BOLD);
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

    /// prefix 뒤 두벌식 자모는 설정이 켜졌을 때만 같은 자리 영문 키다(`[tui] prefix_jamo_keys`, 설정 기본은 켬.
    /// 설정을 읽기 전 App은 끈 채다)
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

    /// 중단 표시: 다시 띄우는 명령이 앞이고, 긴 복원 까닭은 pane 폭 2/3 안에서 줄인다(첫 줄 글을 덮지 않는다)
    #[test]
    fn interrupted_badge_keeps_the_command_and_the_first_line() {
        let mut p = pane_view("abcdef1234", "m1", "job");
        p.state = SessionState::Interrupted;
        p.restore_error = Some("재부팅 뒤 다시 실행하지 않았다(배포 같은 명령이 재부팅마다 다시 돌지 않게). 다시 열면 같은 명령으로 띄운다".into());
        let row = |p: &PaneView, w: u16| {
            let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, w, 1));
            badge(&mut buf, Some(p), r(0, 0, w, 1), true, &theme::Palette::catppuccin());
            let (mut line, mut x) = (String::new(), 0);
            while x < w {
                let sym = buf[(x, 0)].symbol();
                line.push_str(sym);
                x += (unicode_width(sym) as u16).max(1);
            }
            line
        };
        let wide = row(&p, 120);
        assert!(wide.contains("중단(재부팅) — aaa pane resume abcdef12 · 재부팅 뒤") && wide.contains('…'), "{wide:?}");
        assert!(wide.starts_with(&" ".repeat(40)), "왼쪽 1/3은 비운다: {wide:?}");
        // 좁으면 까닭을 빼고 명령만
        let narrow = row(&p, 50);
        assert!(
            narrow.trim_start().starts_with("중단(재부팅) — aaa pane resume abcdef12") && !narrow.contains("재부팅 뒤"),
            "{narrow:?}"
        );
        p.restore_error = None;
        assert!(row(&p, 120).trim_end().ends_with("aaa pane resume abcdef12"));
    }

    /// 보던 pane이 같은 자리의 새 pane으로 바뀌면(`pane resume`·재부팅 복원) 그 pane을 따라간다. 지워지면 첫 pane(QA Q19)
    #[test]
    fn focus_follows_replaced_pane() {
        let mut app = app_with_tabs(&["a"]);
        let root = &mut app.ws.tabs[0].root;
        aaa_proto::layout::split(root, "p0", "p1", aaa_proto::SplitDir::Horizontal);
        aaa_proto::layout::split(root, "p1", "p2", aaa_proto::SplitDir::Vertical);
        app.focus = Some("p1".into());
        let mut w = app.ws.clone();
        aaa_proto::layout::ws_replace(&mut w, "p1", "p9");
        app.set_ws(w);
        assert_eq!(app.focus.as_deref(), Some("p9"));
        let mut w = app.ws.clone();
        aaa_proto::layout::ws_remove(&mut w, "p9");
        app.set_ws(w);
        // 빠지면(모양이 바뀜) 차례상 다음 pane(herdr `TileLayout::close_focused`, `a_removed_tab_or_pane_moves_like_herdr`)
        assert_eq!(app.focus.as_deref(), Some("p2"));
    }

    fn app_with_tabs(names: &[&str]) -> App {
        let mut app = App::new("main".into(), "me".into());
        // 그리기 시험은 herdr 기본 폭 26칸 배치로 그린다(aaa 기본 36은 `shell_layout_like_herdr`가 본다)
        app.sidebar_width = 26;
        app.width = 90;
        app.height = 30;
        app.ws.tabs = names
            .iter()
            .enumerate()
            .map(|(i, n)| aaa_proto::Tab {
                id: format!("t{i}"),
                name: n.to_string(),
                root: aaa_proto::LayoutNode::Pane { pane: format!("p{i}") },
                zoomed: false,
            })
            .collect();
        app
    }

    /// 보던 탭·pane이 없어지면(닫기, 셸 exit) 앞 탭, 차례상 다음 pane(끝이면 앞 pane)으로 간다(herdr `Workspace::close_tab`·
    /// `remove_pane`·`TileLayout::close_focused`)
    #[test]
    fn a_removed_tab_or_pane_moves_like_herdr() {
        let mut app = app_with_tabs(&["a", "b", "c"]);
        let ws = |tabs: &[aaa_proto::Tab]| Workspace { name: "main".into(), tabs: tabs.to_vec() };
        let all = app.ws.tabs.clone();
        app.tab = 1;
        app.set_ws(ws(&[all[0].clone(), all[2].clone()]));
        assert_eq!(app.ws.tabs[app.tab].id, "t0", "가운데 탭이 빠지면 앞 탭");
        app.tab = 0;
        app.set_ws(ws(&[all[2].clone()]));
        assert_eq!(app.ws.tabs[app.tab].id, "t2", "맨 앞 탭이 빠지면 새 맨 앞 탭");

        let leaf = |p: &str| aaa_proto::LayoutNode::Pane { pane: p.into() };
        let split = |a, b| aaa_proto::LayoutNode::Split {
            dir: aaa_proto::SplitDir::Horizontal,
            ratio: 500,
            a: Box::new(a),
            b: Box::new(b),
        };
        let tab = |root| aaa_proto::Tab { id: "t".into(), name: String::new(), root, zoomed: false };
        app.set_ws(ws(&[tab(split(leaf("a"), split(leaf("b"), leaf("c"))))]));
        app.focus = Some("b".into());
        app.set_ws(ws(&[tab(split(leaf("a"), leaf("c")))]));
        assert_eq!(app.focus.as_deref(), Some("c"), "다음 pane");
        app.set_ws(ws(&[tab(leaf("a"))]));
        assert_eq!(app.focus.as_deref(), Some("a"), "끝 pane이 빠지면 앞 pane");
    }

    /// 줄의 `x`칸에 있는 글자(넓은 글자는 두 칸)
    fn at_col(line: &str, x: usize) -> Option<char> {
        let mut col = 0;
        for c in line.chars() {
            if col == x {
                return Some(c);
            }
            col += unicode_width(&c.to_string()).max(1);
            if col > x {
                return None;
            }
        }
        None
    }

    /// 사이드바(펼침 26칸) 오른쪽 부분: 맨 윗줄이면 탭 막대
    fn right_of_sidebar(line: &str) -> String {
        let mut col = 0;
        line.chars()
            .skip_while(|c| {
                let skip = col < 26;
                col += unicode_width(&c.to_string()).max(1);
                skip
            })
            .collect()
    }

    fn pane_view(id: &str, machine: &str, name: &str) -> PaneView {
        PaneView {
            id: id.into(),
            machine_id: machine.into(),
            machine: machine.into(),
            name: name.into(),
            argv: vec![],
            cwd: None,
            state: SessionState::Running,
            connection: aaa_proto::Connection::Connected,
            readonly: false,
            cols: 80,
            rows: 24,
            lease: None,
            role: None,
            agent: None,
            label: None,
            restore_error: None,
            replaced: Vec::new(),
        }
    }

    fn machine(id: &str, name: &str) -> MachineView {
        MachineView {
            id: id.into(),
            name: name.into(),
            local: true,
            connection: aaa_proto::Connection::Connected,
            os: None,
            boot_id: None,
            dev_mode: false,
            last_seen_ms: None,
            health: None,
            clock_skew_ms: None,
            problems: vec![],
        }
    }

    fn draw(app: &App) -> (Vec<String>, Option<(u16, u16)>) {
        let area = Rect { x: 0, y: 0, w: app.width, h: app.height };
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, area.w, area.h));
        let cursor = app.render(&mut buf, area);
        // 넓은 글자 뒤 칸은 건너뛴다(터미널에 보이는 대로)
        let lines = (0..area.h)
            .map(|y| {
                let (mut line, mut x) = (String::new(), 0);
                while x < area.w {
                    let sym = buf[(x, y)].symbol();
                    line.push_str(sym);
                    x += (unicode_width(sym) as u16).max(1);
                }
                line
            })
            .collect();
        (lines, cursor)
    }

    /// 탭 이름표: 붙인 이름 또는 자리 번호, 폭은 이름+4(최소 8), 지금 탭은 강조색 바탕(herdr `render_tab_bar`)
    #[test]
    fn tab_chips_like_herdr() {
        let mut app = app_with_tabs(&["", "빌드", ""]);
        app.tab = 1;
        let (lines, _) = draw(&app);
        let top = &lines[0];
        // 사이드바 머리 ` spaces`·구분선 뒤 탭 막대에 "   1    " "  빌드  " "   3    "(사이 한 칸)
        assert!(top.starts_with(" spaces"), "{top:?}");
        assert!(right_of_sidebar(top).starts_with("   1       빌드      3    "), "{top:?}");
        let area = Rect { x: 0, y: 0, w: 90, h: 30 };
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, 90, 30));
        app.render(&mut buf, area);
        let p = app.palette;
        let x = top.find("빌드").map(|b| unicode_width(&top[..b]) as u16).unwrap();
        assert_eq!((buf[(x, 0)].bg, buf[(x, 0)].fg), (p.accent, theme::contrast(&p)));
        assert!(buf[(x, 0)].modifier.contains(Modifier::BOLD), "붙인 이름의 지금 탭은 굵게");
        assert_eq!(buf[(x - 2, 0)].bg, p.accent, "이름표는 이름 양옆 빈칸까지 칠한다");
        assert_eq!(buf[(x - 3, 0)].bg, p.panel_bg, "이름표 사이 한 칸은 탭 막대 바탕");
        assert_eq!((buf[(x - 4, 0)].bg, buf[(x - 4, 0)].fg), (p.surface0, p.overlay0), "자동 이름은 흐리게");
        assert_eq!(buf[(89, 0)].bg, p.panel_bg, "탭 막대는 줄 끝까지 panel_bg");
        // 마우스를 켜면(기본) 이름표 뒤에 ` + `(herdr `NEW_TAB_WIDTH`), 그린 자리는 마우스가 맞힌다
        assert!(right_of_sidebar(top).starts_with("   1       빌드      3      + "), "{top:?}");
        let hits = app.hits.borrow().clone();
        assert_eq!(
            hits.tabs.iter().map(|(r, i)| (r.x, r.w, *i)).collect::<Vec<_>>(),
            [(26, 8, 0), (35, 8, 1), (44, 8, 2)]
        );
        assert_eq!(hits.new_tab, Some(Rect { x: 53, y: 0, w: 3, h: 1 }));
        assert_eq!(hits.toggle, Some(Rect { x: 24, y: 29, w: 1, h: 1 }));
        assert_eq!(hits.divider, Some(Rect { x: 25, y: 0, w: 1, h: 30 }));
        app.mouse_capture = false;
        let (lines, _) = draw(&app);
        assert!(!right_of_sidebar(&lines[0]).contains('+'), "마우스를 끄면 단추가 없다(herdr)");
    }

    /// 새 탭 이름 창(herdr `render_rename_overlay`): 가운데 56×7 테두리 창, 제목, 입력 칸(커서는 글 뒤), 단추 줄
    #[test]
    fn rename_overlay_like_herdr() {
        let mut app = app_with_tabs(&[""]);
        app.overlay = Some(Overlay::Rename(prompt::Rename {
            title: "새 탭",
            input: editor::Editor::new("2", true),
            target: prompt::RenameTarget::NewTab { default: "2".into() },
        }));
        let (lines, cursor) = draw(&app);
        let shown = lines.join("\n");
        // 90×30 화면: 창은 x 17..73, y 11..18
        let top = lines.iter().position(|l| l.contains("┌")).expect(&shown);
        assert_eq!(top, 11, "{shown}");
        assert_eq!(lines[top].find('┌').map(|b| unicode_width(&lines[top][..b])), Some(17), "{shown}");
        assert_eq!(lines[top].chars().filter(|c| *c == '─').count(), 54, "{shown}");
        assert!(lines[top + 1].contains("│새 탭"), "{shown}");
        assert!(lines[top + 3].contains("│ 2 "), "{shown}");
        for b in ["↵ 저장", "^c 지우기", "esc 취소"] {
            assert!(lines[top + 4].contains(b), "{b}\n{shown}");
        }
        assert!(lines[top + 6].contains("└"), "{shown}");
        assert_eq!(cursor, Some((18 + 1 + 1, 14)));
        // 이름 창이 열린 동안은 입력기를 바꾸지 않는다(한글 이름), 단축키·확인 창은 바꾼다(herdr `wants_ascii_input`)
        assert!(!app.wants_ascii());
        app.overlay = Some(Overlay::Confirm(app.close_confirm(prompt::CloseTarget::Tab("t0".into()))));
        assert!(app.wants_ascii());
        let (lines, cursor) = draw(&app);
        let shown = lines.join("\n");
        assert!(lines.iter().any(|l| l.contains("│ 마지막 탭을 닫을까?")), "{shown}");
        assert!(lines.iter().any(|l| l.contains("main — pane 1개")), "{shown}");
        assert!(lines.iter().any(|l| l.contains("↵ 닫기") && l.contains("esc 취소")), "{shown}");
        assert_eq!(cursor, None);
    }

    /// 붙여넣기는 1.23 host에 맡겨 감싸게 하고(ESC도 거기서 뺀다), 그 전 host에는 전처럼 친 글이다
    #[test]
    fn pastes_go_to_the_host_to_wrap_and_old_hosts_get_typed_text() {
        assert_eq!(
            paste_request(23, "p".into(), "a\nb".into()),
            FrontRequest::PanePaste { pane: "p".into(), text: "a\nb".into() }
        );
        assert_eq!(
            paste_request(22, "p".into(), "a\nb".into()),
            FrontRequest::PaneInput { pane: "p".into(), data: B64(b"a\nb".to_vec()) }
        );
    }

    /// 탭 닫기·이름은 Front API 1.4부터: 옛 host에는 보내지 않고 알린다(모르는 요청은 연결을 끊는다)
    #[test]
    fn old_host_gets_no_tab_requests() {
        let mut app = app_with_tabs(&[""]);
        assert!(app.host_at_least(4) && app.host_at_least(10));
        app.host_minor = 9;
        assert!(app.host_at_least(4) && !app.host_at_least(10));
        assert!(app.error.as_ref().is_some_and(|(e, _)| e.contains("host를 새 aaa로 다시 띄워야")));
    }

    /// copy 모드 찾기 일치는 pane 크기가 바뀌면 지운다(찾은 글·방향은 남아 `n`으로 다시 찾는다). 같은 크기 장면은 그대로
    #[test]
    fn search_matches_clear_when_the_pane_resizes() {
        use aaa_proto::{SearchDirection, TextPoint, TextRange};
        let mut app = app_with_tabs(&["a"]);
        let frame = |cols: u16, rows: u16| GridFrame {
            pane: "p0".into(),
            cols,
            rows,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            full: true,
            lines: vec![],
            modes: PaneModes::default(),
            scroll: GridScroll::default(),
        };
        app.on_msg(FrontMsg::Event(FrontEvent::Grid(frame(40, 10))));
        let at = |row, col| TextPoint { row, col };
        let found = copy::Search {
            query: "ab".into(),
            direction: Some(SearchDirection::Forward),
            matches: vec![TextRange { start: at(3, 1), end: at(3, 2) }],
            total: 1,
            current: Some(0),
            current_global: Some(0),
        };
        app.copy = Some(copy::CopyMode {
            pane: "p0".into(),
            cursor: (3, 1),
            offset: 0,
            entry: 0,
            select: None,
            prompt: None,
            search: found.clone(),
        });
        app.on_msg(FrontMsg::Event(FrontEvent::Grid(frame(40, 10))));
        assert_eq!(app.copy.as_ref().unwrap().search, found, "같은 크기면 그대로");
        app.on_msg(FrontMsg::Event(FrontEvent::Grid(frame(60, 10))));
        let s = &app.copy.as_ref().unwrap().search;
        assert!(s.matches.is_empty() && s.current_global.is_none() && s.total == 0, "{s:?}");
        assert_eq!((s.query.as_str(), s.direction), ("ab", Some(SearchDirection::Forward)));
    }

    /// 폭만 바뀐 장면이 바뀐 줄만 가져와도(고치기 전 host) 나머지 줄은 남는다. 전체 장면은 모두 바꾼다(QA t-0337 Q23)
    #[test]
    fn width_only_frames_keep_unsent_rows() {
        let line =
            |row: u16, t: &str| aaa_proto::GridLine { row, runs: vec![GridRun { t: t.into(), fg: 0, bg: 0, a: 0 }] };
        let frame = |cols: u16, rows: u16, full: bool, lines: Vec<aaa_proto::GridLine>| GridFrame {
            pane: "p".into(),
            cols,
            rows,
            cursor_row: 0,
            cursor_col: 0,
            cursor_visible: true,
            full,
            lines,
            modes: PaneModes::default(),
            scroll: GridScroll::default(),
        };
        let texts =
            |g: &Grid| g.lines.iter().map(|l| l.iter().map(|r| r.t.as_str()).collect()).collect::<Vec<String>>();
        let mut g = Grid::default();
        g.apply(frame(40, 3, true, vec![line(0, "a"), line(1, "b"), line(2, "c")]));
        g.apply(frame(60, 3, false, vec![line(2, "C")]));
        assert_eq!(texts(&g), ["a", "b", "C"]);
        assert_eq!((g.cols, g.rows), (60, 3));
        g.apply(frame(60, 2, true, vec![line(0, "x")]));
        assert_eq!(texts(&g), ["x", ""]);
    }

    /// zoom(herdr `render_panes`·`tab_label`): 포커스 pane 하나가 칸 전체(상자 하나), 탭 이름표에 ` Z`.
    /// 방향 찾기는 나눈 칸 그대로라 zoom 중에도 이웃으로 포커스가 옮겨 가고 그 pane이 크게 보인다.
    #[test]
    fn zoom_shows_the_focused_pane_alone_like_herdr() {
        let mut app = app_with_tabs(&[""]);
        aaa_proto::layout::split(&mut app.ws.tabs[0].root, "p0", "p1", SplitDir::Horizontal);
        app.focus = Some("p1".into());
        app.ws.tabs[0].zoomed = true;
        let area = Rect { x: 0, y: 0, w: app.width, h: app.height };
        let body = app.body(area);
        let shown = app.shown(&app.ws.tabs[0], body);
        assert_eq!(shown.iter().map(|b| (b.pane.as_str(), b.rect)).collect::<Vec<_>>(), [("p1", body)]);
        let (lines, _) = draw(&app);
        assert!(right_of_sidebar(&lines[0]).contains("  1 Z  "), "{:?}", lines[0]);
        // pane이 둘인 탭이라 zoom이어도 상자 하나를 두른다(herdr `zoomed_pane_borders`)
        let row = right_of_sidebar(&lines[5]);
        assert!(row.starts_with('│') && row.ends_with('│') && row.matches('│').count() == 2, "{row:?}");
        app.move_focus(Dir::Left);
        assert_eq!(app.focus.as_deref(), Some("p0"));
        assert_eq!(app.shown(&app.ws.tabs[0], body)[0].pane, "p0");
        app.ws.tabs[0].zoomed = false;
        assert_eq!(app.shown(&app.ws.tabs[0], body).len(), 2);
    }

    /// 테두리 제목은 붙인 이름만: agent가 채운 기본 이름(명령 파일 이름·`shell`)이면 제목이 없다(herdr 셸 pane처럼)
    #[test]
    fn border_titles_only_for_given_names() {
        let named = |name: &str, argv: &[&str]| {
            let mut v = pane_view("p", "m", name);
            v.argv = argv.iter().map(|a| a.to_string()).collect();
            border_title(&v)
        };
        assert_eq!(named("sh", &["/bin/sh"]), None);
        assert_eq!(named("shell", &[]), None);
        assert_eq!(named("demo/t-0001", &["claude"]), Some("demo/t-0001".into()));
        assert_eq!(named(" build ", &["/bin/zsh"]), Some("build".into()));
        // 나눈 탭에서 이름 붙은 pane만 위 테두리에 ` 이름 `
        let mut app = app_with_tabs(&[""]);
        aaa_proto::layout::split(&mut app.ws.tabs[0].root, "p0", "p1", SplitDir::Horizontal);
        for (id, name) in [("p0", "빌드 서버"), ("p1", "sh")] {
            let mut v = pane_view(id, "m1", name);
            v.argv = vec!["/bin/sh".into()];
            app.panes.insert(id.into(), v);
        }
        app.focus = Some("p0".into());
        let (lines, _) = draw(&app);
        let top = right_of_sidebar(&lines[1]);
        assert!(top.starts_with("┌ 빌드 서버 ─") && top.contains("┐┌──") && !top.contains(" sh "), "{top:?}");
    }

    /// 크기 모드 막대(herdr `render_mode_bar`): mauve 바탕 ` RESIZE `와 키 안내
    #[test]
    fn resize_mode_bar_like_herdr() {
        let mut app = app_with_tabs(&[""]);
        app.mode = Mode::Resize;
        assert!(app.wants_ascii(), "크기 모드 동안도 입력기를 영문으로(herdr처럼 모드 전부)");
        let area = Rect { x: 0, y: 0, w: app.width, h: app.height };
        let mut buf = Buffer::empty(ratatui::layout::Rect::new(0, 0, area.w, area.h));
        app.render(&mut buf, area);
        let (lines, _) = draw(&app);
        let bar = right_of_sidebar(lines.last().unwrap());
        assert!(bar.starts_with(" RESIZE   h/l 너비  j/k 높이  esc 끝"), "{bar:?}");
        assert_eq!(buf[(27, area.h - 1)].bg, app.palette.mauve);
        assert_eq!(buf[(89, area.h - 1)].bg, app.palette.panel_bg, "막대는 pane 폭 끝까지");
        assert_eq!(at_col(lines.last().unwrap(), 25), Some('│'), "사이드바는 막대 밑에 그대로");
    }

    /// 화면 배치(herdr `ClientShellConfig::layout`): 펼친 사이드바(aaa 기본 36칸) 온 높이, 그 오른쪽 위 탭 막대 한 줄, 나머지
    /// pane. 접으면 compact 4칸 또는 없음, 64칸 이하는 두 줄 머리, 폭은 `[tui] sidebar_min_width`~`sidebar_max_width`(기본
    /// 18~72)로 묶는다.
    #[test]
    fn shell_layout_like_herdr() {
        let mut app = App::new("main".into(), "me".into());
        let area = r(0, 0, 120, 30);
        let none = |a: Rect| Rect { w: 0, h: 0, ..a };
        assert_eq!(
            app.shell(area),
            Shell { sidebar: r(0, 0, 36, 30), tab_bar: r(36, 0, 84, 1), header: none(area), panes: r(36, 1, 84, 29) }
        );
        app.toggle_list();
        assert_eq!(app.shell(area).sidebar, r(0, 0, 4, 30));
        assert_eq!(app.shell(area).panes, r(4, 1, 116, 29));
        let mut cfg = aaa_config::TuiSection {
            sidebar_collapsed_mode: aaa_config::SidebarCollapsedMode::Hidden,
            ..Default::default()
        };
        app.apply_config(&cfg);
        assert!(app.collapsed(), "다시 읽어도 사용자가 접은 그대로");
        assert_eq!(app.shell(area).sidebar.w, 0);
        assert_eq!(app.shell(area).panes, r(0, 1, 120, 29));
        app.toggle_list();
        cfg.sidebar_width = 90;
        app.apply_config(&cfg);
        assert_eq!(app.shell(area).sidebar.w, 72);
        cfg.sidebar_width = 5;
        app.apply_config(&cfg);
        assert_eq!(app.shell(area).sidebar.w, 18);
        // herdr처럼 한도는 설정으로 바꾼다. 거꾸로면 알리고 기본 한도
        cfg.sidebar_width = 100;
        cfg.sidebar_max_width = 100;
        app.apply_config(&cfg);
        assert_eq!(app.shell(area).sidebar.w, 100);
        assert_eq!(app.shell(r(0, 0, 90, 30)).sidebar.w, 89, "pane 칸 한 칸은 남긴다(herdr cols − 1)");
        cfg.sidebar_min_width = 120;
        app.apply_config(&cfg);
        assert_eq!(app.shell(area).sidebar.w, 72);
        assert!(app.error.as_ref().is_some_and(|e| e.0.contains("sidebar_min_width")), "{:?}", app.error);
        // 64칸 이하: 머리 두 줄, 사이드바·탭 막대 없음
        let narrow = r(0, 0, 64, 20);
        assert_eq!(app.shell(narrow).header, r(0, 0, 64, 2));
        assert_eq!(app.shell(narrow).panes, r(0, 2, 64, 18));
        assert_eq!((app.shell(narrow).sidebar.w, app.shell(narrow).tab_bar.h), (0, 0));
        // 시작할 때 접기
        let mut fresh = App::new("main".into(), "me".into());
        fresh.apply_config(&aaa_config::TuiSection { sidebar_start_collapsed: true, ..Default::default() });
        assert_eq!(fresh.shell(area).sidebar.w, 4);
    }

    /// 사용자가 끌어 바꾼 폭은 설정을 다시 읽어도(설정 창의 테마 바꾸기도 이 길) 그대로다(herdr `reload_client_config`
    /// `sidebar_width_manual`). 끌기를 마치면 host 공유 설정에 남기고, 경계를 두 번 누르면 설정 폭으로 돌아가 남긴 것을 지운다.
    /// 다시 붙으면 host가 기억한 배치로 시작한다.
    #[test]
    fn dragged_sidebar_width_survives_a_theme_change_and_is_remembered() {
        let mut app = App::new("main".into(), "me".into());
        let area = r(0, 0, 160, 30);
        app.apply_config(&aaa_config::TuiSection::default());
        app.drag_sidebar_to(59, 0);
        assert_eq!(app.shell(area).sidebar.w, 60);
        assert!(app.chrome_unsaved.is_empty(), "끄는 동안은 남기지 않는다");
        app.sidebar_drag_done();
        assert_eq!(app.chrome_unsaved.get("tuiSidebarWidth"), Some(&serde_json::json!(60)));
        // 설정 창에서 테마를 바꾸면 host.toml [tui] theme만 쓰고 다시 읽는다
        app.apply_config(&aaa_config::TuiSection { theme: "nord".into(), ..Default::default() });
        assert_eq!((app.shell(area).sidebar.w, app.theme_name.as_str()), (60, "nord"));
        // 설정 폭을 바꿔도 끌어 둔 폭이 앞선다(herdr)
        app.apply_config(&aaa_config::TuiSection { sidebar_width: 30, ..Default::default() });
        assert_eq!(app.shell(area).sidebar.w, 60);
        app.reset_sidebar_width();
        assert_eq!(app.shell(area).sidebar.w, 30);
        assert_eq!(app.chrome_unsaved.get("tuiSidebarWidth"), Some(&serde_json::Value::Null));
        // 접기도 남긴다
        app.toggle_list();
        assert_eq!(app.chrome_unsaved.get("tuiSidebarCollapsed"), Some(&serde_json::json!(true)));

        // 다시 붙음: 설정을 읽고 host가 기억한 배치를 쓴다
        let mut again = App::new("main".into(), "me".into());
        again.apply_config(&aaa_config::TuiSection::default());
        let doc = |v: serde_json::Value| aaa_proto::SharedPreferences {
            revision: 3,
            settings: serde_json::from_value(v).unwrap(),
            pins: None,
        };
        again.apply_chrome(&doc(
            serde_json::json!({ "theme": "dark", "tuiSidebarWidth": 60, "tuiSidebarCollapsed": false }),
        ));
        assert_eq!(again.shell(area).sidebar.w, 60);
        again.apply_config(&aaa_config::TuiSection { theme: "nord".into(), ..Default::default() });
        assert_eq!(again.shell(area).sidebar.w, 60, "기억한 폭도 테마 바꾸기에 남는다");
        // 되돌린 폭(null)은 설정 폭
        let mut reset = App::new("main".into(), "me".into());
        reset.apply_config(&aaa_config::TuiSection::default());
        reset.apply_chrome(&doc(serde_json::json!({ "tuiSidebarWidth": null })));
        assert_eq!(reset.shell(area).sidebar.w, 36);
        // 보기 권한 기기는 남기지 않는다(host가 거부한다)
        reset.reader = true;
        reset.drag_sidebar_to(49, 0);
        reset.sidebar_drag_done();
        assert!(reset.chrome_unsaved.is_empty());
    }

    /// 사이드바 모양(herdr `render_sidebar`·`render_collapsed_sidebar`): 위 ` spaces` 구역(빈 줄 뒤 workspace 줄), 30줄이면
    /// 15줄째에 구분선 `─`과 ` agents`·오른쪽 `projects`, 그 아래 빈 줄 뒤 목록, 오른쪽 `│`, 맨 아래 `«`.
    /// 접으면 번호와 상태 점, `»`. 탐색 모드면 접힌 채여도 펼친 목록을 pane 위에 띄운다.
    #[test]
    fn sidebar_like_herdr() {
        let mut app = app_with_tabs(&[""]);
        app.focus = Some("p0".into());
        app.panes.insert("p0".into(), pane_view("p0", "m", "build"));
        app.pane_order = vec!["p0".into()];
        app.machines = vec![machine("m", "mac-a")];
        let (lines, _) = draw(&app);
        let shown = lines.join("\n");
        assert!(lines[0].starts_with(" spaces "), "{shown}");
        assert!(lines[2].starts_with(" · main "), "{shown}");
        assert!(lines[14].starts_with(" new "), "{shown}");
        assert!(lines[15].starts_with(&"─".repeat(25)), "{shown}");
        assert!(lines[16].starts_with(" agents") && lines[16].contains("projects│"), "{shown}");
        assert!(lines[18].starts_with(" ▾ mac-a"), "{shown}");
        assert!(lines.iter().all(|l| at_col(l, 25) == Some('│')), "{shown}");
        assert!(lines.iter().any(|l| l.starts_with(" ▾ mac-a")), "{shown}");
        assert!(lines.iter().any(|l| l.starts_with(" · build")), "셸 pane은 unknown 점: {shown}");
        assert_eq!(lines[29].chars().nth(24), Some('«'), "{shown}");
        app.toggle_list();
        let (lines, _) = draw(&app);
        let shown = lines.join("\n");
        assert!(lines[0].starts_with("1 ·│"), "{shown}");
        assert_eq!(lines[29].chars().nth(1), Some('»'), "{shown}");
        assert!(!shown.contains("mac-a"), "{shown}");
        app.mode = Mode::Navigate;
        let (lines, _) = draw(&app);
        let shown = lines.join("\n");
        // 접힌 칸(4칸)과 탭 막대는 그대로, 그 오른쪽 아래 pane 칸에 펼친 목록(머리·`▾ 머신`·`«`), 모드 막대 위까지
        assert!(lines[0].starts_with("1 ·│   1   "), "{shown}");
        assert!(lines[1].starts_with("   │ spaces "), "{shown}");
        assert!(lines[3].starts_with("   │ · main "), "{shown}");
        assert!(lines.iter().any(|l| l.starts_with("   │ ▾ mac-a")), "탐색이면 띄운다: {shown}");
        assert_eq!(at_col(&lines[28], 28), Some('«'), "띄운 목록은 막대 위까지: {shown}");
        assert!(lines[29].starts_with(" » │ NAVIGATE  esc 돌아가기"), "접힌 칸은 그대로, 막대는 pane 폭: {shown}");
    }

    /// 새 탭·workspace를 여는 머신(herdr 활성 endpoint, 차이 목록 U8): 포커스 pane의 머신이고 머신 머리 줄로 고르면 그 머신
    /// (포커스가 그대로인 동안), 끊긴 머신은 고르지 않는다. 마우스를 받으면 spaces 맨 아랫줄 ` new`, 머신이 여럿이면 그 이름.
    #[test]
    fn new_workspace_machine_like_herdr_endpoint() {
        let mut app = app_with_tabs(&["", ""]);
        app.focus = Some("p0".into());
        app.panes.insert("p0".into(), pane_view("p0", "m", "build"));
        app.panes.insert("p1".into(), pane_view("p1", "m", "log"));
        let mut far = machine("n", "mac-b");
        far.local = false;
        app.machines = vec![machine("m", "mac-a"), far];
        let footer = |app: &App| draw(app).0[14].clone();
        assert_eq!(app.target_machine().as_deref(), Some("m"));
        assert!(footer(&app).starts_with(" new · mac-a "), "{}", footer(&app));
        app.pick_machine("n".into());
        assert_eq!(app.target_machine().as_deref(), Some("n"));
        assert!(footer(&app).starts_with(" new · mac-b "), "{}", footer(&app));
        assert!(footer(&app).contains("         menu│"), "{}", footer(&app));
        assert_eq!(app.hits.borrow().new_workspace, Some(Rect { x: 0, y: 14, w: 12, h: 1 }));
        assert_eq!(app.hits.borrow().global_menu, Some(Rect { x: 19, y: 14, w: 6, h: 1 }));
        // 포커스가 바뀌면 포커스 pane의 머신
        app.focus = Some("p1".into());
        assert_eq!(app.target_machine().as_deref(), Some("m"));
        // 끊긴 머신은 고르지 않는다
        app.machines[1].connection = aaa_proto::Connection::Disconnected;
        app.pick_machine("n".into());
        assert_eq!(app.target_machine().as_deref(), Some("m"));
        assert!(app.error.as_ref().is_some_and(|e| e.0.contains("mac-b에 붙어 있지 않다")));
        // 포커스가 없으면 로컬 agent, 머신이 하나면 이름 없이, 마우스를 받지 않으면 없다
        app.focus = None;
        assert!(footer(&app).starts_with(" new · mac-a "), "{}", footer(&app));
        app.machines.truncate(1);
        assert!(footer(&app).starts_with(" new    "), "{}", footer(&app));
        assert_eq!(app.hits.borrow().new_workspace, Some(Rect { x: 0, y: 14, w: 5, h: 1 }));
        app.mouse_capture = false;
        assert!(!footer(&app).contains("new"), "{}", footer(&app));
        assert_eq!((app.hits.borrow().new_workspace, app.hits.borrow().global_menu), (None, None));
    }

    /// 설정 창(herdr `render_settings_overlay`·`route_settings_key`): 머리 ` 설정`, 구역 이름표, 지금 테마 `✓`, 고른 줄 `▸`,
    /// 안내와 적용·닫기 단추. 테마 구역에서 고르면 바로 입혀 보고, 구역을 바꾸면 그 구역 지금 값, 닫으면 연 때의 테마로.
    /// (적용은 host.toml에 써서 harness `settings_like_herdr`가 실험실 설정 폴더로 본다)
    #[test]
    fn settings_like_herdr() {
        let mut app = app_with_tabs(&[""]);
        app.overlay = Some(Overlay::Settings(settings::Settings {
            section: settings::Section::Theme,
            selected: 0,
            original_theme: "catppuccin".into(),
        }));
        let (lines, _) = draw(&app);
        let shown = lines.join("\n");
        for want in [
            " 설정",
            " 테마   상태 표시 ",
            " ▸ catppuccin ✓",
            "   terminal",
            " ↑↓ 고르기  tab 구역",
            " ↵ 적용 ",
            " esc 닫기 ",
        ] {
            assert!(shown.contains(want), "{want:?}: {shown}");
        }
        assert_eq!(app.hits.borrow().settings.as_ref().map(|h| h.tabs.len()), Some(2));
        if let Some(Overlay::Settings(s)) = app.overlay.as_mut() {
            s.step(true);
            s.step(true);
        }
        app.settings_preview();
        assert_eq!((app.theme_name.as_str(), &app.palette), ("terminal", &theme::Palette::terminal()));
        let (lines, _) = draw(&app);
        let shown = lines.join("\n");
        assert!(shown.contains(" ▸ terminal") && shown.contains("   catppuccin ✓"), "{shown}");
        app.settings_section(settings::Section::Indicators);
        let (lines, _) = draw(&app);
        let shown = lines.join("\n");
        assert!(shown.contains(" ▸ 색 점  ● ● ● ○ ·") && shown.contains("   기호  × ◐ ✓ ○ ·"), "{shown}");
        // 미리 본 테마는 구역을 바꿔도 그대로, 닫으면 되돌린다
        assert_eq!(app.palette, theme::Palette::terminal());
        app.settings_cancel();
        assert!(app.overlay.is_none());
        assert_eq!((app.theme_name.as_str(), &app.palette), ("catppuccin", &theme::Palette::catppuccin()));
    }

    /// 새 workspace 답: 그 workspace로 가서 새 pane에 포커스(키 붙잡기도 풀림), 실패면 무엇이 실패했는지 알린다
    #[test]
    fn new_workspace_answer_enters_it() {
        let mut app = app_with_tabs(&[""]);
        app.focus = Some("p0".into());
        app.pending.insert("r1".into(), Pending::NewSpace);
        app.focus_wait = Some("r1".into());
        let tab = aaa_proto::Tab {
            id: "t9".into(),
            name: String::new(),
            root: aaa_proto::LayoutNode::Pane { pane: "p9".into() },
            zoomed: false,
        };
        let w = Workspace { name: "workspace".into(), tabs: vec![tab] };
        let ok = Outcome::Ok(aaa_proto::FrontResponse::Workspace(w));
        app.on_msg(FrontMsg::Res { id: "r1".into(), result: ok });
        assert_eq!((app.ws_name.as_str(), app.focus.as_deref()), ("workspace", Some("p9")));
        assert!(app.focus_wait.is_none() && app.spaces.contains_key("workspace"));
        app.pending.insert("r2".into(), Pending::NewSpace);
        let e = aaa_proto::ErrorBody::new(aaa_proto::ErrorCode::NotFound, "머신이 없다: mac-b");
        app.on_msg(FrontMsg::Res { id: "r2".into(), result: Outcome::Err(e) });
        assert_eq!(app.error.as_ref().map(|e| e.0.as_str()), Some("새 workspace를 만들지 못했다: 머신이 없다: mac-b"));
    }

    /// 모드 막대는 평소에 없고(pane 맨 아랫줄은 pane 것), 오류는 ERROR 막대, 알림은 오른쪽 위 카드(herdr)
    #[test]
    fn mode_bar_error_and_notice_like_herdr() {
        let mut app = app_with_tabs(&[""]);
        let (lines, _) = draw(&app);
        assert!(!lines.join("\n").contains("PREFIX"));
        assert_eq!(right_of_sidebar(lines.last().unwrap()).trim(), "");
        app.mode = Mode::Prefix;
        let (lines, _) = draw(&app);
        assert!(right_of_sidebar(lines.last().unwrap()).starts_with(" PREFIX  esc 취소  ctrl+b prefix 보내기"));
        app.mode = Mode::Terminal;
        app.apply_config(&aaa_config::TuiSection {
            theme: "없는색".into(), sidebar_width: 26, ..Default::default()
        });
        assert_eq!(app.palette, theme::Palette::catppuccin());
        let (lines, _) = draw(&app);
        assert!(
            right_of_sidebar(lines.last().unwrap()).starts_with(" ERROR  [tui] theme 「없는색」을 모른다"),
            "{:?}",
            lines.last()
        );
        app.apply_config(&aaa_config::TuiSection { theme: "terminal".into(), sidebar_width: 26, ..Default::default() });
        assert_eq!(app.palette, theme::Palette::terminal());
        app.set_message("탭을 닫았다");
        let (lines, _) = draw(&app);
        // 글 폭 11 + 6 = 17칸, 오른쪽 위 세 줄
        assert!(lines[0].ends_with(&format!("┌{}┐", "─".repeat(15))), "{:?}", lines[0]);
        assert!(lines[1].ends_with("│● 탭을 닫았다  │"), "{:?}", lines[1]);
    }

    /// 좁은 화면 머리(herdr `render_mobile_header`): 상태 점·workspace·탭, 둘째 줄 agent 요약, 오른쪽 `목록` 단추
    #[test]
    fn mobile_header_like_herdr() {
        let mut app = app_with_tabs(&["", "로그"]);
        app.width = 60;
        app.height = 20;
        app.tab = 1;
        let (lines, _) = draw(&app);
        assert!(lines[0].starts_with(" · main"), "{lines:?}");
        assert!(lines[0].contains("탭 로그 · 2/2"), "{lines:?}");
        assert!(lines[0].ends_with("│         "), "{lines:?}");
        assert!(lines[1].starts_with(" agent 없음"), "{lines:?}");
        assert!(lines[1].ends_with("│  목록   "), "{lines:?}");
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
