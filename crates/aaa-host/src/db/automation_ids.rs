//! 자동화 이름은 프로젝트 안의 논리적 식별자다. 삭제해도 번호 예약을 남겨 복원·형식 변환 때 유지한다.
use super::*;

impl Db {
    pub(crate) fn automation_short_id(&self, project: &str, name: &str) -> Result<String> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO automation_ids(project,name,number)
             SELECT ?1,?2,COALESCE(MAX(number),0)+1 FROM automation_ids WHERE project=?1
             ON CONFLICT(project,name) DO NOTHING",
            params![project, name],
        )?;
        let n: i64 = tx.query_row(
            "SELECT number FROM automation_ids WHERE project=?1 AND name=?2",
            params![project, name],
            |r| r.get(0),
        )?;
        tx.commit()?;
        Ok(format!("a-{n:04}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    #[test]
    fn automation_ids_are_unique_durable_and_project_local_under_concurrent_reads() {
        let path = std::env::temp_dir().join(format!("aaa-aids-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&path).unwrap();
        let db = Arc::new(Db::open(&path.join("host.sqlite")).unwrap());
        let gate = Arc::new(Barrier::new(8));
        let jobs: Vec<_> = (0..8)
            .map(|_| {
                let db = db.clone();
                let gate = gate.clone();
                std::thread::spawn(move || {
                    gate.wait();
                    let shared = db.automation_short_id("demo", "shared").unwrap();
                    let own = db.automation_short_id("demo", &aaa_common::util::new_id()).unwrap();
                    (shared, own)
                })
            })
            .collect();
        let mut ids = std::collections::HashSet::new();
        for job in jobs {
            let (shared, own) = job.join().unwrap();
            assert_eq!(shared, "a-0001");
            assert!(ids.insert(own));
        }
        assert_eq!(db.automation_short_id("other", "shared").unwrap(), "a-0001");
        drop(db);
        let db = Db::open(&path.join("host.sqlite")).unwrap();
        assert_eq!(db.automation_short_id("demo", "shared").unwrap(), "a-0001");
        assert_eq!(db.automation_short_id("demo", "next").unwrap(), "a-0010");
        db.project_forget("other").unwrap();
        assert_eq!(db.project_export("other").unwrap()["automation_ids"], serde_json::json!([]));
        assert_eq!(db.automation_short_id("demo", "next").unwrap(), "a-0010");
        drop(db);
        std::fs::remove_dir_all(path).unwrap();
    }
}
