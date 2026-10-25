//! 머신 ID가 들어 있는 기록은 그대로 두고 이름을 저장한 칸만 같은 트랜잭션에서 옮긴다.
use super::*;
use anyhow::Context;
use std::collections::BTreeMap;

impl Db {
    pub(crate) fn machine_by_id(&self, id: &str) -> Result<Option<MachineRow>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row(
            &format!("SELECT {} FROM machines WHERE id=?1 AND revoked=0", Self::MACHINE_COLS),
            [id],
            Self::machine_from,
        )
        .optional()?)
    }
    pub(crate) fn machine_name_exists(&self, name: &str) -> Result<bool> {
        Ok(self
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT EXISTS(SELECT 1 FROM machines WHERE name=?1)", [name], |r| r.get(0))?)
    }
    pub(crate) fn machine_rename(
        &self,
        id: &str,
        from: &str,
        to: &str,
        actor: &str,
        hashes: &BTreeMap<String, String>,
    ) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        anyhow::ensure!(
            tx.execute(
                "UPDATE machines SET name=?3 WHERE id=?1 AND name=?2 AND revoked=0 AND local=0",
                params![id, from, to]
            )? == 1,
            "머신 이름이나 등록이 그 사이 바뀌었다"
        );
        tx.execute("UPDATE install_grants SET name=?2 WHERE machine=?1", params![id, to])?;
        // 원문 로그(task_log·audit·input_events·task_queue_log)는 당시 증거로 보존한다.
        for (table, column) in [
            ("threads", "data"),
            ("questions", "data"),
            ("messages", "data"),
            ("inbox", "data"),
            ("task_queue", "data"),
            ("task_archive", "data"),
            ("automation_runs", "data"),
            ("repo_metrics", "data"),
            ("workspaces", "layout"),
        ] {
            let rows = {
                let mut st = tx.prepare(&format!("SELECT rowid,{column} FROM {table}"))?;
                st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (row, text) in rows {
                let mut value: serde_json::Value =
                    serde_json::from_str(&text).with_context(|| format!("{table}.{column} row={row}"))?;
                let before = value.clone();
                crate::machine_rename::json(&mut value, from, to, hashes);
                if table == "task_archive"
                    && let Some(text) = value.get("body").and_then(serde_json::Value::as_str)
                {
                    let after = crate::machine_rename::tasks(text, from, to);
                    let old = crate::tasks::parse(text);
                    let new = crate::tasks::parse(&after);
                    let hashes =
                        old.iter().zip(&new).map(|(a, b)| (a.block_sha256.clone(), b.block_sha256.clone())).collect();
                    crate::machine_rename::json(&mut value, from, to, &hashes);
                    value["body"] = after.into();
                }
                if value != before {
                    tx.execute(
                        &format!("UPDATE {table} SET {column}=?2 WHERE rowid=?1"),
                        params![row, value.to_string()],
                    )?;
                }
            }
        }
        // meta에는 ID 기반 배포/rollout·workspace 허용·자동화 승인 범위와 이름 기반 pending 입력이 함께 있다.
        let rows = {
            let mut st = tx.prepare("SELECT key,value FROM meta WHERE key != 'machine-rename:pending'")?;
            st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (key, text) in rows {
            let mut next = if key.starts_with("routine_ok:") {
                hashes.get(&text).cloned().unwrap_or_else(|| text.clone())
            } else {
                text.clone()
            };
            if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) {
                let before = value.clone();
                crate::machine_rename::json(&mut value, from, to, hashes);
                if before != value {
                    next = value.to_string()
                }
            }
            let new_key = if key.starts_with("workspace-collection:") && key.ends_with(&format!(":{from}")) {
                format!("{}:{to}", &key[..key.len() - from.len() - 1])
            } else {
                key.clone()
            };
            if new_key != key {
                tx.execute("INSERT INTO meta(key,value) VALUES(?1,?2)", params![new_key, next])?;
                tx.execute("DELETE FROM meta WHERE key=?1", [key])?;
            } else if next != text {
                tx.execute("UPDATE meta SET value=?2 WHERE key=?1", params![key, next])?;
            }
        }
        tx.execute(
            "UPDATE repo_metrics SET id=?2 || substr(id,length(?1)+1) WHERE kind='run' AND substr(id,1,length(?1))=?1",
            params![format!("{from}:"), format!("{to}:")],
        )?;
        tx.execute("UPDATE flow_events SET machine=?2 WHERE machine=?1", params![from, to])?;
        tx.execute("UPDATE posts SET machine=?2 WHERE machine=?1", params![from, to])?;
        tx.execute(
            "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,'machine_rename',?3)",
            params![
                aaa_common::util::now_ms() as i64,
                actor,
                serde_json::json!({"machine_id":id,"from":from,"to":to}).to_string()
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
}
