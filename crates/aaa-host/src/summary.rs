//! thread 작업 요약(plan §8.259). coordinator가 thread마다 `threads/<id>.summary.md`를 통째로 다시 쓰고(보고와 같은 규칙),
//! host는 그 위에 thread 보고의 `## 진행`·`## Decisions`(agent가 정한 것), 요약 뒤에 쌓인 지시(task.md `## Follow-ups`), 요약이 없을
//! 때의 할 일 첫 줄을 붙여 웹 hover 카드·「요약」 단추에 보인다. 참고 넷에는 지시마다 갱신하는 요약이 없어 형식은 새로 정했다.
//!
//! 파일 첫 줄은 host가 쓰는 표지(`<!-- aaa summary at=<ms> by=<누가> seen=<n> -->`)다. `seen`은 요약을 쓸 때의 Follow-ups
//! 수이고, 그보다 늘어난 만큼이 「요약 뒤 지시」다. 사람이 파일을 손으로 고쳐 표지를 지우면 모든 지시가 요약 뒤로 보인다.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use aaa_proto::{ErrorBody, FollowUpLine, SummaryEntry, SummaryMark, SummaryStep, SummaryStepState, ThreadSummaryView};
use serde::{Deserialize, Serialize};

use crate::Host;
use crate::orch::{internal, write_atomic};

/// 요약 파일 상한(바이트). hover 카드는 훑어보는 것이라(10-19 사용자 「길면 안 된다」) 넘으면 거부한다
pub(crate) const MAX_BYTES: usize = 1024;
/// 목록 절마다 줄 수 상한
pub(crate) const MAX_ITEMS: usize = 3;
/// 전체 큰 작업은 완료한 것도 남긴다. hover에 모두 보일 수 있게 최대 5개다.
pub(crate) const MAX_STEPS: usize = 5;
/// 한 줄 상한(글자, 앞의 「누가 시각」은 빼고 센다). 문장이 아니라 짧은 구
pub(crate) const MAX_LINE: usize = 30;
/// 지시 첫 줄·보고 Decisions 줄을 보일 때 자르는 길이(남이 쓴 글이라 거부하지 않고 자른다)
const SHOW_LINE: usize = 60;
/// 요약 뒤 지시를 몇 개까지 보이나(마지막 것부터)
const PENDING_LINES: usize = 3;
const META: &str = "<!-- aaa summary ";

