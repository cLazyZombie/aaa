//! inbox·ticker·coordinator 알림(nudge)(§8.9, 0.2.35 계승).
//!
//! - **inbox:** 잃지 않는 항목과 끝내기(done)로 관리한다. DB가 원본이고 `projects/<p>/inbox/<id>.md`(TOML front
//!   matter, 0.2.35 형식)는 투영이다. 끝낸 항목은 `inbox/done/`으로 옮기고 `inbox_done_days`가 지나면 지운다.
//! - **항목 출처:** 흐름 사건(보고 준비·질문·전달 모름·thread 실패)과 ticker가 찾는 특수 항목
//!   (`session` 세션 끝남, `outage` 머신 단절·회복, `config-error` project.toml 오류).
//! - **nudge:** coordinator(프로젝트 폴더를 cwd로 쓰는 host 머신 pane)가 입력 대기 상태로 `nudge_idle_secs` 이상
//!   머물고 입력창이 비었을 때, 처음 보는 항목 묶음을 한 줄로 알린다. 같은 묶음(hash)은 한 번만 보낸다.
//!   전달을 모르면(unknown) 다시 보내지 않는다(§8.5 a). coordinator가 없으면 보내지 않고 기다린다.
//! - **ticker:** host 안의 task다. `tick_secs`마다, 그리고 사건이 오면 바로 돈다.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{AgentActivity, DeviceRole, ErrorBody, ErrorCode, FlowEvent, InboxItem, ThreadStatus};
use sha2::Digest;

use crate::orch::ThreadRec;
use crate::{Host, err};

/// nudge 문구에 나열하는 대상 수
const NUDGE_SUBJECTS: usize = 5;

fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}

fn safe_subject(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    t.chars().take(40).collect()
}

fn toml_str(s: &str) -> String {
    toml_edit::value(s).to_string().trim().to_string()
}

/// 투영 파일 본문(0.2.35와 같은 `+++` TOML front matter)
fn render_item(i: &InboxItem) -> String {
    format!(
        "+++\nid = {}\nkind = {}\nsubject = {}\ncreated = {}\nsummary = {}\nevent = {}\n+++\n{}",
        toml_str(&i.id),
        toml_str(&i.item_kind),
        toml_str(&i.subject),
        toml_str(&aaa_common::util::utc_text(i.created_at_ms)),
        toml_str(&i.summary.replace(|c: char| c.is_control(), " ")),
        toml_str(&i.event),
        if i.body.is_empty() { String::new() } else { format!("\n{}\n", i.body.trim_end()) },
    )
}

/// 사람이 읽는 대상 이름(`machine mac-b`, `t-0001 session`, `t-0001`)
pub(crate) fn who(i: &InboxItem) -> String {
    match i.item_kind.as_str() {
        "outage" | "health" => format!("machine {}", i.subject),
        "session" => format!("{} session", i.subject),
        "routine" | "routine-approval" => format!("routine {}", i.subject),
        _ => i.subject.clone(),
    }
}

/// 처음 보는 항목 묶음 → nudge 한 줄. 대상별로 사건을 묶고 요약은 넣지 않는다.
pub(crate) fn nudge_text(project: &str, items: &[InboxItem]) -> String {
    let mut order: Vec<String> = Vec::new();
    let mut events: HashMap<String, Vec<String>> = HashMap::new();
    for i in items {
        let who = who(i);
        if !events.contains_key(&who) {
            order.push(who.clone());
        }
        let ev = events.entry(who).or_default();
        if !ev.contains(&i.event) {
            ev.push(i.event.clone());
        }
    }
    let mut parts: Vec<String> =
        order.iter().take(NUDGE_SUBJECTS).map(|w| format!("{w} {}", events[w].join(", "))).collect();
    if order.len() > NUDGE_SUBJECTS {
        parts.push(format!("{} more", order.len() - NUDGE_SUBJECTS));
    }
    format!("[aaa inbox] {} (aaa inbox list {project})", parts.join("; "))
}

