//! backend 설정(TOML). 기획서 §7.1~7.2.
//!
//! - `host.toml`(host 머신), `agent.toml`(모든 agent 머신). 같은 폴더의 `*.local.toml`이 머신별로 덮어쓴다.
//! - 맨 위 `schema = 1`. 모르는 미래 schema는 명시 오류다.
//! - 잘못된 값은 `파일:줄:열`과 이유를 보이고 시작을 거부한다. 모르는 key는 경고한다.
//! - 사용자 파일을 자동으로 다시 쓰지 않는다. 사람이 터미널에서 내린 명령(`aaa profile`·`aaa safety`)만 toml_edit로 그
//!   key만 고친다(주석·차례는 그대로).

pub mod paths;
pub mod profile_edit;
pub mod safety;
pub mod tui_edit;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// 이 판이 쓰는 schema
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// 설정 진단 한 건. 위치를 모르면 줄·열은 0이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    pub severity: Severity,
    pub file: PathBuf,
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl std::fmt::Display for Diag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sev = match self.severity {
            Severity::Error => "오류",
            Severity::Warning => "경고",
        };
        if self.line > 0 {
            write!(f, "{}:{}:{}: {sev}: {}", self.file.display(), self.line, self.col, self.message)
        } else {
            write!(f, "{}: {sev}: {}", self.file.display(), self.message)
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("설정 오류 {}건", .0.iter().filter(|d| d.severity == Severity::Error).count())]
pub struct ConfigErrors(pub Vec<Diag>);

impl ConfigErrors {
    pub fn render(&self) -> String {
        self.0.iter().map(|d| d.to_string()).collect::<Vec<_>>().join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct Loaded<T> {
    pub config: T,
    pub warnings: Vec<Diag>,
    /// 실제로 읽은 파일(없으면 기본값만 쓴 것)
    pub files: Vec<PathBuf>,
}

// ───────────────────────── host.toml ─────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostConfig {
    pub schema: u32,
    pub host: HostSection,
    pub web: WebSection,
    pub local_agent: LocalAgentSection,
    pub tui: TuiSection,
    pub terminal: TerminalSection,
    pub projects: ProjectsSection,
    pub notify: NotifySection,
    pub session: SessionSection,
    /// profile 정의의 정본(참고 herdr-projects config.toml).
    pub profiles: std::collections::BTreeMap<String, Profile>,
    /// `[safety.default]`(모든 프로젝트)·`[safety."<프로젝트>"]`(herdr-projects 0.2.35 `[safety]`, plan §8.163). host는 쓸
    /// 때마다 [`safety::read`]로 다시 읽는다. 여기서는 시작 때 값이 맞는지만 본다.
    pub safety: std::collections::BTreeMap<String, safety::SafetyLayer>,
    pub defaults: DefaultsSection,
    pub update: UpdateSection,
}

/// 실행 파일 바꾸기(`aaa update`, plan §11·§8.180). 그 Mac의 host.toml·agent.toml 어느 쪽에 둬도 되고 둘 다 적으면 둘 다
/// 지킨다(agent만 도는 Mac에는 host.toml이 없다). `aaa update`가 쓸 때 읽는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateSection {
    /// 고정한 version(`x.y.z`): 새 실행 파일이 이 version이어야 바꾼다. 비면 고정하지 않는다(major는 고정해야만 바뀐다)
    pub version: String,
    /// 보관하는 옛 실행 파일 수(`aaa update rollback`이 고른다, 1~100)
    pub keep: u32,
}

impl Default for UpdateSection {
    fn default() -> Self {
        UpdateSection { version: String::new(), keep: 3 }
    }
}

/// `x.y.z`(숫자 셋)
pub fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut it = v
        .split('.')
        .map(|p| if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) { p.parse().ok() } else { None });
    let parsed = (it.next()??, it.next()??, it.next()??);
    it.next().is_none().then_some(parsed)
}

fn validate_update(u: &UpdateSection, out: &mut Vec<Invalid>) {
    if !u.version.is_empty() && parse_version(&u.version).is_none() {
        out.push(Invalid {
            path: key(&["update", "version"]),
            message: format!("x.y.z(숫자 셋) 형식이어야 한다: {:?}", u.version),
        });
    }
    if !(1..=100).contains(&u.keep) {
        out.push(Invalid { path: key(&["update", "keep"]), message: "1~100이어야 한다".into() });
    }
}

/// 정해지지 않았을 때 쓰는 값(herdr-projects `[defaults]`). host는 쓸 때마다 다시 읽는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct DefaultsSection {
    /// thread start에 profile이 없고 project.toml `thread_profile`도 없을 때 쓰는 profile(`aaa profile default threads`)
    pub thread_profile: String,
    /// `aaa project open`에 profile이 없고 project.toml `coordinator_profile`도 없을 때 쓰는 profile
    /// (`aaa profile default coordinator`)
    pub coordinator_profile: String,
}

/// 새 pane(새 탭·나누기). 이름과 값은 herdr 0.9.5 `[terminal] new_cwd`를 따른다(plan §8.76).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalSection {
    /// 새 pane을 띄울 폴더: `"follow"`(원래 pane이 지금 있는 폴더, 같은 머신일 때), `"home"`(그 머신 기본 위치),
    /// 또는 절대 경로. 명령에 cwd를 주면 그것이 먼저다. herdr의 `"current"`(서버 프로세스 위치)는 원격 머신과 맞지 않아 받지 않는다.
    pub new_cwd: String,
}

