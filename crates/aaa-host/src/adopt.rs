//! 사람이 이미 띄운 agent pane을 thread로 데려온다(herdr-projects 0.2.35 `src/adopt.rs` `adopt`, 차이 목록 4장 G #598).
//!
//! - 거부는 아무것도 만들기 전에 한다: 멈춘·보관한 프로젝트, 빈 제목, 없는·끝난 pane, Claude·Codex가 아닌 pane, 어느
//!   프로젝트의 coordinator pane(그 프로젝트 폴더에서 도는 pane), 이미 끝나지 않은 thread인 pane, adopt를 모르는 옛 agent.
//! - Claude·Codex인지는 띄울 때 알린 harness, 없으면 실행 파일 이름으로 정한다(agent `Harness::of_session`과 같은 규칙).
//!   그것으로 모르면(셸 pane) agent가 감지해 알린 상태의 harness다: 참고는 herdr가 감지한 agent가 있는 pane만 데려온다.
//! - 자리는 그 머신 agent가 만든다(`ThreadAdopt`: 세션이 지금 있는 폴더의 `.aaa/<p>-<t>/`). host는 git을 실행하지 않는다.
//! - launch는 pane 그대로다(대화를 이어 띄울 방법을 모른다): restart는 거부하고, resolve는 pane을 끝내지 않는다.
//!   brief는 다른 thread와 같은 길(`spawn_brief`)로 입력을 받을 수 있을 때 보낸다.

use std::sync::Arc;

use aaa_proto::{
    AgentRequest, AgentResponse, ErrorBody, ErrorCode, Launch, Role, ThreadKind, ThreadStatus, ThreadView,
};

use crate::orch::{ThreadRec, internal, write_atomic};
use crate::{Host, err};

/// 할 일을 주지 않았을 때(herdr-projects `DEFAULT_TASK`)
const DEFAULT_TASK: &str = "이 pane에서 하던 일을 이어 한다. 이제 위 프로젝트의 thread다: 그 지침을 따르고, 일을 마치거나 사람의 답을 기다리며 멈출 때 보고를 쓴다.";

/// pane이 Claude·Codex인가: 띄울 때 알린 harness, 그것으로 모르면 실행 파일 이름(경로는 끝 이름, `.exe` 등은 뗀다)
pub(crate) fn harness_of(declared: Option<&str>, argv: &[String]) -> Option<&'static str> {
    fn label(name: &str) -> Option<&'static str> {
        let mut name = name.trim().to_lowercase();
        for suffix in [".exe", ".cmd", ".bat", ".ps1", ".js"] {
            if let Some(s) = name.strip_suffix(suffix) {
                name = s.to_string();
                break;
            }
        }
        match name.rsplit(['/', '\\']).find(|c| !c.is_empty()).unwrap_or(&name) {
            "claude" | "claude-code" => Some("claude"),
            "codex" => Some("codex"),
            _ => None,
        }
    }
    declared.and_then(label).or_else(|| argv.first().and_then(|a| label(a)))
}

fn bad(msg: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, msg)
}

