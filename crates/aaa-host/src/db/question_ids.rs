//! 질문 ID의 영속 원장. 답한 것·post 보존 기간과 별개로 삭제하지 않아 순번을 재사용하지 않는다.
use super::*;
use crate::question_ids::Target;

impl Db {
    /// 독립 post로 한 번 식별한 요청은 나중의 thread 카드에 다시 붙이지 않는다.
    pub(crate) fn has_post_questions(&self, project: &str, subject: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().query_row(
            "SELECT EXISTS(SELECT 1 FROM question_ids WHERE project=?1 AND kind='post' AND subject=?2)",
            params![project, subject],
            |r| r.get(0),
        )?)
    }

    pub(crate) fn question_id(
        &self,
        project: &str,
        kind: &str,
        subject: &str,
        revision: &str,
        position: usize,
    ) -> Result<String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        if let Some(id) = tx.query_row(
            "SELECT id FROM question_ids WHERE project=?1 AND kind=?2 AND subject=?3 AND revision=?4 AND position=?5",
            params![project, kind, subject, revision, position as i64], |r| r.get::<_, String>(0),
        ).optional()? { return Ok(id); }
        let n: u64 =
            tx.query_row("SELECT COALESCE(MAX(seq),999)+1 FROM question_ids WHERE project=?1", [project], |r| {
                r.get(0)
            })?;
        let id = format!("q-{n:04}");
        tx.execute(
            "INSERT INTO question_ids(project,seq,id,kind,subject,revision,position) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![project, n, id, kind, subject, revision, position as i64],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub(crate) fn question_target(&self, project: Option<&str>, id: &str) -> Result<Option<Target>> {
        let original = id;
        let canonical = aaa_proto::canonical_short_id(id, 'q');
        let id = canonical.as_deref().unwrap_or(id);
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT project,id,kind,subject,revision,position,draft FROM question_ids WHERE (?1 IS NULL OR project=?1) AND id=?2")?;
        let mut rows = st.query(params![project, id])?;
        let Some(r) = rows.next()? else { return Ok(None) };
        let draft: Option<String> = r.get(6)?;
        let target = Target {
            project: r.get(0)?,
            id: r.get(1)?,
            kind: r.get(2)?,
            subject: r.get(3)?,
            revision: r.get(4)?,
            position: r.get::<_, i64>(5)? as usize,
            draft: draft.as_deref().map(serde_json::from_str).transpose()?,
        };
        anyhow::ensure!(rows.next()?.is_none(), "같은 질문 ID가 여러 프로젝트에 있다. 프로젝트를 지정한다");
        let legacy = c.query_row("SELECT 1 FROM questions WHERE id=?1", [original], |_| Ok(())).optional()?.is_some();
        anyhow::ensure!(
            !legacy || target.kind == "harness" && target.subject == original,
            "옛 확인창 ID와 질문 번호가 겹친다. 현재 표시한 질문 번호를 지정한다"
        );
        Ok(Some(target))
    }

    pub(crate) fn question_claim_draft(&self, target: &Target, answer: &aaa_proto::QuestionAnswerView) -> Result<bool> {
        let n = self.conn.lock().unwrap().execute(
            "UPDATE question_ids SET draft=?3 WHERE project=?1 AND id=?2 AND draft IS NULL",
            params![target.project, target.id, serde_json::to_string(answer)?],
        )?;
        Ok(n == 1)
    }

    pub(crate) fn question_clear_drafts(&self, project: &str, kind: &str, subject: &str, revision: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE question_ids SET draft=NULL WHERE project=?1 AND kind=?2 AND subject=?3 AND revision=?4",
            params![project, kind, subject, revision],
        )?;
        Ok(())
    }

    pub(crate) fn question_set_draft(
        &self,
        target: &Target,
        answer: Option<&aaa_proto::QuestionAnswerView>,
    ) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE question_ids SET draft=?3 WHERE project=?1 AND id=?2",
            params![target.project, target.id, answer.map(serde_json::to_string).transpose()?],
        )?;
        Ok(())
    }
}
