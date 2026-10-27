//! 재생성 가능한 검색 색인. 원문은 보존 출처에서만 넣고 사라진 출처는 다음 검색 때 제거한다.
use super::*;
use aaa_proto::{RecordDocument, RecordKind};
use std::collections::HashMap;

pub(super) fn init(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS work_records(
            project TEXT NOT NULL, key TEXT NOT NULL, kind TEXT NOT NULL,
            at_ms INTEGER NOT NULL, text TEXT NOT NULL, data TEXT NOT NULL,
            UNIQUE(project,key));
         CREATE INDEX IF NOT EXISTS work_records_page ON work_records(at_ms DESC,key DESC,project DESC);
         CREATE TABLE IF NOT EXISTS work_record_sources(
            project TEXT NOT NULL, origin TEXT NOT NULL, key TEXT NOT NULL, PRIMARY KEY(project,origin));
         CREATE INDEX IF NOT EXISTS work_record_sources_key ON work_record_sources(project,key);
         CREATE VIRTUAL TABLE IF NOT EXISTS work_records_fts USING fts5(text,content='work_records',content_rowid='rowid',tokenize='trigram');
         CREATE TRIGGER IF NOT EXISTS work_records_ask_gone AFTER DELETE ON asks BEGIN
            DELETE FROM work_record_sources WHERE project=old.project AND instr(origin,'question:'||old.id||':')=1;
            DELETE FROM work_records WHERE project=old.project AND NOT EXISTS(SELECT 1 FROM work_record_sources s WHERE s.project=work_records.project AND s.key=work_records.key); END;
         CREATE TRIGGER IF NOT EXISTS work_records_log_gone AFTER DELETE ON task_log BEGIN
            DELETE FROM work_record_sources WHERE project=old.project AND instr(origin,'task:log:'||old.id||':')=1;
            DELETE FROM work_records WHERE project=old.project AND NOT EXISTS(SELECT 1 FROM work_record_sources s WHERE s.project=work_records.project AND s.key=work_records.key); END;
         CREATE TRIGGER IF NOT EXISTS work_records_report_gone AFTER UPDATE OF deleted ON report_ids WHEN new.deleted=1 BEGIN
            DELETE FROM work_record_sources WHERE project=old.project AND origin='report:'||old.id;
            DELETE FROM work_records WHERE project=old.project AND NOT EXISTS(SELECT 1 FROM work_record_sources s WHERE s.project=work_records.project AND s.key=work_records.key); END;
         CREATE TRIGGER IF NOT EXISTS work_records_ai AFTER INSERT ON work_records BEGIN
            INSERT INTO work_records_fts(rowid,text) VALUES(new.rowid,new.text); END;
         CREATE TRIGGER IF NOT EXISTS work_records_ad AFTER DELETE ON work_records BEGIN
            INSERT INTO work_records_fts(work_records_fts,rowid,text) VALUES('delete',old.rowid,old.text); END;
         CREATE TRIGGER IF NOT EXISTS work_records_au AFTER UPDATE OF text ON work_records WHEN old.text<>new.text BEGIN
            INSERT INTO work_records_fts(work_records_fts,rowid,text) VALUES('delete',old.rowid,old.text);
            INSERT INTO work_records_fts(rowid,text) VALUES(new.rowid,new.text); END;",
    )?;
    Ok(())
}

pub(crate) fn kind_name(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Report => "report",
        RecordKind::Question => "question",
        RecordKind::Task => "task",
    }
}

