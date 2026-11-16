//! 구현 맡기기(t-1894 3-2, 확정 계획 §4): 기획 branch 판 문서의 계획 블록 여럿을 할 일 하나로 넘긴다.
//! 할 일을 만들고, 고른 블록 표지에 그 d-ID를 적어 그 기획 branch에 commit해 바로 올리고, 고르면 예약한다.
//! 그 할 일의 thread는 어느 길로 시작하든(`apply_from_task`) `origin/design/<주제>`에서 갈라진 worktree에서 일한다.
//! 맡김 기록은 host 상태 폴더 `design-handoffs/<프로젝트>/<d-ID>.json`에 둔다(TASKS.md에 쓰면 예약한 블록이 바뀐다).
use std::path::PathBuf;
use std::sync::Arc;

use aaa_proto::{
    DesignBranchState, DesignHandoffView, DesignPick, DeviceRole, ErrorBody, ErrorCode, FrontResponse, TaskOp, TaskView,
};
use serde::{Deserialize, Serialize};

use crate::orch::{StartArgs, bad, internal};
use crate::{Host, err, task_edit, tasks};

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
}

impl HandoffRec {
    /// 구현 thread brief에 붙이는 글(확정 계획 §4)
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
            let pushed =
                rec.commit.is_some() && (rec.pushed || (branch.pushed.is_some() && branch.pushed_ms >= rec.marked_ms));
            if !pushed {
                return Err(err(
                    ErrorCode::Unavailable,
                    format!("맡긴 표지를 아직 origin의 design/{}에 올리지 못했다: 올린 뒤 시작한다", rec.slug),
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

    /// 구현 맡기기. 그 기획 branch를 잠근 채 읽기·고른 블록 대조·할 일·기록·표지 commit까지 한다(그 사이 대화 thread의
    /// 저장이 끼어 다른 판·다른 블록에 적지 않게). 표지 commit에 실패하면 만든 할 일과 기록을 지우고 거절한다.
    /// 올리기·예약 실패는 거절하지 않고 경고로 돌려준다(올리기 전에는 그 할 일을 시작하지 않는다)
    pub(crate) async fn design_handoff(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        h: Handoff,
    ) -> Result<FrontResponse, ErrorBody> {
        // 사람만 맡긴다(thread·coordinator 세션은 기획 branch에 쓰지 않는다)
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
        let lock = self.design_branch_lock(project, &target.name, &h.slug);
        let one = lock.lock().await;
        let branch = self
            .design_branch_load(project, &target.name, &h.slug)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{}", h.slug)))?;
        if branch.state == DesignBranchState::Rest {
            return Err(err(ErrorCode::Conflict, format!("쉬는 기획 branch다: design/{}(다시 연 뒤 맡긴다)", h.slug)));
        }
        // 고를 때 읽은 판이고, 고른 차례의 블록 첫 줄이 같아야 한다. 넘기지 않은 계획인지도 할 일을 만들기 전에 본다
        let FrontResponse::DesignDoc { doc } = self.design_read(project, &h.repo, &h.path, Some(&h.slug)).await? else {
            return Err(internal("예상하지 못한 응답"));
        };
        if doc.blob != h.blob {
            return Err(err(ErrorCode::Conflict, "그 사이 문서가 바뀌었다: 다시 읽고 고른다"));
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
        let mut sections: Vec<String> = Vec::new();
        for &p in &picks {
            let Some(m) = marks.get(p) else { continue };
            // 첫 제목 앞의 블록은 문서 제목을 절로 쓴다(웹 `sectionsOf`와 같다)
            let section = if m.section.is_empty() { doc.title.clone() } else { m.section.clone() };
            if !section.is_empty() && !sections.contains(&section) {
                sections.push(section);
            }
        }
        // 머신은 그 문서 저장소의 머신이다(기획 branch가 거기 있고, 자동 배정 머신이 없어도 예약이 자리를 얻는다)
        let machine = Some(self.machine_name(&mid));
        let task =
            self.design_handoff_task(project, actor, &target.name, &title, h.profile.as_deref(), machine, memo).await?;
        let id = task.short_id.clone().ok_or_else(|| internal("새 할 일에 표시 번호가 없다"))?;
        let mut rec = HandoffRec {
            task: id.clone(),
            task_id: task.id.clone().unwrap_or_default(),
            repo: target.name.clone(),
            slug: h.slug.clone(),
            branch_created_ms: branch.created_ms,
            path: doc.path.clone(),
            sections,
            by: actor.to_string(),
            created_ms: aaa_common::util::now_ms(),
            ..Default::default()
        };
        // 기록을 표지 commit보다 먼저 둔다: 그 사이 누가 시작해도 기획 branch 밖에서 시작하지 않는다(올리기 전이라 미룬다)
        let stored = self.design_handoff_store(project, &rec);
        let marked = aaa_proto::design_mark_tasks(&doc.text, &picks, &id).map_err(internal)?;
        let saved = match &stored {
            Ok(()) => {
                self.design_save_event(
                    role,
                    actor,
                    project,
                    &h.repo,
                    &h.slug,
                    &doc.path,
                    Some(h.blob.clone()),
                    marked,
                    Some(format!("구현 맡김 {id}")),
                    Some(("handoff", format!("{id} · 블록 {}", picks.len()))),
                    true,
                )
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
                drop(one);
                self.design_handoff_remove(project, &id);
                let left = match self.design_handoff_task_remove(project, actor, &task).await {
                    Ok(()) => String::new(),
                    Err(e) => format!(" 만든 할 일 {id}도 지우지 못했다: {}", e.message),
                };
                return Err(err(ErrorCode::Conflict, format!("표지를 적지 못해 맡기지 않았다: {why}.{left}")));
            }
        };
        rec.commit = Some(commit.clone());
        rec.marked_ms = aaa_common::util::now_ms();
        let mut warnings: Vec<String> = Vec::new();
        if let Err(e) = self.design_handoff_store(project, &rec) {
            warnings.push(format!(
                "맡김 기록을 쓰지 못했다({}): 구현 thread는 기획 branch를 합친 뒤 일해야 한다",
                e.message
            ));
        }
        drop(one);
        // 쉼을 기다리지 않고 바로 올린다: 구현 thread는 origin의 그 branch에서 갈라진다
        self.design_push_now(project, &target.name, &h.slug).await;
        let after = self.design_branch_load(project, &target.name, &h.slug).ok().flatten();
        // 그 사이 새 저장이 있어 더 앞선 끝을 올렸어도 표지 commit은 그 안에 있다
        let pushed = after.as_ref().is_some_and(|r| r.pushed.is_some() && r.pushed == r.head);
        if pushed {
            rec.pushed = true;
            aaa_common::diagnostic::ignored(self.design_handoff_store(project, &rec), "올린 때로도 알 수 있다");
        } else {
            let why = after.and_then(|r| r.push_error).unwrap_or_else(|| "그 사이 새 저장이 있었다".into());
            warnings.push(format!("origin에 올리지 못했다({why}): 조금 뒤 다시 올리고, 올린 뒤 시작한다"));
        }
        let mut reserved = false;
        if h.reserve && pushed {
            match self.design_handoff_reserve(project, actor, &task).await {
                Ok(()) => reserved = true,
                Err(e) => warnings.push(format!("예약하지 못했다: {}", e.message)),
            }
        } else if h.reserve {
            warnings.push("올린 뒤 할 일 화면에서 예약한다".into());
        }
        let warning = (!warnings.is_empty()).then(|| warnings.join(" · "));
        self.db.audit(
            actor,
            "design_handoff",
            &format!("{project} {}/{} {} {id} 블록 {} 예약 {reserved}", target.name, h.slug, doc.path, picks.len()),
        );
        Ok(FrontResponse::DesignHandoff {
            handoff: DesignHandoffView { task: id, commit: Some(commit), pushed, reserved, warning },
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
        branch.pushed = Some("c1".into());
        branch.pushed_ms = 150;
        h.design_branch_store("demo", &branch).unwrap();
        assert!(start(&mut args(), "01TASK", "pt").is_err(), "표지 commit 전에 올린 판이다");
        branch.pushed_ms = 300;
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