impl Host {
    pub(crate) async fn thread_adopt(
        self: &Arc<Self>,
        project: &str,
        pane: &str,
        title: &str,
        task: &str,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let dir = self.project_dir(project)?;
        self.check_active(project)?;
        let title = title.trim();
        if title.is_empty() {
            return Err(bad("--title이 비었다"));
        }
        let (pane, mid) = self.resolve_pane(pane)?;
        let info = self
            .inner
            .lock()
            .unwrap()
            .panes
            .get(&pane)
            .map(|p| p.info.clone())
            .ok_or_else(|| err(ErrorCode::NotFound, format!("pane이 없다: {pane}")))?;
        if !info.state.is_running() {
            return Err(bad(format!("pane {pane}은 끝났다. 실행 중인 agent pane만 데려온다")));
        }
        let detected = || self.pane_agent(&pane).and_then(|a| harness_of(Some(&a.harness), &[]));
        let Some(harness) = harness_of(info.harness.as_deref(), &info.argv).or_else(detected) else {
            return Err(bad(format!(
                "pane {pane}은 Claude·Codex pane이 아니다(띄운 프로그램도, 감지한 agent도 아니다). 그 pane에서 도는 agent만 데려온다"
            )));
        };
        for p in self.projects_all() {
            if let Ok(d) = self.project_dir(&p.name)
                && self.coordinator_panes(&d).iter().any(|v| v.id == pane)
            {
                return Err(bad(format!("pane {pane}은 `{}`의 coordinator다(작업 폴더가 그 프로젝트 폴더다)", p.name)));
            }
        }
        if let Some(r) = self.thread_of_pane(&pane) {
            return Err(bad(format!("pane {pane}은 이미 `{}`의 thread {}다", r.project, r.id)));
        }
        if !self.machine_feature(&mid, "adopt.v1") {
            return Err(bad(format!(
                "머신 {}의 agent가 thread adopt를 모른다. agent를 새 aaa로 바꾼 뒤 다시 한다",
                self.machine_name(&mid)
            )));
        }
        let conn = self.live_conn(&mid)?;

        let now = aaa_common::util::now_ms();
        let launch = Launch {
            profile: harness.to_string(),
            harness: harness.to_string(),
            argv: info.argv.clone(),
            env: vec![],
            resume_argv: vec![],
            session_key: None,
        };
        let mut rec = ThreadRec {
            pane: Some(pane.clone()),
            launch: Some(launch),
            ..ThreadRec::starting(project, title, &mid, harness, Role::Worker, ThreadKind::Adopted, now)
        };
        let id = self
            .db
            .thread_insert_admitted(
                project,
                None,
                None,
                Some(self.settings(project).max_parallel_threads),
                None,
                |id| {
                    let mut r = rec.clone();
                    r.id = id.to_string();
                    serde_json::to_string(&r).unwrap()
                },
            )
            .map(|(id, _)| id)
            .map_err(internal)?;
        rec.id = id.clone();
        self.db.audit(actor, "thread_adopt", &format!("{project}/{id} pane {pane} @{}", self.machine_name(&mid)));
        let task = if task.trim().is_empty() { DEFAULT_TASK } else { task };
        write_atomic(&dir.join("threads").join(format!("{id}.task.md")), task.as_bytes()).map_err(internal)?;
        let brief = self.brief(&dir, &rec, task, false);
        self.flow(
            &rec,
            "thread_started",
            Some(format!("threads/{id}.task.md")),
            format!("{title}(pane {pane}을 데려옴)"),
        );

        let adopted = match conn
            .request(AgentRequest::ThreadAdopt(aaa_proto::ThreadAdopt {
                project: project.to_string(),
                thread: id.clone(),
                session: pane.clone(),
                brief,
            }))
            .await
        {
            Ok(AgentResponse::ThreadAdopted(a)) => Ok(a),
            Ok(other) => Err(internal(format!("예상하지 못한 응답: {other:?}"))),
            Err(e) => Err(e),
        };
        let a = match adopted {
            Ok(a) => a,
            Err(e) => {
                // 참고 `adopt`: brief를 쓰지 못한 thread는 실패로 남긴다(pane은 그대로)
                if let Ok(r) = self.update_rec(project, &id, |r| {
                    r.status = ThreadStatus::Failed;
                    r.error = Some(e.message.clone());
                }) {
                    rec = r;
                }
                self.flow(&rec, "thread_failed", None, e.message.clone());
                return Err(e);
            }
        };
        let rec = self.update_rec(project, &id, |r| {
            r.cwd = Some(a.cwd.clone());
            r.work_dir = Some(a.work_dir.clone());
            r.repo = a.repo.clone();
            r.branch = a.branch.clone();
            r.status = ThreadStatus::Open;
        })?;
        let _ = conn
            .request(AgentRequest::ThreadWatch { thread: format!("{project}/{id}"), work_dir: a.work_dir.clone() })
            .await;
        self.spawn_brief(project, &id);
        Ok(self.thread_view(&rec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harness_from_declared_then_program_name() {
        let argv = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(harness_of(None, &argv(&["/opt/bin/claude", "--resume"])), Some("claude"));
        assert_eq!(harness_of(None, &argv(&["C:\\bin\\Codex.EXE"])), Some("codex"));
        assert_eq!(harness_of(Some("codex"), &argv(&["/bin/sh", "x.sh"])), Some("codex"));
        // 셸·custom profile은 상태를 모른다
        assert_eq!(harness_of(Some("custom"), &argv(&["/bin/sh"])), None);
        assert_eq!(harness_of(None, &argv(&["zsh"])), None);
        assert_eq!(harness_of(None, &[]), None);
    }
}
