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

/// root 안의 일반 `.jsonl` 파일인지 실제 위치로 확인한다
pub(crate) fn inside(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let deny = || "기록 폴더 밖의 파일은 읽지 않는다".to_string();
    if !path.is_absolute() || path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Err(deny());
    }
    let root = std::fs::canonicalize(root).map_err(|_| "기록 폴더가 없다".to_string())?;
    let real = std::fs::canonicalize(path).map_err(|_| "기록 파일이 없다".to_string())?;
    if !real.starts_with(&root) || real.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Err(deny());
    }
    if !std::fs::metadata(&real).map(|m| m.is_file()).unwrap_or(false) {
        return Err(deny());
    }
    Ok(real)
}

/// Codex 기록: `sessions/YYYY/MM/DD/rollout-…-<thread-id>.jsonl`
pub(crate) fn codex_file(root: &Path, id: &str) -> Result<PathBuf, String> {
    if !aaa_common::util::harness_session_ok(id) {
        return Err("Codex thread-id 형식이 아니다".into());
    }
    let suffix = format!("-{id}.jsonl");
    let mut dirs = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().to_string();
            if ft.is_dir() && depth < 3 {
                dirs.push((e.path(), depth + 1));
            } else if ft.is_file() && name.starts_with("rollout-") && name.ends_with(&suffix) {
                return inside(root, &e.path());
            }
        }
    }
    Err("Codex 기록 파일을 찾지 못했다".into())
}