/// 요약 형식(`aaa thread summary --help`·coordinator 안내와 같다)
pub(crate) const FORMAT: &str = "\
# 목표
<지금 목표, 짧은 구>
처음: <목표가 바뀌었으면 처음 일(선택)>
## 진행
- [x] <끝낸 큰 작업>
- [~] <진행 중인 큰 작업>
- [ ] <남은 큰 작업, 전체 5개 이하>
## 정함
- 사용자 15:0x <정한 것>
## 바뀜
- ~~사용자 14:0x <무효가 된 것>~~
- 사용자 14:1x <최근 바뀐 것은 맨 아래>";

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Parsed {
    pub goal: Option<String>,
    pub origin: Option<String>,
    pub progress: Vec<SummaryStep>,
    pub now: Vec<String>,
    pub decided: Vec<SummaryEntry>,
    pub changed: Vec<SummaryEntry>,
    pub done: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Part {
    Goal,
    Progress,
    Now,
    Decided,
    Changed,
    Done,
}

/// 요약 글을 읽는다. 형식·상한을 어기면 까닭을 돌려준다(쓰기는 거부하고, 읽기는 글만 보인다)
pub(crate) fn parse(text: &str) -> Result<Parsed, String> {
    if text.len() > MAX_BYTES {
        return Err(format!("요약이 {}바이트다. {MAX_BYTES}바이트 아래로 줄인다(지금 유효한 것만)", text.len()));
    }
    let mut p = Parsed::default();
    let mut part: Option<Part> = None;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let at = n + 1;
        if line.is_empty() {
            continue;
        }
        if line.starts_with('#') {
            part = Some(match line {
                "# 목표" => Part::Goal,
                "## 진행" => Part::Progress,
                "## 지금" => Part::Now,
                "## 정함" => Part::Decided,
                "## 바뀜" => Part::Changed,
                "## 끝남" => Part::Done,
                _ => {
                    return Err(format!(
                        "{at}번 줄: 모르는 머리 `{line}`. 머리는 `# 목표`·`## 진행`·`## 지금`·`## 정함`·`## 바뀜`·`## 끝남`이다"
                    ));
                }
            });
            continue;
        }
        match part {
            None => return Err(format!("{at}번 줄: `# 목표` 머리 앞에 글이 있다")),
            Some(Part::Goal) => {
                if let Some(o) = line.strip_prefix("처음:") {
                    short(at, o.trim())?;
                    p.origin = Some(o.trim().to_string()).filter(|o| !o.is_empty());
                } else if p.goal.is_none() {
                    short(at, line)?;
                    p.goal = Some(line.to_string());
                } else {
                    return Err(format!("{at}번 줄: 목표는 한 줄이다"));
                }
            }
            Some(list) => {
                let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).map(str::trim) else {
                    return Err(format!("{at}번 줄: 목록 줄은 `- `로 시작한다"));
                };
                if item.is_empty() {
                    continue;
                }
                let len = match list {
                    Part::Progress => {
                        let (state, text) =
                            if let Some(t) = item.strip_prefix("[x] ").or_else(|| item.strip_prefix("[X] ")) {
                                (SummaryStepState::Done, t)
                            } else if let Some(t) = item.strip_prefix("[~] ") {
                                (SummaryStepState::Working, t)
                            } else if let Some(t) = item.strip_prefix("[ ] ") {
                                (SummaryStepState::Pending, t)
                            } else {
                                return Err(format!("{at}번 줄: 진행은 `[x] 완료`·`[~] 진행 중`·`[ ] 대기`로 쓴다"));
                            };
                        let text = text.trim();
                        if text.is_empty() {
                            return Err(format!("{at}번 줄: 진행 항목에 큰 작업 이름이 없다"));
                        }
                        short(at, text)?;
                        p.progress.push(SummaryStep { text: text.to_string(), state });
                        p.progress.len()
                    }
                    Part::Now => {
                        short(at, item)?;
                        p.now.push(item.to_string());
                        p.now.len()
                    }
                    Part::Decided => {
                        let e = entry(item);
                        short(at, &e.text)?;
                        p.decided.push(e);
                        p.decided.len()
                    }
                    Part::Changed => {
                        let e = entry(item);
                        short(at, &e.text)?;
                        p.changed.push(e);
                        p.changed.len()
                    }
                    _ => {
                        short(at, item)?;
                        p.done.push(item.to_string());
                        p.done.len()
                    }
                };
                let limit = if list == Part::Progress { MAX_STEPS } else { MAX_ITEMS };
                if len > limit {
                    return Err(format!("{at}번 줄: 한 절은 {limit}줄까지다. 큰 작업으로 묶어 짧게 쓴다"));
                }
            }
        }
    }
    if p.goal.is_none() {
        return Err("`# 목표` 아래 목표 한 줄이 없다".into());
    }
    Ok(p)
}

/// 한 줄은 짧은 구: MAX_LINE자 이하
fn short(at: usize, text: &str) -> Result<(), String> {
    let n = text.chars().count();
    if n > MAX_LINE {
        return Err(format!("{at}번 줄이 {n}자다. {MAX_LINE}자 이하의 짧은 구로 쓴다(문장 금지)"));
    }
    Ok(())
}

