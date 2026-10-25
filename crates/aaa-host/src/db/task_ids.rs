//! 표시 번호는 ULID와 별개이며 프로젝트 안에서 영구 예약한다. 삭제·기록 정리는 이 표를 지우지 않는다.
use super::*;

impl Db {
    pub(crate) fn task_short_id(&self, project: &str, id: &str) -> Result<String> {
        let mut c = self.conn.lock().unwrap();
        if let Some(n) = c
            .query_row("SELECT number FROM task_ids WHERE project=?1 AND task_id=?2", params![project, id], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        {
            return Ok(format!("d-{n:04}"));
        }
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO task_ids(project,task_id,number)
             SELECT ?1,?2,COALESCE(MAX(number),0)+1 FROM task_ids WHERE project=?1
             ON CONFLICT(project,task_id) DO NOTHING",
            params![project, id],
        )?;
        let n: i64 =
            tx.query_row("SELECT number FROM task_ids WHERE project=?1 AND task_id=?2", params![project, id], |r| {
                r.get(0)
            })?;
        tx.commit()?;
        Ok(format!("d-{n:04}"))
    }

    pub(crate) fn task_long_id(&self, project: &str, short: &str) -> Result<Option<String>> {
        let Some(n) = aaa_proto::short_id_number(short, 'd') else {
            return Ok(None);
        };
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT task_id FROM task_ids WHERE project=?1 AND number=?2", params![project, n], |r| r.get(0))
            .optional()?)
    }
}
