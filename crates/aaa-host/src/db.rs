//! host SQLite(WAL). 등록된 머신·웹 장치·공개 상태·감사 로그. 쓰기는 짧은 잠금 안에서만 한다.

use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

mod automation_ids;
pub(crate) mod automation_runs;
#[cfg(test)]
mod inbox_tests;
mod library_catalog;
mod question_ids;
mod report_ids;
mod task_ids;
mod task_queue;
mod web_publication;
mod workspace_metrics;

pub const SCHEMA_VERSION: i64 = 1;

pub(crate) struct StartAdmission {
    pub machine: String,
    pub holds: Vec<String>,
    pub rule: String,
    pub max_running: Option<u32>,
    pub max_auto_threads: u32,
}

impl StartAdmission {
    /// 판정 뒤 다른 요청이 끼어도 같은 DB 트랜잭션 안에서 동시 상한을 지킨다.
    fn check(&self, tx: &Connection, project: &str, except: Option<&str>) -> Result<()> {
        let mut statement = tx.prepare("SELECT id,data FROM threads WHERE project=?1")?;
        let active = statement.query_map([project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut rule_count = 0usize;
        let mut auto_count = 0usize;
        for row in active {
            let (id, text) = row?;
            let rec: crate::orch::ThreadRec = serde_json::from_str(&text)?;
            if except == Some(id.as_str()) || !rec.holds_slot() {
                continue;
            }
            if rec.machine_id == self.machine && rec.holds.iter().any(|h| self.holds.contains(h)) {
                anyhow::bail!("start_wait: {}의 측정 표지({id})", self.machine);
            }
            if let Some(name) = rec.automation {
                auto_count += 1;
                rule_count += usize::from(name == self.rule);
            }
        }
        let cap = self.max_running.filter(|n| *n > 0).unwrap_or(1);
        anyhow::ensure!(rule_count < cap as usize, "start_wait: 자동화 thread 규칙 동시 실행 상한 {cap}");
        anyhow::ensure!(
            self.max_auto_threads == 0 || auto_count < self.max_auto_threads as usize,
            "start_wait: 명시한 프로젝트 자동화 thread 상한"
        );
        Ok(())
    }
}

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

#[derive(Debug, Clone)]
pub struct MachineRow {
    pub id: String,
    pub name: String,
    pub fingerprint: Option<String>,
    pub local: bool,
    pub os: Option<String>,
    pub boot_id: Option<String>,
    pub last_seen_ms: Option<u64>,
    /// 마지막 hello의 CPU 종류·판·실행 파일 SHA-256(웹 머신 상세, plan §8.372)
    pub arch: Option<String>,
    pub software: Option<String>,
    pub sha256: Option<String>,
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
    #[cfg(test)]
    pub(crate) fn conn_for_pool_test(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }
    /// 발신자의 32자리 ID와 화면의 p-번호를 모두 받는다. 중복 전송 판정은 원래 post_row만 쓴다.
    pub(crate) fn post_by_ref(&self, id: &str) -> Result<Option<PostRow>> {
        if !id.starts_with("p-") {
            return self.post_row(id);
        }
        let key: Option<String> = self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT post_id FROM posts WHERE json_extract(result,'$.id')=?1", [id], |r| r.get(0))
            .optional()?;
        key.map(|key| self.post_row(&key)).transpose().map(Option::flatten)
    }

    pub(crate) fn post_inbox(&self, project: &str, id: &str) -> Result<Option<aaa_proto::InboxItem>> {
        let data: Option<String> = self
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT data FROM inbox WHERE project=?1 AND id LIKE ?2",
                params![project, format!("%-post-{id}")],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into)).transpose()
    }

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

