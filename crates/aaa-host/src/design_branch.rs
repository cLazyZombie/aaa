//! 기획 저장과 기획 대화 기록(t-1894 2단계, t-2002 main 직접 저장). 사용자 결정(11-18 q-4213~q-4218)이 11-16의 기획 branch
//! 흐름을 대신한다: 기획 저장 하나가 곧 origin 기본 branch commit·push 하나다. 쓰기는 그 저장소 머신 agent(`design_main.v1`)가
//! 「main 기획 자리」에서 하고, host는 저장소마다 차례를 지킨다(`design_main_lock`).
//!
//! - 저장은 파일마다 기준 blob을 주고(새 파일은 없음) 한 commit으로 올린다. 그 사이 다른 판이 생겼으면 거절이고, 저장소 문서
//!   검사(host.toml `[projects.design_check]`, 인자의 `{base}`·`{head}`)를 통과하지 못하면 올리지 않고 그 까닭을 돌려준다.
//!   올린 뒤 host가 기억하는 트리를 새 판으로 다시 읽는다(곧바로 읽어도 저장한 글이다).
//! - 「기획 대화」 기록(`design-branches/<프로젝트>/<저장소>/<주제>.json`, 옛 이름 그대로)은 branch가 아니라 대화 하나다: 주제·
//!   만든 이·main 기획 자리(대화 thread가 읽는다)·마지막 저장. 끝내기는 기록과 대화 thread를 거둔다.
//! - 쓰는 이는 사람(조작 권한)과 기획 대화 thread(자기 대화로만)다. 그 프로젝트 coordinator는 대화를 열고 말하기만 한다
//!   (`design_opener`, t-1928). 다른 aaa 세션(thread·다른 프로젝트 coordinator)은 쓰지 않는다.
//! - 「main에 반영」·반영 예약·쉼·올리기는 없어졌다(t-2002). 옛 기록의 그 칸은 읽기만 한다.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use aaa_proto::{
    AgentRequest, AgentResponse, DesignBranchState, DesignBranchView, DesignMainBlob, DesignMainChange, DesignMainFile,
    DeviceRole, ErrorBody, ErrorCode, FrontResponse, design_slug_ok,
};
use serde::{Deserialize, Serialize};

use crate::orch::{bad, internal};
use crate::{Host, err};

/// 기획 main 직접 저장 agent 기능(t-2002)
pub(crate) const MAIN_FEATURE: &str = aaa_proto::DESIGN_MAIN_FEATURE;
/// main 기획 자리 열기(origin 받기 포함)를 기다리는 상한
pub(crate) const WRITE_WAIT: Duration = Duration::from_secs(120);
/// main 쓰기를 기다리는 상한(받기·검사 300초·push 60초를 `DESIGN_MAIN_TRIES`번까지)
pub(crate) const MAIN_WAIT: Duration = Duration::from_secs(1200);
/// 웹 그림 업로드 주소의 「대화 없음」 자리(주제 이름 규칙이 `main`을 받지 않아 겹치지 않는다)
pub(crate) const NO_CHAT: &str = "main";

const DESIGN_IMAGE_UPLOAD_TTL: Duration = Duration::from_secs(120);
const DESIGN_IMAGE_UPLOAD_LIMIT: usize = 8;

pub(crate) struct DesignImageUpload {
    total: u64,
    dir: Option<String>,
    name: Option<String>,
    bytes: Vec<u8>,
    touched: Instant,
}

enum AttachmentSource {
    WebEdit,
    WebChat,
    Thread,
}
/// 주제·요약 글자 수 상한
const TITLE_MAX: usize = 80;
const SUMMARY_MAX: usize = 200;
/// 「main에 반영」을 부른 옛 front에 주는 까닭
const APPLY_GONE: &str = "「main에 반영」은 없어졌다: 기획은 저장하면 바로 main에 들어간다(t-2002)";

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
    fn record_write(&mut self, commit: &str) {
        let now = aaa_common::util::now_ms();
        self.updated_ms = now;
        self.head = Some(commit.to_string());
        self.own_head = Some(commit.to_string());
        // 올린 것만 저장이다
        self.pushed = Some(commit.to_string());
        self.pushed_ms = now;
        self.push_error = None;
        self.state = DesignBranchState::Open;
    }

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

/// 한 줄로 줄인 글(줄바꿈·앞뒤 빈칸을 빼고 상한까지)
fn one_line(text: &str, max: usize) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// 저장소 이름은 기록 폴더 이름이 된다: 경로 조각이 아닌 이름만
fn repo_ok(repo: &str) -> bool {
    !repo.is_empty() && repo != "." && repo != ".." && !repo.contains(['/', '\\', '\0'])
}

/// 붙일 그림·영상의 종류를 파일 머리로 정한다(그림은 task 첨부와 같은 판정)
fn media_ext(bytes: &[u8]) -> Option<&'static str> {
    if let Some(ext) = aaa_common::util::image_extension(bytes) {
        return Some(ext);
    }
    if bytes.len() > 12 && &bytes[4..8] == b"ftyp" {
        return Some("mp4");
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some("webm");
    }
    None
}

/// 붙일 그림 이름(`--name`): 폴더 없는 이름, 확장자가 내용과 같다(jpg·jpeg는 같다)
fn attach_name(name: &str, ext: &str) -> Result<String, ErrorBody> {
    let name = name.trim();
    let given =
        std::path::Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    let given = if given == "jpeg" { "jpg".to_string() } else { given };
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') || !aaa_proto::design_write_path_ok(name)
    {
        return Err(bad("그림 이름은 폴더 없는 파일 이름이다"));
    }
    if given != ext {
        return Err(bad(format!("그림 이름의 확장자가 내용({ext})과 다르다: {name}")));
    }
    Ok(name.to_string())
}

/// main 쓰기 결과
pub(crate) struct MainSaved {
    pub commit: String,
    pub base: String,
    pub blobs: Vec<DesignMainBlob>,
    pub warnings: Vec<String>,
}

