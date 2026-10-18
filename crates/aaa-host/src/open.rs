//! `aaa project open`(herdr-projects 0.2.35 `coordinator.rs` `open`, 차이 목록 4장 G 「coordinator 열기」, plan §8.166).
//!
//! - 도는 coordinator가 있으면 아무것도 만들지 않고 가장 최근에 상태가 바뀐 것을 알린다(`--new`면 하나 더). coordinator는
//!   aaa의 판정(host 머신에서 프로젝트 폴더를 작업 폴더로 도는 pane, [`Host::coordinator_panes`]) 중 agent가 있는 pane이다:
//!   참고도 agent 목록에서 그 폴더의 agent만 센다. 사람 셸이 그 폴더에 있어도 coordinator가 도는 것은 아니다.
//! - 띄우기: host 머신 agent가 profile을 그 머신 agent.toml로 풀어(§8.126 `SpawnSpec.profile`: hook 설정·`--session-id`) 프로젝트
//!   폴더에서 띄우고, 프로젝트 이름 workspace(없으면 만든다)의 새 탭 「coordinator」에 둔다(참고 `tab_create`·`workspace_create`).
//! - 참고와 다름: 셸 pane에서 치면 그 pane에서 띄우는 길(`run_here`)은 없다. aaa pane은 처음부터 그 명령으로 띄우고 셸 뒤에
//!   agent를 붙이지 않는다(§8.126이 참고의 「agent가 못 떴을 때 열기」를 뺀 것과 같은 까닭). 그래서 `--tab`이 없다. host가
//!   하나라 `--rebind`도 없다. 도는 coordinator에 focus를 옮기지 않는다(focus는 화면마다다): pane ID를 돌려준다.
//! - `--profile`을 주면 그 profile로 띄운 coordinator만 다시 쓰고, 아니면 옆에 하나 더 띄운다(참고: 기록한 coordinator가 그
//!   profile일 때만). 기록은 host DB meta `coordinator:<프로젝트>`(마지막에 띄운·알린 pane과 profile, 참고 `Coordinator` 기록)다.
//! - 대화 잇기는 open이 하지 않는다: profile로 띄운 pane은 SessionStart hook으로 대화 ID를 알고 재부팅 복원(§8.111)이 잇는다.
//! - safety(참고 `check_allowed`·`launch_args`·`Safety::launch_args`): profile은 `coordinator_profiles`에 있어야 하고(돌아도
//!   거부), 띄울 때 yolo면 harness의 권한 건너뛰기 인자를, 프로젝트 기본 coordinator profile과 같은 harness의 기본 profile이면
//!   `coordinator_agent_args`를 붙인다. agent가 profile을 풀므로 `SpawnSpec.safety`로 보낸다(`spawn_safety.v1`).

use std::sync::Arc;

use aaa_proto::{
    ErrorBody, ErrorCode, FrontResponse, LaunchSafety, LayoutNode, PaneView, ProjectOpened, ProjectStatus, Role,
    SpawnSpec, Tab,
};

use crate::{Host, err, util};

fn bad(msg: impl Into<String>) -> ErrorBody {
    err(ErrorCode::BadRequest, msg)
}

/// 마지막에 띄운(또는 알린) coordinator(참고 PROJECT 기록의 `Coordinator`에서 pane과 profile만)
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct Recorded {
    pane: String,
    /// 그 pane을 띄운 profile. 띄우지 않은 pane이면 그 harness(참고 「Another pane's profile is not known」)
    profile: String,
}

fn record_key(project: &str) -> String {
    format!("coordinator:{project}")
}

impl Host {
    /// 그 폴더에서 도는 coordinator 중 agent가 있는 pane: `project open`이 띄운 것(역할 coordinator, custom profile이어도),
    /// 띄울 때 알린 harness·실행 파일 이름이 Claude·Codex인 것, 감지한 agent가 있는 것
    fn running_coordinators(&self, dir: &std::path::Path) -> Vec<PaneView> {
        self.coordinator_panes(dir)
            .into_iter()
            .filter(|p| {
                let declared = self.inner.lock().unwrap().panes.get(&p.id).and_then(|x| x.info.harness.clone());
                p.role == Some(Role::Coordinator)
                    || p.agent.is_some()
                    || crate::adopt::harness_of(declared.as_deref(), &p.argv).is_some()
            })
            .collect()
    }

