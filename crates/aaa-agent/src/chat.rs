//! harness 대화 기록 보기(§8.27, 읽기 전용). Claude·Codex가 남긴 jsonl 기록의 끝부분에서 사람이 읽을 줄만 골라 낸다.
//!
//! - 경로는 믿지 않는다. hook은 세션 안에서 누구나 보낼 수 있다. 그래서 저장된 실행 spec의 계정 홈·CODEX_HOME 또는
//!   agent 환경의 기록 폴더(`$CLAUDE_CONFIG_DIR`|`~/.claude`의 `projects/`, `$CODEX_HOME`|`~/.codex`의 `sessions/`) 안의
//!   일반 `.jsonl` 파일만 읽는다.
//!   symlink는 실제 위치로 풀어 본다.
//! - 터미널보다 많이 보이지 않는다: 도구 결과 본문·하위 agent(sidechain)·시스템이 끼운 글은 뺀다. 생각(thinking)은 웹 「생각 표시」를
//!   켠 화면이 청할 때만 싣는다(herdr-web-ui `showThinking`, plan §8.129).
//! - 기록은 byte/블록 cursor로 앞으로 읽는다(chat_pages). host는 별도 대화 사본을 보관하지 않는다.

use std::io::{BufRead, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use aaa_proto::{ChatMessage, ChatRole};
use serde_json::Value;

#[path = "chat_pages.rs"]
mod pages;

const MAX_CHARS: usize = 4000;
const CHUNK: u64 = 1 << 20;

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Claude 설정 폴더(`$CLAUDE_CONFIG_DIR`|`~/.claude`). agent는 읽기만 한다
pub(crate) fn claude_home() -> PathBuf {
    env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| aaa_config::paths::home().join(".claude"))
}

pub(crate) fn claude_root() -> PathBuf {
    claude_home().join("projects")
}

/// Codex 설정 폴더(`$CODEX_HOME`|`~/.codex`)
pub(crate) fn codex_home() -> PathBuf {
    env_dir("CODEX_HOME").unwrap_or_else(|| aaa_config::paths::home().join(".codex"))
}

pub(crate) fn codex_root() -> PathBuf {
    codex_home().join("sessions")
}

/// 기록 생성 전 상태는 실제 읽기·경로 오류와 구분해 요청 응답에서만 빈 대화로 바꾼다.
#[derive(Clone, Debug)]
pub(crate) enum Error {
    Missing,
    Failed(String),
}
impl Error {
    pub(super) fn io(error: std::io::Error, context: &str) -> Self {
        if error.kind() == std::io::ErrorKind::NotFound {
            Self::Missing
        } else {
            Self::Failed(aaa_common::diagnostic::context(error, context.to_string()))
        }
    }
}
impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}
impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Failed(message.into())
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("대화 기록이 아직 없다"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

/// root 안의 일반 `.jsonl` 파일인지 실제 위치로 확인한다
pub(crate) fn inside(root: &Path, path: &Path) -> Result<PathBuf, Error> {
    let deny = || Error::from("기록 폴더 밖의 파일은 읽지 않는다");
    if !path.is_absolute() || path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Err(deny());
    }
    let resolve = |target: &Path, context| {
        std::fs::canonicalize(target).map_err(|e| {
            // 없는 경로도 명백한 외부·상위 경로 요청은 빈 대화로 숨기지 않는다.
            if e.kind() == std::io::ErrorKind::NotFound
                && (!path.starts_with(root) || path.components().any(|c| c == std::path::Component::ParentDir))
            {
                deny()
            } else {
                Error::io(e, context)
            }
        })
    };
    let real_root = resolve(root, "기록 폴더를 열지 못했다")?;
    let real = resolve(path, "기록 파일을 열지 못했다")?;
    if !real.starts_with(&real_root) || real.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Err(deny());
    }
    if !std::fs::metadata(&real).map_err(|e| Error::io(e, "기록 파일을 확인하지 못했다"))?.is_file() {
        return Err(deny());
    }
    Ok(real)
}

