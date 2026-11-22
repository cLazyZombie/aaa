//! DB 열기와 schema 만들기·올리기.

use super::*;

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // DDL마다 별도 내구 쓰기를 하지 않는다. 실패한 이행도 일부만 남기지 않는다.
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        Self::initialize_schema(&tx)?;
        Self::initialize_inbox_ids(&tx)?;
        tx.commit()?;
        scrub_audit_env(&mut conn)?;
        // 이름 없는 메모리 DB는 두 연결이 데이터를 공유하지 않는다.
        let origin_reader = if conn.path().is_some_and(|name| !name.is_empty()) {
            Some(Mutex::new(crate::backup::readonly(path)?))
        } else {
            None
        };
        Ok(Db { conn: Mutex::new(conn), origin_reader })
    }

    pub(super) fn initialize_schema(conn: &Connection) -> Result<()> {
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
             CREATE INDEX IF NOT EXISTS report_ids_source ON report_ids(project,source,ready,deleted,seq DESC);
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
             CREATE TABLE IF NOT EXISTS agy_slots(
               subscription TEXT PRIMARY KEY, machine TEXT NOT NULL, run TEXT NOT NULL UNIQUE, pane TEXT UNIQUE);
             CREATE TABLE IF NOT EXISTS flow_events(
               project TEXT NOT NULL, seq INTEGER NOT NULL, at_ms INTEGER NOT NULL, boot_id TEXT NOT NULL,
               kind TEXT NOT NULL, thread TEXT, role TEXT, profile TEXT, machine TEXT, evidence TEXT,
               detail TEXT NOT NULL, PRIMARY KEY(project, seq));
             CREATE TABLE IF NOT EXISTS questions(
               id TEXT PRIMARY KEY, pane TEXT NOT NULL, state TEXT NOT NULL, data TEXT NOT NULL, created_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS messages(
               id TEXT PRIMARY KEY, project TEXT NOT NULL, thread TEXT NOT NULL, state TEXT NOT NULL, data TEXT NOT NULL,
               created_at INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS messages_answer_waiting ON messages(project,created_at,id) WHERE state='answer_waiting';
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
             DROP TABLE IF EXISTS idea_ids;
             CREATE TABLE IF NOT EXISTS design_ids(number INTEGER PRIMARY KEY AUTOINCREMENT, project TEXT NOT NULL, repo TEXT NOT NULL, path TEXT NOT NULL, title TEXT NOT NULL DEFAULT '', present INTEGER NOT NULL DEFAULT 0, marked INTEGER NOT NULL DEFAULT 0);
             CREATE INDEX IF NOT EXISTS design_ids_path ON design_ids(project,repo,path);
             CREATE TABLE IF NOT EXISTS task_queue(project TEXT NOT NULL, task_id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(project,task_id));
             CREATE TABLE IF NOT EXISTS task_queue_log(id INTEGER PRIMARY KEY, project TEXT NOT NULL, actor TEXT NOT NULL, at_ms INTEGER NOT NULL, before TEXT NOT NULL, after TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS task_archive(project TEXT NOT NULL, task_id TEXT NOT NULL, data TEXT NOT NULL, PRIMARY KEY(project,task_id));
             CREATE TABLE IF NOT EXISTS task_auto_merge(project TEXT NOT NULL, task_id TEXT NOT NULL, on_ INTEGER NOT NULL, actor TEXT NOT NULL, user_request TEXT, at_ms INTEGER NOT NULL, PRIMARY KEY(project,task_id));
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
        record_search::init(conn)?;
        // v1에는 원격 신원·main 근거가 없다. 통계 전용 자료만 원자적으로 초기화한다.
        let metrics_version: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key='workspace-metrics-schema'", [], |r| r.get(0))
            .optional()?;
        if metrics_version.as_deref() != Some("2") {
            conn.execute_batch("DELETE FROM repo_metrics; DELETE FROM repo_workspaces;
                DELETE FROM meta WHERE key LIKE 'workspace-collection:%';
                INSERT INTO meta(key,value) VALUES('workspace-metrics-schema','2') ON CONFLICT(key) DO UPDATE SET value='2';")?;
        }
        Ok(())
    }
}
