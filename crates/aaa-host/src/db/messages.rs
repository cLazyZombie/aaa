//! thread에 보낸 말과 답 전달 상태.

use super::*;

impl Db {
    pub(crate) fn advance_waiting_answer(&self, id: &str, state: &str) -> Result<bool> {
        let c = self.conn.lock().unwrap();
        Ok(c.execute("UPDATE messages SET state=?2 WHERE id=?1 AND state='answer_waiting'", params![id, state])? == 1)
    }

    pub(crate) fn pending_answers(&self, project: &str) -> Result<Vec<String>> {
        let c = self.conn.lock().unwrap();
        let mut st =
            c.prepare("SELECT data FROM messages WHERE project=?1 AND state='answer_waiting' ORDER BY created_at,id")?;
        let rows = st.query_map([project], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub(crate) fn revise_waiting_answer_notice(
        &self,
        project: &str,
        thread: &str,
        message: &str,
        event: &str,
        summary: &str,
    ) -> Result<Option<aaa_proto::InboxItem>> {
        let c = self.conn.lock().unwrap();
        let data: Option<String> = c
            .query_row(
                "UPDATE inbox SET event=?5, data=json_set(data,'$.event',?5,'$.summary',?6)
                 WHERE project=?1 AND subject=?2 AND kind='thread-state' AND event=?3
                   AND instr(json_extract(data,'$.summary'),?4)=1 RETURNING data",
                params![
                    project,
                    thread,
                    crate::answer_delivery::WAITING_EVENT,
                    format!("{thread}: {message} · "),
                    event,
                    summary
                ],
                |r| r.get(0),
            )
            .optional()?;
        Ok(data.map(|json| serde_json::from_str(&json)).transpose()?)
    }

    /// 앞선 같은 본문의 늦은 기록이 이번 영수증으로 보일 수 있다. 미확인 전송은 즉시 판정도 막는다.
    pub(crate) fn previous_message(
        &self,
        m: &aaa_proto::MessageView,
    ) -> Result<crate::prompt_receipt::PreviousMessage> {
        use crate::prompt_receipt::PreviousMessage;
        let hash = crate::prompt_receipt::receipt_hash(&m.text);
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare(
            "SELECT data FROM messages WHERE project=?1 AND thread=?2 AND state IN ('unknown','accepted') AND id<>?3",
        )?;
        let mut rows = st.query(params![m.project, m.thread, m.id])?;
        let mut found = PreviousMessage::None;
        while let Some(row) = rows.next()? {
            let previous: aaa_proto::MessageView = serde_json::from_str(&row.get::<_, String>(0)?)?;
            if previous.input != Some(aaa_proto::MessageInput::NotWritten)
                && crate::prompt_receipt::receipt_hash(&previous.text) == hash
            {
                if previous.state == aaa_proto::DeliveryState::Unknown {
                    return Ok(PreviousMessage::Unconfirmed);
                }
                found = PreviousMessage::Accepted;
            }
        }
        Ok(found)
    }

    /// 수락 상태와 기존 알림을 함께 바꾼다. 알림의 ID·읽음·완료 상태는 DB의 현재 값을 유지한다.
    pub(crate) fn message_accept_late(
        &self,
        project: &str,
        thread: &str,
        message: &str,
    ) -> Result<Option<aaa_proto::InboxItem>> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        let n = tx.execute(
            "UPDATE messages SET state='accepted',data=json_set(data,'$.state','accepted')
             WHERE project=?1 AND thread=?2 AND id=?3 AND state='unknown' AND json_extract(data,'$.input')='submitted'",
            params![project, thread, message],
        )?;
        if n == 0 {
            return Ok(None);
        }
        let data: Option<String> = tx
            .query_row(
                "UPDATE inbox SET event='message accepted late',
                   data=json_set(data,'$.event','message accepted late','$.summary',?4)
                 WHERE project=?1 AND subject=?2 AND kind='thread-state' AND event='message not delivered'
                   AND instr(json_extract(data,'$.summary'),?3)=1 RETURNING data",
                params![
                    project,
                    thread,
                    format!("{thread}: {message} · "),
                    format!("{thread}: {message} · 늦게 수락 확인"),
                ],
                |r| r.get(0),
            )
            .optional()?;
        let item = data.map(|data| serde_json::from_str(&data)).transpose()?;
        tx.commit()?;
        Ok(item)
    }
}
