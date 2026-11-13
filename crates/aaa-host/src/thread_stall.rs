//! 작업 의도가 남은 idle 감시. ticker가 판정하며 같은 idle의 알림·재개와 10분 내 재개 예산을 DB에 보존한다.
//! 화면 무출력만으로 멈춤이라 하지 않는다. 보고·질문·배경 작업과 현재 입력창 앞의 마지막 결과를 함께 본다.

use std::sync::{Arc, LazyLock};

use aaa_proto::{AgentActivity, AgentStatus, BriefState, ReadFormat, ReadSource, Role, ThreadGroup};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{Host, orch::ThreadRec};

const WINDOW_MS: u64 = 600_000;
const MAX_RESUMES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultKind {
    Active,
    Quiet,
    Transient,
    Manual,
}

static ACTIVE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?im)esc to interrupt|…\s*\((?:\d+[hms][^)]*(?:tokens|·)|thinking)|waiting for \d+ background agent|⏵⏵ .*· [1-9]\d* shells?(?:\s|$)|^\s*• .+…\s*$").unwrap()
});
static TRANSIENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)API Error|connection lost|at capacity|stream disconnected|stream error|reconnecting|error sending request|unexpected status|conversation interrupted").unwrap()
});
static MANUAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)usage limit|you['’]ve hit|rate limit|panicked|insufficient credits|credit balance|authentication|unauthorized|forbidden|\b(?:401|403|429)\b").unwrap()
});

/// 마지막 입력창 바로 앞 결과만 본다. 오래된 오류 뒤 답·새 사용자 입력이 있으면 오류를 되살리지 않는다.
/// 에러 블록의 마지막 4줄까지만 합쳐 폭 때문에 접힌 오류도 읽는다. 상태줄·입력창은 결과에 넣지 않는다.
fn result_kind(harness: &str, lines: &[String]) -> ResultKind {
    let tail = &lines[lines.len().saturating_sub(20)..];
    if ACTIVE.is_match(&tail.join("\n")) {
        return ResultKind::Active;
    }
    let prompt = if harness == "codex" { '›' } else { '❯' };
    let Some(composer) = lines.iter().rposition(|l| l.trim_start().starts_with(prompt)) else {
        return ResultKind::Active; // 현재 입력창을 모르면 감시·입력하지 않는다.
    };
    let body: Vec<&str> = lines[..composer]
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('─') && !l.starts_with('━'))
        .collect();
    let Some(last) = body.last() else { return ResultKind::Quiet };
    // 마지막 결과 블록: 새 응답·도구 출력·사용자 입력의 시작을 넘지 않는다.
    let start = body
        .iter()
        .rposition(|l| l.starts_with(['•', '●', '⏺', '❯', '›']) || diagnostic_line(l))
        .unwrap_or(body.len() - 1);
    let block = body[start.max(body.len().saturating_sub(4))..].join(" ");
    if MANUAL.is_match(&block) {
        return ResultKind::Manual;
    }
    let diagnostic = body.get(start).is_some_and(|line| diagnostic_line(line));
    if diagnostic
        && TRANSIENT.is_match(&block)
        && (TRANSIENT.is_match(last) || last.starts_with("http") || matches!(*last, "response" | "response."))
    {
        return ResultKind::Transient;
    }
    ResultKind::Quiet
}