/// 입력창이 비어 있는가. 맨 아래 몇 줄에서 마지막 입력 줄(❯·›)을 본다.
/// 입력 줄이 없으면(일반 셸 등) 비었다고 보지 않는다: 셸에 알림 글을 쳐 넣어 실행하지 않기 위해서다.
pub(crate) fn prompt_empty(lines: &[String]) -> bool {
    let filled: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
    filled
        .iter()
        .rev()
        .take(8)
        .find_map(|l| {
            let t = l.trim_start();
            ['❯', '›'].iter().find_map(|c| t.strip_prefix(*c))
        })
        .is_some_and(|rest| rest.trim().is_empty())
}

fn same_dir(a: &str, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

impl Host {
    fn inbox_dir(&self, project: &str) -> std::path::PathBuf {
        self.projects_root().join(project).join("inbox")
    }

    /// 항목을 만든다(DB + 투영 파일). 투영 파일을 쓰지 못해도 DB 항목은 남긴다.
    pub(crate) fn inbox_add(&self, project: &str, kind: &str, subject: &str, event: &str, summary: &str) {
        self.inbox_add_body(project, kind, subject, event, summary, "");
    }

    pub(crate) fn inbox_add_body(
        &self,
        project: &str,
        kind: &str,
        subject: &str,
        event: &str,
        summary: &str,
        body: &str,
    ) {
        let now = aaa_common::util::now_ms();
        let n = self.db.inbox_count(project).unwrap_or(0) + 1;
        let item = InboxItem {
            id: format!("{}-{kind}-{}-{n}", aaa_common::util::utc_compact(now), safe_subject(subject)),
            project: project.to_string(),
            item_kind: kind.to_string(),
            subject: subject.to_string(),
            event: event.to_string(),
            summary: summary.chars().take(300).collect(),
            body: body.to_string(),
            done: false,
            seen: false,
            created_at_ms: now,
            done_at_ms: None,
        };
        if let Err(e) = self.db.inbox_put(&item) {
            tracing::error!("inbox 저장 실패: {e:#}");
            return;
        }
        self.notify(&item);
        let dir = self.inbox_dir(project);
        if let Err(e) = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join(format!("{}.md", item.id)), render_item(&item)))
        {
            tracing::warn!("inbox 파일 쓰기 실패: {e}");
        }
        self.tick_wake.notify_one();
    }

    /// 특수 항목: 같은 종류·대상·사건의 끝내지 않은 항목이 있으면 만들지 않는다
    pub(crate) fn inbox_once(&self, project: &str, kind: &str, subject: &str, event: &str, summary: &str) {
        if !self.db.inbox_open_exists(project, kind, subject, event).unwrap_or(true) {
            self.inbox_add(project, kind, subject, event, summary);
        }
    }

    /// 흐름 사건 → inbox 항목
    pub(crate) fn inbox_from_flow(&self, e: &FlowEvent) {
        let Some(thread) = &e.thread else { return };
        // 사후 평가자의 보고·알림은 평가 기록(`evaluation`)으로 따로 알린다. 실패·사본 실패는 그대로 알린다.
        if e.role == Some(aaa_proto::Role::Evaluator) && matches!(e.kind.as_str(), "report_ready" | "message_unknown") {
            return;
        }
        let (kind, event) = match e.kind.as_str() {
            "report_ready" => ("report", "report ready"),
            "question_shown" => ("question", "question"),
            "brief_unknown" => ("delivery", "brief unknown"),
            "message_unknown" => ("delivery", "message unknown"),
            "thread_failed" => ("thread", "failed"),
            "copy_failed" => ("thread", "copy failed"),
            _ => return,
        };
        self.inbox_add(&e.project, kind, thread, event, &e.detail);
    }

    /// `aaa inbox list`: peek가 아니면 보인 항목을 「봤다」로 표시한다
    pub(crate) fn inbox(&self, project: &str, all: bool, peek: bool) -> Result<Vec<InboxItem>, ErrorBody> {
        self.project_dir(project)?;
        let mut items = self.db.inbox_list(Some(project), all).map_err(internal)?;
        if !peek {
            for i in items.iter_mut().filter(|i| !i.done && !i.seen) {
                i.seen = true;
                self.db.inbox_put(i).map_err(internal)?;
            }
        }
        Ok(items)
    }

    /// 항목을 끝낸다: 투영 파일을 `inbox/done/`으로 옮긴다
    pub(crate) fn inbox_done(
        &self,
        project: &str,
        ids: &[String],
        all: bool,
        actor: &str,
    ) -> Result<Vec<InboxItem>, ErrorBody> {
        self.project_dir(project)?;
        if ids.is_empty() && !all {
            return Err(err(ErrorCode::BadRequest, "끝낼 항목 ID나 --all이 필요하다"));
        }
        let open = self.db.inbox_list(Some(project), false).map_err(internal)?;
        if let Some(miss) = ids.iter().find(|id| !open.iter().any(|i| &&i.id == id)) {
            return Err(err(ErrorCode::NotFound, format!("끝내지 않은 항목이 없다: {miss}")));
        }
        let pick: Vec<InboxItem> = open.into_iter().filter(|i| all || ids.contains(&i.id)).collect();
        let n = pick.len();
        self.inbox_finish(project, pick).map_err(internal)?;
        self.db.audit(actor, "inbox_done", &format!("{project} {n}"));
        self.db.inbox_list(Some(project), false).map_err(internal)
    }

    /// 조건에 맞는 끝내지 않은 항목을 끝낸다
    pub(crate) fn inbox_finish_where(&self, project: &str, f: impl Fn(&InboxItem) -> bool) {
        let pick: Vec<InboxItem> =
            self.db.inbox_list(Some(project), false).unwrap_or_default().into_iter().filter(|i| f(i)).collect();
        if !pick.is_empty()
            && let Err(e) = self.inbox_finish(project, pick)
        {
            tracing::warn!("inbox 항목을 끝내지 못했다: {e:#}");
        }
    }

    fn inbox_finish(&self, project: &str, items: Vec<InboxItem>) -> anyhow::Result<()> {
        let dir = self.inbox_dir(project);
        let now = aaa_common::util::now_ms();
        for mut i in items {
            i.done = true;
            i.done_at_ms = Some(now);
            self.db.inbox_put(&i)?;
            let _ = std::fs::create_dir_all(dir.join("done"));
            let _ = std::fs::rename(dir.join(format!("{}.md", i.id)), dir.join("done").join(format!("{}.md", i.id)));
        }
        Ok(())
    }

    /// 질문이 닫혔다(답함·harness가 넘어감): 그 질문의 inbox 항목은 할 일이 없으니 끝낸다(기록은 done에 남는다).
    /// 끝낸 항목은 처음 보는 묶음에서 빠져 nudge에 섞이지 않는다.
    pub(crate) fn inbox_question_closed(&self, q: &aaa_proto::QuestionView) {
        let (Some(project), Some(thread)) = (&q.project, &q.thread) else { return };
        let mine: Vec<InboxItem> = self
            .db
            .inbox_list(Some(project), false)
            .unwrap_or_default()
            .into_iter()
            .filter(|i| {
                i.item_kind == "question" && &i.subject == thread && i.summary.starts_with(&format!("{} ", q.id))
            })
            .collect();
        if !mine.is_empty()
            && let Err(e) = self.inbox_finish(project, mine)
        {
            tracing::warn!("질문 inbox 항목을 끝내지 못했다: {e:#}");
        }
    }

    // ───────────── ticker ─────────────

    pub fn start_ticker(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            let every = Duration::from_secs(me.cfg.projects.tick_secs.max(1));
            loop {
                // 사후 평가자 묶음 때가 더 이르면 그때까지만 잔다
                let sleep = me
                    .evaluator_next_due()
                    .map(|d| d.saturating_duration_since(std::time::Instant::now()).min(every))
                    .unwrap_or(every);
                tokio::select! {
                    _ = tokio::time::sleep(sleep) => {}
                    _ = me.tick_wake.notified() => {}
                }
                me.tick().await;
            }
        });
    }

    async fn tick(self: &Arc<Self>) {
        let projects = self.projects();
        let names: Vec<String> = projects.iter().map(|p| p.name.clone()).collect();
        self.check_outages(&names);
        self.check_health_items(&names);
        self.evaluator_pass();
        for p in &projects {
            self.check_project_config(&p.name, Path::new(&p.path));
            self.check_sessions(&p.name);
            let dir = Path::new(&p.path);
            self.routine_pass(&p.name, dir, self.coordinator_pane(dir).is_some());
            self.nudge(&p.name, dir).await;
        }
        let keep = Duration::from_secs(self.cfg.projects.inbox_done_days * 86_400).as_millis() as u64;
        if let Ok(gone) = self.db.inbox_prune(aaa_common::util::now_ms().saturating_sub(keep)) {
            for (project, id) in gone {
                let _ = std::fs::remove_file(self.inbox_dir(&project).join("done").join(format!("{id}.md")));
            }
        }
    }

    pub(crate) fn open_threads(&self, project: &str) -> Vec<ThreadRec> {
        self.db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| r.status == ThreadStatus::Open)
            .collect()
    }

    /// 열린 thread의 세션이 끝났다(종료·재부팅·유실). 세션(pane)마다 한 번만 알린다(끝낸 뒤에도 다시 만들지 않는다).
    fn check_sessions(&self, project: &str) {
        for r in self.open_threads(project) {
            let Some(pane) = &r.pane else { continue };
            let Some(v) = self.pane_view_of(pane, DeviceRole::Local) else { continue };
            if v.state.is_running() {
                continue;
            }
            let key = format!("session_ended:{project}:{}", r.id);
            if self.db.meta(&key).ok().flatten().as_deref() == Some(pane.as_str()) {
                continue;
            }
            let _ = self.db.set_meta(&key, pane);
            self.inbox_add(
                project,
                "session",
                &r.id,
                "ended",
                &format!("세션이 끝났다({:?}). 다시 시작: aaa thread restart {project} {}", v.state, r.id),
            );
        }
    }

    /// 머신이 오래 끊겼다·돌아왔다. 그 머신에 열린 thread가 있는 프로젝트에 알린다.
    /// 끊긴 시간은 이 host 프로세스가 처음 끊김을 본 때부터 잰다(herdr-projects steps.rs `Outage`).
    /// 옛 `last_seen`으로 재면 host를 다시 띄운 직후 아직 붙지 않은 머신이 오래 끊긴 것으로 보인다.
    fn check_outages(&self, projects: &[String]) {
        let now = aaa_common::util::now_ms();
        let limit = self.cfg.projects.outage_secs * 1000;
        for m in self.machine_views() {
            let ok = m.connection != aaa_proto::Connection::Disconnected;
            let event = self.outages.lock().unwrap().entry(m.id.clone()).or_default().record(ok, now, limit);
            let Some(event) = event else { continue };
            for p in projects {
                if !self.open_threads(p).iter().any(|r| r.machine_id == m.id) {
                    continue;
                }
                match event {
                    OutageEvent::Down => self.inbox_once(
                        p,
                        "outage",
                        &m.name,
                        "down",
                        &format!(
                            "머신 {}가 {}분째 연결되지 않는다. thread는 마지막 상태를 유지한다",
                            m.name,
                            limit / 60_000
                        ),
                    ),
                    OutageEvent::Recovered => {
                        self.inbox_add(p, "outage", &m.name, "recovered", &format!("머신 {}가 다시 연결됐다", m.name))
                    }
                }
            }
        }
    }

    /// project.toml을 읽지 못하면 파일 내용 hash당 한 번 알린다
    fn check_project_config(&self, project: &str, dir: &Path) {
        let Ok(text) = std::fs::read_to_string(dir.join("project.toml")) else { return };
        let Err(e) = text.parse::<toml_edit::DocumentMut>() else { return };
        let hash = hex::encode(sha2::Sha256::digest(text.as_bytes()));
        let key = format!("config_error:{project}");
        if self.db.meta(&key).ok().flatten().as_deref() == Some(hash.as_str()) {
            return;
        }
        let _ = self.db.set_meta(&key, &hash);
        let first = e.to_string().lines().next().unwrap_or_default().to_string();
        self.inbox_add(
            project,
            "config-error",
            "project.toml",
            "not usable",
            &format!("project.toml을 읽지 못한다: {first}"),
        );
    }

    /// 프로젝트 폴더를 cwd로 쓰는 host 머신의 실행 중 pane(coordinator)
    fn coordinator_pane(&self, dir: &Path) -> Option<aaa_proto::PaneView> {
        let local: Vec<String> = self.machine_views().into_iter().filter(|m| m.local).map(|m| m.id).collect();
        self.pane_views(DeviceRole::Local).into_iter().find(|p| {
            local.contains(&p.machine_id)
                && p.state.is_running()
                && p.connection == aaa_proto::Connection::Connected
                && p.cwd.as_deref().is_some_and(|c| same_dir(c, dir))
        })
    }

    async fn nudge(self: &Arc<Self>, project: &str, dir: &Path) {
        let unseen: Vec<InboxItem> =
            self.db.inbox_list(Some(project), false).unwrap_or_default().into_iter().filter(|i| !i.seen).collect();
        let forget_box = || {
            self.nudge_box.lock().unwrap().remove(project);
        };
        if unseen.is_empty() {
            forget_box();
            return;
        }
        let mut ids: Vec<&str> = unseen.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        let hash = hex::encode(sha2::Sha256::digest(ids.join("\n").as_bytes()));
        let key = format!("nudged:{project}");
        if self.db.meta(&key).ok().flatten().as_deref() == Some(hash.as_str()) {
            forget_box();
            return;
        }
        // coordinator가 없으면 기다린다(생기면 그때 알린다)
        let Some(coord) = self.coordinator_pane(dir) else {
            forget_box();
            return;
        };
        let idle_for = Duration::from_secs(self.cfg.projects.nudge_idle_secs);
        // 입력 대기: hook 상태가 있으면 그것으로, 없으면 화면이 그만큼 그대로인지로 본다
        let lines = self.screen_lines(&coord.machine_id, &coord.id).await.unwrap_or_default();
        let ready = match &coord.agent {
            Some(a) => {
                matches!(a.activity, AgentActivity::Idle | AgentActivity::Started)
                    && aaa_common::util::now_ms().saturating_sub(a.since_ms) >= idle_for.as_millis() as u64
            }
            None => {
                let h = hex::encode(sha2::Sha256::digest(lines.join("\n").as_bytes()));
                let mut seen = self.nudge_screens.lock().unwrap();
                let e = seen.entry(coord.id.clone()).or_insert((h.clone(), Instant::now()));
                if e.0 != h {
                    *e = (h, Instant::now());
                }
                e.1.elapsed() >= idle_for
            }
        };
        if !ready || !prompt_empty(&lines) {
            forget_box();
            return;
        }
        // 입력창이 이번 검사와 nudge_quiet_secs 전 검사에서 모두 비어 보여야 보낸다. 사람이 치는 글과 섞이지 않게 한다
        // (herdr-projects steps.rs `box_quiet`: 마지막 키 입력 시각을 알 수 없어 입력창으로 본다).
        {
            let mut boxes = self.nudge_box.lock().unwrap();
            match boxes.get(project) {
                Some((pane, since)) if *pane == coord.id => {
                    if since.elapsed() < Duration::from_secs(self.cfg.projects.nudge_quiet_secs) {
                        return;
                    }
                }
                _ => {
                    boxes.insert(project.to_string(), (coord.id.clone(), Instant::now()));
                    return;
                }
            }
        }
        let text = nudge_text(project, &unseen);
        match self.send_prompt(&coord.machine_id, &coord.id, &text).await {
            Ok(accepted) => {
                // 받았는지 몰라도 같은 묶음은 다시 보내지 않는다(§8.5 a)
                let _ = self.db.set_meta(&key, &hash);
                forget_box();
                let state = if accepted { "accepted" } else { "unknown" };
                self.flow_project(project, "nudge_sent", None, format!("{state}: {text}"));
            }
            Err(e) => tracing::info!(%project, "nudge 보류: {e}"),
        }
    }
}

