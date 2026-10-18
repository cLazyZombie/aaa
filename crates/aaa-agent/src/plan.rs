//! pane의 계획 n/m·배경 작업 수(차이 목록 S5). harness가 이미 대화 기록에 남긴 것을 읽는다: agent에게 새로 묻지 않는다.
//!
//! - 계획(herdr-web-ui `server/session-plan.ts`): Claude는 `TaskCreate`·`TaskUpdate` 호출 중 오류 없이 답을 받은 것만
//!   단계가 된다. Claude는 모든 단계가 끝난 목록을 비우므로, 그 뒤 처음 만든 단계는 새 계획을 시작한다. Codex는
//!   `update_plan` 호출마다 목록 전체를 보내므로 마지막 것이 계획이다.
//! - 배경 작업(herdr-web-ui `server/claude-subagents.ts` 가운데 수만): Claude 부모 기록의 하위 agent 호출(`Agent`·`Task`,
//!   동기면 답까지, 배경이면 「Async agent launched」 뒤 알림까지)과 배경 명령(답의 `backgroundTaskId`)을 센다. 끝남은
//!   `<task-notification>`(사람 쪽 글·queue-operation·attachment 세 자리)의 task-id·tool-use-id나 `TaskStop`·`KillShell`의 답이다.
//!   참고와 달리 하위 agent 폴더(`<session>/subagents/`)는 읽지 않는다: 하위 agent의 기록으로 보는 팀 동료의 끝남 추정·
//!   다시 깨운 agent·하위 agent가 띄운 배경 명령·토큰은 뺐다(plan §8.142).
//! - 배경 작업은 그것을 띄운 Claude 프로세스 안에서 돈다: 지금 프로세스가 시작하기 전에 띄운 것은 세지 않는다(`since_ms`:
//!   Claude SessionStart startup·resume를 받은 때, 모르면 aaa 세션이 시작한 때).
//! - 기록은 처음부터 읽되 한 번에 `budget`만큼만, 그 뒤로는 늘어난 만큼만 읽는다. 처음 읽기가 끝에 닿기 전에는 계획을 말하지 않는다.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{HarnessEvent, PlanSummary};
use serde_json::Value;

/// 파일 하나를 한 번에 읽는 상한. 이보다 긴 한 줄(요청 원문 등)은 건너뛴다
const READ_BUDGET: u64 = 8 << 20;
/// 한 번 훑을 때 모든 기록을 합쳐 읽는 상한
pub const POLL_BUDGET: u64 = 32 << 20;
const MAX_STEPS: usize = 200;
const MAX_NAMES: usize = 5000;
const WORDS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Pending,
    InProgress,
    Completed,
}

