//! host SQLite(WAL). 등록된 머신·웹 장치·공개 상태·감사 로그. 쓰기는 짧은 잠금 안에서만 한다.

mod library;
pub(crate) use library::*;
mod machines;
mod schema;
pub use machines::*;
mod devices;
pub use devices::*;
mod audit;
mod inbox;
mod messages;
mod task_log;
mod threads;
pub(crate) use audit::*;
mod posts;
mod projects;
pub(crate) use posts::*;
use std::path::Path;
use std::sync::{LockResult, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};

mod account_rename;
pub(crate) mod agy_slot;
mod auto_merge;
mod automation_ids;
pub(crate) mod automation_runs;
mod design_ids;
mod inbox_ids;
#[cfg(test)]
mod inbox_tests;
mod library_catalog;
mod library_delete;
mod machine_rename;
mod question_guard;
mod question_ids;
mod record_search;
mod report_ids;
#[cfg(test)]
mod short_ids_tests;
mod task_ids;
mod task_queue;
mod web_publication;
mod workspace_metrics;

pub(crate) use design_ids::{DesignIdDoc, design_id_text};
pub(crate) use library_catalog::LibraryCatalog;

pub const SCHEMA_VERSION: i64 = 1;

pub(crate) struct StartAdmission {
    pub machine: String,
    pub holds: Vec<String>,
    pub rule: String,
    pub max_running: Option<u32>,
    pub max_auto_threads: u32,
    /// 펼친 규칙(`repos = "design"`)의 저장소 이름: 규칙 상한을 이 저장소 thread만으로 센다
    pub repo: Option<String>,
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
            let same_repo = crate::auto_start::same_repo(&rec, self.repo.as_deref());
            if let Some(name) = rec.automation {
                auto_count += 1;
                rule_count += usize::from(name == self.rule && same_repo);
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
    conn: TimedConnection,
    /// HTTP 공개 주소 판정은 긴 쓰기·이력 조회와 독립된 WAL 읽기 연결을 쓴다.
    origin_reader: Option<TimedConnection>,
}

struct TimedConnection(Mutex<Connection>);

impl TimedConnection {
    #[track_caller]
    fn lock(&self) -> LockResult<ConnectionGuard<'_>> {
        let caller = std::panic::Location::caller();
        let guard = self.0.lock();
        let acquired = Instant::now();
        let wrap = |guard| ConnectionGuard { guard: Some(guard), acquired, caller };
        guard.map(wrap).map_err(|error| PoisonError::new(wrap(error.into_inner())))
    }
}

#[derive(Debug)]
pub(crate) struct ConnectionGuard<'a> {
    guard: Option<MutexGuard<'a, Connection>>,
    acquired: Instant,
    caller: &'static std::panic::Location<'static>,
}

impl std::ops::Deref for ConnectionGuard<'_> {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.guard.as_deref().unwrap()
    }
}

impl std::ops::DerefMut for ConnectionGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.guard.as_deref_mut().unwrap()
    }
}

impl Drop for ConnectionGuard<'_> {
    fn drop(&mut self) {
        let held = self.acquired.elapsed();
        drop(self.guard.take());
        if held >= Duration::from_secs(1) {
            tracing::warn!(callsite = %self.caller, held_ms = held.as_millis() as u64, "host DB 잠금 장기 점유");
        }
    }
}

impl Db {
    #[cfg(test)]
    pub(crate) fn conn_for_test(&self) -> ConnectionGuard<'_> {
        self.conn.lock().unwrap()
    }
    /// 발신자의 32자리 ID와 화면의 p-번호를 모두 받는다. 중복 전송 판정은 원래 post_row만 쓴다.
    pub(crate) fn post_by_ref(&self, id: &str) -> Result<Option<PostRow>> {
        if !id.starts_with("p-") {
            return self.post_row(id);
        }
        let canonical = aaa_proto::canonical_short_id(id, 'p');
        let id = canonical.as_deref().unwrap_or(id);
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
}

/// 시험 도우미: 실패하지 않는 `make`로 thread 기록을 넣는다. production은 `try_thread_insert_admitted`·
/// `thread_insert_transaction`만 부른다.
#[cfg(test)]
impl Db {
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
}

#[cfg(test)]
mod schema_tests;

#[cfg(test)]
mod input_tests;

#[cfg(test)]
mod rollout_tests;

#[cfg(test)]
mod audit_env_tests;

#[cfg(test)]
mod flow_tests;
