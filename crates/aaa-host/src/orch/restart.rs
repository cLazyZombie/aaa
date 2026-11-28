//! thread 멈춤과 다시 띄우기.

use super::*;

pub(crate) struct RestartRequest {
    pub resume: bool,
    pub profile: Option<String>,
    pub user_request: Option<String>,
    pub stop_running: bool,
}

struct ProfileRestart {
    change: aaa_proto::ThreadProfileChange,
    task: Option<aaa_proto::TaskView>,
}

/// `thread restart`가 할 일(herdr-projects threads.rs `RestartPlan`). aaa는 세션이 끝나면 pane도 끝나므로 참고의 (c)
/// 「셸에 있는 pane 다시 쓰기」는 없다.
#[derive(Debug, PartialEq)]
pub(super) enum RestartPlan {
    /// (a) 자리를 만들지 못했다(launch 기록이 없다): 자리 만들기부터 다시 한다
    Create,
    /// (e) 있는 자리에서 다시 띄운다(새 대화, `--resume`이면 이어서)
    Reopen,
}

/// herdr-projects threads.rs `restart_plan`. 시작 중(aaa는 시작에 시한이 없다, §8.40)·끝낸 thread와 (d) 실행 중인 세션은
/// 거부한다. (b) 기록된 worktree 없이 branch만 있는 것은 그 머신 agent가 자리를 만들 때 거부한다(host는 repo를 보지 않는다).
pub(super) fn restart_plan(
    rec: &ThreadRec,
    resume: bool,
    running: bool,
    unpark: bool,
) -> Result<RestartPlan, ErrorBody> {
    let id = &rec.id;
    match rec.status {
        ThreadStatus::Open | ThreadStatus::Failed => {}
        ThreadStatus::Parked if unpark => {}
        ThreadStatus::Parked => {
            return Err(bad(format!("{id}는 보류 중이다. `aaa thread unpark {} {id}`로 이어 한다", rec.project)));
        }
        ThreadStatus::Starting => return Err(bad(format!("{id}는 아직 시작 중이다"))),
        ThreadStatus::Resolved => {
            return Err(bad(format!("{id}는 끝냈다. 먼저 `aaa thread resolve {} {id} --reopen`", rec.project)));
        }
    }
    if running {
        let pane = rec.pane.as_deref().unwrap_or_default();
        return Err(bad(format!("세션이 아직 실행 중이다. 끝내려면 `aaa pane kill {pane}`")));
    }
    if rec.launch.is_some() {
        return Ok(RestartPlan::Reopen);
    }
    if resume {
        return Err(bad("시작하지 못한 thread는 이을 대화가 없다. --resume 없이 다시 시작한다"));
    }
    Ok(RestartPlan::Create)
}

impl Host {
    /// thread agent를 멈춘다(Esc 한 번, 0.2.35 `thread stop`과 같다). 세션·작업 폴더는 그대로다.
    pub(crate) async fn thread_stop(&self, project: &str, id: &str, actor: &str) -> Result<ThreadView, ErrorBody> {
        let rec = self.load(project, id)?;
        let pane = rec.pane.clone().ok_or_else(|| bad("thread 세션이 없다"))?;
        self.check_running(&pane)?;
        let conn = self.live_conn(&rec.machine_id)?;
        conn.request(AgentRequest::Input { session: pane, data: aaa_proto::B64(b"\x1b".to_vec()) }).await?;
        self.db.audit(actor, "thread_stop", &format!("{project}/{id}"));
        self.flow(&rec, "thread_stopped", None, "Esc");
        Ok(self.thread_view(&rec))
    }

    /// 끝났거나 시작이 실패한 thread 세션을 다시 띄운다(herdr-projects threads.rs `restart_plan`). 실행 중이면 거부한다(먼저
    /// `aaa pane kill`). 새 pane 기동에 성공하면 옛 pane의 자리·이름을 잇고 기록을 정리한다.
    /// 시작 중이거나 끝낸 thread도 거부한다.
    /// - 기본: task(Follow-ups 포함)·프로젝트 지침·memory를 다시 읽어 brief를 새로 쓰고 새 대화로 띄운 뒤 안내한다.
    /// - 자리를 만들지 못하고 실패한 thread(launch 기록이 없다)는 자리 만들기부터 다시 한다(`RestartPlan::Create`).
    /// - resume: 저장된 대화를 잇는다(brief 안내 없음). harness 세션 ID를 모르면 거부한다.
    pub(crate) async fn thread_restart(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        resume: bool,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let _guard = self.claim_thread(project, id)?;
        self.thread_restart_with(project, id, resume, actor, None).await
    }