/// Codex 0.163은 되돌린 기록에 `_<rollout-id>`를 붙인다. thread ID와 기록 ID를 섞지 않는다.
pub(crate) fn codex_record_name(path: &Path, id: &str) -> bool {
    if !aaa_common::util::harness_session_ok(id) {
        return false;
    }
    let Some(name) = path.file_name().and_then(|s| s.to_str()).and_then(|s| s.strip_prefix("rollout-")) else {
        return false;
    };
    let Some(stem) = name.strip_suffix(".jsonl") else { return false };
    stem.ends_with(&format!("-{id}"))
        || stem.rsplit_once('_').is_some_and(|(thread, rollout)| {
            thread.ends_with(&format!("-{id}")) && aaa_common::util::harness_session_ok(rollout)
        })
}

/// Codex 기록: `sessions/YYYY/MM/DD/rollout-…-<thread-id>[_<rollout-id>].jsonl`
pub(crate) fn codex_hint(root: &Path, id: &str, transcript: &str) -> Option<PathBuf> {
    if !aaa_common::util::harness_session_ok(id) {
        return None;
    }
    let path = inside(root, Path::new(transcript))
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?;
    codex_record_name(&path, id).then_some(path)
}

/// 경로를 모르는 hook·옛 세션은 ID로 찾는다. 셸에서 확인한 정확한 경로가 있으면 `codex_hint`를 먼저 쓴다.
pub(crate) fn codex_file(root: &Path, id: &str) -> Result<PathBuf, Error> {
    if !aaa_common::util::harness_session_ok(id) {
        return Err("Codex thread-id 형식이 아니다".into());
    }
    let mut dirs = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let rd = match std::fs::read_dir(&dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(Error::io(e, "기록 폴더를 읽지 못했다")),
        };
        for entry in rd {
            let e = entry.map_err(|e| Error::io(e, "기록 폴더 항목을 읽지 못했다"))?;
            let ft = e.file_type().map_err(|e| Error::io(e, "기록 파일을 확인하지 못했다"))?;
            if ft.is_dir() && depth < 3 {
                dirs.push((e.path(), depth + 1));
            } else if ft.is_file() && codex_record_name(&e.path(), id) {
                return inside(root, &e.path());
            }
        }
    }
    Err(Error::Missing)
}

