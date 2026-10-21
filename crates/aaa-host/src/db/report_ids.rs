//! 보고 번호 원장. 파일이 삭제돼도 행·seq는 남긴다. 예약 실패의 빈 번호도 재사용하지 않는다.
use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ReportId {
    pub id: String,
    pub source: String,
    pub revision: String,
}

impl Db {
    pub(crate) fn report_reserve(&self, project: &str, source: &str, revision: &str, floor: u64) -> Result<String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let last: u64 =
            tx.query_row("SELECT COALESCE(MAX(seq),0) FROM report_ids WHERE project=?1", [project], |r| r.get(0))?;
        let seq = last.max(floor).checked_add(1).ok_or_else(|| anyhow::anyhow!("보고 번호를 다 썼다"))?;
        let id = format!("r-{seq:04}");
        tx.execute(
            "INSERT INTO report_ids(project,seq,id,source,revision) VALUES(?1,?2,?3,?4,?5)",
            params![project, seq, id, source, revision],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub(crate) fn report_ready(&self, row: &LibraryReportRow, source: &LibraryReportRow) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO library_reports(project,name,updated,to_user,title,from_thread) VALUES(?1,?2,?3,0,?4,?5)",
            params![row.project, row.name, row.updated, row.title, row.from_thread],
        )?;
        tx.execute("INSERT INTO library_reports(project,name,live,updated,to_user,title,sent_at,seen_at,from_thread) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(project,name) DO UPDATE SET live=excluded.live,updated=excluded.updated,to_user=excluded.to_user,title=excluded.title,sent_at=excluded.sent_at,seen_at=excluded.seen_at,from_thread=excluded.from_thread",params![source.project,source.name,source.live,source.updated,source.to_user as i64,source.title,source.sent_at,source.seen_at,source.from_thread])?;
        tx.execute(
            "UPDATE report_ids SET ready=1 WHERE project=?1 AND id=?2 AND deleted=0",
            params![row.project, row.name],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn report_ids(&self, project: &str, source: &str) -> Result<Vec<ReportId>> {
        let c = self.conn.lock().unwrap();
        let mut s=c.prepare("SELECT id,source,revision FROM report_ids WHERE project=?1 AND source=?2 AND ready=1 AND deleted=0 ORDER BY seq DESC")?;
        Ok(s.query_map(params![project, source], |r| {
            Ok(ReportId { id: r.get(0)?, source: r.get(1)?, revision: r.get(2)? })
        })?
        .collect::<Result<Vec<_>, _>>()?)
    }

    pub(crate) fn report_target(&self, project: &str, id: &str) -> Result<Option<ReportId>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,source,revision FROM report_ids WHERE project=?1 AND id=?2 AND ready=1 AND deleted=0",
                params![project, id],
                |r| Ok(ReportId { id: r.get(0)?, source: r.get(1)?, revision: r.get(2)? }),
            )
            .optional()?)
    }

    pub(crate) fn report_reserved(&self, project: &str, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT 1 FROM report_ids WHERE project=?1 AND id=?2", params![project, id], |_| Ok(()))
            .optional()?
            .is_some())
    }

    pub(crate) fn report_deleted(&self, project: &str, id: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE report_ids SET deleted=1 WHERE project=?1 AND id=?2", params![project, id])?;
        Ok(())
    }
}