impl MainSaved {
    fn blob(&self, path: &str) -> String {
        self.blobs.iter().find(|b| b.path == path).and_then(|b| b.blob.clone()).unwrap_or_default()
    }
}

impl Host {
    /// 기획 commit에 남길 쓴 이: thread 세션은 그 ID, coordinator pane 안 CLI는 `coordinator`, 나머지는 `사람`
    pub(crate) fn design_actor_label(&self, actor: &str) -> String {
        if let Some(session) = actor.strip_prefix("worker:") {
            return self.thread_of_pane(session).map_or_else(|| "thread".into(), |r| r.id);
        }
        let coordinator = actor.strip_prefix("local:").and_then(|rest| rest.split_once('@')).is_some_and(|(_, s)| {
            self.pane_view_of(s, DeviceRole::Local).and_then(|p| p.role) == Some(aaa_proto::Role::Coordinator)
                || self.thread_of_pane(s).is_some_and(|r| r.role == aaa_proto::Role::Coordinator)
        });
        if coordinator { "coordinator".into() } else { "사람".into() }
    }

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

    pub(crate) fn design_branch_store(&self, project: &str, rec: &Rec) -> Result<(), ErrorBody> {
        let dir = self.design_branch_dir(project, &rec.repo)?;
        std::fs::create_dir_all(&dir).map_err(|e| internal(format!("{}: {e}", dir.display())))?;
        let bytes = serde_json::to_vec_pretty(rec).map_err(internal)?;
        crate::orch::write_atomic(&dir.join(format!("{}.json", rec.slug)), &bytes).map_err(internal)
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

    /// 대화 하나의 차례(기록·사건)
    pub(crate) fn design_branch_lock(&self, project: &str, repo: &str, slug: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.thread_op_lock(project, &format!("design:{repo}/{slug}"))
    }

    /// 저장소 하나의 main 쓰기 차례(대화 잠금을 쥔 채 잡는다: 차례는 늘 대화 → main)
    pub(crate) fn design_main_lock(&self, project: &str, repo: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.thread_op_lock(project, &format!("design-main-write:{repo}"))
    }

    /// main 쓰기 연결(design_main.v1). 옛 agent는 기획 branch에 쓰던 판이라 보내지 않는다
    pub(crate) fn design_main_conn(&self, mid: &str) -> Result<crate::AgentConn, ErrorBody> {
        let conn = self.live_conn(mid)?;
        if !self.machine_feature(mid, MAIN_FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                format!(
                    "{} agent가 기획 main 직접 저장을 모른다({MAIN_FEATURE} 없음). 그 머신 aaa를 갱신한다",
                    self.machine_name(mid)
                ),
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

    /// 저장 요청의 대화(주제): 주면 그것, 대화 thread 세션이 비워 보내면(`--branch` 없음) 그 thread 기록의 자기 대화
    pub(crate) fn design_conversation(&self, actor: &str, repo: &str, slug: &str) -> Option<String> {
        if !slug.is_empty() && slug != NO_CHAT {
            return Some(slug.to_string());
        }
        // thread pane의 CLI는 `worker:<세션>`, 역할을 알리지 않은 pane은 `local:<client>@<세션>`이다(`design_writer`와 같다)
        let session = actor
            .strip_prefix("worker:")
            .or_else(|| actor.strip_prefix("local:").and_then(|rest| rest.split_once('@')).map(|(_, s)| s))?;
        let rec = self.thread_of_pane(session)?;
        rec.design.as_deref()?.strip_prefix(&format!("{repo}/")).map(str::to_string)
    }

    /// 누가 쓰나: 조작 권한의 사람(aaa 밖 터미널·사람의 셸 pane·웹)과, `branch`(저장소, 주제)를 주면 그 프로젝트 그 기획
    /// 대화의 지금 thread(보기 권한·다른 머신 중계여도). 다른 thread·끝낸 대화에서 남은 thread·coordinator 세션과,
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
                    "thread 세션은 기획에 쓰지 않는다(기획 대화 thread는 자기 대화로만 저장한다)",
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
                return Err(err(
                    ErrorCode::Forbidden,
                    "thread·coordinator 세션 안에서는 기획에 쓰지 않는다(coordinator는 자기 프로젝트의 \
                     `aaa plan new`·`aaa plan say`로 대화를 열고 말만 한다)",
                ));
            }
        }
        if actor.starts_with("evaluator:") || actor.starts_with("detached:") {
            return Err(err(ErrorCode::Forbidden, "이 세션은 기획에 쓰지 않는다"));
        }
        if !role.can_control() {
            return Err(err(
                ErrorCode::Forbidden,
                format!("보기 권한으로는 기획을 고칠 수 없다{}", crate::front::reader_reason(actor)),
            ));
        }
        Ok(self.design_actor_label(actor))
    }

    /// 기획 대화를 여는 이·말하는 이(`aaa plan new`·`aaa plan say`): 쓰는 이(`design_writer`)와 그 프로젝트의 coordinator.
    /// coordinator는 사용자의 「아이디어」·「기획」 요청을 대화로 넘긴다(t-1928, q-4013 2B). 저장·끝내기·되돌리기·맡기기는 여전히
    /// `design_writer`만 지난다. 다른 프로젝트 coordinator·확인하지 못한 세션·thread는 `design_writer`가 거부한다
    pub(crate) fn design_opener(&self, role: DeviceRole, actor: &str, project: &str) -> Result<String, ErrorBody> {
        if role.can_control() && self.coordinator_project_of_actor(actor).as_deref() == Some(project) {
            return Ok("coordinator".into());
        }
        self.design_writer(role, actor, project, None)
    }

