//! 머신 독점 예약·자동 배정 비중·착지 중 머신(t-1265). host가 머신 사용을 통제한다: coordinator가 말로 미루지 않는다.
//!
//! - 예약이 걸린 머신은 주인이 아닌 시작을 받지 않는다. 자동 배정(할 일 예약 큐·자동화·머신 칸 없는 시작)은 그 머신을 빼고,
//!   머신을 지정한 시작(할 일 owner `@머신`·자동화 `machines`·`--machine`)은 풀릴 때까지 기다린다(`start_wait`). 사용자, 또는
//!   사용자 지시 근거를 준 coordinator의 `ignore_reservation` 시작만 통과한다. 주인은 `t-NNNN` thread나 할 일이며, 할 일이
//!   주인이면 그 할 일에서 띄운 thread가 들어간다.
//! - 정본은 host DB meta `machine-reservation:<머신 id>`다. 주인 thread가 끝나거나(Resolved·Failed·기록 없음) 주인 할 일이
//!   활성 목록에서 빠지거나 끝 시각이 지나면 읽을 때 풀고 감사에 남긴다. DB 선점 트랜잭션도 같은 기록을 다시 본다.
//! - 누가: 사용자(웹 조작 기기·세션 밖 CLI), 사용자 지시 근거를 준 그 프로젝트 coordinator, 자기 머신을 자기 이름으로 잡는
//!   thread(post 길이라 원격 thread도 된다. 받는 host가 보낸 pane의 thread와 머신을 확인한다).
//! - 비중: meta `machine-weight:<머신 id>`(사람만). 없으면 agent가 알린 CPU 수다. 자동 배정은 열린 thread÷비중이 작은
//!   머신부터 고르고, 0이면 자동 배정에서 뺀다.
//! - 착지 중 머신: 보고 `[착지]` 단계인 thread가 지금 쓰는 머신이다. 자동 배정에서 뺀다(지정한 시작은 막지 않는다).
//! - 참고: Kubernetes cordon/taint·Nomad drain(머신별 배정 제외 표지와 주인), Slurm reservation(주인·기간), Jenkins
//!   executors(머신 능력만큼의 몫). 개념만 따르고 코드는 가져오지 않았다.

use std::collections::HashMap;

use aaa_proto::{ErrorBody, ErrorCode, FrontResponse, MachineReservation, PostInput, ThreadStatus};
use rusqlite::OptionalExtension;

use crate::orch::{ThreadRec, bad, internal};
use crate::{Host, err};

fn key(mid: &str) -> String {
    format!("machine-reservation:{mid}")
}
fn weight_key(mid: &str) -> String {
    format!("machine-weight:{mid}")
}

/// 사용자 지시 근거·메모 한 줄(공백 제거 뒤 비면 없음, 2000바이트 이하, 제어 문자 없음)
fn line(field: &str, text: Option<String>) -> Result<Option<String>, ErrorBody> {
    let text = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if text.as_deref().is_some_and(|t| t.len() > 2000 || t.chars().any(char::is_control)) {
        return Err(bad(format!("{field}는 한 줄(2000바이트 이하)로 쓴다")));
    }
    Ok(text)
}

/// 이 시작이 예약 주인인가: 같은 프로젝트에서 주인 할 일로 띄운 thread
fn admits(r: &MachineReservation, project: &str, task: Option<&str>) -> bool {
    r.project == project && r.task_id.is_some() && r.task_id.as_deref() == task
}

/// DB 선점 트랜잭션 안의 다시 보기: 판정 뒤 걸린 예약을 지킨다. 주인 생존은 앞의 판정이 이미 봤다(끝 시각만 다시 본다)
pub(crate) fn check_tx(
    tx: &rusqlite::Connection,
    mid: &str,
    project: &str,
    task: Option<&str>,
    now: u64,
) -> anyhow::Result<()> {
    let text: Option<String> =
        tx.query_row("SELECT value FROM meta WHERE key=?1", [key(mid)], |r| r.get(0)).optional()?;
    let Some(text) = text else { return Ok(()) };
    let r: MachineReservation = serde_json::from_str(&text)?;
    if r.until_ms.is_some_and(|u| u <= now) || admits(&r, project, task) {
        return Ok(());
    }
    anyhow::bail!("start_wait: 머신 독점 예약({}) 풀림 대기", r.holder)
}

