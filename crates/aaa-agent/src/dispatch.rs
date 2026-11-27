//! host가 보낸 Agent link 요청을 처리한다.

use super::*;

impl Agent {
    /// 페이지로 그린 HTML 파일의 한 조각. 파일이 그대로면 마지막으로 그린 것을 다시 쓴다(조각마다 다시 그리지 않는다)
    pub(super) fn page_chunk(
        &self,
        path: &str,
        offset: u64,
        len: u32,
        roots: &files::Roots,
    ) -> anyhow::Result<(Vec<u8>, bool)> {
        let (size, modified) = files::page_key(path, roots)?;
        let cached = self
            .page_cache
            .lock()
            .unwrap()
            .as_ref()
            .filter(|c| c.0 == path && c.1 == size && c.2 == modified)
            .map(|c| c.3.clone());
        let bytes = match cached {
            Some(b) => b,
            None => {
                let b = Arc::new(page::render(Path::new(path), roots)?);
                *self.page_cache.lock().unwrap() = Some((path.to_string(), size, modified, b.clone()));
                b
            }
        };
        let start = (offset as usize).min(bytes.len());
        let end = start.saturating_add(len.min(files::MAX_CHUNK) as usize).min(bytes.len());
        Ok((bytes[start..end].to_vec(), end >= bytes.len()))
    }

    /// 파일 보기: 세션이 지금 있는 폴더(없으면 시작 폴더) 기준으로 경로를 찾는다
    pub(super) async fn file_locate(&self, session: &str, path: String) -> Result<AgentResponse, ErrorBody> {
        let started = {
            let s = self.sessions.lock().unwrap();
            s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?.info.cwd.clone()
        };
        // 지금 폴더를 못 읽어도 찾는다(참고 `paneContext` 실패: 절대·~ 경로는 그대로 열린다)
        let cwd = match self.session_cwd(session).await {
            Ok(AgentResponse::Cwd { cwd: Some(c) }) => Some(c),
            _ => started,
        };
        self.locate_in(cwd, path).await
    }

    /// 정한 폴더 기준으로 찾는다(보고서 안 파일 링크: thread worktree, plan §8.171). 규칙·허용 폴더는 `file_locate`와 같다
    pub(super) async fn locate_in(&self, cwd: Option<String>, path: String) -> Result<AgentResponse, ErrorBody> {
        let roots = self.file_roots.lock().unwrap().clone();
        let home = aaa_config::paths::home();
        let found =
            tokio::task::spawn_blocking(move || files::locate(&path, cwd.as_deref().map(Path::new), &home, &roots))
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        match found {
            files::Located::Found(info) => Ok(AgentResponse::FileLocated { info: Some(info), candidates: Vec::new() }),
            files::Located::Candidates(candidates) => Ok(AgentResponse::FileLocated { info: None, candidates }),
            files::Located::Missing => Err(ErrorBody::new(ErrorCode::NotFound, "이 경로에 읽을 수 있는 파일이 없다")),
            files::Located::Outside => Err(ErrorBody::new(
                ErrorCode::Forbidden,
                "허용한 폴더 밖의 파일이라 열지 않는다(그 머신 agent.toml [agent] file_roots)",
            )),
        }
    }

    /// 폴더 보기: 세션이 지금 있는 폴더(없으면 시작 폴더) 기준으로 폴더 한 단계를 읽는다(`file_locate`와 같은 기준)
    pub(super) async fn dir_list(&self, session: &str, path: String, hidden: bool) -> Result<AgentResponse, ErrorBody> {
        let started = {
            let s = self.sessions.lock().unwrap();
            s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?.info.cwd.clone()
        };
        let cwd = match self.session_cwd(session).await {
            Ok(AgentResponse::Cwd { cwd: Some(c) }) => Some(c),
            _ => started,
        };
        let roots = self.file_roots.lock().unwrap().clone();
        let home = aaa_config::paths::home();
        let listed = tokio::task::spawn_blocking(move || {
            files::list_dir(&path, cwd.as_deref().map(Path::new), &home, &roots, hidden)
        })
        .await
        .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        match listed {
            files::Listed::Dir(d) => Ok(AgentResponse::DirListed(d)),
            files::Listed::Missing => Err(ErrorBody::new(ErrorCode::NotFound, "이 경로에 읽을 수 있는 폴더가 없다")),
            files::Listed::Outside => Err(ErrorBody::new(
                ErrorCode::Forbidden,
                "허용한 폴더 밖이라 열지 않는다(그 머신 agent.toml [agent] file_roots)",
            )),
        }
    }

