//! inbox·ticker·coordinator 알림(nudge)(§8.9, 0.2.35 계승).
//!
//! - **inbox:** 잃지 않는 항목과 끝내기(done)로 관리한다. DB가 원본이고 `projects/<p>/inbox/<id>.md`(TOML front
//!   matter, 0.2.35 형식)는 투영이다. 끝낸 항목은 `inbox/done/`으로 옮기고 `inbox_done_days`가 지나면 지운다.
//! - **항목 출처:** ticker의 묶음 전이(`thread-state`: 사람 차례·쉼·새 보고, transitions.rs), thread 사건
//!   (`thread-state`: resolved·brief not delivered·message not delivered·not launched)과 ticker가 찾는 특수 항목
//!   (`session` 한 머신의 세션이 모두 끝남, `outage` 머신 단절·회복, `config-error` project.toml 오류).
//! - **nudge:** coordinator(프로젝트 폴더를 cwd로 쓰는 host 머신 pane)가 입력 대기 상태로 `nudge_idle_secs` 이상
//!   머물고 입력창이 비었을 때, 처음 보는 항목 묶음을 한 줄로 알린다. 같은 묶음(hash)은 한 번만 보낸다.
//!   전달을 모르면(unknown) 다시 보내지 않는다(§8.5 a). coordinator가 없거나 project.toml `nudge = false`면 그 묶음을
//!   본 것으로만 적는다: 항목은 coordinator의 다음 턴 `context`가 보인다(herdr-projects steps.rs `nudge`, plan §8.136).
//! - **ticker:** host 안의 task다. `tick_secs`마다, 그리고 사건이 오면 바로 돈다.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use aaa_proto::{AgentActivity, DeviceRole, ErrorBody, ErrorCode, FlowEvent, InboxItem, ThreadStatus};
use sha2::Digest;

use crate::orch::ThreadRec;
use crate::{Host, err};

/// nudge 문구에 나열하는 대상 수
const NUDGE_SUBJECTS: usize = 5;

/// `inbox`·`context` 한 응답에 싣는 inbox 항목 크기 한도(JSON bytes). front 연결 frame 상한(1 MiB)의 절반이라
/// 응답의 나머지 자리가 남는다. 참고는 파일을 바로 읽어(`context`의 Inbox 절) 이런 한도가 없다.
pub(crate) const INBOX_REPLY_BYTES: usize = aaa_proto::MAX_FRAME / 2;

/// 크기 한도 안에 실은 inbox 항목과 못 실은 수
#[derive(Debug, Default)]
pub(crate) struct InboxPage {
    pub items: Vec<InboxItem>,
    pub omitted_open: u32,
    pub omitted_done: u32,
}

/// 크기 한도(`budget`, JSON bytes) 안에 드는 항목만 싣는다. 끝내지 않은 항목이 먼저 자리를 받고 오래된 것부터 싣는다
/// (처리할 차례다). 끝낸 항목은 최근 것부터 싣는다. 한 번 넘치면 그 뒤는 싣지 않아, 실은 것은 늘 이어진 앞(또는 뒤)이다.
/// 실은 항목은 원래 차례(만든 때)로 돌려준다.
pub(crate) fn fit_inbox(items: Vec<InboxItem>, budget: usize) -> InboxPage {
    let (open, done): (Vec<_>, Vec<_>) = items.into_iter().partition(|i| !i.done);
    let mut used = 0usize;
    let mut take = |list: Vec<InboxItem>| {
        let (mut kept, mut omitted) = (Vec::new(), 0u32);
        for i in list {
            let size = serde_json::to_vec(&i).map_or(usize::MAX, |b| b.len());
            if omitted == 0 && used.saturating_add(size) <= budget {
                used += size;
                kept.push(i);
            } else {
                omitted += 1;
            }
        }
        (kept, omitted)
    };
    let (mut items, omitted_open) = take(open);
    let (recent, omitted_done) = take(done.into_iter().rev().collect());
    items.extend(recent);
    items.sort_by(|a, b| (a.created_at_ms, &a.id).cmp(&(b.created_at_ms, &b.id)));
    InboxPage { items, omitted_open, omitted_done }
}

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