impl Default for TerminalSection {
    fn default() -> Self {
        TerminalSection { new_cwd: "follow".into() }
    }
}

/// 재부팅 뒤 복원(herdr 0.9.5 `[session]`, src/config/model.rs `SessionConfig`). 이름·기본값이 herdr와 같다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionSection {
    /// 재부팅 뒤 Claude·Codex pane과 thread를 저장한 대화로 이어 띄운다. 기본 켬(herdr와 같고, 결정 8-5 「꺼짐」을
    /// 결정 20 D12가 바꿨다). 꺼도 셸 pane은 되살린다.
    pub resume_agents_on_restore: bool,
    /// 이어 띄우기 사이 간격(ms). 0이면 간격 없이.
    pub startup_per_agent_delay_ms: u32,
}

impl Default for SessionSection {
    fn default() -> Self {
        SessionSection { resume_agents_on_restore: true, startup_per_agent_delay_ms: 100 }
    }
}

/// 알림 webhook(§8.7). 봇 token 같은 비밀값이 주소에 들어갈 수 있으니 `host.local.toml`(커밋하지 않는 파일)에 둔다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotifySection {
    /// 보낼 주소(비우면 끈다). https만 받는다(시험용 loopback http 예외). 로그·오류에는 주소를 남기지 않는다.
    pub webhook: String,
    /// `json`(기본, 항목 목록과 글) 또는 `telegram`(Bot API sendMessage 본문)
    pub format: String,
    /// format이 telegram일 때 받을 chat
    pub telegram_chat_id: String,
    /// 알릴 inbox 항목 종류(`item_kinds`가 옛 이름을 바꿔 읽는다). `thread-state`는 사람 차례·새 보고·시작 실패만 보낸다
    pub kinds: Vec<String>,
    /// 이 시간(초) 동안 모아 프로젝트마다 한 번에 보낸다(질문이 몰려도 한 통)
    pub debounce_secs: u64,
    /// 웹에서 「알림 켜기」한 기기에 Web Push로도 보낸다(§8.26)
    pub web_push: bool,
    /// Web Push 구독 주소로 받는 push 서비스 호스트(그 이름이거나 그 아래 이름). https만. 시험용 loopback(127.0.0.1 등)은 적었을 때만 http도 받는다
    pub push_hosts: Vec<String>,
    /// VAPID `sub`(push 서비스가 문제 때 연락할 곳). 비우면 프로젝트 저장소 주소(herdr-web-ui와 같다). 공개 주소는 보내지 않는다
    pub push_subject: String,
}

impl NotifySection {
    /// 알릴 inbox 종류. 옛 thread 항목 종류(report·question·delivery·thread)는 `thread-state`로 읽는다: thread 항목을
    /// 묶음 전이 하나로 합쳤다(plan §8.85). 이미 쓴 host.toml이 알림을 잃지 않게 한다.
    pub fn item_kinds(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for k in &self.kinds {
            let k = match k.as_str() {
                "report" | "question" | "delivery" | "thread" => "thread-state",
                other => other,
            };
            if !out.iter().any(|x| x == k) {
                out.push(k.to_string());
            }
        }
        out
    }
}

impl Default for NotifySection {
    fn default() -> Self {
        NotifySection {
            webhook: String::new(),
            format: "json".into(),
            telegram_chat_id: String::new(),
            kinds: ["thread-state", "session", "outage", "health", "routine-approval", "pr"].map(String::from).to_vec(),
            debounce_secs: 15,
            web_push: true,
            push_hosts: ["fcm.googleapis.com", "push.apple.com", "push.services.mozilla.com", "notify.windows.com"]
                .map(String::from)
                .to_vec(),
            push_subject: String::new(),
        }
    }
}

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig {
            schema: SCHEMA,
            host: HostSection::default(),
            web: WebSection::default(),
            local_agent: LocalAgentSection::default(),
            tui: TuiSection::default(),
            terminal: TerminalSection::default(),
            projects: ProjectsSection::default(),
            notify: NotifySection::default(),
            session: SessionSection::default(),
            profiles: Default::default(),
            safety: Default::default(),
            defaults: DefaultsSection::default(),
            update: UpdateSection::default(),
        }
    }
}