    pub fn open(path: &Path) -> Result<Db> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS repo_workspaces(project TEXT NOT NULL, registration TEXT NOT NULL, id TEXT NOT NULL, PRIMARY KEY(project,registration));
             CREATE TABLE IF NOT EXISTS repo_metrics(project TEXT NOT NULL, workspace TEXT NOT NULL, kind TEXT NOT NULL, id TEXT NOT NULL, at_ms INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY(project,workspace,kind,id));
             CREATE INDEX IF NOT EXISTS repo_metrics_time ON repo_metrics(project,workspace,at_ms);
             CREATE TABLE IF NOT EXISTS machines(
               id TEXT PRIMARY KEY, name TEXT NOT NULL UNIQUE, fingerprint TEXT UNIQUE,
               local INTEGER NOT NULL DEFAULT 0, enrolled_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0,
               os TEXT, boot_id TEXT, last_seen INTEGER);
             CREATE TABLE IF NOT EXISTS devices(
               id TEXT PRIMARY KEY, role TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE,
               created_at INTEGER NOT NULL, last_seen INTEGER, revoked INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS input_events(seq INTEGER PRIMARY KEY, ts INTEGER NOT NULL, pane TEXT NOT NULL, thread TEXT, data TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS audit(ts INTEGER NOT NULL, actor TEXT NOT NULL, action TEXT NOT NULL, detail TEXT);
             CREATE TABLE IF NOT EXISTS library_links(
               id TEXT PRIMARY KEY, project TEXT NOT NULL, thread TEXT NOT NULL,
               created INTEGER NOT NULL, expires INTEGER NOT NULL, opened INTEGER, revoked INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS report_ids(
               project TEXT NOT NULL, seq INTEGER NOT NULL, id TEXT NOT NULL, source TEXT NOT NULL, revision TEXT NOT NULL,
               ready INTEGER NOT NULL DEFAULT 0, deleted INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY(project,id), UNIQUE(project,seq));
             CREATE TABLE IF NOT EXISTS library_archives(
               project TEXT NOT NULL, name TEXT NOT NULL, PRIMARY KEY(project,name));
             CREATE TABLE IF NOT EXISTS library_completion(
               project TEXT NOT NULL, name TEXT NOT NULL, revision TEXT NOT NULL, PRIMARY KEY(project,name));
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
             CREATE TABLE IF NOT EXISTS automation_runs(project TEXT NOT NULL,id TEXT NOT NULL,name TEXT NOT NULL,input_id TEXT NOT NULL,batch TEXT,updated INTEGER NOT NULL,data TEXT NOT NULL,PRIMARY KEY(project,id));
             CREATE INDEX IF NOT EXISTS automation_runs_name ON automation_runs(project,name,updated);
             CREATE TABLE IF NOT EXISTS automation_ids(project TEXT NOT NULL, name TEXT NOT NULL, number INTEGER NOT NULL, PRIMARY KEY(project,name), UNIQUE(project,number));
             CREATE TABLE IF NOT EXISTS task_ids(project TEXT NOT NULL, task_id TEXT NOT NULL, number INTEGER NOT NULL, PRIMARY KEY(project,task_id), UNIQUE(project,number));
             CREATE TABLE IF NOT EXISTS task_queue(project TEXT NOT NULL, task_id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(project,task_id));
             CREATE TABLE IF NOT EXISTS task_queue_log(id INTEGER PRIMARY KEY, project TEXT NOT NULL, actor TEXT NOT NULL, at_ms INTEGER NOT NULL, before TEXT NOT NULL, after TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS task_archive(project TEXT NOT NULL, task_id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(project,task_id));
             CREATE TABLE IF NOT EXISTS task_log(
               id INTEGER PRIMARY KEY AUTOINCREMENT, project TEXT NOT NULL, at_ms INTEGER NOT NULL, actor TEXT NOT NULL,
               action TEXT NOT NULL, list TEXT NOT NULL, from_list TEXT, title TEXT NOT NULL, before TEXT, after TEXT,
               thread TEXT, restored_by INTEGER);
             CREATE INDEX IF NOT EXISTS task_log_project ON task_log(project, id);
             CREATE TABLE IF NOT EXISTS question_ids(
               project TEXT NOT NULL, seq INTEGER NOT NULL, id TEXT NOT NULL, kind TEXT NOT NULL,
               subject TEXT NOT NULL, revision TEXT NOT NULL, position INTEGER NOT NULL, draft TEXT,
               PRIMARY KEY(project,id), UNIQUE(project,kind,subject,revision,position), UNIQUE(project,seq));
             CREATE TABLE IF NOT EXISTS asks(
               project TEXT NOT NULL, id TEXT NOT NULL, kind TEXT NOT NULL, subject TEXT NOT NULL,
               done_ms INTEGER NOT NULL, data TEXT NOT NULL, PRIMARY KEY(project, id));
             CREATE TABLE IF NOT EXISTS machine_setups(id TEXT PRIMARY KEY, data TEXT NOT NULL, updated INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS push_subs(
               endpoint TEXT PRIMARY KEY, p256dh TEXT NOT NULL, auth TEXT NOT NULL, device TEXT NOT NULL,
               created_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS web_link_originals(
               change_id TEXT NOT NULL, origin TEXT NOT NULL, kind TEXT NOT NULL, row_id INTEGER NOT NULL, data TEXT NOT NULL,
               PRIMARY KEY(change_id,kind,row_id));
             CREATE TRIGGER IF NOT EXISTS web_originals_posts_delete AFTER DELETE ON posts BEGIN
               DELETE FROM web_link_originals WHERE kind='posts' AND row_id=OLD.rowid; END;
             CREATE TRIGGER IF NOT EXISTS web_originals_posts_prune AFTER UPDATE OF data ON posts WHEN NEW.data IS NULL BEGIN
               DELETE FROM web_link_originals WHERE kind='posts' AND row_id=OLD.rowid; END;
             CREATE TRIGGER IF NOT EXISTS web_originals_inbox_delete AFTER DELETE ON inbox BEGIN
               DELETE FROM web_link_originals WHERE kind='inbox' AND row_id=OLD.rowid; END;
             CREATE TRIGGER IF NOT EXISTS web_originals_asks_delete AFTER DELETE ON asks BEGIN
               DELETE FROM web_link_originals WHERE kind='asks' AND row_id=OLD.rowid; END;",
        )?;
        // 감시 사건은 thread 없이 repo를 가진다. 옛 DB와 함께 읽는 추가 열이다.
        if !conn.prepare("SELECT 1 FROM pragma_table_info('flow_events') WHERE name='repo'")?.exists([])? {
            conn.execute("ALTER TABLE flow_events ADD COLUMN repo TEXT", [])?;
        }
        if !conn.prepare("SELECT 1 FROM pragma_table_info('flow_events') WHERE name='actor'")?.exists([])? {
            conn.execute("ALTER TABLE flow_events ADD COLUMN actor TEXT", [])?;
        }
        // post 한도는 본문 정리 뒤에도 출처별로 남는다. 옛 행은 기존 한도로 유지한다.
        if !conn.prepare("SELECT 1 FROM pragma_table_info('posts') WHERE name='quota'")?.exists([])? {
            conn.execute("ALTER TABLE posts ADD COLUMN quota TEXT NOT NULL DEFAULT 'standard'", [])?;
        }
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
        if !conn.prepare("SELECT 1 FROM pragma_table_info('push_subs') WHERE name='origin'")?.exists([])? {
            conn.execute("ALTER TABLE push_subs ADD COLUMN origin TEXT", [])?;
        }
        // 사용자 보고서(plan §8.256). 위와 같이 열만 더한다
        let has_to_user =
            conn.prepare("SELECT 1 FROM pragma_table_info('library_reports') WHERE name='to_user'")?.exists([])?;
        if !has_to_user {
            conn.execute_batch(
                "ALTER TABLE library_reports ADD COLUMN to_user INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE library_reports ADD COLUMN title TEXT;
                 ALTER TABLE library_reports ADD COLUMN sent_at INTEGER;
                 ALTER TABLE library_reports ADD COLUMN seen_at INTEGER;",
            )?;
        }
        // 머신의 CPU 종류·판·실행 파일 SHA-256(plan §8.372). 위와 같이 열만 더한다
        let has_arch = conn.prepare("SELECT 1 FROM pragma_table_info('machines') WHERE name='arch'")?.exists([])?;
        if !has_arch {
            conn.execute_batch(
                "ALTER TABLE machines ADD COLUMN arch TEXT;
                 ALTER TABLE machines ADD COLUMN software TEXT;
                 ALTER TABLE machines ADD COLUMN sha256 TEXT;",
            )?;
        }
        // 보고서를 낸 thread(사용자 요청 10-20). 위와 같이 열만 더한다
        let has_from_thread =
            conn.prepare("SELECT 1 FROM pragma_table_info('library_reports') WHERE name='from_thread'")?.exists([])?;
        if !has_from_thread {
            conn.execute("ALTER TABLE library_reports ADD COLUMN from_thread TEXT", [])?;
        }
        scrub_audit_env(&mut conn)?;
        let v: Option<String> =
            conn.query_row("SELECT value FROM meta WHERE key='schema'", [], |r| r.get(0)).optional()?;
        match v.as_deref().map(str::parse::<i64>) {
            None => {
                conn.execute("INSERT INTO meta(key,value) VALUES('schema',?1)", [SCHEMA_VERSION.to_string()])?;
            }
            Some(Ok(SCHEMA_VERSION)) => {}
            Some(other) => anyhow::bail!("host DB schema {other:?}을 이 aaa(schema {SCHEMA_VERSION})가 모른다"),
        }
        // 조작 기기 passkey(관리 GUI W7): 옛 aaa는 이 표를 모르고 지나간다(공개 주소 승인은 그때 passkey 없이 한다)
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS passkeys (
            credential TEXT PRIMARY KEY, device TEXT NOT NULL, rp_id TEXT NOT NULL, cose BLOB NOT NULL,
            sign_count INTEGER NOT NULL DEFAULT 0, created_at INTEGER NOT NULL, last_used INTEGER)",
        )?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS install_grants (
            installation TEXT PRIMARY KEY, plan TEXT NOT NULL, nonce TEXT NOT NULL UNIQUE,
            fingerprint TEXT NOT NULL, machine TEXT NOT NULL UNIQUE, name TEXT NOT NULL)",
        )?;
        // v1에는 원격 신원·main 근거가 없다. 통계 전용 자료만 원자적으로 초기화한다.
        let metrics_version: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key='workspace-metrics-schema'", [], |r| r.get(0))
            .optional()?;
        if metrics_version.as_deref() != Some("2") {
            let tx = conn.transaction()?;
            tx.execute_batch("DELETE FROM repo_metrics; DELETE FROM repo_workspaces;
                DELETE FROM meta WHERE key LIKE 'workspace-collection:%';
                INSERT INTO meta(key,value) VALUES('workspace-metrics-schema','2') ON CONFLICT(key) DO UPDATE SET value='2';")?;
            tx.commit()?;
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

    pub fn start_rollout(&self, id: &str, record: &str) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("INSERT INTO meta(key,value) VALUES(?1,?2)", [format!("rollout:{id}"), record.to_string()])?;
        tx.execute("INSERT INTO meta(key,value) VALUES('rollout:active',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [id])?;
        tx.commit()?;
        Ok(())
    }

    /// 종료 inbox와 발행 표지를 함께 저장한다. inbox 정리/host 재시작 뒤에도 다시 알리지 않는다.
    pub fn save_rollout_record(&self, id: &str, record: &str, item: Option<&aaa_proto::InboxItem>) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [format!("rollout:{id}"), record.to_string()],
        )?;
        let Some(item) = item else {
            tx.commit()?;
            return Ok(false);
        };
        let key = format!("rollout:notice:{}", item.subject);
        if tx.query_row("SELECT value FROM meta WHERE key=?1", [&key], |r| r.get::<_, String>(0)).optional()?.is_some()
        {
            tx.commit()?;
            return Ok(false);
        }
        let n = tx.execute(
            "INSERT INTO inbox(id,project,kind,subject,event,done,done_at,created_at,data) VALUES(?1,?2,?3,?4,?5,0,NULL,?6,?7) ON CONFLICT(id) DO NOTHING",
            params![item.id, item.project, item.item_kind, item.subject, item.event, item.created_at_ms as i64, serde_json::to_string(item)?],
        )?;
        tx.execute("INSERT INTO meta(key,value) VALUES(?1,'1')", [&key])?;
        tx.commit()?;
        Ok(n == 1)
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }

    pub(crate) fn meta_suffixes(&self, prefix: &str) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st =
            c.prepare("SELECT substr(key, length(?1)+1) FROM meta WHERE substr(key,1,length(?1))=?1 ORDER BY key")?;
        Ok(st.query_map([prefix], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [key, value],
        )?;
        Ok(())
    }

    /// 읽기→갱신을 한 transaction에서 한다. 동시에 도는 source의 기록을 잃지 않는다.
    pub(crate) fn update_meta(&self, key: &str, update: impl FnOnce(Option<String>) -> Result<String>) -> Result<()> {
        self.update_meta_checked(key, &[], |old| Ok((update(old)?, vec![]))).map(|_| ())
    }

    /// receipt 검사·상태·여러 receipt를 같은 transaction에서 처리한다.
    pub(crate) fn update_meta_checked(
        &self,
        key: &str,
        absent: &[&str],
        update: impl FnOnce(Option<String>) -> Result<(String, Vec<(String, String)>)>,
    ) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        for guard in absent {
            if tx.query_row("SELECT 1 FROM meta WHERE key=?1", [guard], |_| Ok(())).optional()?.is_some() {
                return Ok(false);
            }
        }
        let old = tx.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional()?;
        let (value, extra) = update(old.clone())?;
        automation_runs::sync(&tx, key, old.as_deref(), &value, &extra)?;
        for (key, value) in std::iter::once((key.to_string(), value)).chain(extra) {
            tx.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [key, value],
            )?;
        }
        tx.commit()?;
        Ok(true)
    }

    /// 이미지 예약도 thread 배정과 같은 DB 잠금·transaction에서 원장을 갱신한다.
    pub(crate) fn image_transaction<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let out = f(&tx)?;
        tx.commit()?;
        Ok(out)
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
            arch: r.get(7)?,
            software: r.get(8)?,
            sha256: r.get(9)?,
        })
    }

    const MACHINE_COLS: &str = "id,name,fingerprint,local,os,boot_id,last_seen,arch,software,sha256";

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

    /// 서명 검증을 마친 grant만 호출한다. nonce 소비와 새 머신 등록은 같은 commit이다.
    pub(crate) fn install_enroll(
        &self,
        installation: &str,
        plan: &str,
        nonce: &str,
        fingerprint: &str,
        id: &str,
        name: &str,
    ) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let previous: Option<(String, String, String, String)> = tx
            .query_row(
                "SELECT plan,fingerprint,machine,name FROM install_grants WHERE installation=?1",
                [installation],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        if let Some(previous) = previous {
            anyhow::ensure!(
                previous == (plan.into(), fingerprint.into(), id.into(), name.into()),
                "install grant target conflict"
            );
            let trusted: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM machines WHERE id=?1 AND name=?2 AND fingerprint=?3 AND revoked=0 AND local=0)",
                params![id,name,fingerprint], |r|r.get(0))?;
            anyhow::ensure!(trusted, "install grant revoked");
        } else {
            let conflict: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM machines WHERE id=?1 OR name=?2 OR fingerprint=?3)",
                params![id, name, fingerprint],
                |r| r.get(0),
            )?;
            // 기존/폐기된 신원을 되살리거나 다른 이름에 덮어쓰지 않는다.
            anyhow::ensure!(!conflict, "install grant target conflict");
            tx.execute("INSERT INTO install_grants(installation,plan,nonce,fingerprint,machine,name) VALUES(?1,?2,?3,?4,?5,?6)",
                params![installation,plan,nonce,fingerprint,id,name])?;
            tx.execute(
                "INSERT INTO machines(id,name,fingerprint,local,enrolled_at) VALUES(?1,?2,?3,0,?4)",
                params![id, name, fingerprint, aaa_common::util::now_ms() as i64],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
    /// 응답 유실 뒤 조회에는 유효 기간을 연장하지 않고 실제 TLS peer와 아직 신뢰하는 머신을 대조한다.
    pub(crate) fn install_enrolled(&self, installation: &str, plan: &str, fingerprint: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT g.machine FROM install_grants g JOIN machines m ON m.id=g.machine
            WHERE g.installation=?1 AND g.plan=?2 AND g.fingerprint=?3 AND m.fingerprint=g.fingerprint AND m.name=g.name AND m.revoked=0 AND m.local=0",
            params![installation,plan,fingerprint], |r|r.get(0)).optional()?)
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

    /// hello를 받을 때마다 그 머신의 OS·부팅·판을 남긴다(끊긴 뒤·host 재시작 뒤에도 마지막 값을 보인다)
    pub fn touch_machine(&self, hello: &aaa_proto::AgentHello) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE machines SET os=?2, boot_id=?3, last_seen=?4, arch=?5, software=?6, sha256=?7 WHERE id=?1",
            params![
                hello.machine_id,
                hello.os,
                hello.boot_id,
                aaa_common::util::now_ms() as i64,
                hello.arch,
                hello.software,
                hello.sha256
            ],
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

    /// SSH 머신 추가 작업(plan §8.384): JSON 한 행. 최근 것부터
    pub(crate) fn machine_setups(&self) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data FROM machine_setups ORDER BY updated DESC, id DESC")?;
        let rows = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(rows)
    }

    pub(crate) fn machine_setup(&self, id: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT data FROM machine_setups WHERE id=?1", [id], |r| r.get(0)).optional()?)
    }

    /// 쓰고, 최근 `keep`개 밖의 오래된 작업은 지운다
    pub(crate) fn machine_setup_put(&self, id: &str, data: &str, updated_ms: u64, keep: usize) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO machine_setups(id,data,updated) VALUES(?1,?2,?3)
             ON CONFLICT(id) DO UPDATE SET data=excluded.data, updated=excluded.updated",
            params![id, data, updated_ms as i64],
        )?;
        c.execute(
            "DELETE FROM machine_setups WHERE id NOT IN (SELECT id FROM machine_setups ORDER BY updated DESC, id DESC LIMIT ?1)",
            [keep as i64],
        )?;
        Ok(())
    }

    pub(crate) fn machine_setup_delete(&self, id: &str) -> Result<bool> {
        Ok(self.conn.lock().unwrap().execute("DELETE FROM machine_setups WHERE id=?1", [id])? != 0)
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
            c.execute("DELETE FROM passkeys WHERE device=?1", [id])?;
        }
        Ok(n > 0)
    }

    /// 시험: 짝지은 때를 옮긴다(passkey 등록 시한)
    #[cfg(test)]
    pub fn set_device_created(&self, id: &str, ms: u64) {
        self.conn
            .lock()
            .unwrap()
            .execute("UPDATE devices SET created_at=?2 WHERE id=?1", params![id, ms as i64])
            .unwrap();
    }

    /// 취소하지 않은 기기 하나
    pub fn device(&self, id: &str) -> Result<Option<DeviceRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,role,label,created_at,last_seen FROM devices WHERE id=?1 AND revoked=0",
            [id],
            Self::device_from,
        )
        .optional()?)
    }

    // ───────────── 조작 기기 passkey(관리 GUI W7) ─────────────

    /// 이 기기의 이 RP passkey를 새 credential로 바꾼다(기기마다 하나). 다른 기기가 쓰는 credential이면 false
    pub fn passkey_put(
        &self,
        credential: &str,
        device: &str,
        rp_id: &str,
        cose: &[u8],
        sign_count: u32,
    ) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let owner: Option<String> =
            tx.query_row("SELECT device FROM passkeys WHERE credential=?1", [credential], |r| r.get(0)).optional()?;
        if owner.is_some_and(|o| o != device) {
            return Ok(false);
        }
        tx.execute("DELETE FROM passkeys WHERE device=?1 AND rp_id=?2", params![device, rp_id])?;
        tx.execute(
            "INSERT OR REPLACE INTO passkeys(credential,device,rp_id,cose,sign_count,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
            params![credential, device, rp_id, cose, sign_count as i64, aaa_common::util::now_ms() as i64],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// 이 기기의 이 RP passkey: (credential, COSE 공개키, 서명 횟수)
    pub fn passkeys(&self, device: &str, rp_id: &str) -> Result<Vec<(String, Vec<u8>, u32)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT credential,cose,sign_count FROM passkeys WHERE device=?1 AND rp_id=?2")?;
        let rows = st
            .query_map(params![device, rp_id], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? as u32)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 이 RP passkey가 있는 기기
    pub fn passkey_devices(&self, rp_id: &str) -> Result<std::collections::HashSet<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT DISTINCT device FROM passkeys WHERE rp_id=?1")?;
        let rows = st.query_map([rp_id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn passkey_used(&self, credential: &str, sign_count: u32, now_ms: u64) -> Result<()> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "UPDATE passkeys SET sign_count=?2, last_used=?3 WHERE credential=?1",
            params![credential, sign_count as i64, now_ms as i64],
        )?;
        Ok(())
    }

    // ───────────── Web Push 구독(§8.26) ─────────────

    /// 구독을 넣는다. 같은 장치의 같은 endpoint면 키를 바꾼다(브라우저가 구독을 새로 만든 경우).
    /// 다른 장치의 endpoint는 키까지 같을 때만 옮긴다: 같은 브라우저가 다시 pairing한 경우다. endpoint만 아는 요청이
    /// 남의 구독을 가져가지 못한다(보안 감사 t-0340 B10. 참고 herdr-web-ui server/push.ts는 늘 옮긴다). 옮기지 않았으면 false.
    /// 알림 종류(`alerts`)를 주면 바꾸고, None이면 전에 고른 것을 둔다(참고 `subscribe`: 「alerts absent keeps what the device chose before」).
    pub fn push_put(&self, s: &PushSub) -> Result<bool> {
        self.push_put_at(s, None)
    }

    pub(crate) fn push_put_at(&self, s: &PushSub, origin: Option<&str>) -> Result<bool> {
        let given = s.alerts.is_some();
        let a = s.alerts.unwrap_or_default();
        let n = self.conn.lock().unwrap().execute(
            "INSERT INTO push_subs(endpoint,p256dh,auth,device,created_at,alert_input,alert_done,origin) VALUES(?1,?2,?3,?4,?5,?6,?7,?9)
             ON CONFLICT(endpoint) DO UPDATE SET p256dh=excluded.p256dh, auth=excluded.auth, device=excluded.device,
               alert_input=CASE WHEN ?8 THEN excluded.alert_input ELSE push_subs.alert_input END,
               alert_done=CASE WHEN ?8 THEN excluded.alert_done ELSE push_subs.alert_done END,
               origin=COALESCE(excluded.origin,push_subs.origin)
             WHERE push_subs.device=excluded.device OR (push_subs.p256dh=excluded.p256dh AND push_subs.auth=excluded.auth)",
            params![s.endpoint, s.p256dh, s.auth, s.device, aaa_common::util::now_ms() as i64, a.input, a.done, given, origin],
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
        self.thread_insert_marked(project, None, None, make).map(|(id, _)| id)
    }
    pub(crate) fn thread_insert_marked(
        &self,
        project: &str,
        key: Option<&str>,
        admission: Option<&StartAdmission>,
        make: impl FnOnce(&str) -> String,
    ) -> Result<(String, bool)> {
        self.thread_insert_admitted(project, key, admission, None, None, make)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn thread_insert_admitted(
        &self,
        project: &str,
        key: Option<&str>,
        admission: Option<&StartAdmission>,
        limit: Option<u32>,
        reservation: Option<(&str, &str)>,
        make: impl FnOnce(&str) -> String,
    ) -> Result<(String, bool)> {
        self.try_thread_insert_admitted(project, key, admission, limit, reservation, |id| Ok(make(id)))
    }

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
        let id = format!("t-{:04}", max.unwrap_or(0) + 1);
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
            repo: r.get(11)?,
            actor: r.get(12)?,
        })
    }

    /// thread 하나의 사건 전체(순서대로)
    pub fn flow_of_thread(&self, project: &str, thread: &str) -> Result<Vec<aaa_proto::FlowEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT project,seq,at_ms,boot_id,kind,thread,role,profile,machine,evidence,detail,repo,actor FROM flow_events
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

    pub(crate) fn input_event(&self, row: &aaa_proto::InputEvent) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO input_events(ts,pane,thread,data) VALUES(?1,?2,?3,?4)",
            params![row.at_ms, row.pane, row.thread, serde_json::to_string(row)?],
        )?;
        tx.execute("DELETE FROM input_events WHERE ts < ?1", [row.at_ms.saturating_sub(30 * 86400 * 1000)])?;
        tx.execute("DELETE FROM input_events WHERE seq <= (SELECT seq FROM input_events ORDER BY seq DESC LIMIT 1 OFFSET 10000)", [])?;
        // 기존 입력 진단 파일(20 × 1MiB)과 같은 최대 본문 예산. 최신 사건부터 남긴다.
        let bytes: u64 =
            tx.query_row("SELECT coalesce(sum(length(CAST(data AS BLOB))),0) FROM input_events", [], |r| r.get(0))?;
        if bytes > 20 * 1024 * 1024 {
            tx.execute("WITH sizes AS (SELECT seq,sum(length(CAST(data AS BLOB))) OVER (ORDER BY seq DESC) AS bytes FROM input_events) DELETE FROM input_events WHERE seq IN (SELECT seq FROM sizes WHERE bytes > 20971520)", [])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn input_events(
        &self,
        pane: Option<&str>,
        thread: Option<&str>,
        since: u64,
    ) -> Result<Vec<aaa_proto::InputEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data FROM (SELECT seq,data FROM input_events WHERE ts>=?1 AND (?2 IS NULL OR pane=?2) AND (?3 IS NULL OR thread=?3 OR substr(thread,instr(thread,'/')+1)=?3) ORDER BY seq DESC LIMIT 1000) ORDER BY seq")?;
        let rows = st.query_map(params![since, pane, thread], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }

    /// 할 일 기록 한 줄(할 일 편집 T2, plan §8.311). 새 id
    pub(crate) fn task_log_add(
        &self,
        project: &str,
        at_ms: u64,
        actor: &str,
        r: &crate::task_edit::Record,
    ) -> Result<i64> {
        let c = self.conn.lock().unwrap();
        c.execute(
            "INSERT INTO task_log(project,at_ms,actor,action,list,from_list,title,before,after,thread)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                project,
                at_ms as i64,
                actor,
                r.action.as_str(),
                r.list,
                r.from_list,
                r.title,
                r.before,
                r.after,
                r.thread
            ],
        )?;
        let id = c.last_insert_rowid();
        if let Some(gone) = r.restored {
            c.execute("UPDATE task_log SET restored_by=?1 WHERE project=?2 AND id=?3", params![id, project, gone])?;
        }
        Ok(id)
    }

    /// 할 일 기록(최신부터, `before` id 앞). `by`는 actor 원문이다(host가 보이는 이름으로 바꾼다)
    pub(crate) fn task_log_list(
        &self,
        project: &str,
        before: Option<i64>,
        limit: u32,
        filter: Option<aaa_proto::TaskLogFilter>,
    ) -> Result<Vec<aaa_proto::TaskLogEntry>> {
        let only = match filter {
            None => "",
            Some(aaa_proto::TaskLogFilter::Deleted) => " AND action IN ('delete','auto_clean')",
            Some(aaa_proto::TaskLogFilter::Auto) => " AND action IN ('auto_clean','completed')",
            Some(aaa_proto::TaskLogFilter::Restored) => " AND (action='restore' OR restored_by IS NOT NULL)",
        };
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(&format!(
            "SELECT id,at_ms,actor,action,list,from_list,title,before,after,thread,restored_by FROM task_log
             WHERE project=?1 AND (?2 IS NULL OR id<?2){only} ORDER BY id DESC LIMIT ?3"
        ))?;
        let rows = st.query_map(params![project, before, limit], task_log_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn task_log_get(&self, project: &str, id: i64) -> Result<Option<aaa_proto::TaskLogEntry>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            "SELECT id,at_ms,actor,action,list,from_list,title,before,after,thread,restored_by FROM task_log
             WHERE project=?1 AND id=?2",
            params![project, id],
            task_log_row,
        )
        .optional()?)
    }

    /// 이 시각보다 오래된 할 일 기록을 지운다. 지운 수
    pub(crate) fn task_log_prune(&self, project: &str, before_ms: u64) -> Result<usize> {
        let c = self.conn.lock().unwrap();
        Ok(c.execute("DELETE FROM task_log WHERE project=?1 AND at_ms<?2", params![project, before_ms as i64])?)
    }

    pub fn audit(&self, actor: &str, action: &str, detail: &str) {
        let c = self.conn.lock().unwrap();
        aaa_common::diagnostic::ignored(
            c.execute(
                "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,?3,?4)",
                params![aaa_common::util::now_ms() as i64, actor, action, detail],
            ),
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
    }

    /// 감사 기록의 (actor, action)을 쓴 순서대로(시험용)
    #[cfg(test)]
    pub(crate) fn audit_actions(&self) -> Vec<(String, String)> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT actor, action FROM audit ORDER BY rowid").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
    }

    /// 프로젝트 하나의 기록(아래 [`PROJECT_TABLES`]의 행과 `<종류>:<프로젝트>…` meta)을 JSON으로. `project delete`가 휴지통 폴더에 둔다.
    /// thread 기록의 launch env는 이름만 남긴다(plan §8.307).
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
                            ValueRef::Text(b) if t == "threads" && name == "data" => {
                                thread_data_without_env(&String::from_utf8_lossy(b)).into()
                            }
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
        crate::account_pool::project_transition(&tx, project, None)?;
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
            let v = if kind == "account-job" { crate::account_pool::rename_job(&v, to)? } else { v };
            tx.execute("DELETE FROM meta WHERE key=?1", [&k])?;
            tx.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [format!("{kind}:{to}{tail}"), v],
            )?;
        }
        crate::account_pool::project_transition(&tx, from, Some(to))?;
        tx.commit()?;
        Ok(())
    }
}

