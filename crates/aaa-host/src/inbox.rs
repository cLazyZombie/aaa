//! inbox·ticker·coordinator 알림(nudge)(§8.9, 0.2.35 계승).
//!
//! - **inbox:** 잃지 않는 항목과 끝내기(done)로 관리한다. DB가 원본이고 `projects/<p>/inbox/<id>.md`(TOML front
//!   matter, 0.2.35 형식)는 투영이다. 끝낸 항목은 `inbox/done/`으로 옮기고 `inbox_done_days`가 지나면 지운다.
//! - **항목 출처:** ticker의 묶음 전이(`thread-state`: 확인 필요·쉼·새 보고, transitions.rs), thread 사건
//!   (`thread-state`: resolved·brief not delivered·message not delivered·not launched)과 ticker가 찾는 특수 항목
//!   (`session` 한 머신의 세션이 모두 끝남, `outage` 머신 단절·회복, `config-error` project.toml 오류).
//! - **nudge:** coordinator(프로젝트 폴더를 cwd로 쓰는 host 머신 pane)가 입력 대기 상태로 `nudge_idle_secs`(기본 3초) 이상
//!   머물고 입력창이 비었을 때, 처음 보는 항목 묶음을 한 줄로 알린다. 같은 묶음(hash)은 한 번만 보낸다. 시간 조건만
//!   남으면 독립 알림 작업이 그때 깬다(t-1627: 턴 끝 뒤 약 5초).
//! - **hook 알림(t-1627):** 턴 도중에는 coordinator의 PostToolUse hook이 `inbox_hook_notice`로 아직 알리지 않은 항목을
//!   물어 다음 도구 결과에 한 줄로 붙인다. 알린 항목은 `nudge_told`와 묶음 hash에 남아 턴 끝 nudge가 다시 보내지 않는다.
//!   전달을 모르면(unknown) 다시 보내지 않는다(§8.5 a). coordinator가 없거나 project.toml `nudge = false`면 그 묶음을
//!   본 것으로만 적는다: 항목은 coordinator의 다음 턴 `context`가 보인다(herdr-projects steps.rs `nudge`, plan §8.136).
//! - **ticker:** 배경 작업과 coordinator 알림 검사는 별도의 task다. ticker는 `tick_secs`마다, 그리고 사건이 오면 바로 돈다.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use aaa_proto::{AgentActivity, DeviceRole, ErrorBody, ErrorCode, FlowEvent, InboxItem, ThreadStatus};
use sha2::Digest;

use crate::orch::ThreadRec;
use crate::{Host, err};

mod dispatch;

#[cfg(test)]
mod hook_tests;

/// nudge 문구에 나열하는 대상 수
const NUDGE_SUBJECTS: usize = 5;

/// 같은 관측이 정해진 창을 채웠는지 본다. 입력창은 0초 설정이어도 두 관측이 필요하다.
/// 시각을 받으므로 실제 nudge 경계와 시험이 같은 판정을 쓰며 벽시계를 기다리지 않는다.
fn steady_observation(
    observations: &mut HashMap<String, (String, Instant)>,
    key: &str,
    value: &str,
    now: Instant,
    window: Duration,
    repeated: bool,
) -> bool {
    match observations.get_mut(key) {
        Some((previous, since)) if previous.as_str() == value => now.duration_since(*since) >= window,
        _ => {
            observations.insert(key.to_string(), (value.to_string(), now));
            !repeated && window.is_zero()
        }
    }
}

fn idle_long_enough(activity: &AgentActivity, since_ms: u64, now_ms: u64, window: Duration) -> bool {
    matches!(activity, AgentActivity::Idle | AgentActivity::Started)
        && now_ms.saturating_sub(since_ms) >= window.as_millis() as u64
}

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
pub(crate) fn order(a: &InboxItem, b: &InboxItem) -> std::cmp::Ordering {
    a.created_at_ms
        .cmp(&b.created_at_ms)
        .then_with(|| a.project.cmp(&b.project))
        .then_with(|| aaa_proto::cmp_short_ids(&a.subject, &b.subject))
        .then_with(|| a.id.cmp(&b.id))
}

pub(crate) fn fit_inbox(mut items: Vec<InboxItem>, budget: usize) -> InboxPage {
    items.sort_by(order);
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
    items.sort_by(order);
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
        "outage" | "health" | "session" => format!("machine {}", i.subject),
        "routine" | "routine-approval" => format!("routine {}", i.subject),
        _ => i.subject.clone(),
    }
}

