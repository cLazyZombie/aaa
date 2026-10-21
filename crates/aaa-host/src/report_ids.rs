//! r-ID는 불변 보고 판이다. 기존 이름·t-ID는 최신 사본을 계속 열며, 판의 본문·복사 파일은 별도로 보존한다.
use crate::{
    Host,
    db::LibraryReportRow,
    library::{manifest_path, safe_rel},
    orch::internal,
};
use aaa_proto::{ErrorBody, LibraryFile};
use std::path::Path;

pub(crate) fn number(name: &str) -> Option<u64> {
    let digits = name.strip_prefix("r-")?;
    (digits.len() >= 4 && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
}

impl Host {
    pub(crate) fn report_version_names(&self, project: &str, name: &str) -> Vec<String> {
        let source =
            self.db.report_target(project, name).ok().flatten().map(|v| v.source).unwrap_or_else(|| name.into());
        self.db.report_ids(project, &source).unwrap_or_default().into_iter().map(|v| v.id).collect()
    }

    pub(crate) fn report_id(&self, project: &str, name: &str) -> Option<String> {
        if let Some(v) = self.db.report_target(project, name).ok().flatten() {
            return Some(v.id);
        }
        self.report_version_names(project, name).into_iter().next()
    }

    pub(crate) fn thread_report_id(&self, project: &str, name: &str, hash: Option<&str>) -> Option<String> {
        self.db.report_ids(project, name).ok()?.into_iter().find(|v| Some(v.revision.as_str()) == hash).map(|v| v.id)
    }

    pub(crate) fn report_history_size(&self, project: &str, name: &str) -> u64 {
        let Ok(dir) = self.project_dir(project) else { return 0 };
        self.report_version_names(project, name)
            .into_iter()
            .map(|id| {
                let files: u64 = self.manifest(&dir, &id).iter().filter(|f| !f.on_agent).map(|f| f.size).sum();
                files.saturating_add(
                    std::fs::metadata(dir.join("threads").join(format!("{id}.md"))).map_or(0, |m| m.len()),
                )
            })
            .sum()
    }

    /// 호출자는 publish의 library_ops 또는 thread collect_lock으로 원본 판 교체를 직렬화한다.
    /// 예약은 먼저 영속화한다. 도중 오류·재시작의 빈 번호는 다시 쓰지 않고, ready 뒤에만 노출한다.
    pub(crate) fn snapshot_report_version(
        &self,
        project: &str,
        source: &str,
        revision: &str,
        root: &Path,
        files: &[LibraryFile],
        report: Option<&[u8]>,
        meta: &LibraryReportRow,
    ) -> Result<String, ErrorBody> {
        let dir = self.project_dir(project)?;
        // 예전에 이름으로 쓴 r-NNNN도 덮어쓰지 않는다.
        let floor = std::fs::read_dir(dir.join("threads"))
            .map_err(internal)?
            .filter_map(Result::ok)
            .filter_map(|e| e.file_name().to_str().and_then(|s| s.strip_suffix(".files.json")).and_then(number))
            .max()
            .unwrap_or(0);
        let id = self.db.report_reserve(project, source, revision, floor).map_err(internal)?;
        let dest = dir.join("library").join(&id);
        std::fs::create_dir_all(&dest).map_err(internal)?;
        let result = (|| {
            let root = std::fs::canonicalize(root).map_err(internal)?;
            for f in files.iter().filter(|f| !f.on_agent) {
                if !safe_rel(&f.path) {
                    return Err(internal("보존할 보고 파일 경로가 안전하지 않다"));
                }
                let (input, size) = crate::library_publish::live_file(&root, &f.path)?;
                if size != f.size {
                    return Err(internal("보고 판 보존 중 파일 크기가 바뀌었다"));
                }
                let path = dest.join(&f.path);
                std::fs::create_dir_all(path.parent().unwrap()).map_err(internal)?;
                let mut output = std::fs::File::create(path).map_err(internal)?;
                use std::io::Read;
                let copied = std::io::copy(&mut input.take(size.saturating_add(1)), &mut output).map_err(internal)?;
                if copied != size {
                    return Err(internal("보고 판 보존 중 파일 크기가 바뀌었다"));
                }
            }
            crate::orch::write_atomic(&manifest_path(&dir, &id), &serde_json::to_vec(files).map_err(internal)?)
                .map_err(internal)?;
            if let Some(report) = report {
                crate::orch::write_atomic(&dir.join("threads").join(format!("{id}.md")), report).map_err(internal)?;
            }
            self.db
                .report_ready(
                    &LibraryReportRow {
                        name: id.clone(),
                        live: None,
                        to_user: false,
                        sent_at: None,
                        seen_at: None,
                        ..meta.clone()
                    },
                    meta,
                )
                .map_err(internal)?;
            Ok(id.clone())
        })();
        if result.is_err() {
            let _ = std::fs::remove_dir_all(&dest);
            let _ = std::fs::remove_file(manifest_path(&dir, &id));
            let _ = std::fs::remove_file(dir.join("threads").join(format!("{id}.md")));
        }
        result
    }

    /// 기존 host의 최신 사본만 처음 한 번 등록한다. 과거 판을 추측해 복원하지 않는다.
    pub(crate) fn backfill_report_ids(&self) -> Result<(), ErrorBody> {
        for project in self.projects_all() {
            let dir = self.project_dir(&project.name)?;
            let entries = match std::fs::read_dir(dir.join("threads")) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(internal(e)),
            };
            let mut names = entries
                .filter_map(Result::ok)
                .filter_map(|e| e.file_name().to_str().and_then(|s| s.strip_suffix(".files.json")).map(str::to_string))
                .collect::<Vec<_>>();
            names.sort();
            for name in names {
                if self.db.report_reserved(&project.name, &name).map_err(internal)?
                    || self.report_id(&project.name, &name).is_some()
                {
                    continue;
                }
                let result = (|| {
                    let rec = self.load(&project.name, &name).ok();
                    let meta =
                        self.db.library_report(&project.name, &name).map_err(internal)?.unwrap_or(LibraryReportRow {
                            project: project.name.clone(),
                            name: name.clone(),
                            updated: rec.as_ref().and_then(|r| r.report_changed_ms).unwrap_or(0),
                            from_thread: rec.as_ref().map(|r| r.id.clone()),
                            ..Default::default()
                        });
                    let root = meta
                        .live
                        .as_ref()
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|| dir.join("library").join(&name));
                    let report = std::fs::read(dir.join("threads").join(format!("{name}.md"))).ok();
                    let files = self.report_manifest(&dir, &project.name, &name)?;
                    self.snapshot_report_version(
                        &project.name,
                        &name,
                        rec.as_ref().and_then(|r| r.report_hash.as_deref()).unwrap_or("legacy"),
                        &root,
                        &files,
                        report.as_deref(),
                        &meta,
                    )
                })();
                if let Err(e) = result {
                    tracing::warn!(project=%project.name,report=%name,"기존 보고 판 보존 실패: {}",e.message);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