/// 목록 한 줄: `[~~]누가 시각 글[~~]`. 누가·시각은 없어도 된다
pub(crate) fn entry(item: &str) -> SummaryEntry {
    let (mut dropped, body) = match item.strip_prefix("~~").and_then(|x| x.strip_suffix("~~")) {
        Some(x) => (true, x.trim()),
        None => (false, item),
    };
    let mut who = None;
    let mut at = None;
    let mut rest = body;
    if let Some((w, r)) = body.split_once(' ') {
        let w = match w {
            "사용자" => Some("사용자"),
            "coordinator" | "coord" => Some("coordinator"),
            "agent" => Some("agent"),
            _ => None,
        };
        if let Some(w) = w {
            who = Some(w.to_string());
            rest = r.trim_start();
            let time = |t: &str| {
                t.contains(':') && t.chars().all(|c| c.is_ascii_digit() || matches!(c, ':' | 'x' | '~' | '-'))
            };
            match rest.split_once(' ') {
                Some((t, r)) if time(t) => {
                    at = Some(t.to_string());
                    rest = r.trim_start();
                }
                None if time(rest) => {
                    at = Some(rest.to_string());
                    rest = "";
                }
                _ => {}
            }
        }
    }
    // 칩 뒤의 글만 그은 것(`사용자 14:0x ~~없앰~~`)
    if let Some(x) = rest.strip_prefix("~~").and_then(|x| x.strip_suffix("~~")) {
        dropped = true;
        rest = x.trim();
    }
    SummaryEntry { who, at, text: rest.to_string(), dropped }
}

/// host가 쓴 표지
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Meta {
    pub at_ms: u64,
    pub by: String,
    pub seen: u32,
}

/// 파일 = 표지 줄 + 요약 글. 표지가 없거나 깨졌으면 None과 파일 전체
pub(crate) fn split_meta(file: &str) -> (Option<Meta>, &str) {
    let (first, rest) = file.split_once('\n').unwrap_or((file, ""));
    let Some(body) = first.trim_end().strip_prefix(META).and_then(|x| x.strip_suffix("-->")) else {
        return (None, file);
    };
    let mut at = None;
    let mut by = None;
    let mut seen = None;
    for kv in body.split_whitespace() {
        match kv.split_once('=') {
            Some(("at", v)) => {
                at = v
                    .parse()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            }
            Some(("by", v)) => by = Some(v.to_string()),
            Some(("seen", v)) => {
                seen = v
                    .parse()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            }
            _ => {}
        }
    }
    match (at, by, seen) {
        (Some(at_ms), Some(by), Some(seen)) => (Some(Meta { at_ms, by, seen }), rest),
        _ => (None, file),
    }
}

pub(crate) fn file_text(meta: &Meta, text: &str) -> String {
    format!("{META}at={} by={} seen={} -->\n{}\n", meta.at_ms, meta.by, meta.seen, text.trim())
}

fn clip(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= SHOW_LINE {
        return s.to_string();
    }
    let mut t: String = s.chars().take(SHOW_LINE - 1).collect();
    t.push('…');
    t
}

/// task.md Follow-ups의 지시 전문. 제목/시각 표지만 떼고 본문은 자르지 않는다.
pub(crate) fn follow_ups(task: &str) -> Vec<FollowUpLine> {
    let mut out: Vec<FollowUpLine> = Vec::new();
    let mut inside = false;
    for line in task.lines() {
        let t = line.trim();
        if !inside {
            if t == "## Follow-ups" {
                inside = true;
            }
            continue;
        }
        if let Some(when) = t.strip_prefix("### ")
            && let Some(at) = aaa_common::util::parse_iso(when)
        {
            out.push(FollowUpLine { at_ms: Some(at), text: String::new() });
            continue;
        }
        if let Some(last) = out.last_mut() {
            last.text.push_str(line);
            last.text.push('\n');
        }
    }
    for line in &mut out {
        line.text = line.text.trim().to_string();
    }
    out
}