    /// 실행 중인 세션이 지금 있는 폴더(새 탭·나누기 cwd follow). 끝났거나 읽지 못하면 None.
    /// 입력칸 「/」 목록(plan §8.323): 세션이 지금 있는 폴더(못 읽으면 띄운 폴더)를 프로젝트로 본다
    pub(super) async fn slash_commands(&self, session: &str, harness: String) -> Result<AgentResponse, ErrorBody> {
        let started = {
            let s = self.sessions.lock().unwrap();
            s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?.info.cwd.clone()
        };
        let cwd = match self.session_cwd(session).await {
            Ok(AgentResponse::Cwd { cwd: Some(c) }) => Some(c),
            _ => started,
        };
        let list = tokio::task::spawn_blocking(move || {
            slash::list(&harness, cwd.as_deref().map(Path::new), &chat::claude_home(), &chat::codex_home())
        })
        .await
        .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?;
        Ok(AgentResponse::Commands(list))
    }

    pub(super) async fn session_cwd(&self, session: &str) -> Result<AgentResponse, ErrorBody> {
        let pid = {
            let s = self.sessions.lock().unwrap();
            let e = s.get(session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?;
            e.info.state.is_running().then_some(e.info.pid).flatten()
        };
        let cwd = match pid {
            Some(pid) => tokio::task::spawn_blocking(move || cwd::follow_cwd(pid))
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?,
            None => None,
        };
        Ok(AgentResponse::Cwd { cwd: cwd.map(|p| p.to_string_lossy().into_owned()) })
    }

    /// 사람이 띄운 세션을 thread로 데려온다: 세션이 지금 있는 폴더(앞 프로그램 기준, 못 읽으면 띄운 폴더)에 작업 폴더를 만든다
    pub(super) async fn thread_adopt(&self, a: aaa_proto::ThreadAdopt) -> Result<AgentResponse, ErrorBody> {
        let (pid, spawned) = {
            let s = self.sessions.lock().unwrap();
            let e = s.get(&a.session).ok_or_else(|| ErrorBody::new(ErrorCode::NotFound, "세션이 없다"))?;
            if !e.info.state.is_running() {
                return Err(ErrorBody::new(ErrorCode::BadRequest, "실행 중인 세션만 데려온다"));
            }
            (e.info.pid, e.info.cwd.clone())
        };
        let live = match pid {
            Some(pid) => tokio::task::spawn_blocking(move || cwd::follow_cwd(pid))
                .await
                .inspect_err(|error| {
                    aaa_common::diagnostic::observe(error, "선택 자료를 읽지 못해 기존 fallback을 쓴다")
                })
                .ok()
                .flatten(),
            None => None,
        };
        let Some(dir) = live.or_else(|| spawned.map(PathBuf::from)) else {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 폴더를 모른다"));
        };
        let exe = self.threads_cfg.lock().unwrap().exe.clone();
        threads::adopt(&exe, a, &dir)
            .await
            .map(AgentResponse::ThreadAdopted)
            .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
    }

    /// 시험용: host 연결의 응답 길로 요청 하나를 처리한다. production은 `handle_deadline`을 부른다.
    #[cfg(test)]
    pub(crate) async fn handle(self: Arc<Self>, id: String, body: AgentRequest) {
        let reply = self.host_tx();
        self.handle_deadline(id, body, None, reply).await;
    }

    /// host 요청 하나. 세션 요청은 pump로 넘기고(응답은 pump가 보낸다), 나머지는 여기서 답한다.
    /// 연결이 끊겨도 이 task는 끝까지 수행한다. 자리·정리 결과는 agent journal에 남긴다.
    pub async fn handle_deadline(
        self: Arc<Self>,
        id: String,
        body: AgentRequest,
        deadline_ms: Option<u64>,
        reply: Option<FrameTx>,
    ) {
        let wait = Duration::from_millis(deadline_ms.unwrap_or(600_000).clamp(1, 86_400_000));
        let direct: Result<AgentResponse, ErrorBody> = match body {
            req @ (AgentRequest::DeployGuardArm { .. } | AgentRequest::DeployGuardControl { .. }) => {
                self.guard_request(req).await
            }
            req @ (AgentRequest::DeployFetch { .. }
            | AgentRequest::DeployCandidate { .. }
            | AgentRequest::DeployStage { .. }
            | AgentRequest::DeployStageStatus { .. }
            | AgentRequest::DeployStageClean { .. }) => {
                let agent = self.clone();
                tokio::task::spawn_blocking(move || agent.deployments.transport(&agent.exe, req))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r)
            }
            AgentRequest::DeployStart { id: job, project, action } => {
                return self.deploy_start(id, job, project, action).await;
            }
            AgentRequest::DeployStatus { id } => self.deployments.status(&id).map(|job| self.deployments.response(job)),
            AgentRequest::ThreadReadReply { request_id, result } => {
                if let Some(tx) = self.read_waiters.lock().unwrap().remove(&request_id) {
                    aaa_common::diagnostic::ignored(tx.send(result), "읽기 응답 수신자가 끝났다");
                }
                return;
            }
            AgentRequest::ImageReply { request_id, result } => {
                if let Some(tx) = self.image_waiters.lock().unwrap().remove(&request_id) {
                    aaa_common::diagnostic::ignored(tx.send(result), "수신자가 끝난 뒤에도 현재 작업은 마친다");
                }
                return;
            }
            AgentRequest::ReviewReply { request_id, result } => {
                if let Some(tx) = self.review_waiters.lock().unwrap().remove(&request_id) {
                    aaa_common::diagnostic::ignored(tx.send(result), "검수 응답 수신자가 끝났다");
                }
                return;
            }
            AgentRequest::ReviewRun(run) => Ok(AgentResponse::ReviewFinished(self.run_review(*run).await)),
            AgentRequest::ReviewCancel { id } => self.cancel_review(&id).map(|()| AgentResponse::Done),
            AgentRequest::PostAck { post_id, result } => {
                self.post_ack(&post_id, result);
                return;
            }
            AgentRequest::MachineLabel { name } => self.machine_label(&name),
            AgentRequest::AccountQuota { session, generation } => self
                .quota_snapshot(&session, &generation)
                .map(|snapshot| AgentResponse::AccountQuota { snapshot })
                .map_err(|source_error| {
                    aaa_common::diagnostic::context(
                        source_error,
                        ErrorBody::new(ErrorCode::BadRequest, "사용량 관측을 읽지 못했다"),
                    )
                }),
            AgentRequest::SpawnAccount { spec, credential } => self
                .spawn_session_inner(spec, Some(credential))
                .await
                .map(AgentResponse::Session)
                .map_err(account_spawn::error),
            AgentRequest::AccountRefreshed { session, result } => {
                if let Some(v) = &result.response {
                    aaa_common::redact::remember_auth("codex", &v.0);
                }
                return self.to_session(id, &session, SessionRequest::AccountRefreshed(result)).await;
            }
            AgentRequest::Spawn(spec) => self
                .spawn_session(spec)
                .await
                .map(AgentResponse::Session)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::List => Ok(AgentResponse::Sessions { sessions: self.sessions() }),
            AgentRequest::Subscribe { session, from } => {
                return self.to_session(id, &session, SessionRequest::Subscribe { from }).await;
            }
            AgentRequest::Unsubscribe { session } => {
                return self.to_session(id, &session, SessionRequest::Unsubscribe).await;
            }
            AgentRequest::Input { session, data } => {
                if data.0.starts_with(b"\x1b[<") {
                    tracing::debug!(request = %id, %session, bytes = data.0.len(), "pane 입력: agent 수신");
                } else {
                    tracing::info!(request = %id, %session, bytes = data.0.len(), "pane 입력: agent 수신");
                }
                return self.to_session(id, &session, SessionRequest::Input { data }).await;
            }
            AgentRequest::Resize { session, cols, rows } => {
                return self.to_session(id, &session, SessionRequest::Resize { cols, rows }).await;
            }
            AgentRequest::Kill { session } => return self.to_session(id, &session, SessionRequest::Kill).await,
            AgentRequest::Screen { session } => return self.to_session(id, &session, SessionRequest::Screen).await,
            AgentRequest::History { session, anchor, depth } => {
                return self.to_session(id, &session, SessionRequest::History { anchor, depth }).await;
            }
            AgentRequest::Forget { session } => self.forget(&session).await,
            AgentRequest::Rename { session, label } => self.rename(&session, label).await,
            AgentRequest::Cwd { session } => self.session_cwd(&session).await,
            AgentRequest::FileLocate { session, path } => self.file_locate(&session, path).await,
            AgentRequest::FileLocateIn { dir, path } => self.locate_in(Some(dir), path).await,
            AgentRequest::DirList { session, path, hidden } => self.dir_list(&session, path, hidden).await,
            AgentRequest::Commands { session, harness } => self.slash_commands(&session, harness).await,
            AgentRequest::FileView { path, offset, len, page } => {
                let roots = self.file_roots.lock().unwrap().clone();
                let me = self.clone();
                tokio::task::spawn_blocking(move || {
                    if page {
                        me.page_chunk(&path, offset, len, &roots)
                    } else {
                        files::read(&path, offset, len, &roots)
                    }
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                .map(|(data, eof)| AgentResponse::FileChunk { data: B64(data), eof })
            }
            AgentRequest::Chat {
                session,
                harness,
                transcript,
                harness_session,
                thinking,
                compact,
                commands,
                before,
            } => {
                let path =
                    self.chat_path(session.as_deref(), &harness, transcript.as_deref(), harness_session.as_deref());
                tokio::task::spawn_blocking(move || {
                    chat::read_path(&harness, path?, thinking, compact, commands, before.as_deref())
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| match r {
                    Ok(page) => Ok(AgentResponse::Chat {
                        truncated: page.before.is_some(),
                        messages: page.messages,
                        before: page.before,
                        history: Some(page.history),
                    }),
                    Err(chat::Error::Missing) => {
                        Ok(AgentResponse::Chat { truncated: false, messages: vec![], before: None, history: None })
                    }
                    Err(error) => Err(ErrorBody::new(ErrorCode::NotFound, error.to_string())),
                })
            }
            AgentRequest::Harnesses => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                cfg.fresh()
                    .map(|c| AgentResponse::Profiles { profiles: threads::builtin_views(&c) })
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::Profiles => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                cfg.fresh()
                    .map(|c| AgentResponse::Profiles { profiles: threads::profile_views(&c) })
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::ThreadPrepare(p) => {
                tracing::debug!(target: "aaa::thread_prepare", request = %id, project = %p.project, thread = %p.thread,
                    wait_ms = wait.as_millis() as u64, "thread 준비 실행");
                let cfg = self.threads_cfg.lock().unwrap().clone();
                let result = match cfg.fresh() {
                    Ok(cfg) => threads::prepare(&cfg, p, wait).await,
                    Err(e) => Err(e),
                };
                tracing::debug!(target: "aaa::thread_prepare", request = %id,
                    error = ?result.as_ref().err().map(|e| format!("{e:#}")), "thread 준비 종료");
                result
                    .map(AgentResponse::ThreadReady)
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::ThreadPlacement { project, thread, title, kind, repo } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                threads::placement(&cfg, &project, &thread, &title, kind, repo.as_deref())
                    .await
                    .map(|placement| AgentResponse::ThreadPlacement { placement })
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::ThreadAdopt(a) => self.thread_adopt(a).await,
            AgentRequest::ThreadWatch { thread, work_dir } => {
                // 감시 key는 `<project>/<thread>`다(thread 번호는 프로젝트마다 다시 센다)
                let ok = thread
                    .split_once('/')
                    .is_some_and(|(p, t)| aaa_proto::valid_project(p) && aaa_proto::valid_thread(t));
                if ok {
                    self.watch_report(thread, PathBuf::from(work_dir));
                    Ok(AgentResponse::Done)
                } else {
                    Err(ErrorBody::new(ErrorCode::BadRequest, "감시 key는 <project>/<thread> 형식이다"))
                }
            }
            AgentRequest::ThreadUnwatch { thread } => {
                if let Some(t) = self.watches.lock().unwrap().remove(&thread) {
                    t.abort();
                }
                Ok(AgentResponse::Done)
            }
            AgentRequest::ChatWatch { key, harness, transcript, harness_session, enabled } => {
                if enabled {
                    self.watch_chat(key, harness, transcript, harness_session);
                } else if let Some(t) = self.chat_watches.lock().unwrap().remove(&key) {
                    t.abort();
                }
                Ok(AgentResponse::Done)
            }
            AgentRequest::ReportHash { work_dir } => {
                tokio::task::spawn_blocking(move || threads::collect_hash(&work_dir))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(threads::report_error))
                    .map(|hash| AgentResponse::ReportHash { hash })
            }
            AgentRequest::ReportTransfer { work_dir, max_library_bytes } => {
                tokio::task::spawn_blocking(move || threads::collect(&work_dir, max_library_bytes))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(threads::report_error))
                    .map(AgentResponse::ReportTransfer)
            }
            AgentRequest::ReportFileRead { work_dir, path, offset, len } => {
                tokio::task::spawn_blocking(move || threads::read_chunk(&work_dir, &path, offset, len))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                    .map(|(data, eof)| AgentResponse::ReportFileChunk { data: B64(data), eof })
            }
            AgentRequest::ReportCollect { work_dir, max_library_bytes } => {
                tokio::task::spawn_blocking(move || threads::collect(&work_dir, max_library_bytes))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(threads::report_error))
                    .map(AgentResponse::Report)
            }
            AgentRequest::FileRead { work_dir, path, offset, len } => {
                tokio::task::spawn_blocking(move || threads::read_chunk(&work_dir, &path, offset, len))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::NotFound, format!("{e:#}"))))
                    .map(|(data, eof)| AgentResponse::FileChunk { data: B64(data), eof })
            }
            AgentRequest::ThreadRebrief { work_dir, brief, launch, profile, safety, resolved_profile } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                cfg.fresh()
                    .and_then(|cfg| {
                        threads::rebrief(
                            &cfg,
                            &work_dir,
                            &brief,
                            launch,
                            profile.as_deref(),
                            safety.as_ref(),
                            resolved_profile.as_ref(),
                        )
                    })
                    .map(AgentResponse::Relaunch)
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::RunCommand { project, cwd, command, timeout_ms } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                command::run(
                    &cfg.work_root,
                    cfg.work_mount.as_deref(),
                    &project,
                    cwd.as_deref(),
                    &command,
                    Duration::from_millis(timeout_ms),
                )
                .await
                .map(AgentResponse::CommandRan)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
            }
            AgentRequest::WorkFileList { work_dir } => {
                if !*self.host_work_files.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "work_files.v1 미협상"))
                } else {
                    threads::work_file_state_list(&work_dir)
                        .map(|files| AgentResponse::WorkFiles { files })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::WorkFileRead { work_dir, path, offset } => {
                if !*self.host_work_files.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "work_files.v1 미협상"))
                } else {
                    threads::work_file_state_read(&work_dir, &path, offset)
                        .map(|(data, eof)| AgentResponse::FileChunk { data: aaa_proto::B64(data), eof })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::WorkFileWriteChunk { work_dir, path, offset, data, eof, sha256 } => {
                if !*self.host_work_files.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "work_files.v1 미협상"))
                } else {
                    threads::work_file_state_write(&work_dir, &path, offset, &data.0, eof, &sha256)
                        .map(|_| AgentResponse::Done)
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::WorkFileInboxStatus { work_dir } => {
                if !*self.host_work_files.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "host가 work_files.v1을 알리지 않았다"))
                } else {
                    threads::work_file_inbox_pending(&work_dir)
                        .map(|names| AgentResponse::WorkFileInbox { pending: names.len() as u32, names })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::MissionStateList { work_dir } => {
                if !*self.host_mission.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "mission.v1 미협상"))
                } else {
                    threads::work_file_state_list(&work_dir)
                        .map(|files| AgentResponse::MissionFiles { files })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::MissionStateRead { work_dir, path, offset } => {
                if !*self.host_mission.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "mission.v1 미협상"))
                } else {
                    threads::work_file_state_read(&work_dir, &path, offset)
                        .map(|(data, eof)| AgentResponse::FileChunk { data: aaa_proto::B64(data), eof })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::MissionStateWrite { work_dir, path, offset, data, eof, sha256 } => {
                if !*self.host_mission.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "mission.v1 미협상"))
                } else {
                    threads::work_file_state_write(&work_dir, &path, offset, &data.0, eof, &sha256)
                        .map(|_| AgentResponse::Done)
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::MissionInboxStatus { work_dir } => {
                if !*self.host_mission.lock().unwrap() {
                    Err(ErrorBody::new(ErrorCode::Forbidden, "host가 mission.v1을 알리지 않았다"))
                } else {
                    threads::work_file_inbox_pending(&work_dir)
                        .map(|names| AgentResponse::MissionInbox { pending: names.len() as u32, names })
                        .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                }
            }
            AgentRequest::WorkFileWrite { work_dir, path, data } => {
                tokio::task::spawn_blocking(move || threads::write_inbox(&work_dir, &path, &data.0))
                    .await
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                    .and_then(|r| r.map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))))
                    .map(|()| AgentResponse::Done)
            }
            AgentRequest::AttachWrite { session, att, offset, data, eof } => {
                self.attach_write(&session, &att, offset, data.0, eof).await
            }
            AgentRequest::WorkAttachWrite { work_dir, att, offset, data, eof } => {
                tokio::task::spawn_blocking(move || {
                    threads::check_work_dir(&work_dir)
                        .and_then(|dir| attach::write_work(&dir, &att, offset, &data.0, eof))
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| {
                    r.map(|r| match r {
                        Some((path, sha256)) => AgentResponse::Attached { path, sha256 },
                        None => AgentResponse::Done,
                    })
                    .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
                })
            }
            AgentRequest::TermColors(c) => {
                self.set_term_colors(c).await;
                Ok(AgentResponse::Done)
            }
            AgentRequest::ThreadCleanupProbe { repo, targets } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                threads::cleanup_absent(&cfg, &self.state, &repo, &targets)
                    .await
                    .map(|absent| AgentResponse::CleanupAbsent { absent })
                    .map_err(|e| ErrorBody::new(ErrorCode::Unavailable, format!("자리 부재 확인: {e:#}")))
            }
            AgentRequest::ThreadCleanup { repo, cwd, branch, merged_head, keep_worktree, uncopied } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                match cfg.check_path(Path::new(&cwd)) {
                    Err(e) => Err(ErrorBody::new(ErrorCode::Unavailable, format!("{e:#}"))),
                    Ok(()) => threads::cleanup_recorded(
                        &self.state,
                        repo.as_deref(),
                        &cwd,
                        branch.as_deref(),
                        merged_head.as_deref(),
                        keep_worktree,
                        uncopied,
                        wait,
                    )
                    .await
                    .map(AgentResponse::Cleanup)
                    .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("{e:#}"))),
                }
            }
            AgentRequest::WorkspaceMetrics { cwd, workspace_key, known_code_commits } => {
                let cfg = self.threads_cfg.lock().unwrap().clone();
                let state = self.state.clone();
                tokio::task::spawn_blocking(move || {
                    let path = std::path::Path::new(&cwd);
                    cfg.check_path(path)?;
                    workspace_metrics::collect(&state, path, &workspace_key, &known_code_commits)
                })
                .await
                .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))
                .and_then(|r| {
                    r.map(AgentResponse::WorkspaceMetrics).map_err(|source_error| {
                        aaa_common::diagnostic::context(
                            source_error,
                            ErrorBody::new(ErrorCode::Unavailable, "지표 수집 자료를 읽지 못했다"),
                        )
                    })
                })
            }
            AgentRequest::GitQuery { cwd, query } => git_watch::query(&cwd, &query)
                .await
                .map(AgentResponse::Git)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::CiQuery { cwd, query } => ci::query(&cwd, &query)
                .await
                .map(AgentResponse::Ci)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::ThreadDiff { cwd, base, query } => diff::query(&cwd, base.as_deref(), &query)
                .await
                .map(AgentResponse::Diff)
                .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}"))),
            AgentRequest::DesignTree { repo, folder, branch } => design::tree(&repo, &folder, branch.as_deref()).await,
            AgentRequest::DesignRead { repo, folder, commit, path, branch } => {
                design::read(&repo, &folder, commit.as_deref(), &path, branch.as_deref()).await
            }
            AgentRequest::DesignFile { repo, folder, commit, path, offset, len, branch } => {
                design::file(&repo, &folder, &commit, &path, (offset, len), branch.as_deref()).await
            }
            AgentRequest::DesignOpen { .. }
            | AgentRequest::DesignSave { .. }
            | AgentRequest::DesignMarkIds { .. }
            | AgentRequest::DesignAttach { .. }
            | AgentRequest::DesignDetach { .. }
            | AgentRequest::DesignPush { .. }
            | AgentRequest::DesignClose { .. }
            | AgentRequest::DesignMainOpen { .. }
            | AgentRequest::DesignMainStage { .. }
            | AgentRequest::DesignMainWrite { .. }
            | AgentRequest::DesignMainMarkIds { .. } => {
                Err(ErrorBody::new(ErrorCode::BadRequest, "기획 편집은 일반 worktree에서 진행한다"))
            }
            AgentRequest::DesignLog { repo, folder, path, limit } => {
                Box::pin(design::log(&repo, &folder, &path, limit)).await
            }
            AgentRequest::WorkRoot { action } => settings_jobs::work_root(&self.state, action).await,
            AgentRequest::CodexHook { install } => settings_jobs::codex_hook(install).await,
            AgentRequest::ModelCatalog => model_catalog::catalog().await,
        };
        if let Some(reply) = reply {
            aaa_common::diagnostic::ignored(
                reply.link(&LinkMsg::Res { id, result: direct.into() }).await,
                "수신자가 끝난 뒤에도 현재 작업은 마친다",
            );
        }
    }

    /// 실행 중인 세션 폴더에 첨부 한 조각을 쓴다
    pub(super) async fn attach_write(
        &self,
        session: &str,
        att: &str,
        offset: u64,
        data: Vec<u8>,
        eof: bool,
    ) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        if !self.sessions.lock().unwrap().get(session).is_some_and(|e| e.info.state.is_running()) {
            return Err(ErrorBody::new(ErrorCode::NotFound, format!("실행 중인 세션이 아니다: {session}")));
        }
        let dir = SessionPaths::new(&self.state, session).dir;
        let att = att.to_string();
        tokio::task::spawn_blocking(move || attach::write(&dir, &att, offset, &data, eof))
            .await
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, e.to_string()))?
            .map(|r| match r {
                Some((path, sha256)) => AgentResponse::Attached { path, sha256 },
                None => AgentResponse::Done,
            })
            .map_err(|e| ErrorBody::new(ErrorCode::BadRequest, format!("{e:#}")))
    }

    /// 끝난 세션을 지울 때 pump가 떨어지기를 기다리는 한도
    pub(super) const FORGET_WAIT: Duration = Duration::from_secs(5);

    /// 사람이 붙인 이름을 `spec.json`에 적고(지우면 뺀다) 바뀐 정보를 host에 알린다. 끝난 세션에도 붙인다(다시 열면 그 이름).
    pub(super) fn machine_label(&self, name: &str) -> Result<AgentResponse, ErrorBody> {
        if !util::machine_name_ok(name) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "머신 이름 형식 오류"));
        }
        let mut label = self.name.lock().unwrap();
        let path = self.state.join("machine-label");
        if *label != name || !path.exists() {
            let temp = self.state.join("machine-label.tmp");
            std::fs::write(&temp, name).and_then(|_| std::fs::rename(&temp, &path)).map_err(|e| {
                ErrorBody::new(ErrorCode::Internal, format!("agent 이름표 저장 {}: {e}", path.display()))
            })?;
            *label = name.into();
        }
        Ok(AgentResponse::Done)
    }

    pub(super) async fn rename(&self, session: &str, label: Option<String>) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        let label = label.map(|l| l.trim().to_string()).filter(|l| !l.is_empty());
        if label
            .as_ref()
            .is_some_and(|l| l.chars().count() > aaa_proto::MAX_PANE_LABEL_CHARS || l.chars().any(char::is_control))
        {
            return Err(ErrorBody::new(
                ErrorCode::BadRequest,
                format!("이름은 제어 문자 없이 {}자 안이어야 한다", aaa_proto::MAX_PANE_LABEL_CHARS),
            ));
        }
        if !self.sessions.lock().unwrap().contains_key(session) {
            return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}")));
        }
        let paths = SessionPaths::new(&self.state, session);
        let mut spec: SpecFile = util::read_json(&paths.spec())
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("spec.json 읽기: {e:#}")))?;
        spec.label = label.clone();
        util::write_json(&paths.spec(), &spec)
            .map_err(|e| ErrorBody::new(ErrorCode::Internal, format!("spec.json 쓰기: {e:#}")))?;
        match &label {
            Some(l) => self.labels.lock().unwrap().insert(session.to_string(), l.clone()),
            None => self.labels.lock().unwrap().remove(session),
        };
        let info = {
            let mut map = self.sessions.lock().unwrap();
            let Some(e) = map.get_mut(session) else {
                return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}")));
            };
            e.info.name = spec.name().to_string();
            e.info.label = label;
            e.info.clone()
        };
        self.send_host(&LinkMsg::Event(AgentEvent::SessionUpdated(info.clone()))).await;
        Ok(AgentResponse::Session(info))
    }

    /// 끝난 세션 기록(폴더)을 지운다. 실행 중이면 거부한다.
    /// pump가 붙어 있으면 떨어질 때까지 기다렸다 지운다. 먼저 지우면 pump가 끝나며 보내는 종료 알림이 기록을 되살린다.
    /// session host는 종료를 알린 뒤에도 2초 더 응답하므로(늦게 붙는 agent도 마지막 화면을 받게) 그 연결은 여기서 닫는다:
    /// 종료는 이미 받았고 마지막 화면은 파일에 있다. 닫기를 기다리면 끝난 thread resolve·pane 닫기마다 2초가 걸렸다(t-0338, plan §8.160).
    pub(super) async fn forget(&self, session: &str) -> Result<AgentResponse, ErrorBody> {
        if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(ErrorBody::new(ErrorCode::BadRequest, "세션 ID 형식이 아니다"));
        }
        let deadline = tokio::time::Instant::now() + Self::FORGET_WAIT;
        let mut closing = false;
        loop {
            {
                let mut map = self.sessions.lock().unwrap();
                match map.get(session) {
                    None => return Err(ErrorBody::new(ErrorCode::NotFound, format!("세션이 없다: {session}"))),
                    Some(e) if e.info.state.is_running() => {
                        return Err(ErrorBody::new(ErrorCode::BadRequest, "실행 중인 세션은 지울 수 없다"));
                    }
                    Some(e) if e.pump.is_none() => {
                        map.remove(session);
                        break;
                    }
                    Some(e) if !closing => {
                        closing = e.pump.as_ref().is_some_and(|p| {
                            p.try_send(PumpCmd::Close)
                                .as_ref()
                                .inspect_err(|error| {
                                    aaa_common::diagnostic::observe(*error, "성공 여부만 쓰는 분기의 하위 오류")
                                })
                                .is_ok()
                        });
                    }
                    Some(_) => {}
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(ErrorBody::new(ErrorCode::BadRequest, "session host가 아직 닫히지 않았다"));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.harness_last.lock().unwrap().remove(session);
        self.labels.lock().unwrap().remove(session);
        self.progress.lock().unwrap().remove(session);
        self.agy_quota.lock().unwrap().remove(session);
        let paths = SessionPaths::new(&self.state, session);
        if let Err(e) = std::fs::remove_dir_all(&paths.dir) {
            tracing::warn!(%session, "세션 폴더 삭제 실패: {e}");
        }
        Ok(AgentResponse::Done)
    }
}
