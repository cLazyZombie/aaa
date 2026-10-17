//! harness 대화 기록 보기(§8.27, 읽기 전용). Claude·Codex가 남긴 jsonl 기록의 끝부분에서 사람이 읽을 줄만 골라 낸다.
//!
//! - 경로는 믿지 않는다. hook은 세션 안에서 누구나 보낼 수 있다. 그래서 이 agent 프로세스의 환경으로 정한 기록 폴더
//!   (`$CLAUDE_CONFIG_DIR`|`~/.claude`의 `projects/`, `$CODEX_HOME`|`~/.codex`의 `sessions/`) 안의 일반 `.jsonl` 파일만 읽는다.
//!   symlink는 실제 위치로 풀어 본다.
//! - 터미널보다 많이 보이지 않는다: 도구 결과 본문·하위 agent(sidechain)·시스템이 끼운 글은 뺀다. 생각(thinking)은 웹 「생각 표시」를
//!   켠 화면이 청할 때만 싣는다(herdr-web-ui `showThinking`, plan §8.125).
//! - 기록은 커질 수 있다(요청 원문 줄이 크다). 뒤에서부터 1MiB씩 읽어 필요한 만큼만 고르고, 16MiB에서 멈춘다.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use aaa_proto::{ChatMessage, ChatRole};
use serde_json::Value;

/// 보낼 줄 수 상한
const MAX_MESSAGES: usize = 150;
/// 한 줄 글자 상한
const MAX_CHARS: usize = 4000;
const CHUNK: u64 = 1 << 20;
/// 이만큼 읽어도 못 채우면 거기서 멈춘다
const READ_CAP: u64 = 16 << 20;

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

fn claude_root() -> PathBuf {
    env_dir("CLAUDE_CONFIG_DIR").unwrap_or_else(|| aaa_config::paths::home().join(".claude")).join("projects")
}

fn codex_root() -> PathBuf {
    env_dir("CODEX_HOME").unwrap_or_else(|| aaa_config::paths::home().join(".codex")).join("sessions")
}

/// root 안의 일반 `.jsonl` 파일인지 실제 위치로 확인한다
fn inside(root: &Path, path: &Path) -> Result<PathBuf, String> {
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
fn codex_file(root: &Path, id: &str) -> Result<PathBuf, String> {
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
        "<user_instructions>",
        "<permissions",
        "Caveat:",
    ]
    .iter()
    .any(|p| t.starts_with(p))
}

fn msg(role: ChatRole, text: &str) -> Option<ChatMessage> {
    let text = clip(text);
    (!text.is_empty()).then_some(ChatMessage { role, text })
}

