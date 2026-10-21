//! 화면 ID와 내부 ULID의 연결. 내용·충돌 hash는 Markdown이 계속 정본이다.
use crate::{Host, orch::internal};
use aaa_proto::{ErrorBody, TaskAnchor, TaskOp, TaskView};

impl Host {
    pub(crate) fn fill_task_ids(&self, project: &str, tasks: &mut [TaskView]) -> Result<(), ErrorBody> {
        for t in tasks {
            if let Some(id) = &t.id {
                t.short_id = Some(self.db.task_short_id(project, id).map_err(internal)?);
            }
        }
        Ok(())
    }

    /// 표시 ID는 제목보다 우선한다. ULID·기존 제목도 그대로 받는다.
    pub(crate) fn task_selector(&self, project: &str, value: &str) -> Result<String, ErrorBody> {
        Ok(self.db.task_long_id(project, value.trim()).map_err(internal)?.unwrap_or_else(|| value.to_string()))
    }

    pub(crate) fn task_op_ids(&self, project: &str, ops: &mut [TaskOp]) -> Result<(), ErrorBody> {
        let resolve = |a: &mut TaskAnchor| -> Result<(), ErrorBody> {
            a.title = self.task_selector(project, &a.title)?;
            Ok(())
        };
        for op in ops {
            match op {
                TaskOp::Update { at, .. } | TaskOp::Delete { at } => resolve(at)?,
                TaskOp::Move { at, before, parent, .. } => {
                    resolve(at)?;
                    for a in before.iter_mut().chain(parent.iter_mut()) {
                        resolve(a)?;
                    }
                }
                TaskOp::Add { before, parent, .. } => {
                    for a in before.iter_mut().chain(parent.iter_mut()) {
                        resolve(a)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
}
