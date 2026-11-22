//! Front API 요청을 권한 검사 뒤 각 기능으로 가른다.

use super::*;

impl Host {
    // ───────────── Front API ─────────────

    pub async fn call(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        req: FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        self.call_from(role, actor, None, req).await
    }

    /// `front`는 대화형 연결(웹 터미널)일 때만 준다. 조작 lease는 이 연결에 묶인다.
    pub async fn call_from(
        self: &Arc<Self>,
        role: DeviceRole,
        actor: &str,
        front: Option<&FrontRef>,
        req: FrontRequest,
    ) -> Result<FrontResponse, ErrorBody> {
        if matches!(&req, FrontRequest::WorkspaceAccessSet { .. }) && !account_meta::shows_email(role, actor) {
            return Err(err(ErrorCode::Forbidden, "workspace 허용 변경은 Owner(웹 조작 기기·aaa 세션 밖 CLI)만 한다"));
        }
        if matches!(&req, FrontRequest::MachineWeightSet { .. }) && !account_meta::shows_email(role, actor) {
            return Err(err(ErrorCode::Forbidden, "머신 비중 변경은 Owner(웹 조작 기기·aaa 세션 밖 CLI)만 한다"));
        }
        // 계정 인증 파일을 넣고 빼는 것은 host의 세션 밖 터미널만 한다
        if matches!(&req, FrontRequest::AccountImport { .. } | FrontRequest::AccountRemove { .. })
            && (role != DeviceRole::Local || front::in_session(actor) || !actor.starts_with("local:"))
        {
            return Err(err(ErrorCode::Forbidden, "계정 등록·삭제는 host의 aaa 세션 밖 터미널에서 한다"));
        }
        // 로그인·기본 계정은 사람이 한다: host의 세션 밖 터미널이나 웹 조작 기기(FRONT_API 1.79, 관리 GUI W5a). 세션 안 agent·worker·
        // evaluator·detached는 Local이어도 거부한다
        if matches!(
            &req,
            FrontRequest::AccountPlan { .. }
                | FrontRequest::AccountEmail { .. }
                | FrontRequest::AccountRename { .. }
                | FrontRequest::AccountImageJobs { .. }
                | FrontRequest::AccountImageDismiss { .. }
                | FrontRequest::AccountDefault { .. }
                | FrontRequest::AccountPoolMember { .. }
                | FrontRequest::AccountPoolProject { .. }
                | FrontRequest::AccountUsageLogin { .. }
                | FrontRequest::AccountLogin { .. }
                | FrontRequest::AccountLoginRemote { .. }
                | FrontRequest::AccountLoginInteraction { .. }
                | FrontRequest::AccountLoginCode { .. }
                | FrontRequest::AccountLoginStatus { .. }
                | FrontRequest::AccountLoginCancel { .. }
                | FrontRequest::AccountLoginConfirm { .. }
        ) && !account_meta::shows_email(role, actor)
        {
            return Err(err(
                ErrorCode::Forbidden,
                "계정 로그인·기본 변경은 사람이 host의 aaa 세션 밖 터미널이나 웹(조작 기기)에서 한다",
            ));
        }
        // 계정 관리의 짧은 저장/조회만 이름 이전과 배타 처리한다. 긴 터미널 대기는 이름 변경을 막지 않는다.
        // 백그라운드 시작·복원·refresh는 각 입구에서 같은 read 잠금을 잡는다.
        let _account_read = if matches!(
            &req,
            FrontRequest::AccountImport { .. }
                | FrontRequest::AccountRemove { .. }
                | FrontRequest::AccountEmail { .. }
                | FrontRequest::AccountPlan { .. }
                | FrontRequest::AccountList
                | FrontRequest::AccountImageJobs { .. }
                | FrontRequest::AccountImageDismiss { .. }
                | FrontRequest::AccountDefaults
                | FrontRequest::AccountPool
                | FrontRequest::AccountDefault { .. }
                | FrontRequest::AccountPoolMember { .. }
                | FrontRequest::AccountPoolProject { .. }
                | FrontRequest::AccountStarts { .. }
                | FrontRequest::AccountStartCancel { .. }
                | FrontRequest::AccountUsageLogin { .. }
                | FrontRequest::AccountLogin { .. }
                | FrontRequest::AccountLoginRemote { .. }
                | FrontRequest::AccountLoginInteraction { .. }
                | FrontRequest::AccountLoginCode { .. }
                | FrontRequest::AccountLoginStatus { .. }
                | FrontRequest::AccountLoginCancel { .. }
                | FrontRequest::AccountLoginConfirm { .. }
        ) {
            Some(self.account_rename_gate.read().await)
        } else {
            None
        };
        if _account_read.is_some() {
            let _recovery = self.account_rename_recovery.lock().unwrap();
            account_rename::recover(&self.db, &self.state)
                .map_err(|e| err(ErrorCode::Internal, format!("계정 이름 이전 복구: {e:#}")))?;
        }
        let shows_email = account_meta::shows_email(role, actor);
        // 서비스 암호는 웹 조작 기기의 전용 입력만 받는다. Local CLI·coordinator/thread·worker는 만들 수도 읽을 수도 없다.
        // 일반 JSON 가림은 암호 자체를 바꾸거나 복제하므로 이 요청은 고정 job/hash 검사 뒤 직접 소비한다.
        if matches!(&req, FrontRequest::MachineSetupApply { service_password: Some(_), .. })
            && (role != DeviceRole::Controller || !actor.starts_with("device:"))
        {
            return Err(err(ErrorCode::Forbidden, "서비스 암호는 사람이 웹 확인 창의 전용 입력칸에 직접 넣는다"));
        }
        // Git에 쓸 그림 바이트는 웹 첨부와 같이 그대로 넘긴다.
        let req = if matches!(
            &req,
            FrontRequest::AccountImport { .. }
                | FrontRequest::MachineSetupApply { .. }
                | FrontRequest::AccountLoginCode { .. }
                | FrontRequest::DesignAttach { .. }
        ) {
            req
        } else {
            aaa_common::redact::json(req).map_err(|source_error| {
                aaa_common::diagnostic::context(
                    source_error,
                    err(ErrorCode::BadRequest, "요청 가림 실패로 처리를 거부했다"),
                )
            })?
        };
        // 기기·머신 신뢰와 공개 범위는 사람이 바꾼다. coordinator도 Local이지만 세션 안이다.
        if matches!(
            req,
            FrontRequest::WebPair { .. }
                | FrontRequest::EnrollOpen { .. }
                | FrontRequest::EnrollClose
                | FrontRequest::EnrollApprove { .. }
                | FrontRequest::MachineRevoke { .. }
                | FrontRequest::MachineRename { .. }
                | FrontRequest::MachineSetupStart { .. }
                | FrontRequest::MachineSetupTrust { .. }
                | FrontRequest::MachineSetupChoose { .. }
                | FrontRequest::MachineSetupCancel { .. }
                | FrontRequest::MachineSetupPlan { .. }
                | FrontRequest::MachineSetupApply { .. }
                | FrontRequest::MachineSetupCheck { .. }
                | FrontRequest::MachineSetupLogin { .. }
                | FrontRequest::DeviceRevoke { .. }
                | FrontRequest::WebPublish
                | FrontRequest::WebPublishPort { .. }
                | FrontRequest::WebPublishOff
                | FrontRequest::LibraryRotate
                | FrontRequest::LibraryUploadPublish { .. }
                | FrontRequest::RoutineApprove { .. }
                | FrontRequest::SafetySet { .. }
                | FrontRequest::ProfileApply { .. }
                | FrontRequest::WorkRootCheck { .. }
                | FrontRequest::WorkRootSet { .. }
                | FrontRequest::HookSet { .. }
                | FrontRequest::LibraryRetentionSet { .. }
                | FrontRequest::ModelSet { .. }
                | FrontRequest::AutoModeSet { .. }
                | FrontRequest::AutoDecisionReview { .. }
                | FrontRequest::AutoDecisionForget { .. }
        ) && (front::in_session(actor)
            || actor.starts_with("worker:")
            || actor.starts_with("evaluator:")
            || actor.starts_with("detached:"))
        {
            return Err(err(
                ErrorCode::Forbidden,
                if matches!(req, FrontRequest::RoutineApprove { .. }) {
                    "routine 명령 승인은 사람이 aaa 밖 터미널이나 웹(조작 기기)에서 한다(이 연결은 aaa 세션 안에서 왔다)"
                } else if matches!(req, FrontRequest::LibraryRetentionSet { .. }) {
                    "보고서 보존 설정은 사람이 aaa 밖 터미널이나 웹(조작 기기)에서 바꾼다"
                } else if matches!(
                    req,
                    FrontRequest::SafetySet { .. }
                        | FrontRequest::ProfileApply { .. }
                        | FrontRequest::WorkRootCheck { .. }
                        | FrontRequest::WorkRootSet { .. }
                        | FrontRequest::HookSet { .. }
                        | FrontRequest::ModelSet { .. }
                ) {
                    "safety·profile·모델·작업 root·hook 설정은 사람이 aaa 밖 터미널이나 웹(조작 기기)에서 바꾼다(이 연결은 aaa 세션 안에서 왔다)"
                } else {
                    "기기·머신 등록과 폐기·웹 공개 변경·보고서 키 교체·올리기는 사람이 aaa 밖 터미널이나 웹에서 한다"
                },
            ));
        }
        if req.local_only() && role != DeviceRole::Local {
            return Err(err(ErrorCode::Forbidden, "이 머신의 CLI에서만 할 수 있다"));
        }
        if req.needs_control() && !role.can_control() {
            return Err(err(
                ErrorCode::Forbidden,
                format!("보기 권한으로는 조작할 수 없다{}", front::reader_reason(actor)),
            ));
        }
        let (mut req, selected_account, pool_request) = match req {
            FrontRequest::ThreadStartAccount { account, start } => {
                if !matches!(&*start, FrontRequest::ThreadStart { .. }) {
                    return Err(err(ErrorCode::BadRequest, "계정 지정은 thread 시작 요청에만 쓴다"));
                }
                (*start, Some(account), None)
            }
            FrontRequest::ThreadStartPool { id, size, start } => {
                if !matches!(&*start, FrontRequest::ThreadStart { .. }) {
                    return Err(err(ErrorCode::BadRequest, "풀 지정은 thread 시작 요청에만 쓴다"));
                }
                (*start, None, Some(account_pool::PoolRequest { id, size }))
            }
            other => (other, None, None),
        };
        // 프로젝트를 생략한 요청(FRONT_API 1.50, plan §8.288): host가 맡은 프로젝트 하나로 채운다
        // 모든 프로젝트 기본·profile 자체를 고치는 설정 관리(W5c)는 프로젝트가 아직 없는 host에서도 된다(그때는 빈 채로 둔다)
        let needs_project = !matches!(
            &req,
            FrontRequest::SafetyShow { scope: aaa_proto::SettingsScope::Global, .. }
                | FrontRequest::SafetySet { scope: aaa_proto::SettingsScope::Global, .. }
        ) && !matches!(
            &req,
            FrontRequest::ProfilePlan { change, .. } | FrontRequest::ProfileApply { change, .. }
                if !matches!(change, aaa_proto::ProfileChangeReq::Allow { scope: aaa_proto::SettingsScope::Project, .. })
        );
        if let Some(project) = req.project_mut()
            && project.is_empty()
        {
            match self.instance_project() {
                Ok(p) => *project = p,
                Err(e) if needs_project => return Err(e),
                Err(source_error) => {
                    aaa_common::diagnostic::observe(&source_error, "오류 뒤 기존 복구·거부 분기를 따른다");
                }
            }
        }
        self.normalize_short_ids(&mut req)?;
        // 보호 판정도 생략한 프로젝트를 채운 뒤에 한다(일괄 변환은 프로젝트 파일을 읽는다).
        if role != DeviceRole::Local {
            automation::check_web_mutation(self, &req)?;
        }
        match req {
            FrontRequest::InstallApprove { request } => self.install_approve(role, actor, *request),
            FrontRequest::InstallPublish { installation_id } => {
                self.install_publish(front.and_then(|f| f.peer_pid), &installation_id).await
            }
            FrontRequest::InstallPublishStatus { installation_id } => {
                self.install_publish_status(&installation_id).await
            }
            FrontRequest::AccountImport { name, harness, secret, email } => {
                let email = email.as_ref().map(|e| account_meta::validated_email(&e.0)).transpose()?;
                let secret = secret.0.trim();
                // 가져온 인증은 로그인으로 기록한다(Codex는 인증의 계정 정보로 메일·plan을 읽는다)
                let identity = if harness == "codex" {
                    account_meta::codex_identity(secret)
                } else {
                    account_meta::Identity::default()
                };
                if matches!((&email, &identity.email), (Some(a), Some(b)) if !a.eq_ignore_ascii_case(b)) {
                    return Err(err(ErrorCode::BadRequest, "입력한 메일과 인증의 계정이 다릅니다"));
                }
                self.register_account(&name, &harness, secret)?;
                self.account_logged_in(
                    &name,
                    &account_meta::Identity { email: email.or(identity.email), ..identity },
                    false,
                )?;
                self.db.audit(actor, "account_import", &name);
                Ok(FrontResponse::Accounts { accounts: self.account_views(shows_email)? })
            }
            FrontRequest::AccountList => Ok(FrontResponse::Accounts { accounts: self.account_views(shows_email)? }),
            FrontRequest::AccountPlans => Ok(FrontResponse::AccountPlans { plans: account_plan::options() }),
            FrontRequest::AccountPlan { name, plan } => self.account_plan(&name, plan.as_deref(), actor),
            FrontRequest::AccountDefaults => Ok(FrontResponse::AccountDefaults { defaults: self.account_defaults()? }),
            FrontRequest::AccountPool => Ok(FrontResponse::AccountPool { pool: self.account_pool()? }),
            FrontRequest::AccountPoolMember { revision, account, member } => {
                self.pool_member_set(&revision, &account, member, actor)
            }
            FrontRequest::AccountPoolProject { revision, project, enabled } => {
                self.pool_project_set(&revision, &project, enabled, actor)
            }
            FrontRequest::AccountStarts { project } => {
                Ok(FrontResponse::AccountStarts { starts: self.pool_starts(&project)? })
            }
            FrontRequest::AccountStartCancel { project, id } => self.pool_cancel(&project, &id, actor).await,
            FrontRequest::AccountDefault { harness, account } => {
                self.set_default_account(&harness, account.as_deref())?;
                self.db.audit(actor, "account_default", &format!("{harness} {}", account.as_deref().unwrap_or("off")));
                self.accounts_bump();
                Ok(FrontResponse::AccountDefaults { defaults: self.account_defaults()? })
            }
            FrontRequest::AccountUsageLogin { name, remote, email } => {
                if !remote && (role != DeviceRole::Local || !actor.starts_with("local:")) {
                    return Err(err(ErrorCode::Forbidden, "host 브라우저 로그인은 host 터미널에서 시작합니다"));
                }
                if remote {
                    self.account_usage_login_remote(&name, actor, email.as_ref().map(|e| e.0.as_str()))
                } else {
                    self.account_login_start(&name, "claude", actor, true, email.as_ref().map(|e| e.0.as_str()))
                }
            }
            FrontRequest::AccountLogin { name, harness, email } => {
                self.account_login_start(&name, &harness, actor, false, email.as_ref().map(|e| e.0.as_str()))
            }
            FrontRequest::AccountLoginRemote { name, harness, email } => {
                self.account_login_remote(&name, &harness, actor, email.as_ref().map(|e| e.0.as_str()))
            }
            FrontRequest::AccountEmail { name, email } => self.account_email(&name, &email.0, actor),
            FrontRequest::AccountLoginInteraction { id } => self.account_login_interaction(id.as_deref(), actor),
            FrontRequest::AccountLoginCode { id, code } => self.account_login_code(&id, code, actor),
            FrontRequest::AccountLoginStatus { id } => self.account_login_status(&id, actor),
            FrontRequest::AccountLoginCancel { id } => self.account_login_cancel(&id, actor).await,
            FrontRequest::AccountLoginConfirm { id, switch } => self.account_login_confirm(&id, switch, actor),
            FrontRequest::AccountRename { name, to } => self.account_rename(&name, &to, actor).await,
            FrontRequest::AccountImageJobs { name } => self.account_image_jobs(&name),
            FrontRequest::AccountImageDismiss { name, id } => self.account_image_dismiss(&name, &id, actor).await,
            FrontRequest::AccountRemove { name } => {
                if self.account_defaults()?.iter().any(|d| d.account == name) {
                    return Err(err(ErrorCode::BadRequest, "기본 계정은 먼저 account default --off로 해제한다"));
                }
                self.pool_forget(&name)?;
                self.accounts.remove(&name)?;
                self.account_meta_remove(&name);
                self.db.audit(actor, "account_remove", &name);
                Ok(FrontResponse::Done)
            }
            FrontRequest::ThreadStartAccount { .. } | FrontRequest::ThreadStartPool { .. } => unreachable!(),
            FrontRequest::Instance => Ok(FrontResponse::Instance { project: self.instance_view() }),
            FrontRequest::DebugInput { pane, thread, since_ms } => self.debug_input(pane, thread, since_ms),
            FrontRequest::RolloutStart { id, spec } => self.rollout_start(role, actor, id, spec).await,
            FrontRequest::RolloutStatus { id } => self.rollout_status(role, actor, id),
            // 웹 「배포」(관리 GUI W6a): 점검은 조작 기기, 목록은 보기 전용 기기도 본다(표시용 요약만)
            FrontRequest::RolloutPlan { source_machine, path, machines } => {
                self.rollout_plan(role, actor, source_machine, path, machines).await
            }
            FrontRequest::Rollouts => self.rollouts(),
            FrontRequest::DeployJobs => self.deployment_jobs().await,
            FrontRequest::DeployContext => Ok(self.deployment_context(actor)),
            FrontRequest::DeployStart { machine, id, action } => {
                self.deployment_start(role, actor, machine, id, action).await
            }
            FrontRequest::DeployStatus { machine, id } => self.deployment_status(role, actor, machine, id).await,
            FrontRequest::Post { project, input } => {
                self.front_post(&project, input, actor).map(|result| FrontResponse::Post { result, handled: None })
            }
            FrontRequest::PostStatus { project, post_id } => self.post_status_view(&project, &post_id, actor),
            FrontRequest::PostHandled { project, post_id } => self.post_handled(&project, &post_id, actor),
            FrontRequest::PostList { project } => self.post_list(&project),
            FrontRequest::PostUnmute { project, thread } => self.post_unmute(&project, &thread, actor),
            FrontRequest::AutoDecisions { project, before, id } => {
                self.project_dir(&project)?;
                let (decisions, next) = self.auto_decision_page(&project, before.as_deref(), id.as_deref())?;
                Ok(FrontResponse::AutoDecisions { decisions, next })
            }
            FrontRequest::AutoModeGet { project } => {
                self.project_dir(&project)?;
                self.auto_mode(&project).map(FrontResponse::AutoMode)
            }
            FrontRequest::AutoModeSet { project, on } => self.auto_mode_set(&project, on, actor),
            FrontRequest::QuestionAutoAnswer { project, id, choice, text, reason } => {
                self.auto_answer(&project, &id, choice, text, reason, actor).await
            }
            FrontRequest::AutoDecisionReview { project, id, answer } => {
                self.auto_decision_review(&project, &id, answer, actor)
            }
            FrontRequest::AutoDecisionForget { project, ids, all } => {
                self.auto_decision_forget(&project, &ids, all, actor)
            }
            FrontRequest::Asks { project, paged } => self.asks_view_mode(&project, paged),
            FrontRequest::AskHistory { project, before, id } => {
                self.ask_history(&project, before.as_deref(), id.as_deref())
            }
            FrontRequest::AskHandle { project, item } => self.ask_handle(&project, &item, actor),
            FrontRequest::QuestionReply { project, id, choice, text } => {
                self.reply_by_id(&project, &id, choice.as_deref(), text, actor).await
            }
            FrontRequest::AskAnswer { project, item, body_hash, picks, text, comments } => {
                self.ask_answer(&project, &item, &body_hash, picks, text, comments, actor)
            }
            FrontRequest::AskForget { project, ids, all } => self.ask_forget(&project, &ids, all, actor),
            FrontRequest::AskCorrect { project, id, base_at_ms, picks, text, comments } => {
                self.ask_correct(&project, &id, base_at_ms, picks, text, comments, actor).await
            }
            FrontRequest::Status => Ok(FrontResponse::Status(HostStatus {
                host_id: self.host_id.clone(),
                software: util::software(),
                proto: FRONT_API,
                dev_mode: self.dev_mode,
                agent_listen: self.agent_listen.lock().unwrap().clone(),
                web_listen: self.web_listen.lock().unwrap().clone(),
                machines: self.machine_views(),
            })),
            FrontRequest::WorkspaceAccess { project, machine } => {
                self.workspace_access_view(&project, &machine).map(FrontResponse::WorkspaceAccess)
            }
            FrontRequest::WorkspaceAccessSet { project, machine, workspace, enabled, revision } => self
                .workspace_access_set(&project, &machine, &workspace, enabled, &revision, actor)
                .map(FrontResponse::WorkspaceAccess),
            FrontRequest::Machines => Ok(FrontResponse::Machines { machines: self.machine_views() }),
            FrontRequest::Panes { role: want } => {
                // 기본은 평가자 세션을 빼고, 역할을 주면 그 역할만(§8.13)
                let panes = self
                    .pane_views(role)
                    .into_iter()
                    .filter(|p| match want {
                        None => true,
                        Some(r) => p.role == Some(r),
                    })
                    .collect();
                Ok(FrontResponse::Panes { panes })
            }
            FrontRequest::ProjectCreate { name, goal, repos, thread_profile, coordinator_profile } => self
                .project_create(
                    &name,
                    orch::ProjectOptions { goal, repos, thread_profile, coordinator_profile },
                    actor,
                ),
            FrontRequest::ProjectSet { project, key, value } => self.project_set(&project, &key, &value, actor),
            FrontRequest::Projects { all } => {
                Ok(FrontResponse::Projects { projects: if all { self.projects_all() } else { self.projects() } })
            }
            FrontRequest::ProjectLifecycle { project, action } => self.project_lifecycle(&project, action, actor).await,
            FrontRequest::ProjectDelete { project, force } => self.project_delete(&project, force, actor).await,
            FrontRequest::ProjectRename { project, to, dry_run } => {
                self.project_rename(&project, &to, dry_run, actor).await
            }
            FrontRequest::ProjectSweep { project, dry_run } => self.project_sweep(&project, dry_run, actor).await,
            FrontRequest::ProjectFiles { fix } => Ok(self.project_files(fix, actor)),
            FrontRequest::ProjectOpen { project, profile, new } => {
                self.project_open(&project, profile, new, actor).await
            }
            FrontRequest::Profiles { machine } => self.profiles(machine.as_deref()).await,
            FrontRequest::ProfileResolve { name } => self.profile_resolve(name.as_deref()),
            FrontRequest::ThreadStart {
                over_limit,
                user_request,
                ignore_reservation,
                holds,
                project,
                title,
                machine,
                profile,
                kind,
                repo,
                base,
                task,
                from_task,
                background,
                link_task: _,
                task_block,
            } => {
                // 이 갈래(StartArgs·할 일 적용·기동)는 큰 지역 값을 여러 개 쥔다. 공용 dispatcher의 poll 스택에 함께 얹히면 기기 폐기 같은
                // 작은 요청도 debug 시험 스택(2MiB)을 넘는다: heap의 future로 따로 둔다(`call_design`과 같은 까닭)
                Box::pin(async move {
                    // herdr-projects threads.rs `placement`: repo가 있으면 worktree, 없으면 repo 없는 작업 폴더. task에서 띄우면
                    // 저장소 칸이 정한다(`apply_task_repo`): 요청이 정한 kind를 그 판정에 넘긴다
                    // 사용자 지시 근거는 한도 무시와 예약 무시가 함께 쓴다
                    let machine_override =
                        self.reservation_override(&project, ignore_reservation, user_request.as_deref(), actor)?;
                    let over_limit = self.thread_limit_override(
                        &project,
                        over_limit,
                        user_request.as_deref().filter(|_| over_limit || !ignore_reservation),
                        from_task.as_deref(),
                        actor,
                    )?;
                    let kind_given = kind;
                    let kind = kind.unwrap_or(if repo.is_some() {
                        aaa_proto::ThreadKind::Worktree
                    } else {
                        aaa_proto::ThreadKind::Dir
                    });
                    let mut a = orch::StartArgs {
                        automatic_workspace: None,
                        start_task: None,
                        pool: pool_request,
                        over_limit,
                        reservation: None,
                        machine_override,
                        account: selected_account,
                        holds,
                        project,
                        title,
                        machine,
                        profile,
                        kind,
                        repo,
                        base,
                        task,
                        role: aaa_proto::Role::Worker,
                        attachments: Vec::new(),
                        design: None,
                    };
                    let (mut warnings, picked) = match from_task {
                        Some(t) => {
                            let (w, t) = self.apply_from_task(&mut a, &t, task_block.as_deref()).await?;
                            if let Some(id) = &t.id
                                && let Some(q) =
                                    self.db.queue_list(&a.project).map_err(orch::internal)?.iter().find(|q| {
                                        &q.task_id == id
                                            && matches!(
                                                q.state,
                                                aaa_proto::QueueState::Queued
                                                    | aaa_proto::QueueState::Check
                                                    | aaa_proto::QueueState::Starting
                                                    | aaa_proto::QueueState::Running
                                            )
                                    })
                            {
                                if q.state == aaa_proto::QueueState::Check {
                                    return Err(err(
                                        ErrorCode::Conflict,
                                        "확인 필요한 예약이다. 원인을 고쳐 예약을 갱신하거나 취소한다",
                                    ));
                                }
                                if let Some(tid) = &q.thread {
                                    return self.thread_show(&a.project, tid).map(Box::new).map(FrontResponse::Thread);
                                }
                                return Err(err(
                                    ErrorCode::Conflict,
                                    "예약한 task는 host가 시작한다. queue를 보거나 예약을 취소한다",
                                ));
                            }
                            let placed = self.apply_task_repo(&mut a, t.repo.as_deref(), kind_given)?;
                            (w.into_iter().chain(placed).collect::<Vec<_>>(), Some(t))
                        }
                        None => {
                            if self.coordinator_project_of_actor(actor).as_deref() == Some(&a.project)
                                && self
                                    .task_board(&a.project)?
                                    .tasks
                                    .iter()
                                    .any(|t| t.title == a.title && (t.profile.is_some() || t.machine.is_some()))
                            {
                                return Err(err(
                                    ErrorCode::Forbidden,
                                    "모델·머신을 정한 task는 --from-task로 시작한다",
                                ));
                            }
                            (self.task_start_warning(&a.project, &a.title).into_iter().collect(), None)
                        }
                    };
                    // 머신을 주지 않았으면 project.toml repos에 적힌 그 repo의 머신(herdr-projects `start`의 `listed`)
                    if a.machine.is_none() {
                        let repos = self.settings(&a.project).repos;
                        a.machine = a
                            .repo
                            .as_ref()
                            .and_then(|r| repos.into_iter().find(|x| &x.path == r))
                            .and_then(|x| x.machine);
                    }
                    let project = a.project.clone();
                    if a.account.is_none() && (a.pool.is_some() || self.pool_project_enabled(&project)?) {
                        a.pool.get_or_insert_with(|| account_pool::PoolRequest {
                            id: aaa_common::util::new_id(),
                            size: aaa_proto::AccountTaskSize::Medium,
                        });
                        return self.pool_submit(a, actor, picked, background).await;
                    }
                    let mut v = if background {
                        self.thread_start_mode(a, actor, true).await?
                    } else {
                        self.thread_start(a, actor).await?
                    };
                    v.warnings.append(&mut warnings);
                    Ok(FrontResponse::Thread(Box::new(v)))
                })
                .await
            }
            FrontRequest::ThreadStartPreview { project, from_task, task_block } => {
                self.thread_start_preview(&project, &from_task, task_block.as_deref()).await
            }
            FrontRequest::ThreadAdopt { project, pane, title, task } => {
                self.thread_adopt(&project, &pane, &title, &task, actor).await.map(Box::new).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadFollowup { project, thread, id, task, reason } => self
                .thread_followup(&project, &thread, &id, task.as_deref(), reason.as_deref(), actor)
                .map(Box::new)
                .map(FrontResponse::Thread),
            FrontRequest::ThreadNext { project, thread, line, add } => {
                self.thread_next(&project, &thread, line, add, actor).await
            }
            FrontRequest::Context { project, peek } => {
                let peek = peek || !self.inbox_seen_by(&project, actor);
                self.context(&project, peek).await.map(|mut c| {
                    if role.can_control() {
                        self.library_notice_links(&mut c.inbox);
                    }
                    FrontResponse::Context(c)
                })
            }
            FrontRequest::WorkspaceDashboard { query } => {
                self.workspace_dashboard(query).map(FrontResponse::WorkspaceDashboard)
            }
            FrontRequest::RecordSearch { query } => {
                let host = self.clone();
                tokio::task::spawn_blocking(move || host.records_search(query))
                    .await
                    .map_err(orch::internal)?
                    .map(FrontResponse::RecordSearch)
            }
            FrontRequest::RecordOpen { project, key, offset } => {
                self.records_chunk(&project, &key, offset).map(FrontResponse::RecordDocument)
            }
            FrontRequest::Tasks { project } => self.task_board(&project).map(FrontResponse::TaskBoard),
            FrontRequest::TaskAttentionAnswer { project, at, report_hash, action } => {
                self.task_attention_answer(&project, at, report_hash, action, actor).await
            }
            FrontRequest::TaskQueue { project, action } => self.task_queue(&project, action, actor).await,
            FrontRequest::TaskArchive { project, restore, view } => {
                self.task_archive(&project, restore.as_deref(), view, actor).await
            }
            FrontRequest::TaskEdit { project, ops, base_sha256, user_request } => {
                self.task_edit(&project, ops, base_sha256, user_request, actor).await
            }
            FrontRequest::TaskLog { project, before, limit, filter, task, search } => {
                self.task_log_search(&project, before, limit, filter, task.as_deref(), search.as_ref())
            }
            FrontRequest::TaskLogPrune { project, before_ms } => self.task_log_prune(&project, before_ms, actor),
            req @ (FrontRequest::DesignTree { .. }
            | FrontRequest::IdeaLinks { .. }
            | FrontRequest::DesignRead { .. }
            | FrontRequest::DesignBranchOpen { .. }
            | FrontRequest::DesignSave { .. }
            | FrontRequest::DesignAttach { .. }
            | FrontRequest::DesignDetach { .. }
            | FrontRequest::DesignSay { .. }
            | FrontRequest::DesignDiscard { .. }
            | FrontRequest::DesignApply { .. }
            | FrontRequest::DesignApplyCancel { .. }
            | FrontRequest::DesignLog { .. }
            | FrontRequest::DesignRevert { .. }
            | FrontRequest::DesignHandoff { .. }
            | FrontRequest::DesignIds { .. }
            | FrontRequest::DesignMarkIds { .. }) => Box::pin(self.call_design(role, actor, req)).await,
            FrontRequest::Assignable { project, check } => {
                self.project_dir(&project)?;
                let owners = self.assignable(&project).await;
                let valid = match check {
                    Some(c) => Some(self.check_owner(&project, &owners, &c)?),
                    None => None,
                };
                Ok(FrontResponse::Assignable { owners, valid })
            }
            FrontRequest::Threads { project, role: want, query } => self.threads_page(project.as_deref(), want, &query),
            FrontRequest::ThreadShow { project, thread } => {
                self.thread_show(&project, &thread).map(Box::new).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadSummary { project, thread } => {
                self.thread_summary(&project, &thread).map(FrontResponse::ThreadSummary)
            }
            FrontRequest::ThreadSummarySet { project, thread, text } => {
                self.thread_summary_set(&project, &thread, &text, actor).map(FrontResponse::ThreadSummary)
            }
            FrontRequest::ThreadLibrary { project, thread } => self
                .thread_library(&project, &thread, actor.strip_prefix("device:"), role.can_control())
                .map(FrontResponse::Library),
            FrontRequest::LibraryPublish { project, name, source, live, to_user, title, from_thread } => {
                let to_user = to_user.then_some(title.as_deref());
                self.library_publish(&project, &name, &source, live, to_user, from_thread.as_deref(), actor)
            }
            FrontRequest::LibraryArchive { reports, archived } => self.library_archive(&reports, archived, actor),
            FrontRequest::LibraryDelete { reports } => self.library_delete(&reports, actor),
            FrontRequest::LibraryImport { project, source, prefix } => {
                self.library_import(&project, &source, &prefix, actor)
            }
            FrontRequest::LibraryPrune { project, older_than_days } => {
                self.library_prune_at(project.as_deref(), older_than_days, util::now_ms(), actor)
            }
            FrontRequest::LibraryPrunePreview { project, older_than_days } => self
                .library_prune_preview(project.as_deref(), older_than_days, util::now_ms())
                .map(FrontResponse::LibraryPrunePreview),
            FrontRequest::LibraryRotate => {
                let out = self.library_rotate(actor)?;
                self.reports_bump();
                Ok(out)
            }
            FrontRequest::LibraryUploadPublish { project, upload, name, files, to_user, title } => {
                self.library_upload_publish(&project, &upload, &name, files, to_user, title.as_deref(), actor)
            }
            FrontRequest::LibraryReports { project } => self
                .library_reports(project.as_deref(), role.can_control())
                .map(|reports| FrontResponse::LibraryReports { reports }),
            FrontRequest::LibraryMarkRead { reports } => self.library_mark_read(&reports, actor, role.can_control()),
            FrontRequest::LibraryRead { project, name, path, offset } => {
                self.library_read(&project, &name, &path, offset).await
            }
            FrontRequest::LibraryOpen { project, name } => {
                let view = self.report_library(&project, &name, actor.strip_prefix("device:"), role.can_control())?;
                self.library_opened(&project, &name);
                Ok(FrontResponse::Library(view))
            }
            FrontRequest::LibraryResolve { project, cap } => self.library_resolve(&project, &cap),
            FrontRequest::LibraryUsage => self.library_usage(),
            FrontRequest::LibraryLink { project, thread, days } => {
                self.library_link(&project, &thread, days, actor).map(FrontResponse::LibraryLink)
            }
            FrontRequest::LibraryLinks { project } => {
                self.library_links(project.as_deref()).map(|links| FrontResponse::LibraryLinks { links })
            }
            FrontRequest::LibraryLinkRevoke { id } => {
                self.library_link_revoke(&id, actor).map(FrontResponse::LibraryLink)
            }
            FrontRequest::ThreadAck { project, thread } => {
                self.thread_ack(&project, &thread, actor).map(Box::new).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadResolve { project, thread, reopen: true, .. } => {
                self.thread_reopen(&project, &thread, actor).map(Box::new).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadResolve {
                project,
                thread,
                keep_worktree,
                reopen: false,
                skip_copy,
                discard_uncopied,
            } => self
                .thread_resolve(
                    &project,
                    &thread,
                    orch::ResolveOpts { keep_worktree, skip_copy, discard_uncopied },
                    orch::ResolveWhy::Manual,
                    actor,
                )
                .await
                .map(Box::new)
                .map(FrontResponse::Thread),
            FrontRequest::ThreadStop { project, thread } => {
                self.thread_stop(&project, &thread, actor).await.map(Box::new).map(FrontResponse::Thread)
            }
            FrontRequest::AutoMergeSet { project, target, on, user_request } => {
                self.auto_merge_set(&project, &target, on, user_request, actor)
            }
            FrontRequest::ThreadPark { project, thread, now, note, user_request, cancel } => self
                .thread_park(&project, &thread, now, note, user_request, cancel, actor)
                .await
                .map(Box::new)
                .map(FrontResponse::Thread),
            FrontRequest::ThreadUnpark { project, thread, machine, repo, dry_run } => {
                self.thread_unpark(&project, &thread, machine, repo, dry_run, actor).await
            }
            FrontRequest::ThreadRestart { project, thread, resume, profile } => self
                .thread_restart(&project, &thread, resume, profile, actor)
                .await
                .map(Box::new)
                .map(FrontResponse::Thread),
            FrontRequest::ThreadBrief { project, thread } => {
                self.thread_brief(&project, &thread, actor).await.map(Box::new).map(FrontResponse::Thread)
            }
            FrontRequest::ThreadRead { project, thread, lines } => self.thread_read(&project, &thread, lines).await,
            FrontRequest::ThreadKeys { project, thread, keys, text } => {
                self.thread_keys(&project, &thread, &keys, text, actor).await
            }
            FrontRequest::Questions { all } => Ok(FrontResponse::Questions { questions: self.question_catalog(all)? }),
            FrontRequest::QuestionAnswer { id, choice, menu, pick } => {
                if let Some(t) = self.db.question_target(None, &id).map_err(orch::internal)? {
                    if t.kind != "harness" || menu.is_none() {
                        return self.reply_by_id(&t.project, &id, Some(&choice), None, actor).await;
                    }
                    return self
                        .answer_menu(
                            &t.subject,
                            &menu.unwrap(),
                            pick.ok_or_else(|| err(ErrorCode::BadRequest, "선택이 없다"))?,
                            actor,
                        )
                        .await
                        .map(FrontResponse::Question);
                }
                match (menu, pick) {
                    (Some(menu), Some(pick)) => self.answer_menu(&id, &menu, pick, actor).await,
                    _ => self.answer(&id, &choice, actor).await,
                }
                .map(FrontResponse::Question)
            }
            FrontRequest::ThreadMessage { project, thread, text, mode } => {
                self.thread_message(&project, &thread, text, mode, actor).await.map(FrontResponse::Message)
            }
            FrontRequest::ThreadAnswer { project, thread, report_hash, picks, text, comments } => self
                .thread_answer(&project, &thread, &report_hash, picks, text, comments, actor)
                .await
                .map(Box::new)
                .map(FrontResponse::Thread),
            FrontRequest::Preferences => Ok(FrontResponse::Preferences(self.preferences.get())),
            FrontRequest::PreferencesSet { settings, pins } => {
                let change = prefs::change_body(settings, pins)?;
                let doc = self.preferences.set(change, |text| self.db.set_meta(prefs::META_KEY, text))?;
                self.db.audit(actor, "preferences_set", &doc.revision.to_string());
                Ok(FrontResponse::Preferences(doc))
            }
            FrontRequest::Inbox { project, all, peek } => {
                self.inbox(&project, all, peek || !self.inbox_seen_by(&project, actor)).map(|mut p| {
                    if role.can_control() {
                        self.library_notice_links(&mut p.items);
                    }
                    FrontResponse::Inbox {
                        items: p.items,
                        done: None,
                        omitted_open: p.omitted_open,
                        omitted_done: p.omitted_done,
                    }
                })
            }
            FrontRequest::InboxHookNotice { project, pane } => {
                self.inbox_hook_notice(&project, &pane, actor).map(|text| FrontResponse::InboxHookNotice { text })
            }
            FrontRequest::InboxDone { project, ids, all } => {
                self.inbox_done(&project, &ids, all, actor).map(|(done, p)| FrontResponse::Inbox {
                    items: p.items,
                    done: Some(done),
                    omitted_open: p.omitted_open,
                    omitted_done: p.omitted_done,
                })
            }
            FrontRequest::TickerTick => Ok(FrontResponse::Ticker { runs: self.ticker_tick().await }),
            FrontRequest::MissionList { .. } => Ok(FrontResponse::Missions { missions: vec![] }),
            FrontRequest::MissionShow { .. }
            | FrontRequest::MissionApprove { .. }
            | FrontRequest::MissionReject { .. }
            | FrontRequest::MissionStart { .. }
            | FrontRequest::MissionStop { .. }
            | FrontRequest::MissionPause { .. }
            | FrontRequest::MissionResume { .. }
            | FrontRequest::MissionReload { .. }
            | FrontRequest::MissionRestart { .. }
            | FrontRequest::MissionOpen { .. } => Err(legacy_mission::removed()),
            FrontRequest::Routines { project } => self.routines_reply(&project, self.routines(&project)),
            FrontRequest::AutomationApprove { project, name, approval_sha256 } => {
                self.routines_reply(&project, self.automation_approve(&project, &name, &approval_sha256, actor))
            }
            FrontRequest::AutomationRead { project, name } => {
                self.automation_read(&project, &name).map(|doc| FrontResponse::AutomationDoc { doc })
            }
            FrontRequest::AutomationValidate { project, name, text, form, body } => self
                .automation_validate(&project, &name, text, form, body)
                .map(|(text, check)| FrontResponse::AutomationChecked { text, check }),
            FrontRequest::AutomationSave { project, name, text, expected_sha256, body, body_expected_sha256 } => self
                .automation_save(&project, &name, text, expected_sha256, body, body_expected_sha256, actor)
                .await
                .map(|doc| FrontResponse::AutomationDoc { doc }),
            FrontRequest::AutomationDelete { project, name, expected_sha256 } => {
                let deleted = self.automation_delete(&project, &name, &expected_sha256, actor).await;
                self.routines_reply(&project, deleted.and_then(|_| self.routines(&project)))
            }
            FrontRequest::AutomationRestore { project, id } => {
                let restored = self.automation_restore(&project, &id, actor).await;
                self.routines_reply(&project, restored.and_then(|_| self.routines(&project)))
            }
            FrontRequest::AutomationRun { project, name, key } => self
                .automation_run(&project, &name, &key, actor)
                .await
                .map(|(accepted, detail)| FrontResponse::AutomationRan { accepted, detail }),
            FrontRequest::ProjectSettings { project } => {
                self.project_settings_view(&project).map(FrontResponse::ProjectSettings)
            }
            FrontRequest::ProjectRepoEdit { project, from, to } => self.project_repo_edit(&project, &from, &to, actor),
            FrontRequest::ThreadResolvePreview { project, thread } => {
                self.thread_resolve_preview(&project, &thread).await.map(FrontResponse::ResolvePreview)
            }
            FrontRequest::LibraryPruneApply { project, older_than_days, expected } => {
                self.library_prune_apply(project.as_deref(), older_than_days, &expected, util::now_ms(), actor)
            }
            FrontRequest::AutomationConvert { project, dry_run, plan_sha256 } => self
                .automation_convert(&project, dry_run, plan_sha256.as_deref(), actor)
                .await
                .map(|(files, plan_sha256)| FrontResponse::AutomationConverted { files, dry_run, plan_sha256 }),
            FrontRequest::AutomationUnblock { project, name, batch, discard } => self.routines_reply(
                &project,
                self.automation_unblock(&project, &name, &batch, discard, actor).and_then(|_| self.routines(&project)),
            ),
            FrontRequest::AutomationExecutions { project, name, before } => self
                .project_dir(&project)
                .and_then(|_| {
                    // 업그레이드 전에는 접수 실패가 관측에만 있었다. 명확한 실패만 원장으로 보완한다.
                    for old in self.routine_history(&project, &name)?.runs.iter().filter(|r| r.status == "failed") {
                        self.db
                            .automation_source_failure(&project, &name, old)
                            .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
                    }
                    self.db
                        .automation_executions(&project, Some(&name), before.as_deref())
                        .map_err(|e| err(ErrorCode::Internal, e.to_string()))
                })
                .and_then(|mut runs| {
                    self.automation_execution_links(&project, &mut runs)?;
                    let next = (runs.len() == 100).then(|| runs.last().unwrap().id.clone());
                    Ok(FrontResponse::AutomationExecutions { runs, next })
                }),
            FrontRequest::AutomationExecution { project, id } => self
                .project_dir(&project)
                .and_then(|_| {
                    self.db.automation_execution(&project, &id).map_err(|e| err(ErrorCode::Internal, e.to_string()))
                })
                .and_then(|run| run.ok_or_else(|| err(ErrorCode::NotFound, "실행이 없다")))
                .and_then(|mut run| {
                    self.automation_execution_links(&project, std::slice::from_mut(&mut run))?;
                    Ok(FrontResponse::AutomationExecution { run })
                }),
            FrontRequest::RoutineRuns { project, name } => self.routine_runs(&project, &name).and_then(|runs| {
                let mut executions = self
                    .db
                    .automation_executions(&project, Some(&name), None)
                    .map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
                self.automation_execution_links(&project, &mut executions)?;
                Ok(FrontResponse::RoutineRuns { runs, executions, source: self.routine_source(&project, &name)? })
            }),
            FrontRequest::RoutineToggle { project, name, on } => {
                self.routines_reply(&project, self.routine_toggle(&project, &name, on, actor).await)
            }
            FrontRequest::RoutineApprove { project, name, command_sha256 } => {
                self.routines_reply(&project, self.routine_approve(&project, &name, &command_sha256, actor))
            }
            FrontRequest::Events { project, after, limit, tail } => {
                self.events(project.as_deref(), after, limit, tail).map(|events| FrontResponse::Events { events })
            }
            FrontRequest::EvaluatorAssign { .. }
            | FrontRequest::EvaluatorStatus { .. }
            | FrontRequest::EvaluatorUnassign { .. }
            | FrontRequest::Evaluations { .. }
            | FrontRequest::EvaluationShow { .. }
            | FrontRequest::EvaluationDecide { .. } => Err(legacy_mission::removed()),
            FrontRequest::PaneRun { machine, name, argv, cwd, cols, rows, profile } => {
                let mid = self.resolve_machine(machine.as_deref())?;
                let resolved_profile =
                    profile.as_deref().map(|name| self.resolved_profile(None, &mid, name)).transpose()?;
                let spec = SpawnSpec {
                    name: name.unwrap_or_default(),
                    argv,
                    cwd,
                    env: vec![],
                    cols: cols.unwrap_or(120),
                    rows: rows.unwrap_or(32),
                    role: None,
                    harness: None,
                    profile,
                    resolved_profile,
                };
                let id = self.spawn_on(&mid, spec, actor).await?;
                self.pane_view_of(&id, role)
                    .map(FrontResponse::Pane)
                    .ok_or_else(|| err(ErrorCode::Internal, "pane 목록에 없다"))
            }
            FrontRequest::PaneRead { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                let running = self.pane_view_of(&id, role).is_some_and(|p| p.state == SessionState::Running);
                if let (Some(conn), true) = (self.conn_of(&mid), running)
                    && let Ok(AgentResponse::Screen(s)) =
                        conn.request(AgentRequest::Screen { session: id.clone() }).await
                {
                    return Ok(FrontResponse::Screen(s));
                }
                // 끊겼거나 끝났다: 마지막 화면(mirror)
                self.mirror_text(&id)
                    .map(FrontResponse::Screen)
                    .ok_or_else(|| err(ErrorCode::AgentDisconnected, "화면 기록이 없다"))
            }
            FrontRequest::PaneFile { pane, path } => self.pane_file(role, actor, &pane, &path).await,
            FrontRequest::ThreadFile { project, thread, path } => {
                self.thread_file(actor, &project, &thread, &path).await
            }
            FrontRequest::ThreadDiff { project, thread, query } => self.thread_diff(&project, &thread, query).await,
            FrontRequest::PaneDir { pane, path, hidden } => self.pane_dir(role, actor, &pane, &path, hidden).await,
            FrontRequest::PaneCommands { pane } => self.pane_commands(role, &pane).await,
            FrontRequest::PaneChat { pane, thinking, compact, commands, before } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                // 보기 범위(§8.25)는 pane 보기와 같다
                if self.pane_view_of(&id, role).is_none() {
                    return Err(err(ErrorCode::NotFound, format!("pane이 없다: {pane}")));
                }
                let (harness, transcript, harness_session) = {
                    let i = self.inner.lock().unwrap();
                    let p = i.panes.get(&id).ok_or_else(|| err(ErrorCode::NotFound, "pane이 없다"))?;
                    let a = p.agent.as_ref();
                    (
                        a.map(|a| a.harness.clone()).unwrap_or_default(),
                        p.transcript.clone(),
                        a.and_then(|a| a.harness_session.clone()),
                    )
                };
                if !matches!(harness.as_str(), "claude" | "codex") {
                    return Err(err(ErrorCode::NotFound, "대화 기록은 Claude·Codex pane만 볼 수 있다"));
                }
                let pages = self
                    .inner
                    .lock()
                    .unwrap()
                    .machines
                    .get(&mid)
                    .is_some_and(|m| m.features.iter().any(|f| f == "chat_pages.v1"));
                if before.is_some() && !pages {
                    return Err(err(ErrorCode::UnsupportedVersion, "이 agent는 앞 기록 읽기를 지원하지 않는다"));
                }
                let conn = self.live_conn(&mid)?;
                match conn
                    .request(AgentRequest::Chat {
                        session: Some(id.clone()),
                        harness: harness.clone(),
                        transcript,
                        harness_session,
                        thinking,
                        compact,
                        commands,
                        before,
                    })
                    .await?
                {
                    AgentResponse::Chat { messages, truncated, before, history } => {
                        Ok(FrontResponse::Chat(aaa_proto::ChatView {
                            pane: id,
                            harness,
                            messages,
                            truncated,
                            before,
                            history,
                        }))
                    }
                    other => Err(err(ErrorCode::Internal, format!("예상하지 못한 응답: {other:?}"))),
                }
            }
            FrontRequest::PaneReadText { pane, source, lines, format } => {
                self.pane_read_text(&pane, source, lines, format).await
            }
            FrontRequest::PaneSendText { pane, text } => self.pane_send_text(&pane, text, actor).await,
            FrontRequest::PaneSendKeys { pane, keys } => self.pane_send_keys(&pane, &keys, actor).await,
            FrontRequest::PaneSubmit { pane, text } => self.pane_submit(&pane, &text, actor).await,
            FrontRequest::PaneWaitOutput { pane, matcher, source, lines, timeout_ms } => {
                self.pane_wait_output(&pane, matcher, source, lines, timeout_ms).await
            }
            FrontRequest::PaneGet { pane } => {
                let (id, _) = self.resolve_pane(&pane)?;
                self.pane_view_of(&id, role)
                    .map(FrontResponse::Pane)
                    .ok_or_else(|| err(ErrorCode::NotFound, format!("pane이 없다: {pane}")))
            }
            FrontRequest::PaneClose { pane } => self.pane_close(&pane, actor).await,
            FrontRequest::Workspaces => Ok(FrontResponse::Workspaces { workspaces: self.workspace_summaries() }),
            FrontRequest::WorkspaceClose { name } => self.workspace_close(&name, actor).await,
            FrontRequest::WorkspaceRename { name, to } => self.workspace_rename(&name, &to, actor),
            FrontRequest::PaneSend { pane, data } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.db.audit(actor, "pane_send", &format!("{id} {} bytes", data.0.len()));
                conn.request(AgentRequest::Input { session: id, data }).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneResize { pane, cols, rows } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                // 대화형 front가 크기를 걸면 그 화면이 지금 쓰는 화면이 된다. 한 번 보내는 CLI는 아무도 쓰지 않을 때만 바꾼다.
                let interactive = match front {
                    Some(f) if f.interactive() => {
                        self.lease_use(&id, f);
                        true
                    }
                    _ => {
                        self.lease_free(&id)?;
                        false
                    }
                };
                let conn = self.live_conn(&mid)?;
                if interactive {
                    return conn
                        .request(AgentRequest::Resize { session: id, cols, rows })
                        .await
                        .map(|_| FrontResponse::Done);
                }
                // 한 번 보내는 CLI(`pane pty-size`): 새 크기가 host에 닿은 뒤 돌아온다. 바로 뒤 읽기가 새 크기의 화면을 본다.
                let rx = self.pane_events(&id)?;
                conn.request(AgentRequest::Resize { session: id.clone(), cols, rows }).await?;
                self.wait_pane(
                    &id,
                    rx,
                    |p| (p.info.cols, p.info.rows) == (cols, rows),
                    "크기를 보냈지만 새 크기가 오지 않았다",
                )
                .await
                .map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneInput { pane, data } => {
                let f = front.ok_or_else(|| {
                    err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다(한 번 보내기는 pane_send)")
                })?;
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.lease_use(&id, f);
                // 입력 내용은 기록하지 않고, 복사 사고를 식별할 종류와 front 출처만 request=cN 로그에 묶는다.
                let input_kind = if data.0 == [3] {
                    "ctrl-c"
                } else if data.0.starts_with(b"\x1b[<") {
                    "mouse-sgr"
                } else {
                    "other"
                };
                tracing::Instrument::instrument(
                    conn.request(AgentRequest::Input { session: id, data }),
                    tracing::info_span!("pane_front", front_id = %f.id, front = %f.label, input_kind),
                )
                .await
                .map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneCtrlEnter { pane, trace } => {
                let f = front.ok_or_else(|| err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다"))?;
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.lease_use(&id, f);
                let data = self.ctrl_enter_data(&id, trace.as_deref());
                // steering할 수 없는 상태(입력 모름·쉬는 Codex 등)는 아무것도 보내지 않는다(plan §8.239 후속)
                if data.is_empty() {
                    return Ok(FrontResponse::Done);
                }
                conn.request(AgentRequest::Input { session: id, data: aaa_proto::B64(data) })
                    .await
                    .map(|_| FrontResponse::Done)
            }
            // herdr 서버가 client의 붙여넣기를 그 pane 모드대로 다시 감싸는 것과 같다(`KeyModes::paste`: 켰으면 감싸고 ESC는 뺀다)
            FrontRequest::PanePaste { pane, text } => {
                let f = front.ok_or_else(|| {
                    err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다(한 번 보내기는 pane_send)")
                })?;
                let (id, mid) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                let conn = self.live_conn(&mid)?;
                self.lease_use(&id, f);
                let data = aaa_proto::B64(self.key_modes(&id).paste(&text));
                conn.request(AgentRequest::Input { session: id, data }).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::PaneTake { pane } => {
                let f = front.ok_or_else(|| err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다"))?;
                let (id, _) = self.resolve_pane(&pane)?;
                self.check_running(&id)?;
                self.lease_set(&id, Some(f.clone()));
                self.db.audit(actor, "pane_take", &id);
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneRelease { pane } => {
                let f = front.ok_or_else(|| err(ErrorCode::BadRequest, "대화형 front 연결에서만 쓴다"))?;
                let (id, _) = self.resolve_pane(&pane)?;
                let mine = self.inner.lock().unwrap().panes.get(&id).is_some_and(|p| p.lease.as_ref() == Some(f));
                if mine {
                    self.lease_set(&id, None);
                }
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneResume { pane } => self.resume(&pane, role, actor).await,
            FrontRequest::PaneForget { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                self.forget(&id, &mid, actor).await?;
                Ok(FrontResponse::Done)
            }
            FrontRequest::PaneRename { pane, label } => self.rename(&pane, label, role, actor).await,
            FrontRequest::PaneAttach { .. } => {
                Err(err(ErrorCode::BadRequest, "그림 붙여넣기는 같은 머신 front 연결(aaa pane attach)에서만 쓴다"))
            }
            FrontRequest::Workspace { name } => {
                Ok(FrontResponse::Workspace(self.workspace(name.as_deref().unwrap_or(DEFAULT_WORKSPACE))))
            }
            FrontRequest::LayoutApply { workspace, change } => {
                // workspace를 주지 않으면 그 pane·탭이 든 workspace(없으면 main)
                let found = match &change {
                    LayoutChange::Split { pane, .. }
                    | LayoutChange::Close { pane, .. }
                    | LayoutChange::Resize { pane, .. }
                    | LayoutChange::MoveToNewTab { pane, .. }
                    | LayoutChange::Zoom { pane, .. }
                    | LayoutChange::Swap { pane, .. } => self.workspace_of_pane(pane),
                    LayoutChange::RenameTab { tab, .. }
                    | LayoutChange::CloseTab { tab }
                    | LayoutChange::SplitRatio { tab, .. }
                    | LayoutChange::MoveTab { tab, .. } => self.workspace_of_tab(tab),
                    LayoutChange::NewTab { .. } | LayoutChange::AddTab { .. } => None,
                };
                let name = workspace.or(found).unwrap_or_else(|| DEFAULT_WORKSPACE.to_string());
                self.layout_apply(&name, change, actor).await.map(FrontResponse::Workspace)
            }
            FrontRequest::PaneKill { pane } => {
                let (id, mid) = self.resolve_pane(&pane)?;
                let conn = self.live_conn(&mid)?;
                self.db.audit(actor, "pane_kill", &id);
                self.kill_session(&conn, &id).await.map(|_| FrontResponse::Done)
            }
            FrontRequest::EnrollOpen { minutes } => {
                let minutes = minutes.clamp(1, 60);
                self.enroll.lock().unwrap().open_until =
                    Some(Instant::now() + Duration::from_secs(minutes as u64 * 60));
                self.db.audit(actor, "enroll_open", &format!("{minutes}m"));
                self.enroll_bump();
                Ok(FrontResponse::Done)
            }
            FrontRequest::SafetyShow { project, scope } => self.safety_show(&project, scope).map(FrontResponse::Safety),
            FrontRequest::SafetySet { project, scope, key, value, expected } => {
                self.safety_set(&project, scope, &key, value, &expected, actor).map(FrontResponse::Safety)
            }
            FrontRequest::ProfileAdmin => self.profile_admin().map(FrontResponse::ProfileAdmin),
            FrontRequest::ProfilePlan { project, change } => {
                self.profile_plan(&project, &change).map(FrontResponse::ProfilePlan)
            }
            FrontRequest::ProfileApply { project, change, expected } => {
                self.profile_apply(&project, &change, &expected, actor)
            }
            FrontRequest::WorkRootShow { machine } => self.work_root(&machine, WorkRootOp::Show, actor).await,
            FrontRequest::WorkRootCheck { machine, path, mount } => {
                self.work_root(&machine, WorkRootOp::Check { path, mount }, actor).await
            }
            FrontRequest::WorkRootSet { machine, path, mount } => {
                self.work_root(&machine, WorkRootOp::Set { path, mount }, actor).await
            }
            FrontRequest::HookStatus { machine } => {
                self.codex_hook(&machine, None, actor).await.map(FrontResponse::Hook)
            }
            FrontRequest::HookSet { machine, install } => {
                self.codex_hook(&machine, Some(install), actor).await.map(FrontResponse::Hook)
            }
            FrontRequest::LibraryRetention => self.library_retention_settings().map(FrontResponse::LibraryRetention),
            FrontRequest::LibraryRetentionSet { max_days, max_mb, expected } => {
                self.library_retention_set(max_days, max_mb, &expected, actor).map(FrontResponse::LibraryRetention)
            }
            FrontRequest::ModelAdmin => self.model_admin().await.map(FrontResponse::ModelAdmin),
            FrontRequest::ModelSet { harness, models, enabled, expected } => {
                self.model_set(&harness, &models, enabled, &expected, actor).await.map(FrontResponse::ModelAdmin)
            }
            FrontRequest::EnrollClose => {
                self.enroll.lock().unwrap().open_until = None;
                self.db.audit(actor, "enroll_close", "");
                self.enroll_bump();
                Ok(FrontResponse::Done)
            }
            FrontRequest::EnrollGuide => Ok(FrontResponse::EnrollGuide { guide: self.enroll_guide().await }),
            FrontRequest::MachineSetupStart { target } => {
                self.setup_start(&target, actor).map(|setup| FrontResponse::MachineSetup { setup })
            }
            FrontRequest::MachineSetupTrust { job, host_key_sha256 } => {
                self.setup_trust(&job, &host_key_sha256, actor).await.map(|setup| FrontResponse::MachineSetup { setup })
            }
            FrontRequest::MachineSetupChoose { job, os, arch } => {
                self.setup_choose(&job, &os, &arch, actor).map(|setup| FrontResponse::MachineSetup { setup })
            }
            FrontRequest::MachineSetupCancel { job } => self.setup_cancel(&job, actor).map(|_| FrontResponse::Done),
            FrontRequest::MachineSetupPlan { job, settings } => {
                self.setup_plan(&job, settings, actor).map(|setup| FrontResponse::MachineSetup { setup })
            }
            FrontRequest::MachineSetupApply { job, plan_sha256, service_password } => self
                .setup_apply(role, &job, &plan_sha256, service_password, actor)
                .map(|setup| FrontResponse::MachineSetup { setup }),
            FrontRequest::MachineSetupCheck { job } => {
                self.setup_check(&job, actor).await.map(|setup| FrontResponse::MachineSetup { setup })
            }
            FrontRequest::MachineSetupLogin { job, harness } => self.setup_login(role, &job, harness, actor).await,
            FrontRequest::MachineSetupList => self.setup_list().map(|setups| FrontResponse::MachineSetups { setups }),
            FrontRequest::EnrollPending => {
                let e = self.enroll.lock().unwrap();
                let now = Instant::now();
                let open_until_ms =
                    e.open_until.filter(|t| *t > now).map(|t| util::now_ms() + (t - now).as_millis() as u64);
                let pending = e
                    .pending
                    .iter()
                    .map(|(name, p)| PendingEnroll {
                        name: name.clone(),
                        machine_id: p.machine_id.clone(),
                        fingerprint_prefix: p.fingerprint[..16].to_string(),
                        since_ms: p.since_ms,
                    })
                    .collect();
                Ok(FrontResponse::Pending { pending, open_until_ms })
            }
            FrontRequest::EnrollApprove { name, code } => self.approve(&name, &code, actor),
            FrontRequest::MachineRename { name, to } => self.machine_rename(&name, &to, actor),
            FrontRequest::MachineReserve { machine, project, holder, note, until_ms, user_request } => {
                self.machine_reserve(&machine, &project, &holder, note, until_ms, user_request, actor)
            }
            FrontRequest::MachineRelease { machine, user_request } => {
                self.machine_release(&machine, user_request, actor)
            }
            FrontRequest::MachineWeightSet { machine, weight } => self.machine_weight_set(&machine, weight, actor),
            FrontRequest::MachineRevoke { name } => {
                // 웹에서는 host 머신 자신을 폐기하지 않는다(그 머신의 agent·화면이 함께 끊긴다: host 터미널에서 한다)
                if actor.starts_with("device:")
                    && self.inner.lock().unwrap().machines.values().any(|m| m.local && m.name == name)
                {
                    return Err(err(
                        ErrorCode::Forbidden,
                        "host 머신은 웹에서 폐기하지 않는다(host 터미널의 aaa machine revoke)",
                    ));
                }
                let id = self.db.revoke_machine(&name).map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
                let Some(id) = id else { return Err(err(ErrorCode::NotFound, format!("머신이 없다: {name}"))) };
                let conn = {
                    let mut i = self.inner.lock().unwrap();
                    i.panes.retain(|_, p| p.machine_id != id);
                    i.machines.remove(&id).and_then(|m| m.conn)
                };
                if let Some(c) = conn {
                    aaa_common::diagnostic::ignored(
                        c.tx.json(&LinkMsg::Reject(err(ErrorCode::NotEnrolled, "등록이 폐기됐다"))).await,
                        "수신자가 끝난 뒤에도 현재 작업은 마친다",
                    );
                    c.fail_all();
                }
                self.db.audit(actor, "machine_revoke", &name);
                self.bump();
                Ok(FrontResponse::Done)
            }
            FrontRequest::WebPair { role: r } => {
                if r == DeviceRole::Local {
                    return Err(err(ErrorCode::BadRequest, "reader 또는 controller만 줄 수 있다"));
                }
                // 웹에서 만드는 코드는 controller만(D6, herdr-web-ui는 웹 pairing이 늘 drive). 보기 기기는 이 머신의 CLI로
                if role != DeviceRole::Local && r != DeviceRole::Controller {
                    return Err(err(
                        ErrorCode::Forbidden,
                        "보기 권한 코드는 host 머신의 CLI(aaa web pair --reader)에서만 만든다",
                    ));
                }
                let (code, expires_in_s) = self.mint_pairing(r, Instant::now());
                self.db.audit(actor, "web_pair_code", &format!("{r:?}"));
                Ok(FrontResponse::PairCode { code, role: r, expires_in_s })
            }
            FrontRequest::Devices => {
                let rows = self.db.devices().map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
                let passkeys = self.passkey_devices();
                Ok(FrontResponse::Devices {
                    devices: rows
                        .into_iter()
                        .map(|d| {
                            let has = passkeys.contains(&d.id);
                            device_view(d, has)
                        })
                        .collect(),
                })
            }
            FrontRequest::DeviceRename { id, label } => {
                let label = normalize_device_label(&label, db::DEVICE_LABEL_FALLBACK);
                if !self.db.rename_device(&id, &label).map_err(|e| err(ErrorCode::Internal, e.to_string()))? {
                    return Err(err(ErrorCode::NotFound, "그런 기기가 없다"));
                }
                self.db.audit(actor, "device_rename", &id);
                Ok(FrontResponse::Done)
            }
            FrontRequest::DeviceRevoke { id } => {
                // 마지막 조작 기기는 웹에서 폐기하지 않는다(사용자 답 W5b 2A): 웹으로 관리할 길이 닫힌다. host 터미널은 된다.
                // 세기와 폐기를 한 잠금 안에서 한다(두 조작 기기가 서로를 함께 폐기하면 둘 다 통과한다)
                let _gate = self.device_gate.lock().unwrap();
                if actor.starts_with("device:") {
                    let rows = self.db.devices().map_err(|e| err(ErrorCode::Internal, e.to_string()))?;
                    let controllers: Vec<_> =
                        rows.iter().filter(|d| d.role != "reader").map(|d| d.id.as_str()).collect();
                    if controllers == [id.as_str()] {
                        return Err(err(
                            ErrorCode::Forbidden,
                            "마지막 조작 기기는 웹에서 폐기하지 않는다: 다른 조작 기기를 먼저 짝짓거나 host 터미널의 aaa web revoke",
                        ));
                    }
                }
                if !self.revoke_device(&id)? {
                    return Err(err(ErrorCode::NotFound, "그런 기기가 없다"));
                }
                self.db.audit(actor, "device_revoke", &id);
                Ok(FrontResponse::Done)
            }
            FrontRequest::WebStatus => Ok(FrontResponse::Publish(self.publish_status().await)),
            FrontRequest::WebPublish => {
                self.db.audit(actor, "web_publish", "");
                self.publish().await.map(FrontResponse::Publish)
            }
            FrontRequest::WebPublishPort { port } => {
                self.publish_port_change(port, actor).await.map(FrontResponse::Publish)
            }
            FrontRequest::WebPublishOff => {
                self.db.audit(actor, "web_publish_off", "");
                self.publish_off().await.map(FrontResponse::Publish)
            }
        }
    }
}
