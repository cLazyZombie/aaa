//! 목록은 원장·보관 참조를 프로젝트마다 한 번, manifest를 사본마다 한 번 읽는다.
//! 첨부 원문·해시는 읽지 않으며 live 폴더만 기존 안전한 순회로 현재 크기를 구한다.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use aaa_proto::{ErrorBody, LibraryReport, ThreadStatus};

use crate::library::{manifest_path, safe_rel};
use crate::{Host, orch::internal};

fn modified_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |t| t.as_millis() as u64)
}

fn referenced_names(rows: &[aaa_proto::ArchivedTask]) -> HashSet<String> {
    let mut names = HashSet::new();
    let mut body = |text: &str| {
        for path in crate::task_attach::refs(text) {
            if let Some((name, _)) = path.strip_prefix("library/").and_then(|p| p.split_once('/')) {
                names.insert(name.to_string());
            }
        }
    };
    for row in rows {
        body(&row.body);
        for h in &row.history {
            if let Some(text) = &h.before {
                body(text);
            }
            if let Some(text) = &h.after {
                body(text);
            }
        }
    }
    for row in rows {
        names.extend(row.task.thread.iter().cloned());
        names.extend(row.history.iter().filter_map(|h| h.thread.clone()));
    }
    names
}

impl Host {
    pub(crate) fn library_reports(&self, project: Option<&str>, reveal: bool) -> Result<Vec<LibraryReport>, ErrorBody> {
        let projects = if let Some(p) = project {
            self.project_dir(p)?;
            vec![p.to_string()]
        } else {
            self.projects_all().into_iter().map(|p| p.name).collect()
        };
        let mut reports = Vec::new();
        for p in projects {
            let dir = self.project_dir(&p)?;
            let catalog = self.db.library_catalog(&p).map_err(internal)?;
            // 보관을 읽지 못하면 예전 protected_copy처럼 모두 보호한다.
            let protected = self.db.archive_list(&p).ok().map(|rows| referenced_names(&rows));
            let resolved: HashSet<_> = catalog
                .threads
                .iter()
                .filter_map(|(id, json)| {
                    let rec: crate::orch::ThreadRec = serde_json::from_str(json).ok()?;
                    (aaa_proto::valid_thread(id) && rec.status == ThreadStatus::Resolved).then_some(id.as_str())
                })
                .collect();
            let mut links = HashMap::new();
            if reveal {
                // DB는 최신 순서다. 폐기된 최신 링크도 그대로 보인다.
                for link in self.db.library_links(Some(&p)).unwrap_or_default() {
                    links.entry(link.thread.clone()).or_insert(link);
                }
            }
            let mut sizes = HashMap::new();
            // 보존 판은 서로 다른 source에서 중복 읽지 않으며 삭제/예약 판은 계산에 넣지 않는다.
            for id in catalog.versions.values().flatten() {
                let bytes: u64 = self.manifest(&dir, id).iter().filter(|f| !f.on_agent).map(|f| f.size).sum();
                let text = std::fs::metadata(dir.join("threads").join(format!("{id}.md"))).map_or(0, |m| m.len());
                sizes.insert(id.as_str(), bytes.saturating_add(text));
            }
            for e in std::fs::read_dir(dir.join("threads")).map_err(crate::library_publish::io_error)? {
                let e = e.map_err(crate::library_publish::io_error)?;
                let file_name = e.file_name();
                let Some(name) = file_name
                    .to_str()
                    .and_then(|s| s.strip_suffix(".files.json"))
                    .filter(|s| safe_rel(s) && !catalog.reserved.contains(*s))
                else {
                    continue;
                };
                let row = catalog.rows.get(name);
                let versions = catalog.versions.get(name).cloned().unwrap_or_default();
                let report_id = versions.first().cloned();
                let history_size = versions.iter().filter_map(|id| sizes.get(id.as_str())).sum();
                let files = if row.is_some_and(|r| r.live.is_some()) {
                    self.report_manifest(&dir, &p, name)
                } else {
                    Ok(self.manifest(&dir, name))
                };
                let (size, problem) = match files {
                    Ok(files) => {
                        (files.iter().filter(|f| !f.on_agent && safe_rel(&f.path)).map(|f| f.size).sum(), None)
                    }
                    Err(e) => (0, Some(e.message)),
                };
                let busy = (name.starts_with("t-") && !resolved.contains(name))
                    || protected
                        .as_ref()
                        .is_none_or(|names| names.contains(name) || versions.iter().any(|v| names.contains(v)));
                let from_thread = row.and_then(|r| r.from_thread.clone()).or_else(|| {
                    crate::library_publish::thread_numbers(name).into_iter().find(|t| catalog.threads.contains_key(t))
                });
                let link = links.remove(name).map(|r| self.link_view_summary(r, size, report_id.clone()));
                reports.push(LibraryReport {
                    project: p.clone(),
                    name: name.into(),
                    report_id,
                    versions,
                    history_size,
                    size,
                    updated_at_ms: row.map_or_else(|| modified_ms(&manifest_path(&dir, name)), |r| r.updated),
                    live: row.is_some_and(|r| r.live.is_some()),
                    problem,
                    link,
                    to_user: row.is_some_and(|r| r.to_user),
                    title: row.and_then(|r| r.title.clone()),
                    unseen: row.is_some_and(|r| r.unseen()),
                    busy,
                    from_thread,
                });
            }
        }
        reports.sort_by(|a, b| {
            b.updated_at_ms.cmp(&a.updated_at_ms).then_with(|| (&a.project, &a.name).cmp(&(&b.project, &b.name)))
        });
        Ok(reports)
    }
}
