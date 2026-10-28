//! 아이디어의 표시 번호(`n-NNNN`)와 폴더 ULID. 프로젝트 안에서 영구 예약하며 복사·완료 뒤에도 번호를 재사용하지 않는다.
use super::*;

impl Db {
    /// 없으면 다음 번호를 예약한다
    pub(crate) fn idea_short_id(&self, project: &str, id: &str) -> Result<String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO idea_ids(project,idea_id,number)
             SELECT ?1,?2,COALESCE(MAX(number),0)+1 FROM idea_ids WHERE project=?1
             ON CONFLICT(project,idea_id) DO NOTHING",
            params![project, id],
        )?;
        let n: i64 =
            tx.query_row("SELECT number FROM idea_ids WHERE project=?1 AND idea_id=?2", params![project, id], |r| {
                r.get(0)
            })?;
        tx.commit()?;
        Ok(format!("n-{n:04}"))
    }

    /// `n-NNNN`의 ULID. 번호 꼴이 아니거나 원장에 없으면 None
    pub(crate) fn idea_long_id(&self, project: &str, short: &str) -> Result<Option<String>> {
        let Some(n) = aaa_proto::short_id_number(short, 'n') else { return Ok(None) };
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT idea_id FROM idea_ids WHERE project=?1 AND number=?2", params![project, n], |r| r.get(0))
            .optional()?)
    }
}
