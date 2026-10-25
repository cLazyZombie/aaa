//! 영구 삭제는 링크 폐기 → 공개 경로 분리 → 원장 확정을 응답하고 실제 제거는 배경에서 한다.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod moves;
mod queue;
pub(crate) use queue::Reaper;
use std::time::Instant;

use aaa_proto::{ErrorBody, ErrorCode, FrontResponse, LibraryRef, ThreadStatus};

use crate::library::{manifest_path, safe_rel};
use crate::{Host, err, orch::internal};

struct DeleteProject {
    dir: PathBuf,
    catalog: crate::db::LibraryCatalog,
    sources: HashMap<String, String>,
    protected: HashSet<String>,
    resolved: HashSet<String>,
}

struct DeleteFamily {
    project: String,
    name: String,
    copies: Vec<LibraryRef>,
    dir: PathBuf,
}

impl Host {
    fn library_delete_plan(&self, reports: &[LibraryRef]) -> Result<Vec<DeleteFamily>, ErrorBody> {
        let mut projects = HashMap::new();
        let mut seen = HashSet::new();
        let mut families = Vec::new();
        for report in reports {
            if !projects.contains_key(&report.project) {
                let dir = self.project_dir(&report.project)?;
                let catalog = self.db.library_catalog(&report.project).map_err(internal)?;
                // 완료 보관·thread 상태는 보고서마다 다시 읽지 않는다. 읽기에 실패하면 파일을 건드리지 않는다.
                let archived = self.db.archive_list(&report.project).map_err(internal)?;
                let protected = crate::library_catalog::referenced_names(&archived);
                let resolved = catalog
                    .threads
                    .iter()
                    .filter_map(|(id, data)| {
                        let rec: crate::orch::ThreadRec = serde_json::from_str(data)
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "thread 상태를 읽지 못해 삭제에서 보호한다")
                            })
                            .ok()?;
                        (rec.status == ThreadStatus::Resolved).then(|| id.clone())
                    })
                    .collect();
                let sources = catalog
                    .versions
                    .iter()
                    .flat_map(|(source, ids)| ids.iter().map(move |id| (id.clone(), source.clone())))
                    .collect();
                projects.insert(report.project.clone(), DeleteProject { dir, catalog, sources, protected, resolved });
            }
            let project = &projects[&report.project];
            let name = project.sources.get(&report.name).unwrap_or(&report.name);
            if !seen.insert((report.project.clone(), name.clone())) {
                continue;
            }
            let versions = project.catalog.versions.get(name).cloned().unwrap_or_default();
            if !safe_rel(name)
                || name.contains('/')
                || (!manifest_path(&project.dir, name).exists()
                    && !project.catalog.rows.contains_key(name)
                    && versions.is_empty())
            {
                return Err(err(ErrorCode::NotFound, format!("보고서가 없다: {}/{name}", report.project)));
            }
            if (name.starts_with("t-") && !project.resolved.contains(name))
                || project.protected.contains(name)
                || versions.iter().any(|id| project.protected.contains(id))
            {
                return Err(err(
                    ErrorCode::BadRequest,
                    format!(
                        "끝나지 않은(resolve 전) thread 또는 완료 보관이 참조하는 보고는 지울 수 없다: {}/{name}",
                        report.project
                    ),
                ));
            }
            let copies = versions
                .into_iter()
                .chain(std::iter::once(name.clone()))
                .map(|name| LibraryRef { project: report.project.clone(), name })
                .collect();
            families.push(DeleteFamily {
                project: report.project.clone(),
                name: name.clone(),
                copies,
                dir: project.dir.clone(),
            });
        }
        Ok(families)
    }

    /// 실시간 등록의 원본은 남기며 r 번호로 골라도 원본과 보존 판 전체를 지운다.
    pub(crate) fn library_delete(&self, reports: &[LibraryRef], actor: &str) -> Result<FrontResponse, ErrorBody> {
        if reports.is_empty() || reports.len() > 500 {
            return Err(err(ErrorCode::BadRequest, "지울 보고서는 1~500개다"));
        }
        let started = Instant::now();
        let _lock = self.library_ops.lock().unwrap();
        let lock_us = started.elapsed().as_micros() as u64;
        let prepare_started = Instant::now();
        let families = self.library_delete_plan(reports)?;
        let prepare_us = prepare_started.elapsed().as_micros() as u64;
        let copies: Vec<_> = families.iter().flat_map(|f| f.copies.iter().cloned()).collect();
        let reaper = self.report_delete_reaper()?;
        let revoke_started = Instant::now();
        // 폐기 commit에 실패하면 파일 삭제를 시작하지 않는다.
        self.db.library_delete_revoke(&copies).map_err(internal)?;
        let revoke_us = revoke_started.elapsed().as_micros() as u64;
        {
            let names: HashSet<_> = copies.iter().map(|r| (r.project.as_str(), r.name.as_str())).collect();
            self.view_caps
                .lock()
                .unwrap()
                .retain(|_, cap| !names.contains(&(cap.project.as_str(), cap.thread.as_str())));
        }
        let files_started = Instant::now();
        let mut completed_copies = Vec::new();
        let mut removed = Vec::new();
        let mut batches = HashMap::new();
        let mut copy_index = 0;
        let mut moves = moves::Moves::new()
            .map_err(|error| err(ErrorCode::Unavailable, format!("보고서 경로 이동 작업자 시작 실패: {error}")))?;
        let files_result: Result<(), ErrorBody> = (|| {
            for family in &families {
                let batch = match batches.entry(family.project.clone()) {
                    std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::hash_map::Entry::Vacant(entry) => {
                        entry.insert(self.report_delete_batch(&family.dir)?)
                    }
                };
                detach_family(family, batch, copy_index, &mut moves, &mut completed_copies)?;
                copy_index += family.copies.len();
                removed.push(format!("{}/{}", family.project, family.name));
            }
            Ok(())
        })();
        let detach_us = files_started.elapsed().as_micros() as u64;
        let finish_started = Instant::now();
        // 파일 중간 실패도 이미 끝난 사본만 반영한다. 남은 행·판 번호는 다시 고를 수 있게 남긴다.
        let finish_result = self.db.library_delete_finish(&completed_copies, &removed, actor).map_err(internal);
        let finish_us = finish_started.elapsed().as_micros() as u64;
        let queued_batches = batches.len();
        for batch in batches.into_values() {
            reaper.submit(batch);
        }
        self.reports_bump();
        tracing::info!(
            requested = reports.len(),
            families = families.len(),
            copies = copies.len(),
            completed = removed.len(),
            completed_copies = completed_copies.len(),
            lock_us,
            prepare_us,
            revoke_us,
            detach_us,
            finish_us,
            queued_batches,
            elapsed_us = started.elapsed().as_micros() as u64,
            success = files_result.is_ok() && finish_result.is_ok(),
            "보고서 영구 삭제 계측"
        );
        // 두 단계가 함께 실패해도 원래 파일 오류를 잃지 않는다.
        if let Err(mut error) = files_result {
            if let Err(finish_error) = finish_result {
                error.message.push_str(&format!("; 삭제 원장 확정 실패: {}", finish_error.message));
            }
            error.message.push_str(&format!(
                "; {}개 중 {}개 완료, {}개 남음",
                families.len(),
                removed.len(),
                families.len() - removed.len()
            ));
            return Err(error);
        }
        finish_result?;
        Ok(FrontResponse::LibraryPruned { removed })
    }
}