impl Status {
    fn of(v: &Value) -> Option<Status> {
        match v.as_str()? {
            "pending" => Some(Status::Pending),
            "in_progress" => Some(Status::InProgress),
            "completed" => Some(Status::Completed),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct Step {
    id: String,
    label: String,
    active: Option<String>,
    status: Status,
}

/// 지금 읽은 것: 계획 요약과 아직 도는 배경 작업 수
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Snapshot {
    pub plan: Option<PlanSummary>,
    pub background: Option<u32>,
}

/// 기록 하나를 따라 읽는 상태
pub struct Reader {
    source: Source,
    path: PathBuf,
    file: Option<(u64, u64)>,
    offset: u64,
    /// 다음 읽기가 원하지 않는 줄(너무 긴 줄) 한가운데서 시작한다
    skipping: bool,
    /// 처음 읽기가 아직 끝에 닿지 않았다
    catching: bool,
    steps: Vec<Step>,
    /// 답을 기다리는 TaskCreate·TaskUpdate 호출(tool_use id → 이름·입력)
    calls: HashMap<String, (String, Value)>,
    /// 도는 배경 작업(하위 agent 호출의 tool_use id, 배경 명령의 task id) → 띄운 시각(ms)
    running: HashMap<String, i64>,
    /// 배경 하위 agent의 agent id → 그 호출의 tool_use id(알림은 agent id로 온다)
    agents: HashMap<String, String>,
    /// 알림이나 멈춤으로 끝난 것(같은 알림이 여러 번 실린다)
    ended: HashSet<String>,
}

fn words(v: Option<&Value>) -> Option<String> {
    let t = v?.as_str()?.trim();
    (!t.is_empty()).then(|| t.chars().take(WORDS).collect())
}

fn id_of(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Number(n) => Some(n.to_string()),
        other => words(Some(other)),
    }
}

/// `2026-10-18T09:17:01.123Z` 꼴(Claude 기록의 UTC 시각) → Unix ms
pub fn stamp_ms(t: &str) -> Option<i64> {
    let b = t.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || !t.ends_with('Z')
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| t.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (num(0..4)?, num(5..7)?, num(8..10)?, num(11..13)?, num(14..16)?, num(17..19)?);
    let frac = t[19..t.len() - 1].strip_prefix('.').unwrap_or("");
    let ms = if frac.is_empty() { 0 } else { format!("{:0<3}", &frac[..frac.len().min(3)]).parse::<i64>().ok()? };
    // 그레고리력 날짜 → 1970-01-01부터 날 수(Howard Hinnant days_from_civil)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + s * 1000 + ms)
}

#[cfg(unix)]
fn identity(m: &std::fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (m.dev(), m.ino())
}

#[cfg(not(unix))]
fn identity(_: &std::fs::Metadata) -> (u64, u64) {
    (0, 0)
}

/// 알림 한 덩어리의 task id와 tool-use id
fn notice(block: &str) -> Option<(String, Option<String>)> {
    let b = block.trim();
    if !b.starts_with("<task-notification>") || !b.ends_with("</task-notification>") {
        return None;
    }
    let tag = |name: &str| {
        let open = format!("<{name}>");
        let from = b.find(&open)? + open.len();
        let to = from + b[from..].find(&format!("</{name}>"))?;
        Some(b[from..to].trim().to_string())
    };
    let task = tag("task-id")
        .filter(|t| !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))?;
    Some((task, tag("tool-use-id")))
}

/// 기록 한 줄이 알림으로 싣는 덩어리들(참고 `blocksIn`): 사람 쪽 글, queue-operation enqueue, task-notification attachment
fn notice_blocks(e: &Value) -> Vec<&str> {
    match e.get("type").and_then(Value::as_str) {
        Some("queue-operation") if e.get("operation").and_then(Value::as_str) == Some("enqueue") => {
            e.get("content").and_then(Value::as_str).into_iter().collect()
        }
        Some("attachment") => {
            let a = e.get("attachment");
            if a.and_then(|a| a.get("commandMode")).and_then(Value::as_str) == Some("task-notification") {
                a.and_then(|a| a.get("prompt")).and_then(Value::as_str).into_iter().collect()
            } else {
                Vec::new()
            }
        }
        Some("user") => match e.pointer("/message/content") {
            Some(Value::String(t)) => vec![t.as_str()],
            // 도구 답이 덩어리를 인용한 것은 알림이 아니다: 글 조각만 본다
            Some(Value::Array(parts)) => parts
                .iter()
                .filter(|p| p.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|p| p.get("text").and_then(Value::as_str))
                .collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(t)) => t.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect(),
        _ => String::new(),
    }
}

impl Reader {
    pub fn new(source: Source, path: PathBuf) -> Self {
        Reader {
            source,
            path,
            file: None,
            offset: 0,
            skipping: false,
            catching: true,
            steps: Vec::new(),
            calls: HashMap::new(),
            running: HashMap::new(),
            agents: HashMap::new(),
            ended: HashSet::new(),
        }
    }

    /// 늘어난 만큼 읽는다(`budget`을 줄인다). 처음 읽기가 끝에 닿기 전이면 None. `since_ms`: 지금 harness 프로세스가 시작한 때
    pub fn poll(&mut self, budget: &mut u64, since_ms: i64) -> Option<Snapshot> {
        let meta = std::fs::metadata(&self.path).ok().filter(|m| m.is_file())?;
        let id = identity(&meta);
        // 다른 파일이 되었거나 줄었다: 처음부터
        if self.file != Some(id) || meta.len() < self.offset {
            *self = Reader::new(self.source, std::mem::take(&mut self.path));
            self.file = Some(id);
        }
        while self.offset < meta.len() && *budget > 0 {
            let Ok(consumed) = self.read_some(meta.len(), (*budget).min(READ_BUDGET)) else { return None };
            *budget = budget.saturating_sub(consumed);
            if consumed == 0 {
                break;
            }
        }
        if self.offset < meta.len() && self.catching {
            return None;
        }
        self.catching = false;
        Some(self.snapshot(since_ms))
    }

