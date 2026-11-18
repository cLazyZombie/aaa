//! 구현 맡기기(t-1894 3-2, 확정 계획 §4, t-2002 q-4224 1A): main 문서의 계획 블록 여럿을 할 일 하나로 넘긴다.
//! 할 일을 만들고, 고른 블록 표지에 그 d-ID를 적어 main에 바로 저장하고(저장소 검사 포함), 고르면 예약한다.
//! 그 할 일의 thread는 어느 길로 시작하든(`apply_from_task`) main에서 갈라지고 brief가 맡긴 자리를 알린다.
//! 옛 기록(t-2002 전: 기획 branch 맡김, 표지 없이 글로 찾는 main 맡김)도 읽는다.
//! 맡김 기록은 host 상태 폴더 `design-handoffs/<프로젝트>/<d-ID>.json`에 둔다(TASKS.md에 쓰면 예약한 블록이 바뀐다).
use std::path::PathBuf;
use std::sync::Arc;

use aaa_proto::{
    DesignHandoffView, DesignPick, DeviceRole, ErrorBody, ErrorCode, FrontResponse, TaskOp, TaskView,
};
use serde::{Deserialize, Serialize};

use crate::orch::{StartArgs, bad, internal};
use crate::{Host, err, task_edit, tasks};

/// 넘긴 블록 고침을 막 뜬 구현 thread가 brief를 받을 때까지 기다리는 한도
const RELAY_WAIT: std::time::Duration = std::time::Duration::from_secs(600);

/// 맡김 기록을 쓴 뒤 표지 commit까지 기다리는 한도: 넘으면 끝나지 못한 맡김으로 본다
const HANDOFF_PENDING_MS: u64 = 10 * 60 * 1000;

/// 웹 창에서 고른 것
pub(crate) struct Handoff {
    pub repo: String,
    pub slug: String,
    pub path: String,
    pub blob: String,
    pub blocks: Vec<DesignPick>,
    pub title: String,
    pub profile: Option<String>,
    pub memo: String,
    pub reserve: bool,
}

/// 맡김 기록: 그 할 일의 thread가 갈라질 기획 branch와 brief에 적을 자리
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HandoffRec {
    /// 표시 번호(`d-NNNN`, 파일 이름)
    pub task: String,
    /// 할 일의 바뀌지 않는 ID: 번호가 다시 쓰인 다른 할 일(프로젝트를 새로 만듦)에 붙지 않게 대조한다
    pub task_id: String,
    pub repo: String,
    pub slug: String,
    /// 그 기획 branch 기록의 만든 때: 같은 이름으로 새로 연 branch에 붙지 않게 대조한다
    pub branch_created_ms: u64,
    pub path: String,
    pub sections: Vec<String>,
    pub by: String,
    pub created_ms: u64,
    /// 표지 commit과 그 때. 그 뒤 origin에 올렸으면(`pushed` 또는 branch의 올린 때가 그 뒤) 구현 thread가 갈라질 수 있다
    pub commit: Option<String>,
    pub marked_ms: u64,
    pub pushed: bool,
    /// main 문서에서 맡겼다. `marked_ms`가 있으면(t-2002) 표지를 main에 적었고, 없으면(t-1894 3-3) 구현 branch 첫 commit에 적는다
    pub main: bool,
    /// 넘긴 블록의 글(main이면 맡길 때 main 판, 기획 branch면 표지를 적은 뒤). 기획 branch에서 바뀌면 그 thread에 알린다
    pub blocks: Vec<String>,
}

impl HandoffRec {
    /// 구현 thread brief에 붙이는 글(확정 계획 §4)
    /// main 문서에서 맡긴 할 일의 brief: 기획 branch가 없으니 구현 branch 첫 commit으로 표지를 적는다(q-3868 5A)
    pub(crate) fn brief_main(&self) -> String {
        let sections =
            if self.sections.is_empty() { String::new() } else { format!(" 「{}」", self.sections.join("」·「")) };
        let blocks = self.blocks.iter().map(|b| fenced(b)).collect::<Vec<_>>().join("\n\n");
        format!(
            "## 기획에서 맡긴 일\n\n기획: `{}`{sections}({}, main 문서에서 맡김). 기획 branch는 없다. \
             이 branch의 첫 commit으로 아래 블록들의 표지를 `[!계획] {}`로 적는다(`[!계획]` 바로 뒤). \
             그 뒤 구현하면서 표지를 풀고 본문을 실제 구현대로 고친다. 바뀐 결정과 실제 화면도 반영하고 다른 블록은 건드리지 않는다.\n\n{}\n",
            self.path,
            self.task,
            self.task,
            capped(&blocks)
        )
    }

    /// main에 표지를 적은 맡김(t-2002)의 brief
    pub(crate) fn brief_marked(&self) -> String {
        let sections =
            if self.sections.is_empty() { String::new() } else { format!(" 「{}」", self.sections.join("」·「")) };
        format!(
            "## 기획에서 맡긴 일\n\n기획: `{}`{sections}({}, main 문서에 표지 `[!계획] {}`가 있다). \
             이 branch에서 그 블록들의 표지를 풀고 본문을 실제 구현대로 고친다. 바뀐 결정과 실제 화면도 반영하고 다른 블록은 \
             건드리지 않는다. 그 블록이 바뀌면 host가 알린다: origin main을 합쳐 반영한다.\n",
            self.path, self.task, self.task
        )
    }

    /// `forked`: 이 worktree가 그 기획 branch에서 갈라졌다(시작 요청이 다른 기준을 주었으면 거짓)
    pub(crate) fn brief(&self, forked: bool) -> String {
        let sections =
            if self.sections.is_empty() { String::new() } else { format!(" 「{}」", self.sections.join("」·「")) };
        let fork = if forked {
            format!("이 worktree는 `origin/design/{}`에서 갈라졌다. ", self.slug)
        } else {
            format!("시작 요청이 다른 기준을 주었다: `origin/design/{}`을 합친 뒤 일한다. ", self.slug)
        };
        format!(
            "## 기획에서 맡긴 일\n\n기획: `{}`{sections}({}, 기획 branch `design/{}`). {fork}\
             이 branch에서 그 블록의 `[!계획] {}` 표지를 풀고 본문을 실제 구현대로 고친다. \
             바뀐 결정과 실제 화면도 반영하고 다른 블록은 건드리지 않는다.\n",
            self.path, self.task, self.slug, self.task
        )
    }
}