/// env 값(`[[이름, 값]…]` 또는 `{이름: 값}`)을 이름 목록으로 바꾼다(키 관리자 K0, plan §8.307). 바꿨으면 true
fn env_names_only(v: &mut serde_json::Value, path: &[&str]) -> bool {
    use serde_json::Value;
    let Some(env) = path.iter().try_fold(v, |v, k| v.get_mut(*k)) else { return false };
    let names: Vec<Value> = match env {
        Value::Array(items) if items.iter().any(|i| !i.is_string()) => {
            items.iter().map(|i| i.get(0).cloned().unwrap_or(Value::Null)).collect()
        }
        Value::Object(map) => map.keys().map(|k| Value::String(k.clone())).collect(),
        _ => return false,
    };
    *env = Value::Array(names);
    true
}

/// pane_run 감사 기록(띄운 spec)에서 env 값을 뺀다: spec의 env와 host가 푼 profile env
fn spawn_env_names_only(v: &mut serde_json::Value) -> bool {
    let a = env_names_only(v, &["env"]);
    env_names_only(v, &["resolved_profile", "env"]) || a
}

/// pane_run 감사 기록의 detail: env는 이름만(plan §8.307)
pub(crate) fn pane_run_detail(spec: &aaa_proto::SpawnSpec) -> String {
    let mut v = serde_json::to_value(spec).unwrap_or_default();
    spawn_env_names_only(&mut v);
    v.to_string()
}