    /// 파일 여럿을 한 commit으로 그 저장소 main에 올린다(저장소마다 하나씩). 올렸으면 기억한 트리를 새 판으로 다시 읽는다
    pub(crate) async fn design_main_write(
        &self,
        project: &str,
        target: &crate::design::Target,
        mid: &str,
        files: Vec<DesignMainFile>,
        message: String,
    ) -> Result<MainSaved, ErrorBody> {
        let conn = self.design_main_conn(mid)?;
        let lock = self.design_main_lock(project, &target.name);
        let _one = lock.lock().await;
        let req = AgentRequest::DesignMainWrite {
            project: project.to_string(),
            repo: target.path.clone(),
            folder: target.folder.clone().unwrap_or_default(),
            files,
            message,
            check: target.check.clone(),
        };
        let AgentResponse::DesignMainSaved { commit, base, blobs, warnings } =
            conn.request_timeout(req, MAIN_WAIT).await?
        else {
            return Err(internal("예상하지 못한 main 저장 응답"));
        };
        if commit != base {
            // 곧바로 읽어도 저장한 글이게 트리(판·ID·표지 수)를 새로 읽는다. 실패해도 저장은 끝났다
            Box::pin(self.design_repo(project, target.clone())).await;
        }
        Ok(MainSaved { commit, base, blobs, warnings })
    }

    /// 기획 대화를 연다: 기록을 만들고(있으면 그대로) 저장소 머신의 main 기획 자리를 origin main 끝으로 맞춘다.
    /// branch는 만들지 않는다(t-2002)
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