    pub(crate) async fn project_open(
        self: &Arc<Self>,
        project: &str,
        profile: Option<String>,
        new: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let dir = self.project_dir(project)?;
        if self.project_status(project) == ProjectStatus::Archived {
            return Err(bad(format!("`{project}`는 보관돼 있다: 먼저 `aaa project unarchive {project}`")));
        }
        let asked = profile.is_some();
        let profile = profile.unwrap_or_else(|| self.default_coordinator_profile(project));
        aaa_config::profile_edit::validate_name(&profile).map_err(bad)?;
        // 프로젝트 폴더는 host 머신에 있다
        let mid = self.machine_views().into_iter().find(|m| m.local).map(|m| m.id).ok_or_else(|| {
            err(ErrorCode::NotFound, "host 머신에 agent가 없다: coordinator는 프로젝트 폴더가 있는 host 머신에서 돈다")
        })?;
        // 참고처럼 profile부터 푼다: 없는 profile은 coordinator가 돌아도 거부한다(agent가 띄울 때 쓰는 규칙과 같다:
        // 그 머신 agent.toml, 없으면 기본 profile claude·codex)
        let known = match self.profiles(Some(&mid)).await? {
            FrontResponse::Profiles { profiles, .. } => profiles.into_iter().map(|p| p.name).collect::<Vec<_>>(),
            _ => Vec::new(),
        };
        if !known.contains(&profile) && aaa_config::profile_edit::builtin(&profile).is_none() {
            let from = if asked { "--profile" } else { "coordinator 기본 profile" };
            return Err(bad(format!(
                "{from} `{profile}`가 host 머신({}) agent.toml에 없다(있는 것: {}, 기본 profile: claude, codex). 사람이 `aaa profile add`로 더한다",
                self.machine_name(&mid),
                if known.is_empty() { "없음".to_string() } else { known.join(", ") }
            )));
        }
        let safety = self.safety(project);
        if !safety.coordinator_profile_allowed(&profile) {
            let list = safety.coordinator_profiles.clone().unwrap_or_default();
            return Err(err(
                ErrorCode::Forbidden,
                format!(
                    "profile `{profile}`는 `{project}`의 coordinator에 허용되지 않았다. 허용: {}. 이 목록은 사람만 바꾼다(`aaa profile allow coordinator … --project {project}`)",
                    if list.is_empty() { "없음".to_string() } else { list.join(", ") }
                ),
            ));
        }
        let canonical = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        let mut running = self.running_coordinators(&canonical);
        // 참고 `max_by_key(state_change_seq)`: 가장 최근에 상태가 바뀐 것
        running.sort_by_key(|p| std::cmp::Reverse(p.agent.as_ref().map_or(0, |a| a.since_ms)));
        let recorded: Option<Recorded> =
            self.db.meta(&record_key(project)).ok().flatten().and_then(|t| serde_json::from_str(&t).ok());
        // 기록한 pane(재부팅 복원·`pane resume`이 대신했으면 그 새 pane)
        let is_recorded =
            |p: &PaneView| recorded.as_ref().is_some_and(|r| p.id == r.pane || p.replaced.contains(&r.pane));
        let reusable: Vec<&PaneView> = running
            .iter()
            .filter(|p| !asked || (is_recorded(p) && recorded.as_ref().is_some_and(|r| r.profile == profile)))
            .collect();
        let view = self.project_view(project, &dir);
        if let Some(top) = reusable.first().filter(|_| !new) {
            let others = running.len() as u32 - 1;
            let workspace = self.workspace_of_pane(&top.id).unwrap_or_default();
            let used = match &recorded {
                Some(r) if is_recorded(top) => r.profile.clone(),
                _ => top.agent.as_ref().map(|a| a.harness.clone()).unwrap_or_default(),
            };
            self.record_coordinator(project, &top.id, &used);
            let mut notes = vec![format!(
                "coordinator가 pane {}에서 돈다{}. 하나 더 띄우려면 --new",
                top.id,
                if others > 0 { format!("(그 밖에 {others}개)") } else { String::new() }
            )];
            if !workspace.is_empty() {
                notes.push(format!("TUI: `aaa attach -w {workspace}`"));
            }
            return Ok(FrontResponse::ProjectOpened(Box::new(ProjectOpened {
                project: view,
                pane: (*top).clone(),
                started: false,
                others,
                profile: used,
                workspace,
                notes,
            })));
        }
        if !self.machine_feature(&mid, "profile.v1") {
            return Err(bad("이 머신의 agent가 profile로 띄우기를 모른다(agent binary 반영 필요)"));
        }
        // 사람이 켠 yolo·coordinator_agent_args가 빠진 채 뜨지 않게: 모르는 agent면 조용히 빼지 않고 거부한다
        let launch_safety = (safety.yolo || !safety.coordinator_agent_args.is_empty()).then(|| LaunchSafety {
            yolo: safety.yolo,
            thread_agent_args: safety.coordinator_agent_args.clone(),
            default_profile: self.default_coordinator_profile(project),
        });
        if launch_safety.is_some() && !self.machine_feature(&mid, "spawn_safety.v1") {
            return Err(bad(format!(
                "host 머신({}) agent가 오래돼 safety yolo·coordinator_agent_args를 coordinator에 붙이지 못한다(agent binary 반영 필요). 사람이 `aaa safety yolo {project} off`·`aaa safety set {project} coordinator_agent_args default`로 끌 수도 있다",
                self.machine_name(&mid)
            )));
        }
        let name = match running.len() {
            0 => format!("{project} coordinator"),
            n => format!("{project} coordinator {}", n + 1),
        };
        let spec = SpawnSpec {
            name,
            argv: vec![],
            cwd: Some(canonical.to_string_lossy().into_owned()),
            env: vec![],
            cols: 120,
            rows: 32,
            // agent가 hook 진행 안내를 넣는 세션이다(참고 `managed`: coordinator·thread pane)
            role: Some(Role::Coordinator),
            harness: None,
            profile: Some(profile.clone()),
            safety: launch_safety,
        };
        let id = self.spawn_on(&mid, spec, actor).await?;
        // spawn 사이에 다른 layout 변경이 있었을 수 있다: 최신을 읽어 붙인다
        let mut w = self.workspace(project);
        w.tabs.push(Tab {
            id: util::new_id(),
            name: "coordinator".into(),
            root: LayoutNode::Pane { pane: id.clone() },
            zoomed: false,
        });
        self.save_workspace(&w);
        self.record_coordinator(project, &id, &profile);
        self.db.audit(actor, "project_open", &format!("{project} {profile} {id}"));
        let pane = self
            .pane_view_of(&id, aaa_proto::DeviceRole::Local)
            .ok_or_else(|| err(ErrorCode::Internal, "pane 목록에 없다"))?;
        let mut notes = vec![format!(
            "`{profile}`로 coordinator를 띄웠다(pane {id}, workspace `{project}`의 탭 coordinator). AGENTS.md를 읽고 스스로 준비한다"
        )];
        if !running.is_empty() {
            notes.push(format!("그 밖에 도는 coordinator {}개", running.len()));
        }
        notes.push(format!("TUI: `aaa attach -w {project}`"));
        Ok(FrontResponse::ProjectOpened(Box::new(ProjectOpened {
            project: view,
            pane,
            started: true,
            others: running.len() as u32,
            profile,
            workspace: project.to_string(),
            notes,
        })))
    }

    fn record_coordinator(&self, project: &str, pane: &str, profile: &str) {
        let r = Recorded { pane: pane.to_string(), profile: profile.to_string() };
        if let Err(e) = self.db.set_meta(&record_key(project), &serde_json::to_string(&r).unwrap_or_default()) {
            tracing::warn!(%project, "coordinator 기록을 저장하지 못했다: {e}");
        }
    }
}
