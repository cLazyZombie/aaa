//! thread 보류(plan §8.426, t-0689): 자리·pane을 돌려주고 worktree·대화 ID·보고를 남겨 나중에 이어 한다.
//! - 보류: 기본은 thread에게 정리(커밋·push·`## Handoff`, 첫 줄 `[보류 준비됨]`)를 맡기고, 그 보고가 오고 agent가 쉬면 ticker가
//!   닫는다(`park_pass`). `now`는 바로 닫는다. 닫을 때 마지막 사본·git 상태(HEAD·origin의 같은 branch·미커밋)·대화 ID를 적고
//!   pane을 지운다. 상태가 Open이 아니므로 실행 자리·auto_resolve·병합 뒤 끝내기·task 자동 정리에서 빠진다.
//! - 이어 하기: 같은 머신은 저장한 대화를 잇고 고유한 한 줄(`brief_line`)로 알린다(잇지 못하는 harness는 새 대화와 요약 brief).
//!   다른 머신은 push된 branch(`origin/<branch>`)로 새 worktree를 만들고 할 일·마지막 보고를 담은 brief로 새 대화를 띄운다.
//!   앞 자리는 지우지 않고 `left_places`에 둔다. 그 머신으로 돌아가면 그 자리에서 origin을 받으라는 brief로 새 대화를 띄운다.
//! - 누가: 사용자(조작 기기·세션 밖 CLI)와 사용자 지시를 인용한 그 프로젝트 coordinator(4A, §8.397과 같은 규칙).

use std::path::Path;
use std::sync::Arc;

use aaa_proto::{
    AgentActivity, AgentRequest, AgentResponse, DiffQuery, DiffScope, ErrorBody, ErrorCode, FrontResponse, GitQuery,
    MessageMode, Role, ThreadGroup, ThreadKind, ThreadPark, ThreadStatus, ThreadView, UnparkOption, UnparkPreview,
};
use serde::{Deserialize, Serialize};

use crate::Host;
use crate::err;
use crate::orch::{ThreadRec, bad, internal};

/// thread가 정리를 마쳤다고 쓰는 보고 첫 줄 표지
pub(crate) const READY: &str = "[보류 준비됨]";
/// brief에 싣는 마지막 보고의 상한(글자)
const REPORT_CAP: usize = 20_000;

/// 다른 머신으로 옮길 때 남긴 앞 자리. 그 머신으로 돌아가면 이 자리에서 다시 띄운다
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LeftPlace {
    pub machine_id: String,
    pub repo: Option<String>,
    pub cwd: Option<String>,
    pub work_dir: Option<String>,
    pub branch: Option<String>,
    pub launch: Option<aaa_proto::Launch>,
}

/// 같은 머신은 같은 대화(잇는 방법을 알 때), 다른 머신은 새 대화다
#[derive(Debug, PartialEq)]
enum Way {
    Same {
        resume: bool,
    },
    /// 다른 머신의 새 worktree(작업 폴더 thread는 새 폴더). 저장소 경로
    Move {
        repo: Option<String>,
    },
    /// 앞에 쓰던 자리로 돌아간다(`left_places` 차례)
    Back {
        index: usize,
    },
}

/// 보류할 수 있는 thread인가(사람이 만든 일반 thread만)
pub(crate) fn parkable(r: &ThreadRec) -> Result<(), String> {
    if r.role != Role::Worker || r.automation.is_some() {
        return Err("사람·coordinator가 시작한 일반 thread만 보류한다(평가자·자동화 thread는 아니다)".into());
    }
    if r.kind == ThreadKind::Adopted {
        return Err("데려온 thread는 사람의 pane이라 보류하지 않는다".into());
    }
    if r.resolve_pending.is_some() {
        return Err("정리 중이다. 먼저 같은 resolve로 마친다".into());
    }
    match r.status {
        ThreadStatus::Open => Ok(()),
        ThreadStatus::Parked => Err("이미 보류했다".into()),
        ThreadStatus::Starting => Err("아직 시작 중이다".into()),
        ThreadStatus::Failed => Err("시작하지 못한 thread는 남길 것이 없다. restart나 resolve를 한다".into()),
        ThreadStatus::Resolved => Err("끝낸 thread다".into()),
    }
}

/// 맡긴 정리가 끝났나: 맡긴 뒤의 새 보고가 `[보류 준비됨]`이고, agent가 일하거나 확인창을 기다리지 않는다
pub(crate) fn ready_to_close(r: &ThreadRec, report: &str, agent: Option<AgentActivity>) -> bool {
    let Some(p) = r.park.as_ref().filter(|p| p.parked_at_ms.is_none()) else { return false };
    r.report_hash.is_some()
        && r.report_hash != p.report_before
        && crate::report_mark::marker(report) == Some(READY)
        && !matches!(agent, Some(AgentActivity::Working | AgentActivity::Question))
}

