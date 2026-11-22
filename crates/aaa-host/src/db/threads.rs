//! thread 기록과 흐름 사건.

use super::*;

impl Db {
    // ───────────── thread·흐름 사건(M1-3) ─────────────

    /// 프로젝트의 다음 thread 번호(`t-0001`부터)를 잡고 기록을 넣는다. `make`는 ID를 받아 저장할 JSON을 만든다.
    /// 새 행이 보이기 전에 호출자가 기동 소유권을 얻는다. 실패하면 행·예약·영수증을 모두 롤백한다.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn try_thread_insert_admitted(
        &self,
        project: &str,
        key: Option<&str>,
        admission: Option<&StartAdmission>,
        limit: Option<u32>,
        reservation: Option<(&str, &str)>,
        make: impl FnOnce(&str) -> Result<String>,
    ) -> Result<(String, bool)> {
        self.thread_insert_transaction(project, key, admission, limit, reservation, |_, id| make(id))
    }

    /// 계정 선택·공유 잔량 예약도 thread 선점과 같은 transaction에서 확정한다.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn thread_insert_transaction(
        &self,
        project: &str,
        key: Option<&str>,
        admission: Option<&StartAdmission>,
        limit: Option<u32>,
        reservation: Option<(&str, &str)>,
        make: impl FnOnce(&Connection, &str) -> Result<String>,
    ) -> Result<(String, bool)> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        if let Some(key) = key
            && let Some(id) =
                tx.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get::<_, String>(0)).optional()?
        {
            return Ok((id, false));
        }
        if let Some(policy) = admission {
            policy.check(&tx, project, None)?;
        }
        let max: Option<i64> =
            tx.query_row("SELECT MAX(CAST(SUBSTR(id, 3) AS INTEGER)) FROM threads WHERE project=?1", [project], |r| {
                r.get(0)
            })?;
        let next = max.unwrap_or(0).checked_add(1).ok_or_else(|| anyhow::anyhow!("thread 번호를 다 썼다"))?;
        let id = format!("t-{next:04}");
        let text = make(&tx, &id)?;
        if let Some(limit) = limit
            && serde_json::from_str::<crate::orch::ThreadRec>(&text)?.automation.is_none()
        {
            anyhow::ensure!(
                task_queue::slots(&tx, project, None)? < limit,
                "start_wait: 프로젝트 실행 한도 {limit}. `aaa task reserve`로 예약한다"
            );
        }
        tx.execute(
            "INSERT INTO threads(project,id,status,data,updated_at) VALUES(?1,?2,'starting',?3,?4)",
            params![project, id, text, aaa_common::util::now_ms() as i64],
        )?;
        if let Some((task, block)) = reservation {
            task_queue::queue_claim(&tx, project, task, block, &id)?;
        }
        if let Some(key) = key {
            tx.execute("INSERT INTO meta(key,value) VALUES(?1,?2)", params![key, id])?;
        }
        tx.commit()?;
        Ok((id, true))
    }

    /// thread 기록을 저장하고 상태(열림·끝남 등)가 바뀌었는지 돌려준다(답 정정 가능 여부가 따라 바뀐다, t-2117)
    pub fn thread_save(&self, project: &str, id: &str, status: &str, data: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let before: Option<String> = c
            .query_row("SELECT status FROM threads WHERE project=?1 AND id=?2", params![project, id], |r| r.get(0))
            .optional()?;
        c.execute(
            "UPDATE threads SET status=?3, data=?4, updated_at=?5 WHERE project=?1 AND id=?2",
            params![project, id, status, data, aaa_common::util::now_ms() as i64],
        )?;
        Ok(before.is_some_and(|b| b != status))
    }

    /// 옮겨 온 기록을 정해진 ID로 넣는다(`aaa migrate import`). 같은 ID가 있으면 넣지 않고 false.
    pub fn thread_import(&self, project: &str, id: &str, status: &str, data: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let n = c.execute(
            "INSERT INTO threads(project,id,status,data,updated_at) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(project, id) DO NOTHING",
            params![project, id, status, data, aaa_common::util::now_ms() as i64],
        )?;
        Ok(n == 1)
    }

    pub fn thread_get(&self, project: &str, id: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT data FROM threads WHERE project=?1 AND id=?2", [project, id], |r| r.get(0))
            .optional()?)
    }

    /// (project, data) 목록. `project`가 없으면 전부.
    pub fn threads(&self, project: Option<&str>) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT data FROM threads WHERE (?1 IS NULL OR project=?1) ORDER BY project, CAST(SUBSTR(id, 3) AS INTEGER)",
        )?;
        let rows = st.query_map([project], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    /// 호출자 확인에서 다른 pane의 저장 기록을 읽지 않는다. 같은 pane에 붙은 후보의 순서는 threads와 같다.
    pub(crate) fn threads_for_pane(&self, pane: &str) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare_cached(
            "SELECT data FROM threads WHERE CASE WHEN json_valid(data) THEN json_extract(data, '$.pane') END = ?1
             ORDER BY project, CAST(SUBSTR(id, 3) AS INTEGER), id",
        )?;
        Ok(st.query_map([pane], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
    }

    /// 필터와 커서를 DB에서 적용해 끝난 thread 전문을 매 페이지 모두 읽지 않는다.
    pub fn thread_page(
        &self,
        project: Option<&str>,
        role: Option<&str>,
        query: &aaa_proto::ThreadListQuery,
        after: Option<&(String, String)>,
        limit: u32,
    ) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT data FROM threads
             WHERE (?1 IS NULL OR project=?1)
               AND (?2 IS NULL OR COALESCE(json_extract(data, '$.role'), 'worker')=?2)
               AND (NOT ?3 OR status != 'resolved')
               AND (?4 IS NULL OR status=?4)
               AND (?5 IS NULL OR (project, CAST(SUBSTR(id, 3) AS INTEGER), id)
                    > (?5, CAST(SUBSTR(?6, 3) AS INTEGER), ?6))
             ORDER BY project, CAST(SUBSTR(id, 3) AS INTEGER), id LIMIT ?7",
        )?;
        Ok(st
            .query_map(
                params![
                    project,
                    role,
                    query.open,
                    query.status.map(|s| s.as_str()),
                    after.map(|a| &a.0),
                    after.map(|a| &a.1),
                    limit
                ],
                |r| r.get(0),
            )?
            .collect::<rusqlite::Result<Vec<String>>>()?)
    }

    /// 흐름 사건을 프로젝트 순번으로 넣고 그 순번을 돌려준다
    pub fn flow_insert(&self, e: &aaa_proto::FlowEvent) -> Result<u64> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let seq: i64 =
            tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM flow_events WHERE project=?1", [&e.project], |r| {
                r.get(0)
            })?;
        tx.execute(
            "INSERT INTO flow_events(project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail,repo,actor)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                e.project,
                seq,
                e.at_ms as i64,
                e.boot_id,
                e.kind,
                e.thread,
                e.role.map(|r| r.as_str()),
                e.profile,
                e.machine,
                e.evidence,
                e.detail,
                e.repo,
                e.actor
            ],
        )?;
        tx.commit()?;
        Ok(seq as u64)
    }

    pub(crate) fn flow_insert_once(&self, e: &aaa_proto::FlowEvent, key: &str) -> Result<Option<u64>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        if tx.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get::<_, String>(0)).optional()?.is_some() {
            return Ok(None);
        }
        let seq: i64 =
            tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM flow_events WHERE project=?1", [&e.project], |r| {
                r.get(0)
            })?;
        tx.execute(
            "INSERT INTO flow_events(project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail,repo,actor)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                e.project,
                seq,
                e.at_ms as i64,
                e.boot_id,
                e.kind,
                e.thread,
                e.role.map(|r| r.as_str()),
                e.profile,
                e.machine,
                e.evidence,
                e.detail,
                e.repo,
                e.actor
            ],
        )?;
        tx.execute("INSERT INTO meta(key,value) VALUES(?1,'1')", [key])?;
        tx.commit()?;
        Ok(Some(seq as u64))
    }

    /// cursor(`after`) 다음 사건. 프로젝트를 주지 않으면 모든 프로젝트를 시각 순으로.
    pub fn flow_after(&self, project: Option<&str>, after: u64, limit: u32) -> Result<Vec<aaa_proto::FlowEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail,repo,actor FROM flow_events
             WHERE (?1 IS NULL OR project=?1) AND seq>?2 ORDER BY at_ms, project, seq LIMIT ?3",
        )?;
        let rows = st
            .query_map(params![project, after as i64, limit as i64], Self::flow_from)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// `flow_after`의 가장 최근 `limit`개(같은 차례: 오래된 것부터)
    pub fn flow_tail(&self, project: Option<&str>, after: u64, limit: u32) -> Result<Vec<aaa_proto::FlowEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail,repo,actor FROM flow_events
             WHERE (?1 IS NULL OR project=?1) AND seq>?2 ORDER BY at_ms DESC, project DESC, seq DESC LIMIT ?3",
        )?;
        let mut rows = st
            .query_map(params![project, after as i64, limit as i64], Self::flow_from)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub(super) fn flow_from(r: &rusqlite::Row) -> rusqlite::Result<aaa_proto::FlowEvent> {
        Ok(aaa_proto::FlowEvent {
            project: r.get(0)?,
            seq: r.get::<_, i64>(1)? as u64,
            at_ms: r.get::<_, i64>(2)? as u64,
            boot_id: r.get(3)?,
            kind: r.get(4)?,
            thread: r.get(5)?,
            role: r.get::<_, Option<String>>(6)?.and_then(|s| aaa_proto::Role::parse(&s)),
            profile: r.get(7)?,
            machine: r.get(8)?,
            evidence: r.get(9)?,
            detail: r.get(10)?,
            repo: r.get(11)?,
            actor: r.get(12)?,
        })
    }

    /// 프로젝트의 마지막 사건 seq(없으면 0)
    pub fn flow_max(&self, project: &str) -> Result<u64> {
        let c = self.conn.lock().unwrap();
        let v: i64 =
            c.query_row("SELECT COALESCE(MAX(seq), 0) FROM flow_events WHERE project=?1", [project], |r| r.get(0))?;
        Ok(v as u64)
    }

    /// 질문·메시지 기록(JSON). `table`은 questions·messages만.
    pub fn rec_put(&self, table: &str, id: &str, state: &str, data: &str, extra: (&str, &str)) -> Result<()> {
        let c = self.conn.lock().unwrap();
        let now = aaa_common::util::now_ms() as i64;
        match table {
            "questions" => c.execute(
                "INSERT INTO questions(id,pane,state,data,created_at) VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(id) DO UPDATE SET state=excluded.state, data=excluded.data",
                params![id, extra.0, state, data, now],
            )?,
            "messages" => c.execute(
                "INSERT INTO messages(id,project,thread,state,data,created_at) VALUES(?1,?2,?3,?4,?5,?6)
                 ON CONFLICT(id) DO UPDATE SET state=excluded.state, data=excluded.data",
                params![id, extra.0, extra.1, state, data, now],
            )?,
            other => anyhow::bail!("모르는 표: {other}"),
        };
        Ok(())
    }

    /// `state`가 주어지면 그 상태만. 만든 순서대로.
    pub fn rec_list(&self, table: &str, state: Option<&str>) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let sql = match table {
            "questions" => "SELECT data FROM questions WHERE (?1 IS NULL OR state=?1) ORDER BY created_at, id",
            "messages" => "SELECT data FROM messages WHERE (?1 IS NULL OR state=?1) ORDER BY created_at, id",
            other => anyhow::bail!("모르는 표: {other}"),
        };
        let mut st = c.prepare(sql)?;
        let rows = st.query_map([state], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }
}
