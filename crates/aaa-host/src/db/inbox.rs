//! inbox 항목.

use super::*;

impl Db {
    pub(super) const INBOX_UPSERT: &str =
        "INSERT INTO inbox(id,project,kind,subject,event,done,done_at,created_at,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
         ON CONFLICT(id) DO UPDATE SET done=excluded.done, done_at=excluded.done_at, data=excluded.data";

    pub(super) fn inbox_write(statement: &mut rusqlite::Statement<'_>, i: &aaa_proto::InboxItem) -> Result<()> {
        statement.execute(params![
            i.id,
            i.project,
            i.item_kind,
            i.subject,
            i.event,
            i.done,
            i.done_at_ms.map(|v| v as i64),
            i.created_at_ms as i64,
            serde_json::to_string(i)?
        ])?;
        Ok(())
    }

    /// inbox 항목(JSON)을 넣거나 바꾼다
    pub fn inbox_put(&self, i: &aaa_proto::InboxItem) -> Result<()> {
        let c = self.conn.lock().unwrap();
        let mut statement = c.prepare(Self::INBOX_UPSERT)?;
        Self::inbox_write(&mut statement, i)
    }

    /// 멈춘 host에 옛 기록을 들여올 때 한 묶음을 함께 확정한다. 중간 실패는 모두 되돌린다.
    pub(crate) fn inbox_put_many(&self, items: &[aaa_proto::InboxItem]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        {
            let mut statement = tx.prepare(Self::INBOX_UPSERT)?;
            for item in items {
                Self::inbox_write(&mut statement, item)?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 열린 항목의 종료와 처리 알림을 함께 저장한다. 다른 경로가 이미 끝냈으면 아무것도 쓰지 않는다.
    pub(crate) fn inbox_finish(
        &self,
        project: &str,
        id: &str,
        now: u64,
        notice: Option<&aaa_proto::InboxItem>,
    ) -> Result<Option<aaa_proto::InboxItem>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        // JSON도 현재 DB 값의 done 필드만 바꾼다. 오래된 목록의 seen·본문을 덮어쓰지 않는다.
        let data: Option<String> = tx
            .query_row(
                "UPDATE inbox SET done=1, done_at=?3,
             data=json_set(data,'$.done',json('true'),'$.done_at_ms',?3)
             WHERE project=?1 AND id=?2 AND done=0 RETURNING data",
                params![project, id, now as i64],
                |r| r.get(0),
            )
            .optional()?;
        let Some(data) = data else { return Ok(None) };
        let item = serde_json::from_str(&data)?;
        if let Some(n) = notice {
            tx.execute(
                "INSERT INTO inbox(id,project,kind,subject,event,done,created_at,data) VALUES(?1,?2,?3,?4,?5,0,?6,?7)",
                params![
                    n.id,
                    n.project,
                    n.item_kind,
                    n.subject,
                    n.event,
                    n.created_at_ms as i64,
                    serde_json::to_string(n)?
                ],
            )?;
        }
        tx.commit()?;
        Ok(Some(item))
    }

    /// 오래된 목록으로 읽음을 표시해도 끝낸 항목을 다시 열지 않는다.
    pub(crate) fn inbox_mark_seen(&self, project: &str, id: &str) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE inbox SET data=json_set(data,'$.seen',json('true')) WHERE project=?1 AND id=?2",
            params![project, id],
        )?;
        Ok(())
    }