/// 처음 보는 항목 묶음 → nudge 한 줄. 대상별로 사건을 묶고 요약은 넣지 않는다.
pub(crate) fn nudge_text(project: &str, items: &[InboxItem]) -> String {
    let mut order: Vec<String> = Vec::new();
    let mut events: HashMap<String, Vec<String>> = HashMap::new();
    let mut ids: HashMap<String, Vec<String>> = HashMap::new();
    for i in items {
        let who = who(i);
        if !events.contains_key(&who) {
            order.push(who.clone());
        }
        let item_ids = ids.entry(who.clone()).or_default();
        if item_ids.len() < NUDGE_SUBJECTS {
            item_ids.push(i.display_id().to_owned());
        }
        let ev = events.entry(who).or_default();
        if !ev.contains(&i.event) {
            ev.push(i.event.clone());
        }
    }
    let mut parts: Vec<String> = order
        .iter()
        .take(NUDGE_SUBJECTS)
        .map(|w| format!("{w} {} [{}]", events[w].join(", "), ids[w].join(" ")))
        .collect();
    if order.len() > NUDGE_SUBJECTS {
        parts.push(format!("{} more", order.len() - NUDGE_SUBJECTS));
    }
    let reminder = if items.iter().any(|i| i.event == crate::question_guard::EVENT) {
        format!("\n{}", crate::question_guard::REMINDER)
    } else if items.iter().any(|i| i.event == crate::question_guard::STALE_EVENT) {
        format!("\n{}", crate::question_guard::STALE_REMINDER)
    } else {
        String::new()
    };
    format!("[aaa inbox] {} (aaa inbox list {project}){reminder}", parts.join("; "))
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

/// Claude의 여러 줄 입력은 테두리 전체를 확인한다. 테두리 없는 단순 프롬프트는 마지막 줄만 비어 있어야 한다.
pub(crate) fn nudge_prompt_empty(screen: &aaa_proto::ScreenText, agent: Option<&aaa_proto::AgentStatus>) -> bool {
    if !agent.is_some_and(|a| a.harness == "claude") {
        return prompt_empty(&screen.lines);
    }
    match crate::prompt_box::check("claude", &screen.ansi) {
        crate::prompt_box::Draft::Empty => true,
        crate::prompt_box::Draft::Typed => false,
        crate::prompt_box::Draft::Unknown => {
            screen.lines.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty()) == Some("❯")
        }
    }
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
        let mut item = InboxItem {
            short_id: None,
            id: String::new(),
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
        // ID는 초·종류·대상·프로젝트 순번이라, 두 프로젝트가 같은 초에 같은 대상(머신 사건)을 쓰면 겹칠 수 있다: 빈 번호까지 올린다.
        // 옛 저장은 겹친 ID의 앞 항목 내용을 덮어썼다(plan §8.209)
        for n in n.. {
            item.id = format!("{}-{kind}-{}-{n}", aaa_common::util::utc_compact(now), safe_subject(subject));
            match self.db.inbox_insert(&item) {
                Ok(true) => break,
                Ok(false) => continue,
                Err(e) => {
                    tracing::error!("inbox 저장 실패: {e:#}");
                    return;
                }
            }
        }
        self.inbox_publish(&item);
    }

    pub(crate) fn inbox_publish(&self, item: &InboxItem) {
        self.bump(); // 사용자 요청의 설치 앱 배지도 다시 읽는다
        if crate::asks::is_user_request(item) {
            self.asks_bump();
        }
        self.inbox_bump(); // 웹 알림함 배지·패널도
        let dir = self.inbox_dir(&item.project);
        if let Err(e) = std::fs::create_dir_all(&dir)
            .and_then(|_| std::fs::write(dir.join(format!("{}.md", item.id)), render_item(item)))
        {
            tracing::warn!("inbox 파일 쓰기 실패: {e}");
        }
        self.wake_nudge();
        self.tick_wake.notify_one();
    }

    /// 특수 항목: 같은 종류·대상·사건의 끝내지 않은 항목이 있으면 만들지 않는다
    pub(crate) fn inbox_once(&self, project: &str, kind: &str, subject: &str, event: &str, summary: &str) {
        if !self.db.inbox_open_exists(project, kind, subject, event).unwrap_or(true) {
            self.inbox_add(project, kind, subject, event, summary);
        }
    }

    /// 흐름 사건 → inbox 항목. 보고·질문은 사건마다가 아니라 묶음 전이로 낸다. 사본 실패는 재수집 뒤 시스템 경고로 낸다(transitions.rs).
    /// 남은 것은 herdr-projects가 그때 바로 쓰는 항목이다: brief.rs `give_up`(brief not delivered), ticker.rs
    /// launch(not launched). 메시지 전달 모름은 aaa에만 있다(메시지를 대기열로 보낸다).
    pub(crate) fn inbox_from_flow(&self, e: &FlowEvent) {
        let Some(thread) = &e.thread else { return };

        let p = &e.project;
        let (event, summary) = match e.kind.as_str() {
            "brief_unknown" => (
                "brief not delivered",
                format!(
                    "{thread}: brief를 보내지 못했다({}). `aaa thread read {p} {thread}`로 pane을 보고, 입력창이 비면 `aaa thread brief {p} {thread}`가 보낸다",
                    e.detail
                ),
            ),
            "message_unknown" => {
                let summary = if let Some(id) = &e.evidence {
                    let detail = e.detail.strip_prefix(id).unwrap_or(&e.detail).trim_start();
                    format!("{thread}: {id} · 메시지 수락 확인 못함 · {detail}")
                } else {
                    format!(
                        "{thread}: 메시지 수락 확인 못함 {}. 다시 보내지 말고 `aaa thread read {p} {thread}`로 입력창을 본다(남았으면 `aaa thread keys {p} {thread} enter`)",
                        e.detail
                    )
                };
                if let Some(id) = &e.evidence
                    && self.revise_waiting_answer_notice(p, thread, id, "message not delivered", &summary)
                {
                    return;
                }
                ("message not delivered", summary)
            }
            "message_accepted_late" => {
                if let Some(id) = &e.evidence {
                    self.inbox_accept_late_message(p, thread, id);
                }
                return;
            }
            "thread_failed" => ("not launched", format!("{thread}: 시작하지 못했다: {}", e.detail)),
            _ => return,
        };
        self.inbox_add(p, "thread-state", thread, event, &summary);
    }

    fn inbox_accept_late_message(&self, project: &str, thread: &str, id: &str) {
        let _projection = self.inbox_projection.lock().unwrap();
        let item = match self.db.message_accept_late(project, thread, id) {
            Ok(Some(item)) => item,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("늦은 메시지 수락 상태 저장 실패: {e:#}");
                return;
            }
        };
        self.update_inbox_projection(&item);
    }

    pub(crate) fn revise_waiting_answer_notice(
        &self,
        project: &str,
        thread: &str,
        message: &str,
        event: &str,
        summary: &str,
    ) -> bool {
        let _projection = self.inbox_projection.lock().unwrap();
        match self.db.revise_waiting_answer_notice(project, thread, message, event, summary) {
            Ok(Some(item)) => {
                self.update_inbox_projection(&item);
                true
            }
            Ok(None) => false,
            Err(e) => {
                tracing::warn!(message, "카드 답 알림 상태 저장 실패: {e:#}");
                false
            }
        }
    }

    fn update_inbox_projection(&self, item: &InboxItem) {
        let dir = self.inbox_dir(&item.project);
        let name = format!("{}.md", item.id);
        let open = dir.join(&name);
        // DB에서만 끝난 경우 기존 파일을 고친다. 완료 쪽이 이 잠금을 받은 뒤 그 파일을 옮긴다.
        let path = if item.done && !open.exists() { dir.join("done").join(name) } else { open };
        if let Err(e) = crate::orch::write_atomic(&path, render_item(item).as_bytes()) {
            tracing::warn!("메시지 전달 알림 파일 쓰기 실패: {e:#}");
        }
        self.inbox_bump();
    }

    /// `aaa inbox list`: peek가 아니면 보인 항목을 「봤다」로 표시한다
    /// 실은 항목만 「봤다」로 표시한다: 못 실은 항목은 다음에 보일 때 처음 보는 것이다.
    /// inbox 항목을 「봤다」로 남기는 호출자: 세션 밖의 사람(터미널·웹)과 그 프로젝트 coordinator. thread·자동화 pane이
    /// context·inbox를 읽어도 coordinator 알림(nudge)이 빠지지 않는다(t-1225: thread brief가 `aaa context`를 안내했다).
    pub(crate) fn inbox_seen_by(&self, project: &str, actor: &str) -> bool {
        match self.coordinator_project_of_actor(actor) {
            Some(p) => p == project,
            None => {
                !crate::front::in_session(actor)
                    && !["worker:", "evaluator:", "detached:"].iter().any(|p| actor.starts_with(p))
            }
        }
    }

    pub(crate) fn inbox(&self, project: &str, all: bool, peek: bool) -> Result<InboxPage, ErrorBody> {
        self.project_dir(project)?;
        let mut items = self.db.inbox_list(Some(project), all).map_err(internal)?;
        self.db.inbox_display_ids(&mut items).map_err(internal)?;
        let mut page = fit_inbox(items, INBOX_REPLY_BYTES);
        if !peek {
            for i in page.items.iter_mut().filter(|i| !i.done && !i.seen) {
                i.seen = true;
                self.db.inbox_mark_seen(project, &i.id).map_err(internal)?;
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
        let selectors = ids;
        let ids: Vec<String> = ids
            .iter()
            .map(|id| {
                self.db
                    .inbox_long_id(project, id)
                    .map_err(|e| err(ErrorCode::BadRequest, format!("{e:#}")))?
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("끝내지 않은 항목이 없다: {id}")))
            })
            .collect::<Result<_, _>>()?;
        let open = self.db.inbox_list(Some(project), false).map_err(internal)?;
        if let Some((miss, _)) = selectors.iter().zip(&ids).find(|(_, id)| !open.iter().any(|i| &&i.id == id)) {
            return Err(err(ErrorCode::NotFound, format!("끝내지 않은 항목이 없다: {miss}")));
        }
        if let Some(item) = open
            .iter()
            .find(|i| ids.contains(&i.id) && crate::asks::is_user_request(i) && !self.request_has_open_question(i))
        {
            return Err(err(
                ErrorCode::BadRequest,
                format!(
                    "미답변 사용자 요청은 inbox 정리로 닫지 않는다: {}. 카드에서 답하거나 aaa post handled <q-ID|p-ID>로 명시적으로 처리한다",
                    selectors[ids.iter().position(|id| *id == item.id).unwrap()]
                ),
            ));
        }
        if let Some(item) = open.iter().find(|i| ids.contains(&i.id) && self.unresolved_followup_inbox(i)) {
            return Err(err(
                ErrorCode::BadRequest,
                format!("{} 후속 남음: 후속 할 일 연결이나 사유 기록 전에는 inbox를 닫지 않는다", item.subject),
            ));
        }
        let preserved = open
            .iter()
            .filter(|i| all && (crate::asks::is_user_request(i) || self.unresolved_followup_inbox(i)))
            .count();
        let pick: Vec<InboxItem> = open
            .into_iter()
            .filter(|i| {
                if all {
                    !crate::asks::is_user_request(i) && !self.unresolved_followup_inbox(i)
                } else {
                    ids.contains(&i.id)
                }
            })
            .collect();
        let n = self.inbox_finish(project, pick, &self.answered_by(actor), true).map_err(internal)?;
        self.db.audit(
            actor,
            "inbox_done",
            &format!("{project} {n} all={all} preserved_user_requests={preserved} ids={}", ids.join(" ")),
        );
        Ok((n, self.inbox(project, false, true)?))
    }

    /// 조건에 맞는 끝내지 않은 항목을 끝낸다
    pub(crate) fn inbox_finish_where(&self, project: &str, f: impl Fn(&InboxItem) -> bool) {
        let pick: Vec<InboxItem> =
            self.db.inbox_list(Some(project), false).unwrap_or_default().into_iter().filter(|i| f(i)).collect();
        if !pick.is_empty()
            && let Err(e) = self.inbox_finish(project, pick, "host", true)
        {
            tracing::warn!("inbox 항목을 끝내지 못했다: {e:#}");
        }
    }

    /// `by`는 끝낸 곳(확인 필요 post의 「답한 것」 기록에 남는다). 실제 끝낸 수를 돌려준다.
    pub(crate) fn inbox_finish(
        &self,
        project: &str,
        items: Vec<InboxItem>,
        by: &str,
        record_requests: bool,
    ) -> anyhow::Result<u32> {
        self.inbox_finish_with_notice(project, items, by, record_requests, |_| None)
    }

    /// 종료를 차지한 경로만 처리 알림을 저장하고 투영·상태 사건을 낸다.
    pub(crate) fn inbox_finish_with_notice(
        &self,
        project: &str,
        items: Vec<InboxItem>,
        by: &str,
        record_requests: bool,
        summary: impl Fn(&InboxItem) -> Option<String>,
    ) -> anyhow::Result<u32> {
        #[cfg(test)]
        {
            let barrier = self.inbox_finish_barrier.lock().unwrap().clone();
            if let Some(barrier) = barrier {
                barrier.wait();
            }
        }
        let now = aaa_common::util::now_ms();
        let mut finished = 0;
        for i in items {
            if self.unresolved_followup_inbox(&i) {
                continue;
            }
            let notice = summary(&i).map(|summary| InboxItem {
                short_id: None,
                id: format!("{}-handled", i.id),
                project: project.into(),
                item_kind: "thread-state".into(),
                subject: "coordinator".into(),
                event: "request handled".into(),
                summary: summary.chars().take(300).collect(),
                body: String::new(),
                done: false,
                seen: false,
                created_at_ms: now,
                done_at_ms: None,
            });
            let Some(done) = self.db.inbox_finish(project, &i.id, now, notice.as_ref())? else { continue };
            finished += 1;
            self.inbox_finish_publish(&done, by, record_requests);
            if let Some(notice) = notice {
                self.inbox_publish(&notice);
            }
        }
        Ok(finished)
    }

    /// DB에서 끝낸 항목의 파일 투영과 상태 사건. 구조화 질문의 답 저장도 이 경로를 쓴다.
    pub(crate) fn inbox_finish_publish(&self, i: &InboxItem, by: &str, record_requests: bool) {
        if crate::asks::is_user_request(i) {
            if record_requests {
                self.record_request(i, by, i.done_at_ms.unwrap_or_else(aaa_common::util::now_ms));
            }
            self.asks_bump();
        }
        self.bump(); // 이미 처리한 사용자 요청은 다른 기기에서도 줄어든다
        let dir = self.inbox_dir(&i.project);
        let _projection = self.inbox_projection.lock().unwrap();
        aaa_common::diagnostic::ignored(
            std::fs::create_dir_all(dir.join("done")),
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
        aaa_common::diagnostic::ignored(
            std::fs::rename(dir.join(format!("{}.md", i.id)), dir.join("done").join(format!("{}.md", i.id))),
            "정리 실패가 본 작업의 결과를 덮지 않게 한다",
        );
        self.inbox_bump();
    }

    // ───────────── ticker ─────────────

    pub fn start_ticker(self: &Arc<Self>) {
        self.start_nudges();
        let me = self.clone();
        self.supervise(crate::supervisor::Task::Ticker, move || {
            let me = me.clone();
            async move {
                let every = Duration::from_secs(me.cfg.projects.tick_secs.max(1));
                loop {
                    // 다음 조회·배경 작업 차례가 더 이르면 그때 깨어난다
                    let sleep = [me.ci_next_due(), me.git_next_due(), me.background_wait_due()]
                        .into_iter()
                        .flatten()
                        .map(|d| d.saturating_duration_since(std::time::Instant::now()))
                        .fold(every, Duration::min);
                    tokio::select! {
                        _ = tokio::time::sleep(sleep) => {}
                        _ = me.tick_wake.notified() => {}
                    }
                    let seq = me.tick_started.fetch_add(1, Ordering::SeqCst) + 1;
                    me.tick().await;
                    me.tick_done.send_replace(seq);
                    me.task_progress(crate::supervisor::Task::Ticker);
                }
            }
        });
    }

    /// tick을 하나 깨우고, 이 요청 뒤에 시작한 tick이 끝나면 끝난 tick 수를 돌려준다. 돌던 tick이 있으면 그 다음 것까지 기다린다
    /// (그 tick은 이 요청 전의 상태를 봤을 수 있다). 시험이 「host가 보고 아무것도 하지 않았다」를 sleep 없이 확인한다(docs/testing.md).
    pub(crate) async fn ticker_tick(&self) -> u64 {
        let want = self.tick_started.load(Ordering::SeqCst) + 1;
        let mut done = self.tick_done.subscribe();
        self.tick_wake.notify_one();
        let completed = match done.wait_for(|n| *n >= want).await {
            Ok(n) => *n,
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                *self.tick_done.borrow()
            }
        };
        let mut checked = self.nudge_checked.subscribe();
        let seq = self.wake_nudge();
        let _ = checked.wait_for(|n| *n >= seq).await;
        completed
    }

    async fn tick(self: &Arc<Self>) {
        self.backup_pass_at(aaa_common::util::now_ms());
        self.refresh_usage();
        // Codex 인증은 만료 하루 전에 host가 미리 갱신한다(배경 작업, t-1688)
        self.account_refresh_pass();
        // 알려진 만료 시각에 다가간 계정(곧 만료·끊김)과 이어진 갱신 실패를 알림함에 낸다(plan §8.351)
        self.account_health_pass();
        self.settle_background_waits();
        if let Err(e) = self.library_retention_pass_at(aaa_common::util::now_ms()) {
            tracing::warn!("보고서 자동 정리 실패: {}", e.message);
        }
        // 멈추거나 보관한 프로젝트는 건너뛴다(herdr-projects ticker, plan §8.180)
        let projects = self.active_projects();
        self.resource_warning_pass(&projects);
        let names: Vec<String> = projects.iter().map(|p| p.name.clone()).collect();
        self.check_outages(&names);
        self.check_health_items(&names);
        for p in &projects {
            self.check_project_config(&p.name, Path::new(&p.path));
            self.check_old_decisions(&p.name, Path::new(&p.path));
            self.recover_launches(&p.name);
            self.thread_pass(&p.name);
            self.retry_report_copies(&p.name, aaa_common::util::now_ms());
            self.legacy_mission_notice(&p.name);
            if let Err(e) = self.legacy_mission_recover(&p.name) {
                self.inbox_once(&p.name, "config-error", "missions", "conversion pending", &e.message);
            }
            self.usage_limit_pass(&p.name).await;
            self.pending_answer_pass(&p.name).await;
            self.thread_stall_pass(&p.name).await;
            self.auto_resolve_pass(&p.name).await;
            self.park_pass(&p.name).await;
            self.chat_rest_pass(&p.name).await;
            let dir = Path::new(&p.path);
            self.routine_pass(&p.name, dir, !self.coordinator_panes(dir).is_empty());
            self.automation_tick(&p.name);
            self.ci_tick(&p.name);
            self.git_tick(&p.name);
            self.tasks_pass(&p.name, dir);
            self.task_clean_pass(&p.name, dir);
            if let Err(e) = self.reconcile_question_requests(&p.name) {
                tracing::warn!("닫힌 질문의 알림 정리 실패: {}", e.message);
            }
            if let Err(e) = self.queue_pass(&p.name).await {
                tracing::warn!(project=%p.name,"예약 판정 실패: {}",e.message);
            }
            if let Err(e) = self.pool_pass(&p.name).await {
                tracing::warn!(project=%p.name,"계정 배정 대기 판정 실패: {}",e.message);
            }
        }
        let now = aaa_common::util::now_ms();
        self.ask_prune_pass(now);
        self.auto_prune_pass(now);
        let keep = Duration::from_secs(self.cfg.projects.inbox_done_days * 86_400).as_millis() as u64;
        if let Ok(gone) = self.db.inbox_prune(aaa_common::util::now_ms().saturating_sub(keep)) {
            for (project, id) in gone {
                aaa_common::diagnostic::ignored(
                    std::fs::remove_file(self.inbox_dir(&project).join("done").join(format!("{id}.md"))),
                    "정리 실패가 본 작업의 결과를 덮지 않게 한다",
                );
            }
        }
    }

    pub(crate) fn open_threads(&self, project: &str) -> Vec<ThreadRec> {
        self.db
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
            .filter(|r| r.status == ThreadStatus::Open)
            .collect()
    }

    /// 머신이 오래 끊겼다·돌아왔다. 그 머신에 열린 thread가 있는 프로젝트에 알린다(밖 알림은 한 번으로 묶는다, notify.rs).
    /// 끊긴 시간은 이 host 프로세스가 처음 끊김을 본 때부터 잰다(herdr-projects steps.rs `Outage`).
    /// 옛 `last_seen`으로 재면 host를 다시 띄운 직후 아직 붙지 않은 머신이 오래 끊긴 것으로 보인다.
    fn check_outages(&self, projects: &[String]) {
        let machines: Vec<(String, String, bool)> = self
            .machine_views()
            .into_iter()
            .map(|m| {
                let ok = m.connection != aaa_proto::Connection::Disconnected;
                (m.id, m.name, ok)
            })
            .collect();
        self.outage_pass(projects, &machines, aaa_common::util::now_ms());
    }

    /// 머신마다(ID, 이름, 붙었나) 끊김을 한 번 센다
    pub(crate) fn outage_pass(&self, projects: &[String], machines: &[(String, String, bool)], now: u64) {
        let limit = self.cfg.projects.outage_secs * 1000;
        for (id, name, ok) in machines {
            let known = self.outages.lock().unwrap().contains_key(id);
            let reported = !known && projects.iter().any(|p| self.outage_open(p, name, "down"));
            let event = self
                .outages
                .lock()
                .unwrap()
                .entry(id.clone())
                .or_insert_with(|| Outage::resumed(reported))
                .record(*ok, now, limit);
            let Some(event) = event else { continue };
            for p in projects {
                if !self.open_threads(p).iter().any(|r| r.machine_id == *id) {
                    continue;
                }
                match event {
                    OutageEvent::Down if !self.outage_open(p, name, "down") => self.inbox_add(
                        p,
                        "outage",
                        name,
                        "down",
                        &format!(
                            "머신 {name}: {}분째 연결되지 않는다. thread는 마지막 상태를 유지한다",
                            limit / 60_000
                        ),
                    ),
                    OutageEvent::Down => {}
                    OutageEvent::Recovered => {
                        self.inbox_add(p, "outage", name, "recovered", &format!("머신 {name}: 다시 연결됐다"))
                    }
                }
            }
        }
    }

    /// 알린 끊김이 아직 이어지나: 그 대상의 마지막 outage 항목이 끝내지 않은 끊김(머신 `down`, `gh` `failing`)이다.
    /// 뒤에 회복 항목이 있으면 끝난 끊김이다(끊김 항목을 끝내지 않았어도). 열린 끊김 항목만 보면 회복을 쓴 뒤에도 host를
    /// 다시 띄울 때마다 「다시 연결」을 또 썼다(plan §8.209)
    pub(crate) fn outage_open(&self, project: &str, subject: &str, down: &str) -> bool {
        matches!(self.db.inbox_last(project, "outage", subject), Ok(Some((event, false))) if event == down)
    }

    /// 설정 파일(`aaa.toml`, 없으면 project.toml)을 읽지 못하면 파일 내용 hash당 한 번 알린다
    fn check_project_config(&self, project: &str, dir: &Path) {
        let path = aaa_config::project_settings_file(dir);
        let file = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let Ok(text) = std::fs::read_to_string(&path)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let Err(e) = text.parse::<toml_edit::DocumentMut>() else { return };
        let hash = hex::encode(sha2::Sha256::digest(text.as_bytes()));
        let key = format!("config_error:{project}");
        if self
            .db
            .meta(&key)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .as_deref()
            == Some(hash.as_str())
        {
            return;
        }
        aaa_common::diagnostic::ignored(self.db.set_meta(&key, &hash), "보조 작업 실패 뒤 기존 처리를 계속한다");
        let first = e.to_string().lines().next().unwrap_or_default().to_string();
        self.inbox_add(project, "config-error", &file, "not usable", &format!("{file}을 읽지 못한다: {first}"));
    }

    /// 옛 aaa의 결정 기록(decisions.md)은 brief에 더 넣지 않는다(결정은 memory에 적는다, D10 G9). 결정이 적혀 있으면
    /// 내용 hash당 한 번 coordinator에게 memory로 옮기라고 알린다. host는 memory를 쓰지 않는다(coordinator 몫).
    /// 프로젝트 폴더는 agent가 고칠 수 있다: symlink·FIFO·큰 파일은 읽지 않고, 크기·시각이 그대로면 tick마다 다시 읽지 않는다.
    fn check_old_decisions(&self, project: &str, dir: &Path) {
        let path = dir.join("decisions.md");
        let Ok(meta) = std::fs::symlink_metadata(&path)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        if !meta.is_file() || meta.len() > OLD_DECISIONS_CAP {
            return;
        }
        let stamp = format!(
            "{}:{:?}",
            meta.len(),
            meta.modified()
                .inspect_err(|error| aaa_common::diagnostic::observe(
                    error,
                    "선택 자료를 읽지 못해 기존 fallback을 쓴다"
                ))
                .ok()
        );
        let seen = format!("old_decisions_stamp:{project}");
        if self
            .db
            .meta(&seen)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .as_deref()
            == Some(stamp.as_str())
        {
            return;
        }
        aaa_common::diagnostic::ignored(self.db.set_meta(&seen, &stamp), "보조 작업 실패 뒤 기존 처리를 계속한다");
        let Ok(text) = std::fs::read_to_string(&path)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
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
        if self
            .db
            .meta(&key)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .as_deref()
            == Some(hash.as_str())
        {
            return;
        }
        aaa_common::diagnostic::ignored(self.db.set_meta(&key, &hash), "보조 작업 실패 뒤 기존 처리를 계속한다");
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

    /// 처음 보는 끝내지 않은 항목과 그 ID 묶음 hash.
    fn unseen_open(&self, project: &str) -> (Vec<InboxItem>, String) {
        let unseen: Vec<InboxItem> =
            self.db.inbox_list(Some(project), false).unwrap_or_default().into_iter().filter(|i| !i.seen).collect();
        let mut ids: Vec<&str> = unseen.iter().map(|i| i.id.as_str()).collect();
        ids.sort();
        let hash = hex::encode(sha2::Sha256::digest(ids.join("\n").as_bytes()));
        (unseen, hash)
    }

    fn nudge_delivery_lock(&self, project: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.nudge_delivery.lock().unwrap().entry(project.into()).or_default().clone()
    }

    /// 전달 잠금 안에서 현재 묶음과 대조한다. 이미 hook이나 pane에 보낸 항목은 새 묶음에 반복하지 않는다.
    fn fresh_nudges(&self, project: &str, unseen: Vec<InboxItem>) -> Vec<InboxItem> {
        let mut told = self.nudge_told.lock().unwrap();
        let Some(set) = told.get_mut(project) else { return unseen };
        set.retain(|id| unseen.iter().any(|i| &i.id == id));
        unseen.into_iter().filter(|i| !set.contains(&i.id)).collect()
    }

    fn remember_told(&self, project: &str, items: &[InboxItem]) {
        self.nudge_told
            .lock()
            .unwrap()
            .entry(project.to_string())
            .or_default()
            .extend(items.iter().map(|i| i.id.clone()));
    }

    /// coordinator PostToolUse hook: 그 pane에 아직 알리지 않은 처음 보는 항목을 nudge와 같은 한 줄로 돌려준다(t-1627).
    /// 턴 도중에 도착한 항목이 다음 도구 결과에 붙어 그 턴 안에서 보인다. 알린 항목과 지금 묶음 hash를 남겨 턴 끝
    /// nudge가 같은 것을 다시 보내지 않는다. 묻는 쪽은 이 머신 agent(세션 밖 연결)나 그 coordinator pane 자신이다:
    /// thread·자동화 pane이 coordinator 알림을 가로채지 못한다.
    pub(crate) fn inbox_hook_notice(
        &self,
        project: &str,
        pane: &str,
        actor: &str,
    ) -> Result<Option<String>, ErrorBody> {
        let dir = self.project_dir(project)?;
        let outside = actor.starts_with("local:") && !crate::front::in_session(actor);
        let own = self.coordinator_project_of_actor(actor).as_deref() == Some(project)
            && actor.rsplit_once('@').is_some_and(|(_, p)| p == pane);
        if !(outside || own) {
            return Err(err(ErrorCode::Forbidden, "inbox hook 알림은 이 머신 agent나 그 coordinator pane만 묻는다"));
        }
        if !self.settings(project).nudge || !self.coordinator_panes(&dir).iter().any(|p| p.id == pane) {
            return Ok(None);
        }
        // pane이 이미 입력 중이면 그 제출의 수락 hook을 기다리게 하지 않는다.
        let delivery = self.nudge_delivery_lock(project);
        let Ok(_delivery) = delivery.try_lock() else { return Ok(None) };
        let (unseen, hash) = self.unseen_open(project);
        let mut fresh = self.fresh_nudges(project, unseen);
        if fresh.is_empty() {
            return Ok(None);
        }
        self.db.inbox_display_ids(&mut fresh).map_err(internal)?;
        self.remember_told(project, &fresh);
        let text = nudge_text(project, &fresh);
        aaa_common::diagnostic::ignored(
            self.db.set_meta(&format!("nudged:{project}"), &hash),
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
        self.flow_project(project, "nudge_sent", None, format!("hook: {text}"));
        Ok(Some(text))
    }

    pub(crate) async fn nudge(self: &Arc<Self>, project: &str, dir: &Path) -> Option<Duration> {
        let (unseen, hash) = self.unseen_open(project);
        let forget_box = || {
            self.nudge_box.lock().unwrap().remove(project);
        };
        if unseen.is_empty() {
            forget_box();
            return None;
        }
        let key = format!("nudged:{project}");
        if self
            .db
            .meta(&key)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .as_deref()
            == Some(hash.as_str())
        {
            forget_box();
            return None;
        }
        // coordinator가 없거나 nudge를 껐으면 묶음 hash만 남긴다(herdr-projects steps.rs `nudge`): 사람에게는 사건마다
        // 알림이 따로 갔고, 항목은 coordinator의 다음 턴 `context`가 보인다. 나중에 생긴 coordinator를 옛 묶음으로 깨우지 않는다
        let coord = if self.settings(project).nudge {
            let panes = self.coordinator_panes(dir);
            panes
                .iter()
                .find(|p| unseen.iter().any(|i| i.event == crate::question_guard::EVENT && i.subject == p.id))
                .cloned()
                .or_else(|| panes.into_iter().next())
        } else {
            None
        };
        let Some(coord) = coord else {
            aaa_common::diagnostic::ignored(self.db.set_meta(&key, &hash), "보조 작업 실패 뒤 기존 처리를 계속한다");
            forget_box();
            return None;
        };
        let idle_for = Duration::from_secs(self.cfg.projects.nudge_idle_secs);
        let question_missing = unseen.iter().any(|i| i.event == crate::question_guard::EVENT && i.subject == coord.id);
        // 입력 대기: hook 상태가 있으면 그것으로, 없으면 화면이 그만큼 그대로인지로 본다
        let Some(screen) = self.screen(&coord.machine_id, &coord.id).await else {
            forget_box();
            return None;
        };
        let ready = match &coord.agent {
            Some(a) => {
                let window = if question_missing { Duration::ZERO } else { idle_for };
                let now_ms = aaa_common::util::now_ms();
                let ready = idle_long_enough(&a.activity, a.since_ms, now_ms, window);
                if !ready && matches!(a.activity, AgentActivity::Idle | AgentActivity::Started) {
                    let waited = Duration::from_millis(now_ms.saturating_sub(a.since_ms));
                    forget_box();
                    return Some(window.saturating_sub(waited));
                }
                ready
            }
            None => {
                let h = hex::encode(sha2::Sha256::digest(screen.lines.join("\n").as_bytes()));
                let mut seen = self.nudge_screens.lock().unwrap();
                // 화면이 바뀌는 동안은 다시 깨우지 않는다(작업 중일 수 있다): 주기 tick·사건으로 본다
                steady_observation(&mut seen, &coord.id, &h, Instant::now(), idle_for, false)
            }
        };
        if !ready || !nudge_prompt_empty(&screen, coord.agent.as_deref()) {
            forget_box();
            return None;
        }
        // 입력창이 이번 검사와 nudge_quiet_secs 전 검사에서 모두 비어 보여야 보낸다. 사람이 치는 글과 섞이지 않게 한다
        // (herdr-projects steps.rs `box_quiet`: 마지막 키 입력 시각을 알 수 없어 입력창으로 본다).
        if !question_missing {
            let mut boxes = self.nudge_box.lock().unwrap();
            let quiet = Duration::from_secs(self.cfg.projects.nudge_quiet_secs);
            if !steady_observation(&mut boxes, project, &coord.id, Instant::now(), quiet, true) {
                // 첫 관측이면 quiet 전체, 이미 본 것이면 남은 만큼 뒤에 다시 본다
                let left = boxes.get(project).map_or(quiet, |(_, since)| quiet.saturating_sub(since.elapsed()));
                drop(boxes);
                return Some(left);
            }
        }
        // 화면을 기다린 사이 hook이 알렸거나 사용자가 목록을 읽었을 수 있다.
        let delivery = self.nudge_delivery_lock(project);
        let _delivery = delivery.lock().await;
        let (current, current_hash) = self.unseen_open(project);
        if current_hash != hash {
            return Some(Duration::ZERO);
        }
        let mut display = self.fresh_nudges(project, current);
        if display.is_empty() {
            forget_box();
            return None;
        }
        if let Err(e) = self.db.inbox_display_ids(&mut display) {
            tracing::error!("inbox 알림 별칭 조회 실패: {e:#}");
            return None;
        }
        let text = nudge_text(project, &display);
        let sent = self.send_nudge(&coord.machine_id, &coord.id, &text).await;
        forget_box();
        let state = match sent {
            Ok(true) => "accepted",
            Ok(false) => "unknown",
            Err(e) if e.typed => {
                tracing::info!(%project, "nudge 입력 결과 미확인: {}", e.msg);
                "unknown"
            }
            Err(e) => {
                tracing::info!(%project, "nudge 보류: {}", e.msg);
                return None;
            }
        };
        self.remember_told(project, &display);
        aaa_common::diagnostic::ignored(self.db.set_meta(&key, &hash), "알림 묶음 저장 실패");
        self.flow_project(project, "nudge_sent", None, format!("{state}: {text}"));
        None
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
mod tests;

#[cfg(test)]
mod alias_tests {
    use super::*;
    use aaa_proto::{FrontRequest as R, FrontResponse as S};

    // 같은 Front 경계의 목록·읽음·혼합 입력·권한·보호 요청을 함께 검사해야 별칭으로 보호를 우회하지 않는다.
    #[tokio::test]
    async fn aliases_keep_front_seen_done_and_request_protection_on_the_stored_item() {
        let root = std::env::temp_dir().join(format!("aaa-inbox-front-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").to_string_lossy().into();
        let h = Host::open(cfg, root.join("state")).unwrap();
        h.project_create("demo", Default::default(), "test").unwrap();
        h.inbox_add("demo", "thread-state", "t-0001", "idle", "첫 항목");
        h.inbox_add("demo", "thread-state", "t-0002", "idle", "둘째 항목");
        h.inbox_add("demo", "post", "coordinator", "post request for user", "미답변 요청");
        let stored = h.db.inbox_list(Some("demo"), false).unwrap();
        let read = |peek| R::Inbox { project: "demo".into(), all: false, peek };
        let S::Inbox { items, .. } = h.call(DeviceRole::Reader, "device:r", read(true)).await.unwrap() else {
            panic!()
        };
        assert_eq!(items.len(), 3);
        for (shown, original) in items.iter().zip(&stored) {
            assert_eq!(h.db.inbox_long_id("demo", &shown.id).unwrap().as_deref(), Some(original.id.as_str()));
            assert_eq!(shown.summary, original.summary);
            assert_eq!(shown.id, original.id);
            assert!(shown.short_id.as_ref().is_some_and(|id| id.starts_with("i-")));
        }
        let second = stored.iter().find(|i| i.summary == "둘째 항목").unwrap();
        assert!(!h.db.inbox_list(Some("demo"), false).unwrap()[0].seen);
        h.call(DeviceRole::Controller, "device:c", read(false)).await.unwrap();
        assert!(h.db.inbox_list(Some("demo"), false).unwrap().iter().all(|i| i.seen));
        let done = |ids: Vec<String>| R::InboxDone { project: "demo".into(), ids, all: false };
        assert!(h.call(DeviceRole::Reader, "device:r", done(vec!["i-1".into()])).await.is_err());
        assert!(h.call(DeviceRole::Controller, "device:c", done(vec!["i-1".into(), "i-9999".into()])).await.is_err());
        assert_eq!(h.db.inbox_list(Some("demo"), false).unwrap().len(), 3);
        let e = h.call(DeviceRole::Controller, "device:c", done(vec!["i-00003".into()])).await.unwrap_err();
        assert!(e.message.contains("미답변"), "{e:?}");
        let S::Inbox { done: Some(n), items, .. } =
            h.call(DeviceRole::Controller, "device:c", done(vec!["i-01".into(), second.id.clone()])).await.unwrap()
        else {
            panic!()
        };
        assert_eq!(n, 2);
        assert_eq!(items[0].short_id.as_deref(), Some("i-0003"));
        for item in stored.iter().filter(|i| i.event == "idle") {
            assert!(h.inbox_dir("demo").join("done").join(format!("{}.md", item.id)).is_file());
        }
        assert!(!h.inbox_dir("demo").join("i-0001.md").exists());
        let first = stored.iter().find(|i| i.summary == "첫 항목").unwrap();
        let mut public = vec![first.clone()];
        h.db.inbox_display_ids(&mut public).unwrap();
        let body = crate::notify::notify_body("json", "", "demo", &public);
        assert_eq!(body["items"][0]["id"], first.id);
        assert_eq!(body["items"][0]["short_id"], "i-0001");
        // 구판 payload는 별칭이 없고, 새 응답의 id도 같은 저장 키다.
        let old_json = serde_json::to_value(first).unwrap();
        assert!(old_json.get("short_id").is_none());
        let old: InboxItem = serde_json::from_value(old_json).unwrap();
        assert_eq!(old.display_id(), first.id);
        let asks = h.user_requests("demo").unwrap();
        assert_eq!(asks[0].id, items[0].id);
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }
}
