//! 프로젝트 멈추기·보관·삭제·이름 바꾸기·청소(herdr-projects 0.2.35 `lifecycle.rs`·`rename.rs`·`sweep.rs`, 차이 목록 4장 G #592,
//! plan §8.180).
//!
//! - 상태는 host DB(`project_status:<프로젝트>`)에 둔다. 참고는 프로젝트 폴더 `.state/project.json`이지만 aaa coordinator의 cwd가
//!   그 폴더라 파일을 고쳐 멈춘 프로젝트를 되살릴 수 있다(§8.19와 같은 까닭으로 문을 폴더 밖에).
//! - paused·archived: ticker가 건너뛰고 `thread start`를 거부한다. archived는 목록에서 숨고 `project open`을 거부하며, 보관할 때
//!   그 프로젝트의 살아 있는 pane(coordinator·끝나지 않은 thread)을 닫는다(참고 `close_workspaces`).
//! - 삭제는 폴더를 `<projects root>/.trash/<이름>-<UTC>`로 옮기고 host 기록을 그 안 `aaa-records.json`으로 뺀 뒤 지운다(같은 이름으로
//!   다시 만든 프로젝트가 옛 thread를 물려받지 않게). worktree·branch·PR은 건드리지 않는다.
//! - 이름 바꾸기는 끝나지 않은 thread가 있거나 coordinator가 돌면 거부한다(참고는 coordinator가 쉴 때 ticker가 닫고 바꾼 뒤 다시
//!   연다). DB의 project 열·thread 기록·meta와 폴더, aaa 표지가 있는 AGENTS.md를 바꾼다.
//! - 청소는 끝난 worktree thread가 resolve 때 남긴 worktree·branch(`ThreadRec.leftover`)를 그 머신 agent에 다시 정리하게 한다
//!   (force 없음, 병합된 branch만). 참고는 저장소의 `hp/<slug>/` worktree를 직접 훑지만 aaa worktree는 thread 머신에 있다.
//! - 한 host는 프로젝트 하나(plan §8.283, 프로젝트 단계 없애기 S0): 보관하지 않은 프로젝트가 둘 이상이면 host가 시작하지 않고
//!   보관·삭제할 목록을 말한다. 남은 하나의 이름이 이 host의 내부 이름이다(DB·폴더·branch의 `projects` 같은 이름을 옮기지 않는다).
//! - 파일 점검(`aaa doctor files [--fix]`, 참고 `doctor`의 `files <slug>`)은 모든 프로젝트의 AGENTS.md(coordinator 안내)·
//!   uploads/·routines/pr-followup.md를 보고, `--fix`면 고친다(`orch::write_files`). 있는 프로젝트에 새 안내가 닿는 길이다.

use std::path::Path;
use std::sync::Arc;

use aaa_proto::{
    AgentRequest, AgentResponse, ErrorBody, ErrorCode, FrontResponse, ProjectAction, ProjectChange, ProjectFilesCheck,
    ProjectStatus, ProjectView, ThreadKind, ThreadStatus, valid_project,
};

use crate::orch::{ThreadRec, internal};
use crate::{Host, err};

/// resolve 때 남긴 것(청소 대상)
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Leftover {
    pub worktree: bool,
    pub branch: bool,
}

fn same_sweep_target(current: &ThreadRec, expected: &ThreadRec) -> bool {
    current.status == ThreadStatus::Resolved
        && current.machine_id == expected.machine_id
        && current.repo == expected.repo
        && current.cwd == expected.cwd
        && current.branch == expected.branch
        && current.leftover == expected.leftover
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

/// 이 host가 맡은 프로젝트(plan §8.283): 보관하지 않은(멈춘 것은 센다) 프로젝트가 없으면 `None`, 하나면 그 이름. 둘 이상이면
/// 그 이름들(시작 거부).
pub(crate) fn sole_project(projects: &[(String, ProjectStatus)]) -> Result<Option<String>, Vec<String>> {
    let open: Vec<String> =
        projects.iter().filter(|(_, s)| *s != ProjectStatus::Archived).map(|(n, _)| n.clone()).collect();
    match open.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one.clone())),
        _ => Err(open),
    }
}