/// 머신 하나의 연속된 끊김(herdr-projects steps.rs `Outage`): 기준 시간을 넘으면 한 번, 돌아오면 한 번 알린다.
/// 짧은 끊김은 아무것도 남기지 않는다. 시작 시각은 이 host 프로세스가 처음 끊김을 본 때다(저장하지 않는다).
#[derive(Debug, Clone, Default)]
pub(crate) struct Outage {
    failing_since: Option<u64>,
    reported: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutageEvent {
    Down,
    Recovered,
}

impl Outage {
    pub(crate) fn record(&mut self, ok: bool, now_ms: u64, limit_ms: u64) -> Option<OutageEvent> {
        if ok {
            let was_reported = self.reported;
            *self = Outage::default();
            return was_reported.then_some(OutageEvent::Recovered);
        }
        let since = *self.failing_since.get_or_insert(now_ms);
        if !self.reported && now_ms.saturating_sub(since) >= limit_ms {
            self.reported = true;
            return Some(OutageEvent::Down);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_outages_write_nothing_and_long_ones_write_one_item_each_way() {
        // herdr-projects steps.rs 같은 이름의 시험을 옮겼다(분 단위)
        let min = 60_000;
        let mut outage = Outage::default();
        assert_eq!(outage.record(false, 0, 10 * min), None);
        assert_eq!(outage.record(false, 5 * min, 10 * min), None);
        // 기준 전에 끝난 짧은 끊김은 아무것도 남기지 않는다
        assert_eq!(outage.record(true, 6 * min, 10 * min), None);

        assert_eq!(outage.record(false, 60 * min, 10 * min), None);
        assert_eq!(outage.record(false, 70 * min, 10 * min), Some(OutageEvent::Down));
        assert_eq!(outage.record(false, 90 * min, 10 * min), None);
        assert_eq!(outage.record(true, 91 * min, 10 * min), Some(OutageEvent::Recovered));
        assert_eq!(outage.record(true, 92 * min, 10 * min), None);
    }

    fn item(kind: &str, subject: &str, event: &str) -> InboxItem {
        InboxItem {
            id: format!("{kind}-{subject}-{event}"),
            project: "demo".into(),
            item_kind: kind.into(),
            subject: subject.into(),
            event: event.into(),
            summary: "요약은 문구에 넣지 않는다".into(),
            body: String::new(),
            done: false,
            seen: false,
            created_at_ms: 0,
            done_at_ms: None,
        }
    }

    #[test]
    fn nudge_groups_by_subject() {
        let items = vec![
            item("report", "t-0001", "report ready"),
            item("question", "t-0001", "question"),
            item("outage", "mac-b", "down"),
            item("report", "t-0002", "report ready"),
        ];
        assert_eq!(
            nudge_text("demo", &items),
            "[aaa inbox] t-0001 report ready, question; machine mac-b down; t-0002 report ready (aaa inbox list demo)"
        );
        let many: Vec<_> = (1..=7).map(|n| item("report", &format!("t-000{n}"), "report ready")).collect();
        assert!(nudge_text("demo", &many).contains("; 2 more (aaa inbox list demo)"));
    }

    #[test]
    fn prompt_box_state() {
        let l = |t: &str| t.lines().map(String::from).collect::<Vec<_>>();
        let idle = "⏺ 끝\n──────\n❯ \n──────\n  Haiku 5.5 | ctx\n  ⏸ manual mode on\n";
        assert!(prompt_empty(&l(idle)));
        // 사람이 쓰던 글
        assert!(!prompt_empty(&l(&idle.replace("❯ ", "❯ 쓰던 글"))));
        // 지난 입력(위쪽 ❯ 줄)은 쓰던 글이 아니다
        assert!(prompt_empty(&l(&format!("❯ 지난 지시\n{idle}"))));
        // 입력 줄이 없는 일반 셸에는 보내지 않는다
        assert!(!prompt_empty(&l("user@mac-a demo % \n")));
    }

    #[test]
    fn projection_is_toml_front_matter() {
        let mut i = item("report", "t-0001", "report ready");
        i.summary = "따옴표 \" 와 줄\n바꿈".into();
        let md = render_item(&i);
        let body = md.trim().trim_start_matches("+++").trim_end_matches("+++");
        let doc = body.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(doc["summary"].as_str(), Some("따옴표 \" 와 줄 바꿈"));
        assert_eq!(doc["kind"].as_str(), Some("report"));
        assert_eq!(safe_subject("Mac B/외장 x!!"), "mac-b-x");
    }
}