/// 사람이 읽는 대상 이름(`machine mac-b`, `t-0001`)
pub(crate) fn who(i: &InboxItem) -> String {
    match i.item_kind.as_str() {
        // `gh` 실패(pr.rs)는 머신이 아니다
        "outage" if i.subject == "gh" => "gh".into(),
        "outage" | "health" | "session" => format!("machine {}", i.subject),
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

/// 옛 decisions.md를 읽는 상한(이보다 크면 결정 기록이 아니다)
const OLD_DECISIONS_CAP: u64 = 1 << 20;

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

    /// 흐름 사건 → inbox 항목. 보고·질문·사본 실패는 사건마다가 아니라 묶음 전이로 낸다(transitions.rs).
    /// 남은 것은 herdr-projects가 그때 바로 쓰는 항목이다: brief.rs `give_up`(brief not delivered), ticker.rs
    /// launch(not launched). 메시지 전달 모름은 aaa에만 있다(메시지를 대기열로 보낸다).
    pub(crate) fn inbox_from_flow(&self, e: &FlowEvent) {
        let Some(thread) = &e.thread else { return };
        // 사후 평가자의 알림은 평가 기록(`evaluation`)으로 따로 알린다. 실패는 그대로 알린다.
        if e.role == Some(aaa_proto::Role::Evaluator) && e.kind == "message_unknown" {
            return;
        }
        let p = &e.project;
        let (event, summary) = match e.kind.as_str() {
            "brief_unknown" => (
                "brief not delivered",
                format!(
                    "{thread}: brief를 보내지 못했다({}). `aaa thread read {p} {thread}`로 pane을 보고, 입력창이 비면 `aaa thread brief {p} {thread}`가 보낸다",
                    e.detail
                ),
            ),
            "message_unknown" => (
                "message not delivered",
                format!(
                    "{thread}: 메시지 {}. 다시 보내지 말고 `aaa thread read {p} {thread}`로 입력창을 본다(남았으면 `aaa thread keys {p} {thread} enter`)",
                    e.detail
                ),
            ),
            "thread_failed" => ("not launched", format!("{thread}: 시작하지 못했다: {}", e.detail)),
            _ => return,
        };
        self.inbox_add(p, "thread-state", thread, event, &summary);
    }

    /// `aaa inbox list`: peek가 아니면 보인 항목을 「봤다」로 표시한다
    /// 실은 항목만 「봤다」로 표시한다: 못 실은 항목은 다음에 보일 때 처음 보는 것이다.
    pub(crate) fn inbox(&self, project: &str, all: bool, peek: bool) -> Result<InboxPage, ErrorBody> {
        self.project_dir(project)?;
        let mut page = fit_inbox(self.db.inbox_list(Some(project), all).map_err(internal)?, INBOX_REPLY_BYTES);
        if !peek {
            for i in page.items.iter_mut().filter(|i| !i.done && !i.seen) {
                i.seen = true;
                self.db.inbox_put(i).map_err(internal)?;
            }
        }
        Ok(page)
    }

    /// 항목을 끝낸다: 투영 파일을 `inbox/done/`으로 옮긴다. 끝낸 수와 남은 항목을 돌려준다
    /// (herdr-projects `inbox done`은 「N item(s) moved to inbox/done」을 쓴다, src/cli.rs).
    pub(crate) fn inbox_done(
        &self,
        project: &str,
        ids: &[String],
        all: bool,
        actor: &str,
    ) -> Result<(u32, InboxPage), ErrorBody> {
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
        Ok((n as u32, fit_inbox(self.db.inbox_list(Some(project), false).map_err(internal)?, INBOX_REPLY_BYTES)))
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

    // ───────────── ticker ─────────────

    pub fn start_ticker(self: &Arc<Self>) {
        let me = self.clone();
        tokio::spawn(async move {
            let every = Duration::from_secs(me.cfg.projects.tick_secs.max(1));
            loop {
                // 사후 평가자 묶음·PR 조회 때가 더 이르면 그때까지만 잔다
                let sleep = [me.evaluator_next_due(), me.pr_next_due()]
                    .into_iter()
                    .flatten()
                    .map(|d| d.saturating_duration_since(std::time::Instant::now()))
                    .fold(every, Duration::min);
                tokio::select! {
                    _ = tokio::time::sleep(sleep) => {}
                    _ = me.tick_wake.notified() => {}
                }
                me.tick_started.fetch_add(1, Ordering::SeqCst);
                me.tick().await;
                me.tick_done.send_modify(|n| *n += 1);
            }
        });
    }

    /// tick을 하나 깨우고, 이 요청 뒤에 시작한 tick이 끝나면 끝난 tick 수를 돌려준다. 돌던 tick이 있으면 그 다음 것까지 기다린다
    /// (그 tick은 이 요청 전의 상태를 봤을 수 있다). 시험이 「host가 보고 아무것도 하지 않았다」를 sleep 없이 확인한다(docs/testing.md).
    pub(crate) async fn ticker_tick(&self) -> u64 {
        let want = self.tick_started.load(Ordering::SeqCst) + 1;
        let mut done = self.tick_done.subscribe();
        self.tick_wake.notify_one();
        match done.wait_for(|n| *n >= want).await {
            Ok(n) => *n,
            Err(_) => *self.tick_done.borrow(),
        }
    }

    async fn tick(self: &Arc<Self>) {
        // 멈추거나 보관한 프로젝트는 건너뛴다(herdr-projects ticker, plan §8.155)
        let projects = self.active_projects();
        let names: Vec<String> = projects.iter().map(|p| p.name.clone()).collect();
        self.check_outages(&names);
        self.check_health_items(&names);
        self.evaluator_pass();
        for p in &projects {
            self.check_project_config(&p.name, Path::new(&p.path));
            self.check_old_decisions(&p.name, Path::new(&p.path));
            self.thread_pass(&p.name);
            self.resolve_merged_pass(&p.name).await;
            self.auto_resolve_pass(&p.name).await;
            let dir = Path::new(&p.path);
            self.routine_pass(&p.name, dir, self.coordinator_pane(dir).is_some());
            self.pr_tick(&p.name);
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

    /// 머신이 오래 끊겼다·돌아왔다. 그 머신에 열린 thread가 있는 프로젝트에 알린다.
    /// 끊긴 시간은 이 host 프로세스가 처음 끊김을 본 때부터 잰다(herdr-projects steps.rs `Outage`).
    /// 옛 `last_seen`으로 재면 host를 다시 띄운 직후 아직 붙지 않은 머신이 오래 끊긴 것으로 보인다.
    fn check_outages(&self, projects: &[String]) {
        let now = aaa_common::util::now_ms();
        let limit = self.cfg.projects.outage_secs * 1000;
        for m in self.machine_views() {
            let ok = m.connection != aaa_proto::Connection::Disconnected;
            let known = self.outages.lock().unwrap().contains_key(&m.id);
            let reported = !known
                && projects.iter().any(|p| self.db.inbox_open_exists(p, "outage", &m.name, "down").unwrap_or(false));
            let event = self
                .outages
                .lock()
                .unwrap()
                .entry(m.id.clone())
                .or_insert_with(|| Outage::resumed(reported))
                .record(ok, now, limit);
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

    /// 옛 aaa의 결정 기록(decisions.md)은 brief에 더 넣지 않는다(결정은 memory에 적는다, D10 G9). 결정이 적혀 있으면
    /// 내용 hash당 한 번 coordinator에게 memory로 옮기라고 알린다. host는 memory를 쓰지 않는다(coordinator 몫).
    /// 프로젝트 폴더는 agent가 고칠 수 있다: symlink·FIFO·큰 파일은 읽지 않고, 크기·시각이 그대로면 tick마다 다시 읽지 않는다.
    fn check_old_decisions(&self, project: &str, dir: &Path) {
        let path = dir.join("decisions.md");
        let Ok(meta) = std::fs::symlink_metadata(&path) else { return };
        if !meta.is_file() || meta.len() > OLD_DECISIONS_CAP {
            return;
        }
        let stamp = format!("{}:{:?}", meta.len(), meta.modified().ok());
        let seen = format!("old_decisions_stamp:{project}");
        if self.db.meta(&seen).ok().flatten().as_deref() == Some(stamp.as_str()) {
            return;
        }
        let _ = self.db.set_meta(&seen, &stamp);
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let n = text
            .lines()
            .filter_map(|l| l.strip_prefix("## "))
            .filter(|h| h.trim_start().starts_with(|c: char| c.is_ascii_digit()))
            .count();
        if n == 0 {
            return;
        }
        let hash = hex::encode(sha2::Sha256::digest(text.as_bytes()));
        let key = format!("old_decisions:{project}");
        if self.db.meta(&key).ok().flatten().as_deref() == Some(hash.as_str()) {
            return;
        }
        let _ = self.db.set_meta(&key, &hash);
        self.inbox_add(
            project,
            "config-error",
            "decisions.md",
            "not read",
            &format!(
                "decisions.md의 결정 {n}건은 thread brief에 더 들어가지 않는다(결정은 memory에 적는다). 뒤 thread가 알아야 할 것을 \
                 memory로 옮기고(MEMORY.md 색인 + memory/ 파일) decisions.md를 지운다"
            ),
        );
    }

    /// 프로젝트 폴더를 cwd로 쓰는 host 머신의 실행 중 pane(coordinator)
    fn coordinator_pane(&self, dir: &Path) -> Option<aaa_proto::PaneView> {
        self.coordinator_panes(dir).into_iter().next()
    }

    /// 프로젝트 폴더를 cwd로 쓰는 host 머신의 실행 중 pane 모두(herdr-projects 「Any agent whose working directory is `cwd`
    /// is a coordinator」)
    pub(crate) fn coordinator_panes(&self, dir: &Path) -> Vec<aaa_proto::PaneView> {
        let local: Vec<String> = self.machine_views().into_iter().filter(|m| m.local).map(|m| m.id).collect();
        self.pane_views(DeviceRole::Local)
            .into_iter()
            .filter(|p| {
                local.contains(&p.machine_id)
                    && p.state.is_running()
                    && p.connection == aaa_proto::Connection::Connected
                    && p.cwd.as_deref().is_some_and(|c| same_dir(c, dir))
            })
            .collect()
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
        // coordinator가 없거나 nudge를 껐으면 묶음 hash만 남긴다(herdr-projects steps.rs `nudge`): 사람에게는 사건마다
        // 알림이 따로 갔고, 항목은 coordinator의 다음 턴 `context`가 보인다. 나중에 생긴 coordinator를 옛 묶음으로 깨우지 않는다
        let coord = if self.settings(project).nudge { self.coordinator_pane(dir) } else { None };
        let Some(coord) = coord else {
            let _ = self.db.set_meta(&key, &hash);
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

impl Outage {
    /// host를 다시 띄운 뒤 처음 본다. 앞 host가 알린 끊김이 아직 열린 inbox 항목으로 남았으면(`reported`) 알린 것으로
    /// 시작한다: 돌아오면 「다시 된다」를 한 번 쓰고, 이어 끊겨도 다시 알리지 않는다. herdr-projects `Memory`는 이 상태를
    /// 저장하지 않아 다시 띄우면 그 항목에 회복이 영영 오지 않는다(참고와 다르게 둔 것, plan §8.134)
    pub(crate) fn resumed(reported: bool) -> Outage {
        Outage { failing_since: None, reported }
    }
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
        // 다시 띄운 host: 앞 host가 알린 끊김이 열려 있으면 이어 끊겨도 다시 알리지 않고 돌아오면 한 번 알린다
        let mut outage = Outage::resumed(true);
        assert_eq!(outage.record(false, 0, 10 * min), None);
        assert_eq!(outage.record(false, 30 * min, 10 * min), None);
        assert_eq!(outage.record(true, 31 * min, 10 * min), Some(OutageEvent::Recovered));
        assert_eq!(Outage::resumed(false).record(true, 0, 10 * min), None);
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
            item("thread-state", "t-0001", "new report"),
            item("thread-state", "t-0001", "blocked on a prompt"),
            item("outage", "mac-b", "down"),
            item("thread-state", "t-0002", "new report"),
            item("session", "mac-c", "sessions ended"),
        ];
        assert_eq!(
            nudge_text("demo", &items),
            "[aaa inbox] t-0001 new report, blocked on a prompt; machine mac-b down; t-0002 new report; machine mac-c sessions ended (aaa inbox list demo)"
        );
        let many: Vec<_> = (1..=7).map(|n| item("thread-state", &format!("t-000{n}"), "new report")).collect();
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
        let mut i = item("thread-state", "t-0001", "new report");
        i.summary = "따옴표 \" 와 줄\n바꿈".into();
        let md = render_item(&i);
        let body = md.trim().trim_start_matches("+++").trim_end_matches("+++");
        let doc = body.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(doc["summary"].as_str(), Some("따옴표 \" 와 줄 바꿈"));
        assert_eq!(doc["kind"].as_str(), Some("thread-state"));
        assert_eq!(safe_subject("Mac B/외장 x!!"), "mac-b-x");
    }

    /// 크기 한도: 끝내지 않은 항목이 먼저(오래된 것부터), 끝낸 항목은 최근 것부터, 넘친 뒤로는 싣지 않는다(t-0334 결함 2)
    #[test]
    fn inbox_page_fits_open_first_then_recent_done() {
        let mk = |n: u64, done: bool| {
            let mut i = item("thread-state", &format!("t-{n:04}"), "idle");
            i.id = format!("{n:04}");
            i.created_at_ms = n;
            i.done = done;
            i
        };
        let size = serde_json::to_vec(&mk(1, false)).unwrap().len();
        let all: Vec<_> = (1..=4).map(|n| mk(n, true)).chain((5..=6).map(|n| mk(n, false))).collect();
        let ids = |p: &InboxPage| p.items.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        let p = fit_inbox(all.clone(), size * 100);
        assert_eq!((p.items.len(), p.omitted_open, p.omitted_done), (6, 0, 0));
        // 자리 셋: 끝내지 않은 둘, 끝낸 것 중 가장 최근 하나. 원래 차례로 돌려준다
        let p = fit_inbox(all.clone(), size * 3);
        assert_eq!(ids(&p), ["0004", "0005", "0006"]);
        assert_eq!((p.omitted_open, p.omitted_done), (0, 3));
        // 끝내지 않은 것만으로 넘친다: 오래된 것 하나만, 끝낸 것은 하나도
        let p = fit_inbox(all, size);
        assert_eq!(ids(&p), ["0005"]);
        assert_eq!((p.omitted_open, p.omitted_done), (1, 4));
        // 한 번 넘치면 뒤의 작은 항목도 싣지 않는다(실은 것은 이어진 앞이다)
        let mut big = mk(7, false);
        big.body = "x".repeat(size * 2);
        let p = fit_inbox(vec![mk(1, false), big, mk(9, false)], size * 2);
        assert_eq!(ids(&p), ["0001"]);
        assert_eq!(p.omitted_open, 2);
    }
}
