//! 묶음 전이와 thread 항목(herdr-projects ticker.rs `thread_pass`·steps.rs `write_thread_items`, plan §8.85).
//!
//! - ticker가 열린 thread마다 묶음을 다시 셈해 기록(`last_group`)과 견준다. 바뀌었으면 전이다.
//! - 확인 필요·쉼으로 바뀔 때만 `thread-state` 항목 한 건을 낸다. 앱 안 알림에 보이며 외부 푸시는 coordinator가 판단한다.
//! - 「새 보고」 항목은 묶음이 검토 대기일 때 보고 hash당 한 번이다. 작업 중에 고쳐 쓴 보고는 항목을 만들지 않는다.
//! - 한 머신에 기록된 pane이 둘 이상이고 모두 사라졌으면(agent 재시작·재부팅) thread마다가 아니라 그 머신의 `session`
//!   항목 한 건이다(참고는 herdr 세션 하나를 본다. aaa는 세션이 머신마다 있어 머신마다 센다).
//! - 연결되지 않은 머신의 thread는 읽지 않는다: 상태도 묶음도 항목도 바꾸지 않는다(herdr-projects `remote_pass`).

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use aaa_proto::{AgentActivity, AgentStatus, Connection, Role, ThreadGroup, ThreadStatus};

use crate::Host;
use crate::orch::{ResolveWhy, ThreadRec, WAITING, thread_group_with};

/// 이 수 이상 기록된 pane이 한 머신에서 모두 사라지면 세션 한 건으로 알린다(herdr-projects `session_lost`)
const SESSION_LOST_PANES: usize = 2;

/// 묶음 하나의 바뀜(herdr-projects `Transition`)
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Transition {
    pub to: ThreadGroup,
    pub note: String,
}

/// 항목 대상 이름: `t-0001 "제목"`(herdr-projects `thread_label`)
pub(crate) fn label(r: &ThreadRec) -> String {
    format!("{} \"{}\"", r.id, r.title)
}

/// 전이 note: pane이 없으면 pane closed, agent 상태가 없으면 no agent, 그 밖은 harness 상태(herdr-projects ticker).
/// 보고 사본 실패는 시스템 경고이므로 전이 까닭에 섞지 않는다.
pub(crate) fn note(pane_exists: bool, agent: Option<&AgentStatus>) -> String {
    if !pane_exists {
        return "pane closed".into();
    }
    match agent.map(|a| a.activity) {
        None => "no agent",
        Some(AgentActivity::Started) => "started",
        Some(AgentActivity::Working) => "working",
        Some(AgentActivity::Idle) => "idle",
        Some(AgentActivity::Question) => "blocked",
        Some(AgentActivity::Error) => "error",
        Some(AgentActivity::Exited) => "exited",
    }
    .into()
}

/// `thread-state` 항목의 사건(herdr-projects steps.rs `transition_event`). 영어 낱말은 참고와 같다(nudge 줄에 쓴다).
pub(crate) fn transition_event(t: &Transition) -> &'static str {
    match (t.to, t.note.as_str()) {
        (ThreadGroup::WaitingOnYou, "blocked") => "blocked on a prompt",
        (ThreadGroup::WaitingOnYou, "pane closed") => "pane closed",
        (ThreadGroup::WaitingOnYou, "no agent") => "agent gone",
        (ThreadGroup::WaitingOnYou, _) => "waiting on you",
        (ThreadGroup::Idle, _) => "idle",
        _ => "changed state",
    }
}

/// 전이를 항목으로 내는가: 확인 필요·쉼만(herdr-projects `write_thread_items`)
pub(crate) fn writes_item(to: ThreadGroup) -> bool {
    matches!(to, ThreadGroup::WaitingOnYou | ThreadGroup::Idle)
}

