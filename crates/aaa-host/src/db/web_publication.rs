//! 주소를 바꾸되 원본 봉투·재전송 판정·처리 상태를 보존한다. 주소·구독·전환 완료는 한 transaction이다.
use super::*;
use crate::web_publication::PortChange;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// URL origin 경계만 옮긴다. 비슷한 도메인·다른 포트·userinfo·경로 속 문자열은 주소로 간주하지 않는다.
struct UrlMove<'a> {
    pattern: regex::Regex,
    new: &'a str,
}
impl<'a> UrlMove<'a> {
    fn new(old: &str, new: &'a str) -> Self {
        let mut pattern = regex::escape(old);
        if !old.rsplit_once(':').is_some_and(|(_, p)| {
            p.parse::<u16>()
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok()
        }) {
            pattern.push_str("(?::443)?");
        }
        Self {
            pattern: regex::RegexBuilder::new(&pattern).case_insensitive(true).build().expect("escaped origin"),
            new,
        }
    }
    fn text(&self, text: &str) -> String {
        let mut result = String::new();
        let mut offset = 0;
        for found in self.pattern.find_iter(text) {
            let before = text[..found.start()].chars().next_back();
            let after = text[found.end()..].chars().next();
            if before.is_some_and(|c| !c.is_whitespace() && !"([<\"'`:「『".contains(c))
                || after.is_some_and(|c| !c.is_whitespace() && !"/?#)]>\"'`,;!」』".contains(c))
            {
                continue;
            }
            result.push_str(&text[offset..found.start()]);
            result.push_str(self.new);
            offset = found.end();
        }
        result.push_str(&text[offset..]);
        result
    }
}
#[cfg(test)]
fn move_urls(text: &str, old: &str, new: &str) -> String {
    UrlMove::new(old, new).text(text)
}

fn rewrite(value: &mut Value, links: &UrlMove<'_>, bodies: &HashMap<String, String>) {
    match value {
        Value::String(text) => {
            *text = bodies.get(text).cloned().unwrap_or_else(|| links.text(text));
        }
        Value::Array(values) => values.iter_mut().for_each(|v| rewrite(v, links, bodies)),
        Value::Object(values) => values.values_mut().for_each(|v| rewrite(v, links, bodies)),
        _ => {}
    }
}