/// 계정 실행의 전용 홈에서만 ID가 다른 SessionStart를 보완한다. 공유 홈에서는 추측하지 않는다.
/// 같은 실행 이후 시작한 CLI 기록의 cwd를 대조하며, 둘 이상이면 고르지 않는다.
pub(crate) fn codex_account_file(root: &Path, id: &str, cwd: &Path, started_ms: u64) -> Result<PathBuf, String> {
    if let Ok(path) = codex_file(root, id) {
        return Ok(path);
    }
    if !aaa_common::util::harness_session_ok(id) {
        return Err("Codex thread-id 형식이 아니다".into());
    }
    let cwd = std::fs::canonicalize(cwd).map_err(|_| "실행 작업 폴더가 없다")?;
    let mut found = None;
    let mut dirs = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for entry in rd.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() && depth < 3 {
                dirs.push((entry.path(), depth + 1));
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !kind.is_file() || !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(path) = inside(root, &entry.path()) else { continue };
            let Ok(file) = std::fs::File::open(&path) else { continue };
            // session_meta 첫 줄만 읽는다. 손상된 거대 줄·FIFO·symlink를 따라가지 않는다.
            let mut bytes = Vec::new();
            if std::io::BufReader::new(file.take(CHUNK)).read_until(b'\n', &mut bytes).is_err()
                || bytes.last() != Some(&b'\n')
            {
                continue;
            }
            let Ok(meta) = serde_json::from_slice::<Value>(&bytes) else { continue };
            let p = &meta["payload"];
            if meta["type"] != "session_meta"
                || p["source"] != "cli"
                || !p["timestamp"].as_str().and_then(aaa_common::util::parse_iso).is_some_and(|t| t >= started_ms)
                || !p["id"]
                    .as_str()
                    .is_some_and(|s| aaa_common::util::harness_session_ok(s) && name.ends_with(&format!("-{s}.jsonl")))
                || !p["cwd"].as_str().and_then(|s| std::fs::canonicalize(s).ok()).is_some_and(|p| p == cwd)
            {
                continue;
            }
            if found.replace(path).is_some() {
                return Err("이 실행의 Codex 기록 후보가 여러 개라 고르지 못했다".into());
            }
        }
    }
    found.ok_or_else(|| "Codex 기록 파일을 찾지 못했다".into())
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
fn claude_human(v: &Value, text: &str) -> bool {
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
            let args: Value = p["arguments"].as_str().and_then(|a| serde_json::from_str(a).ok()).unwrap_or(Value::Null);
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
fn claude_pending(path: &Path) -> std::io::Result<Vec<ChatMessage>> {
    let mut f = std::fs::File::open(path)?;
    let start = f.metadata()?.len().saturating_sub(CHUNK);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let mut lines = buf.split(|b| *b == b'\n');
    if start > 0 {
        lines.next(); // 앞 줄의 뒷부분
    }
    // 글이 아닌 enqueue도 자리를 차지한다(dequeue가 맨 앞을 꺼낸다)
    let mut queue: std::collections::VecDeque<(Option<String>, bool)> = std::collections::VecDeque::new();
    for line in lines {
        if !line.windows(15).any(|w| w == b"queue-operation") {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else { continue };
        if v["type"] != "queue-operation" {
            continue;
        }
        let content = v["content"].as_str();
        match v["operation"].as_str() {
            Some("enqueue") => {
                queue.push_back((content.map(str::to_string), content.is_some_and(|c| claude_human(&v, c))))
            }
            Some("dequeue") => {
                queue.pop_front();
            }
            Some("remove") => {
                if let Some(i) = queue.iter().position(|(q, _)| q.as_deref() == content) {
                    queue.remove(i);
                }
            }
            _ => {}
        }
    }
    Ok(queue
        .into_iter()
        .filter(|(_, human)| *human)
        .filter_map(|(text, _)| text)
        .filter_map(|c| msg(ChatRole::User, &c))
        .map(|m| ChatMessage { pending: true, ..m })
        .collect())
}

/// 기록 한 줄 → 보일 줄(둘째 인자: 생각도 싣는다)
type LineParser = fn(&Value, bool) -> Vec<ChatMessage>;

/// 읽을 기록 파일(읽기와 감시가 같은 파일을 본다). 오류 글에 경로를 넣지 않는다(홈 경로가 웹으로 간다).
pub fn path(harness: &str, transcript: Option<&str>, harness_session: Option<&str>) -> Result<PathBuf, String> {
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
) -> Result<pages::Page, String> {
    let failed = |e: std::io::Error| format!("기록을 읽지 못했다: {}", e.kind());
    let mut page = pages::read(&path, before, |v| line(harness, v, thinking, compact, commands))?;
    if harness == "claude" && before.is_none() {
        page.messages.extend(claude_pending(&path).map_err(failed)?);
    }
    Ok(page)
}

/// 기록 파일이 바뀌었는지 보는 값(참고 conversation-monitor.ts `fileIdentity`): 장치·inode·크기·고친 시각. 없으면 `missing`
pub fn identity(path: &Path) -> String {
    let Ok(m) = std::fs::metadata(path) else { return "missing".into() };
    let modified =
        m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{}:{modified}:{}.{}", m.dev(), m.ino(), m.len(), m.ctime(), m.ctime_nsec())
    }
    #[cfg(not(unix))]
    format!("{}:{modified}", m.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aaa-chat-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// 합성 기록(실제 기록은 쓰지 않는다): 큰 요청 원문 줄, 하위 agent, 생각, 시스템 글, 도구 결과가 섞여 있다
    fn claude_fixture() -> String {
        let blob = format!(r#"{{"type":"api-request-blob","data":"{}"}}"#, "x".repeat(3 << 20));
        [
            r#"{"type":"user","message":{"role":"user","content":"첫 질문"}}"#.to_string(),
            blob,
            r#"{"type":"user","message":{"role":"user","content":"<system-reminder>끼운 글</system-reminder>"}}"#.into(),
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"meta 글"}}"#.into(),
            r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"하위 agent 글"}]}}"#.into(),
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"속생각"},{"type":"text","text":"답을 쓴다"},{"type":"tool_use","name":"Bash","input":{"command":"cargo test\n  --all"}}]}}"#.into(),
            r#"{"type":"user","message":{"content":[{"type":"tool_result","content":"도구 결과 본문"}]}}"#.into(),
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"끝났다"}]}}"#.into(),
            r#"{"type":"queue-operation","operation":"enqueue","content":"작업 중 추가 요청"}"#.into(),
            r#"{"type":"queue-operation","operation":"remove","content":"작업 중 추가 요청","reason":"absorbed_mid_turn"}"#.into(),
            r#"{"type":"attachment","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"prompt":"작업 중 추가 요청"}}"#.into(),
            r#"{"type":"attachment","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"배경 도구 알림"}}"#.into(),
            r#"{"type":"attachment","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"peer"},"prompt":"다른 agent 글"}}"#.into(),
        ]
        .join("\n")
            + "\n"
    }

    #[test]
    fn claude_tail_filters() {
        let root = dir("claude");
        let f = root.join("s.jsonl");
        std::fs::write(&f, claude_fixture()).unwrap();
        let page = pages::read(&f, None, |v| claude_line(v, false)).unwrap();
        let truncated = page.before.is_some();
        let m = page.messages;
        let got: Vec<_> = m.iter().map(|x| (x.role, x.text.as_str())).collect();
        assert_eq!(
            got,
            [
                (ChatRole::User, "첫 질문"),
                (ChatRole::Assistant, "답을 쓴다"),
                (ChatRole::Tool, "Bash: cargo test --all"),
                (ChatRole::Assistant, "끝났다"),
                (ChatRole::User, "작업 중 추가 요청"),
            ]
        );
        assert!(!truncated);
        // 「생각 표시」를 켠 화면이 청하면 생각도 제자리에
        let m = pages::read(&f, None, |v| claude_line(v, true)).unwrap().messages;
        let got: Vec<_> = m.iter().map(|x| (x.role, x.text.as_str())).collect();
        assert_eq!(&got[1..3], [(ChatRole::Thinking, "속생각"), (ChatRole::Assistant, "답을 쓴다")]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn claude_user_origin_separates_notifications_from_literal_receipts() {
        use serde_json::json;
        let notification = "<task-notification><task-id>task-1</task-id><status>completed</status><summary>작업 끝</summary></task-notification>";
        let reminder = "<system-reminder>사람이 인용한 예시</system-reminder>";
        for content in [json!(notification), json!([{"type":"text","text":notification}])] {
            for kind in ["task-notification", "peer"] {
                let record = json!({"type":"user","origin":{"kind":kind},"message":{"role":"user","content":content}});
                assert!(claude_line(&record, false).is_empty(), "내부 알림은 user receipt가 아니다: {kind}");
            }
        }
        for text in [notification, reminder] {
            let record = json!({"type":"user","origin":{"kind":"human"},"message":{"content":text}});
            let messages = claude_line(&record, false);
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].text, text);
            assert_eq!(messages[0], msg(ChatRole::User, text).unwrap(), "사람의 원문과 수신 hash 보존");
            let queued = json!({"type":"attachment","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"prompt":text}});
            assert_eq!(claude_line(&queued, false), messages);
        }
        for flag in ["isMeta", "isCompactSummary", "isSynthetic"] {
            for mut record in [
                serde_json::json!({"type":"user","origin":{"kind":"human"},"message":{"content":"내부 안내"}}),
                serde_json::json!({"type":"attachment","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"prompt":"내부 안내"}}),
            ] {
                record[flag] = json!(true);
                assert!(claude_line(&record, false).is_empty(), "구조 필드가 글 모양보다 우선한다: {flag}");
            }
        }
        // 비사람 origin은 표지 없는 평문이어도 사람이 쓴 글이 아니다.
        assert!(
            claude_line(
                &json!({"type":"user","origin":{"kind":"task-notification"},"message":{"content":"작업 끝"}}),
                false
            )
            .is_empty()
        );
    }

    #[test]
    fn claude_pending_queue() {
        let root = dir("pending");
        let f = root.join("s.jsonl");
        let op = |op: &str, content: Option<&str>| match content {
            Some(c) => format!(r#"{{"type":"queue-operation","operation":"{op}","content":{}}}"#, serde_json::json!(c)),
            None => format!(r#"{{"type":"queue-operation","operation":"{op}"}}"#),
        };
        let lines = [
            // 숨긴 알림도 remove가 같은 원문으로 찾아야 다음 dequeue가 사람 글을 꺼낸다.
            r#"{"type":"queue-operation","operation":"enqueue","origin":{"kind":"task-notification"},"content":"먼저 끝난 알림"}"#.into(),
            op("remove", Some("먼저 끝난 알림")),
            op("enqueue", Some("먼저 꺼낸 글")),
            op("dequeue", None),
            // 한가할 때 보낸 글: 넣자마자 꺼낸다
            op("enqueue", Some("바로 간 글")),
            op("dequeue", None),
            // 작업 중에 보내 흡수된 글
            op("enqueue", Some("흡수된 글")),
            op("remove", Some("흡수된 글")),
            // 아직 남은 글 둘과 배경 작업 알림
            op("enqueue", Some("<task-notification>\n<task-id>b1</task-id>")),
            op("enqueue", Some("기다리는 글")),
            op("enqueue", Some("기다리는 글 둘")),
            // dequeue는 맨 앞(알림)을 꺼낸다
            op("dequeue", None),
            r#"{"type":"queue-operation","operation":"enqueue","origin":{"kind":"human"},"content":"<system-reminder>사람의 예시</system-reminder>"}"#.into(),
            r#"{"type":"queue-operation","operation":"enqueue","origin":{"kind":"task-notification"},"content":"평문 내부 알림"}"#.into(),
        ];
        std::fs::write(&f, lines.join("\n") + "\n").unwrap();
        let got: Vec<_> = claude_pending(&f).unwrap().into_iter().map(|m| (m.role, m.text, m.pending)).collect();
        assert_eq!(
            got,
            [
                (ChatRole::User, "기다리는 글".to_string(), true),
                (ChatRole::User, "기다리는 글 둘".to_string(), true),
                (ChatRole::User, "<system-reminder>사람의 예시</system-reminder>".to_string(), true)
            ]
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn long_tail_is_truncated() {
        let root = dir("long");
        let f = root.join("s.jsonl");
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"줄"}]}}"#;
        std::fs::write(&f, format!("{line}\n").repeat(400)).unwrap();
        let page = pages::read(&f, None, |v| claude_line(v, false)).unwrap();
        let truncated = page.before.is_some();
        let m = page.messages;
        assert_eq!((m.len(), truncated), (150, true));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn codex_lines() {
        let user = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"고쳐 줘"}]}}"#;
        let env = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>cwd</environment_context>"}]}}"#;
        let dev = r#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"규칙"}]}}"#;
        let call = r#"{"type":"response_item","payload":{"type":"function_call","name":"shell","arguments":"{\"command\":[\"bash\",\"-lc\",\"ls\"]}"}}"#;
        let think = r#"{"type":"response_item","payload":{"type":"reasoning","summary":[{"type":"summary_text","text":"목록부터 본다"}],"encrypted_content":"gAAA"}}"#;
        let out = r#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"했다"}]}}"#;
        let interrupted = r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<turn_aborted>\nThe user interrupted the previous turn on purpose.\n</turn_aborted>"}]}}"#;
        let lines = |thinking: bool| -> Vec<_> {
            [user, env, interrupted, dev, think, call, out]
                .iter()
                .flat_map(|l| codex_line(&serde_json::from_str(l).unwrap(), thinking))
                .map(|m| (m.role, m.text))
                .collect()
        };
        assert_eq!(
            lines(false),
            [
                (ChatRole::User, "고쳐 줘".to_string()),
                (ChatRole::Tool, "shell: bash -lc ls".to_string()),
                (ChatRole::Assistant, "했다".to_string())
            ]
        );
        assert_eq!(lines(true)[1], (ChatRole::Thinking, "목록부터 본다".to_string()));
    }

    #[test]
    fn paths_stay_inside() {
        let root = dir("root");
        let projects = root.join("projects");
        std::fs::create_dir_all(projects.join("p")).unwrap();
        let ok = projects.join("p/s.jsonl");
        std::fs::write(&ok, "").unwrap();
        assert!(inside(&projects, &ok).is_ok());
        // 밖의 파일, 상대 경로, 확장자, .. 로 빠져나가기
        let outside = root.join("secret.jsonl");
        std::fs::write(&outside, "").unwrap();
        assert!(inside(&projects, &outside).is_err());
        assert!(inside(&projects, Path::new("p/s.jsonl")).is_err());
        assert!(inside(&projects, &projects.join("p/../../secret.jsonl")).is_err());
        let txt = projects.join("p/s.txt");
        std::fs::write(&txt, "").unwrap();
        assert!(inside(&projects, &txt).is_err());
        #[cfg(unix)]
        {
            // 안에 둔 symlink가 밖을 가리키면 거부, 폴더도 거부
            std::os::unix::fs::symlink(&outside, projects.join("p/link.jsonl")).unwrap();
            assert!(inside(&projects, &projects.join("p/link.jsonl")).is_err());
            std::fs::create_dir_all(projects.join("p/d.jsonl")).unwrap();
            assert!(inside(&projects, &projects.join("p/d.jsonl")).is_err());
            // FIFO는 일반 파일이 아니다
            let fifo = projects.join("p/f.jsonl");
            let c = std::ffi::CString::new(fifo.to_string_lossy().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            assert!(inside(&projects, &fifo).is_err());
        }
        // Codex: thread-id 형식, 깊이 안의 rollout 파일만
        let sessions = root.join("sessions");
        // 지어낸 id다. 실제 세션 기록의 id를 시험 값으로 옮기지 않는다(공개 저장소, check-public 「실제 세션 id」)
        let id = "01900000-0000-7000-8000-00000000a001";
        std::fs::create_dir_all(sessions.join("2026/10/17")).unwrap();
        let roll = sessions.join(format!("2026/10/17/rollout-2026-10-17T07-00-00-{id}.jsonl"));
        std::fs::write(&roll, "").unwrap();
        assert_eq!(codex_file(&sessions, id).unwrap(), std::fs::canonicalize(&roll).unwrap());
        assert!(codex_file(&sessions, "../../etc").is_err());
        assert!(codex_file(&sessions, "01900000-0000-7000-8000-00000000a002").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 정리된 자리(plan §8.309): Claude `isCompactSummary` 사람 줄(10-20 실측 「This session is being continued …」)과 Codex
    /// `compacted`는 host가 청할 때만 「정리됨」 줄이다. 청하지 않으면 전처럼 아무것도 싣지 않는다(옛 host는 이 역할을 모른다).
    /// 요약 글은 사람 말·받음 증거가 아니다
    #[test]
    fn compaction_rows_only_when_asked() {
        let root = dir("compaction");
        let path = root.join("s.jsonl");
        let summary = "This session is being continued from a previous conversation that ran out of context.";
        std::fs::write(
            &path,
            [
                serde_json::json!({"type":"user","message":{"content":"앞 질문"}}).to_string(),
                serde_json::json!({"type":"system","subtype":"compact_boundary","content":"Conversation compacted"}).to_string(),
                serde_json::json!({"type":"user","isCompactSummary":true,"message":{"content":[{"type":"text","text":summary}]}}).to_string(),
                serde_json::json!({"type":"user","message":{"content":"뒤 질문"}}).to_string(),
            ]
            .join("\n")
                + "\n",
        )
        .unwrap();
        let roles = |compact| -> Vec<(ChatRole, String)> {
            pages::read(&path, None, |v| line("claude", v, false, compact, false))
                .unwrap()
                .messages
                .into_iter()
                .map(|m| (m.role, m.text))
                .collect()
        };
        assert_eq!(
            roles(true),
            [
                (ChatRole::User, "앞 질문".into()),
                (ChatRole::Compact, summary.into()),
                (ChatRole::User, "뒤 질문".into())
            ]
        );
        assert_eq!(roles(false), [(ChatRole::User, "앞 질문".into()), (ChatRole::User, "뒤 질문".into())]);
        let codex = serde_json::json!({"type":"compacted","payload":{"message":"","replacement_history":[{"type":"message","role":"user"}]}});
        let m = compaction("codex", &codex).unwrap();
        assert_eq!((m.role, m.text.as_str(), m.receipt.is_none()), (ChatRole::Compact, "", true));
        assert!(compaction("claude", &codex).is_none());
        assert!(
            compaction("codex", &serde_json::json!({"type":"user","isCompactSummary":true,"message":{"content":"x"}}))
                .is_none()
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// 실측(10-20 Claude 2.1.304): 웹에서 보낸 `/context`·`/model`·`/compact`가 남긴 줄과, 앞 판의 알림 줄
    #[test]
    fn claude_commands_and_their_output_only_when_asked() {
        let echo = |name: &str, args: &str| {
            format!(
                "<command-name>{name}</command-name>\n            <command-message>{}</command-message>\n            <command-args>{args}</command-args>",
                &name[1..]
            )
        };
        let lines = [
            serde_json::json!({"type":"user","message":{"role":"user","content":"Reply with exactly: OK"},"origin":{"kind":"human"}}),
            serde_json::json!({"type":"system","subtype":"local_command","content":echo("/context",""),"level":"info","isMeta":false}),
            serde_json::json!({"type":"system","subtype":"local_command","content":"<local-command-stdout> \u{1b}[1mContext Usage\u{1b}[22m\n\u{1b}[38;2;153;153;153m43.9k/1m tokens (4%)\u{1b}[39m</local-command-stdout>","level":"info","isMeta":false}),
            serde_json::json!({"type":"user","isMeta":true,"message":{"role":"user","content":"## Context Usage\n\n**Tokens:** 43.9k / 1m (4%)"}}),
            serde_json::json!({"type":"system","subtype":"local_command","content":echo("/model","sonnet"),"level":"info","isMeta":false}),
            serde_json::json!({"type":"system","subtype":"local_command","content":"<local-command-stdout>Set model to `Sonnet 5.5`</local-command-stdout>","level":"info","isMeta":false}),
            serde_json::json!({"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>The command below was run directly in Claude Code</local-command-caveat>"}}),
            serde_json::json!({"type":"user","message":{"role":"user","content":echo("/compact","")}}),
            serde_json::json!({"type":"user","message":{"role":"user","content":"<local-command-stdout>\u{1b}[2mCompacted (ctrl+o to see full summary)\u{1b}[22m</local-command-stdout>"}}),
            serde_json::json!({"type":"system","subtype":"informational","content":"Unknown command: /ultraplan","level":"info"}),
        ];
        // 실제 읽기 길(쪽 읽기의 칸 고르기까지)로 읽는다
        let root = std::env::temp_dir().join(format!("aaa-chat-commands-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("s.jsonl");
        std::fs::write(&path, lines.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
        let rows = |commands: bool| -> Vec<(ChatRole, String)> {
            read_path("claude", path.clone(), false, false, commands, None)
                .unwrap()
                .messages
                .into_iter()
                .map(|m| (m.role, m.text))
                .collect()
        };
        assert_eq!(
            rows(true),
            [
                (ChatRole::User, "Reply with exactly: OK".into()),
                (ChatRole::Command, "/context".into()),
                (ChatRole::CommandOutput, "Context Usage\n43.9k/1m tokens (4%)".into()),
                (ChatRole::Command, "/model sonnet".into()),
                (ChatRole::CommandOutput, "Set model to `Sonnet 5.5`".into()),
                (ChatRole::CommandOutput, "Unknown command: /ultraplan".into()),
            ]
        );
        // 청하지 않으면(옛 host·front) 지금처럼 사람 말만
        assert_eq!(rows(false), [(ChatRole::User, "Reply with exactly: OK".into())]);
        // Codex는 명령을 기록하지 않는다: 같은 꼴의 줄이 와도 명령 줄로 읽지 않는다
        assert!(line("codex", &lines[1], false, false, true).is_empty());
        assert_eq!(plain("a\u{1b}]8;;http://x\u{7}link\u{1b}]8;;\u{1b}\\b"), "alinkb");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
