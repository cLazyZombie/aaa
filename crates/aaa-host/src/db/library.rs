//! 보고서 사본의 열람 기록과 공유 링크.

use super::*;

#[derive(Debug, Clone)]
pub(crate) struct LibraryLinkRow {
    pub id: String,
    pub project: String,
    pub thread: String,
    pub created: u64,
    pub expires: u64,
    pub opened: Option<u64>,
    pub revoked: bool,
}

pub(super) fn library_link_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryLinkRow> {
    Ok(LibraryLinkRow {
        id: r.get(0)?,
        project: r.get(1)?,
        thread: r.get(2)?,
        created: r.get(3)?,
        expires: r.get(4)?,
        opened: r.get(5)?,
        revoked: r.get(6)?,
    })
}

#[derive(Debug, Clone, Default)]
pub(crate) struct LibraryReportRow {
    pub project: String,
    pub name: String,
    pub live: Option<String>,
    pub updated: u64,
    /// 사용자에게 보낸 웹 보고서(`--to-user`, plan §8.256)
    pub to_user: bool,
    pub title: Option<String>,
    /// 사용자 보고서로 보낸(다시 보낸) 때. 이보다 늦게 열었으면 봤다
    pub sent_at: Option<u64>,
    pub seen_at: Option<u64>,
    /// 보고서를 낸 thread(`t-NNNN`, 사용자 요청 10-20). publish 때 정한다
    pub from_thread: Option<String>,
}

impl LibraryReportRow {
    /// 사용자 보고서를 보낸 뒤 아직 열지 않았다
    pub(crate) fn unseen(&self) -> bool {
        self.to_user && self.seen_at.is_none_or(|s| self.sent_at.is_some_and(|t| s < t))
    }
}

impl Db {
    pub(crate) fn library_report(&self, project: &str, name: &str) -> Result<Option<LibraryReportRow>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT live,updated,to_user,title,sent_at,seen_at,from_thread FROM library_reports WHERE project=?1 AND name=?2",
                params![project, name],
                |r| {
                    Ok(LibraryReportRow {
                        project: project.into(),
                        name: name.into(),
                        live: r.get(0)?,
                        updated: r.get(1)?,
                        to_user: r.get::<_, i64>(2)? != 0,
                        title: r.get(3)?,
                        sent_at: r.get(4)?,
                        seen_at: r.get(5)?,
                        from_thread: r.get(6)?,
                    })
                },
            )
            .optional()?)
    }

    #[cfg(test)]
    pub(crate) fn library_report_put(&self, r: &LibraryReportRow) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO library_reports(project,name,live,updated,to_user,title,sent_at,seen_at,from_thread)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(project,name) DO UPDATE SET live=excluded.live,updated=excluded.updated,to_user=excluded.to_user,
               title=excluded.title,sent_at=excluded.sent_at,seen_at=excluded.seen_at,from_thread=excluded.from_thread",
            params![r.project, r.name, r.live, r.updated, r.to_user as i64, r.title, r.sent_at, r.seen_at, r.from_thread],
        )?;
        Ok(())
    }

    /// 한 열람으로 안 봄과 마지막 열람을 함께 적는다. 처음 보거나 다시 보낸 뒤면 true(배지 사건).
    pub(crate) fn library_opened_at(&self, project: &str, name: &str, now: u64) -> Result<bool> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "UPDATE library_reports SET seen_at=?3 WHERE project=?1 AND name=?2 AND to_user=1
               AND (seen_at IS NULL OR seen_at < sent_at)",
            params![project, name, now],
        )? > 0;
        tx.execute(
            "UPDATE library_links SET opened=?3 WHERE project=?1 AND thread=?2 AND revoked=0",
            params![project, name, now],
        )?;
        tx.commit()?;
        Ok(changed)
    }

    /// 목록의 읽음 표시는 실제 열람(link.opened)과 구별한다. 전체를 한 transaction으로 쓴다.
    pub(crate) fn library_mark_read(&self, reports: &[(String, String)], now: u64) -> Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let mut changed = 0;
        for (project, name) in reports {
            changed += tx.execute(
                "UPDATE library_reports SET seen_at=MAX(?3,COALESCE(sent_at,0))
                 WHERE project=?1 AND name=?2 AND to_user=1 AND (seen_at IS NULL OR seen_at < sent_at)",
                params![project, name, now],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }

    /// 열지 않은 사용자 보고서 수(활동 막대 보고서 배지)
    pub(crate) fn library_unseen_count(&self) -> Result<u32> {
        Ok(self.conn.lock().unwrap().query_row(
            "SELECT COUNT(*) FROM library_reports WHERE to_user=1 AND (seen_at IS NULL OR seen_at < sent_at)",
            [],
            |r| r.get(0),
        )?)
    }

    pub(crate) fn library_report_remove(&self, project: &str, name: &str) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM library_reports WHERE project=?1 AND name=?2", params![project, name])?;
        tx.execute("DELETE FROM library_archives WHERE project=?1 AND name=?2", params![project, name])?;
        tx.execute("UPDATE library_links SET revoked=1 WHERE project=?1 AND thread=?2", params![project, name])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn library_links_revoke_all(&self) -> Result<()> {
        self.conn.lock().unwrap().execute("UPDATE library_links SET revoked=1", [])?;
        Ok(())
    }

    /// 유효한 링크 재사용과 삽입을 한 잠금 안에서 한다(회수·웹 발급이 겹쳐도 하나).
    pub(crate) fn library_link_issue(&self, new: LibraryLinkRow) -> Result<(LibraryLinkRow, bool)> {
        let c = self.conn.lock().unwrap();
        let old = c
            .query_row(
                "SELECT id,project,thread,created,expires,opened,revoked FROM library_links
             WHERE project=?1 AND thread=?2 AND revoked=0 AND expires>?3 ORDER BY created DESC LIMIT 1",
                params![new.project, new.thread, new.created],
                library_link_row,
            )
            .optional()?;
        if let Some(old) = old {
            return Ok((old, false));
        }
        c.execute(
            "INSERT INTO library_links(id,project,thread,created,expires) VALUES(?1,?2,?3,?4,?5)",
            params![new.id, new.project, new.thread, new.created, new.expires],
        )?;
        Ok((new, true))
    }

    pub(crate) fn library_link_get(&self, id: &str) -> Result<Option<LibraryLinkRow>> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT id,project,thread,created,expires,opened,revoked FROM library_links WHERE id=?1",
                [id],
                library_link_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub(crate) fn library_links(&self, project: Option<&str>) -> Result<Vec<LibraryLinkRow>> {
        let c = self.conn.lock().unwrap();
        let mut q = c.prepare(
            "SELECT id,project,thread,created,expires,opened,revoked FROM library_links
            WHERE (?1 IS NULL OR project=?1) ORDER BY created DESC, rowid DESC",
        )?;
        Ok(q.query_map([project], library_link_row)?.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn library_link_revoke(&self, id: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().execute("UPDATE library_links SET revoked=1 WHERE id=?1", [id])? != 0)
    }
}