fn diagnostic_line(line: &str) -> bool {
    let lower = line.to_lowercase();
    line.starts_with(['■', '⚠', '⎿'])
        || [
            "api error",
            "error:",
            "connection lost",
            "selected model",
            "stream disconnected",
            "stream error",
            "reconnecting",
            "error sending request",
            "unexpected status",
            "conversation interrupted",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

/// 보고를 갱신하며 작업하는 thread와 아직 첫 결과가 없는 thread만 감시한다.
fn eligible(r: &ThreadRec, a: &AgentStatus, group: ThreadGroup, attention: bool) -> bool {
    r.status == aaa_proto::ThreadStatus::Open
        && r.role == Role::Worker
        && r.idea.is_none()
        && r.park.is_none()
        && r.resolve_pending.is_none()
        && r.brief == BriefState::Delivered
        && !attention
        && r.report_stage.is_none()
        && r.questions.is_empty()
        && r.question_warning.is_none()
        && a.activity == AgentActivity::Idle
        && matches!(a.harness.as_str(), "claude" | "codex")
        && a.percent != Some(100)
        && a.progress.as_deref() != Some(crate::orch::WAITING)
        && !a.background_wait
        && a.background.unwrap_or(0) == 0
        && !a.compacting
        && (group == ThreadGroup::Working
            || r.report_hash.is_none()
            || (a.progress_at_ms.is_some() && group == ThreadGroup::Idle))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Watch {
    /// pane·idle 시작: 같은 idle에서 진행 보고가 와도 다시 알리거나 재개하지 않는다.
    idle: Option<(String, u64)>,
    since: u64,
    report: Option<String>,
    progress: Option<u64>,
    notified: bool,
    attempts: Vec<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Notify,
    Resume,
    Limited,
}

impl Watch {
    fn clear(&mut self) {
        self.idle = None;
        self.notified = false;
    }

    fn observe(
        &mut self,
        r: &ThreadRec,
        a: &AgentStatus,
        kind: ResultKind,
        now: u64,
        started: u64,
        limit: u64,
    ) -> Option<Action> {
        if kind == ResultKind::Active {
            self.clear();
            return None;
        }
        let idle = (r.pane.clone().unwrap_or_default(), a.since_ms);
        if self.idle.as_ref() != Some(&idle) {
            self.idle = Some(idle);
            self.since = now;
            self.notified = false;
            self.report = r.report_hash.clone();
            self.progress = a.progress_at_ms;
        }
        if self.report != r.report_hash || self.progress != a.progress_at_ms {
            self.since = now;
            self.report = r.report_hash.clone();
            self.progress = a.progress_at_ms;
        }
        if self.notified || now.saturating_sub(self.since.max(started)) < limit {
            return None;
        }
        self.notified = true;
        self.attempts.retain(|at| now.saturating_sub(*at) < WINDOW_MS);
        Some(if kind != ResultKind::Transient {
            Action::Notify
        } else if self.attempts.len() >= MAX_RESUMES {
            Action::Limited
        } else {
            // 전달 성공·실패·불명 모두 예산을 쓴다. host 재시작도 같은 입력을 반복하지 않는다.
            self.attempts.push(now);
            Action::Resume
        })
    }
}

impl Host {
    async fn resume_stalled(self: &Arc<Self>, expected: &ThreadRec, idle: &AgentStatus) -> Result<bool, String> {
        // 다른 입력과 직렬화한 뒤 보고·상태·빈 입력창을 다시 확인한다. 기다리는 사이 새 일을 시작했으면 보내지 않는다.
        let _guard = self.deliver_lock.lock().await;
        let r = self.load(&expected.project, &expected.id).map_err(|e| e.message)?;
        let pane = r.pane.as_deref().ok_or("thread pane이 없다")?;
        let screen = self.screen(&r.machine_id, pane).await.ok_or("현재 화면을 읽지 못했다")?;
        // Screen RPC를 기다리는 동안 회수된 보고도 다시 읽는다.
        let r = self.load(&expected.project, &expected.id).map_err(|e| e.message)?;
        let pane = r.pane.as_deref().ok_or("thread pane이 없다")?;
        let a = self.pane_agent(pane).ok_or("agent 상태가 없다")?;
        if r.pane != expected.pane
            || r.report_hash != expected.report_hash
            || a.since_ms != idle.since_ms
            || a.progress_at_ms != idle.progress_at_ms
            || !eligible(&r, &a, self.thread_view(&r).group, self.report_attention(&r).is_some())
            || self.report_refresh.lock().unwrap().contains_key(&format!("{}/{}", r.project, r.id))
            || crate::prompt_box::check(&a.harness, &screen.ansi) != crate::prompt_box::Draft::Empty
            || crate::trust_screen::detect(&a.harness, &screen.ansi).is_some()
            || crate::harness::modal_of(&screen.lines).is_some()
            || result_kind(&a.harness, &screen.lines) != ResultKind::Transient
        {
            return Err("상태·보고·화면이 달라져 자동 재개하지 않았다".into());
        }
        let result = self.send_prompt(&r.machine_id, pane, "이어서 계속하라").await;
        self.flow(
            &r,
            "thread_stall_resume",
            None,
            match &result {
                Ok(true) => "accepted",
                Ok(false) => "unknown",
                Err(_) => "failed",
            },
        );
        result
    }

    pub(crate) async fn thread_stall_pass(self: &Arc<Self>, project: &str) {
        self.thread_stall_pass_at(project, aaa_common::util::now_ms()).await;
    }

    async fn thread_stall_pass_at(self: &Arc<Self>, project: &str, now: u64) {
        let limit = self.cfg.projects.thread_stall_secs.saturating_mul(1000);
        if limit == 0 {
            return;
        }
        for r in self.open_threads(project) {
            let key = format!("thread_stall:{project}:{}", r.id);
            let stored = match self.db.meta(&key) {
                Ok(v) => v,
                Err(error) => {
                    tracing::warn!(thread = %r.id, %error, "멈춤 감시 기록을 읽지 못했다");
                    continue;
                }
            };
            let mut watch: Watch = match stored.as_deref().map(serde_json::from_str).transpose() {
                Ok(w) => w.unwrap_or_default(),
                Err(error) => {
                    tracing::warn!(thread = %r.id, %error, "멈춤 감시 기록이 잘못되어 재개하지 않는다");
                    continue;
                }
            };
            let (exists, agent) = self.thread_live(&r);
            let connected =
                self.live_conn(&r.machine_id).is_ok() && !self.restoring.lock().unwrap().contains(&r.machine_id);
            let refreshing = self.report_refresh.lock().unwrap().contains_key(&format!("{project}/{}", r.id));
            if !connected || refreshing || agent.is_none() {
                // 재연결·보고 수집은 회복 사건이 아니다. 같은 멈춤의 중복 방지 기록을 보존한다.
                continue;
            }
            let group = self.thread_view(&r).group;
            let candidate =
                exists && agent.as_ref().is_some_and(|a| eligible(&r, a, group, self.report_attention(&r).is_some()));
            let mut action = None;
            let mut summary = String::new();
            if candidate {
                let a = agent.as_ref().unwrap();
                let pane = r.pane.as_deref().unwrap_or_default();
                // host의 동기화된 사본만 읽는다. 끊긴 화면이나 뒤늦은 RPC 응답으로 입력하지 않는다.
                let screen = {
                    let mut inner = self.inner.lock().unwrap();
                    inner.panes.get_mut(pane).and_then(|p| p.mirror.as_mut()).filter(|m| m.synced).map(|m| {
                        (
                            m.vt.read(ReadSource::Visible, None, ReadFormat::Text).text,
                            m.vt.read(ReadSource::Visible, None, ReadFormat::Ansi).text,
                        )
                    })
                };
                if let Some((text, ansi)) = screen {
                    let lines: Vec<String> = text.lines().map(str::to_owned).collect();
                    let kind = if crate::harness::modal_of(&lines).is_some()
                        || crate::trust_screen::detect(&a.harness, &ansi).is_some()
                    {
                        ResultKind::Active
                    } else {
                        result_kind(&a.harness, &lines)
                    };
                    action = watch.observe(&r, a, kind, now, self.started_ms, limit);
                    let tail: Vec<_> = lines.iter().rev().filter(|l| !l.trim().is_empty()).take(6).collect();
                    let tail = tail.into_iter().rev().map(|l| l.trim()).collect::<Vec<_>>().join(" | ");
                    // inbox에도 기존 인증 가리기를 적용한다. 원문을 지시로 보내지 않는다.
                    let tail: String = aaa_common::redact::text(&tail).chars().take(600).collect();
                    summary = format!(
                        "{}: agent idle이 {}초 이상 지속됐다; 마지막 화면(자료): {tail}",
                        crate::transitions::label(&r),
                        self.cfg.projects.thread_stall_secs
                    );
                    // 자동 입력은 현재 입력창이 확실히 빌 때만이다. 초안·모름은 알림만 남긴다.
                    if action == Some(Action::Resume)
                        && crate::prompt_box::check(&a.harness, &ansi) != crate::prompt_box::Draft::Empty
                    {
                        action = Some(Action::Notify);
                        watch.attempts.pop();
                        summary.push_str("; 입력창이 비었다고 확인하지 못해 자동 재개하지 않았다");
                    }
                } else {
                    // 사본 동기화 중에도 이미 알린 idle 기록은 보존한다.
                    continue;
                }
            } else {
                watch.clear();
            }
            let data = serde_json::to_string(&watch).unwrap();
            if stored.as_deref() != Some(&data)
                && let Err(error) = self.db.set_meta(&key, &data)
            {
                tracing::warn!(thread = %r.id, %error, "멈춤 감시 기록 저장 실패로 재개하지 않는다");
                continue;
            }
            let Some(action) = action else { continue };
            if action == Action::Resume {
                self.inbox_add(project, "thread-state", &r.id, "thread stalled", &summary);
                // 화면 문구는 실행하지 않는다. 검토한 고정 문장만 기존 메시지 경로로 한 번 보낸다.
                let delivery = self.resume_stalled(&r, agent.as_ref().unwrap()).await;
                let result = match delivery {
                    Ok(true) => "accepted".into(),
                    Ok(false) => "unknown; 재전송하지 않음".into(),
                    Err(error) => aaa_common::redact::text(&error),
                };
                summary.push_str(&format!(
                    "; 「이어서 계속하라」 자동 재개 시도({result}, 최근 10분 {}회)",
                    watch.attempts.len()
                ));
                self.inbox_add(project, "thread-state", &r.id, "thread resumed", &summary);
            } else {
                if action == Action::Limited {
                    summary.push_str("; 최근 10분 자동 재개 3회로 중단했다. 사람이 원인을 확인한다");
                }
                self.inbox_add(project, "thread-state", &r.id, "thread stalled", &summary);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn rec() -> ThreadRec {
        serde_json::from_value(serde_json::json!({
            "project":"p", "id":"t-0001", "title":"작업", "machine_id":"m", "profile":"x",
            "role":"worker", "kind":"dir", "status":"open", "brief":"delivered", "pane":"s",
            "created_at_ms":0, "updated_at_ms":0
        }))
        .unwrap()
    }

    pub(super) fn agent() -> AgentStatus {
        serde_json::from_value(serde_json::json!({
            "activity":"idle", "since_ms":1000, "harness":"codex", "percent":25,
            "progress":"구현 중", "progress_at_ms":1000
        }))
        .unwrap()
    }

    #[test]
    fn last_result_table() {
        use ResultKind::*;
        for (harness, screen, expected) in [
            ("claude", "❯ 작업\nAPI Error: Connection lost mid-response\n──────────\n❯ \n──────────", Transient),
            ("codex", "› 작업\n■ stream disconnected before completion\n\n› ", Transient),
            ("claude", "❯ 작업\n  ⎿ API Error: Connection lost mid-response\n──────────\n❯ \n──────────", Transient),
            ("codex", "› 작업\nConnection lost mid-response\n› ", Transient),
            ("codex", "■ Selected model is at capacity\n› ", Transient),
            ("codex", "■ stream error: error sending request\n› ", Transient),
            ("codex", "■ unexpected status 503\n› ", Transient),
            ("codex", "■ Reconnecting failed\n› ", Transient),
            ("codex", "■ Conversation interrupted\n› ", Transient),
            ("codex", "■ You've hit your usage limit\n› ", Manual),
            ("claude", "API Error: 429 rate limit\n──────\n❯ \n──────", Manual),
            ("claude", "API Error: 403 forbidden\n──────\n❯ \n──────", Manual),
            ("codex", "thread panicked at failure\n› ", Manual),
            ("codex", "■ stream disconnected\n› 새 요청\n• 완료했습니다\n› ", Quiet),
            ("codex", "■ stream error\n• 정상 응답\n› ", Quiet),
            ("codex", "■ stream error\n정상 응답\n› ", Quiet),
            ("codex", "■ stream error\n› 새 요청\n› ", Quiet),
            ("codex", "• 정상 완료\n› ", Quiet),
            ("codex", "• stream error를 고쳤다\n› ", Quiet),
            ("codex", "■ stream error\n• Working (4s • esc to interrupt)\n› ", Active),
            ("claude", "API Error: Connection lost\n✽ Scampering… (2m 11s · ↓ 4.6k tokens)\n❯ ", Active),
            ("claude", "✽ Thinking… (thinking)\n❯ ", Active),
            ("codex", "■ stream error\n• 다음 구현을 생각하는…\n› ", Active),
            ("claude", "Waiting for 2 background agents\n❯ ", Active),
            ("claude", "❯ \n⏵⏵ accept edits on · 2 shells", Active),
            ("codex", "■ stream error", Active),
        ] {
            let lines = screen.lines().map(str::to_owned).collect::<Vec<_>>();
            assert_eq!(result_kind(harness, &lines), expected, "{harness}: {screen}");
        }
    }

    #[test]
    fn candidate_table() {
        let base = rec();
        let a = agent();
        assert!(eligible(&base, &a, ThreadGroup::Working, false));
        assert!(eligible(&base, &a, ThreadGroup::Idle, false), "첫 결과 없이 TTL이 지나도 감시한다");
        assert!(!eligible(&base, &a, ThreadGroup::Working, true), "완료·검수·결정 보고");
        for activity in [AgentActivity::Working, AgentActivity::Question, AgentActivity::Error, AgentActivity::Exited] {
            let mut a = a.clone();
            a.activity = activity;
            assert!(!eligible(&base, &a, ThreadGroup::Working, false), "{activity:?}");
        }
        let mut variants = Vec::new();
        let mut bg = a.clone();
        bg.background = Some(1);
        variants.push(bg);
        let mut bg = a.clone();
        bg.background_wait = true;
        variants.push(bg);
        let mut waiting = a.clone();
        waiting.progress = Some(crate::orch::WAITING.into());
        variants.push(waiting);
        let mut done = a.clone();
        done.percent = Some(100);
        variants.push(done);
        let mut compact = a.clone();
        compact.compacting = true;
        variants.push(compact);
        for a in variants {
            assert!(!eligible(&base, &a, ThreadGroup::Working, false));
        }
        for status in
            [aaa_proto::ThreadStatus::Parked, aaa_proto::ThreadStatus::Resolved, aaa_proto::ThreadStatus::Starting]
        {
            let mut r = base.clone();
            r.status = status;
            assert!(!eligible(&r, &a, ThreadGroup::Working, false));
        }
        let mut r = base.clone();
        r.report_stage = Some(aaa_proto::ThreadStage::MergeReady);
        assert!(!eligible(&r, &a, ThreadGroup::Working, false));
    }

    #[test]
    fn debounce_report_progress_dedupe_restart_and_resume_budget() {
        let mut r = rec();
        let mut a = agent();
        let mut w = Watch::default();
        assert_eq!(w.observe(&r, &a, ResultKind::Quiet, 1000, 0, 90_000), None);
        assert_eq!(w.observe(&r, &a, ResultKind::Quiet, 90_999, 0, 90_000), None);
        r.report_hash = Some("new-report".into());
        assert_eq!(w.observe(&r, &a, ResultKind::Quiet, 91_000, 0, 90_000), None);
        a.progress_at_ms = Some(100_000);
        assert_eq!(w.observe(&r, &a, ResultKind::Quiet, 100_000, 0, 90_000), None);
        assert_eq!(w.observe(&r, &a, ResultKind::Quiet, 189_999, 0, 90_000), None);
        assert_eq!(w.observe(&r, &a, ResultKind::Quiet, 190_000, 0, 90_000), Some(Action::Notify));
        // DB에서 읽어 새 host가 셈해도 같은 멈춤은 중복하지 않는다.
        w = serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();
        assert_eq!(w.observe(&r, &a, ResultKind::Transient, 300_000, 0, 90_000), None);
        for turn in 0..4 {
            a.since_ms += 1;
            let now = 400_000 + turn * 100_000;
            assert_eq!(w.observe(&r, &a, ResultKind::Transient, now, 0, 90_000), None);
            assert_eq!(
                w.observe(&r, &a, ResultKind::Transient, now + 90_000, 0, 90_000),
                Some(if turn < 3 { Action::Resume } else { Action::Limited })
            );
        }
        a.since_ms += 1;
        assert_eq!(w.observe(&r, &a, ResultKind::Transient, 1_000_000, 0, 90_000), None);
        assert_eq!(
            w.observe(&r, &a, ResultKind::Transient, 1_090_000, 0, 90_000),
            Some(Action::Resume),
            "10분 경계에서 예산을 돌려준다"
        );
        assert_eq!(w.attempts.len(), 3);
        w.clear();
        a.since_ms += 1;
        assert_eq!(w.observe(&r, &a, ResultKind::Transient, 2_000_000, 2_050_000, 90_000), None);
        assert_eq!(w.observe(&r, &a, ResultKind::Transient, 2_100_000, 2_050_000, 90_000), None, "host 재시작 유예");
        assert_eq!(w.observe(&r, &a, ResultKind::Transient, 2_140_000, 2_050_000, 90_000), Some(Action::Resume));
        w.clear();
        a.since_ms += 1;
        w.observe(&r, &a, ResultKind::Manual, 3_000_000, 0, 90_000);
        assert_eq!(w.observe(&r, &a, ResultKind::Manual, 3_090_000, 0, 90_000), Some(Action::Notify));
        assert_eq!(w.observe(&r, &a, ResultKind::Active, 3_100_000, 0, 90_000), None);
        assert!(w.idle.is_none());
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use aaa_common::conn::{FrameRx, spawn_writer};
    use aaa_proto::{AgentRequest, AgentResponse, B64, LinkMsg, SessionInfo, SessionState, Snapshot};

    /// 실제 host pass → agent link → 입력 수락 → inbox. 시각은 주입하고, 입력은 요청·hook 사건으로 끝낸다.
    #[tokio::test]
    async fn host_pass_resumes_once_and_notifies_without_typing_into_a_draft() {
        let root = std::env::temp_dir().join(format!("aaa-stall-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.notify.web_push = false;
        cfg.projects.root = root.join("projects").display().to_string();
        let h = Host::open(cfg, root.join("host")).unwrap();
        h.project_create("p", Default::default(), "test").unwrap();
        h.db.upsert_machine("m", "mac-a", None, true).unwrap();
        let mut r = super::tests::rec();
        r.last_group = Some(ThreadGroup::Working);
        h.db.thread_import("p", &r.id, "open", &serde_json::to_string(&r).unwrap()).unwrap();
        h.on_session(
            "m",
            SessionInfo {
                responsibility: None,
                id: "s".into(),
                name: "codex".into(),
                argv: vec!["codex".into()],
                cwd: None,
                pid: None,
                started_at_ms: 0,
                boot_id: "b".into(),
                cols: 80,
                rows: 24,
                state: SessionState::Running,
                role: None,
                label: None,
                harness: Some("codex".into()),
            },
            false,
        );
        let (writer, reader) = tokio::io::duplex(65536);
        let conn = crate::AgentConn::new(spawn_writer(writer, aaa_proto::MAX_FRAME), 1);
        h.inner.lock().unwrap().machines.insert(
            "m".into(),
            crate::MachineRt {
                name: "mac-a".into(),
                local: true,
                os: None,
                boot_id: None,
                dev_mode: true,
                conn: Some(conn.clone()),
                last_seen_ms: None,
                health: None,
                health_sent: None,
                features: vec![],
                identity: None,
                agent_pid: None,
                colors: None,
                build: Default::default(),
            },
        );
        let typed = Arc::new(std::sync::Mutex::new(Vec::<Vec<u8>>::new()));
        let captured = typed.clone();
        let fake_host = h.clone();
        let fake = tokio::spawn(async move {
            let mut rx = FrameRx::new(reader, aaa_proto::MAX_FRAME);
            while let Ok(LinkMsg::Req { id, body, .. }) = rx.control::<LinkMsg>().await {
                let reply = match body {
                    AgentRequest::Screen { .. } => {
                        let data = captured.lock().unwrap();
                        let prompt = if data.is_empty() { "" } else { "이어서 계속하라" };
                        let text = format!("■ stream disconnected\n› {prompt}");
                        AgentResponse::Screen(aaa_proto::ScreenText {
                            session: "s".into(),
                            cols: 80,
                            rows: 24,
                            lines: text.lines().map(str::to_owned).collect(),
                            cursor_row: 1,
                            cursor_col: 2,
                            ansi: text,
                        })
                    }
                    AgentRequest::Input { data, .. } => {
                        if data.0 == b"\r" {
                            fake_host.on_harness(
                                "m",
                                serde_json::from_value(serde_json::json!({
                                    "session":"s", "harness":"codex", "kind":"working", "at_ms":2000
                                }))
                                .unwrap(),
                            );
                        }
                        captured.lock().unwrap().push(data.0);
                        AgentResponse::Done
                    }
                    _ => AgentResponse::Done,
                };
                if let Some(tx) = conn.pending.lock().unwrap().remove(&id) {
                    tx.send(aaa_proto::Outcome::Ok(reply)).unwrap();
                }
            }
        });
        let frame = |text: &str| Snapshot {
            compression: None,
            session: "s".into(),
            offset: 0,
            cols: 80,
            rows: 24,
            replay: B64(text.replace('\n', "\r\n").into_bytes()),
            history_more: false,
        };
        let now = h.started_ms;
        h.inner.lock().unwrap().panes.get_mut("s").unwrap().agent = Some(super::tests::agent());
        h.on_subscribed("s", Some(frame("■ stream disconnected\n› ")), 0);
        h.thread_stall_pass_at("p", now).await;
        assert!(h.db.inbox_list(Some("p"), false).unwrap().is_empty());
        tokio::time::timeout(std::time::Duration::from_secs(4), h.thread_stall_pass_at("p", now + 90_000))
            .await
            .unwrap();
        let writes = typed.lock().unwrap().clone();
        assert_eq!(writes, ["이어서 계속하라".as_bytes().to_vec(), b"\r".to_vec()]);
        let items = h.db.inbox_list(Some("p"), false).unwrap();
        assert!(items.iter().any(|i| i.event == "thread resumed" && i.summary.contains("accepted")));
        // 수락 신호 뒤 새 상태를 봐도 같은 오류를 반복 입력하지 않는다.
        h.thread_stall_pass_at("p", now + 100_000).await;
        assert_eq!(*typed.lock().unwrap(), writes);
        let mut a = super::tests::agent();
        a.since_ms = 3000;
        h.inner.lock().unwrap().panes.get_mut("s").unwrap().agent = Some(a);
        h.on_subscribed("s", Some(frame("■ stream disconnected\n› 사용자가 쓰는 글")), 0);
        h.thread_stall_pass_at("p", now + 110_000).await;
        h.thread_stall_pass_at("p", now + 200_000).await;
        h.thread_stall_pass_at("p", now + 210_000).await;
        assert_eq!(*typed.lock().unwrap(), writes, "초안에 입력하지 않는다");
        let items = h.db.inbox_list(Some("p"), false).unwrap();
        assert_eq!(items.iter().filter(|i| i.event == "thread stalled").count(), 2);
        // 사본 재동기화와 연결 복구는 새 멈춤이 아니다.
        h.inner.lock().unwrap().panes.get_mut("s").unwrap().mirror.as_mut().unwrap().synced = false;
        h.thread_stall_pass_at("p", now + 220_000).await;
        h.on_subscribed("s", Some(frame("■ stream disconnected\n› 사용자가 쓰는 글")), 0);
        let connection = h.inner.lock().unwrap().machines.get_mut("m").unwrap().conn.take();
        h.thread_stall_pass_at("p", now + 230_000).await;
        h.inner.lock().unwrap().machines.get_mut("m").unwrap().conn = connection;
        h.thread_stall_pass_at("p", now + 320_000).await;
        assert_eq!(h.db.inbox_list(Some("p"), false).unwrap().len(), items.len());
        fake.abort();
        let _ = fake.await;
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }
}