    fn snapshot(&self, since_ms: i64) -> Snapshot {
        let plan = (!self.steps.is_empty()).then(|| {
            let current = self.steps.iter().find(|s| s.status == Status::InProgress);
            PlanSummary {
                done: self.steps.iter().filter(|s| s.status == Status::Completed).count() as u32,
                total: self.steps.len() as u32,
                current: current.map(|s| s.active.clone().unwrap_or_else(|| s.label.clone())),
            }
        });
        let n = self.running.values().filter(|at| **at >= since_ms).count() as u32;
        Snapshot { plan, background: (n > 0).then_some(n) }
    }

    /// 한 조각을 읽어 끝난 줄만 처리한다. 처리한(넘긴) 바이트 수
    fn read_some(&mut self, size: u64, limit: u64) -> std::io::Result<u64> {
        let mut f = std::fs::File::open(&self.path)?;
        f.seek(SeekFrom::Start(self.offset))?;
        let mut buf = Vec::new();
        f.take((size - self.offset).min(limit)).read_to_end(&mut buf)?;
        let got = buf.len() as u64;
        let more = self.offset + got < size;
        let mut start = 0;
        if self.skipping {
            match buf.iter().position(|&c| c == b'\n') {
                Some(i) => start = i + 1,
                None => {
                    self.offset += got;
                    return Ok(got);
                }
            }
        }
        let end = buf.iter().rposition(|&c| c == b'\n').map_or(0, |i| i + 1);
        if end <= start {
            if self.skipping {
                self.skipping = false;
                self.offset += start as u64;
                return Ok(start as u64);
            }
            // 한 줄이 이번 상한보다 길다: 상한이 다 찼으면 그 줄을 버리고, 아니면 다음에 더 크게 읽는다
            if more && limit >= READ_BUDGET {
                self.skipping = true;
                self.offset += got;
                return Ok(got);
            }
            return Ok(0);
        }
        self.skipping = false;
        for line in buf[start..end].split(|&c| c == b'\n') {
            if let Ok(line) = std::str::from_utf8(line)
                && !line.is_empty()
            {
                match self.source {
                    Source::Claude => self.claude_line(line),
                    Source::Codex => self.codex_line(line),
                }
            }
        }
        self.offset += end as u64;
        Ok(end as u64)
    }