/// 같은 대화를 잇는 방법을 아는가: 대화 ID 자리가 있는 resume argv와 UUID 꼴 대화 ID(재부팅 복원 restore.rs와 같은 판정).
/// 대화 ID 자리가 없는 resume(custom 등)은 같은 명령을 다시 돌릴 뿐이라 새 대화와 요약 brief로 잇는다
fn resumable(launch: Option<&aaa_proto::Launch>) -> bool {
    launch.is_some_and(|l| {
        l.resume_argv.iter().any(|a| a.contains("{session}"))
            && l.session_key.as_deref().is_some_and(aaa_common::util::harness_session_ok)
    })
}

fn one_line(what: &str, text: Option<String>) -> Result<Option<String>, ErrorBody> {
    let text = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if text.as_deref().is_some_and(|t| t.len() > 2000 || t.chars().any(char::is_control)) {
        return Err(bad(format!("{what}은 한 줄(2000바이트 이하)로 쓴다")));
    }
    Ok(text)
}

/// thread에게 보내는 정리 부탁(한 덩어리, 보고 형식은 brief 안내와 같다)
fn request_text(project: &str, id: &str, note: Option<&str>) -> String {
    let note = note.map(|n| format!(" 까닭: {n}.")).unwrap_or_default();
    format!(
        "[aaa 보류 준비] 사용자가 {id}를 보류한다(자리·pane을 돌려주고 나중에 이 머신이나 다른 머신에서 이어 한다).{note} \
         지금 하던 것을 안전한 곳에서 멈추고 정리한다: (1) 바꾼 것을 커밋한다(임시면 WIP 커밋). (2) thread branch를 origin에 push한다 \
         (공개 저장소는 공개 검사를 먼저 하고, force push는 하지 않는다. push할 수 없으면 까닭을 적는다). \
         (3) 보고(.aaa/{project}-{id}/report.md)를 다시 쓴다: 첫 줄은 `{READY} <한 줄 요약>`, `## Handoff` 절에 지금 상태·다음에 할 것·\
         주의할 것·관련 파일과 커밋을 적는다. 이어 받는 사람은 앞 대화를 못 볼 수 있다. 그 뒤 아무것도 하지 않고 기다린다(aaa가 pane을 닫는다)."
    )
}

fn base_name(path: &str) -> Option<&std::ffi::OsStr> {
    Path::new(path.trim_end_matches('/')).file_name()
}