impl Db {
    pub(crate) fn observe_publication(&self, origin: Option<&str>, dns: Option<&str>) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM meta WHERE key='publish_origin'", [])?;
        for (key, value) in [("publish_origin", origin), ("publish_dns", dns), ("publish_observed", Some("1"))] {
            if let Some(value) = value {
                tx.execute(
                    "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![key, value],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn finish_port_change(&self, change: &PortChange, actor: &str) -> Result<(usize, usize)> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let links = UrlMove::new(&change.old_origin, &change.new_origin);
        let mut bodies = HashMap::new();
        let mut moved = 0;
        // 원본 봉투와 content_sha는 재전송 검증의 근거다. 원본은 별도 표에 보존하고 content_sha는 바꾸지 않는다.
        for table in ["posts", "inbox", "asks"] {
            let rows = tx
                .prepare(&format!("SELECT rowid,data FROM {table} WHERE data IS NOT NULL"))?
                .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for (rowid, original) in rows {
                let updated = if table == "posts" {
                    let mut post: aaa_proto::PostEnvelope = serde_json::from_str(&original)?;
                    let old_hash = post.body_sha256.clone();
                    let old_body = crate::post::body(&post);
                    post.input.title = links.text(&post.input.title);
                    post.input.body = links.text(&post.input.body);
                    for reference in &mut post.input.refs {
                        *reference = links.text(reference);
                    }
                    post.body_sha256 = hex::encode(Sha256::digest(post.input.body.as_bytes()));
                    if old_hash != post.body_sha256 {
                        // 주소 이전은 같은 질문이다. q-ID·부분 답을 보존하고 새 본문 hash에 연결한다.
                        tx.execute("UPDATE question_ids SET revision=?1 WHERE project=?2 AND kind='post' AND revision=?3 AND subject IN (SELECT id FROM inbox WHERE project=?2 AND id LIKE ?4)",
                            params![post.body_sha256, post.project, old_hash, format!("%-post-{}",post.id)])?;
                    }
                    let new_body = crate::post::body(&post);
                    if old_body != new_body {
                        bodies.insert(old_body, new_body);
                    }
                    serde_json::to_value(post)?
                } else {
                    let mut data: Value = serde_json::from_str(&original)?;
                    rewrite(&mut data, &links, &bodies);
                    data
                };
                if updated == serde_json::from_str::<Value>(&original)? {
                    continue;
                }
                tx.execute(
                    "INSERT OR IGNORE INTO web_link_originals(change_id,origin,kind,row_id,data) VALUES(?1,?2,?3,?4,?5)",
                    params![change.id, change.old_origin, table, rowid, original],
                )?;
                tx.execute(&format!("UPDATE {table} SET data=?1 WHERE rowid=?2"), params![updated.to_string(), rowid])?;
                moved += 1;
            }
        }
        // 구판은 origin을 저장하지 않았다. 그 구독도 무효화하되 알려진 loopback·다른 origin은 보존한다.
        let subscriptions =
            tx.execute("DELETE FROM push_subs WHERE origin IS NULL OR origin=?1", [&change.old_origin])?;
        for (key, value) in [
            ("publish_origin", change.new_origin.clone()),
            ("publish_port", change.new_port.to_string()),
            ("publish_selected_port", change.new_port.to_string()),
            ("publish_observed", "1".into()),
        ] {
            tx.execute(
                "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                params![key, value],
            )?;
        }
        tx.execute("DELETE FROM meta WHERE key='publish_change'", [])?;
        tx.execute(
            "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,'web_publish_port',?3)",
            params![
                aaa_common::util::now_ms() as i64,
                actor,
                format!("{}→{}; records={moved}; subscriptions={subscriptions}", change.old_port, change.new_port)
            ],
        )?;
        tx.commit()?;
        Ok((moved, subscriptions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const OLD: &str = "https://mac-a.example.ts.net:8443";
    const NEW: &str = "https://mac-a.example.ts.net:7443";
    #[test]
    fn moves_only_complete_origins_and_preserves_path_query_fragment_and_other_urls() {
        let input = format!(
            "[보고]({OLD}/v/cap/a%20b?x=1#그림), `{OLD}/#question=q-0001`\n{OLD}\n{OLD}:90/ {OLD}.evil/ {OLD}@evil/ https://other.example.ts.net:8443/ /mirror/{OLD}/"
        );
        let expected = format!(
            "[보고]({NEW}/v/cap/a%20b?x=1#그림), `{NEW}/#question=q-0001`\n{NEW}\n{OLD}:90/ {OLD}.evil/ {OLD}@evil/ https://other.example.ts.net:8443/ /mirror/{OLD}/"
        );
        assert_eq!(move_urls(&input, OLD, NEW), expected);
        assert_eq!(
            move_urls(
                "HTTPS://MAC-A.EXAMPLE.TS.NET:443/#x https://mac-a.example.ts.net:4430/",
                "https://mac-a.example.ts.net",
                NEW
            ),
            format!("{NEW}/#x https://mac-a.example.ts.net:4430/")
        );
    }

    #[test]
    fn database_failure_rolls_back_links_originals_subscriptions_and_publication_together() {
        let db = Db::open(Path::new(":memory:")).unwrap();
        let change = PortChange {
            id: "fixture-change".into(),
            old_port: 8443,
            new_port: 7443,
            old_origin: OLD.into(),
            new_origin: NEW.into(),
            web_port: 7000,
        };
        let item = aaa_proto::InboxItem {
            short_id: None,
            id: "i-1".into(),
            project: "demo".into(),
            item_kind: "post".into(),
            subject: "coordinator".into(),
            event: "post completed".into(),
            summary: format!("보고 {OLD}/#threads"),
            body: format!("본문 {OLD}/v/cap/"),
            done: true,
            seen: true,
            created_at_ms: 1,
            done_at_ms: Some(2),
        };
        db.inbox_put(&item).unwrap();
        db.set_meta("publish_origin", OLD).unwrap();
        db.set_meta("publish_change", &serde_json::to_string(&change).unwrap()).unwrap();
        let ask = json!({"title":"답 보존", "body":format!("{OLD}/#question=q-0001"), "done_ms":2, "by":"CLI"});
        {
            let conn = db.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO asks(project,id,kind,subject,done_ms,data) VALUES('demo','ask-1','request','i-1',2,?1)",
                [ask.to_string()],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO push_subs(endpoint,p256dh,auth,device,created_at) VALUES('fixture','key','auth','d',0)",
                [],
            )
            .unwrap();
            conn.execute_batch("CREATE TRIGGER fixture_failure BEFORE DELETE ON push_subs BEGIN SELECT RAISE(ABORT,'fixture write failure'); END;").unwrap();
        }
        assert!(db.finish_port_change(&change, "local:person").is_err());
        assert_eq!(db.inbox_list(None, true).unwrap(), std::slice::from_ref(&item));
        assert_eq!(db.meta("publish_origin").unwrap().as_deref(), Some(OLD));
        assert!(db.meta("publish_change").unwrap().is_some());
        {
            let conn = db.conn.lock().unwrap();
            assert_eq!(
                conn.query_row("SELECT COUNT(*) FROM web_link_originals", [], |r| r.get::<_, i64>(0)).unwrap(),
                0
            );
            assert_eq!(conn.query_row("SELECT COUNT(*) FROM push_subs", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
            conn.execute_batch("DROP TRIGGER fixture_failure;").unwrap();
        }
        assert_eq!(db.finish_port_change(&change, "local:person").unwrap(), (2, 1));
        let moved = db.inbox_list(None, true).unwrap().remove(0);
        assert_eq!(moved.body, item.body.replace(OLD, NEW));
        assert!(moved.done && moved.seen);
        assert_eq!(moved.done_at_ms, Some(2));
        assert_eq!(db.meta("publish_origin").unwrap().as_deref(), Some(NEW));
        assert!(db.meta("publish_change").unwrap().is_none());
        let conn = db.conn.lock().unwrap();
        let moved: String = conn.query_row("SELECT data FROM asks", [], |r| r.get(0)).unwrap();
        let mut expected = ask;
        expected["body"] = json!(format!("{NEW}/#question=q-0001"));
        assert_eq!(serde_json::from_str::<Value>(&moved).unwrap(), expected);
        conn.execute("DELETE FROM asks", []).unwrap();
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM web_link_originals WHERE kind='asks'", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0,
            "원본 보존도 기존 기록 삭제 수명을 따른다"
        );
    }
}