    fn claude_line(&mut self, line: &str) {
        let planning = line.contains(r#""name":"TaskCreate""#) || line.contains(r#""name":"TaskUpdate""#);
        let answering = line.contains(r#""tool_result""#);
        let background = line.contains("<task-notification>")
            || line.contains(r#""name":"Agent""#)
            || line.contains(r#""name":"Task""#)
            || line.contains(r#""backgroundTaskId""#)
            || line.contains(r#""task_id""#)
            || line.contains(r#""shell_id""#);
        if !planning && !background && !(answering && (!self.calls.is_empty() || !self.running.is_empty())) {
            return;
        }
        let Ok(e) = serde_json::from_str::<Value>(line) else { return };
        let at = e.get("timestamp").and_then(Value::as_str).and_then(stamp_ms);
        let result = e.get("toolUseResult");
        if line.contains("<task-notification>") {
            for block in notice_blocks(&e) {
                if let Some((task, tool)) = notice(block) {
                    self.end(&task);
                    if let Some(u) = self.agents.remove(&task) {
                        self.end(&u);
                    }
                    if let Some(u) = tool {
                        self.end(&u);
                    }
                }
            }
        }
        let Some(content) = e.pointer("/message/content").and_then(Value::as_array) else { return };
        let failed = content.iter().any(|b| {
            b.get("type").and_then(Value::as_str) == Some("tool_result")
                && b.get("is_error") == Some(&Value::Bool(true))
        });
        for b in content {
            let kind = b.get("type").and_then(Value::as_str);
            let name = b.get("name").and_then(Value::as_str);
            let id = b.get("id").and_then(Value::as_str);
            if kind == Some("tool_use") {
                match (name, id) {
                    (Some(n @ ("TaskCreate" | "TaskUpdate")), Some(id)) => {
                        self.calls
                            .insert(id.to_string(), (n.to_string(), b.get("input").cloned().unwrap_or(Value::Null)));
                    }
                    (Some("Agent" | "Task"), Some(id)) => {
                        if let Some(at) = at {
                            self.launch(id, at);
                        }
                    }
                    _ => {}
                }
                continue;
            }
            if kind != Some("tool_result") {
                continue;
            }
            let Some(u) = b.get("tool_use_id").and_then(Value::as_str) else { continue };
            if let Some((name, input)) = self.calls.remove(u) {
                if b.get("is_error") != Some(&Value::Bool(true)) {
                    if name == "TaskCreate" {
                        self.created(&input, result, &result_text(b.get("content")));
                    } else {
                        self.updated(&input, result);
                    }
                }
                continue;
            }
            if self.running.contains_key(u) {
                // 배경 하위 agent는 바로 「Async agent launched」로 답한다: 끝남이 아니다(알림을 agent id로 기다린다)
                let launched = result.and_then(|r| r.get("status")).and_then(Value::as_str) == Some("async_launched")
                    || result_text(b.get("content")).starts_with("Async agent launched");
                if launched {
                    if let Some(agent) = result.and_then(|r| r.get("agentId")).and_then(Value::as_str) {
                        self.agents.insert(agent.to_string(), u.to_string());
                    }
                } else {
                    self.end(u);
                }
            }
        }
        if !failed && let Some(r) = result {
            if let (Some(task), Some(at)) = (r.get("backgroundTaskId").and_then(Value::as_str), at) {
                self.launch(task, at);
            }
            // TaskStop은 멈춘 task를, 옛 KillShell은 shell을 답한다
            let stopped =
                if r.get("task_type").is_some_and(Value::is_string) { r.get("task_id") } else { r.get("shell_id") };
            if let Some(t) = stopped.and_then(Value::as_str) {
                self.end(t);
            }
        }
        if self.calls.len() > MAX_STEPS {
            self.calls.clear();
        }
    }

    fn launch(&mut self, key: &str, at: i64) {
        if self.ended.contains(key) || self.running.len() >= MAX_NAMES {
            return;
        }
        self.running.insert(key.to_string(), at);
    }

    fn end(&mut self, key: &str) {
        self.running.remove(key);
        if self.ended.len() >= MAX_NAMES {
            self.ended.clear();
        }
        self.ended.insert(key.to_string());
    }

    fn created(&mut self, input: &Value, result: Option<&Value>, text: &str) {
        let id = id_of(result.and_then(|r| r.pointer("/task/id"))).or_else(|| {
            let rest = text.strip_prefix("Task #")?;
            let (id, tail) = rest.split_once(' ')?;
            tail.starts_with("created").then(|| id.to_string())
        });
        let Some(id) = id else { return };
        // Claude는 모두 끝난 목록을 비운다: 다음 단계가 새 계획을 시작한다
        if self.steps.iter().all(|s| s.status == Status::Completed) {
            self.steps.clear();
        }
        if self.steps.len() >= MAX_STEPS {
            return;
        }
        let label = words(input.get("subject")).unwrap_or_else(|| format!("#{id}"));
        self.steps.push(Step { id, label, active: words(input.get("activeForm")), status: Status::Pending });
    }

    fn updated(&mut self, input: &Value, result: Option<&Value>) {
        if result.and_then(|r| r.get("success")) == Some(&Value::Bool(false)) {
            return;
        }
        let Some(id) = id_of(input.get("taskId")) else { return };
        let Some(i) = self.steps.iter().position(|s| s.id == id) else { return };
        if input.get("status").and_then(Value::as_str) == Some("deleted") {
            self.steps.remove(i);
            return;
        }
        let step = &mut self.steps[i];
        if let Some(st) = input.get("status").and_then(Status::of) {
            step.status = st;
        }
        if let Some(l) = words(input.get("subject")) {
            step.label = l;
        }
        if let Some(a) = words(input.get("activeForm")) {
            step.active = Some(a);
        }
    }

    fn codex_line(&mut self, line: &str) {
        if !line.contains("update_plan") {
            return;
        }
        let Ok(e) = serde_json::from_str::<Value>(line) else { return };
        let Some(p) = e.get("payload") else { return };
        if p.get("type").and_then(Value::as_str) != Some("function_call")
            || p.get("name").and_then(Value::as_str) != Some("update_plan")
        {
            return;
        }
        let Some(args) = p.get("arguments").and_then(Value::as_str).and_then(|a| serde_json::from_str::<Value>(a).ok())
        else {
            return;
        };
        let Some(items) = args.get("plan").and_then(Value::as_array) else { return };
        self.steps.clear();
        for item in items.iter().take(MAX_STEPS) {
            let Some(label) = words(item.get("step")) else { continue };
            let status = item.get("status").and_then(Status::of).unwrap_or(Status::Pending);
            let id = (self.steps.len() + 1).to_string();
            self.steps.push(Step { id, label, active: None, status });
        }
    }
}

/// 기록을 훑는 간격(참고 `SessionPlans` pollMs)
const POLL_EVERY: Duration = Duration::from_secs(2);
/// 기록을 아직 못 찾은 세션을 다시 찾아보는 간격(참고 retryMs). Codex rollout 찾기는 폴더를 훑는다
const RETRY: Duration = Duration::from_secs(15);

/// 세션 하나의 추적
pub(crate) struct Track {
    /// 무엇을 읽나(harness와 기록 위치 또는 thread-id): 바뀌면 처음부터
    key: String,
    reader: Option<Reader>,
    looked: Option<Instant>,
    /// 마지막으로 host에 알린 것
    sent: Snapshot,
}

/// harness 세션 사건이 알린 기록 파일. 기록 폴더 밖이면 없다(chat.rs와 같은 검사: hook은 세션 안 누구나 보낸다)
fn locate(harness: &str, transcript: Option<&str>, harness_session: Option<&str>) -> Option<(Source, PathBuf)> {
    match harness {
        "claude" => Some((
            Source::Claude,
            crate::chat::inside(&crate::chat::claude_root(), std::path::Path::new(transcript?)).ok()?,
        )),
        "codex" => Some((Source::Codex, crate::chat::codex_file(&crate::chat::codex_root(), harness_session?).ok()?)),
        _ => None,
    }
}

impl crate::Agent {
    /// 계획·배경 작업 수를 2초마다 훑어, 바뀐 세션만 host에 알린다(`kind = "plan"` harness 사건)
    pub fn start_plans(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(POLL_EVERY).await;
                let m = me.clone();
                let changed = tokio::task::spawn_blocking(move || m.poll_plans()).await.unwrap_or_default();
                for ev in changed {
                    me.harness_event(ev).await;
                }
            }
        });
    }

    /// Claude SessionStart(startup·resume)는 새 Claude 프로세스다: 그 전에 띄운 배경 작업은 이 프로세스에 없다
    pub(crate) fn harness_process_started(&self, session: &str, at_ms: u64) {
        self.plan_since.lock().unwrap().insert(session.to_string(), at_ms);
    }

    fn poll_plans(&self) -> Vec<HarnessEvent> {
        let running: HashMap<String, u64> = self
            .sessions
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, e)| e.info.state.is_running())
            .map(|(id, e)| (id.clone(), e.info.started_at_ms))
            .collect();
        // 따라 읽을 세션: 도는 세션 중 harness 세션 사건(기록 위치·thread-id)이 있는 것과, 기억한 마지막 계획
        let mut wanted = Vec::new();
        for (session, list) in self.harness_last.lock().unwrap().iter() {
            if !running.contains_key(session) {
                continue;
            }
            let Some(ev) = list.iter().find(|e| matches!(e.kind.as_str(), "session_start" | "session")) else {
                continue;
            };
            let kept = list
                .iter()
                .find(|e| e.kind == "plan")
                .map(|e| Snapshot { plan: e.plan.clone(), background: e.background });
            wanted.push((session.clone(), ev.harness.clone(), ev.transcript.clone(), ev.harness_session.clone(), kept));
        }
        let since = self.plan_since.lock().unwrap().clone();
        let mut tracks = self.plans.lock().unwrap();
        tracks.retain(|s, _| running.contains_key(s));
        let mut budget = POLL_BUDGET;
        let mut out = Vec::new();
        for (session, harness, transcript, harness_session, kept) in wanted {
            let key = format!(
                "{harness}\0{}\0{}",
                transcript.as_deref().unwrap_or(""),
                harness_session.as_deref().unwrap_or("")
            );
            let t = tracks.entry(session.clone()).or_insert_with(|| Track {
                key: String::new(),
                reader: None,
                looked: None,
                sent: kept.clone().unwrap_or_default(),
            });
            if t.key != key {
                t.key = key;
                t.reader = None;
                t.looked = None;
            }
            if t.reader.is_none() && t.looked.is_none_or(|at| at.elapsed() >= RETRY) {
                t.looked = Some(Instant::now());
                t.reader = locate(&harness, transcript.as_deref(), harness_session.as_deref())
                    .map(|(src, p)| Reader::new(src, p));
            }
            let Some(r) = t.reader.as_mut() else { continue };
            let from = since.get(&session).copied().unwrap_or(running[&session]);
            let Some(snap) = r.poll(&mut budget, from as i64) else { continue };
            if snap == t.sent {
                continue;
            }
            t.sent = snap.clone();
            out.push(HarnessEvent {
                session,
                harness,
                kind: "plan".into(),
                harness_session: None,
                message: None,
                question_kind: None,
                percent: None,
                at_ms: aaa_common::util::now_ms(),
                transcript: None,
                plan: snap.plan,
                background: snap.background,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    fn file(tag: &str, lines: &[Value]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aaa-plan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("s.jsonl");
        std::fs::write(&p, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        p
    }

    fn append(p: &Path, lines: &[Value]) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(p).unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
    }

    const T: &str = "2026-10-18T09:00:00.000Z";

    fn call(id: &str, name: &str, input: Value) -> Value {
        json!({"type":"assistant","timestamp":T,"message":{"content":[{"type":"tool_use","id":id,"name":name,"input":input}]}})
    }

    fn answer(id: &str, text: &str, result: Value, error: bool) -> Value {
        json!({"type":"user","timestamp":T,"toolUseResult":result,"message":{"content":[{"type":"tool_result","tool_use_id":id,"content":text,"is_error":error}]}})
    }

    fn poll(r: &mut Reader) -> Snapshot {
        r.poll(&mut POLL_BUDGET.clone(), 0).unwrap()
    }

    fn plan(done: u32, total: u32, current: Option<&str>) -> Option<PlanSummary> {
        Some(PlanSummary { done, total, current: current.map(str::to_string) })
    }

    #[test]
    fn claude_tasks_make_a_plan_from_answered_calls_only() {
        let p = file(
            "claude",
            &[
                call("u1", "TaskCreate", json!({"subject":"읽기","activeForm":"읽는 중"})),
                answer("u1", "Task #1 created successfully: 읽기", json!({"task":{"id":"1"}}), false),
                call("u2", "TaskCreate", json!({"subject":"고치기"})),
                answer("u2", "Task #2 created successfully: 고치기", json!(null), false),
                // 오류로 끝난 호출은 단계가 아니다
                call("u3", "TaskCreate", json!({"subject":"실패"})),
                answer("u3", "error", json!(null), true),
                call("u4", "TaskUpdate", json!({"taskId":"1","status":"in_progress"})),
                answer("u4", "Updated", json!({"success":true}), false),
            ],
        );
        let mut r = Reader::new(Source::Claude, p.clone());
        assert_eq!(poll(&mut r).plan, plan(0, 2, Some("읽는 중")));
        // 답이 아직 없는 호출은 바꾸지 않는다
        append(&p, &[call("u5", "TaskUpdate", json!({"taskId":"1","status":"completed"}))]);
        assert_eq!(poll(&mut r).plan, plan(0, 2, Some("읽는 중")));
        append(&p, &[answer("u5", "Updated", json!({"success":true}), false)]);
        assert_eq!(poll(&mut r).plan, plan(1, 2, None));
        // 실패한 갱신·지운 단계
        append(
            &p,
            &[
                call("u6", "TaskUpdate", json!({"taskId":"2","status":"completed"})),
                answer("u6", "no", json!({"success":false}), false),
                call("u7", "TaskUpdate", json!({"taskId":"2","status":"deleted"})),
                answer("u7", "Updated", json!({"success":true}), false),
            ],
        );
        assert_eq!(poll(&mut r).plan, plan(1, 1, None));
        // 모두 끝난 뒤 처음 만든 단계는 새 계획이다
        append(
            &p,
            &[
                call("u8", "TaskCreate", json!({"subject":"다음"})),
                answer("u8", "Task #3 created successfully: 다음", json!(null), false),
            ],
        );
        assert_eq!(poll(&mut r).plan, plan(0, 1, None));
    }

    #[test]
    fn codex_plan_is_the_last_update_plan() {
        let up = |items: Value| json!({"type":"response_item","payload":{"type":"function_call","name":"update_plan","arguments":json!({"plan":items}).to_string()}});
        let p = file(
            "codex",
            &[
                up(json!([{"step":"a","status":"in_progress"},{"step":"b","status":"pending"}])),
                // 명령이 계획을 인용한 것은 호출이 아니다
                json!({"type":"response_item","payload":{"type":"function_call","name":"exec","arguments":"update_plan"}}),
                up(
                    json!([{"step":"a","status":"completed"},{"step":"b","status":"in_progress"},{"step":"c","status":"pending"}]),
                ),
            ],
        );
        let mut r = Reader::new(Source::Codex, p);
        assert_eq!(poll(&mut r).plan, plan(1, 3, Some("b")));
    }

    #[test]
    fn background_counts_agents_and_commands_until_their_notice() {
        let note = |task: &str, tool: &str| {
            format!(
                "<task-notification>\n<task-id>{task}</task-id>\n<tool-use-id>{tool}</tool-use-id>\n<status>completed</status>\n<summary>끝</summary>\n</task-notification>"
            )
        };
        let p = file(
            "bg",
            &[
                // 배경 명령
                call("b1", "Bash", json!({"command":"cargo test","run_in_background":true})),
                answer("b1", "Command running in background with ID: bg1", json!({"backgroundTaskId":"bg1"}), false),
                // 배경 하위 agent
                call("a1", "Agent", json!({"description":"찾기","run_in_background":true})),
                answer(
                    "a1",
                    "Async agent launched successfully.",
                    json!({"status":"async_launched","agentId":"ag1"}),
                    false,
                ),
                // 동기 하위 agent(답이 오면 끝)
                call("a2", "Agent", json!({"description":"읽기"})),
            ],
        );
        let mut r = Reader::new(Source::Claude, p.clone());
        assert_eq!(poll(&mut r).background, Some(3));
        append(&p, &[answer("a2", "결과", json!({"status":"completed"}), false)]);
        assert_eq!(poll(&mut r).background, Some(2));
        // 알림은 세 자리에 실려 온다: queue-operation, 사람 쪽 글. 도구 답이 인용한 것은 알림이 아니다
        append(
            &p,
            &[
                answer("zz", &note("bg1", "b1"), json!(null), false),
                json!({"type":"queue-operation","operation":"enqueue","timestamp":T,"content":note("ag1", "a1")}),
            ],
        );
        assert_eq!(poll(&mut r).background, Some(1));
        append(&p, &[json!({"type":"user","timestamp":T,"message":{"content":note("bg1", "b1")}})]);
        assert_eq!(poll(&mut r).background, None);
        // TaskStop으로 멈춘 명령, 지금 프로세스보다 먼저 띄운 명령
        append(
            &p,
            &[
                call("b2", "Bash", json!({"command":"sleep 99"})),
                answer("b2", "running", json!({"backgroundTaskId":"bg2"}), false),
                call("s1", "TaskStop", json!({"task_id":"bg2"})),
                answer("s1", "stopped", json!({"task_id":"bg2","task_type":"local_bash"}), false),
                call("b3", "Bash", json!({"command":"sleep 99"})),
                answer("b3", "running", json!({"backgroundTaskId":"bg3"}), false),
            ],
        );
        assert_eq!(poll(&mut r).background, Some(1));
        let later = stamp_ms(T).unwrap() + 1;
        assert_eq!(r.poll(&mut POLL_BUDGET.clone(), later).unwrap().background, None);
    }

    #[test]
    fn first_read_waits_for_the_end_and_long_lines_are_skipped() {
        let blob = json!({"type":"api-request-blob","data":"x".repeat((READ_BUDGET + 10) as usize)});
        let p = file(
            "long",
            &[
                blob,
                call("u1", "TaskCreate", json!({"subject":"한 단계"})),
                answer("u1", "Task #1 created successfully: x", json!(null), false),
            ],
        );
        let mut r = Reader::new(Source::Claude, p);
        // 한 번에 다 못 읽으면 말하지 않는다
        assert_eq!(r.poll(&mut 1024, 0), None);
        assert_eq!(poll(&mut r).plan, plan(0, 1, None));
    }

    #[test]
    fn stamps_are_utc_ms() {
        assert_eq!(stamp_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(stamp_ms("2026-10-18T09:17:01.123Z"), Some(1_792_315_021_123));
        assert_eq!(stamp_ms("2000-02-29T12:00:00.5Z"), Some(951_825_600_500));
        assert_eq!(stamp_ms("2026-10-18 09:17:01Z"), None);
    }
}