    /// `create`가 아니면 host 기록이 있는 대화만 연다(대화 배경 일: 그 사이 끝낸 대화를 되살리지 않는다)
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
        let by = self.design_opener(role, actor, project)?;
        if let Some(p) = profile.as_deref() {
            self.check_profile_allowed(project, p)?;
        }
        let (target, mid) = self.design_target(project, repo)?;
        let lock = self.design_branch_lock(project, &target.name, slug);
        let _one = lock.lock().await;
        let old = self.design_branch_load(project, &target.name, slug)?;
        if old.is_none() && !create {
            return Err(err(ErrorCode::NotFound, format!("기획 대화가 없다(끝냈다): {slug}")));
        }
        let conn = self.design_main_conn(&mid)?;
        if old.is_none() {
            self.design_chat_forget(project, &target.name, slug).await?;
        }
        let req = AgentRequest::DesignMainOpen { project: project.to_string(), repo: target.path.clone() };
        let AgentResponse::DesignBranch { head, cwd, .. } = conn.request_timeout(req, WRITE_WAIT).await? else {
            return Err(err(ErrorCode::Internal, "예상하지 못한 응답"));
        };
        let now = aaa_common::util::now_ms();
        let fresh = old.is_none();
        let rec = match old {
            Some(mut rec) => {
                rec.state = DesignBranchState::Open;
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
                cwd: Some(cwd),
                base: Some(head.clone()),
                origin: "main".into(),
                ..Default::default()
            },
        };
        self.design_branch_store(project, &rec)?;
        if fresh {
            self.design_chat_event(project, &target.name, slug, "open", "", Some(head)).await;
        }
        if profile.is_some() {
            self.design_chat_edit(project, &target.name, slug, |c| c.profile = profile).await?;
        }
        self.db.audit(actor, "design_branch_open", &format!("{project} {}/{slug}", target.name));
        Ok(FrontResponse::DesignBranch { branch: self.design_view(project, &rec) })
    }

    /// 기획 문서 하나(와 `extra` 글 파일)를 main에 저장한다
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
        extra: Vec<aaa_proto::DesignSaveFile>,
    ) -> Result<FrontResponse, ErrorBody> {
        self.design_save_event(role, actor, project, repo, slug, (path, base, text), extra, summary, None).await
    }

    /// `design_save`와 같고, `event`를 주면 대화에 끼우는 사건을 그것으로 적는다(구현 맡기기의 「구현 맡김」, 되돌리기)
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_save_event(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        (path, base, text): (&str, Option<String>, String),
        extra: Vec<aaa_proto::DesignSaveFile>,
        summary: Option<String>,
        event: Option<(&'static str, String)>,
    ) -> Result<FrontResponse, ErrorBody> {
        let conv = self.design_conversation(actor, repo, slug);
        // 저장소 이름은 그대로 찾으므로(design_target) 대화 thread의 `<저장소>/<주제>`와 바로 견준다
        let by = self.design_writer(role, actor, project, conv.as_deref().map(|s| (repo, s)))?;
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let path = crate::design::checked_path(&folder, path)?;
        let mut writes = vec![(path.clone(), base, text)];
        for f in extra {
            writes.push((crate::design::checked_path(&folder, &f.path)?, f.base, f.text));
        }
        for (p, b, _) in &writes {
            if b.as_deref().is_some_and(|b| !aaa_common::git_watch::valid_sha(b)) {
                return Err(bad(format!("기준 판은 blob SHA다: {p}")));
            }
        }
        // 대화 잠금은 main 쓰기(검사·push·다시 하기로 몇 분) 동안 쥐지 않는다: 쓴 뒤 사건을 남길 때만 잡는다(말하기·열기가 막히지 않게)
        let created = writes[0].1.is_none();
        let summary = summary
            .map(|s| one_line(&s, SUMMARY_MAX))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| if created { format!("{path} 새로") } else { format!("{path} 고침") });
        let mut files = Vec::with_capacity(writes.len());
        for (p, b, t) in writes {
            // 기획서 ID(t-1957): 표지가 없는 문서는 원장 번호의 표지를 넣어 저장한다(있는 표지는 고쳐 쓰지 않는다). main의 옛 글을
            // 읽을 수 있어 heap에 둔다(공용 dispatcher 스택)
            let t = Box::pin(self.design_text_with_id(project, &target.name, &p, b.is_some(), t)).await;
            files.push(DesignMainFile { path: p, base: b, change: DesignMainChange::Text { text: t } });
        }
        let saved = self.design_main_write(project, &target, &mid, files, format!("기획: {summary} ({by})")).await?;
        let commit = saved.commit.clone();
        let blob = saved.blob(&path);
        if let Some(conv) = conv.as_deref() {
            self.design_saved_in_chat(project, &target.name, conv, &by, &path, created, &commit, event.clone()).await;
        }
        self.db.audit(actor, "design_save", &format!("{project} {} {path} {commit}", target.name));
        if event.is_none() && commit != saved.base {
            // 넘긴 블록 고침 알림은 저장을 기다리게 하지 않는다
            let (host, project, repo, conv, path, commit) = (
                self.clone(),
                project.to_string(),
                target.name.clone(),
                conv.unwrap_or_default(),
                path.clone(),
                commit.clone(),
            );
            tokio::spawn(async move { host.design_handoff_relay(&project, &repo, &conv, &path, &commit).await });
        }
        Ok(FrontResponse::DesignSaved { path, commit, blob })
    }

    /// 저장을 그 대화에 남긴다: 사건(대화 thread의 저장인가, 사람이 고친 것인가), 사람이 고친 문서는 다음 말 앞에 알린다.
    /// 끝낸 대화(기록 없음)면 남기지 않는다
    #[allow(clippy::too_many_arguments)]
    async fn design_saved_in_chat(
        &self,
        project: &str,
        repo: &str,
        slug: &str,
        by: &str,
        path: &str,
        created: bool,
        commit: &str,
        event: Option<(&'static str, String)>,
    ) {
        let lock = self.design_branch_lock(project, repo, slug);
        let _one = lock.lock().await;
        let Ok(Some(mut rec)) = self.design_branch_load(project, repo, slug) else { return };
        let by_thread = self.design_chat_load(project, repo, slug).ok().and_then(|c| c.thread).as_deref() == Some(by);
        let kind = match (by_thread, created) {
            (true, true) => "create",
            (true, false) => "save",
            (false, _) => "edit",
        };
        let told = event.is_none();
        let (kind, text) = event.unwrap_or((kind, path.to_string()));
        self.design_chat_event(project, repo, slug, kind, &text, Some(commit.to_string())).await;
        // 사람이 고친 문서·그림은 다음 말 앞에 알린다(맡기기·되돌리기·대화 입력창 그림·지우기는 사건이나 말이 알린다)
        if !by_thread && told {
            let doc = path.to_string();
            aaa_common::diagnostic::ignored(
                self.design_chat_edit(project, repo, slug, |c| {
                    if !c.edited.contains(&doc) {
                        c.edited.push(doc);
                    }
                })
                .await,
                "다음 말 앞 알림만 빠진다",
            );
        }
        rec.record_write(commit);
        if let Err(e) = self.design_branch_store(project, &rec) {
            tracing::warn!(project, repo, slug, "기획 대화 기록을 쓰지 못했다: {}", e.message);
        }
    }

    /// 그림·영상 붙이기(웹 「그림」): 내용 hash 이름(`<문서 폴더>/<sha256 앞 16자>.<확장자>`)으로 main에 새 파일로 올린다.
    /// 그 머신 agent에 조각(`ATTACH_CHUNK`)으로 넘긴다(연결 frame 1MiB). 사람이 고친 것처럼 다음 말 앞에 알린다
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
        self.design_attach_with(role, actor, project, repo, slug, (Some(dir), None), bytes, AttachmentSource::WebEdit)
            .await
    }

    /// 기획 대화 입력창의 그림(t-1950): 보낼 말 앞 「(그림: …)」이 경로를 알리므로 다음 말 앞 「사람이 고친 문서」에 넣지 않는다.
    /// 저장소 경로를 돌려준다
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_attach_quiet(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        dir: &str,
        bytes: &[u8],
    ) -> Result<String, ErrorBody> {
        match self
            .design_attach_with(role, actor, project, repo, slug, (Some(dir), None), bytes, AttachmentSource::WebChat)
            .await?
        {
            FrontResponse::DesignSaved { path, .. } => Ok(path),
            other => Err(internal(format!("그림 저장 응답이 아니다: {other:?}"))),
        }
    }

    /// CLI 그림 붙이기·지우기의 쓰는 이: 그 기획 대화 thread만
    fn design_image_writer(
        &self,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
    ) -> Result<(String, String), ErrorBody> {
        let conv = self
            .design_conversation(actor, repo, slug)
            .ok_or_else(|| err(ErrorCode::Forbidden, "기획 대화 thread만 CLI로 그림을 바꾼다"))?;
        let by = self.design_writer(role, actor, project, Some((repo, &conv)))?;
        if !actor.starts_with("worker:") {
            return Err(err(ErrorCode::Forbidden, "기획 대화 thread만 CLI로 그림을 바꾼다"));
        }
        Ok((by, conv))
    }

    /// main에서 그림 하나를 지우고 문서에 남은 참조를 알린다(저장소 검사가 깨진 참조를 막을 수 있다)
    pub(crate) async fn design_detach(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        path: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let (by, conv) = self.design_image_writer(role, actor, project, repo, slug)?;
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let path = crate::design::checked_path(&folder, path)?;
        let ext = std::path::Path::new(&path).extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp") {
            return Err(bad(format!("PNG·JPEG·GIF·WebP 그림만 지운다: {path}")));
        }
        let file = path.rsplit('/').next().unwrap_or(&path).to_string();
        let files = vec![DesignMainFile { path: path.clone(), base: None, change: DesignMainChange::Delete }];
        let saved =
            self.design_main_write(project, &target, &mid, files, format!("기획: 그림 {file} 삭제 ({by})")).await?;
        let event = ("remove", format!("그림 {file} 삭제"));
        self.design_saved_in_chat(project, &target.name, &conv, &by, &path, false, &saved.commit, Some(event)).await;
        self.db.audit(actor, "design_detach", &format!("{project} {} {path} {}", target.name, saved.commit));
        Ok(FrontResponse::DesignDetached { path, commit: saved.commit, warnings: saved.warnings })
    }

    /// CLI의 여러 frame을 받아 마지막 조각에서만 웹과 같은 저장 경로를 호출한다.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_image_chunk(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        (dir, name): (Option<String>, Option<String>),
        upload: String,
        offset: u64,
        total: u64,
        data: Vec<u8>,
    ) -> Result<FrontResponse, ErrorBody> {
        let (by, _) = self.design_image_writer(role, actor, project, repo, slug)?;
        if total == 0 || total > aaa_proto::DESIGN_MAX_FILE {
            return Err(bad("그림은 1바이트부터 16MiB까지 붙인다"));
        }
        let end = offset.saturating_add(data.len() as u64);
        if upload.len() != 26
            || !upload.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            || data.is_empty()
            || data.len() > aaa_proto::ATTACH_CHUNK
            || end > total
        {
            return Err(bad("그림 전송 ID·조각 크기·위치가 잘못됐다"));
        }
        // 전송 ID는 권한이 확인된 프로젝트·thread에 묶는다. 끊긴 전송은 기한과 개수로 메모리를 제한한다.
        let key = (project.to_string(), by, upload);
        let bytes = {
            let mut pending = self.design_image_uploads.lock().unwrap();
            pending.retain(|_, image| image.touched.elapsed() < DESIGN_IMAGE_UPLOAD_TTL);
            if offset == 0 && end == total && !pending.contains_key(&key) {
                data
            } else {
                if offset == 0 && !pending.contains_key(&key) {
                    if pending.len() >= DESIGN_IMAGE_UPLOAD_LIMIT {
                        return Err(err(ErrorCode::Unavailable, "동시 기획 그림 전송 한도에 도달했다"));
                    }
                    pending.insert(
                        key.clone(),
                        DesignImageUpload {
                            total,
                            dir: dir.clone(),
                            name: name.clone(),
                            bytes: Vec::new(),
                            touched: Instant::now(),
                        },
                    );
                }
                let image = pending.get_mut(&key).ok_or_else(|| bad("그림의 앞 조각이 없다"))?;
                if image.total != total || image.dir != dir || image.name != name || image.bytes.len() as u64 != offset
                {
                    return Err(bad("그림의 조각 순서·크기·저장 폴더·이름이 맞지 않다"));
                }
                image.bytes.extend_from_slice(&data);
                image.touched = Instant::now();
                if end < total {
                    return Ok(FrontResponse::Done);
                }
                pending.remove(&key).expect("마지막 조각을 받은 그림").bytes
            }
        };
        self.design_attach_with(
            role,
            actor,
            project,
            repo,
            slug,
            (dir.as_deref(), name.as_deref()),
            &bytes,
            AttachmentSource::Thread,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn design_attach_with(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        (dir, name): (Option<&str>, Option<&str>),
        bytes: &[u8],
        source: AttachmentSource,
    ) -> Result<FrontResponse, ErrorBody> {
        let thread = matches!(source, AttachmentSource::Thread);
        let conv = self.design_conversation(actor, repo, slug);
        let by = if thread {
            self.design_image_writer(role, actor, project, repo, slug)?.0
        } else {
            self.design_writer(role, actor, project, None)?
        };
        if bytes.is_empty() || bytes.len() as u64 > aaa_proto::DESIGN_MAX_FILE {
            return Err(bad("파일은 1바이트부터 16MiB까지 붙인다"));
        }
        let ext = if thread {
            aaa_common::util::image_extension(bytes).ok_or_else(|| bad("PNG·JPEG·GIF·WebP 그림만 붙인다"))?
        } else {
            media_ext(bytes).ok_or_else(|| bad("그림(PNG·JPEG·GIF·WebP)·영상(MP4·WebM)만 붙인다"))?
        };
        use sha2::Digest as _;
        let sha256 = hex::encode(sha2::Sha256::digest(bytes));
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let clean_dir;
        let dir = dir.unwrap_or(&folder);
        let dir = if thread {
            clean_dir = aaa_proto::design_clean_path(dir).ok_or_else(|| bad("그림 폴더 경로가 잘못됐다"))?;
            clean_dir.as_str()
        } else {
            dir.trim_matches('/')
        };
        let file = match name {
            Some(name) => attach_name(name, ext)?,
            None => format!("{}.{ext}", &sha256[..16]),
        };
        let path = crate::design::checked_path(&folder, &format!("{dir}/{file}"))?;
        let conn = self.design_main_conn(&mid)?;
        // 전송마다 새 ID: 같은 그림을 동시에 붙여도 agent에서 서로의 조각을 건드리지 않는다
        let upload = aaa_common::util::new_id();
        for (i, chunk) in bytes.chunks(aaa_proto::ATTACH_CHUNK).enumerate() {
            let offset = i * aaa_proto::ATTACH_CHUNK;
            let req = AgentRequest::DesignMainStage {
                project: project.to_string(),
                repo: target.path.clone(),
                upload: upload.clone(),
                sha256: sha256.clone(),
                offset: offset as u64,
                data: aaa_proto::B64(chunk.to_vec()),
                eof: offset + chunk.len() == bytes.len(),
            };
            conn.request_timeout(req, WRITE_WAIT).await?;
        }
        // 이름을 준 그림은 그 이름의 다른 그림을 바꾼다. 이름 없는(내용 hash) 그림은 같은 내용만 이미 붙은 것이다
        let change = DesignMainChange::Staged { sha256, upload, replace: name.is_some() };
        let files = vec![DesignMainFile { path: path.clone(), base: None, change }];
        let saved =
            self.design_main_write(project, &target, &mid, files, format!("기획: 그림 {file} 붙임 ({by})")).await?;
        let blob = saved.blob(&path);
        if let Some(conv) = conv.as_deref() {
            // 웹 「그림」은 사람이 고친 것으로(다음 말 앞에 알린다), 대화 입력창·대화 thread의 그림은 말이 알린다
            let event = match source {
                AttachmentSource::WebEdit => None,
                AttachmentSource::WebChat => Some(("edit", path.clone())),
                AttachmentSource::Thread => Some(("create", path.clone())),
            };
            self.design_saved_in_chat(project, &target.name, conv, &by, &path, false, &saved.commit, event).await;
        }
        self.db.audit(
            actor,
            "design_attach",
            &format!("{project} {} {path} {} {} bytes", target.name, saved.commit, bytes.len()),
        );
        Ok(FrontResponse::DesignSaved { path, commit: saved.commit, blob })
    }

    /// 문서 이력(main이 실제로 가졌던 판, 새것부터)
    pub(crate) async fn design_log(&self, project: &str, repo: &str, path: &str) -> Result<FrontResponse, ErrorBody> {
        let (target, mid) = self.design_target(project, repo)?;
        let folder = target.folder.clone().unwrap_or_default();
        let path = crate::design::checked_path(&folder, path)?;
        let conn = self.design_main_conn(&mid)?;
        let req = AgentRequest::DesignLog { repo: target.path, folder, path, limit: aaa_proto::DESIGN_LOG_MAX };
        match conn.request_timeout(req, WRITE_WAIT).await? {
            AgentResponse::DesignLog { entries } => Ok(FrontResponse::DesignLog { entries }),
            _ => Err(internal("예상하지 못한 이력 응답")),
        }
    }

    /// 이 판으로 되돌리기(사람만): 옛 main 판(`at`)의 글을 지금 판(`base`) 위에 새로 저장한다. 지금 판의 넘긴 블록 표지는 옛 판의
    /// 같은 블록(d-ID를 뺀 글이 같은 블록)에 옮겨 넣고, 그 블록이 옛 판에 없으면 거절한다(맡김이 끊기지 않게)
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn design_revert(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        path: &str,
        (at, base): (&str, &str),
        slug: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        if actor.starts_with("worker:") {
            return Err(err(ErrorCode::Forbidden, "되돌리기는 사람이 한다"));
        }
        self.design_writer(role, actor, project, None)?;
        if !aaa_common::git_watch::valid_sha(at) || !aaa_common::git_watch::valid_sha(base) {
            return Err(bad("판은 commit·blob SHA다"));
        }
        let FrontResponse::DesignDoc { doc: now } = self.design_read(project, repo, path, None).await? else {
            return Err(internal("예상하지 못한 응답"));
        };
        if now.blob != base {
            return Err(err(ErrorCode::Conflict, "그 사이 main 문서가 바뀌었다: 다시 읽고 고른다"));
        }
        let FrontResponse::DesignDoc { doc: old } = self.design_read(project, repo, path, Some(at)).await? else {
            return Err(internal("예상하지 못한 응답"));
        };
        if old.text == now.text {
            return Err(bad("지금 판과 같은 글이다"));
        }
        let text = revert_text(&old.text, &now.text).map_err(|e| err(ErrorCode::Conflict, e))?;
        let short = &at[..at.len().min(7)];
        Box::pin(self.design_save_event(
            role,
            actor,
            project,
            repo,
            slug,
            (&now.path, Some(base.to_string()), text),
            Vec::new(),
            Some(format!("{} 되돌림 ← {short}", now.path)),
            Some(("edit", format!("{} 되돌림 ← {short}", now.path))),
        ))
        .await
    }

    /// 「main에 반영」은 없어졌다(t-2002): 옛 front·CLI가 부르면 까닭을 돌려준다
    pub(crate) fn design_apply_gone(&self) -> Result<FrontResponse, ErrorBody> {
        Err(err(ErrorCode::BadRequest, APPLY_GONE))
    }

    /// 기획 대화를 끝낸다: 대화 thread를 알림 없이 끝내고 기록을 지운다. main에 저장한 글은 그대로다(되돌리기는 문서 이력으로)
    pub(crate) async fn design_discard(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.design_writer(role, actor, project, None)?;
        let (target, _) = self.design_target(project, repo)?;
        let lock = self.design_branch_lock(project, &target.name, slug);
        let _one = lock.lock().await;
        let Some(rec) = self.design_branch_load(project, &target.name, slug)? else {
            // 앞선 끝내기가 기록은 지웠는데 대화 thread를 끝내지 못했으면 마저 끝낸다
            if self.design_chat_load(project, &target.name, slug)?.thread.is_some() {
                self.design_chat_retire(project, &target.name, slug, actor).await?;
                return Ok(FrontResponse::Done);
            }
            return Err(err(ErrorCode::NotFound, format!("기획 대화가 없다: {slug}")));
        };
        let path = self.design_branch_dir(project, &target.name)?.join(format!("{slug}.json"));
        std::fs::remove_file(&path).map_err(|e| internal(format!("{}: {e}", path.display())))?;
        // 옛 기획 branch에서 맡긴 기록(t-2002 전)은 그 branch가 없어졌다: 평소처럼 시작하게 지운다
        self.design_handoff_forget(project, &target.name, slug, rec.created_ms);
        self.db.audit(actor, "design_discard", &format!("{project} {}/{slug}", target.name));
        self.design_chat_retire(project, &target.name, slug, actor).await.map_err(|e| {
            err(
                e.code,
                format!(
                    "기획 대화 기록은 지웠으나 대화 thread를 끝내지 못했다(다시 끝내면 마저 끝낸다): {}",
                    e.message
                ),
            )
        })?;
        Ok(FrontResponse::Done)
    }

    /// ticker: 기획 일(끊긴 대화 전달 잇기·끊긴 그림 전송 치우기)을 따로 띄운다(한 프로젝트에 한 번에 하나)
    pub(crate) fn design_pass_spawn(self: &Arc<Self>, project: &str) {
        if !self.design_passes.lock().unwrap().insert(project.to_string()) {
            return;
        }
        let host = self.clone();
        let project = project.to_string();
        tokio::spawn(async move {
            host.design_pass(&project).await;
            host.design_passes.lock().unwrap().remove(&project);
        });
    }

    async fn design_pass(self: &Arc<Self>, project: &str) {
        self.design_image_uploads.lock().unwrap().retain(|_, image| image.touched.elapsed() < DESIGN_IMAGE_UPLOAD_TTL);
        let Ok(targets) = self.design_targets(project) else { return };
        for t in targets.iter().filter(|t| t.folder.is_some()) {
            for rec in self.design_branch_list(project, &t.name) {
                self.design_chat_resume(project, &t.name, &rec.slug);
            }
        }
    }

    /// 트리에 싣는 기획 대화(기록만, agent를 거치지 않는다)
    pub(crate) fn design_branch_views(&self, project: &str, repo: &str) -> Vec<DesignBranchView> {
        self.design_branch_list(project, repo).iter().map(|rec| self.design_view(project, rec)).collect()
    }
}

/// 되돌릴 글: 옛 글(`old`)에 지금 글(`now`)의 넘긴 블록 표지를 옮겨 넣는다. 지금 넘긴 블록마다 옛 글에서 d-ID를 뺀 글이 같은
/// 블록을 찾아 그 d-ID를 적는다(옛 글에 이미 같은 표지가 있으면 그대로). 찾지 못하면 그 d-ID를 까닭으로 거절한다
pub(crate) fn revert_text(old: &str, now: &str) -> Result<String, String> {
    let mut text = old.to_string();
    for m in aaa_proto::design_marks(now) {
        let Some(task) = m.task.clone() else { continue };
        let want = aaa_proto::design_unmark(&aaa_proto::design_mark_text(now, &m));
        let marks = aaa_proto::design_marks(&text);
        if marks.iter().any(|o| {
            o.task.as_deref() == Some(task.as_str())
                && aaa_proto::design_unmark(&aaa_proto::design_mark_text(&text, o)) == want
        }) {
            continue;
        }
        let Some(at) = marks
            .iter()
            .position(|o| o.task.is_none() && aaa_proto::design_unmark(&aaa_proto::design_mark_text(&text, o)) == want)
        else {
            return Err(format!(
                "넘긴 블록 {task}이(가) 그 판에 없어 되돌리지 않았다: 그 블록이 있는 판을 고르거나 대화로 고친다"
            ));
        };
        text = aaa_proto::design_mark_tasks(&text, &[at], &task)?;
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::FrontRequest;

    /// 쓰기는 조작 권한의 사람만: thread 세션과 보기 기기는 agent에 닿기 전에 거부한다(coordinator의 열기·말하기는 아래 시험)
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
                    extra: Vec::new(),
                },
                FrontRequest::DesignDiscard { project: "demo".into(), repo: "repo".into(), branch: "boss-dex".into() },
                FrontRequest::DesignSay {
                    project: "demo".into(),
                    repo: "repo".into(),
                    branch: "boss-dex".into(),
                    text: "말".into(),
                },
                FrontRequest::DesignRevert {
                    project: "demo".into(),
                    repo: "repo".into(),
                    path: "docs/a.md".into(),
                    at: "0".repeat(40),
                    base: "1".repeat(40),
                    branch: String::new(),
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

    /// 그 프로젝트 coordinator(host 머신에서 프로젝트 폴더에 도는 coordinator pane)는 기획 대화를 열고 말하기만 한다(t-1928).
    /// 저장·버리기·반영·맡기기는 거부하고, 프로젝트 폴더 밖 coordinator pane·끝난 coordinator pane은
    /// 열기·말하기도 거부한다
    #[test]
    fn project_coordinator_opens_and_says_but_does_not_write() {
        let state = std::env::temp_dir().join(format!("aaa-design-coord-{}", aaa_common::util::new_id()));
        // Windows에서도 배경 일이 쥔 DB를 runtime과 함께 놓은 뒤 상태 폴더를 지운다(deploy.rs 시험과 같다)
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let mut cfg = aaa_config::HostConfig::default();
            cfg.projects.root = state.join("projects").display().to_string();
            cfg.notify.web_push = false;
            let h = Arc::new(Host::open(cfg, state.join("host")).unwrap());
            h.project_create("demo", Default::default(), "test").unwrap();
            h.agent_connected(
                &aaa_proto::AgentHello {
                    proto: aaa_proto::AGENT_LINK,
                    machine_id: "m".into(),
                    machine_name: "mac-a".into(),
                    boot_id: "boot".into(),
                    os: "test".into(),
                    arch: "test".into(),
                    software: "test".into(),
                    features: vec![],
                    dev_mode: true,
                    sha256: None,
                    selfcheck: None,
                    sessions: vec![],
                    identity: None,
                    harness: vec![],
                },
                true,
                crate::AgentConn::new(aaa_common::conn::spawn_writer(tokio::io::sink(), aaa_proto::MAX_FRAME), 1),
            );
            let home = h.project_dir("demo").unwrap();
            let pane = |id: &str, cwd: &std::path::Path, state: aaa_proto::SessionState| aaa_proto::SessionInfo {
                responsibility: None,
                id: id.into(),
                name: "coordinator".into(),
                argv: vec!["claude".into()],
                cwd: Some(cwd.to_string_lossy().into()),
                pid: None,
                started_at_ms: 0,
                boot_id: "boot".into(),
                cols: 80,
                rows: 24,
                state,
                role: Some(aaa_proto::Role::Coordinator),
                label: None,
                harness: Some("claude".into()),
            };
            h.on_session("m", pane("01COORD", &home, aaa_proto::SessionState::Running), false);
            // 프로젝트 폴더 밖에서 도는 coordinator 역할 pane은 그 프로젝트 coordinator가 아니다
            h.on_session("m", pane("01ELSEWHERE", &state, aaa_proto::SessionState::Running), false);
            h.on_session("m", pane("01GONE", &home, aaa_proto::SessionState::Exited { code: Some(0) }), false);
            let open = || FrontRequest::DesignBranchOpen {
                project: "demo".into(),
                repo: "repo".into(),
                slug: "boss-dex".into(),
                title: Some("보스 도감".into()),
                profile: None,
            };
            let say = || FrontRequest::DesignSay {
                project: "demo".into(),
                repo: "repo".into(),
                branch: "boss-dex".into(),
                text: "사용자: 보스 도감을 기획하자".into(),
            };
            let coordinator = "local:cli@01COORD";
            assert_eq!(h.design_opener(DeviceRole::Local, coordinator, "demo").unwrap(), "coordinator");
            // 열기·말하기는 권한을 지나 저장소 확인에서 멈춘다(이 프로젝트에 저장소가 없다)
            for req in [open(), say()] {
                let e = h.call(DeviceRole::Local, coordinator, req).await.unwrap_err();
                assert_eq!(e.code, ErrorCode::NotFound, "{e:?}");
            }
            let writes = [
                FrontRequest::DesignSave {
                    project: "demo".into(),
                    repo: "repo".into(),
                    branch: "boss-dex".into(),
                    path: "docs/a.md".into(),
                    base: None,
                    text: "# a\n".into(),
                    summary: None,
                    extra: Vec::new(),
                },
                FrontRequest::DesignDiscard { project: "demo".into(), repo: "repo".into(), branch: "boss-dex".into() },
                FrontRequest::DesignRevert {
                    project: "demo".into(),
                    repo: "repo".into(),
                    path: "docs/a.md".into(),
                    at: "0".repeat(40),
                    base: "1".repeat(40),
                    branch: "boss-dex".into(),
                },
                FrontRequest::DesignHandoff {
                    project: "demo".into(),
                    repo: "repo".into(),
                    branch: "boss-dex".into(),
                    path: "docs/a.md".into(),
                    blob: "0".repeat(40),
                    blocks: Vec::new(),
                    title: "a".into(),
                    profile: None,
                    memo: String::new(),
                    reserve: false,
                },
            ];
            for req in writes {
                let e = h.call(DeviceRole::Local, coordinator, req.clone()).await.unwrap_err();
                assert_eq!(e.code, ErrorCode::Forbidden, "{req:?}: {e:?}");
            }
            // 「main에 반영」은 없어졌다(t-2002): 누가 불러도 까닭을 돌려준다
            let gone =
                FrontRequest::DesignApply { project: "demo".into(), repo: "repo".into(), branch: "boss-dex".into() };
            let e = h.call(DeviceRole::Local, coordinator, gone).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::BadRequest, "{e:?}");
            assert!(e.message.contains("없어졌다"), "{e:?}");
            // 프로젝트 폴더 밖 coordinator pane·끝난 coordinator pane
            for actor in ["local:cli@01ELSEWHERE", "local:cli@01GONE"] {
                for req in [open(), say()] {
                    let e = h.call(DeviceRole::Local, actor, req).await.unwrap_err();
                    assert_eq!(e.code, ErrorCode::Forbidden, "{actor}: {e:?}");
                }
            }
            // pane으로 확인하지 못한 세션(aaa 밖 터미널이 세션 ID를 댄 것)은 전처럼 사람이고 coordinator가 되지 않는다
            assert_eq!(h.design_opener(DeviceRole::Local, "local:cli@unverified", "demo").unwrap(), "사람");
            drop(h);
        });
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
            extra: Vec::new(),
        };
        // 자기 대화: 권한을 지나 저장소 확인에서 멈춘다(이 프로젝트에 저장소가 없다). 중계(보기)·같은 머신 pane 모두
        for (role, actor) in [(DeviceRole::Reader, "worker:01DESIGN"), (DeviceRole::Local, "local:me@01DESIGN")] {
            let e = h.call(role, actor, save("boss-dex")).await.unwrap_err();
            assert_eq!(e.code, ErrorCode::NotFound, "{actor}: {e:?}");
        }
        // 대화 thread는 주제를 주지 않아도(`--branch` 없음) 자기 대화로 쓴다(t-2002)
        let e = h.call(DeviceRole::Reader, "worker:01DESIGN", save("")).await.unwrap_err();
        assert_eq!(e.code, ErrorCode::NotFound, "{e:?}");
        assert_eq!(h.design_conversation("worker:01DESIGN", "repo", "").as_deref(), Some("boss-dex"));
        assert_eq!(
            h.design_conversation("local:me@01DESIGN", "repo", "").as_deref(),
            Some("boss-dex"),
            "역할을 알리지 않은 thread pane의 CLI도 자기 대화"
        );
        assert_eq!(h.design_conversation("worker:01DESIGN", "other", ""), None, "다른 저장소는 자기 대화가 아니다");
        assert_eq!(h.design_conversation("device:01WEB", "repo", "main"), None, "웹의 「대화 없음」");
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

    /// 되돌리기(t-2002): 옛 글에 지금의 넘긴 블록 표지를 같은 블록에 옮겨 넣고, 그 블록이 옛 판에 없으면 거절한다
    #[test]
    fn revert_keeps_handed_off_markers() {
        let now = "# 보스\n\n> [!계획] d-0005\n> 얼음 바닥\n\n> [!계획]\n> 해골 왕\n";
        let old = "# 보스\n\n옛 본문\n\n> [!계획]\n> 얼음 바닥\n";
        assert_eq!(revert_text(old, now).unwrap(), "# 보스\n\n옛 본문\n\n> [!계획] d-0005\n> 얼음 바닥\n");
        // 이미 같은 표지가 있으면 그대로
        let marked = "# 보스\n\n> [!계획] d-0005\n> 얼음 바닥\n";
        assert_eq!(revert_text(marked, now).unwrap(), marked);
        // 그 블록이 없는 판
        let e = revert_text("# 보스\n\n> [!계획]\n> 다른 블록\n", now).unwrap_err();
        assert!(e.contains("d-0005"), "{e}");
        // 넘긴 블록이 없으면 옛 글 그대로
        assert_eq!(revert_text(old, "# 보스\n").unwrap(), old);
    }

    /// 붙이기 이름(`--name`): 폴더 없는 이름, 확장자가 내용과 같다(jpg·jpeg는 같다)
    #[test]
    fn attach_names_are_plain_and_match_the_content() {
        assert_eq!(attach_name(" staff-mp-hud-mockup.png ", "png").unwrap(), "staff-mp-hud-mockup.png");
        assert_eq!(attach_name("a.JPEG", "jpg").unwrap(), "a.JPEG");
        for bad in ["a.gif", "dir/a.png", "..\\a.png", ".hidden.png", "", "CON.png"] {
            assert!(attach_name(bad, "png").is_err(), "{bad}");
        }
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