/// 계정 실행의 전용 홈에서만 ID가 다른 SessionStart를 보완한다. 공유 홈에서는 추측하지 않는다.
/// 같은 실행 이후 시작한 CLI 기록의 cwd를 대조하며, 둘 이상이면 고르지 않는다.
pub(crate) fn codex_account_file(root: &Path, id: &str, cwd: &Path, started_ms: u64) -> Result<PathBuf, Error> {
    match codex_file(root, id) {
        Ok(path) => return Ok(path),
        Err(Error::Missing) => {}
        Err(error) => return Err(error),
    }
    if !aaa_common::util::harness_session_ok(id) {
        return Err("Codex thread-id 형식이 아니다".into());
    }
    let cwd = std::fs::canonicalize(cwd)
        .map_err(|e| aaa_common::diagnostic::context(e, "실행 작업 폴더를 열지 못했다".to_string()))?;
    let mut found = None;
    let mut invalid_record = false;
    let mut dirs = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(dir)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            continue;
        };
        for entry in rd.flatten() {
            let Ok(kind) = entry.file_type().inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            if kind.is_dir() && depth < 3 {
                dirs.push((entry.path(), depth + 1));
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !kind.is_file() || !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(path) = inside(root, &entry.path()).inspect_err(|error| {
                invalid_record |= !matches!(error, Error::Missing);
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            let Ok(file) = std::fs::File::open(&path).inspect_err(|error| {
                invalid_record |= error.kind() != std::io::ErrorKind::NotFound;
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            // session_meta 첫 줄만 읽는다. 손상된 거대 줄·FIFO·symlink를 따라가지 않는다.
            let mut bytes = Vec::new();
            if std::io::BufReader::new(file.take(CHUNK))
                .read_until(b'\n', &mut bytes)
                .as_ref()
                .inspect_err(|error| {
                    invalid_record = true;
                    aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                })
                .is_err()
                || bytes.last() != Some(&b'\n')
            {
                continue;
            }
            let Ok(meta) = serde_json::from_slice::<Value>(&bytes).inspect_err(|error| {
                invalid_record = true;
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            let p = &meta["payload"];
            if meta["type"] != "session_meta"
                || p["source"] != "cli"
                || !p["timestamp"].as_str().and_then(aaa_common::util::parse_iso).is_some_and(|t| t >= started_ms)
                || !p["id"].as_str().is_some_and(|s| codex_record_name(&path, s))
                || !p["cwd"]
                    .as_str()
                    .and_then(|s| {
                        std::fs::canonicalize(s)
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()
                    })
                    .is_some_and(|p| p == cwd)
            {
                continue;
            }
            if found.replace(path).is_some() {
                return Err("이 실행의 Codex 기록 후보가 여러 개라 고르지 못했다".into());
            }
        }
    }
    match found {
        Some(path) => Ok(path),
        // 읽기 실패·손상은 오류로 남기고, 무관한 이전 기록만 있으면 새 기록은 아직 없다.
        None if invalid_record => Err("Codex 기록 파일을 찾지 못했다".into()),
        None => Err(Error::Missing),
    }
}

fn clip(t: &str) -> String {
    let t = t.trim();
    if t.chars().count() <= MAX_CHARS {
        return t.to_string();
    }
    t.chars().take(MAX_CHARS).collect::<String>() + "…"
}

fn one_line(t: &str, n: usize) -> String {
    let flat: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= n { flat } else { flat.chars().take(n).collect::<String>() + "…" }
}

/// 도구 호출 한 줄: 이름과 대표 인자 하나
fn tool_line(name: &str, input: &Value) -> String {
    let arg = ["command", "cmd", "file_path", "path", "pattern", "url", "query", "description", "prompt"]
        .iter()
        .find_map(|k| match input.get(*k) {
            Some(Value::String(s)) => Some(s.clone()),
            Some(Value::Array(a)) => Some(a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" ")),
            _ => None,
        });
    match arg {
        Some(a) if !a.is_empty() => format!("{name}: {}", one_line(&a, 160)),
        _ => name.to_string(),
    }
}

/// 사람이 쓴 글이 아니라 harness·시스템이 끼운 글
fn injected(t: &str) -> bool {
    let t = t.trim_start();
    [
        "<system-reminder>",
        "<command-",
        "<local-command-",
        "<user-prompt-submit-hook>",
        "<environment_context>",
        "<turn_aborted>",
        "<user_instructions>",
        "<permissions",
        "Caveat:",
    ]
    .iter()
    .any(|p| t.starts_with(p))
}

/// Claude의 역할은 사람이 썼다는 증거가 아니다. 2.1.303은 배경 작업·peer도 user로 기록한다.
/// 구조 표지가 있으면 글 모양보다 우선한다. 사람이 인용한 XML·붙여넣기 봉투는 그대로 둔다.
pub(crate) fn claude_human(v: &Value, text: &str) -> bool {
    if ["isSidechain", "isMeta", "isCompactSummary", "isSynthetic"].iter().any(|key| v[*key] == true) {
        return false;
    }
    match v["origin"]["kind"].as_str() {
        Some(kind) => kind == "human",
        // 옛 기록·queue-operation에는 origin이 없다. 봉투를 걷어 본문을 추측하지 않는다.
        None => !injected(text) && !text.trim_start().starts_with("<task-notification>"),
    }
}

fn msg(role: ChatRole, text: &str) -> Option<ChatMessage> {
    let receipt = (role == ChatRole::User).then(|| {
        use sha2::{Digest, Sha256};
        let canonical = aaa_proto::input::receipt_text(text);
        aaa_proto::ChatReceipt {
            sha256: format!("{:x}", Sha256::digest(canonical.as_bytes())),
            method: if canonical != text.replace("\r\n", "\n").replace('\r', "\n").trim() {
                aaa_proto::InputReceiptMethod::PastedContent
            } else {
                aaa_proto::InputReceiptMethod::Normal
            },
        }
    });
    let text = clip(text);
    (!text.is_empty()).then_some(ChatMessage { id: None, role, text, pending: false, receipt })
}

/// Claude 기록 한 줄 → 보일 줄(0개 이상, 순서대로)
fn claude_line(v: &Value, thinking: bool) -> Vec<ChatMessage> {
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if v.get("isSidechain").and_then(|b| b.as_bool()) == Some(true)
        || v.get("isMeta").and_then(|b| b.as_bool()) == Some(true)
        || v.get("isCompactSummary").and_then(|b| b.as_bool()) == Some(true)
        || (ty != "assistant" && v.get("isSynthetic").and_then(|b| b.as_bool()) == Some(true))
    {
        return vec![];
    }
    // herdr-web-ui conversation.ts parseClaudeTranscript: 작업 중에 흡수한 사람 입력은
    // user가 아니라 queued_command attachment다. enqueue/remove는 같은 글의 장부라 싣지 않는다.
    if ty == "attachment" {
        let a = &v["attachment"];
        if a["type"] == "queued_command" && a["commandMode"] == "prompt" && a["origin"]["kind"] == "human" {
            return a["prompt"]
                .as_str()
                .filter(|s| claude_human(a, s))
                .and_then(|s| msg(ChatRole::User, s))
                .into_iter()
                .collect();
        }
        return vec![];
    }
    if !matches!(ty, "user" | "assistant") {
        return vec![];
    }
    let content = &v["message"]["content"];
    let mut out = Vec::new();
    match (ty, content) {
        ("user", Value::String(s)) if claude_human(v, s) => out.extend(msg(ChatRole::User, s)),
        (_, Value::Array(blocks)) => {
            for b in blocks {
                match (ty, b.get("type").and_then(|t| t.as_str())) {
                    ("user", Some("text")) => {
                        let t = b["text"].as_str().unwrap_or("");
                        if claude_human(v, t) {
                            out.extend(msg(ChatRole::User, t));
                        }
                    }
                    ("user", Some("image")) if claude_human(v, "") => out.extend(msg(ChatRole::User, "[그림]")),
                    ("assistant", Some("text")) => {
                        out.extend(msg(ChatRole::Assistant, b["text"].as_str().unwrap_or("")))
                    }
                    ("assistant", Some("tool_use")) => {
                        let line = tool_line(b["name"].as_str().unwrap_or("tool"), &b["input"]);
                        out.extend(msg(ChatRole::Tool, &line));
                    }
                    ("assistant", Some("thinking")) if thinking => {
                        out.extend(msg(ChatRole::Thinking, b["thinking"].as_str().unwrap_or("")))
                    }
                    // 청하지 않은 thinking, 가린 생각(redacted_thinking)·tool_result·그 밖의 블록은 싣지 않는다
                    _ => {}
                }
            }
        }
        _ => {}
    }
    out
}

/// Codex 기록 한 줄 → 보일 줄
fn codex_line(v: &Value, thinking: bool) -> Vec<ChatMessage> {
    if v.get("type").and_then(|t| t.as_str()) != Some("response_item") {
        return vec![];
    }
    let p = &v["payload"];
    match p.get("type").and_then(|t| t.as_str()) {
        Some("message") => {
            let role = match p.get("role").and_then(|r| r.as_str()) {
                Some("user") => ChatRole::User,
                Some("assistant") => ChatRole::Assistant,
                _ => return vec![],
            };
            let text: Vec<&str> = p["content"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter(|c| matches!(c["type"].as_str(), Some("input_text" | "output_text")))
                        .filter_map(|c| c["text"].as_str())
                        .filter(|t| !injected(t))
                        .collect()
                })
                .unwrap_or_default();
            msg(role, &text.join("\n")).into_iter().collect()
        }
        Some("function_call") => {
            let args: Value = p["arguments"]
                .as_str()
                .and_then(|a| {
                    serde_json::from_str(a)
                        .inspect_err(|error| {
                            aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                        })
                        .ok()
                })
                .unwrap_or(Value::Null);
            msg(ChatRole::Tool, &tool_line(p["name"].as_str().unwrap_or("tool"), &args)).into_iter().collect()
        }
        // 생각은 요약(summary_text)만 남는다(본문은 암호화되어 있다)
        Some("reasoning") if thinking => {
            let text: Vec<&str> = p["summary"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter(|c| c["type"].as_str() == Some("summary_text"))
                        .filter_map(|c| c["text"].as_str())
                        .collect()
                })
                .unwrap_or_default();
            msg(ChatRole::Thinking, &text.join("\n")).into_iter().collect()
        }
        _ => vec![],
    }
}

/// Claude 대기열에 남은 사람 글(작업 중에 보내 아직 흡수되지 않은 것, plan §8.247). Claude Code는 작업 중에 받은 글을 바로
/// `queue-operation enqueue`로 적고, 흡수하면 같은 글의 `remove`(absorbed_mid_turn)와 `queued_command`를, 한가할 때 바로 꺼내면
/// 글 없는 `dequeue`(맨 앞)를 적는다. 그래서 끝 1MiB를 앞에서부터 읽어 대기열을 다시 세우고 남은 사람 글만 낸다(배경 작업
/// 알림·끼운 글은 뺀다). 그 앞에서 들어가 아직 남은 글은 놓친다: 웹은 보낸 글을 따로 기다린다.
fn claude_pending(path: &Path, history: &str) -> std::io::Result<Vec<ChatMessage>> {
    let mut f = std::fs::File::open(path)?;
    let start = f.metadata()?.len().saturating_sub(CHUNK);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let mut lines = buf.split(|b| *b == b'\n');
    let mut offset = start;
    if start > 0
        && let Some(partial) = lines.next()
    {
        offset += partial.len() as u64 + 1;
    }
    // 글이 아닌 enqueue도 자리를 차지한다(dequeue가 맨 앞을 꺼낸다)
    let mut queue: std::collections::VecDeque<(Option<String>, bool, u64)> = std::collections::VecDeque::new();
    for line in lines {
        let enqueue_offset = offset;
        offset += line.len() as u64 + 1;
        if !line.windows(15).any(|w| w == b"queue-operation") {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            continue;
        };
        if v["type"] != "queue-operation" {
            continue;
        }
        let content = v["content"].as_str();
        match v["operation"].as_str() {
            Some("enqueue") => queue.push_back((
                content.map(str::to_string),
                content.is_some_and(|c| claude_human(&v, c)),
                enqueue_offset,
            )),
            Some("dequeue") => {
                queue.pop_front();
            }
            Some("remove") => {
                if let Some(i) = queue.iter().position(|(q, _, _)| q.as_deref() == content) {
                    queue.remove(i);
                }
            }
            _ => {}
        }
    }
    Ok(queue
        .into_iter()
        .filter(|(_, human, _)| *human)
        .filter_map(|(text, _, offset)| {
            let m = msg(ChatRole::User, &text?)?;
            Some(ChatMessage { id: Some(format!("{history}:{offset:020}:queue")), pending: true, ..m })
        })
        .collect())
}

/// 기록 한 줄 → 보일 줄(둘째 인자: 생각도 싣는다)
type LineParser = fn(&Value, bool) -> Vec<ChatMessage>;

/// 읽을 기록 파일(읽기와 감시가 같은 파일을 본다). 오류 글에 경로를 넣지 않는다(홈 경로가 웹으로 간다).
pub(crate) fn path(harness: &str, transcript: Option<&str>, harness_session: Option<&str>) -> Result<PathBuf, Error> {
    match harness {
        "claude" => {
            let t = transcript.ok_or("Claude가 아직 기록 위치를 알리지 않았다(첫 hook 사건 전)")?;
            inside(&claude_root(), Path::new(t))
        }
        "codex" => {
            let id = harness_session.ok_or("Codex가 아직 thread-id를 알리지 않았다(첫 턴이 끝나기 전)")?;
            codex_file(&codex_root(), id)
        }
        _ => Err("대화 기록은 Claude·Codex pane만 볼 수 있다".into()),
    }
}

/// 대화가 정리된 자리(plan §8.309). herdr-web-ui conversation.ts가 Claude `isCompactSummary` 사람 줄을 「정리 요약」 구분선으로
/// 보이는 것을 옮기고, Codex rollout의 `compacted`(정리 뒤 남은 기록과 요약 message)도 같은 자리로 본다. 요약은 4000자까지
fn compaction(harness: &str, v: &Value) -> Option<ChatMessage> {
    let text = match harness {
        "codex" if v["type"] == "compacted" => v["payload"]["message"].as_str().unwrap_or("").to_string(),
        "claude" if v["isCompactSummary"] == true && v["type"] == "user" => match &v["message"]["content"] {
            Value::String(s) => s.clone(),
            Value::Array(blocks) => blocks
                .iter()
                .filter(|b| b["type"] == "text")
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        },
        _ => return None,
    };
    Some(ChatMessage { id: None, role: ChatRole::Compact, text: clip(&text), pending: false, receipt: None })
}

/// 사람이 친 harness 명령과 그 결과(plan §8.323). Claude 2.1.304는 `system/local_command`에, 앞 판과 `/compact`는 사람 줄에 같은
/// 봉투(`<command-name>`·`<local-command-stdout>`)를 적는다. `/compact`는 정리 자리가 대신하므로 뺀다. `informational`은 「Unknown
/// command: /x」 같은 harness 알림이다. Codex는 명령을 기록하지 않는다(실측 0.163)
fn claude_command(v: &Value) -> Option<ChatMessage> {
    let text = match (v["type"].as_str()?, v["subtype"].as_str()) {
        ("system", Some("local_command")) => v["content"].as_str()?,
        ("system", Some("informational")) => return msg(ChatRole::CommandOutput, &plain(v["content"].as_str()?)),
        ("user", _) if v["isMeta"] != true && v["isSidechain"] != true => v["message"]["content"].as_str()?,
        _ => return None,
    };
    let text = text.trim_start();
    if let Some(name) = tag(text, "command-name") {
        if name.trim() == "/compact" {
            return None;
        }
        let args = tag(text, "command-args").unwrap_or_default();
        return msg(ChatRole::Command, &format!("{} {}", name.trim(), args.trim()));
    }
    let out = tag(text, "local-command-stdout").or_else(|| tag(text, "local-command-stderr"))?;
    let out = plain(&out);
    // `/compact`의 「Compacted (ctrl+o to see full summary)」
    if out.trim().starts_with("Compacted (") {
        return None;
    }
    msg(ChatRole::CommandOutput, &out)
}

/// `<이름>…</이름>` 안 글
fn tag(text: &str, name: &str) -> Option<String> {
    let start = text.find(&format!("<{name}>"))? + name.len() + 2;
    let end = text[start..].find(&format!("</{name}>"))? + start;
    Some(text[start..end].to_string())
}

/// 터미널 색·커서 코드(ESC [ … 끝 글자, ESC ] … BEL)를 뺀 글
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// 기록 한 줄 → 보일 줄. 정리된 자리(`compact`)·harness 명령(`commands`)은 청했을 때만
fn line(harness: &str, v: &Value, thinking: bool, compact: bool, commands: bool) -> Vec<ChatMessage> {
    let parse: LineParser = if harness == "codex" { codex_line } else { claude_line };
    if let Some(m) = compaction(harness, v).filter(|_| compact) {
        return vec![m];
    }
    if commands
        && harness == "claude"
        && let Some(m) = claude_command(v)
    {
        return vec![m];
    }
    parse(v, thinking)
}

/// agent 요청 처리: 검증한 pane 기록 경로만 읽는다.
pub(crate) fn read_path(
    harness: &str,
    path: PathBuf,
    thinking: bool,
    compact: bool,
    commands: bool,
    before: Option<&str>,
) -> Result<pages::Page, Error> {
    let failed = |e| Error::io(e, "기록을 읽지 못했다");
    let mut page = pages::read(&path, before, |v| line(harness, v, thinking, compact, commands))?;
    if harness == "claude" && before.is_none() {
        page.messages.extend(claude_pending(&path, &page.history).map_err(failed)?);
    }
    Ok(page)
}

/// 기록 파일이 바뀌었는지 보는 값(참고 conversation-monitor.ts `fileIdentity`): 장치·inode·크기·고친 시각. 없으면 `missing`
pub fn identity(path: &Path) -> String {
    let Ok(m) = std::fs::metadata(path)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return "missing".into();
    };
    let modified = m
        .modified()
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        })
        .map_or(0, |d| d.as_nanos());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{}:{modified}:{}.{}", m.dev(), m.ino(), m.len(), m.ctime(), m.ctime_nsec())
    }
    #[cfg(not(unix))]
    format!("{}:{modified}", m.len())
}

#[cfg(test)]
mod tests;