impl Db {
    /// 보고 판은 불변이다. 이미 색인한 판의 본문 파일을 매 검색마다 다시 읽지 않는다.
    pub(crate) fn record_report_cache(&self, project: &str) -> Result<HashMap<String, RecordDocument>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT s.origin,r.data FROM work_record_sources s JOIN work_records r ON r.project=s.project AND r.key=s.key WHERE s.project=?1 AND r.kind='report'")?;
        let rows = st.query_map([project], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|r| {
            let (origin, data) = r?;
            Ok((origin, serde_json::from_str(&data)?))
        })
        .collect()
    }

    pub(crate) fn records_replace(&self, project: &str, rows: &[(String, RecordDocument)]) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM work_record_sources WHERE project=?1", [project])?;
        let mut unique: std::collections::BTreeMap<&str, (&RecordDocument, Vec<&str>)> =
            std::collections::BTreeMap::new();
        for (origin, doc) in rows {
            let entry = unique.entry(&doc.hit.key).or_insert((doc, Vec::new()));
            if (doc.hit.at_ms, &doc.hit.source) >= (entry.0.hit.at_ms, &entry.0.hit.source) {
                entry.0 = doc;
            }
            entry.1.push(origin);
        }
        for (key, (doc, mut origins)) in unique {
            origins.sort();
            let aliases = origins.join(" ");
            let text =
                crate::record_search::normalize(&format!("{} {} {} {}", doc.hit.id, doc.hit.title, aliases, doc.body));
            let data = serde_json::to_string(doc)?;
            tx.execute("INSERT INTO work_records(project,key,kind,at_ms,text,data) VALUES(?1,?2,?3,?4,?5,?6)
                ON CONFLICT(project,key) DO UPDATE SET at_ms=excluded.at_ms,text=excluded.text,data=excluded.data
                WHERE work_records.at_ms<>excluded.at_ms OR work_records.text<>excluded.text OR work_records.data<>excluded.data",
                params![project,key,kind_name(doc.hit.kind),doc.hit.at_ms,text,data])?;
            for origin in origins {
                tx.execute(
                    "INSERT INTO work_record_sources(project,origin,key) VALUES(?1,?2,?3)",
                    params![project, origin, key],
                )?;
            }
        }
        // 파일을 읽는 사이 정리된 원장은 같은 트랜잭션에서 다시 대조한다.
        tx.execute("DELETE FROM work_record_sources WHERE project=?1 AND origin LIKE 'question:%' AND NOT EXISTS(SELECT 1 FROM asks a WHERE a.project=work_record_sources.project AND a.id=substr(origin,10,instr(substr(origin,10),':')-1))",[project])?;
        tx.execute("DELETE FROM work_record_sources WHERE project=?1 AND origin LIKE 'report:%' AND NOT EXISTS(SELECT 1 FROM report_ids r WHERE r.project=work_record_sources.project AND r.id=substr(origin,8) AND r.ready=1 AND r.deleted=0)",[project])?;
        tx.execute("DELETE FROM work_record_sources WHERE project=?1 AND origin LIKE 'task:log:%' AND NOT EXISTS(SELECT 1 FROM task_log l WHERE l.project=work_record_sources.project AND l.id=CAST(substr(origin,10,instr(substr(origin,10),':')-1) AS INTEGER))",[project])?;
        tx.execute("DELETE FROM work_records WHERE project=?1 AND NOT EXISTS(SELECT 1 FROM work_record_sources s WHERE s.project=work_records.project AND s.key=work_records.key)", [project])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn record_report_source(&self, project: &str, key: &str) -> Result<Option<String>> {
        let c = self.conn.lock().unwrap();
        Ok(c.query_row("SELECT i.id FROM work_record_sources s JOIN report_ids i ON i.project=s.project AND s.origin='report:'||i.id WHERE s.project=?1 AND s.key=?2 AND i.ready=1 AND i.deleted=0 ORDER BY i.seq DESC LIMIT 1",params![project,key],|r|r.get(0)).optional()?)
    }

    pub(crate) fn record_get(&self, project: &str, key: &str) -> Result<Option<RecordDocument>> {
        let c = self.conn.lock().unwrap();
        let data: Option<String> = c
            .query_row("SELECT data FROM work_records WHERE project=?1 AND key=?2", params![project, key], |r| r.get(0))
            .optional()?;
        data.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
    }

    pub(crate) fn record_search_rows(
        &self,
        projects: &[String],
        kind: Option<RecordKind>,
        terms: &[String],
        after: Option<(u64, &str, &str)>,
        limit: usize,
    ) -> Result<Vec<RecordDocument>> {
        use rusqlite::types::Value;
        let c = self.conn.lock().unwrap();
        let mut sql = String::from("SELECT r.data FROM work_records r WHERE 1=1");
        let mut args: Vec<Value> = Vec::new();
        // 반드시 host가 확인한 프로젝트만. 빈 목록은 전체 허용으로 바꾸지 않는다.
        sql.push_str(" AND r.project IN (");
        sql.push_str(&vec!["?"; projects.len()].join(","));
        sql.push(')');
        args.extend(projects.iter().cloned().map(Value::Text));
        if let Some(kind) = kind {
            sql.push_str(" AND r.kind=?");
            args.push(kind_name(kind).to_string().into());
        }
        let fts: Vec<_> = terms
            .iter()
            .filter(|t| t.chars().count() >= 3)
            .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
            .collect();
        if !fts.is_empty() {
            sql.push_str(" AND r.rowid IN (SELECT rowid FROM work_records_fts WHERE work_records_fts MATCH ?)");
            args.push(fts.join(" AND ").into());
        }
        for term in terms {
            sql.push_str(" AND instr(r.text,?)>0");
            args.push(term.clone().into());
        }
        if let Some((at, key, project)) = after {
            sql.push_str(" AND (r.at_ms,r.key,r.project)<(?,?,?)");
            args.extend([Value::Integer(at as i64), key.to_string().into(), project.to_string().into()]);
        }
        sql.push_str(" ORDER BY r.at_ms DESC,r.key DESC,r.project DESC LIMIT ?");
        args.push((limit as i64).into());
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map(rusqlite::params_from_iter(args), |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
}
