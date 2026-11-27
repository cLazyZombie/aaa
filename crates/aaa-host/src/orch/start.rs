//! thread 시작: brief, 자리 잡기, harness 실행.

use super::*;

impl Host {
    /// brief.md(herdr-projects `brief_for`): 프로젝트 설정·지침·memory를 읽어 `compose_brief`로 짓는다.
    /// memory 파일은 `memory/*.md`(이름순, symlink·숨은 파일 제외)다.
    pub(crate) fn brief(&self, dir: &Path, r: &ThreadRec, task: &str, restart: bool) -> String {
        use crate::brief_text::{BriefInput, THREAD_GUIDE, compose_brief};
        let read = |f: &str| {
            std::fs::read_to_string(dir.join(f))
                .inspect_err(|error| aaa_common::diagnostic::observe(error, "선택 파일을 읽지 못해 기본값을 쓴다"))
                .unwrap_or_default()
        };
        let settings = crate::project::ProjectSettings::load(dir);
        let instructions = read("PROJECT.md");
        let title = instructions.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or_default().trim().to_string();
        let mut mem: Vec<PathBuf> = std::fs::read_dir(dir.join("memory"))
            .into_iter()
            .flatten()
            .filter_map(|e| {
                e.inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
            })
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "md"))
            .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
            .collect();
        mem.sort();
        let memory_files: Vec<(String, String)> = mem
            .iter()
            .filter_map(|f| {
                let text = std::fs::read_to_string(f)
                    .inspect_err(|error| {
                        aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                    })
                    .ok()?;
                Some((f.file_name()?.to_string_lossy().to_string(), text))
            })
            .collect();
        let file = read(&format!("threads/{}.summary.md", r.id));
        let summary = crate::summary::split_meta(&file).1.trim().to_string();
        let mut guide = THREAD_GUIDE.to_string();
        if let Some(review) = &r.review {
            guide.push_str(&format!(
                "\n\n## 할 일 검수 설정\n\n- 검수 모델: {}\n- 검수 계정: {}\n",
                review.profile.as_deref().unwrap_or("구현 모델과 같음"),
                review.account.as_deref().unwrap_or("자동"),
            ));
            if let Some(profile) = r.independent_review_profile() {
                guide.push_str(&format!(
                    "\nfinish의 첫 독립 리뷰에서 커밋된 후보로 `aaa review start --profile {profile} --base <sha>`를 한 번 호출한다. `aaa guide review`에 따라 결과를 받아 판정한다. host가 이 thread의 검수 계정 설정을 적용한다.\n"
                ));
            }
        }
        if let Some(repo) = r.repo.as_deref().map(crate::brief_text::repo_leaf)
            && let Ok(targets) = self.design_targets(&r.project)
            && let Some(target) = targets.iter().find(|t| t.name == repo && t.folder.is_some())
        {
            guide.push_str("\n## 기획 문서 편집과 착지\n");
            guide.push_str("기획 문서를 고치면 저장소 루트에서 `aaa plan preview <문서 경로> --base <요청 기준 commit> --output <library 아래 새 폴더>`를 실행하고 index.html을 보고에 연결한다. 검토한 판을 사용자가 승인한 뒤 일반 착지로 진행한다. 충돌로 문서 내용이 바뀌면 다시 검수한다. 배포는 따로 묻는다.\n");
            if let Some(media) = &target.media {
                guide.push_str(&format!(
                    "새 손 그림·GIF는 문서 폴더 아래 `{media}`에 두고 착지할 때 상대 링크와 자료를 확인한다.\n"
                ));
            }
            guide.push_str("\n착지 시작 main과 최종 통합 SHA를 비교해 검사 범위를 고른다. 같은 최종 통합 worktree에서 문서 검사와 저장소 검증을 실행하고 통과한 SHA를 push한다.\n");
            guide.push_str(&format!(
                "- **기획 문서·그림만 변경**. `aaa verify -- aaa plan check <문서 경로> --base <착지 시작 main SHA> \
                 --head <최종 통합 SHA> --command-json <아래 JSON 배열을 인자 하나로>`를 실행한다. \
                 design_media가 있으면 `--media <설정값>`도 준다. 문서 전용 검사기는 `.json`처럼 저장소가 허용한 문서·그림 범위 밖의 파일이 들어오면 중단한다.\n\n  design_check: {}\n",
                serde_json::json!(target.check),
            ));
            guide.push_str(
                "- **코드와 기획 문서를 함께 변경**. 저장소 안내에 따라 손으로 고친 문서·그림을 골라 \
                 `aaa verify -- <저장소 문서 검사 명령과 인자>`로 직접 검사한다. 상대 링크가 가리키는 자료를 확인하고, \
                 design_media가 있으면 새 그림이 문서 폴더 아래 지정된 위치에 있는지도 확인한다. \
                 생성물은 저장소 절차로 검증한 뒤 저장소 전체 검증을 실행한다.\n",
            );
        }
        let machine = self.machine_name(&r.machine_id);
        compose_brief(&BriefInput {
            title: &title,
            project: &r.project,
            goal: &settings.goal,
            repos: &settings.repos,
            uploads_path: &dir.join("uploads").to_string_lossy(),
            remote: !self.machine_is_local(&r.machine_id),
            machine: &machine,
            repo: r.repo.as_deref().map(crate::brief_text::repo_leaf),
            attachments: r.attachments.len(),
            guide: &guide,
            instructions: &instructions,
            memory_index: &read("MEMORY.md"),
            memory_files: &memory_files,
            memory_cap: self.cfg.projects.brief_memory_chars,
            task,
            restart,
            summary: &summary,
            auto_merge: self.thread_auto_merge(r),
        })
    }

    pub(crate) fn thread_op_lock(&self, project: &str, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        self.thread_ops
            .lock()
            .unwrap()
            .entry(format!("{project}/{id}"))
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub(crate) fn claim_thread(&self, project: &str, id: &str) -> Result<tokio::sync::OwnedMutexGuard<()>, ErrorBody> {
        self.thread_op_lock(project, id).try_lock_owned().map_err(|e| {
            aaa_common::diagnostic::context(e, bad(format!("{id}는 준비·정리 작업 중이다. thread show로 상태를 본다")))
        })
    }

    /// front가 판정한 요청자만 쓴다. env·요청 본문의 역할 자기 신고는 믿지 않는다.
    pub(crate) fn thread_limit_override(
        &self,
        project: &str,
        over_limit: bool,
        user_request: Option<&str>,
        from_task: Option<&str>,
        actor: &str,
    ) -> Result<Option<aaa_proto::ThreadLimitOverride>, ErrorBody> {
        if !over_limit {
            return if user_request.is_some() {
                Err(bad("--user-request는 --over-limit와 함께 쓴다"))
            } else {
                Ok(None)
            };
        }
        let reason = user_request.map(str::trim);
        if reason.is_some_and(|r| r.is_empty() || r.len() > 2000 || r.chars().any(char::is_control)) {
            return Err(bad("사용자 지시 근거는 비지 않은 한 줄(2000바이트 이하)로 쓴다"));
        }
        if self.coordinator_project_of_actor(actor).as_deref() == Some(project) {
            if reason.is_none() || from_task.is_none_or(|t| t.trim().is_empty()) {
                return Err(err(
                    ErrorCode::Forbidden,
                    "coordinator는 사용자가 그 task의 강제 시작을 명시했을 때만 --from-task·--over-limit·--user-request로 시작한다",
                ));
            }
        } else if !(actor.starts_with("device:") || (actor.starts_with("local:") && !crate::front::in_session(actor))) {
            return Err(err(
                ErrorCode::Forbidden,
                "한도 무시 시작은 사용자 직접 조작이나 해당 프로젝트 coordinator의 명시한 사용자 지시만 허용한다",
            ));
        }
        Ok(Some(aaa_proto::ThreadLimitOverride {
            actor: actor.into(),
            at_ms: aaa_common::util::now_ms(),
            user_request: reason.map(str::to_string),
        }))
    }

    /// thread 시작: 기록 → agent가 자리 만들기·profile 해석 → 세션 시작 → 보고 감시 → (배경) brief 안내
    pub(crate) async fn thread_start(self: &Arc<Self>, a: StartArgs, actor: &str) -> Result<ThreadView, ErrorBody> {
        self.thread_start_mode(a, actor, false).await
    }

    pub(crate) async fn thread_start_mode(
        self: &Arc<Self>,
        a: StartArgs,
        actor: &str,
        background: bool,
    ) -> Result<ThreadView, ErrorBody> {
        self.thread_start_key(a, actor, background, None).await
    }

    pub(crate) async fn thread_start_once(
        self: &Arc<Self>,
        a: StartArgs,
        actor: &str,
        origin: &StartOrigin,
    ) -> Result<ThreadView, ErrorBody> {
        self.thread_start_key(a, actor, true, Some(origin)).await
    }

    pub(super) async fn thread_start_key(
        self: &Arc<Self>,
        mut a: StartArgs,
        actor: &str,
        background: bool,
        origin: Option<&StartOrigin>,
    ) -> Result<ThreadView, ErrorBody> {
        let _account_read = self.account_rename_gate.read().await;
        let pool_key = a.pool.as_ref().map(|p| crate::account_pool::receipt(&a.project, &p.id));
        if let Some(key) = &pool_key
            && let Some(id) = self.db.meta(key).map_err(internal)?
        {
            return self.thread_show(&a.project, &id);
        }
        if a.over_limit.is_some() && (origin.is_some() || a.reservation.is_some() || a.role != Role::Worker) {
            return Err(bad("예약·자동 시작은 실행 한도를 무시할 수 없다"));
        }
        if a.role == Role::Evaluator {
            return Err(crate::legacy_mission::removed());
        }
        let dir = self.project_dir(&a.project)?;
        self.check_active(&a.project)?;
        if a.holds.iter().any(|h| !aaa_proto::valid_project(h)) {
            return Err(bad("holds는 timing 같은 표지 이름 목록이어야 한다"));
        }
        a.holds.sort();
        a.holds.dedup();
        if let Some((machine, path)) = a.repo.as_deref().and_then(|r| r.split_once(':'))
            && !machine.is_empty()
            && !(machine.len() == 1 && machine.as_bytes()[0].is_ascii_alphabetic())
            && machine.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(bad(format!(
                "--repo에는 머신:경로를 쓸 수 없다. `--machine {machine} --repo {path}`처럼 머신과 경로를 나눈다"
            )));
        }
        if a.title.trim().is_empty() {
            return Err(bad("제목이 비었다"));
        }
        if a.task.trim().is_empty() {
            return Err(bad("할 일이 비었다"));
        }
        if a.profile.is_empty() {
            a.profile = self.default_thread_profile(&a.project);
        }
        self.check_profile_allowed(&a.project, &a.profile)?;
        if a.kind == ThreadKind::Adopted {
            return Err(bad("데려온 thread는 `aaa thread adopt`로 만든다"));
        }
        if a.kind == ThreadKind::Worktree && a.repo.is_none() {
            return Err(bad("worktree thread에는 --repo(그 머신의 repo 경로)가 필요하다. repo 없이 하려면 --kind dir"));
        }
        let mid = self.resolve_machine(a.machine.as_deref())?;
        self.check_automatic_workspace(&a, &mid)?;
        self.check_machine_reservation(&a, &mid)?;
        let conn = self.live_conn(&mid)?;
        self.check_health(&mid)?;
        // task 첨부는 그 머신 작업 폴더로 넘긴다: 받지 못하는 agent면 thread를 만들기 전에 거부한다(plan §8.327)
        if !a.attachments.is_empty() && !self.machine_feature(&mid, crate::task_attach::FEATURE) {
            return Err(err(
                ErrorCode::Unavailable,
                format!(
                    "{} agent가 task 첨부를 받지 못한다({} 없음). 그 머신 aaa를 갱신하거나 첨부를 다른 머신에서 맡긴다",
                    self.machine_name(&mid),
                    crate::task_attach::FEATURE
                ),
            ));
        }
        let profile = self.resolved_profile(Some(&a.project), &mid, &a.profile)?;
        let review = a.start_task.as_ref().and_then(|t| t.review.clone());
        self.check_implicit_review_profile(review.as_ref(), &profile)?;
        let agy_subscription = if profile.harness == "agy" {
            if a.account.is_some() || a.role != Role::Worker {
                return Err(bad("agy는 실행 머신의 개인 로그인으로 worker를 시작한다"));
            }
            Some(self.agy_subscription(&mid)?)
        } else {
            None
        };
        if a.reservation.is_some()
            && let Some(account) = &a.account
        {
            self.check_reserved_account(account, &profile)?;
        }
        let auto_account = a.account.is_none() && self.pool_harness_enabled(&profile.harness)?;
        if a.account.is_none() && !auto_account {
            a.account = self.default_account(&profile.harness)?;
        }
        if auto_account && !self.machine_feature(&mid, "account_auth.v1") {
            return Err(bad("이 머신 agent는 계정 인증 전달을 지원하지 않는다"));
        }
        if let Some(account) = &a.account {
            let c = self.accounts.credential(account)?;
            if c.harness != profile.harness {
                return Err(bad("계정과 profile의 harness가 다르다"));
            }
            if !self.machine_feature(&mid, "account_auth.v1") {
                return Err(bad("이 머신 agent는 계정 인증 전달을 지원하지 않는다"));
            }
        }
        let mut selection = if auto_account || a.account.is_some() {
            self.account_selection_input(
                &a.project,
                &profile,
                a.account.as_deref(),
                a.pool.as_ref().map_or(aaa_proto::AccountTaskSize::Medium, |p| p.size),
            )
            .await?
        } else {
            None
        };
        if let Some(input) = &mut selection
            && let Some(origin) = origin
        {
            input.minimum = origin.usage_minimum;
        }
        // 자동화 do.start는 자체 실행 기록으로 관리한다. 사용자·coordinator 기동은 할 일을 먼저 남긴다.
        if origin.is_none() && a.start_task.is_none() && a.design.is_none() {
            a.machine = Some(self.machine_name(&mid));
            a.start_task = Some(self.create_start_task(&a, actor).await?);
        }
        let resolved_profile = Some(profile);
        let warnings = Vec::new();
        let now = aaa_common::util::now_ms();
        let mut rec = ThreadRec {
            account: a.account.clone(),
            review,
            holds: a.holds.clone(),
            started_by: Some(actor.into()),
            over_limit: a.over_limit.clone(),
            automation: origin.map(|o| o.rule.clone()),
            automation_run: origin.map(|o| o.run.clone()),
            repo: a.repo.clone(),
            base: a.base.clone(),
            attachments: a.attachments.clone(),
            task_id: a.start_task.as_ref().and_then(|t| t.id.clone()),
            design: a.design.clone(),
            ..ThreadRec::starting(&a.project, a.title.trim(), &mid, &a.profile, a.role, a.kind, now)
        };
        // 내용 승인판 검사와 DB 선점 사이에 host의 파일 쓰기가 끼지 못하게 한다.
        let task_lock = self.file_locks.of(&dir.join("TASKS.md"));
        let task_guard = task_lock.lock().unwrap();
        if let Some((task, block)) = &a.reservation {
            let rev = crate::file_rev::read(&dir.join("TASKS.md")).map_err(internal)?;
            if !crate::tasks::parse(&rev.text).iter().any(|t| t.id.as_ref() == Some(task) && &t.block_sha256 == block) {
                return Err(err(ErrorCode::Conflict, "예약 내용이 선점 전에 바뀌었다"));
            }
        }
        if let Some(t) = &a.start_task {
            let rev = crate::file_rev::read(&dir.join("TASKS.md")).map_err(internal)?;
            if !crate::tasks::parse(&rev.text).iter().any(|now| now.id == t.id && now.block_sha256 == t.block_sha256) {
                return Err(err(ErrorCode::Conflict, "시작할 할 일이 그사이 바뀌었다. 최신 할 일에서 다시 시작한다"));
            }
        }
        // Starting이 복구기에 보이기 전에 기동 task의 잠금을 확보한다.
        // commit→claim 사이에 ticker가 새 행을 중단된 기동으로 처리할 수 없다.
        let mut guard = None;
        let mut claim_error = None;
        let inserted = self
            .db
            .thread_insert_transaction(
                &a.project,
                pool_key.as_deref().or_else(|| origin.map(|o| o.key.as_str())),
                origin.map(|o| &o.admission),
                (a.over_limit.is_none() && a.design.is_none()).then(|| self.settings(&a.project).max_parallel_threads),
                a.reservation.as_ref().map(|(id, b)| (id.as_str(), b.as_str())),
                |tx, id| {
                    if a.machine_override.is_none() {
                        crate::machine_reserve::check_tx(
                            tx,
                            &mid,
                            &a.project,
                            a.start_task.as_ref().and_then(|t| t.id.as_deref()),
                            aaa_common::util::now_ms(),
                        )?;
                    }
                    if let Some(input) = &selection {
                        let (account, allocation) = input.select(tx, aaa_common::util::now_ms())?;
                        anyhow::ensure!(
                            self.accounts
                                .generation(&account)
                                .inspect_err(|error| aaa_common::diagnostic::observe(
                                    error,
                                    "선택 자료를 읽지 못해 기존 fallback을 쓴다"
                                ))
                                .ok()
                                .as_deref()
                                == Some(&allocation.generation),
                            "start_wait: 계정 인증 세대 변경 · 새 관측 대기"
                        );
                        rec.account = Some(account);
                        rec.account_allocation = Some(allocation);
                    }
                    if let Some(alias) = &agy_subscription {
                        crate::db::agy_slot::reserve(tx, alias, &mid, &format!("{}/{id}", a.project))?;
                    }
                    guard = Some(self.claim_thread(&a.project, id).map_err(|error| {
                        let message = error.message.clone();
                        claim_error = Some(error);
                        anyhow::anyhow!(message)
                    })?);
                    let mut r = rec.clone();
                    r.id = id.to_string();
                    Ok(serde_json::to_string(&r).unwrap())
                },
            )
            .map_err(|e| {
                if let Some(error) = claim_error {
                    error
                } else if e.to_string().starts_with("start_wait:") {
                    err(ErrorCode::Unavailable, e.to_string())
                } else {
                    internal(e)
                }
            });
        drop(task_guard);
        let (id, created) = inserted?;
        if !created {
            return self.thread_show(&a.project, &id);
        }
        rec.id = id.clone();
        // pane·worktree를 만들기 전에 연결한다. 연결에 실패한 기동은 실패 상태로 남기고 띄우지 않는다.
        if let Some(t) = &a.start_task
            && let Some(why) = self.link_started(&a.project, t, &id, actor).await
        {
            self.update_rec(&a.project, &id, |r| {
                r.status = ThreadStatus::Failed;
                r.error = Some(why.clone());
            })?;
            self.db.cancel_unspawned_agy_slot(&mid, &format!("{}/{id}", a.project)).map_err(internal)?;
            return Err(err(ErrorCode::Conflict, why));
        }
        #[cfg(test)]
        {
            let hook = self.start_admitted_hook.lock().unwrap().clone();
            if let Some(hook) = hook {
                hook(&id);
            }
        }
        let guard = guard.ok_or_else(|| internal("새 기동의 작업 잠금이 없다"))?;
        if let Some(override_) = &rec.over_limit {
            self.db.audit(
                actor,
                "thread_start_over_limit",
                &format!(
                    "{}/{id} at={} 사용자 지시: {}",
                    a.project,
                    override_.at_ms,
                    override_.user_request.as_deref().unwrap_or("직접 시작")
                ),
            );
        }
        self.db.audit(actor, "thread_start", &format!("{}/{id} {} @{}", a.project, a.profile, self.machine_name(&mid)));
        if let Err(e) = write_atomic(&dir.join("threads").join(format!("{id}.task.md")), a.task.as_bytes()) {
            self.update_rec(&a.project, &id, |r| {
                r.status = ThreadStatus::Failed;
                r.error = Some(e.to_string());
            })?;
            self.db.cancel_unspawned_agy_slot(&mid, &format!("{}/{id}", a.project)).map_err(internal)?;
            return Err(internal(e));
        }
        let brief = self.brief(&dir, &rec, &a.task, false);
        // 세션이 생기기 전에도 웹·coordinator가 준비 중인 thread를 볼 수 있어야 한다.
        aaa_common::diagnostic::ignored(
            self.thread_events.send(self.thread_view(&rec)),
            "수신자가 끝난 뒤에도 현재 작업은 마친다",
        );
        let detail = if let Some(o) = &rec.over_limit {
            format!(
                "{} · 한도 초과로 시작 · 사용자 지시: {}",
                rec.title,
                o.user_request.as_deref().unwrap_or("직접 시작")
            )
        } else {
            rec.title.clone()
        };
        self.flow(&rec, "thread_started", Some(format!("threads/{id}.task.md")), detail);

        let view = self.thread_view(&rec);
        let host = self.clone();
        let actor = actor.to_string();
        // 별도 task가 소유하므로 CLI·웹 연결의 수명과 관계없이 기록까지 마친다.
        let job = tokio::spawn(async move {
            let _guard = guard;
            if let Err(e) = host.place_and_launch(&conn, &mut rec, brief, resolved_profile, &actor).await {
                if let Ok(r) = host.note_launch_failure(&rec.project, &rec.id, &e, false) {
                    rec = r;
                }
                host.flow(&rec, "thread_failed", None, e.message.clone());
                return Err(ErrorBody {
                    message: format!(
                        "thread {}를 시작하지 못했다. `aaa thread restart {} {}`로 다시 한다: {}",
                        rec.id, rec.project, rec.id, e.message
                    ),
                    ..e
                });
            }
            host.spawn_brief(&rec.project, &rec.id);
            Ok(host.thread_view(&rec))
        });
        let mut view = if background { view } else { job.await.map_err(internal)?? };
        view.warnings = warnings;
        Ok(view)
    }

    /// 자동화·병합 대기와 착지 한 자리를 뺀다. DB 기동 선점·예약도 같은 계산을 쓴다.
    pub(crate) fn open_worker_threads(&self, project: &str) -> u32 {
        self.db.worker_slots(project).unwrap_or(u32::MAX)
    }

    pub(crate) async fn screen_lines(&self, mid: &str, pane: &str) -> Option<Vec<String>> {
        self.screen(mid, pane).await.map(|s| s.lines)
    }

    /// 보이는 화면(글자 줄과 SGR 모양을 담은 `ansi`)
    pub(crate) async fn screen(&self, mid: &str, pane: &str) -> Option<aaa_proto::ScreenText> {
        let conn = self.conn_of(mid)?;
        match conn.request(AgentRequest::Screen { session: pane.to_string() }).await {
            Ok(AgentResponse::Screen(s)) => Some(s),
            _ => None,
        }
    }

    /// 그 머신 agent가 자리(worktree·작업 폴더)와 brief.md를 만들고 profile을 해석한 뒤 세션을 띄운다. 시작과, 아무것도
    /// 만들지 못하고 실패한 thread의 다시 시작이 쓴다(herdr-projects threads.rs `place_and_brief`, `RestartPlan::Create`).
    pub(super) async fn place_and_launch(
        self: &Arc<Self>,
        conn: &crate::AgentConn,
        rec: &mut ThreadRec,
        brief: String,
        resolved_profile: Option<aaa_proto::ResolvedProfile>,
        actor: &str,
    ) -> Result<(), ErrorBody> {
        let prep = aaa_proto::ThreadPrepare {
            project: rec.project.clone(),
            thread: rec.id.clone(),
            title: rec.title.clone(),
            kind: rec.kind,
            repo: rec.repo.clone(),
            base: rec.base.clone(),
            profile: rec.profile.clone(),
            brief,
            safety: None,
            resolved_profile,
        };
        tracing::debug!(target: "aaa::thread_prepare", project = %rec.project, thread = %rec.id, "thread 준비 요청 전송 시작");
        let result = conn.request(AgentRequest::ThreadPrepare(prep)).await;
        if let Err(error) = &result {
            tracing::debug!(target: "aaa::thread_prepare", project = %rec.project, thread = %rec.id, code = ?error.code,
                message = %error.message, "thread 준비 응답 오류");
        }
        let ready = match result? {
            AgentResponse::ThreadReady(r) => r,
            other => return Err(internal(format!("예상하지 못한 응답: {other:?}"))),
        };
        *rec = self.update_rec(&rec.project, &rec.id, |r| {
            r.cwd = Some(ready.cwd.clone());
            r.work_dir = Some(ready.work_dir.clone());
            r.branch = ready.branch.clone();
            r.base = ready.base.clone();
            r.launch = Some(ready.launch.clone());
        })?;
        self.launch_session(rec, &ready.launch.argv, actor).await
    }

    /// thread 세션을 띄우고 보고 감시를 건다(시작·다시 시작 공통). 띄우면 열린 thread다(전의 시작 실패는 지운다,
    /// herdr-projects `finish_placement`).
    pub(super) async fn launch_session(
        self: &Arc<Self>,
        rec: &mut ThreadRec,
        argv: &[String],
        actor: &str,
    ) -> Result<(), ErrorBody> {
        self.automation_prepare(rec).await?;
        self.task_attach_transfer(rec).await?;
        let launch = rec.launch.clone().ok_or_else(|| internal("launch 기록이 없다"))?;
        let run = format!("{}/{}", rec.project, rec.id);
        if launch.harness == "agy" {
            if self.safety(&rec.project).yolo {
                return Err(bad("유효 yolo가 켜져 있어 agy를 시작할 수 없다"));
            }
            if !self.machine_feature(&rec.machine_id, aaa_proto::AGY_WORKER_FEATURE) {
                return Err(bad("그 머신 agent는 agy 자동 thread를 지원하지 않는다"));
            }
            if rec.account.is_some() || rec.role != Role::Worker {
                return Err(bad("agy는 실행 머신의 개인 로그인으로 worker를 시작한다"));
            }
            let alias = self.agy_subscription(&rec.machine_id)?;
            self.db
                .reserve_agy_slot(&alias, &rec.machine_id, &run)
                .map_err(|e| err(ErrorCode::Unavailable, e.to_string()))?;
        }
        let old_pane = rec.pane.clone();
        let mut env = launch.env.clone();
        env.retain(|(key, _)| key != aaa_proto::AUTOMATION_REENTRY_ENV);
        if let Some(text) = crate::automation_thread::reentry(rec) {
            env.push((aaa_proto::AUTOMATION_REENTRY_ENV.into(), text));
        }
        env.push(("AAA_PROJECT".into(), rec.project.clone()));
        env.push(("AAA_THREAD".into(), rec.id.clone()));
        let spec = SpawnSpec {
            name: run.clone(),
            argv: argv.to_vec(),
            cwd: rec.cwd.clone(),
            env,
            cols: 120,
            rows: 32,
            role: Some(rec.role),
            // agent가 이 harness(claude·codex)의 화면으로 상태를 본다(plan §8.110)
            harness: Some(launch.harness.clone()),
            profile: None,
            // agent가 머신별 env를 합친다. 재개 argv는 저장된 실행 계약을 유지한다.
            resolved_profile: self
                .resolved_profile(Some(&rec.project), &rec.machine_id, &launch.profile)
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok(),
        };
        let pane = if let Some(name) = &rec.account {
            let mut credential = self.accounts.credential(name)?;
            if rec
                .account_allocation
                .as_ref()
                .is_some_and(|a| credential.auth_generation.as_deref() != Some(&a.generation))
            {
                return Err(bad("기동 전 계정 인증 세대가 바뀌었다. 새 인증으로 명시 재시작한다"));
            }
            if credential.harness != launch.harness {
                return Err(bad("계정과 profile의 harness가 다르다"));
            }
            if !self.machine_feature(&rec.machine_id, "account_auth.v1") {
                return Err(bad("계정 인증을 지원하지 않는 agent다"));
            }
            credential.run = format!("{}/{}", rec.project, rec.id);
            if self.trust_screens(&rec.project) == crate::trust_screen::COORDINATOR {
                credential.trust_cwd = rec.cwd.clone();
            }
            let expected_pane = if rec.pane.is_none() && rec.park.as_ref().is_some_and(|p| p.parked_at_ms.is_some()) {
                Some(self.parked_account_pane(&credential.run, name, &launch.harness)?)
            } else {
                rec.pane.clone()
            };
            *rec = self.update_rec(&rec.project, &rec.id, |r| r.launch_sent = true)?;
            self.spawn_account_on(&rec.machine_id, spec, credential, actor, expected_pane.as_deref()).await?
        } else {
            *rec = self.update_rec(&rec.project, &rec.id, |r| r.launch_sent = true)?;
            self.spawn_on(&rec.machine_id, spec, actor).await?
        };
        if launch.harness == "agy" {
            self.db.bind_agy_slot(&rec.machine_id, &run, &pane).map_err(internal)?;
            let exited = self.inner.lock().unwrap().panes.get(&pane).is_some_and(|p| {
                matches!(p.info.state, aaa_proto::SessionState::Exited { .. } | aaa_proto::SessionState::Interrupted)
            });
            if exited {
                self.db.release_agy_pane(&rec.machine_id, &pane).map_err(internal)?;
            }
        }
        *rec = self.update_rec(&rec.project, &rec.id, |r| {
            r.pane = Some(pane.clone());
            r.status = ThreadStatus::Open;
            r.error = None;
        })?;
        if let Some(old) =
            old_pane.filter(|old| old != &pane && self.pane_view_of(old, aaa_proto::DeviceRole::Local).is_some())
        {
            self.replace_pane(&old, &pane, &rec.machine_id, actor).await;
        }
        if let (Ok(conn), Some(wd)) = (self.live_conn(&rec.machine_id), rec.work_dir.clone()) {
            aaa_common::diagnostic::ignored(
                conn.request(AgentRequest::ThreadWatch { thread: format!("{}/{}", rec.project, rec.id), work_dir: wd })
                    .await,
                "보조 작업 실패 뒤 기존 처리를 계속한다",
            );
        }
        Ok(())
    }
}
