//! 영구 삭제는 보호 판정을 먼저 끝내고 링크 폐기 → 파일 삭제 → 성공 원장 확정 순서로 한다.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
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
            families.push(DeleteFamily { project: report.project.clone(), name: name.clone(), copies });
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
        let files_result: Result<(), ErrorBody> = (|| {
            for family in &families {
                for copy in &family.copies {
                    self.remove_report_files(&copy.project, &copy.name, true)?;
                    completed_copies.push(copy.clone());
                }
                removed.push(format!("{}/{}", family.project, family.name));
            }
            Ok(())
        })();
        let files_us = files_started.elapsed().as_micros() as u64;
        let finish_started = Instant::now();
        // 파일 중간 실패도 이미 끝난 사본만 반영한다. 남은 행·판 번호는 다시 고를 수 있게 남긴다.
        let finish_result = self.db.library_delete_finish(&completed_copies, &removed, actor).map_err(internal);
        let finish_us = finish_started.elapsed().as_micros() as u64;
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
            files_us,
            finish_us,
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
