//! 기획 대화(t-1894 2-2, FRONT_API 1.172): 기획 branch마다 대화 thread 하나. 아이디어 전용 thread(idea_thread.rs)와 같은 꼴이다.
//!
//! - 처음 말할 때 그 저장소 머신에 작업 폴더 thread로 띄운다(기획 worktree와 저장소 코드를 그 머신에서 읽는다). 할 일·실행
//!   한도·coordinator 요약·자동 끝내기에서 빼고(`ThreadRec::chat_only`), 쉬면 조용히 보류한다(idea_thread.rs `idea_rest_pass`).
//! - 보낸 말은 대화 기록(host 상태 폴더 `design-chats/<프로젝트>/<저장소>/<주제>.json`, 기획 branch 기록과 따로 잠근다)에 남기고,
//!   thread가 brief를 받은 뒤 차례로 보낸다. host가 다시 뜨면 ticker가 남은 말을 잇는다.
//! - thread는 자기 기획 branch에만 `aaa plan save`로 쓴다(design_branch.rs `design_writer`). 다른 머신 세션은 agent 중계로 쓴다.
//! - 쉬는 branch에 말하면 branch를 다시 연다. 버리면 대화 thread도 알림 없이 끝낸다(띄우는 중이었으면 뜬 뒤 끝낸다).

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use aaa_proto::{
    DesignBranchState, DeviceRole, ErrorBody, ErrorCode, FrontResponse, MessageMode, Role, ThreadKind, ThreadStatus,
};
use serde::{Deserialize, Serialize};

use crate::orch::{ResolveOpts, ResolveWhy, StartArgs, bad, internal};
use crate::{Host, err};

/// 다른 머신의 대화 thread가 `aaa plan save`를 host에 원격 전달하려면 그 머신 agent·CLI가 알아야 한다
pub(crate) const CHAT_FEATURE: &str = "design_chat.v1";
/// 보낸 말 한 개 상한(웹 입력창과 같다)
const MAX_SAY: usize = 100_000;
/// thread가 brief를 받기를 기다리는 상한. 넘으면 말은 남기고 다음 말·ticker 때 다시 보낸다
const DELIVER_WAIT: Duration = Duration::from_secs(600);
/// 띄우는 중인 대화 thread를 끝내려고 준비가 끝나기를 기다리는 상한
const END_WAIT: Duration = Duration::from_secs(60);
/// 말을 하나도 보내지 못하고 다시 도는 배경 일의 상한(thread가 거듭 멈추면 까닭을 남기고 다음 말을 기다린다)
const MAX_TRIES: u32 = 3;

/// 도는 배경 일 자리. 일이 끝나거나 panic해도 놓는다
struct ChatSlot {
    host: Arc<Host>,
    key: String,
}

impl Drop for ChatSlot {
    fn drop(&mut self) {
        self.host.design_chatting.lock().unwrap().remove(&self.key);
    }
}

/// 기획 branch 하나의 대화
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Chat {
    pub thread: Option<String>,
    /// 아직 보내지 못한 말(차례대로)
    pub pending: Vec<String>,
    /// thread를 띄우지 못한 까닭(다음 말에 지운다)
    pub error: Option<String>,
    /// 마지막으로 말한 때(쉼을 정할 때 마지막 저장과 함께 본다)
    pub said_ms: u64,
    /// 버린 대화: 띄우는 중이던 배경 일이 끝나면 이것을 보고 그 thread를 끝낸다. 다시 말하면 지운다.
    /// 같은 주제의 기획 branch를 새로 열면 기록째 지운다(`design_chat_forget`)
    pub retired: bool,
    /// 대화 thread profile(새 대화 창의 모델, 없으면 프로젝트 기본)
    pub profile: Option<String>,
    /// 대화에 끼우는 host 사건(오래된 차례, 상한 `MAX_EVENTS`)
    pub events: Vec<aaa_proto::DesignEvent>,
    /// 사람이 고쳤고 아직 알리지 않은 문서(다음 말 앞에 「(문서 고침: …)」으로 붙는다)
    pub edited: Vec<String>,
    /// 마지막으로 보낸 말의 앞부분(그 뒤 사건을 그 말의 답 뒤에 끼운다)
    pub last_said: Option<String>,
}

