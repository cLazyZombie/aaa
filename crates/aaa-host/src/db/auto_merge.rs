//! 할 일별 자동 병합(plan §8.530). 할 일 내용의 정본은 Markdown이지만 이 값은 승인이라 host DB만 쓴다(파일 직접 편집으로 켜지 않는다).
//! 행이 없으면 off다. 끈 값도 누가·언제 껐는지 남기려고 행을 지우지 않는다.
use super::*;

impl Db {
    pub(crate) fn auto_merge(&self, project: &str, task_id: &str) -> Result<bool> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT on_ FROM task_auto_merge WHERE project=?1 AND task_id=?2",
                params![project, task_id],
                |r| r.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    /// 켜진 할 일 ULID 전부(목록 채우기용)
    pub(crate) fn auto_merge_on(&self, project: &str) -> Result<std::collections::HashSet<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT task_id FROM task_auto_merge WHERE project=?1 AND on_=1")?;
        let rows = st.query_map(params![project], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// 값을 쓰고 할 일 기록을 같은 transaction에 남긴다. 값이 같으면 아무것도 쓰지 않고 false
    pub(crate) fn auto_merge_set(
        &self,
        project: &str,
        task_id: &str,
        on: bool,
        actor: &str,
        user_request: Option<&str>,
        log: &crate::task_edit::Record,
    ) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let cur = tx
            .query_row(
                "SELECT on_ FROM task_auto_merge WHERE project=?1 AND task_id=?2",
                params![project, task_id],
                |r| r.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false);
        if cur == on {
            return Ok(false);
        }
        let now = aaa_common::util::now_ms() as i64;
        tx.execute(
            "INSERT INTO task_auto_merge(project,task_id,on_,actor,user_request,at_ms) VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(project,task_id) DO UPDATE SET on_=?3,actor=?4,user_request=?5,at_ms=?6",
            params![project, task_id, on, actor, user_request, now],
        )?;
        let log_actor = match user_request {
            Some(reason) => format!("{actor} (사용자 지시: {reason})"),
            None => actor.to_string(),
        };
        tx.execute(
            "INSERT INTO task_log(project,at_ms,actor,action,list,from_list,title,before,after,thread)
             VALUES(?1,?2,?3,?4,?5,NULL,?6,?7,?8,?9)",
            params![
                project,
                now,
                log_actor,
                log.action.as_str(),
                log.list,
                log.title,
                log.before,
                log.after,
                log.thread
            ],
        )?;
        tx.commit()?;
        Ok(true)
    }
}
