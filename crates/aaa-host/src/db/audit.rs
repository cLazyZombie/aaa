//! 입력 기록과 감사 기록(환경 값은 이름만 남긴다).

use super::*;

/// env 값(`[[이름, 값]…]` 또는 `{이름: 값}`)을 이름 목록으로 바꾼다(키 관리자 K0, plan §8.307). 바꿨으면 true
pub(super) fn env_names_only(v: &mut serde_json::Value, path: &[&str]) -> bool {
    use serde_json::Value;
    let Some(env) = path.iter().try_fold(v, |v, k| v.get_mut(*k)) else { return false };
    let names: Vec<Value> = match env {
        Value::Array(items) if items.iter().any(|i| !i.is_string()) => {
            items.iter().map(|i| i.get(0).cloned().unwrap_or(Value::Null)).collect()
        }
        Value::Object(map) => map.keys().map(|k| Value::String(k.clone())).collect(),
        _ => return false,
    };
    *env = Value::Array(names);
    true
}

/// pane_run 감사 기록(띄운 spec)에서 env 값을 뺀다: spec의 env와 host가 푼 profile env
pub(super) fn spawn_env_names_only(v: &mut serde_json::Value) -> bool {
    let a = env_names_only(v, &["env"]);
    env_names_only(v, &["resolved_profile", "env"]) || a
}

/// pane_run 감사 기록의 detail: env는 이름만(plan §8.307)
pub(crate) fn pane_run_detail(spec: &aaa_proto::SpawnSpec) -> String {
    let mut v = serde_json::to_value(spec).unwrap_or_default();
    spawn_env_names_only(&mut v);
    v.to_string()
}

/// export에 넣는 thread 기록: launch env는 이름만. JSON이 아니면 그대로 둔다
pub(super) fn thread_data_without_env(data: &str) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(data)
        .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
    else {
        return data.to_string();
    };
    if env_names_only(&mut v, &["launch", "env"]) { v.to_string() } else { data.to_string() }
}

/// 옛 판이 pane_run 감사 기록에 남긴 env 값을 이름만 남기고 지운다. 지운 값이 빈 page·WAL에 남지 않게 VACUUM하고 WAL을 비운다.
/// 시작마다 값이 남은 행만 찾으므로 옛 판으로 되돌렸다 다시 올려도 그 사이 행을 정리한다. VACUUM 전에 멈추면 표지가 남아 다음 시작에
/// 다시 한다(plan §8.307).
pub(super) fn scrub_audit_env(conn: &mut Connection) -> Result<()> {
    let rows: Vec<(i64, String)> = conn
        .prepare(
            "SELECT rowid, detail FROM audit WHERE action='pane_run' AND json_valid(detail)
             AND (json_type(detail,'$.env[0]')='array' OR json_type(detail,'$.resolved_profile.env')='object')",
        )?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let pending = conn.query_row("SELECT 1 FROM meta WHERE key='audit_vacuum'", [], |_| Ok(())).optional()?.is_some();
    if rows.is_empty() && !pending {
        return Ok(());
    }
    let tx = conn.transaction()?;
    for (rowid, detail) in &rows {
        let mut v: serde_json::Value = serde_json::from_str(detail)?;
        spawn_env_names_only(&mut v);
        tx.execute("UPDATE audit SET detail=?2 WHERE rowid=?1", params![rowid, v.to_string()])?;
    }
    tx.execute("INSERT INTO meta(key,value) VALUES('audit_vacuum','1') ON CONFLICT(key) DO NOTHING", [])?;
    tx.commit()?;
    // 행의 값은 이미 지웠다. VACUUM(임시 공간·잠금이 필요하다)이 실패해도 host는 시작하고, 표지가 남아 다음 시작에 다시 한다
    let vacuum = conn.execute_batch("VACUUM").and_then(|_| {
        conn.execute("DELETE FROM meta WHERE key='audit_vacuum'", [])?;
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
    });
    match vacuum {
        Ok(()) => tracing::info!(rows = rows.len(), "감사 기록의 옛 env 값을 지웠다(VACUUM)"),
        Err(e) => {
            tracing::warn!(rows = rows.len(), error = %e, "감사 기록의 옛 env 값은 지웠지만 VACUUM을 못 했다: 다음 시작에 다시 한다")
        }
    }
    Ok(())
}

impl Db {
    pub(crate) fn input_event(&self, row: &aaa_proto::InputEvent) -> Result<()> {
        let mut c = self.conn.lock().unwrap();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO input_events(ts,pane,thread,data) VALUES(?1,?2,?3,?4)",
            params![row.at_ms, row.pane, row.thread, serde_json::to_string(row)?],
        )?;
        tx.execute("DELETE FROM input_events WHERE ts < ?1", [row.at_ms.saturating_sub(30 * 86400 * 1000)])?;
        tx.execute("DELETE FROM input_events WHERE seq <= (SELECT seq FROM input_events ORDER BY seq DESC LIMIT 1 OFFSET 10000)", [])?;
        // 기존 입력 진단 파일(20 × 1MiB)과 같은 최대 본문 예산. 최신 사건부터 남긴다.
        let bytes: u64 =
            tx.query_row("SELECT coalesce(sum(length(CAST(data AS BLOB))),0) FROM input_events", [], |r| r.get(0))?;
        if bytes > 20 * 1024 * 1024 {
            tx.execute("WITH sizes AS (SELECT seq,sum(length(CAST(data AS BLOB))) OVER (ORDER BY seq DESC) AS bytes FROM input_events) DELETE FROM input_events WHERE seq IN (SELECT seq FROM sizes WHERE bytes > 20971520)", [])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn input_events(
        &self,
        pane: Option<&str>,
        thread: Option<&str>,
        since: u64,
    ) -> Result<Vec<aaa_proto::InputEvent>> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT data FROM (SELECT seq,data FROM input_events WHERE ts>=?1 AND (?2 IS NULL OR pane=?2) AND (?3 IS NULL OR thread=?3 OR substr(thread,instr(thread,'/')+1)=?3) ORDER BY seq DESC LIMIT 1000) ORDER BY seq")?;
        let rows = st.query_map(params![since, pane, thread], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }

    pub fn audit(&self, actor: &str, action: &str, detail: &str) {
        let c = self.conn.lock().unwrap();
        aaa_common::diagnostic::ignored(
            c.execute(
                "INSERT INTO audit(ts,actor,action,detail) VALUES(?1,?2,?3,?4)",
                params![aaa_common::util::now_ms() as i64, actor, action, detail],
            ),
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
    }

    /// 감사 기록의 (actor, action)을 쓴 순서대로(시험용)
    #[cfg(test)]
    pub(crate) fn audit_actions(&self) -> Vec<(String, String)> {
        let c = self.conn.lock().unwrap();
        let mut st = c.prepare("SELECT actor, action FROM audit ORDER BY rowid").unwrap();
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(|r| r.unwrap()).collect()
    }
}
