//! post와 질문(ask) 기록.

use super::*;

const POST_LIST_LIMIT: i64 = 100;
const POST_LIST_BYTES: usize = 512 * 1024;

pub(crate) struct CorrectionPost {
    pub thread: String,
    pub created_ms: u64,
    pub title: String,
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
        tx.execute("INSERT INTO posts(post_id,project,sender,session,machine,content_sha,created_ms,result,data,quota,inbox_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)", params![post_id,row.project,row.sender,row.session,row.machine,row.content_sha,row.created_ms,serde_json::to_string(&row.result)?,env.map(serde_json::to_string).transpose()?,row.quota,inbox.map(|i| &i.id)])?;
        if let Some(i) = inbox {
            tx.execute(
                "INSERT INTO inbox(id,project,kind,subject,event,done,created_at,data) VALUES(?1,?2,?3,?4,?5,0,?6,?7)",
                params![i.id, i.project, i.item_kind, i.subject, i.event, i.created_at_ms, serde_json::to_string(i)?],
            )?;
        }
        tx.execute(
            "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,?3,?4)",
            params![
                row.created_ms,
                actor,
                if matches!(row.result, aaa_proto::PostResult::AutomationCompleted { .. }) {
                    "automation_complete"
                } else if env.is_some() {
                    "post_accept"
                } else {
                    "post_refuse"
                },
                detail
            ],
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

    /// 답 정정(t-2117): 기록의 지금 답 시각이 `base_at_ms`일 때만 바꾼다. 다른 기기가 먼저 바꿨으면 false다
    pub(crate) fn ask_swap(&self, project: &str, r: &aaa_proto::AskRecord, base_at_ms: u64) -> Result<bool> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let data: Option<String> = tx
            .query_row("SELECT data FROM asks WHERE project=?1 AND id=?2", params![project, r.id], |row| row.get(0))
            .optional()?;
        let Some(data) = data else { return Ok(false) };
        let now: aaa_proto::AskRecord = serde_json::from_str(&data)?;
        if now.answer.as_ref().map(|a| a.at_ms) != Some(base_at_ms) {
            return Ok(false);
        }
        let mut save = r.clone();
        save.correction = None;
        tx.execute(
            "UPDATE asks SET data=?3 WHERE project=?1 AND id=?2",
            params![project, r.id, serde_json::to_string(&save)?],
        )?;
        tx.commit()?;
        Ok(true)
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
        let mut st = c.prepare("SELECT data FROM asks WHERE project=?1 ORDER BY done_ms DESC, rowid DESC")?;
        let rows = st.query_map([project], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|d| Ok(serde_json::from_str(d)?)).collect()
    }

    /// 본문·선택지·첨부 주소 없이도 모든 질문 링크와 배지 중복 제거를 보존한다.
    pub(crate) fn ask_index(&self, project: &str) -> Result<Vec<aaa_proto::AskRecordSummary>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT json_object(
            'id',id,'kind',kind,'subject',subject,'done_ms',done_ms,
            'title',json_extract(data,'$.title'),'by',json_extract(data,'$.by'),
            'summary',json_extract(data,'$.summary'),'answer_summary',json_extract(data,'$.answer.summary'),'question_id',json_extract(data,'$.question_id'),
            'question_ids',json((SELECT json_group_array(json_extract(value,'$.id'))
                FROM json_each(asks.data,'$.items') WHERE json_extract(value,'$.id') IS NOT NULL)))
            FROM asks WHERE project=?1 ORDER BY done_ms DESC, rowid DESC")?;
        let rows = st.query_map([project], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows.iter().map(|d| Ok(serde_json::from_str(d)?)).collect()
    }

    /// 50개·256 KiB 경계. 같은 시각의 기록도 rowid로 잇고 삭제된 커서도 계속 읽는다.
    pub(crate) fn ask_page(
        &self,
        project: &str,
        before: Option<(i64, i64)>,
        id: Option<&str>,
    ) -> Result<(Vec<aaa_proto::AskRecord>, Option<String>)> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT data,done_ms,rowid FROM asks WHERE project=?1
            AND (?2 IS NULL OR id=?2) AND (?3 IS NULL OR (done_ms,rowid)<(?3,?4))
            ORDER BY done_ms DESC, rowid DESC LIMIT 51",
        )?;
        let mut rows = st.query(params![project, id, before.map(|c| c.0), before.map(|c| c.1)])?;
        let mut done = Vec::new();
        let mut bytes = 0;
        let mut cursor = None;
        while let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            if done.len() == 50 || (!done.is_empty() && bytes + data.len() > 256 * 1024) {
                return Ok((done, cursor));
            }
            bytes += data.len();
            done.push(serde_json::from_str(&data)?);
            cursor = Some(format!("{}:{}", row.get::<_, i64>(1)?, row.get::<_, i64>(2)?));
        }
        Ok((done, None))
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
        let rows = {
            let c = self.conn.lock().unwrap();
            // 옛 실행 파일도 봉투와 inbox ID를 같은 접수 시각으로 만들었다. 밀리초를 버려 파일명 형식으로 맞춘다.
            let mut st = c.prepare(
                "SELECT p.data, EXISTS(SELECT 1 FROM inbox i WHERE i.id=COALESCE(p.inbox_id,
                        strftime('%Y%m%dT%H%M%SZ',p.created_ms/1000,'unixepoch') || '-post-' || json_extract(p.data,'$.id'))
                    AND i.project=p.project AND i.done=1)
                 FROM posts p WHERE p.project=?1 AND p.data IS NOT NULL ORDER BY p.seq DESC LIMIT ?2",
            )?;
            st.query_map(params![project, POST_LIST_LIMIT], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut bytes = 0;
        let mut posts = Vec::new();
        for (s, handled) in rows {
            if bytes + s.len() > POST_LIST_BYTES {
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

    /// 정정에는 post 목록의 표시 범위 안에서 발신 thread·시각·제목만 필요하다.
    pub(crate) fn post_correction_titles(&self, project: &str) -> Result<Vec<CorrectionPost>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT length(CAST(data AS BLOB)),json_extract(data,'$.from.thread'),
                    json_extract(data,'$.created_ms'),json_extract(data,'$.title')
             FROM posts WHERE project=?1 AND data IS NOT NULL ORDER BY seq DESC LIMIT ?2",
        )?;
        let mut rows = st.query(params![project, POST_LIST_LIMIT])?;
        let mut bytes = 0;
        let mut posts = Vec::new();
        while let Some(row) = rows.next()? {
            bytes += row.get::<_, usize>(0)?;
            if bytes > POST_LIST_BYTES {
                break;
            }
            if let Some(thread) = row.get(1)? {
                posts.push(CorrectionPost { thread, created_ms: row.get(2)?, title: row.get(3)? });
            }
        }
        Ok(posts)
    }
}
