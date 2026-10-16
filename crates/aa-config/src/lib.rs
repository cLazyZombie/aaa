//! backend 설정(TOML). 기획서 §7.1~7.2.
//!
//! - `host.toml`(host 머신), `agent.toml`(모든 agent 머신). 같은 폴더의 `*.local.toml`이 머신별로 덮어쓴다.
//! - 맨 위 `schema = 1`. 모르는 미래 schema는 명시 오류다.
//! - 잘못된 값은 `파일:줄:열`과 이유를 보이고 시작을 거부한다. 모르는 key는 경고한다.
//! - 사용자 파일을 자동으로 다시 쓰지 않는다.

pub mod paths;

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
    pub projects: ProjectsSection,
}

impl Default for HostConfig {
    fn default() -> Self {
        HostConfig {
            schema: SCHEMA,
            host: HostSection::default(),
            web: WebSection::default(),
            local_agent: LocalAgentSection::default(),
            tui: TuiSection::default(),
            projects: ProjectsSection::default(),
        }
    }
}

/// 프로젝트 폴더(§7.1)와 thread 정책(§8)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectsSection {
    /// 프로젝트 폴더 root(비우면 `~/.aa/projects`)
    pub root: String,
    /// thread에 쓸 수 있는 profile 이름(agent.toml이 정의하고 host가 허용한다). `"*"`는 모두.
    pub allow_profiles: Vec<String>,
    /// thread 보고와 함께 host로 복사할 library 상한(MB). 넘는 파일은 agent에 남긴다.
    pub library_copy_mb: u64,
    /// brief에 넣을 memory(MEMORY.md + memory/*.md) 글자 수 상한
    pub brief_memory_chars: usize,
    /// host ticker 주기(초). 사건이 오면 주기를 기다리지 않고 바로 돈다.
    pub tick_secs: u64,
    /// coordinator가 이만큼(초) 입력 대기 상태로 머물러야 inbox 알림(nudge)을 보낸다
    pub nudge_idle_secs: u64,
    /// 머신이 이만큼(초) 끊겨 있으면 inbox에 outage 항목을 만든다
    pub outage_secs: u64,
    /// inbox에서 끝낸(done) 항목을 지우기까지 날짜
    pub inbox_done_days: u64,
    /// 머신 하나에서 동시에 도는(세션 실행 중) thread 한도. 0이면 제한하지 않는다.
    pub max_threads_per_machine: u32,
    /// routine 명령을 실행해도 되는 프로젝트(`"*"`는 모두). 프로젝트 폴더는 agent가 고칠 수 있으므로 여기(host.toml)에 둔다.
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
            nudge_idle_secs: 10,
            outage_secs: 600,
            inbox_done_days: 30,
            max_threads_per_machine: 6,
            routine_commands: Vec::new(),
        }
    }
}

/// TUI front(`aa attach`). host 머신에서 실행하므로 host.toml에 둔다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiSection {
    /// 명령 prefix 키. `C-<글자>` 형식(예: tmux 안에서 쓸 때는 C-a 대신 C-g 같은 것을 고른다)
    pub prefix: String,
}

impl Default for TuiSection {
    fn default() -> Self {
        TuiSection { prefix: "C-b".into() }
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
    /// `aa web publish` 후보 HTTPS 포트(앞에서부터, 이미 쓰는 포트는 건너뛴다)
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
    /// thread를 띄우는 profile(§8.2). host.toml `[projects] allow_profiles`가 허용한 것만 쓴다.
    pub profiles: std::collections::BTreeMap<String, Profile>,
}

impl Default for AgentConfig {
    fn default() -> Self {
        AgentConfig {
            schema: SCHEMA,
            agent: AgentSection::default(),
            reconnect: ReconnectSection::default(),
            selfcheck: SelfcheckSection::default(),
            profiles: Default::default(),
        }
    }
}

/// agent harness 실행 방법. model·effort는 harness별 플래그로 바뀐다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    /// claude·codex·shell·custom
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
    /// thread worktree·작업 폴더를 만드는 root(기본: `~/aa-work`)
    pub work_root: String,
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

// ───────────────────────── 읽기 ─────────────────────────

pub fn load_host(dir: &Path) -> Result<Loaded<HostConfig>, ConfigErrors> {
    load(dir, "host", validate_host)
}

pub fn load_agent(dir: &Path) -> Result<Loaded<AgentConfig>, ConfigErrors> {
    load(dir, "agent", validate_agent)
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
                message: format!("웹은 loopback에서만 연다(외부 공개는 `aa web publish`): {a}"),
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
    for (name, p) in &c.profiles {
        let leak = name.as_str();
        if !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
            out.push(Invalid {
                path: key(&["profiles", leak]), message: "profile 이름은 영문·숫자·-·_만 쓴다".into()
            });
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
            format!("schema {v}은 이 aa({SCHEMA})보다 새롭다. aa를 갱신한다"),
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

/// `aa config migrate`: 지금은 schema 1뿐이라 바꿀 것이 없다. 이전 schema가 생기면 여기서 백업 뒤 toml_edit로 바꾼다.
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

/// 예시 설정 파일(주석 포함). `aa config init`이 쓴다.
pub fn example(stem: &str) -> &'static str {
    match stem {
        "host" => include_str!("../examples/host.toml"),
        _ => include_str!("../examples/agent.toml"),
    }
}

#[cfg(test)]
mod tests;
