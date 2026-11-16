//! 기획 branch(t-1894 2단계, FRONT_API 1.171): 기획서는 main에 바로 쓰지 않고 `design/<slug>` branch에서 고친다(사용자 결정
//! 11-16). host는 branch마다 기록(host 상태 폴더 `design-branches/<프로젝트>/<저장소>/<slug>.json`: agent가 고칠 수 있는
//! 프로젝트 폴더 밖)을 두고 저장 차례를 지킨다. 쓰기는 그 저장소 머신 agent(`design_write.v1`)가 전용 worktree에 한다.
//!
//! - 저장은 기준 blob을 주고(새 문서는 없음) 그 문서 하나만 commit한다. 쓴 뒤 `[projects] design_push_secs`(기본 60초) 더 쓰지
//!   않으면 origin에 묶어서 올린다(force 없음, host.toml `[projects.design_check]`의 그 저장소 검사를 먼저). host가 다시 떠서
//!   놓친 올리기와 실패한 올리기(조금 뒤 다시)는 ticker가 잇는다.
//! - 버리기는 worktree와 로컬 branch를 지운다. origin branch는 host가 만든 branch일 때, host가 올린 그 판일 때만 지운다(남이
//!   그 위로 올렸으면 멈춘다). 있던 로컬 branch를 꺼낸 것은 로컬 branch를 남긴다.
//! - `[projects] design_rest_secs`(기본 14일) 쓰지 않은 branch는 「쉼」이다: worktree를 force 없이 돌려주고 branch는 남긴다
//!   (결정 3A). 다시 열면 이어 쓴다. 지우기는 사람이 「버리기」로 한다.
//! - 쓰는 이는 2단계 앞부분에서는 사람(조작 권한)뿐이다. aaa 세션(thread·coordinator)은 쓰지 않는다: 기획 대화 thread가 뒤에 연다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{
    AgentRequest, AgentResponse, DesignBranchState, DesignBranchView, DeviceRole, ErrorBody, ErrorCode, FrontResponse,
    design_slug_ok,
};
use serde::{Deserialize, Serialize};

use crate::orch::{bad, internal};
use crate::{Host, err};

pub(crate) const WRITE_FEATURE: &str = "design_write.v1";
/// 기획 branch에 그림·영상을 조각으로 넘기는 agent 기능(t-1894 2-3)
pub(crate) const ATTACH_FEATURE: &str = "design_attach.v1";
/// worktree 만들기(origin 받기 포함)·commit·내리기를 기다리는 상한
const WRITE_WAIT: Duration = Duration::from_secs(120);
/// 올리기(agent의 검사 300초 + 올리기 60초보다 길게)
const PUSH_WAIT: Duration = Duration::from_secs(420);
/// 실패한 올리기를 다시 하기까지(쉼 창의 10배, 적어도 10분, 거듭 실패하면 두 배씩 최대 64배)
fn push_retry_ms(push_ms: u64, failures: u32) -> u64 {
    push_ms.saturating_mul(10).max(600_000).saturating_mul(1 << failures.saturating_sub(1).min(6))
}
/// 쉼으로 내리지 못한 branch를 다시 해 보기까지(하루)
const REST_RETRY_MS: u64 = 86_400_000;
/// 주제·요약 글자 수 상한
const TITLE_MAX: usize = 80;
const SUMMARY_MAX: usize = 200;

/// host가 기억하는 기획 branch 하나
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
    pub head: Option<String>,
    pub pushed: Option<String>,
    pub push_error: Option<String>,
    /// 쉼으로 내리지 못한 까닭(다음 저장·열기까지 다시 하지 않는다)
    pub rest_error: Option<String>,
    /// agent가 처음 연 방법: `created`(host가 main에서 만듦)·`local`(있던 로컬 branch)·`remote`(origin branch에서 꺼냄)
    pub origin: String,
    /// 마지막으로 올리려 한 때와 이어진 실패 수(실패한 올리기를 다시 하는 간격)
    pub push_tried_ms: u64,
    pub push_failures: u32,
    /// host가 저장해 만든 마지막 commit. 끝이 이것일 때만 올리고, origin이 이 판일 때만 지운다(남의 commit을 올리거나 지우지 않는다)
    pub own_head: Option<String>,
    /// 마지막으로 쉼으로 내리려 한 때
    pub rest_tried_ms: u64,
    /// 저장소 머신의 기획 worktree 자리(대화 thread가 읽는다)
    pub cwd: Option<String>,
    /// 갈라진 main 판
    pub base: Option<String>,
    /// 마지막으로 올린 때
    pub pushed_ms: u64,
    /// 「main에 반영」을 맡은 thread
    pub apply: Option<String>,
}

impl Rec {
    fn view(&self) -> DesignBranchView {
        DesignBranchView {
            repo: self.repo.clone(),
            slug: self.slug.clone(),
            title: self.title.clone(),
            state: self.state,
            by: self.by.clone(),
            created_ms: self.created_ms,
            updated_ms: self.updated_ms,
            head: self.head.clone(),
            pushed: self.pushed.clone(),
            push_error: self.push_error.clone(),
            base: self.base.clone(),
            pushed_ms: self.pushed_ms,
            apply: self.apply.clone(),
            ..Default::default()
        }
    }
}