    /// 새 항목만 넣는다. 같은 ID가 이미 있으면 넣지 않고 false다(다른 프로젝트 항목을 덮어쓰지 않는다)
    pub fn inbox_insert(&self, i: &aaa_proto::InboxItem) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let n = c.execute(
            "INSERT INTO inbox(id,project,kind,subject,event,done,done_at,created_at,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO NOTHING",
            params![
                i.id,
                i.project,
                i.item_kind,
                i.subject,
                i.event,
                i.done,
                i.done_at_ms.map(|v| v as i64),
                i.created_at_ms as i64,
                serde_json::to_string(i)?
            ],
        )?;
        Ok(n == 1)
    }

    /// 알림의 예산 epoch와 항목을 함께 저장한다. done·prune은 이 표지를 지우지 않는다.
    pub(crate) fn inbox_insert_marked(
        &self,
        i: &aaa_proto::InboxItem,
        key: &str,
        epoch: &str,
        already_seen: bool,
    ) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let previous: Option<String> =
            tx.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional()?;
        if previous.as_deref() == Some(epoch) {
            return Ok(false);
        }
        let inserted = if already_seen {
            false
        } else {
            tx.execute("INSERT INTO inbox(id,project,kind,subject,event,done,done_at,created_at,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO NOTHING",params![i.id,i.project,i.item_kind,i.subject,i.event,i.done,i.done_at_ms.map(|v|v as i64),i.created_at_ms as i64,serde_json::to_string(i)?])?==1
        };
        tx.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [key, epoch],
        )?;
        tx.commit()?;
        Ok(inserted)
    }

    /// 만든 순서대로. `all`이 아니면 끝내지 않은 것만.
    pub fn inbox_list(&self, project: Option<&str>, all: bool) -> Result<Vec<aaa_proto::InboxItem>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT data FROM inbox WHERE (?1 IS NULL OR project=?1) AND (?2 OR done=0) ORDER BY created_at, id",
        )?;
        let rows = st.query_map(params![project, all], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r?)?);
        }
        out.sort_by(crate::inbox::order);
        Ok(out)
    }

    /// 같은 종류·대상·사건의 끝내지 않은 항목이 있는가(특수 항목 중복 방지)
    pub fn inbox_open_exists(&self, project: &str, kind: &str, subject: &str, event: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            "SELECT 1 FROM inbox WHERE project=?1 AND kind=?2 AND subject=?3 AND event=?4 AND done=0 LIMIT 1",
            params![project, kind, subject, event],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
    }

    /// 끝낸 항목까지 포함해 옛 임무 알림이 이미 있었는지 확인한다.
    pub(crate) fn legacy_notice_exists(&self, project: &str, subject: &str, event: &str) -> Result<bool> {
        self.conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM inbox WHERE project=?1 AND subject=?2 AND event=?3)",
                params![project, subject, event],
                |r| r.get(0),
            )
            .map_err(Into::into)
    }

    /// 그 종류·대상의 마지막 항목(사건, 끝냈는가). 끊김이 회복 없이 아직 이어지는지 본다(inbox.rs `outage_open`)
    pub fn inbox_last(&self, project: &str, kind: &str, subject: &str) -> Result<Option<(String, bool)>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            "SELECT event, done FROM inbox WHERE project=?1 AND kind=?2 AND subject=?3
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
            params![project, kind, subject],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
        )
        .optional()?)
    }

    /// 프로젝트 inbox 순번(항목 ID에 붙인다)
    pub fn inbox_count(&self, project: &str) -> Result<u64> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT COUNT(*) FROM inbox WHERE project=?1", [project], |r| r.get::<_, i64>(0))? as u64)
    }

    /// 끝낸 지 오래된 항목을 지우고 (project, id)를 돌려준다
    pub fn inbox_prune(&self, done_before_ms: u64) -> Result<Vec<(String, String)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT project, id FROM inbox WHERE done=1 AND done_at < ?1")?;
        let gone = st
            .query_map([done_before_ms as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
        for (project, id) in &gone {
            if let Some((_, post)) = id.rsplit_once("-post-") {
                c.execute(
                    "UPDATE posts SET data=NULL WHERE project=?1 AND json_extract(data,'$.id')=?2",
                    params![project, post],
                )?;
            }
        }
        c.execute("DELETE FROM inbox WHERE done=1 AND done_at < ?1", [done_before_ms as i64])?;
        Ok(gone)
    }
}