/// 시작 거부 말: 무엇이 남았고 어떻게 하나로 줄이나
pub(crate) fn sole_project_refusal(names: &[String]) -> String {
    format!(
        "host는 프로젝트 하나만 맡는다(plan §8.283). 보관하지 않은 프로젝트가 {}개다: {}. 남길 하나를 빼고 \
         `aaa project archive <이름>`(기록은 남는다)이나 `aaa project delete <이름>`(.trash로 옮긴다)으로 줄인 뒤 다시 띄운다. \
         host가 멈춰 있으면 프로젝트 폴더를 <projects root>/.trash로 옮긴다",
        names.len(),
        names.join(", ")
    )
}

impl Host {
    /// 이 host의 프로젝트(내부 이름). 시작할 때 `run`이 보고, 둘 이상이면 시작하지 않는다.
    pub(crate) fn sole_project(&self) -> Result<Option<String>, Vec<String>> {
        let all: Vec<_> = self
            .project_directories()
            .into_iter()
            .map(|(name, _)| {
                let status = self.project_status(&name);
                (name, status)
            })
            .collect();
        sole_project(&all)
    }

    /// 프로젝트를 생략한 요청이 쓸 이름(plan §8.288): 이 host가 맡은 하나
    pub(crate) fn instance_project(&self) -> Result<String, ErrorBody> {
        match self.sole_project() {
            Ok(Some(project)) => Ok(project),
            Ok(None) => Err(err(
                ErrorCode::NotFound,
                // 인스턴스는 이름을 생략하면 인스턴스 이름으로 만든다(plan §8.294)
                match aaa_config::paths::instance() {
                    Some(name) => format!("이 host에는 아직 프로젝트가 없다: `aaa --instance {name} project create`로 만든다"),
                    None => {
                        "이 host에는 아직 프로젝트가 없다: `aaa project create <이름>`으로 만들거나 프로젝트 이름을 준다".into()
                    }
                },
            )),
            Err(names) => Err(err(ErrorCode::BadRequest, sole_project_refusal(&names))),
        }
    }

    /// `Instance` 보기(FRONT_API 1.50): 이 host가 맡은 프로젝트
    pub(crate) fn instance_view(&self) -> Option<ProjectView> {
        let project = self
            .sole_project()
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()?;
        self.project_dir(&project)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .map(|dir| self.project_view(&project, &dir))
    }

