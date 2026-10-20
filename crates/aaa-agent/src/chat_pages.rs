//! JSONL 앞 쪽 읽기. 파일은 append-only byte stream이며 cursor는 줄 끝과 그 줄에서 이미 보낸 블록 수다.
use aaa_proto::ChatMessage;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

const CHUNK: u64 = 64 * 1024;
const SCAN: u64 = 16 << 20;
const MESSAGES: usize = 150;
// UTF-8·JSON escape까지 frame 1MiB 안에 든다. 한 메시지 4000자 제한은 chat parser의 계약이다.
const CHARS: usize = 40_000;

#[derive(Serialize, Deserialize)]
struct Cursor {
    file: String,
    end: u64,
    skip: usize,
}

pub(crate) struct Page {
    pub messages: Vec<ChatMessage>,
    pub before: Option<String>,
    pub history: String,
}

struct Revision {
    identity: String,
    len: u64,
    modified: Option<std::time::SystemTime>,
    generation: String,
}
static REVISIONS: LazyLock<Mutex<std::collections::HashMap<PathBuf, Revision>>> = LazyLock::new(Default::default);
static PROCESS: LazyLock<String> = LazyLock::new(aaa_common::util::new_id);
fn file_history(path: &Path, file: &std::fs::File) -> std::io::Result<(String, u64)> {
    // snapshot과 revision 비교를 같은 잠금 안에서 한다. 동시 append 읽기의 순서 역전을 축소로 오인하지 않는다.
    let mut revisions = REVISIONS.lock().unwrap();
    let meta = file.metadata()?;
    let identity = format!("{:?}", meta.created().ok());
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!("{identity}:{}:{}", meta.dev(), meta.ino())
    };
    // UI처럼 최근 파일만 기억한다. 재시작/퇴출은 cursor를 무효화하며 옛 파일과 섞지 않는다.
    if revisions.len() >= 128 && !revisions.contains_key(path) {
        revisions.clear();
    }
    let old = revisions.entry(path.to_path_buf()).or_insert_with(|| Revision {
        identity: identity.clone(),
        len: meta.len(),
        modified: meta.modified().ok(),
        generation: aaa_common::util::new_id(),
    });
    if old.identity != identity
        || meta.len() < old.len
        || (meta.len() == old.len && meta.modified().ok() != old.modified)
    {
        old.generation = aaa_common::util::new_id();
    }
    old.identity = identity.clone();
    old.len = meta.len();
    old.modified = meta.modified().ok();
    let fingerprint = format!("{}:{identity}:{}:{}", *PROCESS, path.to_string_lossy(), old.generation);
    Ok((format!("{:x}", Sha256::digest(fingerprint.as_bytes())), meta.len()))
}

// 앞에 읽고 남은 chunk를 재사용한다. 거대한 도구 결과도 메모리에 전부 담지 않는다.
struct Backward {
    file: std::fs::File,
    pos: u64,
    start: u64,
    buf: Vec<u8>,
}
impl Backward {
    fn fill(&mut self) -> std::io::Result<()> {
        if self.pos == 0 || (self.pos > self.start && self.pos <= self.start + self.buf.len() as u64) {
            return Ok(());
        }
        self.start = self.pos.saturating_sub(CHUNK);
        self.buf.resize((self.pos - self.start) as usize, 0);
        self.file.seek(SeekFrom::Start(self.start))?;
        self.file.read_exact(&mut self.buf)
    }
    fn previous(&mut self) -> std::io::Result<Option<u64>> {
        if self.pos == 0 {
            return Ok(None);
        }
        self.fill()?;
        if self.buf[(self.pos - self.start - 1) as usize] == b'\n' {
            self.pos -= 1;
        }
        loop {
            if self.pos == 0 {
                break;
            }
            self.fill()?;
            let bytes = &self.buf[..(self.pos - self.start) as usize];
            let split = bytes.iter().rposition(|b| *b == b'\n');
            let from = split.map_or(0, |i| i + 1);
            self.pos = self.start + from as u64;
            if split.is_some() {
                break;
            }
        }
        Ok(Some(self.pos))
    }
}

