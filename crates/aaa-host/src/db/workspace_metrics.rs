//! 저장소 신원과 작은 실행 요약 원장. 화면 조회는 이 테이블만 읽는다.
use super::Db;
use aaa_proto::{CodeLines, MetricRun, WorkspaceCollected};
use anyhow::Result;
use rusqlite::{OptionalExtension, params};

impl Db {
    /// URL을 정규화한 SHA256만 신원으로 쓴다. 모르는 등록은 빈 ID로 대기한다.
    pub(crate) fn repo_workspace_id(&self, project: &str, registration: &str, remote: Option<&str>) -> Result<String> {
        let db = self.conn.lock().unwrap();
        if let Some(remote) = remote {
            anyhow::ensure!(
                remote.len() == 64 && remote.bytes().all(|b| b.is_ascii_hexdigit()),
                "원격 저장소 신원 형식 오류"
            );
            let id = format!("ws-{remote}");
            db.execute("INSERT INTO repo_workspaces(project,registration,id) VALUES(?1,?2,?3) ON CONFLICT(project,registration) DO UPDATE SET id=excluded.id", params![project,registration,id])?;
            return Ok(id);
        }
        Ok(db
            .query_row(
                "SELECT id FROM repo_workspaces WHERE project=?1 AND registration=?2",
                params![project, registration],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default())
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
            if run.commit.is_empty() {
                continue;
            }
            let mut run = run.clone();
            run.machine = machine.into();
            run.run_id = format!("{machine}:{}", run.run_id);
            tx.execute("INSERT INTO repo_metrics(project,workspace,kind,id,at_ms,data) VALUES(?1,?2,'run',?3,?4,?5) ON CONFLICT(project,workspace,kind,id) DO NOTHING",params![project,workspace,run.run_id,run.observed_at_ms,serde_json::to_string(&run)?])?;
        }
        if let Some(lines) = &value.code_lines
            && lines.branch == "main"
        {
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
    pub(crate) fn workspace_run(&self, project: &str, workspace: &str, id: &str) -> Result<Option<MetricRun>> {
        let db = self.conn.lock().unwrap();
        let data: Option<String> = db
            .query_row(
                "SELECT data FROM repo_metrics WHERE project=?1 AND workspace=?2 AND kind='run' AND id=?3",
                params![project, workspace, id],
                |r| r.get(0),
            )
            .optional()?;
        data.map(|data| serde_json::from_str(&data).map_err(Into::into)).transpose()
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