impl Host {
    /// 보류를 요청한 사람이 할 수 있나. coordinator면 인용한 사용자 지시를 돌려준다
    fn park_actor(
        &self,
        project: &str,
        actor: &str,
        user_request: Option<String>,
    ) -> Result<Option<String>, ErrorBody> {
        let reason = one_line("사용자 지시 근거", user_request)?;
        if self.coordinator_project_of_actor(actor).as_deref() == Some(project) {
            if reason.is_none() {
                return Err(err(
                    ErrorCode::Forbidden,
                    "coordinator는 사용자가 그 thread의 보류를 말했을 때만 --user-request '<사용자 지시>'와 함께 보류한다",
                ));
            }
            return Ok(reason);
        }
        if actor.starts_with("device:") || (actor.starts_with("local:") && !crate::front::in_session(actor)) {
            return Ok(reason);
        }
        Err(err(
            ErrorCode::Forbidden,
            "보류는 사용자 직접 조작이나 그 프로젝트 coordinator의 사용자 지시 인용만 허용한다",
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn thread_park(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        now: bool,
        note: Option<String>,
        user_request: Option<String>,
        cancel: bool,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let user_request = self.park_actor(project, actor, user_request)?;
        let note = one_line("까닭", note)?;
        if cancel {
            let _guard = self.claim_thread(project, id)?;
            let rec = self.load(project, id)?;
            if rec.status != ThreadStatus::Open || rec.park.as_ref().is_none_or(|p| p.parked_at_ms.is_some()) {
                return Err(bad(format!("{id}에 거둘 보류 요청이 없다")));
            }
            let rec = self.update_rec(project, id, |r| r.park = None)?;
            self.db.audit(actor, "thread_park_cancel", &format!("{project}/{id}"));
            self.flow(&rec, "thread_park_cancelled", None, "보류 요청을 거뒀다");
            return Ok(self.thread_view(&rec));
        }
        let rec = self.load(project, id)?;
        parkable(&rec).map_err(|why| bad(format!("{id}: {why}")))?;
        let request = ThreadPark {
            actor: actor.into(),
            requested_at_ms: aaa_common::util::now_ms(),
            note,
            user_request,
            immediate: now,
            report_before: rec.report_hash.clone(),
            parked_at_ms: None,
            machine: self.machine_name(&rec.machine_id),
            head: None,
            pushed: None,
            uncommitted: None,
            git_unknown: None,
            resumable: false,
        };
        if now {
            return self.park_close(project, id, Some(request), actor).await;
        }
        if rec.park.is_some() {
            return Err(bad(format!(
                "{id}에 이미 정리를 맡겼다. 기다리거나 `aaa thread park {project} {id} --now`(바로)·`--cancel`(거두기)"
            )));
        }
        let (live, _) = self.thread_live(&rec);
        if !live {
            return Err(bad(format!(
                "{id}의 세션이 없어 정리를 맡길 수 없다. `aaa thread park {project} {id} --now`로 바로 보류한다"
            )));
        }
        let text = request_text(project, id, request.note.as_deref());
        let rec = self.update_rec(project, id, |r| r.park = Some(request.clone()))?;
        if let Err(e) = self.thread_message(project, id, text, MessageMode::Queue, actor).await {
            aaa_common::diagnostic::ignored(
                self.update_rec(project, id, |r| r.park = None),
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
            return Err(ErrorBody {
                message: format!(
                    "정리 부탁을 보내지 못해 보류하지 않았다: {}. 응답이 없으면 `aaa thread park {project} {id} --now`",
                    e.message
                ),
                ..e
            });
        }
        self.db.audit(actor, "thread_park_request", &format!("{project}/{id}"));
        self.flow(&rec, "thread_park_requested", None, "정리(커밋·push·Handoff)를 맡겼다");
        Ok(self.thread_view(&self.load(project, id).unwrap_or(rec)))
    }

    /// pane을 닫고 보류로 둔다. `request`가 없으면 맡긴 정리의 기록을 쓴다
    pub(crate) async fn park_close(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        request: Option<ThreadPark>,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let _guard = self.claim_thread(project, id)?;
        let rec = self.load(project, id)?;
        parkable(&rec).map_err(|why| bad(format!("{id}: {why}")))?;
        let mut park = request.or_else(|| rec.park.clone()).ok_or_else(|| bad(format!("{id}에 보류 요청이 없다")))?;
        let conn = self.live_conn(&rec.machine_id)?;
        let mut notes = Vec::new();
        // 마지막 사본(resolve와 같다). 실패해도 worktree가 남으므로 보류는 한다
        if let Some(w) = &rec.work_dir
            && matches!(
                conn.request(AgentRequest::ReportCollect { work_dir: w.clone(), max_library_bytes: 0 }).await,
                Ok(AgentResponse::Report(_))
            )
        {
            let lock = self.report_collect_lock(project, id);
            let _collect = lock.lock().await;
            if let Err(e) = self.collect(project, id, true).await {
                notes.push(format!("마지막 사본 실패(worktree에 남는다): {}", e.message));
            }
        }
        self.park_git(&rec, &conn, &mut park).await;
        // 대화 ID: hook이 알린 값이 aaa가 준 값보다 새롭다(정리·/clear 뒤 바뀐다). pane을 지우면 잃으므로 launch에 옮긴다
        let key = rec
            .pane
            .as_deref()
            .and_then(|p| self.pane_agent(p))
            .and_then(|a| a.harness_session)
            .filter(|k| aaa_common::util::harness_session_ok(k));
        let mut launch = rec.launch.clone();
        if let (Some(l), Some(k)) = (launch.as_mut(), key) {
            l.session_key = Some(k);
        }
        let now = aaa_common::util::now_ms();
        park.resumable = resumable(launch.as_ref());
        park.machine = self.machine_name(&rec.machine_id);
        // 끝남처럼 상태를 먼저 바꾼다: pane이 끝날 때 「세션이 끝났다」 항목·알림을 내지 않는다. 닫기를 다 마친 때(`parked_at_ms`)는
        // 맨 끝에 적는다: 그 전에는 「닫는 중」이고 이 잠금이 이어 하기를 막는다(끊겼으면 잠금이 풀려 이어 하기가 된다)
        let rec = self.update_rec(project, id, |r| {
            r.status = ThreadStatus::Parked;
            r.park = Some(park.clone());
            r.launch = launch.clone();
            r.brief_line = None;
            r.error = None;
            r.last_group = Some(ThreadGroup::Parked);
            r.group_changed_ms = Some(now);
            r.merged_seen_ms = None;
        })?;
        let pane_rx = rec.pane.as_deref().and_then(|p| {
            self.pane_events(p)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
        });
        let mut forgot = false;
        if let Some(pane) = rec.pane.as_ref()
            && self.pane_view_of(pane, aaa_proto::DeviceRole::Local).is_some_and(|p| p.state.is_running())
        {
            aaa_common::diagnostic::ignored(
                self.kill_session(&conn, pane).await,
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
        }
        aaa_common::diagnostic::ignored(
            conn.request(AgentRequest::ThreadUnwatch { thread: format!("{project}/{id}") }).await,
            "보조 작업 실패 뒤 기존 처리를 계속한다",
        );
        if let Some(pane) = &rec.pane {
            let closed = match pane_rx {
                None => Ok(false),
                Some(rx) => match self
                    .wait_pane(pane, rx, |p| !p.info.state.is_running(), "끝내기를 보냈지만 세션이 끝나지 않았다")
                    .await
                {
                    Ok(()) => self.forget(pane, &rec.machine_id, actor).await.map(|_| true),
                    Err(e) => Err(e),
                },
            };
            forgot = closed
                .as_ref()
                .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                .is_ok();
            notes.push(match closed {
                Ok(true) => "pane을 닫았다".into(),
                Ok(false) => "pane은 이미 닫혔다".into(),
                Err(e) => format!("pane을 닫지 못했다: {}", e.message),
            });
        }
        notes.push(git_note(&park));
        if !park.resumable {
            notes.push("이 harness는 같은 대화를 잇지 못해 이어 하면 새 대화다".into());
        }
        self.db.audit(actor, "thread_park", &format!("{project}/{id}"));
        self.flow(&rec, "thread_parked", None, notes.join("; "));
        self.inbox_add(
            project,
            "thread-state",
            id,
            "parked",
            &format!(
                "{}: 보류했다(자리 반환). `aaa thread unpark {project} {id}`로 이어 한다: {}",
                crate::transitions::label(&rec),
                notes.join("; ")
            ),
        );
        let rec = self.update_rec(project, id, |r| {
            if let Some(p) = r.park.as_mut() {
                p.parked_at_ms = Some(now);
            }
            // 지운 pane을 가리키지 않는다(웹이 열지 않게). 대화 ID는 위에서 launch로 옮겼다
            if forgot {
                r.pane = None;
            }
        })?;
        self.reports_bump();
        // 예약이 돌려준 자리를 본다
        self.tick_wake.notify_one();
        Ok(self.thread_view(&rec))
    }

    /// 보류할 때의 git: 미커밋 수·HEAD(diff.v1), origin의 같은 branch(git.v1). 모르면 까닭을 남긴다
    async fn park_git(&self, rec: &ThreadRec, conn: &crate::AgentConn, park: &mut ThreadPark) {
        if rec.kind != ThreadKind::Worktree {
            return;
        }
        let (Some(cwd), Some(branch)) = (rec.cwd.as_deref(), rec.branch.as_deref()) else {
            park.git_unknown = Some("worktree·branch 기록이 없다".into());
            return;
        };
        let diff = self
            .thread_diff(&rec.project, &rec.id, DiffQuery { scope: DiffScope::Uncommitted, ..Default::default() })
            .await;
        let view = match diff {
            Ok(FrontResponse::ThreadDiff { diff, .. }) => diff,
            Ok(_) => {
                park.git_unknown = Some("변경 응답이 아니다".into());
                return;
            }
            Err(e) => {
                park.git_unknown = Some(e.message);
                return;
            }
        };
        park.uncommitted = crate::diff::diff_count(&view)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok();
        if view.base_sha.is_empty() {
            park.git_unknown = Some("커밋이 없다".into());
            return;
        }
        park.head = Some(view.base_sha.clone());
        if view.head != branch {
            park.pushed = Some(false);
            park.git_unknown = Some(format!("작업 트리 branch({})가 thread branch({branch})와 다르다", view.head));
            return;
        }
        if !self.machine_feature(&rec.machine_id, "git.v1") {
            park.git_unknown = Some("그 머신 agent가 git 조회(git.v1)를 모른다".into());
            return;
        }
        match self.git_ask(conn, cwd, GitQuery::Heads { branches: vec![branch.to_string()] }).await {
            Ok(a) => {
                park.pushed = Some(a.heads.get(branch).cloned().flatten().as_deref() == Some(view.base_sha.as_str()))
            }
            Err(e) => park.git_unknown = Some(format!("origin branch를 읽지 못했다: {}", e.message)),
        }
    }

    /// ticker: 맡긴 정리가 끝났거나 세션이 끝난 thread를 닫는다
    pub(crate) async fn park_pass(self: &Arc<Self>, project: &str) {
        let Ok(dir) = self
            .project_dir(project)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료 실패 뒤 기존 복구·거부를 따른다"))
        else {
            return;
        };
        let pending: Vec<ThreadRec> = self
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
            .filter(|r| r.status == ThreadStatus::Open && r.park.as_ref().is_some_and(|p| p.parked_at_ms.is_none()))
            .filter(|r| {
                self.live_conn(&r.machine_id)
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok()
            })
            .collect();
        for r in pending {
            let (live, agent) = self.thread_live(&r);
            let report = std::fs::read_to_string(dir.join("threads").join(format!("{}.md", r.id)))
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
                .unwrap_or_default();
            if (!live || ready_to_close(&r, &report, agent.map(|a| a.activity)))
                && let Err(e) = self.park_close(project, &r.id, None, "ticker").await
            {
                tracing::warn!(project, thread = %r.id, "보류 닫기를 다음 셈으로 미룬다: {}", e.message);
            }
        }
    }

    pub(crate) async fn thread_unpark(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        machine: Option<String>,
        repo: Option<String>,
        dry_run: bool,
        actor: &str,
    ) -> Result<FrontResponse, ErrorBody> {
        let rec = self.load(project, id)?;
        if rec.status != ThreadStatus::Parked {
            return Err(bad(format!("{id}는 보류한 thread가 아니다(지금 {})", rec.status.as_str())));
        }
        if dry_run {
            return Ok(FrontResponse::UnparkPreview(self.unpark_preview(&rec)));
        }
        let park = rec.park.clone().ok_or_else(|| internal("보류 기록이 없다"))?;
        let target = match machine.as_deref() {
            Some(m) => self.resolve_machine(Some(m))?,
            None => rec.machine_id.clone(),
        };
        let way = self.unpark_way(&rec, &park, &target, repo.as_deref()).map_err(bad)?;
        self.live_conn(&target)?;
        let before = rec.clone();
        let extra = match &way {
            Way::Same { resume: true } => None,
            _ => Some(self.handoff(&rec, &park, &target, &way)),
        };
        match &way {
            Way::Same { .. } => {}
            Way::Move { repo } => {
                let left = left_place(&rec);
                let base =
                    rec.branch.as_ref().filter(|_| rec.kind == ThreadKind::Worktree).map(|b| format!("origin/{b}"));
                let repo = repo.clone();
                self.update_rec(project, id, |r| {
                    r.left_places.push(left.clone());
                    r.machine_id = target.clone();
                    r.repo = repo.clone();
                    r.base = base.clone();
                    r.cwd = None;
                    r.work_dir = None;
                    r.launch = None;
                    r.pane = None;
                })?;
            }
            Way::Back { index } => {
                let back = rec.left_places[*index].clone();
                let left = left_place(&rec);
                self.update_rec(project, id, |r| {
                    r.left_places.remove(*index);
                    r.left_places.push(left.clone());
                    r.machine_id = back.machine_id.clone();
                    r.repo = back.repo.clone();
                    r.cwd = back.cwd.clone();
                    r.work_dir = back.work_dir.clone();
                    r.branch = back.branch.clone();
                    r.launch = back.launch.clone();
                    r.pane = None;
                })?;
            }
        }
        let resume = way == Way::Same { resume: true };
        let extra = extra.unwrap_or_default();
        let restarted = self.thread_restart_with(project, id, resume, None, actor, Some(&extra)).await;
        let view = match restarted {
            Ok(v) => v,
            Err(e) => {
                // 기동하지 못했다: 보류로 되돌린다(옮긴 자리도). 기동 결과를 모르면(시작 중) 그대로 둔다
                aaa_common::diagnostic::ignored(
                    self.update_rec(project, id, |r| {
                        if matches!(r.status, ThreadStatus::Failed | ThreadStatus::Parked) {
                            let mut back = before.clone();
                            back.error = Some(format!("이어 하기 실패: {}", e.message));
                            back.updated_at_ms = r.updated_at_ms;
                            *r = back;
                        }
                    }),
                    "보조 작업 실패 뒤 기존 처리를 계속한다",
                );
                self.tick_wake.notify_one();
                return Err(e);
            }
        };
        let line = format!("보류를 풀었다({id} 이어 하기). 마지막 보고의 Handoff대로 이어 간다.");
        let rec = self.update_rec(project, id, |r| {
            r.park = None;
            if resume {
                r.brief = aaa_proto::BriefState::Pending;
                r.brief_attempts = 0;
                r.brief_line = Some(line.clone());
            }
        })?;
        if resume {
            self.spawn_brief(project, id);
        }
        let detail = match &way {
            Way::Same { resume: true } => "같은 머신·같은 대화".to_string(),
            Way::Same { resume: false } => "같은 머신·새 대화(요약 brief)".to_string(),
            Way::Move { .. } | Way::Back { .. } => {
                format!("{}에서 새 대화(요약 brief)", self.machine_name(&rec.machine_id))
            }
        };
        self.db.audit(actor, "thread_unpark", &format!("{project}/{id} {detail}"));
        self.flow(&rec, "thread_unparked", None, detail);
        Ok(FrontResponse::Thread(Box::new(self.thread_view(&rec).with_warnings(view.warnings))))
    }

    /// 그 머신으로 어떻게 잇나. 막히면 까닭
    fn unpark_way(&self, r: &ThreadRec, park: &ThreadPark, target: &str, repo: Option<&str>) -> Result<Way, String> {
        if target == r.machine_id {
            if repo.is_some_and(|p| Some(p) != r.repo.as_deref()) {
                return Err("같은 머신은 보류한 자리에서 잇는다(--repo는 다른 머신에서만)".into());
            }
            return Ok(Way::Same { resume: park.resumable });
        }
        if let Some(index) = r.left_places.iter().position(|p| p.machine_id == target) {
            if r.kind == ThreadKind::Worktree && park.pushed != Some(true) {
                return Err(push_why(park));
            }
            return Ok(Way::Back { index });
        }
        if r.kind != ThreadKind::Worktree {
            return Ok(Way::Move { repo: None });
        }
        if park.pushed != Some(true) {
            return Err(push_why(park));
        }
        let settings = self.settings(&r.project);
        let here: Vec<String> = settings
            .repos
            .iter()
            .filter(|x| {
                self.resolve_machine(x.machine.as_deref())
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
                    .as_deref()
                    == Some(target)
            })
            .map(|x| x.path.clone())
            .collect();
        if let Some(p) = repo {
            return if here.iter().any(|x| x == p) {
                Ok(Way::Move { repo: Some(p.to_string()) })
            } else {
                Err(format!("{p}는 이 프로젝트의 그 머신 저장소가 아니다(project.toml repos)"))
            };
        }
        let name = r.repo.as_deref().and_then(base_name);
        let same: Vec<&String> = here.iter().filter(|x| name.is_some() && base_name(x) == name).collect();
        match same.as_slice() {
            [one] => Ok(Way::Move { repo: Some((*one).clone()) }),
            [] => Err(format!(
                "그 머신에 같은 이름의 저장소({})가 프로젝트 repos에 없다",
                name.map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            )),
            _ => Err("그 머신에 같은 이름의 저장소가 여럿이다. --repo로 고른다".into()),
        }
    }

    fn unpark_preview(&self, r: &ThreadRec) -> UnparkPreview {
        let park = r.park.clone().unwrap_or_else(|| ThreadPark {
            actor: String::new(),
            requested_at_ms: 0,
            note: None,
            user_request: None,
            immediate: false,
            report_before: None,
            parked_at_ms: None,
            machine: String::new(),
            head: None,
            pushed: None,
            uncommitted: None,
            git_unknown: None,
            resumable: false,
        });
        let mut machines: Vec<(String, String)> = {
            let i = self.inner.lock().unwrap();
            i.machines.iter().map(|(id, m)| (id.clone(), m.name.clone())).collect()
        };
        // 원래 머신 먼저, 나머지는 이름 차례
        machines.sort_by_key(|(id, name)| (id != &r.machine_id, name.clone()));
        let options = machines
            .into_iter()
            .map(|(mid, name)| {
                let connected = self
                    .live_conn(&mid)
                    .as_ref()
                    .inspect_err(|error| aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류"))
                    .is_ok();
                let way = self.unpark_way(r, &park, &mid, None);
                let (conversation, repo, mut blocked) = match way {
                    Ok(Way::Same { resume }) => (if resume { "resume" } else { "new" }, r.repo.clone(), None),
                    Ok(Way::Move { repo }) => ("new", repo, None),
                    Ok(Way::Back { index }) => ("new", r.left_places[index].repo.clone(), None),
                    Err(why) => ("new", None, Some(why)),
                };
                if !connected && blocked.is_none() {
                    blocked = Some("agent와 연결이 끊겨 있다".into());
                }
                UnparkOption {
                    machine: name,
                    same: mid == r.machine_id,
                    connected,
                    conversation: conversation.into(),
                    repo,
                    blocked,
                }
            })
            .collect();
        UnparkPreview {
            project: r.project.clone(),
            thread: r.id.clone(),
            options,
            open: self.open_worker_threads(&r.project),
            limit: self.settings(&r.project).max_parallel_threads,
        }
    }

    /// 새 대화 brief에 덧붙이는 이어 하기 안내와 마지막 보고
    fn handoff(&self, r: &ThreadRec, park: &ThreadPark, target: &str, way: &Way) -> String {
        let when = park.parked_at_ms.map(aaa_common::util::utc_text).unwrap_or_default();
        let note = park.note.as_deref().map(|n| format!(" 까닭: {n}.")).unwrap_or_default();
        let here = self.machine_name(target);
        let mut out = format!(
            "# 보류에서 이어 하기\n\n이 thread는 {when}에 `{}`에서 보류했다.{note} 지금 `{here}`에서 새 대화로 이어 한다. 앞 대화는 \
             이어지지 않는다: 아래 마지막 보고(특히 `## Handoff`)와 branch의 커밋이 이어 받을 전부다. 시작하면 보고 첫 줄을 \
             `[진행 중]`으로 바꾼다.\n",
            park.machine
        );
        let branch = r.branch.as_deref().unwrap_or("-");
        match way {
            Way::Same { .. } => out.push_str("- 보류한 같은 worktree·작업 폴더에서 잇는다(이 harness는 같은 대화를 잇지 못한다).\n"),
            Way::Move { .. } if r.kind == ThreadKind::Worktree => out.push_str(&format!(
                "- origin의 branch `{branch}`(HEAD `{}`)로 이 머신에 새 worktree를 만들었다. push는 `git push origin HEAD`로 한다. \
                 앞 머신의 worktree(`{}`)는 그대로 남아 있다(손대지 않는다).\n",
                park.head.as_deref().unwrap_or("-"),
                r.cwd.as_deref().unwrap_or("-")
            )),
            Way::Move { .. } => out.push_str(&format!(
                "- 작업 폴더 thread라 앞 머신 폴더(`{}`)의 파일은 옮겨지지 않았다. 보고에 적힌 것으로 다시 시작한다.\n",
                r.cwd.as_deref().unwrap_or("-")
            )),
            Way::Back { .. } => out.push_str(&format!(
                "- 이 머신에 앞에 쓰던 worktree로 돌아왔다. 다른 머신에서 더한 커밋을 먼저 받는다: \
                 `git fetch origin && git merge --ff-only origin/{branch}`. 받을 수 없으면 멈추고 보고한다.\n"
            )),
        }
        if !matches!(way, Way::Same { .. }) {
            let rid = r.report_hash.as_deref().and_then(|h| self.thread_report_id(&r.project, &r.id, Some(h)));
            out.push_str(&format!(
                "- 앞 보고의 library 파일은 host 보고서{}에 남아 있다. 이 작업 폴더의 library는 새로 쓴다.\n",
                rid.map(|id| format!(" {id}")).unwrap_or_default()
            ));
        }
        let report = self
            .project_dir(&r.project)
            .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다"))
            .ok()
            .and_then(|d| {
                std::fs::read_to_string(d.join("threads").join(format!("{}.md", r.id)))
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()
            })
            .unwrap_or_default();
        if report.trim().is_empty() {
            out.push_str("\n(마지막 보고가 없다)\n");
        } else {
            let mut cut: String = report.chars().take(REPORT_CAP).collect();
            if cut.len() < report.len() {
                cut.push_str("\n…(잘림)");
            }
            out.push_str(&format!("\n## 마지막 보고\n\n````markdown\n{}\n````\n", cut.trim_end()));
        }
        out
    }
}

fn left_place(r: &ThreadRec) -> LeftPlace {
    LeftPlace {
        machine_id: r.machine_id.clone(),
        repo: r.repo.clone(),
        cwd: r.cwd.clone(),
        work_dir: r.work_dir.clone(),
        branch: r.branch.clone(),
        launch: r.launch.clone(),
    }
}

fn push_why(park: &ThreadPark) -> String {
    match (park.pushed, park.git_unknown.as_deref()) {
        (_, Some(why)) => format!("다른 머신에서 이으려면 push된 branch가 필요하다: {why}"),
        (Some(false), None) => {
            "다른 머신에서 이으려면 push된 branch가 필요하다: 보류할 때 origin과 HEAD가 달랐다".into()
        }
        _ => "다른 머신에서 이으려면 push된 branch가 필요하다: 보류할 때 git을 읽지 못했다".into(),
    }
}

fn git_note(park: &ThreadPark) -> String {
    let head = park.head.as_deref().map(|h| &h[..h.len().min(8)]).unwrap_or("-");
    match (park.pushed, park.uncommitted, park.git_unknown.as_deref()) {
        (_, _, Some(why)) => format!("git: {why}"),
        (Some(true), Some(0), None) => format!("git: HEAD {head} push됨, 미커밋 없음"),
        (pushed, n, None) => format!(
            "git: HEAD {head} {}, 미커밋 {}",
            if pushed == Some(true) { "push됨" } else { "push 안 됨" },
            n.map(|n| n.to_string()).unwrap_or_else(|| "모름".into())
        ),
    }
}

trait WithWarnings {
    fn with_warnings(self, warnings: Vec<String>) -> Self;
}

impl WithWarnings for ThreadView {
    fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aaa_proto::BriefState;

    fn open() -> ThreadRec {
        let mut r = ThreadRec::starting("demo", "t", "m1", "claude", Role::Worker, ThreadKind::Worktree, 1);
        r.id = "t-0001".into();
        r.status = ThreadStatus::Open;
        r.brief = BriefState::Delivered;
        r
    }

    fn request(before: Option<&str>) -> ThreadPark {
        ThreadPark {
            actor: "device:x".into(),
            requested_at_ms: 1,
            note: None,
            user_request: None,
            immediate: false,
            report_before: before.map(str::to_string),
            parked_at_ms: None,
            machine: "m1".into(),
            head: None,
            pushed: None,
            uncommitted: None,
            git_unknown: None,
            resumable: false,
        }
    }

    #[test]
    fn only_plain_open_workers_park() {
        assert!(parkable(&open()).is_ok());
        for (r, why) in [
            (ThreadRec { status: ThreadStatus::Parked, ..open() }, "이미"),
            (ThreadRec { status: ThreadStatus::Starting, ..open() }, "시작 중"),
            (ThreadRec { status: ThreadStatus::Resolved, ..open() }, "끝낸"),
            (ThreadRec { kind: ThreadKind::Adopted, ..open() }, "데려온"),
            (ThreadRec { role: Role::Evaluator, ..open() }, "일반 thread"),
            (ThreadRec { automation: Some("nightly".into()), ..open() }, "일반 thread"),
        ] {
            assert!(parkable(&r).unwrap_err().contains(why), "{why}");
        }
    }

    /// 맡긴 뒤의 새 `[보류 준비됨]` 보고이고 agent가 일하지 않을 때만 닫는다
    #[test]
    fn closes_only_on_a_new_ready_report_from_a_resting_agent() {
        let ready = "[보류 준비됨] 커밋·push 끝\n\n## Handoff\n- 다음: 검수";
        let asked = ThreadRec { park: Some(request(Some("old"))), report_hash: Some("new".into()), ..open() };
        assert!(ready_to_close(&asked, ready, Some(AgentActivity::Idle)));
        assert!(ready_to_close(&asked, ready, None));
        assert!(!ready_to_close(&asked, ready, Some(AgentActivity::Working)));
        assert!(!ready_to_close(&asked, ready, Some(AgentActivity::Question)));
        assert!(!ready_to_close(&asked, "[진행 중] 정리 중", Some(AgentActivity::Idle)));
        assert!(!ready_to_close(&asked, "보고 [보류 준비됨]", Some(AgentActivity::Idle)));
        let same = ThreadRec { report_hash: Some("old".into()), ..asked.clone() };
        assert!(!ready_to_close(&same, ready, Some(AgentActivity::Idle)), "맡기기 전 보고");
        let none = ThreadRec { park: None, ..asked.clone() };
        assert!(!ready_to_close(&none, ready, Some(AgentActivity::Idle)));
        let mut parked = request(Some("old"));
        parked.parked_at_ms = Some(2);
        let done = ThreadRec { park: Some(parked), ..asked };
        assert!(!ready_to_close(&done, ready, Some(AgentActivity::Idle)), "이미 닫았다");
    }

    #[test]
    fn resumable_needs_a_way_and_a_valid_id() {
        let mut l = aaa_proto::Launch {
            profile: "claude".into(),
            harness: "claude".into(),
            argv: vec!["claude".into()],
            env: vec![],
            resume_argv: vec!["claude".into(), "--resume".into(), "{session}".into()],
            session_key: Some("0b6c3b9e-6a43-4c55-9a52-0f5f5d1f1a11".into()),
        };
        assert!(resumable(Some(&l)));
        l.session_key = Some("--dangerously-skip-permissions".into());
        assert!(!resumable(Some(&l)));
        l.session_key = None;
        assert!(!resumable(Some(&l)));
        l.session_key = Some("0b6c3b9e-6a43-4c55-9a52-0f5f5d1f1a11".into());
        l.resume_argv = vec!["/bin/sh".into(), "fake.sh".into()];
        assert!(!resumable(Some(&l)), "대화 ID 자리 없는 resume은 같은 명령을 다시 돌릴 뿐이다");
        l.resume_argv.clear();
        assert!(!resumable(Some(&l)));
        assert!(!resumable(None));
    }

    #[test]
    fn git_note_says_what_moves() {
        let mut p = request(None);
        p.head = Some("0123456789abcdef".into());
        p.pushed = Some(true);
        p.uncommitted = Some(0);
        assert_eq!(git_note(&p), "git: HEAD 01234567 push됨, 미커밋 없음");
        p.pushed = Some(false);
        p.uncommitted = Some(3);
        assert_eq!(git_note(&p), "git: HEAD 01234567 push 안 됨, 미커밋 3");
        assert!(push_why(&p).contains("origin과 HEAD가 달랐다"));
        p.git_unknown = Some("커밋이 없다".into());
        assert!(push_why(&p).contains("커밋이 없다"));
    }
}
