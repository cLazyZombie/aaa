//! 병합 대기열(§8.8)과 머신별 동시 thread 한도(§8.10).
//!
//! - 대기열은 순번과 흐름 사건만 둔다(`merge_enqueued`·`merge_dropped`·`merge_landed`·`merge_head`).
//!   맨 앞이 바뀌면 `merge_head` 사건을 내고, 그 thread에 무엇을 할지는 coordinator가 정한다. 단계 실행은 M3에서 한다.
//! - resolve한 thread는 대기열에서 빠진다.
//! - `merge_land`(§8.24): 맨 앞 thread를 그 머신 agent가 병합·push한다. 단계마다 `merge_step` 사건을 내고,
//!   push까지 되면 `merge_landed`로 대기열에서 빼고 같은 기준의 다른 열린 thread에 「main 이동」을 대기열 메시지로 알린다.
//!   한 프로젝트에서 동시에 하나만 돈다.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use aaa_proto::{
    AgentRequest, AgentResponse, ErrorBody, ErrorCode, MergeEntry, MergeLandView, MessageMode, Role, ThreadKind,
    ThreadStatus,
};

use crate::orch::ThreadRec;
use crate::{Host, err};

fn internal(e: impl std::fmt::Display) -> ErrorBody {
    err(ErrorCode::Internal, e.to_string())
}

impl Host {
    pub(crate) fn merge_queue(&self, project: &str) -> Result<Vec<MergeEntry>, ErrorBody> {
        self.project_dir(project)?;
        let rows = self.db.merge_list(project).map_err(internal)?;
        Ok(rows
            .into_iter()
            .enumerate()
            .map(|(i, (thread, at))| {
                let rec = self.load(project, &thread).ok();
                MergeEntry {
                    project: project.to_string(),
                    position: i as u32 + 1,
                    title: rec.as_ref().map(|r| r.title.clone()).unwrap_or_default(),
                    machine: rec.as_ref().map(|r| self.machine_name(&r.machine_id)).unwrap_or_default(),
                    branch: rec.and_then(|r| r.branch),
                    thread,
                    enqueued_at_ms: at,
                }
            })
            .collect())
    }

    fn merge_head(&self, project: &str) -> Option<String> {
        self.db.merge_list(project).ok()?.into_iter().next().map(|(t, _)| t)
    }

    /// 맨 앞이 바뀌었으면 사건을 낸다
    fn merge_head_changed(&self, project: &str, before: Option<String>) {
        let after = self.merge_head(project);
        if after != before
            && let Some(t) = after
            && let Ok(r) = self.load(project, &t)
        {
            self.flow(&r, "merge_head", None, "병합 대기열 맨 앞");
        }
    }

    pub(crate) fn merge_enqueue(&self, project: &str, thread: &str, actor: &str) -> Result<Vec<MergeEntry>, ErrorBody> {
        let rec = self.load(project, thread)?;
        if rec.status != ThreadStatus::Open {
            return Err(err(ErrorCode::BadRequest, format!("열린 thread만 넣는다(지금 {})", rec.status.as_str())));
        }
        let before = self.merge_head(project);
        if self.db.merge_add(project, thread).map_err(internal)? {
            let q = self.merge_queue(project)?;
            let pos = q.iter().find(|e| e.thread == thread).map(|e| e.position).unwrap_or(0);
            self.db.audit(actor, "merge_enqueue", &format!("{project}/{thread}"));
            self.flow(&rec, "merge_enqueued", None, format!("순번 {pos}"));
            self.merge_head_changed(project, before);
        }
        self.merge_queue(project)
    }

    pub(crate) fn merge_drop(
        &self,
        project: &str,
        thread: &str,
        landed: bool,
        actor: &str,
    ) -> Result<Vec<MergeEntry>, ErrorBody> {
        let rec = self.load(project, thread)?;
        let before = self.merge_head(project);
        if !self.db.merge_remove(project, thread).map_err(internal)? {
            return Err(err(ErrorCode::NotFound, format!("병합 대기열에 없다: {project}/{thread}")));
        }
        self.db.audit(actor, if landed { "merge_landed" } else { "merge_drop" }, &format!("{project}/{thread}"));
        self.flow(&rec, if landed { "merge_landed" } else { "merge_dropped" }, None, actor.to_string());
        self.merge_head_changed(project, before);
        self.merge_queue(project)
    }

