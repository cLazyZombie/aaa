//! 프로젝트 멈추기·보관·삭제·이름 바꾸기·청소(herdr-projects 0.2.35 `lifecycle.rs`·`rename.rs`·`sweep.rs`, 차이 목록 4장 G #592,
//! plan §8.155).
//!
//! - 상태는 host DB(`project_status:<프로젝트>`)에 둔다. 참고는 프로젝트 폴더 `.state/project.json`이지만 aaa coordinator의 cwd가
//!   그 폴더라 파일을 고쳐 멈춘 프로젝트를 되살릴 수 있다(§8.19와 같은 까닭으로 문을 폴더 밖에).
//! - paused·archived: ticker가 건너뛰고 `thread start`를 거부한다. archived는 목록에서 숨고 `project open`을 거부하며, 보관할 때
//!   그 프로젝트의 살아 있는 pane(coordinator·끝나지 않은 thread)을 닫는다(참고 `close_workspaces`).
//! - 삭제는 폴더를 `<projects root>/.trash/<이름>-<UTC>`로 옮기고 host 기록을 그 안 `aaa-records.json`으로 뺀 뒤 지운다(같은 이름으로
//!   다시 만든 프로젝트가 옛 thread를 물려받지 않게). worktree·branch·PR은 건드리지 않는다.
//! - 이름 바꾸기는 끝나지 않은 thread가 있거나 coordinator가 돌면 거부한다(참고는 coordinator가 쉴 때 ticker가 닫고 바꾼 뒤 다시
//!   연다). DB의 project 열·thread 기록·meta와 폴더, 뼈대 그대로인 AGENTS.md를 바꾼다.
//! - 청소는 끝난 worktree thread가 resolve 때 남긴 worktree·branch(`ThreadRec.leftover`)를 그 머신 agent에 다시 정리하게 한다
//!   (force 없음, 병합된 branch만). 참고는 저장소의 `hp/<slug>/` worktree를 직접 훑지만 aaa worktree는 thread 머신에 있다.

use std::path::Path;
use std::sync::Arc;

use aaa_proto::{
    AgentRequest, AgentResponse, ErrorBody, ErrorCode, FrontResponse, ProjectAction, ProjectChange, ProjectStatus,
    ThreadKind, ThreadStatus, valid_project,
};

use crate::orch::{ThreadRec, internal};
use crate::{Host, err};

/// resolve 때 남긴 것(청소 대상)
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Leftover {
    pub worktree: bool,
    pub branch: bool,
}

fn status_key(project: &str) -> String {
    format!("project_status:{project}")
}

fn bad(msg: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, msg)
}

/// 살아 있는 pane 하나: (무엇, pane ID, 머신 ID, 일하는 중)
struct Live {
    what: String,
    pane: String,
    machine_id: String,
    working: bool,
}

impl Host {
    pub(crate) fn project_status(&self, project: &str) -> ProjectStatus {
        match self.db.meta(&status_key(project)).ok().flatten().as_deref() {
            Some("paused") => ProjectStatus::Paused,
            Some("archived") => ProjectStatus::Archived,
            _ => ProjectStatus::Active,
        }
    }

    /// `thread start`(참고 threads.rs 「`thread start` is refused until it is active again」)
    pub(crate) fn check_active(&self, project: &str) -> Result<(), ErrorBody> {
        match self.project_status(project) {
            ProjectStatus::Active => Ok(()),
            ProjectStatus::Paused => Err(err(
                ErrorCode::BadRequest,
                format!("`{project}`는 멈춰 있어 thread를 시작하지 않는다(`aaa project resume {project}`)"),
            )),
            ProjectStatus::Archived => Err(err(
                ErrorCode::BadRequest,
                format!("`{project}`는 보관돼 있어 thread를 시작하지 않는다(`aaa project unarchive {project}`)"),
            )),
        }
    }