    pub(crate) async fn thread_restart_requested(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        options: RestartRequest,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let RestartRequest { resume, profile, user_request, stop_running } = options;
        let _guard = self.claim_thread(project, id)?;
        let rec = self.load(project, id)?;
        let Some(profile) = profile.filter(|p| p != &rec.profile) else {
            if stop_running || user_request.is_some() {
                return Err(bad("--stop-running·--user-request는 다른 --profile로 바꿀 때 쓴다"));
            }
            return self.thread_restart_with(project, id, resume, actor, None).await;
        };
        let reason = self.profile_change_reason(project, actor, user_request)?;
        if rec.kind == ThreadKind::Adopted
            || rec.role != Role::Worker
            || rec.automation.is_some()
            || rec.design.is_some()
            || rec.resolve_pending.is_some()
            || matches!(rec.status, ThreadStatus::Resolved | ThreadStatus::Starting)
        {
            return Err(bad("모델은 시작 준비·정리 중이 아닌 일반 worker thread에서 바꾼다"));
        }
        if let Some(next) = rec.profile_changes.iter().rev().find(|c| c.from_thread == id && c.to_thread != id) {
            return Err(bad(format!("이 thread의 모델은 {}에서 이어진다", next.to_thread)));
        }
        if rec.status == ThreadStatus::Open && rec.park.is_some() {
            return Err(bad("보류를 맡긴 thread다. 보류를 마치거나 거둔 뒤 모델을 바꾼다"));
        }
        let running = self.thread_live(&rec).0;
        self.check_profile_allowed(project, &profile)?;
        let resolved = self.resolved_profile(Some(project), &rec.machine_id, &profile)?;
        self.check_implicit_review_profile(rec.review.as_ref(), &resolved)?;
        let current_harness = match &rec.launch {
            Some(launch) => launch.harness.clone(),
            None => self.resolved_profile(Some(project), &rec.machine_id, &rec.profile)?.harness,
        };
        let replacing = current_harness != resolved.harness;
        if replacing && rec.account.is_some() && reason.is_none() && !stop_running {
            return Err(bad(
                "고정 계정과 profile의 harness가 다르다. 새 thread의 기본 계정을 쓰려면 --user-request 또는 --stop-running으로 명시한다",
            ));
        }
        if replacing && resume {
            return Err(bad("다른 harness는 새 thread에서 새 대화로 시작한다. --resume을 뺀다"));
        }
        if running && !stop_running {
            return Err(bad("pane이 실행 중이다. 응답 완료와 저장 상태를 확인한 뒤 --stop-running을 붙인다"));
        }
        let _account_read = self.account_rename_gate.read().await;
        let _queue_guard = self.queue_gate.lock().await;
        let task = self.profile_change_task(&rec)?;
        if replacing && task.is_some() && crate::followups::pending(&rec) {
            return Err(bad("기존 thread에 처리할 후속 할 일이 있다. 후속을 기록한 뒤 모델을 바꾼다"));
        }
        let change = aaa_proto::ThreadProfileChange {
            before: rec.profile.clone(),
            after: profile,
            actor: actor.into(),
            at_ms: aaa_common::util::now_ms(),
            user_request: reason,
            from_thread: id.into(),
            to_thread: id.into(),
        };
        if replacing {
            return self.replace_thread_profile(&rec, task, change).await;
        }
        let profile = ProfileRestart { change, task };
        self.restart_session(project, id, resume, actor, None, Some(profile)).await
    }

    fn profile_change_reason(
        &self,
        project: &str,
        actor: &str,
        user_request: Option<String>,
    ) -> Result<Option<String>, ErrorBody> {
        let reason = user_request.map(|s| s.trim().to_string());
        if reason.as_deref().is_some_and(|s| s.is_empty() || s.len() > 2000 || s.chars().any(char::is_control)) {
            return Err(bad("사용자 지시 근거는 비지 않은 한 줄(2000바이트 이하)로 쓴다"));
        }
        if self.coordinator_project_of_actor(actor).as_deref() == Some(project) {
            return reason.map(Some).ok_or_else(|| {
                err(
                    ErrorCode::Forbidden,
                    "정해진 모델 변경은 coordinator가 사용자의 직접 채팅 지시를 --user-request에 인용해야 한다",
                )
            });
        }
        if actor.starts_with("device:") || (actor.starts_with("local:") && !crate::front::in_session(actor)) {
            return Ok(reason);
        }
        Err(err(
            ErrorCode::Forbidden,
            "모델 변경은 사용자 직접 조작이나 해당 프로젝트 coordinator의 사용자 지시만 허용한다",
        ))
    }