/// 전이 항목 요약. 확인창이면 답하는 길을, 그 밖의 확인 필요면 pane과 머신을 적는다.
pub(crate) fn transition_summary(
    project: &str,
    r: &ThreadRec,
    t: &Transition,
    machine: Option<&str>,
    agent: Option<&AgentStatus>,
) -> String {
    let mut s = format!("{}: {}({})", label(r), t.to.label(), t.note);
    if t.to != ThreadGroup::WaitingOnYou {
        return s;
    }
    if agent.is_some_and(|a| a.progress.as_deref() == Some(WAITING)) {
        s.push_str(&format!(": agent가 「{WAITING}」라고 알렸다"));
    }
    let Some(pane) = r.pane.as_deref().filter(|_| t.note != "pane closed") else {
        if t.note == "pane closed" {
            s.push_str(&format!(". 다시 시작: aaa thread restart {project} {}", r.id));
        }
        return s;
    };
    let on = machine.map(|m| format!(" (머신 {m})")).unwrap_or_default();
    if t.note == "blocked" {
        s.push_str(&format!(
            "; pane {pane}{on}에 확인창이 떠 있다: `aaa thread read {project} {id}`로 보고 `aaa thread keys {project} {id}`로 답한다",
            id = r.id
        ));
    } else {
        s.push_str(&format!("; pane {pane}{on}에서 사람이 필요하다"));
    }
    s
}

/// 전이 pass 하나가 본 머신 하나(기록된 pane 수, 사라진 pane 수, 그 thread들)
#[derive(Default)]
struct MachinePanes {
    recorded: usize,
    missing: usize,
    threads: Vec<String>,
}

impl MachinePanes {
    /// 세션이 한꺼번에 사라졌다(herdr-projects ticker `session_lost`: 기록된 pane이 둘 이상이고 모두 없다)
    fn lost(&self) -> bool {
        self.recorded >= SESSION_LOST_PANES && self.missing == self.recorded
    }
}

fn auto_resolve_eligible(r: &ThreadRec) -> bool {
    // 기획 대화 thread는 끝내지 않고 쉬게 한다(chat_rest.rs `chat_rest_pass`)
    r.status == ThreadStatus::Open
        && r.role != Role::Evaluator
        && !r.chat_only()
        && r.last_group == Some(ThreadGroup::Idle)
}

/// auto-resolve 때가 됐나: 묶음·보고가 마지막으로 바뀐 때와 host가 뜬 때 중 가장 늦은 때부터 `limit_ms`가 지났다
/// (herdr-projects steps.rs `auto_resolve`). 두 시각이 모두 없는 기록(옛 host)은 잴 수 없어 끝내지 않는다.
pub(crate) fn auto_resolve_due(r: &ThreadRec, now: u64, started: u64, limit_ms: u64) -> bool {
    let Some(since) =
        [r.group_changed_ms, r.report_changed_ms].into_iter().flatten().map(|t| now.saturating_sub(t)).min()
    else {
        return false;
    };
    since.min(now.saturating_sub(started)) >= limit_ms
}

