//! 조건 → 동작의 파일 계약. 실행 상태·기록과 파일 형식은 분리한다.
mod convert;
pub(crate) mod runtime;
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
    Pr(Pr),
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
    pub every: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily: Option<String>,
    pub catch_up: CatchUp,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Command {
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
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Pr {
    pub events: Vec<String>,
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
    Inbox(Inbox),
    Message(Message),
    Wake(Wake),
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Wake {
    pub mission: String,
    #[serde(default)]
    pub reports: bool,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_file: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Limits {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "optional_duration_value")]
    pub min_gap: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_per_day: Option<u32>,
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
        let metadata = std::fs::symlink_metadata(&at).map_err(|_| "글 파일을 읽지 못한다")?;
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
    std::fs::read_to_string(at).map_err(|_| "글 파일은 UTF-8이어야 한다".into())
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
    pub fn texts(&self) -> (&Option<String>, &Option<String>) {
        match self {
            Self::Inbox(a) => (&a.text, &a.text_file),
            Self::Message(a) => (&a.text, &a.text_file),
            Self::Wake(a) => (&a.text, &a.text_file),
        }
    }
    pub fn set_text(&mut self, value: String) {
        let (text, file) = match self {
            Self::Inbox(a) => (&mut a.text, &mut a.text_file),
            Self::Message(a) => (&mut a.text, &mut a.text_file),
            Self::Wake(a) => (&mut a.text, &mut a.text_file),
        };
        *text = Some(value);
        *file = None;
    }
    pub fn field(&self) -> &'static str {
        match self {
            Self::Inbox(_) => "do.inbox",
            Self::Message(_) => "do.message",
            Self::Wake(_) => "do.wake",
        }
    }
    pub fn mission(&self) -> Option<&str> {
        match self {
            Self::Wake(a) => Some(&a.mission),
            _ => None,
        }
    }
    pub fn label(&self) -> String {
        match self {
            Self::Inbox(_) => "coordinator 안내".into(),
            Self::Message(a) => format!("{} thread에 글", if a.thread == "owner" { "주인" } else { &a.thread }),
            Self::Wake(a) => format!("임무 {} 입력", a.mission),
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
    let mut spec: File = decode(&file, text)?;
    let bad = |field: &str, message: &str| field_error(&file, text, field, message);
    if !aaa_proto::valid_project(name) {
        return Err(bad("title", "자동화 이름 형식이 아니다"));
    }
    let (inline, external) = spec.action.texts();
    if inline.is_some() && external.is_some() {
        return Err(bad(&format!("{}.text_file", spec.action.field()), "text·text_file은 함께 쓸 수 없다"));
    }
    let prompt = match external {
        Some(path) => read(path).map_err(|e| bad(&format!("{}.text_file", spec.action.field()), &e))?,
        None => inline.clone().unwrap_or_default(),
    }
    .trim()
    .to_string();
    spec.action.set_text(prompt.clone());
    if let Action::Wake(a) = &spec.action
        && !aaa_proto::valid_project(&a.mission)
    {
        return Err(bad("do.wake.mission", "임무 이름 형식이 아니다"));
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
    spec.limits.max_per_day = spec.limits.max_per_day.filter(|n| *n > 0);
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
    match &spec.on {
        Condition::Schedule(s) => {
            put("schedule", s.expression().map_err(|e| bad("on.schedule", e))?.into());
            catch_up = s.catch_up;
        }
        Condition::Command(c) => {
            let s = Scheduled { every: c.every.clone(), daily: c.daily.clone(), catch_up: c.catch_up };
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
        Condition::Pr(p) => {
            put("on", "pr".into());
            put("events", p.events.clone().into());
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
                Condition::Pr(_) => "on.pr",
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
    r.enabled = spec.enabled;
    r.title = if spec.title.is_empty() { name.into() } else { spec.title };
    r.file = file;
    r.mission = spec.action.mission().map(str::to_string);
    r.action = spec.action;
    r.limits = spec.limits;
    r.catch_up = catch_up;
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
    let mut schema = serde_json::to_value(schemars::schema_for!(File)).expect("schema serialize");
    for name in ["Scheduled", "Command"] {
        schema["$defs"][name]["oneOf"] = serde_json::json!([{"required":["every"],"not":{"required":["daily"]}},{"required":["daily"],"not":{"required":["every"]}}]);
    }
    schema["title"] = "AAA 자동화".into();
    schema["$defs"]["Limits"]["properties"]["min_gap"] =
        serde_json::json!({"anyOf":[{"type":"string"},{"const":0}],"description":"생략 또는0이면 제한 없음"});
    schema["$defs"]["Limits"]["properties"]["max_per_day"]["minimum"] = 0.into();
    for field in ["usage_session_below", "usage_week_below"] {
        schema["$defs"]["Limits"]["properties"][field]["minimum"] = 0.into();
        schema["$defs"]["Limits"]["properties"][field]["maximum"] = 100.into();
    }
    for name in ["Inbox", "Message", "Wake"] {
        schema["$defs"][name]["not"] = serde_json::json!({"required":["text","text_file"]});
    }
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
            crate::routine::Schedule::Every(n) => Self { every: Some(duration_text(*n)), daily: None, catch_up },
            crate::routine::Schedule::Daily(h, m) => {
                Self { daily: Some(format!("{h:02}:{m:02}")), every: None, catch_up }
            }
        }
    }
}
impl File {
    pub fn from_routine(r: &crate::routine::Routine) -> Self {
        use crate::routine::Trigger;
        let on = match &r.trigger {
            Trigger::Schedule(s) if !r.command.is_empty() => {
                let schedule = Scheduled::from_schedule(s, r.catch_up);
                Condition::Command(Command {
                    every: schedule.every,
                    daily: schedule.daily,
                    catch_up: r.catch_up,
                    command: r.command.clone(),
                    machine: r.machine.clone(),
                })
            }
            Trigger::Schedule(s) => Condition::Schedule(Scheduled::from_schedule(s, r.catch_up)),
            Trigger::Thread(t) => Condition::Thread(t.clone()),
            Trigger::Pr(events) => Condition::Pr(Pr { events: events.clone() }),
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
    pub(crate) async fn automation_convert(&self, p: &str, actor: &str) -> Result<Vec<String>, aaa_proto::ErrorBody> {
        let _guard = self.automation_sources.lock().await;
        let root = self.project_dir(p)?;
        let previous: Vec<_> = self
            .db
            .threads(Some(p))
            .map_err(|e| crate::err(aaa_proto::ErrorCode::Internal, e.to_string()))?
            .iter()
            .filter_map(|j| serde_json::from_str::<crate::orch::ThreadRec>(j).ok())
            .filter(|r| r.status != aaa_proto::ThreadStatus::Resolved)
            .filter_map(|r| r.mission)
            .map(|m| (m.name, m.cursor, m.last_schedule_ms))
            .collect();
        let plan = convert::prepare(&root).map_err(|e| crate::err(aaa_proto::ErrorCode::BadRequest, e))?;
        let files = plan.files();
        for (name, cursor, last) in previous {
            for suffix in ["events", "schedule"] {
                let rule = format!("{name}-{suffix}");
                if files.iter().any(|f| f == &format!("automations/{rule}.toml")) {
                    self.automation_seed_mission(p, &name, &rule, cursor)?;
                    if suffix == "schedule" {
                        self.seed_routine_schedule(p, &rule, last);
                    }
                }
            }
        }
        let files = convert::apply(&root, plan).map_err(|e| crate::err(aaa_proto::ErrorCode::BadRequest, e))?;
        self.db.audit(actor, "automation_convert", &format!("{p}: {} files", files.len()));
        self.tick_wake.notify_one();
        Ok(files)
    }
}
