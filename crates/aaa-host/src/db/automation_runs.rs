//! 큐·커서·receipt와 같은 SQLite transaction에 쓰는 실행 원장.
use super::*;
use aaa_proto::{
    AutomationExecution as Run, AutomationRunEvent, AutomationRunState as Status, AutomationStage as Stage,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) fn id(name: &str, input: &str) -> String {
    format!("ar-{}", hex::encode(Sha256::digest(format!("{name}\n{input}").as_bytes())))
}
fn read(c: &Connection, p: &str, id: &str) -> Result<Option<Run>> {
    let raw: Option<String> = c
        .query_row("SELECT data FROM automation_runs WHERE project=?1 AND id=?2", params![p, id], |r| r.get(0))
        .optional()?;
    raw.map(|v| serde_json::from_str(&v).map_err(Into::into)).transpose()
}
fn put(c: &Connection, p: &str, mut run: Run, batch: Option<&str>) -> Result<()> {
    let previous = read(c, p, &run.id)?;
    if previous.as_ref().is_none_or(|old| old.state != run.state || old.stage != run.stage || old.reason != run.reason)
    {
        run.events.push(AutomationRunEvent {
            at_ms: run.updated_at_ms,
            stage: run.stage,
            state: run.state,
            reason: run.reason.clone(),
        });
    }
    c.execute("INSERT INTO automation_runs(project,id,name,input_id,batch,updated,data) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(project,id) DO UPDATE SET batch=COALESCE(excluded.batch,automation_runs.batch),updated=excluded.updated,data=excluded.data",params![p,run.id,run.name,run.input_id,batch,run.updated_at_ms,serde_json::to_string(&run)?])?;
    Ok(())
}
fn fresh(name: &str, e: &Value, now: u64) -> Result<Run> {
    let input = e["id"].as_str().ok_or_else(|| anyhow::anyhow!("자동화 입력 ID 누락"))?;
    Ok(Run {
        id: id(name, input),
        name: name.into(),
        input_id: input.into(),
        revision: e["revision"].as_str().unwrap_or("").into(),
        source: e["source"].as_str().unwrap_or("legacy").into(),
        created_at_ms: e["at_ms"].as_u64().unwrap_or(now),
        updated_at_ms: now,
        state: Status::Queued,
        stage: Stage::Trigger,
        reason: "입력 접수".into(),
        receipt: None,
        thread: None,
        task: None,
        report: None,
        input_file: None,
        processed: false,
        outcome: None,
        legacy: false,
        events: vec![],
    })
}
fn entries(v: &Value) -> impl Iterator<Item = (&Value, Option<&Value>)> {
    v["pending"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| (e, None))
        .chain(v["inflight"]["events"].as_array().into_iter().flatten().map(|e| (e, Some(&v["inflight"]))))
}
/// meta의 큐 호환 표현을 유지하면서 모든 source가 원장을 빠뜨리지 못하게 한다.
pub(super) fn sync(
    c: &Connection,
    key: &str,
    old: Option<&str>,
    value: &str,
    extra: &[(String, String)],
) -> Result<()> {
    let Some(rest) = key.strip_prefix("automation:") else { return Ok(()) };
    let Some((p, name)) = rest.split_once(':') else { return Ok(()) };
    let before: Value = old.map(serde_json::from_str).transpose()?.unwrap_or(Value::Null);
    let after: Value = serde_json::from_str(value)?;
    let now = aaa_common::util::now_ms();
    let mut active = std::collections::HashSet::new();
    for (e, batch) in entries(&after) {
        let new = fresh(name, e, now)?;
        active.insert(new.id.clone());
        let mut run = read(c, p, &new.id)?.unwrap_or(new);
        run.updated_at_ms = now;
        if let Some(b) = batch {
            if run.stage == Stage::Observe && !b["command_result"].is_null() {
                let result = &b["command_result"];
                let ok = result["exit_code"].as_i64() == Some(0) && result["timed_out"].as_bool() == Some(false);
                run.events.push(AutomationRunEvent {
                    at_ms: now,
                    stage: Stage::Observe,
                    state: if ok { Status::Succeeded } else { Status::Failed },
                    reason: format!("조회 종료 코드 {} · timeout {}", result["exit_code"], result["timed_out"]),
                });
            }
            run.state = if b["unknown"].as_bool() == Some(true) { Status::Unknown } else { Status::Running };
            run.stage =
                if e["observe"].is_object() && b["command_result"].is_null() { Stage::Observe } else { Stage::Action };
            run.reason = if run.state == Status::Unknown {
                "외부 실행 결과 불명 · 재실행 안 함"
            } else {
                "실행 중"
            }
            .into();
            run.thread = b["target"].as_str().map(str::to_owned).or(run.thread);
            if e["action"]["start"]["reuse"] == "open" {
                run.input_file = b["input_name"].as_str().map(|s| format!("inbox/{s}.json"));
            }
        } else if run.state == Status::Running {
            run.state = Status::Waiting;
            run.stage = Stage::Policy;
            run.reason = "다음 판정 대기".into();
        }
        put(c, p, run, batch.and_then(|b| b["id"].as_str()))?;
    }
    for (e, batch) in entries(&before) {
        let new = fresh(name, e, now)?;
        if active.contains(&new.id) {
            continue;
        }
        let mut run = read(c, p, &new.id)?.unwrap_or(new);
        let receipt_key =
            format!("automation_done:{p}:{name}:{}", hex::encode(Sha256::digest(run.input_id.as_bytes())));
        let result =
            extra.iter().find(|(k, _)| k == &receipt_key).and_then(|(_, v)| serde_json::from_str::<Value>(v).ok());
        run.updated_at_ms = now;
        if let Some(v) = result {
            run.state = serde_json::from_value(v["state"].clone())?;
            run.stage = serde_json::from_value(v["stage"].clone())?;
            run.reason = v["reason"].as_str().unwrap_or("").into();
            run.receipt = serde_json::from_value(v["receipt"].clone())?;
            run.thread = v["thread"].as_str().map(str::to_owned).or(run.thread);
        } else {
            run.state = Status::Unknown;
            run.stage = Stage::Legacy;
            run.legacy = true;
            run.reason = "옛 receipt · 동작 결과를 구분할 수 없음".into();
        }
        put(c, p, run, batch.and_then(|b| b["id"].as_str()))?;
    }
    Ok(())
}
impl Db {
    pub(crate) fn automation_executions(&self, p: &str, name: Option<&str>, before: Option<&str>) -> Result<Vec<Run>> {
        let c = self.conn.lock().unwrap();
        let mut stmt=c.prepare("SELECT data FROM automation_runs WHERE project=?1 AND (?2 IS NULL OR name=?2) AND (?3 IS NULL OR rowid<(SELECT rowid FROM automation_runs WHERE project=?1 AND id=?3)) ORDER BY rowid DESC LIMIT 100")?;
        let rows = stmt
            .query_map(params![p, name, before], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter().map(|s| serde_json::from_str(&s).map_err(Into::into)).collect()
    }
    pub(crate) fn automation_execution(&self, p: &str, run: &str) -> Result<Option<Run>> {
        read(&self.conn.lock().unwrap(), p, run)
    }
    pub(crate) fn automation_import_observations(
        &self,
        p: &str,
        name: &str,
        observations: &[aaa_proto::RoutineRun],
    ) -> Result<()> {
        self.image_transaction(|c| {
            for old in observations.iter().rev() {
                if read(c, p, &id(name, &old.id))?.is_some() {
                    continue;
                }
                let e = serde_json::json!({"id":old.id,"at_ms":old.at_ms,"source":"legacy"});
                let mut run = fresh(name, &e, old.at_ms)?;
                run.legacy = true;
                run.stage = Stage::Legacy;
                run.state = Status::Unknown;
                run.thread = old.thread.clone();
                run.reason = format!("옛 관측 {} · {}", old.status, old.detail);
                put(c, p, run, None)?;
            }
            Ok(())
        })
    }
    pub(crate) fn automation_complete(
        &self,
        p: &str,
        thread: &str,
        completion: &aaa_proto::AutomationCompletion,
    ) -> Result<()> {
        self.image_transaction(|c| {
            let mut run = read(c, p, &completion.run)?.ok_or_else(|| anyhow::anyhow!("실행 ID가 없다"))?;
            anyhow::ensure!(run.thread.as_deref() == Some(thread), "이 thread에 전달된 입력이 아니다");
            if let Some(report) = &completion.report {
                let source: Option<String> = c
                    .query_row(
                        "SELECT source FROM report_ids WHERE project=?1 AND id=?2 AND ready=1 AND deleted=0",
                        params![p, report],
                        |r| r.get(0),
                    )
                    .optional()?;
                anyhow::ensure!(source.as_deref() == Some(thread), "이 thread의 준비된 보고가 아니다");
            }
            let outcome = serde_json::to_value(completion.outcome)?.as_str().unwrap().to_string();
            if run.processed {
                anyhow::ensure!(
                    run.outcome.as_deref() == Some(&outcome) && run.report == completion.report,
                    "이미 다른 처리 결과가 기록돼 있다"
                );
                return Ok(());
            }
            run.processed = true;
            run.outcome = Some(outcome);
            run.report = completion.report.clone();
            run.updated_at_ms = aaa_common::util::now_ms();
            run.events.push(AutomationRunEvent {
                at_ms: run.updated_at_ms,
                stage: Stage::FollowUp,
                state: run.state,
                reason: format!("입력 처리 신고: {:?}", completion.outcome),
            });
            put(c, p, run, None)
        })
    }
    /// 대기 사유는 기존 관측 API도 받지만 완료 판정은 명시적 receipt만 맡는다.
    pub(crate) fn automation_wait(
        &self,
        p: &str,
        name: &str,
        input: &str,
        reason: &str,
        thread: Option<&str>,
    ) -> Result<()> {
        self.image_transaction(|c| {
            let mut stmt = c.prepare(
                "SELECT data FROM automation_runs WHERE project=?1 AND name=?2 AND (input_id=?3 OR batch=?3)",
            )?;
            let rows = stmt
                .query_map(params![p, name, input], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            for raw in rows {
                let mut run: Run = serde_json::from_str(&raw)?;
                if !matches!(run.state, Status::Queued | Status::Waiting | Status::Running) {
                    continue;
                }
                run.state = Status::Waiting;
                run.stage = Stage::Policy;
                run.reason = reason.into();
                run.updated_at_ms = aaa_common::util::now_ms();
                run.thread = thread.map(str::to_owned).or(run.thread);
                put(c, p, run, None)?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queue_receipt_and_execution_commit_together_and_survive_restart() {
        let dir = std::env::temp_dir().join(format!("aaa-ledger-{}", aaa_common::util::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("db");
        let db = Db::open(&path).unwrap();
        let key = "automation:demo:watch";
        let e =
            serde_json::json!({"id":"schedule:1","at_ms":1,"source":"routine","revision":"rev","action":{"inbox":{}}});
        let queued = serde_json::json!({"pending":[e],"inflight":null}).to_string();
        db.update_meta(key, |_| Ok(queued.clone())).unwrap();
        let run = db.automation_executions("demo", Some("watch"), None).unwrap().remove(0);
        assert_eq!(run.state, Status::Queued);
        let receipt = format!("automation_done:demo:watch:{}", hex::encode(Sha256::digest(b"schedule:1")));
        let empty = serde_json::json!({"pending":[],"inflight":null}).to_string();
        assert!(
            db.update_meta_checked(key, &[], |_| Ok((
                empty.clone(),
                vec![(receipt.clone(), "{\"state\":\"invalid\"}".into())]
            )))
            .is_err()
        );
        assert_eq!(db.meta(key).unwrap().as_deref(), Some(queued.as_str()));
        assert_eq!(db.automation_execution("demo", &run.id).unwrap().unwrap().state, Status::Queued);
        db.update_meta_checked(key,&[],|_|Ok((empty,vec![(receipt.clone(),serde_json::json!({"state":"succeeded","stage":"action","reason":"안내 전달","receipt":"notice_created","thread":null}).to_string())]))).unwrap();
        drop(db);
        let db = Db::open(&path).unwrap();
        let run = db.automation_execution("demo", &run.id).unwrap().unwrap();
        assert_eq!(run.state, Status::Succeeded);
        assert_eq!(run.receipt, Some(aaa_proto::AutomationReceipt::NoticeCreated));
        assert!(run.outcome.is_none(), "안내 전달을 작업 성공으로 승격하지 않는다");
        assert_eq!(run.events.len(), 2);
        assert!(db.meta(&receipt).unwrap().is_some());
        let completion = aaa_proto::AutomationCompletion {
            run: run.id.clone(),
            outcome: aaa_proto::AutomationOutcome::Completed,
            report: None,
        };
        assert!(db.automation_complete("demo", "t-0001", &completion).is_err());
        db.image_transaction(|c| {
            let mut run = run.clone();
            run.thread = Some("t-0001".into());
            put(c, "demo", run, None)
        })
        .unwrap();
        assert!(db.automation_complete("demo", "t-0002", &completion).is_err());
        assert!(
            db.automation_complete(
                "demo",
                "t-0001",
                &aaa_proto::AutomationCompletion { report: Some("r-0001".into()), ..completion.clone() }
            )
            .is_err()
        );
        db.automation_complete("demo", "t-0001", &completion).unwrap();
        db.automation_complete("demo", "t-0001", &completion).unwrap();
        assert!(
            db.automation_complete(
                "demo",
                "t-0001",
                &aaa_proto::AutomationCompletion { outcome: aaa_proto::AutomationOutcome::Failed, ..completion }
            )
            .is_err()
        );
        assert!(db.automation_execution("demo", &run.id).unwrap().unwrap().processed);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