// 먼저 작은 구조(type/role)만 읽는다. 둘째 pass는 tool_result 같은 제외 블록을 문자열 할당 없이 건넌다.
struct Select<'a> {
    shape: Option<&'a Value>,
    outline: bool,
    depth: usize,
    keep_string: bool,
}
impl<'de> serde::de::DeserializeSeed<'de> for Select<'_> {
    type Value = Value;
    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        if !self.outline
            && self
                .shape
                .and_then(|v| v["type"].as_str())
                .is_some_and(|ty| matches!(ty, "tool_result" | "function_call_output"))
        {
            <serde::de::IgnoredAny as Deserialize>::deserialize(deserializer)?;
            return Ok(Value::Null);
        }
        deserializer.deserialize_any(self)
    }
}
impl<'de> serde::de::Visitor<'de> for Select<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str("transcript JSON")
    }
    fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<Value, M::Error> {
        let mut out = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let selected = if self.outline {
                matches!(key.as_str(), "type" | "role" | "message" | "payload" | "attachment" | "origin" | "kind")
                    || (key == "content" && self.depth < 2)
            } else {
                matches!(
                    key.as_str(),
                    "type"
                        | "message"
                        | "content"
                        | "role"
                        | "text"
                        | "thinking"
                        | "name"
                        | "input"
                        | "command"
                        | "cmd"
                        | "file_path"
                        | "path"
                        | "pattern"
                        | "url"
                        | "query"
                        | "description"
                        | "prompt"
                        | "isSidechain"
                        | "isMeta"
                        | "isCompactSummary"
                        | "isSynthetic"
                        | "origin"
                        | "kind"
                        | "attachment"
                        | "commandMode"
                        | "payload"
                        | "arguments"
                        | "summary"
                )
            };
            if selected {
                let value = map.next_value_seed(Select {
                    shape: self.shape.and_then(|v| v.get(&key)),
                    outline: self.outline,
                    depth: self.depth + 1,
                    keep_string: key != "content",
                })?;
                out.insert(key, value);
            } else {
                map.next_value::<serde::de::IgnoredAny>()?;
            }
        }
        Ok(Value::Object(out))
    }
    fn visit_seq<S: serde::de::SeqAccess<'de>>(self, mut seq: S) -> Result<Value, S::Error> {
        let mut out = Vec::new();
        while let Some(value) = seq.next_element_seed(Select {
            shape: self.shape.and_then(|v| v.get(out.len())),
            outline: self.outline,
            depth: self.depth,
            keep_string: self.keep_string,
        })? {
            out.push(value);
        }
        Ok(Value::Array(out))
    }
    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Value, E> {
        Ok(if self.outline && !self.keep_string { Value::Null } else { Value::String(value.into()) })
    }
    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Value, E> {
        Ok(if self.outline && !self.keep_string { Value::Null } else { Value::String(value) })
    }
    fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Value, E> {
        Ok(Value::Bool(value))
    }
    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Value, E> {
        Ok(Value::Number(value.into()))
    }
    fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Value, E> {
        Ok(serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number))
    }
    fn visit_unit<E: serde::de::Error>(self) -> Result<Value, E> {
        Ok(Value::Null)
    }
}

fn selected(file: &mut std::fs::File, start: u64, size: u64) -> std::io::Result<Option<Value>> {
    use serde::de::DeserializeSeed;
    let mut pass = |shape: Option<&Value>, outline| -> std::io::Result<Option<Value>> {
        file.seek(SeekFrom::Start(start))?;
        let mut de = serde_json::Deserializer::from_reader(std::io::BufReader::new((&mut *file).take(size)));
        let value = Select { shape, outline, depth: 0, keep_string: true }.deserialize(&mut de);
        Ok(value
            .and_then(|value| {
                de.end()?;
                Ok(value)
            })
            .ok())
    };
    let Some(shape) = pass(None, true)? else { return Ok(None) };
    pass(Some(&shape), false)
}