    fn profile_change_task(&self, rec: &ThreadRec) -> Result<Option<aaa_proto::TaskView>, ErrorBody> {
        let tasks = self.task_board(&rec.project)?.tasks;
        let task = rec
            .task_id
            .as_ref()
            .and_then(|id| tasks.iter().find(|t| t.id.as_ref() == Some(id)))
            .or_else(|| tasks.iter().find(|t| t.thread.as_deref() == Some(&rec.id)));
        if task.as_ref().is_some_and(|t| t.done || t.thread.as_ref().is_some_and(|id| id != &rec.id)) {
            return Err(err(ErrorCode::Conflict, "이어진 할 일이 완료됐거나 다른 thread에 연결됐다"));
        }
        Ok(task.cloned())
    }

    async fn stop_profile_pane(&self, rec: &ThreadRec, conn: &crate::AgentConn) -> Result<(), ErrorBody> {
        if let Some(pane) = rec.pane.as_deref().filter(|_| self.thread_live(rec).0) {
            let rx = self.pane_events(pane)?;
            self.kill_session(conn, pane).await?;
            self.wait_pane(pane, rx, |p| !p.info.state.is_running(), "모델 변경 전 pane 종료를 확인하지 못했다")
                .await?;
        }
        Ok(())
    }

    async fn replace_thread_profile(
        self: &Arc<Self>,
        rec: &ThreadRec,
        task: Option<aaa_proto::TaskView>,
        change: aaa_proto::ThreadProfileChange,
    ) -> Result<ThreadView, ErrorBody> {
        let project = &rec.project;
        let id = &rec.id;
        let dir = self.project_dir(project)?;
        let original = std::fs::read_to_string(dir.join("threads").join(format!("{id}.task.md"))).map_err(internal)?;
        let mut args = StartArgs {
            project: project.clone(),
            title: rec.title.clone(),
            machine: Some(self.machine_name(&rec.machine_id)),
            profile: change.after.clone(),
            kind: rec.kind,
            repo: rec.repo.clone(),
            base: rec.branch.clone().or_else(|| rec.base.clone()),
            role: rec.role,
            holds: rec.holds.clone(),
            attachments: rec.attachments.clone(),
            start_task: task,
            ..Default::default()
        };
        self.check_active(project)?;
        self.check_machine_reservation(&args, &rec.machine_id)?;
        let conn = self.live_conn(&rec.machine_id)?;
        self.check_health(&rec.machine_id)?;
        self.stop_profile_pane(rec, &conn).await?;
        let actor = change.actor.clone();
        if rec.status == ThreadStatus::Open {
            self.park_close_claimed(
                project,
                id,
                Some(aaa_proto::ThreadPark {
                    actor: actor.clone(),
                    requested_at_ms: change.at_ms,
                    note: Some(format!("모델 변경 {} → {}", change.before, change.after)),
                    user_request: change.user_request.clone(),
                    immediate: true,
                    report_before: rec.report_hash.clone(),
                    parked_at_ms: None,
                    machine: args.machine.clone().unwrap_or_default(),
                    head: None,
                    pushed: None,
                    uncommitted: None,
                    git_unknown: None,
                    resumable: false,
                }),
                &actor,
                false,
            )
            .await?;
        }
        let previous = self.load(project, id)?;
        let report = self.verified_report(&previous).unwrap_or_default();
        args.task = format!(
            "{}\n\n## 앞 작업\n\n{id}에서 모델을 바꿔 이어 한다. 옛 작업 폴더의 미커밋 변경과 파일도 확인한다.\n\n### 마지막 보고(참고)\n\n{}",
            original.trim_end(),
            report.chars().take(20_000).collect::<String>()
        );
        let mut view = self.thread_start_key(args, &actor, false, None, Some(change)).await?;
        view.warnings.push(format!("{id}의 미커밋 변경과 파일은 기존 작업 폴더에 남아 있다"));
        Ok(view)
    }