/// 한 줄로 줄인 글(줄바꿈·앞뒤 빈칸을 빼고 상한까지)
fn one_line(text: &str, max: usize) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// 저장소 이름은 기록 폴더 이름이 된다: 경로 조각이 아닌 이름만
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
            return Err(bad("기획 branch 이름은 영문 소문자·숫자·`-`다(1~48자)"));
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

    fn design_branch_store(&self, project: &str, rec: &Rec) -> Result<(), ErrorBody> {
        let dir = self.design_branch_dir(project, &rec.repo)?;
        std::fs::create_dir_all(&dir).map_err(|e| internal(format!("{}: {e}", dir.display())))?;
        let bytes = serde_json::to_vec_pretty(rec).map_err(internal)?;
        crate::orch::write_atomic(&dir.join(format!("{}.json", rec.slug)), &bytes).map_err(internal)
    }

    /// 그 저장소의 기획 branch 기록(만든 차례)
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
                    .inspect_err(|e| {
                        tracing::warn!(project, repo, slug, "기획 branch 기록을 읽지 못했다: {}", e.message)
                    })
                    .ok()
                    .flatten()
            })
            .collect();
        list.sort_by_key(|r| (r.created_ms, r.slug.clone()));
        list
    }

    pub(crate) fn design_branch_lock(&self, project: &str, repo: &str, slug: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.thread_op_lock(project, &format!("design:{repo}/{slug}"))
    }

    /// 쓰기 연결(design_write.v1)
    pub(crate) fn design_write_conn(&self, mid: &str) -> Result<crate::AgentConn, ErrorBody> {
        let conn = self.live_conn(mid)?;
        if !self.machine_feature(mid, WRITE_FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                "그 머신 agent가 기획 branch 쓰기를 아직 모른다(agent binary를 바꿔야 한다)",
            ));
        }
        Ok(conn)
    }

    /// 기록과 대화를 합친 보기
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

    /// 누가 쓰나: 조작 권한의 사람(aaa 밖 터미널·사람의 셸 pane·웹)과, `branch`(저장소, 주제)를 주면 그 프로젝트 그 기획
    /// branch 대화의 지금 thread(보기 권한·다른 머신 중계여도). 다른 thread·버린 대화에서 남은 thread·coordinator 세션과,
    /// 알린 세션만으로 정한 thread 범위(`worker:<세션>@unverified`)는 쓰지 않는다
    pub(crate) fn design_writer(
        &self,
        role: DeviceRole,
        actor: &str,
        project: &str,
        branch: Option<(&str, &str)>,
    ) -> Result<String, ErrorBody> {
        let session = actor
            .strip_prefix("worker:")
            .or_else(|| actor.strip_prefix("local:").and_then(|rest| rest.split_once('@')).map(|(_, s)| s));
        if let Some(session) = session {
            if let Some(rec) = self.thread_of_pane(session) {
                if let Some((repo, slug)) = branch
                    && rec.project == project
                    && rec.design.as_deref() == Some(format!("{repo}/{slug}").as_str())
                    && self
                        .design_chat_load(project, repo, slug)
                        .is_ok_and(|c| c.thread.as_deref() == Some(rec.id.as_str()))
                {
                    return Ok(rec.id);
                }
                return Err(err(
                    ErrorCode::Forbidden,
                    "thread 세션은 기획 branch에 쓰지 않는다(기획 대화 thread는 자기 branch에만 저장한다)",
                ));
            }
            if session.ends_with("@unverified") {
                return Err(err(
                    ErrorCode::Forbidden,
                    "이 CLI를 그 thread pane 안의 프로세스로 확인하지 못했다(알린 세션 ID만으로는 thread로 쓰지 않는다)",
                ));
            }
            if actor.starts_with("worker:")
                || self.pane_view_of(session, DeviceRole::Local).and_then(|p| p.role)
                    == Some(aaa_proto::Role::Coordinator)
            {
                return Err(err(ErrorCode::Forbidden, "thread·coordinator 세션 안에서는 기획 branch에 쓰지 않는다"));
            }
        }
        if actor.starts_with("evaluator:") || actor.starts_with("detached:") {
            return Err(err(ErrorCode::Forbidden, "이 세션은 기획 branch에 쓰지 않는다"));
        }
        if !role.can_control() {
            return Err(err(
                ErrorCode::Forbidden,
                format!("보기 권한으로는 기획을 고칠 수 없다{}", crate::front::reader_reason(actor)),
            ));
        }
        Ok(self.idea_actor_label(actor))
    }

    /// 기획 branch를 연다: 없으면 origin 기본 branch에서 만들고, 쉬던 것은 다시 꺼낸다
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_branch_open(
        &self,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        title: Option<String>,
        profile: Option<String>,
    ) -> Result<FrontResponse, ErrorBody> {
        self.design_branch_open_with(role, actor, project, repo, slug, title, profile, true).await
    }

    /// `create`가 아니면 host 기록이 있는 branch만 다시 연다(대화 배경 일: 그 사이 버린 branch를 되살리지 않는다)
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_branch_open_with(
        &self,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        title: Option<String>,
        profile: Option<String>,
        create: bool,
    ) -> Result<FrontResponse, ErrorBody> {
        let by = self.design_writer(role, actor, project, None)?;
        if let Some(p) = profile.as_deref() {
            self.check_profile_allowed(project, p)?;
        }
        let (target, mid) = self.design_target(project, repo)?;
        let lock = self.design_branch_lock(project, &target.name, slug);
        let _one = lock.lock().await;
        let old = self.design_branch_load(project, &target.name, slug)?;
        if old.is_none() && !create {
            return Err(err(ErrorCode::NotFound, format!("기획 branch가 없다(버렸다): design/{slug}")));
        }
        let conn = self.design_write_conn(&mid)?;
        if old.is_none() {
            self.design_chat_forget(project, &target.name, slug).await?;
        }
        let req = AgentRequest::DesignOpen {
            project: project.to_string(),
            repo: target.path.clone(),
            slug: slug.to_string(),
        };
        let AgentResponse::DesignBranch { head, opened, cwd, .. } = conn.request_timeout(req, WRITE_WAIT).await? else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        let now = aaa_common::util::now_ms();
        let fresh = old.is_none();
        // host가 main에서 새로 만든 branch는 처음 끝이 갈라진 main 판이다(agent의 `base`는 ref 이름이다)
        let start = (opened == "created").then(|| head.clone());
        let rec = match old {
            Some(mut rec) => {
                rec.state = DesignBranchState::Open;
                rec.updated_ms = now;
                rec.head = Some(head);
                rec.rest_error = None;
                rec.push_error = None;
                rec.cwd = Some(cwd);
                rec
            }
            None => Rec {
                repo: target.name.clone(),
                slug: slug.to_string(),
                title: title
                    .map(|t| one_line(&t, TITLE_MAX))
                    .filter(|t| !t.is_empty())
                    .unwrap_or_else(|| slug.to_string()),
                state: DesignBranchState::Open,
                by,
                created_ms: now,
                updated_ms: now,
                // origin에서 꺼낸 branch는 그 판이 이미 origin에 있다
                pushed: (opened == "remote").then(|| head.clone()),
                head: Some(head),
                cwd: Some(cwd),
                base: start.clone(),
                origin: if opened == "open" { "local".into() } else { opened },
                ..Default::default()
            },
        };
        self.design_branch_store(project, &rec)?;
        if fresh {
            self.design_chat_event(project, &target.name, slug, "open", "", start).await;
        }
        if profile.is_some() {
            self.design_chat_edit(project, &target.name, slug, |c| c.profile = profile).await?;
        }
        self.db.audit(actor, "design_branch_open", &format!("{project} {}/{slug}", target.name));
        Ok(FrontResponse::DesignBranch { branch: self.design_view(project, &rec) })
    }

    /// 기획 branch에 문서 하나를 저장한다(그 문서만 commit). 쉼 뒤 origin에 올린다
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_save(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        path: &str,
        base: Option<String>,
        text: String,
        summary: Option<String>,
    ) -> Result<FrontResponse, ErrorBody> {
        self.design_save_event(role, actor, project, repo, slug, path, base, text, summary, None).await
    }

    /// `design_save`와 같고, `event`를 주면 대화에 끼우는 사건을 그것으로 적는다(구현 맡기기의 「구현 맡김」)
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_save_event(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        path: &str,
        base: Option<String>,
        text: String,
        summary: Option<String>,
        event: Option<(&'static str, String)>,
    ) -> Result<FrontResponse, ErrorBody> {
        // 저장소 이름은 그대로 찾으므로(design_target) 대화 thread의 `<저장소>/<주제>`와 바로 견준다
        let by = self.design_writer(role, actor, project, Some((repo, slug)))?;
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let path = crate::design::checked_path(&folder, path)?;
        if base.as_deref().is_some_and(|b| !aaa_common::git_watch::valid_sha(b)) {
            return Err(bad("기준 판은 blob SHA다"));
        }
        let lock = self.design_branch_lock(project, &target.name, slug);
        let _one = lock.lock().await;
        let mut rec = self
            .design_branch_load(project, &target.name, slug)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{slug}(먼저 연다)")))?;
        if rec.state == DesignBranchState::Rest {
            return Err(err(ErrorCode::Conflict, format!("쉬는 기획 branch다: design/{slug}(다시 연 뒤 쓴다)")));
        }
        let summary = summary
            .map(|s| one_line(&s, SUMMARY_MAX))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| if base.is_some() { format!("{path} 고침") } else { format!("{path} 새로") });
        let conn = self.design_write_conn(&mid)?;
        let req_base_none = base.is_none();
        let req = AgentRequest::DesignSave {
            project: project.to_string(),
            repo: target.path.clone(),
            folder,
            slug: slug.to_string(),
            path: path.clone(),
            base,
            text,
            message: format!("기획: {summary} ({by})"),
        };
        let created = req_base_none;
        let AgentResponse::DesignSaved { commit, blob } = conn.request_timeout(req, WRITE_WAIT).await? else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        // 대화에 끼우는 사건: 대화 thread의 저장인가, 사람이 고친 것인가(사람이 고친 것은 다음 말 앞에 알린다)
        let by_thread =
            self.design_chat_load(project, &target.name, slug).ok().and_then(|c| c.thread) == Some(by.clone());
        let kind = match (by_thread, created) {
            (true, true) => "create",
            (true, false) => "save",
            (false, _) => "edit",
        };
        let (kind, text) = event.unwrap_or((kind, path.clone()));
        self.design_chat_event(project, &target.name, slug, kind, &text, Some(commit.clone())).await;
        if !by_thread {
            let doc = path.clone();
            aaa_common::diagnostic::ignored(
                self.design_chat_edit(project, &target.name, slug, |c| {
                    if !c.edited.contains(&doc) {
                        c.edited.push(doc);
                    }
                })
                .await,
                "다음 말 앞 알림만 빠진다",
            );
        }
        rec.updated_ms = aaa_common::util::now_ms();
        rec.head = Some(commit.clone());
        rec.own_head = Some(commit.clone());
        rec.push_error = None;
        rec.push_failures = 0;
        rec.rest_error = None;
        self.design_branch_store(project, &rec)?;
        self.db.audit(actor, "design_save", &format!("{project} {}/{slug} {path} {commit}", target.name));
        self.design_push_later(project, &target.name, slug);
        Ok(FrontResponse::DesignSaved { path, commit, blob })
    }

    /// 그림·영상 붙이기(웹 「그림」): 내용 hash 이름(`<문서 폴더>/<sha256 앞 16자>.<확장자>`)으로 기획 branch에 새 파일로 commit한다.
    /// 그 머신 agent에 조각(`ATTACH_CHUNK`)으로 넘긴다(연결 frame 1MiB). 사람이 고친 것처럼 다음 말 앞에 알리고 쉼 뒤 올린다
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_attach(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        dir: &str,
        bytes: &[u8],
    ) -> Result<FrontResponse, ErrorBody> {
        let by = self.design_writer(role, actor, project, None)?;
        if bytes.is_empty() || bytes.len() as u64 > aaa_proto::DESIGN_MAX_FILE {
            return Err(bad("파일은 1바이트부터 16MiB까지 붙인다"));
        }
        let ext =
            crate::ideas::media_ext(bytes).ok_or_else(|| bad("그림(PNG·JPEG·GIF·WebP)·영상(MP4·WebM)만 붙인다"))?;
        use sha2::Digest as _;
        let sha256 = hex::encode(sha2::Sha256::digest(bytes));
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let dir = dir.trim_matches('/');
        let path = crate::design::checked_path(&folder, &format!("{dir}/{}.{ext}", &sha256[..16]))?;
        let conn = self.design_write_conn(&mid)?;
        if !self.machine_feature(&mid, ATTACH_FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                format!(
                    "{} agent가 기획 그림 붙이기를 모른다({ATTACH_FEATURE} 없음). 그 머신 aaa를 갱신한다",
                    self.machine_name(&mid)
                ),
            ));
        }
        let lock = self.design_branch_lock(project, &target.name, slug);
        let _one = lock.lock().await;
        let mut rec = self
            .design_branch_load(project, &target.name, slug)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{slug}(먼저 연다)")))?;
        if rec.state == DesignBranchState::Rest {
            return Err(err(ErrorCode::Conflict, format!("쉬는 기획 branch다: design/{slug}(다시 연 뒤 쓴다)")));
        }
        let file = path.rsplit('/').next().unwrap_or(&path).to_string();
        let mut saved = None;
        for (i, chunk) in bytes.chunks(aaa_proto::ATTACH_CHUNK).enumerate() {
            let offset = i * aaa_proto::ATTACH_CHUNK;
            let req = AgentRequest::DesignAttach {
                project: project.to_string(),
                repo: target.path.clone(),
                folder: folder.clone(),
                slug: slug.to_string(),
                path: path.clone(),
                offset: offset as u64,
                data: aaa_proto::B64(chunk.to_vec()),
                eof: offset + chunk.len() == bytes.len(),
                sha256: sha256.clone(),
                message: format!("기획: 그림 {file} 붙임 ({by})"),
            };
            if let AgentResponse::DesignSaved { commit, blob } = conn.request_timeout(req, WRITE_WAIT).await? {
                saved = Some((commit, blob));
            }
        }
        let (commit, blob) = saved.ok_or_else(|| err(ErrorCode::Internal, "그림 저장 응답이 없다"))?;
        rec.updated_ms = aaa_common::util::now_ms();
        rec.head = Some(commit.clone());
        rec.own_head = Some(commit.clone());
        rec.push_error = None;
        rec.push_failures = 0;
        rec.rest_error = None;
        self.design_branch_store(project, &rec)?;
        self.design_chat_event(project, &target.name, slug, "edit", &path, Some(commit.clone())).await;
        let told = path.clone();
        aaa_common::diagnostic::ignored(
            self.design_chat_edit(project, &target.name, slug, |c| {
                if !c.edited.contains(&told) {
                    c.edited.push(told);
                }
            })
            .await,
            "다음 말 앞 알림만 빠진다",
        );
        self.db.audit(
            actor,
            "design_attach",
            &format!("{project} {}/{slug} {path} {commit} {} bytes", target.name, bytes.len()),
        );
        self.design_push_later(project, &target.name, slug);
        Ok(FrontResponse::DesignSaved { path, commit, blob })
    }

    /// 「main에 반영」: 올리지 않은 저장을 먼저 올리고, `origin/design/<주제>`에서 갈라진 worktree thread를 띄운다. 그 thread가
    /// main을 합치고 저장소 검사를 지나 `[병합 준비됨]`으로 착지 창을 받는다
    pub(crate) async fn design_apply(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.design_writer(role, actor, project, None)?;
        let (target, _) = self.design_target(project, repo)?;
        self.design_push_now(project, &target.name, slug).await;
        let lock = self.design_branch_lock(project, &target.name, slug);
        let _one = lock.lock().await;
        let mut rec = self
            .design_branch_load(project, &target.name, slug)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{slug}")))?;
        if rec.head.is_none() || rec.head != rec.pushed {
            return Err(err(
                ErrorCode::Conflict,
                match rec.push_error.as_deref() {
                    Some(e) => format!("기획 branch를 origin에 올리지 못했다: {e}"),
                    // 올린 뒤 잠금을 잡기 전에 새 저장이 들어왔다
                    None => "올리는 사이에 새 저장이 있었다: 다시 누른다".to_string(),
                },
            ));
        }
        if let Some(id) = rec.apply.as_deref()
            && self
                .load(project, id)
                .is_ok_and(|t| !matches!(t.status, aaa_proto::ThreadStatus::Resolved | aaa_proto::ThreadStatus::Failed))
        {
            return Err(err(ErrorCode::Conflict, format!("이미 반영 중이다: {id}")));
        }
        let folder = target.folder.clone().unwrap_or_default();
        let profile =
            self.design_chat_load(project, &target.name, slug).ok().and_then(|c| c.profile).unwrap_or_default();
        let args = crate::orch::StartArgs {
            project: project.to_string(),
            title: format!("main에 반영 {} {}", target.name, rec.title),
            machine: target.machine.clone(),
            profile,
            kind: aaa_proto::ThreadKind::Worktree,
            repo: Some(target.path.clone()),
            base: Some(format!("origin/design/{slug}")),
            task: format!(
                "기획 branch `design/{slug}`(주제 「{}」)의 기획 문서를 main에 반영한다. 이 worktree는 그 branch에서 갈라졌다.\n\n\
                 - origin main을 받아 합친다. 겹치면 양쪽 내용을 살려 고친다.\n\
                 - 기획 폴더(`{folder}/`) 밖은 바꾸지 않는다. 문서 표기(`[!계획]`·`[!미정]`·「바뀐 것」)를 지킨다.\n\
                 - 저장소 검사를 돌려 통과시킨 뒤 `[병합 준비됨]`으로 보고하고 착지 창을 기다린다.\n",
                rec.title
            ),
            role: aaa_proto::Role::Worker,
            ..Default::default()
        };
        // thread 시작 future는 크다: 공용 dispatcher(`call_design`)의 스택에 얹지 않게 heap에 둔다
        let view = Box::pin(self.thread_start_mode(args, actor, true)).await?;
        rec.apply = Some(view.id.clone());
        self.design_branch_store(project, &rec)?;
        self.design_chat_event(project, &target.name, slug, "apply", &view.id, rec.head.clone()).await;
        self.db.audit(actor, "design_apply", &format!("{project} {}/{slug} {}", target.name, view.id));
        Ok(FrontResponse::DesignBranch { branch: self.design_view(project, &rec) })
    }

    /// 기획 branch를 버린다: worktree·로컬 branch와 host가 올린 origin branch를 지우고 기록을 지운다
    pub(crate) async fn design_discard(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.design_writer(role, actor, project, None)?;
        let (target, mid) = self.design_target(project, repo)?;
        // 잠금을 끝까지 쥔다: 그 사이 말하기·저장·대화의 다시 열기는 기다렸다가 「없다」가 된다
        let lock = self.design_branch_lock(project, &target.name, slug);
        let _one = lock.lock().await;
        let Some(rec) = self.design_branch_load(project, &target.name, slug)? else {
            // 앞선 버리기가 branch는 지웠는데 대화 thread를 끝내지 못했으면 마저 끝낸다
            if self.design_chat_load(project, &target.name, slug)?.thread.is_some() {
                self.design_chat_retire(project, &target.name, slug, actor).await?;
                return Ok(FrontResponse::Done);
            }
            return Err(err(ErrorCode::NotFound, format!("host가 만든 기획 branch가 아니다: design/{slug}")));
        };
        let conn = self.design_write_conn(&mid)?;
        // origin branch는 host가 만들어 올린 것만, 그 판일 때만 지운다. 있던 로컬 branch는 남긴다
        let req = AgentRequest::DesignClose {
            project: project.to_string(),
            repo: target.path.clone(),
            slug: slug.to_string(),
            discard: true,
            keep_local: rec.origin == "local",
            // 올리기는 끝이 host가 저장한 commit일 때만 하므로 올린 판은 늘 host의 commit이다. 그 뒤 더 저장했어도(올리기 전)
            // origin에 있는 것은 그 판이고, 남이 그 위로 올렸으면 agent가 lease로 멈춘다
            remote: rec.pushed.clone().filter(|_| rec.origin == "created"),
        };
        let AgentResponse::Done = conn.request_timeout(req, WRITE_WAIT).await? else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        let path = self.design_branch_dir(project, &target.name)?.join(format!("{slug}.json"));
        std::fs::remove_file(&path).map_err(|e| internal(format!("{}: {e}", path.display())))?;
        self.design_push_gen.lock().unwrap().remove(&format!("{project}/{}/{slug}", target.name));
        self.db.audit(actor, "design_discard", &format!("{project} {}/{slug}", target.name));
        // branch를 지운 뒤에 대화를 끝낸다(닫기가 멈추면 대화는 그대로 남는다)
        self.design_chat_retire(project, &target.name, slug, actor).await.map_err(|e| {
            err(
                e.code,
                format!("기획 branch는 버렸으나 대화 thread를 끝내지 못했다(다시 버리면 마저 끝낸다): {}", e.message),
            )
        })?;
        Ok(FrontResponse::Done)
    }

    /// 저장 뒤 쉼(`design_push_secs`)이 지나도록 다음 저장이 없으면 origin에 올린다
    fn design_push_later(self: &Arc<Self>, project: &str, repo: &str, slug: &str) {
        let key = format!("{project}/{repo}/{slug}");
        let generation = {
            let mut map = self.design_push_gen.lock().unwrap();
            let g = map.entry(key.clone()).or_default();
            *g += 1;
            *g
        };
        let wait = Duration::from_secs(self.cfg.projects.design_push_secs);
        let host = self.clone();
        let (project, repo, slug) = (project.to_string(), repo.to_string(), slug.to_string());
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            if host.design_push_gen.lock().unwrap().get(&key) != Some(&generation) {
                return;
            }
            host.design_push_now(&project, &repo, &slug).await;
            // 끝난 차례는 지운다(남아 있으면 ticker가 놓친 올리기를 잇지 않는다). 그 사이 새 저장이 있으면 그 차례가 지운다
            let mut map = host.design_push_gen.lock().unwrap();
            if map.get(&key) == Some(&generation) {
                map.remove(&key);
            }
        });
    }

    /// 올리지 않은 commit이 있으면 origin에 올리고 결과를 기록한다
    pub(crate) async fn design_push_now(&self, project: &str, repo: &str, slug: &str) {
        let lock = self.design_branch_lock(project, repo, slug);
        let _one = lock.lock().await;
        let Ok(Some(mut rec)) = self.design_branch_load(project, repo, slug) else { return };
        // host가 저장한 commit이 끝일 때만 올린다: 꺼내 온 남의 commit·저장 없는 branch는 올리지 않는다
        if rec.head.is_none()
            || rec.head == rec.pushed
            || rec.head != rec.own_head
            || rec.state != DesignBranchState::Open
        {
            return;
        }
        rec.push_tried_ms = aaa_common::util::now_ms();
        let result = async {
            let (target, mid) = self.design_target(project, repo)?;
            let conn = self.design_write_conn(&mid)?;
            let req = AgentRequest::DesignPush {
                project: project.to_string(),
                repo: target.path,
                slug: slug.to_string(),
                check: target.check,
            };
            match conn.request_timeout(req, PUSH_WAIT).await? {
                AgentResponse::DesignPushed { head } => Ok(head),
                _ => Err(err(ErrorCode::Internal, "예상하지 못한 응답")),
            }
        }
        .await;
        match result {
            Ok(head) => {
                self.design_chat_event(project, repo, slug, "push", "", Some(head.clone())).await;
                rec.pushed = Some(head);
                rec.pushed_ms = aaa_common::util::now_ms();
                rec.push_error = None;
                rec.push_failures = 0;
            }
            Err(e) => {
                tracing::warn!(project, repo, slug, "기획 branch를 올리지 못했다: {}", e.message);
                rec.push_error = Some(e.message);
                rec.push_failures = rec.push_failures.saturating_add(1);
            }
        }
        if let Err(e) = self.design_branch_store(project, &rec) {
            tracing::warn!(project, repo, slug, "기획 branch 기록을 쓰지 못했다: {}", e.message);
        }
    }

    /// ticker: 기획 branch 일(올리기·쉼)을 따로 띄운다(agent를 기다리느라 ticker를 막지 않게, 한 프로젝트에 한 번에 하나)
    pub(crate) fn design_rest_spawn(self: &Arc<Self>, project: &str) {
        if !self.design_passes.lock().unwrap().insert(project.to_string()) {
            return;
        }
        let host = self.clone();
        let project = project.to_string();
        tokio::spawn(async move {
            host.design_rest_pass(&project).await;
            host.design_passes.lock().unwrap().remove(&project);
        });
    }

    /// 놓친·실패한 올리기를 잇고, 오래 쓰지 않은 기획 branch를 쉼으로 둔다(worktree를 돌려준다)
    pub(crate) async fn design_rest_pass(self: &Arc<Self>, project: &str) {
        let Ok(targets) = self.design_targets(project) else { return };
        let now = aaa_common::util::now_ms();
        let push_ms = self.cfg.projects.design_push_secs.saturating_mul(1000);
        let rest_ms = self.cfg.projects.design_rest_secs.saturating_mul(1000);
        for t in targets.iter().filter(|t| t.folder.is_some()) {
            for rec in self.design_branch_list(project, &t.name) {
                // 끊긴 전달을 잇는다(쉬는 branch면 배경 일이 다시 연다)
                self.design_chat_resume(project, &t.name, &rec.slug);
                if rec.state != DesignBranchState::Open {
                    continue;
                }
                // 마지막 저장과 마지막 말 중 늦은 때부터 센다(대화 중인 branch는 쉬지 않는다)
                let said = self.design_chat_load(project, &t.name, &rec.slug).map(|c| c.said_ms).unwrap_or(0);
                let idle = now.saturating_sub(rec.updated_ms.max(said));
                let queued =
                    self.design_push_gen.lock().unwrap().contains_key(&format!("{project}/{}/{}", t.name, rec.slug));
                let retry = rec.push_error.is_none()
                    || now.saturating_sub(rec.push_tried_ms) >= push_retry_ms(push_ms, rec.push_failures);
                let unpushed = rec.head != rec.pushed && rec.head == rec.own_head;
                if unpushed && retry && idle >= push_ms && !queued {
                    self.design_push_now(project, &t.name, &rec.slug).await;
                }
                let rest_again = rec.rest_error.is_none() || now.saturating_sub(rec.rest_tried_ms) >= REST_RETRY_MS;
                if idle >= rest_ms && rest_again {
                    // 다른 머신이 이어 받게 먼저 올린다(못 올려도 로컬 branch는 남는다)
                    if unpushed {
                        self.design_push_now(project, &t.name, &rec.slug).await;
                    }
                    self.design_rest(project, &t.name, &rec.slug).await;
                }
            }
        }
    }

    async fn design_rest(&self, project: &str, repo: &str, slug: &str) {
        let lock = self.design_branch_lock(project, repo, slug);
        let _one = lock.lock().await;
        let Ok(Some(mut rec)) = self.design_branch_load(project, repo, slug) else { return };
        // 잠금을 기다리는 사이 쓰거나 다시 열었으면 쉬지 않는다
        let rest_ms = self.cfg.projects.design_rest_secs.saturating_mul(1000);
        let now = aaa_common::util::now_ms();
        let said = self.design_chat_load(project, repo, slug).map(|c| c.said_ms).unwrap_or(0);
        if rec.state != DesignBranchState::Open
            || now.saturating_sub(rec.updated_ms.max(said)) < rest_ms
            || (rec.rest_error.is_some() && now.saturating_sub(rec.rest_tried_ms) < REST_RETRY_MS)
        {
            return;
        }
        rec.rest_tried_ms = now;
        let result = async {
            let (target, mid) = self.design_target(project, repo)?;
            let conn = self.design_write_conn(&mid)?;
            let req = AgentRequest::DesignClose {
                project: project.to_string(),
                repo: target.path,
                slug: slug.to_string(),
                discard: false,
                keep_local: true,
                remote: None,
            };
            conn.request_timeout(req, WRITE_WAIT).await.map(|_| ())
        }
        .await;
        match result {
            Ok(()) => {
                rec.state = DesignBranchState::Rest;
                rec.rest_error = None;
            }
            Err(e) => {
                tracing::warn!(project, repo, slug, "기획 branch를 쉼으로 내리지 못했다: {}", e.message);
                rec.rest_error = Some(e.message);
            }
        }
        if let Err(e) = self.design_branch_store(project, &rec) {
            tracing::warn!(project, repo, slug, "기획 branch 기록을 쓰지 못했다: {}", e.message);
        }
    }

    /// 트리에 싣는 기획 branch: 기록에 agent가 읽은 main과 다른 파일·앞선 commit 수를 더한다(읽지 못하면 기록만)
    pub(crate) async fn design_branch_views(
        &self,
        project: &str,
        repo: &str,
        repo_path: &str,
        folder: &str,
        mid: &str,
    ) -> Vec<DesignBranchView> {
        let recs = self.design_branch_list(project, repo);
        let conn = if recs.is_empty() { None } else { self.design_write_conn(mid).ok() };
        let mut views = Vec::new();
        for rec in recs {
            let mut view = self.design_view(project, &rec);
            if let Some(conn) = &conn {
                let req = AgentRequest::DesignTree {
                    repo: repo_path.to_string(),
                    folder: folder.to_string(),
                    branch: Some(rec.slug.clone()),
                };
                match conn.request_timeout(req, WRITE_WAIT).await {
                    Ok(AgentResponse::DesignTree(tree)) => {
                        view.changed = tree.changed;
                        view.ahead = tree.ahead;
                        view.head = Some(tree.commit);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(project, repo, slug = rec.slug, "기획 branch를 읽지 못했다: {}", e.message)
                    }
                }
            }
            views.push(view);
        }
        views
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::FrontRequest;

    /// 쓰기는 조작 권한의 사람만: thread·coordinator 세션과 보기 기기는 agent에 닿기 전에 거부한다
    #[tokio::test]
    async fn sessions_and_readers_do_not_write_design_branches() {
        let state = std::env::temp_dir().join(format!("aaa-design-branch-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Arc::new(Host::open(cfg, state.clone()).unwrap());
        h.project_create("demo", Default::default(), "test").unwrap();
        for (role, actor) in [
            (DeviceRole::Local, "worker:01SESSION"),
            (DeviceRole::Local, "evaluator:01SESSION"),
            (DeviceRole::Reader, "device:01READER"),
        ] {
            // 실제 입구(Host::call)로 보낸다: 권한 검사·요청 가림·dispatch를 모두 지난다
            let reqs = [
                FrontRequest::DesignBranchOpen {
                    project: "demo".into(),
                    repo: "repo".into(),
                    slug: "boss-dex".into(),
                    title: None,
                    profile: None,
                },
                FrontRequest::DesignSave {
                    project: "demo".into(),
                    repo: "repo".into(),
                    branch: "boss-dex".into(),
                    path: "docs/a.md".into(),
                    base: None,
                    text: "# a\n".into(),
                    summary: None,
                },
                FrontRequest::DesignDiscard { project: "demo".into(), repo: "repo".into(), branch: "boss-dex".into() },
                FrontRequest::DesignSay {
                    project: "demo".into(),
                    repo: "repo".into(),
                    branch: "boss-dex".into(),
                    text: "말".into(),
                },
            ];
            for req in reqs {
                let e = h.call(role, actor, req).await.unwrap_err();
                assert_eq!(e.code, ErrorCode::Forbidden, "{actor}: {e:?}");
            }
        }
        // 사람은 권한을 지나 저장소 확인에서 멈춘다(이 프로젝트에 저장소가 없다)
        let open = FrontRequest::DesignBranchOpen {
            project: "demo".into(),
            repo: "repo".into(),
            slug: "boss-dex".into(),
            title: None,
            profile: None,
        };
        let e = h.call(DeviceRole::Local, "local:me", open.clone()).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{e:?}");
        // thread·coordinator가 아닌 pane(사람의 셸)도 사람이다
        let e = h.call(DeviceRole::Local, "local:me@01SHELL", open).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{e:?}");
        assert!(h.design_branch_load("demo", "repo", "Bad").is_err(), "branch 이름 검사");
        drop(h);
        std::fs::remove_dir_all(&state).unwrap();
    }

    /// 기획 대화 thread는 (보기 권한·다른 머신 중계여도) 자기 기획 branch 저장만 한다. 열기·버리기·말하기와 다른 branch,
    /// 다른 thread는 거부한다
    #[tokio::test]
    async fn design_chat_thread_saves_only_its_own_branch() {
        let state = std::env::temp_dir().join(format!("aaa-design-writer-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = state.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Arc::new(Host::open(cfg, state.clone()).unwrap());
        h.project_create("demo", Default::default(), "test").unwrap();
        // t-0003은 같은 대화 표지지만 대화의 지금 thread가 아니다(버린 대화에서 남은 thread), t-0004는 다른 프로젝트의 것
        for (project, id, pane, design) in [
            ("demo", "t-0001", "01DESIGN", Some("repo/boss-dex")),
            ("demo", "t-0002", "01OTHER", None),
            ("demo", "t-0003", "01ORPHAN", Some("repo/boss-dex")),
            ("other", "t-0004", "01ELSE", Some("repo/boss-dex")),
        ] {
            let mut rec = crate::orch::ThreadRec::starting(
                project,
                "기획",
                "m",
                "",
                aaa_proto::Role::Worker,
                aaa_proto::ThreadKind::Dir,
                0,
            );
            rec.id = id.into();
            rec.pane = Some(pane.into());
            rec.design = design.map(str::to_string);
            rec.status = aaa_proto::ThreadStatus::Open;
            h.db.thread_import(project, id, "open", &serde_json::to_string(&rec).unwrap()).unwrap();
        }
        let chat = crate::design_chat::Chat { thread: Some("t-0001".into()), ..Default::default() };
        h.design_chat_store("demo", "repo", "boss-dex", &chat).unwrap();
        let save = |branch: &str| FrontRequest::DesignSave {
            project: "demo".into(),
            repo: "repo".into(),
            branch: branch.into(),
            path: "docs/a.md".into(),
            base: None,
            text: "# a\n".into(),
            summary: None,
        };
        // 자기 branch: 권한을 지나 저장소 확인에서 멈춘다(이 프로젝트에 저장소가 없다). 중계(보기)·같은 머신 pane 모두
        for (role, actor) in [(DeviceRole::Reader, "worker:01DESIGN"), (DeviceRole::Local, "local:me@01DESIGN")] {
            let e = h.call(role, actor, save("boss-dex")).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::NotFound, "{actor}: {e:?}");
        }
        assert_eq!(
            h.design_writer(DeviceRole::Reader, "worker:01DESIGN", "demo", Some(("repo", "boss-dex"))).unwrap(),
            "t-0001"
        );
        let others = [
            ("worker:01DESIGN", save("other")),
            // 알린 세션만으로 정한 thread 범위(coordinator pane·떼어 낸 프로세스가 대화 thread 세션을 댄 것)
            ("worker:01DESIGN@unverified", save("boss-dex")),
            ("worker:01ORPHAN", save("boss-dex")),
            ("worker:01ELSE", save("boss-dex")),
            ("worker:01OTHER", save("boss-dex")),
            ("local:me@01OTHER", save("boss-dex")),
            (
                "worker:01DESIGN",
                FrontRequest::DesignBranchOpen {
                    project: "demo".into(),
                    repo: "repo".into(),
                    slug: "boss-dex".into(),
                    title: None,
                    profile: None,
                },
            ),
            (
                "worker:01DESIGN",
                FrontRequest::DesignDiscard { project: "demo".into(), repo: "repo".into(), branch: "boss-dex".into() },
            ),
            (
                "local:me@01DESIGN",
                FrontRequest::DesignSay {
                    project: "demo".into(),
                    repo: "repo".into(),
                    branch: "boss-dex".into(),
                    text: "말".into(),
                },
            ),
        ];
        for (actor, req) in others {
            let e = h.call(DeviceRole::Local, actor, req).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::Forbidden, "{actor}: {e:?}");
        }
        // 원격 중계는 저장만 싣는다(열기·말하기·버리기는 중계 목록 밖)
        let say = FrontRequest::DesignSay {
            project: "demo".into(),
            repo: "repo".into(),
            branch: "boss-dex".into(),
            text: "말".into(),
        };
        assert!(!say.thread_relay_allowed() && save("x").thread_relay_allowed());
        drop(h);
        std::fs::remove_dir_all(&state).unwrap();
    }

    #[test]
    fn failed_pushes_back_off() {
        assert_eq!(push_retry_ms(1_000, 1), 600_000, "적어도 10분");
        assert_eq!(push_retry_ms(120_000, 1), 1_200_000, "쉼 창의 10배");
        assert_eq!(push_retry_ms(120_000, 3), 4_800_000, "거듭 실패하면 두 배씩");
        assert_eq!(push_retry_ms(60_000, 30), 600_000 * 64, "최대 64배");
    }

    #[test]
    fn titles_and_repo_names_stay_plain() {
        assert_eq!(one_line("  보스\n도감  정리 ", 80), "보스 도감 정리");
        assert_eq!(one_line(&"가".repeat(100), 80).chars().count(), 80);
        assert!(repo_ok("pt") && repo_ok("no2-engine"));
        for bad in ["", ".", "..", "a/b", "a\\b"] {
            assert!(!repo_ok(bad), "{bad}");
        }
    }
}
