//! 구현 맡기기(t-1894 3-2, 확정 계획 §4): 기획 branch 판 문서의 계획 블록 여럿을 할 일 하나로 넘긴다.
//! 할 일을 만들고, 고른 블록 표지에 그 d-ID를 적어 그 기획 branch에 commit해 바로 올리고, 고르면 예약한다.
//! 그 할 일의 thread는 어느 길로 시작하든(`apply_from_task`) `origin/design/<주제>`에서 갈라진 worktree에서 일한다.
//! 맡김 기록은 host 상태 폴더 `design-handoffs/<프로젝트>/<d-ID>.json`에 둔다(TASKS.md에 쓰면 예약한 블록이 바뀐다).
use std::path::PathBuf;
use std::sync::Arc;

use aaa_proto::{DesignHandoffView, DeviceRole, ErrorBody, ErrorCode, FrontResponse, TaskOp};
use serde::{Deserialize, Serialize};

use crate::orch::{StartArgs, bad, internal};
use crate::{Host, err, task_edit, tasks};

/// 웹 창에서 고른 것
pub(crate) struct Handoff {
    pub repo: String,
    pub slug: String,
    pub path: String,
    pub blob: String,
    pub blocks: Vec<u32>,
    pub title: String,
    pub profile: Option<String>,
    pub memo: String,
    pub reserve: bool,
}

/// 맡김 기록: 그 할 일의 thread가 갈라질 기획 branch와 brief에 적을 자리
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct HandoffRec {
    pub task: String,
    pub repo: String,
    pub slug: String,
    pub path: String,
    pub sections: Vec<String>,
    pub by: String,
    pub created_ms: u64,
}

