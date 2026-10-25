//! 입력 별칭만 정본으로 바꾼다. 본문·프로젝트 이름·파일 경로·무작위 ID에는 적용하지 않는다.
use crate::{Host, err, orch::internal};
use aaa_proto::{ErrorBody, ErrorCode, FrontRequest as R, canonical_short_id};

fn number(value: &mut String, prefix: char) {
    if let Some(id) = canonical_short_id(value, prefix) {
        *value = id;
    }
}

impl Host {
    fn thread_selector(&self, project: &str, value: &mut String) -> Result<(), ErrorBody> {
        let Some(id) = canonical_short_id(value, 't') else { return Ok(()) };
        if id != *value && self.db.thread_get(project, value).map_err(internal)?.is_some() {
            if self.db.thread_get(project, &id).map_err(internal)?.is_some() {
                return Err(err(ErrorCode::BadRequest, "저장된 thread ID와 번호 별칭이 겹친다. 정본 번호를 지정한다"));
            }
        } else {
            *value = id;
        }
        Ok(())
    }

    fn report_selector(&self, project: &str, value: &mut String) -> Result<(), ErrorBody> {
        if canonical_short_id(value, 't').is_some() {
            return self.thread_selector(project, value);
        }
        let Some(id) = canonical_short_id(value, 'r').or_else(|| canonical_short_id(value, 't')) else {
            return Ok(());
        };
        if id != *value && self.db.library_report(project, value).map_err(internal)?.is_some() {
            // 이관한 보고서의 실제 이름을 다른 번호로 바꾸지 않는다.
            if self.db.library_report(project, &id).map_err(internal)?.is_some() {
                return Err(err(ErrorCode::BadRequest, "보고서 이름과 번호 별칭이 겹친다. 정본 번호를 지정한다"));
            }
        } else {
            *value = id;
        }
        Ok(())
    }

