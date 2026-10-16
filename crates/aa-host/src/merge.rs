//! 병합 대기열(§8.8)과 머신별 동시 thread 한도(§8.10).
//!
//! - 대기열은 순번과 흐름 사건만 둔다(`merge_enqueued`·`merge_dropped`·`merge_landed`·`merge_head`).
//!   맨 앞이 바뀌면 `merge_head` 사건을 내고, 그 thread에 무엇을 할지는 coordinator가 정한다. 단계 실행은 M3에서 한다.
//! - resolve한 thread는 대기열에서 빠진다.

use std::sync::Arc;

use aa_proto::{DeviceRole, ErrorBody, ErrorCode, MergeEntry, ThreadStatus};

use crate::orch::ThreadRec;
use crate::{Host, err};

fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}

impl Host {
    pub(crate) fn merge_queue(&self, project: &str) -> Result<Vec<MergeEntry>, ErrorBody> {
        self.project_dir(project)?;
        let rows = self.db.merge_list(project).map_err(internal)?;
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(i, (thread, at))| {
                let rec = self.load(project, &thread).ok();
                MergeEntry {
                    project: project.to_string(),
                    position: i as u32 + 1,
                    title: rec.as_ref().map(|r| r.title.clone()).unwrap_or_default(),
                    machine: rec.as_ref().map(|r| self.machine_name(&r.machine_id)).unwrap_or_default(),
                    branch: rec.and_then(|r| r.branch),
                    thread,
                    enqueued_at_ms: at,
                }
            })
            .collect())
    }

    fn merge_head(&self, project: &str) -> Option<String> {
        self.db.merge_list(project).ok()?.into_iter().next().map(|(t, _)| t)
    }

    /// 맨 앞이 바뀌었으면 사건을 낸다
    fn merge_head_changed(&self, project: &str, before: Option<String>) {
        let after = self.merge_head(project);
        if after != before
            && let Some(t) = after
            && let Ok(r) = self.load(project, &t)
        {
            self.flow(&r, "merge_head", None, "병합 대기열 맨 앞");
        }
    }

    pub(crate) fn merge_enqueue(&self, project: &str, thread: &str, actor: &str) -> Result<Vec<MergeEntry>, ErrorBody> {
        let rec = self.load(project, thread)?;
        if rec.status != ThreadStatus::Open {
            return Err(err(ErrorCode::BadRequest, format!("열린 thread만 넣는다(지금 {})", rec.status.as_str())));
        }
        let before = self.merge_head(project);
        if self.db.merge_add(project, thread).map_err(internal)? {
            let q = self.merge_queue(project)?;
            let pos = q.iter().find(|e| e.thread == thread).map(|e| e.position).unwrap_or(0);
            self.db.audit(actor, "merge_enqueue", &format!("{project}/{thread}"));
            self.flow(&rec, "merge_enqueued", None, format!("순번 {pos}"));
            self.merge_head_changed(project, before);
        }
        self.merge_queue(project)
    }

    pub(crate) fn merge_drop(
        &self,
        project: &str,
        thread: &str,
        landed: bool,
        actor: &str,
    ) -> Result<Vec<MergeEntry>, ErrorBody> {
        let rec = self.load(project, thread)?;
        let before = self.merge_head(project);
        if !self.db.merge_remove(project, thread).map_err(internal)? {
            return Err(err(ErrorCode::NotFound, format!("병합 대기열에 없다: {project}/{thread}")));
        }
        self.db.audit(actor, if landed { "merge_landed" } else { "merge_drop" }, &format!("{project}/{thread}"));
        self.flow(&rec, if landed { "merge_landed" } else { "merge_dropped" }, None, actor.to_string());
        self.merge_head_changed(project, before);
        self.merge_queue(project)
    }

    /// resolve한 thread는 대기열에서 뺀다
    pub(crate) fn merge_forget(&self, r: &ThreadRec) {
        let before = self.merge_head(&r.project);
        if self.db.merge_remove(&r.project, &r.id).unwrap_or(false) {
            self.flow(r, "merge_dropped", None, "resolve");
            self.merge_head_changed(&r.project, before);
        }
    }

    /// 머신별 동시 thread 한도. 열린 thread 중 세션이 실행 중이거나 시작 중인 것을 센다(`skip`은 빼고).
    pub(crate) fn check_capacity(self: &Arc<Self>, mid: &str, skip: Option<(&str, &str)>) -> Result<(), ErrorBody> {
        let limit = self.cfg.projects.max_threads_per_machine;
        if limit == 0 {
            return Ok(());
        }
        let busy: Vec<String> = self
            .db
            .threads(None)
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| r.machine_id == mid && skip != Some((r.project.as_str(), r.id.as_str())))
            .filter(|r| match r.status {
                ThreadStatus::Starting => true,
                ThreadStatus::Open => r
                    .pane
                    .as_deref()
                    .and_then(|p| self.pane_view_of(p, DeviceRole::Local))
                    .is_some_and(|v| v.state.is_running()),
                _ => false,
            })
            .map(|r| format!("{}/{}", r.project, r.id))
            .collect();
        if busy.len() as u32 >= limit {
            return Err(err(
                ErrorCode::Unavailable,
                format!(
                    "{}에서 이미 thread {}개가 돈다(한도 {limit}, host.toml [projects] max_threads_per_machine): {}",
                    self.machine_name(mid),
                    busy.len(),
                    busy.join(", ")
                ),
            ));
        }
        Ok(())
    }
}
