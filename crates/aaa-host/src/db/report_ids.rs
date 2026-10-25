//! 보고 번호 원장. 파일이 삭제돼도 행·seq는 남긴다. 예약 실패의 빈 번호도 재사용하지 않는다.
use super::*;

#[derive(Debug, Clone)]
pub(crate) struct ReportId {
    pub id: String,
    pub source: String,
    pub revision: String,
}

impl Db {
    /// 이 DB가 소유한 보고 원본만 등록한다. 별도 state의 빈 DB가 기본 프로젝트 폴더를 스캔해 쓰지 않는다.
    pub(crate) fn report_sources(&self) -> Result<Vec<(String, String)>> {
        let c = self.conn.lock().unwrap();
        let mut st =
            c.prepare("SELECT project,name FROM library_reports UNION SELECT project,id FROM threads ORDER BY 1,2")?;
        let mut sources = st
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        sources.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| aaa_proto::cmp_short_ids(&a.1, &b.1)));
        Ok(sources)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_index_upgrades_history_without_changing_visible_versions() {
        let root = std::env::temp_dir().join(format!("aaa-report-index-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("host.db");
        {
            let db = Db::open(&path).unwrap();
            let c = db.conn.lock().unwrap();
            // 색인이 없던 기존 DB: 삭제 판·미완성 판·다른 프로젝트는 조회에서 제외한다.
            c.execute_batch(
                "DROP INDEX report_ids_source;
                INSERT INTO report_ids VALUES('demo',1,'r-0001','same','deleted',1,1);
                INSERT INTO report_ids VALUES('demo',2,'r-0002','same','pending',0,0);
                INSERT INTO report_ids VALUES('demo',3,'r-0003','same','first',1,0);
                INSERT INTO report_ids VALUES('demo',4,'r-0004','same','last',1,0);
                INSERT INTO report_ids VALUES('other',1,'r-0001','same','other',1,0);",
            )
            .unwrap();
        }
        for _ in 0..2 {
            let db = Db::open(&path).unwrap();
            let versions = db.report_ids("demo", "same").unwrap();
            assert_eq!(versions.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(), ["r-0004", "r-0003"]);
            assert_eq!(db.report_ids("other", "same").unwrap()[0].revision, "other");
            let c = db.conn.lock().unwrap();
            let plan: String = c.query_row(
                "EXPLAIN QUERY PLAN SELECT id,source,revision FROM report_ids WHERE project=?1 AND source=?2 AND ready=1 AND deleted=0 ORDER BY seq DESC",
                params!["demo", "same"], |r| r.get(3),
            ).unwrap();
            assert!(
                plan.contains("report_ids_source") && plan.contains("source=?"),
                "프로젝트 전체를 훑지 않는다: {plan}"
            );
            assert_eq!(c.query_row("SELECT count(*) FROM report_ids", [], |r| r.get::<_, i64>(0)).unwrap(), 5);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