/// 프로젝트 폴더(§7.1)와 thread 정책(§8)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectsSection {
    /// 프로젝트 폴더 root(비우면 `~/.aaa/projects`)
    pub root: String,
    /// thread에 쓸 수 있는 profile 이름(agent.toml이 정의하고 host가 허용한다). `"*"`는 모두. 옛 자리다: `[safety.*]
    /// thread_profiles`가 있으면 그것이 먼저다(`aaa profile allow threads`).
    pub allow_profiles: Vec<String>,
    /// thread 보고와 함께 host로 복사할 library 상한(MB). 넘는 파일은 agent에 남긴다.
    pub library_copy_mb: u64,
    /// brief에 넣을 memory(MEMORY.md + memory/*.md) 글자 수 상한
    pub brief_memory_chars: usize,
    /// host ticker 주기(초). 사건이 오면 주기를 기다리지 않고 바로 돈다.
    pub tick_secs: u64,
    /// coordinator가 이만큼(초) 입력 대기 상태로 머물러야 inbox 알림(nudge)을 보낸다(herdr-projects `NUDGE_IDLE_SECS`)
    pub nudge_idle_secs: u64,
    /// coordinator 입력창이 이만큼(초) 비어 있는 것을 두 번 보아야 nudge를 보낸다. 사람이 치는 글과 섞이지 않게 한다
    /// (herdr-projects `NUDGE_QUIET_SECS`).
    pub nudge_quiet_secs: u64,
    /// 머신이 이만큼(초) 끊겨 있으면 inbox에 outage 항목을 만든다. `gh`가 이만큼 계속 실패해도 한 번 알린다.
    pub outage_secs: u64,
    /// 열린 thread의 PR을 이만큼(초)마다 `gh`로 본다(herdr-projects `PR_INTERVAL_SECS`, §8.80). 보고가 새로 오면 바로 본다.
    pub pr_poll_secs: u64,
    /// brief 안내·메시지를 입력한 때부터 harness가 받았다고 알리기(hook이 없으면 Enter 뒤 화면 변화)까지 기다리는 시간
    /// (밀리초). 입력 상자를 그리는 harness는 이 안에서 친 글이 화면에 보이기를 기다린 뒤 Enter한다. 넘으면 전달
    /// 불명(unknown)이고 다시 보내지 않는다.
    pub accept_wait_ms: u64,
    /// interrupt 메시지: Esc 뒤 harness가 멈췄다고 알리기(턴 끝 hook이나 화면의 「Interrupted」)를 기다리는 상한(밀리초).
    /// 넘으면 그대로 보낸다.
    pub stop_wait_ms: u64,
    /// brief를 보낸 뒤 새 보고가 오기 전까지 thread를 작업 중으로 치는 시간(초). 방금 일을 받은 agent는 아직 쉼으로 읽혀서다
    /// (herdr-projects ticker는 안내를 보낸 tick을 작업 중으로 두고 다음 tick(`TICK` 15초)에 센다, plan §8.85). 시험에서 줄인다.
    pub delivered_working_secs: u64,
    /// project.toml `auto_resolve_days`의 하루 길이(초). 시험에서 줄인다.
    pub auto_resolve_day_secs: u64,
    /// PR이 병합된 뒤 새 보고 없이 쉬는 thread를 끝내기까지(초, herdr-projects `MERGE_GRACE_SECS`). 시험에서 줄인다.
    pub merge_grace_secs: u64,
    /// inbox에서 끝낸(done) 항목을 지우기까지 날짜
    pub inbox_done_days: u64,
    /// routine 명령을 실행해도 되는 프로젝트(`"*"`는 모두). 프로젝트 폴더는 agent가 고칠 수 있으므로 여기(host.toml)에 둔다.
    /// 옛 자리다: `[safety.*] routine_commands`가 있으면 그것이 먼저다(`aaa safety set`).
    pub routine_commands: Vec<String>,
}

impl Default for ProjectsSection {
    fn default() -> Self {
        ProjectsSection {
            root: String::new(),
            allow_profiles: vec!["*".into()],
            library_copy_mb: 200,
            brief_memory_chars: 32_000,
            tick_secs: 15,
            nudge_idle_secs: 60,
            nudge_quiet_secs: 10,
            outage_secs: 600,
            pr_poll_secs: 120,
            accept_wait_ms: 10_000,
            stop_wait_ms: 15_000,
            delivered_working_secs: 15,
            auto_resolve_day_secs: 86_400,
            merge_grace_secs: 600,
            inbox_done_days: 30,
            routine_commands: Vec::new(),
        }
    }
}