impl HandoffRec {
    /// 구현 thread brief에 붙이는 글(확정 계획 §4)
    pub(crate) fn brief(&self) -> String {
        let sections =
            if self.sections.is_empty() { String::new() } else { format!(" 「{}」", self.sections.join("」·「")) };
        format!(
            "## 기획에서 맡긴 일\n\n기획: `{}`{sections}({}, 기획 branch `design/{}`). 이 worktree는 `origin/design/{}`에서 갈라졌다. \
             이 branch에서 그 블록의 `[!계획] {}` 표지를 풀고 본문을 실제 구현대로 고친다. \
             바뀐 결정과 실제 화면도 반영하고 다른 블록은 건드리지 않는다.\n",
            self.path, self.task, self.slug, self.slug, self.task
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
    let task_like = ["- [ ]", "- [x]", "- [X]", "* [ ]", "* [x]", "+ [ ]", "+ [x]"].iter().any(|p| t.starts_with(p));
    if task_like || t.starts_with('#') { format!("> {line}") } else { line.to_string() }
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

    /// 할 일에서 thread를 시작할 때(`apply_from_task`): 맡김 기록이 있으면 기획 branch 끝에서 갈라지고 brief에 맡긴 자리를 적는다.
    /// 시작 요청이 기준을 따로 주었으면 그것을 지킨다
    pub(crate) fn design_handoff_start(&self, a: &mut StartArgs, task: &str) {
        let Some(rec) = self.design_handoff_load(&a.project, task) else { return };
        if a.base.is_none() {
            a.base = Some(format!("origin/design/{}", rec.slug));
        }
        a.task = format!("{}\n\n{}", a.task.trim_end(), rec.brief());
    }

    /// 구현 맡기기. 할 일이 생긴 뒤의 표지 commit·올리기·예약 실패는 거절하지 않고 경고로 돌려준다(할 일 메모와 대화 사건에도 남는다)
    pub(crate) async fn design_handoff(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        h: Handoff,
    ) -> Result<FrontResponse, ErrorBody> {
        // 사람만 맡긴다(thread·coordinator 세션은 기획 branch에 쓰지 않는다)
        self.design_writer(role, actor, project, None)?;
        let (target, _) = self.design_target(project, &h.repo)?;
        let title = h.title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            return Err(bad("할 일 제목이 비었다"));
        }
        tasks::check_title(&title).map_err(|e| bad(format!("할 일 제목: {e}")))?;
        let memo: Vec<String> = h.memo.lines().map(str::trim_end).filter(|l| !l.is_empty()).map(note_line).collect();
        if memo.is_empty() {
            return Err(bad("메모가 비었다(구현 thread의 할 일 글이 된다)"));
        }
        // 고를 때 읽은 판이어야 한다. 고른 블록이 넘기지 않은 계획인지 할 일을 만들기 전에 본다
        let FrontResponse::DesignDoc { doc } = self.design_read(project, &h.repo, &h.path, Some(&h.slug)).await? else {
            return Err(internal("예상하지 못한 응답"));
        };
        if doc.blob != h.blob {
            return Err(err(ErrorCode::Conflict, "그 사이 문서가 바뀌었다: 다시 읽고 고른다"));
        }
        let picks: Vec<usize> = h.blocks.iter().map(|&b| b as usize).collect();
        aaa_proto::design_mark_tasks(&doc.text, &picks, "d-0000").map_err(bad)?;
        let marks = aaa_proto::design_marks(&doc.text);
        let mut sections: Vec<String> = Vec::new();
        for &p in &picks {
            if let Some(m) = marks.get(p)
                && !m.section.is_empty()
                && !sections.contains(&m.section)
            {
                sections.push(m.section.clone());
            }
        }
        // 머신은 그 문서 저장소의 머신이다(기획 branch가 거기 있고, 자동 배정 머신이 없어도 예약이 자리를 얻는다)
        let machine = target
            .machine
            .as_deref()
            .map(|m| self.resolve_machine(Some(m)).map(|id| self.machine_name(&id)))
            .transpose()?;
        let task =
            self.design_handoff_task(project, actor, &target.name, &title, h.profile.as_deref(), machine, memo).await?;
        let id = task.short_id.clone().ok_or_else(|| internal("새 할 일에 표시 번호가 없다"))?;
        let rec = HandoffRec {
            task: id.clone(),
            repo: target.name.clone(),
            slug: h.slug.clone(),
            path: doc.path.clone(),
            sections,
            by: actor.to_string(),
            created_ms: aaa_common::util::now_ms(),
        };
        // 예약하면 바로 시작될 수 있다: 기록을 먼저 둔다
        self.design_handoff_store(project, &rec)?;
        let mut warnings: Vec<String> = Vec::new();
        let marked = aaa_proto::design_mark_tasks(&doc.text, &picks, &id).map_err(internal)?;
        let saved = self
            .design_save_event(
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
            )
            .await;
        let commit = match saved {
            Ok(FrontResponse::DesignSaved { commit, .. }) => Some(commit),
            Ok(_) => {
                warnings.push("표지를 적지 못했다: 예상하지 못한 응답".into());
                None
            }
            Err(e) => {
                warnings.push(format!("표지를 적지 못했다: {}", e.message));
                None
            }
        };
        let mut pushed = false;
        if commit.is_some() {
            // 쉼을 기다리지 않고 바로 올린다: 구현 thread는 origin의 그 branch에서 갈라진다
            self.design_push_now(project, &target.name, &h.slug).await;
            let rec = self.design_branch_load(project, &target.name, &h.slug)?;
            pushed = rec.as_ref().is_some_and(|r| r.pushed.is_some() && r.pushed == commit);
            if !pushed {
                let why = rec.and_then(|r| r.push_error).unwrap_or_else(|| "그 사이 새 저장이 있었다".into());
                warnings.push(format!("origin에 올리지 못했다({why}): 조금 뒤 다시 올린다"));
            }
        }
        let mut reserved = false;
        if h.reserve && pushed {
            match self.design_handoff_reserve(project, actor, &task).await {
                Ok(()) => reserved = true,
                Err(e) => warnings.push(format!("예약하지 못했다: {}", e.message)),
            }
        } else if h.reserve {
            warnings.push("올린 뒤에 예약한다(할 일 화면에서 예약)".into());
        }
        let warning = (!warnings.is_empty()).then(|| warnings.join(" · "));
        self.db.audit(
            actor,
            "design_handoff",
            &format!("{project} {}/{} {} {id} 블록 {} 예약 {reserved}", target.name, h.slug, doc.path, picks.len()),
        );
        Ok(FrontResponse::DesignHandoff { handoff: DesignHandoffView { task: id, commit, pushed, reserved, warning } })
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
        let brief = rec.brief();
        assert!(
            brief
                .contains("기획: `docs/game-loop/README.md` 「탑」·「나가기」(d-0005, 기획 branch `design/loop-plan`)"),
            "{brief}"
        );
        assert!(brief.contains("`origin/design/loop-plan`에서 갈라졌다"));
        assert!(brief.contains("`[!계획] d-0005` 표지를 풀고"));
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
    }
}