/// export에 넣는 thread 기록: launch env는 이름만. JSON이 아니면 그대로 둔다
fn thread_data_without_env(data: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return data.to_string();
    };
    if env_names_only(&mut v, &["launch", "env"]) { v.to_string() } else { data.to_string() }
}

/// 옛 판이 pane_run 감사 기록에 남긴 env 값을 이름만 남기고 지운다. 지운 값이 빈 page·WAL에 남지 않게 VACUUM하고 WAL을 비운다.
/// 시작마다 값이 남은 행만 찾으므로 옛 판으로 되돌렸다 다시 올려도 그 사이 행을 정리한다. VACUUM 전에 멈추면 표지가 남아 다음 시작에
/// 다시 한다(plan §8.307).
fn scrub_audit_env(conn: &mut Connection) -> Result<()> {
    let rows: Vec<(i64, String)> = conn
        .prepare(
            "SELECT rowid, detail FROM audit WHERE action='pane_run' AND json_valid(detail)
             AND (json_type(detail,'$.env[0]')='array' OR json_type(detail,'$.resolved_profile.env')='object')",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let pending = conn.query_row("SELECT 1 FROM meta WHERE key='audit_vacuum'", [], |_| Ok(())).optional()?.is_some();
    if rows.is_empty() && !pending {
        return Ok(());
    }
    let tx = conn.transaction()?;
    for (rowid, detail) in &rows {
        let mut v: serde_json::Value = serde_json::from_str(detail)?;
        spawn_env_names_only(&mut v);
        tx.execute("UPDATE audit SET detail=?2 WHERE rowid=?1", params![rowid, v.to_string()])?;
    }
    tx.execute("INSERT INTO meta(key,value) VALUES('audit_vacuum','1') ON CONFLICT(key) DO NOTHING", [])?;
    tx.commit()?;
    // 행의 값은 이미 지웠다. VACUUM(임시 공간·잠금이 필요하다)이 실패해도 host는 시작하고, 표지가 남아 다음 시작에 다시 한다
    let vacuum = conn.execute_batch("VACUUM").and_then(|_| {
        conn.execute("DELETE FROM meta WHERE key='audit_vacuum'", [])?;
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
    });
    match vacuum {
        Ok(()) => tracing::info!(rows = rows.len(), "감사 기록의 옛 env 값을 지웠다(VACUUM)"),
        Err(e) => {
            tracing::warn!(rows = rows.len(), error = %e, "감사 기록의 옛 env 값은 지웠지만 VACUUM을 못 했다: 다음 시작에 다시 한다")
        }
    }
    Ok(())
}

/// project 열이 있는 표(옛 db에 남은 `merge_queue`는 읽지 않아 옮기지 않는다, 새 db에는 없다 — §8.135)
const PROJECT_TABLES: [&str; 22] = [
    "threads",
    "repo_workspaces",
    "repo_metrics",
    "flow_events",
    "messages",
    "inbox",
    "evaluations",
    "library_links",
    "library_reports",
    "library_archives",
    "library_completion",
    "report_ids",
    "question_ids",
    "posts",
    "task_log",
    "task_ids",
    "automation_ids",
    "automation_runs",
    "task_queue",
    "task_queue_log",
    "task_archive",
    "asks",
];

fn task_log_row(r: &rusqlite::Row) -> rusqlite::Result<aaa_proto::TaskLogEntry> {
    let action: String = r.get(3)?;
    Ok(aaa_proto::TaskLogEntry {
        id: r.get(0)?,
        at_ms: r.get::<_, i64>(1)? as u64,
        actor: r.get(2)?,
        by: r.get(2)?,
        action: aaa_proto::TaskAction::parse(&action).unwrap_or(aaa_proto::TaskAction::Raw),
        list: r.get(4)?,
        from_list: r.get(5)?,
        title: r.get(6)?,
        before: r.get(7)?,
        after: r.get(8)?,
        thread: r.get(9)?,
        restored_by: r.get(10)?,
    })
}

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
    pub quota: String,
}
impl Db {
    pub(crate) fn post_row(&self, id: &str) -> Result<Option<PostRow>> {
        let c = self.conn.lock().unwrap();
        let row = c
            .query_row(
                "SELECT project,sender,session,machine,content_sha,created_ms,result,quota FROM posts WHERE post_id=?1",
                [id],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get::<_, String>(6)?,
                        r.get(7)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(project, sender, session, machine, content_sha, created_ms, result, quota)| {
            Ok(PostRow {
                project,
                sender,
                session,
                machine,
                content_sha,
                created_ms,
                result: serde_json::from_str(&result)?,
                quota,
            })
        })
        .transpose()
    }
    pub(crate) fn post_recent(
        &self,
        project: &str,
        sender: &str,
        after: u64,
        quota: Option<&str>,
    ) -> Result<Vec<(String, u64, String, aaa_proto::PostResult)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT post_id,created_ms,content_sha,result FROM posts WHERE project=?1 AND sender=?2 AND created_ms>=?3 AND (?4 IS NULL OR quota=?4) ORDER BY seq")?;
        let rows = st
            .query_map(params![project, sender, after, quota], |r| {
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
        tx.execute("INSERT INTO posts(post_id,project,sender,session,machine,content_sha,created_ms,result,data,quota) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![post_id,row.project,row.sender,row.session,row.machine,row.content_sha,row.created_ms,serde_json::to_string(&row.result)?,env.map(serde_json::to_string).transpose()?,row.quota])?;
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
    /// inbox 항목 ID(`<시각>-post-<번호>`)의 post 봉투. 본문을 정리했으면 None
    pub(crate) fn post_of_item(&self, project: &str, item: &str) -> Result<Option<aaa_proto::PostEnvelope>> {
        let Some((_, id)) = item.rsplit_once("-post-") else { return Ok(None) };
        let c = self.conn.lock().unwrap();
        let data: Option<String> = c
            .query_row(
                "SELECT data FROM posts WHERE project=?1 AND data IS NOT NULL AND json_extract(data,'$.id')=?2",
                params![project, id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(data.map(|d| serde_json::from_str(&d)).transpose()?)
    }

    /// 「답할 것」 기록 하나를 넣는다(plan §8.345)
    pub(crate) fn ask_put(&self, project: &str, r: &aaa_proto::AskRecord) -> Result<()> {
        let kind = serde_json::to_value(r.kind)?.as_str().unwrap_or_default().to_string();
        self.conn.lock().unwrap().execute(
            "INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES(?1,?2,?3,?4,?5,?6)
             ON CONFLICT(project,id) DO UPDATE SET data=excluded.data, done_ms=excluded.done_ms",
            params![project, r.id, kind, r.subject, r.done_ms as i64, serde_json::to_string(r)?],
        )?;
        Ok(())
    }

    /// 요청 종료·답 보존·coordinator 전달을 한 번에 기록한다. 다른 기기에서 끝냈으면 false다.
    pub(crate) fn request_answer(
        &self,
        item: &aaa_proto::InboxItem,
        record: &aaa_proto::AskRecord,
        notice: &aaa_proto::InboxItem,
    ) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let mut done = item.clone();
        done.done = true;
        done.done_at_ms = Some(record.done_ms);
        let n = tx.execute(
            "UPDATE inbox SET done=1, done_at=?3, data=?4 WHERE project=?1 AND id=?2 AND done=0",
            params![item.project, item.id, record.done_ms as i64, serde_json::to_string(&done)?],
        )?;
        if n == 0 {
            return Ok(false);
        }
        tx.execute(
            "INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES(?1,?2,'request',?3,?4,?5)",
            params![item.project, record.id, record.subject, record.done_ms as i64, serde_json::to_string(record)?],
        )?;
        tx.execute(
            "INSERT INTO inbox(id,project,kind,subject,event,done,created_at,data) VALUES(?1,?2,?3,?4,?5,0,?6,?7)",
            params![
                notice.id,
                notice.project,
                notice.item_kind,
                notice.subject,
                notice.event,
                notice.created_at_ms as i64,
                serde_json::to_string(notice)?
            ],
        )?;
        tx.execute(
            "UPDATE question_ids SET draft=NULL WHERE project=?1 AND kind='post' AND subject=?2",
            params![item.project, item.id],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// 프로젝트의 기록, 최신부터
    pub(crate) fn ask_list(&self, project: &str) -> Result<Vec<aaa_proto::AskRecord>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data FROM asks WHERE project=?1 ORDER BY done_ms DESC, id DESC")?;
        let rows = st.query_map([project], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|d| Ok(serde_json::from_str(d)?)).collect()
    }

    /// 기록을 지우고 지운 ID를 돌려준다. `ids`가 None이면 모두
    pub(crate) fn ask_delete(&self, project: &str, ids: Option<&[String]>) -> Result<Vec<String>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let all: Vec<String> = tx
            .prepare("SELECT id FROM asks WHERE project=?1")?
            .query_map([project], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        let gone: Vec<String> = all.into_iter().filter(|id| ids.is_none_or(|ids| ids.contains(id))).collect();
        for id in &gone {
            tx.execute("DELETE FROM asks WHERE project=?1 AND id=?2", params![project, id])?;
        }
        tx.commit()?;
        Ok(gone)
    }

    /// 답한 지 오래된 기록을 지우고 (project, id)를 돌려준다
    pub(crate) fn ask_prune(&self, before_ms: u64) -> Result<Vec<(String, String)>> {
        let c = self.conn.lock().unwrap();
        let gone = c
            .prepare("SELECT project, id FROM asks WHERE done_ms < ?1")?
            .query_map([before_ms as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<(String, String)>>>()?;
        c.execute("DELETE FROM asks WHERE done_ms < ?1", [before_ms as i64])?;
        Ok(gone)
    }

    pub(crate) fn post_list(&self, project: &str) -> Result<Vec<aaa_proto::PostEnvelope>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data, EXISTS(SELECT 1 FROM inbox i WHERE i.project=posts.project AND i.done=1 AND i.id LIKE '%-post-' || json_extract(posts.data,'$.id')) FROM posts WHERE project=?1 AND data IS NOT NULL ORDER BY seq DESC LIMIT 100")?;
        let rows = st
            .query_map([project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut bytes = 0;
        let mut posts = Vec::new();
        for (s, handled) in rows {
            if bytes + s.len() > 512 * 1024 {
                break;
            }
            bytes += s.len();
            let mut post: aaa_proto::PostEnvelope = serde_json::from_str(&s)?;
            if handled {
                post.state = "handled".into();
            }
            posts.push(post);
        }
        Ok(posts)
    }
}

impl Db {}

#[cfg(test)]
mod input_tests {
    use super::*;

    #[test]
    fn input_log_bounds_retention_and_queries_without_message_bodies() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let now = 40 * 86400 * 1000;
        let mut row = aaa_proto::InputEvent {
            at_ms: now,
            id: "new".into(),
            pane: "p1".into(),
            thread: Some("project/t-1".into()),
            harness: Some("claude".into()),
            working: true,
            event: "received".into(),
            detail: "bytes=7 sha256=abc".into(),
        };
        {
            let c = db.conn.lock().unwrap();
            c.execute("WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<10001) INSERT INTO input_events(ts,pane,thread,data) SELECT ?1,'p2','project/t-2',?2 FROM n", params![now, serde_json::to_string(&row).unwrap()]).unwrap();
            c.execute(
                "INSERT INTO input_events(ts,pane,data) VALUES(1,'old',?1)",
                [serde_json::to_string(&row).unwrap()],
            )
            .unwrap();
        }
        db.input_event(&row).unwrap();
        let c = db.conn.lock().unwrap();
        assert_eq!(c.query_row("SELECT count(*) FROM input_events", [], |r| r.get::<_, u64>(0)).unwrap(), 10000);
        assert_eq!(
            c.query_row("SELECT count(*) FROM input_events WHERE pane='old'", [], |r| r.get::<_, u64>(0)).unwrap(),
            0
        );
        drop(c);
        assert_eq!(db.input_events(Some("p1"), Some("t-1"), now).unwrap(), [row.clone()]);
        assert!(db.input_events(Some("p1"), Some("t-2"), 0).unwrap().is_empty());
        assert!(db.input_events(None, None, now + 1).unwrap().is_empty());
        row.at_ms += 1;
        row.id = "later".into();
        db.input_event(&row).unwrap();
        let rows = db.input_events(None, None, 0).unwrap();
        assert_eq!(rows.len(), 1000);
        assert_eq!(rows.last().unwrap(), &row);
        // 1,000자 한국어 상세도 한 줄 3KiB다: 행 수만으로 20MiB를 넘기지 않게 한다.
        let mut large = row.clone();
        large.detail = "가".repeat(1000);
        db.conn
            .lock()
            .unwrap()
            .execute("UPDATE input_events SET data=?1", [serde_json::to_string(&large).unwrap()])
            .unwrap();
        db.input_event(&row).unwrap();
        let c = db.conn.lock().unwrap();
        let (count, bytes): (u64, u64) = c
            .query_row("SELECT count(*),sum(length(CAST(data AS BLOB))) FROM input_events", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert!(count < 10000 && bytes <= 20 * 1024 * 1024);
        drop(c);
        assert_eq!(db.input_events(Some("p1"), Some("t-1"), now + 1).unwrap().last().unwrap(), &row);
    }
}

#[cfg(test)]
mod rollout_tests {
    use super::*;
    #[test]
    fn rollout_terminal_state_and_inbox_commit_together() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        db.start_rollout("r", "running").unwrap();
        let item = aaa_proto::InboxItem {
            id: "rollout-r".into(),
            project: "demo".into(),
            item_kind: "rollout".into(),
            subject: "r".into(),
            event: "rollout finished".into(),
            summary: "배포 완료".into(),
            body: String::new(),
            done: false,
            seen: false,
            created_at_ms: 1,
            done_at_ms: None,
        };
        db.conn.lock().unwrap().execute_batch("CREATE TRIGGER fail_rollout_inbox BEFORE INSERT ON inbox WHEN NEW.kind='rollout' BEGIN SELECT RAISE(ABORT, 'inbox write failed'); END;").unwrap();
        assert!(
            db.save_rollout_record("r", "done", Some(&item))
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_err()
        );
        assert_eq!(db.meta("rollout:r").unwrap().as_deref(), Some("running"), "inbox 없이 완료만 commit하면 안 된다");
        assert_eq!(db.meta("rollout:notice:r").unwrap(), None);
        db.conn.lock().unwrap().execute_batch("DROP TRIGGER fail_rollout_inbox;").unwrap();
        assert!(db.save_rollout_record("r", "done", Some(&item)).unwrap());
        assert_eq!(db.meta("rollout:r").unwrap().as_deref(), Some("done"));
        assert_eq!(db.inbox_list(Some("demo"), true).unwrap(), [item]);
        assert!(!db.save_rollout_record("r", "done", None).unwrap());
    }
}

#[cfg(test)]
mod audit_env_tests {
    use super::*;

    fn bytes_have(dir: &Path, needle: &str) -> Vec<String> {
        ["host.db", "host.db-wal", "host.db-shm"]
            .iter()
            .filter(|f| {
                std::fs::read(dir.join(f))
                    .inspect_err(|error| aaa_common::diagnostic::observe(error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes()))
            })
            .map(|f| f.to_string())
            .collect()
    }

    /// 옛 판이 남긴 pane_run 감사 기록(SpawnSpec 직렬화 전체)을 연 Db가 env 이름만 남기고, 지운 값이 빈 page·WAL에 남지 않게
    /// VACUUM한다(plan §8.307). 정리 전 DB 사본으로 시험하고, 다시 열어도 그대로다.
    #[test]
    fn open_scrubs_old_pane_run_env_values_and_vacuums() {
        let root = std::env::temp_dir().join(format!("aaa-audit-env-{}", std::process::id()));
        let (old, copy) = (root.join("old"), root.join("copy"));
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&root), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&copy).unwrap();
        let (value, gone) = ("aaa-canary-audit-5f29deba74dd23fe", "aaa-canary-deleted-0c827dbcceed4341");
        let spec = |v: &str| aaa_proto::SpawnSpec {
            name: "demo/t-0001".into(),
            argv: vec!["/bin/sh".into()],
            cwd: None,
            env: vec![("GH_TOKEN".into(), v.into()), ("AAA_THREAD".into(), "t-0001".into())],
            cols: 120,
            rows: 32,
            role: None,
            harness: None,
            profile: Some("api".into()),
            resolved_profile: Some(aaa_proto::ResolvedProfile {
                harness: "claude".into(),
                model: String::new(),
                effort: String::new(),
                args: vec![],
                command: vec![],
                env: [("ANTHROPIC_API_KEY".to_string(), v.to_string())].into(),
            }),
        };
        {
            // 고치기 전 판이 쓰던 그대로: spec 전체를 detail에 둔다. 지운 행의 값도 빈 page에 남아 있다
            let db = Db::open(&old.join("host.db")).unwrap();
            let c = db.conn.lock().unwrap();
            let put = |v: &str| {
                c.execute(
                    "INSERT INTO audit(ts,actor,action,detail) VALUES(1,'local','pane_run',?1)",
                    [serde_json::to_string(&spec(v)).unwrap()],
                )
                .unwrap()
            };
            put(value);
            put(gone);
            c.execute("DELETE FROM audit WHERE detail LIKE '%' || ?1 || '%'", [gone]).unwrap();
            c.execute("INSERT INTO audit(ts,actor,action,detail) VALUES(2,'local','pane_run','p1 5 bytes')", [])
                .unwrap();
            c.execute("INSERT INTO audit(ts,actor,action,detail) VALUES(3,'local','web_publish','')", []).unwrap();
        }
        for f in ["host.db", "host.db-wal", "host.db-shm"] {
            aaa_common::diagnostic::ignored(
                std::fs::copy(old.join(f), copy.join(f)),
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
        }
        assert!(
            !bytes_have(&copy, value).is_empty() && !bytes_have(&copy, gone).is_empty(),
            "정리 전 사본에는 값이 있다"
        );

        let db = Db::open(&copy.join("host.db")).unwrap();
        let rows = || -> Vec<(String, String)> {
            let c = db.conn.lock().unwrap();
            let mut st = c.prepare("SELECT action, detail FROM audit ORDER BY ts").unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
        };
        let got = rows();
        assert_eq!(got.len(), 3, "{got:?}");
        let v: serde_json::Value = serde_json::from_str(&got[0].1).unwrap();
        assert_eq!(v["env"], serde_json::json!(["GH_TOKEN", "AAA_THREAD"]), "{v}");
        assert_eq!(v["resolved_profile"]["env"], serde_json::json!(["ANTHROPIC_API_KEY"]), "{v}");
        assert_eq!((v["name"].as_str(), v["profile"].as_str()), (Some("demo/t-0001"), Some("api")), "나머지는 그대로");
        assert_eq!(got[1..], [("pane_run".into(), "p1 5 bytes".into()), ("web_publish".into(), String::new())]);
        assert_eq!(db.meta("audit_vacuum").unwrap(), None, "VACUUM을 끝냈다");
        // 열린 채로도 파일(DB·WAL)에 값이 없다: 시작 때 정리를 마쳤다
        assert_eq!(bytes_have(&copy, value), Vec::<String>::new());
        assert_eq!(bytes_have(&copy, gone), Vec::<String>::new());
        drop(db);
        let db = Db::open(&copy.join("host.db")).unwrap();
        let again: Vec<(String, String)> = {
            let c = db.conn.lock().unwrap();
            let mut st = c.prepare("SELECT action, detail FROM audit ORDER BY ts").unwrap();
            st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect()
        };
        assert_eq!(again, got, "다시 열어도 그대로");
        drop(db);
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&root), "정리 실패가 본 작업의 결과를 덮지 않게 한다");
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;

    /// 웹 알림함 「흐름 사건」(FRONT_API 1.77 `tail`): 가장 최근 N개를 오래된 것부터 보인다
    #[test]
    fn flow_tail_returns_the_latest_in_order() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        for n in 1..=5u64 {
            db.flow_insert(&aaa_proto::FlowEvent {
                project: "demo".into(),
                seq: 0,
                at_ms: n,
                boot_id: "b".into(),
                kind: format!("k{n}"),
                thread: None,
                repo: None,
                actor: None,
                role: None,
                profile: None,
                machine: None,
                evidence: None,
                detail: String::new(),
            })
            .unwrap();
        }
        let kinds = |v: Vec<aaa_proto::FlowEvent>| v.into_iter().map(|e| e.kind).collect::<Vec<_>>();
        assert_eq!(kinds(db.flow_tail(Some("demo"), 0, 2).unwrap()), ["k4", "k5"]);
        assert_eq!(kinds(db.flow_after(Some("demo"), 0, 2).unwrap()), ["k1", "k2"]);
        assert!(db.flow_tail(Some("other"), 0, 2).unwrap().is_empty());
    }
}
