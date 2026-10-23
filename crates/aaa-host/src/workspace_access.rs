//! 머신별 자동 배정 허용. 경로를 복제하지 않고 프로젝트 설정의 workspace 이름만 참조한다.
//! 최초 host 시작에 기존 프로젝트×머신×workspace를 한 transaction으로 켠다. 이후 새 조합은 꺼짐이다.
use std::collections::{BTreeMap, BTreeSet};

use crate::{Host, err, orch::internal, tasks::REPO_DIR};
use aaa_proto::{ErrorBody, ErrorCode, WorkspaceAccessRow, WorkspaceAccessView};

type Access = BTreeMap<String, BTreeSet<String>>;
const MIGRATED: &str = "workspace-access-migrated-v1";
fn key(project: &str) -> String {
    format!("workspace-access:{project}")
}

impl Host {
    pub(crate) fn migrate_workspace_access(&self) -> anyhow::Result<()> {
        if self.db.meta(MIGRATED)?.is_some() {
            return Ok(());
        }
        let machines = self.db.machines()?;
        let mut records = Vec::new();
        for (project, _) in self.project_directories() {
            let mut names: BTreeSet<String> =
                crate::task_repo::repo_names(&self.settings(&project).repos).into_iter().collect();
            names.insert(REPO_DIR.into());
            let access: Access = machines.iter().map(|m| (m.id.clone(), names.clone())).collect();
            records.push((key(&project), serde_json::to_string(&access)?));
        }
        // marker와 모든 기존 허용을 함께 확정한다. 재시작 때 사용자가 끈 것을 다시 켜지 않는다.
        self.db.update_meta_checked(MIGRATED, &[MIGRATED], |_| Ok(("1".into(), records)))?;
        Ok(())
    }

    fn workspace_access_record(&self, project: &str) -> Result<(String, Access), ErrorBody> {
        let text = self.db.meta(&key(project)).map_err(internal)?.unwrap_or_else(|| "{}".into());
        let access = serde_json::from_str(&text).map_err(internal)?;
        Ok((crate::tasks::sha256(&text), access))
    }

    pub(crate) fn workspace_allowed(&self, project: &str, workspace: &str, machine: &str) -> Result<bool, ErrorBody> {
        let mid = self.resolve_machine(Some(machine))?;
        let (_, access) = self.workspace_access_record(project)?;
        Ok(access.get(&mid).is_some_and(|names| names.contains(workspace)))
    }

    pub(crate) fn check_automatic_workspace(&self, a: &crate::orch::StartArgs, mid: &str) -> Result<(), ErrorBody> {
        let Some(workspace) = &a.automatic_workspace else {
            return Ok(());
        };
        if !self.workspace_allowed(&a.project, workspace, mid)? {
            return Err(err(ErrorCode::Unavailable, format!("start_wait: {workspace} 자동 배정 허용이 꺼졌다")));
        }
        if workspace != REPO_DIR {
            let view = self.workspace_access_view(&a.project, mid)?;
            if !view
                .workspaces
                .iter()
                .any(|r| &r.workspace == workspace && a.repo.as_ref().is_some_and(|p| r.paths.contains(p)))
            {
                return Err(err(ErrorCode::Unavailable, "start_wait: 자동 배정한 workspace 경로가 바뀌었다"));
            }
        }
        Ok(())
    }

    pub(crate) fn workspace_access_preview(&self, project: &str) -> Result<BTreeMap<String, Vec<String>>, ErrorBody> {
        let (_, access) = self.workspace_access_record(project)?;
        Ok(self
            .machine_views()
            .into_iter()
            .map(|m| {
                let names = access.get(&m.id).map(|s| s.iter().cloned().collect()).unwrap_or_default();
                (m.name, names)
            })
            .collect())
    }

    pub(crate) fn workspace_access_view(&self, project: &str, machine: &str) -> Result<WorkspaceAccessView, ErrorBody> {
        self.project_dir(project)?;
        let mid = self.resolve_machine(Some(machine))?;
        let (revision, access) = self.workspace_access_record(project)?;
        let repos = self.settings(project).repos;
        let local = self.machine_views().into_iter().find(|m| m.local).map(|m| m.name);
        let name = self.machine_name(&mid);
        let mut names = crate::task_repo::repo_names(&repos);
        names.push(REPO_DIR.into());
        let workspaces = names
            .into_iter()
            .map(|workspace| {
                let paths = repos
                    .iter()
                    .filter(|r| {
                        crate::task_repo::repo_name(&r.path) == Some(&workspace)
                            && r.machine.as_ref().or(local.as_ref()) == Some(&name)
                    })
                    .map(|r| r.path.clone())
                    .collect();
                let enabled = access.get(&mid).is_some_and(|s| s.contains(&workspace));
                WorkspaceAccessRow { workspace, paths, enabled }
            })
            .collect();
        Ok(WorkspaceAccessView { project: project.into(), machine: mid, revision, workspaces })
    }

