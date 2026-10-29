//! host 보관 기록만 검색한다. agent·PTY·외부 저장소를 읽지 않는다.
use crate::{Host, err, orch::internal};
use aaa_proto::{ErrorBody, ErrorCode, RecordDocument, RecordHit, RecordKind, RecordSearchPage, RecordSearchQuery};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::OnceLock};

pub(crate) fn normalize(value: &str) -> String {
    static IDS: OnceLock<regex::Regex> = OnceLock::new();
    let lower = value.to_lowercase();
    IDS.get_or_init(|| regex::Regex::new(r"\b[tdrqa]-[0-9]+\b").unwrap())
        .replace_all(&lower, |c: &regex::Captures<'_>| {
            aaa_proto::canonical_short_id(&c[0], c[0].chars().next().unwrap()).unwrap_or_else(|| c[0].into())
        })
        .into_owned()
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    scope: String,
    at: u64,
    key: String,
    project: String,
}

fn document(
    project: &str,
    kind: RecordKind,
    id: &str,
    title: &str,
    source: &str,
    at_ms: u64,
    body: String,
) -> RecordDocument {
    let body = aaa_common::redact::text(&body);
    let id = aaa_common::redact::text(id);
    let key = hex::encode(Sha256::digest(serde_json::to_vec(&(kind, &id, &body)).unwrap()));
    RecordDocument {
        hit: RecordHit {
            key,
            project: project.into(),
            kind,
            id: preview(&id, 80),
            title: preview(&aaa_common::redact::text(title), 300),
            source: preview(source, 160),
            at_ms,
            excerpt: String::new(),
            report_id: None,
        },
        body,
        next: None,
    }
}

// 목록 메타데이터만 줄인다. 원문 body와 그 판의 해시는 전체 글을 보존한다.
fn preview(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let mut out: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        out.push('…');
    }
    out
}

fn read_optional(path: &Path) -> Result<Option<String>, ErrorBody> {
    match std::fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(err(ErrorCode::Internal, format!("검색 원문 읽기 {}: {e}", path.display()))),
    }
}

