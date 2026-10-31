//! 보고서 목록에서 같은 프로젝트의 원장을 보고서마다 다시 읽지 않는다.
use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub(crate) struct LibraryCatalog {
    pub archived: HashSet<String>,
    pub rows: HashMap<String, LibraryReportRow>,
    pub reserved: HashSet<String>,
    pub versions: HashMap<String, Vec<String>>,
    pub threads: HashMap<String, String>,
}

impl Db {
    pub(crate) fn library_catalog(&self, project: &str) -> Result<LibraryCatalog> {
        let c = self.conn.lock().unwrap();
        let mut catalog = LibraryCatalog::default();
        let mut st = c.prepare("SELECT name FROM library_archives WHERE project=?1")?;
        catalog.archived = st.query_map([project], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let mut st = c.prepare(
            "SELECT name,live,updated,to_user,title,sent_at,seen_at,from_thread FROM library_reports WHERE project=?1",
        )?;
        let rows = st.query_map([project], |r| {
            Ok(LibraryReportRow {
                project: project.into(),
                name: r.get(0)?,
                live: r.get(1)?,
                updated: r.get(2)?,
                to_user: r.get::<_, i64>(3)? != 0,
                title: r.get(4)?,
                sent_at: r.get(5)?,
                seen_at: r.get(6)?,
                from_thread: r.get(7)?,
            })
        })?;
        for row in rows {
            let row = row?;
            catalog.rows.insert(row.name.clone(), row);
        }
        let mut st = c.prepare("SELECT id,source,ready,deleted FROM report_ids WHERE project=?1 ORDER BY seq DESC")?;
        let rows = st.query_map([project], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?, r.get::<_, bool>(3)?))
        })?;
        for row in rows {
            let (id, source, ready, deleted) = row?;
            catalog.reserved.insert(id.clone());
            if ready && !deleted {
                catalog.versions.entry(source).or_default().push(id);
            }
        }
        let mut st = c.prepare("SELECT id,data FROM threads WHERE project=?1")?;
        catalog.threads = st.query_map([project], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
        Ok(catalog)
    }
}

impl Db {
    pub(crate) fn library_archived(&self, project: &str, name: &str) -> Result<bool> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT 1 FROM library_archives WHERE project=?1 AND name=?2",
                params![project, name],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub(crate) fn library_archive_set(&self, reports: &[(String, String)], archived: bool) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        for (project, name) in reports {
            if archived {
                tx.execute(
                    "INSERT OR IGNORE INTO library_archives(project,name) VALUES(?1,?2)",
                    params![project, name],
                )?;
            } else {
                tx.execute("DELETE FROM library_archives WHERE project=?1 AND name=?2", params![project, name])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 자동 등록한 thread 보고만 작업 보고로 돌린다. 명시적으로 발행한 보고·보관한 r 판은 그대로 둔다.
    pub(crate) fn library_uncomplete(&self, project: &str, name: &str) -> Result<bool> {
        // 마지막 사용자 보고의 판·열람 기록은 보존한다. 같은 본문으로 돌아와도 새 안 봄이 되지 않는다.
        Ok(self.conn.lock().unwrap().execute(
            "UPDATE library_reports SET to_user=0 WHERE project=?1 AND name=?2 AND to_user=1
             AND EXISTS(SELECT 1 FROM library_completion WHERE project=?1 AND name=?2)",
            params![project, name],
        )? > 0)
    }

    /// 사용자 확인할 같은 본문은 수집·resolve·재시작으로 다시 안 봄이 되지 않는다.
    pub(crate) fn library_complete(
        &self,
        project: &str,
        name: &str,
        revision: &str,
        title: &str,
        now: u64,
    ) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let changed = tx.execute(
            "INSERT INTO library_completion(project,name,revision) VALUES(?1,?2,?3)
            ON CONFLICT(project,name) DO UPDATE SET revision=excluded.revision WHERE revision<>excluded.revision",
            params![project, name, revision],
        )? > 0;
        let restored = if changed {
            tx.execute("UPDATE library_reports SET to_user=1,title=COALESCE(title,?3),sent_at=?4,seen_at=NULL WHERE project=?1 AND name=?2", params![project,name,title,now])? > 0
        } else {
            tx.execute(
                "UPDATE library_reports SET to_user=1 WHERE project=?1 AND name=?2 AND to_user=0",
                params![project, name],
            )? > 0
        };
        tx.commit()?;
        Ok(changed || restored)
    }
}
