//! 여러 머신이 공유하는 agy 구독 자리. 연결 끊김이나 Spawn 응답 대기로는 반환하지 않는다.

use anyhow::{Result, ensure};
use rusqlite::OptionalExtension;
use rusqlite::{Connection, params};

use super::Db;

/// 새 thread를 승인하는 transaction에서 호출한다. 같은 실행의 미결 자리만 다시 쓸 수 있다.
pub(crate) fn reserve(conn: &Connection, subscription: &str, machine: &str, run: &str) -> Result<()> {
    let changed = conn.execute(
        "INSERT INTO agy_slots(subscription,machine,run) VALUES(?1,?2,?3) ON CONFLICT DO NOTHING",
        params![subscription, machine, run],
    )?;
    if changed == 0 {
        let pending = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM agy_slots WHERE subscription=?1 AND machine=?2 AND run=?3 AND pane IS NULL)",
            params![subscription, machine, run],
            |r| r.get::<_, bool>(0),
        )?;
        ensure!(pending, "start_wait: 이 agy 구독에 실행 중이거나 기동 결과를 기다리는 작업이 있다");
    }
    Ok(())
}

impl Db {
    pub(crate) fn agy_slot_subscription(&self, machine: &str, pane: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT subscription FROM agy_slots WHERE machine=?1 AND pane=?2",
                params![machine, pane],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(crate) fn reserve_agy_slot(&self, subscription: &str, machine: &str, run: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        reserve(&tx, subscription, machine, run)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn bind_agy_slot(&self, machine: &str, run: &str, pane: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let changed = conn.execute(
            "UPDATE agy_slots SET pane=?3 WHERE machine=?1 AND run=?2 AND (pane IS NULL OR pane=?3)",
            params![machine, run, pane],
        )?;
        ensure!(changed == 1, "이 agy 실행의 구독 자리가 없거나 다른 pane에 묶여 있다");
        Ok(())
    }

    pub(crate) fn cancel_unspawned_agy_slot(&self, machine: &str, run: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM agy_slots WHERE machine=?1 AND run=?2 AND pane IS NULL", params![machine, run])?;
        Ok(())
    }

    pub(crate) fn release_agy_pane(&self, machine: &str, pane: &str) -> Result<()> {
        self.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM agy_slots WHERE machine=?1 AND pane=?2", params![machine, pane])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agy_subscription_has_one_durable_slot_across_projects_and_machines() {
        let path = std::env::temp_dir().join(format!("agy-slot-{}", aaa_common::util::new_id()));
        let db = Db::open(&path).unwrap();
        db.reserve_agy_slot("personal", "mac-a", "first/t-0001").unwrap();
        db.reserve_agy_slot("personal", "mac-a", "first/t-0001").unwrap();
        assert!(db.reserve_agy_slot("personal", "mac-b", "second/t-0001").is_err());
        assert!(db.reserve_agy_slot("personal", "mac-c", "third/t-0001").is_err());
        drop(db);
        let db = Db::open(&path).unwrap();
        assert!(db.reserve_agy_slot("personal", "mac-b", "second/t-0001").is_err());
        db.bind_agy_slot("mac-a", "first/t-0001", "pane-a").unwrap();
        assert!(db.bind_agy_slot("mac-a", "first/t-0001", "pane-b").is_err());
        db.cancel_unspawned_agy_slot("mac-a", "first/t-0001").unwrap();
        db.release_agy_pane("mac-b", "pane-a").unwrap();
        assert!(db.reserve_agy_slot("personal", "mac-b", "second/t-0001").is_err());
        db.release_agy_pane("mac-a", "pane-a").unwrap();
        db.reserve_agy_slot("personal", "mac-b", "second/t-0001").unwrap();
        db.cancel_unspawned_agy_slot("mac-b", "second/t-0001").unwrap();
        db.reserve_agy_slot("personal", "mac-c", "third/t-0001").unwrap();
        assert!(db.reserve_agy_slot("personal", "mac-a", "first/t-0002").is_err());
        drop(db);
        std::fs::remove_file(path).unwrap();
    }
}