impl Host {
    fn records_sync(&self, project: &str) -> Result<(), ErrorBody> {
        let dir = self.project_dir(project)?;
        // TASKS 편집·삭제와 같은 잠금 순서. DB 잠금 중 파일을 읽지 않는다.
        let lock = self.file_locks.of(&dir.join("TASKS.md"));
        let _guard = lock.lock().unwrap();
        self.project_dir(project)?;
        let mut rows = Vec::new();
        let catalog = self.db.library_catalog(project).map_err(internal)?;
        let cache = self.db.record_report_cache(project).map_err(internal)?;
        for (source, versions) in catalog.versions {
            for id in versions {
                if !crate::library::safe_rel(&id) || id.contains('/') {
                    continue;
                }
                let origin = format!("report:{id}");
                let path = dir.join("threads").join(format!("{id}.md"));
                if !path.is_file() {
                    continue;
                }
                if let Some(cached) = cache.get(&origin) {
                    let mut doc = cached.clone();
                    doc.hit.project = project.into();
                    doc.hit.at_ms = catalog.rows.get(&id).map_or(0, |r| r.updated);
                    doc.hit.source = origin.clone();
                    doc.hit.report_id = Some(id);
                    rows.push((origin, doc));
                    continue;
                }
                let Some(body) = read_optional(&path)? else {
                    continue;
                };
                if !crate::report_mark::done(crate::report_mark::marker(&body)) {
                    continue;
                }
                let at = catalog.rows.get(&id).map_or(0, |r| r.updated);
                let title = body.lines().next().unwrap_or(&source).to_string();
                let mut doc = document(project, RecordKind::Report, &source, &title, &origin, at, body);
                doc.hit.report_id = Some(id);
                rows.push((origin, doc));
            }
        }
        for r in self.db.ask_list(project).map_err(internal)? {
            let Some(answer) = r.answer.as_ref() else {
                continue;
            };
            for (n, q) in r.items.iter().enumerate() {
                let id = q.id.as_deref().unwrap_or(&r.id);
                let mut body = format!("### {}\n\n{}\n", q.text, q.body.as_deref().unwrap_or(""));
                for (i, c) in q.choices.iter().enumerate() {
                    body.push_str(&format!(
                        "\n- {} {}{}{}\n{}",
                        c.letter_at(i),
                        c.label,
                        if c.recommended { " (추천)" } else { "" },
                        c.description.as_ref().map(|s| format!(" — {s}")).unwrap_or_default(),
                        c.body.as_deref().unwrap_or("")
                    ));
                }
                body.push_str("\n\n## 답\n\n");
                if let Some(Some(pick)) = answer.picks.get(n)
                    && let Some(c) = q.choices.get(*pick as usize)
                {
                    body.push_str(&format!("{} {}\n\n", c.letter_at(*pick as usize), c.label));
                }
                if let Some(Some(comment)) = answer.comments.get(n) {
                    body.push_str(comment);
                    body.push_str("\n\n");
                }
                if let Some(text) = &answer.text {
                    body.push_str(text);
                    body.push_str("\n\n");
                }
                body.push_str(&answer.summary);
                let origin = format!("question:{}:{n}", r.id);
                let mut doc = document(project, RecordKind::Question, id, &q.text, &origin, r.done_ms, body);
                doc.hit.report_id = r.report_id.clone();
                rows.push((origin, doc));
            }
        }
        let path = dir.join("TASKS.md");
        if let Some(body) = read_optional(&path)? {
            let at = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64);
            self.record_tasks(project, "current", at, &body, &mut rows)?;
        }
        for a in self.db.archive_list(project).map_err(internal)? {
            if a.committed {
                self.record_tasks(project, &format!("archive:{}", a.task_id), a.completed_ms, &a.body, &mut rows)?;
            }
        }
        for e in self.db.task_log_list(project, None, u32::MAX, None).map_err(internal)? {
            for (side, body) in [("before", e.before), ("after", e.after)] {
                if let Some(body) = body {
                    self.record_tasks(project, &format!("log:{}:{side}", e.id), e.at_ms, &body, &mut rows)?;
                }
            }
        }
        self.db
            .records_replace(project, &rows)
            .map_err(|e| err(ErrorCode::Internal, format!("작업 기록 색인 {project}: {e:#}")))
    }

    fn record_tasks(
        &self,
        project: &str,
        source: &str,
        at: u64,
        body: &str,
        rows: &mut Vec<(String, RecordDocument)>,
    ) -> Result<(), ErrorBody> {
        let mut tasks = crate::tasks::parse(body);
        self.fill_task_ids(project, &mut tasks)?;
        let lines: Vec<_> = body.split_inclusive('\n').collect();
        for t in tasks {
            let start = t.line as usize - 1;
            let indent = lines[start].len() - lines[start].trim_start().len();
            let mut end = start + 1;
            for (i, line) in lines.iter().enumerate().skip(start + 1) {
                if line.trim().is_empty() {
                    continue;
                }
                let trimmed = line.trim_start();
                if line.len() - trimmed.len() <= indent || trimmed.starts_with('#') || crate::tasks::is_task(line) {
                    break;
                }
                end = i + 1;
            }
            let block = lines[start..end].concat();
            let id = t.short_id.as_deref().or(t.id.as_deref()).unwrap_or(&t.title);
            let origin = format!("task:{source}:{}", t.line);
            rows.push((origin.clone(), document(project, RecordKind::Task, id, &t.title, &origin, at, block)));
        }
        Ok(())
    }

    pub(crate) fn records_search(&self, q: RecordSearchQuery) -> Result<RecordSearchPage, ErrorBody> {
        let terms: Vec<String> = normalize(&q.query).split_whitespace().map(str::to_string).collect();
        if terms.is_empty() || q.query.len() > 1024 || terms.len() > 16 {
            return Err(err(ErrorCode::BadRequest, "검색어는 1~1024바이트, 16단어 이내로 쓴다"));
        }
        let mut projects = if let Some(project) = &q.project {
            self.project_dir(project)?;
            vec![project.clone()]
        } else {
            self.projects_all().into_iter().map(|p| p.name).collect::<Vec<_>>()
        };
        projects.sort();
        let scope = hex::encode(Sha256::digest(serde_json::to_vec(&(&projects, &terms, q.kind)).map_err(internal)?));
        let after = q
            .after
            .as_ref()
            .map(|s| -> Result<Cursor, ErrorBody> {
                let bad = || err(ErrorCode::BadRequest, "검색 커서가 잘못됐거나 검색 조건이 달라졌다");
                if s.len() > 2048 {
                    return Err(bad());
                }
                let c: Cursor =
                    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(s).map_err(|_| bad())?).map_err(|_| bad())?;
                if c.scope != scope || c.at > i64::MAX as u64 {
                    return Err(bad());
                }
                Ok(c)
            })
            .transpose()?;
        for project in &projects {
            self.records_sync(project)?;
        }
        let limit = q.limit.unwrap_or(30).clamp(1, 100) as usize;
        let docs = self
            .db
            .record_search_rows(
                &projects,
                q.kind,
                &terms,
                after.as_ref().map(|c| (c.at, c.key.as_str(), c.project.as_str())),
                limit + 1,
            )
            .map_err(|e| err(ErrorCode::Internal, format!("작업 기록 검색: {e:#}")))?;
        let more = docs.len() > limit;
        let mut hits = Vec::new();
        for doc in docs.into_iter().take(limit) {
            let mut hit = doc.hit;
            hit.excerpt = excerpt(&doc.body, &terms);
            hits.push(hit);
        }
        let next = if more {
            hits.last().map(|h| {
                URL_SAFE_NO_PAD.encode(
                    serde_json::to_vec(&Cursor { scope, at: h.at_ms, key: h.key.clone(), project: h.project.clone() })
                        .unwrap(),
                )
            })
        } else {
            None
        };
        Ok(RecordSearchPage { hits, next })
    }

    pub(crate) fn records_chunk(
        &self,
        project: &str,
        key: &str,
        offset: Option<u64>,
    ) -> Result<RecordDocument, ErrorBody> {
        let mut doc = self.records_open(project, key)?;
        let start = usize::try_from(offset.unwrap_or(0))
            .map_err(|_| err(ErrorCode::BadRequest, "원문 위치가 범위를 넘는다"))?;
        if start > doc.body.len() || !doc.body.is_char_boundary(start) {
            return Err(err(ErrorCode::BadRequest, "원문 위치가 UTF-8 글자 경계가 아니다"));
        }
        let mut end = (start + 128 * 1024).min(doc.body.len());
        while !doc.body.is_char_boundary(end) {
            end -= 1;
        }
        doc.next = (end < doc.body.len()).then_some(end as u64);
        doc.body = doc.body[start..end].into();
        Ok(doc)
    }

    pub(crate) fn records_open(&self, project: &str, key: &str) -> Result<RecordDocument, ErrorBody> {
        self.project_dir(project)?;
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err(ErrorCode::BadRequest, "원문 판 해시가 잘못됐다"));
        }
        let mut doc = self
            .db
            .record_get(project, key)
            .map_err(internal)?
            .ok_or_else(|| err(ErrorCode::NotFound, "검색된 원문 판이 없다. 기록이 정리됐을 수 있다"))?;
        doc.hit.project = project.into();
        // 같은 본문의 중복 판 중 하나가 삭제돼도 남아 있는 동일 본문 판을 연다.
        if doc.hit.kind == RecordKind::Report {
            let id = self
                .db
                .record_report_source(project, key)
                .map_err(internal)?
                .ok_or_else(|| err(ErrorCode::NotFound, "검색된 보고 판이 삭제됐다"))?;
            doc.hit.source = format!("report:{id}");
            doc.hit.report_id = Some(id);
        }
        Ok(doc)
    }
}

fn excerpt(body: &str, terms: &[String]) -> String {
    // 숨은 task 식별자는 원문에만 둔다. 결과 미리보기는 문장으로 읽는다.
    static TASK_MARK: OnceLock<regex::Regex> = OnceLock::new();
    let visible =
        TASK_MARK.get_or_init(|| regex::Regex::new(r"<!--\s*aaa-task:[^>]*-->").unwrap()).replace_all(body, "");
    let compact = visible.split_whitespace().collect::<Vec<_>>().join(" ");
    let body = compact.as_str();
    // 소문자화로 바이트 길이가 달라지는 글자도 있어 원문 char 위치로 다시 찾는다.
    let chars: Vec<char> = body.chars().collect();
    let lower = body.to_lowercase();
    let byte = terms.iter().filter_map(|t| lower.find(t)).min().unwrap_or(0);
    let pos = lower[..byte].chars().count().min(chars.len());
    let start = pos.saturating_sub(60);
    let end = (start + 240).min(chars.len());
    format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        chars[start..end].iter().collect::<String>(),
        if end < chars.len() { "…" } else { "" }
    )
}

#[cfg(test)]
mod tests;
