//! 저장소 신원과 작은 실행 요약 원장. 화면 조회는 이 테이블만 읽는다.
use super::Db;
use aaa_proto::{CodeLines, MetricRun, WorkspaceCollected};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

impl Db {
    pub(crate) fn repo_workspace_id(&self, project: &str, registration: &str, alias: Option<&str>) -> Result<String> {
        let mut db = self.conn.lock().unwrap();
        let tx = db.transaction()?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT id FROM repo_workspaces WHERE project=?1 AND registration=?2",
                params![project, registration],
                |r| r.get(0),
            )
            .optional()?;
        let old_id = existing.clone();
        let id = if let Some(alias) = alias {
            format!("ws-{alias}")
        } else {
            existing.unwrap_or_else(|| format!("ws-{}", aaa_common::util::new_id()))
        };
        if old_id.as_deref() == Some(id.as_str()) {
            tx.commit()?;
            return Ok(id);
        }
        tx.execute("INSERT INTO repo_workspaces(project,registration,id) VALUES(?1,?2,?3) ON CONFLICT(project,registration) DO UPDATE SET id=excluded.id",params![project,registration,id])?;
        if let Some(old) = old_id.filter(|old| old != &id) {
            let remaining: u32 = tx.query_row(
                "SELECT COUNT(*) FROM repo_workspaces WHERE project=?1 AND id=?2",
                params![project, old],
                |r| r.get(0),
            )?;
            if remaining == 0 {
                tx.execute("INSERT OR IGNORE INTO repo_metrics SELECT project,?3,kind,id,at_ms,data FROM repo_metrics WHERE project=?1 AND workspace=?2",params![project,old,id])?;
                tx.execute("DELETE FROM repo_metrics WHERE project=?1 AND workspace=?2", params![project, old])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }
    pub(crate) fn save_workspace_metrics(
        &self,
        project: &str,
        workspace: &str,
        machine: &str,
        value: &WorkspaceCollected,
    ) -> Result<()> {
        let mut db = self.conn.lock().unwrap();
        let tx = db.transaction()?;
        for run in &value.runs {
            let mut run = run.clone();
            run.machine = machine.into();
            tx.execute("INSERT INTO repo_metrics(project,workspace,kind,id,at_ms,data) VALUES(?1,?2,'run',?3,?4,?5) ON CONFLICT(project,workspace,kind,id) DO NOTHING",params![project,workspace,run.run_id,run.observed_at_ms,serde_json::to_string(&run)?])?;
        }
        if let Some(lines) = &value.code_lines {
            let id = format!("{}:{}", lines.commit, lines.rules_version);
            tx.execute("INSERT INTO repo_metrics(project,workspace,kind,id,at_ms,data) VALUES(?1,?2,'lines',?3,?4,?5) ON CONFLICT(project,workspace,kind,id) DO NOTHING",params![project,workspace,id,lines.observed_at_ms,serde_json::to_string(lines)?])?;
        }
        tx.execute(
            "INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![
                format!("workspace-collection:{project}:{workspace}:{machine}"),
                serde_json::to_string(&(aaa_common::util::now_ms(), &value.notes))?
            ],
        )?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn workspace_collection_error(&self, project: &str, workspace: &str, machine: &str) -> Result<()> {
        let key = format!("workspace-collection:{project}:{workspace}:{machine}");
        let last = self
            .meta(&key)?
            .and_then(|s| {
                serde_json::from_str::<(u64, Vec<String>)>(&s)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .map(|c| c.0)
            .unwrap_or(0);
        self.set_meta(&key, &serde_json::to_string(&(last, vec!["agent 전송 대기 또는 수집 오류"]))?)
    }
    pub(crate) fn workspace_code_commits(&self, project: &str, workspace: &str) -> Result<Vec<String>> {
        let db = self.conn.lock().unwrap();
        let mut st=db.prepare("SELECT id FROM repo_metrics WHERE project=?1 AND workspace=?2 AND kind='lines' ORDER BY at_ms DESC LIMIT 4096")?;
        Ok(st.query_map(params![project, workspace], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?)
    }
    pub(crate) fn workspace_records(
        &self,
        project: &str,
        workspace: &str,
        since: u64,
    ) -> Result<(Vec<MetricRun>, Vec<CodeLines>, bool)> {
        let db = self.conn.lock().unwrap();
        let mut stmt=db.prepare("SELECT kind,data FROM repo_metrics WHERE project=?1 AND workspace=?2 AND at_ms>=?3 ORDER BY at_ms DESC,id LIMIT 401")?;
        let rows = stmt
            .query_map(params![project, workspace, since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut runs = Vec::new();
        let mut lines = Vec::new();
        let mut n = 0;
        for row in rows {
            let (kind, data) = row?;
            n += 1;
            if n > 400 {
                break;
            }
            if kind == "run" {
                runs.push(serde_json::from_str(&data)?);
            } else {
                lines.push(serde_json::from_str(&data)?);
            }
        }
        Ok((runs, lines, n > 400))
    }
}
