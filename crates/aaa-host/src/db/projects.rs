//! 프로젝트 단위 내보내기·지우기·이름 바꾸기.

use super::*;

/// project 열이 있는 표(옛 db에 남은 `merge_queue`는 읽지 않아 옮기지 않는다, 새 db에는 없다 — §8.135)
pub(super) const PROJECT_TABLES: [&str; 26] = [
    "work_records",
    "work_record_sources",
    "threads",
    "repo_workspaces",
    "repo_metrics",
    "flow_events",
    "messages",
    "inbox",
    "inbox_ids",
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
    "design_ids",
    "automation_ids",
    "automation_runs",
    "task_queue",
    "task_queue_log",
    "task_archive",
    "asks",
];

/// 그 프로젝트의 meta: key가 `<종류>:<프로젝트>` 또는 `<종류>:<프로젝트>:…`(routine·PR·nudge·설정 오류·상태 등)
pub(super) fn project_meta(c: &Connection, project: &str) -> Result<Vec<(String, String)>> {
    let mut st = c.prepare("SELECT key, value FROM meta WHERE key LIKE '%:' || ?1 OR key LIKE '%:' || ?1 || ':%'")?;
    let rows = st
        .query_map([project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    // LIKE의 `_`·`%`는 글자 그대로가 아니다: 둘째 마디가 정확히 그 이름인 것만
    Ok(rows.into_iter().filter(|(k, _)| k.split(':').nth(1) == Some(project) && !k.starts_with(':')).collect())
}

impl Db {
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
        tx.execute("UPDATE inbox SET data=json_set(data,'$.project',?1) WHERE project=?1", [to])?;
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
