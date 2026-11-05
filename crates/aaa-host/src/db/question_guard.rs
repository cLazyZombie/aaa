//! 질문 감지는 열린 inbox가 아니라 실제 접수·질문 ID 원장을 근거로 삼는다.
use super::*;

impl Db {
    pub(crate) fn question_guard_posted(
        &self,
        project: &str,
        session: &str,
        started: u64,
        finished: u64,
    ) -> Result<bool> {
        Ok(self.conn.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM posts WHERE project=?1 AND created_ms BETWEEN ?3 AND ?4
             AND data IS NOT NULL AND json_extract(data,'$.from.session')=?2
             AND json_extract(data,'$.from.verified')='coordinator-pane'
             AND json_extract(data,'$.to')='user' AND json_extract(data,'$.post_type')='request'
             AND json_extract(data,'$.needs_user')=1)",
            params![project, session, started as i64, finished as i64],
            |r| r.get(0),
        )?)
    }

    /// None은 다른 프로젝트·존재하지 않는 번호다. 오래된 카드도 누락과 혼동하지 않는다.
    pub(crate) fn question_guard_history(&self, project: &str, id: &str) -> Result<Option<bool>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT draft IS NOT NULL OR EXISTS(
               SELECT 1 FROM asks a WHERE a.project=q.project AND json_extract(a.data,'$.answer') IS NOT NULL
               AND (json_extract(a.data,'$.question_id')=q.id OR EXISTS(
                 SELECT 1 FROM json_each(a.data,'$.items') item WHERE json_extract(item.value,'$.id')=q.id)))
             FROM question_ids q WHERE project=?1 AND id=?2",
                params![project, id],
                |r| r.get(0),
            )
            .optional()?)
    }
}
