//! host SQLite(WAL). 등록된 머신·웹 장치·공개 상태·감사 로그. 쓰기는 짧은 잠금 안에서만 한다.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

pub const SCHEMA_VERSION: i64 = 1;

pub struct Db {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone)]
pub struct MachineRow {
    pub id: String,
    pub name: String,
    pub fingerprint: Option<String>,
    pub local: bool,
    pub os: Option<String>,
    pub boot_id: Option<String>,
    pub last_seen_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DeviceRow {
    pub id: String,
    pub role: String,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS machines(
               id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, fingerprint TEXT UNIQUE,
               local INTEGER NOT NULL DEFAULT 0, enrolled_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0,
               os TEXT, boot_id TEXT, last_seen INTEGER);
             CREATE TABLE IF NOT EXISTS devices(
               id TEXT PRIMARY KEY, role TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE,
               created_at INTEGER NOT NULL, last_seen INTEGER, revoked INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS audit(ts INTEGER NOT NULL, actor TEXT NOT NULL, action TEXT NOT NULL, detail TEXT);
             CREATE TABLE IF NOT EXISTS workspaces(name TEXT PRIMARY KEY, layout TEXT NOT NULL, updated_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS threads(
               project TEXT NOT NULL, id TEXT NOT NULL, status TEXT NOT NULL, data TEXT NOT NULL,
               updated_at INTEGER NOT NULL, PRIMARY KEY(project, id));
             CREATE TABLE IF NOT EXISTS flow_events(
               project TEXT NOT NULL, seq INTEGER NOT NULL, at_ms INTEGER NOT NULL, boot_id TEXT NOT NULL,
               kind TEXT NOT NULL, thread TEXT, role TEXT, profile TEXT, machine TEXT, evidence TEXT,
               detail TEXT NOT NULL, PRIMARY KEY(project, seq));
             CREATE TABLE IF NOT EXISTS questions(
               id TEXT PRIMARY KEY, pane TEXT NOT NULL, state TEXT NOT NULL, data TEXT NOT NULL, created_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS messages(
               id TEXT PRIMARY KEY, project TEXT NOT NULL, thread TEXT NOT NULL, state TEXT NOT NULL, data TEXT NOT NULL,
               created_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS inbox(
               id TEXT PRIMARY KEY, project TEXT NOT NULL, kind TEXT NOT NULL, subject TEXT NOT NULL, event TEXT NOT NULL,
               done INTEGER NOT NULL DEFAULT 0, done_at INTEGER, created_at INTEGER NOT NULL, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS merge_queue(
               project TEXT NOT NULL, thread TEXT NOT NULL, seq INTEGER NOT NULL, enqueued_at INTEGER NOT NULL,
               PRIMARY KEY(project, thread));",
        )?;
        let v: Option<String> =
            conn.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get(0)).optional()?;
        match v.as_deref().map(str::parse::<i64>) {
            None => {
                conn.execute("INSERT INTO meta(key,value) VALUES('schema',?1)", [SCHEMA_VERSION.to_string()])?;
            }
            Some(Ok(SCHEMA_VERSION)) => {}
            Some(other) => anyhow::bail!("host DB schema {other:?}을 이 aa(schema {SCHEMA_VERSION})가 모른다"),
        }
        Ok(Db { conn: Mutex::new(conn) })
    }

    pub fn workspaces(&self) -> Result<Vec<(String, String)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT name, layout FROM workspaces")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn save_workspace(&self, name: &str, layout: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO workspaces(name,layout,updated_at) VALUES(?1,?2,?3)
             ON CONFLICT(name) DO UPDATE SET layout=excluded.layout, updated_at=excluded.updated_at",
            params![name, layout, aa_common::util::now_ms() as i64],
        )?;
        Ok(())
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    pub fn del_meta(&self, key: &str) -> Result<()> {
        self.conn.lock().unwrap().execute("DELETE FROM meta WHERE key=?1", [key])?;
        Ok(())
    }

    fn machine_from(r: &rusqlite::Row) -> rusqlite::Result<MachineRow> {
        Ok(MachineRow {
            id: r.get(0)?,
            name: r.get(1)?,
            fingerprint: r.get(2)?,
            local: r.get::<_, i64>(3)? != 0,
            os: r.get(4)?,
            boot_id: r.get(5)?,
            last_seen_ms: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
        })
    }

    const MACHINE_COLS: &str = "id,name,fingerprint,local,os,boot_id,last_seen";

    pub fn machines(&self) -> Result<Vec<MachineRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(&format!(
            "SELECT {} FROM machines WHERE revoked=0 ORDER BY local DESC, name",
            Self::MACHINE_COLS
        ))?;
        let rows = st.query_map([], Self::machine_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn machine_by_fingerprint(&self, fp: &str) -> Result<Option<MachineRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            &format!("SELECT {} FROM machines WHERE fingerprint=?1 AND revoked=0", Self::MACHINE_COLS),
            [fp],
            Self::machine_from,
        )
        .optional()?)
    }

