//! 이전 기획 대화 기록과 main 문서 이력 열람.
use crate::orch::{bad, internal};
use crate::{Host, err};
use aaa_proto::{
    AgentRequest, AgentResponse, DesignBranchState, DesignBranchView, DeviceRole, ErrorBody, ErrorCode, FrontResponse,
    design_slug_ok,
};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, time::Duration};
/// host가 기억하는 기획 대화 하나(옛 기획 branch 기록과 같은 파일·꼴)
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Rec {
    pub repo: String,
    pub slug: String,
    pub title: String,
    pub state: DesignBranchState,
    pub by: String,
    pub created_ms: u64,
    pub updated_ms: u64,
    /// 이 대화가 마지막으로 main에 올린 commit
    pub head: Option<String>,
    /// 옛 기획 branch 칸(t-2002 전 기록): 읽기만 한다
    pub pushed: Option<String>,
    pub push_error: Option<String>,
    pub rest_error: Option<String>,
    /// `main`(t-2002 뒤 만든 대화), 옛 기록은 `created`·`local`·`remote`
    pub origin: String,
    pub push_tried_ms: u64,
    pub push_failures: u32,
    pub own_head: Option<String>,
    pub rest_tried_ms: u64,
    /// 저장소 머신의 main 기획 자리(대화 thread가 읽는다)
    pub cwd: Option<String>,
    /// 대화를 열 때의 main 판
    pub base: Option<String>,
    pub pushed_ms: u64,
    pub apply: Option<String>,
    pub apply_wait: Option<aaa_proto::DesignApplyWait>,
}

impl Rec {
    fn view(&self) -> DesignBranchView {
        DesignBranchView {
            repo: self.repo.clone(),
            slug: self.slug.clone(),
            title: self.title.clone(),
            // 쉼은 없어졌다: 옛 기록도 열린 대화로 보인다
            state: DesignBranchState::Open,
            by: self.by.clone(),
            created_ms: self.created_ms,
            updated_ms: self.updated_ms,
            head: self.head.clone(),
            pushed: self.pushed.clone(),
            base: self.base.clone(),
            pushed_ms: self.pushed_ms,
            ..Default::default()
        }
    }
}

fn repo_ok(repo: &str) -> bool {
    !repo.is_empty() && repo != "." && repo != ".." && !repo.contains(['/', '\\', '\0'])
}

impl Host {
    fn design_branch_dir(&self, project: &str, repo: &str) -> Result<PathBuf, ErrorBody> {
        if !repo_ok(repo) {
            return Err(bad("저장소 이름이 잘못됐다"));
        }
        self.project_dir(project)?;
        Ok(self.state.join("design-branches").join(project).join(repo))
    }

    pub(crate) fn design_branch_load(&self, project: &str, repo: &str, slug: &str) -> Result<Option<Rec>, ErrorBody> {
        if !design_slug_ok(slug) {
            return Err(bad("기획 대화 주제는 영문 소문자·숫자·`-`다(1~48자)"));
        }
        let path = self.design_branch_dir(project, repo)?.join(format!("{slug}.json"));
        match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map(Some).map_err(|e| internal(format!("{}: {e}", path.display())))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(internal(format!("{}: {e}", path.display()))),
        }
    }

    /// 그 저장소의 기획 대화 기록(만든 차례)
    pub(crate) fn design_branch_list(&self, project: &str, repo: &str) -> Vec<Rec> {
        let Ok(dir) = self.design_branch_dir(project, repo) else { return Vec::new() };
        let mut list: Vec<Rec> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let slug = name.strip_suffix(".json")?;
                self.design_branch_load(project, repo, slug)
                    .inspect_err(|e| tracing::warn!(project, repo, slug, "기획 대화 기록을 읽지 못했다: {}", e.message))
                    .ok()
                    .flatten()
            })
            .collect();
        list.sort_by_key(|r| (r.created_ms, r.slug.clone()));
        list
    }

    pub(crate) fn design_view(&self, project: &str, rec: &Rec) -> DesignBranchView {
        let mut view = rec.view();
        if let Ok(chat) = self.design_chat_load(project, &rec.repo, &rec.slug) {
            view.thread = chat.thread;
            view.pending = chat.pending.len() as u32;
            view.chat_error = chat.error;
            view.profile = chat.profile;
            view.events = chat.events;
            view.edited = chat.edited;
        }
        view
    }

    pub(crate) fn design_handoff_authorize(&self, role: DeviceRole, actor: &str) -> Result<(), ErrorBody> {
        if !role.can_control()
            || actor.starts_with("worker:")
            || actor.starts_with("evaluator:")
            || actor.starts_with("detached:")
            || self.coordinator_project_of_actor(actor).is_some()
        {
            return Err(err(ErrorCode::Forbidden, "구현 맡기기는 조작 권한이 있는 사람이 한다"));
        }
        if let Some((_, session)) = actor.strip_prefix("local:").and_then(|a| a.split_once('@'))
            && (session.ends_with("@unverified") || self.thread_of_pane(session).is_some())
        {
            return Err(err(ErrorCode::Forbidden, "thread 안에서는 구현을 맡길 수 없다"));
        }
        Ok(())
    }
    pub(crate) async fn design_log(&self, project: &str, repo: &str, path: &str) -> Result<FrontResponse, ErrorBody> {
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let path = crate::design::checked_path(&folder, path)?;
        let conn = self.design_conn(&mid)?;
        if !self.machine_feature(&mid, "design_log.v1") && !self.machine_feature(&mid, aaa_proto::DESIGN_MAIN_FEATURE) {
            return Err(err(ErrorCode::Unavailable, "기획 이력을 읽으려면 저장소 머신의 agent를 갱신한다"));
        }
        let req = AgentRequest::DesignLog { repo: target.path, folder, path, limit: aaa_proto::DESIGN_LOG_MAX };
        match conn.request_timeout(req, Duration::from_secs(120)).await? {
            AgentResponse::DesignLog { entries } => Ok(FrontResponse::DesignLog { entries }),
            _ => Err(internal("예상하지 못한 이력 응답")),
        }
    }

    /// 트리에 싣는 기획 대화(기록만, agent를 거치지 않는다)
    pub(crate) fn design_branch_views(&self, project: &str, repo: &str) -> Vec<DesignBranchView> {
        self.design_branch_list(project, repo).iter().map(|rec| self.design_view(project, rec)).collect()
    }
}
