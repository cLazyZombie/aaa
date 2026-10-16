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
             CREATE TABLE IF NOT EXISTS workspaces(name TEXT PRIMARY KEY, layout TEXT NOT NULL, updated_at INTEGER NOT NULL);",
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
            params![name, layout, crate::util::now_ms() as i64],
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
            params![id, name, fingerprint, local as i64, crate::util::now_ms() as i64],
        )?;
        Ok(())
    }

    pub fn touch_machine(&self, id: &str, os: &str, boot_id: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE machines SET os=?2, boot_id=?3, last_seen=?4 WHERE id=?1",
            params![id, os, boot_id, crate::util::now_ms() as i64],
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
            params![id, role, token_hash, crate::util::now_ms() as i64],
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

    pub fn audit(&self, actor: &str, action: &str, detail: &str) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,?3,?4)",
            params![crate::util::now_ms() as i64, actor, action, detail],
        );
    }
}
