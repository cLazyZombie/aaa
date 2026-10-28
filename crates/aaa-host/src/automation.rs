//! 조건 → 동작의 파일 계약. 실행 상태·기록과 파일 형식은 분리한다.
mod convert;
mod deploy;
pub(crate) mod edit;
pub(crate) mod input;
pub(crate) mod model;
pub(crate) mod runtime;
mod usage;
/// coordinator 운영 규칙의 웹 보호 표지. 파일 내용·제목과 무관하게 이름으로 판정한다.
/// 다른 운영 자동화는 사용자 결정 없이 여기에 더하지 않는다. CLI·파일 편집은 그대로 허용한다.
pub(crate) fn web_read_only_reason(name: &str) -> Option<&'static str> {
    (name == "idle-audit").then_some("coordinator 운영에 필요한 시스템 자동화입니다. 웹에서는 읽기만 할 수 있습니다. 변경은 CLI나 파일 편집으로 합니다.")
}

/// 웹 요청에서만 호출한다. 원문 검사·조회는 허용하고 모든 규칙 변경 입구를 막는다.
pub(crate) fn check_web_mutation(
    host: &crate::Host,
    req: &aaa_proto::FrontRequest,
) -> Result<(), aaa_proto::ErrorBody> {
    use aaa_proto::FrontRequest as R;
    let name = match req {
        R::AutomationSave { name, .. }
        | R::AutomationDelete { name, .. }
        | R::AutomationRun { name, .. }
        | R::RoutineToggle { name, .. }
        | R::RoutineApprove { name, .. }
        | R::AutomationApprove { name, .. }
        | R::AutomationUnblock { name, .. } => Some(name.as_str()),
        R::AutomationRestore { id, .. } => edit::trash_id(id).map(|(name, _)| name),
        R::AutomationConvert { project, dry_run: false, .. } => {
            // 옛 시스템 규칙을 웹의 일괄 변환으로 바꾸는 것도 금지한다. 미리보기는 읽기다.
            if host
                .routines(project)?
                .iter()
                .any(|r| r.web_read_only_reason.is_some() && r.file.starts_with("routines/"))
            {
                Some("idle-audit")
            } else {
                None
            }
        }
        _ => None,
    };
    if let Some(reason) = name.and_then(web_read_only_reason) {
        return Err(crate::err(aaa_proto::ErrorCode::Forbidden, reason));
    }
    Ok(())
}

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct File {
    #[serde(default)]
    pub title: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub on: Condition,
    #[serde(rename = "do")]
    pub action: Action,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Condition {
    Schedule(Scheduled),
    Command(Command),
    Thread(Threads),
    Ci(Ci),
    Git(Git),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CatchUp {
    #[default]
    Once,
    Skip,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Scheduled {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_open_threads: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily: Option<String>,
    pub catch_up: CatchUp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Command {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_open_threads: Option<u32>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily: Option<String>,
    #[serde(default)]
    pub catch_up: CatchUp,
    pub command: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Threads {
    pub events: Vec<String>,
    #[serde(default)]
    pub repos: Vec<String>,
    /// 자동화가 띄운 thread의 사건도 받는다(plan t-0475 §3.5). 기본은 뺀다: 자동화 thread의 보고가 규칙을 깨워 다시 thread를 띄우는
    /// 고리를 막는다. 켜도 이 규칙이 띄운 thread의 사건은 늘 뺀다
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub include_automation: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ci {
    pub repo: String,
    pub branches: Vec<String>,
    #[serde(default)]
    pub workflows: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Git {
    pub repo: String,
    pub branches: Vec<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    Command(aaa_proto::AutomationCommand),
    Deploy,
    Inbox(Inbox),
    Message(Message),
    Start(Start),
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Inbox {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_file: Option<String>,
    pub notify: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Message {
    pub thread: String,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_file: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProfilePick {
    #[default]
    First,
    WeekLeft,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MachinePick {
    #[default]
    Idlest,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reuse {
    #[default]
    New,
    Open,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    #[serde(default)]
    /// new는 새 thread, open은 같은 규칙의 열린 thread에 이어 보냄(Claude·Codex).
    pub reuse: Reuse,
    #[serde(default)]
    /// open 전용. 기억을 보존하고 SessionStart(압축·재개 포함)에서 brief와 memory.md 재독을 안내한다.
    pub memory: bool,
    #[serde(default)]
    pub reports: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_idle: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    pub profiles: Vec<String>,
    /// 비우거나 생략하면 자동 머신: workspace 자동 배정을 허용한 머신 가운데 독점 예약·착지 중을 빼고 열린 thread÷비중으로 고른다.
    #[serde(default)]
    pub machines: Vec<String>,
    #[serde(default)]
    pub profile_pick: ProfilePick,
    #[serde(default)]
    pub machine_pick: MachinePick,
    #[serde(default)]
    pub wait: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "optional_duration_value")]
    pub wait_max: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_file: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Limits {
    /// 배포 제안: 최근 성공 커밋 또는 등록 head 뒤의 새 main 커밋(1~100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_commits: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "optional_duration_value")]
    pub min_gap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_per_day: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// do.start 규칙별 동시 한도. 생략·0은 1개(자동화 thread는 일반 실행 한도에서 빠진다).
    pub max_running: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_session_below: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_week_below: Option<f64>,
}

/// 제한 없음은 숫자0도 받되 양수 기간은 단위를 명시한다.
pub(crate) fn duration_value<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = serde_json::Value::deserialize(d)?;
    match value {
        serde_json::Value::String(text) => Ok(if text.trim() == "0" { "0m".into() } else { text }),
        serde_json::Value::Number(n) if n.as_u64() == Some(0) => Ok("0m".into()),
        _ => Err(serde::de::Error::custom("기간 문자열 또는 제한 없음(0)을 쓴다")),
    }
}
fn optional_duration_value<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    duration_value(d).map(Some)
}

/// 타입 오류도 의미 오류도 같은 `파일:줄:열 칸: 이유` 형식으로 낸다.
pub(crate) fn diagnostic(file: &str, text: &str, offset: usize, field: &str, message: &str) -> String {
    let mut end = offset.min(text.len());
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let prefix = &text[..end];
    let line = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("{file}:{line}:{column} {field}: {message}")
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(file: &str, text: &str) -> Result<T, String> {
    decode_fragment(file, text, text, 0)
}

pub(crate) fn decode_fragment<T: serde::de::DeserializeOwned>(
    file: &str,
    full: &str,
    fragment: &str,
    start: usize,
) -> Result<T, String> {
    let deserializer = toml::de::Deserializer::parse(fragment)
        .map_err(|e| diagnostic(file, full, start + e.span().map_or(0, |s| s.start), "TOML", e.message()))?;
    serde_path_to_error::deserialize(deserializer).map_err(|e| {
        diagnostic(
            file,
            full,
            start + e.inner().span().map_or(0, |s| s.start),
            &e.path().to_string(),
            e.inner().message(),
        )
    })
}

pub(crate) fn field_error(file: &str, text: &str, field: &str, message: &str) -> String {
    let offset = toml_edit::Document::parse(text)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|doc| {
            let mut item = doc.as_item();
            for segment in field.split('.') {
                let Some(next) = item.get(segment) else { break };
                item = next;
            }
            item.span().map(|s| s.start)
        })
        .unwrap_or(0);
    diagnostic(file, text, offset, field, message)
}

/// 긴 안내 글은 프로젝트 안의 일반 파일만 읽는다. 중간 폴더의 symlink도 거부한다.
pub(crate) fn project_text(root: &std::path::Path, relative: &str) -> Result<String, String> {
    use std::path::{Component, Path};
    let path = Path::new(relative);
    if relative.is_empty() || !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("프로젝트 폴더 안 상대 파일 경로여야 한다".into());
    }
    let mut at = root.to_path_buf();
    let components: Vec<_> = path.components().collect();
    for (index, component) in components.iter().enumerate() {
        at.push(component.as_os_str());
        let metadata = std::fs::symlink_metadata(&at)
            .map_err(|e| aaa_common::diagnostic::context(e, "글 파일을 읽지 못한다".to_string()))?;
        if metadata.file_type().is_symlink()
            || (index + 1 < components.len() && !metadata.is_dir())
            || (index + 1 == components.len() && !metadata.is_file())
        {
            return Err("프로젝트 안 일반 파일이어야 하며 symlink는 받지 않는다".into());
        }
        if index + 1 == components.len() && metadata.len() > 1024 * 1024 {
            return Err("글 파일은 1MiB 이하여야 한다".into());
        }
    }
    std::fs::read_to_string(at)
        .map_err(|e| aaa_common::diagnostic::context(e, "글 파일을 읽지 못했다(UTF-8 필요)".to_string()))
}

impl Scheduled {
    pub fn expression(&self) -> Result<String, &'static str> {
        match (&self.every, &self.daily) {
            (Some(e), None) => Ok(format!("every {}", e.trim())),
            (None, Some(d)) => Ok(format!("daily {}", d.trim())),
            _ => Err("every·daily 중 정확히 하나를 쓴다"),
        }
    }
}
impl Action {
    pub fn texts(&self) -> (Option<&str>, Option<&str>) {
        match self {
            Self::Deploy | Self::Command(_) => (None, None),
            Self::Inbox(a) => (a.text.as_deref(), a.text_file.as_deref()),
            Self::Message(a) => (a.text.as_deref(), a.text_file.as_deref()),
            Self::Start(a) => (a.text.as_deref(), a.brief_file.as_deref().or(a.text_file.as_deref())),
        }
    }
    pub fn set_text(&mut self, value: String) {
        let (text, file) = match self {
            Self::Deploy | Self::Command(_) => return,
            Self::Inbox(a) => (&mut a.text, &mut a.text_file),
            Self::Message(a) => (&mut a.text, &mut a.text_file),
            Self::Start(a) => {
                a.brief_file = None;
                (&mut a.text, &mut a.text_file)
            }
        };
        *text = Some(value);
        *file = None;
    }
    pub fn field(&self) -> &'static str {
        match self {
            Self::Deploy => "do.deploy",
            Self::Command(_) => "action",
            Self::Inbox(_) => "do.inbox",
            Self::Message(_) => "do.message",
            Self::Start(_) => "do.start",
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Deploy => "배포 제안".into(),
            Self::Command(_) => "명령 실행".into(),
            Self::Inbox(_) => "coordinator 안내".into(),
            Self::Message(a) => format!("{} thread에 글", if a.thread == "owner" { "주인" } else { &a.thread }),
            Self::Start(a) => format!("새 thread · {}", a.title),
        }
    }
}

pub(crate) fn parse(root: &std::path::Path, name: &str, text: &str) -> Result<crate::routine::Routine, String> {
    parse_with(name, text, |path| project_text(root, path))
}

pub(crate) fn parse_with(
    name: &str,
    text: &str,
    read: impl Fn(&str) -> Result<String, String>,
) -> Result<crate::routine::Routine, String> {
    use crate::routine::{Routine, Trigger};
    let file = format!("automations/{name}.toml");
    let mut spec = model::read(&file, text)?;
    let bad = |field: &str, message: &str| field_error(&file, text, field, message);
    if !aaa_proto::valid_project(name) {
        return Err(bad("title", "자동화 이름 형식이 아니다"));
    }
    let file_field =
        if matches!(&spec.action, Action::Start(a) if a.brief_file.is_some()) { "brief_file" } else { "text_file" };
    if matches!(&spec.action, Action::Start(a) if a.brief_file.is_some() && a.text_file.is_some()) {
        return Err(bad("do.start.brief_file", "brief_file·text_file은 함께 쓸 수 없다"));
    }
    if matches!(spec.action, Action::Deploy) {
        let Condition::Git(g) = &mut spec.on else {
            return Err(bad("on", "배포 제안은 main Git push 조건만 쓴다"));
        };
        if g.branches != ["main"] {
            return Err(bad("on.git.branches", "배포 제안은 main 하나만 구독한다"));
        }
        if g.events.is_empty() {
            g.events = vec!["pushed".into()];
        }
        if g.events != ["pushed"] {
            return Err(bad("on.git.events", "배포 제안은 pushed만 구독한다"));
        }
        if !(1..=aaa_common::git_watch::MAX_COMMITS as u32).contains(&spec.limits.min_commits.unwrap_or(1)) {
            return Err(bad("limits.min_commits", "1~100의 새 커밋 수를 쓴다"));
        }
    } else if spec.limits.min_commits.is_some() {
        return Err(bad("limits.min_commits", "배포 제안에만 쓴다"));
    }
    let (inline, external) = spec.action.texts();
    if inline.is_some() && external.is_some() {
        return Err(bad(&format!("{}.{file_field}", spec.action.field()), "text·text_file은 함께 쓸 수 없다"));
    }
    let prompt = match external {
        Some(path) => read(path).map_err(|e| bad(&format!("{}.{file_field}", spec.action.field()), &e))?,
        None => inline.unwrap_or_default().to_string(),
    }
    .trim()
    .to_string();
    spec.action.set_text(prompt.clone());
    if let Action::Start(a) = &spec.action {
        if a.reuse != Reuse::Open && (a.memory || a.close_idle.is_some()) {
            return Err(bad("do.start.reuse", "memory·close_idle는 reuse = open에서만 쓴다"));
        }
        if let Some(value) = &a.close_idle {
            duration_ms(value).map_err(|e| bad("do.start.close_idle", &e))?;
        }
        if a.title.trim().is_empty() {
            return Err(bad("do.start.title", "제목이 비었다"));
        }
        if prompt.is_empty() {
            return Err(bad("do.start.text", "text·text_file·brief_file 중 하나로 할 일을 준다"));
        }
        if a.profiles.is_empty() || a.profiles.iter().any(|n| n.trim().is_empty()) {
            return Err(bad("do.start.profiles", "비지 않은 이름 목록을 쓴다"));
        }
        // 빈 machines는 자동 머신이다. 적은 이름은 비지 않아야 한다
        if a.machines.iter().any(|n| n.trim().is_empty()) {
            return Err(bad("do.start.machines", "빈 이름을 쓰지 않는다(자동 머신은 목록을 비운다)"));
        }
        if a.repo.as_ref().is_some_and(|s| s.trim().is_empty()) {
            return Err(bad("do.start.repo", "repo가 비었다"));
        }
        if a.wait.iter().any(|n| !aaa_proto::valid_project(n)) {
            return Err(bad("do.start.wait", "timing 같은 표지 이름 목록을 쓴다"));
        }
        if let Some(wait) = &a.wait_max {
            duration_ms(wait).map_err(|e| bad("do.start.wait_max", &e))?;
        }
    }
    if let Action::Message(a) = &spec.action
        && a.thread != "owner"
        && !aaa_proto::valid_thread(&a.thread)
    {
        return Err(bad("do.message.thread", "owner 또는 thread id여야 한다"));
    }
    if matches!(&spec.action, Action::Message(a) if a.thread == "owner")
        && matches!(spec.on, Condition::Schedule(_) | Condition::Command(_))
    {
        return Err(bad("do.message.thread", "일정·명령 사건에는 owner가 없다. 고정 thread를 쓴다"));
    }
    for (field, duration) in [("batch", &spec.limits.batch), ("min_gap", &spec.limits.min_gap)] {
        if let Some(duration) = duration {
            duration_ms(duration).map_err(|e| bad(&format!("limits.{field}"), &e))?;
        }
    }
    spec.limits.max_per_day = spec.limits.max_per_day.filter(|n| *n > 0 && *n < u32::MAX);
    spec.limits.max_running = spec.limits.max_running.filter(|n| *n > 0 && *n < u32::MAX);
    for (field, value) in
        [("usage_session_below", spec.limits.usage_session_below), ("usage_week_below", spec.limits.usage_week_below)]
    {
        if value.is_some_and(|n| !n.is_finite() || !(0.0..=100.0).contains(&n)) {
            return Err(bad(&format!("limits.{field}"), "사용량 문턱은 0~100이다"));
        }
    }
    let mut front = toml::Table::new();
    let mut put = |key: &str, value: toml::Value| {
        front.insert(key.into(), value);
    };
    let mut catch_up = CatchUp::Once;
    let min_open_threads = match &spec.on {
        Condition::Schedule(s) => s.min_open_threads,
        Condition::Command(c) => c.min_open_threads,
        _ => None,
    };
    if min_open_threads == Some(0) {
        return Err(bad("condition.min_open_threads", "1 이상의 개수를 쓴다"));
    }
    match &spec.on {
        Condition::Schedule(s) => {
            put("schedule", s.expression().map_err(|e| bad("on.schedule", e))?.into());
            catch_up = s.catch_up;
        }
        Condition::Command(c) => {
            let s = Scheduled {
                every: c.every.clone(),
                daily: c.daily.clone(),
                catch_up: c.catch_up,
                min_open_threads: c.min_open_threads,
            };
            put("schedule", s.expression().map_err(|e| bad("on.command", e))?.into());
            if c.command.trim().is_empty() {
                return Err(bad("on.command.command", "명령이 비었다"));
            }
            put("command", c.command.clone().into());
            if let Some(m) = &c.machine {
                put("machine", m.clone().into());
            }
            catch_up = c.catch_up;
        }
        Condition::Ci(c) => {
            put("on", "ci".into());
            put("repo", c.repo.clone().into());
            put("branches", c.branches.clone().into());
            put("workflows", c.workflows.clone().into());
            put("events", c.events.clone().into());
            if let Some(m) = &c.machine {
                put("machine", m.clone().into());
            }
        }
        Condition::Git(g) => {
            put("on", "git".into());
            put("repo", g.repo.clone().into());
            put("branches", g.branches.clone().into());
            put("events", g.events.clone().into());
            if let Some(m) = &g.machine {
                put("machine", m.clone().into());
            }
        }
        Condition::Thread(t) => {
            if t.events.is_empty()
                || t.events.iter().any(|e| {
                    ["post_", "mission_", "routine_", "message_", "automation_"].iter().any(|p| e.starts_with(p))
                        || e == "evaluator_batch"
                })
            {
                return Err(bad("on.thread.events", "빈 사건 목록과 배경·메시지·post 사건은 구독할 수 없다"));
            }
            // 공용 legacy 검증을 거친 뒤 실제 flow trigger로 바꾼다.
            put("schedule", "every 1m".into());
        }
    }
    let legacy = format!("+++\n{}+++\n{prompt}\n", toml::to_string(&front).map_err(|e| bad("on", &e.to_string()))?);
    let mut r: Routine = crate::routine::parse(name, &legacy).map_err(|e| {
        bad(
            match spec.on {
                Condition::Ci(_) => "on.ci",
                Condition::Git(_) => "on.git",
                Condition::Command(_) => "on.command",
                _ => "on.schedule",
            },
            &e,
        )
    })?;
    if let Condition::Thread(t) = &spec.on {
        r.trigger = Trigger::Thread(t.clone());
        r.schedule_text = format!("on thread: {}", t.events.join(", "));
    }
    if let Action::Command(a) = &spec.action {
        if a.command.trim().is_empty() {
            return Err(bad("action.command", "명령이 비었다"));
        }
        r.command = a.command.clone();
        r.machine = a.machine.clone();
    }
    r.enabled = spec.enabled;
    r.title = if spec.title.is_empty() { name.into() } else { spec.title };
    r.file = file;
    r.action = spec.action;
    r.limits = spec.limits;
    r.catch_up = catch_up;
    r.min_open_threads = min_open_threads;
    Ok(r)
}

pub(crate) fn duration_ms(value: &str) -> Result<u64, String> {
    if matches!(value.trim(), "0" | "0s" | "0m") {
        return Ok(0);
    }
    let seconds = std::env::var(crate::routine::TEST_SECONDS).as_deref() == Ok("1");
    match crate::routine::parse_schedule(&format!("every {}", value.trim()), seconds)? {
        crate::routine::Schedule::Every(s) => Ok(s * 1000),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn schema() -> serde_json::Value {
    let mut schema = serde_json::to_value(schemars::schema_for!(aaa_proto::AutomationDefinition)).expect("schema");
    schema["title"] = "AAA 자동화 v2".into();
    schema["properties"]["schema"] = serde_json::json!({"const":2});
    schema["$defs"]["AutomationTrigger"]["oneOf"][0]["oneOf"] = serde_json::json!([{"required":["every"],"not":{"required":["daily"]}},{"required":["daily"],"not":{"required":["every"]}}]);
    for (name, field) in [("AutomationPolicy", "min_gap"), ("AutomationThread", "wait_max")] {
        schema["$defs"][name]["properties"][field] = serde_json::json!({"anyOf":[{"type":"string"},{"const":0}]});
    }
    for name in ["AutomationNotice", "AutomationMessage"] {
        schema["$defs"][name]["not"] = serde_json::json!({"required":["text","text_file"]});
    }
    schema["$defs"]["AutomationThread"]["oneOf"] = serde_json::json!([
        {"required":["text"],"not":{"anyOf":[{"required":["text_file"]},{"required":["brief_file"]}]}},
        {"required":["text_file"],"not":{"anyOf":[{"required":["text"]},{"required":["brief_file"]}]}},
        {"required":["brief_file"],"not":{"anyOf":[{"required":["text"]},{"required":["text_file"]}]}}
    ]);
    schema
}

pub(crate) fn duration_text(seconds: u64) -> String {
    if seconds == 0 {
        "0m".into()
    } else if seconds.is_multiple_of(86400) {
        format!("{}d", seconds / 86400)
    } else if seconds.is_multiple_of(3600) {
        format!("{}h", seconds / 3600)
    } else if seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}
impl Scheduled {
    pub fn from_schedule(s: &crate::routine::Schedule, catch_up: CatchUp) -> Self {
        match s {
            crate::routine::Schedule::Every(n) => {
                Self { every: Some(duration_text(*n)), daily: None, catch_up, min_open_threads: None }
            }
            crate::routine::Schedule::Daily(h, m) => {
                Self { daily: Some(format!("{h:02}:{m:02}")), every: None, catch_up, min_open_threads: None }
            }
        }
    }
}
impl File {
    pub fn from_routine(r: &crate::routine::Routine) -> Self {
        use crate::routine::Trigger;
        let on = match &r.trigger {
            Trigger::Schedule(s) if !r.command.is_empty() && !matches!(r.action, Action::Command(_)) => {
                let schedule = Scheduled::from_schedule(s, r.catch_up);
                Condition::Command(Command {
                    every: schedule.every,
                    daily: schedule.daily,
                    catch_up: r.catch_up,
                    min_open_threads: r.min_open_threads,
                    command: r.command.clone(),
                    machine: r.machine.clone(),
                })
            }
            Trigger::Schedule(s) => {
                let mut schedule = Scheduled::from_schedule(s, r.catch_up);
                schedule.min_open_threads = r.min_open_threads;
                Condition::Schedule(schedule)
            }
            Trigger::Thread(t) => Condition::Thread(t.clone()),
            Trigger::Ci(c) => Condition::Ci(Ci {
                repo: c.repo.clone(),
                branches: c.branches.clone(),
                workflows: c.workflows.clone(),
                events: c.events.clone(),
                machine: r.machine.clone(),
            }),
            Trigger::Git(g) => Condition::Git(Git {
                repo: g.repo.clone(),
                branches: g.branches.clone(),
                events: g.events.clone(),
                machine: r.machine.clone(),
            }),
        };
        Self { title: r.title.clone(), enabled: r.enabled, on, action: r.action.clone(), limits: r.limits.clone() }
    }
}

impl crate::Host {
    pub(crate) fn automation_recover(&self, p: &str) -> Result<Vec<String>, aaa_proto::ErrorBody> {
        let bad = |e: String| crate::err(aaa_proto::ErrorCode::Internal, e);
        let key = format!("automation_migration:{p}");
        let Some(raw) = self.db.meta(&key).map_err(|e| bad(e.to_string()))? else { return Ok(vec![]) };
        let mut timing = convert::Timing::new("recover");
        let plan: convert::Plan = serde_json::from_str(&raw).map_err(|e| bad(e.to_string()))?;
        let root = self.project_dir(p)?;
        let files = timing.measure("files", "", || convert::resume(&root, &plan)).map_err(bad)?;
        let state_started = std::time::Instant::now();
        self.legacy_mission_recover(p)?;
        for name in self.db.meta_suffixes(&format!("automation:{p}:")).map_err(|e| bad(e.to_string()))? {
            self.db
                .update_meta(&format!("automation:{p}:{name}"), |raw| {
                    raw.ok_or_else(|| anyhow::anyhow!("큐가 사라졌다"))
                })
                .map_err(|e| bad(e.to_string()))?;
        }
        for name in self.db.meta_suffixes(&format!("routine_runs:{p}:")).map_err(|e| bad(e.to_string()))? {
            let history = self.routine_history(p, &name)?;
            self.db.automation_import_observations(p, &name, &history.runs).map_err(|e| bad(e.to_string()))?;
        }
        self.db.del_meta(&key).map_err(|e| bad(e.to_string()))?;
        timing.record("db_and_state", state_started);
        Ok(files)
    }
    /// `dry_run`이면 계획만 답한다(쓰기·임무 seed·audit 없음). `expected`가 있으면 그 계획일 때만 변환한다.
    pub(crate) async fn automation_convert(
        &self,
        p: &str,
        dry_run: bool,
        expected: Option<&str>,
        actor: &str,
    ) -> Result<(Vec<String>, String), aaa_proto::ErrorBody> {
        let mut timing = convert::Timing::new(if dry_run { "preview" } else { "convert" });
        let lock_started = std::time::Instant::now();
        let _guard = self.automation_sources.lock().await;
        let dispatch = self.automation_dispatch_lock(p);
        let _dispatch = dispatch.lock().await;
        timing.record("locks", lock_started);
        let root = self.project_dir(p)?;
        let mut plan = timing
            .measure("prepare", "", || convert::prepare(&root, &self.resolve_machine(None).unwrap_or_default()))
            .map_err(|e| crate::err(aaa_proto::ErrorCode::BadRequest, e))?;
        let state_started = std::time::Instant::now();
        let journal = plan.include_state(self, p).await?;
        timing.record("include_state", state_started);
        if dry_run {
            return Ok((plan.listing(), plan.sha256()));
        }
        let sha = plan.sha256();
        if expected.is_some_and(|want| want != sha) {
            return Err(crate::err(
                aaa_proto::ErrorCode::BadRequest,
                "미리보기 뒤 옛 파일이나 변환 계획이 바뀌었다. 미리보기를 다시 연다",
            ));
        }
        let journal_started = std::time::Instant::now();
        self.db
            .set_meta(
                &format!("mission_conversion:{p}"),
                &serde_json::to_string(&journal)
                    .map_err(|e| crate::err(aaa_proto::ErrorCode::Internal, e.to_string()))?,
            )
            .map_err(|e| crate::err(aaa_proto::ErrorCode::Internal, e.to_string()))?;
        self.db
            .set_meta(
                &format!("automation_migration:{p}"),
                &serde_json::to_string(&plan).map_err(|e| crate::err(aaa_proto::ErrorCode::Internal, e.to_string()))?,
            )
            .map_err(|e| crate::err(aaa_proto::ErrorCode::Internal, e.to_string()))?;
        timing.record("journal_db", journal_started);
        let files = self.automation_recover(p)?;
        self.legacy_mission_recover(p)?;
        self.db.audit(actor, "automation_convert", &format!("{p}: {} files", files.len()));
        self.tick_wake.notify_one();
        Ok((files, sha))
    }
}

#[cfg(test)]
#[path = "automation/start_tests.rs"]
mod start_tests;