    pub(crate) fn project_status(&self, project: &str) -> ProjectStatus {
        match self
            .db
            .meta(&status_key(project))
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .flatten()
            .as_deref()
        {
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
                format!("`{project}`는 멈춰 있어 thread를 시작하지 않는다(`aaa project resume`)"),
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
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
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
            aaa_common::diagnostic::ignored(self.thread_events.send(v), "수신자가 끝난 뒤에도 현재 작업은 마친다");
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
        if current == ProjectStatus::Archived && to != ProjectStatus::Archived {
            match self.sole_project() {
                Ok(None) => {}
                Ok(Some(have)) => {
                    return Err(bad(format!(
                        "host는 프로젝트 하나만 맡는다: 이미 `{have}`가 있다. 먼저 보관한 뒤 `{project}`의 보관을 푼다"
                    )));
                }
                Err(names) => return Err(bad(sole_project_refusal(&names))),
            }
        }
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
                notes.push("coordinator는 `aaa coordinator open`으로 연다".to_string());
            }
            ProjectStatus::Active => {}
        }
        // front가 다시 묻지 않고 알게 그 프로젝트 thread 사건을 다시 낸다(`ThreadView.project_status`)
        for j in self.db.threads(Some(project)).unwrap_or_default() {
            if let Ok(r) = serde_json::from_str::<ThreadRec>(&j) {
                aaa_common::diagnostic::ignored(
                    self.thread_events.send(self.thread_view(&r)),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
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
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .collect();
        let trash = self.projects_root().join(".trash");
        let stamp = format!("{project}-{}", aaa_common::util::utc_compact(aaa_common::util::now_ms()));
        // 같은 초에 같은 이름을 다시 지우면(지우고 다시 만든 뒤) 이름이 겹친다: 뒤에 수를 붙인다
        let target = (1..)
            .map(|n| trash.join(if n == 1 { stamp.clone() } else { format!("{stamp}-{n}") }))
            .find(|t| !t.exists())
            .expect("빈 이름");
        std::fs::create_dir_all(&trash).map_err(internal)?;
        std::fs::rename(&dir, &target)
            .map_err(|e| internal(format!("{}를 휴지통으로 옮기지 못했다: {e}", dir.display())))?;
        let json = serde_json::to_vec_pretty(&records).map_err(internal)?;
        crate::orch::write_atomic(&target.join("aaa-records.json"), &json).map_err(internal)?;
        self.hide_threads(&recs);
        self.db.project_forget(project).map_err(internal)?;
        self.accounts_bump();
        // 같은 이름을 다시 만들어도 옛 앱 보기 경로가 새 thread 번호를 읽지 못한다.
        self.view_caps.lock().unwrap().retain(|_, cap| cap.project != project);
        // 작업 요약 표지도: 같은 이름의 새 프로젝트 t-0001이 옛 기억을 쓰지 않게(plan §8.259)
        self.summary_marks.lock().unwrap().retain(|(p, _), _| p != project);
        // 할 일 기억도: 같은 이름의 새 프로젝트 TASKS.md를 옛 글과 비교해 「파일 직접」 기록을 지어내지 않게(plan §8.311)
        self.tasks_seen.lock().unwrap().remove(project);
        self.db.audit(actor, "project_delete", &format!("{project} → {}", target.display()));
        // 끝나지 않은 thread의 감시를 푼다(pane은 그대로, 참고도 닫지 않는다)
        for r in recs.iter().filter(|r| r.status != ThreadStatus::Resolved) {
            if let Ok(conn) = self.live_conn(&r.machine_id) {
                aaa_common::diagnostic::ignored(
                    conn.request(AgentRequest::ThreadUnwatch { thread: format!("{project}/{}", r.id) }).await,
                    "보조 작업 실패 뒤 기존 처리를 계속한다",
                );
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
                auto_mode: self.auto_mode(project)?,
                name: project.to_string(),
                path: target.to_string_lossy().into_owned(),
                threads: recs.len() as u32,
                status: ProjectStatus::Archived,
                coordinator_profile: String::new(),
                coordinator_profiles: None,
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
        // aaa가 쓴 안내(표지 줄)면 새 이름으로 다시 쓴다. 옛 판 aaa의 안내도 표지가 있으면 지금 안내가 된다
        let stock = std::fs::read_to_string(&guide)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .is_some_and(|t| t.contains(crate::orch::GUIDE_MARK));
        let mut notes = vec![
            format!("폴더 {} → {}", dir.display(), new_dir.display()),
            "host 기록(thread·inbox·routine·평가)을 새 이름으로 옮긴다".into(),
        ];
        notes.push(if stock {
            "AGENTS.md(coordinator 안내)를 새 이름으로 다시 쓴다".into()
        } else {
            "AGENTS.md에 aaa 표지가 없어(옛 aaa나 사람이 쓴 것) 그대로 둔다: `aaa doctor files --fix`가 옆에 옮겨 두고 새로 쓴다".into()
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
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .collect();
        std::fs::rename(&dir, &new_dir).map_err(|e| internal(format!("폴더 이름을 바꾸지 못했다: {e}")))?;
        let moved = self.db.project_rename(project, to, |json| match serde_json::from_str::<serde_json::Value>(json) {
            Ok(mut v) => {
                v["project"] = to.into();
                v.to_string()
            }
            Err(source_error) => {
                aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                json.to_string()
            }
        });
        if let Err(e) = moved {
            // 기록을 못 옮겼으면 폴더도 되돌린다(기록과 폴더 이름이 어긋나지 않게)
            aaa_common::diagnostic::ignored(
                std::fs::rename(&new_dir, &dir),
                "정리 실패가 본 작업의 결과를 덮지 않게 한다",
            );
            return Err(internal(format!("host 기록을 옮기지 못해 되돌렸다: {e:#}")));
        }
        self.accounts_bump();
        for cap in self.view_caps.lock().unwrap().values_mut().filter(|cap| cap.project == project) {
            cap.project = to.to_string();
        }
        if stock {
            aaa_common::diagnostic::ignored(
                crate::orch::write_atomic(&new_dir.join("AGENTS.md"), crate::orch::coordinator_guide(to).as_bytes()),
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
        }
        self.db.audit(actor, "project_rename", &format!("{project} → {to}"));
        self.summary_marks.lock().unwrap().retain(|(p, _), _| p != project && p != to);
        // 할 일 기억은 새 이름으로 옮긴다(기록 `task_log`는 project 열째 옮겨졌다, plan §8.311)
        {
            let mut seen = self.tasks_seen.lock().unwrap();
            seen.remove(to);
            if let Some(s) = seen.remove(project) {
                seen.insert(to.to_string(), s);
            }
        }
        self.hide_threads(&old_recs);
        for j in self.db.threads(Some(to)).unwrap_or_default() {
            if let Ok(r) = serde_json::from_str::<ThreadRec>(&j) {
                aaa_common::diagnostic::ignored(
                    self.thread_events.send(self.thread_view(&r)),
                    "수신자가 끝난 뒤에도 현재 작업은 마친다",
                );
            }
        }
        self.tick_wake.notify_one();
        Ok(self.change(to, &new_dir, notes))
    }

    /// 모든 프로젝트(보관 포함)의 aaa 파일을 본다. `fix`면 고칠 것이 있는 프로젝트만 고친다(참고 `doctor [--fix]`)
    pub(crate) fn project_files(&self, fix: bool, actor: &str) -> FrontResponse {
        let checks: Vec<ProjectFilesCheck> = self
            .projects_all()
            .into_iter()
            .map(|p| {
                let dir = Path::new(&p.path);
                let mut problems = crate::orch::files_problems(dir, &p.name);
                if !crate::legacy_mission::files(dir).is_empty() {
                    problems.push(crate::legacy_mission::REMOVED.into());
                }
                let (fixed, error) = match fix && !problems.is_empty() {
                    false => (false, None),
                    true => match crate::orch::write_files(dir, &p.name) {
                        Ok(()) => (true, None),
                        Err(e) => (false, Some(e.to_string())),
                    },
                };
                ProjectFilesCheck { project: p.name, problems, fixed, error }
            })
            .collect();
        if fix {
            let fixed: Vec<&str> = checks.iter().filter(|c| c.fixed).map(|c| c.project.as_str()).collect();
            self.db.audit(actor, "project_files_fix", &fixed.join(" "));
        }
        FrontResponse::ProjectFiles { checks }
    }

    /// 같은 저장소의 없는 자리·branch를 한 번에 확인한다. 구판 agent에는 모르는 요청을 보내지 않는다.
    async fn sweep_absent(&self, found: &[ThreadRec]) -> std::collections::HashSet<String> {
        let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
        for r in found {
            if let (Some(repo), Some(_)) = (&r.repo, &r.cwd) {
                groups.entry((&r.machine_id, repo)).or_default().push(r);
            }
        }
        let mut missing = std::collections::HashSet::new();
        for ((machine, repo), rows) in groups {
            let supported = self
                .inner
                .lock()
                .unwrap()
                .machines
                .get(machine)
                .is_some_and(|m| m.features.iter().any(|f| f == "cleanup_probe.v1"));
            if !supported {
                continue;
            }
            let Ok(conn) = self.live_conn(machine) else {
                continue;
            };
            for chunk in rows.chunks(500) {
                let targets = chunk
                    .iter()
                    .map(|r| aaa_proto::CleanupTarget {
                        cwd: r.cwd.clone().unwrap_or_default(),
                        branch: r.branch.clone(),
                    })
                    .collect();
                match conn.request(AgentRequest::ThreadCleanupProbe { repo: repo.clone(), targets }).await {
                    Ok(AgentResponse::CleanupAbsent { absent }) if absent.len() == chunk.len() => {
                        missing.extend(chunk.iter().zip(absent).filter(|(_, gone)| *gone).map(|(r, _)| r.id.clone()));
                    }
                    result => tracing::warn!(?result, "자리 부재 묶음 확인을 못 해 기존 개별 정리로 넘긴다"),
                }
            }
        }
        missing
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
            .filter_map(|j| {
                serde_json::from_str::<ThreadRec>(j)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
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
        let started = std::time::Instant::now();
        let absent = self.sweep_absent(&found).await;
        let probe_ms = started.elapsed().as_millis();
        let mut agent_time = std::time::Duration::ZERO;
        let mut save_time = std::time::Duration::ZERO;
        for r in &found {
            let agent_started = std::time::Instant::now();
            let before = describe(r);
            match self.load(project, &r.id) {
                Ok(current) if same_sweep_target(&current, r) => {}
                result => {
                    kept += 1;
                    let reason = match result {
                        Err(e) => format!("thread 기록을 읽지 못했다: {}", e.message),
                        Ok(_) => "정리 중 thread 기록이 바뀌었다".into(),
                    };
                    notes.push(format!("남겼다: {before}({reason})"));
                    continue;
                }
            }
            let res = if absent.contains(&r.id) {
                Ok(aaa_proto::Cleanup {
                    worktree_removed: true,
                    branch_deleted: true,
                    notes: vec!["자리와 branch는 이미 없다".into()],
                })
            } else {
                match self.live_conn(&r.machine_id) {
                    Ok(conn) => conn
                        .request(AgentRequest::ThreadCleanup {
                            repo: r.repo.clone(),
                            cwd: r.cwd.clone().unwrap_or_default(),
                            branch: r.branch.clone(),
                            // branch는 PR이 병합됐고 로컬 끝이 병합된 head일 때만 지운다(참고 sweep `Orphan::Branch`, resolve와 같다)
                            merged_head: if r.repo.is_some() {
                                self.merged_head(project, r, &conn).await
                            } else {
                                None
                            },
                            keep_worktree: false,
                            uncopied: false,
                        })
                        .await
                        .and_then(|res| match res {
                            AgentResponse::Cleanup(c) => Ok(c),
                            other => Err(internal(format!("예상하지 못한 응답: {other:?}"))),
                        }),
                    Err(e) => Err(e),
                }
            };
            agent_time += agent_started.elapsed();
            let save_started = std::time::Instant::now();
            match res {
                Ok(c) => {
                    let l = Leftover { worktree: !c.worktree_removed, branch: r.branch.is_some() && !c.branch_deleted };
                    let clean = !l.worktree && !l.branch;
                    let mut unchanged = false;
                    let saved = self.update_rec(project, &r.id, |x| {
                        unchanged = same_sweep_target(x, r);
                        if unchanged {
                            x.leftover = Some(l);
                        }
                    });
                    if let Err(e) = saved {
                        kept += 1;
                        notes.push(format!("남겼다: {before}(정리 표지 저장 실패: {})", e.message));
                        continue;
                    }
                    if !unchanged {
                        kept += 1;
                        notes.push(format!("남겼다: {before}(정리 중 thread 기록이 바뀌었다)"));
                        continue;
                    }
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
            save_time += save_started.elapsed();
        }
        tracing::info!(
            total = found.len(),
            absent = absent.len(),
            elapsed_ms = started.elapsed().as_millis(),
            probe_ms,
            agent_ms = agent_time.as_millis(),
            save_ms = save_time.as_millis(),
            "프로젝트 자리 정리 완료"
        );
        notes.push(format!("`{project}` 청소: {done}개 치움, {kept}개 남김"));
        self.db.audit(actor, "project_sweep", &format!("{project} {done}/{}", found.len()));
        Ok(self.change(project, &dir, notes))
    }

    /// host.toml(·local)의 `[safety.<프로젝트>]` 표가 남는다는 안내(참고 delete의 마지막 줄). 표는 사람만 고친다(§8.163).
    fn safety_table_note(&self, project: &str, which: &str) -> Option<String> {
        let dir = self.config_dir.get()?;
        let files: Vec<&str> = ["host.toml", "host.local.toml"]
            .into_iter()
            .filter(|f| {
                std::fs::read_to_string(dir.join(f))
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .and_then(|t| {
                        t.parse::<toml::Table>()
                            .inspect_err(|error| {
                                aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                            })
                            .ok()
                    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, status: ProjectStatus) -> (String, ProjectStatus) {
        (name.to_string(), status)
    }

    #[test]
    fn sole_project_counts_everything_but_archived() {
        assert_eq!(sole_project(&[]), Ok(None));
        assert_eq!(sole_project(&[p("projects", ProjectStatus::Active)]), Ok(Some("projects".into())));
        // 보관한 것은 세지 않고, 멈춘 것은 센다(되살리면 다시 돈다)
        assert_eq!(
            sole_project(&[p("old", ProjectStatus::Archived), p("projects", ProjectStatus::Paused)]),
            Ok(Some("projects".into()))
        );
        assert_eq!(sole_project(&[p("old", ProjectStatus::Archived)]), Ok(None));
        assert_eq!(
            sole_project(&[
                p("m4-pilot", ProjectStatus::Active),
                p("old", ProjectStatus::Archived),
                p("projects", ProjectStatus::Paused)
            ]),
            Err(vec!["m4-pilot".to_string(), "projects".to_string()])
        );
    }

    /// 지우고 다시 만든 같은 이름을 같은 초에 또 지워도 휴지통 이름이 겹치지 않는다(host는 프로젝트 하나라 다시 만들기가 잦다)
    #[tokio::test]
    async fn deleting_the_same_name_twice_keeps_both_in_the_trash() {
        let state = std::env::temp_dir().join(format!("aaa-trash-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, state.clone()).unwrap();
        for _ in 0..2 {
            h.project_create("demo", Default::default(), "test").unwrap();
            h.project_delete("demo", false, "test").await.unwrap();
        }
        assert_eq!(std::fs::read_dir(state.join("projects/.trash")).unwrap().count(), 2);
        drop(h);
        std::fs::remove_dir_all(&state).unwrap();
    }

    /// 프로젝트를 생략한 요청(FRONT_API 1.50, plan §8.288): host가 맡은 하나로 채우고, 아직 없으면 그렇게 말한다
    #[tokio::test]
    async fn omitted_project_is_the_one_this_host_keeps() {
        use aaa_proto::{DeviceRole, FrontRequest};
        let state = std::env::temp_dir().join(format!("aaa-instance-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Arc::new(Host::open(cfg, state.clone()).unwrap());
        let omitted: FrontRequest = serde_json::from_str(r#"{"op":"context"}"#).unwrap();
        assert_eq!(omitted, FrontRequest::Context { project: String::new(), peek: false });
        let e = h.call(DeviceRole::Local, "test", omitted.clone()).await.unwrap_err();
        assert!(e.code == ErrorCode::NotFound && e.message.contains("아직 프로젝트가 없다"), "{e:?}");
        assert_eq!(
            h.call(DeviceRole::Local, "test", FrontRequest::Instance).await,
            Ok(FrontResponse::Instance { project: None })
        );
        h.project_create("demo", Default::default(), "test").unwrap();
        let Ok(FrontResponse::Context(c)) = h.call(DeviceRole::Local, "test", omitted).await else { panic!("context") };
        assert_eq!(c.project, "demo");
        let Ok(FrontResponse::Instance { project: Some(p) }) =
            h.call(DeviceRole::Local, "test", FrontRequest::Instance).await
        else {
            panic!("instance")
        };
        assert_eq!((p.name.as_str(), p.status), ("demo", ProjectStatus::Active));
        // 이름을 주면 그대로 쓴다(보관한 프로젝트·옛 front)
        let named = FrontRequest::Context { project: "other".into(), peek: true };
        assert_eq!(h.call(DeviceRole::Local, "test", named).await.unwrap_err().code, ErrorCode::NotFound);
        drop(h);
        std::fs::remove_dir_all(&state).unwrap();
    }

    #[test]
    fn refusal_names_what_to_reduce() {
        let m = sole_project_refusal(&["a".into(), "b".into()]);
        assert!(
            m.contains("2개다: a, b") && m.contains("aaa project archive") && m.contains("aaa project delete"),
            "{m}"
        );
    }
}

#[cfg(test)]
#[path = "lifecycle/sweep_tests.rs"]
mod sweep_tests;