pub(super) fn read(
    path: &Path,
    before: Option<&str>,
    parse: impl Fn(&Value) -> Vec<ChatMessage>,
) -> Result<Page, String> {
    let io = |e: std::io::Error| format!("기록을 읽지 못했다: {}", e.kind());
    let mut file = std::fs::File::open(path).map_err(io)?;
    let (history, len) = file_history(path, &file).map_err(io)?;
    let mut cursor = match before {
        Some(raw) if raw.len() <= 256 => serde_json::from_str::<Cursor>(raw).map_err(|_| "대화 쪽 위치가 잘못됐다")?,
        Some(_) => return Err("대화 쪽 위치가 잘못됐다".into()),
        None => Cursor { file: history.clone(), end: len, skip: 0 },
    };
    if cursor.file != history || cursor.end > len || cursor.skip > 100_000 {
        return Err("대화 기록이 바뀌었다. 다시 열어 주세요".into());
    }
    if cursor.end > 0 && before.is_some() {
        file.seek(SeekFrom::Start(cursor.end - 1)).map_err(io)?;
        let mut byte = [0];
        file.read_exact(&mut byte).map_err(io)?;
        if byte[0] != b'\n' {
            return Err("대화 기록이 바뀌었다. 다시 열어 주세요".into());
        }
    }
    // 쓰는 중인 마지막 줄은 다음 최신 요청이 읽는다. 완성되지 않은 경계를 cursor로 발급하지 않는다.
    if before.is_none() && len > 0 {
        file.seek(SeekFrom::Start(len - 1)).map_err(io)?;
        let mut byte = [0];
        file.read_exact(&mut byte).map_err(io)?;
        if byte[0] != b'\n' {
            let mut tail = Backward { file: file.try_clone().map_err(io)?, pos: len, start: len, buf: Vec::new() };
            tail.previous().map_err(io)?;
            cursor.end = tail.pos;
        }
    }
    let scan_end = cursor.end;
    let mut reader = Backward { file, pos: cursor.end, start: cursor.end, buf: Vec::new() };
    let mut rev = Vec::new();
    let mut chars = 0;
    while reader.pos > 0 {
        let end = reader.pos;
        let Some(start) = reader.previous().map_err(io)? else { break };
        // raw 줄은 복사하지 않고 필요한 필드만 스트리밍한다. 큰 그림/도구 결과와 같은 줄의 사람 글도 보존한다.
        let messages =
            selected(&mut reader.file, start, end - start).map_err(io)?.map(|v| parse(&v)).unwrap_or_default();
        let skip = cursor.skip;
        if skip > messages.len() {
            return Err("대화 기록이 바뀌었다. 다시 열어 주세요".into());
        }
        for (taken, (index, mut message)) in messages.into_iter().enumerate().rev().skip(skip).enumerate() {
            let count = message.text.chars().count();
            if rev.len() >= MESSAGES || (!rev.is_empty() && chars + count > CHARS) {
                cursor.end = end;
                cursor.skip = skip + taken;
                rev.reverse();
                return Ok(Page { messages: rev, before: Some(serde_json::to_string(&cursor).unwrap()), history });
            }
            chars += count;
            message.id = Some(format!("{history}:{start:020}:{index:010}"));
            rev.push(message);
        }
        cursor.end = start;
        cursor.skip = 0;
        if scan_end - start >= SCAN {
            break;
        }
    }
    rev.reverse();
    let before = (cursor.end > 0).then(|| serde_json::to_string(&cursor).unwrap());
    Ok(Page { messages: rev, before, history })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{claude_line, codex_line};
    use serde_json::json;
    use std::io::Write;

    #[test]
    fn pages_reach_the_first_block_while_the_file_grows() {
        for (harness, parse) in [("claude", claude_line as super::super::LineParser), ("codex", codex_line)] {
            let root = super::super::tests::dir(&format!("pages-{harness}"));
            let path = root.join("s.jsonl");
            let record = |n| {
                if harness == "claude" {
                    json!({"type":"assistant","message":{"content": if n % 7 == 0 { vec![json!({"type":"text","text":format!("줄 {n}")})] } else { vec![json!({"type":"text","text":format!("줄 {n}")}),json!({"type":"text","text":format!("둘 {n}")})] }}}).to_string() + "\n"
                } else {
                    json!({"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":format!("줄 {n}")}]}}).to_string() + "\n"
                }
            };
            let mut file = std::fs::File::create(&path).unwrap();
            for n in 0..1801 {
                file.write_all(record(n).as_bytes()).unwrap();
            }
            drop(file);
            let mut page = read(&path, None, |v| parse(v, false)).unwrap();
            let history = page.history.clone();
            let mut messages = page.messages;
            let first_cursor = page.before.clone().unwrap();
            // pending write는 페이지 cursor/이미 읽은 ID를 흔들지 않는다.
            let mut file = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
            file.write_all(record(1801).as_bytes()).unwrap();
            file.write_all(b"{\"type\":").unwrap();
            let newest = read(&path, None, |v| parse(v, false)).unwrap();
            assert_eq!(newest.history, history);
            assert_eq!(newest.messages.last().unwrap().text, if harness == "claude" { "둘 1801" } else { "줄 1801" });
            while let Some(cursor) = page.before {
                page = read(&path, Some(&cursor), |v| parse(v, false)).unwrap();
                assert_ne!(page.before.as_ref(), Some(&cursor), "빈 쪽도 진행한다");
                assert_eq!(page.history, history);
                let mut older = std::mem::take(&mut page.messages);
                older.extend(messages);
                messages = older;
            }
            let expected: Vec<_> = (0..1801)
                .flat_map(|n| {
                    if harness == "claude" && n % 7 != 0 {
                        vec![format!("줄 {n}"), format!("둘 {n}")]
                    } else {
                        vec![format!("줄 {n}")]
                    }
                })
                .collect();
            assert_eq!(messages.iter().map(|m| m.text.clone()).collect::<Vec<_>>(), expected);
            let ids: std::collections::HashSet<_> = messages.iter().map(|m| m.id.as_ref().unwrap()).collect();
            assert_eq!(ids.len(), messages.len(), "블록도 중복 없이 안정된 ID");
            let reread = read(&path, Some(&first_cursor), |v| parse(v, false)).unwrap();
            assert!(reread.messages.iter().all(|m| ids.contains(m.id.as_ref().unwrap())));
            // 다른 파일에 cursor를 재사용할 수 없다. 경로·정체성은 외부에 노출되지 않는다.
            std::fs::rename(&path, root.join("old.jsonl")).unwrap();
            std::fs::write(&path, record(0)).unwrap();
            assert!(read(&path, Some(&first_cursor), |v| parse(v, false)).is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn sparse_and_large_records_keep_advancing_with_bounded_wire_pages() {
        let root = super::super::tests::dir("page-budget");
        let path = root.join("s.jsonl");
        let mut file = std::fs::File::create(&path).unwrap();
        writeln!(file, "{}", json!({"type":"user","message":{"content":[{"type":"text","text":"처음"},{"type":"image","source":{"data":"x".repeat(17 << 20)}},{"type":"tool_result","content":"x".repeat(17 << 20)},{"type":"tool_result","content":[{"type":"text","text":"x".repeat(17 << 20)}]}]}})).unwrap();
        // 예전 16MiB 탐색 상한 너머에도 첫 글이 있다. 한 줄 자체가 큰 도구 결과도 건넌다.
        writeln!(file, "{}", json!({"type":"blob","data":"x".repeat(17 << 20)})).unwrap();
        for _ in 0..400 {
            writeln!(
                file,
                "{}",
                json!({"type":"assistant","message":{"content":[{"type":"text","text":"가\"\\".repeat(1333)}]}})
            )
            .unwrap();
        }
        drop(file);
        let mut cursor = None;
        let mut count = 0;
        let mut first = false;
        loop {
            let page = read(&path, cursor.as_deref(), |v| claude_line(v, false)).unwrap();
            let wire = serde_json::to_vec(&aaa_proto::AgentResponse::Chat {
                messages: page.messages.clone(),
                truncated: page.before.is_some(),
                before: page.before.clone(),
                history: Some(page.history),
            })
            .unwrap();
            assert!(wire.len() < aaa_proto::MAX_FRAME);
            count += page.messages.len();
            first |= page.messages.iter().any(|m| m.text == "처음");
            assert!(page.before.is_none() || page.before != cursor);
            cursor = page.before;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(count, 402);
        assert!(first);
        assert!(read(&path, Some("not a cursor"), |v| claude_line(v, false)).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