/// 할 일 표시 번호(`d-NNNN`)만 기록 파일 이름이 된다
fn task_id_ok(id: &str) -> bool {
    id.strip_prefix("d-").is_some_and(|n| !n.is_empty() && n.len() <= 19 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// 할 일 메모 줄: TASKS.md의 하위 할 일·목록 머리로 읽히는 줄은 인용으로 둔다
fn note_line(line: &str) -> String {
    let t = line.trim_start();
    if tasks::is_task(line) || t.starts_with('#') { format!("> {line}") } else { line.to_string() }
}

/// thread에 보내는 블록 글의 상한(그 thread 입력에 그대로 붙는다)
const RELAY_MAX: usize = 16 * 1024;

/// 상한까지 자른 글(글자 경계에서)
fn capped(text: &str) -> String {
    if text.len() <= RELAY_MAX {
        return text.to_string();
    }
    let mut end = RELAY_MAX;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…(뒤는 줄였다: 기획 문서에서 읽는다)", &text[..end])
}

/// 블록 안의 backtick 줄보다 긴 울타리로 감싼다
fn fenced(block: &str) -> String {
    let longest = block.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}\n{block}\n{fence}")
}

/// 넘긴 블록 중 바뀐 것(전에 알린 글에 없는 블록)만, 지금 차례대로
fn changed_blocks(before: &[String], now: &[String]) -> Vec<String> {
    now.iter().filter(|b| !before.contains(b)).cloned().collect()
}

/// 구현 thread에 보낼 고침 글: 바뀐 블록(지금 글)과, 블록 수가 줄었으면 빠진 수. 차례만 바뀌었으면 None.
/// 고친 블록의 옛 글은 「빠진 블록」이 아니다(고침은 바뀐 블록으로만 적는다)
fn relay_body(before: &[String], now: &[String]) -> Option<String> {
    let changed = changed_blocks(before, now);
    let dropped = before.len().saturating_sub(now.len());
    if changed.is_empty() && dropped == 0 {
        return None;
    }
    let mut body = String::new();
    if !changed.is_empty() {
        body.push_str(&format!("바뀐 블록:\n\n{}\n", changed.join("\n\n")));
    }
    if dropped > 0 {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(&format!("이 할 일의 블록 {dropped}개는 표지가 없어져 빠졌다(기획 문서에서 확인한다).\n"));
    }
    Some(body)
}

impl Host {
    fn design_handoff_dir(&self, project: &str) -> Result<PathBuf, ErrorBody> {
        self.project_dir(project)?;
        Ok(self.state.join("design-handoffs").join(project))
    }

    /// 그 할 일의 맡김 기록(없으면 None). thread 시작 길에서 읽기만 한다
    pub(crate) fn design_handoff_load(&self, project: &str, task: &str) -> Option<HandoffRec> {
        if !task_id_ok(task) {
            return None;
        }
        let path = self.design_handoff_dir(project).ok()?.join(format!("{task}.json"));
        let bytes = std::fs::read(&path).ok()?;
        serde_json::from_slice(&bytes)
            .inspect_err(|e| tracing::warn!(project, task, "맡김 기록을 읽지 못했다: {e}"))
            .ok()
    }

    fn design_handoff_store(&self, project: &str, rec: &HandoffRec) -> Result<(), ErrorBody> {
        if !task_id_ok(&rec.task) {
            return Err(internal(format!("할 일 번호가 아니다: {}", rec.task)));
        }
        let dir = self.design_handoff_dir(project)?;
        std::fs::create_dir_all(&dir).map_err(|e| internal(format!("{}: {e}", dir.display())))?;
        let bytes = serde_json::to_vec_pretty(rec).map_err(internal)?;
        crate::orch::write_atomic(&dir.join(format!("{}.json", rec.task)), &bytes).map_err(internal)
    }

    /// 할 일에서 thread를 시작할 때(`apply_from_task`, 상태 파일 읽기만): 맡김 기록이 그 할 일의 것이면 기획 branch 끝에서
    /// 갈라지고 brief에 맡긴 자리를 적는다. 표지 commit을 아직 올리지 못했으면 시작을 미룬다(예약은 대기에 남는다).
    /// 다른 할 일·새로 연 같은 이름 branch의 기록이면 쓰지 않는다. 시작 요청이 기준을 따로 주었으면 그것을 지킨다
    /// `short_id`·`task_id`·`repo`는 시작할 할 일의 표시 번호·바뀌지 않는 ID·저장소 칸이다
    pub(crate) fn design_handoff_start(
        &self,
        a: &mut StartArgs,
        short_id: Option<&str>,
        task_id: Option<&str>,
        repo: Option<&str>,
    ) -> Result<(), ErrorBody> {
        let Some(rec) = short_id.and_then(|id| self.design_handoff_load(&a.project, id)) else {
            return Ok(());
        };
        if task_id != Some(rec.task_id.as_str()) || repo != Some(rec.repo.as_str()) {
            tracing::info!(project = a.project, task = rec.task, "다른 할 일의 맡김 기록이라 쓰지 않는다");
            return Ok(());
        }
        if rec.commit.is_none() {
            // 기록 뒤 표지 commit 전: 맡기는 중이면 미루고, 오래 남았으면 끝나지 못한 맡김(host가 멈춤 등)이라 평소처럼 시작한다
            if aaa_common::util::now_ms().saturating_sub(rec.created_ms) < HANDOFF_PENDING_MS {
                return Err(err(ErrorCode::Unavailable, "구현 맡기는 중이다: 표지를 적은 뒤 시작한다"));
            }
            tracing::info!(project = a.project, task = rec.task, "끝나지 못한 맡김 기록이라 평소처럼 시작한다");
            return Ok(());
        }
        if rec.main {
            // main 문서에서 맡긴 것: 기준은 그대로(origin 기본 branch). 표지를 main에 적었으면(t-2002) 그 자리를, 옛 기록이면 첫
            // commit으로 표지를 적으라고 적는다
            let brief = if rec.marked_ms > 0 { rec.brief_marked() } else { rec.brief_main() };
            a.task = format!("{}\n\n{}", a.task.trim_end(), brief);
            return Ok(());
        }
        let branch = self
            .design_branch_load(&a.project, &rec.repo, &rec.slug)
            .ok()
            .flatten()
            .filter(|b| b.created_ms == rec.branch_created_ms);
        let Some(branch) = branch else {
            tracing::info!(
                project = a.project,
                task = rec.task,
                slug = rec.slug,
                "맡긴 기획 branch가 없어 평소처럼 시작한다"
            );
            return Ok(());
        };
        let forked = a.base.is_none();
        if forked {
            // 표지 commit과 그 뒤 고침까지 origin에 다 올린 판에서 갈라진다(올리기는 쉼 창 안에 이어진다)
            let pushed = rec.commit.is_some() && branch.pushed.is_some() && branch.pushed == branch.head;
            if !pushed {
                return Err(err(
                    ErrorCode::Unavailable,
                    format!(
                        "기획 branch design/{}를 아직 origin에 다 올리지 못했다(맡긴 표지·그 뒤 고침): 올린 뒤 시작한다",
                        rec.slug
                    ),
                ));
            }
            a.base = Some(format!("origin/design/{}", rec.slug));
        }
        a.task = format!("{}\n\n{}", a.task.trim_end(), rec.brief(forked));
        Ok(())
    }

    /// 버린 기획 branch(그 만든 때)에서 맡긴 기록을 지운다
    pub(crate) fn design_handoff_forget(&self, project: &str, repo: &str, slug: &str, created_ms: u64) {
        let Ok(dir) = self.design_handoff_dir(project) else { return };
        let Ok(entries) = std::fs::read_dir(&dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(rec) = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice::<HandoffRec>(&b).ok())
            else {
                continue;
            };
            if rec.repo == repo && rec.slug == slug && rec.branch_created_ms == created_ms {
                aaa_common::diagnostic::ignored(std::fs::remove_file(&path), "맡김 기록 지우기");
            }
        }
    }

    fn design_handoff_remove(&self, project: &str, task: &str) {
        if let Ok(dir) = self.design_handoff_dir(project)
            && task_id_ok(task)
        {
            aaa_common::diagnostic::ignored(std::fs::remove_file(dir.join(format!("{task}.json"))), "맡김 기록 지우기");
        }
    }

    fn design_handoff_records(&self, project: &str) -> Vec<HandoffRec> {
        let Ok(dir) = self.design_handoff_dir(project) else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };
        let mut out: Vec<HandoffRec> = entries
            .flatten()
            .filter_map(|e| std::fs::read(e.path()).ok())
            .filter_map(|b| serde_json::from_slice(&b).ok())
            .collect();
        out.sort_by_key(|a| a.created_ms);
        out
    }

    /// 그 저장소에서 맡긴 블록(main 맡김은 표지가 구현 branch에, 기획 branch 맡김은 그 branch에 있어 main 판에는 아직 없다).
    /// 살아 있는 할 일(번호와 바뀌지 않는 ID가 같은 것)의 기록만, 블록 글은 d-ID를 뺀 main 판 글로
    pub(crate) fn design_handoff_notes(&self, project: &str, repo: &str) -> Vec<aaa_proto::DesignHandoffNote> {
        // main에 표지를 적은 맡김(t-2002)은 그 표지가 칩이다: 글로 찾는 칩은 옛 기록에만 단다
        let recs: Vec<HandoffRec> = self
            .design_handoff_records(project)
            .into_iter()
            .filter(|r| r.repo == repo && !r.blocks.is_empty() && !(r.main && r.marked_ms > 0))
            .collect();
        if recs.is_empty() {
            return Vec::new();
        }
        let live: std::collections::HashMap<String, String> = self
            .task_board(project)
            .map(|b| b.tasks.into_iter().filter(|t| !t.done).filter_map(|t| Some((t.short_id?, t.id?))).collect())
            .unwrap_or_default();
        recs.into_iter()
            .filter(|r| live.get(&r.task) == Some(&r.task_id))
            .map(|r| aaa_proto::DesignHandoffNote {
                task: r.task,
                path: r.path,
                blocks: r.blocks.iter().map(|b| aaa_proto::design_unmark(b)).collect(),
                branch: if r.main { String::new() } else { r.slug },
            })
            .collect()
    }

    /// 이 문서에서 이미 맡긴 블록(d-ID를 뺀 글 → 할 일). main·기획 branch 어느 쪽에서 맡겼든 다시 맡기지 않는다
    fn design_handoff_taken(&self, project: &str, repo: &str, path: &str) -> std::collections::HashMap<String, String> {
        self.design_handoff_notes(project, repo)
            .into_iter()
            .filter(|n| n.path == path)
            .flat_map(|n| n.blocks.into_iter().map(move |b| (b, n.task.clone())))
            .collect()
    }

    /// main 저장 뒤(사람·대화 thread): 맡긴 블록의 글이 바뀌었으면 그 할 일의 구현 thread에 바뀐 글을 보낸다(확정 계획 §4).
    /// 할 일마다 한 번에 하나씩, 잠금 안에서 main의 지금 글을 다시 읽어 견준다(저장이 겹쳐도 옛 글을 나중에 보내지 않는다).
    /// 그 저장의 대화(`slug`, 비면 없음)에 「넘긴 블록 고침」 사건을 남긴다
    pub(crate) async fn design_handoff_relay(
        self: &Arc<Self>,
        project: &str,
        repo: &str,
        slug: &str,
        path: &str,
        commit: &str,
    ) {
        let tasks: Vec<String> = self
            .design_handoff_records(project)
            .into_iter()
            .filter(|r| r.main && r.marked_ms > 0 && r.repo == repo && r.path == path)
            .map(|r| r.task)
            .collect();
        for task in tasks {
            let lock = self.thread_op_lock(project, &format!("design-relay:{task}"));
            let _one = lock.lock().await;
            let Some(rec) = self.design_handoff_load(project, &task) else { continue };
            let Ok(FrontResponse::DesignDoc { doc }) = self.design_read(project, repo, path, None).await else {
                continue;
            };
            let now: Vec<String> = aaa_proto::design_marks(&doc.text)
                .iter()
                .filter(|m| m.task.as_deref() == Some(task.as_str()))
                .map(|m| aaa_proto::design_mark_text(&doc.text, m))
                .collect();
            // 표지가 모두 빠졌으면(구현됨·되돌림) 알릴 블록이 없다
            if now.is_empty() || now == rec.blocks {
                continue;
            }
            let thread = self
                .task_board(project)
                .ok()
                .and_then(|b| b.tasks.into_iter().find(|t| t.short_id.as_deref() == Some(task.as_str())))
                .and_then(|t| t.thread)
                .filter(|id| {
                    self.load(project, id).is_ok_and(|r| {
                        matches!(
                            r.status,
                            aaa_proto::ThreadStatus::Open
                                | aaa_proto::ThreadStatus::Starting
                                | aaa_proto::ThreadStatus::Parked
                        )
                    })
                });
            let Some(thread) = thread else {
                // 아직 thread가 없으면 main에서 갈라지므로 바뀐 글을 그대로 받는다
                self.design_handoff_blocks(project, &task, now);
                continue;
            };
            let Some(body) = relay_body(&rec.blocks, &now) else {
                // 차례만 바뀌었다
                self.design_handoff_blocks(project, &task, now);
                continue;
            };
            let msg = format!(
                "main 문서에서 맡긴 블록({task})이 바뀌었다(`{path}`, commit {}). 자기 branch에 origin main을 합쳐 반영한다.\n\n{}",
                &doc.commit[..doc.commit.len().min(7)],
                capped(&body)
            );
            let note = match self.design_handoff_send(project, &thread, msg).await {
                Some(aaa_proto::DeliveryState::Unknown) => format!("{task} → {thread}(받았는지 모름)"),
                Some(_) => format!("{task} → {thread}"),
                None => {
                    // 보내지 못했다(보류가 풀리지 않음·시한): 기록을 두어 다음 저장에 다시 본다. 대화에 남겨 사람이 안다
                    let note = format!("{task} → {thread}(보내지 못함)");
                    if !slug.is_empty() {
                        self.design_chat_event(project, repo, slug, "relay", &note, Some(commit.to_string())).await;
                    }
                    continue;
                }
            };
            // 받았는지 모르는 것도 다시 보내지 않는다(같은 말이 두 번 갈 수 있다)
            self.design_handoff_blocks(project, &task, now);
            if !slug.is_empty() {
                self.design_chat_event(project, repo, slug, "relay", &note, Some(commit.to_string())).await;
            }
        }
    }

    /// 넘긴 블록의 지금 글을 기록에 둔다(알린 뒤, 또는 thread가 없어 알릴 것이 없을 때). 그 할 일의 relay 잠금 안에서 부른다
    fn design_handoff_blocks(&self, project: &str, task: &str, blocks: Vec<String>) {
        if let Some(mut rec) = self.design_handoff_load(project, task) {
            rec.blocks = blocks;
            aaa_common::diagnostic::ignored(self.design_handoff_store(project, &rec), "다음 고침에 다시 알린다");
        }
    }

    /// 구현 thread에 고침을 보낸다. 뜨는 중이거나 보류된 thread, 막 떠 brief를 아직 받지 않은 thread는 열리고 brief를 받은 뒤에
    /// 보낸다(대화 말 보내기와 같이 thread 사건마다 다시 본다, 10분까지). 보냈으면 그 전달 상태, 못 보냈으면 None
    async fn design_handoff_send(
        self: &Arc<Self>,
        project: &str,
        thread: &str,
        msg: String,
    ) -> Option<aaa_proto::DeliveryState> {
        let mut events = self.thread_events.subscribe();
        let deadline = tokio::time::Instant::now() + RELAY_WAIT;
        loop {
            match self.load(project, thread) {
                Ok(r) if r.status == aaa_proto::ThreadStatus::Open && r.brief == aaa_proto::BriefState::Delivered => {
                    match self
                        .thread_message(project, thread, msg.clone(), aaa_proto::MessageMode::Queue, "design")
                        .await
                    {
                        Ok(m) => return Some(m.state),
                        // 확인창·쓰던 글 같은 막힘: 다음 thread 사건 뒤에 다시 본다
                        Err(e) => tracing::info!(project, thread, "넘긴 블록 고침을 아직 보내지 못했다: {}", e.message),
                    }
                }
                Ok(r)
                    if matches!(
                        r.status,
                        aaa_proto::ThreadStatus::Open
                            | aaa_proto::ThreadStatus::Starting
                            | aaa_proto::ThreadStatus::Parked
                    ) => {}
                _ => return None,
            }
            match tokio::time::timeout_at(deadline, events.recv()).await {
                Ok(Ok(_)) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
                Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => return None,
                Err(_) => {
                    tracing::warn!(project, thread, "넘긴 블록 고침을 {}초 안에 보내지 못했다", RELAY_WAIT.as_secs());
                    return None;
                }
            }
        }
    }

    /// 구현 맡기기(사람만, t-2002 q-4224 1A): main 문서를 읽어 고른 블록을 대조하고, 할 일·맡김 기록을 만든 뒤 고른 블록 표지에
    /// d-ID를 적어 main에 저장한다(저장소 검사 포함). 저장하지 못하면 만든 할 일과 기록을 지우고 거절한다. 예약 실패는 경고다.
    /// `h.slug`는 사건을 남길 기획 대화다(비면 없음)
    pub(crate) async fn design_handoff(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        h: Handoff,
    ) -> Result<FrontResponse, ErrorBody> {
        // 사람만 맡긴다(thread·coordinator 세션은 기획에 쓰지 않는다)
        if actor.starts_with("worker:") {
            return Err(err(ErrorCode::Forbidden, "구현 맡기기는 사람이 한다"));
        }
        self.design_writer(role, actor, project, None)?;
        let (target, mid) = self.design_target(project, &h.repo)?;
        let title = h.title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err(bad("할 일 제목이 비었다"));
        }
        tasks::check_title(&title).map_err(|e| bad(format!("할 일 제목: {e}")))?;
        let memo: Vec<String> = h.memo.lines().map(str::trim_end).filter(|l| !l.is_empty()).map(note_line).collect();
        if memo.is_empty() {
            return Err(bad("메모가 비었다(구현 thread의 할 일 글이 된다)"));
        }
        if h.blocks.is_empty() {
            return Err(bad("넘길 블록을 고르지 않았다"));
        }
        // 같은 저장소의 맡김은 하나씩: 동시에 같은 블록을 두 번 맡기지 않게
        let lock = self.thread_op_lock(project, &format!("design-main:{}", target.name));
        let _one = lock.lock().await;
        // 고를 때 읽은 판이고, 고른 차례의 블록 첫 줄이 같아야 한다. 넘기지 않은 계획인지도 할 일을 만들기 전에 본다
        let FrontResponse::DesignDoc { doc } = self.design_read(project, &h.repo, &h.path, None).await? else {
            return Err(internal("예상하지 못한 응답"));
        };
        if doc.blob != h.blob {
            return Err(err(ErrorCode::Conflict, "그 사이 main 문서가 바뀌었다: 다시 읽고 고른다"));
        }
        let marks = aaa_proto::design_marks(&doc.text);
        let lines: Vec<&str> = doc.text.split('\n').collect();
        for pick in &h.blocks {
            let head =
                marks.get(pick.ordinal as usize).and_then(|m| lines.get(m.line)).map(|l| aaa_proto::design_trim(l));
            if head != Some(aaa_proto::design_trim(&pick.head)) {
                return Err(err(ErrorCode::Conflict, "고른 블록이 문서와 맞지 않는다: 다시 읽고 고른다"));
            }
        }
        let picks: Vec<usize> = h.blocks.iter().map(|p| p.ordinal as usize).collect();
        aaa_proto::design_mark_tasks(&doc.text, &picks, "d-0000").map_err(bad)?;
        // 옛 기록(글로 찾는 맡김)에 이미 있는 같은 블록은 다시 맡기지 않는다
        let taken = self.design_handoff_taken(project, &target.name, &doc.path);
        let mut sections: Vec<String> = Vec::new();
        for &p in &picks {
            let Some(m) = marks.get(p) else { continue };
            if let Some(task) = taken.get(&aaa_proto::design_mark_text(&doc.text, m)) {
                return Err(err(ErrorCode::Conflict, format!("이미 맡긴 블록이다: {task}")));
            }
            // 첫 제목 앞의 블록은 문서 제목을 절로 쓴다(웹 `sectionsOf`와 같다)
            let section = if m.section.is_empty() { doc.title.clone() } else { m.section.clone() };
            if !section.is_empty() && !sections.contains(&section) {
                sections.push(section);
            }
        }
        // 머신은 그 문서 저장소의 머신이다(자동 배정 머신이 없어도 예약이 자리를 얻는다)
        let machine = Some(self.machine_name(&mid));
        let task =
            self.design_handoff_task(project, actor, &target.name, &title, h.profile.as_deref(), machine, memo).await?;
        let id = task.short_id.clone().ok_or_else(|| internal("새 할 일에 표시 번호가 없다"))?;
        let mut rec = HandoffRec {
            task: id.clone(),
            task_id: task.id.clone().unwrap_or_default(),
            repo: target.name.clone(),
            path: doc.path.clone(),
            sections,
            by: actor.to_string(),
            created_ms: aaa_common::util::now_ms(),
            main: true,
            ..Default::default()
        };
        // 기록을 표지 저장보다 먼저 둔다: 그 사이 누가 시작해도 맡긴 자리를 안다(표지를 적기 전에는 미룬다)
        let stored = self.design_handoff_store(project, &rec);
        let marked = aaa_proto::design_mark_tasks(&doc.text, &picks, &id).map_err(internal)?;
        let saved = match &stored {
            Ok(()) => {
                Box::pin(self.design_save_event(
                    role,
                    actor,
                    project,
                    &h.repo,
                    &h.slug,
                    (&doc.path, Some(h.blob.clone()), marked.clone()),
                    Vec::new(),
                    Some(format!("구현 맡김 {id}")),
                    Some(("handoff", format!("{id} · 블록 {}", picks.len()))),
                ))
                .await
            }
            Err(e) => Err(e.clone()),
        };
        let commit = match saved {
            Ok(FrontResponse::DesignSaved { commit, .. }) => commit,
            other => {
                // 표지 없는 할 일은 맡긴 것이 아니다: 지우고 거절한다(다시 맡길 수 있게)
                let why = match other {
                    Err(e) => e.message,
                    Ok(_) => "예상하지 못한 응답".into(),
                };
                self.design_handoff_remove(project, &id);
                let left = match self.design_handoff_task_remove(project, actor, &task).await {
                    Ok(()) => String::new(),
                    Err(e) => format!(" 만든 할 일 {id}도 지우지 못했다: {}", e.message),
                };
                return Err(err(ErrorCode::Conflict, format!("표지를 main에 적지 못해 맡기지 않았다: {why}.{left}")));
            }
        };
        rec.commit = Some(commit.clone());
        rec.marked_ms = aaa_common::util::now_ms();
        rec.pushed = true;
        let after = aaa_proto::design_marks(&marked);
        rec.blocks = after
            .iter()
            .filter(|m| m.task.as_deref() == Some(id.as_str()))
            .map(|m| aaa_proto::design_mark_text(&marked, m))
            .collect();
        let mut warnings: Vec<String> = Vec::new();
        if let Err(e) = self.design_handoff_store(project, &rec) {
            warnings.push(format!("맡김 기록을 쓰지 못했다({}): 구현 thread brief에 맡긴 자리가 빠진다", e.message));
        }
        let mut reserved = false;
        if h.reserve {
            match self.design_handoff_reserve(project, actor, &task).await {
                Ok(()) => reserved = true,
                Err(e) => warnings.push(format!("예약하지 못했다: {}", e.message)),
            }
        }
        let warning = (!warnings.is_empty()).then(|| warnings.join(" · "));
        self.db.audit(
            actor,
            "design_handoff",
            &format!("{project} {} main {} {id} 블록 {} 예약 {reserved}", target.name, doc.path, picks.len()),
        );
        Ok(FrontResponse::DesignHandoff {
            handoff: DesignHandoffView { task: id, commit: Some(commit), pushed: true, reserved, warning },
        })
    }

    /// 표지를 적지 못한 맡김의 할 일을 지운다(기록에 남아 되살릴 수 있다)
    async fn design_handoff_task_remove(&self, project: &str, actor: &str, task: &TaskView) -> Result<(), ErrorBody> {
        let _queue_guard = self.queue_gate.lock().await;
        let op = TaskOp::Delete {
            at: aaa_proto::TaskAnchor {
                list: task.list.clone(),
                title: task.title.clone(),
                block_sha256: task.block_sha256.clone(),
                line: task.line,
            },
        };
        let actor = self.task_writer(project, actor)?;
        let owner_ok = |_: &str, _: bool| Ok(());
        let repo_ok = |r: &str| self.task_repo_ok(project, r);
        let removed = |_: i64| Err("지우기에는 복원을 쓰지 않는다".into());
        let profiles = self.known_profiles(project);
        let ctx = task_edit::Ctx { owner_ok: &owner_ok, profiles: &profiles, repo_ok: &repo_ok, removed: &removed };
        let dir = self.project_dir(project)?;
        let (conflict, _) = self.task_write(project, &dir, &[op], None, &actor, &ctx, false)?;
        match conflict {
            Some((_, reason, _)) => Err(err(ErrorCode::Conflict, reason)),
            None => Ok(()),
        }
    }

    /// 맡김 할 일을 Backlog 끝에 더한다(저장소 칸은 그 문서의 저장소 이름이라 worktree thread가 되고, 머신 칸은 그 저장소 머신)
    #[allow(clippy::too_many_arguments)]
    async fn design_handoff_task(
        &self,
        project: &str,
        actor: &str,
        repo: &str,
        title: &str,
        profile: Option<&str>,
        machine: Option<String>,
        notes: Vec<String>,
    ) -> Result<aaa_proto::TaskView, ErrorBody> {
        let _queue_guard = self.queue_gate.lock().await;
        let board = self.task_board(project)?;
        if board.tasks.iter().any(|t| t.title.to_lowercase() == title.to_lowercase()) {
            return Err(err(ErrorCode::Conflict, format!("같은 제목의 할 일이 있다: {title}")));
        }
        let repo = self.task_repo_ok(project, repo).map_err(bad)?;
        let profile = profile.map(str::trim).filter(|p| !p.is_empty()).map(str::to_string);
        if let Some(p) = &profile
            && !self.known_profiles(project).contains(p)
        {
            return Err(bad(format!("모르는 모델(profile): {p}")));
        }
        let op = TaskOp::Add {
            list: "Backlog".into(),
            title: title.to_string(),
            owner: None,
            person: false,
            profile,
            machine,
            repo: Some(repo),
            notes,
            parent: None,
            before: None,
            thread: None,
        };
        let actor = self.task_writer(project, actor)?;
        let owner_ok = |_: &str, _: bool| Ok(());
        let repo_ok = |r: &str| self.task_repo_ok(project, r);
        let removed = |_: i64| Err("새 할 일 생성에는 복원을 쓰지 않는다".into());
        let profiles = self.known_profiles(project);
        let ctx = task_edit::Ctx { owner_ok: &owner_ok, profiles: &profiles, repo_ok: &repo_ok, removed: &removed };
        let dir = self.project_dir(project)?;
        let (conflict, _) = self.task_write(project, &dir, &[op], None, &actor, &ctx, false)?;
        if let Some((_, reason, _)) = conflict {
            return Err(err(ErrorCode::Conflict, reason));
        }
        tasks::find(&self.task_board(project)?.tasks, title).cloned().ok_or_else(|| internal("새 할 일을 읽지 못했다"))
    }

    /// 예약(할 일 화면의 「예약」과 같다): 그 블록과 모델을 실행 승인하고 자리가 나면 host가 띄운다
    async fn design_handoff_reserve(
        self: &Arc<Self>,
        project: &str,
        actor: &str,
        task: &aaa_proto::TaskView,
    ) -> Result<(), ErrorBody> {
        let profile = task.profile.clone().unwrap_or_else(|| self.default_thread_profile(project));
        let (_, _, profile_revision) = crate::task_queue::model_selection(self, &profile)?;
        self.task_queue(
            project,
            aaa_proto::QueueOp::Reserve {
                account: None,
                task: task.id.clone().ok_or_else(|| err(ErrorCode::Conflict, "할 일 ID가 없다"))?,
                block: task.block_sha256.clone(),
                profile,
                profile_revision,
                after: None,
                after_all: Some(vec![]),
            },
            actor,
        )
        .await
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brief_names_the_branch_doc_sections_and_marker() {
        let rec = HandoffRec {
            task: "d-0005".into(),
            repo: "pt".into(),
            slug: "loop-plan".into(),
            path: "docs/game-loop/README.md".into(),
            sections: vec!["탑".into(), "나가기".into()],
            ..Default::default()
        };
        let brief = rec.brief(true);
        assert!(
            brief
                .contains("기획: `docs/game-loop/README.md` 「탑」·「나가기」(d-0005, 기획 branch `design/loop-plan`)"),
            "{brief}"
        );
        assert!(brief.contains("`origin/design/loop-plan`에서 갈라졌다"));
        assert!(brief.contains("`[!계획] d-0005` 표지를 풀고"));
        assert!(rec.brief(false).contains("시작 요청이 다른 기준을 주었다: `origin/design/loop-plan`을 합친 뒤"));
    }

    /// 시작: 그 할 일의 기록이고 표지를 올린 뒤에만 기획 branch에서 갈라진다. 기준을 준 시작은 그 기준을 지킨다.
    /// 다른 할 일(번호가 다시 쓰임)·저장소를 바꾼 할 일·같은 이름으로 새로 연 branch에는 쓰지 않고, 버리면 기록이 지워진다
    #[tokio::test]
    async fn start_forks_from_the_design_branch_only_for_its_task_after_the_push() {
        let state = std::env::temp_dir().join(format!("aaa-design-handoff-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Arc::new(Host::open(cfg, state.clone()).unwrap());
        h.project_create("demo", Default::default(), "test").unwrap();
        let mut branch = crate::design_branch::Rec {
            repo: "pt".into(),
            slug: "loop-plan".into(),
            created_ms: 100,
            ..Default::default()
        };
        h.design_branch_store("demo", &branch).unwrap();
        let rec = HandoffRec {
            task: "d-0005".into(),
            task_id: "01TASK".into(),
            repo: "pt".into(),
            slug: "loop-plan".into(),
            branch_created_ms: 100,
            path: "docs/a.md".into(),
            commit: Some("c1".into()),
            marked_ms: 200,
            ..Default::default()
        };
        h.design_handoff_store("demo", &rec).unwrap();
        let args = || StartArgs { project: "demo".into(), task: "할 일".into(), ..Default::default() };
        let start =
            |a: &mut StartArgs, id: &str, repo: &str| h.design_handoff_start(a, Some("d-0005"), Some(id), Some(repo));
        let mut a = args();
        assert_eq!(start(&mut a, "01TASK", "pt").unwrap_err().code, ErrorCode::Unavailable, "올리기 전에는 미룬다");
        branch.head = Some("c1".into());
        branch.pushed = Some("c0".into());
        h.design_branch_store("demo", &branch).unwrap();
        assert!(start(&mut args(), "01TASK", "pt").is_err(), "표지 commit(c1) 전의 판(c0)만 올렸다");
        branch.pushed = Some("c1".into());
        h.design_branch_store("demo", &branch).unwrap();
        let mut a = args();
        start(&mut a, "01TASK", "pt").unwrap();
        assert_eq!(a.base.as_deref(), Some("origin/design/loop-plan"));
        assert!(a.task.starts_with("할 일\n\n## 기획에서 맡긴 일") && a.task.contains("갈라졌다"), "{}", a.task);
        let mut a = args();
        a.base = Some("origin/main".into());
        start(&mut a, "01TASK", "pt").unwrap();
        assert_eq!(a.base.as_deref(), Some("origin/main"));
        assert!(a.task.contains("시작 요청이 다른 기준을 주었다"));
        for (id, repo) in [("01OTHER", "pt"), ("01TASK", "other")] {
            let mut a = args();
            start(&mut a, id, repo).unwrap();
            assert_eq!((a.base.as_deref(), a.task.as_str()), (None, "할 일"), "{id} {repo}");
        }
        branch.created_ms = 999;
        h.design_branch_store("demo", &branch).unwrap();
        let mut a = args();
        start(&mut a, "01TASK", "pt").unwrap();
        assert_eq!((a.base.as_deref(), a.task.as_str()), (None, "할 일"), "같은 이름으로 새로 연 branch");
        h.design_handoff_forget("demo", "pt", "loop-plan", 999);
        assert!(h.design_handoff_load("demo", "d-0005").is_some(), "다른 판의 branch는 지우지 않는다");
        h.design_handoff_forget("demo", "pt", "loop-plan", 100);
        assert!(h.design_handoff_load("demo", "d-0005").is_none());
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&state), "시험 폴더 정리");
    }

    /// 맡기는 중(표지 commit 전)인 기록은 시작을 미루고, 오래 남은 것은 끝나지 못한 맡김이라 평소처럼 시작한다
    #[tokio::test]
    async fn pending_record_defers_then_expires() {
        let state = std::env::temp_dir().join(format!("aaa-design-handoff-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Arc::new(Host::open(cfg, state.clone()).unwrap());
        h.project_create("demo", Default::default(), "test").unwrap();
        let branch = crate::design_branch::Rec {
            repo: "pt".into(),
            slug: "loop-plan".into(),
            created_ms: 100,
            ..Default::default()
        };
        h.design_branch_store("demo", &branch).unwrap();
        let mut rec = HandoffRec {
            task: "d-0007".into(),
            task_id: "01TASK".into(),
            repo: "pt".into(),
            slug: "loop-plan".into(),
            branch_created_ms: 100,
            created_ms: aaa_common::util::now_ms(),
            ..Default::default()
        };
        h.design_handoff_store("demo", &rec).unwrap();
        let mut a = StartArgs { project: "demo".into(), task: "할 일".into(), ..Default::default() };
        let start = |a: &mut StartArgs| h.design_handoff_start(a, Some("d-0007"), Some("01TASK"), Some("pt"));
        assert_eq!(start(&mut a).unwrap_err().code, ErrorCode::Unavailable);
        rec.created_ms = aaa_common::util::now_ms() - HANDOFF_PENDING_MS - 1;
        h.design_handoff_store("demo", &rec).unwrap();
        start(&mut a).unwrap();
        assert_eq!((a.base.as_deref(), a.task.as_str()), (None, "할 일"));
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&state), "시험 폴더 정리");
    }

    /// 맡김 할 일: 저장소 칸과 메모로 더하고, 같은 제목은 거절하고, 표지를 적지 못하면 지운다(TASKS.md에서 지우는 유일한 새 길)
    #[tokio::test]
    async fn handoff_task_is_added_and_removed() {
        let state = std::env::temp_dir().join(format!("aaa-design-handoff-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Arc::new(Host::open(cfg, state.clone()).unwrap());
        h.project_create("demo", Default::default(), "test").unwrap();
        std::fs::write(
            h.project_dir("demo").unwrap().join("project.toml"),
            "schema = 1\n\n[[repos]]\npath = '/work/pt'\n",
        )
        .unwrap();
        let notes =
            vec!["[기획 pt docs/a.md 「탑」](design/pt/docs/a.md#탑)".to_string(), "- 100층까지 늘린다.".into()];
        let t = h.design_handoff_task("demo", "test", "pt", "탑 — 100층", None, None, notes.clone()).await.unwrap();
        assert!(t.short_id.as_deref().is_some_and(|id| id.starts_with("d-")));
        assert_eq!((t.repo.as_deref(), t.notes.clone()), (Some("pt"), notes.clone()));
        let again = h.design_handoff_task("demo", "test", "pt", "탑 — 100층", None, None, notes).await.unwrap_err();
        assert_eq!(again.code, ErrorCode::Conflict, "같은 제목");
        h.design_handoff_task_remove("demo", "test", &t).await.unwrap();
        assert!(tasks::find(&h.task_board("demo").unwrap().tasks, "탑 — 100층").is_none());
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&state), "시험 폴더 정리");
    }

    /// main 문서에서 맡김(3-3): 살아 있는 할 일(번호와 ID가 같은 것)의 기록만 웹에 싣고, 시작은 기준을 그대로 두고 첫 commit으로
    /// 표지를 적으라고 한다. 기획 branch 맡김도 d-ID를 뺀 글과 그 주제로 싣는다
    #[tokio::test]
    async fn handoff_notes_and_main_start() {
        let state = std::env::temp_dir().join(format!("aaa-design-handoff-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Arc::new(Host::open(cfg, state.clone()).unwrap());
        h.project_create("demo", Default::default(), "test").unwrap();
        std::fs::write(
            h.project_dir("demo").unwrap().join("project.toml"),
            "schema = 1\n\n[[repos]]\npath = '/work/pt'\n",
        )
        .unwrap();
        let t = h.design_handoff_task("demo", "test", "pt", "난이도", None, None, vec!["메모".into()]).await.unwrap();
        let id = t.short_id.clone().unwrap();
        let block = "> [!계획]\n> **난이도** — 층마다 오른다.".to_string();
        let main = HandoffRec {
            task: id.clone(),
            task_id: t.id.clone().unwrap(),
            repo: "pt".into(),
            path: "docs/a.md".into(),
            main: true,
            blocks: vec![block.clone()],
            created_ms: 1,
            // t-2002 전 main 맡김: 맡길 때의 main 판, 표지 없음
            commit: Some("c0".into()),
            ..Default::default()
        };
        h.design_handoff_store("demo", &main).unwrap();
        let notes = h.design_handoff_notes("demo", "pt");
        assert_eq!(notes.len(), 1);
        assert_eq!(
            (notes[0].task.as_str(), notes[0].path.as_str(), notes[0].blocks.clone()),
            (id.as_str(), "docs/a.md", vec![block.clone()])
        );
        assert!(h.design_handoff_notes("demo", "other").is_empty(), "다른 저장소");
        assert_eq!(notes[0].branch, "", "main 맡김");
        let other = HandoffRec { task_id: "01OTHER".into(), ..main.clone() };
        h.design_handoff_store("demo", &other).unwrap();
        assert!(h.design_handoff_notes("demo", "pt").is_empty(), "번호가 다시 쓰인 다른 할 일의 기록");
        h.design_handoff_store("demo", &main).unwrap();
        let mut a = StartArgs { project: "demo".into(), task: "할 일".into(), ..Default::default() };
        h.design_handoff_start(&mut a, Some(&id), t.id.as_deref(), Some("pt")).unwrap();
        assert_eq!(a.base, None, "main에서 갈라진다(기본 기준)");
        assert!(
            a.task.contains("main 문서에서 맡김")
                && a.task.contains(&format!("`[!계획] {id}`"))
                && a.task.contains(&block),
            "{}",
            a.task
        );

        // main에 표지를 적은 맡김(t-2002): 표지가 칩이라 글로 찾는 칩에 싣지 않고, brief는 표지 자리를 알린다(첫 commit 안내 없음)
        let marked = HandoffRec { marked_ms: 2, commit: Some("c1".into()), ..main.clone() };
        h.design_handoff_store("demo", &marked).unwrap();
        assert!(h.design_handoff_notes("demo", "pt").is_empty());
        let mut a = StartArgs { project: "demo".into(), task: "할 일".into(), ..Default::default() };
        h.design_handoff_start(&mut a, Some(&id), t.id.as_deref(), Some("pt")).unwrap();
        assert_eq!(a.base, None);
        assert!(a.task.contains("main 문서에 표지") && !a.task.contains("첫 commit"), "{}", a.task);
        // 표지를 적기 전(기록만)에는 시작을 미룬다
        let pending = HandoffRec { commit: None, marked_ms: 0, created_ms: aaa_common::util::now_ms(), ..main.clone() };
        h.design_handoff_store("demo", &pending).unwrap();
        let mut a = StartArgs { project: "demo".into(), task: "할 일".into(), ..Default::default() };
        let e = h.design_handoff_start(&mut a, Some(&id), t.id.as_deref(), Some("pt")).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable, "{e:?}");

        // 기획 branch 맡김: 표지의 d-ID를 뺀 글로 싣는다(main 판과 같은 글)
        let on_branch = HandoffRec {
            main: false,
            slug: "loop-plan".into(),
            blocks: vec![format!("> [!계획] {id}\n> **난이도** — 층마다 오른다.")],
            ..main.clone()
        };
        h.design_handoff_store("demo", &on_branch).unwrap();
        let notes = h.design_handoff_notes("demo", "pt");
        assert_eq!((notes[0].branch.as_str(), notes[0].blocks.clone()), ("loop-plan", vec![block.clone()]));
        assert_eq!(
            h.design_handoff_taken("demo", "pt", "docs/a.md").get(&block),
            Some(&id),
            "같은 문서에서는 다시 맡기지 않는다"
        );
        assert!(h.design_handoff_taken("demo", "pt", "docs/other.md").is_empty(), "다른 문서의 같은 글은 막지 않는다");
        // 할 일이 사라지면 main 판에 싣지 않는다
        h.design_handoff_task_remove("demo", "test", &t).await.unwrap();
        assert!(h.design_handoff_notes("demo", "pt").is_empty());
        aaa_common::diagnostic::ignored(std::fs::remove_dir_all(&state), "시험 폴더 정리");
    }

    #[test]
    fn fences_and_caps_keep_blocks_whole() {
        assert_eq!(fenced("> 글"), "```\n> 글\n```");
        assert!(fenced("> ```코드```").starts_with("````\n"), "안의 backtick보다 긴 울타리");
        let long = "가".repeat(RELAY_MAX);
        let cut = capped(&long);
        assert!(cut.len() < long.len() + 80 && cut.ends_with("기획 문서에서 읽는다)"));
    }

    /// 고침 글: 고친 블록은 바뀐 블록으로만, 빠짐은 줄어든 수로만, 차례만 바뀌면 보내지 않는다
    #[test]
    fn relay_body_names_edits_and_drops_without_listing_old_text() {
        let (a, b, b2, c) = (
            "> [!계획] d-1\n> A".to_string(),
            "> [!계획] d-1\n> B".to_string(),
            "> [!계획] d-1\n> B2".to_string(),
            "> [!계획] d-1\n> C".to_string(),
        );
        let edit = relay_body(&[a.clone(), b.clone()], &[a.clone(), b2.clone()]).unwrap();
        assert!(
            edit.contains(&b2) && !edit.contains("> B\n") && !edit.ends_with("> B") && !edit.contains("빠졌다"),
            "{edit}"
        );
        let drop = relay_body(&[a.clone(), b.clone()], std::slice::from_ref(&a)).unwrap();
        assert!(drop.contains("블록 1개는 표지가 없어져 빠졌다") && !drop.contains("바뀐 블록"), "{drop}");
        assert_eq!(relay_body(&[a.clone(), b.clone()], &[b.clone(), a.clone()]), None, "차례만 바뀜");
        let both = relay_body(&[a.clone(), b.clone(), c.clone()], &[a.clone(), b2.clone()]).unwrap();
        assert!(both.contains(&b2) && both.contains("블록 1개는"), "{both}");
    }

    #[test]
    fn only_changed_blocks_are_relayed() {
        let before = vec!["> [!계획] d-0006\n> 100층".to_string(), "> [!계획] d-0006\n> 층 이벤트".to_string()];
        let now =
            vec!["> [!계획] d-0006\n> 100층".to_string(), "> [!계획] d-0006\n> 층 이벤트(보스 층 제외)".to_string()];
        assert_eq!(changed_blocks(&before, &now), vec![now[1].clone()]);
        assert!(changed_blocks(&now, &now).is_empty());
    }

    #[test]
    fn record_names_and_note_lines() {
        assert!(task_id_ok("d-0005") && task_id_ok("d-12345"));
        for bad in ["", "d-", "d-12a", "../d-1", "t-0001", "d-0001/x"] {
            assert!(!task_id_ok(bad), "{bad}");
        }
        assert_eq!(note_line("- 100층까지 늘린다."), "- 100층까지 늘린다.");
        assert_eq!(note_line("- [ ] 하위로 읽힐 줄"), "> - [ ] 하위로 읽힐 줄");
        assert_eq!(note_line("## 목록 머리"), "> ## 목록 머리");
        assert_eq!(note_line("- [-] 하위로 읽힐 다른 꼴"), "> - [-] 하위로 읽힐 다른 꼴");
    }
}