/// TUI front(`aaa attach`). host 머신에서 실행하므로 host.toml에 둔다. 키 이름과 기본값은 herdr 0.9.5 `[keys]`를 따른다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiSection {
    /// 명령 prefix 키. herdr 형식(`ctrl+b`, 여러 개면 배열 `["ctrl+b", "ctrl+a"]`). 예전 `C-b` 형식도 읽는다.
    pub prefix: KeyList,
    /// prefix·탐색·크기·copy 모드 동안 입력기가 한글 등(영문 아님)이면 영문 입력기로 바꾸고, 모드가 끝나면 되돌린다
    /// (macOS·Windows 한국어 IME, ssh 밖). herdr `[experimental] switch_ascii_input_source_in_prefix`와 같은 이름이다.
    /// herdr는 기본 끔이지만 사용자 요청이라 기본 켬이다(plan 결정 20 D12).
    pub switch_ascii_input_source_in_prefix: bool,
    /// 예전 이름(`prefix_ascii`). 적혀 있으면 위 값 대신 쓴다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix_ascii: Option<bool>,
    /// prefix 뒤 두벌식 자모(ㅊ 등)를 같은 자리 영문 키(c 등)로 읽는다(기본 켬, 결정 20 D12). 입력기를 바꾸기 전에 들어온 키·ssh·다른 OS용.
    pub prefix_jamo_keys: bool,
    /// 새 탭(`prefix c`)을 만들기 전에 이름을 묻는다(herdr `[ui] prompt_new_tab_name`, 기본 켬)
    pub prompt_new_tab_name: bool,
    /// 마지막 탭·마지막 pane을 닫기 전에 묻는다(herdr `[ui] confirm_close`, 기본 켬)
    pub confirm_close: bool,
    /// 왼쪽 목록 폭(구분선 포함, herdr `[ui] sidebar_width` 26, 18~36으로 묶는다)
    pub sidebar_width: u16,
    /// 접힌 채 시작(herdr `[ui] sidebar_start_collapsed`, 기본 펼침)
    pub sidebar_start_collapsed: bool,
    /// 접으면: `compact`는 번호와 상태 점만 든 4칸 열, `hidden`은 없앤다(herdr `[ui] sidebar_collapsed_mode`)
    pub sidebar_collapsed_mode: SidebarCollapsedMode,
    /// 이 폭(칸) 이하면 목록·탭 줄 대신 두 줄 머리(herdr `[ui] mobile_width_threshold` 64)
    pub mobile_width_threshold: u16,
    /// 색 테마 이름(herdr `[theme] name`, 기본 `catppuccin`). 모르는 이름이면 TUI가 오류 막대로 알리고 기본을 쓴다.
    pub theme: String,
    /// pane 테두리(herdr `[ui] pane_borders`): `auto`(기본)는 pane이 둘 이상인 탭만 pane마다 상자를 두르고,
    /// `always`는 하나여도, `off`는 테두리 없이 빈 칸 하나로 띄운다. 테두리만큼 PTY 크기가 준다.
    pub pane_borders: PaneBorders,
    /// TUI가 마우스를 받는다(herdr `[ui] mouse_capture`, 기본 켬): 탭·사이드바·pane 누르기, 휠, 끌어 고르기.
    /// 켜면 터미널 자체의 끌어 고르기 대신 TUI가 고르고 복사한다.
    pub mouse_capture: bool,
    /// 끌어 고른 글을 떼는 때 복사한다(herdr `[ui] copy_on_select`, 기본 켬). 끄면 고른 채 두고 ctrl+c·cmd+c로 복사한다.
    pub copy_on_select: bool,
    /// 보통 화면 pane에서 휠 한 칸에 넘기는 기록 줄 수(herdr `[ui] mouse_scroll_lines`, 기본 3, 1 이상)
    pub mouse_scroll_lines: u16,
    /// 보통 화면 pane의 오른쪽 1칸에 기록 scrollbar(herdr `[ui] pane_scrollbars`, 기본 켬). 그 칸만큼 PTY가 좁고,
    /// alt 화면(vim 등)에서는 칸을 돌려준다.
    pub pane_scrollbars: bool,
    /// 상태 표시(herdr `[ui] status_indicators`): `dots`(기본)는 색 점 `● ○ ·`, `symbols`는 상태마다 다른 기호 `× ◐ ✓ ○ ·`
    pub status_indicators: StatusIndicators,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaneBorders {
    #[default]
    Auto,
    Always,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatusIndicators {
    #[default]
    Dots,
    Symbols,
}

impl StatusIndicators {
    /// 설정 글(`[tui] status_indicators` 값)
    pub fn as_str(self) -> &'static str {
        match self {
            StatusIndicators::Dots => "dots",
            StatusIndicators::Symbols => "symbols",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarCollapsedMode {
    #[default]
    Compact,
    Hidden,
}

impl TuiSection {
    /// prefix 동안 영문 입력기로 바꾸나(예전 이름 `prefix_ascii`가 있으면 그 값)
    pub fn switch_ascii(&self) -> bool {
        self.prefix_ascii.unwrap_or(self.switch_ascii_input_source_in_prefix)
    }
}

impl Default for TuiSection {
    fn default() -> Self {
        TuiSection {
            prefix: KeyList::One("ctrl+b".into()),
            switch_ascii_input_source_in_prefix: true,
            prefix_ascii: None,
            prefix_jamo_keys: true,
            prompt_new_tab_name: true,
            confirm_close: true,
            sidebar_width: 26,
            sidebar_start_collapsed: false,
            sidebar_collapsed_mode: SidebarCollapsedMode::Compact,
            mobile_width_threshold: 64,
            theme: "catppuccin".into(),
            pane_borders: PaneBorders::Auto,
            mouse_capture: true,
            copy_on_select: true,
            mouse_scroll_lines: 3,
            pane_scrollbars: true,
            status_indicators: StatusIndicators::Dots,
        }
    }
}

/// 어느 OS 머신의 절대 경로인가(`/…`, `C:\…`·`C:/…`, `\\server\…`). 새 pane은 다른 OS 머신에 뜰 수 있다.
fn is_absolute_anywhere(p: &str) -> bool {
    let b = p.as_bytes();
    p.starts_with('/')
        || p.starts_with("\\\\")
        || (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/'))
}

/// 키 하나 또는 여러 개(herdr 설정의 키 값과 같다)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyList {
    One(String),
    Many(Vec<String>),
}

impl KeyList {
    pub fn values(&self) -> Vec<&str> {
        match self {
            KeyList::One(s) => vec![s.as_str()],
            KeyList::Many(v) => v.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostSection {
    /// Agent link(TCP+TLS) listen 주소. 빈 문자열이면 원격 agent를 받지 않는다.
    pub agent_listen: String,
}

impl Default for HostSection {
    fn default() -> Self {
        HostSection { agent_listen: "0.0.0.0:7430".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WebSection {
    /// host 웹 listen 주소. loopback만 허용한다(TLS는 tailscale serve가 맡는다). 빈 문자열이면 끈다.
    pub listen: String,
    /// `aaa web publish` 후보 HTTPS 포트(앞에서부터, 이미 쓰는 포트는 건너뛴다)
    pub publish_ports: Vec<u16>,
}

impl Default for WebSection {
    fn default() -> Self {
        WebSection { listen: "127.0.0.1:7431".into(), publish_ports: vec![443, 8443, 7443, 17443] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalAgentMode {
    /// 개발 모드: host가 로컬 agent backend를 분리된 자식으로 띄운다(결정 3 MVP 허용). doctor가 「개발 모드」로 표시한다.
    Child,
    /// 운영 모드: OS 감독자(LaunchAgent)가 띄운다. host는 띄우지 않는다.
    Launchd,
    /// 로컬 agent를 쓰지 않는다
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalAgentSection {
    pub mode: LocalAgentMode,
    /// 로컬 agent 머신 이름(기본: 이 머신 hostname)
    pub name: String,
}

impl Default for LocalAgentSection {
    fn default() -> Self {
        LocalAgentSection { mode: LocalAgentMode::Child, name: String::new() }
    }
}

// ───────────────────────── agent.toml ─────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentConfig {
    pub schema: u32,
    pub agent: AgentSection,
    pub reconnect: ReconnectSection,
    pub selfcheck: SelfcheckSection,
    pub health: HealthSection,
    /// 옛 agent 설정 호환용. 새 host는 쓰지 않으며 시작 때 host로 옮기라는 경고를 낸다.
    pub profiles: std::collections::BTreeMap<String, Profile>,
    pub update: UpdateSection,
    /// 머신별 실행 파일·환경만 덮어쓴다. model·effort·args는 host 정본을 따른다.
    pub harnesses: std::collections::BTreeMap<String, HarnessOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HarnessOverride {
    pub command: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            schema: SCHEMA,
            agent: AgentSection::default(),
            reconnect: ReconnectSection::default(),
            selfcheck: SelfcheckSection::default(),
            health: HealthSection::default(),
            profiles: Default::default(),
            update: UpdateSection::default(),
            harnesses: Default::default(),
        }
    }
}

/// agent harness 실행 방법. model·effort는 harness별 플래그로 바뀐다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    /// claude·codex·shell·custom
    #[serde(rename = "agent", alias = "harness")]
    pub harness: String,
    pub model: String,
    pub effort: String,
    /// 뒤에 붙일 인자(권한·daemon 금지 인자 등). resume에도 그대로 붙는다.
    pub args: Vec<String>,
    /// 더할 환경 변수. resume에도 그대로 쓴다.
    pub env: std::collections::BTreeMap<String, String>,
    /// harness 실행 파일(비우면 harness 이름). custom은 필수이고 앞부분 argv 전체다.
    pub command: Vec<String>,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AgentSection {
    /// 머신 이름(기본: hostname)
    pub name: String,
    /// host 주소 `<이름 또는 주소>:<포트>`. 같은 머신 host면 "local"(UDS). 이름은 OS resolver로 푼다.
    pub host: String,
    /// 새 세션 기본 작업 폴더(기본: HOME)
    pub default_cwd: String,
    /// thread worktree·작업 폴더를 만드는 root(기본: `~/aaa-work`)
    pub work_root: String,
    /// 웹 파일 보기(터미널의 파일 경로 링크)가 열 수 있는 폴더(§6.5 허용 폴더). 비우면 집·`work_root`·`default_cwd`·
    /// 임시 폴더다. 그 아래도 symlink를 따라간 실제 경로로 본다(plan §8.125)
    pub file_roots: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReconnectSection {
    /// 재연결 backoff 상한(초)
    pub max_backoff_s: u64,
}

impl Default for ReconnectSection {
    fn default() -> Self {
        ReconnectSection { max_backoff_s: 60 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SelfcheckSection {
    /// 시작 때 첫 실행 지연 표본 수(macOS). 0이면 하지 않는다.
    pub first_exec_samples: u32,
    /// 실제로 읽어 볼 키체인 generic password 항목의 service 이름(없으면 잠금 bit만 본다)
    pub keychain_service: String,
    /// 읽기·쓰기를 확인할 외장 볼륨 폴더(선택)
    pub volume_dir: String,
}

impl Default for SelfcheckSection {
    fn default() -> Self {
        SelfcheckSection { first_exec_samples: 5, keychain_service: String::new(), volume_dir: String::new() }
    }
}

/// 머신 건강 보고(§8.10)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthSection {
    /// 점검 주기(초)
    pub every_s: u64,
    /// 작업 root가 있는 디스크의 여유가 이보다 적으면 새 세션을 거부한다(GB, 0이면 보지 않는다)
    pub min_free_gb: u64,
    /// toolchain 점검 명령(이름 = argv). 실패하면 새 세션을 거부한다. 출력 첫 줄을 버전으로 보인다.
    pub toolchain: std::collections::BTreeMap<String, Vec<String>>,
}

impl Default for HealthSection {
    fn default() -> Self {
        HealthSection { every_s: 60, min_free_gb: 5, toolchain: Default::default() }
    }
}

// ───────────────────────── 읽기 ─────────────────────────

pub fn load_host(dir: &Path) -> Result<Loaded<HostConfig>, ConfigErrors> {
    load(dir, "host", validate_host)
}

pub fn load_agent(dir: &Path) -> Result<Loaded<AgentConfig>, ConfigErrors> {
    let mut loaded: Loaded<AgentConfig> = load(dir, "agent", validate_agent)?;
    if !loaded.config.profiles.is_empty() {
        loaded.warnings.push(diag(
            Severity::Warning,
            &dir.join("agent.toml"),
            0,
            0,
            "옛 agent.toml [profiles]가 있다: host.toml [profiles]로 옮기라. 새 host는 이 정의를 쓰지 않는다".into(),
        ));
    }
    Ok(loaded)
}

/// 검증 실패 한 건: 문제 key 경로와 이유
pub struct Invalid {
    pub path: Vec<String>,
    pub message: String,
}

fn key(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

fn validate_host(c: &HostConfig) -> Vec<Invalid> {
    let mut out = Vec::new();
    let cwd = &c.terminal.new_cwd;
    if !matches!(cwd.as_str(), "follow" | "home") && !is_absolute_anywhere(cwd) {
        out.push(Invalid {
            path: key(&["terminal", "new_cwd"]),
            message: format!("\"follow\", \"home\" 또는 절대 경로여야 한다: {cwd:?}"),
        });
    }
    if !c.host.agent_listen.is_empty() && c.host.agent_listen.parse::<SocketAddr>().is_err() {
        out.push(Invalid {
            path: key(&["host", "agent_listen"]),
            message: format!("주소:포트 형식이 아니다: {:?}", c.host.agent_listen),
        });
    }
    if !c.web.listen.is_empty() {
        match c.web.listen.parse::<SocketAddr>() {
            Ok(a) if !a.ip().is_loopback() => out.push(Invalid {
                path: key(&["web", "listen"]),
                message: format!("웹은 loopback에서만 연다(외부 공개는 `aaa web publish`): {a}"),
            }),
            Ok(_) => {}
            Err(_) => out.push(Invalid {
                path: key(&["web", "listen"]),
                message: format!("주소:포트 형식이 아니다: {:?}", c.web.listen),
            }),
        }
    }
    if c.web.publish_ports.is_empty() || c.web.publish_ports.contains(&0) {
        out.push(Invalid {
            path: key(&["web", "publish_ports"]), message: "1~65535 포트를 하나 이상 적는다".into()
        });
    }
    if c.projects.tick_secs == 0 {
        out.push(Invalid { path: key(&["projects", "tick_secs"]), message: "1초 이상이어야 한다".into() });
    }
    if c.projects.auto_resolve_day_secs == 0 {
        out.push(Invalid {
            path: key(&["projects", "auto_resolve_day_secs"]), message: "1초 이상이어야 한다".into()
        });
    }
    if c.projects.delivered_working_secs == 0 {
        out.push(Invalid {
            path: key(&["projects", "delivered_working_secs"]), message: "1초 이상이어야 한다".into()
        });
    }
    for (name, v) in [("accept_wait_ms", c.projects.accept_wait_ms), ("stop_wait_ms", c.projects.stop_wait_ms)] {
        if v == 0 {
            out.push(Invalid { path: key(&["projects", name]), message: "1밀리초 이상이어야 한다".into() });
        }
    }
    let n = &c.notify;
    if !n.webhook.is_empty() {
        let loopback =
            ["http://127.0.0.1:", "http://localhost:", "http://[::1]:"].iter().any(|p| n.webhook.starts_with(p));
        if !n.webhook.starts_with("https://") && !loopback {
            out.push(Invalid {
                path: key(&["notify", "webhook"]),
                message: "https 주소여야 한다(http는 이 머신 loopback 시험만)".into(),
            });
        }
    }
    for h in &n.push_hosts {
        if h.is_empty() || h.contains(['/', ':', ' ']) && !h.starts_with('[') {
            out.push(Invalid {
                path: key(&["notify", "push_hosts"]),
                message: format!("호스트 이름만 적는다(scheme·경로·port 없이): {h:?}"),
            });
        }
    }
    if !n.push_subject.is_empty() && !n.push_subject.starts_with("mailto:") && !n.push_subject.starts_with("https://") {
        out.push(Invalid {
            path: key(&["notify", "push_subject"]),
            message: "mailto: 또는 https:// 주소여야 한다".into(),
        });
    }
    match n.format.as_str() {
        "json" => {}
        "telegram" if !n.telegram_chat_id.is_empty() => {}
        "telegram" => out.push(Invalid {
            path: key(&["notify", "telegram_chat_id"]),
            message: "format = \"telegram\"이면 chat ID가 필요하다".into(),
        }),
        other => out.push(Invalid {
            path: key(&["notify", "format"]),
            message: format!("json 또는 telegram이어야 한다: {other:?}"),
        }),
    }
    for (table, l) in &c.safety {
        let mut bad = |k: &str, message: String| out.push(Invalid { path: key(&["safety", table, k]), message });
        if let Some(v) = l.start_threads.as_deref().filter(|v| !matches!(*v, "propose" | "auto")) {
            bad("start_threads", format!("\"propose\" 또는 \"auto\"여야 한다: {v:?}"));
        }
        if let Some(v) = l.trust_screens.as_deref().filter(|v| !matches!(*v, "coordinator" | "user")) {
            bad("trust_screens", format!("\"coordinator\" 또는 \"user\"여야 한다: {v:?}"));
        }
        for (key, list) in [("thread_profiles", &l.thread_profiles), ("coordinator_profiles", &l.coordinator_profiles)]
        {
            for name in list.iter().flatten() {
                if let Err(e) = profile_edit::validate_name(name) {
                    bad(key, e);
                }
            }
        }
    }
    for (name, d) in
        [("thread_profile", &c.defaults.thread_profile), ("coordinator_profile", &c.defaults.coordinator_profile)]
    {
        if !d.is_empty()
            && let Err(e) = profile_edit::validate_name(d)
        {
            out.push(Invalid { path: key(&["defaults", name]), message: e });
        }
    }
    for (name, profile) in &c.profiles {
        if let Err(message) = profile_edit::validate_profile(name, profile) {
            out.push(Invalid { path: key(&["profiles", name]), message });
        }
    }
    validate_update(&c.update, &mut out);
    out
}

fn validate_agent(c: &AgentConfig) -> Vec<Invalid> {
    let mut out = Vec::new();
    let h = &c.agent.host;
    if !h.is_empty() && h != "local" {
        let ok =
            h.rsplit_once(':').is_some_and(|(name, port)| !name.is_empty() && port.parse::<u16>().is_ok_and(|p| p > 0));
        if !ok {
            out.push(Invalid {
                path: key(&["agent", "host"]),
                message: format!("\"local\" 또는 <이름 또는 주소>:<포트> 형식이어야 한다: {h:?}"),
            });
        }
    }
    if !(1..=3600).contains(&c.reconnect.max_backoff_s) {
        out.push(Invalid { path: key(&["reconnect", "max_backoff_s"]), message: "1~3600초여야 한다".into() });
    }
    if c.selfcheck.first_exec_samples > 200 {
        out.push(Invalid { path: key(&["selfcheck", "first_exec_samples"]), message: "0~200이어야 한다".into() });
    }
    if !(1..=3600).contains(&c.health.every_s) {
        out.push(Invalid { path: key(&["health", "every_s"]), message: "1~3600초여야 한다".into() });
    }
    for (name, argv) in &c.health.toolchain {
        if argv.is_empty() || argv[0].is_empty() {
            out.push(Invalid {
                path: key(&["health", "toolchain", name]), message: "실행할 argv가 비었다".into()
            });
        }
    }
    for (name, h) in &c.harnesses {
        if !matches!(name.as_str(), "claude" | "codex" | "shell" | "custom") {
            out.push(Invalid { path: key(&["harnesses", name]), message: "모르는 harness다".into() });
        }
        if h.command.first().is_some_and(|v| v.is_empty()) {
            out.push(Invalid {
                path: key(&["harnesses", name, "command"]), message: "실행 파일이 비었다".into()
            });
        }
    }
    for (name, p) in &c.profiles {
        let leak = name.as_str();
        if let Err(message) = profile_edit::validate_name(name) {
            out.push(Invalid { path: key(&["profiles", leak]), message });
        }
        match p.harness.as_str() {
            "claude" | "codex" | "shell" => {}
            "custom" if !p.command.is_empty() => {}
            "custom" => out.push(Invalid {
                path: key(&["profiles", leak, "command"]),
                message: "custom harness는 command(실행할 argv)가 있어야 한다".into(),
            }),
            other => out.push(Invalid {
                path: key(&["profiles", leak, "harness"]),
                message: format!("claude·codex·shell·custom 중 하나여야 한다: {other:?}"),
            }),
        }
    }
    validate_update(&c.update, &mut out);
    out
}

struct Source {
    path: PathBuf,
    text: String,
}

fn load<T>(dir: &Path, stem: &str, validate: fn(&T) -> Vec<Invalid>) -> Result<Loaded<T>, ConfigErrors>
where
    T: DeserializeOwned + Default + Serialize,
{
    let mut sources = Vec::new();
    let mut diags = Vec::new();
    for name in [format!("{stem}.toml"), format!("{stem}.local.toml")] {
        let path = dir.join(&name);
        match std::fs::read_to_string(&path) {
            Ok(text) => sources.push(Source { path, text }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => diags.push(diag(Severity::Error, &path, 0, 0, format!("읽을 수 없다: {e}"))),
        }
    }

    // 1) 파일마다 따로 schema·형식·모르는 key를 본다(오류 위치를 파일 기준으로 보이기 위해).
    let mut merged = serde_json::to_value(T::default()).expect("기본 설정 직렬화");
    for src in &sources {
        if let Err(d) = check_schema(src) {
            diags.push(d);
            continue;
        }
        let de = match toml_edit::de::Deserializer::parse(&src.text) {
            Ok(de) => de,
            Err(e) => {
                diags.push(toml_error(src, &e.to_string(), e.span()));
                continue;
            }
        };
        let mut unknown = Vec::new();
        let checked: Result<T, toml_edit::de::Error> = serde_ignored::deserialize(de, |p| unknown.push(p.to_string()));
        let typed = checked.and_then(|_| toml_edit::de::from_str::<serde_json::Value>(&src.text));
        match typed {
            Ok(v) => merge(&mut merged, v),
            Err(e) => diags.push(toml_error(src, e.message(), e.span())),
        }
        for key in unknown {
            let parts: Vec<&str> = key.split('.').collect();
            let (line, col) = locate(&src.text, &parts).unwrap_or((0, 0));
            diags.push(diag(Severity::Warning, &src.path, line, col, format!("모르는 key `{key}`는 무시한다")));
        }
    }
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(ConfigErrors(diags));
    }

    // 2) 겹친 결과를 최종 타입으로 바꾸고 값의 의미를 검증한다.
    let config: T = match serde_json::from_value(merged) {
        Ok(c) => c,
        Err(e) => {
            let file = sources.last().map(|s| s.path.clone()).unwrap_or_else(|| dir.join(format!("{stem}.toml")));
            diags.push(diag(Severity::Error, &file, 0, 0, format!("겹친 설정이 잘못됐다: {e}")));
            return Err(ConfigErrors(diags));
        }
    };
    for inv in validate(&config) {
        // 값을 마지막으로 정한 파일(local 우선)에서 위치를 찾는다.
        let parts: Vec<&str> = inv.path.iter().map(String::as_str).collect();
        let found = sources.iter().rev().find_map(|s| locate(&s.text, &parts).map(|lc| (s, lc)));
        let (file, (line, col)) = match found {
            Some((s, lc)) => (s.path.clone(), lc),
            None => (dir.join(format!("{stem}.toml")), (0, 0)),
        };
        diags.push(diag(Severity::Error, &file, line, col, format!("`{}`: {}", inv.path.join("."), inv.message)));
    }
    if diags.iter().any(|d| d.severity == Severity::Error) {
        return Err(ConfigErrors(diags));
    }
    Ok(Loaded { config, warnings: diags, files: sources.into_iter().map(|s| s.path).collect() })
}

fn check_schema(src: &Source) -> Result<(), Diag> {
    let doc = toml_edit::Document::parse(src.text.as_str()).map_err(|e| toml_error(src, e.message(), e.span()))?;
    let item = doc.get("schema");
    let Some(item) = item else {
        return Err(diag(Severity::Error, &src.path, 1, 1, format!("맨 위에 `schema = {SCHEMA}`이 있어야 한다")));
    };
    let (line, col) = item.span().map(|s| line_col(&src.text, s.start)).unwrap_or((1, 1));
    match item.as_integer() {
        Some(v) if v == SCHEMA as i64 => Ok(()),
        Some(v) if v > SCHEMA as i64 => Err(diag(
            Severity::Error,
            &src.path,
            line,
            col,
            format!("schema {v}은 이 aaa({SCHEMA})보다 새롭다. aaa를 갱신한다"),
        )),
        Some(v) => {
            Err(diag(Severity::Error, &src.path, line, col, format!("schema {v}은 지원하지 않는다(지원: {SCHEMA})")))
        }
        None => Err(diag(Severity::Error, &src.path, line, col, "schema는 정수여야 한다".into())),
    }
}

fn toml_error(src: &Source, message: &str, span: Option<std::ops::Range<usize>>) -> Diag {
    let (line, col) = span.map(|s| line_col(&src.text, s.start)).unwrap_or((0, 0));
    diag(Severity::Error, &src.path, line, col, message.trim().to_string())
}

fn diag(severity: Severity, file: &Path, line: usize, col: usize, message: String) -> Diag {
    Diag { severity, file: file.to_path_buf(), line, col, message }
}

/// byte 위치 → 1부터 세는 줄·열(열은 글자 수)
fn line_col(text: &str, pos: usize) -> (usize, usize) {
    let pos = pos.min(text.len());
    let before = &text[..pos];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map(|l| l.chars().count()).unwrap_or(0) + 1;
    (line, col)
}

/// key 경로(`["web","listen"]`)의 값 위치. 값이 없으면 가장 가까운 상위 table 위치.
fn locate(text: &str, path: &[&str]) -> Option<(usize, usize)> {
    let doc = toml_edit::Document::parse(text).ok()?;
    let mut item = doc.as_item();
    let mut best = None;
    for key in path {
        let Some(next) = item.get(key) else { break };
        if let Some(span) = next.span() {
            best = Some(span.start);
        }
        item = next;
    }
    best.map(|p| line_col(text, p))
}

fn merge(base: &mut serde_json::Value, over: serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (slot, v) => *slot = v,
    }
}

/// `aaa config migrate`: 지금은 schema 1뿐이라 바꿀 것이 없다. 이전 schema가 생기면 여기서 백업 뒤 toml_edit로 바꾼다.
pub fn migrate_plan(dir: &Path, stem: &str) -> Result<String, ConfigErrors> {
    let path = dir.join(format!("{stem}.toml"));
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Ok(format!("{}: 파일이 없다(기본값 사용)", path.display())),
    };
    let src = Source { path: path.clone(), text };
    check_schema(&src).map_err(|d| ConfigErrors(vec![d]))?;
    Ok(format!("{}: 이미 최신 schema {SCHEMA}이다", path.display()))
}

/// 예시 설정 파일(주석 포함). `aaa config init`이 쓴다.
pub fn example(stem: &str) -> &'static str {
    match stem {
        "host" => include_str!("../examples/host.toml"),
        _ => include_str!("../examples/agent.toml"),
    }
}

#[cfg(test)]
mod tests;
