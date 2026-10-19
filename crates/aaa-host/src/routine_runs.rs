//! 자동화 관측 기록. 실행 판정·승인·전달과 독립이며 실패를 숨기지 않는다.
use crate::{Host, err};
use aaa_proto::{ErrorBody, ErrorCode, RoutineRun, RoutineView};
use serde::{Deserialize, Serialize};

const CAP: usize = 50;
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct History {
    pub runs: Vec<RoutineRun>,
    pub last_result: Option<RoutineRun>,
    pub last_skip: Option<RoutineRun>,
    pub day: i64,
    pub today: u32,
}
fn day(at: u64) -> i64 {
    let seconds = (at / 1000) as i64;
    (seconds + crate::routine::local_offset_secs(seconds)).div_euclid(86400)
}
impl History {
    fn put(&mut self, run: RoutineRun) {
        let today = day(run.at_ms);
        if self.day != today {
            self.day = today;
            self.today = 0;
        }
        let was_done = self.runs.iter().any(|r| r.id == run.id && r.status == "done");
        if run.status == "done" && !was_done {
            self.today = self.today.saturating_add(1);
        }
        if run.status == "skipped" || run.status == "waiting" {
            self.last_skip = Some(run.clone());
        }
        self.last_result = Some(run.clone());
        // 내구 outbox가 같은 항목을 다시 전달하면 같은 기록을 갱신한다.
        self.runs.retain(|r| r.id != run.id);
        self.runs.insert(0, run);
        self.runs.truncate(CAP);
    }
    pub fn fill(self, view: &mut RoutineView) {
        view.last_result = self.last_result;
        view.last_skip = self.last_skip;
        view.today = if self.day == day(aaa_common::util::now_ms()) { self.today } else { 0 };
        view.recent = self.runs.into_iter().take(20).collect();
    }
}
impl Host {
    pub(crate) fn routine_history(&self, p: &str, name: &str) -> Result<History, ErrorBody> {
        self.project_dir(p)?;
        let text =
            self.db.meta(&format!("routine_runs:{p}:{name}")).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
        text.map(|s| serde_json::from_str(&s))
            .transpose()
            .map_err(|e| err(ErrorCode::Internal, e.to_string()))
            .map(Option::unwrap_or_default)
    }
    pub(crate) fn routine_runs(&self, p: &str, name: &str) -> Result<Vec<RoutineRun>, ErrorBody> {
        // 파일이 없어진 규칙의 기록도 이름을 알면 볼 수 있다.
        let valid = name.strip_prefix("mission:").unwrap_or(name);
        if !aaa_proto::valid_project(valid) {
            return Err(err(ErrorCode::BadRequest, "자동화 이름 형식이 아니다"));
        }
        Ok(self.routine_history(p, name)?.runs)
    }
    pub(crate) fn record_routine(
        &self,
        p: &str,
        name: &str,
        id: &str,
        status: &str,
        detail: &str,
        thread: Option<&str>,
    ) {
        let run = RoutineRun {
            id: id.into(),
            at_ms: aaa_common::util::now_ms(),
            status: status.into(),
            detail: detail.chars().take(2000).collect(),
            thread: thread.map(str::to_owned),
        };
        if let Err(e) = self.db.update_meta(&format!("routine_runs:{p}:{name}"), |old| {
            let mut history: History = old.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_default();
            history.put(run);
            Ok(serde_json::to_string(&history)?)
        }) {
            tracing::error!(project=p, routine=name, %e, "자동화 실행 기록을 저장하지 못했다");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn durable_bounded_history_keeps_daily_total_and_delivery_identity() {
        let dir = std::env::temp_dir().join(format!("aaa-runs-{}", aaa_common::util::new_id()));
        let path = dir.join("db");
        std::fs::create_dir_all(&dir).unwrap();
        let db = crate::db::Db::open(&path).unwrap();
        let now = aaa_common::util::now_ms();
        let key = "routine_runs:demo:watch";
        for i in 0..65 {
            db.update_meta(key, |old| {
                let mut h: History = old.map(|s| serde_json::from_str(&s)).transpose()?.unwrap_or_default();
                h.put(RoutineRun {
                    id: i.to_string(),
                    at_ms: now,
                    status: "done".into(),
                    detail: i.to_string(),
                    thread: None,
                });
                Ok(serde_json::to_string(&h)?)
            })
            .unwrap();
        }
        drop(db);
        let db = crate::db::Db::open(&path).unwrap();
        let mut h: History = serde_json::from_str(&db.meta(key).unwrap().unwrap()).unwrap();
        assert_eq!(h.runs.len(), 50);
        assert_eq!(h.runs.first().unwrap().id, "64");
        assert_eq!(h.runs.last().unwrap().id, "15");
        assert_eq!(h.today, 65);
        let mut repeated = h.runs[0].clone();
        repeated.detail = "same delivery".into();
        h.put(repeated);
        assert_eq!(h.today, 65);
        assert_eq!(h.runs.len(), 50);
        h.put(RoutineRun {
            id: "skip".into(),
            at_ms: now,
            status: "skipped".into(),
            detail: "출력 같음".into(),
            thread: None,
        });
        assert_eq!(h.last_skip.as_ref().unwrap().detail, "출력 같음");
        h.put(RoutineRun {
            id: "next".into(),
            at_ms: now + 86400000,
            status: "done".into(),
            detail: "내일".into(),
            thread: None,
        });
        assert_eq!(h.today, 1);
        assert_eq!(h.last_skip.unwrap().detail, "출력 같음");
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