    /// `thread_restart`. `unpark`면 보류한 thread도 받고(park.rs 이어 하기), 그 글(이어 하기 안내·마지막 보고)을 새 대화의
    /// 할 일 뒤에 붙인다. `Some("")`은 붙일 글 없이 보류만 받는다(같은 대화를 잇는다)
    /// 호출자는 작업 자리 변경 전부터 thread 작업 잠금을 잡는다.
    pub(crate) async fn thread_restart_with(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        resume: bool,
        actor: &str,
        unpark: Option<&str>,
    ) -> Result<ThreadView, ErrorBody> {
        self.restart_session(project, id, resume, actor, unpark, None).await
    }

    async fn restart_session(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        resume: bool,
        actor: &str,
        unpark: Option<&str>,
        profile_change: Option<ProfileRestart>,
    ) -> Result<ThreadView, ErrorBody> {
        let mut rec = self.load(project, id)?;
        let profile = profile_change.as_ref().map(|r| &r.change.after);
        // 참고 threads.rs `restart`: 데려온 pane은 사람이 띄운 것이라 같은 것을 다시 띄울 방법을 모른다
        if rec.kind == ThreadKind::Adopted {
            return Err(bad("데려온 thread는 다시 띄우지 않는다. 새 pane을 띄워 `aaa thread adopt`로 데려온다"));
        }
        let running = rec
            .pane
            .as_deref()
            .is_some_and(|p| self.pane_view_of(p, aaa_proto::DeviceRole::Local).is_some_and(|v| v.state.is_running()));
        // host가 재시작돼 준비 task가 없으면 agent journal에서 회수한다.
        if rec.status == ThreadStatus::Starting {
            rec = self.recover_thread_start(&rec)?;
            if rec.status == ThreadStatus::Open
                && rec.pane.as_deref().is_some_and(|p| {
                    self.pane_view_of(p, aaa_proto::DeviceRole::Local).is_some_and(|v| v.state.is_running())
                })
            {
                self.spawn_brief(project, id);
                return Ok(self.thread_view(&rec));
            }
            if rec.status == ThreadStatus::Starting {
                return Err(bad("기동 결과가 아직 불명이다. 실제 세션과 대조한 뒤 처리한다. 새 기동을 보내지 않았다"));
            }
        }
        // 고정 계정은 새 작업의 기본 계정으로 바꾸지 않는다. 거부는 brief·launch·profile을 쓰기 전에 끝낸다.
        let resolved_profile = if resume && profile.is_none() {
            None
        } else {
            let again = profile.unwrap_or(&rec.profile);
            if !again.is_empty() {
                self.check_profile_allowed(project, again)?;
            }
            let p = self.resolved_profile(Some(project), &rec.machine_id, again)?;
            if profile.is_some() {
                self.check_implicit_review_profile(rec.review.as_ref(), &p)?;
            }
            if let Some(account) = &rec.account {
                let credential = self.accounts.credential(account)?;
                if credential.harness != p.harness {
                    return Err(bad(format!(
                        "고정 계정의 harness({})와 profile의 harness({})가 다르다. 기존 thread의 계정은 바꾸지 않는다. \
                         새 harness로 새 thread를 시작한다(`aaa thread start {project} --profile {again} --task-file <파일>`). \
                         새 thread는 그 harness의 기본 계정을 쓰며 --account로 직접 고를 수 있다",
                        credential.harness, p.harness
                    )));
                }
            }
            Some(p)
        };
        let plan = restart_plan(&rec, resume, running && profile_change.is_none(), unpark.is_some())?;
        let with_handoff = |task: String| match unpark.filter(|t| !t.is_empty()) {
            Some(extra) => format!("{}\n\n{extra}", task.trim_end()),
            None => task,
        };
        let conn = self.live_conn(&rec.machine_id)?;
        self.check_health(&rec.machine_id)?;
        let already_held = rec.status == ThreadStatus::Open;
        let mut claimed = false;
        let mut claim = |rec: &mut ThreadRec| -> Result<(), ErrorBody> {
            self.db
                .thread_restart_claim(
                    project,
                    id,
                    self.settings(project).max_parallel_threads,
                    self.automation_restart_admission(rec)?.as_ref(),
                )
                .map_err(|e| err(ErrorCode::Unavailable, e.to_string()))?;
            claimed = true;
            if let Some(account) = &rec.account
                && let Some(generation) = self.accounts.credential(account)?.auth_generation
            {
                *rec = self.update_rec(project, id, |r| {
                    if let Some(a) = &mut r.account_allocation {
                        a.generation = generation;
                        a.ended_at_ms = None;
                        a.released = false;
                        a.selection.selected_at_ms = aaa_common::util::now_ms();
                        a.selection.reason = "처음 선택한 계정으로 명시 재시작".into();
                    }
                })?;
            }
            Ok(())
        };
        let result: Result<ThreadView, ErrorBody> = async {
            let profile_note = profile.as_ref().map(|p| format!(", profile {p}")).unwrap_or_default();
            if plan == RestartPlan::Create {
                let dir = self.project_dir(project)?;
                let task = with_handoff(
                    std::fs::read_to_string(dir.join("threads").join(format!("{id}.task.md"))).map_err(internal)?,
                );
                claim(&mut rec)?;
                if let Some(r) = &profile_change {
                    self.apply_profile_task(project, r)?;
                }
                rec = self.update_rec(project, id, |r| {
                    if let Some(change) = &profile_change {
                        record_profile_change(r, change);
                    }
                    r.brief = BriefState::Pending;
                    r.brief_attempts = 0;
                    r.brief_line = None;
                })?;
                rec = self.update_rec(project, id, |r| {
                    r.status = ThreadStatus::Starting;
                    r.prepare_started_ms = Some(aaa_common::util::now_ms());
                    r.error = None;
                })?;
                let brief = self.brief(&dir, &rec, &task, true);
                self.db.audit(actor, "thread_restart", &format!("{project}/{id} 자리부터{profile_note}"));
                if let Some(r) = &profile_change {
                    self.profile_change_event(&rec, &r.change);
                }
                self.place_and_launch(&conn, &mut rec, brief, resolved_profile, actor).await?;
                self.flow(&rec, "thread_restarted", None, format!("자리부터 다시 만듦{profile_note}"));
                self.spawn_brief(project, id);
                return Ok(self.thread_view(&rec));
            }
            let launch = rec.launch.clone().ok_or_else(|| internal("launch 기록이 없다"))?;
            let key = resume.then(|| self.resume_session_key(&rec, &launch)).flatten();
            if resume && profile_change.is_some() && key.is_none() {
                return Err(bad("harness 세션 ID를 모른다. 새로 시작한다(--resume 없이)"));
            }
            let argv = if resume && profile_change.is_none() {
                let argv = resume_argv(&launch, key.as_deref())?;
                claim(&mut rec)?;
                argv
            } else {
                let dir = self.project_dir(project)?;
                let task = with_handoff(
                    std::fs::read_to_string(dir.join("threads").join(format!("{id}.task.md"))).map_err(internal)?,
                );
                // 새 대화는 host 정본과 지금 safety를 다시 읽는다. --resume은 위의 저장된 launch를 쓴다.
                let again = profile.unwrap_or(&rec.profile).clone();
                let brief_rec = ThreadRec { profile: again.clone(), ..rec.clone() };
                let brief = self.brief(&dir, &brief_rec, &task, true);
                let work_dir = rec.work_dir.clone().ok_or_else(|| internal("작업 폴더 기록이 없다"))?;
                let safety = None;
                let mut relaunched = match conn
                    .request(AgentRequest::ThreadRebrief {
                        work_dir,
                        brief,
                        launch: launch.clone(),
                        profile: Some(again),
                        safety,
                        resolved_profile,
                    })
                    .await?
                {
                    AgentResponse::Relaunch(l) => l,
                    other => return Err(internal(format!("예상하지 못한 응답: {other:?}"))),
                };
                // 옛 agent는 `profile`을 모르고 저장된 launch를 돌려준다
                if let Some(p) = profile
                    && relaunched.profile != *p
                {
                    return Err(bad(format!(
                        "그 머신 agent가 profile을 바꾸지 못했다(돌려준 것: {}). agent를 새 aaa로 바꾼 뒤 다시 한다",
                        relaunched.profile
                    )));
                }
                let argv = if resume {
                    if relaunched.harness != launch.harness
                        || !relaunched.resume_argv.iter().any(|arg| arg.contains("{session}"))
                    {
                        return Err(bad("이 profile은 같은 대화를 이을 수 없다. --resume을 뺀다"));
                    }
                    let argv = resume_argv(&relaunched, key.as_deref())?;
                    relaunched.session_key = key;
                    argv
                } else {
                    relaunched.argv.clone()
                };
                if profile.is_some() && relaunched.harness != launch.harness {
                    return Err(bad("agent가 돌려준 harness가 다르다. 모델 설정과 agent 버전을 확인한다"));
                }
                claim(&mut rec)?;
                if let Some(r) = &profile_change {
                    self.stop_profile_pane(&rec, &conn).await?;
                    self.apply_profile_task(project, r)?;
                }
                let launch = relaunched.clone();
                rec = self.update_rec(project, id, |r| {
                    if let Some(change) = &profile_change {
                        record_profile_change(r, change);
                    }
                    if !resume {
                        r.brief = BriefState::Pending;
                        r.brief_attempts = 0;
                        r.brief_line = None;
                    }
                    r.launch = Some(launch);
                })?;
                if let Some(r) = &profile_change {
                    self.profile_change_event(&rec, &r.change);
                }
                argv
            };
            self.db.audit(actor, "thread_restart", &format!("{project}/{id} resume={resume}{profile_note}"));
            self.launch_session(&mut rec, &argv, actor).await?;
            let detail =
                if resume { "resume".to_string() } else { format!("새 대화·brief 다시 씀{profile_note}") };
            self.flow(&rec, "thread_restarted", None, detail);
            if !resume {
                self.spawn_brief(project, id);
            }
            Ok(self.thread_view(&rec))
        }
        .await;
        if claimed
            && let Err(e) = &result
            && let Ok(r) = self.note_launch_failure(project, id, e, already_held)
        {
            self.flow(&r, "thread_failed", None, e.message.clone());
        }
        result
    }