/// 각 rename은 같은 볼륨에서 원자적이다. EXDEV 등의 실패를 copy/remove로 우회하지 않는다.
fn detach_family(
    family: &DeleteFamily,
    batch: &Path,
    first: usize,
    moves: &mut moves::Moves,
    completed: &mut Vec<LibraryRef>,
) -> Result<(), ErrorBody> {
    let mut paths = Vec::new();
    for (offset, copy) in family.copies.iter().enumerate() {
        let index = first + offset;
        paths.push((
            offset,
            family.dir.join("library").join(&copy.name),
            batch.join(format!("{index}-files")),
            false,
            "사본",
        ));
        paths.push((
            offset,
            manifest_path(&family.dir, &copy.name),
            batch.join(format!("{index}-manifest")),
            true,
            "목록",
        ));
        if copy.name.starts_with("t-") || crate::report_ids::number(&copy.name).is_some() {
            paths.push((
                offset,
                family.dir.join("threads").join(format!("{}.md", copy.name)),
                batch.join(format!("{index}-report")),
                true,
                "본문",
            ));
        }
    }
    let tasks: Vec<_> =
        paths.iter().map(|(_, source, dest, file_only, _)| (source.clone(), dest.clone(), *file_only)).collect();
    let results = moves
        .run(&tasks)
        .map_err(|error| err(ErrorCode::Unavailable, format!("보고서 경로 이동 응답 실패: {error}")))?;
    let mut failed = HashSet::new();
    let mut first_error = None;
    for ((offset, source, dest, _, label), result) in paths.into_iter().zip(results) {
        if let Err(error) = result {
            failed.insert(offset);
            let copy = &family.copies[offset];
            let error = err(
                ErrorCode::Unavailable,
                format!(
                    "보고서 {}/{} {label} 삭제 실패: {} → {}: {error} (OS {:?})",
                    copy.project,
                    copy.name,
                    source.display(),
                    dest.display(),
                    error.raw_os_error()
                ),
            );
            if first_error.is_none() {
                first_error = Some(error);
            } else {
                tracing::warn!("보고서 경로 이동 추가 실패: {}", error.message);
            }
        }
    }
    completed.extend(
        family.copies.iter().enumerate().filter(|(index, _)| !failed.contains(index)).map(|(_, copy)| copy.clone()),
    );
    first_error.map_or(Ok(()), Err)
}