/// Claude 기록 한 줄 → 보일 줄(0개 이상, 순서대로)
fn claude_line(v: &Value, thinking: bool) -> Vec<ChatMessage> {
    let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    if !matches!(ty, "user" | "assistant")
        || v.get("isSidechain").and_then(|b| b.as_bool()) == Some(true)
        || v.get("isMeta").and_then(|b| b.as_bool()) == Some(true)
    {
        return vec![];
    }
    let content = &v["message"]["content"];
    let mut out = Vec::new();
    match (ty, content) {
        ("user", Value::String(s)) if !injected(s) => out.extend(msg(ChatRole::User, s)),
        (_, Value::Array(blocks)) => {
            for b in blocks {
                match (ty, b.get("type").and_then(|t| t.as_str())) {
                    ("user", Some("text")) => {
                        let t = b["text"].as_str().unwrap_or("");
                        if !injected(t) {
                            out.extend(msg(ChatRole::User, t));
                        }
                    }
                    ("user", Some("image")) => out.extend(msg(ChatRole::User, "[그림]")),
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

/// 파일 끝에서부터 줄을 읽어 보일 줄을 고른다. (줄, 다 못 읽음)
fn tail(path: &Path, parse: impl Fn(&Value) -> Vec<ChatMessage>) -> std::io::Result<(Vec<ChatMessage>, bool)> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let mut pos = len;
    let mut carry: Vec<u8> = Vec::new();
    let mut rev: Vec<ChatMessage> = Vec::new();
    let take = |line: &[u8], rev: &mut Vec<ChatMessage>| {
        if let Ok(v) = serde_json::from_slice::<Value>(line) {
            rev.extend(parse(&v).into_iter().rev());
        }
    };
    while pos > 0 && len - pos < READ_CAP && rev.len() < MAX_MESSAGES {
        let start = pos.saturating_sub(CHUNK);
        let mut buf = vec![0u8; (pos - start) as usize];
        f.seek(SeekFrom::Start(start))?;
        f.read_exact(&mut buf)?;
        buf.extend_from_slice(&carry);
        let mut parts: Vec<&[u8]> = buf.split(|b| *b == b'\n').collect();
        // 맨 앞 조각은 이전 조각과 이어지는 줄일 수 있다(파일 맨 앞이 아니면 다음 번에 붙여 읽는다)
        let first = if start > 0 { parts.remove(0).to_vec() } else { Vec::new() };
        for line in parts.iter().rev() {
            if !line.is_empty() {
                take(line, &mut rev);
            }
        }
        // 한 줄이 상한보다 크면 그 줄은 버린다
        carry = if first.len() as u64 > READ_CAP { Vec::new() } else { first };
        pos = start;
    }
    let truncated = pos > 0 || rev.len() > MAX_MESSAGES;
    rev.truncate(MAX_MESSAGES);
    rev.reverse();
    Ok((rev, truncated))
}

/// 기록 한 줄 → 보일 줄(둘째 인자: 생각도 싣는다)
type LineParser = fn(&Value, bool) -> Vec<ChatMessage>;

/// agent 요청 처리. 오류 글에 경로를 넣지 않는다(홈 경로가 웹으로 간다).
pub fn read(
    harness: &str,
    transcript: Option<&str>,
    harness_session: Option<&str>,
    thinking: bool,
) -> Result<(Vec<ChatMessage>, bool), String> {
    let (path, parse): (PathBuf, LineParser) = match harness {
        "claude" => {
            let t = transcript.ok_or("Claude가 아직 기록 위치를 알리지 않았다(첫 hook 사건 전)")?;
            (inside(&claude_root(), Path::new(t))?, claude_line)
        }
        "codex" => {
            let id = harness_session.ok_or("Codex가 아직 thread-id를 알리지 않았다(첫 턴이 끝나기 전)")?;
            (codex_file(&codex_root(), id)?, codex_line)
        }
        _ => return Err("대화 기록은 Claude·Codex pane만 볼 수 있다".into()),
    };
    tail(&path, |v| parse(v, thinking)).map_err(|e| format!("기록을 읽지 못했다: {}", e.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
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
        ]
        .join("\n")
            + "\n"
    }

    #[test]
    fn claude_tail_filters() {
        let root = dir("claude");
        let f = root.join("s.jsonl");
        std::fs::write(&f, claude_fixture()).unwrap();
        let (m, truncated) = tail(&f, |v| claude_line(v, false)).unwrap();
        let got: Vec<_> = m.iter().map(|x| (x.role, x.text.as_str())).collect();
        assert_eq!(
            got,
            [
                (ChatRole::User, "첫 질문"),
                (ChatRole::Assistant, "답을 쓴다"),
                (ChatRole::Tool, "Bash: cargo test --all"),
                (ChatRole::Assistant, "끝났다"),
            ]
        );
        assert!(!truncated);
        // 「생각 표시」를 켠 화면이 청하면 생각도 제자리에
        let (m, _) = tail(&f, |v| claude_line(v, true)).unwrap();
        let got: Vec<_> = m.iter().map(|x| (x.role, x.text.as_str())).collect();
        assert_eq!(&got[1..3], [(ChatRole::Thinking, "속생각"), (ChatRole::Assistant, "답을 쓴다")]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn long_tail_is_truncated() {
        let root = dir("long");
        let f = root.join("s.jsonl");
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"줄"}]}}"#;
        std::fs::write(&f, format!("{line}\n").repeat(400)).unwrap();
        let (m, truncated) = tail(&f, |v| claude_line(v, false)).unwrap();
        assert_eq!((m.len(), truncated), (MAX_MESSAGES, true));
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
        let lines = |thinking: bool| -> Vec<_> {
            [user, env, dev, think, call, out]
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
}