    pub(crate) fn workspace_access_set(
        &self,
        project: &str,
        machine: &str,
        workspace: &str,
        enabled: bool,
        revision: &str,
        actor: &str,
    ) -> Result<WorkspaceAccessView, ErrorBody> {
        let view = self.workspace_access_view(project, machine)?;
        if !view.workspaces.iter().any(|r| r.workspace == workspace) {
            return Err(err(ErrorCode::BadRequest, "프로젝트에 없는 workspace다"));
        }
        let mut conflict = false;
        self.db
            .update_meta(&key(project), |old| {
                let old = old.unwrap_or_else(|| "{}".into());
                if crate::tasks::sha256(&old) != revision {
                    conflict = true;
                    anyhow::bail!("workspace 허용 설정이 바뀌었다");
                }
                let mut access: Access = serde_json::from_str(&old)?;
                let names = access.entry(view.machine.clone()).or_default();
                if enabled {
                    names.insert(workspace.into());
                } else {
                    names.remove(workspace);
                }
                Ok(serde_json::to_string(&access)?)
            })
            .map_err(|e| {
                if conflict {
                    err(ErrorCode::Conflict, "workspace 허용 설정이 바뀌었다. 다시 읽고 고친다")
                } else {
                    internal(e)
                }
            })?;
        self.db.audit(actor, "workspace_access", &format!("{project} {} {workspace}={enabled}", view.machine));
        // 큐와 열린 설정 화면이 현재 허용을 다시 읽는다. 이미 실행 중인 thread에는 영향이 없다.
        let _ = self.task_events.send((project.into(), "workspace-access".into()));
        self.workspace_access_view(project, machine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::{DeviceRole, FrontRequest, FrontResponse, QueueOp, QueueState, Role, ThreadKind};
    use std::{path::PathBuf, sync::Arc};

    struct Lab {
        root: PathBuf,
        h: Arc<Host>,
    }
    impl Lab {
        fn new(legacy: bool) -> Self {
            let root = std::env::temp_dir().join(format!("aaa-access-{}", aaa_common::util::new_id()));
            let mut cfg = aaa_config::HostConfig::default();
            cfg.projects.root = root.join("projects").display().to_string();
            cfg.notify.web_push = false;
            let h = Host::open(cfg.clone(), root.join("host")).unwrap();
            h.project_create("demo", Default::default(), "local:cli").unwrap();
            for (id, name) in [("m-a", "mac-a"), ("m-b", "mac-b")] {
                h.db.upsert_machine(id, name, None, id == "m-a").unwrap();
                h.project_set("demo", "repos.add", &format!("/work/{name}/app@{name}"), "local:cli").unwrap();
            }
            if legacy {
                h.db.del_meta(MIGRATED).unwrap();
            }
            drop(h);
            let h = Host::open(cfg, root.join("host")).unwrap();
            Self { root, h }
        }
        fn set(&self, m: &str, ws: &str, on: bool) {
            let rev = self.h.workspace_access_view("demo", m).unwrap().revision;
            self.h.workspace_access_set("demo", m, ws, on, &rev, "local:cli").unwrap();
        }
        fn args(&self, m: Option<&str>) -> crate::orch::StartArgs {
            crate::orch::StartArgs {
                automatic_workspace: None,
                start_task: None,
                pool: None,
                over_limit: None,
                reservation: None,
                account: None,
                holds: vec![],
                project: "demo".into(),
                title: "작업".into(),
                machine: m.map(Into::into),
                profile: "claude".into(),
                kind: ThreadKind::Dir,
                repo: None,
                base: None,
                task: "작업".into(),
                role: Role::Worker,
                attachments: vec![],
            }
        }
        fn connect(&self) {
            for m in self.h.inner.lock().unwrap().machines.values_mut() {
                m.conn = Some(crate::AgentConn::new(aaa_common::conn::frame_channel().0, 1));
            }
        }
    }
    impl Drop for Lab {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn migration_preserves_existing_assignments_once_and_new_combinations_stay_off() {
        let lab = Lab::new(true);
        for m in ["mac-a", "mac-b"] {
            let v = lab.h.workspace_access_view("demo", m).unwrap();
            assert!(v.workspaces.iter().all(|r| r.enabled));
            assert_eq!(v.workspaces[0].paths.len(), 1);
        }
        lab.set("mac-a", "app", false);
        lab.h.migrate_workspace_access().unwrap();
        assert!(!lab.h.workspace_allowed("demo", "app", "mac-a").unwrap());
        let h = Host::open(lab.h.cfg.clone(), lab.root.join("host")).unwrap();
        assert!(!h.workspace_allowed("demo", "app", "mac-a").unwrap(), "재시작은 꺼짐을 보존한다");
        assert!(h.workspace_allowed("demo", "app", "mac-b").unwrap());
        h.project_set("demo", "repos.add", "/work/new@mac-a", "local:cli").unwrap();
        assert!(!h.workspace_allowed("demo", "new", "mac-a").unwrap());
        h.db.upsert_machine("m-new", "mac-new", None, false).unwrap();
        let newer = Host::open(h.cfg.clone(), lab.root.join("host")).unwrap();
        assert!(!newer.workspace_allowed("demo", "app", "mac-new").unwrap());
        assert!(!newer.workspace_allowed("demo", REPO_DIR, "mac-new").unwrap());
    }

    #[tokio::test]
    async fn owner_only_changes_conflicts_and_reader_view() {
        let lab = Lab::new(false);
        let v = lab.h.workspace_access_view("demo", "mac-a").unwrap();
        assert!(v.workspaces.iter().all(|r| !r.enabled), "새 프로젝트의 기본은 꺼짐");
        let req = FrontRequest::WorkspaceAccessSet {
            project: "demo".into(),
            machine: "mac-a".into(),
            workspace: "app".into(),
            enabled: true,
            revision: v.revision,
        };
        for (role, actor) in [
            (DeviceRole::Reader, "device:reader"),
            (DeviceRole::Local, "worker:p"),
            (DeviceRole::Local, "local:cli@coordinator:p"),
        ] {
            assert_eq!(lab.h.call(role, actor, req.clone()).await.unwrap_err().code, ErrorCode::Forbidden);
        }
        let read = FrontRequest::WorkspaceAccess { project: "demo".into(), machine: "mac-a".into() };
        assert!(matches!(
            lab.h.call(DeviceRole::Reader, "device:reader", read).await.unwrap(),
            FrontResponse::WorkspaceAccess(_)
        ));
        lab.h.call(DeviceRole::Controller, "device:owner", req.clone()).await.unwrap();
        assert_eq!(lab.h.call(DeviceRole::Local, "local:cli", req).await.unwrap_err().code, ErrorCode::Conflict);
        let rev = lab.h.workspace_access_view("demo", "mac-a").unwrap().revision;
        assert_eq!(
            lab.h.workspace_access_set("demo", "mac-a", "unknown", true, &rev, "local:cli").unwrap_err().code,
            ErrorCode::BadRequest
        );
    }

    #[tokio::test]
    async fn automatic_selection_filters_repo_and_dir_but_explicit_machine_warns() {
        let lab = Lab::new(true);
        lab.connect();
        lab.set("mac-a", "app", false);
        let mut a = lab.args(None);
        lab.h.apply_task_repo(&mut a, Some("app"), None).unwrap();
        assert_eq!(a.machine.as_deref(), Some("mac-b"));
        assert_eq!(a.repo.as_deref(), Some("/work/mac-b/app"));
        assert_eq!(a.automatic_workspace.as_deref(), Some("app"));
        lab.h.check_automatic_workspace(&a, "m-b").unwrap();
        lab.h.project_set("demo", "repos.remove", "/work/mac-b/app@mac-b", "local:cli").unwrap();
        assert!(lab.h.check_automatic_workspace(&a, "m-b").unwrap_err().message.contains("경로가 바뀌었다"));
        lab.h.project_set("demo", "repos.add", "/work/mac-b/app@mac-b", "local:cli").unwrap();
        lab.set("mac-b", "app", false);
        assert_eq!(
            lab.h.check_automatic_workspace(&a, "m-b").unwrap_err().code,
            ErrorCode::Unavailable,
            "계정 대기 뒤 기동도 현재 허용을 검사한다"
        );
        assert_eq!(
            lab.h.apply_task_repo(&mut lab.args(None), Some("app"), None).unwrap_err().code,
            ErrorCode::Unavailable
        );
        let mut fixed = lab.args(Some("mac-a"));
        assert!(lab.h.apply_task_repo(&mut fixed, Some("app"), None).unwrap().unwrap().contains("명시한 머신"));
        lab.set("mac-a", REPO_DIR, false);
        let mut dir = lab.args(None);
        lab.h.apply_task_repo(&mut dir, None, None).unwrap();
        assert_eq!(dir.machine.as_deref(), Some("mac-b"));
        lab.set("mac-b", REPO_DIR, false);
        assert!(lab.h.apply_task_repo(&mut lab.args(None), None, None).unwrap_err().message.contains("자리 대기"));
    }

    #[tokio::test]
    async fn queue_keeps_unconfigured_automatic_task_waiting_with_reason() {
        let lab = Lab::new(false);
        let file = lab.h.project_dir("demo").unwrap().join("TASKS.md");
        std::fs::write(file, "## Backlog\n- [ ] 자동 작업 (claude) [작업 폴더]\n  독립 범위와 완료 조건\n").unwrap();
        let task = lab.h.task_board("demo").unwrap().tasks.remove(0);
        lab.h
            .task_queue(
                "demo",
                QueueOp::Reserve {
                    task: task.title,
                    block: task.block_sha256,
                    profile: "claude".into(),
                    profile_revision: crate::task_queue::model_selection(&lab.h, "claude").unwrap().2,
                    after: None,
                },
                "device:owner",
            )
            .await
            .unwrap();
        lab.h.queue_pass("demo").await.unwrap();
        let rows = lab.h.db.queue_list("demo").unwrap();
        assert_eq!(rows[0].state, QueueState::Queued);
        assert!(rows[0].reason.contains("자동 배정이 허용된"));
        assert!(lab.h.db.threads(Some("demo")).unwrap().is_empty());
    }
}
