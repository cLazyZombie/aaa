//! 예약 변경·기동 선점은 같은 SQLite 잠금/트랜잭션을 쓴다.
use super::*;
use aaa_proto::{ArchivedTask, QueueState, TaskReservation};

pub(crate) fn slots(tx: &Connection, project: &str, except: Option<&str>) -> Result<u32> {
    let mut st = tx.prepare("SELECT id,data FROM threads WHERE project=?1")?;
    let rows = st.query_map([project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut count = 0;
    for row in rows {
        let (id, text) = row?;
        if except == Some(id.as_str()) {
            continue;
        }
        let r: crate::orch::ThreadRec = serde_json::from_str(&text)?;
        if r.role == aaa_proto::Role::Worker
            && (matches!(r.status, aaa_proto::ThreadStatus::Open | aaa_proto::ThreadStatus::Starting)
                || r.resolve_pending.is_some())
        {
            count += 1;
        }
    }
    Ok(count)
}

impl Db {
    pub(crate) fn queue_list(&self, project: &str) -> Result<Vec<TaskReservation>> {
        let c = self.conn.lock().unwrap();
        queue_rows(&c, project)
    }

    pub(crate) fn queue_change<T>(
        &self,
        project: &str,
        actor: &str,
        change: impl FnOnce(&mut Vec<TaskReservation>) -> Result<T>,
    ) -> Result<T> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let mut rows = queue_rows(&tx, project)?;
        let before = serde_json::to_string(&rows)?;
        let result = change(&mut rows)?;
        let after = serde_json::to_string(&rows)?;
        if before != after {
            for row in &rows {
                tx.execute("INSERT INTO task_queue(project,task_id,data) VALUES(?1,?2,?3) ON CONFLICT(project,task_id) DO UPDATE SET data=excluded.data", params![project, row.task_id, serde_json::to_string(row)?])?;
            }
            tx.execute(
                "INSERT INTO task_queue_log(project,actor,at_ms,before,after) VALUES(?1,?2,?3,?4,?5)",
                params![project, actor, aaa_common::util::now_ms() as i64, before, after],
            )?;
        }
        tx.commit()?;
        Ok(result)
    }

    pub(crate) fn archive_list(&self, project: &str) -> Result<Vec<ArchivedTask>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data FROM task_archive WHERE project=?1 ORDER BY task_id")?;
        let rows = st.query_map([project], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|j| Ok(serde_json::from_str(&j)?)).collect()
    }
    pub(crate) fn archive_save(&self, project: &str, row: &ArchivedTask) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("INSERT INTO task_archive(project,task_id,data) VALUES(?1,?2,?3) ON CONFLICT(project,task_id) DO UPDATE SET data=excluded.data",params![project,row.task_id,serde_json::to_string(row)?])?;
        Ok(())
    }

    /// 이미 자리를 가진 Open의 재개는 같은 자리다. Failed 재시작의 새 자리만 원자적으로 잡는다.
    pub(crate) fn thread_restart_claim(&self, project: &str, id: &str, limit: u32) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let text: String =
            tx.query_row("SELECT data FROM threads WHERE project=?1 AND id=?2", params![project, id], |r| r.get(0))?;
        let mut rec: crate::orch::ThreadRec = serde_json::from_str(&text)?;
        let already_held = rec.status == aaa_proto::ThreadStatus::Open;
        if !already_held && rec.resolve_pending.is_none() {
            anyhow::ensure!(
                slots(&tx, project, Some(id))? < limit,
                "start_wait: 프로젝트 실행 한도 {limit}. `aaa task reserve`로 예약한다"
            );
        }
        rec.launch_existing_slot = already_held;
        rec.launch_sent = false;
        rec.status = aaa_proto::ThreadStatus::Starting;
        tx.execute(
            "UPDATE threads SET status='starting',data=?3 WHERE project=?1 AND id=?2",
            params![project, id, serde_json::to_string(&rec)?],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn queue_rows(c: &Connection, project: &str) -> Result<Vec<TaskReservation>> {
    let mut st = c.prepare("SELECT data FROM task_queue WHERE project=?1")?;
    let texts = st.query_map([project], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut rows: Vec<TaskReservation> =
        texts.into_iter().map(|s| serde_json::from_str(&s)).collect::<serde_json::Result<_>>()?;
    rows.sort_by_key(|r| (r.position, r.created_ms, r.task_id.clone()));
    Ok(rows)
}

pub(super) fn queue_claim(tx: &Connection, project: &str, task: &str, block: &str, thread: &str) -> Result<()> {
    let mut rows = queue_rows(tx, project)?;
    let before = serde_json::to_string(&rows)?;
    let row = rows.iter_mut().find(|r| r.task_id == task).ok_or_else(|| anyhow::anyhow!("예약이 없다"))?;
    anyhow::ensure!(
        row.state == QueueState::Queued && row.approved_block == block,
        "예약이 취소·수정됐거나 이미 기동했다"
    );
    row.thread = Some(thread.to_string());
    row.launch_id = Some(format!("{project}/{thread}"));
    row.state = QueueState::Starting;
    row.updated_ms = aaa_common::util::now_ms();
    row.reason = "기동 선점".into();
    tx.execute(
        "UPDATE task_queue SET data=?3 WHERE project=?1 AND task_id=?2",
        params![project, task, serde_json::to_string(row)?],
    )?;
    tx.execute(
        "INSERT INTO task_queue_log(project,actor,at_ms,before,after) VALUES(?1,'host',?2,?3,?4)",
        params![project, aaa_common::util::now_ms() as i64, before, serde_json::to_string(&rows)?],
    )?;
    Ok(())
}

#[cfg(test)]
impl Db {
    pub(crate) fn test_archive_fault(&self, fail: bool) {
        self.conn.lock().unwrap().execute_batch(if fail {"CREATE TRIGGER deny_archive BEFORE INSERT ON task_archive BEGIN SELECT RAISE(FAIL,'archive fault'); END;"} else {"DROP TRIGGER deny_archive;"}).unwrap();
    }
}

#[cfg(test)]
impl Db {
    /// 앞 예약의 자리 대기 기록 직후 resolve가 완료되는 경합을 같은 DB 쓰기 경계에 고정한다.
    pub(crate) fn test_queue_release_on_wait(&self, project: &str, task: &str, thread: &str) {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let (project, task, thread) = (quote(project), quote(task), quote(thread));
        self.conn
            .lock()
            .unwrap()
            .execute_batch(&format!(
                "CREATE TEMP TRIGGER release_queue_slot AFTER UPDATE ON task_queue
                 WHEN NEW.project={project} AND NEW.task_id={task}
                   AND json_extract(NEW.data,'$.reason')='자리 대기'
                 BEGIN
                   UPDATE threads SET status='resolved',data=json_set(data,'$.status','resolved')
                   WHERE project={project} AND id={thread};
                 END;",
            ))
            .unwrap();
    }

    pub(crate) fn test_archive_finalize_fault(&self, fail: bool) {
        self.conn.lock().unwrap().execute_batch(if fail {"CREATE TRIGGER deny_archive_update BEFORE UPDATE ON task_archive BEGIN SELECT RAISE(FAIL,'archive finalize fault'); END;"} else {"DROP TRIGGER deny_archive_update;"}).unwrap();
    }
}
