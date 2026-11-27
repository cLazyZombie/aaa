//! thread 멈춤과 다시 띄우기.

use super::*;

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
        profile: Option<String>,
        actor: &str,
    ) -> Result<ThreadView, ErrorBody> {
        let _guard = self.claim_thread(project, id)?;
        self.thread_restart_with(project, id, resume, profile, actor, None).await
    }

    /// `thread_restart`. `unpark`면 보류한 thread도 받고(park.rs 이어 하기), 그 글(이어 하기 안내·마지막 보고)을 새 대화의
    /// 할 일 뒤에 붙인다. `Some("")`은 붙일 글 없이 보류만 받는다(같은 대화를 잇는다)
    /// 호출자는 작업 자리 변경 전부터 thread 작업 잠금을 잡는다.
    pub(crate) async fn thread_restart_with(
        self: &Arc<Self>,
        project: &str,
        id: &str,
        resume: bool,
        profile: Option<String>,
        actor: &str,
        unpark: Option<&str>,
    ) -> Result<ThreadView, ErrorBody> {
        let mut rec = self.load(project, id)?;
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
        if crate::front::in_session(actor) && profile.as_ref().is_some_and(|p| p != &rec.profile) {
            return Err(err(ErrorCode::Forbidden, "정해진 모델 변경은 사용자가 웹이나 aaa 밖 CLI에서 직접 한다"));
        }
        if let Some(p) = &profile {
            if resume {
                return Err(bad("--profile은 새 대화로 다시 시작할 때만 쓴다(--resume과 함께 쓰지 않는다)"));
            }
            self.check_profile_allowed(project, p)?;
        }
        // 고정 계정은 새 작업의 기본 계정으로 바꾸지 않는다. 거부는 brief·launch·profile을 쓰기 전에 끝낸다.
        let resolved_profile = if resume {
            None
        } else {
            let again = profile.as_deref().unwrap_or(&rec.profile);
            if !again.is_empty() {
                self.check_profile_allowed(project, again)?;
            }
            let p = self.resolved_profile(Some(project), &rec.machine_id, again)?;
            if profile.as_ref().is_some_and(|name| name != &rec.profile) {
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
        let plan = restart_plan(&rec, resume, running, unpark.is_some())?;
        let with_handoff = |task: String| match unpark.filter(|t| !t.is_empty()) {
            Some(extra) => format!("{}\n\n{extra}", task.trim_end()),
            None => task,
        };
        let conn = self.live_conn(&rec.machine_id)?;
        self.check_health(&rec.machine_id)?;
        let already_held = rec.status == ThreadStatus::Open;
        self.db
            .thread_restart_claim(
                project,
                id,
                self.settings(project).max_parallel_threads,
                self.automation_restart_admission(&rec)?.as_ref(),
            )
            .map_err(|e| err(ErrorCode::Unavailable, e.to_string()))?;
        let result: Result<ThreadView, ErrorBody> = async {
            if let Some(account) = &rec.account
                && let Some(generation) = self.accounts.credential(account)?.auth_generation
            {
                rec = self.update_rec(project, id, |r| {
                    if let Some(a) = &mut r.account_allocation {
                        a.generation = generation;
                        a.ended_at_ms = None;
                        a.released = false;
                        a.selection.selected_at_ms = aaa_common::util::now_ms();
                        a.selection.reason = "처음 선택한 계정으로 명시 재시작".into();
                    }
                })?;
            }
            let profile_note = profile.as_ref().map(|p| format!(", profile {p}")).unwrap_or_default();
            if plan == RestartPlan::Create {
                let dir = self.project_dir(project)?;
                let task = with_handoff(
                    std::fs::read_to_string(dir.join("threads").join(format!("{id}.task.md"))).map_err(internal)?,
                );
                rec = self.update_rec(project, id, |r| {
                    if let Some(p) = &profile {
                        r.profile = p.clone();
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
                self.place_and_launch(&conn, &mut rec, brief, resolved_profile, actor).await?;
                self.flow(&rec, "thread_restarted", None, format!("자리부터 다시 만듦{profile_note}"));
                self.spawn_brief(project, id);
                return Ok(self.thread_view(&rec));
            }
            let launch = rec.launch.clone().ok_or_else(|| internal("launch 기록이 없다"))?;
            let argv = if resume {
                let key = rec
                    .pane
                    .as_deref()
                    .and_then(|p| self.pane_agent(p))
                    .and_then(|a| a.harness_session)
                    .or(launch.session_key.clone())
                    // 저장된 값도 다시 본다(이 검사 전 host가 받아 둔 것): `{session}`은 argv 낱말 하나가 된다
                    .filter(|k| aaa_common::util::harness_session_ok(k));
                if launch.resume_argv.is_empty() {
                    return Err(bad("이 harness는 대화를 이어 띄우는 방법이 없다"));
                }
                let needs_key = launch.resume_argv.iter().any(|a| a.contains("{session}"));
                match key {
                    Some(k) => launch.resume_argv.iter().map(|a| a.replace("{session}", &k)).collect::<Vec<_>>(),
                    None if needs_key => return Err(bad("harness 세션 ID를 모른다. 새로 시작한다(--resume 없이)")),
                    None => launch.resume_argv.clone(),
                }
            } else {
                let dir = self.project_dir(project)?;
                let task = with_handoff(
                    std::fs::read_to_string(dir.join("threads").join(format!("{id}.task.md"))).map_err(internal)?,
                );
                // 새 대화는 host 정본과 지금 safety를 다시 읽는다. --resume은 위의 저장된 launch를 쓴다.
                let again = profile.clone().unwrap_or_else(|| rec.profile.clone());
                let brief_rec = ThreadRec { profile: again.clone(), ..rec.clone() };
                let brief = self.brief(&dir, &brief_rec, &task, true);
                let work_dir = rec.work_dir.clone().ok_or_else(|| internal("작업 폴더 기록이 없다"))?;
                let safety = None;
                let relaunched = match conn
                    .request(AgentRequest::ThreadRebrief {
                        work_dir,
                        brief,
                        launch,
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
                if let Some(p) = &profile
                    && relaunched.profile != *p
                {
                    return Err(bad(format!(
                        "그 머신 agent가 profile을 바꾸지 못했다(돌려준 것: {}). agent를 새 aaa로 바꾼 뒤 다시 한다",
                        relaunched.profile
                    )));
                }
                let launch = relaunched.clone();
                rec = self.update_rec(project, id, |r| {
                    if let Some(p) = &profile {
                        r.profile = p.clone();
                    }
                    r.brief = BriefState::Pending;
                    r.brief_attempts = 0;
                    r.brief_line = None;
                    r.launch = Some(launch);
                })?;
                relaunched.argv
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
        if let Err(e) = &result
            && let Ok(r) = self.note_launch_failure(project, id, e, already_held)
        {
            self.flow(&r, "thread_failed", None, e.message.clone());
        }
        result
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
