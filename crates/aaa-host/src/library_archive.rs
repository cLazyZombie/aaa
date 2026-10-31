//! 보관함과 완료 보고는 프로젝트 설정 없이 켜진 제품 기본 동작이다.
use crate::{Host, err, orch::internal};
use aaa_proto::{ErrorBody, ErrorCode, FrontResponse, LibraryRef};
use sha2::{Digest, Sha256};

impl Host {
    pub(crate) fn library_mark_read(
        &self,
        reports: &[LibraryRef],
        actor: &str,
        reveal: bool,
    ) -> Result<FrontResponse, ErrorBody> {
        {
            let _lock = self.library_ops.lock().unwrap();
            let mut targets = std::collections::HashSet::new();
            // 쓰기 전에 전부 검증한다. 확인 창을 연 뒤 생긴 새 보고서는 요청 목록에 없다.
            for r in reports {
                self.project_dir(&r.project)?;
                if !crate::library::safe_rel(&r.name) || r.name.contains('/') {
                    return Err(err(ErrorCode::BadRequest, "읽음 처리할 보고서 이름 형식이 아니다"));
                }
                targets.insert((r.project.clone(), r.name.clone()));
                if let Some(v) = self.db.report_target(&r.project, &r.name).map_err(internal)?
                    && self.report_id(&r.project, &v.source).as_deref() == Some(r.name.as_str())
                {
                    // 보관 판도 기존 열람과 같이 최신 판일 때에만 원본을 읽음 처리한다.
                    targets.insert((r.project.clone(), v.source));
                }
            }
            let targets: Vec<_> = targets.into_iter().collect();
            let changed = self.db.library_mark_read(&targets, aaa_common::util::now_ms()).map_err(internal)?;
            self.db.audit(actor, "library_mark_read", &format!("requested={} changed={changed}", reports.len()));
            if changed > 0 {
                self.reports_bump();
            }
        }
        Ok(FrontResponse::LibraryReports { reports: self.library_reports(None, reveal)? })
    }

    pub(crate) fn library_archive(
        &self,
        reports: &[LibraryRef],
        archived: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if reports.is_empty() || reports.len() > 500 {
            return Err(err(ErrorCode::BadRequest, "보관할 보고서는 1~500개다"));
        }
        let _lock = self.library_ops.lock().unwrap();
        // 먼저 전부 검증한다. 이름은 현재 불변 판으로 고정하며 이후 publish를 따라가지 않는다.
        let mut versions = Vec::new();
        for r in reports {
            self.project_dir(&r.project)?;
            if !crate::library::safe_rel(&r.name) || r.name.contains('/') {
                return Err(err(ErrorCode::BadRequest, "보관할 보고서 이름 형식이 아니다"));
            }
            let id = self
                .db
                .report_target(&r.project, &r.name)
                .map_err(internal)?
                .map(|v| v.id)
                .or(self.db.report_ids(&r.project, &r.name).map_err(internal)?.into_iter().next().map(|v| v.id))
                .ok_or_else(|| err(ErrorCode::NotFound, format!("보관할 보고 판이 없다: {}/{}", r.project, r.name)))?;
            versions.push((r.project.clone(), id));
        }
        self.db.library_archive_set(&versions, archived).map_err(internal)?;
        for (project, id) in versions {
            self.db.audit(
                actor,
                if archived { "library_archive" } else { "library_unarchive" },
                &format!("{project}/{id}"),
            );
        }
        self.reports_bump();
        Ok(FrontResponse::LibraryReports { reports: self.library_reports(None, true)? })
    }

    pub(crate) fn complete_thread_report(
        &self,
        project: &str,
        id: &str,
        body: &str,
        revision: &str,
    ) -> Result<(), ErrorBody> {
        let rec = self.load(project, id)?;
        // thread 종료는 보고의 사용자 확인 필요 여부를 바꾸지 않는다.
        if !crate::report_mark::done(crate::report_mark::marker(body)) && !crate::report_mark::needs_user(body) {
            // 옛 host가 resolve만 보고 올린 보고와, 완료 뒤 다시 진행 중이 된 보고를 작업 보고로 돌린다.
            if self.db.library_uncomplete(project, id).map_err(internal)? {
                self.reports_bump();
            }
            return Ok(());
        }
        // 자동화 thread는 새 결함(`[알림]`)이나 질문 카드가 있을 때만 올린다. 나머지는 작업 보고에 남아 자동 정리된다.
        if rec.automation.is_some() && !crate::report_mark::notable(body) {
            tracing::debug!(project, thread = id, "자동화 보고를 작업 보고에 남겼다");
            return Ok(());
        }
        // 불완전한 판은 올리지 않는다. 다음 정상 회수 때 같은 본문으로도 올릴 수 있다.
        if self.thread_report_id(project, id, Some(revision)).is_none() {
            return Ok(());
        }
        if self.db.library_complete(project, id, revision, &rec.title, aaa_common::util::now_ms()).map_err(internal)? {
            tracing::info!(project, thread = id, "사용자 확인할 보고를 내 보고서에 올렸다");
            self.reports_bump();
        }
        Ok(())
    }

    pub(crate) fn backfill_completed_reports(&self) -> Result<(), ErrorBody> {
        for (project, id) in self.db.report_sources().map_err(internal)? {
            if !aaa_proto::valid_thread(&id) || self.load(&project, &id).is_err() {
                continue;
            }
            let dir = self.project_dir(&project)?;
            let body = match std::fs::read_to_string(dir.join("threads").join(format!("{id}.md"))) {
                Ok(body) => body,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(crate::library_publish::io_error(e)),
            };
            let rec = self.load(&project, &id)?;
            // 회수 도중의 본문으로 다른 판을 등록하거나 내리지 않는다.
            if Some(hex::encode(Sha256::digest(body.as_bytes()))).as_ref()
                != rec.report_copy_hash.as_ref().or(rec.report_hash.as_ref())
            {
                continue;
            }
            if let Some(revision) = rec.report_hash {
                self.complete_thread_report(&project, &id, &body, &revision)?;
            }
        }
        Ok(())
    }
}
