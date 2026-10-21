//! 묶음 전이와 thread 항목(herdr-projects ticker.rs `thread_pass`·steps.rs `write_thread_items`, plan §8.85).
//!
//! - ticker가 열린 thread마다 묶음을 다시 셈해 기록(`last_group`)과 견준다. 바뀌었으면 전이다.
//! - 확인 필요·병합 중·쉼으로 바뀔 때만 `thread-state` 항목 한 건을 낸다. 앱 안 알림에 보이며 외부 푸시는 coordinator가 판단한다.
//! - 「새 보고」 항목은 묶음이 검토 대기나 병합 중일 때 보고 hash당 한 번이다. 작업 중에 고쳐 쓴 보고는 항목을 만들지 않는다.
//! - PR이 병합된 thread는 agent가 끝난 뒤 저절로 끝낸다(`resolve_merged_pass`, herdr-projects steps.rs `resolve_merged`).
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
        (ThreadGroup::Landing, _) => "landing",
        (ThreadGroup::Idle, _) => "idle",
        _ => "changed state",
    }
}

/// 전이를 항목으로 내는가: 확인 필요·병합 중·쉼만(herdr-projects `write_thread_items`)
pub(crate) fn writes_item(to: ThreadGroup) -> bool {
    matches!(to, ThreadGroup::WaitingOnYou | ThreadGroup::Landing | ThreadGroup::Idle)
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

/// 병합된 thread를 끝내도 되나(herdr-projects steps.rs `merged_thread_may_resolve`). `seen`은 ticker가 병합을 처음 본
/// 때다. `last_group`은 같은 tick의 `thread_pass`가 막 셈했다. 병합은 agent를 멈추지 않는다: tag·배포·다른 PR·마지막
/// 보고가 남았을 수 있다.
pub(crate) fn merged_may_resolve(
    r: &ThreadRec,
    agent: Option<&AgentStatus>,
    seen: u64,
    now: u64,
    grace_ms: u64,
) -> bool {
    if matches!(r.last_group, Some(ThreadGroup::Working | ThreadGroup::WaitingOnYou)) {
        return false;
    }
    // 진행을 보고하는 agent는 끝을 스스로 말한다: `Done`(100)만 끝이고, 시간이 낮은 진행을 넘지 않는다.
    // 배경 셸로 CI·배포를 기다리면 쉼으로 읽힌다.
    if let Some(a) = agent.filter(|a| a.progress_at_ms.is_some()) {
        return a.percent == Some(100);
    }
    r.report_changed_ms.is_some_and(|t| t > seen) || now.saturating_sub(seen) >= grace_ms
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

    /// PR이 병합된 thread를 마지막 사본 뒤 끝낸다(herdr-projects steps.rs `resolve_merged`): agent가 일하거나 사람을
    /// 기다리지 않고, 끝났고(`merged_may_resolve`), 보고가 이름 붙인 다른 PR이 열려 있지 않을 때. 사본이 실패하면 다음 셈에
    /// 다시 한다. 연결되지 않은 머신의 thread는 건너뛴다.
    pub(crate) async fn resolve_merged_pass(self: &Arc<Self>, project: &str) {
        // 참고는 한 tick 안에서 PR 조회(병합 기록·routine 발화) 뒤에 끝낸다. aaa의 PR 조회는 따로 돌아(`pr_tick`) 그 조회가
        // 보내는 routine 글(처음 본 PR이 이미 승인·병합이면 `review`)을 끝내기가 자를 수 있다: 조회가 도는 동안은 끝내지 않는다
        if self.pr_running(project) {
            return;
        }
        let now = aaa_common::util::now_ms();
        let grace = self.cfg.projects.merge_grace_secs * 1000;
        let merged: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| r.status == ThreadStatus::Open && r.role != Role::Evaluator)
            .filter(|r| r.pr_state.as_deref().is_some_and(|s| s.eq_ignore_ascii_case("merged")))
            .filter(|r| self.live_conn(&r.machine_id).is_ok())
            .collect();
        for r in merged {
            if self.automation_message_pending(project, &r.id) {
                continue;
            }
            let seen = match r.merged_seen_ms {
                Some(t) => t,
                None => {
                    let _ = self.update_rec(project, &r.id, |x| x.merged_seen_ms = Some(now));
                    now
                }
            };
            let (_, agent) = self.thread_live(&r);
            if !merged_may_resolve(&r, agent.as_ref(), seen, now, grace) || self.other_pr_open(project, &r).await {
                continue;
            }
            if let Err(e) = self.thread_resolve(project, &r.id, Default::default(), ResolveWhy::Merged, "ticker").await
            {
                tracing::warn!(project, thread = %r.id, "병합 뒤 resolve를 다음 셈으로 미룬다: {}", e.message);
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
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| r.status == ThreadStatus::Open && r.role != Role::Evaluator)
            .filter(|r| r.last_group == Some(ThreadGroup::Idle) && self.live_conn(&r.machine_id).is_ok())
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

    /// 검토 대기·병합 중인 thread의 새 보고: 보고 hash당 한 번(herdr-projects `write_thread_items` Ready for review).
    /// 계속 작업하는 thread의 착지·완료·확인 필요도 hash당 한 번 inbox에 남긴다. push는 완료·확인 필요만이다. 상태·진행률은 그대로다.
    /// 작업 중과 쉼을 오가는 같은 보고는 아무것도 만들지 않는다. 낸 항목은 앱 안 알림으로도 보낸다(notice.rs). 냈으면 true
    pub(crate) fn new_report_item(&self, project: &str, r: &ThreadRec) -> bool {
        if r.status != ThreadStatus::Open || r.report_hash.is_none() || r.report_hash == r.review_item_hash {
            return false;
        }
        let attention = self.report_attention(r);
        if !matches!(r.last_group, Some(ThreadGroup::ReadyForReview | ThreadGroup::Landing))
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
        let hash = r.report_hash.clone();
        if self.update_rec(project, &r.id, |x| x.review_item_hash = hash).is_err() {
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
    use super::*;

    /// Hello의 앞 pane 상태를 적용한 사건 경계에서 ticker를 돌린다. 뒤 pane의 저장된 Working을
    /// 아직 적용하지 않았다고 쉼(no agent) 알림을 만들면 안 된다. sleep·부하에 기대지 않는다.
    #[test]
    fn hello_replay_does_not_publish_idle_between_panes() {
        use aaa_common::conn::spawn_writer;
        use aaa_proto::{AgentHello, HarnessEvent, SessionInfo, SessionState};
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let roots = rt.block_on(async {
            let mut roots = Vec::new();
            for local in [true, false] {
                let root = std::env::temp_dir().join(format!("aaa-hello-{}", aaa_common::util::new_id()));
                let mut cfg = aaa_config::HostConfig::default();
                cfg.projects.root = root.join("projects").display().to_string();
                cfg.notify.web_push = false;
                let h = Host::open(cfg, root.join("host")).unwrap();
                h.project_create("p", Default::default(), "test").unwrap();
                h.db.upsert_machine("m", "mac-a", None, local).unwrap();
                let mut r = rec();
                r.pane = Some("s".into());
                r.last_group = Some(ThreadGroup::Working);
                h.db.thread_import("p", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
                let event = |session: &str, kind: &str, at_ms| HarnessEvent {
                    session: session.into(),
                    harness: "codex".into(),
                    kind: kind.into(),
                    harness_session: None,
                    message: None,
                    question_kind: None,
                    percent: None,
                    at_ms,
                    transcript: None,
                    plan: None,
                    background: None,
                    context: None,
                };
                let sessions = ["first", "s"].map(|id| SessionInfo {
                    id: id.into(),
                    name: "codex".into(),
                    argv: vec!["codex".into()],
                    cwd: None,
                    pid: None,
                    started_at_ms: 0,
                    boot_id: "boot".into(),
                    cols: 80,
                    rows: 24,
                    state: SessionState::Running,
                    role: None,
                    label: None,
                    harness: Some("codex".into()),
                });
                let (done, seen) = tokio::sync::oneshot::channel();
                let during = h.clone();
                h.waiters.lock().unwrap().insert(
                    "first".into(),
                    vec![(
                        &["working"],
                        done,
                        Some(Box::new(move || {
                            during.thread_pass("p");
                            let items = during.inbox("p", true, true).unwrap().items;
                            assert!(items.is_empty(), "Hello 복원 중 거짓 전이: {items:?}");
                            let r = during.load("p", "t-0001").unwrap();
                            assert_eq!(during.thread_view(&r).group, ThreadGroup::Working);
                        })),
                    )],
                );
                let (writer, _peer) = tokio::io::duplex(65536);
                let generation = h.next_gen();
                h.agent_connected(
                    &AgentHello {
                        proto: aaa_proto::AGENT_LINK,
                        machine_id: "m".into(),
                        machine_name: "mac-a".into(),
                        boot_id: "boot".into(),
                        os: "test".into(),
                        arch: "test".into(),
                        software: "test".into(),
                        features: vec![],
                        dev_mode: true,
                        sha256: None,
                        selfcheck: None,
                        sessions: sessions.to_vec(),
                        harness: vec![event("first", "working", 1), event("s", "working", 1)],
                    },
                    local,
                    crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), generation),
                );
                assert_eq!(seen.await.unwrap(), "working");
                h.thread_pass("p");
                assert!(h.inbox("p", true, true).unwrap().items.is_empty());
                assert_eq!(h.thread_view(&h.load("p", &r.id).unwrap()).group, ThreadGroup::Working);
                // 초기화가 끝난 뒤의 실제 쉼은 숨기지 않는다.
                h.on_harness("m", event("s", "idle", 2));
                tokio::time::timeout(std::time::Duration::from_secs(3), async {
                    loop {
                        let changed = h.tick_wake.notified();
                        tokio::pin!(changed);
                        changed.as_mut().enable();
                        if !h.report_refresh.lock().unwrap().contains_key("p/t-0001") {
                            break;
                        }
                        changed.await;
                    }
                })
                .await
                .unwrap();
                h.thread_pass("p");
                let items = h.inbox("p", true, true).unwrap().items;
                assert_eq!(items.len(), 1, "{items:?}");
                assert!(items[0].summary.contains("쉼(idle)"), "{items:?}");
                drop(h);
                roots.push(root);
            }
            roots
        });
        // agent_connected의 구독 task도 Host·DB를 잡고 있다. runtime을 먼저 끝내 열린 DB 파일을 닫는다.
        drop(rt);
        for root in roots {
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    /// 계속 일하는 thread도 명시한 착지는 알린다. adopt와 새로 띄운 thread의 보고 계약은 같다.
    /// 실제 사본·DB·inbox 경계에서 hash당 한 번, ack와 작업 상태는 별개임을 본다.
    #[tokio::test]
    async fn working_threads_publish_landing_reports_once() {
        use sha2::Digest;
        let root = std::env::temp_dir().join(format!("aaa-landing-report-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        let mut notices = h.notice_events.subscribe();
        let (tx, mut notifications) = tokio::sync::mpsc::unbounded_channel();
        *h.notify_tx.lock().unwrap() = Some(tx);
        h.project_create("p", Default::default(), "local:cli").unwrap();
        let dir = h.project_dir("p").unwrap().join("threads");
        std::fs::create_dir_all(&dir).unwrap();
        for (id, kind) in [
            ("t-0001", aaa_proto::ThreadKind::Adopted),
            ("t-0002", aaa_proto::ThreadKind::Worktree),
            ("t-0003", aaa_proto::ThreadKind::Dir),
        ] {
            let mut r = rec();
            r.id = id.into();
            r.kind = kind;
            r.automation = (id == "t-0003").then(|| "check".into());
            r.last_group = Some(ThreadGroup::Working);
            h.db.thread_import("p", id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
            let write = |r: &mut ThreadRec, body: &str| {
                std::fs::write(dir.join(format!("{id}.md")), body).unwrap();
                r.report_hash = Some(hex::encode(sha2::Sha256::digest(body)));
                // 사람이 이미 사본을 읽고 ack해도 inbox 기록은 hash당 한 번 남아야 한다.
                r.acked_hash = r.report_hash.clone();
                h.db.thread_save("p", id, "open", &serde_json::to_string(r).unwrap()).unwrap();
            };
            write(&mut r, "[진행 중] 검증 중\n\n## Report\n중간\n");
            assert!(!h.new_report_item("p", &r));
            std::fs::write(dir.join(format!("{id}.md")), "[착지] 아직 기록에 반영되기 전 사본\n").unwrap();
            assert!(!h.new_report_item("p", &r), "다른 hash의 사본을 이 보고로 알렸다");
            write(&mut r, "[착지] 1234abcd 첫 변경\n\n## Report\n후속 작업 중\n");
            assert!(h.new_report_item("p", &r), "{kind:?}: 작업 중 착지가 inbox에서 빠졌다");
            assert!(notifications.try_recv().is_err(), "중간 착지는 inbox에만 남고 push하지 않는다");
            r = h.load("p", id).unwrap();
            assert_eq!(r.last_group, Some(ThreadGroup::Working));
            assert!(!h.new_report_item("p", &r), "같은 hash의 중복 알림");
            write(&mut r, "[착지] abcd5678 다음 변경\n\n## Report\n계속한다\n");
            assert!(h.new_report_item("p", &r));
            let items = h.inbox("p", true, true).unwrap().items;
            assert_eq!(items.iter().filter(|i| i.subject == id && i.event == "new report").count(), 2);
            r = h.load("p", id).unwrap();
            write(&mut r, "[병합 준비됨] 전체 확인 뒤 착지 창 대기\n");
            assert_eq!(h.thread_view(&r).report_attention, None);
            assert!(h.new_report_item("p", &r), "병합 대기도 coordinator inbox에 남긴다");
            r = h.load("p", id).unwrap();
            assert!(!h.new_report_item("p", &r));
            assert!(notifications.try_recv().is_err(), "병합 대기로 외부 push를 만들지 않는다");
            for (mark, attention, event) in [
                ("[완료]", aaa_proto::ReportAttention::Completed, "new report"),
                ("[결정 필요]", aaa_proto::ReportAttention::NeedsUser, "report needs user"),
                ("[검수 대기]", aaa_proto::ReportAttention::NeedsUser, "report needs user"),
                ("[확인 필요]", aaa_proto::ReportAttention::NeedsUser, "report needs user"),
                ("[사람 필요]", aaa_proto::ReportAttention::NeedsUser, "report needs user"),
            ] {
                r = h.load("p", id).unwrap();
                write(&mut r, &format!("{mark} 명시적인 알림\n## Report\n결과\n"));
                r.acked_hash = None;
                h.db.thread_save("p", id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
                assert_eq!(h.thread_view(&r).report_attention, Some(attention));
                assert!(h.new_report_item("p", &r));
                assert_eq!(notices.try_recv().is_ok(), !(id == "t-0003" && mark == "[완료]"), "{id} {mark} 앱 안 알림");
                let items = h.inbox("p", true, true).unwrap().items;
                assert!(items.iter().any(|i| i.subject == id && i.event == event), "앱 안 inbox에 보고가 남아야 한다");
                assert!(
                    notifications.try_recv().is_err(),
                    "thread 완료·확인 필요 보고는 외부 푸시 대기열에 넣지 않는다"
                );
            }
            write(&mut r, "[완료인 척] 표지 아님\n[완료] 본문\n");
            assert_eq!(h.thread_view(&r).report_attention, None);
            assert!(!h.new_report_item("p", &r));
        }
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    fn rec() -> ThreadRec {
        serde_json::from_value(serde_json::json!({
            "project": "p", "id": "t-0001", "title": "t", "machine_id": "m", "profile": "x", "role": "worker",
            "kind": "dir", "status": "open", "brief": "delivered", "created_at_ms": 0, "updated_at_ms": 0
        }))
        .unwrap()
    }

    /// herdr-projects steps.rs `auto_resolve`: 묶음·보고가 바뀐 때와 ticker(host)가 뜬 때 중 가장 늦은 때부터 잰다
    #[test]
    fn auto_resolve_counts_from_the_latest_change_or_host_start() {
        let mut r = rec();
        let due = |r: &ThreadRec, now, started| auto_resolve_due(r, now, started, 100);
        assert!(!due(&r, 1_000, 0), "잴 시각이 없는 옛 기록은 끝내지 않는다");
        r.group_changed_ms = Some(500);
        assert!(due(&r, 600, 0) && !due(&r, 599, 0));
        r.report_changed_ms = Some(550);
        assert!(!due(&r, 600, 0), "보고가 더 늦게 바뀌었다");
        assert!(!due(&r, 1_000, 950), "host가 방금 떴다: 꺼져 있던 동안은 세지 않는다");
        assert!(due(&r, 1_050, 950));
    }

    /// herdr-projects scenarios.rs 병합 시험(`a_thread_merged_while_its_agent_works_is_not_resolved` 등)의 판정 부분
    #[test]
    fn a_merged_thread_resolves_once_its_agent_is_done() {
        let may = |r: &ThreadRec, a: Option<&AgentStatus>, now| merged_may_resolve(r, a, 1_000, now, 600);
        let mut r = rec();
        // 일하거나 사람을 기다리면 오래 지나도 끝내지 않는다
        for busy in [ThreadGroup::Working, ThreadGroup::WaitingOnYou] {
            r.last_group = Some(busy);
            assert!(!may(&r, None, 100_000), "{busy:?}");
        }
        // 쉬는 agent: 병합 뒤 새 보고가 오거나, 기다림이 지나면 끝낸다
        r.last_group = Some(ThreadGroup::ReadyForReview);
        assert!(!may(&r, None, 1_599));
        assert!(may(&r, None, 1_600));
        r.report_changed_ms = Some(900);
        assert!(!may(&r, None, 1_100), "병합 전 보고는 끝 신호가 아니다");
        r.report_changed_ms = Some(1_050);
        assert!(may(&r, None, 1_100));
        // 진행을 보고하는 agent는 `Done`(100)만: 기다림도 새 보고도 낮은 진행을 넘지 않는다
        let progress = |percent| AgentStatus { percent, progress_at_ms: Some(0), ..agent(AgentActivity::Idle) };
        assert!(!may(&r, Some(&progress(Some(70))), 100_000));
        assert!(!may(&r, Some(&progress(None)), 100_000));
        assert!(may(&r, Some(&progress(Some(100))), 1_001));
        // 진행 보고가 없는 agent 상태는 그 규칙을 쓰지 않는다
        assert!(may(&r, Some(&agent(AgentActivity::Idle)), 1_100));
    }

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
            plan: None,
            background: None,
            background_wait: false,
            compacting: false,
            context: None,
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
        assert_eq!(note(false, Some(&q)), "pane closed");
        assert_eq!(note(true, None), "no agent");
        assert_eq!(note(true, Some(&q)), "blocked");
        let t = |to, note: &str| Transition { to, note: note.into() };
        assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "blocked")), "blocked on a prompt");
        assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "pane closed")), "pane closed");
        assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "no agent")), "agent gone");
        assert_eq!(transition_event(&t(ThreadGroup::WaitingOnYou, "idle")), "waiting on you");
        assert_eq!(transition_event(&t(ThreadGroup::Idle, "idle")), "idle");
        assert_eq!(transition_event(&t(ThreadGroup::Landing, "idle")), "landing");
        assert!(writes_item(ThreadGroup::WaitingOnYou) && writes_item(ThreadGroup::Idle));
        assert!(writes_item(ThreadGroup::Landing));
        assert!(!writes_item(ThreadGroup::Working) && !writes_item(ThreadGroup::ReadyForReview));
        assert!(!writes_item(ThreadGroup::Resolved));
    }
}
