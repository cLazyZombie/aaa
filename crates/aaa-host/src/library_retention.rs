//! host 사본만 정리한다. 운영 원본·실시간 등록 폴더·thread 기록은 건드리지 않는다(§8.212).
use aaa_proto::{
    ErrorBody, ErrorCode, FrontResponse, LibraryPruneEntry, LibraryPrunePlan, LibraryReport, ThreadStatus,
};

use crate::library::manifest_path;
use crate::{Host, err};

const DAY_MS: u64 = 86_400_000;
const EVERY_MS: u64 = 6 * 3_600_000;
const LAST_PASS: &str = "library_retention_last_pass";
const OVERFLOW: &str = "library_retention_overflow";

impl Host {
    pub(crate) fn protected_copy(&self, r: &LibraryReport) -> bool {
        if let Ok(Some(version)) = self.db.report_target(&r.project, &r.name) {
            let mut source = r.clone();
            source.name = version.source;
            return self.protected_copy(&source);
        }
        // t- 보고서는 thread가 없거나 읽지 못해도 안전하게 남긴다.
        if r.name.starts_with("t-")
            && self.load(&r.project, &r.name).map_or(true, |t| t.status != ThreadStatus::Resolved)
        {
            return true;
        }
        let versions = match self.db.report_ids(&r.project, &r.name) {
            Ok(versions) => versions,
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                return true;
            }
        };
        // 복원 뒤에도 완료 당시 결과와 메모는 보관에 남는다. 미확정 보관도 먼저 보호하며,
        // DB를 읽지 못하면 유일한 결과 사본을 정리하지 않는다.
        self.db.archive_list(&r.project).map_or(true, |rows| {
            rows.iter().any(|a| {
                let refers = |body: &str| {
                    crate::task_attach::refs(body).iter().any(|path| {
                        path.strip_prefix("library/")
                            .and_then(|p| p.split_once('/'))
                            .is_some_and(|(name, _)| name == r.name || versions.iter().any(|v| v.id == name))
                    })
                };
                a.task.thread.as_deref() == Some(r.name.as_str())
                    || refers(&a.body)
                    || a.history.iter().any(|h| {
                        h.thread.as_deref() == Some(r.name.as_str())
                            || h.before.as_deref().is_some_and(refers)
                            || h.after.as_deref().is_some_and(refers)
                    })
            })
        })
    }

    fn retention_plan(
        &self,
        project: Option<&str>,
        days: u64,
        limit: u64,
        now: u64,
    ) -> Result<LibraryPrunePlan, ErrorBody> {
        if let Some(p) = project {
            self.project_dir(p)?;
        }
        // --project로 후보를 제한해도 상한은 host 전체 합계다.
        let reports = self.library_inventory()?;
        let archived_size = reports.iter().filter(|r| r.archived).map(|r| r.size).sum();
        let size = reports.iter().filter(|r| !r.archived).map(|r| r.size).sum();
        let protected_size = reports.iter().filter(|r| !r.archived && r.protected).map(|r| r.size).sum();
        let mut plan = LibraryPrunePlan {
            size,
            archived_size,
            remaining_size: size,
            protected_size,
            max_size: limit,
            max_days: days,
            candidates: Vec::new(),
        };
        if limit > 0 && protected_size > limit {
            return Ok(plan);
        }
        // 최신 이름과 그 r 판을 용량 때문에 절반만 지우면 남은 별칭이 옛 r 번호를 가리킬 수 있다.
        // 보관한 판은 별도 family라 이 묶음 삭제에도 들어가지 않는다.
        let mut families = std::collections::BTreeMap::new();
        for r in reports {
            families.entry((r.project.clone(), r.family.clone())).or_insert_with(Vec::new).push(r);
        }
        let mut families: Vec<_> = families.into_values().collect();
        for family in &mut families {
            family.sort_by_key(|r| r.name != r.family);
        }
        families.sort_by(|a, b| {
            let (a, b) = (&a[0], &b[0]);
            a.updated_at_ms
                .cmp(&b.updated_at_ms)
                .then_with(|| a.project.cmp(&b.project))
                .then_with(|| aaa_proto::cmp_short_ids(&a.family, &b.family))
        });
        let cutoff = now.saturating_sub(days.saturating_mul(DAY_MS));
        let mut capacity = Vec::new();
        let select = |family: Vec<crate::library_inventory::LibraryCopy>, reason: &str, plan: &mut LibraryPrunePlan| {
            for r in family {
                plan.remaining_size = plan.remaining_size.saturating_sub(r.size);
                plan.candidates.push(LibraryPruneEntry {
                    project: r.project,
                    name: r.name,
                    size: r.size,
                    reason: reason.into(),
                });
            }
        };
        for family in families {
            if family.iter().any(|r| r.archived || r.protected || project.is_some_and(|p| p != r.project)) {
                continue;
            }
            if days > 0 && family[0].updated_at_ms < cutoff {
                select(family, "기한", &mut plan);
            } else {
                capacity.push(family);
            }
        }
        for family in capacity {
            if limit == 0 || plan.remaining_size <= limit {
                break;
            }
            select(family, "용량", &mut plan);
        }

        Ok(plan)
    }

    pub(crate) fn library_prune_preview(
        &self,
        project: Option<&str>,
        days: Option<u32>,
        now: u64,
    ) -> Result<LibraryPrunePlan, ErrorBody> {
        let _lock = self.library_ops.lock().unwrap();
        self.retention_plan(
            project,
            days.map(u64::from).unwrap_or(self.cfg.projects.library_max_days),
            self.cfg.projects.library_max_mb.saturating_mul(1024 * 1024),
            now,
        )
    }

    /// 웹 「보고서 정리」 실행(관리 GUI W4, plan §8.343): 미리보기와 같은 판정(기한·용량)으로 다시 셈해, 후보가 사람이 본 목록
    /// (`expected`, `project/name`)과 같을 때만 지운다. 그 사이 사본이 바뀌었으면 하나도 지우지 않고 `conflict`다.
    pub(crate) fn library_prune_apply(
        &self,
        project: Option<&str>,
        days: Option<u32>,
        expected: &[String],
        now: u64,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let _lock = self.library_ops.lock().unwrap();
        let plan = self.retention_plan(
            project,
            days.map(u64::from).unwrap_or(self.cfg.projects.library_max_days),
            self.cfg.projects.library_max_mb.saturating_mul(1024 * 1024),
            now,
        )?;
        let mut found: Vec<String> = plan.candidates.iter().map(|c| format!("{}/{}", c.project, c.name)).collect();
        let mut seen = expected.to_vec();
        found.sort();
        seen.sort();
        if found != seen {
            return Err(err(
                ErrorCode::Conflict,
                format!("미리 본 뒤 정리 후보가 바뀌었다(지금 {}개): 다시 미리 보고 지운다", found.len()),
            ));
        }
        self.remove_copies(&plan, actor).map(|removed| FrontResponse::LibraryPruned { removed })
    }

    pub(crate) fn library_usage(&self) -> Result<FrontResponse, ErrorBody> {
        let _lock = self.library_ops.lock().unwrap();
        let reports = self.library_reports(None, false)?;
        let inventory = self.library_inventory()?;
        Ok(FrontResponse::LibraryUsage {
            size: inventory.iter().filter(|r| !r.archived).map(|r| r.size).sum(),
            archived_size: inventory.iter().filter(|r| r.archived).map(|r| r.size).sum(),
            reports: reports.iter().filter(|r| !r.archived).count() as u32,
            live: reports.iter().filter(|r| r.live).count() as u32,
            problems: reports.iter().filter(|r| r.problem.is_some()).count() as u32,
            max_size: self.cfg.projects.library_max_mb.saturating_mul(1024 * 1024),
            max_days: self.cfg.projects.library_max_days,
            protected_size: inventory.iter().filter(|r| !r.archived && r.protected).map(|r| r.size).sum(),
        })
    }

    fn remove_copies(&self, plan: &LibraryPrunePlan, actor: &str) -> Result<Vec<String>, ErrorBody> {
        let mut removed = Vec::new();
        let mut bytes = 0u64;
        // 도중에 실패해도 이미 지운 수·크기를 한 줄로 남긴다.
        let result = (|| {
            for r in &plan.candidates {
                // 정리는 개별 물리 사본 단위다. 보관한 r 판과 별칭의 다른 판을 함께 지우지 않는다.
                let version = self.db.report_target(&r.project, &r.name).map_err(crate::orch::internal)?.is_some();
                self.remove_report_copy(&r.project, &r.name, version)?;
                if version {
                    self.db.report_deleted(&r.project, &r.name).map_err(crate::orch::internal)?;
                }
                self.db.audit(actor, "library_prune", &format!("{}/{}", r.project, r.name));
                bytes = bytes.saturating_add(r.size);
                removed.push(format!("{}/{}", r.project, r.name));
            }
            Ok(())
        })();
        tracing::info!("보고서 사본 정리: {}개 · {}B", removed.len(), bytes);
        if !removed.is_empty() {
            self.reports_bump();
        }
        result?;
        Ok(removed)
    }

    /// 보고서 하나의 host 사본을 지운다. 정리(prune)는 report.md 사본·thread 기록을 남기고(§8.212), 사용자 지우기는
    /// `with_report`로 thread 보고 사본(`threads/<id>.md`)까지 지운다(§8.256). 실시간 등록 원본은 건드리지 않는다.
    pub(crate) fn remove_report(
        &self,
        project: &str,
        name: &str,
        with_report: bool,
        actor: &str,
        action: &str,
    ) -> Result<(), ErrorBody> {
        let versions = self.report_version_names(project, name);
        for id in versions {
            self.remove_report_copy(project, &id, true)?;
            self.db.report_deleted(project, &id).map_err(crate::orch::internal)?;
        }
        self.remove_report_copy(project, name, with_report)?;
        self.db.audit(actor, action, &format!("{project}/{name}"));
        Ok(())
    }

    fn remove_report_copy(&self, project: &str, name: &str, with_report: bool) -> Result<(), ErrorBody> {
        let dir = self.project_dir(project)?;
        let dest = dir.join("library").join(name);
        // 먼저 링크를 끝낸다. 파일 삭제 실패로 반쯤 남아도 공유 링크로 읽지 못한다.
        self.db.library_report_remove(project, name).map_err(crate::orch::internal)?;
        self.view_caps.lock().unwrap().retain(|_, c| c.project != project || c.thread != name);
        if dest.exists() {
            let remove = if dest.is_dir() { std::fs::remove_dir_all(&dest) } else { std::fs::remove_file(&dest) };
            remove.map_err(|e| {
                aaa_common::diagnostic::context(
                    e,
                    err(ErrorCode::Unavailable, format!("보고서 {project}/{name} 사본 삭제 실패")),
                )
            })?;
        }
        let manifest = manifest_path(&dir, name);
        if manifest.exists() {
            std::fs::remove_file(manifest).map_err(|e| {
                aaa_common::diagnostic::context(
                    e,
                    err(ErrorCode::Unavailable, format!("보고서 {project}/{name} 목록 삭제 실패")),
                )
            })?;
        }
        let report = dir.join("threads").join(format!("{name}.md"));
        if with_report && (name.starts_with("t-") || crate::report_ids::number(name).is_some()) && report.exists() {
            std::fs::remove_file(report).map_err(|e| {
                aaa_common::diagnostic::context(
                    e,
                    err(ErrorCode::Unavailable, format!("보고서 {project}/{name} 본문 삭제 실패")),
                )
            })?;
        }
        Ok(())
    }

    pub(crate) fn library_prune_at(
        &self,
        project: Option<&str>,
        days: u32,
        now: u64,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if days == 0 {
            return Err(err(ErrorCode::BadRequest, "정리 기준은 1일 이상이다"));
        }
        let _lock = self.library_ops.lock().unwrap();
        // 기존 수동 명령은 명시한 기한만 쓴다.
        let plan = self.retention_plan(project, u64::from(days), 0, now)?;
        self.remove_copies(&plan, actor).map(|removed| FrontResponse::LibraryPruned { removed })
    }

    pub(crate) fn library_retention_pass_at(&self, now: u64) -> Result<FrontResponse, ErrorBody> {
        let _lock = self.library_ops.lock().unwrap();
        let days = self.cfg.projects.library_max_days;
        let limit = self.cfg.projects.library_max_mb.saturating_mul(1024 * 1024);
        let skipped = || FrontResponse::LibraryPruned { removed: Vec::new() };
        if days == 0 && limit == 0 {
            return Ok(skipped());
        }
        if self
            .db
            .meta(LAST_PASS)
            .map_err(crate::orch::internal)?
            .and_then(|n| {
                n.parse::<u64>()
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .is_some_and(|last| now >= last && now - last < EVERY_MS)
        {
            return Ok(skipped());
        }
        // 실패도 다음 정해진 회차에 확인한다. tick 사건마다 같은 실패를 재시도하지 않는다.
        self.db.set_meta(LAST_PASS, &now.to_string()).map_err(crate::orch::internal)?;
        let plan = self.retention_plan(None, days, limit, now)?;
        if limit > 0 && plan.protected_size > limit {
            if self.db.meta(OVERFLOW).map_err(crate::orch::internal)?.is_none() {
                let mut projects = self
                    .library_reports(None, false)?
                    .into_iter()
                    .filter(|r| !r.live && self.protected_copy(r))
                    .map(|r| r.project)
                    .collect::<Vec<_>>();
                projects.sort();
                projects.dedup();
                for project in projects {
                    self.inbox_add(
                        &project,
                        "library-retention",
                        "host",
                        "protected-overflow",
                        &format!(
                            "보고서 보호 사본 {}B만으로 합계 상한 {}B를 넘었다. 자동 삭제하지 않는다",
                            plan.protected_size, limit
                        ),
                    );
                }
                self.db.set_meta(OVERFLOW, "1").map_err(crate::orch::internal)?;
            }
        } else {
            self.db.del_meta(OVERFLOW).map_err(crate::orch::internal)?;
        }
        self.remove_copies(&plan, "host").map(|removed| FrontResponse::LibraryPruned { removed })
    }
}