/// 대화 사건을 남기는 상한(오래된 것부터 버린다)
const MAX_EVENTS: usize = 300;
/// 사건 자리를 찾는 보낸 말 앞부분 길이(글자)
const ANCHOR_CHARS: usize = 80;

/// `<저장소>/<주제>`
fn chat_key(repo: &str, slug: &str) -> String {
    format!("{repo}/{slug}")
}

/// thread에 줄 할 일 글(brief 끝)
fn task_text(
    repo: &str,
    slug: &str,
    title: &str,
    folder: &str,
    cwd: Option<&str>,
    repo_path: &str,
    again: bool,
) -> String {
    let start = if again {
        "앞 대화를 이을 수 없어 새로 시작했다. 기획 branch의 문서가 지금까지의 정리본이다: 먼저 읽고 사용자의 다음 말을 기다린다."
    } else {
        "새 기획 대화다. 기획 폴더를 훑어 읽고 사용자의 말을 기다린다."
    };
    format!(
        "저장소 {repo}의 기획 branch `design/{slug}`(주제 「{title}」) 기획 대화 thread다. {start}\n\n\
         - 기획 폴더(저장소 기준): `{folder}/`\n\
         - 기획 worktree(이 branch, 읽기만): {}\n\
         - 저장소 checkout(코드 읽기만): {repo_path}\n\
         - 목록 `aaa plan list`, 읽기 `aaa plan show {repo} <경로> --branch {slug}`\n\
         - 저장 `aaa plan save {repo} <경로> --branch {slug} --base <blob> --file <파일> --summary '<바뀐 곳>'`(새 문서는 `--new`)\n",
        cwd.unwrap_or("(아직 없다: `aaa plan show`로 읽는다)")
    )
}

impl Host {
    fn design_chat_path(&self, project: &str, repo: &str, slug: &str) -> Result<PathBuf, ErrorBody> {
        // 기획 branch 기록과 같은 이름 검사(저장소 이름·주제)를 거친다
        self.design_branch_load(project, repo, slug)?;
        Ok(self.state.join("design-chats").join(project).join(repo).join(format!("{slug}.json")))
    }

