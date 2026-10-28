//! inbox의 저장 ID와 표시 번호를 분리한다. 완료·prune 뒤에도 원장을 남겨 번호를 재사용하지 않는다.
use super::*;
use aaa_proto::InboxItem;

impl Db {
    /// 기존 항목은 시각·저장 ID 순으로 한 번만 채운다. 모든 INSERT 경로의 번호는 같은 transaction에서 예약한다.
    pub(super) fn initialize_inbox_ids(c: &Connection) -> Result<()> {
        c.execute_batch(
            "CREATE TABLE IF NOT EXISTS inbox_ids(
               project TEXT NOT NULL, item_id TEXT NOT NULL, number INTEGER NOT NULL CHECK(number > 0),
               PRIMARY KEY(project,item_id), UNIQUE(project,number));
             CREATE TRIGGER IF NOT EXISTS inbox_assign_id AFTER INSERT ON inbox BEGIN
               INSERT INTO inbox_ids(project,item_id,number)
               SELECT NEW.project,NEW.id,COALESCE(MAX(number),0)+1 FROM inbox_ids WHERE project=NEW.project
               ON CONFLICT(project,item_id) DO NOTHING;
             END;",
        )?;
        let missing = c
            .prepare(
                "SELECT project,id FROM inbox WHERE NOT EXISTS(
               SELECT 1 FROM inbox_ids WHERE inbox_ids.project=inbox.project AND item_id=inbox.id)
             ORDER BY created_at,id",
            )?
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut insert = c.prepare(
            "INSERT INTO inbox_ids(project,item_id,number)
             SELECT ?1,?2,COALESCE(MAX(number),0)+1 FROM inbox_ids WHERE project=?1",
        )?;
        for (project, id) in missing {
            insert.execute(params![project, id])?;
        }
        Ok(())
    }

    /// Front·알림으로 나갈 사본에 표시 별칭만 채운다. 내부 중복 판정·파일명·원문은 저장 ID를 계속 쓴다.
    pub(crate) fn inbox_display_ids(&self, items: &mut [InboxItem]) -> Result<()> {
        let c = self.conn.lock().unwrap();
        let mut find = c.prepare("SELECT number FROM inbox_ids WHERE project=?1 AND item_id=?2")?;
        for item in items {
            let n: i64 = find.query_row(params![item.project, item.id], |r| r.get(0))?;
            item.short_id = Some(format!("i-{n:04}"));
        }
        Ok(())
    }

    /// 반드시 요청 프로젝트 안에서 푼다. 실제 옛 ID와 번호가 겹치면 임의로 하나를 고르지 않는다.
    pub(crate) fn inbox_long_id(&self, project: &str, id: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        let exact: Option<String> = c
            .query_row("SELECT id FROM inbox WHERE project=?1 AND id=?2", params![project, id], |r| r.get(0))
            .optional()?;
        let Some(number) = aaa_proto::short_id_number(id, 'i') else { return Ok(exact) };
        let alias: Option<String> = c
            .query_row("SELECT item_id FROM inbox_ids WHERE project=?1 AND number=?2", params![project, number], |r| {
                r.get(0)
            })
            .optional()?;
        anyhow::ensure!(
            exact.is_none() || alias.is_none() || exact == alias,
            "inbox 저장 ID와 번호 별칭이 겹친다. 목록의 현재 번호를 지정한다"
        );
        Ok(alias.or(exact))
    }
}
