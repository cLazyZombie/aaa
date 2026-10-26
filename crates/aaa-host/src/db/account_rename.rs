//! 기본·풀·대기 시작·thread·관측 기록은 한 트랜잭션에서 이전한다.
use super::*;

impl Db {
    pub(crate) fn account_rename_images(&self) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT value FROM meta WHERE key LIKE 'image-job:%'")?;
        Ok(st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn account_rename_bindings(&self) -> Result<Vec<(String, String)>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT key,value FROM meta WHERE key LIKE 'account-run:%'")?;
        Ok(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn account_rename(&self, from: &str, to: &str, actor: &str, journal: &str) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        for (table, column) in [
            ("threads", "data"),
            ("task_queue", "data"),
            ("task_archive", "data"),
            ("automation_runs", "data"),
            ("inbox", "data"),
        ] {
            let rows = {
                let mut st = tx.prepare(&format!("SELECT rowid,{column} FROM {table}"))?;
                st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for (row, text) in rows {
                let mut value: serde_json::Value = serde_json::from_str(&text)?;
                let before = value.clone();
                crate::account_rename::json(&mut value, from, to);
                if table == "inbox" && value["item_kind"] == "account" && value["subject"] == from {
                    value["subject"] = to.into();
                    // 안내 문구는 과거 명령을 남기지 않고 새 별칭으로 다시 만든다.
                    value["summary"] = format!("확인 필요: 계정 {to}. 웹 설정 → 계정에서 상태를 확인한다").into();
                }
                if value != before {
                    tx.execute(
                        &format!("UPDATE {table} SET {column}=?2 WHERE rowid=?1"),
                        params![row, value.to_string()],
                    )?;
                }
            }
        }
        for prefix in ["account-meta:", "account-quota-v1/"] {
            tx.execute("DELETE FROM meta WHERE key=?1", [format!("{prefix}{to}")])?;
        }
        let rows = {
            let mut st = tx.prepare("SELECT key,value FROM meta WHERE key != ?1")?;
            st.query_map([crate::account_rename::JOURNAL], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (key, text) in rows {
            let new_key = ["account-meta:", "account-quota-v1/"]
                .into_iter()
                .find(|prefix| key == format!("{prefix}{from}"))
                .map(|prefix| format!("{prefix}{to}"))
                .unwrap_or_else(|| key.clone());
            let mut next = text.clone();
            if key.starts_with("default-account:") && text == from {
                next = to.into();
            } else if key == "account-pool-v1"
                || ["account-run:", "account-job:", "image-job:"].iter().any(|p| key.starts_with(p))
            {
                let mut value: serde_json::Value = serde_json::from_str(&text)?;
                crate::account_rename::json(&mut value, from, to);
                if key == "account-pool-v1" {
                    // 한도 그룹은 신원이다. 이름이 옛 별칭과 같아도 옮기지 않는다.
                    value["revision"] = aaa_common::util::new_id().into();
                }
                next = value.to_string();
            }
            if new_key != key {
                // 삭제 뒤 같은 별칭으로 등록했던 관측/metadata가 있으면 현재 계정 기록으로 교체한다.
                tx.execute(
                    "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![new_key, next],
                )?;
                tx.execute("DELETE FROM meta WHERE key=?1", [key])?;
            } else if next != text {
                tx.execute("UPDATE meta SET value=?2 WHERE key=?1", params![key, next])?;
            }
        }
        tx.execute("UPDATE inbox SET subject=?2 WHERE kind='account' AND subject=?1", params![from, to])?;
        // 과거 감사 원문은 증거로 보존하고, 이전 연결을 별도 사건으로 남긴다. 인증·메일은 없다.
        tx.execute(
            "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,'account_rename',?3)",
            params![aaa_common::util::now_ms() as i64, actor, serde_json::json!({"from":from,"to":to}).to_string()],
        )?;
        tx.execute("UPDATE meta SET value=?2 WHERE key=?1", params![crate::account_rename::JOURNAL, journal])?;
        tx.commit()?;
        Ok(())
    }
}
