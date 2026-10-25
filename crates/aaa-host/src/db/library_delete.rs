//! 파일을 만지기 전에 링크 폐기를 확정하고, 파일 작업 뒤 성공한 사본의 원장만 함께 확정한다.
use super::*;
use aaa_proto::LibraryRef;

impl Db {
    pub(crate) fn library_delete_revoke(&self, copies: &[LibraryRef]) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut revoke =
                tx.prepare_cached("UPDATE library_links SET revoked=1 WHERE project=?1 AND thread=?2 AND revoked=0")?;
            for copy in copies {
                revoke.execute(params![copy.project, copy.name])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// 파일 I/O 동안에는 DB 잠금·transaction을 잡지 않는다.
    pub(crate) fn library_delete_finish(&self, copies: &[LibraryRef], completed: &[String], actor: &str) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut rows = tx.prepare_cached("DELETE FROM library_reports WHERE project=?1 AND name=?2")?;
            let mut archives = tx.prepare_cached("DELETE FROM library_archives WHERE project=?1 AND name=?2")?;
            let mut versions = tx.prepare_cached("UPDATE report_ids SET deleted=1 WHERE project=?1 AND id=?2")?;
            for copy in copies {
                rows.execute(params![copy.project, copy.name])?;
                archives.execute(params![copy.project, copy.name])?;
                versions.execute(params![copy.project, copy.name])?;
            }
            let mut audit =
                tx.prepare_cached("INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,'library_delete',?3)")?;
            let now = aaa_common::util::now_ms() as i64;
            for detail in completed {
                audit.execute(params![now, actor, detail])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revoke_and_finish_rollback_the_whole_batch_and_keep_other_projects() {
        let root = std::env::temp_dir().join(format!("aaa-delete-db-{}", aaa_common::util::new_id()));
        std::fs::create_dir(&root).unwrap();
        {
            let db = Db::open(&root.join("host.db")).unwrap();
            let copies: Vec<_> = ["r-0001", "r-0002"]
                .into_iter()
                .map(|name| LibraryRef { project: "demo".into(), name: name.into() })
                .collect();
            db.conn.lock().unwrap().execute_batch(
                "INSERT INTO library_reports(project,name,updated) VALUES('demo','r-0001',1),('demo','r-0002',1),('other','r-0001',1);
                INSERT INTO library_archives VALUES('demo','r-0001'),('demo','r-0002');
                INSERT INTO report_ids VALUES('demo',1,'r-0001','one','rev',1,0),('demo',2,'r-0002','two','rev',1,0),('other',1,'r-0001','one','rev',1,0);
                INSERT INTO library_links(id,project,thread,created,expires) VALUES('one','demo','r-0001',1,2),('two','demo','r-0002',1,2),('other','other','r-0001',1,2);
                CREATE TRIGGER fail_revoke BEFORE UPDATE ON library_links WHEN NEW.id='two' BEGIN SELECT RAISE(ABORT,'revoke fixture'); END;"
            ).unwrap();
            let count = |sql: &str| db.conn.lock().unwrap().query_row(sql, [], |r| r.get::<_, i64>(0)).unwrap();
            assert!(db.library_delete_revoke(&copies).unwrap_err().to_string().contains("revoke fixture"));
            assert_eq!(count("SELECT count(*) FROM library_links WHERE revoked=1"), 0);
            db.conn.lock().unwrap().execute_batch("DROP TRIGGER fail_revoke").unwrap();
            db.library_delete_revoke(&copies).unwrap();
            assert_eq!(count("SELECT count(*) FROM library_links WHERE revoked=1"), 2);
            assert_eq!(count("SELECT count(*) FROM library_reports"), 3, "파일 작업 전에는 남은 원장을 보존한다");
            db.conn.lock().unwrap().execute_batch(
                "CREATE TRIGGER fail_finish BEFORE INSERT ON audit WHEN NEW.detail='demo/two' BEGIN SELECT RAISE(ABORT,'finish fixture'); END;"
            ).unwrap();
            let completed = vec!["demo/one".into(), "demo/two".into()];
            assert!(
                db.library_delete_finish(&copies, &completed, "test")
                    .unwrap_err()
                    .to_string()
                    .contains("finish fixture")
            );
            assert_eq!(count("SELECT count(*) FROM library_reports"), 3);
            assert_eq!(count("SELECT count(*) FROM library_archives"), 2);
            assert_eq!(count("SELECT count(*) FROM report_ids WHERE deleted=1"), 0);
            assert_eq!(count("SELECT count(*) FROM audit WHERE action='library_delete'"), 0);
            db.conn.lock().unwrap().execute_batch("DROP TRIGGER fail_finish").unwrap();
            db.library_delete_finish(&copies, &completed, "test").unwrap();
            assert_eq!(count("SELECT count(*) FROM library_reports"), 1);
            assert_eq!(count("SELECT count(*) FROM library_archives"), 0);
            assert_eq!(count("SELECT count(*) FROM report_ids"), 3, "번호는 재사용하지 않는다");
            assert_eq!(count("SELECT count(*) FROM report_ids WHERE deleted=1"), 2);
            assert_eq!(count("SELECT count(*) FROM library_links WHERE project='other' AND revoked=0"), 1);
            assert_eq!(count("SELECT count(*) FROM audit WHERE action='library_delete' AND actor='test'"), 2);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