impl Host {
    /// 지금 유효한 예약. 주인이 끝났거나 시한이 지난 기록은 여기서 지운다
    pub(crate) fn machine_reservation(&self, mid: &str) -> Option<MachineReservation> {
        let text = self
            .db
            .meta(&key(mid))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "머신 예약을 읽지 못해 예약 없음으로 본다"))
            .ok()??;
        let r: MachineReservation = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(error) => {
                aaa_common::diagnostic::observe(&error, "머신 예약 기록 형식이 어긋나 예약 없음으로 본다");
                return None;
            }
        };
        let Some(why) = self.reservation_over(&r) else { return Some(r) };
        // 그사이 다른 쪽이 새 예약을 걸었으면 지우지 않는다
        if self.db.meta(&key(mid)).ok().flatten().as_deref() == Some(text.as_str()) {
            aaa_common::diagnostic::ignored(self.db.del_meta(&key(mid)), "다음 읽기에서 다시 푼다");
            self.db.audit(
                "host",
                "machine_release",
                &format!("{} {} 저절로 풂: {why}", self.machine_name(mid), r.holder),
            );
            self.bump();
            self.tick_wake.notify_one();
        }
        None
    }

    /// 예약이 끝났는가(까닭)
    fn reservation_over(&self, r: &MachineReservation) -> Option<&'static str> {
        if r.until_ms.is_some_and(|u| u <= aaa_common::util::now_ms()) {
            return Some("끝 시각 지남");
        }
        if let Some(task) = &r.task_id {
            // machine_views가 부르므로 TASKS.md 잠금·ID 쓰기(task_board) 없이 파일만 읽는다: 잠금을 쥔 편집 중에도 불린다
            let text = self
                .project_dir(&r.project)
                .ok()
                .map(|dir| std::fs::read_to_string(dir.join("TASKS.md")))
                .transpose()
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "할 일 파일을 읽지 못해 예약을 유지한다"));
            return match text {
                Ok(Some(text)) if !crate::tasks::parse(&text).iter().any(|t| t.id.as_ref() == Some(task)) => {
                    Some("주인 할 일이 활성 목록에 없음")
                }
                Ok(None) => Some("주인 프로젝트 없음"),
                _ => None,
            };
        }
        let rec = self.db.thread_get(&r.project, &r.holder).ok().flatten();
        match rec.map(|t| serde_json::from_str::<ThreadRec>(&t)) {
            None => Some("주인 thread 기록 없음"),
            Some(Ok(t)) if !t.holds_slot() && t.status != ThreadStatus::Parked => Some("주인 thread 끝남"),
            _ => None,
        }
    }

    /// 머신 id별 유효한 예약
    pub(crate) fn machine_reservations(&self) -> HashMap<String, MachineReservation> {
        let ids: Vec<String> = self.inner.lock().unwrap().machines.keys().cloned().collect();
        ids.into_iter().filter_map(|mid| self.machine_reservation(&mid).map(|r| (mid, r))).collect()
    }

    /// 머신 id별 사람이 정한 비중
    pub(crate) fn machine_weights(&self) -> HashMap<String, u32> {
        let ids: Vec<String> = self.inner.lock().unwrap().machines.keys().cloned().collect();
        ids.into_iter()
            .filter_map(|mid| {
                let w = self.db.meta(&weight_key(&mid)).ok().flatten()?.parse().ok()?;
                Some((mid, w))
            })
            .collect()
    }

    /// 착지 중 머신(id → 착지하는 thread)
    pub(crate) fn landing_machines(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        for text in self.db.threads(None).unwrap_or_default() {
            if let Ok(r) = serde_json::from_str::<ThreadRec>(&text)
                && r.execution_stage() == Some(aaa_proto::ThreadStage::Landing)
            {
                out.insert(r.machine_id, r.id);
            }
        }
        out
    }

    /// 이 시작을 막는 예약(사람이 읽는 까닭). `task`는 이 시작의 할 일 ULID
    pub(crate) fn reservation_block(&self, mid: &str, project: &str, task: Option<&str>) -> Option<String> {
        let r = self.machine_reservation(mid)?;
        if admits(&r, project, task) {
            return None;
        }
        let note = r.note.as_deref().map(|n| format!(" · {n}")).unwrap_or_default();
        Some(format!("{} 독점 예약({}{note})", self.machine_name(mid), r.holder))
    }

    /// thread 시작 직전 판정: 예약이 막으면 `start_wait`(예약 큐·자동화는 기다리고, 직접 시작은 까닭을 본다)
    pub(crate) fn check_machine_reservation(&self, a: &crate::orch::StartArgs, mid: &str) -> Result<(), ErrorBody> {
        if a.machine_override.is_some() {
            return Ok(());
        }
        let task = a.start_task.as_ref().and_then(|t| t.id.as_deref());
        match self.reservation_block(mid, &a.project, task) {
            None => Ok(()),
            Some(why) => Err(err(
                ErrorCode::Unavailable,
                format!("start_wait: {why} 풀림 대기 · 다른 머신을 고르거나 사용자 지시로 --ignore-reservation"),
            )),
        }
    }

    /// 예약 무시 시작의 권한(사용자, 또는 사용자 지시 근거를 준 그 프로젝트 coordinator)
    pub(crate) fn reservation_override(
        &self,
        project: &str,
        ignore: bool,
        user_request: Option<&str>,
        actor: &str,
    ) -> Result<Option<String>, ErrorBody> {
        if !ignore {
            return Ok(None);
        }
        let reason = line("사용자 지시 근거", user_request.map(str::to_string))?;
        let (by, reason) = self.reserve_actor(project, actor, reason)?;
        Ok(Some(match reason {
            Some(r) => format!("{by} · {r}"),
            None => by,
        }))
    }

    /// 예약을 걸고 푸는 쪽(표시 이름과 사용자 지시 근거)
    fn reserve_actor(
        &self,
        project: &str,
        actor: &str,
        reason: Option<String>,
    ) -> Result<(String, Option<String>), ErrorBody> {
        if self.coordinator_project_of_actor(actor).as_deref() == Some(project) {
            if reason.is_none() {
                return Err(err(
                    ErrorCode::Forbidden,
                    "coordinator는 사용자가 머신 예약을 말했을 때만 --user-request '<사용자 지시>'와 함께 한다",
                ));
            }
            return Ok(("coordinator".into(), reason));
        }
        if actor.starts_with("device:") || (actor.starts_with("local:") && !crate::front::in_session(actor)) {
            return Ok(("사용자".into(), reason));
        }
        Err(err(
            ErrorCode::Forbidden,
            "머신 예약은 사용자, 사용자 지시 근거를 준 그 프로젝트 coordinator, 자기 머신을 잡는 thread(`aaa machine reserve`)만 한다",
        ))
    }

    /// 예약 기록을 쓴다. 다른 주인의 예약이 있으면 거부한다(먼저 푼다)
    fn reservation_put(&self, mid: &str, r: MachineReservation, actor: &str) -> Result<(), ErrorBody> {
        if let Some(old) = self.machine_reservation(mid)
            && (old.project != r.project || old.holder != r.holder)
        {
            return Err(err(
                ErrorCode::Conflict,
                format!("{}은 이미 {}가 예약했다. 먼저 푼다", self.machine_name(mid), old.holder),
            ));
        }
        self.db.set_meta(&key(mid), &serde_json::to_string(&r).map_err(internal)?).map_err(internal)?;
        self.db.audit(
            actor,
            "machine_reserve",
            &format!(
                "{} {}/{} by={} until={:?} note={:?} user_request={:?}",
                self.machine_name(mid),
                r.project,
                r.holder,
                r.by,
                r.until_ms,
                r.note,
                r.user_request
            ),
        );
        self.bump();
        Ok(())
    }

    fn reservation_delete(&self, mid: &str, r: &MachineReservation, actor: &str) -> Result<(), ErrorBody> {
        self.db.del_meta(&key(mid)).map_err(internal)?;
        self.db.audit(actor, "machine_release", &format!("{} {}/{}", self.machine_name(mid), r.project, r.holder));
        self.bump();
        self.tick_wake.notify_one();
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn machine_reserve(
        &self,
        machine: &str,
        project: &str,
        holder: &str,
        note: Option<String>,
        until_ms: Option<u64>,
        user_request: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let mid = self.resolve_machine(Some(machine))?;
        self.project_dir(project)?;
        let (by, user_request) = self.reserve_actor(project, actor, line("사용자 지시 근거", user_request)?)?;
        let note = line("까닭", note)?;
        let now = aaa_common::util::now_ms();
        if until_ms.is_some_and(|u| u <= now) {
            return Err(bad("끝 시각이 이미 지났다"));
        }
        let holder = holder.trim();
        let (holder, task_id) = if let Some(id) = aaa_proto::canonical_short_id(holder, 't') {
            let rec = self.thread_show(project, &id)?;
            if !matches!(rec.status, ThreadStatus::Open | ThreadStatus::Starting | ThreadStatus::Parked) {
                return Err(bad(format!("{id}는 끝난 thread다")));
            }
            (id, None)
        } else {
            let board = self.task_board(project)?;
            let t = crate::tasks::find(&board.tasks, holder)
                .ok_or_else(|| bad(format!("주인은 t-NNNN thread나 활성 할 일이다: {holder}")))?;
            let ulid = t.id.clone().ok_or_else(|| internal("할 일에 ID가 없다"))?;
            (t.short_id.clone().unwrap_or_else(|| ulid.clone()), Some(ulid))
        };
        let r = MachineReservation {
            project: project.into(),
            holder,
            task_id,
            note,
            until_ms,
            by,
            user_request,
            at_ms: now,
        };
        self.reservation_put(&mid, r, actor)?;
        Ok(FrontResponse::Machines { machines: self.machine_views() })
    }

    pub(crate) fn machine_release(
        &self,
        machine: &str,
        user_request: Option<String>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let mid = self.resolve_machine(Some(machine))?;
        let r = self
            .machine_reservation(&mid)
            .ok_or_else(|| err(ErrorCode::NotFound, format!("{machine}에 걸린 예약이 없다")))?;
        self.reserve_actor(&r.project, actor, line("사용자 지시 근거", user_request)?)?;
        self.reservation_delete(&mid, &r, actor)?;
        Ok(FrontResponse::Machines { machines: self.machine_views() })
    }

    /// 사람만(call_from이 막는다). None은 CPU 수로 되돌린다
    pub(crate) fn machine_weight_set(
        &self,
        machine: &str,
        weight: Option<u32>,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let mid = self.resolve_machine(Some(machine))?;
        if weight.is_some_and(|w| w > 1024) {
            return Err(bad("비중은 0~1024다"));
        }
        match weight {
            Some(w) => self.db.set_meta(&weight_key(&mid), &w.to_string()),
            None => self.db.del_meta(&weight_key(&mid)),
        }
        .map_err(internal)?;
        self.db.audit(actor, "machine_weight", &format!("{machine} {weight:?}"));
        self.bump();
        self.tick_wake.notify_one();
        Ok(FrontResponse::Machines { machines: self.machine_views() })
    }

    /// thread가 post로 자기 머신을 잡거나 푼다(`machine-hold`·`machine-release` 주제). 본문은
    /// `{"machine":이름,"note":…,"until_ms":…}`이다. 거부는 까닭 글이다
    pub(crate) fn thread_machine_post(&self, rec: &ThreadRec, input: &PostInput) -> Result<(), String> {
        #[derive(serde::Deserialize)]
        struct Body {
            machine: String,
            #[serde(default)]
            note: Option<String>,
            #[serde(default)]
            until_ms: Option<u64>,
        }
        let b: Body = serde_json::from_str(&input.body).map_err(|e| format!("머신 예약 본문 형식: {e}"))?;
        let mid = self.resolve_machine(Some(&b.machine)).map_err(|e| e.message)?;
        if mid != rec.machine_id {
            return Err(format!(
                "thread는 자기 머신만 예약한다: {}는 {}에서 돈다",
                rec.id,
                self.machine_name(&rec.machine_id)
            ));
        }
        let actor = format!("post:{}@{}", rec.id, self.machine_name(&mid));
        if input.topic == "machine-release" {
            let r = self.machine_reservation(&mid).ok_or("이 머신에 걸린 예약이 없다")?;
            if r.project != rec.project || r.holder != rec.id {
                return Err(format!("{}의 예약이다. thread는 자기 예약만 푼다", r.holder));
            }
            return self.reservation_delete(&mid, &r, &actor).map_err(|e| e.message);
        }
        let now = aaa_common::util::now_ms();
        if b.until_ms.is_some_and(|u| u <= now) {
            return Err("끝 시각이 이미 지났다".into());
        }
        let r = MachineReservation {
            project: rec.project.clone(),
            holder: rec.id.clone(),
            task_id: None,
            note: line("까닭", b.note).map_err(|e| e.message)?,
            until_ms: b.until_ms,
            by: rec.id.clone(),
            user_request: None,
            at_ms: now,
        };
        self.reservation_put(&mid, r, &actor).map_err(|e| e.message)
    }
}

#[cfg(test)]
#[path = "machine_reserve/tests.rs"]
mod tests;
