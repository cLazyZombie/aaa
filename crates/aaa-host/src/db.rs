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
pub(crate) struct LibraryLinkRow {
    pub id: String,
    pub project: String,
    pub thread: String,
    pub created: u64,
    pub expires: u64,
    pub opened: Option<u64>,
    pub revoked: bool,
}

fn library_link_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LibraryLinkRow> {
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

#[derive(Debug, Clone)]
pub(crate) struct LibraryReportRow {
    pub project: String,
    pub name: String,
    pub live: Option<String>,
    pub updated: u64,
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

/// Web Push 구독. endpoint는 비밀값처럼 다룬다(로그에 남기지 않는다).
#[derive(Debug, Clone)]
pub struct PushSub {
    pub endpoint: String,
    /// 브라우저 ECDH 공개키(base64url, 65바이트)
    pub p256dh: String,
    /// 브라우저 인증 비밀(base64url, 16바이트)
    pub auth: String,
    pub device: String,
    /// 이 기기가 받을 알림(차이 목록 N3). 넣을 때 None이면 전에 고른 것을 둔다(처음이면 둘 다 켬). 읽으면 늘 Some
    pub alerts: Option<Alerts>,
}

/// 기기마다 고르는 알림(herdr-web-ui `AlertPrefs`, 차이 목록 N3). 참고는 「작업 완료」가 끄기·긴 작업·매번이지만 aaa의
/// 「작업 완료」는 thread 새 보고(일 한 단위)라 길이로 거르지 않고 켜기·끄기만 둔다(plan §8.119).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Alerts {
    /// 확인 필요(질문·확인창·thread가 기다림·시작 실패)
    pub input: bool,
    /// 새 보고(검토 대기)
    pub done: bool,
}

impl Default for Alerts {
    fn default() -> Self {
        Alerts { input: true, done: true }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceRow {
    pub id: String,
    pub role: String,
    pub label: String,
    pub created_ms: u64,
    pub last_seen_ms: Option<u64>,
}

/// 이름이 없는 옛 기기(CLI pairing만 있던 때)
pub const DEVICE_LABEL_FALLBACK: &str = "기기";

impl Db {
    pub(crate) fn library_report(&self, project: &str, name: &str) -> Result<Option<LibraryReportRow>> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT live,updated FROM library_reports WHERE project=?1 AND name=?2",
                params![project, name],
                |r| {
                    Ok(LibraryReportRow {
                        project: project.into(),
                        name: name.into(),
                        live: r.get(0)?,
                        updated: r.get(1)?,
                    })
                },
            )
            .optional()?)
    }

    pub(crate) fn library_report_put(&self, r: &LibraryReportRow) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "INSERT INTO library_reports(project,name,live,updated) VALUES(?1,?2,?3,?4)
             ON CONFLICT(project,name) DO UPDATE SET live=excluded.live,updated=excluded.updated",
            params![r.project, r.name, r.live, r.updated],
        )?;
        Ok(())
    }

    pub(crate) fn library_report_remove(&self, project: &str, name: &str) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM library_reports WHERE project=?1 AND name=?2", params![project, name])?;
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

    pub(crate) fn library_link_opened(&self, id: &str, now: u64) -> Result<()> {
        self.conn.lock().unwrap().execute(
            "UPDATE library_links SET opened=?2 WHERE id=?1 AND (opened IS NULL OR opened<=?3)",
            params![id, now, now.saturating_sub(60_000)],
        )?;
        Ok(())
    }

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
             CREATE TABLE IF NOT EXISTS library_links(
               id TEXT PRIMARY KEY, project TEXT NOT NULL, thread TEXT NOT NULL,
               created INTEGER NOT NULL, expires INTEGER NOT NULL, opened INTEGER, revoked INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS library_reports(
               project TEXT NOT NULL, name TEXT NOT NULL, live TEXT, updated INTEGER NOT NULL,
               PRIMARY KEY(project,name));
             CREATE INDEX IF NOT EXISTS library_links_report ON library_links(project, thread, created);
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
             CREATE TABLE IF NOT EXISTS posts(
               seq INTEGER PRIMARY KEY AUTOINCREMENT, post_id TEXT NOT NULL UNIQUE, project TEXT NOT NULL,
               sender TEXT NOT NULL, session TEXT, machine TEXT NOT NULL, content_sha TEXT NOT NULL,
               created_ms INTEGER NOT NULL, result TEXT NOT NULL, data TEXT);
             CREATE INDEX IF NOT EXISTS posts_sender_time ON posts(project,sender,created_ms);
             CREATE TABLE IF NOT EXISTS evaluations(
               project TEXT NOT NULL, id TEXT NOT NULL, state TEXT NOT NULL, sha256 TEXT NOT NULL, data TEXT NOT NULL,
               created_at INTEGER NOT NULL, PRIMARY KEY(project, id));
             CREATE TABLE IF NOT EXISTS push_subs(
               endpoint TEXT PRIMARY KEY, p256dh TEXT NOT NULL, auth TEXT NOT NULL, device TEXT NOT NULL,
               created_at INTEGER NOT NULL);",
        )?;
        // 기기 이름(웹 설정 → 기기, herdr-web-ui devices.json `label`). 열만 더하므로 schema를 올리지 않는다: 옛 aaa도 이 DB를 연다
        let has_label = conn.prepare("SELECT 1 FROM pragma_table_info('devices') WHERE name='label'")?.exists([])?;
        if !has_label {
            conn.execute("ALTER TABLE devices ADD COLUMN label TEXT", [])?;
        }
        // 기기별 알림 종류(차이 목록 N3). 위와 같이 열만 더한다
        let has_alerts =
            conn.prepare("SELECT 1 FROM pragma_table_info('push_subs') WHERE name='alert_input'")?.exists([])?;
        if !has_alerts {
            conn.execute_batch(
                "ALTER TABLE push_subs ADD COLUMN alert_input INTEGER NOT NULL DEFAULT 1;
                 ALTER TABLE push_subs ADD COLUMN alert_done INTEGER NOT NULL DEFAULT 1;",
            )?;
        }
        let v: Option<String> =
            conn.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get(0)).optional()?;
        match v.as_deref().map(str::parse::<i64>) {
            None => {
                conn.execute("INSERT INTO meta(key,value) VALUES('schema',?1)", [SCHEMA_VERSION.to_string()])?;
            }
            Some(Ok(SCHEMA_VERSION)) => {}
            Some(other) => anyhow::bail!("host DB schema {other:?}을 이 aaa(schema {SCHEMA_VERSION})가 모른다"),
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
            params![name, layout, aaa_common::util::now_ms() as i64],
        )?;
        Ok(())
    }

    pub fn delete_workspace(&self, name: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("DELETE FROM workspaces WHERE name=?1", params![name])?;
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
            params![id, name, fingerprint, local as i64, aaa_common::util::now_ms() as i64],
        )?;
        Ok(())
    }

    pub fn touch_machine(&self, id: &str, os: &str, boot_id: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE machines SET os=?2, boot_id=?3, last_seen=?4 WHERE id=?1",
            params![id, os, boot_id, aaa_common::util::now_ms() as i64],
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

    pub fn add_device(&self, id: &str, role: &str, token_hash: &str, label: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO devices(id,role,token_hash,created_at,label) VALUES(?1,?2,?3,?4,?5)",
            params![id, role, token_hash, aaa_common::util::now_ms() as i64, label],
        )?;
        Ok(())
    }

    fn device_from(r: &rusqlite::Row) -> rusqlite::Result<DeviceRow> {
        Ok(DeviceRow {
            id: r.get(0)?,
            role: r.get(1)?,
            label: r.get::<_, Option<String>>(2)?.unwrap_or_else(|| DEVICE_LABEL_FALLBACK.to_string()),
            created_ms: r.get::<_, i64>(3)? as u64,
            last_seen_ms: r.get::<_, Option<i64>>(4)?.map(|v| v as u64),
        })
    }

    pub fn device_by_token_hash(&self, h: &str) -> Result<Option<DeviceRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,role,label,created_at,last_seen FROM devices WHERE token_hash=?1 AND revoked=0",
            [h],
            Self::device_from,
        )
        .optional()?)
    }

    /// 취소하지 않은 기기를 연결한 순서대로
    pub fn devices(&self) -> Result<Vec<DeviceRow>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT id,role,label,created_at,last_seen FROM devices WHERE revoked=0 ORDER BY created_at, id",
        )?;
        let rows = st.query_map([], Self::device_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 마지막으로 본 때. 취소한 기기는 고치지 않는다
    pub fn device_seen(&self, id: &str, now_ms: u64) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute("UPDATE devices SET last_seen=?2 WHERE id=?1 AND revoked=0", params![id, now_ms as i64])?;
        Ok(())
    }

    /// 이름을 바꾼다. 그런 기기가 없으면(취소됐으면) false
    pub fn rename_device(&self, id: &str, label: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        Ok(c.execute("UPDATE devices SET label=?2 WHERE id=?1 AND revoked=0", params![id, label])? > 0)
    }

    /// 취소한다(token hash는 남겨 다시 쓰지 못하게 한다). 그 기기의 push 구독도 지운다. 없으면(이미 취소) false
    pub fn revoke_device(&self, id: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let n = c.execute("UPDATE devices SET revoked=1 WHERE id=?1 AND revoked=0", [id])?;
        if n > 0 {
            c.execute("DELETE FROM push_subs WHERE device=?1", [id])?;
        }
        Ok(n > 0)
    }

    // ───────────── Web Push 구독(§8.26) ─────────────

    /// 구독을 넣는다. 같은 장치의 같은 endpoint면 키를 바꾼다(브라우저가 구독을 새로 만든 경우).
    /// 다른 장치의 endpoint는 키까지 같을 때만 옮긴다: 같은 브라우저가 다시 pairing한 경우다. endpoint만 아는 요청이
    /// 남의 구독을 가져가지 못한다(보안 감사 t-0340 B10. 참고 herdr-web-ui server/push.ts는 늘 옮긴다). 옮기지 않았으면 false.
    /// 알림 종류(`alerts`)를 주면 바꾸고, None이면 전에 고른 것을 둔다(참고 `subscribe`: 「alerts absent keeps what the device chose before」).
    pub fn push_put(&self, s: &PushSub) -> Result<bool> {
        let given = s.alerts.is_some();
        let a = s.alerts.unwrap_or_default();
        let n = self.conn.lock().unwrap().execute(
            "INSERT INTO push_subs(endpoint,p256dh,auth,device,created_at,alert_input,alert_done) VALUES(?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(endpoint) DO UPDATE SET p256dh=excluded.p256dh, auth=excluded.auth, device=excluded.device,
               alert_input=CASE WHEN ?8 THEN excluded.alert_input ELSE push_subs.alert_input END,
               alert_done=CASE WHEN ?8 THEN excluded.alert_done ELSE push_subs.alert_done END
             WHERE push_subs.device=excluded.device OR (push_subs.p256dh=excluded.p256dh AND push_subs.auth=excluded.auth)",
            params![s.endpoint, s.p256dh, s.auth, s.device, aaa_common::util::now_ms() as i64, a.input, a.done, given],
        )?;
        Ok(n > 0)
    }

    /// endpoint를 지운다. `device`를 주면 그 장치의 구독만 지운다.
    pub fn push_del(&self, endpoint: &str, device: Option<&str>) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        let n = match device {
            Some(d) => c.execute("DELETE FROM push_subs WHERE endpoint=?1 AND device=?2", [endpoint, d])?,
            None => c.execute("DELETE FROM push_subs WHERE endpoint=?1", [endpoint])?,
        };
        Ok(n > 0)
    }

    /// 보낼 구독(취소되지 않은 장치의 것만). `device`를 주면 그 장치 것만.
    pub fn push_list(&self, device: Option<&str>) -> Result<Vec<PushSub>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT s.endpoint,s.p256dh,s.auth,s.device,s.alert_input,s.alert_done FROM push_subs s JOIN devices d ON d.id=s.device
             WHERE d.revoked=0 AND (?1 IS NULL OR s.device=?1) ORDER BY s.created_at",
        )?;
        let rows = st
            .query_map([device], |r| {
                Ok(PushSub {
                    endpoint: r.get(0)?,
                    p256dh: r.get(1)?,
                    auth: r.get(2)?,
                    device: r.get(3)?,
                    alerts: Some(Alerts { input: r.get(4)?, done: r.get(5)? }),
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
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
            params![project, id, make(&id), aaa_common::util::now_ms() as i64],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn thread_save(&self, project: &str, id: &str, status: &str, data: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE threads SET status=?3, data=?4, updated_at=?5 WHERE project=?1 AND id=?2",
            params![project, id, status, data, aaa_common::util::now_ms() as i64],
        )?;
        Ok(())
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

    /// 흐름 사건을 프로젝트 순번으로 넣고 그 순번을 돌려준다
    pub fn flow_insert(&self, e: &aaa_proto::FlowEvent) -> Result<u64> {
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
    pub fn flow_after(&self, project: Option<&str>, after: u64, limit: u32) -> Result<Vec<aaa_proto::FlowEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail FROM flow_events
             WHERE (?1 IS NULL OR project=?1) AND seq>?2 ORDER BY at_ms, project, seq LIMIT ?3",
        )?;
        let rows = st
            .query_map(params![project, after as i64, limit as i64], Self::flow_from)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    fn flow_from(r: &rusqlite::Row) -> rusqlite::Result<aaa_proto::FlowEvent> {
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
        })
    }

    /// thread 하나의 사건 전체(순서대로)
    pub fn flow_of_thread(&self, project: &str, thread: &str) -> Result<Vec<aaa_proto::FlowEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail FROM flow_events
             WHERE project=?1 AND thread=?2 ORDER BY seq",
        )?;
        let rows = st.query_map(params![project, thread], Self::flow_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 프로젝트의 마지막 사건 seq(없으면 0)
    pub fn flow_max(&self, project: &str) -> Result<u64> {
        let c = self.conn.lock().unwrap();
        let v: i64 =
            c.query_row("SELECT COALESCE(MAX(seq), 0) FROM flow_events WHERE project=?1", [project], |r| r.get(0))?;
        Ok(v as u64)
    }

    /// 평가 기록(받은 것은 `open`·`decided`, 받지 않은 것은 `invalid`)
    pub fn eval_put(&self, project: &str, id: &str, state: &str, sha256: &str, data: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO evaluations(project,id,state,sha256,data,created_at) VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(project,id) DO UPDATE SET state=excluded.state, data=excluded.data",
            params![project, id, state, sha256, data, aaa_common::util::now_ms() as i64],
        )?;
        Ok(())
    }

    /// (state, sha256, data)
    pub fn eval_get(&self, project: &str, id: &str) -> Result<Option<(String, String, String)>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT state,sha256,data FROM evaluations WHERE project=?1 AND id=?2", [project, id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?)
    }

    /// (state, data), 받은 순서대로
    pub fn eval_list(&self, project: &str) -> Result<Vec<(String, String)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT state,data FROM evaluations WHERE project=?1 AND state!='invalid' ORDER BY created_at, id",
        )?;
        let rows = st.query_map([project], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
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

    /// inbox 항목(JSON)을 넣거나 바꾼다
    pub fn inbox_put(&self, i: &aaa_proto::InboxItem) -> Result<()> {
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

    pub fn audit(&self, actor: &str, action: &str, detail: &str) {
        let c = self.conn.lock().unwrap();
        let _ = c.execute(
            "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,?3,?4)",
            params![aaa_common::util::now_ms() as i64, actor, action, detail],
        );
    }

    /// 프로젝트 하나의 기록(아래 [`PROJECT_TABLES`]의 행과 `<종류>:<프로젝트>…` meta)을 JSON으로. `project delete`가 휴지통 폴더에 둔다.
    pub fn project_export(&self, project: &str) -> Result<serde_json::Value> {
        use rusqlite::types::ValueRef;
        let c = self.conn.lock().unwrap();
        let mut out = serde_json::Map::new();
        for t in PROJECT_TABLES {
            let mut st = c.prepare(&format!("SELECT * FROM {t} WHERE project=?1"))?;
            let cols: Vec<String> = st.column_names().iter().map(|n| n.to_string()).collect();
            let rows = st
                .query_map([project], |r| {
                    let mut o = serde_json::Map::new();
                    for (i, name) in cols.iter().enumerate() {
                        let v = match r.get_ref(i)? {
                            ValueRef::Null => serde_json::Value::Null,
                            ValueRef::Integer(n) => n.into(),
                            ValueRef::Real(f) => f.into(),
                            ValueRef::Text(b) | ValueRef::Blob(b) => String::from_utf8_lossy(b).into_owned().into(),
                        };
                        o.insert(name.clone(), v);
                    }
                    Ok(serde_json::Value::Object(o))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            out.insert(t.to_string(), rows.into());
        }
        let meta: serde_json::Map<String, serde_json::Value> =
            project_meta(&c, project)?.into_iter().map(|(k, v)| (k, v.into())).collect();
        out.insert("meta".into(), meta.into());
        Ok(out.into())
    }

    /// 프로젝트 하나의 기록을 지운다(`project delete`, 먼저 [`Db::project_export`])
    pub fn project_forget(&self, project: &str) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        for t in PROJECT_TABLES {
            tx.execute(&format!("DELETE FROM {t} WHERE project=?1"), [project])?;
        }
        for (k, _) in project_meta(&tx, project)? {
            tx.execute("DELETE FROM meta WHERE key=?1", [&k])?;
        }
        tx.commit()?;
        Ok(())
    }

    /// 프로젝트 이름을 바꾼다(`project rename`): 표의 project 열, thread 기록 JSON(`rec`가 고친다), meta key를 한 transaction으로
    pub fn project_rename(&self, from: &str, to: &str, rec: impl Fn(&str) -> String) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let threads: Vec<(String, String)> = tx
            .prepare("SELECT id, data FROM threads WHERE project=?1")?
            .query_map([from], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (id, data) in threads {
            tx.execute("UPDATE threads SET data=?3 WHERE project=?1 AND id=?2", params![from, id, rec(&data)])?;
        }
        for t in PROJECT_TABLES {
            tx.execute(&format!("UPDATE {t} SET project=?2 WHERE project=?1"), [from, to])?;
        }
        tx.execute("UPDATE posts SET data=json_set(data,'$.project',?1) WHERE project=?1 AND data IS NOT NULL", [to])?;
        for (k, v) in project_meta(&tx, from)? {
            let (kind, rest) = k.split_once(':').unwrap_or((&k, ""));
            let tail = rest.strip_prefix(from).unwrap_or_default();
            tx.execute("DELETE FROM meta WHERE key=?1", [&k])?;
            tx.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [format!("{kind}:{to}{tail}"), v],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}

/// project 열이 있는 표(옛 db에 남은 `merge_queue`는 읽지 않아 옮기지 않는다, 새 db에는 없다 — §8.135)
const PROJECT_TABLES: [&str; 8] =
    ["threads", "flow_events", "messages", "inbox", "evaluations", "library_links", "library_reports", "posts"];

/// 그 프로젝트의 meta: key가 `<종류>:<프로젝트>` 또는 `<종류>:<프로젝트>:…`(routine·PR·nudge·설정 오류·상태 등)
fn project_meta(c: &Connection, project: &str) -> Result<Vec<(String, String)>> {
    let mut st = c.prepare("SELECT key, value FROM meta WHERE key LIKE '%:' || ?1 OR key LIKE '%:' || ?1 || ':%'")?;
    let rows = st
        .query_map([project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // LIKE의 `_`·`%`는 글자 그대로가 아니다: 둘째 마디가 정확히 그 이름인 것만
    Ok(rows.into_iter().filter(|(k, _)| k.split(':').nth(1) == Some(project) && !k.starts_with(':')).collect())
}

/// post 원장은 거부도 ID별로 남긴다. 거부 본문은 저장하지 않는다.
#[derive(Debug)]
pub(crate) struct PostRow {
    pub project: String,
    pub sender: String,
    pub session: Option<String>,
    pub machine: String,
    pub content_sha: String,
    pub created_ms: u64,
    pub result: aaa_proto::PostResult,
}
impl Db {
    pub(crate) fn post_row(&self, id: &str) -> Result<Option<PostRow>> {
        let c = self.conn.lock().unwrap();
        let row = c
            .query_row(
                "SELECT project,sender,session,machine,content_sha,created_ms,result FROM posts WHERE post_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get::<_, String>(6)?)),
            )
            .optional()?;
        row.map(|(project, sender, session, machine, content_sha, created_ms, result)| {
            Ok(PostRow {
                project,
                sender,
                session,
                machine,
                content_sha,
                created_ms,
                result: serde_json::from_str(&result)?,
            })
        })
        .transpose()
    }
    pub(crate) fn post_recent(
        &self,
        project: &str,
        sender: &str,
        after: u64,
    ) -> Result<Vec<(String, u64, String, aaa_proto::PostResult)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT post_id,created_ms,content_sha,result FROM posts WHERE project=?1 AND sender=?2 AND created_ms>=?3 ORDER BY seq")?;
        let rows = st
            .query_map(params![project, sender, after], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, String>(3)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|(id, at, sha, result)| Ok((id, at, sha, serde_json::from_str(&result)?))).collect()
    }
    pub(crate) fn post_next_id(&self) -> Result<String> {
        let c = self.conn.lock().unwrap();
        let n: u64 = c.query_row("SELECT COALESCE(MAX(seq),0)+1 FROM posts", [], |r| r.get(0))?;
        Ok(format!("p-{n:08}"))
    }
    /// 봉투·결과·inbox·감사를 한 transaction으로 확정한다(투영 파일·알림은 그 뒤).
    pub(crate) fn post_record(
        &self,
        post_id: &str,
        row: &PostRow,
        env: Option<&aaa_proto::PostEnvelope>,
        inbox: Option<&aaa_proto::InboxItem>,
        actor: &str,
        detail: &str,
    ) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("INSERT INTO posts(post_id,project,sender,session,machine,content_sha,created_ms,result,data) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)", params![post_id,row.project,row.sender,row.session,row.machine,row.content_sha,row.created_ms,serde_json::to_string(&row.result)?,env.map(serde_json::to_string).transpose()?])?;
        if let Some(i) = inbox {
            tx.execute(
                "INSERT INTO inbox(id,project,kind,subject,event,done,created_at,data) VALUES(?1,?2,?3,?4,?5,0,?6,?7)",
                params![i.id, i.project, i.item_kind, i.subject, i.event, i.created_at_ms, serde_json::to_string(i)?],
            )?;
        }
        tx.execute(
            "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,?3,?4)",
            params![row.created_ms, actor, if env.is_some() { "post_accept" } else { "post_refuse" }, detail],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn post_list(&self, project: &str) -> Result<Vec<aaa_proto::PostEnvelope>> {
        let c = self.conn.lock().unwrap();
        let mut st =
            c.prepare("SELECT data FROM posts WHERE project=?1 AND data IS NOT NULL ORDER BY seq DESC LIMIT 100")?;
        let rows = st.query_map([project], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut bytes = 0;
        let mut posts = Vec::new();
        for s in rows {
            if bytes + s.len() > 512 * 1024 {
                break;
            }
            bytes += s.len();
            posts.push(serde_json::from_str(&s)?);
        }
        Ok(posts)
    }
}
