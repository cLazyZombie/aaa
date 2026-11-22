//! 할 일 변경 기록.

use super::*;

pub(super) fn task_log_row(r: &rusqlite::Row) -> rusqlite::Result<aaa_proto::TaskLogEntry> {
    let action: String = r.get(3)?;
    Ok(aaa_proto::TaskLogEntry {
        id: r.get(0)?,
        at_ms: r.get::<_, i64>(1)? as u64,
        actor: r.get(2)?,
        by: r.get(2)?,
        action: aaa_proto::TaskAction::parse(&action).unwrap_or(aaa_proto::TaskAction::Raw),
        list: r.get(4)?,
        from_list: r.get(5)?,
        title: r.get(6)?,
        before: r.get(7)?,
        after: r.get(8)?,
        thread: r.get(9)?,
        restored_by: r.get(10)?,
    })
}

impl Db {
    /// 할 일 기록 한 줄(할 일 편집 T2, plan §8.311). 새 id
    pub(crate) fn task_log_add(
        &self,
        project: &str,
        at_ms: u64,
        actor: &str,
        r: &crate::task_edit::Record,
    ) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO task_log(project,at_ms,actor,action,list,from_list,title,before,after,thread)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                project,
                at_ms as i64,
                actor,
                r.action.as_str(),
                r.list,
                r.from_list,
                r.title,
                r.before,
                r.after,
                r.thread
            ],
        )?;
        let id = c.last_insert_rowid();
        if let Some(gone) = r.restored {
            c.execute("UPDATE task_log SET restored_by=?1 WHERE project=?2 AND id=?3", params![id, project, gone])?;
        }
        Ok(id)
    }

    /// 할 일 기록(최신부터, `before` id 앞). `by`는 actor 원문이다(host가 보이는 이름으로 바꾼다)
    pub(crate) fn task_log_list(
        &self,
        project: &str,
        before: Option<i64>,
        limit: u32,
        filter: Option<aaa_proto::TaskLogFilter>,
    ) -> Result<Vec<aaa_proto::TaskLogEntry>> {
        let only = match filter {
            None => "",
            Some(aaa_proto::TaskLogFilter::Deleted) => " AND action IN ('delete','auto_clean')",
            Some(aaa_proto::TaskLogFilter::Auto) => " AND action IN ('auto_clean','completed')",
            Some(aaa_proto::TaskLogFilter::Restored) => " AND (action='restore' OR restored_by IS NOT NULL)",
        };
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(&format!(
            "SELECT id,at_ms,actor,action,list,from_list,title,before,after,thread,restored_by FROM task_log
             WHERE project=?1 AND (?2 IS NULL OR id<?2){only} ORDER BY id DESC LIMIT ?3"
        ))?;
        let rows = st.query_map(params![project, before, limit], task_log_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn task_log_get(&self, project: &str, id: i64) -> Result<Option<aaa_proto::TaskLogEntry>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,at_ms,actor,action,list,from_list,title,before,after,thread,restored_by FROM task_log
             WHERE project=?1 AND id=?2",
            params![project, id],
            task_log_row,
        )
        .optional()?)
    }

    /// 이 시각보다 오래된 할 일 기록을 지운다. 지운 수
    pub(crate) fn task_log_prune(&self, project: &str, before_ms: u64) -> Result<usize> {
        let c = self.conn.lock().unwrap();
        Ok(c.execute("DELETE FROM task_log WHERE project=?1 AND at_ms<?2", params![project, before_ms as i64])?)
    }
}