    /// 맨 앞 thread를 병합한다(§8.24). `want`를 주면 그것이 맨 앞이어야 한다.
    pub(crate) async fn merge_land(
        self: &Arc<Self>,
        project: &str,
        want: Option<&str>,
        actor: &str,
    ) -> Result<MergeLandView, ErrorBody> {
        self.project_dir(project)?;
        let head = self
            .merge_head(project)
            .ok_or_else(|| err(ErrorCode::BadRequest, format!("병합 대기열이 비었다: {project}")))?;
        if let Some(w) = want
            && w != head
        {
            return Err(err(ErrorCode::BadRequest, format!("{w}는 맨 앞이 아니다(맨 앞: {head}). 순번대로 병합한다")));
        }
        let rec = self.load(project, &head)?;
        if rec.status != ThreadStatus::Open {
            return Err(err(ErrorCode::BadRequest, format!("열린 thread만 병합한다(지금 {})", rec.status.as_str())));
        }
        let (Some(repo), Some(cwd), Some(branch), Some(base), ThreadKind::Worktree) =
            (rec.repo.clone(), rec.cwd.clone(), rec.branch.clone(), rec.base.clone(), rec.kind)
        else {
            return Err(err(ErrorCode::BadRequest, "worktree thread만 병합한다(repo·branch·기준이 있어야 한다)"));
        };
        // 기준은 원격 branch(예: origin/main)여야 push할 곳이 정해진다
        let Some((remote, target)) = base.split_once('/').filter(|(r, t)| !r.is_empty() && !t.is_empty()) else {
            return Err(err(
                ErrorCode::BadRequest,
                format!("기준 {base}가 원격 branch가 아니다. 자동 병합은 <원격>/<branch> 기준 thread만 한다"),
            ));
        };
        let conn = self.live_conn(&rec.machine_id)?;
        let _guard = LandGuard::take(project)
            .ok_or_else(|| err(ErrorCode::Unavailable, format!("{project}에서 이미 병합 중이다")))?;
        self.db.audit(actor, "merge_land", &format!("{project}/{head}"));
        self.flow(&rec, "merge_step", None, format!("시작: {branch} → {base}"));
        let req = AgentRequest::MergeLand {
            repo,
            cwd,
            branch: branch.clone(),
            remote: remote.to_string(),
            target: target.to_string(),
            message: format!("병합: {} ({project}/{head})", rec.title),
        };
        let out = match conn.request_timeout(req, LAND_TIMEOUT).await {
            Ok(AgentResponse::Landed(o)) => o,
            Ok(_) => return Err(internal("예상하지 못한 응답")),
            Err(e) => {
                self.flow(&rec, "merge_step", None, format!("실패: {}", e.message));
                return Err(e);
            }
        };
        for st in &out.steps {
            let first = st.note.lines().next().unwrap_or("");
            self.flow(
                &rec,
                "merge_step",
                None,
                format!("{}: {} — {first}", st.name, if st.ok { "ok" } else { "실패" }),
            );
        }
        // push까지 됐으면 병합된 것이다(뒤 정리가 실패해도)
        let landed = out.commit.is_some();
        let mut notified = Vec::new();
        let queue = if landed {
            let q = self.merge_drop(project, &head, true, actor)?;
            let sha = out.commit.as_deref().map(|c| &c[..c.len().min(12)]).unwrap_or("");
            let text = format!(
                "{base}가 움직였다: {project}/{head} 「{}」를 병합했다({sha}). 다음 병합 전에 `git fetch {remote} && git merge {base}`을 실행해 branch에 합치고 다시 시험한다.",
                rec.title
            );
            let others: Vec<_> = self
                .db
                .threads(Some(project))
                .unwrap_or_default()
                .iter()
                .filter_map(|j| serde_json::from_str::<crate::orch::ThreadRec>(j).ok())
                .filter(|r| {
                    r.id != head
                        && r.status == ThreadStatus::Open
                        && r.role != Role::Evaluator
                        && r.kind == ThreadKind::Worktree
                        && r.base.as_deref() == Some(base.as_str())
                })
                .collect();
            for r in others {
                match self.thread_message(project, &r.id, text.clone(), MessageMode::Queue, "aaa:merge").await {
                    Ok(_) => notified.push(r.id.clone()),
                    Err(e) => tracing::warn!(thread = %r.id, "main 이동 알림 실패: {}", e.message),
                }
            }
            q
        } else {
            self.merge_queue(project)?
        };
        Ok(MergeLandView {
            project: project.to_string(),
            thread: head,
            ok: landed,
            steps: out.steps,
            commit: out.commit,
            notified,
            queue,
        })
    }

    /// resolve한 thread는 대기열에서 뺀다
    pub(crate) fn merge_forget(&self, r: &ThreadRec) {
        let before = self.merge_head(&r.project);
        if self.db.merge_remove(&r.project, &r.id).unwrap_or(false) {
            self.flow(r, "merge_dropped", None, "resolve");
            self.merge_head_changed(&r.project, before);
        }
    }
}

/// 병합 단계 전체를 기다리는 시간(fetch·push가 느린 원격을 생각한다)
const LAND_TIMEOUT: Duration = Duration::from_secs(600);

/// 프로젝트마다 병합은 하나씩
struct LandGuard(String);

fn landing() -> &'static Mutex<HashSet<String>> {
    static L: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    L.get_or_init(Default::default)
}

impl LandGuard {
    fn take(project: &str) -> Option<LandGuard> {
        landing().lock().unwrap().insert(project.to_string()).then(|| LandGuard(project.to_string()))
    }
}

impl Drop for LandGuard {
    fn drop(&mut self) {
        landing().lock().unwrap().remove(&self.0);
    }
}
