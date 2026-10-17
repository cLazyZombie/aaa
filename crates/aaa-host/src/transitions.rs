//! 묶음 전이와 thread 항목(herdr-projects ticker.rs `thread_pass`·steps.rs `write_thread_items`, plan §8.85).
//!
//! - ticker가 열린 thread마다 묶음을 다시 셈해 기록(`last_group`)과 견준다. 바뀌었으면 전이다.
//! - 사람 차례·쉼으로 바뀔 때만 `thread-state` 항목 한 건을 낸다(병합 중은 D11). 사람 차례는 알림(push)도 간다.
//! - 「새 보고」 항목은 묶음이 검토 대기일 때 보고 hash당 한 번이다. 작업 중에 고쳐 쓴 보고는 항목을 만들지 않는다.
//! - 한 머신에 기록된 pane이 둘 이상이고 모두 사라졌으면(agent 재시작·재부팅) thread마다가 아니라 그 머신의 `session`
//!   항목 한 건이다(참고는 herdr 세션 하나를 본다. aaa는 세션이 머신마다 있어 머신마다 센다).
//! - 연결되지 않은 머신의 thread는 읽지 않는다: 상태도 묶음도 항목도 바꾸지 않는다(herdr-projects `remote_pass`).

use std::collections::{BTreeMap, HashSet};

use aaa_proto::{AgentActivity, AgentStatus, Connection, Role, ThreadGroup, ThreadStatus};

use crate::Host;
use crate::orch::{ResolveWhy, ThreadRec, WAITING, thread_group};

/// 이 수 이상 기록된 pane이 한 머신에서 모두 사라지면 세션 한 건으로 알린다(herdr-projects `session_lost`)
const SESSION_LOST_PANES: usize = 2;