    pub fn machine_by_name(&self, name: &str) -> Result<Option<MachineRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            &format!("SELECT {} FROM machines WHERE name=?1 AND revoked=0", Self::MACHINE_COLS),
            [name],
            Self::machine_from,
        )
        .optional()?)
    }

    /// 등록(원격) 또는 로컬 agent 자동 등록. 같은 id면 이름·fingerprint를 갱신한다.
    pub fn upsert_machine(&self, id: &str, name: &str, fingerprint: Option<&str>, local: bool) -> Result<()> {
        let c = self.conn.lock().unwrap();
        // 폐기된 같은 이름 행은 지운다(다시 등록)
        c.execute("DELETE FROM machines WHERE name=?1 AND revoked=1", [name])?;
        c.execute(
            "INSERT INTO machines(id,name,fingerprint,local,enrolled_at) VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(id) DO UPDATE SET name=excluded.name, fingerprint=excluded.fingerprint, local=excluded.local, revoked=0",
            params![id, name, fingerprint, local as i64, aa_common::util::now_ms() as i64],
        )?;
        Ok(())
    }

    pub fn touch_machine(&self, id: &str, os: &str, boot_id: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE machines SET os=?2, boot_id=?3, last_seen=?4 WHERE id=?1",
            params![id, os, boot_id, aa_common::util::now_ms() as i64],
        )?;
        Ok(())
    }

    pub fn revoke_machine(&self, name: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        let id: Option<String> =
            c.query_row("SELECT id FROM machines WHERE name=?1 AND revoked=0", [name], |r| r.get(0)).optional()?;
        if let Some(id) = &id {
            c.execute("UPDATE machines SET revoked=1, fingerprint=NULL WHERE id=?1", [id])?;
        }
        Ok(id)
    }

    pub fn add_device(&self, id: &str, role: &str, token_hash: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO devices(id,role,token_hash,created_at) VALUES(?1,?2,?3,?4)",
            params![id, role, token_hash, aa_common::util::now_ms() as i64],
        )?;
        Ok(())
    }

    pub fn device_by_token_hash(&self, h: &str) -> Result<Option<DeviceRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT id,role FROM devices WHERE token_hash=?1 AND revoked=0", [h], |r| {
            Ok(DeviceRow { id: r.get(0)?, role: r.get(1)? })
        })
        .optional()?)
    }

    // ───────────── thread·흐름 사건(M1-3) ─────────────

    /// 프로젝트의 다음 thread 번호(`t-0001`부터)를 잡고 기록을 넣는다. `make`는 ID를 받아 저장할 JSON을 만든다.
    pub fn thread_insert_next(&self, project: &str, make: impl FnOnce(&str) -> String) -> Result<String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let max: Option<i64> =
            tx.query_row("SELECT MAX(CAST(SUBSTR(id, 3) AS INTEGER)) FROM threads WHERE project=?1", [project], |r| {
                r.get(0)
            })?;
        let id = format!("t-{:04}", max.unwrap_or(0) + 1);
        tx.execute(
            "INSERT INTO threads(project,id,status,data,updated_at) VALUES(?1,?2,'starting',?3,?4)",
            params![project, id, make(&id), aa_common::util::now_ms() as i64],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn thread_save(&self, project: &str, id: &str, status: &str, data: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE threads SET status=?3, data=?4, updated_at=?5 WHERE project=?1 AND id=?2",
            params![project, id, status, data, aa_common::util::now_ms() as i64],
        )?;
        Ok(())
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

    /// 흐름 사건을 프로젝트 순번으로 넣고 그 순번을 돌려준다
    pub fn flow_insert(&self, e: &aa_proto::FlowEvent) -> Result<u64> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let seq: i64 =
            tx.query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM flow_events WHERE project=?1", [&e.project], |r| {
                r.get(0)
            })?;
        tx.execute(
            "INSERT INTO flow_events(project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
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
                e.detail
            ],
        )?;
        tx.commit()?;
        Ok(seq as u64)
    }

    /// cursor(`after`) 다음 사건. 프로젝트를 주지 않으면 모든 프로젝트를 시각 순으로.
    pub fn flow_after(&self, project: Option<&str>, after: u64, limit: u32) -> Result<Vec<aa_proto::FlowEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail FROM flow_events
             WHERE (?1 IS NULL OR project=?1) AND seq>?2 ORDER BY at_ms, project, seq LIMIT ?3",
        )?;
        let rows = st
            .query_map(params![project, after as i64, limit as i64], |r| {
                Ok(aa_proto::FlowEvent {
                    project: r.get(0)?,
                    seq: r.get::<_, i64>(1)? as u64,
                    at_ms: r.get::<_, i64>(2)? as u64,
                    boot_id: r.get(3)?,
                    kind: r.get(4)?,
                    thread: r.get(5)?,
                    role: r.get::<_, Option<String>>(6)?.and_then(|s| aa_proto::Role::parse(&s)),
                    profile: r.get(7)?,
                    machine: r.get(8)?,
                    evidence: r.get(9)?,
                    detail: r.get(10)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 질문·메시지 기록(JSON). `table`은 questions·messages만.
    pub fn rec_put(&self, table: &str, id: &str, state: &str, data: &str, extra: (&str, &str)) -> Result<()> {
        let c = self.conn.lock().unwrap();
        let now = aa_common::util::now_ms() as i64;
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

    /// inbox 항목(JSON)을 넣거나 바꾼다
    pub fn inbox_put(&self, i: &aa_proto::InboxItem) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO inbox(id,project,kind,subject,event,done,done_at,created_at,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET done=excluded.done, done_at=excluded.done_at, data=excluded.data",
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
        Ok(())
    }

    /// 만든 순서대로. `all`이 아니면 끝내지 않은 것만.
    pub fn inbox_list(&self, project: Option<&str>, all: bool) -> Result<Vec<aa_proto::InboxItem>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT data FROM inbox WHERE (?1 IS NULL OR project=?1) AND (?2 OR done=0) ORDER BY created_at, id",
        )?;
        let rows = st.query_map(params![project, all], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(serde_json::from_str(&r?)?);
        }
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
        c.execute("DELETE FROM inbox WHERE done=1 AND done_at < ?1", [done_before_ms as i64])?;
        Ok(gone)
    }

    /// 병합 대기열 맨 뒤에 넣는다. 이미 있으면 그대로 두고 false.
    pub fn merge_add(&self, project: &str, thread: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let n = c.execute(
            "INSERT OR IGNORE INTO merge_queue(project,thread,seq,enqueued_at)
             VALUES(?1,?2,(SELECT COALESCE(MAX(seq),0)+1 FROM merge_queue WHERE project=?1),?3)",
            params![project, thread, aa_common::util::now_ms() as i64],
        )?;
        Ok(n == 1)
    }

    pub fn merge_remove(&self, project: &str, thread: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        Ok(c.execute("DELETE FROM merge_queue WHERE project=?1 AND thread=?2", [project, thread])? == 1)
    }

    /// 순서대로 (thread, 넣은 시각)
    pub fn merge_list(&self, project: &str) -> Result<Vec<(String, u64)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT thread, enqueued_at FROM merge_queue WHERE project=?1 ORDER BY seq")?;
        let v = st
            .query_map([project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    }

    pub fn audit(&self, actor: &str, action: &str, detail: &str) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,?3,?4)",
            params![aa_common::util::now_ms() as i64, actor, action, detail],
        );
    }
}