impl Host {
    /// 한 프로젝트의 묶음 전이와 thread 항목(ticker 한 번). 기록은 잠금 안에서 다시 읽고 고친다
    /// (같은 때 resolve·ack가 쓴 필드를 옛 값으로 덮지 않고, 끝낸 thread를 닫힌 pane으로 보지 않게).
    pub(crate) fn thread_pass(&self, project: &str) {
        let now = aaa_common::util::now_ms();
        let delivered_ms = self.cfg.projects.delivered_working_secs * 1000;
        let machines = self.machine_views();
        let connected: HashSet<&str> =
            machines.iter().filter(|m| m.connection == Connection::Connected).map(|m| m.id.as_str()).collect();
        let remote_name = |mid: &str| machines.iter().find(|m| m.id == mid && !m.local).map(|m| m.name.clone());
        let listed: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .filter(|r| matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting) && r.role != Role::Evaluator)
            .filter(|r| connected.contains(r.machine_id.as_str()))
            // 재부팅 뒤 복원 중인 머신의 thread는 끝난 뒤에 센다(사라졌다가 돌아온 것으로 보지 않게, restore.rs)
            .filter(|r| !self.restoring.lock().unwrap().contains(&r.machine_id))
            .collect();
        // 머신마다 기록된 pane과 사라진 pane(보고를 다시 확인하는 thread도 센다: 세션은 한꺼번에 끝난다)
        let mut per_machine: BTreeMap<String, MachinePanes> = BTreeMap::new();
        for r in listed.iter().filter(|r| r.pane.is_some()) {
            let m = per_machine.entry(r.machine_id.clone()).or_default();
            m.recorded += 1;
            m.missing += usize::from(!self.thread_live(r).0);
            m.threads.push(r.id.clone());
        }
        let lost: HashSet<String> = per_machine.iter().filter(|(_, m)| m.lost()).map(|(mid, _)| mid.clone()).collect();
        let mut seen: Vec<(ThreadRec, Option<Transition>, Option<AgentStatus>)> = Vec::new();
        for id in listed.into_iter().map(|r| r.id) {
            // 보고를 다시 확인하는 중이면 끝난 뒤에 센다(orch.rs `refresh_report`)
            if self.report_refresh.lock().unwrap().contains_key(&format!("{project}/{id}")) {
                continue;
            }
            let _guard = self.rec_lock.lock().unwrap();
            let Ok(mut r) = self.load(project, &id).inspect_err(|error| {
                aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다")
            }) else {
                continue;
            };
            if !matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting) {
                continue;
            }
            let (pane_exists, agent) = self.thread_live(&r);
            let group = thread_group_with(
                &r,
                pane_exists,
                agent.as_ref(),
                now,
                delivered_ms,
                (self.cfg.host.thread_prepare_timeout_secs + 60) * 1000,
            );
            let mut change = None;
            if r.last_group != Some(group) {
                match r.last_group {
                    Some(_) => {
                        change = Some(Transition { to: group, note: note(pane_exists, agent.as_ref()) });
                    }
                    // 처음 본다(새 thread·host를 바꾼 직후): 전이가 아니다. 이미 있던 보고는 옛 방식으로 알렸으니
                    // 「새 보고」를 다시 내지 않는다.
                    None => r.review_item_hash = r.report_hash.clone(),
                }
                r.last_group = Some(group);
                r.group_changed_ms = Some(now);
                self.save(&mut r);
            }
            seen.push((r, change, agent));
        }
        for (mid, m) in &per_machine {
            self.session_lost_item(project, mid, lost.contains(mid), &m.threads);
        }
        for (r, change, agent) in &seen {
            if lost.contains(&r.machine_id) {
                continue;
            }
            if let Some(t) = change.as_ref().filter(|t| writes_item(t.to)) {
                let machine = remote_name(&r.machine_id);
                let summary = transition_summary(project, r, t, machine.as_deref(), agent.as_ref());
                self.inbox_add(project, "thread-state", &r.id, transition_event(t), &summary);
            }
            let reported = self.new_report_item(project, r);
            // 같은 턴의 끝남은 새 보고 알림 하나로(notice.rs)
            if let Some(pane) = r.pane.as_deref() {
                self.settle_turn_end(pane, reported);
            }
        }
    }

    /// 쉼으로 `auto_resolve_days`가 지난 thread를 마지막 사본 뒤 끝낸다(herdr-projects steps.rs `auto_resolve`).
    /// 사본이 실패하면 끝내지 않고 다음 셈에 다시 한다. 연결되지 않은 머신의 thread는 건너뛴다.
    pub(crate) async fn auto_resolve_pass(self: &Arc<Self>, project: &str) {
        let days = self.settings(project).auto_resolve_days;
        if days == 0 {
            return;
        }
        let limit = u64::from(days) * self.cfg.projects.auto_resolve_day_secs * 1000;
        let now = aaa_common::util::now_ms();
        let due: Vec<String> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .filter(auto_resolve_eligible)
            .filter(|r| {
                self.live_conn(&r.machine_id)
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
            })
            .filter(|r| auto_resolve_due(r, now, self.started_ms, limit))
            .map(|r| r.id)
            .collect();
        for id in due {
            if let Err(e) = self
                .thread_resolve(project, &id, Default::default(), ResolveWhy::Auto { days: Some(days) }, "ticker")
                .await
            {
                tracing::warn!(project, thread = %id, "자동 resolve를 다음 셈으로 미룬다: {}", e.message);
            }
        }
    }

    /// 검토 대기인 thread의 새 보고: 보고 hash당 한 번(herdr-projects `write_thread_items` Ready for review).
    /// 계속 작업하는 thread의 착지·완료·확인 필요도 hash당 한 번 inbox에 남긴다. push는 완료·확인 필요만이다. 상태·진행률은 그대로다.
    /// 작업 중과 쉼을 오가는 같은 보고는 아무것도 만들지 않는다. 낸 항목은 앱 안 알림으로도 보낸다(notice.rs). 냈으면 true
    pub(crate) fn new_report_item(&self, project: &str, r: &ThreadRec) -> bool {
        if r.status != ThreadStatus::Open || r.report_hash.is_none() || r.report_hash == r.review_item_hash {
            return false;
        }
        let attention = self.report_attention(r);
        if !matches!(r.last_group, Some(ThreadGroup::ReadyForReview))
            && attention.is_none()
            && !matches!(self.report_marker(r), Some("[착지]" | "[병합 준비됨]"))
        {
            return false;
        }
        let short: String = r.report_hash.as_deref().unwrap_or_default().chars().take(12).collect();
        let mut summary = format!("{}: 새 보고 threads/{}.md(보고 {short})", label(r), r.id);
        if !r.copy_notes.is_empty() {
            summary.push_str(&format!("; 다 옮기지 못했다: {}", r.copy_notes.join("; ")));
        }
        let ids = self
            .thread_view(r)
            .questions
            .map(|q| q.items.iter().filter_map(|q| q.id.as_deref()).collect::<Vec<_>>().join(" "))
            .unwrap_or_default();
        if !ids.is_empty() {
            summary = format!("{ids} {summary}");
        }
        if let Some(id) = self.thread_report_id(project, &r.id, r.report_hash.as_deref()) {
            summary = format!("{id} {summary}");
        }
        let hash = r.report_hash.clone();
        if self
            .update_rec(project, &r.id, |x| x.review_item_hash = hash)
            .as_ref()
            .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
            .is_err()
        {
            return false;
        }
        let event =
            if attention == Some(aaa_proto::ReportAttention::NeedsUser) { "report needs user" } else { "new report" };
        self.inbox_add(project, "thread-state", &r.id, event, &summary);
        if attention.is_some() && !(r.automation.is_some() && attention == Some(aaa_proto::ReportAttention::Completed))
        {
            self.report_notice(project, r);
        }
        true
    }

    /// 한 머신의 기록된 pane이 모두 사라졌다: 세션 항목 한 건. 다시 하나라도 살면(또는 다시 시작하면) 잊는다.
    fn session_lost_item(&self, project: &str, mid: &str, lost: bool, threads: &[String]) {
        let key = format!("sessions_lost:{project}:{mid}");
        let written = self
            .db
            .meta(&key)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .is_some();
        if !lost {
            if written {
                aaa_common::diagnostic::ignored(self.db.del_meta(&key), "보조 작업 실패 뒤 기존 처리를 계속한다");
            }
            return;
        }
        if written {
            return;
        }
        aaa_common::diagnostic::ignored(self.db.set_meta(&key, "1"), "보조 작업 실패 뒤 기존 처리를 계속한다");
        let name = self.machine_name(mid);
        let summary = format!(
            "머신 {name}의 thread 세션이 모두 끝났다(agent 재시작·재부팅). thread {}개가 `aaa thread restart {project} <id>`를 기다린다: {}",
            threads.len(),
            threads.join(", ")
        );
        self.inbox_add(project, "session", &name, "sessions ended", &summary);
    }
}

#[cfg(test)]
mod tests;