/// 알림(push·webhook)으로 내보내는 `thread-state` 사건. 참고는 사람 차례 전이와 새 보고에만 notifier를 부른다.
/// 시작 실패(`not launched`)는 참고가 실패한 thread를 묶음 셈에서 빼서 알리지 않지만, 사람이 할 일이라 aaa는 알린다.
pub(crate) const PUSH_EVENTS: [&str; 6] =
    ["blocked on a prompt", "pane closed", "agent gone", "waiting on you", "new report", "not launched"];

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
/// 보고 사본 실패는 aaa에만 있는 사람 차례 까닭이다.
pub(crate) fn note(pane_exists: bool, agent: Option<&AgentStatus>, copy_failed: bool) -> String {
    if !pane_exists {
        return "pane closed".into();
    }
    if copy_failed {
        return "copy failed".into();
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

/// 전이를 항목으로 내는가: 사람 차례·쉼만(herdr-projects `write_thread_items`)
pub(crate) fn writes_item(to: ThreadGroup) -> bool {
    matches!(to, ThreadGroup::WaitingOnYou | ThreadGroup::Idle)
}

/// 전이 항목 요약. 확인창이면 답하는 길을, 그 밖의 사람 차례면 pane과 머신을 적는다.
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
        let machines = self.machine_views();
        let connected: HashSet<&str> =
            machines.iter().filter(|m| m.connection == Connection::Connected).map(|m| m.id.as_str()).collect();
        let remote_name = |mid: &str| machines.iter().find(|m| m.id == mid && !m.local).map(|m| m.name.clone());
        let listed: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
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
            let Ok(mut r) = self.load(project, &id) else { continue };
            if !matches!(r.status, ThreadStatus::Open | ThreadStatus::Starting) {
                continue;
            }
            let (pane_exists, agent) = self.thread_live(&r);
            let group = thread_group(&r, pane_exists, agent.as_ref(), now);
            let mut change = None;
            if r.last_group != Some(group) {
                match r.last_group {
                    Some(_) => {
                        let copy_failed = r.copy.as_deref() == Some("failed");
                        change = Some(Transition { to: group, note: note(pane_exists, agent.as_ref(), copy_failed) });
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
            self.new_report_item(project, r);
        }
    }

    /// 쉼으로 `auto_resolve_days`가 지난 thread를 마지막 사본 뒤 끝낸다(herdr-projects steps.rs `auto_resolve`).
    /// 사본이 실패하면 끝내지 않고 다음 셈에 다시 한다. 연결되지 않은 머신의 thread는 건너뛴다.
    pub(crate) async fn auto_resolve_pass(self: &std::sync::Arc<Self>, project: &str) {
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
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| r.status == ThreadStatus::Open && r.role != Role::Evaluator)
            .filter(|r| r.last_group == Some(ThreadGroup::Idle) && self.live_conn(&r.machine_id).is_ok())
            .filter(|r| auto_resolve_due(r, now, self.started_ms, limit))
            .map(|r| r.id)
            .collect();
        for id in due {
            if let Err(e) = self.thread_resolve(project, &id, false, ResolveWhy::Auto { days }, "ticker").await {
                tracing::warn!(project, thread = %id, "자동 resolve를 다음 셈으로 미룬다: {}", e.message);
            }
        }
    }

    /// 검토 대기인 thread의 새 보고: 보고 hash당 한 번(herdr-projects `write_thread_items` Ready for review).
    /// 작업 중과 쉼을 오가는 같은 보고는 아무것도 만들지 않는다.
    fn new_report_item(&self, project: &str, r: &ThreadRec) {
        if r.status != ThreadStatus::Open || r.report_hash.is_none() || r.report_hash == r.review_item_hash {
            return;
        }
        if r.last_group != Some(ThreadGroup::ReadyForReview) {
            return;
        }
        let short: String = r.report_hash.as_deref().unwrap_or_default().chars().take(12).collect();
        let mut summary = format!("{}: 새 보고 threads/{}.md(보고 {short})", label(r), r.id);
        if !r.copy_notes.is_empty() {
            summary.push_str(&format!("; 다 옮기지 못했다: {}", r.copy_notes.join("; ")));
        }
        let hash = r.report_hash.clone();
        if self.update_rec(project, &r.id, |x| x.review_item_hash = hash).is_ok() {
            self.inbox_add(project, "thread-state", &r.id, "new report", &summary);
        }
    }

    /// 한 머신의 기록된 pane이 모두 사라졌다: 세션 항목 한 건. 다시 하나라도 살면(또는 다시 시작하면) 잊는다.
    fn session_lost_item(&self, project: &str, mid: &str, lost: bool, threads: &[String]) {
        let key = format!("sessions_lost:{project}:{mid}");
        let written = self.db.meta(&key).ok().flatten().is_some();
        if !lost {
            if written {
                let _ = self.db.del_meta(&key);
            }
            return;
        }
        if written {
            return;
        }
        let _ = self.db.set_meta(&key, "1");
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
mod tests {
    /// herdr-projects steps.rs `auto_resolve`: 묶음·보고가 바뀐 때와 ticker(host)가 뜬 때 중 가장 늦은 때부터 잰다
    #[test]
    fn auto_resolve_counts_from_the_latest_change_or_host_start() {
        let mut r: super::ThreadRec = serde_json::from_value(serde_json::json!({
            "project": "p", "id": "t-0001", "title": "t", "machine_id": "m", "profile": "x", "role": "worker",
            "kind": "dir", "status": "open", "brief": "delivered", "created_at_ms": 0, "updated_at_ms": 0
        }))
        .unwrap();
        let due = |r: &super::ThreadRec, now, started| super::auto_resolve_due(r, now, started, 100);
        assert!(!due(&r, 1_000, 0), "잴 시각이 없는 옛 기록은 끝내지 않는다");
        r.group_changed_ms = Some(500);
        assert!(due(&r, 600, 0) && !due(&r, 599, 0));
        r.report_changed_ms = Some(550);
        assert!(!due(&r, 600, 0), "보고가 더 늦게 바뀌었다");
        assert!(!due(&r, 1_000, 950), "host가 방금 떴다: 꺼져 있던 동안은 세지 않는다");
        assert!(due(&r, 1_050, 950));
    }

    use super::*;

    fn agent(activity: AgentActivity) -> AgentStatus {
        AgentStatus {
            activity,
            since_ms: 0,
            harness: "claude".into(),
            message: None,
            percent: None,
            progress: None,
            progress_at_ms: None,
            harness_session: None,
            seen: true,
        }
    }

    #[test]
    fn sessions_are_lost_only_when_every_recorded_pane_of_two_or_more_is_gone() {
        let m = |recorded, missing| MachinePanes { recorded, missing, threads: vec![] };
        // 하나뿐인 pane이 닫힌 것은 그 thread의 전이(pane closed)다
        assert!(!m(1, 1).lost());
        assert!(!m(2, 1).lost() && !m(3, 2).lost());
        assert!(m(2, 2).lost() && m(3, 3).lost());
        assert!(!m(0, 0).lost());
    }

    #[test]
    fn notes_and_events_follow_the_reference() {
        let q = agent(AgentActivity::Question);
        assert_eq!(note(false, Some(&q), true), "pane closed");
        assert_eq!(note(true, Some(&q), true), "copy failed");
        assert_eq!(note(true, None, false), "no agent");
        assert_eq!(note(true, Some(&q), false), "blocked");
        let t = |to, note: &str| Transition { to, note: note.into() };
        assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "blocked")), "blocked on a prompt");
        assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "pane closed")), "pane closed");
        assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "no agent")), "agent gone");
        assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "idle")), "waiting on you");
        assert_eq!(transition_event(&t(ThreadGroup::Idle, "idle")), "idle");
        assert!(writes_item(ThreadGroup::WaitingOnYou) && writes_item(ThreadGroup::Idle));
        assert!(!writes_item(ThreadGroup::Working) && !writes_item(ThreadGroup::ReadyForReview));
        assert!(!writes_item(ThreadGroup::Resolved));
    }
}