/// 요약이 없을 때 보이는 할 일 첫 줄: 첫 `# ` 제목, 없으면 첫 글 줄(Follow-ups 앞에서)
pub(crate) fn task_line(task: &str) -> Option<String> {
    let head = task.split("\n## Follow-ups").next().unwrap_or(task);
    head.lines()
        .find_map(|l| l.trim().strip_prefix("# ").map(clip))
        .or_else(|| head.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#')).map(clip))
}

/// thread 보고의 `## Decisions` 줄(agent가 정한 것, 상한 MAX_ITEMS)
pub(crate) fn decisions(report: &str) -> Vec<String> {
    aaa_proto::section_lines(report, "Decisions").into_iter().take(MAX_ITEMS).map(|l| clip(&l)).collect()
}

/// 보고 전체의 다른 절·코드 예제를 요약 형식으로 읽지 않는다. 잘못된 목록은 마지막 정상 목록을 보존한다.
fn report_progress(report: &str) -> Option<Vec<SummaryStep>> {
    let mut section = String::from("# 목표\n진행\n## 진행\n");
    let mut inside = false;
    let mut found = false;
    let mut fence: Option<(char, usize)> = None;
    for raw in report.lines() {
        let line = raw.trim();
        if let Some((ch, len)) = fence {
            let count = line.chars().take_while(|c| *c == ch).count();
            if count >= len && line[count..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if raw.starts_with("    ") || raw.starts_with('\t') {
            if inside {
                return None;
            }
            continue;
        }
        if line.starts_with("```") || line.starts_with("~~~") {
            let ch = line.chars().next().unwrap();
            fence = Some((ch, line.chars().take_while(|c| *c == ch).count()));
            if inside {
                return None;
            }
            continue;
        }
        if line.starts_with('#') {
            inside = line == "## 진행";
            if inside && found {
                return None;
            }
            found |= inside;
            continue;
        }
        if inside {
            section.push_str(line);
            section.push('\n');
            if section.len() > MAX_BYTES {
                return None;
            }
        }
    }
    let steps = parse(&section)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()?
        .progress;
    (!steps.is_empty()).then_some(steps)
}

/// host의 수신 영수증. 같은 보고 재수집·재시작이 나중 지시를 반영한 것으로 바꾸지 못한다.
#[derive(Serialize, Deserialize)]
struct ReportProgress {
    hash: String,
    at_ms: u64,
    seen: u32,
    progress: Vec<SummaryStep>,
}

fn progress_path(dir: &Path, id: &str) -> std::path::PathBuf {
    dir.join("threads").join(format!("{id}.progress.json"))
}

fn read_progress(dir: &Path, id: &str) -> Option<ReportProgress> {
    serde_json::from_slice(
        &std::fs::read(progress_path(dir, id))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?,
    )
    .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
    .ok()
}

fn effective_meta(meta: Option<Meta>, progress: Option<&ReportProgress>) -> Option<Meta> {
    let seen = meta.as_ref().map_or(0, |m| m.seen).max(progress.map_or(0, |p| p.seen));
    let mut result = match progress {
        Some(p) if meta.as_ref().is_none_or(|m| p.at_ms > m.at_ms) => {
            Some(Meta { at_ms: p.at_ms, by: "agent".into(), seen: p.seen })
        }
        _ => meta,
    };
    if let Some(m) = &mut result {
        m.seen = seen;
    }
    result
}

/// thread 목록이 매번 파일을 읽지 않게 host가 기억하는 표지 재료
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MarkCache {
    pub at_ms: Option<u64>,
    pub seen: u32,
    pub total: u32,
}

impl MarkCache {
    pub(crate) fn mark(&self) -> SummaryMark {
        // 표지가 없는 요약(손으로 쓴 것)은 모든 지시를 요약 뒤로 본다
        let pending = if self.at_ms.is_some() { self.total.saturating_sub(self.seen) } else { self.total };
        SummaryMark { at_ms: self.at_ms, pending }
    }
}

pub(crate) type MarkMap = std::sync::Mutex<HashMap<(String, String), MarkCache>>;

fn summary_path(dir: &Path, id: &str) -> std::path::PathBuf {
    dir.join("threads").join(format!("{id}.summary.md"))
}

impl Host {
    /// 수집 시작 때 지시 수를 잡아 전송 중 도착한 지시는 경고에 남긴다.
    pub(crate) fn summary_report_seen(&self, dir: &Path, id: &str) -> u32 {
        let _guard = self.follow_up_lock.lock().unwrap();
        read_cache(dir, id).total
    }

    pub(crate) fn summary_report_received(
        &self,
        project: &str,
        id: &str,
        report: &str,
        hash: &str,
        seen: u32,
    ) -> Result<(), ErrorBody> {
        let Some(progress) = report_progress(report) else { return Ok(()) };
        let dir = self.project_dir(project)?;
        let _guard = self.follow_up_lock.lock().unwrap();
        if read_progress(&dir, id).is_some_and(|p| p.hash == hash) {
            return Ok(());
        }
        let previous = read_cache(&dir, id);
        let receipt = ReportProgress {
            hash: hash.into(),
            at_ms: aaa_common::util::now_ms().max(previous.at_ms.unwrap_or(0).saturating_add(1)),
            seen,
            progress,
        };
        write_atomic(&progress_path(&dir, id), &serde_json::to_vec(&receipt).map_err(internal)?).map_err(internal)?;
        self.summary_marks.lock().unwrap().insert((project.into(), id.into()), read_cache(&dir, id));
        Ok(())
    }

    /// thread 줄의 요약 표지. 처음 한 번 파일을 읽고, 뒤로는 요약 쓰기·지시 덧붙이기가 고친다
    pub(crate) fn summary_mark(&self, project: &str, id: &str) -> Option<SummaryMark> {
        let key = (project.to_string(), id.to_string());
        if let Some(c) = self.summary_marks.lock().unwrap().get(&key) {
            return Some(c.mark());
        }
        let dir = self
            .project_dir(project)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()?;
        let c = read_cache(&dir, id);
        Some(self.summary_marks.lock().unwrap().entry(key).or_insert(c).mark())
    }

    /// task.md에 지시를 덧붙인 뒤(같은 잠금 안): 그 글로 Follow-ups 수를 고친다
    pub(crate) fn summary_follow_ups(&self, project: &str, id: &str, task: &str) {
        let total = follow_ups(task).len() as u32;
        let key = (project.to_string(), id.to_string());
        let mut map = self.summary_marks.lock().unwrap();
        match map.get_mut(&key) {
            Some(c) => c.total = total,
            None => {
                if let Ok(dir) = self.project_dir(project) {
                    let mut c = read_cache(&dir, id);
                    c.total = total;
                    map.insert(key, c);
                }
            }
        }
    }

    pub(crate) fn thread_summary(&self, project: &str, id: &str) -> Result<ThreadSummaryView, ErrorBody> {
        self.load(project, id)?;
        let dir = self.project_dir(project)?;
        let _guard = self.follow_up_lock.lock().unwrap();
        let read = |name: String| {
            std::fs::read_to_string(dir.join("threads").join(name))
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        };
        let file = read(format!("{id}.summary.md"));
        let task = read(format!("{id}.task.md")).unwrap_or_default();
        let report = read(format!("{id}.md")).unwrap_or_default();
        let all = follow_ups(&task);
        let (meta, text) = match &file {
            Some(f) => {
                let (m, t) = split_meta(f);
                (m, Some(t.trim().to_string()))
            }
            None => (None, None),
        };
        let mut parsed = text
            .as_deref()
            .and_then(|t| {
                parse(t)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .unwrap_or_default();
        let progress = read_progress(&dir, id);
        let meta = effective_meta(meta, progress.as_ref());
        if let Some(p) = progress {
            parsed.progress = p.progress;
        }
        let total = all.len() as u32;
        let pending = match &meta {
            Some(m) => total.saturating_sub(m.seen),
            None => total,
        };
        let skip = all.len().saturating_sub((pending as usize).min(PENDING_LINES));
        Ok(ThreadSummaryView {
            project: project.to_string(),
            thread: id.to_string(),
            goal: parsed.goal,
            origin: parsed.origin,
            progress: parsed.progress,
            now: parsed.now,
            decided: parsed.decided,
            changed: parsed.changed,
            done: parsed.done,
            decisions: decisions(&report),
            at_ms: meta.as_ref().map(|m| m.at_ms),
            by: meta.map(|m| m.by),
            follow_ups: total,
            pending,
            pending_lines: all.into_iter().skip(skip).collect(),
            task: if text.is_none() { task_line(&task) } else { None },
            text,
        })
    }

    /// 요약을 통째로 다시 쓴다. 그때의 Follow-ups 수를 표지에 남겨, 그 뒤 지시만 「요약 뒤」로 센다
    pub(crate) fn thread_summary_set(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        text: &str,
        actor: &str,
    ) -> Result<ThreadSummaryView, ErrorBody> {
        let rec = self.load(project, id)?;
        let dir = self.project_dir(project)?;
        let text = text.trim();
        crate::validate_thread_summary(text)?;
        let by = self.summary_writer(project, actor);
        {
            // 지시 덧붙이기와 엇갈리지 않게(그 사이에 온 지시를 「본 것」으로 세지 않는다)
            let _guard = self.follow_up_lock.lock().unwrap();
            let task = std::fs::read_to_string(dir.join("threads").join(format!("{id}.task.md")))
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
                .unwrap_or_default();
            let total = follow_ups(&task).len() as u32;
            let at_ms = aaa_common::util::now_ms().max(read_cache(&dir, id).at_ms.unwrap_or(0).saturating_add(1));
            let meta = Meta { at_ms, by, seen: total };
            write_atomic(&summary_path(&dir, id), file_text(&meta, text).as_bytes()).map_err(internal)?;
            self.summary_marks.lock().unwrap().insert(
                (project.to_string(), id.to_string()),
                MarkCache { at_ms: Some(meta.at_ms), seen: meta.seen, total },
            );
        }
        self.db.audit(actor, "thread_summary_set", &format!("{project}/{id} {} bytes", text.len()));
        aaa_common::diagnostic::ignored(
            self.thread_events.send(self.thread_view(&rec)),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
        self.thread_summary(project, id)
    }

    /// 표지의 「쓴 쪽」: 그 프로젝트 coordinator pane 안의 CLI면 coordinator, 그 밖(사람의 셸·웹)은 사람
    fn summary_writer(&self, project: &str, actor: &str) -> String {
        let pane = actor.strip_prefix("local:").and_then(|a| a.rsplit_once('@')).map(|(_, p)| p);
        let coordinator = pane
            .and_then(|p| self.pane_view_of(p, aaa_proto::DeviceRole::Local))
            .is_some_and(|v| v.coordinator_project.as_deref() == Some(project));
        if coordinator { "coordinator" } else { "사람" }.to_string()
    }
}

fn read_cache(dir: &Path, id: &str) -> MarkCache {
    let task = std::fs::read_to_string(dir.join("threads").join(format!("{id}.task.md")))
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
        .unwrap_or_default();
    let total = follow_ups(&task).len() as u32;
    let meta = std::fs::read_to_string(summary_path(dir, id))
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
        .ok()
        .and_then(|f| split_meta(&f).0);
    let progress = read_progress(dir, id);
    match effective_meta(meta, progress.as_ref()) {
        Some(m) => MarkCache { at_ms: Some(m.at_ms), seen: m.seen, total },
        None => MarkCache { at_ms: None, seen: 0, total },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_progress_requires_a_real_bounded_section_and_preserves_order() {
        let report = "[진행 중] 확인\n## Report\n```md\n## 진행\n- [x] 예제\n```\n## 진행\n- [X] 완료\n- [~] 작업\n- [ ] 남음\n## Next\n- 다음\n";
        let steps = report_progress(report).unwrap();
        assert_eq!(steps.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["완료", "작업", "남음"]);
        assert_eq!(
            steps.iter().map(|s| s.state).collect::<Vec<_>>(),
            [SummaryStepState::Done, SummaryStepState::Working, SummaryStepState::Pending]
        );
        for invalid in [
            "## Report\n```md\n## 진행\n- [x] 예제\n```",
            "## 진행\n- 상태 없음",
            "## 진행\n- [x] ",
            "## 진행\n",
            "## 진행\n- [x] 하나\n## 진행\n- [ ] 둘",
        ] {
            assert!(report_progress(invalid).is_none(), "{invalid}");
        }
        assert!(report_progress(&format!("## 진행\n- [ ] {}", "가".repeat(MAX_LINE + 1))).is_none());
        assert!(report_progress(&format!("## 진행\n{}", "- [ ] 항목\n".repeat(MAX_STEPS + 1))).is_none());
    }

    const T0393: &str = "# 목표
웹 패널 다듬기·Ctrl+Enter steering
처음: 머신 CPU·MEM 표시 시안
## 지금
- Ctrl+Enter는 작업 중에만
- 사이드 패널 좌우 resize
## 정함
- 사용자 15:0x 들여쓰기 8px
- agent 14:11 판정은 host 한 곳
## 바뀜
- ~~사용자 14:0x Ctrl+Enter 없앰~~
- 사용자 14:1x 줄바꿈 빼고 steering만
- coord 12:2x~14:5x 착지 알림 11번
## 끝남
- CPU·MEM 착지 61917aab";

    #[test]
    fn parses_sections_chips_and_strikes() {
        let p = parse(T0393).unwrap();
        assert_eq!(p.goal.as_deref(), Some("웹 패널 다듬기·Ctrl+Enter steering"));
        assert_eq!(p.origin.as_deref(), Some("머신 CPU·MEM 표시 시안"));
        assert_eq!(p.now.len(), 2);
        assert_eq!(p.done, vec!["CPU·MEM 착지 61917aab"]);
        assert_eq!(
            p.decided[1],
            SummaryEntry {
                who: Some("agent".into()),
                at: Some("14:11".into()),
                text: "판정은 host 한 곳".into(),
                dropped: false
            }
        );
        let c = &p.changed;
        assert_eq!(
            c[0],
            SummaryEntry {
                who: Some("사용자".into()),
                at: Some("14:0x".into()),
                text: "Ctrl+Enter 없앰".into(),
                dropped: true
            }
        );
        assert!(!c[1].dropped);
        assert_eq!(c[2].who.as_deref(), Some("coordinator"));
        assert_eq!(c[2].at.as_deref(), Some("12:2x~14:5x"));
    }

    #[test]
    fn entry_without_chip_and_strike_after_chip() {
        assert_eq!(entry("그냥 글"), SummaryEntry { who: None, at: None, text: "그냥 글".into(), dropped: false });
        let e = entry("사용자 14:0x ~~Ctrl+Enter 없앰~~");
        assert!(e.dropped);
        assert_eq!(e.text, "Ctrl+Enter 없앰");
        // 「사용자」 뒤에 시각이 없으면 글로 둔다
        let e = entry("사용자 결정을 기다림");
        assert_eq!((e.who.as_deref(), e.at, e.text.as_str()), (Some("사용자"), None, "결정을 기다림"));
    }

    #[test]
    fn refuses_what_breaks_the_card() {
        assert!(parse("## 지금\n- a").unwrap_err().contains("목표"));
        assert!(parse("# 목표\n하나\n둘").unwrap_err().contains("한 줄"));
        assert!(parse("# 목표\na\n## 메모\n- b").unwrap_err().contains("모르는 머리"));
        assert!(parse("# 목표\na\n## 지금\n그냥 줄").unwrap_err().contains("`- `"));
        let four = (0..4).map(|i| format!("- {i}")).collect::<Vec<_>>().join("\n");
        assert!(parse(&format!("# 목표\na\n## 지금\n{four}")).unwrap_err().contains("3줄"));
        // 한 줄은 짧은 구(앞의 「누가 시각」은 세지 않는다)
        let long = format!("# 목표\n{}", "가".repeat(MAX_LINE + 1));
        assert!(parse(&long).unwrap_err().contains("짧은 구"));
        assert!(parse(&format!("# 목표\n{}", "가".repeat(MAX_LINE))).is_ok());
        let chip = format!("# 목표\na\n## 정함\n- 사용자 15:0x {}", "가".repeat(MAX_LINE));
        assert!(parse(&chip).is_ok());
        assert!(parse(&format!("# 목표\na\n## 끝남\n- {}", "가".repeat(MAX_LINE + 1))).is_err());
        let big = format!("# 목표\n{}\n{}", "가".repeat(30), "\n".repeat(MAX_BYTES));
        assert!(parse(&big).unwrap_err().contains("바이트"));
        assert!(parse(FORMAT).is_ok());
    }

    #[test]
    fn progress_preserves_the_whole_order_and_validates_states_and_limits() {
        let text = "# 목표\n휴대폰 표시 다듬기\n## 진행\n- [x] 표시 조건 확인\n- [~] 휴대폰에서만 숨김\n- [ ] PC·휴대폰 확인\n- [X] 기존 설정 보존\n- [ ] 변경 적용·정리";
        let p = parse(text).unwrap();
        assert_eq!(
            p.progress.iter().map(|s| (s.text.as_str(), s.state)).collect::<Vec<_>>(),
            vec![
                ("표시 조건 확인", SummaryStepState::Done),
                ("휴대폰에서만 숨김", SummaryStepState::Working),
                ("PC·휴대폰 확인", SummaryStepState::Pending),
                ("기존 설정 보존", SummaryStepState::Done),
                ("변경 적용·정리", SummaryStepState::Pending),
            ]
        );
        assert!(parse(&format!("{text}\n- [ ] 여섯째")).unwrap_err().contains("5줄"));
        for item in ["[?] 모름", "진행 중", "[x]", "[ ]", "[~]", "[x]이름", "[ ]   "] {
            assert!(parse(&format!("# 목표\n표시 확인\n## 진행\n- {item}")).is_err(), "{item}");
        }
        let boundary = format!("# 목표\n표시 확인\n## 진행\n- [ ] {}", "가".repeat(MAX_LINE));
        assert!(parse(&boundary).is_ok());
        assert!(parse(&format!("{boundary}가")).unwrap_err().contains("짧은 구"));
        assert!(parse(T0393).unwrap().progress.is_empty(), "옛 요약에 상태를 추측해 넣지 않는다");
    }

    #[test]
    fn meta_round_trips_and_hand_edits_lose_it() {
        let m = Meta { at_ms: 1_792_000_000_000, by: "coordinator".into(), seen: 14 };
        let f = file_text(&m, T0393);
        let (got, text) = split_meta(&f);
        assert_eq!(got, Some(m));
        assert_eq!(text.trim(), T0393);
        assert_eq!(split_meta(T0393), (None, T0393));
        // 칸이 빠진 표지는 표지가 아니다
        assert!(split_meta("<!-- aaa summary at=1 by=x -->\n# 목표").0.is_none());
    }

    #[test]
    fn follow_ups_keep_time_and_complete_text() {
        let task = "# 할 일 제목\n\n본문\n\n## Follow-ups\n\n### 2026-10-19 04:09Z\n\n사용자 결정(13:1x) 「추천대로」\n둘째 줄\n\n### 2026-10-19 04:32Z\n\n착지 순서\n";
        let f = follow_ups(task);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].text, "사용자 결정(13:1x) 「추천대로」\n둘째 줄");
        let long = "자르지 말고 모두 보일 지시 ".repeat(12);
        assert_eq!(follow_ups(&format!("## Follow-ups\n### 2026-10-19 04:09Z\n{long}"))[0].text, long.trim());
        assert_eq!(f[0].at_ms, aaa_common::util::parse_iso("2026-10-19T04:09:00Z"));
        assert_eq!(f[1].text, "착지 순서");
        assert_eq!(task_line(task).as_deref(), Some("할 일 제목"));
        assert_eq!(task_line("본문만 있다\n").as_deref(), Some("본문만 있다"));
        assert!(follow_ups("# 지시 없음\n").is_empty());
    }

    #[test]
    fn pending_counts_from_the_mark() {
        let c = MarkCache { at_ms: Some(5), seen: 14, total: 16 };
        assert_eq!(c.mark(), SummaryMark { at_ms: Some(5), pending: 2 });
        let none = MarkCache { at_ms: None, seen: 0, total: 3 };
        assert_eq!(none.mark().pending, 3);
    }

    #[test]
    fn decisions_come_from_the_report_section() {
        let r = "[완료] x\n\n## Report\n- 아님\n\n## Decisions\n- herdr도 앱에 보낸다\n- pbcopy 성공만\n\n## Next\n- 병합\n";
        assert_eq!(decisions(r), vec!["herdr도 앱에 보낸다", "pbcopy 성공만"]);
        assert_eq!(aaa_proto::next_lines(r), vec!["병합"]);
    }
}