    fn apply_profile_task(&self, project: &str, profile: &ProfileRestart) -> Result<(), ErrorBody> {
        if let Some(task) = &profile.task {
            self.task_change_thread_profile(project, task, &profile.change)?;
        }
        Ok(())
    }

    fn resume_session_key(&self, rec: &ThreadRec, launch: &aaa_proto::Launch) -> Option<String> {
        rec.pane
            .as_deref()
            .and_then(|pane| self.pane_agent(pane))
            .and_then(|agent| agent.harness_session)
            .or_else(|| launch.session_key.clone())
            .filter(|key| aaa_common::util::harness_session_ok(key))
    }

    pub(super) fn profile_change_event(&self, rec: &ThreadRec, change: &aaa_proto::ThreadProfileChange) {
        let detail = format!(
            "{} {} → {} {} · {}{}",
            change.from_thread,
            change.before,
            change.to_thread,
            change.after,
            change.actor,
            change.user_request.as_ref().map(|r| format!(" · 사용자 지시: {r}")).unwrap_or_default()
        );
        self.db.audit(&change.actor, "thread_profile_changed", &format!("{} {detail}", rec.project));
        self.flow(rec, "thread_profile_changed", None, detail);
    }

    pub(crate) fn note_launch_failure(
        &self,
        project: &str,
        id: &str,
        error: &ErrorBody,
        already_held: bool,
    ) -> Result<ThreadRec, ErrorBody> {
        let rec = self.update_rec(project, id, |r| {
            let unknown = r.launch_sent
                && matches!(error.code, ErrorCode::Timeout | ErrorCode::AgentDisconnected | ErrorCode::Internal);
            r.status = if unknown {
                ThreadStatus::Starting
            } else if already_held {
                ThreadStatus::Open
            } else {
                ThreadStatus::Failed
            };
            r.error = Some(error.message.clone());
        })?;
        if rec.status != ThreadStatus::Starting {
            self.db.cancel_unspawned_agy_slot(&rec.machine_id, &format!("{project}/{id}")).map_err(internal)?;
        }
        self.tick_wake.notify_one();
        Ok(rec)
    }
}

fn record_profile_change(rec: &mut ThreadRec, profile: &ProfileRestart) {
    rec.profile.clone_from(&profile.change.after);
    if let Some(task) = &profile.task {
        rec.task_id.clone_from(&task.id);
    }
    rec.profile_changes.push(profile.change.clone());
}

fn resume_argv(launch: &aaa_proto::Launch, key: Option<&str>) -> Result<Vec<String>, ErrorBody> {
    if launch.resume_argv.is_empty() {
        return Err(bad("이 harness는 대화를 이어 띄우는 방법이 없다"));
    }
    match key {
        Some(key) => Ok(launch.resume_argv.iter().map(|arg| arg.replace("{session}", key)).collect()),
        None if launch.resume_argv.iter().any(|arg| arg.contains("{session}")) => {
            Err(bad("harness 세션 ID를 모른다. 새로 시작한다(--resume 없이)"))
        }
        None => Ok(launch.resume_argv.clone()),
    }
}