    fn open_recs(&self, project: &str) -> Vec<ThreadRec> {
        self.db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| r.status != ThreadStatus::Resolved)
            .collect()
    }

    /// 그 프로젝트에서 살아 있는 pane: 폴더를 cwd로 쓰는 host 머신 pane(coordinator)과 끝나지 않은 thread pane(참고 `alive_panes`)
    fn live_panes(&self, project: &str, dir: &Path) -> Vec<Live> {
        let working =
            |p: &aaa_proto::PaneView| p.agent.as_ref().is_some_and(|a| a.activity == aaa_proto::AgentActivity::Working);
        let mut out: Vec<Live> = self
            .coordinator_panes(dir)
            .into_iter()
            .map(|p| Live { what: "coordinator".into(), working: working(&p), pane: p.id, machine_id: p.machine_id })
            .collect();
        for r in self.open_recs(project) {
            let Some(pane) = &r.pane else { continue };
            if let Some(p) = self.pane_view_of(pane, aaa_proto::DeviceRole::Local).filter(|p| p.state.is_running()) {
                out.push(Live { what: r.id.clone(), working: working(&p), pane: p.id, machine_id: p.machine_id });
            }
        }
        out
    }

    /// 없어진(삭제·옛 이름) 프로젝트의 thread를 front 목록에서 빼게 「보관」으로 한 번 알린다(thread를 지우는 사건이 따로 없다)
    fn hide_threads(&self, recs: &[ThreadRec]) {
        for r in recs {
            let mut v = self.thread_view(r);
            v.project_status = ProjectStatus::Archived;
            let _ = self.thread_events.send(v);
        }
    }

    fn change(&self, project: &str, dir: &Path, notes: Vec<String>) -> FrontResponse {
        FrontResponse::ProjectChange(ProjectChange { project: self.project_view(project, dir), notes })
    }

    /// 멈추기·되살리기·보관·보관 풀기(참고 `lifecycle::set_status`)
    pub(crate) async fn project_lifecycle(
        self: &Arc<Self>,
        project: &str,
        action: ProjectAction,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        let current = self.project_status(project);
        let to = match (action, current) {
            (ProjectAction::Pause, ProjectStatus::Archived) => {
                return Err(bad(format!("`{project}`는 보관돼 있다: 먼저 `aaa project unarchive {project}`")));
            }
            (ProjectAction::Resume, ProjectStatus::Archived) => {
                return Err(bad(format!("`{project}`는 보관돼 있다: `aaa project unarchive {project}`")));
            }
            (ProjectAction::Pause, _) => ProjectStatus::Paused,
            (ProjectAction::Archive, _) => ProjectStatus::Archived,
            (ProjectAction::Resume | ProjectAction::Unarchive, _) => ProjectStatus::Active,
        };
        let key = status_key(project);
        match to {
            ProjectStatus::Active => self.db.del_meta(&key),
            ProjectStatus::Paused => self.db.set_meta(&key, "paused"),
            ProjectStatus::Archived => self.db.set_meta(&key, "archived"),
        }
        .map_err(internal)?;
        self.db.audit(actor, "project_status", &format!("{project} {to:?}"));
        let word = match to {
            ProjectStatus::Active => "진행 중",
            ProjectStatus::Paused => "멈춤",
            ProjectStatus::Archived => "보관",
        };
        let mut notes = vec![format!("`{project}`: 이제 {word}")];
        match to {
            ProjectStatus::Paused => {
                notes.push("ticker가 건너뛰고 `thread start`를 거부한다. 도는 agent는 멈추지 않는다".into());
                for l in self.live_panes(project, &dir).into_iter().filter(|l| l.working) {
                    notes.push(format!("아직 일하는 중: {} (pane {})", l.what, l.pane));
                }
            }
            ProjectStatus::Archived => {
                notes.push(format!(
                    "목록에서 숨고, ticker가 건너뛰며, `aaa project unarchive {project}` 전까지 `thread start`·`project open`을 \
                     거부한다. 폴더와 끝나지 않은 thread의 worktree는 남는다"
                ));
                for l in self.live_panes(project, &dir) {
                    let closed = match self.live_conn(&l.machine_id) {
                        Ok(conn) => conn.request(AgentRequest::Kill { session: l.pane.clone() }).await.map(|_| ()),
                        Err(e) => Err(e),
                    };
                    notes.push(match closed {
                        Ok(()) => format!("{}의 pane {}을 닫았다", l.what, l.pane),
                        Err(e) => format!("{}의 pane {}을 닫지 못했다: {}", l.what, l.pane, e.message),
                    });
                }
            }
            ProjectStatus::Active if current == ProjectStatus::Archived => {
                notes.push(format!("coordinator는 `aaa project open {project}`로 연다"));
            }
            ProjectStatus::Active => {}
        }
        // front가 다시 묻지 않고 알게 그 프로젝트 thread 사건을 다시 낸다(`ThreadView.project_status`)
        for j in self.db.threads(Some(project)).unwrap_or_default() {
            if let Ok(r) = serde_json::from_str::<ThreadRec>(&j) {
                let _ = self.thread_events.send(self.thread_view(&r));
            }
        }
        self.tick_wake.notify_one();
        Ok(self.change(project, &dir, notes))
    }

    /// 폴더를 휴지통으로(참고 `lifecycle::delete`). 살아 있는 pane이 있으면 `force` 없이는 거부한다.
    pub(crate) async fn project_delete(
        self: &Arc<Self>,
        project: &str,
        force: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        let live = self.live_panes(project, &dir);
        if !force && !live.is_empty() {
            let list: Vec<String> = live.iter().map(|l| format!("{} (pane {})", l.what, l.pane)).collect();
            return Err(err(
                ErrorCode::BadRequest,
                format!("`{project}`에 살아 있는 pane이 있다: {}. 닫거나 --force를 준다", list.join(", ")),
            ));
        }
        let records = self.db.project_export(project).map_err(internal)?;
        let recs: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .collect();
        let trash = self.projects_root().join(".trash");
        let target = trash.join(format!("{project}-{}", aaa_common::util::utc_compact(aaa_common::util::now_ms())));
        std::fs::create_dir_all(&trash).map_err(internal)?;
        std::fs::rename(&dir, &target)
            .map_err(|e| internal(format!("{}를 휴지통으로 옮기지 못했다: {e}", dir.display())))?;
        let json = serde_json::to_vec_pretty(&records).map_err(internal)?;
        crate::orch::write_atomic(&target.join("aaa-records.json"), &json).map_err(internal)?;
        self.hide_threads(&recs);
        self.db.project_forget(project).map_err(internal)?;
        self.db.audit(actor, "project_delete", &format!("{project} → {}", target.display()));
        // 끝나지 않은 thread의 감시를 푼다(pane은 그대로, 참고도 닫지 않는다)
        for r in recs.iter().filter(|r| r.status != ThreadStatus::Resolved) {
            if let Ok(conn) = self.live_conn(&r.machine_id) {
                let _ = conn.request(AgentRequest::ThreadUnwatch { thread: format!("{project}/{}", r.id) }).await;
            }
        }
        let mut notes =
            vec![format!("`{project}`를 {}로 옮겼다(host 기록은 그 안 aaa-records.json)", target.display())];
        let left: Vec<&ThreadRec> = recs.iter().filter(|r| r.kind == ThreadKind::Worktree && r.cwd.is_some()).collect();
        if !left.is_empty() {
            notes.push("그대로 둔 것(필요 없으면 직접 지운다):".into());
            for r in left {
                notes.push(format!(
                    "  {}: {}의 worktree {}, branch {}",
                    r.id,
                    self.machine_name(&r.machine_id),
                    r.cwd.as_deref().unwrap_or("-"),
                    r.branch.as_deref().unwrap_or("-")
                ));
            }
        }
        notes.extend(self.safety_table_note(project, "삭제한"));
        Ok(FrontResponse::ProjectChange(ProjectChange {
            project: aaa_proto::ProjectView {
                name: project.to_string(),
                path: target.to_string_lossy().into_owned(),
                threads: recs.len() as u32,
                status: ProjectStatus::Archived,
            },
            notes,
        }))
    }

    /// 이름(폴더 이름)을 바꾼다(참고 `rename`). 끝나지 않은 thread나 도는 coordinator가 있으면 거부한다.
    pub(crate) async fn project_rename(
        self: &Arc<Self>,
        project: &str,
        to: &str,
        dry_run: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        if !valid_project(to) {
            return Err(bad(format!("프로젝트 이름 형식이 아니다(영문 소문자·숫자·-·_): {to}")));
        }
        if to == aaa_config::safety::DEFAULT_TABLE {
            return Err(bad("`default`는 safety의 모든 프로젝트 표 이름이라 프로젝트 이름으로 쓰지 않는다"));
        }
        let new_dir = self.projects_root().join(to);
        if new_dir.exists() {
            return Err(err(ErrorCode::NameTaken, format!("이미 있다: {}", new_dir.display())));
        }
        let open: Vec<String> = self.open_recs(project).into_iter().map(|r| r.id).collect();
        if !open.is_empty() {
            return Err(err(
                ErrorCode::BadRequest,
                format!("끝나지 않은 thread가 있어 이름을 바꾸지 않는다: {}(`aaa thread resolve`)", open.join(", ")),
            ));
        }
        let coords: Vec<String> = self.coordinator_panes(&dir).into_iter().map(|p| p.id).collect();
        if !coords.is_empty() {
            return Err(err(
                ErrorCode::BadRequest,
                format!(
                    "coordinator가 이 폴더에서 돈다(pane {}): 닫은 뒤 바꾸고 새 이름으로 다시 연다",
                    coords.join(", ")
                ),
            ));
        }
        let guide = dir.join("AGENTS.md");
        let stock = std::fs::read_to_string(&guide).ok().is_some_and(|t| t == crate::orch::coordinator_guide(project));
        let mut notes = vec![
            format!("폴더 {} → {}", dir.display(), new_dir.display()),
            "host 기록(thread·inbox·routine·평가)을 새 이름으로 옮긴다".into(),
        ];
        notes.push(if stock {
            "AGENTS.md(coordinator 안내)를 새 이름으로 다시 쓴다".into()
        } else {
            "AGENTS.md는 고친 흔적이 있어 그대로 둔다: 안의 프로젝트 이름은 직접 고친다".into()
        });
        notes.push("끝난 thread의 branch(`aaa/<옛 이름>/…`)는 그 이름 그대로다".into());
        notes.extend(self.safety_table_note(project, "옛"));
        if dry_run {
            notes.insert(0, "(--dry-run: 아무것도 바꾸지 않았다)".into());
            return Ok(self.change(project, &dir, notes));
        }
        let old_recs: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .collect();
        std::fs::rename(&dir, &new_dir).map_err(|e| internal(format!("폴더 이름을 바꾸지 못했다: {e}")))?;
        let moved = self.db.project_rename(project, to, |json| match serde_json::from_str::<serde_json::Value>(json) {
            Ok(mut v) => {
                v["project"] = to.into();
                v.to_string()
            }
            Err(_) => json.to_string(),
        });
        if let Err(e) = moved {
            // 기록을 못 옮겼으면 폴더도 되돌린다(기록과 폴더 이름이 어긋나지 않게)
            let _ = std::fs::rename(&new_dir, &dir);
            return Err(internal(format!("host 기록을 옮기지 못해 되돌렸다: {e:#}")));
        }
        if stock {
            let _ =
                crate::orch::write_atomic(&new_dir.join("AGENTS.md"), crate::orch::coordinator_guide(to).as_bytes());
        }
        self.db.audit(actor, "project_rename", &format!("{project} → {to}"));
        self.hide_threads(&old_recs);
        for j in self.db.threads(Some(to)).unwrap_or_default() {
            if let Ok(r) = serde_json::from_str::<ThreadRec>(&j) {
                let _ = self.thread_events.send(self.thread_view(&r));
            }
        }
        self.tick_wake.notify_one();
        Ok(self.change(to, &new_dir, notes))
    }

    /// 끝난 thread가 resolve 때 남긴 worktree·branch를 다시 정리한다(참고 `sweep`)
    pub(crate) async fn project_sweep(
        self: &Arc<Self>,
        project: &str,
        dry_run: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        let found: Vec<ThreadRec> = self
            .db
            .threads(Some(project))
            .unwrap_or_default()
            .iter()
            .filter_map(|j| serde_json::from_str::<ThreadRec>(j).ok())
            .filter(|r| r.status == ThreadStatus::Resolved && r.kind == ThreadKind::Worktree && r.cwd.is_some())
            .filter(|r| r.leftover.as_ref().is_some_and(|l| l.worktree || l.branch))
            .collect();
        let describe = |r: &ThreadRec| {
            let l = r.leftover.clone().unwrap_or_default();
            let what: Vec<String> = [
                l.worktree.then(|| format!("worktree {}", r.cwd.as_deref().unwrap_or("-"))),
                l.branch.then(|| format!("branch {}", r.branch.as_deref().unwrap_or("-"))),
            ]
            .into_iter()
            .flatten()
            .collect();
            format!("{}({}): {}", r.id, self.machine_name(&r.machine_id), what.join(", "))
        };
        if found.is_empty() {
            return Ok(self.change(project, &dir, vec![format!("`{project}`에 치울 것이 없다")]));
        }
        if dry_run {
            return Ok(self.change(project, &dir, found.iter().map(describe).collect()));
        }
        let mut notes = Vec::new();
        let (mut done, mut kept) = (0, 0);
        for r in &found {
            let before = describe(r);
            let res = match self.live_conn(&r.machine_id) {
                Ok(conn) => conn
                    .request(AgentRequest::ThreadCleanup {
                        repo: r.repo.clone(),
                        cwd: r.cwd.clone().unwrap_or_default(),
                        branch: r.branch.clone(),
                        // branch는 PR이 병합됐고 로컬 끝이 병합된 head일 때만 지운다(참고 sweep `Orphan::Branch`, resolve와 같다)
                        merged_head: if r.repo.is_some() { self.merged_head(project, r, &conn).await } else { None },
                        keep_worktree: false,
                        uncopied: false,
                    })
                    .await
                    .and_then(|res| match res {
                        AgentResponse::Cleanup(c) => Ok(c),
                        other => Err(internal(format!("예상하지 못한 응답: {other:?}"))),
                    }),
                Err(e) => Err(e),
            };
            match res {
                Ok(c) => {
                    let l = Leftover { worktree: !c.worktree_removed, branch: r.branch.is_some() && !c.branch_deleted };
                    let clean = !l.worktree && !l.branch;
                    let _ = self.update_rec(project, &r.id, |x| x.leftover = Some(l));
                    if clean {
                        done += 1;
                        notes.push(format!("치웠다: {before}"));
                    } else {
                        kept += 1;
                        notes.push(format!("남겼다: {before}({})", c.notes.join("; ")));
                    }
                }
                Err(e) => {
                    kept += 1;
                    notes.push(format!("남겼다: {before}({})", e.message));
                }
            }
        }
        notes.push(format!("`{project}` 청소: {done}개 치움, {kept}개 남김"));
        self.db.audit(actor, "project_sweep", &format!("{project} {done}/{}", found.len()));
        Ok(self.change(project, &dir, notes))
    }

    /// host.toml(·local)의 `[safety.<프로젝트>]` 표가 남는다는 안내(참고 delete의 마지막 줄). 표는 사람만 고친다(§8.153).
    fn safety_table_note(&self, project: &str, which: &str) -> Option<String> {
        let dir = self.config_dir.get()?;
        let files: Vec<&str> = ["host.toml", "host.local.toml"]
            .into_iter()
            .filter(|f| {
                std::fs::read_to_string(dir.join(f))
                    .ok()
                    .and_then(|t| t.parse::<toml::Table>().ok())
                    .is_some_and(|t| t.get("safety").and_then(|s| s.get(project)).is_some())
            })
            .collect();
        (!files.is_empty()).then(|| {
            format!(
                "{} `[safety.{project}]` 표는 {}에 남아 같은 이름의 프로젝트에 다시 먹는다(사람이 고친다)",
                which,
                files.join("·")
            )
        })
    }
}