    /// 권한 판정·잠금·파일 접근이 모두 같은 정본을 사용한다.
    pub(crate) fn normalize_short_ids(&self, req: &mut R) -> Result<(), ErrorBody> {
        match req {
            R::ThreadShow { project, thread }
            | R::ThreadLibrary { project, thread }
            | R::LibraryLink { project, thread, .. } => self.report_selector(project, thread)?,
            R::LibraryOpen { project, name } => self.report_selector(project, name)?,
            R::LibraryArchive { reports, .. } | R::LibraryDelete { reports } | R::LibraryMarkRead { reports } => {
                for r in reports {
                    self.report_selector(&r.project, &mut r.name)?;
                }
            }
            R::ThreadNext { project, thread, .. }
            | R::ThreadFile { project, thread, .. }
            | R::ThreadDiff { project, thread, .. }
            | R::ThreadSummary { project, thread, .. }
            | R::ThreadSummarySet { project, thread, .. }
            | R::ThreadAck { project, thread, .. }
            | R::ThreadResolve { project, thread, .. }
            | R::ThreadResolvePreview { project, thread, .. }
            | R::ThreadPark { project, thread, .. }
            | R::ThreadUnpark { project, thread, .. }
            | R::ThreadStop { project, thread, .. }
            | R::ThreadRestart { project, thread, .. }
            | R::ThreadBrief { project, thread, .. }
            | R::ThreadRead { project, thread, .. }
            | R::ThreadKeys { project, thread, .. }
            | R::ThreadMessage { project, thread, .. }
            | R::ThreadAnswer { project, thread, .. }
            | R::PostUnmute { project, thread, .. } => self.thread_selector(project, thread)?,
            R::LibraryPublish { from_thread: Some(thread), .. } => number(thread, 't'),
            R::DebugInput { thread: Some(thread), .. } => {
                if let Some((project, id)) = thread.split_once('/') {
                    if let Some(id) = canonical_short_id(id, 't') {
                        *thread = format!("{project}/{id}");
                    }
                } else {
                    number(thread, 't');
                }
            }
            R::RoutineRuns { project, name }
            | R::RoutineApprove { project, name, .. }
            | R::RoutineToggle { project, name, .. }
            | R::AutomationRead { project, name }
            | R::AutomationExecutions { project, name, .. }
            | R::AutomationApprove { project, name, .. }
            | R::AutomationUnblock { project, name, .. }
            | R::AutomationDelete { project, name, .. }
            | R::AutomationRun { project, name, .. }
            | R::AutomationSave { project, name, expected_sha256: Some(_), .. } => {
                if let Some(id) = aaa_proto::short_id_number(name, 'a')
                    && let Some(resolved) = self.db.automation_long_id(project, id).map_err(internal)?
                {
                    if resolved != *name && self.routines(project)?.iter().any(|r| r.name == *name) {
                        return Err(err(ErrorCode::BadRequest, "자동화 이름과 번호 별칭이 겹친다. 이름을 지정한다"));
                    }
                    *name = resolved;
                }
            }
            // 새 자동화의 이름·본문과 task 선택자는 여기서 바꾸지 않는다.
            // task는 기존 task_selector가 프로젝트의 번호 원장으로 해석한다.
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{DeviceRole, FrontResponse, Role, ThreadKind, ThreadStatus};

    #[tokio::test]
    async fn thread_aliases_preserve_authorization_project_and_request_body() {
        let root = std::env::temp_dir().join(format!("aaa-id-select-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        let h = Host::open(cfg, root.join("state")).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        let mut rec =
            crate::orch::ThreadRec::starting("demo", "번호", "mac-a", "fake", Role::Worker, ThreadKind::Dir, 1);
        rec.id = "t-0965".into();
        rec.status = ThreadStatus::Resolved;
        h.db.thread_import("demo", &rec.id, "resolved", &serde_json::to_string(&rec).unwrap()).unwrap();
        for alias in ["t-965", "t-0965", "t-00965"] {
            let r = h
                .call(DeviceRole::Reader, "device:r", R::ThreadShow { project: "demo".into(), thread: alias.into() })
                .await
                .unwrap();
            assert!(matches!(r, FrontResponse::Thread(t) if t.id == rec.id));
            assert!(
                h.call(DeviceRole::Reader, "device:r", R::ThreadAck { project: "demo".into(), thread: alias.into() })
                    .await
                    .is_err()
            );
            assert!(
                h.call(DeviceRole::Reader, "device:r", R::ThreadShow { project: "other".into(), thread: alias.into() })
                    .await
                    .is_err()
            );
        }
        let mut req =
            R::ThreadFile { project: "demo".into(), thread: "t-00965".into(), path: "library/t-00965.md".into() };
        h.normalize_short_ids(&mut req).unwrap();
        assert!(
            matches!(req, R::ThreadFile { project, thread, path } if project == "demo" && thread == "t-0965" && path == "library/t-00965.md")
        );
        // 과거에 잘못 저장한 별도 ID가 있더라도 별칭으로 다른 기록을 고르지 않는다.
        rec.id = "t-00965".into();
        h.db.thread_import("demo", &rec.id, "resolved", &serde_json::to_string(&rec).unwrap()).unwrap();
        let mut req = R::ThreadSummary { project: "demo".into(), thread: rec.id.clone() };
        assert!(h.normalize_short_ids(&mut req).is_err());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn automation_aliases_cannot_bypass_system_rule_protection_or_shadow_names() {
        let root = std::env::temp_dir().join(format!("aaa-auto-id-select-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        let h = Host::open(cfg, root.join("state")).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        let dir = h.project_dir("demo").unwrap().join("automations");
        let text = "[on.schedule]\nevery = \"1d\"\n[do.inbox]\ntext = \"확인\"\n";
        std::fs::write(dir.join("idle-audit.toml"), text).unwrap();
        let id = h.db.automation_short_id("demo", "idle-audit").unwrap();
        let alias = format!("a-0{}", &id[2..]);
        let error = h
            .call(
                DeviceRole::Controller,
                "device:c",
                R::RoutineToggle { project: "demo".into(), name: alias.clone(), on: Some(false) },
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Forbidden);
        std::fs::write(dir.join(format!("{alias}.toml")), text).unwrap();
        let mut request = R::AutomationRead { project: "demo".into(), name: alias };
        assert!(h.normalize_short_ids(&mut request).is_err());
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }
}