    pub(crate) fn design_chat_load(&self, project: &str, repo: &str, slug: &str) -> Result<Chat, ErrorBody> {
        let path = self.design_chat_path(project, repo, slug)?;
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| internal(format!("{}: {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Chat::default()),
            Err(e) => Err(internal(format!("{}: {e}", path.display()))),
        }
    }

    pub(crate) fn design_chat_store(
        &self,
        project: &str,
        repo: &str,
        slug: &str,
        chat: &Chat,
    ) -> Result<(), ErrorBody> {
        let path = self.design_chat_path(project, repo, slug)?;
        let dir = path.parent().ok_or_else(|| internal("대화 기록 폴더가 없다"))?;
        std::fs::create_dir_all(dir).map_err(|e| internal(format!("{}: {e}", dir.display())))?;
        let bytes = serde_json::to_vec_pretty(chat).map_err(internal)?;
        crate::orch::write_atomic(&path, &bytes).map_err(internal)
    }

    fn design_chat_lock(&self, project: &str, repo: &str, slug: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.thread_op_lock(project, &format!("design-chat:{repo}/{slug}"))
    }

    /// 대화 기록을 잠그고 고친다
    pub(crate) async fn design_chat_edit<T>(
        &self,
        project: &str,
        repo: &str,
        slug: &str,
        edit: impl FnOnce(&mut Chat) -> T,
    ) -> Result<T, ErrorBody> {
        let lock = self.design_chat_lock(project, repo, slug);
        let _one = lock.lock().await;
        let mut chat = self.design_chat_load(project, repo, slug)?;
        let out = edit(&mut chat);
        self.design_chat_store(project, repo, slug, &chat)?;
        Ok(out)
    }

    /// 대화 사건 한 줄을 남긴다(지금 보낸 마지막 말 뒤). 못 남겨도 대화는 그대로다
    pub(crate) async fn design_chat_event(
        &self,
        project: &str,
        repo: &str,
        slug: &str,
        kind: &str,
        text: &str,
        commit: Option<String>,
    ) {
        let event = |c: &mut Chat| {
            c.events.push(aaa_proto::DesignEvent {
                kind: kind.to_string(),
                text: text.to_string(),
                commit,
                after: c.last_said.clone(),
                at_ms: aaa_common::util::now_ms(),
            });
            let over = c.events.len().saturating_sub(MAX_EVENTS);
            c.events.drain(..over);
        };
        aaa_common::diagnostic::ignored(
            self.design_chat_edit(project, repo, slug, event).await,
            "대화 사건 한 줄만 빠진다",
        );
    }

    /// 보낼 말이 남았나(`<저장소>/<주제>`, 쉼 판정)
    pub(crate) fn design_chat_waiting(&self, project: &str, key: &str) -> bool {
        key.split_once('/')
            .and_then(|(repo, slug)| self.design_chat_load(project, repo, slug).ok())
            .is_some_and(|c| !c.pending.is_empty())
    }

    /// 사람이 기획 대화에 말한다: 말을 남기고, thread를 띄우거나 이어 brief 뒤 보낸다(배경)
    pub(crate) async fn design_say(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        project: &str,
        repo: &str,
        slug: &str,
        text: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        self.design_writer(role, actor, project, None)?;
        if text.trim().is_empty() {
            return Err(bad("글이 비었다"));
        }
        if text.len() > MAX_SAY {
            return Err(err(ErrorCode::TooLarge, format!("한 번에 {MAX_SAY}바이트까지 보낸다")));
        }
        let (target, _) = self.design_target(project, repo)?;
        // branch 잠금 안에서 남긴다: 버리기와 엇갈리면 버리기 전(버리기가 이 말도 버린다)이거나 뒤(없다)다
        let rec = {
            let lock = self.design_branch_lock(project, &target.name, slug);
            let _one = lock.lock().await;
            let rec = self
                .design_branch_load(project, &target.name, slug)?
                .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{slug}(먼저 연다)")))?;
            self.design_chat_edit(project, &target.name, slug, |c| {
                c.pending.push(text.to_string());
                c.error = None;
                c.retired = false;
                c.said_ms = aaa_common::util::now_ms();
            })
            .await?;
            rec
        };
        self.db.audit(actor, "design_say", &format!("{project} {}/{slug} {} bytes", target.name, text.len()));
        self.spawn_design_chat(project, &target.name, slug, true, actor);
        Ok(FrontResponse::DesignBranch { branch: self.design_view(project, &rec) })
    }

    /// 배경 일: (쉬는 branch는 다시 열고) thread를 쓸 수 있게 하고(`start`면 없을 때 새로), 남은 말을 brief 뒤 차례로 보낸다
    pub(crate) fn spawn_design_chat(self: &Arc<Self>, project: &str, repo: &str, slug: &str, start: bool, actor: &str) {
        self.spawn_design_chat_try(project, repo, slug, start, actor, 0);
    }

    fn spawn_design_chat_try(
        self: &Arc<Self>,
        project: &str,
        repo: &str,
        slug: &str,
        start: bool,
        actor: &str,
        tries: u32,
    ) {
        let key = format!("{project}/{repo}/{slug}");
        if !self.design_chatting.lock().unwrap().insert(key.clone()) {
            // 도는 일이 끝난 뒤 남은 말을 보고 다시 돈다(아래)
            return;
        }
        let slot = ChatSlot { host: self.clone(), key };
        let (host, project, repo, slug, actor) =
            (self.clone(), project.to_string(), repo.to_string(), slug.to_string(), actor.to_string());
        tokio::spawn(async move {
            let sent = host.design_chat_work(&project, &repo, &slug, start, &actor).await;
            drop(slot);
            // 일하는 사이에 온 말, thread가 멈춰(보류·실패·끝남) 못 보낸 말이 남았으면 다시 돈다. 띄우지 못한 까닭이 있거나
            // 버린 대화는 다음 말을 기다린다. 하나도 보내지 못한 채 거듭 돌면 까닭을 남기고 멈춘다
            let Ok(chat) = host.design_chat_load(&project, &repo, &slug) else { return };
            if chat.pending.is_empty() || chat.error.is_some() || chat.retired {
                return;
            }
            let tries = if sent { 0 } else { tries + 1 };
            if tries >= MAX_TRIES {
                tracing::warn!(project, repo, slug, "기획 대화 thread가 거듭 말을 받지 못해 멈췄다");
                aaa_common::diagnostic::ignored(
                    host.design_chat_edit(&project, &repo, &slug, |c| {
                        c.error = Some("대화 thread가 거듭 말을 받지 못했다(다시 말하면 다시 한다)".into());
                    })
                    .await,
                    "까닭을 못 남겨도 로그에 있다",
                );
                return;
            }
            host.spawn_design_chat_try(&project, &repo, &slug, true, &actor, tries);
        });
    }

    /// 한 번 돈다. 말을 하나라도 보냈으면 참
    async fn design_chat_work(
        self: &Arc<Self>,
        project: &str,
        repo: &str,
        slug: &str,
        start: bool,
        actor: &str,
    ) -> bool {
        let ready = async {
            let rec = self
                .design_branch_load(project, repo, slug)?
                .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{slug}")))?;
            if rec.state == DesignBranchState::Rest {
                // 그 사이 버렸으면 되살리지 않는다
                self.design_branch_open_with(DeviceRole::Local, actor, project, repo, slug, None, None, false).await?;
            }
            self.design_chat_ready(project, repo, slug, actor, start).await
        }
        .await;
        match ready {
            Ok(Some(_)) => {}
            Ok(None) => return false,
            Err(e) => {
                tracing::warn!(project, repo, slug, "기획 대화 thread를 띄우지 못했다: {}", e.message);
                aaa_common::diagnostic::ignored(
                    self.design_chat_edit(project, repo, slug, |c| c.error = Some(e.message)).await,
                    "까닭을 못 남겨도 로그에 있다",
                );
                return false;
            }
        }
        let mut sent = false;
        if let Err(e) = tokio::time::timeout(DELIVER_WAIT, self.design_deliver(project, repo, slug, &mut sent)).await {
            tracing::warn!(project, repo, slug, "기획 대화 thread가 준비되지 않아 말을 남겨 두었다: {e}");
        }
        sent
    }

    /// 대화 thread를 쓸 수 있게 한다: 열린 것은 그대로, 쉬느라 내린 것은 잇고, 없거나 잇지 못하면(`start`일 때만) 새로 띄운다
    async fn design_chat_ready(
        self: &Arc<Self>,
        project: &str,
        repo: &str,
        slug: &str,
        actor: &str,
        start: bool,
    ) -> Result<Option<String>, ErrorBody> {
        let chat = self.design_chat_load(project, repo, slug)?;
        if chat.retired {
            return Ok(None);
        }
        let mut again = false;
        // 잇지 못한 보류 thread: 새 thread를 띄우면 끝낸다(남겨 두면 아무도 끝내지 않는다)
        let mut stale = None;
        if let Some(id) = chat.thread.as_deref() {
            match self.load(project, id) {
                Ok(rec) => match rec.status {
                    ThreadStatus::Open | ThreadStatus::Starting => return Ok(Some(id.to_string())),
                    ThreadStatus::Parked => {
                        // 보류는 상태를 쓴 뒤에도 세션을 닫을 때까지 thread 작업 잠금을 쥔다: 놓기를 기다려 잇는다
                        drop(tokio::time::timeout(END_WAIT, self.thread_op_lock(project, id).lock_owned()).await);
                        match self.thread_unpark(project, id, None, None, false, actor).await {
                            Ok(_) => return Ok(Some(id.to_string())),
                            Err(e) => {
                                tracing::warn!(
                                    project,
                                    thread = id,
                                    "기획 대화 thread를 잇지 못해 새로 띄운다: {}",
                                    e.message
                                );
                                again = true;
                                stale = Some(id.to_string());
                            }
                        }
                    }
                    ThreadStatus::Failed | ThreadStatus::Resolved => again = true,
                },
                Err(e) => {
                    tracing::warn!(
                        project,
                        thread = id,
                        "기획 대화 thread 기록을 읽지 못해 새로 띄운다: {}",
                        e.message
                    );
                    again = true;
                }
            }
        }
        if !start {
            return Ok(None);
        }
        let (target, mid) = self.design_target(project, repo)?;
        self.design_chat_machine_ok(&mid)?;
        let rec = self
            .design_branch_load(project, repo, slug)?
            .ok_or_else(|| err(ErrorCode::NotFound, format!("기획 branch가 없다: design/{slug}")))?;
        let folder = target.folder.clone().unwrap_or_default();
        let args = StartArgs {
            project: project.to_string(),
            title: format!("기획 {repo} {}", rec.title),
            // 그 저장소 머신(기획 worktree와 코드를 거기서 읽는다)
            machine: target.machine.clone(),
            profile: chat.profile.clone().unwrap_or_default(),
            kind: ThreadKind::Dir,
            task: task_text(repo, slug, &rec.title, &folder, rec.cwd.as_deref(), &target.path, again),
            role: Role::Worker,
            design: Some(chat_key(repo, slug)),
            ..Default::default()
        };
        if let Some(old) = stale.as_deref()
            && let Err(e) = self.design_chat_end(project, old, actor).await
        {
            tracing::warn!(project, thread = old, "잇지 못한 기획 대화 thread를 끝내지 못했다: {}", e.message);
        }
        let view = self.thread_start_mode(args, actor, true).await?;
        let kept = self
            .design_chat_edit(project, repo, slug, |c| {
                if c.retired {
                    return false;
                }
                c.thread = Some(view.id.clone());
                true
            })
            .await?;
        if !kept {
            // 띄우는 사이에 버렸다: 버린 branch에 쓰지 않게 방금 띄운 thread를 끝낸다
            tracing::info!(project, repo, slug, thread = %view.id, "띄우는 사이에 버린 기획 대화의 thread를 끝낸다");
            self.design_chat_end(project, &view.id, actor).await?;
            return Ok(None);
        }
        Ok(Some(view.id))
    }

    /// 대화 thread를 띄울 머신: host 머신은 front.sock으로 쓰고, 다른 머신은 저장 원격 전달을 아는 agent여야 한다
    /// (옛 판이면 thread가 떠도 저장이 「host가 없다」로 실패한다)
    fn design_chat_machine_ok(&self, mid: &str) -> Result<(), ErrorBody> {
        if self.machine_is_local(mid) || self.machine_feature(mid, CHAT_FEATURE) {
            return Ok(());
        }
        Err(err(
            ErrorCode::Unavailable,
            format!(
                "{} agent가 기획 대화 저장을 host에 전달하지 못한다({CHAT_FEATURE} 없음). 그 머신 aaa를 갱신한다",
                self.machine_name(mid)
            ),
        ))
    }

    /// 남은 말을 차례로 보낸다(보내면 `sent`를 참으로). thread가 열림·뜨는 중이 아니면(보류·실패·끝남) 돌아간다: 배경 일이
    /// 다시 잇거나 띄운다
    async fn design_deliver(self: &Arc<Self>, project: &str, repo: &str, slug: &str, sent: &mut bool) {
        let mut events = self.thread_events.subscribe();
        loop {
            let Ok(chat) = self.design_chat_load(project, repo, slug) else { return };
            if chat.retired {
                return;
            }
            let Some(next) = chat.pending.first().cloned() else { return };
            let Some(thread) = chat.thread.clone() else { return };
            let ready = match self.load(project, &thread) {
                Ok(r) if r.status == ThreadStatus::Open => r.brief == aaa_proto::BriefState::Delivered,
                Ok(r) if r.status == ThreadStatus::Starting => false,
                _ => return,
            };
            if ready {
                // 사람이 고친 문서는 다음 말 앞에 알린다(thread가 다시 읽고 잇는다)
                let told = chat.edited.clone();
                let text = if told.is_empty() {
                    next.clone()
                } else {
                    format!("(문서 고침: {})\n{next}", told.join(", "))
                };
                // 보내기 전에 자리를 남긴다: thread가 이 말을 받자마자 저장하면 그 사건이 이 말 뒤에 놓여야 한다
                let anchor: String = text.chars().take(ANCHOR_CHARS).collect();
                aaa_common::diagnostic::ignored(
                    self.design_chat_edit(project, repo, slug, |c| c.last_said = Some(anchor)).await,
                    "사건 자리만 앞 말에 붙는다",
                );
                match self.thread_message(project, &thread, text.clone(), MessageMode::Queue, "design").await {
                    Ok(_) => {
                        *sent = true;
                        aaa_common::diagnostic::ignored(
                            self.design_chat_edit(project, repo, slug, |c| {
                                if c.pending.first() == Some(&next) {
                                    c.pending.remove(0);
                                }
                                c.edited.retain(|d| !told.contains(d));
                            })
                            .await,
                            "다음 셈에 다시 보낸다",
                        );
                        continue;
                    }
                    // 확인창·쓰던 글 같은 막힘: 다음 thread 사건 뒤에 다시 본다
                    Err(e) => tracing::info!(project, thread, "기획 대화 말을 아직 보내지 못했다: {}", e.message),
                }
            }
            match events.recv().await {
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    }

    /// ticker: host가 다시 떠 끊긴 전달을 잇는다(남은 말이 있고 thread가 있는 대화)
    pub(crate) fn design_chat_resume(self: &Arc<Self>, project: &str, repo: &str, slug: &str) {
        if self.design_chat_load(project, repo, slug).is_ok_and(|c| !c.pending.is_empty() && c.thread.is_some()) {
            self.spawn_design_chat(project, repo, slug, false, "ticker");
        }
    }

    /// 버리기: 표지를 남기고(보낼 말은 버린다) 대화 thread를 알림 없이 끝낸다(대화 전용 thread라 알림함 항목이 없다).
    /// 끝내지 못하면 thread를 기록에 남겨 다시 버리거나 이어 말할 수 있다
    pub(crate) async fn design_chat_retire(
        self: &Arc<Self>,
        project: &str,
        repo: &str,
        slug: &str,
        actor: &str,
    ) -> Result<(), ErrorBody> {
        let thread = self
            .design_chat_edit(project, repo, slug, |c| {
                c.retired = true;
                c.pending.clear();
                c.error = None;
                c.thread.clone()
            })
            .await?;
        if let Some(id) = thread.as_deref() {
            self.design_chat_end(project, id, actor).await?;
        }
        self.design_chat_edit(project, repo, slug, |c| {
            if c.thread == thread {
                c.thread = None;
            }
        })
        .await
    }

    /// 대화 thread를 끝낸다. 띄우는 중이면(기동 일이 thread 작업 잠금을 쥐고 있다, 상태가 열림이 된 뒤에도 잠시) 놓기를
    /// 기다려 끝낸다
    async fn design_chat_end(self: &Arc<Self>, project: &str, id: &str, actor: &str) -> Result<(), ErrorBody> {
        let deadline = tokio::time::Instant::now() + END_WAIT;
        let lock = self.thread_op_lock(project, id);
        loop {
            let Ok(rec) = self.load(project, id) else { return Ok(()) };
            if matches!(rec.status, ThreadStatus::Resolved | ThreadStatus::Failed) {
                return Ok(());
            }
            // 기다리기만 하고 바로 놓는다: thread_resolve가 스스로 잡는다
            drop(tokio::time::timeout_at(deadline, lock.clone().lock_owned()).await);
            // thread 끝내기 future(약 16KiB)는 heap에 둔다: 이 길이 버리기 요청(공용 dispatcher의 `call_design`)에 있어 그대로
            // 품으면 모든 요청의 poll 스택이 함께 커진다(Windows 시험 스택 넘침, t-1894)
            match Box::pin(self.thread_resolve(project, id, ResolveOpts::default(), ResolveWhy::Manual, actor)).await {
                Ok(_) => return Ok(()),
                Err(e) if lock.try_lock().is_err() && tokio::time::Instant::now() < deadline => {
                    tracing::info!(
                        project,
                        thread = id,
                        "기획 대화 thread가 아직 다른 작업 중이라 다시 끝낸다: {}",
                        e.message
                    );
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// 같은 주제의 기획 branch를 새로 연다: 앞서 버린 대화의 기록(표지·끝난 thread)을 잇지 않는다
    pub(crate) async fn design_chat_forget(&self, project: &str, repo: &str, slug: &str) -> Result<(), ErrorBody> {
        let lock = self.design_chat_lock(project, repo, slug);
        let _one = lock.lock().await;
        let path = self.design_chat_path(project, repo, slug)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(internal(format!("{}: {e}", path.display()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 다른 머신은 저장 원격 전달을 아는 agent에서만 대화 thread를 띄운다(host 머신은 front.sock이라 묻지 않는다)
    #[test]
    fn chat_threads_start_only_where_saves_can_reach_the_host() {
        let root = std::env::temp_dir().join(format!("aaa-design-chat-{}", aaa_common::util::new_id()));
        let mut cfg = aaa_config::HostConfig::default();
        cfg.projects.root = root.join("projects").display().to_string();
        cfg.notify.web_push = false;
        let h = Host::open(cfg, root.join("host")).unwrap();
        for (id, local, features) in [
            ("host-m", true, vec![]),
            ("old", false, vec!["design_write.v1".to_string()]),
            ("new", false, vec!["design_write.v1".to_string(), CHAT_FEATURE.to_string()]),
        ] {
            h.inner.lock().unwrap().machines.insert(
                id.into(),
                crate::MachineRt {
                    name: format!("lab-{id}"),
                    local,
                    os: None,
                    boot_id: None,
                    dev_mode: true,
                    conn: None,
                    last_seen_ms: None,
                    health: None,
                    health_sent: None,
                    features,
                    identity: None,
                    agent_pid: None,
                    colors: None,
                    build: Default::default(),
                },
            );
        }
        assert!(h.design_chat_machine_ok("host-m").is_ok() && h.design_chat_machine_ok("new").is_ok());
        let e = h.design_chat_machine_ok("old").unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable);
        assert!(e.message.contains("lab-old") && e.message.contains(CHAT_FEATURE), "{}", e.message);
        drop(h);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn task_text_points_at_branch_worktree_and_commands() {
        let t = task_text("app", "boss-dex", "보스 도감", "docs", Some("/w/app/demo-design-boss-dex"), "/r/app", false);
        for want in [
            "design/boss-dex",
            "「보스 도감」",
            "`docs/`",
            "/w/app/demo-design-boss-dex",
            "/r/app",
            "aaa plan show app <경로> --branch boss-dex",
            "--branch boss-dex --base <blob>",
        ] {
            assert!(t.contains(want), "{want}: {t}");
        }
        assert!(task_text("app", "x", "x", "docs", None, "/r", true).contains("앞 대화를 이을 수 없어"));
    }
}
